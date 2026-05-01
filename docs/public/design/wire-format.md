# 0001 — IR wire format is a JSON string over the FFI for Slice B

**Date:** 2026-04-21
**Status:** superseded by `0004-slice-c-decisions.md` §A

## Decision

The Python↔Rust IR contract crosses the pyo3 boundary as a UTF-8 JSON string. Python serializes via `msgspec.json.Encoder`; Rust deserializes via `serde_json::from_str`.

## Why

- **Debuggable.** A JSON string can be dumped to disk, diffed, pasted into an issue, replayed into `_rust.roundtrip_ir`. Every other option (pickle, pythonize, arrow IPC) is opaque-at-rest.
- **Matches existing roundtrip infrastructure.** `_rust.roundtrip_ir(&str) -> String` already exists as a probe; reusing it for `render_to_mp4` kept the FFI surface tiny.
- **No performance pressure at Slice B.** Scenes are small; the parse cost is well under one frame time.
- **Lets us ship Slice B without introducing another dep** (`pythonize`) or another serialization format.

## Consequences

- **Buys:** simple FFI, zero-drift schema tests (Rust `deny_unknown_fields` + msgspec `forbid_unknown_fields` catch any mismatch on the first call), human-readable IR in failures.
- **Locks out:** zero-copy sharing, numeric precision beyond JSON f64, bytes-typed geometry (e.g. raw `[f32]` buffers).
- **Implied follow-up:** when scenes grow (e.g. 50 k Bézier control points) the parse cost becomes visible. Slice C is expected to replace this with `pythonize` or `FromPyObject`.

## Rejected alternatives

- **`pythonize` / per-type `FromPyObject`** — cleaner and faster, but adds a dep and couples the schema to pyo3 types before the IR schema stabilizes. Revisit after Slice C geometry expansion.
- **`pickle`** — opaque at rest, Python-only, defeats a third-party IR.
- **MessagePack / CBOR** — binary is faster but not debuggable without tooling; Slice B doesn't need the speed.
- **Arrow IPC** — what `rerun` does; overkill for Slice B's scene shapes, great fit once tracks are columnar.
