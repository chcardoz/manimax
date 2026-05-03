"""Drawable objects — the ``Object`` union.

This is the geometry extension axis. Adding a new shape (Surface, Sphere,
Image, SVG, …) lives here: declare the variant, add it to the union, re-export
from ``__init__``. Path-verb vocabulary is in ``_primitives``; paint descriptors
are in ``paint``.
"""

from __future__ import annotations

from typing import Literal

import msgspec

from manim_rs.ir._primitives import PathVerb, RgbaSrgb, Vec3
from manim_rs.ir.paint import Fill, Stroke


class Polyline(
    msgspec.Struct,
    tag_field="kind",
    tag="Polyline",
    forbid_unknown_fields=True,
    frozen=True,
):
    points: tuple[Vec3, ...]
    closed: bool
    stroke: Stroke | None
    fill: Fill | None


class BezPath(
    msgspec.Struct,
    tag_field="kind",
    tag="BezPath",
    forbid_unknown_fields=True,
    frozen=True,
):
    verbs: tuple[PathVerb, ...]
    stroke: Stroke | None
    fill: Fill | None


class Tex(
    msgspec.Struct,
    tag_field="kind",
    tag="Tex",
    forbid_unknown_fields=True,
    frozen=True,
):
    """LaTeX-flavored math source. Compiled to filled BezPaths in Rust eval.

    `macros` is a ``dict[str, str]`` of no-arg shortcut macros — Python-side
    pre-expansion (Step 5) bakes them into ``src`` before the IR ships, but
    the dict still rides through for cache-key stability and roundtrip.
    Use a key-sorted dict at construction time so the wire format matches
    Rust's BTreeMap canonical ordering. See ``docs/slices/slice-e.md``
    §6 gotcha #4.
    """

    src: str
    macros: dict[str, str]
    # Default color applied only to items RaTeX leaves in plain black;
    # explicit ``\textcolor{...}`` colors ride through unchanged.
    color: RgbaSrgb
    # Multiplier on top of the Rust adapter's ``WORLD_UNITS_PER_EM``.
    scale: float


# Shaped-text knobs. Lowercase string literals match Rust's
# `#[serde(rename_all = "lowercase")]` on `TextWeight` / `TextAlign`,
# and follow the same `Literal[...]` precedent as `JointKind`.
TextWeight = Literal["regular", "bold"]
TextAlign = Literal["left", "center", "right"]


class Text(
    msgspec.Struct,
    tag_field="kind",
    tag="Text",
    forbid_unknown_fields=True,
    frozen=True,
):
    """Plain-text source. Shaped to per-glyph BezPaths in Rust eval.

    Mirrors `Tex` in shape: source is content-only (no per-instance
    transform here — `scale`/position/rotation come from animation tracks
    on the parent `ObjectState`). The cache key is the content tuple
    `(src, font, weight, size, color, align)`.

    `font = None` resolves to bundled Inter Regular. Non-`None` values
    are reserved for user-registered families (S7c/S7f) and not wired
    up yet on the Rust side.
    """

    src: str
    font: str | None
    weight: TextWeight
    # Em size in world units. `1.0` ⇒ one em equals one world unit.
    size: float
    color: RgbaSrgb
    align: TextAlign


Object = Polyline | BezPath | Tex | Text
