//! # Class Schema Registry
//!
//! Authoritative, client-and-engine-shared templates for every TOML-
//! serializable class in Eustress. Every `_instance.toml` read is merged
//! against its class template on load; missing fields get filled,
//! mixed-case keys get normalised, and unrecognised sections are handed
//! to the [`ExtraSectionClaim`] trait so simulation plugins can attach
//! components dynamically.
//!
//! ## Why common, not engine?
//!
//! The same schema has to be valid whether you're the studio engine
//! writing a part, the standalone client reading one at runtime, or an
//! external MCP / LSP tool linting a TOML. Keeping the registry + all
//! templates inside `eustress-common` means every consumer derives the
//! same answer from the same source of truth.
//!
//! ## Schema contract
//!
//! - **Canonical wire format**: **snake_case**. Every engine-side serde
//!   struct (`InstanceDefinition`, `GuiTomlFile`, `ServiceDefinition`,
//!   …) uses snake_case field names — matching the historic on-disk
//!   format and Roblox-style `[metadata] class_name = "Part"` authoring.
//! - **Frontend labels**: Properties panel / Explorer display text is
//!   hand-coded PascalCase (`"ClassName"`, `"CanCollide"`) — purely
//!   cosmetic, never touches TOML keys.
//! - **Tolerant reads**: [`normalise_keys`] accepts either case and
//!   maps back to snake_case before deserialization, so TOMLs left
//!   over from the aborted PascalCase migration still load.
//! - **Sections**: every class has a `[metadata]` section with
//!   `class_name` + `archivable` + optional `name` override. Class-
//!   specific sections (`[gui]`, `[transform]`, `[properties]`,
//!   `[light]`, `[script]`, `[material]`, `[thermodynamic]`,
//!   `[electrochemical]`, `[attachment]`, …) live alongside.
//! - **Extras**: unknown sections are preserved in the loaded value.
//!   [`ExtraSectionRegistry`] lets plugins claim sections by name —
//!   unclaimed extras survive save round-trips so future plugins can
//!   pick them up.
//!
//! ## Adding a new class
//!
//! Target: one filesystem drop + zero Rust changes for vanilla classes.
//! Reality today: one to four touchpoints depending on how unique the
//! class is.
//!
//! 1. **Always**: drop `common/assets/class_schema/MyClass/_instance.toml`
//!    with sensible defaults for every `[section]` the class cares
//!    about. Include `[metadata] class_name = "MyClass"`. Folder-form so
//!    sibling resource files (e.g. an `icon.svg`, a child template
//!    `MyChildClass/_instance.toml`) can live next to the root TOML
//!    and ride along when a new instance is spawned by `copy_dir_recursive`.
//! 2. **Auto-baked**: `common/build.rs` discovers the new folder on the
//!    next build and emits an entry into [`BUILTIN_TEMPLATES`]. No
//!    Rust edits needed.
//! 3. **Usually**: add a variant to `ClassName` in
//!    `common/src/classes.rs` + an `"MyClass" => Ok(ClassName::MyClass)`
//!    arm in `ClassName::from_str`. File loader dispatch goes through
//!    `from_str` now — one arm here replaces N hard-coded branches
//!    across the engine.
//! 4. **Only for unique fields**: extend `InstanceDefinition` (for
//!    parts) or `GuiTomlFile` (for UI classes) with the new struct
//!    fields. Classes that fit the base schema (Part subclasses that
//!    only differ in defaults, like `Seat` or `SpawnLocation`) need
//!    nothing here.
//! 5. **Only for custom spawn behaviour**: register an
//!    [`ExtraSectionClaim`] impl against the class's unique sections.
//!    The loader dispatches each extra section to claimants so
//!    simulation plugins (Thermodynamic, Electrochemical, Material, …)
//!    attach ECS components without touching `spawn_instance`.
//!    [`ExtraSectionRegistry::register_builtins`] wires up every
//!    first-party claim (see `claims.rs`) in one call — plugins
//!    register their own claims additionally.
//!
//! ## Self-heal on load (active)
//!
//! `load_and_heal_instance` parses the on-disk TOML, merges in missing
//! fields from the class template, normalises keys to snake_case, and
//! rewrites the file when the canonical form differs. After one load
//! every file on disk matches the schema — user-missing fields are
//! baked in permanently and legacy-PascalCase files convert to
//! snake_case automatically. Every `_instance.toml` parser either
//! routes through this function or uses [`get_section_insensitive`].
//!
//! ## Extra-section extensibility (active)
//!
//! Sections in a TOML that `InstanceDefinition` doesn't consume as a
//! typed field (e.g. a future plugin's `[fluid]` or `[biology]`) get
//! attached to the spawned entity as [`PendingExtraSections`]. The
//! [`dispatch_pending_extras`] Bevy system then hands each section to
//! any claimant that registered against it via
//! [`ExtraSectionRegistry::register`]. Unclaimed sections are
//! preserved on-disk via `InstanceDefinition.extra` (flatten), so a
//! plugin registered in the next build picks them up on the next load
//! — no migration step needed.
//!
//! ## Build-time template discovery (active)
//!
//! [`BUILTIN_TEMPLATES`] is auto-generated by `common/build.rs` from
//! every `assets/class_schema/<Class>/_instance.toml`. Adding a new
//! class template is a single-folder drop — no Rust edits unless the
//! class needs a new [`ClassName`](crate::classes::ClassName) enum
//! variant (for strong-typed dispatch) or unique `InstanceDefinition`
//! fields (for class-specific typed sections).
//!
//! ## Startup validation (active)
//!
//! [`log_schema_validation`] runs once at Bevy `Startup` and emits
//! warn-level log lines for any template whose `[metadata].class_name`
//! doesn't match its filename stem, or whose stem isn't in the
//! `ClassName` enum. Catches drift-bugs at boot rather than at
//! load-a-Part time.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod templates;
pub use templates::BUILTIN_TEMPLATES;

pub mod claims;
pub use claims::ThermodynamicClaim;

// ============================================================================
// Public types
// ============================================================================

/// Parsed class template — the authoritative skeleton for one class. The inner
/// `toml::Value` is always a `Value::Table` whose keys are section names
/// (`"Metadata"`, `"Transform"`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassTemplate {
    /// Canonical class name (`"Part"`, `"BillboardGui"`, …) — matches the
    /// filename stem of the source `.defaults.toml` AND the `ClassName`
    /// value inside `[Metadata]`.
    pub class_name: String,
    /// Parsed template body. Guaranteed to be a `toml::Value::Table`.
    pub body: toml::Value,
}

