# Tex coverage

**Status:** Slice E (Steps 1–5 shipped). Authoritative subset reference for what `Tex(src)` can render.
**Pairs with:** `docs/decisions/0008-slice-e-decisions.md`, `docs/porting-notes/tex.md` (TODO Step 9), `tests/python/tex_corpus.py`.

Manimax's `Tex` runs on **RaTeX** (pure Rust, KaTeX-grammar subset). It is not full LaTeX. There is no `\usepackage`, no TikZ, no `chemfig`, no system `latex` install. The wheel ships KaTeX TTFs and that's the entire math typography stack.

This document is the contract: what works, what doesn't, what looks subtly different from manimgl, and how to escape the subset when you need full LaTeX fidelity.

## What works

Grouped by rendering machinery (matches `tests/python/tex_corpus.py` categories — the corpus is the executable form of this list).

### Glyph runs, all bundled font faces

- **Math italic** (default for variables): `x`, `y`, `f`, etc. Lowercase Greek (`\alpha \beta \gamma ...`) renders here.
- **Math regular** (uppercase Greek operators): `\Gamma \Delta \Theta \Lambda \Xi \Pi \Sigma \Phi \Psi \Omega`.
- **Blackboard bold**: `\mathbb{R} \mathbb{Z} \mathbb{N} \mathbb{Q} \mathbb{C}`. Double-strike interior contours render with NonZero winding (no holes, no spurious infill).
- **Calligraphic**: `\mathcal{L} \mathcal{F} \mathcal{O}`.
- **Fraktur**: `\mathfrak{g} \mathfrak{h}`. Multi-character arguments work (`\mathfrak{sl}_2`).
- **Bold math**: `\mathbf{v}`. Used in vector notation; `\bm{...}` is **not** supported (out-of-scope KaTeX extension).

### Sub/superscripts

`x^2`, `a_1`, `x_i^2`, arbitrary nesting (`x_{i_j}`). Limits on big operators bind correctly.

### Vertical stacking

- `\frac{a}{b}`, nested `\frac{1}{1 + \frac{1}{x}}`. Fraction bar emits `DisplayItem::Line`.
- `\binom{n}{k}` (vstack without bar).
- `\begin{cases} ... \end{cases}`.

### Large operators with limits

`\sum_{i=1}^n`, `\int_a^b`, `\prod_{k=1}^n`, `\lim_{x \to 0}`. `\to` arrow, `\infty`, `!` factorial. Thin-spaces (`\,`) compose normally.

### Roots / radicals

`\sqrt{...}`, nested radicals. Radical bar emits `DisplayItem::Path`; tessellated through the same fill pipeline as any BezPath, at `FILL_TOLERANCE = 0.001` (ADR 0008 §D).

### Stretched delimiters (`\left ... \right`)

`\left(`, `\left\|`, `\left[`, `\left\langle`. Delimiter sizes selected from KaTeX's stretch-glyph variants. Tested with delimiters around fractions and matrices.

### Matrices

`\begin{pmatrix}`, `\begin{bmatrix}`, `\begin{vmatrix}`, `\begin{matrix}`. `&` column separator, `\\` row separator. Composes inside `\left ... \right` for non-default delimiters.

### `aligned` environment

`\begin{aligned} ... &= ... \\ ... &= ... \end{aligned}`. Multi-line equation alignment around `&`.

### Accents

`\hat`, `\tilde`, `\bar`, `\vec`, `\dot`, `\ddot`. Positioning is glyph-relative and respects accent-class metrics from the KaTeX font metadata.

### Spacing

All five spacing primitives: `\,` (thin), `\:` (medium), `\;` (thick), `\quad`, `\qquad`. They compose; consecutive spaces accumulate.

### Embedded text inside math

`\text{...}` switches from math-italic to the text font for the enclosed content. Math contained inside `\text{...}` is **not** re-entered (KaTeX-subset limitation).

### Color

- Top-level via `Tex(src, color=(r,g,b,a))` — applied uniformly to every glyph and path.
- Per-item via `\textcolor{red}{...}` — overrides the top-level color for the wrapped glyphs only. Standard CSS color names work; arbitrary hex (`{#ff8800}`) does **not** (KaTeX-subset limitation).

### Macro pre-expansion

`Tex(src, tex_macros={r"\R": r"\mathbb{R}"})` does Python-side string substitution before RaTeX sees the source. **No-arg macros only** — `\norm{x}` style macros with arguments are out of scope (see "Not supported" below).

## Not supported

These are explicit boundaries, not bugs. Using them raises `ValueError: invalid Tex source` from the `Tex(...)` constructor.

