# 14 — Geo & Coordinates

> WGS84 ↔ ECEF transforms, hybrid (planet-relative) coordinates, time-of-day,
> orbital grid LOD, GeoJSON parsing, R-tree spatial indexing.
> Distinct from [05_SPACE_STREAMING](05_SPACE_STREAMING.md) (chunk loading) and
> [13_TERRAIN_VOXEL](13_TERRAIN_VOXEL.md) (terrain content) — this is the
> *math layer* underneath both.

## Pass changelog

- **P3 (2026-05-14):** New doc; 9 features.
- **P4 (2026-05-14):** State correction from secondary critique: `HybridPosition` Component **DOES exist** with `PRECISION_THRESHOLD = 100km`, but **no auto-rebase system runs**. Feature 2 state 🟡 → math layer ✅ / ECS plumbing 🟡 / rebase system 🔴. Time-of-day → sun / moon driver not wired from clock.

---

## Concept summary

The **Geo & Coordinates** subsystem handles coordinate transforms between real-world geodetic space and engine-native local space. WGS84 ↔ ECEF (Earth-Centered Earth-Fixed) via Bowring's iterative method. Hybrid coordinates blend ECEF (world position) with local-tangent-plane Vec3 (engine-friendly). Orbital grid provides LOD for planetary-scale worlds (continent → region → local). GeoJSON parser brings vector geographic data (roads, parcels, polygons) into projects. R-tree gives O(log N) spatial queries.

This is **load-bearing** for any project that uses real-world coordinates: terrain from USGS DEM, road networks from OpenStreetMap, real cities, real flight paths. It's also the foundation for [01_CLIENT_PLAYER] Feature 9 (planetary-scale coords / origin rebasing) and [07_AI_PLATFORM] APEX Pillar B (ECEF + DVec3 WorldPosition).

---

## Implementation snapshot

