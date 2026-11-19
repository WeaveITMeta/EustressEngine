//! Curated census inputs (`scripts/agent_eval/census_inputs.json`) and the
//! placeholders every input may use.
//!
//! An override can give a tool its valid input, name the parameter the
//! missing-parameter probe removes, state a curated effect rule (with the
//! reason, which goes into the artifact), and list setup and teardown steps.
//! Steps are not probes: they are never timed or classified.
//!
//! Placeholders, filled in every string of an input or step:
//! `{universe}`, `{universe_b}`, `{workspace}`, `{space}` (absolute Space
//! directory), `{space_name}`, `{space2_name}`, `{run}`, `{probe}` (`v` or
//! `m`), `{rep}`, and `{uuid:<Name>}` (the uuid of the fixture entity named
//! `<Name>`, read from the engine before the census starts).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::classify::EffectRule;

/// One setup or teardown step.
#[derive(Debug, Clone)]
pub enum Step {
    /// A bridge method on the engine.
    Bridge { method: String, params: Value },
    /// A tool on the MCP server.
    McpTool { name: String, arguments: Value },
}

/// A curated effect rule and why it is used.
#[derive(Debug, Clone)]
pub struct CuratedEffect {
    pub rule: EffectRule,
    pub why: String,
}

/// One tool's overrides.
#[derive(Debug, Clone, Default)]
pub struct ToolOverride {
    pub valid_input: Option<Value>,
    pub missing_param: Option<String>,
    pub effect: Option<CuratedEffect>,
    pub setup: Vec<Step>,
    pub teardown: Vec<Step>,
    pub note: Option<String>,
}

/// Every override, by tool name.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub tools: BTreeMap<String, ToolOverride>,
    /// Where they came from, or why none were loaded.
    pub source: String,
}

impl Overrides {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => {
                return Ok(Overrides { tools: BTreeMap::new(), source: format!("none ({} not found)", path.display()) })
            }
        };
        let root: Value = serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        Self::from_json(&root, &path.display().to_string())
    }

    pub fn from_json(root: &Value, source: &str) -> Result<Self, String> {
        let mut tools = BTreeMap::new();
        let Some(map) = root.get("tools").and_then(|t| t.as_object()) else {
            return Err(format!("{source}: no `tools` object"));
        };
        for (name, o) in map {
            let mut ov = ToolOverride {
                valid_input: o.get("valid_input").cloned(),
                missing_param: o.get("missing_param").and_then(|v| v.as_str()).map(str::to_string),
                note: o.get("note").and_then(|v| v.as_str()).map(str::to_string),
                ..Default::default()
            };
            if let Some(e) = o.get("effect") {
                ov.effect = Some(parse_effect(name, e)?);
            }
            ov.setup = parse_steps(name, o.get("setup"))?;
            ov.teardown = parse_steps(name, o.get("teardown"))?;
            tools.insert(name.clone(), ov);
        }
        Ok(Overrides { tools, source: source.to_string() })
    }

    pub fn get(&self, name: &str) -> Option<&ToolOverride> {
        self.tools.get(name)
    }
}

fn parse_effect(tool: &str, e: &Value) -> Result<CuratedEffect, String> {
    let why = e
        .get("why")
        .and_then(|w| w.as_str())
        .filter(|w| !w.trim().is_empty())
        .ok_or_else(|| format!("{tool}: a curated effect must say why"))?
        .to_string();
    let rule = match e.get("kind").and_then(|k| k.as_str()) {
        Some("digest") => EffectRule::Digest,
        Some("payload") => EffectRule::Payload,
        Some("payload_pointer_equals") => EffectRule::PayloadPointerEquals {
            pointer: e
                .get("pointer")
                .and_then(|p| p.as_str())
                .ok_or_else(|| format!("{tool}: payload_pointer_equals needs `pointer`"))?
                .to_string(),
            value: e.get("value").cloned().unwrap_or(Value::Null),
        },
        other => return Err(format!("{tool}: unknown effect kind {other:?}")),
    };
    Ok(CuratedEffect { rule, why })
}

