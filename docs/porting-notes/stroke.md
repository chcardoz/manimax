# Porting note: stroke pipeline

**Status:** Slice B port only. Real port is Slice D.
**Stub label:** `PORT_STUB_MANIMGL_STROKE`.
**Manimgl reference:** `reference/manimgl/manimlib/shaders/quadratic_bezier/stroke/` (`stroke.vert`, `stroke.frag`, `stroke.geom`).

## What manimgl does

Manimgl strokes every `VMobject` as a sequence of **quadratic Bézier curves**, each stroked with a shader that:

1. **Accepts per-vertex attributes** — position, stroke width, unit tangent, joint type, stroke color. The width is *per vertex*, not per object, which is why fades, tapers, and highlights all work uniformly.
2. **Uses a geometry shader** (`stroke.geom`) to widen each Bézier segment into a quad whose cross-section is the stroke width at that arc parameter.
3. **Approximates the Bézier distance field** in the fragment shader and uses it to compute coverage for analytic anti-aliasing along the stroke's lateral edge. This is why manimgl's strokes look smooth at low resolutions where a naive fill would alias badly.
4. **Composites with alpha blending** so overlapping strokes combine correctly.

The pipeline is intricate because it handles curved paths, variable width, and AA simultaneously without touching MSAA.

## What Slice B does instead

Slice B's stroke is deliberately the minimum shape that can draw a closed polyline on screen:

1. **Straight-line polyline only.** No Bézier math. Input is `Vec<[f32; 3]>` (z dropped to 0), fed to `lyon::path::Path` as `begin`/`line_to`/`end`.
2. **Single `StrokeOptions::DEFAULT.with_line_width(w)`.** Rigid, per-object width. No per-vertex width attribute.
3. **`lyon::tessellation::StrokeTessellator` produces a triangle mesh.** This is CPU-side tessellation, not a geometry shader. Output is `VertexBuffers<Vertex, u32>` where `Vertex = { position: vec2, uv: vec2 }`. `uv` is unused; reserved for Slice D per-vertex attributes.
4. **One WGSL shader (`path_stroke.wgsl`)** with trivial vertex (apply MVP uniform, z=0) and trivial fragment (return uniform color). **No AA**, so stroke edges are aliased. Alpha blending is on at the pipeline level so the `stroke_color` RGBA's A component already works.
5. **One uniform buffer, one bind group, one pipeline.** Rewritten via `queue.write_buffer` per draw call. Vertex/index buffers pre-sized at 64 KiB each — enough for any Slice B polyline.
6. **No fill.** The IR's `Polyline` variant does not have a fill field for Slice B.

## What this implies

- Visual delta vs. manimgl: Slice B strokes look **blocky and aliased**, especially at diagonal segments. This is expected.
- At 480×270, the aliasing is visible but the shape is unambiguous.
- Stroke width is in **scene units**, not pixels. At Slice B camera (`[-8,8] × [-4.5,4.5]`) → 480 px, each unit is 30 px. `stroke_width: 0.08` → ~2.4 px.

## What Slice D will do

Stop using `StrokeTessellator`. Port `quadratic_bezier/stroke/*.glsl` to WGSL:

- Add per-vertex attributes: `stroke_width`, `tangent`, `joint_type`.
- Add Bézier variants to the IR's geometry union: `BezPath { curves: Vec<QuadraticBezier> }`.
- Compute a Bézier SDF in the fragment shader; use `fwidth` / `screenPixelRange` for coverage AA.
- Keep straight-line polyline as a degenerate case (B(t) = A + t(C-A)).
- Add MSAA 4× to the render target as a secondary AA layer.

Per CLAUDE.md's porting practice #3, when that port happens, each function header gets a manimgl source file + commit SHA citation.

## Files touched in Slice B

- `crates/manim-rs-raster/src/tessellator.rs` — `Vertex`, `Mesh`, `tessellate_polyline`.
- `crates/manim-rs-raster/src/pipelines/path_stroke.rs` — `StrokePipeline`, `StrokeUniforms`.
- `crates/manim-rs-raster/src/shaders/path_stroke.wgsl` — WGSL source.
- `crates/manim-rs-raster/src/lib.rs` — `Runtime::render(scene_state, camera, background)`.
