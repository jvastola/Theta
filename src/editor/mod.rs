pub mod telemetry;

pub use telemetry::{FrameTelemetry, StageSample, TelemetryReplicator, TelemetrySurface};

pub struct MeshEditor;

impl MeshEditor {
    pub fn new() -> Self {
        Self
    }

    pub fn create_primitive(&mut self) {
        println!("Mesh creation placeholder");
    }
}
