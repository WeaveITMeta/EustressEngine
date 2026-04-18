# Eustress Workshop — Tools, Modes, and UI Reference

Companion document to [EUSTRESS_WORKSHOP.md](EUSTRESS_WORKSHOP.md). This file enumerates every tool registered with the Workshop's `ToolRegistry`, the domain modes that gate them, the keywords that trigger mode activation, and the UI components required to surface the agent's actions in the chat panel.

**Scope:** This covers the AI-agent side of the Workshop (the conversational panel in the right tab strip). For the physical workshop tooling model (CNC tool inventory, storage containers, GPS-tracked fasteners, etc.) see [`docs/development/WORKSHOP_TOOLS.md`](../development/WORKSHOP_TOOLS.md).

---

## 1. Modes

The Workshop agent runs under a **General** base layer plus any number of stacked **domain modes**. Modes activate automatically by keyword scan on user messages (see `ActiveModes::detect_from_message` in [`workshop/modes/mod.rs`](../../eustress/crates/engine/src/workshop/modes/mod.rs)). Each mode contributes a system-prompt fragment to the Claude call and a set of tools to the palette.

| Icon | Mode | Trigger Keywords | Purpose |
|---|---|---|---|
| ⚡ | General | *(always active)* | Base entity/file/script/memory/physics/git/simulation/spatial ops |
| 🏭 | Manufacturing | manufacture, factory, production, assembly, investor, patent, BOM, bill of materials, prototype, pilot | Product ideation → manufacturer / investor allocation → artifact generation |
| 📦 | Warehousing | warehouse, inventory, storage, fulfillment, 3PL, pick, pack, reorder, stock | Inventory + storage layout |
| ⚙️ | Fabrication | CNC, 3D print, mill, lathe, tooling, machining, fabricat, laser cut | Process selection |
| 🔗 | Supply Chain | supply chain, logistics, forecast, demand, supplier, disruption, recall, shipment | Bayesian scenarios + risk scoring |
| 🛒 | Shopping | shop, catalog, pricing, checkout, marketplace, e-commerce, product listing | Pricing / catalog |
| ✈️ | Travel | route, fleet, customs, shipping, freight, transport, delivery | Logistics + customs documentation |
| 💰 | Finance | tax, finance, budget, cost analysis, compliance, revenue, P&L, accounting | Cost + tax compliance |
| 🔬 | Simulation | simulate, script, Rune, Luau, watchpoint, breakpoint, record, playback, physics | Sim control + scripted instrumentation |

**Multi-mode stacking:** Manufacturing + Supply Chain + Finance can all be active simultaneously. The agent sees the combined instruction set and full union of tools.

**Manual override:** The UI mode strip (§3.1) exposes per-pill deactivate and a `+ Add Mode` dropdown for user control.

---

## 2. Tools — full catalogue (52 total)

Each tool is a struct implementing `ToolHandler` in [`workshop/tools/*.rs`](../../eustress/crates/engine/src/workshop/tools/) (or [`workshop/modes/*.rs`](../../eustress/crates/engine/src/workshop/modes/) for per-mode tools). Registration happens once at `WorkshopPlugin::build`.

**Approval semantics:** Tools marked **✋** set `requires_approval: true`. When Claude calls one, the UI renders an approval card (§3.2) and `IdeationPipeline.awaiting_tool_approval` blocks further dispatch until the user Approves or Skips. Non-approval tools auto-execute during the same turn and feed their results straight back to Claude.

### 2.1 General-layer tools (36)

Available in **every** mode.

#### Entities (4)
| Tool | Approval | Description |
|---|---|---|
| `create_entity` | — | Spawn a Part/Model by writing `_instance.toml`. File watcher then loads it into the scene. |
| `query_entities` | — | Search entities by class / name / tag. |
| `update_entity` | — | Change transform, color, or other properties. |
| `delete_entity` | **✋** | Move entity's folder to `.eustress/trash/`. Recoverable via undo. |

#### Files (2)
| Tool | Approval | Description |
|---|---|---|
| `read_file` | — | Read any file under the Universe root. |
| `write_file` | — | Write a file in place. For reviewable edits prefer `stage_file_change`. |

#### Scripts (5)
| Tool | Approval | Description |
|---|---|---|
| `execute_rune` | — | Run Rune source immediately in the simulation. |
| `execute_luau` | — | Run Luau source in server or client context. |
| `image_to_code` | **✋** | Vision → generated code. Multi-call, costly. |
| `document_to_code` | **✋** | Markdown / PDF → generated code. Costly. |
| `generate_docs` | — | Auto-generate docs for an entity or script. |

