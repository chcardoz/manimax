# Slice E — text + math

**Status:** scoped, not started.
**Date:** 2026-04-28.
**Follows:** `slice-d.md` (shipped). Read Slice D's §11 retrospective before any step here.

Slice D shipped real strokes + the snapshot cache. The pipeline now renders multi-shape animated geometry end-to-end, but every "letter" is still a placeholder. Slice E adds the two content kinds that turn this into something an actual math-video author can use: plain text and LaTeX-flavored math. Both reduce to glyph outlines fed into the existing fill/stroke path; no new raster pipeline, no new IR shape beyond two new `MObjectKind` variants.

Ship criteria: **(a)** `Text("Hello, world")` renders correctly to mp4 with a bundled default font, no system font dependency; **(b)** `Tex(r"\sum_{i=1}^n i = \frac{n(n+1)}{2}")` renders correctly using a pure-Rust math typesetter, with no system LaTeX install required; **(c)** both go through the snapshot cache like any other IR node.

Read `docs/architecture.md`, `slice-d.md` (especially §11), and `reference/manimgl/manimlib/{tex,svg,utils/tex_file_writing.py}` first. This doc assumes them.

---

## 1. Goal

Two acceptance commands, both green:

```
# Plain text scene.
python -m manim_rs render examples.text_scene TextScene out.mp4 --duration 3 --fps 30

# Math scene.
python -m manim_rs render examples.tex_scene TexScene out.mp4 --duration 4 --fps 30
```

Both produce mp4s where the rendered glyphs visibly match the source string and expression. Second run of either completes in < 1s (cache hit), byte-identical output.

No system LaTeX. No system font. The wheel ships everything it needs.

---

## 2. Scope Decisions (locked)

