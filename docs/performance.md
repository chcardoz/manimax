# Performance notes

Running list of performance observations and ideas that aren't worth acting on in isolation — we'll batch them into a dedicated performance pass once there are enough to group.

Use this as a scratchpad: add observations whenever you see something worth remembering. Remove entries when they're implemented (link to the commit). When in doubt, write it down — memory fades fast.

Each entry should be short: what's slow, why, rough cost/benefit. Not a spec. Not a plan.

---

## Baseline (Slice C, 2026-04-22)

Probe: `scripts/perf_probe.py` running the Slice C integration scene (3 objects: Polyline + BezPath, 5 track kinds, 4 easings). 2 s scene duration. MSAA 4×, h264/yuv420p, macOS arm64, Metal.

| resolution | 30 fps total | 30 fps per-frame |
| ---------- | ------------ | ---------------- |
| 180p       | 0.21 s       | 3.5 ms           |
| 270p       | 0.22 s       | 3.6 ms           |
| 480p       | 0.42 s       | 6.9 ms           |
| 720p       | 0.50 s       | 8.4 ms           |
| 1080p      | 1.10 s       | 18.4 ms          |

Real-time factor: **9× realtime at 270p**, **~1× realtime at 1080p 60fps** (1.83 s for 2 s of scene).

Random-access `eval_at` (no GPU): **0.13 ms per call**. ~1500× faster than rendering one frame end-to-end.

---

## Observations

### O1. Fixed per-call overhead is ~40 ms

Every `render_to_mp4` call creates a fresh wgpu device, compiles pipelines, and starts an ffmpeg subprocess. Shows up as ~40 ms of overhead independent of frame count. Invisible at 270p 2 s (20% of total); devastating at 1 frame (~90% of total).

**Lever:** cache the `Runtime` across calls. Biggest single win for short renders and interactive/preview use cases. Currently deferred per Slice C plan — revisit when a consumer needs it.

### O2. Per-pixel cost is flat — the bottleneck isn't fragment work

180p → 1080p is 36× more pixels but only ~5× more per-frame time. Most of per-frame cost isn't shader work — it's CPU-side setup (buffer writes, command encoding), MSAA resolve, buffer copy out, or encoder pipe I/O.

**Lever:** profile where the 14 ms at 1080p goes before optimizing anything. The shader is probably a small fraction.

### O3. Per-object submits are load-bearing but won't scale

`Runtime::render` submits one wgpu command buffer per object (see the `docs/gotchas.md` entry and the doc comment in `raster/src/lib.rs::render`). This is because `queue.write_buffer` is ordered before all submits in one batch, so reusing one uniform buffer across objects would overwrite earlier writes.

**Observed at 27 objects:** a 4 s 4K 120 fps render of the complex scene issues **~13,000 command-buffer submissions** (27 objects × 480 frames). That's no longer a theoretical cost — on that render it's a meaningful share of per-frame overhead. Scenes with hundreds of objects would be dominated by submission overhead.

**Lever:** switch to a ring of per-object uniform slots with `has_dynamic_offset: true`; one submit per frame. Known wgpu pattern.

### O4. Per-frame tessellation is wasteful

Every frame re-tessellates every object via lyon. In Slice C, objects only change via transform tracks — geometry is static, so we're re-computing the same triangles 60 times per second.

**Lever:** cache tessellated meshes keyed by (object_id, geometry hash). Re-run only when the IR object actually changes. Magnitude depends on geometry complexity — probably ~20% for the integration scene, bigger for scenes with complex BezPaths.

### O5. FPS is almost free

30 fps → 120 fps at fixed resolution is ~1.9× slower, not 4×. Per-frame cost *drops* as fps rises because fixed overhead amortizes.

**Implication:** don't render at 24 fps to save time — barely saves anything. Default to 60 fps when smoothness matters.

### O6. Encoder is a meaningful share at high resolution

ffmpeg runs as a subprocess with stdin piping of raw RGBA bytes. At 1080p × 120 fps × 2 s = ~2 GB piped per render. Subprocess startup + pipe overhead are non-trivial at these sizes.

**Levers:** (a) in-process libavcodec bindings — removes the pipe and subprocess startup. (b) hardware encoders via libav (VideoToolbox on Apple Silicon, NVENC on Nvidia). Both higher implementation cost than O1/O4; only worth it at 1080p+.

