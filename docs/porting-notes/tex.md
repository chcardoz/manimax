# Porting note: Tex

**Status:** Slice E shipped (Steps 1–8).
**Manimgl reference:** `reference/manimgl/manimlib/mobject/svg/tex_mobject.py`
and `reference/manimgl/manimlib/utils/tex_file_writing.py` at submodule
HEAD as of Slice E.
**RaTeX reference:** `github.com/erweixin/RaTeX` — pin recorded in the
workspace `Cargo.toml`'s `[workspace.dependencies]` block.

Tex is **reimplemented**, not ported. ManimGL shells out to the system
`latex` binary, runs `dvisvgm --no-fonts` to produce SVG, and parses the
SVG paths into mobjects. Manimax replaces that whole pipeline with
RaTeX (pure Rust, KaTeX-grammar subset). The user-visible API tries to
match manimgl where it can; the rendering path doesn't, and nothing
about LaTeX bit-fidelity carries over.

What follows is the set of invariants and edge cases that aren't
obvious from reading either codebase, written down so the next person
porting a `Tex`-adjacent feature doesn't re-derive them.

## What we kept from manimgl

- **`Tex(src, color=...)` constructor shape.** Same positional source
  string, same keyword color override. Color semantics intentionally
  match: top-level `color` covers every glyph; `\textcolor{...}{...}`
  inside the source overrides per item.
- **Color override is "set the whole thing"**, not "blend with author
  color." Slice plan §2 calls this out explicitly. The
  `compile_tex` pass takes the IR `color` and overwrites the per-
  `DisplayItem` color from RaTeX uniformly, except for items RaTeX
  itself colored via `\textcolor` — those keep their per-item color.
- **The bus-factor escape hatch is "vendor the parser."** Same shape as
  ManimGL's "in extremis you can fork TeX" — neither is something we'd
  do casually, but both are knowable, finite operations. Recorded in
  ADR 0008 §G.

## What we explicitly didn't keep

- **System `latex`.** No subprocess, no temp dir, no `dvisvgm`. The
  trade-off is the KaTeX-coverage subset documented in
  `docs/tex-coverage.md`.
- **TikZ, `chemfig`, packages.** Out of subset, will surface as
  parse errors at construction time.
- **Auto equation numbering.** RaTeX requires explicit `\tag{...}`.
  manimgl inherits TeX's automatic `(1)`, `(2)` machinery.
