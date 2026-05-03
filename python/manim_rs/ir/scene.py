"""Scene root — ``Resolution``, ``SceneMetadata``, ``Scene``.

The top-level container. Composes the timeline-op tuple and the track tuple
with the scene-level metadata. The IR's outermost shape.
"""

from __future__ import annotations

import msgspec

from manim_rs.ir._primitives import RgbaSrgb, Time
from manim_rs.ir.ops import TimelineOp
from manim_rs.ir.tracks import Track


class Resolution(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    width: int
    height: int


class SceneMetadata(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    schema_version: int
    fps: int
    duration: Time
    resolution: Resolution
    background: RgbaSrgb


class Scene(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    metadata: SceneMetadata
    timeline: tuple[TimelineOp, ...]
    tracks: tuple[Track, ...]
