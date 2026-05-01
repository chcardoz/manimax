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

### O6. Encoder is a meaningful share at high resolution ✅ partially shipped (2026-04-29, ADR 0010)

(a) shipped: in-process libavcodec via `ffmpeg-the-third` + worker
thread. 1080p −18–20 %, 4K at parity (encoder CPU is the wall, not the
pipe). See N18.

(b) hardware encoders via libav (VideoToolbox on Apple Silicon, NVENC
on Nvidia) remain the durable 4K lever; not done yet.

### O7. `eval_at` is free — build features on top of it

0.13 ms per call means ~7500 evaluations/sec on one core. This is the architectural win over manimgl (which has to replay from t=0). Features enabled but currently absent:

- Interactive scrubbing / seek-based preview.
- Snapshot cache keyed by `(ir_hash, t)`.
- Parallel chunked rendering: split a long scene across N processes, each renders a frame range, concat-demux via ffmpeg at the end. Each chunk pays the O1 overhead once.

Parallel chunked rendering in particular is the **easy-mode way to make long renders fast without touching the renderer itself** — frames are fully independent.

**Headroom observed:** during the 4K 120 fps stress render, `time` reported **198% CPU utilization** — one render thread plus the ffmpeg subprocess. On an 8–10 core M-series that's ~25% of available cores in use. There's 3–4× parallelism headroom sitting unused before we'd hit a core ceiling, so parallel chunked rendering would translate directly into a 3–4× speedup on long renders.

### O8a. 4K render scales super-linearly — likely encoder pipe I/O ✅ partially shipped (2026-04-29, ADR 0010)

Complex scene (27 objects, ~85 tracks, 4s) at 1080p 120fps projects to ~56 ms/frame from the probe's linear model. Measured at 4K 120fps: 132 ms/frame. 2.3× slower than the linear prediction, suggesting raw-RGBA piping to ffmpeg (~33 MB/frame at 4K) is hitting a ceiling that's not per-pixel shader work.

In-process libavcodec (N18) removed the pipe; 4K is now at parity with
the subprocess baseline rather than super-linear, but absolute 4K
throughput is encoder-CPU-bound (per-frame encode ~60 ms vs ~17 ms
raster). Hardware encoder via VideoToolbox is the remaining lever.

### O8. MSAA sample count is unprofiled

Currently hardcoded at 4×. Haven't measured 1× / 2× / 8×. If MSAA resolve is a meaningful share of per-frame cost (O2 suggests it might be), a quality knob is worth adding.

### O9. Encoder quality/bitrate knobs ✅ shipped (2026-04-28)

CLI `--crf 0..51` now plumbs through `_rust.render_to_mp4(..., crf=...)` →
`RenderOptions.encoder.crf` → `Encoder::start_with_options` → `-crf <N>`
on the ffmpeg command line. Default unset preserves prior behavior
(libx264 default crf=23). `--preset` and explicit `--bitrate` not yet
exposed; same plumbing — extend `EncoderOptions` when a caller needs them.

### O10. 4K render memory footprint is ~200 MB per call

MSAA 4× at 3840×2160×RGBA = **133 MB** of GPU memory for the color target alone. Plus the single-sample resolve target (33 MB) and the readback buffer (33 MB). Roughly **200 MB per active render call**.

Fine on modern dedicated GPUs and Apple Silicon. Not fine on CI runners with integrated GPUs / limited VRAM, or on older machines. If we ever want Manimax to run in a resource-constrained CI job (e.g. free-tier GHA), this will be the first wall we hit at 4K.

**Lever:** make MSAA sample count configurable (O8) — dropping to 2× cuts the color target to 66 MB. Also worth considering whether the readback buffer should be freed between renders rather than held for the lifetime of the `Runtime`.

### O11. Progress feedback during long renders ✅ shipped (2026-04-28)

`render_to_mp4_with_cache` now accepts `Option<&mut dyn FnMut(u32, u32)>`;
the pyo3 boundary wraps an optional `Callable[[int, int], None]` that
re-acquires the GIL once per frame (microseconds vs. 10–30 ms render
cost). CLI exposes `--progress/--no-progress` (default on) and prints
`\rframe N/M (P%)` to stderr, throttled to once per percent.

### O12. Post-0005 evaluator-boundary cleanup did not visibly regress render throughput

