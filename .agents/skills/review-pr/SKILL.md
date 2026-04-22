---
name: review-pr
description: Multi-agent code review of a GitHub pull request with confidence-scored issue filtering. Use when the user asks to review a PR, audit a pull request, or invokes /review-pr.
---

# Review PR: Multi-Agent Code Review

Review a GitHub pull request using parallel specialist agents and post the result as a PR comment. The workflow is designed to surface real issues while suppressing nitpicks, false positives, and things CI would catch.

## Inputs

The user provides a PR number (e.g. `review-pr 2` or a GitHub URL). If omitted, ask.

## Workflow

Follow these steps in order. Make a todo list first.

### 1. Eligibility check (Haiku)

Launch one Haiku agent that runs `gh pr view <N> --json state,isDraft,author,title,reviews,comments` and reports whether the PR is:

- (a) closed or merged
- (b) a draft
- (c) obviously trivial/automated (tiny, dependabot, release bumps)
- (d) already has a code review comment from the current `gh` user that starts with `### Code review` (Claude-generated review)

If any are true, stop and tell the user why. Otherwise, continue.

### 2. Collect relevant CLAUDE.md paths (Haiku)

Launch one Haiku agent to enumerate absolute paths to every `CLAUDE.md` that could apply to the PR:

- The root `CLAUDE.md`
- Any `CLAUDE.md` under directories touched by the diff (walk up from each modified file)

Exclude vendored submodules (e.g. `reference/`, `.venv/`). Return just the list of paths.

### 3. Summarize the PR (Haiku)

Launch one Haiku agent to view the PR (`gh pr view <N>` and `gh pr diff <N> --name-only`) and return a ~200-word summary: what it does, which subsystems it touches, the goal.

### 4. Five parallel Sonnet reviewers

Launch all five in a single message (parallel). Pass each the PR number and the CLAUDE.md paths from step 2. Each returns a list of issues with file paths, line ranges, and reasons.

- **Agent 1 — CLAUDE.md compliance.** Read the CLAUDE.md files first, then audit the diff against their specific rules. Focus on: commented-out code, unused `_`-prefixed vars kept as back-compat, re-exports for back-compat, "removed" comments, half-finished implementations, feature flags/shims where code could just be changed, comments that explain WHAT instead of WHY, comments that reference tasks/callers/issue numbers.
- **Agent 2 — Shallow bug scan.** Read only the diff (don't read extra files unless essential to confirm a suspicion). Look for large bugs: off-by-one, wrong condition, swapped args, wrong variable used, resource leaks, panics that shouldn't panic, serialization/byte-order/alignment, concurrency hazards, API contract violations. Ignore style, lint, type errors, test coverage, docs, nitpicks.
- **Agent 3 — Git history.** Get `gh pr diff <N> --name-only`, then for the most heavily-modified files run `git log origin/main -- <file>` and `git blame origin/main -- <file>`. Flag: regressions of earlier fixes, reintroduction of intentionally-removed behavior, contradictions of decisions in prior commit messages, removal of explicitly-added invariants.
- **Agent 4 — Prior-PR comments.** Enumerate prior PRs that touched the same files (`gh pr list --state all --json number,title,files --limit 50`) and read their comments (`gh pr view <N> --comments`). Surface patterns or previously-raised concerns that apply again.
- **Agent 5 — In-file comments.** For each modified file, inspect in-file code comments (both added and unchanged) including doc headers, SAFETY notes, invariants, PORT_STUB markers. Flag changes that violate guidance those comments state.

### 5. Score each issue (parallel Haiku)

Launch one Haiku agent per issue. Give each: the PR number, the issue description, and the CLAUDE.md paths. The agent verifies the issue and scores confidence 0–100 using this rubric (pass it verbatim):

- **0** — Not confident at all. False positive that doesn't stand up to light scrutiny, or pre-existing issue.
- **25** — Somewhat confident. Might be a real issue, but couldn't verify. If stylistic, not explicitly called out in CLAUDE.md.
- **50** — Moderately confident. Verified as real, but a nitpick or rare in practice; not important relative to the rest of the PR.
- **75** — Highly confident. Double-checked; very likely to be hit in practice. Existing approach is insufficient. Important to functionality, or directly mentioned in CLAUDE.md.
- **100** — Absolutely certain. Double-checked, confirmed real, will happen frequently. Evidence directly confirms.

For CLAUDE.md-cited issues, the scorer must verify the CLAUDE.md actually calls out that specific rule. If it doesn't, score low.

### 6. Filter

Drop any issue scoring `< 80`. If none remain, skip step 7 investigation and go straight to step 8 with the "No issues found" comment.

### 7. Re-check eligibility

Repeat step 1 — the PR state may have changed during the run.

### 8. Post the review

Comment on the PR via `gh pr comment <N> --body "..."`. Keep it brief, no emojis, cite and link every finding.

## What counts as a false positive

Score these at 0–25 and drop them:

- Pre-existing issues
- Looks like a bug but isn't
- Pedantic nitpicks a senior engineer wouldn't flag
- Anything a linter/typechecker/compiler/CI would catch (imports, types, formatting, broken tests)
- General code-quality issues (test coverage, docs, security) unless explicitly required by CLAUDE.md
- Issues flagged in CLAUDE.md but silenced via lint-ignore comment
- Changes that are likely intentional and part of the broader PR intent
- Real issues on lines the PR did not modify

Do not run builds, typecheckers, or tests — CI handles those.

## Comment format

Use this exact structure. For citations, use full-SHA GitHub blob URLs (`gh pr view <N> --json headRefOid` to get the SHA) with format `https://github.com/<owner>/<repo>/blob/<SHA>/<path>#L<start>-L<end>`, providing at least one line of context before and after the cited line.

If issues found:

```markdown
### Code review

Found N issues:

1. <brief description> (CLAUDE.md says "<quote>")

<full-SHA blob link with line range>

2. <brief description> (some/other/CLAUDE.md says "<quote>")

<full-SHA blob link with line range>

N. <brief description> (bug due to <file and snippet>)

<full-SHA blob link with line range>

🤖 Generated with [Claude Code](https://claude.ai/code)

<sub>- If this code review was useful, please react with 👍. Otherwise, react with 👎.</sub>
```

If no issues:

```markdown
### Code review

No issues found. Checked for bugs and CLAUDE.md compliance.

🤖 Generated with [Claude Code](https://claude.ai/code)

<sub>- If this code review was useful, please react with 👍. Otherwise, react with 👎.</sub>
```

## Safety

- Never push, never modify the PR branch, never approve/merge.
- Only `gh pr view`, `gh pr diff`, `gh pr list`, `gh pr comment`, and local `git log`/`git blame`.
- Never use bash substitution (e.g. `$(git rev-parse HEAD)`) inside the comment body — the URL won't render. Resolve the SHA first, then paste it literally.
- Don't attempt to build, typecheck, or run tests.
