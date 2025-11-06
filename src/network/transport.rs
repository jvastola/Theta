use super::{TransportDiagnostics, TransportKind, current_time_millis};
use crate::network::command_log::{CommandPacket, MAX_COMMAND_PACKET_BYTES};
use crate::network::voice::{VoiceDiagnosticsHandle, VoicePacket};
use crate::network::wire;
use bytes::Bytes;
use ed25519_dalek::SigningKey;
use flatbuffers::{FlatBufferBuilder, UnionWIPOffset, WIPOffset};
use quinn::{self, Connection, ReadExactError, ReadToEndError, RecvStream, SendStream};
use rand::{RngCore, rngs::OsRng};
use std::convert::TryInto;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{API, APIBuilder};
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::configuration::RTCConfiguration;

#[cfg(not(has_generated_network_schema))]
compile_error!(
    "network-quic feature requires FlatBuffers bindings. Run `cargo build` to generate them."
);

use wire::theta::net::{
    self, Compression, HeartbeatArgs, MessageBody, MessageEnvelopeArgs, PacketHeaderArgs,
    SessionAcknowledgeArgs, SessionHelloArgs,
};

const FRAME_HEADER_LEN: usize = 4;
const HANDSHAKE_CAPACITY: usize = 1024;
const FRAME_KIND_COMMAND_PACKET: u8 = 1;
const FRAME_KIND_COMPONENT_DELTA: u8 = 2;
const VOICE_FRAME_HEADER_BYTES: usize = 8 + 8 + 4;
const LOCAL_SPEAKER_TAG: &str = "local";
const REMOTE_SPEAKER_TAG: &str = "remote";

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("quinn connect error: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("quinn connection error: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("quinn write error: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("quinn read error: {0}")]
    Read(#[from] ReadExactError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("handshake error: {0}")]
    Handshake(String),
    #[error("timeout error: {0}")]
    Timeout(String),
    #[error("flatbuffer decode error: {0}")]
    Flatbuffers(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("quinn read_to_end error: {0}")]
    ReadToEnd(#[from] ReadToEndError),
    #[error("transport unsupported: {0}")]
    Unsupported(&'static str),
    #[error("webrtc error: {0}")]
    WebRtc(String),
}

#[cfg(test)]
mod webrtc_tests {
    use super::*;
    use crate::network::command_log::{
        AuthorId, CommandAuthor, CommandBatch, CommandEntry, CommandId, CommandPayload,
        CommandRole, CommandScope, ConflictStrategy,
    };

    #[tokio::test]
    async fn webrtc_transport_transfers_command_packets() {
        let (sender, receiver) = WebRtcTransport::pair()
            .await
            .expect("create loopback WebRTC pair");

        let author = CommandAuthor::new(AuthorId(9), CommandRole::Editor);
        let payload = CommandPayload::new("transport.test", CommandScope::Global, vec![1, 2, 3]);
        let entry = CommandEntry::new(
            CommandId::new(1, AuthorId(9)),
            99,
            payload,
            ConflictStrategy::LastWriteWins,
            author,
            None,
        );
        let batch = CommandBatch {
            sequence: 7,
            nonce: 11,
            timestamp_ms: 5_000,
            author: AuthorId(9),
            entries: vec![entry],
        };
        let packet = CommandPacket::from_batch(&batch).expect("serialize command batch");

        let send_packet = packet.clone();
        let send_task = tokio::spawn(async move {
            sender
                .send_command_packets(std::slice::from_ref(&send_packet))
                .await
                .expect("send over WebRTC");
            tokio::time::sleep(Duration::from_millis(100)).await;
            sender.close().await;
        });

        let metrics_handle = receiver.metrics_handle();
        let received = receiver
            .receive_command_packet(Duration::from_secs(2))
            .await
            .expect("receive command packet")
            .expect("packet present");
        assert_eq!(received.sequence, packet.sequence);
        assert_eq!(received.nonce, packet.nonce);

        let metrics = metrics_handle.latest().expect("metrics available");
        assert_eq!(metrics.kind, TransportKind::WebRtc);
        assert_eq!(metrics.command_packets_received, 1);

        receiver.close().await;
        send_task.await.expect("sender task completes");
    }

    #[tokio::test]
    async fn webrtc_transport_tolerates_empty_packet_list() {
        let (sender, receiver) = WebRtcTransport::pair()
            .await
            .expect("create loopback WebRTC pair");

        sender
            .send_command_packets(&[])
            .await
            .expect("sending empty slice succeeds");

        let packet = receiver
            .receive_command_packet(Duration::from_millis(100))
            .await
            .expect("receive completes");
        assert!(packet.is_none());

        receiver.close().await;
    }

    #[tokio::test]
    async fn webrtc_transport_drops_oversized_command_packet() {
        use crate::network::command_log::{
            AuthorId, CommandAuthor, CommandBatch, CommandEntry, CommandId, CommandPayload,
            CommandRole, CommandScope, ConflictStrategy,
        };

        let (sender, receiver) = WebRtcTransport::pair()
            .await
            .expect("create loopback WebRTC pair");

        let author = CommandAuthor::new(AuthorId(11), CommandRole::Editor);

        let oversized_packet = CommandPacket {
            sequence: 1,
            nonce: 100,
            timestamp_ms: current_time_millis(),
            payload: vec![0u8; MAX_COMMAND_PACKET_BYTES + 1],
        };

        let valid_entry = CommandEntry::new(
            CommandId::new(3, AuthorId(11)),
            current_time_millis(),
            CommandPayload::new("editor.selection", CommandScope::Global, vec![9, 8, 7]),
            ConflictStrategy::Merge,
            author.clone(),
            None,
        );
        let batch = CommandBatch {
            sequence: 5,
            nonce: 101,
            timestamp_ms: current_time_millis(),
            author: author.id.clone(),
            entries: vec![valid_entry],
        };
        let valid_packet = CommandPacket::from_batch(&batch).expect("serialize valid packet");

        let err = sender
            .send_command_packets(&[oversized_packet])
            .await
            .expect_err("oversized payload should error");
        if let TransportError::WebRtc(msg) = &err {
            assert!(msg.contains("maximum message size"));
        } else {
            panic!("expected WebRTC error, got {err:?}");
        }

        sender
            .send_command_packets(&[valid_packet])
            .await
            .expect("small packet sends successfully");

        let received = receiver
            .receive_command_packet(Duration::from_secs(1))
            .await
            .expect("receive completes")
            .expect("valid packet available");
        assert_eq!(received.sequence, 5);

        sender.close().await;
        receiver.close().await;
    }
}

#[cfg(test)]
mod handshake_unit_tests {
    use super::*;

    #[test]
    fn session_hello_rejects_empty_nonce() {
        let message = build_session_hello(1, 0xABCDu64, &[], &[], None, &[0u8; 32]);
        let result = parse_session_hello(&message, 1, 0xABCDu64);
        assert!(matches!(
            result,
            Err(TransportError::Handshake(msg)) if msg.contains("client nonce")
        ));
    }

    #[test]
    fn session_ack_rejects_empty_nonce() {
        let message = build_session_ack(1, 0xABCDu64, &[], 42, 7, &[], &[0u8; 32]);
        let result = parse_session_ack(&message, 1, 0xABCDu64);
        assert!(matches!(
            result,
            Err(TransportError::Handshake(msg)) if msg.contains("server nonce")
        ));
    }
}

struct FramedStream {
    send: Arc<TokioMutex<SendStream>>,
    recv: Arc<TokioMutex<RecvStream>>,
}

impl FramedStream {
    fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send: Arc::new(TokioMutex::new(send)),
            recv: Arc::new(TokioMutex::new(recv)),
        }
    }

    async fn write_frame(&self, payload: &[u8]) -> Result<(), TransportError> {
        let mut guard = self.send.lock().await;
        write_frame_raw(&mut guard, payload).await
    }

    async fn read_frame(&self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut guard = self.recv.lock().await;
        read_frame_raw(&mut guard, timeout).await
    }

    #[allow(dead_code)]
    async fn write_all(&self, payload: &[u8]) -> Result<(), TransportError> {
        let mut guard = self.send.lock().await;
        guard.write_all(payload).await?;
        Ok(())
    }

    #[allow(dead_code)]
    async fn finish_send(&self) -> Result<(), TransportError> {
        let mut guard = self.send.lock().await;
        guard.finish().await?;
        Ok(())
    }

    #[allow(dead_code)]
    async fn read_to_end(&self, limit: usize) -> Result<Vec<u8>, TransportError> {
        let mut guard = self.recv.lock().await;
        let bytes = guard.read_to_end(limit).await?;
        Ok(bytes.to_vec())
    }
}

#[derive(Clone)]
pub struct TransportMetricsHandle {
    inner: Arc<Mutex<TransportMetricsInner>>,
}

#[derive(Default)]
struct TransportMetricsInner {
    latest: Option<TransportDiagnostics>,
}

impl TransportMetricsHandle {
    pub fn new() -> Self {
        let inner = TransportMetricsInner {
            latest: Some(TransportDiagnostics::default()),
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn latest(&self) -> Option<TransportDiagnostics> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.latest.clone())
    }

    fn update(&self, apply: impl FnOnce(&mut TransportDiagnostics)) {
        if let Ok(mut guard) = self.inner.lock() {
            let metrics = guard
                .latest
                .get_or_insert_with(TransportDiagnostics::default);
            apply(metrics);
        }
    }
}

impl Default for TransportMetricsHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct HeartbeatConfig {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(500),
            timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClientHandshake {
    pub protocol_version: u32,
    pub schema_hash: u64,
    pub capabilities: Vec<u32>,
    pub auth_token: Option<String>,
    pub signing_key: SigningKey,
    pub heartbeat: HeartbeatConfig,
    pub server_name: String,
}

#[derive(Clone, Debug)]
pub struct ServerHandshake {
    pub protocol_version: u32,
    pub schema_hash: u64,
    pub capabilities: Vec<u32>,
    pub signing_key: SigningKey,
    pub heartbeat: HeartbeatConfig,
}

#[derive(Debug, Clone)]
pub struct HandshakeSummary {
    pub session_id: u64,
    pub assigned_role: u32,
    pub capability_mask: Vec<u32>,
    pub client_public_key: [u8; 32],
    pub server_public_key: [u8; 32],
    pub client_nonce: Vec<u8>,
    pub server_nonce: Vec<u8>,
}

#[allow(dead_code)]
pub struct TransportSession {
    connection: Connection,
    control_send: Arc<TokioMutex<SendStream>>,
    replication: FramedStream,
    assets: FramedStream,
    metrics: TransportMetricsHandle,
    heartbeat: HeartbeatActor,
    handshake: HandshakeSummary,
}

impl TransportSession {
    pub fn metrics_handle(&self) -> TransportMetricsHandle {
        self.metrics.clone()
    }

    pub fn handshake(&self) -> &HandshakeSummary {
        &self.handshake
    }

    pub fn kind(&self) -> TransportKind {
        TransportKind::Quic
    }
    pub async fn send_command_packets(
        &self,
        packets: &[CommandPacket],
    ) -> Result<(), TransportError> {
        if packets.is_empty() {
            return Ok(());
        }

        let send_start = Instant::now();
        let mut total_bytes = 0usize;
        for packet in packets {
            let frame = encode_command_packet_frame(packet)?;
            total_bytes = total_bytes.saturating_add(frame.len());
            self.replication.write_frame(&frame).await?;
        }

        let sent = packets.len() as u64;
        let elapsed = send_start.elapsed().as_secs_f32();
        self.metrics.update(|m| {
            m.kind = TransportKind::Quic;
            m.packets_sent = m.packets_sent.saturating_add(sent);
            m.command_packets_sent = m.command_packets_sent.saturating_add(sent);
            if sent > 0 {
                m.compression_ratio = 1.0;
            }
            if total_bytes > 0 {
                let bandwidth = if elapsed > 0.0 {
                    total_bytes as f32 / elapsed
                } else {
                    total_bytes as f32
                };
                m.command_bandwidth_bytes_per_sec = bandwidth;
                m.command_latency_ms = elapsed * 1000.0;
            }
        });

        Ok(())
    }

    pub async fn receive_command_packet(
        &self,
        timeout: Duration,
    ) -> Result<Option<CommandPacket>, TransportError> {
        loop {
            let frame = match self.replication.read_frame(timeout).await {
                Ok(bytes) => bytes,
                Err(TransportError::Timeout(_)) => return Ok(None),
                Err(err) => return Err(err),
            };

            match decode_replication_frame(&frame) {
                Ok(DecodedReplicationFrame::Command(packet)) => {
                    if packet.payload.len() > MAX_COMMAND_PACKET_BYTES {
                        log::warn!(
                            "[transport] dropping oversized command packet {} ({} bytes)",
                            packet.sequence,
                            packet.payload.len()
                        );
                        continue;
                    }
                    let latency_ms =
                        (current_time_millis().saturating_sub(packet.timestamp_ms)) as f32;
                    self.metrics.update(|m| {
                        let previous_latency = m.command_latency_ms;
                        m.kind = TransportKind::Quic;
                        m.packets_received = m.packets_received.saturating_add(1);
                        m.command_packets_received = m.command_packets_received.saturating_add(1);
                        m.compression_ratio = 1.0;
                        m.command_bandwidth_bytes_per_sec = frame.len() as f32;
                        m.command_latency_ms = latency_ms;
                        let delta = (latency_ms - previous_latency).abs();
                        m.jitter_ms = (m.jitter_ms * 0.8) + (delta * 0.2);
                        m.rtt_ms = latency_ms;
                    });
                    return Ok(Some(packet));
                }
                Ok(DecodedReplicationFrame::ComponentDelta(bytes)) => {
                    log::debug!(
                        "[transport] received component delta ({} bytes) while awaiting command",
                        bytes.len()
                    );
                    continue;
                }
                Ok(DecodedReplicationFrame::Unknown(kind, payload)) => {
                    log::warn!(
                        "[transport] ignoring unknown replication frame kind {} ({} bytes)",
                        kind,
                        payload.len()
                    );
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub async fn close(self) {
        self.connection.close(0u32.into(), b"normal shutdown");
    }
}

pub struct WebRtcTransport {
    peer: Arc<RTCPeerConnection>,
    command_channel: Arc<RTCDataChannel>,
    command_inbox: Arc<TokioMutex<UnboundedReceiver<Vec<u8>>>>,
    _command_inbox_sender: Arc<UnboundedSender<Vec<u8>>>,
    voice_channel: Option<Arc<RTCDataChannel>>,
    voice_inbox: Option<Arc<TokioMutex<UnboundedReceiver<Vec<u8>>>>>,
    _voice_inbox_sender: Option<Arc<UnboundedSender<Vec<u8>>>>,
    metrics: TransportMetricsHandle,
    voice_metrics: VoiceDiagnosticsHandle,
    voice_last_latency_ms: Arc<Mutex<Option<f32>>>,
}

impl WebRtcTransport {
    pub fn from_parts(
        peer: Arc<RTCPeerConnection>,
        command_channel: Arc<RTCDataChannel>,
        voice_channel: Option<Arc<RTCDataChannel>>,
    ) -> Self {
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let command_sender = Arc::new(command_tx);
        let command_inbox = Arc::new(TokioMutex::new(command_rx));
        let command_msg_sender = Arc::clone(&command_sender);

        let metrics = TransportMetricsHandle::new();
        metrics.update(|m| {
            m.kind = TransportKind::WebRtc;
            if m.compression_ratio == 0.0 {
                m.compression_ratio = 1.0;
            }
        });

        let voice_metrics = VoiceDiagnosticsHandle::new();
        let voice_last_latency_ms = Arc::new(Mutex::new(None));

        let metrics_for_open = metrics.clone();
        let command_channel_for_open = Arc::clone(&command_channel);
        command_channel_for_open.on_open(Box::new(move || {
            let metrics = metrics_for_open.clone();
            metrics.update(|m| m.kind = TransportKind::WebRtc);
            Box::pin(async {})
        }));

        let metrics_for_close = metrics.clone();
        let command_channel_for_close = Arc::clone(&command_channel);
        command_channel_for_close.on_close(Box::new(move || {
            let metrics = metrics_for_close.clone();
            metrics.update(|m| m.kind = TransportKind::WebRtc);
            Box::pin(async {})
        }));

        let metrics_for_state = metrics.clone();
        peer.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let metrics = metrics_for_state.clone();
            Box::pin(async move {
                metrics.update(|m| {
                    m.kind = TransportKind::WebRtc;
                    if matches!(
                        state,
                        RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed
                    ) {
                        m.command_latency_ms = 0.0;
                    }
                });
            })
        }));

        let command_channel_for_messages = Arc::clone(&command_channel);
        command_channel_for_messages.on_message(Box::new(move |msg: DataChannelMessage| {
            let sender = command_msg_sender.clone();
            let payload = msg.data.to_vec();
            let _ = sender.send(payload);
            Box::pin(async {})
        }));

        let (voice_inbox, voice_sender_arc) = if let Some(channel) = voice_channel.as_ref() {
            let (voice_tx, voice_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let inbox = Arc::new(TokioMutex::new(voice_rx));
            let sender = Arc::new(voice_tx);

            let metrics_for_open = voice_metrics.clone();
            let voice_channel_for_open = Arc::clone(channel);
            voice_channel_for_open.on_open(Box::new(move || {
                let metrics = metrics_for_open.clone();
                metrics.update(|m| {
                    if !m.active_speakers.contains(&"local".to_string()) {
                        m.active_speakers.push("local".to_string());
                    }
                });
                Box::pin(async {})
            }));

            let metrics_for_close = voice_metrics.clone();
            let voice_channel_for_close = Arc::clone(channel);
            voice_channel_for_close.on_close(Box::new(move || {
                let metrics = metrics_for_close.clone();
                metrics.update(|m| {
                    m.active_speakers.retain(|speaker| speaker != "local");
                });
                Box::pin(async {})
            }));

            let message_sender = Arc::clone(&sender);
            channel.on_message(Box::new(move |msg: DataChannelMessage| {
                let sender = message_sender.clone();
                let _ = sender.send(msg.data.to_vec());
                Box::pin(async {})
            }));

            (Some(inbox), Some(sender))
        } else {
            (None, None)
        };

        Self {
            peer,
            command_channel,
            command_inbox,
            _command_inbox_sender: command_sender,
            voice_channel,
            voice_inbox,
            _voice_inbox_sender: voice_sender_arc,
            metrics,
            voice_metrics,
            voice_last_latency_ms,
        }
    }

    pub fn metrics_handle(&self) -> TransportMetricsHandle {
        self.metrics.update(|m| m.kind = TransportKind::WebRtc);
        self.metrics.clone()
    }

    pub fn voice_metrics_handle(&self) -> Option<VoiceDiagnosticsHandle> {
        self.voice_channel
            .as_ref()
            .map(|_| self.voice_metrics.clone())
    }

    pub fn kind(&self) -> TransportKind {
        TransportKind::WebRtc
    }

    pub async fn send_command_packets(
        &self,
        packets: &[CommandPacket],
    ) -> Result<(), TransportError> {
        if packets.is_empty() {
            return Ok(());
        }

        let send_start = Instant::now();
        let mut total_bytes = 0usize;

        for packet in packets {
            let frame = encode_command_packet_frame(packet)?;
            let frame_len = frame.len();
            total_bytes = total_bytes.saturating_add(frame_len);
            let payload = Bytes::from(frame);
            self.command_channel
                .send(&payload)
                .await
                .map_err(|err| TransportError::WebRtc(err.to_string()))?;
        }

        let sent = packets.len() as u64;
        let elapsed = send_start.elapsed().as_secs_f32();
        self.metrics.update(|m| {
            m.kind = TransportKind::WebRtc;
            m.packets_sent = m.packets_sent.saturating_add(sent);
            m.command_packets_sent = m.command_packets_sent.saturating_add(sent);
            if sent > 0 {
                m.compression_ratio = m.compression_ratio.max(1.0);
            }
            if total_bytes > 0 {
                let bandwidth = if elapsed > 0.0 {
                    total_bytes as f32 / elapsed
                } else {
                    total_bytes as f32
                };
                m.command_bandwidth_bytes_per_sec = bandwidth;
                m.command_latency_ms = elapsed * 1000.0;
            }
        });

        Ok(())
    }

    pub async fn receive_command_packet(
        &self,
        timeout: Duration,
    ) -> Result<Option<CommandPacket>, TransportError> {
        loop {
            let frame = {
                let mut guard = self.command_inbox.lock().await;
                match tokio::time::timeout(timeout, guard.recv()).await {
                    Ok(Some(frame)) => frame,
                    Ok(None) => return Ok(None),
                    Err(_) => return Ok(None),
                }
            };

            match decode_replication_frame(&frame) {
                Ok(DecodedReplicationFrame::Command(packet)) => {
                    if packet.payload.len() > MAX_COMMAND_PACKET_BYTES {
                        log::warn!(
                            "[transport] dropping oversized command packet {} ({} bytes) via WebRTC",
                            packet.sequence,
                            packet.payload.len()
                        );
                        continue;
                    }
                    let latency_ms =
                        (current_time_millis().saturating_sub(packet.timestamp_ms)) as f32;
                    self.metrics.update(|m| {
                        let previous_latency = m.command_latency_ms;
                        m.kind = TransportKind::WebRtc;
                        m.packets_received = m.packets_received.saturating_add(1);
                        m.command_packets_received = m.command_packets_received.saturating_add(1);
                        m.compression_ratio = m.compression_ratio.max(1.0);
                        m.command_bandwidth_bytes_per_sec = frame.len() as f32;
                        m.command_latency_ms = latency_ms;
                        let delta = (latency_ms - previous_latency).abs();
                        m.jitter_ms = (m.jitter_ms * 0.8) + (delta * 0.2);
                        m.rtt_ms = latency_ms;
                    });
                    return Ok(Some(packet));
                }
                Ok(DecodedReplicationFrame::ComponentDelta(bytes)) => {
                    log::debug!(
                        "[transport] received component delta over WebRTC ({} bytes) while awaiting command",
                        bytes.len()
                    );
                    continue;
                }
                Ok(DecodedReplicationFrame::Unknown(kind, payload)) => {
                    log::warn!(
                        "[transport] ignoring unknown WebRTC frame kind {} ({} bytes)",
                        kind,
                        payload.len()
                    );
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub async fn send_voice_packet(&self, packet: &VoicePacket) -> Result<(), TransportError> {
        let channel = self
            .voice_channel
            .as_ref()
            .ok_or(TransportError::Unsupported("voice channel not negotiated"))?;

        let encoded = Self::encode_voice_packet(packet);
        let payload_len = encoded.len() as u64;
        let payload = Bytes::from(encoded);

        channel
            .send(&payload)
            .await
            .map_err(|err| TransportError::WebRtc(err.to_string()))?;

        self.voice_metrics.update(|m| {
            m.packets_sent = m.packets_sent.saturating_add(1);
            m.bytes_sent = m.bytes_sent.saturating_add(payload_len);
            m.voiced_frames = m.voiced_frames.saturating_add(1);
            let bits = m.bytes_sent as f32 * 8.0;
            m.bitrate_kbps = bits / 1000.0;
            if !m
                .active_speakers
                .iter()
                .any(|speaker| speaker == LOCAL_SPEAKER_TAG)
            {
                m.active_speakers.push(LOCAL_SPEAKER_TAG.to_string());
            }
        });

        Ok(())
    }

    pub async fn receive_voice_packet(
        &self,
        timeout: Duration,
    ) -> Result<Option<VoicePacket>, TransportError> {
        let inbox = match &self.voice_inbox {
            Some(inbox) => inbox,
            None => return Ok(None),
        };

        let frame = {
            let mut guard = inbox.lock().await;
            match tokio::time::timeout(timeout, guard.recv()).await {
                Ok(Some(frame)) => frame,
                Ok(None) => return Ok(None),
                Err(_) => return Ok(None),
            }
        };

        let packet = Self::decode_voice_packet(&frame)?;
        let now = current_time_millis();
        let latency_ms = (now.saturating_sub(packet.timestamp_ms)) as f32;

        let jitter_delta = if let Ok(mut guard) = self.voice_last_latency_ms.lock() {
            let previous = *guard;
            *guard = Some(latency_ms);
            previous.map(|prev| (latency_ms - prev).abs())
        } else {
            None
        };

        self.voice_metrics.update(|m| {
            m.packets_received = m.packets_received.saturating_add(1);
            m.bytes_received = m.bytes_received.saturating_add(frame.len() as u64);
            m.latency_ms = latency_ms;
            if let Some(delta) = jitter_delta {
                m.jitter_ms = (m.jitter_ms * 0.8) + (delta * 0.2);
            }
            if !m
                .active_speakers
                .iter()
                .any(|speaker| speaker == REMOTE_SPEAKER_TAG)
            {
                m.active_speakers.push(REMOTE_SPEAKER_TAG.to_string());
            }
        });

        Ok(Some(packet))
    }

    fn encode_voice_packet(packet: &VoicePacket) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(VOICE_FRAME_HEADER_BYTES + packet.payload.len());
        bytes.extend_from_slice(&packet.sequence.to_le_bytes());
        bytes.extend_from_slice(&packet.timestamp_ms.to_le_bytes());
        bytes.extend_from_slice(&(packet.payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&packet.payload);
        bytes
    }

    fn decode_voice_packet(bytes: &[u8]) -> Result<VoicePacket, TransportError> {
        if bytes.len() < VOICE_FRAME_HEADER_BYTES {
            return Err(TransportError::Protocol(
                "voice packet shorter than header".into(),
            ));
        }

        let sequence = u64::from_le_bytes(
            bytes[0..8]
                .try_into()
                .map_err(|_| TransportError::Protocol("invalid voice sequence bytes".into()))?,
        );
        let timestamp_ms = u64::from_le_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| TransportError::Protocol("invalid voice timestamp bytes".into()))?,
        );
        let payload_len = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .map_err(|_| TransportError::Protocol("invalid voice payload length".into()))?,
        ) as usize;

        if bytes.len() < VOICE_FRAME_HEADER_BYTES + payload_len {
            return Err(TransportError::Protocol(
                "voice packet truncated payload".into(),
            ));
        }

        let payload =
            bytes[VOICE_FRAME_HEADER_BYTES..VOICE_FRAME_HEADER_BYTES + payload_len].to_vec();
        Ok(VoicePacket::new(sequence, timestamp_ms, payload))
    }

    pub async fn close(self) {
        if let Err(err) = self.command_channel.close().await {
            log::warn!("[transport] failed to close WebRTC data channel: {err}");
        }
        if let Some(channel) = &self.voice_channel
            && let Err(err) = channel.close().await
        {
            log::warn!("[transport] failed to close WebRTC voice channel: {err}");
        }
        if let Err(err) = self.peer.close().await {
            log::warn!("[transport] failed to close WebRTC peer connection: {err}");
        }
    }

    pub async fn pair() -> Result<(Self, Self), TransportError> {
        create_loopback_webrtc_pair().await
    }
}

fn build_webrtc_api() -> Result<API, TransportError> {
    let mut media_engine = MediaEngine::default();

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine)
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;

    Ok(APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build())
}

async fn create_loopback_webrtc_pair() -> Result<(WebRtcTransport, WebRtcTransport), TransportError>
{
    let api = build_webrtc_api()?;

    let offer_pc = Arc::new(
        api.new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|err| TransportError::WebRtc(err.to_string()))?,
    );
    let answer_pc = Arc::new(
        api.new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|err| TransportError::WebRtc(err.to_string()))?,
    );

    let command_channel = offer_pc
        .create_data_channel(
            "theta-command",
            Some(RTCDataChannelInit {
                ordered: Some(true),
                ..Default::default()
            }),
        )
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;
    let voice_channel = offer_pc
        .create_data_channel(
            "theta-voice",
            Some(RTCDataChannelInit {
                ordered: Some(false),
                ..Default::default()
            }),
        )
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;

    struct RemoteChannelWaiters {
        command: Option<oneshot::Sender<Arc<RTCDataChannel>>>,
        voice: Option<oneshot::Sender<Arc<RTCDataChannel>>>,
    }

    let (remote_command_tx, remote_command_rx) = oneshot::channel::<Arc<RTCDataChannel>>();
    let (remote_voice_tx, remote_voice_rx) = oneshot::channel::<Arc<RTCDataChannel>>();
    let remote_waiters = Arc::new(TokioMutex::new(RemoteChannelWaiters {
        command: Some(remote_command_tx),
        voice: Some(remote_voice_tx),
    }));
    answer_pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let waiters = remote_waiters.clone();
        Box::pin(async move {
            let label = dc.label().to_string();
            let mut waiters = waiters.lock().await;
            match label.as_str() {
                "theta-command" => {
                    if let Some(tx) = waiters.command.take() {
                        let _ = tx.send(dc);
                    }
                }
                "theta-voice" => {
                    if let Some(tx) = waiters.voice.take() {
                        let _ = tx.send(dc);
                    }
                }
                other => {
                    log::warn!("[webrtc] received unexpected data channel: {other}");
                }
            }
        })
    }));

    forward_ice_candidates(&offer_pc, Arc::clone(&answer_pc));
    forward_ice_candidates(&answer_pc, Arc::clone(&offer_pc));

    let offer = offer_pc
        .create_offer(None)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;
    offer_pc
        .set_local_description(offer)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;
    let offer_desc = offer_pc
        .local_description()
        .await
        .ok_or_else(|| TransportError::WebRtc("missing local offer description".into()))?;

    answer_pc
        .set_remote_description(offer_desc)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;

    let answer = answer_pc
        .create_answer(None)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;
    answer_pc
        .set_local_description(answer)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;
    let answer_desc = answer_pc
        .local_description()
        .await
        .ok_or_else(|| TransportError::WebRtc("missing local answer description".into()))?;

    offer_pc
        .set_remote_description(answer_desc)
        .await
        .map_err(|err| TransportError::WebRtc(err.to_string()))?;

    let remote_command_channel = remote_command_rx
        .await
        .map_err(|_| TransportError::WebRtc("command channel handshake failed".into()))?;
    let remote_voice_channel = remote_voice_rx
        .await
        .map_err(|_| TransportError::WebRtc("voice channel handshake failed".into()))?;

    wait_for_channel_open(&command_channel).await;
    wait_for_channel_open(&remote_command_channel).await;
    wait_for_channel_open(&voice_channel).await;
    wait_for_channel_open(&remote_voice_channel).await;

    let offer_transport =
        WebRtcTransport::from_parts(Arc::clone(&offer_pc), command_channel, Some(voice_channel));
    let answer_transport = WebRtcTransport::from_parts(
        answer_pc,
        remote_command_channel,
        Some(remote_voice_channel),
    );

    Ok((offer_transport, answer_transport))
}

