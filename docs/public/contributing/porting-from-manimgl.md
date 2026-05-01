# Porting from ManimGL

ManimGL is the canonical reference for what scene-authoring should *feel* like, and the source of truth for rendering and animation semantics. The submodule lives at `reference/manimgl/`. Read the manimgl equivalent before inventing a new API or translating a rendering concept.

This page records what each subsystem ports, what it intentionally diverges on, and the invariants that aren't obvious from either codebase. When you port a new subsystem, drop another `##` section here.

## Conventions

- **Stub label:** `PORT_STUB_MANIMGL_<subsystem>` — greppable, unambiguous, replaces the generic `TODO`.
- **Per-function attribution:** ported algorithms get a header comment with the manimgl source path + commit SHA.
- **Literal-first:** keep manimgl's variable names and control flow on first pass. Refactor to idiomatic Rust only after it works — eliminates "logic bug vs. porting bug" ambiguity.

## Evaluator

**Status:** Slice B complete (position-only, linear easing).
**ManimGL reference:** `manimlib/animation/animation.py` — `Animation.interpolate(alpha)` and the `update_mobjects`/`begin`/`finish` lifecycle.

The evaluator is **reimplemented, not ported**. ManimGL's model is stateful and replayed-from-zero; ours is pure and random-access. Two layers exist:

- `Scene` in `manim-rs-ir` is the plain serializable contract.
- `Evaluator` in `manim-rs-eval` is the compiled/runtime form: it owns a one-time track index and wraps timeline objects in `Arc<Object>` so repeated `eval_at` calls share geometry cheaply.

Shared ownership is a runtime optimization, not part of the IR language.

### The purity contract

`eval_at(scene, t) -> SceneState` is a **pure function**. No caching, no history, no state carried across calls. Same `(scene, t)` → same `SceneState`. Always. This is load-bearing:

1. **Random-access frame rendering** — `eval_at(scene, frame_k / fps)` produces frame `k` without evaluating frames `0..k-1`.
2. **Parallel / chunked rendering** — workers can render disjoint frame ranges in isolation.
3. **Memoization by `(ir_hash, t)`** — legal because `eval_at` is pure.
4. **Determinism in tests** — frame-pixel tests can render any `t` directly.

If you add a new track type, easing, or timeline op, the rule is: **no hidden state.** Anything that looks like it needs state across times means the IR is missing a field.

### Invariants the evaluator trusts (not validated at runtime)

- `scene.timeline` is sorted non-decreasing by `t`.
- Every `Track::Position.id` matches some `TimelineOp::Add.id`.
- Segments within a position track are non-overlapping.

The Python recorder (`python/manim_rs/scene.py`) is responsible for emitting IR that satisfies these. Validate before handing IR from an external source to the evaluator.

### The gap-clamping rule

For a position track with two segments `[0.0, 1.0]` and `[1.2, 2.0]`, asking for `t = 1.1` must return the `to` of the segment that ended at 1.0 — **not** the `to` of the overall last segment. Returning the final segment's `to` during a hold would teleport the object. See `crates/manim-rs-eval/src/lib.rs:108-125`; a dedicated test pins this — don't delete it when refactoring.

### Missing-track conventions

- No position track for an object → position `(0, 0, 0)`.
- `t < segments[0].t0` → `(0, 0, 0)`.
- `t > segments.last().t1` → `segments.last().to`.

Summed across tracks (multiple position tracks → orbit + drift, etc.).

## Rate functions (easings)

**ManimGL source:** `manimlib/utils/rate_functions.py` @ `c5e23d9`.
**Rust:** `crates/manim-rs-eval/src/lib.rs` — `apply_easing`, `smooth`, `squish`, `bezier_scalar`.

Fifteen named easings, selectable per track segment:

```
Linear, Smooth, RushInto, RushFrom, SlowInto, DoubleSmooth,
ThereAndBack, ThereAndBackWithPause { pause_ratio },
RunningStart { pull_factor }, Overshoot { pull_factor },
NotQuiteThere { inner, proportion }, Wiggle { wiggles },
SquishRateFunc { inner, a, b }, Lingering, ExponentialDecay { half_life }
```

