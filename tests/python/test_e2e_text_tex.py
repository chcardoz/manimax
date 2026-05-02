"""Slice E Step 8 — combined Tex + Text integration scene + determinism.

Three things proven here:

1. The two Slice E §1 acceptance commands (`examples.text_scene`,
   `examples.tex_scene`) render to mp4s with the expected duration / fps /
   dimensions / codec / pix_fmt via ffprobe.
2. A scene that holds both Tex and Text active simultaneously, with a
   non-glyph mobject animated on a separate transform track, renders end
   to end. Proves Tex and Text fan-outs coexist in one Evaluator without
   stomping each other.
3. **Determinism.** Running the same render twice produces byte-identical
   mp4s. Catches nondeterminism in eval (HashMap iteration order),
   cosmic-text shaping, raster dispatch, or x264 encoding threads.

Cache-hit verification for `compile_tex` / `compile_text` lives in the Rust
integration tests on `manim-rs-eval` — pyo3 surface stays untouched.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

import pytest
from conftest import ffprobe_stream, requires_ffprobe
from manim_rs import (
    FadeIn,
    Polyline,
    Scene,
    Tex,
    Text,
    Translate,
    _rust,
    ir,
)
from manim_rs.discovery import load_scene

EXAMPLES_DIR = Path(__file__).resolve().parents[2] / "examples"


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _build_text_scene() -> Scene:
    """Load the checked-in text example without relying on repo-root sys.path."""
    scene_cls = load_scene(EXAMPLES_DIR / "text_scene.py", "TextScene")
    scene = scene_cls(fps=30)
    scene.construct()
    return scene


def _build_tex_scene() -> Scene:
    scene_cls = load_scene(EXAMPLES_DIR / "tex_scene.py", "TexScene")
    scene = scene_cls(fps=30)
    scene.construct()
    return scene


def _build_combined_scene() -> Scene:
    """Tex + Text + Polyline simultaneously, each with non-default colors,
    plus an opacity animation on the Tex and a transform animation on the
    Polyline. Mirrors the slice plan's combined-scene shape (§ Step 8)."""
    scene = Scene(fps=30, resolution=ir.Resolution(width=480, height=270))

    formula = Tex(
        r"e^{i\pi} + 1 = 0",
        color=(1.0, 0.85, 0.4, 1.0),
    )
    label = Text(
        "Euler's identity",
        size=0.4,
        color=(0.4, 0.85, 1.0, 1.0),
        align="left",
    )
    underline = Polyline(
        [(-1.0, -0.6, 0.0), (1.0, -0.6, 0.0)],
        stroke_width=0.04,
    )

    scene.add(formula)
    scene.add(label)
    scene.add(underline)
    scene.play(FadeIn(formula, duration=0.5))
    scene.play(Translate(underline, (0.0, -0.2, 0.0), duration=1.0))
    scene.wait(0.5)
    return scene


# Module-scoped fixtures: each scene is built and serialized once, then shared
# by the acceptance test (one render) and the determinism test (two renders).
# Saves three full scene compiles + module loads per run.


@pytest.fixture(scope="module")
def text_scene_payload() -> dict:
    return ir.to_builtins(_build_text_scene().ir)


@pytest.fixture(scope="module")
def tex_scene_payload() -> dict:
    return ir.to_builtins(_build_tex_scene().ir)


@pytest.fixture(scope="module")
def combined_scene_payload() -> dict:
    return ir.to_builtins(_build_combined_scene().ir)


@requires_ffprobe
def test_text_scene_acceptance_command(tmp_path: Path, text_scene_payload: dict) -> None:
    """Slice E §1 acceptance command: text scene mp4 reports expected
    dimensions / fps / codec / pix_fmt / frame count via ffprobe."""
    out = tmp_path / "text_scene.mp4"
    _rust.render_to_mp4(text_scene_payload, str(out))

    info = ffprobe_stream(out)
    assert info["width"] == "480"
    assert info["height"] == "270"
    assert info["codec_name"] == "h264"
    assert info["pix_fmt"] == "yuv420p"
    assert info["avg_frame_rate"] == "30/1"
    # TextScene duration: 0.5 + 1.5 + 1.0 = 3.0s @ 30fps = 90 frames.
    assert info["nb_read_frames"] == "90"


@requires_ffprobe
def test_tex_scene_acceptance_command(tmp_path: Path, tex_scene_payload: dict) -> None:
    """Slice E §1 acceptance command: tex scene mp4 reports expected metadata.

    TexScene duration: 0.5 + 2.0 + 1.5 = 4.0s @ 30fps = 120 frames.
    """
    out = tmp_path / "tex_scene.mp4"
    _rust.render_to_mp4(tex_scene_payload, str(out))

    info = ffprobe_stream(out)
    assert info["width"] == "480"
    assert info["height"] == "270"
    assert info["codec_name"] == "h264"
    assert info["pix_fmt"] == "yuv420p"
    assert info["avg_frame_rate"] == "30/1"
    assert info["nb_read_frames"] == "120"


@requires_ffprobe
def test_combined_tex_text_scene_renders(tmp_path: Path, combined_scene_payload: dict) -> None:
    """Tex + Text + Polyline coexist in one Evaluator; render produces a
    valid mp4 with ffprobe-readable metadata."""
    out = tmp_path / "combined.mp4"
    _rust.render_to_mp4(combined_scene_payload, str(out))

    info = ffprobe_stream(out, "width,height,nb_read_frames")
    assert info["width"] == "480"
    assert info["height"] == "270"
    # 0.5 + 1.0 + 0.5 = 2.0s @ 30fps = 60 frames.
    assert info["nb_read_frames"] == "60"


def test_text_scene_render_is_byte_deterministic(tmp_path: Path, text_scene_payload: dict) -> None:
    """Slice E §5 success criterion: same scene rendered twice ⇒ byte-identical
    mp4. Catches nondeterminism in eval (HashMap iteration), shaping, raster
    dispatch, or libx264 threading."""
    out_a = tmp_path / "text_a.mp4"
    out_b = tmp_path / "text_b.mp4"
    _rust.render_to_mp4(text_scene_payload, str(out_a))
    _rust.render_to_mp4(text_scene_payload, str(out_b))
    assert _sha256(out_a) == _sha256(out_b), "TextScene renders are not byte-identical between runs"


def test_tex_scene_render_is_byte_deterministic(tmp_path: Path, tex_scene_payload: dict) -> None:
    out_a = tmp_path / "tex_a.mp4"
    out_b = tmp_path / "tex_b.mp4"
    _rust.render_to_mp4(tex_scene_payload, str(out_a))
    _rust.render_to_mp4(tex_scene_payload, str(out_b))
    assert _sha256(out_a) == _sha256(out_b), "TexScene renders are not byte-identical between runs"


def test_combined_scene_render_is_byte_deterministic(
    tmp_path: Path, combined_scene_payload: dict
) -> None:
    out_a = tmp_path / "combined_a.mp4"
    out_b = tmp_path / "combined_b.mp4"
    _rust.render_to_mp4(combined_scene_payload, str(out_a))
    _rust.render_to_mp4(combined_scene_payload, str(out_b))
    assert _sha256(out_a) == _sha256(
        out_b
    ), "Combined Tex+Text scene renders are not byte-identical between runs"
