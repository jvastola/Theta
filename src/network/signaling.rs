use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Error)]
pub enum SignalingError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("signaling server already running")]
    AlreadyRunning,
    #[error("peer {0} not found")]
    PeerNotFound(PeerId),
    #[error("peer {0} disconnected")]
    PeerDisconnected(PeerId),
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

    if let Some(id) = &peer_id {
        if let Some(room) = server.inner.state.remove_peer(id).await {
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
    }

    write_task.abort();
    let _ = write_task.await;
    Ok(())
}
