// =============================================================================
// Moderation tools
// =============================================================================
// The Gallery's moderation queue, driven from an agent session. These are the
// MCP face of the tool catalog `infrastructure/cloudflare/api/src/moderation.mjs`
// exposes to its own Grok agent (`MODERATION_TOOLS`): one queue, one case
// view, one `act` that takes any catalog tool by name, and the legacy backfill.
// Both the in-Worker agent and a moderator's IDE session therefore act through
// the same guarded surface with the same refusals, and the playbook at
// docs/moderation/PLAYBOOK.md reads the same for both.
//
// They reach api.eustress.dev with an admin JWT taken from
// `EUSTRESS_MODERATOR_TOKEN`. Without that variable they refuse before any
// request leaves the machine, and the MCP server only grants the Network
// capability these carry when the variable is present (shared_registry.rs), so
// an ordinary session cannot reach the moderation surface at all.
// =============================================================================

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use serde_json::{json, Value};

const DEFAULT_API: &str = "https://api.eustress.dev";

fn api_base() -> String {
    std::env::var("EUSTRESS_API_URL")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_API.to_string())
}

fn token() -> Result<String, String> {
    std::env::var("EUSTRESS_MODERATOR_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "EUSTRESS_MODERATOR_TOKEN is not set. Export an admin JWT for api.eustress.dev \
             in the MCP server's environment to use the moderation tools."
                .to_string()
        })
}

fn ok(name: &str, content: impl Into<String>, data: Value) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: String::new(),
        success: true,
        content: content.into(),
        structured_data: Some(data),
        stream_topic: None,
    }
}

fn err(name: &str, msg: impl Into<String>) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: String::new(),
        success: false,
        content: msg.into(),
        structured_data: None,
        stream_topic: None,
    }
}

fn get(path: &str) -> Result<Value, String> {
    let token = token()?;
    let resp = ureq::get(&format!("{}{}", api_base(), path))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("GET {path}: {e}"))?;
    resp.into_json::<Value>().map_err(|e| format!("GET {path}: bad JSON: {e}"))
}

fn post(path: &str, body: &Value) -> Result<(u16, Value), String> {
    let token = token()?;
    let result = ureq::post(&format!("{}{}", api_base(), path))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    // A refusal from the guardrails comes back as a 400 with a JSON body that
    // says why; that reason is the useful part, so it is returned rather than
    // collapsed into a transport error.
    match result {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.into_json::<Value>().map_err(|e| format!("POST {path}: bad JSON: {e}"))?;
            Ok((status, body))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_json::<Value>().unwrap_or_else(|_| json!({ "error": format!("HTTP {code}") }));
            Ok((code, body))
        }
        Err(e) => Err(format!("POST {path}: {e}")),
    }
}

fn summarize_queue(v: &Value) -> String {
    let status = v["status"].as_str().unwrap_or("?");
    let items = v["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("No cases with status {status}.");
    }
    let mut lines = vec![format!("{} case(s) with status {status}:", items.len())];
    for it in items.iter().take(50) {
        let reasons: Vec<String> = it["reasons"]
            .as_array()
            .map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect())
            .unwrap_or_default();
        lines.push(format!(
            "  {}  lane={}  rating={}  reasons=[{}]  updated={}",
            it["sim_id"].as_str().unwrap_or("?"),
            it["lane"].as_str().unwrap_or("-"),
            it["rating"].as_str().unwrap_or("-"),
            reasons.join(", "),
            it["updated_at"].as_str().unwrap_or("-"),
        ));
    }
    lines.join("\n")
}

