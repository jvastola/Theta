pub mod telemetry;

pub use telemetry::{
    FrameTelemetry, StageSample, TelemetryOverlay, TelemetryReplicator, TelemetrySurface,
};
pub mod commands;
pub use commands::{CMD_SELECTION_HIGHLIGHT, SelectionHighlightCommand};

pub struct MeshEditor {
    telemetry_overlay: TelemetryOverlay,
}

impl MeshEditor {
    pub fn new() -> Self {
        Self {
            telemetry_overlay: TelemetryOverlay::default(),
        }
    }

    pub fn create_primitive(&mut self) {
        println!("Mesh creation placeholder");
    }

    pub fn telemetry_overlay(&self) -> &TelemetryOverlay {
        &self.telemetry_overlay
    }

    pub fn telemetry_overlay_mut(&mut self) -> &mut TelemetryOverlay {
        &mut self.telemetry_overlay
    }
}

impl Default for MeshEditor {
    fn default() -> Self {
        Self::new()
    }
}
