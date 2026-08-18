#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::parts::{PartData, PartType};
use crate::rendering::BevyPartManager;

/// Maximum number of undo/redo actions to keep
const MAX_HISTORY_SIZE: usize = 100;

/// Action types that can be undone/redone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Create a new part
    CreatePart {
        id: u32,
        part_type: PartType,
        position: Vec3,
        parent: Option<u32>,
    },
    
    /// Delete a part
    DeletePart {
        data: PartData,
    },
    
    /// Move a part
    MovePart {
        id: u32,
        old_position: Vec3,
        new_position: Vec3,
    },
    
    /// Rotate a part
    RotatePart {
        id: u32,
        old_rotation: Vec3,
        new_rotation: Vec3,
    },
    
    /// Scale a part
    ScalePart {
        id: u32,
        old_scale: Vec3,
        new_scale: Vec3,
    },
    
    /// Change part color
    ChangeColor {
        id: u32,
        old_color: [f32; 4],
        new_color: [f32; 4],
    },
    
    /// Group parts together
    GroupParts {
        parent_id: u32,
        child_ids: Vec<u32>,
        old_parents: Vec<Option<u32>>,
    },
    
    /// Ungroup parts
    UngroupParts {
        parent_id: u32,
        child_ids: Vec<u32>,
        new_parents: Vec<Option<u32>>,
    },
    
    /// Batch multiple actions together (for multi-select operations)
    Batch {
        actions: Vec<Action>,
    },
    
    /// Change a property on a single entity
    ChangeProperty {
        id: u32,
        property: String,
        old_value: PropertyValueSnapshot,
        new_value: PropertyValueSnapshot,
    },
    
    /// Change a property on multiple entities (for multi-select)
    ChangePropertyMulti {
        /// Entity IDs and their old values
        entities: Vec<(u32, PropertyValueSnapshot)>,
        property: String,
        new_value: PropertyValueSnapshot,
    },
    
    /// Change Parameters component on an entity
    ChangeParameters {
        id: u32,
        /// Serialized old Parameters (JSON)
        old_params: String,
        /// Serialized new Parameters (JSON)
        new_params: String,
    },
    
    /// Change Parameters on multiple entities
    ChangeParametersMulti {
        /// Entity IDs and their old Parameters (serialized JSON)
        entities: Vec<(u32, String)>,
        /// New Parameters to apply (serialized JSON)
        new_params: String,
    },
    
    /// Change Folder domain configuration
    ChangeFolderDomain {
        id: u32,
        old_domain: Option<String>,
        new_domain: Option<String>,
        old_source_override: Option<String>,
        new_source_override: Option<String>,
    },
    
    /// Change Folder sync configuration
    ChangeFolderSyncConfig {
        id: u32,
        /// Serialized old DomainSyncConfig (JSON)
        old_config: Option<String>,
        /// Serialized new DomainSyncConfig (JSON)
        new_config: Option<String>,
    },
    
    /// Change Attributes on an entity
    ChangeAttributes {
        id: u32,
        /// Serialized old Attributes (JSON)
        old_attrs: String,
        /// Serialized new Attributes (JSON)
        new_attrs: String,
    },
    
    /// Change Tags on an entity
    ChangeTags {
        id: u32,
        old_tags: Vec<String>,
        new_tags: Vec<String>,
    },
    
    /// Add a single attribute
    AddAttribute {
        id: u32,
        key: String,
        /// Serialized AttributeValue (JSON)
        value: String,
    },
    
    /// Remove a single attribute
    RemoveAttribute {
        id: u32,
        key: String,
        /// Serialized old AttributeValue (JSON) for undo
        old_value: String,
    },
    
    /// Add a tag
    AddTag {
        id: u32,
        tag: String,
    },
    
    /// Remove a tag
    RemoveTag {
        id: u32,
        tag: String,
    },
    
    /// Transform multiple entities (move/rotate) - uses Entity bits for ECS compatibility
    TransformEntities {
        /// Entity bits and their old transforms (translation, rotation)
        old_transforms: Vec<(u64, [f32; 3], [f32; 4])>,
        /// Entity bits and their new transforms
        new_transforms: Vec<(u64, [f32; 3], [f32; 4])>,
    },
    
    /// Scale multiple entities (resize) - stores position and size changes
    ScaleEntities {
        /// Entity bits and their old state (translation, size)
        old_states: Vec<(u64, [f32; 3], [f32; 3])>,
        /// Entity bits and their new state (translation, size)
        new_states: Vec<(u64, [f32; 3], [f32; 3])>,
    },

    /// Delete entities — files moved to .eustress/trash/ for recovery.
    /// Undo moves them back and triggers a space reload.
    TrashEntities {
        /// (original_path, trash_path) pairs
        paths: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    },

    /// Entities spawned by a Smart Build Tool (Gap Fill, Model Reflect,
    /// Resize Align's Rounded Join, etc.) — undo moves the folders to
    /// `.eustress/trash/` and despawns the entities; redo moves them
    /// back and respawns via the file watcher.
    ///
    /// Each pair is `(original_folder_path, reserved_trash_path)`. The
    /// trash path is chosen at action-record time so undo/redo are
    /// symmetric file-rename operations — no search.
    SpawnFolders {
        folders: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    },

    /// Explorer drag-drop reparent — the ON-DISK half. Each pair is
    /// `(original_path, new_path)`; undo renames `new_path` back to
    /// `original_path`, redo renames forward. Both directions also
    /// rebase every in-memory path reference (`InstanceFile.toml_path` /
    /// `LoadedFromFile.path` on the moved entity AND every descendant)
    /// and rekey the `SpaceFileRegistry`, because `do_reparent_node`
    /// gates the file watcher with `rename_in_progress` for the whole
    /// move — nothing else in the engine would ever notice it.
    ///
    /// Pair it with `ReparentEntities` in a `Batch` when the ECS
    /// `ChildOf` edge also has to be restored. The two are individually
    /// idempotent (each checks whether the folder is already where it
    /// wants it), so either order — and either one alone — is safe.
    MoveFolders {
        moves: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    },

    /// Explorer drag-drop reparent — the ECS half. `do_reparent_node`
    /// sets `ChildOf(target)` itself and suppresses the watcher for the
    /// rename, so the hierarchy is NOT re-derived from disk: a
    /// folder-rename-only undo would leave the entity parented to the
    /// drop target forever. This variant restores the `ChildOf` edge
    /// each entity held before the drag (`None` = scene root).
    ///
    /// Self-healing: if the folder is still sitting at `new_path` when
    /// undo runs (no companion `MoveFolders` entry), the rename is
    /// performed here too — so a lone `ReparentEntities` is complete.
    ReparentEntities {
        entries: Vec<ReparentEntry>,
    },

    /// Ctrl+G — the selection wrapped in a freshly-spawned container
    /// (`grouping.rs`). Group is a pure-ECS operation with no disk
    /// footprint: it spawns a `Model`, sets `ChildOf(model)` on each
    /// member and rewrites each member's LOCAL transform so its world
    /// transform is preserved. Undo restores every member's previous
    /// parent + local transform and despawns the wrapper; redo spawns a
    /// wrapper again and re-applies the grouped locals.
    ///
    /// The wrapper's `Entity` is deliberately NOT stored — redo spawns a
    /// new one, so a stored id would go stale after one undo→redo cycle.
    /// Undo instead locates it as the current parent of the first live
    /// member, the same content-match strategy `CadMateCreate` uses.
    GroupEntities {
        /// Wrapper `Instance.name` (grouping.rs uses `"Model"`).
        model_name: String,
        /// Wrapper `ClassName` in its `as_str()` form, parsed back with
        /// `ClassName::from_str` so new classes need no changes here.
        model_class: String,
        /// The wrapper's own parent at creation time (`None` = root).
        model_parent_bits: Option<u64>,
        /// The wrapper's local translation. `grouping.rs` spawns it
        /// axis-aligned and unit-scaled at the members' mean world
        /// position, so translation alone round-trips it exactly.
        model_translation: [f32; 3],
        /// One entry per grouped member.
        members: Vec<GroupMember>,
    },

    /// Ctrl+U — each selected `Model`/`Folder` dissolved: its children
    /// raised one level with world transforms preserved, the container
    /// despawned. Undo re-spawns each container (same class, name,
    /// parent and local transform) and puts its children back with
    /// their pre-ungroup locals; redo dissolves them again.
    UngroupEntities {
        containers: Vec<UngroupedContainer>,
    },

    /// Objects created by Insert / Paste / Duplicate — the general
    /// "creation" entry. Undo despawns the recorded entities, purges
    /// their records from every Fjall store and moves their folders to
    /// the reserved trash paths; redo renames the folders back and lets
    /// the space rescan respawn the whole subtree.
    ///
    /// This is `SpawnFolders` plus explicit entity ids. Paste/Duplicate
    /// spawn through `Commands`, so the entity is known at record time
    /// but its folder may not have reached `SpaceFileRegistry` yet —
    /// carrying the bits makes the despawn deterministic instead of
    /// registry-timing dependent.
    ///
    /// `paths` holds `(created_folder_path, reserved_trash_path)` pairs
    /// (identical shape + semantics to `SpawnFolders.folders`).
    /// `entities` holds `Entity::to_bits()` values, NOT `Entity` —
    /// `Action` derives `Serialize` and bevy_ecs's `serialize` feature
    /// is off in this build, so a bare `Entity` would not compile. The
    /// rest of this enum (`TransformEntities`, `ScaleEntities`, …) uses
    /// the same `u64` convention. Build one with
    /// [`Action::create_entities`], which does the conversion and
    /// reserves the trash paths for you.
    CreateEntities {
        paths: Vec<(std::path::PathBuf, std::path::PathBuf)>,
        entities: Vec<u64>,
    },

    /// Create ONE binary-ECS entity (the scalable Insert default, C1).
    /// Unlike `SpawnFolders`, a binary entity has NO disk folder — its
    /// authoritative state is a rkyv core in Fjall (Morton + identity
    /// indices). Undo despawns the entity and purges all five stores;
    /// redo re-spawns it. `def_json` is the serialized `InstanceDefinition`
    /// (carrying its uuid), so redo restores the SAME entity (same uuid →
    /// same `stored_id`), keeping identity continuous across undo cycles.
    CreateBinaryInstance {
        stored_id: u64,
        def_json: String,
    },

    /// Feature-tree edit on a CadPart — variable change (including the
    /// Size-driven resize path), constraint add, sketch solve, and
    /// suppress / reorder / delete-feature tree ops. Undo/redo swap
    /// the whole tree TOML: mesh regeneration rides the same
    /// `Changed<CadPart>` path as live edits, and features.toml is
    /// rewritten so the disk mirror never fights the in-memory tree.
    CadTreeEdit {
        entity_bits: u64,
        old_toml: String,
        new_toml: String,
        /// History verb ("Set length", "Suppress Extrude1", …).
        verb: String,
    },

    /// Assembly mate creation (in-memory Avian joint entity). Undo
    /// despawns the joint — located by matching the spec rather than a
    /// stored entity id, so undo→redo→undo cycles survive id churn.
    /// Redo re-fires `CadCreateMateEvent` with undo-recording off.
    CadMateCreate {
        spec_json: String,
    },
}

