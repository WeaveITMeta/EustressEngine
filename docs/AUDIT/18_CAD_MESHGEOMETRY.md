# 18 — CAD & Mesh Geometry

> Parametric BRep CAD kernel (via `truck`), half-edge mesh editing, feature trees,
> sketch-extrude-revolve workflow, mesh validation + repair, STEP / IGES import / export.
> The **geometry kernels** that produce and edit 3D content.

## Pass changelog

- **P3 (2026-05-14):** New doc; 11 features.

---

## Concept summary

**Geometry Kernels** are two complementary subsystems sharing one audit doc:

1. **CAD** ([eustress-cad](../../eustress/crates/cad/)) — parametric Boundary Representation (BRep) via the [truck](https://crates.io/crates/truck) Rust kernel. Vertices, edges, faces, shells, solids. Operations are *parametric*: a 10×10×5 box can be edited to 12×12×5 by changing the sketch parameter; downstream features recompute. The feature tree records ordered operations.
2. **Mesh-Edit** ([eustress-mesh-edit](../../eustress/crates/mesh-edit/)) — half-edge mesh data structure for arbitrary triangle meshes. Operations: extrude face, inset face, bevel edge, loop cut, subdivide. Targets meshes that aren't parametric (imported GLBs, sculpts, terrain extractions).

Both feed into the same write-back path: the result becomes a Part / Model TOML reference with an optional `.glb` mesh asset. They are foundational layers shipping incrementally; CAD has working `Extrude`; mesh-edit has `extrude_face` + `inset_face`. The rest (revolve, fillet, chamfer, sweep, loft, bevel, loop-cut, STEP) are scaffolded.

The Studio's ribbon UI is the missing surface for both: neither has visible toolbar entries today. Discoverability is a P3 blocker.

---

## Implementation snapshot

**Crates:**
- [eustress-cad](../../eustress/crates/cad/) — truck-based BRep; `Quantity` unit system; `FeatureTree` TOML; **Extrude** working; Revolve / Fillet / Chamfer stubbed
- [eustress-mesh-edit](../../eustress/crates/mesh-edit/) — half-edge kernel; `extrude_face` + `inset_face` working; bevel + loop-cut WIP

**Docs:**
- [docs/development/TOOLSET_CAD.md](../development/TOOLSET_CAD.md)
- Cargo.toml comments: "Unblocks TOOLSET.md Phase 1/2"
- [docs/development/TOOLSET.md](../development/TOOLSET.md)

**Working:**
- Extrude (CAD)
- Quantity unit system in CAD
- FeatureTree serialization to TOML
- Half-edge kernel (mesh-edit)
- Extrude face + inset face (mesh-edit)
- VIGA `ImageToGeometryTool` registered (Workshop AI)

**Stubbed / missing:**
- Revolve, Fillet, Chamfer, Sweep, Loft (CAD)
- Bevel, Loop cut, Subdivide (mesh-edit) — bevel blocked on walker
- STEP / IGES import / export
- Mesh-to-BRep conversion
- Validation + repair (non-manifold detection, hole closure)
- 2D sketch + constraint solver
- Studio ribbon entries
- Undo integration (separate stack vs. main?)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | CAD: Extrude | ✅ |
| 2 | CAD: Revolve / Sweep / Loft | 🟠 |
| 3 | CAD: Fillet / Chamfer | 🟠 |
| 4 | CAD: 2D sketch + constraint solver | 🔴 |
| 5 | CAD: Boolean (union / subtract / intersect) | 🟠 |
| 6 | Mesh-edit: Extrude / Inset face | ✅ |
| 7 | Mesh-edit: Bevel edge | 🟡 WIP |
| 8 | Mesh-edit: Loop cut + Subdivide | 🟡 WIP |
| 9 | Mesh validation + repair | 🔴 |
| 10 | STEP / IGES import / export | 🔴 |
| 11 | Studio ribbon UI for both kernels | 🔴 |

---

## Detailed per-feature cards (top 6)

### Feature 1 — CAD: Extrude

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [02_STUDIO], [04_ASSETS], [18]
**Sub-features:** 2D profile → solid · per-feature `Quantity` (with units) · FeatureTree TOML round-trip · re-compute on parameter change

**Concept.** User defines a 2D profile (today: simple rectangle / circle); Extrude pushes it normal-to-plane by `Distance` (Quantity). The Quantity carries unit (mm / cm / m / ft / in / studs) per [C1 units cross-cut]. FeatureTree stores the parametric history.

**Forecasted feedback (R)**
- R1.1 Profile authoring UI doesn't exist; today's profiles are code-defined.
- R1.2 Re-compute on edit triggers full FeatureTree replay; can be slow for complex trees.
- R1.3 Multi-profile extrude (composite shape) is a separate feature.
- R1.4 Extrude direction options (one-side / two-side / symmetric) missing.

**Implications (I)**
- *Architectural:* the FeatureTree IS the parametric history; serialise it well.
- *Cross-system:* [04_ASSETS] needs a `.cad.toml` schema in addition to `.glb`.
- *Operational:* large trees → incremental re-compute, not full replay.

**Risks (X)** — X1.1 Unit confusion (mm vs. m) destroys geometry silently.

**Mitigations (M)** — M1.1 Per-feature unit display in Studio; warning on cross-unit composition.

---

### Feature 4 — CAD: 2D sketch + constraint solver

**State:** 🔴 · **Effort:** XL · **Risk:** Med · **Touches:** [02_STUDIO], [18]
**Sub-features:** sketch plane · 2D primitives (line, arc, circle, polygon) · constraint types (distance, angle, parallel, perpendicular, tangent, coincident) · LM-style solver · over/under-constrained detection · dimension annotations

**Concept.** Standard parametric CAD workflow: pick a face → "Create sketch" → draw 2D shape with dimensional + geometric constraints → close sketch → use as input to Extrude / Revolve / etc. Solver handles real-time constraint satisfaction.

**Forecasted feedback (R)**
- R4.1 No solver exists today; need to build or wrap (Planegcs, FreeCAD's Sketcher, etc.).
- R4.2 UX for constraint editing is non-trivial; ribbon + inspector + viewport-overlay.
- R4.3 Multi-curve segments (NURBS) blow up complexity.
- R4.4 Defer Bézier / spline support to P5.

**Implications (I)**
- *Architectural:* the sketch is the input to every parametric feature; load-bearing.
- *Strategic:* missing 2D sketch = "not real CAD" in customer eyes.

**Risks (X)** — X4.1 Solver bugs produce non-physical sketches that crash downstream features.

**Mitigations (M)** — M4.1 Solver returns "could not satisfy" → highlight conflicting constraints in red.

---

### Feature 5 — CAD: Boolean (union / subtract / intersect)

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [18]
**Sub-features:** union (A ∪ B) · subtract (A − B) · intersect (A ∩ B) · co-planar face handling · non-manifold output detection

**Concept.** Two solids combine. Truck has primitives; expose through ribbon. Critical for CSG modelling workflows.

**Forecasted feedback (R)**
- R5.1 Co-planar face cases produce degenerate output; needs healing.
- R5.2 Non-manifold detection mandatory before export to GLB.
- R5.3 Boolean on imperfect (truncation-error) BReps cascades; epsilon-tuning needed.

**Implications (I)** — *Cross-system:* Boolean output feeds mesh-edit for fine touch-up.

**Risks (X)** — X5.1 Boolean produces 0-volume artefacts; downstream rendering breaks.

**Mitigations (M)** — M5.1 Heal-after-boolean pass; merge co-planar.

---

### Feature 7 — Mesh-edit: Bevel edge

**State:** 🟡 WIP · **Effort:** M · **Risk:** Med · **Touches:** [18]
**Sub-features:** edge selection · bevel width + segments · vertex / edge walker for ring-of-faces traversal · cap face generation · UV preservation

**Concept.** Standard bevel — replace a sharp edge with N segments of fillet-like geometry. Memory note: blocked on walker (half-edge traversal for edge-ring).

**Forecasted feedback (R)**
- R7.1 Walker is the bottleneck; once it ships, bevel + loop cut follow.
- R7.2 UV preservation across bevel is non-trivial; new UVs needed for new faces.
- R7.3 Bevel on convex vs. concave corners produces different topology.

**Implications (I)** — *Cross-system:* bevel + loop cut unlock organic-feel modeling; massive UX win.

**Risks (X)** — X7.1 Non-manifold output on edge cases.

**Mitigations (M)** — M7.1 Pre-bevel manifold check.

---

### Feature 9 — Mesh validation + repair

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [04_ASSETS], [18]
**Sub-features:** non-manifold detection · hole detection + closure · self-intersection · degenerate face removal · normal coherence check · automatic + manual repair

**Concept.** Imported meshes (especially AI-generated, e.g. TripoSR from [07_AI_PLATFORM]) often have non-manifold geometry, holes, or self-intersection. Validation reports issues; repair fixes them (or marks as un-fixable).

**Forecasted feedback (R)**
- R9.1 Without this, AI-generated meshes break physics / rendering.
- R9.2 Repair is heuristic; not always correct; user controls strength.
- R9.3 Validation report shape → which Slint panel?
- R9.4 Mesh statistics (vert count, face count, manifold-yes/no) needed for inspector.

**Implications (I)** — *Cross-system:* AI generation (FLUX / TripoSR) needs this on input side.

**Risks (X)** — X9.1 Over-aggressive repair destroys intended geometry.

**Mitigations (M)** — M9.1 Per-issue confirm dialog; user reviews before apply.

---

### Feature 11 — Studio ribbon UI

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [02_STUDIO], [18]
**Sub-features:** ribbon entries for Build tab (CSG: Union / Subtract / Intersect; Sketch; Extrude; Revolve; …) · ribbon entries for Edit tab (mesh: Bevel / Loop cut / Inset / Subdivide) · preview-during-drag · ESC cancels · history integration

**Concept.** Both kernels exist as code; without ribbon buttons, they're invisible. P3 priority: at minimum, ship Extrude in the ribbon (CAD) + Inset face (mesh-edit) so creators can discover the kernel.

**Forecasted feedback (R)**
- R11.1 Discoverability — without ribbon entries, users don't know features exist.
- R11.2 Preview-during-drag (Maya-style "fast-feedback") is the UX expectation.
- R11.3 ESC cancels mid-operation.
- R11.4 Undo integration: separate stack or main `UndoStack`?

**Implications (I)** — *Strategic:* ribbon entries unlock the kernels; minimal effort, huge perceived-progress jump.

**Mitigations (M)** — M11.1 Ship Extrude + Inset in P4 ribbon; rest follow.

---

## Wiring / import gaps (top 8)

1. CAD ribbon: Extrude / Boolean / Sketch entries
2. Mesh-edit ribbon: Inset / Bevel / Loop cut entries
3. Half-edge walker (mesh-edit blocker)
4. 2D sketch + constraint solver
5. STEP / IGES import / export
6. Mesh validation + repair pipeline
7. Mesh-to-BRep conversion (remesh / voxelise)
8. CAD-undo integration with main `UndoStack`

---

## Cross-system dependencies

- **[02_STUDIO]** ribbon UI + viewport preview + property inspector.
- **[04_ASSET_PIPELINE]** `.cad.toml` (FeatureTree) + `.glb` (baked mesh) round-trip; mesh validation on import.
- **[07_AI_PLATFORM]** AI-generated mesh validation; VIGA `ImageToGeometryTool`.
- **[11_SIMULATION]** geometry collision (mesh colliders); BRep → triangulation for physics.
- **[13_TERRAIN]** terrain-to-Part conversion uses mesh-edit operations.
- **[19_REALISM]** materials science assumes intact (validated) meshes.

---

## Open questions

- Q18.1 CAD undo: separate stack or unified `UndoStack`?
- Q18.2 BRep storage: `.cad.toml` (parametric) or `.brep` binary or both?
- Q18.3 STEP / IGES — own implementation or wrap `freecad` Python via Forge?
- Q18.4 Subdivision surface (SubD) modelling — P4 or later?
- Q18.5 Real-time co-edit of CAD trees in Multiplayer Studio?
- Q18.6 NURBS — out of scope or P5?
