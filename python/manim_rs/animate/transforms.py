"""Animations. Each one emits IR tracks when the Scene asks for them.

Contrast with ``manimlib/animation/animation.py`` — there, ``Animation`` owns an
``interpolate(alpha)`` method that mutates a mobject. Here, animations are
inert descriptions of time-varying value tracks. All interpolation happens in
the Rust evaluator.
"""

from __future__ import annotations

from typing import Protocol

from manim_rs import ir
from manim_rs.objects.geometry import Polyline


class Animation(Protocol):
    """Protocol all animations satisfy.

    ``duration``: seconds the animation occupies.
    ``emit(t_start)``: returns the IR tracks this animation contributes,
    keyed by absolute time on the scene clock.
    """

    duration: float

    def emit(self, t_start: float) -> list[ir.Track]: ...


class Translate:
    """Translate an object by ``delta`` over ``duration`` seconds.

    Slice B restriction: treats the object as starting at the origin for the
    animation and producing a single linear segment from ``(0,0,0)`` to
    ``delta``. Composition with prior motion lands in Slice C.
    """

    __slots__ = ("obj", "delta", "duration")

    def __init__(self, obj: Polyline, delta: ir.Vec3, duration: float) -> None:
        self.obj = obj
        self.delta: ir.Vec3 = (float(delta[0]), float(delta[1]), float(delta[2]))
        self.duration = float(duration)
        if self.duration <= 0.0:
            raise ValueError(f"Translate duration must be positive, got {duration}")

    def emit(self, t_start: float) -> list[ir.Track]:
        if self.obj._id is None:
            raise RuntimeError(
                "Translate target has not been added to a scene — "
                "call scene.add(obj) before scene.play(Translate(obj, ...))."
            )
        return [
            ir.PositionTrack(
                id=self.obj._id,
                segments=(
                    ir.PositionSegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=(0.0, 0.0, 0.0),
                        to=self.delta,
                        easing=ir.LinearEasing(),
                    ),
                ),
            )
        ]
