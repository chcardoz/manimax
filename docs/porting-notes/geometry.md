# Porting note: geometry primitives

**Status:** Slice B (`Polyline`) + Slice C (`BezPath`) port.
**Stub label:** none — shipping.
**Manimgl reference:** `reference/manimgl/manimlib/mobject/geometry.py`,
`manimlib/mobject/types/vectorized_mobject.py` (the `VMobject` anchor/handle model).

## What manimgl does

Manimgl models shapes as `VMobject`s — a flat array of points interpreted as
cubic Bézier anchors and handles (`A H1 H2 A H1 H2 A …`). Every primitive
(`Polygon`, `Circle`, `Square`, `Arc`) is a thin configurator over this array.
Closedness is implicit: closing a path means duplicating the first anchor at
the end. Sub-paths are separated by anchor-handle patterns that cross.

`VMobject` is also the interpolation target — animations mutate its points
array in place during `interpolate()`.

## What Slice B/C ships

Two inert Python classes in `python/manim_rs/objects/geometry.py`, plus a
handful of verb-builder helpers:

- `Polyline(points, *, stroke_color, stroke_width, fill_color, closed=True)`
- `BezPath(verbs, *, stroke_color, stroke_width, fill_color)`
- Verb helpers: `move_to`, `line_to`, `quad_to`, `cubic_to`, `close`

Both are descriptions — they own no render state. When handed to
`Scene.add(...)`, the scene assigns `obj._id` and later calls `to_ir()` to
produce the IR struct for Python→Rust emission.

## Invariants and divergences from manimgl

- **`closed` is explicit, default `True`.** Manimgl closes by duplicating the
  first point at the end of the points array. Here the renderer connects the
  last point back to the first when `closed=True`. **Do not** duplicate the
  first point yourself — it will render a zero-length segment. The default is
  `True` because the canonical Slice B scene (a unit square) needs closure;
  pass `closed=False` for open polylines.
- **Minimum `Polyline` length is 2.** `_normalize_points` raises below that.
  Manimgl would render a single anchor as a degenerate curve.
- **`BezPath` multi-subpath rule.** `MoveTo` *implicitly ends the previous
  subpath* (open, not closed) before starting a new one. The implementation
  in `crates/manim-rs-raster/src/tessellator.rs::verbs_to_path` tracks a
  `started` flag and calls `builder.end(false)` on each subsequent `MoveTo`.
  A trailing verb sequence without an explicit `Close` is also ended as an
  open sub-path. If you want sub-paths closed, author an explicit `close()`
  verb before the next `MoveTo`.
- **`BezPath` verbs are required to exist.** Empty `verbs` raises at
  construction. Empty or unreachable paths silently render as an empty
  mesh on the Rust side (see `tessellate_bezpath` / `tessellate_bezpath_fill`).
- **z is dropped by the rasterizer.** Points are `Vec3` at the IR/Python
  layer (to match manimgl's idiom and leave room for a future 3D path) but
  `tessellator.rs` flattens to 2D via `point(p[0], p[1])`. Authoring a point
  with non-zero z today is legal but has no visible effect.
- **`Polygon`, `Circle`, `Square`, `Arc` do not exist yet.** They would be
  one-liners on top of `Polyline` (square = 4 corners + `closed=True`) or
  `BezPath` (circle = 4 cubic arcs). Deliberately not added until a real
  authored scene needs them — keeping the surface small.
- **Fill non-zero winding is inherited from the tessellator.** If you draw a
  self-intersecting polyline with fill, the inner region fills under
  non-zero rule (documented in `porting-notes/fill.md`). Manimgl matches.
- **`stroke_color=None` disables stroke.** `fill_color=None` (default) leaves
  fill absent. Both map to the `Option<Stroke>` / `Option<Fill>` shape on the
  Rust side.
- **`stroke_width` is in scene units, not pixels.** Camera is pinned at
  `[-8, 8] × [-4.5, 4.5]`, so a `0.04` stroke is ~0.25% of scene width.
  This matches manimgl's unit system. See `docs/ir-schema.md` for the
  coordinate contract.

## Public API surface

```python
from manim_rs.objects.geometry import Polyline, BezPath, move_to, line_to, close

square = Polyline(
    [(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)],
    stroke_color=(1, 1, 1, 1),
    stroke_width=0.04,
    fill_color=(0.1, 0.2, 0.8, 1.0),
    closed=True,
)
path = BezPath(
    [move_to((0, 0, 0)), line_to((1, 0, 0)), close()],
    stroke_color=(1, 1, 1, 1),
)
```

## What Slice D / later will do

- Named primitives (`Square`, `Circle`, `Polygon`, `Arc`) as thin wrappers.
- Text (`Text`, `Tex`) via an SVG/font path — emits as `BezPath` verbs.
- Image mobjects — a new `Object::kind` variant.
- True 3D path support — will involve reviving the currently-dropped z.

## Files touched

- `python/manim_rs/objects/geometry.py` — `Polyline`, `BezPath`, verb helpers.
- `python/manim_rs/ir.py` — mirror structs (`Polyline`, `BezPath`, verb
  variants).
- `crates/manim-rs-ir/src/lib.rs` — the Rust `Object` + `PathVerb` enums.
- `crates/manim-rs-raster/src/tessellator.rs` — `polyline_to_path`,
  `verbs_to_path`, stroke/fill tessellators.
