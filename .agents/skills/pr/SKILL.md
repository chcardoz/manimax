---
name: pr
description: 'Create or update a GitHub pull request with a diff-driven description. Use when user asks to create a PR, open a pull request, or mentions "/pr".'
license: MIT
allowed-tools: Bash
---

# Create / Update Pull Request

Diff-driven PR creation against `origin/main`. Works with worktrees where local `main` may be stale.

## Workflow

### 1. Preflight

```bash
# Must not be on main/master
git rev-parse --abbrev-ref HEAD

# Verify gh is authenticated
gh auth status
```

If on main/master, stop and tell the user.

### 2. Fetch and diff against origin/main

```bash
git fetch origin main
git log origin/main..HEAD --oneline
git diff origin/main...HEAD
```

Use the **full diff** to understand all changes — not just recent commits.

### 3. Push to origin

```bash
git push -u origin HEAD
```

### 4. Create or update PR

**If no PR exists:**

```bash
gh pr create --base main --title "<title>" --body "<body>"
```

**If PR already exists:**

```bash
gh pr edit --title "<title>" --body "<body>"
```

Rewrite the title and description from scratch based on the current diff. Do not append changelog entries like "also adds" or "now includes."

## PR format

- **Title**: Present tense, imperative mood, <80 characters
- **Body** (<5 sentences unless user says otherwise):
  - **Why** — motivation or problem
  - **What** — summary of all changes in the diff
  - **How to test** — brief verification steps

## Safety

- NEVER force push
- NEVER push to main/master
- NEVER add a Co-Authored-By line
- If any step fails, ask the user for help