### Invariants

- Each easing is a pure `fn(alpha: f32) -> f32`. `alpha` is the segment-local parameter in `[0, 1]`.
- Parameterised variants are **struct variants**, not unit variants with extra fields — serde's `deny_unknown_fields` is silent on unit variants under the internal tag `kind` (ADR 0002).
- `NotQuiteThere` and `SquishRateFunc` carry a boxed `inner: Easing` and recurse. The evaluator caps nothing; pathological nesting is a user bug.

### Edge cases

- **`SquishRateFunc { a, b }` with `a == b`** collapses to the inner's value at `0`. Manimgl has the same behavior. Avoid `a == b`.
- **`ExponentialDecay { half_life }` never hits 1** at `alpha = 1.0` — output is `1 - exp(-1/half_life)`. Manimgl preserves this bias, so do we.
- **`Wiggle` and `ThereAndBack` are non-monotonic** and end at 0. Use them for position/rotation, not opacity.
- **f32 round-trip precision.** `ThereAndBackWithPause(pause_ratio=1.0/3.0)` drops precision across Python f64 → serde f32 → msgspec f64. Tests use dyadic rationals.

### Composition rules (applied in `apply_easing`'s callers)

- **Position / rotation:** sum across tracks at `t`.
- **Opacity / scale:** multiply across tracks.
- **Color:** take the latest-starting segment's value.

These rules are evaluator policy, not part of the easings themselves; they interact with `Wiggle`/`ThereAndBack` semantics.

## Geometry primitives

**Status:** Slice B (`Polyline`) + Slice C (`BezPath`).
**ManimGL reference:** `manimlib/mobject/geometry.py`, `manimlib/mobject/types/vectorized_mobject.py`.

ManimGL models shapes as `VMobject`s — flat arrays of cubic Bézier anchors and handles (`A H1 H2 A H1 H2 …`). Closedness is implicit (duplicate the first anchor). Manimax splits the surface in two:

- `Polyline(points, *, stroke_color, stroke_width, fill_color, closed=True)`
- `BezPath(verbs, *, stroke_color, stroke_width, fill_color)` with `move_to`, `line_to`, `quad_to`, `cubic_to`, `close` verb helpers.

Both are inert descriptions. `Scene.add(...)` assigns `obj._id` and calls `to_ir()` later.

### Divergences

- **`closed` is explicit, default `True`.** Manimgl closes by duplicating the first point; we connect last → first when `closed=True`. Do **not** duplicate the first point yourself — renders a zero-length segment.
- **Minimum `Polyline` length is 2.**
- **`BezPath` `MoveTo` implicitly ends the previous subpath as open.** `verbs_to_path` calls `builder.end(false)` on each subsequent `MoveTo`. Author an explicit `close()` if you want subpaths closed.
- **z is dropped by the rasterizer.** Points are `Vec3` for IR/Python parity but `tessellator.rs` flattens to 2D.
- **Named primitives (`Polygon`, `Circle`, `Square`, `Arc`) don't exist yet.** They're one-liners over `Polyline`/`BezPath`; deliberately deferred.
- **Fill non-zero winding** is inherited from lyon (manimgl matches).
- **`stroke_width` is in scene units, not pixels.** Camera is pinned at `[-8, 8] × [-4.5, 4.5]`; a `0.04` stroke is ~0.25% of scene width.

## Stroke pipeline

**Status:** Slice D — real port of `quadratic_bezier/stroke/*.glsl`.
**ManimGL reference:** `manimlib/shaders/quadratic_bezier/stroke/` @ `c5e23d9`.

ManimGL strokes every `VMobject` as quadratic Bézier curves with a geometry shader that widens each segment into a triangle ribbon, plus a fragment shader that approximates the Bézier distance field for analytic AA.

WGSL has no geometry shader, so the ribbon is expanded CPU-side by `expand_stroke` before upload. Everything else maps:

