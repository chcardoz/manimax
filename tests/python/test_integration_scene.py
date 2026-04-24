"""Slice C Step 7 — mandated end-to-end integration test.

Renders ``integration_scene.py`` via the CLI, verifies mp4 metadata, pulls
two frames back as raw RGB, and checks:

- Tolerance-based pixel checksum (sum + nonzero count within ±10%).
- Per-object centroid clustering by color band — each of the three objects
  is distinguishable by its dominant color, so we can find its centroid on
  screen and assert it lands in the expected region.

The expected values were captured from a local render (macOS arm64, wgpu 29,
Metal, ffmpeg h264/yuv420p). H.264 quantization and MSAA sample-pattern
drift mean these are deliberately loose — they catch large-scale rendering
regressions (object missing, animation stuck at t=0, wrong order of
composition) without tripping on benign encoder/driver changes.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import numpy as np
import pytest
from manim_rs.cli import app
from typer.testing import CliRunner

SCENE_FILE = Path(__file__).parent / "integration_scene.py"

WIDTH = 480
HEIGHT = 270
FPS = 30
DURATION = 2.0
TOTAL_FRAMES = int(FPS * DURATION)  # 60


def _have_ffmpeg() -> bool:
    return shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None


pytestmark = pytest.mark.skipif(not _have_ffmpeg(), reason="ffmpeg/ffprobe not on PATH")


@pytest.fixture(scope="module")
def rendered_mp4(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Render once; the frame-extraction tests share the output."""
    out = tmp_path_factory.mktemp("integration") / "integration.mp4"
    runner = CliRunner()
    result = runner.invoke(
        app,
        ["render", str(SCENE_FILE), "IntegrationScene", str(out), "--fps", str(FPS)],
    )
    assert result.exit_code == 0, result.output
    assert out.exists()
    return out


def _ffprobe_stream(path: Path) -> dict[str, str]:
    out = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,pix_fmt,duration,nb_read_frames",
            "-of",
            "default=noprint_wrappers=1",
            str(path),
        ],
        text=True,
    )
    return dict(line.split("=", 1) for line in out.strip().splitlines())


def _extract_frame_rgb(mp4: Path, frame_idx: int) -> bytes:
    """Grab a single frame as raw RGB (3 bytes/pixel, no padding)."""
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
            "rgb24",
            "-",
        ],
        capture_output=True,
        check=True,
    )
    assert len(proc.stdout) == WIDTH * HEIGHT * 3, len(proc.stdout)
    return proc.stdout


def _centroid(
    data: bytes,
    r_band: tuple[int, int],
    g_band: tuple[int, int],
    b_band: tuple[int, int],
) -> tuple[float, float, int] | None:
    """Centroid of pixels whose RGB lands inside the given color bands."""
    arr = np.frombuffer(data, dtype=np.uint8).reshape(HEIGHT, WIDTH, 3)
    r, g, b = arr[..., 0], arr[..., 1], arr[..., 2]
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


def _assert_within(actual: float, expected: float, tol: float, label: str) -> None:
    assert (
        abs(actual - expected) <= tol
    ), f"{label}: got {actual:.1f}, expected {expected:.1f} ±{tol}"


def test_ffprobe_metadata(rendered_mp4: Path) -> None:
    meta = _ffprobe_stream(rendered_mp4)
    assert meta["width"] == str(WIDTH)
    assert meta["height"] == str(HEIGHT)
    assert meta["codec_name"] == "h264"
    assert meta["pix_fmt"] == "yuv420p"
    assert meta["avg_frame_rate"] == f"{FPS}/1"
    assert meta["nb_read_frames"] == str(TOTAL_FRAMES)
    # duration string has trailing zeros — parse as float for flexibility.
    assert float(meta["duration"]) == pytest.approx(DURATION, abs=0.05)


# Expected values captured on macOS arm64, Metal, wgpu 29, ffmpeg h264/yuv420p.
# Tolerance is wide enough to absorb encoder drift but tight enough to catch
# large regressions (object missing, stuck frame, composition order flip).
EXPECTED_F30_SUM = 1_292_070
EXPECTED_F30_NONZERO = 11_558
EXPECTED_F55_SUM = 1_185_030
EXPECTED_F55_NONZERO = 11_101
CHECKSUM_TOLERANCE = 0.10  # ±10%

