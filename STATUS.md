# Status

**Last updated:** 2026-04-28
**Current slice:** Slice E — Steps 1–5 shipped, Steps 6–9 remaining.
Plan: `docs/slices/slice-e.md`. ADRs through `0008`. Slice D shipped.

## Last session did

Slice E Step 5 (Python `Tex` mobject + macro pre-expansion +
`tex_validate`), plus a mid-slice detour adding the single-frame
render API (`render_frame_to_png` / `render_frame` / `frame` CLI
subcommand), plus two visual fixes (lyon `FILL_TOLERANCE = 0.001`
and swash `OUTLINE_PPEM = 1024`). Closed with a `/simplify` review
that landed eight surgical fixes (compile_tex error chain, font
cache race, GIL during tex_validate, cache-key narrowing, CLI
dedup, etc.).

Full session details in:

- `docs/decisions/0008-slice-e-decisions.md` — design decisions A–F.
- `docs/slices/slice-e.md` §11 — retrospective, including the
  /simplify cleanup pass bug-class breakdown.
- `docs/gotchas.md` — two new entries (low-ppem hinting, lyon
  default tolerance).
- `docs/performance.md` — Slice E observations E1–E3, plus E3a/b/c
  (PyRuntime PyClass, tracing instrumentation, error-chain at the
  pyo3 boundary).
- `docs/future-directions.md` — new file: architectural
  watchpoints with concrete triggers.

`cargo test --workspace` and `pytest tests/python` (111 tests)
green.

## Next action

**Slice E Step 6** — Tex coverage corpus + snapshot pinning.
Pinned LaTeX expressions exercising fractions, Greek + AMS
symbols, `\textcolor`, every KaTeX font face. Per-entry
single-frame render + tolerance snapshot. Plan: `slice-e.md` §3
Step 6.

Pre-Step-6 prep:

1. Pick tolerance type (per-pixel max, mean delta, or SSIM) and
   confirm stable across two warm runs.
2. Decide baseline storage strategy
   (`UPDATE_TEX_SNAPSHOTS=1`-style regen flag preferred over
   git-tracked image bytes).
3. Re-skim `docs/gotchas.md` for the two Slice E entries before
   diagnosing any visual artifact.

Working rhythm: one step at a time, rewrite STATUS.md at
end-of-session.

## Blockers

- None.
