# Getting started

## Install

Manimax is a Python package built with [maturin](https://www.maturin.rs/) — a single `pip install` produces both the Python frontend and the compiled Rust runtime.

!!! note "Pre-0.1.0"
    Manimax is not yet published to PyPI. Until 0.1.0 ships, the install path is from source.

```sh
git clone https://github.com/chcardoz/manimax.git
cd manimax
./scripts/setup.sh
```

The setup script initializes the `reference/manimgl` submodule, creates `.venv` via [`uv`](https://docs.astral.sh/uv/), installs the package with dev extras, and runs `maturin develop` to build the Rust extension. Cold builds take 1–3 minutes.

## Render your first scene

The CLI ships a smoke-test scene out of the box:

```sh
python -m manim_rs render /tmp/out.mp4 --duration 2 --fps 30
```

Verify the output:

```sh
ffprobe -v error -select_streams v:0 -count_frames \
  -show_entries stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames \
  -of default=noprint_wrappers=1 /tmp/out.mp4
```

## Author your own scene

Subclass `Scene`, override `construct`, then point the CLI at it. The full pattern is on the [Examples](examples.md) page.

## Day-to-day commands

After making Rust-side changes, rebuild the extension:

```sh
source .venv/bin/activate && maturin develop
```

Run the test suites:

```sh
cargo test --workspace          # Rust
pytest tests/python             # Python
```
