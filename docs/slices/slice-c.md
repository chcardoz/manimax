# Slice C — fully-featured multi-shape animated scene to mp4

**Status:** scope locked 2026-04-21; work not started.
**Date:** 2026-04-21.
**Follows:** `slice-b.md` (shipped). Read Slice B's §10 retrospective before starting any step here.

Slice B proved the pipeline on one shape, one easing, one position track, one object, hardcoded in code. Slice C turns Manimax into something a non-author can use to render a real scene: user-authored file, multiple geometry types, multiple track types, a real easing library, fill + MSAA, and a typed FFI. Ship criterion: **one command renders a user-authored multi-shape animated scene to mp4.**

Read `docs/architecture.md` and `slice-b.md` first. This doc assumes them.

---

## 1. Goal

One command produces a valid mp4 from a user-authored scene file:

```
python -m manim_rs render out.mp4 --scene my_scene.py MyScene --duration 4 --fps 30
```

Where `my_scene.py` contains a `Scene` subclass that composes **multiple geometry primitives** (e.g. a circle, a square, a line), applies **multiple track types simultaneously** (e.g. position + opacity + rotation), uses **non-linear easings** (e.g. `smooth`, `rush_into`), and uses **filled** as well as stroked shapes. The rendered mp4 shows all of that correctly, with MSAA-smoothed edges.

---

## 2. Scope Decisions (locked)

| Dimension | Choice | Rationale |
|---|---|---|
| FFI wire format | **`pythonize`** replaces the JSON string from Slice B. Reuses existing serde derives; single source of truth for IR types. | Slice B §2 + `ADR 0001` committed to revisit in Slice C. Growing IR makes per-frame parse cost unwelcome and weakens the contract. |
| `eval_at` pyfunction | Added. Pure function `_rust.eval_at(ir: PyAny, t: float) -> state`. No `Runtime` needed (no GPU). | Post-Slice-B audit deferred this. Now unlocked by FFI upgrade. |
| IR geometry primitive | **One new variant: `BezPath`**, a sequence of `MoveTo` / `LineTo` / `QuadTo` / `CubicTo` / `Close` verbs. Polyline becomes a `BezPath` preset. | Every manimgl `VMobject` stores cubic Bézier control points. One primitive subsumes Circle, Rectangle, Arc, Polygon, Line, etc. Eliminates the "new IR variant per shape" trap. |
| Python geometry factories | `Circle`, `Rectangle` (+ `Square`), `Line`, `Arc`, `Polygon`. All emit `BezPath` under the hood. Polyline retained as a factory. | High-value subset of manimgl's 27 geometry classes. Additional factories in later slices cost pure Python only. |
| IR track types | `position` (from B), plus `opacity`, `rotation`, `scale`, `color`. All four new tracks ship. | User answered "all the tracks." Opacity + rotation alone would satisfy the multi-track constraint; scale + color add authoring vocabulary at modest cost. |
| Easings | **All 15** from `manimlib/utils/rate_functions.py`: `linear`, `smooth`, `rush_into`, `rush_from`, `slow_into`, `double_smooth`, `there_and_back`, `there_and_back_with_pause`, `running_start`, `overshoot`, `not_quite_there`, `wiggle`, `squish_rate_func`, `lingering`, `exponential_decay`. | 1–5 lines each. Trivial to port; large vocabulary win. |
| Render pipelines | Stroke pipeline from B **+ new fill pipeline** fed by `lyon::FillTessellator`. | Stroke-only is visually unshippable for filled shapes. |
| MSAA | **4× enabled** on the color target. | Standard wgpu default. Slice B §2 deferral closed. |
| Scene discovery | `--scene file.py [ClassName ...]` + `--write-all`. Auto-pick single class; interactive stdin prompt if multiple and none specified (mirrors `manimlib/extract_scene.py:40`). | User: "follow manimgl." |
| CLI surface | Existing `render` subcommand grows `--scene` + optional positional `ClassName`s + `--write-all`. No other flags. | Keep flags minimal; defer quality/resolution to later slices. |
| Runtime lifecycle | `Runtime` remains per-render. `eval_at` is pure and needs no `Runtime`. | No current consumer needs a persistent handle. Defer when one does. |
| Snapshot-test checksum strategy | **Tolerance-based**, not platform-exact. Checksums become sum-within-±N + nonzero-count-within-±N. | MSAA makes exact pixel values backend-dependent; platform-exact pinning won't survive. User: "not platform specific." |
| Platform | macOS arm64 dev box still the only supported target. | Cross-platform wheels are a parallel workstream (see §9). |
| ffmpeg | User-installed on PATH. Status quo from Slice B. | Packaging deferred. |
| ADRs | **One consolidated ADR** (`0004-slice-c-decisions.md`) covering FFI upgrade, `BezPath` as unified primitive, all-tracks-all-easings, fill+MSAA pair, and tolerance-based snapshots. | User: "single one." Slice B retro: retroactive ADRs are more expensive than in-slice ones. |

