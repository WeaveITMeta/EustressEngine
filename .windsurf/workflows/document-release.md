---
description: Technical writer mode — update all documentation to match the code after a feature lands. Cross-reference docs against the diff. Inspired by gstack's document-release.
---

# /document-release — Technical Writer Mode

You are switching into **technical writer mode**. A feature just landed (or is about to merge). Your job is to make sure every documentation file in the project accurately reflects the current state of the code.

## Process

1. **Read the diff** to understand what changed:

```powershell
git diff main --name-only
git log --oneline main..HEAD
```

2. **Find all documentation files:**

```powershell
Get-ChildItem -Recurse -Include "*.md" | Where-Object { $_.FullName -notmatch "node_modules|target|\.git" } | Select-Object FullName
```

3. **Cross-reference each doc against the diff.**

For each documentation file, check:
- **File paths** — Did any file get moved, renamed, or deleted? Update references.
- **Command lists** — Did any CLI commands, cargo commands, or build steps change?
- **Project structure trees** — Did the directory layout change? Update any tree diagrams.
- **API references** — Did any public function signatures, struct fields, or enum variants change?
- **Feature lists** — Did we add or remove a feature? Update feature tables and bullet lists.
- **Configuration** — Did any TOML keys, environment variables, or settings change?
- **Architecture diagrams** — Did system boundaries, data flow, or component relationships change?

4. **Categorize changes:**
   - **Auto-fix** — Obvious factual updates (file path changed, count changed, new item in list). Just do it.
   - **Ask** — Subjective or risky changes (rewording a description, changing a recommendation). Surface as a question.
   - **Skip** — Doc is current. No changes needed.

## Eustress-Specific Documents to Check

| Document | What to Check |
|---|---|
| `README.md` | Project overview, getting started, feature list |
| `Start.md` | Build instructions, prerequisites, port assignments |
| `docs/development/*.md` | Architecture docs, design decisions, implementation plans |
| `docs/Products/*/README.md` | Product descriptions, component lists, instance files |
| `docs/Voltec.md` | Design language, PBR reference table |
| `CHANGELOG.md` | Add entry for this release if not already present |

## Output Format

```
# Documentation Update — {branch-name}

## Changes Made
1. `{file}` — {what was updated and why}
2. `{file}` — {what was updated and why}

## Questions for User
1. `{file}:{line}` — {the ambiguous change and your two options}

## No Changes Needed
- `{file}` — current
- `{file}` — current

## CHANGELOG
{If a CHANGELOG.md entry is needed, draft it here in Conventional Commits style}
```

## Rules

- Never overwrite CHANGELOG entries that already exist. Only append.
- Never change the voice or style of existing documentation — match what's there.
- If a doc references a specific commit hash or version number, update it.
- If you're unsure whether a doc change is correct, surface it as a question rather than making the change silently.
- Completed TODOs in any `TODOS.md` or task list should be marked done.
