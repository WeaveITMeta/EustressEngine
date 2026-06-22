"""Region derivation + manifest assembly + JSON serialization.

Region derivation (the hard contract) maps each row to one of the seven wheels
using the two learned semantic axes plus the row's OKLCH lightness/chroma:

  - low chroma / neutral          -> stone
  - high lightness + good         -> halo
  - low lightness + evil          -> umbra
  - good & abstract               -> aether
  - good & realistic              -> verdure
  - evil & abstract               -> hex
  - evil & realistic              -> char

`build_manifest` runs the per-region clustering + naming and returns a plain
dict; `write_manifest` serializes it to `palette_manifest.json`. The builder is
pure: `generated_at` is passed in by the caller (never datetime.now() inside).
"""

from __future__ import annotations

import json

import numpy as np

from .cluster import cluster_region
from .loader import backend, column_list, df_len
from .name import name_clusters
from .oklch import srgb_u8_to_oklab
from .types import REGIONS

# Region-derivation thresholds (OKLCH L in [0,1], chroma in OKLab units).
LIGHT_HIGH = 0.78
LIGHT_LOW = 0.32
CHROMA_NEUTRAL = 0.035
# Axis decision band: |axis| below this is "neutral" on that axis.
AXIS_EPS = 0.12


def derive_region(
    oklch_l: float,
    oklch_c: float,
    good_evil: float,
    abstract_real: float,
) -> str:
    """Map one color's (lightness, chroma, axes) to a canonical region."""
    # Neutrals first: low-chroma colors are stone regardless of axes.
    if oklch_c < CHROMA_NEUTRAL:
        return "stone"

    good = good_evil >= 0.0
    evil = not good
    abstract = abstract_real >= 0.0

    # Extremes of lightness on the moral axis -> halo / umbra.
    if oklch_l >= LIGHT_HIGH and good and good_evil > AXIS_EPS:
        return "halo"
    if oklch_l <= LIGHT_LOW and evil and good_evil < -AXIS_EPS:
        return "umbra"

    # Quadrants of (good/evil x abstract/realistic).
    if good and abstract:
        return "aether"
    if good and not abstract:
        return "verdure"
    if evil and abstract:
        return "hex"
    return "char"  # evil & realistic


def _token_of_rows(df, centroids: np.ndarray) -> list[int]:
    """Nearest-token index per row, by OKLab distance to vocab centroids."""
    srgb = column_list(df, "srgb")
    toks = []
    for (r, g, b) in srgb:
        lab = np.asarray(srgb_u8_to_oklab(int(r), int(g), int(b)), dtype=np.float32)
        d = np.linalg.norm(centroids - lab[None, :], axis=1)
        toks.append(int(d.argmin()))
    return toks


def assign_regions(df, axes: dict, embeddings: np.ndarray, centroids: np.ndarray):
    """Return (regions list, per-row good_evil, per-row abstract_real).

    Each row's axis values come from its nearest token's embedding projection.
    """
    token_of = _token_of_rows(df, centroids)
    ge_axis = axes["good_evil"]
    ar_axis = axes["abstract_real"]

    # Precompute per-token projections once.
    ge_tok = ge_axis.project(embeddings)
    ar_tok = ar_axis.project(embeddings)

    oklch = column_list(df, "oklch")
    regions = []
    ge_rows = []
    ar_rows = []
    for i, tok in enumerate(token_of):
        ge = float(ge_tok[tok])
        ar = float(ar_tok[tok])
        l, c, _h = oklch[i]
        regions.append(derive_region(float(l), float(c), ge, ar))
        ge_rows.append(ge)
        ar_rows.append(ar)
    return regions, ge_rows, ar_rows


def _filter_region(df, regions: list[str], target: str):
    """Return a sub-DataFrame of rows whose derived region == target."""
    idx = [i for i, r in enumerate(regions) if r == target]
    if backend() == "polars":
        import polars as pl  # local import; backend() guarantees availability

        return df[idx] if idx else pl.DataFrame(schema=df.schema)
    else:
        return df.iloc[idx]


def build_manifest(
    df,
    axes: dict,
    embeddings: np.ndarray,
    centroids: np.ndarray,
    source_path: str,
    generated_at: str,
    embed_dim: int,
    version: str = "0.1.0",
    k_per_region: int | None = None,
    seed: int = 0,
    weight_col: str = "usage",
) -> dict:
    """Assemble the full palette manifest dict.

    `generated_at` is supplied by the caller (the builder stays pure). Returns a
    dict with stable key order; floats are rounded in the NamedColor stage and
    here for axis aggregates.
    """
    regions, ge_rows, ar_rows = assign_regions(df, axes, embeddings, centroids)

    # Build a per-region axis lookup keyed by srgb so each cluster centroid can
    # inherit representative axis values from its nearest member rows.
    srgb_rows = column_list(df, "srgb")

    wheels: dict[str, list] = {}
    counts: dict[str, int] = {}
    for region in REGIONS:
        sub = _filter_region(df, regions, region)
        if df_len(sub) == 0:
            wheels[region] = []
            counts[region] = 0
            continue

        clusters = cluster_region(sub, k=k_per_region, weight_col=weight_col, seed=seed)

        # For each cluster, find axis values by averaging the axis values of the
        # region rows nearest (in OKLab) to that cluster centroid.
        sub_srgb = column_list(sub, "srgb")
        sub_lab = np.array(
            [srgb_u8_to_oklab(int(r), int(g), int(b)) for (r, g, b) in sub_srgb],
            dtype=np.float64,
        )
        # axis values aligned to sub rows
        sub_idx = [i for i, r in enumerate(regions) if r == region]
        sub_ge = np.array([ge_rows[i] for i in sub_idx], dtype=np.float64)
        sub_ar = np.array([ar_rows[i] for i in sub_idx], dtype=np.float64)

        axis_values = []
        for cl in clusters:
            cl_lab = np.asarray(cl["oklab"], dtype=np.float64)
            d = np.linalg.norm(sub_lab - cl_lab[None, :], axis=1)
            # nearest few rows (up to 5) inform the centroid's axis value
            order = np.argsort(d)[: min(5, len(d))]
            axis_values.append((float(sub_ge[order].mean()), float(sub_ar[order].mean())))

        named = name_clusters(region, clusters, axis_values)
        wheels[region] = [
            {
                "id": nc.id,
                "name": nc.name,
                "number": nc.number,
                "srgb": list(nc.srgb),
                "oklch": [round(float(v), 5) for v in nc.oklch],
                "good_evil": round(float(nc.good_evil), 4),
                "abstract_real": round(float(nc.abstract_real), 4),
            }
            for nc in named
        ]
        counts[region] = len(named)

    manifest = {
        "meta": {
            "version": version,
            "source": source_path,
            "generated_at": generated_at,
            "embed_dim": int(embed_dim),
            "rows": df_len(df),
            "regions": list(REGIONS),
            "wheel_counts": {r: counts[r] for r in REGIONS},
        },
        "wheels": {r: wheels[r] for r in REGIONS},
    }
    return manifest


def write_manifest(manifest: dict, path: str) -> None:
    """Serialize the manifest dict to `palette_manifest.json` with stable order."""
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=False, sort_keys=False)
        fh.write("\n")
