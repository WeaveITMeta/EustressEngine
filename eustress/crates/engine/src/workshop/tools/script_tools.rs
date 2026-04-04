//! Script execution tools — write Rune or Luau scripts to SoulService for hot-reload,
//! and image-to-code generation via Claude Vision API.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

pub struct ExecuteRuneTool;

impl ToolHandler for ExecuteRuneTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "execute_rune",
            description: "Write and execute a Rune script in SoulService. The engine hot-reloads .rune files. Use for spawning entities, physics, animations, game logic.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Rune script source code" },
                    "name": { "type": "string", "description": "Script name (without .rune)", "default": "workshop_script" }
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

        let soul_dir = ctx.space_root.join("SoulService");
        let _ = std::fs::create_dir_all(&soul_dir);
        let script_path = soul_dir.join(format!("{}.rune", script_name));

        match std::fs::write(&script_path, code) {
            Ok(_) => ToolResult {
                tool_name: "execute_rune".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Wrote Rune script '{}' ({} bytes) — engine will hot-reload.", script_name, code.len()),
                structured_data: Some(serde_json::json!({ "name": script_name, "path": script_path.to_string_lossy() })),
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
            description: "Write and execute a Luau script in SoulService. The engine hot-reloads .luau and .lua files. Luau provides Roblox API compatibility (Instance.new, RunService, Players, TweenService, DataStoreService, CollectionService, HttpService, MarketplaceService).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Luau script source code" },
                    "name": { "type": "string", "description": "Script name (without .lua extension)", "default": "workshop_script" }
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
        let soul_dir = ctx.space_root.join("SoulService");
        let _ = std::fs::create_dir_all(&soul_dir);
        let script_path = soul_dir.join(format!("{}.luau", script_name));

        match std::fs::write(&script_path, code) {
            Ok(_) => ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Wrote Luau script '{}' ({} bytes) — engine will hot-reload.", script_name, code.len()),
                structured_data: Some(serde_json::json!({ "name": script_name, "path": script_path.to_string_lossy() })),
                stream_topic: Some("workshop.tool.execute_luau".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "execute_luau".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed: {}", e), structured_data: None, stream_topic: None,
            },
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
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") {
                    let name = fname.trim_end_matches(".part.toml").trim_end_matches(".glb.toml");
                    doc.push_str(&format!("- {}\n", name));
                    entity_count += 1;
                }
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