---

## 3. Work Breakdown

Ordered. Each step ends with a testable artifact. Bottom-up: each step compiles and tests before the next begins, so a failure in a later step does not churn earlier ones.

### Step 1 — FFI upgrade (`pythonize` + `eval_at`)

- Add `pythonize` dep to `manim-rs-py`.
- Replace `render_to_mp4(ir_json: &str, ...)` with `render_to_mp4(ir: &Bound<PyAny>, ...)` using `pythonize::depythonize`.
- Add `#[pyfunction] fn eval_at(ir: &Bound<PyAny>, t: f64) -> PyResult<PyObject>` — depythonize IR, call `manim_rs_eval::eval_at`, pythonize the resulting state.
- Update `python/manim_rs/_rust.pyi` stubs.
- Update `python/manim_rs/cli.py` and runtime glue to pass the msgspec.Struct directly instead of `.to_json()`.

**Why first:** every later step passes richer IR through this boundary. Changing plumbing once is cheaper than changing it five times.

**Artifact:** existing Slice B tests (`test_render_to_mp4.py`, `test_scene_recording.py`) pass with the new FFI. New: `tests/python/test_eval_at.py` — build a scene, call `_rust.eval_at(scene.ir, t)` at t=0, 1, 2, assert the returned state matches direct Python evaluation.

### Step 2 — IR growth

- `manim-rs-ir`:
  - Add `Geometry::BezPath { verbs: Vec<PathVerb> }`; `PathVerb::{MoveTo, LineTo, QuadTo, CubicTo, Close}`.
  - Add `PropertyTrack` variants for `opacity`, `rotation`, `scale`, `color`.
  - Add `Easing` variants for all 15 rate functions. Variants that take params (`there_and_back_with_pause`, `running_start`, `overshoot`, `not_quite_there`, `wiggle`, `squish_rate_func`, `exponential_decay`) carry their params as struct fields (non-unit variants — Slice B §10 lesson: unit variants silently ignore extras under internally-tagged unions).
- `python/manim_rs/ir.py`: mirror every new variant as `msgspec.Struct`.
- Update `docs/ir-schema.md` with the full Slice C surface.

**Artifact:** round-trip test (Python → serde → msgspec → equal) covering every new variant. Extend the 7-site unknown-field rejection matrix from Slice B to the new sites.

### Step 3 — Evaluator support

- `manim-rs-eval`: implement each easing as a pure `fn(f64) -> f64` in a new `rate_functions` module. Port literally from `manimlib/utils/rate_functions.py`; Rust functions carry `manimgl source + commit SHA + one-line note` headers per CLAUDE.md porting practice #3.
- Evaluator reads the track variant, dispatches to the matching easing, returns the current value.
- `SceneState` grows `opacity`, `rotation`, `scale`, `color` fields per object (defaults: 1.0, 0.0, 1.0, white).

**Artifact:** Rust unit tests covering: every easing at t=0, t=0.5, t=1 (sampled against manimgl outputs — or documented deliberate deviations); each track type at three sample times; multi-track composition on a single object (position + opacity + rotation simultaneously).

**Porting note to write:** `docs/porting-notes/rate-functions.md` — table of all 15, which are verbatim ports vs. reimplementations, any behavioral deltas.

### Step 4 — Raster: fill pipeline + MSAA

