"""Slice E Step 5 — Python `Tex(...)` constructor + macro pre-expansion."""

from __future__ import annotations

import pytest
from manim_rs import Tex, ir
from manim_rs.objects.tex import _expand_macros

# ----------------------------------------------------------------------------
# Macro expansion
# ----------------------------------------------------------------------------


def test_expand_macros_inlines_simple_no_arg_macro() -> None:
    out = _expand_macros(r"\R^n", {r"\R": r"\mathbb{R}"})
    assert out == r"\mathbb{R}^n"


def test_expand_macros_accepts_unprefixed_keys() -> None:
    # Both "R" and r"\R" should work as keys — convenience for authors.
    out = _expand_macros(r"\R", {"R": r"\mathbb{R}"})
    assert out == r"\mathbb{R}"


def test_expand_macros_respects_word_boundary() -> None:
    # `\Real` must NOT match the `\R` macro — TeX's "control word ends at
    # first non-letter" rule.
    out = _expand_macros(r"\R + \Real", {r"\R": r"\mathbb{R}"})
    assert out == r"\mathbb{R} + \Real"


def test_expand_macros_chains_to_fixed_point() -> None:
    # `\R` expands to `\Reals`, which itself expands to `\mathbb{R}`. One
    # call should resolve both.
    macros = {r"\R": r"\Reals", r"\Reals": r"\mathbb{R}"}
    assert _expand_macros(r"\R", macros) == r"\mathbb{R}"


def test_expand_macros_returns_input_when_empty() -> None:
    assert _expand_macros(r"x^2", {}) == r"x^2"


def test_expand_macros_rejects_self_referential_macro() -> None:
    # `\loop` → `\loop` would never converge.
    with pytest.raises(ValueError, match="did not converge"):
        _expand_macros(r"\loop", {r"\loop": r"\loop x"})


def test_expand_macros_rejects_non_letter_key() -> None:
    with pytest.raises(ValueError, match="ASCII control word"):
        _expand_macros(r"x", {"@": r"y"})


# ----------------------------------------------------------------------------
# Tex constructor — validation + IR emission
# ----------------------------------------------------------------------------


def test_tex_constructor_emits_object_tex() -> None:
    t = Tex(r"x^2 + y^2 = r^2")
    obj = t.to_ir()
    assert isinstance(obj, ir.Tex)
    assert obj.src == r"x^2 + y^2 = r^2"
    assert obj.macros == {}
    assert obj.color == (1.0, 1.0, 1.0, 1.0)
    assert obj.scale == 1.0


def test_tex_constructor_inlines_macros_into_src() -> None:
    t = Tex(r"\R^n", tex_macros={r"\R": r"\mathbb{R}"})
    obj = t.to_ir()
    assert obj.src == r"\mathbb{R}^n"
    # Critical for cache-key stability (Slice E §6 gotcha #4): macros must
    # ship empty so two Tex with the same effective LaTeX hash identically
    # regardless of how the user defined their macros.
    assert obj.macros == {}


def test_tex_constructor_validates_source_at_init() -> None:
    # `\notathing` is parseable as an unknown control sequence; pick
    # something RaTeX actually rejects: an unbalanced brace.
    with pytest.raises(ValueError, match="invalid Tex source"):
        Tex(r"\frac{a}{")


def test_tex_constructor_validates_post_expansion() -> None:
    # The raw src is fine; the macro replacement is what makes it
    # unparseable. Validation runs after expansion.
    with pytest.raises(ValueError, match="invalid Tex source"):
        Tex(r"\bad", tex_macros={r"\bad": r"\frac{a}{"})


def test_tex_constructor_rejects_empty_source() -> None:
    with pytest.raises(ValueError, match="must not be empty"):
        Tex("")
    with pytest.raises(ValueError, match="must not be empty"):
        Tex("   ")


def test_tex_color_and_scale_reach_ir() -> None:
    t = Tex(r"x", color=(0.0, 0.5, 1.0, 1.0), scale=3.5)
    obj = t.to_ir()
    assert obj.color == (0.0, 0.5, 1.0, 1.0)
    assert obj.scale == 3.5


# ----------------------------------------------------------------------------
# IR round-trip — Tex objects survive a Python → JSON → Python pass
# ----------------------------------------------------------------------------


def test_tex_round_trips_through_ir_codec() -> None:
    scene = ir.Scene(
        metadata=ir.SceneMetadata(
            schema_version=ir.SCHEMA_VERSION,
            fps=30,
            duration=1.0,
            resolution=ir.Resolution(width=64, height=64),
            background=(0.0, 0.0, 0.0, 1.0),
        ),
        timeline=(
            ir.AddOp(
                t=0.0,
                id=1,
                object=Tex(r"x^2", color=(1.0, 0.0, 0.0, 1.0), scale=2.0).to_ir(),
            ),
        ),
        tracks=(),
    )
    encoded = ir.encode(scene)
    decoded = ir.decode(encoded)
    assert decoded == scene
