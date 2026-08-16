# Grid-Cell Coordinates

**Status: design. Not implemented.** No code in the workspace implements this
yet. Everything below specifies what to build; the "What exists today" section
marks the seams it would attach to.

The goal is a single Space that stays exact at planetary extents while
rendering and simulating in `f32` — i.e. large worlds without a floating-point
rewrite of Bevy or Avian.

## Table of Contents

1. [The problem](#the-problem)
2. [Why not f64](#why-not-f64)
3. [The design](#the-design)
4. [Precision budget](#precision-budget)
5. [Choosing the step](#choosing-the-step)
6. [What exists today](#what-exists-today)
7. [Integration points](#integration-points)

---

## The problem

Engine-native coordinates are `f32` metres (`common/src/units.rs`). `f32`
carries 24 bits of mantissa, so absolute precision degrades linearly with
distance from the origin: the gap between representable values at distance
`d` is `d × 2⁻²³`.

| Distance from origin | ULP | Practical effect |
|---|---|---|
| 1 km | 0.06 mm | fine |
| 8 km | 1 mm | fine |
| 65 km | 8 mm | visible vertex swim |
| 262 km | 31 mm | obvious jitter |
| 6,371 km (Earth radius) | 0.76 m | unusable |

Two distinct failures hide behind "precision":

- **Storage precision** — a position too large to represent exactly.
- **Cancellation** — `vertex_world − camera_world` computed in `f32` when both
  operands are large. This is the one that actually produces visible jitter,
  and it appears well before storage precision runs out.

A grid-cell system fixes both, because the subtraction happens in the integer
cell domain and the `f32` operands are always cell-local and small.

## Why not f64

Converting the engine to `f64` is not a refactor:

- Bevy's `Transform` / `GlobalTransform` are `f32` `Affine3A`. There is no
  `f64` feature; the type is `f32` all the way into the render world.
- GPU vertex buffers and WGSL are `f32`. Consumer GPUs execute `fp64` at 1/32
  to 1/64 rate, so an `f64` render path is not merely invasive but slow.
- Avian *does* offer an `f64` feature, but the workspace pins
  `"f32", "parry-f32"` (`eustress/Cargo.toml`). Enabling `f64` there yields
  `f64` physics feeding `f32` transforms — double the solver bandwidth for a
  value that is truncated on the way to the renderer.

`f64` is also the weaker answer on its own merits. It moves the error rather
than removing it: precision still varies with magnitude, and accumulated drift
still exists, just further out. A grid cell plus a local offset is **exact** —
the cell index carries no error at all, and the offset's precision is uniform
everywhere in the world.

## The design

A position is a pair:

```rust
/// Which cell. Exact, no floating point.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Where inside that cell, in metres. Always small.
/// This is the existing Bevy `Transform.translation`.
```

Invariant: `|offset| ≤ cell_size`, restored by a *rebase* whenever an entity
crosses a boundary. Absolute position is `cell × cell_size + offset`, but that
value is only ever materialised in `f64` for reporting — never for rendering.

Rendering and physics are **camera-relative**: each frame, the renderer
subtracts the camera's `GridCell` from every visible entity's `GridCell` in
integer arithmetic, then adds the `f32` offsets. Both operands of the final
`f32` subtraction are cell-local, so there is no cancellation regardless of how
far the camera is from the world origin.

Physics stays `f32` and unmodified: Avian only ever sees one cell neighbourhood
at a time, which is by construction a small coordinate range.

## Precision budget

At `cell_size = 256 m`, an `f32` offset has a worst-case ULP of
`256 × 2⁻²³ ≈ 0.03 mm`, **uniformly, at any world extent**.

With `i32` cell indices the addressable world is `2³¹ × 256 m ≈ 5.5 × 10¹¹ m`
— roughly 3.7 astronomical units. `i64` is available if that is ever the
binding limit; it will not be.

Compare against storing the same range in `f32`: at Earth radius the ULP is
0.76 m, a factor of ~25,000 worse, and it keeps degrading.

## Choosing the step

If positions are also **quantised for storage** (see the entity-format work),
the fixed-point step must be a binary fraction of the **authoring unit**, not
of the metre.

This project's convention is `1 stud ≡ 1 ft`. Under a metric step of `2⁻¹⁰ m`,
foot-grid content lands on 312.1152 steps per grid unit — 256 distinct low
bytes and ~7.92 bits of entropy, which destroys the all-zero low byte-plane
that makes the column compress. Under `step = 1 ft / 2⁸ ≈ 1.1906 mm`, the same
content produces exactly one distinct low byte.

| Bits/axis over 256 m | Step | Max error | Bytes (3 axes) |
|---|---|---|---|
| 16 | 3.906 mm | 1.953 mm | 6.00 |
| **18** | **0.977 mm** | **0.488 mm** | **6.75** |
| 21 | 0.122 mm | 0.061 mm | 7.88 |
| 24 | 0.0153 mm | 0.0076 mm | 9.00 |

24-bit fixed-point matches `f32`'s own worst-case absolute error over a 256 m
span (both 7.63 µm) in 25% fewer bits, and unlike `f32` the precision does not
depend on where in the cell the point falls.

## What exists today

The grid already exists as a **storage key** — the work is promoting it to a
**coordinate**.

- `worlddb/src/keys.rs` — `MortonKeyEncoder`, 21 bits per axis with a `1 << 20`
  bias, `chunk_size = 256.0`. `world_to_cell` / `cell_to_world_min` /
  `cell_world_aabb` are the conversion surface.
- `engine/src/space/residency.rs` — camera-locality spawn/evict already
  operates in cell units with load/evict hysteresis.
- `engine/src/space/hlod.rs` — merged-cell proxies, already per-cell.

The seam to change: `world_to_cell` takes `coord: f32`, i.e. it derives a cell
*from* a global `f32` position. Under this design the relationship inverts —
the cell is authoritative and the `f32` offset is cell-local, so nothing ever
needs a global `f32` coordinate to exist. Every call site that currently
converts a world-space `f32` to a cell is a site this design removes.

## Integration points

Ordered by how much they constrain the rest:

1. **`GridCell` component + rebase system.** Rebase on boundary crossing;
   run before transform propagation.
2. **Camera-relative view matrices.** The renderer subtracts cell indices in
   integer space. This is what actually kills the jitter.
3. **Raycasting, gizmos, selection.** Every API taking a "world position"
   becomes cell-aware. This is the bulk of the work and the main source of
   subtle bugs — a raycast that silently uses a stale cell is hard to see.
4. **Physics sync.** Avian bodies live in cell-local space; the sync layer
   translates on rebase. Rebasing a body mid-solve must preserve velocity.
5. **Persistence.** `ArchTransform` stores `[f32; 3]` today; it gains the cell
   index and the offset becomes quantised fixed-point.

Prior art worth reading before implementing: the `big_space` crate takes this
exact approach against Bevy and is the reference for how the rebase and
propagation ordering interact.
