# 0008 — Slice E consolidated decisions

**Date:** 2026-04-28
**Status:** accepted (Steps 1–5 shipped; Steps 6–9 remaining)

One ADR for the design calls that defined Slice E (text + math).
Continues the consolidated-per-slice cadence from 0004 / 0006. The
RaTeX-vs-alternatives choice from §2 of the slice plan is folded in
here rather than getting its own number — there were no surprises
worth a separate record once `tex_to_display_list` worked.

---

## A. Tex fan-out happens at eval time, not at IR-emission time

### Decision
`Object::Tex { src, color, scale, macros }` stays a single IR node
through encode/decode. `Evaluator::eval_at` is what expands one
`Tex` into N `Object::BezPath` `ObjectState`s — by calling a cached
`compile_tex` and pushing one fan-out child per glyph, with the
`ObjectState`'s `scale` field carrying `parent_scale * tex_scale`.
The raster crate never sees a `Tex` node:
`Object::Tex { .. } => unreachable!(...)` in `render_object.rs`.

### Why
- IR stays small and diffable. The wire payload is a 30-character
  source string, not a kilobyte of pre-tessellated paths.
- Cache key stability: hashing the *evaluated* `SceneState` (Slice D §D)
  composes cleanly because the eval-time fan-out is deterministic.
  Two scenes with identical Tex sources hash identically without
  re-running RaTeX layout.
- Per-Evaluator cache (blake3 of `serde_json(Object::Tex)`) means
  repeated frames inside one render share one compile, and warm
  cache hits skip RaTeX entirely.

### Consequences
- `compile_tex` returns `Vec<Arc<Object>>` and the eval loop
  multiplies-out the parent's transform. Position / opacity /
  rotation / color override come from the parent `ObjectState` and
  apply uniformly to every glyph.
- Raster code asserts the fan-out happened. A future caller that
  bypasses `Evaluator::eval_at` and feeds raw `SceneState` into
  raster will panic with a clear message.
- The IR's `scale` field is **not baked into the geometry**. That
  was a deliberate revert mid-Step-4 — pre-baking double-applied
  scale once the parent transform also carried it.

### Rejected alternatives
- **Compile-on-decode (Python builds the BezPaths).** Forces
  RaTeX onto Python's critical path or duplicates the parser.
  Loses zero-Python-deps story.
- **Compile-on-encode (Rust pre-tessellates inside `to_ir`).**
  Bloats the wire payload; loses cache-key compactness.
- **Treat `Tex` as a renderable in raster.** Couples the raster
  crate to `manim-rs-tex`, blocks the future "skip RaTeX entirely
  on cache hit" path, and tangles per-glyph color resolution with
  per-object color override.

---

## B. Per-Evaluator Tex cache, blake3 over serde_json bytes

### Decision
`Evaluator` carries `tex_cache: Arc<Mutex<HashMap<blake3::Hash,
Arc<Vec<Arc<Object>>>>>>`. Key is `blake3::hash(&
serde_json::to_vec(object))` over the whole `Object::Tex` node
(src + macros + color + scale). `compile_tex_cached` does the
classic check-under-read-lock → compile → re-check-under-write-lock
dance.

### Why
- Mirrors Slice D §D's cache-key shape — same hash function, same
  serializer, same determinism guarantees. One mental model.
- Hashing the entire `Object::Tex` (not just `src`) means future
  fields automatically participate. No "did I forget to add this
  to the key" trap.
- Per-Evaluator (not global) avoids cross-render lock contention
  and keeps test isolation cheap.

### Consequences
- Two Tex calls with identical effective LaTeX but different
  `macros` maps would miss each other in cache. That's why Step 5
  pre-expands macros Python-side and emits `macros={}`. The IR
  field still exists for forward compatibility but is never set
  by the public Python surface today.
- Memory grows unbounded across one `Evaluator`'s lifetime. Fine
  in practice (Tex outputs are ~KB; one render uses a handful);
  the cache dies with the Evaluator.

### Rejected alternatives
- **Hash `src` only.** Would silently share entries across
  different `color` / `scale`. We don't bake scale into geometry,
  but color resolution *is* baked into the fan-out children, so
  hashing color matters.
- **Process-global static cache.** Buys nothing here; renders are
  short-lived and the per-Evaluator cache already collapses
  in-render duplicates.

---

## C. Glyph outlines extracted at high internal ppem (1024), then scaled

### Decision
`manim-rs-text/src/glyph.rs` always asks `swash` for outlines at
`OUTLINE_PPEM = 1024` and post-multiplies by an `Affine::scale(
scale / 1024)` to land at the caller's requested ppem.

### Why
- swash applies hinting at low ppem. Manimax uses "1 em = 1 world
  unit," which means the natural ppem is ~1.0 — small enough that
  hinting snaps every control point onto an integer grid. The
  resulting outline is technically correct at ppem≈1 but produces
  visible staircase scallops once `Tex.scale` (or the camera) blows
  it up to display size.
- Extracting at 1024 ppem is effectively hinting-off. Downstream
  scale (Tex.scale, MVP) composes into one smooth affine and the
  rendered glyph stays curve-clean at any zoom.

### Consequences
- All glyph outlines pass through one extra `apply_affine` post-
  process. Negligible cost compared to tessellation.
