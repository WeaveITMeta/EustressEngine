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

// ---------------------------------------------------------------------------
// UUID — IDENTITY.md Wave 2.1 §3.2 / §10.3
// ---------------------------------------------------------------------------

/// Length in lowercase hex chars of a Eustress UUID (`blake3(seed)[..16]` → 32 chars).
pub const UUID_HEX_LEN: usize = 32;

/// Generate a fresh UUID for a Studio-create-style entry surface
/// (IDENTITY.md §3.2 / §10.3). Returns 32 lowercase hex chars derived from
/// `blake3(uuid_v4_random_bytes ‖ "\x1f" ‖ creation_unix_nanos)[..16]`.
///
/// Two simultaneous create surfaces on the same wall-clock instant don't
/// collide because the leading random bytes come from `uuid::Uuid::new_v4`
/// (cryptographic RNG). The trailing nanos make audit-replay deterministic
/// for a sequence of creates by the same user.
///
/// Used by `apply_overrides` to stamp `metadata.uuid` on every Studio
/// create + by the orchestrator's `__bin_` synthetic-path migration path.
pub fn fresh_uuid_for_create() -> String {
    // 16 bytes random from uuid::Uuid::new_v4 (cryptographic RNG).
    let rand_bytes: [u8; 16] = *uuid::Uuid::new_v4().as_bytes();
    let nanos: u128 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut seed = Vec::with_capacity(16 + 1 + 16);
    seed.extend_from_slice(&rand_bytes);
    seed.push(0x1f);
    seed.extend_from_slice(&nanos.to_be_bytes());
    let hash = blake3::hash(&seed);
    hex_encode_first_16(hash.as_bytes())
}

/// Derive a UUID for the TOML-import surface (IDENTITY.md §3.1):
/// `blake3(space_relative_path ‖ "\x1f" ‖ first_load_unix_nanos)[..16]`.
///
/// The path makes UUIDs unique within a Space (no two TOMLs can hash to the
/// same UUID — paths are unique). The timestamp makes a parallel migration on
/// a CI checkout match a developer's local one if they migrate at the same
/// wall-clock instant. The write-back to TOML in the migration guarantees
/// that every subsequent import is the "uuid is present" branch.
pub fn derive_uuid_for_import(space_rel_path: &str, first_load_nanos: u128) -> String {
    let mut seed = Vec::with_capacity(space_rel_path.len() + 1 + 16);
    seed.extend_from_slice(space_rel_path.as_bytes());
    seed.push(0x1f);
    seed.extend_from_slice(&first_load_nanos.to_be_bytes());
    let hash = blake3::hash(&seed);
    hex_encode_first_16(hash.as_bytes())
}

/// Validate that `s` is exactly 32 lowercase hex chars — the IDENTITY.md
/// §7.3 format. Invalid format → caller treats as "not present" and
/// generates fresh (see §6.2 migration).
pub fn is_valid_uuid(s: &str) -> bool {
    s.len() == UUID_HEX_LEN
        && s.bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Encode the first 16 bytes of `hash` as 32 lowercase hex chars (no dashes).
/// IDENTITY.md §7.3: "lowercase hex, 32 chars, no separators — forever".
fn hex_encode_first_16(hash: &[u8]) -> String {
    let mut out = String::with_capacity(UUID_HEX_LEN);
    for &b in &hash[..16] {
        out.push(hex_nibble(b >> 4));
        out.push(hex_nibble(b & 0x0f));
    }
    out
}

#[inline]
fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '0',
    }
}

