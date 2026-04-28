# Slice B — Python → IR → N frames → mp4

**Status:** shipped.
**Date:** 2026-04-21.
**Supersedes:** an earlier informal "Slice A" (triangle → PNG), rejected for not exercising the architectural thesis.

This is the first end-to-end vertical slice of Manimax. It proves the full pipeline — Python authoring → typed IR → Rust evaluation → wgpu rasterization → ffmpeg encode — on the smallest possible scene. It deliberately executes `docs/architecture.md` §9 steps 1–9 in order, with every step narrowed to its minimum.

Read `docs/architecture.md` first. This doc assumes it.

---

## 1. Goal

One command produces a valid mp4:

```
python -m manim_rs render out.mp4 --duration 2 --fps 30
```

The mp4 shows a single polyline (a closed square, hardcoded in the scene script) translating horizontally across the frame with `linear` easing. 480×270, 30fps, ~60 frames, RGBA → `yuv420p` via piped ffmpeg. No CLI flags beyond `--duration` and `--fps`.

---

## 2. Scope Decisions (locked)

| Dimension | Choice | Rationale |
|---|---|---|
| Geometry | **Polyline only** (N straight segments, closed or open) | Arch §9 step 1. Defers BezPath / Arc / Circle. |
| Animated property | **Position** (2D translation) only | Simplest track. Exercises eval + interp. |
| Easings | `linear` only | Port one fn from `manimlib/utils/rate_functions.py`. |
| Timeline ops | `add`, `remove` | No `set`, no `reparent`, no `label`. |
| IR wire format | serde_json string over FFI | Debuggable; swap to `FromPyObject`/`pythonize` in Slice C. |
| IR data types | msgspec.Struct (Python) ↔ serde struct (Rust), hand-mirrored | Arch §6. No codegen yet. |
| Rust runtime object | One `#[pyclass] Runtime` holding `wgpu::Device`, `Queue`, pipelines, encoder process | Arch §5 implies; Slice B proves. |
| Render pipeline | **path_stroke only** (no fill) | Closed square outlined in one color. Arch §9 step 7. |
| Tessellation | `lyon_tessellation::StrokeTessellator` on a `Path` of `line_to` segments | Arch §5. |
| Shader | One WGSL: `path_stroke.wgsl`, vertex + fragment, uniform MVP + color | Conceptual port of `manimlib/shaders/quadratic_bezier/stroke/` *without* Bézier math — straight-line stroke only. |
| Camera | Orthographic 2D, hardcoded `[-8, 8] × [-4.5, 4.5]` | Matches manim frame aspect for 16:9. |
| Color space | `Rgba8UnormSrgb` framebuffer + raw `rgba` piped to ffmpeg | wgpu gamma-corrects on write; ffmpeg treats input as sRGB. |
| MSAA | **Off** | Polyline edges will be aliased. Slice C. |
| Readback | Buffer with `COPY_DST | MAP_READ`, 256-byte aligned `bytes_per_row`, strip padding on CPU | Standard wgpu gotcha. |
| Encoder | Piped ffmpeg subprocess, `rawvideo rgba` stdin → `libx264 yuv420p` mp4 | Port `manimlib/scene/scene_file_writer.py:202-230` verbatim, drop `-vf vflip` (wgpu Y is top-down). |
| CLI | `typer`, one subcommand `render` taking `out`, `--duration`, `--fps` | Arch §6. |
| Scene source | **Hardcoded in `__main__.py`** — no user scene file, no discovery | Scene-discovery → Slice C. |
| Platform | macOS arm64 dev box only | CI matrix → Slice C. |

---

## 3. Work Breakdown

Ordered. Each step ends with a testable artifact.

### Step 0 — Repo skeleton

