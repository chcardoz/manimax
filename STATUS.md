# Status

**Last updated:** 2026-05-02
**Current branch:** `chcardoz/montreal-v1`
**Current slice:** post-Slice-E refactor pass (no PR yet)

Seven unmerged refactor commits on top of Slice E (#8, merged 2026-05-01). Tree is clean, no PR opened. All changes are internal cleanup — no IR, no public Python API, no schema changes. Both `pytest tests/python` (136 passes, ~25s) and ruff are green; `cargo test --workspace` not re-run this session.

- `e44a451` raster: tighten visibility, share `FillTessellator`, drop align helper
- `446386e` eval: extract shared `bezpath_to_verbs`, unify compile-cache helper across `tex.rs` / `text.rs` / `evaluator.rs`
- `9f146db` encode: hoist per-frame allocations; typed error variants for spawn/dims
- `3158850` text: hoist `ScaleContext`, simplify the katex_font lock, add `From` impls
- `7e8f493` tex: hoist `ScaleContext`, compose per-item affines into one pass
- `9138ed3` runtime: extract `RenderSetup`, propagate the png error chain
- `30ef0d2` python: share coerce helpers, animation base, test fixtures

The Python pass (`30ef0d2`, 11 files, +380/−487):
- New `python/manim_rs/objects/_coerce.py` exposes shared `vec3` / `rgba` float-coerce helpers; replaces 8 inline triplet/quad coercions across `geometry.py` / `tex.py` / `text.py` / `animate/transforms.py`.
- `animate/transforms.py` collapsed onto a `_SegmentAnimation` base — Translate / Rotate / ScaleBy / FadeIn / FadeOut / Colorize each declare `_VERB` / `_TRACK_CLS` / `_SEGMENT_CLS` and override `_endpoints()`; the per-class `emit()` boilerplate is gone.
- `objects/text.py` derives `_VALID_WEIGHTS` / `_VALID_ALIGNS` from `typing.get_args(ir.TextWeight/TextAlign)` so the runtime check can't drift from the IR `Literal`.
- `tests/python/conftest.py` centralizes `requires_ffprobe` / `requires_ffmpeg` markers, `ffprobe_stream(path, fields) -> dict`, `extract_frame_raw(...)`, `centroid_in_band(...)`. Five test files dropped their hand-rolled copies.
- `test_e2e_text_tex.py` got module-scoped IR-payload fixtures so each example scene compiles once instead of twice.

## Next action

Open one or two PRs against `main` for the unmerged refactor pass — natural split is "Rust refactors" (six commits) and "Python simplify" (`30ef0d2`). Verify `cargo test --workspace` first. After merge, leading slice candidates: Tex snapshot harness mini-slice, Slice E.5 (SVG import), or Slice F (3D pipeline).

## Explicitly deferred (carried from prior session)

- **Tex snapshot harness** (Slice E Step 6 follow-up): corpus + coverage doc shipped, but the parametrized snapshot test, baseline PNGs, `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE` did not. Cross-platform tolerance pinning needs Linux/lavapipe CI runs. Tracked in `docs/public/contributing/porting-from-manimgl.md` "Tex coverage > Snapshot tolerance".
- **Baseline-PNG version of the Text render test.** Same blocker; the centroid-based check is the placeholder.

## Blockers

- None.
