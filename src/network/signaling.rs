use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

type SignalingStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Error)]
pub enum SignalingError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("websocket error: {0}")]
    WebSocket(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid signaling url: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("signaling server already running")]
    AlreadyRunning,
    #[error("peer {0} not found")]
    PeerNotFound(PeerId),
    #[error("peer {0} disconnected")]
    PeerDisconnected(PeerId),
    #[error(
        "registration mismatch (expected {expected_peer} in {expected_room}, got {actual_peer} in {actual_room})"
    )]
    RegistrationMismatch {
        expected_peer: PeerId,
        expected_room: RoomId,
        actual_peer: PeerId,
        actual_room: RoomId,
    },
    #[error("signaling operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("unexpected signaling response: {0}")]
    UnexpectedResponse(String),
    #[error("signaling connection closed")]
    ConnectionClosed,
    #[error("signaling client not registered")]
    ClientNotRegistered,
}

impl From<tokio_tungstenite::tungstenite::Error> for SignalingError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        SignalingError::WebSocket(Box::new(err))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PeerId(pub String);

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoomId(pub String);

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDescription {
    pub sdp_type: String,
    pub sdp: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingRequest {
    Register { peer_id: PeerId, room_id: RoomId },
    Offer { to: PeerId, sdp: SessionDescription },
    Answer { to: PeerId, sdp: SessionDescription },
    IceCandidate { to: PeerId, candidate: IceCandidate },
    Heartbeat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingResponse {
    Registered {
        peer_id: PeerId,
        room_id: RoomId,
        peers_in_room: Vec<PeerId>,
    },
    Offer {
        from: PeerId,
        sdp: SessionDescription,
    },
    Answer {
        from: PeerId,
        sdp: SessionDescription,
    },
    IceCandidate {
        from: PeerId,
        candidate: IceCandidate,
    },
    PeerJoined {
        peer_id: PeerId,
    },
    PeerLeft {
        peer_id: PeerId,
    },
    Error {
        message: String,
    },
    HeartbeatAck,
}

struct PeerEntry {
    room: RoomId,
    sender: UnboundedSender<Message>,
}

struct ServerState {
    peers: RwLock<HashMap<PeerId, PeerEntry>>,
    rooms: RwLock<HashMap<RoomId, HashSet<PeerId>>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            rooms: RwLock::new(HashMap::new()),
        }
    }

    async fn register_peer(
        &self,
        peer_id: PeerId,
        room_id: RoomId,
        sender: UnboundedSender<Message>,
    ) -> Vec<PeerId> {
        let mut peers_guard = self.peers.write().await;
        peers_guard.insert(
            peer_id.clone(),
            PeerEntry {
                room: room_id.clone(),
                sender,
            },
        );
        drop(peers_guard);

        let mut rooms_guard = self.rooms.write().await;
        let entry = rooms_guard
            .entry(room_id.clone())
            .or_insert_with(HashSet::new);
        let mut existing = entry.iter().cloned().collect::<Vec<_>>();
        entry.insert(peer_id);
        existing.sort_by(|a, b| a.0.cmp(&b.0));
        existing
    }

    async fn remove_peer(&self, peer_id: &PeerId) -> Option<RoomId> {
        let room = {
            let mut peers_guard = self.peers.write().await;
            peers_guard.remove(peer_id).map(|entry| entry.room)
        }?;

        let mut rooms_guard = self.rooms.write().await;
        if let Some(peer_set) = rooms_guard.get_mut(&room) {
            peer_set.remove(peer_id);
            if peer_set.is_empty() {
                rooms_guard.remove(&room);
            }
        }
        Some(room)
    }

    async fn send_to_peer(
        &self,
        target: &PeerId,
        message: SignalingResponse,
    ) -> Result<(), SignalingError> {
        let payload = serde_json::to_string(&message)?;
        let peers_guard = self.peers.read().await;
        if let Some(entry) = peers_guard.get(target) {
            entry
                .sender
                .send(Message::Text(payload))
                .map_err(|_| SignalingError::PeerDisconnected(target.clone()))
        } else {
            Err(SignalingError::PeerNotFound(target.clone()))
        }
    }

    async fn broadcast_to_room(
        &self,
        room: &RoomId,
        message: SignalingResponse,
        skip: Option<&PeerId>,
    ) -> Result<(), SignalingError> {
        let payload = serde_json::to_string(&message)?;
        let peers_guard = self.peers.read().await;
        let rooms_guard = self.rooms.read().await;
        if let Some(peer_ids) = rooms_guard.get(room) {
            for peer_id in peer_ids {
                if skip.is_some() && skip.unwrap() == peer_id {
                    continue;
                }
                if let Some(entry) = peers_guard.get(peer_id) {
                    let _ = entry.sender.send(Message::Text(payload.clone()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct SignalingServer {
    inner: Arc<SignalingServerInner>,
}

pub struct SignalingHandle {
    server: SignalingServer,
    errors: Arc<StdMutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl SignalingHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    pub fn shutdown(&self) {
        self.server.shutdown();
        self.task.abort();
    }

    pub fn drain_errors(&self) -> Vec<String> {
        self.errors
            .lock()
            .map(|mut guard| guard.drain(..).collect())
            .unwrap_or_default()
    }
}

impl Drop for SignalingHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct SignalingClient {
    peer_id: PeerId,
    room_id: RoomId,
    writer: SplitSink<SignalingStream, Message>,
    reader: SplitStream<SignalingStream>,
    pending: VecDeque<SignalingResponse>,
    registered: bool,
}

impl SignalingClient {
    pub async fn connect(
        url: Url,
        peer_id: PeerId,
        room_id: RoomId,
    ) -> Result<Self, SignalingError> {
        let (stream, _response) = connect_async(url).await?;
        let (writer, reader) = stream.split();
        Ok(Self {
            peer_id,
            room_id,
            writer,
            reader,
            pending: VecDeque::new(),
            registered: false,
        })
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    pub async fn register(&mut self, timeout: Duration) -> Result<Vec<PeerId>, SignalingError> {
        self.send_request(SignalingRequest::Register {
            peer_id: self.peer_id.clone(),
            room_id: self.room_id.clone(),
        })
        .await?;

        loop {
            match self.receive_message(timeout).await {
                Ok(SignalingResponse::Registered {
                    peer_id,
                    room_id,
                    peers_in_room,
                }) => {
                    if peer_id != self.peer_id || room_id != self.room_id {
                        return Err(SignalingError::RegistrationMismatch {
                            expected_peer: self.peer_id.clone(),
                            expected_room: self.room_id.clone(),
                            actual_peer: peer_id,
                            actual_room: room_id,
                        });
                    }

                    self.registered = true;
                    return Ok(peers_in_room);
                }
                Ok(other) => {
                    self.pending.push_back(other);
                }
                Err(SignalingError::Timeout(_)) => return Err(SignalingError::Timeout(timeout)),
                Err(err) => return Err(err),
            }
        }
    }

    pub async fn send_offer(
        &mut self,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        self.ensure_registered()?;
        self.send_request(SignalingRequest::Offer {
            to: to.clone(),
            sdp,
        })
        .await
    }

    pub async fn send_answer(
        &mut self,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        self.ensure_registered()?;
        self.send_request(SignalingRequest::Answer {
            to: to.clone(),
            sdp,
        })
        .await
    }

    pub async fn send_ice_candidate(
        &mut self,
        to: &PeerId,
        candidate: IceCandidate,
    ) -> Result<(), SignalingError> {
        self.ensure_registered()?;
        self.send_request(SignalingRequest::IceCandidate {
            to: to.clone(),
            candidate,
        })
        .await
    }

    pub async fn next_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<SignalingResponse>, SignalingError> {
        if let Some(response) = self.pending.pop_front() {
            return Ok(Some(response));
        }

        match self.receive_message(timeout).await {
            Ok(response) => Ok(Some(response)),
            Err(SignalingError::Timeout(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn send_request(&mut self, request: SignalingRequest) -> Result<(), SignalingError> {
        let payload = serde_json::to_string(&request)?;
        self.writer.send(Message::Text(payload)).await?;
        Ok(())
    }

    fn ensure_registered(&self) -> Result<(), SignalingError> {
        if self.registered {
            Ok(())
        } else {
            Err(SignalingError::ClientNotRegistered)
        }
    }

    async fn receive_message(
        &mut self,
        timeout: Duration,
    ) -> Result<SignalingResponse, SignalingError> {
        if let Some(response) = self.pending.pop_front() {
            return Ok(response);
        }

        loop {
            let next = time::timeout(timeout, self.reader.next())
                .await
                .map_err(|_| SignalingError::Timeout(timeout))?;

            let Some(message) = next else {
                return Err(SignalingError::ConnectionClosed);
            };

            let message = message?;
            match message {
                Message::Text(text) => {
                    let response: SignalingResponse = serde_json::from_str(&text)?;
                    return Ok(response);
                }
                Message::Binary(_) | Message::Frame(_) => {
                    return Err(SignalingError::UnexpectedResponse(
                        "binary or raw frame received".into(),
                    ));
                }
                Message::Ping(payload) => {
                    self.writer.send(Message::Pong(payload)).await?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Err(SignalingError::ConnectionClosed),
            }
        }
    }
}

struct SignalingServerInner {
    listener: TokioMutex<Option<TcpListener>>,
    local_addr: SocketAddr,
    state: Arc<ServerState>,
    shutdown: CancellationToken,
}

impl SignalingServer {
    pub async fn bind(addr: SocketAddr) -> Result<Self, SignalingError> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            inner: Arc::new(SignalingServerInner {
                listener: TokioMutex::new(Some(listener)),
                local_addr,
                state: Arc::new(ServerState::new()),
                shutdown: CancellationToken::new(),
            }),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr
    }

    pub fn shutdown(&self) {
        self.inner.shutdown.cancel();
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.shutdown.clone()
    }

    pub fn start(self) -> SignalingHandle {
        let errors = Arc::new(StdMutex::new(Vec::new()));
        let runner = self.clone();
        let error_sink = errors.clone();
        let task = tokio::spawn(async move {
            if let Err(err) = runner.run().await {
                log::error!("[signaling] server terminated: {err}");
                if let Ok(mut guard) = error_sink.lock() {
                    guard.push(err.to_string());
                }
            }
        });

        SignalingHandle {
            server: self,
            errors,
            task,
        }
    }

    pub async fn bind_and_start(addr: SocketAddr) -> Result<SignalingHandle, SignalingError> {
        let server = Self::bind(addr).await?;
        Ok(server.start())
    }

    pub async fn run(&self) -> Result<(), SignalingError> {
        let listener = {
            let mut guard = self.inner.listener.lock().await;
            guard.take().ok_or(SignalingError::AlreadyRunning)?
        };
        self.accept_loop(listener).await
    }

    async fn accept_loop(&self, listener: TcpListener) -> Result<(), SignalingError> {
        loop {
            tokio::select! {
                _ = self.inner.shutdown.cancelled() => {
                    break;
                }
                accept_result = listener.accept() => {
                    let (stream, _addr) = accept_result?;
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_connection(server, stream).await {
                            log::warn!("[signaling] connection error: {err}");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    pub async fn handle_offer(
        &self,
        from: &PeerId,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        self.inner
            .state
            .send_to_peer(
                to,
                SignalingResponse::Offer {
                    from: from.clone(),
                    sdp,
                },
            )
            .await
    }

    pub async fn handle_answer(
        &self,
        from: &PeerId,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        self.inner
            .state
            .send_to_peer(
                to,
                SignalingResponse::Answer {
                    from: from.clone(),
                    sdp,
                },
            )
            .await
    }

    pub async fn handle_ice_candidate(
        &self,
        from: &PeerId,
        to: &PeerId,
        candidate: IceCandidate,
    ) -> Result<(), SignalingError> {
        self.inner
            .state
            .send_to_peer(
                to,
                SignalingResponse::IceCandidate {
                    from: from.clone(),
                    candidate,
                },
            )
            .await
    }
}

async fn handle_connection(
    server: SignalingServer,
    stream: TcpStream,
) -> Result<(), SignalingError> {
    let ws_stream = accept_async(stream).await?;
    let (mut writer, mut reader) = ws_stream.split();
    let (sender, mut receiver): (UnboundedSender<Message>, UnboundedReceiver<Message>) =
        mpsc::unbounded_channel();

    let write_task = tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            if writer.send(message).await.is_err() {
                break;
            }
        }
    });

    let mut peer_id: Option<PeerId> = None;
    let mut _room_id: Option<RoomId> = None;
    let shutdown = server.cancellation_token();

    loop {
        let message = tokio::select! {
            _ = shutdown.cancelled() => {
                break;
            }
            next = reader.next() => next,
        };

        let Some(message) = message else {
            break;
        };

        let message = message?;
        match message {
            Message::Text(text) => {
                let request: SignalingRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(err) => {
                        let _ = sender.send(Message::Text(serde_json::to_string(
                            &SignalingResponse::Error {
                                message: format!("invalid payload: {err}"),
                            },
                        )?));
                        continue;
                    }
                };

                match request {
                    SignalingRequest::Register {
                        peer_id: incoming_id,
                        room_id: incoming_room,
                    } => {
                        if peer_id.is_some() {
                            let _ = sender.send(Message::Text(serde_json::to_string(
                                &SignalingResponse::Error {
                                    message: "peer already registered".into(),
                                },
                            )?));
                            continue;
                        }

                        let peers_in_room = server
                            .inner
                            .state
                            .register_peer(
                                incoming_id.clone(),
                                incoming_room.clone(),
                                sender.clone(),
                            )
                            .await;

                        let response = SignalingResponse::Registered {
                            peer_id: incoming_id.clone(),
                            room_id: incoming_room.clone(),
                            peers_in_room: peers_in_room.clone(),
                        };
                        let _ = sender.send(Message::Text(serde_json::to_string(&response)?));

                        server
                            .inner
                            .state
                            .broadcast_to_room(
                                &incoming_room,
                                SignalingResponse::PeerJoined {
                                    peer_id: incoming_id.clone(),
                                },
                                Some(&incoming_id),
                            )
                            .await?;

                        peer_id = Some(incoming_id);
                        _room_id = Some(incoming_room);
                    }
                    SignalingRequest::Offer { to, sdp } => {
                        let Some(from) = &peer_id else {
                            let _ = sender.send(Message::Text(serde_json::to_string(
                                &SignalingResponse::Error {
                                    message: "peer not registered".into(),
                                },
                            )?));
                            continue;
                        };
                        server.handle_offer(from, &to, sdp).await?;
                    }
                    SignalingRequest::Answer { to, sdp } => {
                        let Some(from) = &peer_id else {
                            let _ = sender.send(Message::Text(serde_json::to_string(
                                &SignalingResponse::Error {
                                    message: "peer not registered".into(),
                                },
                            )?));
                            continue;
                        };
                        server.handle_answer(from, &to, sdp).await?;
                    }
                    SignalingRequest::IceCandidate { to, candidate } => {
                        let Some(from) = &peer_id else {
                            let _ = sender.send(Message::Text(serde_json::to_string(
                                &SignalingResponse::Error {
                                    message: "peer not registered".into(),
                                },
                            )?));
                            continue;
                        };
                        server.handle_ice_candidate(from, &to, candidate).await?;
                    }
                    SignalingRequest::Heartbeat => {
                        let _ = sender.send(Message::Text(serde_json::to_string(
                            &SignalingResponse::HeartbeatAck,
                        )?));
                    }
                }
            }
            Message::Binary(_) => {
                let _ = sender.send(Message::Text(serde_json::to_string(
                    &SignalingResponse::Error {
                        message: "binary messages not supported".into(),
                    },
                )?));
            }
            Message::Frame(_) => {
                let _ = sender.send(Message::Text(serde_json::to_string(
                    &SignalingResponse::Error {
                        message: "frame messages not supported".into(),
                    },
                )?));
            }
            Message::Ping(payload) => {
                let _ = sender.send(Message::Pong(payload));
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                break;
            }
        }
    }

    if let Some(id) = &peer_id
        && let Some(room) = server.inner.state.remove_peer(id).await
    {
        let _ = server
            .inner
            .state
            .broadcast_to_room(
                &room,
                SignalingResponse::PeerLeft {
                    peer_id: id.clone(),
                },
                Some(id),
            )
            .await;
    }

    write_task.abort();
    let _ = write_task.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_url(addr: SocketAddr) -> Url {
        Url::parse(&format!("ws://{addr}/ws")).unwrap()
    }

    #[tokio::test]
    async fn signaling_clients_register_and_receive_events() -> Result<(), SignalingError> {
        let server = SignalingServer::bind("127.0.0.1:0".parse().unwrap()).await?;
        let addr = server.local_addr();
        let handle = server.start();

        let room = RoomId("room-a".into());
        let url = ws_url(addr);

        let mut alice =
            SignalingClient::connect(url.clone(), PeerId("alice".into()), room.clone()).await?;
        let peers = alice.register(Duration::from_secs(1)).await?;
        assert!(peers.is_empty());

        let mut bob =
            SignalingClient::connect(url.clone(), PeerId("bob".into()), room.clone()).await?;
        let peers = bob.register(Duration::from_secs(1)).await?;
        assert_eq!(peers, vec![PeerId("alice".into())]);

        let event = alice
            .next_event(Duration::from_secs(1))
            .await?
            .expect("peer joined event");
        match event {
            SignalingResponse::PeerJoined { peer_id } => assert_eq!(peer_id.0, "bob"),
            other => panic!("unexpected event: {other:?}"),
        }

        handle.shutdown();
        Ok(())
    }

    #[tokio::test]
    async fn signaling_offer_reaches_target_peer() -> Result<(), SignalingError> {
        let server = SignalingServer::bind("127.0.0.1:0".parse().unwrap()).await?;
        let addr = server.local_addr();
        let handle = server.start();

        let url = ws_url(addr);
        let room = RoomId("room-b".into());

        let mut alice =
            SignalingClient::connect(url.clone(), PeerId("alice".into()), room.clone()).await?;
        alice.register(Duration::from_secs(1)).await?;

        let mut bob =
            SignalingClient::connect(url.clone(), PeerId("bob".into()), room.clone()).await?;
        bob.register(Duration::from_secs(1)).await?;

        // Drain the peer-joined notification emitted when Bob registers.
        let joined_event = alice
            .next_event(Duration::from_secs(1))
            .await?
            .expect("peer joined event");
        match joined_event {
            SignalingResponse::PeerJoined { peer_id } => assert_eq!(peer_id.0, "bob"),
            other => panic!("unexpected event before offer: {other:?}"),
        }

        let offer = SessionDescription {
            sdp_type: "offer".into(),
            sdp: "v=0".into(),
        };
        bob.send_offer(&PeerId("alice".into()), offer.clone())
            .await?;

        let event = alice
            .next_event(Duration::from_secs(1))
            .await?
            .expect("offer event");
        match event {
            SignalingResponse::Offer { from, sdp } => {
                assert_eq!(from.0, "bob");
                assert_eq!(sdp.sdp, offer.sdp);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        handle.shutdown();
        Ok(())
    }
}
