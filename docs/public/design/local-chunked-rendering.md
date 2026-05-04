# Local chunked rendering

**Date:** 2026-05-04
**Status:** accepted

## Decision

Add local frame-range parallelism by rendering independent mp4 chunks and
concatenating them in frame order with ffmpeg's concat demuxer.

Each worker owns its own `Evaluator`, wgpu `Runtime`, encoder, and temp
chunk file. Ranges are half-open frame intervals (`start..end`) but still
evaluate absolute scene time (`t = frame_idx / fps`). Chunk timestamps start
at zero; concat places chunks sequentially.

## Why

- The IR already makes `frame_at(t)` independent, so frame ranges are the
  natural parallel unit.
- Mp4 chunks avoid shipping raw RGBA frames through the parent process.
- The existing encoder remains unchanged for each range.
- Temp chunk files make failures easy to reason about: rerendering or
  diagnosing a range does not need a distributed protocol.

## Rejected

- **Raw RGBA chunk files.** Simple concat, but huge disk traffic; this
  repeats the removed pixel-cache problem.
- **One shared wgpu runtime.** Lower memory, but the current runtime is not
  shaped for concurrent frame rendering. Independent workers are simpler and
  match the future process-level model.
- **Custom mp4 stitching in Rust.** Avoids shelling out to ffmpeg, but adds
  muxing complexity before the chunked render path proves its value.

## Consequences

- `ffmpeg` must be on `PATH` for `workers > 1`; single-pass rendering still
  uses the in-process encoder only.
- Output chunks must use identical width, height, fps, codec, pixel format,
  and encoder settings for `-c copy` concat.
- Worker count should stay bounded by caller intent and frame count. Each
  4K worker owns roughly the same render-target/readback footprint as a
  full render.

## Empirical results (2026-05-04)

End-to-end test: 75 s / 30 fps / 1280×720 ComplexScene, hardware encoder,
macOS arm64 / Metal, single GPU. Default vs `--workers {1,2,4,8}`.

- **Correctness:** every config produced an H.264 yuv420p mp4 with all 2,250
  frames present, no duplicates, no gaps. Workers ran concurrently (sum of
  per-frame busy time scaled linearly with worker count).
- **Wall time:** flat at ~35.7 s across all configs; w8 was slightly worse
  at 37.5 s.
- **Why no speedup:** ~78 % of every frame is GPU→CPU readback
  (`copy_texture_to_buffer` + `map_async` + `device.poll(wait_indefinitely)`).
  Readback time scaled 1:1 with worker count (12 → 27 → 58 → 126 ms),
  while the non-readback portion of `raster::render` stayed ~3–5 ms in
  every config. Single-GPU readback is the serialized resource;
  IR-level frame independence cannot route around it. VideoToolbox
  encoder-session contention adds a smaller second-order cost.

**The IR-parallelism premise still holds where it matters** — across
machines (Divita), multi-GPU hosts, or eval-heavy workloads. It just
doesn't deliver local speedup on this hardware shape. Single-machine
levers (IOSurface/CVPixelBuffer GPU-side handoff, pipelined readback) are
tracked in `docs/public/contributing/performance.md` entry **M1** and the
roadmap.