#### Memory (5)
| Tool | Approval | Description |
|---|---|---|
| `remember` | — | Store a persistent memory that survives across sessions. |
| `recall` | — | Retrieve stored memories by semantic query. |
| `list_rules` | — | Read `.eustress/rules/*.md` + `Space/.rules/*.md` constraints. |
| `list_workflows` | — | Enumerate saved workflow definitions. |
| `query_stream_events` | — | Historical query against the EustressStream event log. |

#### Diff (1)
| Tool | Approval | Description |
|---|---|---|
| `stage_file_change` | — | Queue a file edit for human review. Shown in the staged-changes panel for user Approve/Reject. |

#### Git (4)
| Tool | Approval | Description |
|---|---|---|
| `git_status` | — | Working tree state. |
| `git_commit` | **✋** | Create a commit with currently staged changes. |
| `git_log` | — | Recent commit history. |
| `git_diff` | — | Unstaged / staged diff. |

#### Simulation (10)
| Tool | Approval | Description |
|---|---|---|
| `get_sim_value` | — | Read a tagged simulation scalar. |
| `set_sim_value` | — | Write a tagged simulation scalar. |
| `list_sim_values` | — | Enumerate all tagged values. |
| `get_tagged_entities` | — | Return entities carrying a given tag. |
| `raycast` | — | Spatial ray test against the scene. |
| `http_request` | **✋** | External HTTP. Approval-gated due to data-exfiltration risk. |
| `datastore_get` | — | Key-value read from the datastore. |
| `datastore_set` | — | Key-value write. |
| `add_tag` | — | Attach a tag to an entity. |
| `remove_tag` | — | Detach a tag from an entity. |

