// Ports `manimlib/shaders/quadratic_bezier/stroke/frag.glsl` @ c5e23d9.
// CPU-side `expand_stroke` emits a ribbon where `uv.y` runs from -1 at one
// outer edge to +1 at the other; the fragment stage fades alpha over the
// last `anti_alias_width` pixels on each side.

struct Uniforms {
    mvp: mat4x4<f32>,
    // x = anti_alias_width (pixels)
    // y = pixel_size (scene units per pixel)
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) stroke_width: f32,
    @location(3) joint_angle: f32,
    @location(4) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) stroke_width: f32,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos = u.mvp * vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.stroke_width = in.stroke_width;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let aa = u.params.x;         // pixels
    let px = max(u.params.y, 1e-6);
    // half-width in pixels; distance from centerline in pixels.
    let half_w_px = (in.stroke_width * 0.5) / px;
    let dist_px = abs(in.uv.y) * half_w_px;
    // Positive inside the stroke, negative outside. Fade over ±aa/2.
    let edge_dist_px = half_w_px - dist_px;
    var out = in.color;
    out.a = out.a * smoothstep(-aa * 0.5, aa * 0.5, edge_dist_px);
    return out;
}
