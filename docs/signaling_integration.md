# Signaling Event Pump Design

**Status:** Negotiation live (offer/answer + ICE integrated; telemetry pending)  
**Updated:** November 5, 2025

## Overview

The engine now polls signaling events every frame to drive WebRTC peer connection negotiation. This enables automatic discovery and connection establishment between peers in the same signaling room.

## Architecture

### Frame Loop Integration

The `poll_signaling_events()` method runs immediately after `poll_remote_commands()` in the `update_frame_diagnostics()` path:

```rust
#[cfg(feature = "network-quic")]
self.poll_remote_commands();

#[cfg(feature = "network-quic")]
self.poll_signaling_events();
```

### Event Polling Strategy

- **Zero-Timeout Polling:** Uses `Duration::from_millis(0)` to avoid blocking the frame loop
- **Single Event Per Frame:** Processes one signaling event per frame to prevent stalls
- **Graceful Degradation:** Logs warnings on errors but continues running

### Event Handling

The pump dispatches incoming `SignalingResponse` events:

| Event Type | Current Behavior | Next Improvements |
|------------|------------------|-------------------|
| `Offer` | Ensures RTCPeerConnection exists, sets remote SDP, creates local answer, sends via signaling | Add retries/timeout handling and telemetry on slow answers |
| `Answer` | Applies remote SDP to the pending connection and flushes queued ICE candidates | Validate state mismatches and surface replication metrics |
| `IceCandidate` | Queues candidate and attempts to apply immediately; failed additions are retained for retry | Add exponential backoff and discard threshold for stale candidates |
| `PeerJoined` | Initiates offer when local peer ID is lexicographically lower to avoid duplicate negotiations | Evaluate mesh vs. host election heuristics |
| `PeerLeft` | Closes RTCPeerConnection, drops attached WebRTC transport, clears peer entry | Emit telemetry + fallback to QUIC automatically when active |
| `Error` | Logs error | N/A |
| `Registered` | Ignores (handled at bootstrap) | N/A |
| `HeartbeatAck` | Ignores | N/A |

## State Management

### Engine Fields

- `local_signaling_peer`, `signaling_room`, `signaling_endpoint`: establish identity and room scope for signaling and are populated by bootstrap/env overrides.
- `signaling_events_polled`: monotonic counter used to sanity-check pump activity.
- `webrtc_peers: HashMap<PeerId, WebRtcPeerEntry>`: per-peer negotiation state, RTCPeerConnection handle, and queued artifacts.
- `webrtc_event_tx/webrtc_event_rx`: tokio unbounded channel ferrying async WebRTC callbacks (data-channel opens, connection-state changes) back to the frame loop.
- `active_webrtc_peer`: tracks which peer currently owns the attached `CommandTransport::WebRtc` instance so teardown can detach cleanly.

### `WebRtcPeerEntry`

Each peer entry now tracks:

- `connection: Option<Arc<RTCPeerConnection>>` – lazily constructed when a negotiation begins.
- `state: WebRtcConnectionPhase` – coarse lifecycle marker (Idle → Negotiating → Awaiting* → Connected → Closing/Failed).
- `pending_remote_sdp: Option<SessionDescription>` – latest un-applied SDP blob; cleared after `set_remote_description` succeeds.
- `pending_ice: Vec<IceCandidate>` – candidates staged until a remote SDP exists; failed insertions are re-queued.
- `initiated_by_local: bool` – flag to help with retry heuristics.
- `has_local_data_channel: bool` – ensures we only create one outbound data channel per peer.
- `transport_attached: bool` – prevents duplicate command transport attachment when multiple open notifications land.
- `last_event: Instant` – timestamp for telemetry / staleness detection.

### Metrics

- **`signaling_events_polled`**: counter incremented on each processed signaling event (wraps at `u64::MAX`).
- Next: add per-peer negotiation timers, ICE retry counts, and connection failure tallies surfaced via telemetry.

### Runtime Event Dispatch

- `poll_signaling_events()` still processes at most one signaling response per frame with a zero-timeout poll.
- `drain_webrtc_runtime_events()` now immediately attaches `WebRtcTransport` instances once data channels report open and mirrors `RTCPeerConnectionState` into `WebRtcConnectionPhase` to drive cleanup/detach logic.
- The event channel keeps async callbacks off the render thread while retaining deterministic ordering inside the frame loop.

## WebRTC Negotiation Flow

### Offer/Answer Exchange

1. **Peer B joins room** → `poll_signaling_events()` sees `PeerJoined { peer_id: A }` and, when `local_peer_id < peer_id`, calls `initiate_webrtc_connection()`.
2. **Initiator path (`create_local_offer`)**
   - Ensures/creates an `RTCPeerConnection` and outbound data channel (`theta-command`).
   - Registers transport emission so the data channel hands us a `WebRtcTransport` once it opens.
   - Generates a local SDP offer and sends it via `signaling_send_offer()`.
