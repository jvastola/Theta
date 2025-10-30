use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct VrView {
    pub resolution: [u32; 2],
    pub fov: [f32; 2],
    pub transform: [[f32; 4]; 4],
}

impl VrView {
    pub fn from_resolution(resolution: [u32; 2]) -> Self {
        Self {
            resolution,
            fov: [90.0, 90.0],
            transform: identity_matrix(),
        }
    }
}

impl Default for VrView {
    fn default() -> Self {
        Self::from_resolution([0, 0])
    }
}

#[derive(Debug, Clone, Default)]
pub struct VrViewConfig {
    pub views: Vec<VrView>,
}

impl VrViewConfig {
    pub fn view_count(&self) -> usize {
        self.views.len()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceHandle {
    pub id: u64,
    pub size: [u32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct VrFrameSubmission {
    pub surfaces: Vec<SurfaceHandle>,
}

#[derive(Debug)]
pub struct VrError {
    reason: String,
}

impl VrError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for VrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for VrError {}

pub type VrResult<T> = Result<T, VrError>;

pub trait VrBridge: Send {
    fn label(&self) -> &'static str;
    fn acquire_views(&mut self) -> VrViewConfig;
    fn present(&mut self, submission: VrFrameSubmission) -> VrResult<()>;

    #[cfg(feature = "render-wgpu")]
    fn present_gpu(&mut self, submission: GpuFrameSubmission) -> VrResult<()> {
        let fallback = VrFrameSubmission {
            surfaces: submission.handles(),
        };
        self.present(fallback)
    }
}

pub struct NullVrBridge {
    eye_resolution: [u32; 2],
    expected_views: usize,
}

impl NullVrBridge {
    pub fn new(eye_resolution: [u32; 2]) -> Self {
        Self {
            eye_resolution,
            expected_views: 2,
        }
    }
}

impl Default for NullVrBridge {
    fn default() -> Self {
        Self::new([1440, 1600])
    }
}

impl VrBridge for NullVrBridge {
    fn label(&self) -> &'static str {
        "Null VR Bridge"
    }

    fn acquire_views(&mut self) -> VrViewConfig {
        VrViewConfig {
            views: (0..self.expected_views)
                .map(|_| VrView::from_resolution(self.eye_resolution))
                .collect(),
        }
    }

    fn present(&mut self, submission: VrFrameSubmission) -> VrResult<()> {
        if submission.surfaces.len() != self.expected_views {
            return Err(VrError::new(format!(
                "expected {} surfaces but received {}",
                self.expected_views,
                submission.surfaces.len()
            )));
        }
        Ok(())
    }

    #[cfg(feature = "render-wgpu")]
    fn present_gpu(&mut self, submission: GpuFrameSubmission) -> VrResult<()> {
        println!(
            "[vr] presenting {} gpu surfaces (null bridge)",
            submission.surfaces.len()
        );
        self.present(VrFrameSubmission {
            surfaces: submission.handles(),
        })
    }
}

pub struct VrContext<B: VrBridge> {
    bridge: B,
}

impl<B: VrBridge> VrContext<B> {
    pub fn new(bridge: B) -> Self {
        Self { bridge }
    }

    pub fn bridge(&self) -> &B {
        &self.bridge
    }

    pub fn bridge_mut(&mut self) -> &mut B {
        &mut self.bridge
    }
}

fn identity_matrix() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

#[cfg(feature = "render-wgpu")]
#[derive(Debug)]
pub struct GpuSurface {
    pub handle: SurfaceHandle,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

#[cfg(feature = "render-wgpu")]
impl GpuSurface {
    pub fn new(handle: SurfaceHandle, texture: wgpu::Texture, view: wgpu::TextureView) -> Self {
        Self {
            handle,
            texture,
            view,
        }
    }
}

#[cfg(feature = "render-wgpu")]
#[derive(Debug)]
pub struct GpuFrameSubmission {
    pub surfaces: Vec<GpuSurface>,
}

#[cfg(feature = "render-wgpu")]
impl GpuFrameSubmission {
    fn handles(&self) -> Vec<SurfaceHandle> {
        self.surfaces.iter().map(|surface| surface.handle).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_bridge_validates_surface_count() {
        let mut bridge = NullVrBridge::default();
        let views = bridge.acquire_views();

        let submission = VrFrameSubmission {
            surfaces: views
                .views
                .iter()
                .enumerate()
                .map(|(eye, view)| SurfaceHandle {
                    id: eye as u64,
                    size: view.resolution,
                })
                .collect(),
        };

        assert!(bridge.present(submission).is_ok());
    }

    #[test]
    fn null_bridge_rejects_empty_submission() {
        let mut bridge = NullVrBridge::default();
        let err = bridge.present(VrFrameSubmission::default()).unwrap_err();
        assert!(err.to_string().contains("expected"));
    }
}
