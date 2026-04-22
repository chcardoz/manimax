# Status

**Last updated:** 2026-04-22
**Current slice:** Post-Slice C cleanup / architecture follow-up.

## Last session did

- Bundled per-pipeline GPU resources in the raster crate (commit `d4cf198`). Each
  pipeline (fill, stroke) now owns a `PipeBundle` containing its pipeline,
  vertex/index/uniform buffers, and bind group — replacing the earlier flat set
  of parallel fields on `Runtime`. Keeps the per-object submit pattern
  (`docs/gotchas.md` — `queue.write_buffer` ordering) intact; this is pure
  organisational refactor, no behaviour change.
- Verification green:
  - `cargo test --workspace --exclude manim-rs-py`
  - `source .venv/bin/activate && maturin develop && pytest tests/python`

Preceding session (commit `5b8ab77`, still the basis of the current architecture):

- Evaluator-boundary cleanup: `TimelineOp::Add.object` is plain `Object` again;
  `manim_rs_eval::Evaluator` compiles a `Scene` once (wraps timeline objects in
  `Arc<Object>`, builds the track index) and evaluates frames cheaply via
  `Evaluator::eval_at(t)`. Runtime and pyo3 hot paths use the compiled
  evaluator once per render / eval call. ADR 0005 records the decision.

## Next action

- Docs-only cleanup pass (this session, in progress): stale STATUS, missing
  porting notes for `transforms.py` / `geometry.py`, ir-schema coordinate +
  alpha conventions, new performance observations, gotchas for ffmpeg stderr /
  GIL / pythonize.
- Code-legibility follow-ups to consider after docs: split `Runtime::render`
  (170 lines, 3–4 levels of nesting); rename `first` → `needs_clear`; section
  headers in `python/manim_rs/ir.py`; `scene.py:_segments` keyed by track
  class directly.

## Blockers

- None.
