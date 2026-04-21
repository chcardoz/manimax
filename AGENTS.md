# Manimax

A from-scratch replacement for ManimGL. Python frontend, Rust runtime, wgpu rendering. Scenes compile to a typed IR; the Rust runtime evaluates the IR at any time `t` and renders frames on demand.

## What this project is

**Manimax is a standalone rendering library**, separate from any agentic system that might consume it (e.g. Divita). It replaces ManimGL as the engine that turns Python scene code into video. The key architectural difference vs ManimGL: frames are **not** produced by replaying Python execution. Python compiles the scene into an intermediate representation (IR); Rust evaluates the IR and rasterizes pixels.

## Why it exists

ManimGL treats scene code as the renderer — to get frame 1000, you re-run the Python program from frame 0 to 1000. This makes random-access frame requests, partial rerenders, caching, parallel rendering, and quality-independent output fundamentally hard. Manimax decouples **scene description** (Python) from **frame evaluation** (Rust) via an IR. Random access, chunked parallel rendering, and multi-quality output become cheap.

## Users

- Anyone writing animated math/CS/physics videos in Python.
- Agentic pipelines (e.g. Divita) that author scenes via LLM and render at scale on GPU infrastructure.

## Status

**Greenfield.** This repo was just created. No code yet. The full architecture, stack decisions, research, and reasoning live in `docs/architecture.md`. Read that first.

## Read before you touch anything

1. `docs/architecture.md` — the dense context dump. Architecture, stack decisions, research summary, what was ruled out and why, what to build first.
2. `docs/ir-schema.md` — (TODO) the IR specification. Source of truth for the Python↔Rust contract.

## Quick pointers

- **Language split:** Python for authoring (scene API), Rust for runtime (IR eval + wgpu rendering).
- **Connection:** pyo3 extension module `manim_rs._rust`, built by maturin.
- **Renderer:** wgpu 29, with lyon for 2D tessellation, cosmic-text for text, glam for 3D math. WGSL shaders in `crates/manim-rs-raster/shaders/`.
- **Encoding:** subprocess ffmpeg.
- **Repo separation:** Manimax is independent of Divita. Divita consumes Manimax as a pip dependency.

## Reference code: ManimGL

ManimGL is pinned as a git submodule at `reference/manimgl/`. It is the primary reference for what Python authoring should feel like, and the source of truth for rendering/animation semantics we're porting. **Before inventing a new API or translating a rendering concept, read the manimgl equivalent first.** Do not guess at manimgl's behavior from memory.

Contributors cloning fresh: `git clone --recurse-submodules`, or `git submodule update --init` after a normal clone.

### Key subdirectories under `reference/manimgl/manimlib/`

- `scene/` — `Scene.construct()`, lifecycle, frame/time stepping. The Python authoring loop we're replacing with IR emission.
- `mobject/` — mobject hierarchy (VMobject, geometry, tex, text, 3D). Structural vocabulary the IR must express.
- `animation/` — `Animation`, `Transform`, rate functions, composition. Maps to IR time-varying values.
- `shaders/` — GLSL for VMobject rendering, stroke/fill. Reference when porting to WGSL in `crates/manim-rs-raster/shaders/`.
- `camera/` — camera model, frame buffer, window/offline split. Informs the Rust runtime's render target abstraction.
- `utils/` — bezier math, color, tex, SVG parsing. Most of this needs a Rust port.

### Porting practices

1. **Porting notes.** When you port a subsystem, drop a short `docs/porting-notes/<subsystem>.md` capturing invariants, API shape, and edge cases that aren't obvious from the manimgl source. 200–500 words. These compound — over time they become the primary reference and the submodule fades to a fallback.
2. **Distinctive stub labels.** Use `PORT_STUB_MANIMGL_<subsystem>` (not `TODO`) for placeholder Rust waiting on a real port. Greppable, unambiguous.
3. **Per-function attribution.** When porting a non-trivial algorithm, put a short header comment on the Rust function: manimgl source file + commit SHA + one-line note. Answers "which manimgl version does this match?" at the function level; also satisfies MIT attribution.
4. **Literal-first translation.** First pass keeps manimgl's variable names and control flow even if it's ugly Rust. Only refactor to idiomatic Rust after it works. Eliminates "logic bug vs. porting bug" ambiguity.

### Other prior art

- [pydantic-core](https://github.com/pydantic/pydantic-core) — closest analog for "Python builds IR, Rust compiles it."
- [rerun](https://github.com/rerun-io/rerun) — Python SDK + Rust runtime, Arrow-encoded messages.
- [polars](https://github.com/pola-rs/polars) — maturin workspace layout at scale.
