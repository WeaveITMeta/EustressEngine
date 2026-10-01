# Roblox Scripts in a Metre World

Status: proposal, for McKale's decision. Owner of the Luau side: the mlua
session.

A place imported from Roblox lands in metres: its geometry is converted at the
importer, at 0.28 m a stud. Its scripts still think in studs. They write
`door.Position + Vector3.new(0, 10, 0)`, cast rays 100 studs long, and move
characters at 16 studs a second. Every length that crosses between such a
script and the engine is off by the stud factor, so an imported script and
its imported geometry disagree by a factor of 3.6.

The stud round converts the Humanoid's movement values (WalkSpeed,
JumpHeight, HipHeight, JumpPower, StarterPlayer's `Character*`) and the speeds
`Humanoid.Running` and `Climbing` report. This proposal covers every other
length.

## The rule

A script the importer marks as written for Roblox (`ScriptOrigin = "roblox"`,
from `[script] origin` in its file) sees a stud world. Every length that
crosses the Luau boundary converts: into metres when the script hands it to
the engine, into studs when the engine hands it to the script. The script's
own arithmetic stays in studs throughout, so `part.Position.Y +
part.Size.Y / 2` is the top of the part in studs, exactly as on Roblox.

Which script's numbers these are is decided the way the stud round decides
it: by the script that owns the running thread, with tweens carrying the
units of the script that made them. A native script, written in metres, sees
the engine unchanged.

## What a length is: a unit table

The conversion keys on each property's unit, never on its value. The
reflection database gives every property's type, but not its unit: a Vector3
is a position on `Part.Position`, degrees on `Part.Orientation`, and
radians a second on `AssemblyAngularVelocity`. So the table is curated, one
entry per (class, property) whose unit involves length, generated as a
candidate list from the reflection database (every Vector3, CFrame, Ray,
Region3 and number property of a class the engine supports) and classified
once by hand:

| Unit | Converts by | Examples |
|---|---|---|
| Length | × stud | Size, Position, BlastRadius, Range (lights), MaxDistance, Length (Rope, Rod), FreeLength, CameraMinZoomDistance |
| Position frame | translation × stud, rotation as is | CFrame, WorldPivot, Focus, C0, C1, Attachment.CFrame, Mouse.Hit, Mouse.Origin |
| Speed | × stud | AssemblyLinearVelocity, Velocity, VectorVelocity (LinearVelocity), Velocity and Speed (PrismaticConstraint) |
| Acceleration | × stud | Workspace.Gravity, ParticleEmitter.Acceleration |
| Ray | origin and direction × stud | Mouse.UnitRay (origin only; a unit ray stays unit) |
| Unitless | none | Transparency, Orientation, Rotation (degrees), AssemblyAngularVelocity, FieldOfView, Color, UDim2, TextSize |

A test lists every candidate the reflection database offers for the
supported classes and fails on one the table has not classified, so a class
added later cannot silently skip the question.

## What it covers

**Properties**, on get and set, by the table above: BasePart (Position, Size,
CFrame, velocities, pivots), Model (WorldPivot and the pivot methods),
Attachment, Camera (CFrame, Focus, zoom distances), Workspace.Gravity,
constraints (lengths, limits, speeds), lights (Range), sounds (roll-off
distances), explosions, particles (Speed, Acceleration), beams and trails
(widths), BillboardGui (StudsOffset, MaxDistance), and terrain lengths. Tween goals on those properties convert
through the same path, since a tween writes through the property boundary
with its creator's units.

**API calls**, arguments in and results out:

- `workspace:Raycast(origin, direction, params)`, and its result's
  `Position` and `Distance`; `FindPartOnRay` and `Ray.new` values passed to
  it.
- `GetPartBoundsInRadius(position, radius)`, `GetPartBoundsInBox(cframe,
  size)`.
- `Model:MoveTo`, `TranslateBy`, `PivotTo`, `GetPivot`, `GetBoundingBox`,
  `GetExtentsSize`, `SetPrimaryPartCFrame`.
- `Humanoid:MoveTo(position)`. `Humanoid:Move(direction)` is a direction, and
  unitless.
- `Camera:ViewportPointToRay(x, y, depth)`, `WorldToViewportPoint(point)` (the
  depth it returns), `ScreenPointToRay`.
- `Player:DistanceFromCharacter(point)` and its result.
- Terrain: `FillBall(center, radius)`, `FillBlock(cframe, size)`,
  `FillCylinder`, `FillRegion`, `ReadVoxels` and `WriteVoxels` regions and
  resolution, `WorldToCell`, `CellCenterToWorld`.

**Signals** that hand scripts a length, such as `Explosion.Hit`'s distance
once the engine fires it, flagged in the catalog next to their parameters so
the prelude converts them the way it converts `Humanoid.Running` today.

