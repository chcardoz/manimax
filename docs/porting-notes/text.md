# Porting note: Text

**Status:** Slice E shipped (Steps 1–8).
**Manimgl reference:** `reference/manimgl/manimlib/mobject/svg/text_mobject.py`
at submodule HEAD as of Slice E. ManimGL renders text via Pango/Cairo
and parses the output back into an `SVGMobject`.
**Manimax port:** `crates/manim-rs-text/src/cosmic.rs` +
`python/manim_rs/objects/text.py`. ADR 0012.

Like Tex, Text is a **reimplementation, not a port**. ManimGL's path is
Pango shape → Cairo render → SVG → parse. Manimax's path is cosmic-
text shape → swash outlines → kurbo BezPath → fill. The user-facing
constructor follows manimgl's keyword shape (`size`, `color`, `font`,
etc.) but the rendering pipeline shares nothing.

This note captures the invariants that aren't obvious from cosmic.rs
or text.py — alignment semantics, line-height conventions, baseline
anchoring, and the gaps vs. manimgl that aren't caused by Pango/cosmic
divergence.

## What we kept from manimgl

- **Constructor shape.** `Text(src, *, font=None, weight=..., size=...,
  color=..., align=...)`. Same field names, same default
  `font=None → bundled default`. Color is RGBA, matching the rest of
  the IR.
- **`align` keyword as a string.** "left" / "center" / "right" — same
  vocabulary manimgl exposes (we omit "justified", see below).
- **Default font is "the bundled one."** Manimgl doesn't bundle but
  it does pick a sane default per platform; Manimax has one bundled
  font (Inter Regular) and uses it when `font=None`.

## What we explicitly didn't keep

- **Pango.** No system dep, no C linker, no per-platform font scan.
  cosmic-text covers shaping; swash covers outlines.
- **Justification.** `Align::Justified` is supported by cosmic-text;
  we do not expose it via `TextAlign`. Slice E §4 declared
  justification out of scope; advertising it would imply visual
  verification we don't do. Adding it later is a one-variant
  enum extension on both sides.
- **System-font discovery.** fontdb supports `load_system_fonts`. We
  load only `Inter-Regular.ttf` from `crates/manim-rs-text/fonts/`.
  Same scene renders identically on every host. ADR 0012.
- **Multi-weight bundle.** Inter Regular only. Bold / italic require
  the user to supply font bytes via `Text(..., font="path/to.ttf")`
  — the API has the parameter (S7b on the IR side, S7d on the
  Python side) but Slice E doesn't ship a bold face inside the
  wheel.
- **Rich-text inline styling.** `TexTextMobject`-style runs of mixed
  fonts/weights/sizes inside one `Text` call are not supported.
  Compose with multiple `Text` mobjects positioned next to each other
  if needed; or use `Tex(... \text{...} ...)` for math-adjacent
  text styling.

## The cosmic-text contract (what S7b actually wires up)

Three layout facts that aren't obvious until you read the spec:

1. **Layout runs at `SHAPE_PPEM = 1024`, not at the user's `size`.**
   Same hinting workaround as ADR 0008 §C — swash applies hinting
   at low ppem, and "1 em = 1 world unit" gives a ppem near 1.0.
   The result is post-multiplied by `size / SHAPE_PPEM` to land
   in caller units. The pre-multiplied path is hinting-immune.
2. **Baseline at world `y = 0` for the first line.** cosmic-text
   emits y-down with each layout run carrying a `line_y` at that
   line's baseline. We anchor by subtracting the first run's
   `line_y`, so the first line's baseline lands exactly at world
   y = 0; ascenders are positive y, descenders negative y, and
   subsequent lines stack downward (more negative y).
3. **Sub-pixel positioning.** cosmic-text's
   `LayoutGlyph::physical()` rounds glyph positions to pixels for
   raster targets. We bypass it because Manimax renders to vectors
   — `glyph.x_offset` / `glyph.y_offset` come through directly.
   Lossless at any zoom level downstream.

`Wrap::None` is hard-coded. Slice E §4 declared automatic line
breaking out of scope; the user can break with `\n` (cosmic-text
honors `LineEnding`s in the input). Adding `Wrap::Word` later is
an enum + buffer-bounds change.

