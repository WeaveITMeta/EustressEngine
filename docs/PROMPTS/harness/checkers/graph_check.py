#!/usr/bin/env python3
"""graph_check.py — mechanical integrity check of the gauntlet prompt library graph.

Parses the YAML front matter of every item across all packs in docs/PROMPTS/packs/
and re-derives, from the files alone, the nine properties docs/PROMPTS/02_QUEUE.md
sections 8 and 9 assert. It reads no summary and trusts no declared total.

Checks performed
  1. TOTAL ITEMS      — count of parsed items, per-pack breakdown.
  2. CYCLES           — merged depends_on + blocks graph must be acyclic.
  3. DANGLING IDS     — every id named in depends_on or blocks must resolve.
  4. GATE 1           — every item with max_builds > 0 must reach G1.01 transitively
                        (WORKSPACE-BUILD-GATE, 02_QUEUE.md section 8.4).
  5. GATE 2           — every item whose capture_recipe is not `none` must reach both
                        recipe-harness owners transitively (HARNESS-RECIPE-GATE,
                        02_QUEUE.md section 8.2). Owners are derived from the G1 pack,
                        not hardcoded: the item that builds the eustress-capture binary
                        and the item that authors the cited recipes.
  6. TABLE VS FM      — the depends_on column of 02_QUEUE.md section 3 must agree with
                        each item's actual front matter.
  7. ID UNIQUENESS    — no id may be defined twice across the packs.
  8. BLOCKS RECIPROCITY
                      — for every `blocks: [Y]` declared on X, Y's depends_on must
                        reach X, directly or transitively (02_QUEUE.md section 8.7).
                        An unmirrored blocks entry asserts an ordering constraint that
                        nothing schedules on, because section 3's depends_on column and
                        the section 4 wave floors both read depends_on and never blocks.
                        Violations that are also wave inversions — the blocked item
                        floored at or before its blocker — are flagged as such.
  9. WAVE COLUMN      — the Wave cell of every section 3 row must equal the wave floor
                        recomputed from the declared graph, and the section 4.3
                        membership lists must agree with those same floors.

Edge direction
  depends_on: X means "this item needs X first", so the dependency edge runs
  item -> X and reachability is computed by following depends_on forward.
  blocks: Y is the reverse assertion, so it contributes the edge Y -> this item.
  Cycle detection runs over the union of both, which is the merged graph.

Which graph each check reads
  Checks 4 and 5 are gate-reachability questions and read the merged graph, because a
  gate is satisfied by any declared edge. Checks 8 and 9 read the depends_on-only
  graph and must never read the merged one:
    - Check 8 asks whether a blocks entry is mirrored. In the merged graph a blocks
      entry contributes the very edge it is being asked to prove, so the check would
      pass unconditionally and prove nothing.
    - Check 9 recomputes wave floors, and section 3 declares those floors to be
      computed from depends_on. Reading blocks there would verify the queue against a
      graph the queue does not claim to use.

Wave floor
  floor(x) = 0 when x declares no dependency, else 1 + max(floor(d)) over the declared
  depends_on of x. This is the longest chain back to a dependency-free item, which is
  the definition section 3 states. Unresolvable dependency ids are excluded from the
  recurrence and reported by check 3 instead.

Exit codes
  0  clean — every check passed.
  1  one or more defects found (details printed to stdout).
  2  usage or environment error (packs directory or queue file not found,
     no items parsed).

Usage
  python docs/PROMPTS/harness/checkers/graph_check.py [--prompts-dir DIR] [--json]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import defaultdict, deque

EXIT_CLEAN = 0
EXIT_DEFECTS = 1
EXIT_USAGE = 2

# Fields we care about. Everything else in the front matter is ignored.
SCALAR_FIELDS = {"id", "title", "phase", "tier", "max_builds", "capture_recipe", "status"}
LIST_FIELDS = {"depends_on", "blocks"}

ID_RE = re.compile(r"\b([A-Z]\d+\.\d+)\b")


def parse_front_matter_blocks(text: str, path: str):
    """Yield (dict, line_number) for every front-matter block in a pack file.

    A block is a line that is exactly `---` whose immediately following line
    starts with `id:`, terminated by the next line that is exactly `---`.
    """
    lines = text.splitlines()
    i = 0
    n = len(lines)
    while i < n:
        if lines[i].strip() == "---" and i + 1 < n and lines[i + 1].lstrip().startswith("id:"):
            start = i + 1
            j = start
            body = []
            while j < n and lines[j].strip() != "---":
                body.append(lines[j])
                j += 1
            yield parse_block(body, path, start + 1), start + 1
            i = j + 1
        else:
            i += 1


def parse_block(body, path, lineno):
    """Parse the subset of YAML the front matter actually uses."""
    out = {"_file": path, "_line": lineno}
    for raw in body:
        if not raw or raw[0] in " \t#":
            # indented continuation of a folded scalar, or a comment
            continue
        if ":" not in raw:
            continue
        key, _, value = raw.partition(":")
        key = key.strip()
        value = value.strip()
        if key in LIST_FIELDS:
            out[key] = parse_flow_list(value)
        elif key in SCALAR_FIELDS:
            if value in (">", "|"):
                out[key] = ""
            else:
                out[key] = value.strip().strip("`")
    return out


def parse_flow_list(value: str):
    """Parse `[A, B]`, `[]`, or a bare scalar into a list of strings."""
    value = value.strip()
    if not value:
        return []
    if value.startswith("[") and value.endswith("]"):
        inner = value[1:-1].strip()
        if not inner:
            return []
        return [tok.strip().strip("`\"'") for tok in inner.split(",") if tok.strip()]
    return [value.strip().strip("`\"'")]


def to_int(value, default=0):
    try:
        return int(str(value).strip())
    except (TypeError, ValueError):
        return default


def load_items(packs_dir):
    """Return (items_by_id, duplicates, per_pack_counts, ordered_records)."""
    items = {}
    duplicates = []
    per_pack = {}
    records = []
    for name in sorted(os.listdir(packs_dir)):
        if not name.endswith(".md"):
            continue
        path = os.path.join(packs_dir, name)
        with open(path, "r", encoding="utf-8") as fh:
            text = fh.read()
        count = 0
        for block, _lineno in parse_front_matter_blocks(text, name):
            iid = block.get("id")
            if not iid:
                continue
            block["_pack"] = name
            count += 1
            records.append(block)
            if iid in items:
                duplicates.append(
                    "%s defined in %s:%d and again in %s:%d"
                    % (iid, items[iid]["_pack"], items[iid]["_line"], name, block["_line"])
                )
            else:
                items[iid] = block
        per_pack[name] = count
    return items, duplicates, per_pack, records


def build_graph(items):
    """Merged dependency graph. Edge A -> B means A must wait for B.

    depends_on on A contributes A -> B directly.
    blocks: [C] on B contributes C -> B (C waits for B), the reverse assertion.
    """
    deps = defaultdict(set)
    for iid, block in items.items():
        for dep in block.get("depends_on", []):
            deps[iid].add(dep)
        for blocked in block.get("blocks", []):
            deps[blocked].add(iid)
    return deps


def build_depends_only_graph(items):
    """Dependency graph from `depends_on` alone. Edge A -> B means A waits for B.

    Deliberately excludes `blocks`. Checks 8 and 9 both depend on that exclusion:
    reciprocity would be self-proving over the merged graph, and the wave floors in
    02_QUEUE.md sections 3 and 4 are declared to come from depends_on.
    """
    deps = defaultdict(set)
    for iid, block in items.items():
        for dep in block.get("depends_on", []):
            deps[iid].add(dep)
    return deps


def compute_wave_floors(nodes, dep_only):
    """floor(x) = 0 with no declared dependency, else 1 + max(floor(dep)).

    Returns (floors, unresolved). `unresolved` holds ids whose floor could not be
    computed because they sit on a dependency cycle; a cycle is already check 2's
    defect, and this function refuses to invent a number for one.
    """
    floors = {}
    unresolved = []
    for start in sorted(nodes):
        if start in floors:
            continue
        # iterative post-order so a deep chain cannot blow the recursion limit
        stack = [(start, False)]
        active = set()
        while stack:
            node, expanded = stack.pop()
            if expanded:
                active.discard(node)
                edges = [d for d in dep_only.get(node, ()) if d in nodes]
                if not edges:
                    floors[node] = 0
                elif any(floors.get(d) is None for d in edges):
                    # A predecessor sits on a cycle, so it has no floor and neither
                    # does this node. `.get(d) is None` covers both the never-computed
                    # key and the key already marked floorless — testing membership
                    # alone would let a None through into the arithmetic below.
                    floors[node] = None
                else:
                    floors[node] = 1 + max(floors[d] for d in edges)
                continue
            if node in floors or node in active:
                continue
            active.add(node)
            stack.append((node, True))
            for d in sorted(dep_only.get(node, ())):
                if d in nodes and d not in floors:
                    stack.append((d, False))
    for iid in sorted(nodes):
        if floors.get(iid) is None:
            unresolved.append(iid)
            floors.pop(iid, None)
    return floors, unresolved


def find_cycles(nodes, deps):
    """Return every elementary cycle as an explicit id chain (Tarjan SCC + DFS)."""
    index = {}
    low = {}
    on_stack = {}
    stack = []
    counter = [0]
    sccs = []

    def strongconnect(v):
        # iterative Tarjan to survive deep graphs
        work = [(v, iter(sorted(deps.get(v, ()))))]
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on_stack[v] = True
        while work:
            node, it = work[-1]
            advanced = False
            for w in it:
                if w not in nodes:
                    continue
                if w not in index:
                    index[w] = low[w] = counter[0]
                    counter[0] += 1
                    stack.append(w)
                    on_stack[w] = True
                    work.append((w, iter(sorted(deps.get(w, ())))))
                    advanced = True
                    break
                if on_stack.get(w):
                    low[node] = min(low[node], index[w])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[node])
            if low[node] == index[node]:
                comp = []
                while True:
                    w = stack.pop()
                    on_stack[w] = False
                    comp.append(w)
                    if w == node:
                        break
                sccs.append(comp)

    for v in sorted(nodes):
        if v not in index:
            strongconnect(v)

    cycles = []
    for comp in sccs:
        members = set(comp)
        if len(comp) == 1:
            only = comp[0]
            if only in deps.get(only, ()):
                cycles.append("%s -> %s" % (only, only))
            continue
        # enumerate simple cycles inside this SCC
        found = set()
        for start in sorted(members):
            path = [start]
            seen = {start}

            def dfs(node):
                for nxt in sorted(deps.get(node, ())):
                    if nxt not in members:
                        continue
                    if nxt == start:
                        rot = tuple(path)
                        key = min(
                            tuple(rot[i:] + rot[:i]) for i in range(len(rot))
                        )
                        found.add(key)
                    elif nxt not in seen and len(path) < 24:
                        seen.add(nxt)
                        path.append(nxt)
                        dfs(nxt)
                        path.pop()
                        seen.discard(nxt)

            dfs(start)
        for cyc in sorted(found):
            cycles.append(" -> ".join(cyc) + " -> " + cyc[0])
    return sorted(set(cycles))


def reaches(start, target, deps, nodes):
    """True if target is reachable from start following depends_on edges."""
    if start == target:
        return True
    seen = {start}
    q = deque([start])
    while q:
        cur = q.popleft()
        for nxt in deps.get(cur, ()):
            if nxt == target:
                return True
            if nxt in nodes and nxt not in seen:
                seen.add(nxt)
                q.append(nxt)
    return False


def derive_recipe_owners(packs_dir, items):
    """Identify, from the G1 pack itself, the items that build eustress-capture
    and that author the capture recipes. Returns a sorted list of ids."""
    owners = set()
    g1 = os.path.join(packs_dir, "G1_capture_harness.md")
    if not os.path.exists(g1):
        return []
    with open(g1, "r", encoding="utf-8") as fh:
        text = fh.read()
    for block, _ln in parse_front_matter_blocks(text, "G1_capture_harness.md"):
        iid = block.get("id")
        title = (block.get("title") or "").lower()
        if not iid:
            continue
        builds_binary = "eustress-capture" in title and (
            "binary" in title or "producing" in title
        )
        authors_recipes = "recipe" in title and (
            "author" in title or "format" in title
        )
        if builds_binary or authors_recipes:
            owners.add(iid)
    return sorted(owners)


WAVE_CELL_RE = re.compile(r"^W(\d+)$")


def parse_queue_table(queue_path):
    """Return {id: {"deps": [...], "wave": int|None, "wave_raw": str}} from section 3."""
    with open(queue_path, "r", encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    in_section = False
    rows = {}
    for line in lines:
        if line.startswith("## 3."):
            in_section = True
            continue
        if in_section and line.startswith("## ") and not line.startswith("## 3."):
            break
        if not in_section:
            continue
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 10:
            continue
        if cells[0].lower().startswith("wave") or set(cells[0]) <= set(":-"):
            continue
        id_cell = cells[1]
        m = ID_RE.search(id_cell)
        if not m:
            continue
        iid = m.group(1)
        dep_cell = cells[6]
        deps = ID_RE.findall(dep_cell)
        wave_raw = cells[0].strip().strip("`*")
        wm = WAVE_CELL_RE.match(wave_raw)
        rows[iid] = {
            "deps": deps,
            "wave": int(wm.group(1)) if wm else None,
            "wave_raw": wave_raw,
        }
    return rows


def parse_wave_membership(queue_path):
    """Return {id: wave_int} from the section 4.3 membership table of 02_QUEUE.md.

    That table has a Wave column and two item-list columns (tier S, build-consuming),
    and it is located by its own header rather than by position. The gate matters:
    section 4.2's residual shared-path table also opens each row with a bare `W<n>`
    and also names item ids, so a parser that keyed on the wave cell alone would read
    an owner id out of 4.2 and assign it the wave of the contended path.
    """
    with open(queue_path, "r", encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    in_section = False
    in_table = False
    membership = {}
    duplicates = []
    for line in lines:
        if line.startswith("## 4."):
            in_section = True
            continue
        if in_section and line.startswith("## ") and not line.startswith("## 4."):
            break
        if not in_section:
            continue
        if not line.startswith("|"):
            in_table = False
            continue
        lowered = line.lower()
        if "tier s" in lowered and "build-consuming" in lowered:
            in_table = True
            continue
        if not in_table:
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 3:
            continue
        wm = WAVE_CELL_RE.match(cells[0].strip().strip("`*"))
        if not wm:
            continue
        wave = int(wm.group(1))
        found = []
        for cell in cells[1:]:
            found.extend(ID_RE.findall(cell))
        if not found:
            continue
        for iid in found:
            if iid in membership and membership[iid] != wave:
                duplicates.append(
                    "%s listed in both W%d and W%d of section 4.3"
                    % (iid, membership[iid], wave)
                )
            membership[iid] = wave
    return membership, duplicates


WAVE_COUNT_ROW_RE = re.compile(r"^\|\s*W(\d+)\s*\|\s*(\d+)")


def parse_wave_counts(queue_path):
    """Return {wave_int: declared_item_count} from the section 4.3 arithmetic table.

    That table's second cell is the item count for the wave. The pattern is safe
    against the other section 4 tables because it requires an integer in cell two:
    section 4.2's rows carry a path there and the membership rows carry a backticked
    item id, so neither matches.
    """
    with open(queue_path, "r", encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    in_section = False
    counts = {}
    for line in lines:
        if line.startswith("## 4."):
            in_section = True
            continue
        if in_section and line.startswith("## ") and not line.startswith("## 4."):
            break
        if not in_section:
            continue
        m = WAVE_COUNT_ROW_RE.match(line.strip())
        if m:
            counts[int(m.group(1))] = int(m.group(2))
    return counts


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--prompts-dir",
        default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."),
        help="path to docs/PROMPTS (defaults to two levels above this script)",
    )
    ap.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    args = ap.parse_args()

    prompts_dir = os.path.abspath(args.prompts_dir)
    packs_dir = os.path.join(prompts_dir, "packs")
    queue_path = os.path.join(prompts_dir, "02_QUEUE.md")

    if not os.path.isdir(packs_dir):
        print("FATAL: packs directory not found: %s" % packs_dir)
        return EXIT_USAGE

    items, duplicates, per_pack, records = load_items(packs_dir)
    if not records:
        print("FATAL: no items parsed from %s" % packs_dir)
        return EXIT_USAGE

    nodes = set(items)
    deps = build_graph(items)

    # --- 3. dangling ids -------------------------------------------------
    dangling = []
    for iid, block in sorted(items.items()):
        for field in ("depends_on", "blocks"):
            for ref in block.get(field, []):
                if ref not in nodes:
                    dangling.append("%s.%s names %s, which is not a defined item id" % (iid, field, ref))

    # --- 2. cycles -------------------------------------------------------
    cycles = find_cycles(nodes, {k: {v for v in vs if v in nodes} for k, vs in deps.items()})

    clean_deps = {k: {v for v in vs if v in nodes} for k, vs in deps.items()}

    # --- 4. gate 1 -------------------------------------------------------
    gate1_violations = []
    build_consumers = [i for i, b in items.items() if to_int(b.get("max_builds")) > 0]
    for iid in sorted(build_consumers):
        if iid == "G1.01":
            continue
        if "G1.01" not in nodes:
            gate1_violations.append("G1.01 is not a defined item id")
            break
        if not reaches(iid, "G1.01", clean_deps, nodes):
            gate1_violations.append(
                "%s (%s, max_builds=%s) does not reach G1.01"
                % (iid, items[iid]["_pack"], items[iid].get("max_builds"))
            )

    # --- 5. gate 2 -------------------------------------------------------
    owners = derive_recipe_owners(packs_dir, items)
    gate2_violations = []
    recipe_citers = [
        i
        for i, b in items.items()
        if (b.get("capture_recipe") or "none").strip().lower() not in ("none", "")
    ]
    if not owners:
        gate2_violations.append("could not derive recipe-harness owners from G1 pack")
    for iid in sorted(recipe_citers):
        missing = [o for o in owners if o != iid and not reaches(iid, o, clean_deps, nodes)]
        if missing:
            gate2_violations.append(
                "%s (%s, recipe=%s) does not reach %s"
                % (iid, items[iid]["_pack"], items[iid].get("capture_recipe"), ", ".join(missing))
            )

    # --- 6. table vs front matter ----------------------------------------
    table_mismatches = []
    queue_missing = not os.path.exists(queue_path)
    if queue_missing:
        table_mismatches.append("02_QUEUE.md not found at %s" % queue_path)
        table_rows = {}
    else:
        table_rows = parse_queue_table(queue_path)
        for iid in sorted(nodes):
            fm = sorted(items[iid].get("depends_on", []))
            if iid not in table_rows:
                table_mismatches.append("%s has no row in 02_QUEUE.md section 3" % iid)
                continue
            tb = sorted(table_rows[iid]["deps"])
            if fm != tb:
                table_mismatches.append(
                    "%s: table says [%s], front matter says [%s]"
                    % (iid, " ".join(tb) or "-", " ".join(fm) or "-")
                )
        for iid in sorted(table_rows):
            if iid not in nodes:
                table_mismatches.append("%s appears in the table but in no pack" % iid)

    # --- 8. blocks reciprocity -------------------------------------------
    # Read the depends_on-only graph. Over the merged graph a `blocks` entry
    # supplies the exact edge it is being asked to prove, so the check would be
    # vacuous — it could not fail, and a check that cannot fail proves nothing.
    dep_only = build_depends_only_graph(items)
    clean_dep_only = {k: {v for v in vs if v in nodes} for k, vs in dep_only.items()}
    floors, floorless = compute_wave_floors(nodes, clean_dep_only)

    reciprocity_violations = []
    blocks_pairs = 0
    blocks_direct = 0
    blocks_transitive = 0
    for blocker in sorted(nodes):
        for blocked in items[blocker].get("blocks", []):
            if blocked not in nodes:
                continue  # unresolvable id — check 3 owns that defect
            blocks_pairs += 1
            if blocker in items[blocked].get("depends_on", []):
                blocks_direct += 1
                continue
            if reaches(blocked, blocker, clean_dep_only, nodes):
                blocks_transitive += 1
                continue
            bw = floors.get(blocker)
            dw = floors.get(blocked)
            if bw is None or dw is None:
                inversion = "unknown (wave floor undefined)"
            elif dw < bw:
                inversion = "WAVE INVERSION: blocked W%d is %d wave(s) before blocker W%d" % (
                    dw,
                    bw - dw,
                    bw,
                )
            elif dw == bw:
                inversion = "WAVE INVERSION: both floor at W%d and would run concurrently" % dw
            else:
                inversion = "no inversion today (blocked W%d after blocker W%d) but undeclared" % (
                    dw,
                    bw,
                )
            reciprocity_violations.append(
                "%s (%s) blocks %s, but %s.depends_on does not reach %s at any depth: %s"
                % (blocker, items[blocker]["_pack"], blocked, blocked, blocker, inversion)
            )

    # --- 9. wave column agreement ----------------------------------------
    wave_mismatches = []
    membership = {}
    if floorless:
        wave_mismatches.append(
            "wave floors undefined for %d item(s) on a dependency cycle: %s"
            % (len(floorless), " ".join(floorless))
        )
    if not queue_missing:
        for iid in sorted(nodes):
            row = table_rows.get(iid)
            if row is None:
                continue  # check 6 already reported the missing row
            computed = floors.get(iid)
            if row["wave"] is None:
                wave_mismatches.append(
                    "%s: section 3 wave cell %r is not a W<n> value"
                    % (iid, row["wave_raw"])
                )
            elif computed is not None and row["wave"] != computed:
                wave_mismatches.append(
                    "%s: section 3 says W%d, recomputed floor is W%d"
                    % (iid, row["wave"], computed)
                )
        membership, member_dupes = parse_wave_membership(queue_path)
        wave_mismatches.extend(member_dupes)
        for iid in sorted(nodes):
            computed = floors.get(iid)
            if iid not in membership:
                wave_mismatches.append("%s is in no section 4.3 wave list" % iid)
            elif computed is not None and membership[iid] != computed:
                wave_mismatches.append(
                    "%s: section 4.3 lists it in W%d, recomputed floor is W%d"
                    % (iid, membership[iid], computed)
                )
        for iid in sorted(membership):
            if iid not in nodes:
                wave_mismatches.append(
                    "%s appears in a section 4.3 wave list but in no pack" % iid
                )
        declared_counts = parse_wave_counts(queue_path)
        computed_counts = defaultdict(int)
        for iid in nodes:
            if floors.get(iid) is not None:
                computed_counts[floors[iid]] += 1
        for wave in sorted(set(declared_counts) | set(computed_counts)):
            got = declared_counts.get(wave)
            want = computed_counts.get(wave, 0)
            if got is None:
                wave_mismatches.append(
                    "section 4.3 declares no item count for W%d, which holds %d item(s)"
                    % (wave, want)
                )
            elif got != want:
                wave_mismatches.append(
                    "section 4.3 declares %d item(s) in W%d, recomputed count is %d"
                    % (got, wave, want)
                )

    result = {
        "total_items": len(records),
        "unique_ids": len(nodes),
        "per_pack": per_pack,
        "duplicate_ids": duplicates,
        "cycles": cycles,
        "dangling_ids": dangling,
        "recipe_owners": owners,
        "build_consumer_count": len(build_consumers),
        "gate1_violations": gate1_violations,
        "recipe_citer_count": len(recipe_citers),
        "gate2_violations": gate2_violations,
        "table_vs_frontmatter_mismatches": table_mismatches,
        "blocks_pairs": blocks_pairs,
        "blocks_direct_reciprocal": blocks_direct,
        "blocks_transitive_reciprocal": blocks_transitive,
        "reciprocity_violations": reciprocity_violations,
        "wave_mismatches": wave_mismatches,
        "deepest_wave": max(floors.values()) if floors else None,
        "roots": sorted(i for i in nodes if not items[i].get("depends_on")),
    }

    defect = bool(
        duplicates
        or cycles
        or dangling
        or gate1_violations
        or gate2_violations
        or table_mismatches
        or reciprocity_violations
        or wave_mismatches
    )
    result["verdict"] = "DEFECTS_FOUND" if defect else "CLEAN"

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print("== graph_check ==")
        print("packs dir: %s" % packs_dir)
        print("1. TOTAL ITEMS: %d parsed, %d unique ids" % (len(records), len(nodes)))
        for pack in sorted(per_pack):
            print("     %-44s %d" % (pack, per_pack[pack]))
        print("7. ID UNIQUENESS: %d duplicate id(s)" % len(duplicates))
        for d in duplicates:
            print("     DEFECT %s" % d)
        print("2. CYCLES: %d" % len(cycles))
        for c in cycles:
            print("     DEFECT %s" % c)
        print("3. DANGLING IDS: %d" % len(dangling))
        for d in dangling:
            print("     DEFECT %s" % d)
        print(
            "4. GATE 1 (max_builds>0 reaches G1.01): %d build-consuming item(s), %d violation(s)"
            % (len(build_consumers), len(gate1_violations))
        )
        for v in gate1_violations:
            print("     DEFECT %s" % v)
        print(
            "5. GATE 2 (capture_recipe != none reaches %s): %d recipe-citing item(s), %d violation(s)"
            % (", ".join(owners) or "<none derived>", len(recipe_citers), len(gate2_violations))
        )
        for v in gate2_violations:
            print("     DEFECT %s" % v)
        print(
            "6. TABLE VS FRONT MATTER: %d row(s) in 02_QUEUE.md section 3, %d mismatch(es)"
            % (len(table_rows), len(table_mismatches))
        )
        for m in table_mismatches:
            print("     DEFECT %s" % m)
        print(
            "8. BLOCKS RECIPROCITY: %d resolvable blocks pair(s): %d direct, %d transitive, %d violation(s)"
            % (blocks_pairs, blocks_direct, blocks_transitive, len(reciprocity_violations))
        )
        for v in reciprocity_violations:
            print("     DEFECT %s" % v)
        print(
            "9. WAVE COLUMN (section 3 + section 4.3 vs recomputed floors): %d row(s), %d mismatch(es)"
            % (len(table_rows), len(wave_mismatches))
        )
        for m in wave_mismatches:
            print("     DEFECT %s" % m)
        print("roots (no depends_on): %s" % " ".join(result["roots"]))
        print("deepest wave floor: W%s" % result["deepest_wave"])
        print("VERDICT: %s" % result["verdict"])

    return EXIT_DEFECTS if defect else EXIT_CLEAN


if __name__ == "__main__":
    # An uncaught exception would exit 1, which is indistinguishable from "defects
    # found" to any caller reading the exit code — a crashed checker would be read
    # as a working one that found problems. Route it to the environment-error code
    # instead, so exit 1 means defects and nothing else.
    try:
        sys.exit(main())
    except Exception:  # noqa: BLE001 - deliberate: the exit code is the contract
        import traceback

        traceback.print_exc()
        print("FATAL: graph_check crashed; exit 1 is reserved for declared defects")
        sys.exit(EXIT_USAGE)
