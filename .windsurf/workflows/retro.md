---
description: Engineering retrospective — analyze commit history, work patterns, and shipping velocity. Data-driven weekly review. Inspired by gstack's retro.
---

# /retro — Engineering Manager Retrospective Mode

You are switching into **engineering manager mode**. At the end of a work session or week, you want to know what actually happened. Not vibes — data.

## Process

1. **Gather commit data** for the period (default: last 7 days):

```powershell
git log --since="7 days ago" --format="%H|%an|%ae|%ai|%s" --numstat
```

2. **Gather branch info:**

```powershell
git branch -a --sort=-committerdate | Select-Object -First 20
git log --oneline main..HEAD
```

3. **Analyze and produce the retrospective.**

## Metrics to Compute

- **Commit count** — total and per-contributor
- **Lines of code** — added, removed, net (from `--numstat`)
- **Files touched** — which crates/modules got the most activity
- **Hotspot files** — files changed in 3+ commits (churn indicator)
- **Commit frequency** — commits per day, peak hours
- **Test ratio** — approximate: (lines in `*_test.rs` or `#[test]`) / total lines changed
- **Feature vs fix ratio** — count of `feat:` vs `fix:` vs `refactor:` vs `chore:` commits
- **Biggest ship** — the single most impactful commit or feature (by LOC + description)

## Eustress-Specific Analysis

Map commits to engine subsystems:

| Directory Pattern | Subsystem |
|---|---|
| `src/ui/` | Slint UI / Rendering Pipeline |
| `src/camera_*` | Camera System |
| `src/select_tool*`, `src/move_tool*`, `src/rotate_tool*`, `src/scale_tool*` | Tool System |
| `src/space/` | Space / Scene Management |
| `src/workshop/` | Workshop / AI Pipeline |
| `src/soul/` | Soul Service / AI Integration |
| `ui/slint/` | Slint Markup / UI Design |
| `crates/common/` | Common Library |
| `crates/mcp/` | MCP Server |
| `docs/` | Documentation / Design Docs |
| `docs/Products/` | Product Catalog |

## Output Format

```
# Retrospective — Week of {date}

## Summary
{commits} commits | +{added} -{removed} LOC | {files} files | {contributors} contributors

## Highlights
- **Biggest Ship:** {title} — {one-line description}
- **Most Active Subsystem:** {subsystem} ({commit count} commits)
- **Hotspot Files:** {list of churned files}

## Per-Subsystem Breakdown
| Subsystem | Commits | LOC +/- | Key Changes |
|---|---|---|---|
| ... | ... | ... | ... |

## What Went Well
1. {specific praise with evidence}
2. {specific praise with evidence}
3. {specific praise with evidence}

## What Could Improve
1. {specific observation with suggestion}
2. {specific observation with suggestion}
3. {specific observation with suggestion}

## Habits for Next Week
1. {actionable habit}
2. {actionable habit}
3. {actionable habit}
```

## Rules

- Be candid. This is not a feel-good report. Specific praise, specific criticism.
- Every observation must cite evidence (commit hash, file, metric).
- "What could improve" must be actionable — not "write more tests" but "the move_tool.rs had 4 fix commits in 3 days; add integration tests for drag-across-viewport-boundary before the next tool change."
- If there are fewer than 5 commits in the period, say so and suggest a shorter retrospective window.
