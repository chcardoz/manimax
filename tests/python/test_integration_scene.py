"""End-to-end integration test.

Renders ``integration_scene.py`` via the CLI, verifies mp4 metadata, pulls
three frames back as raw RGB, and checks:

- Tolerance-based pixel checksum (sum + nonzero count within ±10%).
- Per-object centroid clustering by color band — each of the four objects
  is distinguishable by its dominant color, so we can find its centroid on
  screen and assert it lands in the expected region.
- Tail-frame existence: at t≈2.8s, blue and yellow have been faded out and
  removed, while red and green persist. Catches a regression in
  ``FadeOut`` / ``RemoveOp`` handling.

The expected values were captured from a local render (macOS arm64, wgpu 29,
Metal, ffmpeg h264/yuv420p). H.264 quantization and MSAA sample-pattern
drift mean these are deliberately loose — they catch large-scale rendering
regressions (object missing, animation stuck at t=0, wrong order of
composition, FadeOut not honored) without tripping on benign encoder/driver
changes.
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
DURATION = 3.0
TOTAL_FRAMES = int(FPS * DURATION)  # 90


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
EXPECTED_F15_SUM = 668_478
EXPECTED_F15_NONZERO = 9_283
EXPECTED_F45_SUM = 1_214_731
EXPECTED_F45_NONZERO = 13_940
CHECKSUM_TOLERANCE = 0.10  # ±10%

# Centroid expectations — object positions at chosen frames.
# Frame 15 (t=0.5s, mid-arrival, FadeIn ~half complete):
#   Red square:   translating left toward (-0.9, 0) → ~pixel (204, 136).
#   Green teardrop: home position → ~pixel (241, 130).
#   Blue triangle: translating right toward (+0.9, 0) → ~pixel (268, 136).
#   Yellow Tex (\pi): translating up toward (0, 0.475) → ~pixel (254, 98).
EXPECTED_F15_CENTROIDS = {
    "red": (204.0, 136.0),
    "green": (241.0, 130.0),
    "blue": (268.0, 136.0),
    "yellow": (254.0, 98.0),
}
# Frame 45 (t=1.5s, mid-flourish):
#   Red square:   fully translated to (-1.8, 0) and partially rotated → (186, 135).
#   Green teardrop: home + scale 1.3 + colorize partway → (241, 130).
#   Blue triangle: peak of ThereAndBack at (1.8, 0.6) → (293, 123).
#   Yellow Tex:   at (0, 0.95), under Wiggle rotation → (249, 90).
EXPECTED_F45_CENTROIDS = {
    "red": (186.0, 135.0),
    "green": (241.0, 130.0),
    "blue": (293.0, 123.0),
    "yellow": (249.0, 90.0),
}
CENTROID_TOLERANCE_PX = 25.0

# Color bands — tuned against H.264/yuv420p decoded frames so we tolerate
# the chroma blur that subsampling introduces on solid colors. The green
# band is intentionally wider than red/blue/yellow because the teardrop
# undergoes Colorize during phase 2 and ends up slightly cyan-ward.
RED_BAND = ((140, 255), (0, 80), (0, 80))
GREEN_BAND = ((0, 170), (150, 255), (30, 220))
BLUE_BAND = ((0, 120), (0, 180), (150, 255))
YELLOW_BAND = ((180, 255), (160, 255), (0, 100))


def _check_centroids(
    data: bytes,
    expected: dict[str, tuple[float, float]],
    label: str,
) -> None:
    for color, (exp_x, exp_y), band in (
        ("red", expected["red"], RED_BAND),
        ("green", expected["green"], GREEN_BAND),
        ("blue", expected["blue"], BLUE_BAND),
        ("yellow", expected["yellow"], YELLOW_BAND),
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


def test_frame_15_pixel_checksum_and_centroids(rendered_mp4: Path) -> None:
    data = _extract_frame_rgb(rendered_mp4, 15)
    total, nonzero = _sum_and_nonzero(data)
    _assert_within(total, EXPECTED_F15_SUM, EXPECTED_F15_SUM * CHECKSUM_TOLERANCE, "f15 sum")
    _assert_within(
        nonzero,
        EXPECTED_F15_NONZERO,
        EXPECTED_F15_NONZERO * CHECKSUM_TOLERANCE,
        "f15 nonzero",
    )
    _check_centroids(data, EXPECTED_F15_CENTROIDS, "f15")


def test_frame_45_pixel_checksum_and_centroids(rendered_mp4: Path) -> None:
    data = _extract_frame_rgb(rendered_mp4, 45)
    total, nonzero = _sum_and_nonzero(data)
    _assert_within(total, EXPECTED_F45_SUM, EXPECTED_F45_SUM * CHECKSUM_TOLERANCE, "f45 sum")
    _assert_within(
        nonzero,
        EXPECTED_F45_NONZERO,
        EXPECTED_F45_NONZERO * CHECKSUM_TOLERANCE,
        "f45 nonzero",
    )
    _check_centroids(data, EXPECTED_F45_CENTROIDS, "f45")


# Tail frame: t≈2.8s, after FadeOut+remove on blue and yellow. Asserts
# *existence* (or absence) per color band rather than centroid coords —
# the goal is to catch a regression where FadeOut or RemoveOp stop
# wiping objects from the active set.
F84_PRESENT_PIXEL_THRESHOLD = 100  # red/green should have plenty
F84_ABSENT_PIXEL_THRESHOLD = 10  # blue/yellow should be near-zero


def test_frame_84_post_remove_active_set(rendered_mp4: Path) -> None:
    data = _extract_frame_rgb(rendered_mp4, 84)

    red = _centroid(data, *RED_BAND)
    assert (
        red is not None and red[2] >= F84_PRESENT_PIXEL_THRESHOLD
    ), f"f84: red expected present (n>={F84_PRESENT_PIXEL_THRESHOLD}), got {red}"

    green = _centroid(data, *GREEN_BAND)
    assert (
        green is not None and green[2] >= F84_PRESENT_PIXEL_THRESHOLD
    ), f"f84: green expected present (n>={F84_PRESENT_PIXEL_THRESHOLD}), got {green}"

    blue = _centroid(data, *BLUE_BAND)
    blue_n = blue[2] if blue is not None else 0
    assert (
        blue_n < F84_ABSENT_PIXEL_THRESHOLD
    ), f"f84: blue expected gone (n<{F84_ABSENT_PIXEL_THRESHOLD}), got n={blue_n}"

    yellow = _centroid(data, *YELLOW_BAND)
    yellow_n = yellow[2] if yellow is not None else 0
    assert (
        yellow_n < F84_ABSENT_PIXEL_THRESHOLD
    ), f"f84: yellow expected gone (n<{F84_ABSENT_PIXEL_THRESHOLD}), got n={yellow_n}"
