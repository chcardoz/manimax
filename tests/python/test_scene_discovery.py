"""Slice C Step 6 — scene discovery.

Covers the three ways ``load_scene`` can fail (bad module, missing class,
wrong class type) plus the happy path (file-path import + instantiation).
"""

from __future__ import annotations

import textwrap
from pathlib import Path

import pytest
from manim_rs.discovery import (
    ModuleLoadError,
    NotASceneError,
    SceneNotFoundError,
    load_scene,
)
from manim_rs.scene import Scene

SCENE_SOURCE = textwrap.dedent(
    """
    from manim_rs import Scene, Polyline, Translate


    class MyScene(Scene):
        def construct(self) -> None:
            square = Polyline(
                [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)],
                stroke_width=0.08,
            )
            self.add(square)
            self.play(Translate(square, (1.0, 0.0, 0.0), duration=0.3))


    class NotAScene:
        pass
    """
)


@pytest.fixture
def scene_file(tmp_path: Path) -> Path:
    p = tmp_path / "user_scene.py"
    p.write_text(SCENE_SOURCE)
    return p


def test_load_scene_returns_subclass(scene_file: Path) -> None:
    cls = load_scene(scene_file, "MyScene")
    assert issubclass(cls, Scene)
    assert cls.__name__ == "MyScene"


def test_load_scene_instantiates_and_runs_construct(scene_file: Path) -> None:
    cls = load_scene(scene_file, "MyScene")
    scene = cls(fps=15)
    scene.construct()
    # Scene is non-empty and has a non-zero duration after construct().
    assert scene.ir.metadata.duration == pytest.approx(0.3)
    assert len(scene.ir.timeline) >= 1


def test_load_scene_missing_module(tmp_path: Path) -> None:
    with pytest.raises(ModuleLoadError):
        load_scene(tmp_path / "does_not_exist.py", "MyScene")


def test_load_scene_missing_class(scene_file: Path) -> None:
    with pytest.raises(SceneNotFoundError) as err:
        load_scene(scene_file, "DoesNotExist")
    # Error should list the concrete scene it did find.
    assert "MyScene" in str(err.value)


def test_load_scene_class_not_scene_subclass(scene_file: Path) -> None:
    with pytest.raises(NotASceneError):
        load_scene(scene_file, "NotAScene")


def test_load_scene_rejects_base_scene(scene_file: Path) -> None:
    # The base ``Scene`` itself must not be selectable — it would produce an
    # empty frame and confuse users. A subclass with only ``pass`` is allowed
    # (and should NotImplementedError on construct, not on discovery).
    with pytest.raises(NotASceneError):
        load_scene(scene_file, "Scene")


def test_base_scene_construct_is_not_implemented() -> None:
    with pytest.raises(NotImplementedError):
        Scene().construct()
