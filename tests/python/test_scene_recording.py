"""Scene recording test — the Python authoring surface emits correct IR.

Asserts that a minimal scene (one polyline, one Translate over 2s) produces
exactly the IR described in ``docs/ir-schema.md`` and ``docs/slices/slice-b.md``.
"""

from __future__ import annotations

import pytest
from manim_rs import Polyline, Scene, Translate, ir


def _square() -> Polyline:
    return Polyline(
        [
            (-1.0, -1.0, 0.0),
            (1.0, -1.0, 0.0),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0),
        ],
        stroke_color=(1.0, 1.0, 1.0, 1.0),
        stroke_width=0.04,
        closed=True,
    )


def test_single_translate_produces_expected_ir() -> None:
    scene = Scene()
    sq = _square()
    scene.add(sq)
    scene.play(Translate(sq, (2.0, 0.0, 0.0), duration=2.0))

    built = scene.ir

    assert built.metadata.schema_version == ir.SCHEMA_VERSION
    assert built.metadata.fps == 30
    assert built.metadata.duration == 2.0
    assert built.metadata.resolution == ir.Resolution(width=480, height=270)

    assert len(built.timeline) == 1
    add = built.timeline[0]
    assert isinstance(add, ir.AddOp)
    assert add.t == 0.0
    assert add.id == 1
    assert isinstance(add.object, ir.Polyline)
    assert add.object.closed is True
    assert len(add.object.points) == 4

    assert len(built.tracks) == 1
    track = built.tracks[0]
    assert isinstance(track, ir.PositionTrack)
    assert track.id == 1
    assert len(track.segments) == 1
    seg = track.segments[0]
    assert seg.t0 == 0.0
    assert seg.t1 == 2.0
    assert seg.from_ == (0.0, 0.0, 0.0)
    assert seg.to == (2.0, 0.0, 0.0)
    assert isinstance(seg.easing, ir.LinearEasing)


def test_recorded_scene_roundtrips_through_rust() -> None:
    """Recorded IR must survive the same wire trip the Step 1 test exercised."""
    from manim_rs import _rust

    scene = Scene()
    sq = _square()
    scene.add(sq)
    scene.play(Translate(sq, (2.0, 0.0, 0.0), duration=2.0))

    encoded = ir.encode(scene.ir).decode("utf-8")
    echoed = _rust.roundtrip_ir(encoded)
    assert ir.decode(echoed) == scene.ir


def test_wait_advances_clock() -> None:
    scene = Scene()
    scene.wait(0.5)
    scene.wait(1.5)
    assert scene.ir.metadata.duration == 2.0


def test_add_assigns_stable_ids() -> None:
    scene = Scene()
    a = _square()
    b = _square()
    scene.add(a)
    scene.add(b)
    assert a._id == 1
    assert b._id == 2


def test_cannot_translate_before_add() -> None:
    scene = Scene()
    sq = _square()
    with pytest.raises(RuntimeError, match="has not been added"):
        scene.play(Translate(sq, (1.0, 0.0, 0.0), duration=1.0))


def test_parallel_plays_use_max_duration() -> None:
    scene = Scene()
    a = _square()
    b = _square()
    scene.add(a)
    scene.add(b)
    scene.play(
        Translate(a, (1.0, 0.0, 0.0), duration=1.0),
        Translate(b, (0.0, 1.0, 0.0), duration=2.5),
    )
    assert scene.ir.metadata.duration == 2.5
    assert len(scene.ir.tracks) == 2


def test_polyline_accepts_numpy_ndarray() -> None:
    """numpy is a declared top-level dep; the ndarray input path must work."""
    import numpy as np
    from manim_rs import Polyline

    points = np.array(
        [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
        dtype=np.float64,
    )
    poly = Polyline(points)
    assert len(poly.points) == 4
    assert poly.points[0] == (-1.0, -1.0, 0.0)
    # The IR shape must not change when input is ndarray.
    irpoly = poly.to_ir()
    assert len(irpoly.points) == 4


def test_remove_emits_timeline_op_at_current_clock() -> None:
    scene = Scene()
    sq = _square()
    scene.add(sq)
    scene.wait(1.5)
    scene.remove(sq)

    assert len(scene._timeline) == 2
    add, rem = scene._timeline
    assert isinstance(add, ir.AddOp) and add.t == 0.0 and add.id == 1
    assert isinstance(rem, ir.RemoveOp) and rem.t == 1.5 and rem.id == 1


def test_remove_before_add_raises() -> None:
    scene = Scene()
    sq = _square()
    with pytest.raises(RuntimeError, match="not active"):
        scene.remove(sq)


def test_double_add_raises() -> None:
    scene = Scene()
    sq = _square()
    scene.add(sq)
    with pytest.raises(RuntimeError, match="already added"):
        scene.add(sq)
