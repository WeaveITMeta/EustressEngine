//! # Tool capabilities and caller permissions
//!
//! Enforcement for **CMMC AC.L1-3.1.2** — "limit information system access to
//! the types of transactions and functions that authorized users are permitted
//! to execute."
//!
//! Before this module, [`crate::ToolRegistry::dispatch`] executed any
//! registered tool for any caller. `read_only()` existed, but only as an
//! advisory `readOnlyHint` on MCP's `tools/list` — nothing consulted it at
//! execution time. So anything that could open the engine bridge could call
//! `run_bash`, `delete_entity`, or `write_file`.
//!
//! ## Design: a central policy table, not a per-handler flag
//!
//! Classification lives in one function, [`capability_of`], rather than being
//! spread across ~50 handler impls. That is deliberate and it is the
//! auditable choice: an assessor reads a single table to see every tool's
//! privilege level, instead of grepping fifty files and trusting that none of
//! them lied. It also makes the omission case testable — see
//! [`tests::every_registered_tool_is_classified`], which fails the build when
//! a new tool is added without a classification rather than letting it
//! inherit a permissive default.
//!
//! ## Default deny for the dangerous half
//!
//! [`Permissions::standard`] — the default for callers that arrive over a
//! transport — grants Read and Write but **not** Execute, Destructive, or
//! Network. Those must be granted explicitly by a caller that has established
//! who it is. Least privilege is the default state, not an opt-in.

use std::collections::BTreeSet;

use eustress_common::editor_action::{parse_action, Action};

/// What class of action a tool performs. Ordered by blast radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Observes state. Cannot change anything.
    Read,
    /// Creates or modifies data inside the Space.
    Write,
    /// Removes data, or mutates it irreversibly.
    Destructive,
    /// Runs code or shells out — the widest blast radius, because the
    /// action's true effect is not visible in the tool's arguments.
    Execute,
    /// Reaches a network endpoint outside the machine.
    Network,
}

impl Capability {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Destructive => "destructive",
            Self::Execute => "execute",
            Self::Network => "network",
        }
    }
}

/// The capability set a caller is permitted to invoke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permissions {
    allowed: BTreeSet<Capability>,
    /// Recorded on denial so the audit trail names who was refused.
    pub principal: Option<String>,
}

impl Permissions {
    /// Observation only. The right level for an untrusted or unauthenticated
    /// peer.
    pub fn read_only() -> Self {
        Self { allowed: [Capability::Read].into(), principal: None }
    }

    /// Read and Write, but not Execute, Destructive, or Network. The default
    /// for a caller arriving over a transport that has not established
    /// identity.
    pub fn standard() -> Self {
        Self {
            allowed: [Capability::Read, Capability::Write].into(),
            principal: None,
        }
    }

    /// Every capability. For the local, user-driven Workshop session, where
    /// the human is present and each destructive tool call still passes
    /// through the approval gate.
    pub fn full() -> Self {
        Self {
            allowed: [
                Capability::Read,
                Capability::Write,
                Capability::Destructive,
                Capability::Execute,
                Capability::Network,
            ]
            .into(),
            principal: None,
        }
    }

    /// Attach the identity this grant was issued to, so denials can name it.
    pub fn for_principal(mut self, principal: impl Into<String>) -> Self {
        self.principal = Some(principal.into());
        self
    }

    /// Add one capability to an existing grant.
    pub fn grant(mut self, capability: Capability) -> Self {
        self.allowed.insert(capability);
        self
    }

    /// Remove one capability from an existing grant.
    pub fn revoke(mut self, capability: Capability) -> Self {
        self.allowed.remove(&capability);
        self
    }

    pub fn allows(&self, capability: Capability) -> bool {
        self.allowed.contains(&capability)
    }

    pub fn granted(&self) -> impl Iterator<Item = &Capability> {
        self.allowed.iter()
    }
}

impl Default for Permissions {
    /// Least privilege. A `ToolContext` built without an explicit decision
    /// gets the safe set, never the full one.
    fn default() -> Self {
        Self::standard()
    }
}

