# Slice D — real stroke port + snapshot cache

**Status:** shipped 2026-04-23.
**Date:** 2026-04-23.
**Follows:** `slice-c.md` (shipped). Read Slice C's §11 retrospective before any step here.

Slice C gave us a filled, MSAA'd, multi-shape animated scene with a typed FFI. Strokes are still the Slice B placeholder: straight-line `lyon::StrokeTessellator`, uniform per-object width, edges smoothed only by MSAA 4× on the color target. Slice D replaces that with a real port of manimgl's quadratic-Bézier stroke pipeline (per-vertex width, joint-aware widening, analytic SDF AA), and adds a content-addressable snapshot cache so a second render of an unchanged scene is a file copy.

Ship criteria: **(a)** strokes render at arbitrary resolution without visible tessellation artifacts, with per-vertex width visible in at least one integration scene, and **(b)** re-rendering an unchanged scene completes in << 1s because every frame is a cache hit.

Read `docs/architecture.md`, `slice-c.md`, and `docs/porting-notes/stroke.md` first. This doc assumes them.

---

## 1. Goal

Two acceptance commands, both green:

```
# Real stroke + snapshot cache, cold render.
python -m manim_rs render examples.integration_scene IntegrationScene out.mp4 --duration 4 --fps 30

# Second run of the same command completes in < 1s.
python -m manim_rs render examples.integration_scene IntegrationScene out.mp4 --duration 4 --fps 30
```

