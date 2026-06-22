"""CLI entrypoint: run the full color-wheel pipeline.

    python -m color_wheel --input color_manifest.ndjson --output palette_manifest.json

Pipeline: load -> tokenize (k-means vocab) -> color2vec embed -> recover axes ->
derive region -> cluster per region (frequency-weighted) -> name -> write
manifest.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import sys

import numpy as np

from . import __version__
from .axes import build_axes
from .cluster import cluster_region  # noqa: F401  (imported for completeness/API)
from .embed.color2vec import Color2VecConfig, train_color2vec
from .embed.tokenize import build_vocab, cooccurrence_pairs
from .loader import df_len, load_manifest
from .manifest import build_manifest, write_manifest
from .types import REGIONS


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="color_wheel",
        description="Learn named color wheels + semantic axes from a color manifest.",
    )
    p.add_argument("--input", "-i", required=True, help="manifest path (.ndjson or .parquet)")
    p.add_argument("--output", "-o", required=True, help="output palette_manifest.json path")
    p.add_argument("--dim", type=int, default=32, help="embedding dimension (default 32)")
    p.add_argument("--seed", type=int, default=0, help="RNG seed (default 0)")
    p.add_argument(
        "--weight",
        default="usage",
        help="frequency weight column for clustering (default 'usage')",
    )
    p.add_argument("--buckets", type=int, default=64, help="vocab size / k-means buckets (default 64)")
    p.add_argument("--epochs", type=int, default=30, help="color2vec epochs (default 30)")
    p.add_argument("--window", type=int, default=4, help="morton adjacency window (default 4)")
    p.add_argument("--k", type=int, default=None, help="clusters per region (default: auto via silhouette)")
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)

    # 1. Load.
    df = load_manifest(args.input)
    n_rows = df_len(df)

    # 2. Tokenize (k-means vocabulary over OKLab).
    centroids, token_of = build_vocab(df, n_buckets=args.buckets, seed=args.seed)
    vocab_size = centroids.shape[0]

    # 3. Co-occurrence pairs (morton adjacency) + color2vec embed.
    pairs = cooccurrence_pairs(df, token_of, window=args.window)
    token_freq = np.bincount(np.asarray(token_of, dtype=np.int64), minlength=vocab_size).astype(np.float64)
    cfg = Color2VecConfig(dim=args.dim, epochs=args.epochs, seed=args.seed)
    embeddings = train_color2vec(pairs, vocab_size, cfg, token_freq=token_freq)

    # 4. Recover semantic axes.
    axes = build_axes(embeddings, centroids)

    # 5-7. Derive region, cluster per region, name, assemble manifest.
    generated_at = _dt.datetime.now(_dt.timezone.utc).isoformat()
    manifest = build_manifest(
        df,
        axes,
        embeddings,
        centroids,
        source_path=args.input,
        generated_at=generated_at,
        embed_dim=args.dim,
        version=__version__,
        k_per_region=args.k,
        seed=args.seed,
        weight_col=args.weight,
    )

    # 8. Write.
    write_manifest(manifest, args.output)

    # Summary.
    counts = manifest["meta"]["wheel_counts"]
    print(f"color_wheel {__version__}")
    print(f"  input    : {args.input}")
    print(f"  output   : {args.output}")
    print(f"  rows     : {n_rows}")
    print(f"  tokens   : {vocab_size}  (pairs: {pairs.shape[0]})")
    print(f"  embed dim: {args.dim}")
    print("  wheels   :")
    for r in REGIONS:
        print(f"    {r:<8}: {counts[r]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
