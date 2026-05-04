# Status

**Last updated:** 2026-05-04
**Current branch:** `chcardoz/montreal-v1`
**Current slice:** local chunked rendering PR work

Implemented local frame-range parallel rendering for this PR. The default
render path is unchanged (`workers <= 1`). `workers > 1` now splits the
scene's absolute frame interval into disjoint ranges, renders each range as
an independent temp mp4 chunk with its own `Evaluator` / wgpu `Runtime` /
encoder, then concatenates chunks in deterministic frame order with ffmpeg's
concat demuxer.

Public surface added:

- Python API: `render_scene(..., workers=1)`.
- CLI: `python -m manim_rs render ... --workers N`.
- Rust runtime: `render_frame_range_to_mp4(_with_options)` plus chunked
  dispatch through `RenderOptions.workers`.

Docs/tests added:

- Design note: `docs/public/design/local-chunked-rendering.md`.
- Runtime tests for exact range frame count and two-worker chunk concat.
- Python CLI test for `--workers 2`.

Verification this session:

- `cargo test --workspace` passed.
- `source .venv/bin/activate && pytest tests/python` passed: 137 passed.
- `cargo test -p manim-rs-runtime` passed outside the sandbox. The sandboxed
  run failed with `Raster(NoAdapter)` because wgpu could not access a GPU
  adapter; the escalated run passed.
- `cargo test -p manim-rs-py` passed.
- `source .venv/bin/activate && maturin develop` passed outside the sandbox
  after the sandboxed run hit the user uv cache permission wall.
- `source .venv/bin/activate && pytest tests/python/test_cli.py` passed
  outside the sandbox. The sandboxed run failed with the same `NoAdapter`.
- `.venv/bin/ruff check python/manim_rs/api.py python/manim_rs/cli/render.py
  python/manim_rs/_rust.pyi tests/python/test_cli.py` passed.
- Full-scene hardware validation passed using a 75s / 30fps / 1280x720
  `ComplexScene` shim in `/tmp/manimax-parallel-e2e`: default/no workers flag,
  `--workers 1`, `2`, `4`, and `8` all completed with `--encoder hardware`.
  Trace JSON showed 2,250 unique frame spans, no missing/duplicate frames, and
  expected per-worker raster runtime / encoder starts; `ffprobe` confirmed
  every MP4 is H.264 yuv420p, 75s, 30fps, and 2,250 decoded frames.

## Next action

Open the PR for local chunked rendering.

## Blockers

- None.
