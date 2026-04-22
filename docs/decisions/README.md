# Decision records

Numbered ADR-lite files: `NNNN-slug.md`. Use the next unused number.

## Template (~10 lines)

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

Write one when you pick between credible alternatives (library X vs Y, schema shape, protocol, scope boundary) or any choice a future agent might reasonably try to undo.

Read the existing ADRs before changing anything they touch. If you're reversing a decision, write a new ADR and mark the old one `superseded by NNNN`.
