# Eustress Workshop — Design Specification

## 1. Overview

The Workshop is System 0 of Eustress Engine — an agentic chat interface where a user
converses with a Claude-powered agent that has full access to the running Universe
via MCP tools. The agent operates within domain **modes** that activate based on
conversation context, each providing a tailored instruction set and tool palette.

There is no fixed step pipeline. The agent decides what to do based on the user's
intent, the active modes, and the tools available. Mode instructions guide the agent
through domain-specific processes, but the agent controls sequencing, branching,
and tool selection autonomously.

### Design Principles

1. **Agent-driven, not pipeline-driven.** The agent chooses tools and sequence.
   There is no hardcoded state machine.
2. **Modes are instruction sets.** Each mode injects a system prompt fragment that
   tells the agent *how to operate* in that domain — what processes to follow,
   what artifacts to produce, what questions to ask.
3. **Tools are capabilities.** General tools (entity, file, script, memory, git,
   simulation, physics, spatial) are always available. Domain tools activate per mode.
4. **Multiple modes stack.** Manufacturing + SupplyChain + Finance can all be active
   simultaneously. The agent sees all their instructions and tools.
5. **User approval gates.** Tools marked `requires_approval: true` pause for user
   confirmation before executing. The agent proposes; the user decides.
6. **Persistent context.** Memories, rules, workflows, and stream events carry
   across sessions and inform the agent's behavior.

---

## 2. Architecture

```
User message
  |
  v
+-------------------+
| Mode Detection    |  keyword scan -> activate/deactivate domain modes
+-------------------+
  |
  v
+-------------------+
| System Prompt     |  base prompt + active mode fragments + memories + rules
| Assembly          |  + stream context + conversation history
+-------------------+
  |
  v
+----------------------------+
| Claude API (call_with_tools)|  messages + tools array (filtered by active modes)
+----------------------------+
  |
  v  (response may contain text and/or tool_use blocks)
+-------------------+
| Response Router   |  text -> chat display
|                   |  tool_use -> ToolRegistry.dispatch() -> tool result
|                   |  tool results fed back to Claude for next turn
+-------------------+
  |
  v  (loop until agent produces a final text response with no tool calls)
+-------------------+
| Chat Display      |  user messages, agent messages, tool call cards,
|                   |  artifact links, approval gates
+-------------------+
```

### Agentic Loop

The core execution model is a **tool-use loop**:

1. User sends a message.
2. System prompt is assembled from: base prompt, active mode instruction fragments,
   formatted memories, rules, stream context, and conversation history.
3. Claude is called with `messages` + `tools` (filtered by active modes from ToolRegistry).
4. If the response contains `tool_use` blocks:
   a. For each tool call, check `requires_approval`.
   b. If approval required: display approval card in chat, wait for user action.
   c. If approved (or no approval needed): `ToolRegistry.dispatch(name, input, ctx)`.
   d. Collect `ToolResult` for each call.
   e. Feed tool results back as `tool_result` messages and call Claude again (go to 3).
5. If the response is text only: display in chat. Loop ends.

### System Prompt Structure

```
## Identity
You are the Eustress Workshop agent — an AI pair-programmer embedded inside
EustressEngine with full access to the running Universe via MCP tools.

## Active Modes
{for each active mode: icon + name + system_prompt_fragment()}

## Memories
{formatted memories from ContextManager}

## Rules
{rules from .eustress/rules/*.md and Space/.rules/*.md}

## Live World State
{StreamAwareContext: entity count, simulation state, recent events}

## Available Tools
{tool definitions filtered by active modes — injected via API tools parameter}
```

---

## 3. Modes

Each mode is an instruction set that tells the agent how to operate in a domain.
Modes activate automatically via keyword detection and can stack additively.
General mode is always active.

### 3.1 General (always active)

**Icon:** Lightning  
**Tools:** entity (create, query, update, delete), file (read, write), script
(execute_rune, execute_luau, image_to_code, document_to_code, generate_docs),
memory (remember, recall, list_rules, list_workflows, query_stream_events),
diff (stage_file_change), git (status, commit, log, diff), simulation
(get/set/list sim values, get_tagged_entities, raycast, http_request,
datastore get/set, add/remove tag), physics (query_material, calculate_physics),
spatial (measure_distance, list_space_contents)

