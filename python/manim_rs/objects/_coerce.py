"""Float-coerce helpers shared across mobjects and animations.

Centralizes the ``(float(x), float(y), float(z))`` triplet and ``(r, g, b, a)``
quad coercion that otherwise fans out across geometry verb constructors,
animation segment endpoints, and Tex/Text color fields.
"""

from __future__ import annotations

from manim_rs import ir


def vec3(v: ir.Vec3) -> ir.Vec3:
    return (float(v[0]), float(v[1]), float(v[2]))


def rgba(c: ir.RgbaSrgb) -> ir.RgbaSrgb:
    return (float(c[0]), float(c[1]), float(c[2]), float(c[3]))
