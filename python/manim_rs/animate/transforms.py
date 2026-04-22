"""Animations. Each one emits IR tracks when the Scene asks for them.

Contrast with ``manimlib/animation/animation.py`` — there, ``Animation`` owns an
``interpolate(alpha)`` method that mutates a mobject. Here, animations are
inert descriptions of time-varying value tracks. All interpolation happens in
the Rust evaluator.
"""

from __future__ import annotations

from typing import Protocol

from manim_rs import ir
from manim_rs.objects.geometry import BezPath, Polyline

Target = Polyline | BezPath


class Animation(Protocol):
    duration: float

    def emit(self, t_start: float) -> list[ir.Track]: ...


def _require_id(obj: Target, verb: str) -> int:
    if obj._id is None:
        raise RuntimeError(
            f"{verb} target has not been added to a scene — call scene.add(obj) first."
        )
    return obj._id


def _check_duration(duration: float, verb: str) -> float:
    d = float(duration)
    if d <= 0.0:
        raise ValueError(f"{verb} duration must be positive, got {duration}")
    return d


def _default_easing() -> ir.Easing:
    return ir.LinearEasing()


def _vec3(v: ir.Vec3) -> ir.Vec3:
    return (float(v[0]), float(v[1]), float(v[2]))


def _rgba(c: ir.RgbaSrgb) -> ir.RgbaSrgb:
    return (float(c[0]), float(c[1]), float(c[2]), float(c[3]))


class Translate:
    """Translate an object by ``delta`` over ``duration`` seconds.

    Emits a single ``PositionSegment`` from ``(0,0,0)`` to ``delta``. The
    evaluator composes this with the object's base position from
    ``AddOp`` — so ``Translate`` always means "offset relative to the
    position the object was born at".
    """

    __slots__ = ("obj", "delta", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        delta: ir.Vec3,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.delta: ir.Vec3 = _vec3(delta)
        self.duration = _check_duration(duration, "Translate")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "Translate")
        return [
            ir.PositionTrack(
                id=oid,
                segments=(
                    ir.PositionSegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=(0.0, 0.0, 0.0),
                        to=self.delta,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class Rotate:
    """Rotate by ``angle`` radians over ``duration``."""

    __slots__ = ("obj", "angle", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        angle: float,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.angle = float(angle)
        self.duration = _check_duration(duration, "Rotate")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "Rotate")
        return [
            ir.RotationTrack(
                id=oid,
                segments=(
                    ir.RotationSegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=0.0,
                        to=self.angle,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class ScaleTo:
    """Scale from 1.0 to ``factor`` over ``duration``.

    Name is ``ScaleTo`` rather than ``Scale`` to avoid confusion with any
    future "scale-by" verb: the target value here is absolute.
    """

    __slots__ = ("obj", "factor", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        factor: float,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.factor = float(factor)
        self.duration = _check_duration(duration, "ScaleTo")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "ScaleTo")
        return [
            ir.ScaleTrack(
                id=oid,
                segments=(
                    ir.ScaleSegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=1.0,
                        to=self.factor,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class FadeIn:
    """Opacity 0 → 1 over ``duration``."""

    __slots__ = ("obj", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.duration = _check_duration(duration, "FadeIn")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "FadeIn")
        return [
            ir.OpacityTrack(
                id=oid,
                segments=(
                    ir.OpacitySegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=0.0,
                        to=1.0,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class FadeOut:
    """Opacity 1 → 0 over ``duration``."""

    __slots__ = ("obj", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.duration = _check_duration(duration, "FadeOut")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "FadeOut")
        return [
            ir.OpacityTrack(
                id=oid,
                segments=(
                    ir.OpacitySegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=1.0,
                        to=0.0,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class Colorize:
    """Override color to ``color`` over ``duration``, starting from ``from_``.

    The evaluator's color-track semantics are "last-write override" — the
    active color-track sample replaces the authored object color for the
    current frame. We author an explicit ``from_`` so the transition is
    visible from frame 0 onward without needing the evaluator to snapshot
    the authored color.
    """

    __slots__ = ("obj", "from_color", "to_color", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        from_color: ir.RgbaSrgb,
        to_color: ir.RgbaSrgb,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.from_color: ir.RgbaSrgb = _rgba(from_color)
        self.to_color: ir.RgbaSrgb = _rgba(to_color)
        self.duration = _check_duration(duration, "Colorize")
        self.easing: ir.Easing = easing or _default_easing()

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, "Colorize")
        return [
            ir.ColorTrack(
                id=oid,
                segments=(
                    ir.ColorSegment(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=self.from_color,
                        to=self.to_color,
                        easing=self.easing,
                    ),
                ),
            )
        ]
