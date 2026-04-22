# Gotchas

Things that bit us during this codebase that aren't obvious from reading code, dependencies' docs, or upstream tutorials. Each entry: **what**, **why**, **where the fix lives**.

Check this file before starting a session in the relevant area. Add an entry any time you lose more than ~15 minutes to a non-obvious trap.

---

## Rust / pyo3 / maturin

### pyo3 0.23 uses `allow_threads`, not `detach`

`py.detach(|| { … })` appears in later pyo3 versions' docs. **0.23 still exposes `py.allow_threads(|| { … })`.** If you're mimicking an external snippet (or a stale slice plan) that uses `detach`, you'll get a compile error.

Fix lives in: `crates/manim-rs-py/src/lib.rs:49`.
Verify by: `grep -R 'fn allow_threads\|fn detach' ~/.cargo/registry/src/*pyo3*` (or wherever cargo caches pyo3).

### pythonize returns tuples for fixed-size Rust arrays

Rust `[f32; 3]` (the `Vec3` alias) round-trips through `pythonize` as a Python **tuple**, not a list. Tests that compare `state["objects"][0]["position"] == [0.0, 0.0, 0.0]` fail with the diff `(0.0, 0.0, 0.0) == [0.0, 0.0, 0.0]`. `msgspec.to_builtins` on the *input* side also produces tuples for `Vec3`, so the shape is tuple-in / tuple-out end-to-end.

Fix: write assertions as tuples (`== (0.0, 0.0, 0.0)`) or normalize with `list(...)` before comparing. Example: `tests/python/test_eval_at.py`.

### Shell-chaining + venv cwd trap

`cd /foo && cargo test ; source .venv/bin/activate` can fail because the activation step resolves `.venv` relative to whatever cwd the shell lands in after the previous command, not the repo root. Agents using `Bash` with `;`-separated chains have hit this.

Fix: **use the absolute path** — `source /Users/chcardoz/conductor/workspaces/manimax/islamabad/.venv/bin/activate` — or keep activation as its own first command and chain the rest with `&&`.

---

## wgpu 29

### API deltas from most online examples

Most wgpu tutorials target 0.20–0.22. wgpu 29 moved several signatures. The ones that bit Slice B's raster crate:

| Thing | wgpu ≤ 22 | wgpu 29 |
|---|---|---|
| `Instance::new` | takes `&InstanceDescriptor` | takes `InstanceDescriptor` (value) |
| `RenderPassDescriptor` | no `multiview_mask` field | **requires** `multiview_mask` |
| `RenderPipelineDescriptor` | `multiview: None` | `multiview_mask: None` |
| `PipelineLayoutDescriptor` | `push_constant_ranges: &[]` | `immediate_size: 0` |
| `PipelineLayoutDescriptor::bind_group_layouts` | `&[&BindGroupLayout]` | `&[Option<&BindGroupLayout>]` |

Fix lives in: `crates/manim-rs-raster/src/lib.rs` and `crates/manim-rs-raster/src/pipelines/path_stroke.rs`. If you're adding a new pipeline, compare against those files, not against tutorials.

### `queue.write_buffer` is ordered before *all* submitted commands — don't reuse one buffer across multiple passes in a single submit

wgpu's queue semantics: every `queue.write_buffer(buf, offset, bytes)` call within a submission is scheduled to happen **before any command buffer executes**. Writes to the same `(buf, offset)` are applied in order — last write wins. The writes do **not** interleave with render passes recorded into an encoder.

Concretely, this pattern renders **only the last object**:

```rust
for obj in &state.objects {
    queue.write_buffer(&vertex_buf, 0, obj_vertices);     // each call overwrites
    queue.write_buffer(&uniform_buf, 0, obj_uniforms);    // the previous
    encoder.begin_render_pass(...).draw(...);              // all passes share buffers
}
queue.submit(encoder.finish());   // writes applied first, last-wins; then all N passes
```

