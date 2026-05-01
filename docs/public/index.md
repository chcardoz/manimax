# Manimax

A from-scratch replacement for ManimGL. **Python frontend, Rust runtime, wgpu rendering.**

Scenes compile to a typed intermediate representation (IR); the Rust runtime evaluates the IR at any time `t` and renders frames on demand. This decouples scene description (Python) from frame evaluation (Rust) — random access, parallel chunked rendering, and multi-quality output become cheap.

## Why it exists

ManimGL treats scene code as the renderer — to get frame 1000, it re-runs the Python program from frame 0 to 1000. That makes random-access requests, partial rerenders, caching, and parallel rendering fundamentally hard. Manimax separates the two:

- **Python** authors a scene and compiles it into IR (just data).
- **Rust** consumes the IR and answers frame requests.
- Frame at `t` is a pure function of `(IR, t)` — no replay.

## Status

Slices B → E shipped — Python authoring → IR → Rust eval → wgpu raster → in-process libavcodec → mp4. Real strokes, fills, snapshot caches, text (cosmic-text + swash), and math (RaTeX). See [Getting started](getting-started.md) to render your first scene, or [Examples](examples.md) to see the API in action.

For internal architecture, decision records, slice plans, and porting notes, the markdown sources live in the [`docs/`](https://github.com/chcardoz/manimax/tree/main/docs) directory of the repo.
