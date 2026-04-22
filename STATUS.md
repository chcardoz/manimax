# Status

**Last updated:** 2026-04-22
**Current slice:** Post-Slice C cleanup / architecture follow-up.

## Last session did

- Reverted the IR boundary back to plain owning data: `TimelineOp::Add.object` is `Object` again; workspace `serde` no longer needs the `rc` feature.
- Added `manim_rs_eval::Evaluator`, a compiled evaluator that consumes `Scene`, wraps timeline objects in `Arc<Object>`, builds the track index once, and evaluates frames cheaply via `Evaluator::eval_at(t)`.
- Kept the pure convenience API `eval_at(&Scene, t)` by routing it through `Evaluator::from_scene(scene)` for one-off callers.
- Updated runtime/Python hot paths to use the compiled evaluator once per render / eval call:
  - `manim-rs-runtime::render_to_mp4(scene, out)` now takes owned `Scene` and compiles once.
  - `crates/manim-rs-py` now moves the depythonized `Scene` into runtime/evaluator rather than cloning through the render loop.
- Simplified raster transform tests to use `ObjectState::with_defaults(...)` instead of repeating neutral fields.
- Wrote ADR `docs/decisions/0005-plain-ir-compiled-evaluator.md` and updated `docs/porting-notes/eval.md` to document the new split.
- Verification green:
  - `cargo test --workspace --exclude manim-rs-py`
  - `source /Users/chcardoz/conductor/workspaces/manimax/wellington/.venv/bin/activate && maturin develop && pytest tests/python`
  - Totals unchanged: **Rust 53 passed / 0 failed**, **Python 86 passed / 0 failed**.
- Stress check after the boundary cleanup:
  - `python scripts/perf_probe.py` stayed in-family with the Slice C baseline (`eval_at` 0.13 ms/call; 480p/30 integration render 0.22 s).
  - CLI render at 4K / 120 fps / 4 s completed in 28.5 s and produced a valid 480-frame mp4.
  - Five consecutive 1080p / 60 fps / 2 s CLI renders all completed and ffprobe reported the expected 120 frames each.

## Next action

- Commit the evaluator-boundary cleanup if desired.
- If we keep pushing on eval perf, the next natural step is removing the remaining generic segment machinery (`Lerp` / `Segment` / macro) only if it improves readability without hurting tests.

## Blockers

- None.
