# PBR Rendering System Roadmap for VR

## Executive Summary

This document outlines the complete implementation path for a **Physically-Based Rendering (PBR)** system in Theta Engine, designed for high-quality VR scene rendering. The system will support complex scenes with realistic materials, lighting, and post-processing effects, similar to Bevy's rendering capabilities but optimized for VR/XR workloads.

**Target**: Production-quality VR rendering with full PBR material system, real-time lighting, and VR-specific optimizations.

---

## Phase 1: Foundation - Camera & Material System

### 1.1 Camera System
**Goal**: Flexible camera abstraction supporting both desktop and VR stereo rendering with player scale support.

**Components**:
```rust
pub struct Camera {
    pub position: Vec3,
    pub rotation: Quat,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub aspect_ratio: f32,
    pub world_scale: f32,  // Player scale multiplier (1.0 = default)
}

pub struct StereoCamera {
    pub left_eye: Camera,
    pub right_eye: Camera,
    pub ipd: f32,  // Inter-pupillary distance (64mm default)
    pub convergence: f32,
    pub player_scale: f32,  // Global player scale (0.1 - 10.0 typical range)
}

impl Camera {
    fn view_matrix(&self) -> Mat4;
    fn projection_matrix(&self) -> Mat4;
    fn view_projection(&self) -> Mat4;
    fn view_matrix_with_scale(&self) -> Mat4;  // Applies world_scale to position
}

impl StereoCamera {
    fn update_scale(&mut self, scale: f32);
    fn scaled_ipd(&self) -> f32;  // IPD adjusted by player_scale
}
```

**Player Scaling System**:
```rust
pub struct PlayerScale {
    pub current_scale: f32,
    pub min_scale: f32,  // 0.1 (10x smaller)
    pub max_scale: f32,  // 10.0 (10x larger)
    pub transition_speed: f32,  // Smooth scaling
}

impl PlayerScale {
    pub fn set_scale(&mut self, scale: f32) -> f32;
    pub fn adjust_scale(&mut self, delta: f32) -> f32;
    pub fn smooth_transition(&mut self, target: f32, dt: f32);
    
    // Apply scale to all rendering transforms
    pub fn apply_to_camera(&self, camera: &mut Camera);
    pub fn apply_to_mesh(&self, transform: &mut Mat4);
}
```

**Scale Presets for VR Comfort**:
```rust
pub enum ScalePreset {
    Miniature,      // 0.1x - See world as giant
    Small,          // 0.5x - Slightly enlarged world
    Normal,         // 1.0x - Real-world scale
    Large,          // 2.0x - Slightly shrunk world
    Giant,          // 5.0x - See world as miniature
    Custom(f32),
}
```

**Implementation Tasks**:
- [ ] Camera component with transform system
- [ ] Perspective and orthographic projection
- [ ] Stereo camera with configurable IPD
- [ ] **Player scale system with smooth transitions**
- [ ] **Scale-aware view matrix calculation**
- [ ] **IPD scaling based on player scale**
- [ ] Camera controller for desktop testing (orbit, first-person)
- [ ] VR head tracking integration
- [ ] **UI controls for runtime scale adjustment**

**Files to Create**:
- `src/render/camera.rs`
- `src/render/camera_controller.rs`
- `src/render/player_scale.rs`

**Estimated Effort**: 4-6 days

---

### 1.2 Material System (PBR Foundation)
**Goal**: Standard PBR material model supporting metallic-roughness workflow.

**Material Properties**:
```rust
pub struct PbrMaterial {
    // Base material properties
    pub base_color: Color,
    pub base_color_texture: Option<TextureHandle>,
    
    // Metallic-roughness workflow
    pub metallic: f32,
    pub roughness: f32,
    pub metallic_roughness_texture: Option<TextureHandle>,
    
    // Normal mapping
    pub normal_map: Option<TextureHandle>,
    pub normal_scale: f32,
    
    // Occlusion
    pub occlusion_texture: Option<TextureHandle>,
    pub occlusion_strength: f32,
    
    // Emissive
    pub emissive: Color,
    pub emissive_texture: Option<TextureHandle>,
    pub emissive_strength: f32,
    
    // Alpha
    pub alpha_mode: AlphaMode,  // Opaque, Mask, Blend
    pub alpha_cutoff: f32,
    
    // Advanced features
    pub double_sided: bool,
    pub unlit: bool,
}

pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}
```

**Shader Structure** (WGSL):
```wgsl
struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    metallic_roughness: vec2<f32>,  // x: metallic, y: roughness
    normal_scale: f32,
    occlusion_strength: f32,
    alpha_cutoff: f32,
    flags: u32,  // Bitfield: has_base_texture, has_normal_map, etc.
}

@group(1) @binding(0)
var<uniform> material: MaterialUniforms;

@group(1) @binding(1)
var base_color_sampler: sampler;
@group(1) @binding(2)
var base_color_texture: texture_2d<f32>;
// ... more texture bindings
```

**Implementation Tasks**:
- [ ] Material struct and builder API
- [ ] Material uniform buffer layout
- [ ] Texture loading and management
- [ ] Material bind group creation
- [ ] Default material (white, medium roughness)

