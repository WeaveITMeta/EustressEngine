# -*- coding: utf-8 -*-
"""Emits the mode → discipline → tab → section → tool hierarchy as JSON for
the eustress.dev admin telemetry page (crates/web/assets/tool_hierarchy.json).

The admin page merges LIVE counts (from /api/telemetry/summary) into this
static skeleton client-side — the worker stores only counters keyed by tool
id, so the hierarchy itself never needs to round-trip through KV.

Run from the repo root after any mode-manifest edit, alongside the other two
generators:
    python scripts/gen_tool_metadata.py
    python scripts/gen_tool_icons.py
    python scripts/gen_web_hierarchy.py
"""
import glob
import importlib.util
import json
import os
import sys

spec = importlib.util.spec_from_file_location("gtm", os.path.join("scripts", "gen_tool_metadata.py"))
gtm = importlib.util.module_from_spec(spec)
sys.modules["gtm"] = gtm
spec.loader.exec_module(gtm)

try:
    import tomllib
except ImportError:
    import tomli as tomllib

OUT = "eustress/crates/web/assets/tool_hierarchy.json"


def tool_entry(tid):
    if tid in gtm.WIRED:
        label, tip, _icon = gtm.WIRED[tid]
        wired = True
    else:
        label, tip, wired = gtm.label_for(tid), gtm.tooltip_for(tid), False
    return {"id": tid, "label": label, "tooltip": tip, "wired": wired}


def tabs_payload(tab_list):
    out = []
    for tab in tab_list:
        out.append({
            "id": tab["id"],
            "name": tab["name"],
            "color": tab.get("color", ""),
            "sections": [
                {"name": s["name"], "tools": [tool_entry(t) for t in s.get("tools", [])]}
                for s in tab.get("sections", [])
            ],
        })
    return out


modes_out = []
for path in sorted(glob.glob(gtm.MODES_GLOB)):
    d = tomllib.load(open(path, "rb"))
    mode = d["mode"]
    submodes = d.get("submodes", [])
    fallback = d.get("tabs", [])
    groups = {}
    for st in d.get("submode_tabs", []):
        groups.setdefault(st["submode"], []).append(st)

    disciplines = []
    for sm in submodes:
        # Mirrors ModeManifest::effective_custom_tabs — override or fallback.
        tabs = groups.get(sm["id"], fallback)
        disciplines.append({
            "id": sm["id"],
            "name": sm["name"],
            "icon": sm.get("icon", ""),
            "tabs": tabs_payload(tabs),
        })

    modes_out.append({
        "id": mode["id"],
        "name": mode["name"],
        "color": mode.get("color", ""),
        "disciplines": disciplines,
        # Flat modes (Gaming) surface their tabs directly.
        "tabs": tabs_payload(fallback) if not submodes else [],
    })

payload = {"modes": modes_out}
n_tools = sum(
    len(sec["tools"])
    for m in modes_out
    for holder in (m["disciplines"] or [{"tabs": m["tabs"]}])
    for tab in holder["tabs"]
    for sec in tab["sections"]
)
os.makedirs(os.path.dirname(OUT), exist_ok=True)
with open(OUT, "w", encoding="utf-8", newline="\n") as f:
    json.dump(payload, f, ensure_ascii=False, separators=(",", ":"))
print(f"wrote {OUT}: {len(modes_out)} modes, {n_tools} tool slots, "
      f"{os.path.getsize(OUT) // 1024} KB")
