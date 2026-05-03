"""``render`` command — full mp4 render."""

from __future__ import annotations

import sys
from pathlib import Path

import typer

from manim_rs import api
from manim_rs.cli._shared import (
    EncoderBackend,
    Quality,
    load_scene_class,
    make_progress_callback,
    open_file,
    resolve_dimensions,
)


def render(
    module_path: str = typer.Argument(
        ...,
        help="Path to a .py file or a dotted importable module containing the scene.",
    ),
    scene_name: str = typer.Argument(..., help="Name of the Scene subclass to render."),
    out: Path = typer.Argument(..., help="Output mp4 path."),  # noqa: B008
    quality: Quality = typer.Option(  # noqa: B008
        None,
        "--quality",
        case_sensitive=False,
        help="Resolution preset: low=854x480, med=1280x720, high=1920x1080, uhd=3840x2160.",
    ),
    resolution: str = typer.Option(
        None,
        "-r",
        "--resolution",
        help='Resolution override "WxH" (e.g. 1920x1080). Wins over --quality.',
    ),
    duration: float = typer.Option(
        None,
        "--duration",
        help="Override scene duration (seconds). Defaults to the scene's natural length.",
    ),
    fps: int = typer.Option(30, "--fps", help="Frames per second."),
    open_after: bool = typer.Option(
        False, "-o", "--open", help="Open the output file once rendering finishes."
    ),
    progress: bool = typer.Option(
        True,
        "--progress/--no-progress",
        help="Print a per-frame progress line to stderr (default on).",
    ),
    crf: int = typer.Option(
        None,
        "--crf",
        min=0,
        max=51,
        help=(
            "libx264 Constant Rate Factor (0-51, lower = higher quality). "
            "Recommended: 18 (visually lossless), 23 (default), 28 (preview). "
            "Unset uses ffmpeg's default. Ignored on --encoder hardware."
        ),
    ),
    encoder: EncoderBackend = typer.Option(  # noqa: B008
        EncoderBackend.software,
        "--encoder",
        case_sensitive=False,
        help=(
            "h264 encoder backend. 'software' = libx264 (default, portable). "
            "'hardware' = platform GPU encoder (h264_videotoolbox on macOS). "
            "Hardware is much faster at 4K but produces a different bit stream."
        ),
    ),
    trace_json: Path = typer.Option(  # noqa: B008
        None,
        "--trace-json",
        help=(
            "Write per-stage tracing spans (eval/raster/readback/encoder) to this "
            "JSON file. Honors RUST_LOG (default: info)."
        ),
    ),
) -> None:
    """Render ``scene_name`` from ``module_path`` to ``out``."""
    if duration is not None and duration <= 0.0:
        raise typer.BadParameter("duration must be positive", param_hint="--duration")
    if fps <= 0:
        raise typer.BadParameter("fps must be positive", param_hint="--fps")

    scene_cls = load_scene_class(module_path, scene_name)
    progress_cb = make_progress_callback() if progress else None

    api.render_scene(
        scene_cls,
        out,
        fps=fps,
        resolution=resolve_dimensions(quality, resolution),
        duration=duration,
        crf=crf,
        encoder=encoder.value,
        progress=progress_cb,
        trace_json=trace_json,
    )

    if progress_cb is not None:
        sys.stderr.write("\n")
        sys.stderr.flush()
    typer.echo(f"wrote {out}")

    if open_after:
        open_file(out)
