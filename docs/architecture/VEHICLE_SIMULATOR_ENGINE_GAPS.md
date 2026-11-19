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
| G-18 | A road draws no surface between a tunnel or deep cut and its end; the portal there is a blank wall | Stage 1 roads (tunnels, mountain roads) | Open |
| G-19 | Two splines joined in line meet with a kink, and one side's surface is half covered by the ground | Stage 1 roads | Open |
| G-20 | `create_entity` stores a 0-255 colour unscaled, so the part renders blown-out white | Stage 2 city | Open |
| G-21 | A material first read while its textures are being written stays blank, and nothing reloads it | Stage 2 city | Open |
| G-22 | `set_properties` refuses `Material`, and `get_properties` shows Plastic for a Space-defined material | Stage 2 city | Open |
| G-23 | `set_properties` cannot rotate a Part that lives in the world database | Stage 2 city | Open |
| G-24 | A road widened with `Lanes` and `Width` shows bare paint with no lane markings | Stage 2 city (road hierarchy) | Open (to verify) |
| G-25 | A part path Studio has held is never respawned, and deleting a sub-folder does not despawn its parts | Stage 2 city | Open |
| G-26 | A Driveway-class road builds a signed junction with crosswalks where it meets a street | Stage 2 city | Open |
| G-27 | `import_asset` places a mesh Part that ignores the GLB's own material | Stage 2 city | Open |
| G-28 | A water body layer did not fill a basin dug by a flatten pad | Stage 2 city (pools) | Open (to verify) |
| G-29 | A flatten pad lowers the ground the terrain query reports while the drawn ground stays level | Stage 2 city (pools) | Open |
| G-30 | After a full unload/load of the Space (2026-11-15 1:00 PM), file-backed parts that name a textured Space material (Brick, RoofSlate, RoofTerracotta, UnitWindowPair, Granite) draw plain white five minutes after the load; inspect_scene reports the right material name; the same materials drew after the 11:19 AM load; Output panel shows no warning. A part written after the load draws its texture, and any `set_properties` write on an affected part (same-value `CanTouch`/`Locked`) makes it draw: the material is loaded, the part resolved it too early at spawn and never re-resolved. Parts keep their colour tint, so only white-tinted parts look wrong | Stage 2 city (estate block) | Open; load-order race, workaround is a property write per part |

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
- **Found 2026-11-15:** one point can be moved. `set_properties` on
  `Workspace/Terrain/Layers/<road>/Point<n>` with `Position` re-bakes the road (verified on
  `CF_RS_Avenue_11` and `CF_RS_Avenue_12`, which now meet North Town's avenues in a four-way). The
  value is relative to the spline's own position, and the tool does not say so: a world position
  sent there put the point 2.6 km off the map until it was corrected. Adding or removing points is
  still not possible.
- **Also wanted (2026-11-15):** `Road:Audit` should list ends of different roads that land on one
  road between 12 m and 60 m apart. Two avenues 20 m apart on opposite sides of a collector were
  gathered into one junction with a dogleg.
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
- **Root cause in the Space (2026-11-15):** the first mountain's roads (`CF_Pass_Road_North`,
  `CF_Pass_Road_Valley`, `CF_Observatory_Road`) were thinned to one control point per hairpin apex and
  one point per 60 m or more, so the fitted curve leaves the routed line and the ground between two
  points climbs up to 26%. The Space fix is a re-bake from the saved route with the loops kept as
  entry, apex and exit points and heights taken from the route; it is not an engine change.
- **After the re-bake (2026-11-15):** with the curve on the routed line and fill held to 13 m, the engine
  still carries long stretches of the flank on piers (about 250 m below the hairpin at (1484, -1225))
  where the Space's profile shows 6 to 13 m of fill at the centre line. The cross slope there is about
  75%, so an embankment has no toe. It reads as an alpine half-viaduct with retaining walls. If a bench
  cut is wanted instead, the engine needs a rule for side slopes: shift the profile into cut when the
  downhill toe cannot land.
- **Needed:** when an Auto profile cannot hold the ground within the class's grade, the road says so
  (`Road:Audit` and a warning naming the stations) and offers the choices: more length, a steeper
  short grade through the hairpins, or a tunnel. A viaduct longer than a set length on a hillside (no
  water or valley under it) is reported, not built silently.
- **Closed when:** baking that alignment on a copy of TerrainBenchmark prints a warning with the
  stations and the grade it would need, and the audit lists the viaduct.

### G-18 A road draws no surface between a tunnel or deep cut and its end

- **Seen (2026-11-15):** `CF_North_Tunnel` (Arterial, Profile Explicit). West of the range the road has
  its surface, markings and an arched portal. East of the range the stretch from the portal to the road's
  end (75 m now, 145 m before the end was moved) has asphalt paint on the ground and no surface mesh, and
  the portal is a blank headwall with no opening. Switching the layer off and on does not bring it back.
  Capture: the Space's `Planning/network/tunnel_east_mouth_no_surface.png`.
