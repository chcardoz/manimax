# Future directions

Architectural watchpoints that aren't decisions yet — things we're
deferring with concrete triggers for when to revisit. Distinct from:

- `docs/decisions/` — decisions actually made, with their rationale.
- `docs/performance.md` — measurable perf levers with cost/benefit.
- `docs/slices/*.md` — what we're building right now.

Each entry: what the watchpoint is, why we're not acting now, and the
**concrete trigger** that flips it from "noted" to "do this." If a
trigger fires and nobody picks the entry up, that's a signal the
trigger was wrong, not the entry — re-tune it rather than
deleting.

---

## F1. Promote `crates/manim-rs-ir` to source-of-truth; codegen Python msgspec from it

**Today.** `crates/manim-rs-ir/src/lib.rs` (serde) and
`python/manim_rs/ir.py` (msgspec) are hand-mirrored. The
`tests/python/test_ir_roundtrip.py` schema-drift test catches
*structural* drift (a missing field shows up as a deserialization
failure) but not *ordering* drift (`docs/gotchas.md` "msgspec / pyo3
tagged-union field order is tolerant"). Adding `Object::Tex` in
Slice E required edits in both files plus a paired update to the
roundtrip fixture.

**Why we're not acting.** With four `Object` variants and ~20 IR
structs, the manual mirror is tractable — the cost of staying in
sync is roughly one extra file edit per IR change, and we have a
test that catches the most likely bug class. Building an IDL +
codegen pipeline (rerun's approach — see [their architecture
doc](https://github.com/rerun-io/rerun/blob/main/ARCHITECTURE.md))
would be premature for the current shape.

**Trigger.** When the next time-invariant content variant lands
(`Text`, `SVG`, `Surface`, …). At that point the IR will have:

- 5+ `Object` variants, several with new field shapes.
- A pattern of "every new variant adds a struct on both sides plus a
  `to_ir`/`from_ir` test fixture."
- Visible duplication where someone adds a Rust field, the Python side
  silently lags, and the bug surfaces only when a renderer test hits it.

When that pattern is concrete (not hypothetical), promote the Rust IR
to the canonical source and either:

- Generate `python/manim_rs/ir.py` from it via a small `build.rs` +
  Python-emit step, or
- Switch the wire to a typed schema (Cap'n Proto, FlatBuffers,
  protobuf) and let upstream codegen handle both sides.

I lean toward the first — the wire stays JSON, msgspec stays the
Python author's tool, and we just stop hand-writing the mirror.
Rerun took the second path because they had four SDK languages; we
have one and a half.

---

## F2. `Object` enum vs. trait + dyn-dispatch registry

**Today.** `Object` is a `serde`-tagged enum with one variant per
renderable kind (`Polyline`, `BezPath`, `Tex`; soon `Text`). Pattern
matches on `Object` exist across `eval` (Tex fan-out site) and
`raster` (`render_object.rs`'s `tessellate_object`,
`render_object.rs`'s `unreachable!` guard for Tex post-fan-out).

**Why we're not acting.** At 4 variants the enum is the right shape:
exhaustiveness checking catches "you added a variant and forgot to
handle it" at compile time, serde does the wire format for free, and
adding a variant is mechanical (~6 sites to update). The cost
function only inverts when sites multiply.

**Trigger.** When `if let Object::*` / `match object { Object::* ... }`
sites cross **6** in either `crates/manim-rs-eval/src` or
`crates/manim-rs-raster/src`. Count today (post-Slice-E):

- `manim-rs-eval/src`: ~2 sites (eval_at fan-out, render_object
  pattern).
- `manim-rs-raster/src`: ~2 sites (tessellator dispatch, fan-out
  guard).

When either side hits 6, the cost of "every new variant edits N
sites" exceeds the cost of designing a trait surface. At that point
the [Bevy plugin pattern](https://bevy.org/learn/quick-start/getting-started/ecs/)
becomes the right reference: a `trait Renderable` with `compile()`,
`tessellate()`, `bbox()` methods + a registry of concrete
`Renderable` impls. Wire format stays serde-tagged-enum (Bevy ECS
isn't relevant for Manimax — we're not building a query-based
runtime).

The watchpoint here is the **count**, not "we're at Slice F so we
should refactor." Premature trait-ification of a 4-variant enum is
a worse trap than waiting until 6.

---

## F3. Drop `Object::Tex.macros` field on the next IR schema bump

**Today.** ADR 0008 §E makes `Object::Tex.macros: BTreeMap<String,
String>` a field that the Python `Tex(...)` constructor always
emits empty (`{}`) because macro pre-expansion runs at construction
time. The field exists for forward compatibility — if we ever wanted
to ship un-expanded macros across the wire, the IR carries the
slot.

**Why this is debt.** Every reader who finds `macros` on
`Object::Tex` will assume it's reachable and try to use it. The
hash key once included it (the post-/simplify cleanup pass narrowed
to `(src, color)`); future cache layers might do the same. It's
empty-on-the-wire dead schema today.

**Trigger.** The next IR schema bump for any reason. When
`SCHEMA_VERSION` increments, drop `macros` in the same change. Don't
bump the schema *just* for this — that's a churn cost without
matching benefit. But ride the next coattail.

If a Slice F+ reason emerges to support runtime macro expansion
(arg macros, vendor-and-patch ratex-parser path), reverse this:
keep the field and start populating it. ADR 0008 §E predicts the
escalation path; until that path lights up, the field is ballast.

---

## F4. Refactor `crates/manim-rs-eval/src/evaluator.rs` track folders

**Today.** `sum_segments`, `sum_scalars`, `product_scalars` are
three near-identical functions. Each iterates `tracks: &[Vec<S>]`,
calls `evaluate_track`, and folds with a different operator + init.

**Why this is debt.** Trivial, low-priority. The compiler will
inline them; no perf cost. The maintenance cost is "any change to
the fold shape (e.g. new `accum_scalars` variant for max-takes-all
semantics) means three near-identical edits."

**Trigger.** Whenever someone next touches that file for any
reason, fold the three into one generic `fold_tracks<S, V, F>(tracks,
t, init, op)`. Don't make a dedicated PR for this — it's a
piggy-back cleanup.

---

## F5. Split `crates/manim-rs-raster/src/lib.rs`

**Today.** ~700 LOC mixing wgpu setup (device, MSAA targets,
pipelines), per-frame render loop, and readback. Most-edited file
in the repo across slices.

**Why this is debt.** Onboarding-cost. New readers have to scroll
through pipeline setup to find the per-frame logic. Refactor target:

- `lib.rs` — public API + `Runtime` struct definition.
- `setup.rs` — device, MSAA targets, pipeline construction.
- `render.rs` — the per-frame `render` method and its helpers.
- `readback.rs` — buffer copy out + row alignment dance.

**Trigger.** The next slice that needs to add a new pipeline
(probably Slice F's surface/depth pipeline) or a new target type
(headless capture, swapchain). A single-file expansion past 1000
LOC is the natural breakpoint.

---

## F6. Re-visit single-format render functions vs. general-purpose `render(scene, frames, sink)`

**Today.** `render_to_mp4` + `render_frame_to_png` are format-specific.
The `FrameSink` trait sketch is in `docs/performance.md` "Future
architectural direction" — kept there because it's the perf doc's
established home for "consolidation directions."

**Watchpoint here**, not in performance.md, because the *trigger* is
architectural, not perf-shaped: the moment a third format request
arrives. (Performance.md tracks the *shape*; this file tracks the
*decision-to-defer*.)

**Trigger.** Any of:

- A user/caller asks for WebM, GIF, APNG, or an image sequence.
- Someone proposes adding a third format-specific entry point on
  the Rust side.
- A snapshot test wants in-memory bytes without going through disk.

When the first of those fires, do the consolidation pass. Don't add
a third format-specific function first — that's how things calcify.

---

## F7. PyO3 boundary error chain ✅ closed (2026-04-28)

Shipped as `manim-rs-py::runtime_err_to_pyerr` — walks
`std::error::Error::source` and builds a `PyRuntimeError` chain via
`PyErr::set_cause`. Free function rather than `From<RuntimeError> for
PyErr` because orphan rules forbid the latter (both types live outside
`manim-rs-py`). See `docs/performance.md` E3c for the shipped form.

---

Update cadence: when a trigger fires and gets actioned, the entry
moves to a slice retro or an ADR. When a trigger fires and we
*choose* not to act, leave a dated note explaining why. Empty
file = no parked work, which is fine — this file should stay
short.
