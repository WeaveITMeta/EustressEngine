# -*- coding: utf-8 -*-
"""Generates the **Tucson** Universe: a civic temple as the hub Space, one Space
per simulation in the civic program, portals between them, and a mind-map
canopy over the temple.

    python scripts/gen_tucson_universe.py [--root <workspace>] [--force]
        [--clobber] [--sync] [--only <Space>]...

Layout produced (under `Documents/Eustress/` unless `--root`/`EUSTRESS_WORKSPACE`):

    Tucson/
      Spaces/
        Temple/                 <- the hub: council chamber, 12 need halls, 16 portals
        S01-Budget-Twin/        <- one Space per simulation, each with a return portal
        ... 16 total ...

The temple is laid out the way a city government actually works. At the centre
the Mayor and the six Ward seats sit around the Public Ledger, facing a
public-comment lectern with the public gallery behind it. Twelve halls radiate
from that chamber, one per BASIC NEED a municipality provides for, and each hall
is the department that answers for its need: the obligation carved at the
entrance, the public services as counters down the hall, the simulation that
measures it posted on a notice board, how the city visibly fails at it on a
stele at the end, and a portal into that simulation. The government is not a
separate building: the halls ARE the disciplines (docs/architecture/
GOVERNMENT_MODE.md section 4.2 maps need to discipline).

## Determinism

This generator is deliberately reproducible: no wall clock, no RNG, no host
paths in content. Instance uuids are `md5(<instance path within the Space>)`
and every `last_modified` is the fixed [`GENERATED_AT`] stamp, so re-running
produces byte-identical output. That is the same property S04 (the determinism
harness) demands of every model in the program, and the scaffolding should not
be exempt from it.

## Existing and open Spaces

Refuses to touch an existing Space unless `--force`, and refuses to overwrite
hand edits unless `--clobber`. `--sync` applies a rebuild to a Space the engine
has OPEN: it rewrites only files whose bytes changed, removes only instances
this generator created, and never touches engine state, so the running engine
hot-reloads the difference instead of reopening the Space.
"""
import argparse
import hashlib
import json
import math
import os
import shutil
import sys
import time
from io import open as io_open

# Fixed stamp; see the Determinism note above.
GENERATED_AT = "2026-07-26T00:00:00+00:00"
AUTHOR = "Eustress"

UNIVERSE = "Tucson"
HUB = "Temple"

# ── Geometry (world units, metres) ───────────────────────────────────────────
# The plan, from the centre out:
#   r 0-24     council dais: Public Ledger, council table, public-comment lectern
#   r 26.6     twelve need beacons, one on each hall's bearing
#   r 34-47    public gallery, in the sector behind the lectern
#   r 62       rotunda colonnade: twelve Corinthian columns BETWEEN the hall axes
#   r 88       each hall's name arch; r 96 is where a returning camera lands
#   r 101      the obligation stele
#   r 108-228  the roofed hall: service counters, then the simulation board
#   r 240      the "fails when" stele
#   r 258      the portal front(s) into that need's simulations
# Twelve halls need arc room at their mouths: at r 88 each gets 46 units of arc
# for a 26-wide arch, and neighbouring hall roofs stay ~14 apart at r 108.
FLOOR_Y = 4.0           # top of the platform
WALK_Y = FLOOR_Y + 1.0  # top of every floor slab: the level you walk on
STYLO_R = 292.0         # outer platform
STEP_R = 284.0          # upper step
ROT_FLOOR_R = 80.0      # rotunda floor
ROT_COL_R = 62.0        # rotunda colonnade radius
# Twelve, at 15 + 30k degrees. Twenty-four put every other column squarely on a
# hall axis, blocking the view down it; those were deleted by hand, and the
# plan now starts from the arrangement that survived.
ROT_COLS = 12
ROT_COL_H = 52.0
ROT_COL_D = 5.2         # Corinthian: about ten diameters tall
CHAMBER_R = 30.0        # the council floor inlay
DAIS_TOP = WALK_Y + 1.6
ARCH_R = 88.0
ARRIVAL_R = 96.0        # contract with the sim Spaces' return portals: keep
OBLIGATION_R = 101.0
HALL_R0 = 108.0         # first column pair = the hall's front
HALL_BAYS = 6
HALL_BAY = 24.0
HALL_R1 = HALL_R0 + (HALL_BAYS - 1) * HALL_BAY   # 228
HALL_HALF_W = 17.0
HALL_COL_H = 30.0
HALL_COL_D = 3.0
ROOF_PITCH = 16.0
FAIL_R = 240.0
PORTAL_R = 258.0
PORTAL_COL_H = 34.0
PORTAL_COL_D = 3.4
PORTAL_PITCH = 18.0
PORTAL_HALF_W = 15.5
GARDEN_R = 186.0

# Mind-map canopy rings: (y, radius, node_diameter). The sim ring sits just
# inside the portal line so each dropline falls almost vertically onto its own
# portal, and the root sits directly over the Public Ledger.
CANOPY_ROOT = (300.0, 0.0, 17.0)
CANOPY_NEED = (242.0, 122.0, 12.0)
CANOPY_DISC = (190.0, 196.0, 9.5)
CANOPY_SIM = (150.0, 250.0, 7.5)

# ── Palette: warm desert stone, terracotta, copper-gold ──────────────────────
STONE = (163, 162, 165)        # institutional grey, still used by the sim Spaces
TRAVERTINE = (226, 212, 186)
HALL_FLOOR = (214, 198, 170)
CHAMBER_STONE = (146, 122, 98)
DAIS_STONE = (236, 226, 204)
SANDSTONE = (196, 160, 122)
STEP_STONE = (210, 178, 140)
COL_STONE = (239, 233, 219)
LEAF_STONE = (214, 197, 162)   # a shade darker, so the capital reads as carved
ENTAB = (231, 222, 201)
FRIEZE = (208, 192, 163)
CORNICE = (244, 239, 226)
ARCH_STONE = (216, 200, 170)
TABLET = (240, 232, 214)
SCREEN_STONE = (228, 215, 190)
TERRACOTTA = (174, 88, 58)
RIDGE_TILE = (138, 66, 44)
BRONZE = (132, 98, 58)
GOLD = (214, 176, 72)
WALNUT = (96, 62, 38)
OAK = (158, 116, 72)
BENCH_WOOD = (142, 100, 62)
OXBLOOD = (116, 34, 38)
SLATE_DARK = (52, 56, 64)
NOTICE = (238, 230, 212)
FAIL_RED = (150, 82, 74)
FAIL_BASE = (104, 58, 52)
PARCHMENT = (246, 234, 204)
BEAM = (255, 236, 196)
LAMP = (255, 222, 170)
LAMP_LIGHT = (255, 214, 160)
VOTE_AMBER = (255, 176, 40)
SAGUARO = (86, 122, 72)
BARREL = (104, 132, 70)
GRAVEL = (188, 150, 110)
BOULDER = (128, 104, 86)

# Label text and plate. The plate is soft (about half opaque) and hugs the text,
# so a sign reads as lettering on the thing it names rather than a dark box.
TEXT_CREAM = (0.97, 0.94, 0.86)
PLATE = (0.07, 0.06, 0.05, 0.55)

# ── The programme ────────────────────────────────────────────────────────────
# Each simulation: id -> (space folder, display title, one-line falsifier).
# The falsifier is posted beside the portal because a model that cannot
# contradict its operator is a prop (GOVERNMENT_MODE.md section 2).
SIMS = {
    "S01": ("S01-Budget-Twin", "Budget Digital Twin & Zero-Based Reconstruction",
            "If the realizable cut lands under 30%, the platform's headline number is wrong."),
    "S02": ("S02-Permit-Queue", "Permit Throughput Queue",
            "If a statutory window or outside agency binds, automation cannot reach 30 days."),
    "S03": ("S03-Response-Isochrones", "911 Response-Time Isochrones",
            "If four minutes requires new stations, this is a spending proposal, not an efficiency one."),
    "S04": ("S04-Determinism-Harness", "Determinism & Reproducibility Harness",
            "Any nondeterminism found invalidates every number downstream until it is fixed."),
    "S05": ("S05-Automation-Displacement", "Automation Displacement & Transition",
            "If attrition-only converges to the same savings, layoffs buy speed, not money."),
    "S06": ("S06-Tax-Incidence", "Tax Incidence & Local Revenue Response",
            "If a residency freeze shifts burden onto renters, the plank needs redesign."),
    "S07": ("S07-Zoning-Buildout", "Zoning Liberalization & Housing Supply",
            "If construction cost or water allocation binds, deregulation is necessary but not sufficient."),
    "S08": ("S08-Procurement-Competition", "Procurement Competition & Award Integrity",
            "If most no-bid awards are sole-source justified, the addressable savings pool shrinks."),
    "S09": ("S09-Patrol-Deployment", "Patrol Deployment Optimization",
            "If the incident surface is too diffuse to beat uniform coverage, redeployment is not the lever."),
    "S10": ("S10-Transit-Farebox", "Transit Fare vs. Farebox Recovery",
            "If collection costs exceed recovery, the free bus is the fiscally conservative position."),
    "S11": ("S11-Homelessness-Flow", "Homelessness Stock-and-Flow",
            "If inflow dominates exit capacity, a 50% reduction in twelve months is unreachable."),
    "S12": ("S12-Cost-Per-Exit", "Enforcement vs. Treatment Cost-per-Exit",
            "If enforcement costs more per sustained exit, argue public order, not thrift."),
    "S13": ("S13-Regression-Guard", "Service-Level Regression Guard",
            "This simulation exists to fire. If it never rejects a cut, it is miscalibrated."),
    "S14": ("S14-Ward-Distribution", "Ward-Level Distributional Explorer",
            "If benefits concentrate in high-income wards and costs in low-income wards, that is a finding."),
    "S15": ("S15-Water-Portfolio", "Water Portfolio & Assured Supply",
            "If assured supply fails a drought scenario, growth policy is the binding constraint."),
    "S16": ("S16-Food-Access", "Food Access & Desert Retail",
            "If travel time rather than store count binds access, this is a transit problem."),
}

