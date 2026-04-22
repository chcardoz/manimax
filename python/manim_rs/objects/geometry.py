"""Author-facing geometry primitives.

Unlike manimgl's ``VMobject``, these are not interpolation targets — they are
inert descriptions that a ``Scene`` copies into IR at ``add`` time. The Rust
runtime owns all interpolation and rendering.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import TYPE_CHECKING

from manim_rs import ir

if TYPE_CHECKING:
    import numpy as np

    PolylineInput = Sequence[tuple[float, float, float]] | np.ndarray


def _normalize_points(points: object) -> tuple[ir.Vec3, ...]:
    """Accept ``np.ndarray`` of shape (N, 3) or any sequence of triplets."""
    try:
        import numpy as np

        if isinstance(points, np.ndarray):
            if points.ndim != 2 or points.shape[1] != 3:
                raise ValueError(f"Polyline points must have shape (N, 3), got {points.shape}")
            if points.shape[0] < 2:
                raise ValueError(f"Polyline needs at least 2 points, got {points.shape[0]}")
            return tuple((float(x), float(y), float(z)) for x, y, z in points)
    except ImportError:  # pragma: no cover — numpy is a declared dep
        pass

    out: list[ir.Vec3] = []
    for i, p in enumerate(points):  # type: ignore[arg-type]
        if len(p) != 3:
            raise ValueError(f"Polyline point {i} must be a 3-tuple, got {p!r}")
        out.append((float(p[0]), float(p[1]), float(p[2])))
    if len(out) < 2:
        raise ValueError(f"Polyline needs at least 2 points, got {len(out)}")
    return tuple(out)


class Polyline:
    """A sequence of straight segments.

    The ``_id`` attribute is ``None`` until the object is handed to
    ``Scene.add``; at that point the scene assigns a stable id used in all
    subsequent IR references.
    """

    __slots__ = ("_id", "points", "stroke_color", "stroke_width", "closed")

    def __init__(
        self,
        points: object,
        *,
        stroke_color: ir.RgbaSrgb = (1.0, 1.0, 1.0, 1.0),
        stroke_width: float = 0.04,
        closed: bool = True,
    ) -> None:
        self._id: int | None = None
        self.points: tuple[ir.Vec3, ...] = _normalize_points(points)
        self.stroke_color: ir.RgbaSrgb = stroke_color
        self.stroke_width: float = float(stroke_width)
        self.closed: bool = bool(closed)

    def to_ir(self) -> ir.Polyline:
        return ir.Polyline(
            points=self.points,
            stroke_color=self.stroke_color,
            stroke_width=self.stroke_width,
            closed=self.closed,
        )