1. **Path sampling** (`sample_bezpath`): produces a flat `Vec<QuadraticSegment>`. Lines become degenerate quadratics, cubics split at fixed depth 2 with each leaf least-squares-fit as a quadratic, `Close` emits a line back to the sub-path start.
2. **Ribbon expansion** (`expand_stroke`): per-quadratic step count `(arc_len * 100).ceil().clamp(2, 32)` matches manimgl's `POLYLINE_FACTOR=100` / `MAX_STEPS=32`. Miter joints use `offset = (w/2) * (N1 + N2) / (1 + N1·N2)`; `Auto` falls back to bevel when `cos(θ) ≤ -0.8` (matches manimgl's `MITER_COS_ANGLE_THRESHOLD`).
3. **Per-vertex attributes:** `{ position, uv, stroke_width, joint_angle, color }` — drops `unit_normal` (recomputed CPU-side).
4. **Fragment-stage AA:** `sd = abs(uv.y) - half_width_ratio`; `alpha *= smoothstep(0.5, -0.5, sd)`. Same visual result as manimgl.
5. **Uniforms:** `StrokeUniforms { mvp, params: vec4 }` where `params.x = anti_alias_width` (1.5 px, matches manimgl) and `params.y = pixel_size` (computed per render from `Camera`).
6. **MSAA 4×** as a secondary AA layer.

### Known deltas

- **No 3D stroke.** `unit_normal` is always `[0, 0, 1]`; out-of-plane strokes wait on the 3D camera.
- **No round joints.** Miter / Bevel / Auto only.
- **Cubic subdivision is fixed depth 2** (4 quadratics per cubic). Sharp cubics may kink; raise `CUBIC_SPLIT_DEPTH` if real scenes show it.
- **Uniform color per object** for now.

## Fill pipeline

**Status:** Slice C. Pairs with MSAA on the color target.
**ManimGL reference:** `manimlib/shaders/quadratic_bezier/fill/` @ `c5e23d9`.

ManimGL's fill triangulates Bézier-patched interiors in a geometry shader and uses the Loop-Blinn implicit form for per-fragment inside/outside tests, getting curved fills *without* tessellating the curve into segments.

Manimax's fill is the minimum shape that renders a filled `BezPath` recognisably:

1. **CPU tessellation** via `lyon::tessellation::FillTessellator`. Output is `VertexBuffers<FillVertex, u32>` where `FillVertex = { position: vec2 }`. No curved fills on the GPU; every curve becomes a polyline-like triangle fan.
2. **Non-zero winding** via `FillRule::NonZero`. Self-intersecting paths render the non-zero answer.
3. **Trivial WGSL shader** (`path_fill.wgsl`): MVP uniform, solid color uniform, no derivatives, no coverage work.
4. **AA via 4× MSAA on the color target**, not analytic coverage. Turning MSAA off is a regression.

### Implications

- Curved fills look piecewise-linear up close. At 480×270 / 1920×1080 the lyon flattening is visually indistinguishable; zoom further and you'll see facets.
- Fill winding is authored. A star drawn as a single self-intersecting loop fills the inner pentagon twice under non-zero. Author stars as five filled triangles if that matters.
- A real port of `quadratic_bezier/fill/*.glsl` (GPU-side Loop-Blinn) is future work.

## Transforms (animations)

**Status:** Slice C. Pairs with the evaluator track semantics.
**ManimGL reference:** `manimlib/animation/*` (`animation.py`, `transform.py`, `fading.py`).

