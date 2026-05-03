"""Continuous-value time series — segments and the ``Track`` union.

Extension axis: animatable properties. Today: position, opacity, rotation,
scale, color. Visibility / depth / camera tracks named in arch §4 land here.

Each segment carries common ``t0 / t1 / easing`` plus value endpoints typed
to its track. Each track carries a ``segments`` tuple.
"""

from __future__ import annotations

import msgspec

from manim_rs.ir._primitives import ObjectId, RgbaSrgb, Time, Vec3
from manim_rs.ir.easing import Easing

# ============================================================================
# === Track segments ===
#
# One shape per value type. `t0 / t1 / easing` are common across all kinds.
# ============================================================================


class PositionSegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: Vec3 = msgspec.field(name="from")  # `from` is a Python keyword
    to: Vec3
    easing: Easing


class OpacitySegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: float = msgspec.field(name="from")
    to: float
    easing: Easing


class RotationSegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: float = msgspec.field(name="from")  # radians
    to: float
    easing: Easing


class ScaleSegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: float = msgspec.field(name="from")
    to: float
    easing: Easing


class ColorSegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: RgbaSrgb = msgspec.field(name="from")
    to: RgbaSrgb
    easing: Easing


# ============================================================================
# === Tracks ===
#
# Internally tagged union with discriminator "kind".
# ============================================================================


class PositionTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Position",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[PositionSegment, ...]


class OpacityTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Opacity",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[OpacitySegment, ...]


class RotationTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Rotation",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[RotationSegment, ...]


class ScaleTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Scale",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[ScaleSegment, ...]


class ColorTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Color",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[ColorSegment, ...]


Track = PositionTrack | OpacityTrack | RotationTrack | ScaleTrack | ColorTrack