| Dimension | Choice | Rationale |
|---|---|---|
| Math typesetter | **RaTeX** (`github.com/erweixin/RaTeX`, MIT). Depend on `ratex-{parser,layout,types,katex-fonts}`; skip `ratex-{svg,pdf,render,ffi,wasm}`. | Pure Rust. Layout already done — saves 3,000–5,000 LOC vs. `pulldown-latex` + writing MathML Core layout ourselves. >99.5% KaTeX coverage. `DisplayList` is a public, structured, stable contract (verified by reading source). See ADR 0008. |
| RaTeX version | Pin to a specific release + SHA in `Cargo.toml`. v0.1.x is young; treat any version bump as breaking and retest the corpus. | Bus-factor / API-stability hedge. Vendor on the spot if upstream goes silent — 24,500 LOC, MIT, vendorable in <1 day. |
| Math coverage = KaTeX coverage | Document explicitly. **Not** full LaTeX. No `\usepackage`, no TikZ, no `chemfig`, no auto-equation-numbering. | This is the price of pure Rust. Document the visible deltas vs. manimgl in `docs/tex-coverage.md`. |
| `\newcommand` support | **Python-side macro pre-expansion**, KaTeX-style string substitution. `Tex(src, macros={"RR": r"\mathbb{R}"})`. | RaTeX doesn't expose macro definition. Python pre-pass covers the 90% case (no-arg shortcut macros). Argument macros (`\norm{x}`) deferred — punt to "vendor and patch ratex-parser" if/when someone needs them. |
| Math fonts | Bundle KaTeX TTFs via `ratex-katex-fonts` crate. ~1–2 MB to wheel. License: OFL-1.1, redistributable. | Zero-install matches Manimax's pure-Rust contract. |
| Text engine | **`cosmic-text`** for shaping/layout, **`swash`** for glyph outlines. | Already on the radar from `slice-d.md` §9. Standard Rust text stack. `swash` is reused by the math half (KaTeX glyph outlines). |
| Default text font | **Inter** (OFL-1.1). Bundle one weight (Regular) inside `manim-rs-text` via `include_bytes!`. | Permissive, broad coverage. Users override via `Text(..., font="path/to.ttf")`. |
| Wheel size budget | KaTeX fonts (~1.5 MB) + Inter Regular (~300 KB) ≈ +2 MB. | Acceptable. Flag in `docs/performance.md` for a future "trim or lazy-load fonts" pass. |
| Glyph → IR adapter | Math: `display_list_to_bezpath()` walks RaTeX's `DisplayList`, resolves `(font, char_code)` via `swash`, emits transformed `kurbo::BezPath`. Text: cosmic-text glyph runs → `swash` outlines → `BezPath`. Both feed the existing fill path. | Both content kinds reduce to "glyph outlines at positions, filled with a color." Unifies cache, MSAA, encode. |
| IR shape | Two new `MObjectKind` variants: `Text { src, font, weight, size, color, align }` and `Tex { src, macros, color, scale }`. **IR carries source strings**, not pre-flattened paths. | Compilation in Rust keeps IR small/diffable, keeps Python side dependency-free, lets the cache key remain content-hash without re-hashing megabytes of paths. |
| Tex `color` | Single Python-supplied color overrides RaTeX's per-`DisplayItem` color. `\textcolor{red}{...}` inside the source still works (overrides per item). | Matches manimgl's `Tex(..., color=BLUE)` semantics. |
| Coordinate system | RaTeX outputs in font-em units, y-down. Adapter flips y and scales to Manimax world units at the boundary. | RaTeX is renderer-agnostic but assumes standard typographic conventions; convert once, in the adapter, with the transform documented in code. |
| `engine="latex"` opt-in | **Out of scope.** Documented as a future fallback (system `latex` + `dvisvgm --no-fonts`, mirroring manim-community's pipeline). Not Tectonic — lighter, no C-build pain, matches manimgl. | Anyone who needs full LaTeX fidelity can wait for the opt-in. Vast majority will never hit it. |
| Equation auto-numbering | Out. Users opt in per-equation via explicit `\tag{...}` (RaTeX's contract). | RaTeX limitation; documenting it is cheaper than working around it. |
| Animated text/Tex | Out. Static-per-render only. `TransformMatchingTex`-style glyph correspondence is a separate slice — needs a design pass for stable glyph identity across edits. | Don't bundle two hard problems. |
| 3D text / extruded text / text-on-surface | Out. Slice F+ (3D pipeline). | |
| SVG import (`SVGMobject`) | Out. Adjacent — the `BezPath` outlines for an SVG path are the same shape as a glyph outline — but pulling in an SVG parser is its own integration. | Possible mini-slice after E. |
| RTL / Indic shaping | Out unless trivially free from `cosmic-text`. Not testing for it. | |
| Cache integration | Free. New IR variants hash like any other node; the existing per-frame cache works without changes. | |
| Snapshot tests | Tolerance-based (Slice C/D pattern). Two corpora: text rendering snapshots + a Tex coverage corpus of 30–50 expressions. | |
| Platform | macOS arm64 dev box, Linux/lavapipe in CI (Slice D pattern, ADR 0007). | Unchanged. |
| ADRs | **Two:** `0008-tex-via-ratex.md` (the load-bearing decision) and `0009-text-via-cosmic-text-swash.md` (briefer; choice is uncontroversial). | The Tex decision touches bus-factor, license, coverage gap, upgrade story; deserves its own record. |

---

## 3. Work Breakdown

Ordered. Each step ends with a testable artifact. Per Slice C §11 and Slice D's reaffirmation: "expose to Python + use in test" is collapsed within each step — no step leaves a feature reachable only from Rust.

### Step 1 — `manim-rs-text` crate: font plumbing + single-glyph end-to-end

- New crate `crates/manim-rs-text/` with `swash` and `kurbo` deps.
- Bundle Inter Regular via `include_bytes!`. Expose `default_text_font() -> &'static [u8]`.
- Bundle KaTeX TTFs (depend on `ratex-katex-fonts`). Expose `katex_font(name: &str) -> Option<&'static [u8]>` keyed on RaTeX's font names.
- `glyph_to_bezpath(font: &[u8], char_code: u32, scale: f32) -> kurbo::BezPath` — `swash` outline → `kurbo` verbs. y-flip applied here so callers always get Manimax-convention coordinates.
- Unit test: outline a single glyph from each bundled font; assert non-empty path with expected bbox.

**Why first:** every subsequent step calls into this. Get the font/outline boundary working in isolation before wiring it through anything.

**Artifact:** `cargo test -p manim-rs-text` green.

### Step 2 — RaTeX integration: parse → DisplayList → BezPath

- New crate `crates/manim-rs-tex/`. Deps: `ratex-parser`, `ratex-layout`, `ratex-types`, `manim-rs-text`, `kurbo`.
- `tex_to_display_list(src: &str) -> Result<DisplayList, TexError>` — wraps RaTeX's parser+layout. Wraps RaTeX errors into a `TexError` enum that carries source location.
- `display_list_to_bezpath(list: &DisplayList) -> Vec<(BezPath, Color)>` — the adapter:
  - `DisplayItem::GlyphPath { font, char_code, x, y, scale, color }` → `manim_rs_text::glyph_to_bezpath` + translate by `(x, y)`.
  - `DisplayItem::Line` → trivial rect path.
  - `DisplayItem::Rect` → trivial rect path.
  - `DisplayItem::Path { commands, x, y, color }` → 1:1 translation: `PathCommand::{MoveTo, LineTo, CubicTo, QuadTo, Close}` → `kurbo::BezPath::{move_to, line_to, curve_to, quad_to, close_path}`. Translate by `(x, y)`.
- Coordinate transform documented in the adapter's doc comment: y-flip, em→world-unit scale.
- Unit tests on a small set: `\frac{a}{b}`, `\sqrt{x}`, `x^2`, `\sum_{i=1}^n i`. Assert `Vec<(BezPath, Color)>` non-empty and bbox sane.

**Reference:** `crates/ratex-svg/src/lib.rs` upstream — 906 LOC, our adapter mirrors its structure but emits `BezPath` instead of SVG strings. Expected size ~250–400 LOC.

**Artifact:** `cargo test -p manim-rs-tex display_list_to_bezpath` green.

### Step 3 — Tex IR variant + Rust-side eval

- `crates/manim-rs-ir`: add `MObjectKind::Tex { src: String, macros: BTreeMap<String,String>, color: Color, scale: f32 }`. `BTreeMap` for canonical serialization (cache-key-stable, per Slice D §5 gotcha).
- `crates/manim-rs-eval`: `Tex` is time-invariant — eval just produces `(Vec<BezPath>, Color)` once and caches by IR hash within the eval run. Macro pre-expansion happens **Python-side** (Step 5) so the IR's `src` is already-expanded source; Rust just calls `tex_to_display_list` and the adapter.
- Wire fills through the existing fill pipeline. No new raster code; the fill path treats Tex outlines like any other `BezPath`.

**Artifact:** `cargo test -p manim-rs-eval tex_eval` — build a `Scene` with one `Tex` mobject in Rust, eval at `t=0`, assert non-empty BezPath output.

### Step 4 — End-to-end Tex render (Rust harness)

- `crates/manim-rs-runtime/tests/tex_render.rs`: Rust-only integration test. Build a scene with one `Tex` node, render to mp4, `ffprobe`-verify dimensions/codec, snapshot a single frame's RGBA against a tolerance baseline.
- Confirm cache integration: second invocation hits the cache (existing machinery — verify, don't extend).

**Why this before the Python surface:** isolates whether problems are in the Tex pipeline vs. the Python boundary. Slice D §11 noted that conflating the two costs hours.

**Artifact:** `cargo test -p manim-rs-runtime tex_render` green; mp4 visually correct.

### Step 5 — Python `Tex()` + macro pre-expansion

- `python/manim_rs/objects/tex.py`:
  - `class Tex(Mobject)` with `__init__(src, *, macros=None, color=WHITE, scale=1.0)`.
  - Macro expansion: small pure-Python pass. Iterate `macros` dict, do longest-key-first regex substitution against word boundaries on `\<key>`. No-arg only; doc-comment that arg macros are not supported. Raise `ValueError` on suspicious input (e.g. recursive macros) — bound substitution to N=8 passes.
- IR emission: `Tex` produces an `MObjectKind::Tex` node carrying the **already-expanded** source.
- `python/manim_rs/colors.py`: ensure standard `RED, BLUE, GREEN, ...` constants exist (manimgl-compatible names) if not already.

**Artifact:** `tests/python/test_tex.py`:
- `Tex(r"\frac{a}{b}")` round-trips through IR.
- `Tex(r"\RR^2", macros={"RR": r"\mathbb{R}"})` expands and round-trips.
- Render a Tex scene to mp4 and check non-empty frame.
- Recursive macro raises `ValueError`.

### Step 6 — Tex coverage corpus

- `tests/python/tex_corpus.py`: 30–50 expressions covering: `\frac`, `\sqrt`, sub/superscripts, `\sum_{i=1}^n`, `\int_a^b`, `\lim_{x \to 0}`, `\prod`, `\binom`, `\pmatrix`/`\bmatrix`/`\vmatrix`, `\begin{aligned}`, `\begin{cases}`, accents (`\hat`, `\tilde`, `\bar`, `\vec`), Greek lowercase + uppercase, `\mathbb{R}`/`\mathcal{L}`/`\mathfrak{g}`, big delimiters (`\left( \right)`, `\left\| \right\|`), spacing (`\,`, `\quad`), `\text{...}` inside math.
- For each: snapshot test that renders to a single frame and tolerance-checks against a baseline rgba.
- `docs/tex-coverage.md`: enumerate the supported subset, document known visible deltas vs. manimgl rendering, document `engine="latex"` as the future opt-in for full fidelity.

**Artifact:** `pytest tests/python/test_tex_corpus.py` green; coverage doc written.

### Step 7 — Python `Text()` via cosmic-text

- `crates/manim-rs-text`: extend with `text_to_bezpaths(src: &str, font: &[u8], size: f32, weight: Weight, align: Align) -> Vec<(BezPath, Color)>`. Uses `cosmic-text` for shaping/layout, `swash` for outlines.
- IR: `MObjectKind::Text { src, font: Option<PathBuf>, weight, size, color, align }`. `font: None` → bundled Inter Regular.
- `crates/manim-rs-eval`: `Text` eval is time-invariant; same shape as Tex.
- Python `python/manim_rs/objects/text.py`: `Text(src, *, font=None, weight="regular", size=1.0, color=WHITE, align="left")`.

**Artifact:** `tests/python/test_text.py` — `Text("Hello")` round-trips through IR, renders to mp4, frame non-empty. Bundled-font path works without any system font.

### Step 8 — Combined integration scene + cache verification

- `examples/text_scene.py`: a 3-second scene with a `Text(...)` greeting and a `Tex(...)` formula on screen simultaneously, both with non-default colors, one of them animated in opacity (proves time-evaluated state still works through the existing eval + cache).
- `tests/python/test_e2e_text_tex.py`:
  - Cold render: produces mp4 with expected duration / fps / dimensions via `ffprobe`.
  - Warm render: < 1s wall-clock, byte-identical mp4.
  - `--no-cache`: same mp4, cold-render time.
- Confirm `manim-rs-runtime`'s cache hashing handles the new IR variants without code change (Slice D's blake3-of-canonical-serde-bytes machinery is content-agnostic; this step verifies that promise).

**Artifact:** the two acceptance commands in §1 green; `ffprobe` clean; cache behavior matches Slice D contract.

### Step 9 — ADRs, porting notes, performance log

- `docs/decisions/0008-tex-via-ratex.md`:
  - Why RaTeX over: Tectonic (C-build pain, network fetch), `pulldown-latex` + custom layout (3–5k LOC of work), Typst+MiTeX (heavyweight, syntax conversion). Independent second-opinion analysis converged.
  - Bus-factor mitigation: small, MIT, vendorable in <1 day.
  - Coverage gap acknowledged: KaTeX-subset, not full LaTeX.
  - Pinned SHA recorded.
  - `\newcommand` deferred to Python-side expansion; documented escalation path (vendor-and-patch parser).
  - Triggers for re-eval: RaTeX abandonment, breaking DisplayList API change, a feature requirement that breaks Python-side macro expansion.
- `docs/decisions/0009-text-via-cosmic-text-swash.md` — brief; standard Rust text stack, default font choice, override mechanism.
- `docs/porting-notes/tex.md` — invariants from RaTeX + manimgl `Tex`. What `\textcolor` does. Coordinate-system flip. Per-`DisplayItem`-color override semantics. SHA cited.
- `docs/porting-notes/text.md` — cosmic-text vs. manimgl Pango: alignment semantics, default line-height, what's missing (RTL, Indic).
- `docs/tex-coverage.md` — supported-subset reference (written in Step 6, expanded here with cross-links).
- `docs/performance.md` — append: wheel size delta from bundled fonts, RaTeX parse+layout cost vs. eval+raster cost (likely negligible), any cache-key-cost observations.
- `docs/gotchas.md` — any traps surfaced during the slice (RaTeX coordinate convention, font-name string brittleness, etc.).

**Artifact:** all docs written; §10 retrospective in this file ready to fill on ship.

---

## 4. Explicitly Out of Scope

Belongs to Slice F+ or its own slice. Resist scope creep:

- **`engine="latex"` opt-in** (system `latex` + `dvisvgm --no-fonts` for full-fidelity LaTeX). Documented as the future fallback for users who need TikZ / unusual packages.
- **Tectonic embedding.** Larger binary, C-build deps, first-run network fetch — and `engine="latex"` covers the same use case more cleanly.
- **`TransformMatchingTex` / glyph correspondence across edits.** Needs a stable-glyph-identity design.
- **`\newcommand` with arguments.** Python-side string substitution can't handle `\norm{x}`. Defer until someone asks; escalation path is vendor-and-patch `ratex-parser`.
- **Equation auto-numbering** (`align`'s automatic `(1)`, `(2)`). RaTeX requires explicit `\tag{...}`.
- **3D text** — extruded text, text on a surface, depth-buffered. Slice F.
- **SVG import** (`SVGMobject`). Adjacent (path-based mobjects) but separate parser integration. Possible mini-slice after E.
- **Animated per-character or per-glyph effects.** Static glyph outlines per render only.
- **RTL scripts and Indic shaping.** Not tested unless trivially free.
- **Multiple text-font weights bundled.** Inter Regular only; users supply `font=` for bold/italic.
- **Full `\textcolor` interaction with Manimax's color system.** Per-`DisplayItem` color works; deeper integration (LaTeX color → Manimax `set_color` semantics) deferred.
- **Lyon-fill replacement / Loop-Blinn AA upgrade.** Slice D §4 carry-over; not bundled.
- **Distributed / S3 cache, LRU eviction, parallel chunked render.** Slice D carry-over; still later.

---

## 5. Success Criteria

- [ ] `maturin develop` builds cleanly; `pytest tests/python` and `cargo test --workspace --exclude manim-rs-py` all green.
- [ ] Both commands in §1 produce `out.mp4`; second run completes in < 1s wall-clock with byte-identical output.
- [ ] `ffprobe out.mp4` reports expected dimensions / fps / codec / pix_fmt for both commands.
- [ ] Visually: `Text` renders a recognizable string; `Tex` renders the formula with correct fraction, sum, sub/superscript layout; both at expected colors.
- [ ] No system LaTeX or system font installed on the CI runner; both commands still pass.
- [ ] Tex coverage corpus (30–50 expressions) all snapshot-stable with tolerance baselines.
- [ ] `Tex(src, macros={...})` expands no-arg macros end-to-end.
- [ ] `--no-cache` skips the cache for Text and Tex scenes; output matches cached output.
- [ ] `0008-tex-via-ratex.md` and `0009-text-via-cosmic-text-swash.md` written.
- [ ] `docs/tex-coverage.md` enumerates supported subset and known deltas vs. manimgl.
- [ ] `docs/porting-notes/{tex,text}.md` written.
- [ ] §10 retrospective filled before hand-off.

---

## 6. Known Gotchas To Pre-Solve

Each costs an hour cold. Pre-empting saves the day:

1. **RaTeX `DisplayList` coordinate convention.** y-down, em-units. Manimax is y-up, world-unit. Apply the transform once in the adapter, document it. Don't sprinkle flips at call sites.
2. **`PathCommand::CubicTo` field names.** RaTeX's `CubicTo { x1, y1, x2, y2, x, y }` maps to `kurbo::BezPath::curve_to(p1, p2, p3)`. Triple-check ordering — bezier control points silently render wrong on swap, no test will catch a subtle mis-flip.
3. **Font-name strings from RaTeX are brittle.** `DisplayItem::GlyphPath::font` is a `String` like `"Main-Regular"`. Hard-code the name → bytes mapping in `manim-rs-text`; verify against the `ratex-katex-fonts` crate's exposed font names. If RaTeX renames a font, all glyph lookups silently miss. Add a unit test that resolves every font name RaTeX emits in the corpus.
4. **`BTreeMap` for `macros` in IR.** Insertion-order maps would invalidate cache on cosmetic Python dict reordering. Slice D §5 has the same gotcha; same fix.
5. **Macro expansion order.** Longest-key-first substitution prevents `\R` clobbering inside `\RR`. Test specifically: `macros={"R": "X", "RR": "Y"}` on input `\RR \R` should yield `Y X`, not `XR X` or similar.
6. **`include_bytes!` paths.** `swash` reads `&[u8]`; the path inside `include_bytes!` is relative to the source file, not the crate root. Trips up moves between crates.
7. **`cosmic-text` font database init cost.** First-call cost can dominate small test scenes. Use a lazy-static / `OnceCell` for the database; benchmark cold vs. warm and log in `docs/performance.md`.
8. **Wheel size CI check.** Bundled fonts move the wheel from "small" to "noticeably larger." If there's a CI wheel-size budget, raise it before this slice merges or the merge fails post-build.
9. **RaTeX's `commands` field on `GlyphPath` is a placeholder.** The doc comment in `display_item.rs` explicitly says it's not used by any renderer (skipped during serialization). Resist the temptation to use it — go through `swash` for the real outlines.
10. **`\textcolor` interaction.** RaTeX emits per-`DisplayItem` colors. If a user writes `Tex(r"\textcolor{red}{x} + y", color=BLUE)`, what color is `x`? Decide: per-item color from RaTeX wins for items it explicitly colors, top-level `color=` covers the default (uncolored) items. Document and test.

---

## 7. Touched Files Map

Rough sketch — actual diff will deviate. New files preceded with `+`.

```
crates/
+ manim-rs-text/              # font plumbing + glyph outlines (text + math share this)
+   Cargo.toml
+   src/lib.rs
+   src/font.rs
+   src/glyph.rs
+   src/cosmic.rs
+   fonts/Inter-Regular.ttf   # ~300 KB
+ manim-rs-tex/                # RaTeX wrapper + DisplayList → BezPath
+   Cargo.toml
+   src/lib.rs
+   src/adapter.rs
+   src/error.rs
  manim-rs-ir/
    src/lib.rs                 # add Text + Tex variants
  manim-rs-eval/
    src/evaluator.rs           # add Text + Tex eval cases (time-invariant)
  manim-rs-runtime/
    src/lib.rs                 # nothing — cache is content-agnostic
+   tests/tex_render.rs
  manim-rs-py/
    src/lib.rs                 # expose Text + Tex constructors

python/manim_rs/
+ objects/text.py
+ objects/tex.py
  ir.py                        # mirror IR additions
  colors.py                    # ensure standard color constants

examples/
+ text_scene.py
+ tex_scene.py

tests/python/
+ test_text.py
+ test_tex.py
+ test_tex_corpus.py
+ test_e2e_text_tex.py
+ tex_corpus.py                # corpus fixture data

docs/
+ decisions/0008-tex-via-ratex.md
+ decisions/0009-text-via-cosmic-text-swash.md
+ porting-notes/tex.md
+ porting-notes/text.md
+ tex-coverage.md
  performance.md               # append
  gotchas.md                   # append (likely)
  STATUS.md                    # rewrite at hand-off
```

---

## 8. Effort Estimate

Honest ranges based on Slice D's actuals (estimate held at 1.5–3 days; real path was ~2 days focused).

- Step 1 (text plumbing + single glyph): ~0.5 day.
- Step 2 (RaTeX adapter): ~1 day. The integration is mostly mechanical because RaTeX's `DisplayList` is already-shaped.
- Step 3 (Tex IR + Rust eval): ~0.5 day.
- Step 4 (Rust E2E Tex): ~0.5 day.
- Step 5 (Python `Tex` + macros): ~0.5 day.
- Step 6 (coverage corpus): ~1–1.5 days. Building the corpus is the slow part — choosing expressions, baselining snapshots.
- Step 7 (Python `Text`): ~1 day.
- Step 8 (E2E + cache verification): ~0.5 day.
- Step 9 (ADRs + porting notes): ~0.5–1 day.

**Total: ~6–7 focused days.** Roughly Slice D's size. The math half is bigger than the text half (Steps 2–6 vs. Step 7).

---

## 9. What Comes After Slice E

Not committed. Natural sequence unchanged from `slice-d.md` §9:

- **Slice F:** 3D — surface pipeline, depth buffer, phi/theta camera. Reintroduces `flat_stroke` + `unit_normal`. 3D text becomes possible.
- **Slice E.5 (mini):** SVG import. Path-based, similar to glyphs; small-but-real scope.
- **`engine="latex"` opt-in:** later, when a real user hits a coverage gap. Pipeline: `latex` → `dvisvgm --no-fonts` → SVG → `BezPath`. Same `MObjectKind::Tex` IR variant; just a different compile path on the Rust side.

Revisit after Slice E lands.

---

## 10. Deltas carried from Slice D §11

To apply in this plan:

- **Single consolidated ADR per slice** worked well in D. Slice E uses **two** ADRs because the Tex and Text decisions are independently load-bearing — Tex carries bus-factor / coverage / upgrade risk; Text is uncontroversial but worth recording. Don't overload `0008` with the Text choice.
- **Cache key shape.** D learned that hashing evaluated state (not raw IR + index) was the right default. E's new IR variants are time-invariant so this distinction barely matters, but keep the same eval-state hashing path; don't fork.
- **Snapshot-test rebaselining.** Tolerance-based, no exact-pixel pins. Same as D.
- **"Expose to Python + use in test" collapsed per step.** Steps 4 and 5 each leave the surface usable from the language layer they target.
- **Pinned-SHA discipline.** RaTeX SHA pinned in Cargo.toml; if it advances mid-slice, re-pin and re-verify the corpus before merge.

---

## 11. Retrospective — what the plan got wrong

In progress; filled incrementally as steps ship. Steps 1–5 done;
Steps 6–9 pending.

### Steps 1–3 (font plumbing, RaTeX adapter, Tex IR + eval)

Mostly tracked the plan. RaTeX's `DisplayList` was as advertised —
adapter sized within the predicted 250–400 LOC. Coordinate
convention (§6.1) was the only real friction; one-time fix in the
adapter, no leakage.

### Step 4 (Rust E2E Tex render) — surprises

- **Tex fan-out site.** The plan assumed `compile_tex` would
  produce a "Tex object with internal BezPaths." Implementation
  revealed the cleaner shape is fanning out at `Evaluator::eval_at`
  into N separate `ObjectState`s (one per glyph), so the raster
  layer never sees a Tex node. ADR 0008 §A. The slice plan's Step 3
  language ("Tex eval just produces `(Vec<BezPath>, Color)`")
  understated this — there's no single Tex `ObjectState`, only
  fan-out children.
- **Scale double-application.** First implementation baked
  `Tex.scale` into the BezPath geometry inside `compile_tex` *and*
  carried it on the `ObjectState`. Anything with `scale != 1.0`
  rendered twice as scaled. Fix: don't bake — multiply parent and
  Tex scale at the fan-out site. Cheap to spot in retrospect; the
  test that caught it was visual, not assertion-based.
- **Per-Evaluator Tex cache.** Not in the plan but obviously right
  once the eval-time fan-out shape was clear. Mirrors Slice D §D's
  hash discipline. ADR 0008 §B.

### Mid-Step-5 detour — single-frame render API

Not in §3. Triggered by visual debugging needs: "render this one
timestamp at this resolution and look at the pixels." Resulting
shape: `render_frame_to_png` in the runtime + `render_frame`
pyo3 entry point + `frame` typer subcommand. ADR 0008 §F. Cost ~1
hour; saved many hours of "render mp4, scrub, squint." Worth doing
inline rather than parking — the Step 5 visual bugs (below) were
unspottable without it.

### Step 5 visual bugs — two distinct quality issues

Both surfaced as "the paths look weird" once a real Tex scene was
rendered. Diagnosed in two iterations:

1. **Lyon fill flatness too coarse.** `FillOptions::DEFAULT`
   tolerance is 0.25 — a quarter of a pixel for SVG-style geometry.
   Glyph outlines arrive in em-units where 1 em ≈ 1 world unit, so
   a 0.25 budget flattens curves into octagons. Fix: pin
   `FILL_TOLERANCE = 0.001`. ADR 0008 §D.
2. **swash hinting at low ppem.** First fix improved 1080p but
   scale=8 zooms still showed staircase scallops. Root cause:
   "1 em = 1 world unit" means asking swash for outlines at ppem≈1,
   where TrueType hinting snaps every control point to the integer
   pixel grid. Scaling those snapped outlines up exposes the
   staircase. Fix: extract at `OUTLINE_PPEM = 1024` and
   post-multiply by `Affine::scale(scale / 1024)`. ADR 0008 §C.

The plan's §6 gotcha list pre-empted RaTeX's coordinate convention,
font-name brittleness, and macro-expansion edge cases. It did not
pre-empt either of these visual bugs — both are font/render-stack
defaults that bite specifically when world units are ≈ ems. Two
new entries in `docs/gotchas.md`.

### Misdiagnosis cost

The hinting bug took two passes to find. First diagnosis was
"y-flip is inverting contour winding and breaking NonZero fill,"
which would have been a real bug had it been true — it wasn't, the
glyph path doesn't go through that flip. Lesson: when a visual
artifact's mechanism isn't obvious, render *into the actual
intermediate stage* (BezPath dump, single-glyph snapshot at the
real ppem) before guessing. The single-frame API made that cheap;
without it, the back-and-forth would have been worse.

### Post-Step-5 cleanup pass (pre-Step-6)

Ran a `/simplify` review across the Slice E diff before starting Step 6.
Eight surgical fixes landed; worth recording the bug-class breakdown so
future slices know what to look for in their own end-of-slice review.

- **`compile_tex` swallowed `TexError` → silent blank render.** The eval
  helper returned `Vec::new()` on parse failure with a comment that
  "well-formed scenes only" was enforced upstream by the Python
  constructor — but the new `tex_render.rs` integration test bypasses
  Python entirely, so the contract was unenforceable. Fixed by
  changing the signature to `Result<Vec<Object>, TexError>` and
  panicking at the cache site (the only caller). Added
  `parse_error_surfaces_as_err` test to pin it.
- **`compile_tex` accepted any `Object`, returned empty for non-Tex.**
  Dead defensive branch — only caller was already inside an
  `if let Object::Tex` arm. Tightened to `compile_tex(src, color)`.
  Lesson: defensive returns *for documentation purposes* are still
  dead code; either enforce at the type level or `unreachable!`.
- **Tex cache key included `scale` and `macros` despite neither
  shaping geometry.** Two `Tex(...)` calls with same src+color but
  different `scale` were missing each other in cache. Tightened to
  hash `(src, color)` only via direct `blake3::Hasher::update` calls
  — also dropped the `serde_json::to_vec` allocation per cache
  lookup. Lesson: cache-key fields must be the *minimal subset that
  shapes the cached output*, not "everything that's on the type."
- **Font cache had a leak-then-lock race.** Concurrent cold misses
  could both `Box::leak` font bytes before either acquired the
  write lock; the loser's leak became permanent waste. Moved
  `write()` before `Box::leak`, re-checked under the lock. Same
  pattern as the Tex cache; should have been written this way the
  first time.
- **Font cache mapped poisoned-lock to None.** Conflated "this font
  id is unknown" with "the cache lock is poisoned" — the latter is
  a real bug surface. `.expect("...")` instead of `.ok()?`.
- **`tex_validate` held the GIL during RaTeX parse+layout.** Mirror
  of the existing `render_to_mp4` pattern — copy `&str` to `String`
  while we have the GIL, then `py.allow_threads` for the work. The
  pattern was established in Slice C but not applied here at
  introduction.
- **CLI `render` and `frame` duplicated 12 lines of scene-construction
  prelude.** Extracted `_resolve_dimensions` + `_build_scene_ir`
  helpers. Lesson: the second copy is the trigger to extract; we
  should have done it when adding `frame` rather than waiting for
  /simplify to flag it.
- **Misleading regex comment in `objects/tex.py`.** Claimed a
  `(?![A-Za-z])` lookahead was enforcing word-boundary; the regex
  has no lookahead, greedy `[A-Za-z]+` does the work. Comment fixed
  to describe the actual mechanism.

Across these eight, the recurring shape is **"feature added quickly,
guarded by a comment instead of a type/structure."** The comments
that papered over the gaps were all locally plausible — the bugs
only surfaced when something bypassed the documented contract (a
direct Rust caller, a cold-miss race, a parallel Python thread). For
Step 6+ slice work: when writing a "this can't happen because the
caller upstream validates" comment, prefer making it structurally
true (panic, narrower type, `unreachable!`) over hoping callers obey.

### Steps 6–9

Not started.

