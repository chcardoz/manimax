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
from manim_rs.objects.tex import Tex
from manim_rs.objects.text import Text

__all__ = [
    "BezPath",
    "Polyline",
    "Tex",
    "Text",
    "close",
    "cubic_to",
    "line_to",
    "move_to",
    "quad_to",
]
