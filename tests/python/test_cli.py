"""Slice C Step 6 CLI smoke tests.

Rendering now goes through scene discovery: the CLI takes a module path and
a scene class name rather than running a hardcoded Slice B fixture. We use
a ``tmp_path`` scene file per test so the input surface is exercised, not
shimmed.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest
from manim_rs.cli import app
from typer.testing import CliRunner

SCENE_SOURCE = textwrap.dedent(
    """
    from manim_rs import Scene, Polyline, Translate


    class DemoScene(Scene):
        def construct(self) -> None:
            square = Polyline(
                [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
                stroke_width=0.08,
            )
            self.add(square)
            self.play(Translate(square, (2.0, 0.0, 0.0), duration=0.4))
    """
)


@pytest.fixture
def scene_file(tmp_path: Path) -> Path:
    p = tmp_path / "demo_scene.py"
    p.write_text(SCENE_SOURCE)
    return p


def _ffprobe(path: Path) -> str:
    probe = subprocess.run(
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
        capture_output=True,
        text=True,
        check=True,
    )
    return probe.stdout


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_cli_render_produces_valid_mp4(scene_file: Path, tmp_path: Path) -> None:
    out = tmp_path / "cli.mp4"
    runner = CliRunner()
    result = runner.invoke(
        app,
        ["render", str(scene_file), "DemoScene", str(out), "--fps", "15"],
    )

    assert result.exit_code == 0, result.output
    assert out.exists()

    probe = _ffprobe(out)
    assert "width=480" in probe, probe
    assert "height=270" in probe, probe
    assert "codec_name=h264" in probe, probe
    assert "pix_fmt=yuv420p" in probe, probe
    assert "avg_frame_rate=15/1" in probe, probe
    # Scene plays a 0.4s animation; 15fps × 0.4s = 6 frames.
    assert "nb_read_frames=6" in probe, probe


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_cli_duration_override_shortens_output(scene_file: Path, tmp_path: Path) -> None:
    out = tmp_path / "shortened.mp4"
    runner = CliRunner()
    result = runner.invoke(
        app,
        [
            "render",
            str(scene_file),
            "DemoScene",
            str(out),
            "--duration",
            "0.2",
            "--fps",
            "15",
        ],
    )
    assert result.exit_code == 0, result.output
    probe = _ffprobe(out)
    # --duration overrides the scene's natural 0.4s → 3 frames at 15fps.
    assert "nb_read_frames=3" in probe, probe


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_python_dash_m_invocation(scene_file: Path, tmp_path: Path) -> None:
    out = tmp_path / "dashm.mp4"
    subprocess.run(
        [
            sys.executable,
            "-m",
            "manim_rs",
            "render",
            str(scene_file),
            "DemoScene",
            str(out),
            "--fps",
            "15",
        ],
        check=True,
        capture_output=True,
    )
    assert out.exists()
    probe = _ffprobe(out)
    assert "nb_read_frames=6" in probe, probe


def test_cli_rejects_nonpositive_duration(scene_file: Path, tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(
        app,
        [
            "render",
            str(scene_file),
            "DemoScene",
            str(tmp_path / "x.mp4"),
            "--duration",
            "0",
        ],
    )
    assert result.exit_code != 0
    assert "duration must be positive" in result.output


def test_cli_rejects_nonpositive_fps(scene_file: Path, tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(
        app,
        [
            "render",
            str(scene_file),
            "DemoScene",
            str(tmp_path / "x.mp4"),
            "--fps",
            "0",
        ],
    )
    assert result.exit_code != 0
    assert "fps must be positive" in result.output


def test_cli_rejects_missing_scene(scene_file: Path, tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(
        app,
        ["render", str(scene_file), "NotThere", str(tmp_path / "x.mp4")],
    )
    assert result.exit_code != 0
    assert "NotThere" in result.output


def test_cli_rejects_missing_module(tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(
        app,
        ["render", str(tmp_path / "no_such_file.py"), "X", str(tmp_path / "x.mp4")],
    )
    assert result.exit_code != 0
    assert "no such file" in result.output.lower()
