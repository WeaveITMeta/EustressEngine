//! Static checks of tool descriptors, and minimal-input synthesis from a
//! descriptor's JSON Schema.
//!
//! The static checks need no engine: they run on a `tools/list` reply.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

/// One finding against one descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub tool: String,
    /// "error" or "warning".
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
}

impl Finding {
    pub fn to_json(&self) -> Value {
        json!({ "tool": self.tool, "severity": self.severity, "code": self.code, "message": self.message })
    }
}

fn err(tool: &str, code: &'static str, message: String) -> Finding {
    Finding { tool: tool.to_string(), severity: "error", code, message }
}

fn warn(tool: &str, code: &'static str, message: String) -> Finding {
    Finding { tool: tool.to_string(), severity: "warning", code, message }
}

/// A tool name every client accepts: 1 to 64 of `[A-Za-z0-9_-]`.
pub fn valid_tool_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The schema of a listed tool: MCP spells it `inputSchema`, the bridge
/// `input_schema`.
pub fn schema_of(tool: &Value) -> Option<&Value> {
    tool.get("inputSchema").or_else(|| tool.get("input_schema"))
}

/// Check every descriptor of one listing. `surface` names the listing in the
/// messages.
pub fn check_listing(surface: &str, tools: &[Value]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for t in tools {
        let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
        *counts.entry(name.to_string()).or_default() += 1;
        out.extend(check_descriptor(surface, t));
    }
    for (name, n) in counts {
        if n > 1 {
            out.push(err(&name, "duplicate_name", format!("{surface}: listed {n} times")));
        }
    }
    out
}

/// Check one descriptor.
pub fn check_descriptor(surface: &str, tool: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let label = if name.is_empty() { "<unnamed>" } else { name };
    if !valid_tool_name(name) {
        out.push(err(label, "invalid_name", format!("{surface}: name {name:?} is not 1-64 of [A-Za-z0-9_-]")));
    }
    match tool.get("description").and_then(|d| d.as_str()) {
        None => out.push(err(label, "missing_description", format!("{surface}: no description"))),
        Some(d) if d.trim().is_empty() => out.push(err(label, "empty_description", format!("{surface}: empty description"))),
        Some(d) if d.trim().chars().count() < 20 => {
            out.push(warn(label, "short_description", format!("{surface}: description is {} characters", d.trim().chars().count())))
        }
        Some(_) => {}
    }
    let Some(schema) = schema_of(tool) else {
        out.push(err(label, "missing_schema", format!("{surface}: no input schema")));
        return out;
    };
    let Some(obj) = schema.as_object() else {
        out.push(err(label, "schema_not_object", format!("{surface}: input schema is not a JSON object")));
        return out;
    };
    if obj.get("type").and_then(|t| t.as_str()) != Some("object") {
        out.push(err(label, "schema_type_not_object", format!("{surface}: input schema type is {:?}, not \"object\"", obj.get("type"))));
    }
    let props = match obj.get("properties") {
        None => None,
        Some(Value::Object(p)) => Some(p),
        Some(_) => {
            out.push(err(label, "properties_not_object", format!("{surface}: `properties` is not an object")));
            None
        }
    };
    if let Some(p) = props {
        for (key, sub) in p {
            check_property(surface, label, key, sub, &mut out);
        }
    }
    match obj.get("required") {
        None => {}
        Some(Value::Array(req)) => {
            let mut seen = BTreeSet::new();
            for r in req {
                match r.as_str() {
                    None => out.push(err(label, "required_not_string", format!("{surface}: `required` holds a non-string {r}"))),
                    Some(k) => {
                        if !seen.insert(k.to_string()) {
                            out.push(err(label, "required_duplicate", format!("{surface}: `{k}` is required twice")));
                        }
                        if !props.map(|p| p.contains_key(k)).unwrap_or(false) {
                            out.push(err(label, "required_not_in_properties", format!("{surface}: required `{k}` is not a declared property")));
                        }
                    }
                }
            }
        }
        Some(_) => out.push(err(label, "required_not_array", format!("{surface}: `required` is not an array"))),
    }
    out
}

