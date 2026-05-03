"""Animations. Each one emits IR tracks when the Scene asks for them.

Contrast with ``manimlib/animation/animation.py`` — there, ``Animation`` owns an
``interpolate(alpha)`` method that mutates a mobject. Here, animations are
inert descriptions of time-varying value tracks. All interpolation happens in
the Rust evaluator.
"""

from __future__ import annotations

from typing import Any, ClassVar, Protocol

from manim_rs import ir
from manim_rs.objects._coerce import rgba as _rgba
from manim_rs.objects._coerce import vec3 as _vec3
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


class _SegmentAnimation:
    """Shared shape for single-track, single-segment animations.

    Each subclass declares ``_VERB`` (error-message label), ``_TRACK_CLS``, and
    ``_SEGMENT_CLS``, and overrides ``_endpoints()`` to return ``(from_, to)``.
    The boilerplate ``emit()`` then composes the one Track / one Segment IR.
    """

    _VERB: ClassVar[str]
    _TRACK_CLS: ClassVar[type[ir.Track]]
    _SEGMENT_CLS: ClassVar[type[Any]]

    __slots__ = ("obj", "duration", "easing")

    def __init__(
        self,
        obj: Target,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        self.obj = obj
        self.duration = _check_duration(duration, self._VERB)
        self.easing: ir.Easing = easing or _default_easing()

    def _endpoints(self) -> tuple[Any, Any]:
        raise NotImplementedError

    def emit(self, t_start: float) -> list[ir.Track]:
        oid = _require_id(self.obj, self._VERB)
        from_, to = self._endpoints()
        return [
            self._TRACK_CLS(
                id=oid,
                segments=(
                    self._SEGMENT_CLS(
                        t0=t_start,
                        t1=t_start + self.duration,
                        from_=from_,
                        to=to,
                        easing=self.easing,
                    ),
                ),
            )
        ]


class Translate(_SegmentAnimation):
    """Translate an object by ``delta`` over ``duration`` seconds.

    Emits a single ``PositionSegment`` from ``(0,0,0)`` to ``delta``. The
    evaluator composes this with the object's base position from
    ``AddOp`` — so ``Translate`` always means "offset relative to the
    position the object was born at".
    """

    _VERB = "Translate"
    _TRACK_CLS = ir.PositionTrack
    _SEGMENT_CLS = ir.PositionSegment

    __slots__ = ("delta",)

    def __init__(
        self,
        obj: Target,
        delta: ir.Vec3,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        super().__init__(obj, duration, easing=easing)
        self.delta: ir.Vec3 = _vec3(delta)

    def _endpoints(self) -> tuple[ir.Vec3, ir.Vec3]:
        return (0.0, 0.0, 0.0), self.delta


class Rotate(_SegmentAnimation):
    """Rotate by ``angle`` radians over ``duration``."""

    _VERB = "Rotate"
    _TRACK_CLS = ir.RotationTrack
    _SEGMENT_CLS = ir.RotationSegment

    __slots__ = ("angle",)

    def __init__(
        self,
        obj: Target,
        angle: float,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        super().__init__(obj, duration, easing=easing)
        self.angle = float(angle)

    def _endpoints(self) -> tuple[float, float]:
        return 0.0, self.angle


class ScaleBy(_SegmentAnimation):
    """Scale the object by ``factor`` over ``duration``.

    Multiplicative and composes with other scale animations: the evaluator
    multiplies across all active ScaleTracks on an object, so
    ``ScaleBy(o, 2)`` then ``ScaleBy(o, 1.5)`` lands at 3× the authored size.
    An absolute-scale verb (override, not compose) can be added later as a
    separate primitive.
    """

    _VERB = "ScaleBy"
    _TRACK_CLS = ir.ScaleTrack
    _SEGMENT_CLS = ir.ScaleSegment

    __slots__ = ("factor",)

    def __init__(
        self,
        obj: Target,
        factor: float,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        super().__init__(obj, duration, easing=easing)
        self.factor = float(factor)

    def _endpoints(self) -> tuple[float, float]:
        return 1.0, self.factor


class FadeIn(_SegmentAnimation):
    """Opacity 0 → 1 over ``duration``."""

    _VERB = "FadeIn"
    _TRACK_CLS = ir.OpacityTrack
    _SEGMENT_CLS = ir.OpacitySegment

    __slots__ = ()

    def _endpoints(self) -> tuple[float, float]:
        return 0.0, 1.0


class FadeOut(_SegmentAnimation):
    """Opacity 1 → 0 over ``duration``."""

    _VERB = "FadeOut"
    _TRACK_CLS = ir.OpacityTrack
    _SEGMENT_CLS = ir.OpacitySegment

    __slots__ = ()

    def _endpoints(self) -> tuple[float, float]:
        return 1.0, 0.0


class Colorize(_SegmentAnimation):
    """Override color to ``to_color`` over ``duration``, starting from ``from_color``.

    The evaluator's color-track semantics are "last-write override" — the
    active color-track sample replaces the authored object color for the
    current frame. We author an explicit ``from_`` so the transition is
    visible from frame 0 onward without needing the evaluator to snapshot
    the authored color.
    """

    _VERB = "Colorize"
    _TRACK_CLS = ir.ColorTrack
    _SEGMENT_CLS = ir.ColorSegment

    __slots__ = ("from_color", "to_color")

    def __init__(
        self,
        obj: Target,
        from_color: ir.RgbaSrgb,
        to_color: ir.RgbaSrgb,
        duration: float,
        *,
        easing: ir.Easing | None = None,
    ) -> None:
        super().__init__(obj, duration, easing=easing)
        self.from_color: ir.RgbaSrgb = _rgba(from_color)
        self.to_color: ir.RgbaSrgb = _rgba(to_color)

    def _endpoints(self) -> tuple[ir.RgbaSrgb, ir.RgbaSrgb]:
        return self.from_color, self.to_color
