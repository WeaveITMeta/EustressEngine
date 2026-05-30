use bevy::prelude::*;
use bevy::prelude::Message;
use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::collections::HashMap;
use crate::classes::{
    Instance, ClassName, BasePart, Part, Model,
    EustressPointLight, EustressSpotLight, EustressDirectionalLight, SurfaceLight,
    Sound, Attachment, ParticleEmitter, Beam, Decal, SpecialMesh,
    BillboardGui, TextLabel,
};
use crate::serialization::scene::CurrentScenePath;
use crate::rendering::BevySelectionManager;

// ============================================================================
// Clipboard Serialization Types
// ============================================================================

/// Serializable entity data for clipboard operations.
///
/// ## Identity (Wave 2.1 / IDENTITY.md §11.2)
///
/// `uuid` is the persistent identity that survives across sessions, file
/// renames, cross-space copies, and audit-log references. It is the only
/// field the cross-space MOVE/COPY contract (IDENTITY.md §3.3 / §3.4)
/// operates on.
///
/// `id` is retained as the **session-local Bevy entity handle** for the
/// transient paste-batch parent linkage (`parent: Option<u32>`). It is not
/// persisted to disk and does not survive engine restart —
/// `Instance.uuid` is the authoritative identity, `Instance.id` is the
/// live ECS handle (per IDENTITY.md §11.7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntityData2 {
    /// Live Bevy entity handle (session-local). Used by the paste-batch
    /// parent linkage only — NOT a persistent identifier.
    pub id: u32,
    /// Persistent 32-char-hex UUID for this entity (IDENTITY.md §11.2).
    ///
    /// `#[serde(default)]` so pre-Wave-2.1 clipboards (or OS-clipboard
    /// payloads from another engine version) deserialize cleanly with
    /// an empty uuid — the paste path then routes through fresh-create
    /// surfaces (`instance_create::fresh_uuid_for_create` per §3.2)
    /// instead of cross-space MOVE/COPY, which is the correct fallback
    /// when no identity is known.
    #[serde(default)]
    pub uuid: String,
    /// Entity name
    pub name: String,
    /// Class name (Part, Model, etc.)
    pub class: String,
    /// Parent entity's Bevy handle (None for root entities). Same
    /// session-local caveat as `id`.
    pub parent: Option<u32>,
    /// Parent entity's persistent UUID (None for root entities).
    /// Mirrors `parent` for the cross-session identity surface.
    /// Defaults to `None` on legacy clipboards.
    #[serde(default)]
    pub parent_uuid: Option<String>,
    /// Transform data
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    /// All properties as key-value pairs
    pub properties: HashMap<String, serde_json::Value>,
    /// Parameters (if any)
    pub parameters: Option<serde_json::Value>,
    /// Raw TOML source — populated for non-visual service children (Sky,
    /// Atmosphere, Star/Sun, Moon, etc.) so paste can write an exact copy
    /// to the target Space's service folder without re-deriving properties.
    #[serde(default)]
    pub source_toml: Option<String>,
    /// Service folder name this entity belongs to (e.g. "Lighting").
    /// Empty for Workspace entities.
    #[serde(default)]
    pub service_folder: String,
    /// On-disk folder of the source entity. Set when the entity is
    /// folder-backed (i.e. `_instance.toml` lives inside its own
    /// directory, and any descendants live in subdirectories of that
    /// directory — the file-system-first convention). When this is
    /// `Some`, paste does a recursive directory copy so the entire
    /// subtree (BillboardGui → TextLabel, Model → Parts, etc.) is
    /// duplicated — not just the selected root. Property-only paste
    /// applies when this is `None` (e.g. flat-file entities or
    /// in-memory-only instances without an on-disk folder).
    ///
    /// Stored as a string for `Serialize`/`Deserialize` friendliness;
    /// `PathBuf` doesn't carry through serde JSON cleanly on Windows.
    #[serde(default)]
    pub source_folder_path: Option<String>,
}

// ============================================================================
// Clipboard Entity Types
// ============================================================================

/// Stored entity data for clipboard - supports all entity types
#[derive(Clone)]
pub struct ClipboardEntity {
    /// Core instance data (required for all entities)
    pub instance: Instance,
    /// Entity name
    pub name: String,
    /// Transform at time of copy
    pub transform: Transform,
    /// Entity-specific data
    pub data: ClipboardEntityData,
    /// Children (for Models/Folders)
    pub children: Vec<ClipboardEntity>,
    /// Original entity ID (for hierarchy reconstruction)
    pub original_entity: Option<Entity>,
}

/// Entity-specific data variants
#[derive(Clone)]
pub enum ClipboardEntityData {
    /// Part with BasePart and Part components
    Part {
        basepart: BasePart,
        part: Part,
    },
    /// Model container (children stored separately)
    Model {
        model: Model,
    },
    /// Folder container
    Folder,
    /// Point light
    PointLight {
        light: EustressPointLight,
    },
    /// Spot light
    SpotLight {
        light: EustressSpotLight,
    },
    /// Directional light
    DirectionalLight {
        light: EustressDirectionalLight,
    },
    /// Surface light
    SurfaceLight {
        light: SurfaceLight,
    },
    /// Sound
    Sound {
        sound: Sound,
    },
    /// Attachment
    Attachment {
        attachment: Attachment,
    },
    /// Particle emitter
    ParticleEmitter {
        emitter: ParticleEmitter,
    },
    /// Beam
    Beam {
        beam: Beam,
    },
    /// Decal
    Decal {
        decal: Decal,
    },
    /// Special mesh
    SpecialMesh {
        mesh: SpecialMesh,
    },
    /// Billboard GUI
    BillboardGui {
        gui: BillboardGui,
    },
    /// Text label
    TextLabel {
        label: TextLabel,
    },
    /// Generic/unknown entity (just transform)
    Generic,
}

impl ClipboardEntity {
    /// Create a new clipboard entity from components
    pub fn new(instance: Instance, name: String, transform: Transform, data: ClipboardEntityData) -> Self {
        Self {
            instance,
            name,
            transform,
            data,
            children: Vec::new(),
            original_entity: None,
        }
    }
    
    /// Add a child entity
    pub fn add_child(&mut self, child: ClipboardEntity) {
        self.children.push(child);
    }
    
    /// Check if this is a container (Model/Folder)
    pub fn is_container(&self) -> bool {
        matches!(self.data, ClipboardEntityData::Model { .. } | ClipboardEntityData::Folder)
    }
    
    /// Get the bounding box top (for stacking)
    pub fn get_top(&self) -> f32 {
        match &self.data {
            ClipboardEntityData::Part { basepart, .. } => {
                self.transform.translation.y + basepart.size.y * 0.5
            }
            _ => self.transform.translation.y + 0.5, // Default 1 unit height
        }
    }
    
    /// Get the bounding box bottom
    pub fn get_bottom(&self) -> f32 {
        match &self.data {
            ClipboardEntityData::Part { basepart, .. } => {
                self.transform.translation.y - basepart.size.y * 0.5
            }
            _ => self.transform.translation.y - 0.5,
        }
    }
    
    /// Get center position
    pub fn get_center(&self) -> Vec3 {
        self.transform.translation
    }
}

// ============================================================================
// Clipboard Resource
// ============================================================================

/// Clipboard resource for storing copied entities
#[derive(Resource)]
pub struct Clipboard {
    /// Copied entities (flat list, hierarchy preserved in children)
    pub entities: Vec<ClipboardEntity>,
    /// Track the top of the last paste for proper stacking
    pub last_paste_top: f32,
    /// Center of copied selection (for relative positioning)
    pub copy_center: Vec3,
    /// Paste offset counter (for multiple pastes)
    pub paste_count: u32,
    /// Original entity IDs that were copied (for checking if still selected)
    pub copied_entity_ids: Vec<String>,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            last_paste_top: f32::NEG_INFINITY,
            copy_center: Vec3::ZERO,
            paste_count: 0,
            copied_entity_ids: Vec::new(),
        }
    }
}

// ============================================================================
// Editor Clipboard - Cross-Scene Support
// ============================================================================

/// Paste mode for cross-scene operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasteMode {
    /// Paste with original IDs (may conflict)
    #[default]
    Normal,
    /// Regenerate all entity IDs to avoid conflicts
    NewIds,
    /// Duplicate in place — new IDs, zero offset
    DuplicateInPlace,
    /// Paste cancelled
    Cancelled,
}

/// Cross-scene paste modal state
#[derive(Debug, Clone, Default)]
pub struct CrossScenePasteModal {
    /// Whether the modal is open
    pub open: bool,
    /// Source scene name for display
    pub source_scene_name: String,
    /// User's choice
    pub choice: Option<PasteMode>,
}

