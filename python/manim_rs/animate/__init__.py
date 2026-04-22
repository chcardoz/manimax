"""Author-facing animations. Each emits IR tracks rather than running."""

from manim_rs import ir
from manim_rs.animate.transforms import (
    Animation,
    Colorize,
    FadeIn,
    FadeOut,
    Rotate,
    ScaleTo,
    Translate,
)

# Friendly easing aliases — ``Smooth()`` reads nicer than ``ir.SmoothEasing()``
# at scene-authoring time, and aligns with manimgl's ``rate_functions`` names.
Linear = ir.LinearEasing
Smooth = ir.SmoothEasing
RushInto = ir.RushIntoEasing
RushFrom = ir.RushFromEasing
SlowInto = ir.SlowIntoEasing
DoubleSmooth = ir.DoubleSmoothEasing
ThereAndBack = ir.ThereAndBackEasing
Lingering = ir.LingeringEasing
ThereAndBackWithPause = ir.ThereAndBackWithPauseEasing
RunningStart = ir.RunningStartEasing
Overshoot = ir.OvershootEasing
Wiggle = ir.WiggleEasing
ExponentialDecay = ir.ExponentialDecayEasing
NotQuiteThere = ir.NotQuiteThereEasing
SquishRateFunc = ir.SquishRateFuncEasing

__all__ = [
    "Animation",
    "Colorize",
    "DoubleSmooth",
    "ExponentialDecay",
    "FadeIn",
    "FadeOut",
    "Linear",
    "Lingering",
    "NotQuiteThere",
    "Overshoot",
    "Rotate",
    "RunningStart",
    "RushFrom",
    "RushInto",
    "ScaleTo",
    "SlowInto",
    "Smooth",
    "SquishRateFunc",
    "ThereAndBack",
    "ThereAndBackWithPause",
    "Translate",
    "Wiggle",
]
