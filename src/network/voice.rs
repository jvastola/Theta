use std::collections::VecDeque;
#[cfg(feature = "network-quic")]
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[cfg(feature = "network-quic")]
use audiopus::{
    Application as OpusApplication, Channels as OpusChannels, MutSignals, SampleRate,
    coder::{Decoder as OpusDecoder, Encoder as OpusEncoder},
    packet::Packet,
};

#[cfg(feature = "network-quic")]
const OPUS_FRAME_DURATION_MS: u32 = 20;

#[cfg(feature = "network-quic")]
const OPUS_MAX_PACKET_SIZE: usize = 1_275;

#[cfg(feature = "network-quic")]
fn opus_channel_count(channels: OpusChannels) -> Result<usize, VoiceCodecError> {
    match channels {
        OpusChannels::Mono => Ok(1),
        OpusChannels::Stereo => Ok(2),
        OpusChannels::Auto => Err(VoiceCodecError(
            "Opus channel auto-detection is not supported".into(),
        )),
    }
}

/// Error type returned by voice codec operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCodecError(pub String);

impl From<&str> for VoiceCodecError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for VoiceCodecError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[cfg(feature = "network-quic")]
impl From<audiopus::Error> for VoiceCodecError {
    fn from(value: audiopus::Error) -> Self {
        Self(value.to_string())
    }
}

/// Trait representing an encoder/decoder pair for voice audio data.
pub trait VoiceCodec {
    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, VoiceCodecError>;
    fn decode(&mut self, encoded: &[u8]) -> Result<Vec<i16>, VoiceCodecError>;
}

/// Simple passthrough codec used for scaffolding and unit testing.
#[derive(Default, Debug, Clone, Copy)]
pub struct PassthroughCodec;

impl VoiceCodec for PassthroughCodec {
    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, VoiceCodecError> {
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for sample in pcm {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Ok(bytes)
    }

    fn decode(&mut self, encoded: &[u8]) -> Result<Vec<i16>, VoiceCodecError> {
        if encoded.len() % 2 != 0 {
            return Err(VoiceCodecError(
                "encoded payload length must be even".into(),
            ));
        }

        let mut samples = Vec::with_capacity(encoded.len() / 2);
        for chunk in encoded.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            samples.push(sample);
        }
        Ok(samples)
    }
}

#[cfg(feature = "network-quic")]
#[derive(Debug)]
pub struct OpusCodec {
    encoder: OpusEncoder,
    decoder: OpusDecoder,
    channels: OpusChannels,
    channel_count: usize,
    frame_samples_per_channel: usize,
    max_frame_samples: usize,
}

#[cfg(feature = "network-quic")]
impl OpusCodec {
    pub fn new(channels: OpusChannels) -> Result<Self, VoiceCodecError> {
        let channel_count = opus_channel_count(channels)?;
        let frame_samples_per_channel =
            (SampleRate::Hz48000 as usize * OPUS_FRAME_DURATION_MS as usize) / 1000;
        let max_frame_samples = frame_samples_per_channel * channel_count;

        let encoder = OpusEncoder::new(SampleRate::Hz48000, channels, OpusApplication::Voip)?;
        let decoder = OpusDecoder::new(SampleRate::Hz48000, channels)?;

        Ok(Self {
            encoder,
            decoder,
            channels,
            channel_count,
            frame_samples_per_channel,
            max_frame_samples,
        })
    }

    pub fn mono() -> Result<Self, VoiceCodecError> {
        Self::new(OpusChannels::Mono)
    }

    pub fn stereo() -> Result<Self, VoiceCodecError> {
        Self::new(OpusChannels::Stereo)
    }

    pub fn expected_samples(&self) -> usize {
        self.max_frame_samples
    }
}

