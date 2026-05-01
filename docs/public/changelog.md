# Changelog

What shipped, slice by slice. Each entry captures what surprised us, what the plan got wrong, and the calls that ended up mattering.

## Slice E — text + math (2026-04-29)

Tex via RaTeX (KaTeX-grammar subset, pure Rust) and Text via cosmic-text + swash. Real strokes, real fills, real glyphs. ADRs 0008, 0012; porting notes for Tex and Text.

### Steps 1–3 (font plumbing, RaTeX adapter, Tex IR + eval)

Mostly tracked the plan. RaTeX's `DisplayList` was as advertised — adapter sized within the predicted 250–400 LOC. Coordinate convention (y-down, em-units) was the only real friction; one-time fix in the adapter, no leakage.

### Step 4 — Tex fan-out

- **Fan-out site moved to `Evaluator::eval_at`.** The plan assumed `compile_tex` would produce a "Tex object with internal BezPaths." Implementation revealed the cleaner shape is fanning out into N separate `ObjectState`s (one per glyph), so the raster layer never sees a Tex node. ADR 0008 §A.
- **Scale double-application.** First implementation baked `Tex.scale` into the BezPath geometry inside `compile_tex` *and* carried it on the `ObjectState`. Anything with `scale != 1.0` rendered twice as scaled. Fix: don't bake — multiply parent and Tex scale at the fan-out site.
- **Per-Evaluator Tex cache.** Not in the plan but obviously right once the eval-time fan-out shape was clear. Mirrors Slice D's hash discipline. ADR 0008 §B.

### Mid-Step-5 detour — single-frame render API

Not in the plan. Triggered by visual debugging: "render this one timestamp at this resolution and look at the pixels." Resulting shape: `render_frame_to_png` + `render_frame` pyo3 entry + `frame` typer subcommand. ADR 0008 §F. Cost ~1 hour; saved many hours of "render mp4, scrub, squint." The Step 5 visual bugs below were unspottable without it.

### Step 5 — two distinct visual quality bugs

1. **Lyon fill flatness too coarse.** `FillOptions::DEFAULT.tolerance = 0.25` is calibrated for SVG-style geometry. Glyph outlines arrive in em-units where 1 em ≈ 1 world unit, so 0.25 flattens curves into octagons. Fix: pin `FILL_TOLERANCE = 0.001`. ADR 0008 §D.
2. **Swash hinting at low ppem.** First fix improved 1080p but scale=8 zooms still showed staircase scallops. Root cause: "1 em = 1 world unit" → ppem ≈ 1, where TrueType hinting snaps every control point to the integer pixel grid. Fix: extract at `OUTLINE_PPEM = 1024` and post-multiply by `Affine::scale(scale / 1024)`. ADR 0008 §C.

The hinting bug took two passes. First diagnosis was "y-flip is inverting contour winding and breaking NonZero fill" — would have been a real bug had it been true. **Lesson:** when a visual artifact's mechanism isn't obvious, render *into the actual intermediate stage* (BezPath dump, single-glyph snapshot at the real ppem) before guessing.

### Post-Step-5 cleanup pass

Eight surgical fixes from a `/simplify` review. Recurring shape: **"feature added quickly, guarded by a comment instead of a type/structure."** The comments were locally plausible; bugs surfaced only when something bypassed the documented contract (a direct Rust caller, a cold-miss race, a parallel Python thread). For future slice work: when writing "this can't happen because the caller upstream validates," prefer making it structurally true (panic, narrower type, `unreachable!`) over hoping callers obey.

Notable:
- `compile_tex` swallowed `TexError` → silent blank render. Tightened to `Result<Vec<Object>, TexError>`.
- Tex cache key included `scale` and `macros` despite neither shaping geometry. Tightened to hash `(src, color)` only.
- Font cache had a leak-then-lock race — concurrent cold misses both `Box::leak`'d before either acquired the write lock.
- `tex_validate` held the GIL during RaTeX parse+layout. Pattern was established in Slice C but not applied at introduction.

### Step 6 — corpus shipped, harness deferred

