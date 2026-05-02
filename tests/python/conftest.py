"""Shared fixtures and helpers for the Python test suite.

Four call sites used to redefine "the canonical Slice B scene" (a unit square
translated +2 on x over a fixed duration). They drifted independently; this
module gives them one source of truth. Each test picks the duration/fps it
needs via the helper.

Also centralizes the ffprobe / ffmpeg / centroid helpers that the e2e tests
all need — the inline copies drifted in shape (return string vs dict, fields
list, pix_fmt) before being unified here.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path
from typing import Final

import numpy as np
import pytest
from manim_rs import ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene

requires_ffprobe: Final = pytest.mark.skipif(
    shutil.which("ffprobe") is None, reason="ffprobe not on PATH"
)
requires_ffmpeg: Final = pytest.mark.skipif(
    shutil.which("ffmpeg") is None, reason="ffmpeg not on PATH"
)
requires_ffmpeg_and_ffprobe: Final = pytest.mark.skipif(
    shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None,
    reason="ffmpeg/ffprobe not on PATH",
)


_DEFAULT_FFPROBE_FIELDS = "width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames"


def ffprobe_stream(path: Path, fields: str = _DEFAULT_FFPROBE_FIELDS) -> dict[str, str]:
    """Run ffprobe and return a ``{field: value}`` dict for stream v:0."""
    out = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            f"stream={fields}",
            "-of",
            "default=noprint_wrappers=1",
            str(path),
        ],
        text=True,
    )
    return dict(line.split("=", 1) for line in out.strip().splitlines())


def extract_frame_raw(
    mp4: Path,
    frame_idx: int = 0,
    *,
    width: int,
    height: int,
    pix_fmt: str = "rgba",
) -> np.ndarray:
    """Decode a single frame from ``mp4`` and return it as a (H, W, C) uint8 array.

    ``pix_fmt`` is passed through to ffmpeg; the channel count is inferred
    (``rgba`` → 4, ``rgb24`` → 3). Raises ``AssertionError`` if the raw byte
    count does not match the expected frame size.
    """
    channels = {"rgba": 4, "rgb24": 3}[pix_fmt]
    proc = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(mp4),
            "-vf",
            f"select=eq(n\\,{frame_idx})",
            "-vframes",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            pix_fmt,
            "-",
        ],
        capture_output=True,
        check=True,
    )
    expected = width * height * channels
    assert len(proc.stdout) == expected, f"unexpected frame size: {len(proc.stdout)} != {expected}"
    return np.frombuffer(proc.stdout, dtype=np.uint8).reshape(height, width, channels)


def centroid_in_band(
    arr: np.ndarray,
    r_band: tuple[int, int] = (40, 255),
    g_band: tuple[int, int] = (40, 255),
    b_band: tuple[int, int] = (40, 255),
    *,
    threshold_any: bool = False,
) -> tuple[float, float, int] | None:
    """Centroid of pixels whose RGB channels fall inside the given bands.

    ``threshold_any=True`` switches the band check to a logical-OR across
    channels (any channel above the band's lower bound) — matches the
    "bright pixel" detector used by the Slice-B/E pixel tests, where the
    upper bounds are saturated and the goal is "lit anywhere". The default
    AND form is what the integration test uses for color-band picking.
    """
    r, g, b = arr[..., 0], arr[..., 1], arr[..., 2]
    if threshold_any:
        mask = (r > r_band[0]) | (g > g_band[0]) | (b > b_band[0])
    else:
        mask = (
            (r >= r_band[0])
            & (r <= r_band[1])
            & (g >= g_band[0])
            & (g <= g_band[1])
            & (b >= b_band[0])
            & (b <= b_band[1])
        )
    ys, xs = np.nonzero(mask)
    if xs.size == 0:
        return None
    return (float(xs.mean()), float(ys.mean()), int(xs.size))


def canonical_square_scene(
    *,
    fps: int = 30,
    duration: float = 2.0,
    stroke_width: float = 0.08,
    translate_x: float = 2.0,
    resolution: ir.Resolution | None = None,
) -> Scene:
    """Unit square at the origin, translated `translate_x` over `duration`s.

    The canonical fixture for exercising the Python → IR → Rust pipeline with
    one object and one position track.
    """
    scene = Scene(fps=fps, resolution=resolution) if resolution else Scene(fps=fps)
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=stroke_width,
    )
    scene.add(square)
    scene.play(Translate(square, (translate_x, 0.0, 0.0), duration=duration))
    return scene
