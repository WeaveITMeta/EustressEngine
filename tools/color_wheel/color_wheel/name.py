"""Name the clusters of each wheel.

  * `halo`  pulls names in order from data/angels_72.json (Shem ha-Mephorash).
  * `umbra` pulls names in order from data/demons_72.json (Ars Goetia).
  * the other five wheels use a templated name:
        f"{modifier} {hueword} {n:02d}"
    where `modifier` is chosen by the sign of the dominant semantic axis for
    that wheel and `hueword` is the named hue sector of the cluster's OKLCH hue.

Each named cluster also gets a sequential `number` (1-based) within its wheel.
"""

from __future__ import annotations

import json
import os

from .types import NamedColor

_DATA_DIR = os.path.join(os.path.dirname(__file__), "data")


def _load_names(filename: str) -> list[str]:
    with open(os.path.join(_DATA_DIR, filename), "r", encoding="utf-8") as fh:
        return json.load(fh)


# Loaded lazily-once at import; small files.
ANGELS = _load_names("angels_72.json")
DEMONS = _load_names("demons_72.json")

# 12-way hue sectors over OKLCH hue degrees.
_HUE_WORDS = [
    (0, "Crimson"),
    (30, "Amber"),
    (60, "Gold"),
    (90, "Lime"),
    (120, "Verdant"),
    (150, "Jade"),
    (180, "Cyan"),
    (210, "Azure"),
    (240, "Cobalt"),
    (270, "Violet"),
    (300, "Magenta"),
    (330, "Rose"),
]

# Modifier word keyed by region + axis sign.
_MODIFIERS = {
    "aether": ("Lucent", "Astral"),     # good & abstract
    "verdure": ("Verdant", "Living"),   # good & realistic
    "stone": ("Pale", "Grey"),          # neutral
    "char": ("Scorched", "Ashen"),      # evil & realistic
    "hex": ("Hexed", "Cursed"),         # evil & abstract
}


def _hue_word(hue_deg: float, chroma: float) -> str:
    if chroma < 0.02:
        return "Neutral"
    h = hue_deg % 360.0
    best = _HUE_WORDS[0][1]
    for boundary, word in _HUE_WORDS:
        if h >= boundary:
            best = word
    # wrap: hues near 360 belong to the first (Crimson) sector
    if h >= 345.0:
        best = "Crimson"
    return best


def _modifier(region: str, good_evil: float, abstract_real: float) -> str:
    pos, neg = _MODIFIERS.get(region, ("Hued", "Muted"))
    # Use the axis most relevant to this wheel to pick the variant.
    if region in ("aether", "verdure", "halo"):
        return pos if good_evil >= 0 else neg
    if region in ("char", "hex", "umbra"):
        return pos if good_evil < 0 else neg
    # stone: keyed on lightness-ish via abstract_real sign as a stable tiebreak
    return pos if abstract_real >= 0 else neg


def name_clusters(
    region: str,
    clusters: list[dict],
    axis_values: list[tuple[float, float]],
) -> list[NamedColor]:
    """Produce NamedColor entries for a region's clusters.

    Args:
        region: one of the canonical regions.
        clusters: cluster dicts from cluster_region (sorted by weight desc).
        axis_values: parallel list of (good_evil, abstract_real) per cluster.

    Returns:
        list[NamedColor] with sequential `number` 1..N.
    """
    out: list[NamedColor] = []
    for i, (cl, (ge, ar)) in enumerate(zip(clusters, axis_values)):
        number = i + 1
        srgb = tuple(int(v) for v in cl["srgb"])
        oklch = tuple(round(float(v), 5) for v in cl["oklch"])

        if region == "halo":
            name = ANGELS[i % len(ANGELS)]
        elif region == "umbra":
            name = DEMONS[i % len(DEMONS)]
        else:
            mod = _modifier(region, ge, ar)
            hue = _hue_word(cl["oklch"][2], cl["oklch"][1])
            name = f"{mod} {hue} {number:02d}"

        cid = f"{region}-{number:02d}"
        out.append(
            NamedColor(
                id=cid,
                name=name,
                number=number,
                srgb=srgb,
                oklch=oklch,
                good_evil=round(float(ge), 5),
                abstract_real=round(float(ar), 5),
            )
        )
    return out
