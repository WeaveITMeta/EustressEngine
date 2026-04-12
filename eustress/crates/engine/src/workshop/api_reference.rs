//! Scripting API catalog — single source of truth for Rune + Luau APIs.
//!
//! Parses `rune_ecs_module.rs` and `SCRIPTING_API_CHECKLIST.md` at compile time
//! via `include_str!` to produce a structured catalog of every scripting function.
//!
//! Three consumers:
//! 1. Workshop agent — `format_full_reference()` for system prompt injection
//! 2. In-engine API Browser panel — `entries` pushed to Slint via `sync_api_reference_to_slint`
//! 3. Web docs — `to_json()` for the /learn documentation pipeline

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::Write;

// ============================================================================
// Data Model
// ============================================================================

/// A single scripting API function or method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEntry {
    pub name: String,
    pub params: Vec<ApiParam>,
    pub return_type: String,
    pub doc: String,
    pub category: String,
    pub language: ScriptLanguage,
    pub status: ApiStatus,
    pub example: String,
}

/// A typed parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiParam {
    pub name: String,
    pub typ: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptLanguage {
    Rune,
    Luau,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiStatus {
    Implemented,
    Partial,
    NotStarted,
    NotApplicable,
    Extension,
}

/// Complete API catalog — Bevy Resource + JSON-serializable.
#[derive(Debug, Clone, Serialize, Deserialize, Resource, Default)]
pub struct ApiCatalog {
    pub entries: Vec<ApiEntry>,
    pub categories: Vec<String>,
    pub rune_type_names: Vec<String>,
    pub luau_services: Vec<String>,
    pub generated_at: String,
}

// ============================================================================
// Display helpers
// ============================================================================

impl std::fmt::Display for ScriptLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rune => write!(f, "Rune"),
            Self::Luau => write!(f, "Luau"),
            Self::Both => write!(f, "Both"),
        }
    }
}

impl std::fmt::Display for ApiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Implemented => write!(f, "implemented"),
            Self::Partial => write!(f, "partial"),
            Self::NotStarted => write!(f, "not-started"),
            Self::NotApplicable => write!(f, "n/a"),
            Self::Extension => write!(f, "extension"),
        }
    }
}

impl ApiStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Implemented => "check",
            Self::Partial => "partial",
            Self::NotStarted => "cross",
            Self::NotApplicable => "dash",
            Self::Extension => "diamond",
        }
    }
}

// ============================================================================
// Catalog Builder
// ============================================================================

impl ApiCatalog {
    /// Build the full catalog by parsing embedded source files at compile time.
    pub fn build() -> Self {
        let rune_source = include_str!("../soul/rune_ecs_module.rs");
        let checklist = include_str!("../../../../../docs/development/SCRIPTING_API_CHECKLIST.md");

        let status_map = parse_checklist_statuses(checklist);
        let mut entries = parse_rune_functions(rune_source, &status_map);
        entries.extend(luau_known_entries(&status_map));

        // Deduplicate and sort
        entries.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));

        let categories: Vec<String> = entries.iter()
            .map(|e| e.category.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let rune_type_names = parse_rune_types(rune_source);

        let luau_services = vec![
            "Instance".into(), "Workspace".into(), "Players".into(),
            "RunService".into(), "TweenService".into(), "DataStoreService".into(),
            "CollectionService".into(), "HttpService".into(), "MarketplaceService".into(),
            "SoundService".into(), "UserInputService".into(), "StarterGui".into(),
        ];

        Self {
            entries,
            categories,
            rune_type_names,
            luau_services,
            generated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    // ---- Output Formatters ------------------------------------------------

    /// Markdown reference for agent prompt injection.
    pub fn format_full_reference(&self) -> String {
        let mut out = String::with_capacity(12288);
        let _ = writeln!(out, "## Scripting API Reference\n");
        let _ = writeln!(out, "_{} functions, {} types (auto-generated from engine source)_\n",
            self.entries.len(), self.rune_type_names.len());

        // Rune functions grouped by category
        out.push_str("### Rune API\n\n");
        let mut last_cat = "";
        for e in self.entries.iter().filter(|e| matches!(e.language, ScriptLanguage::Rune | ScriptLanguage::Both)) {
            if e.category != last_cat {
                let _ = writeln!(out, "#### {}\n", e.category);
                last_cat = &e.category;
            }
            let params_str = e.params.iter()
                .map(|p| format!("{}: {}", p.name, p.typ))
                .collect::<Vec<_>>()
                .join(", ");
            if e.return_type.is_empty() || e.return_type == "()" {
                let _ = writeln!(out, "- `{}({})` — {}", e.name, params_str, e.doc);
            } else {
                let _ = writeln!(out, "- `{}({})` -> {} — {}", e.name, params_str, e.return_type, e.doc);
            }
        }

        // Rune types
        out.push_str("\n#### Data Types\n");
        for t in &self.rune_type_names {
            let _ = writeln!(out, "- `{}`", t);
        }

        // Luau summary
        out.push_str("\n### Luau API\n\n");
        out.push_str("Luau scripts have Roblox API compatibility. Available services:\n");
        for svc in &self.luau_services {
            let _ = writeln!(out, "- `{}`", svc);
        }
        out.push_str("\nUse `Instance.new(className)` to create objects. ");
        out.push_str("All Part properties (Position, Size, Color, Material, Anchored, CanCollide, Transparency, CFrame) work. ");
        out.push_str("GUI elements (ScreenGui, Frame, TextLabel, TextButton, ImageLabel) use standard Roblox patterns.\n\n");

        // Luau functions
        last_cat = "";
        for e in self.entries.iter().filter(|e| matches!(e.language, ScriptLanguage::Luau | ScriptLanguage::Both)) {
            if e.category != last_cat {
                let _ = writeln!(out, "#### {}\n", e.category);
                last_cat = &e.category;
            }
            let _ = writeln!(out, "- `{}` — {}", e.example, e.doc);
        }

        out
    }

    /// JSON for web docs export.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("ApiCatalog serialization cannot fail")
    }

    /// Compact JSON for embedding.
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).expect("ApiCatalog serialization cannot fail")
    }
}

