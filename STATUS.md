# Status

**Last updated:** 2026-04-24
**Current slice:** Slice D — **shipped**. Plan at `docs/slices/slice-d.md`
(§11 retrospective). ADR at `docs/decisions/0006-slice-d-decisions.md`.
Slice E not yet scoped.

## Last session did

Post-Slice-D cleanup and CI hardening on branch
`chcardoz/review-current-diff` (PR #5):

- **Module splits (pure refactor, logic byte-identical):**
  - `crates/manim-rs-eval/src/lib.rs` split into `state`, `evaluator`,
    `tracks`, `lerp`, `easing` (commit `1824ad5`).
  - `crates/manim-rs-raster/src/lib.rs` extracted `render_object` and
    `pipe_bundle` modules (commit `fd5d142`).
  - `///` doc comments added to public items across encode / ir /
    runtime / raster helpers (commit `c488cde`).
- **CI:** new `.github/workflows/ci.yml` — `ubuntu-latest` +
  `WGPU_BACKEND=vulkan` + mesa-vulkan-drivers (lavapipe). Skips the
  `reference/manimgl` submodule; excludes `manim-rs-py` from
  `cargo test` (same as local invocation). Decision recorded in
  `docs/decisions/0007-ci-linux-lavapipe.md`.
- **Shared test fixtures:**
  - Python: `tests/python/conftest.py::canonical_square_scene` now
    backs `test_eval_at.py` and `test_render_to_mp4.py`. Pixel
    assertions vectorized via numpy.
  - Rust: `crates/manim-rs-runtime/tests/common/mod.rs::short_slice_b_scene`
    shared across `end_to_end.rs` and `cache_behaviour.rs`.
- **Docs:** `docs/performance.md` N15 — warm-cache speedup is ~40% on
  1080p60 long renders (commit `8e98109`). Added a longer multi-act
  `showcase_scene` fixture (commit `d062df6`). Updated
  `docs/gotchas.md` pointers that the eval split invalidated.
- Dropped `test_every_easing_roundtrips_through_rust`; coverage is
  preserved by `test_scene_roundtrips_through_rust` via `_wide_scene`,
  which distributes all 15 easing variants across track types.

## Next action

Scope Slice E. Per `slice-d.md` §9 the natural sequence is text
(cosmic-text + swash) / TeX. Before writing the slice plan:

1. Re-read `docs/architecture.md` §2–§5.
2. Skim `reference/manimgl/manimlib/mobject/{svg,tex,text}/` and
   `manimlib/utils/tex_file_writing.py` for what manimgl's text
   pipeline assumes.
3. Write `docs/slices/slice-e.md` — scope lock first, per Slice D §11
   retro (scope lock is what made D ship cleanly).

## Blockers

- None.
