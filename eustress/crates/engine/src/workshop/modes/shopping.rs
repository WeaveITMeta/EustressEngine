//! Shopping mode — product pricing, catalog management, marketplace listings.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct PriceProductTool;

impl ToolHandler for PriceProductTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "price_product",
            description: "Calculate optimal pricing for a product using cost-plus, competitive, and value-based strategies. Takes unit cost, competitor prices, and perceived value to recommend a price point with projected margin and price elasticity estimate.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product": { "type": "string", "description": "Product name" },
                    "unit_cost_usd": { "type": "number", "description": "Total landed cost per unit" },
                    "competitor_prices": { "type": "array", "items": { "type": "number" }, "description": "Competitor prices for similar products" },
                    "target_margin": { "type": "number", "description": "Target gross margin (0.0-1.0, default: 0.40)", "default": 0.40 },
                    "premium_factor": { "type": "number", "description": "Brand premium multiplier (1.0 = commodity, 1.5 = premium, 2.0 = luxury)", "default": 1.0 }
                },
                "required": ["product", "unit_cost_usd"]
            }),
            modes: &[WorkshopMode::Shopping],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let product = input.get("product").and_then(|v| v.as_str()).unwrap_or("Product");
        let cost = input.get("unit_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let margin = input.get("target_margin").and_then(|v| v.as_f64()).unwrap_or(0.40);
        let premium = input.get("premium_factor").and_then(|v| v.as_f64()).unwrap_or(1.0);

        let cost_plus = cost / (1.0 - margin);
        let competitive_avg = input.get("competitor_prices").and_then(|v| v.as_array())
            .map(|a| { let sum: f64 = a.iter().filter_map(|v| v.as_f64()).sum(); sum / a.len().max(1) as f64 })
            .unwrap_or(cost_plus);
        let value_based = competitive_avg * premium;
        let recommended = (cost_plus * 0.3 + competitive_avg * 0.3 + value_based * 0.4);

        ToolResult {
            tool_name: "price_product".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: recommended ${:.2} (cost-plus: ${:.2}, competitive: ${:.2}, value: ${:.2})", product, recommended, cost_plus, competitive_avg, value_based),
            structured_data: Some(serde_json::json!({
                "product": product, "recommended_price": recommended,
                "cost_plus": cost_plus, "competitive_avg": competitive_avg, "value_based": value_based,
                "margin_at_recommended": (recommended - cost) / recommended,
            })),
            stream_topic: None,
        }
    }
}
