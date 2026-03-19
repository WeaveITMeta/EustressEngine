//! # storage
//!
//! Physical storage architecture for workshop tools.
//! Each storage unit (box, bin, drawer, shelf, cabinet, zone) is a folder on disk.
//! The folder contains an `instance.toml` that defines what type of storage it is,
//! its physical location, its GPS chip binding, and its capacity.
//!
//! When a tool's GPS chip reports it has moved to a new container, the engine
//! moves the `.tool.toml` file into the corresponding folder — the file system
//! mirrors physical reality in real time, fully autonomously.
//!
//! ## Table of Contents
//!
//! | Section             | Purpose                                                        |
//! |---------------------|----------------------------------------------------------------|
//! | `StorageKind`       | What type of container this is (box, bin, shelf, ActiveUse, etc.)|
//! | `StorageIotConfig`  | GPS chip binding for the container itself                      |
//! | `StorageUnit`       | Full container definition — serialises to `instance.toml`      |
//! | `StoragePath`       | Hierarchical path: Zone > Cabinet > Shelf > Bin                |
//! | `ContainerIndex`    | In-memory index of all containers rebuilt from folder scan     |
//! | `StorageManager`    | Filesystem manager — creates folders, moves tool TOMLs         |
//! | `ToolMovementEvent` | Records that a tool has been moved between containers          |
//!
//! ## File Layout
//!
//! ```
//! my-workshop/
//! ├── .workshop/
//! │   └── workshop.toml
//! └── tools/
//!     ├── instance.toml                          ← Root storage zone ("Workshop Floor")
//!     ├── milwaukee-m18-drill.tool.toml          ← Tool loose on the workshop floor
//!     ├── bench-3/
//!     │   ├── instance.toml                      ← StorageKind::Bench
//!     │   ├── torque-wrench-3_8.tool.toml        ← Tool sitting on Bench 3
//!     │   ├── left-drawer/
//!     │   │   ├── instance.toml                  ← StorageKind::Drawer
//!     │   │   └── 5mm-drill-bit.tool.toml
//!     │   └── right-shelf/
//!     │       ├── instance.toml                  ← StorageKind::Shelf
//!     │       └── caliper-150mm.tool.toml
//!     ├── cnc-bay/
//!     │   ├── instance.toml                      ← StorageKind::Zone
//!     │   └── shopbot-cnc-router.tool.toml
//!     └── tool-cabinet-a/
//!         ├── instance.toml                      ← StorageKind::Cabinet
//!         ├── top-drawer/
//!         │   ├── instance.toml                  ← StorageKind::Drawer
//!         │   └── combination-wrench-set.tool.toml
//!         └── bin-1/
//!             ├── instance.toml                  ← StorageKind::Bin
//!             └── hex-bolt-m6.tool.toml
//! ```
//!
//! ## GPS-Driven Autonomous Movement
//!
//! When the `LiveStatusStore` receives a telemetry update showing a tool's
//! `space_position` has moved into a different container's bounding volume,
//! `StorageManager::relocate_tool()` is called automatically:
//!
//! 1. Determine which `StorageUnit` the new GPS position falls within
//! 2. If different from the current container, move the `.tool.toml` file
//!    into the new container's folder via `std::fs::rename`
//! 3. Update the `ContainerIndex` in memory
//! 4. Emit a `ToolMovementEvent` for the UI and audit log
//! 5. If no container matches, the tool file moves to the root `tools/` folder

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 1. StorageKind — what type of container
// ============================================================================