fn parse_steps(tool: &str, v: Option<&Value>) -> Result<Vec<Step>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let Some(arr) = v.as_array() else {
        return Err(format!("{tool}: steps must be an array"));
    };
    let mut out = Vec::new();
    for s in arr {
        if let Some(m) = s.get("bridge").and_then(|m| m.as_str()) {
            out.push(Step::Bridge { method: m.to_string(), params: s.get("params").cloned().unwrap_or(Value::Object(Default::default())) });
        } else if let Some(n) = s.get("mcp_tool").and_then(|m| m.as_str()) {
            out.push(Step::McpTool { name: n.to_string(), arguments: s.get("arguments").cloned().unwrap_or(Value::Object(Default::default())) });
        } else {
            return Err(format!("{tool}: a step needs `bridge` or `mcp_tool`: {s}"));
        }
    }
    Ok(out)
}

/// Values for the placeholders.
#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    pub vars: BTreeMap<String, String>,
    /// Fixture entity name to uuid.
    pub uuids: BTreeMap<String, String>,
}

impl Placeholders {
    /// Fill every placeholder in every string of `v`, with `probe` and
    /// `rep` for this invocation. Returns the filled value and the
    /// placeholders that had no value.
    pub fn fill(&self, v: &Value, probe: &str, rep: usize) -> (Value, Vec<String>) {
        let mut missing = Vec::new();
        let out = self.fill_inner(v, probe, rep, &mut missing);
        missing.sort();
        missing.dedup();
        (out, missing)
    }

    fn fill_inner(&self, v: &Value, probe: &str, rep: usize, missing: &mut Vec<String>) -> Value {
        match v {
            Value::String(s) => Value::String(self.fill_str(s, probe, rep, missing)),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.fill_inner(x, probe, rep, missing)).collect()),
            Value::Object(m) => Value::Object(
                m.iter().map(|(k, x)| (k.clone(), self.fill_inner(x, probe, rep, missing))).collect(),
            ),
            other => other.clone(),
        }
    }

    fn fill_str(&self, s: &str, probe: &str, rep: usize, missing: &mut Vec<String>) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                out.push_str(&rest[open..]);
                return out;
            };
            let key = &after[..close];
            let value = match key {
                "probe" => Some(probe.to_string()),
                "rep" => Some(rep.to_string()),
                k if k.starts_with("uuid:") => self.uuids.get(&k[5..]).cloned(),
                k => self.vars.get(k).cloned(),
            };
            match value {
                Some(v) => out.push_str(&v),
                None => {
                    // Not a placeholder this census knows (a JSON-looking
                    // string, say): keep it as written.
                    if is_placeholder_name(key) {
                        missing.push(key.to_string());
                    }
                    out.push('{');
                    out.push_str(key);
                    out.push('}');
                }
            }
            rest = &after[close + 1..];
        }
        out.push_str(rest);
        out
    }
}

fn is_placeholder_name(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn placeholders_fill_and_report_gaps() {
        let mut p = Placeholders::default();
        p.vars.insert("universe".into(), "C:/fx/CensusUniverse".into());
        p.uuids.insert("WelcomeCube".into(), "abc".into());
        let (v, missing) = p.fill(&json!({ "u": "{universe}", "id": "{uuid:WelcomeCube}", "n": "P_{probe}{rep}", "x": "{nope}", "j": "{\"a\":1}" }), "v", 2);
        assert_eq!(v["u"], "C:/fx/CensusUniverse");
        assert_eq!(v["id"], "abc");
        assert_eq!(v["n"], "P_v2");
        assert_eq!(v["x"], "{nope}");
        assert_eq!(v["j"], "{\"a\":1}");
        assert_eq!(missing, vec!["nope".to_string()]);
    }

    #[test]
    fn overrides_parse_and_require_a_reason_for_curated_effects() {
        let ok = json!({ "tools": { "set_active_universe": {
            "valid_input": { "universe": "{universe_b}" },
            "effect": { "kind": "payload_pointer_equals", "pointer": "/changed", "value": true, "why": "session state" },
            "teardown": [{ "mcp_tool": "set_active_universe", "arguments": { "universe": "{universe}" } }]
        } } });
        let o = Overrides::from_json(&ok, "test").unwrap();
        let t = o.get("set_active_universe").unwrap();
        assert_eq!(t.teardown.len(), 1);
        assert!(matches!(t.effect.as_ref().unwrap().rule, EffectRule::PayloadPointerEquals { .. }));
        let bad = json!({ "tools": { "x": { "effect": { "kind": "payload" } } } });
        assert!(Overrides::from_json(&bad, "test").is_err());
    }
}
