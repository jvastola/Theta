#[cfg(feature = "render-wgpu")]
use crate::vr::{GpuFrameSubmission, GpuSurface};
use crate::vr::{SurfaceHandle, VrBridge, VrError, VrFrameSubmission, VrViewConfig};
use std::fmt;

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
}

#[cfg(feature = "render-wgpu")]
pub mod wgpu_backend {
    use super::*;
    use pollster::block_on;

    pub struct WgpuBackend {
        _instance: wgpu::Instance,
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

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

            Ok(Self {
                _instance: instance,
                _adapter: adapter,
                device,
                queue,
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

            let mut gpu_surfaces = Vec::with_capacity(views.view_count());
            let mut handles = Vec::with_capacity(views.view_count());

            for (eye, view) in views.views.iter().enumerate() {
                let extent = wgpu::Extent3d {
                    width: view.resolution[0].max(1),
                    height: view.resolution[1].max(1),
                    depth_or_array_layers: 1,
                };

                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Codex Eye Frame"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: COLOR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });

                let view_handle = texture.create_view(&wgpu::TextureViewDescriptor::default());

                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Codex Clear Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view_handle,
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

                let handle = SurfaceHandle {
                    id: eye as u64,
                    size: view.resolution,
                };

                handles.push(handle);
                gpu_surfaces.push(GpuSurface::new(handle, texture, view_handle));
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::{NullVrBridge, VrBridge};

    #[test]
    fn null_pipeline_advances_frame_index() {
        let backend: Box<dyn GpuBackend> = Box::new(NullGpuBackend::default());
        let vr: Box<dyn VrBridge> = Box::new(NullVrBridge::default());
        let mut renderer = Renderer::new(RendererConfig::default(), backend, vr);

        renderer
            .render(0.016)
            .expect("null pipeline should not fail");
        assert_eq!(renderer.frame_index(), 1);
    }
}
