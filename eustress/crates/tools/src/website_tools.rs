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
            description: "Report the Website service in the current Space: whether it exists, its namespace and schema_version, whether a manifest auth key is set, every Reference it holds with its kind and source, and its README.md (the sim page's About tab: size against the 32 KiB cap, how many of its at most 8 images would upload, and every image that would show as its alt text, and why). Use this before adding references, and to check what a publish would bake. To add an About tab, write Website/README.md in the Space; its images are files inside the Space, written relative to the Website folder like a GitHub README (PNG, JPEG or WebP, 2 MB each). Read-only.",
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
        let (readme_line, readme_warnings, readme) = readme_status(ctx);
        lines.push(format!("  README.md:      {readme_line}"));
        for w in &readme_warnings {
            lines.push(format!("    warning: {w}"));
        }
        if !namespace.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "  consumers fetch: {api}/api/simulation/{namespace}/latest/manifest",
                api = eustress_common::api_base::api_base()
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
                "readme": readme,
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

        if let Err(e) = std::fs::write(&file, set_service_scalars(&body, &carried_page_scalars(&previous))) {
            return err("website_setup", format!("Could not write {}: {e}", file.display()));
        }

        ok(
            "website_setup",
            format!(
                "{} Website service\n  namespace:      {namespace}\n  schema_version: {schema_version}\n\
                 \n\
                 Next: website_add_reference to mark values, then Publish.\n\
                 Consumers will fetch:\n  {api}/api/simulation/{namespace}/latest/manifest",
                if existed { "Updated" } else { "Created" },
                api = eustress_common::api_base::api_base()
            ),
            json!({
                "created": !existed,
                "namespace": namespace,
                "schema_version": schema_version,
                "manifest_url": eustress_common::api_base::api_url(&format!("/api/simulation/{namespace}/latest/manifest")),
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
        // The key names a folder that is removed, so it follows the rule
        // website_add_reference gave it: letters, digits and underscores.
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return err(
                "website_remove_reference",
                format!("key '{key}' must be letters, digits and underscores, as website_add_reference made it."),
            );
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
        let url = eustress_common::api_base::api_url(&format!("/api/simulation/{namespace}/latest/manifest"));
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

// ---------------------------------------------------------------------------
// The sim page: theme, images and off-site links
// ---------------------------------------------------------------------------
//
// The Website service also themes the public page of the Space's simulation
// and lists its off-site links (PageSpec v1, docs/design/SIM_PAGE_THEME_AND_LINKS.md).
// The theme and images are flat scalars under [service] in _service.toml; each
// link is a PageLink folder child, as a Reference is. Publish bakes both
// (engine/src/website/page.rs). The rules live in eustress_common::page_spec,
// shared with the bake, so a value these tools accept is a value the bake sends.
//
// Scripts: the page is static, so there is no runtime script API. A Luau or
// Rune script that can already read and write service properties generically
// reaches these same keys; nothing here adds to or restricts that.

use eustress_common::page_spec::{self, PageInput};

/// `Website/README.md` as a publish would bake it, for website_status: a one
/// line summary, every warning (an image shown as its alt text, or the file
/// refused), and the same as data. The rules are the bake's
/// (`page_spec::check_readme_text` and `page_spec::rewrite_readme`); image
/// paths come out of the rewrite resolved inside the Space, so the reads stay
/// in it.
fn readme_status(ctx: &ToolContext) -> (String, Vec<String>, Value) {
    let max = page_spec::README_MAX_BYTES;
    let Ok(bytes) = std::fs::read(service_dir(ctx).join(page_spec::README_FILE)) else {
        return (
            "none (write Website/README.md to give the sim page an About tab)".to_string(),
            Vec::new(),
            json!({ "exists": false, "max_bytes": max, "max_images": page_spec::README_MAX_IMAGES }),
        );
    };
    let text = match page_spec::check_readme_text(&bytes) {
        Ok(text) => text,
        Err(e) => {
            let warning = format!("README.md is not uploaded: {e}");
            return (
                format!("{} bytes, refused", bytes.len()),
                vec![warning.clone()],
                json!({ "exists": true, "bytes": bytes.len(), "max_bytes": max, "uploads": false, "warnings": [warning] }),
            );
        }
    };
    if text.trim().is_empty() {
        return (
            "empty (nothing uploads)".to_string(),
            Vec::new(),
            json!({ "exists": true, "bytes": bytes.len(), "max_bytes": max, "uploads": false, "warnings": [] }),
        );
    }
    let space_root = ctx.space_root.clone();
    let build = page_spec::rewrite_readme(text, |rel| {
        let image = std::fs::read(space_root.join(rel)).map_err(|_| format!("there is no file {rel} in the Space"))?;
        page_spec::check_image_bytes(&image).map(|_| ()).map_err(|e| format!("{rel}: {e}"))
    });
    let mut warnings = build.warnings;
    let uploads = build.markdown.len() <= max;
    if !uploads {
        warnings.push(format!(
            "README.md is not uploaded: with its images pointed at their slots it is {} bytes; the limit is {max}",
            build.markdown.len()
        ));
    }
    let images: Vec<Value> = build.images.iter().map(|(slot, path)| json!({ "slot": slot, "path": path })).collect();
    let line = format!(
        "{} of {max} bytes, {} of {} image(s){}; reviewed before it shows",
        bytes.len(),
        images.len(),
        page_spec::README_MAX_IMAGES,
        if uploads { ", uploads at publish" } else { ", refused" }
    );
    let data = json!({
        "exists": true,
        "bytes": bytes.len(),
        "max_bytes": max,
        "uploads": uploads,
        "images": images,
        "warnings": warnings,
    });
    (line, warnings, data)
}

/// Read `Website/_service.toml`, or say to run website_setup.
fn read_service_text(ctx: &ToolContext) -> Result<String, String> {
    std::fs::read_to_string(service_file(ctx))
        .map_err(|_| "No Website service in this Space. Run website_setup first.".to_string())
}

/// Every page setting in the Space: the theme and image fields, and each
/// PageLink child in folder-name order.
fn read_page_input(ctx: &ToolContext) -> Result<PageInput, String> {
    let text = read_service_text(ctx)?;
    let doc: toml::Value = text.parse().map_err(|e: toml::de::Error| format!("_service.toml: {e}"))?;
    let mut input = PageInput::default();
    page_spec::read_service_doc(&doc, &mut input);
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(service_dir(ctx)) {
        Ok(entries) => entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    for dir in dirs {
        let Ok(t) = std::fs::read_to_string(dir.join("_instance.toml")) else { continue };
        let Ok(d) = t.parse::<toml::Value>() else { continue };
        let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        if let Some(link) = page_spec::link_from_instance_doc(name, &d) {
            input.links.push(link);
        }
    }
    Ok(input)
}

/// `text` with each `(key, TOML literal)` set under `[service]`: an existing
/// line is replaced in place, a missing key is added at the end of the
/// section, and everything else (comments, other keys, line endings) stays.
fn set_service_scalars(text: &str, updates: &[(String, String)]) -> String {
    let crlf = text.contains("\r\n");
    let norm = text.replace("\r\n", "\n");
    let mut lines: Vec<String> = norm.split('\n').map(|l| l.to_string()).collect();
    let Some(start) = lines.iter().position(|l| l.trim() == "[service]") else {
        let mut out = norm.trim_end().to_string();
        out.push_str("\n\n[service]\n");
        for u in updates {
            out.push_str(&format!("{} = {}\n", u.0, u.1));
        }
        return if crlf { out.replace('\n', "\r\n") } else { out };
    };
    let mut end = lines.len();
    for i in (start + 1)..lines.len() {
        if lines[i].trim_start().starts_with('[') {
            end = i;
            break;
        }
    }
    let mut missing: Vec<&(String, String)> = Vec::new();
    for u in updates {
        let mut found = false;
        for i in (start + 1)..end {
            let t = lines[i].trim_start();
            if t.starts_with('#') {
                continue;
            }
            if let Some(rest) = t.strip_prefix(u.0.as_str()) {
                if rest.trim_start().starts_with('=') {
                    lines[i] = format!("{} = {}", u.0, u.1);
                    found = true;
                    break;
                }
            }
        }
        if !found {
            missing.push(u);
        }
    }
    if !missing.is_empty() {
        let mut at = end;
        while at > start + 1 && lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        let added: Vec<String> = missing.iter().map(|u| format!("{} = {}", u.0, u.1)).collect();
        lines.splice(at..at, added);
    }
    let out = lines.join("\n");
    if crlf {
        out.replace('\n', "\r\n")
    } else {
        out
    }
}

/// A string as a TOML literal, escaped.
fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn page_warnings_text(input: &PageInput) -> (String, Option<serde_json::Value>, Vec<String>) {
    let build = page_spec::build_page_spec(input);
    let summary = match &build.spec {
        Some(spec) => format!(
            "A publish would bake a page with {} link(s) and {} image(s).",
            spec["links"].as_array().map(|l| l.len()).unwrap_or(0),
            build.images.len()
        ),
        None => "Nothing is set, so a publish bakes no page.".to_string(),
    };
    (summary, build.spec, build.warnings)
}

// ---------------------------------------------------------------------------
// website_theme_get
// ---------------------------------------------------------------------------

pub struct WebsiteThemeGetTool;

impl ToolHandler for WebsiteThemeGetTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_theme_get",
            description: "Report the sim page theme in the Website service: preset, colour overrides, font, layout, motion, slideshow and banner timings, and the logo, banner and gallery image paths, plus the PageSpec a publish would bake and every warning (an invalid entry the bake would drop). Also lists the six presets with their colours. Read-only.",
            input_schema: json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        let input = match read_page_input(ctx) {
            Ok(i) => i,
            Err(e) => return err("website_theme_get", e),
        };
        let (summary, spec, warnings) = page_warnings_text(&input);
        let presets: Vec<Value> = page_spec::PRESETS
            .iter()
            .map(|p| {
                json!({ "name": p.name, "primary": p.primary, "accent": p.accent,
                        "background": p.background, "surface": p.surface, "text": p.text })
            })
            .collect();
        let shown = |s: &str| if s.trim().is_empty() { "default".to_string() } else { s.trim().to_string() };
        let colors: Vec<String> = page_spec::COLOR_KEYS
            .iter()
            .map(|k| format!("{k}={}", input.theme.colors.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty()).unwrap_or("(preset)")))
            .collect();
        let mut lines = vec![
            format!("Sim page theme: {}", service_file(ctx).display()),
            format!("  preset:   {}", shown(&input.theme.preset)),
            format!("  colors:   {}", colors.join(" ")),
            format!("  font:     {}", shown(&input.theme.font)),
            format!("  layout:   {}", shown(&input.theme.layout)),
            format!("  motion:   {}", shown(&input.theme.motion)),
            format!(
                "  slideshow_interval_s: {}   banner_fade_ms: {}",
                input.theme.slideshow_interval_s.unwrap_or(page_spec::DEFAULT_SLIDESHOW_INTERVAL_S),
                input.theme.banner_fade_ms.unwrap_or(page_spec::DEFAULT_BANNER_FADE_MS)
            ),
            format!("  logo:     {}", shown(&input.logo)),
            format!("  banner:   {}", shown(&input.banner)),
        ];
        for (i, g) in input.gallery.iter().enumerate() {
            if !g.trim().is_empty() {
                lines.push(format!("  gallery-{i}: {}", g.trim()));
            }
        }
        lines.push(format!("  links:    {} (website_links_list)", input.links.len()));
        lines.push(String::new());
        lines.push(summary);
        for w in &warnings {
            lines.push(format!("  warning: {w}"));
        }
        ok(
            "website_theme_get",
            lines.join("\n"),
            json!({
                "preset": input.theme.preset,
                "colors": input.theme.colors,
                "font": input.theme.font,
                "layout": input.theme.layout,
                "motion": input.theme.motion,
                "slideshow_interval_s": input.theme.slideshow_interval_s,
                "banner_fade_ms": input.theme.banner_fade_ms,
                "logo": input.logo,
                "banner": input.banner,
                "gallery": input.gallery,
                "page_spec": spec,
                "warnings": warnings,
                "presets": presets,
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_theme_set
// ---------------------------------------------------------------------------

pub struct WebsiteThemeSetTool;

impl ToolHandler for WebsiteThemeSetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_theme_set",
            description: "Set the sim page theme and images in the Website service. Every field is optional; only the ones given change. Validated against the same rules the publish bake and the Worker use, and nothing is written if any given value breaks one. Nothing reaches the public page until it is reviewed at publish. Use \"default\" for preset, font, layout or motion to clear it, an empty string to clear a colour or image.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "preset": { "type": "string", "enum": ["default", "studio", "daylight", "blueprint", "laboratory", "arcade", "paper"], "description": "A full colour set. default is studio." },
                    "colors": {
                        "type": "object",
                        "description": "Override single colours of the preset, each as #RRGGBB (empty string clears the override). Text must reach 4.5:1 contrast on background and surface or the page uses the preset's text colour.",
                        "properties": {
                            "primary": { "type": "string" },
                            "accent": { "type": "string" },
                            "background": { "type": "string" },
                            "surface": { "type": "string" },
                            "text": { "type": "string" }
                        }
                    },
                    "font": { "type": "string", "enum": ["default", "inter", "space-grotesk", "ibm-plex-sans", "jetbrains-mono", "merriweather"] },
                    "layout": { "type": "string", "enum": ["default", "classic", "wide", "compact"] },
                    "motion": { "type": "string", "enum": ["default", "none", "subtle", "full"], "description": "The page always honours a visitor's reduced-motion setting." },
                    "slideshow_interval_s": { "type": "integer", "minimum": 3, "maximum": 30, "description": "Seconds each gallery image shows." },
                    "banner_fade_ms": { "type": "integer", "minimum": 0, "maximum": 2000, "description": "Banner fade-in, milliseconds." },
                    "logo": { "type": "string", "description": "Logo image, a path inside the Space (PNG, JPEG or WebP, at most 2 MB). Empty clears it." },
                    "banner": { "type": "string", "description": "Banner image, a path inside the Space. Empty clears it." },
                    "gallery": { "type": "array", "maxItems": 6, "items": { "type": "string" }, "description": "Up to six gallery image paths inside the Space. Replaces the whole gallery." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let text = match read_service_text(ctx) {
            Ok(t) => t,
            Err(e) => return err("website_theme_set", e),
        };
        let mut updates: Vec<(String, String)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        let mut presets: Vec<&str> = vec![page_spec::UNSET];
        presets.extend(page_spec::preset_names());
        let mut fonts: Vec<&str> = vec![page_spec::UNSET];
        fonts.extend(page_spec::FONTS.iter().copied());
        let mut layouts: Vec<&str> = vec![page_spec::UNSET];
        layouts.extend(page_spec::LAYOUTS.iter().copied());
        let mut motions: Vec<&str> = vec![page_spec::UNSET];
        motions.extend(page_spec::MOTIONS.iter().copied());

        for (field, key, allowed) in [
            ("preset", "theme_preset", &presets),
            ("font", "theme_font", &fonts),
            ("layout", "theme_layout", &layouts),
            ("motion", "theme_motion", &motions),
        ] {
            let Some(v) = input.get(field) else { continue };
            match v.as_str().ok_or_else(|| "must be a string".to_string()).and_then(|s| page_spec::check_enum(s, allowed)) {
                Ok(s) => updates.push((key.to_string(), toml_string(&s))),
                Err(e) => errors.push(format!("{field}: {e}")),
            }
        }

        if let Some(colors) = input.get("colors") {
            match colors.as_object() {
                None => errors.push("colors: must be an object".to_string()),
                Some(map) => {
                    for (k, v) in map {
                        if !page_spec::COLOR_KEYS.contains(&k.as_str()) {
                            errors.push(format!("colors.{k}: the keys are {}", page_spec::COLOR_KEYS.join(", ")));
                            continue;
                        }
                        let s = v.as_str().unwrap_or("").trim();
                        if s.is_empty() {
                            updates.push((format!("theme_color_{k}"), toml_string("")));
                        } else if let Some(hex) = page_spec::normalise_hex(s) {
                            updates.push((format!("theme_color_{k}"), toml_string(&hex)));
                        } else {
                            errors.push(format!("colors.{k}: \"{s}\" is not #RRGGBB"));
                        }
                    }
                }
            }
        }

        for (field, range, default) in [
            ("slideshow_interval_s", page_spec::SLIDESHOW_INTERVAL_RANGE, page_spec::DEFAULT_SLIDESHOW_INTERVAL_S),
            ("banner_fade_ms", page_spec::BANNER_FADE_RANGE, page_spec::DEFAULT_BANNER_FADE_MS),
        ] {
            let Some(v) = input.get(field) else { continue };
            match v.as_i64() {
                Some(n) if (range.0..=range.1).contains(&n) => updates.push((field.to_string(), n.to_string())),
                Some(n) => errors.push(format!("{field}: {n} is outside {} to {} (default {default})", range.0, range.1)),
                None => errors.push(format!("{field}: must be a whole number")),
            }
        }

        // Image paths: the same rules as the bake, and the file has to be
        // there and be a PNG, JPEG or WebP of at most 2 MB.
        let mut gallery_values: Vec<Value> = Vec::new();
        let mut images: Vec<(String, &Value)> = Vec::new();
        for (field, key) in [("logo", "image_logo"), ("banner", "image_banner")] {
            if let Some(v) = input.get(field) {
                images.push((key.to_string(), v));
            }
        }
        if let Some(g) = input.get("gallery") {
            match g.as_array() {
                None => errors.push("gallery: must be an array of paths".to_string()),
                Some(items) if items.len() > page_spec::MAX_GALLERY => {
                    errors.push(format!("gallery: at most {} images", page_spec::MAX_GALLERY))
                }
                Some(items) => {
                    gallery_values = items.clone();
                    // Replacing the gallery clears the slots after the new ones.
                    gallery_values.resize(page_spec::MAX_GALLERY, Value::String(String::new()));
                }
            }
        }
        for (i, v) in gallery_values.iter().enumerate() {
            images.push((format!("image_gallery_{i}"), v));
        }
        for (key, v) in images {
            let raw = v.as_str().unwrap_or("").trim();
            if raw.is_empty() {
                updates.push((key, toml_string("")));
                continue;
            }
            let label = key.trim_start_matches("image_").replace('_', "-");
            match page_spec::clean_image_path(raw) {
                Err(e) => errors.push(format!("{label}: {e}")),
                Ok(clean) => match std::fs::read(ctx.space_root.join(&clean)) {
                    Err(_) => errors.push(format!("{label}: no file at {clean} in the Space")),
                    Ok(bytes) => match page_spec::check_image_bytes(&bytes) {
                        Err(e) => errors.push(format!("{label}: {clean}: {e}")),
                        Ok(_) => updates.push((key, toml_string(&clean))),
                    },
                },
            }
        }

        if !errors.is_empty() {
            return err("website_theme_set", format!("Nothing was changed.\n  {}", errors.join("\n  ")));
        }
        if updates.is_empty() {
            return err("website_theme_set", "Nothing to set: give at least one of preset, colors, font, layout, motion, slideshow_interval_s, banner_fade_ms, logo, banner, gallery.");
        }

        let file = service_file(ctx);
        if let Err(e) = std::fs::write(&file, set_service_scalars(&text, &updates)) {
            return err("website_theme_set", format!("Could not write {}: {e}", file.display()));
        }

        let after = read_page_input(ctx).unwrap_or_default();
        let (summary, spec, warnings) = page_warnings_text(&after);
        let mut lines = vec![format!("Set {} page field(s).", updates.len()), summary];
        for w in &warnings {
            lines.push(format!("  warning: {w}"));
        }
        lines.push("Nothing shows on the public page until it is reviewed at publish.".to_string());
        ok(
            "website_theme_set",
            lines.join("\n"),
            json!({ "changed": updates.iter().map(|u| u.0.clone()).collect::<Vec<_>>(), "page_spec": spec, "warnings": warnings }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_link_add
// ---------------------------------------------------------------------------

/// A PageLink's folder name must be a plain name: it is a folder under Website.
fn link_key_ok(key: &str) -> bool {
    !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub struct WebsiteLinkAddTool;

impl ToolHandler for WebsiteLinkAddTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_link_add",
            description: "Add or replace one off-site link on the sim page, as a PageLink under the Website service. Kinds: website (at most 1), social (8), shop (4; stores, payment and donation pages), other (4; docs, papers, press). The url must be https, on a public host name, with no user, password, port or IP address; the label is plain text of 1 to 40 characters. Every link is reviewed before it shows, and a risky one is removed with the reason. Child-directed listings may carry no links, and adult listings no shop links.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Folder name for the link, letters digits and underscores, e.g. 'youtube'. Reusing a key replaces that link." },
                    "kind": { "type": "string", "enum": ["website", "social", "shop", "other"] },
                    "url": { "type": "string", "description": "https:// URL, at most 500 characters. Link the destination itself, not a shortener." },
                    "label": { "type": "string", "description": "What the visitor reads, 1 to 40 characters, not a URL." }
                },
                "required": ["key", "kind", "url", "label"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let key = s("key");
        if !link_key_ok(&key) {
            return err("website_link_add", format!("key '{key}' must be 1 to 64 letters, digits and underscores: it becomes a folder name."));
        }
        let kind = match page_spec::validate_link_kind(&s("kind")) {
            Ok(k) => k,
            Err(e) => return err("website_link_add", format!("kind {e}")),
        };
        let url = match page_spec::validate_link_url(&s("url")) {
            Ok(u) => u,
            Err(e) => return err("website_link_add", format!("url: {e}")),
        };
        let label = match page_spec::validate_label(&s("label")) {
            Ok(l) => l,
            Err(e) => return err("website_link_add", format!("label: {e}")),
        };

        let page = match read_page_input(ctx) {
            Ok(p) => p,
            Err(e) => return err("website_link_add", e),
        };
        let cap = page_spec::LINK_KINDS.iter().find(|(k, _)| *k == kind).map(|(_, c)| *c).unwrap_or(0);
        let same_kind = page.links.iter().filter(|l| l.name != key && l.kind.trim().eq_ignore_ascii_case(&kind)).count();
        if same_kind >= cap {
            return err("website_link_add", format!("A page holds at most {cap} {kind} link(s) and it already has {same_kind}. Remove one with website_link_remove first."));
        }
        let total_others = page.links.iter().filter(|l| l.name != key).count();
        if total_others >= page_spec::MAX_LINKS {
            return err("website_link_add", format!("A page holds at most {} links.", page_spec::MAX_LINKS));
        }

        let dir = service_dir(ctx).join(&key);
        let file = dir.join("_instance.toml");
        // Never overwrite a folder that is something else, a Reference say.
        let existing = std::fs::read_to_string(&file).ok();
        let replaced = match &existing {
            None => false,
            Some(t) => {
                let is_link = t
                    .parse::<toml::Value>()
                    .ok()
                    .map(|d| page_spec::link_from_instance_doc(&key, &d).is_some())
                    .unwrap_or(false);
                if !is_link {
                    return err("website_link_add", format!("'{key}' in the Website service is not a PageLink (a Reference, say). Choose another key."));
                }
                true
            }
        };

        if let Err(e) = std::fs::create_dir_all(&dir) {
            return err("website_link_add", format!("Could not create {}: {e}", dir.display()));
        }
        let body = format!(
            "# PageLink: one off-site link on the sim's public page. Reviewed before it shows.\n\
             \n\
             [metadata]\n\
             class_name = \"PageLink\"\n\
             archivable = true\n\
             \n\
             [attributes]\n\
             kind = {}\n\
             url = {}\n\
             label = {}\n",
            toml_string(&kind),
            toml_string(&url),
            toml_string(&label)
        );
        if let Err(e) = std::fs::write(&file, body) {
            return err("website_link_add", format!("Could not write {}: {e}", file.display()));
        }
        ok(
            "website_link_add",
            format!(
                "{} {kind} link '{key}': {label} -> {url}\nIt is reviewed at publish; a risky link is removed and you are told why.",
                if replaced { "Replaced" } else { "Added" }
            ),
            json!({ "key": key, "kind": kind, "url": url, "label": label, "replaced": replaced }),
        )
    }
}

// ---------------------------------------------------------------------------
// website_link_remove
// ---------------------------------------------------------------------------

pub struct WebsiteLinkRemoveTool;

impl ToolHandler for WebsiteLinkRemoveTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_link_remove",
            description: "Remove one off-site link (a PageLink) from the Website service. The next publish drops it from the public page. Check website_links_list first.",
            input_schema: json!({
                "type": "object",
                "properties": { "key": { "type": "string", "description": "The link's folder name, as website_links_list shows it." } },
                "required": ["key"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
        if !link_key_ok(key) {
            return err("website_link_remove", format!("key '{key}' must be letters, digits and underscores, as website_link_add made it."));
        }
        let dir = service_dir(ctx).join(key);
        let is_link = std::fs::read_to_string(dir.join("_instance.toml"))
            .ok()
            .and_then(|t| t.parse::<toml::Value>().ok())
            .map(|d| page_spec::link_from_instance_doc(key, &d).is_some())
            .unwrap_or(false);
        if !is_link {
            return err("website_link_remove", format!("No PageLink '{key}' in the Website service. Run website_links_list to list them."));
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            return err("website_link_remove", format!("Could not remove {}: {e}", dir.display()));
        }
        ok("website_link_remove", format!("Removed link '{key}'. The next publish drops it from the page."), json!({ "removed": key }))
    }
}

// ---------------------------------------------------------------------------
// website_links_list
// ---------------------------------------------------------------------------

pub struct WebsiteLinksListTool;

impl ToolHandler for WebsiteLinksListTool {
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "website_links_list",
            description: "List the off-site links (PageLinks) in the Website service with their kind, url and label, and whether each passes the rules a publish applies (a link that does not is dropped at publish with the reason shown here). Counts per kind against the caps. Read-only.",
            input_schema: json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        let page = match read_page_input(ctx) {
            Ok(p) => p,
            Err(e) => return err("website_links_list", e),
        };
        let mut rows: Vec<Value> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        for l in &page.links {
            let problem = page_spec::validate_link_kind(&l.kind)
                .map(|_| ())
                .map_err(|e| format!("kind {e}"))
                .and_then(|_| page_spec::validate_link_url(&l.url).map(|_| ()).map_err(|e| format!("url: {e}")))
                .and_then(|_| page_spec::validate_label(&l.label).map(|_| ()).map_err(|e| format!("label: {e}")))
                .err();
            lines.push(format!(
                "  {} [{}] {} -> {}{}",
                l.name,
                l.kind,
                l.label,
                l.url,
                problem.as_ref().map(|p| format!("   DROPPED AT PUBLISH: {p}")).unwrap_or_default()
            ));
            rows.push(json!({ "key": l.name, "kind": l.kind, "url": l.url, "label": l.label, "valid": problem.is_none(), "problem": problem }));
        }
        let caps: Vec<String> = page_spec::LINK_KINDS
            .iter()
            .map(|(k, cap)| {
                let n = page.links.iter().filter(|l| l.kind.trim().eq_ignore_ascii_case(k)).count();
                format!("{k} {n}/{cap}")
            })
            .collect();
        let mut out = vec![format!("Page links: {} ({})", page.links.len(), caps.join(", "))];
        out.extend(lines);
        ok("website_links_list", out.join("\n"), json!({ "links": rows }))
    }
}

// ---------------------------------------------------------------------------
// Page keys that website_setup carries
// ---------------------------------------------------------------------------

/// Every page key the service holds and its default, as a TOML literal. The
/// shipped template carries them all; website_setup rewrites the file, so it
/// keeps what the author set and adds the rest, because the Properties panel
/// persists only keys already in the file.
const PAGE_DEFAULTS: &[(&str, &str)] = &[
    ("theme_preset", "\"default\""),
    ("theme_color_primary", "\"\""),
    ("theme_color_accent", "\"\""),
    ("theme_color_background", "\"\""),
    ("theme_color_surface", "\"\""),
    ("theme_color_text", "\"\""),
    ("theme_font", "\"default\""),
    ("theme_layout", "\"default\""),
    ("theme_motion", "\"default\""),
    ("slideshow_interval_s", "6"),
    ("banner_fade_ms", "600"),
    ("image_logo", "\"\""),
    ("image_banner", "\"\""),
    ("image_gallery_0", "\"\""),
    ("image_gallery_1", "\"\""),
    ("image_gallery_2", "\"\""),
    ("image_gallery_3", "\"\""),
    ("image_gallery_4", "\"\""),
    ("image_gallery_5", "\"\""),
];

/// The page keys of `previous` (a `_service.toml`), each with its value as a
/// TOML literal; a key the file lacks gets its default.
fn carried_page_scalars(previous: &str) -> Vec<(String, String)> {
    let doc: Option<toml::Value> = previous.parse().ok();
    PAGE_DEFAULTS
        .iter()
        .map(|(key, default)| {
            let kept = doc
                .as_ref()
                .and_then(|d| d.get("service"))
                .and_then(|s| s.get(*key))
                .filter(|v| v.is_str() || v.is_integer())
                .map(|v| v.to_string());
            (key.to_string(), kept.unwrap_or_else(|| default.to_string()))
        })
        .collect()
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
            mlua_executor: None,
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

    /// A Space with a Website service, in a directory of its own.
    fn page_space(tag: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("eus_ws_page_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(SERVICE_FOLDER)).unwrap();
        std::fs::write(
            tmp.join(SERVICE_FOLDER).join("_service.toml"),
            "# Website\r\n[service]\r\nclass_name = \"Website\"\r\nnamespace = \"x\"\r\ntheme_preset = \"default\"\r\n\r\n[metadata]\r\nid = \"website-service\"\r\n",
        )
        .unwrap();
        tmp
    }

    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn set_service_scalars_replaces_in_place_adds_missing_and_keeps_line_endings() {
        let text = "# c\r\n[service]\r\nnamespace = \"x\"\r\ntheme_preset = \"default\"\r\n\r\n[metadata]\r\nid = \"w\"\r\n";
        let out = set_service_scalars(
            text,
            &[("theme_preset".to_string(), "\"arcade\"".to_string()), ("banner_fade_ms".to_string(), "250".to_string())],
        );
        assert_eq!(
            out,
            "# c\r\n[service]\r\nnamespace = \"x\"\r\ntheme_preset = \"arcade\"\r\nbanner_fade_ms = 250\r\n\r\n[metadata]\r\nid = \"w\"\r\n"
        );
        // A key that is only a prefix of another is not that key.
        let out = set_service_scalars("[service]\ntheme_preset_x = 1\n", &[("theme_preset".to_string(), "2".to_string())]);
        assert_eq!(out, "[service]\ntheme_preset_x = 1\ntheme_preset = 2\n");
    }

    #[test]
    fn the_theme_is_validated_then_written_and_read_back() {
        let tmp = page_space("theme");
        std::fs::create_dir_all(tmp.join("Assets")).unwrap();
        std::fs::write(tmp.join("Assets").join("logo.png"), PNG_BYTES).unwrap();
        let c = ctx(&tmp);

        // One bad value stops the whole call: nothing is written.
        let before = std::fs::read_to_string(tmp.join(SERVICE_FOLDER).join("_service.toml")).unwrap();
        let bad = WebsiteThemeSetTool.execute(
            json!({ "preset": "arcade", "colors": { "primary": "blue" }, "slideshow_interval_s": 99, "logo": "Assets/missing.png" }),
            &c,
        );
        assert!(!bad.success);
        for needle in ["primary", "slideshow_interval_s", "logo"] {
            assert!(bad.content.contains(needle), "{}", bad.content);
        }
        assert_eq!(std::fs::read_to_string(tmp.join(SERVICE_FOLDER).join("_service.toml")).unwrap(), before);

        let good = WebsiteThemeSetTool.execute(
            json!({ "preset": "Arcade", "colors": { "primary": "#ff00aa" }, "font": "jetbrains-mono", "slideshow_interval_s": 12, "logo": "Assets/logo.png" }),
            &c,
        );
        assert!(good.success, "{}", good.content);
        let got = WebsiteThemeGetTool.execute(json!({}), &c);
        assert!(got.success, "{}", got.content);
        let data = got.structured_data.unwrap();
        assert_eq!(data["preset"], "arcade");
        assert_eq!(data["colors"]["primary"], "#FF00AA");
        assert_eq!(data["slideshow_interval_s"], 12);
        assert_eq!(data["logo"], "Assets/logo.png");
        assert_eq!(data["page_spec"]["theme"]["font"], "jetbrains-mono");
        assert_eq!(data["presets"].as_array().unwrap().len(), 6);

        // The file kept its comment, its other keys and its line endings.
        let after = std::fs::read_to_string(tmp.join(SERVICE_FOLDER).join("_service.toml")).unwrap();
        assert!(after.starts_with("# Website\r\n") && after.contains("namespace = \"x\"") && after.contains("[metadata]"), "{after}");
        assert!(!after.contains("\r\n\n") && !after.replace("\r\n", "").contains('\n'), "line endings stayed CRLF");
    }

    #[test]
    fn a_theme_image_that_is_not_an_image_is_refused() {
        let tmp = page_space("badimg");
        std::fs::write(tmp.join("notes.png"), b"just text").unwrap();
        let r = WebsiteThemeSetTool.execute(json!({ "banner": "notes.png" }), &ctx(&tmp));
        assert!(!r.success && r.content.contains("PNG, JPEG or WebP"), "{}", r.content);
        let r = WebsiteThemeSetTool.execute(json!({ "banner": "../outside.png" }), &ctx(&tmp));
        assert!(!r.success, "a path out of the Space is refused");
    }

    #[test]
    fn links_are_validated_capped_listed_and_removed() {
        let tmp = page_space("links");
        let c = ctx(&tmp);
        let add = |key: &str, kind: &str, url: &str, label: &str| {
            WebsiteLinkAddTool.execute(json!({ "key": key, "kind": kind, "url": url, "label": label }), &c)
        };

        let r = add("lab", "website", "https://example-robotics.com", "Our lab");
        assert!(r.success, "{}", r.content);
        for (key, kind, url, label) in [
            ("a", "social", "http://youtube.com/@x", "YouTube"),
            ("b", "social", "https://127.0.0.1/", "Local"),
            ("c", "blog", "https://example-robotics.com", "Blog"),
            ("d", "social", "https://youtube.com/@x", "https://youtube.com/@x"),
            ("../e", "social", "https://youtube.com/@x", "YouTube"),
        ] {
            assert!(!add(key, kind, url, label).success, "{key} {kind} {url} must be refused");
        }
        // One website only; replacing it by its own key is fine.
        assert!(!add("lab2", "website", "https://two.com", "Two").success);
        assert!(add("lab", "website", "https://example-robotics.com/new", "Our lab").success);

        let file = std::fs::read_to_string(tmp.join(SERVICE_FOLDER).join("lab").join("_instance.toml")).unwrap();
        assert!(file.contains("class_name = \"PageLink\"") && file.contains("https://example-robotics.com/new"), "{file}");

        let list = WebsiteLinksListTool.execute(json!({}), &c);
        assert!(list.success && list.content.contains("website 1/1"), "{}", list.content);
        assert_eq!(list.structured_data.unwrap()["links"].as_array().unwrap().len(), 1);

        // A Reference is not a link: it is neither overwritten nor removed.
        std::fs::create_dir_all(tmp.join(SERVICE_FOLDER).join("ref1")).unwrap();
        std::fs::write(
            tmp.join(SERVICE_FOLDER).join("ref1").join("_instance.toml"),
            "[metadata]\nclass_name = \"Reference\"\n\n[attributes]\nkind = \"count\"\n",
        )
        .unwrap();
        assert!(!add("ref1", "social", "https://youtube.com/@x", "YouTube").success);
        assert!(!WebsiteLinkRemoveTool.execute(json!({ "key": "ref1" }), &c).success);
        assert!(tmp.join(SERVICE_FOLDER).join("ref1").join("_instance.toml").is_file());

        assert!(WebsiteLinkRemoveTool.execute(json!({ "key": "lab" }), &c).success);
        assert!(!tmp.join(SERVICE_FOLDER).join("lab").exists());
        assert!(!WebsiteLinkRemoveTool.execute(json!({ "key": "../lab" }), &c).success);
    }

    #[test]
    fn website_setup_keeps_the_page_settings() {
        let tmp = page_space("setup");
        let c = ctx(&tmp);
        assert!(WebsiteThemeSetTool.execute(json!({ "preset": "paper", "motion": "none" }), &c).success);
        assert!(WebsiteSetupTool.execute(json!({ "namespace": "renamed" }), &c).success);
        let got = WebsiteThemeGetTool.execute(json!({}), &c).structured_data.unwrap();
        assert_eq!(got["preset"], "paper", "a namespace edit must not drop the theme");
        assert_eq!(got["motion"], "none");
    }

    #[test]
    fn website_status_reports_the_readme_as_a_publish_would_bake_it() {
        let tmp = page_space("readme");
        let c = ctx(&tmp);
        let none = WebsiteStatusTool.execute(json!({}), &c).structured_data.unwrap();
        assert_eq!(none["readme"]["exists"], false);

        std::fs::create_dir_all(tmp.join("Assets")).unwrap();
        std::fs::write(tmp.join("Assets").join("cell.png"), PNG_BYTES).unwrap();
        std::fs::write(
            tmp.join(SERVICE_FOLDER).join("README.md"),
            "# Lab\n\n![Cell](../Assets/cell.png) ![Gone](gone.png) ![Pixel](https://evil.example/p.png)\n",
        )
        .unwrap();
        let r = WebsiteStatusTool.execute(json!({}), &c);
        assert!(r.content.contains("README.md:"), "{}", r.content);
        let got = r.structured_data.unwrap();
        assert_eq!(got["readme"]["uploads"], true);
        assert_eq!(got["readme"]["images"], json!([{ "slot": "readme-0", "path": "Assets/cell.png" }]));
        assert_eq!(got["readme"]["warnings"].as_array().unwrap().len(), 2, "{}", got["readme"]);
    }
}
