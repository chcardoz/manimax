# IR Schema — v1 (Slice B → Slice C)

**Status:** active. Slice C grew the surface additively; `SCHEMA_VERSION` is still **1**. No field changed meaning, so no bump.
**Wire format:** at the FFI boundary we pass a Python value (msgspec `Struct` → `msgspec.to_builtins` → `pythonize` → serde). On disk and for the schema-drift guard the representation is JSON (`serde_json` ↔ `msgspec.json`).
**Encoding principles:**

- **Internally tagged unions.** Every sum type carries its discriminator as a named field (`op`, `kind`) flattened into the payload. The only form both `serde` and `msgspec` support natively.
- **Strict schemas.** Every serde struct uses `#[serde(deny_unknown_fields)]`; every msgspec struct uses `forbid_unknown_fields=True`. Drift fails loudly at deserialize time.
- **Required fields, typed absence.** Most fields are required. Where a value is genuinely optional (`stroke`, `fill`), the field is still required on the wire — its value may be `null`. Matches the `Option<T>` shape without handing an escape hatch to the encoder.
- **Schema version is stored.** `Scene.metadata.schema_version = 1`. Evaluator rejects versions it does not recognize.

---

## Scalar types

| Logical type | Wire | Rust | Python |
|---|---|---|---|
| `Time` | JSON number | `f64` | `float` — seconds, fps-independent |
| `ObjectId` | JSON number | `u32` | `int` — stable within a scene, assigned at construction |
| `Vec3` | JSON array of 3 numbers | `[f32; 3]` | `tuple[float, float, float]` — 3D point, matches manimgl idiom; Slice B's rasterizer drops z |
| `RgbaSrgb` | JSON array of 4 numbers | `[f32; 4]` | `tuple[float, float, float, float]` — sRGB floats in `[0, 1]`, matches manimgl's `color_to_rgba` |
| `Resolution` | `{"width": u32, "height": u32}` | named struct | named struct |

Color space note: values are stored in **sRGB** space (no gamma correction on the wire), matching manimgl. The wgpu framebuffer is `Rgba8UnormSrgb`, which gamma-encodes on write, so GPU math happens in linear space while IR and user-facing API stay sRGB.

---

## Scene

```jsonc
{
  "metadata": {
    "schema_version": 1,
    "fps": 30,
    "duration": 2.0,
    "resolution": { "width": 480, "height": 270 },
    "background": [0.0, 0.0, 0.0, 1.0]
  },
  "timeline": [ /* TimelineOp[] */ ],
  "tracks":   [ /* Track[] */ ]
}
```

- `metadata.fps`: `u32`. Target output framerate.
- `metadata.duration`: `Time` seconds. Total scene length.
- `metadata.resolution`: output raster size.
- `metadata.background`: `RgbaSrgb`.
- `timeline`: ordered list of object lifecycle operations.
- `tracks`: animated property values.

---

## Stroke and Fill

Every geometry variant carries an `Option<Stroke>` and an `Option<Fill>`. Both fields are required on the wire; `null` denotes absence. A shape with `stroke: null, fill: null` has no visible surface (legal but renders nothing).

```jsonc
// Stroke
{ "color": [r, g, b, a], "width": 0.04 }
```

- `width` is in **scene units**, not pixels. Camera is hardcoded at `[-8, 8] × [-4.5, 4.5]` in Slice B.

```jsonc
// Fill
{ "color": [r, g, b, a] }
```

---

## Object

Internally tagged union with discriminator `"kind"`.

### Polyline

```jsonc
{ "kind": "Polyline",
  "points": [[x, y, z], ...],          // Vec3[]
  "closed": true,                       // bool
  "stroke": { /* Stroke | null */ },
  "fill":   { /* Fill   | null */ }
}
```

- `closed: true` — the renderer connects the last point back to the first. Explicit rather than manimgl's duplicate-first-point convention.

### BezPath

A sequence of SVG/lyon-style path verbs. Slice C ships the shape; tessellation is wired up in a later step.

```jsonc
{ "kind": "BezPath",
  "verbs": [ /* PathVerb[] */ ],
  "stroke": { /* Stroke | null */ },
  "fill":   { /* Fill   | null */ }
}
```

`PathVerb` is itself an internally tagged union with discriminator `"kind"`:

| `kind` | Fields | Meaning |
|---|---|---|
| `MoveTo` | `to: Vec3` | Start a new sub-path at `to`. |
| `LineTo` | `to: Vec3` | Straight segment to `to`. |
| `QuadTo` | `ctrl: Vec3`, `to: Vec3` | Quadratic Bézier. |
| `CubicTo` | `ctrl1: Vec3`, `ctrl2: Vec3`, `to: Vec3` | Cubic Bézier. |
| `Close` | — | Close the current sub-path. |

Future variants (Slice C+): `Circle`, `Text`, `Image`. Each is a new `Object` `kind`.

---

## TimelineOp

Internally tagged union with discriminator `"op"`. Ordered by `t` ascending; renderer assumes sorted input.