Plan called for a full snapshot harness parametrized over `tex_corpus.py` with a pinned `TEX_SNAPSHOT_TOLERANCE`. What shipped: the corpus data and the coverage doc. Harness deferred — picking a cross-platform tolerance requires CI runs against actual lavapipe, which became its own scope. **Lesson:** "build a corpus" and "build a corpus harness" are two steps, not one.

### Step 7 — Text

- Tracked the plan tightly through S7a → S7e. Sub-steps were added during execution (font plumbing, cosmic-text adapter + IR, eval fan-out + cache, Python constructor, end-to-end render). Mirrored the Tex sequence.
- **No Python `_rust.text_validate`.** cosmic-text accepts any UTF-8; LaTeX has parse failures worth surfacing at construction, UTF-8 strings don't.
- **Stale `_rust` extension** caught S7e — `cargo test` was green but doesn't rebuild the pyo3 extension; only `maturin develop` does.

### Step 8 — determinism + cache probe

- **Cache probe via `Arc::ptr_eq`**, not pyo3 counters. The cleanest probe is pointer-identity on fan-out children of two Tex/Text instances sharing a source. No pyo3 surface change, no test-only feature flag. **Lesson:** when a test wants to assert "X and Y refer to the same compiled object," prefer pointer-identity over counters.
- **Determinism is real and clean.** Three byte-determinism tests cover Tex, Text, and a combined Tex+Text+Polyline scene. The eval (HashMap iteration over `BTreeMap`-ordered IR), cosmic-text shaping, swash outlines, lyon tessellation, and in-process libx264 are all deterministic.

## Slice D — strokes + cache (2026-04-23)

Real port of `quadratic_bezier/stroke/*.glsl` to WGSL (geometry shader → CPU-side ribbon expansion + analytic SDF AA). Per-`Evaluator` snapshot cache. ADR 0006.

### Cache key shape was wrong in the plan

Plan locked `blake3(scene_ir ‖ frame_idx ‖ w ‖ h)`. Shipped `blake3(version, metadata, camera, SceneState@t)` — hashing the evaluated state per frame, not the raw scene + index. The planned scheme would have invalidated every frame on any scene edit, defeating the cache.

Caught while writing `local_track_edit_invalidates_only_affected_frames` — expected 3 hits / 3 misses, got 5/1, because the cache turned out to be content-addressed in a useful way we hadn't planned for. **Lesson:** write the locality test *before* pinning the key shape; the test forces you to confront what "locality" actually means.

### "Cold run = every frame misses" is false

Corollary of the above. The content-addressed key means frames sharing a `SceneState` (e.g. a static prefix) collapse into one cache entry. Python integration test initially asserted `misses == TOTAL_FRAMES`; actual was 5/9. Corrected to `misses == unique_frame_states`.

### `FillUniforms = StrokeUniforms` alias couldn't just "drop"

Plan said "accept the duplication." Reality: the alias was still in place when `StrokeUniforms` grew `{ anti_alias_width, pixel_size }` and renamed `color` to `params`, silently breaking the fill shader. Split into two separate structs. **Lesson:** kill aliases the moment the structs diverge semantically, not when they diverge syntactically.

### Test-shape lessons

- **Diagonal-stroke AA test needed a diagonal.** Initial horizontal line gave either 247 or 255 — no intermediate values. The smoothstep fade zone was sub-pixel on an axis-aligned line. Switched to `(-3,-2)→(3,2)` diagonal so MSAA sub-pixel coverage produces a broad fade band.
- **Tapered-stroke test checked alpha on opaque-black background.** Every row counted. Switched to R channel.

### Misc

- Plan's `--no-cache` CLI flag deferred in favor of Python-level `cache_dir=` parameter that the integration test needed for isolation.
- ADR number collided — plan said `0005-slice-d-decisions.md`; `0005-plain-ir-compiled-evaluator.md` already existed. Shipped as `0006`. Check `ls ../design/` before pinning a number.

## Slice C — multi-object, MSAA, Bézier paths (2026-04-22)

`BezPath` as the unified primitive, MSAA on the color target, fill via lyon, tolerance-based snapshots, scene discovery, six animation transforms. ADR 0004.

### CLI shape diverged from plan

Plan §1 wrote `--scene FILE [CLASS]`. Shipped fully positional: `render MODULE SCENE OUT [opts]`. The positional form is what tests and authored scenes use; the flag form would have added a redundant lever.

