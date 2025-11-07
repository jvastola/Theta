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

pub enum RenderMode {
    Xr,        // XR/VR rendering mode with stereo views
    Window,    // Desktop window rendering mode (fallback)
    Headless,  // Headless mode for testing
}
```

### Runtime Mode Detection

The engine automatically detects the best available rendering mode at startup:

1. **XR Mode** (Priority 1): If OpenXR runtime is available (requires `vr-openxr` feature)
2. **Window Mode** (Priority 2): If wgpu is available (requires `render-wgpu` feature)
3. **Headless Mode** (Priority 3): Fallback when no GPU rendering is available

```rust
// Automatic detection (recommended)
let engine = Engine::new();  // Detects XR → Window → Headless

// Manual override
let mut config = RendererConfig::default();
config.backend = BackendKind::Wgpu;
config.mode = RenderMode::Window;
let engine = Engine::with_renderer_config(config);
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
  - **Stereoscopic rendering** with multiple viewport layouts
  - Hot-reload friendly event loop
  - Automatic surface resize handling
  - Test geometry rendering (triangle with vertex colors)
  - Platform-optimized presentation (Metal on macOS, Vulkan on Linux, DX12 on Windows)
- **Stereo Modes**:
  - **Mono**: Single viewport (traditional rendering)
  - **Side-by-Side**: Left and right eye viewports arranged horizontally
  - **Top-Bottom**: Left and right eye viewports arranged vertically
- **Activation**: `--features render-wgpu`
- **Target**: Developer machines without XR hardware

## Desktop Window Rendering

### Quick Start

```bash
# Run stereo window test (side-by-side mode)
cargo run --example stereo_window_test --features render-wgpu -- sbs

# Run mono window test
cargo run --example stereo_window_test --features render-wgpu -- mono

# Run top-bottom stereo
cargo run --example stereo_window_test --features render-wgpu -- tb

# Run with debug logging
RUST_LOG=info cargo run --example stereo_window_test --features render-wgpu -- sbs
```

### Stereo Rendering Modes

The window backend supports three viewport layout modes for XR development without hardware:

#### Mono Mode
- **Layout**: Single viewport filling entire window
- **Use Case**: Traditional 2D/3D rendering, non-XR applications
- **Window Size**: 800×800 (default)

#### Side-by-Side Mode (Recommended for XR Testing)
- **Layout**: Two viewports arranged horizontally (left | right)
- **Use Case**: XR preview, dual-screen debugging
- **Window Size**: 1600×800 (default, 800px per eye)
- **Visual Distinction**: Right eye has slightly brighter background

#### Top-Bottom Mode
- **Layout**: Two viewports stacked vertically (top = left, bottom = right)
- **Use Case**: Portrait-oriented displays, VR180 video testing
- **Window Size**: 800×1600 (default, 800px per eye)
- **Visual Distinction**: Right eye (bottom) has slightly brighter background

### API Usage

```rust
use theta_engine::render::{StereoMode, WindowApp, WindowConfig, WindowEventLoop};

let config = WindowConfig {
    title: "Theta Engine - Stereo Test".to_string(),
    width: 1600,
    height: 800,
    resizable: true,
    color_space: ColorSpace::Srgb,
    stereo_mode: StereoMode::SideBySide,
};

let event_loop = WindowEventLoop::new()?;
event_loop.run(move |event_loop| {
    WindowApp::new(event_loop, config.clone())
        .map(|app| Box::new(app) as Box<dyn WindowAppTrait>)
})?;
```

### Configuration Options

**WindowConfig Fields:**
- **title**: Window title bar text
- **width**: Initial window width in pixels
- **height**: Initial window height in pixels
- **resizable**: Allow user to resize window
- **color_space**: `Srgb` (standard) or `DisplayP3` (wide gamut on macOS)
- **stereo_mode**: `Mono`, `SideBySide`, or `TopBottom`

**Recommended Sizes:**
- Mono: 800×800 or 1280×720
- Side-by-Side: 1600×800 (800px per eye)
- Top-Bottom: 800×1600 (800px per eye)

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
- `WindowBackend`: Manages wgpu device, surface configuration, geometry pipeline, and stereo rendering
- `WindowEventLoop`: Wraps winit event loop with Theta-specific initialization
- `WindowApp`: Application state managing window, backend, and frame timing
- `WindowAppTrait`: Interface for custom window applications
- `GeometryPipeline`: Shader pipeline with vertex/index buffers for test rendering

