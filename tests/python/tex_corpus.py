"""Tex coverage corpus for Slice E Step 6.

Pure data. The test harness (`test_tex_corpus.py`, next step) parametrizes
over `CORPUS`, renders each entry via `render_frame_to_png`, and
tolerance-checks the rgba against a baseline PNG in
`tests/python/snapshots/tex/<name>.png`.

Entries are picked by **distinct rendering machinery**, not by visual
variety. Each one should fail differently when something specific breaks:
duplicates without a distinct mechanism are dead weight, so resist the
temptation to add a second `\\frac` "for variety."

Coverage targets:
- every bundled font face (math-italic, blackboard, calligraphic, fraktur)
- every `DisplayItem` variant RaTeX emits (GlyphPath, Line, Rect, Path)
- every `PathCommand` variant the adapter translates (MoveTo, LineTo,
  CubicTo; QuadTo is rare in KaTeX fonts — re-evaluate after first run)
- every visual bug Slice E already paid for, pinned by an explicit entry
  citing its ADR/gotcha
- a few cross-platform skew probes (anti-aliased thin strokes, tiny
  glyphs, the largest stretched delimiter) chosen to be hardest to fit
  under one tolerance — if `TEX_SNAPSHOT_TOLERANCE` passes these on both
  macOS-arm64 and Linux/lavapipe, it'll pass everything.

Out of scope (do NOT add): `\\newcommand` with arguments, `\\tikz`, auto
equation numbering, anything requiring a system LaTeX install or a
non-bundled font. Those would either fail forever (training the harness
to ignore failures) or test out-of-scope features.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class CorpusEntry:
    name: str  # snapshot filename stem; pytest test id
    src: str  # LaTeX source
    category: str  # human grouping; not load-bearing
    scale: float = 1.0
    notes: str = ""  # why this entry is in the corpus


CORPUS: tuple[CorpusEntry, ...] = (
    # ------------------------------------------------------------------
    # Glyph runs / font-face coverage
    # ------------------------------------------------------------------
    CorpusEntry(
        name="greek_lower",
        src=r"\alpha \beta \gamma \delta \epsilon \zeta \eta \theta",
        category="glyph_runs",
        notes="lowercase Greek; math-italic font face",
    ),
    CorpusEntry(
        name="greek_upper",
        src=r"\Gamma \Delta \Theta \Lambda \Xi \Pi \Sigma \Phi \Psi \Omega",
        category="glyph_runs",
        notes="uppercase Greek; regular math font face",
    ),
    CorpusEntry(
        name="bb",
        src=r"\mathbb{R} \mathbb{Z} \mathbb{N} \mathbb{Q} \mathbb{C}",
        category="glyph_runs",
        notes=(
            "blackboard-bold; double-strike interior contours catch winding "
            "regressions (the y-flip / NonZero misdiagnosis from Step 5)"
        ),
    ),
    CorpusEntry(
        name="cal",
        src=r"\mathcal{L} \mathcal{F} \mathcal{O}",
        category="glyph_runs",
        notes="calligraphic font face",
    ),
    CorpusEntry(
        name="frak",
        src=r"\mathfrak{g} \mathfrak{h} \mathfrak{sl}_2",
        category="glyph_runs",
        notes="fraktur font face",
    ),
    # ------------------------------------------------------------------
    # Sub/superscripts
    # ------------------------------------------------------------------
    CorpusEntry(
        name="super_simple",
        src=r"x^2 + y^2 = z^2",
        category="scripts",
        notes="basic superscript; script-positioning math",
    ),
    CorpusEntry(
        name="sub_simple",
        src=r"a_1 + a_2 + a_3",
        category="scripts",
        notes="basic subscript",
    ),
    CorpusEntry(
        name="sub_super",
        src=r"x_i^2",
        category="scripts",
        notes="sub + super on one base",
    ),
    CorpusEntry(
        name="nested_subs",
        src=r"x_{i_j}",
        category="scripts",
        notes=(
            "tiny-glyph stress; cross-platform skew probe (anti-aliasing "
            "of small glyphs differs between macOS-arm64 and lavapipe)"
        ),
    ),
    # ------------------------------------------------------------------
    # Vertical stacking
    # ------------------------------------------------------------------
    CorpusEntry(
        name="frac_simple",
        src=r"\frac{a}{b}",
        category="stacking",
        notes="fraction bar emits DisplayItem::Line; vbox layout",
    ),
    CorpusEntry(
        name="frac_nested",
        src=r"\frac{1}{1 + \frac{1}{x}}",
        category="stacking",
        notes="recursive vbox layout (cuttable if CI runtime tightens — "
        "overlaps quadratic_formula mechanically)",
    ),
    CorpusEntry(
        name="binom",
        src=r"\binom{n}{k}",
        category="stacking",
        notes="vstack without the bar",
    ),
    CorpusEntry(
        name="cases",
        src=r"f(x) = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}",
        category="stacking",
        notes="\\begin{cases} + embedded text/math branches",
    ),
    # ------------------------------------------------------------------
    # Large operators with limits
    # ------------------------------------------------------------------
    CorpusEntry(
        name="sum",
        src=r"\sum_{i=1}^n i = \frac{n(n+1)}{2}",
        category="big_ops",
        notes="README example; sums + frac in one expression",
    ),
    CorpusEntry(
        name="int",
        src=r"\int_a^b f(x) \, dx",
        category="big_ops",
        notes="limits on \\int + thin-space \\,",
    ),
    CorpusEntry(
        name="lim",
        src=r"\lim_{x \to 0} \frac{\sin x}{x} = 1",
        category="big_ops",
        notes="\\lim underset + \\to arrow",
    ),
    # ------------------------------------------------------------------
    # Roots / radicals
    # ------------------------------------------------------------------
    CorpusEntry(
        name="sqrt_simple",
        src=r"\sqrt{x + 1}",
        category="radicals",
        notes=(
            "radical bar emits DisplayItem::Path; lyon-flatness regression "
            "pin (ADR 0008 §D, FILL_TOLERANCE = 0.001)"
        ),
    ),
    CorpusEntry(
        name="sqrt_nested",
        src=r"\sqrt{1 + \sqrt{1 + x}}",
        category="radicals",
        notes="radical-inside-radical; thin-stroke cross-platform probe",
    ),
    # ------------------------------------------------------------------
    # Stretched delimiters (\left ... \right)
    # ------------------------------------------------------------------
    CorpusEntry(
        name="left_paren_frac",
        src=r"\left( \frac{a}{b} \right)",
        category="delimiters",
        notes="\\left/\\right delimiter-size selection",
    ),
    CorpusEntry(
        name="norm",
        src=r"\left\| \mathbf{v} \right\|",
        category="delimiters",
        notes="escaped-pipe delimiter; distinct glyph variant",
    ),
    CorpusEntry(
        name="left_bracket_matrix",
        src=r"\left[ \begin{matrix} a & b \\ c & d \end{matrix} \right]",
        category="delimiters",
        notes=(
            "delimiter around matrix; largest stretched-delimiter case in "
            "the corpus; cross-platform skew probe"
        ),
    ),
    # ------------------------------------------------------------------
    # Matrices
    # ------------------------------------------------------------------
    CorpusEntry(
        name="pmatrix",
        src=r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}",
        category="matrices",
        notes="parens-delimited matrix",
    ),
    CorpusEntry(
        name="bmatrix",
        src=r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}",
        category="matrices",
        notes="brackets-delimited matrix (same layout, different delimiter)",
    ),
    CorpusEntry(
        name="vmatrix",
        src=r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
        category="matrices",
        notes="bars; determinant-style",
    ),
    # ------------------------------------------------------------------
    # Aligned environment
    # ------------------------------------------------------------------
    CorpusEntry(
        name="aligned",
        src=(
            r"\begin{aligned} (a+b)^2 &= a^2 + 2ab + b^2 \\ "
            r"(a-b)^2 &= a^2 - 2ab + b^2 \end{aligned}"
        ),
        category="environments",
        notes=(
            "coordinate-system-flip regression pin (gotcha §6.1); upside-"
            "down rows would be the obvious symptom"
        ),
    ),
    # ------------------------------------------------------------------
    # Accents
    # ------------------------------------------------------------------
    CorpusEntry(
        name="accents_hat_tilde",
        src=r"\hat{x} + \tilde{y}",
        category="accents",
        notes="accent-over-glyph positioning",
    ),
    CorpusEntry(
        name="bar_vec",
        src=r"\bar{z} \cdot \vec{v}",
        category="accents",
        notes="bar + vec; different accent class than hat/tilde",
    ),
    CorpusEntry(
        name="dot_ddot",
        src=r"\dot{x} + \ddot{y}",
        category="accents",
        notes="single vs. double dot accents (cuttable if CI tightens — "
        "overlaps accents_hat_tilde / bar_vec mechanically)",
    ),
    # ------------------------------------------------------------------
    # Other big operators
    # ------------------------------------------------------------------
    CorpusEntry(
        name="prod",
        src=r"\prod_{k=1}^n k = n!",
        category="big_ops",
        notes="\\prod glyph; distinct from \\sum (cuttable if CI tightens)",
    ),
    # ------------------------------------------------------------------
    # Spacing primitives
    # ------------------------------------------------------------------
    CorpusEntry(
        name="spacing",
        src=r"a \, b \: c \; d \quad e \qquad f",
        category="spacing",
        notes="every spacing primitive on one line; only catcher of "
        "kerning/space-token regressions",
    ),
    # ------------------------------------------------------------------
    # Embedded text inside math
    # ------------------------------------------------------------------
    CorpusEntry(
        name="text_in_math",
        src=r"x \in \mathbb{R} \text{ such that } x > 0",
        category="text_boundary",
        notes="math-font ↔ text-font boundary inside one expression",
    ),
    # ------------------------------------------------------------------
    # Color (per-DisplayItem override)
    # ------------------------------------------------------------------
    CorpusEntry(
        name="textcolor",
        src=r"\textcolor{red}{x} + y",
        category="color",
        notes=(
            "\\textcolor sets per-DisplayItem color; interaction with the "
            "Tex(color=...) default is gotcha §6.10"
        ),
    ),
    # ------------------------------------------------------------------
    # Mixed visual stress / broad regression sentinels
    # ------------------------------------------------------------------
    CorpusEntry(
        name="quadratic_formula",
        src=r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
        category="mixed",
        notes=(
            "frac + sqrt + super; classic shape — should look exactly "
            "right or the slice has failed"
        ),
    ),
    CorpusEntry(
        name="taylor",
        src=(r"f(x) = \sum_{n=0}^\infty \frac{f^{(n)}(a)}{n!} (x-a)^n"),
        category="mixed",
        notes=(
            "sum + frac + super + parens; dense; exposes spacing drift "
            "across composed primitives"
        ),
    ),
    # Note: same source as `sub_simple`, intentionally — this entry pins
    # a *render-scale* regression, not a source variant. Do not deduplicate.
    CorpusEntry(
        name="large_zoom_subscript",
        src=r"a_1",
        category="mixed",
        scale=8.0,
        notes=(
            "swash low-ppem hinting regression pin (ADR 0008 §C, "
            "OUTLINE_PPEM = 1024). Same src as `sub_simple` but rendered "
            "at scale=8 where the staircase artifact appeared before the "
            "fix. Distinct snapshot, not a duplicate expression."
        ),
    ),
)


# Sanity check: corpus names must be unique (used as filenames + test ids).
assert len({entry.name for entry in CORPUS}) == len(
    CORPUS
), "duplicate corpus entry name; snapshot filenames would collide"
