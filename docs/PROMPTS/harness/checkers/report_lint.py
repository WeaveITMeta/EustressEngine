#!/usr/bin/env python3
"""Lint a validation report against docs/validation/PROOF_STANDARD.md.

Exit 0: no violations. Exit 2: violations. Exit 1: usage or standard error.
Offline only. Every required field is read from the standard's table; each row names a
`check:<id>` that must exist in CHECKS below. All fields are checked; nothing short-circuits.
"""
import argparse
import hashlib
import os
import re
import sys

LABELS = ("MEASURED", "TARGET", "CONFIG_DEFAULT")
ROW = re.compile(r"^\|\s*`([a-z_]+(?:\.[a-z_]+)+)`\s*\|.*\|\s*`check:([a-z_]+)`\s*\|")
BANNED = ("open" + " source", "game" + " engine")


def parse_standard(path):
    fields = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = ROW.match(line.strip())
            if m:
                fields.append((m.group(1), m.group(2)))
    return fields


def parse_report(path):
    text = open(path, encoding="utf-8").read()
    m = re.search(r"^```proof[ \t]*\r?\n(.*?)^```", text, re.S | re.M)
    values = {}
    if m:
        for line in m.group(1).splitlines():
            k, sep, v = line.partition(":")
            if sep and re.fullmatch(r"[a-z_]+(\.[a-z_]+)+", k.strip()):
                values[k.strip()] = v.strip()
    return text, values, bool(m)


def find_root(report):
    d = os.path.abspath(os.path.dirname(report))
    while True:
        if os.path.isdir(os.path.join(d, "eustress")) and os.path.isdir(os.path.join(d, "docs")):
            return d
        p = os.path.dirname(d)
        if p == d:
            return os.getcwd()
        d = p


# Each check returns None when satisfied, else a short reason.
def c_nonempty(v, ctx):
    return None if v else "empty"


def c_inline_label(v, ctx):
    tags = [t for t in LABELS if "[" + t + "]" in v]
    return None if len(tags) == 1 else "needs exactly one inline [MEASURED|TARGET|CONFIG_DEFAULT] tag"


def c_toolchain(v, ctx):
    if not re.fullmatch(r"\d+\.\d+\.\d+", v):
        return "not a x.y.z version"
    tc = os.path.join(ctx["root"], "eustress", "rust-toolchain.toml")
    if os.path.isfile(tc):
        m = re.search(r'^\s*channel\s*=\s*"([^"]+)"', open(tc, encoding="utf-8").read(), re.M)
        if m and m.group(1) != v:
            return "differs from rust-toolchain.toml channel " + m.group(1)
    return None


def c_os(v, ctx):
    return None if v and re.search(r"\d", v) else "needs an OS name and a version digit"


def c_features(v, ctx):
    if not v:
        return "empty"
    if v == "none":
        return None
    return None if re.fullmatch(r"[A-Za-z0-9_\-]+(\s*,\s*[A-Za-z0-9_\-]+)*", v) else "not a comma list or none"


def c_command(v, ctx):
    if not v:
        return "empty"
    if not v.startswith("cd eustress &&"):
        return "must start with 'cd eustress &&'"
    if "cargo check" in v:
        return "cargo check is rejected"
    if not re.search(r"cargo\s+(run|test)\b", v):
        return "must use cargo run or cargo test"
    feats = ctx["values"].get("environment.cargo_features", "")
    if feats and feats != "none":
        m = re.search(r"--features[ =]+(\S+)", v)
        if not m:
            return "features declared but command has no --features"
        used = {x.strip() for x in m.group(1).split(",")}
        missing = [f.strip() for f in feats.split(",") if f.strip() not in used]
        if missing:
            return "command omits features: " + ",".join(missing)
    return None


def c_path_exists(v, ctx):
    return None if v and os.path.isfile(os.path.join(ctx["root"], v)) else "file not found: " + v


def c_label_enum(v, ctx):
    if v not in LABELS:
        return "not one of " + "|".join(LABELS)
    tag = re.search(r"\[(MEASURED|TARGET|CONFIG_DEFAULT)\]", ctx["values"].get("claim.text", ""))
    if tag and tag.group(1) != v:
        return "differs from claim.text tag " + tag.group(1)
    return None


def c_tolerance(v, ctx):
    ok = v == "exact" or re.fullmatch(r"abs\s+\d+(\.\d+)?\s+\S+", v) or re.fullmatch(r"rel\s+\d+(\.\d+)?%", v)
    return None if ok else "must be exact, 'abs <n> <unit>' or 'rel <n>%'"


def c_uncertainty(v, ctx):
    if len(v.split()) < 3:
        return "needs at least three words"
    return "not a source" if v.lower() in ("none", "unknown", "n/a") else None


def c_content_hash(v, ctx):
    m = re.fullmatch(r"sha256:([0-9a-f]{64})", v)
    if not m:
        return "must be sha256: plus 64 lowercase hex"
    want = hashlib.sha256(ctx["values"].get("result.value", "").strip().encode("utf-8")).hexdigest()
    return None if m.group(1) == want else "does not match sha256 of result.value"


CHECKS = {
    "nonempty": c_nonempty, "inline_label": c_inline_label, "toolchain": c_toolchain,
    "os": c_os, "features": c_features, "command": c_command, "path_exists": c_path_exists,
    "label_enum": c_label_enum, "tolerance": c_tolerance, "uncertainty": c_uncertainty,
    "content_hash": c_content_hash,
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--standard", required=True)
    ap.add_argument("--report")
    ap.add_argument("--list-required-fields", action="store_true")
    a = ap.parse_args()
    fields = parse_standard(a.standard)
    if not fields:
        print("error: no required fields found in standard", file=sys.stderr)
        return 1
    unknown = [c for _, c in fields if c not in CHECKS]
    if unknown:
        print("error: standard names checks the linter lacks: " + ",".join(unknown), file=sys.stderr)
        return 1
    if a.list_required_fields:
        for name, _ in fields:
            print(name)
        return 0
    if not a.report:
        print("error: --report required", file=sys.stderr)
        return 1
    text, values, found = parse_report(a.report)
    ctx = {"values": values, "root": find_root(a.report)}
    out = []
    if not found:
        out.append("  missing: proof block")
    for name, chk in fields:
        if name not in values:
            out.append("  missing: " + name)
            continue
        why = CHECKS[chk](values[name], ctx)
        if why:
            out.append("  invalid: %s (%s)" % (name, why))
    low = text.lower()
    for b in BANNED:
        if b in low:
            out.append("  banned phrase: " + b)
    print("required_fields=%d violations=%d" % (len(fields), len(out)))
    for line in out:
        print(line)
    return 2 if out else 0


if __name__ == "__main__":
    sys.exit(main())
