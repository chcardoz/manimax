# Status

**Last updated:** 2026-04-21
**Current slice:** Slice C — planned (`docs/slices/slice-c.md`), Step 1 complete.

## Last session did

- **Wrote Slice C scope.** `docs/slices/slice-c.md`: 5 candidates in (FFI upgrade, second geometry, all tracks, all 15 easings, fill+MSAA), 8-step work breakdown, integration scene as its own step 7, tolerance-based snapshot checksums (supersedes Slice B's platform-pinned ones), cross-platform wheels marked as a parallel workstream. Single consolidated ADR 0004 planned.
- **Step 1 shipped — FFI upgrade.** `pythonize` replaces the JSON-string FFI; `_rust.render_to_mp4` now takes a Python value (dict from `msgspec.to_builtins`). New pyfunction `_rust.eval_at(ir, t)` returns a pythonized `SceneState`.
  - Added `pythonize = "0.23"` to workspace deps.
  - `manim-rs-eval`: added `serde` dep; derived `Serialize`/`Deserialize` on `ObjectState` and `SceneState`.
  - `ir.py`: new `to_builtins(scene)` helper is the canonical FFI prep path.
  - `cli.py` + `test_render_to_mp4.py` migrated off the JSON string. `roundtrip_ir` kept as-is (schema drift guard).
  - New `tests/python/test_eval_at.py` — 6 cases covering start/midpoint/endpoint/past-end/not-yet-live/bad-input.
- **Repo bootstrap codified.** `scripts/setup.sh` (idempotent: submodules → `uv venv` → deps → `maturin develop`), `conductor.json` points `scripts.setup` at it, `AGENTS.md` "First-time setup" section added above "Dev commands." Closes a gap where `.venv` creation was never documented. Conductor note: UI Scripts panel overrides `conductor.json`, so teammates must clear it.
- **Gotchas added:** `pythonize` returns tuples for `[f32; 3]`, not lists — every future `state[...]["position"]` assertion needs tuple syntax.

Totals: **all Rust tests** green (`cargo test --workspace --exclude manim-rs-py`), **45 Python tests** (up from 39; +6 from `test_eval_at.py`).

## Next action

**Slice C Step 2 — IR growth.** Add the `Geometry::BezPath { verbs: Vec<PathVerb> }` variant, new track variants (opacity / rotation / scale / color), and all 15 `Easing` variants (with struct-not-unit bodies per Slice B §10 lesson). Mirror on the Python side in `ir.py`. Extend the 7-site unknown-field rejection matrix to every new site. Update `docs/ir-schema.md`. No evaluator or raster work yet — Step 2 is pure schema + round-trip.

## Blockers

None.

## Notes for next session

- `cli.py` still hardcodes the Slice B demo scene — scene discovery is Step 6. No urgency to touch it during Step 2.
- `snapshot.rs` and other Slice B pixel-exact tests still pin mac-arm64/Metal/wgpu-29 values. Step 4 (fill+MSAA) invalidates these and is when we migrate to tolerance-based; don't pre-emptively rewrite them.
- Step 1's pythonize API path is a gotcha multiplier: `[f32; 3]`, `[f32; 4]`, etc. all become tuples on both FFI edges. Keep it in mind for every new IR field and every test.

## Convention for updating this file

- **Rewrite, don't append.** This file is current-state, not history. Git log is the history.
- Update at the end of every session *before* handing back to the user.
- Keep it under ~50 lines. If it's growing, state is leaking in that should be in `docs/slices/<slice>.md` checkboxes or a porting note.
- Three required sections: **Last session did**, **Next action**, **Blockers**. Everything else is optional.