/// Enhanced clipboard with cross-scene support and serialization.
///
/// ## Identity model (Wave 2.1 / IDENTITY.md §3.3, §3.4, §11.2)
///
/// COPY operations regenerate every pasted entity's UUID per §3.4
/// `blake3(source_uuid ‖ target_space_id ‖ copy_counter_be8)[..16]`;
/// MOVE operations preserve the source UUID verbatim per §3.3. The
/// `paste_count` field doubles as the §3.4 *copy_counter*: it starts at
/// 0 and is bumped after each paste, so the first paste sees `1`, the
/// second sees `2`, etc. — yielding 10 distinct UUIDs across 10 paste
/// presses against the same source. `clear()` (called on every fresh
/// Ctrl+C) resets `paste_count` back to 0, matching the §3.4 contract:
/// *"counter resets on the next ctrl-C"*.
#[derive(Resource)]
pub struct EditorClipboard {
    /// Serialized entity data
    pub entities: Vec<ClipboardEntityData2>,
    /// Source scene path (for cross-scene awareness)
    pub source_scene: Option<PathBuf>,
    /// Timestamp of copy operation
    pub copied_at: Option<std::time::Instant>,
    /// Include Parameters/Attributes/Tags in copy
    pub include_metadata: bool,
    /// Center of copied selection (for relative positioning)
    pub copy_center: Vec3,
    /// Paste offset counter — also the §3.4 *copy_counter*. Bumped per
    /// paste; reset to 0 on `clear()`.
    pub paste_count: u32,
    /// Original entity IDs that were copied
    pub copied_entity_ids: Vec<String>,
    /// Cross-scene paste modal state
    pub cross_scene_modal: CrossScenePasteModal,
    /// Cut mode (delete originals after paste)
    pub is_cut: bool,
    /// UUID mapping (source_uuid → minted_target_uuid) for cross-
    /// reference fix-up during a single paste batch. Per IDENTITY.md
    /// §11.2 this is the persistent-identity counterpart of the old
    /// session-local `HashMap<u32, u32>`. Cleared at the start of each
    /// remap pass (and on `clear()`).
    pub uuid_mapping: HashMap<String, String>,
}

impl Default for EditorClipboard {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            source_scene: None,
            copied_at: None,
            include_metadata: true,
            copy_center: Vec3::ZERO,
            paste_count: 0,
            copied_entity_ids: Vec::new(),
            cross_scene_modal: CrossScenePasteModal::default(),
            is_cut: false,
            uuid_mapping: HashMap::new(),
        }
    }
}

impl EditorClipboard {
    /// Check if clipboard is empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    
    /// Get entity count
    pub fn count(&self) -> usize {
        self.entities.len()
    }
    
    /// Clear the clipboard. Resets `paste_count` (the §3.4 copy_counter)
    /// to 0 so a subsequent Ctrl+C correctly starts a fresh counter — the
    /// "ten distinct uuids from ten ctrl-V" guarantee depends on this.
    pub fn clear(&mut self) {
        self.entities.clear();
        self.source_scene = None;
        self.copied_at = None;
        self.copy_center = Vec3::ZERO;
        self.paste_count = 0;
        self.copied_entity_ids.clear();
        self.is_cut = false;
        self.uuid_mapping.clear();
    }
    
    /// Check if this is a cross-scene paste
    pub fn is_cross_scene(&self, current_scene: Option<&PathBuf>) -> bool {
        match (&self.source_scene, current_scene) {
            (Some(source), Some(current)) => source != current,
            (Some(_), None) => true, // Pasting into unsaved scene
            (None, Some(_)) => true, // Copied from unsaved scene
            (None, None) => false,   // Both unsaved, same "scene"
        }
    }
    
    /// Get source scene name for display
    pub fn source_scene_name(&self) -> String {
        self.source_scene
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    }
    
    /// Get paste offset — places copy directly above the original.
    /// Uses the entity's Y size so parts stack flush with no gap.
    pub fn get_paste_offset(&self) -> Vec3 {
        let height = self.entities.iter()
            .filter_map(|e| e.properties.get("size").and_then(|v| v.as_array()))
            .map(|a: &Vec<serde_json::Value>| a.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32)
            .next()
            .unwrap_or(1.0);
        Vec3::new(0.0, height * (self.paste_count as f32 + 1.0), 0.0)
    }

    /// Increment paste counter
    pub fn increment_paste_count(&mut self) {
        self.paste_count += 1;
    }

    /// Reset paste counter (called after new copy)
    pub fn reset_paste_count(&mut self) {
        self.paste_count = 0;
    }
    
    /// Mint a fresh UUID for a cross-space COPY paste per IDENTITY.md §3.4.
    ///
    /// `new_uuid = hex(blake3(source_uuid_bytes ‖ 0x1f ‖ target_space_id_bytes
    /// ‖ 0x1f ‖ copy_counter.to_be_bytes())[..16])`
    ///
    /// - Two distinct sources → distinct uuids (different `source_uuid`).
    /// - Same source pasted into two different target spaces → distinct
    ///   uuids (different `target_space_id`). Why this matters: without
    ///   `target_space_id`, paste-into-SpaceB and paste-into-SpaceC of the
    ///   same source would collide if a future move-from-B-to-C landed.
    /// - Same source, same target, N consecutive pastes → N distinct
    ///   uuids (different `copy_counter`). The §3.4 contract: one Ctrl+C
    ///   followed by ten Ctrl+V presses must produce ten distinct
    ///   entities. The counter resets on the next Ctrl+C via `clear()`.
    /// - `source_uuid` may be empty (legacy entities with no uuid yet) —
    ///   in that case the hash still produces a fresh deterministic uuid
    ///   per-(target, counter), which is fine: the destination TOML is
    ///   written with that uuid and behaves identically to a fresh
    ///   create.
    pub fn mint_paste_uuid(
        source_uuid: &str,
        target_space_id: &[u8],
        copy_counter: u64,
    ) -> String {
        let mut seed =
            Vec::with_capacity(source_uuid.len() + 1 + target_space_id.len() + 1 + 8);
        seed.extend_from_slice(source_uuid.as_bytes());
        seed.push(0x1f);
        seed.extend_from_slice(target_space_id);
        seed.push(0x1f);
        seed.extend_from_slice(&copy_counter.to_be_bytes());
        let hash = blake3::hash(&seed);
        // 32-char lowercase hex per IDENTITY.md §7.3 — locked format forever.
        let mut out = String::with_capacity(32);
        for &b in &hash.as_bytes()[..16] {
            out.push(hex_nibble(b >> 4));
            out.push(hex_nibble(b & 0x0f));
        }
        out
    }

    /// Compute a stable `target_space_id` byte-blob from a Space root path
    /// (or any opaque path that names a Space). The bytes only need to be
    /// stable across the engine session — they feed `mint_paste_uuid` to
    /// distinguish two destination Spaces; no other consumer reads them.
    pub fn target_space_id_for(space_root: Option<&PathBuf>) -> Vec<u8> {
        space_root
            .map(|p| p.to_string_lossy().as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Remap UUIDs across the current clipboard batch per IDENTITY.md
    /// §3.3 / §3.4.
    ///
    /// `target_space_id` is the byte-form of the destination Space's
    /// identity (the Space root path bytes work fine — see
    /// `target_space_id_for`).
    ///
    /// Behaviour:
    ///
    /// - COPY (`self.is_cut == false`): every entity's `uuid` is
    ///   regenerated via [`mint_paste_uuid`] using
    ///   `copy_counter = paste_count + 1` (the §3.4 contract: counter
    ///   starts at 1 for the first paste). Within the same batch each
    ///   entity gets a distinct uuid because each has its own
    ///   `source_uuid`; if two entities in the batch happened to share
    ///   a source_uuid (only possible if the user copied an alias),
    ///   they would still differ because the closure also folds the
    ///   batch index into the counter.
    /// - MOVE (`self.is_cut == true`): uuids are preserved verbatim.
    ///   The destination receives the same uuid; the source row in the
    ///   Fjall partition + on-disk folder is removed by the paste
    ///   handler's post-paste trash step.
    ///
    /// In both branches `parent_uuid` is rewritten through the same map
    /// so within-batch parent linkage survives the rename.
    pub fn remap_uuids(&mut self, target_space_id: &[u8]) {
        self.uuid_mapping.clear();
        if self.is_cut {
            // MOVE — preserve uuids verbatim. We still populate the
            // mapping as identity so the parent-rewrite pass below is a
            // no-op rather than a panic on `Option<&String>`.
            for entity_data in &self.entities {
                if !entity_data.uuid.is_empty() {
                    self.uuid_mapping
                        .insert(entity_data.uuid.clone(), entity_data.uuid.clone());
                }
            }
            return;
        }

        // COPY — mint a fresh uuid per source per §3.4. The §3.4
        // copy_counter starts at 1 for the first paste; map it from
        // `paste_count` (which is bumped AFTER each paste, so reads as 0
        // on the first paste). The trailing `batch_idx` makes
        // within-batch collisions impossible even when two entities
        // share a source_uuid.
        let copy_counter_base = (self.paste_count as u64) + 1;
        for (batch_idx, entity_data) in self.entities.iter().enumerate() {
            let counter = copy_counter_base
                .wrapping_mul(0x100)
                .wrapping_add(batch_idx as u64);
            let new_uuid =
                Self::mint_paste_uuid(&entity_data.uuid, target_space_id, counter);
            // Only insert when source is non-empty — legacy clipboards
            // can have multiple empty-uuid entries and we don't want
            // them all mapping to the same minted value (each gets its
            // own fresh uuid by virtue of the differing batch_idx).
            if !entity_data.uuid.is_empty() {
                self.uuid_mapping.insert(entity_data.uuid.clone(), new_uuid);
            }
        }

        // Apply the new uuids to each entity + rewrite parent_uuid links.
        let copy_counter_base = (self.paste_count as u64) + 1;
        for (batch_idx, entity_data) in self.entities.iter_mut().enumerate() {
            if !entity_data.uuid.is_empty() {
                if let Some(new_uuid) = self.uuid_mapping.get(&entity_data.uuid) {
                    entity_data.uuid = new_uuid.clone();
                }
            } else {
                // Empty source uuid — synthesize a fresh one anyway so
                // the destination TOML carries a valid 32-hex uuid.
                let counter = copy_counter_base
                    .wrapping_mul(0x100)
                    .wrapping_add(batch_idx as u64);
                entity_data.uuid = Self::mint_paste_uuid("", target_space_id, counter);
            }

            // Parent uuid: rewrite if we have a mapping for it. A parent
            // outside the current batch keeps its uuid unchanged (the
            // user copied a leaf without its parent — the parent
            // already exists in the destination).
            if let Some(ref old_parent) = entity_data.parent_uuid {
                if let Some(new_parent) = self.uuid_mapping.get(old_parent) {
                    entity_data.parent_uuid = Some(new_parent.clone());
                }
            }
        }
    }
}

// ============================================================================
// UUID helpers — local to clipboard so we don't widen `eustress_common`
// for one call site. These are wire-compatible with the IDENTITY.md
// §7.3 format (32-char lowercase hex, no separators).
// ============================================================================

/// Map 0..16 → `'0'..'9' | 'a'..'f'`. IDENTITY.md §7.3 — lowercase only.
#[inline]
fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '0',
    }
}