/// Registry of every known class template. Shared via `Arc` so both client
/// and engine can hold a reference without duplicating the parse.
#[derive(Debug, Clone)]
pub struct ClassSchemaRegistry {
    templates: HashMap<String, ClassTemplate>,
}

impl Default for ClassSchemaRegistry {
    fn default() -> Self {
        Self::from_builtin()
    }
}

impl ClassSchemaRegistry {
    /// The built-in registry, parsed once per process. `from_builtin` parses
    /// every embedded class template; the loaders called it per entity, so a
    /// 106K-entity Space parsed the whole template set 106K times (the bulk
    /// of its ~5 ms per entity in a debug build). Read-only callers take
    /// this instead.
    pub fn builtin() -> &'static ClassSchemaRegistry {
        static BUILTIN: std::sync::OnceLock<ClassSchemaRegistry> = std::sync::OnceLock::new();
        BUILTIN.get_or_init(ClassSchemaRegistry::from_builtin)
    }
}

impl ClassSchemaRegistry {
    /// Build a registry from the PascalCase templates embedded in this crate
    /// via `include_str!`. No filesystem I/O — the bytes are baked into the
    /// binary so a headless client without an assets/ folder still validates
    /// correctly.
    pub fn from_builtin() -> Self {
        let mut templates = HashMap::with_capacity(BUILTIN_TEMPLATES.len());
        for (class_name, body_str) in BUILTIN_TEMPLATES {
            match body_str.parse::<toml::Value>() {
                Ok(mut body) => {
                    if !body.is_table() {
                        tracing::error!(
                            "ClassSchemaRegistry: template {} does not have a table root",
                            class_name
                        );
                        continue;
                    }
                    // Templates on disk are snake_case. Normalise the
                    // in-memory body against the canonical snake_case
                    // direction so merges against user TOMLs (which may
                    // be either case) compare key-for-key.
                    normalise_keys(&mut body);
                    templates.insert(
                        class_name.to_string(),
                        ClassTemplate {
                            class_name: class_name.to_string(),
                            body,
                        },
                    );
                }
                Err(err) => {
                    tracing::error!(
                        "ClassSchemaRegistry: failed to parse builtin template {}: {}",
                        class_name,
                        err
                    );
                }
            }
        }
        Self { templates }
    }

    /// Fetch the template for a class name. Returns `None` when the class is
    /// unknown to the schema (e.g. a user-invented subclass) — callers should
    /// fall back to writing the TOML unchanged in that case.
    pub fn template(&self, class_name: &str) -> Option<&ClassTemplate> {
        self.templates.get(class_name)
    }

    /// Number of templates in the registry.
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Iterate every class name known to the schema.
    pub fn class_names(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(|s| s.as_str())
    }

    /// Sanity-check the templates against each other. Run at engine
    /// startup so template / struct / enum drift surfaces as warn-level
    /// log lines instead of mysterious load failures later. Checks:
    ///
    /// * Every template's `[metadata].class_name` matches the filename
    ///   stem it was registered under (catches copy-paste mistakes).
    /// * `ClassName::from_str(stem)` resolves successfully (catches
    ///   "template added but enum variant forgotten").
    ///
    /// Returns a `Vec<String>` of human-readable warnings, empty when
    /// everything is in sync. Never fails — the engine keeps booting
    /// because an inconsistent template still loads data correctly,
    /// it just might route through the Folder fallback.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for (stem, tpl) in &self.templates {
            // Check `[metadata].class_name` agrees with the filename stem.
            if let Some(declared) = tpl
                .body
                .get("metadata")
                .and_then(|m| m.get("class_name"))
                .and_then(|v| v.as_str())
            {
                if declared != stem {
                    warnings.push(format!(
                        "template {}.defaults.toml declares class_name = \"{}\" — does not match filename stem",
                        stem, declared
                    ));
                }
            } else {
                warnings.push(format!(
                    "template {}.defaults.toml is missing [metadata] class_name",
                    stem
                ));
            }
            // Check the enum knows about this class.
            if crate::classes::ClassName::from_str(stem).is_err() {
                warnings.push(format!(
                    "template {}.defaults.toml has no matching ClassName enum variant — \
                     add one in common/src/classes.rs or file_loader will dispatch it as Folder",
                    stem
                ));
            }
        }
        warnings
    }
}

/// Bevy `Resource` wrapper — insert with `commands.insert_resource(…)` at
/// startup. The engine's `instance_loader` + `save_space` systems query
/// this to drive merge / self-heal / PascalCase rewrites. `common` always
/// depends on `bevy` in this workspace, so no feature gating is needed
/// yet; if we ever split `common-core` from `common-bevy` this is the
/// one type that moves.
#[derive(bevy::prelude::Resource, Debug, Clone, Default)]
pub struct ClassSchemaResource(pub ClassSchemaRegistry);

/// Startup-time consistency check. Logs every `ClassSchemaRegistry::validate`
/// warning at `warn!` level. Add this to `App::build` right after
/// inserting `ClassSchemaResource` so template / enum drift surfaces
/// immediately instead of manifesting as mysterious fallback-Folder
/// spawns down the line.
pub fn log_schema_validation(registry: bevy::prelude::Res<ClassSchemaResource>) {
    let warnings = registry.0.validate();
    if warnings.is_empty() {
        tracing::info!(
            "class_schema: {} class templates validated, no drift detected",
            registry.0.len()
        );
    } else {
        for w in &warnings {
            tracing::warn!("class_schema drift: {}", w);
        }
    }
}

// ============================================================================
// snake_case → PascalCase key normalisation
// ============================================================================

