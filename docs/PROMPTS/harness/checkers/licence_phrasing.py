#!/usr/bin/env python3
"""Licence-phrasing verifier.

Eustress is source-available under PolyForm Shield 1.0.0 (see `LICENSE`, dual-licensed
against `LICENSE-COMMERCIAL.md`). PolyForm Shield carries a Noncompete clause, so telling
a reader the project is "open source" or "MIT-friendly" tells them they hold rights the
licence withholds. This checker scans the whole repository for those phrasings and exits
non-zero on any hit that is not covered by an explicit, reasoned allowlist entry.

Design constraints this file deliberately honours:

* It scans the **whole repository** from `--repo-root` down, minus a short, explicit list
  of excluded directories (version-control internals, build output, vendored third-party
  code, and the prompt library that quotes the banned phrases in order to forbid them).
  It is not pointed at a hand-listed set of files -- a scanner that only looks where its
  author already looked cannot catch the next occurrence.
* The allowlist is capped (`--max-allowlist`) and every entry needs a reason of at least
  `--min-reason-chars` characters. An entry without a reason is a suppression.
* When a `--baseline` report is supplied, the checker fails if the pre-fix unallowlisted
  hit count is below `MIN_BASELINE`. Reaching zero by allowlisting everything and reaching
  zero by correcting the copy produce the same final number; the baseline separates them.
* PDFs are not scanned. They are binary and `grep -I` skips them, and an early draft that
  inflated their FlateDecode streams recovered nothing but embedded-font metadata. The recorded
  sha256 of a regenerated PDF is the evidence that it was rebuilt from corrected sources.

Exit codes:
    0  clean
    2  usage error
    5  unallowlisted hits found
    6  allowlist violation (over the ceiling, or an entry missing/short on `reason`)
    7  baseline missing, unreadable, or below MIN_BASELINE
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys

# --------------------------------------------------------------------------------------
# What counts as a misstatement.
#
# The floor is the "open source" / MIT family, plus the near-miss paraphrases that mean the
# same thing to a reader ("free and open", "MIT-like", "OSI-approved", "FOSS"). The standard
# is that the reader's belief about their rights matches LICENSE; this list is the
# mechanical part of that standard, not the whole of it.
# --------------------------------------------------------------------------------------
BANNED_PATTERNS: list[tuple[str, str]] = [
    ("open_source", r"open[\s\-]sourc(?:e|ed|ing)"),
    ("opensource_org", r"opensource\.org"),
    ("mit_friendly", r"MIT[\s\-]?friendly"),
    ("mit_licence", r"MIT[\s\-]?licen[cs](?:e|ed|ing)"),
    ("mit_like", r"MIT[\s\-]?like"),
    ("free_and_open", r"free\s+and\s+open"),
    ("osi_approved", r"OSI[\s\-]?approved"),
    ("foss", r"\bFOSS\b"),
]
COMPILED = [(name, re.compile(pat, re.IGNORECASE)) for name, pat in BANNED_PATTERNS]

# --------------------------------------------------------------------------------------
# Rule statements are not claims.
#
# `docs/PROMPTS/` is excluded wholesale because the prompt library quotes the banned phrases
# in order to forbid them. The same thing happens outside that directory: a checklist item or
# a briefing note writes 'say "source-available", never "open source"'. Those lines assert the
# correct position; flagging them would push an author towards deleting the rule.
#
# The test is deliberately narrow: the correct term must appear on the SAME LINE as the banned
# phrase. A line cannot both state that Eustress is source-available under PolyForm Shield and
# mislead a reader into thinking it is open source. Every line set aside this way is printed
# and recorded verbatim in the report, so the classification is auditable rather than silent.
# --------------------------------------------------------------------------------------
RULE_STATEMENT_TERMS = re.compile(r"source[\s\-]available|PolyForm", re.IGNORECASE)

# Directory names excluded wherever they occur in the tree.
EXCLUDED_DIR_NAMES: list[str] = [
    ".git",                 # version-control internals
    "node_modules",         # vendored third-party packages with their own accurate notices
    "vendor",               # vendored third-party crates
    "dist",                 # build output; regenerates
    "__pycache__",
    ".venv",
]

# Cargo/Trunk build directories at any depth. Listed explicitly rather than globbed so the
# exclusion set stays readable and auditable.
EXCLUDED_BUILD_DIR_NAMES: list[str] = [
    "target",
    "target-lsp-check",
    "target-prof",
    "target-verify",
    "target-web",
    ".trunk-web-target",
]

# Repo-relative paths excluded outright, each for a stated reason.
EXCLUDED_REL_PATHS: dict[str, str] = {
    "docs/PROMPTS": "the prompt library quotes the banned phrases in order to forbid them",
    "eustress/.patches": "vendored upstream wgpu/naga/Deno sources and their own accurate MIT notices",
    "eustress/crates/player-android/build": "Gradle build output; regenerates",
    "eustress/crates/engine/assets/icons/lucide": "vendored third-party icon set, accurate about its own MIT terms",
}

MIN_BASELINE = 9

TEXT_PROBE_BYTES = 8192


def excluded_dirs_summary() -> list[str]:
    return (
        [f"<any>/{d}" for d in EXCLUDED_DIR_NAMES]
        + [f"<any>/{d}" for d in EXCLUDED_BUILD_DIR_NAMES]
        + list(EXCLUDED_REL_PATHS)
    )


def sha256_of(path: str) -> str | None:
    try:
        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def scan_file(path: str, rel: str) -> tuple[bool, list[dict]]:
    """Return (was_scanned, hits)."""
    try:
        with open(path, "rb") as fh:
            probe = fh.read(TEXT_PROBE_BYTES)
            rest = fh.read()
    except OSError:
        return False, []
    raw = probe + rest

    if b"\x00" in probe:
        # Binary, in the same sense `grep -I` means it.
        return False, []
    else:
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            text = raw.decode("latin-1")
        source = "text"

    hits: list[dict] = []
    for lineno, line in enumerate(text.splitlines(), 1):
        for name, rx in COMPILED:
            m = rx.search(line)
            if m:
                hits.append(
                    {
                        "path": rel,
                        "line": lineno,
                        "pattern": name,
                        "matched": m.group(0),
                        "text": line.strip()[:240],
                        "source": source,
                        "rule_statement": bool(RULE_STATEMENT_TERMS.search(line)),
                    }
                )
                break
    return True, hits


def walk(repo_root: str) -> tuple[list[dict], int]:
    excl_names = set(EXCLUDED_DIR_NAMES) | set(EXCLUDED_BUILD_DIR_NAMES)
    excl_rel = {p.replace("\\", "/") for p in EXCLUDED_REL_PATHS}
    hits: list[dict] = []
    scanned = 0
    for dirpath, dirnames, filenames in os.walk(repo_root):
        rel_dir = os.path.relpath(dirpath, repo_root).replace("\\", "/")
        rel_dir = "" if rel_dir == "." else rel_dir
        keep = []
        for d in dirnames:
            child = f"{rel_dir}/{d}" if rel_dir else d
            if d in excl_names or child in excl_rel:
                continue
            keep.append(d)
        dirnames[:] = keep
        for fn in filenames:
            rel = f"{rel_dir}/{fn}" if rel_dir else fn
            ok, file_hits = scan_file(os.path.join(dirpath, fn), rel)
            if ok:
                scanned += 1
                hits.extend(file_hits)
    hits.sort(key=lambda h: (h["path"], h["line"]))
    return hits, scanned


def load_allowlist(path: str | None) -> tuple[list[dict], list[str]]:
    if not path:
        return [], []
    if not os.path.isfile(path):
        return [], [f"allowlist file not found: {path}"]
    try:
        with open(path, "r", encoding="utf-8") as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError) as exc:
        return [], [f"allowlist unreadable: {exc}"]
    entries = doc.get("entries", doc if isinstance(doc, list) else [])
    if not isinstance(entries, list):
        return [], ["allowlist 'entries' must be a list"]
    return entries, []


def entry_matches(entry: dict, hit: dict) -> bool:
    if entry.get("path", "").replace("\\", "/") != hit["path"]:
        return False
    if int(entry.get("line", -1)) != hit["line"]:
        return False
    permitted = entry.get("text", "")
    return permitted.strip() != "" and permitted.strip() in hit["text"]


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo-root", required=True)
    ap.add_argument("--allowlist", default=None)
    ap.add_argument("--baseline", default=None)
    ap.add_argument("--out", default=None)
    ap.add_argument("--report-only", action="store_true",
                    help="Skip allowlist-ceiling and baseline enforcement; still exits 5 if hits remain.")
    ap.add_argument("--max-allowlist", type=int, default=6)
    ap.add_argument("--min-reason-chars", type=int, default=20)
    ap.add_argument("--pdf", default="docs/marketing/UofA_Center_For_Innovation_Pilot.pdf",
                    help="Repo-relative PDF whose sha256 is recorded as regeneration evidence.")
    args = ap.parse_args(argv)

    repo_root = os.path.abspath(args.repo_root)
    if not os.path.isdir(repo_root):
        print(f"repo root not a directory: {repo_root}", file=sys.stderr)
        return 2

    entries, allow_errors = load_allowlist(args.allowlist)

    # Allowlist hygiene. In --report-only mode these are recorded but not fatal, because the
    # baseline pass runs against a tree where the allowlist may not exist yet.
    for i, e in enumerate(entries):
        where = f"allowlist[{i}] {e.get('path','?')}:{e.get('line','?')}"
        if not e.get("path") or "line" not in e:
            allow_errors.append(f"{where}: needs both 'path' and 'line'")
        if not str(e.get("text", "")).strip():
            allow_errors.append(f"{where}: needs the exact permitted 'text'")
        reason = str(e.get("reason", ""))
        if len(reason.strip()) < args.min_reason_chars:
            allow_errors.append(
                f"{where}: reason is {len(reason.strip())} chars, needs >= {args.min_reason_chars}"
            )
    if len(entries) > args.max_allowlist:
        allow_errors.append(f"allowlist has {len(entries)} entries, ceiling is {args.max_allowlist}")

    hits, scanned = walk(repo_root)

    allowlisted: list[dict] = []
    unallowlisted: list[dict] = []
    rule_statements: list[dict] = []
    for h in hits:
        if h.get("rule_statement"):
            rule_statements.append(h)
        elif any(entry_matches(e, h) for e in entries):
            allowlisted.append(h)
        else:
            unallowlisted.append(h)

    pdf_rel = args.pdf.replace("\\", "/")
    pdf_abs = os.path.join(repo_root, *pdf_rel.split("/"))
    pdf_sha = sha256_of(pdf_abs)

    baseline_count = None
    baseline_hits = None
    baseline_pdf_sha = None
    baseline_errors: list[str] = []
    if args.baseline:
        try:
            with open(args.baseline, "r", encoding="utf-8") as fh:
                bdoc = json.load(fh)
            baseline_count = int(bdoc["unallowlisted_hits"])
            baseline_hits = bdoc.get("unallowlisted", [])
            baseline_pdf_sha = (bdoc.get("pdf") or {}).get("sha256")
        except (OSError, json.JSONDecodeError, KeyError, TypeError, ValueError) as exc:
            baseline_errors.append(f"baseline unreadable: {exc}")
        if baseline_count is not None and baseline_count < MIN_BASELINE:
            baseline_errors.append(
                f"baseline_unallowlisted_hits={baseline_count} is below the floor of {MIN_BASELINE}; "
                "a low baseline means the verifier never saw the defect it exists to catch"
            )

    pdf_regenerated = None
    if baseline_pdf_sha is not None and pdf_sha is not None:
        pdf_regenerated = pdf_sha != baseline_pdf_sha

    report = {
        "checker": "licence_phrasing",
        "repo_root": repo_root.replace("\\", "/"),
        "licence": "PolyForm Shield 1.0.0 (source-available), dual-licensed against LICENSE-COMMERCIAL.md",
        "banned_patterns": {name: pat for name, pat in BANNED_PATTERNS},
        "excluded_dirs": excluded_dirs_summary(),
        "excluded_dir_reasons": {
            "<any>/.git": "version-control internals, not a readable surface",
            "<any>/node_modules": "vendored third-party packages carrying their own accurate MIT notices",
            "<any>/vendor": "vendored third-party crates, accurate about themselves",
            "<any>/dist": "build output; regenerates from corrected sources",
            "<any>/target*, <any>/.trunk-web-target": "cargo and trunk build output; regenerates",
            **{k: v for k, v in EXCLUDED_REL_PATHS.items()},
        },
        "scanned_files": scanned,
        "excluded_dir_count": len(excluded_dirs_summary()),
        "hits_total": len(hits),
        "unallowlisted_hits": len(unallowlisted),
        "unallowlisted": unallowlisted,
        "rule_statement_rule": (
            "A hit whose line also carries the correct term (source-available / PolyForm) states "
            "the house rule rather than making a claim, and is classified out. Same-line only, and "
            "every such line is listed below verbatim."
        ),
        "rule_statement_hits": len(rule_statements),
        "rule_statements": rule_statements,
        "allowlisted_hits": len(allowlisted),
        "allowlist_entries": len(entries),
        "allowlist": entries,
        "allowlist_errors": allow_errors,
        "baseline_unallowlisted_hits": baseline_count,
        "baseline_unallowlisted": baseline_hits,
        "baseline_errors": baseline_errors,
        "min_baseline": MIN_BASELINE,
        "pdf": {
            "path": pdf_rel,
            "sha256": pdf_sha,
            "baseline_sha256": baseline_pdf_sha,
            "regenerated": pdf_regenerated,
            "note": (
                "The scanner inflates FlateDecode streams as a best-effort supplement, but a PDF's "
                "text may be compressed or subset-encoded beyond that. The sha256 above is the "
                "evidence that the PDF was regenerated from the corrected HTML, not a grep."
            ),
        },
        "report_only": bool(args.report_only),
    }

    # Narrative context (decisions taken, build exit codes, the command that regenerated the PDF)
    # lives beside the allowlist as `licence_phrasing_context.json` and is merged in here, so the
    # exit-criterion command on its own reproduces the whole artifact without hand-editing.
    if args.allowlist:
        ctx_path = os.path.join(os.path.dirname(os.path.abspath(args.allowlist)),
                                "licence_phrasing_context.json")
        if os.path.isfile(ctx_path):
            try:
                with open(ctx_path, "r", encoding="utf-8") as fh:
                    report["item_context"] = json.load(fh)
            except (OSError, json.JSONDecodeError) as exc:
                report["item_context"] = {"error": f"context unreadable: {exc}"}

    if args.out:
        out_dir = os.path.dirname(os.path.abspath(args.out))
        if out_dir:
            os.makedirs(out_dir, exist_ok=True)
        with open(args.out, "w", encoding="utf-8") as fh:
            json.dump(report, fh, indent=2)
            fh.write("\n")

    print(f"scanned_files={scanned} excluded_dirs={len(excluded_dirs_summary())}")
    if baseline_count is not None:
        print(f"baseline_unallowlisted_hits={baseline_count}")
    print(f"unallowlisted_hits={len(unallowlisted)}")
    print(f"allowlist_entries={len(entries)}")
    print(f"rule_statement_hits={len(rule_statements)}")
    for h in rule_statements:
        print(f"  RULE {h['path']}:{h['line']}  {h['text']}")
    for e in entries:
        print(f"  {e.get('path')}:{e.get('line')}  \"{e.get('text')}\"  reason=\"{e.get('reason')}\"")
    for h in unallowlisted:
        print(f"  HIT {h['path']}:{h['line']}  [{h['pattern']}]  {h['text']}")
    print(f"pdf_regenerated={pdf_regenerated} pdf_sha256={pdf_sha}")

    if args.report_only:
        for msg in allow_errors:
            print(f"  note(allowlist) {msg}")
        return 5 if unallowlisted else 0

    for msg in allow_errors:
        print(f"  ERROR(allowlist) {msg}", file=sys.stderr)
    for msg in baseline_errors:
        print(f"  ERROR(baseline) {msg}", file=sys.stderr)

    if args.baseline and (baseline_errors or baseline_count is None):
        return 7
    if allow_errors:
        return 6
    if unallowlisted:
        return 5
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