- Add 4× MSAA to the existing color target. Create a matching resolve texture; render pass uses sample count 4 + resolve on end.
- Add `FillTessellator` path in `manim-rs-raster/src/tessellator.rs`. Input: `BezPath`. Output: `VertexBuffers<Vertex, u32>`.
- New pipeline `pipelines/path_fill.rs` + `shaders/path_fill.wgsl`. Uniforms: MVP + color + opacity.
- Existing stroke pipeline gains the same opacity uniform; rotation + scale fold into the per-object transform on the CPU side.
- Per-object submit from Slice B retained (`docs/gotchas.md` wgpu write_buffer ordering trap).

**Risks:** MSAA sample-count config errors, resolve-texture alignment. Fill winding rules — default to non-zero; document.

**Artifact:** `cargo run -p manim-rs-raster --example fill_aa_png` renders a filled, MSAA'd Bézier shape to PNG. Rust tests: `crates/manim-rs-raster/tests/fill_pipeline.rs` (filled square interior pixels are the fill color); `tests/msaa.rs` (edge-pixel gradient proves AA, using the existing three-object `multi_object.rs` scene as a base).

**Porting note to write:** `docs/porting-notes/fill.md` — fill pipeline delta vs. manimgl; winding rule; why we don't port manimgl's fill shader literally (same reasoning as stroke in Slice B).

### Step 5 — Python authoring API

- `python/manim_rs/objects/geometry.py`:
  - `Circle(radius, ...)`, `Rectangle(width, height, ...)` (+ `Square(side, ...)`), `Line(a, b, ...)`, `Arc(radius, start_angle, end_angle, ...)`, `Polygon(*points)`. All emit `BezPath`.
  - Keep `Polyline` as a `BezPath` factory for back-compat with Slice B tests.
- `python/manim_rs/animate/transforms.py`:
  - `Translate` (from B), plus `FadeIn`/`FadeOut`/`SetOpacity`, `Rotate`, `Scale`, `SetColor`. Each emits the matching track into `scene.play()`.
  - `Easing` enum (or string alias) usable in every animation helper.
- No `.animate` proxy, no `AnimationBuilder`. Explicit function calls only, same as Slice B.

**Reference:** `manimlib/animation/transform.py`, `manimlib/animation/fading.py`, `manimlib/animation/rotation.py` — read before writing; do not port.

**Artifact:** `tests/python/test_authoring.py` — build a scene combining each geometry factory with each animation helper, assert the resulting IR has the expected structure and time bounds.

### Step 6 — Scene discovery CLI

- `python/manim_rs/cli.py`: `render` grows `--scene PATH` and optional positional `CLASS_NAMES...`, plus `--write-all`.
- `python/manim_rs/scene_discovery.py` (new): load the module dynamically via `importlib.util.spec_from_file_location`, enumerate `Scene` subclasses, resolve names/indices, prompt via stdin if ambiguous. Mirror `extract_scene.py:40-109`.
- `python/manim_rs/__main__.py` keeps the hardcoded demo as the default *only* when no `--scene` is provided.
- Error paths: missing file, no `Scene` subclasses found, named class not in module, interactive prompt aborted.

**Reference:** `manimlib/extract_scene.py:40-109` at commit `c5e23d9`.

**Porting note to write:** `docs/porting-notes/scene-discovery.md` — what we kept, what we dropped (`--write_all` kept; `insert_embed_line_to_module` dropped; pre-run total-frame counting dropped).

**Artifact:** `tests/python/test_scene_discovery.py` — single-class file auto-picks; multi-class file with name argument picks the named one; multi-class file with bad name errors cleanly; `--write-all` returns every class. Use `pytest`'s `tmp_path` + `monkeypatch` for stdin to exercise the prompt path.

### Step 7 — Integration scene (the mandated E2E)

**This step is its own step specifically so Slice C is not "done" until the integration case ships with a passing test. Direct codification of Slice B §10's regression lesson.**

- `tests/python/test_integration_scene.py`: build a scene with **≥3 objects of ≥2 different geometry types**, each with **≥2 simultaneous track types** drawn from {position, opacity, rotation, scale, color}, using **≥2 different easings** including at least one non-linear. Both fill and stroke represented.
- Render the scene to mp4.
- Verify:
  - `ffprobe` reports expected width/height/fps/duration/codec/pix_fmt.
  - Snapshot test at ≥2 chosen frames (one mid-animation, one near end) using the tolerance-based checksum approach — sum-within-tolerance + nonzero-count-within-tolerance.
  - Per-object centroid test at the chosen frames: each object's centroid is within ±N px of the expected position (reuses the centroid technique from Slice B's post-audit tests).

