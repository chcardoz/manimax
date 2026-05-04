# Roadmap

Architectural watchpoints — work that's deferred, with concrete triggers for when to revisit. Each entry says what the watchpoint is, why we're not acting now, and the trigger that flips it from "noted" to "do this."

## IR codegen — promote Rust to source-of-truth, generate Python

**Today.** `crates/manim-rs-ir/src/lib.rs` (serde) and `python/manim_rs/ir/` (msgspec subpackage) are hand-mirrored. The roundtrip test catches structural drift but not field-ordering drift. Adding an IR variant requires edits in both files plus a paired fixture update.

**Why we're not acting.** With four `Object` variants and ~20 IR structs, the manual mirror is tractable. Building an IDL + codegen pipeline ([rerun's approach](https://github.com/rerun-io/rerun/blob/main/ARCHITECTURE.md)) would be premature.

**Trigger.** When the next time-invariant content variant lands (`SVG`, `Surface`, …) — at 5+ `Object` variants the duplication starts paying real cost. Likely path: codegen `python/manim_rs/ir/` from the Rust IR via a small `build.rs` step, keeping JSON wire and msgspec authoring. Rerun went the typed-schema route (Cap'n Proto / Arrow IPC) because they had four SDK languages; we have one and a half.

## `Object` enum vs. trait + dyn-dispatch registry

**Today.** `Object` is a `serde`-tagged enum with one variant per renderable kind. Pattern matches exist across `eval` and `raster`.

**Why we're not acting.** At 4 variants the enum is right: exhaustiveness checking catches "you added a variant and forgot to handle it" at compile time, serde does the wire format for free, adding a variant is mechanical (~6 sites).

**Trigger.** When `if let Object::*` / `match object { Object::* ... }` sites cross **6** in either `crates/manim-rs-eval/src` or `crates/manim-rs-raster/src`. Today both sides sit at ~2. When either hits 6, refactor to `trait Renderable` with `compile()`, `tessellate()`, `bbox()` + a registry of impls. Wire format stays serde-tagged-enum.

The watchpoint is the **count**, not "we're at Slice F so we should refactor." Premature trait-ification of a 4-variant enum is a worse trap than waiting until 6.

## Drop `Object::Tex.macros` field on the next IR schema bump

**Today.** ADR 0008 §E left `Object::Tex.macros: BTreeMap<String, String>` as a forward-compat field; the Python `Tex(...)` constructor always emits it empty because macro pre-expansion runs at construction time.

**Why this is debt.** Empty-on-the-wire dead schema. Every reader will assume it's reachable.

**Trigger.** The next `SCHEMA_VERSION` bump for any reason. Don't bump *just* for this. If a reason emerges to support runtime macro expansion (arg macros, vendor-and-patch ratex-parser path), reverse this and start populating it.

## Refactor evaluator track folders

**Today.** `sum_segments`, `sum_scalars`, `product_scalars` in `crates/manim-rs-eval/src/evaluator.rs` are three near-identical functions.

**Why this is debt.** Trivial and low-priority. Compiler inlines them; no perf cost. Maintenance cost is "any change to fold shape means three near-identical edits."

**Trigger.** Whenever someone next touches that file for any reason, fold into one generic `fold_tracks<S, V, F>(tracks, t, init, op)`. Don't make a dedicated PR — piggy-back cleanup.

## Split `crates/manim-rs-raster/src/lib.rs`

**Today.** ~700 LOC mixing wgpu setup, per-frame render loop, and readback. Most-edited file in the repo across slices.

**Why this is debt.** Onboarding cost. New readers scroll through pipeline setup to find the per-frame logic. Refactor target:

- `lib.rs` — public API + `Runtime` struct
- `setup.rs` — device, MSAA targets, pipelines
- `render.rs` — per-frame `render` and helpers
- `readback.rs` — buffer copy out + row alignment dance

**Trigger.** The next slice that needs to add a new pipeline (probably the surface/depth pipeline) or a new target type (headless capture, swapchain). Single-file expansion past 1000 LOC is the natural breakpoint.

## General-purpose `render(scene, frames, sink)` over format-specific functions

**Today.** `render_to_mp4` + `render_frame_to_png` are format-specific.

**Trigger.** Any of:

- A user/caller asks for WebM, GIF, APNG, or an image sequence.
- Someone proposes adding a third format-specific entry point.
- A snapshot test wants in-memory bytes without going through disk.

When the first fires, do the consolidation pass — introduce a `FrameSink` trait. Don't add a third format-specific function first; that's how things calcify.

## GPU-side encode handoff (skip CPU readback on macOS)

**Today.** Every frame goes GPU → padded readback buffer → `map_async` + `device.poll(wait_indefinitely)` → CPU `Vec<u8>` → encoder. Empirically (`performance.md` M1) this readback is ~78 % of per-frame time on a 1280×720 hardware-encoded render, and it's the reason `--workers > 1` cannot deliver local speedup on a single GPU — readback serializes through one DMA bus.

**Why we're not acting.** Requires a Metal-specific path through `wgpu::hal` to share an `IOSurface`-backed `CVPixelBuffer` between the wgpu render target and a VideoToolbox `VTCompressionSession`. Distinct effort from the portable encode pipeline. Pipelined / double-buffered readback (entry **N6** in `performance.md`) is the portable predecessor and likely captures most of the win without platform code.

**Trigger.** Any of:

- A target user (Divita, local previewers) reports render-throughput pain on single-GPU hardware that the pipelined-readback fix didn't resolve.
- We add a second platform-specific encoder backend (NVENC) — bundle the IOSurface and the equivalent CUDA/D3D11 surface path together.
- Frame readback exceeds 50 % of frame time at 4K with the in-process hardware encoder after pipelining lands.

Sequencing: do **N6** (pipelined readback) first; re-trace; only then evaluate IOSurface handoff against remaining frame-time share.

## Update cadence

When a trigger fires and gets actioned, the entry moves to the changelog or a new design note. When a trigger fires and we *choose* not to act, leave a dated note explaining why. Empty file = no parked work, which is fine.
