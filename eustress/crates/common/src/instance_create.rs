//! Canonical entity-creation pipeline (crate-portable).
//!
//! Every "create a new instance of class X" path — Insert menu, Model
//! ribbon, Toolbox, MCP `create_entity`, future drag-drop import —
//! routes through [`create_instance`]. The result is exactly one folder
//! + one `_instance.toml` (with any class-defined sibling resources)
//! on disk at a unique non-colliding path, picked up by the file
//! watcher and spawned into the ECS with proper parent-child placement.
//!
//! ## Why this lives in `common`
//!
//! The engine crate calls this from `slint_ui.rs`, `space_ops.rs`,
//! `toolbox.rs`, … and the out-of-process MCP server's
//! `entity_tools::CreateEntityTool` calls it too. Sharing the helper
//! eliminates the previous drift where every callsite hand-built the
//! `_instance.toml` body, each with its own subtle bugs (e.g. the
//! Attachment-only-metadata invisible-visualizer regression).
//!
//! No bevy types in the public API — overrides use plain `[f32; 3]` /
//! `[f32; 4]` arrays so the tools crate (which doesn't depend on bevy)
//! can call it.

use std::path::{Path, PathBuf};

/// Per-call overrides applied to the root `_instance.toml` after the
/// template is copied. Every field is optional — `None` means "keep the
/// template's default".
#[derive(Debug, Clone, Default)]
pub struct InstanceOverrides {
    /// Desired display name. The folder name is computed from this by
    /// `unique_entity_name`; if `None`, the class name itself is used.
    pub display_name: Option<String>,
    /// World-space position for the root entity. Sets `[transform].position`.
    pub position: Option<[f32; 3]>,
    /// Optional rotation quaternion `[x, y, z, w]`. Sets `[transform].rotation`.
    pub rotation: Option<[f32; 4]>,
    /// Optional scale `[x, y, z]`. Sets `[transform].scale`.
    pub scale: Option<[f32; 3]>,
    /// Optional RGBA color override. Sets `[properties].color`.
    pub color_rgba: Option<[f32; 4]>,
    /// Optional material preset name. Sets `[properties].material`.
    pub material: Option<String>,
    /// Optional anchored flag. Sets `[properties].anchored`.
    pub anchored: Option<bool>,
    /// Optional can_collide flag. Sets `[properties].can_collide`.
    pub can_collide: Option<bool>,
    /// Optional mesh asset reference (relative path like `parts/block.glb`).
    /// When present, ensures the root TOML has an `[asset]` section with
    /// this mesh + `scene = "Scene0"`. Used by the MCP `create_entity`
    /// path to swap primitive shapes (block / ball / cylinder / …)
    /// without needing per-shape template folders.
    pub asset_mesh: Option<String>,
    /// Optional single-path asset reference (Universe-relative). Used by
    /// the Image/Video class templates whose `[asset]` block carries a
    /// `path = "assets/images/x.png"` instead of `mesh` + `scene`.
    /// Mutually exclusive with `asset_mesh`; the file-event-handler
    /// import path sets this after copying the source media into the
    /// Universe's `assets/<kind>/` directory.
    pub asset_path: Option<String>,
}

/// Outcome of a successful [`create_instance`] call.
#[derive(Debug, Clone)]
pub struct CreatedInstance {
    /// The new instance's folder on disk.
    pub folder_path: PathBuf,
    /// `<folder_path>/_instance.toml`.
    pub toml_path: PathBuf,
    /// Final (unique-safed) folder name — may differ from the requested
    /// name if a collision was avoided.
    pub folder_name: String,
    /// The class name the instance was created from.
    pub class_name: String,
}

/// Errors the pipeline can surface to its caller.
#[derive(Debug)]
pub enum CreateError {
    /// No template at `common/assets/class_schema/<Class>/_instance.toml`.
    TemplateMissing { class_name: String, looked_at: PathBuf },
    /// `mkdir`/`copy` failed at the OS layer.
    Io { what: String, error: std::io::Error },
    /// `_instance.toml` couldn't be parsed back after copy — should be
    /// impossible given the template was valid, but treated as a hard
    /// error rather than silently swallowed.
    TomlParse { path: PathBuf, error: toml::de::Error },
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::TemplateMissing { class_name, looked_at } => write!(
                f, "no class template for '{}' (expected {})",
                class_name, looked_at.display(),
            ),
            CreateError::Io { what, error } => write!(f, "{}: {}", what, error),
            CreateError::TomlParse { path, error } => write!(
                f, "parse {}: {}", path.display(), error,
            ),
        }
    }
}

