# Slice E — text + math

**Status:** scoped, not started.
**Date:** 2026-04-28.
**Follows:** `slice-d.md` (shipped). Read Slice D's §11 retrospective before any step here.

Slice D shipped real strokes + the snapshot cache. The pipeline now renders multi-shape animated geometry end-to-end, but every "letter" is still a placeholder. Slice E adds the two content kinds that turn this into something an actual math-video author can use: plain text and LaTeX-flavored math. Both reduce to glyph outlines fed into the existing fill/stroke path; no new raster pipeline, no new IR shape beyond two new `MObjectKind` variants.

Ship criteria: **(a)** `Text("Hello, world")` renders correctly to mp4 with a bundled default font, no system font dependency; **(b)** `Tex(r"\sum_{i=1}^n i = \frac{n(n+1)}{2}")` renders correctly using a pure-Rust math typesetter, with no system LaTeX install required. (Per ADR 0009 the rgba pixel cache is gone; "second run hits cache" is no longer a ship criterion. The Tex `compile_tex` geometry cache and font cache — both keyed on source, not on `SceneState` — remain.)

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

Both produce mp4s where the rendered glyphs visibly match the source string and expression. Re-running the same command produces a deterministic mp4 (same bytes given same inputs) — but every run pays the full eval+raster+encode cost; the rgba pixel cache that previously made warm runs sub-second has been removed (ADR 0009).

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
| Pixel cache | **Removed mid-slice (ADR 0009).** Cold-render write cost dominated raster cost; warm wins were modest; on-disk store was GB-scale. Slice E no longer has a "cache hit" path to verify. | |
| Tex / font caches | Source-keyed, in-process. `compile_tex` caches `(src, color) → Vec<Object>` per `Evaluator`; `manim-rs-text` caches font-id → leaked bytes. Both survive the pixel-cache removal because they're keyed on the source they derive from, not on `SceneState`. | ADR 0008 §B; ADR 0009 consequences. |
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

### Step 6 — Tex coverage corpus + tolerance pinning

