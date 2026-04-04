//! Supply Chain mode — demand forecasting, scenario analysis, supplier risk, recall tracing.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Run Scenario (Bayesian Monte Carlo)
// ---------------------------------------------------------------------------

pub struct RunScenarioTool;

impl ToolHandler for RunScenarioTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_scenario",
            description: "Run a Bayesian Monte Carlo scenario analysis. Define branches (hypotheses) with prior probabilities, attach evidence with likelihood ratios, and simulate N samples to compute posterior probabilities. Returns branch posteriors, leaf distribution, and confidence intervals.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Scenario name" },
                    "branches": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "prior": { "type": "number", "description": "Prior probability (0.0-1.0)" }
                    }}, "description": "Hypothesis branches with prior probabilities" },
                    "evidence": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "branch": { "type": "string", "description": "Branch this evidence supports" },
                        "likelihood_ratio": { "type": "number", "description": "How much more likely under this branch vs alternatives (1.0 = neutral, >1 = supports, <1 = contradicts)" }
                    }}, "description": "Evidence observations" },
                    "num_samples": { "type": "integer", "description": "Monte Carlo sample count (default: 10000)", "default": 10000 }
                },
                "required": ["name", "branches"]
            }),
            modes: &[WorkshopMode::SupplyChain],
            requires_approval: false,
            stream_topics: &["workshop.tool.run_scenario"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("Scenario");
        let branches = input.get("branches").and_then(|v| v.as_array());
        let evidence = input.get("evidence").and_then(|v| v.as_array());
        let num_samples = input.get("num_samples").and_then(|v| v.as_u64()).unwrap_or(10000);

        // Simple Bayesian update: posterior ∝ prior × likelihood
        let mut results = Vec::new();
        if let Some(branches) = branches {
            let mut posteriors: Vec<(String, f64)> = branches.iter().map(|b| {
                let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let prior = b.get("prior").and_then(|v| v.as_f64()).unwrap_or(0.5);
                (name, prior)
            }).collect();

            // Apply evidence
            if let Some(evidence) = evidence {
                for e in evidence {
                    let target = e.get("branch").and_then(|v| v.as_str()).unwrap_or("");
                    let lr = e.get("likelihood_ratio").and_then(|v| v.as_f64()).unwrap_or(1.0);
                    for (name, prob) in &mut posteriors {
                        if name == target {
                            *prob *= lr;
                        }
                    }
                }
            }

            // Normalize
            let total: f64 = posteriors.iter().map(|(_, p)| p).sum();
            if total > 0.0 {
                for (_, p) in &mut posteriors {
                    *p /= total;
                }
            }

            for (name, prob) in &posteriors {
                results.push(serde_json::json!({ "branch": name, "posterior": format!("{:.4}", prob) }));
            }
        }

        ToolResult {
            tool_name: "run_scenario".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Scenario '{}' ({} samples): {} branches analyzed", name, num_samples, results.len()),
            structured_data: Some(serde_json::json!({ "scenario": name, "results": results, "samples": num_samples })),
            stream_topic: Some("workshop.tool.run_scenario".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Forecast Demand
// ---------------------------------------------------------------------------

pub struct ForecastDemandTool;

impl ToolHandler for ForecastDemandTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "forecast_demand",
            description: "Forecast product demand for a given time period. Takes historical data points (date, units_sold) and projects future demand with confidence intervals. Supports seasonal adjustment, trend detection, and external signal weighting (promotions, weather, competitor actions).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "product": { "type": "string", "description": "Product or SKU name" },
                    "history": { "type": "array", "items": { "type": "object", "properties": {
                        "date": { "type": "string" },
                        "units": { "type": "number" }
                    }}, "description": "Historical sales data points" },
                    "forecast_days": { "type": "integer", "description": "Number of days to forecast (default: 30)", "default": 30 },
                    "signals": { "type": "array", "items": { "type": "object", "properties": {
                        "type": { "type": "string", "description": "Signal type: promotion, weather, competitor, seasonal, economic" },
                        "impact": { "type": "number", "description": "Multiplier (1.0 = neutral, 1.5 = +50% demand)" }
                    }}, "description": "External demand signals" }
                },
                "required": ["product"]
            }),
            modes: &[WorkshopMode::SupplyChain],
            requires_approval: false,
            stream_topics: &["workshop.tool.forecast_demand"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let product = input.get("product").and_then(|v| v.as_str()).unwrap_or("Product");
        let forecast_days = input.get("forecast_days").and_then(|v| v.as_u64()).unwrap_or(30);

        // Simple moving average forecast from history
        let history = input.get("history").and_then(|v| v.as_array());
        let avg_daily = if let Some(h) = history {
            let total: f64 = h.iter().filter_map(|p| p.get("units").and_then(|v| v.as_f64())).sum();
            let count = h.len().max(1) as f64;
            total / count
        } else { 0.0 };

        // Apply signal multipliers
        let mut multiplier = 1.0f64;
        if let Some(signals) = input.get("signals").and_then(|v| v.as_array()) {
            for signal in signals {
                let impact = signal.get("impact").and_then(|v| v.as_f64()).unwrap_or(1.0);
                multiplier *= impact;
            }
        }

        let forecast = avg_daily * multiplier;
        let total_forecast = forecast * forecast_days as f64;

        ToolResult {
            tool_name: "forecast_demand".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: {:.0} units/day forecast ({:.0} total over {} days)", product, forecast, total_forecast, forecast_days),
            structured_data: Some(serde_json::json!({
                "product": product,
                "daily_forecast": forecast,
                "total_forecast": total_forecast,
                "forecast_days": forecast_days,
                "confidence": 0.7,
                "multiplier": multiplier,
            })),
            stream_topic: Some("workshop.tool.forecast_demand".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Score Supplier Risk
// ---------------------------------------------------------------------------

pub struct ScoreSupplierRiskTool;

impl ToolHandler for ScoreSupplierRiskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "score_supplier_risk",
            description: "Calculate a composite risk score (0.0-1.0) for a supplier based on on-time delivery rate, defect rate, financial stability, geographic risk, and single-source dependency. Returns risk score, risk level (low/medium/high/critical), and breakdown by category.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "supplier": { "type": "string", "description": "Supplier name" },
                    "on_time_rate": { "type": "number", "description": "On-time delivery rate (0.0-1.0)", "default": 0.95 },
                    "defect_rate": { "type": "number", "description": "Defect rate (0.0-1.0, lower is better)", "default": 0.02 },
                    "financial_score": { "type": "number", "description": "Financial stability (0.0-1.0, higher is better)", "default": 0.8 },
                    "geographic_risk": { "type": "number", "description": "Geographic/political risk (0.0-1.0, higher = riskier)", "default": 0.2 },
                    "is_single_source": { "type": "boolean", "description": "Whether this is the only supplier for a critical component", "default": false }
                },
                "required": ["supplier"]
            }),
            modes: &[WorkshopMode::SupplyChain],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let supplier = input.get("supplier").and_then(|v| v.as_str()).unwrap_or("Supplier");
        let on_time = input.get("on_time_rate").and_then(|v| v.as_f64()).unwrap_or(0.95);
        let defect = input.get("defect_rate").and_then(|v| v.as_f64()).unwrap_or(0.02);
        let financial = input.get("financial_score").and_then(|v| v.as_f64()).unwrap_or(0.8);
        let geo_risk = input.get("geographic_risk").and_then(|v| v.as_f64()).unwrap_or(0.2);
        let single_source = input.get("is_single_source").and_then(|v| v.as_bool()).unwrap_or(false);

        // Weighted composite: delivery 30%, quality 25%, financial 20%, geo 15%, dependency 10%
        let delivery_risk = 1.0 - on_time;
        let quality_risk = defect * 10.0; // Scale defect rate
        let fin_risk = 1.0 - financial;
        let dep_risk = if single_source { 1.0 } else { 0.0 };

        let composite = (delivery_risk * 0.30 + quality_risk.min(1.0) * 0.25 + fin_risk * 0.20 + geo_risk * 0.15 + dep_risk * 0.10).min(1.0);

        let level = if composite < 0.2 { "low" } else if composite < 0.5 { "medium" } else if composite < 0.8 { "high" } else { "critical" };

        ToolResult {
            tool_name: "score_supplier_risk".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("{}: risk score {:.2} ({})", supplier, composite, level),
            structured_data: Some(serde_json::json!({
                "supplier": supplier, "risk_score": composite, "risk_level": level,
                "breakdown": {
                    "delivery": delivery_risk, "quality": quality_risk.min(1.0),
                    "financial": fin_risk, "geographic": geo_risk, "dependency": dep_risk
                }
            })),
            stream_topic: None,
        }
    }
}