/// Recursively walk a `toml::Value` and normalise every table key to
/// `snake_case`. Idempotent — running twice yields the same result.
///
/// Historically the engine's serde structs (`InstanceDefinition`,
/// `GuiTomlFile`, `ServiceDefinition`, etc.) all use snake_case field
/// names. When the class-schema self-heal briefly rewrote on-disk TOMLs
/// to PascalCase it broke every downstream parser that hadn't been
/// migrated, so the canonical in-memory form is snake_case again and
/// this helper exists only to ACCEPT PascalCase inputs gracefully —
/// `ClassName` → `class_name`, `CanCollide` → `can_collide` — without
/// forcing every parser to care.
///
/// Not idempotent against already-CamelCased keys that cross underscore
/// boundaries — it treats `ABC_DEF` → `a_b_c_d_e_f` per capital letter.
/// That's fine because we only run it against keys that were either
/// authored snake_case (no-op) or emitted PascalCase by the earlier
/// aborted migration (safe input).
pub fn normalise_keys(value: &mut toml::Value) {
    match value {
        toml::Value::Table(table) => {
            let entries: Vec<(String, toml::Value)> = table
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            table.clear();
            for (key, mut v) in entries {
                let canonical = pascal_to_snake(&key);
                if !holds_authored_keys(&canonical) {
                    normalise_keys(&mut v);
                }
                // Two spellings of one key (`[Metadata]` and `[metadata]`)
                // become one, never one replacing the other.
                let spelled_canonically = key == canonical;
                match table.get_mut(&canonical) {
                    Some(existing) => merge_spellings(existing, v, spelled_canonically),
                    None => {
                        table.insert(canonical, v);
                    }
                }
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                normalise_keys(item);
            }
        }
        _ => {}
    }
}

/// Fold a second spelling of one key into the first. Tables merge key by
/// key; on any other clash the incoming value replaces the one already there
/// only when it was spelled canonically, so `color` wins over a `Color` that
/// was converted to it.
fn merge_spellings(existing: &mut toml::Value, incoming: toml::Value, incoming_wins: bool) {
    match (existing, incoming) {
        (toml::Value::Table(into), toml::Value::Table(from)) => {
            for (key, value) in from {
                match into.get_mut(&key) {
                    Some(slot) => merge_spellings(slot, value, incoming_wins),
                    None => {
                        into.insert(key, value);
                    }
                }
            }
        }
        (slot, incoming) => {
            if incoming_wins {
                *slot = incoming;
            }
        }
    }
}

/// A table whose keys are data, not schema fields: an instance's attributes,
/// parameters and parameter bindings are keyed by names their author chose
/// (`MaxHealth`, `hvac.SpawnRate`), and a tagged attribute's one key names its
/// type (`{ Color3 = [...] }`). They keep every key as written.
fn holds_authored_keys(key: &str) -> bool {
    matches!(key, "attributes" | "parameters" | "parameter_bindings")
}

/// Convert `PascalCase` / `camelCase` → `snake_case`. Already-snake_case
/// inputs pass through unchanged.
///
/// Examples: `ClassName` → `class_name`, `CanCollide` → `can_collide`,
/// `sizeOffset` → `size_offset`, `rgba` → `rgba`, `class_name` →
/// `class_name`. An uppercase letter preceded by a lowercase letter or
/// digit gets an underscore prepended; consecutive uppercase runs
/// (`RGB`, `UUID`) stay fused unless followed by lowercase (`RGBA_Color`
/// → `rgba_color`; `UUIDv2` → `uui_dv2` … acceptable edge case — no
/// engine TOML schema key hits it).
pub fn pascal_to_snake(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = key.chars().collect();
    let mut out = String::with_capacity(key.len() + 4);
    for (i, ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            // Insert underscore only when a boundary truly starts:
            // - previous char was lowercase or digit
            // - next char is lowercase (handles `ABCDef` → `abc_def`)
            let prev_lower_or_digit = i > 0
                && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase();
            if i > 0 && (prev_lower_or_digit || next_lower) && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(ch.to_ascii_lowercase().to_string().chars());
        } else {
            out.push(*ch);
        }
    }
    out
}

/// Legacy name retained for callers that imported it. Forwards to
/// [`pascal_to_snake`] now that the canonical direction has flipped.
#[deprecated(note = "use `pascal_to_snake` — canonical direction is snake_case")]
pub fn snake_to_pascal(key: &str) -> String {
    pascal_to_snake(key)
}

/// Case-tolerant `toml::Value::get` for TOML files that may be in either
/// snake_case (historic) or PascalCase (post-aborted-migration). Tries
/// the given snake_case key first, then the PascalCase equivalent.
///
/// Intended for ad-hoc `toml::Value` reads in places that don't run the
/// file through [`normalise_keys`] first (ServiceDefinition loader,
/// `attribute_tag_migration`, `file_loader` pre-checks, Rune FFI TOML
/// reads). New code should prefer passing through
/// [`load_and_heal_instance`] or `normalise_keys` and using plain
/// snake_case lookups — this helper is the transitional adapter.
pub fn get_section_insensitive<'a>(
    value: &'a toml::Value,
    snake_key: &str,
) -> Option<&'a toml::Value> {
    value.get(snake_key).or_else(|| {
        // Compute PascalCase equivalent once per call. Keys in the
        // engine are 6-20 chars so the allocation is negligible; if
        // hotter call sites appear we can switch to a
        // `SmallVec<[u8; 32]>` buffer.
        let mut pascal = String::with_capacity(snake_key.len());
        let mut capitalize_next = true;
        for ch in snake_key.chars() {
            if ch == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                pascal.extend(ch.to_uppercase());
                capitalize_next = false;
            } else {
                pascal.push(ch);
            }
        }
        value.get(&pascal)
    })
}

// ============================================================================
// Merge + self-heal
// ============================================================================

/// Deep-merge missing keys from `template` into `target`. User-set values are
/// never overwritten — only absent keys get filled from the template. Tables
/// recurse; scalars stop at the first level.
pub fn merge_template_into(target: &mut toml::Value, template: &toml::Value) {
    merge_into(target, template, false);
}

/// [`merge_template_into`], knowing whether `target` is an author's table
/// ([`holds_authored_keys`]). There a default counts as present when the
/// author spelled its name in any case, with or without underscores, so it
/// never lands beside the author's own spelling (`Intensity` beside
/// `intensity`).
fn merge_into(target: &mut toml::Value, template: &toml::Value, authored: bool) {
    let (Some(target_table), Some(template_table)) =
        (target.as_table_mut(), template.as_table())
    else {
        return;
    };
    for (key, template_value) in template_table {
        if let Some(existing) = target_table.get_mut(key) {
            if existing.is_table() && template_value.is_table() {
                merge_into(existing, template_value, authored || holds_authored_keys(key));
            }
            continue;
        }
        if authored && target_table.keys().any(|k| same_name(k, key)) {
            continue;
        }
        target_table.insert(key.clone(), template_value.clone());
    }
}

/// Two names that differ only in case or underscores.
fn same_name(a: &str, b: &str) -> bool {
    let flat = |s: &str| s.replace('_', "").to_ascii_lowercase();
    flat(a) == flat(b)
}

