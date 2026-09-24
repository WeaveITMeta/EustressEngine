# CAD Kernel v2

`eustress-cad` is a parametric B-rep modeler on the pure-Rust `truck` kernel. A part is a feature tree (TOML); evaluating it yields bodies, each a truck solid with a persistent name for every face. This document covers the kernel's model, its naming scheme, what each feature does, and the limits it works within. The phase plan that surrounds it is `CAD_PLATFORM_PLAN.md`.

## Model

| Layer | Module | Role |
|---|---|---|
| Placement | `frame.rs` | Every sketch lives in a right-handed frame resolved from `Sketch.plane`. |
| Profiles | `profile.rs` | Sketch entities weld into loops; nesting gives regions with holes. |
| Bodies and names | `topology.rs` | A part is `Vec<Body>`; every face has a name that survives regeneration. |
| Builders | `build.rs` | Prism, ruled loft, revolve, slab, hole cutters, countersink cone. Each returns the solid plus a name per face. |
| Blends | `blend.rs` | Fillet and chamfer on named edges. |
| Evaluator | `eval.rs` | Walks the tree, applies features, combines bodies, tessellates. |
| Sketcher | `solver.rs` | Levenberg-Marquardt over constraints and dimensions, DOF by Jacobian rank. |
| Exchange | `step.rs`, `export_glb.rs` | STEP AP214 (exact B-rep, mm) and GLB (mesh). |

### Planes

| Name | x | y | normal |
|---|---|---|---|
| `xy` | +X | +Y | +Z |
| `xz` | +X | -Z | +Y (the Y-up ground plane: parts sketched here stand on the floor) |
| `yz` | +Y | +Z | +X |

A plane can also be a `ReferencePlane` feature (offset, three-point, tangent to a planar face, normal to a straight edge) or a planar face by name. A sketch on a face uses the world origin projected into the face plane, with axes taken from the world axes, so its coordinates read like world coordinates and do not move when an upstream edit reshapes the face.

### Profiles

Every non-construction line, arc, circle and rectangle takes part. Segments weld end to end (tolerance scaled to the sketch), loops nest by containment, and the even-odd rule turns nesting into regions: an outer boundary plus the holes directly inside it; an island inside a hole is its own region. A point where three or more segments meet is refused with its location. Open chains are counted and reported, never silently extruded.

### Bodies

| `combine` | Effect |
|---|---|
| `new_body` | Adds a body. |
| `add` | Joins every body the feature touches into one (the oldest keeps its name); a feature touching none becomes a body. |
| `subtract` | Cuts every body it touches; touching none is an error. |
| `intersect` | Keeps what each body shares with the feature; bodies sharing nothing are removed. |

`bodies = [...]` restricts a feature to named bodies. Bodies are named after the feature that created them. A feature that fails leaves every body as it was.

Whether a boolean that returned nothing failed or had nothing to do is decided by tessellated distance plus a containment probe, so disjoint geometry is never reported as a failure and a real failure is never mistaken for disjoint geometry.

## Names

| Face | Name |
|---|---|
| Extrude caps | `Extrude1.cap_start` (sketch-plane side), `Extrude1.cap_end` |
| Wall swept from sketch entity 3 | `Extrude1.side.e3` |
| One side of a rectangle (entity 0) | `Extrude1.side.e0.bottom` / `.right` / `.top` / `.left` |
| Second region of a multi-region extrude | caps `Extrude1.cap_end.r2` |
| Hole bore | `Hole1.wall.0`, `Hole1.wall.1`; `Hole1.floor` for a blind hole |
| Blend face k | `Fillet1.face.0` |
| Shell cavity | `Shell1.inner.side.e0.top` |
| Pattern copy 2 | `Hole1.wall.0@Pattern1.2` |
| Mirror copy | `Extrude1.cap_end@Mirror1` |

An edge is named by the two faces it separates, sorted: `Extrude1.cap_end | Extrude1.side.e0.top`. Lookups accept either order. Pieces of one face (a face cut in two) share its name, numbered `#2`, `#3` by position.

Names survive booleans because truck-shapeops builds every result face on a surface taken unchanged from an operand (`divide_one_face` reuses `face.surface()`). Each result face is matched to the operand face with the same surface: planes by their defining points (so two coplanar faces from different features keep their own names), other surfaces by sampling their parameter range. When two operand faces share a surface, the older feature's name wins.

`cad_list_topology` lists every body, face and edge by name, with planarity, normals, end points and lengths, optionally sorted by distance from a point.

## Features

| Feature | Notes |
|---|---|
| Extrude | `blind` (negative depth or `reverse` goes against the normal; a cut or intersect given neither goes into the material when only that side has any, so a cut sketched on a face cuts into the part), `mid_plane`, `through_all` (through every body, both ways), `to_plane` / `to_surface` (a plane or planar face; parallel targets are exact spans, oblique ones are trimmed by the plane), `up_to_next` (the first planar face in the way, found by ray cast). `draft_angle` builds a ruled loft to the offset profile. `thin` extrudes a wall band inside every loop. |
| Revolve | Axis: `x`/`y`/`z`, a line of the same sketch (`e3`), or a straight edge by name. `both_sides` splits the angle. |
| Loft | Ruled through two or more single-outline sections. Sections are wound the same way about the loft direction, matched in edge count by splitting the longest edges, and rotated to the nearest start vertex so the loft does not twist. |
| Sweep | Profile along a line-chain path, with rotation-minimizing frames and overlapping segments. |
| Hole | Drills into the material on whichever side of the sketch plane it lies. Counterbore, true conical countersink (default 90 degrees), and `tap_class` as a cosmetic thread (ISO coarse pitch by default; the bore is cut at the stated diameter and a mismatch with the tap drill is reported). |
| Fillet, Chamfer | On named edges. Chamfer takes `distance`, plus `distance2` or `angle` for an asymmetric one. |
| Shell | Opens the named caps of the body's extrusion; no open faces makes a closed cavity (an inward boundary shell of the same solid, no boolean needed). |
| Mirror, Pattern | Replicate a feature's own geometry (the prism an extrude built, the cutter a hole drilled with), or every body when `features` is empty. A copy combines the way its source did unless `combine` overrides it. `count` includes the seed. Linear direction may come from a straight edge (`direction_ref`). |
| Boolean | `target` alone is the tool applied to every other body; `target` with `tools` applies the tools to the target. `keep_tools` keeps them. |
| Split | Every body the plane crosses becomes two bodies. |
| Reference plane | Offset, three-point, tangent to a planar face, normal to a straight edge. |

