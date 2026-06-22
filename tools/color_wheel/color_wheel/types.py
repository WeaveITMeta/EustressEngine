"""Shared types and the canonical region list.

REGIONS is the canonical, ordered set of the seven color wheels. Spelling and
order are a hard contract shared with the Rust importer side.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

# Canonical seven wheels — exact set, spelling, lowercase. Do NOT reorder
# casually: downstream manifest key order and tests depend on this.
REGIONS = ["aether", "halo", "verdure", "stone", "char", "hex", "umbra"]


@dataclass(frozen=True)
class PartColor:
    """One per-part color row as it arrives from the manifest (pre-region).

    Mirrors the NDJSON input schema. `region` is intentionally absent — it is
    derived later in the pipeline, never read from upstream.
    """

    world_id: str
    part_id: int  # u64
    srgb: tuple[int, int, int]  # 0-255 each
    oklch: tuple[float, float, float]  # (L, C, hue_deg)
    roblox_brick: Optional[int]
    class_name: str
    morton: int  # u64


@dataclass(frozen=True)
class NamedColor:
    """A named cluster centroid emitted into the palette manifest."""

    id: str
    name: str
    number: int
    srgb: tuple[int, int, int]
    oklch: tuple[float, float, float]
    good_evil: float  # learned axis projection, [-1, 1]
    abstract_real: float  # learned axis projection, [-1, 1]
