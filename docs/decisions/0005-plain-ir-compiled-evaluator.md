# 0005 — Plain IR, compiled evaluator

**Date:** 2026-04-22
**Status:** accepted

## Decision
Keep `manim-rs-ir::Scene` as plain owning data (`TimelineOp::Add.object: Object`) and move shared-geometry/indexing optimizations into `manim-rs-eval::Evaluator`.

## Why
- The IR is the Python↔Rust contract, so it should describe scenes plainly rather than expose Rust-specific storage details like `Arc`.
- Examples, tests, and Python callers should construct `Object` directly; requiring `Arc::new(...)` at the boundary leaks an evaluator optimization upward.
- Rendering and repeated `eval_at` calls still need to be fast, so compilation now consumes `Scene` and builds shared `Arc<Object>` timeline entries plus a one-time track index inside the evaluator.

## Consequences
- Serialized IR and scene construction stay simple; `serde` no longer needs the workspace-wide `rc` feature.
- The fast path is explicit: `Evaluator::new(scene)` compiles once, then `eval_at(t)` is cheap across many frames.
- Borrowed convenience APIs may clone once up front (`Evaluator::from_scene`, free `eval_at(&Scene, t)`), which is acceptable for one-off calls but not the preferred render path.

## Rejected alternatives
- **Keep `Arc<Object>` in the IR.** Rejected: simpler evaluator internals, but a leakier Python/Rust contract and noisier examples/tests.
- **Add a new crate for compiled scenes.** Rejected: too much surface area for a still-local optimization boundary; `manim-rs-eval` is the right home today.
