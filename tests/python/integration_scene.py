"""Slice C Step 7 integration scene.

Exercises every surface Slice C added — both geometry types, all five track
kinds, and multiple easings — in a single scene that renders to a watchable
mp4. This file is loaded by ``test_integration_scene.py`` via the same CLI
path users would take, so anything broken in scene discovery or the
Python→IR bridge surfaces here first.

Composition notes:

- Three objects, each with a distinct *base color band* so pixel-space
  centroid tests can tell them apart even under scale/rotation.
- Every object carries ≥2 simultaneous tracks.
- Easings used: ``Linear``, ``Smooth``, ``Overshoot``, ``ThereAndBack``
  (three non-linear). ``Smooth`` is wrapped in ``NotQuiteThere`` on the
  triangle to exercise the recursive-combinator path.
- Both *fill* and *stroke* are represented (teardrop has both; square and
  triangle are stroke-only to keep the centroid heuristic simple).
"""

from __future__ import annotations

import math

from manim_rs import (
    BezPath,
    Colorize,
    FadeIn,
    NotQuiteThere,
    Overshoot,
    Polyline,
    Rotate,
    ScaleBy,
    Scene,
    Smooth,
    ThereAndBack,
    Translate,
    cubic_to,
    move_to,
    quad_to,
)

SCENE_DURATION = 2.0  # seconds


def _square_points() -> list[tuple[float, float, float]]:
    s = 0.6
    return [(-s, -s, 0.0), (s, -s, 0.0), (s, s, 0.0), (-s, s, 0.0)]


def _triangle_points() -> list[tuple[float, float, float]]:
    # Equilateral-ish triangle centered on origin.
    s = 0.8
    return [
        (0.0, s, 0.0),
        (-s * 0.866, -s * 0.5, 0.0),
        (s * 0.866, -s * 0.5, 0.0),
    ]


def _teardrop_verbs():
    # Start bottom, curve up-right (quad), curve back down-left (cubic),
    # close. Closed so fill has something to cover.
    return [
        move_to((0.0, -0.8, 0.0)),
        quad_to((1.0, -0.3, 0.0), (0.4, 0.8, 0.0)),
        cubic_to((-0.1, 1.2, 0.0), (-1.1, 0.5, 0.0), (-0.3, -0.2, 0.0)),
        quad_to((-0.5, -0.7, 0.0), (0.0, -0.8, 0.0)),
    ]


class IntegrationScene(Scene):
    """Three objects, five track kinds, four easings."""

    def construct(self) -> None:
        red_square = Polyline(
            _square_points(),
            stroke_color=(1.0, 0.0, 0.0, 1.0),
            stroke_width=0.08,
            closed=True,
        )
        green_teardrop = BezPath(
            _teardrop_verbs(),
            stroke_color=(1.0, 1.0, 1.0, 1.0),
            stroke_width=0.04,
            fill_color=(0.0, 0.9, 0.2, 1.0),
        )
        blue_triangle = Polyline(
            _triangle_points(),
            stroke_color=(0.2, 0.3, 1.0, 1.0),
            stroke_width=0.08,
            closed=True,
        )

        self.add(red_square)
        self.add(green_teardrop)
        self.add(blue_triangle)

        # Parallel-play: every animation runs together for 2 seconds.
        self.play(
            # Red square: translate left + rotate (linear + smooth).
            Translate(red_square, (-1.8, 0.0, 0.0), duration=SCENE_DURATION),
            Rotate(red_square, math.pi, duration=SCENE_DURATION, easing=Smooth()),
            # Green teardrop: fade in + scale pop (overshoot on scale makes the
            # "pop" look intentional, not like a bug).
            FadeIn(green_teardrop, duration=SCENE_DURATION * 0.5),
            ScaleBy(
                green_teardrop,
                1.3,
                duration=SCENE_DURATION,
                easing=Overshoot(pull_factor=1.5),
            ),
            # Blue triangle: translate right with ThereAndBack (so it returns to
            # origin at end — lets the centroid test assert "near origin at t=T")
            # plus a color shift into the final cyan/white using NotQuiteThere(Smooth).
            Translate(
                blue_triangle,
                (1.8, 0.4, 0.0),
                duration=SCENE_DURATION,
                easing=ThereAndBack(),
            ),
            Colorize(
                blue_triangle,
                from_color=(0.2, 0.3, 1.0, 1.0),
                to_color=(0.2, 0.9, 1.0, 1.0),
                duration=SCENE_DURATION,
                easing=NotQuiteThere(inner=Smooth(), proportion=0.3),
            ),
        )