After ADR 0005 moved `Arc<Object>` out of the IR and into `manim-rs-eval::Evaluator`, the existing `scripts/perf_probe.py` numbers stayed in-family with the Slice C baseline: `eval_at` remained **0.13 ms/call** and the 480p/30 integration render measured **0.22 s** total in the probe (vs 0.42 s/0.22 s style baseline scale, depending on machine load and warm caches). A direct CLI stress render of the integration scene at **4K, 120 fps, 4 s** completed successfully in **28.5 s** (480 frames, valid h264/yuv420p output).

**Implication:** the new plain-IR/compiled-evaluator split appears architecturally cleaner without an obvious throughput penalty on the current integration scene. If a regression hunt is needed later, compare compile-once `Evaluator::new(scene)` against the borrowed convenience path `eval_at(&Scene, t)` directly with a dedicated Rust bench.

---

## New observations (2026-04-22 review pass)

Added after a fresh read across `crates/manim-rs-eval`, `raster`, `runtime`,
`encode`, and the pyo3 boundary. **N9 is the prerequisite** — without
instrumentation we're guessing about relative cost. Everything else is
sequenced on the assumption that gets done first.

### N9. Timing instrumentation ✅ shipped (2026-04-28)

`tracing` spans now wrap `eval_at`, `raster::render`, `readback`,
`encoder::push_frame`, plus a `frame{idx}` span around each frame's
work and a `render_to_mp4` span around the whole call. Follow-up
instrumentation also covers the previously-hidden fixed/setup and cache
boundaries: `raster::Runtime::new`, `encoder::start`,
`encoder::finish`, `cache::key_hasher`, `cache::frame_key`,
`cache::get`, `cache::put`, `cache::put.write_all`,
`cache::put.sync_all`, `cache::put.persist`, and `png::write_rgba`.
Subscriber lives in `manim-rs-py::install_trace_json(path)`; CLI
exposes `--trace-json PATH` on both `render` and `frame` subcommands.
Output format is `tracing-subscriber` JSON (one event per span close),
filterable via `RUST_LOG`. Built-in JSON, not Chrome-trace — a small
post-processor can convert if Perfetto/Chrome ingest is wanted.

This unblocks sizing of O2 (per-pixel), O3 (per-object submit), O4
(tessellation cache), N6 (render/encode pipeline). Next perf pass:
collect trace data on a representative scene and re-prioritise.

### N17. Post-cache-removal trace probes (2026-04-29)

Same `tests/python/integration_scene.py` scene, traces collected after
ADR 0009. No `cache::*` spans appear; no `.manim-rs-cache/` directory
is created on any run.

| run | before (cache, N16) | after (no cache) | speedup | frame mean before → after |
| --- | ---: | ---: | ---: | --- |
| 1080p60, 2s cold | 7.91 s | **3.11 s** | 2.5× | 64.7 ms → **18.5 ms** |
| 1080p60, 2s 2nd run | 2.20 s (warm hit-every-frame) | **2.19 s** (no cache to hit) | ~tied | 15.5 ms → 14.2 ms |
| 4K30, 0.5s cold | 6.06 s | **1.36 s** | 4.5× | 371.7 ms → **35.7 ms** |

The 1080p60 first-run win is exactly the `cache::put` cost (~46 ms/frame)
disappearing. The 4K30 first-run win is even larger (~336 ms/frame) for
the same reason — raw-RGBA writes scale linearly with pixel count, so
removing them helps higher resolutions disproportionately.

The 2nd-run case at 1080p60 is tied because the previous "warm" path was
already pipe-bound (every frame: `cache::get` 7.3 ms + `push_frame` 7.9 ms
+ `finish` 311 ms). The new path rasters instead of reading cache, but the
encoder pipe + finish are still the floor — same wall time.

**New visible bottleneck at 4K:** `encoder::finish` is now **631 ms**
(~46 % of 4K30 total wall time). Cold 4K renders are now finish-bound,
not write-bound. The lever is O6 / O8a (in-process encoder skips the
pipe entirely) or running multiple encoder processes in parallel via
chunked rendering (O7).

### N18. In-process libavcodec encoder via worker thread (2026-04-29) ✅ shipped (ADR 0010)