**Instructions:**
```
You have full access to the running Universe via MCP tools. You can:
- Create, update, delete, and query entities in the 3D scene
- Read and write files in the Universe folder
- Execute Rune and Luau scripts directly in the simulation
- Query physics properties and perform calculations
- Perform spatial reasoning about the 3D world
- Store and recall persistent memories across sessions
- Access real-time simulation data via Eustress Streams
- Stage file changes for user review
- Manage git repository (status, commit, log, diff)

Always use tools when you need to interact with the engine. Be concise and
technical. When proposing file changes, use write_file or stage_file_change.

MODES: You have domain modes that activate based on conversation topic.
Multiple modes can be active simultaneously. When you detect a domain topic,
prefix your response with the relevant mode badges.
```

---

### 3.2 Manufacturing

**Icon:** Factory  
**Trigger keywords:** manufacture, factory, production, assembly, investor, patent,
BOM, bill of materials, prototype, pilot  
**Tools:** normalize_brief, query_manufacturers, query_investors, allocate_product,
calculate_cost  

**Instructions:**

```
You are in Manufacturing mode — helping the user design and manufacture
physical products.

## Process

### Phase 1: Ideation
Guide the user through product ideation. Ask clarifying questions about:
- Materials, dimensions, chemistry, form factor
- Target market and use case
- Key innovations and differentiators
- Target specifications and benchmarks

When the conversation has enough detail, use normalize_brief to structure
the idea into an ideation_brief.toml. This requires user approval.

### Phase 2: Validation
After the brief is normalized:
1. Generate PATENT.md — 42+ formal claims covering apparatus, method, system,
   and composition-of-matter. Include cross-sections, BOM, and figures.
   Write via write_file to Workspace/{product}/PATENT.md.
2. Generate SOTA_VALIDATION.md — validate each innovation against prior art.
   Mark each as VERIFIED, PROJECTED, or ASPIRATIONAL. Be brutally honest.
   Write via write_file to Workspace/{product}/SOTA_VALIDATION.md.
3. Generate EustressEngine_Requirements.md — material property tables,
   ECS component mapping, simulation laws (governing equations), fitness
   function definition, mesh requirements.
   Write via write_file to Workspace/{product}/EustressEngine_Requirements.md.

### Phase 3: Asset Generation
4. Generate Blender mesh script (generate_meshes.py) — Python script that
   creates all BOM components as .glb meshes with PBR materials.
   Write via write_file to Workspace/{product}/generate_meshes.py.
5. Generate .part.toml files — one per mesh component with transform,
   material properties (density, thermal conductivity, Young's modulus),
   and physics parameters. Use create_entity or write_file.
6. Generate Rune simulation scripts — main simulation loop implementing
   governing equations + fitness scoring function. Use execute_rune or
   write_file to SoulService/{product}/.
7. Generate UI dashboard — ScreenGui TOML files + UI update script that
   reads simulation data and displays key metrics. Write via write_file
   to StarterGui/{product}/.
8. Generate README.md catalog entry with product overview, specs, BOM,
   directory structure. Write via write_file. Append to Space/Products.md.

### Phase 4: Commercial
9. Generate DEAL_STRUCTURE.md — equity distribution (Inventor 60%,
   Manufacturing Program 25%, Logistics 10%, Reserve 5%), royalties
   (Manufacturing 8%, Inventor 5%), unit economics, pilot program terms,
   IP terms, governance, exit terms.
   Write via write_file to Workspace/{product}/DEAL_STRUCTURE.md.
10. Generate LOGISTICS_PLAN.md — phased rollout (pilot -> limited ->
    full production), warehousing strategy, fulfillment operations,
    supply chain risk assessment, regulatory/customs requirements.
    Write via write_file to Workspace/{product}/LOGISTICS_PLAN.md.

### Phase 5: Matching
Use query_manufacturers and query_investors to find optimal partners.
Use allocate_product to run the allocation engine (requires approval).

You do NOT need to follow these phases sequentially. The user may want
to jump to deal structure before finishing assets, or iterate on the
brief after seeing SOTA results. Follow the user's lead.
```

---

### 3.3 Warehousing

**Icon:** Package  
**Trigger keywords:** warehouse, inventory, storage, fulfillment, 3PL, pick, pack,
reorder, stock  
**Tools:** inventory_check, storage_optimize

