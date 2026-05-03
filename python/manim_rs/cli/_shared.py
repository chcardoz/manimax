"""CLI-side helpers shared across commands.

Holds typer-specific concerns (enums, argv parsing, `BadParameter` raises) and
terminal UX (progress callback, file open). Anything specific to a single
command lives in that command's own module.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from collections.abc import Callable
from enum import Enum
from pathlib import Path

import typer

from manim_rs.discovery import DiscoveryError, load_scene
from manim_rs.scene import Scene


class Quality(str, Enum):
    low = "low"
    med = "med"
    high = "high"
    uhd = "uhd"


class EncoderBackend(str, Enum):
    software = "software"
    hardware = "hardware"


_QUALITY_RESOLUTIONS: dict[Quality, tuple[int, int]] = {
    Quality.low: (854, 480),
    Quality.med: (1280, 720),
    Quality.high: (1920, 1080),
    Quality.uhd: (3840, 2160),
}


def parse_resolution(s: str) -> tuple[int, int]:
    """Parse a ``WxH`` string into a positive ``(width, height)`` tuple."""
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


def resolve_dimensions(quality: Quality | None, resolution: str | None) -> tuple[int, int] | None:
    """Pick ``(w, h)`` from ``-r`` (wins) or ``--quality``, or ``None`` for the scene default."""
    if resolution is not None:
        return parse_resolution(resolution)
    if quality is not None:
        return _QUALITY_RESOLUTIONS[quality]
    return None


def load_scene_class(module_path: str, scene_name: str) -> type[Scene]:
    """Wrap ``discovery.load_scene`` and convert failures to ``BadParameter``."""
    try:
        return load_scene(module_path, scene_name)
    except DiscoveryError as err:
        raise typer.BadParameter(str(err), param_hint="module_path/scene_name") from err


def make_progress_callback() -> Callable[[int, int], None]:
    """Build a ``\\r``-overwriting per-frame stderr line, throttled to once per percent.

    Frame indices arrive 0-based; we display 1-based. The throttle keeps a 4K
    120 fps long render from drowning the terminal in escape sequences.
    """
    last_pct = -1

    def _cb(idx: int, total: int) -> None:
        nonlocal last_pct
        if total <= 0:
            return
        pct = ((idx + 1) * 100) // total
        if pct == last_pct and idx + 1 != total:
            return
        last_pct = pct
        sys.stderr.write(f"\rframe {idx + 1}/{total} ({pct:3d}%)")
        sys.stderr.flush()

    return _cb


def open_file(path: Path) -> None:
    """Open ``path`` with the platform default app, best-effort."""
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
