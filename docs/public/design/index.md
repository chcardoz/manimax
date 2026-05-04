# Design notes

The decisions that shape Manimax, with the alternatives considered and the rationale for the path taken. These are public so users can understand *why* the API looks the way it does — and so contributors can avoid relitigating settled questions.

Each note answers: what was decided, why, what was rejected, and what consequences fall out. They're durable artifacts; once accepted, they stay even when superseded (with a "supersedes" / "superseded by" link).

## Cross-cutting policies

- **[ManimGL fidelity](manimgl-fidelity.md)** — when a choice has both a "technically correct" answer and a "matches manimgl" answer, default to the latter. Deviate only with a conscious reason.

## Wire format and IR shape

- **[Why an IR?](why-an-ir.md)** — the IR is plain owning data; the evaluator is a separate compiled artifact. Keeps the Python↔Rust contract simple; moves shared-geometry optimizations into the runtime.
- **[Wire format: JSON over FFI](wire-format.md)** — Python encodes via msgspec, Rust decodes via serde_json. Debuggable, dumpable, diffable. Locks out zero-copy; revisit when scenes grow.
- **[Internally-tagged unions](internally-tagged-unions.md)** — `"op"` for events, `"kind"` for things. Symmetric on both sides; unknown-field rejection catches schema drift.

## Rendering

- **[Pixel cache: removed](pixel-cache.md)** — the cache was more expensive than the work it was supposed to skip. `eval_at` is fast enough that re-rendering is cheaper than maintaining a GB-scale on-disk store.
- **[In-process libavcodec encoder](encoder-in-process.md)** — replaced the ffmpeg subprocess + stdin pipe with worker-thread libavcodec via `ffmpeg-the-third`. Saves the subprocess `wait()` tail; restores raster/encode parallelism with a bounded channel.
- **[Local chunked rendering](local-chunked-rendering.md)** — renders disjoint frame ranges as independent mp4 chunks, then concatenates them in deterministic frame order.
- **[Hardware encoder fallback chain](encoder-hardware.md)** — `--encoder hardware` walks `videotoolbox → nvenc` and uses whichever is linked. Single deploy artifact across macOS dev and Linux GPU containers.

## Text and math

- **[Text stack: cosmic-text + swash](text-stack.md)** — bundled Inter Regular, no system-font scan ever. Reuses Slice E's glyph machinery (high-ppem outlines, em-scaled fill tolerance).

## CI and infrastructure

- **[CI on lavapipe](ci-lavapipe.md)** — Linux + Mesa software Vulkan. Deterministic, GPU-free, what wgpu's own CI uses.

## Per-slice decision bundles

Some slices accumulated enough small decisions that a per-slice consolidated note made more sense than 10 atomic ones:

- **[Slice C decisions](slice-c-decisions.md)** — pythonize boundary, MSAA + tolerance snapshots, BezPath unification, tessellator API.
- **[Slice D decisions](slice-d-decisions.md)** — analytic SDF stroke AA, cubic subdivision depth, snapshot cache (later removed by [pixel-cache](pixel-cache.md)).
- **[Slice E decisions](slice-e-decisions.md)** — Tex fan-out at eval time, per-Evaluator cache, high-ppem outline extraction, lyon flatness pin, single-frame render API.

## When to write one

When you pick between credible alternatives — library X vs Y, schema shape, protocol, scope boundary — or make any choice a future contributor might reasonably try to undo. Two shapes:

- **Atomic** (~10 lines): one decision, one rationale.
- **Consolidated per-slice** (150–300 lines, sectioned A/B/C): a bundle of small calls made together where the cross-references matter.

Use the next unused number. Both shapes are valid.
