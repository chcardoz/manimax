"""Scene discovery — load a user module and find a named ``Scene`` subclass.

Mirrors manimgl's ``manimlib/__main__.py`` + ``extract_scene.py`` contract:
user writes ``class MyScene(Scene): def construct(self): ...`` in a .py file
(or importable module), and the CLI picks a scene by class name.

Two input forms are accepted:

- **Path to a .py file.** Loaded via ``importlib.util.spec_from_file_location``
  under a synthetic module name so imports inside the file don't collide with
  anything already in ``sys.modules``.
- **Dotted module name.** Loaded via ``importlib.import_module``. Requires the
  module to be on ``sys.path``.
"""

from __future__ import annotations

import importlib
import importlib.util
import sys
from pathlib import Path
from types import ModuleType

from manim_rs.scene import Scene


class DiscoveryError(Exception):
    """Base class for scene-discovery failures."""


class ModuleLoadError(DiscoveryError):
    """The target module could not be imported."""


class SceneNotFoundError(DiscoveryError):
    """No class with the requested name was found in the module."""


class NotASceneError(DiscoveryError):
    """The named class exists but does not subclass ``Scene``."""


def load_module(target: str | Path) -> ModuleType:
    """Resolve ``target`` as a file path or dotted module name and import it."""
    path = Path(target)
    if path.is_file():
        return _load_from_path(path)
    if "/" in str(target) or str(target).endswith(".py"):
        # Looks like a path but doesn't exist on disk.
        raise ModuleLoadError(f"no such file: {target}")
    try:
        return importlib.import_module(str(target))
    except ImportError as err:
        raise ModuleLoadError(f"cannot import module {target!r}: {err}") from err


def _load_from_path(path: Path) -> ModuleType:
    resolved = path.resolve()
    module_name = f"_manim_rs_scene_{resolved.stem}_{abs(hash(resolved))}"
    spec = importlib.util.spec_from_file_location(module_name, resolved)
    if spec is None or spec.loader is None:
        raise ModuleLoadError(f"cannot build import spec for {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    try:
        spec.loader.exec_module(module)
    except Exception as err:
        sys.modules.pop(module_name, None)
        raise ModuleLoadError(f"failed to execute {path}: {err}") from err
    return module


def find_scene_class(module: ModuleType, scene_name: str) -> type[Scene]:
    """Look up ``scene_name`` in ``module`` and verify it's a ``Scene`` subclass."""
    obj = getattr(module, scene_name, None)
    if obj is None:
        available = _scene_names(module)
        hint = f" available: {', '.join(available)}" if available else ""
        raise SceneNotFoundError(f"no scene named {scene_name!r} in {module.__name__}.{hint}")
    if not (isinstance(obj, type) and issubclass(obj, Scene)):
        raise NotASceneError(f"{scene_name!r} is not a Scene subclass (got {type(obj).__name__})")
    if obj is Scene:
        raise NotASceneError(f"{scene_name!r} is the base Scene class, not a user subclass")
    return obj


def _scene_names(module: ModuleType) -> list[str]:
    """List concrete ``Scene`` subclasses declared in ``module`` (not the base)."""
    out: list[str] = []
    for name in dir(module):
        obj = getattr(module, name)
        if isinstance(obj, type) and issubclass(obj, Scene) and obj is not Scene:
            out.append(name)
    return out


def load_scene(target: str | Path, scene_name: str) -> type[Scene]:
    """Top-level convenience: import and return the named ``Scene`` subclass."""
    module = load_module(target)
    return find_scene_class(module, scene_name)
