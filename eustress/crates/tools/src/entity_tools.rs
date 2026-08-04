//! Entity management tools — create and query entities in the Space.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Create Entity
// ---------------------------------------------------------------------------

pub struct CreateEntityTool;

/// Map a primitive shape id (or class name) to its shared-mesh asset
/// path. Must stay in sync with `toolbox::get_mesh_catalog` on the
/// engine side — the engine's file-watcher resolves this path via
/// `PRIMITIVE_MESHES` + `material_loader::resolve_material`.
///
/// Without an `[asset]` section the file watcher's `spawn_instance`
/// hits its `asset.is_none()` branch and attaches only a bare
/// `Instance + Transform + Visibility` (no `BasePart`, `Part`,
/// `Mesh3d`, `MeshMaterial3d`, `Collider`) — which is why the
/// previous version of this tool produced entities that were
/// invisible and unselectable in the viewport.
fn primitive_mesh_path(shape: &str) -> Option<&'static str> {
    match shape.to_ascii_lowercase().as_str() {
        "block" | "part" | "cube"                 => Some("parts/block.glb"),
        "ball" | "sphere"                         => Some("parts/ball.glb"),
        "cylinder"                                => Some("parts/cylinder.glb"),
        "wedge"                                   => Some("parts/wedge.glb"),
        "corner_wedge" | "corner" | "cornerwedge" => Some("parts/corner_wedge.glb"),
        "cone"                                    => Some("parts/cone.glb"),
        _                                         => None,
    }
}

/// Recursively search `workspace` for a folder named `safe_name` that
/// contains `_instance.toml` — i.e. a live entity, at any nesting depth
/// under Model folders. `create_entity`'s `parent` argument lets callers
/// nest entities arbitrarily deep (`Workspace/Plant/Stage2Reactor/…`),
/// but `update_entity`/`delete_entity` used to only ever check
/// `Workspace/<name>/` directly — any entity created with a `parent` was
/// silently unreachable by name afterward, so a mistake in ONE
/// `create_entity` call (wrong shape, wrong color) had no in-place fix;
/// the only recovery was deleting and rebuilding the whole subtree.
///
/// Depth-first, returns the first match — a exact-depth match at the
/// current level always wins over a deeper one, so the common case
/// (entity directly under `Workspace/`) doesn't pay for a full walk.
/// Skips hidden directories (`.eustress`, `.git`, …) so trash/undo
/// scaffolding never shadows a real entity of the same name.
fn find_entity_folder(workspace: &std::path::Path, safe_name: &str) -> Option<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, safe_name: &str) -> Option<std::path::PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let is_hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(true);
            if is_hidden {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some(safe_name)
                && path.join("_instance.toml").exists()
            {
                return Some(path);
            }
            subdirs.push(path);
        }
        for sub in subdirs {
            if let Some(found) = walk(&sub, safe_name) {
                return Some(found);
            }
        }
        None
    }
    walk(workspace, safe_name)
}

