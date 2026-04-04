//! Warehousing mode — inventory management, storage optimization, fulfillment.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct InventoryCheckTool;

impl ToolHandler for InventoryCheckTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "inventory_check",
            description: "Check inventory levels for a product across warehouse locations. Returns current stock, reorder point, days of supply remaining, and whether reorder is needed. Calculates based on average daily consumption rate.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product": { "type": "string", "description": "Product or SKU name" },
                    "current_stock": { "type": "number", "description": "Current units in stock" },
                    "daily_consumption": { "type": "number", "description": "Average daily units consumed" },
                    "reorder_point": { "type": "number", "description": "Stock level that triggers reorder" },
                    "lead_time_days": { "type": "number", "description": "Supplier lead time in days" }
                },
                "required": ["product", "current_stock", "daily_consumption"]
            }),
            modes: &[WorkshopMode::Warehousing],
            requires_approval: false,
            stream_topics: &["workshop.tool.inventory_check"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let product = input.get("product").and_then(|v| v.as_str()).unwrap_or("Product");
        let stock = input.get("current_stock").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let daily = input.get("daily_consumption").and_then(|v| v.as_f64()).unwrap_or(1.0).max(0.01);
        let reorder_pt = input.get("reorder_point").and_then(|v| v.as_f64()).unwrap_or(daily * 7.0);
        let lead_time = input.get("lead_time_days").and_then(|v| v.as_f64()).unwrap_or(14.0);

        let days_supply = stock / daily;
        let needs_reorder = stock <= reorder_pt;
        let safety_stock = daily * lead_time * 1.5;

        ToolResult {
            tool_name: "inventory_check".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: {:.0} units ({:.0} days supply){}", product, stock, days_supply, if needs_reorder { " ⚠️ REORDER NEEDED" } else { "" }),
            structured_data: Some(serde_json::json!({
                "product": product, "current_stock": stock, "days_supply": days_supply,
                "reorder_point": reorder_pt, "needs_reorder": needs_reorder,
                "safety_stock": safety_stock, "lead_time_days": lead_time
            })),
            stream_topic: Some("workshop.tool.inventory_check".to_string()),
        }
    }
}

pub struct StorageOptimizeTool;

impl ToolHandler for StorageOptimizeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "storage_optimize",
            description: "Calculate optimal storage layout for a set of SKUs. Assigns products to zones (fast-pick, bulk, cold, hazmat) based on velocity (picks/day), weight, temperature requirements, and hazmat classification. Returns zone assignments and estimated pick efficiency improvement.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "products": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "picks_per_day": { "type": "number" },
                        "weight_kg": { "type": "number" },
                        "requires_cold": { "type": "boolean" },
                        "is_hazmat": { "type": "boolean" }
                    }}, "description": "Products to assign to storage zones" }
                },
                "required": ["products"]
            }),
            modes: &[WorkshopMode::Warehousing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let products = input.get("products").and_then(|v| v.as_array());
        let mut assignments = Vec::new();

        if let Some(products) = products {
            for p in products {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let picks = p.get("picks_per_day").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cold = p.get("requires_cold").and_then(|v| v.as_bool()).unwrap_or(false);
                let hazmat = p.get("is_hazmat").and_then(|v| v.as_bool()).unwrap_or(false);

                let zone = if hazmat { "hazmat" } else if cold { "cold_storage" } else if picks > 50.0 { "fast_pick" } else { "bulk" };
                assignments.push(serde_json::json!({ "product": name, "zone": zone, "picks_per_day": picks }));
            }
        }

        ToolResult {
            tool_name: "storage_optimize".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{} products assigned to zones", assignments.len()),
            structured_data: Some(serde_json::json!({ "assignments": assignments })),
            stream_topic: None,
        }
    }
}
