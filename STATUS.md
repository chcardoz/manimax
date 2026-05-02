# Status

**Last updated:** 2026-05-02
**Current branch:** `chcardoz/montreal-v1`
**Current slice:** post-Slice-E refactor pass (no PR yet)

Twelve unmerged refactor commits on top of Slice E (#8, merged 2026-05-01). Tree is clean, no PR opened. All changes are internal cleanup — no IR, no public Python API, no schema changes. `cargo test --workspace` not re-run end-to-end this session, but `cargo test -p <crate>` was green for every Rust crate touched (eval 38/38, tex 9/9, raster 38/38 across 12 binaries, runtime all green, encode lib smoke, py builds). Python tests last run earlier this branch (136 passes).

Original seven-commit pass (2026-05-01):

- `e44a451` raster: tighten visibility, share `FillTessellator`, drop align helper
- `446386e` eval: extract shared `bezpath_to_verbs`, unify compile-cache helper across `tex.rs` / `text.rs` / `evaluator.rs`
- `9f146db` encode: hoist per-frame allocations; typed error variants for spawn/dims
- `3158850` text: hoist `ScaleContext`, simplify the katex_font lock, add `From` impls
- `7e8f493` tex: hoist `ScaleContext`, compose per-item affines into one pass
- `9138ed3` runtime: extract `RenderSetup`, propagate the png error chain
- `30ef0d2` python: share coerce helpers, animation base, test fixtures

Code-quality polish pass added today (`/code-quality` loop across all 8 Rust crates):

- `de02fa6` eval: exhaustive `match` for Tex/Text fan-out dispatch (was `if let / else if let / else` — silent passthrough on a future `Object` variant).
- `431bd5e` tex: `WORLD_UNITS_PER_EM` `pub(crate)` → private; `display_list_to_bezpath` drops `Option<(BezPath, Color)>` indirection.
- `34e0c4e` raster: `expand_stroke` carries between-segment state in a named `SegmentEnd` struct instead of a 5-tuple destructured 60 lines later.
- `d5d0e6b` runtime: `meta.background.map(f64::from)` replaces a 4-line manual array literal.
- `dc5345c` py: drop unused direct deps on `manim-rs-raster` / `manim-rs-encode` / `tracing` from `Cargo.toml` (raster + encode still in the dep tree via runtime; only the falsely-claimed direct fan-in goes).

The crates with no diff this pass (`manim-rs-ir`, `manim-rs-text`, `manim-rs-encode`) were already at the bar — the original seven-commit pass had covered the candidates.

## Next action

Open one or two PRs against `main` for the unmerged refactor pass. Natural splits:

- "Rust refactors" — eleven commits (`e44a451`, `446386e`, `9f146db`, `3158850`, `7e8f493`, `9138ed3`, plus today's `de02fa6` / `431bd5e` / `34e0c4e` / `d5d0e6b` / `dc5345c`).
- "Python simplify" — `30ef0d2` alone.

Verify `cargo test --workspace` and `pytest tests/python` clean before the PR. After merge, leading slice candidates: Tex snapshot harness mini-slice, Slice E.5 (SVG import), or Slice F (3D pipeline).

## Explicitly deferred (carried from prior session)

- **Tex snapshot harness** (Slice E Step 6 follow-up): corpus + coverage doc shipped, but the parametrized snapshot test, baseline PNGs, `--update-snapshots` flag, and pinned `TEX_SNAPSHOT_TOLERANCE` did not. Cross-platform tolerance pinning needs Linux/lavapipe CI runs. Tracked in `docs/public/contributing/porting-from-manimgl.md` "Tex coverage > Snapshot tolerance".
- **Baseline-PNG version of the Text render test.** Same blocker; the centroid-based check is the placeholder.
- **`text_to_bezpaths` returns `Vec<(BezPath, [f32; 4])>` with always-white color.** The pair shape is anticipatory parity with `display_list_to_bezpath`; the eval consumer discards the color. Revisit when a markup-aware Text variant arrives — collapse to `Vec<BezPath>` then, or wire per-glyph color through.
- **`__build_probe` returns the literal "manim_rs._rust: slice-c step 1".** Stale label but only `.startswith("manim_rs._rust")` is asserted; harmless. Update when the next slice touches the binding crate.

## Blockers

- None.
