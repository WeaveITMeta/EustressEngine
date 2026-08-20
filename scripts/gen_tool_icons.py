# -*- coding: utf-8 -*-
"""Composes a UNIQUE icon per ribbon tool id into
eustress/crates/engine/assets/icons/tools/<id-sanitized>.svg.

Grammar (how AAA icon families scale to thousands without clip-art):
  base glyph  = the tool's domain archetype (from gen_tool_metadata's
                assignment), drawn at 80% scale, top-left anchored, with a
                masked knockout under the badge corner
  badge       = a small action modifier derived from the id's suffix
                (builder→plus, solver→check, tracker→pulse, viewer→eye, …)
                inside a 4.5px ring at the bottom-right
Ids with no recognizable action suffix render the base alone at 92% — the
absence of a badge is itself information (nouns vs. actions).

Run from the repo root AFTER gen_tool_metadata.py (it imports its tables):
    python scripts/gen_tool_icons.py
"""
import importlib.util
import os
import re
import sys

spec = importlib.util.spec_from_file_location("gtm", os.path.join("scripts", "gen_tool_metadata.py"))
gtm = importlib.util.module_from_spec(spec)
_main = gtm.__dict__
sys.modules["gtm"] = gtm
spec.loader.exec_module(gtm)

UI_DIR = "eustress/crates/engine/assets/icons/ui/"
OUT_DIR = "eustress/crates/engine/assets/icons/tools/"
os.makedirs(OUT_DIR, exist_ok=True)

S = 'stroke="#d4d4d4" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" fill="none"'
SB = 'stroke="#d4d4d4" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"'
HDR = '<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">\n'

# ── Badge modifiers: suffix/keyword → tiny glyph in the 15..20.2 box ─────────
# Each is drawn centered on (17.6, 17.6), radius budget ~2.6.
MODS = {
    "plus":    f'<path d="M17.6 15.4v4.4M15.4 17.6h4.4" {SB}/>',
    "check":   f'<path d="M15.6 17.7l1.4 1.4 2.6-2.9" {SB}/>',
    "magnify": f'<circle cx="17" cy="17" r="1.9" {SB}/><path d="M18.4 18.4l1.5 1.5" {SB}/>',
    "pencil":  f'<path d="M15.6 19.6l.4-1.6 2.9-2.9 1.2 1.2-2.9 2.9-1.6.4z" {SB}/>',
    "gear":    f'<circle cx="17.6" cy="17.6" r="1.1" {SB}/><path d="M17.6 15.1v1M17.6 19.1v1M15.1 17.6h1M19.1 17.6h1M15.9 15.9l.7.7M18.6 18.6l.7.7M19.3 15.9l-.7.7M16.6 18.6l-.7.7" {SB}/>',
    "eye":     f'<path d="M15.1 17.6s1-1.7 2.5-1.7 2.5 1.7 2.5 1.7-1 1.7-2.5 1.7-2.5-1.7-2.5-1.7z" {SB}/><circle cx="17.6" cy="17.6" r=".6" fill="#d4d4d4" stroke="none"/>',
    "pulse":   f'<path d="M15.2 17.6h1l.7-1.7 1.2 3.4.8-1.7h1.1" {SB}/>',
    "export":  f'<path d="M15.8 19.4l3.6-3.6M17 15.6h2.6v2.6" {SB}/>',
    "cycle":   f'<path d="M15.7 16.6a2.4 2.4 0 0 1 4-.4M19.5 18.6a2.4 2.4 0 0 1-4 .4" {SB}/><path d="M19.8 14.9v1.5h-1.5M15.4 20.3v-1.5h1.5" {SB}/>',
    "play":    f'<path d="M16.4 15.6l3 2-3 2v-4z" {SB}/>',
    "chart":   f'<path d="M15.4 19.8v-2.2M17.6 19.8v-3.6M19.8 19.8v-5" {SB}/>',
    "grid":    f'<rect x="15.4" y="15.4" width="4.4" height="4.4" {SB}/><path d="M17.6 15.4v4.4M15.4 17.6h4.4" {SB}/>',
    "lines":   f'<path d="M15.5 15.9h4.2M15.5 17.6h4.2M15.5 19.3h2.6" {SB}/>',
    "clock":   f'<circle cx="17.6" cy="17.6" r="2.3" {SB}/><path d="M17.6 16.2v1.4l1 .7" {SB}/>',
    "bolt":    f'<path d="M18 15.1l-1.9 2.8h1.4l-.9 2.6 2.4-3h-1.5l.5-2.4z" {SB}/>',
    "question":f'<path d="M16.6 16.6a1.1 1.1 0 0 1 2.1.4c0 .8-1.1.9-1.1 1.7" {SB}/><circle cx="17.6" cy="19.7" r=".55" fill="#d4d4d4" stroke="none"/>',
    "bell":    f'<path d="M16 18.6v-1.2a1.6 1.6 0 0 1 3.2 0v1.2l.6.8h-4.4l.6-.8z" {SB}/>',
    "calcpad": f'<rect x="15.7" y="15.2" width="3.8" height="4.8" rx=".7" {SB}/><path d="M16.6 16.4h2M16.7 18h.01M17.7 18h.01M18.7 18h.01M16.7 19.1h.01M17.7 19.1h.01" {SB}/>',
    "pin":     f'<path d="M17.6 20.2s-2-1.9-2-3.2a2 2 0 0 1 4 0c0 1.3-2 3.2-2 3.2z" {SB}/><circle cx="17.6" cy="16.9" r=".5" fill="#d4d4d4" stroke="none"/>',
    "link":    f'<path d="M16.9 18.3l1.4-1.4M16 17.2l-.5.5a1.3 1.3 0 0 0 1.9 1.9l.5-.5M19.2 18l.5-.5a1.3 1.3 0 0 0-1.9-1.9l-.5.5" {SB}/>',
    "shieldm": f'<path d="M17.6 15.1l-2.1.9v1.5c0 1.3.9 2.2 2.1 2.7 1.2-.5 2.1-1.4 2.1-2.7V16l-2.1-.9z" {SB}/>',
    "stack":   f'<path d="M15.4 16.2h4.4M15.4 17.8h4.4M15.4 19.4h4.4" {SB}/>',
    "person":  f'<circle cx="17.6" cy="16.4" r="1.1" {SB}/><path d="M15.8 19.9c.3-1.2 1-1.8 1.8-1.8s1.5.6 1.8 1.8" {SB}/>',
    "flagm":   f'<path d="M16.2 20.2v-5l3 .9-3 .9" {SB}/>',
}