**Geometry Rendering**:
- **Vertices**: Colored triangle (red, green, blue corners)
- **Shader**: WGSL vertex + fragment shader with position and color attributes
- **Format**: Vertex buffer contains `[position: vec3<f32>, color: vec3<f32>]`
- **Pipeline**: Triangle list topology with back-face culling

**Stereo Viewport Rendering**:
1. Acquire swapchain texture from surface
2. Create texture view for rendering target
3. For each viewport (1 for Mono, 2 for Stereo):
   - Set viewport region (x, y, width, height)
   - Begin render pass with clear operation
   - Render test geometry (triangle)
4. Submit command buffer to GPU queue
5. Present frame to window surface
6. Request next redraw

### Future Enhancements

#### Animated Geometry (Phase 6)
- Rotate/scale triangle based on elapsed time
- Add more complex geometry (cube, sphere)
- Camera controls for scene navigation

#### Runtime Mode Detection (Phase 6) ✅ COMPLETE
- ✅ Detect XR runtime availability at startup
- ✅ Automatic fallback: XR → Window → Null
- Environment variable override: `THETA_RENDER_MODE=window`
- Graceful degradation logging

#### Scene Rendering (Phase 7+)
- glTF mesh loading and rendering
- Material system (PBR shaders)
- Lighting (directional, point, spot)
- Shadow mapping
- Post-processing pipeline

## Performance Characteristics

### Window Mode (macOS M1, 1600×800 Stereo)
- **Frame Time**: ~1.2ms (stereo triangle rendering)
- **GPU Usage**: <2% (minimal workload with geometry)
- **Presentation**: VSync-locked at 60 Hz (Metal default)
- **Viewport Overhead**: ~0.2ms per additional viewport

### Memory Footprint
- **Device**: ~12 MB (wgpu device + adapter)
- **Swapchain**: ~20 MB per surface (1600×800 × 4 bytes × 3 images)
- **Geometry Pipeline**: ~4 KB (vertex buffer + index buffer + shader modules)
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
# Test stereo rendering modes
cargo run --example stereo_window_test --features render-wgpu -- mono
cargo run --example stereo_window_test --features render-wgpu -- sbs
cargo run --example stereo_window_test --features render-wgpu -- tb

# With logging
RUST_LOG=theta_engine=debug cargo run --example stereo_window_test --features render-wgpu -- sbs
```

### CI Status
- ✅ macOS: Window initialization, stereo rendering, and geometry pipeline verified
- ⚠️ Linux: Pending CI runner with X11/Wayland
- ⚠️ Windows: Pending CI runner with DirectX 12

## Engine Integration

### Automatic Mode Detection

The engine automatically selects the best rendering mode:

```rust
// Automatic detection (recommended)
let engine = Engine::new();
```

**Detection Priority:**
1. **XR Mode**: If `vr-openxr` feature enabled and OpenXR runtime available
2. **Window Mode**: If `render-wgpu` feature enabled (automatic stereo fallback)
3. **Headless Mode**: If no GPU features enabled

**Note**: Window mode requires an event loop to display the window. Use the `stereo_window_test` example for interactive window rendering, or integrate the `WindowEventLoop` into your application's main loop.

**Manual Override:**
```rust
let mut config = RendererConfig::default();
config.backend = BackendKind::Wgpu;
config.mode = RenderMode::Window;  // Force window mode
let engine = Engine::with_renderer_config(config);
```

### Example: Standalone Window Rendering

For interactive window rendering, use the `WindowEventLoop` directly:

```rust
use theta_engine::render::{StereoMode, WindowApp, WindowConfig, WindowEventLoop};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    let config = WindowConfig {
        title: "Theta Engine".to_string(),
        width: 1600,
        height: 800,
        resizable: true,
        color_space: theta_engine::render::ColorSpace::Srgb,
        stereo_mode: StereoMode::SideBySide,
    };

    let event_loop = WindowEventLoop::new()?;
    event_loop.run(move |event_loop| {
        WindowApp::new(event_loop, config.clone())
            .map(|app| Box::new(app) as Box<dyn theta_engine::render::WindowAppTrait>)
    })?;

    Ok(())
}
```

**Expected Logs (without XR hardware):**
```
[INFO render] window backend initialized (adapter: "Apple M1")
[INFO render] window surface configured (1600x800, format: Bgra8UnormSrgb)
[INFO render] window application initialized
```

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
