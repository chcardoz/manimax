"""Slice B Step 8 exit criterion — end-to-end render through the pyo3 binding.

Slice C migrated the FFI from a JSON string to a dict via pythonize;
``ir.to_builtins(scene.ir)`` is the canonical way to prepare the IR payload.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest
from manim_rs import _rust, ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def _build_scene() -> Scene:
    scene = Scene(
        fps=15,
        resolution=ir.Resolution(width=128, height=72),
    )
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=0.1,
    )
    scene.add(square)
    scene.play(Translate(square, (1.0, 0.0, 0.0), duration=0.4))
    return scene


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_render_to_mp4_produces_valid_file(tmp_path: Path) -> None:
    scene = _build_scene()
    out = tmp_path / "py_out.mp4"

    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out))

    assert out.exists(), "mp4 was not written"
    assert out.stat().st_size > 0, "mp4 is empty"

    probe = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,nb_read_frames",
            "-of",
            "default=noprint_wrappers=1",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    output = probe.stdout
    assert "width=128" in output, output
    assert "height=72" in output, output
    assert "codec_name=h264" in output, output
    assert "avg_frame_rate=15/1" in output, output
    # 15 fps × 0.4s = 6 frames.
    assert "nb_read_frames=6" in output, output


def test_render_to_mp4_fps_override(tmp_path: Path) -> None:
    scene = _build_scene()
    out = tmp_path / "py_out_override.mp4"
    # Override the fps; scene authored at 15, render at 30.
    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out), fps=30)

    assert out.exists()

    if shutil.which("ffprobe") is None:
        pytest.skip("ffprobe not on PATH")

    probe = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=avg_frame_rate",
            "-of",
            "default=noprint_wrappers=1",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "avg_frame_rate=30/1" in probe.stdout, probe.stdout


def test_render_to_mp4_rejects_zero_fps(tmp_path: Path) -> None:
    scene = _build_scene()
    with pytest.raises(ValueError, match="fps must be positive"):
        _rust.render_to_mp4(ir.to_builtins(scene.ir), str(tmp_path / "zero.mp4"), fps=0)


def test_render_to_mp4_rejects_bad_ir(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="IR depythonize failed"):
        _rust.render_to_mp4({"not": "a scene"}, str(tmp_path / "bad.mp4"))


@pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="ffmpeg not on PATH")
def test_render_to_mp4_frame0_has_content_at_origin(tmp_path: Path) -> None:
    """End-to-end pixel check: decode frame 0 and confirm a centered square
    renders as a bright region around the scene origin.

    Catches bugs where the mp4 "looks fine" to ffprobe (right dims, right
    codec, right framerate) but the Python→IR→Rust→wgpu→ffmpeg pipeline
    has silently drifted — MVP sign flip, background-color leak, Translate
    evaluated at the wrong ``t``, etc.

    Uses a fat stroke on a 480×270 canvas (not the 128×72 fixture): yuv420p
    at the small size crushes a 0.1-width stroke to sub-threshold values and
    the centroid becomes noise.
    """
    width, height = 480, 270
    scene = Scene(fps=15, resolution=ir.Resolution(width=width, height=height))
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=0.15,
    )
    scene.add(square)
    scene.wait(0.2)
    out = tmp_path / "centroid.mp4"
    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out))

    raw = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(out),
            "-vframes",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    assert len(raw) == width * height * 4, f"unexpected frame size: {len(raw)}"

    # Centroid of non-background pixels.
    n, sx, sy = 0, 0, 0
    for y in range(height):
        for x in range(width):
            i = (y * width + x) * 4
            if raw[i] > 40 or raw[i + 1] > 40 or raw[i + 2] > 40:
                n += 1
                sx += x
                sy += y
    assert n > 100, f"too few bright pixels in frame 0: {n}"

    cx, cy = sx / n, sy / n
    # At SLICE_B_DEFAULT + 480×270, scene (0,0) maps to pixel (240, 135).
    # No Translate in this fixture; the centroid sits on the origin.
    assert abs(cx - 240) <= 3, f"centroid x={cx:.1f} drifted from 240"
    assert abs(cy - 135) <= 3, f"centroid y={cy:.1f} drifted from 135"
