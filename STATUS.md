# Status

**Last updated:** 2026-04-30
**Current slice:** Slice E **complete** (Steps 1–9 shipped; Step 6
snapshot harness explicitly deferred — see below).
**Next slice:** unscoped. Natural candidates: SVG import (E.5),
Slice F (3D pipeline), or the Tex snapshot harness.

Branch (`chcardoz/vancouver-v1`) carries Slice E since `main`'s
`99e1492` (perf + hardware-encoder push). Most recent commits:

- `fe6e23d` Step 8: examples + e2e Tex+Text test + cache probe
- `079a428` Step 7 S7d/S7e: Python `Text(...)` + e2e render
- `0bcba9b` Step 7 S7c: eval-time Text fan-out + cache
- `93f04ab` Step 7 S7a/S7b: `Object::Text` IR variant + cosmic-text adapter
- `eeeddbf` Step 6: corpus + tex-coverage doc

Step 9 (this commit) lands ADR 0012, the ADR 0008 addendum, the
two new porting notes, and the performance/gotchas/coverage
appends. No code changes — docs only.

## Last session did

**Step 8 (e2e Tex+Text + determinism + cache probe).**
- `examples/{text_scene,tex_scene}.py` for the §1 acceptance commands.
- `tests/python/test_e2e_text_tex.py`: ffprobe metadata checks,
  combined Tex+Text+Polyline scene, byte-identical determinism for
  all three (TextScene, TexScene, combined).
- Cache-hit probe via `Arc::ptr_eq` integration tests on
  duplicate sources in `crates/manim-rs-eval/src/lib.rs`. pyo3
  surface untouched.
- All 136 Python tests + 30 Rust test groups green.

**Step 9 (Slice E docs).**
- `docs/decisions/0012-text-via-cosmic-text-swash.md` — Text ADR
  (cosmic-text + swash + bundled Inter Regular, why over
  rusttype/ab_glyph/fontdue/Pango).
- `docs/decisions/0008-slice-e-decisions.md` §G addendum — RaTeX
  bus-factor mitigation, upgrade triggers, `\newcommand` deferral.
- `docs/porting-notes/tex.md` (new) — RaTeX/manimgl invariants,
  `\textcolor` semantics, coordinate flip, `compile_tex` cache
  discipline, the two visual-bug fixes from Step 5.
- `docs/porting-notes/text.md` (new) — cosmic-text contract,
  alignment semantics, line-height, baseline anchoring,
  RTL/Indic/emoji gaps, cache discipline.
- `docs/performance.md` E4–E7 — wheel-size delta, RaTeX cost (vs.
  raster: invisible), cache hit rates, determinism canary.
- `docs/gotchas.md` — three new entries: stale `_rust` extension
  (cargo test alone doesn't rebuild it), cosmic-text font db
  init / system scan trap, Tex snapshot tolerance cross-platform
  skew.
- `docs/tex-coverage.md` — preamble cross-links to ADR 0008 §C
  (high-ppem outlines) and §D (lyon `FILL_TOLERANCE = 0.001`).
- `docs/slices/slice-e.md` — §5 marks corpus-snapshot success
  criterion as deferred; §11 retrospective filled for Steps 6–9.

## Slice E exit summary

**Shipped:**

- Both §1 acceptance commands work end-to-end. `Text("...")` and
  `Tex(r"...")` render correctly with bundled fonts; no system
  LaTeX or system font required.
- Tex coverage subset documented in `docs/tex-coverage.md` (33
  expressions in `tests/python/tex_corpus.py`).
- Per-`Evaluator` source-keyed caches for Tex and Text geometry
  (the future glyph caches ADR 0009 explicitly carved out).
- `compile_tex` and `compile_text` measurably hit on duplicate
  sources (Step 8 `Arc::ptr_eq` probes).
- Byte-determinism across re-renders (Step 8).
- ADRs 0008 (consolidated, with §G addendum), 0009 (pixel-cache
  removal, mid-slice), 0010/0011 (encoder push, off-slice but
  driven by Slice E traces), 0012 (Text).

**Explicitly deferred:**

- **Tex snapshot harness** (Step 6 follow-up). The corpus data
  shipped, but the parametrized snapshot test, baseline PNGs,
  `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE`
  did not. Picking a cross-platform tolerance requires running
  the corpus through Linux/lavapipe CI (ADR 0007), which is its
  own scope. Track in `docs/tex-coverage.md` "Snapshot tolerance"
  and as a candidate next-slice or Slice E.5 task.
- **Baseline-PNG version of S7e's Text render test.** Same
  blocker (no harness yet). Today's centroid-based check in
  `tests/python/test_text.py::test_text_renders_visible_pixels_at_origin`
  is the placeholder; when the harness lands, replace with a
  PNG-baseline + tolerance comparison.

**Not in scope, never were:**

- Tectonic / system LaTeX / `engine="latex"` opt-in.
- `\newcommand` with arguments.
- 3D text, animated per-glyph effects, `TransformMatchingTex`.
- SVG import (`SVGMobject`).
- RTL / Indic / color-emoji shaping.
- Multi-weight bundled Text faces.

## Next action

Pick the next slice. Options:

- **Tex snapshot harness as a mini-slice.** Picks up the deferred
  Step 6 work. Build `tests/python/test_tex_corpus.py`
  parametrized over `CORPUS`, `--update-snapshots` flag, baselines
  in `tests/python/snapshots/tex/`, pin `TEX_SNAPSHOT_TOLERANCE`
  cross-platform. Closes the success criterion that Slice E
  punted.
- **Slice E.5 — SVG import.** Adjacent to glyph outlines (path-
  based mobjects, same fill pipeline). Small-but-real scope.
  Unblocks rendering manimgl-style SVGMobjects.
- **Slice F — 3D pipeline.** Surface pipeline, depth buffer,
  phi/theta camera; reintroduces `flat_stroke` + `unit_normal`
  from the deferred Slice D §4 carry-over. 3D text becomes
  possible after this lands.

CLAUDE.md's "Read before you touch anything" list is the right
on-ramp; Slice E touches all of `architecture.md`,
`decisions/`, `slices/`, `gotchas.md`, `porting-notes/`,
`performance.md`. New slice should write a `docs/slices/<name>.md`
plan first.

## Blockers

- None.
