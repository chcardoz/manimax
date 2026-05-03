"""Slice B Step 8 exit criterion — end-to-end render through the pyo3 binding.

Slice C migrated the FFI from a JSON string to a dict via pythonize;
``ir.to_builtins(scene.ir)`` is the canonical way to prepare the IR payload.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from conftest import (
    canonical_square_scene,
    centroid_in_band,
    extract_frame_raw,
    ffprobe_stream,
    requires_ffmpeg,
    requires_ffprobe,
)
from manim_rs import _rust, ir
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def _build_scene() -> Scene:
    return canonical_square_scene(
        fps=15,
        duration=0.4,
        stroke_width=0.1,
        translate_x=1.0,
        resolution=ir.Resolution(width=128, height=72),
    )


@requires_ffprobe
def test_render_to_mp4_produces_valid_file(tmp_path: Path) -> None:
    scene = _build_scene()
    out = tmp_path / "py_out.mp4"

    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out))

    assert out.exists(), "mp4 was not written"
    assert out.stat().st_size > 0, "mp4 is empty"

    info = ffprobe_stream(out)
    assert info["width"] == "128"
    assert info["height"] == "72"
    assert info["codec_name"] == "h264"
    assert info["avg_frame_rate"] == "15/1"
    # 15 fps × 0.4s = 6 frames.
    assert info["nb_read_frames"] == "6"


@requires_ffprobe
def test_render_to_mp4_fps_override(tmp_path: Path) -> None:
    scene = _build_scene()
    out = tmp_path / "py_out_override.mp4"
    # Override the fps; scene authored at 15, render at 30.
    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out), fps=30)

    assert out.exists()
    info = ffprobe_stream(out, "avg_frame_rate")
    assert info["avg_frame_rate"] == "30/1"


def test_render_to_mp4_rejects_zero_fps(tmp_path: Path) -> None:
    scene = _build_scene()
    with pytest.raises(ValueError, match="fps must be positive"):
        _rust.render_to_mp4(ir.to_builtins(scene.ir), str(tmp_path / "zero.mp4"), fps=0)


def test_render_to_mp4_rejects_bad_ir(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="IR depythonize failed"):
        _rust.render_to_mp4({"not": "a scene"}, str(tmp_path / "bad.mp4"))


@requires_ffmpeg
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

    arr = extract_frame_raw(out, width=width, height=height)
    res = centroid_in_band(arr, threshold_any=True)
    assert res is not None and res[2] > 100, f"too few bright pixels in frame 0: {res}"
    cx, cy, _ = res
    # At SLICE_B_DEFAULT + 480×270, scene (0,0) maps to pixel (240, 135).
    # No Translate in this fixture; the centroid sits on the origin.
    assert abs(cx - 240) <= 3, f"centroid x={cx:.1f} drifted from 240"
    assert abs(cy - 135) <= 3, f"centroid y={cy:.1f} drifted from 135"
