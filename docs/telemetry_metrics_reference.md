# Telemetry Metrics Reference

**Date:** October 31, 2025  
**Owner:** Systems & Networking Team

This reference describes the command telemetry fields emitted by the engine each frame. Values originate from the `CommandPipeline` metrics snapshot and are surfaced through the `FrameTelemetry` JSON payload consumed by the in-editor overlay.

## Command Metrics Snapshot Fields

| Field | Type | Description |
| ----- | ---- | ----------- |
| `total_appended` | `u64` | Cumulative count of commands appended locally since session start. Monotonic. |
| `append_rate_per_sec` | `f32` | Exponentially weighted moving average of local append throughput (commands/sec). Uses α = 0.2. |
| `conflict_rejections` | `HashMap<ConflictStrategy, u64>` | Per-strategy counters for conflicts, duplicates, or permission failures encountered locally or during remote integration. |
| `queue_depth` | `usize` | Number of serialized `CommandPacket`s currently staged inside the `CommandTransportQueue`. Updated once per frame. |
| `signature_verify_latency_ms` | `f32` | EWMA (α = 0.2) of Ed25519 verification latency recorded during remote packet integration. Reported in milliseconds. |
| `replay_rejections` | `u64` | Count of commands dropped due to replay detection (nonce/lamport high-water check). Includes local and remote enforcement. |
| `rate_limit_drops` | `u64` | Count of commands rejected by per-author token bucket limiter (local + remote). |
| `payload_guard_drops` | `u64` | Count of command packets dropped because they exceeded the 64 KiB payload cap or failed serialization. |

## Overlay Presentation

The telemetry overlay renders transport statistics (including active transport kind) followed by command metrics when a `CommandMetricsSnapshot` is present:

```
Network  kind WebRtc RTT   4.10 ms jitter   0.16 ms packets 32/30 ratio 1.00
Commands rate  3.25/s total 42 queue 5 sig-lat  2.87 ms
  Conflicts LastWriteWins:1, Merge:2
  Guards rate-limit 3 replay 1 payload 2
```

* Rate and queue depth thresholds:
  * Queue depth ≥ 10 triggers a yellow warning banner (planned for Phase 5 visual polish).
  * Conflict counters > 0 highlight the second line in amber to prompt operator review.

## Data Sources

| Metric | Source | Update Cadence |
| ------ | ------ | --------------- |
| Append rate, total appended | `CommandPipeline::record_local_append` | On every successful local append |
| Queue depth | `Engine::update_frame_diagnostics` → `CommandTransportQueue::pending_depth` | Once per frame |
| Signature latency | `CommandPipeline::integrate_remote_packet` | For every remote entry processed |
| Conflict counters | `CommandPipeline::record_conflict` | On conflicts, duplicates, or permission failures |
| Replay rejections | `CommandPipeline::record_replay_rejection` | On replay detection during local append or remote integration |
| Rate-limit drops | `CommandPipeline::record_rate_limit_drop` | Whenever token bucket rejects a command |
| Payload guard drops | `CommandPipeline::record_payload_guard_drop` | Whenever serialization fails or a packet breaches the payload cap |
| Transport kind | `TransportMetricsHandle::update` (QUIC/WebRTC) | Whenever transport metrics are refreshed |

## Transport Diagnostics Fields

`TransportDiagnostics` accompanies command metrics inside the telemetry payload. It now exposes the active transport kind so operators can verify QUIC vs WebRTC fallback in real time.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `kind` | `TransportKind` (`Unknown` \| `Quic` \| `WebRtc`) | Currently active transport pipeline for command replication. Defaults to `Unknown` until a transport attaches. |
| `rtt_ms` | `f32` | Smoothed round-trip time measured via heartbeat pings. |
| `jitter_ms` | `f32` | Absolute change in RTT between consecutive heartbeats. |
| `packets_sent` / `packets_received` | `u64` | Command packets transmitted/received over the active transport. |
| `compression_ratio` | `f32` | Ratio of compressed to raw bytes (1.0 when compression disabled). |
| `command_bandwidth_bytes_per_sec` | `f32` | Estimated outbound bandwidth utilisation for command packets. |
| `command_latency_ms` | `f32` | Transport-side latency derived from packet timestamps. |

## Alerting Guidelines

| Condition | Impact | Recommended Action |
| --------- | ------ | ------------------ |
| `queue_depth > 25` for >2s | Transport backlog forming | Inspect QUIC session health, validate remote clients draining packets |
| `append_rate_per_sec > 20` sustained | Unusual command burst | Enable rate limiting (Phase 5) or investigate rogue tool |
| `signature_verify_latency_ms > 15` | Potential crypto bottleneck | Profile verifier, consider batching verifications |
| Any conflicts recorded with `Reject` | Data loss risk | Communicate to user; review in-flight editor actions |
| `replay_rejections > 0` | Suspicious duplicates or clock skew | Inspect offending author, verify Lamport/nonce sync |
| `rate_limit_drops > 10` / min | Command flood | Investigate offending tool/client; tune limiter thresholds |

## Future Enhancements

1. **Overlay Highlighting:** Add colour-coded emphasis for backlog and conflict alerts (Phase 5 UI polish).
2. **Time-Series Export:** Stream telemetry snapshots to diagnostics service for historical analysis.
3. **Per-Author Metrics:** Break down append/conflict rates per contributor for accountability.