# suffix → modifier (ordered; first match on the LAST token, then contains)
SUFFIX_MOD = [
    (("builder", "creator", "composer", "generator", "designer", "drafter", "wizard", "planner"), "plus"),
    (("solver", "verifier", "checker", "validator", "prover", "balancer"), "check"),
    (("finder", "search", "lookup", "browser", "explorer", "inspector", "screener", "detector", "selector"), "magnify"),
    (("editor", "writer", "annotator", "redliner", "stamper"), "pencil"),
    (("manager", "engine", "tuner", "config", "adjuster", "organizer", "orchestrator"), "gear"),
    (("viewer", "monitor", "observer", "watch", "dashboard"), "eye"),
    (("tracker", "log", "logger", "recorder", "journal"), "pulse"),
    (("exporter", "export", "publisher", "router", "sender", "packager"), "export"),
    (("converter", "transformer", "translator", "reducer", "processor", "propagator"), "cycle"),
    (("simulator", "sim", "runner", "player", "rehearsal"), "play"),
    (("analyzer", "analysis", "analytics", "stats", "profiler", "scorer", "estimator", "evaluator", "assessor"), "chart"),
    (("matrix", "table", "grid", "spreadsheet"), "grid"),
    (("report", "notes", "memo", "worksheet", "checklist", "list", "roster", "inventory", "workbook"), "lines"),
    (("scheduler", "calendar", "timer", "clock", "deadline"), "clock"),
    (("optimizer", "accelerator", "booster", "maximizer"), "bolt"),
    (("advisor", "guide", "reference", "primer", "glossary", "assistant", "helper"), "question"),
    (("alert", "alerts", "notifier", "alarm", "warning"), "bell"),
    (("calculator", "calc", "counter"), "calcpad"),
    (("mapper", "locator", "delineator", "geocoder", "plotter"), "pin"),
    (("connector", "linker", "integrator", "bridge", "sync"), "link"),
    (("protector", "guard", "shield", "safety", "custody"), "shieldm"),
    (("library", "catalog", "registry", "archive", "bank", "repository", "collection", "vault"), "stack"),
    (("profiler2", "persona", "profile"), "person"),
    (("milestone", "goal", "flagger"), "flagm"),
]


def modifier_for(tool_id):
    rest = tool_id.split(":", 1)[1] if ":" in tool_id else tool_id
    tokens = rest.split("_")
    last = tokens[-1]
    for keys, mod in SUFFIX_MOD:
        if last in keys:
            return mod
    for keys, mod in SUFFIX_MOD:
        if any(k in tokens for k in keys):
            return mod
    return None