- `Cargo.toml` workspace (edition 2024, resolver 3), `rust-toolchain.toml` pinning 1.95.
- `pyproject.toml` with `maturin` backend, `python-source = "python"`, `module-name = "manim_rs._rust"`.
- Empty `deny.toml`, `.pre-commit-config.yaml` (ruff + cargo fmt only).
- Crates created, compiling empty:
  - `crates/manim-rs-ir/` (deps: `serde`)
  - `crates/manim-rs-eval/` (deps: `-ir`)
  - `crates/manim-rs-raster/` (deps: `wgpu`, `lyon`, `glam`, `bytemuck`)
  - `crates/manim-rs-encode/` (deps: `std::process`)
  - `crates/manim-rs-runtime/` (deps: all of the above)
  - `crates/manim-rs-py/` (cdylib, pyo3, deps: `-runtime` + `-ir`)
- `python/manim_rs/__init__.py`, `_rust.pyi`, `py.typed`.
- `maturin develop` produces an importable package.

**Artifact:** `pytest tests/python/test_import.py` passes on a single-line import test.

### Step 1 — IR v0

- Write `docs/ir-schema.md` with **only** Slice B surface: SceneMetadata, ObjectId, Polyline, `TimelineOp::{Add, Remove}`, PropertyTrack for position with linear easing.
- Rust `manim-rs-ir`: serde structs.
- Python `python/manim_rs/ir.py`: msgspec.Structs mirroring Rust.
- Round-trip test: Python → `msgspec.json.encode` → Rust `serde_json::from_slice` → Rust `serde_json::to_string` → Python `msgspec.json.decode` → structural equality.

**Artifact:** `pytest tests/python/test_ir_roundtrip.py`.

### Step 2 — Python scene recording

- `python/manim_rs/scene.py`: `Scene` with `add`, `remove`, `play`, `wait`, `.ir` property.
- `python/manim_rs/objects/geometry.py`: `Polyline(points: np.ndarray)` — stable ID at construction, stores points, records nothing until `scene.add`.
- `python/manim_rs/animate/transforms.py`: minimal `Translate(obj, delta, duration)`. Under `scene.play(...)` emits a position track with one linear segment.
- No `.animate` proxy, no `AnimationBuilder`. Just `scene.play(Translate(sq, [2, 0, 0], duration=2))`.

**Reference:** `manimlib/scene/scene.py:577` (`play`), `:596` (`wait`) for the clock-advance contract. Reimplement; do not port.

**Artifact:** Python test builds a scene, reads `.ir`, asserts exactly: 1 polyline, 1 add at t=0, 1 position track [0, 2.0] linear.

### Step 3 — Rust evaluator

- `manim-rs-eval::eval_at(ir: &SceneIR, t: f64) -> SceneState`.
- `SceneState` = for each active object, current geometry + current position offset.
- Active = appears in `Add` ≤ t and no `Remove` in (add, t].
- Position = base + sum of track-segment contributions where `segment.t0 ≤ t ≤ segment.t1`, linear.

**Reference:** conceptually replaces `manimlib/animation/animation.py`'s `interpolate`. Rewrite, don't port.

**Artifact:** Rust unit tests at t=0, t=1, t=2, t=3.

### Step 4 — wgpu bringup (highest risk)

- `manim-rs-raster::Runtime::new()` — `Instance` → `Adapter` (HighPerformance) → `Device` + `Queue`. No surface.
- Offscreen color texture `Rgba8UnormSrgb`, 480×270, `RENDER_ATTACHMENT | COPY_SRC`.
- `render_clear(color) -> Vec<u8>` — clear, copy to buffer with 256-aligned `bytes_per_row`, map, strip padding, return 480×270×4 bytes.
- Standalone example: write a PNG, eyeball.

**Risks:** macOS Metal init variance. Padding bug will happen once — fix the unpad helper and done.

**Artifact:** `cargo run -p manim-rs-raster --example clear_png` writes a solid-color PNG.

### Step 5 — Tessellation + stroke pipeline

- `tessellator.rs`: `Polyline { points: Vec<[f32; 2]> }` → `lyon::path::Path` → `StrokeTessellator` → `VertexBuffers<Vertex, u32>`. Vertex = `[pos.xy, uv.xy]` (uv unused for now).
- `pipelines/path_stroke.rs`: wgpu `RenderPipeline`.
- `path_stroke.wgsl`: vertex applies 4×4 MVP uniform; fragment outputs uniform color.
- `render(scene_state, camera) -> Vec<u8>`: per object, tessellate (re-tess every frame for Slice B; cache in Slice C), upload VB/IB, draw.

