//! `WorkshopMode` — the mode enum that gates which tools each active
//! Claude conversation has access to, plus every mode's self-descriptive
//! metadata (display name, icon, color, trigger keywords, system prompt
//! fragment, greeting).
//!
//! Lives in the shared crate so tool definitions can reference
//! `&[WorkshopMode]` in their `modes` field without needing the engine
//! as a dependency. Rust's orphan rules forbid inherent `impl` blocks
//! on foreign types, so every method that belongs to `WorkshopMode`
//! must live here too — including the per-mode prompt constants.
//!
//! Mode orchestration (`ActiveModes`: domain tracking, keyword-triggered
//! activation, system-prompt composition with live API-reference
//! injection) stays in the engine because it depends on engine-private
//! state (api_reference catalog, streaming context).

use serde::{Deserialize, Serialize};

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
    /// Universe/Space/Script browsing — historically MCP-only tools.
    /// Exposed to Workshop for completeness but not advertised in the
    /// default "General" tool list so the agent's surface stays tight.
    UniverseBrowsing,
}

impl WorkshopMode {
    /// Every variant — used by `tools/list` callers that want the
    /// complete tool catalogue without per-mode filtering.
    pub const ALL: &'static [WorkshopMode] = &[
        WorkshopMode::General,
        WorkshopMode::Manufacturing,
        WorkshopMode::Warehousing,
        WorkshopMode::Fabrication,
        WorkshopMode::SupplyChain,
        WorkshopMode::Shopping,
        WorkshopMode::Travel,
        WorkshopMode::Finance,
        WorkshopMode::Simulation,
        WorkshopMode::UniverseBrowsing,
    ];

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
            Self::UniverseBrowsing => "Universe Browser",
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
            Self::UniverseBrowsing => "🗺️",
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
            Self::UniverseBrowsing => "#2a3a3a",
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
            Self::UniverseBrowsing => &["universe", "space", "browse", "list scripts", "find entity"],
        }
    }

    /// System prompt fragment appended to the base Workshop prompt.
    ///
    /// Simulation's fragment is a static preamble — the live
    /// auto-generated API reference is injected by the engine's
    /// `ActiveModes::system_prompt_fragments` because it depends on
    /// engine-private state (the `ApiCatalog`). Callers that want
    /// just the static slice use this method directly.
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
            Self::Simulation => SIMULATION_PREAMBLE,
            Self::UniverseBrowsing => UNIVERSE_BROWSING_PROMPT,
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
            Self::UniverseBrowsing => "Universe Browser mode active. I can list Universes and Spaces, find entities, read scripts, and search across the Universe.",
        }
    }

    /// All domain modes (excludes General which is always active).
    /// UniverseBrowsing is excluded from keyword auto-activation to
    /// keep casual conversations from unintentionally enabling it.
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

/// Preamble for Simulation mode. The engine extends this with a live
/// auto-generated API reference from `ScriptingApiReference::build()`
/// because the reference evolves with every Rune/Luau binding change
/// and can't be hand-maintained here without drift.
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

const UNIVERSE_BROWSING_PROMPT: &str = r#"
You are in Universe Browser mode — introspecting the user's Universe folder structure.
You can list Universes and Spaces, enumerate scripts + assets, find entities by name,
read script sources, and do text search across the Universe. Use these tools to orient
yourself before making changes via the manipulation tools in General mode.
"#;