impl Clipboard {
    /// Copy entities to clipboard
    pub fn copy(&mut self, entities: Vec<ClipboardEntity>) {
        // Calculate center of all entities
        if !entities.is_empty() {
            let mut center = Vec3::ZERO;
            let mut count = 0;
            for entity in &entities {
                center += entity.transform.translation;
                count += 1;
            }
            self.copy_center = center / count as f32;
        } else {
            self.copy_center = Vec3::ZERO;
        }
        
        self.entities = entities;
        self.last_paste_top = f32::NEG_INFINITY;
        self.paste_count = 0;
        // Note: copied_entity_ids should be set separately by the caller
    }
    
    /// Copy entities to clipboard with their original entity IDs
    pub fn copy_with_ids(&mut self, entities: Vec<ClipboardEntity>, entity_ids: Vec<String>) {
        self.copy(entities);
        self.copied_entity_ids = entity_ids;
    }
    
    /// Check if any of the originally copied entities are still selected
    pub fn are_originals_selected(&self, current_selection: &[String]) -> bool {
        // If no originals tracked, assume not selected
        if self.copied_entity_ids.is_empty() {
            return false;
        }
        // Check if ANY of the original copied entities are in the current selection
        self.copied_entity_ids.iter().any(|id| current_selection.contains(id))
    }
    
    /// Get paste offset for current paste operation
    pub fn get_paste_offset(&self) -> Vec3 {
        // Paste stacks on top of the original (Y offset only)
        let y_offset = (self.paste_count as f32 + 1.0) * 2.0;
        Vec3::new(0.0, y_offset, 0.0)
    }

    /// Increment paste counter
    pub fn increment_paste_count(&mut self) {
        self.paste_count += 1;
    }
    
    pub fn set_last_paste_top(&mut self, top: f32) {
        self.last_paste_top = top;
    }
    
    pub fn get_last_paste_top(&self) -> f32 {
        self.last_paste_top
    }
    
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    
    pub fn clear(&mut self) {
        self.entities.clear();
        self.last_paste_top = f32::NEG_INFINITY;
        self.copy_center = Vec3::ZERO;
        self.paste_count = 0;
        self.copied_entity_ids.clear();
    }
    
    /// Get total entity count (including children)
    pub fn total_count(&self) -> usize {
        fn count_recursive(entities: &[ClipboardEntity]) -> usize {
            entities.iter().map(|e| 1 + count_recursive(&e.children)).sum()
        }
        count_recursive(&self.entities)
    }
}

// ============================================================================
// Clipboard Events
// ============================================================================

/// Event to trigger copy operation
#[derive(Event, Message, Debug, Clone)]
pub struct CopyEvent {
    /// Whether this is a cut operation
    pub is_cut: bool,
}

/// Event to trigger paste operation
#[derive(Event, Message, Debug, Clone)]
pub struct PasteEvent {
    /// Paste mode (normal or with new IDs)
    pub mode: PasteMode,
    /// Target position (None = use offset from copy center)
    pub target_position: Option<Vec3>,
}

/// Event to trigger duplicate operation
#[derive(Event, Message, Debug, Clone)]
pub struct DuplicateEvent;

/// Event fired when paste completes (for undo integration)
#[derive(Event, Message, Debug, Clone)]
pub struct PasteCompletedEvent {
    /// IDs of newly created entities
    pub created_entity_ids: Vec<String>,
}

// ============================================================================
// Clipboard Systems
// ============================================================================

/// System to handle copy/cut operations (simplified query)
pub fn handle_copy_event(
    mut events: MessageReader<CopyEvent>,
    mut clipboard: ResMut<EditorClipboard>,
    mut old_clipboard: ResMut<Clipboard>,
    selection: Option<Res<BevySelectionManager>>,
    query: Query<(Entity, &Instance, &Transform, Option<&BasePart>, Option<&eustress_common::classes::Part>, Option<&crate::space::instance_loader::InstanceFile>, Option<&crate::space::LoadedFromFile>)>,
    current_scene: Option<Res<CurrentScenePath>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    let Some(selection) = selection else { return };
    for event in events.read() {
        let selected_ids = selection.0.read().get_selected();

        if selected_ids.is_empty() {
            notifications.warning("Nothing selected to copy");
            continue;
        }

        // Cut is destructive — it deletes the source entities on paste.
        // Core services (Workspace / Lighting / SoulService / …) are
        // scaffolding for the entire Space; cutting one would orphan
        // every child. Refuse up front so the user gets a clear toast
        // instead of a broken state after paste. Plain Copy is harmless
        // and stays allowed.
        if event.is_cut {
            let mut protected_count = 0u32;
            for (entity, _inst, _t, _bp, _p, inst_file, loaded) in query.iter() {
                let entity_id = format!("{}v{}", entity.index(), entity.generation());
                if !selected_ids.contains(&entity_id) { continue; }
                let path = inst_file.map(|f| f.toml_path.clone())
                    .or_else(|| loaded.map(|l| l.path.clone()));
                if let Some(p) = path {
                    if crate::space::is_protected_service_path(&p) {
                        protected_count += 1;
                    }
                }
            }
            if protected_count > 0 {
                notifications.warning(format!(
                    "Cut refused: {} core service{} cannot be cut from the Explorer (Workspace, Lighting, SoulService, etc. are protected).",
                    protected_count, if protected_count == 1 { "" } else { "s" },
                ));
                continue;
            }
        }

        // Clear previous clipboard
        clipboard.clear();
        clipboard.is_cut = event.is_cut;
        clipboard.copied_at = Some(std::time::Instant::now());
        
        // Set source scene
        if let Some(ref scene_path) = current_scene {
            clipboard.source_scene = scene_path.0.clone();
        }
        
        // Calculate center of selection
        let mut center = Vec3::ZERO;
        let mut count = 0;
        
        // Collect selected entities
        let mut entity_data_list = Vec::new();
        let mut old_clipboard_entities = Vec::new();
        
        for (entity, instance, transform, basepart, part, instance_file, _loaded) in query.iter() {
            let entity_id = format!("{}v{}", entity.index(), entity.generation());
            
            if !selected_ids.contains(&entity_id) {
                continue;
            }
            
            center += transform.translation;
            count += 1;
            
            // Create ClipboardEntityData2 for serialization
            let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
            let mut properties = HashMap::new();
            
            // Add ALL BasePart properties so paste creates an exact clone
            if let Some(bp) = basepart {
                properties.insert("size".to_string(),
                    serde_json::json!([bp.size.x, bp.size.y, bp.size.z]));
                properties.insert("color".to_string(),
                    serde_json::json!([bp.color.to_srgba().red, bp.color.to_srgba().green,
                                       bp.color.to_srgba().blue, bp.color.to_srgba().alpha]));
                properties.insert("transparency".to_string(),
                    serde_json::json!(bp.transparency));
                properties.insert("reflectance".to_string(),
                    serde_json::json!(bp.reflectance));
                properties.insert("anchored".to_string(),
                    serde_json::json!(bp.anchored));
                properties.insert("can_collide".to_string(),
                    serde_json::json!(bp.can_collide));
                properties.insert("locked".to_string(),
                    serde_json::json!(bp.locked));
                properties.insert("material".to_string(),
                    serde_json::json!(format!("{:?}", bp.material)));
            }

            // Save Part shape (Ball, Cylinder, Wedge, etc.)
            if let Some(p) = part {
                let shape_str = match p.shape {
                    eustress_common::classes::PartType::Block => "Block",
                    eustress_common::classes::PartType::Ball => "Ball",
                    eustress_common::classes::PartType::Cylinder => "Cylinder",
                    eustress_common::classes::PartType::Wedge => "Wedge",
                    eustress_common::classes::PartType::CornerWedge => "CornerWedge",
                    eustress_common::classes::PartType::Cone => "Cone",
                };
                properties.insert("shape".to_string(), serde_json::json!(shape_str));
            }
            
            // `scale` field feeds the pasted entity's
            // `TransformData.scale` — which the instance loader
            // interprets as `BasePart.size` for primitive parts and
            // `Transform.scale` for custom-GLB meshes. For parts,
            // `Transform.scale` is kept at `[1, 1, 1]` (mesh is
            // regenerated at the authored size), so reading from
            // `transform.scale` here gave every duplicate + paste a
            // unit-size body — the exact bug user caught
            // 2026-04-23. Prefer `BasePart.size` when present so the
            // duplicate matches the source's real dimensions.
            let capture_scale = match basepart {
                Some(bp) => [bp.size.x, bp.size.y, bp.size.z],
                None     => [transform.scale.x, transform.scale.y, transform.scale.z],
            };
            // For non-visual service children (Sky, Atmosphere, Star, Moon,
            // etc.) read the raw TOML off disk so paste can reproduce it
            // exactly in any target Space's service folder.
            let (source_toml, service_folder) = if basepart.is_none() {
                if let Some(inst_file) = instance_file {
                    let toml_content = std::fs::read_to_string(&inst_file.toml_path).ok();
                    // Infer service folder from the TOML path (parent directory name)
                    let svc = inst_file.toml_path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    (toml_content, svc)
                } else {
                    (None, String::new())
                }
            } else {
                (None, String::new())
            };

            // Record the entity's on-disk folder when it's the
            // folder-form (`<dir>/_instance.toml`). Paste uses this to
            // recursively copy the whole subtree so children come
            // along — selecting a BillboardGui and Ctrl+V now also
            // brings its TextLabel descendants, matching the user's
            // mental model of "duplicate this thing and everything
            // inside it". Flat-file entities (`<Name>.toml` next to
            // siblings) don't have a private folder, so this stays
            // `None` and the property-based paste path runs.
            //
            // `to_string_lossy()` instead of direct `OsStr == &str`
            // comparison for cross-platform robustness — Windows
            // wide-string OsStr can confuse the `PartialEq<str>`
            // path in some toolchain combos.
            let source_folder_path = instance_file.and_then(|inst| {
                let toml = &inst.toml_path;
                let is_folder_form = toml
                    .file_name()
                    .map(|n| n.to_string_lossy() == "_instance.toml")
                    .unwrap_or(false);
                if !is_folder_form {
                    info!(
                        "📋 copy: '{}' is flat-file form ({}); children will not be cloned",
                        instance.name, toml.display(),
                    );
                    return None;
                }
                let parent = toml.parent().map(|p| p.to_string_lossy().to_string());
                if let Some(ref p) = parent {
                    info!("📋 copy: '{}' folder-form source captured: {}", instance.name, p);
                }
                parent
            });

            let entity_data = ClipboardEntityData2 {
                id: instance.id,
                // Capture the persistent identity (IDENTITY.md §11.2).
                // Legacy entities created before Wave 2.1 carry an empty
                // string here — `remap_uuids` handles that by minting a
                // fresh uuid via §3.4 on the COPY path.
                uuid: instance.uuid.clone(),
                name: instance.name.clone(),
                class: instance.class_name.as_str().to_string(),
                parent: None,
                parent_uuid: None,
                position: [transform.translation.x, transform.translation.y, transform.translation.z],
                rotation: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                scale: capture_scale,
                properties,
                parameters: None,
                source_toml,
                service_folder,
                source_folder_path,
            };
            
            entity_data_list.push(entity_data);
            clipboard.copied_entity_ids.push(entity_id.clone());
            
            // Also populate old clipboard for backward compatibility
            let clipboard_data = if basepart.is_some() {
                ClipboardEntityData::Part {
                    basepart: basepart.cloned().unwrap_or_default(),
                    part: Part::default(),
                }
            } else {
                ClipboardEntityData::Generic
            };
            
            let mut clip_entity = ClipboardEntity::new(
                instance.clone(),
                instance.name.clone(),
                *transform,
                clipboard_data,
            );
            clip_entity.original_entity = Some(entity);
            old_clipboard_entities.push(clip_entity);
        }
        
        if count > 0 {
            clipboard.copy_center = center / count as f32;
            clipboard.entities = entity_data_list;
            
            // Update old clipboard too
            old_clipboard.copy_with_ids(old_clipboard_entities, clipboard.copied_entity_ids.clone());
            old_clipboard.copy_center = clipboard.copy_center;
            
            let action = if event.is_cut { "Cut" } else { "Copied" };
            notifications.info(format!("{} {} object(s)", action, count));
        }
    }
}

