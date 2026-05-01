# Contributing

Manimax is a from-scratch ManimGL replacement: Python frontend, typed IR, Rust runtime, wgpu rendering. The architecture is described in [Concepts](../concepts/index.md); the per-decision rationale in [Design notes](../design/index.md). This page covers what you need to actually work on it.

## Read before you touch anything

In rough order:

1. **[Architecture](../concepts/architecture.md)** — the stack, the version pins, what was ruled out and why.
2. **[IR schema](../concepts/ir-schema.md)** — the Python↔Rust contract.
3. **[Design notes](../design/index.md)** — the numbered decision records. Read these before changing anything a prior decision touched.
4. **[Changelog](../changelog.md)** — what shipped slice by slice, with retrospectives. Read the most recent before planning the next.
5. **[Gotchas](gotchas.md)** — non-obvious traps. Skim before starting in an unfamiliar subsystem.
6. **[Performance](performance.md)** — running list of perf observations. Append rather than acting in isolation.
7. **`STATUS.md`** at the repo root — current state of work in progress: active slice, what the last session did, next action, blockers. Rewritten at the end of every session.

## First-time setup

Fresh clone or fresh worktree — run once before anything else:

```sh
./scripts/setup.sh
```

Initializes the `reference/manimgl` submodule, creates `.venv` via [`uv`](https://docs.astral.sh/uv/), installs the package with dev extras, runs `maturin develop`. Idempotent. Cold `maturin develop` is 1–3 min.

## Day-to-day commands

```sh
# Rebuild the pyo3 extension after Rust changes.
source .venv/bin/activate && maturin develop

# Rust tests.
cargo test --workspace

# Python tests.
pytest tests/python

# End-to-end smoke (produces a viewable mp4).
python -m manim_rs render /tmp/out.mp4 --duration 2 --fps 30

# Verify an mp4 deterministically.
ffprobe -v error -select_streams v:0 -count_frames \
  -show_entries stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames \
  -of default=noprint_wrappers=1 /tmp/out.mp4
```

## Working rhythm

For non-trivial slices, prefer **one step at a time**: explain the next step → user confirms → implement → update `STATUS.md` → repeat. Don't batch steps. Batching tends to produce drift between the plan and the code, and an outdated `STATUS.md`.

### Clean-tree gate

Before the **first** file mutation of a session — Edit, Write, NotebookEdit, **or** a Bash command that changes the working tree (`rm`, `mv`, `cp`, redirection, `sed -i`, `git checkout/restore`, etc.) — check the working tree:

1. Run `git status` and `git diff --stat`. If clean, proceed.
2. If dirty, **stop**. Propose a logical commit grouping for the existing changes, get approval, commit. Only then proceed.

Once you've checked once in a session, subsequent edits go through.

## Reference: ManimGL

ManimGL is pinned as a git submodule at `reference/manimgl/`. It's the primary reference for what Python authoring should feel like, and the source of truth for rendering/animation semantics being ported. **Before inventing a new API or translating a rendering concept, read the manimgl equivalent first.**

When you port a subsystem, append a `##` section to [Porting from ManimGL](porting-from-manimgl.md) capturing invariants, API shape, and edge cases that aren't obvious from the manimgl source. These compound — over time they become the primary reference and the submodule fades to a fallback.

### Porting practices

1. **Distinctive stub labels** — `PORT_STUB_MANIMGL_<subsystem>` (not `TODO`). Greppable, unambiguous.
2. **Per-function attribution** — when porting a non-trivial algorithm, put a short header comment on the Rust function: manimgl source file + commit SHA + one-line note.
3. **Literal-first translation** — first pass keeps manimgl's variable names and control flow even if it's ugly Rust. Refactor only after it works. Eliminates "logic bug vs. porting bug" ambiguity.

## Other prior art

- [pydantic-core](https://github.com/pydantic/pydantic-core) — closest analog for "Python builds IR, Rust compiles it."
- [rerun](https://github.com/rerun-io/rerun) — Python SDK + Rust runtime, Arrow-encoded messages.
- [polars](https://github.com/pola-rs/polars) — maturin workspace layout at scale.