/// The capability classification for every registered tool.
///
/// Returns `None` for an unknown tool, which [`crate::ToolRegistry::dispatch`]
/// treats as **deny** — an unclassified tool is not executable. That is what
/// makes a forgotten classification a safe failure instead of a silent hole.
pub fn capability_of(tool_name: &str) -> Option<Capability> {
    use Capability::*;
    Some(match tool_name {
        // --- Execute: runs code. Widest blast radius. -------------------
        "run_bash" | "execute_luau" | "execute_rune" => Execute,

        // --- Destructive: removes or irreversibly mutates ---------------
        "delete_entity" | "git_commit" | "git_branch" => Destructive,
        // Dispatches any live editor action by name, and Delete and Cut are
        // among them. This is its base class: `capability_of_call` judges a
        // call by the action it names, and one that names none stays here.
        "invoke_action" => Destructive,
        // Calls a method its class declares. This is its base class:
        // `capability_of_call` judges a call by the method's declared effect,
        // and an undeclared one keeps this class.
        "call_method" => Destructive,
        // The same reasoning covers the rest of the UI-driving family: each can
        // reach ANY control. `ui_click` presses whatever is at a coordinate,
        // `ui_sequence` chains clicks and tool picks, and `invoke_mode_tool` runs
        // any ribbon tool's real handler -- Delete and Publish included. Filed
        // as Write, any of them would bypass the Destructive tier that
        // `delete_entity` sits behind.
        // `ui_sequence` is judged per call by its steps (`capability_of_call`):
        // a step that can press Play makes it Execute.
        "ui_sequence" | "invoke_mode_tool" => Destructive,
        // A coordinate reaches every control, the Play button included, and
        // Play runs the Space's scripts.
        "ui_click" => Execute,
        // Uploads the open Space to the PUBLIC gallery: irreversible and
        // outward-facing. Deliberately NOT Network, even though it leaves the
        // machine. The MCP server grants Network to anyone holding
        // EUSTRESS_MODERATOR_TOKEN, a credential that exists so a moderator can
        // work the review queue; filed as Network, that review credential would
        // silently double as publish rights. `capability_of` returns ONE class,
        // so it gets the one no unrelated token hands out, and it fails closed
        // for every MCP caller.
        "publish_space" => Destructive,
        // Remove ground: a carve empties a shape of the terrain, and a clear
        // deletes the whole terrain's files with no undo.
        "terrain_carve" | "terrain_clear" => Destructive,
        // Replaces the live terrain with its files; with `discard_unsaved` it
        // loses edits. This is its base class: `capability_of_call` lowers a
        // call that cannot lose anything to Write.
        "terrain_reload" => Destructive,
        // Presses keys and buttons in a running game: it changes the live
        // session, never the Space on disk.
        "play_input" => Write,

        // --- Network: leaves the machine --------------------------------
        "http_request" | "image_to_code" | "image_to_geometry" | "document_to_code" => Network,
        // Moderation acts on api.eustress.dev as an admin; the reads are Network
        // too, since a case record leaves the machine to be read.
        "moderation_queue" | "moderation_case" | "moderation_act" | "moderation_backfill" => Network,

        // --- Write: creates or modifies inside the Space ----------------
        "create_entity"
        | "update_entity"
        | "particle_simulation"
        // Changes PhysicsService and saves it to the Space's _service.toml,
        // the same effect as a properties panel edit.
        | "set_physics_settings"
        | "insert_gaussian_splats"
        | "write_file"
        | "create_script"
        | "stage_file_change"
        | "remember"
        | "add_tag"
        | "remove_tag"
        | "datastore_set"
        | "set_sim_value"
        // Steps the running simulation: it advances the world.
        | "sim_step"
        | "generate_docs"
        | "cad_create_part"
        | "cad_set_variable"
        | "cad_export_glb"
        | "cad_add_feature"
        | "cad_edit_feature"
        | "cad_delete_feature"
        | "cad_create_sketch"
        | "cad_add_sketch_entity"
        | "cad_add_constraint"
        | "cad_dimension"
        | "cad_offset_sketch"
        | "cad_publish_part"
        // Writes a .step file into the Space, like cad_export_glb.
        | "cad_export_step"
        // Engine-registered Workshop tools that mutate state or write
        // files — see the Read-bucket note above for why they were
        // missing entirely.
        | "run_scenario"
        | "control_simulation"
        | "set_breakpoint"
        | "storage_optimize"
        | "allocate_product"
        | "export_recording"
        | "run_simulation"
        | "stop_simulation"
        | "pause_simulation"
        | "run_experiment"
        | "new_space"
        | "new_universe"
        | "rename_space"
        | "rename_universe"
        | "set_active_universe"
        | "set_next_launch_universe"
        | "promote_entity"
        // Website service authoring writes TOML inside the Space.
        | "website_setup"
        | "website_add_reference"
        | "demote_entity"
        // MCP-server bridge tools (mcp-server/src/shared_registry.rs) that
        // change state. Selection and the active tool are the USER's live
        // editor state -- they drive the gizmos and the Properties panel --
        // so they are Write rather than observation, and a read-only peer may
        // not move them.
        | "select_entity"
        | "equip_tool"
        | "data_bind"
        | "data_unbind"
        | "export_instances_toml"
        // Terrain edits in the live engine. Sculpt, paint, fill, replace and
        // layer creation are each one undo step; generate and flat replace the
        // terrain's files but keep the layers someone authored.
        | "terrain_generate"
        | "terrain_flat"
        | "terrain_sculpt"
        | "terrain_paint"
        | "terrain_fill"
        | "terrain_replace_material"
        | "terrain_layer_create"
        // Switches the live mode/discipline and persists it to editor settings.
        | "set_mode"
        // Snapshots of the open Space. Saving one is Ctrl+S plus a commit in
        // the Space's own autosave repo (and a database checkpoint): it adds a
        // restore point and removes nothing, unlike git_commit, which takes
        // arbitrary paths and messages. A revert overwrites the Space, but
        // only after a safety snapshot of it, unsaved script tabs included,
        // is confirmed saved, and it aborts before touching anything
        // otherwise; reverting to that snapshot undoes it. Cancelling removes
        // a pending restore plan and leaves the Space as it is.
        | "save_snapshot"
        | "revert_to_snapshot"
        | "cancel_pending_revert"
        // Adds or changes one Parameter with the Properties panel's rules,
        // saved into the instance's record; it never deletes one.
        | "parameters_set"
        // Sets properties through the Properties panel's own edits, as one
        // undo step; it never deletes or renames.
        | "set_properties"
        // Sets or removes attributes on one instance in Edit, as one undo
        // step, saved into its own record.
        | "attribute_set"
        // Moves and turns an instance and its subtree in Edit, as one
        // undo step, saved into each moved record.
        | "transform_subtree"
        // Tree edits in Edit, each one undo step; a clone or a move from
        // a server-only service to a player one needs expose_to_clients.
        | "rename_instance"
        | "reparent_instance"
        | "clone_instance"
        | "create_instance"
        // Loads a mesh file again; changes no record.
        | "reload_mesh"
        // Copies a GLB from inside the Universe into the Space and writes
        // Parts that draw it; a different file already at the destination is
        // replaced only when the call says overwrite, as write_file replaces.
        | "import_asset"
        | "place_instances" => Write,

        // Changes the Space the person works in and clears the undo
        // history; neither can be undone.
        "open_space" => Destructive,

        // Removing a Reference deletes an instance folder.
        "website_remove_reference" => Destructive,

        // --- Read: observation only -------------------------------------
        "query_entities"
        | "find_entity"
        | "read_file"
        | "list_directory"
        // Reads a data file and computes a report; writes nothing.
        | "mine_data"
        | "list_space_contents"
        | "measure_distance"
        | "raycast"
        | "scene_raycast"
        | "query_material"
        | "calculate_physics"
        | "get_sim_value"
        | "list_sim_values"
        | "get_tagged_entities"
        | "get_simulation_state"
        | "await_simulation"
        | "compare_runs"
        | "list_experiments"
        | "tail_telemetry"
        | "query_audit_log"
        | "query_stream_events"
        | "recall"
        | "list_rules"
        | "list_workflows"
        | "git_status"
        | "git_log"
        | "git_diff"
        | "feedback_diff"
        | "datastore_get"
        | "cad_describe_part"
        | "cad_validate_part"
        | "cad_measure"
        | "cad_list_templates"
        | "cad_solve_sketch"
        | "cad_list_sources"
        | "cad_list_topology"
        // Mode-specific Workshop tools. These are registered by the
        // ENGINE (engine/src/workshop/mod.rs) on top of this crate's
        // baseline, so they are advertised to callers and to the
        // engine bridge but were never classified here — every call
        // was refused at dispatch. Everything in this group is pure
        // computation or a lookup, hence Read.
        | "calculate_cost"
        | "estimate_tax"
        | "price_product"
        | "estimate_shipping"
        | "select_process"
        | "forecast_demand"
        | "score_supplier_risk"
        | "inventory_check"
        | "query_manufacturers"
        | "query_investors"
        | "normalize_brief"
        | "list_universes"
        | "list_spaces"
        | "list_scripts"
        | "read_script"
        | "list_assets"
        | "find_similar_entities"
        | "search_universe"
        | "scene_overview"
        | "partition_scene"
        | "inspect_scene"
        | "website_status"
        | "website_manifest_url"
        | "sim_bindings"
        | "oplog_tail"
        | "get_editor_state"
        | "read_output"
        | "get_conversation"
        | "suggest_contextual_edits"
        | "suggest_swap_template"
        | "suggest_tool_defaults"
        // MCP-server bridge tools. The server registers these on top of this
        // crate's baseline, outside `every_registered_tool_is_classified`
        // below, which is how all twelve shipped unclassified and were
        // refused at dispatch; the server now guards its own registry. The
        // three pose tools only aim the AI's OWN off-screen camera, which
        // never displaces the user's view, so they are observation.
        | "ai_camera_capture"
        | "ai_camera_frame"
        | "ai_camera_orbit"
        | "ai_camera_set_pose"
        | "capture_viewport"
        | "data_bindings"
        // Reports PhysicsService settings and which domains are running.
        | "get_physics_settings"
        // Observation only. `publish_status` reports readiness and progress
        // WITHOUT publishing.
        | "list_modes"
        | "list_mode_tools"
        | "publish_status"
        // Snapshot history and what changed since one: git log, diff and
        // ls-files in the Space's repo.
        | "list_snapshots"
        | "diff_snapshot"
        // Lists an instance's Parameters.
        | "parameters_get"
        // Lists an instance's attributes.
        | "attribute_get"
        // Lists an instance's properties and methods.
        | "get_properties"
        // Lists the mesh files the open Space draws.
        | "list_meshes"
        // Lists what scripts made in the running Play session.
        | "list_play_instances"
        // Terrain reads: stats, surface samples, terrain-only raycasts and
        // voxel reads of the live engine's terrain.
        | "terrain_stats"
        | "terrain_query"
        | "terrain_raycast"
        | "terrain_read_voxels" => Read,

        _ => return None,
    })
}

