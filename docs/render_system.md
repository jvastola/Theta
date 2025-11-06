# Render System Architecture

## Overview

The Theta Engine render system provides flexible rendering backends supporting both XR (VR/AR) devices and desktop windows for development and testing. The architecture prioritizes XR-first design while maintaining a practical development workflow on desktop platforms.

## Render Backends

### Backend Types

```rust
pub enum BackendKind {
    Null,    // Headless rendering for testing
    Wgpu,    // GPU-accelerated rendering via wgpu
}
```

### Null Backend
- **Purpose**: Headless testing and CI environments
- **Behavior**: Simulates frame rendering without GPU interaction
- **Use Case**: Unit tests, performance profiling, automated validation

### WGPU Backend (XR Mode)
- **Purpose**: Stereo rendering for VR headsets
- **Features**:
  - Per-eye swapchain management (3 images per eye)
  - Automatic fence-based synchronization
  - OpenXR compositor integration
  - Foveated rendering support (planned)
- **Target**: Quest 3, SteamVR, Apple Vision Pro

### Window Backend (Desktop Mode) ✨ NEW
- **Purpose**: Development and testing on macOS/Linux/Windows
- **Features**:
  - Native windowing via winit
  - Hot-reload friendly event loop
  - Automatic surface resize handling
  - Platform-optimized presentation (Metal on macOS, Vulkan on Linux, DX12 on Windows)
- **Activation**: `--features render-wgpu`
- **Target**: Developer machines without XR hardware

## Desktop Window Rendering

### Quick Start

```bash
# Run window test example
cargo run --example window_test --features render-wgpu

# Run with debug logging
RUST_LOG=info cargo run --example window_test --features render-wgpu
```

### API Usage

```rust
use theta_engine::render::{WindowApp, WindowConfig, WindowEventLoop};

let config = WindowConfig {
    title: "My Theta App".to_string(),
    width: 1280,
    height: 720,
    resizable: true,
    color_space: ColorSpace::Srgb,
};

let event_loop = WindowEventLoop::new()?;
event_loop.run(move |event_loop| {
    WindowApp::new(event_loop, config.clone())
        .map(|app| Box::new(app) as Box<dyn WindowAppTrait>)
})?;
```

### Configuration

- **Title**: Window title bar text
- **Size**: Initial window dimensions (width × height in pixels)
- **Resizable**: Allow user to resize window
- **Color Space**: `Srgb` (standard) or `DisplayP3` (wide gamut on macOS)

### Supported Platforms

| Platform | GPU API | Status | Notes |
|----------|---------|--------|-------|
| macOS | Metal | ✅ Tested | Native Metal backend via wgpu |
| Linux | Vulkan | ⚠️ Untested | Expected to work |
| Windows | DirectX 12 | ⚠️ Untested | Expected to work |

## Architecture Details

### Render Flow

```
┌─────────────────┐
│  Engine::run()  │
│   (main loop)   │
└────────┬────────┘
         │
         ▼
┌─────────────────────────────┐
│   Renderer::render(Δt)     │
│  - Poll VR/window events    │
│  - Acquire frame inputs     │
└────────┬────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│   GpuBackend::render_frame  │
│  - Create command encoder   │
│  - Execute render passes    │
│  - Submit to GPU queue      │
└────────┬────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│   VrBridge::present()       │  (XR mode)
│  or frame.present()         │  (Window mode)
└─────────────────────────────┘
```

### Window Backend Implementation

**Key Components**:
- `WindowBackend`: Manages wgpu device, surface configuration, and frame rendering
- `WindowEventLoop`: Wraps winit event loop with Theta-specific initialization
- `WindowApp`: Application state managing window, backend, and frame timing
- `WindowAppTrait`: Interface for custom window applications

**Frame Rendering**:
1. Acquire swapchain texture from surface
2. Create texture view for rendering target
3. Begin render pass with clear operation
4. Submit command buffer to GPU queue
5. Present frame to window surface
6. Request next redraw

### Future Enhancements

#### Test Geometry Renderer (Phase 6)
- Render simple triangle/cube for visual validation
- Vertex/index buffer management
- Basic shader pipeline (position + color)
- Camera controls for scene navigation

#### Runtime Mode Detection (Phase 6)
- Detect XR runtime availability at startup
- Automatic fallback: XR → Window → Null
- Environment variable override: `THETA_RENDER_MODE=window`
- Graceful degradation logging

