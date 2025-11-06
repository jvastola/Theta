#[cfg(feature = "render-wgpu")]
use crate::vr::{GpuFrameSubmission, GpuSurface};
use crate::vr::{SurfaceHandle, VrBridge, VrError, VrFrameSubmission, VrViewConfig};
use std::fmt;
#[cfg(feature = "render-wgpu")]
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Null,
    Wgpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
}

#[derive(Debug, Clone, Copy)]
pub struct RendererConfig {
    pub backend: BackendKind,
    pub enable_vsync: bool,
    pub color_space: ColorSpace,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            backend: BackendKind::Null,
            enable_vsync: true,
            color_space: ColorSpace::Srgb,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FrameInputs {
    pub frame_index: u64,
    pub delta_seconds: f32,
    pub elapsed_seconds: f32,
}

#[derive(Debug)]
pub struct RenderSubmission {
    pub frame_index: u64,
    pub vr_submission: VrFrameSubmission,
    #[cfg(feature = "render-wgpu")]
    pub gpu_submission: Option<GpuFrameSubmission>,
}

#[cfg(feature = "render-wgpu")]
#[derive(Clone)]
pub struct WgpuContext {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

#[derive(Debug)]
pub enum RenderError {
    Vr(VrError),
    FrameOutOfOrder { expected: u64, got: u64 },
    Backend(&'static str),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Vr(err) => write!(f, "vr bridge error: {err}"),
            RenderError::FrameOutOfOrder { expected, got } => write!(
                f,
                "renderer expected frame {expected} but backend produced {got}"
            ),
            RenderError::Backend(reason) => write!(f, "gpu backend failure: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<VrError> for RenderError {
    fn from(value: VrError) -> Self {
        RenderError::Vr(value)
    }
}

pub type RenderResult<T> = Result<T, RenderError>;

pub trait GpuBackend: Send {
    fn label(&self) -> &'static str;
    fn render_frame(
        &mut self,
        inputs: &FrameInputs,
        views: &VrViewConfig,
    ) -> RenderResult<RenderSubmission>;

    #[cfg(feature = "render-wgpu")]
    fn wgpu_context(&self) -> Option<WgpuContext> {
        None
    }
}

pub struct Renderer {
    config: RendererConfig,
    backend: Box<dyn GpuBackend>,
    vr: Box<dyn VrBridge>,
    frame_index: u64,
    elapsed_seconds: f32,
}

impl Renderer {
    pub fn new(
        config: RendererConfig,
        backend: Box<dyn GpuBackend>,
        vr: Box<dyn VrBridge>,
    ) -> Self {
        Self {
            config,
            backend,
            vr,
            frame_index: 0,
            elapsed_seconds: 0.0,
        }
    }

    pub fn render(&mut self, delta_seconds: f32) -> RenderResult<()> {
        let next_index = self.frame_index + 1;
        let elapsed = self.elapsed_seconds + delta_seconds;
        let views = self.vr.acquire_views();
        let inputs = FrameInputs {
            frame_index: next_index,
            delta_seconds,
            elapsed_seconds: elapsed,
        };

        let submission = self.backend.render_frame(&inputs, &views)?;
        #[cfg(feature = "render-wgpu")]
        let RenderSubmission {
            frame_index,
            vr_submission,
            gpu_submission,
        } = submission;

        #[cfg(not(feature = "render-wgpu"))]
        let RenderSubmission {
            frame_index,
            vr_submission,
        } = submission;
        if frame_index != next_index {
            return Err(RenderError::FrameOutOfOrder {
                expected: next_index,
                got: frame_index,
            });
        }

        #[cfg(feature = "render-wgpu")]
        {
            if let Some(gpu_submission) = gpu_submission {
                self.vr.present_gpu(gpu_submission)?;
            } else {
                self.vr.present(vr_submission)?;
            }
        }

        #[cfg(not(feature = "render-wgpu"))]
        {
            self.vr.present(vr_submission)?;
        }
        self.frame_index = next_index;
        self.elapsed_seconds = elapsed;
        Ok(())
    }

    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    pub fn backend_label(&self) -> &'static str {
        self.backend.label()
    }

    pub fn vr_label(&self) -> &'static str {
        self.vr.label()
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }
}

#[derive(Default)]
pub struct NullGpuBackend;

impl GpuBackend for NullGpuBackend {
    fn label(&self) -> &'static str {
        "Null GPU Backend"
    }

