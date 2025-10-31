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

## Overlay Presentation

The telemetry overlay renders command metrics beneath transport statistics when a `CommandMetricsSnapshot` is present:

```
Commands rate  3.25/s total 42 queue 5 sig-lat  2.87 ms
  Conflicts LastWriteWins:1, Merge:2
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

## Alerting Guidelines

| Condition | Impact | Recommended Action |
| --------- | ------ | ------------------ |
| `queue_depth > 25` for >2s | Transport backlog forming | Inspect QUIC session health, validate remote clients draining packets |
| `append_rate_per_sec > 20` sustained | Unusual command burst | Enable rate limiting (Phase 5) or investigate rogue tool |
| `signature_verify_latency_ms > 15` | Potential crypto bottleneck | Profile verifier, consider batching verifications |
| Any conflicts recorded with `Reject` | Data loss risk | Communicate to user; review in-flight editor actions |

## Future Enhancements

1. **Overlay Highlighting:** Add colour-coded emphasis for backlog and conflict alerts (Phase 5 UI polish).
2. **Time-Series Export:** Stream telemetry snapshots to diagnostics service for historical analysis.
3. **Per-Author Metrics:** Break down append/conflict rates per contributor for accountability.