/// The physical type of a storage unit.
/// Determines icon in the Explorer tree, capacity defaults, and 3D mesh spawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    /// A broad physical zone or area (e.g. "CNC Bay", "Welding Zone", "Storage Room B")
    Zone,
    /// A workbench or work table surface
    Bench,
    /// A freestanding tool chest or rolling cabinet with multiple drawers
    Cabinet,
    /// A single drawer within a cabinet or bench
    Drawer,
    /// A fixed or adjustable shelf on a wall unit or cabinet
    Shelf,
    /// An open bin or tote (often labelled by fastener type or material)
    Bin,
    /// A portable tool box (latching lid, typically carried to job site)
    ToolBox,
    /// A wall-mounted pegboard panel
    Pegboard,
    /// A rack (pipe rack, lumber rack, bar stock rack)
    Rack,
    /// A locker or secure enclosure
    Locker,
    /// A floor space or mat (for large machines that sit on the floor)
    FloorSpace,
    /// A portable rolling cart
    Cart,
    /// **Active Use** — the virtual container for tools currently being wielded.
    /// There is exactly one `active-use/` folder per workshop (at the tools root).
    /// When a kinetic GPS chip fires because the tool is picked up and moving,
    /// the tool's `.tool.toml` is moved here. When the chip fires again and the
    /// tool lands in a known container, it moves back. Tools in this folder are
    /// shown as "In Use" across the entire system and are NOT assignable to new steps.
    ActiveUse,
    /// Custom / user-defined storage type
    Custom(String),
}

impl StorageKind {
    /// Returns a human-readable label for the Explorer panel and Properties Panel
    pub fn display_label(&self) -> String {
        match self {
            StorageKind::Zone => "Zone".into(),
            StorageKind::Bench => "Bench".into(),
            StorageKind::Cabinet => "Cabinet".into(),
            StorageKind::Drawer => "Drawer".into(),
            StorageKind::Shelf => "Shelf".into(),
            StorageKind::Bin => "Bin".into(),
            StorageKind::ToolBox => "Tool Box".into(),
            StorageKind::Pegboard => "Pegboard".into(),
            StorageKind::Rack => "Rack".into(),
            StorageKind::Locker => "Locker".into(),
            StorageKind::FloorSpace => "Floor Space".into(),
            StorageKind::Cart => "Cart".into(),
            StorageKind::ActiveUse => "Active Use".into(),
            StorageKind::Custom(name) => name.clone(),
        }
    }

    /// Returns the icon name used in the Explorer tree (maps to assets/icons/workshop/)
    pub fn icon_name(&self) -> &str {
        match self {
            StorageKind::Zone => "zone",
            StorageKind::Bench => "bench",
            StorageKind::Cabinet => "cabinet",
            StorageKind::Drawer => "drawer",
            StorageKind::Shelf => "shelf",
            StorageKind::Bin => "bin",
            StorageKind::ToolBox => "toolbox",
            StorageKind::Pegboard => "pegboard",
            StorageKind::Rack => "rack",
            StorageKind::Locker => "locker",
            StorageKind::FloorSpace => "floor",
            StorageKind::Cart => "cart",
            StorageKind::ActiveUse => "active-use",
            StorageKind::Custom(_) => "storage",
        }
    }

    /// Returns true if this container kind means a tool is actively being used
    pub fn is_active_use(&self) -> bool {
        matches!(self, StorageKind::ActiveUse)
    }
}

// ============================================================================
// 2. StorageIotConfig — GPS chip binding for the container itself
// ============================================================================

/// IoT chip configuration for a storage unit.
/// Some containers (e.g. portable carts, tool boxes) have their own GPS chip
/// so their location can be tracked independently from the tools inside them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageIotConfig {
    /// Hardware chip identifier for this container's GPS chip
    pub chip_id: String,
    /// MQTT topic for container telemetry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mqtt_topic: Option<String>,
    /// Whether this container has GPS (portable containers do; fixed shelves usually don't)
    #[serde(default)]
    pub has_gps: bool,
}

// ============================================================================
// 3. BoundingVolume — defines where in the 3D Space this container exists
// ============================================================================

/// A 3D axis-aligned bounding volume in the digital twin Space.
/// Used to determine which container a GPS-tracked tool is currently inside.
/// Coordinates are in metres, relative to the workshop's Space origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingVolume {
    /// Minimum corner [x, y, z] in metres
    pub min: [f32; 3],
    /// Maximum corner [x, y, z] in metres
    pub max: [f32; 3],
}