/// Move `[metadata] tags` to the document's root `tags`, where every reader
/// takes them, when the root has no `tags` key. The Roblox importer wrote an
/// instance's CollectionService tags under `[metadata]` until 2026-09-23. The
/// typed definition has no field there, so the loader never saw them and a
/// typed write-back (moving the part, editing a property) dropped them. A
/// root key that is present wins, even an empty one, as in
/// `datamodel::record::record_tags`. True when anything moved.
pub fn migrate_legacy_metadata_tags(doc: &mut toml::Value) -> bool {
    let Some(root) = doc.as_table_mut() else { return false };
    if root.contains_key("tags") {
        return false;
    }
    let is_list = root
        .get("metadata")
        .and_then(|m| m.get("tags"))
        .is_some_and(|t| t.is_array());
    if !is_list {
        return false;
    }
    let Some(tags) = root
        .get_mut("metadata")
        .and_then(|m| m.as_table_mut())
        .and_then(|m| m.remove("tags"))
    else {
        return false;
    };
    root.insert("tags".to_string(), tags);
    true
}

/// Move an imported light's values from the Roblox importer's old
/// `[properties.extras]` `light_*` keys into its `[light]` section, and drop
/// the keys. Those keys predate the section and hold Roblox's units (the
/// importer's old ×800 lumens, divided back out here, and lengths in studs,
/// converted to metres), so they overwrite
/// whatever `[light]` holds: an earlier heal filled it with the class
/// template's defaults, not with anything authored. True when anything moved.
///
/// Once moved, the section is the only copy, which a Properties edit updates
/// and every writer (typed or raw) preserves; the extras were dropped by the
/// typed tool writers, so dragging an imported light lost its values.
pub fn migrate_legacy_light_extras(doc: &mut toml::Value) -> bool {
    use crate::plugins::light_classes::{
        legacy_studs_to_m, normalize_face, LEGACY_EXTRAS_BRIGHTNESS, LEGACY_LIGHT_EXTRAS,
    };
    let class = doc
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|c| c.as_str());
    if !matches!(class, Some("PointLight" | "SpotLight" | "SurfaceLight")) {
        return false;
    }
    let Some(extras) = doc
        .get("properties")
        .and_then(|p| p.get("extras"))
        .and_then(|e| e.as_table())
    else {
        return false;
    };
    let mut moved = toml::value::Table::new();
    for (old, new) in LEGACY_LIGHT_EXTRAS {
        let Some(v) = extras.get(old) else { continue };
        let value = match new {
            "brightness" => v
                .as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .map(|b| toml::Value::Float(b / LEGACY_EXTRAS_BRIGHTNESS as f64)),
            "range" | "radius" => v
                .as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .map(|studs| toml::Value::Float(legacy_studs_to_m(studs))),
            "face" => match v {
                toml::Value::String(s) => Some(toml::Value::String(normalize_face(s).to_string())),
                toml::Value::Integer(i) => {
                    Some(toml::Value::String(normalize_face(&i.to_string()).to_string()))
                }
                _ => None,
            },
            _ => Some(v.clone()),
        };
        if let Some(value) = value {
            moved.insert(new.to_string(), value);
        }
    }
    let Some(root) = doc.as_table_mut() else { return false };
    if !moved.is_empty() {
        let light = root
            .entry("light".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(light) = light.as_table_mut() {
            for (k, v) in moved {
                light.insert(k, v);
            }
        }
    }
    crate::plugins::light_classes::remove_legacy_light_extras(doc)
}

/// Result of a single `load_and_heal_instance` call.
#[derive(Debug, Clone)]
pub struct HealResult {
    /// The merged, key-normalised TOML value ready for deserialization.
    pub value: toml::Value,
    /// Sections that were NOT defined in the class template — candidates
    /// for `ExtraSectionRegistry::claim` at spawn time. Ordered for a
    /// deterministic traversal (BTreeMap-backed).
    pub extras: Vec<String>,
    /// True when the on-disk TOML was rewritten because the merged /
    /// normalised form differed from the bytes we read. Saves a redundant
    /// write when the file was already schema-valid.
    pub rewrote_disk: bool,
}

/// Read `path`, merge in the class template, normalise keys, and — if the
/// result differs from what's on disk — write the corrected TOML back. The
/// returned `value` is ready to hand to `InstanceDefinition::deserialize`.
///
/// `class_name` is resolved from the parsed TOML's `[Metadata].ClassName`
/// (or legacy `class_name`) before merging; passing `None` means "no
/// template merge, just normalise keys and compare round-trip."
pub fn load_and_heal_instance(
    path: &std::path::Path,
    registry: &ClassSchemaRegistry,
) -> Result<HealResult, String> {
    let original = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let mut parsed: toml::Value = original
        .parse()
        .map_err(|e: toml::de::Error| format!("parse {}: {}", path.display(), e))?;

    // Normalise every key once — any legacy snake_case field is now
    // PascalCase. Templates are PascalCase so the merge key-compare below
    // works without fuzzy matching.
    normalise_keys(&mut parsed);
    // Before the template merge, so an imported light's real values land in
    // `[light]` rather than the template's defaults.
    migrate_legacy_light_extras(&mut parsed);
    migrate_legacy_metadata_tags(&mut parsed);
    // A Sun, Moon, Sky or Atmosphere from the old Lighting templates: their
    // descriptor sections were never read (see `celestial_sections`).
    crate::plugins::celestial_sections::drop_legacy_template_sections(&mut parsed);

    // After key normalisation every TOML is snake_case in memory,
    // regardless of on-disk case.
    let class_name: Option<String> = parsed
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Merge template in if known.
    if let Some(ref name) = class_name {
        if let Some(template) = registry.template(name) {
            merge_template_into(&mut parsed, &template.body);
        }
    }

    // Compute extras — sections present in `parsed` that the template
    // doesn't know about. Non-sections (top-level scalars) are ignored;
    // only table-shaped entries qualify.
    let template_sections: std::collections::HashSet<String> = class_name
        .as_deref()
        .and_then(|n| registry.template(n))
        .and_then(|t| t.body.as_table())
        .map(|t| t.keys().cloned().collect())
        .unwrap_or_default();
    let extras: Vec<String> = parsed
        .as_table()
        .map(|t| {
            t.iter()
                .filter(|(_, v)| v.is_table())
                .map(|(k, _)| k.clone())
                .filter(|k| !template_sections.contains(k))
                .collect()
        })
        .unwrap_or_default();

    // Self-heal ENABLED: the parser audit landed (file_loader pre-
    // check, gui_loader, service_loader, attribute_tag_migration,
    // rune_ecs_module, InstanceDefinition) — every consumer either
    // routes through this module or uses the
    // [`get_section_insensitive`] helper. Rewriting a PascalCase
    // file back to snake_case is now safe because every parser
    // either normalises on read or tolerates both cases.
    //
    // Serialize the merged + normalised value back to TOML text. If
    // the canonical form differs from the bytes on disk, rewrite.
    // Template defaults that were missing from the user's file get
    // baked in permanently so subsequent loads converge on the
    // schema.
    // A file naming a class the engine does not know loads degraded
    // (`datamodel::record::class_from_toml`), so it is never rewritten: it
    // stays exactly as its author wrote it.
    let unknown_class = class_name
        .as_deref()
        .is_some_and(|name| !crate::datamodel::record::is_known_class(name));
    let canonical = toml::to_string_pretty(&parsed)
        .map_err(|e| format!("reserialize {}: {}", path.display(), e))?;
    let rewrote_disk = !unknown_class && canonical.trim() != original.trim();
    if rewrote_disk {
        if let Err(e) = std::fs::write(path, &canonical) {
            // Log but don't fail the load — the in-memory value is
            // still correct even when disk is read-only / locked.
            tracing::warn!(
                "class_schema: self-heal write failed for {}: {}",
                path.display(),
                e
            );
        } else {
            tracing::info!(
                "class_schema: self-healed {} ({})",
                path.display(),
                class_name.as_deref().unwrap_or("<unknown class>")
            );
        }
    }

    Ok(HealResult {
        value: parsed,
        extras,
        rewrote_disk,
    })
}

