//! Desktop window rendering test
//!
//! This example demonstrates the window-based render backend for development/testing on macOS.
//! Run with: cargo run --example window_test --features render-wgpu

#[cfg(feature = "render-wgpu")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    use theta_engine::render::{WindowApp, WindowConfig, WindowEventLoop};

    let config = WindowConfig {
        title: "Theta Engine - Window Test".to_string(),
        width: 1280,
        height: 720,
        resizable: true,
        ..Default::default()
    };

    let event_loop = WindowEventLoop::new()?;

    println!("Starting window render test...");
    println!("Press ESC or close window to exit");

    event_loop.run(move |event_loop| {
        WindowApp::new(event_loop, config.clone())
            .map(|app| Box::new(app) as Box<dyn theta_engine::render::window::WindowAppTrait>)
            .map_err(|e| e.into())
    })?;

    Ok(())
}

#[cfg(not(feature = "render-wgpu"))]
fn main() {
    eprintln!("This example requires the 'render-wgpu' feature.");
    eprintln!("Run with: cargo run --example window_test --features render-wgpu");
    std::process::exit(1);
}
