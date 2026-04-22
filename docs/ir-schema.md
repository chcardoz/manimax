# IR Schema — v1 (Slice B)

**Status:** active. Describes the minimum surface needed by Slice B.
**Wire format:** JSON (via `serde_json` in Rust, `msgspec.json` in Python). Slice B uses a JSON string at the FFI boundary; Slice C will swap to `pythonize`/`FromPyObject` without changing the schema.
**Encoding principles:**

- **Internally tagged unions.** Every sum type carries its discriminator as a named field (`op`, `kind`) flattened into the payload. Chosen because it is the only form both `serde` and `msgspec` support natively — no hand-written wrapping layer on either side.
- **Strict schemas.** Every serde struct uses `#[serde(deny_unknown_fields)]`; every msgspec struct uses `forbid_unknown_fields=True`. Drift between Python and Rust fails loudly at deserialize time.
- **All fields required.** Optionality is a forward-compat escape hatch we don't need yet. When we do, we add it deliberately.
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

## Object

Slice B has exactly one object variant:

```jsonc
{ "kind": "Polyline",
  "points":       [[x, y, z], ...],   // Vec3[]
  "stroke_color": [r, g, b, a],       // RgbaSrgb
  "stroke_width": 0.04,               // f32, scene-space units
  "closed":       true                 // bool
}
```

- `closed: true` — the renderer connects the last point back to the first. Explicit rather than manimgl's duplicate-first-point convention.
- `stroke_width` is in **scene units**, not pixels. Camera is hardcoded at `[-8, 8] × [-4.5, 4.5]` in Slice B.

Future variants (Slice C+): `Circle`, `BezPath`, `Text`, `Image`. Each is a new `kind`.

---

## TimelineOp

Internally tagged union with discriminator `"op"`. Ordered by `t` ascending; renderer assumes sorted input.

```jsonc
{ "op": "Add",    "t": 0.0, "id": 1, "object": { /* Object */ } }
{ "op": "Remove", "t": 2.0, "id": 1 }
```

An object is **active at time `t`** iff an `Add` with that id occurs at time `≤ t` and no subsequent `Remove` with that id occurs in `(add_t, t]`. Re-adding the same id after a remove is permitted; each activation interval is independent.

Slice B only emits one `Add` per scene. `Remove`, `Set`, `Reparent`, `Label`, `CameraSet` are deferred.

---

## Track

Internally tagged union with discriminator `"kind"`. One track per animated property per object; multiple segments within a track describe the time-varying value.

### PositionTrack

```jsonc
{ "kind": "Position",
  "id": 1,
  "segments": [
    { "t0": 0.0, "t1": 2.0,
      "from": [0.0, 0.0, 0.0],
      "to":   [2.0, 0.0, 0.0],
      "easing": { "kind": "Linear" } }
  ]
}
```

- `id` references an object that must be active throughout `[t0, t1]`.
- Segments within a track must have `t0 < t1` and must not overlap; gaps are allowed (position stays at the last `to` value).
- The object's **effective position at time `t`** is `object.base_position + active_segment_value(t)`. For Slice B, `base_position` is implicitly zero (positions live entirely in the track).

### Easing

Internally tagged. Slice B: one variant.

```jsonc
{ "kind": "Linear" }
```

Evaluated as `from + (to - from) * (t - t0) / (t1 - t0)`.

Future variants (Slice C+): `Smooth`, `Rush`, `Slow`, parameterized variants e.g. `{ "kind": "Smooth", "inflection": 10.0 }`. Each is additive.

---

## Validation rules (evaluator contract)

The evaluator treats invalid IR as a hard error. Producers (the Python scene API) are responsible for emitting valid IR; the evaluator does not silently correct.

1. `schema_version` must equal 1.
2. `timeline` is sorted non-decreasing by `t`.
3. Every `Remove` op references an id currently active.
4. Every track's `id` refers to an object that `Add`'s at some point.
5. Every track's segments are sorted, non-overlapping, with `t0 < t1`.
6. `duration` ≥ the latest timestamp referenced by any op or segment.
7. `fps` ≥ 1. `resolution.{width,height}` ≥ 1.

---

## What's not here

Deliberately out of scope for v1, per `docs/slices/slice-b.md` §4:

- Non-polyline geometry (circles, bezier paths, text, surfaces).
- Color, opacity, rotation, scale tracks.
- Non-linear easings.
- `Set`, `Reparent`, `Label`, `CameraSet` timeline ops.
- Fill (only stroke).
- Scene graph / parenting.
- Multi-camera, 3D camera.
- Chunked rendering metadata, cache keys.

Each lands as an additive change (new `kind`, new `op`, or new top-level field) in a later slice. `schema_version` bumps when an existing field changes meaning.