### O7. `eval_at` is free — build features on top of it

0.13 ms per call means ~7500 evaluations/sec on one core. This is the architectural win over manimgl (which has to replay from t=0). Features enabled but currently absent:

- Interactive scrubbing / seek-based preview.
- Snapshot cache keyed by `(ir_hash, t)`.
- Parallel chunked rendering: split a long scene across N processes, each renders a frame range, concat-demux via ffmpeg at the end. Each chunk pays the O1 overhead once.

Parallel chunked rendering in particular is the **easy-mode way to make long renders fast without touching the renderer itself** — frames are fully independent.

**Headroom observed:** during the 4K 120 fps stress render, `time` reported **198% CPU utilization** — one render thread plus the ffmpeg subprocess. On an 8–10 core M-series that's ~25% of available cores in use. There's 3–4× parallelism headroom sitting unused before we'd hit a core ceiling, so parallel chunked rendering would translate directly into a 3–4× speedup on long renders.

### O8a. 4K render scales super-linearly — likely encoder pipe I/O

Complex scene (27 objects, ~85 tracks, 4s) at 1080p 120fps projects to ~56 ms/frame from the probe's linear model. Measured at 4K 120fps: 132 ms/frame. 2.3× slower than the linear prediction, suggesting raw-RGBA piping to ffmpeg (~33 MB/frame at 4K) is hitting a ceiling that's not per-pixel shader work.

**Lever:** same as O6 — in-process encoder skips the pipe entirely. At 4K this would likely be a meaningful win. Hardware encoder via libav (VideoToolbox) would compound.

### O8. MSAA sample count is unprofiled

Currently hardcoded at 4×. Haven't measured 1× / 2× / 8×. If MSAA resolve is a meaningful share of per-frame cost (O2 suggests it might be), a quality knob is worth adding.

### O9. Encoder quality/bitrate knobs aren't exposed

The 4K 120 fps complex-scene render produced a **2.9 MB mp4 for 4 s of video — ~5.8 Mbps**. That's very low for 4K (Netflix ships 4K at 15–25 Mbps). We pass no `--crf`, `--preset`, or `--bitrate` to ffmpeg; we're taking the default, which is optimized for fast encode, not quality. Scenes with gradients, many small objects, or motion-heavy content may be getting silently lossy output. Users will notice before they notice any perf issue.

**Lever:** plumb a `--quality` level (or explicit `--crf`) from the CLI through to the encoder. Not a speedup, but a quality knob that prevents "my scene looks bad" bug reports.

### O10. 4K render memory footprint is ~200 MB per call

MSAA 4× at 3840×2160×RGBA = **133 MB** of GPU memory for the color target alone. Plus the single-sample resolve target (33 MB) and the readback buffer (33 MB). Roughly **200 MB per active render call**.

Fine on modern dedicated GPUs and Apple Silicon. Not fine on CI runners with integrated GPUs / limited VRAM, or on older machines. If we ever want Manimax to run in a resource-constrained CI job (e.g. free-tier GHA), this will be the first wall we hit at 4K.

**Lever:** make MSAA sample count configurable (O8) — dropping to 2× cuts the color target to 66 MB. Also worth considering whether the readback buffer should be freed between renders rather than held for the lifetime of the `Runtime`.

### O11. No progress feedback during long renders

The 4K stress test ran for 63 seconds with a silent terminal. No "frame X/480" line, no bar, nothing. Not a perf bug but a perceived-perf / UX one: silent long operations feel broken. A single `\r`-based stderr line updated per frame would cost nothing and remove the "is it hung?" anxiety.

**Lever:** pass a progress callback into `render_to_mp4` and print from the Python side. Trivial to add.

### O12. Post-0005 evaluator-boundary cleanup did not visibly regress render throughput

After ADR 0005 moved `Arc<Object>` out of the IR and into `manim-rs-eval::Evaluator`, the existing `scripts/perf_probe.py` numbers stayed in-family with the Slice C baseline: `eval_at` remained **0.13 ms/call** and the 480p/30 integration render measured **0.22 s** total in the probe (vs 0.42 s/0.22 s style baseline scale, depending on machine load and warm caches). A direct CLI stress render of the integration scene at **4K, 120 fps, 4 s** completed successfully in **28.5 s** (480 frames, valid h264/yuv420p output).

