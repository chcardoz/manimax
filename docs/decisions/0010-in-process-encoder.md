# 0010 — In-process libavcodec encoder via `ffmpeg-the-third`

**Date:** 2026-04-29
**Status:** accepted

## Decision

Replace `Encoder`'s `Command::spawn("ffmpeg")` + stdin pipe with
in-process libavcodec/libavformat/libswscale via `ffmpeg-the-third`
("the-third" — a maintained fork of `ffmpeg-next`). Encoder runs on a
worker thread fed by a `mpsc::sync_channel(1)`; the producer (raster
loop) hands an owned `Vec<u8>` per frame and continues immediately
once the worker is idle.

## Why

- After the pixel cache was removed (0009), `encoder::finish` became
  the dominant tail at 4K (`docs/performance.md` N17): 631 ms wait on
  ffmpeg subprocess teardown after the last frame was written.
  Subprocess `wait()` is unavoidable while we shell out.
- `stdin` pipe at 4K writes ~33 MB per frame; libavcodec reads from
  in-memory frames, skipping the IPC.
- A naive single-threaded in-process encoder *regressed* throughput
  (1080p60 cold 3.11 → 3.17 s, 4K30 cold 1.36 → 1.76 s) because the
  subprocess was getting free OS-level parallelism — encode in a
  separate process while raster runs. The worker thread restores that
  overlap inside our process.
- `ffmpeg-the-third` over `ffmpeg-next 7.x`: `ffmpeg-next` 7.x bindgen
  fails on Homebrew's ffmpeg 8.x because `avfft.h` was removed; the
  fork's `5.x` releases pin against ffmpeg 8.1.

## Consequences

- New build-time dependency: system ffmpeg dev libs
  (`brew install ffmpeg`, `apt install libav*-dev`). LGPL via dynamic
  linking — do not enable static linking without re-checking the
  GPL implications.
- Encoder is now in-tree Rust: configurable via `EncoderOptions`,
  callers pass owned `Vec<u8>` (not `&[u8]`) so the worker can
  encode without holding a producer-side borrow. Tests and runtime
  glue updated.
- Wall-clock perf (N18, post-worker-thread):

  | Workload          | Subprocess | In-process (worker) | Δ      |
  | ----------------- | ---------- | ------------------- | ------ |
  | 1080p60 cold 2 s  | 3.11 s     | 2.56 s              | −18 %  |
  | 1080p60 warm 2 s  | ~3.10 s    | 2.48 s              | −20 %  |
  | 4K30 cold 0.5 s   | 1.36 s     | 1.52 s              | +12 %† |
  | 4K30 cold 2.0 s   | ~5.4 s     | 5.69 s              | ≈      |

  † Short-run encoder-bound workloads cannot amortize the bounded
  channel: at 4K, per-frame encode (~60 ms) is 3.5× per-frame raster
  (~17 ms), so 13/15 frames drain inside `finish`. With longer runs
  in-process matches subprocess. The architecture isn't the
  bottleneck — libx264 throughput at 4K is.
- A few packet-mux subtleties have to be done explicitly (the muxer
  doesn't infer them from `frame_rate`):
  - Set `stream.time_base = 1/fps` *before* `write_header`, then
    re-read `out_time_base` *after* (mp4 muxer rewrites to `1/15360`).
  - `packet.set_duration(1)` on every packet — without it,
    `avg_frame_rate` reports `N/(N-1)` because the muxer can't infer
    the last frame's display window from PTS alone.
  - Read the format-level `GLOBAL_HEADER` flag into a local *before*
    calling `add_stream`, since `add_stream` holds a mutable borrow of
    `octx` that conflicts with `octx.format()`.
- `SwsContext` is `!Send`, so the scaler and `src_frame` are
  constructed *inside* the worker closure; only the codec context,
  output context, and the bounded channel cross the spawn boundary.
- `Drop` drops the channel sender first to signal EOF, then joins the
  worker. Output mp4 may be missing its trailer if the encoder is
  dropped without `finish()`, but the file handle is released — the
  old subprocess-zombie test (`dropped_encoder_releases_resources`)
  still passes against the same path.

## Rejected alternatives

- **Disable libx264 b-frames to flatten the `finish` tail (`max_b_frames=0`).**
  Tried first: 4K30 cold 1.76 → 1.84 s. Moves work from `finish` into
  per-frame; doesn't reduce total time. Reverted.
- **Stay on the subprocess, async-drain the pipe.** Doesn't avoid the
  subprocess `wait()`; the kernel still has to reap a child that's
  flushing libx264's b-frame buffer. Same tail, more glue.
- **GStreamer / `mp4-rust` / pure-Rust h264 encoder (e.g. `openh264-rs`).**
  GStreamer pulls in a much bigger runtime; pure-Rust h264 encoders are
  either GPL-encumbered (`x264`-as-Rust) or quality-trailing
  (`openh264`'s Cisco encoder). libavcodec is the same encoder we were
  already using via subprocess — switching the *binding*, not the
  encoder.
- **Hardware encoder (`videotoolbox` / `nvenc`).** Worth doing later for
  4K, but not the right first step: it's a quality/format tradeoff and
  splits CI between platforms with and without the hardware path.
  Architectural prerequisite (the in-process pipeline) is what this ADR
  buys; the codec switch is a follow-on lever.
