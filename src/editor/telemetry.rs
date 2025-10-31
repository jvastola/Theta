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
    pub rolling_ms: f32,
    pub read_only_violation: bool,
    pub violation_count: u32,
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
        stage_rolling_ms: &[f32; Stage::count()],
        stage_read_only_violation: &[bool; Stage::count()],
        stage_violation_count: &[u32; Stage::count()],
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
                rolling_ms: stage_rolling_ms[index],
                read_only_violation: stage_read_only_violation[index],
                violation_count: stage_violation_count[index],
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
    initialized: bool,
}

impl Default for TelemetryReplicator {
    fn default() -> Self {
        Self {
            session: NetworkSession::connect(),
            last_change_set: None,
            initialized: false,
        }
    }
}

impl TelemetryReplicator {
    pub fn publish(&mut self, entity: Entity, telemetry: &FrameTelemetry) {
        match serde_json::to_vec(telemetry) {
            Ok(bytes) => {
                let diff = if self.initialized {
                    ComponentDiff::update::<TelemetryComponent>(entity, bytes)
                } else {
                    ComponentDiff::insert::<TelemetryComponent>(entity, bytes)
                };
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
                self.initialized = true;
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
    use crate::network::{ChangeSet, DiffPayload};
    use proptest::prelude::*;

    fn build_sample(
        frame: u64,
        average: f32,
        totals: [f32; Stage::count()],
        sequential: [f32; Stage::count()],
        parallel: [f32; Stage::count()],
        rolling: [f32; Stage::count()],
        violations: [bool; Stage::count()],
        violation_counts: [u32; Stage::count()],
        triggers: [f32; 2],
    ) -> FrameTelemetry {
        FrameTelemetry::from_stage_arrays(
            frame,
            average,
            &totals,
            &sequential,
            &parallel,
            &rolling,
            &violations,
            &violation_counts,
            triggers,
        )
    }

    fn static_sample(frame: u64) -> FrameTelemetry {
        build_sample(
            frame,
            4.2,
            [1.0, 2.0, 3.0, 4.0],
            [0.5, 1.0, 1.5, 2.0],
            [0.25, 0.5, 0.75, 1.0],
            [1.0, 2.0, 3.0, 4.0],
            [false, true, false, true],
            [0, 1, 0, 2],
            [0.1, 0.9],
        )
    }

    #[test]
    fn frame_samples_preserve_stage_metadata() {
        let telemetry = static_sample(7);
        assert_eq!(telemetry.stage_samples.len(), Stage::count());
        for (sample, stage) in telemetry.stage_samples.iter().zip(Stage::ordered()) {
            assert_eq!(sample.stage.as_str(), stage.label());
        }
        assert_eq!(telemetry.frame, 7);
        assert_eq!(telemetry.controller_trigger, [0.1, 0.9]);
        assert!(telemetry.stage_samples[1].read_only_violation);
        assert_eq!(telemetry.stage_samples[0].rolling_ms, 1.0);
        assert_eq!(telemetry.stage_samples[1].violation_count, 1);
    }

    #[test]
    fn telemetry_surface_detects_new_frames() {
        let mut surface = TelemetrySurface::default();

        let first = static_sample(1);
        assert!(surface.record(first));

        let same_frame = static_sample(1);
        assert!(!surface.record(same_frame));

        let next = static_sample(2);
        assert!(surface.record(next.clone()));
        assert_eq!(surface.latest().map(|t| t.frame), Some(2));
    }

    #[test]
    fn replicator_serializes_change_sets() {
        let entity = Entity::new(12, 3);
        let telemetry = static_sample(5);
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
            DiffPayload::Insert { bytes } => {
                let round_trip: FrameTelemetry =
                    serde_json::from_slice(bytes).expect("telemetry should deserialize");
                assert_eq!(round_trip.frame, telemetry.frame);
                assert_eq!(round_trip.stage_samples.len(), Stage::count());
                assert_eq!(
                    round_trip.stage_samples[3].violation_count,
                    telemetry.stage_samples[3].violation_count
                );
            }
            other => panic!("expected insert payload, got {:?}", other),
        }

        let follow_up = static_sample(6);
        replicator.publish(entity, &follow_up);
        let next_change = replicator
            .last_change_set()
            .expect("replicator should store newest change set");
        assert_eq!(next_change.sequence, 2);

        match &next_change.diffs[0].payload {
            DiffPayload::Update { bytes } => {
                let round_trip: FrameTelemetry =
                    serde_json::from_slice(bytes).expect("telemetry should deserialize");
                assert_eq!(round_trip.frame, follow_up.frame);
            }
            other => panic!("expected update payload, got {:?}", other),
        }
    }

    proptest::prop_compose! {
        fn stage_arrays()(values in proptest::array::uniform4(-2000i16..2000i16)) -> [f32; Stage::count()] {
            values.map(|v| v as f32 / 10.0)
        }
    }

    proptest::prop_compose! {
        fn triggers()(values in proptest::array::uniform2(-100i16..100i16)) -> [f32; 2] {
            values.map(|v| v as f32 / 10.0)
        }
    }

    proptest::prop_compose! {
        fn violation_counts()(values in proptest::array::uniform4(0u16..200u16)) -> [u32; Stage::count()] {
            values.map(|v| v as u32)
        }
    }

    proptest::proptest! {
        #[test]
        fn replicator_sequences_monotonic_under_interleaving(
            ops in proptest::collection::vec(
                (
                    0usize..3,
                    0u64..128,
                    -500i16..500i16,
                    stage_arrays(),
                    stage_arrays(),
                    stage_arrays(),
                    stage_arrays(),
                    proptest::array::uniform4(any::<bool>()),
                    violation_counts(),
                    triggers(),
                ),
                1..32
            )
        ) {
            const PUBLISHERS: usize = 3;
            let mut replicators: [TelemetryReplicator; PUBLISHERS] = [
                TelemetryReplicator::default(),
                TelemetryReplicator::default(),
                TelemetryReplicator::default(),
            ];
            let entities = [
                Entity::new(101, 0),
                Entity::new(202, 0),
                Entity::new(303, 0),
            ];
            let mut history: [Vec<ChangeSet>; PUBLISHERS] = std::array::from_fn(|_| Vec::new());
            let mut expectations: [Vec<FrameTelemetry>; PUBLISHERS] = std::array::from_fn(|_| Vec::new());

            for (publisher, frame, avg_i16, totals, sequential, parallel, rolling, violations, counts, trigger_vals) in ops {
                let index = publisher % PUBLISHERS;
                let average = (avg_i16 as f32).abs() / 100.0 + 0.001;
                let telemetry = build_sample(
                    frame,
                    average,
                    totals,
                    sequential,
                    parallel,
                    rolling,
                    violations,
                    counts,
                    trigger_vals,
                );

                replicators[index].publish(entities[index], &telemetry);
                history[index].push(
                    replicators[index]
                        .last_change_set()
                        .expect("change set after publish")
                        .clone(),
                );
                expectations[index].push(telemetry);
            }

            for (index, changes) in history.iter().enumerate() {
                for (sequence_idx, change) in changes.iter().enumerate() {
                    let expected_frame = &expectations[index][sequence_idx];
                    assert_eq!(change.sequence, (sequence_idx as u64) + 1, "publisher {} sequence mismatch", index);
                    assert_eq!(change.diffs.len(), 1);
                    let diff = &change.diffs[0];
                    assert_eq!(diff.entity.index, entities[index].index());
                    assert_eq!(diff.entity.generation, entities[index].generation());
                    match (&diff.payload, sequence_idx == 0) {
                        (DiffPayload::Insert { bytes }, true) | (DiffPayload::Update { bytes }, false) => {
                            let restored: FrameTelemetry = serde_json::from_slice(bytes)
                                .expect("telemetry payload should deserialize");
                            assert_eq!(restored.stage_samples.len(), Stage::count());
                            assert_eq!(restored.frame, expected_frame.frame);
                            for (restored_sample, expected_sample) in restored
                                .stage_samples
                                .iter()
                                .zip(expected_frame.stage_samples.iter())
                            {
                                assert_eq!(restored_sample.rolling_ms, expected_sample.rolling_ms);
                                assert_eq!(
                                    restored_sample.violation_count,
                                    expected_sample.violation_count
                                );
                            }
                        }
                        (payload, is_first) => panic!(
                            "unexpected payload {:?} at position {} (is_first={})",
                            payload,
                            sequence_idx,
                            is_first
                        ),
                    }
                }
            }
        }
    }
}
