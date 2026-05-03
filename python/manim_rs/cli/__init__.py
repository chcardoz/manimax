"""Manimax CLI — router only.

Two commands, declared one place each:

    python -m manim_rs render <module_path> <SceneName> <out.mp4> [...]
    python -m manim_rs frame  <module_path> <SceneName> <out.png> --t S [...]

The bodies live in ``render.py`` and ``frame.py``. Their shared helpers
(typer enums, ``WxH`` parsing, progress UX, ``--open``) live in ``_shared.py``.
The actual rendering logic — instantiate scene, build IR, dispatch to Rust —
lives in ``manim_rs.api``, which the CLI is a thin adapter over.
"""

import typer

from manim_rs.cli.frame import frame
from manim_rs.cli.render import render

app = typer.Typer(add_completion=False, help="Manimax renderer.")


@app.callback()
def _root() -> None:
    """Force typer to treat ``render`` as a required subcommand instead of
    collapsing it into the top-level command (the single-command shortcut)."""


app.command()(render)
app.command()(frame)