/// Why a dispatch was refused. Carried into the `ToolResult` so the caller —
/// and the audit log — sees the specific denial, not a generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub tool_name: String,
    pub required: Option<Capability>,
    pub principal: Option<String>,
}

impl Denial {
    /// Message surfaced to the caller. States the capability required and the
    /// principal refused, so the fix is obvious and the event is greppable.
    pub fn message(&self) -> String {
        match self.required {
            Some(cap) => format!(
                "Permission denied: '{}' requires the '{}' capability, which {} does not hold.",
                self.tool_name,
                cap.label(),
                self.principal.as_deref().unwrap_or("this caller"),
            ),
            None => format!(
                "Permission denied: '{}' has no capability classification, so it cannot be \
                 executed. Add it to capability_of() in tools/src/capability.rs.",
                self.tool_name
            ),
        }
    }
}

/// The class of one editor action, judged by what its handler REACHES, not by
/// its name. `invoke_action` runs any of them, so this decides what a caller
/// holding only Write can do with it.
///
/// No wildcard arm: a new `Action` variant does not compile until someone
/// decides its class here.
///
/// - Write: changes the view, the selection, a tool or a setting, or makes an
///   edit the editor's Undo reverses.
/// - Destructive: removes or overwrites data, walks the edit history (Undo can
///   delete what a create made, Redo can replay a delete), switches or blocks
///   the person's session, reaches the network, or faces outward.
/// - Execute: starts Play, which runs the Space's scripts.
pub fn action_capability(action: Action) -> Capability {
    use Capability::*;
    match action {
        // Tools, panels, camera, selection and editor settings.
        Action::SelectTool
        | Action::MoveTool
        | Action::RotateTool
        | Action::ScaleTool
        | Action::ToggleExplorer
        | Action::ToggleProperties
        | Action::ToggleOutput
        | Action::ToggleCommandBar
        // No handler, or only an "on the roadmap" notice.
        | Action::ToggleAssets
        | Action::ToggleCollaboration
        | Action::ToggleNetworkPanel
        | Action::ToggleTransformSpace
        | Action::FocusSelection
        | Action::ViewPerspectiveToggle
        | Action::ViewTop
        | Action::ViewFront
        | Action::ViewSideLeft
        | Action::ViewSideRight
        | Action::ViewMode2D
        | Action::ViewMode3D
        // Adds a view to .eustress/viewpoints.toml.
        | Action::SaveViewpoint
        | Action::NextViewpoint
        | Action::SnapMode1
        | Action::SnapMode2
        | Action::SnapModeOff
        | Action::ToggleCollisions
        | Action::SelectAll
        | Action::SelectChildren
        | Action::SelectDescendants
        | Action::SelectParent
        | Action::SelectSiblings
        | Action::InvertSelection
        // Open a dialog or a search box inside the editor window.
        | Action::InsertObject
        | Action::FindReplace
        | Action::FindInScripts
        | Action::FocusExplorerSearch
        | Action::FocusPropertiesFilter
        // Arm a modal tool. Its edit takes clicks, which `ui_click` gates.
        | Action::ToolPartSwap
        | Action::ToolEdgeAlign
        | Action::ToolModelReflect
        | Action::ToolGapFill
        | Action::ToolResizeAlign
        | Action::ToolMaterialFlip
        | Action::ToolLinearArray
        | Action::ToolRadialArray
        | Action::ToolGridArray
        | Action::ToolPathArray
        | Action::ToolPaintBucket
        // Keyboard nudges; nothing handles them as an action.
        | Action::LiftSelection
        | Action::SettleSelection
        // Terrain tool and brush state. Sampling reads the ground.
        | Action::TerrainTools
        | Action::TerrainDraw
        | Action::TerrainSculpt
        | Action::TerrainSmooth
        | Action::TerrainFlatten
        | Action::TerrainPaint
        | Action::TerrainSeaLevel
        | Action::TerrainRegion
        | Action::TerrainSizeDown
        | Action::TerrainSizeUp
        | Action::TerrainStrengthDown
        | Action::TerrainStrengthUp
        | Action::TerrainPivotPrev
        | Action::TerrainPivotNext
        | Action::TerrainPlaneLock
        | Action::TerrainPlanePick
        | Action::TerrainPlaneUp
        | Action::TerrainPlaneDown
        | Action::TerrainPlaneUpFast
        | Action::TerrainPlaneDownFast
        | Action::TerrainSnap
        | Action::TerrainSnapStep
        | Action::TerrainContours
        | Action::TerrainMirror
        | Action::TerrainMirrorAxis
        | Action::TerrainSampleMaterial
        // Samples the look under the cursor; changes no part.
        | Action::SampleLook
        // Copy fills the region clipboard. Paste only arms a placement; the
        // write is a later click.
        | Action::TerrainRegionCopy
        | Action::TerrainRegionPaste
        // Session controls. Stop restores the source state.
        | Action::PauseResume
        | Action::StopPlay => Write,

        // Edits the editor's Undo reverses, and a save.
        Action::SaveScene
        | Action::Copy
        | Action::Paste
        | Action::PasteInto
        | Action::Duplicate
        | Action::Group
        | Action::ToggleAnchor
        | Action::LockSelection
        | Action::UnlockSelection
        | Action::RotateY90
        | Action::TiltZ90 => Write,

        // Remove or overwrite data. CSG trashes its source parts; a terrain
        // duplicate writes its copy's air too, so it can carve.
        Action::Delete
        | Action::Cut
        | Action::CSGNegate
        | Action::CSGUnion
        | Action::MeshSplitParts
        | Action::MeshSplitEdges
        | Action::CSGIntersect
        | Action::CSGSeparate
        | Action::TerrainRegionCut
        | Action::TerrainRegionDelete
        | Action::TerrainRegionDuplicate
        // Undo can delete what a create made, and Redo can replay a delete.
        | Action::Undo
        | Action::Redo
        // Only partly reversible for a folder-backed group (grouping.rs).
        | Action::Ungroup
        // Renames the Universe or Space folder on disk, which moves every
        // path inside it; the chooser can reach either.
        | Action::Rename
        | Action::RenameUniverse
        | Action::RenameSpace
        // Switch the open Space, or block the editor on a native dialog.
        | Action::NewSpace
        | Action::NewUniverse
        | Action::OpenFile
        | Action::SaveSceneAs
        // Hosting reaches the network, and stopping it drops players.
        | Action::StartServer
        | Action::StopServer
        // Outward-facing. Today they open the publish dialog; a later change
        // that publishes directly must not become callable at Write.
        | Action::PublishSpace
        | Action::PublishUniverse => Destructive,

        // Play runs the Space's scripts.
        Action::PlayWithCharacter | Action::PlaySolo => Execute,
    }
}

