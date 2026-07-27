# -*- coding: utf-8 -*-
"""Generates the **Tucson** Universe: an abstract civic temple as the hub Space,
one Space per simulation in the civic program, portals between them, and a
mind-map canopy over the rotunda.

    python scripts/gen_tucson_universe.py [--root <workspace>] [--force]

Layout produced (under `Documents/Eustress/` unless `--root`/`EUSTRESS_WORKSPACE`):

    Tucson/
      Spaces/
        Temple/                 <- the hub: rotunda + 12 need-wings + 16 portals
        S01-Budget-Twin/        <- one Space per simulation, each with a return portal
        ... 16 total ...

The temple's twelve radial wings are the twelve BASIC NEEDS a municipality
actually provides for; each wing's colonnade ends in a portal arch to the
simulation that measures whether the city is meeting that need. The government
is not a separate building — the wings ARE the disciplines
(docs/architecture/GOVERNMENT_MODE.md §4.2 maps need -> discipline 1:1).

## Determinism

This generator is deliberately reproducible: no wall clock, no RNG, no host
paths in content. Instance uuids are `md5(<instance path within the Space>)`
and every `last_modified` is the fixed [`GENERATED_AT`] stamp, so re-running
produces byte-identical output. That is the same property S04 (the determinism
harness) demands of every model in the program — the scaffolding should not be
exempt from it.

## Idempotence

Refuses to clobber an existing Space unless `--force`. Only ever writes inside
`<root>/Tucson/`; never deletes anything outside a Space it is regenerating.
"""
import argparse
import hashlib
import json
import math
import os
import shutil
import sys
from io import open as io_open

# Fixed stamp — see the Determinism note above.
GENERATED_AT = "2026-07-26T00:00:00+00:00"
AUTHOR = "Eustress"

UNIVERSE = "Tucson"
HUB = "Temple"

# ── Geometry constants (world units; the engine renders 1 unit = 1 ft display,
# see docs/UNITS.md — a ~200-unit temple reads as a real civic building) ──────
# Monumental by design. Twelve wings on a radial plan need real circumferential
# room or they touch at the mouth: at the old WING_START=30 each wing had
# 2*pi*30/12 = 15.7 units of arc to live in while being 14 wide, so the whole
# thing read as one dense knot and no single label could be isolated. At
# WING_START=84 each wing gets 44 units of arc for a 34-wide colonnade, and by
# the portal line they are 173 apart. Roughly a 660-unit span end to end.
STYLO_R = 340.0         # outer stylobate radius
STEP_R = 322.0          # upper step
ROT_FLOOR_R = 78.0      # rotunda floor radius
ROT_COL_R = 62.0        # rotunda colonnade radius
ROT_COLS = 24           # columns in the rotunda ring
COL_H = 52.0            # rotunda column height
FLOOR_Y = 4.0           # walking level
WING_START = 84.0       # wing colonnade begins
# 9 bays starting one full step past the name arch. At 10 bays with a +6
# offset the first pair landed at r=90 while the arch sits at r=88 — measured
# overlap between the column plinths and the arch piers, so the arch was buried
# in the colonnade instead of framing its entrance.
WING_BAYS = 9           # column pairs per wing
WING_BAY_STEP = 24.0
WING_BAY_OFFSET = 30.0  # first column at r = WING_START + this (114)
WING_HALF_W = 17.0      # colonnade half-width
WING_COL_H = 34.0
PORTAL_R = 330.0        # portal arch radius from centre
ARRIVAL_R = 96.0        # where a returning camera lands (wing mouth)

# Mind-map canopy rings: (y, radius, node_diameter). Lifted well clear of the
# dome so the graph reads as a canopy over the building rather than clutter
# inside it; the sim ring sits just inside the portal line so each dropline
# falls almost vertically onto its own arch.
CANOPY_ROOT = (300.0, 0.0, 17.0)
CANOPY_NEED = (242.0, 122.0, 12.0)
CANOPY_DISC = (188.0, 212.0, 9.5)
CANOPY_SIM = (142.0, 300.0, 7.5)

STONE = (163, 162, 165)   # #A3A2A5 — the Government mode's institutional stone

# ── The programme ────────────────────────────────────────────────────────────
# Each simulation: id -> (space folder, display title, one-line falsifier).
# The falsifier is carved on the Space's monolith because a model that cannot
# contradict its operator is a prop (GOVERNMENT_MODE.md §2).
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

# The twelve wings. `angle` is degrees clockwise from +Z (the wing axis is
# (sin a, 0, cos a)). `sims` are the portals at that wing's end.
#
# `inscription` is the civic OBLIGATION the need creates — carved at the wing's
# mouth, read on the way in. `failure_mode` is how a city visibly fails at it —
# carved at the wing's END, so the last thing read before stepping through the
# portal is what failure looks like, and the simulation beyond measures exactly
# that. `accent` is the wing's wayfinding colour: twelve maximally-separated
# hues, because with radial symmetry colour is the only cue telling you which
# wing you are in.
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

# One line of ~192px holds roughly 20 characters legibly, so the canopy shows a
# short form. The full discipline name is the manifest's business
# (modes/government.toml), not the map's.
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

ROTUNDA_INSCRIPTION = "THE LEDGER"
ROTUNDA_SUBTITLE = "The record is public. Every number answers to its source."


# ── helpers ──────────────────────────────────────────────────────────────────
def hex_rgb(h):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16))


def uuid_for(key):
    """Deterministic 32-hex uuid from a stable key (the instance's Space path)."""
    return hashlib.md5(key.encode("utf-8")).hexdigest()


def quat_y(deg):
    """Quaternion [x,y,z,w] rotating about +Y by `deg`."""
    h = math.radians(deg) / 2.0
    return [0.0, math.sin(h), 0.0, math.cos(h)]


