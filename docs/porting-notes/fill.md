# Porting note: fill pipeline

**Status:** Slice C port. Pairs with MSAA on the color target.
**Stub label:** none — this is the shipping implementation.
**Manimgl reference:** `reference/manimgl/manimlib/shaders/quadratic_bezier/fill/` (`fill.vert`, `fill.frag`, `fill.geom`) at commit `c5e23d9`.

## What manimgl does

Manimgl fills every `VMobject` using a shader pair keyed to the same
quadratic-Bézier representation as stroke:

1. **Triangulates Bézier-patched interiors in a geometry shader.** Every
   curve segment contributes a triangle whose third vertex is chosen so the
   union of triangles covers the fill region.
2. **Per-fragment inside/outside test** using the Loop-Blinn implicit form
   (`u² - v ≷ 0` in barycentric-like coords) decides whether a fragment is
   inside the curved boundary or outside. This is how manimgl gets
   **curved fills without tessellating the curve into straight segments**.
3. **No MSAA.** Fragment-level coverage comes from the Loop-Blinn test and a
   derivative-based anti-alias band. Fill and stroke share the AA strategy.
4. **Handles self-intersection and holes** implicitly via the shader's
   winding accumulation; no CPU preprocessing.

The pipeline is intricate because the representation (quadratic Bézier patches)
and the GPU solve (Loop-Blinn) are cotuned — you can't port one without the
other.

## What Slice C does instead

Slice C's fill is the minimum shape that renders a filled `BezPath` recognisably:

1. **CPU tessellation.** `lyon::tessellation::FillTessellator` flattens the
   `BezPath` (cubic/quadratic included) into triangles. Output is a
   `VertexBuffers<FillVertex, u32>` where `FillVertex = { position: vec2 }`.
   No curved fills on the GPU; every curve becomes a polyline-like triangle fan.
2. **Non-zero winding.** `FillOptions::DEFAULT.with_fill_rule(FillRule::NonZero)`.
   Matches manimgl's winding interpretation. Self-intersecting paths render
   the non-zero answer, not the even-odd one — document in authored scenes.
3. **Trivial WGSL shader** (`path_fill.wgsl`): MVP uniform, solid color
   uniform, no derivatives, no coverage work. `{ mvp, color }` layout is
   aliased from `StrokeUniforms` — the two shaders read the same buffer shape.
4. **AA via 4× MSAA on the color target**, not analytic coverage. `FillPipeline`
   sets `multisample: { count: MSAA_SAMPLE_COUNT, ... }` to match the shared
   MSAA color attachment. Edges soften from raster supersampling, not the
   shader.
5. **No per-vertex attributes beyond position.** Color and opacity live in the
   uniform; they're per-object, not per-vertex.
6. **Same per-object submit pattern as stroke.** The `queue.write_buffer`
   ordering gotcha (`docs/gotchas.md`) still applies — fill geometry is
   rewritten per object between submits.

## What this implies

- **Curved fills look piecewise-linear up close.** At canonical sizes
  (480×270 and 1920×1080 in authored scenes) the lyon flattening tolerance
  is visually indistinguishable; zoom further and you'll see facets.
- **Fill winding is authored.** A star drawn as a single self-intersecting
  loop fills the inner pentagon twice (visible only if translucent) under
  non-zero. Author stars as five filled triangles if that matters.
- **MSAA is doing the AA work.** Turning MSAA off (sample count 1) is a
  regression — edges alias hard. `path_fill.wgsl` alone does nothing to
  smooth them.

## What Slice D / later will do

Real port of `manimlib/shaders/quadratic_bezier/fill/*.glsl` when the rest of
the Bézier stroke port lands:

- GPU-side Loop-Blinn patch evaluation. Curved boundaries stay curved on
  display.
- Drop MSAA's role to edge-only smoothing; let the fragment shader do coverage
  on curves.
- Share the per-vertex `(tangent, width, joint_type)` attributes with the
  stroke pipeline (Slice D's `PORT_STUB_MANIMGL_STROKE`).

Per CLAUDE.md porting practice #3, when that port happens, each ported function
gets a manimgl source file + commit SHA header.

## Files touched in Slice C

- `crates/manim-rs-raster/src/pipelines/path_fill.rs` — `FillPipeline`,
  `FillVertex`. `FillUniforms` aliased to `StrokeUniforms`.
- `crates/manim-rs-raster/src/shaders/path_fill.wgsl` — WGSL source.
- `crates/manim-rs-raster/src/tessellator.rs` — `FillMesh`,
  `tessellate_polyline_fill`, `tessellate_bezpath_fill`, shared
  `tessellate_fill_path` using `FillRule::NonZero`.
- `crates/manim-rs-raster/src/lib.rs` — MSAA color + resolve textures,
  `Runtime::render` submits fill before stroke per object.
