# Status

**Last updated:** 2026-04-22
**Current slice:** Slice C — **shipped.** All eight steps green (`docs/slices/slice-c.md`).

## Last session did

- **Step 8 landed: consolidated ADR + porting notes + retrospective.**
  - `docs/decisions/0004-slice-c-decisions.md` — five sub-decisions (pythonize FFI, `BezPath` primitive, all-tracks-all-easings, fill+MSAA pair, tolerance snapshots). Marked 0001 `superseded by 0004 §A`.
  - `docs/porting-notes/rate-functions.md` — 15-easing table, f32 precision caveat, composition rules per track kind.
  - `docs/porting-notes/fill.md` — `lyon::FillTessellator` + non-zero winding + MSAA-for-AA deltas vs manimgl's Loop-Blinn fill.
  - `docs/porting-notes/scene-discovery.md` — `extract_scene.py` port (kept: class lookup; dropped: `--write-all`, interactive prompt, `compute_total_frames`).
  - `docs/gotchas.md` — new entries for H.264/yuv420p chroma shift and MSAA resolve-target format/dim pairing; tolerance-snapshot rule appended to the existing platform-pinned entry.
  - `docs/slices/slice-c.md §11` — retrospective filled (plan deltas, surprising calls, missed §6 gotchas, process observations, Slice D hand-off notes).
- **Nine Slice C commits on branch `chcardoz/wellington`** (ahead of `origin/main` by 14 total including earlier scaffolding). Branch is unpushed.

Totals: **Rust 53 passed / 0 failed**, **Python 86 passed / 0 failed** (unchanged from Step 7).

## Next action

Open to whatever's next. Natural candidates:

- **Push + PR** `chcardoz/wellington` → `main`. 14 commits, clean working tree.
- **Slice D kick-off.** Real Bézier stroke port (`manimlib/shaders/quadratic_bezier/stroke/*`) with per-vertex width + AA; snapshot cache. See `slice-c.md §10` for the natural sequence and §11's Slice D deltas.
- **Cross-platform wheels** (parallel workstream, `slice-c.md §9`). Unblocked now that snapshots are tolerance-based.

## Blockers

None.

## Notes for next session

- The single Slice C commit block passed pre-commit cleanly after four small fix-ups (ruff UP007, ruff B008×2, cargo fmt on IR + eval). All folded into their respective commits; no fixup commits.
- `rate_functions.py` at `c5e23d9` — if the submodule advances and upstream adds easings, our enum drifts silently. Slice D planning should re-check.
- The perf log (`docs/performance.md`) gained entries during this slice but was not acted on; it's a batch target for a future perf pass.

## Convention for updating this file

- **Rewrite, don't append.** This file is current-state, not history. Git log is the history.
- Update at the end of every session *before* handing back to the user.
- Keep it under ~50 lines. If it's growing, state is leaking in that should be in `docs/slices/<slice>.md` checkboxes or a porting note.
- Three required sections: **Last session did**, **Next action**, **Blockers**. Everything else is optional.
