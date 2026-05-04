# Eustress CAD Toolset — Parametric Authoring for Simulation-First Worlds

> Sibling docs:
> - [TOOLSET.md](TOOLSET.md) — direct-manipulation editor tools (select/move/rotate/scale)
> - [ADORNMENT_ARCHITECTURE.md](ADORNMENT_ARCHITECTURE.md) — mesh-based handle system
> - [MANUFACTURING_PROGRAM.md](MANUFACTURING_PROGRAM.md) — Forge Bliss handoff
> - [ENGINE_BRIDGE.md](../ENGINE_BRIDGE.md) — JSON-RPC for external CAD integrations

## 1. The Thesis

CAD and game-engine authoring are two halves of one missing platform.

AutoCAD/Fusion 360/SolidWorks give you **parametric precision** but ship
your work to a dead file format the moment you're done. The model is
frozen. Want to simulate it under real physics in a real environment?
Export. Convert. Lose provenance. Re-author in another tool.

Roblox Studio / Unity / Unreal give you **runtime simulation** but
treat geometry as pixels — scale a dial, drag a slider, no parametric
history, no dimensions, no constraints. Edit the wrong field and
there's no "25.4 mm" to snap back to.

**Eustress merges both.** Every part is authored with CAD-grade
precision (dimensions, constraints, feature tree, history), stored as
plain-text TOML that git can diff, simulated live in physics the
moment it's placed. The same part becomes the prototype, the
tested assembly, and the CNC instruction set — with history preserved
all the way through.

The reference test: *an ME intern who spent a year in SolidWorks should
feel at home in Eustress the moment they open the Sketch tool, AND
they should be able to drop their assembly into a live world and watch
it run immediately.*

## 2. Design Principles

1. **Every geometry is parametric or it doesn't exist.** No "just a mesh"
   — every part has a feature tree. Mesh-only primitives remain for
   imported assets (GLB), but *authored* geometry is always parametric.
2. **Dimensions are first-class properties, not annotations.** A
   dimension in Eustress IS the source-of-truth for the geometry it
   measures. Edit the dimension and geometry follows.
3. **Constraints reference, not merely annotate.** A "parallel"
   constraint between two lines actually forces them parallel in the
   solver. Not a visual hint.
4. **Feature tree is a script.** The feature tree serializes to
   readable TOML / Rune that a reviewer can `git log` — not an opaque
   binary.
5. **Sim runs during modeling.** Rib is too thin → FEA shows red while
   you're still drafting. The distinction between "design phase" and
   "analysis phase" is dead.
6. **AI infers what you meant.** Sketch a rough rectangle → corners
   coincident, sides perpendicular, all inferred. User approves / edits.
   No manual constraint application for 80% of cases.
7. **Manufacturing path is one click.** Every parametric feature has
   a manufacturing translation (extrude → 3-axis mill path, revolve →
   lathe, sheet metal bend → press brake). Forge Bliss consumes the
   feature tree, not the mesh.
8. **Collaborative by default.** Two engineers can edit the same
   sketch at once; the CRDT-backed constraint solver reconciles.
   Onshape is the only commercial tool with this; we match it.
9. **Rune-scriptable features.** Any feature a user can build by hand,
   they can script. Custom "through-hole with countersink at depth D"?
   Thirty lines of Rune. Shared via Toolbox.
10. **Nothing ships without units.** Every value is unit-tagged
    (meter, stud, radian, kg, newton). Unit conversion is automatic and
    explicit — no accidental inches-as-meters.

## 3. What Already Exists (Honest Audit)

- **CSG crate** — boolean union / difference / intersect. **Now surfaced
  in the CAD tab's Boolean group** (`ribbon.slint`); menu actions route
  through `csg:union/negate/intersect/separate` to existing keybinding
  Actions in `ui/slint_ui.rs`. Still mesh-level, not parametric.
- **`classes::` Parts** — Transform + BasePart + rigid primitives. Not
  parametric; scale is cartesian.
- **Rune scripting** — live REPL, ECS mutation. Could drive feature
  scripts if APIs land.
- **Physics / Realism crate** — thermodynamics, fluid, material density.
  Ready to run on parametric parts the moment they exist.
- **Workshop (AI)** — generates geometry from prompts. Currently emits
  primitives; ready to emit feature trees instead.
- **File-system-first** — all entities as TOML. Parametric trees fit
  the same pattern (TOML list of features).
- **Hot reload** — edit TOML externally, engine reloads. Parametric
  re-evaluation fits here once it exists.

**What is missing: essentially everything CAD-specific.** The platform
can host it; nothing's been authored yet.

## 4. Feature Matrix — Eustress vs. the World

Legend: ✓ first-class, ● partial / awkward, ✗ absent.

### Sketching & Constraints

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| 2D sketch on plane                      | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Sketch on face of 3D part               | ✗       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Line / rectangle / circle / arc / polygon| ✓      | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Spline (NURBS / Bezier)                 | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Fillet in sketch                        | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Trim / Extend / Offset                  | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Dimension (linear)                      | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Dimension (angular, radial, diameter)   | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: coincident                  | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: parallel                    | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: perpendicular               | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: tangent                     | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Constraint: horizontal / vertical       | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: equal length / equal radius | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Constraint: symmetric                   | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Constraint: fix (lock in place)         | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Under-constrained detection (color code)| ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Over-constrained diagnosis              | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| AI-inferred constraints                 | ✗       | ✗        | ●      | ✗          | ✗       | ✗     | ✗      | **P1 (edge)**|
| Parametric equations (x = y × 2)        | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Variable table                          | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Unit-aware input (`2.5 mm`)             | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |

