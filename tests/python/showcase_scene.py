"""Longer multi-act showcase scene — three acts over ~9 seconds.

Not a test fixture; sits in tests/python/ alongside integration_scene.py
because it uses the same import path and CLI entry point.
"""

from __future__ import annotations

import math

from manim_rs import (
    BezPath,
    Colorize,
    FadeIn,
    FadeOut,
    NotQuiteThere,
    Overshoot,
    Polyline,
    Rotate,
    ScaleBy,
    Scene,
    Smooth,
    ThereAndBack,
    Translate,
    Wiggle,
    cubic_to,
    move_to,
    quad_to,
)


def _square_points(s: float = 0.6):
    return [(-s, -s, 0.0), (s, -s, 0.0), (s, s, 0.0), (-s, s, 0.0)]


def _triangle_points(s: float = 0.8):
    return [
        (0.0, s, 0.0),
        (-s * 0.866, -s * 0.5, 0.0),
        (s * 0.866, -s * 0.5, 0.0),
    ]


def _teardrop_verbs():
    return [
        move_to((0.0, -0.8, 0.0)),
        quad_to((1.0, -0.3, 0.0), (0.4, 0.8, 0.0)),
        cubic_to((-0.1, 1.2, 0.0), (-1.1, 0.5, 0.0), (-0.3, -0.2, 0.0)),
        quad_to((-0.5, -0.7, 0.0), (0.0, -0.8, 0.0)),
    ]


class ShowcaseScene(Scene):
    """Three-act showcase: arrival, individual flourish, convergence."""

    def construct(self) -> None:
        red_square = Polyline(
            _square_points(),
            stroke_color=(1.0, 0.2, 0.2, 1.0),
            stroke_width=0.08,
            closed=True,
        )
        green_teardrop = BezPath(
            _teardrop_verbs(),
            stroke_color=(1.0, 1.0, 1.0, 1.0),
            stroke_width=0.04,
            fill_color=(0.0, 0.85, 0.25, 1.0),
        )
        blue_triangle = Polyline(
            _triangle_points(),
            stroke_color=(0.3, 0.4, 1.0, 1.0),
            stroke_width=0.08,
            closed=True,
        )

        self.add(red_square)
        self.add(green_teardrop)
        self.add(blue_triangle)

        # ----- Act 1 (0 → 3s): arrival --------------------------------------
        # Fade in and slide to a row across the frame.
        self.play(
            FadeIn(red_square, duration=1.5),
            FadeIn(green_teardrop, duration=1.5),
            FadeIn(blue_triangle, duration=1.5),
            Translate(red_square, (-3.5, 0.0, 0.0), duration=2.0, easing=Smooth()),
            Translate(blue_triangle, (3.5, 0.0, 0.0), duration=2.0, easing=Smooth()),
            ScaleBy(green_teardrop, 1.4, duration=2.0, easing=Overshoot(pull_factor=1.5)),
        )
        self.wait(0.5)

        # ----- Act 2 (3.5 → 6.5s): individual flourish ----------------------
        # Square spins twice; teardrop wiggles its color; triangle orbits.
        self.play(
            Rotate(red_square, 4 * math.pi, duration=3.0, easing=Smooth()),
            Colorize(
                green_teardrop,
                from_color=(0.0, 0.85, 0.25, 1.0),
                to_color=(1.0, 0.7, 0.0, 1.0),
                duration=3.0,
                easing=Wiggle(wiggles=3.0),
            ),
            Translate(blue_triangle, (-2.0, 1.5, 0.0), duration=1.5, easing=Smooth()),
            Rotate(blue_triangle, 2 * math.pi, duration=3.0),
        )
        # Triangle finishes its orbit — second leg back to the right.
        self.play(
            Translate(blue_triangle, (2.0, -1.5, 0.0), duration=1.5, easing=Smooth()),
        )

        # ----- Act 3 (6.5 → 9s): convergence --------------------------------
        # Pull everything to center and fade out with one last color shift.
        self.play(
            Translate(red_square, (3.5, 0.0, 0.0), duration=1.5, easing=ThereAndBack()),
            Translate(blue_triangle, (-3.5, 0.0, 0.0), duration=1.5, easing=ThereAndBack()),
            ScaleBy(
                green_teardrop,
                0.7,
                duration=1.5,
                easing=NotQuiteThere(inner=Smooth(), proportion=0.6),
            ),
        )
        self.play(
            FadeOut(red_square, duration=1.0),
            FadeOut(green_teardrop, duration=1.0),
            FadeOut(blue_triangle, duration=1.0),
        )
