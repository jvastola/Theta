// Text rendering shader — screen-space textured quads

struct Uniforms {
    resolution: vec2<f32>,
}

@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0)
var t_font: texture_2d<f32>;
@group(0) @binding(1)
var s_font: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Convert pixel coords to clip space: (0,0) = top-left
    var clip = vec2<f32>(
        input.position.x / uniforms.resolution.x * 2.0 - 1.0,
        1.0 - input.position.y / uniforms.resolution.y * 2.0,
    );
    out.clip_position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var tex_color = textureSample(t_font, s_font, in.uv);
    return vec4<f32>(in.color.rgb, in.color.a * tex_color.a);
}