## Where it breaks

The boundary converts what crosses between a script and the engine. It cannot
convert what crosses between two scripts, because a plain number carries no
unit. That matters only where a Roblox-origin script and a native script
share values:

- **Attributes**: `SetAttribute("Spawn", Vector3.new(0, 10, 0))` from a Roblox
  script stores studs; a native script reading it gets studs. An attribute
  has no declared unit, so it stays as written.
- **Remotes and BindableEvents**: arguments cross as values. A Roblox script
  firing a position to a native handler sends studs.
- **ModuleScripts**: a native script requiring a Roblox-origin module runs the
  module's code on its own, native, thread: the module's literals are studs
  but nothing converts them. A place is usually one or the other, so this
  shows up only when Roblox code is dropped into a native Space.
- **Value objects**: a `Vector3Value` or `NumberValue` holds whatever its
  writer meant. Between two Roblox scripts that is consistent; between a
  Roblox script and a native one it is studs.
- **Gravity is Roblox's in an imported Space.** Roblox's gravity is 196.2
  studs/s², which at 0.28 m a stud is 54.9 m/s², about 5.6 g, and an
  imported Space keeps it, so a script that assumes 196.2 (a projectile arc,
  a jump solver) matches the physics. `workspace.Gravity` converts like any
  acceleration and reads 196.2 there. Masses and forces are left as they are:
  Roblox's mass unit follows its own density, which converting lengths does
  not reach, so a `VectorForce` tuned on Roblox may push differently.

Within one Roblox-origin place, every script sees studs and the engine sees
metres, so the only place the edges above apply is mixed content.

## Cost

A property read already takes the DataModel lock, looks up the class, checks
methods, service members and signals, and copies the value out. The unit
table adds one hash lookup on (class, property). Only a length property
continues: one call into the prelude to ask whether the running thread's
owner works in studs (a few hundred nanoseconds), then a multiply. A Vector3
read already builds a new value, so converting its components costs nothing
extra.

For scale: 100 zombies each reading and writing Position every frame at 60
frames a second is 12,000 length accesses a second, about 4 ms of CPU a
second, under 0.5% of one core, and only in Roblox-origin places. Native
places pay the one hash lookup.

API conversions wrap about 30 methods; each converts its few arguments and
results only when the caller works in studs.

## How a test proves it

1. **Round trips**, per unit class: a Roblox-origin script writes each length
   property and reads it back unchanged in studs, while the tree holds metres
   at exactly value × stud.
2. **An imported fixture place lines up with its geometry.** A small `.rbxl`
   with a floor, a door and a script, run through the real importer, then
   played:
   - the script raises the door 10 studs (`door.Position += Vector3.new(0,
     10, 0)`), and the door rises exactly 10 × 0.28 m;
   - the script casts a ray from the door straight down and prints the
     distance, which equals the imported gap in studs;
   - the script sets a crate's CFrame so it rests on the floor, computed from
     `floor.Position.Y + floor.Size.Y / 2 + crate.Size.Y / 2`, and the crate
     neither sinks nor floats (no contact overlap, gap under 1 mm);
   - a native script in the same place reads the same door in metres.
3. **The mixed edges are pinned**, so they stay the documented behaviour: an
   attribute a Roblox script writes reads as studs from a native script.
4. **Coverage**: every length candidate in the reflection database for a
   supported class is in the unit table.

## Other ways considered

- **Import the place at stud scale.** Keep 1 stud = 1 engine unit for an
  imported Space and set its gravity to 196.2: scripts and geometry agree
  trivially, and Roblox physics feels the same. The place no longer sits at
  true scale beside native content, which is the reason for the metre world.
- **Convert only the Humanoid** (the stud round) and flag length literals in
  Roblox-origin scripts in the editor. Cheap, and every imported script that
  moves a part is still wrong at runtime.
- **Rewrite imported scripts at import**, multiplying literals. A literal
  carries no unit either (`wait(10)` beside `Vector3.new(0, 10, 0)`), and a
  rewrite would change code the user owns.

## Recommendation

Convert by unit at the boundary, as above, in two steps:

1. Parts, models, attachments, the camera, the mouse and workspace gravity,
   plus the spatial API (Raycast, bounds queries, pivots, bounding boxes,
   MoveTo). That is what imported gameplay scripts touch most, and the
   fixture test proves it.
2. Constraints, lights, sounds, particles, beams, GUIs in the world and
   terrain.

Leave mass and force as they are, document the mixed-origin edges in the
scripting guide, and have the importer write a note into each imported
Space that names them, so a user who drops Roblox code into a native Space
knows what to expect.
