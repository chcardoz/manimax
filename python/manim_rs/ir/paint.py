"""Paint descriptors: ``Stroke`` and ``Fill``.

A geometry's ``stroke`` / ``fill`` fields are ``Optional``; the wire format
requires both fields and allows ``null``. This file is the home for any future
``Brush`` union (Color | Gradient | Pattern) — that growth lands here, not in
``objects.py``.
"""

from __future__ import annotations

from typing import Literal

import msgspec

from manim_rs.ir._primitives import RgbaSrgb

# Stroke width is either a single scalar (uniform across the stroke) or a
# per-vertex list. msgspec's union-of-scalar-and-array matches the Rust side's
# `#[serde(untagged)]` on `StrokeWidth`.
StrokeWidth = float | tuple[float, ...]

JointKind = Literal["miter", "bevel", "auto"]


class Stroke(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    color: RgbaSrgb
    width: StrokeWidth
    joint: JointKind = "auto"


class Fill(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    color: RgbaSrgb
