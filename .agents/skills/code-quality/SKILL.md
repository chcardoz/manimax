---
name: code-quality
description: Review or write maintainable Rust, Python, and application code with a focus on readability, simplicity, boundaries, tests, and long-term code health. Use when asked to improve code quality, evaluate maintainability, refactor, review changed code, or make implementation choices where clarity matters.
---

# Code Quality

Use this skill to make code easier to understand, change, test, and trust. The goal is not aesthetic polish; it is lower cognitive load and better long-term code health.

Great code lets the next competent engineer make a correct change with confidence, without reverse-engineering the original author's intent.

## Quality Bar

Code is high quality when it has these properties:

1. **Clear purpose.** A reader can quickly tell what the module, type, function, or test is for.
2. **Simple design.** It solves the current problem directly, without speculative abstraction or generic machinery.
3. **Local reasoning.** A small change should not require tracing hidden state, global side effects, or distant call chains.
4. **Good names.** Names explain domain intent, not implementation trivia. They are precise but not inflated.
5. **Explicit boundaries.** Public APIs, internal helpers, ownership, error cases, and side effects are visible.
6. **Useful tests.** Tests cover behavior, edge cases, invariants, and regressions. They fail for the right reason when code breaks.
7. **Comments explain why.** Comments capture constraints, invariants, compatibility, protocol details, or non-obvious algorithms. They do not narrate obvious syntax.
8. **Consistent style.** Formatting and naming follow project conventions so style disappears and logic stands out.
9. **Dependable failures.** Errors are specific, actionable, and preserve useful context.
10. **Maintainable change shape.** Refactors, behavior changes, generated changes, and formatting are separated when practical.

## Review Workflow

When reviewing code quality, prefer this order:

1. **Understand intent.** Identify the user-facing behavior, developer-facing API, or invariant the change is meant to support.
2. **Check placement.** Ask whether this logic belongs here, or whether it violates a module or abstraction boundary.
3. **Reduce complexity.** Look for unnecessary indirection, over-generalization, nested conditionals, duplicate state, and parameter sprawl.
4. **Inspect names and data flow.** Verify that names make the flow readable without comments and that state changes are explicit.
5. **Check failure paths.** Review invalid inputs, IO failures, concurrency, partial updates, cleanup, and error messages.
6. **Check tests.** Tests should encode behavior and edge cases, not implementation details that make refactors painful.
7. **Check docs and comments.** Public APIs need usage and behavior documentation. Inline comments should justify non-obvious decisions.
8. **Prefer the smallest correct fix.** Do not introduce helpers, abstractions, compatibility layers, or framework changes unless they clearly pay for themselves.

## Red Flags

Flag these strongly:

- A function that requires reading many unrelated files to understand normal behavior.
- New generic abstractions with only one caller and no concrete second use.
- Boolean flags that radically change behavior without making the call site obvious.
- Long parameter lists where a domain type would clarify intent.
- Repeated blocks with slight variation that can drift independently.
- Hidden mutation of shared state.
- Catch-all error handling that masks bugs or discards context.
- Tests that only assert that code runs, not what behavior matters.
- Comments that explain what the next line does instead of why the code exists.
- Public APIs accidentally exposing internal data structures or naming.
- Compatibility code kept without a concrete shipped behavior, persisted data, or external consumer.

## Good Patterns

Prefer these:

- Early returns or guard clauses over deeply nested control flow.
- Domain types and enums over raw strings, booleans, and loosely shaped dictionaries.
- Small modules organized by responsibility, not by vague buckets like `utils` unless the project already uses that convention.
- Narrow public APIs with private helpers behind them.
- Clear construction paths: builders for many optional settings, direct constructors for small required state.
- Specific error variants or exception types for cases callers may handle differently.
- Tests named after behavior, with arrange/act/assert structure when it improves readability.
- Documentation examples for public APIs that are easy to copy and run.

## Rust Guidance

For Rust code, emphasize type-driven clarity and predictable ownership.

Check for:

- Types and enum variants that encode invariants instead of relying on comments.
- `Result` errors that carry enough context for debugging and callers.
- `Option` used for true absence, not as a vague failure channel.
- Minimal `clone`, `Arc`, `Mutex`, and dynamic dispatch. Use them when ownership, sharing, or extensibility requires them, not to avoid modeling data flow.
- No `unwrap` or `expect` in library/runtime code unless the invariant is obvious and documented. Tests and examples may use them more freely.
- Public items documented when they are part of a crate API.
- Module boundaries that separate core logic from IO, CLI, encoding, rendering, or FFI glue.
- Trait usage that buys substitution or abstraction. Avoid traits for a single concrete implementation unless the boundary is real.
- Lifetimes and generics kept as simple as the actual problem allows.

Good Rust often looks boring: explicit data types, clear ownership, small `impl` blocks, precise errors, and tests around edge cases.

## Python Guidance

For Python code, emphasize readable APIs, straightforward data flow, and consistent style.

Check for:

- PEP 8/project-style consistency, ideally enforced by tools rather than review debate.
- Type hints on public or non-trivial functions when they improve comprehension.
- Explicit public vs internal APIs, using leading underscores and `__all__` where appropriate.
- Specific exception handling. Avoid bare `except` and broad `except Exception` unless logging/cleanup/re-raise semantics are intentional.
- Context managers for resource cleanup.
- Simple data containers (`dataclass`, `TypedDict`, enums) when dictionaries or tuples would hide meaning.
- No surprising mutable defaults.
- No module-level side effects beyond cheap constants, registration required by the framework, or intentional initialization.
- Docstrings on public modules, classes, and functions that explain behavior and parameters.

Good Python often has an obvious top-level API, concise implementation, and tests that read like examples.

## Application Software Guidance

For application code, quality depends on boundaries more than isolated line style.

Check for:

- Clear separation between domain logic, IO, persistence, rendering, transport, and CLI/UI adapters.
- Configuration loaded at the edge and passed inward explicitly.
- Side effects isolated so core behavior is easy to test.
- Data contracts documented and validated at boundaries.
- Observability that helps diagnose production failures without exposing secrets.
- Migrations, schema changes, protocol changes, and cache changes treated as compatibility boundaries.
- Performance-sensitive paths identified before optimizing. Prefer measurements over guesses.
- Security-sensitive paths reviewed explicitly: path handling, subprocesses, deserialization, secrets, auth, sandboxing, and untrusted input.

## Output Format For Reviews

When the user asks for a review, lead with findings. Use this shape:

```markdown
Findings
- [severity] `path:line` Issue and why it matters. Suggested fix.

Open Questions
- Anything needed to judge tradeoffs or compatibility.

Summary
- Brief note on what looks good or what was changed.
```

If there are no findings, say so directly and mention residual risks or tests not run.

## Output Format For Refactors

When making changes, keep them small and explain:

1. What complexity was removed.
2. What behavior was preserved or changed.
3. What tests or checks were run.
4. Any remaining tradeoff.

Do not do broad cleanup unrelated to the user's request. Do not mix formatting-only edits with behavioral edits unless the formatter requires it.

## Inspiration

This skill reflects common guidance from Google Engineering Practices, PEP 8, Rust API Guidelines, and patterns seen in mature open-source projects such as `ripgrep` and `requests`: design first, simplicity over cleverness, clear names, useful comments, focused tests, and consistency with the surrounding codebase.
