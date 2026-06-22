"""Manifest loading + validation.

Reads the per-part color manifest emitted by the Eustress Roblox importer. The
default on-disk format is NDJSON (one JSON object per line); Parquet is also
supported. polars is preferred; if it is unavailable we fall back to
pandas+pyarrow, and failing that a pure-stdlib NDJSON reader (Parquet then
requires pyarrow).

REQUIRED columns (the contract — there is NO `region` column upstream):
    world_id:str, part_id:int(u64), srgb:[int,int,int] (0-255),
    oklch:[float,float,float] ([L, C, hue_deg]), roblox_brick:int|null,
    class:str, morton:int(u64)

This module always returns a polars.DataFrame. If polars cannot be imported at
all we still build a polars DataFrame from the fallback-parsed rows — so callers
downstream have one shape to deal with. The only hard dep of this module is that
*something* providing a DataFrame exists; we try polars, then pandas.
"""

from __future__ import annotations

import json
import os
from typing import Any

REQUIRED_COLUMNS = (
    "world_id",
    "part_id",
    "srgb",
    "oklch",
    "roblox_brick",
    "class",
    "morton",
)

# ---------------------------------------------------------------------------
# DataFrame backend selection. We expose `pl` as either real polars or a thin
# shim around pandas so the rest of the codebase can call a tiny common subset.
# ---------------------------------------------------------------------------

try:  # pragma: no cover - import side effect
    import polars as pl  # type: ignore

    _BACKEND = "polars"
except Exception:  # pragma: no cover
    pl = None  # type: ignore
    _BACKEND = None

if _BACKEND is None:
    try:  # pragma: no cover
        import pandas as _pd  # type: ignore

        _BACKEND = "pandas"
    except Exception:  # pragma: no cover
        _pd = None  # type: ignore


def backend() -> str:
    """Return the active DataFrame backend name ('polars' or 'pandas')."""
    if _BACKEND is None:
        raise RuntimeError(
            "Neither polars nor pandas is importable. Install one of them "
            "(see requirements.txt)."
        )
    return _BACKEND


def _read_ndjson_pure(path: str) -> list[dict[str, Any]]:
    """Parse an NDJSON file with the stdlib only."""
    rows: list[dict[str, Any]] = []
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            rows.append(json.loads(line))
    return rows


def _coerce_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Validate + normalize parsed rows into a canonical dict shape."""
    out: list[dict[str, Any]] = []
    for i, row in enumerate(rows):
        missing = [c for c in REQUIRED_COLUMNS if c not in row]
        if missing:
            raise ValueError(f"row {i} missing required columns: {missing}")

        srgb = row["srgb"]
        if not (isinstance(srgb, (list, tuple)) and len(srgb) == 3):
            raise ValueError(f"row {i}: srgb must be a 3-element array, got {srgb!r}")
        srgb = [int(v) for v in srgb]
        for v in srgb:
            if not (0 <= v <= 255):
                raise ValueError(f"row {i}: srgb channel {v} out of 0-255 range")

        oklch = row["oklch"]
        if not (isinstance(oklch, (list, tuple)) and len(oklch) == 3):
            raise ValueError(f"row {i}: oklch must be a 3-element array, got {oklch!r}")
        oklch = [float(v) for v in oklch]

        rb = row["roblox_brick"]
        out.append(
            {
                "world_id": str(row["world_id"]),
                "part_id": int(row["part_id"]),
                "srgb": srgb,
                "oklch": oklch,
                "roblox_brick": None if rb is None else int(rb),
                "class": str(row["class"]),
                "morton": int(row["morton"]),
            }
        )
    return out


def _to_dataframe(rows: list[dict[str, Any]]):
    """Build a DataFrame (polars preferred, pandas fallback) from canonical rows."""
    if _BACKEND == "polars":
        return pl.DataFrame(rows)  # type: ignore[union-attr]
    if _BACKEND == "pandas":
        return _pd.DataFrame(rows)  # type: ignore[union-attr]
    raise RuntimeError(
        "Neither polars nor pandas is importable. Install one of them "
        "(see requirements.txt)."
    )


def _add_usage(df):
    """Add a `usage` weight column: per identical color, the count of rows.

    Each row contributes weight 1.0; rows sharing the same srgb triple are
    aggregated so the weight reflects how often that color appears. The original
    row count is preserved (we add a column, we do not collapse rows).
    """
    if _BACKEND == "polars":
        # Build a string key for the srgb list and count occurrences.
        df = df.with_columns(
            pl.col("srgb").cast(pl.List(pl.Int64)).list.eval(pl.element().cast(pl.Utf8)).list.join(",").alias("_srgb_key")
        )
        counts = df.group_by("_srgb_key").len().rename({"len": "usage"})
        df = df.join(counts, on="_srgb_key", how="left")
        df = df.with_columns(pl.col("usage").cast(pl.Float64)).drop("_srgb_key")
        return df
    else:
        # pandas path
        keys = df["srgb"].apply(lambda v: ",".join(str(int(x)) for x in v))
        df = df.assign(_srgb_key=keys)
        counts = df.groupby("_srgb_key").size().rename("usage").astype(float)
        df = df.merge(counts, on="_srgb_key", how="left")
        df = df.drop(columns=["_srgb_key"])
        return df


def df_len(df) -> int:
    """Row count of a DataFrame, backend-agnostic."""
    return int(df.height) if _BACKEND == "polars" else int(len(df))


def column_list(df, name: str) -> list:
    """Return a column as a plain Python list, backend-agnostic."""
    if _BACKEND == "polars":
        return df.get_column(name).to_list()
    return list(df[name])


def load_manifest(path: str):
    """Load a per-part color manifest into a DataFrame.

    Supports `.ndjson` / `.json` / `.jsonl` (newline-delimited JSON) and
    `.parquet`. Validates the REQUIRED columns and adds a `usage` weight column.
    No `region` column is required or read.
    """
    ext = os.path.splitext(path)[1].lower()

    if ext == ".parquet":
        if _BACKEND == "polars":
            df = pl.read_parquet(path)  # type: ignore[union-attr]
            rows = df.to_dicts()
        elif _BACKEND == "pandas":
            df = _pd.read_parquet(path)  # type: ignore[union-attr]
            rows = df.to_dict(orient="records")
        else:
            raise RuntimeError(
                "Parquet input requires polars or pandas+pyarrow installed."
            )
    elif ext in (".ndjson", ".jsonl", ".json", ".txt"):
        rows = _read_ndjson_pure(path)
    else:
        raise ValueError(
            f"Unsupported manifest extension {ext!r}; expected .ndjson/.jsonl/"
            f".json or .parquet"
        )

    rows = _coerce_rows(rows)
    if not rows:
        raise ValueError(f"manifest {path!r} contained no rows")
    df = _to_dataframe(rows)
    df = _add_usage(df)
    return df