/// System to handle paste operations
pub fn handle_paste_event(
    mut events: MessageReader<PasteEvent>,
    mut clipboard: ResMut<EditorClipboard>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    current_scene: Option<Res<CurrentScenePath>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    mut paste_completed: MessageWriter<PasteCompletedEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    selection_manager: Option<Res<BevySelectionManager>>,
    mut material_registry: Option<ResMut<crate::space::material_loader::MaterialRegistry>>,
    mut mesh_cache: Option<ResMut<crate::space::instance_loader::PrimitiveMeshCache>>,
    mut file_registry: Option<ResMut<crate::space::file_loader::SpaceFileRegistry>>,
    // Bundled into one tuple param to stay within Bevy's 16-system-param ceiling.
    cut_queries: (
        Query<&crate::space::instance_loader::InstanceFile>,
        Query<&crate::space::file_loader::LoadedFromFile>,
    ),
    mut paste_queue: ResMut<crate::space::file_loader::PasteSpawnQueue>,
) {
    let (instance_file_query, loaded_from_file_query) = (&cut_queries.0, &cut_queries.1);
    for event in events.read() {
        if clipboard.is_empty() {
            notifications.warning("Clipboard is empty");
            continue;
        }
        
        // Check for cross-scene paste
        let current_path = current_scene.as_ref().and_then(|s| s.0.as_ref());
        if clipboard.is_cross_scene(current_path) && event.mode == PasteMode::Normal {
            // Open cross-scene modal instead of pasting directly
            clipboard.cross_scene_modal.open = true;
            clipboard.cross_scene_modal.source_scene_name = clipboard.source_scene_name();
            clipboard.cross_scene_modal.choice = None;
            continue;
        }
        
        if event.mode == PasteMode::Cancelled {
            continue;
        }

        // Resolve target space identity FIRST — the §3.4 hash needs it
        // before we mint paste uuids. `space_root.0` is the canonical
        // path of the active Space (the destination), `current_scene`
        // is the active scene (typically nested under the Space). We
        // prefer the Space root because COPY/MOVE conflicts are
        // resolved at Space granularity (IDENTITY.md §8.3).
        let target_space_id = EditorClipboard::target_space_id_for(
            space_root.as_ref().map(|sr| &sr.0),
        );

        // Resolve workspace directory for the collision scan + TOML
        // writes below. Defined here (before the MOVE conflict check)
        // because both the §8.3 scan and the paste loop need it.
        let workspace_dir = space_root.as_ref()
            .map(|sr| sr.0.join("Workspace"))
            .unwrap_or_else(|| crate::space::default_space_root().join("Workspace"));

        // ── IDENTITY.md §8.3 — cross-space MOVE conflict check ───────
        // For a MOVE (is_cut=true), refuse the paste up-front when any
        // source uuid already exists in the target Space. The toast is
        // emitted once; the source is NOT deleted (the user resolves
        // manually by either deleting the destination's copy or
        // choosing COPY instead).
        //
        // Detection scans the target Workspace tree for any
        // `_instance.toml` whose `[metadata].uuid` matches a source
        // uuid. That's O(N) over destination entities, but cross-space
        // MOVE is rare (the common path is same-space ctrl-X /
        // ctrl-V which short-circuits at `is_cross_scene == false`),
        // so the filesystem scan is acceptable.
        if clipboard.is_cut && !clipboard.entities.is_empty() {
            let source_uuids: std::collections::HashSet<String> = clipboard
                .entities
                .iter()
                .filter(|e| !e.uuid.is_empty())
                .map(|e| e.uuid.clone())
                .collect();
            if !source_uuids.is_empty() {
                let collisions =
                    count_uuid_collisions_in_workspace(&workspace_dir, &source_uuids);
                if collisions > 0 {
                    notifications.warning(format!(
                        "Move refused: {} entity uuid(s) already exist in the target space \
                         (likely a previous Roblox import shared a referent). \
                         Choose Copy instead of Cut, or delete the target's existing copy first.",
                        collisions,
                    ));
                    // Critical: do NOT execute the move. Clear is_cut
                    // so the source files survive; the user has to
                    // re-issue Cut+Paste after resolving.
                    clipboard.is_cut = false;
                    continue;
                }
            }
        }

        // Remap UUIDs per IDENTITY.md §3.3 / §3.4. The helper honours
        // `is_cut` internally — MOVE preserves uuids verbatim, COPY
        // regenerates via blake3(source ‖ target_space_id ‖ counter).
        //
        // This runs UNCONDITIONALLY (not gated on `NewIds | DuplicateInPlace`
        // as the legacy `remap_ids` was) because the §3.4 contract for COPY
        // and the §3.3 contract for MOVE both depend on uuid logic running
        // on every paste — not just the "regenerate IDs to avoid conflicts"
        // branch. The PasteMode enum still controls *offset* behaviour
        // (Normal vs NewIds vs DuplicateInPlace), but uuid policy is
        // orthogonal: every paste either regenerates (COPY) or preserves
        // (MOVE).
        clipboard.remap_uuids(&target_space_id);

        // Calculate paste offset — DuplicateInPlace uses zero offset
        let offset = if event.mode == PasteMode::DuplicateInPlace {
            Vec3::ZERO
        } else {
            event.target_position
                .map(|pos| pos - clipboard.copy_center)
                .unwrap_or_else(|| clipboard.get_paste_offset())
        };
        
        let mut created_ids = Vec::new();

        // `workspace_dir` is defined above (before the §8.3 collision
        // scan) — reused here for the per-entity TOML writes.

        // Spawn entities from clipboard — write TOML files for Parts (same as Insert)
        for entity_data in &clipboard.entities {
            let spawned_id = spawn_pasted_entity(
                &mut commands,
                &asset_server,
                &mut materials,
                entity_data,
                offset,
                &workspace_dir,
                material_registry.as_deref_mut(),
                mesh_cache.as_deref_mut(),
                file_registry.as_deref_mut(),
                &mut paste_queue,
            );

            if let Some(id) = spawned_id {
                created_ids.push(id);
            }
        }
        
        clipboard.increment_paste_count();

        // Select the newly pasted entities so the user sees immediate feedback
        // and can move/delete/duplicate them without re-selecting. Works for
        // any pasted class (Part, MeshPart, Model, GUI element — the id
        // strings abstract over the underlying class).
        if !created_ids.is_empty() {
            if let Some(ref sel) = selection_manager {
                sel.0.write().set_selected(created_ids.clone());
            }
        }

        // Paste undo recording lives in the command system: each spawn_pasted_entity
        // call records a CreatePart in the History via spawn_events/command_history
        // flow. If the flow is not yet wired for paste specifically, Ctrl+Z will
        // fall back to deleting the most-recently-selected paste result — see
        // `PasteCompletedEvent` which updates the selection to the new entities.

        // Fire completion event
        paste_completed.write(PasteCompletedEvent {
            created_entity_ids: created_ids.clone(),
        });
        
        notifications.info(format!("Pasted {} object(s)", clipboard.entities.len()));
        
        // If this was a cut, trash the original files and despawn entities.
        // Without the file-trash step, the file watcher re-creates entities
        // from the still-present files on the next scan.
        if clipboard.is_cut {
            for entity_id in &clipboard.copied_entity_ids {
                if let Some(entity) = crate::entity_utils::id_string_to_entity(entity_id) {
                    if commands.get_entity(entity).is_ok() {
                        // Trash InstanceFile-backed entities (parts, folders)
                        if let Ok(inst_file) = instance_file_query.get(entity) {
                            let toml_path = inst_file.toml_path.clone();
                            let is_folder = toml_path.file_name()
                                .map(|n| n.to_string_lossy() == "_instance.toml")
                                .unwrap_or(false);
                            let source = if is_folder {
                                toml_path.parent().unwrap_or(toml_path.as_path()).to_path_buf()
                            } else {
                                toml_path.clone()
                            };
                            if source.exists() {
                                let trash_dir = source.parent()
                                    .unwrap_or(std::path::Path::new("."))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);
                                let stem = source.file_name().and_then(|n| n.to_str()).unwrap_or("entity");
                                let trash_path = {
                                    let base = trash_dir.join(stem);
                                    if base.exists() {
                                        let ts = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_millis()).unwrap_or(0);
                                        trash_dir.join(format!("{}-{:x}", stem, ts))
                                    } else { base }
                                };
                                if let Some(ref mut reg) = file_registry {
                                    reg.rename_in_progress.insert(toml_path.clone());
                                    reg.rename_in_progress.insert(source.clone());
                                }
                                let _ = std::fs::rename(&source, &trash_path);
                            }
                            if let Some(ref mut reg) = file_registry {
                                reg.unregister_file(&toml_path);
                            }
                        }
                        // Trash LoadedFromFile-backed entities (soul scripts, Rune/Luau)
                        else if let Ok(loaded) = loaded_from_file_query.get(entity) {
                            let source = loaded.path.clone();
                            if source.exists() {
                                let trash_dir = source.parent()
                                    .unwrap_or(std::path::Path::new("."))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);
                                let stem = source.file_name().and_then(|n| n.to_str()).unwrap_or("entity");
                                let trash_path = {
                                    let base = trash_dir.join(stem);
                                    if base.exists() {
                                        let ts = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_millis()).unwrap_or(0);
                                        trash_dir.join(format!("{}-{:x}", stem, ts))
                                    } else { base }
                                };
                                if let Some(ref mut reg) = file_registry {
                                    reg.rename_in_progress.insert(source.clone());
                                }
                                let _ = std::fs::rename(&source, &trash_path);
                            }
                            if let Some(ref mut reg) = file_registry {
                                reg.unregister_file(&source);
                            }
                        }
                        commands.entity(entity).despawn();
                    }
                }
            }
            // Clear selection (originals are gone)
            if let Some(ref sel) = selection_manager {
                sel.0.write().clear();
            }
            clipboard.clear();
        }
    }
}

