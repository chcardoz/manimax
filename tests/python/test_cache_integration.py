"""End-to-end cache integration. Renders a tapered-stroke polyline animated
by a position track through the full Python → IR → Rust eval → wgpu raster
→ ffmpeg mp4 pipeline, routing every frame through the snapshot cache.

Assertions:

1. Cold run produces a valid mp4 (ffprobe metadata check) and populates
   the cache with one file per unique frame state.
2. Warm rerun reports ``hits == total_frames`` and writes no new files to
   the cache directory (filename set + mtimes unchanged).
3. Local edit — rerender with the position track's delta changed. Frames
   strictly before the track starts still hit; frames inside the animated
   window miss. Re-asserted via filesystem side effects (new files appear
   only for the genuinely-new states).
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import msgspec
import pytest
from manim_rs import _rust, ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def _have_ffmpeg() -> bool:
    return shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None


pytestmark = pytest.mark.skipif(not _have_ffmpeg(), reason="ffmpeg/ffprobe not on PATH")


FPS = 15
DURATION = 0.6  # 9 frames
TOTAL_FRAMES = int(FPS * DURATION)
WIDTH = 128
HEIGHT = 72

# Position track only covers the back half — frames at t < 0.3 evaluate
# to the object's static state and should share a single cache entry.
ANIM_START = 0.3
ANIM_LEN = DURATION - ANIM_START


def _build_scene_ir(translate_x: float) -> ir.Scene:
    scene = Scene(
        fps=FPS,
        resolution=ir.Resolution(width=WIDTH, height=HEIGHT),
        background=(0.0, 0.0, 0.0, 1.0),
    )
    stroke = Polyline(
        points=[(-1.5, 0.0, 0.0), (-0.5, 0.0, 0.0), (0.5, 0.0, 0.0), (1.5, 0.0, 0.0)],
        stroke_color=(1.0, 1.0, 1.0, 1.0),
        stroke_width=(0.02, 0.2, 0.2, 0.02),  # tapered
        joint="auto",
        closed=False,
    )
    scene.add(stroke)
    scene.wait(ANIM_START)
    scene.play(Translate(stroke, (translate_x, 0.0, 0.0), duration=ANIM_LEN))
    return msgspec.structs.replace(
        scene.ir,
        metadata=msgspec.structs.replace(scene.ir.metadata, duration=DURATION),
    )


def _ffprobe(path: Path) -> dict[str, str]:
    out = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames",
            "-of",
            "default=noprint_wrappers=1",
            str(path),
        ],
        text=True,
    )
    return dict(line.split("=", 1) for line in out.strip().splitlines())


def _snapshot(dir_: Path) -> dict[str, float]:
    """Map of filename → mtime for every cache entry."""
    return {p.name: p.stat().st_mtime_ns for p in dir_.iterdir()}


def test_cache_integration(tmp_path: Path) -> None:
    cache_dir = tmp_path / "cache"
    cache_dir.mkdir()
    out = tmp_path / "out.mp4"

    scene_ir = _build_scene_ir(translate_x=1.0)
    payload = ir.to_builtins(scene_ir)

    # 1. Cold run — mp4 is valid, cache populates. The cache is
    # content-addressed, so frames that share a `SceneState` (e.g. the
    # static prefix before the animation starts) collapse into one entry:
    # the first such frame misses and writes, subsequent ones hit that
    # just-written entry within the same render. So misses == unique
    # frame states, hits == total_frames - misses.
    cold = _rust.render_to_mp4(payload, str(out), cache_dir=str(cache_dir))
    assert cold["hits"] + cold["misses"] == TOTAL_FRAMES
    assert cold["misses"] >= 1
    assert cold["write_errors"] == 0
    assert out.exists()

    meta = _ffprobe(out)
    assert meta["width"] == str(WIDTH)
    assert meta["height"] == str(HEIGHT)
    assert meta["codec_name"] == "h264"
    assert meta["pix_fmt"] == "yuv420p"
    assert meta["avg_frame_rate"] == f"{FPS}/1"
    assert meta["nb_read_frames"] == str(TOTAL_FRAMES)

    # Unique frame states ≤ total frames (pre-segment frames collapse into
    # one cache entry); the exact count depends on how many frames fall
    # outside the animated window, so we just assert the bounds.
    cold_files = _snapshot(cache_dir)
    assert (
        len(cold_files) == cold["misses"]
    ), "one cache file per miss (each miss writes exactly one entry)"

    # 2. Warm rerun — all hits, no new files, no rewrites.
    warm = _rust.render_to_mp4(payload, str(out), cache_dir=str(cache_dir))
    assert warm["hits"] == TOTAL_FRAMES, warm
    assert warm["misses"] == 0
    warm_files = _snapshot(cache_dir)
    assert warm_files == cold_files, "warm rerun must not touch cache files"

    # 3. Local edit — change the translate's delta. Frames during the
    # static prefix (t < ANIM_START) still hit; frames inside the
    # animated window evaluate to new positions and miss.
    edited_ir = _build_scene_ir(translate_x=3.0)  # chosen to avoid state collisions
    edited_payload = ir.to_builtins(edited_ir)
    edit = _rust.render_to_mp4(edited_payload, str(out), cache_dir=str(cache_dir))
    assert edit["hits"] > 0, "pre-segment frames should still hit"
    assert edit["misses"] > 0, "animated frames should miss"
    assert edit["hits"] + edit["misses"] == TOTAL_FRAMES

    edit_files = _snapshot(cache_dir)
    # All previous entries preserved; new misses appended.
    assert set(cold_files).issubset(set(edit_files))
    new_entries = len(edit_files) - len(cold_files)
    assert (
        new_entries == edit["misses"]
    ), f"expected {edit['misses']} new cache files, got {new_entries}"