# Centroid expectations — object positions at chosen frames.
# Frame 30 (t=1.0s, mid-scene):
#   Red square:   translated to world (-0.9, 0) → ~pixel (213, 135). Observed
#                 (202, 136) — stroke tessellation pulls centroid slightly.
#   Green teardrop: near origin → ~pixel (240, 135). Observed (241, 130).
#   Blue triangle: ThereAndBack peak at world (+1.8, +0.4) → ~pixel (294, 123).
#                 Observed (291, 118).
EXPECTED_F30_CENTROIDS = {
    "red": (202.0, 136.0),
    "green": (241.0, 130.0),
    "blue": (291.0, 118.0),
}
# Frame 55 (t=1.833s, near end):
#   Red square:   translated further left → ~pixel (190, 135).
#   Green teardrop: still near origin, scaled larger → ~pixel (241, 130).
#   Blue triangle: ThereAndBack returning, near origin → ~pixel (230, 144).
EXPECTED_F55_CENTROIDS = {
    "red": (190.0, 135.0),
    "green": (241.0, 130.0),
    "blue": (230.0, 144.0),
}
CENTROID_TOLERANCE_PX = 25.0

# Color bands — tuned against H.264/yuv420p decoded frames so we tolerate
# the chroma blur that subsampling introduces on solid colors.
RED_BAND = ((140, 255), (0, 80), (0, 80))
GREEN_BAND = ((0, 80), (180, 255), (50, 200))
BLUE_BAND = ((0, 120), (0, 180), (150, 255))


def _check_centroids(
    data: bytes,
    expected: dict[str, tuple[float, float]],
    label: str,
) -> None:
    for color, (exp_x, exp_y), band in (
        ("red", expected["red"], RED_BAND),
        ("green", expected["green"], GREEN_BAND),
        ("blue", expected["blue"], BLUE_BAND),
    ):
        res = _centroid(data, band[0], band[1], band[2])
        assert res is not None, f"{label}: no {color} pixels found"
        cx, cy, n = res
        assert n > 10, f"{label}: {color} pixel count too low ({n})"
        _assert_within(cx, exp_x, CENTROID_TOLERANCE_PX, f"{label} {color} cx")
        _assert_within(cy, exp_y, CENTROID_TOLERANCE_PX, f"{label} {color} cy")


def _sum_and_nonzero(data: bytes) -> tuple[int, int]:
    arr = np.frombuffer(data, dtype=np.uint8)
    return int(arr.sum()), int(np.count_nonzero(arr))


def test_frame_30_pixel_checksum_and_centroids(rendered_mp4: Path) -> None:
    data = _extract_frame_rgb(rendered_mp4, 30)
    total, nonzero = _sum_and_nonzero(data)
    _assert_within(total, EXPECTED_F30_SUM, EXPECTED_F30_SUM * CHECKSUM_TOLERANCE, "f30 sum")
    _assert_within(
        nonzero,
        EXPECTED_F30_NONZERO,
        EXPECTED_F30_NONZERO * CHECKSUM_TOLERANCE,
        "f30 nonzero",
    )
    _check_centroids(data, EXPECTED_F30_CENTROIDS, "f30")


def test_frame_55_pixel_checksum_and_centroids(rendered_mp4: Path) -> None:
    data = _extract_frame_rgb(rendered_mp4, 55)
    total, nonzero = _sum_and_nonzero(data)
    _assert_within(total, EXPECTED_F55_SUM, EXPECTED_F55_SUM * CHECKSUM_TOLERANCE, "f55 sum")
    _assert_within(
        nonzero,
        EXPECTED_F55_NONZERO,
        EXPECTED_F55_NONZERO * CHECKSUM_TOLERANCE,
        "f55 nonzero",
    )
    _check_centroids(data, EXPECTED_F55_CENTROIDS, "f55")
