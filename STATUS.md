# Status

**Last updated:** 2026-04-30
**Current slice:** Slice E.
**Step 6:** corpus + coverage doc landed; **harness deferred**.
**Step 7 complete (S7a–S7e).** S7a font plumbing, S7b cosmic-text
adapter + IR variant, S7c eval fan-out + cache, S7d Python
constructor, **S7e end-to-end render** all landed. Baseline-PNG
snapshot deferred — see "Last session did" for the rationale and
the marker left in `test_text.py`.

Branch (`chcardoz/vancouver-v1`) carries everything since `main`'s
`99e1492` (perf + hardware-encoder) — four new commits, none PR'd yet:

- `eeeddbf` Step 6 corpus + tex-coverage doc.
- `93f04ab` Step 7 S7a/S7b: `Object::Text` IR variant + cosmic-text
  adapter.
- (prior commit) STATUS + slice-e plan refresh.
- (prior commit) Step 7 S7c: eval-time Text fan-out + cache.
- (prior commit) Step 7 S7d: Python `Text(...)` constructor.
- (this commit) Step 7 S7e: end-to-end Text render test.

`docs/slices/slice-e.md` Steps 6–9 were already rewritten to absorb
ADR 0009 (pixel cache removed) + ADR 0010/0011 (encoder push) and
renumber the planned Text ADR to `0012-text-via-cosmic-text-swash.md`.

## Last session did

**Step 7 S7e (end-to-end Text render).**
- `tests/python/test_text.py::test_text_renders_visible_pixels_at_origin`:
  builds a 480×270 Scene, adds `Text("HI", size=0.6)`, renders to
  mp4, decodes frame 0 with ffmpeg, masks bright pixels with numpy,
  asserts (a) >50 lit pixels (rules out blank frame / single-glyph
  clip), (b) <5000 lit pixels (rules out runaway shape), (c)
  centroid right of canvas center (proves left-aligned text extends
  rightward from origin), (d) centroid above baseline pixel y=135
  (proves ascenders sit above world y=0 with no descenders in "HI").
- Mirrors the pattern from `test_render_to_mp4_frame0_has_content_at_origin`.
- **Baseline-PNG snapshot deferred** — Step 6's harness is still
  TBD, so a tolerance-snapshot against a checked-in PNG isn't yet
  possible. Module docstring in `test_text.py` flags this as a
  follow-up; when Step 6 lands the PNG-baseline test should sit
  alongside (or replace) the centroid check.
- **Bug surfaced and fixed** during this work: I forgot to run
  `maturin develop` after S7c's Rust changes — the first run of
  this test panicked with `unreachable: Object::Text must be
  expanded by Evaluator::eval_at` because the loaded `_rust`
  extension was the pre-S7c build. Worth flagging for `gotchas.md`
  if a future agent hits it (cargo test alone doesn't rebuild the
  extension).

Verification: `pytest tests/python` green (130 tests, 1 new in
`test_text.py`); `cargo test --workspace` still green.

**Step 7 S7d (Python `Text(...)` constructor).**
- `python/manim_rs/objects/text.py`: `Text` class mirroring `Tex`'s
  shape — keyword-only args (`font`, `weight`, `size`, `color`,
  `align`), `to_ir()` returns `ir.Text`. Validation: empty `src`,
  unknown `weight`/`align`, non-positive or non-finite `size` all
  raise `ValueError`. No `_rust.text_validate` parallel — cosmic-text
  accepts any UTF-8, so there's nothing to validate beyond argument
  shape (Tex's validator catches LaTeX parse errors; Text has no
  equivalent failure mode at construction time).
- Re-exports updated: `python/manim_rs/objects/__init__.py` and
  `python/manim_rs/__init__.py` now expose `Text` at the top level.
- `tests/python/test_text.py`: 17 tests — defaults, custom values,
  parametrized over each weight + each align, every validation error
  path, color/size coercion, `_id` starts unbound.
- pyo3 surface intentionally untouched. The pre-existing `ir.Text`
  msgspec struct (S7b) handles wire-format; the constructor is a thin
  Python class.

Verification: `pytest tests/python` green (129 tests, 17 new in
`test_text.py`); `cargo test --workspace` still green.

**Step 7 S7c (Text eval fan-out + cache).**
- `crates/manim-rs-eval/src/text.rs`: `compile_text(src, font, weight,
  size, color, align)` wraps `text_to_bezpaths`, recolors all glyph
  paths to the IR's `color` (cosmic-text emits white), and emits
  fill-only `Object::BezPath`s. Mirror of `tex.rs` shape; no
  `\textcolor`-style per-item override yet.
- `crates/manim-rs-eval/src/evaluator.rs`: per-`Evaluator`
  `text_cache: Arc<Mutex<HashMap<blake3::Hash, Arc<Vec<Arc<Object>>>>>>`
  (mirrors `tex_cache` — `RwLock`/`Box::leak` from Slice E §11 was
  aspirational, never shipped for Tex; we kept Tex/Text symmetric on
  the actually-deployed Mutex+Arc pattern). Cache key is
  `(src, font, weight, size, color, align)` only — per-instance
  transforms intentionally absent (same lesson as Tex's post-Step-5
  cleanup). `Object::Text` arm in `Evaluator::eval_at` fans out to
  one `ObjectState` per glyph; track-resolved `scale` passes through
  unchanged (`size` is baked into shaped geometry, no IR `scale`
  field on Text).