#### Scene Rendering (Phase 7+)
- glTF mesh loading and rendering
- Material system (PBR shaders)
- Lighting (directional, point, spot)
- Shadow mapping
- Post-processing pipeline

## Performance Characteristics

### Window Mode (macOS M1, 1440p)
- **Frame Time**: ~0.5ms (clear pass only)
- **GPU Usage**: <1% (minimal workload)
- **Presentation**: VSync-locked at 60 Hz (Metal default)

### Memory Footprint
- **Device**: ~12 MB (wgpu device + adapter)
- **Swapchain**: ~16 MB per surface (1440p × 4 bytes × 3 images)
- **Per-Frame**: <1 MB (command buffers, temp allocations)

### Scalability Notes
- Window mode shares device/queue with XR swapchains when both active
- Switching modes doesn't require re-initializing wgpu device
- Render passes can be reused across backends (future optimization)

## Debugging Tips

### Enable wgpu Validation
```bash
export WGPU_VALIDATION=1
cargo run --example window_test --features render-wgpu
```

### Profile Frame Timing
```rust
let start = std::time::Instant::now();
backend.render_frame(&inputs, &views)?;
println!("Frame time: {:?}", start.elapsed());
```

### Check Surface Configuration
```bash
RUST_LOG=debug cargo run --example window_test --features render-wgpu
# Look for: [render] window surface configured (WxH, format: ...)
```

## Related Documentation
- [Architecture Overview](architecture.md) - Overall engine design
- [VR Integration](architecture.md#vr-integration) - XR backend details
- [Phase 5 Plan](phase5_parallel_plan.md) - Current sprint priorities
- [Rendering Roadmap](roadmap_november_2025.md) - Future milestones

## Testing

### Unit Tests
```bash
cargo test --features render-wgpu render::window
```

### Integration Example
```bash
# Basic window rendering
cargo run --example window_test --features render-wgpu

# With logging
RUST_LOG=theta_engine=debug cargo run --example window_test --features render-wgpu
```

### CI Status
- ✅ macOS: Window initialization and frame rendering verified
- ⚠️ Linux: Pending CI runner with X11/Wayland
- ⚠️ Windows: Pending CI runner with DirectX 12

## Migration Guide

### From Null to Window Backend

**Before** (headless only):
```rust
let backend = Box::new(NullGpuBackend::default());
let mut renderer = Renderer::new(config, backend, vr_bridge);
```

**After** (window support):
```rust
#[cfg(feature = "render-wgpu")]
let backend = Box::new(WindowBackend::initialize(window_config)?);

#[cfg(not(feature = "render-wgpu"))]
let backend = Box::new(NullGpuBackend::default());

let mut renderer = Renderer::new(config, backend, vr_bridge);
```

### Adding Custom Window App

Implement `WindowAppTrait`:
```rust
struct MyApp {
    window: Arc<Window>,
    backend: WindowBackend,
    // your app state
}

impl WindowAppTrait for MyApp {
    fn handle_window_event(...) { /* input handling */ }
    fn render_frame(&mut self) -> RenderResult<()> {
        // custom rendering logic
        self.backend.render_frame(&inputs, &views)?;
        self.window.request_redraw();
        Ok(())
    }
}
```

## Feature Flags

| Flag | Enables | Dependencies |
|------|---------|--------------|
| `render-wgpu` | GPU rendering, window backend | `wgpu`, `pollster`, `winit`, `raw-window-handle` |
| `vr-openxr` | OpenXR runtime, stereo rendering | `openxr` |
| `target-pcvr` | High-fidelity PC VR mode | Implies `render-wgpu` |

## Troubleshooting

### "failed to find a compatible GPU adapter"
- Ensure your system supports Vulkan 1.1+ (Linux), Metal (macOS), or DirectX 12 (Windows)
- Update graphics drivers
- Check wgpu compatibility: `WGPU_BACKEND=vulkan` (Linux) or `WGPU_BACKEND=dx12` (Windows)

### Window doesn't appear
- Verify `render-wgpu` feature is enabled
- Check event loop initialization logs
- Try headless mode if display server unavailable

### Poor performance
- Disable VSync: Set `color_space: ColorSpace::Srgb` and check present mode logs
- Profile with `RUST_LOG=debug` to identify bottlenecks
- Reduce window resolution for baseline measurement
