//! UI Demo — orthographic 2D button with text, toggleable to perspective 3D view.
//! Run: cargo run --example ui_demo --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use theta_engine::render::FontAtlas;
    use wgpu::util::DeviceExt;
    use winit::event::{ElementState, Event, MouseButton, StartCause, WindowEvent};
    use winit::event_loop::EventLoop;
    use winit::keyboard::{KeyCode, PhysicalKey};

    // ── Math ──────────────────────────────────────────────────────────
    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0]-b[0], a[1]-b[1], a[2]-b[2]] }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]] }
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0]*b[0]+a[1]*b[1]+a[2]*b[2] }
    fn normalize(v: [f32; 3]) -> [f32; 3] { let l=dot(v,v).sqrt(); if l<1e-8{[0.0,0.0,-1.0]}else{[v[0]/l,v[1]/l,v[2]/l]} }
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
    fn orthographic(size:f32,aspect:f32,near:f32,far:f32)->[[f32;4];4]{
        let r=size*0.5;let t=r/aspect;let nf=1.0/(near-far);
        [[1.0/r,0.0,0.0,0.0],[0.0,1.0/t,0.0,0.0],[0.0,0.0,2.0*nf,-(far+near)*nf],[0.0,0.0,0.0,1.0]]
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

    fn build_vp(cam:&Camera,aspect:f32,ortho:bool)->[[f32;4];4]{
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
        if ortho{
            mul_mat4(orthographic(2.0,aspect,-10.0,10.0),view)
        }else{
            mul_mat4(perspective(std::f32::consts::FRAC_PI_4,aspect,0.1,100.0),view)
        }
    }

    fn vp_to_clip(vp:&[[f32;4];4],pos:[f32;3])->[f32;4]{
        [ vp[0][0]*pos[0]+vp[0][1]*pos[1]+vp[0][2]*pos[2]+vp[0][3],
          vp[1][0]*pos[0]+vp[1][1]*pos[1]+vp[1][2]*pos[2]+vp[1][3],
          vp[2][0]*pos[0]+vp[2][1]*pos[1]+vp[2][2]*pos[2]+vp[2][3],
          vp[3][0]*pos[0]+vp[3][1]*pos[1]+vp[3][2]*pos[2]+vp[3][3] ]
    }

    // ── Vertex type ────────────────────────────────────────────────────
    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct V2d{position:[f32;3],color:[f32;4]}
    impl V2d{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<V2d>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[
                    wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x4},
                ]}
        }
    }

    // ── Init wgpu ──────────────────────────────────────────────────────
    env_logger::init();
    let event_loop=EventLoop::new()?;
    let window=Arc::new(winit::window::WindowBuilder::new()
        .with_title("Theta Engine – UI Demo")
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

    // ── Shader & pipeline ──────────────────────────────────────────────
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("2D"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms { vp: mat4x4<f32> }
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
struct VertexInput { @location(0) position: vec3<f32>, @location(1) color: vec4<f32> }
struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) color: vec4<f32> }
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.vp * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { return in.color; }
"#.into()),
    });

    let atlas = FontAtlas::generate(10, 16);
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

    // ── Text shader & pipeline ─────────────────────────────────────────
    let tex_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Tex"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms { vp: mat4x4<f32> }
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var t_tex: texture_2d<f32>;
@group(1) @binding(1) var s_tex: sampler;
struct VertexInput { @location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32> }
struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32> }
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.vp * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var tex_color = textureSample(t_tex, s_tex, in.uv);
    return vec4<f32>(in.color.rgb * tex_color.rgb, in.color.a * tex_color.a);
}
"#.into()),
    });

    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct TV2d{position:[f32;3],uv:[f32;2],color:[f32;4]}
    impl TV2d{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<TV2d>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[
                    wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x2},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;5]>() as wgpu::BufferAddress,shader_location:2,format:wgpu::VertexFormat::Float32x4},
                ]}
        }
    }

    let tex_bgl=wgpu::BindGroupLayoutDescriptor{label:Some("TBGL"),entries:&[
        wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
        wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
    ]};
    // We'll create the pipeline per-frame since bind groups are per-frame

    // ── App state ──────────────────────────────────────────────────────
    let mut camera=Camera{pos:[0.0,0.0,0.0],yaw:-std::f32::consts::FRAC_PI_2,pitch:0.0};
    let mut keys=std::collections::HashSet::<KeyCode>::new();
    let mut is_ortho=true;
    let mut o_was_pressed=false;
    let btn_x=0.0f32; let btn_y=0.0f32; let btn_w=0.5f32; let btn_h=0.1f32;
    let mut btn_hover=false; let mut btn_pressed=false; let mut click_count=0u32;
    let mut mouse_pos=[640.0f32,360.0]; let mut last_frame=std::time::Instant::now();

    println!("╔══════════════════════════════════════════════════╗");
    println!("║          Theta Engine – UI Demo                  ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  Click the button!  O = toggle ortho/perspective ║");
    println!("║  WASD/QE = move camera (perspective mode)        ║");
    println!("╚══════════════════════════════════════════════════╝");

    // ── Event loop ─────────────────────────────────────────────────────
    event_loop.run(move|event,el|{
        match event{
            Event::NewEvents(StartCause::Init)=>{},
            Event::WindowEvent{event,..}=>match event{
                WindowEvent::CloseRequested=>el.exit(),
                WindowEvent::Resized(s)=>{
                    if s.width>0&&s.height>0{surf_cfg.width=s.width;surf_cfg.height=s.height;surface.configure(&device,&surf_cfg);}
                }
                WindowEvent::KeyboardInput{event:ke,..}=>{
                    let PhysicalKey::Code(key)=ke.physical_key else{return};
                    if key==KeyCode::KeyO{
                        let pressed=ke.state==ElementState::Pressed;
                        if pressed&&!o_was_pressed{
                            is_ortho=!is_ortho;
                            if is_ortho{ camera.pos=[0.0,0.0,0.0]; camera.yaw=-std::f32::consts::FRAC_PI_2; camera.pitch=0.0; }
                            println!("DEBUG: is_ortho={}",is_ortho);
                        }
                        o_was_pressed=pressed;
                    } else { match ke.state{ElementState::Pressed=>{keys.insert(key);}ElementState::Released=>{keys.remove(&key);}} }
                }
                WindowEvent::CursorMoved{position,..}=>{ let s=window.scale_factor() as f32; mouse_pos=[position.x as f32/s,position.y as f32/s]; }
                WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left,..}=>{ if btn_hover{ btn_pressed=true; click_count+=1; println!("Button clicked! count={}",click_count); } }
                WindowEvent::MouseInput{state:ElementState::Released,button:MouseButton::Left,..}=>{ btn_pressed=false; }
                _=>{}
            }
            Event::AboutToWait=>{
                // WASD movement in perspective mode
                if !is_ortho{
                    let now=std::time::Instant::now(); let dt=now.duration_since(last_frame).as_secs_f32(); last_frame=now;
                    let v=3.0f32*dt; let fwd=camera.forward(); let right=camera.right();
                    if keys.contains(&KeyCode::KeyW){camera.pos[0]+=fwd[0]*v;camera.pos[1]+=fwd[1]*v;camera.pos[2]+=fwd[2]*v;}
                    if keys.contains(&KeyCode::KeyS){camera.pos[0]-=fwd[0]*v;camera.pos[1]-=fwd[1]*v;camera.pos[2]-=fwd[2]*v;}
                    if keys.contains(&KeyCode::KeyD){camera.pos[0]-=right[0]*v;camera.pos[1]-=right[1]*v;camera.pos[2]-=right[2]*v;}
                    if keys.contains(&KeyCode::KeyA){camera.pos[0]+=right[0]*v;camera.pos[1]+=right[1]*v;camera.pos[2]+=right[2]*v;}
                    if keys.contains(&KeyCode::KeyQ){camera.pos[1]-=v;}
                    if keys.contains(&KeyCode::KeyE){camera.pos[1]+=v;}
                }

                let phys_w=surf_cfg.width as f32; let phys_h=surf_cfg.height as f32;
                let scale=window.scale_factor() as f32;
                let log_w=phys_w/scale; let log_h=phys_h/scale;
                let vp=build_vp(&camera,phys_w/phys_h,is_ortho);
                let vp_t=transpose(vp);

                // Hit test: project button corners to screen, do 2D AABB test
                let bz=-5.0f32;
                let corners=[[btn_x-btn_w,btn_y-btn_h,bz],[btn_x+btn_w,btn_y-btn_h,bz],[btn_x+btn_w,btn_y+btn_h,bz],[btn_x-btn_w,btn_y+btn_h,bz]];
                let mut sc_min=[1e20f32,1e20f32]; let mut sc_max=[-1e20f32,-1e20f32];
                for c in &corners{ let clip=vp_to_clip(&vp_t,*c); let ndc=[clip[0]/clip[3],clip[1]/clip[3]]; let sx=(ndc[0]+1.0)*0.5*log_w; let sy=(1.0-ndc[1])*0.5*log_h; sc_min[0]=sc_min[0].min(sx); sc_min[1]=sc_min[1].min(sy); sc_max[0]=sc_max[0].max(sx); sc_max[1]=sc_max[1].max(sy); }
                btn_hover=mouse_pos[0]>=sc_min[0]&&mouse_pos[0]<=sc_max[0]&&mouse_pos[1]>=sc_min[1]&&mouse_pos[1]<=sc_max[1];
                // Uniform buffer
                let ub_buf=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("UB"),contents:bytemuck::cast_slice(&vp_t),usage:wgpu::BufferUsages::UNIFORM});
                let bgl=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("BGL"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
                let bg=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("BG"),layout:&bgl,entries:&[wgpu::BindGroupEntry{binding:0,resource:ub_buf.as_entire_binding()}]});
                let pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("PLL"),bind_group_layouts:&[&bgl],push_constant_ranges:&[]});
                let pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                    label:Some("Rect"),layout:Some(&pll),
                    vertex:wgpu::VertexState{module:&shader,entry_point:"vs_main",buffers:&[V2d::desc()]},
                    fragment:Some(wgpu::FragmentState{module:&shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
                    primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,..Default::default()},
                    depth_stencil:None, multisample:wgpu::MultisampleState::default(),multiview:None,
                });

                // Button rect
                let bc=if btn_pressed{[0.1f32,0.8,0.1,1.0]}else if btn_hover{[0.2,1.0,0.2,1.0]}else{[0.15,0.7,0.15,1.0]};
                // hover state already computed above
                let bz=-5.0f32;
                let verts:Vec<V2d>=vec![
                    V2d{position:[btn_x-btn_w,btn_y-btn_h,bz],color:bc},
                    V2d{position:[btn_x+btn_w,btn_y-btn_h,bz],color:bc},
                    V2d{position:[btn_x+btn_w,btn_y+btn_h,bz],color:bc},
                    V2d{position:[btn_x-btn_w,btn_y+btn_h,bz],color:bc},
                ];
                let idx:&[u16]=&[0,1,2,0,2,3];
                let vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("VB"),contents:bytemuck::cast_slice(&verts),usage:wgpu::BufferUsages::VERTEX});
                let ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("IB"),contents:bytemuck::cast_slice(idx),usage:wgpu::BufferUsages::INDEX});

                // Text characters using font atlas
                let label="Click Me!";
                let tc=[1.0f32,1.0,1.0,1.0];
                let cw=0.06f32; let ch=0.1f32; let gap=0.008f32;
                let tw=label.len() as f32*(cw+gap)-gap; let tx0=btn_x-tw*0.5;
                let inv_tw=1.0/atlas.width as f32; let inv_th=1.0/atlas.height as f32;
                let mut cverts:Vec<TV2d>=Vec::new(); let mut cidx:Vec<u16>=Vec::new();
                for(i,ch_byte) in label.bytes().enumerate(){
                    let ch_byte=if ch_byte>=32&&ch_byte<127{ch_byte}else{b'?'};
                    let idx=(ch_byte-32)as u32; let col=idx%atlas.cols; let row=idx/atlas.cols;
                    let ax0=col*atlas.cell_width; let ay0=row*atlas.cell_height;
                    let sx=atlas.cell_width/5; let sy=atlas.cell_height/7;
                    for gy in 0..7u32{for gx in 0..5u32{
                        let ax=ax0+gx*sx+sx/2; let ay=ay0+gy*sy+sy/2;
                        let off=((ay*atlas.width+ax)*4+3)as usize;
                        if *atlas.pixels.get(off).unwrap_or(&0)<=128{continue;}
                        let wx0=tx0+i as f32*(cw+gap)+gx as f32*(cw/5.0);
                        let wx1=wx0+cw/5.0; let wy1=btn_y+ch*0.5-gy as f32*(ch/7.0); let wy0=wy1-ch/7.0;
                        let qu0=(ax0+gx*sx)as f32*inv_tw; let qu1=(ax0+(gx+1)*sx)as f32*inv_tw;
                        let qv0=(ay0+gy*sy)as f32*inv_th; let qv1=(ay0+(gy+1)*sy)as f32*inv_th;
                        let b=cverts.len() as u16;
                        let tz=bz-0.01;
                        cverts.extend_from_slice(&[TV2d{position:[wx0,wy0,tz],uv:[qu0,qv0],color:tc},TV2d{position:[wx1,wy0,tz],uv:[qu1,qv0],color:tc},TV2d{position:[wx1,wy1,tz],uv:[qu1,qv1],color:tc},TV2d{position:[wx0,wy1,tz],uv:[qu0,qv1],color:tc}]);
                        cidx.extend_from_slice(&[b,b+1,b+2,b,b+2,b+3]);
                    }}
                }
                let cvb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("CVB"),contents:bytemuck::cast_slice(&cverts),usage:wgpu::BufferUsages::VERTEX});
                let cib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("CIB"),contents:bytemuck::cast_slice(&cidx),usage:wgpu::BufferUsages::INDEX});

                // Text pipeline (per-frame since bind group layout changes)
                let tex_bgl=device.create_bind_group_layout(&tex_bgl);
                let tex_bg=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("TBG"),layout:&tex_bgl,entries:&[wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&font_view)},wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&font_samp)}]});
                let tex_pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("TPLL"),bind_group_layouts:&[&bgl,&tex_bgl],push_constant_ranges:&[]});
                let tex_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                    label:Some("Text"),layout:Some(&tex_pll),
                    vertex:wgpu::VertexState{module:&tex_shader,entry_point:"vs_main",buffers:&[TV2d::desc()]},
                    fragment:Some(wgpu::FragmentState{module:&tex_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),
                    primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,..Default::default()},
                    depth_stencil:None, multisample:wgpu::MultisampleState::default(),multiview:None,
                });

                // Render
                let frame=match surface.get_current_texture(){Ok(f)=>f,Err(e)=>{eprintln!("get_current_texture error: {:?}",e);return;}};
                let view=frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut enc=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("Frame")});
                { let mut p=enc.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("UI"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:&view,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.04,g:0.06,b:0.09,a:1.0}),store:wgpu::StoreOp::Store}})],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None});
                    p.set_pipeline(&pipe); p.set_bind_group(0,&bg,&[]);
                    p.set_vertex_buffer(0,vb.slice(..)); p.set_index_buffer(ib.slice(..),wgpu::IndexFormat::Uint16); p.draw_indexed(0..6,0,0..1);
                    p.set_pipeline(&tex_pipe); p.set_bind_group(0,&bg,&[]); p.set_bind_group(1,&tex_bg,&[]);
                    p.set_vertex_buffer(0,cvb.slice(..)); p.set_index_buffer(cib.slice(..),wgpu::IndexFormat::Uint16); p.draw_indexed(0..cidx.len() as u32,0,0..1);
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