## Alignment semantics (what `TextAlign` means)

cosmic-text's `Align` operates per-line within the buffer's bounds.
We pass `Some(f32::INFINITY)` for buffer width with `Wrap::None`,
which means *the alignment knob has no visible effect on a single-
line input*. It only matters when:

- The source contains explicit `\n`, producing multiple layout runs,
  and
- The layout runs have different natural widths,

at which point cosmic-text positions each run within the bounding
box of the longest run according to `align`. Test pin in
`tests/python/test_text.py` covers single-line inputs (where align is
parametrized for API surface coverage but doesn't affect layout) and
trusts cosmic-text for the multi-line case.

If you ever expose `Wrap::Word` and a configurable `width`, alignment
becomes load-bearing — at that point write a real visual test.

## Line height

`LINE_HEIGHT_FACTOR = 1.2`, hard-coded. Matches typographic
convention used by web browsers, most editors, and Pango defaults.
Surface as a knob if a future caller needs override. The Metrics
constructor takes `(font_size, line_height)` so the override would
be a single field on the IR — no shape change to cosmic.rs.

## What's not bundled (the coverage gaps)

- **RTL / Indic shaping.** cosmic-text supports them via rustybuzz,
  but we ship Inter Regular which has no RTL coverage. A user
  passing Arabic text will get tofu glyphs unless they supply
  `font="path/to/NotoSansArabic.ttf"`. Not tested, not promised
  to work end-to-end at correct visual fidelity even with a
  custom font — Slice E §4 explicitly out of scope.
- **Bidi.** Same issue. cosmic-text handles bidi internally, but
  the bundled font can't render the relevant codepoints.
- **Color emoji.** swash returns monochrome outlines. COLR/CPAL or
  bitmap emoji tables are not consumed. Out of scope.
- **Variable fonts axes.** cosmic-text resolves font weight via the
  fontdb attributes; specifying a non-Regular weight when only
  Regular is registered triggers cosmic-text's nearest-match
  fallback, which on `fontdb` 0.18 returns Regular and may
  synthesize bold (artificial stem widening). Documented, not
  tested.

## Cache discipline

Same shape as Tex (`compile_text` in
`crates/manim-rs-eval/src/text.rs`, cache in
`crates/manim-rs-eval/src/evaluator.rs`):

- Cache key is `(src, font, weight, size, color, align)` — only
  the inputs that shape the output. Per-instance transforms
  (position, rotation, opacity) are applied at the eval-time
  fan-out site, never reach `compile_text`, never participate in
  the key.
- Per-`Evaluator`. Dies with the Evaluator. No process-global
  cache; tests are cheap to isolate.
- Mutex+Arc, double-checked under lock for cold-miss races.
- ADR 0009 explicitly carves out future glyph caches as legitimate
  source-keyed in-memory caches. This is one of them.

When adding a field to `Object::Text`: ask "does this change the
shaped geometry?" If yes, add to the key. If no — leave it out.
Slice E learned this the hard way for Tex (slice plan §11
post-Step-5 cleanup) and got it right the first time for Text.

## Files touched

- `crates/manim-rs-text/src/cosmic.rs` — shaping + layout adapter.
- `crates/manim-rs-text/src/glyph.rs` — swash outline extraction
  by glyph id (bypasses charmap when cosmic-text already resolved).
- `crates/manim-rs-text/src/font.rs` — bundled Inter Regular.
- `crates/manim-rs-text/fonts/Inter-Regular.ttf` — the font itself.
  License: `Inter-LICENSE.txt` (OFL-1.1) sits next to it.
- `crates/manim-rs-ir/src/lib.rs` — `Object::Text`, `TextWeight`,
  `TextAlign`. SCHEMA_VERSION 3.
- `crates/manim-rs-eval/src/text.rs` — `compile_text` + color
  override.
- `crates/manim-rs-eval/src/evaluator.rs` — Text fan-out + cache.
- `python/manim_rs/objects/text.py` — Python `Text` constructor.
- `python/manim_rs/ir.py` — msgspec mirrors of `Text` /
  `TextWeight` / `TextAlign`.