# The twelve halls. `angle` is degrees clockwise from +Z (the hall axis is
# (sin a, 0, cos a)). `sims` are the portals at that hall's end.
#
# `inscription` is the civic OBLIGATION the need creates, carved on the stele at
# the hall's entrance. `failure_mode` is how a city visibly fails at it, carved
# on the stele at the hall's END, so the last thing read before stepping through
# the portal is what failure looks like, and the simulation beyond measures
# exactly that. `accent` is the hall's wayfinding colour: twelve
# maximally-separated hues, because with radial symmetry colour is the only cue
# telling you which hall you are in.
WINGS = [
    dict(need="Water", angle=0, accent="#38bdf8", discipline="Public Works & Utilities",
         inscription="Water reaches every tap. Every gallon is accounted.",
         provisions=["Treatment Plants", "Distribution Mains", "Meters and Billing", "Drought Reserve"],
         failure_mode="Taps run dry or the water is unsafe, and the shortfall appears as "
                      "unaccounted-for volume no one can explain.",
         sims=["S15"]),
    dict(need="Shelter", angle=30, accent="#fb923c", discipline="Permitting & Land Use",
         inscription="The city shall not obstruct lawful shelter.",
         provisions=["Zoning Code", "Building Permits", "Inspections", "Code Enforcement"],
         failure_mode="Housing costs outrun wages while applications sit in review, and units "
                      "that were legal to build never get built.",
         sims=["S07", "S11"]),
    dict(need="Food", angle=60, accent="#a3e635", discipline="Health & Human Services",
         inscription="Food sold here is inspected. Hunger is counted.",
         provisions=["Restaurant Inspection", "Food Handler Cards", "Congregate Meals", "Market Permits"],
         failure_mode="Inspections lapse until an outbreak announces them, and assistance lines "
                      "grow faster than the programs behind them.",
         sims=["S16"]),
    dict(need="Health", angle=90, accent="#f472b6", discipline="Health & Human Services",
         inscription="Disease that spreads in public is public business.",
         provisions=["Immunization Clinics", "Vector Control", "Sanitation Code", "Emergency Medical Service"],
         failure_mode="Preventable illness spreads through public facilities and the city learns "
                      "of it from hospitals rather than from its own data.",
         sims=["S12"]),
    dict(need="Safety", angle=120, accent="#f87171", discipline="Public Safety Administration",
         inscription="Force is answerable. Response times are measured.",
         provisions=["Patrol Coverage", "Fire Stations", "911 Dispatch", "Alternative Response"],
         failure_mode="Response times lengthen in the neighbourhoods carrying the most calls, and "
                      "force is used more often with less explanation.",
         sims=["S03", "S09"]),
    dict(need="Movement", angle=150, accent="#fbbf24", discipline="Public Works & Utilities",
         inscription="Every street the city owns shall remain passable.",
         provisions=["Street Pavement", "Traffic Signals", "Sidewalks and Curbs", "Bus Service"],
         failure_mode="Pavement condition declines faster than it is repaired, until deferred "
                      "maintenance costs more than reconstruction would have.",
         sims=["S10"]),
    dict(need="Livelihood", angle=180, accent="#34d399", discipline="Procurement & Contracts",
         inscription="Public work is bid openly and paid honestly.",
         provisions=["Open Solicitations", "Business Licensing", "Contract Awards", "Prompt Payment"],
         failure_mode="Awards concentrate on the same vendors without competition, and local firms "
                      "stop bidding because bidding never wins.",
         sims=["S08", "S02"]),
    dict(need="Voice", angle=210, accent="#818cf8", discipline="Council (Ward)",
         inscription="No ordinance precedes the hearing it requires.",
         provisions=["Public Comment", "Meeting Notice", "Ward Offices", "Published Agendas"],
         failure_mode="Decisions arrive already made, with comment scheduled after the vote is "
                      "arithmetically settled.",
         sims=["S14"]),
    dict(need="Record", angle=240, accent="#e2dcbc", discipline="Clerk & Elections",
         inscription="What the city writes down, the city discloses.",
         provisions=["Records Retention", "Public Records Requests", "Meeting Minutes", "Campaign Finance Filings"],
         failure_mode="Requested records go missing, arrive redacted past usefulness, or cannot be "
                      "shown to be unaltered since publication.",
         sims=["S04"]),
    dict(need="Accounting", angle=270, accent="#facc15", discipline="Budget & Finance",
         inscription="Every dollar taken is a dollar explained.",
         provisions=["Adopted Budget", "Annual Financial Report", "Checkbook Register", "Fiscal Notes"],
         failure_mode="Adopted budget and actual spending diverge, and no headline number can be "
                      "traced back to a line item.",
         sims=["S01", "S06"]),
    dict(need="Stewardship", angle=300, accent="#a78bfa", discipline="Administration",
         inscription="What the city owns, the city maintains.",
         provisions=["Asset Registry", "Preventive Maintenance", "Capital Plan", "Reserve Funds"],
         failure_mode="Assets are run to failure while new capital projects are announced, shifting "
                      "the bill onto whoever holds the seat next.",
         sims=["S05"]),
    dict(need="Scrutiny", angle=330, accent="#94a3b8", discipline="Audit & Inspector General",
         inscription="Findings are published, favourable or not.",
         provisions=["Audit Plan", "Findings Register", "Management Responses", "Whistleblower Intake"],
         failure_mode="Findings are softened before release, recommendations are closed without "
                      "being implemented, and the audit function goes quietly unfunded.",
         sims=["S13"]),
]

# The canopy shows a short form. The full discipline name is the manifest's
# business (modes/government.toml), not the map's.
DISC_SHORT = {
    "Public Works & Utilities": "Public Works",
    "Permitting & Land Use": "Permitting",
    "Health & Human Services": "Human Services",
    "Public Safety Administration": "Public Safety",
    "Procurement & Contracts": "Procurement",
    "Council (Ward)": "Council",
    "Clerk & Elections": "Clerk",
    "Budget & Finance": "Budget",
    "Administration": "Administration",
    "Audit & Inspector General": "Audit",
}

# The council. Tucson is governed by a Mayor and six Ward council members. The
# horseshoe table opens toward the Voice hall, so the Mayor faces the
# public-comment lectern across the ledger and the gallery sits behind whoever
# is speaking.
LEDGER_TITLE = "PUBLIC LEDGER"
LEDGER_RULE = "The record is public. Every number answers to its source."
LECTERN_BEARING = 210.0
COUNCIL_TABLE_R = 12.5
COUNCIL_OPENING = 32.0     # half-angle of the horseshoe's open side
SEATS = [
    ("Mayor", 30.0, "Mayor of Tucson"),
    ("Ward 1", 70.0, "Council Member, Ward 1"),
    ("Ward 2", 110.0, "Council Member, Ward 2"),
    ("Ward 3", 150.0, "Council Member, Ward 3"),
    ("Ward 4", 270.0, "Council Member, Ward 4"),
    ("Ward 5", 310.0, "Council Member, Ward 5"),
    ("Ward 6", 350.0, "Council Member, Ward 6"),
]


# ── helpers ──────────────────────────────────────────────────────────────────
def hex_rgb(h):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16))


def mix(a, b, t):
    """Blend colour `a` toward `b` by `t` (0 = a, 1 = b)."""
    return tuple(int(round(a[i] * (1.0 - t) + b[i] * t)) for i in range(3))


def uuid_for(key):
    """Deterministic 32-hex uuid from a stable key (the instance's Space path)."""
    return hashlib.md5(key.encode("utf-8")).hexdigest()


def quat_y(deg):
    """Quaternion [x,y,z,w] rotating about +Y by `deg`."""
    h = math.radians(deg) / 2.0
    return [0.0, math.sin(h), 0.0, math.cos(h)]


def quat_y_to(d):
    """Quaternion taking +Y onto direction `d`: aims a cylinder's long axis."""
    n = math.sqrt(sum(c * c for c in d))
    if n < 1e-9:
        return [0.0, 0.0, 0.0, 1.0]
    dx, dy, dz = (c / n for c in d)
    dot = dy  # dot((0,1,0), d)
    if dot > 0.999999:
        return [0.0, 0.0, 0.0, 1.0]
    if dot < -0.999999:
        return [1.0, 0.0, 0.0, 0.0]  # 180 deg about X
    # axis = cross((0,1,0), d) = (dz, 0, -dx)
    ax, ay, az = dz, 0.0, -dx
    an = math.sqrt(ax * ax + ay * ay + az * az)
    ax, ay, az = ax / an, ay / an, az / an
    ang = math.acos(max(-1.0, min(1.0, dot)))
    s = math.sin(ang / 2.0)
    return [ax * s, ay * s, az * s, math.cos(ang / 2.0)]


def quat_axis_angle(axis, deg):
    """Quaternion for `deg` about an arbitrary unit-ish `axis`."""
    n = math.sqrt(sum(c * c for c in axis))
    if n < 1e-9:
        return [0.0, 0.0, 0.0, 1.0]
    ax, ay, az = (c / n for c in axis)
    h = math.radians(deg) / 2.0
    s = math.sin(h)
    return [ax * s, ay * s, az * s, math.cos(h)]


def quat_mul(a, b):
    """Hamilton product a*b: apply `b` first, then `a`."""
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]


# Upside down: a cone's apex at -Y, its base at +Y. The Corinthian bell.
QUAT_FLIP = [1.0, 0.0, 0.0, 0.0]


def wing_axis(deg):
    a = math.radians(deg)
    return (math.sin(a), 0.0, math.cos(a))


def wing_perp(deg):
    a = math.radians(deg)
    return (math.cos(a), 0.0, -math.sin(a))


def along(deg, r, lateral=0.0, y=0.0):
    """Point at radius `r` along bearing `deg`, offset `lateral` sideways."""
    ax, _, az = wing_axis(deg)
    px, _, pz = wing_perp(deg)
    return [ax * r + px * lateral, y, az * r + pz * lateral]


def f(v):
    """Format a float the way the engine's own writer does: ALWAYS with a
    decimal point. TOML distinguishes `8` (integer) from `8.0` (float), and the
    transform/attribute deserializers want floats; emitting a bare integer for a
    whole number risks `invalid type: integer, expected f32`. Also normalises
    `-0.0` to `0.0` so mirrored geometry hashes identically."""
    s = f"{float(v):.6f}".rstrip("0")
    if s.endswith("."):
        s += "0"
    return "0.0" if s == "-0.0" else s


def arr(v):
    return "[" + ", ".join(f(x) for x in v) + "]"


def iarr(v):
    return "[" + ", ".join(str(int(x)) for x in v) + "]"


def toml_str(s):
    """A TOML basic string."""
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


# ── emitters ─────────────────────────────────────────────────────────────────
def part_toml(key, mesh, pos, scale, color, *, rot=None, material="Marble",
              transparency=0.0, can_collide=False, class_name="Part",
              attributes=None, cast_shadow=True):
    """`can_collide` defaults to FALSE: structural surfaces opt IN.

    Only things you stand on or walk into carry a collider. Decoration
    (leaves, bands, cornices, labels' anchors) does not need one, and at a few
    thousand parts the difference is real."""
    rot = rot or [0.0, 0.0, 0.0, 1.0]
    out = []
    if mesh:
        out += ['[asset]', f'mesh = "parts/{mesh}.glb"', 'scene = "Scene0"', '']
    out += [
        '[transform]',
        f'position = {arr(pos)}',
        f'rotation = {arr(rot)}',
        f'scale = {arr(scale)}',
        '',
        '[properties]',
        f'color = {iarr(color)}',
        f'transparency = {f(transparency)}',
        'anchored = true',
        f'can_collide = {"true" if can_collide else "false"}',
        f'cast_shadow = {"true" if cast_shadow else "false"}',
        'reflectance = 0.0',
        f'material = "{material}"',
        'locked = false',
        'respect_gltf_materials = false',
        '',
    ]
    if attributes:
        out.append('[attributes]')
        for k, v in attributes.items():
            if isinstance(v, str):
                out.append(f'{k} = {toml_str(v)}')
            elif isinstance(v, (list, tuple)):
                out.append(f'{k} = {arr(v)}')
            elif isinstance(v, bool):
                out.append(f'{k} = {"true" if v else "false"}')
            else:
                out.append(f'{k} = {f(v)}')
        out.append('')
    # No `[metadata.created_by]`. `CreatorStamp` requires `public_key` with no
    # serde default, so a partial stamp fails to deserialize `InstanceMetadata`,
    # and because the whole file then fails to parse, the loader falls back to
    # Folder and the geometry silently never renders. Omitting the table is both
    # the fix and the truth: the struct's own doc says the stamp is "absent for
    # entities created offline", and a generator has no signing identity.
    out += [
        '[metadata]',
        f'class_name = "{class_name}"',
        'archivable = true',
        f'last_modified = "{GENERATED_AT}"',
        f'uuid = "{uuid_for(key)}"',
        '',
    ]
    return "\n".join(out)


def light_toml(key, rgb, *, brightness, range_m):
    """A PointLight. As a child of a part it shines from that part's centre."""
    return "\n".join([
        '[light]',
        f'color = {iarr(rgb)}',
        f'brightness = {f(brightness)}',
        f'range = {f(range_m)}',
        'shadows = false',
        'enabled = true',
        '',
        '[metadata]',
        'class_name = "PointLight"',
        'archivable = true',
        f'last_modified = "{GENERATED_AT}"',
        f'uuid = "{uuid_for(key)}"',
        '',
    ])