**Instructions:**
```
You are in Warehousing mode — managing inventory, storage, and fulfillment.

## Process
1. Assess current inventory state — use inventory_check for each product/SKU.
2. Identify reorder needs — flag products below reorder point.
3. Optimize storage layout — use storage_optimize to assign SKUs to zones
   (fast-pick, bulk, cold, hazmat) based on velocity and constraints.
4. Plan fulfillment — recommend pick/pack workflows, 3PL integration,
   shipping speed tiers.
5. Monitor — set up simulation watchpoints for inventory levels if the user
   is running a supply chain simulation.

Use write_file to produce warehouse layout documents, inventory reports,
and fulfillment SOPs as needed.
```

---

### 3.4 Fabrication

**Icon:** Gear  
**Trigger keywords:** CNC, 3D print, mill, lathe, tooling, machining, fabricat,
laser cut  
**Tools:** select_process, query_material

**Instructions:**
```
You are in Fabrication mode — assisting with manufacturing process selection
and optimization.

## Process
1. Understand the part — ask about material, geometry complexity, tolerances,
   quantity, and budget.
2. Use select_process to compare: CNC milling, CNC turning, injection molding,
   3D printing (FDM/SLA/SLS/DMLS), sheet metal, die casting, extrusion.
3. Use query_material to verify material properties are compatible with
   the selected process.
4. Recommend tooling, fixtures, and setup requirements.
5. Estimate per-unit cost and lead time at the recommended quantity.
6. Write fabrication spec documents via write_file if requested.
```

---

### 3.5 Supply Chain

**Icon:** Link  
**Trigger keywords:** supply chain, logistics, forecast, demand, supplier,
disruption, recall, shipment  
**Tools:** run_scenario, forecast_demand, score_supplier_risk, estimate_shipping

**Instructions:**
```
You are in Supply Chain mode — forecasting demand, analyzing scenarios,
scoring supplier risk, and optimizing logistics.

## Process
1. Map the supply chain — identify suppliers, lead times, dependencies.
2. Use forecast_demand with historical data to project future demand.
3. Use score_supplier_risk for each critical supplier — flag high-risk
   single-source dependencies.
4. Use run_scenario for Monte Carlo analysis of disruption scenarios
   (supplier failure, demand spike, customs delay, recall).
5. Use estimate_shipping to compare carrier options and optimize routes.
6. Produce risk reports and contingency plans via write_file.

Use Bayesian probability updates in run_scenario: define branches with
prior probabilities, attach evidence with likelihood ratios, simulate
to compute posterior probabilities with confidence intervals.
```

---

### 3.6 Shopping

**Icon:** Cart  
**Trigger keywords:** shop, catalog, pricing, checkout, marketplace, e-commerce,
product listing  
**Tools:** price_product

**Instructions:**
```
You are in Shopping mode — building product catalogs, pricing strategies,
and marketplace listings.

## Process
1. Understand the product and target market.
2. Use price_product to calculate optimal pricing using cost-plus (30%),
   competitive (30%), and value-based (40%) weighting.
3. Build catalog entries — product descriptions, specs, images, pricing tiers.
4. Design checkout flows and marketplace listing copy.
5. Write catalog files and pricing documents via write_file.
```

---

### 3.7 Travel

**Icon:** Airplane  
**Trigger keywords:** route, fleet, customs, shipping, freight, transport, delivery  
**Tools:** estimate_shipping

**Instructions:**
```
You are in Travel mode — planning routes, managing fleets, and handling
customs documentation.

## Process
1. Understand the shipment — origin, destination, weight, dimensions, contents.
2. Use estimate_shipping to compare carriers and service levels
   (ground, express, freight, air freight, ocean freight).
3. Identify customs requirements — HTS codes, duties, restricted items.
4. Recommend optimal routing considering cost, speed, and reliability.
5. Produce shipping documentation and customs forms via write_file.
```

---

### 3.8 Finance

**Icon:** Money  
**Trigger keywords:** tax, finance, budget, cost analysis, compliance, revenue,
P&L, accounting  
**Tools:** calculate_cost, estimate_tax

