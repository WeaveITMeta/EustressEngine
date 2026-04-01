# Eustress Functions — Spatial Intelligence DSL

## The Vision

A 12-stage function ontology turning EustressEngine into a programmable spatial
intelligence platform. Every stage — from entity creation through knowledge
representation to autonomous reasoning — is composable Rust core functions
exposed to Rune Script without recompilation.

## Honest Status

### What Exists (Rust Infrastructure)

| Crate | Provides |
|---|---|
| `engine/src/soul/rune_ecs_module.rs` | Rune VM, Vector3/Color3/CFrame, raycasting, Instance, Tween, input, sound, DataStore, HTTP, tags (~2300 lines) |
| `engine/src/soul/parallel_execution.rs` | Rayon parallel script execution + VM pooling |
| `common/src/realism/scripting/api.rs` | Physics: gas law, kinetic energy, drag, buoyancy, entropy, von Mises |
| `common/src/realism/scripting/bindings.rs` | ScriptContext, EntityData cache, PropertyUpdate queue |
| `common/src/realism/scripting/viga_api.rs` | VIGA scene building (spawn_cube/sphere, set_color/material) — **stubs only** |
| `embedvec/` | OntologyTree, OntologyIndex, SpatialContextEmbedder, KNN search, RocksDB persistence |
| `stream/` | EustressStream (zero-copy, multi-subscriber, ring buffer) |
| `common/src/simulation/` | SimulationClock, WatchPointRegistry, BreakPointRegistry |
| `common/src/change_queue.rs` | ECS mutation deltas over EustressStream |
| `mcp/` | HTTP/WebSocket server, EEP export protocol, stream-backed routing |

### What Does NOT Exist (The DSL Layer)

**Not exposed to Rune today:**
- Ontology (classify, relate, engineer, traverse)
- Vector search (nearest, compose, spatial_query)
- Knowledge web (model, weave, graph, link)
- Data pipeline (cleanse, transform, extract, represent)
- Temporal (timestamp, diff, evolve, predict)
- Statistical reasoning (probability, confidence, bayes)
- Planning loop (plan, simulate, decide, execute, observe)
- Meta/introspection (introspect, profile, optimize)

**Stubs returning placeholder values:**
- `get_voltage()`, `get_soc()`, `get_temperature()`, `get_dendrite_risk()` → return `0.0`
- `get_sim_value()`, `set_sim_value()` → no-op
- VIGA `soul::spawn_*` → `println!` only, no ECS writes

## Architecture

```
┌──────────────────────────────────────────────┐
│              Rune Script DSL                 │
│  use eustress::functions::genesis::*;        │
│  use eustress::functions::proximity::*;      │
├──────────────────────────────────────────────┤
│         eustress-functions crate             │
│  12 Rune modules + thread-local bridges      │
│  Feature flags per stage                     │
├──────────┬──────────┬────────┬───────────────┤
│ rune_ecs │ embedvec │ stream │ parameters    │
│ module   │ crate    │ crate  │ (common)      │
├──────────┴──────────┴────────┴───────────────┤
│            Bevy ECS Runtime                  │
└──────────────────────────────────────────────┘
```

### Thread-Local Bridge Pattern (Established)

Already used for `ScriptSpatialQuery` and `ECSBindings`:
1. Before Rune execution: install bridges into thread-local storage
2. During execution: Rune functions read from thread-locals
3. After execution: clear bridges

Each new stage adds its own bridge (OntologyBridge, EmbedvecBridge, etc.).

## Crate: `eustress-functions`

New crate at `crates/functions/` with feature flags per stage:

```toml
[features]
default = ["genesis"]
genesis = []
proximity = ["dep:eustress-embedvec"]
ontology = ["dep:eustress-embedvec"]
knowledge = ["ontology", "proximity"]
measurement = []
refinement = []
language = ["dep:eustress-embedvec"]
temporal = ["dep:eustress-stream"]
spatial = ["knowledge", "temporal"]
statistical = []
planning = ["spatial", "statistical"]
meta = ["dep:eustress-stream"]
full = ["genesis", "proximity", "ontology", "knowledge", "measurement",
        "refinement", "language", "temporal", "spatial", "statistical",
        "planning", "meta"]
```

