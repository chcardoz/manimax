// path_fill.wgsl — Slice C Step 5 fill shader.
// Solid-color fill of triangulated path interiors. No AA (the pipeline relies
// on the MSAA color attachment for edge smoothing).

struct Uniforms {
    mvp: mat4x4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec2<f32>,
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