**Files to Create**:
- `src/render/material.rs`
- `src/render/texture.rs`
- `src/render/shaders/pbr_material.wgsl`

**Estimated Effort**: 5-7 days

---

## Phase 2: Lighting System

### 2.1 Light Types
**Goal**: Support standard light types with shadow mapping.

**Light Definitions**:
```rust
pub enum Light {
    Directional(DirectionalLight),
    Point(PointLight),
    Spot(SpotLight),
}

pub struct DirectionalLight {
    pub direction: Vec3,
    pub color: Color,
    pub intensity: f32,
    pub shadows_enabled: bool,
    pub shadow_map_size: u32,  // 2048, 4096, etc.
}

pub struct PointLight {
    pub position: Vec3,
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub shadows_enabled: bool,
}

pub struct SpotLight {
    pub position: Vec3,
    pub direction: Vec3,
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub inner_angle: f32,
    pub outer_angle: f32,
    pub shadows_enabled: bool,
}
```

**Lighting Shader** (PBR BRDF):
```wgsl
// Cook-Torrance BRDF
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;
    
    let denom = (n_dot_h2 * (a2 - 1.0) + 1.0);
    return a2 / (PI * denom * denom);
}

fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = (roughness + 1.0);
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_l = max(dot(n, l), 0.0);
    let ggx1 = geometry_schlick_ggx(n_dot_v, roughness);
    let ggx2 = geometry_schlick_ggx(n_dot_l, roughness);
    return ggx1 * ggx2;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
}

fn calculate_pbr_lighting(
    material: MaterialUniforms,
    world_pos: vec3<f32>,
    normal: vec3<f32>,
    view_dir: vec3<f32>,
    light_dir: vec3<f32>,
    light_color: vec3<f32>,
    light_intensity: f32,
) -> vec3<f32> {
    let h = normalize(view_dir + light_dir);
    
    // Calculate reflectance at normal incidence
    let f0 = mix(vec3<f32>(0.04), material.base_color.rgb, material.metallic_roughness.x);
    
    // Cook-Torrance BRDF
    let ndf = distribution_ggx(normal, h, material.metallic_roughness.y);
    let g = geometry_smith(normal, view_dir, light_dir, material.metallic_roughness.y);
    let f = fresnel_schlick(max(dot(h, view_dir), 0.0), f0);
    
    // Specular and diffuse components
    let ks = f;
    let kd = (vec3<f32>(1.0) - ks) * (1.0 - material.metallic_roughness.x);
    
    let n_dot_l = max(dot(normal, light_dir), 0.0);
    let denominator = 4.0 * max(dot(normal, view_dir), 0.0) * n_dot_l + 0.0001;
    let specular = (ndf * g * f) / denominator;
    
    let diffuse = kd * material.base_color.rgb / PI;
    
    return (diffuse + specular) * light_color * light_intensity * n_dot_l;
}
```

**Implementation Tasks**:
- [ ] Light component system
- [ ] Light uniform buffers (array of lights)
- [ ] PBR BRDF shader implementation
- [ ] Multi-light accumulation
- [ ] Light culling for performance

**Files to Create**:
- `src/render/light.rs`
- `src/render/shaders/pbr_lighting.wgsl`

**Estimated Effort**: 7-10 days

---

### 2.2 Shadow Mapping
**Goal**: Real-time shadows for all light types.

**Shadow Map System**:
```rust
pub struct ShadowMap {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub size: u32,
}

pub struct ShadowAtlas {
    pub atlas_texture: wgpu::Texture,
    pub atlas_size: u32,
    pub allocations: HashMap<LightId, ShadowAllocation>,
}

pub struct ShadowAllocation {
    pub offset: (u32, u32),
    pub size: u32,
}
```

**Shadow Shader**:
```wgsl
@group(2) @binding(0)
var shadow_sampler: sampler_comparison;
@group(2) @binding(1)
var shadow_map: texture_depth_2d_array;

fn calculate_shadow(
    light_space_pos: vec4<f32>,
    layer: i32,
    bias: f32,
) -> f32 {
    let proj_coords = light_space_pos.xyz / light_space_pos.w;
    let depth = proj_coords.z - bias;
    
    // PCF (Percentage-Closer Filtering) for soft shadows
    var shadow = 0.0;
    let texel_size = 1.0 / f32(textureNumLevels(shadow_map));
    
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel_size;
            let sample_coords = proj_coords.xy + offset;
            shadow += textureSampleCompare(
                shadow_map,
                shadow_sampler,
                sample_coords,
                layer,
                depth
            );
        }
    }
    
    return shadow / 9.0;
}
```

**Implementation Tasks**:
- [ ] Shadow map texture creation
- [ ] Shadow pass rendering
- [ ] Light-space matrix calculation
- [ ] Cascade shadow maps for directional lights
- [ ] Cube map shadows for point lights
- [ ] PCF and VSM (Variance Shadow Maps)

**Files to Create**:
- `src/render/shadow.rs`
- `src/render/shaders/shadow_pass.wgsl`