# ── Labels ───────────────────────────────────────────────────────────────────
# Every label is rasterised into ONE 192 px atlas tile, whatever its world size
# (billboards.rs TILE_W/TILE_H). World size is free: a label's quad is its
# requested size at 50 px per unit. Sharpness is not: text is drawn at its box
# size and then scaled into 192 px, so what a label can hold legibly is fixed by
# the tile, roughly 20 characters a line and 7 or 8 lines. Long prose therefore
# gets a square box and wraps into a block; it never gets a wide strip.
#
# `text_scaled = true` is avoided for signage: its search band has been capped
# at 72 px in some builds, which makes a big label's text SMALLER as the label
# grows. An explicit, fitted `font_size` takes the uncapped path in every build.
SLOT_PX = 192
PX_PER_STUD = 50.0


def fit_text(text, box_px, *, max_lines=2, band_px=None, max_font=None):
    """Largest font (px) whose word-wrapped layout fits a square `box_px` label.

    Returns `(font_px, lines, advance)`. The advance is a deliberately WIDE
    per-character guess for GothamBold (0.74 em for capitals, 0.64 mixed). The
    engine wraps at the label's real width, and greedy wrapping at a width at
    least as large as every guessed line yields the same number of lines or
    fewer. Guessing narrow would do the opposite, and an overflow line is
    clipped, not shown."""
    caps = not any(c.islower() for c in text)
    adv = 0.74 if caps else 0.64
    words = text.split()
    width = box_px * 0.9
    band = min(box_px * 0.94, band_px or box_px)
    top = int(min(box_px, max_font or box_px))
    for fs in range(top, 7, -1):
        cw = adv * fs
        if max(len(wd) for wd in words) * cw > width or 1.4 * fs > band:
            continue
        lines, cur = [], ""
        for wd in words:
            trial = f"{cur} {wd}".strip()
            if len(trial) * cw <= width:
                cur = trial
            else:
                lines.append(cur)
                cur = wd
        lines.append(cur)
        if len(lines) <= max_lines and len(lines) * 1.4 * fs <= band:
            return fs, lines, adv
    return 8, [text], adv


def uniform_font(texts, studs, *, max_lines=1, band=None):
    """The largest font every one of `texts` fits at, so a set of like signs
    (the twelve hall names, say) share one letter height."""
    box = studs * PX_PER_STUD
    band_px = band * PX_PER_STUD if band else None
    return min(fit_text(t, box, max_lines=max_lines, band_px=band_px)[0]
               for t in texts)


def sign_billboard_toml(key, studs, *, max_distance):
    """A square billboard `studs` on a side, centred ON its anchor.

    Zero offset, so which object a label names is never ambiguous, and
    `always_on_top`, because a co-located label is otherwise swallowed by its
    own anchor (`z_index` orders GUI layers, it does not win a depth test).
    `z_index = 2` is just enough to clear its own shape."""
    s = float(studs)
    return "\n".join([
        '[gui]',
        'always_on_top = true',
        f'max_distance = {f(max_distance)}',
        'position = [0.0, 0.0, 0.0, 0.0]',
        f'size = [{f(s)}, 0.0, {f(s)}, 0.0]',
        'units_offset_world_space = [0.0, 0.0, 0.0]',
        'visible = true',
        'z_index = 2',
        '',
        '[transform]',
        'position = [0.0, 0.0, 0.0]',
        'rotation = [0.0, 0.0, 0.0, 1.0]',
        'scale = [1.0, 1.0, 1.0]',
        '',
        '[properties]',
        'anchored = true',
        'can_collide = false',
        'cast_shadow = false',
        f'color = {iarr(STONE)}',
        'locked = false',
        'material = "Plastic"',
        'reflectance = 0.0',
        'transparency = 0.0',
        '',
        '[metadata]',
        'class_name = "BillboardGui"',
        'archivable = true',
        f'last_modified = "{GENERATED_AT}"',
        f'uuid = "{uuid_for(key)}"',
        '',
    ])


def sign_textlabel_toml(text, font_px, lines, adv, box_px, *, rgb, bg=PLATE):
    """A TextLabel whose plate hugs its text, centred in the square billboard.

    The plate is sized from the same wide advance guess `fit_text` wrapped
    with, plus 20%, so the engine's own wrap never needs more lines than the
    plate was sized for."""
    text_w = max(len(line) for line in lines) * adv * font_px
    w = min(1.0, (text_w * 1.2 + 1.2 * font_px) / box_px)
    h = min(1.0, (len(lines) * 1.4 + 0.5) * font_px / box_px)
    x, y = (1.0 - w) / 2.0, (1.0 - h) / 2.0
    return "\n".join([
        '[metadata]',
        'class_name = "TextLabel"',
        'archivable = true',
        '',
        '[gui]',
        'anchor_point = [0.0, 0.0]',
        f'position = [{f(x)}, 0.0, {f(y)}, 0.0]',
        f'size = [{f(w)}, 0.0, {f(h)}, 0.0]',
        f'background_color = {arr(bg)}',
        'border_size = 0.0',
        'visible = true',
        'z_index = 6',
        '',
        '[text]',
        f'text = {toml_str(text)}',
        f'text_color = [{f(rgb[0])}, {f(rgb[1])}, {f(rgb[2])}, 1.0]',
        f'font_size = {int(font_px)}',
        'font = "GothamBold"',
        'text_scaled = false',
        'text_wrapped = true',
        'text_x_alignment = "center"',
        'text_y_alignment = "center"',
        '',
    ])


def billboard_toml(key, px, *, y_offset=0.0, max_distance=160.0, z=6):
    """Legacy one-slot label (the sim Spaces still use it): a 192 px square
    sitting exactly on its anchor, `always_on_top`, `z_index = 2`."""
    w = h = float(SLOT_PX)
    _ = (y_offset, z, px)
    return "\n".join([
        '[gui]',
        'always_on_top = true',
        f'max_distance = {f(max_distance)}',
        'position = [0, 0, 0, 0]',
        f'size = [0, {f(w)}, 0, {f(h)}]',
        'units_offset_world_space = [0, 0, 0]',
        'visible = true',
        'z_index = 2',
        '',
        '[transform]',
        'position = [0, 0, 0]',
        'rotation = [0, 0, 0, 1]',
        'scale = [1.0, 1.0, 1.0]',
        '',
        '[properties]',
        'anchored = true',
        'can_collide = false',
        'cast_shadow = false',
        f'color = {iarr(STONE)}',
        'locked = false',
        'material = "Plastic"',
        'reflectance = 0.0',
        'transparency = 0.0',
        '',
        '[metadata]',
        'class_name = "BillboardGui"',
        'archivable = true',
        f'last_modified = "{GENERATED_AT}"',
        f'uuid = "{uuid_for(key)}"',
        '',
    ])


def textlabel_toml(text, *, rgb=(1.0, 1.0, 1.0), size=28, y=0.0, h=1.0,
                   bg=(0.06, 0.07, 0.10, 0.72), font="GothamBold"):
    """Legacy full-slot TextLabel for `billboard_toml`."""
    return "\n".join([
        '[metadata]',
        'class_name = "TextLabel"',
        'archivable = true',
        '',
        '[gui]',
        'anchor_point = [0.0, 0.0]',
        f'position = [0.0, 0.0, {f(y)}, 0.0]',
        f'size = [1.0, 0.0, {f(h)}, 0.0]',
        f'background_color = {arr(bg)}',
        'border_size = 0.0',
        'visible = true',
        'z_index = 6',
        '',
        '[text]',
        f'text = {toml_str(text)}',
        f'text_color = [{f(rgb[0])}, {f(rgb[1])}, {f(rgb[2])}, 1.0]',
        f'font_size = {int(size)}',
        f'font = "{font}"',
        'text_scaled = true',
        'text_x_alignment = "center"',
        'text_y_alignment = "center"',
        '',
    ])


# Pause between removing stale files and writing new ones, so the engine's
# watcher despawns the old instances (and releases their label slots) before
# the new ones arrive.
SYNC_SETTLE_SECS = 2.0


