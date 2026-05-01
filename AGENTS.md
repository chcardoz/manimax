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

**Slices B → E shipped** (end-to-end: Python → IR → Rust eval → wgpu raster → in-process libavcodec → mp4). Real strokes, fills, snapshot caches, text via cosmic-text + swash, math via RaTeX. The full docs site is in `docs/public/` (also published via mkdocs). Read it first, then `STATUS.md` for what's being worked on right now.

## Read before you touch anything

All paths below are inside `docs/public/`.

1. `concepts/architecture.md` — the dense context dump. Architecture, stack decisions, research summary, what was ruled out and why.
2. `concepts/ir-schema.md` — the Python↔Rust contract.
3. `design/` — numbered decision records (`NNNN-slug.md` rewritten with descriptive slugs). Read before changing anything a prior decision touched. **Write a new one** when you pick between credible alternatives (library X vs Y, schema shape, protocol, scope boundary) or make any choice a future agent might reasonably try to undo. Two shapes — atomic (~10 lines, one decision) or consolidated per-slice (150–300 lines, sectioned A/B/C). Index in `design/index.md`.
4. `changelog.md` — what shipped slice by slice, with retrospectives ("what surprised us, what the plan got wrong"). Read the most recent before planning the next slice.
5. `contributing/gotchas.md` — non-obvious traps (API deltas, shell quirks, invariants that bite). Skim before starting in an unfamiliar subsystem. Add an entry any time you lose >15 minutes to a trap not already listed.
6. `contributing/performance.md` — running list of perf observations and ideas to batch into a future performance pass. **If you notice anything perf-relevant (slow path, unused lever, measurement), append an entry there** rather than acting on it in isolation.
7. `contributing/porting-from-manimgl.md` — per-subsystem porting notes (one `##` per subsystem). Read the relevant section before porting a manimgl-adjacent feature.
8. `roadmap.md` — deferred work with concrete triggers for revisiting.
9. `STATUS.md` (repo root) — current state of work in progress: active slice, what the last session did, next action, blockers. **Read this last** (it's the freshest) and **rewrite it at the end of every session** before handing back. Keep it under ~50 lines; anything larger probably belongs in a design note or the changelog.

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

# Preview the docs site locally (live-reload). Opens at http://127.0.0.1:8000/manimax/
NO_MKDOCS_2_WARNING=true mkdocs serve
```

**Gotcha:** when chaining `source .venv/bin/activate` with other commands via `;`, the activation may not resolve `.venv` from the repo root. Prefer either (a) activation as the first `&&`-chained command, or (b) an absolute path to `.venv/bin/activate`. See `docs/public/contributing/gotchas.md`.

## Working rhythm

For non-trivial slices, prefer **one step at a time**: explain the next step → user confirms → implement → update `STATUS.md` → repeat. Don't batch steps. This cadence is what shipped Slice B cleanly; batching tends to produce drift between the plan and the code and an outdated `STATUS.md`.

Tasks vs `STATUS.md`: **tasks are for in-session tracking** (3+ steps inside one turn); **`STATUS.md` is the between-session handoff**. They don't duplicate. Close tasks inside the session; rewrite `STATUS.md` at the end before handing back.

### Clean-tree gate (do this before the first edit of every session)

Before the **first** file mutation of a session — `Edit`, `Write`, `NotebookEdit`, **or** a `Bash` command that changes the working tree (`rm`, `mv`, `cp`, redirection, `sed -i`, `git checkout/restore`, etc.) — check the working tree:

1. Run `git status` and `git diff --stat`. If both report a clean tree, proceed.
2. If the tree is dirty, **stop**. Invoke the `/git-commit` skill to propose a logical commit grouping for the existing changes. Present the grouping to the user, explain it, and ask for approval before committing.
3. Only after the user approves the commit (or explicitly waives the check — "go ahead", "skip the commit", etc.) may you proceed with the original edit.

This applies to *Bash mutations too*, not just editor tools. `rm somefile.rs` counts as a first edit. The point is to land in-flight work as a clean diff before you start mixing a new task into it; bypassing this rule via `Bash` defeats the whole purpose.

Once you've checked (or been waived) once in a session, you don't need to re-check; subsequent edits in the same session go through.

## Quick pointers

- **Language split:** Python authoring → IR → Rust runtime (eval + wgpu raster + ffmpeg encode). See `docs/public/concepts/architecture.md` §2–§5 for the full stack and version pins.
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

1. **Porting notes.** When you port a subsystem, append a `## <subsystem>` section to `docs/public/contributing/porting-from-manimgl.md` capturing invariants, API shape, and edge cases that aren't obvious from the manimgl source. 200–500 words. These compound — over time they become the primary reference and the submodule fades to a fallback.
2. **Distinctive stub labels.** Use `PORT_STUB_MANIMGL_<subsystem>` (not `TODO`) for placeholder Rust waiting on a real port. Greppable, unambiguous.
3. **Per-function attribution.** When porting a non-trivial algorithm, put a short header comment on the Rust function: manimgl source file + commit SHA + one-line note. Answers "which manimgl version does this match?" at the function level; also satisfies MIT attribution.
4. **Literal-first translation.** First pass keeps manimgl's variable names and control flow even if it's ugly Rust. Only refactor to idiomatic Rust after it works. Eliminates "logic bug vs. porting bug" ambiguity.

### Other prior art

- [pydantic-core](https://github.com/pydantic/pydantic-core) — closest analog for "Python builds IR, Rust compiles it."
- [rerun](https://github.com/rerun-io/rerun) — Python SDK + Rust runtime, Arrow-encoded messages.
- [polars](https://github.com/pola-rs/polars) — maturin workspace layout at scale.