**Crates / files:**
- [eustress-geo](../../eustress/crates/geo/) — GeoTIFF, GeoJSON, R-tree, `proj` / `flatgeobuf` deps
- [common/src/orbital/wgs84.rs](../../eustress/crates/common/src/orbital/) — geodetic ↔ ECEF transforms (Bowring's method)
- [common/src/orbital/hybrid_coords.rs](../../eustress/crates/common/src/orbital/) — hybrid local-tangent + ECEF
- [docs/development/ORBITAL_GRID.md](../development/ORBITAL_GRID.md), [HYBRID_COORDINATES.md](../development/HYBRID_COORDINATES.md), [LOCAL_GEOSPATIAL.md](../development/LOCAL_GEOSPATIAL.md)

**Working:**
- WGS84 ↔ ECEF
- DVec3 storage for planet-scale positions
- GeoTIFF reader
- R-tree spatial index

**Stubbed / missing:**
- `WorldPosition` ECS component + `sync_render_positions` system ([01_CLIENT_PLAYER] gap)
- Origin rebasing (camera + physics)
- Great-circle distance helper API exposure
- Time-of-day driving orbital phase
- ECEF in Studio rendering (today f32 only)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | WGS84 ↔ ECEF conversion (Bowring) | ✅ |
| 2 | Hybrid coordinates (DVec3 ECEF + Vec3 local) | 🟡 |
| 3 | `WorldPosition` Component + sync system | 🔴 |
| 4 | Origin rebasing (camera + physics) | 🔴 |
| 5 | Time-of-day + sun/moon orbital | 🟡 |
| 6 | GeoJSON parser + Roblox-geometry conversion | 🟡 |
| 7 | GeoTIFF / FlatGeobuf raster import | ✅ |
| 8 | R-tree spatial index for polygon / point queries | ✅ |
| 9 | Orbital grid LOD (continent → regional → local) | 🟠 |

---

## Detailed per-feature cards (top 5)

### Feature 1 — WGS84 ↔ ECEF

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [01], [13], [14], [19]
**Sub-features:** WGS84 datum constants · Bowring iterative geodetic → ECEF · inverse ECEF → geodetic · altitude above ellipsoid · normal vector at point

**Concept.** Standard WGS84 ellipsoid; Bowring's method converges in ~3 iterations. Public API takes (lat, lon, alt) and returns DVec3 ECEF, and vice versa. Used by hybrid coords + GeoTIFF georeferencing.

**Forecasted feedback (R)**
- R1.1 Convergence tolerance default (1e-12 m) is over-precise for game-scale; expose tuning.
- R1.2 Convergence at poles is special — verify.
- R1.3 Alternative datums (NAD27, NAD83) for historical maps — out of scope or worth a feature flag?

**Implications (I)**
- *Architectural:* foundation for every other geo feature; do not break.
- *Cross-system:* [13_TERRAIN] GeoTIFF; [19_REALISM] gravity computation depends on ECEF.
- *Operational:* f64 mandatory — f32 introduces meter-scale error in ECEF.

**Risks (X)** — X1.1 Mixing f32 and f64 silently truncates near poles.

**Mitigations (M)** — M1.1 Strict f64 in geo API surface; convert at engine boundary only.

---

### Feature 2 / 3 / 4 — Hybrid coords + WorldPosition + origin rebasing

**State:** 🟡 hybrid math / 🔴 ECS component + rebase · **Effort:** L · **Risk:** High · **Touches:** [01], [03], [11], [14]
**Sub-features:** `WorldPosition(DVec3)` Component · `CameraWorldOrigin(DVec3)` Resource · `sync_render_positions` system in `Last` schedule · physics in local frame · interpolation across origin rebase

**Concept.** Entities store true world position as f64 ECEF; render-time Transform is f32 relative to a camera-tracking origin that snaps every N km. This avoids f32 jitter at ~10⁵ m. Physics runs in local frame (rebased post-step).

**Forecasted feedback (R)**
- R2.1 Component + system absent today in Client and Studio — only math is done.
- R2.2 Avian step in local frame is fine; rebase must come *after* the step.
- R2.3 One-frame visible artifact at rebase if not in `Last`.
- R2.4 Multiplayer determinism cross-platform via f64 has its own quirks.
- R2.5 Migration: existing projects use f32 Transform; need an "upgrade to planetary mode" flag.

**Implications (I)**
- *Architectural:* opting in changes every gameplay system that reads Transform — audit large.
- *Cross-system:* [01_CLIENT] Feature 9 + [11_SIMULATION] determinism + [03_MULTIPLAYER] prediction all read this.
- *Operational:* f64 doubles per-entity memory for position — bigger ECS.
- *Strategic:* Earth-scale demos are the differentiator; without this they're infeasible.

**Risks (X)**
- X2.1 Cross-system bugs from incorrect Transform vs WorldPosition use.
- X2.2 Rebase during gameplay snaps physics state — could cause solver instability.

**Mitigations (M)**
- M2.1 Single `Transform` → `WorldPosition` migration tool.
- M2.2 Rebase budget: only rebase when camera moves > 1 km since last rebase.

---

### Feature 6 — GeoJSON parser + geometry conversion

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [04_ASSETS], [13_TERRAIN], [14]
**Sub-features:** RFC 7946 features (Point, Line, Polygon, MultiPoint, etc.) · CRS reprojection · Roblox-geometry mapper (line → BasePart strip, polygon → fill mesh) · property tags

**Concept.** Read a `.geojson` file → produce a tree of Eustress instances. A LineString becomes a chain of `BasePart` strips with elevation from heightmap. A Polygon becomes a filled mesh. Properties (name, road type, parcel ID) become metadata.

**Forecasted feedback (R)**
- R6.1 Big GeoJSON (city-scale, 100k features) needs streaming parser.
- R6.2 Property → metadata mapping needs a user-side rule DSL.
- R6.3 Topology preservation (shared edges between adjacent polygons) is hard.

**Implications (I)** — *Strategic:* OpenStreetMap import unlocks real-world cities as projects.

**Risks (X)** — X6.1 Mis-projected coords silently land at (0,0,0).

**Mitigations (M)** — M6.1 Validate import bounding box matches expected region; warn on outlier.

---

### Feature 9 — Orbital grid LOD

**State:** 🟠 · **Effort:** XL · **Risk:** Med · **Touches:** [05_SPACE_STREAMING], [13_TERRAIN], [14]
**Sub-features:** quadtree on ellipsoid surface · per-tile LOD selection · ECEF-friendly tile addressing · day-night masking · cloud layer

**Concept.** A planet-scale tile hierarchy. Surface tiles addressed by (face, x, y, level). LOD chosen by camera distance + viewing angle. Distinct from chunk grid (which is local per-Space).

**Forecasted feedback (R)** — R9.1 Cube-mapped sphere vs. quadrilaterized-sphere — pick one. R9.2 Surface area distortion near corners.

**Implications (I)** — *Cross-system:* [01_CLIENT] f64 WorldPosition + this enable Earth-scale flying / driving demos.

**Risks (X)** — X9.1 Mesh seams across face boundaries are the classic planet-rendering bug.

**Mitigations (M)** — M9.1 Cubic projection with overlap rings.

---

## Wiring / import gaps (top 8)

1. `WorldPosition` Component + `sync_render_positions` system in Client + Studio
2. `CameraWorldOrigin` Resource + rebase trigger
3. Avian local-frame step with post-step rebase
4. Great-circle distance + bearing helpers in public API
5. Time-of-day → sun direction → atmosphere shader
6. GeoJSON streaming parser for big files
7. Orbital quadtree tile addressing
8. Migration tool: f32 Transform → DVec3 WorldPosition

---

## Cross-system dependencies

- **[01_CLIENT]** Feature 9 (planetary coords)
- **[03_MULTIPLAYER]** prediction determinism with f64
- **[05_SPACE_STREAMING]** chunk addressing under ECEF
- **[11_SIMULATION]** physics determinism + replay
- **[13_TERRAIN]** GeoTIFF import + CRS handling
- **[19_REALISM]** gravity per-position computation

---

## Open questions

- Q14.1 ECEF vs. local-tangent-plane as canonical storage — pick.
- Q14.2 Rebase threshold default — 500 m or 1 km?
- Q14.3 Quadtree face count — 6 (cube) or 20 (icosahedron)?
- Q14.4 GeoJSON property → metadata DSL — TOML or scripting?
- Q14.5 Atmospheric scattering — built-in or per-project shader?