**Estimated Effort**: 7-10 days

---

## Phase 3: Advanced Texturing

### 3.1 Texture System
**Goal**: Efficient texture loading, compression, and management.

**Texture Types**:
```rust
pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub format: wgpu::TextureFormat,
    pub size: (u32, u32),
    pub mip_levels: u32,
}

pub enum TextureFormat {
    Rgba8,
    Rgba16Float,
    Bc1RgbUnorm,  // DXT1
    Bc3RgbaUnorm, // DXT5
    Bc7RgbaUnorm, // High quality
    Etc2Rgb8,     // Mobile
    Astc4x4,      // Mobile, VR
}
```

**Texture Loading Pipeline**:
```rust
pub struct TextureLoader {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    compression_enabled: bool,
}

impl TextureLoader {
    pub async fn load(&self, path: &Path) -> Result<Texture>;
    pub async fn load_ktx2(&self, data: &[u8]) -> Result<Texture>;
    pub fn generate_mipmaps(&self, texture: &wgpu::Texture);
    pub fn compress_texture(&self, data: &[u8], format: TextureFormat) -> Vec<u8>;
}
```

**Implementation Tasks**:
- [ ] Image loading (PNG, JPEG, KTX2, DDS)
- [ ] Automatic mipmap generation
- [ ] Texture compression (BC7, ASTC for VR)
- [ ] Texture streaming for large scenes
- [ ] Texture atlas generation

**Files to Create**:
- `src/render/texture_loader.rs`
- `src/render/texture_compression.rs`

**Dependencies**: `image`, `ktx2`, `basis-universal`

**Estimated Effort**: 5-7 days

---

### 3.2 Normal & Parallax Mapping
**Goal**: Enhanced surface detail without geometry.

**Shader Implementation**:
```wgsl
fn calculate_normal_from_map(
    normal_map: texture_2d<f32>,
    normal_sampler: sampler,
    uv: vec2<f32>,
    world_normal: vec3<f32>,
    world_tangent: vec3<f32>,
) -> vec3<f32> {
    let tangent_normal = textureSample(normal_map, normal_sampler, uv).xyz * 2.0 - 1.0;
    
    let n = normalize(world_normal);
    let t = normalize(world_tangent);
    let b = cross(n, t);
    let tbn = mat3x3<f32>(t, b, n);
    
    return normalize(tbn * tangent_normal);
}

fn parallax_occlusion_mapping(
    height_map: texture_2d<f32>,
    height_sampler: sampler,
    uv: vec2<f32>,
    view_dir: vec3<f32>,
    scale: f32,
) -> vec2<f32> {
    let num_layers = 32.0;
    let layer_depth = 1.0 / num_layers;
    var current_layer_depth = 0.0;
    
    let p = view_dir.xy * scale;
    let delta_uv = p / num_layers;
    
    var current_uv = uv;
    var current_depth_map_value = textureSample(height_map, height_sampler, current_uv).r;
    
    for (var i = 0; i < i32(num_layers); i++) {
        if (current_layer_depth >= current_depth_map_value) {
            break;
        }
        current_uv -= delta_uv;
        current_depth_map_value = textureSample(height_map, height_sampler, current_uv).r;
        current_layer_depth += layer_depth;
    }
    
    return current_uv;
}
```

**Implementation Tasks**:
- [ ] Tangent space calculation
- [ ] Normal map sampling
- [ ] Parallax occlusion mapping
- [ ] Height map support

**Estimated Effort**: 3-4 days

---

## Phase 4: Environment & IBL

### 4.1 Image-Based Lighting (IBL)
**Goal**: Realistic ambient lighting from environment maps.

**Environment Map System**:
```rust
pub struct EnvironmentMap {
    pub skybox: Texture,  // Cube map
    pub irradiance_map: Texture,  // Pre-filtered for diffuse
    pub prefiltered_map: Texture,  // Pre-filtered for specular
    pub brdf_lut: Texture,  // 2D lookup table
}

impl EnvironmentMap {
    pub fn from_equirectangular(data: &[u8]) -> Self;
    pub fn generate_irradiance_map(&self, device: &wgpu::Device) -> Texture;
    pub fn generate_prefiltered_map(&self, device: &wgpu::Device) -> Texture;
    pub fn generate_brdf_lut(device: &wgpu::Device) -> Texture;
}
```

