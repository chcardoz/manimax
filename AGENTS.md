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

## Reference repos to study when in doubt

- [pydantic-core](https://github.com/pydantic/pydantic-core) — closest prior art for "Python builds IR, Rust compiles it."
- [rerun](https://github.com/rerun-io/rerun) — Python SDK + Rust runtime, Arrow-encoded messages.
- [polars](https://github.com/pola-rs/polars) — maturin workspace layout at scale.
- [3b1b/manim (ManimGL)](https://github.com/3b1b/manim) — cloned locally at `/Users/chcardoz/development/manimgl-ref/` for reference. Source of truth for what Python authoring should feel like.