## Phased Roadmap

### Phase 1: Foundation (Genesis + Concurrence)

**Goal:** Make entities real. Wire stubs to actual ECS. Minimum viable DSL.

| Function | Backed By | Work |
|---|---|---|
| `identity(class, name) → WorldRef` | `instance_new()` → real `Commands::spawn()` | Wire stub |
| `bind(entity, key, value) → TypedSlot` | `ScriptContext::queue_update()` | Wire stub |
| `locate(entity, x, y, z) → Position` | ECS Transform write | Wire stub |
| `fork(predicate, a, b) → Branch` | Rune closures | New function |
| `spawn(task) → Handle` | `task_spawn()` | Already registered |
| `join(handles) → Vec<Result>` | VmPool results | New function |

```rune
use eustress::functions::genesis;
pub fn main() {
    let pillar = genesis::identity("Part", "Sacred Pillar");
    genesis::locate(pillar, 10.0, 0.0, 20.0);
    genesis::bind(pillar, "Material", "Marble");
    genesis::bind(pillar, "ai", true);
}
```

### Phase 2: Perception (Proximity + Ontology + Knowledge Web)

**Goal:** Make the world queryable. Expose embedvec and ontology to Rune.

| Function | Backed By | Work |
|---|---|---|
| `nearest(entity, k) → Vec<Neighbor>` | `EmbedvecResource::query()` | New bridge |
| `nearest_class(entity, class, k)` | `OntologyIndex::search_class()` | New function |
| `classify(entity) → OntologyPath` | `OntologyTree::get_by_path()` | New function |
| `relate(a, b, predicate) → Relation` | `set_parent()` + spatial | New function |
| `model(ontology) → KnowledgeBase` | `OntologyIndex::new()` | New wrapper |
| `weave(entity, embedding)` | `OntologyIndex::insert()` | New function |
| `traverse(class, query, k) → Vec<Result>` | `search_class()` with descendants | New function |

```rune
use eustress::functions::{genesis, proximity, ontology};
pub fn find_nearby_trees(entity) {
    let neighbors = proximity::nearest_class(entity, "Entity/Spatial/Prop/Vegetation", 5);
    for n in neighbors {
        eustress::log_info(&format!("{} at {}", n.name, n.distance));
    }
    let path = ontology::classify(entity);
    // "Entity/Spatial/Actor/Character/Player"
}
```

### Phase 3: Pipeline (Measurement + Refinement + Language)

**Goal:** Data flows through cleaning, transformation, and embedding.

| Function | Backed By | Work |
|---|---|---|
| `measure(network) → Distribution` | `OntologyIndex::class_stats()` | New function |
| `entropy(signal) → Bits` | `physics::entropy_change()` | Expose existing |
| `cleanse(entity) → Pure` | AI consent filter | New function |
| `transform(record, shape) → Reshaped` | EEP record conversion | New function |
| `tokenize(text) → Vec<Token>` | `embed_query()` | New function |
| `validate(data, schema) → Bool` | `ScriptManager::validate_script()` | New function |

### Phase 4: Temporal + Spatial Intelligence

**Goal:** Time-aware world model with full graph queries.

| Function | Backed By | Work |
|---|---|---|
| `timestamp(event) → TimePoint` | `OwnedMessage.timestamp` | New function |
| `diff(state_t1, state_t2) → Delta` | `ChangeQueue` deltas | New function |
| `evolve(model, delta) → Model_t` | `ChangeQueue::push_delta()` | New function |
| `graph(kr) → KnowledgeGraph` | `OntologyIndex` | New wrapper |
| `link(a, b) → Edge` | Hierarchy + spatial edges | New function |
| `spatial_query(graph, embedding) → Neighborhood` | `search_global()` | New function |
| `resolve(linked) → WorldModel` | Live ECS + OntologyIndex + Stream | New function |

### Phase 5: Intelligence (Statistical + Planning + Meta)