**IBL Shader**:
```wgsl
@group(3) @binding(0)
var irradiance_map: texture_cube<f32>;
@group(3) @binding(1)
var prefiltered_map: texture_cube<f32>;
@group(3) @binding(2)
var brdf_lut: texture_2d<f32>;
@group(3) @binding(3)
var env_sampler: sampler;

fn calculate_ibl(
    normal: vec3<f32>,
    view_dir: vec3<f32>,
    f0: vec3<f32>,
    roughness: f32,
    metallic: f32,
    base_color: vec3<f32>,
) -> vec3<f32> {
    // Diffuse IBL
    let irradiance = textureSample(irradiance_map, env_sampler, normal).rgb;
    let diffuse = irradiance * base_color;
    
    // Specular IBL
    let reflect_dir = reflect(-view_dir, normal);
    let max_mip_level = f32(textureNumLevels(prefiltered_map) - 1u);
    let prefiltered_color = textureSampleLevel(
        prefiltered_map,
        env_sampler,
        reflect_dir,
        roughness * max_mip_level
    ).rgb;
    
    let n_dot_v = max(dot(normal, view_dir), 0.0);
    let brdf = textureSample(brdf_lut, env_sampler, vec2<f32>(n_dot_v, roughness)).rg;
    let specular = prefiltered_color * (f0 * brdf.x + brdf.y);
    
    let ks = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let kd = (1.0 - ks) * (1.0 - metallic);
    
    return kd * diffuse + specular;
}
```

**Implementation Tasks**:
- [ ] Cube map rendering from equirectangular
- [ ] Irradiance map pre-computation (convolution)
- [ ] Specular pre-filtering (split-sum approximation)
- [ ] BRDF integration LUT generation
- [ ] Skybox rendering

**Files to Create**:
- `src/render/environment.rs`
- `src/render/shaders/ibl.wgsl`
- `src/render/shaders/skybox.wgsl`

**Estimated Effort**: 7-10 days

---

## Phase 5: Mesh & Scene Management

### 5.1 Mesh System
**Goal**: Efficient mesh storage and rendering.

**Mesh Structure**:
```rust
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub bounds: BoundingBox,
    pub submeshes: Vec<SubMesh>,
}

pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],  // w component is handedness
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],  // Optional second UV set
    pub color: [f32; 4],  // Optional vertex colors
}

pub struct SubMesh {
    pub index_start: u32,
    pub index_count: u32,
    pub material_id: MaterialId,
}
```

**LOD (Level of Detail)**:
```rust
pub struct MeshLod {
    pub levels: Vec<Mesh>,
    pub distances: Vec<f32>,
}

impl MeshLod {
    pub fn select_lod(&self, camera_distance: f32) -> &Mesh;
    pub fn generate_lods(&self, target_reductions: &[f32]) -> Self;
}
```

**Implementation Tasks**:
- [ ] Vertex and index buffer management
- [ ] Mesh builder API
- [ ] Primitive generators (cube, sphere, plane, etc.)
- [ ] Mesh optimization (vertex cache, overdraw)
- [ ] LOD generation (quadric edge collapse)
- [ ] GPU instancing support

**Files to Create**:
- `src/render/mesh.rs`
- `src/render/mesh_builder.rs`
- `src/render/mesh_optimizer.rs`

**Estimated Effort**: 5-7 days

---

### 5.2 glTF Loading
**Goal**: Standard 3D asset format support.

**glTF Loader**:
```rust
pub struct GltfLoader {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture_loader: TextureLoader,
}

impl GltfLoader {
    pub async fn load(&self, path: &Path) -> Result<Scene>;
    pub async fn load_from_bytes(&self, data: &[u8]) -> Result<Scene>;
    
    fn load_meshes(&self, gltf: &gltf::Document) -> Vec<Mesh>;
    fn load_materials(&self, gltf: &gltf::Document) -> Vec<PbrMaterial>;
    fn load_animations(&self, gltf: &gltf::Document) -> Vec<Animation>;
    fn load_skins(&self, gltf: &gltf::Document) -> Vec<Skin>;
}
```

**Implementation Tasks**:
- [ ] glTF 2.0 parsing
- [ ] Binary glTF (GLB) support
- [ ] Material conversion to PBR
- [ ] Mesh hierarchy loading
- [ ] Animation data extraction
- [ ] Skinning/skeletal animation support

**Dependencies**: `gltf`, `gltf-json`

**Files to Create**:
- `src/render/gltf_loader.rs`

**Estimated Effort**: 7-10 days

---

## Phase 6: Post-Processing

### 6.1 Post-Processing Pipeline
**Goal**: Screen-space effects for enhanced visuals.

**Post-Process Effects**:
```rust
pub struct PostProcessStack {
    pub effects: Vec<Box<dyn PostProcessEffect>>,
    pub render_targets: Vec<RenderTarget>,
}

pub trait PostProcessEffect {
    fn name(&self) -> &str;
    fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    );
}

// Tone Mapping
pub struct ToneMapping {
    pub operator: TonemapOperator,
    pub exposure: f32,
}

pub enum TonemapOperator {
    Linear,
    Reinhard,
    ReinhardLuminance,
    AcesFilmic,
    Uncharted2,
}

// Bloom
pub struct Bloom {
    pub threshold: f32,
    pub intensity: f32,
    pub num_passes: u32,
}

// SSAO (Screen-Space Ambient Occlusion)
pub struct Ssao {
    pub radius: f32,
    pub bias: f32,
    pub samples: u32,
}

// Temporal Anti-Aliasing
pub struct Taa {
    pub jitter_scale: f32,
    pub feedback_min: f32,
    pub feedback_max: f32,
}

// FXAA
pub struct Fxaa {
    pub edge_threshold: f32,
    pub edge_threshold_min: f32,
}
```