### 3D Features (Solid Modeling)

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Extrude (single distance)               | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Extrude (to-surface, to-plane, midplane)| ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Extrude with draft angle                | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Push / Pull (drag face direct)          | ✗       | ✓        | ✓      | ●          | ✓       | ✓     | ✗      | P0           |
| Revolve (around axis)                   | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Sweep (profile along path)              | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Loft (between profiles)                 | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Boundary surface (4-edge patch)         | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Helix (spring / thread)                 | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Rib (supporting feature)                | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Hole feature (parametric: dia/depth/tap)| ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Thread (internal / external)            | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Fillet (edge radius)                    | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Variable-radius fillet                  | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Chamfer (edge 45° or custom)            | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Shell (hollow with wall thickness)      | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Draft (per-face taper for molding)      | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Split body                              | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Combine / Boolean (union)               | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ●      | P0           |
| Combine / Boolean (difference)          | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ●      | P0           |
| Combine / Boolean (intersect)           | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ●      | P0           |
| Split / Section cut (view only)         | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |

### Patterns, Mirror, Arrays

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Linear pattern (N × step vector)        | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Rectangular pattern (2D grid)           | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Circular / radial pattern               | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Path-driven pattern (along curve)       | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Sketch-driven pattern (at each sketch pt)| ✗      | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Mirror (around plane)                   | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ●      | P0           |
| Mirror with instancing (links)          | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Skip instances (manual omit)            | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |

### Reference Geometry

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Reference plane (offset, 3-point, tangent)| ●     | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Reference axis (through edge, 2 points) | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Reference point (at vertex, on edge)    | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P0           |
| Coordinate system (local frame)         | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| 3D sketch (lines in space, not on plane)| ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |

### Assembly & Mates

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Mate: coincident (face-face)            | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Mate: concentric (holes to shaft)       | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Mate: distance (gap = N)                | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Mate: angle                             | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Mate: parallel / perpendicular          | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Mate: tangent                           | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Joint: revolute (hinge)                 | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ●      | P1 (Motor6D) |
| Joint: prismatic (slider)               | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ●      | P1           |
| Joint: ball (spherical)                 | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ●      | P1           |
| Joint: cylindrical                      | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Joint: universal / gear / rack-pinion   | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Degrees-of-freedom readout              | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Under / over-constrained assembly diagnose| ✗     | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Interference detection (static)         | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ●      | P1           |
| Motion study (dynamic simulation)       | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ●      | ✓ (play mode)|
| Contact sets (collision pairs)          | ✗       | ✗        | ●      | ✓          | ●       | ✓     | ✓      | ✓ (exists)   |

### Surface Modeling

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Extrude surface                         | ●       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Revolve surface                         | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Loft surface                            | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Sweep surface                           | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Boundary surface (N-sided patch)        | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Knit surfaces → solid                   | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Trim surface                            | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Offset surface                          | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Thicken surface (to solid)              | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| NURBS edit (CV handles)                 | ●       | ✗        | ✓      | ✓          | ●       | ✓     | ✗      | P2           |

### Sheet Metal

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Base flange                             | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Edge flange                             | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Bend (sharp or K-factor)                | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Unfold / flatten (flat pattern)         | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| DXF export of flat pattern              | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Punch / form                            | ✗       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Hem / jog / louver                      | ✗       | ✗        | ●      | ✓          | ✓       | ✓     | ✗      | P2           |

### Weldments (structural profiles)

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Structural member (along sketch path)   | ✗       | ✗        | ●      | ✓          | ●       | ✓     | ✗      | P2           |
| Trim/extend at intersection             | ✗       | ✗        | ●      | ✓          | ●       | ✓     | ✗      | P2           |
| Gusset / end cap                        | ✗       | ✗        | ✗      | ✓          | ✗       | ✓     | ✗      | P2           |
| Cut list / BOM                          | ✗       | ✗        | ●      | ✓          | ●       | ✓     | ✗      | P2           |

### Technical Drawings (2D from 3D)

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Orthographic projection views           | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Isometric view                          | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Section view                            | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Detail view (zoom inset)                | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Auto-dimensioning                       | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Title block + template                  | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| Hole callout + BOM                      | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| GD&T (tolerances / datums)              | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| PDF / DXF / DWG export                  | ✓       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |

### Manufacturing / Export

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| STEP export                             | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| IGES export                             | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ✗      | P2           |
| STL export (for 3D print)               | ✓       | ✓        | ✓      | ✓          | ✓       | ✓     | ●      | P0           |
| 3MF export                              | ●       | ●        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| OBJ / GLB / USD export                  | ✓       | ✓        | ✓      | ●          | ✓       | ●     | ●      | P0           |
| CAM integration (toolpaths)             | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | **Forge path**|
| BOM export (CSV / Excel)                | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Material cost roll-up                   | ✗       | ✗        | ●      | ✓          | ✓       | ✓     | ✗      | **Forge path**|
| Direct RFQ to manufacturer              | ✗       | ✗        | ●      | ●          | ●       | ●     | ✗      | **Forge edge**|