def quat_y_to(d):
    """Quaternion taking +Y onto direction `d` — used to aim cylinder edges."""
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
    """Hamilton product a*b — apply `b` first, then `a`."""
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]


def wing_axis(deg):
    a = math.radians(deg)
    return (math.sin(a), 0.0, math.cos(a))


def wing_perp(deg):
    a = math.radians(deg)
    return (math.cos(a), 0.0, -math.sin(a))


def along(deg, r, lateral=0.0, y=0.0):
    """Point at radius `r` along wing `deg`, offset `lateral` sideways."""
    ax, _, az = wing_axis(deg)
    px, _, pz = wing_perp(deg)
    return [ax * r + px * lateral, y, az * r + pz * lateral]


def f(v):
    """Format a float the way the engine's own writer does — ALWAYS with a
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


# ── emitters ─────────────────────────────────────────────────────────────────
def part_toml(key, mesh, pos, scale, color, *, rot=None, material="Marble",
              transparency=0.0, can_collide=False, class_name="Part",
              attributes=None, cast_shadow=True):
    """`can_collide` defaults to FALSE — structural surfaces opt IN.

    I briefly defaulted this to true to make picking precise (the viewport
    raycast is physics-first, with a coarse AABB fallback for collider-less
    parts). That took the Temple from ~45 FPS to ~2: roughly 1,850 extra
    colliders on decorative geometry. The 100k-static-collider = 8.1ms figure in
    the perf notes does not transfer here — these are mesh-derived, not cheap
    primitives. Precision on decoration is not worth a 23x frame cost, so only
    things you stand on or walk into carry a collider."""
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
                out.append(f'{k} = "{v}"')
            elif isinstance(v, (list, tuple)):
                out.append(f'{k} = {arr(v)}')
            elif isinstance(v, bool):
                out.append(f'{k} = {"true" if v else "false"}')
            else:
                out.append(f'{k} = {f(v)}')
        out.append('')
    # No `[metadata.created_by]`. `CreatorStamp` requires `public_key` with no
    # serde default, so a partial stamp fails to deserialize `InstanceMetadata`
    # — and because the whole file then fails to parse, the loader falls back to
    # Folder and the geometry silently never renders. Omitting the table is both
    # the fix and the truth: the struct's own doc says the stamp is "absent for
    # entities created offline", and a generator has no signing identity.
    # Inventing a public_key here would forge provenance in a project whose
    # entire thesis is that provenance is verifiable.
    out += [
        '[metadata]',
        f'class_name = "{class_name}"',
        'archivable = true',
        f'last_modified = "{GENERATED_AT}"',
        f'uuid = "{uuid_for(key)}"',
        '',
    ]
    return "\n".join(out)


# ── The billboard budget is FIXED, and it is small ──────────────────────────
# `billboard_gui.rs` packs every label into one texture atlas: 16384 px across
# 85 columns = a 192x192 px slot each. `PIXELS_PER_METER = 50`, so a full slot
# is 3.84 x 3.84 WORLD UNITS and that is the hard ceiling — a billboard asking
# for more is clamped to the slot. Authoring 46-112 "studs" (2300-5600 px, as an
# earlier pass here did) makes every label clamp to the SAME 3.84 square, which
# is why they all rendered identically sized, detached from their anchors, and
# with sentence-length text downsampled into 192 px of fuzz.
#
# So: size billboards in PIXELS, never in studs, keep both axes <= 192 so the
# authored aspect ratio survives, keep text SHORT enough to be legible in that
# many pixels, and gate with `max_distance` — a 3.84-unit label is unreadable
# past ~150 units no matter what, and rendering it anyway is pure visual noise.
SLOT_PX = 192

def billboard_toml(key, px, *, y_offset=0.0, max_distance=160.0, z=6):
    # SQUARE, always, at exactly one slot. The atlas slot is 192x192; a
    # non-square quad (192x52) maps into a square slot and comes out squished —
    # the same failure the gui_loader size_scale bug produced. The proven shape
    # (see the MindMap labels in Universe1) is a square billboard carrying a
    # TextLabel that occupies a horizontal BAND inside it, so apparent text
    # height is set by the child's `size`/`position`, not by the quad's aspect.
    # `px` is kept in the signature only to document intent per call site.
    w = h = float(SLOT_PX)
    # NO vertical offset: the billboard sits exactly on its anchor, so which
    # node a label belongs to is never ambiguous. A lift of any size reads as a
    # detached label once several anchors are near each other — which is what it
    # did. The cost is that one anchor gets exactly ONE line of text, so display
    # strings are kept short and the long prose lives in `[attributes]` on the
    # part instead of being rendered illegibly.
    _ = y_offset
    # Just enough to clear its own anchor shape, nothing more. A large z_index
    # (384, from reading "size.x * 2" as pixels) hoists a label above unrelated
    # geometry across the whole scene, which is how you get text sitting on top
    # of a building it belongs behind.
    z_order = 2
    _ = (z, px)
    # Pure-pixel UDim2 (scale = 0): world size is exactly px / PIXELS_PER_METER,
    # independent of any parent transform, so no inverse-scale compensation is
    # needed or possible — the billboard system writes this entity's transform.
    return "\n".join([
        '[gui]',
        # With ZERO vertical offset the billboard is co-located with its anchor,
        # and every anchor here is a solid shape wider than 3.84 units — a 7-unit
        # Neon orb, a 14-unit slate slab — so depth-testing hides the label
        # inside its own object. `z_index` only orders GUI layers against each
        # other; it does not win a depth test against geometry. `always_on_top`
        # is the lever that lets a label clear its own shape, which is precisely
        # what zero offset requires. Bounded by the tight `max_distance` below,
        # so it never floats over the far side of the temple.
        'always_on_top = true',
        f'max_distance = {f(max_distance)}',
        'position = [0, 0, 0, 0]',
        f'size = [0, {f(w)}, 0, {f(h)}]',
        'units_offset_world_space = [0, 0, 0]',
        'visible = true',
        f'z_index = {z_order}',
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
    """Fill the WHOLE billboard, not a band.

    The atlas slot is 192x192 px and that is a hard ceiling on a label's world
    size (3.84 units at PIXELS_PER_METER = 50). Spending only 25% of the height
    on the text — the Roblox-ish default this started from — throws away 3/4 of
    the only resolution available, which is most of why the text read as fuzz.
    Full-height with `text_scaled` gives roughly 4x the glyph height for free."""
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
        f'text = "{text}"',
        f'text_color = [{f(rgb[0])}, {f(rgb[1])}, {f(rgb[2])}, 1.0]',
        f'font_size = {int(size)}',
        f'font = "{font}"',
        'text_scaled = true',
        'text_x_alignment = "center"',
        'text_y_alignment = "center"',
        '',
    ])


class SpaceWriter:
    """Accumulates instance folders for one Space, then flushes to disk."""

    def __init__(self, root, name):
        self.root = root
        self.name = name
        self.files = {}   # relative path -> text

    def instance(self, path, text):
        self.files[f"Workspace/{path}/_instance.toml"] = text

    def label(self, parent, text, *, px=(SLOT_PX, 48), y_offset=3.0,
              rgb=(1.0, 1.0, 1.0), font_size=28, max_distance=160.0,
              name="Label"):
        """Attach a billboard label sized in PIXELS (<= 192 per axis; see
        `billboard_toml` for why). `y_offset` is a world-space lift."""
        key = f"{self.name}/{parent}/{name}"
        self.files[f"Workspace/{parent}/{name}/_instance.toml"] = billboard_toml(
            key, px, y_offset=y_offset, max_distance=max_distance)
        self.files[f"Workspace/{parent}/{name}/{name}.textlabel.toml"] = textlabel_toml(
            text, rgb=rgb, size=font_size)

    def text_block(self, parent, text, *, width=22, max_lines=5, y_top=6.0,
                   line_h=0.92, rgb=(1.0, 1.0, 1.0), font_size=22,
                   max_distance=200.0, px=(SLOT_PX, 40)):
        """Carve a sentence as STACKED one-line billboards.

        A single 192 px slot cannot hold a sentence legibly — but ~22 characters
        across 192 px is crisp, so wrap and stack. Each line is its own
        billboard lifted in world space, which is the only way to get readable
        prose out of a fixed-slot atlas."""
        words, lines, cur = text.split(), [], ""
        for word in words:
            trial = f"{cur} {word}".strip()
            if len(trial) <= width:
                cur = trial
            else:
                lines.append(cur)
                cur = word
            if len(lines) == max_lines:
                break
        if cur and len(lines) < max_lines:
            lines.append(cur)
        if not lines:
            return
        # Ellipsise rather than silently truncate, so an over-long string is
        # visibly clipped instead of quietly losing its tail.
        consumed = sum(len(l.split()) for l in lines)
        if consumed < len(words):
            lines[-1] = lines[-1][: width - 1] + "…"
        for i, line in enumerate(lines):
            self.label(parent, line, px=px, y_offset=y_top - i * line_h,
                       rgb=rgb, font_size=font_size,
                       max_distance=max_distance, name=f"Line{i}")

    def part(self, path, mesh, pos, scale, color, **kw):
        self.instance(path, part_toml(f"{self.name}/{path}", mesh, pos, scale, color, **kw))

    def ionic_column(self, path, centre_xz, y0, height, diameter, *,
                     facing_deg=0.0, color=None, material="Marble"):
        """An IONIC column, assembled from the six primitives that exist.

        Anatomy, bottom to top — the parts that make an order recognisable:
          plinth   square block, wider than the shaft
          base     Attic torus (a squat cylinder)
          shaft    two segments, upper narrower, faking entasis; the cylinder
                   mesh's own vertical striping reads as fluting
          echinus  a slightly flared band under the capital
          volutes  TWO horizontal scrolls — the Ionic signature, and the whole
                   reason this is not a Doric column
          abacus   thin square slab the entablature actually sits on

        Eight parts. Ionic proportion is ~9 lower-diameters tall, so callers
        should pass a slender diameter relative to height or it reads Doric.
        """
        col = color or STONE
        cx, _, cz = centre_xz
        d = float(diameter)
        # Vertical budget: plinth 4%, base 5%, shaft 79%, echinus 4%, capital 8%.
        h_plinth = height * 0.04
        h_base = height * 0.05
        h_shaft = height * 0.79
        h_ech = height * 0.04
        h_abac = height * 0.035
        y = y0

        self.part(f"{path}/Plinth", "block", [cx, y + h_plinth / 2, cz],
                  [d * 1.55, h_plinth, d * 1.55], col, material=material,
                  can_collide=True)
        y += h_plinth
        self.part(f"{path}/Base", "cylinder", [cx, y + h_base / 2, cz],
                  [d * 1.34, h_base, d * 1.34], col, material=material,
                  can_collide=True)
        y += h_base

        # Entasis: two stacked segments, the upper one narrower.
        seg = h_shaft / 2.0
        self.part(f"{path}/ShaftLower", "cylinder", [cx, y + seg / 2, cz],
                  [d, seg, d], col, material=material, can_collide=True)
        self.part(f"{path}/ShaftUpper", "cylinder", [cx, y + seg + seg / 2, cz],
                  [d * 0.88, seg, d * 0.88], col, material=material,
                  can_collide=True)
        y += h_shaft

        self.part(f"{path}/Echinus", "cylinder", [cx, y + h_ech / 2, cz],
                  [d * 1.06, h_ech, d * 1.06], col, material=material)
        y += h_ech

        # Volutes: cylinders laid on their sides so the circular face reads as a
        # scroll. Their axis runs along the colonnade's cross-direction, so the
        # spirals face the way you walk past them.
        vax = wing_perp(facing_deg)
        vrot = quat_y_to(vax)
        # Proportion matters more than presence here. At 1.15x the shaft (the
        # first attempt) the scrolls read as marble boulders sitting on a stick.
        # On a real Ionic capital the two scrolls plus the space between them
        # span only ~1.4x the shaft, so each is a bit over half its width.
        v_d = d * 0.58          # scroll face diameter
        v_len = d * 0.42        # how deep the scroll is
        v_off = d * 0.44        # sideways offset of each scroll
        vy = y + v_d * 0.36
        for side, s in (("L", -1.0), ("R", 1.0)):
            self.part(
                f"{path}/Volute_{side}", "cylinder",
                [cx + vax[0] * v_off * s, vy, cz + vax[2] * v_off * s],
                [v_d, v_len, v_d], col, rot=vrot, material=material)
        y = vy + v_d * 0.36

        self.part(f"{path}/Abacus", "block", [cx, y + h_abac / 2, cz],
                  [d * 1.38, h_abac, d * 1.38], (176, 175, 178),
                  rot=quat_y(facing_deg), material=material)
        return y + h_abac

    def arch(self, path, angle_deg, radius, springline_y, *, centre_r,
             depth=3.2, thickness=3.0, color=None, voussoirs=11,
             material="Marble", pier_h=None, pier_d=3.4):
        """A semicircular voussoir arch spanning a wing, open underneath.

        Built as a ring of blocks each rotated to its own tangent, so you walk
        THROUGH it — the thing a slab across the path could never do. The arch
        face lies in the plane containing the wing's perpendicular and Y, so
        every voussoir tilts about the WING AXIS, composed onto the wing yaw.

        Returns the crown position, so a caller can hang a keystone + title on it.
        """
        col = color or (150, 149, 152)
        perp = wing_perp(angle_deg)
        axis = wing_axis(angle_deg)
        base_rot = quat_y(angle_deg)

        # Piers carry the ring down to the floor from the springline.
        if pier_h is None:
            pier_h = springline_y - FLOOR_Y
        if pier_h > 0.5:
            for side, s in (("L", -1.0), ("R", 1.0)):
                w_half = radius
                self.part(
                    f"{path}/Pier_{side}", "block",
                    along(angle_deg, centre_r, s * w_half, FLOOR_Y + pier_h / 2),
                    [thickness, pier_h, depth], col, rot=base_rot,
                    material=material, can_collide=True)

        # Voussoir ring. Slight arc-length overlap so no daylight between blocks.
        seg = math.pi * radius / voussoirs * 1.18
        crown = None
        for i in range(voussoirs):
            th = math.pi * (i + 0.5) / voussoirs      # 0..pi across the ring
            cx = perp[0] * radius * math.cos(th)
            cz = perp[2] * radius * math.cos(th)
            cy = radius * math.sin(th)
            base = along(angle_deg, centre_r, 0.0, springline_y)
            pos = [base[0] + cx, base[1] + cy, base[2] + cz]
            # local X -> tangent(th) = -sin(th)*perp + cos(th)*Y, i.e. rotate the
            # yawed X (which points along perp) by th + 90 degrees about the axis.
            phi = math.degrees(th) + 90.0
            self.part(f"{path}/V{i:02d}", "block", pos, [seg, thickness, depth],
                      col, rot=quat_mul(quat_axis_angle(axis, phi), base_rot),
                      material=material)
            if crown is None or pos[1] > crown[1]:
                crown = pos
        return crown

    def edge(self, path, p1, p2, color, *, thick=0.35, material="Neon"):
        d = [p2[i] - p1[i] for i in range(3)]
        length = math.sqrt(sum(c * c for c in d))
        mid = [(p1[i] + p2[i]) / 2.0 for i in range(3)]
        self.part(path, "cylinder", mid, [thick, length, thick], color,
                  rot=quat_y_to(d), material=material, cast_shadow=False)

    # Manifest of what THIS generator last wrote, so a later run can tell its
    # own output apart from a human's edits. Lives in `.eustress/` (outside
    # `Workspace/`, which gets rmtree'd, and skipped by the file loader).
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

    def _divergence(self, space_dir):
        """`(removed, added)` vs the last generated manifest, or None when the
        tree matches it exactly.

        FAIL SAFE. A missing or unreadable manifest means the provenance of
        what is on disk is UNKNOWN — it may be entirely hand-authored. The
        first version of this guard returned None there ("nothing to protect")
        and so did not protect anything on its very first run, which is exactly
        when it was needed: it rmtree'd a Workspace someone had just edited.
        Unknown provenance is now treated as divergence."""
        mpath = os.path.join(space_dir, self.MANIFEST_REL)
        live_n = len(self._live_instance_set(space_dir))
        if not os.path.isfile(mpath):
            return (0, live_n) if live_n else None
        try:
            with io_open(mpath, encoding="utf-8") as fh:
                recorded = set(json.load(fh).get("instances", []))
        except Exception:
            return (0, live_n) if live_n else None
        live = self._live_instance_set(space_dir)
        removed, added = recorded - live, live - recorded
        return (len(removed), len(added)) if (removed or added) else None

    def _write_manifest(self, space_dir):
        mpath = os.path.join(space_dir, self.MANIFEST_REL)
        os.makedirs(os.path.dirname(mpath), exist_ok=True)
        with io_open(mpath, "w", encoding="utf-8", newline="\n") as fh:
            json.dump({"generator": "gen_tucson_universe.py",
                       "generated_at": GENERATED_AT,
                       "instances": sorted(self._live_instance_set(space_dir))},
                      fh, indent=1)

    def flush(self, force, clobber=False):
        space_dir = os.path.join(self.root, UNIVERSE, "Spaces", self.name)
        if os.path.isdir(space_dir):
            if not force:
                print(f"  SKIP {self.name} (exists; pass --force to regenerate)")
                return False
            # HAND-EDIT GUARD. `--force` rmtree's the whole Workspace and
            # rewrites it, which silently resurrects anything deleted in Studio
            # — it looks exactly like "deletions don't stick after restart".
            # Compare what is on disk against the manifest written by the last
            # generation; if a human has since touched it, refuse.
            diverged = self._divergence(space_dir)
            if diverged and not clobber:
                gone, added = diverged
                print(f"  REFUSING {self.name}: {gone} instance(s) removed and "
                      f"{added} added since this generator last wrote it.")
                print("           Those are hand edits — regenerating would undo them.")
                print("           Re-run with --clobber to overwrite anyway.")
                return False
            # Only ever remove the authored Workspace tree — never the engine's
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

        self.files["Workspace/_service.toml"] = "\n".join([
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

        for rel, text in sorted(self.files.items()):
            dest = os.path.join(space_dir, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(text)
        self._write_manifest(space_dir)
        print(f"  {self.name}: {len(self.files)} files")
        return True


# ── the temple ───────────────────────────────────────────────────────────────
def build_temple(root, force, clobber=False):
    w = SpaceWriter(root, HUB)

    # Stylobate, steps, rotunda floor.
    w.part("Stylobate", "cylinder", [0, 1, 0], [STYLO_R * 2, 2, STYLO_R * 2],
           (120, 119, 122), material="Granite", can_collide=True)
    w.part("Step", "cylinder", [0, 3, 0], [STEP_R * 2, 2, STEP_R * 2],
           (142, 141, 144), material="Granite", can_collide=True)
    w.part("RotundaFloor", "cylinder", [0, FLOOR_Y + 0.5, 0],
           [ROT_FLOOR_R * 2, 1, ROT_FLOOR_R * 2], STONE, material="Marble",
           can_collide=True)

    # Rotunda colonnade + architrave + abstract dome (a cone — this is an
    # ABSTRACT temple; the primitive set is block/ball/cylinder/cone/wedge).
    for i in range(ROT_COLS):
        a = 360.0 * i / ROT_COLS
        px, _, pz = along(a, ROT_COL_R)
        # Ionic proportion: ~9 lower-diameters tall. COL_H 52 -> d 5.8.
        w.ionic_column(f"Rotunda/Column_{i:02d}", [px, 0.0, pz], FLOOR_Y + 1.0,
                       COL_H, 5.8, facing_deg=a)
    # A real three-band entablature instead of one flat slab: architrave,
    # frieze (recessed), cornice (oversailing). The step in plan is what makes
    # a roofline read as classical rather than as a lid.
    arch_y = FLOOR_Y + 1.0 + COL_H + 2.0
    w.part("Rotunda/Architrave", "cylinder", [0, arch_y, 0],
           [ROT_COL_R * 2 + 13, 3.2, ROT_COL_R * 2 + 13], (172, 171, 174),
           material="Marble")
    w.part("Rotunda/Frieze", "cylinder", [0, arch_y + 3.2, 0],
           [ROT_COL_R * 2 + 9, 3.4, ROT_COL_R * 2 + 9], (142, 141, 145),
           material="Marble")
    w.part("Rotunda/Cornice", "cylinder", [0, arch_y + 6.6, 0],
           [ROT_COL_R * 2 + 18, 2.2, ROT_COL_R * 2 + 18], (186, 185, 188),
           material="Marble")
    arch_y += 7.9
    w.part("Rotunda/Dome", "cone", [0, arch_y + 2.5 + 35.0, 0],
           [ROT_COL_R * 2 + 4, 70.0, ROT_COL_R * 2 + 4], (176, 175, 178),
           material="Marble")
    w.part("Rotunda/Finial", "ball", [0, arch_y + 76.0, 0], [12, 12, 12],
           (250, 204, 21), material="Neon", cast_shadow=False)

    # The Ledger — the public record, at the centre, visible from everywhere.
    w.part("Rotunda/TheLedger", "block", [0, FLOOR_Y + 9.0, 0], [26, 18, 26],
           (60, 60, 66), material="Slate", can_collide=True)
    w.label("Rotunda/TheLedger", ROTUNDA_INSCRIPTION, px=(SLOT_PX, 56),
            font_size=48, max_distance=1600.0)
    w.part("Rotunda/RecordOrb", "ball", [0, FLOOR_Y + 34.0, 0], [16, 16, 16],
           (0, 188, 212), material="Neon", transparency=0.25, cast_shadow=False)
    w.label("Rotunda/RecordOrb", "THE RECORD", px=(120, 40), font_size=30,
            rgb=(0.85, 0.93, 0.98), max_distance=420.0)

    # ── Twelve wings ────────────────────────────────────────────────────────
    for wing in WINGS:
        need, ang = wing["need"], wing["angle"]
        accent = hex_rgb(wing["accent"])
        base = f"Wing_{need}"
        rot = quat_y(ang)

        # Wing floor: a paved spine from the rotunda out to the portal line.
        span = PORTAL_R - WING_START + 8.0
        mid_r = WING_START + span / 2.0
        w.part(f"{base}/Floor", "block", along(ang, mid_r, 0.0, FLOOR_Y + 0.5),
               [WING_HALF_W * 2 + 14, 1, span], (134, 133, 136), rot=rot,
               material="Granite", can_collide=True)

        # Colonnade: paired Ionic columns carrying a three-band entablature.
        for b in range(WING_BAYS):
            r = WING_START + WING_BAY_OFFSET + b * WING_BAY_STEP
            for side, lat in (("L", -WING_HALF_W), ("R", WING_HALF_W)):
                p = along(ang, r, lat, 0.0)
                w.ionic_column(f"{base}/Col_{b:02d}{side}", p, FLOOR_Y + 1.0,
                               WING_COL_H, 3.9, facing_deg=ang)

        lint_y = FLOOR_Y + 1.0 + WING_COL_H + 2.0
        lint_span = (WING_BAYS - 1) * WING_BAY_STEP + 14.0
        lint_r = WING_START + WING_BAY_OFFSET + lint_span / 2.0 - 7.0
        for side, lat in (("L", -WING_HALF_W), ("R", WING_HALF_W)):
            w.part(f"{base}/Architrave_{side}", "block",
                   along(ang, lint_r, lat, lint_y), [4.9, 2.8, lint_span],
                   (172, 171, 174), rot=rot, material="Marble")
            w.part(f"{base}/Frieze_{side}", "block",
                   along(ang, lint_r, lat, lint_y + 2.8), [3.9, 2.8, lint_span],
                   (142, 141, 145), rot=rot, material="Marble")
            w.part(f"{base}/Cornice_{side}", "block",
                   along(ang, lint_r, lat, lint_y + 5.5), [6.4, 1.9, lint_span],
                   (186, 185, 188), rot=rot, material="Marble")

        # The wing's inscription, at its mouth — clear of the rotunda floor
        # (radius 78) and just inside ARRIVAL_R, so a camera returning from this
        # need's simulation lands looking straight at the obligation.
        # A GATEWAY ARCH at the wing mouth, titled on its keystone. This was a
        # 34-wide, 14-tall slab standing square across the walkway — a wall you
        # could not pass. An arch does the same job of naming the threshold
        # while being a threshold: you walk under it.
        crown = w.arch(f"{base}/NameArch", ang, 13.0, FLOOR_Y + 17.0,
                       centre_r=88.0, depth=3.4, thickness=3.2,
                       color=(126, 125, 130))
        w.part(f"{base}/NameArch/Keystone", "block",
               [crown[0], crown[1] + 2.6, crown[2]], [5.4, 5.0, 4.2],
               (176, 175, 178), rot=rot, material="Marble",
               attributes={"need": need, "inscription": wing["inscription"],
                           "discipline": wing["discipline"]})
        w.label(f"{base}/NameArch/Keystone", need.upper(), px=(SLOT_PX, 60),
                font_size=44, rgb=[c / 255.0 for c in accent],
                max_distance=1000.0)

        # Four provisions: what government actually provides toward this need.
        # Spaced 12 apart so all four stay INSIDE the colonnade (r 38..74) and
        # clear of the failure slab at r≈83.5; they sit on the wing axis, so the
        # columns at lateral ±7 never collide with them.
        # Alternating sides, never the centreline: four 12-wide plinths down the
        # middle of a walkway is an obstacle course. Flanking them leaves the
        # spine clear and reads as a promenade you pass things along.
        for i, prov in enumerate(wing["provisions"]):
            r = WING_START + 26.0 + i * 62.0
            plat = (-1.0 if i % 2 == 0 else 1.0) * 10.5
            w.part(f"{base}/Provision_{i}", "block",
                   along(ang, r, plat, FLOOR_Y + 4.0), [12, 8, 12],
                   (96, 96, 102), rot=rot, material="Slate", can_collide=True)
            # Sibling, not a child: a child's POSITION is parent-local and so is
            # also scaled by the pedestal's [5,3.5,5], which would throw the orb
            # to y=12.6 as well as inflating it. World-space placement keeps both
            # honest and keeps the orb's own scale uniform for its label.
            w.part(f"{base}/Provision_{i}_Orb", "ball",
                   along(ang, r, plat, FLOOR_Y + 12.5), [7, 7, 7],
                   accent, material="Neon", cast_shadow=False)
            # Deliberately short max_distance: provisions are the fine grain of
            # the plan and should resolve only when you are inside the wing,
            # not add to the wall of text seen from outside.
            w.label(f"{base}/Provision_{i}_Orb", prov, px=(130, 40),
                    font_size=26, rgb=[c / 255.0 for c in accent],
                    max_distance=340.0)

        # The failure slab, at the wing's END: the last thing read before
        # stepping through the portal is how a city visibly fails at this need.
        # The simulation on the far side measures exactly that.
        # r=83.5 sits in the clear gap between the last colonnade column (r=79)
        # and the portal arch (r=88) — at PORTAL_R-9 it would have been exactly
        # on top of that column.
        # The failure marker is an arch too — same reason, and it gives the wing
        # a rhythm: pass under the NAME going in, under FAILS WHEN going out,
        # then through the portal into the model that measures it.
        fcrown = w.arch(f"{base}/FailArch", ang, 13.0, FLOOR_Y + 15.0,
                        centre_r=318.0, depth=3.4, thickness=3.0,
                        color=(96, 74, 76))
        w.part(f"{base}/FailArch/Keystone", "block",
               [fcrown[0], fcrown[1] + 2.4, fcrown[2]], [5.4, 4.6, 4.2],
               (128, 96, 98), rot=rot, material="Slate",
               attributes={"need": need, "failure_mode": wing["failure_mode"]})
        w.label(f"{base}/FailArch/Keystone", "FAILS WHEN", px=(SLOT_PX, 44),
                font_size=30, rgb=(0.95, 0.66, 0.66), max_distance=420.0)

        # Portal arches at the wing's end — one per simulation this need owns.
        sims = wing["sims"]
        lats = [0.0] if len(sims) == 1 else [-38.0, 38.0]
        for sim_id, lat in zip(sims, lats):
            space_name, title, _falsifier = SIMS[sim_id]
            tag = f"{base}/Portal_{sim_id}"
            # Ionic piers, an entablature across them, and a gable pediment —
            # a temple FRONT at the end of each wing, so a portal reads as a
            # threshold you are meant to walk through.
            for side, off in (("L", -16.0), ("R", 16.0)):
                w.ionic_column(f"{tag}/Pier_{side}",
                               along(ang, PORTAL_R, lat + off, 0.0),
                               FLOOR_Y + 1.0, 40.0, 4.6, facing_deg=ang)
            ent_y = FLOOR_Y + 1.0 + 40.0 + 2.0
            w.part(f"{tag}/Architrave", "block",
                   along(ang, PORTAL_R, lat, ent_y), [44.0, 3.0, 5.4],
                   (172, 171, 174), rot=rot, material="Marble")
            w.part(f"{tag}/Frieze", "block",
                   along(ang, PORTAL_R, lat, ent_y + 3.0), [40.0, 3.0, 4.4],
                   (142, 141, 145), rot=rot, material="Marble")
            w.part(f"{tag}/Cornice", "block",
                   along(ang, PORTAL_R, lat, ent_y + 5.9), [50.0, 2.0, 7.0],
                   (186, 185, 188), rot=rot, material="Marble")
            # Gable: two rakes leaning into each other. Built from blocks rather
            # than `wedge.glb` so the geometry is exactly known. With the wing's
            # base yaw applied, a block's local X runs along the perpendicular
            # (the portal's left-right), so the rake tilts about the WING AXIS —
            # composed onto the yaw, not replacing it.
            # Tilt sign: at lateral offset `s`, rotating by `+s * angle` lifts
            # each rake's OUTER end and drops the inner one — a valley, not a
            # gable. Negate so the high edge meets at the centre and the eaves
            # fall away outward, which is the way a pediment actually sits.
            axis = wing_axis(ang)
            for side, s in (("L", -1.0), ("R", 1.0)):
                w.part(
                    f"{tag}/Rake_{side}", "block",
                    along(ang, PORTAL_R, lat + s * 12.0, ent_y + 10.2),
                    [27.0, 2.0, 6.0],
                    (188, 187, 190),
                    rot=quat_mul(quat_axis_angle(axis, -s * 26.0), rot),
                    material="Marble")

            # The trigger itself: a translucent Neon veil in the arch. Its
            # `[attributes]` are what `portal.rs` reads — no new class.
            w.part(f"{tag}/Veil", "block",
                   along(ang, PORTAL_R, lat, FLOOR_Y + 21.0),
                   [30.0, 40.0, 1.5], accent, rot=rot, material="Neon",
                   transparency=0.55, cast_shadow=False,
                   attributes={
                       "portal_target": space_name,
                       "portal_label": f"{sim_id} · {title}",
                       # Scaled with the arch: a 30-wide veil needs a trigger
                       # you cannot slip past at free-camera speed.
                       "portal_radius": 20.0,
                       # Land clear of the destination's return portal.
                       "portal_arrival": [0.0, 10.0, 0.0],
                       "portal_arrival_yaw": 180.0,
                   })
            w.label(f"{tag}/Veil", sim_id, px=(SLOT_PX, 56), font_size=48,
                    rgb=[c / 255.0 for c in accent], max_distance=1400.0)

    # ── Mind-map canopy ─────────────────────────────────────────────────────
    # Four rings above the rotunda: THE PEOPLE -> need -> discipline -> sim.
    # Each strand hangs directly over the wing that serves it, so the graph and
    # the architecture are the same object read at two altitudes.
    ry, _, rd = CANOPY_ROOT
    root_p = [0.0, ry, 0.0]
    w.part("MindMap/Root", "ball", root_p, [rd, rd, rd], (250, 204, 21),
           material="Neon", cast_shadow=False)
    w.label("MindMap/Root", "THE PEOPLE", px=(SLOT_PX, 60), y_offset=12.0,
            font_size=44, rgb=(1.0, 0.94, 0.72), max_distance=1400.0)

    # Deduplicate discipline nodes: several needs share one discipline, so the
    # graph must converge rather than draw the same seat twice.
    disc_nodes = {}
    for wing in WINGS:
        disc_nodes.setdefault(wing["discipline"], []).append(wing["angle"])

    ny, nr, nd = CANOPY_NEED
    dy, dr, dd = CANOPY_DISC
    sy, sr, sd = CANOPY_SIM

    disc_pos = {}
    for i, (disc, angles) in enumerate(sorted(disc_nodes.items())):
        # Place a shared discipline at the mean bearing of the needs it serves.
        mx = sum(math.sin(math.radians(a)) for a in angles) / len(angles)
        mz = sum(math.cos(math.radians(a)) for a in angles) / len(angles)
        bearing = math.degrees(math.atan2(mx, mz))
        p = along(bearing, dr, 0.0, dy)
        disc_pos[disc] = p
        slug = disc.replace(" ", "").replace("&", "").replace("(", "").replace(")", "")
        w.part(f"MindMap/Disc_{slug}", "ball", p, [dd, dd, dd], (200, 200, 208),
               material="Neon", cast_shadow=False)
        w.label(f"MindMap/Disc_{slug}", DISC_SHORT.get(disc, disc),
                px=(120, 40), font_size=26, rgb=(0.92, 0.92, 0.96),
                max_distance=700.0)

    for wing in WINGS:
        need, ang = wing["need"], wing["angle"]
        accent = hex_rgb(wing["accent"])
        rgbf = [c / 255.0 for c in accent]
        np_ = along(ang, nr, 0.0, ny)
        w.part(f"MindMap/Need_{need}", "ball", np_, [nd, nd, nd], accent,
               material="Neon", cast_shadow=False)
        w.label(f"MindMap/Need_{need}", need, px=(SLOT_PX, 52),
                font_size=38, rgb=rgbf, max_distance=1000.0)
        w.edge(f"MindMap/E_Root_{need}", root_p, np_, (250, 204, 21), thick=1.1)
        w.edge(f"MindMap/E_{need}_Disc", np_, disc_pos[wing["discipline"]],
               accent, thick=0.9)

        lats = [0.0] if len(wing["sims"]) == 1 else [-34.0, 34.0]
        for sim_id, lat in zip(wing["sims"], lats):
            sp = along(ang, sr, lat, sy)
            w.part(f"MindMap/Sim_{sim_id}", "ball", sp, [sd, sd, sd], accent,
                   material="Neon", transparency=0.15, cast_shadow=False)
            w.label(f"MindMap/Sim_{sim_id}", sim_id, px=(140, 44),
                    font_size=36, rgb=rgbf, max_distance=900.0)
            w.edge(f"MindMap/E_{sim_id}", disc_pos[wing["discipline"]], sp,
                   accent, thick=0.7)
            # A dropline from the sim node to its portal, so the canopy visibly
            # anchors into the wing below.
            w.edge(f"MindMap/E_Drop_{sim_id}", sp,
                   along(ang, PORTAL_R, lat, FLOOR_Y + 44.0), accent,
                   thick=0.45)

    return w.flush(force, clobber)


# ── one Space per simulation ─────────────────────────────────────────────────
def build_sim_space(root, force, clobber, sim_id, wing):
    space_name, title, falsifier = SIMS[sim_id]
    accent = hex_rgb(wing["accent"])
    rgbf = [c / 255.0 for c in accent]
    ang = wing["angle"]
    w = SpaceWriter(root, space_name)

    w.part("Ground", "cylinder", [0, 0, 0], [620, 2, 620], (108, 107, 110),
           material="Concrete", can_collide=True)
    w.part("Dais", "cylinder", [0, 2, 0], [170, 2, 170], (136, 135, 138),
           material="Granite", can_collide=True)

    # Title monolith — the simulation and, beneath it, its falsifier. Every
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
    # Empty on purpose — declared ahead of their data, same honest-placeholder
    # rule the mode manifests use.
    stations = [
        ("Inputs", -78.0, "INPUTS — sourced, provenance-tagged"),
        ("Mechanism", 0.0, "MECHANISM — deterministic, seeded"),
        ("Outputs", 78.0, "OUTPUTS — published with uncertainty"),
    ]
    for name, x, caption in stations:
        w.part(f"Station_{name}", "block", [x, 9.0, -22.0], [34, 8, 34],
               (92, 92, 98), material="Slate", can_collide=True)
        # Sibling, not a child — same parent-scale trap as the wing provisions.
        w.part(f"Station_{name}_Orb", "ball", [x, 22.0, -22.0], [10, 10, 10],
               accent, material="Neon", transparency=0.2, cast_shadow=False)
        w.label(f"Station_{name}_Orb", name.upper(), px=(110, 40),
                font_size=28, rgb=rgbf, max_distance=420.0)

    # Return portal — lands the camera at this need's wing mouth in the Temple,
    # looking back down the wing toward the rotunda (yaw = the wing bearing).
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

    return w.flush(force, clobber)


def main():
    ap = argparse.ArgumentParser(description="Generate the Tucson Universe.")
    ap.add_argument("--root", default=None,
                    help="workspace root (default: EUSTRESS_WORKSPACE, else ~/Documents/Eustress)")
    ap.add_argument("--force", action="store_true",
                    help="regenerate the authored Workspace/ tree of Spaces that already exist")
    ap.add_argument("--clobber", action="store_true",
                    help="with --force, overwrite even a Space that has been hand-edited "
                         "since this generator last wrote it (DESTROYS those edits)")
    args = ap.parse_args()

    root = args.root or os.environ.get("EUSTRESS_WORKSPACE") or os.path.join(
        os.path.expanduser("~"), "Documents", "Eustress")
    if not os.path.isdir(root):
        print(f"workspace root not found: {root}", file=sys.stderr)
        return 1

    # Sanity: every simulation must be reachable from exactly one wing, or a
    # Space would exist with no way in.
    reachable = [s for wing in WINGS for s in wing["sims"]]
    missing = sorted(set(SIMS) - set(reachable))
    dupes = sorted({s for s in reachable if reachable.count(s) > 1})
    if missing or dupes:
        print(f"wing/sim mismatch — unreachable: {missing}, duplicated: {dupes}",
              file=sys.stderr)
        return 1

    print(f"Universe '{UNIVERSE}' -> {root}")
    os.makedirs(os.path.join(root, UNIVERSE, "Spaces"), exist_ok=True)
    for sub in ("assets/images", "assets/splats", "references"):
        os.makedirs(os.path.join(root, UNIVERSE, sub.replace("/", os.sep)),
                    exist_ok=True)

    written = 0
    written += 1 if build_temple(root, args.force, args.clobber) else 0
    for wing in WINGS:
        for sim_id in wing["sims"]:
            written += 1 if build_sim_space(root, args.force, args.clobber, sim_id, wing) else 0

    print(f"\n{written} Space(s) written. "
          f"{len(SIMS)} portals out of the Temple, {len(SIMS)} return portals back.")
    print("Open Tucson/Spaces/Temple in Studio and fly the free camera into an arch.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