ManimGL's `Animation` is an active object — subclasses override `interpolate(alpha)` and mutate a `Mobject` in place each frame. Manimax's animations in `python/manim_rs/animate/transforms.py` are **inert descriptions** of time-varying value tracks: each has a target object (already `scene.add()`'d), a `duration`, an optional `easing`, and an `emit(t_start)` method returning `list[ir.Track]`. All interpolation happens in Rust.

### Surface

| Transform | Track kind | From → To | Notes |
|---|---|---|---|
| `Translate(obj, delta, duration, *, easing=None)` | `PositionTrack` | `(0,0,0) → delta` | Offset relative to base position |
| `Rotate(obj, angle, duration, *, easing=None)` | `RotationTrack` | `0.0 → angle` | Radians |
| `ScaleBy(obj, factor, duration, *, easing=None)` | `ScaleTrack` | `1.0 → factor` | Multiplicative; composes across tracks |
| `FadeIn(obj, duration, *, easing=None)` | `OpacityTrack` | `0.0 → 1.0` | |
| `FadeOut(obj, duration, *, easing=None)` | `OpacityTrack` | `1.0 → 0.0` | |
| `Colorize(obj, from_color, to_color, duration, *, easing=None)` | `ColorTrack` | `from → to` (override) | Both required |

### Divergences

- **Default easing is `Linear`**, not `Smooth`. Pass `easing=ir.SmoothEasing()` to match manimgl.
- **`ScaleBy`, not `ScaleTo`** — multiplicative composition. Two `ScaleBy(2.0)` calls land at 4×.
- **`Colorize` is last-write override**, not a tween of authored color. Both `from_color` and `to_color` are required; on frame 0 the displayed color is `from_color`. Pass the current color explicitly for "fade from current".
- **Target must already be in the scene.** Manimgl would auto-add during `play()`; here ordering is explicit.
- **Duration must be strictly positive.** Zero-duration jumps are authored as `ir.*Segment(t0=t, t1=t, ...)` directly.

## Tex

**Status:** Slice E shipped.
**ManimGL reference:** `manimlib/mobject/svg/tex_mobject.py`, `manimlib/utils/tex_file_writing.py`.
**RaTeX reference:** `github.com/erweixin/RaTeX` (pinned in `[workspace.dependencies]`).

Reimplemented, not ported. ManimGL shells out to system `latex`, runs `dvisvgm --no-fonts` to produce SVG, and parses paths into mobjects. Manimax replaces the whole pipeline with RaTeX (pure Rust, KaTeX-grammar subset).

### Kept from manimgl

- `Tex(src, color=...)` constructor shape — same positional source string, same keyword color override.
- Color override is "set the whole thing", not "blend with author color." Items RaTeX explicitly colored via `\textcolor` keep their per-item color; the rest get the top-level color.
- Bus-factor escape hatch is "vendor the parser" (ADR 0008 §G).

### Explicitly dropped

- **System `latex`.** No subprocess, no temp dir, no `dvisvgm`. Trade-off is the KaTeX-coverage subset (see `Tex coverage`).
- **TikZ, `chemfig`, packages.** Out of subset; surface as parse errors at construction.
- **Auto equation numbering** — RaTeX requires explicit `\tag{...}`.
- **Computer Modern.** RaTeX uses KaTeX's bundled fonts; `\mathcal` and `\mathfrak` are the most visibly different faces.
- **`set_color_by_tex` / glyph-range overrides.** Whole-tex `color=` plus inline `\textcolor` only.

### RaTeX `DisplayList` contract

`crates/manim-rs-tex/src/adapter.rs` translates to `Vec<(BezPath, [f32; 4])>`. Three coordinate-system facts:

1. **RaTeX is y-down, em-units.** The adapter applies one y-flip + one em→world scale at the boundary.
2. **`DisplayItem::GlyphPath` carries a `font` string + `char_code`.** Resolution lives in `crates/manim-rs-text/src/font.rs` via `ratex-katex-fonts`. If RaTeX renames a font upstream, glyph lookups go silent.
3. **`DisplayItem::GlyphPath::commands` is a placeholder** ("not used by any renderer"). We pull outlines through swash, not through this field.

`PathCommand` cubic ordering: `CubicTo { x1, y1, x2, y2, x, y }` → `BezPath::curve_to(p1, p2, p3)`. Swap and you get visually plausible but mathematically wrong cubics.

### `\textcolor` interaction

`Tex(r"\textcolor{red}{x} + y", color=BLUE)`: `x` is red, `+` and `y` are blue. `compile_tex` only overwrites items whose color RaTeX left at default. `\textcolor` accepts CSS color names; hex literals (`{#ff8800}`) are not supported — use `Tex(color=(r,g,b,a))` for arbitrary colors.

### Coordinate flip and scale

```
world.x = item.x
world.y = -item.y          # y-flip
final = world * em_to_world_scale
```

`em_to_world_scale` is the IR's `Tex.scale` *not* baked into the geometry (ADR 0008 §A — baking caused double-application during Slice E Step 4). `compile_tex` produces `Vec<Arc<Object>>` at unit scale; per-glyph `ObjectState`s carry `parent_scale * tex_scale`; raster multiplies that into the MVP. **Don't inline `Tex.scale` into the `BezPath` for "performance" — fan-out children share the cached geometry across instances; baking scale defeats the cache.**

### Visual bugs already paid for

- **Swash hinting at low ppem.** Outlines extracted at ppem ≈ 1.0 get TrueType hinting applied — control points snap to the integer pixel grid. Fix: extract at `OUTLINE_PPEM = 1024` and post-multiply by `Affine::scale(scale / 1024)` (ADR 0008 §C).
- **Lyon `FillOptions::DEFAULT.tolerance = 0.25` is em-scaled-fatal.** Calibrated for SVGs in pixel units; flattens em-scaled glyphs into octagons. Fix: pin `FILL_TOLERANCE = 0.001` (ADR 0008 §D).

If a glyph-rendering bug surfaces and isn't either of these, render the *intermediate stage* (BezPath dump, single-glyph snapshot at the actual ppem) before guessing. The single-frame API (`python -m manim_rs frame`, ADR 0008 §F) makes that cheap.

### Cache discipline

Cache key is the entire `Object::Tex` node minus `scale` and `macros` (both invalidate cache without changing shaped output). **Only fields that change the BezPath geometry belong in the cache key.** Color is in the key because `compile_tex` bakes color into `(BezPath, [f32; 4])` pairs; scale and per-instance transforms apply at the eval-time fan-out site.

### Tex coverage

The corpus in `tests/python/tex_corpus.py` is the executable form of the supported subset.

**Works:**
- All KaTeX font faces: math italic, math regular, blackboard bold (`\mathbb`), calligraphic (`\mathcal`), Fraktur (`\mathfrak`), bold math (`\mathbf`)
- Sub/superscripts, arbitrary nesting, limits on big operators
- `\frac`, `\binom`, `\begin{cases}`
- Large operators with limits (`\sum`, `\int`, `\prod`, `\lim`), `\to`, `\infty`, `!`
- `\sqrt` including nested radicals
- Stretched delimiters (`\left ... \right`)
- Matrices (`pmatrix`, `bmatrix`, `vmatrix`, `matrix`)
- `aligned` environment
- Accents (`\hat`, `\tilde`, `\bar`, `\vec`, `\dot`, `\ddot`)
- Spacing primitives (`\,`, `\:`, `\;`, `\quad`, `\qquad`)
- `\text{...}` (no math re-entry inside)
- Top-level color and `\textcolor{name}{...}` (CSS color names only)
- Macro pre-expansion via `Tex(src, tex_macros={r"\R": r"\mathbb{R}"})` — **no-arg macros only**

**Not supported** (raises `ValueError: invalid Tex source` at construction):

| Feature | Workaround |
|---|---|
| `\usepackage{...}`, TikZ/PGF, `chemfig`, `mhchem` | Future `engine="latex"` fallback |
| `\newcommand` with arguments | Define expansion in Python, pass via `tex_macros={}` |
| Auto equation numbering | Explicit `\tag{1}` |
| `\bm{...}` | `\mathbf{...}` for upright bold; no italic-bold |
| Hex colors `\textcolor{#ff8800}{...}` | Top-level `Tex(color=(r,g,b,a))` |
| Non-Latin scripts inside `\text{...}` | Use `Text(...)`, position separately |
| Custom math fonts | None — KaTeX fonts only |
| Nested `\text{... math ...}` re-entry | Break into separate `Tex` calls |

### Visible deltas vs. manimgl

Even when an expression looks "the same," sub-pixel differences are guaranteed. Documented:

- **Font:** ManimGL → Computer Modern; Manimax → KaTeX bundled fonts. `\mathcal` and `\mathfrak` differ most.
- **Spacing:** KaTeX's tables aren't bit-equal to TeX's. Wide expressions can drift several em-units.
- **Stretched delimiter sizing:** KaTeX picks from a discrete table; LaTeX synthesizes from extension pieces.
- **Accent positioning:** KaTeX metadata vs. TeX's `\skew` — `\hat{f}` can drift by a fraction of an em.

These are deliberate (the price of pure Rust). Snapshot baselines are Manimax's rendering, not manimgl's.

### Future `engine="latex"` escape hatch

When a user hits a coverage gap they can't work around (TikZ, exotic notation), the planned escape is `Tex(src, engine="latex")` — falls back to a system `latex` + `dvisvgm` pipeline. Same `MObjectKind::Tex` IR variant, different compile path. Trade-off: zero-install becomes "requires system LaTeX." Lands when a real user hits a wall.

We are **not** embedding Tectonic — larger binary, C-build pain, first-run network fetch.

## Text

**Status:** Slice E shipped.
**ManimGL reference:** `manimlib/mobject/svg/text_mobject.py`. ManimGL renders text via Pango/Cairo and parses the output back into an `SVGMobject`.
**Manimax port:** `crates/manim-rs-text/src/cosmic.rs` + `python/manim_rs/objects/text.py`. ADR 0012.

Reimplementation. ManimGL: Pango shape → Cairo render → SVG → parse. Manimax: cosmic-text shape → swash outlines → kurbo BezPath → fill. The user-facing constructor follows manimgl's keyword shape; the rendering pipeline shares nothing.

### Kept from manimgl

- `Text(src, *, font=None, weight=..., size=..., color=..., align=...)` — same field names, same default.
- `align` keyword as a string: "left" / "center" / "right" (omits "justified").
- Default font is "the bundled one" — Inter Regular, used when `font=None`.

### Explicitly dropped

- **Pango.** No system dep, no C linker, no per-platform font scan. cosmic-text covers shaping; swash covers outlines.
- **Justification.** Supported by cosmic-text but not exposed — would imply visual verification we don't do.
- **System-font discovery.** fontdb supports `load_system_fonts`; we load only `Inter-Regular.ttf`. Same scene renders identically on every host (ADR 0012).
- **Multi-weight bundle.** Inter Regular only. Bold/italic require `Text(..., font="path/to.ttf")`.
- **Rich-text inline styling.** Compose with multiple `Text` mobjects, or use `Tex(... \text{...} ...)` for math-adjacent text.

### cosmic-text contract

1. **Layout runs at `SHAPE_PPEM = 1024`, not the user's `size`.** Same hinting workaround as Tex (ADR 0008 §C). Result is post-multiplied by `size / SHAPE_PPEM`.
2. **Baseline at world `y = 0` for the first line.** cosmic-text emits y-down; we anchor by subtracting the first run's `line_y`. Ascenders positive y, descenders negative, subsequent lines stack downward.
3. **Sub-pixel positioning.** We bypass `LayoutGlyph::physical()` (which rounds to pixels for raster targets) — Manimax renders to vectors, so `glyph.x_offset` / `glyph.y_offset` come through directly.

`Wrap::None` is hard-coded. Break with `\n`.

### Alignment semantics

cosmic-text's `Align` operates per-line within buffer bounds. We pass `Some(f32::INFINITY)` with `Wrap::None`, so **alignment has no visible effect on a single-line input**. It only matters with explicit `\n` producing multi-run input where runs have different natural widths.

### Line height

`LINE_HEIGHT_FACTOR = 1.2`, hard-coded. Matches typographic convention. Surface as a knob if needed — single field on the IR.

### Coverage gaps

- **RTL / Indic shaping.** cosmic-text supports them via rustybuzz, but Inter Regular has no RTL coverage. Custom font required; not promised to work end-to-end at correct visual fidelity.
- **Bidi.** Same issue.
- **Color emoji.** swash returns monochrome outlines.
- **Variable fonts axes.** Specifying non-Regular weight when only Regular is registered triggers cosmic-text's nearest-match fallback (may synthesize artificial bold).

### Cache discipline

Same shape as Tex. Cache key is `(src, font, weight, size, color, align)` — only inputs that shape the output. Per-`Evaluator`, dies with the Evaluator. Mutex+Arc, double-checked under lock for cold-miss races. ADR 0009 explicitly carves out future glyph caches as legitimate source-keyed in-memory caches.

## ffmpeg encoder

**Status:** Slice B port.
**ManimGL reference:** `manimlib/scene/scene_file_writer.py:202-230` (`open_movie_pipe`) @ `c5e23d9`.

ManimGL spawns ffmpeg as a subprocess and pipes raw RGBA frames into stdin. Manimax does the same with a tighter command and no temp-file dance.

```
ffmpeg -y \
  -f rawvideo -s WxH -pix_fmt rgba -r FPS -i - \
  -an -loglevel error \
  -vcodec libx264 -pix_fmt yuv420p \
  <output>
```

### Diffs from manimgl, with reasons

| Change | Reason |
|---|---|
| **Drop `-vf vflip`** | wgpu readback is top-down (row 0 = top). ffmpeg's `rawvideo` default is top-down. Manimgl needs `vflip` because OpenGL FBO readback is bottom-up. |
| **Drop `-vf eq=saturation=S:gamma=G`** | Manimgl uses these to compensate for OpenGL color quirks. We render to `Rgba8UnormSrgb`, which gamma-encodes on write. |
| **Drop temp-file-then-rename** | Manimgl's pattern guards against truncated mp4s on crash. Our `Drop` impl kills ffmpeg on panic; re-rendering from IR is cheap. |
| **Hardcode `-vcodec libx264 -pix_fmt yuv420p`** | Slice B accepts no codec flags. Add `--codec` later if needed. |
| **Drop per-animation partial movie files** | Manimgl needs them because its replay model has no cheap way to render frames *M..N*. Random-access `eval_at` makes this unnecessary. |
| **Kill child on `Drop`** | Manimgl's Python process exit cleanly terminates stdin → ffmpeg follows. Rust's panic semantics don't guarantee that. |

Audio pipelines, progress display, format switches, and section concatenation are not ported.

## Scene discovery

**ManimGL reference:** `manimlib/extract_scene.py` @ `c5e23d9`.
**Manimax port:** `python/manim_rs/discovery.py`; wired from `python/manim_rs/cli.py`.

```
python -m manim_rs render MODULE SCENE OUT [--quality | -r WxH] [--duration SEC] [--fps N] [-o]
```

`MODULE` is a `.py` path or dotted module name; `SCENE` is the class name (a `Scene` subclass).

### Kept from manimgl

- Class-name positional argument.
- `issubclass(obj, Scene)` gate.
- Synthetic module names (`_manim_rs_scene_<stem>_<hash>`) to avoid `sys.modules` collisions.

### Dropped from manimgl

- **`--write_all`** — render every scene in the file. Slice C is one-scene-per-invocation; add back when a consumer needs batch export.
- **Interactive prompt** for ambiguous class names. Hostile in agentic pipelines and annoying to test. `SceneNotFoundError` carries an `available:` hint instead.
- **`compute_total_frames` pre-run.** Manimgl runs the scene twice — once with `skip_animations=True` for a progress bar, once for real. The IR makes this unnecessary: `scene.ir` reports timeline length directly.
- **`insert_embed_line_to_module`** — manimgl rewrites the user file to inject `self.embed()` for its interactive shell. Manimax is offline-only.
- **`__module__.startswith(module.__name__)` filter** — `issubclass` + name-match is enough since we resolve exactly one name per call.

### Edge cases

- Module exec-time errors bubble out as `ModuleLoadError` with the original exception chained. The synthetic `sys.modules` entry is rolled back so a retry gets a clean slate.
- Same path resolved twice gets the same synthetic name (deterministic hash).
- Scene class defined in a sibling module via `from .scenes import MyScene` works — `find_scene_class` does attribute lookup on the loaded module. It does **not** walk imports.
