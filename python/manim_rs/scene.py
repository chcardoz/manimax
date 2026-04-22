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
from manim_rs.objects.geometry import Polyline

# Slice B defaults — hardcoded per `docs/slices/slice-b.md` §2.
# CLI flags override fps and duration in Step 9; resolution/background stay
# hardcoded until Slice C.
_DEFAULT_FPS = 30
_DEFAULT_RESOLUTION = ir.Resolution(width=480, height=270)
_DEFAULT_BACKGROUND: ir.RgbaSrgb = (0.0, 0.0, 0.0, 1.0)


class Scene:
    """Builds an IR scene by recording imperative calls."""

    __slots__ = (
        "_t",
        "_next_id",
        "_objects",
        "_active",
        "_timeline",
        "_position_segments",
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
        self._objects: dict[int, Polyline] = {}
        self._active: set[int] = set()
        self._timeline: list[ir.TimelineOp] = []
        # Per-object ordered list of position segments. Merged into a single
        # `ir.PositionTrack` per object by `.ir`.
        self._position_segments: dict[int, list[ir.PositionSegment]] = {}
        self.fps: int = fps
        self.resolution: ir.Resolution = resolution
        self.background: ir.RgbaSrgb = background

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def add(self, obj: Polyline) -> None:
        if obj._id is not None:
            raise RuntimeError(f"object {obj!r} already added to a scene (id={obj._id})")
        obj._id = self._next_id
        self._next_id += 1
        self._objects[obj._id] = obj
        self._active.add(obj._id)
        self._timeline.append(ir.AddOp(t=self._t, id=obj._id, object=obj.to_ir()))

    def remove(self, obj: Polyline) -> None:
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
                self._absorb_track(track)
        self._t = t_start + longest

    # ------------------------------------------------------------------
    # IR emission
    # ------------------------------------------------------------------

    @property
    def ir(self) -> ir.Scene:
        tracks: list[ir.Track] = [
            ir.PositionTrack(id=oid, segments=tuple(segs))
            for oid, segs in sorted(self._position_segments.items())
            if segs
        ]
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

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _absorb_track(self, track: ir.Track) -> None:
        if isinstance(track, ir.PositionTrack):
            self._position_segments.setdefault(track.id, []).extend(track.segments)
        else:  # pragma: no cover — only PositionTrack exists in Slice B
            raise TypeError(f"unhandled track kind: {type(track).__name__}")
