# Lucide Icons — Eustress Manifest

Hand-authored, MIT-licensed icon set ([lucide.dev](https://lucide.dev))
used across the Eustress toolset. We don't vendor the full 1200+ set
because the subset we use is small; when a new UI surface needs an
icon, download the specific Lucide SVG from
<https://lucide.dev/icons/> and drop it here (or in `../ui/` for
tool-specific custom icons).

## Style contract (matches hand-drawn tool icons)

- Viewbox: `24 24` (ribbon + menu) or `16 16` (cursor badges)
- Stroke width: `1.5` (ribbon) or `1.75` (cursor badges)
- `fill="none"` + `stroke="currentColor"` so `Theme.colorize` can tint
- `stroke-linecap="round"` + `stroke-linejoin="round"`

## Currently used

The following icons live in `../ui/` (project root for icons) and
follow the Lucide naming convention:

### Ribbon / menu
- `swap.svg` — Part Swap
- `folder-open.svg` — file-open cues
- `gap-fill.svg` — Gap Fill (custom hybrid, Lucide-styled)
- `resize-align.svg` — Resize Align (custom)
- `edge-align.svg` — Edge Align (custom)
- `part-swap.svg` — Part Swap tool
- `mirror.svg` — Model Reflect
- `csg-union.svg`, `csg-subtract.svg`, `csg-intersect.svg`,
  `csg-separate.svg` — CSG ops
- `align-left.svg`, `align-center.svg`, `align-right.svg` — alignment
- `distribute.svg` — distribute-evenly
- `array-linear.svg`, `array-radial.svg`, `array-grid.svg` — Pattern
  ribbon group

### Cursor badges
- `cursor-badge-gap-fill.svg`, `cursor-badge-resize-align.svg`,
  `cursor-badge-edge-align.svg`, `cursor-badge-part-swap.svg`,
  `cursor-badge-mirror.svg`, `cursor-badge-material-flip.svg`

## Bulk import procedure

For new UI surfaces that need several stock Lucide icons at once:

```bash
# Replace `box cube chevron-down` with the icons you need.
for icon in box cube chevron-down; do
  curl -sL "https://lucide.dev/api/icons/${icon}" -o "${icon}.svg"
done
```

Normalize stroke-width + color via the contract above. Commit the
SVGs + an update to this manifest in the same change.

## Licensing

Lucide ships under the MIT license. Keep an `ATTRIBUTION.md` entry if
the upstream requirements change; today the license header on each
SVG is sufficient (we preserve it on direct imports).
