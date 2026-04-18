//! Workshop Modes — domain-specific configurations for the AI agent.
//!
//! Each mode defines:
//! - A system prompt fragment (injected into Claude's context)
//! - Pipeline sidebar steps (if any — empty for chat-only modes)
//! - A greeting message when the mode activates
//!
//! The `General` mode is always active as a base layer. Domain modes
//! add specialized tools and prompts on top.

pub mod manufacturing;
pub mod simulation;
pub mod supply_chain;
pub mod warehousing;
pub mod finance;
pub mod fabrication;
pub mod shopping;
pub mod travel;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// WorkshopMode enum
// ---------------------------------------------------------------------------

/// Domain modes that the AI activates based on conversation context.
/// Multiple modes can be active simultaneously. General is always on.
/// Modes are NOT user-selected — they're inferred by the AI from what
/// the user is talking about, and stack additively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkshopMode {
    /// Always active. Entity management, scripts, physics, files, memory.
    General,
    /// Product ideation, manufacturer/investor matching, deal structure.
    Manufacturing,
    /// Warehouse layout, inventory, storage, pick/pack, 3PL.
    Warehousing,
    /// CNC, 3D printing, assembly line, tooling.
    Fabrication,
    /// Demand forecasting, Bayesian scenarios, logistics.
    SupplyChain,
    /// Product catalog, pricing, checkout, marketplace.
    Shopping,
    /// Route planning, fleet, customs documentation.
    Travel,
    /// Tax calculation, compliance, financial reporting.
    Finance,
    /// Rune scripting, scene building, recording/playback.
    Simulation,
}

impl WorkshopMode {
    /// Display name shown in mode badges.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Manufacturing => "Manufacturing",
            Self::Warehousing => "Warehousing",
            Self::Fabrication => "Fabrication",
            Self::SupplyChain => "Supply Chain",
            Self::Shopping => "Shopping",
            Self::Travel => "Travel",
            Self::Finance => "Finance",
            Self::Simulation => "Simulation",
        }
    }

    /// Icon emoji for inline mode badges in chat responses.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::General => "⚡",
            Self::Manufacturing => "🏭",
            Self::Warehousing => "📦",
            Self::Fabrication => "⚙️",
            Self::SupplyChain => "🔗",
            Self::Shopping => "🛒",
            Self::Travel => "✈️",
            Self::Finance => "💰",
            Self::Simulation => "🔬",
        }
    }

    /// Format as an inline badge string for chat display.
    /// e.g. "🏭 Manufacturing" or "🔬 Simulation"
    pub fn badge(&self) -> String {
        format!("{} {}", self.icon(), self.display_name())
    }

    /// CSS-style color for mode badge backgrounds.
    pub fn color(&self) -> &'static str {
        match self {
            Self::General => "#3a3a4a",
            Self::Manufacturing => "#4a3a1a",
            Self::Warehousing => "#1a3a4a",
            Self::Fabrication => "#3a3a1a",
            Self::SupplyChain => "#1a4a3a",
            Self::Shopping => "#4a1a3a",
            Self::Travel => "#1a2a4a",
            Self::Finance => "#3a4a1a",
            Self::Simulation => "#2a1a4a",
        }
    }

    /// Keywords that trigger this mode when detected in user messages.
    /// The AI uses these to decide which modes to activate.
    pub fn trigger_keywords(&self) -> &'static [&'static str] {
        match self {
            Self::General => &[],
            Self::Manufacturing => &["manufacture", "factory", "production", "assembly", "investor", "patent", "BOM", "bill of materials", "prototype", "pilot"],
            Self::Warehousing => &["warehouse", "inventory", "storage", "fulfillment", "3PL", "pick", "pack", "reorder", "stock"],
            Self::Fabrication => &["CNC", "3D print", "mill", "lathe", "tooling", "machining", "fabricat", "laser cut"],
            Self::SupplyChain => &["supply chain", "logistics", "forecast", "demand", "supplier", "disruption", "recall", "shipment"],
            Self::Shopping => &["shop", "catalog", "pricing", "checkout", "marketplace", "e-commerce", "product listing"],
            Self::Travel => &["route", "fleet", "customs", "shipping", "freight", "transport", "delivery"],
            Self::Finance => &["tax", "finance", "budget", "cost analysis", "compliance", "revenue", "P&L", "accounting"],
            Self::Simulation => &["simulate", "script", "Rune", "Luau", "watchpoint", "breakpoint", "record", "playback", "physics"],
        }
    }

    /// System prompt fragment appended to the base Workshop prompt.
    pub fn system_prompt_fragment(&self) -> &'static str {
        match self {
            Self::General => GENERAL_PROMPT,
            Self::Manufacturing => MANUFACTURING_PROMPT,
            Self::Warehousing => WAREHOUSING_PROMPT,
            Self::Fabrication => FABRICATION_PROMPT,
            Self::SupplyChain => SUPPLY_CHAIN_PROMPT,
            Self::Shopping => SHOPPING_PROMPT,
            Self::Travel => TRAVEL_PROMPT,
            Self::Finance => FINANCE_PROMPT,
            Self::Simulation => SIMULATION_PROMPT,
        }
    }

    /// Greeting message when the mode is activated.
    pub fn greeting(&self) -> &'static str {
        match self {
            Self::General => "Workshop ready. I can create entities, execute Rune scripts, query material physics, read and write Universe files, and store persistent memories. What would you like to build?",
            Self::Manufacturing => "Manufacturing mode active. I can guide you through product ideation, find manufacturers, allocate investors, and generate all engineering artifacts. Describe your product idea.",
            Self::Warehousing => "Warehousing mode active. I can help with inventory management, storage optimization, pick/pack workflows, and 3PL integration.",
            Self::Fabrication => "Fabrication mode active. I can help with CNC programming, 3D print slicing, assembly line design, and tooling optimization.",
            Self::SupplyChain => "Supply Chain mode active. I can forecast demand, run scenario analyses, score supplier risk, trace recalls, and optimize logistics.",
            Self::Shopping => "Shopping mode active. I can help build product catalogs, pricing strategies, checkout flows, and marketplace listings.",
            Self::Travel => "Travel mode active. I can help with route planning, fleet management, customs documentation, and logistics optimization.",
            Self::Finance => "Finance mode active. I can help with tax calculations, compliance checks, financial reporting, and cost analysis.",
            Self::Simulation => "Simulation mode active. I have deep awareness of the running simulation via Eustress Streams. I can write Rune scripts, set watchpoints, control playback, and analyze results.",
        }
    }

    /// All domain modes (excludes General which is always active).
    pub fn all_domains() -> &'static [WorkshopMode] {
        &[
            Self::Manufacturing,
            Self::Simulation,
            Self::SupplyChain,
            Self::Warehousing,
            Self::Fabrication,
            Self::Shopping,
            Self::Travel,
            Self::Finance,
        ]
    }
}