/// Snapshot of a property value for undo/redo
/// (`PartialEq` so producers can skip pushing no-op edits — e.g. the
/// Properties panel committing an unchanged field on blur.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValueSnapshot {
    String(String),
    Float(f32),
    Bool(bool),
    Vector3([f32; 3]),
    Color([f32; 4]),
    Material(String),
}

/// One entity's slice of an Explorer drag-drop reparent
/// (`Action::ReparentEntities`).
///
/// Entities are identified by `Entity::to_bits()` rather than `Entity`
/// because `Action` derives `Serialize` and bevy_ecs's `serialize`
/// feature is off in this build — the same reason `TransformEntities`
/// and `ScaleEntities` carry `u64`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReparentEntry {
    /// The dragged entity, `Entity::to_bits()`.
    pub entity_bits: u64,
    /// Its `ChildOf` parent BEFORE the drag (`None` = scene root).
    pub old_parent_bits: Option<u64>,
    /// Its `ChildOf` parent AFTER the drag — the drop target
    /// (`None` = scene root).
    pub new_parent_bits: Option<u64>,
    /// The folder/file that was moved, at its pre-drag location. This
    /// is `src_entry` in `do_reparent_node`: the FOLDER for folder-form
    /// entities (never the `_instance.toml` inside it), the file itself
    /// for flat files.
    pub old_path: std::path::PathBuf,
    /// The same folder/file at its post-drag location (`dest`).
    pub new_path: std::path::PathBuf,
}

/// One member of a Group (`Action::GroupEntities`) or one child raised
/// out of a dissolved container (`Action::UngroupEntities`).
///
/// `old_*` is the LOCAL transform the entity had before the operation,
/// `new_*` the local transform it was given by it — Group/Ungroup both
/// rewrite locals so the entity's WORLD transform is preserved across
/// the reparent, so undo must restore the local, not the world, value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    /// The member entity, `Entity::to_bits()`.
    pub entity_bits: u64,
    /// The member's `ChildOf` parent before the operation
    /// (`None` = scene root).
    ///
    /// Meaningful for `GroupEntities` only. For `UngroupEntities` the
    /// pre-op parent is by definition the dissolved container, which
    /// undo re-spawns under a fresh `Entity` id — so undo reparents to
    /// the entity it just spawned and ignores this field.
    pub old_parent_bits: Option<u64>,
    pub old_translation: [f32; 3],
    /// Quaternion in `Quat::to_array()` order — `[x, y, z, w]`.
    pub old_rotation: [f32; 4],
    pub old_scale: [f32; 3],
    pub new_translation: [f32; 3],
    /// Quaternion in `Quat::to_array()` order — `[x, y, z, w]`.
    pub new_rotation: [f32; 4],
    pub new_scale: [f32; 3],
}

/// One container dissolved by Ctrl+U (`Action::UngroupEntities`).
///
/// Enough state to re-spawn the container exactly: undo recreates it
/// from `class_name` + `name` + its local transform, re-parents it to
/// `parent_bits`, then pulls `children` back inside using each child's
/// `old_*` local transform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UngroupedContainer {
    /// `ClassName::as_str()` of the dissolved container — `"Model"` or
    /// `"Folder"`. Parsed back with `ClassName::from_str`.
    pub class_name: String,
    /// The container's `Instance.name`.
    pub name: String,
    /// The container's OWN parent before it was despawned — where its
    /// children were raised to (`None` = scene root).
    pub parent_bits: Option<u64>,
    /// The container's local transform.
    pub translation: [f32; 3],
    /// Quaternion in `Quat::to_array()` order — `[x, y, z, w]`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// The children it held, with their pre- and post-ungroup locals.
    pub children: Vec<GroupMember>,
}

impl Action {
    /// Stable topic key used for Eustress Stream publication + history
    /// panel filtering. One word per variant so subscribers can pattern
    /// match on `history.<kind>` without parsing descriptions.
    pub fn topic_kind(&self) -> &'static str {
        match self {
            Action::CreatePart { .. }             => "create",
            Action::DeletePart { .. }             => "delete",
            Action::MovePart { .. }               => "move",
            Action::RotatePart { .. }             => "rotate",
            Action::ScalePart { .. }              => "scale",
            Action::ChangeColor { .. }            => "color",
            Action::GroupParts { .. }             => "group",
            Action::UngroupParts { .. }           => "ungroup",
            Action::Batch { .. }                  => "batch",
            Action::ChangeProperty { .. }         => "property",
            Action::ChangePropertyMulti { .. }    => "property",
            Action::ChangeParameters { .. }       => "parameters",
            Action::ChangeParametersMulti { .. }  => "parameters",
            Action::ChangeFolderDomain { .. }     => "domain",
            Action::ChangeFolderSyncConfig { .. } => "sync",
            Action::ChangeAttributes { .. }       => "attributes",
            Action::ChangeTags { .. }             => "tags",
            Action::AddAttribute { .. }           => "attributes",
            Action::RemoveAttribute { .. }        => "attributes",
            Action::AddTag { .. }                 => "tags",
            Action::RemoveTag { .. }              => "tags",
            Action::TransformEntities { .. }      => "transform",
            Action::ScaleEntities { .. }          => "scale",
            Action::TrashEntities { .. }          => "delete",
            Action::SpawnFolders { .. }           => "create",
            Action::MoveFolders { .. }            => "reparent",
            Action::ReparentEntities { .. }       => "reparent",
            Action::GroupEntities { .. }          => "group",
            Action::UngroupEntities { .. }        => "ungroup",
            Action::CreateEntities { .. }         => "create",
            Action::CreateBinaryInstance { .. }   => "create",
            Action::CadTreeEdit { .. }            => "cad",
            Action::CadMateCreate { .. }          => "create",
        }
    }

    /// Get a human-readable description of the action
    pub fn description(&self) -> String {
        match self {
            Action::CreatePart { .. } => "Create Part".to_string(),
            Action::DeletePart { .. } => "Delete Part".to_string(),
            Action::MovePart { .. } => "Move Part".to_string(),
            Action::RotatePart { .. } => "Rotate Part".to_string(),
            Action::ScalePart { .. } => "Scale Part".to_string(),
            Action::ChangeColor { .. } => "Change Color".to_string(),
            Action::GroupParts { child_ids, .. } => format!("Group {} Parts", child_ids.len()),
            Action::UngroupParts { child_ids, .. } => format!("Ungroup {} Parts", child_ids.len()),
            Action::Batch { actions } => format!("Batch ({} actions)", actions.len()),
            Action::ChangeProperty { property, .. } => format!("Change {}", property),
            Action::ChangePropertyMulti { entities, property, .. } => format!("Change {} on {} objects", property, entities.len()),
            Action::ChangeParameters { .. } => "Change Parameters".to_string(),
            Action::ChangeParametersMulti { entities, .. } => format!("Change Parameters on {} objects", entities.len()),
            Action::ChangeFolderDomain { new_domain, .. } => {
                match new_domain {
                    Some(d) => format!("Set domain to '{}'", d),
                    None => "Clear domain".to_string(),
                }
            }
            Action::ChangeFolderSyncConfig { .. } => "Change Folder sync config".to_string(),
            Action::ChangeAttributes { .. } => "Change Attributes".to_string(),
            Action::ChangeTags { .. } => "Change Tags".to_string(),
            Action::AddAttribute { key, .. } => format!("Add attribute '{}'", key),
            Action::RemoveAttribute { key, .. } => format!("Remove attribute '{}'", key),
            Action::AddTag { tag, .. } => format!("Add tag '{}'", tag),
            Action::RemoveTag { tag, .. } => format!("Remove tag '{}'", tag),
            Action::TransformEntities { old_transforms, .. } => format!("Transform {} objects", old_transforms.len()),
            Action::ScaleEntities { old_states, .. } => format!("Scale {} objects", old_states.len()),
            Action::TrashEntities { paths, .. } => format!("Delete {} objects", paths.len()),
            Action::SpawnFolders { folders, .. } => format!("Spawn {} objects", folders.len()),
            Action::MoveFolders { moves } => format!("Move {} objects", moves.len()),
            Action::ReparentEntities { entries } => format!("Reparent {} objects", entries.len()),
            Action::GroupEntities { members, .. } => format!("Group {} objects", members.len()),
            Action::UngroupEntities { containers } => format!(
                "Ungroup {} container{}",
                containers.len(),
                if containers.len() == 1 { "" } else { "s" },
            ),
            Action::CreateEntities { paths, entities } => {
                format!("Create {} objects", paths.len().max(entities.len()))
            }
            Action::CreateBinaryInstance { .. } => "Create object".to_string(),
            Action::CadTreeEdit { verb, .. } => verb.clone(),
            Action::CadMateCreate { .. } => "Create mate".to_string(),
        }
    }

    /// Reserve the trash path a freshly-created folder gets moved to
    /// when its creation is undone:
    /// `<space_root>/.eustress/trash/<UTC stamp>/<folder name>`.
    ///
    /// Reserving it at RECORD time (rather than searching the trash at
    /// undo time) is what keeps undo and redo symmetric single-rename
    /// operations — the same reason `TrashEntities` stores both halves
    /// of the pair. The stamp has sub-second resolution so two objects
    /// created in the same second never collide.
    pub fn reserve_trash_path(space_root: &Path, folder: &Path) -> PathBuf {
        space_root
            .join(".eustress")
            .join("trash")
            .join(chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string())
            .join(folder.file_name().unwrap_or_default())
    }

    /// Build a `SpawnFolders` entry for newly-created folders, reserving
    /// each trash path. Use this from array tools / Smart Build Tools /
    /// anything that creates folders but has no entity ids to hand;
    /// use [`Action::create_entities`] when you DO have them.
    pub fn spawn_folders(space_root: &Path, folders: &[PathBuf]) -> Action {
        Action::SpawnFolders {
            folders: folders
                .iter()
                .map(|f| (f.clone(), Self::reserve_trash_path(space_root, f)))
                .collect(),
        }
    }

    /// Build a `CreateEntities` entry — the one undo record Insert,
    /// Paste and Duplicate should push. Reserves a trash path per
    /// folder and converts `Entity` → `Entity::to_bits()` so callers
    /// never have to think about the serialization constraint on the
    /// variant.
    ///
    /// `folders` may be shorter than `entities` (or empty): entities
    /// with no folder of their own are still despawned by undo.
    pub fn create_entities(space_root: &Path, folders: &[PathBuf], entities: &[Entity]) -> Action {
        Action::CreateEntities {
            paths: folders
                .iter()
                .map(|f| (f.clone(), Self::reserve_trash_path(space_root, f)))
                .collect(),
            entities: entities.iter().map(|e| e.to_bits()).collect(),
        }
    }
}

