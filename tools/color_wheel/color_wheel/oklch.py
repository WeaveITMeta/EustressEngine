"""sRGB <-> OKLab <-> OKLCH conversions, Ottosson's formulation.

This module is the single source of truth for color math and MUST agree with the
Rust importer that wrote the `oklch` column in the manifest. The constants below
are reproduced verbatim from the Rust contract:

  - sRGB EOTF decode is applied to each 0-255 channel (after /255) BEFORE the
    linear-sRGB -> LMS matrix.
  - linear-sRGB -> LMS matrix M1 (Ottosson).
  - cbrt of LMS, then LMS' -> OKLab matrix M2 (Ottosson).
  - OKLCH = (L, hypot(a, b), atan2(b, a) in degrees, wrapped to [0, 360)).

Pure-Python (math + a thin numpy-free core) so it can be imported with no heavy
deps and unit-tested against known sRGB->OKLCH values.
"""

from __future__ import annotations

import math

# Linear-sRGB -> LMS (Ottosson M1).
_M1 = (
    (0.4122214708, 0.5363325363, 0.0514459929),
    (0.2119034982, 0.6806995451, 0.1073969566),
    (0.0883024619, 0.2817188376, 0.6299787005),
)

# LMS' (cube-rooted) -> OKLab (Ottosson M2).
_M2 = (
    (0.2104542553, 0.7936177850, -0.0040720468),
    (1.9779984951, -2.4285922050, 0.4505937099),
    (0.0259040371, 0.7827717662, -0.8086757660),
)

# Inverse matrices for the OKLab -> sRGB direction.
# OKLab -> LMS' (inverse of _M2).
_M2_INV = (
    (1.0, 0.3963377774, 0.2158037573),
    (1.0, -0.1055613458, -0.0638541728),
    (1.0, -0.0894841775, -1.2914855480),
)
# LMS -> linear-sRGB (inverse of _M1).
_M1_INV = (
    (4.0767416621, -3.3077115913, 0.2309699292),
    (-1.2684380046, 2.6097574011, -0.3413193965),
    (-0.0041960863, -0.7034186147, 1.7076147010),
)

# Hue is undefined for (near-)achromatic colors; clamp to 0 below this chroma.
HUE_EPS = 1e-4


def _srgb_eotf_decode(u: float) -> float:
    """sRGB electro-optical transfer function (gamma decode), 0..1 domain."""
    if u <= 0.04045:
        return u / 12.92
    return ((u + 0.055) / 1.055) ** 2.4


def _srgb_oetf_encode(u: float) -> float:
    """Inverse of :func:`_srgb_eotf_decode` (linear -> gamma)."""
    if u <= 0.0031308:
        return 12.92 * u
    return 1.055 * (u ** (1.0 / 2.4)) - 0.055


def srgb_u8_to_oklab(r: int, g: int, b: int) -> tuple[float, float, float]:
    """Convert an 8-bit sRGB triple (0-255) to OKLab (L, a, b)."""
    lr = _srgb_eotf_decode(r / 255.0)
    lg = _srgb_eotf_decode(g / 255.0)
    lb = _srgb_eotf_decode(b / 255.0)

    l = _M1[0][0] * lr + _M1[0][1] * lg + _M1[0][2] * lb
    m = _M1[1][0] * lr + _M1[1][1] * lg + _M1[1][2] * lb
    s = _M1[2][0] * lr + _M1[2][1] * lg + _M1[2][2] * lb

    l_ = math.copysign(abs(l) ** (1.0 / 3.0), l)
    m_ = math.copysign(abs(m) ** (1.0 / 3.0), m)
    s_ = math.copysign(abs(s) ** (1.0 / 3.0), s)

    okl = _M2[0][0] * l_ + _M2[0][1] * m_ + _M2[0][2] * s_
    oka = _M2[1][0] * l_ + _M2[1][1] * m_ + _M2[1][2] * s_
    okb = _M2[2][0] * l_ + _M2[2][1] * m_ + _M2[2][2] * s_
    return (okl, oka, okb)


def oklab_to_oklch(okl: float, oka: float, okb: float) -> tuple[float, float, float]:
    """Convert OKLab to OKLCH (L, C, hue_deg). Hue clamped to 0 when near-gray."""
    c = math.hypot(oka, okb)
    if c < HUE_EPS:
        return (okl, c, 0.0)
    h = math.degrees(math.atan2(okb, oka))
    if h < 0.0:
        h += 360.0
    return (okl, c, h)


def srgb_u8_to_oklch(r: int, g: int, b: int) -> tuple[float, float, float]:
    """Convert an 8-bit sRGB triple to OKLCH (L, C, hue_deg)."""
    return oklab_to_oklch(*srgb_u8_to_oklab(r, g, b))


def oklch_to_oklab(l: float, c: float, h_deg: float) -> tuple[float, float, float]:
    """Convert OKLCH back to OKLab (L, a, b)."""
    h = math.radians(h_deg)
    return (l, c * math.cos(h), c * math.sin(h))


def oklab_to_srgb_u8(okl: float, oka: float, okb: float) -> tuple[int, int, int]:
    """Convert OKLab to an 8-bit sRGB triple (clamped to 0-255)."""
    l_ = _M2_INV[0][0] * okl + _M2_INV[0][1] * oka + _M2_INV[0][2] * okb
    m_ = _M2_INV[1][0] * okl + _M2_INV[1][1] * oka + _M2_INV[1][2] * okb
    s_ = _M2_INV[2][0] * okl + _M2_INV[2][1] * oka + _M2_INV[2][2] * okb

    l = l_ * l_ * l_
    m = m_ * m_ * m_
    s = s_ * s_ * s_

    lr = _M1_INV[0][0] * l + _M1_INV[0][1] * m + _M1_INV[0][2] * s
    lg = _M1_INV[1][0] * l + _M1_INV[1][1] * m + _M1_INV[1][2] * s
    lb = _M1_INV[2][0] * l + _M1_INV[2][1] * m + _M1_INV[2][2] * s

    out = []
    for lin in (lr, lg, lb):
        enc = _srgb_oetf_encode(max(0.0, min(1.0, lin)))
        out.append(int(round(max(0.0, min(1.0, enc)) * 255.0)))
    return (out[0], out[1], out[2])


def oklch_to_srgb_u8(l: float, c: float, h_deg: float) -> tuple[int, int, int]:
    """Convert OKLCH to an 8-bit sRGB triple (clamped to 0-255)."""
    return oklab_to_srgb_u8(*oklch_to_oklab(l, c, h_deg))
