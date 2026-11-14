# Vehicle Simulator: engine gap log

Engine gaps found while rebuilding Vehicle Simulator in the TerrainBenchmark Space
(`Documents/Eustress/_TerrainLab/Spaces/TerrainBenchmark`). The Space work is staged: roads, city,
chassis, body. Each gap names the stage it blocks, the evidence, and the measurement that closes it.

The Space session adds rows here as it works. The engine session works the rows on a debug build and
fills in the Status column. Repro and verification run on a copy of TerrainBenchmark, never the open
Space.

**Status values:** Open, In progress, Fixed (debug build, with the evidence), Deferred (with the reason).

## Summary

| ID | Gap | Blocks | Status |
|---|---|---|---|
| G-01 | Junction box asphalt does not continue the approach roads' asphalt | Stage 1 roads | Open |
| G-02 | Roads meeting within about 20 m build two boxes with a stub between them | Stage 1 roads | Open |
| G-03 | A through road authored as two splines stops at the side street it passes | Stage 1 roads | Closed in the Space (authoring); the engine side is `Road:Join` in G-05 |
| G-04 | A lane-count change at a junction has no taper | Stage 1 roads | Open |
| G-05 | Roads expose no class methods (Split, Join, ConnectAt, Audit) | Stage 1 roads | Open |
| G-06 | Road asphalt is one 4 m tile with no variation, and a Space cannot replace it | Stage 1 roads | Open |
| G-07 | A script raycast that hits a road returns no Instance | Stage 3 chassis | Open |
| G-08 | A mesh Part draws one primitive with one material | Stage 2 city, Stage 4 body | Open |
| G-09 | A mesh Part collides as a box | Stage 2 city, Stage 4 body | Open |
| G-10 | Assembly mass and wheel spin as scripts read them | Stage 3 chassis | Open (re-verify) |
| G-11 | A spline takes at most 64 points over `terrain.layer_create` | Stage 1 roads | Open |
| G-12 | There is no railway: no track class, bed, crossing or rail bridge | Stage 1 roads (the map's two railways) | Open |
| G-13 | Two road ends that meet at an angle build a junction box with crosswalks and stop bars | Stage 1 roads | Open |
| G-14 | A fork at an acute angle on a steep slope builds a rough box | Stage 1 roads | Open |
| G-15 | A rural road's edge is painted as a row of dirt blobs | Stage 1 roads | Open |
| G-16 | `terrain.replace_material` refuses a material the Space defines | Stage 1 roads (terrain clean-up) | Open |
| G-17 | An Auto road steeper than its class leaves the ground on a viaduct, with no warning | Stage 1 roads (mountain roads) | Open |

## Stage 1: roads and junctions

The network on 2026-11-14: 60 Road-mode splines, 67.1 km, about 140 junctions (113 crossings and 28
end-on-side tees counted from the control polylines). Classes: 9 Arterial, 13 Collector, 16 Local,
17 Driveway, 3 Mountain, 2 Rural. Profiles: 55 Ground, 3 Auto, 2 Explicit.

### G-01 Junction box asphalt does not continue the approach roads' asphalt

- **Seen:** from above, every junction box reads as a different patch of asphalt from the roads feeding
  it, with a line at each mouth. Clear at the Central Ave crossing near (-2428, 420) and the Market St and
  Rail Parkway junction near (-970, 1365).
- **Cause (read in code):** a road's surface UVs are metres across and along the road
  (`common/src/terrain/road_mesh.rs`, the `uvs.push` sites at lines 46 and 87). A junction's are world X
  and Z (`common/src/terrain/road_junction.rs:697-701`). The two texture frames meet at an arbitrary
  offset and angle at each mouth.
- **Needed:** one frame across a mouth. Either every asphalt surface samples in world space (one frame
  for the whole network, also right for G-06's macro variation), or the box blends each leg's frame in
  over the first metres past its mouth.
- **Closed when:** AI camera captures straight down from 120 m over (-2428, 420), (-970, 1365) and
  (-730, 100) show no line or tone step at any mouth, and a driver-height capture down each leg shows
  none either.

### G-02 Roads meeting within about 20 m build two boxes with a stub between them

- **Seen:** near (-730, 100) Campus Loop, Campus North Rd and North Loop meet as two junction boxes
  about 20 m apart. The stub between them carries two crosswalks back to back and a stop bar.
- **Input there:** three road ends share the point exactly (Campus Loop, Campus North Rd, North Loop,
  0 m apart in the control points), and the capture shows four legs, so one road reaches the node
  twice. Campus Loop is the likely one (a loop whose two ends meet at its own junction); to verify.
- **Rule today:** ends gather within 12 m (`docs/design/ROAD_SYSTEM.md`, Junctions).
- **Needed:** junctions whose boxes or setbacks would touch or leave less than a usable block between
  them (under about 40 m centre to centre) merge into one multi-leg junction.
- **Closed when:** the capture over (-730, 100) shows one box with five legs and no stub.

### G-03 A through road authored as two splines stops at the side street it passes

- **Seen:** near (-970, 1365) Market St and Rail Parkway (both Arterial, 7 degrees apart) meet end to
  end where a side street tees in. The through movement gets a crosswalk and stop bar on both sides and
  the centre line ends.
- **Result (Space session, 2026-11-14):** this is authoring, and the engine builds it correctly from one
  spline. The two splines were disabled and one road, `CF_City_Market_Rail_Parkway`, was created through
  the same eight points. The junction rebuilt as a tee with the far kerb and sidewalk running straight
  through. Its crosswalks and stop bars on the through legs are the signalised Arterial with Arterial
  rule, as designed.
- **Engine side:** no junction rule is needed. `Road:Join` (G-05) and the point cap (G-11) are what let a
  long road stay one spline.
- **Open question for McKale (engine session, 2026-11-14):** `ROAD_SYSTEM.md` says signals go "where two
  through roads (collector or larger) cross" and that "the stem stops at a T of equals". The control rule
  in `road_junction.rs` counts every collector-or-larger leg toward signals, a tee's stem included, so an
  Arterial teeing into an Arterial is signalised rather than stop-controlled on its stem. Which is meant:
  the doc (the stem stops, the through road keeps priority) or the code (signals)? No code changed.

### G-04 A lane-count change at a junction has no taper

- **Seen:** near (-1100, 1870) Commerce Rd and South Boulevard: a six-lane road with parking ends
  against a two-lane road across a triangular box. Six lanes arrive at a mouth two lanes wide.
- **Needed:** lane sections (phase 4 of the road system): a lane drop or add is a width transition
  ahead of the junction, with its taper length from the design speed.
- **Closed when:** every lane entering a box has a lane to drive into on some exit, reported per junction
  by the audit (G-05).

### G-05 Roads expose no class methods

- **Seen:** `get_properties` on `Workspace/Terrain/Layers/CF_City_Central_Ave` returns `methods: []`.
  `ROAD_SYSTEM.md` specifies `Road:Split`, `Road:Join`, `Road:ConnectAt` and `Road:Audit`.
- **Needed first:** `Road:Audit`, returning per road and per junction: radius and grade violations,
  the step in height between each leg and the box at its mouth, lanes with no exit, junctions closer
  than the merge distance, and structures. The Space session then works the network against numbers
  instead of captures. Then `Join` and `Split`.
- **Also wanted (2026-11-14):** a way to replace a road's points as a set (`Road:SetPoints`, or points
  accepted by `set_properties`). A road whose points are wrong is disabled and written again under a new
  name today, which leaves the old layer in the Space.
- **Closed when:** `call_method` runs `Road:Audit` on a road in a copy of TerrainBenchmark and the
  numbers match a hand check at two junctions.

### G-06 Road asphalt is one 4 m tile with no variation, and a Space cannot replace it

- **Seen:** from the air the carriageway reads as uniform speckle; at driver height the tile repeats.
  There is no wear in the wheel paths, no patching, cracking or staining.
- **Cause (read in code):** `ASPHALT_TILE_METRES = 4.0` and the two built-in looks `asphalt` and
  `asphalt_worn` (`common/src/terrain/road_surface.rs:95`, `:1063-1064`).
- **Needed:** (a) a macro variation layer in world space over the tile, wheel-path wear across the
  lane, and a detail normal; (b) each road look resolves through the Space's MaterialService, so a Space
  supplies its own PBR sets (albedo, normal, roughness, height) per road class and for kerb, sidewalk
  and paint.
- **Closed when:** a Space-supplied asphalt set shows on a designed road after a reload with no engine
  change, and a 120 m top-down capture shows no visible tiling.
- **Status (engine session, 2026-11-14): Deferred.** G-01 laid the ground for it: every road and junction
  surface now samples in world metres, so a macro layer has one frame across the network. The rest is
  two pieces of work this pass did not take on. (a) The variation and wheel-path wear need a road
  material of their own (an extended `StandardMaterial` with a world-space macro noise, a lane-relative
  wear mask and a detail normal) in place of the plain tiled one at `road_surface.rs:1086-1127`. (b) A
  Space's own sets need a naming contract: which MaterialService names a road class, kerb, sidewalk and
  paint resolve to. That contract is part of the Space format, so it is McKale's call before any code.

### G-11 A spline takes at most 64 points over `terrain.layer_create`

- **Source:** `ROAD_SYSTEM.md`, "Where roads stand today". A road longer than about a kilometre becomes
  several splines, which then meet as junctions (G-03).
- **Needed:** points numbered once when the road is written.
- **Cost today:** the mountain pass (2026-11-14) is 86 control points over 8.9 km and is two splines; where
  they meet is G-13 unless a third road tees in there or the two share a heading.

### G-13 Two road ends that meet at an angle build a junction box with crosswalks and stop bars

- **Seen (2026-11-14):** `CF_Coast_Spine_South` ended and `CF_South_Trunk_West` began at (-2195, 4775),
  about 25 degrees apart, both Arterial, Profile Auto, no third road. The engine built a box with a
  crosswalk and a stop bar on each leg and a kink between them. Written as one spline through the same
  points (`CF_Coast_Spine_South_Trunk`), the road runs through on a curve with its centre line unbroken.
- **Ends in line join cleanly:** `CF_City_Market_Rail_Parkway` (city Arterial, sidewalk and parking) and
  `CF_Valley_Road` (Arterial, shoulders) meet end to end at (600, 1720) on one heading. Lanes and the
  centre line continue with no box. The cross-section changes with no taper: the sidewalk, kerb and
  parking lane stop dead at the joint.
- **Needed:** two ends with no third leg are a continuation. One alignment is fitted through the joint
  (a curve where the headings differ), with no box, crosswalk or stop bar, and the cross-section changes
  over a taper. With the 64-point cap (G-11) every long road has such joints.
- **In line, three more joints (2026-11-14):** the second spline was started 150 m along the first's
  heading. `CF_Valley_Road` into `CF_Canyon_Viaduct` at (2205, 1975) and `CF_Coast_Spine_South_Trunk`
  into `CF_South_Trunk_East` at (1216, 5056) built no box. `CF_North_Trunk` into `CF_North_Tunnel` at
  (1100, -4200) built no box but shows a seam: the lane lines step sideways and a sliver of bare bed
  shows beside the carriageway for about 30 m.
- **Closed when:** two Arterial splines meeting at 25 degrees with no third road build no box, and a
  capture down the centre line shows it unbroken through the joint.

### G-14 A fork at an acute angle on a steep slope builds a rough box

- **Seen (2026-11-14):** at (1442, -938), 340 m up the range's west face, `CF_Pass_Road_North` ends,
  `CF_Observatory_Road` carries on up the slope and `CF_Pass_Road_Valley` leaves about 40 degrees to its
  right, going down (the engine put it on a viaduct). All three are Mountain, Profile Auto. The box is a
  small patch with terrain paint showing in squares around it, and the north approach shows about 80 m of
  plain grey with no markings before the box (to verify: the surface is under the ground, or the leg's
  setback is that long).
- **Needed:** a fork where one leg climbs and one falls away needs the legs to share a surface for the
  length it takes them to separate by a lane width and a kerb, then split, with the cut and fill of both
  resolved together.
- **Closed when:** a driver-height capture from each of the three legs shows a continuous surface with
  markings through the fork, and a car drives each of the three movements without leaving the asphalt.

### G-15 A rural road's edge is painted as a row of dirt blobs

- **Seen (2026-11-14):** along `CF_Coast_Spine_North` near (-3375, -2020), on flat grass, both edges of
  the road carry round patches of Dirt about every 15 m instead of a continuous verge. `terrain_query`
  reports Dirt at the road's edge there. The same scallops show along `CF_Valley_Road`.
- **Needed:** the shoulder and verge as a continuous strip at the road's own width, at the terrain's
  material resolution or drawn by the road's mesh.
- **Closed when:** a 60 m oblique capture along a rural Arterial on flat ground shows an even edge.

### G-16 `terrain.replace_material` refuses a material the Space defines

- **Seen (2026-11-14):** replacing Asphalt with `Lawn` in TerrainBenchmark was refused as an unknown
  material in `to`, the reply listing only the 23 built-in names. The Space defines Cropland (slot 23), Fallow (24),
  Airfield (25) and Lawn (26) in `Workspace/Terrain/materials`, and `terrain.paint` accepts them.
- **Cost:** clearing old road paint out of crop fields took a replace to a built-in material and then
  35 round brush strokes sized by hand to stay inside each field.
- **Needed:** `from` and `to` accept the same names `terrain.paint` does.
- **Closed when:** a box replace from Asphalt to Cropland changes the cells and `terrain_query` reports
  Cropland there.

### G-17 An Auto road steeper than its class leaves the ground on a viaduct, with no warning

- **Seen (2026-11-14):** `CF_North_Pass` (Mountain, Profile Auto, 44 points, now disabled) was routed along
  the ground at the class's full 10% with hairpins whose two ends differ by up to 24 m of ground height.
  West of the saddle it built on the ground. East of it, about 1.5 km and 170 m of descent, the engine
  carried the whole switchback stack on piers, the deck far above the slope. Nothing in the Output panel
  said the alignment could not be graded.
- **What works:** an Explicit profile with a tunnel. `CF_North_Trunk` passes the spur ridge at (0, -4050)
  through two tunnels (about 100 m and 310 m) with an open cut between and crosses the lake arm on a
  pier bridge; `CF_North_Tunnel` passes the range in one tunnel of about 1.2 km. Portals, lining and lane
  markings built as designed.
- **Needed:** when an Auto profile cannot hold the ground within the class's grade, the road says so
  (`Road:Audit` and a warning naming the stations) and offers the choices: more length, a steeper
  short grade through the hairpins, or a tunnel. A viaduct longer than a set length on a hillside (no
  water or valley under it) is reported, not built silently.
- **Closed when:** baking that alignment on a copy of TerrainBenchmark prints a warning with the
  stations and the grade it would need, and the audit lists the viaduct.

### G-12 There is no railway

- **Seen:** the Coastal Frontier map carries two railways: the wide band along the foot of the range on
  the east side, north to south (confirmed a railway by McKale, 2026-11-14), and the diagonal freight
  line with the port sidings. `TerrainSpline` modes are Road, Path, River, Canyon and Embankment.
- **Needed:** a Rail mode or class beside Road: a graded formation with ballast, sleepers and rails for
  one to four tracks, rail bridges and tunnels on the road structures' kit, and level crossings where a
  road meets a track at grade (gates, lights, the road's surface carried across the rails). Trains
  themselves are later work.
- **Closed when:** a two-track spline 1 km long builds its bed and tracks on a copy of TerrainBenchmark,
  and a Collector crossing it builds a level crossing a car can drive over.
- **Status (engine session, 2026-11-14): Deferred, needs McKale.** A Rail mode is a new `SplineMode`
  value (`common/src/terrain/layers.rs:113`), written to every rail spline's file: a Space file format
  change, which the brief reserves for McKale. It is also a feature the size of the road system.

## Stage 2 and 4: city buildings and vehicle bodies

### G-08 A mesh Part draws one primitive with one material

- **Source:** the `import_asset` tool: "Studio draws a mesh Part from the GLB's first mesh, its first
  primitive and its first material, without node transforms." The Coupe body is split into one GLB per
  material for this reason (`Assets/Meshes/vcar3_Color.glb`, `vcar3_Glass.glb` and the rest).
- **Needed:** every primitive of a GLB drawn with its own material and its node transform, with the
  GLB's PBR textures (base colour, normal, metallic-roughness, emissive, occlusion). To verify: which
  texture maps the loader keeps today.
- **Closed when:** one multi-material GLB (a building with glass, concrete and metal; a car body with
  paint, glass, lights and trim) draws as authored from a single Part.
- **Verified in code (2026-11-14):** `engine/src/space/instance_loader.rs:3011-3012` loads
  `#Mesh0/Primitive0` and `#Material0/std` only, with no node transform. The material is Bevy's glTF
  `StandardMaterial`, so the one material it does load keeps every map Bevy reads (base colour, normal,
  metallic-roughness, emissive, occlusion).
- **Status (engine session, 2026-11-14): Deferred.** Drawing every primitive means a mesh Part becomes
  one entity per primitive under the Part, each with its node's transform, and every place that assumes
  a Part has one mesh handle (the resident mesh cache, picking, bounds, selection outline, the mesh
  collider G-09 needs) follows. It is a loader feature of its own, bigger than this pass.

### G-09 A mesh Part collides as a box

- **Source:** the `import_asset` tool: "Collision today is a box the size of `scale` in metres at the
  Part's origin, not the mesh's shape."
- **Needed:** a collision choice per mesh Part: box, convex hull, convex decomposition, or the triangle
  mesh for anchored parts. Buildings with setbacks, arcades and podiums, and car bodies, need it.
- **Closed when:** a car driven at an anchored L-shaped building mesh stops at the wall it drew, on both
  inner faces.
- **Verified in code (2026-11-14):** a mesh Part's collider is a box or primitive from its size
  (`safe_collider_from`, `part_draw::shape_collider`). `CollisionFidelity` is declared
  (`common/src/classes.rs:8051`) and nothing reads it.
- **Status (engine session, 2026-11-14): Deferred.** The choice belongs on the existing
  `CollisionFidelity` (Box, Hull, Default as a coarse convex decomposition, PreciseConvexDecomposition),
  built from the same primitives G-08 draws, so it follows G-08. The exact triangle mesh for anchored
  parts would be a new value, a Space format change, so McKale decides that one.

## Stage 3: chassis

### G-07 A script raycast that hits a road returns no Instance

- **Seen:** Output, 2026-11-13 7:03 PM: `[ROADPROBE] x -1001.9 z 697.8: road collider top 22.237
  (outside the tree); terrain collider 21.839; gap 0.397 m`. The probe is
  `ServerScriptService/Chassis/Playground/Playground.server.luau:1553-1562`; "outside the tree" is its
  text for `result.Instance == nil`.
- **Needed:** a ray or contact on a road surface reports the road instance and the surface under the
  hit (carriageway, shoulder, kerb, sidewalk, paint) with its material, so a tyre's grip and its sound
  follow the surface. Terrain already reports `workspace.Terrain` and a material.
- **Closed when:** the same probe prints the road's full name and a material on a designed road.

### G-10 Assembly mass and wheel spin as scripts read them (re-verify on the current build)

From `Docs/CarEvaluation.md` in the Space (the Coupe on SimChassis 4.0):

- A welded body of 1,515 kg design mass reads `AssemblyMass` 68.3 kg, so the script scales its springs
  and air by 0.045.
- A wheel's angular velocity is not readable from scripts, so wheelspin and a locked wheel look the same
  as rolling.
- Rune's `dm::delta()` and `dm::now()` are virtual time capped at 33 ms.

Each needs a check on the current build before any work: read the mass of a known assembly and a spun
wheel's angular velocity from Rune in Play.
