# Porting note: scene discovery

**Manimgl reference:** `reference/manimgl/manimlib/extract_scene.py` at commit `c5e23d9`.
**Manimax port:** `python/manim_rs/discovery.py`; wired from `python/manim_rs/cli.py`'s `render` command.

## Public API

```
python -m manim_rs render MODULE SCENE OUT [--quality | -r WxH] \
  [--duration SEC] [--fps N] [-o/--open]
```

- `MODULE` — path to a `.py` file **or** a dotted module name on `sys.path`.
- `SCENE` — the class name (a `Scene` subclass) inside that module. Positional, required.
- `OUT` — mp4 output path.

Importable entry points from `python/manim_rs/discovery.py`:

- `load_module(target) -> ModuleType`
- `find_scene_class(module, scene_name) -> type[Scene]`
- `build_scene(module, scene_name) -> Scene` — calls the zero-arg constructor.
- Exception hierarchy rooted at `DiscoveryError`: `ModuleLoadError`,
  `SceneNotFoundError`, `NotASceneError`.

## Invariants

- **File paths** resolve via `importlib.util.spec_from_file_location` under a
  synthetic module name (`_manim_rs_scene_<stem>_<hash>`) so two files with the
  same basename don't collide in `sys.modules`.
- **Dotted names** resolve via `importlib.import_module`; caller must have
  arranged `sys.path`.
- **Scene classes must subclass `manim_rs.scene.Scene`.** `NotASceneError` is
  raised when the attribute exists but fails the `issubclass` check.
- Missing files and path-like-but-not-file strings (`foo/bar.py`, `x.py`)
  raise `ModuleLoadError` before any import is attempted.

## Edge cases

- **Module exec-time errors** bubble out as `ModuleLoadError` with the original
  exception chained. The synthetic entry in `sys.modules` is rolled back so a
  retry gets a clean slate.
- **Duplicate resolve of the same path** gets a different synthetic module
  name on each call (hash includes the resolved path; same path → same name).
  That is deliberate — tests re-load the same file under different
  configurations without cache bleed-through.
- **Scene class defined in an imported sibling module** (e.g. the user's file
  does `from .scenes import MyScene`) still works because `find_scene_class`
  does attribute lookup on the loaded module. It does **not** walk imports.

## Manimax mapping

### Kept from manimgl

- **Class-name positional argument.** Same shape as manimgl's
  `scene_names` list (reduced to a single name for now).
- **`issubclass(obj, Scene)` gate.** Same safeguard as manimgl's
  `is_child_scene`.
- **Synthetic module names** to avoid `sys.modules` collisions — manimgl
  does the equivalent with `__file__`-derived keys.

### Dropped from manimgl

- **`--write_all`** flag. Manimgl renders every scene in the file when it's
  set; Slice C renders one scene per CLI invocation. Add back when a consumer
  needs batch export; today there's no use case.
- **Interactive prompt** for ambiguous class names. Manimgl's
  `prompt_user_for_choice` prints a numbered menu and reads `stdin`. Our
  `find_scene_class` raises `SceneNotFoundError` with an `available:` hint
  instead. Rationale: interactive prompts are hostile in agentic pipelines
  (Divita-style consumers), and `pytest`'s stdin plumbing makes them
  annoying to test. If we add it back, use `monkeypatch.setattr('builtins.input', ...)` per `docs/gotchas.md`.
- **`compute_total_frames` pre-run.** Manimgl runs the scene twice — once
  with `skip_animations=True` to count frames for a progress bar, once for
  real. The IR makes this unnecessary: `scene.ir` reports the timeline length
  directly, no dry-run needed.
- **`insert_embed_line_to_module`.** Manimgl rewrites the user file to inject
  `self.embed()` for its interactive shell mode. Manimax is offline-only.
- **`__module__.startswith(module.__name__)` filter.** Manimgl excludes
  `Scene` subclasses re-exported from other modules. Our `issubclass` +
  name-match is enough since we resolve exactly one name per call.

### Intentional divergences

- **`MODULE` positional, not `--scene`.** The slice plan §1 wrote
  `--scene my_scene.py MyScene`; we shipped `MODULE SCENE` positional,
  matching the 99% call shape (`render my_scene.py MyScene out.mp4`) and
  dropping the flag. Plan delta, not a bug — logged in the Slice C retro.
- **Single scene per invocation.** Together with dropping `--write_all`,
  this locks the CLI to one mp4 per call. Agentic callers (the primary
  consumer shape) prefer this: one process, one artifact, one exit code.

## Files touched

- `python/manim_rs/discovery.py` — the module itself.
- `python/manim_rs/cli.py` — the `render` subcommand invokes
  `load_module` → `find_scene_class` → `build_scene`.
- `tests/python/test_scene_discovery.py` — covers file + dotted-name
  loading, missing-scene error path, `NotASceneError`, and exec-time
  failure rollback.