impl BoundingVolume {
    /// Returns true if the given point falls within this bounding volume
    pub fn contains(&self, point: [f32; 3]) -> bool {
        point[0] >= self.min[0]
            && point[0] <= self.max[0]
            && point[1] >= self.min[1]
            && point[1] <= self.max[1]
            && point[2] >= self.min[2]
            && point[2] <= self.max[2]
    }

    /// Returns the centre point of this bounding volume
    pub fn centre(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }
}

// ============================================================================
// 4. StorageUnit — the instance.toml root struct
// ============================================================================

/// A physical storage unit in the workshop.
/// Serialises to/from an `instance.toml` file inside the container's folder.
/// The folder name IS the container's identifier — human-readable, git-diffable.
///
/// Container hierarchy is expressed purely through folder nesting:
/// `tools/tool-cabinet-a/top-drawer/` = Zone > Cabinet > Drawer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageUnit {
    /// Stable unique identifier — never changes after creation
    pub id: Uuid,
    /// Human-readable display name (e.g. "Bench 3", "Top Drawer", "Bin A")
    pub name: String,
    /// What kind of storage this is
    pub kind: StorageKind,
    /// Human-readable description of what this container holds
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Maximum number of tools this container should hold (soft limit, advisory only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u32>,
    /// 3D bounding volume in the digital twin Space (used for GPS containment detection)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<BoundingVolume>,
    /// IoT chip binding (present only for mobile containers with GPS chips)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iot: Option<StorageIotConfig>,
    /// 3D mesh path for the digital twin entity spawn (relative to workspace)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_path: Option<String>,
    /// Colour label for visual grouping in the Explorer tree (hex string e.g. "#0078d4")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Tags for filtering in the Explorer panel
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Whether this container is currently locked / inaccessible
    #[serde(default)]
    pub locked: bool,
    /// ISO 8601 timestamp when this container was registered
    pub created_at: DateTime<Utc>,
}

impl StorageUnit {
    /// Create a new storage unit with required fields
    pub fn new(name: impl Into<String>, kind: StorageKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            description: None,
            capacity: None,
            bounds: None,
            iot: None,
            mesh_path: None,
            color: None,
            tags: Vec::new(),
            locked: false,
            created_at: Utc::now(),
        }
    }

    /// Returns true if the given 3D point is inside this container's bounding volume.
    /// Used for GPS-driven autonomous tool placement.
    pub fn contains_point(&self, point: [f32; 3]) -> bool {
        self.bounds
            .as_ref()
            .map(|b| b.contains(point))
            .unwrap_or(false)
    }
}

// ============================================================================
// 5. StoragePath — hierarchical path representation
// ============================================================================

/// The hierarchical path to a container, expressed as a Vec of folder names.
/// E.g. `["tools", "tool-cabinet-a", "top-drawer"]`
/// This maps 1:1 to the filesystem path under the workspace root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoragePath(pub Vec<String>);

impl StoragePath {
    /// Build a StoragePath from a filesystem path relative to the tools root
    pub fn from_path(root: &Path, path: &Path) -> Option<Self> {
        path.strip_prefix(root).ok().map(|rel| {
            StoragePath(
                rel.components()
                    .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
                    .collect(),
            )
        })
    }

    /// Convert to a display string like "Bench 3 / Right Shelf"
    pub fn display(&self) -> String {
        self.0.join(" / ")
    }

    /// Returns the depth of nesting (0 = root zone, 1 = top-level container, etc.)
    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

// ============================================================================
// 6. ToolMovementRecord — audit log entry
// ============================================================================

/// Records that a tool has been moved between containers.
/// Written to an append-only `movement_log.toml` in the tools root.
/// Provides full audit trail of where every tool has been.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMovementRecord {
    /// The tool that moved
    pub tool_id: Uuid,
    /// Human-readable tool name
    pub tool_name: String,
    /// Container ID the tool moved FROM (None = workshop floor / root)
    pub from_container_id: Option<Uuid>,
    /// Container display path the tool moved FROM
    pub from_path: Option<String>,
    /// Container ID the tool moved TO (None = workshop floor / root)
    pub to_container_id: Option<Uuid>,
    /// Container display path the tool moved TO
    pub to_path: Option<String>,
    /// How the movement was detected
    pub trigger: MovementTrigger,
    /// GPS coordinates at the time of movement
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps_position: Option<[f32; 3]>,
    /// UTC timestamp of the movement
    pub moved_at: DateTime<Utc>,
}

