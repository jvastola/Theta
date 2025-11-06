use crate::vr::{SimulatedInputProvider, VrError, VrInputProvider, VrInputSample};
use openxr::{ApplicationInfo, Entry, ExtensionSet, FormFactor, Instance};

pub struct OpenXrInputProvider {
    instance: Instance,
    system_id: openxr::SystemId,
    fallback: SimulatedInputProvider,
    runtime_active: bool,
}

impl OpenXrInputProvider {
    pub fn initialize() -> Result<Self, VrError> {
        let entry = Entry::load()
            .map_err(|err| VrError::new(format!("failed to load OpenXR loader: {err}")))?;
        let app_info = ApplicationInfo {
            application_name: "Theta Engine",
            application_version: 1,
            engine_name: "Theta Engine",
            engine_version: 1,
        };

        let enabled_extensions = ExtensionSet::default();

        let instance = entry
            .create_instance(&app_info, &enabled_extensions, &[])
            .map_err(|err| VrError::new(format!("failed to create OpenXR instance: {err}")))?;

        let system_id = instance
            .system(FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|err| VrError::new(format!("failed to query OpenXR system: {err}")))?;

        Ok(Self {
            instance,
            system_id,
            fallback: SimulatedInputProvider::default(),
            runtime_active: true,
        })
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn system_id(&self) -> openxr::SystemId {
        self.system_id
    }
}

impl VrInputProvider for OpenXrInputProvider {
    fn label(&self) -> &'static str {
        "OpenXR"
    }

    fn sample(&mut self, delta_seconds: f32) -> VrInputSample {
        if !self.runtime_active {
            return self.fallback.sample(delta_seconds);
        }

        // TODO: poll OpenXR actions once swapchains and sessions are wired in.
        // For now, provide simulated data so downstream systems exercise the pipeline.
        self.fallback.sample(delta_seconds)
    }
}