Replaced `Command::spawn("ffmpeg") + stdin pipe` with in-process
libavcodec via `ffmpeg-the-third = "5"`. Encoder runs on a worker
thread fed by `mpsc::sync_channel(1)`; `push_frame` hands an owned
`Vec<u8>` and continues immediately when the worker is idle.

A naive single-threaded in-process encoder *regressed* throughput
(1080p60 cold 3.11 → 3.17 s, 4K30 cold 1.36 → 1.76 s) because the
subprocess was getting free OS-level parallelism. Worker thread restores
the overlap inside our process.

| Workload          | Subprocess (N17) | In-process (worker) | Δ      |
| ----------------- | ---------------- | ------------------- | ------ |
| 1080p60 cold 2 s  | 3.11 s           | **2.56 s**          | −18 %  |
| 1080p60 warm 2 s  | ~3.10 s          | **2.48 s**          | −20 %  |
| 4K30 cold 0.5 s   | 1.36 s           | 1.52 s              | +12 %† |
| 4K30 cold 2.0 s   | ~5.4 s extrap.   | 5.69 s              | ≈      |

† Short 4K runs are encoder-throughput-bound, not architecture-bound:
per-frame encode is ~60 ms vs ~17 ms raster, so 13/15 frames drain
inside `finish` in the 0.5 s case. Longer 4K runs are at parity with
subprocess. The follow-on lever is hardware encoders (videotoolbox /
nvenc) — not more pipeline plumbing.

Closes O6 / O8a for 1080p; partially closes for 4K (architecture neutral,
encoder CPU is the wall).

### N16. Raw RGBA cache writes dominate cold high-res renders ✅ closed by removal (2026-04-29, ADR 0009)

Measured 2026-04-29 with the expanded trace spans on
`tests/python/integration_scene.py`:

| run | total | frame mean | raster mean | cache::put mean | write_all mean | sync_all mean | finish |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1080p60, 2s cold | 7.91 s | 64.7 ms | 7.7 ms | 52.8 ms | 40.4 ms | 11.3 ms | 121 ms |
| 1080p60, 2s warm | 2.20 s | 15.5 ms | n/a | n/a | n/a | n/a | 311 ms |
| 4K30, 0.5s cold | 6.06 s | 371.7 ms | 19.9 ms | 340.7 ms | 313.9 ms | 26.1 ms | 449 ms |

The earlier "missing time" was writing raw RGBA cache entries to disk
on every miss — more expensive than the raster it was meant to skip.
Resolved by deleting the entire on-disk pixel cache (ADR 0009). D2 / D3
/ D6 / D7 (cache-tuning suggestions) and N15 (small warm-rerun speedup)
all close out at the same time. Future raster-skip features should be
considered fresh against `Runtime` caching (O1) and parallel chunked
rendering (O7) instead of a frame-bytes store.

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

### N15. Warm-cache speedup is much smaller than design implies (1080p60 / 9.5s)

Measured 2026-04-23 on the showcase scene (3 objects, 9 s of content) at
1080p60 = 570 frames:

| state                              | wall   | per-frame |
| ---------------------------------- | ------ | --------- |
| cold (`.manim-rs-cache` deleted)   | 14.9 s | 26 ms     |
| warm (every frame already cached)  |  9.0 s | 16 ms     |

A ~40 % speedup, not the ~10× a "skip all GPU work" cache should imply.
570 frames × 8.3 MB (1920·1080·4) = **~4.7 GB streamed through ffmpeg's
stdin pipe per render regardless of cache**, plus per-frame
`serde_json::to_vec(SceneState)` for the hash key (D1) and a full file
read on every hit (D2). That pipe + serialization + disk I/O is the warm
floor.

**Implication:** at 1080p+ the cache hides less than it looks like on
paper. The design wins big when the GPU is the bottleneck (small/cheap
encoders, eval-only use, low-res); it doesn't help much when the
encoder pipe dominates. The fixes already exist as separate entries —
D1 (binary hash format), D2 (`stream_to` skipping the intermediate Vec),
O6 / O8a (in-process encoder, the only thing that removes the 4.7 GB
pipe). This entry exists so a future perf pass can prioritise them with
a concrete number, not "the cache should be faster."

### N14. ffmpeg stderr drain ✅ shipped (2026-04-28)

