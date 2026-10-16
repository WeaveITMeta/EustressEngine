# Road system

Roads that read as built infrastructure: an alignment designed to a road class's standard, a modelled
cross-section with cut and fill slopes, asphalt with lane markings, intersections, structures where the
ground cannot carry the road, and a road graph that traffic and navigation can use.

## Where roads stand today

A road is a `TerrainSpline` layer in Road mode. Its `RoadClass` property decides how it is built.

**Designed roads** (`RoadClass` set to a class):

- **Alignment:** `terrain::road_design` fits tangents, clothoid spirals and arcs through the control
  points to the class's standard. Each interior point is the middle of its curve.
  - A leg too short for the curves at its ends costs their spirals first; a radius goes under the class's
    minimum only once its spirals are gone.
  - Turns of more than 75 degrees on a road with a hairpin radius are switchback turns and take that
    radius as their minimum.
  - However tight a curve is, the inside of the road stops short of its centre, so no edge folds back.
- **Profile:** the `Profile` property picks where the elevation comes from.
  - `Auto` grades the road to the ground within the class's grade, eased by vertical curves.
  - `Explicit` passes through each control point's height.
  - `Ground` follows the ground smoothed over about 10 m.
- **Lane layout:** four properties change the class's layout, each with the class's own as its default:
  - `Lanes`: lanes in each direction, 1 to 4 (every lane, on a one-way road). `Width` is shared by every
    lane.
  - `OneWay`: every lane runs toward the road's last point. The centre line goes; a yellow line runs
    along the left of travel and white dashes between the lanes.
  - `Parking`: a 2.4 m parking lane along each kerb, striped into 6.1 m stalls.
  - `Edge`: `Auto` (the class's), `Sidewalk` (a gutter, kerb and a 3 m sidewalk, 2 m on a Local street),
    `Ditch` or `None`.
  - A kerbed two-way street keeps its centre line solid throughout.
- **Sides:** each station's sides are chosen from the ground across them, out to `ShoulderWidth`:
  - In cut: the class's soil slope, then its rock slope, whichever meets the ground first.
  - In fill: the class's fill slope, then 1.5:1, then a 1:1 reinforced slope.
  - Past those, a retaining wall: at the formation's edge in fill (with a parapet), a metre out in cut.
    The ground beyond a wall stays as it is.
- **Structures:** a structure carries the lanes and shoulders (and a kerbed road's sidewalks), never a
  ditch: its parapets and walls stand at the shoulder's edge.
  - **Bridges:** where the fill would pass the class's `max_fill`, a bridge carries the road: a solid deck
    out to its parapets, piers every 30 m and an abutment at each end. Each parapet is a solid concrete
    wall 0.45 m thick and 1.1 m high: its inner face stands at the shoulder's edge, its outer face is
    flush with the deck's fascia, and its ends close over the abutments.
  - **Tunnels:** where the cut would pass `max_cut`, a tunnel does: an arched lining that rises from the
    shoulder's edge, and a portal at each mouth.
    - **Portal:** over the last 30 m out from each portal the cut steepens from the class's slope to a
      rock face (0.4 horizontal to 1 vertical), so the cutting at the portal is no wider than the portal
      needs. A headwall closes it 2.8 m out from the portal, past the raster cell the ground ramps across
      there. It is 1 m thick and runs from a footing 1 m under the road up each rock face, 1.5 m into it,
      to 0.8 m above where it meets the ground, so the hill stands behind it and no terrain edge shows
      round the mouth. Across the opening it runs on under the road as a sill. An arched collar
      4 m wide stands 0.4 m proud of it, the lining runs out to the collar's face, and a slab buried at
      the carve's top closes the ground over the bore for 3 m behind the wall.
    - **Bore:** the terrain's volume is carved end to end, from one headwall's face to the other's,
      2.5 m clear of the lining (one lattice cell), so no ground stands inside it. The end boxes stand
      plumb, so no carve reaches in front of a wall at the road or above it. A chunk the bore passes
      through hangs no skirt across it, drawn marched or as a heightfield at a coarser level of detail.
  - Either structure's ends sit back where the cut or fill falls to 60% of the limit, and a tunnel's
    portals sit back far enough that at least 3.5 m of ground closes over the crown. Under a structure the
    ground stays as it is.
  - An `Auto` road is graded straight through its tunnels instead of over the mountain.
- **Bake:** the layer bake carves the class's cross-section `ROAD_BED_SINK` under the road's surface:
  crown or superelevation, shoulders, then kerbs and sidewalks or ditches. Beyond, each side's slope or
  wall. Anything that has not met the ground by the outer part of the band fades into it there.
  - The terrain renders bilinear between its raster vertices, so every vertex within one raster step's
    diagonal of the road's drawn surface (lanes, shoulders, kerbs and sidewalks) stands below the lowest
    of that surface within that reach, less the grade over it: `ROAD_BED_SINK` deeper under paving, flush
    beside a gravel shoulder. No gutter, kerb foot, crown edge, or toe of a cut slope or wall shows ground
    through the road.
  - `Width` is the lanes' total width.
  - `ShoulderWidth` is how far the slopes may reach beyond the formation.
  - The bed material paints the paved formation and the shoulder material paints the ditches and slopes.
    An unpaved shoulder is gravel on the ground: the ground stays flush under it and keeps its own
    material.
- **Junctions:** designed roads meet where:
  - their ends gather within 12 m of each other;
  - an end comes within 12 m of another road's side (a tee);
  - two roads cross along their lengths (a crossroads).

  Roads more than 3 m apart in height cross at different levels instead and meet nowhere. Roads within
  15 degrees of parallel where they would meet run side by side or merge and meet nowhere either, and
  when two such roads both reach one junction the higher class keeps its leg (then the road listed first)
  while the other runs on over it.
  - A junction centres where its roads' centre lines meet (the point nearest all of them), not where a
    road's end happens to stop, and each leg is measured from that centre along its road, past an end
    that stops short of it, so every road's surface begins exactly at its mouth.
  - Each junction is designed by `road_junction`: kerb returns, setbacks, control, crosswalks and stop bars.
    Neighbouring legs less than 30 degrees apart meet with a straight corner rather than a kerb return,
    and no setback reaches past 80 m, so no set of roads gives an unbounded box.
  - Its surfaces are draped over the roads that meet it: the box takes each road's own height and crown
    at its mouth; crosswalks (kerb to kerb) and stop bars lie on each road's surface; each corner
    sidewalk is a solid (kerb face, top, and a back that reaches below the ground) whose ends match the
    roads' kerbs and sidewalks.
  - A one-way road's leg lets traffic only in or only out: no stop bar and no lane path where traffic
    only leaves, a stop bar across every lane where it only comes in.
  - The bake shapes the ground to the junction after every layer: its draped surface under the box and
    corners, then flush with the paving's edge, fading back to the ground. Past each mouth the road's own
    bake rules.
    - Under the paving each vertex stands below the lowest paving within one raster step around it, as
      beside a road.
    - Inside the box a road paints its bed only under the box's paving, so no bed paint lies in the
      corners past the kerb returns.
  - Each road stops at the box, leaves its crosswalk and stop bar unpainted, and holds its centre line
    solid for the last 30 m before it.
- **Furniture:** `terrain::road_furniture` generates what a road's design and its junctions' control call
  for, merged into one mesh per finish:
  - **Signals:** at a signalled junction, a mast-arm pole on the far-right corner of every approach
    traffic comes in by, a signal head over each lane it drives into, 5 m clear of the road.
  - **Stop signs:** at every other approach that stops, on the incoming traffic's right, level with its
    stop bar.
  - **Streetlights:** along every kerbed road, staggered every 35 m on each side, an arm over the road
    and a spot light shining down (no shadows), clear of junction boxes.
  - **Street trees:** on sidewalks 3 m wide or more, every 12 m on each side, clear of the streetlights
    and junctions.
  - Poles and posts collide.
- **Surface:** `terrain::road_surface` draws every surface of the section and its markings
  (`terrain::road_mesh`) on that formation, each line and every dash cut exactly to its range.
  - The carriageway wears the asphalt textures, paved shoulders wear worn asphalt, and unpaved shoulders
    wear gravel (the shoulder material's colour when one is set).
  - Kerbs, sidewalks and paint have their own colours.
  - A passing zone shorter than 120 m to 240 m (with the design speed) stays no-passing, and so is every
    tunnel.
  - Beyond the formation it draws earth wedges down the first metres of each slope, which close the
    step the terrain raster leaves at the edge. Their reach is averaged along the road and tapers to
    nothing at each run's ends, so their outer edge is a smooth line. They wear the terrain's own surface
    material (its material map and textures, shaded as the heightfield is), so they read as the ground
    around them. It also draws the walls, bridges
    and tunnels in concrete.
  - One triangle-mesh collider covers the paved surfaces, crown, banking and kerbs included; the walls and
    structures collide as another.
- **Scatter:** scatter that avoids roads keeps off the formation and junction boxes; the slopes carry it.

**Plain roads** (`RoadClass` None):

- `terrain::road` builds its path: a centripetal Catmull-Rom curve through the control points' XZ, with
  stations every 1 to 2 m.
- Its elevation profile is a natural cubic spline fitted to the ground sampled every 10 to 20 m along that
  path. The control points' Y is not used, so the road follows the ground it crosses.
- The layer bake (`terrain::layers`) carves a flat bed to that profile with a smoothstep shoulder back to
  the ground, and paints the bed and shoulder materials.
- `terrain::road_surface` lays one flat ribbon mesh and a chain of box colliders along the stations, in
  the bed material's swatch colour.

Either way, `terrain.layer_create` takes at most 64 points per spline, because each point reads its
spline's folder back to number itself. `RoadClass`, `Profile`, `Lanes`, `OneWay`, `Parking` and `Edge`
are ordinary properties, set in the Properties panel or over MCP like any other.

What plain roads give in practice (the Coastal Frontier rebuild, 2026-10-01):

- A profile that follows the ground cuts and fills wherever the ground bends, so a switchback becomes a
  slab of asphalt with knife-edge cut walls. A road that was graded beforehand loses its grading.
- The shoulder is painted terrain, so the edge of every road is a wide blotch of shoulder material, not a
  shoulder.
- There are no lanes, markings, kerbs or sidewalks, and two roads that meet simply overlap.
- One road of more than about a kilometre is several splines.

## The model

### Road and its parts

A road is a `Road` instance in `Workspace/Terrain/Layers`, ordered with the other terrain layers. Its
children are `RoadPoint` instances (the points of intersection of its horizontal alignment, in order) and,
optionally, `RoadProfilePoint` instances (vertical points of intersection: station and elevation). A
`RoadJunction` instance joins road ends. A `TerrainSpline` in Road mode keeps working; a converter turns
one into a `Road` of class Local.

| Property | Meaning |
|---|---|
| `Class` | Motorway, Arterial, Collector, Local, Mountain, Rural, Unpaved, Driveway |
| `Lanes` | Lanes per direction; the class sets the default |
| `DesignSpeed` | km/h; the class sets the default and the limits follow from it |
| `Profile` | `Auto` (graded by the solver), `Explicit` (profile points), `Ground` (follows the ground, for tracks) |
| `Surface` | Asphalt, Concrete, Cobble, Gravel, Dirt |
| `Markings` | Class default, None, or an override per marking kind |
| `StructureMode` | Auto (bridges and tunnels proposed where the ground fails), Never |

### Class standards

`RoadStandard`, one per class, holds the numbers every other part reads. The values below are the
defaults; a Space can override them.

| Class | Lanes/dir | Lane m | Shoulder m | Speed km/h | Max grade | Min radius m | Crown | Max superelevation |
|---|---|---|---|---|---|---|---|---|
| Motorway | 2-4 | 3.65 | 3.0 paved | 110 | 4% | 600 | 2% | 8% |
| Arterial | 2 | 3.5 | 2.0 paved | 70 | 6% | 200 | 2% | 6% |
| Collector | 1 | 3.5 | 1.5 paved | 50 | 8% | 90 | 2% | 6% |
| Local | 1 | 3.0 | kerb and sidewalk | 40 | 10% | 50 | 2% | 4% |
| Mountain | 1 | 3.25 | 0.75 gravel | 40 | 10% (12% short) | 30 at hairpins | 2.5% | 8% |
| Rural | 1 | 3.25 | 1.0 gravel | 70 | 8% | 150 | 2% | 6% |
| Unpaved | 1 | 3.0 | none | 30 | 12% | 25 | 4% | 0% |

Each standard also carries the vertical-curve K values (crest and sag) for its speed, the cut slope (1:1
in rock, 1.5:1 in soil) and fill slope (2:1), the depth past which a fill becomes a bridge and a cut a
tunnel, and its marking scheme.

### Horizontal alignment

Between consecutive `RoadPoint`s the alignment is a tangent, and at each interior point a circular arc
with a clothoid (Euler spiral) in and out of it, the arc radius at least the standard's minimum. Where two
points are too close for the radius, the alignment uses the largest radius that fits and reports the point
as a violation. A hairpin is an arc of 150 to 180 degrees at the Mountain class's hairpin radius, with the
grade eased through it.

Stations are placed every metre along the exact alignment; nothing downstream re-fits a curve.

### Vertical alignment

- **Auto:** the solver fits a profile to the ground under the alignment. It respects the standard's
  maximum grade and joins every grade change with a parabolic vertical curve of the standard's K, and it
  minimises the balance of cut and fill, weighted toward cut in rock. Profile points it chose are written
  back as `RoadProfilePoint`s, so the user can pin and edit them.
- **Explicit:** the profile points are the profile.
- **Ground:** today's behaviour, for tracks and paths.

### Cross-section

The class gives a template, evaluated at every station: lanes with crown, or with superelevation through
curves and the transitions into it, then shoulders, then kerb and sidewalk (Local) or a ditch (Rural and
Mountain), then a daylight slope from the edge of the formation to the ground, at the cut or fill slope.

The template drives three outputs from one evaluation, so they cannot disagree:

1. **The terrain bake:** the corridor is shaped to the formation and its daylight slopes, replacing the
   smoothstep shoulder. The shoulder and slope materials are painted from the template: gravel shoulder,
   rock cut, grassed fill.
2. **The road surface meshes:** one mesh per material zone (carriageway, shoulder, kerb, sidewalk), with
   UVs in metres: u across, v along the road. Meshes are cut into sections of about 200 m so they cull and
   stream with the terrain.
3. **The collider:** a trimesh of the carriageway and shoulders per section, replacing the box chain.

### Surface and markings

- **Asphalt** is a PBR material set (base colour, normal, roughness), seamless at about 4 m, with two wear
  variants. A vertex channel carries wheel-path wear and patching, so a lane darkens in its wheel paths.
- **Markings** are generated from the class's marking scheme, as thin mesh strips just above the surface
  that share the road's stations. The kinds are:
  - **Lines:** centre lines (dashed, solid, or double yellow where the sight distance falls short), lane
    lines and edge lines.
  - **At junctions:** stop and give-way bars, crosswalks, turn arrows.
  - **Signs painted on the road:** speed numbers and "SLOW" on mountain approaches.
- Dashes follow arc length, so they keep their rhythm through curves.
- A no-passing zone is computed from the profile's and the alignment's sight distance at the design speed,
  so the line switches between dashed and double without anyone placing it.

### Junctions

A `RoadJunction` names the road ends it joins and its kind: T, crossroads, roundabout, or merge (on and
off ramps).

- **Trimming:** each road is trimmed at a setback.
- **Surface:** the junction builds one surface polygon with kerb returns at the class's corner radius,
  graded to a plane through the incoming profiles.
- **Markings and lanes:** it adds the stop lines, crosswalks and turn lanes its kind calls for.
- **Placement:** the road tool places junctions where a road end is dropped on another road.

### Lanes, junctions and interchanges

Freeform splines cannot carry a city. A downtown crossroads with turn pockets, a motorway exit with its
deceleration lane, or a cloverleaf is a set of roads whose lanes appear, taper, split and join, and every
one of those changes follows a rule. The road network therefore has four layers, the same structure as
ASAM OpenDRIVE (the format driving simulators, autonomous-vehicle tools and RoadRunner use), so Eustress
can import and export `.xodr` and bring real networks in:

1. **Reference line:** the alignment above (tangents, clothoid spirals, arcs: OpenDRIVE's own three
   primitives) with its vertical profile. It is the only curve anyone edits.
2. **Lane sections:** stretches along the reference line with a constant lane layout. Each lane has:
   - **Width:** a cubic in station, so a taper, a turn pocket, a merge or a lane drop is a width
     transition, never new geometry.
   - **Properties:** a type (driving, turn, parking, bike, bus, shoulder, sidewalk, median), a direction,
     a speed, a surface, and a marking on each side chosen by the rules below.
3. **Junctions:** the box where roads meet, its kerb returns and its control. Its connecting roads are
   short roads with their own reference lines, one per legal movement from an incoming lane to an
   outgoing lane. They carry the lane graph, so traffic, navigation and the AI drive the same paths the
   markings describe.
4. **Interchanges:** grade-separated junctions built from the same pieces:
   - **Ramps** are roads.
   - **Speed-change lanes** are lane sections on the mainline that hand over to a ramp.
   - **Structures** stand where a ramp crosses another road.

Everything else is derived. A person edits the reference lines' points, the lane counts and the junction
kinds; the network rebuilds the lane sections, junction boxes, markings, meshes and the terrain corridor.
When a road moves, the junctions at its ends re-solve, and the roads that meet there follow.

#### At-grade junction rules

| Rule | Default |
|---|---|
| Kerb return radius, by the design vehicle of the lesser road | car 6 m (urban local) to 7.5 m; single-unit truck 9 m (collector); semi-trailer 12 m (arterial); 15 m on truck routes |
| Setback | each road stops past its corners and clear of every kerb it crosses |
| Control | signals where two through roads (collector or larger) cross; stop signs on the lesser road where a local meets a larger one; all-way stop where local streets cross; the stem stops at a T of equals |
| Crosswalk | 3 m, continental bars 0.6 m on 1.2 m centres, on every urban approach and every signalised one |
| Stop bar | 0.45 m, 1.2 m behind the crosswalk, across the incoming lanes |
| Centre line on the approach | solid double yellow for 30 m (no passing near an intersection) |
| Left-turn pocket | arterials and signalised collectors: storage from the traffic model (30 to 60 m by default) plus a taper of L = W V² / 155 m up to 70 km/h, L = W V / 1.6 above (W the lane width in m, V in km/h) |
| Signal phasing | a protected left phase where the opposing through traffic has two or more lanes |
| Signal spacing | at least 300 m along an arterial; the generator merges or drops signals closer than that |
| Sight triangles | at every stop-controlled corner, a triangle kept clear of buildings, walls and tall scatter |
| Vertical clearance | 5.1 m over a road, 7.0 m over rail: a constraint on the vertical profile, not a check afterwards |

#### Interchanges

The kind follows from the roads that cross and the room there is:

| Crossing | Kinds offered |
|---|---|
| Arterial over or under a motorway | diamond; single-point urban (SPUI) where the corners are built up; partial cloverleaf where one quadrant must stay clear |
| Motorway with motorway | cloverleaf (light traffic), partial directional, stack (heavy traffic) |
| Three legs onto a motorway | trumpet, directional T |

- **Ramps:** design speed 40 to 70 km/h, loop radius at least 50 m, grade at most 6% up and 8% down,
  superelevation to 8%, spirals at both ends.
- **Speed-change lanes:** deceleration lane plus taper by mainline speed: 80 km/h 110 + 60 m, 100 km/h
  145 + 70 m, 110 km/h 160 + 75 m. Acceleration lanes are sized by the ramp's speed at its merge.
- **Gores:** painted chevrons with the exit sign and its number.

#### Downtown generation

The street graph of a plan becomes roads and junctions in one pass:

1. **Classify:** each edge gets its class from the plan (traced widths, connectivity, the zone it runs
   through).
2. **Lay roads:** each edge becomes a reference line through its points.
3. **Design junctions:** each node becomes a junction designed by the rules above.
4. **Add lane sections:** turn pockets go in before signals, parking lanes along local streets downtown,
   bus bays where a route stops.
5. **Audit:** the network audit reports anything the rules could not satisfy: signals too close, a return
   that will not fit between two buildings, a ramp over grade.

### Structures

Where the auto profile needs a fill deeper than the standard's limit, or the alignment crosses water,
the span becomes a bridge. Where it needs a cut deeper than the limit, it becomes a tunnel. Each is a
`RoadStructure` child of the road with its own kind:

- **Kinds:** girder, arch, cable-stayed or suspension bridge; tunnel; retaining wall.
- **Geometry:** built from a small parametric kit (deck, piers at a span the kind allows, abutments,
  portals).
- **Terrain:** the terrain bake leaves the ground alone under a bridge and over a tunnel.

### Furniture

Furniture is a `RoadFurniture` child of a road or junction, generated the way `TerrainScatter` places
scatter: from rules, the road's design and a seed, deterministically.

- **What it places:**
  - **Signs:** regulatory, warning and guide signs.
  - **Signals and lighting:** traffic signals on mast arms, street lights.
  - **Edge protection:** guardrails and delineator posts.
  - **Temporary work:** cones, barrels and work-zone layouts.
- **Rules:** they come from the design and the sign code (MUTCD-style placement by default):
  - **Control:** a stop sign at every stop bar; signal heads and mast arms from the junction's phasing.
  - **Speed:** speed-limit signs after junctions and class changes; a curve warning with an advisory
    speed ahead of any curve tighter than the design speed allows; chevrons on curves under the
    hairpin radius.
  - **Guide signs:** exit signs at 2 km, 1 km and the gore before a ramp.
  - **Edge protection:** guardrails on fills over 3 m and on the outside of tight curves, delineators
    every 50 m on rural roads.
  - **Lighting:** street lights staggered every 30 to 40 m on arterials.
  - **Work zones:** a `WorkZone` child lays its taper, buffer and lane closure by the standard's taper
    lengths.
- **Items are real instances,** so they can be selected, moved, edited and deleted. The furniture child
  keeps every edit as an override (moved, restyled, removed) keyed by the item's rule and station, so
  regenerating after the road changes keeps what a person changed.
- **Destructible:**
  - **Breakaway supports:** sign posts have slip bases and light poles have breakaway bases, built as
    jointed physics bodies that break above an impulse.
  - **Loose items:** cones and barrels are light dynamic bodies.
  - **Reset:** damage is Play state; Stop restores the generated layout.

### The graph

A `RoadNetwork` resource holds the graph: junctions as nodes, roads as edges, lanes with their
directions, widths and connections, and each junction's control state. Traffic, navigation, sensors and
the AI read it, and it is the HD map OpenDRIVE export writes.

### Properties, methods and MCP

Roads are classes like any other, so nothing about them needs a road-specific API:

- **Properties:** every property of `Road`, `RoadPoint`, lane sections, `RoadJunction` and `RoadFurniture`
  is a class property. The Properties panel shows it, scripts read and write it, and the generic MCP
  property tools (`get_properties`, `set_properties` on any instance) change it. One edit is one undo step
  and triggers the same rebuild a Properties edit does.
- **Methods:** a few operations change several instances at once and are not property edits:
  - `Road:Split(station)` and `Road:Join(other)`.
  - `Road:ConnectAt(other, station)`, which makes a junction.
  - `Road:Audit()`, which returns the violations, earthwork and structures.
  - `RoadNetwork:Route(from, to)` and `RoadFurniture:Regenerate()`.

  They are class methods, callable from Luau and Rune and from the generic MCP method tool
  (`call_method` on any instance), the same tool every class uses.
- **Road tool:** picks a class and draws `RoadPoint`s. It shows the alignment's violations as it goes
  (radius, grade, structure needed) and offers the fix.
- **No point cap:** points are numbered once when the road is written, not by reading the folder back per
  point.

The generic property and method tools are an engine prerequisite, not part of the road system: today the
bridge's `entity.update` sets only Part fields (position, size, colour, material, transparency, anchored,
can-collide), so no other class's properties are reachable over MCP.

## Phases

1. **Standards and geometry (common, pure):** `RoadStandard` per class, horizontal alignment (arcs and
   clothoids from PI points), the auto profile solver with vertical curves, and cross-section template
   evaluation. Unit tests against hand-worked alignments.
2. **Bake, surface and look:** the terrain bake follows the template and the design profile. The road
   surface becomes per-zone meshes with metre UVs and a trimesh collider. Add the asphalt material set and
   generated markings.
3. **Instances and tools:** `Road`, `RoadPoint`, `RoadProfilePoint`, the loader, Properties, the road tool,
   the converter from Road-mode splines, and the bridge and MCP methods.
4. **Junctions, lanes and structures:**
   - **At-grade junctions:** returns, setbacks, control, crosswalks, stop bars, the lane graph
     (`road_junction`).
   - **Lane sections:** turn pockets, tapers, parking lanes.
   - **Interchanges:** the templates, ramps and speed-change lanes.
   - **Structures:** the bridge, tunnel and retaining-wall kit, and guardrails.
5. **Network:** the road graph, signal phasing, signs, street lights, the downtown generator, and
   OpenDRIVE (`.xodr`) import and export.

## Acceptance

- **Switchbacks:** a Mountain road up a 30 degree slope builds as switchbacks with hairpins at 30 m
  radius. No grade is over 10% outside the hairpins or over 12% within them, and no cut exceeds the
  standard's limit without becoming a structure.
- **Profile:** the profile follows the solver's, within 5 cm, wherever the ground bends under it.
- **Markings:** they hold their dash rhythm through curves, and the centre line is double wherever the
  passing sight distance is short.
- **Junctions:** a crossroads of two Arterials builds kerb returns, stop bars and crosswalks with no
  overlapping surfaces.
- **Collision:** a vehicle driving the carriageway meets the collider within 2 cm of the drawn surface.