/// What triggered a tool movement record
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementTrigger {
    /// GPS telemetry detected the tool entered a new container's bounding volume
    GpsAutomatic,
    /// User manually moved the tool in the Explorer panel (drag-and-drop)
    ManualExplorer,
    /// User moved the tool via the Properties Panel
    ManualProperties,
    /// Build guide step checkout assigned the tool to a step
    BuildGuideCheckout,
    /// Build guide step return moved the tool back to its home container
    BuildGuideReturn,
}

// ============================================================================
// 7. ContainerIndex — in-memory index of all storage units
// ============================================================================

/// In-memory index of all `StorageUnit` instances, built by scanning the folder tree
/// for `instance.toml` files. Used for GPS containment queries and Explorer tree rendering.
#[derive(Debug, Default)]
pub struct ContainerIndex {
    /// Primary map: container UUID → StorageUnit
    by_id: HashMap<Uuid, StorageUnit>,
    /// Filesystem path → container UUID
    by_path: HashMap<PathBuf, Uuid>,
    /// Container UUID → filesystem folder path
    paths: HashMap<Uuid, PathBuf>,
    /// Container UUID → list of child container UUIDs (folder children)
    children: HashMap<Uuid, Vec<Uuid>>,
    /// Container UUID → parent container UUID (None = root)
    parents: HashMap<Uuid, Option<Uuid>>,
}

impl ContainerIndex {
    /// Look up a container by its UUID
    pub fn get(&self, id: &Uuid) -> Option<&StorageUnit> {
        self.by_id.get(id)
    }

    /// Look up a container by its filesystem path
    pub fn get_by_path(&self, path: &Path) -> Option<&StorageUnit> {
        self.by_path
            .get(path)
            .and_then(|id| self.by_id.get(id))
    }

    /// Get the filesystem folder path for a container
    pub fn folder_path(&self, id: &Uuid) -> Option<&PathBuf> {
        self.paths.get(id)
    }

    /// Get the parent container of a given container (None = root)
    pub fn parent_of(&self, id: &Uuid) -> Option<Option<Uuid>> {
        self.parents.get(id).copied()
    }

    /// Get the direct child containers of a given container
    pub fn children_of(&self, id: &Uuid) -> &[Uuid] {
        self.children.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Find which container a 3D point falls within.
    /// Searches deepest (most specific) containers first — a tool on a shelf inside
    /// a cabinet is reported as being on the shelf, not in the cabinet.
    pub fn find_container_for_point(&self, point: [f32; 3]) -> Option<&StorageUnit> {
        // Sort by nesting depth descending so deepest containers are checked first
        let mut candidates: Vec<(&StorageUnit, usize)> = self
            .by_id
            .values()
            .filter(|c| c.contains_point(point))
            .map(|c| {
                let depth = self
                    .paths
                    .get(&c.id)
                    .map(|p| p.components().count())
                    .unwrap_or(0);
                (c, depth)
            })
            .collect();

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.into_iter().next().map(|(c, _)| c)
    }

    /// All containers
    pub fn all(&self) -> impl Iterator<Item = &StorageUnit> {
        self.by_id.values()
    }

    /// Total number of registered containers
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Returns true if no containers are registered
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Insert a container into the index
    fn insert(&mut self, unit: StorageUnit, folder: PathBuf, parent_id: Option<Uuid>) {
        let id = unit.id;
        self.by_path.insert(folder.clone(), id);
        self.paths.insert(id, folder);
        self.parents.insert(id, parent_id);
        if let Some(parent) = parent_id {
            self.children.entry(parent).or_default().push(id);
        }
        self.by_id.insert(id, unit);
    }
}

// ============================================================================
// 8. StorageManager — filesystem manager
// ============================================================================

/// Manages the storage container folder tree and the autonomous movement of
/// `.tool.toml` files between containers based on GPS telemetry.
///
/// # Folder Convention
/// - Every container folder contains an `instance.toml` (the `StorageUnit` definition)
/// - Tool files (`*.tool.toml`) sit in the folder of their current container
/// - Nesting folders = physical nesting (drawer inside cabinet inside zone)
///
/// # GPS-Driven Autonomous Movement
/// When `relocate_tool_by_position()` is called with a new GPS position:
/// 1. `ContainerIndex::find_container_for_point()` identifies the deepest matching container
/// 2. If the tool's current folder differs from the target container's folder,
///    `std::fs::rename()` moves the file atomically
/// 3. A `ToolMovementRecord` is appended to `movement_log.toml`
pub struct StorageManager {
    /// Root directory (the `tools/` folder)
    root: PathBuf,
    /// In-memory container index
    pub index: ContainerIndex,
    /// Movement audit log (in-memory; flushed to movement_log.toml periodically)
    movement_log: Vec<ToolMovementRecord>,
}

impl StorageManager {
    /// Open the storage manager at the given tools root directory.
    /// Scans the entire folder tree for `instance.toml` files and builds the index.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();

