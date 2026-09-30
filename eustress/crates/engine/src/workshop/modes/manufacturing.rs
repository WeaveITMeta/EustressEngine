//! Manufacturing mode — wraps the existing IdeationPipeline.
//!
//! This mode exposes the 11-step manufacturing pipeline as MCP tools:
//! normalize_brief, generate_patent, sota_validation, generate_requirements,
//! generate_meshes, generate_parts, generate_sim_scripts, generate_ui,
//! finalize_catalog, generate_deal_structure, generate_logistics_plan.
//!
//! The existing IdeationPipeline state machine, normalizer, artifact_gen,
//! and claude_bridge all continue to work unchanged. This module provides
//! tool wrappers that trigger the same underlying pipeline steps.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Normalize Brief Tool
// ---------------------------------------------------------------------------

pub struct NormalizeBriefTool;

impl ToolHandler for NormalizeBriefTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "normalize_brief",
            description: "Convert the current conversation into a structured ideation_brief.toml. Extracts product name, innovations, target specs, bill of materials, physics model, and deal structure from the accumulated chat context. Writes to Workspace/{product}/ideation_brief.toml.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product_name": { "type": "string", "description": "Product name to use for the output directory" }
                },
                "required": ["product_name"]
            }),
            modes: &[WorkshopMode::Manufacturing],
            requires_approval: true,
            stream_topics: &["workshop.tool.normalize_brief"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let product_name = input.get("product_name").and_then(|v| v.as_str()).unwrap_or("Product");
        let safe_name = product_name.replace(' ', "_").replace('/', "_").replace(':', "_");
        let output_dir = ctx.space_root.join("Workspace").join(&safe_name);
        let _ = std::fs::create_dir_all(&output_dir);

        ToolResult {
            tool_name: "normalize_brief".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Normalization requested for '{}'. The pipeline will convert the conversation into ideation_brief.toml at {}", product_name, output_dir.display()),
            structured_data: Some(serde_json::json!({
                "action": "normalize_brief",
                "product_name": product_name,
                "output_dir": output_dir.to_string_lossy(),
            })),
            stream_topic: Some("workshop.tool.normalize_brief".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Query Manufacturers Tool
// ---------------------------------------------------------------------------

pub struct QueryManufacturersTool;

impl ToolHandler for QueryManufacturersTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_manufacturers",
            description: "Search the manufacturing program registry for manufacturers matching a capability query. Filters by process type (injection molding, CNC, SMT, 3D printing), materials, certifications (ISO 9001, UL, CE, REACH), and minimum production capacity.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "process": { "type": "string", "description": "Manufacturing process: injection_molding, cnc_machining, smt_assembly, 3d_printing, die_casting, sheet_metal, extrusion" },
                    "materials": { "type": "array", "items": { "type": "string" }, "description": "Required materials: aluminum, steel, abs, polycarbonate, titanium, copper, silicon" },
                    "certifications": { "type": "array", "items": { "type": "string" }, "description": "Required certifications: ISO_9001, UL, CE, REACH, RoHS, FCC, IATF_16949" },
                    "min_capacity": { "type": "integer", "description": "Minimum monthly unit capacity" }
                }
            }),
            modes: &[WorkshopMode::Manufacturing],
            requires_approval: false,
            stream_topics: &["workshop.tool.query_manufacturers"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        // The same vendors the RFQ Builder offers: the Space's
        // `DataService/Vendors` folders, read here because tools run off the
        // main thread and cannot reach the ECS registry.
        let registry = crate::manufacturing::ManufacturingProgramRegistry::load_space(&ctx.space_root);
        let norm = |s: &str| s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let strings = |key: &str| -> Vec<String> {
            input
                .get(key)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(norm).collect())
                .unwrap_or_default()
        };
        let process = input.get("process").and_then(|v| v.as_str()).map(norm).filter(|p| !p.is_empty());
        let materials = strings("materials");
        let certifications = strings("certifications");
        let min_capacity = input.get("min_capacity").and_then(|v| v.as_u64()).unwrap_or(0);

        let has_certification = |m: &crate::manufacturing::Manufacturer, want: &str| {
            let c = &m.certifications;
            match want {
                "iso_9001" => c.iso_9001,
                "iso_14001" => c.iso_14001,
                "iatf_16949" => c.iatf_16949,
                "ul" => c.ul_certified,
                "ul_battery" => c.ul_battery_certified,
                "reach" => c.reach_compliant,
                "rohs" => c.rohs_compliant,
                other => c.additional.iter().any(|a| norm(a) == other),
            }
        };
        let matches: Vec<&crate::manufacturing::Manufacturer> = registry
            .manufacturers
            .iter()
            .filter(|m| {
                process
                    .as_ref()
                    .map_or(true, |p| m.capabilities.processes.iter().any(|x| norm(x) == *p))
            })
            // "steel" finds "stainless_steel"; "aluminum" finds "aluminum_alloys".
            .filter(|m| {
                materials
                    .iter()
                    .all(|want| m.capabilities.materials.iter().any(|x| norm(x).contains(want.as_str())))
            })
            .filter(|m| certifications.iter().all(|want| has_certification(*m, want.as_str())))
            .filter(|m| u64::from(m.capacity.monthly_units_available) >= min_capacity)
            .collect();

        let rows: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "name": m.name,
                    "status": m.status,
                    "country": m.country,
                    "region": m.region,
                    "lead_time_days": m.lead_time_days,
                    "processes": m.capabilities.processes,
                    "materials": m.capabilities.materials,
                    "monthly_units_available": m.capacity.monthly_units_available,
                    "min_order_quantity": m.capacity.min_order_quantity,
                })
            })
            .collect();

        let vendors_dir = ctx.space_root.join(crate::manufacturing::VENDORS_DIR);
        let content = if registry.manufacturers.is_empty() {
            format!(
                "No vendors are registered in this Space ({}). Add them with the Procurement panel's vendor registry, then query again.",
                vendors_dir.display()
            )
        } else if matches.is_empty() {
            format!(
                "None of the {} registered vendors match these filters.",
                registry.manufacturers.len()
            )
        } else {
            let lines: Vec<String> = matches
                .iter()
                .map(|m| {
                    format!(
                        "- {} ({}, {:?}): {} units/month, lead time {} days",
                        m.name, m.country, m.status, m.capacity.monthly_units_available, m.lead_time_days
                    )
                })
                .collect();
            format!(
                "{} of {} registered vendors match:\n{}",
                matches.len(),
                registry.manufacturers.len(),
                lines.join("\n")
            )
        };

        ToolResult {
            tool_name: "query_manufacturers".to_string(),
            tool_use_id: String::new(),
            success: true,
            content,
            structured_data: Some(serde_json::json!({
                "action": "query_manufacturers",
                "filters": input,
                "registered": registry.manufacturers.len(),
                "manufacturers": rows,
            })),
            stream_topic: Some("workshop.tool.query_manufacturers".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Query Investors Tool
// ---------------------------------------------------------------------------

pub struct QueryInvestorsTool;

impl ToolHandler for QueryInvestorsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_investors",
            description: "Search the manufacturing program registry for investors matching a product vertical. Filters by investor type (individual, venture_fund, family_office, strategic_corporate), minimum check size, and target vertical (consumer_electronics, energy_storage, medical_devices, industrial, automotive, aerospace).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "vertical": { "type": "string", "description": "Product vertical: consumer_electronics, energy_storage, medical_devices, industrial, automotive, aerospace, agriculture, defense" },
                    "min_check_usd": { "type": "number", "description": "Minimum investment check size in USD" },
                    "investor_type": { "type": "string", "description": "Investor type: individual, venture_fund, family_office, strategic_corporate" }
                }
            }),
            modes: &[WorkshopMode::Manufacturing],
            requires_approval: false,
            stream_topics: &["workshop.tool.query_investors"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult {
            tool_name: "query_investors".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: "Querying investor registry...".to_string(),
            structured_data: Some(serde_json::json!({
                "action": "query_investors",
                "filters": input,
            })),
            stream_topic: Some("workshop.tool.query_investors".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Allocate Product Tool
// ---------------------------------------------------------------------------

pub struct AllocateProductTool;

impl ToolHandler for AllocateProductTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "allocate_product",
            description: "Run the AI allocation engine to select the optimal manufacturer and investor set for a product. Requires a normalized ideation_brief.toml. Scores manufacturers on capability (40%), quality (25%), cost (20%), speed (10%), risk (5%). Returns selected manufacturer, investor allocations with equity stakes, and confidence score.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product_name": { "type": "string", "description": "Product name (must have existing ideation_brief.toml)" },
                    "target_capital_usd": { "type": "number", "description": "Total capital needed for pilot manufacturing run" },
                    "target_unit_cost_usd": { "type": "number", "description": "Target per-unit manufacturing cost" },
                    "pilot_quantity": { "type": "integer", "description": "Number of units for pilot batch" }
                },
                "required": ["product_name", "target_capital_usd"]
            }),
            modes: &[WorkshopMode::Manufacturing],
            requires_approval: true,
            stream_topics: &["workshop.tool.allocate_product"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let product_name = input.get("product_name").and_then(|v| v.as_str()).unwrap_or("Product");
        let capital = input.get("target_capital_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);

        ToolResult {
            tool_name: "allocate_product".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Running allocation engine for '{}' (${:.0} target capital)...", product_name, capital),
            structured_data: Some(serde_json::json!({
                "action": "allocate_product",
                "product_name": product_name,
                "target_capital_usd": capital,
                "filters": input,
            })),
            stream_topic: Some("workshop.tool.allocate_product".to_string()),
        }
    }
}
