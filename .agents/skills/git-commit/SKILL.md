---
name: git-commit
description: 'Execute git commit with conventional commit message analysis, intelligent staging, and message generation. Use when user asks to commit changes, create a git commit, or mentions "/commit". Supports: (1) Auto-detecting type and scope from changes, (2) Generating conventional commit messages from diff, (3) Interactive commit with optional type/scope/description overrides, (4) Intelligent file staging for logical grouping'
license: MIT
allowed-tools: Bash
---

# Git Commit with Conventional Commits

## Overview

Create standardized, semantic git commits using the Conventional Commits specification. Analyze the actual diff to determine appropriate type, scope, and message.

## Conventional Commit Format

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

## Commit Types

| Type       | Purpose                        |
| ---------- | ------------------------------ |
| `feat`     | New feature                    |
| `fix`      | Bug fix                        |
| `docs`     | Documentation only             |
| `style`    | Formatting/style (no logic)    |
| `refactor` | Code refactor (no feature/fix) |
| `perf`     | Performance improvement        |
| `test`     | Add/update tests               |
| `build`    | Build system/dependencies      |
| `ci`       | CI/config changes              |
| `chore`    | Maintenance/misc               |
| `revert`   | Revert commit                  |

## Breaking Changes

```
# Exclamation mark after type/scope
feat!: remove deprecated endpoint

# BREAKING CHANGE footer
feat: allow config to extend other configs

BREAKING CHANGE: `extends` key behavior changed
```

## Workflow

### 1. Analyze Diff

```bash
# If files are staged, use staged diff
git diff --staged

# If nothing staged, use working tree diff
git diff

# Also check status
git status --porcelain
```

### 2. Stage Files (if needed)

If nothing is staged or you want to group changes differently:

```bash
# Stage specific files
git add path/to/file1 path/to/file2

# Stage by pattern
git add *.test.*
git add src/components/*

# Interactive staging
git add -p
```

**Never commit secrets** (.env, credentials.json, private keys).

### 3. Generate Commit Message

Analyze the diff to determine:

- **Type**: What kind of change is this?
- **Scope**: What area/module is affected?
- **Description**: One-line summary of what changed (present tense, imperative mood, <72 chars)
- **Body**: Default to **none**. Only add a body when the WHY is non-obvious to a reader six
  months from now — e.g. a deletion (no diff line tells you why something was removed), a
  workaround for a bug or upstream issue, an architectural call between credible alternatives,
  a change motivated by something outside the diff (incident, deadline, ADR, perf number).
  For routine code, docs, tests, or refactors, ship the title alone.
- **Footer**: Issue references (`Closes #123`, `Refs #456`), `BREAKING CHANGE:`. Don't add
  hand-wavy "generated with X" or summary footers unless the user asks.

### 4. Execute Commit

**Default — single line** (matches the "no body" default from Step 3):

```bash
git commit -m "<type>[scope]: <description>"
```

**Only when Step 3's body criteria fired** — multi-line HEREDOC:

```bash
git commit -m "$(cat <<'EOF'
<type>[scope]: <description>

<body — tight; ≤4 short lines or ≤6 short bullets>
EOF
)"
```

PR descriptions belong in PR bodies, not commit messages.

### 5. Verify Commit

After a successful commit, always run `git status` to show remaining changes:

```bash
git status
```

## Best Practices

- One logical change per commit
- Present tense: "add" not "added"
- Imperative mood: "fix bug" not "fixes bug"
- Reference issues: `Closes #123`, `Refs #456`
- Keep description under 72 characters
- Default to a single-line commit (no body). Add a body only when the WHY is non-obvious — see Step 3.
- Body length cap: ≤4 short lines or ≤6 short bullets. Don't restate what the diff already says.

## Git Safety Protocol

- NEVER update git config
- NEVER run destructive commands (--force, hard reset) without explicit request
- NEVER skip hooks (--no-verify) unless user asks
- NEVER force push to main/master
- If commit fails due to hooks, fix and create NEW commit (don't amend)
- NEVER add a Co-Authored-By line to commit messages