`Encoder::start` now spawns a background thread that line-reads ffmpeg
stderr into a 64 KiB-capped `Arc<Mutex<String>>`. Cap is "first N bytes
win"; chatty libx264 warnings on long renders can no longer fill the
kernel pipe buffer and deadlock the encoder. `finish` joins the drain
thread after `wait()`; `Drop` does the same on abnormal shutdown.
`NonZeroExit.stderr` reads from the captured buffer instead of
`child.stderr.read_to_string()`.

---

## D1–D7 — Pixel cache tuning ✅ closed by removal (2026-04-29, ADR 0009)

The seven D-series entries (hash format, allocation on hit, size-check
ordering, per-frame tessellation, hit pipelining, cache size, drop
`sync_all`) all targeted the on-disk pixel cache. The cache itself was
removed (ADR 0009 / N16) — D1, D2, D3, D5, D6, D7 are obsolete. D4 is
about per-frame *tessellation* allocation, which is independent of the
pixel cache; left in place below.

## D1 — Cache hash format: JSON vs. direct byte stream

`cache::frame_key` serializes the per-frame `SceneState` with
`serde_json::to_vec` then feeds those bytes into blake3. JSON formatting
(per-`f32` decimal, field names, braces) is several× the size of a
packed binary representation and almost certainly dominates the hash
cost — blake3 itself is much faster than JSON format-then-parse. Already
partially mitigated: the per-render prefix (version, metadata, camera)
is hashed once and `Hasher::clone`'d per frame, so only the changing
`SceneState` pays JSON cost each frame.

**Lever:** implement `hash_into(&mut Hasher)` that writes raw LE bytes
per field (no allocation, no format). `bincode` is a cheap middle ground.

## D2 — Cache hit path allocates a full frame Vec

`FrameCache::get` calls `fs::read(path)`, allocating ≈8 MB at 1080p,
≈33 MB at 4K per hit. The encoder immediately writes via
`stdin.write_all(rgba)`. A `stream_to<W: Write>(key, expected_len, &mut
W)` that opens the file and `io::copy`'s into encoder stdin halves peak
RSS and skips one malloc+memcpy per frame. Pair with D3.

## D3 — Size-check on cache hits reads the whole file first

`FrameCache::get` reads full file → checks length → returns. Switch to
`fs::metadata(&path).ok()?.len() as usize != expected_len` first; only
then `fs::read`. Negligible cost on happy path, skips a large read on
truncation.

## D4 — Per-frame tessellation re-allocates

`tessellate_object` / `sample_bezpath` / `polyline_to_segments` /
`resolve_stroke_widths` allocate fresh `Vec<QuadraticSegment>`,
`Vec<f32>`, and lyon `VertexBuffers` per object per miss frame. Thread
a `TessScratch { segs, widths, fill_verts, fill_idx, stroke_verts,
stroke_idx }` through, `clear()` between objects. `resolve_stroke_widths`
also clones per-vertex widths unconditionally — could return `Cow`.

## D5 — Frame loop is strictly sequential; hits could pipeline

`render_to_mp4_with_cache`: `eval → hash → get → (render if miss) →
(put) → push_frame`. Once the hash is computed, hit-path work has no
data dependency on adjacent frames. Hits could run on a reader thread
feeding the encoder via a bounded channel, and hashes could pipeline
ahead of wgpu submit. Fix D1 first — don't pipeline a hot JSON loop.

## D6 — Cache grows unbounded; raw RGBA is 4× what it needs to be

Documented design choice (ADR 0006 §E). A 1080p60 30s video is ~11 GB
of raw RGBA. Cheap future lever: zstd level-3 on cache entries. ~4–5×
disk savings at modest CPU cost; likely still net positive vs.
re-rendering on hit.

## D7 — `sync_all` before `persist` is stricter than rename(2) needs

`FrameCache::put` calls `tmp.as_file_mut().sync_all()` before
`tmp.persist`. POSIX `rename(2)` is atomic without an fsync — the
sync only protects against power loss, not process crash. Cheap to
drop if "cache entry missing after crash = miss" is acceptable (by
design, it is).

---

## Slice E (text + math) observations

### E1. Lyon fill tolerance is global; em-scaled paths want their own knob

