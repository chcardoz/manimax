# Performance notes

Running list of perf observations and ideas that aren't worth acting on in isolation — batched into a future perf pass once enough accumulate.

Scratchpad rules: short entries (what's slow, why, rough cost/benefit). **Remove entries when they ship** — link the commit if useful. When in doubt, write it down.

---

## Baseline (Slice C, 2026-04-22)

Probe: `scripts/perf_probe.py` running the Slice C integration scene (3 objects, 5 track kinds, 4 easings). 2 s scene, MSAA 4×, h264/yuv420p, macOS arm64, Metal.

| resolution | 30 fps total | per-frame |
| ---------- | ------------ | --------- |
| 180p       | 0.21 s       | 3.5 ms    |
| 270p       | 0.22 s       | 3.6 ms    |
| 480p       | 0.42 s       | 6.9 ms    |
| 720p       | 0.50 s       | 8.4 ms    |
| 1080p      | 1.10 s       | 18.4 ms   |

Realtime factor: **9× at 270p**, **~1× at 1080p 60fps**. Random-access `eval_at` (no GPU): **0.13 ms/call**, ~1500× faster than rendering one frame.

After ADR 0009 (pixel cache removed) the comparable cold runs drop ~2.5× at 1080p60 and ~4.5× at 4K30 by removing per-frame raw-RGBA disk writes; warm runs are ~tied with the prior warm path because the encoder pipe is the floor.

---

## Live observations

### O1. Fixed per-call overhead is ~40 ms

Every `render_to_mp4` rebuilds the wgpu device, compiles pipelines, starts the encoder. Independent of frame count. Invisible at 270p 2 s; devastating at 1 frame.

**Lever:** cache the `Runtime` across calls (paired with N13 so memory doesn't pin). Biggest win for short renders / preview / interactive.

### O2. Per-pixel cost is flat — fragment work isn't the bottleneck

180p → 1080p is 36× more pixels but ~5× more per-frame time. CPU-side setup, MSAA resolve, buffer copy, encoder I/O dominate.

### O3. Per-object submits are load-bearing but won't scale

`Runtime::render` issues one wgpu submit per object (see `gotchas.md` and the doc comment in `raster/src/lib.rs::render`). At 27 objects × 480 frames a 4K 120fps render does **~13,000 submissions** — a meaningful share of per-frame cost. Hundreds of objects would be dominated by submission overhead.

**Lever:** ring of per-object uniform slots with `has_dynamic_offset: true`; one submit per frame.

### O4. Per-frame tessellation is wasteful

Every frame re-tessellates every object via lyon. Geometry is static between transform-only changes.

**Lever:** cache tessellated meshes keyed by `(object_id, geometry hash)`. Magnitude depends on geometry complexity.

### O5. FPS is almost free

30 → 120 fps is ~1.9×, not 4×. Per-frame cost *drops* as fps rises (fixed overhead amortizes). Default to 60 fps when smoothness matters.

### O6. Hardware encoder is the remaining 4K lever

In-process libavcodec via worker thread shipped (ADR 0010), closing O6/O8a for 1080p. 4K is at parity with the subprocess baseline but encoder-CPU-bound (~60 ms encode vs ~17 ms raster). VideoToolbox on Apple Silicon and NVENC on Nvidia are the durable lever.

### O8. MSAA sample count is unprofiled

Hardcoded at 4×. Haven't measured 1× / 2× / 8×. If MSAA resolve is meaningful (O2 hints), a quality knob is worth adding.

### O10. 4K render memory footprint is ~200 MB per call

MSAA 4× at 3840×2160×RGBA = 133 MB color target + 33 MB resolve + 33 MB readback. Fine on dedicated GPUs / Apple Silicon; the wall on resource-constrained CI runners.

**Lever:** make MSAA configurable (O8); 2× cuts the color target to 66 MB.

### O12. Post-ADR-0005 evaluator-boundary cleanup did not regress throughput

After ADR 0005 split the IR from the compiled `Evaluator`, `eval_at` stayed at 0.13 ms/call and the 480p/30 integration render stayed at 0.22 s in the probe. A 4K, 120 fps, 4 s CLI render completed in 28.5 s. No throughput penalty observed for the cleaner architecture.

---

## Per-frame allocator / eval churn

### N1. `eval_at` allocates fresh collections per frame

`eval/src/lib.rs::active_objects_at` builds a fresh `Vec`, `HashSet`, `HashMap`, and result `Vec` on every call. ~2000 heap allocations per 27-object × 480-frame render to answer "who's alive?" — and the timeline is sorted and immutable after `Evaluator::new`.

**Lever:** precompute per-object `(add_t, remove_t)` once in `Evaluator::new`; eval_at becomes a linear scan with zero allocation.

### N2. `SceneState.objects` Vec is rebuilt per frame

`Evaluator::eval_at` returns by value; the runtime drops it and the next call allocates fresh. Trivial fix: `eval_at_into(&mut SceneState)`. Pairs with N1.

### N3. `evaluate_track` is O(segments) per call per track

Linear scan over every segment per track on each `eval_at`. Render loop is monotonic in `t` — a per-track cursor would amortize O(1).

**Lever:** stateful `FrameEvaluator`; falls back to `partition_point` for seeky scrubbing.

### N4. Lyon tessellator + `VertexBuffers` allocated per object per frame

`raster/src/tessellator.rs:157-186` — fresh `StrokeTessellator::new()` / `FillTessellator::new()` and `VertexBuffers::new()` per call. Both tessellator objects are reusable per lyon's docs.

**Lever:** hold one `StrokeTessellator`, one `FillTessellator`, and reusable `VertexBuffers` on `Runtime`. Independent of O4; compounds with it.

---

## Pipelining / readback

### N6. Render + encode are strictly serial

`runtime/src/lib.rs:40-45` blocks on `device.poll`, unpads readback, copies into `Vec`, calls `encoder.push_frame`, then frame N+1 starts. GPU and encoder alternate idle phases. Probably the single biggest 4K win on top of in-process encoding.

**Lever:** depth-2 pipeline via `mpsc::sync_channel<Vec<u8>>(1)` and a writer thread owning `push_frame`. Frame N+1 starts immediately after submit.

### N7. Readback row-by-row copy (4K-sensitive)

`raster/src/lib.rs:479-487` does `extend_from_slice` per row. At 4K = 2160 calls/frame. When `padded == unpadded` (widths whose `width*4` is a multiple of 256, e.g. 1920×1080) the copy collapses to one call.

**Lever:** fast path the no-padding case. Sub-ms today; material at 4K.

### N10. `poll(wait_indefinitely)` has no timeout

If the GPU driver stalls, the loop waits forever silently. Cheap defence-in-depth: `wait_for(Duration::from_secs(5))` and log on timeout.

---

## pyo3 boundary

### N11. `depythonize_scene` does three tree walks per render

`py/src/lib.rs:37-39` + `cli.py:151`: msgspec builds a Python dict, pyo3's `pythonize::depythonize` walks it into a `PyAny` tree, serde reconstructs `Scene`. Three passes, **per-render** not per-frame.

**Lever:** `msgspec.json.encode → bytes → serde_json::from_slice` skips the dict intermediate. One walk. Pairs with O1.

### N12. `eval_at` rebuilds `Evaluator` per call

`py/src/lib.rs:73-78` re-depythonizes and recompiles the `Evaluator` on every `eval_at`. Kills the O7 vision of cheap interactive scrubbing — every frame pays both costs.

**Lever:** expose a `PyEvaluator` class holding a compiled `Evaluator` across calls. Compounds with O1 and N13 (E3a folds them all together).

### N13. Readback buffer residency would cement memory if `Runtime` is cached

If O1 lands, the 33 MB 4K readback buffer becomes a permanent ~200 MB resident baseline per cached Runtime (currently freed between calls).

**Lever:** pool or shrink-on-idle. Don't silently inherit O10's footprint when implementing O1.

---

## Slice E (text + math)

### E1. Lyon fill tolerance is global; em-scaled paths want their own knob

`raster/src/tessellator.rs` pins `FILL_TOLERANCE = 0.001` (ADR 0008 §D). Right for em-scaled glyph outlines, ~16× over-tessellated for SVG-style geometry where 0.25 would suffice. Tessellation cost scales as `1 / sqrt(tolerance)`.

**Lever:** thread tolerance through as a per-Object knob. Adaptive variant: derive from world-space scale × camera zoom — a 16-px glyph doesn't need sub-em flatness. Worth measuring once Slice E text scenes appear in benchmarks.

### E2. Glyph outlines extracted at 1024 ppem then post-scaled

`text/src/glyph.rs::OUTLINE_PPEM = 1024` — every glyph passes through one extra `apply_affine` to scale to the caller's ppem. Deliberate hinting avoidance (ADR 0008 §C); the affine runs inside the cached compile so warm hits skip it. Listed so a future perf-pass reader doesn't "optimize away" the high-ppem extraction.

### E3. `tex_validate` runs RaTeX parse+layout twice per Tex

Python's `Tex.__init__` calls `_rust.tex_validate`; the Evaluator's first `compile_tex` for the same source then calls `tex_to_display_list` again. Sub-millisecond for short expressions; not on any critical path today.

**Lever (latent):** stash the parsed `DisplayList` on the Python object and pass it to `compile_tex` via a parallel pyo3 path. Adds API surface; only worth it if profiling shows RaTeX layout dominating.

### E3a. `PyRuntime` PyClass: hold `Runtime` + `Evaluator` across calls

Today every `render_to_mp4` / `render_frame` / `eval_at` Python entrypoint rebuilds wgpu + recompiles the `Evaluator`. Fine for one-shot CLI; blocks every interactive use case the architecture was meant to enable (scrubbing, snapshot batches, regenerate-on-edit, REPL).

**Lever:** `#[pyclass] struct PyRuntime` exposed as `manim_rs._rust.PyRuntime` holding `Arc<Mutex<Runtime>>` and `Option<Evaluator>`. Methods: `render_frame`, `render_range`, `eval_at`, `set_scene`. Collapses O1 + N12 into one fix. Biggest single architectural lever in the current codebase.

**Trigger:** when a caller (snapshot test corpus, interactive scrubber) hits a render/eval entrypoint more than ~5 times per process.

### E4. Bundled-font wheel size

Inter Regular (~310 KB) + KaTeX TTFs (~1.5 MB) added ~2 MB to the wheel. Acceptable today (wgpu already pulls ~10 MB of compiled artifacts). Future trim levers: lazy-load via `font=...`, or compress at wheel-build time.

### E5. RaTeX parse+layout vs eval+raster

Sub-millisecond per `compile_tex` for the corpus expressions tested in Slice E (no expression > ~50 glyphs). Two `compile_tex` invocations per Tex (one validate at construction + one fan-out) still don't show in the trace. Expensive part is the same as a Polyline render: lyon tessellation at `FILL_TOLERANCE = 0.001`, GPU dispatch, readback, encode. Pinned so a future perf pass doesn't go hunting for a parser bottleneck that isn't there.

### E6. `compile_text` / `compile_tex` cache hit rates

100 % hit rate after first compile within an `Evaluator`. Step 8 combined scene: 60 frames sharing one Tex source → one parse, 59 hits. Same shape for Text. Verified via `Arc::ptr_eq` integration tests in `crates/manim-rs-eval/src/lib.rs`. Cache is content-addressed by `(src, font, weight, size, color, align)` for text; `(src, color)` for tex.

### E7. cosmic-text + libx264 are deterministic across re-renders

Step 8 byte-determinism tests cover Tex, Text, and a combined Tex+Text+Polyline scene. Two consecutive `_rust.render_to_mp4` calls with identical IR produce **byte-identical** mp4s. Empirical confirmation that HashMap iteration, cosmic-text shaping, swash outlines, lyon tessellation, MSAA resolve, and in-process libx264 are all deterministic in the current configuration. The test in `tests/python/test_e2e_text_tex.py` is the canary if a future encoder change re-introduces nondeterminism.

### M1. Local chunked workers don't speed up single-GPU renders (2026-05-04)

Empirical result from the Montreal local-chunked-rendering PR. 75s / 30fps / 1280x720 ComplexScene, hardware encoder, macOS arm64 / Metal, single GPU.

| workers | wall   | frame mean | raster::render | readback | render − readback |
| ------- | ------ | ---------- | -------------- | -------- | ----------------- |
| default | 35.76s | 15.78 ms   | 15.74 ms       | 12.23 ms | ~3.5 ms           |
| 1       | 35.91s | 15.86      | 15.82          | 12.39    | ~3.4              |
| 2       | 35.73s | 31.32      | 31.27          | 27.36    | ~3.9              |
| 4       | 35.67s | 62.62      | 62.57          | 58.49    | ~4.1              |
| 8       | 37.49s | 130.58     | 130.53         | 125.60   | ~4.9              |

Workers DO run concurrently (sum of frame-busy across w4 = 140.9 s in 35.7 s wall ≈ 4× concurrency). What scales 1:1 with worker count is `readback`; raster compute (`raster::render − readback`) is fixed at ~3–5 ms. Conclusion: **the IR-level parallelism is real and correct, but on a single-GPU box the bottleneck is the shared GPU→CPU readback path, not anything chunkable.** ~78 % of every frame is the `copy_texture_to_buffer` + `map_async` + `device.poll(wait_indefinitely)` round-trip in `crates/manim-rs-raster/src/lib.rs:449–474`. Eight VideoToolbox encoder sessions also contend (`encoder::start` 150 ms × 1 → 303 ms × 8).

**Where the IR parallelism does pay off:** across machines (Divita), multi-GPU hosts, or workloads where eval is heavy (eval is currently sub-millisecond — `frame` and `raster::render` busy times are within 0.05 ms).

**Single-machine speedup levers, ranked.** None of these are about more workers.

1. **GPU-side handoff to VideoToolbox via `IOSurface` / `CVPixelBuffer`.** Skip readback entirely on macOS — wgpu writes a Metal texture, VideoToolbox encodes from the same `IOSurface`. Removes the 12 ms/frame that dominates today. Largest available local lever; macOS-specific code path through `wgpu::hal`.
2. **Pipelined / double-buffered readback (extends N6).** Today the CPU thread blocks on `poll(wait_indefinitely)` before frame N+1 starts encoding. With 2–3 readback buffers in flight, frame N readback overlaps frame N+1 draw and frame N−1 encode. Theoretical ceiling ~4 ms/frame (the non-readback part) → ~3–4× speedup with no API changes and no extra GPU pressure. Localized refactor inside `manim-rs-raster`. **Highest-leverage portable change.**
3. **Render at lower resolution for previews.** Readback cost is proportional to pixel count; 1280×720 → 960×540 cuts readback ~2.25×.

Things that explicitly **won't** help on one GPU: more local workers (proven by the table above), parallel software encode (encoder push doesn't show up in traces), threaded eval (already sub-ms).

Trace artifacts: `/tmp/manimax-parallel-e2e/full-{default,w1,w2,w4,w8}.trace.json` (ephemeral; reproduce with `python -m manim_rs render ... --workers N` against a 75s ComplexScene shim).

**Per-worker startup tax (O(N) in worker count).** Two known costs scale linearly with `--workers`, both in `crates/manim-rs-runtime/src/lib.rs` chunked dispatch:

1. **`Scene::clone()` per worker.** Each worker gets a deep clone of the IR (`Scene` and all nested `Vec`/`String` heap data). For text-heavy or math-heavy scenes this is non-trivial. Wrapping `Scene` in `Arc<Scene>` and reworking `Evaluator` to borrow rather than own would make this O(1) per worker.
2. **wgpu adapter + device init per worker.** Each worker calls `Runtime::new`, which runs `instance.request_adapter` + device creation (~50–500 ms each on macOS) and starts cold Tex/Text caches. Sharing one `Instance`/`Adapter`/`Device` across workers (each owning only its render targets) would eliminate this. Cold caches per worker also mean text-heavy chunked renders repeat per-glyph compile work.

Neither is on the per-frame hot path, so the impact is bounded by `N × startup_cost`. Worth revisiting if/when chunked rendering becomes the default or scales beyond 8 workers on one machine.

---

## Priority order (when a future perf pass happens)

Cost/benefit for the live items above:

1. **O1 + N13 — cache `Runtime`** — cheap, big win for interactive/short renders.
2. **N4 + N1 + N2 — per-frame allocator churn** — small, compounding.
3. **O4 — tessellation cache** — medium cost, proportional to scene complexity.
4. **N6 — render/encode pipelining** — medium cost; biggest 4K win after the in-process encoder.
5. **O3 — per-object submit refactor** — medium cost; unlocks many-object scenes (13k-submit evidence).
6. **E3a — `PyRuntime` PyClass** — medium cost; folds O1 + N12 into one fix and unlocks the interactive surface.
7. **O6 (b) — hardware encoders (VideoToolbox / NVENC)** — higher cost; the remaining 4K lever.

Start with cache policy and `Runtime` lifecycle, then re-trace and pick between O4, N6, O3 based on the remaining span shape.
