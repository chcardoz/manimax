# Status

**Last updated:** 2026-04-23
**Current slice:** Slice D — **shipped**. Plan at `docs/slices/slice-d.md`
(now includes §11 retrospective). ADR at
`docs/decisions/0006-slice-d-decisions.md`.

## Last session did

- **Slice D Step 7 shipped — slice complete.**
  - `docs/decisions/0006-slice-d-decisions.md` (new): 6-section
    consolidated ADR covering the stroke port (CPU expansion + WGSL
    SDF AA), cubic→quadratic fixed-depth split, stroke-width /
    joint schema, **cache key shape** (hashing evaluated
    `SceneState`, not raw scene + frame index — a deliberate
    course-correction from the plan), raw-RGBA on-disk format +
    atomic writes + no eviction, and the Python surface changes
    (`cache_dir` param + `CacheStats` dict return, deferred
    `--no-cache` CLI flag). Numbered 0006 because 0005 was taken by
    the plain-IR ADR written mid-Slice-C.
  - `docs/slices/slice-d.md`: status flipped to **shipped**, §11
    retrospective filled. 9 retrospective items; the load-bearing
    ones for future agents: (1) cache-key shape deviated from the
    plan — hashing `SceneState` per frame beats hashing scene+index;
    (2) "cold run = every frame misses" is false because content
    addressing lets frames with identical state share entries;
    (3) kill struct aliases the moment semantics diverge, not syntax;
    (4) AA tests need non-axis-aligned shapes; (5) alpha ≠ "pixel
    lit" when background is opaque.
  - No new code; this step was documentation-only.
- Full suites still green from Step 6: `cargo test --workspace
  --exclude manim-rs-py` clean; `pytest tests/python` 97/97 passed.

Slice D in aggregate:

- Real stroke port: `sample_bezpath` + `expand_stroke` in
  `tessellator.rs`; WGSL analytic SDF AA frag shader ported from
  `manimlib/shaders/quadratic_bezier/stroke/frag.glsl @ c5e23d9`;
  `StrokeUniforms { mvp, anti_alias_width, pixel_size }` with color
  moved to per-vertex.
- Per-vertex stroke width + `miter | bevel | auto` joint selection on
  the Python surface; untagged `StrokeWidth` enum keeps scalar
  strokes wire-compatible with Slice C payloads.
- blake3-keyed snapshot cache at
  `crates/manim-rs-runtime/src/cache.rs`: content-addressed, raw
  RGBA, atomic writes, env-overridable dir. `render_to_mp4` returns
  `CacheStats` through pyo3.

## Next action

Slice E. Not committed yet; per `slice-d.md` §9 the natural sequence
is text (cosmic-text + swash) / TeX. Before scoping:

1. Re-read `docs/architecture.md` §2–§5.
2. Skim `reference/manimgl/manimlib/mobject/{svg,tex,text}/` and
   `manimlib/utils/tex_file_writing.py` for what manimgl's text
   pipeline assumes.
3. Write `docs/slices/slice-e.md` — scope lock first, per §11 of
   Slice D's retro (scope lock is what made D ship cleanly).

## Blockers

- None.