Slice E pinned `FILL_TOLERANCE = 0.001` in
`crates/manim-rs-raster/src/tessellator.rs` (ADR 0008 §D, gotchas).
0.001 is the right answer for em-scaled glyph outlines but
ludicrously over-tessellated for SVG-style geometry where 0.25
would suffice. Tessellation cost scales as `1 / sqrt(tolerance)`
so glyph paths now do ~16× more work than they need to at small
display sizes.

**Lever:** thread tolerance through as a per-Object knob (or per
object-kind default), so SVG-imported paths can keep their cheap
0.25 budget while glyph paths stay at 0.001. Adaptive variant:
derive tolerance from the object's world-space scale × camera
zoom — a glyph rendered at 16-px height doesn't need sub-em
flatness. Both worth a measurement pass once Slice E text scenes
appear in benchmarks; today's text scenes are tens of glyphs and
tessellation cost is invisible against `eval_at` + GPU submit.

### E2. Glyph outlines extracted at 1024 ppem then post-scaled

`crates/manim-rs-text/src/glyph.rs::OUTLINE_PPEM = 1024`. Every
glyph passes through one extra `apply_affine` to scale down to
the caller's requested ppem. Two consequences worth tracking if
text renders ever land in the perf probe:

- **Allocation cost.** `kurbo::BezPath::apply_affine` walks every
  command in place — no extra allocation, but it's an extra
  pass over the path per glyph compile.
- **Cache stability.** `compile_tex` is cached blake3-by-Object
  (ADR 0008 §B); the affine is applied inside the cached compile,
  so warm hits skip it. Cold compiles only — fine.

No action needed today. Listed so a future perf-pass reader
knows the high-ppem extraction is deliberate (hinting avoidance,
ADR 0008 §C) and shouldn't be "optimized away."

### E3a. `PyRuntime` PyClass: hold `Runtime` + `Evaluator` across calls

Today every `render_to_mp4` / `render_frame` / `eval_at` Python entry
point rebuilds the wgpu device, recompiles pipelines, and (for
`eval_at`) recompiles the `Evaluator`. The fixed overhead is the O1
~40 ms baseline (worse for `eval_at` if the scene has many tracks).
This was acceptable for one-shot CLI but blocks every interactive
use case the architecture was supposed to enable: scrubbing, snapshot
batches, regenerate-on-edit, REPL-driven inspection.

**Lever:** introduce a `#[pyclass] struct PyRuntime` exposed as
`manim_rs._rust.PyRuntime` (or similar) holding
`Arc<Mutex<Runtime>>` and `Option<Evaluator>`. Methods:
`render_frame(t, out)`, `render_range(start, end, sink)`, `eval_at(t)`,
`set_scene(ir)`. Per pyo3 docs, the struct must be `Send + Sync`;
`Mutex<Runtime>` covers it. Compounds with O1 (cached `Runtime`) and
N12 (cached `Evaluator`) — they collapse into one fix.

**Trigger:** when a caller (snapshot test corpus, interactive
scrubber, agentic regenerate-on-edit) calls a render/eval entry
point more than ~5 times per process. Slice E Step 6's corpus is
the most likely first trigger; doing this before Step 6 would let
the corpus run without 40 ms × N overhead.

Cost: medium (new pyo3 surface, lifecycle docs, small refactor of
existing `#[pyfunction]`s into thin wrappers). Biggest single
architectural lever in the current codebase.

### E3b. `tracing` instrumentation at four runtime boundaries (prerequisite)

Listed as N9 above with no concrete shape; surfacing it here with
a concrete diff sketch because every E-tier perf decision below it
remains guesswork without it.

**Sketch:**

```toml
# Cargo.toml workspace deps
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json"] }
```

```rust
// crates/manim-rs-runtime/src/lib.rs
#[tracing::instrument(skip_all, fields(t = scene.metadata.duration))]
pub fn render_to_mp4_with_cache(...) { ... }

// inside the frame loop:
let _span = tracing::info_span!("eval_at", t).entered();
// drop, then
let _span = tracing::info_span!("raster::render").entered();
// etc. for readback, encoder::push_frame
```

Plus a `--trace-json` CLI flag in `python/manim_rs/cli.py` that
sets `RUST_LOG=manim_rs=trace` and a `tracing-subscriber` JSON
emitter writing to a file. Chrome-trace viewer ingests this
directly. One afternoon. Gates O2/O3/O4/N6/D1.