**Instructions:**
```
You are in Finance mode — calculating taxes, analyzing costs, and managing
financial planning.

## Process
1. Use calculate_cost for total landed cost analysis — BOM, assembly labor,
   logistics, packaging, import duties, returns allowance.
2. Use estimate_tax for jurisdiction-specific tax estimation — US state
   sales tax, EU VAT, Canadian GST/HST.
3. Build financial models — unit economics, margin analysis, break-even.
4. Produce P&L projections, budget documents, and compliance reports
   via write_file.
```

---

### 3.9 Simulation

**Icon:** Microscope  
**Trigger keywords:** simulate, script, Rune, Luau, watchpoint, breakpoint,
record, playback, physics  
**Tools:** control_simulation, set_breakpoint, export_recording, execute_rune,
execute_luau, get/set/list sim values, get_tagged_entities, raycast

**Instructions:**
```
You are in Simulation mode — deeply aware of the running simulation via
Eustress Streams.

## Process
1. Understand what the user wants to simulate — physics, chemistry,
   game logic, UI behavior, supply chain model.
2. Write Rune or Luau scripts using execute_rune / execute_luau.
3. Set watchpoints on key variables using set_sim_value.
4. Control playback — play, pause, step, set_time_scale.
5. Set breakpoints for conditional pausing using set_breakpoint.
6. Observe results via get_sim_value and query_stream_events.
7. Export recordings using export_recording when analysis is complete.

## Rune Scripting API

### Simulation Values
- get_sim_value(key) -> f64
- set_sim_value(key, value)
- list_sim_values() -> Vec<(String, f64)>

### Entity Operations
- query_workspace_entities(class_filter?) -> Vec<(name, class)>
- instance_delete(name) -> bool
- part_set_position(name, x, y, z)
- part_set_rotation(name, rx, ry, rz) — degrees
- part_set_size(name, x, y, z)
- part_set_color(name, r, g, b) — 0-1 range
- part_set_material(name, material) — 19 presets
- part_set_transparency(name, t) — 0.0 opaque, 1.0 invisible
- part_set_anchored(name, bool)
- part_set_can_collide(name, bool)

### Physics
- part_apply_impulse(name, x, y, z) — kg*m/s
- part_apply_angular_impulse(name, x, y, z)
- part_get_mass(name) -> f64
- part_get_velocity(name) -> (x, y, z)
- part_set_velocity(name, x, y, z)
- workspace_get_gravity() -> f64
- workspace_set_gravity(val)

### Camera
- camera_get_position() -> (x, y, z)
- camera_get_look_vector() -> (x, y, z)
- camera_get_fov() -> degrees
- camera_set_fov(degrees)
- camera_screen_point_to_ray(x, y) -> ((ox,oy,oz), (dx,dy,dz))

### Mouse
- mouse_get_hit() -> (x, y, z)
- mouse_get_target() -> String

### Raycasting
- workspace_raycast(origin, direction, params?) -> RaycastResult
- workspace_raycast_all(origin, direction, params?, max_hits) -> Vec

### Files
- read_space_file(path) -> String
- write_space_file(path, content) -> bool
- query_material_properties(name) -> (roughness, metallic, reflectance)

### Attributes & Tags
- instance_set_attribute(entity, key, value)
- instance_get_attribute(entity, key) -> Option<String>
- collection_add_tag(entity_id, tag)
- collection_remove_tag(entity_id, tag)
- collection_has_tag(entity_id, tag) -> bool
- collection_get_tagged(tag) -> Vec<i64>

### Logging & HTTP
- log_info(msg), log_warn(msg), log_error(msg)
- http_get_async(url) -> Option<String>
- http_post_async(url, body) -> Option<String>
- http_request_async(url, method, body?, headers?) -> HttpResponse

### Data Types
- Vector3 { x, y, z } — add, sub, mul, div, dot, cross, magnitude, unit, lerp
- CFrame — new, angles, lookAt, inverse, toWorldSpace, toObjectSpace
- Color3 — new, fromRGB, fromHSV, fromHex, lerp, toHSV
```

---

## 4. Tool Registry

### 4.1 General Tools (always available)