#[cfg(feature = "network-quic")]
impl VoiceCodec for OpusCodec {
    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, VoiceCodecError> {
        if pcm.len() != self.max_frame_samples {
            return Err(VoiceCodecError(format!(
                "expected {} samples for {:?}, got {}",
                self.max_frame_samples,
                self.channels,
                pcm.len()
            )));
        }

        let mut buffer = vec![0u8; OPUS_MAX_PACKET_SIZE];
        let len = self.encoder.encode(pcm, &mut buffer)?;
        buffer.truncate(len);
        Ok(buffer)
    }

    fn decode(&mut self, encoded: &[u8]) -> Result<Vec<i16>, VoiceCodecError> {
        let packet = Packet::try_from(encoded)?;
        let mut samples = vec![0i16; self.max_frame_samples];
        let signals = MutSignals::try_from(samples.as_mut_slice())?;
        let decoded_per_channel = self.decoder.decode(Some(packet), signals, false)?;
        if decoded_per_channel != self.frame_samples_per_channel {
            return Err(VoiceCodecError(format!(
                "decoded {decoded_per_channel} samples per channel, expected {}",
                self.frame_samples_per_channel
            )));
        }
        let total_samples = decoded_per_channel * self.channel_count;
        samples.truncate(total_samples);
        Ok(samples)
    }
}

/// Packet carrying encoded voice audio over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePacket {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub payload: Vec<u8>,
}

impl VoicePacket {
    pub fn new(sequence: u64, timestamp_ms: u64, payload: Vec<u8>) -> Self {
        Self {
            sequence,
            timestamp_ms,
            payload,
        }
    }
}

/// Tracks the order of incoming packets and mitigates jitter by reordering them.
#[derive(Debug, Clone)]
struct JitterBuffer {
    capacity: usize,
    packets: VecDeque<VoicePacket>,
}

impl JitterBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            packets: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    fn push(&mut self, packet: VoicePacket) {
        if self.packets.len() == self.capacity {
            self.packets.pop_front();
        }

        if let Some(position) = self
            .packets
            .iter()
            .position(|entry| entry.sequence > packet.sequence)
        {
            self.packets.insert(position, packet);
        } else {
            self.packets.push_back(packet);
        }
    }

    fn pop(&mut self) -> Option<VoicePacket> {
        self.packets.pop_front()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.packets.len()
    }
}

/// Simple energy-based detector to determine if a frame contains speech.
#[derive(Debug, Clone, Copy)]
pub struct VoiceActivityDetector {
    threshold: f32,
}

impl VoiceActivityDetector {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    pub fn is_voiced(&self, samples: &[i16]) -> bool {
        if samples.is_empty() {
            return false;
        }

        let max_value = i16::MAX as f32;
        let rms = samples
            .iter()
            .map(|sample| {
                let normalized = *sample as f32 / max_value;
                normalized * normalized
            })
            .sum::<f32>()
            / samples.len() as f32;

        let level = rms.sqrt() * std::f32::consts::SQRT_2;
        level >= self.threshold
    }
}

/// Aggregated voice telemetry used for diagnostics and monitoring.
#[derive(Debug, Default, Clone)]
pub struct VoiceMetrics {
    total_packets: u64,
    voiced_frames: u64,
    dropped_packets: u64,
}

impl VoiceMetrics {
    pub fn record_received(&mut self) {
        self.total_packets = self.total_packets.saturating_add(1);
    }

    pub fn record_voiced(&mut self) {
        self.voiced_frames = self.voiced_frames.saturating_add(1);
    }

    pub fn record_dropped(&mut self) {
        self.dropped_packets = self.dropped_packets.saturating_add(1);
    }

    pub fn record_gap(&mut self, missing: u64) {
        if missing == 0 {
            return;
        }
        self.dropped_packets = self.dropped_packets.saturating_add(missing);
    }

    pub fn reset(&mut self) {
        self.total_packets = 0;
        self.voiced_frames = 0;
        self.dropped_packets = 0;
    }

    pub fn total_packets(&self) -> u64 {
        self.total_packets
    }

    pub fn voiced_frames(&self) -> u64 {
        self.voiced_frames
    }