/// The bridge request `invoke_action` sends for this input: its `action` field
/// and nothing else. The gate classifies this value and the tool sends this
/// value, so the two cannot disagree about which action runs.
pub fn invoke_action_params(input: &serde_json::Value) -> serde_json::Value {
    let mut params = serde_json::Map::new();
    if let Some(action) = input.get("action") {
        params.insert("action".to_string(), action.clone());
    }
    serde_json::Value::Object(params)
}

/// The class of one call, arguments included. `invoke_action` is judged by the
/// action it would run ([`action_capability`]), read with the engine's own
/// [`parse_action`] from the request the tool sends; an input that names no
/// action keeps the tool's base class, so a malformed or misspelt request
/// fails closed. `ui_sequence` is judged by its steps
/// ([`ui_sequence_capability`]). Every other tool has its base class.
pub fn capability_of_call(tool_name: &str, input: &serde_json::Value) -> Option<Capability> {
    match tool_name {
        "invoke_action" => parse_action(&invoke_action_params(input))
            .map(action_capability)
            .or_else(|| capability_of(tool_name)),
        "ui_sequence" => Some(ui_sequence_capability(input)),
        "terrain_reload" => Some(terrain_reload_capability(input)),
        "call_method" => Some(call_method_capability(input)),
        _ => capability_of(tool_name),
    }
}