**Implication:** the new plain-IR/compiled-evaluator split appears architecturally cleaner without an obvious throughput penalty on the current integration scene. If a regression hunt is needed later, compare compile-once `Evaluator::new(scene)` against the borrowed convenience path `eval_at(&Scene, t)` directly with a dedicated Rust bench.

---

## New observations (2026-04-22 review pass)

Added after a fresh read across `crates/manim-rs-eval`, `raster`, `runtime`,
`encode`, and the pyo3 boundary. **N9 is the prerequisite** — without
instrumentation we're guessing about relative cost. Everything else is
sequenced on the assumption that gets done first.

### N9. No timing instrumentation anywhere — the meta-fix

`perf_probe.py` measures wallclock totals; there is no per-stage breakdown.
We cannot currently answer "of the 18 ms/frame at 1080p, how much is eval
vs. tessellation vs. GPU submit vs. readback vs. ffmpeg pipe?" Every O-item
above is sized by guesswork.

**Lever:** `tracing` spans at four boundaries in `manim-rs-runtime::render_to_mp4`:
`eval_at`, `raster::render`, `readback`, `encoder::push_frame`. Chrome-trace
JSON dump behind a `--trace-json` CLI flag. One afternoon; gates prioritisation
of O2/O3/O4/O8a.

### N1. `eval_at` allocates fresh collections per frame

`crates/manim-rs-eval/src/lib.rs::active_objects_at` builds a fresh
`Vec<ObjectId>`, `HashSet<ObjectId>`, `HashMap<ObjectId, &Arc<Object>>`, and
final result `Vec` on every call. For a 27-object × 480-frame render that's
~2000 heap allocations just to answer "who's alive?" — and the timeline is
sorted and immutable after `Evaluator::new`.

**Lever:** precompute per-object `(add_t, remove_t)` intervals once in
`Evaluator::new`; eval_at becomes a linear scan with zero allocations.

### N2. `SceneState.objects` Vec is rebuilt and dropped per frame

`Evaluator::eval_at` returns `SceneState` by value; the caller (runtime loop)
drops it and lets the next call allocate a fresh `Vec`. No buffer reuse.

**Lever:** expose `eval_at_into(&mut SceneState)` so the runtime reuses one
allocation across all frames. Trivial. Pairs with N1.

### N3. `evaluate_track` is O(segments) linear per-call, per-track

`crates/manim-rs-eval/src/lib.rs` scans every segment per track on each
`eval_at`. Calls from the render loop are strictly monotonic in `t`, so a
cursor per track would make it amortized O(1).

**Lever:** stateful `FrameEvaluator` that remembers cursor positions across
monotonically-increasing `t`; falls back to `partition_point` for seeky
scrubbing. Worthwhile once scenes have many segments — not yet critical.

### N4. Lyon tessellator + `VertexBuffers` allocated per object per frame

`crates/manim-rs-raster/src/tessellator.rs:157-186` — each call to
`tessellate_stroke_path` / `tessellate_fill_path` does
`VertexBuffers::new()` and a fresh `StrokeTessellator::new()` /
`FillTessellator::new()`. Both tessellator objects are reusable across calls
per lyon's docs.

**Lever:** hold one `StrokeTessellator`, one `FillTessellator`, and
reusable `VertexBuffers<Vertex, u32>` + `VertexBuffers<FillVertex, u32>` on
`Runtime`. Independent of O4 (cache); compounds with it.

### N6. Render + encode are strictly serial

`crates/manim-rs-runtime/src/lib.rs:40-45` — `render` blocks on
`device.poll(wait_indefinitely)`, unpads readback, copies into `Vec`,
`encoder.push_frame(...)` writes to ffmpeg stdin, then frame N+1 starts.
GPU and ffmpeg pipe alternate idle phases. Probably the single biggest 4K
win on top of in-process encoding (O6).

**Lever:** depth-2 pipeline — `std::sync::mpsc::sync_channel<Vec<u8>>(1)` with
a dedicated writer thread owning `Encoder::push_frame`. Frame N+1's render
starts immediately after submit; readback is moved off the main thread.
Pairs with O8a.

### N7. Readback row-by-row copy (4K-sensitive)

