# 0007 — CI on Linux with lavapipe software Vulkan

**Date:** 2026-04-24
**Status:** accepted

## Decision
Run CI on `ubuntu-latest` with `WGPU_BACKEND=vulkan` + `mesa-vulkan-drivers`
(lavapipe). Skip the `reference/manimgl` submodule (`submodules: false`) and
exclude `manim-rs-py` from `cargo test --workspace` because its link step
needs maturin's flags.

## Why
- GitHub-hosted runners have no GPU; lavapipe is deterministic CPU Vulkan
  and the path wgpu's own CI uses for headless testing.
- `reference/manimgl` is large and no test imports it — fetching it just
  slows every job.
- `manim-rs-py` is a pyo3 cdylib; `cargo test` alone fails at link time.
  Documented in CLAUDE.md; CI must match that invocation.

## Consequences
- MSAA sample positions may drift between lavapipe and macOS Metal;
  snapshot tests must stay tolerance-based per ADR 0004 §E.
- Adds a ~2 min apt install step per job; the Swatinem Rust cache
  amortises the rest.
- Developers running tests locally on macOS can still hit GPU-only bugs
  that CI won't catch — acceptable trade-off for deterministic CI.

## Rejected alternatives
- `WGPU_BACKEND=gl` + Mesa software GL — second-class in wgpu, fewer
  debugging eyes, and MSAA behaviour is murkier than Vulkan.
- Skipping wgpu-touching tests on CI — defeats the point of CI for a
  rendering library.
- macOS runners — more expensive, slower, and not reproducible for
  contributors on other platforms.
- Fetching the manimgl submodule — pure cost, zero benefit at this
  slice's test scope.
