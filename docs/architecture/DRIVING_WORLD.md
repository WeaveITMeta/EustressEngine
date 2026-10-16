# Driving world

What Eustress needs for living roads (traffic, racers, police, pedestrians), for vehicles that drive
like real ones, and for the Voltec vehicle's digital twin: a city whose roads, signals and traffic run in
real time, on a desktop or on a screen inside the car, from the same Space. The road network itself is in
`docs/design/ROAD_SYSTEM.md`.

## Engine or Space script

The split decides what runs fast, what stays correct across every Space, and what a creator can change
without a build.

**Engine (Rust):** anything every Space with roads needs, anything that runs per agent per frame, and
anything whose correctness other systems depend on.

- Road geometry, lane sections, junctions, furniture generation and the lane graph.
- Vehicle dynamics: tires, suspension, drivetrain, brakes, steering, aerodynamics, inside Avian.
- The traffic kernel: car following, lane changes, right of way, signal controllers, routing on the lane
  graph, spawning and level of detail.
- Pedestrian movement on sidewalks and crosswalks.
- Sensors (camera, LiDAR, radar, GPS, IMU) and their ground-truth labels.
- Rendering, physics, destruction, audio synthesis, replication, recording and replay.

**Space scripts (Luau and Rune):** what a particular world is and how it behaves. That covers:

- **Traffic make-up:** how much traffic there is, what kinds, and at what hours.
- **Driver behaviour:** personalities as parameter sets.
- **Rules of this world:** police and wanted rules, race rules and checkpoints, missions and events.
- **Configuration:** signal timing plans (as data), and which roads are closed or under works.
- **Experience:** the HUD and in-car UI, and the scenario a test runs.

The rule: if two different Spaces would both want it, it is engine; if it is the story or the rules of
one world, it is script. Scripts never loop over agents every frame. The engine exposes parameters,
events (`Vehicle.Collided`, `Junction.SignalChanged`, `Agent.Arrived`) and class methods, and a script
steers thousands of agents by setting their parameters and goals. A police pursuit is a script that
chooses a target and a tactic; pursuing, boxing in and the PIT manoeuvre are engine behaviours it calls.

## What exists today

- **Roads:** a Road-mode terrain spline with a `RoadClass` is designed to that class: its alignment,
  graded profile, cross-section with cut and fill slopes, asphalt and markings, and a collider on the drawn
  surface. Without a class it is a ribbon over a carved bed. At-grade junction geometry exists as a module
  (`road_junction`) that no instance uses yet.
- **Vehicles:** `VehicleSeat` and physics vehicles built in Space scripts on Avian (the Vehicle Simulator
  and Chassis Playground Spaces). There is no engine tire model, drivetrain or suspension component.
- **Destruction:** deformation and fracture are live for parts.
- **Data and streaming:** Eustress Streams and the data platform ingest and replay time series.
  Recording and replay exist for the timeline.
- **Networking:** server authority, protocol v3.
- **Point clouds:** imported, not simulated (no sensor simulation).
- **Not present:** traffic simulation, pedestrians, a behaviour runtime, scenario files, and generic
  property and method access over MCP for classes other than parts.

## What is missing

Ordered by what unblocks the most.

### 1. Foundations

1. **Generic class access.** `get_properties`, `set_properties` and `call_method` for every class over the
   bridge, MCP and scripts, through the class schema the Properties panel already uses, one undo step
   per edit. Every later system (roads, furniture, traffic settings, vehicles) is then reachable by AI
   and scripts without a tool of its own.
2. **Class methods.** A registry so a class declares methods (`Road:Split`, `Vehicle:SetGoal`) once and
   they appear in Luau, Rune, MCP and the docs alike.
3. **The road system's phases 2 to 5.** Bake and surface from the design, `Road`, `RoadPoint`,
   `RoadJunction` and `RoadFurniture` instances, lane sections, interchanges, structures, the network
   graph, OpenDRIVE import and export.

### 2. Vehicles

