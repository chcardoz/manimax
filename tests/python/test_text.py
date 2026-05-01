"""Slice E Step 7 S7d/S7e — Python `Text(...)` constructor + end-to-end render.

IR wire-shape roundtrip is covered in `test_ir_roundtrip.py`. This file pins:
- S7d (constructor): defaults, every weight/align value, validation errors,
  `to_ir()` produces the right msgspec struct.
- S7e (render): `Text("HI")` survives the full Python → IR → Rust → wgpu →
  ffmpeg → frame-0 pipeline with non-trivial pixel content in the expected
  region of the canvas.

The S7e check is intentionally a behavioral pixel-count + centroid test, not
a tolerance-snapshot against a checked-in baseline PNG. The baseline-snapshot
harness is deferred to Step 6 (still TBD); when it lands, this test should
gain a stricter sibling (or get replaced) that compares against an exact PNG
under the pinned `TEX_SNAPSHOT_TOLERANCE`.
"""

from __future__ import annotations

import math
import shutil
import subprocess
from pathlib import Path

import numpy as np
import pytest
from manim_rs import Text, _rust, ir
from manim_rs.scene import Scene


def test_text_constructor_emits_object_text_with_defaults() -> None:
    t = Text("Hello")
    obj = t.to_ir()
    assert isinstance(obj, ir.Text)
    assert obj.src == "Hello"
    assert obj.font is None
    assert obj.weight == "regular"
    assert obj.size == 1.0
    assert obj.color == (1.0, 1.0, 1.0, 1.0)
    assert obj.align == "left"


def test_text_constructor_passes_through_custom_values() -> None:
    t = Text(
        "World",
        font="Inter",
        weight="bold",
        size=2.5,
        color=(0.1, 0.2, 0.3, 0.4),
        align="center",
    )
    obj = t.to_ir()
    assert obj.src == "World"
    assert obj.font == "Inter"
    assert obj.weight == "bold"
    assert obj.size == 2.5
    assert obj.color == (0.1, 0.2, 0.3, 0.4)
    assert obj.align == "center"


@pytest.mark.parametrize("weight", ["regular", "bold"])
def test_text_constructor_accepts_each_weight(weight: ir.TextWeight) -> None:
    obj = Text("x", weight=weight).to_ir()
    assert obj.weight == weight


@pytest.mark.parametrize("align", ["left", "center", "right"])
def test_text_constructor_accepts_each_align(align: ir.TextAlign) -> None:
    obj = Text("x", align=align).to_ir()
    assert obj.align == align


def test_text_constructor_rejects_empty_src() -> None:
    with pytest.raises(ValueError, match="must not be empty"):
        Text("")


def test_text_constructor_rejects_unknown_weight() -> None:
    with pytest.raises(ValueError, match="weight must be one of"):
        Text("x", weight="heavy")  # type: ignore[arg-type]


def test_text_constructor_rejects_unknown_align() -> None:
    with pytest.raises(ValueError, match="align must be one of"):
        Text("x", align="justified")  # type: ignore[arg-type]


@pytest.mark.parametrize("size", [0.0, -1.0, math.inf, math.nan])
def test_text_constructor_rejects_non_positive_or_non_finite_size(size: float) -> None:
    with pytest.raises(ValueError, match="size must be a positive finite number"):
        Text("x", size=size)


def test_text_color_coerces_iterable_to_float_tuple() -> None:
    # Lists also work — the constructor normalizes via `float(color[i])`.
    obj = Text("x", color=[0, 1, 0, 1]).to_ir()  # type: ignore[arg-type]
    assert obj.color == (0.0, 1.0, 0.0, 1.0)
    assert all(isinstance(c, float) for c in obj.color)


def test_text_size_coerces_int_to_float() -> None:
    obj = Text("x", size=3).to_ir()  # type: ignore[arg-type]
    assert obj.size == 3.0
    assert isinstance(obj.size, float)


def test_text_id_starts_unbound() -> None:
    # Mirrors Tex: the `_id` slot is reserved for Scene.add(...) wiring,
    # not set at construction time.
    t = Text("x")
    assert t._id is None


# ----------------------------------------------------------------------------
# S7e — end-to-end render through the full pipeline.
# ----------------------------------------------------------------------------


@pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="ffmpeg not on PATH")
def test_text_renders_visible_pixels_at_origin(tmp_path: Path) -> None:
    """`Text("HI")` produces non-trivial bright pixels in the expected region.

    Catches regressions anywhere in the pipeline: shaping (cosmic-text),
    eval-time fan-out (`Object::Text` → per-glyph `Object::BezPath`),
    rasterization, encoding, decoding. With `align="left"` the first glyph's
    left edge sits at world x=0 (centered horizontally → pixel 240 on a
    480-wide canvas), and the baseline sits at world y=0 (centered vertically
    → pixel 135). The bright-pixel centroid must therefore land to the right
    of the canvas center (text extends right of origin) and within a few-em
    band around the vertical center (ascenders above baseline, no descenders
    in "HI").
    """
    width, height = 480, 270
    scene = Scene(fps=15, resolution=ir.Resolution(width=width, height=height))
    scene.add(Text("HI", size=0.6))
    scene.wait(0.2)
    out = tmp_path / "text_hi.mp4"
    _rust.render_to_mp4(ir.to_builtins(scene.ir), str(out))

    raw = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(out),
            "-vframes",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    assert len(raw) == width * height * 4, f"unexpected frame size: {len(raw)}"

    arr = np.frombuffer(raw, dtype=np.uint8).reshape(height, width, 4)
    mask = (arr[..., 0] > 40) | (arr[..., 1] > 40) | (arr[..., 2] > 40)
    ys, xs = np.nonzero(mask)
    n = xs.size
    # Two glyphs at size=0.6 on a 480×270 canvas land around ~100 lit
    # pixels (size=0.6 em maps to ~50 px tall on this canvas). A blank
    # frame or a hard clip would drop below this floor; a runaway shape
    # would blow well past it. The exact count drifts with antialiasing
    # + x264 CRF rounding, so we keep the bound loose at both ends.
    assert n > 50, f"too few bright pixels for 'HI': {n}"
    assert n < 5000, f"unreasonable bright pixel count for 'HI': {n}"

    cx, cy = float(xs.mean()), float(ys.mean())
    # `align="left"` at origin → glyphs extend right of pixel 240. Centroid
    # must sit to the right of canvas center (allow a small margin for the
    # 'H' bowl extending leftward into x ≈ 240).
    assert cx > width / 2 - 5, f"expected left-aligned text right of center; cx={cx:.1f}"
    assert cx < width * 0.85, f"text extending past sane horizontal extent; cx={cx:.1f}"
    # No descenders in "HI"; baseline at world y=0 ⇒ pixel y ≈ 135 with
    # ascenders sitting at lower pixel y. Centroid lands above the baseline,
    # i.e. at pixel y < 135. Bound the upper drift to one em-ish.
    baseline_y = height / 2
    assert (
        cy < baseline_y
    ), f"ascenders should sit above baseline; cy={cy:.1f} ≥ baseline {baseline_y:.1f}"
    assert cy > baseline_y - 80, f"text drifting unreasonably far above baseline; cy={cy:.1f}"
