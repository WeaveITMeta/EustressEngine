"""End-to-end and unit tests for the color_wheel package."""

from __future__ import annotations

import datetime as _dt
import json
import os
import sys

import numpy as np
import pytest

# Make the package importable when running `pytest tools/color_wheel/tests`
# from the repo root without an editable install.
_PKG_PARENT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PKG_PARENT not in sys.path:
    sys.path.insert(0, _PKG_PARENT)

from color_wheel import __version__  # noqa: E402
from color_wheel.axes import build_axes  # noqa: E402
from color_wheel.cluster import cluster_region  # noqa: E402
from color_wheel.embed.color2vec import Color2VecConfig, train_color2vec  # noqa: E402
from color_wheel.embed.tokenize import build_vocab, cooccurrence_pairs  # noqa: E402
from color_wheel.loader import load_manifest  # noqa: E402
from color_wheel.manifest import build_manifest  # noqa: E402
from color_wheel.oklch import (  # noqa: E402
    oklch_to_srgb_u8,
    srgb_u8_to_oklch,
)
from color_wheel.types import REGIONS  # noqa: E402


# ---------------------------------------------------------------------------
# Synthetic fixture
# ---------------------------------------------------------------------------

def _synthetic_rows():
    """A few dozen rows spanning light/dark/saturated/muted colors."""
    palette = [
        # light & good
        (255, 255, 255), (255, 248, 220), (255, 223, 120), (240, 255, 240),
        (255, 250, 205), (173, 216, 230),
        # dark & evil
        (10, 10, 10), (40, 0, 0), (30, 0, 40), (20, 30, 10), (60, 0, 20),
        (15, 15, 25),
        # saturated / abstract primaries
        (255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 0, 255), (0, 255, 255),
        (255, 255, 0),
        # muted / realistic earthy
        (110, 90, 70), (90, 100, 70), (130, 120, 100), (80, 70, 60),
        (100, 110, 120), (120, 100, 80),
        # neutral / low chroma
        (128, 128, 128), (90, 90, 92), (200, 200, 198), (60, 62, 60),
    ]
    rows = []
    pid = 0
    morton = 0
    for rep in range(3):  # repeat to give usage weight + co-occurrence
        for (r, g, b) in palette:
            l, c, h = srgb_u8_to_oklch(r, g, b)
            rows.append(
                {
                    "world_id": "synthetic",
                    "part_id": pid,
                    "srgb": [r, g, b],
                    "oklch": [round(l, 6), round(c, 6), round(h, 6)],
                    "roblox_brick": None,
                    "class": "Part",
                    "morton": morton,
                }
            )
            pid += 1
            morton += 7  # spread spatial codes
    return rows


