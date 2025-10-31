use std::fmt;

#[cfg(feature = "vr-openxr")]
pub mod openxr;

#[cfg(feature = "render-wgpu")]
use std::sync::Arc;
#[cfg(feature = "render-wgpu")]
use std::sync::atomic::{AtomicBool, Ordering};

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

#[derive(Debug, Clone, Copy)]
pub struct TrackedPose {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
}

impl Default for TrackedPose {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            orientation: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerState {
    pub pose: TrackedPose,
    pub trigger: f32,
    pub grip: f32,
    pub buttons: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VrInputSample {
    pub head: TrackedPose,
    pub left: ControllerState,
    pub right: ControllerState,
}

pub trait VrInputProvider: Send {
    fn label(&self) -> &'static str;
    fn sample(&mut self, delta_seconds: f32) -> VrInputSample;
}

#[derive(Default)]
pub struct SimulatedInputProvider {
    elapsed: f32,
}

impl VrInputProvider for SimulatedInputProvider {
    fn label(&self) -> &'static str {
        "Simulated VR Input"
    }

    fn sample(&mut self, delta_seconds: f32) -> VrInputSample {
        self.elapsed += delta_seconds;
        let wobble = (self.elapsed * 0.5).sin() * 0.1;
        let forward = (self.elapsed * 0.75).cos() * 0.05;

        let mut sample = VrInputSample::default();
        sample.head.position = [wobble, 1.6 + forward, 0.0];
        sample.left.pose.position = [-0.25 + wobble * 0.5, 1.4, 0.2 + forward];
        sample.right.pose.position = [0.25 + wobble * 0.5, 1.4, 0.2 - forward];
        sample.left.trigger = ((self.elapsed * 0.7).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        sample.right.trigger = ((self.elapsed * 0.9).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        sample
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
    pub texture: Arc<wgpu::Texture>,
    pub view: wgpu::TextureView,
    release: Option<Arc<AtomicBool>>,
}

#[cfg(feature = "render-wgpu")]
impl GpuSurface {
    pub fn new(handle: SurfaceHandle, texture: wgpu::Texture, view: wgpu::TextureView) -> Self {
        Self {
            handle,
            texture: Arc::new(texture),
            view,
            release: None,
        }
    }

    pub(crate) fn with_release(
        handle: SurfaceHandle,
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        release: Arc<AtomicBool>,
    ) -> Self {
        Self {
            handle,
            texture,
            view,
            release: Some(release),
        }
    }
}

#[cfg(feature = "render-wgpu")]
impl Drop for GpuSurface {
    fn drop(&mut self) {
        if let Some(flag) = self.release.take() {
            flag.store(true, Ordering::Release);
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