/// In-memory twin of [`load_and_heal_instance`] for callers that hold
/// the TOML *content* rather than a path — primarily the WorldDb
/// cold-load path, which replays `INSTANCE_META` bytes out of Fjall.
///
/// Identical key-normalise + template-merge + extras computation as
/// the path-based function, but performs **no disk IO**: it never
/// reads a file and never self-heals (`rewrote_disk` is always
/// `false`). The canonicalisation step is therefore skipped entirely
/// — the caller owns persistence (it writes back to Fjall, not to a
/// `_instance.toml`).
///
/// Keep this in lockstep with [`load_and_heal_instance`]'s in-memory
/// stages — the two are intentionally duplicated rather than sharing
/// an extracted core, because the path-based function is on the hot
/// legacy load path and a shared-core refactor would change its call
/// graph across every existing caller (engine, MCP server,
/// attribute-tag migration). Foundation-first: add alongside, don't
/// rewire.
pub fn heal_instance_from_str(
    content: &str,
    registry: &ClassSchemaRegistry,
) -> Result<HealResult, String> {
    let parsed: toml::Value = content
        .parse()
        .map_err(|e: toml::de::Error| format!("parse instance bytes: {}", e))?;
    heal_instance_value(parsed, registry)
}

/// The same heal for a document that is already parsed. The bulk loader
/// parses each `_instance.toml` once on the worker pool and hands the value
/// here, so the typed definition costs no second parse.
pub fn heal_instance_value(
    mut parsed: toml::Value,
    registry: &ClassSchemaRegistry,
) -> Result<HealResult, String> {
    normalise_keys(&mut parsed);
    migrate_legacy_light_extras(&mut parsed);
    migrate_legacy_metadata_tags(&mut parsed);
    crate::plugins::celestial_sections::drop_legacy_template_sections(&mut parsed);

    let class_name: Option<String> = parsed
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(ref name) = class_name {
        if let Some(template) = registry.template(name) {
            merge_template_into(&mut parsed, &template.body);
        }
    }

    let template_sections: std::collections::HashSet<String> = class_name
        .as_deref()
        .and_then(|n| registry.template(n))
        .and_then(|t| t.body.as_table())
        .map(|t| t.keys().cloned().collect())
        .unwrap_or_default();
    let extras: Vec<String> = parsed
        .as_table()
        .map(|t| {
            t.iter()
                .filter(|(_, v)| v.is_table())
                .map(|(k, _)| k.clone())
                .filter(|k| !template_sections.contains(k))
                .collect()
        })
        .unwrap_or_default();

    Ok(HealResult {
        value: parsed,
        extras,
        rewrote_disk: false,
    })
}

// ============================================================================
// Extra-section claim registry
// ============================================================================

/// Result of a single claim attempt by an `ExtraSectionClaim` implementor.
#[derive(Debug, Clone)]
pub enum ClaimResult {
    /// The claimant recognised the section and attached whatever ECS
    /// components / resources it needed. The loader treats the section as
    /// handled and does NOT warn.
    Claimed,
    /// The section's name looked familiar but its contents don't match a
    /// supported schema — a warning is emitted so the author can fix it.
    Invalid(String),
    /// The claimant doesn't own this section — loader continues trying
    /// other claimants.
    NotMine,
}

/// Anything that can consume one or more `[section]`s inside an instance
/// TOML that aren't part of the base class template. Registered into
/// [`ExtraSectionRegistry`] at plugin build time.
///
/// Intended impls (one per simulation facet beyond what
/// `InstanceDefinition` already consumes as typed fields):
/// * `Fluid` / `Plasma` / `Biology` — third-party sections that attach
///   simulation plugin components.
/// * Custom metadata layers that author-time tools want to round-trip
///   without the engine understanding them.
///
/// Unclaimed sections are preserved on disk via `InstanceDefinition.extra`
/// flatten, so a future claimant picks them up next load — the system is
/// strictly additive.
pub trait ExtraSectionClaim: Send + Sync + 'static {
    /// Section names this claimant will accept.
    fn section_names(&self) -> &'static [&'static str];

    /// Called once per matching `[section]` found in an instance TOML.
    /// Attach ECS components via `commands.entity(entity).insert(…)`.
    fn claim(
        &self,
        section_name: &str,
        section_value: &toml::Value,
        entity: bevy::ecs::entity::Entity,
        commands: &mut bevy::ecs::system::Commands<'_, '_>,
    ) -> ClaimResult;
}

