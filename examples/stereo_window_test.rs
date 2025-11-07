/// Test stereoscopic window rendering with different viewport layouts
use theta_engine::render::{StereoMode, WindowApp, WindowConfig, WindowEventLoop};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Parse stereo mode from command line
    let args: Vec<String> = env::args().collect();
    let stereo_mode = if args.len() > 1 {
        match args[1].as_str() {
            "mono" => StereoMode::Mono,
            "sbs" | "side-by-side" => StereoMode::SideBySide,
            "tb" | "top-bottom" => StereoMode::TopBottom,
            _ => {
                eprintln!("Usage: {} [mono|sbs|tb]", args[0]);
                eprintln!("  mono: Single viewport (default)");
                eprintln!("  sbs:  Side-by-side stereo");
                eprintln!("  tb:   Top-bottom stereo");
                std::process::exit(1);
            }
        }
    } else {
        StereoMode::SideBySide // Default to side-by-side for XR testing
    };

    let title = match stereo_mode {
        StereoMode::Mono => "Theta Engine - Mono Window",
        StereoMode::SideBySide => "Theta Engine - Stereo (Side-by-Side)",
        StereoMode::TopBottom => "Theta Engine - Stereo (Top-Bottom)",
    };

    let config = WindowConfig {
        title: title.to_string(),
        width: if stereo_mode == StereoMode::SideBySide { 1600 } else { 800 },
        height: if stereo_mode == StereoMode::TopBottom { 1600 } else { 800 },
        resizable: true,
        color_space: theta_engine::render::ColorSpace::Srgb,
        stereo_mode,
    };

    println!("Starting window with stereo mode: {:?}", stereo_mode);
    println!("Press ESC or close window to exit");

    let event_loop = WindowEventLoop::new()?;
    event_loop.run(move |event_loop| {
        WindowApp::new(event_loop, config.clone())
            .map(|app| Box::new(app) as Box<dyn theta_engine::render::WindowAppTrait>)
    })?;

    Ok(())
}