    pub fn dropped_packets(&self) -> u64 {
        self.dropped_packets
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VoiceDiagnostics {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_dropped: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub bitrate_kbps: f32,
    pub latency_ms: f32,
    pub jitter_ms: f32,
    pub voiced_frames: u64,
    #[serde(default)]
    pub active_speakers: Vec<String>,
}

#[derive(Clone, Default)]
pub struct VoiceDiagnosticsHandle {
    inner: Arc<Mutex<VoiceDiagnostics>>,
}

impl VoiceDiagnosticsHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VoiceDiagnostics::default())),
        }
    }

    pub fn snapshot(&self) -> Option<VoiceDiagnostics> {
        self.inner.lock().ok().map(|guard| guard.clone())
    }

    pub fn update(&self, apply: impl FnOnce(&mut VoiceDiagnostics)) {
        if let Ok(mut guard) = self.inner.lock() {
            apply(&mut guard);
        }
    }
}

/// High-level voice session composed of a codec, jitter buffer, VAD and metrics.
pub struct VoiceSession<C: VoiceCodec> {
    codec: C,
    jitter_buffer: JitterBuffer,
    vad: VoiceActivityDetector,
    metrics: VoiceMetrics,
    buffer_capacity: usize,
    highest_sequence: Option<u64>,
}

impl<C: VoiceCodec> VoiceSession<C> {
    pub fn new(codec: C, buffer_capacity: usize, vad_threshold: f32) -> Self {
        let normalized_capacity = buffer_capacity.max(1);
        Self {
            codec,
            jitter_buffer: JitterBuffer::new(normalized_capacity),
            vad: VoiceActivityDetector::new(vad_threshold),
            metrics: VoiceMetrics::default(),
            buffer_capacity: normalized_capacity,
            highest_sequence: None,
        }
    }

    pub fn enqueue_packet(&mut self, packet: VoicePacket) {
        if let Some(highest) = self.highest_sequence {
            if packet.sequence > highest {
                let expected_next = highest.saturating_add(1);
                if packet.sequence > expected_next {
                    self.metrics.record_gap(packet.sequence - expected_next);
                }
                self.highest_sequence = Some(packet.sequence);
            }
        } else {
            self.highest_sequence = Some(packet.sequence);
        }
        self.jitter_buffer.push(packet);
    }

    pub fn dequeue_samples(&mut self) -> Result<Option<Vec<i16>>, VoiceCodecError> {
        let packet = match self.jitter_buffer.pop() {
            Some(packet) => packet,
            None => return Ok(None),
        };

        self.metrics.record_received();
        let mut decoded = self.codec.decode(&packet.payload)?;

        if self.vad.is_voiced(&decoded) {
            self.metrics.record_voiced();
        } else {
            // Silence frames are still useful but we normalise them to zeros to keep tests simple.
            decoded.fill(0);
        }

        Ok(Some(decoded))
    }

    pub fn metrics(&self) -> &VoiceMetrics {
        &self.metrics
    }