**Artifact:** green CI. Visually: open the mp4, eyeball.

### Step 8 — Consolidated ADR + retrospective prep

- Write `docs/decisions/0004-slice-c-decisions.md` covering: pythonize FFI, BezPath unified primitive, all-tracks-all-easings, fill+MSAA pair, tolerance-based snapshots. Each decision ~10 lines per `docs/decisions/README.md` template.
- Update `docs/gotchas.md` with any traps surfaced during the slice.
- Leave §11 retrospective in this file empty until ship. Fill immediately on completion.

---

## 4. Explicitly Out of Scope

Belongs to Slice D+. Resist scope creep:

- Real AA stroke port from `manimlib/shaders/quadratic_bezier/stroke/` with per-vertex width — Slice D.
- Text / TeX / SVG — Slice E. Needs glyph atlas + cosmic-text + swash.
- 3D, surfaces, depth buffer, phi/theta camera — Slice F.
- Snapshot cache (memoize renders by IR hash) — later.
- Parallel / chunked rendering, incremental movie files — later.
- `set`, `reparent`, `label`, `camera_set` IR ops — no forcing use case yet.
- Shader hot reload, windowed preview, multi-scene processes.
- Cross-platform wheels (macOS x86_64, manylinux, Windows) — parallel workstream, see §9.
- Bundled or linkable ffmpeg — later.
- Quality / resolution flags beyond `--fps`.
- `--write-all` rendering to separate mp4s per class (**included**) vs. concatenated (not included).
- Persistent `Runtime` handle across calls — defer until a consumer needs it.

---

## 5. Success Criteria

- [ ] `maturin develop` builds cleanly; `pytest tests/python` and `cargo test --workspace --exclude manim-rs-py` all green.
- [ ] Command in §1 produces `out.mp4`.
- [ ] `ffprobe out.mp4` reports expected dimensions / fps / codec / pix_fmt.
- [ ] Visually: the integration scene plays correctly (multi-shape, multi-track, non-linear easings, fill, AA edges).
- [ ] FFI migrated; `_rust.eval_at` exists and is tested.
- [ ] All 15 easings, 5 track types, 1 new geometry variant in the IR; unknown-field rejection extended to all new sites.
- [ ] Step 7 integration test green.
- [ ] `Ctrl-C` still cleanly kills ffmpeg (Slice B guarantee preserved).
- [ ] Pixel-checksum tests use tolerance-based checks; no mac-arm64-specific exact values in new tests.
- [ ] `0004-slice-c-decisions.md` written.
- [ ] Retrospective §11 filled before hand-off.

---

## 6. Known Gotchas To Pre-Solve

Each costs an hour cold. Pre-empting saves the day:

1. **pyo3 0.23 idioms.** Slice B hit `detach` vs `allow_threads`. Verify `pythonize` 0.23+ API against installed source before writing signatures.
2. **`pythonize::depythonize` + msgspec.Struct.** msgspec Structs are not dicts; `depythonize` relies on `PyAny` attribute/dict access. Verify round-trip on a small struct first; fall back to `msgspec.to_builtins` on the Python side if needed.
3. **Internally-tagged unions with non-unit variants** (Slice B §10). Every new IR variant that takes params *must* be a struct variant, not a unit variant — serde's `deny_unknown_fields` is silent on unit variants under an internal tag.
4. **MSAA resolve target format alignment.** Resolve texture must match color target format and dimensions exactly; mismatches panic inside wgpu with cryptic messages.
5. **Fill winding rule.** Default to non-zero. Star-shape and self-intersecting paths will render unexpectedly with even-odd; document and pick one.
6. **Per-object submit count.** Slice B's per-object submit is preserved; this slice's richer scenes will submit more. Note-only for Slice D if frame rate suffers.
7. **Interactive stdin in tests.** Prompt path must accept piped stdin; `pytest capsys` won't help. Use `monkeypatch.setattr('builtins.input', ...)`.
8. **MSAA changes pixel values.** Old platform-exact checksums will fail on first MSAA run. This is the entire reason for the tolerance-based migration — make sure new tests don't re-pin exact values.
9. **Dynamic scene module loading.** Use `importlib.util.spec_from_file_location`. Scene files may import from their own directory — add the parent dir to `sys.path` temporarily.