```jsonc
{ "op": "Add",    "t": 0.0, "id": 1, "object": { /* Object */ } }
{ "op": "Remove", "t": 2.0, "id": 1 }
```

An object is **active at time `t`** iff an `Add` with that id occurs at time `≤ t` and no subsequent `Remove` with that id occurs in `(add_t, t]`. Re-adding the same id after a remove is permitted; each activation interval is independent.

`Set`, `Reparent`, `Label`, `CameraSet` are deferred.

---

## Track

Internally tagged union with discriminator `"kind"`. One track per animated property per object; multiple segments within a track describe the time-varying value. Multiple tracks of the same `kind` may reference the same `id`; their contributions sum.

Every track variant has the same shape:

```jsonc
{ "kind": "<variant>", "id": 1, "segments": [ /* Segment[] */ ] }
```

Every segment carries `t0: Time`, `t1: Time`, `from`, `to`, `easing: Easing`. The value type of `from`/`to` is per-segment kind.

| Track `kind` | Segment type | Value type | Notes |
|---|---|---|---|
| `Position` | `PositionSegment` | `Vec3` | Offset added to object base position. |
| `Opacity` | `OpacitySegment` | `f32` | Multiplicative, default 1.0. |
| `Rotation` | `RotationSegment` | `f32` (radians) | Matches manimgl / numpy convention. |
| `Scale` | `ScaleSegment` | `f32` | Uniform scale; 1.0 is identity. Per-axis scale deferred. |
| `Color` | `ColorSegment` | `RgbaSrgb` | Stroke/fill tint; lerped componentwise. |

Segment rules (all tracks):

- `id` references an object that must be active throughout `[t0, t1]`.
- Segments within a single track have `t0 < t1` and must not overlap. Gaps are allowed — the value holds at the last segment's `to`.
- Before the first segment, the value is the implicit default (zero for Position, 1.0 for Opacity / Scale, 0.0 for Rotation, object's authored color for Color).

### Easing

All 15 manimgl rate functions. Every variant carries `"kind": "<name>"`. Parameterless variants are empty structs so `deny_unknown_fields` / `forbid_unknown_fields` still reject extras (serde silently tolerates extras on unit variants under an internal tag).

| `kind` | Parameters | Equivalent manimgl function |
|---|---|---|
| `Linear` | — | `linear(t) = t` |
| `Smooth` | — | `smooth(t)` — bezier(0,0,0,1,1,1) |
| `RushInto` | — | `2 · smooth(t/2)` |
| `RushFrom` | — | `2 · smooth((t+1)/2) − 1` |
| `SlowInto` | — | `sqrt(1 − (1−t)²)` |
| `DoubleSmooth` | — | stitched `smooth` at `t=0.5` |
| `ThereAndBack` | — | `smooth(min(2t, 2−2t))` |
| `Lingering` | — | `squish(Linear, 0.0, 0.8)` |
| `ThereAndBackWithPause` | `pause_ratio: f32` | plateau in the middle of the segment |
| `RunningStart` | `pull_factor: f32` | bezier(0,0,pf,pf,1,1,1) |
| `Overshoot` | `pull_factor: f32` | bezier(0,0,pf,pf,1,1) |
| `Wiggle` | `wiggles: f32` | `there_and_back(t) · sin(wiggles·π·t)` |
| `ExponentialDecay` | `half_life: f32` | `1 − exp(−t / half_life)` |
| `NotQuiteThere` | `inner: Easing`, `proportion: f32` | `proportion · inner(t)` |
| `SquishRateFunc` | `inner: Easing`, `a: f32`, `b: f32` | `inner((t−a)/(b−a))` clamped outside `[a, b]` |

`NotQuiteThere` and `SquishRateFunc` are recursive — `inner` is itself an `Easing`, encoded with its own `kind` tag.

---

## Validation rules (evaluator contract)

The evaluator treats invalid IR as a hard error. Producers (the Python scene API) are responsible for emitting valid IR; the evaluator does not silently correct.

1. `schema_version` must equal 1.
2. `timeline` is sorted non-decreasing by `t`.
3. Every `Remove` op references an id currently active.
4. Every track's `id` refers to an object that `Add`'s at some point.
5. Every track's segments are sorted, non-overlapping, with `t0 ≤ t1` (`t0 == t1` is a zero-duration jump — legal and evaluates to `to`).
6. `duration` ≥ the latest timestamp referenced by any op or segment.
7. `fps` ≥ 1. `resolution.{width,height}` ≥ 1.

---

## What's not here

Deliberately out of scope for v1:

- Non-Polyline / non-BezPath geometry (circles, text, surfaces).
- Fill pipeline (Stroke renders; Fill is represented in the IR but the rasterizer does not yet draw it).
- `Set`, `Reparent`, `Label`, `CameraSet` timeline ops.
- Scene graph / parenting.
- Multi-camera, 3D camera.
- Chunked rendering metadata, cache keys.

Each lands as an additive change (new `kind`, new `op`, or new top-level field) in a later slice. `schema_version` bumps when an existing field changes meaning.
