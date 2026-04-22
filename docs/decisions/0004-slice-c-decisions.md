# 0004 — Slice C consolidated decisions

**Date:** 2026-04-22
**Status:** accepted
**Supersedes:** `0001-ir-wire-format-json-string.md` (FFI wire format).

One ADR covering the five design calls that defined Slice C. Each would normally
be its own file; grouping them here matches the "single consolidated ADR" scope
decision in `docs/slices/slice-c.md` §2 and avoids retroactive ADR churn flagged
in Slice B's retro.

---

## A. FFI wire format: `pythonize` (supersedes 0001)

### Decision
The IR crosses the pyo3 boundary as a Python object graph converted via
`pythonize::depythonize` / `pythonize`, not a JSON string. `render_to_mp4` and
the new `eval_at` pyfunction both take `&Bound<PyAny>`.

### Why
- Per-frame parse cost scales with IR size; Slice C's richer IR (BezPath,
  5 track kinds, 15 easings) would make JSON parsing visible in frame budgets.
- Single source of truth: the serde derives that back Rust types also drive FFI
  deserialization — no second schema to keep in sync.
- `eval_at` would otherwise force IR into a string on every sample, defeating
  the purpose of a pure evaluator.

### Consequences
- Wire format is no longer dump-to-disk debuggable. Mitigation: the
  `_rust.roundtrip_ir` probe still uses JSON, and `Scene.ir.to_json()` is
  preserved for inspection.
- msgspec Structs go through `pythonize` attribute access cleanly; see the
  `Vec3-as-tuple` gotcha in `docs/gotchas.md`.

### Rejected alternatives
- **Keep JSON string.** Rejected: parse cost + weak contract across a
  growing IR.
- **Per-type `FromPyObject` impls.** More code to maintain; `pythonize` re-uses
  the existing serde derives.
- **Arrow IPC.** Overkill for per-scene payloads; appropriate if tracks become
  columnar in a later slice.

---

## B. `BezPath` as the unified vector primitive

### Decision
Add one IR geometry variant — `BezPath { verbs: Vec<PathVerb> }` with verbs
`MoveTo | LineTo | QuadTo | CubicTo | Close` — and express every shape
(`Circle`, `Rectangle`, `Arc`, `Polygon`, `Line`) as a Python factory that emits
a `BezPath`. Polyline remains in the IR as a convenience variant for straight
open chains.

### Why
- Every manimgl `VMobject` is ultimately a cubic Bézier chain. One primitive
  subsumes all of them.
- Adding a new shape becomes a pure-Python task (factory emits verbs); no IR
  schema change, no new Rust evaluator path, no new shader.
- Avoids the "one IR variant per shape" trap that would balloon the schema.

### Consequences
- Rust fill + tessellation speak one input type. Stroke and fill pipelines
  share the same vertex input shape.
- Straight polylines have two authoring paths (`Polyline` and the `BezPath`
  factory with all `LineTo`s). Slice C tests treat them as equivalent.

### Rejected alternatives
- **Per-shape variants** (`Circle { radius }`, `Arc { ... }`, …). Rejected:
  explosion in IR surface; every new shape becomes a Rust PR.
- **SVG-path string.** Extra parsing layer, no typing benefit over verbs.

---

## C. All 5 track kinds + all 15 easings in one go

### Decision
IR adds `opacity`, `rotation`, `scale`, `color` tracks alongside Slice B's
`position`; all 15 easings from `manimlib/utils/rate_functions.py` ship with
struct-variant parameters.

### Why
- Each easing is 1–5 lines; incremental adoption would have meant re-touching
  the internally-tagged union repeatedly.
- 5 track kinds cover the manimgl authoring vocabulary comfortably; further
  tracks (stroke width, dash offset) are rare and can come later.
- Struct variants dodge ADR 0002's unit-variant trap cleanly for
  parameterized easings (`Overshoot { pull_factor }` etc.).

### Consequences
- Composition rule per track kind is explicit in the evaluator:
  position/rotation **sum**, opacity/scale **multiply**, color takes the
  latest-started segment. See `docs/porting-notes/rate-functions.md`.
- IR surface grew ~2× but all of it is concept-closed — Slice D need not
  revisit.

### Rejected alternatives
- **Opacity + rotation only.** Satisfies the multi-track constraint but
  would have forced another bump in Slice D for scale/color.
- **Easings as a 15-arm enum without params.** Fails ADR 0002 for
  parameterized easings.

---

## D. Fill + 4× MSAA as a paired shipping unit

### Decision
Fill pipeline (`lyon::FillTessellator`, `NonZero` winding) ships together
with 4× MSAA on the color target. Both reach the integration scene in the same
slice.

### Why
- Stroke-only output looks unfinished — the integration test must visibly
  render filled shapes.
- Without MSAA, filled shape edges alias worse than stroked ones (the fill
  pipeline has no AA analogue to `lyon`'s stroke joint handling). MSAA is the
  cheapest way to make fills look shippable before Slice D's real stroke port.
- Pairing them means one round of wgpu render-target plumbing, not two.

### Consequences
- `COLOR_FORMAT` color target is now a 4-sample texture with a separate
  resolve target; every render pass carries `resolve_target: Some(...)`.
- Platform-exact pixel checksums die — see decision E.
- Non-zero winding is authored in. Self-intersecting paths render the winding
  interpretation, not the even-odd one; documented in
  `docs/porting-notes/fill.md`.

### Rejected alternatives
- **Fill first, MSAA in Slice D.** Rejected: integration scene would need
  two rounds of snapshot tuning.
- **Analytic fill AA in the fragment shader.** Correct answer for Slice D's
  real stroke port; too much lift for Slice C.

---

## E. Tolerance-based raster snapshots

### Decision
Pixel snapshot tests assert sum-within-±N and nonzero-count-within-±N rather
than pinning exact RGBA byte values. The previous exact-value snapshots in
`crates/manim-rs-raster/tests/snapshot.rs` are migrated.

### Why
- MSAA sample positions are backend-dependent (Metal vs Vulkan vs D3D12);
  exact pixel values drift across platforms.
- Cross-platform wheels (parallel workstream) will hit this the moment they
  add a non-macOS CI job.
- "Looks right" is the test we actually care about; exact bytes are a proxy
  that gets expensive to maintain.

### Consequences
- Snapshot tests no longer catch single-pixel regressions. Mitigation: the
  integration scene's per-object centroid tests (color-band masks) catch
  larger-scale regressions a sum-tolerance can miss.
- Test matrix is platform-neutral; cross-platform CI does not need per-backend
  snapshot baselines.

### Rejected alternatives
- **Per-platform exact baselines.** O(platforms) maintenance burden and
  strong temptation to ignore backend drift rather than investigate it.
- **Perceptual diff (SSIM / flip).** Right answer eventually; dependency weight
  not worth it for Slice C's small canvases.