**Post-Process Shaders**:
```wgsl
// Tone mapping (ACES Filmic)
fn aces_filmic(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Bloom (Gaussian blur)
fn gaussian_blur(
    input: texture_2d<f32>,
    tex_sampler: sampler,
    uv: vec2<f32>,
    direction: vec2<f32>,
) -> vec3<f32> {
    let weights = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    let texel_size = 1.0 / vec2<f32>(textureDimensions(input));
    
    var result = textureSample(input, tex_sampler, uv).rgb * weights[0];
    
    for (var i = 1; i < 5; i++) {
        let offset = f32(i) * texel_size * direction;
        result += textureSample(input, tex_sampler, uv + offset).rgb * weights[i];
        result += textureSample(input, tex_sampler, uv - offset).rgb * weights[i];
    }
    
    return result;
}
```

**Implementation Tasks**:
- [ ] Post-process effect interface
- [ ] Render target management
- [ ] Tone mapping operators
- [ ] Bloom effect (threshold + blur + combine)
- [ ] SSAO implementation
- [ ] TAA (Temporal Anti-Aliasing)
- [ ] FXAA/SMAA
- [ ] Vignette, chromatic aberration
- [ ] Color grading LUT support

**Files to Create**:
- `src/render/post_process.rs`
- `src/render/shaders/tonemap.wgsl`
- `src/render/shaders/bloom.wgsl`
- `src/render/shaders/ssao.wgsl`
- `src/render/shaders/taa.wgsl`

**Estimated Effort**: 10-14 days

---

## Phase 7: VR-Specific Optimizations

### 7.1 Foveated Rendering
**Goal**: Reduce rendering cost by lowering resolution in peripheral vision.

**Foveation System**:
```rust
pub struct FoveatedRenderer {
    pub inner_region: Rect,  // Full resolution
    pub mid_region: Rect,    // Half resolution
    pub outer_region: Rect,  // Quarter resolution
    pub eye_tracking: Option<EyeTracker>,
    pub player_scale: f32,   // Adjust foveation regions based on scale
}

pub struct EyeTracker {
    pub gaze_direction: Vec3,
    pub confidence: f32,
}

impl FoveatedRenderer {
    pub fn adjust_for_scale(&mut self, scale: f32);
}
```

**Implementation**:
- [ ] Multi-resolution rendering
- [ ] Eye tracking integration (Quest Pro, Vive Pro Eye)
- [ ] Dynamic foveation based on gaze
- [ ] Variable rate shading (VRS) support
- [ ] **Scale-adaptive foveation regions**

**Estimated Effort**: 7-10 days

---

### 7.2 Player Scale Integration

**Goal**: Ensure all VR systems respect player scale for consistent experience.

**Scale-Aware Rendering**:
```rust
pub struct ScaleContext {
    pub player_scale: f32,
    pub world_scale: f32,      // Inverse of player scale for world transforms
    pub ui_scale: f32,         // Independent UI scaling
    pub physics_scale: f32,    // Physics simulation scale factor
}

impl ScaleContext {
    // Transform matrices with scale
    pub fn world_to_view(&self, world_pos: Vec3) -> Vec3;
    pub fn view_to_world(&self, view_pos: Vec3) -> Vec3;
    
    // Scale-aware collision and interaction
    pub fn scale_interaction_distance(&self, base_distance: f32) -> f32;
    pub fn scale_grab_radius(&self, base_radius: f32) -> f32;
    
    // UI positioning in VR space
    pub fn scale_ui_distance(&self, base_distance: f32) -> f32;
}
```

**Shader Integration**:
```wgsl
struct CameraUniforms {
    view_projection: mat4x4<f32>,
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    position: vec3<f32>,
    player_scale: f32,          // Player scale factor
    world_scale: f32,           // Inverse for world transforms
    ipd_scaled: f32,            // IPD adjusted by scale
    near_plane_scaled: f32,     // Near plane adjusted by scale
    far_plane_scaled: f32,      // Far plane adjusted by scale
}

// Apply scale in vertex shader
@vertex
fn vs_main(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    
    // Scale world position relative to player
    let scaled_position = vertex.position * camera.world_scale;
    
    out.clip_position = camera.view_projection * vec4<f32>(scaled_position, 1.0);
    out.world_position = scaled_position;
    
    return out;
}
```

**Scale-Dependent Features**:
```rust
pub struct ScaleDependentQuality {
    // Adjust LOD based on player scale
    pub lod_bias: f32,
    
    // Shadow cascade distances
    pub shadow_distances: Vec<f32>,
    
    // Particle sizes
    pub particle_scale: f32,
    
    // Text/UI readability
    pub ui_compensation: f32,
}

impl ScaleDependentQuality {
    pub fn update_for_scale(&mut self, player_scale: f32) {
        // When player is small (giant world), need more detail at distance
        self.lod_bias = 1.0 / player_scale.sqrt();
        
        // Scale shadow cascade distances
        self.shadow_distances = self.base_shadow_distances
            .iter()
            .map(|d| d * player_scale)
            .collect();
            
        // Keep particles visually consistent
        self.particle_scale = player_scale;
        
        // UI compensation: keep text readable
        self.ui_compensation = 1.0 / player_scale.clamp(0.5, 2.0);
    }
}
```