All N render passes execute against the final state of the buffers, so every pass draws object N.

**Fixes (in order of effort):**
1. **Submit per object.** One command encoder per iteration; `queue.submit(Some(encoder.finish()))` inside the loop. Forces writes and passes to interleave as authored. This is what `Runtime::render` does today — see `crates/manim-rs-raster/src/lib.rs`.
2. **Per-object buffers.** Allocate `N` vertex/index/uniform buffers up front; each pass references its own. No repeated writes.
3. **Pack all geometry upfront.** Tessellate every object first, concatenate into one big vertex/index buffer with per-object offsets, then record passes that each draw their slice via `draw_indexed(offset..offset+count, ...)` and dynamic uniform offsets. One submit, one buffer.

Slice B uses #1. Slice C should upgrade to #3 if per-frame submit count becomes a bottleneck.

Regression test: `crates/manim-rs-raster/tests/multi_object.rs` renders two separated squares and asserts both show up. This test fails against the old single-submit code.

### 256-byte row alignment on readback

wgpu requires `bytes_per_row` for buffer↔texture copies to be a multiple of 256. At 480×4 bpp, the natural row is 1920 B and you must pad to 2048 B. On the CPU readback path, strip the padding back out.

Fix lives in: `crates/manim-rs-raster/src/lib.rs` — `align_up` + the `readback_pixels` row strip.

---

## Serde / IR schema

### `deny_unknown_fields` is silently ignored on unit variants under an internal tag

`#[serde(tag = "kind", deny_unknown_fields)]` on an enum does nothing for **unit** variants (`Linear,`) — extras pass through without an error. Use empty-struct variants (`Linear {}`) instead. Wire format is identical; enforcement actually works.

Same rule on the Python side: msgspec Structs that carry only a tag must still be a class body, not a bare tag constant.

Caught by: the parametrized unknown-field test matrix in `tests/python/test_ir_roundtrip.py` (21 sites as of Slice C). See also ADR 0002's addendum.

### Round-trip equality on f32 fields needs dyadic rationals

IR scalar fields (e.g. `Easing::ExponentialDecay.half_life`, `Stroke.width`) are `f32` in Rust but `float` (f64) in Python. The wire path is: Python f64 → ryu shortest decimal → serde f32 → ryu shortest decimal → msgspec f64. For values like `0.1` or `1.0/3.0`, the f32 round-trip produces a *different* f64 bit pattern (e.g. `0.3333333333333333` goes to `0.33333334`), so structural `==` on decoded msgspec structs fails even though the wire contract is sound.

Fix in tests: use dyadic rationals (`0.25`, `0.125`, `0.5`, `2.0`) for any float parameter that a round-trip test compares with `==`. Don't "fix" this by switching fields to `f64` — f32 is correct for graphics-adjacent values; the issue is the test fixture, not the schema.

Caught by: `test_every_easing_roundtrips_through_rust` flagging `ThereAndBackWithPauseEasing(pause_ratio=1.0/3.0)` during Slice C Step 2.

---

## Testing / verification

### ffprobe is the ground truth for video tests

`ffprobe -v error -select_streams v:0 -count_frames -show_entries stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames -of default=noprint_wrappers=1 <path>` returns deterministic, parseable output. Use it over file-size heuristics or human eyeballing.

The invocation is duplicated across `crates/manim-rs-runtime/tests/end_to_end.rs`, `tests/python/test_render_to_mp4.py`, and `tests/python/test_cli.py`. If it starts drifting, extract a helper.

### ffprobe says "valid mp4" ≠ frames have content

`ffprobe`-only tests confirm the container is well-formed (dims, codec, framerate, frame count). They do **not** confirm anything was actually drawn. Slice B's `end_to_end.rs` and `test_render_to_mp4_produces_valid_file` both passed for a session while the mp4 was all-black at 128×72.

