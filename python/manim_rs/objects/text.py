"""Author-facing Text mobject — plain-text label, shaped via cosmic-text in Rust eval.

Mirrors `Tex` in shape: this module is the user-visible front door, while the
heavy lifting (shaping + glyph outline + caching) happens in
`crates/manim-rs-eval/src/text.rs` at eval time.

Unlike `Tex`, there is no Python-side validator round-trip — cosmic-text accepts
any UTF-8 source. Validation here is limited to argument shape (non-empty src,
known weight/align literals, positive finite size). Coverage gaps for unbundled
fonts and bold synthesis are documented in `crates/manim-rs-text/src/cosmic.rs`.
"""

from __future__ import annotations

import math
from typing import get_args

from manim_rs import ir
from manim_rs.objects._coerce import rgba as _rgba

_VALID_WEIGHTS = get_args(ir.TextWeight)
_VALID_ALIGNS = get_args(ir.TextAlign)


class Text:
    """A shaped plain-text label.

    Compiled in Rust eval (`crates/manim-rs-eval/src/text.rs`) into N filled
    BezPaths, one per glyph. Track-based animations on a Text target apply
    uniformly to all sub-paths via the eval-time fan-out.

    Parameters
    ----------
    src
        Text source. Any UTF-8 string. Newlines (`'\\n'`) start new lines;
        wrapping is disabled (Slice E §4 declares justification + complex
        line-breaking out of scope — break lines explicitly).
    font
        Family name to shape with. ``None`` (default) ⇒ bundled Inter Regular.
        Non-``None`` values are reserved for user-registered families
        (S7c/S7f) and currently ignored by the shaper.
    weight
        ``"regular"`` or ``"bold"``. With only Inter Regular bundled, ``"bold"``
        falls back to synthesized bold or to Regular — see
        `crates/manim-rs-text/src/cosmic.rs` for the documented gap.
    size
        Em size in world units. ``size = 1.0`` ⇒ one em equals one world unit
        (matches ``Tex``'s ``WORLD_UNITS_PER_EM`` at ``scale = 1.0``). Must be
        positive and finite.
    color
        Fill color, applied uniformly to every glyph. No ``\\textcolor``-style
        per-item override (yet) — the Rust path recolors every glyph at
        compile time.
    align
        ``"left"``, ``"center"``, or ``"right"``. ``"justified"`` deliberately
        omitted (Slice E §4).
    """

    __slots__ = ("_id", "src", "font", "weight", "size", "color", "align")

    def __init__(
        self,
        src: str,
        *,
        font: str | None = None,
        weight: ir.TextWeight = "regular",
        size: float = 1.0,
        color: ir.RgbaSrgb = (1.0, 1.0, 1.0, 1.0),
        align: ir.TextAlign = "left",
    ) -> None:
        if not src:
            raise ValueError("Text source must not be empty")
        if weight not in _VALID_WEIGHTS:
            raise ValueError(f"Text weight must be one of {_VALID_WEIGHTS}, got {weight!r}")
        if align not in _VALID_ALIGNS:
            raise ValueError(f"Text align must be one of {_VALID_ALIGNS}, got {align!r}")
        size_f = float(size)
        if not math.isfinite(size_f) or size_f <= 0.0:
            raise ValueError(f"Text size must be a positive finite number, got {size!r}")

        self._id: int | None = None
        self.src: str = src
        self.font: str | None = font
        self.weight: ir.TextWeight = weight
        self.size: float = size_f
        self.color: ir.RgbaSrgb = _rgba(color)
        self.align: ir.TextAlign = align

    def to_ir(self) -> ir.Text:
        return ir.Text(
            src=self.src,
            font=self.font,
            weight=self.weight,
            size=self.size,
            color=self.color,
            align=self.align,
        )
