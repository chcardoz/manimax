"""Manimax CLI.

    python -m manim_rs render <module_path> <SceneName> <out.mp4> \
        [--quality low|med|high|uhd] [-r WxH] [--fps N] [--duration S] [-o]

    python -m manim_rs frame <module_path> <SceneName> <out.png> \
        --t <SECONDS> [--quality low|med|high|uhd] [-r WxH] [--fps N] [-o]

``<module_path>`` is either a path to a ``.py`` file or a dotted importable
module. ``<SceneName>`` must be a concrete subclass of ``manim_rs.Scene``
declared (or imported) in that module. The CLI instantiates the scene,
runs ``construct()``, and hands the IR to the Rust runtime.

``frame`` runs eval+raster for one timestamp `--t` and writes a PNG; it
skips the ffmpeg encoder entirely. Useful for visual inspection and
snapshot baselines.

Resolution comes from ``--quality`` (preset) or ``-r/--resolution`` (explicit
``WxH``). Explicit wins. When neither is given, the scene's own default is
used (480×270 for the base ``Scene``). Quality presets match manimgl's
ladder: ``low`` 854×480, ``med`` 1280×720, ``high`` 1920×1080, ``uhd`` 3840×2160.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from enum import Enum
from pathlib import Path

import msgspec.structs
import typer

from manim_rs import _rust, ir
from manim_rs.discovery import DiscoveryError, load_scene


class Quality(str, Enum):
    low = "low"
    med = "med"
    high = "high"
    uhd = "uhd"


_QUALITY_RESOLUTIONS: dict[Quality, tuple[int, int]] = {
    Quality.low: (854, 480),
    Quality.med: (1280, 720),
    Quality.high: (1920, 1080),
    Quality.uhd: (3840, 2160),
}


app = typer.Typer(add_completion=False, help="Manimax renderer.")


@app.callback()
def _root() -> None:
    """Force typer to treat ``render`` as a required subcommand instead of
    collapsing it into the top-level command (the single-command shortcut)."""


def _parse_resolution(s: str) -> tuple[int, int]:
    try:
        w_str, h_str = s.lower().split("x", 1)
        w, h = int(w_str), int(h_str)
    except ValueError as err:
        raise typer.BadParameter(
            f"resolution must be WxH (e.g. 1920x1080), got {s!r}",
            param_hint="--resolution",
        ) from err
    if w <= 0 or h <= 0:
        raise typer.BadParameter(
            f"resolution dimensions must be positive, got {w}x{h}",
            param_hint="--resolution",
        )
    return w, h


def _resolve_dimensions(
    quality: Quality | None, resolution: str | None
) -> tuple[int, int] | tuple[None, None]:
    if resolution is not None:
        return _parse_resolution(resolution)
    if quality is not None:
        return _QUALITY_RESOLUTIONS[quality]
    return (None, None)


def _build_scene_ir(
    module_path: str,
    scene_name: str,
    fps: int,
    w: int | None,
    h: int | None,
):
    try:
        scene_cls = load_scene(module_path, scene_name)
    except DiscoveryError as err:
        raise typer.BadParameter(str(err), param_hint="module_path/scene_name") from err

    scene_kwargs: dict = {"fps": fps}
    if w is not None:
        scene_kwargs["resolution"] = ir.Resolution(width=w, height=h)

    scene = scene_cls(**scene_kwargs)
    scene.construct()
    return scene.ir


def _make_progress_callback():
    """Build a `(frame_idx, total)` callback that overwrites a single stderr
    line via `\\r`. Frame indices arrive 0-based; we display 1-based for
    end users. The renderer guarantees `total > 0` only when there are
    frames to render, so we guard against ZeroDivisionError defensively."""
    last_pct = -1

    def _cb(idx: int, total: int) -> None:
        nonlocal last_pct
        if total <= 0:
            return
        # Throttle redraws to once per percent so the terminal doesn't drown
        # in escape sequences on a 4K 120 fps long render.
        pct = ((idx + 1) * 100) // total
        if pct == last_pct and idx + 1 != total:
            return
        last_pct = pct
        sys.stderr.write(f"\rframe {idx + 1}/{total} ({pct:3d}%)")
        sys.stderr.flush()

    return _cb


def _open_file(path: Path) -> None:
    if sys.platform == "darwin":
        cmd = ["open", str(path)]
    elif sys.platform.startswith("linux"):
        cmd = ["xdg-open", str(path)]
    elif sys.platform == "win32":
        cmd = ["cmd", "/c", "start", "", str(path)]
    else:
        typer.echo(f"(--open not supported on {sys.platform})")
        return
    if shutil.which(cmd[0]) is None and sys.platform != "win32":
        typer.echo(f"(--open: {cmd[0]} not on PATH)")
        return
    subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


@app.command()
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
            "Unset uses ffmpeg's default."
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

    if trace_json is not None:
        _rust.install_trace_json(str(trace_json))

    w, h = _resolve_dimensions(quality, resolution)
    scene_ir = _build_scene_ir(module_path, scene_name, fps, w, h)

    if duration is not None:
        scene_ir = msgspec.structs.replace(
            scene_ir,
            metadata=msgspec.structs.replace(scene_ir.metadata, duration=duration),
        )

    progress_cb = _make_progress_callback() if progress else None
    _rust.render_to_mp4(
        ir.to_builtins(scene_ir),
        str(out),
        crf=crf,
        progress=progress_cb,
    )
    if progress_cb is not None:
        sys.stderr.write("\n")
        sys.stderr.flush()
    typer.echo(f"wrote {out}")

    if open_after:
        _open_file(out)


@app.command()
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

    if trace_json is not None:
        _rust.install_trace_json(str(trace_json))

    w, h = _resolve_dimensions(quality, resolution)
    scene_ir = _build_scene_ir(module_path, scene_name, fps, w, h)

    _rust.render_frame(ir.to_builtins(scene_ir), str(out), float(t))
    typer.echo(f"wrote {out}")

    if open_after:
        _open_file(out)