**Reference:** read `manimlib/shaders/quadratic_bezier/stroke/stroke.vert`/`.frag`; do **not** port literally. Bézier-stroke math not needed for polyline.

**Porting note to write:** `docs/porting-notes/stroke.md` — what manimgl does (AA bezier stroke, per-vertex width) vs. what Slice B does (rigid-width polyline, aliased).

**Artifact:** standalone example renders a hardcoded square, writes PNG.

### Step 6 — Encoder

- `manim-rs-encode::Encoder::start(path, width, height, fps)` — `Popen` ffmpeg with args ported from `scene_file_writer.py:213-230`. Keep stdin pipe on the struct.
- `encoder.push_frame(&[u8])`.
- `encoder.finish()` — close stdin, wait, handle nonzero exit.
- Validate ffmpeg on PATH at `start`; error cleanly.

**Reference:** direct port. Put `scene_file_writer.py:213-230` + commit SHA + one-line note in Rust fn header per CLAUDE.md porting practice #3.

**Porting note to write:** `docs/porting-notes/ffmpeg.md` — pixel format choices, Y-flip (drop `-vf vflip`; verify on first frame), audio (none), partial-movie splicing (skipped).

**Artifact:** Rust integration test hand-constructs 30 solid-color frames, encodes to `/tmp/color.mp4`, `ffprobe` confirms duration/fps/dimensions.

### Step 7 — Runtime glue

- `manim-rs-runtime::render_to_mp4(ir: &SceneIR, out: &Path, fps: u32)`:
  1. Create `Raster::Runtime`.
  2. Create `Encoder`.
  3. For `frame_idx in 0..total_frames`: `t = frame_idx / fps`; `state = eval_at(ir, t)`; `pixels = raster.render(state)`; `encoder.push_frame(pixels)`.
  4. `encoder.finish()`.
- Wrap errors with `thiserror`.

**Artifact:** `cargo run -p manim-rs-runtime --example render_square_mp4` produces a 2s mp4.

### Step 8 — pyo3 binding

- `manim-rs-py::lib.rs`:
  - `#[pymodule] fn _rust(m: &Bound<PyModule>) -> PyResult<()>` — register `Runtime` and `render_to_mp4`.
  - `#[pyclass] struct Runtime` wrapping `raster::Runtime`.
  - `#[pyfunction] fn render_to_mp4(ir_json: &str, out: &str, fps: u32) -> PyResult<()>` — deserialize IR from JSON string, call runtime fn, `py.detach` around the render loop.
- `python/manim_rs/_rust.pyi` stub.

**Why JSON string, not `FromPyObject` yet:** tiny FFI surface for Slice B. Swap to `pythonize`/`FromPyObject` in Slice C once IR shape is stable.

**Artifact:** Python test calls `manim_rs._rust.render_to_mp4(scene.ir.to_json(), "/tmp/out.mp4", 30)` and the mp4 exists.

### Step 9 — CLI + end-to-end

- `python/manim_rs/cli.py` with `typer`: one subcommand `render` with `out`, `--duration`, `--fps`.
- `python/manim_rs/__main__.py`: hardcoded `Scene` with a `Polyline` square that translates right over `duration` seconds, calls Rust render.
- Smoke test the command from §1. Eyeball mp4.

**Artifact:** command from §1 produces a viewable mp4.

---

## 4. Explicitly Out of Scope

Belongs to Slice C+. Resist scope creep:

- Circle, Arc, any non-polyline geometry.
- Fill (only stroke).
- Text / TeX / SVG.
- 3D, surfaces, cameras with phi/theta.
- MSAA, depth buffer.
- `set`, `reparent`, `label`, `camera_set` ops.
- Any easing besides linear.
- Color / opacity / rotation / scale tracks.
- Scene file discovery (`--scene path.py`).
- Quality flags, resolution overrides beyond `--fps`.
- Snapshot cache.
- Parallel / chunked rendering.
- Incremental / partial movie files.
- Glyph atlas, shader hot reload.
- Multi-scene processes.
- Windows / Linux CI.

---

## 5. Success Criteria