        if !root.exists() {
            std::fs::create_dir_all(&root)
                .with_context(|| format!("Failed to create tools root: {}", root.display()))?;
        }

        let mut manager = Self {
            root,
            index: ContainerIndex::default(),
            movement_log: Vec::new(),
        };
        manager.ensure_active_use_folder()?;
        manager.reload()?;
        Ok(manager)
    }

    /// Ensure the `active-use/` folder exists at the tools root with its `instance.toml`.
    /// Called automatically on `open()`. The `active-use/` folder is the well-known
    /// container for tools currently being wielded — every workshop has exactly one.
    pub fn ensure_active_use_folder(&self) -> Result<()> {
        let folder = self.root.join("active-use");
        let instance_path = folder.join("instance.toml");

        if instance_path.exists() {
            return Ok(()); // Already set up
        }

        std::fs::create_dir_all(&folder)
            .with_context(|| "Failed to create active-use/ folder")?;

        let unit = StorageUnit {
            id: Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                b"eustress-workshop-active-use-container",
            ),
            name: "Active Use".into(),
            kind: StorageKind::ActiveUse,
            description: Some(
                "Tools currently being wielded. Populated automatically by kinetic GPS chip events.".into(),
            ),
            capacity: None,
            bounds: None,
            iot: None,
            mesh_path: None,
            color: Some("#ff6b35".into()), // Orange — visually distinct in the Explorer
            tags: vec!["active".into(), "in-use".into()],
            locked: false,
            created_at: Utc::now(),
        };

        let content = toml::to_string_pretty(&unit)
            .with_context(|| "Failed to serialise active-use instance.toml")?;
        std::fs::write(&instance_path, content)
            .with_context(|| "Failed to write active-use/instance.toml")?;

