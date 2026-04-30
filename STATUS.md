# Status

**Last updated:** 2026-04-29
**Current slice:** Slice E — Steps 1–5 shipped, Steps 6–9 remaining.
This branch (`chcardoz/hello`) carries the perf + hardware-encoder
work: pixel-cache removal (ADR 0009), in-process encoder (ADR 0010),
and hardware h264 backend with VT→NVENC fallback (ADR 0011). Ready
for PR; resume Slice E Step 6 after merge.

## Last session did

Added `EncoderBackend::Hardware` to `EncoderOptions`, exposed as
`--encoder hardware` on the CLI. The backend walks `HARDWARE_CANDIDATES`
(`h264_videotoolbox` → `h264_nvenc`) at session start and picks the
first codec present in libavcodec. NV12 pixel format for both. Same
binary works on macOS dev (VT) and Linux+Nvidia deploy (NVENC) without
a code change.

Also rewrote the Python integration test scene to cover every
author-facing surface shipped through Slice E:

- All 3 mobjects (`Polyline`, `BezPath`, `Tex`)
- All 5 BezPath verbs (the green teardrop alone uses every one)
- All 6 transforms (`Translate`/`Rotate`/`ScaleBy`/`FadeIn`/`FadeOut`/`Colorize`)
- 8 representative easings + the 4 Scene API methods
  (`add`/`play`/`wait`/`remove`)

Three-phase 3 s timeline (arrival → flourish → depart-and-remove);
frames 15/45 strict centroid checks, frame 84 existence-only check
catches `FadeOut`/`RemoveOp` regressions.

Verification:

- `cargo test --workspace`: green (5 encoder tests now, was 4 — new
  `hardware_encoder_writes_valid_mp4` smoke gates on
  `BackendUnavailable` and skips silently if no hw encoder is linked).
- `pytest tests/python`: 111 passed (added integration test).
- `--encoder hardware` perf on M-series macOS (VT vs libx264):
  - 4K30 2 s: 10.6 s → **3.4 s** (~3×, CPU 134 % → 82 %)
  - 1080p60 2 s: 2.42 s → 2.14 s (~12 %)

## Next action

After PR lands, resume **Slice E Step 6**: Tex coverage corpus +
tolerance snapshot pinning.

Perf followups (now reframed):

- N17 (encoder finish tail): closed by 0010 at 1080p; closed by 0011
  at 4K via VT.
- O1 (cache `Runtime`): next big lever for short renders / interactive
  use — unblocks PyRuntime (E3a).
- Deploy-time NVENC validation on Modal/fly.io GPU containers (T4/A10/A100):
  out of scope here; pre-wired so the deploy commit is pure infra.

## Blockers

- None.
