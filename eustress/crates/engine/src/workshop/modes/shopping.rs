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

        // A margin of 1.0 or more divided by zero or went negative.
        if !(0.0..1.0).contains(&margin) || cost < 0.0 || !cost.is_finite() {
            return ToolResult {
                tool_name: "price_product".to_string(), tool_use_id: String::new(),
                success: false,
                content: "price_product needs a target_margin from 0.0 up to (not including) 1.0 and a non-negative unit_cost_usd.".to_string(),
                structured_data: None,
                stream_topic: None,
            };
        }
        let cost_plus = cost / (1.0 - margin);
        // The mean of the prices given. With none, the cost-plus price
        // stands in (an empty list used to average to 0 and drag the
        // recommendation to a third of cost-plus).
        let prices: Vec<f64> = input.get("competitor_prices").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_default();
        let competitive_avg = if prices.is_empty() { cost_plus } else { prices.iter().sum::<f64>() / prices.len() as f64 };
        let value_based = competitive_avg * premium;
        let recommended = cost_plus * 0.3 + competitive_avg * 0.3 + value_based * 0.4;

        ToolResult {
            tool_name: "price_product".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: recommended ${:.2} (cost-plus: ${:.2}, competitive: ${:.2}, value: ${:.2})", product, recommended, cost_plus, competitive_avg, value_based),
            structured_data: Some(serde_json::json!({
                "product": product, "recommended_price": recommended,
                "cost_plus": cost_plus, "competitive_avg": competitive_avg, "value_based": value_based,
                "margin_at_recommended": (if recommended > 0.0 { Some((recommended - cost) / recommended) } else { None }),
            })),
            stream_topic: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workshop::modes::test_context;
    use serde_json::json;

    fn price(input: serde_json::Value) -> ToolResult {
        PriceProductTool.execute(input, &test_context())
    }

    #[test]
    fn no_competitor_prices_falls_back_to_cost_plus() {
        let r = price(json!({ "product": "p", "unit_cost_usd": 60.0, "target_margin": 0.4, "competitor_prices": [] }));
        let d = r.structured_data.unwrap();
        // cost-plus is 60 / 0.6 = 100; with nothing to compare it stands in for all three terms.
        assert!((d["competitive_avg"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        assert!((d["recommended_price"].as_f64().unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_margin_of_one_or_more_is_refused() {
        assert!(!price(json!({ "product": "p", "unit_cost_usd": 10.0, "target_margin": 1.0 })).success);
        assert!(!price(json!({ "product": "p", "unit_cost_usd": 10.0, "target_margin": 1.5 })).success);
        assert!(!price(json!({ "product": "p", "unit_cost_usd": -1.0 })).success);
    }
}
