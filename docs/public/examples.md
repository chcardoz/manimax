# Examples

Each code block below runs at docs build time via [`markdown-exec`](https://pawamoy.github.io/markdown-exec/) — what you see is what the actual library produces.

## Building IR without rendering

`Scene` records imperative calls into an IR. Until you call the renderer, it's just data — useful for inspection, testing, and parallel chunked rendering.

```python exec="true" source="material-block" result="text"
from manim_rs import Scene, Polyline

scene = Scene(fps=30)
scene.add(
    Polyline(
        points=[(-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0)],
        closed=True,
    )
)
scene.wait(1.5)

ir = scene.ir
print(f"duration   = {ir.metadata.duration} s")
print(f"fps        = {ir.metadata.fps}")
print(f"resolution = {ir.metadata.resolution.width}x{ir.metadata.resolution.height}")
print(f"timeline   = {len(ir.timeline)} ops")
print(f"first op   = {type(ir.timeline[0]).__name__} at t={ir.timeline[0].t}")
```

## Animating with `play`

`play(...)` runs animations in parallel and advances the clock by the longest one.

```python exec="true" source="material-block" result="text"
from manim_rs import Scene, Polyline, Translate, Smooth

scene = Scene(fps=60)
square = Polyline(
    points=[(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)],
    closed=True,
)
scene.add(square)
scene.play(
    Translate(square, (2.0, 0.0, 0.0), 2.0, easing=Smooth()),
)

ir = scene.ir
print(f"duration  = {ir.metadata.duration} s")
print(f"tracks    = {len(ir.tracks)}")
print(f"track[0]  = {type(ir.tracks[0]).__name__} on object id={ir.tracks[0].id}")
print(f"segments  = {len(ir.tracks[0].segments)}")
```

## Rendering to mp4

Once an IR is built, the Rust runtime evaluates and rasterizes it. The CLI is the most common entry point:

=== "CLI"
    ```sh
    python -m manim_rs render examples/your_scene.py YourScene out.mp4 --duration 2 --fps 30
    ```

=== "Python"
    ```python
    from manim_rs import _rust
    _rust.render_to_mp4(scene.ir, "out.mp4")
    ```

This documentation site does not embed rendered videos at build time — yet. The pattern is straightforward (`markdown-exec` runs the example, the next line embeds the resulting mp4) and will land alongside the 0.1.0 docs gallery.