4. **Vehicle dynamics component.**
   - **Wheels and suspension:** wheels on suspension (spring, damper, anti-roll) with a brush or Pacejka
     tire model (slip ratio and angle, combined slip, load sensitivity, rolling resistance).
   - **Drivetrain and brakes:** engine torque curve, gearbox, differentials (open, limited-slip,
     locked), brakes with ABS.
   - **Driver aids, steering and aero:** traction control and stability control, steering with
     Ackermann geometry, downforce and drag.

   It is one data-driven class so a car, a truck, a bus, a motorcycle and the Voltec vehicle are presets,
   not code. It drives physical bodies, never raycast cars.
5. **Electric powertrain model** for Voltec: motor maps, inverter and battery (cell model with state of
   charge, temperature, internal resistance; the V-Cell data where it applies), regenerative braking, and
   thermal limits. Energy and range in the simulation then match the real pack.
6. **Vehicle damage.** Deformation and fracture extended to vehicle bodies, detached panels and glass,
   and damage that changes handling (a bent wheel pulls).

### 3. Traffic and AI

7. **Traffic kernel.**
   - **Driving model:** the intelligent driver model for car following, MOBIL for lane changes, and gap
     acceptance and right of way at junctions.
   - **Signals:** controllers running phase plans, actuated by detectors.
   - **Movement:** routing on the lane graph, parking manoeuvres, and emergency vehicles that others
     yield to.
   - **Scale:** tens of thousands of agents with level of detail: full physics near the player, a
     kinematic model on the lane graph at middle range, and aggregate flow far away. Agents switch tier
     without a visible pop.
8. **Pedestrians.** Walking on sidewalks, crossing at crosswalks with the signals, jaywalking as a
   parameter, crowds at plazas, and reactions to vehicles.
9. **Behaviour runtime.** A behaviour tree and utility system in the engine whose trees are data in the
   Space, so racers (racing lines, overtakes, defending), police (pursuit, roadblocks, spike strips,
   boxing in) and civilians (panic, yielding, road rage) are authored, not compiled.
10. **Racing line and track tools.** Optimal racing lines computed from the road geometry, and track
    limits, timing lines and pit lanes as road properties.

### 4. Autonomy simulation and sensors

The bar for "competes with Tesla" is two things, and both are reachable:

- **In-car visualisation better than Tesla's.** Tesla's screen draws a stylised world from its
  perception output. A Eustress digital twin can draw the actual city, at quality, with every agent in
  it.
- **A simulator that autonomy can be trained and validated in,** in the class of CARLA and NVIDIA DRIVE
  Sim.

Autopilot's driving itself comes from perception and planning trained on fleet data. Eustress makes
building and testing that kind of stack possible; it is not that stack.

11. **Sensor simulation.**
    - **Cameras:** with lens distortion, exposure and noise.
    - **LiDAR and radar:** LiDAR with beam patterns, dropouts and intensity, and radar with returns,
      Doppler and clutter.
    - **Ultrasonic, GPS and IMU:** with bias and drift.
    - **Timing:** every sensor time-stamped on one clock.
12. **Ground truth.** Depth, semantic and instance segmentation, 2D and 3D boxes, lane and drivable-area
    labels, written alongside every frame for training.
13. **Scenarios.** OpenSCENARIO import and export, deterministic replay, and parameter sweeps (thousands
    of runs of one scenario with varied weather, traffic and timing) on the headless runtime.
14. **Bridges to real stacks.** ROS 2, a CAN interface, and a socket for an external driving stack, so a
    real autonomy stack drives a simulated Voltec vehicle.

### 5. Digital twin and the Voltec vehicle

15. **Live twin sync.** Vehicle telemetry (CAN signals, GPS, IMU, battery) streamed into Eustress Streams
    and onto the twin in the Space in real time, with map matching to the lane graph.
16. **Georeferenced worlds.** Spaces placed on the real Earth (the geo crate) so a real route and its
    twin line up, and the road network imports from OpenStreetMap and OpenDRIVE.
