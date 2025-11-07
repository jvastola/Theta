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
use wgpu::util::DeviceExt;
#[cfg(feature = "render-wgpu")]
use winit::{
    event::WindowEvent,
    event_loop::{EventLoop, EventLoopWindowTarget},
    window::{Window, WindowId},
};

#[cfg(feature = "render-wgpu")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StereoMode {
    /// No stereoscopic rendering - single viewport
    Mono,
    /// Side-by-side stereo - left eye on left half, right eye on right half
    SideBySide,
    /// Top-bottom stereo - left eye on top half, right eye on bottom half
    TopBottom,
}

#[cfg(feature = "render-wgpu")]
#[derive(Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub color_space: ColorSpace,
    pub stereo_mode: StereoMode,
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
            stereo_mode: StereoMode::Mono,
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
    geometry_pipeline: Option<GeometryPipeline>,
    initialized_for_rendering: bool,
}

#[cfg(feature = "render-wgpu")]
struct GeometryPipeline {
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_projection: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[cfg(feature = "render-wgpu")]
struct WindowSurface {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    window: Arc<Window>,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
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
            geometry_pipeline: None,
            initialized_for_rendering: false,
        })
    }

    fn create_geometry_pipeline(&self, surface_format: wgpu::TextureFormat) -> GeometryPipeline {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Theta Geometry Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/geometry.wgsl").into()),
        });

        // Create uniform buffer
        let uniform_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Theta Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group layout
        let bind_group_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Theta Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create bind group
        let uniform_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Theta Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let render_pipeline_layout =
            self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Theta Geometry Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Theta Geometry Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back), // Re-enable back-face culling
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        // Create a colored cube
        // Each face has a different color: front=red, back=cyan, left=green, right=magenta, top=blue, bottom=yellow
        #[rustfmt::skip]
        let vertices = &[
            // Front face (red) - facing +Z
            Vertex { position: [-0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0] },
            Vertex { position: [ 0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0] },
            Vertex { position: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0] },
            Vertex { position: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0] },
            
            // Back face (cyan) - facing -Z
            Vertex { position: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0] },
            Vertex { position: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0] },
            Vertex { position: [-0.5,  0.5, -0.5], color: [0.0, 1.0, 1.0] },
            Vertex { position: [ 0.5,  0.5, -0.5], color: [0.0, 1.0, 1.0] },
            
            // Left face (green) - facing -X
            Vertex { position: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0] },
            Vertex { position: [-0.5, -0.5,  0.5], color: [0.0, 1.0, 0.0] },
            Vertex { position: [-0.5,  0.5,  0.5], color: [0.0, 1.0, 0.0] },
            Vertex { position: [-0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0] },
            
            // Right face (magenta) - facing +X
            Vertex { position: [ 0.5, -0.5,  0.5], color: [1.0, 0.0, 1.0] },
            Vertex { position: [ 0.5, -0.5, -0.5], color: [1.0, 0.0, 1.0] },
            Vertex { position: [ 0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0] },
            Vertex { position: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0] },
            
            // Top face (blue) - facing +Y
            Vertex { position: [-0.5,  0.5,  0.5], color: [0.0, 0.0, 1.0] },
            Vertex { position: [ 0.5,  0.5,  0.5], color: [0.0, 0.0, 1.0] },
            Vertex { position: [ 0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0] },
            Vertex { position: [-0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0] },
            
            // Bottom face (yellow) - facing -Y
            Vertex { position: [-0.5, -0.5, -0.5], color: [1.0, 1.0, 0.0] },
            Vertex { position: [ 0.5, -0.5, -0.5], color: [1.0, 1.0, 0.0] },
            Vertex { position: [ 0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0] },
            Vertex { position: [-0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0] },
        ];

        #[rustfmt::skip]
        let indices: &[u16] = &[
            // Front
            0, 1, 2,  2, 3, 0,
            // Back
            4, 5, 6,  6, 7, 4,
            // Left
            8, 9, 10,  10, 11, 8,
            // Right
            12, 13, 14,  14, 15, 12,
            // Top
            16, 17, 18,  18, 19, 16,
            // Bottom
            20, 21, 22,  22, 23, 20,
        ];

        let vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Theta Vertex Buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Theta Index Buffer"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        GeometryPipeline {
            render_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            uniform_buffer,
            uniform_bind_group,
        }
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

        // Create depth texture
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: size.width.max(1),
                height: size.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Initialize geometry pipeline with surface format
        let geometry_pipeline = self.create_geometry_pipeline(format);

        self.surface = Some(WindowSurface {
            surface,
            surface_config,
            window,
            depth_texture,
            depth_view,
        });

        self.geometry_pipeline = Some(geometry_pipeline);
        self.initialized_for_rendering = true;

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

    fn create_view_projection_matrix(&self, elapsed_seconds: f32, eye_offset: f32, aspect_ratio: f32) -> [[f32; 4]; 4] {
        // Simple rotation in clip space
        let angle = elapsed_seconds * 0.5;
        let c = angle.cos();
        let s = angle.sin();
        let scale = 0.4;
        
        // Rotate around Y axis and scale down
        [
            [c * scale, 0.0, s * scale, 0.0],
            [0.0, scale, 0.0, 0.0],
            [-s * scale, 0.0, c * scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
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
        // If not initialized for rendering yet, return a dummy submission
        if !self.initialized_for_rendering {
            log::warn!("[render] window backend not yet attached to window, skipping frame");
            return Ok(RenderSubmission {
                frame_index: inputs.frame_index,
                vr_submission: VrFrameSubmission {
                    surfaces: vec![],
                },
                gpu_submission: None,
            });
        }

        let surface = self
            .surface
            .as_ref()
            .ok_or(RenderError::Backend("window surface not initialized"))?;

        let geometry = self
            .geometry_pipeline
            .as_ref()
            .ok_or(RenderError::Backend("geometry pipeline not initialized"))?;

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

        let size = surface.window.inner_size();
        
        // Single render pass for all viewports to avoid clearing between them
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Theta Window Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.1,
                            b: 0.12,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &surface.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Set pipeline once
            render_pass.set_pipeline(&geometry.render_pipeline);
            render_pass.set_vertex_buffer(0, geometry.vertex_buffer.slice(..));
            render_pass.set_index_buffer(geometry.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            
            // Render to each viewport with appropriate eye offset
            match self.config.stereo_mode {
                StereoMode::Mono => {
                    let aspect_ratio = size.width as f32 / size.height as f32;
                    
                    // No eye separation for mono
                    let matrix = self.create_view_projection_matrix(inputs.elapsed_seconds, 0.0, aspect_ratio);
                    let uniforms = Uniforms {
                        view_projection: matrix,
                    };
                    self.queue.write_buffer(
                        &geometry.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[uniforms]),
                    );
                    
                    render_pass.set_bind_group(0, &geometry.uniform_bind_group, &[]);
                    render_pass.set_viewport(
                        0.0,
                        0.0,
                        size.width as f32,
                        size.height as f32,
                        0.0,
                        1.0,
                    );
                    render_pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                }
                StereoMode::SideBySide => {
                    let half_width = size.width / 2;
                    let aspect_ratio = half_width as f32 / size.height as f32;
                    
                    // Left eye viewport (negative offset)
                    let left_matrix = self.create_view_projection_matrix(inputs.elapsed_seconds, -1.0, aspect_ratio);
                    let left_uniforms = Uniforms {
                        view_projection: left_matrix,
                    };
                    self.queue.write_buffer(
                        &geometry.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[left_uniforms]),
                    );
                    
                    render_pass.set_bind_group(0, &geometry.uniform_bind_group, &[]);
                    render_pass.set_viewport(
                        0.0,
                        0.0,
                        half_width as f32,
                        size.height as f32,
                        0.0,
                        1.0,
                    );
                    render_pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                    
                    // Right eye viewport (positive offset)
                    let right_matrix = self.create_view_projection_matrix(inputs.elapsed_seconds, 1.0, aspect_ratio);
                    let right_uniforms = Uniforms {
                        view_projection: right_matrix,
                    };
                    self.queue.write_buffer(
                        &geometry.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[right_uniforms]),
                    );
                    
                    render_pass.set_bind_group(0, &geometry.uniform_bind_group, &[]);
                    render_pass.set_viewport(
                        half_width as f32,
                        0.0,
                        half_width as f32,
                        size.height as f32,
                        0.0,
                        1.0,
                    );
                    render_pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                }
                StereoMode::TopBottom => {
                    let half_height = size.height / 2;
                    let aspect_ratio = size.width as f32 / half_height as f32;
                    
                    // Left eye viewport (top, negative offset)
                    let left_matrix = self.create_view_projection_matrix(inputs.elapsed_seconds, -1.0, aspect_ratio);
                    let left_uniforms = Uniforms {
                        view_projection: left_matrix,
                    };
                    self.queue.write_buffer(
                        &geometry.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[left_uniforms]),
                    );
                    
                    render_pass.set_bind_group(0, &geometry.uniform_bind_group, &[]);
                    render_pass.set_viewport(
                        0.0,
                        0.0,
                        size.width as f32,
                        half_height as f32,
                        0.0,
                        1.0,
                    );
                    render_pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                    
                    // Right eye viewport (bottom, positive offset)
                    let right_matrix = self.create_view_projection_matrix(inputs.elapsed_seconds, 1.0, aspect_ratio);
                    let right_uniforms = Uniforms {
                        view_projection: right_matrix,
                    };
                    self.queue.write_buffer(
                        &geometry.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[right_uniforms]),
                    );
                    
                    render_pass.set_bind_group(0, &geometry.uniform_bind_group, &[]);
                    render_pass.set_viewport(
                        0.0,
                        half_height as f32,
                        size.width as f32,
                        half_height as f32,
                        0.0,
                        1.0,
                    );
                    render_pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        // For window rendering, we create a dummy VR submission
        // The actual presentation happens via frame.present() above
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