# ── Base glyph bodies: extracted from the archetype SVG files on disk ────────
_base_cache = {}
INNER_RE = re.compile(r"<svg[^>]*>(.*)</svg>", re.DOTALL)
VB_RE = re.compile(r'viewBox="0 0 (\d+) (\d+)"')


def base_body(icon_id):
    if icon_id in _base_cache:
        return _base_cache[icon_id]
    fname = "settings.svg" if icon_id == "gear" else f"{icon_id}.svg"
    path = os.path.join(UI_DIR, fname)
    try:
        raw = open(path, encoding="utf-8").read()
    except OSError:
        raw = open(os.path.join(UI_DIR, "settings.svg"), encoding="utf-8").read()
    inner = INNER_RE.search(raw).group(1).strip()
    vb = VB_RE.search(raw)
    size = int(vb.group(1)) if vb else 24
    if size != 24:  # a few legacy glyphs are 16×16
        inner = f'<g transform="scale({24 / size:.4f})">{inner}</g>'
    _base_cache[icon_id] = inner
    return inner


# Badges that merely restate a base archetype — when both would encode the
# same concept (tracker → clipboard base + pulse badge), the icon wastes its
# uniqueness budget. In that case re-derive the base from the id WITHOUT its
# action suffix so the base carries the DOMAIN and the badge carries the
# ACTION (the actual point of the grammar).
MOD_REDUNDANT_BASE = {
    "pulse": {"clipboard"}, "calcpad": {"calc"}, "clock": {"clock", "calendar"},
    "plus": {"calendar"}, "magnify": {"search"}, "eye": {"eye", "radar"},
    "chart": {"chart-line", "chart-bar", "gauge"}, "pencil": {"pen"},
    "pin": {"map"}, "stack": {"database"}, "lines": {"list", "clipboard"},
    "check": {"checklist"}, "bell": {"bell"}, "gear": {"gears"},
    "link": {"link"}, "shieldm": {"shield"}, "person": {"user"},
    "question": {"book"}, "export": {"export"}, "play": {"atom"},
}


# Per-id base overrides — the handful of wired siblings that would otherwise
# share one glyph inside a single section.
ID_BASE = {
    "insert:localscript": "user",      # client-side script
    "insert:modulescript": "package",  # reusable module
}