**Implementation Tasks**:
- [ ] Scale context system
- [ ] Camera uniform updates with scale
- [ ] World-to-view transforms with scale
- [ ] LOD system scale compensation
- [ ] Shadow cascade scaling
- [ ] Particle system scaling
- [ ] UI scale compensation
- [ ] Physics interaction scaling
- [ ] Audio attenuation scaling

**Files to Create**:
- `src/render/scale_context.rs`
- `src/render/shaders/scale_uniforms.wgsl`

**Estimated Effort**: 5-7 days

---

### 7.3 VR Performance Optimizations

**Techniques**:
```rust
// Single-pass stereo rendering
pub struct SinglePassStereo {
    pub enabled: bool,
    pub use_multiview: bool,
    pub player_scale: f32,  // Adjust eye separation
}

// Adaptive quality
pub struct AdaptiveQuality {
    pub target_framerate: f32,
    pub min_quality: f32,
    pub max_quality: f32,
    pub adjustment_rate: f32,
    pub scale_factor: f32,  // Quality scaling based on player scale
}

// Reprojection
pub struct AsyncReprojection {
    pub enabled: bool,
    pub prediction_time: f32,
    pub scale_compensation: f32,  // Adjust prediction for scale
}
```

**Implementation Tasks**:
- [ ] Multiview rendering extension
- [ ] Single-pass stereo (render both eyes in one pass)
- [ ] **Scale-aware eye separation**
- [ ] Adaptive resolution scaling
- [ ] **Player scale quality compensation**
- [ ] Frame pacing and prediction
- [ ] Asynchronous TimeWarp/SpaceWarp
- [ ] **Scale-compensated motion prediction**

**Estimated Effort**: 12-16 days

---

## Phase 8: Advanced Features

### 8.1 Volumetric Effects

**Fog & Atmospheric Scattering**:
```rust
pub struct VolumetricFog {
    pub density: f32,
    pub color: Color,
    pub height_falloff: f32,
    pub max_distance: f32,
}

pub struct VolumetricLighting {
    pub num_samples: u32,
    pub scattering: f32,
    pub extinction: f32,
}
```

**Implementation**:
- [ ] Height-based fog
- [ ] Volumetric light shafts (god rays)
- [ ] Participating media (smoke, clouds)

**Estimated Effort**: 7-10 days

---

### 8.2 Reflections

**Screen-Space Reflections (SSR)**:
```rust
pub struct Ssr {
    pub max_steps: u32,
    pub max_distance: f32,
    pub thickness: f32,
    pub fade_distance: f32,
}
```

**Reflection Probes**:
```rust
pub struct ReflectionProbe {
    pub position: Vec3,
    pub cube_map: Texture,
    pub box_projection: Option<BoundingBox>,
    pub blend_distance: f32,
}
```

**Implementation**:
- [ ] SSR raymarching
- [ ] Reflection probe system
- [ ] Planar reflections
- [ ] Cube map blending

**Estimated Effort**: 7-10 days

---

## Phase 9: Animation & Skinning

### 9.1 Skeletal Animation

```rust
pub struct Skeleton {
    pub bones: Vec<Bone>,
    pub inverse_bind_matrices: Vec<Mat4>,
}

pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub transform: Transform,
}

pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<AnimationChannel>,
}

pub struct AnimationChannel {
    pub target_bone: usize,
    pub keyframes: Keyframes,
}
```

**Implementation Tasks**:
- [ ] Bone hierarchy system
- [ ] Animation sampling and interpolation
- [ ] Skinning shader (GPU)
- [ ] Animation blending
- [ ] IK (Inverse Kinematics)

**Estimated Effort**: 10-14 days

---

## Phase 10: Rendering Architecture

### 10.1 Render Graph

**Goal**: Flexible, optimized rendering pipeline.

```rust
pub struct RenderGraph {
    pub nodes: Vec<RenderNode>,
    pub edges: Vec<RenderEdge>,
}

pub enum RenderNode {
    ShadowPass(ShadowPassNode),
    ForwardPass(ForwardPassNode),
    PostProcess(PostProcessNode),
    Present(PresentNode),
}

impl RenderGraph {
    pub fn add_node(&mut self, node: RenderNode) -> NodeId;
    pub fn add_edge(&mut self, from: NodeId, to: NodeId);
    pub fn compile(&self) -> CompiledGraph;
    pub fn execute(&self, context: &mut RenderContext);
}
```

**Implementation Tasks**:
- [ ] Render graph builder
- [ ] Automatic resource barriers
- [ ] Render pass culling
- [ ] GPU timeline optimization

**Estimated Effort**: 14-21 days

---

## Development Timeline

### Summary

