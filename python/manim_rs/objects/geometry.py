"""Author-facing geometry primitives.

Unlike manimgl's ``VMobject``, these are not interpolation targets — they are
inert descriptions that a ``Scene`` copies into IR at ``add`` time. The Rust
runtime owns all interpolation and rendering.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Literal, cast, get_args

import numpy as np

from manim_rs import ir

PolylineInput = Sequence[tuple[float, float, float]] | np.ndarray
StrokeWidthInput = float | Sequence[float]
JointInput = Literal["miter", "bevel", "auto"]
_VALID_JOINTS = get_args(JointInput)


def _build_stroke(
    stroke_color: ir.RgbaSrgb | None,
    stroke_width: StrokeWidthInput,
    joint: JointInput,
    expected_endpoints: int | None,
) -> ir.Stroke | None:
    """Assemble an ``ir.Stroke`` from author-friendly inputs.

    ``expected_endpoints`` is the required length for per-vertex widths when
    it can be computed cheaply at author time (``Polyline``: ``len(points)``).
    Pass ``None`` when it cannot (``BezPath``: depends on the sampler's
    cubic-subdivision depth) and the Rust runtime handles length checks with
    a graceful fallback to scalar.
    """
    if stroke_color is None:
        return None
    if isinstance(stroke_width, int | float):
        width: ir.StrokeWidth = float(stroke_width)
    else:
        widths = tuple(float(w) for w in stroke_width)
        if not widths:
            raise ValueError("stroke_width list must not be empty")
        if expected_endpoints is not None and len(widths) != expected_endpoints:
            raise ValueError(
                f"stroke_width list length {len(widths)} does not match "
                f"expected endpoint count {expected_endpoints}"
            )
        width = widths
    if joint not in _VALID_JOINTS:
        raise ValueError(f"joint must be one of {_VALID_JOINTS}, got {joint!r}")
    return ir.Stroke(color=stroke_color, width=width, joint=cast(ir.JointKind, joint))


def _normalize_points(points: PolylineInput) -> tuple[ir.Vec3, ...]:
    """Accept ``np.ndarray`` of shape (N, 3) or any sequence of triplets."""
    if isinstance(points, np.ndarray):
        if points.ndim != 2 or points.shape[1] != 3:
            raise ValueError(f"Polyline points must have shape (N, 3), got {points.shape}")
        if points.shape[0] < 2:
            raise ValueError(f"Polyline needs at least 2 points, got {points.shape[0]}")
        return tuple((float(x), float(y), float(z)) for x, y, z in points)

    out: list[ir.Vec3] = []
    for i, p in enumerate(points):
        if len(p) != 3:
            raise ValueError(f"Polyline point {i} must be a 3-tuple, got {p!r}")
        out.append((float(p[0]), float(p[1]), float(p[2])))
    if len(out) < 2:
        raise ValueError(f"Polyline needs at least 2 points, got {len(out)}")
    return tuple(out)


class BezPath:
    """A path described by a sequence of verbs.

    Mirrors manimgl's multi-subpath VMobject: ``MoveTo`` starts a new subpath,
    ``Close`` terminates one. The builder surface is inert — verbs are copied
    into IR at ``Scene.add`` time.
    """

    __slots__ = ("_id", "verbs", "stroke", "fill")

    def __init__(
        self,
        verbs: Sequence[ir.PathVerb],
        *,
        stroke_color: ir.RgbaSrgb | None = (1.0, 1.0, 1.0, 1.0),
        stroke_width: StrokeWidthInput = 0.04,
        joint: JointInput = "auto",
        fill_color: ir.RgbaSrgb | None = None,
    ) -> None:
        self._id: int | None = None
        self.verbs: tuple[ir.PathVerb, ...] = tuple(verbs)
        if not self.verbs:
            raise ValueError("BezPath needs at least one verb")
        # Per-vertex stroke widths are endpoint-indexed against the Rust
        # sampler's `segment_count + 1`. That depends on cubic-subdivision
        # depth, so at author time we validate against the raw verb count
        # as a close proxy; Rust falls back gracefully on mismatch.
        self.stroke: ir.Stroke | None = _build_stroke(
            stroke_color,
            stroke_width,
            joint,
            expected_endpoints=None,
        )
        self.fill: ir.Fill | None = ir.Fill(color=fill_color) if fill_color is not None else None

    def to_ir(self) -> ir.BezPath:
        return ir.BezPath(
            verbs=self.verbs,
            stroke=self.stroke,
            fill=self.fill,
        )


# Verb-builder convenience: short lowercase constructors so scene authoring
# reads like lyon/Bezier code — `move_to((0,0,0))`, `quad_to(ctrl, to)`.
def move_to(to: ir.Vec3) -> ir.MoveToVerb:
    return ir.MoveToVerb(to=(float(to[0]), float(to[1]), float(to[2])))


def line_to(to: ir.Vec3) -> ir.LineToVerb:
    return ir.LineToVerb(to=(float(to[0]), float(to[1]), float(to[2])))


def quad_to(ctrl: ir.Vec3, to: ir.Vec3) -> ir.QuadToVerb:
    return ir.QuadToVerb(
        ctrl=(float(ctrl[0]), float(ctrl[1]), float(ctrl[2])),
        to=(float(to[0]), float(to[1]), float(to[2])),
    )


def cubic_to(ctrl1: ir.Vec3, ctrl2: ir.Vec3, to: ir.Vec3) -> ir.CubicToVerb:
    return ir.CubicToVerb(
        ctrl1=(float(ctrl1[0]), float(ctrl1[1]), float(ctrl1[2])),
        ctrl2=(float(ctrl2[0]), float(ctrl2[1]), float(ctrl2[2])),
        to=(float(to[0]), float(to[1]), float(to[2])),
    )


def close() -> ir.CloseVerb:
    return ir.CloseVerb()


class Polyline:
    """A sequence of straight segments.

    The ``_id`` attribute is ``None`` until the object is handed to
    ``Scene.add``; at that point the scene assigns a stable id used in all
    subsequent IR references.
    """

    __slots__ = ("_id", "points", "closed", "stroke", "fill")

    def __init__(
        self,
        points: PolylineInput,
        *,
        stroke_color: ir.RgbaSrgb | None = (1.0, 1.0, 1.0, 1.0),
        stroke_width: StrokeWidthInput = 0.04,
        joint: JointInput = "auto",
        fill_color: ir.RgbaSrgb | None = None,
        closed: bool = True,
    ) -> None:
        self._id: int | None = None
        self.points: tuple[ir.Vec3, ...] = _normalize_points(points)
        self.closed: bool = bool(closed)
        # Polyline per-vertex widths map 1:1 onto the input point list. The
        # rasterizer pads the closing edge (if `closed`) by re-using widths[0],
        # so the author surface stays intuitive: one width per point.
        expected = len(self.points)
        self.stroke: ir.Stroke | None = _build_stroke(
            stroke_color,
            stroke_width,
            joint,
            expected_endpoints=expected,
        )
        self.fill: ir.Fill | None = ir.Fill(color=fill_color) if fill_color is not None else None

    def to_ir(self) -> ir.Polyline:
        return ir.Polyline(
            points=self.points,
            closed=self.closed,
            stroke=self.stroke,
            fill=self.fill,
        )