**Goal:** Autonomous reasoning loop callable from Rune scripts.

| Function | Backed By | Work |
|---|---|---|
| `confidence(inference) → Score` | `SearchResult.score` | New function |
| `bayes(prior, likelihood) → Posterior` | Governor arbitration | New function |
| `plan(goal, world_model) → Strategy` | Governor blackboard | New function |
| `simulate(plan, model) → OutcomeSet` | SimulationClock time dilation | New function |
| `decide(outcomes) → Action` | Arbitrator MergeStrategy | New function |
| `execute(action) → Effect` | MCP `POST /mcp/create` → stream | New function |
| `observe(effect) → Feedback` | `bridge_parameter_changed_to_stream` | New function |
| `introspect(system) → Structure` | `stream.topics()` | New function |
| `profile(execution) → Metrics` | `RouterStats`, `IndexStats` | New function |

## Complete Function Inventory (72 Functions)

### Stage 1: Genesis (3)
`identity`, `bind`, `locate`

### Stage 2: Proximity (3)
`nearest`, `nearest_class`, `compose`

### Stage 3: Concurrence (3)
`fork`, `spawn`, `join`

### Stage 4: Ontological Genius (3)
`classify`, `relate`, `engineer`

### Stage 5: Knowledge Web (3)
`model`, `weave`, `traverse`

### Stage 6: Measurement (3)
`measure`, `entropy`, `bridge`

### Stage 7: Refinement (4)
`operate`, `analyze`, `cleanse`, `transform`

### Stage 8: Re-modeling (3)
`remodel`, `group`, `nest`

### Stage 9: Language (3)
`parse`, `tokenize`, `lex`

### Stage 10: Materialization (3)
`type_of`, `instantiate`, `evaluate`

### Stage 11: Extraction (3)
`extract`, `represent`, `reason`

### Stage 12: Spatial Intelligence (4)
`graph`, `link`, `spatial_query`, `resolve`

### Extended: Temporal (5)
`timestamp`, `sequence`, `diff`, `evolve`, `predict`

### Extended: Statistical (5)
`probability`, `confidence`, `bayes`, `sample`, `estimate`

### Extended: Learning (5)
`evaluate_outcome`, `update`, `reinforce`, `adapt`, `converge`

### Extended: Planning (5)
`plan`, `simulate`, `decide`, `execute`, `observe`

### Extended: Validation (5)
`validate`, `constrain`, `detect_anomaly`, `reconcile`, `assert`

### Extended: Infrastructure (5)
`embed`, `cluster`, `index`, `cache`, `introspect`

### Extended: Meta (5)
`profile`, `optimize`, `rewrite`, `synthesize`

## The Through-Line

```
HTTP/AI Input → parse → identity → bind → locate
    → classify → relate → weave → embed
    → nearest → compose → spatial_query
    → extract → represent → reason → resolve
    → timestamp → diff → evolve → predict
    → plan → simulate → decide → execute → observe
    → evaluate_outcome → update → reinforce → converge
```

Every arrow is an EustressStream topic publish.
Every function is a Rust core function exposed to Rune Script.
The pipeline is zero-copy, fan-out, replayable, persistent, multi-transport ready.

## Implementation Order

| Phase | Stages | Functions | Dependencies | Est. Effort |
|---|---|---|---|---|
| **1** | Genesis, Concurrence | 6 | ECSBindings (existing) | 1 week |
| **2** | Proximity, Ontology, Knowledge | 9 | embedvec crate | 2 weeks |
| **3** | Measurement, Refinement, Language | 10 | physics API, embedvec | 1 week |
| **4** | Temporal, Spatial Intelligence | 9 | eustress-stream, ChangeQueue | 2 weeks |
| **5** | Statistical, Planning, Meta | 15+ | Governor, SimulationClock | 3 weeks |

**Total: ~72 functions across 5 phases, ~9 weeks estimated.**

## Next Step

Create `crates/functions/` with `Cargo.toml`, `src/lib.rs`, and the Phase 1
genesis module backed by real ECS writes through the existing ECSBindings bridge.