impl std::error::Error for CreateError {}

/// Core entry point. Resolves the class template, copies its folder
/// shape to `dest_dir/<unique_name>/`, then patches the root TOML with
/// the supplied overrides.
///
/// `dest_dir` is the parent on disk (e.g. `<space_root>/Workspace` or
/// `<space_root>/Workspace/MyPart` for a child). `requested_name` is
/// what the user typed or what the calling surface picked; the actual
/// folder name on disk is the unique-safed version of it.
pub fn create_instance(
    dest_dir: &Path,
    class_name: &str,
    requested_name: Option<&str>,
    overrides: InstanceOverrides,
) -> Result<CreatedInstance, CreateError> {
    let template_root = crate::class_schema_dir().join(class_name);
    let template_toml = template_root.join("_instance.toml");
    if !template_toml.is_file() {
        return Err(CreateError::TemplateMissing {
            class_name: class_name.to_string(),
            looked_at: template_toml,
        });
    }

    let preferred = requested_name.unwrap_or(class_name);
    std::fs::create_dir_all(dest_dir).map_err(|e| CreateError::Io {
        what: format!("create dest dir {}", dest_dir.display()),
        error: e,
    })?;
    let folder_name = unique_entity_name(dest_dir, preferred);
    let folder_path = dest_dir.join(&folder_name);

    copy_template_recursive(&template_root, &folder_path).map_err(|e| CreateError::Io {
        what: format!("copy template {} → {}", template_root.display(), folder_path.display()),
        error: e,
    })?;

    let toml_path = folder_path.join("_instance.toml");
    apply_overrides(&toml_path, &folder_name, class_name, &overrides)?;

    Ok(CreatedInstance {
        folder_path,
        toml_path,
        folder_name,
        class_name: class_name.to_string(),
    })
}

/// Parent-first recursive directory copy. Files at each level land
/// before recursing into subfolders so the file_watcher sees parents
/// before children and `ChildOf` lookups succeed at every nesting level.
fn copy_template_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(src)?
        .collect::<Result<Vec<_>, _>>()?;
    for entry in &entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') { continue; }
        let ty = entry.file_type()?;
        if !ty.is_file() { continue; }
        std::fs::copy(entry.path(), dst.join(&name))?;
    }
    for entry in &entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') { continue; }
        let ty = entry.file_type()?;
        if !ty.is_dir() { continue; }
        copy_template_recursive(&entry.path(), &dst.join(&name))?;
    }
    Ok(())
}

