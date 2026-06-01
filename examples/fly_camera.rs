//! Fly-camera example – freely explore a 3D scene with WASD + mouse.
//!
//! Controls:
//!   W / S       Move forward / backward
//!   A / D       Strafe left / right
//!   Space       Move up
//!   LCtrl       Move down
//!   Mouse       Look around (click to capture, ESC to release)
//!   Shift       Move faster
//!
//! Run with:
//!   cargo run --example fly_camera --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use theta_engine::render::window::{
        WindowAppTrait, WindowConfig, WindowEventLoop,
    };
    use theta_engine::render::GpuBackend;
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
        event_loop::{EventLoopWindowTarget},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };

    // ── Camera ────────────────────────────────────────────────────────────
    #[derive(Clone, Copy)]
    struct Camera {
        pos: [f32; 3],
        yaw: f32,
        pitch: f32,
    }

    impl Camera {
        fn forward(&self) -> [f32; 3] {
            let (sy, cy) = (self.yaw.sin(), self.yaw.cos());
            let (sp, cp) = (self.pitch.sin(), self.pitch.cos());
            [cy * cp, sp, sy * cp]
        }
        fn right(&self) -> [f32; 3] {
            let (sy, cy) = (self.yaw.sin(), self.yaw.cos());
            [sy, 0.0, -cy]
        }
    }

    // ── Fly camera app ────────────────────────────────────────────────────
    struct FlyCameraApp {
        window: Arc<Window>,
        backend: theta_engine::render::window::WindowBackend,
        frame_index: u64,
        elapsed_seconds: f32,
        last_frame: std::time::Instant,
        camera: Camera,
        keys_held: std::collections::HashSet<KeyCode>,
        mouse_captured: bool,
    }

    impl FlyCameraApp {
        fn new(
            event_loop: &EventLoopWindowTarget<()>,
            config: WindowConfig,
        ) -> Result<Self, theta_engine::render::RenderError> {
            use winit::dpi::LogicalSize;
            let window = winit::window::WindowBuilder::new()
                .with_title(config.title.clone())
                .with_inner_size(LogicalSize::new(config.width, config.height))
                .with_resizable(config.resizable)
                .build(event_loop)
                .map_err(|_| {
                    theta_engine::render::RenderError::Backend("failed to create window")
                })?;

            let window = Arc::new(window);
            let mut backend =
                theta_engine::render::window::WindowBackend::initialize(config)?;
            backend.create_window_surface(Arc::clone(&window))?;

            Ok(Self {
                window,
                backend,
                frame_index: 0,
                elapsed_seconds: 0.0,
                last_frame: std::time::Instant::now(),
                camera: Camera {
                    pos: [0.0, 2.0, -5.0],
                    yaw: std::f32::consts::FRAC_PI_2,
                    pitch: -0.15,
                },
                keys_held: std::collections::HashSet::new(),
                mouse_captured: false,
            })
        }

        /// Build a view-projection matrix matching the format expected by
        /// `WindowBackend::create_view_projection_matrix` (row-major, uploaded
        /// directly for the WGSL shader).
        fn build_vp_matrix(&self, aspect: f32) -> [[f32; 4]; 4] {
            let cam = &self.camera;

            // Perspective
            let fov_y = std::f32::consts::FRAC_PI_4;
            let near = 0.1f32;
            let far = 100.0f32;
            let f = 1.0 / (fov_y * 0.5).tan();
            let nf = 1.0 / (near - far);

            let proj: [[f32; 4]; 4] = [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, (far + near) * nf, 2.0 * far * near * nf],
                [0.0, 0.0, -1.0, 0.0],
            ];

            // View (look-at)
            let fwd = cam.forward();
            let target = [
                cam.pos[0] + fwd[0],
                cam.pos[1] + fwd[1],
                cam.pos[2] + fwd[2],
            ];
            let up: [f32; 3] = [0.0, 1.0, 0.0];

            let f_dir = normalize(sub(target, cam.pos));
            let s = normalize(cross(f_dir, up));
            let u = cross(s, f_dir);

            let view: [[f32; 4]; 4] = [
                [s[0], s[1], s[2], -dot(s, cam.pos)],
                [u[0], u[1], u[2], -dot(u, cam.pos)],
                [-f_dir[0], -f_dir[1], -f_dir[2], dot(f_dir, cam.pos)],
                [0.0, 0.0, 0.0, 1.0],
            ];

            // VP = proj × view (row-major), then transpose for WGSL column-major
            transpose(mul_mat4(proj, view))
        }
    }

    impl WindowAppTrait for FlyCameraApp {
        fn handle_device_event(
            &mut self,
            _event_loop: &EventLoopWindowTarget<()>,
            event: DeviceEvent,
        ) {
            if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
                if self.mouse_captured {
                    let sensitivity = 0.002;
                    self.camera.yaw += dx as f32 * sensitivity;
                    self.camera.pitch -= dy as f32 * sensitivity;
                    self.camera.pitch = self.camera.pitch.clamp(-1.55, 1.55);
                }
            }
        }

        fn handle_window_event(
            &mut self,
            event_loop: &EventLoopWindowTarget<()>,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    self.backend.resize(size.width, size.height);
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return;
                    };
                    match key {
                        KeyCode::Escape => {
                            if self.mouse_captured {
                                self.mouse_captured = false;
                                let _ = self.window.set_cursor_grab(
                                    winit::window::CursorGrabMode::None,
                                );
                                self.window.set_cursor_visible(true);
                            } else {
                                event_loop.exit();
                            }
                        }
                        _ => match event.state {
                            ElementState::Pressed => {
                                self.keys_held.insert(key);
                            }
                            ElementState::Released => {
                                self.keys_held.remove(&key);
                            }
                        },
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if !self.mouse_captured {
                        self.mouse_captured = true;
                        let _ = self.window.set_cursor_grab(
                            winit::window::CursorGrabMode::Locked,
                        );
                        self.window.set_cursor_visible(false);
                        let size = self.window.inner_size();
                        let _ = self.window.set_cursor_position(PhysicalPosition::new(
                            size.width as f64 / 2.0,
                            size.height as f64 / 2.0,
                        ));
                    }
                }
                _ => {}
            }
        }

        fn render_frame(&mut self) -> theta_engine::render::RenderResult<()> {
            use theta_engine::render::{FrameInputs};
            use theta_engine::vr::{VrViewConfig};

            let now = std::time::Instant::now();
            let delta_seconds = now.duration_since(self.last_frame).as_secs_f32();
            self.last_frame = now;

            self.frame_index += 1;
            self.elapsed_seconds += delta_seconds;

            // ── Update camera position ─────────────────────────────────
            let speed = if self.keys_held.contains(&KeyCode::ShiftLeft)
                || self.keys_held.contains(&KeyCode::ShiftRight)
            {
                12.0
            } else {
                4.0
            };
            let vel = speed * delta_seconds;

            let fwd = self.camera.forward();
            let right = self.camera.right();

            if self.keys_held.contains(&KeyCode::KeyW) {
                self.camera.pos[0] += fwd[0] * vel;
                self.camera.pos[1] += fwd[1] * vel;
                self.camera.pos[2] += fwd[2] * vel;
            }
            if self.keys_held.contains(&KeyCode::KeyS) {
                self.camera.pos[0] -= fwd[0] * vel;
                self.camera.pos[1] -= fwd[1] * vel;
                self.camera.pos[2] -= fwd[2] * vel;
            }
            if self.keys_held.contains(&KeyCode::KeyD) {
                self.camera.pos[0] += right[0] * vel;
                self.camera.pos[1] += right[1] * vel;
                self.camera.pos[2] += right[2] * vel;
            }
            if self.keys_held.contains(&KeyCode::KeyA) {
                self.camera.pos[0] -= right[0] * vel;
                self.camera.pos[1] -= right[1] * vel;
                self.camera.pos[2] -= right[2] * vel;
            }
            if self.keys_held.contains(&KeyCode::Space) {
                self.camera.pos[1] += vel;
            }
            if self.keys_held.contains(&KeyCode::ControlLeft) {
                self.camera.pos[1] -= vel;
            }

            // ── Set custom VP matrix ───────────────────────────────────
            let size = self.window.inner_size();
            let aspect = size.width as f32 / size.height as f32;
            let vp = self.build_vp_matrix(aspect);
            self.backend.set_custom_view_projection(Some(vp));

            let inputs = FrameInputs {
                frame_index: self.frame_index,
                delta_seconds,
                elapsed_seconds: self.elapsed_seconds,
            };

            let views = VrViewConfig {
                views: vec![theta_engine::vr::VrView {
                    resolution: [size.width, size.height],
                    fov: [90.0, 90.0],
                    transform: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                }],
            };

            self.backend.render_frame(&inputs, &views)?;
            self.window.request_redraw();

            Ok(())
        }
    }

    // ── Math helpers ──────────────────────────────────────────────────────
    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn normalize(v: [f32; 3]) -> [f32; 3] {
        let l = dot(v, v).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    }
    fn mul_mat4(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
        let mut out = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    out[i][j] += a[i][k] * b[k][j];
                }
            }
        }
        out
    }
    fn transpose(m: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
        let mut out = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                out[i][j] = m[j][i];
            }
        }
        out
    }

    // ── Main ──────────────────────────────────────────────────────────────
    env_logger::init();

    let config = WindowConfig {
        title: "Theta Engine – Fly Camera".to_string(),
        width: 1280,
        height: 720,
        resizable: true,
        ..Default::default()
    };

    let event_loop = WindowEventLoop::new()?;

    println!("╔══════════════════════════════════════════════════╗");
    println!("║        Theta Engine – Fly Camera Example          ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  W/S      Move forward / backward                ║");
    println!("║  A/D      Strafe left / right                    ║");
    println!("║  Space    Move up                                ║");
    println!("║  LCtrl    Move down                              ║");
    println!("║  Shift    Hold to move faster                    ║");
    println!("║  Click    Capture mouse for look                  ║");
    println!("║  ESC      Release mouse / quit                   ║");
    println!("╚══════════════════════════════════════════════════╝");

    event_loop.run(move |event_loop| {
        FlyCameraApp::new(event_loop, config.clone())
            .map(|app| Box::new(app) as Box<dyn WindowAppTrait>)
    })?;

    Ok(())
}

#[cfg(not(feature = "render-wgpu"))]
fn main() {
    eprintln!("This example requires the 'render-wgpu' feature.");
    eprintln!("Run with: cargo run --example fly_camera --features render-wgpu");
    std::process::exit(1);
}
