"""Manimax IR — v1, Python side.

Spec: ``docs/ir-schema.md``. This module mirrors ``crates/manim-rs-ir/src/lib.rs``.
Keep them in sync. Drift is caught by ``deny_unknown_fields`` on the Rust side
and ``forbid_unknown_fields=True`` on msgspec.

All unions are internally tagged via ``msgspec.Struct(tag_field=..., tag=...)``,
which matches serde's ``#[serde(tag = "...")]`` wire format.
"""

from __future__ import annotations

import msgspec

SCHEMA_VERSION: int = 1

# ---------------------------------------------------------------------------
# Scalars
#
# Serialization representations chosen to match the Rust side:
#   - Vec3 / RgbaSrgb serialize as JSON arrays (tuples work; lists would too).
#   - Time is f64 seconds.
#   - ObjectId is u32.
# ---------------------------------------------------------------------------

Time = float
ObjectId = int
Vec3 = tuple[float, float, float]
RgbaSrgb = tuple[float, float, float, float]


class Resolution(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    width: int
    height: int


class SceneMetadata(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    schema_version: int
    fps: int
    duration: Time
    resolution: Resolution
    background: RgbaSrgb


# ---------------------------------------------------------------------------
# Object — internally tagged union with discriminator "kind".
# ---------------------------------------------------------------------------


class Polyline(
    msgspec.Struct,
    tag_field="kind",
    tag="Polyline",
    forbid_unknown_fields=True,
    frozen=True,
):
    points: tuple[Vec3, ...]
    stroke_color: RgbaSrgb
    stroke_width: float
    closed: bool


Object = Polyline  # union placeholder; grows in Slice C (Circle | BezPath | ...)


# ---------------------------------------------------------------------------
# TimelineOp — internally tagged union with discriminator "op".
# ---------------------------------------------------------------------------


class AddOp(
    msgspec.Struct,
    tag_field="op",
    tag="Add",
    forbid_unknown_fields=True,
    frozen=True,
):
    t: Time
    id: ObjectId
    object: Object


class RemoveOp(
    msgspec.Struct,
    tag_field="op",
    tag="Remove",
    forbid_unknown_fields=True,
    frozen=True,
):
    t: Time
    id: ObjectId


TimelineOp = AddOp | RemoveOp


# ---------------------------------------------------------------------------
# Easing — internally tagged union with discriminator "kind".
# ---------------------------------------------------------------------------


class LinearEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Linear",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


Easing = LinearEasing  # grows in Slice C (Smooth | Rush | ...)


# ---------------------------------------------------------------------------
# Track — internally tagged union with discriminator "kind".
# ---------------------------------------------------------------------------


class PositionSegment(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    t0: Time
    t1: Time
    from_: Vec3 = msgspec.field(name="from")  # `from` is a Python keyword
    to: Vec3
    easing: Easing


class PositionTrack(
    msgspec.Struct,
    tag_field="kind",
    tag="Position",
    forbid_unknown_fields=True,
    frozen=True,
):
    id: ObjectId
    segments: tuple[PositionSegment, ...]


Track = PositionTrack  # grows in Slice C (Opacity | Color | ...)


class Scene(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    metadata: SceneMetadata
    timeline: tuple[TimelineOp, ...]
    tracks: tuple[Track, ...]


# ---------------------------------------------------------------------------
# Codec helpers. Centralizing encode/decode here means callers do not need to
# import msgspec directly, and we can swap encoders (msgspec.json → orjson →
# pythonize) without touching call sites.
# ---------------------------------------------------------------------------

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