| Feature | Status | Workaround |
|---|---|---|
| `\usepackage{...}` | Out — RaTeX has no package system | `engine="latex"` (future) |
| TikZ / PGF | Out | `engine="latex"` (future) or render externally and import as SVG (Slice E.5+) |
| `chemfig`, `mhchem` | Out | `engine="latex"` (future) |
| `\newcommand` with arguments | Out | Define the expansion in Python and pass via `tex_macros={}` (no-arg only) |
| Auto equation numbering (`align`'s `(1)`, `(2)`) | Out — RaTeX requires explicit `\tag` | Use `\tag{1}` per equation |
| `\bm{...}` (bold-math extension) | Out | `\mathbf{...}` for upright bold; no italic-bold |
| Hex colors `\textcolor{#ff8800}{...}` | Out — RaTeX accepts named colors only | Pass top-level `Tex(color=(r,g,b,a))` for non-named colors |
| Non-Latin scripts (CJK, Devanagari, Arabic) inside `\text{...}` | Out — bundled fonts are KaTeX + Inter Latin | Use `Text(...)` for non-Latin instead, position separately |
| Custom math fonts | Out — bundled KaTeX TTFs only | Drop the request; `engine="latex"` won't help either (KaTeX uses its own fonts) |
| Nested `\text{... math ...}` re-entry into math mode | Out (KaTeX limitation) | Break the expression into separate `Tex` calls |
| `\input{...}` / file inclusion | Out | Build the string in Python |

## Known visible deltas vs. manimgl

ManimGL renders Tex via the system LaTeX install (`latex` → `dvisvgm` → SVG). Manimax renders via RaTeX directly to glyph outlines. Even when a corpus expression looks "the same" between the two, sub-pixel differences are guaranteed. Documented deltas:

- **Font.** ManimGL uses Computer Modern (LaTeX default); Manimax uses KaTeX's bundled fonts (`Main-Regular`, `Math-Italic`, `AMS-Regular`, etc.). Letterforms are similar but not identical — `\mathcal` and `\mathfrak` are the most visibly different faces.
- **Spacing.** KaTeX's spacing tables are not bit-for-bit equal to TeX's. Wide expressions can drift several em-units in horizontal extent vs. a manimgl render of the same source.
- **Stretched delimiter sizing.** KaTeX picks delimiter glyph variants from a discrete table; LaTeX synthesizes from extension pieces. Manimax's `\left( \frac{a}{b} \right)` may pick a slightly larger or smaller paren than manimgl's.
- **Accent positioning.** Accent placement uses KaTeX's metadata, not TeX's `\skew` machinery. Most glyphs match closely; ascender-heavy glyphs (`\hat{f}`) can drift by a fraction of an em.
- **Color semantics.** `\textcolor{red}{x}` overrides per-glyph color in Manimax (per `DisplayItem::color`); manimgl applies the same color but resolves it through its `set_color` machinery — the rendered alpha may differ if the user has set non-1.0 opacity on the parent `Tex`.

These deltas are deliberate (the price of pure Rust). They are **not** regressions and should not be tuned out of snapshot tests via tolerance — the snapshot baselines are Manimax's rendering, not manimgl's.

## Future full-fidelity escape hatch: `engine="latex"`

**Out of scope for Slice E.** Documented here so users know it's coming.

When a user hits a coverage gap they cannot work around (TikZ, an unusual package, exotic notation), the planned escape is `Tex(src, engine="latex")` — falls back to a system `latex` + `dvisvgm --no-fonts` pipeline (mirrors manim-community). Same `MObjectKind::Tex` IR variant, different compile path on the Rust side. The trade-off is explicit: zero-install becomes "requires system LaTeX," but you get the full LaTeX universe.

We are **not** embedding Tectonic. Larger binary, C-build pain, first-run network fetch — and `engine="latex"` covers the same use case more cleanly.

The opt-in lands when a real user hits a wall, not on speculation.

## Extending the corpus

If you need a Tex feature not in the corpus and it's in the supported subset:

1. Add an entry to `tests/python/tex_corpus.py`. Pick by *distinct rendering machinery*, not by visual variety — duplicates without a distinct mechanism are dead weight. The `notes` field cites which `DisplayItem` variant or layout pass the entry exercises.
2. If the entry exercises a new failure mode (a bug a corpus run caught), tag it as a regression pin in `notes` with the ADR/gotcha citation. Future readers should know why it's there.
3. Re-baseline (when the harness lands) and verify cross-platform tolerance still holds.

If the feature is **not** in the supported subset, the `Tex(...)` constructor will raise `ValueError`. Add a row to "Not supported" in this document with the workaround. Do not silently expand the corpus to test the unsupported path — that trains the harness to ignore failures.

## Snapshot tolerance

The snapshot harness for the corpus is deferred (Slice E §STATUS, 2026-04-29). When it lands, this section will document:

- The pinned `TEX_SNAPSHOT_TOLERANCE` constant (max channel delta + max % pixels exceeding).
- The values' cross-platform rationale (macOS-arm64 dev vs. Linux/lavapipe CI; ADR 0007).
- The audit trail for any change to those values.

Until then: anyone re-rendering corpus expressions for visual review can use `python -m manim_rs frame ...` (the single-frame API from ADR 0008 §F) to produce a PNG and eyeball it against the source.

## See also

- `docs/decisions/0008-slice-e-decisions.md` — design calls behind Tex (fan-out at eval time, per-Evaluator cache, hinting at high ppem, fill tolerance pinning).
- `docs/decisions/0009-remove-pixel-cache.md` — why corpus snapshots are a *baseline* check, not a *cache hit* check.
- `tests/python/tex_corpus.py` — the executable form of the supported subset.
- `reference/manimgl/manimlib/tex/` — what the source manimgl pipeline does.
