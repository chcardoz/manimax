"""Slice C Step 1 exit criterion — ``_rust.eval_at`` returns the expected
state for the canonical Slice B scene at chosen times.

Mirrors the Rust unit tests in ``crates/manim-rs-eval/src/lib.rs`` from the
Python side, verifying the new pythonize-based FFI round-trips IR → Scene →
SceneState → plain Python values correctly.
"""

from __future__ import annotations

import pytest
from manim_rs import _rust, ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def _canonical_scene() -> Scene:
    scene = Scene(fps=30)
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=0.08,
    )
    scene.add(square)
    scene.play(Translate(square, (2.0, 0.0, 0.0), duration=2.0))
    return scene


def test_eval_at_start() -> None:
    state = _rust.eval_at(ir.to_builtins(_canonical_scene().ir), 0.0)
    assert len(state["objects"]) == 1
    obj = state["objects"][0]
    assert obj["position"] == (0.0, 0.0, 0.0)
    # Geometry passes through — the IR tag survives the round-trip.
    assert obj["object"]["kind"] == "Polyline"
    # Slice C Step 3 added the track-derived state. With no Opacity / Rotation
    # / Scale / Color tracks, defaults must surface untouched.
    assert obj["opacity"] == 1.0
    assert obj["rotation"] == 0.0
    assert obj["scale"] == 1.0
    assert obj["color_override"] is None


def test_eval_at_midpoint() -> None:
    state = _rust.eval_at(ir.to_builtins(_canonical_scene().ir), 1.0)
    assert state["objects"][0]["position"] == (1.0, 0.0, 0.0)


def test_eval_at_endpoint() -> None:
    state = _rust.eval_at(ir.to_builtins(_canonical_scene().ir), 2.0)
    assert state["objects"][0]["position"] == (2.0, 0.0, 0.0)


def test_eval_at_past_endpoint_clamps() -> None:
    state = _rust.eval_at(ir.to_builtins(_canonical_scene().ir), 3.0)
    assert state["objects"][0]["position"] == (2.0, 0.0, 0.0)


def test_eval_at_before_add_is_empty() -> None:
    """Advance the add-time so the object is not yet live at t=0."""
    scene = Scene(fps=30)
    scene.wait(1.0)  # clock advances to t=1 before the add
    scene.add(Polyline([(-1.0, -1.0, 0.0), (1.0, 1.0, 0.0)], stroke_width=0.05))

    state = _rust.eval_at(ir.to_builtins(scene.ir), 0.5)
    assert state["objects"] == []


def test_eval_at_rejects_bad_ir() -> None:
    with pytest.raises(ValueError, match="IR depythonize failed"):
        _rust.eval_at({"not": "a scene"}, 0.0)