### Blends

Two exact paths, chosen per body:

1. **Profile blends.** When every listed edge is one an extrude swept from a corner of its sketch profile, the corner is rounded (or cut) in 2D, the prism is rebuilt from the new profile, and every boolean applied to the body since is replayed. The blend surface is built by the same sweep that built the walls, so no boolean touches it. This covers the vertical edges of plates, brackets and enclosures.
2. **Cutters.** Any other straight, convex edge between two planar faces, with convex ends, gets a cutter shaped exactly like the removed material, extended past the part on every side, and subtracted. Chamfer cutters meet the part transversally. Fillet cutters are tangent to both faces, where truck's booleans are least reliable; an edge that fails is reported by name.

Concave edges (blends that add material) and edges ending at an inside corner are refused with that reason. Positional references of the form `Extrude1/edge-0` never identified an edge; they are applied as a visual-only crease rounding and reported as such.

### Coplanar operands

truck-shapeops degenerates when operands share a plane. A blind join whose base sits on material is buried a quarter of its length into it; a blind cut that starts on a face starts 5% of its length out in the air, and one that ends exactly at the far face is carried through it. Material is probed under the profile itself. The extra length merges into the body or falls in empty space, so the solid is the same. Hole cutters and through-all cuts always overcut.

## Sketcher

Entities keep their own endpoints; endpoints drawn coincident are welded for the solve.

- **Degrees of freedom** are free parameters minus the rank of the constraint Jacobian (central differences, rank by full-pivot elimination). `redundant` counts rows that restate others.
- **Status**: `Failed` (no convergence: the constraints conflict), `UnderConstrained` (freedoms left), `OverConstrained` (converged with redundant rows), `FullyConstrained`.
- **Every constraint and dimension is checked** against the entity kinds it names before solving; an unsupported pair fails with its position and reason.
- **Point selectors** `p1` / `p2` (`start`, `end`, `center`) say which point a point constraint or distance means. Without them, a point is itself, a circle its centre, and `coincident` takes the nearest pair of endpoints as drawn.

| Constraint | Applies to |
|---|---|
| coincident | two points (any entity via selectors) |
| concentric | circles, arcs, points |
| collinear, parallel, perpendicular, equal_length | two lines |
| equal_radius | circles and arcs |
| tangent | line with circle or arc; two circles or arcs (inside or outside, as drawn) |
| horizontal, vertical | a line; or two points (`e2`) |
| symmetric | a line about an axis line (`e1`, `e2`), or two points about an axis (`e1`, `e2`, `e3`) |
| midpoint, point_on_line | a point and a line |
| point_on_circle | a point and a circle or arc |
| fix | any entity |

| Dimension | Meaning |
|---|---|
| linear, `e1` only | a line's length; a rectangle's width (`axis = "x"`, default) or height (`axis = "y"`) |
| linear with `e2` | point to point, point to line, or between parallel lines; `axis` measures one component |
| radial, diameter | circles and arcs |
| angular | two lines; 0 to 180 degrees unsigned, otherwise counter-clockwise from `e1` |

## Output

- **Mesh.** Tessellated per face, so every triangle records its face (`EvalMesh.face_ids` indexes `face_names` / `face_bodies`). This is what lets the Studio pick and highlight a face or edge by the name a feature uses.
- **STEP.** `step_string` / `write_step`, AP214, one solid per body, millimetres as the file declares (geometry is scaled by 1000). Edges truck represents as intersection curves are written as their B-spline leader, because truck-stepio 0.3 writes `INTERSECTION_CURVE` with a duplicated entity id. `cad_export_step` refuses to write a file whose entity ids repeat.
- **Colliders.** The engine gives a single convex body its exact hull and anything else (including a multi-body part) a convex decomposition; approximations are labelled in the part status.

## Limits

- Shell and thin features need profiles of straight segments or a full circle, and shell opens only the caps of a body that is still a single-profile extrusion plus booleans. Other bodies fall back to a scaled cavity that is exact only for boxes, reported as degraded.
- Draft is one-sided and needs the same kind of profile.
- Loft is ruled; guide curves are refused.
- Sweep joins straight segments by overlap; corners are not mitred.
- Fillets and chamfers that add material, and cutter-path fillets where truck's boolean fails, are not produced.
- No STEP import yet.

## Roadmap

1. **Studio sketcher and history.** Editable sketch canvas on the solver above (drag-solve, inline dimensions, constraint glyphs), a timeline with rollback, face and edge picking through `face_ids`, and in-canvas manipulators for extrude distance and fillet radius.
2. **Assemblies over MCP.** Mates exist in `engine/src/cad_assembly.rs`; expose them as tools and check interference with the engine's colliders.
3. **STEP import** through truck-stepio's `in` module.
4. **Drawings.** Orthographic projection of bodies with dimensions, to PDF and DXF.
5. **A blend kernel.** Rolling-ball fillets built as faces rather than cutters, which removes the tangent-boolean limit and allows concave blends and vertex blends.