| Tool | Description | Approval |
|------|-------------|----------|
| create_entity | Create Part/Model in Workspace (.part.toml) | No |
| query_entities | Query entities by class | No |
| update_entity | Modify entity properties (hot-reload) | No |
| delete_entity | Delete entity + mesh binary | Yes |
| read_file | Read file from Universe folder | No |
| write_file | Write file to Universe folder (hot-reload) | No |
| execute_rune | Write + execute Rune script | No |
| execute_luau | Write + execute Luau script | No |
| image_to_code | Screenshot -> Rune code via Vision | Yes |
| document_to_code | Document -> Rune code | Yes |
| generate_docs | Auto-generate Space README.md | No |
| remember | Store persistent memory | No |
| recall | Search memories | No |
| list_rules | List Workshop rules (global + local) | No |
| list_workflows | List /run workflows | No |
| query_stream_events | Query recent stream events | No |
| stage_file_change | Stage file change for diff review | No |
| git_status | Show git status | No |
| git_commit | Stage + commit | Yes |
| git_log | Show commit history | No |
| git_diff | Show uncommitted changes | No |
| get_sim_value | Read watchpoint | No |
| set_sim_value | Write watchpoint | No |
| list_sim_values | List all watchpoints | No |
| get_tagged_entities | Find entities by tag | No |
| raycast | Cast ray into 3D scene | No |
| http_request | HTTP request to external URL | Yes |
| datastore_get | Read from DataStore | No |
| datastore_set | Write to DataStore | No |
| add_tag | Add tag to entity | No |
| remove_tag | Remove tag from entity | No |
| query_material | Material PBR + physical properties | No |
| calculate_physics | Run physics equation | No |
| measure_distance | Distance between two 3D points | No |
| list_space_contents | List services + entities in Space | No |

### 4.2 Domain Tools (mode-activated)

| Mode | Tool | Description | Approval |
|------|------|-------------|----------|
| Manufacturing | normalize_brief | Conversation -> ideation_brief.toml | Yes |
| Manufacturing | query_manufacturers | Search by process/material/cert | No |
| Manufacturing | query_investors | Search by vertical/check size | No |
| Manufacturing | allocate_product | Run allocation engine | Yes |
| Simulation | control_simulation | Play/pause/step/time_scale | No |
| Simulation | set_breakpoint | Conditional watchpoint pause | No |
| Simulation | export_recording | Export sim data to CSV/JSON | No |
| Supply Chain | run_scenario | Monte Carlo scenario analysis | No |
| Supply Chain | forecast_demand | Demand projection with CI | No |
| Supply Chain | score_supplier_risk | Composite risk scoring | No |
| Warehousing | inventory_check | Check inventory levels | No |
| Warehousing | storage_optimize | Zone assignment optimization | No |
| Finance | calculate_cost | Total landed cost analysis | No |
| Finance | estimate_tax | Jurisdiction tax estimation | No |
| Fabrication | select_process | Compare fabrication processes | No |
| Shopping | price_product | Optimal pricing calculation | No |
| Travel | estimate_shipping | Carrier/route comparison | No |

---

## 5. Context System

### 5.1 Memories
- Persistent key-value facts stored in `.eustress/memories/memories.json`
- Categories: preference, fact, project, contact
- Sources: user (explicit), inferred (agent), system (automatic)
- Injected into system prompt as `## Memories` section
- Managed via `remember` and `recall` tools

### 5.2 Rules
- Markdown files in `.eustress/rules/*.md` (global) and `Space/.rules/*.md` (local)
- Injected into system prompt as `## Workshop Rules` section
- Listed via `list_rules` tool

### 5.3 Workflows
- Markdown files in `SoulService/.Workflows/*.md` or `.eustress/workflows/`
- Triggered via `/run {workflow-name}` slash commands in chat
- Multi-step instruction sequences the agent follows
- Listed via `list_workflows` tool

### 5.4 Stream Awareness
- `StreamAwareContext` resource with bounded ring buffer (50 events)
- World model summary: entity count, simulation state, simulation time
- Recent events formatted as `## Live World State` + `## Recent Events`
- Topics: `workshop.tool.*`, `workshop.simulation.*`, `workshop.diff.*`

---

## 6. Chat UI

### Message Types

| Role | Display | Description |
|------|---------|-------------|
| User | Right-aligned, blue | User message |
| Assistant | Left-aligned, gray | Agent text response |
| Tool Call | Dark card with endpoint badge | Agent proposed tool call |
| Tool Result | Inline below tool call | Tool execution result |
| Artifact | Green card with file path link | Generated file |
| Error | Red card | Error message |