    fn render_frame(
        &mut self,
        inputs: &FrameInputs,
        views: &VrViewConfig,
    ) -> RenderResult<RenderSubmission> {
        let surfaces = views
            .views
            .iter()
            .enumerate()
            .map(|(eye, view)| SurfaceHandle {
                id: eye as u64,
                size: view.resolution,
            })
            .collect::<Vec<_>>();

        println!(
            "[renderer] frame {} (Δ {:.3} s) - producing {} surfaces",
            inputs.frame_index,
            inputs.delta_seconds,
            surfaces.len()
        );

        Ok(RenderSubmission {
            frame_index: inputs.frame_index,
            vr_submission: VrFrameSubmission { surfaces },
            #[cfg(feature = "render-wgpu")]
            gpu_submission: None,
        })
    }

    #[cfg(feature = "render-wgpu")]
    fn wgpu_context(&self) -> Option<WgpuContext> {
        None
    }
}

#[cfg(feature = "render-wgpu")]
pub mod wgpu_backend {
    use super::*;
    use pollster::block_on;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub struct WgpuBackend {
        _instance: wgpu::Instance,
        _adapter: wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        swapchains: Vec<EyeSwapchain>,
    }

    const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    const EYE_BUFFER_COUNT: usize = 3;

    impl WgpuBackend {
        pub fn initialize() -> RenderResult<Self> {
            block_on(Self::initialize_async())
        }

        async fn initialize_async() -> RenderResult<Self> {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok_or(RenderError::Backend(
                    "failed to find a compatible GPU adapter",
                ))?;

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Codex WGPU Device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_defaults(),
                    },
                    None,
                )
                .await
                .map_err(|_| RenderError::Backend("failed to create wgpu device"))?;

            let device = Arc::new(device);
            let queue = Arc::new(queue);