/// Snapshot payload for a single pushed Action, queued for publication
/// to the `"history.<kind>"` Eustress Stream topic. The history-stream
/// bridge (`history_stream.rs`) drains this each frame + tees events
/// into the in-process stream so MCP/CLI/LSP subscribers see every
/// mutation in sequential order without touching `UndoStack` directly.
#[derive(Debug, Clone)]
pub struct PendingHistoryStreamEvent {
    pub topic: String,
    pub kind: &'static str,
    pub description: String,
    pub label: Option<String>,
    /// Monotonic sequence number across the program's lifetime — lets
    /// subscribers detect gaps if the stream restarts.
    pub sequence: u64,
}

/// Undo/Redo stack resource
#[derive(Resource, Default)]
pub struct UndoStack {
    /// Stack of undoable actions
    history: VecDeque<Action>,
    /// Parallel stack of human-readable labels for each action —
    /// displayed in the History panel and toast hints (e.g.
    /// `"Linear Array (24 parts)"`, `"Align Y Center (5 parts)"`).
    /// `None` = action shown by its structural name only.
    labels: VecDeque<Option<String>>,
    /// Current position in history (for redo)
    current_index: usize,
    /// Monotonic push counter. Increments on every `push_internal`
    /// regardless of trim/pop; subscribers use it to detect lost
    /// events across restarts.
    push_sequence: u64,
    /// Events queued for the `"history.<kind>"` Eustress Stream topic
    /// but not yet drained. `history_stream.rs` drains + clears.
    pending_stream: Vec<PendingHistoryStreamEvent>,
}

impl UndoStack {
    /// Push a new action onto the stack
    pub fn push(&mut self, action: Action) {
        self.push_internal(action, None);
    }

    /// Push with a human-readable label. Same semantics as `push`
    /// otherwise. Tools that bulk-mutate many entities should use
    /// this so the user sees one meaningful entry per operation
    /// instead of a run of generic "Transform" entries.
    pub fn push_labeled(&mut self, label: impl Into<String>, action: Action) {
        self.push_internal(action, Some(label.into()));
    }

    fn push_internal(&mut self, action: Action, label: Option<String>) {
        // Remove any actions after current index (they were undone)
        self.history.truncate(self.current_index);
        self.labels.truncate(self.current_index);

        // Queue the stream-publication payload before we move `action`
        // into the deque. `history_stream.rs` tees these into the
        // in-process EustressStream on the `history.<kind>` topic.
        self.push_sequence = self.push_sequence.wrapping_add(1);
        let kind = action.topic_kind();
        self.pending_stream.push(PendingHistoryStreamEvent {
            topic: format!("history.{}", kind),
            kind,
            description: action.description(),
            label: label.clone(),
            sequence: self.push_sequence,
        });

        // Add new action
        self.history.push_back(action);
        self.labels.push_back(label);

        // Maintain max size
        if self.history.len() > MAX_HISTORY_SIZE {
            self.history.pop_front();
            self.labels.pop_front();
        } else {
            self.current_index += 1;
        }
    }

    /// Drain queued stream events. Called by `publish_history_stream`
    /// in `history_stream.rs` once per frame.
    pub fn drain_pending_stream(&mut self) -> Vec<PendingHistoryStreamEvent> {
        std::mem::take(&mut self.pending_stream)
    }

    /// Monotonic push counter — increments on every push, unaffected
    /// by undo/redo/trim. Subscribers (History stream, Bliss
    /// contribution tracker) compare it across frames to detect that
    /// new undoable work happened.
    pub fn sequence(&self) -> u64 {
        self.push_sequence
    }

    /// Topic-kind for the action at `index`, if it exists. Used by the
    /// History panel row renderer to drive the topic chip + filter.
    pub fn topic_kind_at(&self, index: usize) -> Option<&'static str> {
        self.history.get(index).map(|a| a.topic_kind())
    }

    /// Remove a single action at `index` after applying its inverse.
    /// Returns the action so the caller (World-access system) can feed
    /// it to `apply_undo_action`. After removal, `current_index` shifts
    /// down if it was past the removed slot — the rest of the history
    /// stays intact, so subsequent redo targets remain reachable.
    ///
    /// This is the backing operation for the History panel's
    /// `"Undo This Event"` right-click action.
    pub fn take_at(&mut self, index: usize) -> Option<Action> {
        if index >= self.history.len() { return None; }
        let removed = self.history.remove(index);
        let _ = self.labels.remove(index);
        if self.current_index > index {
            self.current_index -= 1;
        }
        removed
    }

    /// Collect the actions that need to be undone to walk the cursor
    /// back to `target` (inclusive — `target` stays applied). Used by
    /// the History panel's `"Revert to Here"` right-click action.
    ///
    /// Returns them in reverse application order (newest first) so the
    /// caller can apply them with `apply_undo_action` in sequence.
    pub fn drain_until(&mut self, target: usize) -> Vec<Action> {
        let mut out = Vec::new();
        // `current_index` points one past the most-recently-applied
        // action. To keep `target` applied we stop when the cursor
        // equals `target + 1`.
        let stop = target.saturating_add(1);
        while self.current_index > stop && self.current_index > 0 {
            self.current_index -= 1;
            if let Some(action) = self.history.get(self.current_index).cloned() {
                out.push(action);
            }
        }
        out
    }

    /// Human-readable label for the action at `index`, or `None` if
    /// no label was attached at push time. History-panel code reads
    /// this to populate row text.
    pub fn label_at(&self, index: usize) -> Option<&str> {
        self.labels.get(index).and_then(|l| l.as_deref())
    }

    /// Label of the most recently pushed action (= top of the undo
    /// stack), if any. Convenience for toast messages.
    pub fn last_label(&self) -> Option<&str> {
        if self.current_index == 0 { return None; }
        self.label_at(self.current_index - 1)
    }
    
    /// Check if we can undo
    pub fn can_undo(&self) -> bool {
        self.current_index > 0
    }
    
    /// Check if we can redo
    pub fn can_redo(&self) -> bool {
        self.current_index < self.history.len()
    }
    
    /// Get the action to undo (if any)
    pub fn undo(&mut self) -> Option<Action> {
        if self.can_undo() {
            self.current_index -= 1;
            self.history.get(self.current_index).cloned()
        } else {
            None
        }
    }
    
    /// Get the action to redo (if any)
    pub fn redo(&mut self) -> Option<Action> {
        if self.can_redo() {
            let action = self.history.get(self.current_index).cloned();
            self.current_index += 1;
            action
        } else {
            None
        }
    }
    
    /// Get the current undo index
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Get a reference to the history deque
    pub fn history(&self) -> &VecDeque<Action> {
        &self.history
    }

    /// Clear the entire history
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_index = 0;
    }
    
    /// Get the description of the last action (for UI display)
    pub fn last_action_description(&self) -> Option<String> {
        if self.current_index > 0 {
            self.history.get(self.current_index - 1).map(|a| a.description())
        } else {
            None
        }
    }
    
    /// Get the description of the next redo action (for UI display)
    pub fn next_redo_description(&self) -> Option<String> {
        if self.can_redo() {
            self.history.get(self.current_index).map(|a| a.description())
        } else {
            None
        }
    }
}

/// Event to request undo
#[derive(Message)]
pub struct UndoEvent;

/// Event to request redo
#[derive(Message)]
pub struct RedoEvent;

/// Undo just the action at `index` (right-click → "Undo This Event").
/// The inverse is applied + the slot removed from the stack; the rest
/// of the history is preserved.
#[derive(Message)]
pub struct UndoSingleEvent { pub index: usize }

/// Revert the undo cursor back to `target` (right-click → "Revert to
/// Here"). Applies the inverse of every action between the current
/// cursor and `target`, newest first, but leaves them in the stack so
/// the user can redo forward again.
#[derive(Message)]
pub struct RevertToEvent { pub target: usize }

/// Plugin for undo/redo functionality
pub struct UndoPlugin;

impl Plugin for UndoPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<UndoStack>()
            .add_message::<UndoEvent>()
            .add_message::<RedoEvent>()
            .add_message::<UndoSingleEvent>()
            .add_message::<RevertToEvent>()
            .add_systems(Update, (
                handle_undo_events,
                handle_redo_events,
                handle_undo_single_events,
                handle_revert_to_events,
            ));
    }
}

/// System to handle undo events (Modern ECS with World access)
pub fn handle_undo_events(world: &mut World) {
    // Get events
    let mut undo_events = world.resource_mut::<Messages<UndoEvent>>();
    let events: Vec<_> = undo_events.drain().collect();
    drop(undo_events);
    
    if events.is_empty() {
        return;
    }
    
    let mut undo_stack = world.resource_mut::<UndoStack>();
    let actions: Vec<_> = events.iter().filter_map(|_| undo_stack.undo()).collect();
    drop(undo_stack);
    
    let had_actions = !actions.is_empty();
    
    for action in actions {
        info!("Undoing: {}", action.description());
        apply_undo_ecs(&action, world);
        
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.info(format!("↶ Undid: {}", action.description()));
    }
    
    // Show warning if there was nothing to undo
    if !events.is_empty() && !had_actions {
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.warning("Nothing to undo");
    }
}

/// System to handle redo events (Modern ECS with World access)
fn handle_redo_events(world: &mut World) {
    // Get events
    let mut redo_events = world.resource_mut::<Messages<RedoEvent>>();
    let events: Vec<_> = redo_events.drain().collect();
    drop(redo_events);
    
    if events.is_empty() {
        return;
    }
    
    let mut undo_stack = world.resource_mut::<UndoStack>();
    let actions: Vec<_> = events.iter().filter_map(|_| undo_stack.redo()).collect();
    drop(undo_stack);
    
    let had_actions = !actions.is_empty();
    
    for action in actions {
        info!("Redoing: {}", action.description());
        apply_redo_ecs(&action, world);
        
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.info(format!("↷ Redid: {}", action.description()));
    }
    
    // Show warning if there was nothing to redo
    if !events.is_empty() && !had_actions {
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.warning("Nothing to redo");
    }
}

/// Handle `UndoSingleEvent`: apply the inverse of a single entry at
/// `index` and remove it from the stack. Other history entries keep
/// their meaning — this is "reverse this one change, leave the rest".
pub fn handle_undo_single_events(world: &mut World) {
    let mut events = world.resource_mut::<Messages<UndoSingleEvent>>();
    let targets: Vec<usize> = events.drain().map(|e| e.index).collect();
    drop(events);
    if targets.is_empty() { return; }

    for idx in targets {
        let action = {
            let mut stack = world.resource_mut::<UndoStack>();
            stack.take_at(idx)
        };
        let Some(action) = action else { continue };
        info!("Undo-single [{}]: {}", idx, action.description());
        apply_undo_ecs(&action, world);
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.info(format!("↶ Reversed: {}", action.description()));
    }
}

/// Handle `RevertToEvent`: walk the cursor back to `target`, applying
/// each action's inverse in reverse order. Entries remain in the stack
/// so the user can redo forward again.
pub fn handle_revert_to_events(world: &mut World) {
    let mut events = world.resource_mut::<Messages<RevertToEvent>>();
    let targets: Vec<usize> = events.drain().map(|e| e.target).collect();
    drop(events);
    if targets.is_empty() { return; }

    for target in targets {
        let actions = {
            let mut stack = world.resource_mut::<UndoStack>();
            stack.drain_until(target)
        };
        let count = actions.len();
        for action in &actions {
            info!("Revert: {}", action.description());
            apply_undo_ecs(action, world);
        }
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        if count > 0 {
            notifications.info(format!("↶ Reverted {} change{}", count, if count == 1 { "" } else { "s" }));
        }
    }
}

