// path_stroke.wgsl — Slice B stroke shader.
// Rigid-width polyline stroke with one solid color. No AA, no Bezier.
// Replaces `manimlib/shaders/quadratic_bezier/stroke/*.glsl` for Slice B only;
// the real port is Slice D. See docs/porting-notes/stroke.md.

struct Uniforms {
    mvp: mat4x4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos = u.mvp * vec4<f32>(in.position, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(_in: VertexOut) -> @location(0) vec4<f32> {
    return u.color;
}