- The constant is documented in-source — future readers shouldn't
  "simplify" by passing the caller's `scale` straight to
  `scaler.size(scale)`.

### Rejected alternatives
- **Disable hinting via swash's hint flag.** swash 0.2 doesn't
  expose a stable knob for this; the safe path is to render at a
  ppem high enough that hinting is a no-op.
- **Render at exactly the final display ppem.** Couples outline
  extraction to camera/display state — re-introduces hinting when
  the user is just previewing at low res.

---

## D. Lyon fill flatness tolerance pinned at 0.001 (was `FillOptions::DEFAULT`)

### Decision
`crates/manim-rs-raster/src/tessellator.rs` uses
`FillOptions::tolerance(FILL_TOLERANCE).with_fill_rule(NonZero)`
with `FILL_TOLERANCE = 0.001`.

### Why
- `FillOptions::DEFAULT.tolerance == 0.25` is calibrated for SVGs
  authored in *pixel* units. Glyph outlines arrive in *em*-scaled
  world units where a single em is ~1 unit; 0.25 is a quarter-em
  flatness budget, which is enough to flatten an `o` into a
  stop-sign octagon.
- 0.001 (1‰ of an em) is empirically smooth at all reasonable
  display scales tested in Slice E (scale=0.25 to scale=8).

### Consequences
- Tessellation cost scales with `1 / sqrt(tolerance)`. Going from
  0.25 → 0.001 is ~16× more curve segments per glyph. Hasn't
  shown up in profiling yet (text scenes are tens of glyphs, not
  thousands), but logged in `docs/performance.md` as a future
  per-Object knob.

### Rejected alternatives
- **Per-Object tolerance in IR.** Right answer eventually; not
  worth the schema growth before someone hits a perf wall.
- **Adaptive tolerance from object world-space scale.** Plausible
  optimization later; needs camera state inside the tessellator.

---

## E. Python-side macro pre-expansion; IR ships `macros={}`

### Decision
`python/manim_rs/objects/tex.py::_expand_macros` runs a regex
substitution pass (`\\([A-Za-z]+)` for control words, with a
look-after check to enforce TeX's word-boundary rule) and iterates
to a fixed point with a 50-pass cap. The IR's `macros` field is
always emitted as `{}`. Construction validates the fully-expanded
source via a new `_rust.tex_validate` pyo3 entry point.

### Why
- RaTeX doesn't expose macro definition; pre-expansion is the only
  realistic path that doesn't fork the parser.
- Emitting `macros={}` keeps cache keys stable across Python dict
  reordering and across "same effective source, different macros
  map" call sites.
- Validating at construction matches Python authoring norms — typos
  surface where the code lives, not at render time on a different
  machine.

### Consequences
- Argument macros (`\norm{x}`) remain unsupported. Documented in
  `docs/tex-coverage.md` (Step 9). Escalation path is vendor-and-
  patch `ratex-parser`.
- `tex_validate` re-runs parse+layout once at construction and
  again at compile. Cheap — RaTeX layout for short expressions is
  sub-millisecond. Logged in `docs/performance.md` as a candidate
  to cache via `Tex.__init__` storing the parsed DisplayList if it
  ever shows up in a profile.

### Rejected alternatives
- **Vendor and patch ratex-parser to accept `\newcommand`.** Real
  cost; deferred until a user hits the limitation.
- **Pass `macros` through the IR untouched, expand Rust-side.**
  Two implementations of the same logic; loses cache stability.

---

## F. Single-frame render API at both Rust and Python surfaces

### Decision
Mid-slice addition (between Step 5 and Step 6): expose
`render_frame_to_png(scene, out, t)` from `manim-rs-runtime`,
`render_frame(ir, out, t)` from pyo3, and a `frame` typer
subcommand from the CLI. The frame path bypasses ffmpeg entirely
and writes RGBA8 → PNG via the `png` crate.

### Why
- Visual debugging during Step 5 needed "render this single
  timestamp at this resolution and let me look at it." mp4 + scrub
  in QuickTime is the long way around when the question is "is
  glyph quality OK at scale=8."
- The Rust surface was already evaluating one frame's pixels for
  the snapshot tests; exposing it as a public API was a small
  delta — one new entry point, no encoder changes.

### Consequences
- We now have **two format-specific rendering functions**
  (`render_to_mp4`, `render_frame_to_png`). This is fine for now
  but the right shape is a single general-purpose,
  format-agnostic render-N-frames function with a sink trait
  covering MP4 / PNG / WebM / GIF / APNG / image sequences /
  raw RGBA / in-memory bytes / future formats — one entry
  point, output format chosen by the sink. Flagged as future
  work in `STATUS.md` and `docs/performance.md`. Don't grow a
  third format-specific entry point before consolidating.
- `RuntimeError` grew `Png` and `Io` variants. Same shape as the
  existing `Encoder` variant, no surprise.

### Rejected alternatives
- **Reuse `render_to_mp4` with `--duration 1/fps`.** Forces ffmpeg
  on the path for debugging single frames; the encoder is the
  least-deterministic, slowest part of the pipeline.
- **Hold off and build the general-purpose API now.** Out-of-scope
  scope creep mid-Step-5; bigger design that deserves its own
  pass. Recorded as future work instead.
