"""Scene recorder.

A ``Scene`` looks imperative — ``add``, ``play``, ``wait`` — but it never
renders. Each call appends to an internal log, and ``.ir`` freezes that log
into an ``ir.Scene`` for the Rust runtime to evaluate.

Contrast with ``manimlib/scene/scene.py`` which advances the clock *and*
rasterizes a frame inside ``play``/``wait``. We decouple the two.
"""

from __future__ import annotations

from manim_rs import ir
from manim_rs.animate.transforms import Animation
from manim_rs.objects.geometry import BezPath, Polyline
from manim_rs.objects.tex import Tex

_DEFAULT_FPS = 30
_DEFAULT_RESOLUTION = ir.Resolution(width=480, height=270)
_DEFAULT_BACKGROUND: ir.RgbaSrgb = (0.0, 0.0, 0.0, 1.0)

AnyObject = Polyline | BezPath | Tex

# Track classes, in the order we emit them from ``.ir``. Iteration order drives
# the output `Scene.tracks` ordering; used for deterministic serialization.
_TRACK_CLASSES: tuple[type[ir.Track], ...] = (
    ir.PositionTrack,
    ir.OpacityTrack,
    ir.RotationTrack,
    ir.ScaleTrack,
    ir.ColorTrack,
)


class Scene:
    """Builds an IR scene by recording imperative calls."""

    __slots__ = (
        "_t",
        "_next_id",
        "_objects",
        "_active",
        "_timeline",
        "_segments",
        "fps",
        "resolution",
        "background",
    )

    def __init__(
        self,
        *,
        fps: int = _DEFAULT_FPS,
        resolution: ir.Resolution = _DEFAULT_RESOLUTION,
        background: ir.RgbaSrgb = _DEFAULT_BACKGROUND,
    ) -> None:
        self._t: float = 0.0
        self._next_id: int = 1
        self._objects: dict[int, AnyObject] = {}
        self._active: set[int] = set()
        self._timeline: list[ir.TimelineOp] = []
        # One inner list per emitted track — parallel same-kind animations on
        # the same object stay as separate tracks so the evaluator's N-ary
        # composition (sum for Position/Rotation, product for Opacity/Scale)
        # sees each contribution.
        self._segments: dict[type[ir.Track], dict[int, list[list]]] = {
            cls: {} for cls in _TRACK_CLASSES
        }
        self.fps: int = fps
        self.resolution: ir.Resolution = resolution
        self.background: ir.RgbaSrgb = background

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def construct(self) -> None:
        """User override — populate the scene via ``add``/``play``/``wait``.

        Mirrors manimgl's ``Scene.construct``. The default raises so a
        base-class instance with no subclass isn't silently rendered as an
        empty frame.
        """
        raise NotImplementedError(
            f"{type(self).__name__}.construct() is not implemented — "
            "subclass Scene and override construct() to author your scene."
        )

    def add(self, obj: AnyObject) -> None:
        if obj._id is not None:
            raise RuntimeError(f"object {obj!r} already added to a scene (id={obj._id})")
        obj._id = self._next_id
        self._next_id += 1
        self._objects[obj._id] = obj
        self._active.add(obj._id)
        self._timeline.append(ir.AddOp(t=self._t, id=obj._id, object=obj.to_ir()))

    def remove(self, obj: AnyObject) -> None:
        if obj._id is None or obj._id not in self._active:
            raise RuntimeError(f"object {obj!r} is not active in this scene")
        self._active.discard(obj._id)
        self._timeline.append(ir.RemoveOp(t=self._t, id=obj._id))

    # ------------------------------------------------------------------
    # Time
    # ------------------------------------------------------------------

    def wait(self, duration: float) -> None:
        if duration < 0.0:
            raise ValueError(f"wait duration must be non-negative, got {duration}")
        self._t += float(duration)

    def play(self, *animations: Animation) -> None:
        """Run animations in parallel; advance the clock by the longest one.

        Matches manimgl's ``self.play(a, b, c)`` shape — duration = max.
        """
        if not animations:
            return
        t_start = self._t
        longest = 0.0
        for anim in animations:
            longest = max(longest, anim.duration)
            for track in anim.emit(t_start):
                bucket = self._segments.get(type(track))
                if bucket is None:
                    raise TypeError(f"unhandled track kind: {type(track).__name__}")
                bucket.setdefault(track.id, []).append(list(track.segments))
        self._t = t_start + longest

    # ------------------------------------------------------------------
    # IR emission
    # ------------------------------------------------------------------

    @property
    def ir(self) -> ir.Scene:
        tracks: list[ir.Track] = []
        for track_cls in _TRACK_CLASSES:
            for oid, track_segs in sorted(self._segments[track_cls].items()):
                for segs in track_segs:
                    if segs:
                        tracks.append(track_cls(id=oid, segments=tuple(segs)))
        return ir.Scene(
            metadata=ir.SceneMetadata(
                schema_version=ir.SCHEMA_VERSION,
                fps=self.fps,
                duration=self._t,
                resolution=self.resolution,
                background=self.background,
            ),
            timeline=tuple(self._timeline),
            tracks=tuple(tracks),
        )