- [ ] `maturin develop` builds on a clean checkout after `uv sync && maturin develop` (with `ffmpeg` on PATH).
- [ ] Command in §1 produces `out.mp4`.
- [ ] `ffprobe out.mp4` reports 480×270, ~60 frames, 30fps, h264, yuv420p.
- [ ] Visually: a square outline moves left → right over 2 seconds.
- [ ] IR round-trip test passes (Step 1).
- [ ] Evaluator unit tests pass (Step 3).
- [ ] Encoder integration test passes (Step 6).
- [ ] No panics on happy path; `Ctrl-C` cleanly kills ffmpeg.

---

## 6. Known Gotchas To Pre-Solve

Each costs an hour cold. Pre-empting saves the day:

1. **`bytes_per_row` must be a multiple of 256.** 480×4 = 1920 — already aligned, so Slice B doesn't hit it. Comment the helper so future resolutions don't regress.
2. **wgpu Y is top-down; ffmpeg `-vf vflip` flips again.** ManimGL needs `vflip` because OpenGL FBO read is bottom-up. wgpu readback is top-down. **Drop `-vf vflip`.** Verify on first render — if the square moves the wrong way, flip back.
3. **sRGB framebuffer + rawvideo rgba.** `Rgba8UnormSrgb` writes sRGB bytes; ffmpeg `rawvideo rgba` treats input as sRGB. Should match. If colors wash out, swap to `Rgba8Unorm` + `pow(color, 2.2)` in shader.
4. **Device lost on sleep.** Laptop sleeps → wgpu device invalid. Catch and reinit. Not needed for Slice B (renders in <1s); note for Slice C.
5. **ffmpeg subprocess orphaning.** Rust panic between `start` and `finish` → ffmpeg hangs on stdin. `Drop` on `Encoder` kills the child. Test by panicking mid-loop.
6. **macOS Gatekeeper on `maturin`-built `.so`.** Rare; `xattr -d com.apple.quarantine` fixes it.

---

## 7. Effort Estimate

| Step | Optimistic | Realistic | Pessimistic |
|---|---|---|---|
| 0. Skeleton | 2h | 4h | 1d |
| 1. IR | 2h | 4h | 1d |
| 2. Python scene | 2h | 4h | 1d |
| 3. Evaluator | 2h | 4h | 1d |
| 4. wgpu bringup | 4h | 1d | 2d |
| 5. Tessellation + pipeline | 4h | 1d | 2d |
| 6. Encoder | 2h | 4h | 1d |
| 7. Runtime glue | 2h | 3h | 6h |
| 8. pyo3 | 2h | 4h | 1d |
| 9. CLI + E2E | 2h | 3h | 1d |
| **Total** | **~3 days** | **~5 days** | **~10 days** |

Assume realistic. Steps 4 and 5 are the volatility; everything else is legwork.

---

## 8. Artifacts Produced Along The Way

Per CLAUDE.md porting practices:

- `docs/ir-schema.md` — IR v0, polyline-only.
- `docs/porting-notes/stroke.md` — stroke pipeline delta vs. manimgl.
- `docs/porting-notes/ffmpeg.md` — encoder delta vs. `scene_file_writer.py`.
- `docs/porting-notes/scene-recording.md` — how `play`/`wait` differ (record vs. run).

Each 200–500 words, as CLAUDE.md specifies.

---

## 9. What Comes After Slice B

Not committed, but the natural sequence:

- **Slice C:** second shape (Circle → exercises BezPath in IR), fill pipeline, a second easing (`smooth`), opacity track, CI matrix (macOS + Linux), swap FFI from JSON-string to `pythonize`/`FromPyObject`.
- **Slice D:** real stroke port from `manimlib/shaders/quadratic_bezier/stroke/` with width attribute + AA. MSAA. Snapshot cache.
- **Slice E:** Text via cosmic-text + swash, glyph atlas. TeX via LaTeX subprocess.
- **Slice F:** 3D — surface pipeline, depth buffer, camera with phi/theta.

This list is a sketch, not a commitment. Revisit after Slice B lands.

---

## 10. Retrospective — what the plan got wrong