/// Public function to apply undo action (called from keyboard shortcuts)
pub fn apply_undo_action(action: &Action, world: &mut World) {
    apply_undo_ecs(action, world);
}

/// Public function to apply redo action (called from keyboard shortcuts)
pub fn apply_redo_action(action: &Action, world: &mut World) {
    apply_redo_ecs(action, world);
}

/// Apply the inverse of an action using modern ECS (for undo)
fn apply_undo_ecs(action: &Action, world: &mut World) {
    #[allow(unused_imports)]
    use crate::classes::BasePart;
    
    match action {
        Action::DeletePart { data } => {
            // Undo delete = recreate entity
            let mut entity = world.spawn((
                crate::classes::Instance {
                    name: data.name.clone(),
                    class_name: crate::classes::ClassName::Part,
                    archivable: true,
                    id: data.id,
                    ..Default::default()
                },
                Name::new(data.name.clone()),
            ));
            
            // Add BasePart if it existed
            if let Some(transform_data) = data.parent {
                // Restore transform/basepart
                entity.insert(Transform::from_translation(Vec3::from(data.position)));
            }
        }
        Action::MovePart { id, old_position, .. } => {
            // Restore old position - query by ID and update Transform
            let mut query = world.query::<(&crate::classes::Instance, &mut Transform)>();
            for (instance, mut transform) in query.iter_mut(world) {
                if instance.id == *id {
                    transform.translation = *old_position;
                    break;
                }
            }
        }
        Action::ChangeProperty { id, property, old_value, .. } => {
            // Restore old property value
            apply_property_value_to_entity(*id, property, old_value, world);
        }
        Action::ChangePropertyMulti { entities, property, .. } => {
            // Restore old property values for each entity
            for (id, old_value) in entities {
                apply_property_value_to_entity(*id, property, old_value, world);
            }
        }
        Action::Batch { actions } => {
            // Undo all actions in reverse order
            for action in actions.iter().rev() {
                apply_undo_ecs(action, world);
            }
        }
        Action::ChangeParameters { id, old_params, .. } => {
            // Restore old Parameters
            apply_parameters_to_entity(*id, old_params, world);
        }
        Action::ChangeParametersMulti { entities, .. } => {
            // Restore old Parameters for each entity
            for (id, old_params) in entities {
                apply_parameters_to_entity(*id, old_params, world);
            }
        }
        Action::ChangeFolderDomain { id, old_domain, old_source_override, .. } => {
            apply_folder_domain(*id, old_domain.clone(), old_source_override.clone(), world);
        }
        Action::ChangeFolderSyncConfig { id, old_config, .. } => {
            apply_folder_sync_config(*id, old_config.clone(), world);
        }
        Action::ChangeAttributes { id, old_attrs, .. } => {
            apply_attributes_to_entity(*id, old_attrs, world);
        }
        Action::ChangeTags { id, old_tags, .. } => {
            apply_tags_to_entity(*id, old_tags.clone(), world);
        }
        Action::AddAttribute { id, key, .. } => {
            // Undo add = remove
            remove_attribute_from_entity(*id, key, world);
        }
        Action::RemoveAttribute { id, key, old_value } => {
            // Undo remove = add back
            add_attribute_to_entity(*id, key, old_value, world);
        }
        Action::AddTag { id, tag } => {
            // Undo add = remove
            remove_tag_from_entity(*id, tag, world);
        }
        Action::RemoveTag { id, tag } => {
            // Undo remove = add back
            add_tag_to_entity(*id, tag, world);
        }
        Action::TransformEntities { old_transforms, .. } => {
            // Restore old transforms
            for (entity_bits, old_pos, old_rot) in old_transforms {
                let entity = Entity::from_bits(*entity_bits);
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*old_pos);
                        transform.rotation = Quat::from_array(*old_rot);
                    }
                    // Also update BasePart.cframe if present
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*old_pos);
                        bp.cframe.rotation = Quat::from_array(*old_rot);
                    }
                }
            }
        }
        Action::ScaleEntities { old_states, .. } => {
            // Restore old positions and sizes for every entity
            // recorded in this scale group.
            //
            // Two paths depending on how the part stores its size:
            //   * **File-system-first** (entity has `MeshSource`,
            //     loaded from a `.glb`): the unit-scale GLB mesh is
            //     authoritative; world size = `Transform.scale`. We
            //     restore by setting `transform.scale = old_size` and
            //     **leave the mesh handle alone** — overwriting the
            //     `Mesh3d` with a fresh `Cuboid::from_size` (what the
            //     legacy branch below does) destroys the .glb mesh and
            //     leaves a Cuboid scaled by the old size, producing
            //     the "undo grows to a random bigger size" symptom
            //     the user reported.
            //   * **Legacy primitive** (no `MeshSource`): mesh is
            //     baked at `BasePart.size`; `Transform.scale = ONE`.
            //     We bake a fresh primitive mesh at `old_size` and
            //     reset `transform.scale = ONE`.
            for (entity_bits, old_pos, old_size) in old_states {
                let entity = Entity::from_bits(*entity_bits);
                let size = Vec3::from_array(*old_size);

                let has_mesh_source = world.get::<crate::spawn::MeshSource>(entity).is_some();
                let part_shape = world.get::<crate::classes::Part>(entity)
                    .map(|p| p.shape);

                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*old_pos);
                        if has_mesh_source {
                            // GLB mesh is unit-scale; world size lives
                            // entirely in `Transform.scale`.
                            transform.scale = size;
                        } else {
                            // Primitive: mesh holds the size; scale
                            // stays at ONE so a stale mid-drag ratio
                            // doesn't double-apply.
                            transform.scale = Vec3::ONE;
                        }
                    }
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*old_pos);
                        bp.size = size;
                    }
                }

                // Regenerate primitive mesh at restored size — only
                // for legacy (non-`MeshSource`) parts. Touching the
                // Mesh3d handle on a file-system-first part would
                // overwrite its `.glb` with a Cuboid (the bug above).
                if !has_mesh_source {
                    if let Some(shape) = part_shape {
                        let new_mesh = world.resource_scope(|_world, mut meshes: Mut<Assets<Mesh>>| {
                            match shape {
                                crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                                crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                                crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                                _ => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                            }
                        });
                        if let Some(mut mesh3d) = world.get_mut::<Mesh3d>(entity) {
                            mesh3d.0 = new_mesh;
                        }
                    }
                }
            }
        }
        Action::TrashEntities { paths } => {
            // Undo delete: move files back from trash to original location.
            // Folder renames are atomic on disk but the platform watcher
            // emits a single Create event for the destination dir — not
            // recursive Creates for every restored child file. That left
            // descendant entities (a child BillboardGui under a restored
            // Part) unspawned and Ctrl+Z appearing broken. Trigger a
            // full SpaceRescan once below so the file loader walks every
            // restored subtree and spawns every entity inside it.
            let mut any_restored = false;
            for (original_path, trash_path) in paths {
                if trash_path.exists() {
                    if let Some(parent) = original_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::rename(trash_path, original_path) {
                        Ok(_) => {
                            info!("↶ Restored {:?} from trash", original_path.file_name().unwrap_or_default());
                            any_restored = true;
                        }
                        Err(e) => warn!("Failed to restore {:?}: {}", original_path, e),
                    }
                }
            }
            if any_restored {
                world.insert_resource(crate::space::space_ops::SpaceRescanNeeded(true));
            }
        }
        Action::SpawnFolders { folders } => {
            // Undo spawn: move the newly-created folders into the trash
            // AND despawn everything that lived inside them.
            //
            // Originally this was a bare `fs::rename` that left the
            // despawn to the file watcher — fine for the single-folder
            // CSG/Smart-Build case it was written for, but the watcher
            // emits one event per renamed folder, not a recursive
            // cascade, so any descendant entity (a Label under a
            // generated Part, an array element's child) survived as an
            // orphan pointing at a path that no longer exists. Routing
            // through `trash_created_folder` makes the variant usable
            // by array tools and Insert as well, with no change to the
            // payload — existing callers (csg.rs, tools_smart.rs,
            // cad_plugin.rs) keep working unmodified and simply get the
            // subtree cleanup for free.
            for (original_path, trash_path) in folders.iter().rev() {
                trash_created_folder(world, original_path, trash_path, &[]);
            }
        }
        Action::CreateEntities { paths, entities } => {
            // Undo create: same as SpawnFolders, plus the explicitly
            // recorded entity ids so Paste/Duplicate targets whose
            // folders have not reached the registry yet still go away.
            for (folder, trash_path) in paths.iter().rev() {
                trash_created_folder(world, folder, trash_path, entities);
            }
            // Entities with no folder of their own (in-memory paste
            // targets, service children) still have to be despawned.
            for bits in entities {
                let entity = Entity::from_bits(*bits);
                if world.get_entity(entity).is_ok() {
                    world.despawn(entity);
                }
            }
            mark_explorer_dirty(world);
        }
        Action::MoveFolders { moves } => {
            // Undo reparent, disk half: rename each folder back to
            // where it came from. Reverse order so nested moves unwind
            // outside-in — a child moved after its parent has to come
            // home before the parent does.
            for (original_path, new_path) in moves.iter().rev() {
                move_folder_and_rebase(world, new_path, original_path);
            }
        }
        Action::ReparentEntities { entries } => {
            // Undo reparent, ECS half. The disk call is the
            // self-healing no-op described on the variant: it returns
            // immediately when a companion `MoveFolders` entry already
            // put the folder back.
            for entry in entries.iter().rev() {
                move_folder_and_rebase(world, &entry.new_path, &entry.old_path);
                set_parent(world, entry.entity_bits, entry.old_parent_bits);
            }
            mark_explorer_dirty(world);
        }
        Action::GroupEntities { members, .. } => {
            // Undo group: detach every member back to its original
            // parent + local transform FIRST, then despawn the wrapper.
            // Order matters — Bevy despawns hierarchies recursively, so
            // despawning a still-populated wrapper would take the
            // members with it.
            let wrapper = container_of(world, members);
            for m in members {
                set_local_transform(
                    world,
                    m.entity_bits,
                    m.old_translation,
                    m.old_rotation,
                    m.old_scale,
                );
                set_parent(world, m.entity_bits, m.old_parent_bits);
            }
            // Resolved into a `let` first: a temporary closure borrowing
            // `world` inside the `match` scrutinee would stay alive for
            // the whole match and collide with the `despawn` below.
            let live_wrapper = wrapper.filter(|e| world.get_entity(*e).is_ok());
            match live_wrapper {
                Some(e) => {
                    world.despawn(e);
                    info!("↶ Undo group: despawned wrapper, freed {} members", members.len());
                }
                None => warn!("↶ Undo group: wrapper not found (members already reparented?)"),
            }
            mark_explorer_dirty(world);
        }
        Action::UngroupEntities { containers } => {
            // Undo ungroup = re-group: re-spawn each dissolved
            // container and pull its children back inside with the
            // local transforms they had before the dissolve.
            for c in containers.iter().rev() {
                let container = spawn_container(
                    world,
                    &c.class_name,
                    &c.name,
                    c.parent_bits,
                    c.translation,
                    c.rotation,
                    c.scale,
                );
                for child in &c.children {
                    set_local_transform(
                        world,
                        child.entity_bits,
                        child.old_translation,
                        child.old_rotation,
                        child.old_scale,
                    );
                    set_parent(world, child.entity_bits, Some(container.to_bits()));
                }
                info!(
                    "↶ Undo ungroup: re-created '{}' with {} children",
                    c.name,
                    c.children.len(),
                );
            }
            mark_explorer_dirty(world);
        }
        Action::CreateBinaryInstance { stored_id, def_json } => {
            // Undo create: purge the Fjall core + identity indices, then
            // despawn the live entity. (No files — binary entities have
            // none.) The entity is found by its stable `stored_id`, which
            // survives redo (redo re-uses the stored uuid), so repeated
            // undo/redo cycles stay correct.
            #[cfg(feature = "world-db")]
            {
                if let Ok(def) = serde_json::from_str::<crate::space::instance_loader::InstanceDefinition>(def_json) {
                    let uuid_hex = def.metadata.uuid.clone().unwrap_or_default();
                    let uuid_bytes =
                        eustress_common::instance_create::uuid_hex_to_bytes(&uuid_hex)
                            .unwrap_or([0u8; 16]);
                    let class = def.metadata.class_name.clone();
                    let synthetic_rel = format!(
                        "Workspace/__bin_{}_{:016x}/_instance.toml",
                        class, stored_id
                    );
                    crate::space::active_db::delete_binary_instance(
                        *stored_id,
                        &uuid_bytes,
                        &class,
                        def.transform.position,
                        &synthetic_rel,
                    );
                }
                let sid = *stored_id;
                let mut q = world
                    .query::<(Entity, &crate::space::world_db_binary::BinaryEcsInstance)>();
                let target = q
                    .iter(world)
                    .find(|(_, b)| b.stored_id == sid)
                    .map(|(e, _)| e);
                if let Some(e) = target {
                    world.despawn(e);
                    info!("↶ Undo create: despawned + purged binary entity {:016x}", sid);
                }
            }
        }
        Action::CadTreeEdit { entity_bits, old_toml, .. } => {
            apply_cad_tree_toml(world, *entity_bits, old_toml);
        }
        Action::CadMateCreate { spec_json } => {
            // Undo create = despawn the joint entity matching the spec.
            // Matched by content, not stored id — redo recreates the
            // joint under a fresh entity id.
            if let Ok(spec) = serde_json::from_str::<crate::cad_assembly::MateSpec>(spec_json) {
                let mut q = world.query::<(Entity, &crate::cad_assembly::CadMate)>();
                let target = q
                    .iter(world)
                    .find(|(_, m)| {
                        m.kind == spec.kind
                            && m.part_a.to_bits() == spec.part_a
                            && m.part_b.to_bits() == spec.part_b
                    })
                    .map(|(e, _)| e);
                if let Some(e) = target {
                    world.despawn(e);
                    info!("↶ Undo mate: despawned {:?} joint", spec.kind);
                } else {
                    warn!("↶ Undo mate: no matching {:?} joint found", spec.kind);
                }
            }
        }
        _ => {
            warn!("Undo not yet implemented for: {}", action.description());
        }
    }
}

