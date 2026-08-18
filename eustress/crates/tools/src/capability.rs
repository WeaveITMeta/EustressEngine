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

        // --- Network: leaves the machine --------------------------------
        "http_request" | "image_to_code" | "image_to_geometry" | "document_to_code" => Network,

        // --- Write: creates or modifies inside the Space ----------------
        "create_entity"
        | "update_entity"
        | "insert_gaussian_splats"
        | "write_file"
        | "create_script"
        | "stage_file_change"
        | "remember"
        | "add_tag"
        | "remove_tag"
        | "datastore_set"
        | "set_sim_value"
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
        | "demote_entity" => Write,

        // --- Read: observation only -------------------------------------
        "query_entities"
        | "find_entity"
        | "read_file"
        | "list_directory"
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
        | "sim_bindings"
        | "oplog_tail"
        | "sim_step"
        | "get_editor_state"
        | "get_conversation"
        | "suggest_contextual_edits"
        | "suggest_swap_template"
        | "suggest_tool_defaults" => Read,

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

/// Decide whether a call may proceed.
pub fn authorize(tool_name: &str, permissions: &Permissions) -> Result<Capability, Denial> {
    match capability_of(tool_name) {
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
}