Completed 2026-04-21. All §5 success criteria green: 18 Rust tests + 19 Python tests, `python -m manim_rs render out.mp4 --duration 2 --fps 30` produces a valid 480×270 / 60-frame / h264 / yuv420p mp4.

Things this plan predicted badly, surprising calls that landed, and gotchas §6 missed. Future slices' plans should read this before going to press.

### Plan got wrong

- **Step 8 said `py.detach(...)`.** Wrong for pyo3 0.23 — `detach` is a later-version rename. Actually used `py.allow_threads(|| ...)`. Now in `docs/gotchas.md`. Lesson: don't cite symbol names for a pinned dep without grepping the installed source.
- **Step 9 omitted typer's single-subcommand flattening.** `python -m manim_rs render out.mp4 ...` would have rendered `out.mp4` as the first positional arg to the top-level command, silently broken. Fix: add a no-op `@app.callback()`. Now in `docs/gotchas.md` + `cli.py` comment.
- **§6.2 (wgpu Y-top-down)** predicted we'd need to verify pixel direction on first render and potentially re-add `-vf vflip`. Validated on first render as predicted. The pre-empting worked — logging this so future slices copy the "pre-empt the top-3 gotchas" habit.
- **Effort bracket was right on Steps 0–7, generous on Steps 8–9.** The pyo3 binding and CLI took ~2h each, not 4h. Don't over-bracket the final plumbing steps.

### Surprising calls that landed

- **Match-manimgl-over-correct** — we made this call twice (sRGB floats, Vec3 coordinates) under user pushback. Now codified in `docs/decisions/0003-match-manimgl-over-correct.md`. Default rule going forward.
- **Internally-tagged unions with `"op"` / `"kind"` discriminators** — symmetric between serde and msgspec, human-readable in dumps, `deny_unknown_fields`/`forbid_unknown_fields` caught zero bugs *but* that's the point. See `docs/decisions/0002-internally-tagged-unions.md`.
- **JSON string over FFI.** Initially felt too loose for a typed contract; in practice the debuggability + minimal FFI surface won. Locked in by `0001-ir-wire-format-json-string.md`. Revisit when scene sizes grow in Slice C.
- **Evaluator purity paid off immediately.** Zero shared state across `eval_at` calls made the frame loop in `manim-rs-runtime` a 10-line driver. Keep this rule. See `docs/porting-notes/eval.md`.

### Gotchas §6 missed

- **pyo3 0.23 `allow_threads` vs `detach`** (above).
- **typer single-command flattening** (above).
- **Shell-chaining `source .venv/...` with `;` can silently mis-resolve cwd** — cost ~15 min before I switched to absolute paths. In `docs/gotchas.md`.
- **Evaluator gap-clamping** — bug appeared once (held value between two position segments pulled from the overall last segment's `to`, not the most recently completed one). Fixed; documented in `docs/porting-notes/eval.md` as the invariant most likely to regress.
- **wgpu `queue.write_buffer` ordering bug — the slice shipped with a multi-object render regression that no test caught.** The raster loop reused one vertex/index/uniform buffer across N passes in a single submit; writes are ordered before any submitted command buffer, so every pass drew the last object. Discovered post-slice when rendering a two-object proof scene and finding only one shape visible. Fix: submit per object. Regression test: `crates/manim-rs-raster/tests/multi_object.rs`. Gotcha: `docs/gotchas.md`. **Lesson: §5 success criteria covered "the hardcoded single-object scene renders" — they did not cover "multi-object scenes render." Future slices' success criteria must include at least one case that exercises the polymorphism of the IR, not just the demo shape.**

### Process observations

- **One-step-at-a-time cadence worked.** Explain → confirm → implement → update `STATUS.md` → repeat. Never batched steps. Zero rework.
- **`STATUS.md` "rewrite don't append"** stayed small and always current. Good.
- **Porting notes written in-slice** (stroke.md, ffmpeg.md) caught invariants we'd have otherwise had to re-derive. Extended post-slice to add eval.md.
- **Retroactive ADRs would have been cheaper written in-slice.** Three of them landed only after Slice B was done; each was a decision worth ~10 minutes of writing when it was being made.
