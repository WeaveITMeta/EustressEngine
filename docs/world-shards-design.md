# World shards

Status: **design only.** This branch does not implement shards, a shard
router, boundary transfer, a second QUIC port, or the Paris playground. No
Worker deploy, and no port, DNS, or firewall change.

The first game this is for is **not** Box Head Zombie Defense Tycoon
(listing `07d084d8-9098-4192-bb48-84d2c428db36`). That listing is the
single-host staging test for a dedicated server on one machine. Shards are
a later branch.

## Hierarchy

McKale, 2026-10-20. France is a **universe**. Each city is a **space**.

Shards live inside spaces. The **space is the unit of distribution**, so
one host owns a whole city's simulation. A Forge job for Paris runs Paris.
A Forge job for Lyon runs Lyon. Physics only has to reconcile where one
space meets another.

Inside a city, cars and pedestrians never cross a shard line mid-road.
There is no seam down the middle of a street, a quay, or a square. Driving
from Paris to Lyon crosses a **space boundary**. That boundary is a defined
handoff point (the road that leaves one city space and enters the next),
not an arbitrary cut through the map.

The default is therefore one job per city space, not a grid of slices
inside the city. In-space subdivision is the fallback in
[When a city must be split](#when-a-city-must-be-split), and only when one
host cannot carry that city.

## Target use case

The first shard-capable game is a **full Paris, France playground**: a
distributed physics game with roads and cars, built with Terrain Benchmark.
France is the universe that playground sits in. Paris is one space in it.
Lyon, and any other city, is another space. The distributed part, in the
default, is the handoff between those spaces.

What this repository already says, and nothing past it:

- `docs/architecture/TERRAIN_OCEAN.md` describes Terrain Benchmark as
  **10 km by 10 km**. It uses that size to explain why the ocean is not one
  particle simulation: one particle per cubic metre of only the top metre of
  sea would be 100 million particles, and splash-quality spacing would be
  far beyond what the particle solver runs. That sentence is about the
  benchmark scene's water. It is not a measurement of Paris, and it is not
  a decision to cut a city into shards.
- A search of this tree did not find a Paris map, a road network, a car
  simulation, IGN BD TOPO, the Paris Plan de voirie, RGE ALTI, Lambert-93
  data, `frame_T.json`, or a `_civic/roads` pipeline. Those live on
  McKale's PC, described in the next section. This document does not invent
  their layout, vehicle model, or coordinates.

Box Head stays the single-host test. It is not a city space and it is not
split.

## Data on McKale's PC

Reported by McKale from Civic Twin. **Not in this repository.** Nothing
below was opened or checked from this tree. Paths are Windows paths on his
machine.

`Documents\Eustress\France` is the France universe. The space is
`France/Paris`. That folder holds:

- the IGN BD TOPO road graph, with lanes, one-way, and bridge/tunnel flags;
- the Paris Plan de voirie;
- 139k traffic-signal records;
- the RGE ALTI 1 m DEM and buildings;
- the Lambert-93 to engine transform in `frame_T.json`;
- Python road-mesh pipelines in `_civic\roads`.

Civic Twin can build a lane-level graph plus traffic demand. The
2×2 km Notre-Dame pilot export is on his PC, described under
[First shippable Paris milestone](#first-shippable-paris-milestone). It is
not in this repository, and this branch does not import it.

## What a shard is

Several hosts run **one shared universe**. Each host runs one or more
**spaces** (cities). Players and vehicles move between cities. The drive
is supposed to feel continuous: leaving Paris on a road arrives in the next
city's space without a loading break.

Each space a host simulates is **another Forge Orchestration job**. The
jobs model is already "one job, one game-server instance"
(`forge-orchestration` 0.6.0, used by `eustress/crates/forge`). Scaling
across cities means more jobs, not a second scheduler. Nomad, when it is
configured, is what spreads jobs across machines (`Forge::submit_job`
calls `NomadClient::submit_job` only if a Nomad client was set). The HTTP
control plane's `POST /api/v1/jobs` stores the job in the process; it does
not start a process and it does not place a city. A local process launcher,
if one exists for single-host jobs, is still one machine starting
processes. It is not a shard router, and this document does not add one.

Job identity stays the job id, not the machine and not the process. That
is the same hook as host migration (`docs/host-migration-design.md`): a
city's job can later move to another machine without renaming the city.
Host migration moves **one** job to a new machine. Shards are **many**
jobs at once, one per city space (or, only in the fallback, one per tile
inside a city that had to be split). They share the job-id idea and the
snapshot shape. They are not the same feature, and neither is built here.

## Scaling risk

Reported by McKale for Paris on his PC. **Not measured in this repository.**

Paris already uses about **15 GB RAM** and **9.7 of 11.3 GB GPU memory**.
Eustress has hung on large loads. One host owning a whole city therefore
needs **streaming and LOD**. Loading the city as one resident scene is how
that machine runs out of memory.

In-space subdivision stays an **optional fallback** for when one host
cannot carry a city even with streaming and LOD. It is not the layout of
France. The default remains one host, one city space.

## First shippable Paris milestone

McKale, 2026-10-20. **Not this branch.** No 2×2 km export, no car-mass
change, no suspension change, no chunk-crossing log.

The first thing that ships is one **2×2 km piece of Paris on a single
staging host**. It is not a shard split and not the whole city. France is
still the universe and Paris is still one space. This piece is the part of
that space one host can carry.

What it is made of:

- Civic Twin's road graph and 1 m elevation data, from McKale's PC (see
  [Data on McKale's PC](#data-on-mckales-pc) and the export below). Those
  files are not in this repository.
- Baked into Terrain Benchmark's **400 m chunks**. This tree's terrain
  config test describes a 10 km world as chunks of 400 m a side
  (`eustress/crates/common/src/terrain/toml_loader.rs`, the test
  `a_rendered_terrain_toml_reads_back_as_the_terrain_it_describes`,
  `chunk_size: 400.0`). The pilot export is five of those chunks on a side.
- About **20 main streets** and **one coupe**.

### Input data

Reported by McKale. **Not opened from this tree.** A search of the
repository did not find `terrainbench_pilot_notredame`, `r16_index.json`,
`dem_rgealti_1m.tif`, or `validation.json`.

Civic Twin's pilot export is on his PC at
`C:\Users\miksu\Documents\Eustress\_civic\export\terrainbench_pilot_notredame\`.
He reports **41 files, 32 MB**, including `README.md` and `build.py`.

- Area of interest, EPSG:2154 (Lambert-93): **651200, 6860800** to
  **653200, 6862800**. That is 2×2 km, or 5×5 chunks of 400 m.
- Roads, edges, nodes, signals, and lanes as GeoJSON, in both Lambert-93
  and WGS84.
- `dem_rgealti_1m.tif`.
- **25 r16 chunks**, each **161×161 at 2.5 m** with shared edges. The index
  is `r16_index.json`. Eustress Y = IGN69 − 34.85.
- `frame_T.json` and `validation.json`.
- Counts: **2066 road segments** and **1422 nodes**. The vehicle-only
  strongly connected component covers **1052 of 1255 nodes**.
- Caveat he states: the DEM is bare earth, so bridge and tunnel decks use
  the road segments' `z_ini` / `z_fin`, not the DEM height. The export has
  **32 bridges** and **68 tunnels**.

### First tasks

Not this branch.

(1) **Turn on road collision in the Paris Space, including bridge and
tunnel decks.** McKale reports that the export's roads have no collision.
The DEM is bare earth, so a bridge or a tunnel deck is not in the height
field. Each of those decks needs its own height from that road segment's
`z_ini` and `z_fin`. Without that, a car follows the bare-earth surface
and drives through the bridge or the tunnel instead of on the deck. He
reports **32 bridges** and **68 tunnels** in the pilot. Collision for
those decks is part of this task, not a later one. This branch does not
add the colliders and does not read `z_ini` / `z_fin`.

(2) **Filter the signal data to vehicle signals only**, excluding
pedestrian and repeater lights, using the `lib_ouvrag` field. Review which
junctions are signalized: the current flag is only a **15 m proximity
guess**. This branch does not filter the signals and does not replace
that guess.

Two car fixes come **before** the piece is playable. Both are McKale's
measurements of the car he is driving. A search of this tree did not find a
body that simulates at about 68 kg, and it did not find a suspension limited
to 1 cm of travel.

- The car's mass is corrected. He measures it simulating at about **68 kg**.
  The design mass is **1,515 kg**. In this tree the Chassis Playground coupe
  test asserts a welded group of **1,260 kg** on the spine
  (`eustress/crates/play-runtime/src/joints.rs`,
  `a_coupe_welded_through_its_seat_settles_stays_together_and_drives`:
  "the spine's body weighs the whole 1260 kg group"). The number 1,515
  appears there only as a scale on wheel torque
  (`5.0 * 1515.0 * 0.33 / 2.0`), not as the body's mass. Whether 68 kg is
  that coupe, another car, or a mass the solver dropped is **unverified**.
- The suspension gets real travel. He measures **1 cm** today. This tree's
  spring joint has a `free_length` and the coupe test hangs springs on the
  seat (`free_length: 0.6 + sag` in `spawn_welded_coupe`). Nothing in that
  test asserts a 1 cm travel limit. The 1 cm figure is **unverified** here.

The **400 m chunk lines are the future host-split lines.** They are not a
split in this milestone. The car stays on one host. The lines are the grid
a later fallback split would cut on, the same Lambert-93 tile idea as
[When a city must be split](#when-a-city-must-be-split), aligned to chunks
the terrain already uses. The Output panel logs each time the car crosses
one. Studio already has an Output panel (`OutputConsole` in
`eustress/crates/engine/src/ui/slint_ui.rs`, and `print` / `warn` are
documented as writing there). Nothing in this branch writes a chunk-cross
line to it.

It passes when all three are true:

- Two players see the car drive smoothly.
- The listing shows Live.
- Frame time and bandwidth are logged.

Box Head remains the single-host staging test for the dedicated server.
This milestone is the first Paris playtest, still one host, still staging.
It does not import Civic Twin, does not change the coupe, and does not
allocate a second host.

## When a city must be split

Only after streaming and LOD are not enough for that city. The split is
still inside one space. Cars and pedestrians still must not meet a seam in
the middle of a road. The rules below are the constraints on that fallback.
They are not implemented.

- Use a **tile-aligned Lambert-93 grid**, not arrondissements. Arrondissement
  borders follow administration, not the road graph, and they cut squares
  and river banks.
- Keep boundaries **off the Périphérique, the quais, and the big squares**.
  Those are where traffic is densest and a seam would be most visible.
- Use **100–200 m overlap buffers**, so a vehicle is already known to the
  next tile before it is the only owner.
- Hand off at **lane-graph edges**, not at an arbitrary world-space plane.
  The object that moves is a car on a lane. The edge is the place the graph
  already treats as a connection between two pieces of road.
- Run **one global signal clock**. A light on one tile and the next light
  on the neighbour must share the same time base. Two clocks drift and the
  handoff sees a green that the other tile still has red.
- Give **bridges and tunnels explicit ownership**. A bridge or tunnel is
  one structure even when the grid line crosses it. One tile owns it. The
  neighbour does not also simulate it.
- Use a **fixed timestep** and a **per-car seeded RNG**. Two tiles that
  both predict the same car across the overlap must get the same step and
  the same random draws, or the overlap buffer shows two different cars.

The 2×2 km Notre-Dame pilot is the first place those rules would be tried.
It is not a split of all of Paris, and this branch does not apply them.

## First design question: the handoff protocol

The open problem is the handoff, and the handoff that matters first is the
**space boundary** (Paris to Lyon), not a cut down a Paris street.

When a vehicle crosses from one city space into the next, the leaving host
and the receiving host have to transfer it cleanly:

- One owner at a time. The car is simulated on the source city until the
  destination city has accepted it, then only on the destination. Two hosts
  applying forces to the same car is a duplicate, not a handoff.
- The transfer carries the state the destination needs to continue the
  motion: pose, velocity, lane id, and whatever the car needs so it does
  not spawn at rest. The bytes are a delta against the last snapshot of
  that object, same idea as host migration (kilobytes, not the whole city).
- The source stops simulating it only after the destination has it. A crash
  between those two steps loses the changes since the last snapshot, which
  is the same limit as a host crash in `docs/host-migration-design.md`.
- Players riding in a car move with the car. Their session key stays. Their
  connect address changes to the destination city's host. They reconnect to
  that address with the same session key and certificate pin, without a
  round trip through the website. That reconnect path is specified for host
  migration and is not implemented.
- The road they are on is the handoff point. Both spaces already know that
  road as the edge where they meet. The car does not cross an invisible
  plane in the middle of a block.

The same protocol is what a fallback tile split would use, at a lane-graph
edge inside the overlap buffer. The default case does not need that split.

Latency between hosts has to stay low enough that the drive feels
continuous. A boundary that takes a second is a visible cut, which fails
this game even if the transfer is correct. No budget number is measured
here. What is already known from the host-migration notes:

- Under a second end to end wants the destination **already warm** (process
  up, city or overlap loaded, snapshot stream in progress). A cold start of
  the next city cannot hide inside a car crossing the boundary.
- The orchestration handoff itself is a network round trip: milliseconds,
  not microseconds.
- The physics step across the boundary has to be in that same budget. The
  protocol has to say who simulates the car during the round trip so it
  does not freeze and does not get simulated twice.

Inside one city, on one host, there is no shard handoff. Physics reconciles
at space boundaries only, unless that city was split under the fallback
rules above.

## QUIC between shards

Shard-to-shard traffic — city to city, or tile to tile in the fallback —
uses QUIC, which the engine already speaks for players.

What exists today (`eustress/crates/common/eustress-networking/src/native.rs`):

- One WebTransport listener per host, on UDP 7777 unless
  `EUSTRESS_HOST_PORT` says otherwise.
- The certificate is a self-signed ECDSA P-256 certificate, valid a minute
  under 14 days, pinned by SHA-256. Browsers join with
  `serverCertificateHashes`. Desktop Players use the same pin.
- The session is the player handshake (EUST), not a host-to-host channel.

Shard traffic should be QUIC, but it should **not** be a second player
joining on 7777. A player session and a shard peer are different: the peer
is another host of the same universe, it is allowed to receive server-side
state, and it must not be reachable as a gallery join. A later branch can
open a separate QUIC port for shard peers, with its own pin, and keep 7777
for players. That port is not allocated here, and this branch does not
change firewall or DNS guidance.

Why QUIC rather than a new transport: the engine already has a QUIC stack
(wtransport / quinn) in the Player path, including loss recovery and
streams. City-boundary deltas want a reliable stream for the car handoff
and an unreliable datagram for positions that the next tick replaces. Those
are the two QUIC modes the player path already separates. Reusing the stack
is the proposal. Reusing the player admission and the gallery link is not.

## What the orchestration layer would route

Not built. The later branch would:

- Register each city space as a job whose metadata names the universe
  (France), the space (Paris, Lyon, …), and the job id. A fallback tile
  would add the Lambert-93 tile id on that same job.
- Tell a joining player which city's job owns the place they enter, and
  hand them that host's join address.
- When a car crosses a space boundary, tell both jobs. The jobs move the
  car. The orchestrator does not simulate it.
- Drop a job that has stopped heartbeating, the same way a live host goes
  offline after 90 seconds. Cars that job owned need a rule (freeze on the
  last snapshot, or fall back to the neighbour city). That rule is not
  chosen here.

A browser of city hosts would be the same kind of row as a single-host
listing, one row per city job. Sorting by fullness ("busiest server that is
not full") does not place a player in the right city. Paris needs a
placement rule (which space owns this road) that fullness does not answer.
Fullness only picks among hosts of the same place that can still take a
person. Region, the host's round trip to the API, and a hardware class on
that row are not in this branch.

## Out of scope for this branch

- No shard job type, no city map, no space-boundary protocol, no Lambert-93
  tile split, no second QUIC port.
- No Paris content, no roads, no cars, no import of Civic Twin's files.
- No change to Box Head's single-host test.
- No deploy, and no production API.

A later revision of this document is where the Notre-Dame pilot's formats,
the size of a car's handoff delta, and a measured budget for a Paris–Lyon
crossing belong. Until those exist, the protocol stays an open question on
purpose. The hierarchy does not: France is the universe, each city is a
space, and one host owns that space unless the fallback split is required.
