"""Slice E §1 acceptance — plain-text scene.

Run via:

    python -m manim_rs render examples.text_scene TextScene out.mp4 \
        --duration 3 --fps 30
"""

from __future__ import annotations

from manim_rs import FadeIn, Scene, Text, Translate


class TextScene(Scene):
    def construct(self) -> None:
        greeting = Text(
            "Hello, Manimax!",
            size=0.8,
            color=(0.4, 0.85, 1.0, 1.0),  # cool blue
        )
        self.add(greeting)
        self.play(FadeIn(greeting, duration=0.5))
        self.play(Translate(greeting, (0.0, -0.5, 0.0), duration=1.5))
        self.wait(1.0)
