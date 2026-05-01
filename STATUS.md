# Status

**Last updated:** 2026-04-30
**Current slice:** none (Slice E complete; next slice unscoped)

Slice E (text + math) shipped end-to-end on `chcardoz/vancouver-v1` ahead of `main` (`8df2808`). Both §1 acceptance commands work: `Text("...")` and `Tex(r"...")` render correctly with bundled fonts, no system LaTeX or system font required. Per-`Evaluator` source-keyed caches for Tex and Text geometry; `compile_tex` / `compile_text` measurably hit on duplicate sources (Step 8 `Arc::ptr_eq` probes); byte-determinism across re-renders. ADRs 0008–0012 written. Full retrospective in `docs/slices/slice-e.md` §11.

This session ran a docs-simplify loop on top: see `docs/simplify-survey.md` for the SLOC baseline and `docs/performance.md` (699 → 206 lines) and this file for the trims that landed.

## Explicitly deferred

- **Tex snapshot harness** (Step 6 follow-up): corpus data + coverage doc shipped, but the parametrized snapshot test, baseline PNGs, `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE` did not. Cross-platform tolerance pinning needs Linux/lavapipe CI runs. Tracked in `docs/tex-coverage.md` "Snapshot tolerance".
- **Baseline-PNG version of the Text render test.** Same blocker. Today's centroid-based check in `tests/python/test_text.py::test_text_renders_visible_pixels_at_origin` is the placeholder.

## Next action

Pick the next slice. Three candidates, all reasonable:

- **Tex snapshot harness as a mini-slice** — picks up deferred Step 6, closes the success criterion Slice E punted.
- **Slice E.5 — SVG import** — adjacent to glyph outlines, small-but-real, unblocks SVGMobject.
- **Slice F — 3D pipeline** — surface pipeline, depth buffer, phi/theta camera; reintroduces `flat_stroke`/`unit_normal` from deferred Slice D §4. Unlocks 3D text.

Also outstanding from the docs-simplify pass:
- Trim shipped slice plans (b/c/d), audit `architecture.md` for staleness, prune `gotchas.md` (next loop).
- Decision: structured docs framework (Fumadocs / Nextra / Starlight) for 0.1.0 release. See `docs/simplify-survey.md` recommendation.

CLAUDE.md's "Read before you touch anything" list is the on-ramp. New slice should start with a `docs/slices/<name>.md` plan.

## Blockers

- None.