17. **In-vehicle client on V-OS.**
    - **The platform:** the Voltec vehicle runs V-OS, the Voltec Operating System: a hard fork of Redox
      (the Rust microkernel OS) with Eustress built in.
    - **No IPC bridge:** the engine, its renderer and the Slint UI run as part of the system, not as an
      application talking to it across an IPC layer, so the twin renders at the latency and frame rate
      the hardware allows.
    - **Long-term direction:** V-OS as an industrial-strength operating system, on hybrid
      quantum and classical hardware, with Vortex super intelligence.
    - **What Eustress needs:**
      - a V-OS target for the engine and the Player (Redox's syscalls, scheme-based I/O, its graphics
        and input stack);
      - a Slint backend for V-OS;
      - a fixed latency and power budget for the in-car render;
      - streaming the twin from the vehicle's own data when offline.
18. **Safety boundary.**
    - **Display only:** the in-car twin is infotainment and only displays. Nothing it runs may command
      the vehicle.
    - **Driving functions:** anything that does goes through a functional-safety process: ISO 26262,
      ISO 21448 (SOTIF), ISO/SAE 21434 cybersecurity, UNECE R155 and R156 for over-the-air updates.
    - **Isolation on V-OS:** with Eustress inside the operating system, the boundary is drawn by V-OS
      itself. Driving functions run in their own isolated, certified partition that the twin can read
      from and never write to.
    - **Design:** the architecture keeps the boundary explicit from the start.

### 6. Realism of the world

19. **Road rendering.**
    - **Surfaces:** wet asphalt with puddles and spray, and retroreflective paint at night.
    - **Wear and lights:** tire marks and skid decals, and street-light pools.
    - **Vehicles:** headlights with real cut-off patterns, brake and indicator lights.
20. **Weather on surfaces.** Rain, wetness, snow and ice changing both the look and the tire grip.
21. **City content.** Building kits per district, props and interiors where the camera goes, and level of
    detail across a 10 km city.
22. **Audio.** Engine and motor synthesis from the drivetrain state, tire noise from slip, wind,
    sirens with Doppler, and city ambience.

### 7. Tools

23. **Editors.** The road and junction tools, a lane-section editor, a traffic and signal-plan editor, a
    scenario editor, and debug views (lane graph, agent intentions, sensor frustums, police state).
24. **Audit everything.** One audit per Space reporting road rule violations, junction conflicts,
    unreachable lanes, signal spacing, and scatter or furniture where the rules forbid it, so an AI can
    iterate against numbers.

## Vehicle Simulator: the first product

The driving world ships first as Vehicle Simulator, the official remaster, published on eustress.dev
with Early Access sold for about $10 worth of Tickets.

- **Chassis:** the Chassis Playground Space is where the modular chassis system is proven. A chassis is
  assembled from modules (frame, axles, suspension, wheels and tires, powertrain, body) on the engine's
  vehicle dynamics component. Its test benchmark has to pass before the modules converge into Vehicle
  Simulator.
- **Map:** Coastal Frontier, regenerated with the road system, then built out until the whole 10 km map
  is mapped: every road, junction, structure and district from the plan.
- **MVP, in order:**
  1. **Road system:** finished and compiled, then tested in the TerrainBenchmark Space.
  2. **Coastal Frontier:** regenerated with the new roads, then built out zone by zone until the whole
     map is done.
  3. **Chassis:** cars, trucks, semis and other land vehicles on the modular chassis, passing the
     Chassis Playground benchmark.
  4. **Vehicle Simulator:** the map and the vehicles together, with traffic enough to drive among,
     published on eustress.dev for Early Access.

The Early Access MVP needs items 1 to 4 and 7 below in a first form; sensors, the twin and the in-car
client follow.

## Order of work

1. Generic class access and class methods (item 1 and 2): small, and they unlock AI and scripting for
   everything after.
2. Road system phases 2 to 5 (item 3).
3. Vehicle dynamics (4), then the electric powertrain (5).
4. Traffic kernel (7) and the behaviour runtime (9), then pedestrians (8).
5. Sensors, ground truth and scenarios (11 to 13).
6. Live twin sync and the in-vehicle client (15 to 17), with the safety boundary (18) designed in at the
   start of that step.
7. Realism, audio and tools alongside, as each system lands.