---

## 7. Effort Estimate

| Step | Optimistic | Realistic | Pessimistic |
|---|---|---|---|
| 1. FFI upgrade | 3h | 6h | 1d |
| 2. IR growth | 3h | 6h | 1.5d |
| 3. Evaluator (+ all easings) | 3h | 6h | 1d |
| 4. Fill + MSAA | 1d | 2d | 3d |
| 5. Python authoring API | 4h | 1d | 1.5d |
| 6. Scene discovery | 3h | 6h | 1d |
| 7. Integration scene | 3h | 6h | 1d |
| 8. ADR + retro | 2h | 3h | 6h |
| **Total** | **~4 days** | **~7 days** | **~12 days** |

Assume realistic. Step 4 is the volatility; everything else is legwork that depends on earlier steps compiling cleanly.

---

## 8. Artifacts Produced Along The Way

Per CLAUDE.md porting practices:

- `docs/ir-schema.md` — updated with all Slice C additions (BezPath, new tracks, 15 easings).
- `docs/porting-notes/rate-functions.md` — easings port notes.
- `docs/porting-notes/fill.md` — fill pipeline delta vs. manimgl.
- `docs/porting-notes/scene-discovery.md` — CLI port notes vs. `extract_scene.py`.
- `docs/decisions/0004-slice-c-decisions.md` — consolidated ADR.
- `docs/gotchas.md` — traps added as they surface.

---

## 9. Parallel workstream (tracked, not in this slice)

Cross-platform distribution. Independent of the work above; can run in parallel without gating Slice C.