/// Swap a CadPart's feature tree and mirror it to features.toml —
/// shared by `CadTreeEdit` undo/redo. Setting `tree_toml` trips
/// `Changed<CadPart>` so the regenerate system rebuilds the mesh,
/// collider, and `BasePart.size` exactly as a live edit would.
fn apply_cad_tree_toml(world: &mut World, entity_bits: u64, toml: &str) {
    let entity = Entity::from_bits(entity_bits);
    let Some(mut cad) = world.get_mut::<crate::cad_plugin::CadPart>(entity) else {
        warn!("CadTreeEdit: entity {entity_bits:#x} has no CadPart (deleted?)");
        return;
    };
    cad.tree_toml = toml.to_string();
    let feat_path = world
        .get::<crate::space::instance_loader::InstanceFile>(entity)
        .and_then(|f| f.toml_path.parent().map(|p| p.join("features.toml")));
    if let Some(path) = feat_path {
        if let Err(e) = std::fs::write(&path, toml) {
            warn!("CadTreeEdit: failed to write {:?}: {e}", path);
        }
    }
}

// ============================================================================
// Structural-undo helpers (folder moves, hierarchy edits, creation)
//
// Every structural variant below — MoveFolders, ReparentEntities,
// SpawnFolders, CreateEntities, GroupEntities, UngroupEntities — is
// built out of these. They are deliberately idempotent and
// "all-or-nothing": if the on-disk half can't be made to happen, the
// helper logs loudly and returns `false` WITHOUT touching ECS state, so
// a failed undo leaves the engine consistent with the disk instead of
// half-applied.
// ============================================================================

/// Rename `from` → `to`, retrying a few times before giving up.
///
/// Windows transiently locks a folder for ~50–200 ms after Bevy's asset
/// server drops a `.glb` handle; inside that window `fs::rename` returns
/// `Os { code: 5, kind: PermissionDenied }`. The Delete path
/// (`keybindings.rs`) learned this the expensive way — its old
/// `remove_dir_all` fallback "succeeded" against the already-released
/// handle and destroyed the only copy of the data. So: no fallback, 5
/// attempts 60 ms apart, and on exhaustion log loudly and return `false`
/// so the caller leaves everything untouched and the user can retry.
fn rename_with_retry(from: &Path, to: &Path) -> bool {
    const ATTEMPTS: u32 = 5;
    let mut last_err: Option<std::io::Error> = None;
    for i in 0..ATTEMPTS {
        match std::fs::rename(from, to) {
            Ok(_) => {
                if i > 0 {
                    info!(
                        "↔ Renamed {:?} → {:?} after {} retr{}",
                        from.file_name().unwrap_or_default(),
                        to.file_name().unwrap_or_default(),
                        i,
                        if i == 1 { "y" } else { "ies" },
                    );
                }
                return true;
            }
            Err(e) => {
                last_err = Some(e);
                if i + 1 < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }
            }
        }
    }
    warn!(
        "❌ Could not rename {:?} → {:?} after {} attempts ({}). \
         Leaving state untouched — retry the undo/redo.",
        from,
        to,
        ATTEMPTS,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
    );
    false
}

/// Stage both ends of a rename in `SpaceFileRegistry.rename_in_progress`
/// so the watcher swallows the delete+create pair `notify` emits on
/// Windows instead of despawning and respawning the entity. Folder-form
/// entities are watched via their `_instance.toml`, so that path is
/// staged too. Mirrors `slint_ui::do_reparent_node`; entries are cleared
/// by the file_watcher as it processes the events.
fn guard_rename(world: &mut World, from: &Path, to: &Path, is_dir: bool) {
    let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() else {
        return;
    };
    registry.rename_in_progress.insert(from.to_path_buf());
    registry.rename_in_progress.insert(to.to_path_buf());
    if is_dir {
        registry.rename_in_progress.insert(from.join("_instance.toml"));
        registry.rename_in_progress.insert(to.join("_instance.toml"));
    }
}

/// Undo `guard_rename` — called only when the rename FAILED, so the
/// watcher resumes tracking the paths it was told to ignore.
fn unguard_rename(world: &mut World, from: &Path, to: &Path, is_dir: bool) {
    let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() else {
        return;
    };
    registry.rename_in_progress.remove(from);
    registry.rename_in_progress.remove(to);
    if is_dir {
        registry.rename_in_progress.remove(&from.join("_instance.toml"));
        registry.rename_in_progress.remove(&to.join("_instance.toml"));
    }
}

/// Flag the Explorer for an immediate re-sync. Structural undo changes
/// the tree's shape without going through the file watcher, so nothing
/// else marks the panel stale.
fn mark_explorer_dirty(world: &mut World) {
    if let Some(mut es) = world.get_resource_mut::<crate::ui::slint_ui::UnifiedExplorerState>() {
        es.dirty = true;
        es.needs_immediate_sync = true;
    }
}

/// Move `from` → `to` on disk and bring every in-memory reference along.
///
/// This is the exact inverse of the bookkeeping `do_reparent_node` does
/// after its forward rename: rebase `InstanceFile.toml_path` /
/// `LoadedFromFile.path` on the moved entity AND every descendant
/// (a `SimpleBlock/Label/_instance.toml` under a moved `SimpleBlock`),
/// then rekey the registry — folder-form entities are DOUBLE-registered
/// under the folder and its `_instance.toml`, both pointing at the same
/// entity, and missing either key leaves the Explorer resolving the
/// entity at its old slot.
///
/// Idempotent by design: when the folder is already at `to` (a companion
/// `Batch` entry beat us to the rename) the move is skipped but the
/// rebase still runs, which is itself a no-op if already applied. That
/// is what makes `MoveFolders` and `ReparentEntities` safe to push
/// together, in either order, or alone.
///
/// Returns `false` — having logged — if the rename could not be made, in
/// which case NOTHING was mutated.
fn move_folder_and_rebase(world: &mut World, from: &Path, to: &Path) -> bool {
    if from == to {
        return true;
    }

    let needs_move = from.exists();
    if needs_move && to.exists() {
        warn!(
            "↔ Move: destination {:?} already exists — leaving {:?} where it is",
            to, from,
        );
        return false;
    }
    if !needs_move && !to.exists() {
        warn!("↔ Move: neither {:?} nor {:?} exists — nothing to move", from, to);
        return false;
    }
    // `is_dir` must be sampled on whichever side actually exists —
    // once the entry has moved, `from.is_dir()` reads false even though
    // it is the same logical folder.
    let is_dir = if needs_move { from.is_dir() } else { to.is_dir() };

    if needs_move {
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        guard_rename(world, from, to, is_dir);
        if !rename_with_retry(from, to) {
            unguard_rename(world, from, to, is_dir);
            return false;
        }
    }

    // Rebase path references. Iterating the full component set is O(N)
    // per move, but structural undo is user-initiated and rare — worth
    // it to avoid maintaining a separate descendant index.
    {
        let mut q = world.query::<&mut crate::space::instance_loader::InstanceFile>();
        for mut f in q.iter_mut(world) {
            let rebased = if f.toml_path.as_path() == from {
                Some(to.to_path_buf())
            } else {
                f.toml_path.strip_prefix(from).ok().map(|rel| to.join(rel))
            };
            if let Some(p) = rebased {
                f.toml_path = p;
            }
        }
    }
    {
        let mut q = world.query::<&mut crate::space::LoadedFromFile>();
        for mut lff in q.iter_mut(world) {
            let rebased = if lff.path.as_path() == from {
                Some(to.to_path_buf())
            } else {
                lff.path.strip_prefix(from).ok().map(|rel| to.join(rel))
            };
            if let Some(p) = rebased {
                lff.path = p;
            }
        }
    }

    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        let _ = registry.rename_file(from, to.to_path_buf());
        if is_dir {
            let _ = registry.rename_file(
                &from.join("_instance.toml"),
                to.join("_instance.toml"),
            );
        }
    }

    mark_explorer_dirty(world);
    true
}

