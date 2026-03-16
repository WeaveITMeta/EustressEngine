---
description: QA lead mode — build, test, run the engine, and verify nothing is broken. Diff-aware testing for Rust/Bevy/Slint desktop app. Inspired by gstack's qa.
---

# /qa — QA Lead Mode

You are switching into **QA lead mode**. Your job is to verify that the current branch works correctly. Not "looks right in the diff" — actually works.

## Modes

### Diff-Aware (default on feature branches)

1. **Read the diff** to understand what changed:

```powershell
git diff main --stat
git diff main --name-only
```

2. **Categorize the changes:**
   - **Rust source** (`*.rs`) — needs build + test
   - **Slint UI** (`*.slint`) — needs build + visual verification
   - **TOML config** (`*.toml`) — needs build + runtime check
   - **Documentation** (`*.md`) — no build needed, check for accuracy
   - **Blender scripts** (`*.py`) — needs headless Blender run
   - **Instance files** (`*.glb.toml`) — needs runtime load check

3. **Build the engine:**

// turbo
```powershell
cargo build -p eustress-engine --bin eustress-engine 2>&1 | Select-Object -Last 10
```

4. **Run workspace tests:**

// turbo
```powershell
cargo test --workspace 2>&1 | Select-Object -Last 30
```

5. **Run the engine** and check for runtime errors (panics, warnings, missing assets):

```powershell
cargo run -p eustress-engine --bin eustress-engine 2>&1
```

Let it run for ~10 seconds, then check the output for:
- Panics or crash backtraces
- `ERROR` or `WARN` log lines that are NEW (not pre-existing)
- Missing asset warnings
- Slint rendering issues (check for "SlintScene entity" warnings)
- Window resize handling
- Scene diagnostic output (camera count, mesh count, entity count)

6. **Report findings.**

### Full (when asked or for release candidates)

Run all of the above PLUS:
- `cargo clippy --workspace 2>&1` — check for lint issues
- `cargo doc --workspace --no-deps 2>&1` — check for doc build issues
- Verify every `.glb.toml` in the Space directory loads without error
- Check that all Slint callbacks in `slint_ui.rs` have matching handlers in `drain_slint_actions`

### Quick (`/qa --quick`)

Build + test only. No runtime check.

// turbo
```powershell
cargo build -p eustress-engine --bin eustress-engine 2>&1 | Select-Object -Last 5
cargo test --workspace 2>&1 | Select-Object -Last 10
```

## QA Report Format

```
# QA Report — {branch-name}
Date: {ISO 8601}
Mode: {Diff-Aware | Full | Quick}

## Build
- Status: PASS / FAIL
- Warnings: {count} ({count} new)
- Errors: {count}

## Tests
- Status: PASS / FAIL
- Passed: {count}
- Failed: {count}
- Skipped: {count}

## Runtime ({if applicable})
- Launched: YES / NO
- Panics: {count}
- New Warnings: {list}
- Scene loaded: YES / NO ({entity count} entities, {mesh count} meshes)
- Slint UI rendered: YES / NO
- Window resize: OK / BROKEN

## Diff Analysis
- Files changed: {count}
- Affected systems: {list of Bevy systems/plugins touched}
- Risk areas: {list of areas that could regress}

## Issues Found
1. [SEVERITY] {description} — {file:line}

## Verdict
{SHIP IT / FIX FIRST / NEEDS MORE TESTING}
```

## Rules

- Always build before testing. Always test before runtime checks.
- Distinguish between NEW issues (introduced by this branch) and PRE-EXISTING issues.
- If the build fails, stop immediately. Do not attempt runtime checks on a broken build.
- If tests fail, report which tests and why. Check if the failure is related to the diff.
- Runtime checks should be quick — 10-15 seconds max. Kill the process after checking output.