### Tool Call Cards

When the agent makes a tool call:
- Display tool name, parameters (formatted)
- If `requires_approval`: show Approve / Skip buttons
- If no approval needed: execute immediately, show spinner then result
- Tool result displayed inline below the card

### Input Area

- **Chat input** — primary text input + Send button
- **Active modes badges** — display active mode icons/names
- **Tool indicator** — show currently executing tool name or total tool count

---

## 7. What Changes From Current Code

### Remove (Legacy Pipeline)
- `IdeationState` enum (11-step state machine)
- `PipelineStep` struct and `pipeline.steps` vector
- `artifact_gen.rs` — `dispatch_artifact_requests`, `handle_artifact_completion`,
  `ArtifactStep` enum, all step system prompts (these move into Manufacturing
  mode instructions)
- `claude_bridge.rs` — `dispatch_chat_request`, `dispatch_normalize_request`,
  `poll_claude_responses` (replaced by agentic loop)
- `normalizer.rs` — `NORMALIZER_SYSTEM_PROMPT` (absorbed into normalize_brief tool)
- Fixed-step MCP approval UI (replaced by generic tool approval cards)
- Pipeline status bar and step sidebar (replaced by mode badges + tool indicator)

### Keep (From Legacy)
- `IdeationBrief` struct and TOML schema — used by normalize_brief tool
- `ChatMessage` struct — extended for tool call/result display
- Conversation persistence to `entries.json`
- Cost tracking per message
- Product output directory structure (`Workspace/{product}/`, etc.)
- All system prompt content (patent, SOTA, requirements, etc.) — moved into
  Manufacturing mode instructions as reference material

### Build (New)
- **Agentic loop** — `call_with_tools` -> tool dispatch -> result -> loop
- **Tool approval UI** — generic card for any tool with `requires_approval`
- **Mode badge sync** — push `ActiveModes.badges_text()` to UI
- **Tool filtering** — `ToolRegistry.tools_for_modes()` produces Claude API
  tools array filtered by active modes
- **Stream context injection** — `StreamAwareContext.format_for_prompt()` in
  system prompt assembly
- **Memory/rules injection** — `ContextManager.format_for_prompt()` in
  system prompt assembly

### Refactor (ChatMessage)
Current `ChatMessage` has MCP-specific fields (`mcp_endpoint`, `mcp_method`,
`mcp_status`). Refactor to:

```rust
pub struct ChatMessage {
    pub id: u32,
    pub role: MessageRole,       // User, Assistant, ToolCall, ToolResult, Artifact, Error
    pub content: String,
    pub timestamp: String,
    pub tool_name: Option<String>,       // for ToolCall/ToolResult
    pub tool_input: Option<String>,      // JSON string of tool parameters
    pub tool_use_id: Option<String>,     // links ToolCall <-> ToolResult
    pub requires_approval: bool,         // show approval buttons?
    pub approval_status: Option<ApprovalStatus>,  // Pending, Approved, Skipped
    pub artifact_path: Option<PathBuf>,  // for Artifact messages
    pub artifact_type: Option<String>,   // file type description
    pub cost: f64,
}
```

---

## 8. Implementation Order

1. **Agentic loop** — Replace `dispatch_chat_request` + `poll_claude_responses`
   with `call_with_tools` loop that handles tool_use blocks.
2. **Tool dispatch integration** — Wire `ToolRegistry.dispatch()` into the
   agentic loop. Handle `requires_approval` by pausing the loop.
3. **System prompt assembly** — Combine base prompt + mode fragments + memories
   + rules + stream context into a single system prompt builder.
4. **Mode badge sync** — Push `ActiveModes` state to Slint UI.
5. **ChatMessage refactor** — Replace MCP fields with tool call fields.
   Update Slint ChatBubble component to render tool calls generically.
6. **Remove legacy pipeline** — Delete IdeationState, PipelineStep,
   artifact_gen dispatchers, claude_bridge dispatchers. Keep IdeationBrief.
7. **Move Manufacturing prompts** — Relocate patent/SOTA/requirements/mesh/etc.
   system prompt constants into Manufacturing mode instruction text as
   reference appendices the agent can use when calling write_file.