| Phase | Component | Estimated Time | Priority |
|-------|-----------|----------------|----------|
| 1 | Camera & Material System | 9-13 days | Critical |
| 2 | Lighting & Shadows | 14-20 days | Critical |
| 3 | Texturing & Normal Maps | 8-11 days | High |
| 4 | IBL & Environment | 7-10 days | High |
| 5 | Mesh & glTF Loading | 12-17 days | Critical |
| 6 | Post-Processing | 10-14 days | Medium |
| 7 | VR Optimizations + Player Scale | 24-33 days | Critical (VR) |
| 8 | Advanced Effects | 14-20 days | Low |
| 9 | Animation & Skinning | 10-14 days | Medium |
| 10 | Render Graph | 14-21 days | High |

**Total Estimated Time**: 122-173 days (4-6 months with 1 developer)

**Parallelization Opportunities**:
- Materials + Lighting can be developed in parallel
- Post-processing can happen alongside VR optimizations
- Animation work is independent of most rendering features
- Player scale system can be developed alongside camera system

---

## Player Scale System - Design Considerations

### Use Cases

**1. Accessibility**
- Users with different physical heights can adjust to comfortable viewing
- Wheelchair users can scale to match standing perspective
- Children can scale up to see from adult height

**2. Creative Expression**
- Architectural visualization: View buildings from ant or giant perspective
- Game mechanics: Size-based puzzles and exploration
- Social VR: Avatars with different scales

**3. Comfort & Ergonomics**
- Reduce VR sickness by finding optimal scale
- Adjust for different play spaces (small room vs large warehouse)
- Compensate for calibration issues

### Technical Challenges

**1. Physics Consistency**
```rust
// Scale affects physics interactions
pub struct ScaledPhysics {
    pub gravity_scale: f32,      // Adjust gravity feel (optional)
    pub collision_tolerance: f32, // Scale-aware collision detection
    pub grab_distance: f32,       // Interaction distance scaling
}

impl ScaledPhysics {
    pub fn adjust_for_scale(&mut self, player_scale: f32) {
        // Smaller players might want stronger relative gravity
        self.gravity_scale = player_scale.powf(0.5);
        
        // Collision tolerance must scale
        self.collision_tolerance = 0.01 * player_scale;
        
        // Interaction distance scales linearly
        self.grab_distance = 1.5 * player_scale;
    }
}
```

**2. UI Readability**
```rust
// UI must remain readable across scales
pub struct ScaleCompensatedUI {
    pub base_distance: f32,   // Default UI distance (1.5m)
    pub min_distance: f32,    // Minimum (0.3m)
    pub max_distance: f32,    // Maximum (5.0m)
    pub scale_compensation: f32,
}

impl ScaleCompensatedUI {
    pub fn calculate_ui_distance(&self, player_scale: f32) -> f32 {
        // Keep UI at comfortable reading distance
        let scaled_distance = self.base_distance * player_scale;
        scaled_distance.clamp(self.min_distance, self.max_distance)
    }
    
    pub fn calculate_text_size(&self, player_scale: f32) -> f32 {
        // Compensate text size to maintain readability
        // Don't scale text 1:1 with player
        1.0 + (player_scale - 1.0) * 0.5
    }
}
```

**3. LOD and Culling**
```rust
// Scale affects what detail is needed
pub struct ScaleAwareLOD {
    pub base_distances: Vec<f32>,
    pub scale_factor: f32,
}

impl ScaleAwareLOD {
    pub fn get_lod_distances(&self, player_scale: f32) -> Vec<f32> {
        // Smaller player sees less detail at same world distance
        // Larger player sees more detail
        self.base_distances
            .iter()
            .map(|d| d * player_scale)
            .collect()
    }
    
    pub fn get_culling_distance(&self, base_distance: f32, player_scale: f32) -> f32 {
        // Don't linearly scale culling - use sqrt for better balance
        base_distance * player_scale.sqrt()
    }
}
```

**4. Audio Scaling**
```rust
pub struct ScaleAwareAudio {
    pub doppler_scale: f32,
    pub attenuation_scale: f32,
}

impl ScaleAwareAudio {
    pub fn adjust_for_scale(&mut self, player_scale: f32) {
        // Doppler effect scales with velocity
        self.doppler_scale = player_scale;
        
        // Sound attenuation: smaller player hears sounds travel further
        self.attenuation_scale = 1.0 / player_scale;
    }
}
```

### Best Practices

**1. Smooth Transitions**
```rust
// Never snap scale instantly - always interpolate
pub fn update_scale_smooth(
    current: &mut f32,
    target: f32,
    dt: f32,
    speed: f32,
) {
    let diff = target - *current;
    let step = diff * speed * dt;
    *current += step;
    
    // Clamp to prevent overshooting
    if diff.abs() < 0.001 {
        *current = target;
    }
}
```

**2. Scale Limits**
```rust
pub const MIN_PLAYER_SCALE: f32 = 0.1;  // 10x smaller (ant size)
pub const MAX_PLAYER_SCALE: f32 = 10.0; // 10x larger (giant size)
pub const DEFAULT_PLAYER_SCALE: f32 = 1.0;

// Common presets
pub const SCALE_MINIATURE: f32 = 0.1;
pub const SCALE_CHILD: f32 = 0.7;
pub const SCALE_NORMAL: f32 = 1.0;
pub const SCALE_TALL: f32 = 1.3;
pub const SCALE_GIANT: f32 = 5.0;
```