/// Restore an entity's `ChildOf` edge. `None` = detach to the scene
/// root, matching `grouping.rs`'s `remove::<ChildOf>()`.
fn set_parent(world: &mut World, entity_bits: u64, parent_bits: Option<u64>) {
    let entity = Entity::from_bits(entity_bits);
    if world.get_entity(entity).is_err() {
        warn!("↶ Reparent: entity {entity_bits:#x} no longer exists — skipping");
        return;
    }
    match parent_bits {
        Some(bits) => {
            let parent = Entity::from_bits(bits);
            if world.get_entity(parent).is_ok() {
                world.entity_mut(entity).insert(ChildOf(parent));
            } else {
                // The old parent is gone — its own creation was undone
                // first, or a rescan respawned it under a new id.
                // Detaching to the root is the honest fallback; keeping
                // the stale edge would strand the entity inside a
                // despawned hierarchy where nothing can reach it.
                warn!("↶ Reparent: parent {bits:#x} is gone — detaching {entity_bits:#x} to root");
                world.entity_mut(entity).remove::<ChildOf>();
            }
        }
        None => {
            world.entity_mut(entity).remove::<ChildOf>();
        }
    }
}

/// Restore an entity's LOCAL transform. Group/Ungroup rewrite locals to
/// preserve world transforms across a reparent, so the local is exactly
/// what has to be put back — restoring a world transform here would make
/// the object jump by the container's offset.
fn set_local_transform(
    world: &mut World,
    entity_bits: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
) {
    let entity = Entity::from_bits(entity_bits);
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
        return;
    };
    entity_mut.insert(Transform {
        translation: Vec3::from_array(translation),
        rotation: Quat::from_array(rotation),
        scale: Vec3::from_array(scale),
    });
}

/// Find the container currently wrapping `members` — the `ChildOf`
/// parent of the first member still alive.
///
/// Group/Ungroup containers are found by content rather than by a stored
/// `Entity`: redo re-spawns the container under a fresh id, so a stored
/// id would go stale after a single undo→redo cycle. Same strategy
/// `CadMateCreate` uses to locate its joint.
fn container_of(world: &World, members: &[GroupMember]) -> Option<Entity> {
    members.iter().find_map(|m| {
        let entity = Entity::from_bits(m.entity_bits);
        world.get::<ChildOf>(entity).map(|c| c.0)
    })
}

/// Spawn a Group/Ungroup container (`Model` / `Folder`) with the exact
/// component set `grouping.rs` gives it, optionally parented.
fn spawn_container(
    world: &mut World,
    class_name: &str,
    name: &str,
    parent_bits: Option<u64>,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
) -> Entity {
    let class = crate::classes::ClassName::from_str(class_name)
        .unwrap_or(crate::classes::ClassName::Model);
    let container = world
        .spawn((
            crate::classes::Instance {
                name: name.to_string(),
                class_name: class,
                archivable: true,
                id: 0,
                ..Default::default()
            },
            Transform {
                translation: Vec3::from_array(translation),
                rotation: Quat::from_array(rotation),
                scale: Vec3::from_array(scale),
            },
            Visibility::default(),
            Name::new(name.to_string()),
        ))
        .id();
    if let Some(bits) = parent_bits {
        let parent = Entity::from_bits(bits);
        if world.get_entity(parent).is_ok() {
            world.entity_mut(container).insert(ChildOf(parent));
        }
    }
    container
}

/// Undo a creation: move `folder` to its reserved `trash` path and tear
/// down every entity that lived inside it.
///
/// Why not just rename and let the file watcher despawn? Because the
/// watcher emits ONE event for the folder, not a recursive cascade, so
/// descendants (a `BillboardGui` under a pasted Part) would survive as
/// orphans whose queued save-on-`Changed` writes then fail against the
/// now-missing directory. We therefore despawn the whole registered
/// subtree ourselves, purge each path from every Fjall store (clearing
/// only the tree records resurrects the object on the next reconcile),
/// and stage the paths in `rename_in_progress` so the watcher does not
/// fire a second, redundant despawn.
///
/// `known_bits` covers entities spawned this frame that the registry has
/// not caught up with yet — Paste/Duplicate go through `Commands`, so
/// their folder registration can lag the undo record by a frame.
///
/// Returns `false` if the rename failed; ECS state is left untouched.
fn trash_created_folder(
    world: &mut World,
    folder: &Path,
    trash: &Path,
    known_bits: &[u64],
) -> bool {
    if !folder.exists() {
        // Already gone — a companion entry beat us to it, or the user
        // deleted the object by hand. Nothing to undo on disk.
        return false;
    }

    let registered: Vec<(PathBuf, Entity)> = world
        .get_resource::<crate::space::SpaceFileRegistry>()
        .map(|r| r.descendants_of(folder))
        .unwrap_or_default();

    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        for (path, _) in &registered {
            registry.rename_in_progress.insert(path.clone());
        }
        registry.rename_in_progress.insert(folder.to_path_buf());
    }

    if let Some(parent) = trash.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !rename_with_retry(folder, trash) {
        if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
            for (path, _) in &registered {
                registry.rename_in_progress.remove(path);
            }
            registry.rename_in_progress.remove(folder);
        }
        return false;
    }

    // Rename landed — bring ECS + DB down to match the moved subtree.
    let mut victims: Vec<(Option<PathBuf>, Entity)> = registered
        .iter()
        .map(|(p, e)| (Some(p.clone()), *e))
        .collect();
    for bits in known_bits {
        let entity = Entity::from_bits(*bits);
        if !victims.iter().any(|(_, v)| *v == entity) {
            victims.push((None, entity));
        }
    }

    for (path, entity) in victims {
        // Capture identity BEFORE the despawn so every uuid-keyed store
        // can be purged.
        let (uuid_hex, class) = world
            .get::<crate::classes::Instance>(entity)
            .map(|i| (i.uuid.clone(), i.class_name.as_str().to_string()))
            .unwrap_or_default();
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
        match path {
            Some(p) => {
                crate::space::active_db::purge_path_all_stores(&p, &uuid_hex, &class);
                if let Some(mut registry) =
                    world.get_resource_mut::<crate::space::SpaceFileRegistry>()
                {
                    registry.unregister_file(&p);
                }
            }
            None => {
                if let Some(mut registry) =
                    world.get_resource_mut::<crate::space::SpaceFileRegistry>()
                {
                    registry.unregister_entity(entity);
                }
            }
        }
    }

    info!(
        "↶ Undo create: trashed {:?} + despawned its subtree",
        folder.file_name().unwrap_or_default(),
    );
    mark_explorer_dirty(world);
    true
}

/// Redo a creation: rename the folder back out of the trash and request
/// a full `SpaceRescan`.
///
/// A rescan rather than trusting the watcher, for the same reason
/// `TrashEntities` undo does it: a folder rename produces a single
/// Create event for the destination directory, NOT recursive Creates for
/// every restored child, so descendant entities would never respawn.
fn restore_created_folder(world: &mut World, folder: &Path, trash: &Path) -> bool {
    if !trash.exists() {
        return false;
    }
    if folder.exists() {
        warn!("↷ Redo create: {:?} already exists — skipping restore", folder);
        return false;
    }
    if let Some(parent) = folder.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !rename_with_retry(trash, folder) {
        return false;
    }
    info!("↷ Restored {:?} from trash", folder.file_name().unwrap_or_default());
    world.insert_resource(crate::space::space_ops::SpaceRescanNeeded(true));
    mark_explorer_dirty(world);
    true
}