class SpaceWriter:
    """Accumulates instance folders for one Space, then flushes to disk."""

    def __init__(self, root, name):
        self.root = root
        self.name = name
        self.files = {}   # relative path -> text
        self.notes = []   # legibility warnings, printed at flush

    def instance(self, path, text):
        self.files[f"Workspace/{path}/_instance.toml"] = text

    def part(self, path, mesh, pos, scale, color, **kw):
        self.instance(path, part_toml(f"{self.name}/{path}", mesh, pos, scale, color, **kw))

    def light(self, path, rgb, *, brightness, range_m):
        self.instance(path, light_toml(f"{self.name}/{path}", rgb,
                                       brightness=brightness, range_m=range_m))

    def label(self, parent, text, *, px=(SLOT_PX, 48), y_offset=3.0,
              rgb=(1.0, 1.0, 1.0), font_size=28, max_distance=160.0,
              name="Label"):
        """Legacy one-slot label; see `billboard_toml`."""
        key = f"{self.name}/{parent}/{name}"
        self.files[f"Workspace/{parent}/{name}/_instance.toml"] = billboard_toml(
            key, px, y_offset=y_offset, max_distance=max_distance)
        self.files[f"Workspace/{parent}/{name}/{name}.textlabel.toml"] = textlabel_toml(
            text, rgb=rgb, size=font_size)

    def sign(self, parent, text, *, studs, max_lines=2, band=None,
             rgb=TEXT_CREAM, max_distance=120.0, name="Label", max_font=None,
             bg=PLATE):
        """Letter `text` onto `parent`: a square label `studs` wide, centred on
        the part, text fitted to at most `max_lines` lines within `band` studs
        of height."""
        box = studs * PX_PER_STUD
        fs, lines, adv = fit_text(text, box, max_lines=max_lines,
                                  band_px=band * PX_PER_STUD if band else None,
                                  max_font=max_font)
        raster = fs * min(SLOT_PX / box, 8.0)
        if raster < 11.0 or len(lines) > max_lines:
            self.notes.append(f"{parent}: {raster:.1f} px glyphs, {len(lines)} lines: {text!r}")
        key = f"{self.name}/{parent}/{name}"
        self.files[f"Workspace/{parent}/{name}/_instance.toml"] = sign_billboard_toml(
            key, studs, max_distance=max_distance)
        self.files[f"Workspace/{parent}/{name}/{name}.textlabel.toml"] = sign_textlabel_toml(
            text, fs, lines, adv, box, rgb=rgb, bg=bg)

    def corinthian_column(self, path, centre, y0, height, d, *, facing_deg=0.0,
                          full=True):
        """A CORINTHIAN column from the primitives that exist.

        Anatomy, bottom to top:
          plinth    square block
          base      Attic base: two tori (full) or one (lite)
          shaft     slender, about ten diameters to the whole order; the full
                    version carries eight raised fillets that read as fluting
          bell      the kalathos: an upturned cone rising out of the shaft top,
                    flaring to 1.3 diameters. The tall bell is what separates
                    Corinthian from Ionic and Doric, whose capitals are squat
          leaves    acanthus: two tiers of four, tilted outward (full); one tier
                    of four on the diagonals, curling toward the abacus corners
                    (lite)
          helices   four small scrolls under the abacus corners (full)
          abacus    thin square slab the entablature sits on

        `height` is the whole column, plinth to abacus top. Returns that top.
        Full costs 27 parts, lite 9, so the lite order carries the long
        colonnades and the full order the rotunda, where you stand close."""
        cx, _, cz = centre
        yaw = quat_y(facing_deg)
        h_plinth = 0.32 * d
        h_base = 0.30 * d
        h_bell = 1.0 * d
        h_abac = 0.16 * d
        h_shaft = height - h_plinth - h_base - h_bell - h_abac
        d_shaft = 0.94 * d
        y = y0

        self.part(f"{path}/Plinth", "block", [cx, y + h_plinth / 2, cz],
                  [1.5 * d, h_plinth, 1.5 * d], COL_STONE, rot=yaw)
        y += h_plinth
        if full:
            self.part(f"{path}/TorusLower", "cylinder", [cx, y + 0.1 * d, cz],
                      [1.38 * d, 0.2 * d, 1.38 * d], COL_STONE)
            self.part(f"{path}/TorusUpper", "cylinder", [cx, y + 0.25 * d, cz],
                      [1.16 * d, 0.1 * d, 1.16 * d], COL_STONE)
        else:
            self.part(f"{path}/Base", "cylinder", [cx, y + h_base / 2, cz],
                      [1.3 * d, h_base, 1.3 * d], COL_STONE)
        y += h_base

        self.part(f"{path}/Shaft", "cylinder", [cx, y + h_shaft / 2, cz],
                  [d_shaft, h_shaft, d_shaft], COL_STONE, can_collide=True)
        if full:
            for k in range(8):
                phi = facing_deg + 22.5 + 45.0 * k
                u = wing_axis(phi)
                rr = 0.49 * d
                self.part(f"{path}/Fillet_{k}", "block",
                          [cx + u[0] * rr, y + h_shaft / 2, cz + u[2] * rr],
                          [0.09 * d, h_shaft * 0.96, 0.1 * d], COL_STONE,
                          rot=quat_y(phi))
        y += h_shaft
        if full:
            self.part(f"{path}/Astragal", "cylinder", [cx, y, cz],
                      [1.0 * d, 0.07 * d, 1.0 * d], COL_STONE)

        # The bell: an upturned cone whose surface leaves the shaft exactly at
        # the shaft's radius. Everything below that is buried inside the shaft.
        r_s = d_shaft / 2.0
        r_top = 0.66 * d
        cone_h = h_bell / (1.0 - r_s / r_top)
        y_top = y + h_bell
        apex = y_top - cone_h
        self.part(f"{path}/Bell", "cone", [cx, y_top - cone_h / 2, cz],
                  [2 * r_top, cone_h, 2 * r_top], COL_STONE, rot=QUAT_FLIP)

        def bell_r(yy):
            return r_top * (yy - apex) / cone_h

        def leaf(name, phi, yc, h_leaf, w_leaf, tilt):
            u = wing_axis(phi)
            t = wing_perp(phi)
            rr = bell_r(yc) + 0.05 * d
            # quat_y(phi) turns the leaf's thin axis (local Z) outward; the tilt
            # about the tangent then leans its tip (local +Y) out over the bell.
            rot = quat_mul(quat_axis_angle(t, tilt), quat_y(phi))
            self.part(f"{path}/{name}", "ball",
                      [cx + u[0] * rr, yc, cz + u[2] * rr],
                      [w_leaf, h_leaf, 0.15 * d], LEAF_STONE, rot=rot)

        if full:
            for k in range(4):
                leaf(f"LeafLow_{k}", facing_deg + 90.0 * k, y + 0.30 * d,
                     0.62 * d, 0.46 * d, 12.0)
                leaf(f"LeafHigh_{k}", facing_deg + 45.0 + 90.0 * k,
                     y + 0.62 * d, 0.62 * d, 0.46 * d, 20.0)
            for k in range(4):
                u = wing_axis(facing_deg + 45.0 + 90.0 * k)
                rr = 0.74 * d
                self.part(f"{path}/Helix_{k}", "ball",
                          [cx + u[0] * rr, y_top - 0.1 * d, cz + u[2] * rr],
                          [0.26 * d, 0.26 * d, 0.26 * d], LEAF_STONE)
        else:
            for k in range(4):
                leaf(f"Leaf_{k}", facing_deg + 45.0 + 90.0 * k, y + 0.48 * d,
                     0.9 * d, 0.55 * d, 18.0)

        self.part(f"{path}/Abacus", "block", [cx, y_top + h_abac / 2, cz],
                  [1.5 * d, h_abac, 1.5 * d], CORNICE, rot=yaw)
        return y_top + h_abac

    def arch(self, path, angle_deg, radius, springline_y, *, centre_r,
             depth=3.2, thickness=3.0, color=None, voussoirs=11,
             material="Marble", pier_h=None):
        """A semicircular voussoir arch spanning a hall's axis, open underneath.

        Built as a ring of blocks each rotated to its own tangent, so you walk
        THROUGH it. The arch face lies in the plane of the hall's perpendicular
        and Y, so every voussoir tilts about the HALL AXIS, composed onto the
        hall's yaw. Returns the crown position, for a keystone and title."""
        col = color or ARCH_STONE
        perp = wing_perp(angle_deg)
        axis = wing_axis(angle_deg)
        base_rot = quat_y(angle_deg)

        # Piers carry the ring down to the floor from the springline.
        if pier_h is None:
            pier_h = springline_y - FLOOR_Y
        if pier_h > 0.5:
            for side, s in (("L", -1.0), ("R", 1.0)):
                self.part(
                    f"{path}/Pier_{side}", "block",
                    along(angle_deg, centre_r, s * radius, FLOOR_Y + pier_h / 2),
                    [thickness, pier_h, depth], col, rot=base_rot,
                    material=material, can_collide=True)

        # Voussoir ring. Slight arc-length overlap so no daylight between blocks.
        seg = math.pi * radius / voussoirs * 1.18
        crown = None
        for i in range(voussoirs):
            th = math.pi * (i + 0.5) / voussoirs      # 0..pi across the ring
            base = along(angle_deg, centre_r, 0.0, springline_y)
            pos = [base[0] + perp[0] * radius * math.cos(th),
                   base[1] + radius * math.sin(th),
                   base[2] + perp[2] * radius * math.cos(th)]
            # local X -> tangent(th) = -sin(th)*perp + cos(th)*Y, i.e. rotate the
            # yawed X (which points along perp) by th + 90 degrees about the axis.
            phi = math.degrees(th) + 90.0
            self.part(f"{path}/V{i:02d}", "block", pos, [seg, thickness, depth],
                      col, rot=quat_mul(quat_axis_angle(axis, phi), base_rot),
                      material=material)
            if crown is None or pos[1] > crown[1]:
                crown = pos
        return crown

    def tympanum(self, path, ang, r, lat_c, base_y, half_w, pitch_deg, color,
                 *, depth=1.6):
        """The triangular face of a pediment, as two right-angled wedges back to
        back. `wedge.glb` has its tall face at local -Z and its slope falling
        toward +Z, extruded along X; yawing each half by the bearing +-90 turns
        local +Z outward along the perpendicular, so the tall faces meet at the
        centre and the slopes fall to the eaves. Returns the apex height."""
        h = half_w * math.tan(math.radians(pitch_deg))
        for side, s in (("L", -1.0), ("R", 1.0)):
            self.part(f"{path}/Tympanum_{side}", "wedge",
                      along(ang, r, lat_c + s * half_w / 2.0, base_y + h / 2.0),
                      [depth, h, half_w], color, rot=quat_y(ang + 90.0 * s))
        return base_y + h

    def slope_slab(self, path, ang, r, lat_c, s, eave_y, half_w, pitch_deg, *,
                   overhang, thick, length, color, material="Marble"):
        """One side of a gable: a slab whose UNDERSIDE runs from the eave
        (`half_w + overhang` out, at `eave_y` minus the overhang's drop) up to
        the ridge over `lat_c`. Rotating the yawed slab by -s * pitch about the
        axis lifts its inner edge, so the two sides meet at the top."""
        th = math.radians(pitch_deg)
        run = half_w + overhang
        lat_mid = run / 2.0 + (thick / 2.0) * math.sin(th)
        y_mid = eave_y + (half_w - overhang) * math.tan(th) / 2.0 + (thick / 2.0) * math.cos(th)
        rot = quat_mul(quat_axis_angle(wing_axis(ang), -s * pitch_deg), quat_y(ang))
        self.part(path, "block", along(ang, r, lat_c + s * lat_mid, y_mid),
                  [run / math.cos(th), thick, length], color, rot=rot,
                  material=material)

    def edge(self, path, p1, p2, color, *, thick=0.35, material="Neon",
             transparency=0.0):
        d = [p2[i] - p1[i] for i in range(3)]
        length = math.sqrt(sum(c * c for c in d))
        mid = [(p1[i] + p2[i]) / 2.0 for i in range(3)]
        self.part(path, "cylinder", mid, [thick, length, thick], color,
                  rot=quat_y_to(d), material=material, cast_shadow=False,
                  transparency=transparency)

    # Manifest of what THIS generator last wrote, so a later run can tell its
    # own output apart from a human's edits. Lives in `.eustress/` (outside
    # `Workspace/`, and skipped by the file loader).
    MANIFEST_REL = os.path.join(".eustress", "generated.manifest")

    @staticmethod
    def _live_instance_set(space_dir):
        """Relative paths of every `_instance.toml` currently under Workspace/,
        ignoring dot-dirs and `trash` exactly as the engine's loader does."""
        ws = os.path.join(space_dir, "Workspace")
        out = set()
        for dirpath, dirnames, filenames in os.walk(ws):
            dirnames[:] = [d for d in dirnames
                           if not d.startswith(".") and d != "trash"]
            if "_instance.toml" in filenames:
                out.add(os.path.relpath(dirpath, ws).replace(os.sep, "/"))
        return out

    def _recorded_instances(self, space_dir):
        mpath = os.path.join(space_dir, self.MANIFEST_REL)
        try:
            with io_open(mpath, encoding="utf-8") as fh:
                return set(json.load(fh).get("instances", []))
        except Exception:
            return None

    def _divergence(self, space_dir):
        """`(removed, added)` vs the last generated manifest, or None when the
        tree matches it exactly.

        FAIL SAFE. A missing or unreadable manifest means the provenance of
        what is on disk is UNKNOWN, and it may be entirely hand-authored, so it
        counts as divergence rather than as "nothing to protect"."""
        live = self._live_instance_set(space_dir)
        recorded = self._recorded_instances(space_dir)
        if recorded is None:
            return (0, len(live)) if live else None
        removed, added = recorded - live, live - recorded
        return (len(removed), len(added)) if (removed or added) else None

    def _write_manifest(self, space_dir, instances=None):
        mpath = os.path.join(space_dir, self.MANIFEST_REL)
        os.makedirs(os.path.dirname(mpath), exist_ok=True)
        if instances is None:
            instances = self._live_instance_set(space_dir)
        with io_open(mpath, "w", encoding="utf-8", newline="\n") as fh:
            json.dump({"generator": "gen_tucson_universe.py",
                       "generated_at": GENERATED_AT,
                       "instances": sorted(instances)},
                      fh, indent=1)

    def _service_toml(self):
        return "\n".join([
            "[service]",
            'class_name = "Workspace"',
            'icon = "workspace"',
            "can_have_children = true",
            "gravity = [0.0, -196.2, 0.0]",
            "air_density = 1.225",
            "streaming_enabled = false",
            "streaming_min_radius = 64",
            "streaming_target_radius = 1024",
            "",
            "[metadata]",
            'id = "workspace-service"',
            f'created = "{GENERATED_AT}"',
            f'last_modified = "{GENERATED_AT}"',
            "",
        ])

    def flush(self, force, clobber=False, sync=False):
        for note in self.notes:
            print(f"  note: {note}")
        space_dir = os.path.join(self.root, UNIVERSE, "Spaces", self.name)
        self.files["Workspace/_service.toml"] = self._service_toml()
        if os.path.isdir(space_dir):
            if not force:
                print(f"  SKIP {self.name} (exists; pass --force to regenerate)")
                return False
            # HAND-EDIT GUARD. Regenerating silently resurrects anything
            # deleted in Studio, which looks exactly like "deletions don't stick
            # after restart". Compare what is on disk against the manifest
            # written by the last generation; if a human has touched it, refuse.
            diverged = self._divergence(space_dir)
            if diverged and not clobber:
                gone, added = diverged
                print(f"  REFUSING {self.name}: {gone} instance(s) removed and "
                      f"{added} added since this generator last wrote it.")
                print("           Those are hand edits; regenerating would undo them.")
                print("           Re-run with --clobber to overwrite anyway.")
                return False
            if sync:
                return self._sync(space_dir)
            # Only ever remove the authored Workspace tree, never the engine's
            # own world.fjalldb / header.bin / .eustress state.
            ws = os.path.join(space_dir, "Workspace")
            if os.path.isdir(ws):
                shutil.rmtree(ws)
        os.makedirs(space_dir, exist_ok=True)

        with open(os.path.join(space_dir, "space.toml"), "w", encoding="utf-8", newline="\n") as fh:
            fh.write("\n".join([
                "# EEP Space metadata",
                "[space]",
                f'name = "{self.name}"',
                f'author = "{AUTHOR}"',
                'version = "0.1.0"',
                'created_with = "Eustress Engine"',
                "",
                "[metadata]",
                f'created = "{GENERATED_AT}"',
                f'last_modified = "{GENERATED_AT}"',
                "",
            ]))

        for rel, text in sorted(self.files.items()):
            dest = os.path.join(space_dir, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(text)
        self._write_manifest(space_dir)
        print(f"  {self.name}: {len(self.files)} files")
        return True

    def _sync(self, space_dir):
        """Apply this build to a Space the engine has OPEN, as a diff.

        1. Remove every file inside an instance folder this generator created
           (per the last manifest) that the new build does not produce. Folders
           a human created are never touched.
        2. Label folders whose content changes are removed too and written back
           after the pause: an in-place edit of an existing label does not
           always reach the label atlas, a fresh create does.
        3. Write parents before children, skipping files whose bytes already
           match, and read each write back.

        Files are written in place. Write-to-temp-then-rename would be safer
        against a torn write, but this watcher stitches a rename into ONE event
        and keeps its first path (the temp file, already gone), so the rename
        target would never hot-reload. The read-back is the guard instead: a
        write that did not land stops the run, and re-running is idempotent."""
        ws = os.path.join(space_dir, "Workspace")
        svc = os.path.join(ws, "_service.toml")
        want = {}
        for rel, text in self.files.items():
            p = os.path.join(space_dir, rel.replace("/", os.sep))
            if p == svc and os.path.isfile(p):
                continue  # the engine owns its service file once the Space exists
            want[p] = text.encode("utf-8")
        owned = self._recorded_instances(space_dir) or set()

        def on_disk(p):
            try:
                with open(p, "rb") as fh:
                    return fh.read()
            except OSError:
                return None

        by_dir = {}
        for p in want:
            by_dir.setdefault(os.path.dirname(p), []).append(p)
        stale_labels = {
            d for d, ps in by_dir.items()
            if any(p.endswith(".textlabel.toml") for p in ps)
            and os.path.isdir(d)
            and any(on_disk(p) != want[p] for p in ps)
        }

        doomed = []
        for dirpath, dirnames, filenames in os.walk(ws):
            dirnames[:] = [d for d in dirnames
                           if not d.startswith(".") and d != "trash"]
            rel_dir = os.path.relpath(dirpath, ws).replace(os.sep, "/")
            for fn in filenames:
                p = os.path.join(dirpath, fn)
                if dirpath in stale_labels or (rel_dir in owned and p not in want):
                    doomed.append(p)
        doomed.sort(key=lambda p: (-p.count(os.sep), p))
        for p in doomed:
            os.remove(p)
        pruned = 0
        for dirpath, dirnames, filenames in os.walk(ws, topdown=False):
            if dirpath == ws:
                continue
            parts = os.path.relpath(dirpath, ws).split(os.sep)
            if any(x.startswith(".") or x == "trash" for x in parts):
                continue
            if not os.listdir(dirpath):
                os.rmdir(dirpath)
                pruned += 1
        if doomed:
            time.sleep(SYNC_SETTLE_SECS)

        written = unchanged = 0
        for p in sorted(want, key=lambda p: (p.count(os.sep),
                                             not p.endswith("_instance.toml"), p)):
            data = want[p]
            if on_disk(p) == data:
                unchanged += 1
                continue
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as fh:
                fh.write(data)
            if on_disk(p) != data:
                raise RuntimeError(f"write did not land: {p}")
            written += 1

        generated = {os.path.relpath(os.path.dirname(p), ws).replace(os.sep, "/")
                     for p in want if os.path.basename(p) == "_instance.toml"}
        self._write_manifest(space_dir, generated)
        print(f"  {self.name} (sync): removed {len(doomed)} files "
              f"({len(stale_labels)} labels refreshed), pruned {pruned} folders, "
              f"wrote {written}, unchanged {unchanged}")
        return True


# ── the temple ───────────────────────────────────────────────────────────────
ARCH_TITLE_STUDS = 16.0
PEDIMENT_STUDS = 16.0


def build_temple(root, force, clobber=False, sync=False):
    w = SpaceWriter(root, HUB)

    w.part("Stylobate", "cylinder", [0, 1, 0], [STYLO_R * 2, 2, STYLO_R * 2],
           SANDSTONE, material="Concrete", can_collide=True)
    w.part("Step", "cylinder", [0, 3, 0], [STEP_R * 2, 2, STEP_R * 2],
           STEP_STONE, material="Concrete", can_collide=True)

    _rotunda(w)
    _chamber(w)
    fonts = {
        "title": uniform_font([wg["need"].upper() for wg in WINGS],
                              ARCH_TITLE_STUDS, band=3.6),
        "discipline": uniform_font([wg["discipline"].upper() for wg in WINGS],
                                   PEDIMENT_STUDS, max_lines=2, band=4.6),
    }
    for wing in WINGS:
        _hall(w, wing, fonts)
    _gardens(w)
    _mind_map(w)
    return w.flush(force, clobber, sync)


def _rotunda(w):
    """An open ring of twelve Corinthian columns carrying a twelve-sided
    entablature, open to the sky so the Ledger's beam rises straight to the
    canopy. Every column stands BETWEEN two hall axes."""
    w.part("RotundaFloor", "cylinder", [0, FLOOR_Y + 0.5, 0],
           [ROT_FLOOR_R * 2, 1, ROT_FLOOR_R * 2], TRAVERTINE,
           material="Marble", can_collide=True)
    for i in range(ROT_COLS):
        a = 15.0 + 360.0 * i / ROT_COLS
        w.corinthian_column(f"Rotunda/Column_{i:02d}", along(a, ROT_COL_R),
                            WALK_Y, ROT_COL_H, ROT_COL_D, facing_deg=a, full=True)

    # Architrave, frieze and cornice as straight beams from column to column.
    # Each beam's midpoint sits over a hall axis, high above the view down it.
    half = 180.0 / ROT_COLS
    chord = 2.0 * ROT_COL_R * math.sin(math.radians(half))
    r_mid = ROT_COL_R * math.cos(math.radians(half))
    bands = (("Architrave", 3.4, 4.6, ENTAB, 0.0),
             ("Frieze", 3.6, 4.0, FRIEZE, 0.0),
             ("Cornice", 2.2, 6.6, CORNICE, 3.0))
    for i in range(ROT_COLS):
        mid = 30.0 + 360.0 * i / ROT_COLS
        y = WALK_Y + ROT_COL_H
        for name, h, depth, col, extra in bands:
            w.part(f"Rotunda/{name}_{i:02d}", "block",
                   along(mid, r_mid, 0.0, y + h / 2.0),
                   [chord + 1.5 * ROT_COL_D + extra, h, depth], col,
                   rot=quat_y(mid))
            y += h
    top = WALK_Y + ROT_COL_H + sum(b[1] for b in bands)
    for i in range(ROT_COLS):
        a = 15.0 + 360.0 * i / ROT_COLS
        w.part(f"Rotunda/Acroterion_{i:02d}/Base", "block",
               along(a, ROT_COL_R, 0.0, top + 1.2), [4.2, 2.4, 4.2], CORNICE,
               rot=quat_y(a))
        w.part(f"Rotunda/Acroterion_{i:02d}/Flame", "ball",
               along(a, ROT_COL_R, 0.0, top + 4.2), [3.0, 3.6, 3.0], GOLD,
               material="Gold")


def _chamber(w):
    """The centre: where the city actually decides things.

    The Mayor and the six Ward members sit around a horseshoe table with the
    Public Ledger in its middle. The open end faces the public-comment lectern,
    and behind the lectern sits the public gallery. A column of light rises from
    the open ledger to THE PEOPLE at the top of the mind map: the record answers
    to them. Twelve beacons ring the dais, one per basic need, each on its hall's
    bearing with a glowing runner leading down that hall to its portal."""
    w.part("Chamber/Floor", "cylinder", [0, WALK_Y + 0.015, 0],
           [CHAMBER_R * 2, 0.1, CHAMBER_R * 2], CHAMBER_STONE, material="Marble")
    w.part("Chamber/DaisStep", "cylinder", [0, WALK_Y + 0.45, 0],
           [48.0, 0.9, 48.0], TRAVERTINE, material="Marble", can_collide=True)
    w.part("Chamber/Dais", "cylinder", [0, WALK_Y + 0.8, 0],
           [42.0, 1.6, 42.0], DAIS_STONE, material="Marble", can_collide=True)
    w.part("Chamber/Carpet", "cylinder", [0, DAIS_TOP + 0.02, 0],
           [36.0, 0.06, 36.0], OXBLOOD, material="Fabric")

    # Horseshoe table, open toward the lectern. Straight segments sized so
    # their OUTER edges meet.
    start = LECTERN_BEARING + COUNCIL_OPENING
    sweep = 360.0 - 2.0 * COUNCIL_OPENING
    n = 10
    step = sweep / n
    depth, body_h = 3.2, 2.6
    seg = 2.0 * (COUNCIL_TABLE_R + depth / 2.0) * math.tan(math.radians(step / 2.0))
    for k in range(n):
        phi = start + step * (k + 0.5)
        w.part(f"Chamber/Table/Body_{k:02d}", "block",
               along(phi, COUNCIL_TABLE_R, 0.0, DAIS_TOP + body_h / 2.0),
               [seg, body_h, depth], WALNUT, rot=quat_y(phi), material="Wood",
               can_collide=True)
        w.part(f"Chamber/Table/Top_{k:02d}", "block",
               along(phi, COUNCIL_TABLE_R, 0.0, DAIS_TOP + body_h + 0.15),
               [seg + 0.3, 0.3, depth + 0.6], OAK, rot=quat_y(phi),
               material="Wood")
    table_top = DAIS_TOP + body_h + 0.3

    # Seven seats outside the table, facing in. Each has a vote light on the
    # table in front of it: attributes a script or a sim can drive later.
    for seat, phi, office in SEATS:
        tag = f"Chamber/Seat_{seat.replace(' ', '')}"
        rot = quat_y(phi)
        mayor = seat == "Mayor"
        back_h = 6.2 if mayor else 4.6
        w.part(f"{tag}/Pedestal", "block", along(phi, 16.9, 0.0, DAIS_TOP + 0.8),
               [1.8, 1.6, 1.8], WALNUT, rot=rot, material="Wood")
        w.part(f"{tag}/Cushion", "block", along(phi, 16.9, 0.0, DAIS_TOP + 1.9),
               [2.8, 0.6, 2.8], OXBLOOD, rot=rot, material="Fabric",
               can_collide=True)
        w.part(f"{tag}/Back", "block",
               along(phi, 18.35, 0.0, DAIS_TOP + 2.2 + back_h / 2.0),
               [3.0, back_h, 0.5], WALNUT, rot=rot, material="Wood",
               attributes={"seat": seat, "office": office})
        w.sign(f"{tag}/Back", seat.upper(), studs=4.5, max_lines=1,
               max_distance=90.0)
        w.part(f"{tag}/VoteLight", "ball",
               along(phi, COUNCIL_TABLE_R + 0.9, 0.0, table_top + 0.45),
               [0.9, 0.9, 0.9], GOLD if mayor else VOTE_AMBER, material="Neon",
               cast_shadow=False,
               attributes={"seat": seat, "vote": "none"})

    # Public-comment lectern in the horseshoe's mouth. The wedge's tall face is
    # toward the centre, so the reading slope falls toward the speaker.
    phi = LECTERN_BEARING
    rot = quat_y(phi)
    r = 16.8
    w.part("Chamber/Lectern/Post", "block", along(phi, r, 0.0, DAIS_TOP + 1.9),
           [2.4, 3.8, 1.8], WALNUT, rot=rot, material="Wood", can_collide=True,
           attributes={"role": "public_comment",
                       "rule": "Any resident may speak to any item before the vote."})
    w.part("Chamber/Lectern/Desk", "wedge", along(phi, r, 0.0, DAIS_TOP + 4.25),
           [3.0, 0.9, 2.4], OAK, rot=rot, material="Wood")
    u = wing_axis(phi)
    t = wing_perp(phi)
    tilt = math.radians(25.0)
    dvec = [u[0] * math.sin(tilt), math.cos(tilt), u[2] * math.sin(tilt)]
    b = along(phi, r - 0.9, 0.0, DAIS_TOP + 4.7)
    w.part("Chamber/Lectern/Mic", "cylinder",
           [b[i] + dvec[i] * 0.6 for i in range(3)], [0.14, 1.2, 0.14],
           (44, 44, 48), rot=quat_axis_angle(t, 25.0), material="Metal")
    w.part("Chamber/Lectern/MicHead", "ball",
           [b[i] + dvec[i] * 1.25 for i in range(3)], [0.36, 0.36, 0.36],
           (30, 30, 34), material="Metal")
    w.sign("Chamber/Lectern/Post", "PUBLIC COMMENT", studs=6.0, max_lines=2,
           max_distance=90.0)

    # The Public Ledger: an open book on a pedestal at the exact centre, its
    # spine on the lectern-to-Mayor line so both read it the right way round.
    yaw = quat_y(LECTERN_BEARING - 180.0)
    spine = wing_axis(LECTERN_BEARING - 180.0)
    across = wing_perp(LECTERN_BEARING - 180.0)
    w.part("Chamber/Ledger/Pedestal", "cylinder", [0.0, DAIS_TOP + 1.7, 0.0],
           [4.2, 3.4, 4.2], DAIS_STONE, material="Marble", can_collide=True)
    w.part("Chamber/Ledger/Capital", "block", [0.0, DAIS_TOP + 3.6, 0.0],
           [6.0, 0.4, 5.0], CORNICE, rot=yaw)
    by = DAIS_TOP + 3.8
    w.part("Chamber/Ledger/Cover", "block", [0.0, by + 0.15, 0.0],
           [6.8, 0.3, 4.8], OXBLOOD, rot=yaw, material="Fabric",
           attributes={"role": "public_ledger",
                       "holds": "budget lines, votes, contracts, minutes",
                       "rule": LEDGER_RULE})
    page_tilt = 7.0
    for side, s in (("L", -1.0), ("R", 1.0)):
        c = [across[0] * s * 1.62,
             by + 0.3 + 0.25 + 1.6 * math.sin(math.radians(page_tilt)),
             across[2] * s * 1.62]
        # +s * tilt about the spine lifts each page's OUTER edge: an open book.
        w.part(f"Chamber/Ledger/Page_{side}", "block", c, [3.2, 0.5, 4.4],
               PARCHMENT,
               rot=quat_mul(quat_axis_angle(spine, s * page_tilt), yaw),
               material="Neon", cast_shadow=False)
    w.part("Chamber/Ledger/Spine", "cylinder", [0.0, by + 0.5, 0.0],
           [0.5, 4.6, 0.5], GOLD, rot=quat_y_to(spine), material="Gold")
    # The title is lettered on the pedestal, under the book. Anything
    # translucent in front of a label (a glowing halo was tried) is drawn over
    # it and washes the text out.
    w.sign("Chamber/Ledger/Pedestal", LEDGER_TITLE, studs=10.0, max_lines=1,
           rgb=(1.0, 0.92, 0.70), max_distance=240.0)
    w.light("Chamber/Ledger/Cover/Light", LAMP_LIGHT, brightness=3.0, range_m=34.0)
    root_y, _, root_d = CANOPY_ROOT
    w.edge("Chamber/Ledger/Beam", [0.0, by + 0.9, 0.0],
           [0.0, root_y - root_d / 2.0, 0.0], BEAM, thick=3.0,
           transparency=0.82)

    # Twelve need beacons, each on its hall's bearing.
    for wing in WINGS:
        need, a = wing["need"], float(wing["angle"])
        acc = hex_rgb(wing["accent"])
        tag = f"Chamber/Beacon_{need}"
        w.part(f"{tag}/Plinth", "block", along(a, 26.6, 0.0, WALK_Y + 1.5),
               [2.4, 3.0, 2.4], DAIS_STONE, rot=quat_y(a), can_collide=True)
        w.part(f"{tag}/Orb", "ball", along(a, 26.6, 0.0, WALK_Y + 4.2),
               [2.2, 2.2, 2.2], acc, material="Neon", cast_shadow=False,
               attributes={"need": need, "discipline": wing["discipline"],
                           "simulations": " ".join(wing["sims"]),
                           "status": "unmeasured"})
        w.sign(f"{tag}/Orb", need, studs=5.0, max_lines=1,
               rgb=[c / 255.0 for c in acc], max_distance=90.0)

    # Public gallery: three rows in four sections behind the lectern, with
    # aisles left open on every hall bearing.
    for sec, centre in enumerate((165.0, 195.0, 225.0, 255.0)):
        for row, rr in enumerate((35.5, 40.5, 45.5)):
            chord = 2.0 * rr * math.sin(math.radians(8.0))
            seat_h = 1.4 + 0.5 * row
            w.part(f"Chamber/Gallery/Bench_{sec}{row}", "block",
                   along(centre, rr, 0.0, WALK_Y + seat_h / 2.0),
                   [chord, seat_h, 2.2], BENCH_WOOD, rot=quat_y(centre),
                   material="WoodPlanks", can_collide=True)
            w.part(f"Chamber/Gallery/Back_{sec}{row}", "block",
                   along(centre, rr + 1.25, 0.0, WALK_Y + seat_h + 1.1),
                   [chord, 2.2, 0.4], BENCH_WOOD, rot=quat_y(centre),
                   material="WoodPlanks")


def _hall(w, wing, fonts):
    """One need's hall: the department that answers for that need."""
    need, ang = wing["need"], float(wing["angle"])
    acc = hex_rgb(wing["accent"])
    acc_f = [c / 255.0 for c in acc]
    tint = mix(acc, ENTAB, 0.3)
    base = f"Wing_{need}"
    yaw = quat_y(ang)
    sims = wing["sims"]
    lats = [0.0] if len(sims) == 1 else [-24.0, 24.0]

    # Floors. Each sits a hair below the one it overlaps so no two top faces
    # are coplanar.
    r_a, r_b = 78.0, HALL_R1 + 12.0
    w.part(f"{base}/Floor", "block", along(ang, (r_a + r_b) / 2.0, 0.0, FLOOR_Y + 0.46),
           [HALL_HALF_W * 2 + 14.0, 1.0, r_b - r_a], HALL_FLOOR, rot=yaw,
           material="Marble", can_collide=True)
    plaza_w = 48.0 if len(sims) == 1 else 92.0
    p_a, p_b = HALL_R1 + 8.0, PORTAL_R + 12.0
    w.part(f"{base}/Plaza", "block", along(ang, (p_a + p_b) / 2.0, 0.0, FLOOR_Y + 0.42),
           [plaza_w, 1.0, p_b - p_a], HALL_FLOOR, rot=yaw, material="Marble",
           can_collide=True)
    r0, r1 = 27.9, PORTAL_R - 14.0
    w.part(f"{base}/Runner", "block", along(ang, (r0 + r1) / 2.0, 0.0, WALK_Y + 0.04),
           [1.2, 0.12, r1 - r0], acc, rot=yaw, material="Neon",
           transparency=0.35, cast_shadow=False)

    # Name arch: the need, in its colour, on the keystone.
    crown = w.arch(f"{base}/NameArch", ang, 13.0, FLOOR_Y + 17.0,
                   centre_r=ARCH_R, depth=3.4, thickness=3.0)
    key = f"{base}/NameArch/Keystone"
    w.part(key, "block", [crown[0], crown[1] + 2.4, crown[2]], [5.0, 4.8, 4.2],
           tint, rot=yaw, attributes={"need": need,
                                      "discipline": wing["discipline"]})
    w.sign(key, need.upper(), studs=ARCH_TITLE_STUDS, max_lines=1, band=3.6,
           rgb=acc_f, max_font=fonts["title"], max_distance=280.0)

    # The obligation, on a stele facing the rotunda.
    ob = f"{base}/Obligation"
    w.part(f"{ob}/Base", "block", along(ang, OBLIGATION_R, 0.0, WALK_Y + 0.8),
           [13.0, 1.6, 2.8], ARCH_STONE, rot=yaw, can_collide=True)
    w.part(f"{ob}/Tablet", "block", along(ang, OBLIGATION_R, 0.0, WALK_Y + 4.2),
           [11.0, 5.2, 0.9], TABLET, rot=yaw, can_collide=True,
           attributes={"need": need, "inscription": wing["inscription"]})
    w.part(f"{ob}/Cap", "block", along(ang, OBLIGATION_R, 0.0, WALK_Y + 7.05),
           [11.8, 0.5, 1.4], BRONZE, rot=yaw, material="Bronze")
    w.sign(f"{ob}/Tablet", wing["inscription"], studs=11.0, max_lines=3,
           band=4.6, max_distance=80.0)

    # The hall: a Corinthian colonnade on each side under a terracotta roof.
    for bay in range(HALL_BAYS):
        r = HALL_R0 + bay * HALL_BAY
        for side, s in (("L", -1.0), ("R", 1.0)):
            w.corinthian_column(f"{base}/Col_{bay}{side}",
                                along(ang, r, s * HALL_HALF_W), WALK_Y,
                                HALL_COL_H, HALL_COL_D, facing_deg=ang,
                                full=False)
    ey = WALK_Y + HALL_COL_H
    run = HALL_R1 - HALL_R0 + 5.0
    rm = (HALL_R0 + HALL_R1) / 2.0
    for side, s in (("L", -1.0), ("R", 1.0)):
        lat = s * HALL_HALF_W
        w.part(f"{base}/Architrave_{side}", "block", along(ang, rm, lat, ey + 1.1),
               [4.4, 2.2, run], ENTAB, rot=yaw)
        w.part(f"{base}/Frieze_{side}", "block", along(ang, rm, lat, ey + 3.3),
               [3.8, 2.2, run], FRIEZE, rot=yaw)
        w.part(f"{base}/Cornice_{side}", "block", along(ang, rm, lat, ey + 5.1),
               [5.6, 1.4, run + 1.6], CORNICE, rot=yaw)
    eave = ey + 5.8
    span = 2.0 * HALL_HALF_W + 4.4
    for end, r in (("Front", HALL_R0), ("Back", HALL_R1)):
        w.part(f"{base}/{end}Architrave", "block", along(ang, r, 0.0, ey + 1.1),
               [span, 2.2, 4.4], ENTAB, rot=yaw)
        w.part(f"{base}/{end}Frieze", "block", along(ang, r, 0.0, ey + 3.3),
               [span - 0.6, 2.2, 3.8], FRIEZE, rot=yaw)
        w.part(f"{base}/{end}Cornice", "block", along(ang, r, 0.0, ey + 5.1),
               [span + 1.6, 1.4, 5.6], CORNICE, rot=yaw)

    # Pediments: the front one in the hall's colour, lettered with the
    # department that answers for this need.
    roof_w = HALL_HALF_W + 3.0
    w.tympanum(f"{base}/FrontPediment", ang, HALL_R0 + 0.6, 0.0, eave, roof_w,
               ROOF_PITCH, tint)
    w.tympanum(f"{base}/BackPediment", ang, HALL_R1 - 0.6, 0.0, eave, roof_w,
               ROOF_PITCH, ENTAB)
    tab = f"{base}/FrontPediment/Tablet"
    w.part(tab, "block", along(ang, HALL_R0 - 0.5, 0.0, eave + 2.0),
           [15.0, 2.4, 0.5], BRONZE, rot=yaw, material="Bronze",
           attributes={"discipline": wing["discipline"]})
    w.sign(tab, wing["discipline"].upper(), studs=PEDIMENT_STUDS, max_lines=2,
           band=4.6, max_font=fonts["discipline"], max_distance=220.0)

    roof_len = HALL_R1 - HALL_R0 + 9.0
    for side, s in (("L", -1.0), ("R", 1.0)):
        w.slope_slab(f"{base}/Roof_{side}", ang, rm, 0.0, s, eave, roof_w,
                     ROOF_PITCH, overhang=1.8, thick=1.0, length=roof_len,
                     color=TERRACOTTA, material="Brick")
    th = math.radians(ROOF_PITCH)
    ridge_y = eave + roof_w * math.tan(th) + 1.0 / math.cos(th) - 0.25
    w.part(f"{base}/Ridge", "cylinder", along(ang, rm, 0.0, ridge_y),
           [1.6, roof_len + 0.6, 1.6], RIDGE_TILE,
           rot=quat_y_to(wing_axis(ang)), material="Brick")

    # Two pendant lamps down the hall's spine.
    ceiling = eave + roof_w * math.tan(th) - 0.3
    globe_y = WALK_Y + 24.0
    for j, r in enumerate((HALL_R0 + 36.0, HALL_R0 + 84.0)):
        chain = ceiling - (globe_y + 1.3)
        w.part(f"{base}/Lamp_{j}/Chain", "cylinder",
               along(ang, r, 0.0, globe_y + 1.3 + chain / 2.0),
               [0.22, chain, 0.22], (60, 52, 44), material="Metal",
               cast_shadow=False)
        w.part(f"{base}/Lamp_{j}/Globe", "ball", along(ang, r, 0.0, globe_y),
               [2.6, 2.6, 2.6], LAMP, material="Neon", cast_shadow=False)
        w.light(f"{base}/Lamp_{j}/Globe/Light", LAMP_LIGHT, brightness=2.5,
                range_m=46.0)

    # Four service counters: what the department actually provides.
    for i, prov in enumerate(wing["provisions"]):
        r = HALL_R0 + 12.0 + (i // 2) * 48.0
        s = -1.0 if i % 2 == 0 else 1.0
        tag = f"{base}/Service_{i}"
        w.part(f"{tag}/Counter", "block", along(ang, r, s * 11.6, WALK_Y + 1.7),
               [3.0, 3.4, 11.0], WALNUT, rot=yaw, material="Wood",
               can_collide=True,
               attributes={"need": need, "provision": prov,
                           "discipline": wing["discipline"]})
        w.part(f"{tag}/Top", "block", along(ang, r, s * 11.6, WALK_Y + 3.575),
               [3.6, 0.35, 11.6], TABLET, rot=yaw)
        w.part(f"{tag}/Screen", "block", along(ang, r, s * 15.6, WALK_Y + 7.0),
               [0.8, 14.0, 13.0], SCREEN_STONE, rot=yaw, can_collide=True)
        w.part(f"{tag}/Sign", "block", along(ang, r, s * 15.05, WALK_Y + 9.5),
               [0.3, 3.2, 10.0], tint, rot=yaw)
        w.sign(f"{tag}/Sign", prov, studs=9.0, max_lines=2, band=3.0,
               max_distance=70.0)

    # The simulation notice board(s): what measures this need, and what
    # result would prove the platform wrong.
    rb = HALL_R0 + 108.0
    for j, sim_id in enumerate(sims):
        space_name, title, falsifier = SIMS[sim_id]
        s = -1.0 if j == 0 else 1.0
        tag = f"{base}/Board_{sim_id}"
        w.part(f"{tag}/Screen", "block", along(ang, rb, s * 15.6, WALK_Y + 8.5),
               [0.8, 17.0, 13.5], SCREEN_STONE, rot=yaw, can_collide=True)
        w.part(f"{tag}/Face", "block", along(ang, rb, s * 15.0, WALK_Y + 8.0),
               [0.4, 14.0, 12.0], SLATE_DARK, rot=yaw, material="Slate",
               attributes={"simulation": sim_id, "space": space_name,
                           "title": title, "falsifier": falsifier})
        w.part(f"{tag}/Header", "block", along(ang, rb, s * 14.75, WALK_Y + 13.2),
               [0.3, 2.8, 12.0], tint, rot=yaw)
        w.part(f"{tag}/Notice", "block", along(ang, rb, s * 14.75, WALK_Y + 5.8),
               [0.3, 8.6, 11.0], NOTICE, rot=yaw)
        w.sign(f"{tag}/Header", f"{sim_id} · {title}", studs=12.0, max_lines=3,
               band=3.6, max_distance=90.0)
        w.sign(f"{tag}/Notice", f"Falsifier: {falsifier}", studs=12.0,
               max_lines=7, band=8.2, max_distance=60.0)
    if len(sims) == 1:
        w.part(f"{base}/Bench", "block", along(ang, rb, 11.8, WALK_Y + 0.9),
               [2.6, 1.8, 10.0], TABLET, rot=yaw, can_collide=True)

    # The failure stele, the last thing read before the portal.
    fs_tag = f"{base}/FailStele"
    w.part(f"{fs_tag}/Base", "block", along(ang, FAIL_R, 0.0, WALK_Y + 1.2),
           [14.0, 2.4, 3.0], FAIL_BASE, rot=yaw, material="Granite",
           can_collide=True)
    w.part(f"{fs_tag}/Tablet", "block", along(ang, FAIL_R, 0.0, WALK_Y + 6.9),
           [12.5, 9.0, 0.9], FAIL_RED, rot=yaw, material="Granite",
           can_collide=True,
           attributes={"need": need, "failure_mode": wing["failure_mode"]})
    w.part(f"{fs_tag}/Cap", "block", along(ang, FAIL_R, 0.0, WALK_Y + 11.7),
           [13.2, 0.6, 1.5], FAIL_BASE, rot=yaw, material="Granite")
    w.sign(f"{fs_tag}/Tablet", f"Fails when: {wing['failure_mode']}",
           studs=12.0, max_lines=8, band=8.6, rgb=(1.0, 0.86, 0.82),
           max_distance=70.0)

    # Portal fronts: a small Corinthian temple front per simulation.
    for sim_id, lat in zip(sims, lats):
        space_name, title, _falsifier = SIMS[sim_id]
        tag = f"{base}/Portal_{sim_id}"
        for side, s in (("L", -1.0), ("R", 1.0)):
            w.corinthian_column(f"{tag}/Col_{side}",
                                along(ang, PORTAL_R, lat + s * 13.0), WALK_Y,
                                PORTAL_COL_H, PORTAL_COL_D, facing_deg=ang,
                                full=False)
        py = WALK_Y + PORTAL_COL_H
        w.part(f"{tag}/Architrave", "block", along(ang, PORTAL_R, lat, py + 1.1),
               [31.0, 2.2, 4.8], ENTAB, rot=yaw)
        w.part(f"{tag}/Frieze", "block", along(ang, PORTAL_R, lat, py + 3.3),
               [30.4, 2.2, 4.2], FRIEZE, rot=yaw)
        w.part(f"{tag}/Cornice", "block", along(ang, PORTAL_R, lat, py + 5.1),
               [33.0, 1.4, 6.0], CORNICE, rot=yaw)
        p_eave = py + 5.8
        w.tympanum(tag, ang, PORTAL_R, lat, p_eave, PORTAL_HALF_W, PORTAL_PITCH,
                   tint, depth=1.4)
        for side, s in (("L", -1.0), ("R", 1.0)):
            w.slope_slab(f"{tag}/Rake_{side}", ang, PORTAL_R, lat, s, p_eave,
                         PORTAL_HALF_W, PORTAL_PITCH, overhang=1.2, thick=1.0,
                         length=6.4, color=CORNICE)
        # The trigger: a translucent Neon veil. Its `[attributes]` are what
        # `portal.rs` reads, no new class.
        w.part(f"{tag}/Veil", "block", along(ang, PORTAL_R, lat, WALK_Y + 16.5),
               [19.0, 33.0, 1.2], acc, rot=yaw, material="Neon",
               transparency=0.55, cast_shadow=False,
               attributes={
                   "portal_target": space_name,
                   "portal_label": f"{sim_id} · {title}",
                   "portal_radius": 16.0,
                   # Land clear of the destination's return portal.
                   "portal_arrival": [0.0, 10.0, 0.0],
                   "portal_arrival_yaw": 180.0,
               })
        w.sign(f"{tag}/Veil", sim_id, studs=8.0, max_lines=1, rgb=acc_f,
               max_distance=400.0)


def _gardens(w):
    """Sonoran desert courtyards between the halls: a saguaro, barrel cacti
    and boulders on a gravel bed. This is Tucson's temple."""
    for k in range(12):
        g = 15.0 + 30.0 * k
        tag = f"Garden_{k:02d}"
        c = along(g, GARDEN_R)
        w.part(f"{tag}/Bed", "cylinder", [c[0], FLOOR_Y + 0.3, c[2]],
               [26.0, 0.6, 26.0], GRAVEL, material="Sand", can_collide=True)
        y0 = FLOOR_Y + 0.6
        sdir = wing_axis(g + 57.0 * k)
        sx, sz = c[0] + sdir[0] * 3.0, c[2] + sdir[2] * 3.0
        trunk_h = 20.0 + (k % 3) * 2.5
        w.part(f"{tag}/Saguaro/Trunk", "cylinder", [sx, y0 + trunk_h / 2.0, sz],
               [3.0, trunk_h, 3.0], SAGUARO, material="Plastic", can_collide=True)
        w.part(f"{tag}/Saguaro/Crown", "ball", [sx, y0 + trunk_h, sz],
               [3.0, 3.0, 3.0], SAGUARO, material="Plastic")
        arm_dir = wing_perp(g + 37.0 * k)
        for arm, (sgn, frac, up) in enumerate(((1.0, 0.42, 7.0), (-1.0, 0.55, 5.0))):
            hx, hz = arm_dir[0] * sgn, arm_dir[2] * sgn
            ya = y0 + trunk_h * frac
            e = 4.6
            w.part(f"{tag}/Saguaro/Arm{arm}_Out", "cylinder",
                   [sx + hx * e / 2.0, ya, sz + hz * e / 2.0], [2.2, e, 2.2],
                   SAGUARO, rot=quat_y_to((hx, 0.0, hz)), material="Plastic")
            w.part(f"{tag}/Saguaro/Arm{arm}_Elbow", "ball",
                   [sx + hx * e, ya, sz + hz * e], [2.2, 2.2, 2.2], SAGUARO,
                   material="Plastic")
            w.part(f"{tag}/Saguaro/Arm{arm}_Up", "cylinder",
                   [sx + hx * e, ya + up / 2.0, sz + hz * e], [2.2, up, 2.2],
                   SAGUARO, material="Plastic")
            w.part(f"{tag}/Saguaro/Arm{arm}_Tip", "ball",
                   [sx + hx * e, ya + up, sz + hz * e], [2.2, 2.2, 2.2],
                   SAGUARO, material="Plastic")
        for b in range(2):
            d = wing_axis(g + 140.0 + 110.0 * b + 23.0 * k)
            w.part(f"{tag}/Barrel_{b}", "ball",
                   [c[0] + d[0] * 7.5, y0 + 0.9, c[2] + d[2] * 7.5],
                   [2.6, 2.0, 2.6], BARREL, material="Plastic")
        for b in range(2):
            d = wing_axis(g + 200.0 + 95.0 * b + 31.0 * k)
            w.part(f"{tag}/Boulder_{b}", "ball",
                   [c[0] + d[0] * 8.5, y0 + 0.7, c[2] + d[2] * 8.5],
                   [3.6, 2.2, 3.0], BOULDER, material="Slate",
                   rot=quat_y(37.0 * b + 11.0 * k))


def _mind_map(w):
    """Four rings over the temple: THE PEOPLE -> need -> discipline -> sim.
    Each strand hangs over the hall that serves it, so the graph and the
    architecture are the same object read at two altitudes."""
    ry, _, rd = CANOPY_ROOT
    root_p = [0.0, ry, 0.0]
    w.part("MindMap/Root", "ball", root_p, [rd, rd, rd], (250, 204, 21),
           material="Neon", cast_shadow=False)
    w.sign("MindMap/Root", "THE PEOPLE", studs=16.0, max_lines=1,
           rgb=(1.0, 0.94, 0.72), max_distance=1400.0)

    # Several needs share one discipline, so the graph converges rather than
    # drawing the same seat twice.
    disc_nodes = {}
    for wing in WINGS:
        disc_nodes.setdefault(wing["discipline"], []).append(wing["angle"])

    ny, nr, nd = CANOPY_NEED
    dy, dr, dd = CANOPY_DISC
    sy, sr, sd = CANOPY_SIM

    disc_pos = {}
    for disc, angles in sorted(disc_nodes.items()):
        # A shared discipline sits at the mean bearing of the needs it serves.
        mx = sum(math.sin(math.radians(a)) for a in angles) / len(angles)
        mz = sum(math.cos(math.radians(a)) for a in angles) / len(angles)
        bearing = math.degrees(math.atan2(mx, mz))
        p = along(bearing, dr, 0.0, dy)
        disc_pos[disc] = p
        slug = disc.replace(" ", "").replace("&", "").replace("(", "").replace(")", "")
        w.part(f"MindMap/Disc_{slug}", "ball", p, [dd, dd, dd], (200, 200, 208),
               material="Neon", cast_shadow=False)
        w.sign(f"MindMap/Disc_{slug}", DISC_SHORT.get(disc, disc), studs=9.0,
               max_lines=1, rgb=(0.92, 0.92, 0.96), max_distance=700.0)

    apex_y = (WALK_Y + PORTAL_COL_H + 5.8
              + PORTAL_HALF_W * math.tan(math.radians(PORTAL_PITCH)) + 1.0)
    for wing in WINGS:
        need, ang = wing["need"], wing["angle"]
        accent = hex_rgb(wing["accent"])
        rgbf = [c / 255.0 for c in accent]
        np_ = along(ang, nr, 0.0, ny)
        w.part(f"MindMap/Need_{need}", "ball", np_, [nd, nd, nd], accent,
               material="Neon", cast_shadow=False)
        w.sign(f"MindMap/Need_{need}", need, studs=10.0, max_lines=1, rgb=rgbf,
               max_distance=1000.0)
        w.edge(f"MindMap/E_Root_{need}", root_p, np_, (250, 204, 21), thick=1.1)
        w.edge(f"MindMap/E_{need}_Disc", np_, disc_pos[wing["discipline"]],
               accent, thick=0.9)

        lats = [0.0] if len(wing["sims"]) == 1 else [-24.0, 24.0]
        for sim_id, lat in zip(wing["sims"], lats):
            sp = along(ang, sr, lat, sy)
            w.part(f"MindMap/Sim_{sim_id}", "ball", sp, [sd, sd, sd], accent,
                   material="Neon", transparency=0.15, cast_shadow=False)
            w.sign(f"MindMap/Sim_{sim_id}", sim_id, studs=6.0, max_lines=1,
                   rgb=rgbf, max_distance=900.0)
            w.edge(f"MindMap/E_{sim_id}", disc_pos[wing["discipline"]], sp,
                   accent, thick=0.7)
            # A dropline from the sim node to its portal's pediment, so the
            # canopy visibly anchors into the hall below.
            w.edge(f"MindMap/E_Drop_{sim_id}", sp,
                   along(ang, PORTAL_R, lat, apex_y), accent, thick=0.45)


# ── one Space per simulation ─────────────────────────────────────────────────
def build_sim_space(root, force, clobber, sim_id, wing, sync=False):
    space_name, title, falsifier = SIMS[sim_id]
    accent = hex_rgb(wing["accent"])
    rgbf = [c / 255.0 for c in accent]
    ang = wing["angle"]
    w = SpaceWriter(root, space_name)

    w.part("Ground", "cylinder", [0, 0, 0], [620, 2, 620], (108, 107, 110),
           material="Concrete", can_collide=True)
    w.part("Dais", "cylinder", [0, 2, 0], [170, 2, 170], (136, 135, 138),
           material="Granite", can_collide=True)

    # Title monolith: the simulation and, beneath it, its falsifier. Every
    # Space states up front what result would prove its own thesis wrong.
    w.part("Monolith", "block", [0, 32.0, 66.0], [78, 52, 6], (58, 58, 64),
           material="Slate", can_collide=True)
    w.label("Monolith", sim_id, px=(SLOT_PX, 60), font_size=52, rgb=rgbf,
            max_distance=1600.0)
    w.part("Falsifier", "block", [0, 12.0, 66.0], [70, 12, 7.0], (46, 46, 52),
           material="Slate",
           attributes={"sim": sim_id, "title": title, "falsifier": falsifier})
    w.label("Falsifier", "FALSIFIER", px=(96, 40), font_size=30,
            rgb=(0.96, 0.72, 0.72), max_distance=520.0)

    # Three scaffold stations: the shape every model in the program takes.
    # Empty on purpose: declared ahead of their data, the same honest
    # placeholder rule the mode manifests use.
    for name, x in (("Inputs", -78.0), ("Mechanism", 0.0), ("Outputs", 78.0)):
        w.part(f"Station_{name}", "block", [x, 9.0, -22.0], [34, 8, 34],
               (92, 92, 98), material="Slate", can_collide=True)
        # Sibling, not a child: a child's position is parent-local and its
        # scale is inherited, which would throw the orb up and inflate it.
        w.part(f"Station_{name}_Orb", "ball", [x, 22.0, -22.0], [10, 10, 10],
               accent, material="Neon", transparency=0.2, cast_shadow=False)
        w.label(f"Station_{name}_Orb", name.upper(), px=(110, 40),
                font_size=28, rgb=rgbf, max_distance=420.0)

    # Return portal: lands the camera at this need's hall mouth in the Temple,
    # looking back down the hall toward the rotunda (yaw = the hall bearing).
    w.part("ReturnPortal/Pier_L", "block", [-16.0, 21.0, -130.0], [7, 42, 7],
           STONE, material="Marble", can_collide=True)
    w.part("ReturnPortal/Pier_R", "block", [16.0, 21.0, -130.0], [7, 42, 7],
           STONE, material="Marble", can_collide=True)
    w.part("ReturnPortal/Lintel", "block", [0, 44.0, -130.0], [46, 5, 7],
           (150, 149, 152), material="Marble")
    w.part("ReturnPortal/Veil", "block", [0, 21.0, -130.0], [30, 40, 1.5],
           (226, 220, 188), material="Neon", transparency=0.55,
           cast_shadow=False,
           attributes={
               "portal_target": HUB,
               "portal_label": f"Temple · {wing['need']} wing",
               "portal_radius": 20.0,
               "portal_arrival": along(ang, ARRIVAL_R, 0.0, FLOOR_Y + 14.0),
               "portal_arrival_yaw": float(ang),
           })
    w.label("ReturnPortal/Veil", "RETURN", px=(SLOT_PX, 56), font_size=48,
            rgb=(0.90, 0.88, 0.76), max_distance=1400.0)

    return w.flush(force, clobber, sync)


def main():
    ap = argparse.ArgumentParser(description="Generate the Tucson Universe.")
    ap.add_argument("--root", default=None,
                    help="workspace root (default: EUSTRESS_WORKSPACE, else ~/Documents/Eustress)")
    ap.add_argument("--force", action="store_true",
                    help="regenerate the authored Workspace/ tree of Spaces that already exist")
    ap.add_argument("--clobber", action="store_true",
                    help="with --force, overwrite even a Space that has been hand-edited "
                         "since this generator last wrote it (DESTROYS those edits)")
    ap.add_argument("--sync", action="store_true",
                    help="with --force, apply as a diff to a Space the engine has open: "
                         "rewrite only changed files, remove only this generator's own "
                         "instances, never touch engine state")
    ap.add_argument("--only", action="append", default=None, metavar="SPACE",
                    help="build only this Space (repeatable), e.g. --only Temple")
    args = ap.parse_args()

    root = args.root or os.environ.get("EUSTRESS_WORKSPACE") or os.path.join(
        os.path.expanduser("~"), "Documents", "Eustress")
    if not os.path.isdir(root):
        print(f"workspace root not found: {root}", file=sys.stderr)
        return 1

    # Sanity: every simulation must be reachable from exactly one hall, or a
    # Space would exist with no way in.
    reachable = [s for wing in WINGS for s in wing["sims"]]
    missing = sorted(set(SIMS) - set(reachable))
    dupes = sorted({s for s in reachable if reachable.count(s) > 1})
    if missing or dupes:
        print(f"hall/sim mismatch: unreachable {missing}, duplicated {dupes}",
              file=sys.stderr)
        return 1

    def wanted(name):
        return not args.only or name in args.only

    print(f"Universe '{UNIVERSE}' -> {root}")
    os.makedirs(os.path.join(root, UNIVERSE, "Spaces"), exist_ok=True)
    for sub in ("assets/images", "assets/splats", "references"):
        os.makedirs(os.path.join(root, UNIVERSE, sub.replace("/", os.sep)),
                    exist_ok=True)

    written = 0
    if wanted(HUB):
        written += 1 if build_temple(root, args.force, args.clobber, args.sync) else 0
    for wing in WINGS:
        for sim_id in wing["sims"]:
            if wanted(SIMS[sim_id][0]):
                written += 1 if build_sim_space(root, args.force, args.clobber,
                                                sim_id, wing, args.sync) else 0

    print(f"\n{written} Space(s) written. "
          f"{len(SIMS)} portals out of the Temple, {len(SIMS)} return portals back.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
