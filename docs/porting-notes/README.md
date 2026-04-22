# Porting notes

Distilled notes on manimgl subsystems as they're ported to Manimax. Each file captures the invariants, public API shape, and edge cases that aren't obvious from reading the manimgl source — the stuff you need on hand to port correctly, and that future contributors (including Claude) need to avoid re-deriving.

The manimgl source itself lives at `reference/manimgl/` (git submodule). These notes are your interpretation of it.

## File format

One file per subsystem. Keep each to roughly 200–500 words. Suggested structure:

```markdown
# <subsystem>

**manimgl source:** `reference/manimgl/manimlib/<path>/` at commit `<sha>`

## Public API

What the Python surface looks like. Signatures, key methods, usage pattern in `example_scenes.py`.

## Invariants

Non-obvious rules the implementation relies on. "Points array length must be a multiple of 3." "Animations must be idempotent under replay." Etc.

## Edge cases

What's easy to get wrong. Cases the original author hit (find them in comments, git blame, GitHub issues).

## Manimax mapping

How this subsystem maps to our IR / Rust runtime. What lifts straight across, what changes shape.
```

## Naming

One file per subsystem, matching the manimgl subdirectory where possible:

- `scene.md`
- `mobject.md` (or split: `vmobject.md`, `tex.md`, `text.md`)
- `animation.md`
- `shaders.md`
- `camera.md`

Add more granular files as needed. Over time these become the primary reference; `reference/manimgl/` fades to a fallback for questions the notes don't cover.
