use crate::ecs::Entity;
use crate::engine::schedule::Stage;
use crate::network::{ChangeSet, ComponentDiff, DiffPayload, NetworkSession};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StageSample {
    pub stage: &'static str,
    pub total_ms: f32,
    pub sequential_ms: f32,
    pub parallel_ms: f32,
    pub read_only_violation: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameTelemetry {
    pub frame: u64,
    pub average_frame_time: f32,
    pub stage_samples: Vec<StageSample>,
    pub controller_trigger: [f32; 2],
}

impl FrameTelemetry {
    #[allow(clippy::too_many_arguments)]
    pub fn from_stage_arrays(
        frame: u64,
        average_frame_time: f32,
        stage_total_ms: &[f32; Stage::count()],
        stage_sequential_ms: &[f32; Stage::count()],
        stage_parallel_ms: &[f32; Stage::count()],
        stage_read_only_violation: &[bool; Stage::count()],
        controller_trigger: [f32; 2],
    ) -> Self {
        let stage_samples = Stage::ordered()
            .iter()
            .enumerate()
            .map(|(index, stage)| StageSample {
                stage: stage.label(),
                total_ms: stage_total_ms[index],
                sequential_ms: stage_sequential_ms[index],
                parallel_ms: stage_parallel_ms[index],
                read_only_violation: stage_read_only_violation[index],
            })
            .collect();

        Self {
            frame,
            average_frame_time,
            stage_samples,
            controller_trigger,
        }
    }
}

#[derive(Default)]
pub struct TelemetrySurface {
    latest: Option<FrameTelemetry>,
    last_frame: Option<u64>,
}

impl TelemetrySurface {
    pub fn record(&mut self, telemetry: FrameTelemetry) -> bool {
        let frame = telemetry.frame;
        let changed = self.last_frame.map_or(true, |last| last != frame);
        self.last_frame = Some(frame);
        self.latest = Some(telemetry);
        changed
    }

    pub fn latest(&self) -> Option<&FrameTelemetry> {
        self.latest.as_ref()
    }
}

pub struct TelemetryReplicator {
    session: NetworkSession,
    last_change_set: Option<ChangeSet>,
}

impl Default for TelemetryReplicator {
    fn default() -> Self {
        Self {
            session: NetworkSession::connect(),
            last_change_set: None,
        }
    }
}

impl TelemetryReplicator {
    pub fn publish(&mut self, entity: Entity, telemetry: &FrameTelemetry) {
        match serde_json::to_vec(telemetry) {
            Ok(bytes) => {
                let diff = ComponentDiff::update::<TelemetryComponent>(entity, bytes);
                let change_set = self.session.craft_change_set(vec![diff]);
                let payload_size = change_set
                    .diffs
                    .first()
                    .map(|diff| match &diff.payload {
                        DiffPayload::Insert { bytes } | DiffPayload::Update { bytes } => {
                            bytes.len()
                        }
                        DiffPayload::Remove => 0,
                    })
                    .unwrap_or_default();

                println!(
                    "[telemetry] frame {} replicated (seq {}, {} bytes)",
                    telemetry.frame, change_set.sequence, payload_size
                );

                self.last_change_set = Some(change_set);
            }
            Err(err) => {
                eprintln!(
                    "[telemetry] failed to serialize frame {}: {err}",
                    telemetry.frame
                );
            }
        }
    }

    pub fn last_change_set(&self) -> Option<&ChangeSet> {
        self.last_change_set.as_ref()
    }
}

pub struct TelemetryComponent;
