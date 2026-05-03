"""``frame`` command — single-frame PNG render."""

from __future__ import annotations

from pathlib import Path

import typer

from manim_rs import api
from manim_rs.cli._shared import (
    Quality,
    load_scene_class,
    open_file,
    resolve_dimensions,
)


def frame(
    module_path: str = typer.Argument(
        ...,
        help="Path to a .py file or a dotted importable module containing the scene.",
    ),
    scene_name: str = typer.Argument(..., help="Name of the Scene subclass to render."),
    out: Path = typer.Argument(..., help="Output PNG path."),  # noqa: B008
    t: float = typer.Option(
        0.0, "--t", help="Timestamp in seconds at which to evaluate the scene."
    ),
    quality: Quality = typer.Option(  # noqa: B008
        None,
        "--quality",
        case_sensitive=False,
        help="Resolution preset.",
    ),
    resolution: str = typer.Option(
        None,
        "-r",
        "--resolution",
        help='Resolution override "WxH". Wins over --quality.',
    ),
    fps: int = typer.Option(30, "--fps", help="Frames per second (only affects metadata)."),
    open_after: bool = typer.Option(
        False, "-o", "--open", help="Open the output file once rendering finishes."
    ),
    trace_json: Path = typer.Option(  # noqa: B008
        None,
        "--trace-json",
        help="Write per-stage tracing spans to this JSON file.",
    ),
) -> None:
    """Render a single frame at time ``--t`` from ``scene_name`` to ``out`` as PNG."""
    if t < 0.0:
        raise typer.BadParameter("--t must be non-negative", param_hint="--t")
    if fps <= 0:
        raise typer.BadParameter("fps must be positive", param_hint="--fps")

    scene_cls = load_scene_class(module_path, scene_name)

    api.render_frame(
        scene_cls,
        out,
        t,
        fps=fps,
        resolution=resolve_dimensions(quality, resolution),
        trace_json=trace_json,
    )
    typer.echo(f"wrote {out}")

    if open_after:
        open_file(out)
