/// Desktop window rendering backend for macOS/Linux/Windows testing
/// Provides a fallback render target when XR hardware is unavailable

#[cfg(feature = "render-wgpu")]
use super::{ColorSpace, FrameInputs, GpuBackend, RenderError, RenderResult, RenderSubmission, WgpuContext};
#[cfg(feature = "render-wgpu")]
use crate::vr::{SurfaceHandle, VrFrameSubmission, VrViewConfig};
#[cfg(feature = "render-wgpu")]
use pollster::block_on;
#[cfg(feature = "render-wgpu")]
use std::sync::Arc;
#[cfg(feature = "render-wgpu")]
use winit::{
    event::WindowEvent,
    event_loop::{EventLoop, EventLoopWindowTarget},
    window::{Window, WindowId},
};

#[cfg(feature = "render-wgpu")]
#[derive(Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub color_space: ColorSpace,
}

#[cfg(feature = "render-wgpu")]
impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Theta Engine".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
            color_space: ColorSpace::Srgb,
        }
    }
}

#[cfg(feature = "render-wgpu")]
pub struct WindowBackend {
    config: WindowConfig,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: Option<WindowSurface>,
}

#[cfg(feature = "render-wgpu")]
struct WindowSurface {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    window: Arc<Window>,
}

#[cfg(feature = "render-wgpu")]
impl WindowBackend {
    pub fn initialize(config: WindowConfig) -> RenderResult<Self> {
        block_on(Self::initialize_async(config))
    }

    async fn initialize_async(config: WindowConfig) -> RenderResult<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::Backend(
                "failed to find a compatible GPU adapter for window rendering",
            ))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Theta Window Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|_| RenderError::Backend("failed to create wgpu device for window"))?;

        log::info!(
            "[render] window backend initialized (adapter: {:?})",
            adapter.get_info().name
        );

        Ok(Self {
            config,
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface: None,
        })
    }

    pub fn create_window_surface(&mut self, window: Arc<Window>) -> RenderResult<()> {
        let surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|_| RenderError::Backend("failed to create wgpu surface from window"))?;

        let capabilities = surface.get_capabilities(&self.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if self.config.color_space == ColorSpace::DisplayP3 {
                wgpu::PresentMode::Fifo
            } else {
                wgpu::PresentMode::AutoVsync
            },
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&self.device, &surface_config);

        self.surface = Some(WindowSurface {
            surface,
            surface_config,
            window,
        });

        log::info!(
            "[render] window surface configured ({}x{}, format: {:?})",
            size.width,
            size.height,
            format
        );

        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(ref mut surf) = self.surface {
            surf.surface_config.width = width.max(1);
            surf.surface_config.height = height.max(1);
            surf.surface.configure(&self.device, &surf.surface_config);
            log::debug!("[render] window resized to {}x{}", width, height);
        }
    }

    pub fn window(&self) -> Option<&Window> {
        self.surface.as_ref().map(|s| s.window.as_ref())
    }
}

#[cfg(feature = "render-wgpu")]
impl GpuBackend for WindowBackend {
    fn label(&self) -> &'static str {
        "Window Backend (Desktop)"
    }

    fn render_frame(
        &mut self,
        inputs: &FrameInputs,
        _views: &VrViewConfig,
    ) -> RenderResult<RenderSubmission> {
        let surface = self
            .surface
            .as_ref()
            .ok_or(RenderError::Backend("window surface not initialized"))?;

        let frame = surface
            .surface
            .get_current_texture()
            .map_err(|_err| RenderError::Backend("failed to acquire swapchain texture"))?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Theta Window Frame Encoder"),
            });

        // Simple clear pass for now - will be replaced with actual scene rendering
        {
            let time_phase = (inputs.elapsed_seconds * 0.5).sin() * 0.5 + 0.5;
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Theta Window Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: (0.05 + time_phase * 0.1) as f64,
                            g: 0.1,
                            b: (0.12 + time_phase * 0.08) as f64,
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

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        // For window rendering, we create a dummy VR submission
        // The actual presentation happens via frame.present() above
        let size = surface.window.inner_size();
        Ok(RenderSubmission {
            frame_index: inputs.frame_index,
            vr_submission: VrFrameSubmission {
                surfaces: vec![SurfaceHandle {
                    id: 0,
                    size: [size.width, size.height],
                }],
            },
            gpu_submission: None,
        })
    }

    fn wgpu_context(&self) -> Option<WgpuContext> {
        Some(WgpuContext {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
        })
    }
}

