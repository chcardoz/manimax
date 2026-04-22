# Porting note: rate functions (easings)

**Manimgl source:** `reference/manimgl/manimlib/utils/rate_functions.py` at commit `c5e23d9`.
**Rust port:** `crates/manim-rs-eval/src/lib.rs` — `apply_easing`, `smooth`, `squish`, `bezier_scalar`.
**IR surface:** `crates/manim-rs-ir/src/lib.rs` — `Easing` enum; `python/manim_rs/ir.py` mirrors.

## Public API

Fifteen named easings selectable per track segment in the IR:

```
Linear, Smooth, RushInto, RushFrom, SlowInto, DoubleSmooth,
ThereAndBack, ThereAndBackWithPause { pause_ratio },
RunningStart { pull_factor }, Overshoot { pull_factor },
NotQuiteThere { inner, proportion }, Wiggle { wiggles },
SquishRateFunc { inner, a, b }, Lingering, ExponentialDecay { half_life }.
```

Python authors pass either a friendly alias (`Smooth`, `Overshoot(...)`) or a raw `ir.Easing` struct. Parameterised easings carry defaults matching manimgl.

## Invariants

- Each easing is a pure `fn(alpha: f32) -> f32` with no side-state. `alpha` is the segment-local parameter in `[0, 1]`.
- Variants with parameters are **struct variants** — not unit variants with extra fields — because serde's `deny_unknown_fields` is silent on unit variants under the internal tag `kind`. Enforced by ADR 0002; see `docs/gotchas.md`.
- `NotQuiteThere` and `SquishRateFunc` carry a boxed `inner: Easing`, so they recurse. The evaluator caps nothing; pathological deep nesting is a user bug, not a port gap.
- `bezier_scalar` is a 1-D De Casteljau over `&[f32]` — used by `RunningStart` and `Overshoot`. Manimgl's `bezier()` returns a closure; we inline the evaluation at the call site.

## Edge cases

- **`SquishRateFunc { a, b }` with `a == b`.** Manimgl returns `a` (i.e. `func(0)` at a, `func(1)` at b, ambiguous when equal). Port matches — the zero-width window collapses to the inner's value at `0`. Avoid `a == b` in authored scenes; the result is surprising.
- **`ExponentialDecay { half_life }` never hits 1.** At `alpha = 1.0` the output is `1 - exp(-1/half_life)`, e.g. `0.99995` at `half_life = 0.1`. Manimgl's comment admits this ("cut-off error at the end"); port preserves the bias rather than clamping.
- **`Wiggle` is not monotonic.** It oscillates; final value is always `0` (track end undoes the motion). Use it for position/rotation, not opacity, or opacity will end where it started after a visible wobble.
- **`ThereAndBack` ends at 0.** Same shape-return caveat as `Wiggle`.
- **f32 precision on round-trip tests.** `ThereAndBackWithPause(pause_ratio=1.0/3.0)` drops precision across the Python f64 → serde f32 → msgspec f64 round trip; tests use dyadic rationals. See `docs/gotchas.md`.

## Manimax mapping

| manimgl fn | IR variant | Port flavour |
|---|---|---|
| `linear` | `Linear {}` | identity, trivial |
| `smooth` | `Smooth {}` | verbatim algebra |
| `rush_into` / `rush_from` | same | verbatim |
| `slow_into` | same | verbatim (`sqrt(1-(1-t)^2)`) |
| `double_smooth` | same | verbatim branch |
| `there_and_back` | same | verbatim |
| `there_and_back_with_pause` | struct variant with `pause_ratio` | verbatim, default `1/3` |
| `running_start` | struct variant with `pull_factor` | verbatim via `bezier_scalar` |
| `overshoot` | struct variant with `pull_factor` | verbatim via `bezier_scalar` |
| `not_quite_there` | struct variant with `{ inner, proportion }` | reimplemented (manimgl returns a closure; Rust enum carries the inner by `Box<Easing>`) |
| `wiggle` | struct variant with `wiggles` | verbatim (`sin` from `std::f32`) |
| `squish_rate_func` | struct variant with `{ inner, a, b }` | verbatim, `a == b` corner-case preserved |
| `lingering` | `Lingering {}` | literal expansion of `squish_rate_func(linear, 0, 0.8)` — no boxed inner |
| `exponential_decay` | struct variant with `half_life` | verbatim, default `0.1` |

Composition rule per track kind (in `apply_easing`'s *callers*, not the functions themselves):

- **Position / rotation:** sum contributions from all tracks at `t`.
- **Opacity / scale:** multiply contributions.
- **Color:** take the latest-starting segment's current value.

These rules are evaluator policy, not part of the easings themselves; documented here because they interact with `ThereAndBack` / `Wiggle` semantics (non-monotonic easings compose differently under sum vs product).

## What didn't port

- Manimgl's `bezier()` factory in `manimlib/utils/bezier.py`. We re-implement just the scalar case inline in `bezier_scalar`. A full port would pay for itself when we port the Bézier stroke shader in Slice D.
- Manimgl's `Callable`-typed easings (the closure returned by `not_quite_there` / `squish_rate_func`). Rust enum form is strictly more constrained but round-trips cleanly.
