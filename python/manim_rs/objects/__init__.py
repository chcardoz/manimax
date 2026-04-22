"""Author-facing objects. Each exposes a ``to_ir()`` that emits an IR object."""

from manim_rs.objects.geometry import (
    BezPath,
    Polyline,
    close,
    cubic_to,
    line_to,
    move_to,
    quad_to,
)

__all__ = [
    "BezPath",
    "Polyline",
    "close",
    "cubic_to",
    "line_to",
    "move_to",
    "quad_to",
]
