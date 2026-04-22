# Porting note: evaluator

**Status:** Slice B complete (position-only, linear easing).
**Manimgl reference:** `reference/manimgl/manimlib/animation/animation.py` — `Animation.interpolate(alpha)` and the `update_mobjects`/`begin`/`finish` lifecycle.

The evaluator is **reimplemented, not ported**. Manimgl's model is stateful and replayed-from-zero; ours is pure and random-access. The invariants below are what make that model hold — violating any of them re-couples frame `n` to frame `n-1` and kills the architectural thesis of the project.

As of 2026-04-22 the Rust side has two layers:

- `Scene` in `manim-rs-ir` is still the plain serializable contract.
- `Evaluator` in `manim-rs-eval` is the compiled/runtime form: it owns a one-time track index and wraps timeline objects in `Arc<Object>` so repeated `eval_at` calls share geometry cheaply.

That split is deliberate. Shared ownership is a runtime optimization, not part of the IR language.

## The purity contract

`eval_at(scene: &Scene, t: Time) -> SceneState` is a **pure function**. No caching, no history, no state carried across calls.

For hot paths, compile once with `Evaluator::new(scene)` and call `evaluator.eval_at(t)`. That preserves purity while avoiding per-call track indexing and object re-wrapping.

Same `(scene, t)` → same `SceneState`. Always. This is load-bearing, not aesthetic:

1. **Random-access frame rendering** — `eval_at(scene, frame_k / fps)` produces frame `k` without evaluating frames `0..k-1`. The whole point of the IR.
2. **Parallel / chunked rendering** — a worker rendering frames `[300, 600)` can do so in isolation; no shared state across frame ranges.
3. **Memoization by `(ir_hash, t)`** — legal because `eval_at` is pure; unlocks snapshot caches later.
4. **Determinism in tests** — frame-pixel tests can render any `t` directly.

If you add a new track type, easing, or timeline op, the rule is: **no hidden state.** Anything that looks like it needs state across times is a sign the IR is missing a field.

## Non-obvious invariants

The evaluator *trusts* the following; none are validated at runtime:

- `scene.timeline` is sorted non-decreasing by `t`.
- Every `Track::Position.id` matches some `TimelineOp::Add.id`.
- Segments within a position track are non-overlapping.

The Python recorder (`python/manim_rs/scene.py`) is responsible for emitting IR that satisfies these. If you ever take IR from an external source, validate before handing it to the evaluator.

## The gap-clamping rule (the bug most likely to regress)

For a position track with two segments `[0.0, 1.0]` and `[1.2, 2.0]`, asking for `t = 1.1` must return **the `to` of the segment that ended at 1.0**, not the `to` of the overall last segment.

**Wrong:**

```rust
let mut held = segments.last().map(|s| s.to).unwrap_or([0.0; 3]);
for seg in segments {
    if t >= seg.t0 && t <= seg.t1 { return lerp(...); }
}
held
```

**Right** (see `crates/manim-rs-eval/src/lib.rs:108-125`):

```rust
let mut held: Vec3 = [0.0, 0.0, 0.0];
for seg in segments {
    if t >= seg.t0 && t <= seg.t1 { return lerp(...); }
    if seg.t1 < t { held = seg.to; }
}
held
```

The distinction matters because gaps are legal (e.g. "translate from 0s to 1s, hold, translate again from 1.2s to 2s"). Returning the final segment's `to` during a hold would teleport the object.

A dedicated test (search the module for gap + held language) pins this — don't delete it when refactoring.

## Missing-track conventions

- **No position track for an object** → position `(0, 0, 0)`. Not an error.
- **`t < track.segments[0].t0`** → position `(0, 0, 0)`.
- **`t > track.segments.last().t1`** → position `segments.last().to`.

Summed across tracks. In Slice B there's at most one position track per object, but the evaluator already sums in preparation for Slice C (multiple tracks per object — e.g. orbit + drift).

## Manimax ↔ manimgl mapping

| manimgl | Manimax |
|---|---|
| `Animation.interpolate(alpha)` on each mobject | `evaluate_position_track(segments, t)` on each track |
| `Scene.play(anim)` advances clock + renders | `Scene.play(anim)` only *records* tracks; rendering happens downstream |
| `rate_functions.linear` | `Easing::Linear` → `apply_easing` returns `alpha` as-is |
| Animation composition via `AnimationGroup` | Multiple tracks on the same object ID — evaluator sums |

## What Slice C adds here

- More easings (`Smooth`, `Rush`, custom). Each lands in `apply_easing`.
- Non-position tracks (opacity, color, rotation, scale) each get their own `evaluate_*_track` function following the same shape.
- Still no cross-frame state. Still pure.

## Files touched

- `crates/manim-rs-eval/src/lib.rs` — all eval logic. 9 tests.
