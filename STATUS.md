# Status

**Last updated:** 2026-04-29
**Current slice:** Slice E — Steps 1–5 shipped, Steps 6–9 remaining.
This branch carries two perf PRs: pixel-cache removal (ADR 0009) and
in-process encoder (ADR 0010), to be landed before resuming Step 6.

## Last session did

Replaced the ffmpeg subprocess (`Command::spawn("ffmpeg") + stdin pipe`)
with in-process libavcodec via `ffmpeg-the-third = "5"`. Motivating
data after cache removal (N17): `encoder::finish` was 631 ms at 4K30
cold — subprocess `wait()` on libx264's b-frame flush was the new tail.

A naive single-threaded in-process encoder *regressed* throughput
(1080p60 cold 3.11 → 3.17 s, 4K30 cold 1.36 → 1.76 s) because the
subprocess was getting free OS-level parallelism. Solution: run the
encoder on a worker thread fed by `mpsc::sync_channel(1)`. The producer
hands an owned `Vec<u8>` per frame and continues immediately.

Changes:

- `crates/manim-rs-encode/src/lib.rs` rewritten end-to-end. Public
  surface (`Encoder::start_with_options`, `push_frame`, `finish`,
  `EncoderOptions`) preserved; `push_frame` now takes
  `Vec<u8>` (was `&[u8]`) so the worker can encode without a
  borrow on the producer side.
- Workspace adds `ffmpeg-the-third = "5"` (LGPL via dynamic linking,
  ffmpeg 8.1 on Homebrew). `ffmpeg-next` 7.x was unusable: bindgen looks
  for `avfft.h`, removed in ffmpeg 8.
- Mux subtleties pinned in code comments + ADR 0010: explicit
  `stream.set_time_base(1/fps)` *before* `write_header`, re-read after
  (mp4 muxer rewrites to `1/15360`); `packet.set_duration(1)` on every
  packet (else `avg_frame_rate` reports `N/(N-1)`); read
  `GLOBAL_HEADER` flag into a local before `add_stream`.
- `SwsContext` is `!Send`, so the scaler is constructed *inside* the
  worker thread closure.
- ADR 0010 records the change.

Verification:

- `cargo test --workspace` green (114 tests across crates,
  encode tests including pixel-roundtrip + the dropped-encoder zombie
  test).
- `maturin develop` rebuilt; `pytest tests/python` 110 passed.
- Trace probes (post-worker-thread) vs subprocess baseline:

  | Workload          | Subprocess | In-process (worker) | Δ      |
  | ----------------- | ---------- | ------------------- | ------ |
  | 1080p60 cold 2 s  | 3.11 s     | **2.56 s**          | −18 %  |
  | 1080p60 warm 2 s  | ~3.10 s    | **2.48 s**          | −20 %  |
  | 4K30 cold 0.5 s   | 1.36 s     | 1.52 s              | +12 %† |
  | 4K30 cold 2.0 s   | ~5.4 s     | 5.69 s              | ≈      |

  † Short 4K runs are encoder-throughput-bound, not architecture-bound:
  per-frame encode is ~60 ms vs ~17 ms raster, so 13 of 15 frames drain
  inside `finish`. Longer runs are at parity with subprocess. The
  follow-on lever is hardware encoding (videotoolbox/nvenc), not more
  pipeline work.

## Next action

After this PR lands, resume **Slice E Step 6**: Tex coverage corpus +
tolerance snapshot pinning.

Perf followups (now reframed):

- N17 (encoder finish tail): closed by 0010 at 1080p; partially closed
  at 4K (architecture neutral, encoder CPU is the wall).
- O1 (cache `Runtime`) is the next big lever for short renders / interactive
  use — unblocks PyRuntime (E3a).
- Hardware encoder (videotoolbox on macOS) is the durable 4K lever; not
  worth it before chunked-parallel (O7) ships.

## Blockers

- None.