- **Seen also:** `CF_Pass_Road_North` (Mountain, now Profile Explicit, cut and fill within 10.1 m by the
  Space's own profile check). At the hairpin near (1484, -1225) the uphill leg is a cut with paint and no
  surface and a headwall slab at its foot; the downhill leg is a bridge deck on a pier.
- **Update (2026-11-15, later):** the pass road was re-baked from the route with every hairpin as five
  points and explicit heights (`CF_Pass_North_1`, `_2`, `CF_Summit_Road_1`, `_2`, `CF_Pass_Valley_1`,
  `_2`; the three old roads are disabled). Its surface now draws along the cuts, and the slabs beside it
  are retaining walls, not portals. The missing surface on the mountain went with the old sparse points;
  the tunnel's east mouth is unchanged and is the case to reproduce.
- **Needed:** the surface mesh runs the whole road, tunnels and cuts included, and a portal is built at
  both mouths. `Road:Audit` lists each structure with its stations.
- **Closed when:** a capture from inside the east cut of `CF_North_Tunnel` in a copy of TerrainBenchmark
  shows lane markings up to an arched portal.

### G-19 Two splines joined in line meet with a kink

- **Seen (2026-11-15):** `CF_North_Trunk` ends and `CF_North_Tunnel` starts at (1100, -4200). The two
  control chords there share a heading within 0.1 degree. The built roads meet at about 7 degrees: each
  road's end tangent comes from its own curve fit, and the trunk is still turning from a 31 degree bend
  160 m back. South-west of the joint the trunk's north half shows bare paint with the lane dashes on it,
  the mesh covering only the south half. Capture: `Planning/network/seam_tunnel_mouth_before.png`.
- **Needed:** an end tangent the author can set, or `Road:Join` (G-05) that makes two splines one
  alignment. A road's mesh and its ground paint share one centre line.
- **Closed when:** the same joint in a copy of TerrainBenchmark shows one straight yellow line across
  it in a top-down capture.

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

## Stage 2: city

### G-20 `create_entity` stores a 0-255 colour unscaled

- **Seen (2026-11-15):** `create_entity` with `color: [255, 255, 255]` (the tool documents "0.0-1.0 floats or
  0-255 integers") on a Part routed to the world database. `get_properties` then reads `Color` as
  `255, 255, 255, 1` and the part renders as a glowing white block. `color: [1.0, 1.0, 1.0]` is stored
  and drawn correctly.
- **Closed when:** the same call stores `1, 1, 1, 1`.

### G-21 A material first read while its textures are being written stays blank

- **Seen (2026-11-15):** `MaterialService/FacadeCurtain.mat.toml` and its three PNGs were written, then the
  PNGs were rewritten a few seconds later while the engine was loading them. Every part using the material
  drew blank white from then on. Rewriting the `.mat.toml` did not reload it. The same definition saved
  under a new name (`FacadeGlass`) with the PNGs at rest drew correctly at once.
- **Needed:** a material whose map fails to decode retries when the file changes, and a changed PNG or
  `.mat.toml` reloads the material live. The Output panel names a map that failed.
- **Closed when:** overwriting a material's base colour PNG in a running Studio changes the parts that
  use it within a few seconds.

### G-22 `set_properties` refuses `Material`

- **Seen (2026-11-15):** `set_properties {Material: "FacadeCurtain"}` answers "Unknown property: Material"
  although `get_properties` lists `Material` as editable. `update_entity {material}` does set a
  Space-defined material (it writes the part's `material_name`), and `get_properties` still reports
  `Plastic` afterwards. `create_entity`'s `material` documents the built-in presets only, yet a
  Space-defined name works there.
- **Closed when:** `set_properties` sets a built-in or Space-defined material and `get_properties` reads
  the same name back.

### Stage 2 notes that are not gaps (2026-11-15)

- See-through glass works today: a material whose base colour PNG carries alpha, with
  `alpha_mode = "blend"` in its `.mat.toml`, draws the wall opaque and the panes transparent on a block
  Part. The pilot's `FacadeClear*` materials use it over a core block inside each building.
- To verify in Play: `ServerScriptService/CityLights` (Rune) sets each `*_Core` part to Neon at night by
  `set_string(part, "Material", "Neon")` and `set_color`. Not run yet; an MCP caller cannot start Play.

### G-23 `set_properties` cannot rotate a Part that lives in the world database

- **Seen (2026-11-15):** `set_properties {Rotation: [0, 180, 0]}` on a Part made by `create_entity` (routed
  "binary", path `Workspace/__bin_Part_<uuid>/_instance.toml`) fails with "schema heal ... The system cannot
  find the path specified". `Color` and `Reflectance` on the same Part succeed. `create_entity` takes no
  rotation, so a database Part cannot be turned over MCP at all.
- **Closed when:** the same call turns the Part, and `create_entity` accepts a rotation.

### G-24 A road widened with `Lanes` and `Width` shows bare paint (to verify)

- **Seen (2026-11-15):** `CF_RS_Street_08` (Collector, Edge Sidewalk) set to `Lanes: 2, Width: 14,
  Parking: true`. The bed, kerbs, sidewalks and the junction with the crossing avenue rebuilt at the new
  width. In two captures a minute apart the carriageway is smooth dark paint with only a short centre
  line by the junction: no lane lines, no parking stalls, no asphalt texture. Capture: the Space's
  `Planning/city/main_street_4_lanes.png`. Not yet checked after a Space reload.
- **Closed when:** the same road shows four lanes and striped stalls after the property change, without
  a reload.

### G-25 A part path Studio has held is never respawned

- **Seen (2026-11-15):** a block of 290 instance folders under `Workspace/SuburbNorthTown/Block_6_2` was
  loaded live, then its folder was deleted and written again with 793 parts. Deleting `Block_6_2` alone
  left every old part drawn; they went only when the parent `SuburbNorthTown` folder was deleted too.
  Of the 793 new records, 662 spawned. `inspect_scene` found none of the parts whose path matched a
  path of the first build (every `*_Roof*`, `Driveway`, `Pool_Water`, `*_Canopy`; 11 of 12 `Front_Walk`),
  and `get_properties` answers "no instance" for them although the files are on disk. One house's
  walls were missing as well, cause unknown. No line in the Output panel reports a skipped file. The
  same block written under new folder names loaded in full.
- **Also:** after that rebuild `inspect_scene` counts 1,211 Parts in the block's box where the disk holds
  761, so parts of the deleted build are still live.
- **Confirmed again (2026-11-15, later):** the block was rebuilt a third time under new lot folder names
  (`L01A_d` ...) after deleting the top-level `SuburbNorthTown` folder. `inspect_scene` then listed both
  builds live: every `Main_Roof_*` of the deleted `L..._c` lots with its old material beside the new
  `L..._d` ones at the same positions. So a top-level delete does not always despawn either. Editing a
  live part's record in place (12 `Pool_Water` records: material, colour, transparency) changed nothing
  on screen. The Space session cannot show a clean result until Studio reopens the Space.
- **Needed:** deleting a folder despawns everything under it; a file created at a path that was deleted
  spawns; a record the watcher skips is named in the Output panel.
- **Closed when:** delete and rewrite of one block folder in a running Studio leaves `inspect_scene`'s
  count equal to the files on disk.

### G-26 A Driveway-class road builds a signed junction where it meets a street

- **Seen (2026-11-15):** a two-point spline, `RoadClass: Driveway`, Width 6, from a garage to the centre
  line of a Local street. The drive itself graded well down to the road. The street got a full junction
  box: two crosswalks, stop bars and a stop sign.
- **Workaround in the Space:** driveways and walks are `Mode: Path` splines (BedMaterial Concrete), which
  lie on the ground, cut through the bank beside the road and build nothing on the street.
- **Needed:** a Driveway meeting a street builds a dropped kerb and an apron across the sidewalk, with
  no markings on the street.

### G-27 `import_asset` places a mesh Part that ignores the GLB's own material

- **Seen (2026-11-15):** `import_asset` with `place` wrote a Part with `material = "Plastic"`, colour
  `[163, 162, 165]` (0-255, see G-20) and no `respect_gltf_materials`. The quad drew as a flat grey
  rectangle. Adding the flag to the record afterwards changed nothing. The same GLB placed by an
  instance record that has `respect_gltf_materials = true` from the start drew its picture, alpha
  blended, correctly.
- **Needed:** a placed mesh draws the GLB's material by default, and editing the flag reloads the part.

### G-28 A water body layer did not fill a basin dug by a flatten pad (to verify)

- **Seen (2026-11-15):** a `TerrainFlattenPad` 1.5 m below the ground made a basin (terrain query confirms
  23.9 m against 25.4 m around). A `TerrainWaterBody` with Level 25.15 placed at its centre showed no
  water. Not investigated further; pools are built as lined tubs of parts with see-through water.
- **Note:** `terrain_carve` is refused to MCP callers by design, so a pool basin cannot be cut with
  vertical sides; the pad's sides slope at the 2.5 m cell size and a deck hides them.

### G-29 A flatten pad lowers the queried ground while the drawn ground stays level

- **Seen (2026-11-15, after the Space was reopened):** twelve `TerrainFlattenPad` layers dig pool basins in
  `Block_6_2`. For nine the drawn ground is dug. For three (`CF_NT_Pool_6_2_01`, `CF_NT_Pool_Lot_02_B`,
  `CF_NT_Pool_Lot_04_A`) `terrain_query` returns the pad's height (25.51 m at (-1029, -3157), under a pool
  floor at 25.70 m) while the capture shows Limestone ground filling the tub to its rim, about 1.6 m
  higher. Changing the pad's size, or switching it off and on, repaints the Limestone footprint but
  leaves the drawn height. Capture: the Space's `Planning/city/estate_v4_pool_ground_inside.png`.
- **Also:** `set_properties` on a pad's `Position` changed what `terrain_query` returned (24.9 m), and
  switching the layer off and on put it back to the height in the file (25.51 m): the move was not kept.
- **Closed when:** the drawn ground and `terrain_query` agree inside every pad after a reopen.