    pub fn reset(&mut self) {
        self.jitter_buffer = JitterBuffer::new(self.buffer_capacity);
        self.metrics.reset();
        self.highest_sequence = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_roundtrip_preserves_samples() {
        let mut codec = PassthroughCodec;
        let samples = vec![0, 1_000, -3_000, 12_345, -32_768, 32_767];

        let encoded = codec.encode(&samples).expect("encode samples");
        assert_eq!(encoded.len(), samples.len() * 2);

        let decoded = codec.decode(&encoded).expect("decode samples");
        assert_eq!(decoded, samples);
    }

    #[test]
    fn jitter_buffer_reorders_packets() {
        let mut buffer = JitterBuffer::new(4);
        buffer.push(VoicePacket::new(2, 200, vec![2]));
        buffer.push(VoicePacket::new(1, 100, vec![1]));
        buffer.push(VoicePacket::new(3, 300, vec![3]));

        assert_eq!(buffer.len(), 3);

        let first = buffer.pop().expect("first packet");
        let second = buffer.pop().expect("second packet");
        let third = buffer.pop().expect("third packet");

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(third.sequence, 3);
    }

    #[test]
    fn vad_detects_speech_vs_silence() {
        let vad = VoiceActivityDetector::new(0.05);

        let silence = vec![0i16; 160];
        assert!(!vad.is_voiced(&silence));

        let quiet = vec![50i16; 160];
        assert!(!vad.is_voiced(&quiet));

        let loud = vec![3_000i16; 160];
        assert!(vad.is_voiced(&loud));
    }

    #[test]
    fn vad_rejects_background_noise_burst() {
        let vad = VoiceActivityDetector::new(0.1);
        let mut noise = Vec::with_capacity(960);
        for idx in 0..960 {
            let value = (((idx * 19) % 120) as i16) - 60;
            noise.push(value);
        }
        assert!(!vad.is_voiced(&noise));
    }

    #[test]
    fn metrics_track_packet_flow() {
        let mut metrics = VoiceMetrics::default();
        metrics.record_received();
        metrics.record_received();
        metrics.record_voiced();
        metrics.record_dropped();
        metrics.record_gap(3);

        assert_eq!(metrics.total_packets(), 2);
        assert_eq!(metrics.voiced_frames(), 1);
        assert_eq!(metrics.dropped_packets(), 4);
    }

    #[test]
    fn session_processes_packets_and_updates_metrics() {
        let mut codec = PassthroughCodec;
        let samples = vec![1_500i16; 160];
        let payload = codec.encode(&samples).expect("encode samples");

        let mut session = VoiceSession::new(PassthroughCodec, 4, 0.05);
        session.enqueue_packet(VoicePacket::new(1, 10, payload));

        let decoded = session
            .dequeue_samples()
            .expect("decode packet")
            .expect("packet available");
        assert_eq!(decoded.len(), samples.len());
        assert_eq!(session.metrics().total_packets(), 1);
        assert_eq!(session.metrics().voiced_frames(), 1);
    }

    #[cfg(feature = "network-quic")]
    #[test]
    fn opus_codec_roundtrip_preserves_samples() {
        let mut codec = OpusCodec::mono().expect("create opus codec");
        let sample_count = codec.expected_samples();
        let samples: Vec<i16> = (0..sample_count)
            .map(|idx| ((idx as i32 * 127).wrapping_sub(5_000) as i16))
            .collect();

        let encoded = codec.encode(&samples).expect("encode opus frame");
        assert!(!encoded.is_empty());
        let decoded = codec.decode(&encoded).expect("decode opus frame");
        assert_eq!(decoded, samples);
    }

    #[test]
    fn voice_session_tracks_packet_loss_and_reset() {
        let mut passthrough = PassthroughCodec;
        let payload = passthrough.encode(&[0i16, 1]).expect("encode payload");

        let mut session = VoiceSession::new(PassthroughCodec, 4, 0.05);
        session.enqueue_packet(VoicePacket::new(1, 0, payload.clone()));
        session.enqueue_packet(VoicePacket::new(4, 30, payload.clone()));

        assert_eq!(session.metrics().dropped_packets(), 2);

        session.reset();
        assert_eq!(session.metrics().dropped_packets(), 0);

        session.enqueue_packet(VoicePacket::new(1, 60, payload.clone()));
        session.enqueue_packet(VoicePacket::new(2, 70, payload));
        assert_eq!(session.metrics().dropped_packets(), 0);
    }

    #[test]
    fn voice_session_reset_clears_metrics() {
        let mut session = VoiceSession::new(PassthroughCodec, 2, 0.05);
        session.metrics.record_received();
        session.metrics.record_voiced();
        session.metrics.record_dropped();
        assert!(session.metrics().total_packets() > 0);

        session.reset();
        assert_eq!(session.metrics().total_packets(), 0);
        assert_eq!(session.metrics().voiced_frames(), 0);
        assert_eq!(session.metrics().dropped_packets(), 0);
    }
}
