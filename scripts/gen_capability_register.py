# -*- coding: utf-8 -*-
"""Capability register for Eustress mode tool ids (gauntlet item G6.30).

Every tool id declared in a mode manifest (eustress/crates/engine/modes/*.toml)
is classified from source text, never by hand:

    SHIPPED      a live dispatch site for the exact id exists in Rust source
    DECLARED     the id has manifest and metadata entries and no dispatch site
    UNDECIDABLE  evidence exists but cannot be resolved mechanically; the entry
                 carries the reason

The register exists so every demo shows working software and every declared
tool has a clear, checkable path to shipped. Nothing in this file wires a tool.

Modes (run from the repository root):

    # generate the register (JSON, plus a Markdown rendering next to it)
    python scripts/gen_capability_register.py \
        --manifests eustress/crates/engine/modes --sources eustress/crates \
        --out docs/PROMPTS/artifacts/G6.30/capability_register.json

    # byte-compare two registers, ignoring named top-level fields
    python scripts/gen_capability_register.py --compare A.json B.json \
        --ignore-fields generated_at

    # ask what the register says about one id (exit 0 if it matches)
    python scripts/gen_capability_register.py --assert-class data:import=SHIPPED

    # the pitch gate: exit 2 if any demo step is not SHIPPED, exit 0 otherwise
    python scripts/gen_capability_register.py --gate \
        --register docs/PROMPTS/artifacts/G6.30/capability_register.json \
        --demo-script <script.json>

Exit codes: 0 ok, 1 usage or I/O error or assertion failure, 2 gate refusal.

Demo script format (JSON): {"title": "...", "steps": [{"step": 1, "tool": "data:import"}, ...]}
A step may also be a bare string. A step may name a manifest tool id or a
registered MCP tool name; both are looked up in the register. Anything the
register does not contain is refused (fail closed).

Classification rule, in full, is embedded in the register as
`classification_rule` and printed by --print-rule.
"""
import argparse
import bisect
import datetime
import glob
import hashlib
import json
import os
import re
import subprocess
import sys

try:
    import tomllib
except ImportError:  # Python < 3.11
    import tomli as tomllib

RULE_NAME = "dispatch_literal_or_tooldefinition_name"

ID_RE = re.compile(r"^[a-z0-9]+:[a-z0-9_]+$")

CLASSIFICATION_RULE = {
    "name": RULE_NAME,
    "summary": (
        "An id classifies SHIPPED if and only if its exact string literal appears in a dispatch position "
        "in live Rust source (outside tool_metadata.rs, outside the mode manifests, outside test code and "
        "comments), or it is reached through a prefix dispatch block that names it, or it equals a "
        "registered MCP ToolDefinition name. Presence in tool_metadata.rs is never evidence."
    ),
    "scanned_files": (
        "Every *.rs file under --sources except tool_metadata.rs, except any path with a tests, benches, "
        "examples, target or node_modules component, and with every #[cfg(test)] or #[test] item and every "
        "comment removed before matching."
    ),
    "shipped_evidence": [
        "match_arm: the literal is a match arm pattern, alone or inside an alternation (\"id\" => ..., "
        "\"a\" | \"id\" => ..., \"id\" if guard => ...), and the arm body is not a display label and does "
        "not say it is unimplemented.",
        "eq_compare: the literal is compared with == (action == \"id\" or \"id\" == action).",
        "prefix_dispatch: the file gates the id's prefix with `if let Some(V) = X.strip_prefix(\"P:\") { ... }` "
        "and, inside that brace block, the name part of the id is compared with V (V == \"name\") or is an "
        "arm of `match V { ... }`.",
        "mcp_tool_name: the id is exactly equal to a registered ToolDefinition name (name: \"...\" in "
        "crates/tools/src and crates/mcp-server/src). Manifest ids contain a colon and MCP names do not, so "
        "no heuristic mapping between the two namespaces is made.",
    ],
    "undecidable_reasons": [
        "stub_arm: a match arm exists but its body says the action is not implemented.",
        "label_arm: the only match arms are display labels (an arm whose body is a bare string or Some(\"...\")).",
        "matches_macro: the only reference is inside matches!(...), which may be a predicate and not a handler.",
        "slint_dispatch: the id is compared in a .slint file (a UI-side handler), which this Rust-source rule "
        "does not count; it needs a human decision on whether the Slint handler is a feature.",
        "prefix_gate_unresolved: the id's prefix has a strip_prefix gate whose block could not be delimited, "
        "so name dispatch inside it cannot be ruled in or out.",
    ],
    "declared_evidence": (
        "No dispatch site found. Literal references that are not dispatch (lists, call arguments, producers "
        "such as MenuAction(\"id\".into()), labels) are listed under `references` so a reader can check them."
    ),
    "known_limits": [
        "Arm presence proves a handler is reachable from code, not that its output is complete or correct.",
        "A name literal inside a prefix block is matched by comparison with the gate's binding or an arm of "
        "match on that binding; an unrelated arm with the same name inside the same block would be counted.",
        "Ids dispatched only through data (a table looked up at runtime) classify DECLARED.",
        "metadata_wired_flag is the generated tool_metadata.rs flag, recorded for cross-checking only.",
    ],
}