**Trigger:** before any of those perf items get touched. Don't
optimize what you haven't measured.

### E3c. PyErr chain at the pyo3 boundary ✅ shipped (2026-04-28)

`manim-rs-py::runtime_err_to_pyerr(RuntimeError) -> PyErr` walks the
`std::error::Error::source` chain and builds a `PyRuntimeError` with
`__cause__` set via `PyErr::set_cause` (inside-out construction so
outer message points to inner cause). Free function rather than
`From<RuntimeError> for PyErr` — orphan rules forbid the latter
since both types live outside `manim-rs-py`. Two call sites
(`render_to_mp4`, `render_frame`) collapsed from
`.map_err(|e| PyRuntimeError::new_err(format!("X failed: {e}")))?`
to `.map_err(runtime_err_to_pyerr)?`. Discharges future-directions F7.

### E4. Bundled-font wheel size (Slice E ship)

Slice E adds `crates/manim-rs-text/fonts/Inter-Regular.ttf` (~310 KB)
plus the `ratex-katex-fonts` bundle (~1.5 MB across the seven KaTeX
TTFs). The pre-Slice-E wheel was small enough that the contribution is
visible: built artifact grew by roughly **2 MB**. ADR 0008 §wheel and
ADR 0012 cover the trade-off.

Levers for a future trim pass: lazy-load fonts via the `font=...`
parameter (i.e. drop the bundle and require users to point at a font
file), or compress at wheel-build time. Neither is worth doing today
— 2 MB on a wheel that already pulls in wgpu (~10 MB compiled
artifacts) is in the noise.

### E5. RaTeX parse+layout cost vs. eval+raster (Slice E ship)

Empirically sub-millisecond per `compile_tex` call for the corpus
expressions tested in Slice E (no expression in the corpus exceeds
~50 glyphs). Two `compile_tex` invocations per Tex (one from
`tex_validate` at Python construction, one from the Evaluator's first
fan-out — see E3 above) still don't show in the trace under
`--trace-json`. The expensive part of a Tex render is the same as
a Polyline render: lyon tessellation of em-scaled paths at
`FILL_TOLERANCE = 0.001`, then GPU dispatch + readback + encode.
Slice E §11 retrospective notes this without a measurement; this
entry pins the observation so a future perf pass doesn't go hunting
for a parser bottleneck that isn't there.

If a corpus run ever shows RaTeX in the hot spans, E3 is the natural
next step (cache the parsed `DisplayList` on the Python object and
hand it to Rust via a parallel pyo3 path).

### E6. `compile_text` / `compile_tex` cache hit rates on the Slice E §1 scenes

Confirmed via the Step 8 cache-probe tests
(`tests/python/test_e2e_text_tex.py` + integration tests in
`crates/manim-rs-eval/src/lib.rs`):

- **Tex:** 100 % hit rate after the first compile within an
  `Evaluator`. The Step 8 combined scene has one Tex source rendered
  across 60 frames → one parse+layout, 59 cache hits. Every
  `Tex(src=…)` instance in a single render compiles once.
- **Text:** Same shape. Step 8's TextScene renders 90 frames sharing
  one shaped layout. The cache is content-addressed by
  `(src, font, weight, size, color, align)` — duplicate `Text(...)`
  instances at different positions or under different transforms
  share geometry via `Arc::ptr_eq`-confirmed pointer equality.

No timing instrumentation through the cache yet; if this ever shows
in a flame graph, the lever is the same one already documented in E3
(skip the `tex_validate` re-parse at construction).

### E7. Determinism of cosmic-text + libx264 across re-renders (Slice E ship)

Step 8 added byte-determinism tests covering Tex, Text, and a combined
Tex+Text+Polyline scene. Two consecutive `_rust.render_to_mp4` calls
with identical IR produce **byte-identical** mp4s. Empirical confirmation
that the eval (HashMap iteration order), cosmic-text shaping, swash
outline extraction, lyon tessellation, MSAA resolve, and the in-process
libx264 encoder are all deterministic in Manimax's current configuration.
Worth flagging in case a future encoder change (parallel slice threading,
psnr-only motion estimation off, etc.) re-introduces nondeterminism —
the test in `tests/python/test_e2e_text_tex.py` is the canary.

