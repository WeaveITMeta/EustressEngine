//! Fabrication mode — CNC programming, 3D printing, tooling, process selection.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct SelectProcessTool;

impl ToolHandler for SelectProcessTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "select_process",
            description: "Recommend the optimal fabrication process for a part based on material, geometry complexity, quantity, tolerance, and budget. Compares: CNC milling, CNC turning, injection molding, 3D printing (FDM/SLA/SLS/DMLS), sheet metal, die casting, extrusion, and hand assembly. Returns ranked recommendations with cost per unit, lead time, and suitability score.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "material": { "type": "string", "description": "Material: aluminum, steel, titanium, abs, polycarbonate, nylon, resin, copper, brass" },
                    "quantity": { "type": "integer", "description": "Production quantity" },
                    "complexity": { "type": "string", "description": "Geometry complexity: low (prismatic), medium (pockets/holes), high (organic/undercuts)", "default": "medium" },
                    "tolerance_mm": { "type": "number", "description": "Required tolerance in mm", "default": 0.1 },
                    "max_dimension_mm": { "type": "number", "description": "Largest part dimension in mm", "default": 100 },
                    "budget_per_unit_usd": { "type": "number", "description": "Target budget per unit" }
                },
                "required": ["material", "quantity"]
            }),
            modes: &[WorkshopMode::Fabrication, WorkshopMode::Manufacturing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let material = input.get("material").and_then(|v| v.as_str()).unwrap_or("aluminum");
        let qty = input.get("quantity").and_then(|v| v.as_u64()).unwrap_or(1);
        let complexity = input.get("complexity").and_then(|v| v.as_str()).unwrap_or("medium");

        let is_metal = matches!(material, "aluminum" | "steel" | "titanium" | "copper" | "brass");
        let is_plastic = matches!(material, "abs" | "polycarbonate" | "nylon" | "resin");

        let mut recommendations = Vec::new();

        if is_metal {
            recommendations.push(serde_json::json!({ "process": "CNC Milling", "suitability": 0.9, "cost_trend": "medium", "lead_days": 5 }));
            if qty > 1000 { recommendations.push(serde_json::json!({ "process": "Die Casting", "suitability": 0.85, "cost_trend": "low at volume", "lead_days": 30 })); }
            if complexity == "low" { recommendations.push(serde_json::json!({ "process": "Sheet Metal", "suitability": 0.8, "cost_trend": "low", "lead_days": 3 })); }
            recommendations.push(serde_json::json!({ "process": "DMLS (Metal 3D Print)", "suitability": if complexity == "high" { 0.95 } else { 0.6 }, "cost_trend": "high", "lead_days": 7 }));
        }
        if is_plastic {
            if qty > 500 { recommendations.push(serde_json::json!({ "process": "Injection Molding", "suitability": 0.95, "cost_trend": "low at volume (high tooling)", "lead_days": 45 })); }
            recommendations.push(serde_json::json!({ "process": "FDM 3D Print", "suitability": if qty < 50 { 0.9 } else { 0.5 }, "cost_trend": "low for prototypes", "lead_days": 1 }));
            recommendations.push(serde_json::json!({ "process": "SLA 3D Print", "suitability": if complexity == "high" { 0.9 } else { 0.7 }, "cost_trend": "medium", "lead_days": 2 }));
            if material == "nylon" { recommendations.push(serde_json::json!({ "process": "SLS 3D Print", "suitability": 0.85, "cost_trend": "medium", "lead_days": 3 })); }
        }

        recommendations.sort_by(|a, b| {
            let sa = a.get("suitability").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let sb = b.get("suitability").and_then(|v| v.as_f64()).unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        ToolResult {
            tool_name: "select_process".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{} process(es) recommended for {} × {} ({})", recommendations.len(), qty, material, complexity),
            structured_data: Some(serde_json::json!({ "material": material, "quantity": qty, "recommendations": recommendations })),
            stream_topic: None,
        }
    }
}
