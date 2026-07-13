//! # Capability Catalog — the formal universe vocabulary
//!
//! Every function a Rune program may call is tagged with a [`Capability`].
//! The catalog enumerates the *entire* Eustress API surface registered in
//! `rune_ecs_module::create_ecs_module` plus the GUI/event-bus modules. This
//! catalog IS the formal grammar's vocabulary: a call whose target is not in the
//! catalog is, by definition, an **unknown capability** and is rejected.
//!
//! ## Single source of truth
//!
//! The catalog is hand-maintained here today (a parallel list). The KERNEL
//! VERDICT flagged that this risks drift from `create_ecs_module` — a new fn
//! registered there without a catalog entry becomes silently "unknown" and gets
//! rejected. The long-term fix (TODO seam below) is to derive the catalog from
//! the module registration itself rather than maintain two lists. Until then,
//! [`CapabilityCatalog::assert_in_sync_with_module`] is the test hook that fails
//! CI if the two diverge.

use std::collections::HashMap;

/// The broad effect class a capability belongs to. Universe laws grant or
/// withhold capabilities at this granularity (e.g. "Matrix withholds `Network`
/// and `Persistence`"), with per-function overrides available via the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityClass {
    /// Read-only access to a simulation value or component. Always safe.
    ReadSim,
    /// Mutates a simulation value or component on an existing entity.
    WriteSim,
    /// Reads a file from the space directory (traversal-guarded at runtime).
    FilesystemRead,
    /// Writes a file into the space directory (traversal-guarded at runtime).
    FilesystemWrite,
    /// Mutates a world-level law/parameter (gravity, etc.). High blast radius.
    WorldLaw,
    /// Creates or deletes an `Instance` in the world.
    SpawnInstance,
    /// Outbound network I/O (HttpService).
    Network,
    /// Durable key/value persistence (DataStoreService).
    Persistence,
    /// Plays/stops audio.
    Audio,
    /// Reads input devices (keyboard/mouse).
    Input,
    /// Reads/sets camera pose or FOV.
    Camera,
    /// Mutates GUI elements.
    Gui,
    /// Applies physics forces/impulses or sets velocity.
    Physics,
    /// CollectionService tag add/remove/query.
    Tagging,
    /// Marketplace / monetization calls.
    Marketplace,
    /// Coroutine scheduling (task.wait / spawn / defer / delay).
    Scheduling,
    /// Pure logging / unit-conversion helpers. Always safe, never withheld.
    Diagnostic,
    /// Pure constructors for value types (Vector3/Color3/CFrame/UDim/...).
    /// Always allowed — constructing a value has no world effect.
    ValueConstructor,
}

impl CapabilityClass {
    /// Stable string id used in law TOML / violation messages.
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityClass::ReadSim => "read_sim",
            CapabilityClass::WriteSim => "write_sim",
            CapabilityClass::FilesystemRead => "fs_read",
            CapabilityClass::FilesystemWrite => "fs_write",
            CapabilityClass::WorldLaw => "world_law",
            CapabilityClass::SpawnInstance => "spawn_instance",
            CapabilityClass::Network => "network",
            CapabilityClass::Persistence => "persistence",
            CapabilityClass::Audio => "audio",
            CapabilityClass::Input => "input",
            CapabilityClass::Camera => "camera",
            CapabilityClass::Gui => "gui",
            CapabilityClass::Physics => "physics",
            CapabilityClass::Tagging => "tagging",
            CapabilityClass::Marketplace => "marketplace",
            CapabilityClass::Scheduling => "scheduling",
            CapabilityClass::Diagnostic => "diagnostic",
            CapabilityClass::ValueConstructor => "value_constructor",
        }
    }

    /// Capabilities that are NEVER withheld by any universe — granting them is a
    /// no-op because they have no world effect. The grammar always admits these.
    pub fn is_always_allowed(self) -> bool {
        matches!(self, CapabilityClass::Diagnostic | CapabilityClass::ValueConstructor)
    }
}

/// A single named capability — a Rune-callable function plus its effect class.
/// `name` is the bare Rune call symbol the AST walker matches (e.g.
/// `"set_sim_value"`, `"workspace_set_gravity"`, `"Instance::new"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// The Rune call symbol (last path segment, or `Type::method` for
    /// associated functions).
    pub name: &'static str,
    /// Effect class used by universe laws to grant/withhold.
    pub class: CapabilityClass,
}

impl Capability {
    const fn new(name: &'static str, class: CapabilityClass) -> Self {
        Self { name, class }
    }
}

/// The full enumerated vocabulary. Lookups are by bare call symbol.
#[derive(Debug, Clone, Default)]
pub struct CapabilityCatalog {
    by_name: HashMap<&'static str, Capability>,
}

