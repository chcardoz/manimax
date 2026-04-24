"""IR round-trip test — Slice C surface.

Proves the Python msgspec mirror (`manim_rs.ir`) and the Rust serde structs
(`manim-rs-ir`) agree on the JSON wire format for every variant exposed by
Slice C.

Flow:
    Python Scene
      -> msgspec.json.encode
      -> Rust serde_json::from_str (via roundtrip_ir)
      -> Rust serde_json::to_string
      -> msgspec.json.decode
      -> structural equality against the original

Covers:
    - Polyline with stroke / with fill / with both / with neither.
    - BezPath with every PathVerb variant.
    - All 15 Easing variants, including the two recursive combinators.
    - All 5 Track variants and their segment shapes.
    - Unknown-field rejection at every struct site.
"""

from __future__ import annotations

import msgspec
import pytest
from manim_rs import _rust, ir

# ---------------------------------------------------------------------------
# Fixture: a wide Scene that touches every new IR surface at least once.
# ---------------------------------------------------------------------------


def _all_easings() -> tuple[ir.Easing, ...]:
    # All float parameters are dyadic rationals (exactly representable in both
    # f32 and f64). Non-exact values like 1/3 or 0.1 round-trip as f32 → ryu
    # decimal → f64, which is a different bit pattern from the original f64 —
    # structural equality then fails even though the wire contract is fine.
    inner = ir.SmoothEasing()
    return (
        ir.LinearEasing(),
        ir.SmoothEasing(),
        ir.RushIntoEasing(),
        ir.RushFromEasing(),
        ir.SlowIntoEasing(),
        ir.DoubleSmoothEasing(),
        ir.ThereAndBackEasing(),
        ir.LingeringEasing(),
        ir.ThereAndBackWithPauseEasing(pause_ratio=0.25),
        ir.RunningStartEasing(pull_factor=-0.5),
        ir.OvershootEasing(pull_factor=1.5),
        ir.WiggleEasing(wiggles=2.0),
        ir.ExponentialDecayEasing(half_life=0.125),
        ir.NotQuiteThereEasing(inner=inner, proportion=0.75),
        ir.SquishRateFuncEasing(
            inner=ir.NotQuiteThereEasing(inner=inner, proportion=0.5),
            a=0.25,
            b=0.75,
        ),
    )