/// Component attached by `spawn_instance` when a TOML carries
/// `[section]`s that `InstanceDefinition` doesn't type-consume. A
/// deferred Bevy system (`dispatch_pending_extras` below) drains the
/// map by calling [`ExtraSectionRegistry::dispatch`] for each entry,
/// attaching plugin-specific components as they claim them.
///
/// Decoupling the claim-dispatch from `spawn_instance` keeps the
/// latter a plain `fn` (no ECS Resource plumbing through every one of
/// its nine callers) while still letting plugins react to fresh
/// extras on the exact frame they spawn.
#[derive(bevy::prelude::Component, Debug, Clone, Default)]
pub struct PendingExtraSections {
    pub sections: std::collections::HashMap<String, toml::Value>,
}

/// Bevy `Resource` holding every claimant. Plugins push their impls into it
/// at `App::build`; `dispatch_pending_extras` drains it per-entity.
#[derive(bevy::prelude::Resource, Default)]
pub struct ExtraSectionRegistry {
    claimants: Vec<Box<dyn ExtraSectionClaim>>,
}

impl std::fmt::Debug for ExtraSectionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtraSectionRegistry")
            .field("claimants", &self.claimants.len())
            .finish()
    }
}

impl ExtraSectionRegistry {
    pub fn register<C: ExtraSectionClaim>(&mut self, claimant: C) {
        self.claimants.push(Box::new(claimant));
    }

    /// Register every first-party claimant in one call. Invoke from
    /// the engine's plugin `build` after `init_resource::<ExtraSectionRegistry>()`.
    /// Third-party claimants can still `register` additionally — the
    /// dispatcher walks claimants in insertion order.
    pub fn register_builtins(&mut self) {
        self.register(claims::ThermodynamicClaim);
    }

    /// Try each registered claimant in turn. First one to return `Claimed`
    /// wins; if none match, preserves the section on save via the
    /// generic `extra` flatten field so a later-registered plugin
    /// picks it up next load.
    pub fn dispatch(
        &self,
        section_name: &str,
        section_value: &toml::Value,
        entity: bevy::ecs::entity::Entity,
        commands: &mut bevy::ecs::system::Commands<'_, '_>,
    ) -> ClaimResult {
        for claimant in &self.claimants {
            if claimant
                .section_names()
                .iter()
                .any(|n| *n == section_name)
            {
                match claimant.claim(section_name, section_value, entity, commands) {
                    ClaimResult::NotMine => continue,
                    other => return other,
                }
            }
        }
        ClaimResult::NotMine
    }
}

