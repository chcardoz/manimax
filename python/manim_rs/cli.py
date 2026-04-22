"""Manimax CLI — Slice B.

One subcommand, `render`, that builds the hardcoded Slice B scene and hands
it to the Rust runtime. Scene discovery (``--scene path.py``) is Slice C.
"""

from __future__ import annotations

from pathlib import Path

import typer

from manim_rs import _rust, ir
from manim_rs.animate import Translate
from manim_rs.objects import Polyline
from manim_rs.scene import Scene

app = typer.Typer(add_completion=False, help="Manimax renderer.")


@app.callback()
def _root() -> None:
    """Force typer to treat ``render`` as a required subcommand instead of
    collapsing it into the top-level command (the single-command shortcut)."""


def _build_slice_b_scene(duration: float, fps: int) -> Scene:
    scene = Scene(fps=fps)
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=0.08,
    )
    scene.add(square)
    scene.play(Translate(square, (2.0, 0.0, 0.0), duration=duration))
    return scene


@app.command()
def render(
    out: Path = typer.Argument(..., help="Output mp4 path."),
    duration: float = typer.Option(2.0, "--duration", help="Scene duration in seconds."),
    fps: int = typer.Option(30, "--fps", help="Frames per second."),
) -> None:
    """Render the Slice B demo scene to ``out``."""
    if duration <= 0.0:
        raise typer.BadParameter("duration must be positive", param_hint="--duration")
    if fps <= 0:
        raise typer.BadParameter("fps must be positive", param_hint="--fps")

    scene = _build_slice_b_scene(duration=duration, fps=fps)
    ir_json = ir.encode(scene.ir).decode("utf-8")
    _rust.render_to_mp4(ir_json, str(out))
    typer.echo(f"wrote {out}")
