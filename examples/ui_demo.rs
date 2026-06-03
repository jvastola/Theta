//! UI Demo — NxN inventory grid with render-to-texture window plane.
//! The plane shows a live ortho view of the cubes rendered as a texture.
//! ESC = toggle cursor capture + fly camera. O = toggle ortho/perspective.
//! Run: cargo run --example ui_demo --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use theta_engine::render::FontAtlas;
    use wgpu::util::DeviceExt;
    use winit::event::{DeviceEvent, ElementState, Event, MouseButton, StartCause, WindowEvent};
    use winit::event_loop::EventLoop;
    use winit::keyboard::{KeyCode, PhysicalKey};

    // ── Math ──────────────────────────────────────────────────────────
    fn sub(a:[f32;3],b:[f32;3])->[f32;3]{[a[0]-b[0],a[1]-b[1],a[2]-b[2]]}
    fn cross(a:[f32;3],b:[f32;3])->[f32;3]{[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]}
    fn dot(a:[f32;3],b:[f32;3])->f32{a[0]*b[0]+a[1]*b[1]+a[2]*b[2]}
    fn normalize(v:[f32;3])->[f32;3]{let l=dot(v,v).sqrt();if l<1e-8{[0.0,0.0,-1.0]}else{[v[0]/l,v[1]/l,v[2]/l]}}
    fn mul_mat4(a:[[f32;4];4],b:[[f32;4];4])->[[f32;4];4]{let mut o=[[0.0f32;4];4];for i in 0..4{for j in 0..4{for k in 0..4{o[i][j]+=a[i][k]*b[k][j];}}}o}
    fn transpose(m:[[f32;4];4])->[[f32;4];4]{let mut o=[[0.0f32;4];4];for i in 0..4{for j in 0..4{o[i][j]=m[j][i];}}o}
    fn perspective(fov_y:f32,aspect:f32,near:f32,far:f32)->[[f32;4];4]{let f=1.0/(fov_y*0.5).tan();let nf=1.0/(near-far);[[f/aspect,0.0,0.0,0.0],[0.0,f,0.0,0.0],[0.0,0.0,(far+near)*nf,2.0*far*near*nf],[0.0,0.0,-1.0,0.0]]}
    fn orthographic(size:f32,aspect:f32,near:f32,far:f32)->[[f32;4];4]{let r=size*0.5;let t=r/aspect;let nf=1.0/(near-far);[[1.0/r,0.0,0.0,0.0],[0.0,1.0/t,0.0,0.0],[0.0,0.0,2.0*nf,-(far+near)*nf],[0.0,0.0,0.0,1.0]]}

    struct Camera{pos:[f32;3],yaw:f32,pitch:f32}
    impl Camera{
        fn forward(&self)->[f32;3]{let(sy,cy)=(self.yaw.sin(),self.yaw.cos());let(sp,cp)=(self.pitch.sin(),self.pitch.cos());[cy*cp,sp,sy*cp]}
        fn right(&self)->[f32;3]{let(sy,cy)=(self.yaw.sin(),self.yaw.cos());[sy,0.0,-cy]}
    }

    fn build_vp(cam:&Camera,aspect:f32,ortho:bool)->[[f32;4];4]{
        let fwd=cam.forward();let target=[cam.pos[0]+fwd[0],cam.pos[1]+fwd[1],cam.pos[2]+fwd[2]];
        let f_dir=normalize(sub(target,cam.pos));let s=normalize(cross(f_dir,[0.0,1.0,0.0]));let u=cross(s,f_dir);
        let view=[[s[0],s[1],s[2],-dot(s,cam.pos)],[u[0],u[1],u[2],-dot(u,cam.pos)],[-f_dir[0],-f_dir[1],-f_dir[2],dot(f_dir,cam.pos)],[0.0,0.0,0.0,1.0]];
        if ortho{mul_mat4(orthographic(2.0,aspect,-10.0,10.0),view)}else{mul_mat4(perspective(std::f32::consts::FRAC_PI_4,aspect,0.1,100.0),view)}
    }

    fn vp_to_clip(vp:&[[f32;4];4],pos:[f32;3])->[f32;4]{
        [vp[0][0]*pos[0]+vp[0][1]*pos[1]+vp[0][2]*pos[2]+vp[0][3],
         vp[1][0]*pos[0]+vp[1][1]*pos[1]+vp[1][2]*pos[2]+vp[1][3],
         vp[2][0]*pos[0]+vp[2][1]*pos[1]+vp[2][2]*pos[2]+vp[2][3],
         vp[3][0]*pos[0]+vp[3][1]*pos[1]+vp[3][2]*pos[2]+vp[3][3]]
    }

    fn mat4_mul_vec4(m:&[[f32;4];4],v:[f32;4])->[f32;4]{
        [m[0][0]*v[0]+m[0][1]*v[1]+m[0][2]*v[2]+m[0][3]*v[3],
         m[1][0]*v[0]+m[1][1]*v[1]+m[1][2]*v[2]+m[1][3]*v[3],
         m[2][0]*v[0]+m[2][1]*v[1]+m[2][2]*v[2]+m[2][3]*v[3],
         m[3][0]*v[0]+m[3][1]*v[1]+m[3][2]*v[2]+m[3][3]*v[3]]
    }

    fn inv_mat4(m:&[[f32;4];4])->[[f32;4];4]{
        let mut s=[[0.0f32;4];4];for i in 0..4{for j in 0..4{s[i][j]=m[i][j];}}
        let mut inv=[[0.0f32;4];4];for i in 0..4{inv[i][i]=1.0;}
        for col in 0..4{
            let mut pivot=col;
            for row in (col+1)..4{if s[row][col].abs()>s[pivot][col].abs(){pivot=row;}}
            if s[pivot][col].abs()<1e-10{return inv;}
            if pivot!=col{for j in 0..4{let t=s[col][j];s[col][j]=s[pivot][j];s[pivot][j]=t;let t=inv[col][j];inv[col][j]=inv[pivot][j];inv[pivot][j]=t;}}
            let sc=1.0/s[col][col];for j in 0..4{s[col][j]*=sc;inv[col][j]*=sc;}
            for row in 0..4{if row==col{continue;}let f=s[row][col];for j in 0..4{s[row][j]-=f*s[col][j];inv[row][j]-=f*inv[col][j];}}
        }
        inv
    }

    // ── Vertex types ───────────────────────────────────────────────────
    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct V3d{position:[f32;3],color:[f32;3]}
    impl V3d{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<V3d>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x3}]}
        }
    }

    #[repr(C)]#[derive(Copy,Clone,Debug,bytemuck::Pod,bytemuck::Zeroable)]
    struct TV3d{position:[f32;3],uv:[f32;2]}
    impl TV3d{
        const fn desc()->wgpu::VertexBufferLayout<'static>{
            wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<TV3d>() as wgpu::BufferAddress,step_mode:wgpu::VertexStepMode::Vertex,
                attributes:&[wgpu::VertexAttribute{offset:0,shader_location:0,format:wgpu::VertexFormat::Float32x3},
                    wgpu::VertexAttribute{offset:std::mem::size_of::<[f32;3]>() as wgpu::BufferAddress,shader_location:1,format:wgpu::VertexFormat::Float32x2}]}
        }
    }

    fn cube_verts(cx:f32,cy:f32,cz:f32,half:f32,color:[f32;3])->Vec<V3d>{
        let (x0,x1)=(cx-half,cx+half);let (y0,y1)=(cy-half,cy+half);let (z0,z1)=(cz-half,cz+half);let c=color;
        vec![V3d{position:[x0,y0,z1],color:c},V3d{position:[x1,y0,z1],color:c},V3d{position:[x1,y1,z1],color:c},V3d{position:[x0,y1,z1],color:c},V3d{position:[x1,y0,z0],color:c},V3d{position:[x0,y0,z0],color:c},V3d{position:[x0,y1,z0],color:c},V3d{position:[x1,y1,z0],color:c},V3d{position:[x0,y1,z1],color:c},V3d{position:[x1,y1,z1],color:c},V3d{position:[x1,y1,z0],color:c},V3d{position:[x0,y1,z0],color:c},V3d{position:[x0,y0,z0],color:c},V3d{position:[x1,y0,z0],color:c},V3d{position:[x1,y0,z1],color:c},V3d{position:[x0,y0,z1],color:c},V3d{position:[x1,y0,z1],color:c},V3d{position:[x1,y0,z0],color:c},V3d{position:[x1,y1,z0],color:c},V3d{position:[x1,y1,z1],color:c},V3d{position:[x0,y0,z0],color:c},V3d{position:[x0,y0,z1],color:c},V3d{position:[x0,y1,z1],color:c},V3d{position:[x0,y1,z0],color:c}]
    }
    fn cube_indices(base:u16)->Vec<u16>{let mut idx=Vec::new();for f in 0..6u16{let b=base+f*4;idx.extend_from_slice(&[b,b+1,b+2,b,b+2,b+3]);}idx}

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

    // ── Shaders ────────────────────────────────────────────────────────
    let color_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Color"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms{vp:mat4x4<f32>}
