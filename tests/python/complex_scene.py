"""Stress-test scene — many objects, many tracks, many easings.

Designed to find edges of the current pipeline. Not a pytest — a visual
fixture you render with the CLI:

    python -m manim_rs render tests/python/complex_scene.py ComplexScene \\
        /tmp/complex.mp4 --quality uhd --fps 120 -o

Composition:

- **Central pulsar** — a BezPath flower with fill + stroke, rotating,
  scaling via Overshoot, and color-shifting through a rainbow.
- **Orbiting ring** — 16 polygonal satellites around the pulsar. Each has
  a phase-shifted translation (ThereAndBack on a different radius),
  rotation (NotQuiteThere(Smooth)), and a staggered fade-in.
- **Outer frame** — 4 corner BezPath squiggles doing synchronised
  ScaleBy + Colorize, outside the orbit to frame the composition.
- **Background trails** — 6 long diagonal strokes fading in and out,
  each with RunningStart easing so they feel like shooting stars.

Totals: ~27 objects, ~85 tracks, 6 distinct easings.
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
    RunningStart,
    ScaleBy,
    Scene,
    Smooth,
    ThereAndBack,
    Translate,
    cubic_to,
    line_to,
    move_to,
    quad_to,
)

DURATION = 4.0


def _regular_polygon(n: int, radius: float) -> list[tuple[float, float, float]]:
    pts = []
    for i in range(n):
        theta = (2.0 * math.pi * i) / n
        pts.append((radius * math.cos(theta), radius * math.sin(theta), 0.0))
    return pts


def _flower_verbs(petals: int, r_inner: float, r_outer: float):
    verbs = []
    for i in range(petals):
        a0 = (2.0 * math.pi * i) / petals
        a1 = (2.0 * math.pi * (i + 1)) / petals
        am = (a0 + a1) * 0.5
        p0 = (r_inner * math.cos(a0), r_inner * math.sin(a0), 0.0)
        p_mid = (r_outer * math.cos(am) * 1.6, r_outer * math.sin(am) * 1.6, 0.0)
        p1 = (r_inner * math.cos(a1), r_inner * math.sin(a1), 0.0)
        if i == 0:
            verbs.append(move_to(p0))
        verbs.append(quad_to(p_mid, p1))
    return verbs


def _squiggle_verbs(cx: float, cy: float, sign: int):
    # A small cubic-bezier wiggle that starts near a corner and arcs inward.
    s = sign
    return [
        move_to((cx, cy, 0.0)),
        cubic_to(
            (cx - s * 1.2, cy - 0.5, 0.0),
            (cx - s * 0.6, cy - s * 1.2, 0.0),
            (cx - s * 1.8, cy - s * 0.9, 0.0),
        ),
        quad_to(
            (cx - s * 2.4, cy - s * 0.2, 0.0),
            (cx - s * 2.0, cy + s * 0.6, 0.0),
        ),
        line_to((cx - s * 2.5, cy + s * 1.1, 0.0)),
    ]


def _rainbow(t: float) -> tuple[float, float, float, float]:
    # t in [0, 1) → RGBA on a rainbow wheel.
    h = t * 6.0
    i = int(h) % 6
    f = h - int(h)
    if i == 0:
        r, g, b = 1.0, f, 0.0
    elif i == 1:
        r, g, b = 1.0 - f, 1.0, 0.0
    elif i == 2:
        r, g, b = 0.0, 1.0, f
    elif i == 3:
        r, g, b = 0.0, 1.0 - f, 1.0
    elif i == 4:
        r, g, b = f, 0.0, 1.0
    else:
        r, g, b = 1.0, 0.0, 1.0 - f
    return (r, g, b, 1.0)


class ComplexScene(Scene):
    def construct(self) -> None:
        # --- Central pulsar (BezPath with fill + stroke) ---------------------
        pulsar = BezPath(
            _flower_verbs(petals=6, r_inner=0.6, r_outer=1.0),
            stroke_color=(1.0, 1.0, 1.0, 1.0),
            stroke_width=0.04,
            fill_color=(1.0, 0.3, 0.6, 1.0),
        )
        self.add(pulsar)

        # --- Ring of satellites (mixed polygons) -----------------------------
        N = 16
        ring_radius = 3.0
        satellites: list[Polyline] = []
        for i in range(N):
            sides = 3 + (i % 4)  # triangles, squares, pentagons, hexagons
            color_hue = i / N
            sat = Polyline(
                _regular_polygon(sides, 0.22),
                stroke_color=_rainbow(color_hue),
                stroke_width=0.03,
                closed=True,
            )
            self.add(sat)
            satellites.append(sat)

        # --- Outer frame squiggles (BezPath, stroke-only) --------------------
        corners = [(6.5, 3.3, +1), (-6.5, 3.3, -1), (6.5, -3.3, +1), (-6.5, -3.3, -1)]
        squiggles: list[BezPath] = []
        for cx, cy, s in corners:
            sq = BezPath(
                _squiggle_verbs(cx, cy, s),
                stroke_color=(0.7, 0.8, 1.0, 1.0),
                stroke_width=0.06,
            )
            self.add(sq)
            squiggles.append(sq)

        # --- Background trails (long diagonal polylines) --------------------
        trails: list[Polyline] = []
        for i in range(6):
            t_off = (i - 2.5) * 1.0
            trail = Polyline(
                [(-7.0 + t_off, -4.0, 0.0), (7.0 + t_off, 4.0, 0.0)],
                stroke_color=(0.4, 0.6, 1.0, 1.0),
                stroke_width=0.015,
                closed=False,
            )
            self.add(trail)
            trails.append(trail)

        # ---------------------------------------------------------------------
        # Orchestrate animations — all running in parallel in one play() call
        # so total scene length is DURATION.
        # ---------------------------------------------------------------------
        anims = []

        # Pulsar: scale Overshoot, rotate smoothly, colorize through magenta→cyan.
        anims.append(ScaleBy(pulsar, 1.4, duration=DURATION, easing=Overshoot(pull_factor=1.8)))
        anims.append(Rotate(pulsar, 2.0 * math.pi, duration=DURATION, easing=Smooth()))
        anims.append(
            Colorize(
                pulsar,
                from_color=(1.0, 0.3, 0.6, 1.0),
                to_color=(0.2, 0.9, 1.0, 1.0),
                duration=DURATION,
                easing=ThereAndBack(),
            )
        )

        # Satellites: orbit via ThereAndBack translate + phased Rotate + staggered fade-in.
        for i, sat in enumerate(satellites):
            phase = (2.0 * math.pi * i) / N
            cx = ring_radius * math.cos(phase)
            cy = ring_radius * math.sin(phase)
            # Place each satellite at its starting orbit position by giving a
            # Translate from origin to (cx, cy) that holds — since the base
            # position is origin, we use a ThereAndBack that peaks at (cx, cy)
            # to trace half an orbit out and back.
            anims.append(
                Translate(
                    sat,
                    (cx, cy, 0.0),
                    duration=DURATION,
                    easing=ThereAndBack(),
                )
            )
            anims.append(
                Rotate(
                    sat,
                    2.0 * math.pi * (1 if i % 2 == 0 else -1),
                    duration=DURATION,
                    easing=NotQuiteThere(inner=Smooth(), proportion=0.15),
                )
            )
            # Staggered fade-in: each satellite comes in a bit later than the last.
            fade_dur = max(0.4, DURATION * 0.4 - (i / N) * DURATION * 0.25)
            anims.append(FadeIn(sat, duration=fade_dur))

        # Corner squiggles: simultaneous scale-pop + rainbow colorize.
        for i, sq in enumerate(squiggles):
            anims.append(ScaleBy(sq, 1.3, duration=DURATION, easing=ThereAndBack()))
            anims.append(
                Colorize(
                    sq,
                    from_color=(0.7, 0.8, 1.0, 1.0),
                    to_color=_rainbow(i / 4),
                    duration=DURATION,
                    easing=Smooth(),
                )
            )

        # Background trails: fade in over the first 40% of the scene using
        # RunningStart so they feel like they're streaking into view.
        for tr in trails:
            anims.append(
                FadeIn(
                    tr,
                    duration=DURATION * 0.4,
                    easing=RunningStart(pull_factor=0.4),
                )
            )

        self.play(*anims)