fn forward_ice_candidates(source: &Arc<RTCPeerConnection>, destination: Arc<RTCPeerConnection>) {
    let (candidate_tx, mut candidate_rx) =
        tokio::sync::mpsc::unbounded_channel::<RTCIceCandidateInit>();
    source.on_ice_candidate(Box::new(move |candidate| {
        let tx = candidate_tx.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate
                && let Ok(json) = candidate.to_json()
            {
                let _ = tx.send(json);
            }
        })
    }));

    tokio::spawn(async move {
        while let Some(candidate) = candidate_rx.recv().await {
            if let Err(err) = destination.add_ice_candidate(candidate).await {
                log::warn!("[webrtc] failed to add ICE candidate: {err}");
            }
        }
    });
}

async fn wait_for_channel_open(channel: &Arc<RTCDataChannel>) {
    for _ in 0..500 {
        if channel.ready_state() == RTCDataChannelState::Open {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    log::warn!("[webrtc] data channel did not reach open state within expected window");
}

pub enum CommandTransport {
    Quic(TransportSession),
    WebRtc(WebRtcTransport),
}

impl CommandTransport {
    pub fn kind(&self) -> TransportKind {
        match self {
            CommandTransport::Quic(_) => TransportKind::Quic,
            CommandTransport::WebRtc(transport) => transport.kind(),
        }
    }

    pub fn metrics_handle(&self) -> TransportMetricsHandle {
        match self {
            CommandTransport::Quic(session) => session.metrics_handle(),
            CommandTransport::WebRtc(transport) => transport.metrics_handle(),
        }
    }

    pub fn voice_metrics_handle(&self) -> Option<VoiceDiagnosticsHandle> {
        match self {
            CommandTransport::Quic(_) => None,
            CommandTransport::WebRtc(transport) => transport.voice_metrics_handle(),
        }
    }

    pub async fn send_command_packets(
        &self,
        packets: &[CommandPacket],
    ) -> Result<(), TransportError> {
        match self {
            CommandTransport::Quic(session) => session.send_command_packets(packets).await,
            CommandTransport::WebRtc(transport) => transport.send_command_packets(packets).await,
        }
    }

    pub async fn receive_command_packet(
        &self,
        timeout: Duration,
    ) -> Result<Option<CommandPacket>, TransportError> {
        match self {
            CommandTransport::Quic(session) => session.receive_command_packet(timeout).await,
            CommandTransport::WebRtc(transport) => transport.receive_command_packet(timeout).await,
        }
    }

    pub async fn send_voice_packet(&self, packet: &VoicePacket) -> Result<(), TransportError> {
        match self {
            CommandTransport::Quic(_) => Err(TransportError::Unsupported(
                "voice channel not available on QUIC transport",
            )),
            CommandTransport::WebRtc(transport) => transport.send_voice_packet(packet).await,
        }
    }

    pub async fn receive_voice_packet(
        &self,
        timeout: Duration,
    ) -> Result<Option<VoicePacket>, TransportError> {
        match self {
            CommandTransport::Quic(_) => Ok(None),
            CommandTransport::WebRtc(transport) => transport.receive_voice_packet(timeout).await,
        }
    }

    pub async fn close(self) {
        match self {
            CommandTransport::Quic(session) => session.close().await,
            CommandTransport::WebRtc(transport) => transport.close().await,
        }
    }
}

impl From<TransportSession> for CommandTransport {
    fn from(value: TransportSession) -> Self {
        CommandTransport::Quic(value)
    }
}

impl From<WebRtcTransport> for CommandTransport {
    fn from(value: WebRtcTransport) -> Self {
        CommandTransport::WebRtc(value)
    }
}

struct HeartbeatActor {
    sender: JoinHandle<()>,
    receiver: JoinHandle<()>,
}

impl Drop for HeartbeatActor {
    fn drop(&mut self) {
        self.sender.abort();
        self.receiver.abort();
    }
}

pub async fn connect(
    endpoint: &quinn::Endpoint,
    server_addr: SocketAddr,
    handshake: ClientHandshake,
) -> Result<TransportSession, TransportError> {
    let connecting = endpoint.connect(server_addr, &handshake.server_name)?;
    let connection = connecting.await?;
    establish_client_session(connection, handshake).await
}

pub async fn accept(
    connecting: quinn::Connecting,
    handshake: ServerHandshake,
) -> Result<TransportSession, TransportError> {
    let connection = connecting.await?;
    establish_server_session(connection, handshake).await
}

async fn establish_client_session(
    connection: Connection,
    handshake: ClientHandshake,
) -> Result<TransportSession, TransportError> {
    let (mut control_send, mut control_recv) = connection.open_bi().await?;
    let client_nonce = random_nonce();
    let client_public_key = handshake.signing_key.verifying_key().to_bytes();

    let hello_bytes = build_session_hello(
        handshake.protocol_version,
        handshake.schema_hash,
        &client_nonce,
        &handshake.capabilities,
        handshake.auth_token.as_deref(),
        &client_public_key,
    );

    write_frame_raw(&mut control_send, &hello_bytes).await?;

    let ack_bytes = read_frame_raw(&mut control_recv, handshake.heartbeat.timeout).await?;
    let ack = parse_session_ack(
        &ack_bytes,
        handshake.protocol_version,
        handshake.schema_hash,
    )?;

    let replication = connection.open_bi().await?;
    let assets = connection.open_bi().await?;

    let metrics = TransportMetricsHandle::new();
    metrics.update(|m| {
        m.kind = TransportKind::Quic;
        m.packets_sent = m.packets_sent.saturating_add(1);
        m.packets_received = m.packets_received.saturating_add(1);
        m.compression_ratio = 1.0;
    });

    let control_send_arc = Arc::new(TokioMutex::new(control_send));
    let heartbeat = HeartbeatActor::spawn(
        control_send_arc.clone(),
        control_recv,
        metrics.clone(),
        handshake.heartbeat,
    );

    Ok(TransportSession {
        connection,
        control_send: control_send_arc,
        replication: FramedStream::new(replication.0, replication.1),
        assets: FramedStream::new(assets.0, assets.1),
        metrics,
        heartbeat,
        handshake: HandshakeSummary {
            session_id: ack.session_id,
            assigned_role: ack.assigned_role,
            capability_mask: ack.capability_mask,
            client_public_key,
            server_public_key: ack.server_public_key,
            client_nonce,
            server_nonce: ack.server_nonce,
        },
    })
}

async fn establish_server_session(
    connection: Connection,
    handshake: ServerHandshake,
) -> Result<TransportSession, TransportError> {
    let (mut control_send, mut control_recv) = connection.accept_bi().await?;

    let hello_bytes = read_frame_raw(&mut control_recv, handshake.heartbeat.timeout).await?;
    let session_request = parse_session_hello(
        &hello_bytes,
        handshake.protocol_version,
        handshake.schema_hash,
    )?;

    let capability_mask =
        negotiate_capabilities(&handshake.capabilities, &session_request.capabilities);

    let server_nonce = random_nonce();
    let session_id = rand::random::<u64>();
    let assigned_role = 1u32;
    let server_public_key = handshake.signing_key.verifying_key().to_bytes();

    let ack_bytes = build_session_ack(
        handshake.protocol_version,
        handshake.schema_hash,
        &server_nonce,
        session_id,
        assigned_role,
        &capability_mask,
        &server_public_key,
    );

    write_frame_raw(&mut control_send, &ack_bytes).await?;

    let replication = connection.accept_bi().await?;
    let assets = connection.accept_bi().await?;

    let metrics = TransportMetricsHandle::new();
    metrics.update(|m| {
        m.kind = TransportKind::Quic;
        m.packets_received = m.packets_received.saturating_add(1);
        m.compression_ratio = 1.0;
    });

    let control_send_arc = Arc::new(TokioMutex::new(control_send));
    let heartbeat = HeartbeatActor::spawn(
        control_send_arc.clone(),
        control_recv,
        metrics.clone(),
        handshake.heartbeat,
    );

    Ok(TransportSession {
        connection,
        control_send: control_send_arc,
        replication: FramedStream::new(replication.0, replication.1),
        assets: FramedStream::new(assets.0, assets.1),
        metrics,
        heartbeat,
        handshake: HandshakeSummary {
            session_id,
            assigned_role,
            capability_mask,
            client_public_key: session_request.client_public_key,
            server_public_key,
            client_nonce: session_request.client_nonce,
            server_nonce,
        },
    })
}

struct SessionHelloData {
    capabilities: Vec<u32>,
    client_public_key: [u8; 32],
    client_nonce: Vec<u8>,
}

struct SessionAckData {
    session_id: u64,
    assigned_role: u32,
    capability_mask: Vec<u32>,
    server_public_key: [u8; 32],
    server_nonce: Vec<u8>,
}

fn parse_session_hello(
    bytes: &[u8],
    protocol_version: u32,
    schema_hash: u64,
) -> Result<SessionHelloData, TransportError> {
    let envelope = net::root_as_message_envelope(bytes)
        .map_err(|err| TransportError::Flatbuffers(err.to_string()))?;
    let header = envelope
        .header()
        .ok_or_else(|| TransportError::Handshake("missing packet header".into()))?;
    if header.schema_hash() != schema_hash {
        return Err(TransportError::Handshake("schema hash mismatch".into()));
    }
    if header.sequence_id() != 0 {
        return Err(TransportError::Handshake("unexpected sequence id".into()));
    }
    let hello = envelope
        .body_as_session_hello()
        .ok_or_else(|| TransportError::Handshake("expected SessionHello".into()))?;
    if hello.protocol_version() != protocol_version {
        return Err(TransportError::Handshake(
            "protocol version mismatch".into(),
        ));
    }
    let capabilities = hello
        .requested_capabilities()
        .map(|vec| vec.iter().collect::<Vec<u32>>())
        .unwrap_or_default();
    let public_key_bytes: Vec<u8> = hello
        .client_public_key()
        .ok_or_else(|| TransportError::Handshake("missing client public key".into()))?
        .iter()
        .collect();
    if public_key_bytes.len() != 32 {
        return Err(TransportError::Handshake(
            "client public key must be 32 bytes".into(),
        ));
    }
    let nonce: Vec<u8> = hello
        .client_nonce()
        .ok_or_else(|| TransportError::Handshake("missing client nonce".into()))?
        .iter()
        .collect();
    if nonce.is_empty() {
        return Err(TransportError::Handshake(
            "client nonce must be non-empty".into(),
        ));
    }
    Ok(SessionHelloData {
        capabilities,
        client_public_key: public_key_bytes.as_slice().try_into().unwrap(),
        client_nonce: nonce,
    })
}
fn parse_session_ack(
    bytes: &[u8],
    protocol_version: u32,
    schema_hash: u64,
) -> Result<SessionAckData, TransportError> {
    let envelope = net::root_as_message_envelope(bytes)
        .map_err(|err| TransportError::Flatbuffers(err.to_string()))?;
    let header = envelope
        .header()
        .ok_or_else(|| TransportError::Handshake("missing packet header".into()))?;
    if header.schema_hash() != schema_hash {
        return Err(TransportError::Handshake("schema hash mismatch".into()));
    }
    let ack = envelope
        .body_as_session_acknowledge()
        .ok_or_else(|| TransportError::Handshake("expected SessionAcknowledge".into()))?;
    if ack.protocol_version() != protocol_version {
        return Err(TransportError::Handshake(
            "protocol version mismatch".into(),
        ));
    }
    let capability_mask = ack
        .capability_mask()
        .map(|vec| vec.iter().collect::<Vec<u32>>())
        .unwrap_or_default();
    let public_key_bytes: Vec<u8> = ack
        .server_public_key()
        .ok_or_else(|| TransportError::Handshake("missing server public key".into()))?
        .iter()
        .collect();
    if public_key_bytes.len() != 32 {
        return Err(TransportError::Handshake(
            "server public key must be 32 bytes".into(),
        ));
    }
    let server_nonce: Vec<u8> = ack
        .server_nonce()
        .ok_or_else(|| TransportError::Handshake("missing server nonce".into()))?
        .iter()
        .collect();
    if server_nonce.is_empty() {
        return Err(TransportError::Handshake(
            "server nonce must be non-empty".into(),
        ));
    }

    Ok(SessionAckData {
        session_id: ack.session_id(),
        assigned_role: ack.assigned_role(),
        capability_mask,
        server_public_key: public_key_bytes.as_slice().try_into().unwrap(),
        server_nonce,
    })
}

fn negotiate_capabilities(server: &[u32], requested: &[u32]) -> Vec<u32> {
    requested
        .iter()
        .copied()
        .filter(|cap| server.contains(cap))
        .collect()
}

fn build_session_hello(
    protocol_version: u32,
    schema_hash: u64,
    client_nonce: &[u8],
    capabilities: &[u32],
    auth_token: Option<&str>,
    client_public_key: &[u8],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(HANDSHAKE_CAPACITY);
    let nonce_vec = builder.create_vector(client_nonce);
    let capabilities_vec = builder.create_vector(capabilities);
    let auth = auth_token.map(|token| builder.create_string(token));
    let public_key_vec = builder.create_vector(client_public_key);
    let hello = net::SessionHello::create(
        &mut builder,
        &SessionHelloArgs {
            protocol_version,
            schema_hash,
            client_nonce: Some(nonce_vec),
            requested_capabilities: Some(capabilities_vec),
            auth_token: auth,
            client_public_key: Some(public_key_vec),
        },
    );
    let header = net::PacketHeader::create(
        &mut builder,
        &PacketHeaderArgs {
            sequence_id: 0,
            timestamp_ms: current_time_millis(),
            compression: Compression::None,
            schema_hash,
        },
    );
    let body = WIPOffset::<UnionWIPOffset>::new(hello.value());
    let envelope = net::MessageEnvelope::create(
        &mut builder,
        &MessageEnvelopeArgs {
            header: Some(header),
            body_type: MessageBody::SessionHello,
            body: Some(body),
        },
    );
    net::finish_message_envelope_buffer(&mut builder, envelope);
    builder.finished_data().to_vec()
}

fn build_session_ack(
    protocol_version: u32,
    schema_hash: u64,
    server_nonce: &[u8],
    session_id: u64,
    assigned_role: u32,
    capability_mask: &[u32],
    server_public_key: &[u8],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(HANDSHAKE_CAPACITY);
    let nonce_vec = builder.create_vector(server_nonce);
    let mask_vec = builder.create_vector(capability_mask);
    let public_key_vec = builder.create_vector(server_public_key);
    let ack = net::SessionAcknowledge::create(
        &mut builder,
        &SessionAcknowledgeArgs {
            protocol_version,
            schema_hash,
            server_nonce: Some(nonce_vec),
            session_id,
            assigned_role,
            capability_mask: Some(mask_vec),
            server_public_key: Some(public_key_vec),
        },
    );
    let header = net::PacketHeader::create(
        &mut builder,
        &PacketHeaderArgs {
            sequence_id: 0,
            timestamp_ms: current_time_millis(),
            compression: Compression::None,
            schema_hash,
        },
    );
    let body = WIPOffset::<UnionWIPOffset>::new(ack.value());
    let envelope = net::MessageEnvelope::create(
        &mut builder,
        &MessageEnvelopeArgs {
            header: Some(header),
            body_type: MessageBody::SessionAcknowledge,
            body: Some(body),
        },
    );
    net::finish_message_envelope_buffer(&mut builder, envelope);
    builder.finished_data().to_vec()
}

fn build_heartbeat_message(sequence: u64, metrics: TransportDiagnostics) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let heartbeat = net::Heartbeat::create(
        &mut builder,
        &HeartbeatArgs {
            sequence_id: sequence,
            timestamp_ms: current_time_millis(),
            reported_rtt_ms: metrics.rtt_ms as u32,
            jitter_ms: metrics.jitter_ms as u32,
        },
    );
    let header = net::PacketHeader::create(
        &mut builder,
        &PacketHeaderArgs {
            sequence_id: sequence,
            timestamp_ms: current_time_millis(),
            compression: Compression::None,
            schema_hash: 0,
        },
    );
    let body = WIPOffset::<UnionWIPOffset>::new(heartbeat.value());
    let envelope = net::MessageEnvelope::create(
        &mut builder,
        &MessageEnvelopeArgs {
            header: Some(header),
            body_type: MessageBody::Heartbeat,
            body: Some(body),
        },
    );
    net::finish_message_envelope_buffer(&mut builder, envelope);
    builder.finished_data().to_vec()
}

async fn write_frame_raw(stream: &mut SendStream, payload: &[u8]) -> Result<(), TransportError> {
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(payload).await?;
    Ok(())
}

async fn read_frame_raw(
    stream: &mut RecvStream,
    timeout: Duration,
) -> Result<Vec<u8>, TransportError> {
    let mut len_buf = [0u8; FRAME_HEADER_LEN];
    tokio::time::timeout(timeout, stream.read_exact(&mut len_buf))
        .await
        .map_err(|_| TransportError::Timeout("control stream timed out".into()))??;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut data = vec![0u8; len];
    tokio::time::timeout(timeout, stream.read_exact(&mut data))
        .await
        .map_err(|_| TransportError::Timeout("control stream timed out".into()))??;
    Ok(data)
}

impl HeartbeatActor {
    fn spawn(
        control_send: Arc<TokioMutex<SendStream>>,
        mut control_recv: RecvStream,
        metrics: TransportMetricsHandle,
        config: HeartbeatConfig,
    ) -> Self {
        let send_handle_metrics = metrics.clone();
        let send_handle = tokio::spawn(async move {
            let mut sequence = 0u64;
            loop {
                tokio::time::sleep(config.interval).await;
                sequence = sequence.wrapping_add(1);
                let snapshot = send_handle_metrics.latest().unwrap_or_default();
                let payload = build_heartbeat_message(sequence, snapshot.clone());
                if write_frame_locked(&control_send, payload).await.is_err() {
                    break;
                }
                send_handle_metrics.update(|m| {
                    m.kind = TransportKind::Quic;
                    m.packets_sent = m.packets_sent.saturating_add(1);
                    m.compression_ratio = 1.0;
                });
            }
        });

        let recv_handle_metrics = metrics.clone();
        let recv_handle = tokio::spawn(async move {
            loop {
                let frame = read_frame_raw(&mut control_recv, config.timeout).await;
                let bytes = match frame {
                    Ok(bytes) => bytes,
                    Err(_err) => {
                        break;
                    }
                };
                match net::root_as_message_envelope(&bytes) {
                    Ok(envelope) => {
                        if matches!(envelope.body_type(), MessageBody::Heartbeat)
                            && let Some(hb) = envelope.body_as_heartbeat()
                        {
                            let now_ms = current_time_millis();
                            let remote_ts = hb.timestamp_ms();
                            let diff = if now_ms >= remote_ts {
                                (now_ms - remote_ts) as f32
                            } else {
                                0.0
                            };
                            recv_handle_metrics.update(|m| {
                                m.kind = TransportKind::Quic;
                                let prev = m.rtt_ms;
                                m.rtt_ms = diff;
                                m.jitter_ms = (diff - prev).abs();
                                m.packets_received = m.packets_received.saturating_add(1);
                                m.compression_ratio = 1.0;
                            });
                        }
                    }
                    Err(err) => {
                        eprintln!("[transport] failed to decode control message: {err}");
                    }
                }
            }
        });

        Self {
            sender: send_handle,
            receiver: recv_handle,
        }
    }
}

async fn write_frame_locked(
    sender: &Arc<TokioMutex<SendStream>>,
    payload: Vec<u8>,
) -> Result<(), TransportError> {
    let mut guard = sender.lock().await;
    write_frame_raw(&mut guard, &payload).await
}

fn random_nonce() -> Vec<u8> {
    let mut nonce = vec![0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

#[derive(Debug, PartialEq, Eq)]
enum DecodedReplicationFrame {
    Command(CommandPacket),
    ComponentDelta(Vec<u8>),
    Unknown(u8, Vec<u8>),
}

fn encode_command_packet_frame(packet: &CommandPacket) -> Result<Vec<u8>, TransportError> {
    let payload =
        serde_json::to_vec(packet).map_err(|err| TransportError::Serialization(err.to_string()))?;
    Ok(encode_framed_payload(FRAME_KIND_COMMAND_PACKET, payload))
}

#[cfg(test)]
fn encode_component_delta_frame(bytes: &[u8]) -> Vec<u8> {
    encode_framed_payload(FRAME_KIND_COMPONENT_DELTA, bytes.to_vec())
}

fn encode_framed_payload(kind: u8, payload: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&payload);
    frame
}

fn decode_replication_frame(bytes: &[u8]) -> Result<DecodedReplicationFrame, TransportError> {
    if bytes.is_empty() {
        return Err(TransportError::Protocol(
            "replication frame missing kind byte".into(),
        ));
    }

    let payload = bytes[1..].to_vec();
    match bytes[0] {
        FRAME_KIND_COMMAND_PACKET => {
            let packet = serde_json::from_slice::<CommandPacket>(&payload)
                .map_err(|err| TransportError::Serialization(err.to_string()))?;
            Ok(DecodedReplicationFrame::Command(packet))
        }
        FRAME_KIND_COMPONENT_DELTA => Ok(DecodedReplicationFrame::ComponentDelta(payload)),
        other => Ok(DecodedReplicationFrame::Unknown(other, payload)),
    }
}

#[cfg(all(test, feature = "network-quic"))]
mod tests {
    use super::*;
    use quinn::{ClientConfig, Endpoint, ServerConfig};
    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use std::sync::Arc;

    fn build_certified_key() -> CertifiedKey {
        generate_simple_self_signed(["localhost".into()]).expect("self-signed cert")
    }

    fn server_config(cert_key: &CertifiedKey) -> ServerConfig {
        let cert_der = cert_key.cert.der().as_ref().to_vec();
        let key_der = cert_key.key_pair.serialize_der();
        ServerConfig::with_single_cert(
            vec![rustls::Certificate(cert_der)],
            rustls::PrivateKey(key_der),
        )
        .expect("server config")
    }

    fn client_config(cert_key: &CertifiedKey) -> ClientConfig {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(&rustls::Certificate(cert_key.cert.der().as_ref().to_vec()))
            .expect("add root cert");
        ClientConfig::with_root_certificates(roots)
    }

    fn client_endpoint(config: ClientConfig) -> Endpoint {
        let mut endpoint =
            Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("client endpoint");
        endpoint.set_default_client_config(config);
        endpoint
    }

    #[tokio::test]
    async fn quic_handshake_timeout_returns_error_within_deadline() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(100),
            timeout: Duration::from_millis(500),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(connection) = connecting.await
            {
                // Drop the connection without responding to force the client to time out.
                drop(connection);
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let start = Instant::now();
        let result = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: SigningKey::generate(&mut OsRng),
                heartbeat: heartbeat_cfg,
                server_name: "localhost".into(),
            },
        )
        .await;

        assert!(result.is_err(), "handshake unexpectedly succeeded");
        assert!(start.elapsed() < Duration::from_secs(2));

        let _ = server_task.await;
    }

    #[tokio::test]
    async fn quic_transport_tolerates_empty_packet_list() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(100),
            timeout: Duration::from_secs(1),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_handshake = ServerHandshake {
            protocol_version: 1,
            schema_hash: 0xABCDu64,
            capabilities: vec![1, 2, 3],
            signing_key: SigningKey::generate(&mut OsRng),
            heartbeat: heartbeat_cfg.clone(),
        };

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(connecting, server_handshake).await
            {
                tokio::time::sleep(Duration::from_millis(150)).await;
                session.close().await;
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: SigningKey::generate(&mut OsRng),
                heartbeat: heartbeat_cfg,
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        client_session
            .send_command_packets(&[])
            .await
            .expect("sending empty slice succeeds");

        tokio::time::sleep(Duration::from_millis(100)).await;
        client_session.close().await;

        let _ = server_task.await;
    }

    #[tokio::test]
    async fn quic_handshake_and_heartbeat_updates_metrics() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(200),
            timeout: Duration::from_secs(2),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_heartbeat = heartbeat_cfg.clone();
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);
        let client_signing_key = SigningKey::generate(&mut OsRng);

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: server_signing_key,
                        heartbeat: server_heartbeat,
                    },
                )
                .await
            {
                tokio::time::sleep(Duration::from_secs(1)).await;
                session.close().await;
            }
        });

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: heartbeat_cfg,
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        tokio::time::sleep(Duration::from_millis(250)).await;
        let metrics = client_session
            .metrics_handle()
            .latest()
            .expect("metrics snapshot");
        assert!(metrics.packets_sent > 0);
        assert!(metrics.packets_received > 0);

        client_session.close().await;
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn handshake_validates_protocol_version() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);
        let client_signing_key = SigningKey::generate(&mut OsRng);

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await {
                // Server should reject due to protocol version mismatch
                let result = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![],
                        signing_key: server_signing_key,
                        heartbeat: HeartbeatConfig::default(),
                    },
                )
                .await;
                assert!(result.is_err());
            }
        });

        // Client uses different protocol version
        let result = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 2,
                schema_hash: 0xABCDu64,
                capabilities: vec![],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(100),
                    timeout: Duration::from_millis(500),
                },
                server_name: "localhost".into(),
            },
        )
        .await;

        // Client should get error (timeout or connection reset)
        assert!(result.is_err());
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn handshake_validates_schema_hash() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);
        let client_signing_key = SigningKey::generate(&mut OsRng);

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await {
                // Server should reject due to schema hash mismatch
                let result = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![],
                        signing_key: server_signing_key,
                        heartbeat: HeartbeatConfig::default(),
                    },
                )
                .await;
                assert!(result.is_err());
            }
        });

        // Client uses different schema hash
        let result = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xDEADBEEFu64,
                capabilities: vec![],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(100),
                    timeout: Duration::from_millis(500),
                },
                server_name: "localhost".into(),
            },
        )
        .await;

        // Client should get error (timeout or connection reset)
        assert!(result.is_err());
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn capability_negotiation_filters_unsupported() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);
        let client_signing_key = SigningKey::generate(&mut OsRng);

        let _server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: server_signing_key,
                        heartbeat: HeartbeatConfig::default(),
                    },
                )
                .await
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                session.close().await;
            }
        });

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![2, 4, 5],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: HeartbeatConfig::default(),
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        let handshake = client_session.handshake();
        // Only capability 2 is in both client and server sets
        assert_eq!(handshake.capability_mask, vec![2]);

        client_session.close().await;
    }

    #[tokio::test]
    async fn handshake_exchanges_public_keys() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);
        let server_public_key = server_signing_key.verifying_key().to_bytes();
        let client_signing_key = SigningKey::generate(&mut OsRng);
        let client_public_key = client_signing_key.verifying_key().to_bytes();

        let _server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![],
                        signing_key: server_signing_key,
                        heartbeat: HeartbeatConfig::default(),
                    },
                )
                .await
            {
                let handshake = session.handshake();
                assert_eq!(handshake.client_public_key, client_public_key);
                tokio::time::sleep(Duration::from_millis(100)).await;
                session.close().await;
            }
        });

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: HeartbeatConfig::default(),
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        let handshake = client_session.handshake();
        assert_eq!(handshake.server_public_key, server_public_key);
        assert_eq!(handshake.client_public_key, client_public_key);
        assert_eq!(handshake.client_nonce.len(), 24);
        assert_eq!(handshake.server_nonce.len(), 24);

        client_session.close().await;
    }

    #[tokio::test]
    async fn multiple_clients_receive_heartbeats_independently() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(150),
            timeout: Duration::from_secs(5),
        };
        let server_heartbeat_cfg = heartbeat_cfg.clone();

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_task = tokio::spawn(async move {
            let mut sessions = Vec::new();
            for _ in 0..2 {
                let server_handshake = ServerHandshake {
                    protocol_version: 1,
                    schema_hash: 0xABCDu64,
                    capabilities: vec![1, 2, 3],
                    signing_key: SigningKey::generate(&mut OsRng),
                    heartbeat: server_heartbeat_cfg.clone(),
                };
                if let Some(connecting) = server_endpoint.accept().await
                    && let Ok(session) = accept(connecting, server_handshake).await
                {
                    sessions.push(session);
                }
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
            for session in sessions {
                session.close().await;
            }
        });

        let mut client_cfg1 = client_config(&cert_key);
        client_cfg1.transport_config(transport.clone());
        let client_endpoint1 = client_endpoint(client_cfg1);

        let mut client_cfg2 = client_config(&cert_key);
        client_cfg2.transport_config(transport.clone());
        let client_endpoint2 = client_endpoint(client_cfg2);

        let client1_key = SigningKey::generate(&mut OsRng);
        let client2_key = SigningKey::generate(&mut OsRng);

        let client1_session = connect(
            &client_endpoint1,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: client1_key,
                heartbeat: heartbeat_cfg.clone(),
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client1 connects");

        tokio::time::sleep(Duration::from_millis(250)).await;

        let metrics1 = client1_session
            .metrics_handle()
            .latest()
            .expect("metrics snapshot 1");
        assert!(metrics1.packets_received > 0);

        client1_session.close().await;
        let client2_session = connect(
            &client_endpoint2,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![2, 5],
                auth_token: None,
                signing_key: client2_key,
                heartbeat: heartbeat_cfg.clone(),
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client2 connects");

        tokio::time::sleep(Duration::from_millis(250)).await;

        let metrics2 = client2_session
            .metrics_handle()
            .latest()
            .expect("metrics snapshot 2");
        assert!(metrics2.packets_received > 0);

        client2_session.close().await;
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn assets_stream_transfers_large_payloads() {
        const PAYLOAD_SIZE: usize = 2 * 1024 * 1024; // 2 MiB
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(250),
            timeout: Duration::from_secs(1),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let payload_pattern = 0xACu8;

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: SigningKey::generate(&mut OsRng),
                        heartbeat: heartbeat_cfg,
                    },
                )
                .await
            {
                let received = session
                    .assets
                    .read_to_end(PAYLOAD_SIZE + 1024)
                    .await
                    .expect("read payload");
                assert_eq!(received.len(), PAYLOAD_SIZE);
                assert!(received.into_iter().all(|b| b == payload_pattern));
                session.close().await;
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: SigningKey::generate(&mut OsRng),
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(250),
                    timeout: Duration::from_secs(1),
                },
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        let payload = vec![payload_pattern; PAYLOAD_SIZE];
        client_session
            .assets
            .write_all(&payload)
            .await
            .expect("write payload");
        client_session
            .assets
            .finish_send()
            .await
            .expect("finish payload stream");

        client_session.close().await;
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn command_packets_roundtrip_over_replication_stream() {
        use crate::network::command_log::{
            AuthorId, CommandAuthor, CommandBatch, CommandEntry, CommandId, CommandPayload,
            CommandRole, CommandScope, ConflictStrategy,
        };

        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(200),
            timeout: Duration::from_secs(1),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: server_signing_key,
                        heartbeat: heartbeat_cfg,
                    },
                )
                .await
            {
                let packet = session
                    .receive_command_packet(Duration::from_secs(1))
                    .await
                    .expect("receive command packet")
                    .expect("command packet present");

                assert_eq!(packet.sequence, 42);
                let batch = packet.decode().expect("decode command batch");
                assert_eq!(batch.entries.len(), 1);
                let entry = &batch.entries[0];
                assert_eq!(entry.payload.command_type, "test.command");
                assert_eq!(entry.strategy, ConflictStrategy::LastWriteWins);

                session.close().await;
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let client_signing_key = SigningKey::generate(&mut OsRng);

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 2, 3],
                auth_token: None,
                signing_key: client_signing_key,
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(200),
                    timeout: Duration::from_secs(1),
                },
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        let author = CommandAuthor::new(AuthorId(7), CommandRole::Editor);
        let payload = CommandPayload::new("test.command", CommandScope::Global, vec![1, 2, 3, 4]);
        let entry = CommandEntry::new(
            CommandId::new(9, AuthorId(7)),
            1_234,
            payload,
            ConflictStrategy::LastWriteWins,
            author,
            None,
        );
        let batch = CommandBatch {
            sequence: 42,
            nonce: 9,
            timestamp_ms: 5_678,
            author: AuthorId(7),
            entries: vec![entry],
        };
        let packet = CommandPacket::from_batch(&batch).expect("serialize command batch");

        let delta_marker = vec![0xAA, 0xBB, 0xCC];
        let delta_frame = encode_component_delta_frame(&delta_marker);

        client_session
            .replication
            .write_frame(&delta_frame)
            .await
            .expect("send delta frame");

        client_session
            .send_command_packets(std::slice::from_ref(&packet))
            .await
            .expect("send command packet");

        let metrics = client_session
            .metrics_handle()
            .latest()
            .expect("metrics snapshot");
        assert!(metrics.packets_sent >= 2);

        client_session.close().await;
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn quic_transport_drops_oversized_command_packet() {
        use crate::network::command_log::{
            AuthorId, CommandAuthor, CommandBatch, CommandEntry, CommandId, CommandPayload,
            CommandRole, CommandScope, ConflictStrategy,
        };

        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(200),
            timeout: Duration::from_secs(1),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_signing_key = SigningKey::generate(&mut OsRng);

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: server_signing_key,
                        heartbeat: heartbeat_cfg,
                    },
                )
                .await
            {
                let packet = session
                    .receive_command_packet(Duration::from_secs(1))
                    .await
                    .expect("receive command packet")
                    .expect("valid packet available");

                assert_eq!(
                    packet.sequence, 2,
                    "expected oversized packet to be dropped"
                );

                session.close().await;
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 2, 3],
                auth_token: None,
                signing_key: SigningKey::generate(&mut OsRng),
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(200),
                    timeout: Duration::from_secs(1),
                },
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        let author = CommandAuthor::new(AuthorId(7), CommandRole::Editor);

        let oversized_packet = CommandPacket {
            sequence: 1,
            nonce: 9,
            timestamp_ms: current_time_millis(),
            payload: vec![0u8; MAX_COMMAND_PACKET_BYTES + 1],
        };

        let valid_entry = CommandEntry::new(
            CommandId::new(2, AuthorId(7)),
            current_time_millis(),
            CommandPayload::new("editor.selection", CommandScope::Global, vec![1, 2, 3]),
            ConflictStrategy::Merge,
            author.clone(),
            None,
        );
        let batch = CommandBatch {
            sequence: 2,
            nonce: 10,
            timestamp_ms: current_time_millis(),
            author: author.id.clone(),
            entries: vec![valid_entry],
        };
        let valid_packet = CommandPacket::from_batch(&batch).expect("serialize valid packet");

        client_session
            .send_command_packets(&[oversized_packet, valid_packet])
            .await
            .expect("send packets");

        tokio::time::sleep(Duration::from_millis(100)).await;
        client_session.close().await;

        let _ = server_task.await;
    }

    #[test]
    fn replication_frame_decoding_classifies_component_delta() {
        let payload = vec![1, 2, 3, 4];
        let frame = encode_component_delta_frame(&payload);
        match decode_replication_frame(&frame).expect("decode frame") {
            DecodedReplicationFrame::ComponentDelta(bytes) => assert_eq!(bytes, payload),
            other => panic!("unexpected frame variant: {other:?}"),
        }

        let mut unknown = vec![0xFE];
        unknown.extend_from_slice(&payload);
        match decode_replication_frame(&unknown).expect("decode unknown frame") {
            DecodedReplicationFrame::Unknown(kind, bytes) => {
                assert_eq!(kind, 0xFE);
                assert_eq!(bytes, payload);
            }
            other => panic!("unexpected frame variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn heartbeat_tasks_stop_after_connection_drop() {
        let cert_key = build_certified_key();
        let transport = Arc::new(quinn::TransportConfig::default());
        let heartbeat_cfg = HeartbeatConfig {
            interval: Duration::from_millis(50),
            timeout: Duration::from_millis(200),
        };

        let mut server_cfg = server_config(&cert_key);
        server_cfg.transport_config(transport.clone());
        let server_endpoint =
            Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).expect("server endpoint");
        let server_addr = server_endpoint.local_addr().expect("server addr");

        let server_task = tokio::spawn(async move {
            if let Some(connecting) = server_endpoint.accept().await
                && let Ok(session) = accept(
                    connecting,
                    ServerHandshake {
                        protocol_version: 1,
                        schema_hash: 0xABCDu64,
                        capabilities: vec![1, 2, 3],
                        signing_key: SigningKey::generate(&mut OsRng),
                        heartbeat: heartbeat_cfg,
                    },
                )
                .await
            {
                tokio::time::sleep(Duration::from_millis(120)).await;
                drop(session);
            }
        });

        let mut client_cfg = client_config(&cert_key);
        client_cfg.transport_config(transport);
        let client_endpoint = client_endpoint(client_cfg);

        let client_session = connect(
            &client_endpoint,
            server_addr,
            ClientHandshake {
                protocol_version: 1,
                schema_hash: 0xABCDu64,
                capabilities: vec![1, 4],
                auth_token: None,
                signing_key: SigningKey::generate(&mut OsRng),
                heartbeat: HeartbeatConfig {
                    interval: Duration::from_millis(50),
                    timeout: Duration::from_millis(200),
                },
                server_name: "localhost".into(),
            },
        )
        .await
        .expect("client handshake");

        tokio::time::sleep(Duration::from_millis(400)).await;

        tokio::time::timeout(Duration::from_millis(400), async {
            while !client_session.heartbeat.receiver.is_finished() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("receiver finishes after drop");

        client_session.close().await;
        let _ = server_task.await;
    }

    #[test]
    fn heartbeat_metrics_clamp_future_timestamps() {
        let handle = TransportMetricsHandle::new();
        handle.update(|m| {
            m.rtt_ms = 24.0;
            m.jitter_ms = 0.0;
        });

        let now = current_time_millis();
        let future = now.saturating_add(5_000);

        // Mimic HeartbeatActor receiver logic with a future timestamp
        let diff = if now >= future {
            (now - future) as f32
        } else {
            0.0
        };
        handle.update(|m| {
            let prev = m.rtt_ms;
            m.rtt_ms = diff;
            m.jitter_ms = (diff - prev).abs();
            m.packets_received = m.packets_received.saturating_add(1);
        });

        let metrics = handle.latest().expect("metrics snapshot");
        assert_eq!(metrics.rtt_ms, 0.0);
        assert_eq!(metrics.jitter_ms, 24.0);
        assert_eq!(metrics.packets_received, 1);
    }
}
