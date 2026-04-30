"""End-to-end integration scene — exercises every Python authoring surface
shipped through Slice E.

Loaded by ``test_integration_scene.py`` via the same CLI path users would
take, so anything broken in scene discovery, the Python→IR bridge, eval,
raster, or encode surfaces here first.

Coverage map (refresh when adding a new author-facing primitive):

- Mobjects: ``Polyline``, ``BezPath``, ``Tex`` (Slice E §1–§5).
- Path verbs: ``move_to``, ``line_to``, ``quad_to``, ``cubic_to``, ``close``
  (the green teardrop alone uses all five).
- Transforms: ``Translate``, ``Rotate``, ``ScaleBy``, ``FadeIn``,
  ``FadeOut``, ``Colorize``.
- Easings (representative — easing leaves are unit-tested elsewhere):
  ``Linear`` (default), ``Smooth``, ``DoubleSmooth``, ``Overshoot``,
  ``ThereAndBack``, ``NotQuiteThere``, ``RushFrom``, ``Wiggle``.
- Scene API: ``add``, ``play``, ``wait``, ``remove``.

Composition notes:

- Four objects, each with a distinct color band so pixel-space centroid
  tests can tell them apart even under scale/rotation. Yellow Tex is the
  Slice-E addition; its band is disjoint from red/green/blue.
- Three timeline phases separated by a ``wait``:
  arrival → flourish → depart-and-remove. The final phase calls
  ``remove`` on faded-out objects so the IR's ``RemoveOp`` path is
  exercised even though the pixel diff is moot once alpha hits zero.
"""

from __future__ import annotations

import math

from manim_rs import (
    BezPath,
    Colorize,
    DoubleSmooth,
    FadeIn,
    FadeOut,
    NotQuiteThere,
    Overshoot,
    Polyline,
    Rotate,
    RushFrom,
    ScaleBy,
    Scene,
    Smooth,
    Tex,
    ThereAndBack,
    Translate,
    Wiggle,
    close,
    cubic_to,
    line_to,
    move_to,
    quad_to,
)

SCENE_DURATION = 3.0  # seconds


def _square_points() -> list[tuple[float, float, float]]:
    s = 0.6
    return [(-s, -s, 0.0), (s, -s, 0.0), (s, s, 0.0), (-s, s, 0.0)]


def _triangle_points() -> list[tuple[float, float, float]]:
    s = 0.8
    return [
        (0.0, s, 0.0),
        (-s * 0.866, -s * 0.5, 0.0),
        (s * 0.866, -s * 0.5, 0.0),
    ]


def _teardrop_verbs():
    # All five BezPath verbs used in this single shape:
    # move_to → quad_to → cubic_to → line_to → close.
    return [
        move_to((0.0, -0.8, 0.0)),
        quad_to((1.0, -0.3, 0.0), (0.4, 0.8, 0.0)),
        cubic_to((-0.1, 1.2, 0.0), (-1.1, 0.5, 0.0), (-0.3, -0.2, 0.0)),
        line_to((-0.3, -0.7, 0.0)),
        close(),
    ]


class IntegrationScene(Scene):
    """Three-phase end-to-end coverage scene."""

    def construct(self) -> None:
        red_square = Polyline(
            _square_points(),
            stroke_color=(1.0, 0.0, 0.0, 1.0),
            stroke_width=0.08,
            closed=True,
        )
        green_teardrop = BezPath(
            _teardrop_verbs(),
            stroke_color=(0.6, 0.9, 0.6, 1.0),
            stroke_width=0.04,
            fill_color=(0.0, 0.9, 0.2, 1.0),
        )
        blue_triangle = Polyline(
            _triangle_points(),
            stroke_color=(0.2, 0.3, 1.0, 1.0),
            stroke_width=0.08,
            closed=True,
        )
        # Tex: Slice E surface. Yellow so its color band is disjoint from
        # red/green/blue and from the (faintly-green) teardrop stroke.
        yellow_pi = Tex(
            r"\pi",
            color=(1.0, 0.95, 0.0, 1.0),
            scale=1.6,
        )

        self.add(red_square)
        self.add(green_teardrop)
        self.add(blue_triangle)
        self.add(yellow_pi)

        # ----- Phase 1 (0 → 1.0s): arrival ---------------------------------
        # Everyone fades in and slides to a starting layout. Mixes four
        # easings to cover the catalog without overcrowding the test.
        self.play(
            FadeIn(red_square, duration=1.0, easing=Smooth()),
            FadeIn(green_teardrop, duration=1.0, easing=DoubleSmooth()),
            FadeIn(blue_triangle, duration=1.0),  # Linear (default)
            FadeIn(yellow_pi, duration=1.0, easing=Smooth()),
            Translate(red_square, (-1.8, 0.0, 0.0), duration=1.0, easing=Smooth()),
            Translate(blue_triangle, (1.8, 0.0, 0.0), duration=1.0, easing=Smooth()),
            Translate(yellow_pi, (0.0, 0.95, 0.0), duration=1.0, easing=RushFrom()),
        )

        # ----- Wait gap (1.0 → 1.2s) ---------------------------------------
        # Holds the layout for 6 frames at 30 fps. Verifies wait advances
        # the clock without emitting tracks.
        self.wait(0.2)

        # ----- Phase 2 (1.2 → 2.2s): flourish ------------------------------
        # Each object gets a distinct on-screen flourish. Hits Rotate,
        # ScaleBy, Colorize, and a non-trivial Translate together.
        self.play(
            Rotate(red_square, math.pi, duration=1.0, easing=Smooth()),
            ScaleBy(green_teardrop, 1.3, duration=1.0, easing=Overshoot(pull_factor=1.5)),
            Colorize(
                green_teardrop,
                from_color=(0.0, 0.9, 0.2, 1.0),
                to_color=(0.2, 0.95, 0.5, 1.0),
                duration=1.0,
                easing=NotQuiteThere(inner=Smooth(), proportion=0.3),
            ),
            Translate(
                blue_triangle,
                (0.0, 0.6, 0.0),
                duration=1.0,
                easing=ThereAndBack(),
            ),
            Rotate(yellow_pi, math.pi / 6, duration=1.0, easing=Wiggle(wiggles=2.0)),
        )

        # ----- Phase 3 (2.2 → 2.6s): depart --------------------------------
        # Blue and the Tex fade out. Red and green stay through the end.
        self.play(
            FadeOut(blue_triangle, duration=0.4),
            FadeOut(yellow_pi, duration=0.4, easing=Smooth()),
        )

        # Remove the now-invisible objects. Pixel tests can't tell remove
        # from alpha=0, but the IR's RemoveOp path is exercised here so
        # the timeline-op round-trip and active-set tracking get covered.
        self.remove(blue_triangle)
        self.remove(yellow_pi)

        # ----- Tail (2.6 → 3.0s): hold ------------------------------------
        # 12 frames of just red + green. Confirms removed objects don't
        # reappear and that the renderer handles a shrinking active set.
        self.wait(0.4)
