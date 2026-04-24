# Porting note: stroke pipeline

**Status:** Slice D shipped — real port of `quadratic_bezier/stroke/*.glsl`.
**Manimgl reference:** `reference/manimgl/manimlib/shaders/quadratic_bezier/stroke/` @ commit `c5e23d9`.

## What manimgl does

Manimgl strokes every `VMobject` as a sequence of **quadratic Bézier curves**, each stroked with a shader that:

1. **Accepts per-vertex attributes** — position, stroke width, unit tangent, joint angle, stroke color. The width is *per vertex*, not per object, which is why fades, tapers, and highlights all work uniformly.
2. **Uses a geometry shader** (`geom.glsl`) to widen each Bézier segment into a triangle ribbon whose cross-section is the stroke width at that arc parameter.
3. **Approximates the Bézier distance field** in the fragment shader and uses it to compute coverage for analytic anti-aliasing along the stroke's lateral edge.
4. **Composites with alpha blending** so overlapping strokes combine correctly.

## What Slice D does

Manimax matches manimgl's behaviour with one architectural substitution: WGSL has no geometry shader, so the ribbon is expanded CPU-side by `expand_stroke` before upload. Everything else maps:

1. **Path sampling.** `sample_bezpath(verbs)` — ports the intent of manimgl's per-curve sampling. Produces a flat `Vec<QuadraticSegment>` from any `BezPath`: lines become degenerate quadratics (`p1 = midpoint`), cubics split at fixed depth 2 and each leaf is least-squares-fit as a quadratic, `Close` emits a line back to the sub-path start.
2. **Ribbon expansion.** `expand_stroke(segments, widths, color, joint)` — ports `geom.glsl`. Per-quadratic step count `(arc_len * POLYLINE_FACTOR).ceil().clamp(2, STROKE_MAX_STEPS)` matches manimgl's `POLYLINE_FACTOR = 100` / `MAX_STEPS = 32`. Miter joints use `offset = (w/2) * (N1 + N2) / (1 + N1·N2)`; `Auto` falls back to bevel when `cos(θ) ≤ MITER_COS_ANGLE_THRESHOLD = -0.8`, same as manimgl.
3. **Per-vertex attributes.** `StrokeVertex { position, uv, stroke_width, joint_angle, color }` — drops `unit_normal` (recomputed on CPU), keeps the rest.
4. **Fragment-stage AA.** `path_stroke.wgsl` ports `frag.glsl`: `sd = abs(uv.y) - half_width_ratio`; `alpha *= smoothstep(0.5, -0.5, sd)`. We parameterise differently (ribbon-space `uv.y` in `[-1, 1]` plus `stroke_width` + `pixel_size` uniform) but produce the same visual result.
5. **Uniforms.** `StrokeUniforms { mvp, params: vec4 }` where `params.x = anti_alias_width` (pixels, default 1.5 matching manimgl's `ANTI_ALIAS_WIDTH`) and `params.y = pixel_size` (scene units per pixel, computed per render from `Camera`).
6. **MSAA 4×** stays on as a secondary AA layer over the shader-driven fade.

## Known deltas from manimgl

- **No 3D stroke.** `unit_normal` is always `[0, 0, 1]` for Slice D. Out-of-plane strokes land with the 3D camera (Slice F).
- **No round joints.** Miter / Bevel / Auto only. Round joints are non-trivial without tessellating arc fans.
- **Cubic subdivision is fixed depth 2** (4 quadratics per cubic). Sharp cubics may kink; raise `CUBIC_SPLIT_DEPTH` if real scenes show it.
- **Uniform color per object** for Slice D. Per-vertex color is out of scope until the Python surface Step 4 exposes it.

## Files touched in Slice D

- `crates/manim-rs-raster/src/tessellator.rs` — `QuadraticSegment`, `StrokeVertex`, `JointType`, `sample_bezpath`, `expand_stroke`, `polyline_to_segments`.
- `crates/manim-rs-raster/src/pipelines/path_stroke.rs` — new `StrokeUniforms { mvp, params }`, rich vertex attribute layout.
- `crates/manim-rs-raster/src/shaders/path_stroke.wgsl` — analytic SDF AA fragment stage.
- `crates/manim-rs-raster/src/lib.rs` — `Runtime::render` threads `pixel_size`; `tessellate_object` uses `expand_stroke`; `FillUniforms` split into its own struct.
- `crates/manim-rs-raster/tests/stroke_aa.rs` — edge-fade assertion.