            Ok(Self {
                _instance: instance,
                _adapter: adapter,
                device,
                queue,
                swapchains: Vec::new(),
            })
        }
    }

    impl GpuBackend for WgpuBackend {
        fn label(&self) -> &'static str {
            "WGPU Backend"
        }

        fn render_frame(
            &mut self,
            inputs: &FrameInputs,
            views: &VrViewConfig,
        ) -> RenderResult<RenderSubmission> {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Codex Frame Encoder"),
                });

            if self.swapchains.len() < views.view_count() {
                let current_len = self.swapchains.len();
                self.swapchains
                    .extend((current_len..views.view_count()).map(EyeSwapchain::new));
            }

            let mut gpu_surfaces = Vec::with_capacity(views.view_count());
            let mut handles = Vec::with_capacity(views.view_count());

            for (eye, view) in views.views.iter().enumerate() {
                let swapchain = &mut self.swapchains[eye];
                let AcquiredImage {
                    handle,
                    texture,
                    view,
                    release,
                } = swapchain.acquire(&self.device, view.resolution, COLOR_FORMAT);

                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Codex Clear Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: if eye == 0 { 0.1 } else { 0.05 },
                                    g: 0.1,
                                    b: 0.12,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                }
                handles.push(handle);
                gpu_surfaces.push(GpuSurface::with_release(handle, texture, view, release));
            }

            self.queue.submit(std::iter::once(encoder.finish()));

            Ok(RenderSubmission {
                frame_index: inputs.frame_index,
                vr_submission: VrFrameSubmission { surfaces: handles },
                gpu_submission: Some(GpuFrameSubmission {
                    surfaces: gpu_surfaces,
                }),
            })
        }

        fn wgpu_context(&self) -> Option<WgpuContext> {
            Some(WgpuContext {
                device: Arc::clone(&self.device),
                queue: Arc::clone(&self.queue),
            })
        }
    }

    struct SwapchainSlot {
        texture: Arc<wgpu::Texture>,
        fence: Arc<AtomicBool>,
    }

    struct EyeSwapchain {
        eye_index: usize,
        size: [u32; 2],
        slots: Vec<SwapchainSlot>,
        cursor: usize,
    }

    impl EyeSwapchain {
        fn new(eye_index: usize) -> Self {
            Self {
                eye_index,
                size: [0, 0],
                slots: Vec::new(),
                cursor: 0,
            }
        }

        fn acquire(
            &mut self,
            device: &wgpu::Device,
            size: [u32; 2],
            format: wgpu::TextureFormat,
        ) -> AcquiredImage {
            if self.size != size || self.slots.is_empty() {
                self.resize(device, size, format);
            }

            let slot_index = self
                .next_available_slot()
                .unwrap_or_else(|| self.reuse_in_flight_slot());

            let slot = &self.slots[slot_index];

            let texture = Arc::clone(&slot.texture);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            AcquiredImage {
                handle: SurfaceHandle {
                    id: self.eye_index as u64,
                    size,
                },
                texture,
                view,
                release: slot.fence.clone(),
            }
        }

        fn resize(&mut self, device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) {
            self.size = size;
            self.slots = (0..EYE_BUFFER_COUNT)
                .map(|_| {
                    let extent = wgpu::Extent3d {
                        width: size[0].max(1),
                        height: size[1].max(1),
                        depth_or_array_layers: 1,
                    };

                    let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Codex Eye Swapchain Image"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    }));
                    SwapchainSlot {
                        texture,
                        fence: Arc::new(AtomicBool::new(true)),
                    }
                })
                .collect();
            self.cursor = 0;
        }

        fn next_available_slot(&mut self) -> Option<usize> {
            if self.slots.is_empty() {
                return None;
            }

            let slot_count = self.slots.len();
            for offset in 0..slot_count {
                let index = (self.cursor + offset) % slot_count;
                let fence = &self.slots[index].fence;
                if fence
                    .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    self.cursor = (index + 1) % slot_count;
                    return Some(index);
                }
            }

            None
        }

        fn reuse_in_flight_slot(&mut self) -> usize {
            assert!(
                !self.slots.is_empty(),
                "swapchain slots must be initialized before reuse"
            );

            let index = self.cursor % self.slots.len();
            let fence = &self.slots[index].fence;
            let _ = fence.swap(false, Ordering::AcqRel);
            eprintln!(
                "[render] eye {} buffer pool exhausted; reusing in-flight image (expect reprojection)",
                self.eye_index
            );
            self.cursor = (index + 1) % self.slots.len();
            index
        }
    }

    struct AcquiredImage {
        handle: SurfaceHandle,
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        release: Arc<AtomicBool>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::{NullVrBridge, VrBridge};
    use std::sync::{Arc, Mutex};

    #[test]
    fn null_pipeline_advances_frame_index() {
        let backend: Box<dyn GpuBackend> = Box::new(NullGpuBackend);
        let vr: Box<dyn VrBridge> = Box::new(NullVrBridge::default());
        let mut renderer = Renderer::new(RendererConfig::default(), backend, vr);

        renderer
            .render(0.016)
            .expect("null pipeline should not fail");
        assert_eq!(renderer.frame_index(), 1);
    }

    struct TestBackend {
        forced_frame_index: Option<u64>,
        log: Option<Arc<Mutex<Vec<FrameInputs>>>>,
    }

    impl TestBackend {
        fn new() -> Self {
            Self {
                forced_frame_index: None,
                log: None,
            }
        }

        fn with_forced_index(mut self, frame_index: u64) -> Self {
            self.forced_frame_index = Some(frame_index);
            self
        }

        fn with_log(mut self, log: Arc<Mutex<Vec<FrameInputs>>>) -> Self {
            self.log = Some(log);
            self
        }
    }

    impl GpuBackend for TestBackend {
        fn label(&self) -> &'static str {
            "Test Backend"
        }

        fn render_frame(
            &mut self,
            inputs: &FrameInputs,
            views: &VrViewConfig,
        ) -> RenderResult<RenderSubmission> {
            if let Some(log) = &self.log {
                log.lock().unwrap().push(*inputs);
            }

            let frame_index = self.forced_frame_index.unwrap_or(inputs.frame_index);
            let surfaces = views
                .views
                .iter()
                .enumerate()
                .map(|(eye, view)| SurfaceHandle {
                    id: eye as u64,
                    size: view.resolution,
                })
                .collect();

            Ok(RenderSubmission {
                frame_index,
                vr_submission: VrFrameSubmission { surfaces },
                #[cfg(feature = "render-wgpu")]
                gpu_submission: None,
            })
        }
    }

    #[test]
    fn renderer_detects_out_of_order_frames() {
        let backend: Box<dyn GpuBackend> = Box::new(TestBackend::new().with_forced_index(0));
        let vr: Box<dyn VrBridge> = Box::new(NullVrBridge::default());
        let mut renderer = Renderer::new(RendererConfig::default(), backend, vr);

        let err = renderer
            .render(0.016)
            .expect_err("should detect out-of-order frames");
        match err {
            RenderError::FrameOutOfOrder { expected, got } => {
                assert_eq!(expected, 1);
                assert_eq!(got, 0);
            }
            other => panic!("unexpected render error: {other}"),
        }
    }

    #[test]
    fn renderer_passes_delta_and_elapsed_to_backend() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let backend: Box<dyn GpuBackend> = Box::new(TestBackend::new().with_log(log.clone()));
        let vr: Box<dyn VrBridge> = Box::new(NullVrBridge::default());
        let mut renderer = Renderer::new(RendererConfig::default(), backend, vr);

        renderer.render(0.25).expect("first frame OK");
        renderer.render(0.5).expect("second frame OK");

        let records = log.lock().unwrap().clone();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].frame_index, 1);
        assert_eq!(records[0].delta_seconds, 0.25);
        assert_eq!(records[0].elapsed_seconds, 0.25);
        assert_eq!(records[1].frame_index, 2);
        assert_eq!(records[1].elapsed_seconds, 0.75);
    }
}