// ---------------------------------------------------------------------------
// Active Modes (inferred, stackable)
// ---------------------------------------------------------------------------

/// Tracks which modes are currently active based on conversation context.
/// General is always active. Domain modes activate when the AI detects
/// relevant topics and can stack (e.g. Manufacturing + SupplyChain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveModes {
    /// Currently active domain modes (General is implicit).
    pub domains: Vec<WorkshopMode>,
}

impl Default for ActiveModes {
    fn default() -> Self {
        Self { domains: Vec::new() }
    }
}

impl ActiveModes {
    /// Get all active modes including General.
    pub fn all(&self) -> Vec<WorkshopMode> {
        let mut modes = vec![WorkshopMode::General];
        modes.extend_from_slice(&self.domains);
        modes
    }

    /// Check if a specific mode is active.
    pub fn is_active(&self, mode: WorkshopMode) -> bool {
        mode == WorkshopMode::General || self.domains.contains(&mode)
    }

    /// Activate a domain mode (no-op if already active).
    pub fn activate(&mut self, mode: WorkshopMode) {
        if mode != WorkshopMode::General && !self.domains.contains(&mode) {
            self.domains.push(mode);
        }
    }

    /// Deactivate a domain mode.
    pub fn deactivate(&mut self, mode: WorkshopMode) {
        self.domains.retain(|m| *m != mode);
    }

    /// Detect modes from a user message by keyword matching.
    /// Activates matching modes, returns which new modes were activated.
    pub fn detect_from_message(&mut self, message: &str) -> Vec<WorkshopMode> {
        let lower = message.to_lowercase();
        let mut newly_activated = Vec::new();

        for mode in WorkshopMode::all_domains() {
            if self.domains.contains(mode) { continue; }
            let triggered = mode.trigger_keywords().iter().any(|kw| lower.contains(&kw.to_lowercase()));
            if triggered {
                self.domains.push(*mode);
                newly_activated.push(*mode);
            }
        }

        newly_activated
    }

