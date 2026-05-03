"""Easing curves — the ``Easing`` union.

Extension axis: rate functions applied to track segments. Today: all 15
manimgl rate functions. Two are recursive combinators wrapping an inner
easing (``NotQuiteThere``, ``SquishRateFunc``). New curves (spring, custom
Bezier) land here.

Internally tagged union with discriminator "kind" — matches Rust's
``#[serde(tag = "kind")]`` on the ``Easing`` enum.
"""

from __future__ import annotations

import msgspec


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
