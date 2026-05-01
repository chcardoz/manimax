"""Slice E §1 acceptance — math scene.

Run via:

    python -m manim_rs render examples.tex_scene TexScene out.mp4 \
        --duration 4 --fps 30
"""

from __future__ import annotations

from manim_rs import FadeIn, ScaleBy, Scene, Tex


class TexScene(Scene):
    def construct(self) -> None:
        # The Basel problem — exercises sub/superscripts, fraction, infinity,
        # Greek letters, and the limits of the sum operator.
        formula = Tex(
            r"\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}",
            color=(1.0, 0.85, 0.4, 1.0),  # warm gold
            scale=1.5,
        )
        self.add(formula)
        self.play(FadeIn(formula, duration=0.5))
        self.play(ScaleBy(formula, 1.2, duration=2.0))
        self.wait(1.5)
