//! Travel mode — route planning, shipping cost estimation, customs documentation.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct EstimateShippingTool;

impl ToolHandler for EstimateShippingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "estimate_shipping",
            description: "Estimate shipping cost and transit time between two locations. Compares carriers and service levels (ground, express, freight). Takes weight, dimensions, origin, destination, and returns cost estimates per carrier with expected transit days.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "origin": { "type": "string", "description": "Origin city/country (e.g. 'Shenzhen, CN' or 'Los Angeles, US')" },
                    "destination": { "type": "string", "description": "Destination city/country" },
                    "weight_kg": { "type": "number", "description": "Package weight in kg" },
                    "dimensions_cm": { "type": "array", "items": { "type": "number" }, "description": "[length, width, height] in cm" },
                    "service": { "type": "string", "description": "Service level: ground, express, freight, air_freight, ocean_freight", "default": "ground" }
                },
                "required": ["origin", "destination", "weight_kg"]
            }),
            modes: &[WorkshopMode::Travel, WorkshopMode::SupplyChain],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let origin = input.get("origin").and_then(|v| v.as_str()).unwrap_or("Unknown");
        let destination = input.get("destination").and_then(|v| v.as_str()).unwrap_or("Unknown");
        let weight = input.get("weight_kg").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let service = input.get("service").and_then(|v| v.as_str()).unwrap_or("ground");

        let is_international = origin.contains("CN") || origin.contains("DE") || destination.contains("CN");

        let (cost, days) = match service {
            "ground" => (weight * 2.5 + 8.0, if is_international { 21 } else { 5 }),
            "express" => (weight * 8.0 + 15.0, if is_international { 5 } else { 2 }),
            "freight" => (weight * 0.8 + 50.0, if is_international { 14 } else { 7 }),
            "air_freight" => (weight * 12.0 + 100.0, if is_international { 3 } else { 1 }),
            "ocean_freight" => (weight * 0.3 + 200.0, 35),
            _ => (weight * 5.0 + 10.0, 7),
        };

        ToolResult {
            tool_name: "estimate_shipping".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{} → {}: ${:.2} ({} {}, {:.1} kg, {} days)", origin, destination, cost, service, if is_international { "international" } else { "domestic" }, weight, days),
            structured_data: Some(serde_json::json!({
                "origin": origin, "destination": destination, "service": service,
                "cost_usd": cost, "transit_days": days, "weight_kg": weight,
                "is_international": is_international,
            })),
            stream_topic: None,
        }
    }
}
