# 0006 — Slice D consolidated decisions

**Date:** 2026-04-23
**Status:** partially superseded by 0009 (sections C–F: pixel cache).
Sections A (real stroke port) and B (cubic→quadratic split) remain
accepted.

One ADR covering the design calls that defined Slice D. Continues the
single-consolidated-ADR-per-slice cadence established by 0004 and
reaffirmed by Slice C's retro.

---

## A. Stroke port: CPU-side expansion + WGSL SDF AA

### Decision
Port `manimlib/shaders/quadratic_bezier/stroke/` (pinned SHA `c5e23d9`)
as a hybrid CPU/GPU pipeline: **CPU** expands `QuadraticSegment`s into
a per-vertex quad-strip (`position, uv, stroke_width, joint_angle,
color`), **GPU** does MVP + analytic SDF AA in the fragment shader
(`alpha *= smoothstep(-aa/2, aa/2, half_w_px - |uv.y|·half_w_px)`).

### Why
- WGSL has no geometry shader; CPU expansion is the only realistic
  port target. Vertex-shader instancing works but adds complexity
  without perf headroom at Slice D scales.
- Analytic AA in the fragment shader gives resolution-independent
  edges. Slice C's MSAA-only strokes shimmered at 480×270.
- Quadratic-native math lets one shader variant cover the whole pipe
  (cubics split up front in Step 1).

### Consequences
- Stroke and fill pipelines no longer share a uniform shape.
  `FillUniforms` had to become its own struct (`{ mvp, color }`)
  after `StrokeUniforms` grew `{ anti_alias_width, pixel_size }` and
  moved color off-uniform. The `FillUniforms = StrokeUniforms` alias
  from Slice C is gone — see Consequence notes on the fill porting
  note.
- Both AA layers compose: analytic AA inside stroke edges, MSAA 4×
  everywhere else. Tolerance baselines drifted as predicted in §6.4;
  re-pinned in one pass during Step 3.

### Rejected alternatives
- **Geometry-shader translation.** WGSL doesn't have them.
- **Vertex-shader instanced expansion.** Cleaner for static widths,
  worse for per-vertex widths — index gymnastics outweigh the CPU
  savings at our scale.
- **MSAA-only (keep Slice B/C).** Fails ship criterion (a): visible
  shimmer at 480×270.

---

## B. Cubic→quadratic split via fixed-depth midpoint subdivision

### Decision
`BezPath::CubicTo` verbs are split into **4 quadratic segments** via
fixed-depth midpoint subdivision in `sample_bezpath`. `LineTo`
degenerates to a quadratic with `p1 = (p0+p2)/2`; `QuadTo` passes
through; `Close` emits a line back to the sub-path start.

### Why
- Stroke shader math is quadratic-native; one shader, no branches.
- Fixed depth 4 is kink-free on the sharp cubics in our integration
  fixtures (including the S-curve test in `test_bezpath_verbs.rs`).
- Deterministic: same input → same segments → same cache key. An
  adaptive scheme would couple the cache to a tolerance parameter.

### Consequences
- Pathologically sharp cubics may visibly kink at depth 4. If that
  surfaces, raise the constant and bump `CACHE_KEY_VERSION`.
- Segment count is predictable: `cubics×4 + lines + quads + closes`.
  Width-list length validation in Python can rely on this.

### Rejected alternatives
- **Adaptive (de Casteljau until flatness).** Couples cache keys to a
  tolerance param. Revisit if fixed depth starts costing visible
  quality.
- **Ship as cubics, port manimgl's cubic stroke path.** Two shader
  variants to maintain. Cost > benefit at Slice D.

---

## C. Stroke width as `Scalar | PerVertex` union; joints as `miter | bevel | auto`

### Decision
IR: `Stroke.width: StrokeWidth` where
`StrokeWidth = Scalar(f32) | PerVertex(Vec<f32>)` (serde `untagged` —
serializes as bare number or JSON array). `Stroke.joint: JointKind`
where `JointKind ∈ {Miter, Bevel, Auto}` (serde `rename_all =
"lowercase"`, `#[serde(default)] = Auto`). Python surface matches:
`stroke_width: float | Sequence[float]`, `joint: Literal["miter",
"bevel", "auto"] = "auto"`.

### Why
- Untagged enum keeps scalar strokes wire-compatible with Slice B/C
  payloads — no schema migration.
- `Default = Auto` + `serde(default)` on `joint` means legacy payloads
  without the field still deserialize. One IR, no version fork.
- Auto replicates manimgl's `MITER_COS_ANGLE_THRESHOLD = -0.8`
  behavior by default; authors who need either joint explicitly still
  have the knob.
- No `NO_JOINT`, no round joints — manimgl's round-joint branch is a
  whole extra shader path. Deferred to whenever an author actually
  asks.

### Consequences
- Per-vertex width length has a platform-specific rule: Polyline
  requires `len(points)`; BezPath defers to Rust because the endpoint
  count depends on subdivision depth (B). Documented in the objects'
  `_build_stroke` helper.
- Closed Polyline has a natural off-by-one (N vertices, N edges): the
  Rust tessellator pads `widths[0]` at the wrap point. Gracefully
  degrades to scalar on any other length mismatch instead of panicking.

### Rejected alternatives
- **`width: Vec<f32>` always; scalar becomes `[w]`.** Loses wire
  compatibility with Slice B/C. Migration cost higher than the union
  cost.
- **`joint: Option<JointKind>` with `None` meaning auto.** Triple-valued
  semantics for a two-axis choice. The named enum reads better.
- **Exposing round joint.** Separate shader variant; no asker.

---