// ============================================================================
// Rune Parser
// ============================================================================

/// Parse `#[rune::function]` annotated functions from rune_ecs_module.rs source.
fn parse_rune_functions(
    source: &str,
    status_map: &std::collections::HashMap<String, (ApiStatus, ApiStatus)>,
) -> Vec<ApiEntry> {
    let lines: Vec<&str> = source.lines().collect();
    let mut entries = Vec::new();
    let mut current_category = "General".to_string();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Track category from section comments
        if line.starts_with("//") && !line.starts_with("///") {
            let comment = line.trim_start_matches('/').trim();
            if !comment.chars().all(|c| c == '=' || c == '-' || c == ' ')
                && !comment.is_empty()
                && comment.len() < 80
                && !comment.contains("TODO")
                && !comment.to_lowercase().contains("thread")
            {
                current_category = clean_category(comment);
            }
        }

        // Look for #[rune::function]
        if line == "#[rune::function]" {
            // Collect doc comments above (skip #[cfg] lines)
            let mut doc_lines = Vec::new();
            let mut j = i as isize - 1;
            while j >= 0 {
                let prev = lines[j as usize].trim();
                if prev.starts_with("///") {
                    doc_lines.push(prev.trim_start_matches('/').trim());
                    j -= 1;
                } else if prev.starts_with("#[cfg") {
                    j -= 1;
                } else {
                    break;
                }
            }
            doc_lines.reverse();
            let doc = doc_lines.join(" ").trim().to_string();

            // Next non-attribute line should be the fn signature
            let mut k = i + 1;
            while k < lines.len() && lines[k].trim().starts_with("#[") {
                k += 1;
            }
            if k < lines.len() {
                let fn_line = lines[k].trim();
                if let Some(entry) = parse_rune_fn_signature(fn_line, &doc, &current_category, status_map) {
                    entries.push(entry);
                }
            }
        }
        i += 1;
    }
    entries
}

/// Parse a single `fn name(params) -> RetType {` line into an ApiEntry.
fn parse_rune_fn_signature(
    line: &str,
    doc: &str,
    category: &str,
    status_map: &std::collections::HashMap<String, (ApiStatus, ApiStatus)>,
) -> Option<ApiEntry> {
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let line = line.strip_prefix("fn ").unwrap_or(line);

    let paren_open = line.find('(')?;
    let name = line[..paren_open].trim().to_string();

    // Skip internal helpers
    if name.starts_with("with_") || name.starts_with("set_space") || name.starts_with("clear_")
        || name.starts_with("set_ecs") || name.starts_with("set_spatial")
    {
        return None;
    }

    let paren_close = line.find(')')?;
    let raw_params = &line[paren_open + 1..paren_close];

    let params: Vec<ApiParam> = raw_params.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty() && *p != "&self")
        .map(|p| {
            if let Some(colon) = p.find(':') {
                ApiParam {
                    name: p[..colon].trim().to_string(),
                    typ: p[colon + 1..].trim().to_string(),
                    optional: p.contains("Option"),
                }
            } else {
                ApiParam { name: p.to_string(), typ: "any".to_string(), optional: false }
            }
        })
        .collect();

    // Return type
    let after_paren = &line[paren_close + 1..];
    let return_type = if let Some(arrow) = after_paren.find("->") {
        after_paren[arrow + 2..].trim().trim_end_matches('{').trim().to_string()
    } else {
        String::new()
    };

    // Auto-generate example
    let params_call = params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ");
    let example = if return_type.is_empty() || return_type == "()" {
        format!("{}({})", name, params_call)
    } else {
        format!("let result = {}({})", name, params_call)
    };

    // Look up status from checklist (default: Implemented since it's in the module)
    let status = status_map.get(&name)
        .map(|(_, rune_status)| *rune_status)
        .unwrap_or(ApiStatus::Implemented);

    Some(ApiEntry {
        name,
        params,
        return_type,
        doc: doc.to_string(),
        category: category.to_string(),
        language: ScriptLanguage::Rune,
        status,
        example,
    })
}