/// Event loop wrapper for desktop window rendering
#[cfg(feature = "render-wgpu")]
pub struct WindowEventLoop {
    event_loop: EventLoop<()>,
}

#[cfg(feature = "render-wgpu")]
impl WindowEventLoop {
    pub fn new() -> RenderResult<Self> {
        let event_loop = EventLoop::new()
            .map_err(|_| RenderError::Backend("failed to create event loop"))?;
        Ok(Self { event_loop })
    }

    pub fn run<F>(self, mut app_factory: F) -> RenderResult<()>
    where
        F: FnMut(&EventLoopWindowTarget<()>) -> RenderResult<Box<dyn WindowAppTrait>> + 'static,
    {
        use winit::event::{Event, StartCause};

        let mut app: Option<Box<dyn WindowAppTrait>> = None;

        self.event_loop
            .run(move |event, event_loop_target| {
                match event {
                    Event::NewEvents(StartCause::Init) => {
                        match app_factory(event_loop_target) {
                            Ok(new_app) => {
                                log::info!("[render] window application initialized");
                                app = Some(new_app);
                            }
                            Err(err) => {
                                log::error!("[render] failed to initialize window app: {err}");
                                event_loop_target.exit();
                            }
                        }
                    }
                    Event::WindowEvent { window_id, event } => {
                        if let Some(app) = app.as_mut() {
                            app.handle_window_event(event_loop_target, window_id, event);
                        }
                    }
                    Event::AboutToWait => {
                        if let Some(app) = app.as_mut() {
                            if let Err(err) = app.render_frame() {
                                log::error!("[render] frame error: {err}");
                                event_loop_target.exit();
                            }
                        }
                    }
                    _ => {}
                }
            })
            .map_err(|_| RenderError::Backend("event loop terminated with error"))?;

        Ok(())
    }
}

/// Trait for window application implementations
#[cfg(feature = "render-wgpu")]
pub trait WindowAppTrait {
    fn handle_window_event(
        &mut self,
        event_loop: &EventLoopWindowTarget<()>,
        window_id: WindowId,
        event: WindowEvent,
    );
    fn render_frame(&mut self) -> RenderResult<()>;
}

/// Application state for window rendering
#[cfg(feature = "render-wgpu")]
pub struct WindowApp {
    window: Arc<Window>,
    backend: WindowBackend,
    frame_index: u64,
    elapsed_seconds: f32,
    last_frame: std::time::Instant,
}

#[cfg(feature = "render-wgpu")]
impl WindowApp {
    pub fn new(event_loop: &EventLoopWindowTarget<()>, config: WindowConfig) -> RenderResult<Self> {
        use winit::dpi::LogicalSize;

        let window = winit::window::WindowBuilder::new()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(config.width, config.height))
            .with_resizable(config.resizable)
            .build(event_loop)
            .map_err(|_| RenderError::Backend("failed to create window"))?;

        let window = Arc::new(window);

        let mut backend = WindowBackend::initialize(config)?;
        backend.create_window_surface(Arc::clone(&window))?;

        Ok(Self {
            window,
            backend,
            frame_index: 0,
            elapsed_seconds: 0.0,
            last_frame: std::time::Instant::now(),
        })
    }
}

#[cfg(feature = "render-wgpu")]
impl WindowAppTrait for WindowApp {
    fn handle_window_event(
        &mut self,
        event_loop: &EventLoopWindowTarget<()>,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("[render] window close requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.backend.resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.render_frame() {
                    log::error!("[render] redraw failed: {err}");
                }
            }
            _ => {}
        }
    }

    fn render_frame(&mut self) -> RenderResult<()> {
        let now = std::time::Instant::now();
        let delta_seconds = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        self.frame_index += 1;
        self.elapsed_seconds += delta_seconds;

        let inputs = FrameInputs {
            frame_index: self.frame_index,
            delta_seconds,
            elapsed_seconds: self.elapsed_seconds,
        };

        // Use a single dummy view config for window rendering
        let views = VrViewConfig {
            views: vec![crate::vr::VrView {
                resolution: {
                    let size = self.window.inner_size();
                    [size.width, size.height]
                },
                fov: [90.0, 90.0],
                transform: identity_matrix(),
            }],
        };

        self.backend.render_frame(&inputs, &views)?;
        self.window.request_redraw();

        Ok(())
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

#[cfg(test)]
#[cfg(feature = "render-wgpu")]
mod tests {
    use super::*;

    #[test]
    fn window_backend_initializes() {
        let config = WindowConfig::default();
        let backend = WindowBackend::initialize(config);
        assert!(backend.is_ok(), "window backend should initialize");
    }
}
