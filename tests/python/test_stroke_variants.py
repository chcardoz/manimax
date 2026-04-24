"""Per-vertex stroke width + joint selection on the Python authoring surface.

Covers:

- ``Polyline`` / ``BezPath`` accept a scalar or per-point/per-endpoint width.
- Per-vertex widths round-trip through the Rust IR as a JSON array.
- ``joint`` defaults to ``"auto"`` and serializes as a lowercase string.
- Length-mismatched per-vertex widths raise ``ValueError`` at construction
  (Polyline — Python knows the expected count eagerly).
- Unknown joint strings raise ``ValueError`` at construction.
"""

from __future__ import annotations

import pytest
from manim_rs import _rust, ir
from manim_rs.objects.geometry import BezPath, Polyline, close, line_to, move_to


def _polyline_ir(poly: Polyline) -> bytes:
    """Encode a minimal Scene containing just this polyline."""
    scene = ir.Scene(
        metadata=ir.SceneMetadata(
            schema_version=ir.SCHEMA_VERSION,
            fps=30,
            duration=0.0,
            resolution=ir.Resolution(width=16, height=16),
            background=(0.0, 0.0, 0.0, 1.0),
        ),
        timeline=(ir.AddOp(t=0.0, id=1, object=poly.to_ir()),),
        tracks=(),
    )
    return ir.encode(scene)


def test_scalar_width_still_works_and_defaults_to_auto_joint() -> None:
    poly = Polyline(
        [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
        stroke_width=0.05,
        closed=False,
    )
    assert poly.stroke is not None
    assert poly.stroke.width == 0.05
    assert poly.stroke.joint == "auto"


def test_per_vertex_width_roundtrips_through_rust() -> None:
    widths = (0.02, 0.08, 0.02, 0.08)
    poly = Polyline(
        [(-2.0, 0.0, 0.0), (0.0, 1.0, 0.0), (2.0, 0.0, 0.0), (0.0, -1.0, 0.0)],
        stroke_width=widths,
        joint="miter",
        closed=True,
    )
    assert poly.stroke is not None
    assert poly.stroke.width == widths
    assert poly.stroke.joint == "miter"

    encoded = _polyline_ir(poly).decode("utf-8")
    assert '"width":[0.02,0.08,0.02,0.08]' in encoded
    assert '"joint":"miter"' in encoded

    # Rust consumes the per-vertex form and re-emits the same JSON.
    rust_echoed = _rust.roundtrip_ir(encoded)
    assert '"width":[0.02,0.08,0.02,0.08]' in rust_echoed


def test_polyline_mismatched_per_vertex_widths_rejected_at_construction() -> None:
    with pytest.raises(ValueError, match="does not match expected endpoint count"):
        Polyline(
            [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)],
            stroke_width=(0.02, 0.05),  # 2 widths for 3 points
        )


def test_bezpath_accepts_per_vertex_widths_without_length_check() -> None:
    # BezPath vertex count depends on the Rust sampler's cubic subdivision
    # depth, so Python does not validate eagerly — only basic non-empty and
    # joint-string checks fire at construction.
    widths = (0.02, 0.04, 0.02)
    path = BezPath(
        verbs=[move_to((-1.0, 0.0, 0.0)), line_to((1.0, 0.0, 0.0)), close()],
        stroke_width=widths,
        joint="bevel",
    )
    assert path.stroke is not None
    assert path.stroke.width == widths
    assert path.stroke.joint == "bevel"


def test_empty_per_vertex_widths_rejected() -> None:
    with pytest.raises(ValueError, match="must not be empty"):
        Polyline(
            [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
            stroke_width=(),
        )


def test_invalid_joint_rejected() -> None:
    with pytest.raises(ValueError, match="joint must be"):
        Polyline(
            [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
            joint="round",  # type: ignore[arg-type]
        )


def test_stroke_joint_absent_on_the_wire_defaults_to_auto_on_rust() -> None:
    # Validates the Rust `#[serde(default)]` on JointKind — a stray legacy
    # payload with no `joint` field is still accepted.
    legacy = (
        '{"metadata":{"schema_version":1,"fps":30,"duration":0.0,'
        '"resolution":{"width":16,"height":16},"background":[0,0,0,1]},'
        '"timeline":[{"op":"Add","t":0.0,"id":1,"object":'
        '{"kind":"Polyline","points":[[-1,0,0],[1,0,0]],"closed":false,'
        '"stroke":{"color":[1,1,1,1],"width":0.04},"fill":null}}],'
        '"tracks":[]}'
    )
    # Rust accepts. Python msgspec side is strict (joint must be present on
    # structs constructed in Python), but Rust → Python round-trip refills it.
    echoed = _rust.roundtrip_ir(legacy)
    assert '"joint":"auto"' in echoed
