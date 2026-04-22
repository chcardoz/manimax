# 0003 — Default to "what manimgl does" over "what's technically correct"

**Date:** 2026-04-21
**Status:** accepted — general policy

## Decision

When a design choice has a "technically correct" answer and a "manimgl-compatible" answer, **default to the manimgl-compatible one.** Deviate only with a conscious ADR that names what motivates the deviation.

## Why

- **Primary goal is being a drop-in ManimGL replacement** (see `AGENTS.md`/`CLAUDE.md`). Surprising divergences compound into porting pain — every scene author re-learns an old rule on new tooling.
- **Manimgl's choices encode years of empirical tradeoffs.** "Technically correct" on paper often loses to "what Grant found worked" in practice (e.g. sRGB floats vs linear color: the whole animation library — palettes, fades, compositing — was tuned against sRGB semantics).
- **Deviations should be rare and intentional.** Writing an ADR when we deviate forces the question "is this worth the divergence?" — which is the right question to ask.
- **Agents default to "clean engineering" without this rule.** Multiple Slice B decisions (color space, Vec3 vs Vec2, easing semantics) started with "the right thing is…" before being redirected back to manimgl. Codifying the rule saves that round-trip.

## Consequences

- **Buys:** scene-author mental model transfers directly; manimgl examples and teaching material translate 1:1; easier to diff behavior against the reference submodule.
- **Locks out:** a small amount of correctness purity (linear color, strict physical-unit coordinates). We accept this.
- **Implied workflow:** before designing a new subsystem, the first question is "what does manimgl do here?" — answered by reading `reference/manimgl/`, not by memory.

## Precedents set by this rule

- **sRGB floats for all color.** Linear would be correct; manimgl's entire palette tuning is in sRGB. See `docs/ir-schema.md` + Slice B §2.
- **`Vec3` coordinates everywhere, even in 2D scenes.** `Vec2` would be more honest; manimgl uses 3D points universally (Z unused in 2D scenes) and every `animation/` algorithm matches that shape. See `crates/manim-rs-ir/src/lib.rs`.

## Rejected alternatives

- **"Correct-by-default, manimgl-compatible as opt-in."** Inverts the choice we want. Would make Manimax technically cleaner and practically harder to port *to*.
- **"Match manimgl silently."** Loses the ADR discipline. Without ADRs, a future agent undoes the rule unconsciously.
- **"Match manimgl only where user-visible."** The line between "user-visible" and "internal" is fuzzy for rendering semantics (color compositing, interpolation, coordinate conventions all leak).

## When to deviate

Legitimate reasons to diverge, each of which warrants a new ADR:

- Performance: if matching manimgl forces an algorithm that can't be parallelized, we own that divergence explicitly.
- Type safety: where manimgl relies on Python duck-typing in a way that can't be expressed in Rust without pervasive `enum`/`dyn`.
- Architectural thesis: anything where matching manimgl would re-couple frames (see `docs/porting-notes/eval.md` purity contract).

Everything else: match manimgl.