        tracing::info!("Created active-use/ container at {}", folder.display());
        Ok(())
    }

    /// Returns the path to the `active-use/` folder
    pub fn active_use_folder(&self) -> PathBuf {
        self.root.join("active-use")
    }

    /// Move a tool's `.tool.toml` into the `active-use/` folder.
    /// Called when a kinetic chip fires its **Departure** event (tool picked up).
    /// Returns the new path of the tool file.
    pub fn move_to_active_use(
        &mut self,
        tool_id: Uuid,
        tool_name: &str,
        current_path: &Path,
    ) -> Result<PathBuf> {
        let active_use_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            b"eustress-workshop-active-use-container",
        );
        self.move_tool(
            tool_id,
            tool_name,
            current_path,
            Some(active_use_id),
            MovementTrigger::GpsAutomatic,
            None,
        )
    }

    /// Move a tool's `.tool.toml` out of `active-use/` into its resolved container.
    /// Called when a kinetic chip fires its **Arrival** event (tool set down).
    /// `target_container_id` is the result of `ContainerIndex::find_container_for_point()`.
    /// Returns the new path of the tool file.
    pub fn return_from_active_use(
        &mut self,
        tool_id: Uuid,
        tool_name: &str,
        gps_position: [f32; 3],
    ) -> Result<PathBuf> {
        let current_path = self
            .active_use_folder()
            .join(format!("{}.tool.toml", tool_name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-")));

        let target_container_id = self
            .index
            .find_container_for_point(gps_position)
            .map(|c| c.id);

        self.move_tool(
            tool_id,
            tool_name,
            &current_path,
            target_container_id,
            MovementTrigger::GpsAutomatic,
            Some(gps_position),
        )
    }

    /// Recursively scan the folder tree for `instance.toml` files and rebuild the index
    pub fn reload(&mut self) -> Result<usize> {
        self.index = ContainerIndex::default();
        let count = self.scan_folder(&self.root.clone(), None)?;
        tracing::info!(
            "Storage manager loaded {} containers from {}",
            count,
            self.root.display()
        );
        Ok(count)
    }

    /// Recursive folder scanner — builds ContainerIndex from instance.toml files
    fn scan_folder(&mut self, folder: &Path, parent_id: Option<Uuid>) -> Result<usize> {
        let instance_path = folder.join("instance.toml");
        let mut count = 0;

        let current_parent_id = if instance_path.exists() {
            let content = std::fs::read_to_string(&instance_path)
                .with_context(|| format!("Cannot read {}", instance_path.display()))?;
            match toml::from_str::<StorageUnit>(&content) {
                Ok(unit) => {
                    let id = unit.id;
                    self.index.insert(unit, folder.to_path_buf(), parent_id);
                    count += 1;
                    Some(id)
                }
                Err(err) => {
                    tracing::warn!(
                        "Skipping malformed instance.toml at {}: {}",
                        folder.display(),
                        err
                    );
                    parent_id
                }
            }
        } else {
            parent_id
        };

        // Recurse into subdirectories
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    count += self.scan_folder(&path, current_parent_id)?;
                }
            }
        }

        Ok(count)
    }

    /// Create a new container folder with an `instance.toml` file.
    /// `parent_folder` must be an existing container folder (or the root).
    pub fn create_container(
        &mut self,
        parent_folder: &Path,
        unit: StorageUnit,
    ) -> Result<PathBuf> {
        // Derive a slug from the container name for the folder name
        let slug = unit
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let folder = parent_folder.join(&slug);
        std::fs::create_dir_all(&folder)
            .with_context(|| format!("Failed to create container folder: {}", folder.display()))?;

        let instance_path = folder.join("instance.toml");
        let content = toml::to_string_pretty(&unit)
            .with_context(|| format!("Failed to serialise StorageUnit: {}", unit.name))?;
        std::fs::write(&instance_path, &content)
            .with_context(|| format!("Failed to write {}", instance_path.display()))?;

        tracing::info!(
            "Created container '{}' ({}) at {}",
            unit.name,
            unit.kind.display_label(),
            folder.display()
        );

        // Determine parent ID from the parent folder path
        let parent_id = self.index.get_by_path(parent_folder).map(|u| u.id);
        self.index.insert(unit, folder.clone(), parent_id);
        Ok(folder)
    }

    /// Find which container a tool currently lives in, based on its `.tool.toml` path.
    /// Returns `None` if the tool is in the root tools folder (not inside any container).
    pub fn container_of_tool(&self, tool_toml_path: &Path) -> Option<&StorageUnit> {
        let parent_folder = tool_toml_path.parent()?;
        self.index.get_by_path(parent_folder)
    }

    /// Move a tool's `.tool.toml` file to the target container folder.
    /// Used by both manual moves (Explorer drag-and-drop) and GPS-driven automatic moves.
    ///
    /// Returns the new path of the tool file after the move.
    pub fn move_tool(
        &mut self,
        tool_id: Uuid,
        tool_name: &str,
        current_path: &Path,
        target_container_id: Option<Uuid>,
        trigger: MovementTrigger,
        gps_position: Option<[f32; 3]>,
    ) -> Result<PathBuf> {
        // Resolve the target folder
        let target_folder = match target_container_id {
            Some(id) => self
                .index
                .folder_path(&id)
                .cloned()
                .with_context(|| format!("Container {} not found in index", id))?,
            None => self.root.clone(), // No container = root tools folder
        };

        // Determine the current container for the audit log
        let from_container = self.container_of_tool(current_path);
        let from_container_id = from_container.map(|c| c.id);
        let from_path_str = from_container.map(|c| c.name.clone());

        let to_container = target_container_id.and_then(|id| self.index.get(&id));
        let to_path_str = to_container.map(|c| c.name.clone());

        // Compute target file path
        let filename = current_path
            .file_name()
            .with_context(|| "Tool path has no filename")?;
        let new_path = target_folder.join(filename);

        // Only move if the destination is actually different
        if current_path == new_path {
            return Ok(new_path);
        }

        // Atomic file move
        std::fs::rename(current_path, &new_path).with_context(|| {
            format!(
                "Failed to move {} → {}",
                current_path.display(),
                new_path.display()
            )
        })?;

        tracing::info!(
            "Moved tool '{}' {} → {}",
            tool_name,
            from_path_str.as_deref().unwrap_or("root"),
            to_path_str.as_deref().unwrap_or("root"),
        );

        // Append to in-memory movement log
        self.movement_log.push(ToolMovementRecord {
            tool_id,
            tool_name: tool_name.to_string(),
            from_container_id,
            from_path: from_path_str,
            to_container_id: target_container_id,
            to_path: to_path_str,
            trigger,
            gps_position,
            moved_at: Utc::now(),
        });

        Ok(new_path)
    }

    /// GPS-driven autonomous relocation.
    /// Called when `LiveStatusStore` receives a new position for a GPS-tracked tool.
    /// Determines the deepest matching container, and if different from the current
    /// container, moves the `.tool.toml` file automatically.
    ///
    /// Returns `Some(new_path)` if the file was moved, `None` if it stayed put.
    pub fn relocate_tool_by_position(
        &mut self,
        tool_id: Uuid,
        tool_name: &str,
        current_tool_path: &Path,
        new_position: [f32; 3],
    ) -> Result<Option<PathBuf>> {
        let target_container = self.index.find_container_for_point(new_position);
        let target_id = target_container.map(|c| c.id);

        let current_container = self.container_of_tool(current_tool_path).map(|c| c.id);

        if target_id == current_container {
            return Ok(None); // Tool is already in the right container — no move needed
        }

        let new_path = self.move_tool(
            tool_id,
            tool_name,
            current_tool_path,
            target_id,
            MovementTrigger::GpsAutomatic,
            Some(new_position),
        )?;

        Ok(Some(new_path))
    }

    /// Flush the in-memory movement log to `movement_log.toml` in the tools root.
    /// Appends to the existing file — full audit trail, never truncated.
    pub fn flush_movement_log(&mut self) -> Result<()> {
        if self.movement_log.is_empty() {
            return Ok(());
        }

        let log_path = self.root.join("movement_log.toml");

        // Load existing log entries
        let mut existing: Vec<ToolMovementRecord> = if log_path.exists() {
            let content = std::fs::read_to_string(&log_path)
                .with_context(|| "Failed to read movement_log.toml")?;
            toml::from_str::<MovementLog>(&content)
                .map(|l| l.entries)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        existing.extend(self.movement_log.drain(..));

        let log = MovementLog { entries: existing };
        let content = toml::to_string_pretty(&log)
            .with_context(|| "Failed to serialise movement log")?;
        std::fs::write(&log_path, content)
            .with_context(|| "Failed to write movement_log.toml")?;

        Ok(())
    }

    /// Returns the movement log entries (in-memory only — not yet flushed)
    pub fn pending_movements(&self) -> &[ToolMovementRecord] {
        &self.movement_log
    }

    /// Returns the root tools directory
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns a tree-structured summary of all containers and their contents
    /// for rendering in the Explorer panel
    pub fn tree_summary(&self) -> Vec<ContainerSummary> {
        let mut summaries = Vec::new();
        self.collect_summaries(None, 0, &mut summaries);
        summaries
    }

    fn collect_summaries(
        &self,
        parent_id: Option<Uuid>,
        depth: u32,
        out: &mut Vec<ContainerSummary>,
    ) {
        let children: Vec<Uuid> = self
            .index
            .all()
            .filter(|c| self.index.parent_of(&c.id) == Some(parent_id))
            .map(|c| c.id)
            .collect();

        for id in children {
            if let Some(container) = self.index.get(&id) {
                out.push(ContainerSummary {
                    id,
                    name: container.name.clone(),
                    kind: container.kind.clone(),
                    depth,
                    folder_path: self
                        .index
                        .folder_path(&id)
                        .cloned()
                        .unwrap_or_default(),
                });
                self.collect_summaries(Some(id), depth + 1, out);
            }
        }
    }
}