### Collaboration

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Real-time co-editing                    | ✗       | ✗        | ●      | ✗          | ✓       | ●     | ✓      | P1           |
| Branching (like git for CAD)            | ✗       | ✗        | ✗      | ●          | ✓       | ✓     | ✗      | ✓ (git)      |
| Comments / markup                       | ✓       | ●        | ✓      | ✓          | ✓       | ✓     | ●      | P1           |
| Review mode (read-only with comments)   | ●       | ✗        | ✓      | ✓          | ✓       | ✓     | ✗      | P1           |
| Permissions per part / feature          | ✗       | ✗        | ●      | ●          | ✓       | ✓     | ●      | P1 (KYC tie) |
| Locking (soft / hard)                   | ●       | ✗        | ✓      | ✓          | ●       | ✓     | ✓      | P0           |

### Scripting & AI

| Capability                              | AutoCAD | SketchUp | Fusion | SolidWorks | Onshape | CATIA | Roblox | **Eustress** |
|-----------------------------------------|:-------:|:--------:|:------:|:----------:|:-------:|:-----:|:------:|:------------:|
| Scripted feature authoring              | ✓ LISP  | ✓ Ruby   | ✓ Py   | ✓ VSTA     | ✓ FScr. | ✓ KWL | ✗      | **Rune**     |
| Natural-language modeling               | ✗       | ✗        | ●      | ✗          | ●       | ✗     | ✗      | **P1 edge**  |
| AI geometry generation                  | ✗       | ✗        | ●      | ✗          | ●       | ✗     | ✗      | ✓ Workshop   |
| AI constraint inference                 | ✗       | ✗        | ●      | ✗          | ✗       | ✗     | ✗      | **P1 edge**  |
| AI dimension suggestion                 | ✗       | ✗        | ✗      | ✗          | ✗       | ✗     | ✗      | **P2 edge**  |
| Semantic search over parts              | ●       | ✗        | ●      | ●          | ●       | ●     | ✗      | ✓ embedvec   |
| Auto-fix over/under-constrained         | ✗       | ✗        | ●      | ●          | ●       | ●     | ✗      | **P1 edge**  |
| Optimization (find best params)         | ✗       | ✗        | ●      | ✓ SimW     | ✗       | ✓ Opt | ✗      | **P2**       |

## 5. Tool Categories — Detailed Scope

### 5.1 Sketching (P0)

A **Sketch** is a 2D plane with a coordinate system, populated by
constrained primitives (lines, arcs, circles, splines, polygons, points)
and dimensions that drive their geometry.

#### Entry points
- Select a flat face on a 3D part → "Sketch on Face"
- Select a reference plane → "Sketch on Plane"
- Fresh sketch on world XY / XZ / YZ

#### Primitives
- Line (2 points)
- Polyline (chain of lines)
- Rectangle (2-point or center+corner)
- Circle (center+radius)
- Arc (center+start+end, or 3-point)
- Ellipse
- Polygon (inscribed / circumscribed, N sides)
- Spline (B-spline through N control points, or 2+ tangency points)
- Point
- Construction geometry (same primitives, but non-generating)
- Image (trace reference photo — architectural workflow)

#### Operations
- Trim (cut at intersection)
- Extend (to another entity)
- Offset (parallel at distance)
- Fillet (round intersection)
- Chamfer (bevel intersection)
- Mirror (about line)
- Pattern (linear / radial within sketch)
- Convert entities (project 3D edge into sketch)
- Offset entities (from projected geometry)

#### Dimensions
- Linear distance between points / edges
- Angular between lines
- Radial (of circle/arc)
- Diametral (of circle)
- Path length (total of polyline)
- Dimension as equation: `D1 = D2 × 2 + 5mm`

#### Constraints
- Coincident (point on point, point on line)
- Concentric (two arcs / circles share center)
- Collinear (two lines on same infinite line)
- Parallel
- Perpendicular
- Tangent (line tangent to arc)
- Horizontal / Vertical (relative to sketch axes)
- Equal length / Equal radius
- Symmetric (about construction line)
- Fix / Lock (no DOF)

#### Solver
- Gauss-Newton on residual vector of constraint equations
- Reports DOF: over / perfectly / under-constrained
- Color code: blue = under, black = perfect, red = over
- Per-constraint residual readout for debugging

#### AI inference (Eustress edge)
- When user drops a rectangle, infer perpendicular + parallel + dimensions
- When user draws 2 lines at 89.7°, suggest perpendicular
- When 3 circles within 2% of same radius, suggest equal-radius
- Applied as "suggestions" panel; user approves with 1 click

### 5.2 Features (Parametric 3D)

Every feature is a TOML entry in `<part>/features.toml` (ordered list).
Re-evaluating the feature tree produces the part's mesh. Edit a feature
→ downstream features regenerate.

#### Sketch-based features (require a sketch input)
- **Extrude** — blind distance, to-surface, to-plane, midplane, through-all, up-to-next
- **Revolve** — around an axis, angle 1–360°
- **Sweep** — sketch profile along a path (another sketch or 3D curve)
- **Loft** — between 2+ profile sketches, optional guide curves
- **Helix** — defined by pitch + height + revolutions + taper
- **Extrude cut** — same as extrude but boolean-difference
- **Revolve cut** — same for revolve

