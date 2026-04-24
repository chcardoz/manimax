"""Slice C Step 1 exit criterion — ``_rust.eval_at`` returns the expected
state for the canonical Slice B scene at chosen times.

Mirrors the Rust unit tests in ``crates/manim-rs-eval/src/lib.rs`` from the
Python side, verifying the new pythonize-based FFI round-trips IR → Scene →
SceneState → plain Python values correctly.
"""

from __future__ import annotations

import pytest
from conftest import canonical_square_scene
from manim_rs import _rust, ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def _canonical_scene() -> Scene:
    return canonical_square_scene(fps=30, duration=2.0)


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


def test_parallel_same_kind_animations_compose() -> None:
    """Two parallel Translates on one object must sum, not drop one.

    Regression: the recorder used to flatten same-kind parallel animations
    into a single track with overlapping segments; the evaluator returns on
    the first match and silently dropped the second contribution.
    """
    scene = Scene(fps=30)
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=0.08,
    )
    scene.add(square)
    scene.play(
        Translate(square, (3.0, 0.0, 0.0), duration=2.0),
        Translate(square, (0.0, 4.0, 0.0), duration=2.0),
    )

    # Two tracks in the IR — not one with overlapping segments.
    position_tracks = [t for t in scene.ir.tracks if isinstance(t, ir.PositionTrack)]
    assert len(position_tracks) == 2

    ir_dict = ir.to_builtins(scene.ir)
    # Midpoint: each track contributes half its delta; sum is (1.5, 2.0, 0.0).
    mid = _rust.eval_at(ir_dict, 1.0)["objects"][0]["position"]
    assert mid == pytest.approx((1.5, 2.0, 0.0))
    # Endpoint: full delta sum.
    end = _rust.eval_at(ir_dict, 2.0)["objects"][0]["position"]
    assert end == pytest.approx((3.0, 4.0, 0.0))