/// Parse Rune type registrations: `module.ty::<TypeName>()?;`
fn parse_rune_types(source: &str) -> Vec<String> {
    let mut types = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("module.ty::<") {
            if let Some(start) = trimmed.find("::<") {
                if let Some(end) = trimmed[start..].find(">(") {
                    let type_name = &trimmed[start + 3..start + end];
                    types.push(type_name.to_string());
                }
            }
        }
    }
    types.sort();
    types.dedup();
    types
}

// ============================================================================
// Luau Known Entries (fallback for functions that are hard to auto-parse)
// ============================================================================

/// Known Luau API entries — the Luau runtime uses procedural mlua calls
/// that are harder to auto-parse than Rune's #[rune::function] pattern.
fn luau_known_entries(
    status_map: &std::collections::HashMap<String, (ApiStatus, ApiStatus)>,
) -> Vec<ApiEntry> {
    let defs: &[(&str, &str, &str, &str, &str)] = &[
        // (name, params, return_type, doc, category)
        ("Instance.new", "className", "Instance", "Create a new instance of the given class", "Instance"),
        ("instance:Clone", "", "Instance", "Deep-copy this instance and its descendants", "Instance"),
        ("instance:Destroy", "", "", "Remove this instance and disconnect all events", "Instance"),
        ("instance:FindFirstChild", "name, recursive?", "Instance?", "Find child by name", "Instance"),
        ("instance:FindFirstChildOfClass", "className", "Instance?", "Find child by class", "Instance"),
        ("instance:GetChildren", "", "table", "Get array of direct children", "Instance"),
        ("instance:GetDescendants", "", "table", "Get array of all descendants recursively", "Instance"),
        ("instance:IsA", "className", "bool", "Check if instance is of class or inherits from it", "Instance"),
        ("instance:GetAttribute", "key", "any", "Get custom attribute value", "Instance"),
        ("instance:SetAttribute", "key, value", "", "Set custom attribute value", "Instance"),
        ("instance:WaitForChild", "name, timeout?", "Instance?", "Wait for child (synchronous lookup)", "Instance"),

        // Workspace
        ("workspace.Gravity", "", "number", "Gravity constant (default 9.80665 m/s^2)", "Workspace"),

        // task library
        ("task.wait", "seconds?", "number", "Yield for N seconds (default 0 = next frame)", "Task"),
        ("task.spawn", "fn, args...", "thread", "Run function in new thread immediately", "Task"),
        ("task.defer", "fn, args...", "thread", "Run function after current thread yields", "Task"),
        ("task.delay", "seconds, fn, args...", "thread", "Run function after delay", "Task"),
        ("task.cancel", "thread", "", "Cancel a spawned thread", "Task"),

        // TweenService
        ("TweenInfo.new", "time, style?, direction?, repeatCount?, reverses?, delayTime?", "TweenInfo", "Create tween timing info", "TweenService"),
        ("TweenService:Create", "instance, tweenInfo, properties", "Tween", "Create property animation tween", "TweenService"),

        // RunService
        ("RunService:IsClient", "", "bool", "Check if running on client", "RunService"),
        ("RunService:IsServer", "", "bool", "Check if running on server", "RunService"),
        ("RunService:IsStudio", "", "bool", "Check if running in Studio mode", "RunService"),
        ("RunService:IsRunning", "", "bool", "Check if simulation is running", "RunService"),

        // CollectionService
        ("CollectionService:AddTag", "instance, tag", "", "Tag an instance", "CollectionService"),
        ("CollectionService:RemoveTag", "instance, tag", "", "Remove tag from instance", "CollectionService"),
        ("CollectionService:HasTag", "instance, tag", "bool", "Check if instance has tag", "CollectionService"),
        ("CollectionService:GetTagged", "tag", "table", "Get all instances with tag", "CollectionService"),

        // HttpService
        ("HttpService:GetAsync", "url", "string", "HTTP GET request", "HttpService"),
        ("HttpService:PostAsync", "url, data", "string", "HTTP POST request", "HttpService"),
        ("HttpService:RequestAsync", "options", "HttpResponse", "Full HTTP request with method/headers/body", "HttpService"),
        ("HttpService:JSONEncode", "value", "string", "Encode value as JSON string", "HttpService"),
        ("HttpService:JSONDecode", "json", "any", "Decode JSON string to value", "HttpService"),
        ("HttpService:UrlEncode", "input", "string", "URL-encode a string", "HttpService"),
        ("HttpService:GenerateGUID", "wrapInBraces?", "string", "Generate a UUID", "HttpService"),

        // DataStoreService
        ("DataStoreService:GetDataStore", "name", "DataStore", "Get a named DataStore", "DataStoreService"),
        ("DataStore:GetAsync", "key", "any", "Read value by key", "DataStoreService"),
        ("DataStore:SetAsync", "key, value", "", "Write value by key", "DataStoreService"),
        ("DataStore:RemoveAsync", "key", "", "Remove key", "DataStoreService"),
        ("DataStore:IncrementAsync", "key, delta?", "number", "Atomic increment", "DataStoreService"),

        // MarketplaceService
        ("MarketplaceService:PromptPurchase", "player, productId", "", "Open purchase dialog", "MarketplaceService"),
        ("MarketplaceService:GetProductInfo", "productId", "ProductInfo", "Get product metadata", "MarketplaceService"),
        ("MarketplaceService:PlayerOwnsGamePass", "player, passId", "bool", "Check pass ownership", "MarketplaceService"),

        // Sound
        ("Sound:Play", "", "", "Start audio playback", "Sound"),
        ("Sound:Stop", "", "", "Stop audio playback", "Sound"),

        // Players
        ("Players:GetPlayerByUserId", "userId", "Player?", "Find player by user ID", "Players"),
        ("Players.LocalPlayer", "", "Player", "Get the local player", "Players"),

        // UserInputService
        ("UserInputService:IsKeyDown", "keyCode", "bool", "Check if key is held", "UserInputService"),
        ("UserInputService:IsMouseButtonPressed", "button", "bool", "Check mouse button state", "UserInputService"),
        ("UserInputService:GetMouseLocation", "", "Vector2", "Get mouse screen position", "UserInputService"),
        ("UserInputService:GetMouseDelta", "", "Vector2", "Get mouse movement since last frame", "UserInputService"),

        // GUI
        ("gui_set_text", "elementName, text", "", "Set GUI element text content", "GUI"),
        ("gui_get_text", "elementName", "string", "Get GUI element text content", "GUI"),
        ("gui_set_visible", "elementName, visible", "", "Show/hide GUI element", "GUI"),
        ("gui_set_bg_color", "elementName, r, g, b", "", "Set background color (0-1)", "GUI"),
        ("gui_set_text_color", "elementName, r, g, b", "", "Set text color (0-1)", "GUI"),
        ("gui_set_position", "elementName, x, y", "", "Set GUI element position", "GUI"),
        ("gui_set_size", "elementName, w, h", "", "Set GUI element size", "GUI"),
        ("gui_set_font_size", "elementName, size", "", "Set font size", "GUI"),

        // Global
        ("print", "args...", "", "Print to Output panel", "Global Functions"),
        ("warn", "args...", "", "Print warning to Output panel", "Global Functions"),
        ("typeof", "value", "string", "Get type name (detects Vector3, CFrame, Color3, etc.)", "Global Functions"),
    ];

    defs.iter().map(|(name, params, ret, doc, category)| {
        let status = status_map.get(*name)
            .map(|(luau_status, _)| *luau_status)
            .unwrap_or(ApiStatus::Implemented);

        let params_vec: Vec<ApiParam> = if params.is_empty() {
            Vec::new()
        } else {
            params.split(',').map(|p| {
                let p = p.trim();
                let optional = p.ends_with('?');
                let clean = p.trim_end_matches('?').trim_end_matches("...");
                ApiParam { name: clean.to_string(), typ: "any".to_string(), optional }
            }).collect()
        };

        let example = if ret.is_empty() {
            format!("{}({})", name, params)
        } else {
            format!("local result = {}({})", name, params)
        };

        ApiEntry {
            name: name.to_string(),
            params: params_vec,
            return_type: ret.to_string(),
            doc: doc.to_string(),
            category: category.to_string(),
            language: ScriptLanguage::Luau,
            status,
            example,
        }
    }).collect()
}

