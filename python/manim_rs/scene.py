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

_DEFAULT_FPS = 30
_DEFAULT_RESOLUTION = ir.Resolution(width=480, height=270)
_DEFAULT_BACKGROUND: ir.RgbaSrgb = (0.0, 0.0, 0.0, 1.0)

AnyObject = Polyline | BezPath

# Track-kind → (Segment class, Track class). The Scene maintains one
# segment list per (object_id, track_kind) and merges lists into Tracks
# at ``.ir`` time.
_TRACK_KINDS: tuple[tuple[type, type], ...] = (
    (ir.PositionSegment, ir.PositionTrack),
    (ir.OpacitySegment, ir.OpacityTrack),
    (ir.RotationSegment, ir.RotationTrack),
    (ir.ScaleSegment, ir.ScaleTrack),
    (ir.ColorSegment, ir.ColorTrack),
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
        # Segment buckets keyed by (track-kind-index, object-id). One bucket
        # per value type per object — merged into a single ``Track`` by
        # ``.ir`` using ``_TRACK_KINDS``.
        self._segments: list[dict[int, list]] = [dict() for _ in _TRACK_KINDS]
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
                bucket = self._bucket_for(track)
                bucket.setdefault(track.id, []).extend(track.segments)
        self._t = t_start + longest

    def _bucket_for(self, track: ir.Track) -> dict[int, list]:
        for idx, (_seg_cls, track_cls) in enumerate(_TRACK_KINDS):
            if isinstance(track, track_cls):
                return self._segments[idx]
        raise TypeError(f"unhandled track kind: {type(track).__name__}")

    # ------------------------------------------------------------------
    # IR emission
    # ------------------------------------------------------------------

    @property
    def ir(self) -> ir.Scene:
        tracks: list[ir.Track] = []
        for idx, (_seg_cls, track_cls) in enumerate(_TRACK_KINDS):
            bucket = self._segments[idx]
            for oid, segs in sorted(bucket.items()):
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