### Python authoring API filled in during Step 7, not Step 5

Plan's Step 5 → 7 ordering was right in principle, but the authoring API landed as the work to make Step 7 possible. **Lesson:** for future slices, don't split "expose to Python" from "use from Python in a test" — collapse them.

### Surprising calls that landed

- **`FillUniforms = StrokeUniforms` type alias.** Two pipelines' uniform buffers are `{ mat4x4 mvp, vec4 color }`. Aliased instead of duplicating. (Bit us in Slice D — see above.)
- **Tolerance-based snapshots were right the first time.** ADR 0004 §E. MSAA broke the Slice B exact pins on the first render, as predicted. Pre-solving paid off.
- **`BezPath` as the unified primitive immediately justified itself.** Five Python factories (`Circle`, `Rectangle`, `Line`, `Arc`, `Polygon`) land as <50 lines each; no IR schema churn.
- **`Colorize` requires explicit `from_color`.** Color-track's "last-write-override" semantics don't infer the starting color. Matches the position track (explicit `delta`); keeps the evaluator free of object-state reads.

### Gotchas the plan missed

- **`pythonize` returns tuples for fixed-size arrays.** `[f32; 3]` → Python `tuple`. Tests comparing against `[0.0, 0.0, 0.0]` fail on type, not value.
- **f32 round-trip precision on parameterised easings.** `ThereAndBackWithPause(pause_ratio=1/3)` round-trips to a different f64 bit pattern; tests must use dyadic rationals.
- **H.264/yuv420p chroma shift on solid fills.** `(0, 229, 51)` decodes as approximately `(0, 240, 120)`. Per-object centroid was the right test shape.
- **lyon dedupes sub-epsilon stroke points.** Caught during `GeometryOverflow` calibration; forced a zigzag fixture.

## Slice B — first end-to-end render (2026-04-21)

Python → IR → Rust eval → wgpu raster → ffmpeg mp4. Single hardcoded scene. ADRs 0001, 0002, 0003.

### Plan got wrong

- **Step 8 said `py.detach(...)`.** Wrong for pyo3 0.23 — `detach` is a later-version rename. Actually used `py.allow_threads(|| ...)`. **Lesson:** don't cite symbol names for a pinned dep without grepping the installed source.
- **Step 9 omitted typer's single-subcommand flattening.** `python -m manim_rs render out.mp4 ...` would have rendered `out.mp4` as the first positional arg to the top-level command, silently broken. Fix: add a no-op `@app.callback()`.

### Surprising calls that landed

- **Match-manimgl-over-correct** — made twice (sRGB floats, Vec3 coordinates) under user pushback. Codified in ADR 0003.
- **Internally-tagged unions** with `"op"` / `"kind"` discriminators — symmetric between serde and msgspec, human-readable in dumps. ADR 0002.
- **JSON string over FFI.** Initially felt too loose for a typed contract; in practice the debuggability + minimal FFI surface won. ADR 0001.
- **Evaluator purity paid off immediately.** Zero shared state across `eval_at` calls made the frame loop a 10-line driver.

### The bug §5 didn't catch

**wgpu `queue.write_buffer` ordering.** The slice shipped with a multi-object render regression no test caught. The raster loop reused one vertex/index/uniform buffer across N passes in a single submit; writes are ordered before any submitted command buffer, so every pass drew the last object. Discovered post-slice when rendering a two-object proof scene and finding only one shape visible.

Fix: submit per object. Regression test: `crates/manim-rs-raster/tests/multi_object.rs`. **Lesson: §5 success criteria covered "the hardcoded single-object scene renders" — they did not cover "multi-object scenes render." Future slices' success criteria must include at least one case that exercises the polymorphism of the IR, not just the demo shape.**

### Process observations

- **One-step-at-a-time cadence worked.** Explain → confirm → implement → update `STATUS.md` → repeat. Zero rework.
- **Porting notes written in-slice** caught invariants we'd have otherwise had to re-derive.
- **Retroactive ADRs would have been cheaper written in-slice.** Three of them landed only after Slice B was done; each was worth ~10 minutes of writing when it was being made.
