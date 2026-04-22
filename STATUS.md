# Status

**Last updated:** 2026-04-21
**Current slice:** Slice B — shipped and patched; this session expanded test coverage following a post-slice audit.

## Last session did

- **Audited test coverage** against Slice B's surface area and agreed on a plan to plug the gaps. Polyline-specific tests deferred (geometry will grow soon). Upgrade C (FFI `_rust.eval_at` pyfunction) deferred to Slice C's FFI revamp.
- **IR schema fix — `Easing::Linear` → `Easing::Linear {}`.** Parametrized unknown-field test caught that serde's `deny_unknown_fields` silently ignores extras on *unit* variants under an internal tag. Switched to an empty-struct variant across `manim-rs-ir`, `manim-rs-eval`, `manim-rs-runtime` (wire format unchanged). Rejection now works at all seven IR sites.
- **New Rust tests:**
  - `crates/manim-rs-eval/src/lib.rs` — zero-duration segment (jumps to endpoint); multi add/remove cycles (track liveness across re-add).
  - `crates/manim-rs-raster/tests/edge_cases.rs` — empty-scene clear is flat background; degenerate polyline skipped but siblings render; oversized polyline returns `GeometryOverflow` (calibrated zigzag @ n=3000 → 6002 vertices, lyon dedupes circle arcs so they didn't trigger).
  - `crates/manim-rs-raster/tests/snapshot.rs` — pixel-exact RGBA checksum for canonical Slice B scene. Constants (`EXPECTED_SUM=2_350_080`, `EXPECTED_NONZERO=9_216`) are mac arm64 + Metal + wgpu 29; update under scrutiny if they drift.
  - `crates/manim-rs-raster/tests/multi_object.rs` — three-object scene with centroid placement (±3 px) catches off-by-one iteration bugs and position drift.
  - `crates/manim-rs-encode/tests/encode_solid.rs` — dropped-encoder releases resources (reuse path test); solid color survives yuv420p roundtrip (±6/255 per channel at center pixel, alpha pinned to 0xFF).
- **New Python tests:**
  - `tests/python/test_scene_recording.py` — `Polyline` accepts numpy ndarray; `Scene.remove` emits `RemoveOp` at clock; remove-before-add raises; double-add raises.
  - `tests/python/test_ir_roundtrip.py` — `ir.decode(str)` round-trip; 7-site × 2-side parametrized unknown-field rejection matrix (Python msgspec + Rust serde).
  - `tests/python/test_render_to_mp4.py` — frame 0 decoded, centroid sits at pixel (240, 135) for origin-centered square (uses 480×270 + fat stroke because yuv420p crushes stroke 0.1 at 128×72 to sub-threshold).

Totals: **28 Rust tests**, **39 Python tests** (up from 19 + 19).

## Next action

**Scope Slice C** before writing code. Candidates unchanged — scene file discovery, second geometry (Circle/BezPath/Arc), FFI upgrade to `pythonize`, more easings + tracks, MSAA/fill pipeline. Slice C's plan §5 must include at least one end-to-end test exercising multi-object × multi-track × multi-geometry simultaneously.

## Blockers

None.

## Notes for next session

- The centroid test at 480×270 exists because the 128×72 canonical scene's stroke 0.1 is *not* recoverable from yuv420p — decoded frames round to all zeros. If we later add a higher-resolution canonical fixture, fold the centroid check into `_build_scene`.
- The pixel-checksum test in `snapshot.rs` pins hardware-dependent values. First update under scrutiny; second time check for a root cause before bumping.
- Per-object submit in `Runtime::render` still pending an upgrade if frame-rate-sensitive scenes hit submit-count bottlenecks. `docs/gotchas.md` lists two upgrade paths.

## Convention for updating this file

- **Rewrite, don't append.** This file is current-state, not history. Git log is the history.
- Update at the end of every session *before* handing back to the user.
- Keep it under ~50 lines. If it's growing, state is leaking in that should be in `docs/slices/<slice>.md` checkboxes or a porting note.
- Three required sections: **Last session did**, **Next action**, **Blockers**. Everything else is optional.