# Token → base vocabulary: lets a button differentiate by its OWN
# distinguishing word instead of collapsing into its section-mates'
# domain+action combo (the source of the 150+ same-section twins).
# Scanned right-to-left over the id's tokens (action suffix excluded).
TOKEN_BASE = {
    # ── AI mode (2026-08-20): ML/RL vocabulary, so composed glyphs read as
    # their subject rather than falling back to the prefix archetype.
    "reward": "trophy", "policy": "robot", "rollout": "robot",
    "episode": "gamepad", "env": "gamepad", "agent": "robot",
    "checkpoint": "save", "shard": "package", "parquet": "package",
    "corpus": "database", "dataset": "database", "dataloader": "database",
    "card": "document", "manifest": "certificate", "license": "certificate",
    "rights": "certificate", "merkle": "fingerprint", "hash": "fingerprint",
    "lineage": "route", "provenance": "route", "payout": "coin",
    "contributor": "users", "tokenizer": "sigma", "embedding": "sigma",
    "gradient": "trend-up", "grad": "trend-up", "loss": "chart-line",
    "benchmark": "trophy", "leaderboard": "trophy", "eval": "gauge",
    "metric": "gauge", "label": "tag", "annotate": "tag",
    "split": "separate", "leakage": "shield", "holdout": "shield",
    "sensor": "antenna", "telemetry": "antenna", "harvest": "import",
    "model": "brain", "adapter": "puzzle", "sweep": "radar",
    "run": "play", "hub": "rocket", "bias": "scales",
    "balance": "scales", "income": "trend-up", "cash": "coin",
    "statement": "document", "ratio": "chart-pie", "variance": "chart-bar",
    "accrual": "calendar", "matching": "link", "reconciliation": "link",
    "interview": "users", "pain": "heart-pulse", "proposition": "lightbulb",
    "persona": "user", "journey": "route", "canvas": "kanban",
    "influence": "wave", "modal": "wave", "seismic": "wave",
    "vibration": "wave", "weld": "fire", "connection": "link",
    "timber": "tree", "masonry": "block", "concrete": "block",
    "steel": "layers", "rebar": "array-grid", "shear": "layers",
    "boring": "drop", "sample": "flask", "piezometer": "gauge",
    "settlement": "layers", "pile": "column", "footing": "block",
    "slope": "layers", "liquefaction": "water", "surcharge": "layers",
    "curve": "chart-line", "superelevation": "road", "sight": "eye",
    "barrier": "shield", "roundabout": "lifebuoy", "signal": "lightning",
    "phasing": "clock", "crash": "bell",
    "hydrograph": "chart-line", "rainfall": "drop", "watershed": "map",
    "culvert": "cylinder", "weir": "water", "pump": "gears",
    "levee": "layers", "reservoir": "database", "crew": "users",
    "equipment": "truck", "productivity": "gauge", "risk": "shield",
    "weather": "sun", "contingency": "shield", "submittal": "envelope",
    "punch": "checklist", "warranty": "certificate", "photo": "camera",
    "rfi": "envelope", "habitat": "leaf", "wetland": "water",
    "species": "dna", "noise": "wave", "plume": "funnel",
    "excavation": "build", "bioretention": "leaf", "clarifier": "cylinder",
    "disinfection": "drop", "gnss": "satellite", "baseline": "ruler",
    "datum": "compass", "contour": "map", "surface": "layers",
    "volume": "package", "parcel": "map", "boundary": "ruler",
    "orthophoto": "image", "point": "map",
    "plat": "document", "title": "document", "control": "crosshair",
    "trench": "layers", "thrust": "block", "clash": "bell",
    "locate": "map", "strike": "bell", "asset": "database",
    "renewal": "history", "armor": "shield", "overtopping": "water",
    "breakwater": "block", "marina": "anchor", "channel": "water",
    "dune": "layers", "erosion": "water", "vulnerability": "shield",
    "adaptation": "route", "monitoring": "eye", "storm": "sun",
    "hearing": "users", "objection": "bell", "opening": "megaphone",
    "witness": "user", "exhibit": "tag", "verdict": "gavel",
    "settlement2": "handshake", "caucus": "users", "impasse": "puzzle",
    "shuttle": "route", "award": "trophy", "annulment": "column",
    "asset2": "database", "costs": "coin", "dissent": "pen",
    "brief": "document", "citation": "link", "authority": "column",
    "jurisdiction": "map", "clause": "document", "engagement": "handshake",
    "conflict": "bell", "wall": "block", "matter": "briefcase",
    "diligence": "checklist", "disclosure": "envelope", "synergy": "puzzle",
    "proxy": "envelope", "bylaws": "scroll", "antitrust": "scales",
    "vision": "eye", "mission2": "flag", "okr": "crosshair",
    "roadmap": "route", "initiative": "flag", "swot": "kanban",
    "pestel": "globe", "positioning": "map", "war": "crosshair",
    "quarterly": "calendar", "resource": "package", "tradeoff": "scales",
    "npv": "trend-up", "wacc": "chart-pie", "irr": "trend-up",
    "dcf": "chart-line", "amortization": "calendar", "credit": "gauge",
    "runway": "clock", "unit": "calc", "tam": "chart-pie",
    "landing": "presentation", "pricing": "tag", "demo": "presentation",
    "kinematics": "gears", "friction": "layers", "projectile": "crosshair",
    "energy": "lightning", "momentum": "trend-up", "circuit": "lightning",
    "field": "magnet", "maxwell": "wave", "lagrangian": "sigma",
    "dimensional": "ruler", "sensor": "antenna", "error": "bell",
    "optical": "eye", "oscilloscope": "wave", "apparatus": "flask",
    "n": "network", "thermo": "thermometer", "qualifying": "certificate",
    "vitals": "heart-pulse", "wound": "heart-pulse", "fall": "shield",
    "triage": "funnel", "med": "pill", "isbar": "envelope",
    "braden": "gauge", "dosage": "calc", "iv": "drop",
    "pressure": "gauge", "code": "bell", "skills": "checklist",
    # ── pair-driven batch (2026-07-23): entries chosen from measured
    # same-section collisions, not guesswork ──
    "polar": "chart-line", "tunnel": "funnel", "flutter": "wave",
    "trim": "gauge", "avenue": "route", "ipb": "map", "fire": "fire",
    "air": "plane", "change": "swap", "claims": "document", "bid": "coin",
    "quantity": "calc", "shoreline": "map", "transport": "route",
    "improvement": "trend-up", "material": "layers", "energy": "lightning",
    "pfd": "flowchart", "pid": "gauge", "valve": "plug",
    "dispersion": "funnel", "vle": "thermometer", "declension": "list",
    "conjugation": "swap", "parsing": "tree", "accent": "wave",
    "etymology": "tree", "recitation": "megaphone", "fluency": "gauge",
    "reading": "book", "criteria": "checklist", "flag": "flag",
    "morbidity": "heart-pulse", "soap": "clipboard", "progress": "trend-up",
    "signature": "pen", "condition": "gauge", "fiduciary": "handshake",
    "diversion": "route", "offer": "handshake", "services": "users",
    "sorting": "array-linear", "recursion": "tree", "algorithm": "flowchart",
    "katas": "keyboard", "closure": "link", "resection": "crosshair",
    "plat": "map", "title": "certificate", "modal": "sigma",
    "warrant": "stamp", "los": "road", "trench": "build",
    "bmp": "leaf", "quality": "gauge", "scs": "layers",
    "concentration": "funnel", "rational": "sigma", "fiscal": "bank",
    "monetary": "coin", "supply": "scales", "consumer": "user",
    "law": "scales", "node": "network", "autorouter": "robot",
    "pcb": "array-grid",
}