- **Computer Modern.** ManimGL renders in CM (TeX's default); Manimax
  renders in KaTeX's bundled fonts. Letterforms are similar but not
  identical — `\mathcal` and `\mathfrak` are the most visible
  differences. Documented in `docs/tex-coverage.md`.
- **`set_color_by_tex` / glyph-range color overrides.** ManimGL has
  fine-grained `set_color_by_tex(part, color)` machinery; we have
  whole-tex `color=` plus inline `\textcolor`. Adding the manimgl
  shape requires per-`DisplayItem` index ranges plumbed through the
  IR — not on any roadmap.

## RaTeX `DisplayList` / `DisplayItem` — the contract

`crates/manim-rs-tex/src/adapter.rs` translates RaTeX's `DisplayList`
into `Vec<(BezPath, [f32; 4])>`. The contract has three coordinate-
system facts that bite if you forget them:

1. **RaTeX is y-down, em-units.** Every `DisplayItem`'s `(x, y)` is
   in em-scaled font space with the y axis pointing down (typographic
   convention). Manimax is y-up, world-units. The adapter applies one
   y-flip plus one em→world scale at the boundary; downstream code
   never sees y-down coordinates.
2. **`DisplayItem::GlyphPath` carries a `font` string and a `char_code`.**
   The `font` is something like `"Main-Regular"` or `"Math-Italic"`.
   Resolution lives in `crates/manim-rs-text/src/font.rs` via the
   `ratex-katex-fonts` crate's named font enum. If RaTeX renames a
   font upstream, glyph lookups go silent (return `None`). Slice plan
   §6.3 gotcha.
3. **`DisplayItem::GlyphPath::commands` is a placeholder.** RaTeX
   doc-comments it as "not used by any renderer" — it's there for
   future use. We pull outlines through swash, not through this
   field. Don't read from it; the data is incomplete.

`PathCommand` ordering matters: `CubicTo { x1, y1, x2, y2, x, y }`
maps to `kurbo::BezPath::curve_to(p1, p2, p3)` where `p1 = (x1, y1)`,
`p2 = (x2, y2)`, `p3 = (x, y)`. Swap and you get visually plausible
but mathematically wrong cubics — no test in the corpus catches it
because the bbox is preserved.

## The `\textcolor` interaction

User writes `Tex(r"\textcolor{red}{x} + y", color=BLUE)`. The
question is "what color is `x`?" Decision (slice plan §6.10): per-item
color from RaTeX wins for items it explicitly colored; the top-level
`color=` covers the rest. So `x` is red, `+` and `y` are blue.

This matches RaTeX's `DisplayItem::color` semantics directly —
`compile_tex` only overwrites items whose color RaTeX left at the
default. Test pin in `crates/manim-rs-eval/src/tex.rs` (and the
compile-color tests around it).

`\textcolor{...}` accepts CSS color names (RaTeX's table). Hex
literals (`{#ff8800}`) are not supported — RaTeX rejects them at
parse time. `tex-coverage.md` documents the workaround (use
`Tex(color=(r,g,b,a))` for arbitrary colors, leave `\textcolor` for
named highlights).

## Coordinate flip and scale

The em→world transform is documented in
`crates/manim-rs-tex/src/adapter.rs`. One affine, applied once per
`DisplayItem`:

```
world.x = item.x
world.y = -item.y          # y-flip
final = world * em_to_world_scale
```

`em_to_world_scale` is the IR's `Tex.scale` *not* baked into the
geometry — see ADR 0008 §A. Baking caused double-application during
Slice E Step 4. The current contract:

- `compile_tex` produces `Vec<Arc<Object>>` at unit scale (1 em =
  1 world unit).
- Per-glyph `ObjectState`s carry `parent_scale * tex_scale` as a
  uniform transform.
- Raster multiplies that into the MVP. One affine, no doubling.

If you're tempted to inline `Tex.scale` into the `BezPath` for
"performance" — don't. The fan-out children share the cached
geometry across all instances of the same `(src, color)`; baking
scale defeats the cache.

## Visual bugs we already paid for

Both surface as "the curves look wrong" once you render. Both have
ADRs and gotcha entries; capturing the diagnosis here so future
Tex-adjacent work doesn't re-discover them:

- **swash hinting at low ppem.** Outlines extracted at ppem ≈ 1.0
  (the natural ask given "1 em = 1 world unit") get TrueType
  hinting applied — control points snap to the integer pixel grid.
  Rescaling the snapped outline produces staircase scallops at any
  zoom level. Fix: extract at `OUTLINE_PPEM = 1024` and post-multiply
  by `Affine::scale(scale / 1024)`. ADR 0008 §C; gotcha in
  `docs/gotchas.md` ("swash hinting at low ppem...").
- **Lyon `FillOptions::DEFAULT.tolerance = 0.25` is em-scaled-fatal.**
  0.25 is calibrated for SVGs in pixel units; an em-scaled glyph
  flattens into an octagon at that budget. Fix: pin
  `FILL_TOLERANCE = 0.001`. ADR 0008 §D; same gotcha section.

If a glyph-rendering bug surfaces and isn't either of these, render
the *intermediate stage* (BezPath dump or a single-glyph snapshot at
the actual ppem) before guessing. The single-frame API
(`python -m manim_rs frame`, ADR 0008 §F) makes that cheap; without
it, the back-and-forth is brutal — Slice E Step 5 spent two
diagnostic passes on the hinting bug because of a wrong first guess.

## Cache discipline

`compile_tex` is cached per-`Evaluator` (ADR 0008 §B). The key is
**the entire `Object::Tex` node** hashed via blake3 of
`serde_json::to_vec` bytes. Per the post-Step-5 cleanup pass
(retrospective in slice plan §11), the key was tightened during
Slice E to drop `scale` and `macros` after they were caught
spuriously included — both invalidate cache without changing the
shaped output.

Rule for adding fields to `Object::Tex`: **only fields that change
the BezPath geometry belong in the cache key.** Color is in the key
because `compile_tex` bakes color into `(BezPath, [f32; 4])` pairs;
scale and per-instance transforms are not, because they're applied
at the eval-time fan-out site and never reach `compile_tex`.

## Files touched

- `crates/manim-rs-tex/src/adapter.rs` — `DisplayList` → `BezPath`.
- `crates/manim-rs-tex/src/error.rs` — `TexError` enum, source-
  location-bearing.
- `crates/manim-rs-text/src/font.rs` — RaTeX font name → bytes.
- `crates/manim-rs-text/src/glyph.rs` — swash outline extraction
  with the `OUTLINE_PPEM` workaround.
- `crates/manim-rs-eval/src/tex.rs` — `compile_tex` + color override.
- `crates/manim-rs-eval/src/evaluator.rs` — Tex fan-out + cache.
- `crates/manim-rs-raster/src/tessellator.rs` — `FILL_TOLERANCE`.
- `python/manim_rs/objects/tex.py` — Python `Tex` constructor +
  macro pre-expansion.
- `tests/python/tex_corpus.py` — supported-subset corpus.