**3. Network Synchronization**
```rust
// In multiplayer, scale must sync
pub struct NetworkedPlayerScale {
    pub local_scale: f32,
    pub remote_scales: HashMap<PlayerId, f32>,
    pub sync_interval: Duration,
}

impl NetworkedPlayerScale {
    pub fn sync_scale(&mut self, player_id: PlayerId, scale: f32) {
        self.remote_scales.insert(player_id, scale);
        // Update relative positions and interactions
    }
}
```

**4. Visual Feedback**
```rust
// Show player their current scale
pub struct ScaleIndicator {
    pub show_grid: bool,         // Show grid for size reference
    pub show_height_marker: bool, // Show height in meters
    pub show_comparison: bool,    // Show reference object (human silhouette)
}
```

### Integration Checklist

When implementing player scale, ensure these systems are updated:

- [ ] **Camera**: View matrices, IPD, near/far planes
- [ ] **Rendering**: World-to-view transforms, LOD distances
- [ ] **Physics**: Collision detection, interaction distances
- [ ] **UI**: Text size, panel distances, readability
- [ ] **Audio**: Attenuation, doppler, spatial positioning
- [ ] **Lighting**: Shadow cascade distances
- [ ] **Particles**: Size and emission rates
- [ ] **Navigation**: Movement speed, step height
- [ ] **Teleportation**: Arc trajectory, valid surface detection
- [ ] **Grab/Interaction**: Reach distance, object manipulation
- [ ] **Network**: Scale synchronization across clients
- [ ] **Comfort**: Motion prediction, reprojection adjustments

---

## Testing Strategy

### Unit Tests
- Material property validation
- Matrix math (view, projection)
- Light calculations
- Texture loading

### Integration Tests
- Full render pipeline with simple scene
- Shadow map rendering
- Multi-light scenes
- Post-process chain

### Performance Tests
- Frame time profiling
- GPU utilization monitoring
- VR latency measurements (motion-to-photon)
- Memory consumption tracking
- **Player scale performance impact** (different scales: 0.1x, 1x, 10x)
- **LOD transition smoothness at various scales**

### Visual Tests
- Reference image comparisons
- PBR validation scenes
- Material test spheres (different roughness/metallic)
- **Player scale visual consistency** (UI, physics, interactions)
- **Scale transition smoothness** (no popping or jarring artifacts)

---

## Dependencies & Tools

### Rust Crates
```toml
[dependencies]
# Math
glam = "0.24"

# Graphics
wgpu = "0.19"
pollster = "0.3"
bytemuck = "1.14"

# Assets
gltf = "1.4"
image = "0.24"
ktx2 = "0.3"

# Compression
basis-universal = "0.3"

# Utility
thiserror = "1.0"
log = "0.4"
```

### External Tools
- **Blender**: Asset creation and glTF export
- **RenderDoc**: GPU debugging
- **PIX** (Windows): DirectX debugging
- **NSight** (NVIDIA): Advanced GPU profiling
- **Substance Designer**: PBR texture creation

---

## References & Resources

### Books
- "Physically Based Rendering: From Theory to Implementation" (Pharr, Jakob, Humphreys)
- "Real-Time Rendering" (Akenine-Möller, Haines, Hoffman)
- "GPU Gems" series

### Papers
- Cook-Torrance BRDF (1982)
- Epic Games PBR course notes (2013)
- "Moving Frostbite to PBR" (Lagarde & de Rousiers, 2014)

### Online Resources
- [LearnOpenGL PBR Tutorial](https://learnopengl.com/PBR/Theory)
- [Google Filament Documentation](https://google.github.io/filament/Filament.html)
- [Bevy Rendering Architecture](https://bevyengine.org/learn/book/gpu-rendering/)

---

## Conclusion

This roadmap provides a complete path to production-quality PBR rendering in VR with comprehensive player scaling support. The system is designed to be modular, allowing incremental development and testing. Priority should be given to:

1. **Phase 1-2**: Core material and lighting (foundation)
2. **Phase 5**: Asset loading (enables testing with real content)
3. **Phase 7**: VR optimizations + player scale (critical for target platform)
4. **Phase 6, 8-10**: Polish and advanced features

The architecture mirrors industry-standard approaches (Bevy, Unreal, Unity) while being optimized for VR/XR workloads with features like foveated rendering, single-pass stereo, adaptive quality scaling, and comprehensive player scale support for accessibility and creative applications.

### Player Scale Impact Summary

Player scaling affects every subsystem:
- **Rendering**: Camera matrices, IPD, projection planes, LOD distances
- **Physics**: Collision tolerance, interaction distances, gravity feel
- **UI**: Text readability, panel positioning, comfortable viewing distance
- **Audio**: Attenuation curves, doppler effects, spatial accuracy
- **Performance**: Detail levels, culling distances, quality scaling

Proper implementation ensures a consistent, comfortable experience whether users are exploring as an ant or a giant.