    /// Format active modes as badge text for chat display.
    /// e.g. "⚡ General  🏭 Manufacturing  🔗 Supply Chain"
    pub fn badges_text(&self) -> String {
        self.all().iter().map(|m| m.badge()).collect::<Vec<_>>().join("  ")
    }

    /// Format active modes as a compact system prompt fragment.
    /// Simulation mode uses the auto-generated API reference from rune_ecs_module.rs
    /// instead of the hand-maintained constant, so the agent always sees every registered function.
    pub fn system_prompt_fragments(&self) -> String {
        let mut out = String::new();
        for mode in &self.domains {
            out.push_str(&format!("\n## Active Mode: {} {}\n", mode.icon(), mode.display_name()));
            if *mode == WorkshopMode::Simulation {
                // Static preamble + auto-generated API reference from source
                out.push_str(SIMULATION_PREAMBLE);
                out.push('\n');
                let catalog = super::api_reference::ApiCatalog::build();
                out.push_str(&catalog.format_full_reference());
            } else {
                out.push_str(mode.system_prompt_fragment());
            }
            out.push('\n');
        }
        out
    }
}

// ---------------------------------------------------------------------------
// System prompt fragments per mode
// ---------------------------------------------------------------------------

const GENERAL_PROMPT: &str = r#"
You are the Eustress Workshop agent — an AI pair-programmer embedded inside EustressEngine.
You have full access to the running Universe via MCP tools. You can:
- Create, update, delete, and query entities in the 3D scene
- Read and write files in the Universe folder (.toml, .rune, .lua, .md, .json)
- Execute Rune scripts directly in the simulation
- Query physics properties (materials, fluids, electrochemistry)
- Perform spatial reasoning about the 3D world
- Store and recall persistent memories across sessions
- Access real-time simulation data via Eustress Streams

MODES: You have domain modes that activate based on what the user is discussing.
Multiple modes can be active simultaneously. When you detect a domain topic, prefix
your response with the relevant mode badges (e.g. "🏭 Manufacturing  🔗 Supply Chain").
Available modes and their icons:
- 🏭 Manufacturing — products, factories, investors, patents, BOM
- 📦 Warehousing — inventory, storage, fulfillment, 3PL
- ⚙️ Fabrication — CNC, 3D printing, tooling, machining
- 🔗 Supply Chain — demand forecasting, suppliers, logistics, recalls
- 🛒 Shopping — catalogs, pricing, checkout, marketplace
- ✈️ Travel — routes, fleet, customs, shipping
- 💰 Finance — taxes, compliance, budgets, cost analysis
- 🔬 Simulation — Rune scripting, physics, watchpoints, recording

Always use tools when you need to interact with the engine. Be concise and technical.
When proposing file changes, use the write_file tool to make them directly.
Format mode badges as inline labels at the start of responses when domain modes are relevant.
"#;

const MANUFACTURING_PROMPT: &str = r#"
You are in Manufacturing mode — helping the user design and manufacture physical products.
You have access to the full manufacturing pipeline:
- Product ideation and brief normalization
- Patent drafting and SOTA validation
- Manufacturer and investor matching
- Cost estimation and deal structuring
- Logistics and warehousing planning

Guide the user through the ideation process. Ask clarifying questions about
materials, dimensions, chemistry, form factor, and target market.
When ready, use the normalize_brief tool to structure the conversation into a TOML brief.
"#;

const WAREHOUSING_PROMPT: &str = r#"
You are in Warehousing mode — managing inventory, storage, and fulfillment operations.
Help with warehouse layout, pick/pack optimization, inventory levels, reorder points,
3PL integration, and storage cost analysis.
"#;

const FABRICATION_PROMPT: &str = r#"
You are in Fabrication mode — assisting with CNC programming, 3D printing,
assembly line design, tooling selection, and manufacturing process optimization.
"#;

const SUPPLY_CHAIN_PROMPT: &str = r#"
You are in Supply Chain mode — forecasting demand, analyzing scenarios,
scoring supplier risk, tracing recalls, and optimizing multi-echelon logistics.
Use the scenario tools for Monte Carlo analysis and Bayesian probability updates.
"#;

const SHOPPING_PROMPT: &str = r#"
You are in Shopping mode — building product catalogs, pricing strategies,
checkout flows, marketplace listings, and customer analytics.
"#;

const TRAVEL_PROMPT: &str = r#"
You are in Travel mode — planning routes, managing fleets, handling customs
documentation, and optimizing transportation logistics.
"#;

const FINANCE_PROMPT: &str = r#"
You are in Finance mode — calculating taxes, checking compliance, generating
financial reports, analyzing costs, and managing budgets.
"#;