/// System to handle duplicate operations (copy + paste in one step)
pub fn handle_duplicate_event(
    mut events: MessageReader<DuplicateEvent>,
    mut copy_events: MessageWriter<CopyEvent>,
    mut paste_events: MessageWriter<PasteEvent>,
    _clipboard: Res<EditorClipboard>,
) {
    for _event in events.read() {
        // Copy then paste in exact same position (duplicate = clone in place)
        copy_events.write(CopyEvent { is_cut: false });
        paste_events.write(PasteEvent {
            mode: PasteMode::DuplicateInPlace,
            target_position: None,
        });
    }
}

/// Spawn a pasted entity by writing a TOML file and using the standard instance loader.
/// This ensures pasted parts have full parity with inserted parts (InstanceFile, properties, etc.)
/// Recursively copy `src` directory to `dst`. Used by the folder-form
/// paste path so child instances (BillboardGui → TextLabel, Model →
/// Parts, …) ride along with the selected root.
///
/// **Order matters.** Files in this directory are copied BEFORE
/// descending into subdirectories. That way each level's `_instance.toml`
/// is created on disk before any descendant's `_instance.toml` — the
/// file_watcher then receives events in parent-first order and the
/// `ChildOf(parent)` lookup in `process_file_changes` finds the parent
/// entity already registered. Reversing the order (the naive "iterate
/// + recurse on dirs first" pattern) creates descendants first,
/// orphans them at workspace-root, and the user sees Label and TextLabel
/// as siblings of the duplicated Part instead of nested under it.
///
/// Skipped during the walk:
/// - `.eustress/` — engine-internal state (trash for undo, caches,
///   per-folder bookkeeping). Without this guard, copying a Part that
///   had ever been edited+undone would also clone every trashed
///   subentity from `<source>/.eustress/trash/<*>/_instance.toml`,
///   and the file_watcher would hot-load each of them as a fresh
///   workspace-root entity.
/// - Symlinks — same policy as `ui::file_event_handler::copy_dir_recursive`.
/// - Other hidden dirs (starting with `.`) — defensive: anything
///   namespaced with a leading dot is editor metadata, not scene data.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    // Buffer the entries so we can scan twice without re-reading the
    // directory (cheap — typical instance folder has ≤ 5 entries).
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(src)?
        .collect::<Result<Vec<_>, _>>()?;

    // Pass 1: copy regular files in this directory. The `_instance.toml`
    // at the current level lands on disk before any descendant's, so the
    // file_watcher's parent-lookup succeeds when descendant events fire.
    //
    // We copy through a `.tmp` + atomic rename so the destination only
    // becomes visible to readers as a complete file. External readers
    // (the `SpaceFileWatcher`'s reload pass, text editors, antivirus
    // scanners) could hit "os error 32 / file in use by another
    // process" against a half-written file mid-copy; the rename makes
    // every read see either the full content or nothing.
    for entry in &entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') { continue; }
        let ty = entry.file_type()?;
        if !ty.is_file() { continue; }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        let stem = name.to_string_lossy();
        let tmp_path = dst.join(format!(".{}.tmp", stem));
        std::fs::copy(&src_path, &tmp_path)?;
        // Retry rename a few times on transient Windows share-mode
        // conflicts. Three attempts is plenty now that the workspace
        // runs only one notify watcher (the engine's); the retries
        // are just a guard against external readers like antivirus
        // or text-editor reload.
        let mut last_err: Option<std::io::Error> = None;
        for _ in 0..3u32 {
            match std::fs::rename(&tmp_path, &dst_path) {
                Ok(()) => { last_err = None; break; }
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        if let Some(e) = last_err {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    // Pass 2: recurse into subdirectories. Each child's `_instance.toml`
    // is created after this level's, satisfying the parent-first invariant
    // at every depth.
    for entry in &entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') { continue; }
        let ty = entry.file_type()?;
        if !ty.is_dir() { continue; }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        copy_dir_recursive(&src_path, &dst_path)?;
    }
    // Symlinks fall through unread.
    Ok(())
}

/// Walk `workspace_dir` (recursively) and count how many
/// `_instance.toml` files claim a uuid present in `uuids`. Returns 0
/// when the directory doesn't exist or contains no matches. Skips
/// `.eustress` and other hidden directories.
///
/// Used by the IDENTITY.md §8.3 cross-space MOVE conflict check.
/// Optimistic — a malformed TOML is silently skipped (no false
/// positive). Caller treats a non-zero return as "refuse the move".
fn count_uuid_collisions_in_workspace(
    workspace_dir: &std::path::Path,
    uuids: &std::collections::HashSet<String>,
) -> u32 {
    if uuids.is_empty() || !workspace_dir.exists() {
        return 0;
    }
    let mut hits = 0u32;
    let mut stack: Vec<std::path::PathBuf> = vec![workspace_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let name_lossy = name.to_string_lossy();
            if name_lossy.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(ty) = entry.file_type() else { continue };
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() && name_lossy == "_instance.toml" {
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                let Ok(doc) = raw.parse::<toml::Value>() else { continue };
                let Some(uuid) = doc
                    .get("metadata")
                    .and_then(|m| m.get("uuid"))
                    .and_then(|v| v.as_str())
                else {
                    continue;
                };
                if uuids.contains(uuid) {
                    hits = hits.saturating_add(1);
                }
            }
        }
    }
    hits
}