/// Parse a 32-char lowercase-hex UUID into its 16 raw bytes. Returns `None`
/// when the input fails [`is_valid_uuid`]. Used at the Fjall boundary
/// (`path_to_uuid`, `entities/<uuid>` keys are byte-keyed; the human-facing
/// surface keeps the hex string).
pub fn uuid_hex_to_bytes(hex: &str) -> Option<[u8; 16]> {
    if !is_valid_uuid(hex) {
        return None;
    }
    let mut out = [0u8; 16];
    let bytes = hex.as_bytes();
    for i in 0..16 {
        let hi = hex_byte(bytes[i * 2])?;
        let lo = hex_byte(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

/// Inverse of [`uuid_hex_to_bytes`] — 16 raw bytes → 32-char lowercase hex.
pub fn uuid_bytes_to_hex(bytes: &[u8; 16]) -> String {
    let mut out = String::with_capacity(UUID_HEX_LEN);
    for &b in bytes {
        out.push(hex_nibble(b >> 4));
        out.push(hex_nibble(b & 0x0f));
    }
    out
}

#[inline]
fn hex_byte(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

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
    /// Optional reflectance (0.0–1.0). Sets `[properties].reflectance`.
    /// The runtime loader reads `properties.reflectance` into `BasePart`
    /// and `material_sync` applies it, so this slot makes the importer's
    /// Roblox `Reflectance` functional instead of inert extras.
    pub reflectance: Option<f32>,
    /// Optional cast_shadow flag. Sets `[properties].cast_shadow`.
    /// Consumed by the runtime loader's `BasePart` build + `material_sync`
    /// just like `reflectance`.
    pub cast_shadow: Option<bool>,
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
    /// Authoring unit symbol for this instance — stamped into
    /// `metadata.unit`. When `None`, the field is omitted from the
    /// TOML and the engine treats the entity as engine-native meters
    /// on the next load. Callers (Insert menu, paste, MCP create) pass
    /// the Space-default unit here so the file records its provenance
    /// instead of relying on the implicit default.
    pub unit_symbol: Option<String>,
    /// Optional explicit uuid to stamp into `metadata.uuid` instead of
    /// minting a fresh one. Used by Phase-3.5 PROMOTE (binary-ECS core →
    /// on-disk TOML folder): the materialized folder MUST keep the entity's
    /// existing uuid so identity (find-by-uuid, references) survives the
    /// representation change. Ignored unless it is a valid 32-char
    /// lowercase-hex uuid; `None` keeps the normal fresh-mint behavior.
    pub uuid: Option<String>,
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
    let has_template = template_toml.is_file();

    let preferred = requested_name.unwrap_or(class_name);
    std::fs::create_dir_all(dest_dir).map_err(|e| CreateError::Io {
        what: format!("create dest dir {}", dest_dir.display()),
        error: e,
    })?;
    let folder_name = unique_entity_name(dest_dir, preferred);
    let folder_path = dest_dir.join(&folder_name);

    if has_template {
        copy_template_recursive(&template_root, &folder_path).map_err(|e| CreateError::Io {
            what: format!("copy template {} → {}", template_root.display(), folder_path.display()),
            error: e,
        })?;
    } else {
        // No authored `class_schema/<Class>/_instance.toml`. Rather than fail
        // — which drops the node and, for a Roblox import, aborts the whole
        // place — SYNTHESIZE a minimal generic instance. This makes instance
        // creation TOTAL over `ClassName`: every mapped class materializes
        // losslessly. The class identity is preserved in `metadata.class_name`;
        // `apply_overrides` (below) + any caller second-pass layer the real
        // transform / material / properties on top, and a registered
        // `ClassSpawner` (if one exists for this class) still drives behaviour.
        std::fs::create_dir_all(&folder_path).map_err(|e| CreateError::Io {
            what: format!("create synthesized instance folder {}", folder_path.display()),
            error: e,
        })?;
        let skeleton = format!(
            "# Auto-synthesized instance — no class_schema template for this class.\n\
             # Instance creation is total over ClassName: the class identity and\n\
             # all mapped properties are preserved so nothing is dropped on import.\n\n\
             [metadata]\nclass_name = \"{class_name}\"\narchivable = true\n"
        );
        std::fs::write(folder_path.join("_instance.toml"), skeleton).map_err(|e| {
            CreateError::Io {
                what: format!(
                    "write synthesized _instance.toml in {}",
                    folder_path.display()
                ),
                error: e,
            }
        })?;
    }

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

    // UUID stamp — IDENTITY.md §3.2 / §10.3. Every Studio create lands with
    // a uuid in `[metadata]` so the file_watcher's spawn sees it on the very
    // first load (no transient empty-uuid window per §12.5). If the template
    // already had a uuid (rare — class_schema templates are uuid-less), we
    // preserve it verbatim; otherwise we mint a fresh one via
    // `fresh_uuid_for_create()`.
    {
        let meta = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(meta_table) = meta.as_table_mut() {
            let has_valid = meta_table
                .get("uuid")
                .and_then(|v| v.as_str())
                .map(is_valid_uuid)
                .unwrap_or(false);
            if !has_valid {
                // Phase 3.5 PROMOTE: honor an explicit valid uuid override
                // (preserve the binary entity's identity when materializing
                // it to disk) before falling back to a fresh mint.
                let uuid = overrides
                    .uuid
                    .as_deref()
                    .filter(|u| is_valid_uuid(u))
                    .map(|u| u.to_string())
                    .unwrap_or_else(fresh_uuid_for_create);
                meta_table.insert(
                    "uuid".to_string(),
                    toml::Value::String(uuid),
                );
            }
        }
    }

    // Authoring-unit override — Stage 7. Only stamped when the caller
    // specifies a unit (so legacy templates without a unit field stay
    // implicitly meter-native instead of getting a spurious `"m"`
    // sprinkled in). The symbol is validated by the caller; this
    // function trusts the value and writes it verbatim.
    if let Some(sym) = overrides.unit_symbol.as_deref() {
        let meta = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(meta_table) = meta.as_table_mut() {
            meta_table.insert(
                "unit".to_string(),
                toml::Value::String(sym.to_string()),
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
        || overrides.reflectance.is_some()
        || overrides.cast_shadow.is_some()
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
            if let Some(refl) = overrides.reflectance {
                p.insert("reflectance".to_string(), toml::Value::Float(refl as f64));
            }
            if let Some(cs) = overrides.cast_shadow {
                p.insert("cast_shadow".to_string(), toml::Value::Boolean(cs));
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

// ---------------------------------------------------------------------------
// Tests — IDENTITY.md §14.1
// ---------------------------------------------------------------------------

#[cfg(test)]
mod uuid_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fresh_uuid_is_32_lowercase_hex() {
        let u = fresh_uuid_for_create();
        assert_eq!(u.len(), 32, "uuid must be 32 chars, got {}: {u:?}", u.len());
        assert!(
            u.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "uuid must be lowercase hex: {u:?}"
        );
        assert!(is_valid_uuid(&u));
    }

    #[test]
    fn ten_thousand_uuids_are_unique() {
        // The random component drives uniqueness — even if every call
        // landed on the same nanosecond they must not collide.
        let mut seen: HashSet<String> = HashSet::new();
        for _ in 0..10_000 {
            let u = fresh_uuid_for_create();
            assert!(seen.insert(u.clone()), "collision: {u:?}");
        }
    }

    #[test]
    fn derive_uuid_for_import_is_deterministic() {
        let a = derive_uuid_for_import("Workspace/Tower/_instance.toml", 1234);
        let b = derive_uuid_for_import("Workspace/Tower/_instance.toml", 1234);
        assert_eq!(a, b, "same seed → same uuid");
        let c = derive_uuid_for_import("Workspace/Tower/_instance.toml", 1235);
        assert_ne!(a, c, "different nanos → different uuid");
        let d = derive_uuid_for_import("Workspace/Other/_instance.toml", 1234);
        assert_ne!(a, d, "different path → different uuid");
    }

    #[test]
    fn is_valid_uuid_examples() {
        assert!(is_valid_uuid("4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7"));
        assert!(is_valid_uuid("0000000000000000000000000000ffff"));
        // Wrong length
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("4f3a8c2b1e9d7654a0b8c2e3f4d5a6"));
        assert!(!is_valid_uuid("4f3a8c2b1e9d7654a0b8c2e3f4d5a6b78"));
        // Wrong case
        assert!(!is_valid_uuid("4F3A8C2B1E9D7654A0B8C2E3F4D5A6B7"));
        // Wrong charset
        assert!(!is_valid_uuid("4f3a8c2b1e9d7654a0b8c2e3f4d5a6gh"));
        // Includes dashes — IDENTITY.md §7.3 forbids
        assert!(!is_valid_uuid("4f3a8c2b-1e9d-7654-a0b8-c2e3f4d5a6b7"));
    }

    #[test]
    fn uuid_bytes_roundtrip() {
        let hex = fresh_uuid_for_create();
        let bytes = uuid_hex_to_bytes(&hex).expect("valid uuid parses");
        let back = uuid_bytes_to_hex(&bytes);
        assert_eq!(hex, back);
    }

    #[test]
    fn uuid_hex_to_bytes_rejects_invalid() {
        assert!(uuid_hex_to_bytes("").is_none());
        assert!(uuid_hex_to_bytes("not-hex-at-all-not-hex-at-all-no").is_none());
        assert!(uuid_hex_to_bytes("4F3A8C2B1E9D7654A0B8C2E3F4D5A6B7").is_none());
    }
}

#[cfg(test)]
mod apply_overrides_tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    /// Build a unique tempdir under `std::env::temp_dir()`. Avoids adding a
    /// dev-dependency on `tempfile` (orchestrator owns Cargo.toml). The dir
    /// is best-effort cleaned by the test's drop; even if the cleanup fails
    /// on a CI host, the deterministic-but-unique seed bounds the leak.
    fn make_tempdir() -> std::path::PathBuf {
        let stem = format!(
            "eustress_uuid_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let p = std::env::temp_dir().join(stem);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    /// `apply_overrides` always stamps a fresh UUID into `[metadata].uuid`
    /// when one is missing.
    #[test]
    fn apply_overrides_stamps_uuid_when_missing() {
        let dir = make_tempdir();
        let toml_path = dir.join("_instance.toml");
        // Minimal valid template — no metadata.uuid.
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "[metadata]").unwrap();
        writeln!(f, "class_name = \"Part\"").unwrap();
        drop(f);
        let overrides = InstanceOverrides::default();
        apply_overrides(&toml_path, "Part", "Part", &overrides).expect("apply_overrides");
        let raw = fs::read_to_string(&toml_path).unwrap();
        // Parse and inspect — robust against TOML key order.
        let doc: toml::Value = raw.parse().unwrap();
        let uuid_str = doc
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .expect("uuid was stamped into metadata");
        assert!(
            is_valid_uuid(uuid_str),
            "stamped uuid must be 32-lowercase-hex: {uuid_str:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An existing valid UUID is preserved verbatim — the create path is
    /// idempotent under re-runs (IDENTITY.md §6.2 resumability discipline).
    #[test]
    fn apply_overrides_preserves_existing_uuid() {
        let dir = make_tempdir();
        let toml_path = dir.join("_instance.toml");
        let original = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "[metadata]").unwrap();
        writeln!(f, "class_name = \"Part\"").unwrap();
        writeln!(f, "uuid = \"{original}\"").unwrap();
        drop(f);
        apply_overrides(&toml_path, "Part", "Part", &InstanceOverrides::default())
            .expect("apply_overrides");
        let raw = fs::read_to_string(&toml_path).unwrap();
        let doc: toml::Value = raw.parse().unwrap();
        let uuid_str = doc
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(uuid_str, original, "existing uuid must be preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An INVALID-format uuid on disk (e.g. RFC-4122 form with dashes) gets
    /// replaced — the engine treats the bogus value as "not present" and
    /// mints a fresh canonical one.
    #[test]
    fn apply_overrides_replaces_invalid_uuid() {
        let dir = make_tempdir();
        let toml_path = dir.join("_instance.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "[metadata]").unwrap();
        writeln!(f, "class_name = \"Part\"").unwrap();
        writeln!(f, "uuid = \"NOT-A-VALID-FORMAT\"").unwrap();
        drop(f);
        apply_overrides(&toml_path, "Part", "Part", &InstanceOverrides::default())
            .expect("apply_overrides");
        let raw = fs::read_to_string(&toml_path).unwrap();
        let doc: toml::Value = raw.parse().unwrap();
        let uuid_str = doc
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(is_valid_uuid(uuid_str), "bogus uuid replaced with valid one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 3.5 PROMOTE: an explicit valid `uuid` override is stamped instead
    /// of a fresh mint, so materializing a binary entity to disk PRESERVES its
    /// identity. An invalid override falls back to a fresh mint.
    #[test]
    fn apply_overrides_honors_uuid_override() {
        let dir = make_tempdir();
        let toml_path = dir.join("_instance.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "[metadata]").unwrap();
        writeln!(f, "class_name = \"Part\"").unwrap();
        drop(f);
        let preserved = "abcdef0123456789abcdef0123456789";
        let overrides = InstanceOverrides {
            uuid: Some(preserved.to_string()),
            ..Default::default()
        };
        apply_overrides(&toml_path, "Part", "Part", &overrides).expect("apply_overrides");
        let doc: toml::Value = fs::read_to_string(&toml_path).unwrap().parse().unwrap();
        let uuid_str = doc
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(uuid_str, preserved, "explicit uuid override must be preserved");

        // Invalid override → fresh valid mint (not the bogus value).
        let toml2 = dir.join("_instance2.toml");
        let mut f2 = fs::File::create(&toml2).unwrap();
        writeln!(f2, "[metadata]").unwrap();
        writeln!(f2, "class_name = \"Part\"").unwrap();
        drop(f2);
        let bad = InstanceOverrides {
            uuid: Some("NOT-VALID".to_string()),
            ..Default::default()
        };
        apply_overrides(&toml2, "Part", "Part", &bad).expect("apply_overrides");
        let doc2: toml::Value = fs::read_to_string(&toml2).unwrap().parse().unwrap();
        let u2 = doc2
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(is_valid_uuid(u2) && u2 != "NOT-VALID", "invalid override → fresh mint");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
