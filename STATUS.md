# Status

**Last updated:** 2026-04-30
**Current slice:** none (Slice E complete; next slice unscoped)

Slice E (text + math) shipped end-to-end on `chcardoz/vancouver-v1` ahead of `main` (`8df2808`). Both §1 acceptance commands work: `Text("...")` and `Tex(r"...")` render correctly with bundled fonts, no system LaTeX or system font required. Per-`Evaluator` source-keyed caches for Tex and Text geometry; `compile_tex` / `compile_text` measurably hit on duplicate sources; byte-determinism across re-renders.

This session collapsed the docs tree into `docs/public/` and wired up MkDocs Material with auto-deploy via `.github/workflows/docs.yml`. Old structure (`docs/architecture.md`, `docs/decisions/`, `docs/slices/`, `docs/porting-notes/`, etc.) is gone — see `docs/public/contributing/index.md` for the new on-ramp.

## Explicitly deferred

- **Tex snapshot harness** (Slice E Step 6 follow-up): corpus data + coverage doc shipped, but the parametrized snapshot test, baseline PNGs, `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE` did not. Cross-platform tolerance pinning needs Linux/lavapipe CI runs. Tracked in `docs/public/contributing/porting-from-manimgl.md` "Tex coverage > Snapshot tolerance".
- **Baseline-PNG version of the Text render test.** Same blocker. Today's centroid-based check is the placeholder.

## Next action

Pick the next slice. Three candidates, all reasonable:

- **Tex snapshot harness as a mini-slice** — picks up deferred Step 6.
- **Slice E.5 — SVG import** — adjacent to glyph outlines, small-but-real, unblocks SVGMobject.
- **Slice F — 3D pipeline** — surface pipeline, depth buffer, phi/theta camera; reintroduces `flat_stroke`/`unit_normal` from deferred Slice D §4. Unlocks 3D text.

`AGENTS.md`'s "Read before you touch anything" list is the on-ramp.

## Blockers

- None.