### E3. `tex_validate` runs RaTeX parse+layout twice per Tex (once at construction, once at compile)

Python's `Tex.__init__` calls `_rust.tex_validate` after macro
expansion. The Evaluator's first `compile_tex` for the same
source then calls `tex_to_display_list` again — same parser,
same layout pass, no shared state. Both are sub-millisecond for
short expressions; not on any critical path today.

**Lever (latent):** `Tex.__init__` could stash the parsed
`DisplayList` on the Python object and pyo3 could pass it to
`compile_tex` via a parallel path. Adds API surface; only worth
it if profiling shows RaTeX layout dominating one day. Logged
here so we don't forget the duplication exists.

---

## Future architectural direction

### Single general-purpose render entry point (vs. one per output format)

Mid-Slice-E we shipped `render_frame_to_png(scene, out, t)`
alongside the existing `render_to_mp4(scene, out)` (ADR 0008 §F).
Both are useful, but the right shape is a **single
render-N-frames function that's output-format-agnostic** —
PNG, MP4, WebM, GIF, APNG, numbered PNG/JPEG image sequences,
raw RGBA dumps, in-memory `Vec<Vec<u8>>` for tests, and any
future format (AVIF, animated WebP, ProRes, …) all flow through
the same entry point. Today's pair is fine; a third format
shouldn't grow as a third entry point — that's the trigger to
consolidate.

Sketch: `render(scene, frames: impl Iterator<Item=f64>, sink:
&mut impl FrameSink)` where `FrameSink` is `push_frame(rgba) ->
Result<()>` plus lifecycle hooks (`begin`, `finish`). Concrete
sinks:

- `Mp4Sink` / `WebmSink` / `GifSink` / `ApngSink` — wrap ffmpeg
  (or in-process libavcodec eventually) with format-specific
  args.
- `PngSink(path)` — single-frame PNG; today's
  `render_frame_to_png` collapses to `render(scene, [t], &mut
  PngSink::new(out))`.
- `ImageSequenceSink(dir, "frame_%05d.png")` — numbered
  per-frame files, matches manimgl's `--write_images` mode.
- `RawRgbaSink(writer)` — uncompressed framebuffer dump for
  lossless re-encoding pipelines.
- `MemorySink` — collects frames into `Vec<Vec<u8>>` so
  snapshot tests stop choosing between "render to disk and read
  back" vs. "reach into the runtime."
- `CallbackSink(impl FnMut(rgba))` — for users who want to do
  their own thing per frame without writing a Sink impl.

Not urgent. Cost: medium. Triggers: (a) a third format request,
(b) the runtime grows another single-purpose entry point, (c) a
caller wants the rendered bytes without an intermediate file,
(d) someone asks for image-sequence output.

---

## Priority order (if doing a perf pass)

Rough cost/benefit for a future batched pass. Items shipped 2026-04-28
struck out and left in place for cross-reference.

1. ~~**N9 — instrumentation**~~ ✅ shipped 2026-04-28. Trace data now
   collectable via `--trace-json`; sizes the items below.
2. ~~**O11 — progress output**~~ ✅ shipped 2026-04-28.
3. ~~**N14 — ffmpeg stderr drain**~~ ✅ shipped 2026-04-28.
4. **O1 — cache Runtime** — cheap, big win for interactive/short renders. Pair with N13 so caching doesn't silently pin 200 MB.
5. **O7 — parallel chunked rendering at CLI** — cheap, 3–4× wins on long renders (real core headroom confirmed).
6. ~~**O9 — encoder quality knob**~~ ✅ shipped 2026-04-28.
7. **N4 + N1 + N2 — per-frame allocator churn** — small, compounding wins once N9 traces prove eval/tess time is meaningful.
8. **O4 — tessellation cache** — medium cost, proportional benefit with scene complexity.
9. **N6 — render/encode pipelining** — medium cost, probably the biggest 4K win before O6.
10. **O3 — per-object submit refactor** — medium cost, unlocks many-object scenes (now with concrete 13k-submit evidence).
11. **O6 / O8a — in-process encoder** — higher cost, only worth it at 1080p+.

Next perf pass should start with cache policy, not renderer internals:
make cache writes optional/read-only or cheaper, then re-run the same
trace set and pick between O1, O4, N6, O3 based on the remaining span
shape.
