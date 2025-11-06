#![cfg(feature = "network-quic")]

use std::sync::Arc;

use theta_engine::editor::telemetry::TelemetrySurface;
use theta_engine::engine::Engine;
use theta_engine::network::transport::WebRtcTransport;
use theta_engine::network::voice::VoiceDiagnostics;
use tokio::runtime::Builder as RuntimeBuilder;

#[test]
fn voice_metrics_become_available_after_loopback_session() {
    let runtime = Arc::new(
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );

    let (transport_a, transport_b) = runtime
        .block_on(WebRtcTransport::pair())
        .expect("loopback WebRTC pair");

    let mut engine_a = Engine::new();
    let mut engine_b = Engine::new();

    engine_a.set_network_runtime(runtime.clone());
    engine_b.set_network_runtime(runtime.clone());

    engine_a.attach_webrtc_transport(transport_a);
    engine_b.attach_webrtc_transport(transport_b);

    engine_a.configure_max_frames(120);
    engine_b.configure_max_frames(120);

    for _ in 0..3 {
        engine_a.run();
        engine_b.run();
    }

    let voice_a = latest_voice_metrics(&engine_a).expect("peer A voice metrics");
    assert!(
        voice_a.packets_sent > 0 || voice_a.packets_received > 0,
        "peer A should record voice traffic"
    );
    assert!(
        voice_a.voiced_frames > 0,
        "peer A should observe voiced frames"
    );

    let voice_b = latest_voice_metrics(&engine_b).expect("peer B voice metrics");
    assert!(
        voice_b.packets_sent > 0 || voice_b.packets_received > 0,
        "peer B should record voice traffic"
    );
    assert!(
        voice_b.voiced_frames > 0,
        "peer B should observe voiced frames"
    );
}

fn latest_voice_metrics(engine: &Engine) -> Option<VoiceDiagnostics> {
    let entity = engine.telemetry_entity()?;
    let world = engine.world();
    let surface = world.get::<TelemetrySurface>(entity)?;
    let telemetry = surface.latest()?;
    telemetry.webrtc.as_ref()?.voice.clone()
}
