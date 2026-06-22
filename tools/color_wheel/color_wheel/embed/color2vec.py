"""Skip-gram with negative sampling — numpy only, no torch.

Treats each color token like a word and the morton-adjacency co-occurrence pairs
like (center, context) word pairs. Trains two embedding tables (input/output)
via SGD with negative sampling and returns the input embeddings as the learned
per-token color vectors.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class Color2VecConfig:
    """Hyperparameters for :func:`train_color2vec`."""

    dim: int = 32
    window: int = 5  # advisory; the actual window is applied in tokenize
    negatives: int = 5
    epochs: int = 30
    lr: float = 0.05
    seed: int = 0


def _sigmoid(x: np.ndarray) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-np.clip(x, -30.0, 30.0)))


def train_color2vec(
    pairs: np.ndarray,
    vocab_size: int,
    config: Color2VecConfig = Color2VecConfig(),
    token_freq: np.ndarray | None = None,
) -> np.ndarray:
    """Train skip-gram/neg-sampling embeddings.

    Args:
        pairs: (M, 2) int array of (center, context) token ids.
        vocab_size: number of distinct tokens.
        config: hyperparameters.
        token_freq: optional (vocab_size,) frequency weights used to build the
            negative-sampling distribution (raised to the 0.75 power, the
            classic word2vec unigram smoothing). Uniform if None.

    Returns:
        (vocab_size, dim) float32 array of learned token vectors (L2-normalized).
    """
    rng = np.random.default_rng(config.seed)
    dim = config.dim

    # Xavier-ish small init.
    w_in = (rng.standard_normal((vocab_size, dim)) * (1.0 / np.sqrt(dim))).astype(np.float32)
    w_out = np.zeros((vocab_size, dim), dtype=np.float32)

    if vocab_size <= 1 or pairs.shape[0] == 0:
        # Degenerate corpus: return normalized init so downstream still works.
        return _l2_normalize(w_in)

    # Negative-sampling distribution.
    if token_freq is None:
        freq = np.ones(vocab_size, dtype=np.float64)
    else:
        freq = np.asarray(token_freq, dtype=np.float64) + 1e-9
    neg_p = freq ** 0.75
    neg_p /= neg_p.sum()

    centers = pairs[:, 0]
    contexts = pairs[:, 1]
    m = pairs.shape[0]

    for _ in range(config.epochs):
        perm = rng.permutation(m)
        c = centers[perm]
        ctx = contexts[perm]

        # Draw negatives for the whole epoch at once: (m, negatives).
        negs = rng.choice(vocab_size, size=(m, config.negatives), p=neg_p)

        for i in range(m):
            ci = c[i]
            pi = ctx[i]
            ni = negs[i]

            v_in = w_in[ci]  # (dim,)

            # Positive sample.
            score_pos = float(v_in @ w_out[pi])
            g_pos = _sigmoid(np.array([score_pos]))[0] - 1.0  # label 1

            # Negative samples.
            v_neg = w_out[ni]  # (neg, dim)
            score_neg = v_neg @ v_in  # (neg,)
            g_neg = _sigmoid(score_neg)  # label 0

            # Accumulate gradient on the input vector.
            grad_in = g_pos * w_out[pi] + g_neg @ v_neg

            # Update output vectors.
            w_out[pi] -= config.lr * g_pos * v_in
            w_out[ni] -= config.lr * np.outer(g_neg, v_in)

            # Update input vector last (uses pre-update outputs).
            w_in[ci] -= config.lr * grad_in

    return _l2_normalize(w_in)


def _l2_normalize(x: np.ndarray) -> np.ndarray:
    norms = np.linalg.norm(x, axis=1, keepdims=True)
    norms[norms == 0.0] = 1.0
    return (x / norms).astype(np.float32)
