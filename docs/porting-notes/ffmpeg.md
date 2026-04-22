# Porting note: ffmpeg encoder

**Status:** Slice B port.
**Stub label:** `PORT_STUB_MANIMGL_ENCODE` (no stubs remaining — full port; label exists for consistency).
**Manimgl reference:** `reference/manimgl/manimlib/scene/scene_file_writer.py:202-230` (`open_movie_pipe`) @ submodule commit `c5e23d93`.

## What manimgl does

Manimgl spawns ffmpeg as a subprocess and pipes raw RGBA frames into its stdin. The command it builds (from `scene_file_writer.py:213-230`):

```
ffmpeg -y \
  -f rawvideo -s WxH -pix_fmt rgba -r FPS -i - \
  -vf vflip,eq=saturation=S:gamma=G \
  -an \
  -loglevel error \
  -vcodec libx264 \
  -pix_fmt yuv420p \
  <temp_output>
```

Once encoding finishes, the temp file is renamed to the final output path. Manimgl also stitches "partial movie files" (per-animation `.mov` fragments) into the final render via a separate `ffmpeg -f concat` pass, because its replay model produces sections it wants to cache and reuse.

## What Manimax does

Slice B's `Encoder::start` builds:

```
ffmpeg -y \
  -f rawvideo -s WxH -pix_fmt rgba -r FPS -i - \
  -an \
  -loglevel error \
  -vcodec libx264 \
  -pix_fmt yuv420p \
  <output>
```

Written directly to the final path. No temp file, no concat step, one encoder process per render.

## Diffs from the manimgl port, with reasons

| Change | Reason |
|---|---|
| **Drop `-vf vflip`.** | wgpu readback is top-down (row 0 = top of image). Manimgl needs `vflip` because OpenGL FBO readback is bottom-up. ffmpeg's `rawvideo` default interpretation is top-down. If Slice B's end-to-end test shows inverted motion, re-add. |
| **Drop `-vf eq=saturation=S:gamma=G`.** | Manimgl uses these to compensate for its OpenGL color pipeline quirks. We render to `Rgba8UnormSrgb`, which gamma-encodes on write — no post-hoc correction needed. |
| **Drop temp-file-then-rename dance.** | That pattern exists so a crashed render doesn't leave a truncated mp4 at the real path. Our `Drop` impl kills ffmpeg on panic, and we can re-render cheaply from IR, so the risk calculus differs. Slice C can re-add if user feedback demands it. |
| **Hardcode `-vcodec libx264` and `-pix_fmt yuv420p`.** | Slice B accepts no codec flags. Manimgl lets the user pick via `use_fast_encoding()` (which switches to `libx264rgb` + `rgb32`). Slice C or later adds a `--codec` flag. |
| **Drop per-animation partial movie files.** | Manimgl needs these because its replay model has no cheap way to render frames *M..N* of section *k*. Our evaluator's random-access `eval_at` means partial renders are trivial — we don't need to cache intermediate mp4 fragments. |
| **Kill child on `Drop`.** | Manimgl's Python process exiting cleanly terminates stdin and ffmpeg follows naturally. Rust's panic semantics don't guarantee that; the `Drop` impl ensures the subprocess can't orphan. (gotcha §6.5 in `slice-b.md`.) |

## What was kept verbatim

- Input spec: `-f rawvideo -s WxH -pix_fmt rgba -r FPS -i -`.
- `-an` to tell ffmpeg we have no audio stream.
- `-loglevel error` so tests don't drown in ffmpeg's informational output.
- Output container inferred from the path extension (mp4).

## What isn't ported

- Audio pipelines (not in Slice B scope).
- Progress display (`ProgressDisplay` in manimgl). Our renderer is fast enough that Slice B doesn't need one; Slice C can add one if useful.
- Format switches (`use_fast_encoding`, saturation/gamma knobs).
- Section concatenation / "insert" files.

## Verification

Integration test `crates/manim-rs-encode/tests/encode_solid.rs` encodes 30 frames of solid color and shells out to `ffprobe` to confirm:

- `width=480`, `height=270`
- `codec_name=h264`
- `avg_frame_rate=30/1`
- `nb_read_frames=30`

Additional eyeball test on output: `ffprobe` confirms `pix_fmt=yuv420p`, `profile=High`, `duration=1.000000`.
