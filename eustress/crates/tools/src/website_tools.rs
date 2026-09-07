// =============================================================================
// Website Service tools
// =============================================================================
// Authoring the Website service and its References from an agent.
//
// These are FILE tools, not bridge tools. Setting up a Website service is
// writing `_service.toml` and a folder of `_instance.toml` files, and none of
// that needs a running engine. Resolution against the live datamodel happens at
// publish, which is where specification 3.1 requires it; an agent that could
// only author while the editor was open would be useless for the case these
// exist for, which is wiring a Space up from a script.
//
// See docs/design/WEBSITE_SERVICE.md for the model these write.
// =============================================================================

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Folder name of the service inside a Space. Fixed, because three lookups key
/// off the class name while the Explorer row is built from the folder on disk,
/// so folder and class have to agree.
const SERVICE_FOLDER: &str = "Website";

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

fn service_dir(ctx: &ToolContext) -> PathBuf {
    ctx.space_root.join(SERVICE_FOLDER)
}

fn service_file(ctx: &ToolContext) -> PathBuf {
    service_dir(ctx).join("_service.toml")
}

/// Read a flat scalar out of the `[service]` table.
///
/// Flat under `[service]` and not a nested table, because the loader runs each
/// `[service]` value through `toml_to_property_value`, which drops tables, and
/// the saver then rewrites the file from that map. A nested table does not
/// survive the first save of the service.
fn service_scalar(text: &str, key: &str) -> Option<String> {
    let doc: toml::Value = text.parse().ok()?;
    let v = doc.get("service")?.get(key)?;
    Some(match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// The five kinds a Reference can be, and what each resolves against.
const KINDS: &[(&str, &str)] = &[
    ("instance", "a property on one instance, e.g. Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"),
    ("sim", "a published simulation value; requires run_label so the figure can be reproduced"),
    ("count", "entities matching a path glob, e.g. Workspace/V-Cell/V1/Assembly/**"),
    ("measure", "a geometric measure over a subtree, e.g. bbox:Workspace/V-Cell/V1/Assembly"),
    ("expr", "an expression over other reference keys in the same namespace"),
];

// ---------------------------------------------------------------------------
// website_status
// ---------------------------------------------------------------------------

pub struct WebsiteStatusTool;

impl ToolHandler for WebsiteStatusTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_status",
            description: "Report the Website service in the current Space: whether it exists, its namespace and schema_version, whether a manifest auth key is set, and every Reference it holds with its kind and source. Use this before adding references, and to check what a publish would bake. Read-only.",
            input_schema: json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        let dir = service_dir(ctx);
        let file = service_file(ctx);
        if !file.exists() {
            return ok(
                "website_status",
                format!(
                    "No Website service in this Space.\n\
                     Run website_setup to create one at {}.\n\
                     A Space without a Website service publishes exactly as it does today; \
                     the manifest is opt-in.",
                    dir.display()
                ),
                json!({ "exists": false }),
            );
        }

        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => return err("website_status", format!("Could not read {}: {e}", file.display())),
        };

        let namespace = service_scalar(&text, "namespace").unwrap_or_default();
        let schema_version = service_scalar(&text, "schema_version").unwrap_or_else(|| "1".into());
        let key_id = service_scalar(&text, "key_id").unwrap_or_default();

        // Every child folder holding an _instance.toml is a candidate Reference.
        let mut refs: Vec<Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut names: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            names.sort();
            for path in names {
                let inst = path.join("_instance.toml");
                if !inst.exists() {
                    continue;
                }
                let Ok(t) = std::fs::read_to_string(&inst) else { continue };
                let doc: toml::Value = match t.parse() {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let class = doc
                    .get("metadata")
                    .and_then(|m| m.get("class_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if class != "Reference" {
                    continue;
                }
                let attrs = doc.get("attributes");
                let get = |k: &str| {
                    attrs
                        .and_then(|a| a.get(k))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                refs.push(json!({
                    "key": path.file_name().and_then(|s| s.to_str()).unwrap_or_default(),
                    "kind": get("kind"),
                    "source": get("source"),
                    "label": get("label"),
                    "unit": get("unit"),
                    "basis": get("basis"),
                }));
            }
        }

        let mut lines = vec![
            format!("Website service: {}", dir.display()),
            format!("  namespace:      {}", if namespace.is_empty() { "(unset - publish will fail)" } else { &namespace }),
            format!("  schema_version: {schema_version}"),
            format!("  auth key:       {}", if key_id.is_empty() { "none (manifest is open)" } else { &key_id }),
            format!("  references:     {}", refs.len()),
        ];
        for r in &refs {
            lines.push(format!(
                "    - {} [{}] {}",
                r["key"].as_str().unwrap_or("?"),
                r["kind"].as_str().unwrap_or("?"),
                r["source"].as_str().unwrap_or("")
            ));
        }
        if !namespace.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "  consumers fetch: https://api.eustress.dev/api/simulation/{namespace}/latest/manifest"
            ));
        }

        ok(
            "website_status",
            lines.join("\n"),
            json!({
                "exists": true,
                "namespace": namespace,
                "schema_version": schema_version,
                "key_id": key_id,
                "references": refs,
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_setup
// ---------------------------------------------------------------------------

pub struct WebsiteSetupTool;

impl ToolHandler for WebsiteSetupTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_setup",
            description: "Create or update the Website service in the current Space, setting the namespace a manifest publishes under and the schema_version consumers pin against. Creating it is safe and idempotent. Bump schema_version only when the MEANING of a key changes rather than its value: consumers hard-stop on a mismatch, so bumping it for a value change stops every website on a day nothing needed to change.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "namespace": {
                        "type": "string",
                        "description": "Stable identifier the manifest publishes under, e.g. 'vcell'. Appears in the consumer URL and in every data-eus attribute. Lowercase letters, digits and hyphens."
                    },
                    "schema_version": {
                        "type": "integer",
                        "description": "Consumer contract version. Defaults to 1 on creation. Bump only on a meaning change.",
                        "minimum": 1
                    }
                },
                "required": ["namespace"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let namespace = input.get("namespace").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if namespace.is_empty() {
            return err("website_setup", "namespace is required");
        }
        // The namespace becomes part of a URL and an R2 object key, so it is
        // validated here rather than at publish, where the author has already
        // moved on.
        if !namespace.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return err(
                "website_setup",
                format!("namespace '{namespace}' must be lowercase letters, digits and hyphens only. It becomes part of a URL and a storage key."),
            );
        }

        let dir = service_dir(ctx);
        let file = service_file(ctx);
        let existed = file.exists();

        let previous = std::fs::read_to_string(&file).unwrap_or_default();
        let schema_version = input
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .or_else(|| service_scalar(&previous, "schema_version").and_then(|s| s.parse().ok()))
            .unwrap_or(1);
        // Preserve a key that is already minted. Silently dropping it on a
        // namespace edit would un-protect a live manifest on the next publish.
        let key_id = service_scalar(&previous, "key_id").unwrap_or_default();

        if let Err(e) = std::fs::create_dir_all(&dir) {
            return err("website_setup", format!("Could not create {}: {e}", dir.display()));
        }

        let body = format!(
            "# Website service. Values a website reads, baked into one published\n\
             # manifest at publish time. See docs/design/WEBSITE_SERVICE.md.\n\
             #\n\
             # Flat scalars under [service]: the loader drops nested tables and the\n\
             # saver rewrites this file from that map, so a nested table would not\n\
             # survive the first save.\n\
             \n\
             [service]\n\
             class_name = \"Website\"\n\
             icon = \"website\"\n\
             description = \"Values a website reads - References baked into a published manifest\"\n\
             can_have_children = true\n\
             \n\
             # Stable identifier the manifest publishes under. Appears in the\n\
             # consumer URL and in every data-eus attribute on the page.\n\
             namespace = \"{namespace}\"\n\
             \n\
             # Bumped when the MEANING of a key changes, never when its value does.\n\
             # Consumers pin against this and hard-stop on a mismatch.\n\
             schema_version = {schema_version}\n\
             \n\
             # Handle of the current manifest key. NEVER the key itself: this file\n\
             # is tracked by git and packaged into the .pak, so a key written here\n\
             # is readable by anyone who never visits the site, which removes the\n\
             # only thing the key buys.\n\
             key_id = \"{key_id}\"\n\
             \n\
             [metadata]\n\
             class_name = \"Website\"\n"
        );

        if let Err(e) = std::fs::write(&file, body) {
            return err("website_setup", format!("Could not write {}: {e}", file.display()));
        }

        ok(
            "website_setup",
            format!(
                "{} Website service\n  namespace:      {namespace}\n  schema_version: {schema_version}\n\
                 \n\
                 Next: website_add_reference to mark values, then Publish.\n\
                 Consumers will fetch:\n  https://api.eustress.dev/api/simulation/{namespace}/latest/manifest",
                if existed { "Updated" } else { "Created" }
            ),
            json!({
                "created": !existed,
                "namespace": namespace,
                "schema_version": schema_version,
                "manifest_url": format!("https://api.eustress.dev/api/simulation/{namespace}/latest/manifest"),
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_add_reference
// ---------------------------------------------------------------------------

pub struct WebsiteAddReferenceTool;

impl ToolHandler for WebsiteAddReferenceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_add_reference",
            description: "Add or replace one Reference in the Website service. A Reference is a POINTER, never a copy: it stores the path to a value and the publish resolves it live. That is the whole point, because a cached copy drifts from the model it came from. The folder name becomes the manifest key and the data-eus attribute a page uses.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Manifest key, e.g. 'specific_energy'. Becomes data-eus=\"{namespace}:{key}\" on the page."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["instance", "sim", "count", "measure", "expr"],
                        "description": "instance: a property on one instance. sim: a published simulation value, requires run_label. count: entities matching a path glob. measure: a geometric measure over a subtree. expr: an expression over other reference keys."
                    },
                    "source": {
                        "type": "string",
                        "description": "What to resolve. instance: Path/To/Instance#section.field. sim: the simulation value name. count: a path glob. measure: bbox:Path/To/Subtree. expr: an expression over other keys."
                    },
                    "label": { "type": "string", "description": "Human label carried into the manifest." },
                    "unit": { "type": "string", "description": "Unit for display, e.g. 'Wh/kg'." },
                    "format": { "type": "string", "description": "Format string for `display`, e.g. '{:.0}'. The consumer renders display and never reformats value." },
                    "basis": {
                        "type": "string",
                        "enum": ["measured", "simulated", "derived", "counted"],
                        "description": "Where the number came from. Required for instance, sim and expr; count and measure imply their own. A figure without its basis is not a figure."
                    },
                    "run_label": { "type": "string", "description": "sim only. The run this number came from. Without it the publish fails, because a figure nobody can reproduce is not a figure." },
                    "at": { "type": "string", "description": "sim only: final | min | max | mean | at_cycle:<n>. Defaults to final." },
                    "axis": { "type": "string", "description": "measure only: x | y | z | volume | surface." },
                    "filter": { "type": "string", "description": "count only, e.g. 'class_name = Part'." }
                },
                "required": ["key", "kind", "source"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let key = s("key");
        let kind = s("kind");
        let source = s("source");

        if key.is_empty() || kind.is_empty() || source.is_empty() {
            return err("website_add_reference", "key, kind and source are all required");
        }
        if !KINDS.iter().any(|(k, _)| *k == kind) {
            let list: Vec<String> = KINDS.iter().map(|(k, d)| format!("  {k}: {d}")).collect();
            return err(
                "website_add_reference",
                format!("kind '{kind}' is not one of the five.\n{}", list.join("\n")),
            );
        }
        // The key becomes a folder name and a JSON object key, so it is checked
        // here rather than producing a manifest a consumer cannot address.
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return err(
                "website_add_reference",
                format!("key '{key}' must be letters, digits and underscores: it becomes a folder name and a data-eus attribute."),
            );
        }

        let basis = s("basis");
        let implied = matches!(kind.as_str(), "count" | "measure");
        if basis.is_empty() && !implied {
            return err(
                "website_add_reference",
                format!("basis is required for kind '{kind}'. Use measured, simulated, derived or counted. A figure without its basis is not a figure, and guessing one for a number measured on a rig would be a guess a reader cannot see."),
            );
        }
        let run_label = s("run_label");
        if kind == "sim" && run_label.is_empty() {
            return err(
                "website_add_reference",
                "run_label is required for a sim reference. A number lifted from whatever happened to be in memory is how a figure ends up on a website with no way to reproduce it.",
            );
        }

        if !service_file(ctx).exists() {
            return err(
                "website_add_reference",
                "No Website service in this Space. Run website_setup first.",
            );
        }

        let dir = service_dir(ctx).join(&key);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return err("website_add_reference", format!("Could not create {}: {e}", dir.display()));
        }

        let mut attrs = vec![
            format!("kind = \"{kind}\""),
            format!("source = \"{source}\""),
        ];
        for (field, value) in [
            ("label", s("label")),
            ("unit", s("unit")),
            ("format", s("format")),
            ("basis", basis.clone()),
            ("run_label", run_label.clone()),
            ("at", s("at")),
            ("axis", s("axis")),
            ("filter", s("filter")),
        ] {
            if !value.is_empty() {
                attrs.push(format!("{field} = \"{value}\""));
            }
        }

        let body = format!(
            "# Reference: a POINTER to a value, never a copy of it.\n\
             # The publish resolves `source` against the live datamodel, so the\n\
             # model stays the one source of truth. A cached copy would drift,\n\
             # which is the failure this whole feature exists to prevent.\n\
             \n\
             [metadata]\n\
             class_name = \"Reference\"\n\
             archivable = true\n\
             \n\
             [attributes]\n{}\n",
            attrs.join("\n")
        );

        let file = dir.join("_instance.toml");
        let existed = file.exists();
        if let Err(e) = std::fs::write(&file, body) {
            return err("website_add_reference", format!("Could not write {}: {e}", file.display()));
        }

        let namespace = std::fs::read_to_string(service_file(ctx))
            .ok()
            .and_then(|t| service_scalar(&t, "namespace"))
            .unwrap_or_default();

        ok(
            "website_add_reference",
            format!(
                "{} reference '{key}' [{kind}]\n  source: {source}\n\n\
                 On the page:\n  <span data-eus=\"{namespace}:{key}\">CURRENT VALUE HERE</span>\n\n\
                 The element's existing text is the fallback and must already be correct. \
                 A page that renders empty until the fetch resolves renders empty when it fails.",
                if existed { "Replaced" } else { "Added" }
            ),
            json!({
                "key": key, "kind": kind, "source": source,
                "replaced": existed,
                "data_eus": format!("{namespace}:{key}"),
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_remove_reference
// ---------------------------------------------------------------------------

pub struct WebsiteRemoveReferenceTool;

impl ToolHandler for WebsiteRemoveReferenceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_remove_reference",
            description: "Remove one Reference from the Website service. The next publish drops that key from the manifest, so any page still carrying its data-eus attribute keeps showing its baked fallback text and logs a missing key. Check website_status first.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Manifest key to remove." }
                },
                "required": ["key"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
        if key.is_empty() {
            return err("website_remove_reference", "key is required");
        }
        let dir = service_dir(ctx).join(key);
        if !dir.join("_instance.toml").exists() {
            return err(
                "website_remove_reference",
                format!("No reference '{key}' in the Website service. Run website_status to list them."),
            );
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            return err("website_remove_reference", format!("Could not remove {}: {e}", dir.display()));
        }
        ok(
            "website_remove_reference",
            format!(
                "Removed reference '{key}'.\nAny page still carrying data-eus for it will keep its baked \
                 fallback and log a missing key on the next fetch."
            ),
            json!({ "removed": key }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_manifest_url
// ---------------------------------------------------------------------------

pub struct WebsiteManifestUrlTool;

impl ToolHandler for WebsiteManifestUrlTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_manifest_url",
            description: "Return the URL a consumer fetches for this Space's manifest, with the markup and hydration a page needs. Use this when handing integration instructions to whoever maintains the website. Read-only.",
            input_schema: json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        let file = service_file(ctx);
        let Ok(text) = std::fs::read_to_string(&file) else {
            return err("website_manifest_url", "No Website service in this Space. Run website_setup first.");
        };
        let namespace = service_scalar(&text, "namespace").unwrap_or_default();
        if namespace.is_empty() {
            return err("website_manifest_url", "The Website service has no namespace set. Run website_setup.");
        }
        let url = format!("https://api.eustress.dev/api/simulation/{namespace}/latest/manifest");
        let key_id = service_scalar(&text, "key_id").unwrap_or_default();

        ok(
            "website_manifest_url",
            format!(
                "GET {url}\n\
                 {}\n\n\
                 Markup:\n  <span data-eus=\"{namespace}:KEY\">CURRENT VALUE</span>\n\n\
                 The element's existing text is the fallback and must already be correct. \
                 Bake current values in at build time and let the manifest correct them: the page \
                 is right before the fetch, and the fetch can only improve it.\n\n\
                 Full integration guide: docs/api/WEBSITE_MANIFEST_INTEGRATION.md",
                if key_id.is_empty() {
                    "  (no auth key set: this manifest is open)".to_string()
                } else {
                    format!("  X-Eustress-Key: <the key for {key_id}>")
                }
            ),
            json!({ "manifest_url": url, "namespace": namespace, "key_id": key_id }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &Path) -> ToolContext {
        // Constructed field by field: ToolContext does not implement Default,
        // and a struct-update shorthand would break the moment a field is added.
        ToolContext {
            space_root: dir.to_path_buf(),
            universe_root: dir.to_path_buf(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            // Default grants Read and Write but not Execute, Destructive or
            // Network, which is what these tools need.
            permissions: crate::capability::Permissions::default(),
        }
    }

    #[test]
    fn a_namespace_that_would_break_a_url_is_refused() {
        let tmp = std::env::temp_dir().join(format!("eus_ws_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let r = WebsiteSetupTool.execute(json!({ "namespace": "V Cell/latest" }), &ctx(&tmp));
        assert!(!r.success, "a namespace with a space and a slash must be refused");
    }

    #[test]
    fn a_sim_reference_without_a_run_label_is_refused() {
        let tmp = std::env::temp_dir().join(format!("eus_ws_sim_{}", std::process::id()));
        let _ = std::fs::create_dir_all(tmp.join(SERVICE_FOLDER));
        let _ = std::fs::write(tmp.join(SERVICE_FOLDER).join("_service.toml"), "[service]\nnamespace = \"x\"\n");
        let r = WebsiteAddReferenceTool.execute(
            json!({ "key": "cycles", "kind": "sim", "source": "battery.retention", "basis": "simulated" }),
            &ctx(&tmp),
        );
        assert!(!r.success, "a sim reference with no run_label must fail: the figure could not be reproduced");
    }

    #[test]
    fn count_and_measure_do_not_need_a_declared_basis() {
        let tmp = std::env::temp_dir().join(format!("eus_ws_cnt_{}", std::process::id()));
        let _ = std::fs::create_dir_all(tmp.join(SERVICE_FOLDER));
        let _ = std::fs::write(tmp.join(SERVICE_FOLDER).join("_service.toml"), "[service]\nnamespace = \"x\"\n");
        let r = WebsiteAddReferenceTool.execute(
            json!({ "key": "part_count", "kind": "count", "source": "Workspace/**" }),
            &ctx(&tmp),
        );
        assert!(r.success, "a count is self-describing: {}", r.content);
    }
}