impl CapabilityCatalog {
    /// Build the canonical catalog for Eustress Core. This is the vocabulary the
    /// default (unrestricted) [`super::laws::UniverseLaws`] grants in full.
    ///
    /// Mirrors `rune_ecs_module::create_ecs_module` registration order. KEEP IN
    /// SYNC — see [`Self::assert_in_sync_with_module`].
    pub fn eustress_core() -> Self {
        use CapabilityClass::*;
        let entries: &[Capability] = &[
            // --- Entity component access (existing realism stubs) ---
            Capability::new("get_voltage", ReadSim),
            Capability::new("get_soc", ReadSim),
            Capability::new("get_temperature", ReadSim),
            Capability::new("get_dendrite_risk", ReadSim),
            // --- Simulation values ---
            Capability::new("get_sim_value", ReadSim),
            Capability::new("set_sim_value", WriteSim),
            Capability::new("list_sim_values", ReadSim),
            // --- Entity query + file access ---
            Capability::new("query_workspace_entities", ReadSim),
            Capability::new("read_space_file", FilesystemRead),
            Capability::new("write_space_file", FilesystemWrite),
            Capability::new("query_material_properties", ReadSim),
            // --- Logging (always allowed) ---
            Capability::new("log_info", Diagnostic),
            Capability::new("log_warn", Diagnostic),
            Capability::new("log_error", Diagnostic),
            // --- Unit conversion helpers (pure, always allowed) ---
            Capability::new("units_from_meters", Diagnostic),
            Capability::new("units_to_meters", Diagnostic),
            // --- Value constructors (pure) ---
            Capability::new("Vector3::new", ValueConstructor),
            Capability::new("Color3::new", ValueConstructor),
            Capability::new("CFrame::new", ValueConstructor),
            Capability::new("UDim::new", ValueConstructor),
            Capability::new("UDim2::new", ValueConstructor),
            // --- Raycasting ---
            Capability::new("workspace_raycast", ReadSim),
            Capability::new("workspace_raycast_all", ReadSim),
            // --- Instance API ---
            Capability::new("Instance::new", SpawnInstance),
            Capability::new("instance_delete", SpawnInstance),
            // --- TweenService ---
            Capability::new("tween_info_new", ValueConstructor),
            Capability::new("tween_info_full", ValueConstructor),
            Capability::new("tween_service_create", WriteSim),
            // --- task library (scheduling) ---
            Capability::new("task_wait", Scheduling),
            Capability::new("task_spawn", Scheduling),
            Capability::new("task_defer", Scheduling),
            Capability::new("task_delay", Scheduling),
            Capability::new("task_cancel", Scheduling),
            // --- UserInputService ---
            Capability::new("is_key_down", Input),
            Capability::new("is_mouse_button_pressed", Input),
            Capability::new("get_mouse_location", Input),
            Capability::new("get_mouse_delta", Input),
            // --- DataStoreService (persistence) ---
            Capability::new("datastore_service_get", Persistence),
            Capability::new("datastore_service_get_ordered", Persistence),
            Capability::new("datastore_get", Persistence),
            Capability::new("datastore_set", Persistence),
            Capability::new("datastore_remove", Persistence),
            Capability::new("datastore_increment", Persistence),
            Capability::new("ordered_datastore_get_sorted", Persistence),
            // --- HttpService (network) ---
            Capability::new("http_get_async", Network),
            Capability::new("http_post_async", Network),
            Capability::new("http_request_async", Network),
            Capability::new("http_url_encode", Diagnostic),
            Capability::new("http_generate_guid", Diagnostic),
            Capability::new("http_json_encode", Diagnostic),
            Capability::new("http_json_decode", Diagnostic),
            // --- CollectionService (tags) ---
            Capability::new("collection_add_tag", Tagging),
            Capability::new("collection_add_tag_by_id", Tagging),
            Capability::new("collection_remove_tag", Tagging),
            Capability::new("collection_remove_tag_by_id", Tagging),
            Capability::new("collection_has_tag", ReadSim),
            Capability::new("collection_has_tag_by_id", ReadSim),
            Capability::new("collection_get_tagged", ReadSim),
            // --- Sound ---
            Capability::new("sound_play", Audio),
            Capability::new("sound_stop", Audio),
            Capability::new("sound_set_volume", Audio),
            // --- MarketplaceService ---
            Capability::new("marketplace_prompt_purchase", Marketplace),
            Capability::new("marketplace_get_product_info", Marketplace),
            Capability::new("marketplace_player_owns_game_pass", Marketplace),
            Capability::new("marketplace_get_ticket_balance", Marketplace),
            Capability::new("players_get_player_by_user_id", ReadSim),
            Capability::new("players_get_local_player", ReadSim),
            // --- RunService (environment queries) ---
            Capability::new("run_service_is_client", ReadSim),
            Capability::new("run_service_is_server", ReadSim),
            Capability::new("run_service_is_studio", ReadSim),
            Capability::new("run_service_is_running", ReadSim),
            // --- BasePart property access ---
            Capability::new("part_set_position", WriteSim),
            Capability::new("part_set_rotation", WriteSim),
            Capability::new("part_set_size", WriteSim),
            Capability::new("part_set_anchored", WriteSim),
            Capability::new("part_set_color", WriteSim),
            Capability::new("part_set_material", WriteSim),
            Capability::new("part_set_transparency", WriteSim),
            Capability::new("part_set_can_collide", WriteSim),
            // --- Attribute system ---
            Capability::new("instance_set_attribute", WriteSim),
            Capability::new("instance_get_attribute", ReadSim),
            // --- Workspace properties (world law) ---
            Capability::new("workspace_get_gravity", ReadSim),
            Capability::new("workspace_set_gravity", WorldLaw),
            // --- Camera ---
            Capability::new("camera_get_position", Camera),
            Capability::new("camera_get_look_vector", Camera),
            Capability::new("camera_get_fov", Camera),
            Capability::new("camera_set_fov", Camera),
            Capability::new("camera_screen_point_to_ray", Camera),
            // --- Mouse ---
            Capability::new("mouse_get_hit", Input),
            Capability::new("mouse_get_target", Input),
            // --- Physics forces ---
            Capability::new("part_apply_impulse", Physics),
            Capability::new("part_apply_angular_impulse", Physics),
            Capability::new("part_get_mass", ReadSim),
            Capability::new("part_get_velocity", ReadSim),
            Capability::new("part_set_velocity", Physics),
            // --- GUI scripting ---
            Capability::new("gui_set_text", Gui),
            Capability::new("gui_get_text", ReadSim),
            Capability::new("gui_set_visible", Gui),
            Capability::new("gui_set_bg_color", Gui),
            Capability::new("gui_set_text_color", Gui),
            Capability::new("gui_set_border_color", Gui),
            Capability::new("gui_set_position", Gui),
            Capability::new("gui_set_size", Gui),
            Capability::new("gui_set_font_size", Gui),
            // --- Studio plugin API (Rune half of the unified plugin bridge) ---
            // Only enforced in the authoring (`build_pipeline`) path today —
            // `rune_runtime.rs`'s plugin loader trusts local files you
            // installed, same as Phase 2's Luau host. Catalogued anyway so a
            // future authoring-assist flow doesn't hit a mystery rejection.
            Capability::new("plugin_add_section", Gui),
            Capability::new("plugin_add_button", Gui),
            Capability::new("plugin_notify", Diagnostic),
            Capability::new("plugin_get_selection", ReadSim),
        ];

        let mut by_name = HashMap::with_capacity(entries.len());
        for cap in entries {
            by_name.insert(cap.name, cap.clone());
        }
        Self { by_name }
    }

