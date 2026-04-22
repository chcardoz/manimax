#!/usr/bin/env bash
# Bootstrap a fresh checkout or Conductor worktree to a state where
# `pytest tests/python` and `cargo test --workspace --exclude manim-rs-py`
# both pass. Idempotent — re-running is safe.
#
# Invoked automatically by Conductor via `conductor.json` on worktree
# creation. Humans can run it directly from any fresh clone.

set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> manimax setup"

# 1. Submodules. `reference/manimgl` is the primary porting reference.
if [ ! -f "reference/manimgl/manimlib/__init__.py" ]; then
    echo "--> initializing submodules"
    git submodule update --init --recursive
else
    echo "--> submodules already initialized"
fi

# 2. Python venv. `uv` is the declared tool (docs/architecture.md §6).
if [ ! -d ".venv" ]; then
    echo "--> creating .venv with uv"
    uv venv
else
    echo "--> .venv already exists"
fi

# shellcheck source=/dev/null
source .venv/bin/activate

# 3. Python deps. `-e` so edits to python/ apply without reinstall.
echo "--> installing python deps (incl. dev extras)"
uv pip install -e ".[dev]"

# 4. Rust extension. Cold-build is 1–3 min; warm rebuild is seconds.
echo "--> building rust extension (maturin develop)"
maturin develop

echo "==> setup complete. next: pytest tests/python"