/// Apply an action using modern ECS (for redo)
fn apply_redo_ecs(action: &Action, world: &mut World) {
    match action {
        Action::DeletePart { data } => {
            // Redo delete = despawn entity
            let mut query = world.query::<(Entity, &crate::classes::Instance)>();
            let entity_to_despawn: Option<Entity> = query
                .iter(world)
                .find(|(_, instance)| instance.id == data.id)
                .map(|(entity, _)| entity);
            
            if let Some(entity) = entity_to_despawn {
                world.despawn(entity);
            }
        }
        Action::MovePart { id, new_position, .. } => {
            // Apply new position
            let mut query = world.query::<(&crate::classes::Instance, &mut Transform)>();
            for (instance, mut transform) in query.iter_mut(world) {
                if instance.id == *id {
                    transform.translation = *new_position;
                    break;
                }
            }
        }
        Action::ChangeProperty { id, property, new_value, .. } => {
            // Apply new property value
            apply_property_value_to_entity(*id, property, new_value, world);
        }
        Action::ChangePropertyMulti { entities, property, new_value } => {
            // Apply new property value to all entities
            for (id, _) in entities {
                apply_property_value_to_entity(*id, property, new_value, world);
            }
        }
        Action::Batch { actions } => {
            // Redo all actions in order
            for action in actions {
                apply_redo_ecs(action, world);
            }
        }
        Action::ChangeParameters { id, new_params, .. } => {
            // Apply new Parameters
            apply_parameters_to_entity(*id, new_params, world);
        }
        Action::ChangeParametersMulti { entities, new_params } => {
            // Apply new Parameters to all entities
            for (id, _) in entities {
                apply_parameters_to_entity(*id, new_params, world);
            }
        }
        Action::ChangeFolderDomain { id, new_domain, new_source_override, .. } => {
            apply_folder_domain(*id, new_domain.clone(), new_source_override.clone(), world);
        }
        Action::ChangeFolderSyncConfig { id, new_config, .. } => {
            apply_folder_sync_config(*id, new_config.clone(), world);
        }
        Action::ChangeAttributes { id, new_attrs, .. } => {
            apply_attributes_to_entity(*id, new_attrs, world);
        }
        Action::ChangeTags { id, new_tags, .. } => {
            apply_tags_to_entity(*id, new_tags.clone(), world);
        }
        Action::AddAttribute { id, key, value } => {
            // Redo add = add
            add_attribute_to_entity(*id, key, value, world);
        }
        Action::RemoveAttribute { id, key, .. } => {
            // Redo remove = remove
            remove_attribute_from_entity(*id, key, world);
        }
        Action::AddTag { id, tag } => {
            // Redo add = add
            add_tag_to_entity(*id, tag, world);
        }
        Action::RemoveTag { id, tag } => {
            // Redo remove = remove
            remove_tag_from_entity(*id, tag, world);
        }
        Action::TransformEntities { new_transforms, .. } => {
            // Apply new transforms
            for (entity_bits, new_pos, new_rot) in new_transforms {
                let entity = Entity::from_bits(*entity_bits);
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*new_pos);
                        transform.rotation = Quat::from_array(*new_rot);
                    }
                    // Also update BasePart.cframe if present
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*new_pos);
                        bp.cframe.rotation = Quat::from_array(*new_rot);
                    }
                }
            }
        }
        Action::ScaleEntities { new_states, .. } => {
            // Symmetric to the undo branch — see its comment block for
            // the file-system-first vs. legacy split rationale. Redo
            // re-applies the post-scale state.
            for (entity_bits, new_pos, new_size) in new_states {
                let entity = Entity::from_bits(*entity_bits);
                let size = Vec3::from_array(*new_size);

                let has_mesh_source = world.get::<crate::spawn::MeshSource>(entity).is_some();
                let part_shape = world.get::<crate::classes::Part>(entity)
                    .map(|p| p.shape);

                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*new_pos);
                        if has_mesh_source {
                            transform.scale = size;
                        } else {
                            transform.scale = Vec3::ONE;
                        }
                    }
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*new_pos);
                        bp.size = size;
                    }
                }

                if !has_mesh_source {
                    if let Some(shape) = part_shape {
                        let new_mesh = world.resource_scope(|_world, mut meshes: Mut<Assets<Mesh>>| {
                            match shape {
                                crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                                crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                                crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                                _ => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                            }
                        });
                        if let Some(mut mesh3d) = world.get_mut::<Mesh3d>(entity) {
                            mesh3d.0 = new_mesh;
                        }
                    }
                }
            }
        }
        Action::TrashEntities { paths } => {
            // Redo delete: move files back to trash
            for (original_path, trash_path) in paths {
                if original_path.exists() {
                    if let Some(parent) = trash_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::rename(original_path, trash_path);
                    info!("↷ Re-trashed {:?}", original_path.file_name().unwrap_or_default());
                }
            }
        }
        Action::SpawnFolders { folders } => {
            // Redo spawn: restore from trash back to the original
            // location. `restore_created_folder` requests a full
            // SpaceRescan rather than trusting the watcher, so
            // descendants inside the restored subtree respawn too.
            for (original_path, trash_path) in folders {
                restore_created_folder(world, original_path, trash_path);
            }
        }
        Action::CreateEntities { paths, .. } => {
            // Redo create: symmetric to the undo branch. Entity ids are
            // NOT reused — the rescan spawns fresh entities from the
            // restored files, so a further undo relies on the registry
            // lookup inside `trash_created_folder` rather than the
            // recorded bits (which is exactly why that lookup exists).
            for (folder, trash_path) in paths {
                restore_created_folder(world, folder, trash_path);
            }
        }
        Action::MoveFolders { moves } => {
            // Redo reparent, disk half: rename forward again, in the
            // original application order.
            for (original_path, new_path) in moves {
                move_folder_and_rebase(world, original_path, new_path);
            }
        }
        Action::ReparentEntities { entries } => {
            for entry in entries {
                move_folder_and_rebase(world, &entry.old_path, &entry.new_path);
                set_parent(world, entry.entity_bits, entry.new_parent_bits);
            }
            mark_explorer_dirty(world);
        }
        Action::GroupEntities {
            model_name,
            model_class,
            model_parent_bits,
            model_translation,
            members,
        } => {
            // Redo group: spawn a FRESH wrapper — the original died
            // with the undo — and re-apply each member's grouped local.
            // `grouping.rs` spawns the wrapper axis-aligned and
            // unit-scaled, so identity rotation / unit scale here.
            let wrapper = spawn_container(
                world,
                model_class,
                model_name,
                *model_parent_bits,
                *model_translation,
                Quat::IDENTITY.to_array(),
                Vec3::ONE.to_array(),
            );
            for m in members {
                set_local_transform(
                    world,
                    m.entity_bits,
                    m.new_translation,
                    m.new_rotation,
                    m.new_scale,
                );
                set_parent(world, m.entity_bits, Some(wrapper.to_bits()));
            }
            info!("↷ Redo group: wrapped {} objects in a {}", members.len(), model_class);
            mark_explorer_dirty(world);
        }
        Action::UngroupEntities { containers } => {
            // Redo ungroup: raise the children back out to the
            // container's own parent and despawn the container. It is
            // located by content (current parent of the first live
            // child) because undo re-spawned it under a new id.
            for c in containers {
                let container = container_of(world, &c.children);
                for child in &c.children {
                    set_local_transform(
                        world,
                        child.entity_bits,
                        child.new_translation,
                        child.new_rotation,
                        child.new_scale,
                    );
                    set_parent(world, child.entity_bits, c.parent_bits);
                }
                // See the undo branch: resolve before the `match` so no
                // borrow of `world` outlives the scrutinee.
                let live_container = container.filter(|e| world.get_entity(*e).is_ok());
                match live_container {
                    Some(e) => {
                        world.despawn(e);
                    }
                    None => warn!("↷ Redo ungroup: container '{}' not found", c.name),
                }
            }
            mark_explorer_dirty(world);
        }
        Action::CreateBinaryInstance { def_json, .. } => {
            // Redo create: queue the stored def for re-spawn. A dedicated
            // system (`drain_pending_binary_recreate`) does the actual
            // spawn next frame with proper system params (Commands + the
            // mesh/material resources) — re-using the def's uuid so the
            // SAME entity/stored_id comes back. Doing it here would need
            // Commands + several ResMut concurrently, which a raw
            // `&mut World` can't hand out at once.
            #[cfg(feature = "world-db")]
            {
                world
                    .get_resource_or_insert_with(
                        crate::space::world_db_binary::PendingBinaryRecreate::default,
                    )
                    .0
                    .push(def_json.clone());
            }
        }
        Action::CadTreeEdit { entity_bits, new_toml, .. } => {
            apply_cad_tree_toml(world, *entity_bits, new_toml);
        }
        Action::CadMateCreate { spec_json } => {
            // Redo create = re-fire the creation event with recording
            // off, so `handle_create_mate` doesn't push a duplicate
            // undo entry for a mate the stack already owns.
            if let Ok(spec) = serde_json::from_str::<crate::cad_assembly::MateSpec>(spec_json) {
                world.write_message(spec.to_event(false));
            }
        }
        _ => {
            warn!("Redo not yet implemented for: {}", action.description());
        }
    }
}

/// Apply a property value snapshot to an entity by ID
fn apply_property_value_to_entity(id: u32, property: &str, value: &PropertyValueSnapshot, world: &mut World) {
    use crate::classes::{BasePart, Instance};
    
    // Find entity by Instance ID
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for property undo/redo", id);
        return;
    };
    
    match (property, value) {
        ("Name", PropertyValueSnapshot::String(name)) => {
            if let Some(mut inst) = world.get_mut::<Instance>(entity) {
                inst.name = name.clone();
            }
        }
        ("Position", PropertyValueSnapshot::Vector3(pos)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.cframe.translation = Vec3::from_array(*pos);
            }
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.translation = Vec3::from_array(*pos);
            }
        }
        ("Orientation", PropertyValueSnapshot::Vector3(rot)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.cframe.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    rot[0].to_radians(),
                    rot[1].to_radians(),
                    rot[2].to_radians(),
                );
            }
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    rot[0].to_radians(),
                    rot[1].to_radians(),
                    rot[2].to_radians(),
                );
            }
        }
        ("Size", PropertyValueSnapshot::Vector3(size)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.size = Vec3::from_array(*size);
            }
        }
        // Properties-panel "Scale.X/Y/Z" edits write Transform.scale (GLB /
        // mesh entities size via scale, not BasePart.size) — restore the
        // same field. Without this arm those edits warned "unknown property"
        // on undo.
        ("Scale", PropertyValueSnapshot::Vector3(scale)) => {
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.scale = Vec3::from_array(*scale);
            }
        }
        ("Color", PropertyValueSnapshot::Color(rgba)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
        ("Material", PropertyValueSnapshot::Material(mat_str)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                // Parse material from string
                bp.material = match mat_str.as_str() {
                    "Plastic" => crate::classes::Material::Plastic,
                    "SmoothPlastic" => crate::classes::Material::SmoothPlastic,
                    "Wood" => crate::classes::Material::Wood,
                    "WoodPlanks" => crate::classes::Material::WoodPlanks,
                    "Metal" => crate::classes::Material::Metal,
                    "CorrodedMetal" => crate::classes::Material::CorrodedMetal,
                    "DiamondPlate" => crate::classes::Material::DiamondPlate,
                    "Foil" => crate::classes::Material::Foil,
                    "Grass" => crate::classes::Material::Grass,
                    "Concrete" => crate::classes::Material::Concrete,
                    "Brick" => crate::classes::Material::Brick,
                    "Granite" => crate::classes::Material::Granite,
                    "Marble" => crate::classes::Material::Marble,
                    "Slate" => crate::classes::Material::Slate,
                    "Sand" => crate::classes::Material::Sand,
                    "Fabric" => crate::classes::Material::Fabric,
                    "Glass" => crate::classes::Material::Glass,
                    "Neon" => crate::classes::Material::Neon,
                    "Ice" => crate::classes::Material::Ice,
                    _ => crate::classes::Material::Plastic,
                };
            }
        }
        ("Transparency", PropertyValueSnapshot::Float(t)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.transparency = *t;
            }
        }
        ("Reflectance", PropertyValueSnapshot::Float(r)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.reflectance = *r;
            }
        }
        ("Anchored", PropertyValueSnapshot::Bool(a)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.anchored = *a;
            }
        }
        ("CanCollide", PropertyValueSnapshot::Bool(c)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.can_collide = *c;
            }
        }
        ("CanTouch", PropertyValueSnapshot::Bool(ct)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.can_touch = *ct;
            }
        }
        ("Locked", PropertyValueSnapshot::Bool(l)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.locked = *l;
            }
        }
        ("Destructible", PropertyValueSnapshot::Bool(d)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.destructible = *d;
            }
        }
        ("FieldOfView", PropertyValueSnapshot::Float(deg)) => {
            if let Some(mut proj) = world.get_mut::<Projection>(entity) {
                if let Projection::Perspective(p) = proj.as_mut() {
                    p.fov = deg.clamp(1.0, 170.0).to_radians();
                }
            }
        }
        ("NearClipPlane", PropertyValueSnapshot::Float(v)) => {
            if let Some(mut proj) = world.get_mut::<Projection>(entity) {
                if let Projection::Perspective(p) = proj.as_mut() {
                    p.near = v.clamp(0.001, 100.0);
                }
            }
        }
        ("FarClipPlane", PropertyValueSnapshot::Float(v)) => {
            if let Some(mut proj) = world.get_mut::<Projection>(entity) {
                if let Projection::Perspective(p) = proj.as_mut() {
                    p.far = v.clamp(100.0, 1_000_000.0);
                }
            }
        }
        _ => {
            warn!("Unknown property for undo/redo: {}", property);
        }
    }
}

/// Apply serialized Parameters JSON to an entity by ID
fn apply_parameters_to_entity(id: u32, params_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::parameters::Parameters;
    
    // Find entity by Instance ID
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Parameters undo/redo", id);
        return;
    };
    
    // Deserialize Parameters from JSON
    match serde_json::from_str::<Parameters>(params_json) {
        Ok(params) => {
            // Insert or replace Parameters component
            world.entity_mut(entity).insert(params);
            info!("Applied Parameters to entity {}", id);
        }
        Err(e) => {
            warn!("Failed to deserialize Parameters for undo/redo: {}", e);
        }
    }
}

