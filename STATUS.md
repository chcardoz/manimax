# Status

**Last updated:** 2026-05-01
**Current slice:** none (CI fix after Slice E/docs work)

CI failure `test` / GitHub Job `73898676458` was four failures in `tests/python/test_e2e_text_tex.py`: helpers imported `examples.text_scene` / `examples.tex_scene` as top-level modules, but the CI editable/maturin environment did not put the repo root on `sys.path`.

This session changed those helpers to load the checked-in example files by path via `manim_rs.discovery.load_scene`, preserving coverage of the real examples without depending on ambient import path. Added the CI import-path trap to `docs/public/contributing/gotchas.md`.

Slice E (text + math) remains shipped end-to-end. Both §1 acceptance scenes render with bundled fonts, no system LaTeX or system font required. Per-`Evaluator` source-keyed caches for Tex and Text geometry; `compile_tex` / `compile_text` measurably hit on duplicate sources; byte-determinism across re-renders.

## Explicitly deferred

- **Tex snapshot harness** (Slice E Step 6 follow-up): corpus data + coverage doc shipped, but the parametrized snapshot test, baseline PNGs, `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE` did not. Cross-platform tolerance pinning needs Linux/lavapipe CI runs. Tracked in `docs/public/contributing/porting-from-manimgl.md` "Tex coverage > Snapshot tolerance".
- **Baseline-PNG version of the Text render test.** Same blocker. Today's centroid-based check is the placeholder.

## Next action

If CI is green, pick the next slice. Leading candidates: Tex snapshot harness mini-slice, Slice E.5 SVG import, or Slice F 3D pipeline.

## Blockers

- None.