def _wide_scene() -> ir.Scene:
    """A Scene that touches every IR surface worth round-tripping."""
    polyline_stroked = ir.Polyline(
        points=(
            (-1.0, -1.0, 0.0),
            (1.0, -1.0, 0.0),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0),
        ),
        closed=True,
        stroke=ir.Stroke(color=(1.0, 1.0, 1.0, 1.0), width=0.04),
        fill=None,
    )
    polyline_filled = ir.Polyline(
        points=(
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (1.0, 1.0, 0.0),
        ),
        closed=True,
        stroke=None,
        fill=ir.Fill(color=(0.2, 0.8, 0.2, 1.0)),
    )
    bezpath = ir.BezPath(
        verbs=(
            ir.MoveToVerb(to=(0.0, 0.0, 0.0)),
            ir.LineToVerb(to=(1.0, 0.0, 0.0)),
            ir.QuadToVerb(ctrl=(1.0, 1.0, 0.0), to=(0.0, 1.0, 0.0)),
            ir.CubicToVerb(
                ctrl1=(-1.0, 1.0, 0.0),
                ctrl2=(-1.0, 0.0, 0.0),
                to=(0.0, 0.0, 0.0),
            ),
            ir.CloseVerb(),
        ),
        stroke=ir.Stroke(color=(0.5, 0.5, 0.5, 1.0), width=0.02),
        fill=ir.Fill(color=(0.1, 0.2, 0.3, 0.5)),
    )

    easings = _all_easings()

    # Position track exercises the first few easings; the remaining are
    # attached to Opacity/Rotation/Scale/Color tracks so every easing variant
    # round-trips in at least one place.
    position_segments = tuple(
        ir.PositionSegment(
            t0=float(i),
            t1=float(i) + 0.5,
            from_=(0.0, 0.0, 0.0),
            to=(float(i) + 1.0, 0.0, 0.0),
            easing=easings[i],
        )
        for i in range(5)
    )
    opacity_segments = tuple(
        ir.OpacitySegment(
            t0=float(i),
            t1=float(i) + 0.5,
            from_=0.0,
            to=1.0,
            easing=easings[5 + i],
        )
        for i in range(3)
    )
    rotation_segments = tuple(
        ir.RotationSegment(
            t0=float(i),
            t1=float(i) + 0.5,
            from_=0.0,
            to=3.14159,
            easing=easings[8 + i],
        )
        for i in range(3)
    )
    scale_segments = tuple(
        ir.ScaleSegment(
            t0=float(i),
            t1=float(i) + 0.5,
            from_=1.0,
            to=2.0,
            easing=easings[11 + i],
        )
        for i in range(2)
    )
    color_segments = (
        ir.ColorSegment(
            t0=0.0,
            t1=1.0,
            from_=(1.0, 0.0, 0.0, 1.0),
            to=(0.0, 0.0, 1.0, 1.0),
            easing=easings[13],
        ),
        ir.ColorSegment(
            t0=1.0,
            t1=2.0,
            from_=(0.0, 0.0, 1.0, 1.0),
            to=(0.0, 1.0, 0.0, 1.0),
            easing=easings[14],
        ),
    )

    return ir.Scene(
        metadata=ir.SceneMetadata(
            schema_version=ir.SCHEMA_VERSION,
            fps=30,
            duration=5.0,
            resolution=ir.Resolution(width=480, height=270),
            background=(0.0, 0.0, 0.0, 1.0),
        ),
        timeline=(
            ir.AddOp(t=0.0, id=1, object=polyline_stroked),
            ir.AddOp(t=0.0, id=2, object=polyline_filled),
            ir.AddOp(t=0.0, id=3, object=bezpath),
            ir.RemoveOp(t=5.0, id=2),
        ),
        tracks=(
            ir.PositionTrack(id=1, segments=position_segments),
            ir.OpacityTrack(id=1, segments=opacity_segments),
            ir.RotationTrack(id=1, segments=rotation_segments),
            ir.ScaleTrack(id=1, segments=scale_segments),
            ir.ColorTrack(id=1, segments=color_segments),
        ),
    )


# ---------------------------------------------------------------------------
# Happy-path round-trips.
# ---------------------------------------------------------------------------


def test_scene_roundtrips_through_rust() -> None:
    scene = _wide_scene()
    encoded = ir.encode(scene).decode("utf-8")
    rust_echoed = _rust.roundtrip_ir(encoded)
    back = ir.decode(rust_echoed)
    assert back == scene


def test_tag_fields_are_on_the_wire() -> None:
    scene = _wide_scene()
    encoded = ir.encode(scene).decode("utf-8")
    # TimelineOp discriminator.
    assert '"op":"Add"' in encoded
    assert '"op":"Remove"' in encoded
    # Object discriminator.
    assert '"kind":"Polyline"' in encoded
    assert '"kind":"BezPath"' in encoded
    # PathVerb discriminator.
    for verb in ("MoveTo", "LineTo", "QuadTo", "CubicTo", "Close"):
        assert f'"kind":"{verb}"' in encoded, verb
    # Track discriminator — every variant.
    for track in ("Position", "Opacity", "Rotation", "Scale", "Color"):
        assert f'"kind":"{track}"' in encoded, track


