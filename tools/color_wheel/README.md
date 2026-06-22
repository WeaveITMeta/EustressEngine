# color-wheel

Learns **named color wheels** and **two semantic axes** from the per-part color
manifest emitted by the Eustress Roblox importer, and writes
`palette_manifest.json`.

This tool is fully independent of the Rust engine: it lives outside the cargo
workspace and uses its own Python virtual environment.

## What it produces

Seven canonical, named color wheels plus two learned axes per color:

- **Wheels** (exact ids, lowercase): `aether, halo, verdure, stone, char, hex, umbra`
- **Axes**: `good_evil` and `abstract_real`, each a scalar in `[-1, 1]`.

`halo` clusters are named after the 72 Shem ha-Mephorash angels (in order);
`umbra` clusters after the 72 Ars Goetia demons (in order). The other five wheels
use templated names like `Lucent Azure 03`.

## Pipeline

```
load -> tokenize (k-means vocab over OKLab)
     -> color2vec embed (numpy skip-gram + negative sampling)
     -> recover semantic axes (seed-color directions in embedding space)
     -> derive region (from the two axes + OKLCH lightness/chroma)
     -> cluster per region (frequency-weighted KMeans, sample_weight = usage)
     -> name clusters
     -> write palette_manifest.json
```

## Input column contract

NDJSON by default (one JSON object per line); Parquet is also supported. **Field
names are the contract** — they must match what the Rust importer writes:

| field          | type                    | notes                                  |
|----------------|-------------------------|----------------------------------------|
| `world_id`     | str                     |                                        |
| `part_id`      | int (u64)               |                                        |
| `srgb`         | `[int, int, int]`       | 0-255 per channel                      |
| `oklch`        | `[float, float, float]` | `[L, C, hue_deg]`                      |
| `roblox_brick` | int \| null             |                                        |
| `class`        | str                     | Roblox class name                      |
| `morton`       | int (u64)               | spatial Morton code (neighbor source)  |

There is **no `region` column upstream** — region is *derived* in this pipeline
from the two learned axes plus OKLCH lightness/chroma:

- low chroma / neutral → `stone`
- high lightness + good → `halo`; low lightness + evil → `umbra`
- good & abstract → `aether`; good & realistic → `verdure`
- evil & abstract → `hex`; evil & realistic → `char`

A `usage` weight column is added at load time (each row weight 1.0, aggregated by
identical color) and used as the clustering `sample_weight`.

## OKLCH math

Ottosson's OKLab/OKLCH, with the sRGB EOTF decode applied per channel **before**
the linear-sRGB→LMS matrix. The matrices and the OKLab coefficients are
reproduced verbatim from the Rust side in `color_wheel/oklch.py`, so the OKLCH
computed here agrees with the manifest's `oklch` column.

## Install & run

```bash
# from the repo root: E:/Workspace/EustressEngine
py -m venv tools/color_wheel/.venv
tools/color_wheel/.venv/Scripts/python -m pip install -r tools/color_wheel/requirements.txt

# run the pipeline
tools/color_wheel/.venv/Scripts/python -m color_wheel \
    --input color_manifest.ndjson --output palette_manifest.json
```

Or, with the package on `PYTHONPATH` / installed (`pip install -e tools/color_wheel`):

```bash
python -m color_wheel --input color_manifest.ndjson --output palette_manifest.json
# or the console script:
color-wheel --input color_manifest.ndjson --output palette_manifest.json
```

### CLI flags

```
--input/-i    manifest path (.ndjson or .parquet)   [required]
--output/-o   output palette_manifest.json path      [required]
--dim         embedding dimension                    (default 32)
--seed        RNG seed                               (default 0)
--weight      frequency-weight column for clustering (default 'usage')
--buckets     vocab size / k-means buckets           (default 64)
--epochs      color2vec epochs                        (default 30)
--window      morton-adjacency window                 (default 4)
--k           clusters per region                     (default: auto via silhouette)
```

## Output shape

`palette_manifest.json`:

```json
{
  "meta": {
    "version": "0.1.0",
    "source": "color_manifest.ndjson",
    "generated_at": "<ISO-8601, supplied by the caller>",
    "embed_dim": 32,
    "rows": 84,
    "regions": ["aether","halo","verdure","stone","char","hex","umbra"],
    "wheel_counts": { "aether": 3, "halo": 2, ... }
  },
  "wheels": {
    "aether": [
      { "id": "aether-01", "name": "Lucent Azure 01", "number": 1,
        "srgb": [r,g,b], "oklch": [L,C,hue], "good_evil": 0.71, "abstract_real": 0.62 }
    ],
    "halo": [...], "verdure": [...], "stone": [...],
    "char": [...], "hex": [...], "umbra": [...]
  }
}
```

The builder is pure: `generated_at` is passed in by the caller (the CLI uses
`datetime.now(UTC)`), never computed inside `build_manifest`.

## Seed provenance

The two axes are learned, not hand-coded — only their *anchors* are built in:

- **good** = bright, luminous, clean light (whites, warm creams, golden/sky tones).
- **evil** = chaotic, dark, sickly tones (near-blacks, blood/bruise hues).
- **abstract** = pure saturated electric primaries (RGB/CMY corners).
- **realistic** = muddy naturalistic earth tones (mud, olive, slate, clay, moss).

Each seed color is mapped to its nearest vocabulary token; the axis direction is
`normalize(centroid(pos) − centroid(neg))` in color2vec embedding space, and
token projections are robustly rescaled to `[-1, 1]`. Seed lists live in
`color_wheel/axes.py`.

The angel/demon name lists live in `color_wheel/data/{angels_72.json,
demons_72.json}` (Shem ha-Mephorash and Ars Goetia, standard transliterations,
ordered 1..72).

## Dependency fallbacks

- **DataFrame backend**: polars is preferred. If polars cannot be imported, the
  loader automatically falls back to pandas (install the `pandas` extra). NDJSON
  parsing additionally has a pure-stdlib path that needs neither.
- **Clustering / vocab**: scikit-learn is preferred for KMeans + silhouette. If
  it is unavailable, a pure-numpy weighted Lloyd's k-means is used (auto-k falls
  back to a heuristic).
- **Parquet** input requires polars or pandas+pyarrow.

If a dependency could not be installed in your environment, the lightest path
that still runs is selected automatically; see the import guards in
`color_wheel/loader.py`, `embed/tokenize.py`, and `cluster.py`.