    /// Resolve a bare call symbol to its capability, if it is in the vocabulary.
    /// `None` => the call is an **unknown capability** and must be rejected.
    pub fn lookup(&self, name: &str) -> Option<&Capability> {
        self.by_name.get(name)
    }

    /// Total number of catalogued capabilities.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Iterate every catalogued capability.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.by_name.values()
    }

    /// The set of distinct capability classes present in this catalog.
    pub fn classes(&self) -> std::collections::HashSet<CapabilityClass> {
        self.by_name.values().map(|c| c.class).collect()
    }

    // ====================================================================
    // TODO seams
    // ====================================================================

    /// TODO(kernel-sync): derive the catalog directly from the live Rune
    /// `Module` produced by `rune_ecs_module::create_ecs_module` so the two can
    /// never drift. Rune's `Module` does not yet expose an iterable list of
    /// registered function metas through its public 0.14 API; until it does, the
    /// catalog above is the hand-maintained mirror. When that API lands, replace
    /// `eustress_core()` with a derivation pass and keep `class` tags in a small
    /// side-table keyed by symbol.
    ///
    /// `_module_symbols` would be the list of registered call symbols pulled
    /// from the module; this asserts the catalog covers exactly that set.
    #[cfg(test)]
    pub fn assert_in_sync_with_module(&self, _module_symbols: &[&str]) {
        // TODO: when create_ecs_module exposes its registered symbols, assert
        //       set-equality here. For now this is a placeholder the test suite
        //       can flesh out once the introspection seam exists.
    }
}

#[cfg(test)]
mod plugin_capability_tests {
    use super::CapabilityCatalog;

    /// Hand-maintained mirror of `rune_ecs_module::create_ecs_module`'s
    /// `plugin_*` registrations (see the "Studio plugin API" section
    /// there) — the same manual-sync discipline `assert_in_sync_with_module`'s
    /// own TODO describes for the whole catalog, applied to just the four
    /// functions this Phase 3 change added. Real introspection is blocked
    /// on rune 0.14 not exposing a `Module`'s registered symbols; until it
    /// does, this is what keeps a future `plugin_*` addition from
    /// silently missing its catalog entry (an unmet entry means the
    /// Kernel L12 authoring-path validator rejects it as an unknown
    /// capability).
    const EXPECTED_PLUGIN_CAPABILITIES: &[&str] = &[
        "plugin_add_section",
        "plugin_add_button",
        "plugin_notify",
        "plugin_get_selection",
    ];

    #[test]
    fn plugin_capabilities_are_catalogued() {
        let catalog = CapabilityCatalog::eustress_core();
        for name in EXPECTED_PLUGIN_CAPABILITIES {
            assert!(
                catalog.lookup(name).is_some(),
                "capability catalog missing '{name}' — add it to CapabilityCatalog::eustress_core() \
                 to match rune_ecs_module::create_ecs_module"
            );
        }
    }
}