def test_optional_stroke_and_fill_serialize_as_null() -> None:
    """`stroke=None` / `fill=None` ⇒ JSON `null`, not the field dropped."""
    scene = ir.Scene(
        metadata=ir.SceneMetadata(
            schema_version=ir.SCHEMA_VERSION,
            fps=30,
            duration=0.0,
            resolution=ir.Resolution(width=16, height=16),
            background=(0.0, 0.0, 0.0, 1.0),
        ),
        timeline=(
            ir.AddOp(
                t=0.0,
                id=1,
                object=ir.Polyline(
                    points=((0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
                    closed=False,
                    stroke=None,
                    fill=None,
                ),
            ),
        ),
        tracks=(),
    )
    encoded = ir.encode(scene).decode("utf-8")
    assert '"stroke":null' in encoded
    assert '"fill":null' in encoded
    # And Rust accepts it — the absence must be representable on the wire.
    _rust.roundtrip_ir(encoded)


def test_ir_decode_accepts_str_and_bytes() -> None:
    """`ir.decode` advertises ``bytes | str``; both must round-trip identically."""
    scene = _wide_scene()
    as_bytes = ir.encode(scene)
    assert ir.decode(as_bytes) == scene
    assert ir.decode(as_bytes.decode("utf-8")) == scene


# ---------------------------------------------------------------------------
# Unknown-field rejection — proves `deny_unknown_fields` /
# `forbid_unknown_fields` is wired up at every struct site. If a future
# refactor drops the attribute on one struct, exactly that row fails loudly.
# ---------------------------------------------------------------------------

_UNKNOWN_FIELD_SITES = [
    # (description, needle present in wide-scene payload, replacement)
    ("metadata", '"schema_version":1', '"schema_version":1,"extra":"nope"'),
    ("add_op", '"op":"Add"', '"op":"Add","extra":"nope"'),
    ("remove_op", '"op":"Remove"', '"op":"Remove","extra":"nope"'),
    ("polyline", '"kind":"Polyline"', '"kind":"Polyline","extra":"nope"'),
    ("bezpath", '"kind":"BezPath"', '"kind":"BezPath","extra":"nope"'),
    ("moveto", '"kind":"MoveTo"', '"kind":"MoveTo","extra":"nope"'),
    ("lineto", '"kind":"LineTo"', '"kind":"LineTo","extra":"nope"'),
    ("quadto", '"kind":"QuadTo"', '"kind":"QuadTo","extra":"nope"'),
    ("cubicto", '"kind":"CubicTo"', '"kind":"CubicTo","extra":"nope"'),
    ("close_verb", '"kind":"Close"', '"kind":"Close","extra":"nope"'),
    ("stroke", '"width":0.04', '"width":0.04,"extra":"nope"'),
    ("fill", '"color":[0.2,0.8,0.2,1.0]', '"color":[0.2,0.8,0.2,1.0],"extra":"nope"'),
    ("position_track", '"kind":"Position"', '"kind":"Position","extra":"nope"'),
    ("opacity_track", '"kind":"Opacity"', '"kind":"Opacity","extra":"nope"'),
    ("rotation_track", '"kind":"Rotation"', '"kind":"Rotation","extra":"nope"'),
    ("scale_track", '"kind":"Scale"', '"kind":"Scale","extra":"nope"'),
    ("color_track", '"kind":"Color"', '"kind":"Color","extra":"nope"'),
    ("linear_easing", '"kind":"Linear"', '"kind":"Linear","extra":"nope"'),
    ("smooth_easing", '"kind":"Smooth"', '"kind":"Smooth","extra":"nope"'),
    ("wiggle_easing", '"kind":"Wiggle"', '"kind":"Wiggle","extra":"nope"'),
    (
        "squish_easing",
        '"kind":"SquishRateFunc"',
        '"kind":"SquishRateFunc","extra":"nope"',
    ),
]


@pytest.mark.parametrize("name,needle,replacement", _UNKNOWN_FIELD_SITES)
def test_unknown_field_rejected_at_every_site_python(name, needle, replacement) -> None:
    payload = ir.encode(_wide_scene()).decode("utf-8")
    assert needle in payload, f"{name}: needle {needle!r} not in payload — update fixture"
    mutated = payload.replace(needle, replacement, 1)
    with pytest.raises(msgspec.ValidationError):
        ir.decode(mutated)


@pytest.mark.parametrize("name,needle,replacement", _UNKNOWN_FIELD_SITES)
def test_unknown_field_rejected_at_every_site_rust(name, needle, replacement) -> None:
    payload = ir.encode(_wide_scene()).decode("utf-8")
    assert needle in payload, f"{name}: needle {needle!r} not in payload — update fixture"
    mutated = payload.replace(needle, replacement, 1)
    with pytest.raises(ValueError, match="IR deserialize failed"):
        _rust.roundtrip_ir(mutated)
