# Using EustressEngine as a Sparse Submodule

External AI models (Vortex, research agents, external solvers) that need
EustressEngine as their simulation substrate do not need the full monorepo.
Only the axiomatic core is required.

## What you need

| Crate | Why |
|-------|-----|
| `eustress-common` | `WorldState` trait, `SalienceFilter`, `GoalTree`, `MemoryTierController`, `CausalModel`, `SymbolResolver`, `ArcEpisodeRecord`, Iggy delta types |
| `eustress-embedvec` | HNSW semantic memory, `RocksOntologyIndex` |
| `eustress/.patches/iggy_common-0.9.0/` | Windows build fix for `iggy_common` |
| `eustress/Cargo.toml` | Workspace definition + `[patch.crates-io]` |

## What you do NOT need

- `eustress-engine` (Bevy 3D editor — heavy Bevy dep)
- `eustress-client` / `eustress-server` (multiplayer runtime)
- `eustress-web` / `eustress-backend` (Leptos + Axum web layer)
- `eustress-bliss` (Ethereum integration)
- `eustress-geo` (GeoTIFF / geospatial)
- `eustress-workshop` (IoT tool tracking)
- `eustress-networking` / `eustress-forge` (orchestration)
- `eustress-mcp` (MCP server)
- `texture-gen` (procedural textures)
- `eustress-player-mobile`

## Setup — Git Submodule with Sparse Checkout

```bash
# 1. Add EustressEngine as a submodule (shallow clone)
git submodule add --depth 1 \
  https://github.com/WeaveITMeta/EustressEngine \
  deps/EustressEngine

# 2. Enable sparse checkout for the submodule
git -C deps/EustressEngine sparse-checkout init --cone

# 3. Set the paths you need (cone mode: directory prefixes)
git -C deps/EustressEngine sparse-checkout set \
  eustress/Cargo.toml \
  eustress/crates/common \
  eustress/crates/embedvec \
  eustress/.patches/iggy_common-0.9.0

# 4. Verify
git -C deps/EustressEngine sparse-checkout list
```

## Cargo.toml in Your Solver

```toml
[dependencies]
eustress-common = { path = "deps/EustressEngine/eustress/crates/common", features = [
  "iggy-streaming",       # AgentCommand, AgentObservation, Iggy topics
  "realism-symbolic",     # CausalModel, SymbolResolver (requires Symbolica)
] }
eustress-embedvec = { path = "deps/EustressEngine/eustress/crates/embedvec" }

# Pull in the iggy_common Windows fix
[patch.crates-io]
iggy_common = { path = "deps/EustressEngine/eustress/.patches/iggy_common-0.9.0" }
```

## Minimum feature set (no Bevy, no physics, no Iggy)

For pure offline reasoning (tests, CI, no live Iggy connection):

```toml
eustress-common = { path = "...", default-features = false }
# No features needed — WorldState, SalienceFilter, GoalTree, CausalModel,
# MemoryTierController, Sandbox, HypothesisTree are all feature-free.
```

## What the solver implements

Everything in this table lives **outside** EustressEngine, in the solver:

| Item | Trait / Type in Eustress | Solver impl |
|------|--------------------------|-------------|
| 2D ARC grids | `WorldState` | `Grid2D : WorldState` |
| 3D scenes (if headless) | `WorldState` | `Scene3D : WorldState` |
| DSL primitives | `WorldState::Action` | `DSLOp`, `WorkshopAction` |
| Hypothesis generation | caller of `CausalGraph::suggest_hypotheses` | Vortex search loop |
| `solve<W>()` function | uses `Sandbox` + `HypothesisTree` | Vortex core |
| `CausalGraph::suggest_hypotheses` | stub in `causal.rs` | Vortex provides impl |
| `CausalGraph::integrate_episode` | stub in `causal.rs` | Vortex provides impl |

## The Axiomatic Boundary

EustressEngine guarantees:

1. `SceneDelta` + Iggy = the only observable ground truth
2. `ArcEpisodeRecord.efficiency_ratio` + `final_score` = the only learning signals
3. `SimulationMode::StepN` = deterministic, repeatable stepping
4. `SalienceFilter` = well-defined signal/noise boundary
5. `GoalTree` = goals are always explicit and first-class
6. `CausalModel` = domain-agnostic formula → symbolic derivative → effect chain

Everything above this boundary is the solver's domain.
Everything below is the engine's contract.
