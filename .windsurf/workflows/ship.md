---
description: Release engineer mode — sync, test, commit, push, and land the branch. No more talking, just execution. Inspired by gstack's ship.
---

# /ship — Release Engineer Mode

You are switching into **release engineer mode**. The interesting work is done. The product thinking is done. The architecture is done. The review pass is done. Now the branch just needs to get landed.

Do NOT brainstorm. Do NOT suggest improvements. Execute the release checklist.

## Pre-Flight Checks

1. **Verify branch state.**

```powershell
git status
git log --oneline -5
```

Confirm: no untracked files that should be committed, no uncommitted changes that belong to this feature.

2. **Sync with main.**

```powershell
git fetch origin main
git rebase origin/main
```

If there are conflicts, resolve them. If the conflicts are non-trivial, stop and ask the user.

3. **Build.**

// turbo
```powershell
cargo build -p eustress-engine --bin eustress-engine 2>&1 | Select-Object -Last 10
```

Must exit 0 with no errors (warnings are acceptable).

4. **Run tests.**

// turbo
```powershell
cargo test --workspace 2>&1 | Select-Object -Last 20
```

All tests must pass. If any fail, stop and fix them before continuing.

5. **Check for new warnings.** Scan the build output for any NEW warnings introduced by this branch. Existing warnings are acceptable. New ones introduced by our changes should be fixed.

## Ship It

6. **Stage and commit** any remaining changes with a clean Conventional Commits message:

```
<type>(<scope>): <description>

<body explaining what and why>
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `chore`, `test`, `style`, `build`.

7. **Push the branch.**

```powershell
git push origin HEAD
```

8. **Generate changelog entry** using the `generate-changelog` skill if available, or manually summarize the commits on this branch vs main.

## Post-Ship

9. **Report to the user:**

```
Branch: <branch-name>
Commits: <count> commits ahead of main
Build: PASS
Tests: PASS (<count> passed, <count> skipped)
Pushed: YES

Summary:
- <one-line per commit>
```

## Rules

- Do NOT skip the build step. Ever.
- Do NOT skip the test step. Ever.
- Do NOT force-push unless the user explicitly asks.
- If anything fails, stop and report. Do not silently continue.
- Momentum matters. Do not pause to ask permission between steps unless something fails.
