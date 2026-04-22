"""Manimax — Python authoring frontend for the Rust runtime."""

from manim_rs import _rust, ir
from manim_rs.animate import Translate
from manim_rs.objects import Polyline
from manim_rs.scene import Scene

__all__ = ["Polyline", "Scene", "Translate", "_rust", "ir"]
__version__ = "0.0.0"