fn check_property(surface: &str, tool: &str, key: &str, sub: &Value, out: &mut Vec<Finding>) {
    let Some(o) = sub.as_object() else {
        out.push(err(tool, "property_not_object", format!("{surface}: property `{key}` schema is not an object")));
        return;
    };
    let typed = ["type", "enum", "const", "oneOf", "anyOf", "allOf", "$ref"].iter().any(|k| o.contains_key(*k));
    if !typed {
        out.push(warn(tool, "untyped_property", format!("{surface}: property `{key}` declares no type")));
    }
    if let Some(e) = o.get("enum") {
        match e.as_array() {
            Some(a) if a.is_empty() => out.push(err(tool, "empty_enum", format!("{surface}: property `{key}` has an empty enum"))),
            Some(_) => {}
            None => out.push(err(tool, "enum_not_array", format!("{surface}: property `{key}` enum is not an array"))),
        }
    }
    if o.get("type").and_then(|t| t.as_str()) == Some("array") && !o.contains_key("items") {
        out.push(warn(tool, "array_without_items", format!("{surface}: array property `{key}` declares no `items`")));
    }
}

/// Required property names, in declaration order.
pub fn required_params(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// A minimal input: every required property, synthesized from its schema.
/// Strings can carry `{placeholders}` that the census fills in later.
pub fn synthesize_minimal(schema: &Value) -> Value {
    let mut out = Map::new();
    let props = schema.get("properties").and_then(|p| p.as_object());
    for key in required_params(schema) {
        let sub = props.and_then(|p| p.get(&key)).cloned().unwrap_or(Value::Null);
        out.insert(key.clone(), synthesize_value(&key, &sub, 0));
    }
    Value::Object(out)
}

/// A value for a property called `key` with schema `schema`.
pub fn synthesize_value(key: &str, schema: &Value, depth: usize) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    if let Some(c) = schema.get("const") {
        return c.clone();
    }
    if let Some(first) = schema.get("enum").and_then(|e| e.as_array()).and_then(|a| a.first()) {
        return first.clone();
    }
    if let Some(d) = schema.get("default") {
        if !d.is_null() {
            return d.clone();
        }
    }
    if let Some(ex) = schema.get("examples").and_then(|e| e.as_array()).and_then(|a| a.first()) {
        return ex.clone();
    }
    for alt in ["oneOf", "anyOf"] {
        if let Some(first) = schema.get(alt).and_then(|a| a.as_array()).and_then(|a| a.first()) {
            return synthesize_value(key, first, depth + 1);
        }
    }
    let ty = match schema.get("type") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str())
            .find(|s| *s != "null")
            .unwrap_or("string")
            .to_string(),
        _ if schema.get("properties").is_some() => "object".to_string(),
        _ => "string".to_string(),
    };
    match ty.as_str() {
        "integer" => Value::from(number_in_range(schema, 1.0).round() as i64),
        "number" => json!(number_in_range(schema, 1.0)),
        "boolean" => Value::Bool(false),
        "array" => {
            let n = schema.get("minItems").and_then(|m| m.as_u64()).unwrap_or(1).max(1) as usize;
            let item = schema.get("items").cloned().unwrap_or(json!({ "type": "string" }));
            Value::Array((0..n).map(|_| synthesize_value(key, &item, depth + 1)).collect())
        }
        "object" => {
            let mut o = Map::new();
            let props = schema.get("properties").and_then(|p| p.as_object());
            for k in required_params(schema) {
                let sub = props.and_then(|p| p.get(&k)).cloned().unwrap_or(Value::Null);
                o.insert(k.clone(), synthesize_value(&k, &sub, depth + 1));
            }
            Value::Object(o)
        }
        _ => Value::String(string_for(key, schema)),
    }
}

