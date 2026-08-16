#!/usr/bin/env python3
"""Gate for the G1.30 claim ledger.

Refuses to exit 0 while any extracted claim is unclassified, while any record in a
class named by --require-repro-for lacks a reproduction command or artifact path,
while any record lacks a source line, or while the ledger holds fewer than
--min-claims records.

Exit codes:
  0  every check passed
  2  at least one check failed
  3  the ledger could not be read or parsed
"""

import argparse
import collections
import json
import sys

VALID_CLASSES = ("MEASURED", "TARGET", "CONFIG_DEFAULT", "UNSUPPORTED")

REQUIRED_KEYS = (
    "id",
    "claim_text",
    "source_path",
    "source_line",
    "class",
    "number",
    "unit",
    "reproduction_command",
    "reproduction_artifact_path",
    "config_field_path",
    "audit_reference",
    "licence_conflict",
)

REQUIRED_TOP_LEVEL = ("generated_at", "corpus", "records", "unreadable_sources")


def parse_args(argv):
    p = argparse.ArgumentParser(description="Validate a G1.30 claim ledger.")
    p.add_argument("--ledger", required=True, help="Path to claim_ledger.json.")
    p.add_argument("--min-claims", type=int, default=1,
                   help="Minimum number of claim records the ledger must hold.")
    p.add_argument("--require-repro-for", action="append", default=[],
                   help="Class whose records must carry a reproduction command and "
                        "artifact path. Repeatable, or comma-separated.")
    p.add_argument("--print-summary", action="store_true",
                   help="Print the counter block on stdout.")
    return p.parse_args(argv)


def load(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except OSError as exc:
        print(f"ERROR unreadable ledger {path}: {exc}", file=sys.stderr)
        raise SystemExit(3)
    except json.JSONDecodeError as exc:
        print(f"ERROR malformed JSON in {path}: {exc}", file=sys.stderr)
        raise SystemExit(3)


def is_nonempty_str(value):
    return isinstance(value, str) and value.strip() != ""


def main(argv):
    args = parse_args(argv)

    repro_classes = set()
    for entry in args.require_repro_for:
        repro_classes.update(part.strip() for part in entry.split(",") if part.strip())

    ledger = load(args.ledger)
    if not isinstance(ledger, dict):
        print("ERROR ledger root is not an object", file=sys.stderr)
        return 3

    violations = []

    for key in REQUIRED_TOP_LEVEL:
        if key not in ledger:
            violations.append(f"ledger is missing the top-level key '{key}'")

    records = ledger.get("records")
    if not isinstance(records, list):
        print("ERROR ledger 'records' is not a list", file=sys.stderr)
        return 3

    unknown_repro = repro_classes - set(VALID_CLASSES)
    if unknown_repro:
        violations.append(
            "--require-repro-for names classes outside the enum: "
            + ", ".join(sorted(unknown_repro))
        )

    by_class = collections.Counter()
    unclassified = 0
    measured_missing_repro = 0
    records_missing_source_line = 0
    seen_ids = set()

    for index, record in enumerate(records):
        label = f"record[{index}]"
        if not isinstance(record, dict):
            violations.append(f"{label} is not an object")
            unclassified += 1
            records_missing_source_line += 1
            continue

        label = f"record {record.get('id', f'[{index}]')}"

        for key in REQUIRED_KEYS:
            if key not in record:
                violations.append(f"{label} is missing the key '{key}'")

        rid = record.get("id")
        if not is_nonempty_str(rid):
            violations.append(f"{label} has no id")
        elif rid in seen_ids:
            violations.append(f"{label} duplicates an earlier id")
        else:
            seen_ids.add(rid)

        if not is_nonempty_str(record.get("claim_text")):
            violations.append(f"{label} has an empty claim_text")
        elif len(record["claim_text"]) > 200:
            violations.append(f"{label} claim_text exceeds 200 characters")

        if not is_nonempty_str(record.get("source_path")):
            violations.append(f"{label} has no source_path")

        line = record.get("source_line")
        if not isinstance(line, int) or isinstance(line, bool) or line < 1:
            records_missing_source_line += 1
            violations.append(f"{label} has no usable source_line")

        cls = record.get("class")
        if cls not in VALID_CLASSES:
            unclassified += 1
            violations.append(f"{label} carries class {cls!r}, which is outside the enum")
        else:
            by_class[cls] += 1
            if cls in repro_classes:
                missing = []
                if not is_nonempty_str(record.get("reproduction_command")):
                    missing.append("reproduction_command")
                if not is_nonempty_str(record.get("reproduction_artifact_path")):
                    missing.append("reproduction_artifact_path")
                if missing:
                    measured_missing_repro += 1
                    violations.append(
                        f"{label} is class {cls} but lacks " + " and ".join(missing)
                    )
            if cls == "CONFIG_DEFAULT" and not is_nonempty_str(record.get("config_field_path")):
                violations.append(f"{label} is class CONFIG_DEFAULT but lacks config_field_path")

        if not isinstance(record.get("licence_conflict"), bool):
            violations.append(f"{label} has a non-boolean licence_conflict")

    total_claims = len(records)
    if total_claims < args.min_claims:
        violations.append(
            f"ledger holds {total_claims} records, below the floor of {args.min_claims}"
        )

    unreadable = ledger.get("unreadable_sources")
    unreadable_count = len(unreadable) if isinstance(unreadable, list) else 0
    if isinstance(unreadable, list):
        for index, source in enumerate(unreadable):
            if not isinstance(source, dict):
                violations.append(f"unreadable_sources[{index}] is not an object")
                continue
            for key in ("path", "byte_length", "nul_bytes"):
                if key not in source:
                    violations.append(f"unreadable_sources[{index}] is missing '{key}'")
    else:
        violations.append("unreadable_sources is not a list")

    if args.print_summary:
        print(f"total_claims={total_claims}")
        print("by_class " + " ".join(f"{c}={by_class.get(c, 0)}" for c in VALID_CLASSES))
        print(f"unclassified={unclassified}")
        print(f"measured_missing_repro={measured_missing_repro}")
        print(f"records_missing_source_line={records_missing_source_line}")
        print(f"unreadable_sources={unreadable_count}")

    if violations:
        print(f"FAIL {len(violations)} violation(s):", file=sys.stderr)
        for violation in violations:
            print(f"  - {violation}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