/// Rewrite `[metadata].uuid` in a raw TOML body to `new_uuid`. Returns
/// `None` on parse failure (caller writes the source bytes verbatim).
/// Used by the service-child paste path so a cross-space COPY of a
/// non-visual service entity (Sky, Atmosphere, Star, …) lands with a
/// fresh uuid rather than colliding against the source.
fn rewrite_service_toml_uuid(raw: &str, new_uuid: &str) -> Option<String> {
    let mut doc: toml::Value = raw.parse().ok()?;
    let table = doc.as_table_mut()?;
    let meta = table
        .entry("metadata".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let meta_table = meta.as_table_mut()?;
    meta_table.insert(
        "uuid".to_string(),
        toml::Value::String(new_uuid.to_string()),
    );
    toml::to_string_pretty(&doc).ok()
}

/// Patch the root `_instance.toml` of a freshly-pasted folder so the
/// entity sits at `new_pos` in world space. Only the root needs
/// adjustment — descendants' transforms are relative to their parent's
/// local frame and survive the copy untouched. Returns `Ok(())` even
/// when the TOML lacks a `[transform]` table; failure modes are write
/// errors only.
///
/// Also stamps `metadata.uuid` to `target_uuid` when the caller supplied
/// one — that's the persistent-identity write-back per IDENTITY.md §3.3
/// (MOVE: preserve source uuid) and §3.4 (COPY: minted via
/// `mint_paste_uuid`). When `target_uuid` is empty the field is left
/// alone so legacy clipboards that didn't carry a uuid don't accidentally
/// erase the source folder's existing uuid (the duplicate would still
/// trip the §8.1 "uuid collision" rename on next load, but that's the
/// correct fallback for a pre-Wave-2.1 payload).
fn apply_offset_to_root_toml(
    toml_path: &std::path::Path,
    new_pos: Vec3,
    display_name: &str,
    target_uuid: &str,
) -> std::io::Result<()> {
    let raw = std::fs::read_to_string(toml_path)?;
    let mut doc: toml::Value = match raw.parse() {
        Ok(v) => v,
        Err(e) => {
            warn!("paste: failed to parse {} as TOML: {}", toml_path.display(), e);
            return Ok(());
        }
    };
    if let Some(table) = doc.as_table_mut() {
        let tform = table
            .entry("transform")
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(tform_table) = tform.as_table_mut() {
            tform_table.insert(
                "position".to_string(),
                toml::Value::Array(vec![
                    toml::Value::Float(new_pos.x as f64),
                    toml::Value::Float(new_pos.y as f64),
                    toml::Value::Float(new_pos.z as f64),
                ]),
            );
        }

        // Pin `[metadata].name` to the user-visible base so the Explorer
        // doesn't surface the disk-safe hex suffix
        // (`SimpleBlock-810f`). The folder name on disk has to be
        // unique vs. siblings — we resolve that with a hex tag — but
        // the entity's display name is read from `metadata.name` first
        // (instance_loader::spawn_instance), so writing the original
        // base here gives the duplicate the same label as the source
        // (`SimpleBlock`/`SimpleBlock`) while the on-disk folder stays
        // uniquely addressable.
        let meta = table
            .entry("metadata")
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(meta_table) = meta.as_table_mut() {
            meta_table.insert(
                "name".to_string(),
                toml::Value::String(display_name.to_string()),
            );
            // Stamp the persistent uuid per IDENTITY.md §3.3 / §3.4.
            // For COPY: a fresh §3.4 hash. For MOVE: the preserved
            // source uuid. Empty string means "legacy clipboard, no
            // uuid known" — leave the field alone so the file watcher
            // can mint one via §3.1 on first load (or §8.1 collision
            // rename if the source's old uuid is already present).
            if !target_uuid.is_empty() {
                meta_table.insert(
                    "uuid".to_string(),
                    toml::Value::String(target_uuid.to_string()),
                );
            }
        }
    }
    let serialised = toml::to_string_pretty(&doc)
        .unwrap_or_else(|_| raw.clone());
    // Atomic write + retry. The fresh duplicate folder is still being
    // touched by the engine's file watcher / antivirus / text editors;
    // a plain write here races with their reads and used to drop the
    // position/name patch (the duplicate would render at the source's
    // exact spot with no metadata override).
    crate::space::gui_loader::write_atomic(toml_path, serialised.as_bytes())?;
    Ok(())
}

fn spawn_pasted_entity(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    data: &ClipboardEntityData2,
    offset: Vec3,
    workspace_dir: &std::path::Path,
    material_registry: Option<&mut crate::space::material_loader::MaterialRegistry>,
    mesh_cache: Option<&mut crate::space::instance_loader::PrimitiveMeshCache>,
    file_registry: Option<&mut crate::space::file_loader::SpaceFileRegistry>,
    paste_queue: &mut crate::space::file_loader::PasteSpawnQueue,
) -> Option<String> {
    use crate::spawn::*;

    // ── Folder-form paste ────────────────────────────────────────────────
    // If the source recorded an on-disk folder, duplicate the whole
    // directory tree. This is the path that carries CHILDREN — a
    // BillboardGui's TextLabel, a Model's Parts, etc. The file watcher
    // will discover the new folder and spawn the entire subtree, so we
    // skip the property-based spawn below entirely. The selection
    // doesn't get the new entity ID immediately (file_watcher spawns
    // asynchronously); the post-paste `set_selected` call deals with
    // the empty case gracefully.
    if let Some(src_folder_str) = data.source_folder_path.as_deref() {
        let src_folder = std::path::PathBuf::from(src_folder_str);
        info!(
            "📋 paste: folder-form '{}' src='{}' exists={}",
            data.name, src_folder.display(), src_folder.exists(),
        );
        if src_folder.exists() {
            let new_pos = Vec3::new(data.position[0], data.position[1], data.position[2]) + offset;
            let _ = std::fs::create_dir_all(workspace_dir);
            let folder_name = crate::space::instance_loader::unique_entity_name(
                workspace_dir, &data.name,
            );
            let dst_folder = workspace_dir.join(&folder_name);
            if let Err(e) = copy_dir_recursive(&src_folder, &dst_folder) {
                warn!(
                    "📋 paste: failed to copy folder {} → {}: {}",
                    src_folder.display(), dst_folder.display(), e
                );
                return None;
            }
            // Patch root TOML so the duplicate appears at the paste
            // position instead of overlapping the source. `data.name`
            // is the original entity's display name — pinning
            // `metadata.name` to it stops the Explorer from showing
            // the disk-safe hex suffix (`SimpleBlock-810f`).
            //
            // `data.uuid` is the destination uuid set by the §3.3 /
            // §3.4 remap pass:
            // - COPY (`is_cut=false`): `remap_uuids` minted a fresh hash.
            // - MOVE (`is_cut=true`): preserved verbatim from the source.
            // - Pre-Wave-2.1 clipboards: empty string, in which case
            //   `apply_offset_to_root_toml` leaves the field alone and
            //   the file watcher's §3.1 path generates one on load.
            let dst_root_toml = dst_folder.join("_instance.toml");
            if let Err(e) = apply_offset_to_root_toml(
                &dst_root_toml,
                new_pos,
                &data.name,
                &data.uuid,
            ) {
                warn!(
                    "📋 paste: copied folder but failed to patch position in {}: {}",
                    dst_root_toml.display(), e
                );
            }
            // Spawn the pasted subtree DETERMINISTICALLY: queue the folder for
            // `drain_paste_spawn_queue`, which scans + spawns the whole tree
            // parent-first (children attached) by reusing the cold-load loader —
            // instead of relying on the file watcher, which dropped/orphaned
            // children (ordering/timing/low-FPS). Return `None` so we don't
            // pollute `created_ids` with a synthetic string (the spawn lands a
            // frame later); the notification reads `clipboard.entities.len()`.
            paste_queue.folders.push(dst_folder.clone());
            let _ = (commands, asset_server, materials, material_registry, mesh_cache, file_registry);
            info!("📋 paste: queued folder for deterministic spawn {} → {}", src_folder.display(), dst_folder.display());
            return None;
        } else {
            warn!(
                "📋 paste: source folder {} no longer exists; falling back to property paste",
                src_folder.display()
            );
        }
    } else {
        info!(
            "📋 paste: '{}' has no source_folder_path; property-based paste only (children won't be cloned)",
            data.name,
        );
    }

    let class_name = match ClassName::from_str(&data.class) {
        Ok(cn) => cn,
        Err(_) => {
            warn!("Unknown class name: {}", data.class);
            return None;
        }
    };

    let pos = Vec3::new(data.position[0], data.position[1], data.position[2]) + offset;
    let rot = Quat::from_euler(
        EulerRot::XYZ,
        data.rotation[0].to_radians(),
        data.rotation[1].to_radians(),
        data.rotation[2].to_radians(),
    );
    let scale = Vec3::new(data.scale[0], data.scale[1], data.scale[2]);

    let entity = match class_name {
        ClassName::Part => {
            // Determine mesh path from shape
            let shape = data.properties.get("shape")
                .and_then(|v| v.as_str())
                .unwrap_or("Block");
            let mesh_path = match shape {
                "Ball" => "assets/parts/ball.glb",
                "Cylinder" => "assets/parts/cylinder.glb",
                "Wedge" => "assets/parts/wedge.glb",
                "CornerWedge" => "assets/parts/corner_wedge.glb",
                "Cone" => "assets/parts/cone.glb",
                _ => "assets/parts/block.glb",
            };

            // Extract properties for TOML
            let color = data.properties.get("color")
                .and_then(|v| v.as_array())
                .map(|a| [
                    a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.639) as f32,
                    a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.635) as f32,
                    a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.647) as f32,
                    a.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                ])
                .unwrap_or([0.639, 0.635, 0.647, 1.0]);
            let transparency = data.properties.get("transparency")
                .and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let reflectance = data.properties.get("reflectance")
                .and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let anchored = data.properties.get("anchored")
                .and_then(|v| v.as_bool()).unwrap_or(true);
            let can_collide = data.properties.get("can_collide")
                .and_then(|v| v.as_bool()).unwrap_or(true);
            let locked = data.properties.get("locked")
                .and_then(|v| v.as_bool()).unwrap_or(false);
            let material_name = data.properties.get("material")
                .and_then(|v| v.as_str()).unwrap_or("Plastic").to_string();

            let now = chrono::Utc::now().to_rfc3339();

            // Build InstanceDefinition (same structure as toolbox insert)
            let instance_def = crate::space::instance_loader::InstanceDefinition {
                asset: Some(crate::space::instance_loader::AssetReference {
                    mesh: mesh_path.to_string(),
                    scene: "Scene0".to_string(),
                }),
                transform: crate::space::instance_loader::TransformData {
                    position: [pos.x, pos.y, pos.z],
                    rotation: [rot.x, rot.y, rot.z, rot.w],
                    scale: [scale.x, scale.y, scale.z],
                },
                properties: crate::space::instance_loader::InstanceProperties {
                    color,
                    transparency,
                    reflectance,
                    anchored,
                    can_collide,
                    cast_shadow: true,
                    material: material_name,
                    locked,
                    ..Default::default()
                },
                metadata: crate::space::instance_loader::InstanceMetadata {
                    class_name: "Part".to_string(),
                    archivable: true,
                    name: Some(data.name.clone()),
                    created: now.clone(),
                    last_modified: now,
                    // Stamp the persistent uuid per IDENTITY.md §3.3 / §3.4.
                    // For COPY: minted by `remap_uuids` via §3.4 hash.
                    // For MOVE: preserved from source.
                    // Pre-Wave-2.1 clipboards (empty uuid) fall through to
                    // None — the file watcher's §3.1 path will mint one
                    // on first load.
                    uuid: if data.uuid.is_empty() {
                        None
                    } else {
                        Some(data.uuid.clone())
                    },
                    ..Default::default()
                },
                material: None,
                thermodynamic: None,
                electrochemical: None,
                ui: None,
                attributes: None,
                tags: None,
                parameters: None,
                extra: std::collections::HashMap::new(),
            };

            // Generate unique folder name (folder-first architecture).
            // Uses the shared helper so flat-file variants (`Block.toml`,
            // `Block.glb.toml`) also count as taken — otherwise duplicating
            // a flat-file entity would silently shadow it with a folder of
            // the same name.
            let _ = std::fs::create_dir_all(workspace_dir);
            let folder_name = crate::space::instance_loader::unique_entity_name(workspace_dir, &data.name);
            let instance_dir = workspace_dir.join(&folder_name);
            let _ = std::fs::create_dir_all(&instance_dir);
            let toml_path = instance_dir.join("_instance.toml");

            // Write TOML file
            if let Err(e) = crate::space::instance_loader::write_instance_definition(&toml_path, &instance_def) {
                warn!("Failed to write pasted entity TOML: {}", e);
                return None;
            }

            // Spawn via standard instance loader (same path as file_loader)
            let mut default_mat_reg = crate::space::material_loader::MaterialRegistry::default();
            let mat_reg = material_registry.unwrap_or(&mut default_mat_reg);
            let mut default_mesh_cache = crate::space::instance_loader::PrimitiveMeshCache::default();
            let mesh_c = mesh_cache.unwrap_or(&mut default_mesh_cache);

            let entity = crate::space::instance_loader::spawn_instance(
                commands,
                asset_server,
                materials,
                mat_reg,
                mesh_c,
                toml_path.clone(),
                instance_def,
            );

            // Register in file registry (folder path, not TOML path)
            if let Some(registry) = file_registry {
                registry.register(
                    toml_path.clone(),
                    entity,
                    crate::space::FileMetadata {
                        path: instance_dir,
                        file_type: crate::space::FileType::Directory,
                        service: "Workspace".to_string(),
                        name: data.name.clone(),
                        size: 0,
                        modified: std::time::SystemTime::now(),
                        children: Vec::new(),
                    },
                );
            }

            Some(entity)
        }
        ClassName::Model => {
            let instance = Instance { name: data.name.clone(), class_name, archivable: true, id: data.id, ..Default::default() };
            Some(spawn_model(commands, instance, Model::default()))
        }
        ClassName::Folder => {
            let instance = Instance { name: data.name.clone(), class_name, archivable: true, id: data.id, ..Default::default() };
            Some(spawn_folder(commands, instance))
        }
        ClassName::PointLight => {
            let instance = Instance { name: data.name.clone(), class_name, archivable: true, id: data.id, ..Default::default() };
            let transform = Transform { translation: pos, rotation: rot, scale };
            Some(spawn_point_light(commands, instance, EustressPointLight::default(), transform))
        }
        ClassName::SpotLight => {
            let instance = Instance { name: data.name.clone(), class_name, archivable: true, id: data.id, ..Default::default() };
            let transform = Transform { translation: pos, rotation: rot, scale };
            Some(spawn_spot_light(commands, instance, EustressSpotLight::default(), transform))
        }
        _ if !data.service_folder.is_empty() && data.source_toml.is_some() => {
            // Generic service child (Sky, Atmosphere, Star/Sun, Moon, or any
            // future Lighting/ or other service child). Write the raw TOML
            // into the target Space's service folder — the file watcher picks
            // it up and the hydration system attaches the right ECS components.
            //
            // IDENTITY.md §3.3 / §3.4: stamp the destination uuid into the
            // raw TOML's `[metadata]` table BEFORE writing. The COPY path
            // hashes a fresh uuid (so the source row + destination row
            // don't collide on next load); the MOVE path preserves the
            // source uuid (which equals data.uuid after remap_uuids' MOVE
            // branch). When the clipboard predates Wave 2.1 (data.uuid
            // empty) we leave the TOML alone and let the §3.1 path fire.
            let raw = data.source_toml.as_deref().unwrap_or("");
            let toml_content: String = if data.uuid.is_empty() {
                raw.to_string()
            } else {
                rewrite_service_toml_uuid(raw, &data.uuid).unwrap_or_else(|| {
                    warn!(
                        "📋 paste: failed to rewrite uuid in service TOML; \
                         writing source bytes verbatim (may collide on next load)",
                    );
                    raw.to_string()
                })
            };
            let service_dir = workspace_dir
                .parent()  // Space root (workspace_dir is SpaceRoot/Workspace)
                .unwrap_or(workspace_dir)
                .join(&data.service_folder);
            let _ = std::fs::create_dir_all(&service_dir);
            // Use <Name>.instance.toml — matches what space_ops writes.
            let file_name = format!("{}.instance.toml", data.name);
            let target_path = service_dir.join(&file_name);
            if let Err(e) = std::fs::write(&target_path, &toml_content) {
                warn!("Failed to write pasted service child TOML {:?}: {}", target_path, e);
                return None;
            }
            // Register in file registry so Explorer shows it immediately.
            if let Some(registry) = file_registry {
                let dummy_entity = commands.spawn(Name::new(data.name.clone())).id();
                registry.register(
                    target_path.clone(),
                    dummy_entity,
                    crate::space::FileMetadata {
                        path: target_path,
                        file_type: crate::space::FileType::Toml,
                        service: data.service_folder.clone(),
                        name: data.name.clone(),
                        size: toml_content.len() as u64,
                        modified: std::time::SystemTime::now(),
                        children: Vec::new(),
                    },
                );
                Some(dummy_entity)
            } else {
                // Spawn a placeholder — the file watcher will reload it properly.
                Some(commands.spawn(Name::new(data.name.clone())).id())
            }
        }
        _ => {
            warn!("Paste not fully implemented for {:?}", class_name);
            None
        }
    };
    
    entity.map(|e| format!("{}v{}", e.index(), e.generation()))
}

