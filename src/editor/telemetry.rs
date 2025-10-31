use crate::ecs::Entity;
use crate::engine::schedule::Stage;
use crate::network::{ChangeSet, ComponentDiff, DiffPayload, NetworkSession};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageSample {
    pub stage: String,
    pub total_ms: f32,
    pub sequential_ms: f32,
    pub parallel_ms: f32,
    pub read_only_violation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
                stage: stage.label().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::DiffPayload;

    fn sample_arrays(frame: u64) -> FrameTelemetry {
        FrameTelemetry::from_stage_arrays(
            frame,
            4.2,
            &[1.0, 2.0, 3.0, 4.0],
            &[0.5, 1.0, 1.5, 2.0],
            &[0.25, 0.5, 0.75, 1.0],
            &[false, true, false, true],
            [0.1, 0.9],
        )
    }

    #[test]
    fn frame_samples_preserve_stage_metadata() {
        let telemetry = sample_arrays(7);
        assert_eq!(telemetry.stage_samples.len(), Stage::count());
        for (sample, stage) in telemetry.stage_samples.iter().zip(Stage::ordered()) {
            assert_eq!(sample.stage, stage.label());
        }
        assert_eq!(telemetry.frame, 7);
        assert_eq!(telemetry.controller_trigger, [0.1, 0.9]);
        assert!(telemetry.stage_samples[1].read_only_violation);
    }

    #[test]
    fn telemetry_surface_detects_new_frames() {
        let mut surface = TelemetrySurface::default();

        let first = sample_arrays(1);
        assert!(surface.record(first));

        let same_frame = sample_arrays(1);
        assert!(!surface.record(same_frame));

        let next = sample_arrays(2);
        assert!(surface.record(next.clone()));
        assert_eq!(surface.latest().map(|t| t.frame), Some(2));
    }

    #[test]
    fn replicator_serializes_change_sets() {
        let entity = Entity::new(12, 3);
        let telemetry = sample_arrays(5);
        let mut replicator = TelemetryReplicator::default();

        replicator.publish(entity, &telemetry);

        let change_set = replicator
            .last_change_set()
            .expect("replicator should cache a change set");
        assert_eq!(change_set.sequence, 1);
        assert_eq!(change_set.diffs.len(), 1);

        let diff = &change_set.diffs[0];
        assert_eq!(diff.entity.index, entity.index());
        assert_eq!(diff.entity.generation, entity.generation());

        match &diff.payload {
            DiffPayload::Update { bytes } => {
                let round_trip: FrameTelemetry =
                    serde_json::from_slice(bytes).expect("telemetry should deserialize");
                assert_eq!(round_trip.frame, telemetry.frame);
                assert_eq!(round_trip.stage_samples.len(), Stage::count());
            }
            _ => panic!("replicator should emit update payloads"),
        }

        let follow_up = sample_arrays(6);
        replicator.publish(entity, &follow_up);
        let next_change = replicator
            .last_change_set()
            .expect("replicator should store newest change set");
        assert_eq!(next_change.sequence, 2);
    }
}