/// Create an undo action for a Parameters change
pub fn create_parameters_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_params: &eustress_common::parameters::Parameters,
) -> Option<Action> {
    let old_json = serde_json::to_string(old_params).ok()?;
    let new_json = serde_json::to_string(new_params).ok()?;
    
    Some(Action::ChangeParameters {
        id,
        old_params: old_json,
        new_params: new_json,
    })
}

/// Create an undo action for Parameters changes on multiple entities
pub fn create_parameters_multi_change_action(
    entities_old: Vec<(u32, &eustress_common::parameters::Parameters)>,
    new_params: &eustress_common::parameters::Parameters,
) -> Option<Action> {
    let new_json = serde_json::to_string(new_params).ok()?;
    
    let entities: Vec<(u32, String)> = entities_old
        .into_iter()
        .filter_map(|(id, params)| {
            serde_json::to_string(params).ok().map(|json| (id, json))
        })
        .collect();
    
    if entities.is_empty() {
        return None;
    }
    
    Some(Action::ChangeParametersMulti {
        entities,
        new_params: new_json,
    })
}

// ============================================================================
// Folder Domain/SyncConfig Helpers
// ============================================================================

/// Apply domain configuration to an entity's Parameters
fn apply_folder_domain(id: u32, domain: Option<String>, source_override: Option<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::parameters::Parameters;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for domain undo/redo", id);
        return;
    };
    
    if let Some(mut params) = world.get_mut::<Parameters>(entity) {
        params.domain = domain.unwrap_or_default();
        params.global_source_ref = source_override;
        info!("Applied domain to entity {}", id);
    } else {
        warn!("Entity {} does not have Parameters component", id);
    }
}

/// Apply sync config to an entity's Parameters
fn apply_folder_sync_config(id: u32, config_json: Option<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::parameters::{Parameters, DomainSyncConfig};
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for sync config undo/redo", id);
        return;
    };
    
    if let Some(mut params) = world.get_mut::<Parameters>(entity) {
        params.sync_config = config_json
            .and_then(|json| serde_json::from_str::<DomainSyncConfig>(&json).ok());
        info!("Applied sync config to entity {}", id);
    } else {
        warn!("Entity {} does not have Parameters component", id);
    }
}

// ============================================================================
// Attributes Helpers
// ============================================================================

/// Apply serialized attributes to an entity
fn apply_attributes_to_entity(id: u32, attrs_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::{Attributes, AttributeValue};
    use std::collections::HashMap;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Attributes undo/redo", id);
        return;
    };
    
    match serde_json::from_str::<HashMap<String, AttributeValue>>(attrs_json) {
        Ok(values) => {
            let mut attrs = Attributes::new();
            for (key, value) in values {
                attrs.set(&key, value);
            }
            world.entity_mut(entity).insert(attrs);
            info!("Applied Attributes to entity {}", id);
        }
        Err(e) => {
            warn!("Failed to deserialize Attributes for undo/redo: {}", e);
        }
    }
}

/// Add a single attribute to an entity
fn add_attribute_to_entity(id: u32, key: &str, value_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::{Attributes, AttributeValue};
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for add attribute", id);
        return;
    };
    
    if let Ok(value) = serde_json::from_str::<AttributeValue>(value_json) {
        if let Some(mut attrs) = world.get_mut::<Attributes>(entity) {
            attrs.set(key, value);
        } else {
            let mut attrs = Attributes::new();
            attrs.set(key, value);
            world.entity_mut(entity).insert(attrs);
        }
    }
}

/// Remove a single attribute from an entity
fn remove_attribute_from_entity(id: u32, key: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Attributes;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for remove attribute", id);
        return;
    };
    
    if let Some(mut attrs) = world.get_mut::<Attributes>(entity) {
        attrs.remove(key);
    }
}

// ============================================================================
// Tags Helpers
// ============================================================================

/// Apply tags to an entity
fn apply_tags_to_entity(id: u32, tag_list: Vec<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Tags undo/redo", id);
        return;
    };
    
    let mut tags = Tags::new();
    for tag in tag_list {
        tags.add(&tag);
    }
    world.entity_mut(entity).insert(tags);
    info!("Applied Tags to entity {}", id);
}

/// Add a single tag to an entity
fn add_tag_to_entity(id: u32, tag: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for add tag", id);
        return;
    };
    
    if let Some(mut tags) = world.get_mut::<Tags>(entity) {
        tags.add(tag);
    } else {
        let mut tags = Tags::new();
        tags.add(tag);
        world.entity_mut(entity).insert(tags);
    }
}

/// Remove a single tag from an entity
fn remove_tag_from_entity(id: u32, tag: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for remove tag", id);
        return;
    };
    
    if let Some(mut tags) = world.get_mut::<Tags>(entity) {
        tags.remove(tag);
    }
}

// ============================================================================
// Action Creation Helpers
// ============================================================================

/// Create an undo action for a domain change (now uses Parameters)
pub fn create_folder_domain_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_domain: Option<String>,
    new_source_override: Option<String>,
) -> Action {
    Action::ChangeFolderDomain {
        id,
        old_domain: if old_params.domain.is_empty() { None } else { Some(old_params.domain.clone()) },
        new_domain,
        old_source_override: old_params.global_source_ref.clone(),
        new_source_override,
    }
}

/// Create an undo action for a sync config change (now uses Parameters)
pub fn create_folder_sync_config_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_config: Option<&eustress_common::classes::DomainSyncConfig>,
) -> Option<Action> {
    let old_config = old_params.sync_config.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let new_config = new_config
        .and_then(|c| serde_json::to_string(c).ok());
    
    Some(Action::ChangeFolderSyncConfig {
        id,
        old_config,
        new_config,
    })
}

/// Create an undo action for adding an attribute
pub fn create_add_attribute_action(
    id: u32,
    key: String,
    value: &eustress_common::attributes::AttributeValue,
) -> Option<Action> {
    let value_json = serde_json::to_string(value).ok()?;
    Some(Action::AddAttribute { id, key, value: value_json })
}

/// Create an undo action for removing an attribute
pub fn create_remove_attribute_action(
    id: u32,
    key: String,
    old_value: &eustress_common::attributes::AttributeValue,
) -> Option<Action> {
    let old_value_json = serde_json::to_string(old_value).ok()?;
    Some(Action::RemoveAttribute { id, key, old_value: old_value_json })
}

/// Create an undo action for adding a tag
pub fn create_add_tag_action(id: u32, tag: String) -> Action {
    Action::AddTag { id, tag }
}

/// Create an undo action for removing a tag
pub fn create_remove_tag_action(id: u32, tag: String) -> Action {
    Action::RemoveTag { id, tag }
}

/// OLD LEGACY IMPLEMENTATION (kept for reference)
/// Apply the inverse of an action (for undo)
#[allow(dead_code, unused_variables)]
fn apply_undo(_action: &Action, _part_manager: &BevyPartManager) {
    // use crate::parts::PartUpdate;
    
    // OLD IMPLEMENTATION USING LEGACY BevyPartManager:
    /*
    
    match _action {
        Action::CreatePart { id, .. } => {
            // Undo create = delete
            let pm = _part_manager.0.write();
            let _ = pm.delete_part(*id);
        }
        Action::DeletePart { data } => {
            // Undo delete = recreate
            let pm = _part_manager.0.write();
            let new_id = pm.create_part(data.part_type, Vec3::from(data.position), Some(data.name.clone()));
            // Restore parent relationship if it had one
            if let Some(parent_id) = data.parent {
                let _ = pm.update_part(new_id, PartUpdate {
                    parent: Some(Some(parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::MovePart { id, old_position, .. } => {
            // Restore old position
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                position: Some(old_position.to_array()),
                ..Default::default()
            });
        }
        Action::RotatePart { id, old_rotation, .. } => {
            // Restore old rotation
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                rotation: Some(old_rotation.to_array()),
                ..Default::default()
            });
        }
        Action::ScalePart { id, old_scale, .. } => {
            // Restore old scale
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                size: Some(old_scale.to_array()),
                ..Default::default()
            });
        }
        Action::ChangeColor { id, old_color, .. } => {
            // Restore old color
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                color: Some(*old_color),
                ..Default::default()
            });
        }
        Action::GroupParts { child_ids, old_parents, .. } => {
            // Restore old parent relationships
            let pm = _part_manager.0.write();
            for (child_id, old_parent) in child_ids.iter().zip(old_parents.iter()) {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(*old_parent),
                    ..Default::default()
                });
            }
        }
        Action::UngroupParts { parent_id, child_ids, .. } => {
            // Restore grouped state - undo ungroup means re-group
            let pm = _part_manager.0.write();
            for child_id in child_ids {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::Batch { actions } => {
            // Undo batch in reverse order
            for action in actions.iter().rev() {
                apply_undo(action, _part_manager);
            }
        }
    }
    */
}

/// Apply an action (for redo)
/// TODO: Refactor to use ECS queries instead of BevyPartManager
#[allow(dead_code, unused_variables)]
fn apply_redo(_action: &Action, _part_manager: &BevyPartManager) {
    // use crate::parts::PartUpdate;
    
    // TODO: Reimplement using ECS queries
    /* OLD IMPLEMENTATION USING LEGACY BevyPartManager:
    
    match _action {
        Action::CreatePart { part_type, position, parent, .. } => {
            // Redo create
            let pm = _part_manager.0.write();
            let new_id = pm.create_part(*part_type, *position, None);
            // Set parent relationship if specified
            if let Some(parent_id) = parent {
                let _ = pm.update_part(new_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::DeletePart { data } => {
            // Redo delete
            let pm = _part_manager.0.write();
            let _ = pm.delete_part(data.id);
        }
        Action::MovePart { id, new_position, .. } => {
            // Apply new position
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                position: Some(new_position.to_array()),
                ..Default::default()
            });
        }
        Action::RotatePart { id, new_rotation, .. } => {
            // Apply new rotation
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                rotation: Some(new_rotation.to_array()),
                ..Default::default()
            });
        }
        Action::ScalePart { id, new_scale, .. } => {
            // Apply new scale
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                size: Some(new_scale.to_array()),
                ..Default::default()
            });
        }
        Action::ChangeColor { id, new_color, .. } => {
            // Apply new color
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                color: Some(*new_color),
                ..Default::default()
            });
        }
        Action::GroupParts { parent_id, child_ids, .. } => {
            // Apply grouping
            let pm = _part_manager.0.write();
            for child_id in child_ids {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::UngroupParts { child_ids, new_parents, .. } => {
            // Apply ungrouping
            let pm = _part_manager.0.write();
            for (child_id, new_parent) in child_ids.iter().zip(new_parents.iter()) {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(*new_parent),
                    ..Default::default()
                });
            }
        }
        Action::Batch { actions } => {
            // Redo batch in order
            for action in actions {
                apply_redo(action, _part_manager);
            }
        }
    }
    */
}