/// System to render cross-scene paste modal
/// Note: Modal UI is now handled by Slint
pub fn render_cross_scene_modal(
    mut clipboard: ResMut<EditorClipboard>,
    mut paste_events: MessageWriter<PasteEvent>,
) {
    // Cross-scene paste modal is now handled by Slint UI
    // For now, auto-paste with new IDs when cross-scene is detected
    if clipboard.cross_scene_modal.open {
        clipboard.cross_scene_modal.open = false;
        paste_events.write(PasteEvent {
            mode: PasteMode::NewIds,
            target_position: None,
        });
    }
}

/// System that consumes `pending_paste` flag from StudioState and fires a PasteEvent
/// with the mouse cursor's world-space position (raycast against surfaces or ground plane).
/// This bridges the keybinding (Ctrl+V) path to the actual paste logic.
pub fn consume_pending_paste(
    mut studio_state: ResMut<crate::ui::StudioState>,
    mut paste_events: MessageWriter<PasteEvent>,
) {
    if !studio_state.pending_paste {
        return;
    }
    studio_state.pending_paste = false;

    // Ctrl+V paste: place above original using clipboard offset (no raycast).
    // get_paste_offset() returns Y offset based on entity height.

    paste_events.write(PasteEvent {
        mode: PasteMode::Normal,
        target_position: None,
    });
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for clipboard system
pub struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(Clipboard::default())
            .insert_resource(EditorClipboard::default())
            .add_message::<CopyEvent>()
            .add_message::<PasteEvent>()
            .add_message::<DuplicateEvent>()
            .add_message::<PasteCompletedEvent>()
            .add_systems(Update, (
                handle_duplicate_event,
                handle_copy_event.after(handle_duplicate_event),
                consume_pending_paste.after(handle_copy_event),
                handle_paste_event.after(consume_pending_paste),
                render_cross_scene_modal,
            ));
    }
}

// ============================================================================
// Tests — IDENTITY.md §3.3 / §3.4 / §11.2
// ============================================================================
//
// Wave 4 task wave4_B contract: prove that the clipboard's identity surface
// honours the four-surface contract for cross-space COPY (regenerate uuid
// via §3.4 hash) and MOVE (preserve source uuid per §3.3). Tests run the
// helpers directly — they do not boot the Bevy plugin (the goal is to lock
// the math, not exercise the system schedule).

#[cfg(test)]
mod uuid_tests {
    use super::*;
    use eustress_common::instance_create::is_valid_uuid;

    /// Build a minimal clipboard entry for tests. Properties are empty;
    /// only `id` + `uuid` are meaningful for the §3.3 / §3.4 surface.
    fn entry(id: u32, uuid: &str) -> ClipboardEntityData2 {
        ClipboardEntityData2 {
            id,
            uuid: uuid.to_string(),
            name: format!("Entity{id}"),
            class: "Part".to_string(),
            parent: None,
            parent_uuid: None,
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            properties: HashMap::new(),
            parameters: None,
            source_toml: None,
            service_folder: String::new(),
            source_folder_path: None,
        }
    }

