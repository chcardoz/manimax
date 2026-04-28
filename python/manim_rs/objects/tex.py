"""Author-facing Tex mobject — the user-visible front door for math typesetting.

Backed by RaTeX in Rust (see `crates/manim-rs-tex`); the Python side is a thin
constructor that:

1. Inlines no-arg user macros into ``src`` (Slice E §6 gotcha #4 — the cache
   key hashes the IR's ``macros`` field, so semantically equal Tex with
   different macro maps would miss each other in the cache). The IR's
   ``macros`` field therefore always ships empty.
2. Validates the resulting source via ``_rust.tex_validate`` so a typo fails
   here, at construction time, instead of silently rendering blank.
"""

from __future__ import annotations

import re

from manim_rs import _rust, ir

# A TeX control sequence is `\` + (one or more letters) OR `\` + (single
# non-letter). For macro substitution we only care about control words
# (the letter form) — that's what `\newcommand` defines. Greedy
# `[A-Za-z]+` enforces TeX's "control word terminates at first non-letter"
# rule by always extending to the longest letter run: `\R` in `\R{n}` is
# captured as "R", but `\Real` captures "Real" — the macro name "R" never
# matches inside "Real".
_CONTROL_WORD_RE = re.compile(r"\\([A-Za-z]+)")
# Hard cap on macro-expansion iterations. A self-referential macro would
# otherwise loop forever. Real macro graphs depth-out fast (1–3 levels);
# 50 is generous and bounds the worst case.
_MAX_EXPANSION_PASSES = 50


def _expand_macros(src: str, macros: dict[str, str]) -> str:
    """Inline no-arg control-word macros in ``src``.

    Iterates to a fixed point so chained macros (``\\R`` → ``\\mathbb{R}``,
    ``\\mathbb`` itself a user macro) resolve in one call. Raises ``ValueError``
    on suspected infinite recursion.

    Macro keys are accepted with or without the leading backslash; the matcher
    works on the unbackslashed name internally.
    """
    if not macros:
        return src

    # Normalize keys: strip a leading `\` if present so `{r"\R": ...}` and
    # `{"R": ...}` both work. Reject empty / non-letter names — those would
    # not match `_CONTROL_WORD_RE` and silently no-op.
    normalized: dict[str, str] = {}
    for name, value in macros.items():
        bare = name.lstrip("\\")
        if not bare or not bare.isalpha() or not bare.isascii():
            raise ValueError(
                f"tex_macros key {name!r} must be an ASCII control word "
                f"(letters only, optional leading '\\\\')"
            )
        normalized[bare] = value

    def replace_one(match: re.Match[str]) -> str:
        name = match.group(1)
        # Enforce TeX word boundary: must not be followed by a letter.
        end = match.end()
        if end < len(current) and current[end].isalpha():
            return match.group(0)
        return normalized.get(name, match.group(0))

    current = src
    for _ in range(_MAX_EXPANSION_PASSES):
        next_pass = _CONTROL_WORD_RE.sub(replace_one, current)
        if next_pass == current:
            return current
        current = next_pass
    raise ValueError(
        f"tex_macros expansion did not converge after "
        f"{_MAX_EXPANSION_PASSES} passes — likely a self-referential macro"
    )


class Tex:
    """A LaTeX-flavored math expression.

    Compiled in Rust eval (`crates/manim-rs-eval/src/tex.rs`) into N filled
    BezPaths, one per glyph or decoration. Track-based animations on a Tex
    target apply uniformly to all sub-paths via the eval-time fan-out
    (Slice E Step 4).

    Parameters
    ----------
    src
        Tex source. Math mode by default; `$...$` is implicit.
    color
        Default fill color. Items RaTeX leaves in plain black inherit this;
        items colored explicitly with ``\\textcolor{...}`` ride through.
    scale
        Multiplier on top of the Rust adapter's ``WORLD_UNITS_PER_EM``
        (= 1.0). Composes multiplicatively with `Track::Scale` at render
        time — see Step 4 decision (b).
    tex_macros
        Optional ``{name: replacement}`` map of no-arg control-word macros.
        Inlined into ``src`` before IR emission so the cache key stays
        stable across equivalent expressions with different macro maps.
        Keys may be given as ``r"\\R"`` or ``"R"``.
    """

    __slots__ = ("_id", "src", "color", "scale")

    def __init__(
        self,
        src: str,
        *,
        color: ir.RgbaSrgb = (1.0, 1.0, 1.0, 1.0),
        scale: float = 1.0,
        tex_macros: dict[str, str] | None = None,
    ) -> None:
        if not src or not src.strip():
            raise ValueError("Tex source must not be empty")

        expanded = _expand_macros(src, tex_macros or {})

        # Validate the post-expansion source: typos in the user's source OR
        # in macro replacements both surface here. Cheaper than compile_tex
        # because it stops at the DisplayList stage.
        _rust.tex_validate(expanded)

        self._id: int | None = None
        self.src: str = expanded
        self.color: ir.RgbaSrgb = (
            float(color[0]),
            float(color[1]),
            float(color[2]),
            float(color[3]),
        )
        self.scale: float = float(scale)

    def to_ir(self) -> ir.Tex:
        # `macros` always ships empty: pre-expansion already inlined them.
        # Keeping the field on the IR (rather than dropping it post-bake)
        # preserves round-trip symmetry with the Rust schema and lets a
        # future change reintroduce macro pass-through without an IR bump.
        return ir.Tex(
            src=self.src,
            macros={},
            color=self.color,
            scale=self.scale,
        )