3. **Responder path (`handle_webrtc_offer`)**
   - Ensures/creates an `RTCPeerConnection` (with inbound `on_data_channel` hook).
   - Sets the remote SDP, produces a local answer, and sends it with `signaling_send_answer()`.
   - Any queued ICE candidates are applied after the SDP lands.
4. **Initiator processes answer (`handle_webrtc_answer`)**
   - Applies the remote SDP and flushes queued ICE candidates.
5. **Data channel opens**
   - The async callback emits a `WebRtcRuntimeEvent::TransportEstablished`, the frame loop attaches a `CommandTransport::WebRtc`, and negotiation state flips to `Connected`.

### ICE Candidate Exchange

- Both peers emit ICE candidates via `on_ice_candidate` callback
- Each candidate is sent to the remote peer via `signaling_send_ice_candidate()`
- Remote peer receives `SignalingResponse::IceCandidate` and adds via `RTCPeerConnection::add_ice_candidate()`
- Process continues until ICE gathering completes or connectivity is established

### Connection Topology

**Option A: Full Mesh**
- Every peer initiates offers to all other peers
- Pros: Maximum redundancy, no single point of failure
- Cons: O(n²) connections, high bandwidth overhead

**Option B: Peer ID Ordering**
- Only the peer with the lexicographically lower ID initiates the offer
- Pros: Prevents duplicate offers, O(n) connections
- Cons: Requires deterministic peer ID comparison

**Option C: Star Topology (Future)**
- Designated host peer handles all relaying
- Pros: O(n) connections from client perspective
- Cons: Single point of failure, host bandwidth bottleneck

**Current Recommendation:** Option B with fallback to Option A if ordering fails

## Implementation Roadmap

### Phase 1: Scaffolding (✅ Complete)
- [x] Add `poll_signaling_events()` to frame loop
- [x] Implement event dispatch and logging
- [x] Add metrics tracking (`signaling_events_polled`)
- [x] Document TODO placeholders for WebRTC handlers

### Phase 2: WebRTC Negotiation (✅ Complete)
- [x] Promote offer/answer handlers to set local/remote SDP and drive signaling responses
- [x] Lazily instantiate `RTCPeerConnection` entries and create outbound data channels for initiators
- [x] Attach `WebRtcTransport` once data channels report `Open`
- [x] Queue and replay ICE candidates after SDP application

### Phase 3: Peer Connection Management (In Progress)
- [x] Track per-peer negotiation metadata inside `WebRtcPeerEntry`
- [ ] Surface connection-state transitions and negotiation timings via telemetry
- [ ] Support blending/hand-off between QUIC and WebRTC transports (including fallback)
- [ ] Implement retry/backoff for unanswered offers or ICE application failures

### Phase 4: Integration & Testing
- [ ] Write integration test: two engines connect via signaling and exchange commands
- [ ] Validate STUN/TURN fallback paths
- [ ] Measure connection establishment latency
- [ ] Surface WebRTC connection metrics in telemetry overlay

### Phase 5: Production Hardening
- [ ] Implement reconnection logic for transient failures
- [ ] Add connection quality monitoring (RTT, packet loss)
- [ ] Support graceful degradation (fallback to QUIC when WebRTC unavailable)
- [ ] Document NAT traversal troubleshooting guide

## Environment Configuration

Signaling bootstrap respects existing environment variables:

- `THETA_SIGNALING_URL`: External signaling server endpoint
- `THETA_SIGNALING_BIND`: Local server bind address
- `THETA_PEER_ID`: Override auto-generated peer ID
- `THETA_ROOM_ID`: Signaling room identifier
- `THETA_SIGNALING_TIMEOUT_MS`: Registration timeout
- `THETA_SIGNALING_DISABLED=1`: Disable automatic bootstrap

No additional configuration is required for event polling; it activates automatically when signaling is bootstrapped.

## Performance Considerations

### Frame Budget

- Signaling event processing should complete in <1ms to avoid frame drops
- Zero-timeout polling ensures no blocking waits
- SDP parsing and peer connection setup may take 5-10ms; consider deferring to background task

### Batching Strategy

- Current: Process one event per frame
- Future: Batch ICE candidates (multiple candidates per frame) while handling offers/answers individually

### Background Processing

Consider moving heavy operations to the network runtime:

```rust
let runtime = self.ensure_network_runtime();
runtime.spawn(async move {
    // Create peer connection, set SDP, gather ICE candidates
    // Signal completion via channel back to main thread
});
```


## References

- `src/engine/mod.rs`: `poll_signaling_events()` implementation
- `src/network/signaling.rs`: SignalingClient, SignalingResponse types
- `src/network/transport.rs`: WebRtcTransport example (loopback test)
- `docs/architecture.md`: Signaling bootstrap documentation
- `docs/phase5_parallel_plan.md`: WebRTC fallback roadmap