    fn target_space_a() -> Vec<u8> {
        b"Spaces/SpaceA".to_vec()
    }

    fn target_space_b() -> Vec<u8> {
        b"Spaces/SpaceB".to_vec()
    }

    /// §3.4 — single COPY: source uuid is replaced by a fresh, valid
    /// 32-hex uuid that differs from the source.
    #[test]
    fn copy_mints_fresh_uuid_per_3_4() {
        let source_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let mut cb = EditorClipboard::default();
        cb.is_cut = false;
        cb.entities.push(entry(1, source_uuid));

        cb.remap_uuids(&target_space_a());

        let result = &cb.entities[0];
        assert_eq!(result.id, 1, "Bevy live handle is preserved");
        assert_ne!(
            result.uuid, source_uuid,
            "COPY MUST mint a fresh uuid (§3.4)"
        );
        assert!(
            is_valid_uuid(&result.uuid),
            "minted uuid must satisfy IDENTITY.md §7.3 format: {:?}",
            result.uuid
        );
        // The mapping records the source → target translation.
        assert_eq!(
            cb.uuid_mapping.get(source_uuid).map(String::as_str),
            Some(result.uuid.as_str()),
            "uuid_mapping carries the rename for parent-link fix-up",
        );
    }

    /// §3.3 — MOVE preserves the source uuid verbatim. This is the
    /// cross-space identity transport surface — the destination Fjall
    /// row uses the same uuid so audit-log refs, network refs, and
    /// script lookups all stay correct.
    #[test]
    fn move_preserves_source_uuid_per_3_3() {
        let source_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let mut cb = EditorClipboard::default();
        cb.is_cut = true; // MOVE mode
        cb.entities.push(entry(1, source_uuid));

        cb.remap_uuids(&target_space_a());

        let result = &cb.entities[0];
        assert_eq!(
            result.uuid, source_uuid,
            "MOVE MUST preserve source uuid verbatim (§3.3)"
        );
        // The mapping is identity so any parent_uuid rewrites are no-ops.
        assert_eq!(
            cb.uuid_mapping.get(source_uuid).map(String::as_str),
            Some(source_uuid),
        );
    }

    /// §3.4 counter contract — ten Ctrl+V presses after one Ctrl+C
    /// produce ten distinct uuids. Implemented by bumping `paste_count`
    /// per paste; `mint_paste_uuid` folds it into the seed.
    #[test]
    fn three_copies_produce_three_distinct_uuids_via_counter() {
        let source_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let space_id = target_space_a();
        let mut minted: Vec<String> = Vec::new();

        for paste_index in 0..3u32 {
            let mut cb = EditorClipboard::default();
            cb.is_cut = false;
            cb.paste_count = paste_index; // before bumping, like the live flow
            cb.entities.push(entry(1, source_uuid));
            cb.remap_uuids(&space_id);
            minted.push(cb.entities[0].uuid.clone());
        }

        assert_eq!(minted.len(), 3);
        assert_ne!(minted[0], minted[1], "paste 1 != paste 2");
        assert_ne!(minted[1], minted[2], "paste 2 != paste 3");
        assert_ne!(minted[0], minted[2], "paste 1 != paste 3");
        for u in &minted {
            assert!(
                is_valid_uuid(u),
                "every minted uuid must be valid 32-hex: {u:?}"
            );
            assert_ne!(u, source_uuid, "all distinct from source");
        }
    }

    /// `mint_paste_uuid` is deterministic across processes — same inputs
    /// always produce the same uuid. This is the property that lets a
    /// parallel CI checkout match a developer's local result.
    #[test]
    fn mint_paste_uuid_is_deterministic() {
        let a = EditorClipboard::mint_paste_uuid("source", b"target", 1);
        let b = EditorClipboard::mint_paste_uuid("source", b"target", 1);
        assert_eq!(a, b);
        let c = EditorClipboard::mint_paste_uuid("source", b"target", 2);
        assert_ne!(a, c, "different counter → different uuid");
        let d = EditorClipboard::mint_paste_uuid("source", b"OTHER", 1);
        assert_ne!(a, d, "different target_space_id → different uuid");
        let e = EditorClipboard::mint_paste_uuid("OTHER", b"target", 1);
        assert_ne!(a, e, "different source uuid → different uuid");
    }

    /// `mint_paste_uuid` always returns a IDENTITY.md §7.3-compliant
    /// uuid: 32 chars, lowercase hex only.
    #[test]
    fn mint_paste_uuid_output_is_valid_format() {
        let u = EditorClipboard::mint_paste_uuid(
            "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7",
            b"Spaces/SpaceA",
            42,
        );
        assert!(
            is_valid_uuid(&u),
            "minted uuid must be 32-lowercase-hex per §7.3: {u:?}"
        );
    }

    /// COPY into a different target Space produces a different uuid even
    /// for the same source — §3.4 includes `target_space_id` in the
    /// seed specifically so paste-into-B vs paste-into-C never collide.
    #[test]
    fn copy_into_two_target_spaces_gives_distinct_uuids() {
        let source_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";

        let mut into_a = EditorClipboard::default();
        into_a.is_cut = false;
        into_a.paste_count = 0;
        into_a.entities.push(entry(1, source_uuid));
        into_a.remap_uuids(&target_space_a());

        let mut into_b = EditorClipboard::default();
        into_b.is_cut = false;
        into_b.paste_count = 0;
        into_b.entities.push(entry(1, source_uuid));
        into_b.remap_uuids(&target_space_b());

        assert_ne!(
            into_a.entities[0].uuid, into_b.entities[0].uuid,
            "paste from A into B and from A into C must produce different uuids \
             (§3.4 — target_space_id in the seed)"
        );
    }

    /// `clear()` resets `paste_count` so a fresh Ctrl+C starts the
    /// counter at 0 — the §3.4 contract: "counter resets on the next
    /// ctrl-C".
    #[test]
    fn clear_resets_copy_counter() {
        let mut cb = EditorClipboard::default();
        cb.paste_count = 9;
        cb.entities.push(entry(1, "abc"));
        cb.uuid_mapping
            .insert("a".to_string(), "b".to_string());
        cb.is_cut = true;
        cb.clear();
        assert_eq!(cb.paste_count, 0, "counter must reset on clear()");
        assert!(cb.entities.is_empty());
        assert!(cb.uuid_mapping.is_empty());
        assert!(!cb.is_cut);
    }

    /// A multi-entity COPY batch produces distinct uuids for each entity
    /// even when they share a source uuid — the `batch_idx` term in
    /// `remap_uuids` makes within-batch collisions impossible.
    #[test]
    fn copy_batch_with_same_source_uuid_produces_distinct_uuids() {
        let source_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let mut cb = EditorClipboard::default();
        cb.is_cut = false;
        cb.entities.push(entry(1, source_uuid));
        cb.entities.push(entry(2, source_uuid));
        cb.entities.push(entry(3, source_uuid));
        cb.remap_uuids(&target_space_a());

        let uuids: Vec<String> = cb.entities.iter().map(|e| e.uuid.clone()).collect();
        assert_ne!(uuids[0], uuids[1]);
        assert_ne!(uuids[1], uuids[2]);
        assert_ne!(uuids[0], uuids[2]);
        for u in &uuids {
            assert!(is_valid_uuid(u));
        }
    }

    /// Legacy clipboards (pre-Wave-2.1) carry an empty `uuid` string.
    /// COPY still produces a valid fresh uuid in the destination — the
    /// destination TOML lands with a 32-hex uuid as if it were a fresh
    /// create.
    #[test]
    fn copy_legacy_empty_uuid_still_mints_valid_destination_uuid() {
        let mut cb = EditorClipboard::default();
        cb.is_cut = false;
        cb.entities.push(entry(1, "")); // legacy: no uuid
        cb.remap_uuids(&target_space_a());
        assert!(
            is_valid_uuid(&cb.entities[0].uuid),
            "legacy empty source must mint a valid destination uuid: {:?}",
            cb.entities[0].uuid
        );
    }

    /// `parent_uuid` is rewritten through the uuid_mapping so a parent
    /// in the same paste batch keeps the correct linkage.
    #[test]
    fn copy_rewrites_parent_uuid_within_batch() {
        let parent_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7";
        let child_source = "00112233445566778899aabbccddeeff";
        let mut cb = EditorClipboard::default();
        cb.is_cut = false;
        // Parent
        cb.entities.push(entry(1, parent_uuid));
        // Child — references parent's uuid
        let mut child = entry(2, child_source);
        child.parent_uuid = Some(parent_uuid.to_string());
        cb.entities.push(child);

        cb.remap_uuids(&target_space_a());

        // The child's parent_uuid must now point at the parent's MINTED
        // uuid, not the source — otherwise the destination hierarchy
        // would orphan the child.
        let new_parent_uuid = cb.entities[0].uuid.clone();
        assert_eq!(
            cb.entities[1].parent_uuid,
            Some(new_parent_uuid),
            "child must reference parent's minted uuid post-remap",
        );
    }

    /// `target_space_id_for` returns the path bytes when present and
    /// empty when absent — exercised by tests that need to inspect the
    /// helper without booting Bevy.
    #[test]
    fn target_space_id_for_handles_some_and_none() {
        use std::path::PathBuf;
        let p = PathBuf::from("E:\\foo\\bar");
        let some = EditorClipboard::target_space_id_for(Some(&p));
        assert!(!some.is_empty());
        let none = EditorClipboard::target_space_id_for(None);
        assert!(none.is_empty());
    }
}
