# Manimax — Architecture, Stack, and Design Context

**Date of last update:** 2026-04-20
**Context:** This document is the full dump of every decision, rejected option, and piece of research that led to the current architecture. It exists because the previous conversation was long and the next agent needs to pick up cold. Read this completely before proposing any change.

---

## 1. The Problem Manimax Solves

ManimGL (3Blue1Brown's renderer) treats **scene code as the renderer**. To render frame `N`, it replays Python execution from frame 0 to frame `N`, mutating mobjects each frame and rasterizing as it goes. This makes fundamentally hard:

- Random-access frame requests (`render frame at t=3.2`)
- Rendering arbitrary ranges (`render [t0, t1]`)
- Rendering the same scene at multiple qualities without replay
- Partial rerender / caching
- Parallel rendering of a single scene across workers
- Escaping the Xvfb / pyglet / moderngl / X11 stack for headless rendering

Manimax's core idea: **decouple scene description from frame evaluation via a typed Intermediate Representation (IR)**. Python authors a scene; the Python library compiles it into an IR (just data); Rust consumes the IR and answers frame requests. Rust never runs Python. Python never rasterizes.

Goal properties that fall out:
- Frame at `t` is a pure function of (IR, t) — no replay.
- Parallel chunked rendering is trivial (hand different ranges to different workers).
- Same IR rasterizes at 480p or 4K without re-running the scene.
- IR is a stable serializable artifact — cacheable, hashable, shippable.

---

## 2. Architecture at a Glance

```
┌─────────────────────────────────────────┐
│ Python frontend (authoring)             │   Python package: manim_rs
│ Scene, Circle, Polyline, play(), wait() │   Everything the user/agent imports
│ Records imperative operations into IR   │
└──────────────┬──────────────────────────┘
               │
               ▼  (serde JSON or msgpack across FFI)
┌─────────────────────────────────────────┐
│ IR — typed data, not code               │   defined by schema in both langs
│ objects, timeline ops, tracks, labels   │   kurbo/peniko types on Rust side
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│ Rust runtime                            │   Workspace of crates, one pyo3 cdylib
│ eval(ir, t) → SceneState                │   eval separated from raster
│ raster(SceneState) → pixels (wgpu)      │
│ encode(frames) → mp4 (subprocess ffmpeg)│
└─────────────────────────────────────────┘
```

**Single shippable package:** `pip install manim-rs`. User sees only Python. Rust is a `.so`/`.pyd` inside the wheel. The `manim-rs` in the PyPI name is optional — inside Python it's `import manim_rs`.

---

## 3. What Goes Where (the manimgl → manimax mapping)

### Python (authoring + geometry construction)

Everything that **describes** a scene lives on the Python side. This is substantial — the authoring surface of ManimGL is not small, and Manimax keeps almost all of it.

Ported from ManimGL's `manimlib/`:
- `manimlib/mobject/` (all of it, adapted): `Circle`, `Polygon`, `Arrow`, `Axes`, `NumberLine`, `Matrix`, `ParametricFunction`, `VectorField`, etc. Each constructor computes point arrays with numpy; instead of a GPU-bound VMobject, it creates an IR object table entry.
- `manimlib/mobject/svg/`: `Tex`, `Text`, SVG parsing. LaTeX subprocess runs at scene-construction time. Output paths bake into the IR.
- `manimlib/utils/`: `bezier.py`, `color.py`, `space_ops.py`, `rate_functions.py`, `tex_file_writing.py`.
- `manimlib/constants.py`: colors, directional vectors, PI, etc.
- `manimlib/scene/scene.py`: **rewritten**. Same `play/wait/add/remove/checkpoint` surface, but the implementation appends to an IR buffer instead of running a render loop.
- `manimlib/animation/`: **rewritten as track generators**. `FadeIn` used to mean "update opacity each frame"; now it means "emit an opacity property track from t0 to t1 with v0=0, v1=1." Same classes, same API, radically different implementation.

### Rust (timeline evaluation + rasterization + encoding)

Everything that **consumes** an IR and produces pixels.

Reimplemented in Rust:
- `manimlib/camera/camera.py` → Rust camera state + view matrix application
- `manimlib/shader_wrapper.py` + `manimlib/shaders/` → new WGSL shaders + wgpu pipelines
- `manimlib/scene/scene_file_writer.py` → subprocess ffmpeg in Rust
- `manimlib/animation/animation.py`'s interpolation loop → Rust timeline evaluator

Net-new (no ManimGL equivalent):
- IR parser/validator
- Scene graph built from IR
- Track interpolation engine with named easings
- Snapshot cache for fast seeking
- FFI surface exposed via pyo3

### Dropped entirely (v1, possibly forever)

These parts of ManimGL don't survive because they aren't compilable to a static IR:
- `manimlib/mobject/mobject_update_utils.py` — updater callbacks (`always_redraw`, `always`, `f_always`, etc.)
- `manimlib/animation/update.py` — `UpdateFromFunc`, `MaintainPositionRelativeTo`
- `manimlib/mobject/interactive.py` — interactive scene features
- `manimlib/scene/interactive_scene.py`, `scene_embed.py`
- `manimlib/event_handler/`
- `manimlib/window.py` (live window)
- Pyglet / moderngl / Xvfb / X11

**Updaters are the biggest loss.** Real 3B1B scenes use them. The tradeoff is explicit in the design docs — you cannot have opaque per-frame Python callbacks AND random-access frame evaluation. Manimax chose random-access. Scenes that rely on updaters must be rewritten or retained in ManimGL (hybrid not in v1).

### The seam

When in doubt: **does this code describe the scene, or does it draw the scene?** That's the Python/Rust boundary.

---

## 4. The IR

The IR is the entire contract between Python and Rust. It has 6 sections:

1. **Scene metadata** — name, duration, IR version, units.
2. **Object table** — stable IDs, kind (polyline/circle/rect/text/tex/group/surface), geometry payload (uses `kurbo::BezPath` vocabulary), style (uses `peniko::{Color, Brush, Stroke}`), default transform, parent relationships.
3. **Timeline ops** — discrete events: `add`, `remove`, `set`, `reparent`, `label`, `camera_set`. Each carries a time `t`.
4. **Property tracks** — continuous changes over time. Per (object, prop) the track is a list of segments `{t0, t1, v0, v1, ease}`. First-version properties: position, scale, rotation, opacity, stroke_color, fill_color, stroke_width, camera_center, camera_zoom.
5. **Labels** — named checkpoints at specific times. Used for inspection, debug, audio alignment.
6. **Snapshots** — periodic fully-materialized scene states. Runtime-owned (not authored). Strategy decided by runtime for fast seeking.

Design rules on the Python side:
- Stable object identity (each handle gets a permanent ID at construction time).
- Explicit compile-time clock advanced by `play`/`wait`.
- Immediate mutations before `play` (e.g. `circle.shift(RIGHT)` outside `play` records a `set` op and updates current object state; doesn't spawn a track).
- No untracked side effects — if the API doesn't have a lowering rule, it's rejected.
- Deterministic easing names shared by Python and Rust: `linear`, `in_quad`, `out_quad`, `in_cubic`, `out_cubic`, `in_out_cubic`, etc.

On the Rust side:
- Build scene graph from IR at load time.
- Evaluate state at time `t`: membership from ops ≤ `t`, discrete properties from set ops ≤ `t`, continuous properties from active track segments at `t`.
- Separate evaluation from rasterization — same `SceneState` rasterizes at any quality.
- Snapshots are cheap checkpoints so `frame_at(t)` doesn't replay from 0.

Read `docs/ir-schema.md` for the authoritative schema (to be written). Read the original specs at `../mogadishu/docs/superpowers/specs/2026-04-20-compiled-renderer-*.md` for longer-form rationale.

---

## 5. Rust Stack (April 2026, verified)

All versions and recommendations below were verified via web research in April 2026. Flag if any of this rots.

### Toolchain
- **Rust 1.95 stable**, edition **2024**, resolver **3**. Pin Rust in `rust-toolchain.toml`. MSRV 1.83 (pyo3 0.28 floor). Edition 2024 is the current edition name, not the calendar year.
- **pyo3 0.28.2**. Breaking changes in last 12 months: `IntoPyObject` rework (0.23), `FromPyObject` rework (0.28), `.downcast()` → `.cast()` (deprecated), modules default to `Py_MOD_GIL_NOT_USED`.
- **maturin 1.13.1**. Use `python-source = "python"`, `module-name = "manim_rs._rust"`.

### Core dependencies
- **`wgpu` 29** — GPU abstraction. Headless-friendly (surfaceless Vulkan/Metal/D3D12). Production-proven (Bevy, Zed, Firefox, Deno, Servo, rerun).
- **`lyon` 1.0** — 2D tessellation (Bezier paths → triangle meshes for wgpu).
- **`kurbo`** (Linebender) — path math vocabulary for the IR.
- **`peniko`** (Linebender) — brush/color vocabulary for the IR.
- **`cosmic-text`** — text shaping stack (uses `harfrust` + `fontdb` + `swash` internally).
- **`swash`** — glyph rasterization / glyph-to-path for TeX.
- **`ttf-parser`** — low-level font parsing for TeX glyph extraction.
- **`glam` 0.28** — linear algebra (matrices, vectors) for 2D and 3D camera math.
- **`image`** — PNG/JPEG encode for debug output.
- **`serde` + `serde_json`** — IR serialization (human-readable; debuggability matters more than bytes here).
- **`postcard`** — binary serialization for caches/snapshots (beats bincode in size and speed; serde-compatible).
- **`tracing` + `tracing-subscriber`** — structured logging.
- **`thiserror`** in library crates, **`color-eyre`** in the CLI binary.
- **`bytemuck`** — zero-copy casts between structs and GPU byte buffers.

### Why wgpu, not tiny-skia or Vello
- tiny-skia: 2D only, CPU only. Good but the 3D gap is real.
- Vello: 2D only, still **alpha in April 2026** (Linebender's own wording). `vello_cpu` 0.6 exists but API is explicitly unstable.
- Skia (via skia-safe): 2D only, heavy C++ dep.
- ManimGL has 3D content (Surfaces, Sphere, Torus, 3D cameras, lighting). Manimax commits to 3D. Only wgpu gives 2D + 3D in one Rust stack without a dep on an alpha engine.

The cost is real: with wgpu, you **write the renderer** (WGSL shaders, tessellation pipeline, glyph atlas, 3D surface pipeline). Not "integrate a renderer." Expect 4-6 weeks of Rust work to get the first end-to-end pipeline.

### Rejected for specific reasons
- `tiny-skia`: no 3D. Discussed above.
- Vello: alpha, unstable. Revisit when it hits beta.
- `raqote`: effectively dormant.
- `pathfinder` (Mozilla): dead since 2021.
- `piet`: maintenance mode; Linebender steered users to Vello.
- `femtovg`: fine but smaller ecosystem than lyon+wgpu.
- `ffmpeg-next`: maintenance-only mode. Use subprocess.
- `rsmpeg`: only needed for in-process filter graphs — we don't need that.
- Direct NVENC bindings: painful, no reason vs. `ffmpeg -c:v h264_nvenc`.

### Testing / dev tooling
- **`cargo-nextest`** as test runner (3× faster than `cargo test`).
- **`insta`** for snapshot tests (text, JSON round-trips).
- **`rstest`** for parameterized tests.
- **`proptest`** for property-based testing of IR invariants.
- **`image-compare`** for SSIM-based image regression (do NOT byte-compare PNGs — GPU/driver nondeterminism is real).
- **`divan`** for wall-clock benchmarks (has overtaken criterion in 2026).
- **`iai-callgrind`** for CI-stable instruction-count benchmarks.

### Lints / security
- `clippy` (of course).
- `cargo-machete` — unused deps (fast, text-based).
- `cargo-udeps` — unused deps (compiler-accurate, nightly; run weekly, not per-PR).
- `cargo-semver-checks` — catch accidental API breaks.
- `cargo-deny` — license, advisory, duplicate-version policy. Preferred over bare `cargo-audit`.
- `cargo-miri` — UB detection on unsafe blocks (relevant for wgpu/ffmpeg FFI).

### Release
- **`release-plz`** — creates release PRs from conventional commits, runs `cargo-semver-checks`, publishes to crates.io. Default for multi-crate workspaces in 2026.
- Version sync with Python: CI script that greps `pyproject.toml` version against all `Cargo.toml` versions and fails on mismatch. Polars does this in `release-python.yml`.

---

## 6. Python Stack (April 2026, verified)

### Language / build
- **Python 3.11+** (3.10 is security-only, EOL Oct 2026). Free-threaded (`cp313t`, `cp314t`) wheels added later, not in v1 — abi3t isn't standardized yet (PEP 803 in flight).
- **Maturin** as build backend. Package `manim_rs`, compiled submodule `manim_rs._rust`. Layout: Python under `python/manim_rs/`, matches Polars / pydantic-core.
- **uv** for env + lock. Commit `uv.lock`.

### Lint / format / type check
- **`ruff`** — lint + format. Single tool replaces black, isort, flake8, pyupgrade, pydocstyle.
- **Pyright** in CI (public contract check).
- **`ty`** (Astral) locally for speed. Beta as of Dec 2025. Not mission-critical ready yet, so not the CI gate.

### Testing
- **`pytest`**.
- **`hypothesis`** — property-based; great for "Python IR round-trips match Rust eval" parity tests.
- **`syrupy`** with `PNGSnapshotExtension` for image snapshot regression. SSIM tolerance ≥ 0.99. Don't byte-compare — GPU/driver variance.
- **`pytest-benchmark`** for per-frame timing checks.

### Data shapes
- **`msgspec.Struct`** for Python-side IR types. ~12× pydantic v2, ~4× dataclasses/attrs for struct construction. Perfect for a hot-path IR where you're building thousands of track segments per scene.
- Python dataclasses / msgspec Structs mirror Rust serde types. Start hand-maintained. Add codegen from `schemas/ir.schema.json` if drift becomes a problem.

### Other
- **`typer`** for CLI (thin type-hint layer over Click).
- **stdlib `logging`** with `NullHandler` on the package logger. NEVER ship structlog/loguru from a library — forces global config on consumers.
- **`mkdocs-material`** for docs. Plan Zensical migration when it ships (material announced minimal maintenance through late 2026).
- Hand-written **`_rust.pyi`** stubs + **`pyo3-stub-gen`** (0.22+) for the compiled submodule.
- **`py.typed`** marker file (PEP 561).

### FFI choice for the IR
Three tiers, pick by shape:
1. **`#[derive(FromPyObject)]`** — best for small, stable IR. Zero extra deps, good errors.
2. **`pythonize` / `serde-pyobject`** — best for deeply nested schema-evolving IR where you have `serde` derives anyway.
3. **msgpack/JSON wire format across FFI** — if Python already serializes.

**For Manimax: start with `#[derive(FromPyObject)]` + enum variants for op unions.** Shift to `serde-pyobject` only if the IR gets deeply recursive.

Release GIL around heavy Rust work: `py.detach(|| { /* render */ })` (0.28 naming, `allow_threads` still aliased).

### Zero-copy numpy
`rust-numpy` (≥ 0.23 for free-threaded support). `PyReadonlyArray2<f32>` with `as_slice()?` gives zero-copy `&[f32]` — document that point arrays must be C-contiguous (or Python side calls `np.ascontiguousarray`).

---

## 7. CI, Release, Security

### GitHub Actions
- Matrix: OS × Python 3.11–3.14.
- **`PyO3/maturin-action@v1`** for wheels (NOT cibuildwheel — maturin-action is the standard for pyo3 projects).
- **`Swatinem/rust-cache@v2`**, **`astral-sh/setup-uv@v7`**.
- Gate branch protection with **`re-actors/alls-green@release/v1`**.

### Wheel matrix
- `manylinux_2_28` (NOT manylinux2014 — being deprecated).
- `musllinux_1_2`.
- macOS x86_64 + arm64 **separate wheels** (ruff explicitly stopped shipping universal2).
- Windows x86_64 (+ optional arm64 on `windows-11-arm` runner).
- Free-threaded (`cp313t`, `cp314t`) added later.

### PyPI publishing
- **Trusted Publishers (OIDC)** — no long-lived tokens. `pypa/gh-action-pypi-publish@release/v1` with `permissions: id-token: write`.
- Tag-triggered release workflow (`on: push: tags: ['v*']`).

### Pre-commit
- `ruff check --fix`, `ruff format`, `cargo fmt`, `validate-pyproject`, `end-of-file-fixer`.

### Security
- Dependabot (cargo + pip) native.
- `cargo-deny` scheduled.
- `pip-audit` / `uv pip audit` for Python.
- CodeQL for both languages.

---

## 8. Repo / File Tree (target state)

```
manimax/
├── Cargo.toml                              # workspace, edition "2024", resolver "3"
├── Cargo.lock
├── rust-toolchain.toml                     # pin 1.95
├── pyproject.toml                          # maturin backend
├── uv.lock
├── deny.toml                               # cargo-deny policy
├── README.md
├── AGENTS.md                               # product summary (exists)
├── CLAUDE.md                               # symlink to AGENTS.md (exists)
├── LICENSE
├── CHANGELOG.md
├── .pre-commit-config.yaml
├── .cargo/
│   └── config.toml                         # nextest as test runner
├── .github/
│   ├── dependabot.yml
│   └── workflows/
│       ├── ci.yml                          # Rust + Python test matrix
│       ├── wheels.yml                      # tag-triggered, maturin-action + Trusted Publishing
│       └── security.yml                    # cargo-deny, pip-audit, scheduled
├── docs/
│   ├── architecture.md                     # this file
│   ├── ir-schema.md                        # TODO — authoritative IR spec
│   ├── authoring.md                        # TODO — how to write scenes
│   └── pipelines.md                        # TODO — 2D vs 3D render paths
├── schemas/
│   └── ir.schema.json                      # TODO — cross-language source of truth
│
├── crates/
│   │
│   ├── manim-rs-ir/                        # IR types only — no runtime deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scene.rs                    # SceneIR { metadata, objects, ops, tracks, labels }
│   │       ├── metadata.rs
│   │       ├── object.rs                   # ObjectId, Object, Geometry variants
│   │       ├── geometry.rs                 # kurbo::BezPath for 2D, mesh types for 3D
│   │       ├── style.rs                    # peniko::{Color, Brush, Stroke}
│   │       ├── ops.rs                      # TimelineOp enum
│   │       ├── tracks.rs                   # PropertyTrack, Segment, Easing
│   │       ├── labels.rs
│   │       ├── camera.rs                   # CameraState (2D + 3D unified)
│   │       └── validation.rs
│   │
│   ├── manim-rs-eval/                      # timeline evaluator — no GPU
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scene_graph.rs
│   │       ├── eval.rs                     # eval_at(ir, t) -> SceneState
│   │       ├── easing.rs
│   │       ├── interpolate.rs
│   │       ├── snapshot_cache.rs
│   │       └── state.rs
│   │
│   ├── manim-rs-raster/                    # wgpu-based rendering
│   │   ├── Cargo.toml                      # wgpu, lyon, glam, bytemuck, cosmic-text, swash
│   │   ├── shaders/                        # WGSL
│   │   │   ├── path_fill.wgsl
│   │   │   ├── path_stroke.wgsl
│   │   │   ├── text_glyph.wgsl
│   │   │   ├── surface_lit.wgsl            # 3D Phong lighting for surfaces
│   │   │   ├── depth_prepass.wgsl
│   │   │   ├── msaa_resolve.wgsl
│   │   │   └── common.wgsl
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── device.rs                   # wgpu Instance/Adapter/Device (headless)
│   │       ├── surface.rs                  # offscreen render target, MSAA, depth buffer
│   │       ├── camera.rs                   # view + projection matrices
│   │       ├── tessellator.rs              # lyon wrapper: BezPath -> VertexBuffer
│   │       ├── pipelines/
│   │       │   ├── mod.rs
│   │       │   ├── path_fill.rs
│   │       │   ├── path_stroke.rs
│   │       │   ├── text.rs
│   │       │   └── surface_3d.rs
│   │       ├── text/
│   │       │   ├── mod.rs
│   │       │   ├── shaping.rs              # cosmic-text integration
│   │       │   ├── glyph_atlas.rs          # swash raster -> GPU atlas
│   │       │   └── sdf.rs                  # optional MSDF for scalable text
│   │       ├── render.rs                   # render(SceneState) -> framebuffer
│   │       └── readback.rs                 # GPU texture -> CPU PNG bytes
│   │
│   ├── manim-rs-encode/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ffmpeg.rs                   # subprocess PNG sequence -> mp4
│   │       └── nvenc.rs                    # future: direct GPU-encode (feature flag)
│   │
│   ├── manim-rs-runtime/                   # glue: eval + raster + encode
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── render_frame.rs
│   │       ├── render_range.rs
│   │       ├── list_labels.rs
│   │       └── errors.rs
│   │
│   └── manim-rs-py/                        # the pyo3 cdylib
│       ├── Cargo.toml                      # cdylib type; deps on runtime + ir
│       └── src/
│           └── lib.rs                      # #[pymodule] manim_rs._rust
│
├── python/
│   └── manim_rs/
│       ├── __init__.py                     # re-exports Scene, objects, animations, constants
│       ├── _rust.pyi                       # hand-written stubs for compiled submodule
│       ├── py.typed                        # PEP 561 marker
│       ├── scene.py                        # Scene class, clock, IR recording, render dispatch
│       ├── ir.py                           # msgspec.Struct mirroring Rust IR types
│       ├── easing.py                       # names matching Rust enum
│       ├── constants.py                    # BLUE/RED/UP/DOWN/PI
│       ├── colors.py
│       ├── camera.py                       # camera state recording
│       ├── cli.py                          # typer app
│       ├── objects/
│       │   ├── __init__.py
│       │   ├── base.py
│       │   ├── group.py
│       │   ├── geometry.py                 # Circle, Line, Polyline, Rectangle, Polygon,
│       │   │                               # Arc, Arrow, Dot, Square, RegularPolygon
│       │   ├── number_line.py
│       │   ├── axes.py                     # Axes, NumberPlane, ThreeDAxes
│       │   ├── coordinate_systems.py
│       │   ├── functions.py                # ParametricFunction, FunctionGraph
│       │   ├── matrix.py
│       │   ├── probability.py
│       │   ├── vector_field.py
│       │   ├── shape_matchers.py
│       │   ├── text.py
│       │   ├── tex.py                      # LaTeX subprocess + dvisvgm
│       │   ├── svg.py
│       │   └── three_d.py                  # Sphere, Torus, Surface
│       ├── animate/
│       │   ├── __init__.py
│       │   ├── builder.py                  # .animate proxy, AnimationBuilder
│       │   ├── transforms.py
│       │   ├── fading.py
│       │   ├── creation.py                 # Write, ShowCreation, DrawBorderThenFill
│       │   ├── movement.py
│       │   └── indication.py
│       └── utils/
│           ├── __init__.py
│           ├── bezier.py
│           ├── paths.py
│           ├── space_ops.py
│           ├── iterables.py
│           ├── tex_writing.py
│           └── caching.py
│
├── tests/
│   ├── python/
│   │   ├── conftest.py
│   │   ├── test_scene_recording.py
│   │   ├── test_ir_roundtrip.py
│   │   ├── test_animations.py
│   │   ├── test_render_frame.py
│   │   └── snapshots/                      # syrupy PNG snapshots
│   └── rust/
│       ├── integration/
│       │   ├── eval_tests.rs
│       │   ├── tessellation_tests.rs
│       │   └── render_tests.rs             # image-compare vs reference PNGs
│       └── snapshots/
│
├── benches/                                # divan
│   ├── eval_throughput.rs
│   ├── tessellation.rs
│   └── frame_render.rs
│
└── examples/
    ├── 01_hello.py
    ├── 02_fourier.py
    ├── 03_graph_layout.py
    ├── 04_parametric_surface.py            # exercises the 3D pipeline
    └── 05_tex_equations.py
```

---

## 9. What Gets Built First (Critical Path)

Goal: shortest end-to-end slice that proves the pipeline.

1. **Freeze the IR schema.** Write `docs/ir-schema.md` and `schemas/ir.schema.json`. Types only: scene metadata, object table with a single kind (polyline), timeline ops (add/remove), one property track (position). No text, no 3D, no snapshots yet. **Once this is frozen, Python and Rust can proceed in parallel.**
2. **Python Scene + Polyline + play + wait** — just enough to emit an IR with one polyline that moves. Stub everything else.
3. **Rust IR deserialization** — parse the IR into serde structs. Round-trip test.
4. **Rust evaluator** — `eval_at(ir, t) -> SceneState` for the polyline-only IR.
5. **Rust wgpu bringup** — headless wgpu device, offscreen render target, clear to blue, read back a PNG.
6. **Rust lyon path tessellation** — tessellate one polyline into a vertex buffer.
7. **First wgpu pipeline — path_stroke.wgsl** — draw the tessellated polyline with a color.
8. **Wire Python → Rust via pyo3** — `manim_rs._rust.render_frame(ir_dict, t, width, height) -> bytes`.
9. **CLI** — `manim-rs render scene.py --t=0.4 --out=frame.png`.
10. End-to-end slice works. Then iterate: add `Circle`, add fill pipeline, add text, add more animations, add 3D surface pipeline.

**Do not chase Vello, NVENC zero-copy, or SDF raymarching for v1.** Those are the research doc's ambitious vision. Ship the boring version first.

---

## 10. Rejected / Deferred Design Decisions

These were discussed at length and rejected for v1. Revisit only with reason.

- **Pure Python (skia-python):** 2D only. 3D surfaces not handleable. Matches the skia-python tradeoff — good for 2D, forces Xvfb hybrid for 3D. Deferred.
- **Pure Rust authoring:** breaks the LLM agent use case. Rust scene code would be verbose and lacks the Python training data advantage. Authoring stays in Python.
- **Tiny-skia as raster backend:** 2D only. Revisit only if 3D is formally dropped from v1.
- **Vello:** alpha as of April 2026. Revisit post-beta.
- **NVENC zero-copy via Vulkan↔CUDA:** research-grade. wgpu has no standardized API to export native Vulkan handles (see gfx-rs/wgpu#965). Subprocess ffmpeg is the v1 encoder.
- **Updater compilation (compiling `add_updater` to an IR):** requires embedding Python or inventing a new bytecode. Scope explosion. Deferred indefinitely.
- **Interactive scenes / event handling:** not in v1.
- **Free-threaded Python wheels:** not in v1. abi3t isn't standardized.
- **`cibuildwheel`:** rejected — `PyO3/maturin-action` is the standard for pyo3 projects in 2026.
- **`criterion` benchmarks:** rejected — `divan` + `iai-callgrind` is the 2026 pattern.
- **`cargo-audit` alone:** rejected in favor of `cargo-deny`.
- **`ffmpeg-next`:** maintenance mode. Subprocess ffmpeg.
- **`rsmpeg`:** only needed for in-process filtering — don't need that.
- **`pyxel`-style Python-drives-per-frame:** defeats the point. Python hands IR to Rust, then gets out of the way.
- **Monorepo with Divita:** rejected. Different audiences, cadences, CI needs. Manimax is a standalone library. Divita consumes it as a PyPI dep.

---

## 11. Research Artifacts Referenced

Lives in `../mogadishu/research/` and `../mogadishu/docs/superpowers/specs/`:

- `research/next-gen-renderer.md` — the ambitious "Vello + NVENC + SDF + 10-60x speedup" vision. **Useful for direction, but its numbers were not fully verified. Specifically: the "20ms glReadPixels" claim doesn't match ManimGL's actual code (which does a single `draw_fbo.read()`, more like 3-5ms on modern GPUs). The "render 1hr video in 40s with 20 workers" claim assumes NVENC zero-copy integration that doesn't exist cleanly with wgpu as of 2026.**
- `research/manim-llm-papers.md` — LLM + Manim literature review; orthogonal to renderer work but context for Divita side.
- `research/voiceover-and-rendering.md` — narration/TTS pipeline, Divita's concern.
- `research/wip-approach.md` — RL-trained Manim agent design; Divita's concern.
- `docs/superpowers/specs/2026-04-20-compiled-renderer-strategy.md` — the strategy doc. Why Python-frontend + Rust-backend + IR.
- `docs/superpowers/specs/2026-04-20-compiled-renderer-ir-and-api.md` — the concrete IR + Python API spec. Inform `docs/ir-schema.md`.
- `docs/superpowers/specs/2026-04-20-multi-scene-parallel-render-design.md` — Divita's multi-scene parallel design. Orthogonal, but the motivation (Modal timeout) evaporates when Manimax is fast.

### Prior art worth studying
- **[3b1b/manim (ManimGL)](https://github.com/3b1b/manim)** — cloned at `/Users/chcardoz/development/manimgl-ref/`. Source of truth for authoring API feel and shader math.
- **[pydantic-core](https://github.com/pydantic/pydantic-core)** — closest architectural match (Python builds IR → Rust compiles/executes).
- **[rerun](https://github.com/rerun-io/rerun)** — Python SDK + Rust runtime, production-grade.
- **[polars](https://github.com/pola-rs/polars)** — maturin multi-crate workspace at scale.
- **[ruff](https://github.com/astral-sh/ruff)** — pyo3 wheel CI patterns.
- **[tokenizers](https://github.com/huggingface/tokenizers)** — pyo3 multi-language bindings.
- **[pygfx/wgpu-py](https://github.com/pygfx/wgpu-py)** — the only graphics-adjacent Python+native stack in production; uses CFFI (not pyo3) to wrap wgpu-native. Worth reading for scene-graph structure.

---

## 12. Verified Claims vs Unverified Claims

This was fact-checked in April 2026 via web research. Flag any of these if they rot.

### Verified
- skia-python tracks Skia milestone m144, actively maintained, wheels for mac (incl arm64) + Windows + manylinux.
- Skia's `SkPath` supports `quadTo` natively (`cubicTo` too). Cairo's `curve_to` is cubic-only.
- Vello is still **alpha** in April 2026. `vello_cpu` 0.6 declared "ready for production" except API stability.
- Rust `wgpu` is at v29.0.1.
- pyo3 0.28.2 + maturin 1.13.1 are current.
- WebKitGTK 2.46 replaced Cairo with Skia.
- Kimi K2.5 = Claude Sonnet 4 on ManiBench Executability (66.7%) — but Sonnet 4 is cleanly better on Alignment (100% vs 91.7%) and VCER (0% vs 8.3%).
- manylinux_2_28 is becoming standard; manylinux2014 is being deprecated.
- PyPI Trusted Publishers (OIDC) is standard in 2026.
- `PyO3/maturin-action` is the default for pyo3 CI, not cibuildwheel.
- `cargo-nextest`, `divan`, `iai-callgrind`, `insta`, `rstest`, `proptest`, `image-compare`, `release-plz` are all current defaults.

### Unverified / approximate
- "Cairo 2-3x slower than Skia on complex paths" — direction correct, magnitude unsourced.
- "ManimGL is 43ms/frame at 1080p with 20ms glReadPixels" — unverified. Actual `camera.py:141` uses a single `draw_fbo.read()` which on modern GPUs is ~3-5ms. The "20ms" number likely conflates transfer with pipeline stall cost. **No public ManimGL benchmark exists.**
- "NVENC zero-copy pipeline ~1k lines" (research doc claim) — wildly optimistic. wgpu has no standardized Vulkan handle export; doing this requires unsafe hal access or a wgpu fork.
- "~30MB skia-python wheel size" — unverified.
- Vello's "177 fps M1 Max 1600²" is self-reported best case, not peer-reviewed.

---

## 13. Open Questions for the Next Agent

1. **Raster architecture:** do we do Vello-style compute rasterization (write our own) or lyon-tessellation-then-raster (boring, proven)? Current bet: **lyon + standard raster pipelines** for v1. Vello-compute later if perf demands.
2. **Text rendering strategy:** glyph atlas (fast, baked per font size) or MSDF (scalable, one-time atlas cost, more shader work)? Current bet: **glyph atlas for v1**, MSDF if zoom/scale quality becomes a problem.
3. **TeX pipeline:** keep ManimGL's approach (LaTeX → dvi → SVG → path vertices)? Probably yes. Cache outputs aggressively.
4. **3D camera semantics:** match ManimGL's Euler angles (phi/theta/gamma), or adopt a cleaner quaternion-based representation? Decide during IR schema work.
5. **Snapshot strategy:** periodic every N seconds, or at labels, or both? Runtime-owned, but needs a default policy.
6. **When does Divita switch from ManimGL to Manimax?** Probably when Manimax can render the `_starter_scene.py.txt` equivalent. Until then, Divita stays on ManimGL.

---

## 14. What This Document Is Not

- Not a step-by-step implementation guide. That's for `docs/ir-schema.md`, `docs/authoring.md`, `docs/pipelines.md` (to be written).
- Not a final API freeze. The Python surface and IR schema will iterate once real scenes start going through.
- Not the Divita plan. Divita is a separate project that will consume Manimax.

---

## 15. Summary

- **Manimax** = Python frontend + typed IR + Rust wgpu runtime.
- **Python** owns scene authoring (ported from ManimGL's `mobject/`, `utils/`, rewritten `scene/` and `animation/`).
- **Rust** owns IR evaluation + wgpu rendering + encoding.
- **wgpu** over tiny-skia/Vello because it's the only Rust stack supporting 2D + 3D in one codebase without alpha deps.
- **Stack is verified current** as of April 2026. Rust 1.95 / edition 2024 / wgpu 29 / pyo3 0.28.2 / maturin 1.13.1 / Python 3.11+.
- **Separate repo from Divita.** Manimax is a standalone library.
- **Greenfield.** Start by freezing the IR schema, then build the smallest end-to-end slice (Python records polyline → Rust renders frame at t).