- `tests/python/tex_corpus.py`: 30–50 expressions covering: `\frac`, `\sqrt`, sub/superscripts, `\sum_{i=1}^n`, `\int_a^b`, `\lim_{x \to 0}`, `\prod`, `\binom`, `\pmatrix`/`\bmatrix`/`\vmatrix`, `\begin{aligned}`, `\begin{cases}`, accents (`\hat`, `\tilde`, `\bar`, `\vec`), Greek lowercase + uppercase, `\mathbb{R}`/`\mathcal{L}`/`\mathfrak{g}`, big delimiters (`\left( \right)`, `\left\| \right\|`), spacing (`\,`, `\quad`), `\text{...}` inside math.
- For each: snapshot test that uses `render_frame_to_png` (the single-frame API added mid-Step-5) to render one frame and tolerance-checks the rgba against a baseline PNG checked into `tests/python/snapshots/tex/`. Use the existing tolerance helper from Slice C/D — same max-Δ-per-channel + %-of-pixels-differing pair, no exact-pixel pins.
- **Pin the tolerance numbers in this step.** Pick values that pass on macOS-arm64 dev *and* Linux/lavapipe CI (ADR 0007) for every expression in the corpus, then bake them into a single `TEX_SNAPSHOT_TOLERANCE` constant in the test module so future changes have to consciously re-baseline rather than silently relax.
- Baselines are generated once with a `--update-snapshots` test flag (mirroring Slice D's pattern) and checked in. A bit-for-bit re-run on the same machine should succeed; cross-platform must succeed within tolerance.
- `docs/tex-coverage.md`: enumerate the supported subset, document known visible deltas vs. manimgl rendering, document `engine="latex"` as the future opt-in for full fidelity. Note the tolerance values + rationale at the bottom so re-baseliners know what they're loosening.

**Artifact:** `pytest tests/python/test_tex_corpus.py` green on macOS dev and Linux/lavapipe CI; coverage doc written; baseline PNGs + `TEX_SNAPSHOT_TOLERANCE` checked in.

### Step 7 — Python `Text()` via cosmic-text

- `crates/manim-rs-text`: extend with `text_to_bezpaths(src: &str, font: &[u8], size: f32, weight: Weight, align: Align) -> Vec<(BezPath, Color)>`. Uses `cosmic-text` for shaping/layout, `swash` for outlines.
- IR: `MObjectKind::Text { src, font: Option<PathBuf>, weight, size, color, align }`. `font: None` → bundled Inter Regular.
- `crates/manim-rs-eval`: `Text` eval mirrors the Tex shape that actually shipped — fan out to per-glyph `ObjectState`s at `Evaluator::eval_at` (not "produce a Text node with internal BezPaths"), with a per-`Evaluator` `(src, font_id, size, weight, align, color) → Vec<Object>` cache keyed on the geometry-shaping subset only. Same `Box::leak`-under-write-lock pattern as the font cache (per the §11 cleanup-pass note); same minimal-cache-key discipline (per the post-Step-5 cleanup pass — `scale` and other per-instance transforms must not be in the key).
- Python `python/manim_rs/objects/text.py`: `Text(src, *, font=None, weight="regular", size=1.0, color=WHITE, align="left")`.
- Apply the visual-bug pre-empts Step 5 paid for: extract glyph outlines at `OUTLINE_PPEM = 1024` and post-multiply by `Affine::scale(size / 1024)` (same fix as Tex; ADR 0008 §C). Reuse `FILL_TOLERANCE = 0.001` (ADR 0008 §D) — don't fall back to lyon defaults.
- GIL discipline: copy `&str` to `String` while holding the GIL, then `py.allow_threads` for shape+layout+outline (mirror the `tex_validate` cleanup-pass fix).

**Artifact:** `tests/python/test_text.py` — `Text("Hello")` round-trips through IR, renders to mp4, frame non-empty; tolerance-snapshot one frame against a checked-in baseline PNG using the same `TEX_SNAPSHOT_TOLERANCE` from Step 6. Bundled-font path works without any system font.

### Step 8 — Combined integration scene + determinism check

(Renamed from "+ cache verification" — the rgba pixel cache was deleted in ADR 0009; there's no cache hit/miss path to assert anymore.)

- `examples/text_scene.py`: a 3-second scene with a `Text(...)` greeting and a `Tex(...)` formula on screen simultaneously, both with non-default colors, one of them animated in opacity (proves time-evaluated state still works for the Text+Tex fan-out shape, alongside an animated transform on a non-glyph mobject).
- `tests/python/test_e2e_text_tex.py`:
  - Render: produces mp4 with expected duration / fps / dimensions / codec / pix_fmt via `ffprobe`.
  - **Determinism:** running the same command twice in a row on the same host produces byte-identical mp4. (No "warm < 1s" assertion — every run pays full cost now. The wall-clock floor is the encoder pipe + readback, not raster, per ADR 0009's perf table.)
  - Confirm the in-process `compile_tex` cache and font cache still work: a scene with the same Tex expression appearing twice should populate `compile_tex` once. Add a counter / probe behind a test-only feature flag rather than asserting on timing.
- Drop the `--no-cache` line; the flag was removed with the pixel cache.

**Artifact:** the two acceptance commands in §1 green; `ffprobe` clean; two consecutive runs byte-identical; `compile_tex` cache hit count == (Tex node count − unique source count) for a scene with intentional duplicates.

### Step 9 — ADRs, porting notes, performance log

ADR landscape changed since the original plan. What's already shipped:

- `0008-slice-e-decisions.md` — consolidated Slice E decisions (Tex fan-out at eval, per-`Evaluator` Tex cache, swash hinting at high ppem, pinned `FILL_TOLERANCE`, single-frame render API). Shipped with Steps 1–5. The originally-planned `0008-tex-via-ratex.md` was folded into this consolidated doc, matching Slice D's pattern (and superseding §10's "two ADRs" guidance).
- `0009-remove-pixel-cache.md` — pixel cache removal (replaces the originally-planned `0009-text-via-cosmic-text-swash.md` slot).
- `0010-in-process-encoder.md`, `0011-hardware-encoder-portability.md` — perf push, off-slice but consumed numbers from N15/N16 traces taken during Slice E.

Remaining Step 9 work:

- **`docs/decisions/0012-text-via-cosmic-text-swash.md`** (renumbered from the planned 0009 — that slot is now taken). Brief: standard Rust text stack, default font choice (Inter Regular bundled), override mechanism, why cosmic-text over alternatives. Note the `OUTLINE_PPEM = 1024` + `FILL_TOLERANCE = 0.001` reuse from Tex (ADR 0008 §C/§D) so future readers don't re-hunt.
- **`docs/decisions/0008-slice-e-decisions.md` addendum** — add a section recording what the slice plan got wrong about RaTeX upgrade triggers, bus-factor mitigation, and the `\newcommand` deferral, since those originally lived in the planned standalone Tex ADR. Keep brief.
- `docs/porting-notes/tex.md` — invariants from RaTeX + manimgl `Tex`. What `\textcolor` does. Coordinate-system flip. Per-`DisplayItem`-color override semantics. SHA cited.
- `docs/porting-notes/text.md` — cosmic-text vs. manimgl Pango: alignment semantics, default line-height, what's missing (RTL, Indic).
- `docs/tex-coverage.md` — written in Step 6; expand here with cross-links to ADR 0008 §C/§D (visual-bug fixes that shape what the corpus actually looks like) and to the tolerance-pinning rationale.
- `docs/performance.md` — append: wheel size delta from bundled fonts, RaTeX parse+layout cost vs. eval+raster cost (likely negligible), `compile_tex` and font-cache hit-rate observations on the corpus run, any new traces captured. Note that any cache-key cost observations now apply to the *source-keyed* caches (Tex geometry, fonts) only — the rgba pixel cache is gone (ADR 0009).
- `docs/gotchas.md` — already gained the two visual-bug entries from Step 5 (Lyon flatness, swash low-ppem hinting). Append any further Step 6/7 traps (e.g. cosmic-text font-database init cost per §6.7, RTL fallthrough behavior, snapshot-tolerance cross-platform skew).

**Artifact:** all docs written; §11 retrospective ready to fill on ship.

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
- **Distributed / S3 cache, parallel chunked render.** The local rgba pixel cache that these were once successors to is gone (ADR 0009). Any future raster-skip story should be designed fresh against `Runtime` caching (perf O1) and `eval_at`, not retrofitted onto the deleted cache.

---

## 5. Success Criteria

- [ ] `maturin develop` builds cleanly; `pytest tests/python` and `cargo test --workspace` all green. (Cargo line drops the `--exclude manim-rs-py` because the extension-module gate is now feature-flagged — see CLAUDE.md "Day-to-day".)
- [ ] Both commands in §1 produce `out.mp4`; running the same command twice in a row produces byte-identical output.
- [ ] `ffprobe out.mp4` reports expected dimensions / fps / codec / pix_fmt for both commands.
- [ ] Visually: `Text` renders a recognizable string; `Tex` renders the formula with correct fraction, sum, sub/superscript layout; both at expected colors.
- [ ] No system LaTeX or system font installed on the CI runner; both commands still pass.
- [ ] ~~Tex coverage corpus (30–50 expressions) all snapshot-stable with a single pinned `TEX_SNAPSHOT_TOLERANCE` constant; baselines green on macOS-arm64 dev *and* Linux/lavapipe CI.~~ **Deferred** (2026-04-29). Step 6 shipped the corpus data + coverage doc only; the snapshot harness, baseline PNGs, `--update-snapshots` flag, and pinned tolerance constant did not land. Tracked in `docs/tex-coverage.md` "Snapshot tolerance" and `STATUS.md`. Visual review uses `python -m manim_rs frame` ad-hoc until the harness lands as its own slice or a Slice E.5 task.
- [ ] `Tex(src, macros={...})` expands no-arg macros end-to-end.
- [ ] In-process `compile_tex` and font caches measurably hit on a Tex scene with duplicate sources (verified via test-only probe, not timing).
- [ ] `0012-text-via-cosmic-text-swash.md` written; `0008-slice-e-decisions.md` addended with the originally-Tex-only items (RaTeX bus-factor, upgrade triggers, `\newcommand` escalation path).
- [ ] `docs/tex-coverage.md` enumerates supported subset and known deltas vs. manimgl.
- [ ] `docs/porting-notes/{tex,text}.md` written.
- [ ] §11 retrospective filled before hand-off.

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
    src/lib.rs                 # eval → raster → encode, no cache hop (ADR 0009)
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
  decisions/0008-slice-e-decisions.md   # consolidated, already shipped — Step 9 adds Tex bus-factor / upgrade-trigger addendum
+ decisions/0012-text-via-cosmic-text-swash.md   # 0009/0010/0011 taken (cache removal + encoder push)
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

- **Single consolidated ADR per slice** worked well in D. The plan originally called for *two* Slice E ADRs (Tex + Text); reality consolidated everything Tex-shaped into `0008-slice-e-decisions.md`, matching D. Step 9 picks up the Text ADR as `0012-text-via-cosmic-text-swash.md` (0009 was repurposed for pixel-cache removal, 0010/0011 for the encoder push).
- **~~Cache key shape~~ → cache layer removed.** Slice D's blake3-of-canonical-serde-bytes pixel cache was deleted mid-Slice-E (ADR 0009): cold-render write cost dominated raster, warm wins were ~40 %, on-disk store was GB-scale. Slice E inherits the *replacement* discipline instead — source-keyed in-process caches (Tex geometry, font bytes) that derive from immutable inputs, not from `SceneState`. Cache-key minimality (post-Step-5 cleanup pass: `compile_tex` key shrank to `(src, color)` after `scale`/`macros` were caught spuriously included) is the lasting lesson, applied to those caches.
- **Snapshot-test rebaselining.** Tolerance-based, no exact-pixel pins. Same as D. Step 6 pins a single `TEX_SNAPSHOT_TOLERANCE` constant for the whole corpus rather than per-expression knobs.
- **"Expose to Python + use in test" collapsed per step.** Steps 4 and 5 each leave the surface usable from the language layer they target. Confirmed by ship.
- **Pinned-SHA discipline.** RaTeX SHA pinned in Cargo.toml; if it advances mid-slice, re-pin and re-verify the corpus before merge.
- **Single-frame render API as a debugging primitive.** Mid-Step-5 detour shipped `render_frame_to_png` (ADR 0008 §F) inline rather than parking. Step 6's tolerance baselining and Step 7's Text snapshot reuse it directly — the cost was repaid before Step 5 ended.

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

### Step 6 (data + doc; harness deferred)

- **Plan vs reality.** The plan called for a full snapshot harness
  parametrized over `tex_corpus.py` with a pinned
  `TEX_SNAPSHOT_TOLERANCE` baselined cross-platform. What shipped:
  the corpus data and the coverage doc — but no harness, no
  baseline PNGs, no `--update-snapshots`, no tolerance constant. The
  harness was deferred to its own follow-up because picking a
  cross-platform tolerance requires CI runs against an actual lavapipe
  target, which became its own scope. Documented in
  `docs/tex-coverage.md` "Snapshot tolerance" and §5 above.
- **Lesson:** "build a corpus" and "build a corpus harness" are two
  steps, not one. The corpus is a written artifact (33 entries,
  picked by distinct rendering machinery); the harness is a CI-shape
  decision that interacts with ADR 0007. Conflating them was the
  plan-side mistake — a future slice planning a snapshot pass should
  separate the two.

### Step 7 (Text — IR through Python)

- **Tracked the plan tightly** through S7a → S7e. Sub-steps were
  added during execution (the slice plan listed Step 7 as one
  bullet; the actual landing went S7a font plumbing, S7b cosmic-text
  adapter + IR, S7c eval fan-out + cache, S7d Python constructor,
  S7e end-to-end render). The decomposition was natural, mirrored
  the Tex sequence, and matched the working-rhythm note in
  CLAUDE.md (one step at a time).
- **Cache pattern.** Plan said `RwLock + Box::leak` (the §10
  carry-over from the Tex font cache). Reality: Tex actually shipped
  `Mutex<HashMap<blake3::Hash, Arc<Vec<Arc<Object>>>>>` because
  `Box::leak` was never the actually-deployed shape — it was an
  aspirational note from §11 that didn't survive contact. Text
  matched Tex's actual shape rather than the plan's. Lesson:
  re-read the *current* code, not the slice plan, before mirroring
  a pattern called out in a retrospective.
- **No Python `_rust.text_validate`.** The plan didn't require it,
  but it was natural to ask "do we want a parallel to
  `_rust.tex_validate`?" Answer: no. cosmic-text accepts any
  UTF-8; there's nothing to validate beyond argument shape (size,
  weight, align — all checked in Python). LaTeX has parse failures
  worth surfacing at construction; UTF-8 strings don't.
- **Stale `_rust` extension.** S7e initially panicked because the
  loaded extension was the pre-S7c build. Caught fast in retrospect
  — `cargo test` was green, but `cargo test` doesn't rebuild the
  pyo3 extension; only `maturin develop` does. New entry in
  `docs/gotchas.md`.

### Step 8 (E2E + determinism + cache probe)

- **Cache probe via Rust integration tests, not pyo3 counters.**
  The plan suggested "a counter / probe behind a test-only feature
  flag." Reality: the cleanest probe is `Arc::ptr_eq` on the
  fan-out children of two Tex (or Text) instances sharing a source.
  No pyo3 surface change, no test-only feature flag, no
  conditionally-compiled instrumentation. Three tests in
  `crates/manim-rs-eval/src/lib.rs` cover it
  (`tex_cache_returns_same_arc_on_repeat`,
  `duplicate_tex_sources_share_compiled_geometry`,
  `duplicate_text_sources_share_compiled_geometry`). Lesson: when a
  test wants to assert "X and Y refer to the same compiled object,"
  prefer pointer-identity assertions over counters.
- **Determinism is real and clean.** Three byte-determinism tests
  in `tests/python/test_e2e_text_tex.py` cover Tex, Text, and a
  combined Tex+Text+Polyline scene. All pass. The eval (HashMap
  iteration over `BTreeMap`-ordered IR), cosmic-text shaping,
  swash outlines, lyon tessellation, and in-process libx264 are
  all deterministic in the current configuration. Logged in
  `docs/performance.md` E7 with a note that the test is the canary
  if a future change introduces nondeterminism.
- **No determinism issues surfaced from the eval-time cache fan-out.**
  The plan flagged HashMap iteration order as a likely culprit;
  reality is that the cache *output* (the `Vec<Arc<Object>>`) is
  produced by deterministic code paths (RaTeX layout, cosmic-text
  shape, swash outline) and stored under a content-addressed key.
  The HashMap's iteration order is irrelevant because we never
  iterate it — we look up by key. Worth recording so the next
  cache discussion doesn't relitigate this.

### Step 9 (this commit)

- **Plan vs reality.** Step 9's deliverables matched the plan: ADR
  0012, ADR 0008 addendum, `docs/porting-notes/{tex,text}.md`,
  performance/gotchas/tex-coverage appends, retrospective fill, and
  STATUS.md. The plan got the renumbering right (0012 not 0009;
  0009/0010/0011 already taken).
- **Lesson:** the "two ADRs" guidance in §10 of the slice plan was
  already wrong by the time Step 9 ran (consolidated 0008 ate the
  Tex-side ADR, ADR 0012 ate the Text-side). The slice plan's
  carry-over notes are a useful guide *as of when they were
  written*; check ADR numbering against the actual `decisions/`
  directory before pinning a number.

