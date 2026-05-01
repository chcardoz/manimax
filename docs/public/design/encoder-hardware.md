# 0011 — Hardware h264 encoder: VideoToolbox → NVENC fallback chain

**Date:** 2026-04-29
**Status:** accepted

## Decision

Add `EncoderBackend::Hardware` to `EncoderOptions`, exposed as
`--encoder hardware` on the CLI. When selected, the encoder walks an
ordered `HARDWARE_CANDIDATES` list (`h264_videotoolbox`, then
`h264_nvenc`) and picks the first codec linked into the running
libavcodec. NV12 pixel format is used for both. If none is linked,
`Encoder::start_with_options` returns `EncodeError::BackendUnavailable`
and the caller decides whether to fall back to software.

## Why

- libx264 is the wall at 4K (ADR 0010 N18: ~60 ms/frame at 4K vs ~17 ms
  raster). Architecture work is done; the codec switch is the lever.
- macOS dev machines have VideoToolbox; deploy targets (Modal, fly.io
  GPU containers) have NVENC. Same `--encoder hardware` flag should
  Just Work in both — the binary should pick whichever is present
  without needing a per-platform feature flag or a separate CLI knob.
- Ordering `videotoolbox → nvenc` is "most likely available where this
  binary is currently running" (dev → deploy). VAAPI/AMF can be
  appended later when a deploy target needs them.
- Local-only verification for now: the smoke test
  (`hardware_encoder_writes_valid_mp4`) gates on `BackendUnavailable`
  and skips silently. CI Linux boxes (lavapipe, no Nvidia) stay green;
  macOS dev exercises the VT path. Real perf measurement on T4/A10/A100
  happens at deploy time, not in-tree.

## Consequences

- Wall-clock perf on macOS (M-series, VT) vs libx264:
  - 4K30 2 s: 10.6 s → **3.4 s** (~3× faster, CPU 134 % → 82 %)
  - 1080p60 2 s: 2.42 s → 2.14 s (~12 % faster — encoder is no longer
    the dominant cost at 1080p)
- CRF is intentionally ignored on the Hardware backend. VT and NVENC
  use different quality knobs (bitrate / `-q:v` for VT, `cq` for NVENC);
  not yet plumbed through `EncoderOptions`. Software backend keeps the
  full `--crf 0..51` range.
- LGPL via dynamic linking still holds. NVENC adds an *additional*
  runtime requirement on Linux: `libnvidia-encode.so` from the Nvidia
  driver, plus an ffmpeg build with `--enable-nvenc`. Modal's
  `nvcr.io/nvidia/cuda` images and `jrottenberg/ffmpeg:nvidia-ubuntu`
  satisfy both. The Manimax binary does not link NVENC directly —
  libavcodec does, at ffmpeg build time.
- The fallback chain is *resolution*-only, not *runtime fallback*. If
  VT is linked but fails at session start (e.g. macOS without GPU
  acceleration in a VM), the error surfaces; we don't retry NVENC.
  Runtime fallback would mask config bugs.

## Rejected alternatives

- **Separate `--encoder videotoolbox` / `--encoder nvenc` flags.**
  Forces the caller to know what platform the binary is on. Defeats the
  goal of a single deploy artifact that does the right thing on each
  target.
- **Per-platform Cargo feature gates (`#[cfg(target_os = "macos")]`).**
  Doesn't help: NVENC is available on macOS-built binaries running in
  Linux containers (cross-compiled deploy), and we want the same
  binary to work in both. `find_by_name` runtime probe is the right
  abstraction.
- **Ship a Dockerfile.gpu / Modal smoke test in this PR.** Out of scope
  per "just local stuff for now". Pre-wired so the infra commit is
  pure deploy work, no encoder changes needed.
- **Build our own NVENC wrapper / use `nvidia-video-codec-sdk` directly.**
  Same rationale as 0010's "stay on libavcodec" — switching the
  *binding* layer is correct, switching the *encoder library* is a
  bigger commitment with worse compatibility.
