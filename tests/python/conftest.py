"""Shared fixtures for the Python test suite.

Four call sites used to redefine "the canonical Slice B scene" (a unit square
translated +2 on x over a fixed duration). They drifted independently; this
module gives them one source of truth. Each test picks the duration/fps it
needs via the helper.
"""

from __future__ import annotations

from manim_rs import ir
from manim_rs.animate.transforms import Translate
from manim_rs.objects.geometry import Polyline
from manim_rs.scene import Scene


def canonical_square_scene(
    *,
    fps: int = 30,
    duration: float = 2.0,
    stroke_width: float = 0.08,
    translate_x: float = 2.0,
    resolution: ir.Resolution | None = None,
) -> Scene:
    """Unit square at the origin, translated `translate_x` over `duration`s.

    The canonical fixture for exercising the Python → IR → Rust pipeline with
    one object and one position track.
    """
    scene = Scene(fps=fps, resolution=resolution) if resolution else Scene(fps=fps)
    square = Polyline(
        [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
        stroke_width=stroke_width,
    )
    scene.add(square)
    scene.play(Translate(square, (translate_x, 0.0, 0.0), duration=duration))
    return scene