#### Reference-driven features
- **Fillet** — radius on selected edges (variable-radius P2)
- **Chamfer** — distance-distance, distance-angle, vertex-based
- **Shell** — hollow the part, wall thickness, open-face selection
- **Draft** — taper face relative to pulling direction (injection mold)
- **Rib** — thin supporting wall from a sketch + extrude-until-hit
- **Hole wizard** — parametric: dia, depth, counterbore, countersink, tap class
- **Thread** — external / internal, profile + pitch from standards table

#### Operation features (body-level)
- **Boolean** — union / difference / intersect between two bodies
- **Split body** — along a plane or surface
- **Scale** — uniform or non-uniform (relative to a reference point)
- **Move / rotate body** — feature-level translate (vs. assembly mate)
- **Pattern** — linear / circular / path / sketch-driven
- **Mirror body** — across plane, with optional instance linking

#### Surface features (P2)
- **Extrude surface**
- **Revolve surface**
- **Loft surface**
- **Sweep surface**
- **Boundary surface** (N-sided patch)
- **Offset surface**
- **Thicken surface** (to solid)
- **Knit surfaces** (collection → solid)
- **Trim surface** (with another surface / curve)
- **Extend surface**
- **Ruled surface** (between two curves)

### 5.3 Reference Geometry

- **Plane** — offset / angle from existing plane, 3-point, tangent-to-face, normal-to-curve
- **Axis** — along edge, intersection of 2 planes, through 2 points, normal to face
- **Point** — at vertex, midpoint, centroid, along edge at parameter, intersection
- **Coordinate system** — local frame (useful for imported CAD at arbitrary orientation)

All reference geometry is parametric — edit the defining inputs, the
reference updates, downstream features regenerate.

### 5.4 Feature History Tree

- Ordered list, top-to-bottom evaluation
- Rollback bar: drag past a feature to see the model without it
- Suppress (hide feature temporarily without deleting)
- Reorder (some features — mostly those without body-dependency)
- **Diff two commits in git** — parametric-aware diff (which feature
  changed vs. textual TOML diff)
- **Rename, group** (organization)
- **Error state** — feature regen failed (parent geometry gone, etc.),
  red X, click to diagnose

### 5.5 Parametric Equations

Variables panel on each Part:
```toml
[variables]
length     = "50 mm"
width      = "length * 0.6"
hole_dia   = "M6"                # from standards table
plate_qty  = "4"
```

Any dimension can reference any variable by name. Variable changes
propagate through the feature tree.

Standards tables — built-in lookups:
- Metric / imperial fastener dimensions
- Pipe schedules
- Structural profile catalogs (I-beam, C-channel, etc.)
- Sheet-metal gauge thicknesses

### 5.6 Assembly

An **Assembly** is a parent Model containing multiple Parts (possibly
other Models recursively) with **mates** that constrain their relative
positions.

#### Mate types
- Coincident (face-face, edge-edge, point-face)
- Concentric (cylindrical face-cylindrical face)
- Distance (parallel faces at fixed gap)
- Angle (between faces / edges)
- Parallel / Perpendicular
- Tangent (curved face to flat or curved face)
- Symmetric (across a plane)
- Lock (both as rigid)

#### Joints (mechanical DOF)
- Revolute (hinge — 1 rotational DOF)
- Prismatic (slider — 1 translational)
- Cylindrical (rotate + slide along axis — 2 DOF)
- Ball (3 rotational DOF)
- Universal (2 rotational, perpendicular axes)
- Gear (2 revolutes coupled by ratio)
- Rack-and-pinion (revolute coupled to prismatic)
- Planar (3 DOF in a plane)

Maps directly onto **Motor6D** / physics-constraints for the live
simulation. No separate "kinematic mode" — once the mate is solved,
the joint is live.

#### Diagnostics
- **DOF readout** — "this assembly has 3 translational DOF, 1 rotational"
- **Over-constrained warning** — redundant mates highlighted red
- **Under-constrained warning** — missing constraints to lock motion
- **Interference check** — static detection of intersecting bodies
- **Motion study** — drive a joint with a profile (constant velocity,
  ramped, from a time-series) and record frames for export

### 5.7 Drawings (2D Technical)

A **Drawing** is a paper-space document with views of a 3D model.

- **Views**: Front/Top/Right/Iso orthographic, projection to drawing plane
- **Section views** — cut along a line, show interior
- **Detail views** — circled region shown enlarged
- **Auxiliary views** — angled to a face
- **Exploded assembly views** (offset components along vectors)
- **Dimensions**: linear / radial / angular / ordinate / baseline
- **Annotations**: notes, arrows, balloons (for BOM)
- **GD&T symbols**: datum, feature control frames, tolerance stacks
- **Title block**: template with fields bound to document metadata
- **BOM table**: auto-generated from assembly structure, exportable
- **Export**: PDF, DXF, DWG, SVG

### 5.8 Manufacturing Pipeline (Forge Bliss integration)

Every parametric feature has a **manufacturing translation**:

