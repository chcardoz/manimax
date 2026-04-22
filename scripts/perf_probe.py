"""Throwaway performance probe — resolution × fps sweep.

Not a test, not documentation. Prints a table to stdout and exits.

    python scripts/perf_probe.py

All measurements against ``tests/python/integration_scene.py`` at a fixed
2-second duration. We sweep the two knobs that actually matter for real
rendering: **resolution** (quality) and **fps** (smoothness). Everything
else is held constant.
"""

from __future__ import annotations

import statistics
import sys
import tempfile
import time
from pathlib import Path

import msgspec.structs

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tests" / "python"))

from integration_scene import IntegrationScene  # noqa: E402
from manim_rs import _rust, ir  # noqa: E402

# 16:9 resolution ladder — scene camera is 16:9, so non-16:9 would distort.
RESOLUTIONS: list[tuple[int, int, str]] = [
    (320, 180, "180p"),
    (480, 270, "270p"),
    (854, 480, "480p"),
    (1280, 720, "720p"),
    (1920, 1080, "1080p"),
]
FPS_VALUES: list[int] = [24, 30, 60, 120]
DURATION: float = 2.0
RUNS_PER_CELL: int = 3


def _build_scene_ir(width: int, height: int, fps: int) -> dict:
    scene = IntegrationScene(fps=fps, resolution=ir.Resolution(width=width, height=height))
    scene.construct()
    scene_ir = msgspec.structs.replace(
        scene.ir,
        metadata=msgspec.structs.replace(scene.ir.metadata, duration=DURATION),
    )
    return ir.to_builtins(scene_ir)


def _time(fn, *args, **kwargs) -> float:
    t0 = time.perf_counter()
    fn(*args, **kwargs)
    return time.perf_counter() - t0


def _median(n: int, fn, *args, **kwargs) -> tuple[float, float]:
    samples = [_time(fn, *args, **kwargs) for _ in range(n)]
    return (
        statistics.median(samples),
        statistics.stdev(samples) if n > 1 else 0.0,
    )


def _print_row(cells: list[str], widths: list[int]) -> None:
    print("  ".join(c.rjust(w) for c, w in zip(cells, widths, strict=False)))


def measure_2d_sweep(tmp: Path) -> None:
    print()
    print(f"### A. Resolution × FPS, {DURATION}s scene, median of {RUNS_PER_CELL} runs")
    print()
    print("Total wall-clock seconds per render:")
    print()
    label_w = 10
    col_w = 10
    header = [""] + [f"{fps}fps" for fps in FPS_VALUES]
    _print_row(header, [label_w] + [col_w] * len(FPS_VALUES))
    _print_row([""] + ["------" for _ in FPS_VALUES], [label_w] + [col_w] * len(FPS_VALUES))

    per_frame: dict[tuple[str, int], float] = {}
    for width, height, label in RESOLUTIONS:
        row = [f"{label}"]
        for fps in FPS_VALUES:
            scene_ir = _build_scene_ir(width, height, fps)
            out = tmp / f"r_{label}_f{fps}.mp4"
            med, _sd = _median(RUNS_PER_CELL, _rust.render_to_mp4, scene_ir, str(out), fps=fps)
            frames = int(fps * DURATION)
            per_frame[(label, fps)] = (med / frames) * 1000.0
            row.append(f"{med:.2f}s")
        _print_row(row, [label_w] + [col_w] * len(FPS_VALUES))

    print()
    print("Cost per frame (ms) — same data, divided by frame count:")
    print()
    _print_row(header, [label_w] + [col_w] * len(FPS_VALUES))
    _print_row([""] + ["------" for _ in FPS_VALUES], [label_w] + [col_w] * len(FPS_VALUES))
    for _width, _height, label in RESOLUTIONS:
        row = [f"{label}"]
        for fps in FPS_VALUES:
            row.append(f"{per_frame[(label, fps)]:.1f}ms")
        _print_row(row, [label_w] + [col_w] * len(FPS_VALUES))


def measure_eval_vs_render(tmp: Path) -> None:
    """Random-access eval vs. full render — the architectural pitch."""
    print()
    print("### B. Random-access eval vs. full render (at 480x270, 30fps)")
    print()
    scene_ir = _build_scene_ir(480, 270, 30)
    widths = [32, 12]
    _print_row(["operation", "median"], widths)
    _print_row(["---------", "------"], widths)

    med, _ = _median(5, _rust.eval_at, scene_ir, 1.0)
    _print_row(["eval_at (t=1.0), 1 call", f"{med * 1000:.2f}ms"], widths)

    def eval_three() -> None:
        _rust.eval_at(scene_ir, 0.25)
        _rust.eval_at(scene_ir, 1.0)
        _rust.eval_at(scene_ir, 1.75)

    med, _ = _median(5, eval_three)
    _print_row(["eval_at × 3 calls", f"{med * 1000:.2f}ms"], widths)

    out = tmp / "full.mp4"
    med, _ = _median(3, _rust.render_to_mp4, scene_ir, str(out), fps=30)
    _print_row(["render_to_mp4 (60 frames)", f"{med * 1000:.1f}ms"], widths)


def main() -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        print("Manimax perf probe — integration scene, MSAA 4x, h264/yuv420p.")
        print(f"Fixed: {DURATION}s scene duration, scene camera 16:9.")
        measure_2d_sweep(tmp)
        measure_eval_vs_render(tmp)
        print()


if __name__ == "__main__":
    main()
