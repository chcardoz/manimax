# Status

**Last updated:** 2026-04-22
**Current slice:** Slice C — Steps 1–7 complete (`docs/slices/slice-c.md`). Only Step 8 (ADR + retro) remains.

## Last session did

- **Step 7 shipped — mandated integration scene green.** First render that exercises the entire Slice C stack end-to-end through the Python authoring surface.
  - **Filled the Python authoring gap** (the skipped doc-Step 5):
    - `BezPath` object class in `objects/geometry.py` + verb builders `move_to`/`line_to`/`quad_to`/`cubic_to`/`close`.
    - Five animation verbs in `animate/transforms.py`: `Rotate`, `ScaleTo`, `FadeIn`, `FadeOut`, `Colorize`. Each accepts `easing=` (default `Linear`). `Translate` also gained the `easing=` kwarg.
    - All 15 easings re-exported as friendly aliases (`Smooth`, `Overshoot`, `ThereAndBack`, `NotQuiteThere`, …) from `manim_rs` top-level.
    - `Scene.play` generalised from position-only to all 5 track kinds via a per-kind segment-bucket table; `.ir` emits whichever tracks have segments.
  - **Integration scene** at `tests/python/integration_scene.py`: red square + green teardrop BezPath + blue triangle, each with ≥2 simultaneous tracks drawn from {position, opacity, rotation, scale, color}, using 4 different easings (Linear, Smooth, Overshoot, ThereAndBack, plus NotQuiteThere wrapping Smooth to exercise the recursive easing path). Both fill and stroke represented.
  - **Test** at `tests/python/test_integration_scene.py` (3 tests):
    - `ffprobe` metadata (width/height/fps/codec/pix_fmt/frame count).
    - Pixel sum + nonzero count at frames 30 and 55, ±10% tolerance.
    - Per-object centroid via color bands (red / green / blue), ±25 px tolerance — confirms each object animated to the expected screen position.

Totals: **Rust 53 passed / 0 failed** (unchanged), **Python 86 passed / 0 failed** (up from 83 — +3 integration tests).

Visual check: rendered `/tmp/integration.mp4`, eyeballed f30 + f55 — all three objects visible with distinct colors, fill + stroke both rendering, animation state matches expected composition at both inspection points.

## Next action

**Slice C Step 8 — consolidated ADR + retrospective prep.**

- Write `docs/decisions/0004-slice-c-decisions.md` (~10 lines per ADR-lite template) covering: pythonize FFI, BezPath unified primitive, all-tracks-all-easings, fill+MSAA pair, tolerance-based snapshots.
- Update `docs/gotchas.md` with anything surfaced this slice (the H.264/yuv420p chroma-smear that needed tolerant color-band filters is a candidate).
- Fill the Retrospective section in `docs/slices/slice-c.md` immediately after.

## Blockers

None.

## Notes for next session

- `integration_scene.py` lives in `tests/python/` because it's both a test fixture and documentation of the authoring API. If we grow more examples, consider a top-level `examples/` that tests can pull from.
- H.264/yuv420p chroma subsampling visibly shifts solid-fill colors (e.g. `(0, 229, 51)` decodes as `(0, 240, 120)` ish). The centroid color-bands in `test_integration_scene.py` are tuned for that. If we ever offer a lossless-raw output, the bands should be tightened for that code path.
- `Colorize` requires explicit `from_color` — the evaluator's color-track semantics are "last-write override" rather than "transition from current authored color." Worth revisiting if we want `Colorize(obj, to=...)` inferring `from_` from the object's authored color.

## Convention for updating this file

- **Rewrite, don't append.** This file is current-state, not history. Git log is the history.
- Update at the end of every session *before* handing back to the user.
- Keep it under ~50 lines. If it's growing, state is leaking in that should be in `docs/slices/<slice>.md` checkboxes or a porting note.
- Three required sections: **Last session did**, **Next action**, **Blockers**. Everything else is optional.