| Feature           | Manufacturing method                          |
|-------------------|-----------------------------------------------|
| Extrude (prism)   | 3-axis mill pocket, laser cut (profile only)  |
| Extrude with draft| Injection mold cavity (draft for release)     |
| Revolve           | Lathe turning                                 |
| Hole feature      | Drilling → reaming → tapping sequence         |
| Fillet            | Ball-end mill path (3-axis or 5-axis)         |
| Thread            | Tap / thread mill / die                       |
| Shell             | Injection mold (walled part)                  |
| Sheet metal bend  | Press brake program                           |
| Structural member | Cut list with stock length + bevel angles     |

Forge Bliss consumes the **feature tree** directly, not an exported
mesh. It computes:
- Toolpath (G-code for CNC)
- Material volume + cost
- Setup operations + fixturing
- Bids from partner shops (RFQ with parametric cost roll-up)

The manufacturing handoff is **lossless** — an engineering change to
the part propagates straight into the production path.

### 5.9 Simulation Integration

Because physics / realism / thermodynamics all run in-editor, CAD-grade
analysis is a side effect of existing, not a separate phase:

- **Static stress analysis** — apply load, fix constraints → von Mises
  stress visualization, FEA mesh auto-generated from feature tree
- **Thermal analysis** — heat source + sinks → temperature distribution
- **Fluid flow** — inlet / outlet boundary conditions → streamlines
  (existing fluid crate)
- **Modal analysis** — natural frequencies + mode shapes
- **Fatigue** — cyclic loads → estimated cycle count to failure
- **Interference check during motion** — play a motion study,
  Avian3D contact detection flags collisions
- **Center-of-mass / moment-of-inertia** — live readout as part evolves

None of these require a separate "Simulation" application. The feature
tree is the input; the same mesh that renders is the mesh analyzed.

### 5.10 AI Assistance (Eustress edge)

#### AI-inferred constraints (P1)
When a user draws a sketch, the solver + an LLM agent observe the
drawn entities and suggest constraints:
- "These two lines differ by 0.3° from perpendicular. Apply perpendicular?"
- "These three circles have radii 5.01, 5.00, 4.99. Apply equal-radius?"

Suggestions appear as ghost constraints; user approves / dismisses in
bulk.

#### AI natural-language modeling (P1)
Workshop command: *"extrude this rectangle 20mm with a 5mm fillet on
all edges"*. Translates to feature-tree additions. Verified via preview
before commit.

#### AI dimension suggestion (P2)
After a sketch is drawn but before it's fully dimensioned, the system
proposes a minimal dimensioning scheme that would fully constrain it.
Especially useful for users new to parametric CAD.

#### AI auto-fix (P2)
Over-constrained sketch → propose which constraint to remove. Regen
error → propose parent geometry fix. Trained on diagnostic history.

#### Semantic search over part library (uses embedvec)
*"bracket with mounting holes"* returns matching parts across the
Toolbox + current project.

#### Parameter optimization (P2)
Objective: minimize mass. Constraints: max stress < yield. Variables:
thickness, rib count, hole positions. Engine runs gradient-based
search in simulation, reports Pareto-optimal designs.

### 5.11 Collaboration

- **Real-time co-editing** — CRDT on the constraint graph + feature
  tree. Two users can edit the same sketch; constraints auto-merge.
- **Branching** — piggyback on git. Every save is a commit; branches
  are natural. Feature-tree-aware diff on review.
- **Comments** — thread anchored to a feature / face / edge, persists
  in TOML metadata.
- **Review mode** — read-only view with comment rights.
- **Permissions** — KYC-gated (see `reference_kyc.md`). Certain jurisdictions
  restrict export of certain geometry classifications.

### 5.12 Scripted Features (Rune)

Custom features are first-class. A Rune script that defines a feature
registers it in the Toolbox:

```rust
// my_countersunk_hole.rune
feature "countersunk_hole" {
    inputs: [
        sketch_point p,
        length diameter     = 5 mm,
        length depth        = 10 mm,
        length csk_diameter = 8 mm,
        angle csk_angle     = 90 deg,
    ]
    operation: {
        let cyl   = extrude_cut(circle(p, diameter / 2), depth);
        let cone  = revolve_cut(
            triangle_profile(p, csk_diameter, csk_angle),
            axis_through(p, normal(face_of(p)))
        );
        cyl + cone
    }
}
```

Publish to Toolbox → available as a feature on any sketch in any part.
Versioned with the project (hot-reloadable).

## 6. Architecture

### 6.1 Feature Tree Storage

Each Part stores its feature tree at `<part>/features.toml`:

```toml
[sketch.Sketch1]
plane = "XY"
entities = [
    { type = "line",   p1 = [0,0],    p2 = [50,0]  },
    { type = "line",   p1 = [50,0],   p2 = [50,30] },
    { type = "line",   p1 = [50,30],  p2 = [0,30]  },
    { type = "line",   p1 = [0,30],   p2 = [0,0]   },
]
constraints = [
    { type = "perpendicular", e1 = 0, e2 = 1 },
    { type = "perpendicular", e1 = 1, e2 = 2 },
    { type = "perpendicular", e1 = 2, e2 = 3 },
    { type = "horizontal",    e1 = 0 },
]
dimensions = [
    { type = "linear", e1 = 0,      value = "length" },
    { type = "linear", e1 = 1,      value = "width"  },
]

[feature.Extrude1]
sketch  = "Sketch1"
depth   = "height"
op      = "new_body"

[feature.Fillet1]
edges   = ["Extrude1/edge-0", "Extrude1/edge-2"]
radius  = "fillet_r"

[variables]
length    = "50 mm"
width     = "30 mm"
height    = "20 mm"
fillet_r  = "5 mm"
```