fn summarize_case(v: &Value) -> String {
    let c = &v["case"];
    let d = &c["decision"];
    let reasons: Vec<String> = d["reasons"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    format!(
                        "{}{}",
                        r["code"].as_str().unwrap_or("?"),
                        r["p"].as_f64().map(|p| format!(" p={p:.2}")).unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let evidence: Vec<String> = c["judge"]["spatial_evidence"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut hard: Vec<String> = Vec::new();
    if let Some(answers) = c["jev"]["answers"].as_object() {
        for k in [
            "csam_or_minor_sexualization",
            "terrorism_or_extremist_promotion",
            "mass_casualty_attack_planning",
            "ncii_or_real_person_sexual",
            "doxxing_or_targeted_harassment",
        ] {
            if let Some(p) = answers.get(k).and_then(|a| a["p"].as_f64()) {
                if p >= 0.2 {
                    hard.push(format!("{k}={p:.2}"));
                }
            }
        }
    }
    format!(
        "case {}  status={}  lane={}  outcome={}  rating={}  quality={}\n\
         reasons: [{}]\n\
         hard flags: [{}]\n\
         judge: verdict={} quality={} confidence={}\n\
         spatial evidence: {}\n\
         legal hold: {}\n\
         appeal: {}\n\
         history: {} event(s), last: {}",
        c["sim_id"].as_str().unwrap_or("?"),
        c["status"].as_str().unwrap_or("?"),
        d["lane"].as_str().unwrap_or("-"),
        d["outcome"].as_str().unwrap_or("-"),
        d["rating"].as_str().unwrap_or("-"),
        d["quality"].as_str().unwrap_or("-"),
        reasons.join(", "),
        hard.join(", "),
        c["judge"]["verdict"].as_str().unwrap_or("-"),
        c["judge"]["quality"].as_str().unwrap_or("-"),
        c["judge"]["confidence"].as_f64().map(|x| format!("{x:.2}")).unwrap_or_else(|| "-".into()),
        if evidence.is_empty() { "-".to_string() } else { evidence.join(" | ") },
        if c["legal_hold"].is_null() { "none".to_string() } else { c["legal_hold"].to_string() },
        if c["appeal"].is_null() { "none".to_string() } else { c["appeal"]["status"].as_str().unwrap_or("?").to_string() },
        c["history"].as_array().map(|a| a.len()).unwrap_or(0),
        c["history"]
            .as_array()
            .and_then(|a| a.last())
            .map(|h| format!("{} at {}", h["event"].as_str().unwrap_or("?"), h["at"].as_str().unwrap_or("?")))
            .unwrap_or_else(|| "-".into()),
    )
}

// ---------------------------------------------------------------------------

pub struct ModerationQueueTool;

impl ToolHandler for ModerationQueueTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "moderation_queue",
            description: "List Gallery moderation cases by status: held (needs a human), quarantined (legal lane), appealed, changes_requested, classifying, rejected, approved, pending. Each row names the lane, rating and reason codes. Start here when asked to work the queue. Requires EUSTRESS_MODERATOR_TOKEN. Read-only.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Case status to list. Default held.", "enum": ["held", "quarantined", "appealed", "changes_requested", "classifying", "rejected", "approved", "pending"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Rows to return. Default 50." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let status = input.get("status").and_then(|v| v.as_str()).unwrap_or("held");
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).clamp(1, 200);
        match get(&format!("/api/admin/moderation/queue?status={status}&limit={limit}")) {
            Ok(v) => ok("moderation_queue", summarize_queue(&v), v),
            Err(e) => err("moderation_queue", e),
        }
    }
}

pub struct ModerationCaseTool;

impl ToolHandler for ModerationCaseTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "moderation_case",
            description: "Read one moderation case in full: deterministic signals, the classifier's calibrated answers, the judge verdict with its spatial evidence, the agent transcript, legal hold, appeal and history. Captures are at GET /api/admin/moderation/captures/{sim_id}/{n}. Read-only.",
            input_schema: json!({
                "type": "object",
                "properties": { "sim_id": { "type": "string", "description": "The published simulation id." } },
                "required": ["sim_id"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let Some(sim_id) = input.get("sim_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
            return err("moderation_case", "Missing required parameter: sim_id");
        };
        match get(&format!("/api/admin/moderation/case/{sim_id}")) {
            Ok(v) if v["ok"].as_bool() == Some(true) => ok("moderation_case", summarize_case(&v), v),
            Ok(v) => err("moderation_case", v["error"].as_str().unwrap_or("no case").to_string()),
            Err(e) => err("moderation_case", e),
        }
    }
}

pub struct ModerationActTool;

impl ToolHandler for ModerationActTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "moderation_act",
            description: "Run one moderation catalog tool by name, as the signed-in moderator: moderation_approve {sim_id, rating, rationale, child_directed?, featured?}, moderation_reject {sim_id, lane: harm|quality, category, rationale, suggested_edit?}, moderation_hold {sim_id, reason}, moderation_request_changes {sim_id, changes[], rationale}, moderation_set_rating {sim_id, rating, child_directed?}, moderation_quarantine {sim_id, category, rationale}, moderation_escalate_legal {sim_id, category, note}, moderation_author_notice {sim_id, message}, moderation_release {sim_id, rationale, rating?} (lifts a hold or quarantine), moderation_resolve_appeal {sim_id, decision: overturned|upheld, rationale, rating?}, moderation_rerun {sim_id}. Ratings: all_ages, teen_13, mature_17, adult_18. Every rationale must cite specific observed evidence and be at least 20 characters; the API refuses otherwise. Follow docs/moderation/PLAYBOOK.md.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Catalog tool name, e.g. moderation_approve." },
                    "args": { "type": "object", "description": "That tool's arguments." }
                },
                "required": ["name", "args"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let Some(name) = input.get("name").and_then(|v| v.as_str()).filter(|s| s.starts_with("moderation_")) else {
            return err("moderation_act", "name must be a moderation_* catalog tool");
        };
        let args = input.get("args").cloned().unwrap_or_else(|| json!({}));
        match post("/api/admin/moderation/tool", &json!({ "name": name, "args": args })) {
            Ok((status, body)) if (200..300).contains(&status) && body["ok"].as_bool() == Some(true) => {
                let summary = body["summary"].as_str().unwrap_or("done").to_string();
                ok("moderation_act", summary, body)
            }
            Ok((status, body)) => err(
                "moderation_act",
                format!("{name} refused (HTTP {status}): {}", body["error"].as_str().unwrap_or("no reason given")),
            ),
            Err(e) => err("moderation_act", e),
        }
    }
}

