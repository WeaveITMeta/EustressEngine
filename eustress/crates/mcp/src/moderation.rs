//! Moderation MCP Tools — Admin commands for user management, screening, and risk control.
//!
//! These tools are exposed via the MCP protocol so AI agents (Grok, Claude) can
//! perform moderation actions programmatically.
//!
//! All tools call the Cloudflare Worker at api.eustress.dev/api/admin/*.

use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.eustress.dev";

/// MCP Tool definition for moderation actions.
#[derive(Debug, Clone, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// All available moderation tools.
pub fn moderation_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "mod_list_users".into(),
            description: "List all registered users with risk scores and status".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
        McpTool {
            name: "mod_ban_user".into(),
            description: "Ban a user by username. Prevents login and all platform access.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Username to ban" },
                    "reason": { "type": "string", "description": "Reason for the ban" }
                },
                "required": ["username", "reason"]
            }),
        },
        McpTool {
            name: "mod_warn_user".into(),
            description: "Issue a warning to a user. Warnings accumulate and affect reputation.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Username to warn" },
                    "message": { "type": "string", "description": "Warning message" }
                },
                "required": ["username", "message"]
            }),
        },
        McpTool {
            name: "mod_review_user".into(),
            description: "Complete a manual review of a flagged user, setting their risk decision.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Username to review" },
                    "decision": { "type": "string", "enum": ["APPROVE", "REVIEW", "DENY"], "description": "Review decision" },
                    "notes": { "type": "string", "description": "Review notes" }
                },
                "required": ["username", "decision"]
            }),
        },
        McpTool {
            name: "mod_risk_override".into(),
            description: "Override a user's AI-assigned risk score. Use for false positives/negatives.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Username" },
                    "risk_score": { "type": "number", "description": "New risk score (0-100)" },
                    "decision": { "type": "string", "enum": ["APPROVE", "REVIEW", "DENY"] },
                    "reason": { "type": "string", "description": "Reason for override" }
                },
                "required": ["username", "risk_score", "reason"]
            }),
        },
        McpTool {
            name: "mod_rescreen_user".into(),
            description: "Re-run AI background screening on a user. Tracks score deltas over time.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Username to re-screen" }
                },
                "required": ["username"]
            }),
        },
        McpTool {
            name: "mod_screening_report".into(),
            description: "Get the full screening report — all users sorted by risk score with history.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}

/// Execute a moderation tool by calling the Cloudflare Worker admin API.
pub async fn execute_tool(
    tool_name: &str,
    args: serde_json::Value,
    admin_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();

    let (method, endpoint, body) = match tool_name {
        "mod_list_users" => ("GET", "/api/admin/users", None),
        "mod_ban_user" => ("POST", "/api/admin/ban", Some(args.clone())),
        "mod_warn_user" => ("POST", "/api/admin/warn", Some(args.clone())),
        "mod_review_user" => ("POST", "/api/admin/review", Some(args.clone())),
        "mod_risk_override" => ("POST", "/api/admin/risk-override", Some(args.clone())),
        "mod_rescreen_user" => ("POST", "/api/admin/rescreen", Some(args.clone())),
        "mod_screening_report" => ("GET", "/api/admin/screening-report", None),
        _ => return Err(format!("Unknown tool: {}", tool_name)),
    };

    let url = format!("{}{}", API_URL, endpoint);
    let mut req = match method {
        "POST" => client.post(&url),
        _ => client.get(&url),
    };

    req = req.header("Authorization", format!("Bearer {}", admin_token));
    if let Some(body) = body {
        req = req.json(&body);
    }

    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.is_success() {
        serde_json::from_str(&text).map_err(|e| format!("Parse error: {}", e))
    } else {
        Err(format!("API error ({}): {}", status, text))
    }
}