Re-evaluation is deterministic: variables → sketches (solve) →
features (evaluate) → final BRep / mesh.

### 6.2 Constraint Solver

**Gauss-Newton on residual vector.** For each constraint, a function
`r(entity_positions) → real` where `r=0` means satisfied. Jacobian
computed analytically per constraint type. Line search for step size.
Tolerance 10⁻⁷ m.

Fallbacks:
- If Jacobian is singular (under-constrained) → SVD, use minimum-norm step
- If residual doesn't converge in 50 iters → flag as over-constrained

### 6.3 BRep / Mesh Layer

Feature evaluation produces a **BRep** (boundary representation:
vertices, edges, faces with parametric geometry backing). BRep tessellates
to a mesh for display / physics / export. BRep is what fillets /
chamfers / surfaces operate on (mesh-level is too lossy).

Backend choice:
- **Option A**: Integrate an existing BRep kernel (OpenCascade via FFI)
- **Option B**: Build a minimal BRep over our existing mesh crate
- **Option C**: Use a pure-Rust kernel (`truck` crate looks promising)

Decision deferred to implementation phase; keep the API boundary small
so we can swap.

### 6.4 Units Everywhere

Every length / angle / mass / force value is a `Quantity { value: f64,
unit: UnitId }`. Unit catalog defined in a shared crate. Operations
between incompatible units panic (or return Err) — never silently
convert. UI shows user's preferred unit; solver uses SI internally.

### 6.5 Hot Reload

Edit `features.toml` externally → file watcher re-evaluates → mesh
regenerates → physics rebinds. Same infrastructure as existing
file-loader hot reload; feature-tree eval is the only new hop.

## 7. Phases

### Shipped so far (as of 2026-04-22)

The CAD tab exists in the ribbon with four shipped groups:
- **Smart Edit** — Gap Fill / Resize Align / Edge Align / Part Swap /
  Mirror
- **Align** — Align X/Y/Z Center + Distribute X/Y/Z
- **Pattern** — Linear / Radial / Grid Array (Phase-1, Part-level —
  sibling to the parametric feature-tree Pattern which remains P0 of
  this doc)
- **Boolean** — Union / Subtract / Intersect / Separate (mesh-level
  CSG via existing keybinding Actions; feature-tree variant remains
  Phase-0 of this doc)

Sketch / Features / Modify groups still have placeholder strips.

### BRep kernel adoption — `truck` (2026-04-22)

Per-user decision: `truck` is the chosen pure-Rust BRep kernel. New
[`eustress-cad`](../../eustress/crates/cad/) workspace crate wraps it
with Eustress-native types:

- [`quantity.rs`](../../eustress/crates/cad/src/quantity.rs) —
  `Quantity` unit-tagged scalar with parser (`"50 mm"`, `"90 deg"`,
  `"1.5m"`, `"2 studs"`), `to_si()`, full length / angle / mass /
  force family
- [`feature_tree.rs`](../../eustress/crates/cad/src/feature_tree.rs) —
  `FeatureTree { variables, entries, metadata }` + `FeatureEntry::
  {Sketch, Feature, Suppressed}`; TOML I/O via `load_tree()` /
  `save_tree()`; variable-expression resolution with cycle guard
- [`sketch.rs`](../../eustress/crates/cad/src/sketch.rs) — `Sketch`
  with `SketchEntity::{Line, Rectangle, Circle, Arc, Point,
  Construction}`, `SketchDimension::{Linear, Radial, Angular}`,
  `ConstraintKind` enum with all 12 Phase-0 constraints
- [`feature.rs`](../../eustress/crates/cad/src/feature.rs) — `Feature`
  tagged enum covering every Phase 0-2 op: Extrude / Revolve /
  Fillet / Chamfer / Shell / Sweep / Loft / Hole / Mirror / Pattern /
  ReferencePlane / Boolean / Split; `FeatureOp::{NewBody, Add,
  Subtract, Intersect}` for how each feature combines with the
  running body; `EndCondition`, `PatternKind`, `BooleanOp` supporting
  enums
