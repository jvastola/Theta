//! Fly-camera example with 3D text label above the cube.
//! Run: cargo run --example fly_camera --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use theta_engine::render::FontAtlas;
    use wgpu::util::DeviceExt;
    use winit::event::{DeviceEvent, ElementState, Event, MouseButton, StartCause, WindowEvent};
    use winit::event_loop::EventLoop;
    use winit::keyboard::{KeyCode, PhysicalKey};

    // ── Math ──────────────────────────────────────────────────────────
    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0]-b[0], a[1]-b[1], a[2]-b[2]] }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]] }
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0]*b[0]+a[1]*b[1]+a[2]*b[2] }
    fn normalize(v: [f32; 3]) -> [f32; 3] { let l=dot(v,v).sqrt(); [v[0]/l,v[1]/l,v[2]/l] }
    fn mul_mat4(a: [[f32;4];4], b: [[f32;4];4]) -> [[f32;4];4] {
        let mut o=[[0.0f32;4];4];
        for i in 0..4{for j in 0..4{for k in 0..4{o[i][j]+=a[i][k]*b[k][j];}}}
        o
    }
    fn transpose(m: [[f32;4];4]) -> [[f32;4];4] {
        let mut o=[[0.0f32;4];4];
        for i in 0..4{for j in 0..4{o[i][j]=m[j][i];}}
        o
    }
    fn perspective(fov_y:f32,aspect:f32,near:f32,far:f32)->[[f32;4];4]{
        let f=1.0/(fov_y*0.5).tan();let nf=1.0/(near-far);
        [[f/aspect,0.0,0.0,0.0],[0.0,f,0.0,0.0],[0.0,0.0,(far+near)*nf,2.0*far*near*nf],[0.0,0.0,-1.0,0.0]]
    }

    struct Camera{pos:[f32;3],yaw:f32,pitch:f32}
    impl Camera{
        fn forward(&self)->[f32;3]{
            let(sy,cy)=(self.yaw.sin(),self.yaw.cos());
            let(sp,cp)=(self.pitch.sin(),self.pitch.cos());
            [cy*cp,sp,sy*cp]
        }
        fn right(&self)->[f32;3]{let(sy,cy)=(self.yaw.sin(),self.yaw.cos());[sy,0.0,-cy]}
    }

    fn build_vp(cam:&Camera,aspect:f32)->[[f32;4];4]{
        let fwd=cam.forward();
        let target=[cam.pos[0]+fwd[0],cam.pos[1]+fwd[1],cam.pos[2]+fwd[2]];
        let f_dir=normalize(sub(target,cam.pos));
        let s=normalize(cross(f_dir,[0.0,1.0,0.0]));
        let u=cross(s,f_dir);
        let view=[
            [s[0],s[1],s[2],-dot(s,cam.pos)],
            [u[0],u[1],u[2],-dot(u,cam.pos)],
            [-f_dir[0],-f_dir[1],-f_dir[2],dot(f_dir,cam.pos)],
            [0.0,0.0,0.0,1.0],
        ];
        mul_mat4(perspective(std::f32::consts::FRAC_PI_4,aspect,0.1,100.0),view)
    }

    // ── Init wgpu ──────────────────────────────────────────────────────
    env_logger::init();
    let event_loop=EventLoop::new()?;
    let window=Arc::new(winit::window::WindowBuilder::new()
        .with_title("Theta Engine – Fly Camera")
        .with_inner_size(winit::dpi::LogicalSize::new(1280,720))
        .build(&event_loop)?);

    let instance=wgpu::Instance::new(wgpu::InstanceDescriptor{backends:wgpu::Backends::PRIMARY,..Default::default()});
    let surface=instance.create_surface(Arc::clone(&window))?;
    let adapter=pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions{
        power_preference:wgpu::PowerPreference::HighPerformance,compatible_surface:Some(&surface),force_fallback_adapter:false,
    })).ok_or("no adapter")?;
    let (device,queue)=pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor{label:Some("Device"),required_features:wgpu::Features::empty(),required_limits:wgpu::Limits::default()},None,
    )).map_err(|_|"device error")?;

    let caps=surface.get_capabilities(&adapter);
    let format=caps.formats.iter().copied().find(|f|f.is_srgb()).unwrap_or(caps.formats[0]);
    let sc=window.inner_size();
    let mut surf_cfg=wgpu::SurfaceConfiguration{
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT,format,width:sc.width.max(1),height:sc.height.max(1),
        present_mode:wgpu::PresentMode::AutoVsync,alpha_mode:wgpu::CompositeAlphaMode::Opaque,view_formats:vec![],desired_maximum_frame_latency:2,
    };
    surface.configure(&device,&surf_cfg);

    let mut depth_tex={
        let t=device.create_texture(&wgpu::TextureDescriptor{
            label:Some("Depth"),size:wgpu::Extent3d{width:surf_cfg.width,height:surf_cfg.height,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,
            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
        });
        t.create_view(&wgpu::TextureViewDescriptor::default())
    };

    // ── 3D shader & pipeline ───────────────────────────────────────────
    let shader3d=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Geometry"),source:wgpu::ShaderSource::Wgsl(include_str!("../src/render/shaders/geometry.wgsl").into()),
    });
    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct V3d{position:[f32;3],color:[f32;3]}
    impl V3d{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<V3d>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x3}]}
        }
    }
    let ub3d=device.create_buffer(&wgpu::BufferDescriptor{label:Some("UB3d"),size:64,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    let bgl3d=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("BGL3d"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
    let bg3d=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("BG3d"),layout:&bgl3d,entries:&[wgpu::BindGroupEntry{binding:0,resource:ub3d.as_entire_binding()}]});
    let pll3d=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("PLL3d"),bind_group_layouts:&[&bgl3d],push_constant_ranges:&[]});

    let tri_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
        label:Some("Tri"),layout:Some(&pll3d),
        vertex:wgpu::VertexState{module:&shader3d,entry_point:"vs_main",buffers:&[V3d::desc()]},
        fragment:Some(wgpu::FragmentState{module:&shader3d,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
        primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,front_face:wgpu::FrontFace::Ccw,cull_mode:Some(wgpu::Face::Back),..Default::default()},
        depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
        multisample:wgpu::MultisampleState::default(),multiview:None,
    });
    let line_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
        label:Some("Line"),layout:Some(&pll3d),
        vertex:wgpu::VertexState{module:&shader3d,entry_point:"vs_main",buffers:&[V3d::desc()]},
        fragment:Some(wgpu::FragmentState{module:&shader3d,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
        primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::LineList,cull_mode:None,..Default::default()},
        depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
        multisample:wgpu::MultisampleState::default(),multiview:None,
    });

    // ── Cube ───────────────────────────────────────────────────────────
    #[rustfmt::skip]
    let cube_v:&[V3d]=&[
        V3d{position:[-0.5,-0.5,0.5],color:[1.0,0.2,0.2]},V3d{position:[0.5,-0.5,0.5],color:[1.0,0.2,0.2]},
        V3d{position:[0.5,0.5,0.5],color:[1.0,0.2,0.2]},V3d{position:[-0.5,0.5,0.5],color:[1.0,0.2,0.2]},
        V3d{position:[0.5,-0.5,-0.5],color:[0.2,1.0,1.0]},V3d{position:[-0.5,-0.5,-0.5],color:[0.2,1.0,1.0]},
        V3d{position:[-0.5,0.5,-0.5],color:[0.2,1.0,1.0]},V3d{position:[0.5,0.5,-0.5],color:[0.2,1.0,1.0]},
        V3d{position:[-0.5,-0.5,-0.5],color:[0.2,1.0,0.2]},V3d{position:[-0.5,-0.5,0.5],color:[0.2,1.0,0.2]},
        V3d{position:[-0.5,0.5,0.5],color:[0.2,1.0,0.2]},V3d{position:[-0.5,0.5,-0.5],color:[0.2,1.0,0.2]},
        V3d{position:[0.5,-0.5,0.5],color:[1.0,0.2,1.0]},V3d{position:[0.5,-0.5,-0.5],color:[1.0,0.2,1.0]},
        V3d{position:[0.5,0.5,-0.5],color:[1.0,0.2,1.0]},V3d{position:[0.5,0.5,0.5],color:[1.0,0.2,1.0]},
        V3d{position:[-0.5,0.5,0.5],color:[0.3,0.3,1.0]},V3d{position:[0.5,0.5,0.5],color:[0.3,0.3,1.0]},
        V3d{position:[0.5,0.5,-0.5],color:[0.3,0.3,1.0]},V3d{position:[-0.5,0.5,-0.5],color:[0.3,0.3,1.0]},
        V3d{position:[-0.5,-0.5,-0.5],color:[1.0,1.0,0.2]},V3d{position:[0.5,-0.5,-0.5],color:[1.0,1.0,0.2]},
        V3d{position:[0.5,-0.5,0.5],color:[1.0,1.0,0.2]},V3d{position:[-0.5,-0.5,0.5],color:[1.0,1.0,0.2]},
    ];
    #[rustfmt::skip]
    let cube_i:&[u16]=&[0,1,2,2,3,0,4,5,6,6,7,4,8,9,10,10,11,8,12,13,14,14,15,12,16,17,18,18,19,16,20,21,22,22,23,20];
    let _cube_vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("CVB"),contents:bytemuck::cast_slice(cube_v),usage:wgpu::BufferUsages::VERTEX});
    let cube_ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("CIB"),contents:bytemuck::cast_slice(cube_i),usage:wgpu::BufferUsages::INDEX});

    // ── Grid ───────────────────────────────────────────────────────────
    let mut gv=Vec::new();let mut gi=Vec::new();let gc=[0.25f32,0.28,0.32];let ge=20i32;let mut gii:u16=0;
    for i in -ge..=ge{
        let x=i as f32;
        gv.push(V3d{position:[x,0.0,-ge as f32],color:gc});gv.push(V3d{position:[x,0.0,ge as f32],color:gc});
        gi.push(gii);gi.push(gii+1);gii+=2;
        gv.push(V3d{position:[-ge as f32,0.0,x],color:gc});gv.push(V3d{position:[ge as f32,0.0,x],color:gc});
        gi.push(gii);gi.push(gii+1);gii+=2;
    }
    let grid_vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("GVB"),contents:bytemuck::cast_slice(&gv),usage:wgpu::BufferUsages::VERTEX});
    let grid_ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("GIB"),contents:bytemuck::cast_slice(&gi),usage:wgpu::BufferUsages::INDEX});
    let grid_ic=gi.len() as u32;

    // ══════════════════════════════════════════════════════════════════════
    // Text rendering — 3D billboard approach
    // Instead of projecting to screen space, we render text as a 3D quad
    // in world space that always faces the camera (billboard).
    // This is simpler and more reliable than screen-space text.
    // ══════════════════════════════════════════════════════════════════════

    // Generate font atlas
    let atlas = FontAtlas::generate(10, 16);

    // Create font texture
    let font_tex = device.create_texture(&wgpu::TextureDescriptor{
        label:Some("Font"),size:wgpu::Extent3d{width:atlas.width,height:atlas.height,depth_or_array_layers:1},
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,
        format:wgpu::TextureFormat::Rgba8Unorm,
        usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture{texture:&font_tex,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
        &atlas.pixels,wgpu::ImageDataLayout{offset:0,bytes_per_row:Some(atlas.width*4),rows_per_image:Some(atlas.height)},
        wgpu::Extent3d{width:atlas.width,height:atlas.height,depth_or_array_layers:1},
    );
    let font_view=font_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let font_samp=device.create_sampler(&wgpu::SamplerDescriptor{
        label:Some("FS"),mag_filter:wgpu::FilterMode::Nearest,min_filter:wgpu::FilterMode::Nearest,..Default::default()
    });

    // Text shader — same geometry shader but with texture sampling
    let text_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Text Shader"),source:wgpu::ShaderSource::Wgsl(include_str!("../src/render/shaders/text.wgsl").into()),
    });

    // Text pipeline: uses 2D vertex with UV, but we'll transform in the shader
    // Actually, let's use a simpler approach: textured 3D quads with identity VP
    // We'll compute the billboard transform on the CPU and pass it as the VP

    // For the text pipeline, we use a simple 2D vertex with a combined VP that
    // transforms from pixel space to clip space. The CPU computes the quad
    // position in world space, then we project it.

    // Simpler approach: just use the 3D pipeline with a texture.
    // We'll create a textured quad in world space that faces the camera.

    // Text vertex: position (3D) + uv (2D) + color (4D)
    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct TextVert{position:[f32;3],uv:[f32;2],color:[f32;4]}
    impl TextVert{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<TextVert>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[
                    wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x2},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;5]>() as wgpu::BufferAddress,shader_location:2,format:wgpu::VertexFormat::Float32x4},
                ]}
        }
    }

    // Textured 3D shader
    let tex3d_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Tex3d"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms { vp: mat4x4<f32> }
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var t_tex: texture_2d<f32>;
@group(1) @binding(1) var s_tex: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.vp * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var tex_color = textureSample(t_tex, s_tex, in.uv);
    return vec4<f32>(in.color.rgb * tex_color.rgb, in.color.a * tex_color.a);
}
"#.into()),
    });

    let tex_ub=device.create_buffer(&wgpu::BufferDescriptor{label:Some("TexUB"),size:64,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    let tex_bgl0=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("TexBGL0"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
    let tex_bgl1=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("TexBGL1"),entries:&[
        wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
        wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
    ]});
    let tex_bg0=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("TexBG0"),layout:&tex_bgl0,entries:&[wgpu::BindGroupEntry{binding:0,resource:tex_ub.as_entire_binding()}]});
    let tex_bg1=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("TexBG1"),layout:&tex_bgl1,entries:&[
        wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&font_view)},
        wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&font_samp)},
    ]});
    let tex_pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("TexPLL"),bind_group_layouts:&[&tex_bgl0,&tex_bgl1],push_constant_ranges:&[]});
    let tex_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
        label:Some("TexPipe"),layout:Some(&tex_pll),
        vertex:wgpu::VertexState{module:&tex3d_shader,entry_point:"vs_main",buffers:&[TextVert::desc()]},
        fragment:Some(wgpu::FragmentState{module:&tex3d_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),
        primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,cull_mode:None,..Default::default()},
        depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
        multisample:wgpu::MultisampleState::default(),multiview:None,
    });

    // ── App state ──────────────────────────────────────────────────────
    let mut camera=Camera{pos:[0.0,2.0,-5.0],yaw:std::f32::consts::FRAC_PI_2,pitch:-0.15};
    let mut keys=std::collections::HashSet::<KeyCode>::new();
    let mut mouse_cap=true;
    let mut last_frame=std::time::Instant::now();
    let mut cube_rot_y:f32=0.0;
    let mut hit_point:Option<[f32;3]>=None;
    let mut mouse_pos:[f32;2]=[640.0,360.0]; // track mouse position in screen pixels
    let _=window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
    window.set_cursor_visible(false);

    println!("╔══════════════════════════════════════════════════╗");
    println!("║        Theta Engine – Fly Camera + 3D Text        ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  W/S      Move forward / backward                ║");
    println!("║  A/D      Strafe left / right                    ║");
    println!("║  Space    Move up    LCtrl  Move down            ║");
    println!("║  Shift    Faster     ESC    Release mouse        ║");
    println!("╚══════════════════════════════════════════════════╝");

    // Track ESC key state to avoid repeat toggling
    let mut esc_was_pressed=false;

    // ── Event loop ─────────────────────────────────────────────────────
    event_loop.run(move|event,el|{
        match event{
            Event::NewEvents(StartCause::Init)=>{},
            Event::WindowEvent{event,..}=>match event{
                WindowEvent::CloseRequested=>el.exit(),
                WindowEvent::Resized(s)=>{
                    if s.width>0&&s.height>0{
                        surf_cfg.width=s.width;surf_cfg.height=s.height;
                        surface.configure(&device,&surf_cfg);
                        let t=device.create_texture(&wgpu::TextureDescriptor{
                            label:Some("Depth"),size:wgpu::Extent3d{width:s.width,height:s.height,depth_or_array_layers:1},
                            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,
                            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
                        });
                        depth_tex=t.create_view(&wgpu::TextureViewDescriptor::default());
                    }
                }
                WindowEvent::KeyboardInput{event:ke,..}=>{
                    let PhysicalKey::Code(key)=ke.physical_key else{return};
                    match key{
                        KeyCode::Escape=>{
                            // Only toggle on the initial press, not key repeat
                            let pressed=ke.state==ElementState::Pressed;
                            if pressed && !esc_was_pressed{
                                mouse_cap=!mouse_cap;
                                if mouse_cap{
                                    let _=window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                                    window.set_cursor_visible(false);
                                }else{
                                    let _=window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                }
                                eprintln!("DEBUG: ESC toggled, mouse_cap={}", mouse_cap);
                            }
                            esc_was_pressed=pressed;
                        }
                        _=>match ke.state{ElementState::Pressed=>{keys.insert(key);}ElementState::Released=>{keys.remove(&key);}},
                    }
                }
                WindowEvent::Focused(f)=>{
                    eprintln!("DEBUG: Focused event, focused={}", f);
                    // Only auto-recapture if we were previously in capture mode
                    // and the window regains focus (e.g. after clicking X button)
                    if f && mouse_cap{
                        let _=window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        window.set_cursor_visible(false);
                    }
                }
                WindowEvent::CursorMoved{position,..}=>{
                    mouse_pos=[position.x as f32,position.y as f32];
                }
                WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left,..}=>{
                    let ray_origin=camera.pos;
                    let ray_dir=if mouse_cap{
                        // Mouse locked: ray from center
                        camera.forward()
                    }else{
                        // Mouse free: ray from cursor position
                        let phys_w=window.inner_size().width as f32;
                        let phys_h=window.inner_size().height as f32;
                        // Convert mouse pixel to normalized device coordinates [-1,1]
                        let ndc_x=1.0-(mouse_pos[0]/phys_w)*2.0; // flip X so left=left
                        let ndc_y=1.0-(mouse_pos[1]/phys_h)*2.0;
                        // Build ray direction from camera basis + NDC offset
                        let fwd=camera.forward();
                        let right=camera.right();
                        let up=[0.0f32,1.0,0.0];
                        let tan_half_fov=(std::f32::consts::FRAC_PI_8).tan();
                        let aspect=phys_w/phys_h;
                        normalize([
                            fwd[0]+right[0]*ndc_x*aspect*tan_half_fov+up[0]*ndc_y*tan_half_fov,
                            fwd[1]+right[1]*ndc_x*aspect*tan_half_fov+up[1]*ndc_y*tan_half_fov,
                            fwd[2]+right[2]*ndc_x*aspect*tan_half_fov+up[2]*ndc_y*tan_half_fov,
                        ])
                    };
                    let cube_center=[0.0f32,0.5,0.0];
                    let cube_radius=0.9f32;
                    let oc=sub(ray_origin,cube_center);
                    let a=dot(ray_dir,ray_dir);
                    let b=2.0*dot(oc,ray_dir);
                    let c=dot(oc,oc)-cube_radius*cube_radius;
                    let disc=b*b-4.0*a*c;
                    if disc>=0.0{
                        let t=(-b-disc.sqrt())/(2.0*a);
                        if t>0.0{
                            hit_point=Some([ray_origin[0]+ray_dir[0]*t,ray_origin[1]+ray_dir[1]*t,ray_origin[2]+ray_dir[2]*t]);
                            cube_rot_y+=std::f32::consts::FRAC_PI_2;
                        }
                    }
                }
                _=>{}
            }
            Event::DeviceEvent{event:DeviceEvent::MouseMotion{delta:(dx,dy)},..}=>{
                if mouse_cap{camera.yaw+=dx as f32*0.002;camera.pitch-=dy as f32*0.002;camera.pitch=camera.pitch.clamp(-1.55,1.55);}
            }
            Event::AboutToWait=>{
                // Update camera
                let now=std::time::Instant::now();let dt=now.duration_since(last_frame).as_secs_f32();last_frame=now;
                let speed=if keys.contains(&KeyCode::ShiftLeft)||keys.contains(&KeyCode::ShiftRight){12.0}else{4.0};
                let v=speed*dt;let fwd=camera.forward();let right=camera.right();
                if keys.contains(&KeyCode::KeyW){camera.pos[0]+=fwd[0]*v;camera.pos[1]+=fwd[1]*v;camera.pos[2]+=fwd[2]*v;}
                if keys.contains(&KeyCode::KeyS){camera.pos[0]-=fwd[0]*v;camera.pos[1]-=fwd[1]*v;camera.pos[2]-=fwd[2]*v;}
                if keys.contains(&KeyCode::KeyD){camera.pos[0]-=right[0]*v;camera.pos[1]-=right[1]*v;camera.pos[2]-=right[2]*v;}
                if keys.contains(&KeyCode::KeyA){camera.pos[0]+=right[0]*v;camera.pos[1]+=right[1]*v;camera.pos[2]+=right[2]*v;}
                if keys.contains(&KeyCode::Space){camera.pos[1]+=v;}
                if keys.contains(&KeyCode::ControlLeft){camera.pos[1]-=v;}

                let w=surf_cfg.width as f32;let h=surf_cfg.height as f32;
                let vp=build_vp(&camera,w/h);
                let vp_gpu=transpose(vp);
                queue.write_buffer(&ub3d,0,bytemuck::cast_slice(&vp_gpu));

                // Build rotated cube vertices
                let cos_r=cube_rot_y.cos();
                let sin_r=cube_rot_y.sin();
                let cube_verts_rot:Vec<V3d>=cube_v.iter().map(|v|{
                    let rx=v.position[0]*cos_r+v.position[2]*sin_r;
                    let rz=-v.position[0]*sin_r+v.position[2]*cos_r;
                    V3d{position:[rx,v.position[1],rz],color:v.color}
                }).collect();
                let cube_vb_rot=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("CVR"),contents:bytemuck::cast_slice(&cube_verts_rot),usage:wgpu::BufferUsages::VERTEX});

                // Build ray line + hit point cross vertices
                let mut ray_verts:Vec<V3d>=Vec::new();
                let mut ray_idx:Vec<u16>=Vec::new();
                if let Some(hp)=hit_point{
                    ray_verts.push(V3d{position:camera.pos,color:[1.0,1.0,0.0]});
                    ray_verts.push(V3d{position:hp,color:[1.0,1.0,0.0]});
                    ray_idx.push(0);ray_idx.push(1);
                    let s=0.06f32;
                    let b=ray_verts.len() as u16;
                    ray_verts.push(V3d{position:[hp[0]-s,hp[1],hp[2]],color:[1.0,0.2,0.2]});
                    ray_verts.push(V3d{position:[hp[0]+s,hp[1],hp[2]],color:[1.0,0.2,0.2]});
                    ray_verts.push(V3d{position:[hp[0],hp[1]-s,hp[2]],color:[0.2,1.0,0.2]});
                    ray_verts.push(V3d{position:[hp[0],hp[1]+s,hp[2]],color:[0.2,1.0,0.2]});
                    ray_verts.push(V3d{position:[hp[0],hp[1],hp[2]-s],color:[0.2,0.2,1.0]});
                    ray_verts.push(V3d{position:[hp[0],hp[1],hp[2]+s],color:[0.2,0.2,1.0]});
                    ray_idx.extend_from_slice(&[b,b+1,b+2,b+3,b+4,b+5]);
                }
                let (ray_vb,ray_ib)=if !ray_verts.is_empty(){
                    (Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("RVB"),contents:bytemuck::cast_slice(&ray_verts),usage:wgpu::BufferUsages::VERTEX})),
                     Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("RIB"),contents:bytemuck::cast_slice(&ray_idx),usage:wgpu::BufferUsages::INDEX})))
                }else{(None,None)};

                // Build 3D extruded text: each character is built from per-pixel quads
                // for the glyph's opaque pixels only. Side faces trace the glyph
                // silhouette. Characters are spaced with a small gap to prevent touching.
                let text="Hello World";
                let text_world_pos=[0.0f32,1.5,0.0];
                let char_count=text.len() as f32;
                // Use square pixels to preserve the font's aspect ratio.
                // The glyph is 5×7 pixels; we size by the larger dimension so pixels are square.
                let glyph_aspect = 5.0f32 / 7.0f32; // width/height of glyph in pixels
                let char_gap = 0.03f32; // gap between characters in world units
                let total_gaps = char_gap * (char_count - 1.0).max(0.0);
                // Height drives the size; width follows from glyph aspect ratio
                let text_height = 0.25f32;
                let char_h = text_height;
                let char_w = char_h * glyph_aspect; // square pixels: 5 wide × 7 tall
                let text_width = char_w * char_count + total_gaps;
                // Depth proportional to character height
                let text_depth = text_height * 0.15f32;

                let inv_tw=1.0/atlas.width as f32;
                let inv_th=1.0/atlas.height as f32;
                let front_color=[1.0f32,0.9,0.4,1.0];
                let side_color =[0.7f32,0.6,0.2,1.0];
                let back_color =[0.4f32,0.3,0.1,1.0];

                let z_near=text_world_pos[2]-text_depth*0.5;
                let z_far =text_world_pos[2]+text_depth*0.5;
                let z_eps = 0.001f32;
                let z_front = z_near - z_eps;
                let z_back  = z_far + z_eps;

                let mut text_verts:Vec<TextVert>=Vec::new();
                let mut text_indices:Vec<u16>=Vec::new();

                for(i,ch_byte) in text.bytes().enumerate(){
                    let ch_byte=if ch_byte>=32&&ch_byte<127{ch_byte}else{b'?'};
                    let idx=(ch_byte-32)as u32;
                    let col=idx%atlas.cols;
                    let row=idx/atlas.cols;
                    // Character x position: each char gets char_w + gap, centered
                    let x0=text_world_pos[0]+i as f32*(char_w+char_gap) - text_width*0.5 - total_gaps*0.5;
                    let y1=text_world_pos[1]+text_height*0.5;

                    // Front & back faces — per-pixel quads for opaque pixels only.
                    // This is critical: transparent pixels must NOT write to the depth
                    // buffer, otherwise they occlude the side faces of inner edges
                    // (e.g. the inside of a 'W'). We use the same pixel-sampling
                    // approach as the sides.
                    //
                    // Side faces — for every ON pixel that has an exposed edge
                    // (neighbor is OFF or out of bounds), emit a quad extruded from
                    // front to back. This traces the full glyph silhouette including
                    // inner edges of shapes like 'W', 'A', 'B' — matching how Godot
                    // TextMesh and Unity TextMeshPro handle 3D text sides.
                    //
                    // The glyph is 5×7 pixels scaled to fill the cell. We iterate over
                    // the 5×7 glyph pixels (not the full cell) and compute world-space
                    // size from the glyph dimensions so the geometry tightly fits.

                    let ax0 = col * atlas.cell_width; // atlas pixel origin of this cell
                    let ay0 = row * atlas.cell_height;
                    let glyph_w = 5u32; // glyph is 5 pixels wide
                    let glyph_h = 7u32; // glyph is 7 pixels high
                    let glyph_px_w = char_w / glyph_w as f32; // world units per glyph pixel (x)
                    let glyph_px_h = text_height / glyph_h as f32; // world units per glyph pixel (y)

                    // Sample the atlas at glyph pixel (gx, gy). The glyph pixel maps
                    // to atlas pixels [gx*sx .. (gx+1)*sx) × [gy*sy .. (gy+1)*sy).
                    let sx = atlas.cell_width / glyph_w;
                    let sy = atlas.cell_height / glyph_h;
                    let glyph_pixel_on = |gx: i32, gy: i32| -> bool {
                        if gx < 0 || gy < 0 || gx >= glyph_w as i32 || gy >= glyph_h as i32 {
                            return false;
                        }
                        // Sample the center of the glyph pixel region in the atlas
                        let ax = ax0 + (gx as u32) * sx + sx / 2;
                        let ay = ay0 + (gy as u32) * sy + sy / 2;
                        let off = ((ay * atlas.width + ax) * 4 + 3) as usize;
                        *atlas.pixels.get(off).unwrap_or(&0) > 128
                    };

                    for gy in 0..glyph_h {
                        for gx in 0..glyph_w {
                            if !glyph_pixel_on(gx as i32, gy as i32) { continue; }

                            let wx0 = x0 + gx as f32 * glyph_px_w;
                            let wx1 = wx0 + glyph_px_w;
                            let wy1 = y1 - gy as f32 * glyph_px_h;
                            let wy0 = wy1 - glyph_px_h;
                            let qu0 = (ax0 + gx as u32 * sx) as f32 * inv_tw;
                            let qu1 = (ax0 + (gx + 1) as u32 * sx) as f32 * inv_tw;
                            let qv0 = (ay0 + gy as u32 * sy) as f32 * inv_th;
                            let qv1 = (ay0 + (gy + 1) as u32 * sy) as f32 * inv_th;

                            // Front face quad for this opaque pixel
                            {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx0, wy0, z_front], uv:[qu0, qv1], color:front_color},
                                    TextVert{position:[wx1, wy0, z_front], uv:[qu1, qv1], color:front_color},
                                    TextVert{position:[wx1, wy1, z_front], uv:[qu1, qv0], color:front_color},
                                    TextVert{position:[wx0, wy1, z_front], uv:[qu0, qv0], color:front_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }

                            // Back face quad for this opaque pixel
                            {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx1, wy0, z_back], uv:[qu1, qv1], color:back_color},
                                    TextVert{position:[wx0, wy0, z_back], uv:[qu0, qv1], color:back_color},
                                    TextVert{position:[wx0, wy1, z_back], uv:[qu0, qv0], color:back_color},
                                    TextVert{position:[wx1, wy1, z_back], uv:[qu1, qv0], color:back_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }

                            // Top edge exposed?
                            if !glyph_pixel_on(gx as i32, gy as i32 - 1) {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx0, wy1, z_front], uv:[qu0, qv0], color:side_color},
                                    TextVert{position:[wx1, wy1, z_front], uv:[qu1, qv0], color:side_color},
                                    TextVert{position:[wx1, wy1, z_back],  uv:[qu1, qv1], color:side_color},
                                    TextVert{position:[wx0, wy1, z_back],  uv:[qu0, qv1], color:side_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }

                            // Bottom edge exposed?
                            if !glyph_pixel_on(gx as i32, gy as i32 + 1) {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx1, wy0, z_front], uv:[qu1, qv1], color:side_color},
                                    TextVert{position:[wx0, wy0, z_front], uv:[qu0, qv1], color:side_color},
                                    TextVert{position:[wx0, wy0, z_back],  uv:[qu0, qv0], color:side_color},
                                    TextVert{position:[wx1, wy0, z_back],  uv:[qu1, qv0], color:side_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }

                            // Left edge exposed?
                            if !glyph_pixel_on(gx as i32 - 1, gy as i32) {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx0, wy0, z_front], uv:[qu0, qv1], color:side_color},
                                    TextVert{position:[wx0, wy1, z_front], uv:[qu1, qv1], color:side_color},
                                    TextVert{position:[wx0, wy1, z_back],  uv:[qu1, qv0], color:side_color},
                                    TextVert{position:[wx0, wy0, z_back],  uv:[qu0, qv0], color:side_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }

                            // Right edge exposed?
                            if !glyph_pixel_on(gx as i32 + 1, gy as i32) {
                                let base = text_verts.len() as u16;
                                text_verts.extend_from_slice(&[
                                    TextVert{position:[wx1, wy1, z_front], uv:[qu1, qv0], color:side_color},
                                    TextVert{position:[wx1, wy0, z_front], uv:[qu0, qv0], color:side_color},
                                    TextVert{position:[wx1, wy0, z_back],  uv:[qu0, qv1], color:side_color},
                                    TextVert{position:[wx1, wy1, z_back],  uv:[qu1, qv1], color:side_color},
                                ]);
                                text_indices.extend_from_slice(&[base, base+1, base+2, base+2, base+3, base]);
                            }
                        }
                    }
                }

                // Write VP to both uniform buffers
                queue.write_buffer(&tex_ub,0,bytemuck::cast_slice(&vp_gpu));

                // ── Render ────────────────────────────────────────────
                let Ok(frame)=surface.get_current_texture()else{return};
                let view=frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut enc=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("Frame")});

                // Create text buffers (front+back faces, textured)
                let (tvb,tib)=if !text_verts.is_empty(){
                    let tvb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("TVB"),contents:bytemuck::cast_slice(&text_verts),usage:wgpu::BufferUsages::VERTEX});
                    let tib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("TIB"),contents:bytemuck::cast_slice(&text_indices),usage:wgpu::BufferUsages::INDEX});
                    (Some(tvb),Some(tib))
                }else{(None,None)};

                // 3D pass: scene + text
                {
                    let mut pass=enc.begin_render_pass(&wgpu::RenderPassDescriptor{
                        label:Some("3D"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{
                            view:&view,resolve_target:None,
                            ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.04,g:0.06,b:0.09,a:1.0}),store:wgpu::StoreOp::Store},
                        })],
                        depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment{
                            view:&depth_tex,depth_ops:Some(wgpu::Operations{load:wgpu::LoadOp::Clear(1.0),store:wgpu::StoreOp::Store}),stencil_ops:None,
                        }),
                        timestamp_writes:None,occlusion_query_set:None,
                    });

                    // Grid
                    pass.set_pipeline(&line_pipe);pass.set_bind_group(0,&bg3d,&[]);
                    pass.set_vertex_buffer(0,grid_vb.slice(..));pass.set_index_buffer(grid_ib.slice(..),wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..grid_ic,0,0..1);

                    // Cube (rotated)
                    pass.set_pipeline(&tri_pipe);pass.set_bind_group(0,&bg3d,&[]);
                    pass.set_vertex_buffer(0,cube_vb_rot.slice(..));pass.set_index_buffer(cube_ib.slice(..),wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..cube_i.len() as u32,0,0..1);

                    // Ray line + hit point cross
                    if let (Some(rvb),Some(rib))=(&ray_vb,&ray_ib){
                        pass.set_pipeline(&line_pipe);
                        pass.set_bind_group(0,&bg3d,&[]);
                        pass.set_vertex_buffer(0,rvb.slice(..));
                        pass.set_index_buffer(rib.slice(..),wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..ray_idx.len() as u32,0,0..1);
                    }

                    // Text front + back + side faces (all textured via tex_pipe)
                    if let (Some(tvb),Some(tib))=(&tvb,&tib){
                        pass.set_pipeline(&tex_pipe);
                        pass.set_bind_group(0,&tex_bg0,&[]);
                        pass.set_bind_group(1,&tex_bg1,&[]);
                        pass.set_vertex_buffer(0,tvb.slice(..));
                        pass.set_index_buffer(tib.slice(..),wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..text_indices.len() as u32,0,0..1);
                    }
                }

                queue.submit(std::iter::once(enc.finish()));frame.present();window.request_redraw();
            }
            _=>{}
        }
    })?;
    Ok(())
}

#[cfg(not(feature="render-wgpu"))]
fn main(){eprintln!("Requires render-wgpu feature");std::process::exit(1);}