/// Bevy system that drains every `PendingExtraSections` component on
/// newly-spawned entities and routes each section through the
/// `ExtraSectionRegistry`. Plugins that register claimants at startup
/// get called here with the section's TOML body + a `ClaimContext`
/// they can use to attach ECS components.
///
/// Runs on `Update`; entities with no unclaimed sections lose the
/// marker component after one pass.
pub fn dispatch_pending_extras(
    mut commands: bevy::prelude::Commands,
    registry: Option<bevy::prelude::Res<ExtraSectionRegistry>>,
    pending: bevy::prelude::Query<
        (bevy::prelude::Entity, &PendingExtraSections),
        bevy::prelude::Added<PendingExtraSections>,
    >,
) {
    let Some(registry) = registry else { return };
    // Snapshot (entity, sections) pairs outside the command loop so
    // each claim.call doesn't re-enter the `pending` query while it
    // holds a borrow; keeps Bevy's Commands API happy.
    let jobs: Vec<(bevy::prelude::Entity, Vec<(String, toml::Value)>)> = pending
        .iter()
        .map(|(e, p)| {
            let sections = p
                .sections
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (e, sections)
        })
        .collect();

    for (entity, sections) in jobs {
        for (name, value) in &sections {
            match registry.dispatch(name, value, entity, &mut commands) {
                ClaimResult::Claimed => {}
                ClaimResult::Invalid(reason) => {
                    tracing::warn!(
                        "extra section [{}] on entity {:?} rejected: {}",
                        name, entity, reason
                    );
                }
                ClaimResult::NotMine => {
                    tracing::debug!(
                        "extra section [{}] on entity {:?} has no claimant",
                        name, entity
                    );
                }
            }
        }
        // Drop the marker so the system is one-shot per entity.
        // Unclaimed sections remain in the entity's in-memory
        // `InstanceDefinition.extra` HashMap + on-disk TOML; the next
        // plugin to register a claimant picks them up next load.
        commands.entity(entity).remove::<PendingExtraSections>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;

    #[test]
    fn metadata_tags_move_to_the_root_only_when_the_root_has_none() {
        let mut legacy: toml::Value =
            "[metadata]\nclass_name = \"Part\"\ntags = [\"car\", \"sfx\"]\n".parse().unwrap();
        assert!(migrate_legacy_metadata_tags(&mut legacy));
        let root: Vec<&str> = legacy["tags"].as_array().unwrap().iter().filter_map(|t| t.as_str()).collect();
        assert_eq!(root, vec!["car", "sfx"]);
        assert!(legacy["metadata"].get("tags").is_none(), "one copy, at the root");

        // A root key that is present wins, even an empty one.
        let mut both: toml::Value =
            "tags = []\n[metadata]\ntags = [\"stale\"]\n".parse().unwrap();
        assert!(!migrate_legacy_metadata_tags(&mut both));
        assert!(both["tags"].as_array().unwrap().is_empty());
    }

    #[test]
    fn snakifies_pascal_case_keys() {
        assert_eq!(pascal_to_snake("ClassName"), "class_name");
        assert_eq!(pascal_to_snake("CanCollide"), "can_collide");
        assert_eq!(pascal_to_snake("SizeOffset"), "size_offset");
        assert_eq!(pascal_to_snake("Rgba"), "rgba");
    }

    #[test]
    fn leaves_snake_case_keys_alone() {
        assert_eq!(pascal_to_snake("class_name"), "class_name");
        assert_eq!(pascal_to_snake("size_offset"), "size_offset");
    }

    #[test]
    fn normalise_keys_converts_pascal_to_snake() {
        let mut v: toml::Value = r#"
[Metadata]
ClassName = "Part"

[Properties]
CanCollide = true
Color = [163, 162, 165]
"#
        .parse()
        .unwrap();
        normalise_keys(&mut v);
        let t = v.as_table().unwrap();
        assert!(t.contains_key("metadata"));
        assert!(t.contains_key("properties"));
        assert!(t["metadata"].as_table().unwrap().contains_key("class_name"));
        assert!(t["properties"].as_table().unwrap().contains_key("can_collide"));
    }

    #[test]
    fn normalise_keys_keeps_attribute_and_parameter_names() {
        let mut v: toml::Value = r#"
[Attributes]
MaxHealth = 100
Tint = { Color3 = [1.0, 0.5, 0.0] }

[Parameters]
SpawnRate = 2.5

[parameter_bindings."hvac.SpawnRate"]
connector = "Rates"

[Properties]
CanCollide = true
"#
        .parse()
        .unwrap();
        normalise_keys(&mut v);
        let t = v.as_table().unwrap();
        let attributes = t["attributes"].as_table().unwrap();
        assert!(attributes.contains_key("MaxHealth"), "{attributes:?}");
        assert!(attributes["Tint"].as_table().unwrap().contains_key("Color3"));
        assert!(t["parameters"].as_table().unwrap().contains_key("SpawnRate"));
        assert!(t["parameter_bindings"].as_table().unwrap().contains_key("hvac.SpawnRate"));
        assert!(t["properties"].as_table().unwrap().contains_key("can_collide"));
    }

    #[test]
    fn merge_fills_missing_sections() {
        let mut target: toml::Value = r#"
[metadata]
class_name = "Part"
"#
        .parse()
        .unwrap();
        let template: toml::Value = r#"
[metadata]
class_name = "Part"
archivable = true

[properties]
anchored = false
can_collide = true
"#
        .parse()
        .unwrap();
        merge_template_into(&mut target, &template);
        let t = target.as_table().unwrap();
        assert!(t.contains_key("properties"));
        assert_eq!(
            t["metadata"]["archivable"].as_bool(),
            Some(true)
        );
        // User-set values must NOT be clobbered
        assert_eq!(t["metadata"]["class_name"].as_str(), Some("Part"));
    }

    // ------------------------------------------------------------------
    // Integration tests — the full schema contract end-to-end.
    // ------------------------------------------------------------------
    //
    // These guard against the exact regressions we hit during the
    // PascalCase migration misadventure: half-specified user TOMLs
    // drifting, self-heal breaking round-trip idempotency, and extras
    // bypassing the claim pipeline. Every future schema change should
    // keep these green.

    /// A Part template baked in at build time must merge a half-
    /// specified user TOML into the full canonical form, and a second
    /// merge of the same user TOML against the same template must
    /// produce identical bytes — i.e. the self-heal converges in one
    /// pass and is idempotent from there.
    #[test]
    fn part_template_round_trips_half_specified_toml() {
        let registry = ClassSchemaRegistry::from_builtin();
        let template = registry
            .template("Part")
            .expect("Part template must be registered via BUILTIN_TEMPLATES");

        // User-authored: only [metadata] and a user-chosen transform.
        // The template fills in [properties] and the rest of the
        // transform fields.
        let user_toml = r#"
[metadata]
class_name = "Part"

[transform]
position = [5.0, 10.0, 0.0]
"#;
        let mut first_pass: toml::Value = user_toml.parse().unwrap();
        normalise_keys(&mut first_pass);
        merge_template_into(&mut first_pass, &template.body);

        let canonical = toml::to_string_pretty(&first_pass)
            .expect("merged value must serialize");

        // Second pass: feed the canonical output back through the
        // same pipeline. A correct self-heal is idempotent — one
        // rewrite brings the file to the schema, then every
        // subsequent load is a no-op.
        let mut second_pass: toml::Value = canonical.parse().unwrap();
        normalise_keys(&mut second_pass);
        merge_template_into(&mut second_pass, &template.body);
        let canonical_2 = toml::to_string_pretty(&second_pass)
            .expect("second merge must serialize");

        assert_eq!(
            canonical, canonical_2,
            "self-heal must be idempotent — one pass converged to the schema, \
             second pass produced different bytes:\n---PASS 1---\n{}\n---PASS 2---\n{}",
            canonical, canonical_2
        );

        // Verify the merge actually filled the template defaults
        // (guards against "idempotent but empty" false-positives).
        let t = first_pass.as_table().unwrap();
        assert!(t.contains_key("properties"), "properties filled from template");
        assert_eq!(
            t["properties"]["anchored"].as_bool(),
            Some(false),
            "properties.anchored default came from template"
        );
        assert_eq!(
            t["properties"]["can_collide"].as_bool(),
            Some(true),
            "properties.can_collide default came from template"
        );
        assert_eq!(
            t["metadata"]["archivable"].as_bool(),
            Some(true),
            "metadata.archivable default came from template"
        );
        // User-authored value survived the merge.
        assert_eq!(
            t["transform"]["position"][0].as_float(),
            Some(5.0),
            "user transform.position survived"
        );
    }

    /// `load_and_heal_instance` on a real file must emit the same
    /// canonical TOML whether the user's file is snake_case or the
    /// legacy PascalCase. Both must converge to snake_case on disk.
    #[test]
    fn load_and_heal_converges_mixed_case_to_snake() {
        let registry = ClassSchemaRegistry::from_builtin();
        let tmp = std::env::temp_dir().join(format!(
            "eustress_class_schema_heal_{}.toml",
            std::process::id()
        ));

        // Write a PascalCase-flavoured TOML (the aborted-migration
        // format). Load-and-heal must rewrite it to snake_case and
        // fill template defaults.
        std::fs::write(
            &tmp,
            r#"[Metadata]
ClassName = "Part"

[Transform]
Position = [1.0, 2.0, 3.0]
"#,
        )
        .unwrap();

        let result = load_and_heal_instance(&tmp, &registry)
            .expect("heal must succeed");
        assert!(result.rewrote_disk, "PascalCase → snake_case must rewrite disk");

        let healed = std::fs::read_to_string(&tmp).unwrap();
        assert!(
            healed.contains("class_name = \"Part\""),
            "on-disk form must be snake_case after heal, got:\n{}",
            healed
        );
        assert!(
            healed.contains("[properties]"),
            "template defaults must be filled on disk, got:\n{}",
            healed
        );

        // Second load is a no-op — idempotent once converged.
        let result2 = load_and_heal_instance(&tmp, &registry).unwrap();
        assert!(
            !result2.rewrote_disk,
            "second load on converged file must NOT rewrite disk"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// The heal rewrites a file's schema keys, never its attributes or
    /// parameters: on disk and in memory they stay exactly as written.
    #[test]
    fn load_and_heal_keeps_attribute_names_and_tags_on_disk() {
        let registry = ClassSchemaRegistry::from_builtin();
        let tmp = std::env::temp_dir().join(format!("eustress_class_schema_attrs_{}.toml", std::process::id()));
        std::fs::write(
            &tmp,
            r#"[Metadata]
ClassName = "Part"

[attributes]
MaxHealth = 100
tint = { Color3 = [1.0, 0.5, 0.0] }

[parameters]
SpawnRate = 2.5
"#,
        )
        .unwrap();
        let written: toml::Value = std::fs::read_to_string(&tmp).unwrap().parse().unwrap();

        let result = load_and_heal_instance(&tmp, &registry).expect("heal must succeed");
        let healed: toml::Value = std::fs::read_to_string(&tmp).unwrap().parse().unwrap();
        let _ = std::fs::remove_file(&tmp);

        assert!(result.rewrote_disk, "the PascalCase metadata makes the heal write the file");
        assert_eq!(healed.get("attributes"), written.get("attributes"), "attributes on disk");
        assert_eq!(healed.get("parameters"), written.get("parameters"), "parameters on disk");
        assert_eq!(result.value.get("attributes"), written.get("attributes"), "attributes in memory");
    }

    #[test]
    fn merge_never_adds_a_default_beside_an_authors_spelling() {
        let mut target: toml::Value = "[attributes]\nintensity = 5.0\n".parse().unwrap();
        let template: toml::Value = "[attributes]\nIntensity = 1000.0\nFalloff = 0.25\n".parse().unwrap();
        merge_template_into(&mut target, &template);
        let attributes = target["attributes"].as_table().unwrap();
        assert_eq!(attributes.get("intensity").and_then(|v| v.as_float()), Some(5.0));
        assert!(!attributes.contains_key("Intensity"), "{attributes:?}");
        assert_eq!(attributes.get("Falloff").and_then(|v| v.as_float()), Some(0.25), "a missing default is filled");
    }

    #[test]
    fn normalise_keys_merges_two_spellings_of_one_table() {
        let mut v: toml::Value =
            "[Metadata]\nClassName = \"Part\"\nName = \"Old\"\n\n[metadata]\nuuid = \"u1\"\nname = \"New\"\n"
                .parse()
                .unwrap();
        normalise_keys(&mut v);
        let metadata = v["metadata"].as_table().unwrap();
        assert_eq!(metadata.get("class_name").and_then(|c| c.as_str()), Some("Part"), "{metadata:?}");
        assert_eq!(metadata.get("uuid").and_then(|c| c.as_str()), Some("u1"));
        assert_eq!(metadata.get("name").and_then(|c| c.as_str()), Some("New"), "the canonical spelling wins");
    }

    #[test]
    fn load_and_heal_never_rewrites_a_file_of_an_unknown_class() {
        let registry = ClassSchemaRegistry::from_builtin();
        let tmp = std::env::temp_dir().join(format!("eustress_class_schema_unknown_{}.toml", std::process::id()));
        let text = "[Metadata]\nClassName = \"Gizmo9000\"\n\n[Config]\nMaxSpeed = 3\n";
        std::fs::write(&tmp, text).unwrap();
        let result = load_and_heal_instance(&tmp, &registry).expect("heal must succeed");
        let after = std::fs::read_to_string(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert!(!result.rewrote_disk);
        assert_eq!(after, text, "the file stays as written");
    }

    /// End-to-end dispatcher test — construct a real `bevy::App`,
    /// spawn an entity with `PendingExtraSections` carrying a
    /// `[thermodynamic]` body, run one tick, assert the
    /// `ThermodynamicClaim` parsed the body and attached a
    /// `ThermodynamicState` component.
    ///
    /// This is the proof that the plugin-extensibility path is wired
    /// correctly: section TOML → extras map → pending component →
    /// dispatch system → registered claim → ECS component on the
    /// entity. A third-party plugin registering its own claim follows
    /// the exact same path; this test is the canary that it still
    /// works.
    #[test]
    fn thermodynamic_claim_attaches_component_via_dispatcher() {
        let mut app = App::new();
        app.init_resource::<ExtraSectionRegistry>();
        app.world_mut()
            .resource_mut::<ExtraSectionRegistry>()
            .register_builtins();
        app.add_systems(Update, dispatch_pending_extras);

        // Build a synthetic [thermodynamic] section — the kind of
        // payload a plugin would hand the dispatcher directly, or
        // that would end up in `InstanceDefinition.extra` if the
        // typed field were removed.
        let section: toml::Value = r#"
temperature = 350.0
pressure = 200000.0
moles = 2.0
"#
        .parse()
        .unwrap();

        let mut sections = std::collections::HashMap::new();
        sections.insert("thermodynamic".to_string(), section);

        let entity = app
            .world_mut()
            .spawn(PendingExtraSections { sections })
            .id();

        app.update();

        // The claim must have attached the component and the
        // dispatcher must have cleared the pending marker.
        let world = app.world();
        let state = world
            .get::<crate::realism::particles::prelude::ThermodynamicState>(entity)
            .expect("ThermodynamicState must be inserted by the claim");
        assert!(
            (state.temperature - 350.0).abs() < 1e-3,
            "temperature round-trip: got {}, expected 350.0",
            state.temperature
        );
        assert!(
            (state.pressure - 200_000.0).abs() < 1e-3,
            "pressure round-trip: got {}, expected 200000.0",
            state.pressure
        );
        assert!(
            (state.moles - 2.0).abs() < 1e-3,
            "moles round-trip: got {}, expected 2.0",
            state.moles
        );

        assert!(
            world.get::<PendingExtraSections>(entity).is_none(),
            "dispatch must remove PendingExtraSections after claiming"
        );
    }

    /// The claim must reject a non-table `[thermodynamic]` section
    /// (author error) without panicking or silently attaching a
    /// garbage component. The dispatcher should log a warning and
    /// move on.
    #[test]
    fn thermodynamic_claim_rejects_non_table_body() {
        let mut app = App::new();
        app.init_resource::<ExtraSectionRegistry>();
        app.world_mut()
            .resource_mut::<ExtraSectionRegistry>()
            .register_builtins();
        app.add_systems(Update, dispatch_pending_extras);

        let mut sections = std::collections::HashMap::new();
        sections.insert(
            "thermodynamic".to_string(),
            toml::Value::String("not a table".into()),
        );

        let entity = app
            .world_mut()
            .spawn(PendingExtraSections { sections })
            .id();
        app.update();

        assert!(
            app.world()
                .get::<crate::realism::particles::prelude::ThermodynamicState>(entity)
                .is_none(),
            "invalid [thermodynamic] must NOT attach a component"
        );
    }
}
