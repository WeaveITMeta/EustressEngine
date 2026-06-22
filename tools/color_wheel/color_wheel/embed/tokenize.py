"""Build a color vocabulary (k-means over OKLab) and emit co-occurrence pairs.

`build_vocab` quantizes the continuous OKLab colors into `n_buckets` discrete
tokens via k-means; the cluster centroids are the token "prototypes" and each
row is mapped to its nearest centroid (its token).

`cooccurrence_pairs` produces (token_a, token_b) training pairs for the
skip-gram model. The default neighbor source is **morton adjacency**: rows are
sorted by their Morton code (spatial locality), and within a sliding window each
row co-occurs with its near neighbors. The neighbor source is pluggable via the
`neighbor_source` argument so other adjacency notions can be swapped in.
"""

from __future__ import annotations

from typing import Callable, Iterable

import numpy as np

from ..loader import column_list


def _oklab_matrix(df) -> np.ndarray:
    """Extract an (N, 3) float32 OKLab matrix from a manifest DataFrame.

    OKLab is recomputed from the sRGB column via the contract math so it does not
    depend on the upstream `oklch` column's hue wrapping (Cartesian a,b cluster
    far better than polar L,C,h).
    """
    from ..oklch import srgb_u8_to_oklab

    srgb = column_list(df, "srgb")
    lab = np.empty((len(srgb), 3), dtype=np.float64)
    for i, (r, g, b) in enumerate(srgb):
        lab[i] = srgb_u8_to_oklab(int(r), int(g), int(b))
    return lab.astype(np.float32)


def build_vocab(df, n_buckets: int = 64, seed: int = 0):
    """k-means over OKLab. Returns (centroids, token_of).

    centroids: (k, 3) float32 array of OKLab cluster centers.
    token_of:  list[int] of length N — token id (cluster index) for each row.

    k is min(n_buckets, n_unique_colors) so we never request more clusters than
    distinct colors. Uses scikit-learn KMeans when available, else a compact
    pure-numpy Lloyd's iteration fallback.
    """
    lab = _oklab_matrix(df)
    n = lab.shape[0]
    # Cap clusters by the number of distinct colors.
    uniq = np.unique(lab, axis=0)
    k = int(min(n_buckets, uniq.shape[0]))
    k = max(1, k)

    try:
        from sklearn.cluster import KMeans

        km = KMeans(n_clusters=k, random_state=seed, n_init=10)
        labels = km.fit_predict(lab)
        centroids = km.cluster_centers_.astype(np.float32)
    except Exception:
        centroids, labels = _kmeans_numpy(lab, k, seed)

    token_of = [int(x) for x in labels]
    return centroids.astype(np.float32), token_of


def _kmeans_numpy(x: np.ndarray, k: int, seed: int, iters: int = 50):
    """Minimal Lloyd's-algorithm k-means fallback (pure numpy)."""
    rng = np.random.default_rng(seed)
    n = x.shape[0]
    # k-means++-ish init: random distinct points.
    idx = rng.choice(n, size=k, replace=False) if n >= k else rng.choice(n, size=k, replace=True)
    centroids = x[idx].copy()
    labels = np.zeros(n, dtype=np.int64)
    for _ in range(iters):
        d = np.linalg.norm(x[:, None, :] - centroids[None, :, :], axis=2)
        new_labels = d.argmin(axis=1)
        if np.array_equal(new_labels, labels):
            labels = new_labels
            break
        labels = new_labels
        for c in range(k):
            members = x[labels == c]
            if members.shape[0] > 0:
                centroids[c] = members.mean(axis=0)
    return centroids, labels


def morton_neighbor_source(df, token_of: list[int], window: int = 4) -> Iterable[tuple[int, int]]:
    """Yield (token, token) pairs from Morton-sorted spatial adjacency.

    Rows are sorted by their `morton` code; each row pairs with the `window`
    rows that follow it in sorted order (both directions are emitted by the
    caller via symmetric handling). Self-pairs and identical-token pairs are
    skipped — co-occurrence between *different* color tokens is the signal.
    """
    morton = column_list(df, "morton")
    order = sorted(range(len(morton)), key=lambda i: morton[i])
    n = len(order)
    for pos in range(n):
        i = order[pos]
        ti = token_of[i]
        for off in range(1, window + 1):
            if pos + off >= n:
                break
            j = order[pos + off]
            tj = token_of[j]
            if ti == tj:
                continue
            yield (ti, tj)
            yield (tj, ti)


def cooccurrence_pairs(
    df,
    token_of: list[int],
    window: int = 4,
    neighbor_source: Callable[..., Iterable[tuple[int, int]]] | None = None,
) -> np.ndarray:
    """Build an (M, 2) int array of co-occurrence training pairs.

    Defaults to morton-adjacency. Pass a different `neighbor_source` callable
    (signature ``f(df, token_of, window) -> Iterable[(int, int)]``) to swap the
    adjacency notion.
    """
    src = neighbor_source or morton_neighbor_source
    pairs = list(src(df, token_of, window))
    if not pairs:
        return np.empty((0, 2), dtype=np.int64)
    return np.asarray(pairs, dtype=np.int64)
