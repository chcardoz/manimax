"""Stable IR primitives — types that don't grow.

Anything in this file is a deliberate "anti-extension" promise: scalars and
fixed-vocabulary unions that won't have variants added over time. New variant
axes (geometry, ops, tracks, easing, paint) live in their own files.

Contains:
- Scalar aliases: ``Time``, ``ObjectId``, ``Vec3``, ``RgbaSrgb``.
- Path verbs: the universal ``MoveTo`` / ``LineTo`` / ``QuadTo`` / ``CubicTo`` /
  ``Close`` vocabulary used by lyon, kurbo, SVG, Skia. Closed system; not an
  extension axis.
"""

from __future__ import annotations

import msgspec

Time = float
ObjectId = int
Vec3 = tuple[float, float, float]
RgbaSrgb = tuple[float, float, float, float]


# ============================================================================
# === Path verbs ===
#
# Internally tagged union with discriminator "kind". Mirrors the Rust
# `PathVerb` enum and SVG / lyon path events.
# ============================================================================


class MoveToVerb(
    msgspec.Struct,
    tag_field="kind",
    tag="MoveTo",
    forbid_unknown_fields=True,
    frozen=True,
):
    to: Vec3


class LineToVerb(
    msgspec.Struct,
    tag_field="kind",
    tag="LineTo",
    forbid_unknown_fields=True,
    frozen=True,
):
    to: Vec3


class QuadToVerb(
    msgspec.Struct,
    tag_field="kind",
    tag="QuadTo",
    forbid_unknown_fields=True,
    frozen=True,
):
    ctrl: Vec3
    to: Vec3


class CubicToVerb(
    msgspec.Struct,
    tag_field="kind",
    tag="CubicTo",
    forbid_unknown_fields=True,
    frozen=True,
):
    ctrl1: Vec3
    ctrl2: Vec3
    to: Vec3


class CloseVerb(
    msgspec.Struct,
    tag_field="kind",
    tag="Close",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


PathVerb = MoveToVerb | LineToVerb | QuadToVerb | CubicToVerb | CloseVerb
