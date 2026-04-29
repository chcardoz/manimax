# 0009 — Remove the per-frame pixel cache

**Date:** 2026-04-29
**Status:** accepted

Supersedes the cache-related sections of 0006 (Slice D consolidated
decisions): §C (blake3 cache key), §D (raw-RGBA cache format), §E
(`.manim-rs-cache/` location, atomic writes, no LRU), §F (Python
`cache_dir` + `CacheStats`). Slice D's stroke/cubic decisions in §A and
§B remain accepted.

## Decision

Delete the on-disk pixel snapshot cache (`FrameCache`, `CacheStats`,
`cache::frame_key`, `cache::key_hasher`, `render_to_mp4_with_cache`,
`MANIM_RS_CACHE_DIR`, `.manim-rs-cache/`, the `cache_dir` Python
parameter, `cache_behaviour.rs`, `test_cache_integration.py`).
`render_to_mp4` now goes straight from eval → raster → encode with no
read-side or write-side cache step.

## Why

- **Cold renders are dominated by the cache write itself**, not by the
  work the cache is supposed to skip. Trace probes (`docs/performance.md`
  N16, 2026-04-29): at 1080p60 cold the per-frame `cache::put` is 52.8 ms
  vs raster 7.7 ms; at 4K30 cold it is 340.7 ms vs raster 19.9 ms. The
  cache is more expensive than the thing it caches at every interesting
  resolution.
- **Warm reruns recover only ~40 % of cold time** (N15: 14.9 s → 9.0 s
  on a 1080p60 9.5 s scene). The encoder-pipe + readback floor is the
  true warm cost and the cache cannot lower it.
- **Raw RGBA is enormous and uncompressed.** A 1080p60 30 s scene is
  ~11 GB of cache files (D6); a 4K version is ~44 GB. Disk fills before
  the cache earns its keep.
- **Most real reruns invalidate the cache anyway** — author edits a
  scene, changes parameters, or sweeps quality settings, all of which
  change `SceneState` or `metadata` and miss every frame.
- **The mp4 already is the durable artifact.** Re-encode-only workflows
  can be served by reading frames back out of mp4 if/when they become a
  real workflow; we don't need a parallel raw store on speculation.
- **Random-access frame requests** (the original architectural goal in
  `CLAUDE.md`) are cheap because `eval_at` is 0.13 ms/call (O7); the
  bottleneck for one-frame requests is wgpu/ffmpeg startup (O1), not
  re-evaluation. A pixel cache addresses neither.

## Consequences

- `render_to_mp4(scene, out)` is now a one-shot eval-raster-encode loop
  with no I/O between frames. Smaller code path, no `cache::*` spans,
  no `tempfile`/`blake3` crate deps in `manim-rs-runtime`.
- Cold-render wall time at 1080p60 should drop noticeably (~50 ms/frame
  saved on miss) and at 4K30 should drop substantially (~340 ms/frame).
- Warm rerun wall time goes *up* on scenes that previously hit the
  cache — they now repay raster cost. This is fine: the perf table
  showed warm renders only saved ~40 %, and the renderer is fast enough
  (O7) that paying that back on every run is preferable to maintaining
  the GB-scale on-disk store.
- `CacheStats` disappears from the Python return value; the function
  now returns `None`. Two test suites (`cache_behaviour.rs`,
  `test_cache_integration.py`) and one Python keyword (`cache_dir=`)
  are removed.
- Any scaling story that *does* want raster-skip later (parallel chunked
  rendering, interactive scrubbing, regenerate-on-edit) should be
  considered fresh — `Runtime` caching (O1) and `eval_at` are the
  enabling primitives, not a frame-bytes cache.
- `compile_tex` (Tex parse → tessellated geometry) and any future
  glyph/atlas caches stay where they are: keyed on the source they
  derive from (per ADR 0008 §B), not on `SceneState`. They are small,
  expensive to recompute, and live inside the crates that own the
  derivation.

## Rejected alternatives

- **Keep the cache, drop `sync_all` only (D7).** Saves ~11 ms at 1080p,
  ~26 ms at 4K. Doesn't address `write_all` (40 ms / 314 ms), which is
  the dominant cost. Worth a few percent, not enough to keep the layer.
- **Keep the cache, add a real `--no-cache` / read-only mode.** Just
  papers over the same issue: still pays the write cost by default;
  any caller who needs the perf has to know to opt out. A subsystem
  that needs to be turned off by default in its hot path doesn't earn
  its keep.
- **Keep the cache, switch to PNG/zstd-compressed entries (D6).** ~4–5×
  smaller, but encode CPU on the write path replaces disk bandwidth on
  the write path. Cold-render time probably no better. Adds a decode
  cost on the read path. Hides the actual lever (encoder pipe), as N15
  documented.
- **Keep the cache, add async/deferred writes.** Removes write latency
  from the hot path but doesn't shrink the on-disk store, and adds
  background-thread coordination for a feature whose value is already
  questionable. Not worth the complexity.
- **Keep `CacheStats` and the keying primitives "for later".** YAGNI.
  When a cheaper artifact cache (Tex geometry, glyph atlas) needs
  blake3 keying, it can use `blake3` directly with a typed key — none
  of the `SceneState`/metadata-prefix machinery is reusable for those.
