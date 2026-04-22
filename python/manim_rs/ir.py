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

# ============================================================================
# === Scalars ===
#
# Serialization representations chosen to match the Rust side:
#   - Vec3 / RgbaSrgb serialize as JSON arrays (tuples work; lists would too).
#   - Time is f64 seconds.
#   - ObjectId is u32.
# ============================================================================

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


# ============================================================================
# === Stroke / Fill ===
#
# Shared paint descriptors. A geometry's `stroke` / `fill` fields are
# `Optional`; wire format requires both fields and allows `null`.
# ============================================================================


class Stroke(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    color: RgbaSrgb
    width: float


class Fill(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    color: RgbaSrgb


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


# ============================================================================
# === Objects ===
#
# Internally tagged union with discriminator "kind".
# ============================================================================


class Polyline(
    msgspec.Struct,
    tag_field="kind",
    tag="Polyline",
    forbid_unknown_fields=True,
    frozen=True,
):
    points: tuple[Vec3, ...]
    closed: bool
    stroke: Stroke | None
    fill: Fill | None


class BezPath(
    msgspec.Struct,
    tag_field="kind",
    tag="BezPath",
    forbid_unknown_fields=True,
    frozen=True,
):
    verbs: tuple[PathVerb, ...]
    stroke: Stroke | None
    fill: Fill | None


Object = Polyline | BezPath


# ============================================================================
# === Timeline ops ===
#
# Internally tagged union with discriminator "op".
# ============================================================================


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


# ============================================================================
# === Easing ===
#
# Internally tagged union with discriminator "kind". All 15 manimgl rate
# functions. Two are recursive combinators wrapping an inner easing.
# ============================================================================


class LinearEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Linear",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class SmoothEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Smooth",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class RushIntoEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="RushInto",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class RushFromEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="RushFrom",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class SlowIntoEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="SlowInto",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class DoubleSmoothEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="DoubleSmooth",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class ThereAndBackEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="ThereAndBack",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class LingeringEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Lingering",
    forbid_unknown_fields=True,
    frozen=True,
):
    pass


class ThereAndBackWithPauseEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="ThereAndBackWithPause",
    forbid_unknown_fields=True,
    frozen=True,
):
    pause_ratio: float


class RunningStartEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="RunningStart",
    forbid_unknown_fields=True,
    frozen=True,
):
    pull_factor: float


class OvershootEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Overshoot",
    forbid_unknown_fields=True,
    frozen=True,
):
    pull_factor: float


class WiggleEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="Wiggle",
    forbid_unknown_fields=True,
    frozen=True,
):
    wiggles: float


class ExponentialDecayEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="ExponentialDecay",
    forbid_unknown_fields=True,
    frozen=True,
):
    half_life: float


class NotQuiteThereEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="NotQuiteThere",
    forbid_unknown_fields=True,
    frozen=True,
):
    inner: Easing  # noqa: F821 — forward reference resolved below.
    proportion: float


class SquishRateFuncEasing(
    msgspec.Struct,
    tag_field="kind",
    tag="SquishRateFunc",
    forbid_unknown_fields=True,
    frozen=True,
):
    inner: Easing  # noqa: F821
    a: float
    b: float


Easing = (
    LinearEasing
    | SmoothEasing
    | RushIntoEasing
    | RushFromEasing
    | SlowIntoEasing
    | DoubleSmoothEasing
    | ThereAndBackEasing
    | LingeringEasing
    | ThereAndBackWithPauseEasing
    | RunningStartEasing
    | OvershootEasing
    | WiggleEasing
    | ExponentialDecayEasing
    | NotQuiteThereEasing
    | SquishRateFuncEasing
)


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


# ============================================================================
# === Scene ===
# ============================================================================


class Scene(msgspec.Struct, forbid_unknown_fields=True, frozen=True):
    metadata: SceneMetadata
    timeline: tuple[TimelineOp, ...]
    tracks: tuple[Track, ...]


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