/// The class of a `ui_sequence` call: its base class, Destructive, or Execute
/// when a step can run code. An `action` step is judged as `invoke_action`
/// judges that action, from the same request the step sends, and a `click`
/// step can press any control, Play included.
fn ui_sequence_capability(input: &serde_json::Value) -> Capability {
    let runs_code = input
        .get("steps")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .any(|step| match step.get("op").and_then(|v| v.as_str()) {
            Some("click") => true,
            Some("action") => {
                parse_action(&invoke_action_params(step)).map(action_capability) == Some(Capability::Execute)
            }
            _ => false,
        });
    if runs_code {
        Capability::Execute
    } else {
        Capability::Destructive
    }
}

/// The class of a `call_method` call: the effect its method declares
/// (`eustress_common::class_methods`), read from the "Class:Name" it names.
/// An undeclared or unreadable method keeps the base class, Destructive.
fn call_method_capability(input: &serde_json::Value) -> Capability {
    use eustress_common::class_methods::{declared, split_qualified, MethodEffect};
    input
        .get("method")
        .and_then(|v| v.as_str())
        .and_then(split_qualified)
        .and_then(|(class, name)| declared(class, name))
        .map_or(Capability::Destructive, |m| match m.effect {
            MethodEffect::Read => Capability::Read,
            MethodEffect::Write => Capability::Write,
            MethodEffect::Destructive => Capability::Destructive,
        })
}