@group(0)@binding(0)var<uniform>uniforms:Uniforms;
struct VI{@location(0)position:vec3<f32>,@location(1)color:vec3<f32>}
struct VO{@builtin(position)clip_position:vec4<f32>,@location(0)color:vec3<f32>}
@vertex fn vs_main(input:VI)->VO{var out:VO;out.clip_position=uniforms.vp*vec4<f32>(input.position,1.0);out.color=input.color;return out;}
@fragment fn fs_main(in:VO)->@location(0)vec4<f32>{return vec4<f32>(in.color,1.0);}
"#.into()),
    });

    let tex_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Tex"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms{vp:mat4x4<f32>}
@group(0)@binding(0)var<uniform>uniforms:Uniforms;
@group(1)@binding(0)var t_tex:texture_2d<f32>;
@group(1)@binding(1)var s_tex:sampler;
struct VI{@location(0)position:vec3<f32>,@location(1)uv:vec2<f32>}
struct VO{@builtin(position)clip_position:vec4<f32>,@location(0)uv:vec2<f32>}
@vertex fn vs_main(input:VI)->VO{var out:VO;out.clip_position=uniforms.vp*vec4<f32>(input.position,1.0);out.uv=input.uv;return out;}
@fragment fn fs_main(in:VO)->@location(0)vec4<f32>{return textureSample(t_tex,s_tex,in.uv);}
"#.into()),
    });

    // ── Font atlas & text pipeline ─────────────────────────────────────
    let atlas=FontAtlas::generate(10,16);
    let font_tex=device.create_texture(&wgpu::TextureDescriptor{
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

    let text_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("Text"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms{vp:mat4x4<f32>}
@group(0)@binding(0)var<uniform>uniforms:Uniforms;
@group(1)@binding(0)var t_tex:texture_2d<f32>;
@group(1)@binding(1)var s_tex:sampler;
struct VI{@location(0)position:vec3<f32>,@location(1)uv:vec2<f32>,@location(2)color:vec4<f32>}
struct VO{@builtin(position)clip_position:vec4<f32>,@location(0)uv:vec2<f32>,@location(1)color:vec4<f32>}
@vertex fn vs_main(input:VI)->VO{var out:VO;out.clip_position=uniforms.vp*vec4<f32>(input.position,1.0);out.uv=input.uv;out.color=input.color;return out;}
@fragment fn fs_main(in:VO)->@location(0)vec4<f32>{var c=textureSample(t_tex,s_tex,in.uv);return vec4<f32>(in.color.rgb*c.rgb,in.color.a*c.a);}
"#.into()),
    });

    let text_ub=device.create_buffer(&wgpu::BufferDescriptor{label:Some("TextUB"),size:64,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    let text_bgl0=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("TBGL0"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
    let text_bgl1=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("TBGL1"),entries:&[
        wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
        wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
    ]});
    let text_bg0=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("TBG0"),layout:&text_bgl0,entries:&[wgpu::BindGroupEntry{binding:0,resource:text_ub.as_entire_binding()}]});
    let text_bg1=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("TBG1"),layout:&text_bgl1,entries:&[
        wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&font_view)},
        wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&font_samp)},
    ]});
    let text_pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("TPLL"),bind_group_layouts:&[&text_bgl0,&text_bgl1],push_constant_ranges:&[]});
    let text_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
        label:Some("TPipe2"),layout:Some(&text_pll),
        vertex:wgpu::VertexState{module:&text_shader,entry_point:"vs_main",buffers:&[TextVert::desc()]},
        fragment:Some(wgpu::FragmentState{module:&text_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),
        primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,front_face:wgpu::FrontFace::Cw,cull_mode:Some(wgpu::Face::Back),..Default::default()},
        depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
        multisample:wgpu::MultisampleState::default(),multiview:None,
    });

    // ── Render target for ortho view ───────────────────────────────────
    let rt_size=512u32;
    let rt_tex=device.create_texture(&wgpu::TextureDescriptor{
        label:Some("RT"),size:wgpu::Extent3d{width:rt_size,height:rt_size,depth_or_array_layers:1},
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
    });
    let rt_view=rt_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let rt_depth_tex=device.create_texture(&wgpu::TextureDescriptor{
        label:Some("RTDepth"),size:wgpu::Extent3d{width:rt_size,height:rt_size,depth_or_array_layers:1},
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[],
    });
    let rt_depth_view=rt_depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let rt_sampler=device.create_sampler(&wgpu::SamplerDescriptor{
        label:Some("RTSamp"),mag_filter:wgpu::FilterMode::Linear,min_filter:wgpu::FilterMode::Linear,..Default::default()
    });

    // ── QWERTY keyboard layout ───────────────────────────────────────
    // Each key: (label, x_offset, y_offset) — y grows downward (row 0 = top)
    // Spacing is uniform; offsets are in "key units" (1 unit = key width + gap).
    let key_w=0.22f32;       // key width  (world units)
    let key_gap=0.04f32;     // gap between keys
    let key_step=key_w+key_gap;
    let key_half=key_w*0.5;
    let cell_z=-5.0f32;
    let cube_half=key_w*0.45; // cube half-extent (slightly smaller than key)

    // QWERTY rows (top to bottom).  Each tuple: (label, x_offset_in_key_units).
    // Row 0 is the number row, row 4 is the spacebar row.
    let qwerty_rows:&[&[( &str,f32)]]=&[
        &[("1",0.0),("2",1.0),("3",2.0),("4",3.0),("5",4.0),("6",5.0),("7",6.0),("8",7.0),("9",8.0),("0",9.0)],
        &[("Q",0.0),("W",1.0),("E",2.0),("R",3.0),("T",4.0),("Y",5.0),("U",6.0),("I",7.0),("O",8.0),("P",9.0)],
        &[("A",0.0),("S",1.0),("D",2.0),("F",3.0),("G",4.0),("H",5.0),("J",6.0),("K",7.0),("L",8.0)],
        &[("Z",0.0),("X",1.0),("C",2.0),("V",3.0),("B",4.0),("N",5.0),("M",6.0)],
        &[("_",0.0)],  // spacebar placeholder
    ];
    // Row offsets (in key units) so the rows look like a real keyboard
    let row_offsets:&[f32]=&[0.0,0.0,0.0,0.0,0.0];
    let row_count=qwerty_rows.len();

    // Compute the world-space center of each key and the total bounds.
    // First find the max row width in key units.
    let max_row_width=qwerty_rows.iter().enumerate().map(|(ri,row)|{
        let last=row.last().unwrap();
        row_offsets[ri]+last.1+1.0
    }).fold(0.0f32,f32::max);
    let center_x=0.0f32;
    let center_y=0.0f32;
    // Build a flat list of keys: (label, cx, cy, row_index, col_index)
    #[allow(dead_code)]
    struct KeyInfo{label:&'static str,cx:f32,cy:f32,row:usize,col:usize}
    let mut key_list:Vec<KeyInfo>=Vec::new();
    for (ri,row) in qwerty_rows.iter().enumerate(){
        let row_width=row.last().map(|(_,x)|row_offsets[ri]+x+1.0).unwrap_or(1.0);
        let row_start_x=center_x-(row_width*key_step)*0.5+key_w*0.5;
        let row_y=center_y-((ri as f32)*key_step-key_w*0.5);
        for (ci,(_label,x_off)) in row.iter().enumerate(){
            let cx=row_start_x+(row_offsets[ri]+*x_off)*key_step;
            key_list.push(KeyInfo{label:_label,cx,cy:row_y,row:ri,col:ci});
        }
    }
    let key_count=key_list.len();

    // Total extent for the ortho rect
    let total_w=max_row_width*key_step;
    let total_h=row_count as f32*key_step;

    // ── Window plane (above the keyboard) ─────────────────────────────
    let plane_z=cell_z-0.5;
    let plane_hx=total_w*0.55;
    let plane_hy=total_h*0.55;
    let plane_y=key_w+0.15+plane_hy; // sit above the top row with a gap

    // ── App state ──────────────────────────────────────────────────────
    let mut camera=Camera{pos:[0.0,0.5,3.0],yaw:-std::f32::consts::FRAC_PI_2,pitch:0.15};
    let mut keys=std::collections::HashSet::<KeyCode>::new();
    let mut is_ortho=false;
    let mut hovered_plane=false;
    let mut mouse_cap=false;
    let mut o_was_pressed=false;
    let mut esc_was_pressed=false;
    let mut mouse_pos=[640.0f32,360.0];
    let mut last_frame=std::time::Instant::now();
    let mut hovered_key:Option<usize>=None;
    let mut clicked_key:Option<usize>=None;
    let mut click_count=0u32;

    println!("╔══════════════════════════════════════════════════╗");
    println!("║          Theta Engine – UI Demo (QWERTY)         ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  {} keys  |  ESC=fly mode  |  O=ortho/persp       ║",key_count);
    println!("╚══════════════════════════════════════════════════╝");

    // ── Helper: build text verts for a single character label ──────
    // Returns (vertices, indices) for a flat textured quad per opaque pixel.
    fn build_char_verts(ch:u8,atlas:&FontAtlas,world_x:f32,world_y:f32,world_z:f32,char_w:f32,char_h:f32,color:[f32;4])->(Vec<TextVert>,Vec<u16>){
        let ch=if ch>=32&&ch<127{ch}else{b'?'};
        let idx=(ch-32)as u32;
        let col=idx%atlas.cols;
        let row=idx/atlas.cols;
        let ax0=col*atlas.cell_width;
        let ay0=row*atlas.cell_height;
        let glyph_w=5u32;
        let glyph_h=7u32;
        let px_w=char_w/glyph_w as f32;
        let px_h=char_h/glyph_h as f32;
        let sx=atlas.cell_width/glyph_w;
        let sy=atlas.cell_height/glyph_h;
        let inv_tw=1.0/atlas.width as f32;
        let inv_th=1.0/atlas.height as f32;
        let pixel_on=|gx:i32,gy:i32|->bool{
            if gx<0||gy<0||gx>=glyph_w as i32||gy>=glyph_h as i32{return false;}
            let ax=ax0+(gx as u32)*sx+sx/2;
            let ay=ay0+(gy as u32)*sy+sy/2;
            let off=((ay*atlas.width+ax)*4+3)as usize;
            *atlas.pixels.get(off).unwrap_or(&0)>128
        };
        let mut verts=Vec::new();
        let mut indices=Vec::new();
        for gy in 0..glyph_h{
            for gx in 0..glyph_w{
                if !pixel_on(gx as i32,gy as i32){continue;}
                let wx0=world_x+gx as f32*px_w;
                let wx1=wx0+px_w;
                let wy1=world_y-gy as f32*px_h;
                let wy0=wy1-px_h;
                let qu0=(ax0+gx as u32*sx)as f32*inv_tw;
                let qu1=(ax0+(gx+1)as u32*sx)as f32*inv_tw;
                let qv0=(ay0+gy as u32*sy)as f32*inv_th;
                let qv1=(ay0+(gy+1)as u32*sy)as f32*inv_th;
                let base=verts.len() as u16;
                verts.extend_from_slice(&[
                    TextVert{position:[wx0,wy0,world_z],uv:[qu0,qv1],color},
                    TextVert{position:[wx1,wy0,world_z],uv:[qu1,qv1],color},
                    TextVert{position:[wx1,wy1,world_z],uv:[qu1,qv0],color},
                    TextVert{position:[wx0,wy1,world_z],uv:[qu0,qv0],color},
                ]);
                indices.extend_from_slice(&[base,base+2,base+1,base,base+3,base+2]);
            }
        }
        (verts,indices)
    }

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
                    match key{
                        KeyCode::KeyO=>{
                            let p=ke.state==ElementState::Pressed;
                            if p&&!o_was_pressed{
                                is_ortho=!is_ortho;
                                if is_ortho{ camera.pos=[0.0,0.0,0.0]; camera.yaw=-std::f32::consts::FRAC_PI_2; camera.pitch=0.0; }
                                else{ camera.pos=[0.0,0.5,3.0]; camera.yaw=-std::f32::consts::FRAC_PI_2; camera.pitch=0.15; }
                            }
                            o_was_pressed=p;
                        }
                        KeyCode::Escape=>{
                            let p=ke.state==ElementState::Pressed;
                            if p&&!esc_was_pressed{
                                mouse_cap=!mouse_cap;
                                if mouse_cap{let _=window.set_cursor_grab(winit::window::CursorGrabMode::Locked);window.set_cursor_visible(false);}
                                else{let _=window.set_cursor_grab(winit::window::CursorGrabMode::None);window.set_cursor_visible(true);}
                            }
                            esc_was_pressed=p;
                        }
                        _=>match ke.state{ElementState::Pressed=>{keys.insert(key);}ElementState::Released=>{keys.remove(&key);}},
                    }
                }
                WindowEvent::CursorMoved{position,..}=>{let s=window.scale_factor() as f32;mouse_pos=[position.x as f32/s,position.y as f32/s];}
                WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left,..}=>{
                    if let Some(ki)=hovered_key{click_count+=1;clicked_key=Some(ki);println!("Clicked '{}' count={}",key_list[ki].label,click_count);}
                }
                WindowEvent::MouseInput{state:ElementState::Released,button:MouseButton::Left,..}=>{clicked_key=None;}
                _=>{}
            }
            Event::DeviceEvent{event:DeviceEvent::MouseMotion{delta:(dx,dy)},..}=>{
                if mouse_cap&&!is_ortho{camera.yaw+=dx as f32*0.002;camera.pitch-=dy as f32*0.002;camera.pitch=camera.pitch.clamp(-1.55,1.55);}
            }
            Event::AboutToWait=>{
                // Movement
                let now=std::time::Instant::now();let dt=now.duration_since(last_frame).as_secs_f32();last_frame=now;
                let v=3.0f32*dt;
                if mouse_cap{
                    let fwd=camera.forward();let right=camera.right();
                    if keys.contains(&KeyCode::KeyW){camera.pos[0]+=fwd[0]*v;camera.pos[1]+=fwd[1]*v;camera.pos[2]+=fwd[2]*v;}
                    if keys.contains(&KeyCode::KeyS){camera.pos[0]-=fwd[0]*v;camera.pos[1]-=fwd[1]*v;camera.pos[2]-=fwd[2]*v;}
                    if keys.contains(&KeyCode::KeyD){camera.pos[0]-=right[0]*v;camera.pos[1]-=right[1]*v;camera.pos[2]-=right[2]*v;}
                    if keys.contains(&KeyCode::KeyA){camera.pos[0]+=right[0]*v;camera.pos[1]+=right[1]*v;camera.pos[2]+=right[2]*v;}
                    if keys.contains(&KeyCode::KeyQ){camera.pos[1]-=v;}
                    if keys.contains(&KeyCode::KeyE){camera.pos[1]+=v;}
                }

                let phys_w=surf_cfg.width as f32;let phys_h=surf_cfg.height as f32;
                let log_w=phys_w/window.scale_factor() as f32;
                let log_h=phys_h/window.scale_factor() as f32;

                // ── Step 1: Render ortho view to texture ──────────────
                {
                    let ortho_cam=Camera{pos:[0.0,0.0,0.0],yaw:-std::f32::consts::FRAC_PI_2,pitch:0.0};
                    let ortho_vp=build_vp(&ortho_cam,1.0,true);
                    let ortho_ub=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("OUB"),contents:bytemuck::cast_slice(&transpose(ortho_vp)),usage:wgpu::BufferUsages::UNIFORM});
                    queue.write_buffer(&text_ub,0,bytemuck::cast_slice(&transpose(ortho_vp)));
                    let ortho_bgl=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("OBGL"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
                    let ortho_bg=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("OBG"),layout:&ortho_bgl,entries:&[wgpu::BindGroupEntry{binding:0,resource:ortho_ub.as_entire_binding()}]});
                    let ortho_pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("OPLL"),bind_group_layouts:&[&ortho_bgl],push_constant_ranges:&[]});
                    let ortho_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                        label:Some("OPipe"),layout:Some(&ortho_pll),
                        vertex:wgpu::VertexState{module:&color_shader,entry_point:"vs_main",buffers:&[V3d::desc()]},
                        fragment:Some(wgpu::FragmentState{module:&color_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
                        primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,..Default::default()},
                        depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),multisample:wgpu::MultisampleState::default(),multiview:None,
                    });
                    // Build geometry for ortho render (with hover/click highlight)
                    let mut o_verts:Vec<V3d>=Vec::new();let mut o_idx:Vec<u16>=Vec::new();
                    for (ki,k) in key_list.iter().enumerate(){
                        let is_h=hovered_key==Some(ki);
                        let is_c=clicked_key==Some(ki);
                        let bc=if is_c{[0.0f32,0.9,0.0]}else if is_h{[0.0,0.7,0.0]}else{[0.2,0.5,0.2]};
                        let h=key_half;
                        let b=o_verts.len() as u16;
                        o_verts.extend_from_slice(&[V3d{position:[k.cx-h,k.cy-h,cell_z],color:bc},V3d{position:[k.cx+h,k.cy-h,cell_z],color:bc},V3d{position:[k.cx+h,k.cy+h,cell_z],color:bc},V3d{position:[k.cx-h,k.cy+h,cell_z],color:bc}]);
                        o_idx.extend_from_slice(&[b,b+1,b+2,b,b+2,b+3]);
                    }
                    let o_vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("OVB"),contents:bytemuck::cast_slice(&o_verts),usage:wgpu::BufferUsages::VERTEX});
                    let o_ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("OIB"),contents:bytemuck::cast_slice(&o_idx),usage:wgpu::BufferUsages::INDEX});

                    // Build text labels for ortho render
                    let o_label_h=0.06f32;
                    let o_label_w=o_label_h*(5.0/7.0);
                    let o_label_color=[1.0f32,0.95,0.7,1.0];
                    let o_label_z=cell_z+key_half+0.02;
                    let mut o_text_verts:Vec<TextVert>=Vec::new();
                    let mut o_text_idx:Vec<u16>=Vec::new();
                    for k in key_list.iter(){
                        let ch=k.label.bytes().next().unwrap_or(b'?');
                        let lx=k.cx-o_label_w*0.5;
                        let ly=k.cy+o_label_h*0.5;
                        let (mut cv,ci)=build_char_verts(ch,&atlas,lx,ly,o_label_z,o_label_w,o_label_h,o_label_color);
                        let base=o_text_verts.len() as u16;
                        o_text_verts.append(&mut cv);
                        for idx in &ci{o_text_idx.push(*idx+base);}
                    }
                    let (o_tvb,o_tib)=if !o_text_verts.is_empty(){
                        (Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("OTVB"),contents:bytemuck::cast_slice(&o_text_verts),usage:wgpu::BufferUsages::VERTEX})),
                         Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("OTIB"),contents:bytemuck::cast_slice(&o_text_idx),usage:wgpu::BufferUsages::INDEX})))
                    }else{(None,None)};

                    let mut enc=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("OFrame")});
                    {let mut p=enc.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("OPass"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:&rt_view,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.04,g:0.06,b:0.09,a:1.0}),store:wgpu::StoreOp::Store}})],depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment{view:&rt_depth_view,depth_ops:Some(wgpu::Operations{load:wgpu::LoadOp::Clear(1.0),store:wgpu::StoreOp::Store}),stencil_ops:None}),timestamp_writes:None,occlusion_query_set:None});
                        // Key quads
                        p.set_pipeline(&ortho_pipe);p.set_bind_group(0,&ortho_bg,&[]);
                        p.set_vertex_buffer(0,o_vb.slice(..));p.set_index_buffer(o_ib.slice(..),wgpu::IndexFormat::Uint16);
                        p.draw_indexed(0..o_idx.len() as u32,0,0..1);
                        // Text labels
                        if o_tvb.is_some()&&o_tib.is_some(){
                            p.set_pipeline(&text_pipe);p.set_bind_group(0,&text_bg0,&[]);p.set_bind_group(1,&text_bg1,&[]);
                            p.set_vertex_buffer(0,o_tvb.as_ref().unwrap().slice(..));p.set_index_buffer(o_tib.as_ref().unwrap().slice(..),wgpu::IndexFormat::Uint16);
                            p.draw_indexed(0..o_text_idx.len() as u32,0,0..1);
                        }
                    }
                    queue.submit(std::iter::once(enc.finish()));
                }

                // ── Step 2: Main render ────────────────────────────────
                let vp=build_vp(&camera,phys_w/phys_h,is_ortho);
                let vp_t=transpose(vp);

                // Hit test
                hovered_key=None;
                hovered_plane=false;
                // ── Window plane hit test → ray into ortho scene ──
                {
                    let inv_vp2=inv_mat4(&vp);
                    let ndc_x=(mouse_pos[0]/log_w)*2.0-1.0;let ndc_y=1.0-(mouse_pos[1]/log_h)*2.0;
                    let nw=mat4_mul_vec4(&inv_vp2,[ndc_x,ndc_y,-1.0,1.0]);
                    let fw=mat4_mul_vec4(&inv_vp2,[ndc_x,ndc_y,1.0,1.0]);
                    let nw3=[nw[0]/nw[3],nw[1]/nw[3],nw[2]/nw[3]];
                    let fw3=[fw[0]/fw[3],fw[1]/fw[3],fw[2]/fw[3]];
                    let rd=normalize(sub(fw3,nw3));
                    if rd[2].abs()>1e-8{
                        let t=(plane_z-nw3[2])/rd[2];
                        if t>0.0{
                            let hx=nw3[0]+rd[0]*t;let hy=nw3[1]+rd[1]*t;
                            // Check hit against the plane quad (in local plane coords)
                            let local_y=hy-plane_y;
                            if hx>=-plane_hx&&hx<=plane_hx&&local_y>=-plane_hy&&local_y<=plane_hy{
                                hovered_plane=true;
                                // Map UV on the plane to ortho-view normalized coords.
                                // UV: u = (hx + plane_hx) / (2*plane_hx)
                                //     v = (plane_hy - local_y) / (2*plane_hy)  (top=0,bottom=1)
                                // The ortho camera renders the keyboard with ortho(2,1,-10,10)
                                // which maps scene X/Y directly to clip [-1,1].
                                // So ortho_norm_x = hx / plane_hx (approx, since plane shows the scene)
                                // Actually: the ortho VP maps world x∈[-1,1] → clip [-1,1]
                                // The plane UV maps to the same clip space, so:
                                //   scene_x = (u*2.0 - 1.0)  (ortho normalized)
                                //   scene_y = (1.0 - v*2.0)  (ortho normalized, Y up)
                                let u=(hx+plane_hx)/(2.0*plane_hx);
                                let v=(plane_hy-local_y)/(2.0*plane_hy);
                                let onx=u*2.0-1.0; // ortho normalized x [-1,1]
                                let ony=1.0-v*2.0; // ortho normalized y [-1,1]
                                // Find the key whose center is closest in ortho-normalized space
                                let mut best_dist=f32::INFINITY;
                                for (ki,k) in key_list.iter().enumerate(){
                                    // Normalize key center to ortho space
                                    // The ortho cam at [0,0,0] yaw=-PI/2 pitch=0 looks along -Z
                                    // with ortho(size=2, aspect=1). The view maps:
                                    //   clip_x = world_x / 1.0  (since ortho half-width = 1)
                                    //   clip_y = world_y / 1.0  (since ortho half-height = 1)
                                    // So ortho_norm = world_pos for this camera
                                    let kx=k.cx;
                                    let ky=k.cy;
                                    let dx=onx-kx;
                                    let dy=ony-ky;
                                    let dist=dx*dx+dy*dy;
                                    // Check if within key half-extent (in ortho-normalized units)
                                    let kh=key_half;
                                    if dx.abs()<=kh&&dy.abs()<=kh&&dist<best_dist{
                                        best_dist=dist;
                                        hovered_key=Some(ki);
                                    }
                                }
                            }
                        }
                    }
                }

                // Direct key hit test (screen-space AABB for ortho, ray-AABB for perspective)
                if is_ortho{
                    for (ki,k) in key_list.iter().enumerate(){
                        let corners=[[k.cx-key_half,k.cy-key_half,cell_z],[k.cx+key_half,k.cy-key_half,cell_z],[k.cx+key_half,k.cy+key_half,cell_z],[k.cx-key_half,k.cy+key_half,cell_z]];
                        let mut sc_min=[1e20f32,1e20f32];let mut sc_max=[-1e20f32,-1e20f32];
                        for c in &corners{let cl=vp_to_clip(&vp_t,*c);if cl[3]<=0.0{continue;}let n=[cl[0]/cl[3],cl[1]/cl[3]];let sx=(n[0]+1.0)*0.5*log_w;let sy=(1.0-n[1])*0.5*log_h;sc_min[0]=sc_min[0].min(sx);sc_min[1]=sc_min[1].min(sy);sc_max[0]=sc_max[0].max(sx);sc_max[1]=sc_max[1].max(sy);}
                        if mouse_pos[0]>=sc_min[0]&&mouse_pos[0]<=sc_max[0]&&mouse_pos[1]>=sc_min[1]&&mouse_pos[1]<=sc_max[1]{hovered_key=Some(ki);}
                    }
                } else {
                    let inv_vp=inv_mat4(&vp);
                    let ro=camera.pos;
                    let ndc_x=(mouse_pos[0]/log_w)*2.0-1.0;let ndc_y=1.0-(mouse_pos[1]/log_h)*2.0;
                    let nw=mat4_mul_vec4(&inv_vp,[ndc_x,ndc_y,-1.0,1.0]);
                    let fw=mat4_mul_vec4(&inv_vp,[ndc_x,ndc_y,1.0,1.0]);
                    let nw3=[nw[0]/nw[3],nw[1]/nw[3],nw[2]/nw[3]];
                    let fw3=[fw[0]/fw[3],fw[1]/fw[3],fw[2]/fw[3]];
                    let rd=normalize(sub(fw3,nw3));
                    let mut best_t=f32::INFINITY;
                    for (ki,k) in key_list.iter().enumerate(){
                        let h=cube_half;
                        let (a0,a1)=([k.cx-h,k.cy-h,cell_z-h],[k.cx+h,k.cy+h,cell_z+h]);
                        let (mut tmin,mut tmax)=(f32::NEG_INFINITY,f32::INFINITY);
                        let mut hit=true;
                        for i in 0..3{
                            if rd[i].abs()<1e-8{if ro[i]<a0[i]||ro[i]>a1[i]{hit=false;break;}}
                            else{let t1=(a0[i]-ro[i])/rd[i];let t2=(a1[i]-ro[i])/rd[i];let(tlo,thi)=if t1<t2{(t1,t2)}else{(t2,t1)};tmin=tmin.max(tlo);tmax=tmax.min(thi);}
                        }
                        if hit&&tmin<=tmax&&tmax>0.0&&tmin<best_t{best_t=tmin;hovered_key=Some(ki);}
                    }
                }

                // Build geometry
                let mut all_verts:Vec<V3d>=Vec::new();let mut all_idx:Vec<u16>=Vec::new();
                for (ki,k) in key_list.iter().enumerate(){
                    let is_h=hovered_key==Some(ki);
                    let is_c=clicked_key==Some(ki);
                    let bc=if is_c{[0.0f32,0.9,0.0]}else if is_h{[0.0,0.7,0.0]}else{[0.2,0.5,0.2]};
                    if is_ortho{
                        let h=key_half;let b=all_verts.len() as u16;
                        all_verts.extend_from_slice(&[V3d{position:[k.cx-h,k.cy-h,cell_z],color:bc},V3d{position:[k.cx+h,k.cy-h,cell_z],color:bc},V3d{position:[k.cx+h,k.cy+h,cell_z],color:bc},V3d{position:[k.cx-h,k.cy+h,cell_z],color:bc}]);
                        all_idx.extend_from_slice(&[b,b+1,b+2,b,b+2,b+3]);
                    } else {
                        let h=cube_half;let b=all_verts.len() as u16;
                        all_verts.extend_from_slice(&cube_verts(k.cx,k.cy,cell_z,h,bc));
                        all_idx.extend_from_slice(&cube_indices(b));
                    }
                }

                // ── Text labels above each key ────────────────────────
                let label_h=0.06f32; // height of each label in world units
                let label_w=label_h*(5.0/7.0); // match glyph aspect (5 wide × 7 tall)
                let label_color=[1.0f32,0.95,0.7,1.0];
                let label_z=cell_z+cube_half+0.08; // float above the cube top
                let mut text_verts:Vec<TextVert>=Vec::new();
                let mut text_idx:Vec<u16>=Vec::new();
                for k in key_list.iter(){
                    let ch=k.label.bytes().next().unwrap_or(b'?');
                    let lx=k.cx-label_w*0.5;
                    let ly=k.cy+label_h*0.5;
                    let (mut cv,ci)=build_char_verts(ch,&atlas,lx,ly,label_z,label_w,label_h,label_color);
                    let base=text_verts.len() as u16;
                    text_verts.append(&mut cv);
                    for idx in &ci{text_idx.push(*idx+base);}
                }

                // Window plane with texture (offset above keyboard)
                let py=plane_y;
                let plane_verts:Vec<TV3d>=vec![
                    TV3d{position:[-plane_hx,py-plane_hy,plane_z],uv:[0.0,1.0]},
                    TV3d{position:[plane_hx,py-plane_hy,plane_z],uv:[1.0,1.0]},
                    TV3d{position:[plane_hx,py+plane_hy,plane_z],uv:[1.0,0.0]},
                    TV3d{position:[-plane_hx,py+plane_hy,plane_z],uv:[0.0,0.0]},
                ];
                let plane_idx:&[u16]=&[0,1,2,0,2,3];

                // Hover border for window plane (thin quad outline)
                let border=0.015f32;
                let border_verts:Vec<V3d>=if hovered_plane{
                    let b=plane_z+0.001;
                    let hx=plane_hx+border;let hy=plane_hy+border;
                    let bc=[0.4f32,0.8,1.0];
                    let t0=V3d{position:[-hx,py+hy+border,b],color:bc};let t1=V3d{position:[hx,py+hy+border,b],color:bc};let t2=V3d{position:[hx,py+hy,b],color:bc};let t3=V3d{position:[-hx,py+hy,b],color:bc};
                    let b0=V3d{position:[-hx,py-hy,b],color:bc};let b1=V3d{position:[hx,py-hy,b],color:bc};let b2=V3d{position:[hx,py-hy-border,b],color:bc};let b3=V3d{position:[-hx,py-hy-border,b],color:bc};
                    let l0=V3d{position:[-hx,py+hy,b],color:bc};let l1=V3d{position:[-hx+border,py+hy,b],color:bc};let l2=V3d{position:[-hx+border,py-hy,b],color:bc};let l3=V3d{position:[-hx,py-hy,b],color:bc};
                    let r0=V3d{position:[hx-border,py+hy,b],color:bc};let r1=V3d{position:[hx,py+hy,b],color:bc};let r2=V3d{position:[hx,py-hy,b],color:bc};let r3=V3d{position:[hx-border,py-hy,b],color:bc};
                    vec![t0,t1,t2,t0,t2,t3,b0,b1,b2,b0,b2,b3,l0,l1,l2,l0,l2,l3,r0,r1,r2,r0,r2,r3]
                }else{vec![]};

                // Uniforms & pipelines
                let ub_buf=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("UB"),contents:bytemuck::cast_slice(&vp_t),usage:wgpu::BufferUsages::UNIFORM});
                let bgl0=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("BGL0"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
                let bg0=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("BG0"),layout:&bgl0,entries:&[wgpu::BindGroupEntry{binding:0,resource:ub_buf.as_entire_binding()}]});
                let pll0=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("PLL0"),bind_group_layouts:&[&bgl0],push_constant_ranges:&[]});
                let color_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                    label:Some("CPipe"),layout:Some(&pll0),
                    vertex:wgpu::VertexState{module:&color_shader,entry_point:"vs_main",buffers:&[V3d::desc()]},
                    fragment:Some(wgpu::FragmentState{module:&color_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
                    primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,..Default::default()},
                    depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
                    multisample:wgpu::MultisampleState::default(),multiview:None,
                });

                let bgl1=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("BGL1"),entries:&[
                    wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
                    wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
                ]});
                let bg1=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("BG1"),layout:&bgl1,entries:&[wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&rt_view)},wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&rt_sampler)}]});
                let pll1=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("PLL1"),bind_group_layouts:&[&bgl0,&bgl1],push_constant_ranges:&[]});
                let tex_pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                    label:Some("TPipe"),layout:Some(&pll1),
                    vertex:wgpu::VertexState{module:&tex_shader,entry_point:"vs_main",buffers:&[TV3d::desc()]},
                    fragment:Some(wgpu::FragmentState{module:&tex_shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
                    primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,..Default::default()},
                    depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState::default()}),
                    multisample:wgpu::MultisampleState::default(),multiview:None,
                });

                let vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("VB"),contents:bytemuck::cast_slice(&all_verts),usage:wgpu::BufferUsages::VERTEX});
                let ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("IB"),contents:bytemuck::cast_slice(&all_idx),usage:wgpu::BufferUsages::INDEX});
                let pvb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("PVB"),contents:bytemuck::cast_slice(&plane_verts),usage:wgpu::BufferUsages::VERTEX});
                let pib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("PIB"),contents:bytemuck::cast_slice(plane_idx),usage:wgpu::BufferUsages::INDEX});
                let bvb=if hovered_plane&&!border_verts.is_empty(){Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("BVB"),contents:bytemuck::cast_slice(&border_verts),usage:wgpu::BufferUsages::VERTEX}))}else{None};
                let (text_vb,text_ib)=if !text_verts.is_empty(){
                    queue.write_buffer(&text_ub,0,bytemuck::cast_slice(&vp_t));
                    (Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("TVB"),contents:bytemuck::cast_slice(&text_verts),usage:wgpu::BufferUsages::VERTEX})),
                     Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("TIB"),contents:bytemuck::cast_slice(&text_idx),usage:wgpu::BufferUsages::INDEX})))
                }else{(None,None)};

                // Create depth texture
                let depth_tex=device.create_texture(&wgpu::TextureDescriptor{
                    label:Some("Depth"),size:wgpu::Extent3d{width:surf_cfg.width,height:surf_cfg.height,depth_or_array_layers:1},
                    mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,
                    usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
                });
                let depth_view=depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

                let Ok(frame)=surface.get_current_texture()else{return};
                let view=frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut enc=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("Frame")});
                {let mut p=enc.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("Pass"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:&view,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.04,g:0.06,b:0.09,a:1.0}),store:wgpu::StoreOp::Store}})],depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment{view:&depth_view,depth_ops:Some(wgpu::Operations{load:wgpu::LoadOp::Clear(1.0),store:wgpu::StoreOp::Store}),stencil_ops:None}),timestamp_writes:None,occlusion_query_set:None});
                    // Cubes
                    p.set_pipeline(&color_pipe);p.set_bind_group(0,&bg0,&[]);
                    p.set_vertex_buffer(0,vb.slice(..));p.set_index_buffer(ib.slice(..),wgpu::IndexFormat::Uint16);
                    p.draw_indexed(0..all_idx.len() as u32,0,0..1);
                    // Window plane with ortho texture
                    p.set_pipeline(&tex_pipe);p.set_bind_group(0,&bg0,&[]);p.set_bind_group(1,&bg1,&[]);
                    p.set_vertex_buffer(0,pvb.slice(..));p.set_index_buffer(pib.slice(..),wgpu::IndexFormat::Uint16);
                    p.draw_indexed(0..plane_idx.len() as u32,0,0..1);
                    // Hover border
                    if let Some(ref bvb)=bvb{
                        p.set_pipeline(&color_pipe);p.set_bind_group(0,&bg0,&[]);
                        p.set_vertex_buffer(0,bvb.slice(..));
                        p.draw(0..border_verts.len() as u32,0..1);
                    }
                    // Text labels
                    if text_vb.is_some()&&text_ib.is_some(){
                        p.set_pipeline(&text_pipe);p.set_bind_group(0,&text_bg0,&[]);p.set_bind_group(1,&text_bg1,&[]);
                        p.set_vertex_buffer(0,text_vb.as_ref().unwrap().slice(..));p.set_index_buffer(text_ib.as_ref().unwrap().slice(..),wgpu::IndexFormat::Uint16);
                        p.draw_indexed(0..text_idx.len() as u32,0,0..1);
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
