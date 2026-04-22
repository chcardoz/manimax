"""IR v1 round-trip test.

Proves the Python msgspec mirror (`manim_rs.ir`) and the Rust serde structs
(`manim-rs-ir`) agree on the JSON wire format for every Slice B variant.

Flow:
    Python Scene
      -> msgspec.json.encode
      -> Rust serde_json::from_str (via roundtrip_ir)
      -> Rust serde_json::to_string
      -> msgspec.json.decode
      -> structural equality against the original
"""

from __future__ import annotations

import msgspec
import pytest

from manim_rs import _rust, ir


def _sample_scene() -> ir.Scene:
    return ir.Scene(
        metadata=ir.SceneMetadata(
            schema_version=ir.SCHEMA_VERSION,
            fps=30,
            duration=2.0,
            resolution=ir.Resolution(width=480, height=270),
            background=(0.0, 0.0, 0.0, 1.0),
        ),
        timeline=(
            ir.AddOp(
                t=0.0,
                id=1,
                object=ir.Polyline(
                    points=(
                        (-1.0, -1.0, 0.0),
                        (1.0, -1.0, 0.0),
                        (1.0, 1.0, 0.0),
                        (-1.0, 1.0, 0.0),
                    ),
                    stroke_color=(1.0, 1.0, 1.0, 1.0),
                    stroke_width=0.04,
                    closed=True,
                ),
            ),
            ir.RemoveOp(t=2.0, id=1),
        ),
        tracks=(
            ir.PositionTrack(
                id=1,
                segments=(
                    ir.PositionSegment(
                        t0=0.0,
                        t1=2.0,
                        from_=(0.0, 0.0, 0.0),
                        to=(2.0, 0.0, 0.0),
                        easing=ir.LinearEasing(),
                    ),
                ),
            ),
        ),
    )


def test_scene_roundtrips_through_rust() -> None:
    scene = _sample_scene()
    encoded = ir.encode(scene).decode("utf-8")
    rust_echoed = _rust.roundtrip_ir(encoded)
    back = ir.decode(rust_echoed)
    assert back == scene


def test_tag_fields_are_on_the_wire() -> None:
    scene = _sample_scene()
    encoded = ir.encode(scene).decode("utf-8")
    # Internal tags are the whole point of this IR revision — assert them.
    assert '"op":"Add"' in encoded
    assert '"op":"Remove"' in encoded
    assert '"kind":"Polyline"' in encoded
    assert '"kind":"Position"' in encoded
    assert '"kind":"Linear"' in encoded


def test_unknown_field_rejected_by_rust() -> None:
    bad = (
        '{"metadata":{"schema_version":1,"fps":30,"duration":2.0,'
        '"resolution":{"width":480,"height":270},'
        '"background":[0,0,0,1],"extra":"nope"},'
        '"timeline":[],"tracks":[]}'
    )
    with pytest.raises(ValueError, match="IR deserialize failed"):
        _rust.roundtrip_ir(bad)


def test_unknown_field_rejected_by_python() -> None:
    payload = ir.encode(_sample_scene()).decode("utf-8")
    # Splice in an unknown field at the metadata level.
    assert '"schema_version":1' in payload
    mutated = payload.replace(
        '"schema_version":1',
        '"schema_version":1,"extra":"nope"',
        1,
    )
    with pytest.raises(msgspec.ValidationError):
        ir.decode(mutated)


def test_ir_decode_accepts_str_and_bytes() -> None:
    """`ir.decode` advertises ``bytes | str``; both must round-trip identically."""
    scene = _sample_scene()
    as_bytes = ir.encode(scene)
    assert ir.decode(as_bytes) == scene
    assert ir.decode(as_bytes.decode("utf-8")) == scene


# The `deny_unknown_fields` / `forbid_unknown_fields` guarantee must hold at
# *every* struct site, not just metadata. Parametrize over all Slice B sites —
# if a future refactor drops the attribute on one struct, exactly that row
# fails loudly.
_UNKNOWN_FIELD_SITES = [
    # (description, search-for, replacement)
    ("metadata",  '"schema_version":1',    '"schema_version":1,"extra":"nope"'),
    ("add_op",    '"op":"Add"',            '"op":"Add","extra":"nope"'),
    ("polyline",  '"kind":"Polyline"',     '"kind":"Polyline","extra":"nope"'),
    ("position",  '"kind":"Position"',     '"kind":"Position","extra":"nope"'),
    ("segment",   '"t1":2.0',              '"t1":2.0,"extra":"nope"'),
    ("easing",    '"kind":"Linear"',       '"kind":"Linear","extra":"nope"'),
    ("remove_op", '"op":"Remove"',         '"op":"Remove","extra":"nope"'),
]


@pytest.mark.parametrize("name,needle,replacement", _UNKNOWN_FIELD_SITES)
def test_unknown_field_rejected_at_every_site_python(name, needle, replacement) -> None:
    payload = ir.encode(_sample_scene()).decode("utf-8")
    assert needle in payload, f"{name}: needle {needle!r} not in payload — update fixture"
    mutated = payload.replace(needle, replacement, 1)
    with pytest.raises(msgspec.ValidationError):
        ir.decode(mutated)


@pytest.mark.parametrize("name,needle,replacement", _UNKNOWN_FIELD_SITES)
def test_unknown_field_rejected_at_every_site_rust(name, needle, replacement) -> None:
    payload = ir.encode(_sample_scene()).decode("utf-8")
    assert needle in payload, f"{name}: needle {needle!r} not in payload — update fixture"
    mutated = payload.replace(needle, replacement, 1)
    with pytest.raises(ValueError, match="IR deserialize failed"):
        _rust.roundtrip_ir(mutated)
