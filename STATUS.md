# Status

**Last updated:** 2026-04-30
**Current slice:** Slice E.
**Step 6:** corpus + coverage doc landed; **harness deferred**.
**Step 7:** S7a (font plumbing reused) + S7b (cosmic-text adapter +
`Object::Text` IR variant) landed; S7c–S7e remaining.

Branch (`chcardoz/vancouver-v1`) carries everything since `main`'s
`99e1492` (perf + hardware-encoder) — three new commits today, none
PR'd yet:

- `eeeddbf` Step 6 corpus + tex-coverage doc.
- `93f04ab` Step 7 S7a/S7b: `Object::Text` IR variant + cosmic-text
  adapter.
- (this commit) STATUS + slice-e plan refresh.

`docs/slices/slice-e.md` Steps 6–9 were already rewritten to absorb
ADR 0009 (pixel cache removed) + ADR 0010/0011 (encoder push) and
renumber the planned Text ADR to `0012-text-via-cosmic-text-swash.md`.

## Last session did

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

Resume **Slice E Step 7 S7c–S7e**:

- **S7c — eval-time fan-out.** `crates/manim-rs-eval`: when
  `Evaluator::eval_at` encounters an `Object::Text`, call
  `text_to_bezpaths`, recolor to the IR's `color`, and emit one
  per-glyph `ObjectState` (mirrors how Tex fans out today). Add a
  per-`Evaluator` `(src, font, weight, size, color, align) →
  Vec<Object>` cache; `Box::leak`-under-write-lock pattern (per
  Slice E §11 cleanup-pass note); minimal cache key (no per-instance
  transforms in the key — same lesson as Tex's post-Step-5 cleanup).
- **S7d — pyo3 surface + Python `Text(...)` constructor.**
  `python/manim_rs/objects/text.py` mirroring `objects/tex.py`'s
  shape. GIL discipline: copy `&str` to `String` while holding the
  GIL, then `py.allow_threads` for shape+layout+outline (`tex_validate`
  cleanup-pass pattern).
- **S7e — end-to-end Text snapshot.** `tests/python/test_text.py`:
  `Text("Hello")` round-trips, renders to mp4, single-frame
  tolerance-snapshot against a checked-in baseline PNG using the
  same `TEX_SNAPSHOT_TOLERANCE` from Step 6 (still TBD; coordinate
  with the Step 6 harness).

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
