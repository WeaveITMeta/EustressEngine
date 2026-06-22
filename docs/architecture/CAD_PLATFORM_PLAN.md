# CAD Platform Plan — Canonical Workflow

**Status:** ACTIVE TRACK (2026-06-10). This document supersedes the phase *ordering* in
`docs/development/TOOLSET_CAD.md`; that document remains the per-feature spec reference.

**Goal:** One platform rivaling Revit / Fusion / Infraworks / AutoCAD / ForgeCAD plus
Roblox Procedural Models — parametric BRep CAD, attribute-driven procedural generation,
direct mesh editing, assemblies that physically articulate, and analysis — all
AI-native over MCP.

---

## Locked decisions (do not re-litigate)

| Decision | Choice | Rationale |
|---|---|---|
| BRep kernel | **truck** (adopted 2026-04-22, `eustress-cad` crate) | Pure Rust, no FFI; 8 evaluators already working |
| Constraint solver | **In-house pure Rust** — Levenberg-Marquardt on constraint residuals (nalgebra) | Zero FFI; covers the 12 typed constraint kinds; `ezpz` (KittyCAD) is a *reference*, not a dependency |
| Diffusion / generative ML for geometry | **No** | Constraint solving is deterministic optimization |
| SDF third backend | **Deferred** | Mesh CSG + BRep must integrate first; revisit for terrain/organic |
| `manifold` C++ bindings | **No** | FFI / Windows build pain; truck-shapeops + mesh CSG cover booleans |
| Physics | **Avian** (never Rapier) | Assemblies → Avian joints for kinematics/motion/interference |
| FEA | **Extends the realism crate** | `realism/materials/stress_strain.rs` (StressTensor) + `realism/deformation/` (Hooke's-law strain → vertex displacement) already exist; upgrade per-part uniform strain → element-based field solve |
| Surfaces ship **in lockstep** | Every phase delivers a human (Slint/Studio) surface AND an agent (MCP) surface together | User decision 2026-06-10; accepts the schedule cost of the sketch UI |
| Parameter system | Reuse `Quantity` + feature-tree variables + the GetAttribute/SetAttribute ECS attribute seam | No parallel `Params` struct zoo |
| Feature-tree storage | TOML document in the WorldDb **tree partition** (per the binary pivot / hybrid-store decision) | Rich docs live as TOML in `tree`; rkyv cores in `entities` |

## Ground truth at plan time (verified 2026-06-10)

- `eustress-cad`: Extrude, Revolve, Mirror, Pattern (linear/circular), Boolean, Split,
  Hole **working**. Fillet/Chamfer blocked on truck-shapeops upstream; Shell needs an
  offset-surface op; Sweep/Loft need the path/profile resolver.
- `tessellate()` in `cad/src/eval.rs` is a **stub returning an empty mesh** — blocks
  everything downstream (viewport, STL, STEP, GLB).
- The engine crate does **not depend on `eustress-cad`** at all. No plugin, no UI.
- Sketch constraints are **types only** — nothing solves them.
- GLB **export** does not exist anywhere in the workspace
  (`pointcloud/formats.rs::export_gltf` returns `Err`).
- No CAD tools on the MCP surface.
- `eustress-mesh-edit`: extrude_face + inset_face working; bevel_edge + loop_cut
  blocked on the edge-loop walker; no engine ModalTool wired.
- Truck workspace pins are **mixed versions** (base/geometry 0.5, topology/modeling 0.6,
  meshalgo 0.4, shapeops 0.4, stepio 0.3) — alignment audit is a Phase A item.
- Realism: `StressTensor` + principal-stress → Hooke's-law strain → uniform-field vertex
  deformation exists; comment in `deformation/systems.rs` marks field interpolation as
  the intended upgrade.

---

## Phase A — Close the loop (vertical slice)

Everything else is blocked on this. Exit test: **author a feature tree, see the solid in
the viewport, change a dimension in Properties, watch it regenerate, export a valid
`.glb`.**

1. **Real `tessellate()`** — truck `Solid` → `PolygonMesh` (truck-meshalgo) → Bevy
   `Mesh`. Includes the truck version-alignment audit.
   **✅ DONE 2026-06-11.** `tessellate_solid()` in `cad/src/eval.rs`: triangulation +
   robust-retry + attribute weld + normal fill + per-corner index expansion (positions/
   normals/uvs/indices, crease-preserving). Per-tree `metadata.mesh_tolerance` override.
   Version audit: the mixed pins (base/geometry 0.5, topology/modeling/polymesh 0.6,
   meshalgo/shapeops 0.4, stepio 0.3) are truck's coherent latest family — Cargo.lock
   resolves one set, no action needed.
   **Bonus discoveries + fixes (same increment):**
   - `boolean_not` was a hardcoded `None` → **Subtract, Hole, and Split never worked**
     despite being recorded as working. Now implemented as `A ∩ ¬B` via
     `Solid::not()` (truck's own punched-cube pattern).
   - truck-shapeops 0.4 has an **absolute scale floor**: identical geometry booleans
     fine at unit scale, fails/panics at centimeter scale. Meter-native parts sit under
     it. Fix: `boolean_normalized` — geometric-mean rescale to ~unit + dense tolerance
     ladder + `catch_unwind` per rung (truck panics instead of returning None on
     Newton divergence). Invert must happen *after* the rescale.
   - Boolean **results** stay surface-eval-poisoned at meter scale → `tessellate_solid`
     re-normalizes internally and emits positions via plain f32 scaling.
   - Hole cuts were **flush at the sketch plane** (coplanar → degenerate boolean):
     now over-extended, with through-hole detection extending the far end too.
   - `tests/shapeops_probe.rs` pins all of this, including a **canary** that fails
     when upstream fixes the scale floor (signal to retire the normalization).
   - String API (`parse_tree`/`tree_to_toml`) added for the WorldDb tree-partition path.
   Verify: `cargo test -p eustress-cad` (13 tests green).
2. **`CadPlugin` + `CadPart` class** — engine depends on `eustress-cad`; feature tree
   stored as a TOML doc in the tree partition; evaluated on load and on change;
   tessellated mesh bound to the entity; edit-time hot regen. Route creation through
   `eustress_common::instance_create`.
3. **GLB exporter** — tessellated mesh + materials → `gltf` crate writer. Param schema +
   semantic tags embedded in glTF `extras` (self-documenting for agents). This is the
   engine-wide `.glb` exporter, not CAD-only.
4. **Human surface:** Insert → CadPart; dimensions/variables editable in the Properties
   panel via the attribute seam. **Agent surface:** `cad_create_part`,
   `cad_set_variable`, `cad_export_glb` MCP tools on the existing bridge.

## Phase B — ProceduralModel (Roblox parity, then past it)

No constraint solver required — Roblox Procedural Models don't have one. Attribute-driven
regeneration needs only typed params + a generator + the regen loop, and the attribute
ECS seam just landed.

1. **`ProceduralModel` class** — typed attributes (value/default/min/max/unit/
   description), a `Generator` reference, non-destructive child regeneration on
   attribute or Size change.
2. **Generators, two flavors:**
   - **Rust trait impls** — built-in parametric library: gear, stair, railing, truss,
     pipe-run (the Revit/Infraworks-flavored wins).
   - **Luau/Rune scripts** via the soul runtime — `OnGenerate(params)`, exact Roblox
     mental model so imported creators feel at home.
3. **Human surface:** attribute sliders in Properties; generator picker; regen-on-edit.
   **Agent surface:** `cad_create_procedural`, `cad_set_param`, `cad_get_schema` (JSON
   schema per generator for reliable LLM tool calling). Agents drive the existing
   build→look→judge loop (AI camera) against procedural models.

## Phase C — Sketch solver + sketch-on-face (the Fusion-grade unlock)

1. **2D constraint solver** — in-house Levenberg-Marquardt over residuals of the 12
   `ConstraintKind`s; DOF analysis with under/over-constrained reporting (this is what
   makes AI-generated sketches debuggable).
2. **Face references** — resolve `"Extrude1/face-0"` → truck face; sketch-on-face.
   Prerequisite for Sweep's path resolver.
3. **Human surface (the big lift, accepted):** Slint sketch canvas — draw entities,
   apply constraints, drag-solve live; feature-tree history panel (reuse History panel
   patterns: reorder / suppress / delete). **Agent surface:** `cad_add_sketch`,
   `cad_add_constraint`, `cad_solve` (returns DOF + residuals).

## Phase D — Depth + analysis (parallel tracks once A–C stand)

- **Features:** Sweep + Loft (path resolver from C unblocks both); Shell (in-house
  offset surface); Fillet/Chamfer — watch truck-shapeops upstream, OCCT FFI is the
  explicitly-acknowledged fallback if it stalls past this phase.
- **Assemblies:** mates/joints mapped onto **Avian joints** — kinematics, motion study,
  interference via collision. A constrained assembly that physically articulates live
  in-engine is something Fusion cannot do.
- **Mesh-edit convergence:** edge-loop walker (unblocks bevel + loop_cut), then the
  `mesh_edit_tool` ModalTool + Slint select-mode switcher (Object/Vertex/Edge/Face).
  Direct-modeling half stays deliberately separate from the parametric half.
- **Outputs:** STEP export (truck-stepio glue), BOM from the assembly graph, technical
  drawings later.
- **FEA (realism extension):** upgrade `realism/deformation` from per-part uniform
  strain to an element-based stress field solved on the CAD tessellation (tet/beam
  elements); visualize via the existing deformation → vertex-displacement path. Avian
  supplies loads/boundary conditions from the live sim.

---

## Explicitly dropped from the Grok 4.3 proposal

- vcad / Fornjot kernel adoption (truck is decided and working)
- Diffusion-model geometry
- `manifold` C++ bindings (FFI)
- SDF as a launch backend (deferred, not rejected)
- Standalone `Params`/`Constraint` struct system (attribute seam + Quantity cover it)
- Its phase ordering (constraints-first) — integration is the bottleneck, and
  procedural generation does not need the solver

**Kept from it:** attribute-driven non-destructive generators, MCP-native tool surface,
glTF `extras` metadata, semantic tags, headless batch generation for training data.