/// Preamble for Simulation mode — the API reference section is auto-generated
/// from rune_ecs_module.rs at startup via `ScriptingApiReference::build()`.
const SIMULATION_PREAMBLE: &str = r#"
You are in Simulation mode — deeply aware of the running simulation via Eustress Streams.
You can:
- Write and execute Rune scripts that interact with the ECS world
- Write and execute Luau scripts with full Roblox API compatibility
- Set watchpoints and breakpoints on simulation variables
- Control simulation playback (play, pause, step, time compression)
- Record simulation runs and export data
- Analyze real-time stream events for anomalies

Use the simulation tools to observe and control the running world.
Reference specific entity names and properties from the live data model.
Use execute_rune or execute_luau to write scripts — the engine hot-reloads them.
The full API reference below is auto-generated from the engine source.
"#;

const SIMULATION_PROMPT: &str = r#"
You are in Simulation mode — deeply aware of the running simulation via Eustress Streams.
You can:
- Write and execute Rune scripts that interact with the ECS world
- Set watchpoints and breakpoints on simulation variables
- Control simulation playback (play, pause, step, time compression)
- Record simulation runs and export data
- Analyze real-time stream events for anomalies

Use the simulation tools to observe and control the running world.
Reference specific entity names and properties from the live data model.

## Rune Scripting API Reference

### Simulation Values
- `get_sim_value(key)` → f64 — read watchpoint
- `set_sim_value(key, value)` — write watchpoint
- `list_sim_values()` → Vec<(String, f64)> — all watchpoints

### Entity Operations
- `query_workspace_entities(class_filter?)` → Vec<(name, class)>
- `instance_delete(name)` → bool — delete entity + mesh binary
- `part_set_position(name, x, y, z)` — move entity
- `part_set_rotation(name, rx, ry, rz)` — rotate (degrees)
- `part_set_size(name, x, y, z)` — resize
- `part_set_color(name, r, g, b)` — set color (0-1 range)
- `part_set_material(name, material)` — 19 presets
- `part_set_transparency(name, t)` — 0.0 opaque, 1.0 invisible
- `part_set_anchored(name, bool)` — fix/unfix
- `part_set_can_collide(name, bool)` — collision toggle

### Physics
- `part_apply_impulse(name, x, y, z)` — linear force (kg·m/s)
- `part_apply_angular_impulse(name, x, y, z)` — torque
- `part_get_mass(name)` → f64 — mass in kg
- `part_get_velocity(name)` → (x, y, z) — m/s
- `part_set_velocity(name, x, y, z)` — set velocity directly
- `workspace_get_gravity()` → f64 — m/s² (default 9.80665)
- `workspace_set_gravity(val)` — change gravity

### Camera
- `camera_get_position()` → (x, y, z)
- `camera_get_look_vector()` → (x, y, z) unit vector
- `camera_get_fov()` → degrees
- `camera_set_fov(degrees)`
- `camera_screen_point_to_ray(x, y)` → ((ox,oy,oz), (dx,dy,dz))

### Mouse
- `mouse_get_hit()` → (x, y, z) world-space cursor hit position
- `mouse_get_target()` → String entity name under cursor

### Raycasting
- `workspace_raycast(origin, direction, params?)` → RaycastResult
- `workspace_raycast_all(origin, direction, params?, max_hits)` → Vec

### Files
- `read_space_file(path)` → String
- `write_space_file(path, content)` → bool
- `query_material_properties(name)` → (roughness, metallic, reflectance)

### Attributes
- `instance_set_attribute(entity_name, key, value)`
- `instance_get_attribute(entity_name, key)` → Option<String>

### Tags
- `collection_add_tag(entity_id, tag)`
- `collection_remove_tag(entity_id, tag)`
- `collection_has_tag(entity_id, tag)` → bool
- `collection_get_tagged(tag)` → Vec<i64>

### Logging
- `log_info(message)`, `log_warn(message)`, `log_error(message)`

### HTTP
- `http_get_async(url)` → Option<String>
- `http_post_async(url, body)` → Option<String>
- `http_request_async(url, method, body?, headers?)` → HttpResponse

### Data Types
- `Vector3 { x, y, z }` — add, sub, mul, div, dot, cross, magnitude, unit, lerp
- `CFrame` — new, angles, lookAt, inverse, toWorldSpace, toObjectSpace
- `Color3` — new, fromRGB, fromHSV, fromHex, lerp, toHSV
"#;