pub struct ModerationBackfillTool;

impl ToolHandler for ModerationBackfillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "moderation_backfill",
            description: "Classify Gallery listings that predate the moderation gate, oldest first, a bounded number per call. Each one gets a case and a decision exactly like a fresh publish; expect more holds than usual because a single thumbnail is all the judge has to look at.",
            input_schema: json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Listings to classify in this call. Default 25." } }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(25).clamp(1, 200);
        match post("/api/admin/moderation/backfill", &json!({ "limit": limit })) {
            Ok((status, body)) if (200..300).contains(&status) => ok(
                "moderation_backfill",
                body["summary"].as_str().unwrap_or("backfill ran").to_string(),
                body,
            ),
            Ok((status, body)) => err("moderation_backfill", format!("HTTP {status}: {}", body["error"].as_str().unwrap_or("?"))),
            Err(e) => err("moderation_backfill", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_and_case_summaries_read_like_a_worklist() {
        let q = json!({ "status": "held", "items": [
            { "sim_id": "abc", "lane": "legal", "rating": null, "reasons": ["doxxing_or_targeted_harassment"], "updated_at": "2026-09-20T00:00:00Z" }
        ]});
        let s = summarize_queue(&q);
        assert!(s.contains("1 case(s) with status held"));
        assert!(s.contains("abc  lane=legal"));
        assert!(s.contains("doxxing_or_targeted_harassment"));

        let c = json!({ "ok": true, "case": {
            "sim_id": "abc", "status": "held",
            "decision": { "lane": "legal", "outcome": "hold", "rating": "teen_13", "quality": "listed",
                          "reasons": [{ "code": "doxxing_or_targeted_harassment", "p": 0.51 }] },
            "jev": { "answers": { "doxxing_or_targeted_harassment": { "p": 0.51 }, "csam_or_minor_sexualization": { "p": 0.01 } } },
            "judge": { "verdict": "publish", "quality": "listed", "confidence": 0.8, "spatial_evidence": ["a lit stage with rows of seats"] },
            "legal_hold": null, "appeal": null,
            "history": [{ "event": "created", "at": "t0" }, { "event": "held", "at": "t1" }]
        }});
        let s = summarize_case(&c);
        assert!(s.contains("status=held"));
        assert!(s.contains("doxxing_or_targeted_harassment p=0.51"));
        assert!(s.contains("hard flags: [doxxing_or_targeted_harassment=0.51]"));
        assert!(s.contains("a lit stage with rows of seats"));
        assert!(s.contains("last: held at t1"));
    }

    #[test]
    fn tools_refuse_without_a_moderator_token() {
        // Only meaningful when the variable is absent, which is the default
        // for a test process; a developer with it exported skips this.
        if std::env::var("EUSTRESS_MODERATOR_TOKEN").map(|v| !v.trim().is_empty()).unwrap_or(false) {
            return;
        }
        let ctx = crate::ToolContext {
            space_root: std::path::PathBuf::new(),
            universe_root: std::path::PathBuf::new(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: crate::capability::Permissions::full(),
        };
        let r = ModerationQueueTool.execute(json!({}), &ctx);
        assert!(!r.success);
        assert!(r.content.contains("EUSTRESS_MODERATOR_TOKEN"));
        let r = ModerationActTool.execute(json!({ "name": "moderation_hold", "args": {} }), &ctx);
        assert!(!r.success);
        let r = ModerationActTool.execute(json!({ "name": "delete_entity", "args": {} }), &ctx);
        assert!(r.content.contains("catalog tool"));
    }
}
