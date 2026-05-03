"""Manimax IR — v1, Python side.

Spec: ``docs/ir-schema.md``. This package mirrors ``crates/manim-rs-ir/src/``.
Keep them in sync. Drift is caught by ``deny_unknown_fields`` on the Rust side
and ``forbid_unknown_fields=True`` on msgspec.

All unions are internally tagged via ``msgspec.Struct(tag_field=..., tag=...)``,
which matches serde's ``#[serde(tag = "...")]`` wire format.

This file is the public façade. Submodules organize the schema by extension
axis: each likely future addition (a new geometry, op, track, easing) touches
exactly one submodule plus the re-export below.
"""

from __future__ import annotations

import msgspec

from manim_rs.ir._primitives import (
    CloseVerb,
    CubicToVerb,
    LineToVerb,
    MoveToVerb,
    ObjectId,
    PathVerb,
    QuadToVerb,
    RgbaSrgb,
    Time,
    Vec3,
)
from manim_rs.ir.easing import (
    DoubleSmoothEasing,
    Easing,
    ExponentialDecayEasing,
    LinearEasing,
    LingeringEasing,
    NotQuiteThereEasing,
    OvershootEasing,
    RunningStartEasing,
    RushFromEasing,
    RushIntoEasing,
    SlowIntoEasing,
    SmoothEasing,
    SquishRateFuncEasing,
    ThereAndBackEasing,
    ThereAndBackWithPauseEasing,
    WiggleEasing,
)
from manim_rs.ir.objects import (
    BezPath,
    Object,
    Polyline,
    Tex,
    Text,
    TextAlign,
    TextWeight,
)
from manim_rs.ir.ops import AddOp, RemoveOp, TimelineOp
from manim_rs.ir.paint import Fill, JointKind, Stroke, StrokeWidth
from manim_rs.ir.scene import Resolution, Scene, SceneMetadata
from manim_rs.ir.tracks import (
    ColorSegment,
    ColorTrack,
    OpacitySegment,
    OpacityTrack,
    PositionSegment,
    PositionTrack,
    RotationSegment,
    RotationTrack,
    ScaleSegment,
    ScaleTrack,
    Track,
)

SCHEMA_VERSION: int = 3


# ============================================================================
# === Codec helpers ===
#
# Centralizing encode/decode here means callers do not need to import msgspec
# directly, and we can swap encoders (msgspec.json → orjson → pythonize)
# without touching call sites.
# ============================================================================

_encoder = msgspec.json.Encoder()
_decoder = msgspec.json.Decoder(Scene)


def encode(scene: Scene) -> bytes:
    """Serialize a Scene to UTF-8 JSON bytes."""
    return _encoder.encode(scene)


def decode(data: bytes | str) -> Scene:
    """Deserialize JSON bytes or str into a Scene, validating the schema."""
    if isinstance(data, str):
        data = data.encode("utf-8")
    return _decoder.decode(data)


def to_builtins(scene: Scene) -> dict:
    """Convert a Scene into a plain dict/list/scalar tree for the Rust FFI.

    pythonize's depythonize expects mapping protocol, which msgspec.Struct does
    not implement directly — msgspec.to_builtins gives a JSON-compatible Python
    object tree (with tag fields injected for tagged unions) that pythonize
    turns into the serde-typed Scene in one hop.
    """
    return msgspec.to_builtins(scene)


__all__ = [
    "AddOp",
    "BezPath",
    "CloseVerb",
    "ColorSegment",
    "ColorTrack",
    "CubicToVerb",
    "DoubleSmoothEasing",
    "Easing",
    "ExponentialDecayEasing",
    "Fill",
    "JointKind",
    "LineToVerb",
    "LinearEasing",
    "LingeringEasing",
    "MoveToVerb",
    "NotQuiteThereEasing",
    "Object",
    "ObjectId",
    "OpacitySegment",
    "OpacityTrack",
    "OvershootEasing",
    "PathVerb",
    "Polyline",
    "PositionSegment",
    "PositionTrack",
    "QuadToVerb",
    "RemoveOp",
    "Resolution",
    "RgbaSrgb",
    "RotationSegment",
    "RotationTrack",
    "RunningStartEasing",
    "RushFromEasing",
    "RushIntoEasing",
    "SCHEMA_VERSION",
    "ScaleSegment",
    "ScaleTrack",
    "Scene",
    "SceneMetadata",
    "SlowIntoEasing",
    "SmoothEasing",
    "SquishRateFuncEasing",
    "Stroke",
    "StrokeWidth",
    "Tex",
    "Text",
    "TextAlign",
    "TextWeight",
    "ThereAndBackEasing",
    "ThereAndBackWithPauseEasing",
    "Time",
    "TimelineOp",
    "Track",
    "Vec3",
    "WiggleEasing",
    "decode",
    "encode",
    "to_builtins",
]