fn apply_overrides(
    toml_path: &Path,
    folder_name: &str,
    class_name: &str,
    overrides: &InstanceOverrides,
) -> Result<(), CreateError> {
    let raw = std::fs::read_to_string(toml_path).map_err(|e| CreateError::Io {
        what: format!("read {}", toml_path.display()),
        error: e,
    })?;
    let mut doc: toml::Value = raw.parse().map_err(|e| CreateError::TomlParse {
        path: toml_path.to_path_buf(),
        error: e,
    })?;
    let Some(root) = doc.as_table_mut() else {
        return Ok(());
    };

    // Display-name override.
    if folder_name != class_name || overrides.display_name.is_some() {
        let display = overrides.display_name.as_deref().unwrap_or(class_name);
        let meta = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(meta_table) = meta.as_table_mut() {
            meta_table.insert(
                "name".to_string(),
                toml::Value::String(display.to_string()),
            );
        }
    }

    // Asset section override — used to swap primitive meshes per-call
    // (Part with shape=ball / cylinder / …) without dedicated templates.
    if let Some(mesh) = overrides.asset_mesh.as_deref() {
        let asset = root
            .entry("asset".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = asset.as_table_mut() {
            t.insert("mesh".to_string(), toml::Value::String(mesh.to_string()));
            t.entry("scene".to_string())
                .or_insert_with(|| toml::Value::String("Scene0".to_string()));
        }
    }

    // Single-path asset override — Image / Video classes carry an
    // `[asset].path` field (not mesh+scene). File-Import populates this
    // after copying the source media into the Universe asset tree.
    if let Some(path) = overrides.asset_path.as_deref() {
        let asset = root
            .entry("asset".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = asset.as_table_mut() {
            t.insert("path".to_string(), toml::Value::String(path.to_string()));
        }
    }

    // Transform overrides.
    if overrides.position.is_some()
        || overrides.rotation.is_some()
        || overrides.scale.is_some()
    {
        let tform = root
            .entry("transform".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = tform.as_table_mut() {
            if let Some(p) = overrides.position {
                t.insert(
                    "position".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(p[0] as f64),
                        toml::Value::Float(p[1] as f64),
                        toml::Value::Float(p[2] as f64),
                    ]),
                );
            }
            if let Some(r) = overrides.rotation {
                t.insert(
                    "rotation".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(r[0] as f64),
                        toml::Value::Float(r[1] as f64),
                        toml::Value::Float(r[2] as f64),
                        toml::Value::Float(r[3] as f64),
                    ]),
                );
            }
            if let Some(s) = overrides.scale {
                t.insert(
                    "scale".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(s[0] as f64),
                        toml::Value::Float(s[1] as f64),
                        toml::Value::Float(s[2] as f64),
                    ]),
                );
            }
        }
    }

    // Properties overrides.
    if overrides.color_rgba.is_some()
        || overrides.material.is_some()
        || overrides.anchored.is_some()
        || overrides.can_collide.is_some()
    {
        let props = root
            .entry("properties".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(p) = props.as_table_mut() {
            if let Some(c) = overrides.color_rgba {
                p.insert(
                    "color".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(c[0] as f64),
                        toml::Value::Float(c[1] as f64),
                        toml::Value::Float(c[2] as f64),
                        toml::Value::Float(c[3] as f64),
                    ]),
                );
            }
            if let Some(mat) = overrides.material.as_deref() {
                p.insert(
                    "material".to_string(),
                    toml::Value::String(mat.to_string()),
                );
            }
            if let Some(a) = overrides.anchored {
                p.insert("anchored".to_string(), toml::Value::Boolean(a));
            }
            if let Some(cc) = overrides.can_collide {
                p.insert("can_collide".to_string(), toml::Value::Boolean(cc));
            }
        }
    }

    let serialised = toml::to_string_pretty(&doc).unwrap_or(raw);
    std::fs::write(toml_path, serialised).map_err(|e| CreateError::Io {
        what: format!("write {}", toml_path.display()),
        error: e,
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Naming
// ---------------------------------------------------------------------------

/// Reserved file/folder names that EEP uses as in-folder markers — a
/// user-facing entity must never claim one. Mirrors
/// `engine::space::instance_loader::is_eep_reserved_name` so both crates
/// agree on the invariant.
pub fn is_eep_reserved_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "_instance.toml"
            | "_service.toml"
            | "_universe.toml"
            | "_space.toml"
            | "_eustress"
            | ".eustress"
    )
}

/// `true` if `name` can be used as a fresh entity folder name in `dir`
/// without collision (neither a folder nor a flat `name.*.toml` with a
/// matching stem already exists).
pub fn entity_name_is_available(dir: &Path, name: &str) -> bool {
    if name.is_empty() { return false; }
    if is_eep_reserved_name(name) { return false; }
    if dir.join(name).exists() { return false; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let Some(s) = fname.to_str() else { continue };
            if s.split('.').next() == Some(name) {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    return false;
                }
            }
        }
    }
    true
}

/// Pick a unique folder name in `dir`, starting from `base` and
/// appending a 4-hex-digit suffix on collision. Identical behaviour to
/// the engine's `unique_entity_name` (it now re-exports from here).
pub fn unique_entity_name(dir: &Path, base: &str) -> String {
    let base = if is_eep_reserved_name(base) {
        tracing::warn!(
            "unique_entity_name: caller passed reserved name {:?} — substituting 'Entity'",
            base
        );
        "Entity"
    } else {
        base
    };
    if entity_name_is_available(dir, base) {
        return base.to_string();
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for i in 0u32..10_000 {
        let mut x = seed.wrapping_add(i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 30;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 27;
        let tag = (x as u32) & 0xFFFF;
        let candidate = format!("{}-{:04x}", base, tag);
        if entity_name_is_available(dir, &candidate) {
            return candidate;
        }
    }
    format!("{}-{}", base, chrono::Utc::now().timestamp())
}