// ============================================================================
// Checklist Parser
// ============================================================================

/// Parse SCRIPTING_API_CHECKLIST.md tables to extract implementation status.
/// Returns map of function_name → (luau_status, rune_status).
fn parse_checklist_statuses(source: &str) -> std::collections::HashMap<String, (ApiStatus, ApiStatus)> {
    let mut map = std::collections::HashMap::new();

    for line in source.lines() {
        let line = line.trim();
        // Match table rows: | `name(...)` | status | status | notes |
        if !line.starts_with('|') || line.starts_with("| ---") || line.starts_with("| Roblox") {
            continue;
        }

        let cols: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cols.len() < 5 { continue; }

        // Column 1: function/type name (may be wrapped in backticks)
        let raw_name = cols[1].trim_matches('`').trim();
        // Strip parameter list if present: "print(...)" → "print"
        let name = if let Some(paren) = raw_name.find('(') {
            &raw_name[..paren]
        } else {
            raw_name
        };
        if name.is_empty() { continue; }

        // Columns 2 and 3: Luau and Rune status
        let luau_status = parse_status_cell(cols[2]);
        let rune_status = parse_status_cell(cols[3]);

        map.insert(name.to_string(), (luau_status, rune_status));
    }

    map
}

/// Parse a status cell from the checklist table.
fn parse_status_cell(cell: &str) -> ApiStatus {
    let cell = cell.trim();
    if cell.contains('\u{2705}') || cell.contains("✅") {  // checkmark
        ApiStatus::Implemented
    } else if cell.contains('\u{1F536}') || cell.contains("🔶") {  // orange diamond
        ApiStatus::Partial
    } else if cell.contains('\u{274C}') || cell.contains("❌") {  // cross
        ApiStatus::NotStarted
    } else if cell.contains('\u{2796}') || cell.contains("➖") {  // minus
        ApiStatus::NotApplicable
    } else if cell.contains('\u{1F537}') || cell.contains("🔷") {  // blue diamond
        ApiStatus::Extension
    } else {
        ApiStatus::NotStarted
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Clean a section comment into a category name.
fn clean_category(comment: &str) -> String {
    let c = comment
        .trim_start_matches("P2:")
        .trim_start_matches("P1:")
        .trim()
        .trim_end_matches("API")
        .trim_end_matches("—")
        .trim_end_matches('-')
        .trim();

    if c.is_empty() { return "General".to_string(); }

    // Capitalize first letter
    let mut chars = c.chars();
    match chars.next() {
        None => "General".to_string(),
        Some(first) => {
            let rest: String = chars.collect();
            format!("{}{}", first.to_uppercase(), rest)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_catalog() {
        let catalog = ApiCatalog::build();
        assert!(catalog.entries.len() > 100,
            "Expected 100+ entries, got {}", catalog.entries.len());
        assert!(!catalog.categories.is_empty(), "No categories found");
        assert!(catalog.rune_type_names.len() > 5,
            "Expected 5+ Rune types, got {}", catalog.rune_type_names.len());

        // Verify key functions exist
        let names: Vec<&str> = catalog.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"part_set_position"), "Missing part_set_position");
        assert!(names.contains(&"workspace_raycast"), "Missing workspace_raycast");
        assert!(names.contains(&"camera_get_position"), "Missing camera_get_position");
        assert!(names.contains(&"Instance.new"), "Missing Instance.new");

        // Verify reference format
        let reference = catalog.format_full_reference();
        assert!(reference.contains("Rune API"));
        assert!(reference.contains("Luau API"));
        assert!(reference.contains("part_set_position"));
    }

    #[test]
    fn test_json_export() {
        let catalog = ApiCatalog::build();
        let json = catalog.to_json();
        assert!(json.contains("\"entries\""));
        assert!(json.contains("\"categories\""));
        // Verify roundtrip
        let _: ApiCatalog = serde_json::from_str(&json).expect("JSON roundtrip failed");
    }

    #[test]
    fn test_checklist_parsing() {
        let checklist = include_str!("../../../../../docs/development/SCRIPTING_API_CHECKLIST.md");
        let statuses = parse_checklist_statuses(checklist);
        assert!(!statuses.is_empty(), "No statuses parsed from checklist");
        // print(...) should be Implemented in both
        if let Some((luau, rune)) = statuses.get("print") {
            assert_eq!(*luau, ApiStatus::Implemented);
            assert_eq!(*rune, ApiStatus::Implemented);
        }
    }
}
