# Decision records

Numbered ADR-lite files: `NNNN-slug.md`. Use the next unused number.

Two shapes are in use, and both are fine:

## Atomic ADR (~10 lines) — for one decision

```markdown
# NNNN — <title>

**Date:** YYYY-MM-DD
**Status:** accepted | superseded by NNNN | deprecated

## Decision
One sentence.

## Why
2–3 bullets.

## Consequences
What this buys us / locks us out of.

## Rejected alternatives
Named options we considered and why they lost.
```

Examples: `0001`, `0002`, `0003`, `0005`, `0007`, `0009`, `0010`, `0011`.

## Consolidated per-slice ADR — for a clump of related decisions made inside one slice

When a slice makes 4–8 design calls that are individually small but architecturally coherent (Slice C's stroke pipeline, Slice D's snapshot cache, Slice E's text+math), one ADR with `## A.`, `## B.`, ... sections is cleaner than 4–8 atomic ones cross-referencing each other. Length: 150–300 lines.

Examples: `0004` (Slice C), `0006` (Slice D), `0008` (Slice E), `0012` (text via cosmic-text + swash).

The shape is the same per section: **Decision / Why / Consequences / Rejected alternatives**.

## When to write one

You're picking between credible alternatives (library X vs Y, schema shape, protocol, scope boundary), or making any choice a future agent might reasonably try to undo. Write atomic when the decision stands alone; consolidate when several decisions in one slice need to be read together.

Read existing ADRs before changing anything they touch. Reversing a decision means a new ADR + marking the old one `superseded by NNNN`.