impl ToolHandler for CreateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_entity",
            description: "Create a new 3D entity in the Space's Workspace. Writes a folder with _instance.toml so the engine's file watcher hot-spawns it. Use `parent` to place entities inside a Model folder (e.g. parent=\"V-Cell/V2\" creates at Workspace/V-Cell/V2/{name}/). Without parent, entities land at the Workspace root. ALWAYS create the parent Model first, then create child Parts with that parent path.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class": {
                        "type": "string",
                        "description": "Entity class. `Part` (default) = visible 3D primitive. `Model` = folder container (no mesh). Any other string is passed through for service/script classes.",
                        "default": "Part"
                    },
                    "shape": {
                        "type": "string",
                        "description": "Primitive shape for Part class: block (default), ball, cylinder, wedge, corner_wedge, cone. Determines which shared mesh asset is referenced in the `[asset]` section.",
                        "default": "block"
                    },
                    "name":     { "type": "string",  "description": "Entity name (used as folder + Instance.name)" },
                    "position": { "type": "array",   "items": { "type": "number" }, "description": "[x, y, z] world position in meters (1 unit = 1 meter)" },
                    "size":     { "type": "array",   "items": { "type": "number" }, "description": "[x, y, z] size in meters (maps to Transform.scale)" },
                    "material": { "type": "string",  "description": "Material preset: Plastic, SmoothPlastic, Wood, WoodPlanks, Metal, CorrodedMetal, DiamondPlate, Foil, Grass, Concrete, Brick, Granite, Marble, Slate, Sand, Fabric, Glass, Neon, Ice" },
                    "color":    { "type": "array",   "items": { "type": "number" }, "description": "[r, g, b] color — either 0.0-1.0 floats or 0-255 integers" },
                    "parent": {
                        "type": "string",
                        "description": "Path relative to Workspace/ where this entity should be created. Use this to place Parts inside a Model folder. Example: 'MyProduct/V2' creates at Workspace/MyProduct/V2/{name}/. Omit or empty to place directly in Workspace/."
                    },
                    "anchored":     { "type": "boolean", "description": "Prevents physics from moving the part (default true)" },
                    "can_collide":  { "type": "boolean", "description": "Whether the part participates in collisions (default true)" },
                    "unit": {
                        "type": "string",
                        "description": "Authoring unit for `position` and `size`. Accepts canonical symbols (m, cm, mm, ft, in, studs) and lenient aliases (meters, feet, ...). When provided, position/size are interpreted in this unit and converted to engine-native meters before writing. Stamped into `metadata.unit` so the round-trip is symmetric. Defaults to the Space-default unit, or meters if none."
                    }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.create_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let class_raw = input.get("class").and_then(|v| v.as_str()).unwrap_or("Part");
        let shape = input.get("shape").and_then(|v| v.as_str()).unwrap_or("block");
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("NewPart");
        let position = parse_vec3(&input, "position", [0.0, 0.0, 0.0]);
        let size = parse_vec3(&input, "size", [1.0, 1.0, 1.0]);
        let material = input.get("material").and_then(|v| v.as_str()).map(str::to_string);
        let color = normalize_color(parse_vec3(&input, "color", [0.639, 0.635, 0.647]));
        let anchored = input.get("anchored").and_then(|v| v.as_bool());
        let can_collide = input.get("can_collide").and_then(|v| v.as_bool());

        // If `class` is actually a primitive shape name ("Ball",
        // "Cylinder", …) the caller probably meant `class=Part,
        // shape=ball` — fold it back so older prompts keep working.
        let (class, shape) = if primitive_mesh_path(class_raw).is_some() {
            ("Part", class_raw)
        } else {
            (class_raw, shape)
        };

        let workspace_dir = ctx.space_root.join("Workspace");

        // Optional parent path: place entity inside a Model folder hierarchy.
        // Reject path traversal attempts (no '..' components allowed).
        let parent_path = input.get("parent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let base_dir = if parent_path.is_empty() {
            workspace_dir.clone()
        } else {
            if parent_path.contains("..") {
                return ToolResult {
                    tool_name: "create_entity".to_string(),
                    tool_use_id: String::new(),
                    success: false,
                    content: "Invalid parent path: '..' is not allowed.".to_string(),
                    structured_data: None,
                    stream_topic: None,
                };
            }
            workspace_dir.join(parent_path)
        };

        // Pick the mesh asset. Only `Part` gets one — `Model` / scripts
        // / services skip `[asset]` and render as folder-containers.
        let asset_mesh = if class.eq_ignore_ascii_case("Part") {
            Some(primitive_mesh_path(shape).unwrap_or("parts/block.glb").to_string())
        } else {
            None
        };

        let has_asset = asset_mesh.is_some();

        // Stage 9 — unit handling. Priority order:
        //   1. caller-supplied `unit` arg (most specific)
        //   2. user-selected DisplayUnit from `ctx.display_unit` (what
        //      the user is currently working in — Workshop should
        //      default to this so the AI's "5 ft cube" actually lands
        //      as 5 ft when the user is editing in feet)
        //   3. Space-default unit from `_project/settings.toml`
        //   4. engine-native meters (no unit field stamped)
        // Position and size are reinterpreted FROM the resolved unit
        // BACK to engine-native meters before the file is written —
        // because Stage 4's create-time unit_symbol path stores values
        // verbatim, so caller-supplied "5 ft" must be converted before
        // the file is written.
        let space_unit = eustress_common::project_manifest::read_space_default_unit(&ctx.space_root);
        let unit_arg = input.get("unit").and_then(|v| v.as_str());
        let (unit_symbol, authored) = match unit_arg.and_then(eustress_common::units::Unit::from_any)
            .or_else(|| ctx.display_unit.as_deref().and_then(eustress_common::units::Unit::from_any))
            .or_else(|| space_unit.as_deref().and_then(eustress_common::units::Unit::from_any)) {
            Some(u) => (Some(u.symbol().to_string()), u),
            None => (None, eustress_common::units::ENGINE_NATIVE_UNIT),
        };
        // The caller's numbers are in `authored` units; the file
        // wants them in `authored` units too (so the round-trip is
        // identity at load). No conversion needed for the file value,
        // BUT the engine's runtime spawn will apply the Stage-3 load
        // conversion which assumes "file values are in authored unit"
        // — that's exactly what we're writing, so behaviour lines up.
        let overrides = eustress_common::instance_create::InstanceOverrides {
            display_name: Some(name.to_string()),
            position: Some(position),
            scale: Some(size),
            color_rgba: Some([color[0], color[1], color[2], 1.0]),
            material,
            anchored,
            can_collide,
            asset_mesh,
            asset_path: None,
            rotation: None,
            unit_symbol,
            uuid: None,
            ..Default::default()
        };
        // Silence unused-binding warning when units_v1 is off and the
        // load-time conversion is identity.
        let _ = authored;

        match eustress_common::instance_create::create_instance(
            &base_dir,
            class,
            Some(name),
            overrides,
        ) {
            Ok(created) => ToolResult {
                tool_name: "create_entity".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Created {} '{}' at [{:.1}, {:.1}, {:.1}] (shape={}, asset={})",
                    class, created.folder_name,
                    position[0], position[1], position[2], shape,
                    if has_asset { "attached" } else { "none" }
                ),
                structured_data: Some(serde_json::json!({
                    "class": class,
                    "shape": shape,
                    "name": created.folder_name,
                    "file": created.toml_path.to_string_lossy(),
                    "has_asset": has_asset,
                })),
                stream_topic: Some("workshop.tool.create_entity".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "create_entity".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Failed to create entity: {}", e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Insert Gaussian Splats (3DGS)
// ---------------------------------------------------------------------------

pub struct InsertGaussianSplatsTool;

impl ToolHandler for InsertGaussianSplatsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "insert_gaussian_splats",
            description: "Insert a photoreal Gaussian-splat (3DGS) cloud into the Space from a `.ply` file. Copies the `.ply` into the Universe's `assets/splats/` and writes a `GaussianSplats` instance, so the engine's file-watcher hot-spawns it — rendering the radiance field, culling floaters, and extracting a physics collider automatically. This is the ONLY way to insert a WORKING splat over MCP: `create_entity(class=\"GaussianSplats\")` produces an EMPTY splat because it cannot attach the cloud path. Standard 3DGS `.ply` only (f_dc/f_rest/opacity/scale/rot).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the source `.ply` Gaussian-splat file — absolute, or relative to the Universe root (e.g. `assets/splats/bicycle.ply`). It is copied into `assets/splats/` if not already there (a file already staged there is referenced in place, never re-copied)."
                    },
                    "name":     { "type": "string", "description": "Entity name (folder + Instance.name)." },
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] world position in meters. Default [0, 2, 0]." },
                    "cull_floaters": { "type": "boolean", "description": "Remove near-transparent floater splats on load (cleaner capture). Default true." },
                    "ppisp":         { "type": "boolean", "description": "Apply PPISP photometric correction. Default true." }
                },
                "required": ["path", "name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.insert_gaussian_splats"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let fail = |msg: String| ToolResult {
            tool_name: "insert_gaussian_splats".to_string(),
            tool_use_id: String::new(),
            success: false,
            content: msg,
            structured_data: None,
            stream_topic: None,
        };

        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("SplatCloud");
        let position = parse_vec3(&input, "position", [0.0, 2.0, 0.0]);
        let cull_floaters = input.get("cull_floaters").and_then(|v| v.as_bool()).unwrap_or(true);
        let ppisp = input.get("ppisp").and_then(|v| v.as_bool()).unwrap_or(true);

        if path_str.is_empty() {
            return fail("'path' is required (a `.ply` Gaussian-splat file)".to_string());
        }

        // The splat asset root is `<Universe>/assets/splats/`. `universe_root`
        // is the sandbox boundary the engine is running against — the same
        // place its file-watcher scans.
        let universe_root = ctx.universe_root.clone();

        // Resolve the source `.ply` (absolute, or relative to the Universe root).
        let src = {
            let p = std::path::Path::new(path_str);
            if p.is_absolute() { p.to_path_buf() } else { universe_root.join(path_str) }
        };
        if !src.exists() {
            return fail(format!("source `.ply` not found: {}", src.display()));
        }
        let is_ply = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ply"))
            .unwrap_or(false);
        if !is_ply {
            return fail("only standard 3DGS `.ply` files are supported".to_string());
        }

        // Stage into `<Universe>/assets/splats/<basename>` — but NEVER re-copy a
        // multi-hundred-MB cloud that is already staged there.
        let splats_dir = universe_root.join("assets").join("splats");
        if let Err(e) = std::fs::create_dir_all(&splats_dir) {
            return fail(format!("could not create assets/splats: {e}"));
        }
        let basename = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "cloud.ply".to_string());
        let dest = splats_dir.join(&basename);
        if src != dest && !dest.exists() {
            if let Err(e) = std::fs::copy(&src, &dest) {
                return fail(format!("could not copy `.ply` into assets/splats: {e}"));
            }
        }
        let rel_path = format!("assets/splats/{}", basename);

        // Write the GaussianSplats instance folder + `_instance.toml`.
        let workspace_dir = ctx.space_root.join("Workspace");
        let overrides = eustress_common::instance_create::InstanceOverrides {
            display_name: Some(name.to_string()),
            position: Some(position),
            // Land COLMAP-frame splats upright (180° about X) — see the const doc.
            rotation: Some(eustress_common::instance_create::GAUSSIAN_SPLAT_UPRIGHT_ROTATION),
            ..Default::default()
        };
        let created = match eustress_common::instance_create::create_instance(
            &workspace_dir,
            "GaussianSplats",
            Some(name),
            overrides,
        ) {
            Ok(c) => c,
            Err(e) => return fail(format!("could not create GaussianSplats instance: {e}")),
        };

        // Post-process: inject the `[gaussian_splats]` section. `create_instance`
        // has no notion of it, and it must NOT be an `[asset]` (whose `mesh`
        // field is required — a path-only asset fails to deserialize); it lands
        // in the instance's generic `extra` catch-all, which the loader reads to
        // attach the radiance-field cloud. Read-parse-insert-write, mirroring the
        // engine's `do_import_gaussian_splat`.
        let toml_path = created.toml_path.clone();
        let raw = match std::fs::read_to_string(&toml_path) {
            Ok(s) => s,
            Err(e) => return fail(format!("wrote instance but could not re-read it: {e}")),
        };
        let mut doc: toml::Value = match raw.parse() {
            Ok(d) => d,
            Err(e) => return fail(format!("wrote instance but could not parse it: {e}")),
        };
        if let Some(t) = doc.as_table_mut() {
            let mut gs = toml::value::Table::new();
            gs.insert("path".to_string(), toml::Value::String(rel_path.clone()));
            gs.insert("cull_floaters".to_string(), toml::Value::Boolean(cull_floaters));
            gs.insert("ppisp".to_string(), toml::Value::Boolean(ppisp));
            t.insert("gaussian_splats".to_string(), toml::Value::Table(gs));
        }
        let out = match toml::to_string_pretty(&doc) {
            Ok(s) => s,
            Err(e) => return fail(format!("could not re-serialize instance: {e}")),
        };
        if let Err(e) = std::fs::write(&toml_path, out) {
            return fail(format!("could not write [gaussian_splats] section: {e}"));
        }

        ToolResult {
            tool_name: "insert_gaussian_splats".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!(
                "Inserted GaussianSplats '{}' from {} at [{:.1}, {:.1}, {:.1}] — the engine will render it, cull floaters (cull_floaters={}, ppisp={}), and extract a physics collider.",
                created.folder_name, rel_path, position[0], position[1], position[2], cull_floaters, ppisp
            ),
            structured_data: Some(serde_json::json!({
                "class": "GaussianSplats",
                "name": created.folder_name,
                "file": created.toml_path.to_string_lossy(),
                "cloud_path": rel_path,
                "cull_floaters": cull_floaters,
                "ppisp": ppisp,
            })),
            stream_topic: Some("workshop.tool.insert_gaussian_splats".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Query Entities
// ---------------------------------------------------------------------------

pub struct QueryEntitiesTool;

impl ToolHandler for QueryEntitiesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_entities",
            description: "Query entities in the current Space's Workspace. Optionally filter by class. Returns names, classes, and file paths.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class": { "type": "string", "description": "Filter by entity class: Part or Model" }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let class_filter = input.get("class").and_then(|v| v.as_str());
        let workspace_dir = ctx.space_root.join("Workspace");
        let mut entities = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&workspace_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Resolve TOML: folder/_instance.toml or flat .part.toml/.glb.toml
                let toml_path = if path.is_dir() {
                    let inst = path.join("_instance.toml");
                    if inst.exists() { inst } else { continue; }
                } else if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") {
                    path.clone()
                } else {
                    continue;
                };

                if let Ok(content) = std::fs::read_to_string(&toml_path) {
                    if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                        let class = val.get("metadata").and_then(|m| m.get("class_name")).and_then(|c| c.as_str()).unwrap_or("Unknown");
                        // Name fallback: strip the compound extension
                        // from the filename so `Grid.part.toml` yields
                        // "Grid", not the full filename. Folder-based
                        // entities use their parent directory name
                        // (`Block/` with `_instance.toml` → "Block").
                        let stem = if path.is_dir() {
                            path.file_name().and_then(|n| n.to_str()).unwrap_or(fname).to_string()
                        } else {
                            fname.split('.').next().unwrap_or(fname).to_string()
                        };
                        let name = val.get("metadata").and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or(&stem);
                        if let Some(f) = class_filter { if class != f { continue; } }
                        entities.push(serde_json::json!({ "name": name, "class": class, "file": fname }));
                    }
                }
            }
        }

        // Claude reads `content`, not `structured_data`, so spell the
        // entities out in the summary instead of parking them in a
        // sidecar field the LLM never sees. Without this, the agent
        // reliably called `query_entities` and then asked "but I don't
        // know the names" because `content` only said "Found N".
        let filter_note = class_filter
            .map(|c| format!(" of class '{}'", c))
            .unwrap_or_default();
        let body = if entities.is_empty() {
            format!("No entities{} in Workspace.", filter_note)
        } else {
            let lines: Vec<String> = entities
                .iter()
                .map(|e| format!(
                    "  - {} ({})",
                    e.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                    e.get("class").and_then(|v| v.as_str()).unwrap_or("?"),
                ))
                .collect();
            format!(
                "Found {} entities{}:\n{}",
                entities.len(),
                filter_note,
                lines.join("\n")
            )
        };

        ToolResult {
            tool_name: "query_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: body,
            structured_data: Some(serde_json::json!({ "entities": entities })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Update Entity
// ---------------------------------------------------------------------------

pub struct UpdateEntityTool;

impl ToolHandler for UpdateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "update_entity",
            description: "Update properties of an existing entity in the Workspace. Reads the .part.toml or .glb.toml file, modifies specified properties, and writes back. The engine hot-reloads the changes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Entity name (matches [metadata].name in the TOML)" },
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] new position" },
                    "size": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] new size" },
                    "material": { "type": "string", "description": "New material preset" },
                    "color": { "type": "array", "items": { "type": "number" }, "description": "[r, g, b] new color — either 0.0-1.0 floats or 0-255 integers" },
                    "transparency": { "type": "number", "description": "Transparency (0.0 = opaque, 1.0 = invisible)" },
                    "anchored": { "type": "boolean", "description": "Whether the entity is anchored (immovable)" },
                    "can_collide": { "type": "boolean", "description": "Whether the entity participates in collision" },
                    "unit": {
                        "type": "string",
                        "description": "Unit `position` and `size` are expressed in (m, cm, mm, ft, in, studs or aliases). Defaults to engine-native meters. The value is converted to the file's authored unit (from metadata.unit) before writing — so the round-trip stays symmetric regardless of which unit the caller speaks."
                    }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.update_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace = ctx.space_root.join("Workspace");

        // Find the entity file. Recursive folder search first (handles
        // entities created under a `parent` Model at any depth), then
        // legacy flat files at the Workspace root.
        let filepath = if let Some(folder) = find_entity_folder(&workspace, &safe_name) {
            Some(folder.join("_instance.toml"))
        } else {
            let legacy_candidates = [
                workspace.join(format!("{}.part.toml", safe_name)),
                workspace.join(format!("{}.glb.toml", safe_name)),
            ];
            legacy_candidates.into_iter().find(|p| p.exists())
        };
        let filepath = match filepath {
            Some(p) => p,
            None => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Entity '{}' not found in Workspace", name),
                structured_data: None, stream_topic: None,
            },
        };

        // Read existing TOML
        let content = match std::fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(e) => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to read {}: {}", filepath.display(), e),
                structured_data: None, stream_topic: None,
            },
        };

        let mut doc: toml::Value = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to parse TOML: {}", e),
                structured_data: None, stream_topic: None,
            },
        };

        let mut changes = Vec::new();

        // Stage 9 — resolve caller's input unit and the file's
        // authored unit. We convert caller→authored at write time so
        // the on-disk values stay consistent with `metadata.unit`.
        // Input unit priority: explicit `unit` arg → current
        // DisplayUnit (what the user is working in) → engine-native
        // meters. Same chain as create_entity so the AI doesn't need
        // to know the unit explicitly when the user has already
        // declared it via the status-bar dropdown.
        let authored_unit: eustress_common::units::Unit = doc.get("metadata")
            .and_then(|m| m.as_table())
            .and_then(|m| m.get("unit"))
            .and_then(|v| v.as_str())
            .and_then(eustress_common::units::Unit::from_symbol)
            .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
        let input_unit: eustress_common::units::Unit = input.get("unit")
            .and_then(|v| v.as_str())
            .and_then(eustress_common::units::Unit::from_any)
            .or_else(|| ctx.display_unit.as_deref().and_then(eustress_common::units::Unit::from_any))
            .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);

        // Apply position
        if let Some(pos) = input.get("position").and_then(|v| v.as_array()) {
            if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
                let arr: Vec<toml::Value> = pos.iter().map(|v| {
                    let m = eustress_common::units::convert(
                        v.as_f64().unwrap_or(0.0), input_unit, authored_unit,
                    );
                    toml::Value::Float(m)
                }).collect();
                transform.insert("position".to_string(), toml::Value::Array(arr));
                changes.push("position");
            }
        }

        // Apply size (stored as scale in transform)
        if let Some(size) = input.get("size").and_then(|v| v.as_array()) {
            if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
                let arr: Vec<toml::Value> = size.iter().map(|v| {
                    toml::Value::Float(eustress_common::units::convert(
                        v.as_f64().unwrap_or(1.0), input_unit, authored_unit,
                    ))
                }).collect();
                transform.insert("scale".to_string(), toml::Value::Array(arr));
                changes.push("size");
            }
        }

        // Apply properties
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            if let Some(mat) = input.get("material").and_then(|v| v.as_str()) {
                props.insert("material".to_string(), toml::Value::String(mat.to_string()));
                changes.push("material");
            }
            if let Some(color) = input.get("color").and_then(|v| v.as_array()) {
                let raw = [
                    color.get(0).and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
                    color.get(1).and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
                    color.get(2).and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
                ];
                let normalized = normalize_color(raw);
                let arr = vec![
                    toml::Value::Float(normalized[0] as f64),
                    toml::Value::Float(normalized[1] as f64),
                    toml::Value::Float(normalized[2] as f64),
                    toml::Value::Float(1.0),
                ];
                props.insert("color".to_string(), toml::Value::Array(arr));
                changes.push("color");
            }
            if let Some(t) = input.get("transparency").and_then(|v| v.as_f64()) {
                props.insert("transparency".to_string(), toml::Value::Float(t));
                changes.push("transparency");
            }
            if let Some(a) = input.get("anchored").and_then(|v| v.as_bool()) {
                props.insert("anchored".to_string(), toml::Value::Boolean(a));
                changes.push("anchored");
            }
            if let Some(c) = input.get("can_collide").and_then(|v| v.as_bool()) {
                props.insert("can_collide".to_string(), toml::Value::Boolean(c));
                changes.push("can_collide");
            }
        }

        if changes.is_empty() {
            return ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: true, content: format!("No changes specified for '{}'", name),
                structured_data: None, stream_topic: None,
            };
        }

        // Write back
        let new_content = toml::to_string_pretty(&doc).unwrap_or_default();
        match std::fs::write(&filepath, &new_content) {
            Ok(_) => ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: true,
                content: format!("Updated '{}': {}", name, changes.join(", ")),
                structured_data: Some(serde_json::json!({ "name": name, "changed": changes })),
                stream_topic: Some("workshop.tool.update_entity".to_string()),
            },
            Err(e) => ToolResult {
                tool_name: "update_entity".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Failed to write: {}", e),
                structured_data: None, stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Delete Entity
// ---------------------------------------------------------------------------

pub struct DeleteEntityTool;

impl ToolHandler for DeleteEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delete_entity",
            description: "Delete an entity from the Workspace by removing its .part.toml or .glb.toml file. The engine will despawn the entity on next file-watcher cycle.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Entity name to delete" }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.delete_entity"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let safe_name = name.replace(' ', "_").replace('/', "_");
        let workspace = ctx.space_root.join("Workspace");

        // Try folder-based first (recursive — handles entities created
        // under a `parent` Model at any depth), then legacy flat files.
        if let Some(folder_path) = find_entity_folder(&workspace, &safe_name) {
            match std::fs::remove_dir_all(&folder_path) {
                Ok(_) => return ToolResult {
                    tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                    success: true,
                    content: format!("Deleted entity '{}' (folder)", name),
                    structured_data: Some(serde_json::json!({ "name": name, "file": folder_path.to_string_lossy() })),
                    stream_topic: Some("workshop.tool.delete_entity".to_string()),
                },
                Err(e) => return ToolResult {
                    tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                    success: false, content: format!("Failed to delete folder: {}", e),
                    structured_data: None, stream_topic: None,
                },
            }
        }

        // Legacy flat file fallback
        let legacy_candidates = [
            workspace.join(format!("{}.part.toml", safe_name)),
            workspace.join(format!("{}.glb.toml", safe_name)),
        ];
        for path in &legacy_candidates {
            if path.exists() {
                match std::fs::remove_file(path) {
                    Ok(_) => return ToolResult {
                        tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                        success: true,
                        content: format!("Deleted entity '{}' ({})", name, path.file_name().unwrap_or_default().to_string_lossy()),
                        structured_data: Some(serde_json::json!({ "name": name, "file": path.to_string_lossy() })),
                        stream_topic: Some("workshop.tool.delete_entity".to_string()),
                    },
                    Err(e) => return ToolResult {
                        tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
                        success: false, content: format!("Failed to delete: {}", e),
                        structured_data: None, stream_topic: None,
                    },
                }
            }
        }

        ToolResult {
            tool_name: "delete_entity".to_string(), tool_use_id: String::new(),
            success: false, content: format!("Entity '{}' not found in Workspace", name),
            structured_data: None, stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a color triple to the engine's 0.0-1.0 linear range.
/// `create_entity`'s schema documents `color` as accepting EITHER
/// 0.0-1.0 floats OR 0-255 integers, but the value used to be written to
/// `_instance.toml` verbatim while the renderer always treats it as
/// 0.0-1.0 — so any 0-255 input silently clamped every channel above 1.0
/// to pure white, with no error. A 157-entity scene authored entirely in
/// 0-255 (the schema's second documented form) rendered entirely white.
///
/// If any component exceeds 1.0, the WHOLE triple is assumed to be
/// 0-255 and divided down together — scaling only the out-of-range
/// channel would desaturate the color instead of just rescaling it.
/// `[1,1,1]` is genuinely ambiguous between float white and near-black
/// 0-255; resolved as float white here, matching the schema's
/// float-first documentation order (callers who want near-black 0-255
/// should pass `[1.0, 1.0, 1.0]` won't hit this path anyway since no
/// component exceeds 1.0 — use `[2, 2, 2]` or similar if a true 0-255
/// near-black is intended, or just pass the float form directly).
fn normalize_color(c: [f32; 3]) -> [f32; 3] {
    if c[0] > 1.0 || c[1] > 1.0 || c[2] > 1.0 {
        [c[0] / 255.0, c[1] / 255.0, c[2] / 255.0]
    } else {
        c
    }
}

fn parse_vec3(input: &serde_json::Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    input.get(key).and_then(|v| v.as_array()).map(|a| {
        [
            a.get(0).and_then(|v| v.as_f64()).unwrap_or(default[0] as f64) as f32,
            a.get(1).and_then(|v| v.as_f64()).unwrap_or(default[1] as f64) as f32,
            a.get(2).and_then(|v| v.as_f64()).unwrap_or(default[2] as f64) as f32,
        ]
    }).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_color_passes_through_float_range() {
        assert_eq!(normalize_color([0.541, 0.549, 0.518]), [0.541, 0.549, 0.518]);
    }

    #[test]
    fn normalize_color_scales_0_255_range() {
        let got = normalize_color([138.0, 140.0, 132.0]);
        assert!((got[0] - 138.0 / 255.0).abs() < 1e-6);
        assert!((got[1] - 140.0 / 255.0).abs() < 1e-6);
        assert!((got[2] - 132.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_color_treats_all_ones_as_float_white() {
        // Documented ambiguous case: [1,1,1] resolves to float white,
        // not near-black 0-255 — matches the schema's float-first order.
        assert_eq!(normalize_color([1.0, 1.0, 1.0]), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn normalize_color_scales_whole_triple_not_per_channel() {
        // One channel over 1.0 must scale ALL three together, not clamp
        // just the offending channel (which would desaturate the color).
        let got = normalize_color([255.0, 0.0, 0.0]);
        assert!((got[0] - 1.0).abs() < 1e-6);
        assert_eq!(got[1], 0.0);
        assert_eq!(got[2], 0.0);
    }

    #[test]
    fn find_entity_folder_locates_directly_nested_entity() {
        let tmp = std::env::temp_dir().join(format!(
            "eustress-entity-tools-test-{}-direct",
            std::process::id()
        ));
        let workspace = tmp.join("Workspace");
        let entity = workspace.join("TopLevelPart");
        std::fs::create_dir_all(&entity).unwrap();
        std::fs::write(entity.join("_instance.toml"), "").unwrap();

        let found = find_entity_folder(&workspace, "TopLevelPart");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(found, Some(entity));
    }

    #[test]
    fn find_entity_folder_locates_deeply_nested_entity() {
        let tmp = std::env::temp_dir().join(format!(
            "eustress-entity-tools-test-{}-nested",
            std::process::id()
        ));
        let workspace = tmp.join("Workspace");
        // Mirrors the report's exact repro shape:
        // Workspace/Plant/Stage2_Reactor/S2_PlasmaCore/_instance.toml
        let entity = workspace.join("Plant").join("Stage2_Reactor").join("S2_PlasmaCore");
        std::fs::create_dir_all(&entity).unwrap();
        std::fs::write(entity.join("_instance.toml"), "").unwrap();
        // A sibling folder without _instance.toml must NOT match — proves
        // the search checks for a real entity marker, not just a name.
        let decoy = workspace.join("Plant").join("S2_PlasmaCore");
        std::fs::create_dir_all(&decoy).unwrap();

        let found = find_entity_folder(&workspace, "S2_PlasmaCore");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(found, Some(entity));
    }

    #[test]
    fn find_entity_folder_skips_hidden_directories() {
        let tmp = std::env::temp_dir().join(format!(
            "eustress-entity-tools-test-{}-hidden",
            std::process::id()
        ));
        let workspace = tmp.join("Workspace");
        // A trashed entity under `.eustress/trash/` must not be found by
        // a live-entity lookup — mirrors the file watcher's own hidden-dir
        // skip convention.
        let trashed = workspace.join(".eustress").join("trash").join("GhostPart");
        std::fs::create_dir_all(&trashed).unwrap();
        std::fs::write(trashed.join("_instance.toml"), "").unwrap();

        let found = find_entity_folder(&workspace, "GhostPart");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(found, None);
    }

    #[test]
    fn find_entity_folder_returns_none_when_absent() {
        let tmp = std::env::temp_dir().join(format!(
            "eustress-entity-tools-test-{}-absent",
            std::process::id()
        ));
        let workspace = tmp.join("Workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let found = find_entity_folder(&workspace, "DoesNotExist");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(found, None);
    }
}
