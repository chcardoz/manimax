"""Discrete timeline events — the ``TimelineOp`` union.

Extension axis: discrete events that happen at a specific ``t``. Today: ``Add``
and ``Remove``. Architecture doc §4 names ``set``, ``reparent``, ``label``,
and ``camera_set`` as planned. New ops land here.
"""

from __future__ import annotations

import msgspec

from manim_rs.ir._primitives import ObjectId, Time
from manim_rs.ir.objects import Object


class AddOp(
    msgspec.Struct,
    tag_field="op",
    tag="Add",
    forbid_unknown_fields=True,
    frozen=True,
):
    t: Time
    id: ObjectId
    object: Object


class RemoveOp(
    msgspec.Struct,
    tag_field="op",
    tag="Remove",
    forbid_unknown_fields=True,
    frozen=True,
):
    t: Time
    id: ObjectId


TimelineOp = AddOp | RemoveOp