`crates/manim-rs-raster/src/lib.rs:479-487` — `extend_from_slice` per row.
At 4K that's 2160 calls per frame. When `padded_bytes_per_row ==
unpadded_bytes_per_row` (widths whose `width*4` is a multiple of 256, e.g.
1920×1080 → 7680) the entire copy collapses to one `extend_from_slice`.

**Lever:** fast path the no-padding case. Sub-ms today; material at 4K.

### N10. `poll(wait_indefinitely)` has no timeout or diagnostic

`crates/manim-rs-raster/src/lib.rs` — if the GPU driver stalls (bug, hang,
driver reset), the render loop waits forever with no log line.

**Lever:** wrap with `wait_for(Duration::from_secs(5))` (or `PollType::Wait`
with timeout in newer wgpu) and log a warning. Cheap defence-in-depth.

### N11. `depythonize_scene` does three tree walks per render

`crates/manim-rs-py/src/lib.rs:37-39` + `python/manim_rs/cli.py:151`:
msgspec builds a Python dict, pyo3's `pythonize::depythonize` walks it into a
`PyAny` tree, and serde reconstructs `Scene` from that. Three passes over
the same data. Cost is **per-render**, not per-frame.

**Lever:** skip the Python dict intermediate — `msgspec.json.encode(scene_ir)
→ bytes → serde_json::from_slice` on the Rust side. One walk instead of
three. Pairs with O1 (caching `Runtime`).

### N12. `eval_at` pyo3 entrypoint rebuilds `Evaluator` per call

`crates/manim-rs-py/src/lib.rs:73-78` — each Python `eval_at(ir, t)` call
re-depythonizes the scene and compiles a fresh `Evaluator`. Fine for one-shot,
but kills the O7 vision of "free interactive scrubbing from Python" — every
frame pays both costs.

**Lever:** expose a `PyEvaluator` class that holds a compiled `Evaluator`
across calls. Cheap once the shape of interactive use is clear.

### N13. Readback buffer residency turns into memory bloat once `Runtime` is cached (O10 follow-up)

If O1 lands (cache `Runtime`), the 33 MB readback buffer at 4K becomes a
permanent ~200 MB resident baseline per cached Runtime. Currently freed
between calls.

**Lever:** pool or shrink-on-idle. Don't silently cement O10's memory
footprint when implementing O1.

### N14. ffmpeg stderr is never drained during encode

`crates/manim-rs-encode/src/lib.rs` — stderr is captured but only read after
`wait()` on encoder finish. Long renders with chatty ffmpeg output
(occasional libx264 warnings even at `loglevel error`) can fill the pipe's
kernel buffer and block ffmpeg on stderr writes — which stalls stdin →
stalls our `push_frame` write → deadlock.

**Lever:** spawn a stderr-drain thread in `Encoder::start` that reads and
discards (or captures into a ring buffer for error diagnostics). Pairs
with O11 (progress output) — both want a stderr reader.

---

## Priority order (if doing a perf pass)

Rough cost/benefit for a future batched pass:

1. **N9 — instrumentation** — prerequisite. Everything below is guesswork until we can measure per-stage time.
2. **O11 — progress output** — trivial, fixes the "is it hung?" UX at high resolutions immediately.
3. **N14 — ffmpeg stderr drain** — cheap, closes a latent deadlock on long renders. Pairs with O11.
4. **O1 — cache Runtime** — cheap, big win for interactive/short renders. Pair with N13 so caching doesn't silently pin 200 MB.
5. **O7 — parallel chunked rendering at CLI** — cheap, 3–4× wins on long renders (real core headroom confirmed).
6. **O9 — encoder quality knob** — cheap, prevents silent quality regressions.
7. **N4 + N1 + N2 — per-frame allocator churn** — small, compounding wins once N9 proves eval/tess time is meaningful.
8. **O4 — tessellation cache** — medium cost, proportional benefit with scene complexity.
9. **N6 — render/encode pipelining** — medium cost, probably the biggest 4K win before O6.
10. **O3 — per-object submit refactor** — medium cost, unlocks many-object scenes (now with concrete 13k-submit evidence).
11. **O6 / O8a — in-process encoder** — higher cost, only worth it at 1080p+.

Items 1–6 are the fastest route to making every real-world render feel snappier and safer.
