---
description: Deep audit mode — systematically audit a specific subsystem for correctness, performance, dead code, and missing edge cases. Eustress-specific.
---

# /audit — Deep Audit Mode

You are switching into **deep audit mode**. The user will name a subsystem or area of concern. Your job is to systematically read every line of relevant code and produce a comprehensive findings report.

## Process

1. **Identify the scope.** Ask the user if not clear: which subsystem, which files, what aspect (correctness? performance? completeness?).

2. **Read all relevant files.** Do not skim. Do not sample. Read every line of every file in the scope. Use `code_search` and `grep_search` to find all references.

3. **Trace every code path.** For each public function or system:
   - What are the inputs?
   - What are the outputs?
   - What are the side effects?
   - What happens on error?
   - What happens on empty/null/zero input?

4. **Check for these categories:**

### Correctness
- Logic errors, off-by-one, wrong comparison operators
- Unhandled None/Err cases
- Stale state after resize, tool change, or mode switch
- Resources initialized but never updated (dead code)
- Resources updated but never read (wasted work)

### Performance
- Per-frame allocations (Vec::new(), String::from() in Update systems)
- Unnecessary .get_mut() on Bevy assets (forces GPU re-upload)
- Slint properties set every frame even when unchanged (forces repaint)
- Systems that run every frame but only need to run on change
- Large data copies that could be references

### Completeness
- Missing feature: callback wired in Slint but no handler in Rust
- Missing feature: handler in Rust but no callback in Slint
- TODO/FIXME/HACK comments — are they still relevant?
- Documented behavior that does not match actual behavior

### Safety
- unwrap() on paths that can fail at runtime
- Index access without bounds check
- Division by zero possibility
- Integer overflow in coordinate math
- File paths constructed from user input without sanitization

## Output Format

### Audit Report Header

Report title, date, scope (files and directories examined), approximate lines examined.

### Summary

2-3 sentence overview of findings.

### Findings (ordered by severity)

For each finding:
- Severity tag: CRITICAL, HIGH, MEDIUM, or LOW
- Title (one line)
- File path and line number
- Category (Correctness, Performance, Completeness, Safety)
- Description of what is wrong
- Impact (what breaks)
- Concrete fix (not "consider improving")

### Statistics

- Files examined count
- Total findings count broken down by severity
- Dead code found count
- Missing handlers count
- Performance issues count

### Recommendations

Prioritized action items (numbered list).

## Rules

- Every finding must cite a specific file and line number.
- Every finding must have a concrete fix, not "consider improving."
- Findings are ordered by severity: CRITICAL then HIGH then MEDIUM then LOW.
- If you find zero issues, say so honestly. Do not invent findings.
- Do not repeat findings already fixed in the current branch.
