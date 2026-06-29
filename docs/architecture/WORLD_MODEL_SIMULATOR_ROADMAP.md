# Eustress World-Model Simulator — Master Roadmap

> Canonical, exhaustive engineering roadmap. Composed from verified ground-truth sweeps of the codebase (June 2026, `feat/bevy-0.19`). Research-grade items are labeled honestly; nothing here overclaims.

---

## 1. Thesis & how to read this

**Eustress IS the Simulator.** World models (per World Labs' *"The World Is Not Made of Words"*) project the POMDP loop — agent → action → state → observation — into three functions:

| Function | Output | Truth claim |
|---|---|---|
| **RENDERER** | pixels / observations | visually plausible, **not** physically true |
| **SIMULATOR** | STATE = geometry + physics + dynamics | true, computable; serves humans **and** programs/agents — **the linchpin** |
| **PLANNER** | actions | derives from state |

Eustress is the **Simulator**: it outputs true computable state, can also **Render** (via Gaussian Splatting, `radiance`), and is closing the loop toward **Planner** through its agent surface (`engine_bridge` + MCP + dual scripting).

**Strategy — ingest-and-surpass.** Ingest World Labs World API / Marble output and *re-derive* true simulatable state. Generation models are commodity inputs; the real-physics simulator + agent loop is the moat. A real simulator **manufactures the scarce 3D/physics ground-truth the field lacks** — a synthetic-data flywheel.

**The architecture-generation loop replaces RL.** The flagship subsystem (§2) turns the Simulator into a generative AEC **Planner with no policy gradient**: simulation-in-the-loop search over STRUCTURE × MATERIAL × FIXTURES/BONDS, where the sim itself supplies fitness (stands? efficient? code-compliant? on-style?).

**Reading conventions:**
- **Luau implies Rune throughout.** `common/src/scripting` is the shared Rune+Luau host. Every behavior/agent subtask targets ONE shared host API so `execute_luau` and `execute_rune` stay in lockstep. When a subtask says "scripting," it means both runtimes via one binding layer.
- **Tag format** on every subtask line: `[crate/system]` `[dep: …]` `[effort: S/M/L/XL]` `[status: exists | extend | new | research]`. Cross-refs link a subtask to the contention it clears or the loop stage it serves.
- **Crates root is `eustress/crates`** (NOT repo-root `crates`).
- **Status legend:** `exists` = works today · `extend` = build on a real asset · `new` = net-new but buildable now · `research` = research-grade, honestly flagged, sequenced late.

---

## 2. The physics-driven generative architecture-design loop (replaces RL)

> The flagship subsystem. No RL, no policy gradient. The simulator supplies the fitness.

### 2.1 Condensed (for planners)

- **The loop.** generate candidate (geometry + material + joints) → evaluate in-sim (Avian dynamics + realism closed-form + FEA) → score (stability, efficiency, code, style) → select/mutate/optimize → persist to WorldDb + log to data/Polars → embed into style latent (embedvec) → condition next generation.
- **Optimizer families, each with a job:**
  - **Topology optimization** (SIMP / level-set) — *where* material goes in a fixed design domain; continuous density field, gradient-driven.
  - **Form-finding** (force-density / dynamic relaxation / TNA) — the *equilibrium shape* (funicular/shell/tensile); shape is an output of the load path.
  - **Evolutionary / CMA-ES / NSGA-II** — *discrete* choices (topology family, member counts, joint TYPE, material class) + multi-objective Pareto. The default early driver.
  - **Gradient via differentiable sim** — *continuous* sizing/material params when the eval is differentiable. Research-grade (needs adjoint/autodiff).
  - **Constraint solver** (`realism::symbolic`) — not an optimizer; the *feasibility gate* projecting infeasible candidates back onto the feasible set.
- **Style is a latent + constraints.** *Invent* = sample/interpolate the embedvec latent under physics-feasibility, score novelty vs corpus. *Mimic* = condition on exemplar embeddings (k-NN region) + add style-match term. `spatial-llm` names styles and turns NL briefs into constraints.
- **Synthetic-data flywheel.** Every candidate — feasible OR failed — is a (design → physical-performance) row in Polars/Arrow. That corpus trains the generator + learned style/surrogate models → next batch is better.
- **Surrogates close the speed gap.** A learned cheap predictor pre-screens candidates so full Avian+FEA runs only on promising ones.
- **Honest gaps (research-grade):** (1) no FEA mesh solver — only closed-form per-member `structures/` law; (2) no differentiable structural sim — gradient path needs adjoint/autodiff; (3) no trained style/surrogate model — embedvec stores vectors but the AEC corpus + encoder are unbuilt; (4) no topology-optimization engine; (5) code-compliance rule pack unbuilt.
- **Drive surface exists.** MCP exposes `create_entity`, `execute_luau`/`execute_rune`, `run_simulation`, `run_experiment`, `raycast`, `inspect_scene`, `query_entities`, `capture_viewport` — the loop is orchestration over these plus new structural/optimizer verbs.
- **Contention to clear first:** engine_bridge TCP not accepting connections post-0.19 (blocks the whole agent loop); duplicate StudioState; dual TOML/WorldDb authority (design records must be WorldDb-authoritative); monolithic engine build time.
- **Build order:** wire the loop on closed-form fitness first (works today) → add surrogate + evolutionary search → add FEA → add differentiable sim + trained style model last.

### 2.2 Stage → crate map

| Stage | Crate / system (verified) |
|---|---|
| Geometry / topology gen (B-rep, CSG, extrude) | `cad` (truck: `feature.rs`, `sketch.rs`, `feature_tree.rs`, `eval.rs`) |
| Mesh topology ops (extrude/inset; bevel/loop-cut pending) | `mesh-edit` (`half_edge.rs`, `ops.rs`) |
| Structural eval — closed form | `common/realism/structures` (beams/columns/fatigue/composites) + `materials` (stress_strain/deformation/fracture/properties) |
| Structural eval — FEA | **NEW** FEA module extending `realism` (gap) |
| Dynamics / stability ("does it stand") | Avian 0.7 (rigid + xpbd_joints + parry/convex-decomp) |
| Fixtures/bonds (joints, welds, fasteners) | Avian joints + **NEW** connection/bond model on `scripting` instance API |
| Constraint enforcement (code/clearance/geometry) | `common/realism/symbolic` (`solver.rs`, `nonlinear.rs`, `resolver.rs`, `codegen.rs`) |
| Performance logging / corpus | `data` (Polars/Arrow, default-on) + `data-store` |
| Style latent space | `embedvec` (`spatial.rs`, `knowledge.rs`, `ontology.rs`, `rocksdb_store.rs`) |
| Style description / NL brief → constraints | `spatial-llm` (`generation.rs`, `prompt.rs`, `query.rs`, `context.rs`) |
| Agent / behavior surface | shared `common/scripting` (Rune+Luau host) + `mcp`/`mcp-server` |
| Persistence (authoritative design records) | `worlddb` (Fjall; rkyv cores + TOML tree; `.eustress` dir) |
| Visualization / ingest references | `radiance` (Gaussian Splatting) + `ppisp` (differentiable ISP) |
| Canonical entity creation | `eustress_common::instance_create::create_instance` |

### 2.3 End-to-end loop

```
        ┌──────────────────────────────────────────────────────────────┐
        │  BRIEF (NL or exemplars)  →  spatial-llm → constraints+style   │
        └──────────────────────────────────────────────────────────────┘
                                   │
            ┌──────────────────────▼───────────────────────┐
   (1) GENERATE  candidate = { STRUCTURE (cad/mesh-edit geometry+topology),
                               MATERIAL  (realism::materials selection),
                               FIXTURES  (Avian joints + bond model) }
            └──────────────────────┬───────────────────────┘
                                   │  (surrogate pre-screen: cheap reject)
            ┌──────────────────────▼───────────────────────┐
   (2) EVALUATE IN-SIM
        • feasibility/constraints  → realism::symbolic solver
        • static structural        → realism::structures + materials  (closed form)
        • FEA stress/displacement  → NEW FEA module (gap)
        • dynamics "does it stand" → Avian (settle test, joint failure, collapse)
            └──────────────────────┬───────────────────────┘
                                   │
            ┌──────────────────────▼───────────────────────┐
   (3) SCORE  fitness = w1·stability + w2·efficiency(mass/cost) +
                        w3·code_compliance + w4·style_score(embedvec/spatial-llm)
            └──────────────────────┬───────────────────────┘
                                   │
            ┌──────────────────────▼───────────────────────┐
   (4) SELECT / MUTATE / OPTIMIZE   (optimizer dispatch — see families)
            └──────────────────────┬───────────────────────┘
                                   │
            ┌──────────────────────▼───────────────────────┐
   (5) PERSIST + LEARN
        • design record → WorldDb (authoritative)
        • (design → performance) row → data/Polars  ← FLYWHEEL
        • embed design → embedvec latent; retrain style/surrogate
        • visualize → radiance (Gaussian Splat)
            └──────────────────────────────────────────────┘
                                   │ feeds back to (1)
```

**Optimizer dispatch — when each family fires:**

- **Topology optimization (SIMP / level-set):** *where should material be* inside a fixed design domain under fixed loads/supports (slabs, brackets, shear walls, cores). Continuous density field, compliance minimization.
- **Form-finding (force-density / dynamic relaxation / TNA):** *equilibrium shape* — funicular arches, shells, gridshells, cable/tensile nets. Shape is an output of the load path.
- **Evolutionary / CMA-ES / NSGA-II:** *discrete* choices — topology family, member counts, joint TYPE (weld vs bolt vs pin), material class — and Pareto fronts (cost vs mass vs style). Default driver early on.
- **Gradient via differentiable sim:** differentiable eval, *continuous* variables (cross-sections, material params, prestress). Fast local refinement once topology is fixed. Research-grade.
- **Constraint solver (`realism::symbolic`):** the *feasibility gate* — code clearances, geometric closure, span limits as hard constraints; projects infeasible candidates back before scoring.

**Typical pipeline:** constraint-solve (feasible domain) → form-find or topology-opt (coarse structure) → evolutionary (discrete topology + joint/material, Pareto) → gradient (sizing refinement) → final FEA+Avian verification.

### 2.4 Subtasks — Ways 1–7 (the loop)

#### Way A1 — The generate→evaluate→score→optimize loop (orchestrator)

**Plan**
- Define the `Candidate` schema {structure_ref, material_assignments, joint/bond graph, provenance, scores} as the single design record. `[worlddb, common::instance_create]` `[dep: WorldDb authority resolved]` `[effort: S]` `[status: new]` — Cross-ref: design records MUST be WorldDb-authoritative to avoid tree-staleness.
- Specify the fitness contract: weighted sum + hard-constraint mask, pluggable per-objective scorers returning normalized [0,1]. `[data, realism]` `[dep: none]` `[effort: S]` `[status: new]`
- Decide optimizer-dispatch policy (which family fires given problem/variable types). `[realism::symbolic, data]` `[dep: none]` `[effort: S]` `[status: new]`
- Design the surrogate pre-screen interface. `[data, embedvec]` `[dep: flywheel schema]` `[effort: M]` `[status: new]`

**Make**
- Build `GenerativeArchPlugin` owning the loop state machine (Generate/Evaluate/Score/Select as ECS systems). `[engine, common]` `[dep: candidate schema]` `[effort: M]` `[status: new]`
- Implement evolutionary driver (population, mutation/crossover over candidate graph, NSGA-II Pareto). `[common, data]` `[dep: candidate schema, fitness]` `[effort: M]` `[status: new]`
- Implement constraint-solve gate wrapping `realism::symbolic` for feasibility projection. `[realism::symbolic]` `[dep: none]` `[effort: M]` `[status: extend]`
- Wire MCP verbs `generate_design`, `score_design`, `step_optimizer`, `get_pareto_front`. `[mcp, mcp-server]` `[dep: plugin]` `[effort: M]` `[status: extend]` — **GATED**: engine_bridge TCP accept post-0.19.
- Expose the same verbs through the shared behavior API (one impl in `scripting`, bind both). `[common::scripting]` `[dep: MCP verbs]` `[effort: M]` `[status: extend]`

#### Way A2 — Candidate generation (STRUCTURE + MATERIAL + FIXTURES)

**Plan**
- Define a parametric structure grammar (members, panels, design-domain voxels) → cad feature trees + mesh-edit half-edge meshes. `[cad, mesh-edit]` `[dep: none]` `[effort: M]` `[status: new]`
- Specify material-assignment model: per-member selection from `realism::materials` + cost/sustainability metadata. `[realism::materials]` `[dep: none]` `[effort: S]` `[status: extend]`
- Specify the bond/connection graph: joint type (weld/bolt/pin/fixed/cable), capacity, location → Avian xpbd_joints + load-transfer descriptor. `[common::physics, common::scripting]` `[dep: none]` `[effort: M]` `[status: new]` — joint TYPE is a discrete evolutionary variable.

**Make**
- Build geometry generator over cad (use Extrude for prism members; scaffold others). `[cad]` `[dep: grammar]` `[effort: M]` `[status: extend]`
- Build topology mutation ops on mesh-edit half-edge. `[mesh-edit]` `[dep: grammar]` `[effort: M]` `[status: extend]`
- Implement material assigner writing MaterialProperties components + DisplayUnit-correct quantities. `[realism::materials, common::units]` `[dep: material model]` `[effort: S]` `[status: extend]`
- Implement bond-graph instantiator (Avian joints + bond descriptor components via `create_instance`). `[common::physics, common::instance_create]` `[dep: bond model]` `[effort: M]` `[status: new]`
- Implement topology-optimization generator (SIMP density field over voxel domain → mesh-edit isosurface). `[mesh-edit, realism]` `[dep: load eval]` `[effort: L]` `[status: new]`
- Implement form-finding generator (dynamic relaxation / force-density → funicular/shell meshes). `[common, mesh-edit]` `[dep: none]` `[effort: M]` `[status: new]`

#### Way A3 — In-sim evaluation (Avian + realism + FEA)

**Plan**
- Define the structural eval contract: (geometry, materials, supports, loads, joints) → (max stress, displacement, utilization, failed members, mode). `[realism::structures]` `[dep: none]` `[effort: S]` `[status: extend]`
- Specify the "does it stand" dynamics test: spawn as Avian bodies+joints, apply gravity+loads, settle N steps, measure drift/joint break/collapse. `[Avian, engine]` `[dep: bond instantiator]` `[effort: S]` `[status: new]`
- Decide FEA scope/discretization (1D frame first; 2D shell / 3D solid later). `[realism (new FEA)]` `[dep: none]` `[effort: M]` `[status: research]` — **HONEST GAP**: no FEA today.

**Make**
- Implement closed-form static eval (`structures` + `materials`) — works TODAY, ship the loop on this first. `[realism::structures, realism::materials]` `[dep: eval contract]` `[effort: S]` `[status: exists]`
- Implement Avian settle-test evaluator (collapse/joint-failure, displacement). `[Avian, engine]` `[dep: dynamics spec]` `[effort: M]` `[status: new]`
- Build a 1D linear FEA solver (frame/truss stiffness assembly + sparse solve) as `realism::fea`. `[realism (new)]` `[dep: numerics]` `[effort: L]` `[status: new]` — reuse `realism::numerics`.
- Extend FEA to 2D shells / 3D solids. `[realism::fea]` `[dep: 1D FEA]` `[effort: XL]` `[status: research]`
- Log every eval result (incl. failures) to data/Polars. `[data]` `[dep: flywheel schema]` `[effort: S]` `[status: extend]`

#### Way A4 — Scoring: stability, efficiency, code-compliance, style

**Plan**
- Define efficiency metric (mass/cost/embodied-carbon per unit load capacity). `[realism::materials, data]` `[dep: eval]` `[effort: S]` `[status: new]`
- Specify a code-compliance rule pack (allowable stress, deflection, slenderness, spans) as symbolic constraints. `[realism::symbolic]` `[dep: none]` `[effort: M]` `[status: research]` — **HONEST GAP**: no rule pack today.
- Define style-score: cosine distance to target centroid + novelty (distance to nearest corpus neighbor). `[embedvec, spatial-llm]` `[dep: style latent]` `[effort: M]` `[status: new]`

**Make**
- Stability score from Avian settle-test + FEA factor-of-safety. `[Avian, realism::fea]` `[dep: eval]` `[effort: S]` `[status: new]`
- Efficiency + embodied-carbon scorer. `[realism::materials, data]` `[dep: metric]` `[effort: S]` `[status: new]`
- Code-compliance scorer over the symbolic rule pack. `[realism::symbolic]` `[dep: rule pack]` `[effort: M]` `[status: new]`
- Style scorer querying embedvec spatial store. `[embedvec]` `[dep: latent]` `[effort: M]` `[status: extend]`

#### Way A5 — Synthetic-data flywheel

**Plan**
- Design the (design → performance) Polars/Arrow schema: candidate_id, structure descriptor, material vector, joint vector, loads, FEA outputs, Avian outcome, sub-scores, optimizer provenance, style_embedding_ref, feasible flag. `[data, data-store]` `[dep: candidate schema]` `[effort: S]` `[status: new]`
- Decide retention + dataset partitioning (by style family / problem type). `[data, worlddb]` `[dep: schema]` `[effort: S]` `[status: new]`
- Specify surrogate target + generator-guidance signal. `[data, embedvec]` `[dep: corpus]` `[effort: M]` `[status: research]`

**Make**
- Implement the logging sink (every candidate, incl. failures) to a Polars Dataset; route through EustressStream topics + history tee. `[data, stream]` `[dep: schema]` `[effort: S]` `[status: extend]`
- Implement embedvec ingestion (embed each descriptor on persist). `[embedvec]` `[dep: encoder]` `[effort: M]` `[status: extend]`
- Train/refresh a surrogate from the corpus; plug as Way-A1 pre-screen. `[data]` `[dep: corpus volume]` `[effort: L]` `[status: research]`
- Implement WorldDb persistence of authoritative design records (rkyv core + TOML tree). `[worlddb]` `[dep: candidate schema]` `[effort: M]` `[status: extend]`

#### Way A6 — Style: invent new vs mimic existing

**Plan**
- Define the AEC style latent (geometry stats, topology motifs, proportion ratios, material palette, joint vocabulary). `[embedvec, spatial-llm]` `[dep: none]` `[effort: M]` `[status: research]` — **HONEST GAP**: no trained AEC style encoder.
- Specify MIMIC mode (condition on exemplar embeddings + style-match score). `[spatial-llm, embedvec]` `[dep: latent]` `[effort: M]` `[status: new]`
- Specify INVENT mode (sample/interpolate latent under feasibility, reward novelty). `[embedvec, realism::symbolic]` `[dep: latent, feasibility gate]` `[effort: M]` `[status: new]`
- Define NL-brief → constraints + style-target pipeline. `[spatial-llm]` `[dep: none]` `[effort: M]` `[status: extend]`

**Make**
- Build the style encoder (descriptor → latent vectors in embedvec). `[embedvec, spatial-llm]` `[dep: corpus]` `[effort: L]` `[status: research]`
- Implement spatial-llm style describer/namer (latent ↔ words ↔ constraints). `[spatial-llm]` `[dep: latent]` `[effort: M]` `[status: extend]`
- Implement exemplar-conditioning (ingest reference → embed → set generation target) via radiance. `[embedvec, radiance]` `[dep: latent]` `[effort: M]` `[status: new]`
- Implement novelty-reward sampler for INVENT mode. `[embedvec, common]` `[dep: latent]` `[effort: M]` `[status: new]`

#### Way A7 — Agent surface & visualization (drive + show)

**Plan**
- Design ONE shared behavior/host API for the loop (Rune + Luau call identical functions). `[common::scripting]` `[dep: Way-A1 MCP verbs]` `[effort: M]` `[status: extend]`
- Specify visualization contract (candidates + Pareto front as splats/meshes). `[radiance, engine]` `[dep: candidate]` `[effort: S]` `[status: extend]`
- Specify reference-ingest honesty path (PPISP-normalized exemplars before embedding). `[ppisp]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Bind loop calls into both runtimes from the shared host. `[common::scripting, common::luau]` `[dep: shared API]` `[effort: M]` `[status: extend]`
- Implement candidate/Pareto visualization + Properties polymorphic inspector for design records. `[engine, radiance]` `[dep: candidate schema]` `[effort: M]` `[status: extend]` — fix duplicate StudioState first.
- Implement run-experiment harness wiring (batch generations as experiments, compare_runs). `[mcp, engine]` `[dep: loop]` `[effort: M]` `[status: extend]`

---

## 3. The 50 ways → subtasks

> Ten theme groups, five `### Way N` blocks each. Plan/Make + tag format throughout. Verified-ground-truth deviations from the brief are folded in.

### GROUP 1 — State over pixels

#### Way 1 — Outputs true 3D state (geometry/position/velocity/mass/material), not pixels
**Plan**
- Define the canonical **WorldState DTO** (entity id + Morton pos, Transform, Linear/AngularVelocity, ColliderDensity→mass, Material handle, MeasureUnit) as the single serialization contract every read surface returns. `[common/types.rs, scripting/types.rs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Make `world-db` default-on; demote TOML to import/export so state reads hit Fjall, not disk mirrors. `[worlddb, main.rs:686,699]` `[dep: K2 codec/entities-partition load]` `[effort: M]` `[status: extend]`
- Spec radiance ingest→re-derive contract (splat in → WorldState DTO out). `[radiance, cad, mesh-edit]` `[dep: collider extraction]` `[effort: M]` `[status: research]`

**Make**
- Implement `radiance::extract_colliders(cloud, strategy)` (surface-extract → mesh-edit decimate → Avian convex-decomp OR truck CSG-fit) — fills `collider.rs:32` TODO. `[radiance/collider.rs, mesh-edit, cad]` `[dep: surface extraction]` `[effort: L]` `[status: research]` — the renderer→simulator moat.
- Land K2 codec + entities-partition load so `BinaryEcs` rkyv cores return velocity/mass/material. `[worlddb, serialization]` `[dep: none]` `[effort: M]` `[status: extend]`
- Add mass/density to the state core (mass = ColliderDensity × volume, persisted). `[common/physics, worlddb]` `[dep: DTO]` `[effort: S]` `[status: extend]`
- Wire `ecs.query`/`ecs.inspect` + MCP `query_entities`/`inspect_scene` to emit the DTO verbatim (one serializer, both runtimes). `[engine_bridge, mcp]` `[dep: DTO, bridge fix]` `[effort: S]` `[status: extend]`
- Log every state snapshot to `data` (Polars/Arrow). `[data, data-store]` `[dep: DTO]` `[effort: S]` `[status: extend]`

#### Way 2 — Geometry holds up under inspection (real B-rep/mesh topology)
**Plan**
- Author the geometry-validity contract → typed `GeometryReport` (2-manifold, no self-intersections, no degenerate faces, consistent winding, scale-sane AABB vs MeasureUnit). `[mesh-edit/error.rs, cad/error.rs]` `[dep: none]` `[effort: S]` `[status: new]` — mesh-edit has ~zero topology validation today.
- Sequence cad feature completion (Extrude works; spec Revolve/Sweep/Loft/Fillet/Chamfer vs truck, flag research). `[cad]` `[dep: none]` `[effort: M]` `[status: extend]`
- Decide B-rep↔mesh round-trip authority (truck B-rep source-of-truth → Bevy mesh + Avian collider). `[cad/eval.rs, mesh-edit]` `[dep: report]` `[effort: M]` `[status: extend]`

**Make**
- Implement mesh-edit validators: `is_manifold`, `find_self_intersections`, `find_degenerate_faces`, winding → `GeometryReport`. `[mesh-edit/half_edge.rs, ops.rs]` `[dep: contract]` `[effort: M]` `[status: new]`
- Finish bevel + loop-cut so generated topology is editable without re-triangulating. `[mesh-edit/ops.rs]` `[dep: validators]` `[effort: M]` `[status: extend]`
- Implement cad Revolve + Sweep on truck; leave Loft/Fillet/Chamfer as tracked research. `[cad]` `[dep: kernel spec]` `[effort: L]` `[status: research]`
- Add a scale-sanity gate at `create_instance` (reject/flag implausible AABB vs unit). `[common/instance_create.rs, units.rs]` `[dep: contract]` `[effort: S]` `[status: extend]`
- Expose `geometry.validate` over bridge + MCP (both runtimes). `[engine_bridge, mcp]` `[dep: validators, bridge fix]` `[effort: S]` `[status: new]`

#### Way 3 — Physics respects Newton's laws (Avian), not a renderer's impossible flames
**Plan**
- Spec determinism config: pin Avian `SubstepCount`, fixed-dt schedule, one `GlobalRngSeed`. `[main.rs:609, scenarios]` `[dep: none]` `[effort: M]` `[status: extend]` — "reproducible run" today covers only the Monte-Carlo tree, NOT physics.
- Decide FEA architecture (real mesh solver extending realism; distinct from visual-only `deformation`). `[realism (new fea)]` `[dep: none]` `[effort: M]` `[status: research]`
- Resolve gravity-unit contract (route Workspace gravity through `units` to meters; pick ONE sync system). `[units, runtime/physics.rs:42, eustress-networking/physics.rs:212]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Collapse the two gravity-sync systems into one; convert `ws.gravity` via `units`. `[runtime, eustress-networking]` `[dep: gravity contract]` `[effort: S]` `[status: extend]`
- Implement determinism config + seed plumbing; add same-seed→bit-identical test. `[engine/physics, simulation]` `[dep: determinism spec]` `[effort: M]` `[status: extend]`
- Build linear-elastic FEA (`realism::fea`) reusing `realism/numerics`. `[realism/fea (new)]` `[dep: FEA spec]` `[effort: XL]` `[status: research]`
- Reclassify `realism/deformation` outputs as visual-only in the state DTO. `[realism/deformation, common/types.rs]` `[dep: DTO]` `[effort: S]` `[status: extend]`
- Add MCP result fields distinguishing "Avian-stepped state" vs "statistical sim". `[mcp, scenarios]` `[dep: determinism config]` `[effort: S]` `[status: extend]`

#### Way 4 — Metric ground truth (meter-native Dynamic Unit System everywhere)
**Plan**
- Audit every dimensional boundary for the two-pinch-point rule; list violators (gravity, networking `scale.rs`, character movement, particle speeds). `[units, eustress-networking/scale.rs, classes.rs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec velocity/acceleration as first-class units (currently length-only `Unit` enum). `[units]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide single DisplayUnit→serializer path (Slint Properties, bridge, MCP all one converter). `[units, Properties]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Convert character/vehicle constants (16/32/50 studs/s) + networking `scale.rs` to meter-native. `[classes.rs, scale.rs]` `[dep: audit]` `[effort: M]` `[status: extend]`
- Extend `Unit` (or add `VelocityUnit`/`MassUnit`) + convert paths. `[units]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Route Workspace gravity + particle speed ranges through `units` at load/write. `[scene.rs, instance_create.rs]` `[dep: audit]` `[effort: S]` `[status: extend]` — shares Way 3 gravity fix.
- Surface `MeasureUnit`/`DisplayUnit` in `query_entities`/`inspect_scene`/`measure_distance`. `[mcp, measure_tool.rs]` `[dep: DTO]` `[effort: S]` `[status: extend]`
- Add a units round-trip test matrix (m→studs→m→ft→m bit-stability). `[units tests]` `[dep: none]` `[effort: S]` `[status: extend]`

#### Way 5 — Directly queryable world (raycast / measure_distance / query_entities)
**Plan**
- FIRST clear the blocker: spec the engine_bridge TCP accept fix (Startup bind vs Update drain race post-0.19). `[engine_bridge/server.rs, mod.rs]` `[dep: none]` `[effort: S]` `[status: extend]` — **GATE ALL Way-5 work behind this.**
- Spec new live bridge verbs `ecs.raycast`, `ecs.measure_distance`, richer `ecs.query` (today: `EcsQuery`/`EcsInspect` only; raycast is *planned* at protocol.rs:95). `[engine_bridge/protocol.rs]` `[dep: bridge fix]` `[effort: S]` `[status: extend]`
- Decide disk-MCP→live-bridge consumer path. `[mcp-server/bridge_client.rs, bridge_tools.rs]` `[dep: bridge verbs]` `[effort: M]` `[status: extend]`

**Make**
- Verify-and-fix the bridge accept live (TCP client connects + round-trips `ping`). `[engine_bridge/server.rs]` `[dep: accept spec]` `[effort: S]` `[status: extend]` — hard gate.
- Implement `ecs.raycast` (Avian spatial query) + `ecs.measure_distance` (reuse measure_tool) returning unit-tagged metric. `[engine_bridge, measure_tool.rs, physics]` `[dep: bridge fix]` `[effort: M]` `[status: extend]`
- Point mcp-server raycast/measure/query tools at the bridge client. `[mcp-server]` `[dep: bridge verbs]` `[effort: M]` `[status: extend]`
- Expose all query verbs identically to both VMs via shared host. `[common/scripting]` `[dep: bridge verbs]` `[effort: M]` `[status: extend]`
- Add `query_entities` predicates (class/tag/AABB/material) backed by WorldDb Morton index. `[worlddb, engine_bridge]` `[dep: world-db default-on]` `[effort: M]` `[status: extend]`
- Log every agent query + result to `data` as flywheel rows. `[data, mcp-server]` `[dep: bridge verbs]` `[effort: S]` `[status: extend]`

> **Verified deviation:** bridge `MethodName` has NO raycast/measure today — they exist only MCP/tool-side; Way 5's "programs read live state" is partly aspirational and gated on the bridge work. Only ONE `pub struct StudioState` was found (`ui/mod.rs`) — treat the "duplicate" as verify-then-act.

### GROUP 2 — Trust, determinism, ownership

#### Way 6 — Deterministic simulation (same inputs → same world)
**Plan**
- Write the determinism contract (covers geometry/physics/scripts/RNG; excludes rendering/GS; canonical input tuple = world snapshot + seed + tick schedule + script set). `[docs]` `[dep: none]` `[effort: S]` `[status: new]`
- Decide the fixed-timestep model (Avian in `FixedUpdate`, fixed dt + `SubstepCount`, accumulator on stall). `[main.rs, Avian]` `[dep: contract]` `[effort: S]` `[status: extend]`
- Design `GlobalRngSeed` + deterministic-RNG pattern (`StdRng` per-system from `seed.derive(system_id, tick)`). `[common]` `[dep: contract]` `[effort: S]` `[status: new]`
- Audit the 9 RNG files + HashMap iteration order + float-ordering hazards. `[engine + common]` `[dep: none]` `[effort: S]` `[status: research]` — both VMs must step on the same deterministic clock.

**Make**
- Convert physics + scripting stepping to the fixed schedule; pin `SubstepCount`; add accumulator. `[main.rs, scripting/plugin.rs]` `[dep: model]` `[effort: M]` `[status: extend]`
- Land `GlobalRngSeed`; route all 9 RNG sites through it. `[common + engine + client]` `[dep: design]` `[effort: M]` `[status: extend]`
- Collapse duplicate gravity-sync to ONE ordered system + units conversion. `[runtime, eustress-networking, units]` `[dep: model]` `[effort: S]` `[status: extend]`
- Replace nondeterministic HashMap iteration in mutating systems (stable order / BTreeMap). `[common, engine]` `[dep: audit]` `[effort: M]` `[status: extend]`
- Build determinism harness (run N ticks twice, hash final WorldDb, assert bit-equality) + MCP `verify_determinism`. `[engine, worlddb, mcp-server]` `[dep: seed, schedule]` `[effort: M]` `[status: new]`

#### Way 7 — Persistent authoritative state (WorldDb/Fjall)
**Plan**
- Single-authority decision doc (WorldDb sole writer; TOML import/export only; kill `space_is_migrated()` dual-mode). `[docs, worlddb]` `[dep: none]` `[effort: S]` `[status: new]`
- Finalize entities-partition store shape (hybrid rkyv core + TOML tree) + K2 on-disk format. `[worlddb]` `[dep: doc]` `[effort: M]` `[status: extend]`
- Design `create_instance` write-through to WorldDb on every creation surface. `[common/instance_create, space]` `[dep: store shape]` `[effort: S]` `[status: extend]`
- Spec versioned rkyv migration policy. `[worlddb]` `[dep: store shape]` `[effort: S]` `[status: new]`

**Make**
- Implement K2 codec + entities-partition load (WorldDb read-authoritative at Space open). `[worlddb, world_db_binary.rs]` `[dep: store shape]` `[effort: L]` `[status: extend]`
- Make `world-db` default-on; remove TOML fallback read path. `[main.rs]` `[dep: K2 load, migration]` `[effort: S]` `[status: extend]`
- Route every creation/edit surface through `create_instance` → WorldDb write-through; debug-assert no runtime TOML writes. `[common, engine, mcp-server, scripting]` `[dep: write-through]` `[effort: M]` `[status: extend]`
- Fix duplicate `StudioState` so drains write the WorldDb-backed state. `[engine/ui]` `[dep: none]` `[effort: S]` `[status: extend]`
- Add WorldDb integrity check + repair tool (MCP verb). `[worlddb, mcp-server]` `[dep: K2 load]` `[effort: M]` `[status: new]`

#### Way 8 — Auditable causal chains (stream/history topics; replayable counterfactuals)
**Plan**
- Define canonical causal-event schema: `(tick, actor [human/Luau/Rune/MCP], command, before→after refs, seed)` on durable `mutations.*`. `[stream, common]` `[dep: Way 6 clock]` `[effort: S]` `[status: new]` — actor MUST distinguish runtimes.
- Decide durability tier (turn ON `StreamConfig` persistence for `mutations.*`/`history.*`; full segment log for replay). `[stream]` `[dep: schema]` `[effort: S]` `[status: extend]`
- Design counterfactual-replay model (snapshot at T + command log from T, one command edited, re-execute under same seed). `[worlddb, engine, stream]` `[dep: Way 6, schema]` `[effort: M]` `[status: research]`
- Spec audit query API → existing MCP verbs (`query_audit_log`, `query_stream_events`, `compare_runs`). `[mcp-server]` `[dep: schema]` `[effort: S]` `[status: extend]`

**Make**
- Emit `mutations.*` from the single write-through point (`create_instance`) with actor + seed + before/after. `[common/instance_create, stream]` `[dep: Way 7 write-through]` `[effort: M]` `[status: extend]`
- Persist `mutations.*`/`history.*` via `StorageBackend`; wire `read_range` full-replay behind `replay_log(from_tick)`. `[stream, stream-node]` `[dep: durability]` `[effort: M]` `[status: extend]`
- Implement counterfactual fork → divergent WorldDb branch; MCP `replay_counterfactual`. `[engine, worlddb, mcp-server]` `[dep: Way 6 determinism]` `[effort: L]` `[status: new]` — **gate behind determinism harness passing.**
- Build `compare_runs` diff over two replays (entity + physical-metric delta from Polars). `[data, mcp-server]` `[dep: fork]` `[effort: M]` `[status: extend]`
- History-panel: right-click `history.<kind>` → "Replay from here as counterfactual". `[timeline_panel, ui]` `[dep: fork]` `[effort: M]` `[status: extend]`

#### Way 9 — Versioned worlds (git + WorldDb diff/branch/revert)
**Plan**
- Decide versioning substrate split (git owns `.eustress` dir + schema/assets; WorldDb owns semantic entity-level diff/branch/revert). `[docs, worlddb]` `[dep: Way 7]` `[effort: S]` `[status: new]`
- Spec WorldDb diff format (entity add/remove/modify by stable id, property deltas, unit-aware). `[worlddb, units]` `[dep: split]` `[effort: S]` `[status: new]`
- Design branch/merge on Fjall (COW partition prefix / snapshot ref; 3-way entity diff). `[worlddb]` `[dep: diff]` `[effort: M]` `[status: research]`
- Decide deterministic Fjall→text export (ordered entity dump for readable git diffs). `[worlddb]` `[dep: diff]` `[effort: S]` `[status: new]`

**Make**
- Implement entity-level `diff(snapshot_a, snapshot_b)` + MCP `worlddb_diff`. `[worlddb, mcp-server]` `[dep: format]` `[effort: M]` `[status: new]`
- Implement WorldDb branch + revert (snapshot-ref) + MCP verbs. `[worlddb, mcp-server]` `[dep: design]` `[effort: L]` `[status: new]`
- Implement 3-way merge + conflict reporting (Studio resolve surface). `[worlddb, ui]` `[dep: branch/revert]` `[effort: L]` `[status: research]`
- Add deterministic text export on commit; wire into git autosave. `[worlddb, engine]` `[dep: text-export, Way 6 ordering]` `[effort: M]` `[status: extend]` — requires Way 6 stable iteration or commits churn.
- Bridge the two layers: `version_world` = WorldDb snapshot + git commit atomically. `[mcp-server, engine]` `[dep: branch, git verbs]` `[effort: S]` `[status: extend]`

#### Way 10 — No frame-to-frame drift (state evolves by law, not by guessing)
**Plan**
- Define "law-only state evolution" invariant (between ticks, state changes ONLY via Avian or a named realism law; scripts request forces, never teleport). `[docs, common]` `[dep: Way 6]` `[effort: S]` `[status: new]` — one shared host policy for both VMs.
- Spec drift monitor (per-tick energy/momentum + constraint-residual budget). `[realism]` `[dep: invariant]` `[effort: S]` `[status: new]`
- Decide FEA scope replacing the deformation approximation (linear-elastic tet solver). `[realism + cad + mesh-edit]` `[dep: none]` `[effort: M]` `[status: research]`
- Spec GS→collider re-derivation (surface extract → decimate → convex-decomp/CSG-fit). `[radiance + cad + Avian]` `[dep: none]` `[effort: M]` `[status: research]` — ingest-and-surpass.

**Make**
- Enforce law-only: running-sim transform writes from scripts go through apply-force/impulse/set-velocity; block raw Transform mutation on simulated bodies; log violations to `mutations.*`. `[scripting (shared host), engine]` `[dep: invariant, Way 8]` `[effort: M]` `[status: extend]`
- Implement per-tick drift monitor + MCP `tail_telemetry` channel. `[realism, mcp-server]` `[dep: spec, Way 6]` `[effort: M]` `[status: new]`
- Implement linear-elastic tet FEA replacing the `deformation` approximation. `[realism]` `[dep: scope]` `[effort: XL]` `[status: research]`
- Implement GS→collider extraction (real Avian colliders that evolve under law). `[radiance/collider.rs, cad, Avian]` `[dep: spec, Way 6]` `[effort: L]` `[status: extend]`
- Re-tune `light_cull` GPU clustering for 0.19 so render stalls don't perturb the fixed-step clock. `[light_cull.rs]` `[dep: Way 6 schedule]` `[effort: S]` `[status: extend]`
- Verify engine_bridge TCP accept post-0.19 so drift telemetry is reachable. `[engine_bridge]` `[dep: none]` `[effort: S]` `[status: extend]` — clear this first.

### GROUP 3 — Editability & control

#### Way 11 — Worlds are editable structures (ECS/CAD/mesh-edit), not frozen captures
**Plan**
- Spec the editable-structure contract (every object reachable via ECS / CAD feature tree / half-edge mesh; route by kind). `[classes.rs, cad, mesh-edit]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide authoritative store for structural edits (WorldDb; write-through + reconcile-on-open). `[worlddb, toml_materializer.rs]` `[dep: dual-authority]` `[effort: M]` `[status: extend]`
- Design CAD↔ECS bridge (`CadPart` holding `FeatureTree`; evaluate-on-change → Mesh + collider; Explorer rollback bar). `[cad/eval.rs, engine, common]` `[dep: contract]` `[effort: M]` `[status: new]` — today only `keybindings.rs` references cad; nothing lifts `EvalMesh`.
- Design mesh-edit↔ECS bridge (`MeshEditMesh`; op apply + re-tessellate). `[mesh-edit/ops.rs, engine]` `[dep: contract]` `[effort: M]` `[status: new]` — mesh-edit has zero consumers today.
- Spec ingest→re-derive (splat → surface-extract → decimate → convex-decomp/CSG-fit → `CadPart`/`MeshEditMesh`). `[radiance/collider.rs, cad, mesh-edit]` `[dep: bridges]` `[effort: L]` `[status: research]`

**Make**
- Implement CAD↔ECS bridge (`Changed<CadPart>` → `cad::evaluate_tree` → Mesh3d + collider via `create_instance`). `[engine, cad, instance_create.rs]` `[dep: spec]` `[effort: M]` `[status: new]`
- Implement mesh-edit↔ECS bridge (spawn-from-Mesh, apply ops, re-derive Mesh + collider; wire to Smart Build Tools/gizmos). `[engine, mesh-edit]` `[dep: spec]` `[effort: M]` `[status: new]`
- Land bevel + loop-cut. `[mesh-edit/ops.rs]` `[dep: bridge]` `[effort: M]` `[status: extend]`
- Persist structural edits to WorldDb; undo via UndoStack/history. `[worlddb, common, engine]` `[dep: store decision]` `[effort: M]` `[status: extend]`
- Add MCP `cad_set_feature`/`cad_add_feature`/`mesh_apply_op`. `[mcp-server, engine_bridge]` `[dep: bridges + bridge fix]` `[effort: M]` `[status: new]` — **GATED** on bridge accept.

#### Way 12 — Parametric/CSG geometry (change a dimension → world updates)
**Plan**
- Spec live-parameter loop (edit `FeatureTree` var → re-run `evaluate_tree` → update mesh + collider same frame, debounced). `[cad/feature_tree.rs, engine]` `[dep: Way 11 bridge]` `[effort: S]` `[status: extend]`
- Decide variable-expression surface (extend `resolve_quantity` to full expressions; render params in Properties). `[cad/feature_tree.rs, properties.rs]` `[dep: none]` `[effort: M]` `[status: extend]` — today `variables` stores strings but only var-lookup + literal parse exists.
- Spec CSG authoring UX (`FeatureOp` + `Boolean`/`Split`/`Pattern`/`Mirror` as ribbon/gizmo ops). `[ribbon/tools, cad/feature.rs]` `[dep: Way 11 bridge]` `[effort: M]` `[status: new]`
- Prioritize blocked features (Fillet/Chamfer/Shell/Sweep/Loft `NotImplemented` pending truck) — in-house offset-surface vs OpenCascade FFI. `[cad/eval.rs]` `[dep: none]` `[effort: M]` `[status: research]`
- Spec parameter→physics propagation (rebuilt collider re-derives ColliderDensity-based mass). `[cad bridge, Avian]` `[dep: live loop]` `[effort: S]` `[status: extend]`

**Make**
- Implement the expression evaluator for `variables` (arithmetic + cross-var refs over `Quantity`). `[cad/feature_tree.rs, quantity.rs]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Wire `Changed<CadPart>` → re-eval + re-tessellate + rebuild collider, debounced. `[engine, cad/eval.rs]` `[dep: Way 11 bridge]` `[effort: M]` `[status: new]`
- Render feature-tree variables as live FloatingNumericInput dials in Properties. `[slint_ui, properties.rs]` `[dep: evaluator]` `[effort: M]` `[status: extend]`
- Expose CSG ops as MCP verbs + Smart Build Tools. `[mcp-server, tools]` `[dep: Way 11 verbs]` `[effort: M]` `[status: new]`
- Persist parametric edits + log (parameter → geometry/mass) rows for the corpus. `[worlddb, data]` `[dep: Way 11 persistence]` `[effort: M]` `[status: extend]`

#### Way 13 — Semantic addressability (identity/class/tags; act by name/type)
**Plan**
- Spec addressing model (UUID, class_name, name-path, Tags; canonical resolver order). `[instance_create.rs, classes.rs, attributes.rs]` `[dep: none]` `[effort: S]` `[status: extend]` — `instance_create` mints 32-hex UUIDs; `scripting/instance.rs` has find-by-name/class.
- Design one shared host query API (by-uuid/class/which-is-a/tag/name-path) for both VMs + MCP verbs. `[scripting, luau, mcp-server]` `[dep: model]` `[effort: M]` `[status: extend]`
- Spec Tags/CollectionService runtime (systems + index for O(1) tag→entities / class→entities). `[attributes.rs]` `[dep: none]` `[effort: S]` `[status: extend]` — `AttributesPlugin::build` is empty TODO; index not maintained.
- Spec class "which-is-a" semantics (IsA(base) via class_registry/class_schema). `[class_registry, classes.rs]` `[dep: model]` `[effort: S]` `[status: extend]`

**Make**
- Implement `AttributesPlugin` systems + `TagIndex`/`ClassIndex` maintained on spawn/despawn/Changed. `[attributes.rs]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Implement the shared resolver bound identically in Luau + Rune. `[scripting, luau/runtime.rs, Rune module]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Extend MCP `query_entities`/`find_entity`/`get_tagged_entities` by uuid+class+IsA+tag. `[mcp-server/shared_registry.rs, engine_bridge]` `[dep: bridge fix]` `[effort: M]` `[status: extend]`
- Round-trip tags + attributes to WorldDb (in the entity core). `[worlddb, attributes.rs]` `[dep: Way 11 store]` `[effort: S]` `[status: extend]`

#### Way 14 — Authoring + generation unified in one scene graph
**Plan**
- Spec unifying invariant (authored + generated are indistinguishable nodes; both via `create_instance`; differ only by `provenance` tag). `[instance_create.rs, classes.rs]` `[dep: Way 13]` `[effort: S]` `[status: extend]`
- Design generation→scene-graph write contract (loop emits `CadPart`/mesh + materials + joints via `create_instance` under a generated subtree). `[generation.rs, cad, scripting]` `[dep: Ways 11–13]` `[effort: M]` `[status: extend]`
- Resolve duplicate StudioState so authoring + generation edits land in one drained state. `[slint_ui.rs + mod.rs]` `[dep: contention]` `[effort: S]` `[status: extend]`
- Spec shared NL/agent entry (spatial-llm brief → constraints; same behavior API drives manual + generation). `[spatial-llm, scripting]` `[dep: Way 13]` `[effort: M]` `[status: extend]`
- Spec provenance + history (timeline shows authored vs generated uniformly; both undoable). `[history/timeline, stream]` `[dep: invariant]` `[effort: S]` `[status: extend]`

**Make**
- Add `Provenance` Component/metadata stamped by `create_instance`; surface in Properties/Explorer. `[instance_create.rs, engine]` `[dep: spec]` `[effort: S]` `[status: extend]`
- Route loop candidate output through `create_instance` into a live generated subtree. `[generation.rs, instance_create.rs]` `[dep: contract + Ways 11–12]` `[effort: M]` `[status: extend]`
- Fix duplicate StudioState. `[slint_ui.rs + mod.rs]` `[dep: contention]` `[effort: S]` `[status: extend]`
- Wire spatial-llm brief→constraints through the shared API; MCP `generate_into_scene`. `[spatial-llm, scripting, mcp-server]` `[dep: shared API + bridge fix]` `[effort: L]` `[status: research]`
- Log generated nodes + scores to Polars; embed into embedvec latent. `[data, embedvec]` `[dep: write path]` `[effort: M]` `[status: extend]`

#### Way 15 — Physics parameters are dials (gravity/friction/materials/constraints)
**Plan**
- Spec dial→physics contract (gravity/material/constraint change re-derives Avian Component live, not just at spawn). `[physics, material_sync.rs, Avian]` `[dep: none]` `[effort: S]` `[status: extend]` — `instance_loader.rs:208-216` wires Friction/Restitution/Density at SPAWN only.
- Resolve gravity contention FIRST (one canonical sync system, units-correct). `[runtime/physics.rs, eustress-networking, units, main.rs]` `[dep: contention]` `[effort: S]` `[status: extend]`
- Spec material dial → Avian sync (`Changed<MaterialProperties>`/`Changed<BasePart>` → Friction/Restitution/ColliderDensity). `[material_sync.rs, properties.rs]` `[dep: contract]` `[effort: M]` `[status: extend]` — `MaterialSyncPlugin` already re-syncs VISUAL on Changed; extend to physics.
- Spec constraint dials (Hinge/Distance/Prismatic/BallSocket/Spring/Rope/Universal/Motor → live Avian joint params). `[classes.rs, xpbd_joints]` `[dep: contract]` `[effort: M]` `[status: extend]` — classes.rs:228-238 still say rapier/quinnet; correct to Avian.
- Spec determinism (pin SubstepCount + fixed dt + GlobalRngSeed; reproducible covers physics). `[main.rs, simulation]` `[dep: contention]` `[effort: M]` `[status: extend]`
- Spec MCP/agent dial surface (sweep params as experiment). `[mcp-server, engine_bridge]` `[dep: bridge fix]` `[effort: S]` `[status: extend]`

**Make**
- Collapse to one gravity-sync with units conversion. `[runtime, eustress-networking, units]` `[dep: contention]` `[effort: S]` `[status: extend]`
- Extend `MaterialSyncPlugin` to re-derive Friction/Restitution/ColliderDensity on change. `[material_sync.rs]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Implement constraint-parameter live update (limits/compliance/motor targets on Changed). `[common, engine, Avian]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Pin physics determinism + self-test. `[main.rs, simulation]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Surface dials in Properties with live re-sim. `[slint_ui, properties.rs]` `[dep: sync]` `[effort: M]` `[status: extend]`
- Expose dials as MCP settable + physics-sweep `run_experiment` logging (dials → outcome). `[mcp-server, data]` `[dep: bridge fix]` `[effort: M]` `[status: extend]`

### GROUP 4 — AI-native closed loop (POMDP)

#### Way 16 — All three functions (render + simulate + act) in one substrate
**Plan**
- Write the "one-substrate" contract (WorldDb = single authoritative POMDP state; renderer + act-surface are consumers/mutators). `[worlddb, docs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec render↔simulate identity (every splat resolves to a collider; every Part renderable; document lossy spots). `[radiance, instance_create]` `[dep: contract]` `[effort: S]` `[status: new]`
- Decide determinism envelope (SubstepCount, fixed-dt, single GlobalRngSeed). `[common, main.rs:609]` `[dep: none]` `[effort: M]` `[status: new]`
- Resolve state-authority (K2 codec + entities-partition load; world-db default-on). `[worlddb, engine]` `[dep: none]` `[effort: M]` `[status: extend]`

**Make**
- Implement `radiance::collider::extract_colliders(cloud, strategy) -> Compound`. `[radiance, mesh-edit, cad]` `[dep: spec]` `[effort: L]` `[status: research]` — only honest splat→Avian path.
- Route Workspace gravity through units + collapse duplicate sync. `[units, runtime]` `[dep: determinism]` `[effort: S]` `[status: extend]`
- Make WorldDb default-on; guard TOML fallback. `[engine, worlddb]` `[dep: K2 load]` `[effort: M]` `[status: extend]`
- Land `substrate_roundtrip` test (create → simulate → render-capture → persist → reload → identical state + capture). `[worlddb, engine, radiance]` `[dep: all above]` `[effort: M]` `[status: new]`

#### Way 17 — A real training ground for planners/agents (run_experiment / run_simulation at scale, safely)
> **Honesty:** today's `run_simulation`/`run_experiment` drive a Monte-Carlo Bayesian hypothesis-tree sampler (`scenarios/engine.rs`), NOT the physics/ECS world. The MC engine becomes the *outer* search over physical rollouts.

**Plan**
- Define the POMDP episode contract (`reset → (observation, reward, done, info)` over the *physical* substrate). `[engine, common]` `[dep: Way 16 determinism]` `[effort: M]` `[status: new]`
- Spec "at scale" for physical rollouts (N sub-Apps vs N `forge` instances vs Avian-only mini-worlds). `[forge, engine, scaling]` `[dep: episode]` `[effort: M]` `[status: new]` — SCALING_ARCHITECTURE bounds per-world ≤100K.
- Spec "safely" (scratch WorldDb space + scratch asset root + step/time budget + rollback-on-crash). `[worlddb, engine]` `[dep: episode]` `[effort: M]` `[status: new]` — episodes must pin their own FileAssetReader root.
- Decide reproducibility (manifest = seed + substep + asset hash + scenario-tree hash). `[scenarios, worlddb]` `[dep: Way 16]` `[effort: M]` `[status: extend]`

**Make**
- Extend `scenarios/engine.rs` so a `BranchNode` rollout invokes a *physical* episode (reuse `scenarios/scripting.rs` Rune hooks). `[scenarios, realism]` `[dep: episode]` `[effort: L]` `[status: extend]`
- Implement headless parallel episode runner (sandboxed WorldDb + pinned root + budget; collect trajectories). `[engine, worlddb]` `[dep: at-scale + safely]` `[effort: L]` `[status: new]`
- Add manifest + deterministic replay + extend `compare_runs` over physical trajectories. `[engine, mcp-server]` `[dep: reproducibility]` `[effort: M]` `[status: extend]`
- Emit every episode to `data`/`data-store` as trajectory rows. `[data, data-store]` `[dep: runner]` `[effort: S]` `[status: extend]` — feeds Way 20.

#### Way 18 — Agent-callable over MCP
**Plan**
- Triage the bridge accept blocker first (Startup bind + Update drain accept on 0.19; 500ms `recv_timeout` race). `[engine_bridge]` `[dep: none]` `[effort: S]` `[status: research]` — **gates Ways 17–20.**
- Spec the unified live verb set (which tools route over bridge `bridge_tools.rs` vs disk `tools.rs`; migrate sim/experiment/structural verbs onto the bridge). `[mcp-server, engine_bridge]` `[dep: accept fix]` `[effort: M]` `[status: extend]`
- Design "learn in-world" contract (bridge responses = POMDP observations: state delta + image + reward). `[engine_bridge, mcp-server]` `[dep: Way 17 episode]` `[effort: M]` `[status: extend]`
- Resolve duplicate StudioState before agents drive UI actions. `[engine/ui]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Fix/verify the bridge accept loop end-to-end live + smoke test. `[engine_bridge, mcp-server]` `[dep: triage]` `[effort: M]` `[status: research]`
- Add bridge `MethodName` variants for `RunSimulation`, `RunExperiment`, structural/optimizer verbs. `[engine_bridge]` `[dep: verb spec]` `[effort: M]` `[status: extend]`
- Make `capture_viewport`/`ai_camera_capture` return image + depth/segmentation in one packet. `[engine_bridge]` `[dep: learn-in-world]` `[effort: M]` `[status: extend]` — same buffers as Way 20.
- Add agent-loop conformance harness (N steps inspect→act→capture→score; assert determinism + latency). `[mcp-server, engine]` `[dep: verbs + packet]` `[effort: M]` `[status: new]`

#### Way 19 — Live programmable behavior (Luau/Rune scripted agents and rules)
> **Structural gap (contention #4):** two parallel stacks (Luau `runtime.rs` ~5102 LOC; Rune `rune_ecs_module.rs` ~3745 LOC) with no shared behavior API. Every subtask covers BOTH via one host API.

**Plan**
- Define ONE host-side behavior/ECS-binding API (instance CRUD, query, raycast, signal/connect, sim read/write, scheduler hooks). `[scripting, realism/scripting]` `[dep: none]` `[effort: M]` `[status: extend]`
- Spec the in-sim rules model (declarative constraints/triggers/rewards → Bevy systems, fixed-step). `[scripting, engine]` `[dep: shared API]` `[effort: M]` `[status: new]` — rules double as gen-loop hard-constraints via `realism/symbolic`.
- Decide determinism/sandbox for scripted behavior (per-VM step budget, seeded RNG handle, no wall-clock). `[scripting]` `[dep: Way 16/17]` `[effort: M]` `[status: new]`
- Spec parity tests (behavior-API conformance run against both runtimes). `[scripting]` `[dep: shared API]` `[effort: S]` `[status: new]`

**Make**
- Refactor Luau (`runtime.rs`) + Rune (`rune_ecs_module.rs`) host bindings onto the single shared API. `[scripting]` `[dep: spec]` `[effort: L]` `[status: extend]`
- Implement declarative rule registry (rules → fixed-step systems; violations → reward/`done` + symbolic feasibility). `[engine, realism/symbolic]` `[dep: rules spec]` `[effort: L]` `[status: new]`
- Wire deterministic scripting mode into both VM pools, gated under the manifest. `[scripting]` `[dep: sandbox spec]` `[effort: M]` `[status: extend]`
- Add dual-runtime conformance suite to CI (same scenario, Luau + Rune, identical deltas). `[scripting]` `[dep: parity spec, refactor]` `[effort: M]` `[status: new]`

#### Way 20 — Synthetic ground-truth data flywheel (labeled geometry / depth / segmentation / physics)
> Foundations: `data` (Polars/Arrow), MCP export already advertises `training_data: true`/`spatial_export: true`. Missing: labeled render outputs (depth + segmentation), the (design → performance) schema, automated emit. Physics labels are true only where the substrate is true — deformation is visual-only, FEA absent.

**Plan**
- Define labeled-sample schema {RGB, depth, instance-seg, semantic-seg, camera intrinsics/extrinsics, geometry refs, Avian state, realism material params}. `[data, common]` `[dep: none]` `[effort: M]` `[status: new]`
- Scope "physics" labels honestly (rigid/joint/contact + closed-form `structures` = ground-truth-grade now; deformation/stress-field = research-grade). `[realism]` `[dep: schema]` `[effort: S]` `[status: research]`
- Spec the flywheel control loop (episodes + candidates → labeled rows → embed → condition next generation; surrogate-training read path). `[data, embedvec, spatial-llm]` `[dep: schema, Way 17]` `[effort: M]` `[status: new]`
- Decide storage/versioning (WorldDb-authoritative design records + Polars shards + dataset manifests + provenance per row). `[worlddb, data, data-store]` `[dep: schema]` `[effort: M]` `[status: extend]`

**Make**
- Implement labeled G-buffer export (depth + instance-seg + semantic-seg + RGB + camera matrices). `[engine, radiance]` `[dep: schema; Way 18 packet]` `[effort: L]` `[status: extend]` — reuse Way 18 observation buffers.
- Implement physics-label extractor (Avian pose/velocity/contacts/joint-force + closed-form `structures` per step → Polars). `[realism/structures, data]` `[dep: schema, Way 16]` `[effort: M]` `[status: extend]`
- Build auto-emit pipeline (every episode + every candidate, feasible OR failed, with provenance). `[data, data-store, engine]` `[dep: loop spec, Way 17]` `[effort: M]` `[status: new]`
- Close the flywheel (encoder embeds rows into embedvec; read path conditions next batch). `[embedvec, spatial-llm, data]` `[dep: auto-emit]` `[effort: L]` `[status: research]` — no-RL simulation-in-the-loop conditioning.

### GROUP 5 — Hybrid representation

#### Way 21 — Splats for appearance + mesh/CSG/voxel for state (state primary)
**Plan**
- Spec dual-channel scene contract (appearance handle `SplatCloud` bound to state handle mesh/CSG/voxel + Avian Collider; state declared authoritative). `[radiance, instance_create]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide link model (parent-state / child-visual so raycast/query/Avian operate on state). `[common ECS, space]` `[dep: contract]` `[effort: S]` `[status: new]`
- Define `RepresentationKind` + `PrimaryRepresentation` marker. `[common]` `[dep: link model]` `[effort: S]` `[status: new]`
- Spec WorldDb persistence (state rkyv core authoritative; splat = asset ref + transform). `[worlddb, common]` `[dep: kind]` `[effort: M]` `[status: extend]` — FileAssetReader per-Space root swap: splat source must round-trip Space-relative or `file://`.

**Make**
- Add `LinkedAppearance`/`LinkedState` pair + sync system (state drives, appearance follows). `[radiance, common]` `[dep: link model]` `[effort: S]` `[status: new]`
- Route splat spawning through `create_instance` (replace demo `spawn_splat_cloud`). `[instance_create, radiance]` `[dep: kind]` `[effort: S]` `[status: extend]`
- Extend Properties to show both channels + pick primary. `[Slint UI]` `[dep: PrimaryRepresentation]` `[effort: M]` `[status: extend]`
- Make Avian + raycast/query/measure resolve to state channel only; regression test (raycast hits mesh proxy, not splats). `[mcp-server, physics]` `[dep: link model]` `[effort: M]` `[status: extend]`
- Land WorldDb-authoritative design records (K2 + entities-partition load). `[worlddb]` `[dep: contract]` `[effort: M]` `[status: extend]`

#### Way 22 — Relightable path (inverse-render GS → per-splat PBR + real dynamic lights)
**Plan**
- Lock relighting contract (import per-splat Cook-Torrance albedo/roughness/metallic/normal as PBR surfels → PBR renderer + `LightClassPlugin` + shadows + atmosphere). `[radiance, light_sync]` `[dep: Phase-0 render]` `[effort: S]` `[status: research]` — USABLE-RESEARCH, not SHIPPING.
- Spec PPISP front-end completion order (vignetting → color homography → CRF; each forward + adjoint, validated vs CPU f64 oracle). `[ppisp]` `[dep: exposure (exists)]` `[effort: M]` `[status: extend]`
- Pick inverse-render trainer (GS-IR / R3DG / GaussianShader); flag noisy Gaussian normals → weak specular. `[radiance (offline)]` `[dep: PPISP front-end]` `[effort: L]` `[status: research]`
- Define PBR-surfel import format + fork seam in `bevy_gaussian_splatting`. `[radiance]` `[dep: contract]` `[effort: M]` `[status: research]`

**Make**
- Implement PPISP vignetting + color + CRF (CPU ref then WGSL, validated at tolerance). `[ppisp]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Build offline inverse-render bake (port/FFI → `.gcloud` + PBR sidecar). `[radiance, texture-gen]` `[dep: trainer choice]` `[effort: L]` `[status: research]`
- Fork splat renderer to emit normal+albedo+roughness+metallic G-buffer → `LightClassPlugin` + shadows; ship one moving-light demo. `[radiance, light_sync]` `[dep: surfel import + light_cull re-tune]` `[effort: L]` `[status: research]`
- Add MCP/Properties toggle baked-vs-engine lighting + place dynamic light. `[mcp-server, engine UI]` `[dep: G-buffer]` `[effort: M]` `[status: extend]`

#### Way 23 — Many representations coexist (splats / mesh / voxel / SDF / point cloud)
**Plan**
- Spec a `Representation` capability matrix (per kind: render? collide? raycast? boolean-edit? mass-properties?). `[common]` `[dep: Way 21 kind]` `[effort: S]` `[status: new]`
- Decide conversion edges as a graph (splat→mesh §4; pointcloud→mesh via existing `reconstruct_surface`; mesh→CSG via truck; mesh→voxel; mesh→SDF; lossy vs reversible). `[common, cad, mesh-edit]` `[dep: matrix]` `[effort: M]` `[status: extend]`
- Reconcile existing `pointcloud` module (Poisson/MarchingCubes/Delaunay + quadric simplify + LOD) as first-class. `[common/pointcloud]` `[dep: graph]` `[effort: S]` `[status: extend]`
- Spec which representation each MCP verb prefers + fallback chain. `[mcp-server]` `[dep: matrix]` `[effort: S]` `[status: extend]`

**Make**
- Add `Representation` enum + capability flags + `representation_of(entity)` helper. `[common]` `[dep: matrix]` `[effort: M]` `[status: new]`
- Implement conversion entry points / MCP verbs (`to_mesh`/`to_csg`/`to_voxel`/`to_sdf`/`to_pointcloud`). `[cad, mesh-edit, pointcloud, mcp-server]` `[dep: edges]` `[effort: L]` `[status: extend]`
- Promote pointcloud placeholder reconstructors (Delaunay/BallPivoting) to working impls. `[common/pointcloud]` `[dep: reconcile]` `[effort: M]` `[status: extend]`
- Make Properties render any representation polymorphically; add/switch without losing others. `[Slint UI]` `[dep: enum]` `[effort: M]` `[status: extend]` — resolve duplicate StudioState first.

#### Way 24 — PPISP-grounded capture (photometric correction → honest ingested data)
**Plan**
- Spec ingest order (raw multi-view → PPISP exposure→vignette→color→CRF → corrected frames → reconstruction). `[ppisp, radiance]` `[dep: Way 22 transforms]` `[effort: S]` `[status: extend]`
- Define ingest-provenance record (per-frame EV/WB/vignette/CRF latents, residual error). `[data, worlddb]` `[dep: forward+backward]` `[effort: M]` `[status: new]`
- Spec the latent-fit optimizer (burn MLP + adjoint). `[ppisp]` `[dep: four transforms]` `[effort: L]` `[status: research]`
- Decide ingest surface for Marble / World API (correct-then-re-derive). `[radiance, mcp-server]` `[dep: order]` `[effort: M]` `[status: research]` — PPISP is photometric only, NOT collision/geometry.

**Make**
- Wire `ppisp_correct(frames, config) → corrected + latents` (GPU validated vs f64 oracle). `[ppisp]` `[dep: transforms]` `[effort: M]` `[status: extend]`
- Implement latent-fit optimizer + recovered params + residuals. `[ppisp]` `[dep: correct]` `[effort: L]` `[status: research]`
- Persist provenance (latents + residuals + source id) to Polars + WorldDb sidecar. `[data, worlddb]` `[dep: spec]` `[effort: M]` `[status: new]`
- Add `ingest_capture` MCP verb (path/URL → corrected cloud + provenance). `[mcp-server]` `[dep: correct + provenance]` `[effort: M]` `[status: research]` — respect per-Space root swap.

#### Way 25 — Physics from splats (surface extraction → colliders)
**Plan**
- Lock the §4 extraction pipeline (2DGS TSDF+MC indoor / GOF MT outdoor → mesh-edit weld+decimate ~5–20K tris → CoACD 16–64 hulls / V-HACD fallback / truck CSG fit → Avian compound). `[radiance/collider]` `[dep: Phase-0 render]` `[effort: M]` `[status: extend]`
- Decide strategy selector (static shell→trimesh/CoACD; dynamic→CoACD capped; blocky→CSG fit) backing `ColliderStrategy`. `[radiance/collider]` `[dep: pipeline]` `[effort: S]` `[status: extend]`
- Choose surface-extraction impl (reuse `pointcloud::reconstruct_surface` vs dedicated 2DGS/GOF). `[pointcloud, radiance]` `[dep: pipeline]` `[effort: L]` `[status: research]` — splat→watertight-surface doesn't exist in Rust today.
- Spec Tier-B deformable track (VR-GS: VDB→MC→TetGen tet cage + XPBD/FEM + two-level skinning) — parallel, never blocking Tier A. `[radiance, physics]` `[dep: Tier A]` `[effort: XL]` `[status: research]`

**Make**
- Implement `extract_colliders(cloud, strategy) -> Compound` (the `collider.rs` TODO); ship Tier A. `[radiance/collider, mesh-edit, physics]` `[dep: extraction choice]` `[effort: L]` `[status: extend]`
- Implement Tier A′ truck CSG primitive fit for blocky scans. `[cad, radiance/collider]` `[dep: extract]` `[effort: M]` `[status: extend]`
- Attach extracted colliders as Way-21 state channel + persist proxy mesh + strategy in WorldDb. `[radiance, worlddb, common]` `[dep: Way 21 link]` `[effort: M]` `[status: extend]`
- Add MCP `extract_colliders` + smoke verb (drop rigid body on splat floor, report contact). `[mcp-server]` `[dep: extract]` `[effort: M]` `[status: extend]` — gate on bridge accept + determinism.
- Prototype Tier-B offline (NVIDIA Kaolin/Simplicits reference). `[radiance (offline), physics]` `[dep: Tier A]` `[effort: XL]` `[status: research]`

### GROUP 6 — Multi-physics & realism depth

> **Verified:** realism is far deeper than the brief implied — `fluids` (sph/water/aerodynamics/buoyancy), `plasma` (mhd/fusion/debye), `nuclear`, `thermocycles`, `chemistry`, `electrical`, `quantum`, full `laws/` tree, `numerics/statistics`, a `gpu/` compute path. Confirmed gaps: `structures` is closed-form (no FEM assembly); `deformation` is vertex-displacement (visual); `radiance/collider.rs` extract is a scaffold.

#### Way 26 — Multi-physics substrate (rigid + soft + constraints + FEA + fluids + nuclear)
**Plan**
- Publish the verified physics-substrate inventory; mark `structures` closed-form-only and `deformation` visual-only. `[realism + Avian]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide coupling contract (one `PhysicsSet` ordering + fixed-dt: Avian → realism law systems → deformation/FEA read-back). `[realism, main.rs]` `[dep: determinism]` `[effort: M]` `[status: extend]`
- Spec soft-body tier (XPBD soft constraints vs mass-spring; "soft = simulated" bar). `[deformation, Avian]` `[dep: contract]` `[effort: M]` `[status: research]`
- Spec FEA as `realism/structures→fea` (linear-elastic tet/hex, global stiffness, sparse solve via nalgebra-sparse/faer). `[realism]` `[dep: audit]` `[effort: L]` `[status: research]`
- Lock fluids roadmap (keep `sph` interactive; add grid/FVM behind a flag). `[fluids]` `[dep: contract]` `[effort: M]` `[status: extend]`

**Make**
- Implement unified fixed-dt schedule + SubstepCount pin + single GlobalRngSeed; collapse the two gravity-sync systems through units. `[realism, main.rs, runtime, eustress-networking]` `[dep: contract]` `[effort: M]` `[status: extend]`
- Build FEA MVP (linear static tet4 assembly + BCs + sparse solve → stress/displacement → `visualizers/stress_viz.rs`). `[realism/structures/fea]` `[dep: FEA spec]` `[effort: L]` `[status: new]`
- Wire FEA + closed-form structures + Avian stability into one `calculate_physics`/`run_experiment` schema. `[mcp, realism]` `[dep: FEA MVP]` `[effort: M]` `[status: extend]`
- Implement soft-body XPBD constraints; validate vs cantilever-sag. `[deformation, Avian]` `[dep: soft spec]` `[effort: M]` `[status: new]`
- Extend nuclear + thermocycles coupling (reactor thermal → Rankine/Brayton loop). `[nuclear, thermocycles]` `[dep: contract]` `[effort: M]` `[status: extend]`

#### Way 27 — Real physical-laws crate (optics/photons, thermodynamics, statistics)
**Plan**
- Inventory + certify the `laws/` tree (optics/{geometric,photons,wave}, electromagnetism, acoustics, kinetics, biology, mechanics, thermodynamics, conservation, electrochemistry; numerics/statistics) — SI units, validity, source eq. `[laws + numerics]` `[dep: none]` `[effort: M]` `[status: extend]`
- Define a "law card" standard (equation, assumptions, units, accuracy class, citation) per public law fn. `[realism]` `[dep: inventory]` `[effort: S]` `[status: new]`
- Decide units-enforcement boundary (laws take/return SI; convert only at the units edge — fixes the gravity class of bug). `[realism, units]` `[dep: inventory]` `[effort: S]` `[status: extend]`
- Spec coverage gaps (radiometric/spectral optics, real-gas thermo, uncertainty-propagation over statistics). `[laws/optics, thermodynamics, numerics/statistics]` `[dep: inventory]` `[effort: M]` `[status: research]`

**Make**
- Add law-card doc-attributes + compile-time registry (introspection enumerates laws w/ units + accuracy). `[realism, mcp]` `[dep: card spec]` `[effort: M]` `[status: new]`
- Implement uncertainty/error-propagation utility in numerics/statistics; surface in `run_experiment`. `[numerics/statistics, mcp]` `[dep: stats inventory]` `[effort: M]` `[status: extend]`
- Extend optics (spectral/radiometric) and connect to `radiance` + `ppisp`. `[laws/optics, radiance, ppisp]` `[dep: optics gap]` `[effort: L]` `[status: extend]` — ingest-and-surpass: real optics re-derives radiometric state.
- Expose laws to BOTH runtimes via shared host (`scripting/laws/*` reachable from Luau + Rune identically). `[realism/scripting/laws, mcp]` `[dep: cards]` `[effort: M]` `[status: extend]`

#### Way 28 — Engineering-grade accuracy path (CAD + FEA)
**Plan**
- Define the AEC pipeline (cad B-rep/CSG → mesh-edit meshing → structures + FEA → results, with tolerances per stage). `[cad, mesh-edit, realism]` `[dep: Way 26 FEA]` `[effort: M]` `[status: extend]`
- Scope CAD feature completion (revolve/sweep/fillet/boolean by AEC frequency). `[cad]` `[dep: pipeline]` `[effort: M]` `[status: extend]`
- Spec code-compliance rule pack via `symbolic` (clearances, spans, load combinations as hard constraints). `[realism/symbolic]` `[dep: pipeline]` `[effort: M]` `[status: research]`
- Decide design-record authority (CAD/FEA records WorldDb-authoritative; K2 + entities-partition load). `[worlddb, common]` `[dep: none]` `[effort: M]` `[status: extend]`

**Make**
- Implement cad-mesh → FEA mesh adapter (truck eval → tet mesh + material from properties.rs). `[cad, mesh-edit, realism/structures/fea]` `[dep: FEA MVP, pipeline]` `[effort: L]` `[status: new]`
- Implement code-compliance rule pack over symbolic + `check_compliance` MCP verb. `[realism/symbolic, mcp]` `[dep: rule spec]` `[effort: M]` `[status: new]`
- Add engineering report surface (dimensioned `quantity.rs` + FEA safety factors + compliance → Polars + Properties). `[cad/quantity, data, Properties]` `[dep: FEA MVP]` `[effort: M]` `[status: extend]` — fix duplicate StudioState.
- Persist CAD feature_tree + FEA results as WorldDb-authoritative; verify round-trip. `[worlddb, cad]` `[dep: authority]` `[effort: M]` `[status: extend]`

#### Way 29 — Verifiable domains (e.g. fission sim) checkable against known physics
**Plan**
- Pick the first verifiable set (nuclear criticality/decay/shielding, thermocycles efficiency, optics lensing, beam deflection). `[nuclear, thermocycles, laws/optics, structures]` `[dep: none]` `[effort: S]` `[status: extend]`
- Define verification harness (golden-value tests, tolerance bands, analytic reference + citation, headless + `run_experiment`). `[realism, mcp]` `[dep: case set]` `[effort: M]` `[status: new]` — tie to Way 27 law cards.
- Spec determinism prerequisites (fixed dt, seeded RNG, no frame-rate dependence). `[realism, main.rs]` `[dep: Way 26 determinism]` `[effort: S]` `[status: extend]`

**Make**
- Implement nuclear golden-reference suite (six-factor/criticality, half-life, shielding attenuation). `[nuclear]` `[dep: harness, determinism]` `[effort: M]` `[status: new]`
- Extend to optics (thin-lens/Snell), thermocycles (Carnot bound), structures (beam deflection). `[laws/optics, thermocycles, structures]` `[dep: nuclear suite]` `[effort: M]` `[status: extend]`
- Expose `verify_domain` (sim-vs-analytic deltas) via both runtimes. `[mcp, realism/scripting]` `[dep: harness]` `[effort: M]` `[status: new]`
- Log every verification run to Polars as highest-quality corpus rows. `[data, realism]` `[dep: harness]` `[effort: S]` `[status: extend]`

#### Way 30 — Smaller sim-to-real gap (real units + real laws + real materials)
**Plan**
- Define the triad contract (real units + real laws + real materials, one accuracy ledger). `[units, laws, materials]` `[dep: Ways 27/29]` `[effort: M]` `[status: extend]`
- Audit + expand material DB (E, ν, density, yield, thermal with provenance per material). `[materials]` `[dep: none]` `[effort: M]` `[status: extend]`
- Spec ingest-and-surpass metric ("gap closed" = geometric + physical fidelity vs source). `[radiance, mesh-edit, cad]` `[dep: none]` `[effort: M]` `[status: research]`

**Make**
- Implement units-at-the-edge repo-wide (SI at every physics/law boundary) + a gravity-mismatch regression. `[units, realism, runtime]` `[dep: triad]` `[effort: M]` `[status: extend]`
- Expand material library with sourced values + provenance; expose via `query_material`. `[materials, mcp]` `[dep: audit]` `[effort: M]` `[status: extend]`
- Implement GS→collider extraction (surface extract → decimate → convex-decomp / CSG-fit). `[radiance/collider.rs, mesh-edit, cad, Avian]` `[dep: ingest spec]` `[effort: L]` `[status: new]` — gate on bridge accept.
- Build sim-to-real scorecard (verification deltas + units coverage + material provenance → Polars + Studio). `[data, Slint, realism]` `[dep: triad, harness]` `[effort: M]` `[status: extend]`

### GROUP 7 — Scale, performance, deployment

> **Canonical plan:** `docs/architecture/SCALING_ARCHITECTURE.md` (P1–P6). Licensing is uniformly MIT OR Apache-2.0. Residency manager (Phase 2), Morton (built, not default), HLOD + render_cascade exist; `.echk` bake unwired.

#### Way 31 — 10M-entity scaling (persistence + Morton streaming + GPU cull)
**Plan**
- Decide entities-partition load contract (K2 codec + `iter_instance_cores` range-scan). `[worlddb keys.rs, world_db_binary.rs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec residency tier table (Persisted 10M / Resident ~250–500K / Live ECS ≤100K / Drawn) + spawn caps + hysteresis. `[residency.rs]` `[dep: load contract]` `[effort: S]` `[status: extend]`
- Decide Morton-default migration (additive + verified + reversible re-key; `WorldSchemaVersion` bump). `[worlddb keys.rs, header]` `[dep: none]` `[effort: S]` `[status: extend]`
- Settle `.echk`-vs-live split (live = Fjall Morton store; `.echk` = immutable R2/cold-start). `[worlddb bake.rs]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Make `world-db` default-on (or delete TOML fallback). `[main.rs:686,699]` `[dep: load contract]` `[effort: S]` `[status: extend]`
- Flip `MortonKeyEncoder` default for `entities_uuid`/`INSTANCE_CORE`; ship FlatKey→Morton re-key. `[keys.rs, fjall_backend.rs]` `[dep: migration]` `[effort: M]` `[status: extend]`
- Replace boot-time all-load with camera-cell residency manager (diff resident set, range scans + evictions on AsyncComputeTaskPool, `spawn_batch` capped). `[residency.rs, world_db_binary.rs]` `[dep: Morton default]` `[effort: L]` `[status: extend]`
- Kill rkyv realign copy (pad to 16-byte / tag out-of-band → zero-copy `rkyv::access()` from block cache, decode off-thread). `[rkyv_values.rs]` `[dep: residency mgr]` `[effort: M]` `[status: extend]`
- Coalesce per-frame Changed mirror into one atomic Fjall `WriteBatch`. `[world_db_binary.rs]` `[dep: residency mgr]` `[effort: M]` `[status: extend]`
- Wire all create sites to honor `representation_for_part` (scalable Part → rkyv core + indices; custom meshes stay FileSystem). `[instance_create.rs, representation.rs, promote.rs]` `[dep: load contract]` `[effort: L]` `[status: extend]` — the create-flip; flywheel bulk writes reuse this.
- Extend `generate_benchmark_map --binary-ecs N` past 2.1M toward 10M + moving-camera flythrough harness (live-count, p50/p99, hitch, SSTable levels → compare_runs). `[generate_benchmark_map.rs, active_db.rs]` `[dep: residency mgr]` `[effort: M]` `[status: extend]`
- GPU-driven cull → indirect draw (frustum + HZB occlusion in compute, compacted multi_draw_indirect). `[instanced_pbr.rs]` `[dep: residency mgr]` `[effort: XL]` `[status: new]` — needs light_cull 0.19 re-tune first.

#### Way 32 — Real-time on consumer GPUs (Bevy/wgpu)
**Plan**
- Define consumer-GPU target matrix + frame-budget (3060 / 6600 / M-series iGPU @ 1080p60). `[SCALING_ARCHITECTURE §1]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec quality-scalability ladder (GTAO/SSR/TAA/shadow cascades/impostor distance → one preset resource). `[lighting_plugin.rs, render_cascade.rs]` `[dep: matrix]` `[effort: S]` `[status: new]`
- Decide C2 cutover gate (reference renders pixel-match before retiring clone-per-entity path). `[material_sync.rs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec fixed-dt physics + SubstepCount pin (frame-rate variance must not change outcomes). `[main.rs:609]` `[dep: none]` `[effort: S]` `[status: extend]` — same determinism fix the agent loop needs.

**Make**
- Re-tune `light_cull` GPU clustering for 0.19/wgpu 29. `[light_cull / lighting]` `[dep: 0.19 merge]` `[effort: M]` `[status: extend]` — prerequisite for any lit benchmark.
- Port `material_sync` tint math to per-instance material-params GPU storage buffer (dual path until parity green). `[material_sync.rs, instanced_pbr.rs]` `[dep: parity gate]` `[effort: L]` `[status: extend]`
- Implement bindless / sparse-virtual texture array (unique material = array slot, not pipeline change). `[rendering]` `[dep: params buffer]` `[effort: XL]` `[status: new]`
- Implement physics LOD (colliders/rigidbodies only on Hero+Active tiers; restore-on-promote verified). `[render_cascade.rs, Avian]` `[dep: residency tiers]` `[effort: L]` `[status: extend]`
- Ship togglable photoreal stack (GTAO, SSR + probes, shadow caps, contact shadows, TAA, bloom, auto-exposure, ACES/AgX, volumetric fog). `[lighting_plugin.rs]` `[dep: quality ladder]` `[effort: L]` `[status: extend]`
- Pin Avian determinism + seed all RNG behind GlobalRngSeed. `[main.rs:609-610]` `[dep: fixed-dt]` `[effort: M]` `[status: extend]`

#### Way 33 — Web-native + lightweight (wasm, single binary)
> **HONEST GAP:** the shipped engine is the monolithic Slint **desktop** app; Slint does not target wasm. `crates/web` is a Leptos CSR marketing/auth frontend, NOT the engine. The engine-in-wasm path today is the separate `eustress_demo` + `eustress-svelte`. Bevy/wgpu DO compile to wasm — the blocker is the Slint editor shell + the monolithic crate.

**Plan**
- Decide web product shape (thin viewer/runtime in wasm vs full editor → viewer-first; editor stays desktop). `[research]` `[dep: none]` `[effort: S]` `[status: research]`
- Audit wasm-clean vs blocked crates (Slint, Fjall on-disk LSM, AsyncComputeTaskPool/threads, engine_bridge TCP). `[engine + worlddb + common]` `[dep: shape]` `[effort: M]` `[status: research]` — also the decomposition lever.
- Spec wasm persistence (IndexedDB/OPFS core cache or read-only `.echk` from R2). `[worlddb backend.rs, bake.rs]` `[dep: shape]` `[effort: M]` `[status: research]`
- Decide getrandom/threadpool/WebGPU-vs-WebGL2 + canvas mount. `[.cargo-watch.toml, .cargo/config.toml]` `[dep: audit]` `[effort: S]` `[status: exists]`

**Make**
- Carve `eustress-runtime-wasm` (Bevy app + sim, no Slint) reusing the `eustress_demo` wasm-bindgen pipeline. `[engine → new wasm crate]` `[dep: audit]` `[effort: XL]` `[status: research]` — forces partial decomposition.
- Implement wasm WorldDb backend behind the `WorldDb` trait (OPFS/IndexedDB + read-only `.echk` fetch). `[worlddb backend.rs]` `[dep: persistence spec]` `[effort: L]` `[status: research]`
- Re-enable `wasm-opt` + CI wasm build + size budget. `[crates/web/Trunk.toml, CI]` `[dep: runtime-wasm]` `[effort: M]` `[status: extend]`
- Wire `radiance` GS playback into the web viewer (cheapest web-native win). `[radiance, runtime-wasm]` `[dep: runtime-wasm]` `[effort: L]` `[status: extend]` — make FileAssetReader root swap wasm-safe.
- Produce a true single-binary desktop distributable (embedded assets/WorldDb open path). `[engine packaging]` `[dep: world-db default-on]` `[effort: M]` `[status: extend]`

#### Way 34 — Open & owned (Rust, MIT/Apache-2.0 stack)
> **VERIFIED:** every first-party crate carries `MIT OR Apache-2.0`; vendored fjall/lsm-tree/gpu-allocator/wgpu are permissive.

**Plan**
- Decide open-core surface (common/realism/worlddb/cad/mesh-edit open; forge/identity/deck proprietary) + dual-license header policy. `[workspace]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec license-audit gate (cargo-deny / cargo-about; no GPL/AGPL enters silently). `[CI, Cargo.toml]` `[dep: none]` `[effort: S]` `[status: new]`
- Define data-ownership contract (`.eustress` dir fully user-owned + portable; documented open formats). `[worlddb .eustress]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec synthetic-data export schema (every candidate as portable Polars/Arrow the user owns). `[data]` `[dep: none]` `[effort: S]` `[status: extend]`

**Make**
- Add LICENSE-MIT + LICENSE-APACHE + per-crate SPDX headers. `[workspace]` `[dep: surface]` `[effort: S]` `[status: extend]`
- Add cargo-deny/cargo-about to CI + THIRD-PARTY notices. `[CI]` `[dep: gate spec]` `[effort: S]` `[status: new]`
- Document + stabilize the open `.eustress` on-disk format (rkyv `ArchInstanceCore` + `EusValue` tail + partition layout). `[rkyv_values.rs, keys.rs]` `[dep: contract]` `[effort: M]` `[status: extend]`
- Ship flywheel export verb (candidate rows → user-owned Parquet via `data`). `[data, mcp-server, scenarios]` `[dep: export schema]` `[effort: M]` `[status: extend]`

#### Way 35 — Streaming + LOD for worlds (explore city-scale without loading it all)
**Plan**
- Define streaming UX contract (continuous flythrough with impostor coverage; p99 hitch < 2 ms). `[residency.rs, render_cascade.rs]` `[dep: residency mgr (Way 31)]` `[effort: S]` `[status: extend]`
- Spec HLOD authoring (merge distant Morton cell meshes into one decimated proxy; built-once + persisted + toggled). `[hlod.rs]` `[dep: Morton default]` `[effort: M]` `[status: extend]`
- Decide impostor-atlas bake (octahedral atlas → cascade `MeshLodTier::Lod3` swap). `[asset bake, render_cascade.rs]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec ring pre-fetch + dead-zone hysteresis. `[residency.rs ResidencyConfig]` `[dep: residency mgr]` `[effort: S]` `[status: extend]`

**Make**
- Wire `.echk` bake into live streaming (immutable cold-start/region-prefetch; R2-fetchable). `[bake.rs, residency.rs]` `[dep: residency mgr]` `[effort: L]` `[status: extend]` — web viewer (Way 33) fetches these.
- Extend `render_cascade` from visibility-only into a full tier reactor (mesh-LOD→impostor swap + physics-LOD add/remove). `[render_cascade.rs]` `[dep: residency tiers]` `[effort: L]` `[status: extend]`
- Implement HLOD merged-cell proxy generation + persistence + toggle. `[hlod.rs]` `[dep: HLOD spec]` `[effort: L]` `[status: extend]`
- Bake octahedral impostor atlases; swap mesh→billboard at Streamed tier. `[asset bake, billboard pipeline]` `[dep: impostor contract]` `[effort: M]` `[status: extend]`
- Make FileAssetReader per-Space root swap streaming-safe. `[asset reader]` `[dep: none]` `[effort: M]` `[status: extend]` — also blocks radiance splat loading.
- Add city-scale flythrough acceptance benchmark → compare_runs. `[generate_benchmark_map.rs, data]` `[dep: residency mgr + HLOD]` `[effort: M]` `[status: extend]`

### GROUP 8 — Causality, knowledge, reasoning

> **Verified:** `units.rs` is length-only (no Dimension/Quantity); `symbolic/causal.rs` `CausalModel` is real but feature-gated/scalar-only; `embedvec/ontology.rs` has `OntologyTree`; `class_schema` lives in both `common/src` and `common/assets`.

#### Way 36 — True causal model (interventions/counterfactuals on real state — do-calculus)
> The `CausalModel` already has a causal graph + Symbolica derivatives + `do(var += Δ)` + Beta-bandit `learn_from_episode`. Honest gap: first-order scalar do-calculus, NOT a full SCM with confounder adjustment.

**Plan**
- Spec live state → causal context (map ECS components into the `HashMap<String,f64>` `counterfactual_query` consumes; canonical var names). `[causal.rs, engine_bridge ecs.inspect]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide counterfactual validation protocol (predicted Δ vs forked Avian re-run: snapshot → apply `do()` as ECS edit → step N → diff). `[Avian, symbolic]` `[dep: physics determinism]` `[effort: M]` `[status: research]` — blocked by contention #2.
- Design SCM upgrade beyond first-order (confounder/back-door, multi-cause edges, order-2 terms). `[causal.rs]` `[dep: none]` `[effort: M]` `[status: research]`
- Spec `counterfactual`/`intervene` MCP verb + Rune/Luau binding. `[mcp, mcp-server, scripting]` `[dep: live-state]` `[effort: S]` `[status: new]` — gated by bridge accept.

**Make**
- Un-gate `realism-symbolic` (or its own `causal` feature); make finite-difference fallback first-class. `[Cargo.toml, symbolic]` `[dep: none]` `[effort: S]` `[status: extend]`
- Implement `CausalContext::from_world(entity)`. `[engine_bridge, causal.rs]` `[dep: live-state spec]` `[effort: M]` `[status: new]`
- Implement `validate_counterfactual()` (forked Avian re-run diff vs predicted) + prediction-vs-actual rows. `[Avian, symbolic, data]` `[dep: determinism, protocol]` `[effort: L]` `[status: research]`
- Wire `learn_from_episode` to `run_experiment`/`compare_runs` outcomes. `[symbolic, scenarios, data]` `[dep: validation]` `[effort: M]` `[status: extend]`
- Implement `intervene`/`counterfactual` MCP verb + shared Rune/Luau binding. `[mcp-server, scripting]` `[dep: verb spec, bridge accept]` `[effort: M]` `[status: new]`

#### Way 37 — Explicit knowledge/ontology (class schema + place-ontology give meaning)
**Plan**
- Audit + reconcile the two systems (is class_schema generated FROM `OntologyTree`, or does the tree ingest class_schema? pick one source). `[class_schema, ontology.rs]` `[dep: none]` `[effort: S]` `[status: research]` — resolved ontology persists in worlddb.
- Spec place-ontology nodes (47 places → 6 Universes as first-class `OntologyNode`s). `[ontology.rs, worlddb]` `[dep: reconciliation]` `[effort: M]` `[status: extend]`
- Design property-provenance (each `PropertySchema` declares dimension + unit → joins Way 40). `[ontology.rs, units.rs]` `[dep: Way 40 Dimension]` `[effort: S]` `[status: new]`

**Make**
- Implement ontology↔class_schema bridge (loader or codegen) so create_instance + tree agree. `[instance_create, class_schema, ontology.rs]` `[dep: reconciliation]` `[effort: M]` `[status: extend]`
- Add dimension/unit fields to `PropertySchema`; populate core classes (BasePart.size, Mass, Velocity, Force). `[ontology.rs]` `[dep: provenance]` `[effort: S]` `[status: extend]`
- Persist resolved ontology + place-ontology to worlddb; surface ontology path + typed props in Properties. `[worlddb, Slint Properties]` `[dep: place spec]` `[effort: M]` `[status: extend]` — keep one StudioState.
- Expose ontology queries (`class_of`/`ancestors`/`properties_of`/`places_in_universe`) over MCP + shared runtimes, reusing spatial-llm NL→query. `[mcp-server, spatial-llm, scripting]` `[dep: bridge]` `[effort: M]` `[status: extend]`

#### Way 38 — Falsifiable (predictions checked against physics — an answer to induction)
> The moat loop: simulator states a prediction (closed-form structures/materials or CausalModel), checks it against an independent physics run, logs every pass/fail. Honest gap: the second oracle is weak today (deformation is visual; no FEA).

**Plan**
- Define the falsification record schema `(prediction_source, claim, predicted, oracle, observed, residual, verdict)` as Polars/Arrow. `[data, data-store]` `[dep: none]` `[effort: S]` `[status: new]` — this IS the flywheel ledger.
- Spec prediction sources + oracles (structures → Avian behavior; CausalModel Δ → forked re-run). `[structures, Avian, symbolic]` `[dep: physics determinism]` `[effort: M]` `[status: extend]`
- Scope the FEA oracle honestly (closed-form = prediction; mesh FEA = higher-fidelity oracle; sequence LAST). `[NEW FEA]` `[dep: none]` `[effort: XL]` `[status: research]`
- Design tolerance/verdict policy (per-quantity residual thresholds + dimensional sanity pre-check). `[realism, units]` `[dep: Way 40]` `[effort: S]` `[status: new]`

**Make**
- Implement a `Prediction`/`Oracle` trait pair; wire closed-form structures as Predictions. `[structures, materials]` `[dep: spec]` `[effort: M]` `[status: extend]`
- Implement the Avian-behavior oracle (predicted-stable actually stands / predicted-buckling actually collapses). `[Avian, realism]` `[dep: determinism]` `[effort: L]` `[status: new]`
- Implement `log_falsification()` → Polars + a "predictions checked: P/F" panel; add `falsification.*` stream topic. `[data, timeline/history]` `[dep: schema]` `[effort: M]` `[status: extend]`
- Expose `predict_then_check` over MCP + shared runtimes. `[mcp-server, scripting]` `[dep: oracle, bridge]` `[effort: M]` `[status: new]`
- (Research, last) FEA mesh oracle validated against closed-form analytic cases. `[NEW FEA, structures]` `[dep: trait]` `[effort: XL]` `[status: research]` — replaces visual-only `deformation`.

#### Way 39 — Compositional generalization (laws + parts compose into never-seen scenes)
**Plan**
- Define the compositional contract (scene = graph of typed parts + bonds + material assignments; every law/oracle operates generically). `[class_schema, Avian joints, realism]` `[dep: Way 37 ontology]` `[effort: M]` `[status: extend]` — bonds are the missing first-class noun.
- Spec a held-out generalization test set (part×material×joint combos absent from any authored Space). `[scenarios, data]` `[dep: contract]` `[effort: M]` `[status: new]` — the gen-loop evaluator on machine candidates.
- Design law-composition rule (shared variables via `symbolic/resolver.rs` synonyms; effects chain across the causal graph). `[causal.rs, resolver.rs]` `[dep: Way 36 SCM]` `[effort: M]` `[status: extend]`

**Make**
- Implement first-class `Bond`/connection set (weld/fastener/truss → Avian joints + parry) via create_instance. `[instance_create, xpbd_joints]` `[dep: contract]` `[effort: L]` `[status: new]`
- Implement a generic scene evaluator (any part+bond+material graph → Avian + structures + CausalModel, zero scene-specific branches). `[Avian, realism, symbolic]` `[dep: bond model, Way 38 traits]` `[effort: L]` `[status: extend]`
- Implement headless combinatorial generator (spawn held-out scenes via cad+mesh-edit + create_instance, evaluate, log). `[cad, mesh-edit, scenarios, data]` `[dep: test spec, evaluator]` `[effort: L]` `[status: new]` — surrogate pre-screen gates which combos get full eval.
- Add CI/MCP "generalization sweep" verb (run held-out set, report pass rate) via shared runtimes. `[mcp-server, scripting]` `[dep: generator, bridge]` `[effort: M]` `[status: new]`

#### Way 40 — Dimensional consistency (unit/dimension checks catch nonsense)
> **Critical finding:** `units.rs` is length-only — no Dimension type, no Quantity<D>, no dimensional algebra. Today the engine catches "wrong length unit" but cannot catch "added a force to a velocity."

**Plan**
- Spec the dimension model (7-base SI vector `[L,M,T,I,Θ,N,J]` + `Quantity { value_si, dim }`; `Unit` becomes one instance). `[units.rs]` `[dep: none]` `[effort: M]` `[status: new]`
- Spec where checks fire (PropertySchema writes, CausalModel/structures formula eval, script set values). `[units, symbolic, class_schema, scripting]` `[dep: model]` `[effort: M]` `[status: new]` — hard pre-filter for Way 38 verdict + Way 36 `do()`.
- Design gravity-boundary fix as first consumer (route gravity through dimensioned conversion accel=L·T⁻²; collapse duplicate sync). `[runtime/physics.rs, eustress-networking, units]` `[dep: model]` `[effort: S]` `[status: extend]` — resolves contention #5/#6.

**Make**
- Implement `Dimension` + `Quantity` (× / divide add/subtract exponents; +/- require equal dims; powi). `[units.rs]` `[dep: model spec]` `[effort: M]` `[status: new]`
- Annotate realism law strings + structures/materials outputs with dimensions; `check_dimensions()` before eval. `[causal.rs, structures, materials]` `[dep: Quantity]` `[effort: M]` `[status: extend]` — dim-mismatch edge rejected at `register_law`.
- Enforce dimension checks on PropertySchema writes + Rune/Luau `set` (nonsense → typed error). `[ontology.rs, scripting, mcp-server]` `[dep: Quantity, Way 37]` `[effort: M]` `[status: new]`
- Fix the gravity boundary (single dimensioned conversion + one sync system; m/s² round-trip regression). `[runtime/physics.rs, eustress-networking, units]` `[dep: impl]` `[effort: S]` `[status: extend]`

### GROUP 9 — Ingest-and-surpass

> **Verified:** radiance Phase-0 render works; `extract_colliders` is TODO. ppisp only `exposure` done. cad Extrude/Revolve/Boolean work; Fillet/Chamfer/Shell/Loft blocked upstream. `mesh_import.rs` detects STL/STEP/OBJ/FBX/PLY but STL→GLB is a scaffold; Draco only detected. `geo` does GeoJSON + HGT. No GLB/USD exporter. `roblox-import/materializer.rs` is the canonical ingest→create_instance pattern to mirror.

#### Way 41 — Ingest Marble/World-API output as a seed, then re-derive editable simulatable state
**Plan**
- Spec ingest→re-derive contract (Marble/World API output → instances via `create_instance`, mirroring `roblox-import/materializer.rs`). `[materializer.rs, instance_create]` `[dep: none]` `[effort: S]` `[status: extend]`
- Decide the re-derivation ladder (SplatCloud → proxy mesh → segmented parts → Avian colliders → CAD/mesh-edit handles; which layers authoritative vs cache). `[radiance, mesh-edit, cad, common]` `[dep: contract]` `[effort: M]` `[status: research]`
- Spec World API client + provenance (`IngestSource` instance: source URL, model, seed, license, poses; WorldDb-authoritative). `[worlddb, common]` `[dep: dual-authority]` `[effort: S]` `[status: new]`

**Make**
- Build fetch client (pull output to active Space root via `set_space_asset_root` so clouds resolve under the per-Space FileAssetReader). `[radiance, space_asset_source.rs]` `[dep: FileAssetReader cleared]` `[effort: M]` `[status: new]`
- Implement `IngestSource` + WorldDb persistence; surface in Properties. `[worlddb, Properties]` `[dep: authority]` `[effort: M]` `[status: extend]`
- Implement scene segmentation (classify extracted geometry → named editable parts via create_instance). `[instance_create, mesh-edit]` `[dep: GS→mesh (Way 43), segmentation]` `[effort: L]` `[status: research]`
- Wire MCP `ingest_world` (fetch → re-derive → place) via shared host. `[mcp, mcp-server, engine_bridge]` `[dep: bridge fix]` `[effort: S]` `[status: extend]`

#### Way 42 — Vendor-agnostic generation backends (Marble/TRELLIS/Hunyuan swappable)
**Plan**
- Define a `GenerationBackend` trait (text/image → {splat|mesh|volume}; capability flags; async job+poll; cost/latency hints). `[radiance, common]` `[dep: none]` `[effort: S]` `[status: new]`
- Spec a normalized `GeneratedAsset` envelope (format, units, up-axis, scale, bounds, pose, license, seed) reusing DUS. `[units, radiance]` `[dep: ingest contract]` `[effort: S]` `[status: extend]`
- Decide hosting matrix (local TRELLIS/Hunyuan via candle/ONNX/subprocess vs remote Marble/World API) + config registry + secrets. `[common, identity]` `[dep: trait]` `[effort: M]` `[status: research]`

**Make**
- Implement the trait + `BackendRegistry` resource (default remote). `[radiance, common]` `[dep: trait spec]` `[effort: M]` `[status: new]`
- Implement first remote adapter (Marble/World API) → `GeneratedAsset`. `[radiance]` `[dep: Way 41 client, registry]` `[effort: M]` `[status: new]`
- Implement one local mesh adapter (TRELLIS/Hunyuan via subprocess/ONNX) to prove swappability. `[radiance, texture-gen]` `[dep: registry, hosting]` `[effort: L]` `[status: research]`
- Add MCP `generate_asset`/`list_generation_backends` via shared host. `[mcp, mcp-server, scripting]` `[dep: bridge fix, registry]` `[effort: S]` `[status: extend]`

#### Way 43 — Re-simulate generated geometry with real physics
**Plan**
- Spec the splat→collider pipeline (decide surface-extraction approach — the "doesn't exist in Rust today" blocker). `[radiance/collider.rs, mesh-edit, cad]` `[dep: none]` `[effort: M]` `[status: research]`
- Spec physics-grounding pass (assign mass/material/restitution from re-derived material via properties.rs; settle/stability check). `[realism/materials, Avian]` `[dep: collider pipeline]` `[effort: M]` `[status: extend]`
- Spec determinism prerequisites (fixed-dt, SubstepCount, seeded RNG, one gravity-sync). `[runtime/physics.rs, main.rs]` `[dep: none]` `[effort: M]` `[status: extend]`

**Make**
- Implement `extract_colliders(cloud, strategy) -> Compound` (the collider.rs:32 TODO). `[radiance/collider.rs, mesh-edit, cad]` `[dep: extraction research]` `[effort: L]` `[status: new]`
- Collapse the two gravity-sync systems + route through units. `[runtime, eustress-networking, units]` `[dep: none]` `[effort: S]` `[status: extend]`
- Pin physics determinism (fixed schedule + dt + SubstepCount + GlobalRngSeed). `[main.rs, runtime]` `[dep: gravity-sync]` `[effort: M]` `[status: extend]`
- Implement material/mass grounding system (re-derived material → density/friction/restitution → Avian). `[realism/materials, Avian]` `[dep: extract, Way 41 segmentation]` `[effort: M]` `[status: extend]`
- Add MCP `resimulate`/`stability_report` via shared host. `[mcp, mcp-server, scripting]` `[dep: bridge fix, grounding]` `[effort: S]` `[status: extend]` — closed-form structures (not the deformation approximation) is the only true structural fitness until FEA.

#### Way 44 — Open interchange (SPZ/PLY/glTF/USD import + export)
**Plan**
- Audit + spec the import matrix (glTF/GLB w/ Draco detect-only; PLY/STL/STEP/OBJ/FBX detect but STL→GLB scaffold; GeoJSON/HGT; splat PLY/gcloud). Targets: SPZ, USD/USDZ, real STL/OBJ→GLB, Draco decode. `[mesh_import.rs, draco_decoder.rs, radiance, geo]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec export (absent today): glTF/GLB scene, PLY/SPZ splat, USD for DCC handoff; lossless vs lossy. `[serialization/scene.rs, radiance]` `[dep: none]` `[effort: M]` `[status: new]`
- Decide one conversion hub crate + dep set (gltf-json, stl_io, USD via openusd/usd-rs/subprocess, SPZ codec) + unit/up-axis normalization. `[engine, units]` `[dep: specs]` `[effort: M]` `[status: research]`

**Make**
- Finish scaffolded STL/OBJ→GLB (mesh_import.rs:269,321). `[mesh_import.rs]` `[dep: dep-set]` `[effort: S]` `[status: extend]`
- Implement Draco decode. `[draco_decoder.rs]` `[dep: dep-set]` `[effort: M]` `[status: extend]`
- Implement SPZ import/export behind radiance (normalize via GeneratedAsset). `[radiance, units]` `[dep: Way 42 envelope]` `[effort: M]` `[status: new]`
- Implement glTF/GLB exporter reusing scene.rs PropertyAccess → gltf-json. `[scene.rs]` `[dep: export spec]` `[effort: L]` `[status: new]`
- Implement USD/USDZ import + export (prims ↔ create_instance). `[engine, instance_create]` `[dep: USD binding]` `[effort: L]` `[status: research]`
- Add MCP `import_asset`/`export_scene` via shared host. `[mcp, mcp-server, scripting]` `[dep: bridge fix]` `[effort: S]` `[status: extend]` — all importers place via create_instance + WorldDb-authoritative.

#### Way 45 — Professional-fidelity lineage (CAD/truck) for architects/engineers/filmmakers
**Plan**
- Spec ingested-blob → parametric CAD lift (segmented walls/slabs → editable truck features; planar→Extrude, blocky→Boolean). `[cad/feature.rs, eval.rs, mesh-edit]` `[dep: Way 41 segmentation]` `[effort: M]` `[status: extend]`
- Spec CAD constraint/lineage layer (dimensions + geometric/clearance/code constraints via `realism/symbolic`). `[realism/symbolic, cad]` `[dep: lift spec]` `[effort: L]` `[status: extend]`
- Unblock the pro feature set (Fillet/Chamfer/Shell/Loft blocked upstream — vendor-fix vs upstream-wait vs in-house; pair w/ bevel/loop-cut). `[cad, mesh-edit]` `[dep: none]` `[effort: L]` `[status: research]`
- Spec FEA upgrade path (true mesh solver extending realism, replacing visual deformation + per-member closed-form). `[realism (FEA), structures]` `[dep: none]` `[effort: XL]` `[status: research]`

**Make**
- Implement planar/blocky→truck-feature fit (Sketch+Extrude/Boolean from segmented geometry → editable feature tree). `[cad/eval.rs, sketch.rs, instance_create]` `[dep: Way 41 segmentation]` `[effort: L]` `[status: extend]`
- Wire `realism/symbolic` as the CAD constraint engine; expose in Properties. `[symbolic, cad, Properties]` `[dep: fit impl]` `[effort: L]` `[status: extend]`
- Land Fillet/Chamfer/Shell + mesh-edit bevel/loop-cut. `[cad/eval.rs, mesh-edit/ops.rs]` `[dep: unblock decision]` `[effort: L]` `[status: research]`
- Build FEA module extending realism (mesh assembly + linear-static solve); keep deformation flagged visual-only. `[realism (new FEA), structures]` `[dep: none]` `[effort: XL]` `[status: research]`
- Add MCP `cad_feature_add`/`solve_constraints`/`run_fea` via shared host. `[mcp, mcp-server, scripting]` `[dep: bridge fix, FEA/solver]` `[effort: M]` `[status: extend]` — persist feature trees + constraints WorldDb-authoritative.

### GROUP 10 — Structural moat

#### Way 46 — Own the linchpin (render + plan derive from simulation)
**Plan**
- Write the "State-is-Authoritative" contract (single read/write path through worlddb + create_instance; TOML legacy import-only). `[worlddb, instance_create]` `[dep: none]` `[effort: S]` `[status: extend]`
- Spec render-derives-from-state seam (radiance consumes the same state snapshot; `extract_colliders` signature). `[radiance]` `[dep: contract]` `[effort: S]` `[status: extend]`
- Decide determinism config (SubstepCount, fixed-dt, one GlobalRngSeed). `[common (physics), engine]` `[dep: none]` `[effort: M]` `[status: extend]`
- Resolve dual-authority (world-db default-on; delete/quarantine TOML write path). `[engine, worlddb]` `[dep: contract]` `[effort: S]` `[status: extend]`

**Make**
- Land K2 codec + entities-partition load (Fjall single store, not a mirror). `[worlddb, common]` `[dep: contract]` `[effort: M]` `[status: extend]`
- Implement `radiance::extract_colliders` (2DGS/GOF → decimate via mesh-edit → convex-decomp / truck CSG-fit → Avian). `[radiance, mesh-edit, cad]` `[dep: seam spec]` `[effort: L]` `[status: research]` — the ingest-and-surpass linchpin proof.
- Collapse duplicate gravity-sync + route through units. `[runtime, common]` `[dep: determinism]` `[effort: S]` `[status: extend]`
- Re-verify + fix engine_bridge TCP accept on 0.19 (Startup bind / Update drain). `[engine_bridge]` `[dep: none]` `[effort: S]` `[status: extend]` — **gate all Way 50 work behind this.**

#### Way 47 — One substrate, three projections (state → observations, dynamics, affordances)
**Plan**
- Define three projection traits over one state snapshot: `Observe`, `Step`, `Affordances`. `[common (new module)]` `[dep: Way 46 contract]` `[effort: M]` `[status: new]`
- Spec observation projection (capture_viewport + ai_camera + raycast/measure as sensor set; typed record RGB/depth/seg/contacts). `[engine, mcp]` `[dep: traits]` `[effort: S]` `[status: extend]`
- Spec affordance projection (derive actions from class_schema + scripting/instance.rs + selection). `[scripting, class_schema]` `[dep: traits]` `[effort: M]` `[status: new]` — affordances expressed once in the shared API.
- Spec dynamics projection determinism + step-result schema. `[common (realism+physics)]` `[dep: Way 46 determinism]` `[effort: S]` `[status: extend]`

**Make**
- Implement `Observe` (headless observation system → Polars rows). `[engine, data]` `[dep: obs spec]` `[effort: M]` `[status: extend]`
- Implement `Affordances` query (selection → legal actions vs class_schema + shared API). `[scripting, common]` `[dep: affordance spec]` `[effort: M]` `[status: new]` — same set via both runtimes.
- Wrap the three as MCP verbs (`observe`/`step`/`affordances`) over engine_bridge. `[mcp-server, engine_bridge]` `[dep: bridge fix (Way 46)]` `[effort: M]` `[status: extend]`
- Add a single `Substrate` facade returning all three from one state handle. `[common]` `[dep: three projections]` `[effort: S]` `[status: new]`

#### Way 48 — Compounding synthetic-data moat
**Plan**
- Define the corpus schema (geometry hash, material, joints/bonds, Avian stability, closed-form margins, code flags, style embedding id, pass/fail). `[data, data-store]` `[dep: Way 46 logging]` `[effort: S]` `[status: new]`
- Spec embed-into-latent (which features → embedvec vectors; style-latent layout). `[embedvec]` `[dep: schema]` `[effort: M]` `[status: extend]`
- Spec surrogate pre-screen (cheap predictor gating full Avian+FEA eval). `[data, realism]` `[dep: schema]` `[effort: M]` `[status: research]`
- Decide provenance + WorldDb authority (Fjall changestream = append log; Polars = analytics projection). `[worlddb, data]` `[dep: Way 46 authority]` `[effort: S]` `[status: extend]`

**Make**
- Implement candidate-logger (every eval, success AND failure → Polars keyed to changestream). `[data, worlddb, common]` `[dep: schema]` `[effort: M]` `[status: new]` — the loop's persist+log stage.
- Implement style-embedding ingestion (design → embedvec under style ontology for k-NN). `[embedvec, spatial-llm]` `[dep: latent]` `[effort: M]` `[status: extend]`
- Train + wire surrogate (`predict_feasibility(candidate)` pre-screen). `[data, realism]` `[dep: surrogate spec + corpus]` `[effort: L]` `[status: research]`
- Build flywheel harness (headless batch runner mass-generates → evaluates → logs → reports corpus growth). `[engine (bin), data]` `[dep: logger]` `[effort: M]` `[status: new]` — drives generation through the shared API.

#### Way 49 — Trillion-dollar surface (digital twins / robotics / AV / AEC / drug discovery)
**Plan**
- Write the vertical-adapter pattern (each vertical = domain class_schema + units profile + scoring/compliance rule pack + sensor set, all on Way 47 projections — no fork). `[class_schema/units, docs]` `[dep: Way 47]` `[effort: S]` `[status: new]`
- Spec the AEC wedge first (closed-form structural fitness + code-compliance via symbolic). `[structures, symbolic]` `[dep: pattern]` `[effort: M]` `[status: extend]` — AEC is the loop's first tenant.
- Spec robotics/AV adapter (observation → sensor stream; affordance → robot action set). `[engine, mcp]` `[dep: Way 47]` `[effort: M]` `[status: research]`
- Scope FEA need per vertical (AEC depth; drug-discovery deferred). `[realism]` `[dep: AEC spec]` `[effort: S]` `[status: research]`

**Make**
- Implement AEC structural-fitness evaluator (structures + materials → margins + pass/fail). `[structures, materials]` `[dep: AEC spec]` `[effort: M]` `[status: extend]`
- Build code-compliance rule pack v1 (clearances, span/depth ratios, load cases) as symbolic constraints. `[symbolic]` `[dep: AEC spec]` `[effort: L]` `[status: new]` — extend the existing solver, don't rebuild.
- Implement a true FEA mesh solver extending realism (linear-elastic stiffness assembly first). `[realism (new fea)]` `[dep: AEC scope]` `[effort: XL]` `[status: research]`
- Ship one robotics/AV demo adapter (stream observation + drive one action set over MCP, no fork). `[mcp-server, engine]` `[dep: robotics spec + bridge]` `[effort: M]` `[status: research]`

#### Way 50 — AI-native from the ground up (agent loop / MCP / live scripting native)
**Plan**
- Write the agent-loop contract (generate → step/run_simulation → score → select/mutate → persist, all as MCP verbs over engine_bridge). `[mcp-server, engine_bridge]` `[dep: Way 46 bridge]` `[effort: S]` `[status: extend]` — no policy gradient; fitness from the sim.
- Spec the unified behavior/host API (Luau runtime ~5102 LOC + Rune ~3745 LOC converge to ONE binding layer). `[common::scripting]` `[dep: none]` `[effort: M]` `[status: extend]`
- Spec new structural/optimizer MCP verbs (`evaluate_structure`/`optimize`/`score_style`). `[mcp-server]` `[dep: Way 49 evaluator]` `[effort: S]` `[status: new]`
- Decide the optimizer-family interface (topology-opt, form-finding, CMA-ES, gradient, constraint-solve behind one `Optimizer` trait). `[common (new module)]` `[dep: loop contract]` `[effort: M]` `[status: new]`

**Make**
- Implement the shared behavior API + reroute both VMs through it. `[common::scripting]` `[dep: spec]` `[effort: L]` `[status: extend]` — fixes the live divergence risk.
- Fix duplicate StudioState (single source the agent loop reads). `[engine (Slint UI)]` `[dep: none]` `[effort: S]` `[status: extend]`
- Implement the `Optimizer` trait + first strategy (CMA-ES over topology + joint-type + material) calling in-sim fitness. `[common (optimizer), realism]` `[dep: interface + AEC evaluator]` `[effort: L]` `[status: new]` — gradient/differentiable path deferred (no adjoint today).
- Add new MCP verbs + an end-to-end loop bin (generate→evaluate→score→persist headless). `[mcp-server, engine (bin)]` `[dep: optimizer + evaluator + bridge]` `[effort: M]` `[status: new]`
- Harden engine_bridge for sustained sessions (lifecycle, backpressure, error surfacing). `[engine_bridge]` `[dep: Way 46 accept fix]` `[effort: M]` `[status: extend]`

---

## 4. Engine contention audit & resolutions

> Verified across the codebase. Each item: what's wrong, severity, resolution, and the subtasks it **gates** (so nothing bolts onto a broken foundation).

### 4.1 Top blockers (clear first)

| # | Contention | Severity | Resolution | Gates |
|---|---|---|---|---|
| **C1** | **State authority is TOML-disk, not the DB.** `representation.rs:35-37` honors `BinaryEcs` only after K2 codec + entities-partition load; `world_db_plugin.rs:102-117` seeds tree from disk + reconcile-on-open. `world-db` feature-gated OFF default (`main.rs:686,699`). | Critical | Land K2 codec + entities-partition load; make `world-db` default-on; demote TOML to import/export. | Ways 1, 7, 9, 11, 14, 16, 21, 28, 31, 46, 48; all A-loop persistence |
| **C2** | **Physics step has no determinism config** (`main.rs:609`): no `SubstepCount`/`SolverConfig`/fixed-dt; virtual-time clamp is frame-rate-dependent; RNG unseeded across 9 sites. | Critical | Fixed schedule + fixed dt + `SubstepCount` pin + one `GlobalRngSeed`; determinism integration test. | Ways 3, 6, 8 (counterfactual), 15, 16, 17, 26, 29, 32, 36, 38, 43 |
| **C3** | **engine_bridge TCP accept must be re-verified live post-0.19.** Code is structurally OK (`server.rs:65-90`, `mod.rs:83-88`); the 500ms bounded `recv_timeout` may race schedule init. | High | Startup self-test (bridge pings itself, logs loudly); verify live; move bind off the 500ms timeout if it races. | Every MCP-facing subtask: Ways 5, 18, 25, 36–39, 41–45, 47, 50; A1/A7 |
| **C4** | **Two parallel scripting stacks, no shared behavior API.** Luau `runtime.rs` ~5102 LOC / Rune `rune_ecs_module.rs` ~3745 LOC bind ECS independently. | High | One host-side behavior/ECS-binding facade both VMs call; VMs become thin frontends. | Ways 13, 19, 27, 47, 50; "Luau implies Rune" throughout |
| **C5** | **Gravity bypasses the unit boundary.** `main.rs:610` = 9.80665 m/s²; `scene.rs:219` default 35.0 "studs/s²"; `runtime/physics.rs:48-50` + `eustress-networking/physics.rs:218-220` both write `Gravity.0 = ws.gravity` with NO conversion. | High | Convert `ws.gravity` through `units` before writing `Gravity`; standardize one default unit. | Ways 3, 4, 6, 15, 26, 30, 40, 43, 46 |
| **C6** | **Two duplicate gravity-sync systems** (`runtime/physics.rs:42` + `eustress-networking/physics.rs:212`) → nondeterministic last-writer-wins. | High | Single canonical system in `common`; both engine + networking schedule it. | Same as C5 |

### 4.2 Secondary

| # | Contention | Severity | Resolution | Gates |
|---|---|---|---|---|
| **C7** | "run_simulation" determinism is the Monte-Carlo scenario tree (`scenarios/engine.rs:156-160,477`), NOT physics. | High | Keep two named sim kinds; build physics-replay separately; don't let MCP "reproducible run" imply physics determinism. | Ways 6, 8, 17 |
| **C8** | **Duplicate StudioState** — historically `slint_ui.rs` + `mod.rs`. **Verified:** only ONE `pub struct StudioState` found (`ui/mod.rs`); `slint_ui.rs:204,597-600` imports it as single source. | Low (verify webview.rs) | Confirm `ui/webview.rs` imports `super::StudioState`; add a compile guard. Treat as verify-then-act. | Ways 7, 14, 18, 23, 28, 37, 50 |
| **C9** | **GS → collider extraction is a TODO scaffold** (`radiance/collider.rs:32`). Splats contribute zero physics/state today. | Critical (for the thesis) | Build the Phase-1 pipeline (2DGS/GOF extract → decimate → CoACD/V-HACD or truck CSG → Avian). | Ways 1, 10, 16, 21, 25, 30, 41, 43, 46 |
| **C10** | **No true FEA / multi-physics coupling.** `realism/deformation` = vertex displacement from a stress-tensor approximation. | Med | Treat deformation as visual-only; build a real FEA module before claiming physically-true stress. | Ways 3, 10, 20, 26, 28, 38, 45, 49 |
| **C11** | **FileAssetReader swaps asset root per active Space** (`space_asset_source.rs`, `file_loader.rs`). External clouds/meshes need an absolute/alternate AssetSource. | Med | Register a second named global AssetSource (engine assets + splat clouds) that doesn't move with the active Space. | Ways 21, 24, 33, 35, 41 |
| **C12** | **WorldDb + GS feature-gated off the default build** (`main.rs:686,699`). | Med | Make `world-db` default-on; decide GS first-class vs research. | C1, C9; Ways 16, 31, 33 |
| **C13** | **Monolithic engine crate** (~10-15 min builds). Timeline split into 3 plugins proves the decomposition lever. | Med | Continue plugin-extraction; pull `soul`/`simulation`/`engine_bridge`/spawners into sibling crates; engine becomes a thin assembler. | Iteration speed for all; Way 33 (wasm carve) |
| **C14** | **light_cull needs 0.19 GPU-clustering re-tune.** | Med | Re-tune clustered-forward binning for wgpu 29. | Ways 10, 22, 31, 32 |
| **C15** | **P2P disabled on 0.19** (bevy_quinnet no 0.19 release). | Low | Restore when quinnet ships 0.19 or swap transport. Off the world-model critical path. | — |

---

## 5. Sequenced master roadmap

> Highest-leverage subtasks pulled into ordered phases. Foundation fixes first so nothing bolts onto a broken base. **🔬 = contains research-grade bets.**

### Phase 0 — Foundation fixes (clear the blockers)
**Milestone:** the substrate is trustworthy and reachable; the agent loop can connect.
- Fix/verify engine_bridge TCP accept live on 0.19 + startup self-test. (C3 → Way 5, 18, 46)
- Collapse the two gravity-sync systems into one + route `ws.gravity` through `units`. (C5/C6 → Way 3, 40)
- Confirm StudioState single-source (verify webview.rs import). (C8 → Way 7, 50)
- Pin Avian determinism (fixed schedule + dt + SubstepCount + GlobalRngSeed) + determinism test. (C2/C7 → Way 6)
- Re-tune light_cull for 0.19. (C14)
> **Clears:** C2, C3, C5, C6, C7, C8, C14. Gate-opener for everything downstream.

### Phase 1 — State & determinism authoritative
**Milestone:** WorldDb is the single source of computable state in the shipped binary; runs replay bit-identically.
- Land K2 codec + entities-partition load; make `world-db` default-on; demote TOML. (C1/C12 → Way 1, 7, 16, 46)
- WorldState DTO + one serializer for bridge/MCP/Properties. (Way 1, 4)
- Persistent causal/audit stream (`mutations.*` from create_instance write-through). (Way 8)
- Dynamic Unit System coverage audit + velocity/mass units + round-trip test. (Way 4, 40)
> **Clears:** C1, C12. **Depends on:** Phase 0.

### Phase 2 — Agent loop operational (POMDP)
**Milestone:** an agent drives, inspects, and learns over MCP in one live session; both runtimes in lockstep.
- Unified shared behavior/host API; reroute Luau + Rune through it + conformance suite. (C4 → Way 19, 47, 50)
- Live bridge verbs (raycast/measure/query/observe/step/affordances); migrate sim/experiment onto the bridge. (Way 5, 18, 47)
- Headless episode runner (sandboxed WorldDb + pinned root + budget) + manifest replay. (Way 17)
- Three projections + `Substrate` facade. (Way 47)
> **Clears:** C4. **Depends on:** Phases 0–1.

### Phase 3 — Representation & Gaussian Splatting 🔬
**Milestone:** splats render as appearance, mesh/CSG/voxel is authoritative state, captured worlds become collidable.
- Dual-channel scene contract (appearance ↔ state) + parent-state/child-visual. (Way 21)
- `radiance::extract_colliders` Tier A (surface extract → decimate → CoACD / truck CSG → Avian). 🔬 (C9 → Way 25, 43, 46)
- Second global AssetSource for external clouds (FileAssetReader fix). (C11 → Way 35, 41)
- Representation capability matrix + conversion graph; reconcile pointcloud module. (Way 23)
> **Clears:** C9, C11. **Depends on:** Phases 0–1.

### Phase 4 — Multi-physics & FEA 🔬
**Milestone:** physically-true structural state, not visual-only; verifiable domains check against known physics.
- Unified `PhysicsSet` coupling contract. (Way 26)
- 1D linear FEA MVP (`realism::fea`) → 2D/3D shells/solids. 🔬 (C10 → Way 3, 26, A3)
- Reclassify deformation as visual-only in the DTO. (Way 3)
- Verification harness + nuclear/optics/thermo/structures golden suites. (Way 29)
- Law cards + dimension/Quantity system. (Way 27, 40)
> **Clears:** C10. **Depends on:** Phases 0–1.

### Phase 5 — Architecture-generation loop (the flagship)
**Milestone:** generate→evaluate→score→optimize runs end-to-end on closed-form fitness, then surrogate + evolutionary.
- Candidate schema + fitness contract + GenerativeArchPlugin. (Way A1)
- Candidate generation (structure grammar + material assigner + bond-graph instantiator). (Way A2)
- Closed-form eval first (works today) → Avian settle-test → FEA confirm. (Way A3, Phase 4)
- Scoring (stability/efficiency/code-compliance/style) + code-compliance rule pack via symbolic. (Way A4, 28, 49)
- Synthetic-data flywheel logging + WorldDb design records + embedvec ingestion. (Way A5, 48, 20)
- Evolutionary/CMA-ES `Optimizer` trait + dispatch. (Way 50, A1)
- 🔬 Style latent + invent/mimic; 🔬 surrogate pre-screen; 🔬 differentiable-sim gradient path (last).
> **Depends on:** Phases 0–4. **First tenant:** AEC.

### Phase 6 — Ingest-and-surpass 🔬
**Milestone:** ingest Marble/World API output and re-derive editable, simulatable, professional-fidelity state.
- Vendor-agnostic `GenerationBackend` trait + `GeneratedAsset` envelope. (Way 42)
- Ingest→re-derive ladder + `IngestSource` provenance + segmentation. 🔬 (Way 41)
- Re-simulate generated geometry (collider + material/mass grounding + stability). (Way 43)
- PPISP front-end completion + PPISP-grounded capture. 🔬 (Way 24, 22)
- Open interchange (SPZ/PLY/glTF import/export; Draco; 🔬 USD). (Way 44)
- 🔬 Parametric CAD lift + symbolic constraints + relightable GS.
> **Depends on:** Phases 3–5. Uses the flywheel to score re-derivation fidelity.

### Phase 7 — Scale & web
**Milestone:** 10M-entity persistence + streaming + GPU cull; consumer-GPU real-time; a web-native viewer.
- Morton default + residency manager + atomic WriteBatch + create-flip. (Way 31)
- GPU-driven cull → indirect draw. 🔬 (Way 31)
- HLOD merged-cell proxies + `.echk` live streaming + impostor atlases. (Way 35)
- Consumer-GPU quality ladder + bindless materials + physics LOD. 🔬 (Way 32)
- 🔬 `eustress-runtime-wasm` carve (forces engine decomposition, C13) + wasm WorldDb backend + GS web playback. (Way 33)
- Open & owned: SPDX headers + cargo-deny + stabilized `.eustress` format + flywheel export. (Way 34)
> **Clears:** C13 (partial). **Depends on:** Phases 1, 3.

### Cross-cutting (any phase, after Phase 1)
- Causal model un-gate + live-state context + 🔬 counterfactual validation. (Way 36, gated by C2)
- Falsifiable predict-then-check loop. (Way 38, gated by C2/C10)
- Compositional generalization sweep + first-class Bond model. (Way 39)
- Ontology↔class_schema reconciliation + place-ontology. (Way 37)
- Versioned worlds (WorldDb diff/branch/revert + git). (Way 9, gated by C1/C6)

---

## 6. Honest status ledger

| Capability | Status | Evidence / note |
|---|---|---|
| Bevy 0.19 / wgpu 29 / Avian 0.7 substrate | **exists** | rigid + xpbd_joints + parry/convex-decomp enabled |
| Closed-form structural law (beams/columns/fatigue/composites) | **exists** | `realism/structures` — analytic, no FEM assembly |
| Material presets (E/ν/density/yield/thermal) | **exists** | `realism/materials/{properties,stress_strain,deformation,fracture}` |
| Realism breadth (fluids/plasma/nuclear/thermocycles/chemistry/electrical/quantum/laws/numerics/gpu) | **exists** | far deeper than the brief implied |
| Symbolic/causal solver (graph + derivatives + do-calculus + bandit) | **exists (feature-gated)** | `symbolic/causal.rs` — scalar, first-order |
| Dual scripting runtimes (Luau live + Rune) | **exists** | shared *types* in `scripting/mod.rs`; behavior API NOT shared |
| MCP tool surface + engine_bridge | **exists** | bridge accept needs live re-verify post-0.19 (C3) |
| WorldDb (Fjall, rkyv cores + TOML tree, `.eustress` dir) | **exists (not authoritative yet)** | K2 codec + entities-partition load + default-on pending (C1/C12) |
| Gaussian Splatting render (Phase 0) | **exists** | `radiance` renders PLY/gcloud/glTF |
| PPISP exposure transform + adjoint | **exists** | vignette/color/CRF scaffolded |
| Dynamic Unit System (length, meter-native) | **exists (length-only)** | no Dimension/Quantity type yet (Way 40) |
| Morton keys / HLOD / render_cascade / residency | **exists (not default / partial)** | Morton not default; `.echk` unwired |
| Determinism config (physics) | **extend** | only the Monte-Carlo sampler is reproducible (C2/C7) |
| Shared behavior/host API (both VMs) | **extend** | collapse ~8.8k LOC of duplicated bindings (C4) |
| WorldDb-authoritative state in shipped binary | **extend** | K2 + default-on (C1) |
| Unit boundary at gravity + velocity/mass units | **extend** | gravity bypasses the boundary (C5/C6); units length-only |
| CAD features beyond Extrude (Revolve/Sweep/Boolean work; Fillet/Chamfer/Shell/Loft) | **extend / research** | Fillet/Chamfer/Shell/Loft blocked on truck upstream |
| mesh-edit bevel/loop-cut + topology validation | **extend / new** | extrude/inset shipped; validators are net-new |
| Candidate schema + generative loop orchestrator | **new** | Phase 5 flagship |
| Synthetic-data flywheel (corpus schema + auto-emit + logger) | **new / extend** | `data` exists; labeled outputs + emit pipeline net-new |
| Labeled G-buffer (depth + instance/semantic segmentation) | **new** | only RGB today |
| First-class Bond/connection model | **new** | Avian joints exist; bond graph is net-new |
| GS → collider extraction | **research** | `collider.rs:32` TODO; surface-extraction missing in Rust (C9) |
| FEA mesh solver (1D buildable; 2D/3D shells/solids) | **new (1D) / research (2D/3D)** | replaces visual-only deformation (C10) |
| Differentiable structural sim (gradient optimizer family) | **research** | needs adjoint/autodiff layer |
| Trained AEC style encoder + corpus | **research** | embedvec stores vectors; corpus + encoder unbuilt |
| Surrogate predictor (pre-screen) | **research** | needs corpus volume first |
| Topology optimization (SIMP/level-set) | **research** | net-new; depends on a load evaluator/FEA |
| Code-compliance rule pack | **research** | author as symbolic constraints |
| Counterfactual validation against forked sim | **research** | gated by C2 determinism |
| Web-native engine (wasm runtime) | **research** | Slint desktop-only; needs decomposition (C13) + wasm WorldDb backend |
| Inverse-render relightable GS (per-splat PBR) | **research** | noisy Gaussian normals → weak specular |
| USD/USDZ interchange | **research** | no binding chosen yet |
| Robotics/AV vertical adapter | **research** | observation + affordance projections feed it |

---

*Crates root: `eustress/crates`. This roadmap is composed from verified sweeps; research-grade items are flagged in-line and in §6. Build order is law-fixed: Foundation → State/Determinism → Agent Loop → Representation/GS → Multi-physics/FEA → Arch-Gen Loop → Ingest-and-Surpass → Scale/Web.*
