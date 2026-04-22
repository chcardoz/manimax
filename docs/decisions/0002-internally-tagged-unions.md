# 0002 — IR unions are internally tagged with `"op"` / `"kind"`

**Date:** 2026-04-21
**Status:** accepted

## Decision

Every polymorphic type in the IR serializes as an object with an internal discriminator field: `"op"` for timeline operations, `"kind"` for geometry objects, easings, and tracks.

Rust: `#[serde(tag = "op")]` / `#[serde(tag = "kind")]` on the enum.
Python: `msgspec.Struct(tag_field="op", tag="Add", forbid_unknown_fields=True)` per variant.

Both sides also set `deny_unknown_fields` / `forbid_unknown_fields=True`.

## Why

- **Symmetric, literal wire format** — the discriminator is right there in the JSON, no wrapper object, no positional decoding. A human reading a dumped IR can tell what each entry is.
- **serde ↔ msgspec agree out of the box** when you pin the tag field name on both sides — verified by `tests/python/test_ir_roundtrip.py`.
- **Unknown-field rejection on both sides** catches schema drift on the next CI run, not at a mysterious runtime crash three weeks later. Cheap insurance for a hand-mirrored schema.
- **Kind/op split is intentional** — `op` for things that *happen* (timeline events), `kind` for things that *are* (geometry, easings, tracks). Reads naturally.

## Consequences

- **Buys:** zero-ambiguity wire format, self-describing IR, symmetric encode/decode tests are trivial.
- **Locks in:** adding a variant requires editing both the Rust enum *and* the msgspec struct list; codegen is deferred to a later slice.
- **Compatible with the "hand-mirror Python and Rust structs" approach** from `docs/architecture.md` §6.

## Rejected alternatives

- **Externally tagged (`{"Add": {...}}`)** — adds a wrapper object; less readable in dumps.
- **Adjacently tagged (`{"type": "Add", "payload": {...}}`)** — even more nesting for no gain.
- **Untagged + try-each-variant** — serde supports it, but decoding ambiguity is a foot-gun once variant shapes overlap (e.g. two tracks with the same field set).
- **Codegen from a shared schema file** — the right long-term answer, but building the codegen is more work than hand-mirroring two small files. Revisit when the IR has ~10+ variants per union.

## Addendum (2026-04-21): variants must be struct-shaped, not unit

Serde's `deny_unknown_fields` is silently ignored on **unit variants** under an internal tag. This defeats the unknown-field guarantee this ADR sells.

```rust
// ❌ Extra fields on {"kind": "Linear", "bogus": 1} pass through silently.
enum Easing { Linear }

// ✅ Empty struct variant — same wire format, deny_unknown_fields enforced.
enum Easing { Linear {} }
```

**Rule:** every variant of every internally-tagged enum in the IR must be a struct variant (named fields, possibly empty `{}`) — never a unit variant. Same rule on the Python side: every msgspec Struct that participates in a tagged union must be a class body (even if empty), not just a tag constant. Caught in April 2026 by a parametrized Python-side unknown-field test hitting the seven IR sites — the `Easing` site didn't fail until `Linear` was changed to `Linear {}`.