## D. Snapshot cache key: blake3 over `(version, metadata, camera, SceneState)`

### Decision
Cache key is `blake3(serde_json::to_vec(FrameKeyInput { version,
metadata, camera, state }))`, where `state` is the **evaluated
`SceneState` at frame time `t`** — not the raw `Scene` IR plus frame
index.

```rust
pub const CACHE_KEY_VERSION: u32 = 1;
FrameKeyInput { version, metadata: &SceneMetadata, camera:
  CameraHashable, state: &SceneState }
```

### Why
- Hashing the *evaluated* state gives true frame-level locality: two
  frames whose evaluated scenes are byte-identical share a cache
  entry (e.g. a static prefix before an animation starts). The
  originally-planned `blake3(scene_ir ‖ frame_idx)` scheme would
  invalidate every frame on any scene edit — strictly worse.
- `serde_json::to_vec` is deterministic over our IR:
  `active_objects_at` walks the timeline in insertion order, and
  `Evaluator`'s `HashMap<ObjectId, TrackBundle>` is only used for
  `.get`, never iterated. Struct fields serialize in declaration
  order. No JCS dependency needed.
- `CACHE_KEY_VERSION` is an independent invalidation knob for
  raster-side changes (shader edits, MSAA/format changes,
  tessellator rewrites) that don't show up in the hashed inputs.
  Orthogonal to `SceneMetadata::schema_version`.

### Consequences
- The cache is **content-addressed**, not frame-indexed. Visible in
  the Python integration test: even the cold run has non-zero hits
  when the static prefix collapses into one entry. `misses == unique
  frame states`, not `misses == total_frames`. Flagged in STATUS.md
  handoff notes.
- A cross-run collision in `SceneState` = a cache hit. This is
  intentional and correct; pixels really are identical.
- Authors don't hash their Python source — they hash the *IR the
  evaluator sees*, which is the right boundary.

### Rejected alternatives
- **`blake3(scene_ir ‖ frame_idx ‖ w ‖ h)` (original plan).**
  Invalidates everything on any scene change. Loses Slice D's big
  win.
- **Canonical JCS via `serde_jcs`.** Extra dep for a determinism
  property we already have.
- **`BTreeMap` sorted-map canonicalization.** We don't serialize
  maps in hashed inputs; determinism comes from `Vec` iteration
  order.

---

## E. Raw RGBA on-disk format, atomic writes, no eviction

### Decision
Each cache entry is `<hex>.rgba` — raw RGBA8 framebuffer bytes, no
container. Writes go through `tempfile::NamedTempFile::new_in(&dir)`
+ `persist(final_path)` for atomicity on the same filesystem. `get`
performs a size check (`bytes.len() == expected_frame_len`);
mismatch is treated as a miss. **No eviction, no size caps, no LRU.**

### Why
- PNG/QOI encode cost on write dominates the cache-miss savings we
  care about. Raw bytes cost 4× disk but zero CPU. ffmpeg ingests
  RGBA directly; no transcode on hit.
- Atomic rename survives Ctrl-C and concurrent writers
  (`NamedTempFile` on the same `dir` guarantees `persist` is a
  rename-within-filesystem, not a cross-device copy).
- Size-check-on-read is cheap, catches truncated writes and
  manual corruption. The corrupted-entry test proves the caller
  recovers (re-render + overwrite).
- Eviction is its own problem; coupling it to rendering means every
  render decides what to delete. The user-facing story is
  `rm -rf .manim-rs-cache/`. If that becomes a real pain, a
  standalone CLI tool is the right home.

### Consequences
- 1080p60 × 30s = ~11 GB of raw RGBA. Flagged in
  `../contributing/performance.md`: zstd compression is a future lever
  worth ~4–5× savings at modest CPU cost.
- No cross-machine or S3 layer. `MANIM_RS_CACHE_DIR` can point at a
  shared path but we don't coordinate writes beyond the atomic
  rename. Distributed render is a later slice anyway.

### Rejected alternatives
- **PNG / QOI entries.** Encode CPU kills the miss-to-hit amortization.
- **SQLite / sled.** Overkill for content-addressed blobs; file-per-key
  is simpler and lets `ls` and `du` work as debug tools.
- **LRU with size cap.** Defer until someone asks. `rm -rf` suffices.

---

## F. Python surface: `cache_dir` parameter + `CacheStats` dict return

### Decision
The pyo3 `render_to_mp4` function takes an optional `cache_dir` and
returns a dict `{"hits", "misses", "write_errors"}`. Omitting
`cache_dir` falls back to `FrameCache::open_default`
(`$MANIM_RS_CACHE_DIR` or `.manim-rs-cache/`). The CLI ignores the
return value.

### Why
- Test isolation needs an explicit cache path. An env var works for
  CLI use but is awkward from Python fixtures.
- Exposing `CacheStats` is free once it exists on the Rust side —
  refusing to expose it would force tests into filesystem snooping
  (mtime diffing, which the integration test also does as a
  cross-check).
- Dict return is the lowest-commitment shape. We can promote it to a
  typed struct later without breaking the wire.

### Consequences
- **No `--no-cache` CLI flag shipped.** Planned in §5 of the slice
  doc; deferred. Workaround: `MANIM_RS_CACHE_DIR=$(mktemp -d)`. If a
  real caller wants it, add the flag and re-wire
  `render_to_mp4_with_cache` to accept an `Option<&FrameCache>`.
- CLI unchanged at the user level; everything flows through the same
  render path.

### Rejected alternatives
- **Env var only.** Works for one cache location per process, breaks
  test parallelism.
- **Return `None` and expose stats via a separate call.** Two calls
  for one render; race-prone if we ever go concurrent.