fn number_in_range(schema: &Value, preferred: f64) -> f64 {
    let mut v = preferred;
    if let Some(min) = schema.get("minimum").and_then(|m| m.as_f64()) {
        if v < min {
            v = min;
        }
    }
    if let Some(min) = schema.get("exclusiveMinimum").and_then(|m| m.as_f64()) {
        if v <= min {
            v = min + 1.0;
        }
    }
    if let Some(max) = schema.get("maximum").and_then(|m| m.as_f64()) {
        if v > max {
            v = max;
        }
    }
    v
}

/// A string chosen by the property's name, pointing at fixture content where
/// the name suggests a reference. Placeholders are filled by the census.
fn string_for(key: &str, schema: &Value) -> String {
    let k = key.to_ascii_lowercase();
    let s = match k.as_str() {
        "universe" | "universe_path" => "{universe}".to_string(),
        "space" | "space_name" => "{space_name}".to_string(),
        "uuid" | "entity_uuid" | "instance_uuid" | "target_uuid" => "{uuid:WelcomeCube}".to_string(),
        "name" | "new_name" | "label" | "title" => "CensusProbe_{run}_{probe}{rep}".to_string(),
        "entity" | "entity_name" | "target" | "instance" | "instance_name" | "part" | "query" | "adornee" => {
            "WelcomeCube".to_string()
        }
        "class" | "class_name" | "classname" => "Part".to_string(),
        "material" => "Plastic".to_string(),
        "color" | "colour" => "#ff8800".to_string(),
        "url" | "uri" | "endpoint" => "http://127.0.0.1:9/census".to_string(),
        "unit" | "units" => "m".to_string(),
        "service" | "parent_service" | "new_parent_service" => "Workspace".to_string(),
        "parent" => "Workspace".to_string(),
        // The file tools resolve paths against the Universe root.
        _ if k.contains("path") || k.contains("file") || k.contains("dir") => "Spaces/{space_name}".to_string(),
        _ if k.ends_with("id") => "census-probe".to_string(),
        _ => "census probe".to_string(),
    };
    let min = schema.get("minLength").and_then(|m| m.as_u64()).unwrap_or(0) as usize;
    if s.chars().count() < min {
        format!("{s}{}", "x".repeat(min - s.chars().count()))
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, desc: &str, schema: Value) -> Value {
        json!({ "name": name, "description": desc, "inputSchema": schema })
    }

    #[test]
    fn a_clean_descriptor_has_no_findings() {
        let t = tool(
            "read_file",
            "Read a file inside the active Space.",
            json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
        );
        assert!(check_descriptor("mcp", &t).is_empty());
    }

    #[test]
    fn broken_descriptors_are_reported() {
        let t = tool("bad name", "", json!({ "type": "array", "required": ["x", "x"] }));
        let codes: Vec<_> = check_descriptor("mcp", &t).into_iter().map(|f| f.code).collect();
        for c in ["invalid_name", "empty_description", "schema_type_not_object", "required_not_in_properties", "required_duplicate"] {
            assert!(codes.contains(&c), "{c} missing from {codes:?}");
        }
    }

    #[test]
    fn duplicates_are_found_across_a_listing() {
        let s = json!({ "type": "object" });
        let list = vec![tool("a_tool", "does a thing well enough", s.clone()), tool("a_tool", "does a thing well enough", s)];
        assert!(check_listing("mcp", &list).iter().any(|f| f.code == "duplicate_name"));
    }

    #[test]
    fn synthesis_fills_every_required_property() {
        let s = json!({
            "type": "object",
            "properties": {
                "uuid": { "type": "string" },
                "mode": { "type": "string", "enum": ["fast", "slow"] },
                "count": { "type": "integer", "minimum": 3 },
                "position": { "type": "array", "items": { "type": "number" }, "minItems": 3 },
                "opt": { "type": "string" }
            },
            "required": ["uuid", "mode", "count", "position"]
        });
        let v = synthesize_minimal(&s);
        assert_eq!(v["uuid"], "{uuid:WelcomeCube}");
        assert_eq!(v["mode"], "fast");
        assert_eq!(v["count"], 3);
        assert_eq!(v["position"].as_array().unwrap().len(), 3);
        assert!(v.get("opt").is_none(), "minimal means required only");
    }
}