- ADR 0009 connection: this is the "future glyph cache" the ADR
  explicitly carved out (§Consequences last bullet) — keyed on shape
  source, in-memory, no I/O. Different failure profile from the
  deleted pixel cache.

Verification: `cargo test --workspace` green (8 new tests in eval —
3 in `text::tests`, 5 in `evaluator` integration tests). Tests cover
fan-out, color landing, scale-track passthrough, cache-hit pointer
equality, and color-distinguishing cache keys.

**Step 6 (data + doc only).**
- `tests/python/tex_corpus.py`: 33 entries, picked by distinct
  rendering machinery (every `DisplayItem` variant, every bundled
  font face, every visual bug Slice E already paid for, plus three
  cross-platform skew probes).
- `docs/tex-coverage.md`: supported KaTeX-grammar subset, "Not
  supported" boundaries with workarounds, manimgl deltas (font /
  spacing / delimiter sizing / accent positioning / color
  semantics), future `engine="latex"` escape hatch.
- **Snapshot harness, baseline PNGs, `--update-snapshots` flag, and
  pinned `TEX_SNAPSHOT_TOLERANCE` are NOT yet shipped** —
  `tex-coverage.md` flags the gap explicitly.

**Step 7 S7a/S7b (Text IR + Rust shaping).**
- `crates/manim-rs-text/src/cosmic.rs`: `text_to_bezpaths` shapes via
  a process-wide `OnceLock<Mutex<FontSystem>>` seeded only with
  bundled Inter Regular (no system-font scan; gotcha §6.7 becomes
  one-time deterministic init). Layout at `SHAPE_PPEM = 1024` to
  dodge low-ppem TrueType hinting (mirrors ADR 0008 §C). First
  line's baseline anchored at world y = 0; descenders below,
  additional lines stack downward.
- `glyph.rs`: `glyph_to_bezpath_by_id` bypasses charmap when
  cosmic-text already resolved the glyph id during shaping.
- `crates/manim-rs-ir`: `Object::Text { src, font, weight, size,
  color, align }` + `TextWeight` / `TextAlign` enums. Mirrors
  `Object::Tex` shape (source-only, no per-instance transforms in
  IR). `SCHEMA_VERSION` 2 → 3.
- `crates/manim-rs-raster::tessellate_object`: `Object::Text =>
  unreachable!(...)`, mirror of the Tex contract (raster never sees
  a raw Text node).
- `python/manim_rs/ir.py`: mirror `Text` msgspec struct +
  `TextWeight` / `TextAlign` Literals (`#[serde(rename_all =
  "lowercase")]` on the Rust side).
- `tests/python/test_ir_roundtrip.py`: Python↔Rust roundtrip
  including a `font: Some("Inter")` variant — wire shape reserved
  for S7c/S7f without a future IR bump.
- `Cargo.toml`: `cosmic-text = "0.19"` workspace dep.

Verification: `cargo test --workspace` green (8 new tests in
`manim-rs-text::cosmic`, 2 new in `manim-rs-ir`); `pytest
tests/python` green (1 new test in `test_ir_roundtrip`).

## Next action

Two parallel tracks open. Pick one before resuming:

**Track A — finish Step 6 harness.** This unblocks the
baseline-PNG version of S7e and pins
`TEX_SNAPSHOT_TOLERANCE`. Build `tests/python/test_tex_corpus.py`
parametrized over `CORPUS`, `--update-snapshots` flag, baselines
checked into `tests/python/snapshots/tex/`, pin a tolerance that
passes on macOS-arm64 dev *and* Linux/lavapipe CI. Document the
value at the bottom of `tex-coverage.md`. Then upgrade the S7e
centroid check to a baseline-PNG comparison.

**Track B — Step 8 (E2E + determinism + cache probe).** End-to-end
scene with both Tex and Text active, determinism check across two
runs, observable evidence the eval-level Tex/Text caches are hit
(repeated frames don't recompile). No new functionality, just
proving Step 7's invariants under load.

**Step 9 (ADR 0012 + porting notes + perf log)** sits after both.
ADR 0012 — `text-via-cosmic-text-swash` — captures the cosmic-text
+ swash + Inter Regular bundling decisions.

Then **Step 6 harness completion**: `tests/python/test_tex_corpus.py`
parametrized over `CORPUS`, `--update-snapshots` flag, baselines
checked into `tests/python/snapshots/tex/`, pin
`TEX_SNAPSHOT_TOLERANCE` that passes on macOS-arm64 dev *and*
Linux/lavapipe CI. Document the value at the bottom of
`tex-coverage.md`.

Step 8 (E2E + determinism + cache probe) and Step 9 (ADR 0012 +
porting notes + perf log) remaining after that.

## Blockers

- None.
