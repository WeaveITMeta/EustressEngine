//! Script execution tools — write Rune or Luau scripts to SoulService for hot-reload,
//! and image-to-code generation via Claude Vision API.
//!
//! ## SoulService folder convention
//!
//! Scripts live as folders, not flat files, so the `_instance.toml`
//! on the folder turns the script into a proper Explorer entity:
//!
//! ```text
//! SoulService/
//!   cycle_life_test/                ← folder == display name == class instance
//!     _instance.toml                ← `class_name = "SoulScript"`, `language = "rune"`
//!     cycle_life_test.rune          ← canonical source (<folder>/<folder>.rune)
//!     cycle_life_test.md            ← canonical summary (<folder>/<folder>.md)
//! ```
//!
//! Both `execute_rune` and `execute_luau` now write this layout. The
//! file watcher + Script-Editor center tab expect exactly these
//! filenames (see `engine::ui::center_tabs::script_source_path_canonical`
//! / `script_summary_path_canonical`) — deviating broke hot-reload and
//! Explorer icons before this change.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

/// Stable slugification: keep ASCII alphanumerics + underscore, collapse
/// everything else to `_`. Used when the caller passes a name with
/// spaces / punctuation so the folder name stays filesystem-safe
/// without requiring a dependency on the `slug` crate in the
/// MCP-server build.
fn slug_safe(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Build the `_instance.toml` + `<name>.md` + `<name>.{rune|luau}`
/// folder under `SoulService/<name>/` for a hot-reload-ready script.
/// Returns `(folder_path, source_path)` on success.
///
/// * `extension` — `"rune"` or `"luau"`, decides the source filename
///   and the `language` field in `_instance.toml`.
/// * `summary_seed` — optional one-paragraph summary to seed the
///   `<name>.md` file. Empty strings produce a minimal stub so the
///   Script-Editor Summary tab opens cleanly.
fn write_soul_script_folder(
    soul_root: &std::path::Path,
    raw_name: &str,
    code: &str,
    extension: &str,
    summary_seed: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), std::io::Error> {
    let name = {
        let s = slug_safe(raw_name);
        if s.is_empty() { "workshop_script".to_string() } else { s }
    };
    let folder = soul_root.join(&name);
    std::fs::create_dir_all(&folder)?;

    // `_instance.toml` — marks the folder as a SoulScript entity so
    // the Explorer renders it with the script icon, the Soul panel
    // picks it up, and the file watcher hot-reloads edits.
    let language = match extension { "luau" | "lua" => "luau", _ => "rune" };
    let class_name = "SoulScript";
    let instance_toml = format!(
        "[metadata]\nclass_name = \"{class}\"\nname = \"{name}\"\narchivable = true\n\n[properties]\nlanguage = \"{language}\"\n",
        class = class_name,
        name = name,
        language = language,
    );
    std::fs::write(folder.join("_instance.toml"), instance_toml)?;

    // Canonical source + summary paths — match `center_tabs::
    // script_source_path_canonical` + `script_summary_path_canonical`
    // byte-for-byte. Drifting from that helper breaks hot-reload +
    // the Summary tab.
    let source_path = folder.join(format!("{}.{}", name, extension));
    std::fs::write(&source_path, code)?;

    // Summary file — created empty on purpose when the caller didn't
    // provide a `summary_seed`. The Script-Editor "Build" pipeline
    // reads this `.md` as the soul-spec; a pre-filled stub biases
    // the first-run build toward boilerplate. Leaving it at 0 bytes
    // lets the build step populate it fresh from the `.rune` source
    // (or lets the user author it before the first build).
    let summary_path = folder.join(format!("{}.md", name));
    if !summary_path.exists() {
        let body = if summary_seed.trim().is_empty() {
            String::new()
        } else {
            format!("# {}\n\n{}\n", name, summary_seed.trim())
        };
        std::fs::write(&summary_path, body)?;
    }

    Ok((folder, source_path))
}

pub struct ExecuteRuneTool;

impl ToolHandler for ExecuteRuneTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "execute_rune",
            description: "Write and execute a Rune script in SoulService. Creates a folder `SoulService/<name>/` containing `_instance.toml` (class SoulScript), `<name>.rune` (source), and `<name>.md` (summary) — matching the folder-based script convention the engine uses for entity detection + Explorer rendering. The engine hot-reloads source edits. **Feedback loop:** after writing, call `query_stream_events` with topic `rune.compile.error` to check whether the script compiled cleanly. Each compile error is published as one event with `{ script, line }` — fix any reported diagnostics and call `execute_rune` again until the topic returns no new events. Diagnostic lines follow `<name>:<line>:<col>: <severity>: <message>` format.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "code":    { "type": "string", "description": "Rune script source code" },
                    "name":    { "type": "string", "description": "Script folder + base filename (without .rune). Folder-safe; spaces / punctuation get normalized to underscores.", "default": "workshop_script" },
                    "summary": { "type": "string", "description": "Optional one-paragraph description written into `<name>.md`. Leave empty to get a stub.", "default": "" }
                },
                "required": ["code"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.execute_rune"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let code = match input.get("code").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult {
                tool_name: "execute_rune".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing: code".into(), structured_data: None, stream_topic: None,
            },
        };
        let script_name = input.get("name").and_then(|v| v.as_str()).unwrap_or("workshop_script");
        let summary = input.get("summary").and_then(|v| v.as_str()).unwrap_or("");

        let soul_dir = ctx.space_root.join("SoulService");
        let _ = std::fs::create_dir_all(&soul_dir);

        match write_soul_script_folder(&soul_dir, script_name, code, "rune", summary) {
            Ok((folder, source)) => ToolResult {
                tool_name: "execute_rune".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Wrote Rune script folder '{}' ({} bytes) — engine will hot-reload.",
                    folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    code.len(),
                ),
                structured_data: Some(serde_json::json!({
                    "name": script_name,
                    "folder": folder.to_string_lossy(),
                    "source": source.to_string_lossy(),
                })),
                stream_topic: Some("workshop.tool.execute_rune".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "execute_rune".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed: {}", e), structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Execute Luau Script
// ---------------------------------------------------------------------------

pub struct ExecuteLuauTool;

impl ToolHandler for ExecuteLuauTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "execute_luau",
            description: "Write and execute a Luau script in SoulService. Creates a folder `SoulService/<name>/` containing `_instance.toml` (class SoulScript, language luau), `<name>.luau` (source), and `<name>.md` (summary) — same folder-per-script convention as `execute_rune`. Luau provides Roblox API compatibility (Instance.new, RunService, Players, TweenService, DataStoreService, CollectionService, HttpService, MarketplaceService). The engine hot-reloads `.luau` and `.lua` files.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "code":    { "type": "string", "description": "Luau script source code" },
                    "name":    { "type": "string", "description": "Script folder + base filename (without .luau). Folder-safe; spaces / punctuation normalized to underscores.", "default": "workshop_script" },
                    "summary": { "type": "string", "description": "Optional one-paragraph description written into `<name>.md`.", "default": "" }
                },
                "required": ["code"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.execute_luau"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let code = match input.get("code").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing required parameter: code".into(), structured_data: None, stream_topic: None,
            },
        };
        let script_name = input.get("name").and_then(|v| v.as_str()).unwrap_or("workshop_script");
        let summary = input.get("summary").and_then(|v| v.as_str()).unwrap_or("");

        // Always write the script folder for persistence and hot-reload
        let soul_dir = ctx.space_root.join("SoulService");
        let _ = std::fs::create_dir_all(&soul_dir);

        let write_result = write_soul_script_folder(&soul_dir, script_name, code, "luau", summary);
        let (folder_name, _source_path) = match &write_result {
            Ok((folder, source)) => (
                folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                Some(source.clone()),
            ),
            Err(e) => return ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to write script: {}", e), structured_data: None, stream_topic: None,
            },
        };

        // If the engine provided a Luau executor, run the script inline
        // and materialize any Instance.new("Part") calls as entity folders.
        if let Some(ref executor) = ctx.luau_executor {
            let result = executor(code, script_name);

            // Materialize created instances as entity _instance.toml files.
            // Written into Workspace/.generated/ so open_space can wipe them
            // on Space switch — keeps generated entities Space-local and transient.
            let workspace_dir = ctx.space_root.join("Workspace");
            let generated_dir = workspace_dir.join(".generated");
            let _ = std::fs::create_dir_all(&generated_dir);
            let mut spawned = 0;

            for entity in &result.created_entities {
                let safe_name = entity.name.replace(' ', "_").replace('/', "_");
                let entity_dir = generated_dir.join(&safe_name);
                let _ = std::fs::create_dir_all(&entity_dir);
                let instance_path = entity_dir.join("_instance.toml");

                // Select mesh based on Part shape — fixes all-Block default
                let mesh_path = if entity.class_name == "Part" {
                    match entity.shape.as_str() {
                        "Ball"        => "assets/parts/ball.glb",
                        "Cylinder"    => "assets/parts/cylinder.glb",
                        "Wedge"       => "assets/parts/wedge.glb",
                        "CornerWedge" => "assets/parts/corner_wedge.glb",
                        "Cone"        => "assets/parts/cone.glb",
                        _             => "assets/parts/block.glb",
                    }
                } else { "" };

                let asset_section = if !mesh_path.is_empty() {
                    format!("[asset]\nmesh = \"{}\"\nscene = \"Scene0\"\n\n", mesh_path)
                } else {
                    String::new()
                };

                let toml_content = format!(
r#"[metadata]
class_name = "{class}"
name = "{name}"
archivable = true

{asset}[transform]
position = [{px}, {py}, {pz}]
rotation = [{rx}, {ry}, {rz}, {rw}]
scale = [{sx}, {sy}, {sz}]

[properties]
material = "{material}"
shape = "{shape}"
color = [{cr}, {cg}, {cb}, {ca}]
transparency = {transparency}
anchored = {anchored}
can_collide = {can_collide}
cast_shadow = true
reflectance = 0.0
locked = false
"#,
                    class = entity.class_name,
                    name = entity.name,
                    asset = asset_section,
                    shape = entity.shape,
                    px = entity.position[0], py = entity.position[1], pz = entity.position[2],
                    rx = entity.rotation[0], ry = entity.rotation[1], rz = entity.rotation[2], rw = entity.rotation[3],
                    sx = entity.size[0], sy = entity.size[1], sz = entity.size[2],
                    cr = entity.color[0], cg = entity.color[1], cb = entity.color[2], ca = entity.color[3],
                    material = entity.material,
                    transparency = entity.transparency,
                    anchored = entity.anchored,
                    can_collide = entity.can_collide,
                );

                if std::fs::write(&instance_path, &toml_content).is_ok() {
                    spawned += 1;
                }
            }

            let content = if result.success {
                format!(
                    "Luau script '{}' executed — {} entities created, {} written to Workspace/.generated/.",
                    folder_name, result.created_entities.len(), spawned,
                )
            } else {
                format!(
                    "Luau script '{}' written but execution failed: {}",
                    folder_name, result.message,
                )
            };

            ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: result.success,
                content,
                structured_data: Some(serde_json::json!({
                    "name": script_name,
                    "executed": result.success,
                    "entities_created": result.created_entities.len(),
                    "entities_written": spawned,
                    "message": result.message,
                })),
                stream_topic: Some("workshop.tool.execute_luau".to_string()),
            }
        } else {
            // No Luau executor available — write-only mode (MCP server)
            ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Wrote Luau script folder '{}' ({} bytes) — engine will hot-reload on Play.",
                    folder_name, code.len(),
                ),
                structured_data: Some(serde_json::json!({
                    "name": script_name,
                    "executed": false,
                })),
                stream_topic: Some("workshop.tool.execute_luau".to_string()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Image to Code (Vision API)
// ---------------------------------------------------------------------------

pub struct ImageToCodeTool;

impl ToolHandler for ImageToCodeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "image_to_code",
            description: "Convert a screenshot or design image into Rune script code using Claude Vision API. Reads an image file from the Universe folder, sends it to Claude with a prompt describing what code to generate, and returns the generated Rune script. Supported image formats: PNG, JPG, WEBP, GIF.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "description": "Path to the image file (relative to Universe root)" },
                    "prompt": { "type": "string", "description": "What code to generate from the image (e.g. 'recreate this UI layout as a ScreenGui' or 'generate a Rune script that builds this 3D scene')" },
                    "output_language": { "type": "string", "description": "Output language: rune or luau", "default": "rune" }
                },
                "required": ["image_path", "prompt"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: true,
            stream_topics: &["workshop.tool.image_to_code"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let image_path = input.get("image_path").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let output_lang = input.get("output_language").and_then(|v| v.as_str()).unwrap_or("rune");

        if image_path.is_empty() || prompt.is_empty() {
            return ToolResult {
                tool_name: "image_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: "Both image_path and prompt are required".into(),
                structured_data: None, stream_topic: None,
            };
        }

        // Validate path is inside Universe
        if image_path.contains("..") {
            return ToolResult {
                tool_name: "image_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: "Path traversal not allowed".into(),
                structured_data: None, stream_topic: None,
            };
        }

        let full_path = ctx.universe_root.join(image_path);
        if !full_path.exists() {
            return ToolResult {
                tool_name: "image_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Image not found: {}", image_path),
                structured_data: None, stream_topic: None,
            };
        }

        // Read and base64-encode the image
        let image_bytes = match std::fs::read(&full_path) {
            Ok(b) => b,
            Err(e) => return ToolResult {
                tool_name: "image_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to read image: {}", e),
                structured_data: None, stream_topic: None,
            },
        };

        let base64_image = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_bytes);
        let media_type = match full_path.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            _ => "image/png",
        };

        // Return the image data for the agentic loop to send to Claude Vision
        ToolResult {
            tool_name: "image_to_code".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Image loaded ({} bytes). Sending to Claude Vision API with prompt: '{}'", image_bytes.len(), prompt),
            structured_data: Some(serde_json::json!({
                "action": "image_to_code",
                "image_base64": base64_image,
                "media_type": media_type,
                "prompt": prompt,
                "output_language": output_lang,
                "image_path": image_path,
            })),
            stream_topic: Some("workshop.tool.image_to_code".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Image to Geometry  (VIGA — Vision-as-Inverse-Graphics)
// ---------------------------------------------------------------------------
//
// Separate from `image_to_code`. Where `image_to_code` emits a Rune
// *script* that represents an image (UI, 2D layout, instructions),
// `image_to_geometry` emits *3D scene geometry* — entities, parts,
// meshes, transforms — reconstructing the contents of a reference
// image as a working scene the user can walk inside.
//
// Invokes the VIGA pipeline (see `eustress_engine::viga`) which runs
// an iterative generate-render-verify loop: the LLM proposes entity
// additions, the engine renders the candidate scene, a verifier pass
// compares the render to the reference, and the loop refines.

pub struct ImageToGeometryTool;

impl ToolHandler for ImageToGeometryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "image_to_geometry",
            description: "Reconstruct a reference photo, screenshot, or concept art as 3D scene geometry. Invokes VIGA — the Vision-as-Inverse-Graphics Agent — an iterative generate-render-verify loop that adds parts / meshes / lighting / materials to the current Space until the rendered result matches the reference image. Returns the spawned entity tree + iteration stats. Use for: recreating a photograph as a buildable Eustress Space, turning concept art into a playable scene, prototyping from visual references. DIFFERENT from `image_to_code` (which returns a Rune script, not 3D entities). Supported image formats: PNG, JPG, WEBP, GIF.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "image_path": { "type": "string", "description": "Path to the reference image file (relative to Universe root)" },
                    "prompt":     { "type": "string", "description": "Optional additional guidance (e.g. 'focus on the architectural structure, ignore the sky'). Empty → VIGA uses default scene-reconstruction instructions." },
                    "max_iterations": { "type": "integer", "description": "Maximum refinement iterations before returning best result (default 5, max 20)", "default": 5 },
                    "target_space":   { "type": "string", "description": "Space name to spawn geometry into (default: currently-active Space)" }
                },
                "required": ["image_path"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,   // spawns world geometry — user must confirm
            stream_topics: &["workshop.tool.image_to_geometry"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let image_path = input.get("image_path").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let max_iterations = input.get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20);
        let target_space = input.get("target_space").and_then(|v| v.as_str()).unwrap_or("");

        if image_path.is_empty() {
            return ToolResult {
                tool_name: "image_to_geometry".to_string(), tool_use_id: String::new(),
                success: false, content: "image_path is required".into(),
                structured_data: None, stream_topic: None,
            };
        }
        if image_path.contains("..") {
            return ToolResult {
                tool_name: "image_to_geometry".to_string(), tool_use_id: String::new(),
                success: false, content: "Path traversal not allowed".into(),
                structured_data: None, stream_topic: None,
            };
        }

        let full_path = ctx.universe_root.join(image_path);
        if !full_path.exists() {
            return ToolResult {
                tool_name: "image_to_geometry".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Reference image not found: {}", image_path),
                structured_data: None, stream_topic: None,
            };
        }

        // Read + base64-encode the reference image.
        let image_bytes = match std::fs::read(&full_path) {
            Ok(b) => b,
            Err(e) => return ToolResult {
                tool_name: "image_to_geometry".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to read image: {}", e),
                structured_data: None, stream_topic: None,
            },
        };
        let base64_image = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_bytes);
        let media_type = match full_path.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            _ => "image/png",
        };

        // Signal the engine's VIGA pipeline. The `action` key tells the
        // Workshop consumer to invoke `VigaPipeline::start_request`
        // (see `eustress_engine::viga::pipeline`) rather than the
        // code-generator flow used by `image_to_code`.
        ToolResult {
            tool_name: "image_to_geometry".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Queued VIGA scene reconstruction from '{}' ({} bytes, up to {} iterations).",
                image_path, image_bytes.len(), max_iterations,
            ),
            structured_data: Some(serde_json::json!({
                "action": "image_to_geometry",
                "image_base64": base64_image,
                "media_type": media_type,
                "prompt": prompt,
                "max_iterations": max_iterations,
                "target_space": target_space,
                "image_path": image_path,
            })),
            stream_topic: Some("workshop.tool.image_to_geometry".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Document to Code
// ---------------------------------------------------------------------------

pub struct DocumentToCodeTool;

impl ToolHandler for DocumentToCodeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "document_to_code",
            description: "Convert a document (Markdown, plain text, or structured specification) into executable Rune or Luau code. Reads a document file from the Universe folder and generates code that implements the described behavior, UI layout, or simulation logic. Useful for turning design documents, requirements, or pseudocode into working scripts.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_path": { "type": "string", "description": "Path to the document file (relative to Universe root). Supports .md, .txt, .toml, .json" },
                    "prompt": { "type": "string", "description": "Instructions for what code to generate from the document (e.g. 'implement the UI layout described in this spec' or 'generate simulation scripts from these requirements')" },
                    "output_language": { "type": "string", "description": "Output language: rune or luau", "default": "rune" }
                },
                "required": ["document_path", "prompt"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: true,
            stream_topics: &["workshop.tool.document_to_code"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let doc_path = input.get("document_path").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let output_lang = input.get("output_language").and_then(|v| v.as_str()).unwrap_or("rune");

        if doc_path.is_empty() || prompt.is_empty() {
            return ToolResult {
                tool_name: "document_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: "Both document_path and prompt are required".into(),
                structured_data: None, stream_topic: None,
            };
        }

        if doc_path.contains("..") {
            return ToolResult {
                tool_name: "document_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: "Path traversal not allowed".into(),
                structured_data: None, stream_topic: None,
            };
        }

        let full_path = ctx.universe_root.join(doc_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => return ToolResult {
                tool_name: "document_to_code".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to read document: {}", e),
                structured_data: None, stream_topic: None,
            },
        };

        // Truncate large documents to fit Claude context
        let truncated = if content.len() > 15000 {
            format!("{}...\n[document truncated — {} bytes total]", &content[..15000], content.len())
        } else {
            content
        };

        ToolResult {
            tool_name: "document_to_code".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Document loaded ({} bytes). Ready for code generation in {}.", truncated.len(), output_lang),
            structured_data: Some(serde_json::json!({
                "action": "document_to_code",
                "document_content": truncated,
                "prompt": prompt,
                "output_language": output_lang,
                "document_path": doc_path,
            })),
            stream_topic: Some("workshop.tool.document_to_code".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Generate Documentation
// ---------------------------------------------------------------------------

pub struct GenerateDocsTool;

impl ToolHandler for GenerateDocsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "generate_docs",
            description: "Auto-generate a README.md documenting the current Space. Scans all services, entities, scripts, and materials to produce a structured overview with entity counts, script descriptions, material list, and file structure.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "output_path": { "type": "string", "description": "Output path relative to Space root", "default": "README.md" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.generate_docs"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let output_path = input.get("output_path").and_then(|v| v.as_str()).unwrap_or("README.md");

        let mut doc = String::with_capacity(4096);
        let space_name = ctx.space_root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Space");

        doc.push_str(&format!("# {}\n\n", space_name));
        doc.push_str("Auto-generated documentation for this Eustress Space.\n\n");

        // Scan services
        doc.push_str("## Services\n\n");
        if let Ok(entries) = std::fs::read_dir(&ctx.space_root) {
            let mut services: Vec<(String, usize)> = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.starts_with('.') || name == "meshes" { continue; }
                    let file_count = std::fs::read_dir(&path)
                        .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count())
                        .unwrap_or(0);
                    services.push((name.to_string(), file_count));
                }
            }
            services.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, count) in &services {
                doc.push_str(&format!("- **{}** — {} files\n", name, count));
            }
        }

        // Scan workspace entities
        doc.push_str("\n## Workspace Entities\n\n");
        let workspace = ctx.space_root.join("Workspace");
        let mut entity_count = 0;
        if let Ok(entries) = std::fs::read_dir(&workspace) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = entry.file_name().to_string_lossy().to_string();
                // Folder-based parts: folder/_instance.toml; legacy: .part.toml/.glb.toml
                let name = if path.is_dir() && path.join("_instance.toml").exists() {
                    fname.clone()
                } else if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") {
                    fname.trim_end_matches(".part.toml").trim_end_matches(".glb.toml").to_string()
                } else {
                    continue;
                };
                doc.push_str(&format!("- {}\n", name));
                entity_count += 1;
            }
        }
        if entity_count == 0 {
            doc.push_str("No entities in Workspace.\n");
        }

        // Scan scripts
        doc.push_str("\n## Scripts\n\n");
        let soul_dir = ctx.space_root.join("SoulService");
        let mut script_count = 0;
        if let Ok(entries) = std::fs::read_dir(&soul_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".rune") || fname.ends_with(".lua") || fname.ends_with(".luau") || fname.ends_with(".soul") {
                    doc.push_str(&format!("- `{}`\n", fname));
                    script_count += 1;
                }
            }
        }
        if script_count == 0 {
            doc.push_str("No scripts in SoulService.\n");
        }

        // Write the file
        let full_path = ctx.space_root.join(output_path);
        match std::fs::write(&full_path, &doc) {
            Ok(_) => ToolResult {
                tool_name: "generate_docs".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Generated documentation: {} ({} entities, {} scripts)", output_path, entity_count, script_count),
                structured_data: Some(serde_json::json!({ "path": full_path.to_string_lossy(), "entities": entity_count, "scripts": script_count })),
                stream_topic: Some("workshop.tool.generate_docs".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "generate_docs".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to write docs: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}