def domain_base(tool_id, archetype, mod):
    per_id = ID_BASE.get(tool_id)
    if per_id:
        return per_id
    prefix, _, rest = tool_id.partition(":")
    tokens = rest.split("_")
    scan = tokens[:-1] if mod is not None and len(tokens) > 1 else tokens
    for t in scan:
        hit = TOKEN_BASE.get(t)
        if hit and hit != archetype:
            return hit
    # Literal fallback: a token that IS an icon file name (network, anchor,
    # camera, …) names its own base.
    for t in scan:
        if t != archetype and os.path.exists(os.path.join(UI_DIR, t + ".svg")):
            return t
    if mod is None or archetype not in MOD_REDUNDANT_BASE.get(mod, ()):
        return archetype
    if len(tokens) > 1:
        shorter = prefix + ":" + "_".join(tokens[:-1])
        cand = gtm.icon_for(shorter)
        if cand != "settings" and cand not in MOD_REDUNDANT_BASE.get(mod, ()):
            return cand
    return gtm.PREFIX_ICON.get(prefix, archetype)


def compose(tool_id, archetype):
    mod = modifier_for(tool_id)
    archetype = domain_base(tool_id, archetype, mod)
    base = base_body(archetype)
    if mod is None:
        return HDR + f'<g transform="translate(0.96 0.96) scale(0.92)">{base}</g>' + "\n</svg>\n"
    mask_id = "kb"
    return (
        HDR
        + f'<mask id="{mask_id}"><rect x="0" y="0" width="24" height="24" fill="white"/>'
        + f'<circle cx="17.6" cy="17.6" r="6.1" fill="black"/></mask>\n'
        + f'<g mask="url(#{mask_id})"><g transform="scale(0.82)">{base}</g></g>\n'
        + f'<circle cx="17.6" cy="17.6" r="4.5" {S.replace("1.8", "1.5")}/>\n'
        + MODS[mod]
        + "\n</svg>\n"
    )


def sanitize(tool_id):
    return tool_id.replace(":", "__").replace("-", "_")


def main():
    # Collect every tool id + its archetype exactly as tool_metadata does.
    import glob
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib
    ids = set()
    for path in sorted(glob.glob(gtm.MODES_GLOB)):
        d = tomllib.load(open(path, "rb"))
        for key in ("tabs", "submode_tabs"):
            for tab in d.get(key, []):
                for sec in tab.get("sections", []):
                    ids.update(sec.get("tools", []))

    import xml.dom.minidom
    from collections import Counter
    combo = Counter()
    n = 0
    for tid in sorted(ids):
        if tid in gtm.WIRED:
            arche = gtm.WIRED[tid][2]
        else:
            arche = gtm.icon_for(tid)
        svg = compose(tid, arche)
        xml.dom.minidom.parseString(svg)
        with open(os.path.join(OUT_DIR, sanitize(tid) + ".svg"), "w", encoding="utf-8", newline="\n") as f:
            f.write(svg)
        combo[(arche, modifier_for(tid))] += 1
        n += 1

    uniq = sum(1 for c in combo.values() if c == 1)
    print(f"wrote {n} composed icons to {OUT_DIR}")
    print(f"distinct base+badge combos: {len(combo)} ({uniq} used exactly once)")
    print("most shared combos:", combo.most_common(6))


if __name__ == "__main__":
    main()
