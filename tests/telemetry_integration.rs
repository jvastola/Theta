use theta_engine::editor::telemetry::{FrameTelemetry, TelemetryReplicator, TelemetrySurface};
use theta_engine::engine::schedule::Stage;
use theta_engine::engine::Engine;
use theta_engine::network::DiffPayload;

#[test]
fn engine_emits_change_sets_after_running() {
    let mut engine = Engine::new();
    engine.configure_max_frames(3);
    engine.run();

    let telemetry_entity = engine
        .telemetry_entity()
        .expect("engine should create telemetry entity");
    let world = engine.world();

    let surface = world
        .get::<TelemetrySurface>(telemetry_entity)
        .expect("telemetry surface component present");
    let latest = surface.latest().expect("latest telemetry sample after run");
    assert!(latest.frame >= 1);
    assert_eq!(latest.stage_samples.len(), Stage::count());
    assert_eq!(
        latest.stage_samples[0].stage.as_str(),
        Stage::Startup.label()
    );
    let render_sample = &latest.stage_samples[Stage::Render.index()];
    assert!(!render_sample.read_only_violation);
    assert_eq!(render_sample.violation_count, 0);

    let replicator = world
        .get::<TelemetryReplicator>(telemetry_entity)
        .expect("telemetry replicator component present");
    let change_set = replicator
        .last_change_set()
        .expect("telemetry replicator should publish at least one change set");
    assert_eq!(change_set.diffs.len(), 1);

    let diff = &change_set.diffs[0];
    assert_eq!(diff.entity.index, telemetry_entity.index());
    assert_eq!(diff.entity.generation, telemetry_entity.generation());

    match &diff.payload {
        DiffPayload::Update { bytes } => {
            let decoded: FrameTelemetry =
                serde_json::from_slice(bytes).expect("telemetry payload should decode");
            assert_eq!(decoded.frame, latest.frame);
            assert_eq!(decoded.stage_samples.len(), Stage::count());
            assert!(
                decoded
                    .stage_samples
                    .iter()
                    .all(|sample| sample.rolling_ms.is_finite())
            );
            let decoded_render = &decoded.stage_samples[Stage::Render.index()];
            assert!(!decoded_render.read_only_violation);
            assert_eq!(decoded_render.violation_count, 0);
        }
        other => panic!("expected update payload, got {:?}", other),
    }
}