/// TOML wrapper for the movement audit log file
#[derive(Debug, Serialize, Deserialize, Default)]
struct MovementLog {
    entries: Vec<ToolMovementRecord>,
}

// ============================================================================
// 9. ContainerSummary — lightweight view for Explorer tree rendering
// ============================================================================

/// Lightweight summary of a container for the Explorer tree.
/// Does not include full `StorageUnit` — just what the tree needs to render.
#[derive(Debug, Clone)]
pub struct ContainerSummary {
    pub id: Uuid,
    pub name: String,
    pub kind: StorageKind,
    /// Nesting depth (0 = top-level zone, 1 = inside a zone, etc.)
    pub depth: u32,
    /// Filesystem folder path for this container
    pub folder_path: PathBuf,
}

// ============================================================================
// 10. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounding_volume_contains_point() {
        let bounds = BoundingVolume {
            min: [0.0, 0.0, 0.0],
            max: [2.0, 1.0, 1.0],
        };
        assert!(bounds.contains([1.0, 0.5, 0.5]));
        assert!(!bounds.contains([3.0, 0.5, 0.5]));
    }

    #[test]
    fn storage_unit_slug_from_name() {
        let unit = StorageUnit::new("Tool Cabinet A", StorageKind::Cabinet);
        let slug = unit
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        assert_eq!(slug, "tool-cabinet-a");
    }

    #[test]
    fn storage_unit_roundtrips_through_toml() {
        let mut unit = StorageUnit::new("Bench 3", StorageKind::Bench);
        unit.bounds = Some(BoundingVolume {
            min: [1.0, 0.0, 2.0],
            max: [3.0, 1.0, 4.0],
        });
        let serialised = toml::to_string_pretty(&unit).unwrap();
        let deserialised: StorageUnit = toml::from_str(&serialised).unwrap();
        assert_eq!(deserialised.name, unit.name);
        assert_eq!(deserialised.id, unit.id);
        assert!(deserialised.bounds.is_some());
    }

    #[test]
    fn container_index_finds_deepest_container_for_point() {
        let mut index = ContainerIndex::default();

        // Zone: 0,0,0 → 10,5,10
        let mut zone = StorageUnit::new("CNC Bay", StorageKind::Zone);
        zone.bounds = Some(BoundingVolume { min: [0.0, 0.0, 0.0], max: [10.0, 5.0, 10.0] });
        let zone_id = zone.id;
        index.insert(zone, PathBuf::from("tools/cnc-bay"), None);

        // Bench inside zone: 1,0,1 → 4,1,4
        let mut bench = StorageUnit::new("CNC Bench", StorageKind::Bench);
        bench.bounds = Some(BoundingVolume { min: [1.0, 0.0, 1.0], max: [4.0, 1.0, 4.0] });
        index.insert(bench, PathBuf::from("tools/cnc-bay/cnc-bench"), Some(zone_id));

        // A point on the bench — should return the bench (deepest match)
        let result = index.find_container_for_point([2.0, 0.5, 2.0]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "CNC Bench");

        // A point in the zone but NOT on the bench
        let result2 = index.find_container_for_point([8.0, 2.0, 8.0]);
        assert!(result2.is_some());
        assert_eq!(result2.unwrap().name, "CNC Bay");
    }
}
