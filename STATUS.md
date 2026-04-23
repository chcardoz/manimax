# Status

**Last updated:** 2026-04-23
**Current slice:** Between slices — Slice C shipped, Slice D not yet planned.

## Last session did

- Audited the post-Slice C cleanup backlog against the actual repo state and
  found it already landed — porting notes (`transforms.md`, `geometry.md`),
  `ir-schema.md` conventions sections, `performance.md` observations
  (O3/O7/O12, N0/N1), gotchas (ffmpeg stderr draining, `allow_threads`
  ordering, pythonize tuples), `Runtime::render` split + `needs_clear` rename,
  `ir.py` section banners, and class-keyed `scene.py:_segments` are all in.
  STATUS was stale — fixed.

Still-load-bearing context from earlier sessions:

- `d4cf198` — per-pipeline `PipeBundle` on `Runtime` (pipeline + buffers + bind
  group bundled). Organisational; preserves `queue.write_buffer` ordering.
- `5b8ab77` / ADR 0005 — plain IR + compiled `manim_rs_eval::Evaluator`.
  `Evaluator::new(scene)` wraps timeline objects in `Arc`, builds the track
  index once; frames evaluate cheaply via `Evaluator::eval_at(t)`.

## Next action

Plan Slice D. Starting point: `docs/slices/slice-c.md` §10 (stroke port from
`manimlib/shaders/quadratic_bezier/stroke/` + snapshot cache keyed on IR hash)
and §11 "Deltas for Slice D planning" (collapse expose-to-Python + use-in-test;
keep `BezPath` verbs stable; re-check `rate_functions.py` at its pinned SHA).

## Blockers

- None.