/// The class of a `terrain_reload` call: Write when it cannot lose an edit
/// (`discard_unsaved` absent, null or false, so the engine refuses a terrain
/// with unsaved edits), Destructive for any other value. A value the engine
/// would not read as true is still judged the stronger way.
fn terrain_reload_capability(input: &serde_json::Value) -> Capability {
    match input.get("discard_unsaved") {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => Capability::Write,
        Some(_) => Capability::Destructive,
    }
}

/// Decide whether a call may proceed, by the tool's base class alone. The
/// registry dispatches through [`authorize_call`], which can lower the class
/// for a call whose arguments make it safer.
pub fn authorize(tool_name: &str, permissions: &Permissions) -> Result<Capability, Denial> {
    decide(tool_name, capability_of(tool_name), permissions)
}

/// Decide whether this call, arguments included, may proceed.
pub fn authorize_call(
    tool_name: &str,
    input: &serde_json::Value,
    permissions: &Permissions,
) -> Result<Capability, Denial> {
    decide(tool_name, capability_of_call(tool_name, input), permissions)
}

fn decide(
    tool_name: &str,
    class: Option<Capability>,
    permissions: &Permissions,
) -> Result<Capability, Denial> {
    match class {
        Some(cap) if permissions.allows(cap) => Ok(cap),
        Some(cap) => Err(Denial {
            tool_name: tool_name.to_string(),
            required: Some(cap),
            principal: permissions.principal.clone(),
        }),
        // Unclassified => deny. Fail closed.
        None => Err(Denial {
            tool_name: tool_name.to_string(),
            required: None,
            principal: permissions.principal.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_permissions_exclude_the_dangerous_capabilities() {
        let p = Permissions::default();
        assert!(p.allows(Capability::Read));
        assert!(p.allows(Capability::Write));
        // The three that must be granted deliberately.
        assert!(!p.allows(Capability::Execute));
        assert!(!p.allows(Capability::Destructive));
        assert!(!p.allows(Capability::Network));
    }

    #[test]
    fn the_named_cmmc_gap_tools_are_denied_by_default() {
        // These are the exact tools cited in the AC.L1-3.1.2 finding.
        let p = Permissions::default();
        for tool in ["run_bash", "delete_entity"] {
            assert!(
                authorize(tool, &p).is_err(),
                "{tool} must not be callable under default permissions"
            );
        }
        // write_file was also cited; it is a Write, so it is permitted at
        // standard level but denied to a read-only peer.
        assert!(authorize("write_file", &p).is_ok());
        assert!(authorize("write_file", &Permissions::read_only()).is_err());
    }

    #[test]
    fn unclassified_tools_fail_closed() {
        let denial = authorize("some_tool_added_next_week", &Permissions::full())
            .expect_err("an unclassified tool must be denied even with full permissions");
        assert_eq!(denial.required, None);
        assert!(denial.message().contains("no capability classification"));
    }

    #[test]
    fn full_permissions_allow_execute_but_still_classify() {
        let p = Permissions::full();
        assert_eq!(authorize("run_bash", &p), Ok(Capability::Execute));
        assert_eq!(authorize("delete_entity", &p), Ok(Capability::Destructive));
        assert_eq!(authorize("http_request", &p), Ok(Capability::Network));
        assert_eq!(authorize("read_file", &p), Ok(Capability::Read));
    }

    #[test]
    fn denial_message_names_capability_and_principal() {
        let p = Permissions::standard().for_principal("bridge-peer-127.0.0.1");
        let denial = authorize("run_bash", &p).unwrap_err();
        let msg = denial.message();
        assert!(msg.contains("run_bash"));
        assert!(msg.contains("execute"));
        assert!(msg.contains("bridge-peer-127.0.0.1"));
    }

    #[test]
    fn read_only_peer_can_only_observe() {
        let p = Permissions::read_only();
        assert!(authorize("query_entities", &p).is_ok());
        assert!(authorize("inspect_scene", &p).is_ok());
        for tool in ["create_entity", "write_file", "delete_entity", "run_bash", "http_request"] {
            assert!(authorize(tool, &p).is_err(), "{tool} must be denied to a read-only peer");
        }
    }

    #[test]
    fn grant_and_revoke_are_explicit() {
        let p = Permissions::standard().grant(Capability::Execute);
        assert!(authorize("execute_luau", &p).is_ok());
        let p = p.revoke(Capability::Execute);
        assert!(authorize("execute_luau", &p).is_err());
    }

    #[test]
    fn terrain_tools_are_classified_by_what_they_can_remove() {
        for tool in ["terrain_stats", "terrain_query", "terrain_raycast", "terrain_read_voxels"] {
            assert_eq!(capability_of(tool), Some(Capability::Read), "{tool}");
        }
        for tool in [
            "terrain_generate",
            "terrain_flat",
            "terrain_sculpt",
            "terrain_paint",
            "terrain_fill",
            "terrain_replace_material",
            "terrain_layer_create",
        ] {
            assert_eq!(capability_of(tool), Some(Capability::Write), "{tool}");
        }
        // Carving and clearing remove ground, so a standard caller is refused.
        for tool in ["terrain_carve", "terrain_clear"] {
            assert_eq!(capability_of(tool), Some(Capability::Destructive), "{tool}");
            assert!(
                authorize(tool, &Permissions::standard()).is_err(),
                "{tool} must be refused to a standard caller"
            );
        }
    }

    #[test]
    fn every_registered_tool_is_classified() {
        // The omission guard: a tool registered without a classification is
        // undispatchable, so this failing is a build-time reminder rather
        // than a runtime hole.
        let mut registry = crate::ToolRegistry::default();
        crate::register_all_tools(&mut registry);
        let unclassified: Vec<&str> = registry
            .all_tools()
            .into_iter()
            .map(|d| d.name)
            .filter(|name| capability_of(name).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "these registered tools have no capability classification and would be \
             denied at dispatch: {unclassified:?}"
        );
    }

    #[test]
    fn invoke_action_is_judged_by_the_action_it_runs() {
        use serde_json::json;
        let class = |action: &str| capability_of_call("invoke_action", &json!({ "action": action }));
        for write in ["ViewMode2D", "ViewTop", "SelectAll", "MoveTool", "Copy", "Group", "SaveScene", "FindInScripts"] {
            assert_eq!(class(write), Some(Capability::Write), "{write}");
        }
        for destructive in [
            "Delete",
            "Cut",
            "Undo",
            "Redo",
            "Ungroup",
            "CSGUnion",
            "MeshSplitParts",
            "MeshSplitEdges",
            "OpenFile",
            "NewSpace",
            "SaveSceneAs",
            "StartServer",
            "StopServer",
            "PublishSpace",
            "PublishUniverse",
            "TerrainRegionDuplicate",
        ] {
            assert_eq!(class(destructive), Some(Capability::Destructive), "{destructive}");
        }
        for execute in ["PlaySolo", "PlayWithCharacter"] {
            assert_eq!(class(execute), Some(Capability::Execute), "{execute}");
        }
    }

    #[test]
    fn an_unreadable_action_keeps_the_base_class() {
        use serde_json::json;
        assert_eq!(capability_of("invoke_action"), Some(Capability::Destructive));
        for input in [
            json!({ "action": "delete" }),
            json!({ "action": "DELETE" }),
            json!({ "action": " ViewMode2D" }),
            json!({ "action": 5 }),
            json!({ "action": { "ViewMode2D": null } }),
            json!({}),
            json!("ViewMode2D"),
        ] {
            assert_eq!(
                capability_of_call("invoke_action", &input),
                Some(Capability::Destructive),
                "{input}"
            );
        }
    }

    #[test]
    fn a_repeated_action_key_is_judged_by_the_action_sent() {
        // serde_json keeps the last value, and the tool sends the very request
        // the gate classified, so "ViewMode2D, then Delete" is a Delete at both.
        let input: serde_json::Value =
            serde_json::from_str(r#"{"action":"ViewMode2D","action":"Delete"}"#).unwrap();
        assert_eq!(capability_of_call("invoke_action", &input), Some(Capability::Destructive));
        assert_eq!(invoke_action_params(&input)["action"], "Delete");
    }

    #[test]
    fn the_request_sent_carries_only_the_action() {
        let input = serde_json::json!({ "action": "ViewMode2D", "extra": true });
        assert_eq!(invoke_action_params(&input), serde_json::json!({ "action": "ViewMode2D" }));
    }

    #[test]
    fn a_standard_caller_may_run_write_actions_only() {
        use serde_json::json;
        let p = Permissions::standard().for_principal("mcp-client");
        assert_eq!(
            authorize_call("invoke_action", &json!({ "action": "ViewMode2D" }), &p),
            Ok(Capability::Write)
        );
        let denial = authorize_call("invoke_action", &json!({ "action": "Delete" }), &p).unwrap_err();
        assert_eq!(denial.required, Some(Capability::Destructive));
        assert!(authorize_call("invoke_action", &json!({ "action": "PlaySolo" }), &p).is_err());
        // The name-only check still refuses the tool outright.
        assert!(authorize("invoke_action", &p).is_err());
    }

    #[test]
    fn a_ui_sequence_is_as_strong_as_its_strongest_step() {
        use serde_json::json;
        let class = |steps: serde_json::Value| capability_of_call("ui_sequence", &json!({ "steps": steps }));
        assert_eq!(
            class(json!([{ "op": "set_mode", "mode": "build" }, { "op": "action", "action": "Delete" }])),
            Some(Capability::Destructive)
        );
        // Play runs the Space's scripts, whichever step presses it.
        assert_eq!(class(json!([{ "op": "action", "action": "PlaySolo" }])), Some(Capability::Execute));
        assert_eq!(class(json!([{ "op": "wait", "ms": 10 }, { "op": "click", "x": 10, "y": 10 }])), Some(Capability::Execute));
        assert_eq!(capability_of_call("ui_sequence", &json!({})), Some(Capability::Destructive));
        let without_execute = Permissions::full().revoke(Capability::Execute);
        assert!(authorize_call("ui_sequence", &json!({ "steps": [{ "op": "action", "action": "PlaySolo" }] }), &without_execute).is_err());
    }

    #[test]
    fn a_coordinate_click_can_press_play() {
        assert_eq!(capability_of("ui_click"), Some(Capability::Execute));
    }

    #[test]
    fn stepping_the_simulation_changes_the_world() {
        assert_eq!(capability_of("sim_step"), Some(Capability::Write));
        assert!(authorize("sim_step", &Permissions::read_only()).is_err());
    }

    /// Every bridge tool added in builds 30 and 31 keeps the class Security
    /// approved: reads are Read, edits Write, and opening a Space (which
    /// clears the undo history) Destructive, so auto mode asks first.
    #[test]
    fn the_bridge_edit_tools_are_classified() {
        for read in ["attribute_get", "list_play_instances", "list_meshes"] {
            assert_eq!(capability_of(read), Some(Capability::Read), "{read}");
        }
        for write in [
            "attribute_set",
            "transform_subtree",
            "rename_instance",
            "reparent_instance",
            "clone_instance",
            "create_instance",
            "reload_mesh",
        ] {
            assert_eq!(capability_of(write), Some(Capability::Write), "{write}");
        }
        assert_eq!(capability_of("open_space"), Some(Capability::Destructive));
    }

    #[test]
    fn the_property_tools_are_classified() {
        use serde_json::json;
        assert_eq!(capability_of("get_properties"), Some(Capability::Read));
        assert_eq!(capability_of("set_properties"), Some(Capability::Write));
        assert_eq!(capability_of("call_method"), Some(Capability::Destructive));
        // No method is declared yet, so every call keeps the base class.
        for input in [json!({ "method": "Road:Split" }), json!({ "method": "Split" }), json!({})] {
            assert_eq!(capability_of_call("call_method", &input), Some(Capability::Destructive), "{input}");
        }
        assert!(authorize_call("call_method", &json!({ "method": "Road:Split" }), &Permissions::standard()).is_err());
    }

    #[test]
    fn a_terrain_reload_is_destructive_only_when_it_can_lose_edits() {
        use serde_json::json;
        assert_eq!(capability_of("terrain_reload"), Some(Capability::Destructive));
        for input in [json!({}), json!({ "discard_unsaved": null }), json!({ "discard_unsaved": false })] {
            assert_eq!(capability_of_call("terrain_reload", &input), Some(Capability::Write), "{input}");
        }
        for input in [json!({ "discard_unsaved": true }), json!({ "discard_unsaved": "true" }), json!({ "discard_unsaved": 1 })] {
            assert_eq!(capability_of_call("terrain_reload", &input), Some(Capability::Destructive), "{input}");
        }
        let mcp = Permissions::standard().for_principal("mcp-client");
        assert!(authorize_call("terrain_reload", &json!({}), &mcp).is_ok());
        assert!(authorize_call("terrain_reload", &json!({ "discard_unsaved": true }), &mcp).is_err());
    }

    #[test]
    fn other_tools_keep_their_base_class_whatever_the_input() {
        let input = serde_json::json!({ "action": "ViewMode2D" });
        for tool in [
            "run_bash",
            "delete_entity",
            "ui_click",
            "invoke_mode_tool",
            "publish_space",
            "read_file",
            "tool_added_next_week",
        ] {
            assert_eq!(capability_of_call(tool, &input), capability_of(tool), "{tool}");
        }
    }
}