@pytest.fixture()
def manifest_path(tmp_path):
    rows = _synthetic_rows()
    p = tmp_path / "color_manifest.ndjson"
    with open(p, "w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row) + "\n")
    return str(p)


# ---------------------------------------------------------------------------
# OKLCH tests
# ---------------------------------------------------------------------------

def test_oklch_roundtrip():
    """srgb -> oklch -> srgb should return within a tight tolerance."""
    samples = [
        (255, 255, 255), (0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255),
        (128, 64, 200), (110, 90, 70), (12, 200, 33), (200, 200, 198),
    ]
    for (r, g, b) in samples:
        l, c, h = srgb_u8_to_oklch(r, g, b)
        rr, gg, bb = oklch_to_srgb_u8(l, c, h)
        assert abs(rr - r) <= 1, (r, g, b, rr, gg, bb)
        assert abs(gg - g) <= 1, (r, g, b, rr, gg, bb)
        assert abs(bb - b) <= 1, (r, g, b, rr, gg, bb)


def test_oklch_matches_rust_constants():
    """A couple of known sRGB -> OKLCH values from Ottosson's reference math."""
    # Pure white: L=1, C=0 (hue clamped to 0).
    l, c, h = srgb_u8_to_oklch(255, 255, 255)
    assert l == pytest.approx(1.0, abs=1e-4)
    assert c == pytest.approx(0.0, abs=1e-4)
    assert h == 0.0

    # Pure black: L=0, C=0.
    l, c, h = srgb_u8_to_oklch(0, 0, 0)
    assert l == pytest.approx(0.0, abs=1e-6)
    assert c == pytest.approx(0.0, abs=1e-6)

    # Pure sRGB red. Ottosson reference OKLab ~ (0.6279, 0.2249, 0.1258).
    # OKLCH: L~0.6279, C~hypot(0.2249,0.1258)~0.2577, hue~atan2(b,a)~29.23 deg.
    l, c, h = srgb_u8_to_oklch(255, 0, 0)
    assert l == pytest.approx(0.6279, abs=2e-3)
    assert c == pytest.approx(0.2577, abs=2e-3)
    assert h == pytest.approx(29.23, abs=0.5)

    # Pure sRGB green. Reference OKLab ~ (0.8664, -0.2339, 0.1795).
    l, c, h = srgb_u8_to_oklch(0, 255, 0)
    assert l == pytest.approx(0.8664, abs=2e-3)
    assert c == pytest.approx(0.2948, abs=3e-3)
    assert h == pytest.approx(142.5, abs=1.0)


# ---------------------------------------------------------------------------
# Axis sign test
# ---------------------------------------------------------------------------

def test_axis_sign(manifest_path):
    """A bright 'good' seed color should project positive on good_evil."""
    df = load_manifest(manifest_path)
    centroids, token_of = build_vocab(df, n_buckets=24, seed=0)
    pairs = cooccurrence_pairs(df, token_of, window=4)
    freq = np.bincount(np.asarray(token_of), minlength=centroids.shape[0]).astype(float)
    emb = train_color2vec(pairs, centroids.shape[0], Color2VecConfig(dim=16, epochs=20, seed=0), token_freq=freq)
    axes = build_axes(emb, centroids)

    from color_wheel.oklch import srgb_u8_to_oklab

    # White is an unambiguous "good" anchor.
    lab = np.asarray(srgb_u8_to_oklab(255, 255, 255), dtype=np.float32)
    d = np.linalg.norm(centroids - lab[None, :], axis=1)
    tok = int(d.argmin())
    ge = axes["good_evil"].project_one(emb[tok])
    assert ge > 0.0, f"expected white to project good-positive, got {ge}"


# ---------------------------------------------------------------------------
# Weighted clustering test
# ---------------------------------------------------------------------------

def test_weighted_cluster_respects_usage():
    """A heavily-weighted color should pull the single centroid toward itself."""
    # Build a tiny 2-color region by hand, one color far more frequent.
    rows = []
    # 1x dark color, 50x bright color -> centroid should be near bright.
    for _ in range(1):
        rows.append({"srgb": [10, 10, 10], "usage": 1.0})
    for _ in range(50):
        rows.append({"srgb": [240, 240, 240], "usage": 1.0})

    # Construct a backend DataFrame via the loader's helpers.
    from color_wheel.loader import _to_dataframe, _add_usage  # type: ignore

    # _to_dataframe needs the full canonical shape; build minimal rows.
    full_rows = []
    pid = 0
    for r in rows:
        full_rows.append(
            {
                "world_id": "w",
                "part_id": pid,
                "srgb": r["srgb"],
                "oklch": [0.0, 0.0, 0.0],
                "roblox_brick": None,
                "class": "Part",
                "morton": pid,
            }
        )
        pid += 1
    df = _add_usage(_to_dataframe(full_rows))

    clusters = cluster_region(df, k=1, weight_col="usage", seed=0)
    assert len(clusters) == 1
    cr, cg, cb = clusters[0]["srgb"]
    # The weighted-mean centroid must lean bright, not sit at the midpoint.
    assert cr > 180 and cg > 180 and cb > 180, clusters[0]["srgb"]


# ---------------------------------------------------------------------------
# Full manifest test
# ---------------------------------------------------------------------------

def test_manifest_has_seven_wheels(manifest_path):
    df = load_manifest(manifest_path)
    centroids, token_of = build_vocab(df, n_buckets=24, seed=0)
    pairs = cooccurrence_pairs(df, token_of, window=4)
    freq = np.bincount(np.asarray(token_of), minlength=centroids.shape[0]).astype(float)
    emb = train_color2vec(pairs, centroids.shape[0], Color2VecConfig(dim=16, epochs=15, seed=0), token_freq=freq)
    axes = build_axes(emb, centroids)

    manifest = build_manifest(
        df,
        axes,
        emb,
        centroids,
        source_path=manifest_path,
        generated_at=_dt.datetime(2026, 1, 1, tzinfo=_dt.timezone.utc).isoformat(),
        embed_dim=16,
        version=__version__,
        seed=0,
    )

    # Exactly the seven canonical wheels, correct spelling + order.
    assert list(manifest["wheels"].keys()) == REGIONS
    assert len(manifest["wheels"]) == 7

    # Meta keys present and correct.
    meta = manifest["meta"]
    for key in ("version", "source", "generated_at", "embed_dim", "rows", "regions", "wheel_counts"):
        assert key in meta, f"missing meta key {key}"
    assert meta["version"] == __version__
    assert meta["embed_dim"] == 16
    assert meta["regions"] == REGIONS

    # Every named color carries the required keys.
    required = {"id", "name", "number", "srgb", "oklch", "good_evil", "abstract_real"}
    total = 0
    for region, colors in manifest["wheels"].items():
        for c in colors:
            assert required.issubset(c.keys()), (region, c.keys())
            assert len(c["srgb"]) == 3
            assert len(c["oklch"]) == 3
            total += 1
    # The synthetic palette spans all quadrants, so several wheels populate.
    assert total >= 5