Complement at least one end-to-end video test with a decoded-frame pixel check: `ffmpeg -i <mp4> -vframes 1 -f rawvideo -pix_fmt rgba -` and assert on the raw bytes. Example in `tests/python/test_render_to_mp4.py::test_render_to_mp4_frame0_has_content_at_origin`.

### yuv420p crushes thin strokes below threshold at small canvases

A 0.1-unit stroke on the canonical Slice B scene (128×72, `Rgba8UnormSrgb` → `-c:v libx264 -pix_fmt yuv420p`) decodes back to **all zeros** — h264/yuv420p chroma subsampling + range compression eats low-coverage strokes.

Pixel-check tests that assert "bright pixels exist somewhere in frame 0" need a recognizable signal: either bump stroke width (≥ 0.15) or bump canvas size (≥ 480×270). The centroid test in `test_render_to_mp4.py` uses both.

If you need to test that the *renderer* output survives, split the test: one hits `Runtime::render` directly (no encoder), one hits `render_to_mp4` with a beefier stroke.

### Pixel-exact snapshot constants are platform-pinned

`crates/manim-rs-raster/tests/snapshot.rs` pins an RGBA byte-sum and non-zero count for the canonical Slice B scene. Values are mac arm64 + Metal + wgpu 29. On a different backend (Vulkan on Linux, D3D12 on Windows) they will drift — that's expected, not a bug. Update under scrutiny: verify the *kind* of drift (all channels scaled uniformly vs. only some pixels changing) before bumping the constants.

### lyon dedupes sub-epsilon stroke points — circles can't force overflow

A unit circle with 500 000 tessellation points collapses to ~282 vertices in lyon because neighbours are sub-epsilon apart. If you're calibrating a `GeometryOverflow` test (or any "what if input is huge" tessellator test), use non-coincident points — e.g. a zigzag `[-5 + 10i/n, ±1, 0]` forces every segment to a distinct diagonal that lyon can't merge.

Working calibration: zigzag @ n=3000 → 6002 vertices = 96 032 B > 64 KiB vertex cap. See `crates/manim-rs-raster/tests/edge_cases.rs::oversized_polyline_returns_geometry_overflow`.

---

## Python / typer

### typer auto-flattens single-subcommand apps

If you register exactly one `@app.command()`, typer drops the subcommand name — `python -m pkg render ARG` becomes `python -m pkg ARG`, breaking any documentation that shows the subcommand. **Add a no-op `@app.callback()`** to force typer to keep subcommands.

Fix lives in: `python/manim_rs/cli.py` — the `_root` callback. Delete it only once a second subcommand exists.

---

## Evaluator

### Gap-clamping on position tracks

When `t` falls in a gap between two position segments (segment N ends at 1.0, segment N+1 starts at 1.2, you ask for `t=1.1`), the evaluator must return **the most recently completed segment's `to`** — *not* the overall last segment's `to`. The bug form: iterating all segments and setting `held = last.to`, then clamping. The correct form: track `held` as iteration proceeds, updating only when a segment's `t1 <= t`.

Fix lives in: `crates/manim-rs-eval/src/lib.rs` — `evaluate_position_track`. Test lives in: `crates/manim-rs-eval/src/lib.rs` (the gap test — named around "held value between segments").

This is the bug a re-implementer is most likely to re-introduce.

---

## Cultural

### Pick "match manimgl" over "what's technically correct" by default

When a decision has a correct answer (e.g. linear color space) and a manimgl-compatible answer (e.g. sRGB floats), **default to the manimgl-compatible one** and write an ADR. Reason: the primary goal is being a drop-in ManimGL replacement; surprising divergences compound into porting pain. Deviate only with a conscious ADR that names what motivates it.

Precedent: the sRGB color decision (Slice B) and Vec3 (rather than Vec2) coordinate decision (Slice B IR). Both cases the "technically right" answer was tempting and both times matching manimgl was the right call.
