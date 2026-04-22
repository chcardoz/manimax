"""Slice B Step 9 smoke test — run the CLI end-to-end and ffprobe the output.

Two paths:
- Typer's ``CliRunner``: fast, no subprocess, exercises argument parsing and
  the render call-site.
- ``python -m manim_rs``: proves the ``__main__`` wiring works the way the
  slice plan's §1 user-facing command does.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from typer.testing import CliRunner

from manim_rs.cli import app


def _ffprobe(path: Path) -> str:
    probe = subprocess.run(
        [
            "ffprobe",
            "-v", "error",
            "-select_streams", "v:0",
            "-count_frames",
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,pix_fmt,nb_read_frames",
            "-of", "default=noprint_wrappers=1",
            str(path),
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return probe.stdout


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_cli_render_produces_valid_mp4(tmp_path: Path) -> None:
    out = tmp_path / "cli.mp4"
    runner = CliRunner()
    result = runner.invoke(app, ["render", str(out), "--duration", "0.4", "--fps", "15"])

    assert result.exit_code == 0, result.output
    assert out.exists()

    probe = _ffprobe(out)
    assert "width=480" in probe, probe
    assert "height=270" in probe, probe
    assert "codec_name=h264" in probe, probe
    assert "pix_fmt=yuv420p" in probe, probe
    assert "avg_frame_rate=15/1" in probe, probe
    # 15fps × 0.4s = 6 frames.
    assert "nb_read_frames=6" in probe, probe


@pytest.mark.skipif(shutil.which("ffprobe") is None, reason="ffprobe not on PATH")
def test_python_dash_m_invocation(tmp_path: Path) -> None:
    out = tmp_path / "dashm.mp4"
    subprocess.run(
        [sys.executable, "-m", "manim_rs", "render", str(out), "--duration", "0.2", "--fps", "15"],
        check=True,
        capture_output=True,
    )
    assert out.exists()
    probe = _ffprobe(out)
    # 15fps × 0.2s = 3 frames.
    assert "nb_read_frames=3" in probe, probe


def test_cli_rejects_nonpositive_duration(tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(app, ["render", str(tmp_path / "x.mp4"), "--duration", "0"])
    assert result.exit_code != 0
    assert "duration must be positive" in result.output


def test_cli_rejects_nonpositive_fps(tmp_path: Path) -> None:
    runner = CliRunner()
    result = runner.invoke(app, ["render", str(tmp_path / "x.mp4"), "--fps", "0"])
    assert result.exit_code != 0
    assert "fps must be positive" in result.output
