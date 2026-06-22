"""Per-region frequency-weighted clustering of OKLab colors.

`cluster_region` runs a usage-weighted KMeans over the OKLab colors of a single
region's rows. When `k` is None it auto-selects the cluster count by maximizing
the silhouette score over a small candidate range. The frequency weight
(`usage`) biases centroids toward colors that appear often.

Returns a list of cluster dicts: {srgb, oklab, oklch, weight}.
"""

from __future__ import annotations

import numpy as np

from .loader import column_list, df_len
from .oklch import oklab_to_oklch, oklab_to_srgb_u8, srgb_u8_to_oklab


def _region_oklab(df_region) -> tuple[np.ndarray, np.ndarray]:
    """Return (oklab (N,3), weights (N,)) for a region DataFrame."""
    srgb = column_list(df_region, "srgb")
    lab = np.empty((len(srgb), 3), dtype=np.float64)
    for i, (r, g, b) in enumerate(srgb):
        lab[i] = srgb_u8_to_oklab(int(r), int(g), int(b))
    try:
        w = np.asarray(column_list(df_region, "usage"), dtype=np.float64)
    except Exception:
        w = np.ones(lab.shape[0], dtype=np.float64)
    if w.shape[0] != lab.shape[0]:
        w = np.ones(lab.shape[0], dtype=np.float64)
    return lab, w


def _auto_k(lab: np.ndarray, weights: np.ndarray, seed: int) -> int:
    """Pick k by silhouette over [2 .. min(8, n_unique-1)]."""
    uniq = np.unique(lab, axis=0).shape[0]
    if uniq <= 2:
        return max(1, uniq)
    hi = int(min(8, uniq - 1))
    if hi < 2:
        return 1

    try:
        from sklearn.cluster import KMeans
        from sklearn.metrics import silhouette_score

        best_k, best_score = 2, -1.0
        for k in range(2, hi + 1):
            km = KMeans(n_clusters=k, random_state=seed, n_init=10)
            labels = km.fit_predict(lab, sample_weight=weights)
            if len(np.unique(labels)) < 2:
                continue
            score = silhouette_score(lab, labels)
            if score > best_score:
                best_score, best_k = score, k
        return best_k
    except Exception:
        # Heuristic fallback when sklearn is unavailable.
        return max(1, min(5, uniq))


def cluster_region(df_region, k: int | None = None, weight_col: str = "usage", seed: int = 0):
    """Weighted KMeans over a region's OKLab colors.

    Args:
        df_region: rows belonging to one region.
        k: cluster count; auto-selected via silhouette when None.
        weight_col: name of the frequency-weight column (default "usage").
        seed: RNG seed.

    Returns:
        list of dicts {srgb:(r,g,b), oklab:(L,a,b), oklch:(L,C,h), weight:float},
        sorted by descending weight.
    """
    n = df_len(df_region)
    if n == 0:
        return []

    srgb = column_list(df_region, "srgb")
    lab = np.empty((n, 3), dtype=np.float64)
    for i, (r, g, b) in enumerate(srgb):
        lab[i] = srgb_u8_to_oklab(int(r), int(g), int(b))

    try:
        weights = np.asarray(column_list(df_region, weight_col), dtype=np.float64)
        if weights.shape[0] != n:
            weights = np.ones(n, dtype=np.float64)
    except Exception:
        weights = np.ones(n, dtype=np.float64)

    uniq = np.unique(lab, axis=0).shape[0]
    if k is None:
        k = _auto_k(lab, weights, seed)
    k = int(max(1, min(k, uniq)))

    if k == 1:
        # Single weighted-mean centroid.
        center = np.average(lab, axis=0, weights=weights)
        total_w = float(weights.sum())
        return [_make_cluster(center, total_w)]

    try:
        from sklearn.cluster import KMeans

        km = KMeans(n_clusters=k, random_state=seed, n_init=10)
        labels = km.fit_predict(lab, sample_weight=weights)
        centers = km.cluster_centers_
    except Exception:
        centers, labels = _weighted_kmeans_numpy(lab, weights, k, seed)

    clusters = []
    for c in range(centers.shape[0]):
        mask = labels == c
        if not mask.any():
            continue
        w_sum = float(weights[mask].sum())
        clusters.append(_make_cluster(centers[c], w_sum))

    clusters.sort(key=lambda d: d["weight"], reverse=True)
    return clusters


def _make_cluster(center_lab: np.ndarray, weight: float) -> dict:
    okl, oka, okb = float(center_lab[0]), float(center_lab[1]), float(center_lab[2])
    return {
        "srgb": oklab_to_srgb_u8(okl, oka, okb),
        "oklab": (okl, oka, okb),
        "oklch": oklab_to_oklch(okl, oka, okb),
        "weight": weight,
    }


def _weighted_kmeans_numpy(x: np.ndarray, w: np.ndarray, k: int, seed: int, iters: int = 50):
    """Weighted Lloyd's k-means fallback (pure numpy)."""
    rng = np.random.default_rng(seed)
    n = x.shape[0]
    idx = rng.choice(n, size=k, replace=(n < k))
    centers = x[idx].copy()
    labels = np.zeros(n, dtype=np.int64)
    for _ in range(iters):
        d = np.linalg.norm(x[:, None, :] - centers[None, :, :], axis=2)
        new_labels = d.argmin(axis=1)
        if np.array_equal(new_labels, labels):
            labels = new_labels
            break
        labels = new_labels
        for c in range(k):
            mask = labels == c
            if mask.any():
                centers[c] = np.average(x[mask], axis=0, weights=w[mask])
    return centers, labels
