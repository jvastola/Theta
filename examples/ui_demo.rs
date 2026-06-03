//! UI Demo — NxN inventory grid. Ortho mode shows flat buttons, perspective shows 3D cubes.
//! ESC = toggle cursor capture + fly camera mode. O = toggle ortho/perspective.
//! Run: cargo run --example ui_demo --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
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

    fn mat4_mul_vec4(m:&[[f32;4];4],v:[f32;4])->[f32;4]{
        [m[0][0]*v[0]+m[0][1]*v[1]+m[0][2]*v[2]+m[0][3]*v[3],
         m[1][0]*v[0]+m[1][1]*v[1]+m[1][2]*v[2]+m[1][3]*v[3],
         m[2][0]*v[0]+m[2][1]*v[1]+m[2][2]*v[2]+m[2][3]*v[3],
         m[3][0]*v[0]+m[3][1]*v[1]+m[3][2]*v[2]+m[3][3]*v[3]]
    }

    fn inv_mat4(m:&[[f32;4];4])->[[f32;4];4]{
        // Compute inverse using cofactor expansion
        let mut inv = [[0.0f32;4];4];
        let mut s = [[0.0f32;4];4];
        // Copy matrix
        for i in 0..4{for j in 0..4{s[i][j]=m[i][j];}}
        // Gauss-Jordan elimination
        let mut inv_out = [[0.0f32;4];4];
        for i in 0..4{inv_out[i][i]=1.0;}
        for col in 0..4{
            // Find pivot
            let mut pivot = col;
            for row in (col+1)..4{
                if s[row][col].abs()>s[pivot][col].abs(){pivot=row;}
            }
            if s[pivot][col].abs()<1e-10{
                return inv_out; // Singular, return identity-ish
            }
            // Swap rows
            if pivot!=col{
                for j in 0..4{let tmp=s[col][j];s[col][j]=s[pivot][j];s[pivot][j]=tmp;}
                for j in 0..4{let tmp=inv_out[col][j];inv_out[col][j]=inv_out[pivot][j];inv_out[pivot][j]=tmp;}
            }
            // Scale pivot row
            let scale=1.0/s[col][col];
            for j in 0..4{s[col][j]*=scale;inv_out[col][j]*=scale;}
            // Eliminate column
            for row in 0..4{
                if row==col{continue;}
                let factor=s[row][col];
                for j in 0..4{s[row][j]-=factor*s[col][j];inv_out[row][j]-=factor*inv_out[col][j];}
            }
        }
        inv_out
    }

    fn vp_to_clip(vp:&[[f32;4];4],pos:[f32;3])->[f32;4]{
        [vp[0][0]*pos[0]+vp[0][1]*pos[1]+vp[0][2]*pos[2]+vp[0][3],
         vp[1][0]*pos[0]+vp[1][1]*pos[1]+vp[1][2]*pos[2]+vp[1][3],
         vp[2][0]*pos[0]+vp[2][1]*pos[1]+vp[2][2]*pos[2]+vp[2][3],
         vp[3][0]*pos[0]+vp[3][1]*pos[1]+vp[3][2]*pos[2]+vp[3][3]]
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

    // ── Cube vertices (for perspective mode) ──────────────────────────
    fn cube_verts(cx:f32,cy:f32,cz:f32,half:f32,color:[f32;3])->Vec<V3d>{
        let (x0,x1)=(cx-half,cx+half);let (y0,y1)=(cy-half,cy+half);let (z0,z1)=(cz-half,cz+half);
        let c=color;
        vec![
            // front
            V3d{position:[x0,y0,z1],color:c},V3d{position:[x1,y0,z1],color:c},V3d{position:[x1,y1,z1],color:c},V3d{position:[x0,y1,z1],color:c},
            // back
            V3d{position:[x1,y0,z0],color:c},V3d{position:[x0,y0,z0],color:c},V3d{position:[x0,y1,z0],color:c},V3d{position:[x1,y1,z0],color:c},
            // top
            V3d{position:[x0,y1,z1],color:c},V3d{position:[x1,y1,z1],color:c},V3d{position:[x1,y1,z0],color:c},V3d{position:[x0,y1,z0],color:c},
            // bottom
            V3d{position:[x0,y0,z0],color:c},V3d{position:[x1,y0,z0],color:c},V3d{position:[x1,y0,z1],color:c},V3d{position:[x0,y0,z1],color:c},
            // right
            V3d{position:[x1,y0,z1],color:c},V3d{position:[x1,y0,z0],color:c},V3d{position:[x1,y1,z0],color:c},V3d{position:[x1,y1,z1],color:c},
            // left
            V3d{position:[x0,y0,z0],color:c},V3d{position:[x0,y0,z1],color:c},V3d{position:[x0,y1,z1],color:c},V3d{position:[x0,y1,z0],color:c},
        ]
    }

    fn cube_indices(base:u16)->Vec<u16>{
        let mut idx=Vec::new();
        for face in 0..6u16{
            let b=base+face*4;
            idx.extend_from_slice(&[b,b+1,b+2,b,b+2,b+3]);
        }
        idx
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

    // ── Shader ─────────────────────────────────────────────────────────
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{
        label:Some("3D"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms{vp:mat4x4<f32>}
@group(0)@binding(0)var<uniform>uniforms:Uniforms;
struct VertexInput{@location(0)position:vec3<f32>,@location(1)color:vec3<f32>}
struct VertexOutput{@builtin(position)clip_position:vec4<f32>,@location(0)color:vec3<f32>}
@vertex fn vs_main(input:VertexInput)->VertexOutput{
    var out:VertexOutput;
    out.clip_position=uniforms.vp*vec4<f32>(input.position,1.0);
    out.color=input.color;
    return out;
}
@fragment fn fs_main(in:VertexOutput)->@location(0)vec4<f32>{return vec4<f32>(in.color,1.0);}
"#.into()),
    });

    // ── Grid config ────────────────────────────────────────────────────
    let grid_n:usize=5;
    let cell_size=0.12f32;
    let cell_gap=0.03f32;
    let total_size=grid_n as f32*(cell_size*2.0+cell_gap)-cell_gap;
    let grid_origin_x=-total_size/2.0+cell_size;
    let grid_origin_y=total_size/2.0-cell_size;
    let cell_z=-5.0f32; // cells at Z=-5 in front of camera (camera looks down -Z)
    let cube_half=cell_size*0.7; // cube half-size for perspective mode (fits within cell)

    // ── App state ──────────────────────────────────────────────────────
    let mut camera=Camera{pos:[0.0,0.0,0.0],yaw:-std::f32::consts::FRAC_PI_2,pitch:0.0};
    let mut keys=std::collections::HashSet::<KeyCode>::new();
    let mut is_ortho=true;
    let mut mouse_cap=false;
    let mut o_was_pressed=false;
    let mut esc_was_pressed=false;
    let mut mouse_pos=[640.0f32,360.0];
    let mut last_frame=std::time::Instant::now();
    let mut hovered_cell:Option<(usize,usize)>=None;
    let mut clicked_cell:Option<(usize,usize)>=None;
    let mut click_count=0u32;

    println!("╔══════════════════════════════════════════════════╗");
    println!("║          Theta Engine – UI Demo                  ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  {}x{} inventory grid                             ║",grid_n,grid_n);
    println!("║  ESC = toggle cursor / fly camera                ║");
    println!("║  O   = toggle ortho/perspective                  ║");
    println!("║  WASD+QE = move  Mouse = look (when captured)    ║");
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
                    match key{
                        KeyCode::KeyO=>{
                            let pressed=ke.state==ElementState::Pressed;
                            if pressed&&!o_was_pressed{
                                is_ortho=!is_ortho;
                                if is_ortho{ camera.pos=[0.0,0.0,0.0]; camera.yaw=-std::f32::consts::FRAC_PI_2; camera.pitch=0.0; }
                            }
                            o_was_pressed=pressed;
                        }
                        KeyCode::Escape=>{
                            let pressed=ke.state==ElementState::Pressed;
                            if pressed&&!esc_was_pressed{
                                mouse_cap=!mouse_cap;
                                if mouse_cap{
                                    let _=window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                                    window.set_cursor_visible(false);
                                }else{
                                    let _=window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                }
                            }
                            esc_was_pressed=pressed;
                        }
                        _=>match ke.state{ElementState::Pressed=>{keys.insert(key);}ElementState::Released=>{keys.remove(&key);}},
                    }
                }
                WindowEvent::CursorMoved{position,..}=>{
                    let s=window.scale_factor() as f32;
                    mouse_pos=[position.x as f32/s,position.y as f32/s];
                }
                WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left,..}=>{
                    eprintln!("CLICK: hover={:?} mouse={:.0},{:.0} ortho={}",hovered_cell,mouse_pos[0],mouse_pos[1],is_ortho);
                    if let Some((r,c))=hovered_cell{
                        click_count+=1;
                        clicked_cell=Some((r,c));
                        println!("Clicked cell [{},{}] count={}",r,c,click_count);
                    }
                }
                WindowEvent::MouseInput{state:ElementState::Released,button:MouseButton::Left,..}=>{clicked_cell=None;}
                _=>{}
            }
            Event::DeviceEvent{event:DeviceEvent::MouseMotion{delta:(dx,dy)},..}=>{
                if mouse_cap&&!is_ortho{
                    camera.yaw+=dx as f32*0.002;
                    camera.pitch-=dy as f32*0.002;
                    camera.pitch=camera.pitch.clamp(-1.55,1.55);
                }
            }
            Event::AboutToWait=>{
                // WASD movement
                let now=std::time::Instant::now();let dt=now.duration_since(last_frame).as_secs_f32();last_frame=now;
                let v=3.0f32*dt;
                if mouse_cap{
                    // Fly mode: WASD + mouse look
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
                let vp=build_vp(&camera,phys_w/phys_h,is_ortho);
                let vp_t=transpose(vp);

                // ── Hit test each cell ─────────────────────────────────
                hovered_cell=None;
                if is_ortho{
                    // Ortho: project cell corners to screen, do 2D AABB test
                    for row in 0..grid_n{for col in 0..grid_n{
                        let cx=grid_origin_x+col as f32*(cell_size*2.0+cell_gap);
                        let cy=grid_origin_y-row as f32*(cell_size*2.0+cell_gap);
                        let corners=[
                            [cx-cell_size,cy-cell_size,cell_z],[cx+cell_size,cy-cell_size,cell_z],
                            [cx+cell_size,cy+cell_size,cell_z],[cx-cell_size,cy+cell_size,cell_z],
                        ];
                        let mut sc_min=[1e20f32,1e20f32];let mut sc_max=[-1e20f32,-1e20f32];
                        for corner in &corners{
                            let clip=vp_to_clip(&vp_t,*corner);
                            if clip[3]<=0.0{continue;}
                            let ndc=[clip[0]/clip[3],clip[1]/clip[3]];
                            let sx=(ndc[0]+1.0)*0.5*log_w;
                            let sy=(1.0-ndc[1])*0.5*log_h;
                            sc_min[0]=sc_min[0].min(sx);sc_min[1]=sc_min[1].min(sy);
                            sc_max[0]=sc_max[0].max(sx);sc_max[1]=sc_max[1].max(sy);
                        }
                        if mouse_pos[0]>=sc_min[0]&&mouse_pos[0]<=sc_max[0]&&mouse_pos[1]>=sc_min[1]&&mouse_pos[1]<=sc_max[1]{
                            hovered_cell=Some((row,col));
                        }
                    }}
                } else {
                    // Perspective: cast ray from camera through mouse position
                    // Build inverse VP to go from screen to world
                    let inv_vp = inv_mat4(&vp);
                    let ray_origin = camera.pos;
                    // NDC from mouse position
                    let ndc_x = (mouse_pos[0]/log_w)*2.0-1.0;
                    let ndc_y = 1.0-(mouse_pos[1]/log_h)*2.0;
                    // Two points on the ray in NDC (near and far)
                    let near_ndc = [ndc_x, ndc_y, -1.0, 1.0];
                    let far_ndc = [ndc_x, ndc_y, 1.0, 1.0];
                    // Unproject to world space
                    let near_world = mat4_mul_vec4(&inv_vp, near_ndc);
                    let far_world = mat4_mul_vec4(&inv_vp, far_ndc);
                    let near_w = [near_world[0]/near_world[3], near_world[1]/near_world[3], near_world[2]/near_world[3]];
                    let far_w = [far_world[0]/far_world[3], far_world[1]/far_world[3], far_world[2]/far_world[3]];
                    let ray_dir = normalize(sub(far_w, near_w));
                    // Test ray against each cube AABB
                    let mut best_t = f32::INFINITY;
                    for row in 0..grid_n{for col in 0..grid_n{
                        let cx=grid_origin_x+col as f32*(cell_size*2.0+cell_gap);
                        let cy=grid_origin_y-row as f32*(cell_size*2.0+cell_gap);
                        let h = cube_half;
                        let aabb_min = [cx-h, cy-h, cell_z-h];
                        let aabb_max = [cx+h, cy+h, cell_z+h];
                        let mut tmin = f32::NEG_INFINITY;
                        let mut tmax = f32::INFINITY;
                        let mut hit = true;
                        for i in 0..3{
                            if ray_dir[i].abs()<1e-8{
                                if ray_origin[i]<aabb_min[i]||ray_origin[i]>aabb_max[i]{hit=false;break;}
                            }else{
                                let t1=(aabb_min[i]-ray_origin[i])/ray_dir[i];
                                let t2=(aabb_max[i]-ray_origin[i])/ray_dir[i];
                                let (tlo,thi)=if t1<t2{(t1,t2)}else{(t2,t1)};
                                tmin=tmin.max(tlo); tmax=tmax.min(thi);
                            }
                        }
                        if hit && tmin<=tmax && tmax>0.0 && tmin<best_t{
                            best_t=tmin;
                            hovered_cell=Some((row,col));
                        }
                    }}
                }

                // ── Build geometry ─────────────────────────────────────
                let mut all_verts:Vec<V3d>=Vec::new();
                let mut all_idx:Vec<u16>=Vec::new();
                for row in 0..grid_n{for col in 0..grid_n{
                    let cx=grid_origin_x+col as f32*(cell_size*2.0+cell_gap);
                    let cy=grid_origin_y-row as f32*(cell_size*2.0+cell_gap);
                    let is_hovered=hovered_cell==Some((row,col));
                    let is_clicked=clicked_cell==Some((row,col));
                    let base_color=if is_clicked{[0.0f32,0.9,0.0]}
                        else if is_hovered{[0.0,0.7,0.0]}
                        else if (row+col)%2==0{[0.2,0.5,0.2]}
                        else{[0.15,0.4,0.15]};
                    if is_ortho{
                        // Flat quad
                        let h=cell_size;
                        let base=all_verts.len() as u16;
                        all_verts.extend_from_slice(&[
                            V3d{position:[cx-h,cy-h,cell_z],color:base_color},
                            V3d{position:[cx+h,cy-h,cell_z],color:base_color},
                            V3d{position:[cx+h,cy+h,cell_z],color:base_color},
                            V3d{position:[cx-h,cy+h,cell_z],color:base_color},
                        ]);
                        all_idx.extend_from_slice(&[base,base+1,base+2,base,base+2,base+3]);
                    }else{
                        // 3D cube
                        let h=cube_half;
                        let base=all_verts.len() as u16;
                        all_verts.extend_from_slice(&cube_verts(cx,cy,cell_z,h,base_color));
                        all_idx.extend_from_slice(&cube_indices(base));
                    }
                }}

                // ── Uniform buffer & pipeline ───────────────────────────
                let ub_buf=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("UB"),contents:bytemuck::cast_slice(&vp_t),usage:wgpu::BufferUsages::UNIFORM});
                let bgl=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("BGL"),entries:&[wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None}]});
                let bg=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("BG"),layout:&bgl,entries:&[wgpu::BindGroupEntry{binding:0,resource:ub_buf.as_entire_binding()}]});
                let pll=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("PLL"),bind_group_layouts:&[&bgl],push_constant_ranges:&[]});
                let pipe=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
                    label:Some("Pipe"),layout:Some(&pll),
                    vertex:wgpu::VertexState{module:&shader,entry_point:"vs_main",buffers:&[V3d::desc()]},
                    fragment:Some(wgpu::FragmentState{module:&shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::REPLACE),write_mask:wgpu::ColorWrites::ALL})]}),
                    primitive:wgpu::PrimitiveState{topology:wgpu::PrimitiveTopology::TriangleList,cull_mode:None,..Default::default()},
                    depth_stencil:if is_ortho{None}else{Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:wgpu::StencilState::default(),bias:wgpu::DepthBiasState{constant:-2,slope_scale:-4.0,clamp:0.0}})},
                    multisample:wgpu::MultisampleState::default(),multiview:None,
                });

                let vb=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("VB"),contents:bytemuck::cast_slice(&all_verts),usage:wgpu::BufferUsages::VERTEX});
                let ib=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("IB"),contents:bytemuck::cast_slice(&all_idx),usage:wgpu::BufferUsages::INDEX});

                // ── Render ─────────────────────────────────────────────
                let Ok(frame)=surface.get_current_texture()else{return};
                let view=frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut enc=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("Frame")});
                {
                    let mut depth_view_opt=None;
                    if !is_ortho{
                        let depth_tex=device.create_texture(&wgpu::TextureDescriptor{
                            label:Some("Depth"),size:wgpu::Extent3d{width:surf_cfg.width,height:surf_cfg.height,depth_or_array_layers:1},
                            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,
                            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
                        });
                        depth_view_opt=Some(depth_tex.create_view(&wgpu::TextureViewDescriptor::default()));
                    }
                    let depth_attachment=depth_view_opt.as_ref().map(|dv|wgpu::RenderPassDepthStencilAttachment{
                        view:dv,depth_ops:Some(wgpu::Operations{load:wgpu::LoadOp::Clear(1.0),store:wgpu::StoreOp::Store}),stencil_ops:None,
                    });

                    let mut p=enc.begin_render_pass(&wgpu::RenderPassDescriptor{
                        label:Some("UI"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{
                            view:&view,resolve_target:None,
                            ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.04,g:0.06,b:0.09,a:1.0}),store:wgpu::StoreOp::Store},
                        })],
                        depth_stencil_attachment:depth_attachment,
                        timestamp_writes:None,occlusion_query_set:None,
                    });
                    p.set_pipeline(&pipe);p.set_bind_group(0,&bg,&[]);
                    p.set_vertex_buffer(0,vb.slice(..));p.set_index_buffer(ib.slice(..),wgpu::IndexFormat::Uint16);
                    p.draw_indexed(0..all_idx.len() as u32,0,0..1);
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
