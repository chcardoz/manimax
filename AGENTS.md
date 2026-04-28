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

**Slice B shipped** (end-to-end: Python → IR → Rust eval → wgpu raster → ffmpeg mp4). The full architecture, stack decisions, research, and reasoning live in `docs/architecture.md`. Read that first, then `STATUS.md` for what's being worked on right now.

## Read before you touch anything

1. `docs/architecture.md` — the dense context dump. Architecture, stack decisions, research summary, what was ruled out and why, what to build first.
2. `docs/ir-schema.md` — (TODO) the IR specification. Source of truth for the Python↔Rust contract.
3. `docs/decisions/` — numbered decision records (`NNNN-slug.md`, ADR-lite). Read these before changing anything a prior decision touched. **Write a new one** when you pick between credible alternatives (library X vs Y, schema shape, protocol, scope boundary) or make any choice a future agent might reasonably try to undo. Use the next unused number. ~10 lines: Decision / Why / Consequences / Rejected alternatives. Template in `docs/decisions/README.md`.
4. `docs/slices/` — execution plans for each end-to-end vertical slice. Start with the latest active slice. Completed slices append a **Retrospective** section (what surprised us, what the plan got wrong) — read it before writing the next slice's plan. Each slice plan pins scope, work breakdown, success criteria, and explicit out-of-scope — the **what to build now**, as opposed to `architecture.md`'s **what the system is**.
5. `docs/gotchas.md` — aggregator of non-obvious traps (API deltas, shell quirks, invariants that bite). Skim this before starting a session in an unfamiliar subsystem. Add an entry any time you lose >15 minutes to a trap not already listed.
6. `docs/performance.md` — running list of perf observations and ideas to batch into a future performance pass. **If you notice anything perf-relevant (slow path, unused lever, measurement), append an entry there** rather than acting on it in isolation.
7. `STATUS.md` — current state of work in progress: active slice, what the last session did, next action, blockers. **Read this last** (it's the freshest) and **rewrite it at the end of every session** before handing back. Keep it under ~50 lines; anything larger probably belongs in a slice plan or porting note.

## Dev commands

### First-time setup

Fresh clone or fresh Conductor worktree — run once before anything else:

```sh
./scripts/setup.sh
```

This initializes the `reference/manimgl` submodule, creates `.venv` via `uv`, installs the package with dev extras, and runs `maturin develop`. Idempotent. Cold `maturin develop` is 1–3 min.

Conductor users: `conductor.json` points `scripts.setup` at this same script, so new worktrees bootstrap automatically. If you've populated Conductor's Repository Settings → Scripts UI for this repo, clear it — per Conductor docs, UI Scripts fully override `conductor.json`.

### Day-to-day

The five commands a new agent needs within five minutes:

```sh
# Rebuild the pyo3 extension after Rust changes.
source .venv/bin/activate && maturin develop

# Rust tests (all crates in the workspace).
# manim-rs-py's `extension-module` is gated behind a feature so this works
# without `--exclude`; maturin still turns it on for actual extension builds.
cargo test --workspace

# Python tests (pytest config in pyproject.toml).
pytest tests/python

# End-to-end smoke (Slice B — produces a viewable mp4).
python -m manim_rs render /tmp/out.mp4 --duration 2 --fps 30

# Verify an mp4 deterministically.
ffprobe -v error -select_streams v:0 -count_frames \
  -show_entries stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames \
  -of default=noprint_wrappers=1 /tmp/out.mp4
```

**Gotcha:** when chaining `source .venv/bin/activate` with other commands via `;`, the activation may not resolve `.venv` from the repo root. Prefer either (a) activation as the first `&&`-chained command, or (b) an absolute path to `.venv/bin/activate`. See `docs/gotchas.md`.

## Working rhythm

For non-trivial slices, prefer **one step at a time**: explain the next step → user confirms → implement → update `STATUS.md` → repeat. Don't batch steps. This cadence is what shipped Slice B cleanly; batching tends to produce drift between the plan and the code and an outdated `STATUS.md`.

Tasks vs `STATUS.md`: **tasks are for in-session tracking** (3+ steps inside one turn); **`STATUS.md` is the between-session handoff**. They don't duplicate. Close tasks inside the session; rewrite `STATUS.md` at the end before handing back.

## Quick pointers

- **Language split:** Python authoring → IR → Rust runtime (eval + wgpu raster + ffmpeg encode). See `docs/architecture.md` §2–§5 for the full stack and version pins.
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