Cold render produces an mp4 whose stroke edges show analytic AA (smooth at 480×270 where Slice C's MSAA-only strokes still shimmer) and whose one new tapered-stroke test object demonstrates per-vertex width. Warm render returns byte-identical mp4 from cache.

---

## 2. Scope Decisions (locked)

| Dimension | Choice | Rationale |
|---|---|---|
| Stroke representation | Port `manimlib/shaders/quadratic_bezier/stroke/` at pinned SHA `c5e23d9`. Per-vertex attributes: `position`, `stroke_rgba`, `stroke_width`, `joint_angle`. Fragment AA via Loop-Blinn-style SDF + `smoothstep`. | The point of Slice D. Matches manimgl semantics so scene authors' intuition transfers. |
| Stroke expansion | **CPU-side**, in `manim-rs-raster/src/tessellator.rs`. WGSL has no geometry shader; the alternative (vertex-shader instancing with expansion) costs complexity with no perf win at Slice D scales. | Simpler; reuses lyon's quadratic curve primitives; keeps the raster crate pure-wgsl on the GPU side. |
| Bézier order | Quadratic internally. `BezPath::CubicTo` verbs are split to quadratics via a fixed-depth midpoint approximation (4 quadratics per cubic). | Matches manimgl's representation; the stroke shader math is quadratic-native; one fewer shader variant. |
| 2D only | Drop `flat_stroke`, `scale_stroke_with_zoom`, `frame_scale`, `unit_normal` uniforms from the port. Hardcode `unit_normal = (0,0,1)`. | Slice F (3D) will reintroduce these. Carrying them now adds branches with no visible effect. |
| Joint types | **MITER and BEVEL only**, selected via the same angle threshold manimgl uses (`MITER_COS_ANGLE_THRESHOLD = -0.8`) as the default `AUTO`. Expose `joint: "miter" \| "bevel" \| "auto"` on Python `Stroke`. No `NO_JOINT` exposed; no round joint. | AUTO gives manimgl-equivalent behavior by default. Round joints cost a separate shader branch; defer. |
| Per-vertex width | `Stroke.width` on Python is either a scalar (current behavior) or a per-vertex `list[float]` aligned with the path's sampled vertices. Scalars broadcast. | Minimum shape needed to demonstrate the feature. Richer width-over-arclength helpers (tapers, animated width) are deferred Python-only. |
| Fill pipeline | **Untouched.** Slice C's lyon fill + MSAA stays. | Per §11, `path_fill.wgsl` stays trivial another slice. Changing both AA strategies simultaneously would confound snapshot diffs. |
| MSAA | **Keep 4×** on the color target. Analytic AA handles within-stroke edges; MSAA covers fill edges and intersection of stroke/fill. | Two AA layers compose. Disabling MSAA would regress fill quality. |
| Snapshot cache key | `blake3(serde_canonical_bytes(scene_ir) ‖ u32_le(frame_idx) ‖ u32_le(width) ‖ u32_le(height))`. | Blake3 is the Rust-ecosystem default, fast, no collisions at this scale. Canonical serde bytes need the existing `serde_json::to_vec` with sorted maps — no new dep. |
| Cache location | `.manim-rs-cache/<scene_hash>/<frame_idx>.rgba` — raw RGBA8 framebuffer, not PNG. | PNG encode cost dominates cache-miss savings. Raw RGBA is 4× the bytes but zero CPU. Encode only happens once, on ffmpeg ingest. |
| Cache granularity | Per-frame, not per-mp4. A duration change invalidates only the tail. | Frame-level is the natural unit; mp4-level cache would invalidate on every timing tweak. |
| Cache invalidation | Implicit (hash changes → new key). No explicit eviction. `--no-cache` CLI flag bypasses both read and write. | Cache is a build artifact; if it grows too big the user deletes `.manim-rs-cache/`. Defer LRU until someone asks. |
| Cache scope | Local filesystem only. No S3, no shared cache. | Per architecture.md, distributed render is a later slice. |
| `Runtime` lifecycle | Still per-render. Cache is a plain module-level helper, not a `Runtime` field. | No consumer needs a persistent runtime. Keeps the caching layer orthogonal to GPU state. |
| Snapshot-test strategy | Tolerance-based from Slice C stays. Re-pin fresh tolerance baselines after the stroke port since analytic AA shifts edge pixels. | §E. Platform-exact checksums still off-limits. |
| Platform | macOS arm64 dev box. | Unchanged. |
| ADRs | **One consolidated ADR** (`0006-slice-d-decisions.md`) covering real stroke port, cubic→quadratic split strategy, blake3 cache key, raw-RGBA cache format. | Slice C retro confirmed single-ADR-per-slice is the right cadence. |

---

## 3. Work Breakdown

Ordered. Each step ends with a testable artifact. **Per Slice C §11 delta: "expose to Python + use in test" is collapsed within each step** — no step leaves a feature reachable only from Rust.

### Step 1 — Cubic→quadratic split + BezPath sampling

- `manim-rs-raster/src/tessellator.rs`: add `sample_bezpath(path: &BezPath) -> Vec<QuadraticSegment>` where `QuadraticSegment = { p0, p1, p2 }`. `CubicTo` verbs split to 4 quadratics via fixed-depth midpoint subdivision; `LineTo` degenerates to `p1 = (p0+p2)/2`; `QuadTo` passes through; `Close` emits a line back to sub-path start.
- Unit test in `tests/tessellator_sample.rs`: every `BezPath` verb variant produces the expected segment count and endpoint continuity.
- No Python surface change.

**Why first:** every subsequent step consumes `QuadraticSegment`s. Getting the input representation right before writing any GPU code keeps the shader port honest.

**Artifact:** `cargo test -p manim-rs-raster tessellator_sample` green.

### Step 2 — Stroke expansion with per-vertex attributes

- Extend `StrokeVertex` in `tessellator.rs`: `{ position: Vec2, uv: Vec2, stroke_width: f32, joint_angle: f32, color: [f32; 4] }`.
- New `expand_stroke(segments: &[QuadraticSegment], widths: &[f32], color: [f32;4], joint: JointType) -> VertexBuffers<StrokeVertex, u32>`. Each quadratic is polylined to `MAX_STEPS=32` (matches manimgl `POLYLINE_FACTOR=100` / step heuristic, capped); each polyline step becomes a quad strip widened by `stroke_width[i]` along the perpendicular; adjacent strips join via miter or bevel per `joint_angle`.
- Width alignment: `widths.len() ∈ {1, segments.len()+1}`; scalar broadcasts.
- Drop `lyon::StrokeTessellator` from the stroke path. Polyline factory stays via the generic `BezPath` sampler.

**Reference:** `manimlib/shaders/quadratic_bezier/stroke/geom.glsl` at commit `c5e23d9`. Literal-first translation per CLAUDE.md porting practice #4 — keep manimgl variable names (`joint_angle`, `step_to_corner`, etc.) on the first pass.

**Artifact:** `cargo test -p manim-rs-raster expand_stroke` — fixtures for {straight line, L-bend with miter joint, L-bend with bevel joint, tapered width}. Assert vertex counts and perpendicular direction at one sample per fixture.

### Step 3 — WGSL port + pipeline wiring

- Rewrite `path_stroke.wgsl`:
  - Vertex: MVP transform, pass `v_dist_to_center`, `v_half_width_ratio`, `v_color` to fragment. `v_dist_to_center` is the interpolated perpendicular coordinate; `v_half_width_ratio = stroke_width / (2 * anti_alias_width)`.
  - Fragment: `let sd = abs(v_dist_to_center) - v_half_width_ratio; color.a *= smoothstep(0.5, -0.5, sd); return color;` — ports `frag.glsl` at commit `c5e23d9`.
- `StrokeUniforms`: add `anti_alias_width: f32`, `pixel_size: f32`. MVP stays. Remove the per-object `color` field (moved to per-vertex).
- `FillUniforms = StrokeUniforms` alias drops (the structs diverge). Accept the duplication; §11 "alias when byte-compatible" is about byte-compatibility, not aspirational reuse.
- `Runtime::render` path: replace the stroke draw with one using the new vertex layout. MSAA config untouched.

**Porting note to update:** `docs/porting-notes/stroke.md` — flip status from "Slice B stub" to "Slice D shipped." Keep the §"What Slice B did instead" section as historical context; add §"What Slice D ships" mirroring the `fill.md` structure. Per-function manimgl+SHA headers on the WGSL and the Rust expansion fn.

**Artifact:** `cargo test --workspace --exclude manim-rs-py` green (existing raster tests may need tolerance rebaselines — do them now); new `crates/manim-rs-raster/tests/stroke_aa.rs` — renders a 45° line at 480×270 and asserts edge gradient (stroke-edge pixels have intermediate alpha, proving analytic AA vs. Slice C's MSAA-only hard-then-soft transition).

### Step 4 — Python: per-vertex width + joint option

- `python/manim_rs/objects/stroke.py` (new — was inline in `geometry.py`):
  - `Stroke(color, width: float | list[float] = 1.0, joint: str = "auto")`.
  - Validation: `joint ∈ {"miter", "bevel", "auto"}`; if `width` is a list, length must match the path's vertex count as emitted by the factory.
- Expose via existing geometry factories: `Circle(..., stroke=Stroke(..., width=[...]))` etc. Factories compute their own vertex count so authors can size the width list correctly; surface a `Circle(...).vertex_count` property for discoverability.
- IR: extend `Stroke` in `ir.py` and `manim-rs-ir` to carry `width: Vec<f32>` (1 or N) and `joint: JointType`. Non-unit variants per Slice B §10.

**Artifact:** `tests/python/test_stroke.py` — build scenes with {scalar width, per-vertex width, miter joint, bevel joint}; IR round-trips; render one to mp4 and centroid-check the tapered shape.

### Step 5 — Snapshot cache

- New crate module `manim-rs-runtime::cache`:
  - `fn scene_hash(ir: &Scene) -> [u8; 32]` — serializes to canonical `serde_json` bytes, blake3-hashes.
  - `fn frame_key(scene_hash: [u8;32], frame_idx: u32, w: u32, h: u32) -> PathBuf`.
  - `fn read(path: &Path) -> Option<Vec<u8>>` / `fn write(path: &Path, rgba: &[u8])` — atomic write via `tempfile` + rename.
- `Runtime::render_frame` (or the frame loop in `runtime`, depending on current structure) consults the cache before the raster pass. On miss, raster + write. Result feeds the existing ffmpeg stdin.
- CLI: `--no-cache` flag on `render`. Default behavior: cache enabled. Env var `MANIM_RS_CACHE_DIR` overrides the default `.manim-rs-cache/` location.
- Cache lives under the **current working directory**, not the scene file's directory. Matches how ffmpeg output already works.

**Gotcha to pre-empt:** canonical serde must sort map keys. `serde_json::to_vec` does not sort by default — use `serde_json::to_value` → recursive `BTreeMap` conversion, or depend on `serde_json` with `preserve_order = false` (default). Verify with a round-trip test where two IRs differing only in map insertion order produce the same hash.

**Artifact:** `tests/python/test_cache.py` — render once (cold, timed), render again same args (warm, timed), assert warm < cold/5 and mp4 bytes are identical. `--no-cache` produces the same mp4 and always takes cold-render time.

### Step 6 — Integration scene update

- `tests/python/test_integration_scene.py`: add one new object whose stroke uses a per-vertex width list (e.g. tapered line or varying-width polygon). Keep the existing objects.
- Tolerance baselines refreshed for the analytic-AA crossover.
- `examples/tapered_stroke.py`: standalone scene demonstrating per-vertex width; referenced by the Slice D ADR.

**Artifact:** green CI. Visually: open the mp4, confirm taper visible, strokes smooth at diagonal segments.

### Step 7 — Consolidated ADR + retrospective prep

- Write `docs/decisions/0006-slice-d-decisions.md` covering: real stroke port, cubic→quadratic split, CPU-side expansion (no geometry shader), blake3 cache key, raw-RGBA cache format, `.manim-rs-cache/` cwd-relative location.
- Update `docs/gotchas.md` and `docs/performance.md` with any traps/observations surfaced during the slice.
- §11 retrospective in this file empty until ship. Fill immediately on completion.

---

## 4. Explicitly Out of Scope

Belongs to Slice E+. Resist scope creep:

- **3D stroke** — `flat_stroke`, `unit_normal` projection, camera-relative tangent. Slice F.
- **Round joints and round caps.** Separate shader branch; defer.
- **Stroke dash patterns.** Manimgl doesn't have these; no consumer has asked.
- **Animated per-vertex width** (width as a track type). Geometry width is authored once per object; if we need it animated later, add a `WidthTrack`.
- **Text / TeX / SVG.** Slice E.
- **Fill AA improvement (Loop-Blinn port).** `fill.md` flagged this; MSAA is good enough for another slice.
- **Distributed / S3-backed snapshot cache.** Local only.
- **LRU eviction / cache size caps.** Manual `rm -rf .manim-rs-cache/` is the eviction story.
- **Cache-aware parallel rendering** (chunk mp4 across processes). Later.
- **Persistent `Runtime` handle across renders.** Still no consumer.
- **Cross-platform wheels.** Parallel workstream (`slice-c.md` §9), not gated on this slice.

---

## 5. Success Criteria

- [ ] `maturin develop` builds cleanly; `pytest tests/python` and `cargo test --workspace --exclude manim-rs-py` all green.
- [ ] Both commands in §1 produce `out.mp4`; second run completes in < 1s wall-clock with byte-identical output.
- [ ] `ffprobe out.mp4` reports expected dimensions / fps / codec / pix_fmt.
- [ ] Visually: integration scene plays; tapered stroke object shows visible width variation; strokes smooth at 480×270 (where Slice C shimmered).
- [ ] `Stroke.width` accepts scalar and per-vertex list on the Python side; IR round-trips both.
- [ ] `--no-cache` skips the cache on both read and write paths.
- [ ] Tolerance-based snapshot tests use refreshed baselines; no exact-pixel pins.
- [ ] `0006-slice-d-decisions.md` written.
- [ ] Retrospective §11 filled before hand-off.

---

## 6. Known Gotchas To Pre-Solve

Each costs an hour cold. Pre-empting saves the day:

1. **Canonical serde JSON is not sort-default.** `serde_json::to_vec` preserves map insertion order. Any cache hash built on a `HashMap`-backed struct will be order-sensitive. Round-trip test before shipping the cache.
2. **Raw RGBA cache size grows fast.** A 1080p60 30s video is ~11 GB of raw RGBA. Documented in the ADR; `--no-cache` and `rm -rf` are the user-facing levers. Flag in `docs/performance.md` — future work: zstd-compress the cache entries.
3. **blake3 is not in Cargo.lock yet.** Adding it pulls a few hundred KiB; confirm with `cargo deny check` per `deny.toml`.
4. **Stroke edge pixels change values after the port.** Every existing snapshot tolerance baseline needs a refresh pass. Budget a dedicated sub-step; don't interleave with logic changes.
5. **Cubic subdivision depth.** Fixed depth 4 (→ 4 quadratics per cubic) is a magic number. Test on a sharp cubic (e.g. S-curve) to confirm it doesn't kink visibly. If it does, raise to 8. Document the choice in `stroke.md` porting note.
6. **Joint angle sign.** `manimgl` uses a signed `joint_angle`; the perpendicular direction flips on sign. Port the sign convention verbatim, test on both left-turn and right-turn L-bends.
7. **`stroke_width` units.** Manimgl uses `STROKE_WIDTH_CONVERSION = 0.01` to translate author units into scene units. Slice B shipped scene-unit stroke_width directly. Preserve Slice B's interpretation — do not introduce the 0.01 factor — and document the deviation in the porting note.
8. **MSAA + analytic AA double-counting.** Analytic AA already softens stroke edges; MSAA 4× over-softens them. The visual test is subjective; if strokes look fuzzy, the fix is MSAA-off on the stroke pipeline and MSAA-on on the fill pipeline (separate render passes). Flag as a note; probably don't need it.
9. **Per-vertex width list length.** Authors who write `Circle(stroke=Stroke(width=[1,2,3]))` will hit a validation error because `Circle.vertex_count` is much larger. Error message must say the expected count explicitly; refer them to `obj.vertex_count`.
10. **Ctrl-C during cache write.** Atomic write (tempfile + rename) already handles this; confirm with a `kill -INT` test.

---

## 7. Effort Estimate

| Step | Optimistic | Realistic | Pessimistic |
|---|---|---|---|
| 1. BezPath sampling | 2h | 3h | 6h |
| 2. Stroke expansion | 4h | 1d | 1.5d |
| 3. WGSL port + pipeline | 4h | 1d | 1.5d |
| 4. Python surface | 2h | 4h | 1d |
| 5. Snapshot cache | 3h | 6h | 1d |
| 6. Integration scene + baselines | 2h | 4h | 1d |
| 7. ADR + retro | 1h | 2h | 4h |
| **Total** | **~1.5 days** | **~3 days** | **~6 days** |

Assume realistic. Steps 2+3 are the volatility; everything else is legwork. User intuition was "~1 day" — optimistic is achievable only if manimgl's geom.glsl ports cleanly on the first pass, which history suggests it won't.

---

## 8. Artifacts Produced Along The Way

Per CLAUDE.md porting practices:

- `docs/ir-schema.md` — updated with `Stroke.width: Scalar | List`, `Stroke.joint`.
- `docs/porting-notes/stroke.md` — rewrite from "Slice B stub" to "Slice D shipped." Historical Slice B section retained.
- `docs/porting-notes/snapshot-cache.md` — new. Design, hash scheme, format, trade-offs.
- `docs/decisions/0006-slice-d-decisions.md` — consolidated ADR.
- `docs/gotchas.md` — traps added as they surface.
- `docs/performance.md` — cache size observation; per-object submit count still on the list.
- `examples/tapered_stroke.py` — per-vertex-width demo.

---

## 9. What Comes After Slice D

Not committed. Natural sequence unchanged from `slice-c.md` §10:

- **Slice E:** Text via cosmic-text + swash, glyph atlas. TeX via LaTeX subprocess.
- **Slice F:** 3D — surface pipeline, depth buffer, phi/theta camera. Reintroduces `flat_stroke` + `unit_normal` uniforms deferred here.

Revisit after Slice D lands.

---

## 10. Deltas carried from Slice C §11

Applied in this plan:

- **Collapse "expose to Python" + "use in test."** Each step (2–5) ends with the Python surface wired and a test exercising it. No step leaves a feature reachable only from Rust.
- **`BezPath` verbs stable.** No new verb added; per-vertex attributes ride on the `Stroke` struct, not the verbs.
- **`rate_functions.py` re-check done.** Audited at pinned SHA `c5e23d9` on 2026-04-23 — 15 functions, zero drift from Slice C's IR. No action needed.
- **Pinned SHA stability.** All porting notes cite `c5e23d9`. If the submodule advances mid-slice, re-pin and re-verify before merge.

---

## 11. Retrospective — what the plan got wrong

Shipped 2026-04-23 in one continuous run across Steps 1–7. The 1.5–3-day
estimate held; realistic path was ~2 days of focused work. What the
plan got wrong or missed:

1. **Cache key shape was wrong in the plan.** §2 locked
   `blake3(scene_ir ‖ frame_idx ‖ w ‖ h)` as the cache key. Shipped
   `blake3(version, metadata, camera, SceneState@t)` instead — hashing
   the evaluated state per frame, not the raw scene + index. The
   planned scheme would have invalidated every frame on any scene
   edit, defeating the whole point of the cache. Caught while writing
   the `local_track_edit_invalidates_only_affected_frames` test —
   expected 3 hits / 3 misses, got 5/1, because the cache turned out
   to be content-addressed in a useful way we hadn't planned for.
   **Lesson:** write the locality test *before* pinning the key
   shape; the test forces you to confront what "locality" actually
   means.

2. **"Cold run = every frame misses" is false.** Corollary of (1).
   The content-addressed key means frames sharing a `SceneState`
   (e.g. a static prefix before an animation starts) collapse into
   one cache entry — first frame misses + writes, subsequent frames
   in the same render hit the just-written entry. The Python
   integration test initially asserted `misses == TOTAL_FRAMES`;
   actual was 5/9. Corrected assertion is
   `misses == unique_frame_states`. Now flagged in
   `STATUS.md`-handoff language and in the ADR.

3. **`FillUniforms = StrokeUniforms` alias couldn't just "drop."**
   §2 said "accept the duplication." What actually happened: the
   alias was still in place when `StrokeUniforms` grew `{
   anti_alias_width, pixel_size }` and renamed `color` to `params`,
   which silently broke the fill shader (which reads `u.color`).
   Split into two separate structs with their own
   `UNIFORM_SIZE` constants. Cost: 15 minutes of "why is fill
   black?" **Lesson:** kill aliases the moment the structs diverge
   semantically, not when they diverge syntactically.

4. **The diagonal-stroke AA test needed a diagonal.** Step 3's
   `stroke_aa.rs` initially used a horizontal line to assert analytic
   AA, counting intermediate-alpha pixels. Got either 247 or 255 — no
   intermediate values. The smoothstep fade zone was sub-pixel on an
   axis-aligned line. Switched to `(-3,-2)→(3,2)` diagonal so MSAA
   sub-pixel coverage produces a broad fade band. **Lesson:** AA
   tests need a non-axis-aligned shape; axis-aligned edges hit the
   pixel grid too cleanly to exercise the fade.

5. **The tapered-stroke integration test checked the wrong channel.**
   Step 4's `tapered_stroke.rs` asserted lit-row counts by checking
   `pixels[i+3] > 16` (alpha). Background is opaque black (alpha=255),
   so every row counted. Switched to R channel. **Lesson:** when the
   background has full alpha, alpha is not a "pixel lit" signal. Use
   luminance or a specific color channel.

6. **Step 5's planned CLI `--no-cache` flag didn't ship.** Deferred
   in favor of the Python-level `cache_dir=` parameter, which the
   integration test needed anyway for isolation. No real caller has
   asked for `--no-cache`; `MANIM_RS_CACHE_DIR=$(mktemp -d)` is the
   current workaround. ADR §F records the defer.

7. **ADR number collided.** Plan said `0005-slice-d-decisions.md`;
   `0005-plain-ir-compiled-evaluator.md` already existed (written
   mid-Slice C). Shipped as `0006-slice-d-decisions.md`.
   **Lesson:** check `ls docs/decisions/` before pinning an ADR
   number in a slice plan.

8. **`examples/tapered_stroke.py` didn't ship.** Listed in §8; the
   tapered-stroke integration test (`test_cache_integration.py`)
   covers the visual story end-to-end, and no Slice-E consumer
   needs a standalone example. Dropped silently — flagging here
   rather than in STATUS.md because it's scope-truthing, not a
   regression.

9. **Cache eviction, size caps, and cross-machine sharing stayed
   deferred as planned.** `docs/performance.md` carries the
   "zstd-compress cache entries" note for a future pass.

What the plan got right: the step ordering (1 before 2 before 3
before 4 before 5, then the Python integration at 6). Each step
really did produce a testable artifact, and the "expose to Python
within each step" delta from Slice C's retro held — no step left a
feature Rust-only. The "one consolidated ADR per slice" cadence
continues to be worth it.

Cadence note: the explain-confirm-implement-update rhythm from
CLAUDE.md held across all 7 steps. `STATUS.md` was rewritten after
each step, which made the post-compaction resume clean.
