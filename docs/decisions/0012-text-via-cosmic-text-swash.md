# 0012 — Text via cosmic-text + swash, with bundled Inter Regular

**Date:** 2026-04-30
**Status:** accepted

Slice E originally numbered this ADR `0009`; the `0009` slot was taken
by the pixel-cache removal mid-slice. Renumbered without changing
substance. Companion to `0008-slice-e-decisions.md`, which covers the
math-typesetting half of Slice E.

## Decision

Plain-text rendering uses **cosmic-text 0.19** for shaping/layout and
**swash 0.2** for glyph outlines, fed into the same fill pipeline that
Tex glyphs use. The wheel bundles **Inter Regular** (OFL-1.1, ~300 KB)
via `include_bytes!` inside `manim-rs-text`. Cosmic-text is initialized
once per process behind an `OnceLock<Mutex<FontSystem>>` seeded with a
private `fontdb` containing only Inter — no system-font scan, ever.
Layout runs at `SHAPE_PPEM = 1024`; the result is post-multiplied by
`size / 1024` to land in caller units. The first line's baseline
anchors at world `y = 0`.

## Why

- **Standard Rust text stack.** cosmic-text + swash + fontdb is the
  combo that pops out of every "modern Rust text" survey in 2025–26.
  It handles Unicode shaping (RTL, ligatures, combining marks via
  rustybuzz), layout (line breaking, alignment), and outline
  extraction without pulling C deps. No alternative is close in
  maturity.
- **Reuses Slice E's glyph machinery.** swash is already on the path
  for KaTeX glyph extraction (ADR 0008 §C). Same outline crate, same
  hinting workaround at high ppem, same fill pipeline downstream.
  Adding cosmic-text on top of that adds a shaper and a layout pass,
  not a parallel render path.
- **Bundled Inter Regular = zero-install promise.** Manimax ships as
  a wheel that "just works." Depending on system fonts breaks that
  on macOS dev (where users have many fonts) and on CI runners
  (where they don't). Inter is permissively licensed (OFL-1.1),
  Latin-complete, and visually neutral.
- **Singleton FontSystem amortizes init cost.** Slice E §6 gotcha #7
  flagged cosmic-text's font-database init as a per-call hazard.
  `OnceLock<Mutex<FontSystem>>` collapses it to a one-time
  deterministic boot — first call pays Inter parse, every later call
  pays a Mutex acquire.
- **Hinting-immune outlines.** Asking swash for outlines at low ppem
  activates TrueType hinting and snaps control points to the integer
  grid; rescaling those snapped points produces visible staircase
  scallops. ADR 0008 §C resolves this for math glyphs at
  `OUTLINE_PPEM = 1024`. Text reuses the same trick under a
  different name (`SHAPE_PPEM = 1024` in cosmic.rs) so future
  readers don't need to re-derive the workaround.
- **`FILL_TOLERANCE = 0.001` reuse.** Em-scaled glyph paths are em-
  scaled glyph paths regardless of source. The lyon flatness pin
  from ADR 0008 §D applies to text without further calibration.

## Consequences

- Wheel size grows by ~300 KB for Inter Regular plus ~1.5 MB for the
  KaTeX bundle (same number as ADR 0008's Tex story, recorded in
  performance.md).
- Bold / italic / non-Latin scripts are not bundled. Users hit
  `Text(..., font="path/to.ttf")` for those — the API has the
  parameter (S7b) but Slice E ships only the Regular face. Bold
  resolves to whatever cosmic-text falls back to (synthesized bold
  or Regular).
- Any future system-font opt-in must be explicit. The
  `OnceLock<FontSystem>` is seeded from the bundled DB only; making
  it scan system fonts later would change determinism guarantees and
  must come with a separate ADR.
- Justification (`Align::Justified`) is intentionally *not* exposed
  via `TextAlign`. cosmic-text supports it; Slice E §4 declared
  justified text out of scope to avoid advertising a feature we
  don't visually verify. Adding it later is a one-line enum
  extension.
- The Text fan-out at eval time mirrors ADR 0008 §A's Tex shape:
  `Object::Text` is a single IR node; `Evaluator::eval_at` expands
  it into per-glyph `Object::BezPath`s via a per-`Evaluator` cache
  keyed on `(src, font, weight, size, color, align)`. Raster never
  sees `Object::Text` (`unreachable!` contract). ADR 0009 carved
  out this future glyph cache explicitly — it's source-keyed,
  in-memory, not the kind of thing the pixel-cache removal applied
  to.

## Rejected alternatives

- **`rusttype` / `ab_glyph` for outlines, no shaping layer.** Both
  are outline-only. Skipping shaping means every "string" becomes
  a left-to-right run of charmap-resolved glyphs — no kerning, no
  ligatures, no RTL, no combining-mark composition. Adequate for
  ASCII-only debug labels, useless for "real text in animations."
  Manimax's audience writes math/physics/CS videos and routinely
  needs Greek and combining accents; punting shaping would force
  a swap mid-roadmap.
- **`fontdue` for shaping + raster.** Software rasterizer; Manimax
  rasterizes through wgpu via lyon. Using fontdue would mean a
  parallel raster path for text only, breaking the "everything is
  a fill / stroke through the same pipeline" invariant. swash gives
  outlines instead of pixels and slots into the existing path.
- **Direct rustybuzz / harfrust without cosmic-text.** Rustybuzz is
  the shaper cosmic-text already wraps; using it directly means
  reimplementing line breaking, alignment, font-fallback, and the
  shape→glyph-run plumbing. cosmic-text exists specifically to
  spare us that. The cost of "one more crate in the dep tree" is
  much lower than the cost of porting layout.
- **Pango via system bindings (manimgl's choice).** C dep, system
  install required, complicates the wheel build. The Rust side of
  Manimax is meant to be self-contained; pulling in a C library to
  render text would defeat the point.
- **System font scan as default.** fontdb supports
  `Database::load_system_fonts`. Rejected because (a) determinism:
  same scene renders differently on different hosts, (b) test
  hermeticism: CI font sets diverge from dev font sets, and (c)
  init cost (the Slice E §6 gotcha). Bundling Inter sidesteps all
  three. The escape hatch — `Text(..., font="path/to.ttf")` — is
  enough for users who need a different face.
- **Bundle multiple weights / italic up-front.** Each face is
  ~300 KB. Bundling Regular + Bold + Italic + BoldItalic balloons
  the wheel for a feature the slice plan declared out of scope.
  The single-face bundle is the smallest commitment that lets the
  zero-install promise hold for typical Latin text.

## See also

- `docs/decisions/0008-slice-e-decisions.md` — Tex half of Slice E.
  §C (high-ppem outline extraction) and §D (lyon flatness pin) are
  inherited, not re-derived, here.
- `docs/decisions/0009-remove-pixel-cache.md` — explicitly preserves
  source-keyed in-memory caches (Tex geometry, future glyph caches)
  while deleting the rgba snapshot cache. Text's cache lives in
  that carve-out.
- `docs/porting-notes/text.md` — alignment semantics, line-height
  multiplier, what's missing vs. manimgl's Pango.
- `crates/manim-rs-text/src/cosmic.rs` — the implementation.