SKIP_DIR_PARTS = {"tests", "benches", "examples", "target", "node_modules", ".git"}
STUB_RE = re.compile(
    r"not\s+(?:yet\s+)?(?:implemented|wired|available|built|supported)|coming\s+soon|\btodo\b|unimplemented!|"
    r"\bstub\b|placeholder|acknowledged\s+until|not\s+wired",
    re.IGNORECASE,
)
ARM_RE = re.compile(r"\s*(?:\|\s*\"\x01*\"\s*)*(?:if\b[^{};]*?)?=>")
TEST_ATTR_RE = re.compile(r"#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\]")


# ───────────────────────────── helpers ──────────────────────────────────────

def norm(p):
    return p.replace("\\", "/")


def read_text(path):
    with open(path, "rb") as f:
        raw = f.read()
    return raw.decode("utf-8", errors="replace").replace("\r\n", "\n").replace("\r", "\n")


def git_commit(root):
    try:
        r = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, timeout=30)
        if r.returncode == 0:
            return r.stdout.strip()
    except Exception:
        pass
    return "unknown"


def tokenize_rust(text):
    """Return (skel, strings).

    skel is text of the same length with comments blanked and string/char
    literal interiors replaced by \\x01 (quote marks kept) so braces inside
    strings never confuse brace matching. strings is a list of
    (start, end, value) for each string literal; start is the opening quote
    (or the r / b prefix start), end is one past the closing quote.
    """
    n = len(text)
    out = list(text)
    strings = []
    i = 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            if j < 0:
                j = n
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            j = i + 2
            while j < n and depth:
                if text[j] == "/" and j + 1 < n and text[j + 1] == "*":
                    depth += 1
                    j += 2
                elif text[j] == "*" and j + 1 < n and text[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue
        # raw strings: r"..", r#".."#, br#".."#
        if c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i
            if text[j] == "b" and j + 1 < n and text[j + 1] == "r":
                j += 1
            if text[j] == "r":
                k = j + 1
                hashes = 0
                while k < n and text[k] == "#":
                    hashes += 1
                    k += 1
                if k < n and text[k] == '"':
                    closer = '"' + "#" * hashes
                    e = text.find(closer, k + 1)
                    if e < 0:
                        e = n - len(closer)
                    end = e + len(closer)
                    strings.append((i, end, text[k + 1:e]))
                    for m in range(k + 1, e):
                        if out[m] != "\n":
                            out[m] = "\x01"
                    i = end
                    continue
        if c == '"' or (c == "b" and i + 1 < n and text[i + 1] == '"' and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_"))):
            start = i
            j = i + (2 if c == "b" else 1)
            body_start = j
            while j < n and text[j] != '"':
                if text[j] == "\\":
                    j += 2
                else:
                    j += 1
            end = min(j + 1, n)
            strings.append((start, end, text[body_start:j]))
            for m in range(body_start, min(j, n)):
                if out[m] != "\n":
                    out[m] = "\x01"
            i = end
            continue
        if c == "'":
            # char literal or lifetime
            if i + 1 < n and text[i + 1] == "\\":
                j = text.find("'", i + 2)
                if j > 0 and j - i < 12:
                    for m in range(i + 1, j):
                        out[m] = "\x01"
                    i = j + 1
                    continue
            elif i + 2 < n and text[i + 2] == "'" and text[i + 1] != "'":
                out[i + 1] = "\x01"
                i += 3
                continue
        i += 1
    return "".join(out), strings


def brace_end(skel, open_idx):
    """Index one past the brace that closes skel[open_idx] == '{'; -1 if none."""
    depth = 0
    n = len(skel)
    i = open_idx
    while i < n:
        ch = skel[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return -1


def blank_tests(skel):
    """Blank #[cfg(test)] / #[test] items. Returns (new_skel, dead_ranges)."""
    dead = []
    chars = list(skel)
    pos = 0
    while True:
        m = TEST_ATTR_RE.search(skel, pos)
        if not m:
            break
        i = m.end()
        brace = skel.find("{", i)
        semi = skel.find(";", i)
        if brace < 0 and semi < 0:
            break
        if semi >= 0 and (brace < 0 or semi < brace):
            end = semi + 1
        else:
            end = brace_end(skel, brace)
            if end < 0:
                end = len(skel)
        dead.append((m.start(), end))
        for k in range(m.start(), end):
            if chars[k] != "\n":
                chars[k] = " "
        pos = end
    return "".join(chars), dead


def in_ranges(ranges, idx):
    for a, b in ranges:
        if a <= idx < b:
            return True
    return False


def line_of(offsets, idx):
    return bisect.bisect_right(offsets, idx)  # 1-based


# ───────────────────────────── manifests ────────────────────────────────────

def load_manifests(mdir):
    """Return (manifest_summaries, placements, raw_text_ids, malformed)."""
    files = sorted(glob.glob(os.path.join(mdir, "*.toml")))
    summaries = []
    placements = []  # dicts
    malformed = []
    raw_ids = set()
    for f in files:
        text = read_text(f)
        d = tomllib.loads(text)
        mode = d.get("mode", {})
        base = os.path.basename(f)
        count = [0]

        def add(tid, discipline, tab, section):
            if not isinstance(tid, str) or not ID_RE.match(tid):
                malformed.append({"manifest": base, "value": tid, "discipline": discipline, "tab": tab, "section": section})
                return
            count[0] += 1
            placements.append({"id": tid, "manifest": base, "mode": mode.get("id", base[:-5]),
                               "discipline": discipline, "tab": tab, "section": section})

        for ts in d.get("toolsets", []):
            for t in ts.get("tools", []):
                add(t, "*", "toolset:" + str(ts.get("id", "")), str(ts.get("name", "")))
        for tab in d.get("tabs", []):
            for sec in tab.get("sections", []):
                for t in sec.get("tools", []):
                    add(t, "*", str(tab.get("id", "")), str(sec.get("name", "")))
        for tab in d.get("submode_tabs", []):
            for sec in tab.get("sections", []):
                for t in sec.get("tools", []):
                    add(t, str(tab.get("submode", "")), str(tab.get("id", "")), str(sec.get("name", "")))
        # Independent coverage check: raw text extraction of every tools = [...] array.
        for m in re.finditer(r"(?m)^\s*tools\s*=\s*\[(.*?)\]", text, re.S):
            for s in re.findall(r'"([^"]*)"', re.sub(r"#[^\n]*", "", m.group(1))):
                if ID_RE.match(s):
                    raw_ids.add(s)
        summaries.append({"file": base, "mode": mode.get("id", base[:-5]), "placements": count[0]})
    return summaries, placements, raw_ids, malformed


# ───────────────────────────── source scan ──────────────────────────────────

def collect_rs_files(src_root):
    out = []
    for root, dirs, files in os.walk(src_root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIR_PARTS)
        for fn in sorted(files):
            if fn.endswith(".rs") and fn != "tool_metadata.rs":
                out.append(os.path.join(root, fn))
    return out


def collect_slint_files(src_root):
    out = []
    for root, dirs, files in os.walk(src_root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIR_PARTS)
        for fn in sorted(files):
            if fn.endswith(".slint"):
                out.append(os.path.join(root, fn))
    return out


def arm_body(skel, text, arrow_end):
    """Original-text body of the arm that starts after '=>' at arrow_end."""
    n = len(skel)
    i = arrow_end
    while i < n and skel[i] in " \t\n":
        i += 1
    if i < n and skel[i] == "{":
        e = brace_end(skel, i)
        return text[i:e if e > 0 else min(n, i + 2000)]
    depth = 0
    j = i
    while j < n:
        ch = skel[j]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            break
        elif ch == "\n" and depth == 0 and j - i > 400:
            break
        j += 1
    return text[i:j]


LABEL_BODY_RE = re.compile(r'^\s*(?:Some\(\s*)?"[^"]*"\s*\)?\s*$', re.S)


def scan_sources(src_root, ids, prefixes, rel_root):
    """Scan Rust source. Returns (per_id_refs, gates, files_scanned, fingerprint_parts, mcp_names)."""
    prefix_re = re.compile(r'"(?:' + "|".join(sorted(re.escape(p) for p in prefixes)) + r"):")
    refs = {}      # id -> list of ref dicts
    gates = {}     # prefix -> list of gate dicts
    fp = []
    scanned = 0
    for path in collect_rs_files(src_root):
        text = read_text(path)
        rel = norm(os.path.relpath(path, rel_root))
        if not prefix_re.search(text):
            continue
        scanned += 1
        fp.append((rel, hashlib.sha256(text.encode("utf-8")).hexdigest()))
        skel, strings = tokenize_rust(text)
        skel, dead = blank_tests(skel)
        offsets = [m.end() for m in re.finditer("\n", text)]
        # Map newline positions: offsets[k] is the index just after the k-th newline.
        live = [(s, e, v) for (s, e, v) in strings if not in_ranges(dead, s)]
        for s, e, v in live:
            if v in ids:
                ln = line_of(offsets, s) + 1
                before = skel[max(0, s - 60):s]
                after = skel[e:e + 400]
                kind = "other"
                detail = ""
                if re.search(r"==\s*$", before) or re.match(r"\s*==", after):
                    kind = "eq_compare"
                elif re.search(r"!=\s*$", before) or re.match(r"\s*!=", after):
                    kind = "neq_compare"
                elif ARM_RE.match(after):
                    am = ARM_RE.match(after)
                    body = arm_body(skel, text, e + am.end())
                    if LABEL_BODY_RE.match(body.strip().rstrip(",")):
                        kind = "label_arm"
                        detail = body.strip()[:60]
                    else:
                        sm = STUB_RE.search(body)
                        if sm:
                            kind = "stub_arm"
                            detail = sm.group(0)
                        else:
                            kind = "match_arm"
                elif re.search(r"matches!\s*\([^;{}]*$", before):
                    kind = "matches_macro"
                elif re.search(r"(?:MenuAction|push|from|into)\s*\(\s*$", before):
                    kind = "producer"
                elif re.search(r"[\[,&(]\s*$", before):
                    kind = "list_or_argument"
                refs.setdefault(v, []).append({"path": rel, "line": ln, "kind": kind, "detail": detail})
        # prefix gates: strip_prefix("P:") with a binding and a block
        for m in re.finditer(r'strip_prefix\s*\(\s*"\x01*"\s*\)', skel):
            # find the literal for this call
            lit = None
            for s, e, v in live:
                if s >= m.start() and e <= m.end() and v.endswith(":") and v[:-1] in prefixes:
                    lit = v
                    break
            if lit is None:
                continue
            p = lit[:-1]
            head = skel[max(0, m.start() - 160):m.start()]
            bm = re.search(r"(?:let\s+Some\s*\(\s*(\w+)\s*\)|let\s+(\w+)\s*(?::[^=]*)?)\s*=\s*[^;{]*$", head)
            var = None
            if bm:
                var = bm.group(1) or bm.group(2)
            ob = skel.find("{", m.end())
            semi = skel.find(";", m.end())
            gl = line_of(offsets, m.start()) + 1
            gate = {"path": rel, "line": gl, "var": var, "resolved": False, "names": {}}
            if var and bm and bm.group(1) and ob >= 0 and (semi < 0 or ob < semi):
                end = brace_end(skel, ob)
                if end > 0:
                    gate["resolved"] = True
                    block = skel[ob:end]
                    # names: eq compare with var
                    for em in re.finditer(r"\b" + re.escape(var) + r"\s*==\s*\"(\x01*)\"|\"(\x01*)\"\s*==\s*\b" + re.escape(var) + r"\b", block):
                        a = ob + em.start()
                        for s, e, v in live:
                            if s >= a and e <= ob + em.end() and "\x01" not in v:
                                gate["names"].setdefault(v, norm(rel) + ":" + str(line_of(offsets, s) + 1) + " (eq on %s)" % var)
                    # names: arms of `match var {`
                    for mm in re.finditer(r"\bmatch\s+" + re.escape(var) + r"\s*\{", block):
                        mo = ob + mm.end() - 1
                        me = brace_end(skel, mo)
                        if me < 0:
                            continue
                        for s, e, v in live:
                            if mo < s and e < me and ARM_RE.match(skel[e:e + 400]):
                                gate["names"].setdefault(v, norm(rel) + ":" + str(line_of(offsets, s) + 1) + " (arm of match %s)" % var)
            gates.setdefault(p, []).append(gate)
    return refs, gates, scanned, fp


def scan_slint(src_root, ids, rel_root):
    found = {}
    for path in collect_slint_files(src_root):
        text = read_text(path)
        rel = norm(os.path.relpath(path, rel_root))
        for ln, line in enumerate(text.split("\n"), 1):
            code = line.split("//", 1)[0]
            for m in re.finditer(r'==\s*"([a-z0-9]+:[a-z0-9_]+)"', code):
                if m.group(1) in ids:
                    found.setdefault(m.group(1), []).append(rel + ":" + str(ln))
    return found


def collect_mcp_names(src_root, rel_root):
    names = {}
    roots = [os.path.join(src_root, "tools", "src"), os.path.join(src_root, "mcp-server", "src")]
    for r in roots:
        if not os.path.isdir(r):
            continue
        for path in collect_rs_files(r):
            text = read_text(path)
            rel = norm(os.path.relpath(path, rel_root))
            for m in re.finditer(r'(?:\bname\s*:\s*|"name"\s*:\s*)"([A-Za-z0-9_:.\-]+)"', text):
                ln = text.count("\n", 0, m.start()) + 1
                names.setdefault(m.group(1), rel + ":" + str(ln))
    return names


def parse_metadata_flags(src_root):
    """tool_metadata.rs: 'id' => (label, tip, icon, wired). Cross-check only."""
    flags = {}
    for path in glob.glob(os.path.join(src_root, "**", "tool_metadata.rs"), recursive=True):
        text = read_text(path)
        for m in re.finditer(r'"([a-z0-9]+:[a-z0-9_]+)"\s*=>\s*\(\s*"[^"]*"\s*,\s*"[^"]*"\s*,\s*"[^"]*"\s*,\s*(true|false)\s*\)', text):
            flags[m.group(1)] = (m.group(2) == "true")
    return flags


# ───────────────────────────── classification ───────────────────────────────

def classify(tid, refs, gates, slint, mcp_names):
    r = refs.get(tid, [])
    ev = []
    shipped = []
    for x in r:
        if x["kind"] in ("match_arm", "eq_compare"):
            shipped.append("%s:%d (%s)" % (x["path"], x["line"], x["kind"]))
    prefix, name = tid.split(":", 1)
    for g in gates.get(prefix, []):
        if g["resolved"] and name in g["names"]:
            shipped.append("%s (prefix_dispatch via %s:%d)" % (g["names"][name], g["path"], g["line"]))
    if tid in mcp_names:
        shipped.append("%s (mcp_tool_name)" % mcp_names[tid])
    refs_out = ["%s:%d %s" % (x["path"], x["line"], x["kind"]) for x in r if x["kind"] not in ("match_arm", "eq_compare")]
    if shipped:
        return "SHIPPED", sorted(set(shipped))[:4], sorted(set(refs_out))
    reasons = []
    for x in r:
        if x["kind"] == "stub_arm":
            reasons.append("stub_arm at %s:%d (body says \"%s\")" % (x["path"], x["line"], x["detail"]))
        elif x["kind"] == "label_arm":
            reasons.append("label_arm only at %s:%d" % (x["path"], x["line"]))
        elif x["kind"] == "matches_macro":
            reasons.append("matches_macro only at %s:%d" % (x["path"], x["line"]))
    if tid in slint:
        reasons.append("slint_dispatch at %s" % ", ".join(sorted(set(slint[tid]))[:2]))
    for g in gates.get(prefix, []):
        if not g["resolved"]:
            reasons.append("prefix_gate_unresolved for '%s:' at %s:%d" % (prefix, g["path"], g["line"]))
    if reasons:
        return "UNDECIDABLE", sorted(set(reasons)), sorted(set(refs_out))
    return "DECLARED", ["no dispatch site found in Rust source"], sorted(set(refs_out))


def build_register(manifests_dir, sources_dir, repo_root):
    summaries, placements, raw_ids, malformed = load_manifests(manifests_dir)
    manifest_ids = sorted({p["id"] for p in placements})
    ids = set(manifest_ids)
    prefixes = sorted({i.split(":", 1)[0] for i in ids})
    refs, gates, scanned, fp = scan_sources(sources_dir, ids, set(prefixes), repo_root)
    slint = scan_slint(sources_dir, ids, repo_root)
    mcp = collect_mcp_names(sources_dir, repo_root)
    flags = parse_metadata_flags(sources_dir)
    first = {}
    count = {}
    modes = {}
    for p in placements:
        key = p["id"]
        count[key] = count.get(key, 0) + 1
        modes.setdefault(key, set()).add(p["mode"])
        if key not in first or (p["manifest"], p["discipline"], p["tab"], p["section"]) < (
                first[key]["manifest"], first[key]["discipline"], first[key]["tab"], first[key]["section"]):
            first[key] = p
    entries = []
    totals = {"SHIPPED": 0, "DECLARED": 0, "UNDECIDABLE": 0}
    disagree = []
    for tid in manifest_ids:
        cls, evidence, references = classify(tid, refs, gates, slint, mcp)
        totals[cls] += 1
        f = first[tid]
        e = {"id": tid, "manifest": f["manifest"], "mode": f["mode"], "discipline": f["discipline"],
             "tab": f["tab"], "section": f["section"], "class": cls, "evidence": evidence,
             "placements": count[tid], "modes": sorted(modes[tid]),
             "metadata_wired_flag": flags.get(tid)}
        if references:
            e["references"] = references
        entries.append(e)
        if flags.get(tid) is not None and flags[tid] != (cls == "SHIPPED") and cls != "UNDECIDABLE":
            disagree.append(tid)
    total = len(manifest_ids)
    covered = len({e["id"] for e in entries} & raw_ids)
    fp.extend(("manifest/" + s["file"], hashlib.sha256(read_text(os.path.join(manifests_dir, s["file"])).encode("utf-8")).hexdigest()) for s in summaries)
    fingerprint = hashlib.sha256(json.dumps(sorted(fp)).encode("utf-8")).hexdigest()
    per_file = {}
    for s in summaries:
        per_file[s["file"]] = s
    for e in entries:
        per_file[e["manifest"]].setdefault("first_listed_ids", 0)
        per_file[e["manifest"]]["first_listed_ids"] += 1
    undec_pct = round(100.0 * totals["UNDECIDABLE"] / total, 2) if total else 0.0
    register = {
        "schema": "eustress.capability_register/1",
        "generated_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "commit": git_commit(repo_root),
        "source_fingerprint_sha256": fingerprint,
        "generator": "scripts/gen_capability_register.py",
        "purpose": ("Every demo shows working software, and every declared tool has a clear, checkable path "
                    "to shipped. The register reports what is reachable by a user today and changes only "
                    "when source changes."),
        "classification_rule": CLASSIFICATION_RULE,
        "inputs": {"manifests_dir": norm(os.path.relpath(manifests_dir, repo_root)),
                   "sources_dir": norm(os.path.relpath(sources_dir, repo_root)),
                   "rust_files_with_manifest_prefix_literals": scanned,
                   "mcp_tool_names_registered": len(mcp)},
        "manifests": [per_file[k] for k in sorted(per_file)],
        "coverage": {"unique_ids_in_manifests": total, "ids_in_register": len(entries),
                     "ids_found_by_independent_text_extraction": len(raw_ids),
                     "text_extraction_ids_present_in_register": covered,
                     "malformed_tool_entries": malformed,
                     "coverage_pct": round(100.0 * len(entries) / total, 2) if total else 0.0,
                     "cross_check_pct": round(100.0 * covered / len(raw_ids), 2) if raw_ids else 0.0},
        "totals": {"shipped": totals["SHIPPED"], "declared": totals["DECLARED"],
                   "undecidable": totals["UNDECIDABLE"], "total_ids": total,
                   "undecidable_pct": undec_pct},
        "metadata_flag_disagreements": {
            "note": ("tool_metadata.rs carries a generated wired flag. It is not evidence. These ids are "
                     "ones where the flag and the source-derived class disagree (UNDECIDABLE excluded)."),
            "count": len(disagree), "ids": disagree},
        "mcp_tools": {"note": "Registered ToolDefinition names; a gate step naming one of these passes.",
                      "names": {k: mcp[k] for k in sorted(mcp)}},
        "ids": entries,
    }
    return register


# ───────────────────────────── rendering ────────────────────────────────────

def render_md(reg):
    t = reg["totals"]
    out = []
    out.append("# Capability register")
    out.append("")
    out.append("Generated from source by `scripts/gen_capability_register.py`; do not edit by hand. "
               "Commit `%s`, generated %s." % (reg["commit"], reg["generated_at"]))
    out.append("")
    out.append(reg["purpose"])
    out.append("")
    out.append("manifests=%d total_ids=%d shipped=%d declared=%d undecidable=%d (%.2f%%)"
               % (len(reg["manifests"]), t["total_ids"], t["shipped"], t["declared"], t["undecidable"], t["undecidable_pct"]))
    out.append("")
    out.append("## Classification rule (%s)" % reg["classification_rule"]["name"])
    out.append("")
    out.append(reg["classification_rule"]["summary"])
    out.append("")
    for k in ("shipped_evidence", "undecidable_reasons", "known_limits"):
        out.append("**%s**" % k.replace("_", " "))
        out.append("")
        for line in reg["classification_rule"][k]:
            out.append("- " + line)
        out.append("")
    out.append("## By mode")
    out.append("")
    bymode = {}
    for e in reg["ids"]:
        bymode.setdefault(e["mode"], []).append(e)
    out.append("| mode | ids | shipped | declared | undecidable |")
    out.append("|---|---|---|---|---|")
    for m in sorted(bymode):
        es = bymode[m]
        out.append("| %s | %d | %d | %d | %d |" % (m, len(es), sum(e["class"] == "SHIPPED" for e in es),
                                                   sum(e["class"] == "DECLARED" for e in es),
                                                   sum(e["class"] == "UNDECIDABLE" for e in es)))
    out.append("")
    out.append("Ids are listed once, under the mode of their first placement in manifest-file order; "
               "`placements` in the JSON counts every ribbon slot.")
    out.append("")
    out.append("## Shipped ids")
    out.append("")
    for m in sorted(bymode):
        es = [e for e in bymode[m] if e["class"] == "SHIPPED"]
        if not es:
            continue
        out.append("### %s" % m)
        out.append("")
        for e in es:
            out.append("- `%s`: %s" % (e["id"], "; ".join(e["evidence"][:2])))
        out.append("")
    out.append("## Undecidable ids")
    out.append("")
    for e in reg["ids"]:
        if e["class"] == "UNDECIDABLE":
            out.append("- `%s` (%s): %s" % (e["id"], e["mode"], "; ".join(e["evidence"])))
    out.append("")
    out.append("## Declared ids by mode")
    out.append("")
    out.append("Each id below has a manifest entry and generated metadata and no dispatch site in Rust "
               "source today. They are the road map, not the product.")
    out.append("")
    for m in sorted(bymode):
        es = [e for e in bymode[m] if e["class"] == "DECLARED"]
        if not es:
            continue
        out.append("### %s (%d)" % (m, len(es)))
        out.append("")
        out.append(", ".join("`%s`" % e["id"] for e in es))
        out.append("")
    return "\n".join(out) + "\n"


def dump_json(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def write_file(path, text):
    d = os.path.dirname(os.path.abspath(path))
    os.makedirs(d, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


# ───────────────────────────── gate ─────────────────────────────────────────

def load_demo_steps(path):
    with open(path, "r", encoding="utf-8") as f:
        d = json.load(f)
    steps = d.get("steps", d) if isinstance(d, dict) else d
    out = []
    for i, s in enumerate(steps, 1):
        if isinstance(s, str):
            out.append((i, s))
        else:
            out.append((int(s.get("step", i)), s.get("tool") or s.get("id") or ""))
    return out


def gate(register_path, script_path):
    reg = json.loads(read_text(register_path))
    classes = {e["id"]: e["class"] for e in reg["ids"]}
    for name in reg.get("mcp_tools", {}).get("names", {}):
        classes.setdefault(name, "SHIPPED")
    steps = load_demo_steps(script_path)
    blocked = 0
    for num, tool in steps:
        cls = classes.get(tool, "UNKNOWN")
        if cls != "SHIPPED":
            blocked += 1
            print("GATE: demo step %d references '%s' -> %s. Blocked." % (num, tool, cls))
    if blocked:
        print("GATE: %d of %d steps blocked; the script cannot be blessed." % (blocked, len(steps)))
        return 2
    print("GATE: all %d steps are SHIPPED; the script is accepted." % len(steps))
    return 0


# ───────────────────────────── main ─────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser(description="Eustress capability register generator and demo gate")
    ap.add_argument("--manifests", default="eustress/crates/engine/modes")
    ap.add_argument("--sources", default="eustress/crates")
    ap.add_argument("--out")
    ap.add_argument("--md", help="Markdown output path (default: --out with .md)")
    ap.add_argument("--compare", nargs=2, metavar=("A", "B"))
    ap.add_argument("--ignore-fields", nargs="*", default=[])
    ap.add_argument("--assert-class", action="append", metavar="ID=CLASS")
    ap.add_argument("--register", default="docs/PROMPTS/artifacts/G6.30/capability_register.json")
    ap.add_argument("--gate", action="store_true")
    ap.add_argument("--demo-script")
    ap.add_argument("--print-rule", action="store_true")
    ap.add_argument("--max-undecidable-pct", type=float, default=5.0)
    args = ap.parse_args()

    if args.print_rule:
        print(json.dumps(CLASSIFICATION_RULE, indent=2))
        return 0

    if args.compare:
        a = json.loads(read_text(args.compare[0]))
        b = json.loads(read_text(args.compare[1]))
        for k in args.ignore_fields:
            a.pop(k, None)
            b.pop(k, None)
        if dump_json(a) == dump_json(b):
            print("COMPARE: identical apart from %s" % (", ".join(args.ignore_fields) or "nothing"))
            return 0
        for k in sorted(set(a) | set(b)):
            if a.get(k) != b.get(k):
                print("COMPARE: field differs: %s" % k)
        return 1

    if args.assert_class:
        reg = json.loads(read_text(args.register))
        classes = {e["id"]: e["class"] for e in reg["ids"]}
        bad = 0
        for item in args.assert_class:
            tid, _, want = item.partition("=")
            got = classes.get(tid, "ABSENT")
            if got == want:
                print("ASSERT: %s == %s ok" % (tid, want))
            else:
                print("ASSERT: %s expected %s, register says %s" % (tid, want, got))
                bad += 1
        return 1 if bad else 0

    if args.gate:
        if not args.demo_script:
            print("--gate needs --demo-script", file=sys.stderr)
            return 1
        try:
            return gate(args.register, args.demo_script)
        except (OSError, ValueError, KeyError) as e:
            print("GATE: cannot evaluate: %s" % e, file=sys.stderr)
            return 1

    if not args.out:
        ap.print_usage(sys.stderr)
        return 1
    repo_root = os.getcwd()
    if not os.path.isdir(args.manifests) or not os.path.isdir(args.sources):
        print("manifests or sources directory not found (run from the repository root)", file=sys.stderr)
        return 1
    reg = build_register(args.manifests, args.sources, repo_root)
    write_file(args.out, dump_json(reg))
    md_path = args.md or (os.path.splitext(args.out)[0] + ".md")
    write_file(md_path, render_md(reg))
    t = reg["totals"]
    print("manifests=%d total_ids=%d shipped=%d declared=%d undecidable=%d (%.2f%%)"
          % (len(reg["manifests"]), t["total_ids"], t["shipped"], t["declared"], t["undecidable"], t["undecidable_pct"]))
    print("classification_rule=%s" % RULE_NAME)
    print("coverage=%.2f%% cross_check=%.2f%% metadata_flag_disagreements=%d"
          % (reg["coverage"]["coverage_pct"], reg["coverage"]["cross_check_pct"], reg["metadata_flag_disagreements"]["count"]))
    if reg["coverage"]["coverage_pct"] < 100.0 or reg["coverage"]["cross_check_pct"] < 100.0:
        print("FAIL: coverage below 100%", file=sys.stderr)
        return 1
    if t["undecidable_pct"] > args.max_undecidable_pct:
        print("FAIL: undecidable %.2f%% exceeds %.1f%% ceiling" % (t["undecidable_pct"], args.max_undecidable_pct), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
