# Concepts

The mental model behind Manimax. Read in order.

## The IR is the contract

A scene is a Python program that builds a typed intermediate representation (IR) — `Scene`, `Object`, `Track`, `TimelineOp`. The Rust runtime consumes the IR and produces pixels. **Rust never runs Python; Python never rasterizes.** The interface between them is data, not function calls.

```
Python (authoring)  →  IR (data)  →  Rust (eval + raster + encode)  →  mp4
```

The IR is just msgspec on the Python side and serde on the Rust side, exchanged as a JSON string across the FFI boundary. See [IR schema](ir-schema.md) for the field-level contract.

## Frames are pure functions of (IR, t)

ManimGL gets frame `N` by replaying Python from frame 0 to N. Manimax computes frame `N` by evaluating the IR at time `t = N / fps`. Same `(IR, t)` always produces the same `SceneState`.

This is load-bearing, not aesthetic:

- **Random access:** render frame 1000 directly, no replay.
- **Parallel rendering:** workers handle disjoint frame ranges in isolation.
- **Memoization:** content-addressed snapshot caches by `(ir_hash, t)`.
- **Test determinism:** any frame at any time, byte-for-byte reproducible.

If you add a track type, easing, or timeline op, the rule is **no hidden state.** Anything that looks like it needs state across times means the IR is missing a field.

## Authoring is recording

`Scene.play(...)`, `scene.add(...)`, `scene.wait(...)` look like ManimGL but they don't render anything. They append to a timeline. After `construct()` returns, `scene.ir` holds the complete recorded artifact, ready to hand to the runtime — or inspect, or hash, or ship over the wire.

Animation classes are **inert track descriptions**, not active interpolation loops. `Translate(obj, by=(2, 0, 0), duration=2.0)` emits a `PositionTrack` with one segment. The Rust evaluator interprets it.

## What lives on each side

**Python** (authoring):
- Scene class, mobject hierarchy, geometry constructors
- Animation classes that emit tracks
- LaTeX/text shaping decisions (Tex/Text invariants)
- numpy-heavy point-array construction
- The CLI

**Rust** (consumption):
- IR evaluation (`eval_at`)
- wgpu rasterization (stroke + fill + glyph pipelines)
- In-process libavcodec encoding
- Snapshot cache (per-`Evaluator`)

## Read next

- [Architecture](architecture.md) — the stack, version pins, what was ruled out and why
- [IR schema](ir-schema.md) — the Python↔Rust contract, field by field