#### Physics (2)
| Tool | Approval | Description |
|---|---|---|
| `query_material` | — | Material properties (density, Young's modulus, thermal conductivity, etc.). |
| `calculate_physics` | — | Evaluate a physics expression (force, energy, pressure, …). |

#### Spatial (2)
| Tool | Approval | Description |
|---|---|---|
| `measure_distance` | — | Euclidean distance between two named entities. |
| `list_space_contents` | — | Flat enumeration of everything under Workspace. |

### 2.2 Manufacturing mode (4) — 🏭

Activated by product / factory / investor keywords. Drives the ideation-to-allocation pipeline.

| Tool | Approval | Description |
|---|---|---|
| `normalize_brief` | **✋** | Convert the conversation into a structured `ideation_brief.toml`. Extracts product name, innovations, target specs, BOM, physics model, deal structure. |
| `query_manufacturers` | — | Search mfr registry by process (injection molding, CNC, SMT, 3D printing), materials, certifications (ISO 9001, UL, CE, REACH), minimum capacity. |
| `query_investors` | — | Search investor pool by type (individual, venture_fund, family_office, strategic_corporate), min check, target vertical. |
| `allocate_product` | **✋** | Run the AI allocation engine. Scores manufacturers on capability / quality / cost / speed / risk (40/25/20/10/5). Returns selected mfr + investor allocations with equity stakes + confidence score. |

### 2.3 Simulation mode (3) — 🔬

Activated by simulation / script / watchpoint keywords. Plus the full [Rune ECS API reference](../../eustress/crates/engine/src/ui/rune_ecs_bindings.rs) is auto-injected into the system prompt so the agent always sees every registered function.

| Tool | Approval | Description |
|---|---|---|
| `control_simulation` | — | Play / pause / step / rewind the active simulation. |
| `set_breakpoint` | — | Watchpoint on a script line or tagged value. |
| `export_recording` | — | Write simulation playback to disk for later review. |

### 2.4 Supply Chain mode (3) — 🔗

| Tool | Approval | Description |
|---|---|---|
| `run_scenario` | — | Bayesian what-if simulation (disruption, demand shock, supplier failure). |
| `forecast_demand` | — | Demand prediction from historical + lead indicators. |
| `score_supplier_risk` | — | Risk score per supplier. |

### 2.5 Warehousing mode (2) — 📦

| Tool | Approval | Description |
|---|---|---|
| `inventory_check` | — | Stock levels. |
| `storage_optimize` | — | Layout optimization (bin packing, pick-path minimization). |

### 2.6 Finance mode (2) — 💰

| Tool | Approval | Description |
|---|---|---|
| `calculate_cost` | — | BOM cost rollup. |
| `estimate_tax` | — | Jurisdiction-aware compliance / tax estimate. |

### 2.7 Fabrication, Shopping, Travel (1 each)

| Mode | Tool | Approval | Description |
|---|---|---|---|
| ⚙️ Fabrication | `select_process` | — | Recommend CNC vs 3D-print vs injection vs casting given spec + volume. |
| 🛒 Shopping | `price_product` | — | Retail pricing recommendation with margin / positioning. |
| ✈️ Travel | `estimate_shipping` | — | Freight cost + time + customs paperwork list. |

### 2.8 Approval-gate summary

7 of the 52 tools require approval. The criteria are:
- **Destructive:** `delete_entity`
- **Published state change:** `git_commit`
- **Costly LLM operations:** `image_to_code`, `document_to_code`
- **Data exfiltration risk:** `http_request`
- **Product-commitment steps:** `normalize_brief`, `allocate_product`

The other 45 auto-execute during the turn. This split exists so the common case (querying, reading, updating, simulating) stays conversational while the dangerous or irreversible actions always surface as cards the user confirms.

---

## 3. UI Modules

Components the Slint chat needs to surface the agent's behaviour. Each maps directly to fields on [`ChatMessage`](../../eustress/crates/engine/src/workshop/mod.rs) / [`IdeationPipeline`](../../eustress/crates/engine/src/workshop/mod.rs).

### 3.1 Mode Strip

Top of the Workshop panel.

```
[⚡ General] [🏭 Manufacturing ×] [🔗 Supply Chain ×]              [+ Add Mode ▾]
```

- Pills per active mode, icon + name, color per `WorkshopMode::color()`.
- `×` on domain pills to manually deactivate (General is sticky).
- `+ Add Mode` dropdown offers the other 7 domains.

**Data source:** `IdeationPipeline.active_modes.domains`.
**Callbacks:** `on-deactivate-mode(mode_name)`, `on-activate-mode(mode_name)`.

### 3.2 Tool Use Card

Rendered inline wherever a `ChatMessage { role: Mcp, mcp_method: Some("tool_use"), .. }` appears. Five visual states:

```
╭─ 🟡 PENDING ──────────────────────────────╮   Pending approval
│  🏭  allocate_product                      │   (requires_approval: true)
│  Pick manufacturer + investors for …       │
│  ▸ Input                          (expand) │
│  [ Approve ]  [ Skip ]   ~$0.05 est cost   │
╰────────────────────────────────────────────╯

╭─ 🔵 RUNNING ──────────────────────────────╮   Dispatched, awaiting ToolResult
│  ⚡ create_entity  ⟳                       │
│  name="Sphere", size=[2,2,2]               │
╰────────────────────────────────────────────╯

╭─ 🟢 DONE ─────────────────────────────────╮   Completed
│  ⚡ create_entity                          │
│  name="Sphere" → Part ⟨42⟩                │
│  ✓ Created Part 'Sphere' at [0, 0, 0]      │
╰────────────────────────────────────────────╯

╭─ 🔴 FAILED ───────────────────────────────╮   Tool error
│  ⚡ delete_entity                          │
│  ✗ Entity 'Foo' not found                  │
│  [ Retry ]  [ Explain ]                    │
╰────────────────────────────────────────────╯

╭─ ⚫ SKIPPED ──────────────────────────────╮   User skipped
│  ⚡ http_request  ·  skipped by user       │
╰────────────────────────────────────────────╯
```

**Component properties:**
- `tool-name: string`
- `mode-icon: string`
- `input-preview: string` (collapsed)
- `input-json: string` (expanded view)
- `output: string`
- `error: string`
- `state: string` — `"pending" | "running" | "completed" | "failed" | "skipped"`
- `estimated-cost: string`

**Callbacks:** `on-approve()`, `on-skip()`, `on-retry()`, `on-toggle-expand()`.

**Data source:** `ChatMessage.mcp_status` + `tool_use_id` + `mcp_endpoint` + `tool_input` + `tool_result`.

### 3.3 Streaming Assistant Message

```
╭─ ⚡ 🏭 ────────────────────────────────────╮
│  I'll design the cell assembly. First I'll │
│  normalize the brief, then allocate manu-  │
│  facturers …                               │
╰────────────────────────────────────────────╯
```

Mode badges inline at the top, text body below. When streaming is added later, a blinking cursor at the tail.

**Data source:** `ChatMessage { role: System, content }`.

### 3.4 Mode-Activation Banner

A subtle slim strip between messages when a mode newly activates:

```
⚡ 🏭  Manufacturing — mode activated. I can now guide you through product ideation …
```

Smaller font, lower opacity, colored per mode.

**Data source:** `ChatMessage { role: System, content }` where `content.contains("— mode activated")`. These are also filtered out of the Claude message array by `build_anthropic_messages` (UI-only).

### 3.5 Turn Footer

Attached under the final assistant message of a turn:

```
                                         2 tools · $0.027 · 1.2s
```

Aggregates across the turn's messages: tool_use count, summed cost (`estimated_cost` totals), wall-clock duration.

### 3.6 Pipeline Sidebar (Manufacturing mode only)

Expandable right-side panel when Manufacturing is active:

```
╭─ PRODUCT PIPELINE ──────────────╮
│ ✓ Normalize brief     ·  $0.03  │
│ ● Patent draft        ·  $0.05  │
│ ○ SOTA validation     ·  $0.04  │
│ ○ Requirements        ·  $0.03  │
│ ○ Mesh generation     ·  $0.02  │
│ ○ Simulation scripts  ·  $0.03  │
│ ─────────────────────           │
│ Total est: $0.20 / $5 budget    │
│ [ Approve All Remaining ]       │
╰──────────────────────────────────╯
```

Shows `IdeationPipeline.steps` with their `StepStatus`. "Approve All Remaining" bulk-approves every pending step and cascades through the pipeline.

### 3.7 Approval Gate Dialog

For destructive / high-stakes tools, clicking Approve on the ToolUseCard (§3.2) pops an additional confirmation:

```
╭─ CONFIRM: delete_entity ──────────╮
│  This will move                    │
│    Workspace/V-Cell/Cathode        │
│  to trash. Recoverable via undo.   │
│  [ Cancel ]    [ Confirm Delete ]  │
╰────────────────────────────────────╯
```

Triggered whenever `requires_approval == true` and the user hasn't opted into "Auto-confirm destructive operations" in settings.

---

## 4. Implementation status

| Component | Status |
|---|---|
| Mode detection + keyword trigger | ✅ wired in `handle_send_message` |
| Mode-activation badge messages | ✅ emitted as `System` messages |
| `call_with_tools` dispatch | ✅ |
| Tool palette filtering by active mode | ✅ via `ToolRegistry::claude_tools(active_modes)` |
| Auto-execute non-approval tools | ✅ in `poll_agentic_responses` |
| Tool result feed-back to Claude | ✅ via `build_anthropic_messages` pairing tool_use ↔ tool_result |
| Approval gate (blocks dispatch) | ✅ `awaiting_tool_approval` |
| Approve → dispatch → result | ✅ in `handle_approve_mcp` |
| Skip → synthesize tool_result | ✅ in `handle_skip_mcp` |
| Mode Strip UI (§3.1) | 🔲 to build |
| Tool Use Card UI (§3.2) | 🔲 to build — currently renders as plain MCP message |
| Mode-Activation Banner (§3.4) | 🔲 to style — currently plain System message |
| Pipeline Sidebar (§3.6) | 🔲 to build |
| Approval Gate Dialog (§3.7) | 🔲 to build |

The backend agentic loop is complete. The remaining work is Slint UI layer that visually distinguishes the message types the backend already produces.

---

## 5. Extension: adding a new tool

```rust
// eustress/crates/engine/src/workshop/tools/my_new_tool.rs
pub struct MyNewTool;
impl ToolHandler for MyNewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "my_new_tool",
            description: "Does X. Returns Y.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "foo": { "type": "string", "description": "…" }
                },
                "required": ["foo"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.my_new_tool"],
        }
    }
    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        // … read `input`, do the thing, return ToolResult
    }
}
```

Register in `WorkshopPlugin::build`:
```rust
registry.register(tools::my_new_tool::MyNewTool);
```

That's it. The system prompt is automatically updated (tool definitions are emitted via the API's `tools` array, not the prompt text), the mode filter picks it up, and the agent can call it starting on the next turn.

## 6. Extension: adding a new mode

1. Add variant to `WorkshopMode` enum in [`modes/mod.rs`](../../eustress/crates/engine/src/workshop/modes/mod.rs).
2. Fill in `display_name`, `icon`, `color`, `trigger_keywords`, `system_prompt_fragment`, `greeting`.
3. Add to `WorkshopMode::all_domains()`.
4. Create `modes/my_new_mode.rs` with the mode-specific tool handlers and register them in `WorkshopPlugin::build`.
5. Optionally add a Pipeline Sidebar config if the mode has a linear workflow like Manufacturing.

No breaking changes required in the Claude bridge — the new mode's keywords will trigger activation and its tools will appear in future Claude calls automatically.
