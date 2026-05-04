"""Programmatic render API.

Pure-Python entry points for rendering a `Scene` without going through the
CLI. The CLI is a thin adapter on top of these. Library callers — agentic
pipelines that already have a `Scene` subclass in hand — should use these
directly: `from manim_rs.api import render_scene; render_scene(MyScene, "out.mp4")`.

No typer, no argv parsing, no terminal UX lives here. Validation that depends
on user-input shape (e.g. `--resolution WxH` parsing) belongs to the CLI tier;
this module trusts its callers.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Literal

import msgspec.structs

from manim_rs import _rust, ir
from manim_rs.scene import Scene

EncoderName = Literal["software", "hardware"]
ProgressCallback = Callable[[int, int], None]


def render_scene(
    scene_cls: type[Scene],
    out: Path | str,
    *,
    fps: int = 30,
    resolution: tuple[int, int] | None = None,
    duration: float | None = None,
    crf: int | None = None,
    encoder: EncoderName = "software",
    workers: int = 1,
    progress: ProgressCallback | None = None,
    trace_json: Path | str | None = None,
) -> None:
    """Render `scene_cls` to an mp4 at `out`.

    Instantiates the scene, runs `construct()`, freezes the IR, and dispatches
    to the Rust runtime. `duration` overrides the scene's natural length;
    `resolution`, when given, overrides the scene's declared default.
    """
    if trace_json is not None:
        _rust.install_trace_json(str(trace_json))
    if workers <= 0:
        raise ValueError(f"workers must be positive, got {workers}")

    scene_ir = _build_ir(scene_cls, fps=fps, resolution=resolution)
    if duration is not None:
        scene_ir = msgspec.structs.replace(
            scene_ir,
            metadata=msgspec.structs.replace(scene_ir.metadata, duration=duration),
        )

    _rust.render_to_mp4(
        ir.to_builtins(scene_ir),
        str(out),
        crf=crf,
        encoder_backend=encoder,
        workers=workers,
        progress=progress,
    )


def render_frame(
    scene_cls: type[Scene],
    out: Path | str,
    t: float,
    *,
    fps: int = 30,
    resolution: tuple[int, int] | None = None,
    trace_json: Path | str | None = None,
) -> None:
    """Render a single frame of `scene_cls` at time `t` to a PNG at `out`."""
    if trace_json is not None:
        _rust.install_trace_json(str(trace_json))

    scene_ir = _build_ir(scene_cls, fps=fps, resolution=resolution)
    _rust.render_frame(ir.to_builtins(scene_ir), str(out), float(t))


def _build_ir(
    scene_cls: type[Scene],
    *,
    fps: int,
    resolution: tuple[int, int] | None,
) -> ir.Scene:
    scene_kwargs: dict = {"fps": fps}
    if resolution is not None:
        scene_kwargs["resolution"] = ir.Resolution(width=resolution[0], height=resolution[1])
    scene = scene_cls(**scene_kwargs)
    scene.construct()
    return scene.ir
