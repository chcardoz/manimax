# Porting note: transforms (animations)

**Status:** Slice C port. Pairs with the evaluator track semantics in
`docs/ir-schema.md` and `crates/manim-rs-eval/src/lib.rs`.
**Stub label:** none — this is the shipping Python authoring surface.
**Manimgl reference:** `reference/manimgl/manimlib/animation/*` (notably
`animation.py`, `transform.py`, `fading.py`).

## What manimgl does

`Animation` in manimgl is an active object: subclasses override
`interpolate(alpha)` (plus `begin()`/`finish()`) and mutate a `Mobject` in
place each frame. The scene's render loop calls `interpolate(alpha)` with
`alpha = rate_func((t - start) / duration)` and then re-renders the scene from
its new state. Animations compose by sequential or parallel scheduling; the
canonical transforms live in `transform.py` (`ApplyMethod`, `Rotate`,
`FadeIn`, `FadeOut`, `Transform`, etc.) and default to `rate_functions.smooth`.

## What Slice C does instead

Animations in `python/manim_rs/animate/transforms.py` are **inert
descriptions** of time-varying value tracks. Each one has:

- a target object that has already been `scene.add()`'d (so it has an `_id`);
- a `duration` (seconds);
- an optional `easing: ir.Easing`;
- an `emit(t_start)` method that returns a `list[ir.Track]`.

The scene calls `emit(t_start)` at compile time, collects the returned tracks,
and hands them to the Rust evaluator. All interpolation happens in Rust.

## Invariants and divergences from manimgl

These are the bits that bite a porter who reads manimgl and reaches for the
same behaviour here.

- **Default easing is `Linear`, not `Smooth`.** `_default_easing()` returns
  `ir.LinearEasing()`. Manimgl's `Animation` defaults to `rate_functions.smooth`.
  Opinionated but deliberate — keeps the default behaviour predictable for
  authored scenes; pass `easing=ir.SmoothEasing()` to match manimgl.
- **`Translate(obj, delta, duration)` emits a segment `(0,0,0) → delta`.**
  Evaluator adds the position-track value to the object's base position from
  `AddOp`, so `Translate` means "offset relative to where the object was born".
  Manimgl's `ApplyMethod(obj.shift, delta)` is shape-equivalent but applies
  by mutation — our version is not stateful.
- **`ScaleBy`, not `ScaleTo`.** The emitted segment is `from=1.0, to=factor`
  and scale tracks compose *multiplicatively* across all active tracks on an
  object, so `ScaleBy(obj, 2.0)` then `ScaleBy(obj, 1.5)` lands at 3× the
  authored size. An absolute-target verb (override semantics, like `Colorize`)
  can be added later as a separate primitive if needed.
- **`Colorize` is an override, not a tween of the authored color.**
  Color-track semantics in the evaluator are "last-write override" —
  the active color-track sample *replaces* the authored object color for the
  current frame, it does not blend with it. So `Colorize` authors an explicit
  `from_color` *and* `to_color`; both are required. The segment interpolates
  between the two override values, and on frame 0 the displayed color is
  `from_color`, not the object's authored stroke/fill color. This is a
  deliberate simplification — it sidesteps evaluator-side color snapshotting.
  If you want "fade from current color to red", pass the current color as
  `from_color` at scene-build time.
- **`FadeIn` / `FadeOut` are `OpacityTrack` segments `0→1` / `1→0`.**
  They compose multiplicatively with the object's authored opacity (default
  1.0). There's no analogue of manimgl's `FadeInFromPoint` / `FadeInFrom`.
- **Target must already be in the scene.** `_require_id` raises if the
  target object's `_id` is `None`. Manimgl would accept an un-added mobject
  and add it during `play()`; here, ordering is explicit.
- **Duration must be strictly positive.** `_check_duration` rejects
  `0.0` and negative values. Zero-duration jumps must be authored as
  `ir.*Segment(t0=t, t1=t, …)` directly; the high-level transforms don't
  expose that path.

## Public API surface

| Transform | Track kind emitted | From → To | Notes |
|---|---|---|---|
| `Translate(obj, delta, duration, *, easing=None)` | `PositionTrack` | `(0,0,0) → delta` | Offset relative to base position. |
| `Rotate(obj, angle, duration, *, easing=None)` | `RotationTrack` | `0.0 → angle` | Radians; manimgl convention. |
| `ScaleBy(obj, factor, duration, *, easing=None)` | `ScaleTrack` | `1.0 → factor` | Multiplicative; composes across tracks. |
| `FadeIn(obj, duration, *, easing=None)` | `OpacityTrack` | `0.0 → 1.0` | |
| `FadeOut(obj, duration, *, easing=None)` | `OpacityTrack` | `1.0 → 0.0` | |
| `Colorize(obj, from_color, to_color, duration, *, easing=None)` | `ColorTrack` | `from → to` (override) | Both colors required; replaces authored. |

All six satisfy the `Animation` protocol: they expose `duration: float` and
`emit(t_start: float) -> list[ir.Track]`.

## What Slice D / later will do

- Composition primitives: `AnimationGroup` (parallel), `Succession` (serial),
  `LaggedStart`.
- Per-axis scale (once `Scale` track grows from `f32` to `Vec3`).
- `TransformMatchingShapes` / `Transform` equivalents — these are the
  shape-morph animations that drive most manimgl videos. They require
  mobject-to-mobject correspondence, not just track emission.
- A `from_=None` sentinel on `Colorize` that resolves to the object's
  authored color at compile time, removing the "must specify both ends"
  awkwardness.

## Files touched in Slice C

- `python/manim_rs/animate/transforms.py` — the six transforms above.
- `python/manim_rs/animate/__init__.py` — re-exports.
- IR track variants in `python/manim_rs/ir.py` and
  `crates/manim-rs-ir/src/lib.rs`.
- Evaluator segment machinery in `crates/manim-rs-eval/src/lib.rs`
  (especially `latest_segments` for the color-override rule).
