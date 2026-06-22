"""Learned semantic axes recovered from the color2vec embedding.

A :class:`SeedAxis` is defined by two small lists of seed colors (positive and
negative anchors). Each seed color is mapped to its nearest vocabulary token
(by OKLab distance to the token centroid), and the axis *direction* in embedding
space is::

    dir = normalize( centroid(embeddings[pos_tokens]) - centroid(embeddings[neg_tokens]) )

Every token is then projected onto this direction (dot product) and the
projections are rescaled to [-1, 1] using a robust max-abs scale (the 98th
percentile of |projection|, so a single outlier cannot flatten everything).

Two axes are provided with built-in seeds:
  * good_evil    — bright luminous colors are "good"; chaotic dark colors "evil".
  * abstract_real — pure saturated primaries are "abstract"; muddy naturalistic
                    colors are "realistic".
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from .oklch import srgb_u8_to_oklab

# ---------------------------------------------------------------------------
# Built-in seed palettes (sRGB 0-255). Chosen to be unambiguous anchors.
# ---------------------------------------------------------------------------

# good = bright, luminous, warm/clean light
GOOD_SEEDS = [
    (255, 255, 255),  # pure white
    (255, 248, 220),  # warm cream
    (255, 223, 120),  # golden light
    (173, 216, 230),  # sky blue
    (255, 215, 0),    # gold
    (240, 255, 240),  # honeydew glow
    (255, 250, 205),  # lemon chiffon
]
# evil = chaotic, dark, sickly
EVIL_SEEDS = [
    (10, 10, 10),     # near-black
    (40, 0, 0),       # blood-dark red
    (30, 0, 40),      # bruised purple
    (20, 30, 10),     # rotten green-black
    (60, 0, 20),      # dark crimson
    (15, 15, 25),     # cold black-blue
    (45, 20, 0),      # charred brown
]

# abstract = pure saturated primaries / electric hues
ABSTRACT_SEEDS = [
    (255, 0, 0),      # pure red
    (0, 255, 0),      # pure green
    (0, 0, 255),      # pure blue
    (255, 0, 255),    # magenta
    (0, 255, 255),    # cyan
    (255, 255, 0),    # yellow
    (255, 0, 128),    # hot pink
]
# realistic = muddy, naturalistic, earthy
REALISTIC_SEEDS = [
    (110, 90, 70),    # mud brown
    (90, 100, 70),    # olive
    (130, 120, 100),  # stone tan
    (80, 70, 60),     # bark
    (100, 110, 120),  # weathered slate
    (120, 100, 80),   # clay
    (95, 105, 85),    # moss
]


@dataclass
class SeedAxis:
    """A semantic axis learned from positive/negative seed colors."""

    name: str
    pos_seeds: list[tuple[int, int, int]]
    neg_seeds: list[tuple[int, int, int]]
    direction: np.ndarray | None = field(default=None)
    _scale: float = field(default=1.0)

    def _seeds_to_tokens(self, seeds, centroids: np.ndarray) -> list[int]:
        """Map each seed color to its nearest token centroid (OKLab distance)."""
        toks = []
        for (r, g, b) in seeds:
            lab = np.asarray(srgb_u8_to_oklab(int(r), int(g), int(b)), dtype=np.float32)
            d = np.linalg.norm(centroids - lab[None, :], axis=1)
            toks.append(int(d.argmin()))
        return toks

    def fit(self, embeddings: np.ndarray, centroids: np.ndarray) -> "SeedAxis":
        """Compute the axis direction and the robust projection scale.

        embeddings: (V, dim) learned token vectors.
        centroids:  (V, 3) OKLab token centroids (for nearest-token seed mapping).
        """
        pos_tok = self._seeds_to_tokens(self.pos_seeds, centroids)
        neg_tok = self._seeds_to_tokens(self.neg_seeds, centroids)

        pos_c = embeddings[pos_tok].mean(axis=0)
        neg_c = embeddings[neg_tok].mean(axis=0)
        direction = pos_c - neg_c
        norm = np.linalg.norm(direction)
        if norm < 1e-12:
            # Degenerate (seeds landed on same tokens) — fall back to first axis.
            direction = np.zeros(embeddings.shape[1], dtype=np.float32)
            if embeddings.shape[1] > 0:
                direction[0] = 1.0
            norm = 1.0
        self.direction = (direction / norm).astype(np.float32)

        proj = embeddings @ self.direction
        scale = float(np.percentile(np.abs(proj), 98))
        self._scale = scale if scale > 1e-9 else 1.0
        return self

    def project(self, vectors: np.ndarray) -> np.ndarray:
        """Project token vectors onto the axis, scaled to [-1, 1]."""
        if self.direction is None:
            raise RuntimeError(f"axis {self.name!r} not fitted")
        proj = vectors @ self.direction
        return np.clip(proj / self._scale, -1.0, 1.0)

    def project_one(self, vector: np.ndarray) -> float:
        """Project a single token vector to a scalar in [-1, 1]."""
        return float(self.project(vector[None, :])[0])


def build_axes(embeddings: np.ndarray, centroids: np.ndarray) -> dict[str, SeedAxis]:
    """Fit and return the two canonical semantic axes."""
    good_evil = SeedAxis("good_evil", GOOD_SEEDS, EVIL_SEEDS).fit(embeddings, centroids)
    abstract_real = SeedAxis(
        "abstract_real", ABSTRACT_SEEDS, REALISTIC_SEEDS
    ).fit(embeddings, centroids)
    return {"good_evil": good_evil, "abstract_real": abstract_real}
