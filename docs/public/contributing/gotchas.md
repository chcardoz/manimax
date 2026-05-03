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

Fix: **use the absolute path** to the worktree's `.venv/bin/activate`, or keep activation as its own first command and chain the rest with `&&`.

### Depythonize before `py.allow_threads`, not after

`pythonize::depythonize` touches Python objects — it requires the GIL. The
pattern in `crates/manim-rs-py/src/lib.rs::render_to_mp4` deliberately
depythonizes the scene **first**, then calls `py.allow_threads(move || …)`
over the GIL-free render loop:

```rust
let mut scene = depythonize_scene(ir)?;   // needs GIL
// … optional mutation …
py.allow_threads(move || rust_render_to_mp4(scene, &out_path))
```

Reordering these ("release the GIL earlier so Python can do other work during
depythonize") crashes inside pyo3 with an abort. Same rule for `eval_at`:
build the `Evaluator` (which consumes the `Scene`) outside `allow_threads`.

Fix lives in: `crates/manim-rs-py/src/lib.rs:50-68`.

### msgspec / pyo3 tagged-union field order is tolerant — don't rely on it

`pythonize::depythonize` reads fields by *name*, not position, so a Python
msgspec Struct whose fields appear in a different order than the Rust serde
struct still deserializes correctly as long as every tagged-union discriminator
(`kind`, `op`) is present in the payload. This is convenient but makes
ordering bugs invisible: reordering fields on one side and not the other
silently "works" — until a consumer that *does* depend on order (an external
tool, a pretty-printer, a future binary format) breaks.

**Rule:** keep Python msgspec Struct field order aligned with the Rust struct
declaration. The round-trip tests don't catch mis-ordering today; the schema
drift guard in `tests/python/test_ir_roundtrip.py` is the closest thing and
only asserts structural equality after re-serialization.

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

### MSAA resolve target must match color target format + dimensions exactly

`RenderPassDescriptor::color_attachments[0].resolve_target` must point to a texture with the **same format** as the multisampled color texture and the **same `width × height`**. A format mismatch (e.g. resolve `Rgba8Unorm` vs color `Rgba8UnormSrgb`) or a size mismatch panics inside wgpu with a message that doesn't immediately name the offending pair.

Fix lives in: `crates/manim-rs-raster/src/lib.rs` — the `msaa_color_target` and `resolve_target` descriptors share `COLOR_FORMAT`, `width`, `height` by construction. Keep them paired; don't let one drift.

Sample count is the other axis — the render pipeline's `multisample.count` must match the color attachment's `sample_count` (both `MSAA_SAMPLE_COUNT = 4`). `StrokePipeline::new` and `FillPipeline::new` both read the constant; new pipelines should too.

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

Caught by: `test_scene_roundtrips_through_rust` (via `_wide_scene`, which distributes all 15 easing variants across track types) during Slice C Step 2. Originally surfaced as `ThereAndBackWithPauseEasing(pause_ratio=1.0/3.0)`; the current fixture uses `pause_ratio=0.25` (dyadic) to avoid the f32 round-trip.

---

## Testing / verification

### Repo-root example imports are not stable in CI

Tests that need checked-in example scenes should load them by file path (for
example via `manim_rs.discovery.load_scene`) instead of importing
`examples.*`. Editable/maturin installs do not guarantee the repository root is
on `sys.path` in every pytest invocation, even though the `python/` package
source is importable. The failure mode is `ModuleNotFoundError: No module named
'examples'` while running from CI or from outside the repo root.

Fix lives in: `tests/python/test_e2e_text_tex.py`.

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

### Any encoder subprocess pipe must be drained on a worker thread

Capturing a chatty subprocess pipe (libx264 warnings leak through `-loglevel error`) and only reading after `wait()` deadlocks: the kernel pipe fills → child blocks on `write(stderr)` → its stdin stalls → our `push_frame` blocks → we never reach `wait()`.

`crates/manim-rs-encode/src/lib.rs` solved the original case (stderr) with a background line-reader into a 64 KiB-capped `Arc<Mutex<String>>`, joined by `finish` and `Drop`. Any future encoder pipe (`-progress pipe:` for ffmpeg-native progress, etc.) needs the same drain discipline at introduction, not after the deadlock.

### Pixel-exact snapshot constants are platform-pinned

`crates/manim-rs-raster/tests/snapshot.rs` pins an RGBA byte-sum and non-zero count for the canonical Slice B scene. Values are mac arm64 + Metal + wgpu 29. On a different backend (Vulkan on Linux, D3D12 on Windows) they will drift — that's expected, not a bug. Update under scrutiny: verify the *kind* of drift (all channels scaled uniformly vs. only some pixels changing) before bumping the constants.

Slice C migrated these to tolerance-based checks (sum ± N, nonzero count ± N). Any new snapshot test must follow suit — do not re-introduce exact byte pins. ADR `0004 §E`.

### H.264 / yuv420p chroma subsampling shifts solid fill colors

Solid fill `(0, 229, 51)` (the integration scene's green teardrop) decodes back as approximately `(0, 240, 120)` after the libx264 + yuv420p round trip. Chroma subsampling is 2:2:0 so 2×2 pixel blocks share chroma, and gamut compression on the sRGB → BT.709 conversion nudges the hue. This is not a renderer bug.

Per-object color-band tests (`tests/python/test_integration_scene.py`) tune their RGB tolerance accordingly — e.g. the green mask accepts G ≥ 150 with B up to 150. Tightening those bands without a lossless output path will produce spurious failures.

If we ever expose a lossless-raw output (e.g. `ffv1` or raw RGBA mp4), add a separate test path with tight bands against that codec — don't try to tune one set of bounds to cover both.

### lyon dedupes sub-epsilon stroke points — circles can't force overflow

A unit circle with 500 000 tessellation points collapses to ~282 vertices in lyon because neighbours are sub-epsilon apart. If you're calibrating a `GeometryOverflow` test (or any "what if input is huge" tessellator test), use non-coincident points — e.g. a zigzag `[-5 + 10i/n, ±1, 0]` forces every segment to a distinct diagonal that lyon can't merge.

Working calibration: zigzag @ n=3000 → 6002 tessellated vertices > `MAX_VERTICES_PER_OBJECT = 4096` cap. See `crates/manim-rs-raster/tests/edge_cases.rs::oversized_polyline_returns_geometry_overflow`.

---

## Python / typer

### typer auto-flattens single-subcommand apps

If you register exactly one `@app.command()`, typer drops the subcommand name — `python -m pkg render ARG` becomes `python -m pkg ARG`, breaking any documentation that shows the subcommand. **Add a no-op `@app.callback()`** to force typer to keep subcommands.

Fix lives in: `python/manim_rs/cli/__init__.py` — the `_root` callback. Delete it only once a second subcommand exists.

---

## Evaluator

### Gap-clamping on position tracks

When `t` falls in a gap between two position segments (segment N ends at 1.0, segment N+1 starts at 1.2, you ask for `t=1.1`), the evaluator must return **the most recently completed segment's `to`** — *not* the overall last segment's `to`. The bug form: iterating all segments and setting `held = last.to`, then clamping. The correct form: track `held` as iteration proceeds, updating only when a segment's `t1 <= t`.

Fix lives in: `crates/manim-rs-eval/src/tracks.rs` — `evaluate_track` (generic over all track types). Test lives in: `crates/manim-rs-eval/src/lib.rs` `#[cfg(test)]` block — `gap_between_segments_holds_last_to`.

This is the bug a re-implementer is most likely to re-introduce.

---

## Text / fonts / glyph outlines

### swash hinting at low ppem produces stair-stepped outlines that explode under scale-up

swash applies TrueType hinting when the requested ppem is small.
Manimax uses "1 em = 1 world unit," so the natural ask is `ppem ≈ 1.0` — small enough that hinting snaps every control point to the integer pixel grid. The outline is correct at ppem≈1 but scales catastrophically: `Tex.scale=8` (or any zoomed camera) turns the snapped points into visible staircase scallops on every curve.

Fix: extract at a high internal ppem and scale down via affine. `crates/manim-rs-text/src/glyph.rs` pins `OUTLINE_PPEM = 1024` and post-multiplies the BezPath by `Affine::scale(scale / 1024)`. The 1024 value is effectively hinting-off; smooth at every downstream scale we tested (0.25 to 8).

Symptom that surfaces this: the path is geometrically right (closed contours, correct winding, fills the right region) but every curve is faceted and the facets get bigger as you zoom in. If you only see this at one zoom level, suspect lyon flatness instead (see next entry).

Caught by visual inspection during Slice E Step 5, not by any test in the corpus. Tests that *would* catch it: pin `bbox` to a value derived from the curve (not just non-degenerate), or do a raster-snapshot at `Tex.scale=4+`.

### `cargo test` does not rebuild the pyo3 extension; stale `_rust` panics

Slice E Step 7 hit this. Rust changes in `crates/manim-rs-eval`
landed and `cargo test --workspace` was green. The Python integration
test then panicked with `unreachable: Object::Text must be expanded by
Evaluator::eval_at` — because the loaded `manim_rs._rust` extension
was the pre-S7c build, and only the Rust-only test path saw the new
fan-out arm.

**Fix:** after any change in a crate that ends up inside the pyo3
extension (`manim-rs-py` and everything it depends on), run
`source .venv/bin/activate && maturin develop` *before* running
pytest. cargo's incremental build won't help — the .so/.dylib
loaded by Python is built by maturin, not by cargo, and lives in
`.venv/lib/python*/site-packages/manim_rs/_rust*.so`.

CLAUDE.md's "Day-to-day" section already lists `maturin develop` as
the rebuild step. The trap is that test-driven workflows happily
ship the Rust tests and skip the Python suite for a session, then
get bitten on the next pytest run that exercises the boundary.

### cosmic-text `FontSystem::new()` does a system font scan that breaks hermetic tests

cosmic-text's default `FontSystem::new()` invokes
`fontdb::Database::load_system_fonts()` on first use. On a dev box
that's hundreds of fonts, ~100 ms of init. On CI runners with sparse
font sets the result diverges from local; on test machines with
*different* system fonts the same call resolves a `Family::Name`
lookup to different bytes, breaking determinism.

**Fix:** seed your own `fontdb::Database` with `load_font_data` from
known bytes, then construct via `FontSystem::new_with_locale_and_db`.
`crates/manim-rs-text/src/cosmic.rs::font_system` does exactly this
and wraps the result in an `OnceLock<Mutex<FontSystem>>` so the seed
runs once per process. Add new fonts via `db.load_font_data(...)`
inside the same `OnceLock` init; never mutate the database after
publication.

If you're tempted to "let users mix in system fonts," ADR 0012 covers
why we don't — determinism + reproducibility + hermetic tests
outweigh convenience. The `Text(..., font="path/to.ttf")` parameter
is the supported escape hatch.

### Snapshot tolerance values picked on macOS-arm64 dev usually need to grow on Linux/lavapipe CI

When/if Slice E's Tex corpus harness lands (currently deferred per
slice plan §STATUS), the `TEX_SNAPSHOT_TOLERANCE` constant must be
chosen to pass on **both** macOS-arm64 + Metal (the dev target) and
Linux + lavapipe (the CI target). Slice C/D experience: these two
backends produce non-trivially different rasterizations of the same
geometry — sub-pixel coverage thresholds, MSAA pattern, sRGB
linearization order, and lavapipe's CPU-side scan conversion all
diverge from Metal in small but systematic ways.

**Fix when baselining:** generate baselines on macOS, then run the
full corpus through CI before pinning the tolerance. The CI run will
likely surface 5–15% of pixels that differ by one or two channel
values; pick a max-channel-delta + max-percent-pixels-differing pair
that covers the worst case in the corpus with ~2× headroom for
future drift. ADR 0007 (CI on lavapipe) is the load-bearing
constraint here.

Until the harness ships, anyone re-rendering corpus expressions for
visual review should use `python -m manim_rs frame ...` (ADR 0008
§F) to spot-check on dev. Don't write a test that pins the macOS
tolerance and assume CI will pass — it won't.

### lyon `FillOptions::DEFAULT.tolerance` (0.25) is wildly too coarse for em-scaled geometry

`FillOptions::default()` ships with `tolerance = 0.25`, a budget calibrated for SVGs in *pixel* coordinates. Glyph outlines (and any Tex-derived geometry) arrive in *em*-scaled world units where 1 em ≈ 1 world unit. A 0.25 budget on em-scaled curves flattens an `o` into an octagon.

Fix: `crates/manim-rs-raster/src/tessellator.rs` pins `FILL_TOLERANCE = 0.001` (1‰ of an em). Empirically smooth across the scales tested in Slice E.

Trade-off: tessellation cost scales with `1 / sqrt(tolerance)`, so 0.25→0.001 is ~16× more curve segments per glyph. Hasn't bitten yet but logged in `../contributing/performance.md` as a future per-Object knob.

If you see "geometric octagons, identical at every zoom level" → flatness tolerance. If you see "stair-stepping that gets coarser as you zoom in" → swash hinting (previous entry). They look superficially similar at one resolution and you can have both at once.

---

## Cultural

### Pick "match manimgl" over "what's technically correct" by default

When a decision has a correct answer (e.g. linear color space) and a manimgl-compatible answer (e.g. sRGB floats), **default to the manimgl-compatible one** and write an ADR. Reason: the primary goal is being a drop-in ManimGL replacement; surprising divergences compound into porting pain. Deviate only with a conscious ADR that names what motivates it.

Precedent: the sRGB color decision (Slice B) and Vec3 (rather than Vec2) coordinate decision (Slice B IR). Both cases the "technically right" answer was tempting and both times matching manimgl was the right call.
