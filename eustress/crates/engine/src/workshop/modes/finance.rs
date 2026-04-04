//! Finance mode — tax calculation, cost analysis, margin computation, compliance.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct CalculateCostTool;

impl ToolHandler for CalculateCostTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calculate_cost",
            description: "Calculate total landed cost for a product including BOM cost, assembly labor, logistics, packaging, import duties, and returns allowance. Returns per-unit cost breakdown with gross margin at a given retail price.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product": { "type": "string", "description": "Product name" },
                    "bom_cost_usd": { "type": "number", "description": "Bill of materials cost per unit" },
                    "assembly_hours": { "type": "number", "description": "Assembly labor hours per unit" },
                    "labor_rate_usd": { "type": "number", "description": "Labor rate per hour (default: $25)", "default": 25 },
                    "shipping_cost_usd": { "type": "number", "description": "Shipping cost per unit" },
                    "packaging_cost_usd": { "type": "number", "description": "Packaging cost per unit", "default": 2.0 },
                    "duty_rate": { "type": "number", "description": "Import duty rate (0.0-1.0)", "default": 0.0 },
                    "returns_rate": { "type": "number", "description": "Expected return rate (0.0-1.0)", "default": 0.05 },
                    "retail_price_usd": { "type": "number", "description": "Target retail price for margin calculation" }
                },
                "required": ["product", "bom_cost_usd"]
            }),
            modes: &[WorkshopMode::Finance, WorkshopMode::Manufacturing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let product = input.get("product").and_then(|v| v.as_str()).unwrap_or("Product");
        let bom = input.get("bom_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let hours = input.get("assembly_hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let rate = input.get("labor_rate_usd").and_then(|v| v.as_f64()).unwrap_or(25.0);
        let shipping = input.get("shipping_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let packaging = input.get("packaging_cost_usd").and_then(|v| v.as_f64()).unwrap_or(2.0);
        let duty = input.get("duty_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let returns = input.get("returns_rate").and_then(|v| v.as_f64()).unwrap_or(0.05);
        let retail = input.get("retail_price_usd").and_then(|v| v.as_f64());

        let labor = hours * rate;
        let subtotal = bom + labor + shipping + packaging;
        let duties = subtotal * duty;
        let returns_cost = subtotal * returns;
        let total = subtotal + duties + returns_cost;

        let margin = retail.map(|r| ((r - total) / r * 100.0));

        ToolResult {
            tool_name: "calculate_cost".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: ${:.2}/unit total landed cost{}", product, total,
                margin.map(|m| format!(" ({:.1}% gross margin at ${:.2})", m, retail.unwrap_or(0.0))).unwrap_or_default()),
            structured_data: Some(serde_json::json!({
                "product": product, "breakdown": {
                    "bom": bom, "labor": labor, "shipping": shipping, "packaging": packaging,
                    "duties": duties, "returns_allowance": returns_cost
                },
                "total_cost": total, "gross_margin_pct": margin,
            })),
            stream_topic: None,
        }
    }
}

pub struct EstimateTaxTool;

impl ToolHandler for EstimateTaxTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "estimate_tax",
            description: "Estimate sales tax or VAT for a transaction based on jurisdiction. Returns tax amount, effective rate, and applicable tax type (sales tax, VAT, GST). Supports US state sales tax, EU VAT, Canadian GST/HST.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "amount_usd": { "type": "number", "description": "Transaction amount before tax" },
                    "jurisdiction": { "type": "string", "description": "Tax jurisdiction (e.g. 'US-CA', 'US-TX', 'EU-DE', 'CA-ON')" },
                    "product_type": { "type": "string", "description": "Product type for exemptions: physical, digital, food, clothing", "default": "physical" }
                },
                "required": ["amount_usd", "jurisdiction"]
            }),
            modes: &[WorkshopMode::Finance],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let amount = input.get("amount_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let jurisdiction = input.get("jurisdiction").and_then(|v| v.as_str()).unwrap_or("US-CA");

        let (rate, tax_type) = match jurisdiction {
            "US-CA" => (0.0725, "Sales Tax"), "US-TX" => (0.0625, "Sales Tax"),
            "US-NY" => (0.08, "Sales Tax"), "US-WA" => (0.065, "Sales Tax"),
            "US-FL" => (0.06, "Sales Tax"), "US-OR" => (0.0, "No Sales Tax"),
            "US-NH" => (0.0, "No Sales Tax"), "US-MT" => (0.0, "No Sales Tax"),
            j if j.starts_with("EU-") => (0.20, "VAT"),
            "EU-DE" => (0.19, "VAT"), "EU-FR" => (0.20, "VAT"), "EU-NL" => (0.21, "VAT"),
            "CA-ON" => (0.13, "HST"), "CA-BC" => (0.12, "GST+PST"), "CA-AB" => (0.05, "GST"),
            "GB" => (0.20, "VAT"), "AU" => (0.10, "GST"), "JP" => (0.10, "Consumption Tax"),
            _ => (0.0, "Unknown"),
        };

        let tax = amount * rate;

        ToolResult {
            tool_name: "estimate_tax".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: ${:.2} {} ({:.1}%) on ${:.2} → ${:.2} total", jurisdiction, tax, tax_type, rate * 100.0, amount, amount + tax),
            structured_data: Some(serde_json::json!({
                "jurisdiction": jurisdiction, "tax_type": tax_type, "rate": rate,
                "tax_amount": tax, "subtotal": amount, "total": amount + tax,
            })),
            stream_topic: None,
        }
    }
}