- maturin + GitHub Actions matrix producing wheels for: macOS arm64, macOS x86_64, manylinux x86_64, Windows x86_64.
- Per-platform snapshot tests (the tolerance-based approach from §2 is the mitigation — platform-exact checksums won't survive).
- ffmpeg prereq documented in README.

If this is picked up mid-slice, update this section with pointer to its own doc.

---

## 10. What Comes After Slice C

Not committed. Natural sequence, shifted one from `slice-b.md` §9:

- **Slice D:** real stroke port from `manimlib/shaders/quadratic_bezier/stroke/` with per-vertex width + AA. Snapshot cache keyed on IR hash.
- **Slice E:** Text via cosmic-text + swash, glyph atlas. TeX via LaTeX subprocess.
- **Slice F:** 3D — surface pipeline, depth buffer, phi/theta camera.

Revisit after Slice C lands.

---

## 11. Retrospective — what the plan got wrong

Completed 2026-04-22. §5 success criteria green: Rust 53 passed / 0 failed, Python 86 passed / 0 failed, `integration_scene.py` renders to mp4 with per-object centroid checks, MSAA edges verifiably smooth, tolerance-based snapshot tests in place, ADR `0004-slice-c-decisions.md` + porting notes landed.

### Plan got wrong

- **CLI shape is `render MODULE SCENE OUT [opts]`, not `--scene FILE [CLASS]`.** Plan §1 wrote the flag form; what shipped is fully positional plus `--quality` / `-r` / `--duration` / `--fps` / `-o`. The positional form is what the tests and every authored scene actually use; the flag form would have added a redundant lever. Logged in `docs/porting-notes/scene-discovery.md`.
- **`--write-all` and interactive stdin prompt did not ship.** Plan §3 Step 6 included both; we dropped them because no consumer (agentic or otherwise) needs them, and interactive prompts are hostile to pipelines. `SceneNotFoundError` carries an `available:` hint instead. Revisit if a consumer asks.
- **Python authoring API (Step 5) filled in during Step 7, not before.** The plan's Step 5 → Step 7 ordering was right in principle, but the authoring API landed as the work to make Step 7 possible — `BezPath` object class, four new animation verbs, `Colorize`, and the `easing=` kwarg all went in under the "integration scene" commit. Not a bug; the single-object Step-4 tests were sufficient to validate the raster path without an authoring pass. Consequence: for Slice D, don't split "expose to Python" from "use from Python in a test" — collapse them.
- **Effort bracket: Step 5 and Step 6 were under-estimated.** Plan said 4h / 3h optimistic. Reality: each was ~1d once the `easing=` kwarg and dispatch table across five track kinds was counted. Step 4 (MSAA + fill) came in on the optimistic end, which was the slice's loudest risk — the pre-solve list in §6 was load-bearing.
- **Pinned commit SHA drift.** Plan §6 item 1 said "verify `pythonize` 0.23+ API before writing signatures." We pinned a commit SHA in porting-note headers; one (`rate-functions.md`) points at `c5e23d9`, same as Slice B's stroke note. If the submodule advances, the SHA citations are stable; if we start editing the manimgl submodule in-tree, they rot. Guard against that in Slice D.

### Surprising calls that landed

- **`FillUniforms = StrokeUniforms` type alias.** The two pipelines' uniform buffers are `{ mat4x4 mvp, vec4 color }`. Aliasing instead of duplicating is the smallest possible expression of "these are the same layout." Keep this posture — any time two WGSL structs are byte-compatible, alias rather than re-declare.
- **Tolerance-based snapshots were the right call the first time.** ADR 0004 §E. MSAA broke the Slice B exact pins on the first render, as predicted in §6 item 8; migration was already authored, so there was nothing to rewrite. Pre-solving paid off.
- **`BezPath` as the unified primitive immediately justified itself.** Five Python factories (`Circle`, `Rectangle`, `Line`, `Arc`, `Polygon`) land as <50 lines each; no IR schema churn. The "one IR variant per shape" trap was real and we dodged it.
- **Per-object submit stayed.** Slice B §10 flagged this as a Slice D perf concern; Slice C's integration scene (3 objects, ~60 frames) produces ~180 submits per render and it's not visible. Note for the perf log, not a blocker.
- **`Colorize` requires explicit `from_color`.** The color track's "last-write-override" semantics don't infer the starting color from the object. The explicit form matches the position track (explicit `delta`), and keeps the evaluator free of object-state reads. Would revisit if authoring friction surfaces in Slice E.

### Gotchas §6 missed

- **`pythonize` returns tuples for fixed-size arrays.** `[f32; 3]` → Python `tuple`, not `list`. Any test comparing against `[0.0, 0.0, 0.0]` fails on type, not value. Now in `docs/gotchas.md`; cost ~20 min before the first test panel pointed at it.
- **f32 round-trip precision on parameterised easings.** `ThereAndBackWithPause(pause_ratio=1/3)` round-trips to a different f64 bit pattern; tests must use dyadic rationals. Now in `docs/gotchas.md`. Would have saved an hour chasing a "schema bug" that was a fixture bug.
- **H.264/yuv420p chroma shift on solid fills.** `(0, 229, 51)` decodes as approximately `(0, 240, 120)`. Integration-scene color-band masks had to widen. Now in `docs/gotchas.md`. Per-object centroid was the right test shape — narrower per-pixel assertions would have chased ghosts.
- **lyon dedupes sub-epsilon stroke points.** Caught during `GeometryOverflow` calibration; forced a zigzag fixture. Documented under the Slice C edge-cases test.

### Process observations

- **Single-commit per step held up.** Nine commits for nine concerns (IR, eval, raster fill+MSAA, raster tests, Python API, CLI, integration, perf log, docs). No mixed diffs, no "while I was in there" drift.
- **Pre-commit hook caught four issues during the commit pass** (two ruff UP007 / B008, two cargo fmt). Each fix was local and obvious; hook-as-review worked.
- **STATUS.md "rewrite don't append" continues to pay.** At no point during the slice did it grow past ~50 lines.
- **The parallel workstream (§9, cross-platform wheels) did not start.** Not a gap — it was explicitly scoped out — but worth flagging that tolerance-based snapshots make it *possible* now, which is the point of §E.

### Deltas for Slice D planning

- Collapse "expose to Python" + "use in a test" into one step.
- Keep `BezPath` verbs stable — Slice D's stroke port will add per-vertex attributes, not change the verb vocabulary.
- `path_stroke.wgsl` will evolve significantly (Loop-Blinn, per-vertex width); `path_fill.wgsl` may stay trivial for another slice depending on where we land the fill AA story.
- Slice D should re-check `rate_functions.py` at its pinned SHA — new easings have been added upstream in the past and we don't want to drift silently.