- [`eval.rs`](../../eustress/crates/cad/src/eval.rs) —
  `evaluate_tree()` deterministic walker producing `Option<Solid>` +
  tessellated mesh + per-entry status for the feature-tree UI panel.
  **Extrude is the canonical working evaluator** (rectangle sketch →
  prism via truck's `builder::tsweep`); every other feature variant
  is typed + routed, with `evaluate_feature_into_body` returning a
  clean `NotImplemented` error that the UI surfaces as a red-X entry
  status. Per-feature PRs land incrementally against this pipeline
- [`error.rs`](../../eustress/crates/cad/src/error.rs) — 9 typed
  error variants (`UnitMismatch`, `EvalFailed`, `SketchNotFound`,
  `UnderConstrained`, `OverConstrained`, `NotImplemented`, `Kernel`,
  `Parse`, `Serialize`, `Io`)

**Truck sub-crates wired** in `eustress/Cargo.toml`: `truck-base` /
`truck-geometry` / `truck-topology` / `truck-modeling` /
`truck-meshalgo` / `truck-shapeops` / `truck-stepio`.

The per-feature status column below reflects the actual landing
order: **SCAFFOLD** = typed + tree-routed + NotImplemented error;
**SHIPPED** = evaluator wired + produces a truck body. STEP import/
export gets the `truck-stepio` dep free — Phase 1 STEP export lands
as a thin wrapper when mesh tessellation glue is written.

### Phase 0 — CAD Foundation (blocks Forge Bliss parametric path)

- [x] Unit-aware `Quantity` type across crates *(shipped: [quantity.rs](../../eustress/crates/cad/src/quantity.rs))*
- [x] Feature-tree TOML schema + loader *(shipped: [feature_tree.rs](../../eustress/crates/cad/src/feature_tree.rs) + `load_tree()`/`save_tree()`)*
- [~] Sketch entity + solver (primitives, dimensions, core constraints) *(entities + dimensions + constraint types shipped in [sketch.rs](../../eustress/crates/cad/src/sketch.rs); Gauss-Newton constraint solver itself is the next increment)*
- [~] Sketch on plane / Sketch on face *(Sketch carries a `plane: String` that accepts `"xy"`/`"xz"`/`"yz"` OR a face reference like `"Extrude1/face-0"`. Face-reference resolution lands with the Fillet/Chamfer pass)*
- [x] Extrude + Extrude Cut *(shipped: [eval.rs](../../eustress/crates/cad/src/eval.rs) — `Extrude` variant with `combine: FeatureOp::{NewBody, Add, Subtract, Intersect}`. Supports Rectangle / Circle / closed-polyline profiles via `build_planar_face`; `both_sides` symmetric extrusion; Subtract + Intersect route through truck-shapeops)*
- [x] Revolve + Revolve Cut *(shipped: [eval.rs](../../eustress/crates/cad/src/eval.rs) — `builder::rsweep` around world x/y/z axes; arbitrary angle + combine-mode. Edge-reference axes land with reference-tree plumbing)*
- [~] Fillet + Chamfer (edge-level) *(scaffolded: `Feature::Fillet { edges, radius, propagate_tangent }` + `Feature::Chamfer { edges, distance, distance2, angle }`; evaluator returns typed `NotImplemented` pending `truck-shapeops` upstream fillet/chamfer API stabilization — tracked for auto-upgrade when the dep version bumps)*
- [x] Linear + Circular Pattern (feature-tree-level) *(shipped: `pattern_linear` via translation loop, `pattern_circular` via rotation loop around any world axis, both producing pairwise-unioned bodies. Full-360° sweeps divide by `count`, partial by `count-1` for exact endpoints. Path + Sketch-driven patterns return typed `NotImplemented` pending path-sketch resolver)*
- [x] Mirror (plane) — **feature-tree level** *(shipped: `mirror_bodies` uses `builder::transformed` with a reflection matrix `I - 2nnᵀ` through the plane origin. Operates on either the running body or explicit feature list; unions and combines via `FeatureOp`. Sibling to the Part/Model-level `tools_smart::ModelReflect` in [TOOLSET.md](TOOLSET.md) §4.13.6)*
- [x] Boolean Union / Difference / Intersect *(shipped end-to-end:
      ribbon UI on CAD tab (mesh-level CSG) + feature-tree variant
      via `truck_shapeops::or` / `::and` / `::not` in [eval.rs](../../eustress/crates/cad/src/eval.rs).
      `Feature::Boolean { target, op }` references another feature's
      output body; `FeatureOp::{Add, Subtract, Intersect}` route
      through the same boolean helpers so every feature combines
      cleanly)*
- [~] Reference Plane / Axis / Point *(scaffolded: `Feature::ReferencePlane { plane: ReferencePlane::{Offset, ThreePoint, TangentFace, NormalToCurve} }`; evaluator is pass-through (doesn't produce a body). Axis + Point variants land as sibling `Feature::ReferenceAxis` / `Feature::ReferencePoint` in the next increment — same pattern)*
- [ ] Feature tree UI panel with reorder / suppress / delete — `FeatureEntry::Suppressed` variant shipped in the data model; Slint panel pending
- [~] Parametric variables with equation parser *(shipped: `FeatureTree.variables: HashMap<String, String>` with `resolve_quantity_depth` cycle-guarded lookup. Full math-expression evaluation (e.g. `length * 0.6 + 5mm`) lands by porting the `numeric_input::eval_expression` work from the engine crate)*
- [ ] Hot reload of features.toml — loader is pure; Bevy plugin wiring lands as part of the engine-side integration PR
- [~] STL / OBJ export from feature tree *(free once `tessellate()` in [eval.rs](../../eustress/crates/cad/src/eval.rs) is wired — `truck-meshalgo` exposes `obj::write` natively; STL via `stl_io` is a 5-line follow-up)*
- [ ] Lock / soft-lock on part during edit — Bliss role-stamp integration needed; Phase 2 Engine-Bridge concern

### Phase 1 — Production-Grade Authoring

- [ ] Sweep + Loft + Helix
- [~] Shell + Hole Wizard *(Hole Wizard shipped in [eval.rs](../../eustress/crates/cad/src/eval.rs): `Feature::Hole` decomposes into a circular extrude-cut + optional counterbore union + approximated countersink; diameter / depth / counterbore / countersink all parameterized through `Quantity`. Shell remains blocked on truck-modeling adding a shell operation upstream or a standalone offset-surface implementation)*
- [ ] Push/Pull (direct face drag)
- [ ] Path + Rectangular Pattern
- [ ] Reference coordinate system
- [ ] Standard-driven dimensions (M6 bolt table, pipe schedules)
- [ ] Assembly mates: coincident, concentric, distance, angle
- [ ] Joint types → Motor6D / physics constraints
- [ ] DOF readout + under/over-constrained diagnosis
- [ ] Interference check (static)
- [ ] Motion study (drive joints, record)
- [ ] AI-inferred constraints on sketch
- [ ] AI natural-language modeling commands (Workshop integration)
- [ ] STEP export
- [ ] BOM export (CSV)
- [ ] Forge Bliss parametric handoff (initial: extrude / revolve / hole)
- [ ] Comments / markup anchored to features
- [ ] Real-time co-edit (CRDT on sketch + feature tree)
- [ ] Semantic search over **part library / Toolbox** (embedvec over
      external assets). Distinct from the scene-entity variant "AI
      Select Similar" in [TOOLSET.md](TOOLSET.md) Phase 2 — sibling
      implementations sharing embedvec infrastructure.
- [ ] Named selection sets, inverse selection, class/tag/material
      filters all ship in [TOOLSET.md](TOOLSET.md) Phase 1 — no
      CAD-specific variant needed

### Phase 2 — Advanced & Differentiating

- [ ] Variable-radius fillet
- [ ] Draft feature (molding)
- [ ] Rib feature
- [ ] Thread feature (internal + external)
- [ ] Surface modeling suite (extrude / revolve / loft / sweep / patch / knit / trim / thicken)
- [ ] 3D sketch (lines / splines in space)
- [ ] Sheet metal (base flange, edge flange, bend, unfold, DXF)
- [ ] Weldments (structural members, trim, gusset, cut list)
- [ ] Technical drawings (views, section, detail, dimensions, BOM, GD&T)
- [ ] PDF / DXF / DWG export
- [ ] Advanced joints (cylindrical, universal, gear, rack-pinion)
- [ ] Contact sets with friction / restitution (simulation tie)
- [ ] AI dimension suggestion
- [ ] AI auto-fix for over/under-constrained
- [ ] Parameter optimization (gradient + simulation)
- [ ] Scripted feature authoring (Rune Toolbox publish)
- [ ] GD&T authoring + tolerance stack analysis
- [ ] Branching + parametric-aware git diff
- [ ] IGES export
- [ ] CAM toolpaths → Forge Bliss / direct post

## 8. Non-Goals

- **Full BRep kernel from scratch.** Either integrate (OpenCascade /
  CGAL / truck) or license. Reinventing is a PhD thesis.
- **Sub-surface scattering / film VFX.** Eustress is real-time sim, not
  offline render.
- **FEA solver from scratch.** Integrate MFEM / deal.II / Kratos or
  similar. Wrap, don't rewrite.
- **CAM post-processors for every machine on earth.** Forge Bliss
  partners provide machine-specific posts; we provide the universal
  pre-post feature tree.
- **Legal-spec GD&T compliance (ASME Y14.5 edge cases).** We implement
  the 90% that users actually use; full compliance is a certification
  problem.
- **Drafting paper space as a first-class surface.** 2D tech drawings
  are an export, not a live editor. No AutoCAD Paper-Space equivalent.
- **Replace Blender for sculpt / retopo.** Import / export is the seam.

## 9. Success Metrics

For each Phase-0 feature, beat the commercial CAD baseline on one
dimension:

| Task                                             | Fusion / SolidWorks baseline | Eustress target      |
|--------------------------------------------------|------------------------------|----------------------|
| Create a parametric rectangular plate w/4 holes  | 60–90s                       | ≤30s (AI infer)      |
| Change a dimension after placing 20 parts        | 5–30s (regen)                | ≤1s (hot reload)     |
| Diff two versions of a feature tree              | GUI tree side-by-side        | git + parametric diff |
| Collaborate with a remote teammate on a sketch   | "check out" workflow         | live co-edit (Onshape parity) |
| Run stress analysis on a new part                | Separate Sim module, 2–5min  | in-editor, <10s      |
| Generate a manufacturing quote                   | Export → email → wait hours  | 1-click Forge RFQ    |
| Extend the toolset with a custom feature         | VSTA / API / compile         | 30-line Rune script  |
| Import an existing STEP file and edit it         | STEP → native, parametric loss | P2: feature recognition |

Plus system-level:
- **Sketch solver convergence**: <50ms for 100-entity sketch
- **Feature tree regen**: <200ms for 50-feature part
- **Hot-reload latency**: <250ms from save to rendered geometry
- **STEP import fidelity**: ≥95% feature recognition (P2)

## 10. Ship or Die

CAD-grade authoring in a game engine is a platform-level unlock. It's
also the step that most commercial simulation tools never take — they
treat authoring as someone else's problem. Every category in §4 that
reads ✗ for Roblox is a wedge we can drive through. Fusion / SolidWorks
can't answer our integration story; game engines can't answer our
precision story. We do both, or we're just another game engine with a
nicer tree view.

---

*Last updated: 2026-04-22. Keep this in sync with
[TOOLSET.md](TOOLSET.md) and [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md)
as features land — or this becomes a wishlist.*
