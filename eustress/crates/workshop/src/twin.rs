//! # twin
//!
//! Bevy ECS integration for the physical-digital twin workshop.
//! Each `.tool.toml` file in the registry spawns as an instanced mesh entity
//! in the 3D Space. Live IoT telemetry from `LiveStatusStore` drives entity
//! transforms so tools appear at their real-time physical locations.
//!
//! ## Table of Contents
//!
//! | Section               | Purpose                                                      |
//! |-----------------------|--------------------------------------------------------------|
//! | `ToolComponent`       | ECS component attached to every spawned tool entity          |
//! | `WorkshopTwinState`   | Resource holding registry + live status accessible to Bevy   |
//! | `WorkshopTwinPlugin`  | Bevy plugin — registers systems and resources                |
//! | Systems               | spawn_tool_entities, sync_tool_transforms, sync_tool_status  |
//!
//! ## How it works
//!
//! 1. `WorkshopTwinPlugin` is added to the Bevy `App`.
//! 2. On `Startup`, `spawn_tool_entities` reads every `RegisteredTool` from the
//!    `WorkshopTwinState` resource and spawns a `SceneBundle` entity (or a placeholder
//!    cube if no mesh is configured) with a `ToolComponent` attached.
//! 3. Each `Update` tick, `sync_tool_transforms` checks `LiveStatusStore` for updated
//!    GPS positions and moves the entity's `Transform` accordingly.
//! 4. The file watcher (external, via `notify`) calls `WorkshopTwinState::reload()`
//!    which triggers `despawn_removed_tools` and `spawn_tool_entities` to reconcile.

use bevy::prelude::*;
use uuid::Uuid;

use crate::registry::ToolRegistry;
use crate::status::{LiveStatusStore, OperationalState};

// ============================================================================
// 1. ECS Components
// ============================================================================

/// ECS component attached to every tool entity spawned in the digital twin Space.
/// This links the Bevy entity back to the `.tool.toml` registry entry.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct ToolComponent {
    /// Stable UUID matching `RegisteredTool.id` — never changes
    pub tool_id: Uuid,
    /// Human-readable name (cached from the TOML for fast label rendering)
    pub name: String,
    /// Home location label (used when live telemetry is unavailable)
    pub home_location: String,
    /// Whether this tool currently has a live IoT chip connection
    pub is_iot_tracked: bool,
}

/// Marker component for tool entities whose live position has been updated
/// this frame from the IoT telemetry store
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct ToolPositionUpdated;

/// Marker component for tool entities that are currently unavailable
/// (in use, checked out, or missing) — used for visual highlighting
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct ToolUnavailable;

// ============================================================================
// 2. WorkshopTwinState — shared resource
// ============================================================================

/// Bevy resource that owns the `ToolRegistry` and `LiveStatusStore`.
/// Inserted into the World by `WorkshopTwinPlugin` on startup.
/// Systems access this to read tool definitions and live telemetry.
#[derive(Resource)]
pub struct WorkshopTwinState {
    /// The TOML-backed tool registry (in-memory index + file manager)
    pub registry: ToolRegistry,
    /// Latest IoT telemetry for all tracked tools
    pub live_status: LiveStatusStore,
    /// Whether the registry has been modified since the last entity sync
    pub dirty: bool,
}

impl WorkshopTwinState {
    /// Create a new twin state by opening the registry at the given path
    pub fn new(tools_dir: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        Ok(Self {
            registry: ToolRegistry::open(tools_dir)?,
            live_status: LiveStatusStore::default(),
            dirty: true,
        })
    }

    /// Mark the registry as needing an entity sync (called by file watcher)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

// ============================================================================
// 3. Events
// ============================================================================

/// Fired when a tool entity's position has been updated from live telemetry
#[derive(Event, Debug, Clone)]
pub struct ToolPositionChangedEvent {
    pub tool_id: Uuid,
    pub new_position: Vec3,
}

/// Fired when a tool's operational state changes (e.g. Available → InUse)
#[derive(Event, Debug, Clone)]
pub struct ToolStateChangedEvent {
    pub tool_id: Uuid,
    pub new_state: String,
}

// ============================================================================
// 4. Systems
// ============================================================================

/// Startup system — spawns a Bevy entity for every tool in the registry.
/// Called once at startup and again whenever `WorkshopTwinState.dirty` is true.
fn spawn_tool_entities(
    mut commands: Commands,
    mut twin: ResMut<WorkshopTwinState>,
    existing: Query<(Entity, &ToolComponent)>,
    asset_server: Res<AssetServer>,
) {
    if !twin.dirty {
        return;
    }
    twin.dirty = false;

    // Build set of already-spawned tool IDs to avoid duplicates
    let already_spawned: std::collections::HashSet<Uuid> =
        existing.iter().map(|(_, tc)| tc.tool_id).collect();

    let tools: Vec<_> = twin
        .registry
        .index()
        .all()
        .filter(|t| !already_spawned.contains(&t.id))
        .cloned()
        .collect();

    for tool in tools {
        // Determine the spawn position — use live telemetry if available,
        // otherwise place at the origin (the Properties Panel shows home_location label)
        let position = twin
            .live_status
            .get(&tool.id)
            .and_then(|t| t.location.space_position)
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .unwrap_or(Vec3::ZERO);

        let transform = Transform::from_translation(position).with_scale(
            Vec3::splat(tool.mesh.scale),
        );

        let tool_component = ToolComponent {
            tool_id: tool.id,
            name: tool.name.clone(),
            home_location: tool.home_location.clone(),
            is_iot_tracked: tool.iot.is_some(),
        };

        // Spawn with the configured mesh if it exists, otherwise a placeholder transform node
        let mesh_path = tool.mesh.mesh_path.clone();
        if !mesh_path.is_empty() && mesh_path != "assets/models/tools/generic_tool.glb" {
            commands.spawn((
                SceneRoot(asset_server.load(format!("{}#Scene0", mesh_path))),
                transform,
                tool_component,
                Name::new(format!("Tool: {}", tool.name)),
            ));
        } else {
            // Placeholder entity with no mesh — still trackable and selectable
            commands.spawn((
                transform,
                Visibility::default(),
                tool_component,
                Name::new(format!("Tool: {}", tool.name)),
            ));
        }

        tracing::debug!("Spawned digital twin entity for tool: {}", tool.name);
    }
}

/// Despawn tool entities whose registry entry has been removed.
/// Called when `WorkshopTwinState` is reloaded after a file deletion.
fn despawn_removed_tools(
    mut commands: Commands,
    twin: Res<WorkshopTwinState>,
    query: Query<(Entity, &ToolComponent)>,
) {
    for (entity, tool_component) in query.iter() {
        if twin.registry.index().get(&tool_component.tool_id).is_none() {
            commands.entity(entity).despawn();
            tracing::info!(
                "Despawned digital twin entity for removed tool: {}",
                tool_component.name
            );
        }
    }
}

/// Update system — syncs live IoT telemetry positions to entity Transforms.
/// Runs every frame but only moves entities when a new telemetry payload has arrived.
fn sync_tool_transforms(
    twin: Res<WorkshopTwinState>,
    mut query: Query<(&ToolComponent, &mut Transform)>,
    mut position_events: EventWriter<ToolPositionChangedEvent>,
) {
    for (tool_component, mut transform) in query.iter_mut() {
        let Some(telemetry) = twin.live_status.get(&tool_component.tool_id) else {
            continue;
        };

        let Some(space_pos) = telemetry.location.space_position else {
            continue;
        };

        let new_pos = Vec3::new(space_pos[0], space_pos[1], space_pos[2]);

        // Only update if position has meaningfully changed (>1mm threshold)
        if transform.translation.distance(new_pos) > 0.001 {
            transform.translation = new_pos;
            position_events.write(ToolPositionChangedEvent {
                tool_id: tool_component.tool_id,
                new_position: new_pos,
            });
        }
    }
}

/// Update system — adds/removes `ToolUnavailable` marker component based on live state.
/// Drives visual highlighting in the 3D Space (materials, gizmos, etc.)
fn sync_tool_availability(
    mut commands: Commands,
    twin: Res<WorkshopTwinState>,
    query: Query<(Entity, &ToolComponent)>,
    mut state_events: EventWriter<ToolStateChangedEvent>,
) {
    for (entity, tool_component) in query.iter() {
        let state = twin.live_status.state_of(&tool_component.tool_id);
        let is_unavailable = !state.is_assignable();

        if is_unavailable {
            commands.entity(entity).insert(ToolUnavailable);
            state_events.write(ToolStateChangedEvent {
                tool_id: tool_component.tool_id,
                new_state: state.display_label().to_string(),
            });
        } else {
            commands.entity(entity).remove::<ToolUnavailable>();
        }
    }
}

// ============================================================================
// 5. WorkshopTwinPlugin
// ============================================================================

/// Bevy plugin that registers the digital twin systems, components, and events.
///
/// # Usage
/// ```rust,no_run
/// use eustress_workshop::twin::{WorkshopTwinPlugin, WorkshopTwinState};
///
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .insert_resource(WorkshopTwinState::new("path/to/workshop/tools").unwrap())
///     .add_plugins(WorkshopTwinPlugin)
///     .run();
/// ```
pub struct WorkshopTwinPlugin;

impl Plugin for WorkshopTwinPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register reflected types for the Properties Panel
            .register_type::<ToolComponent>()
            .register_type::<ToolPositionUpdated>()
            .register_type::<ToolUnavailable>()
            // Events
            .add_event::<ToolPositionChangedEvent>()
            .add_event::<ToolStateChangedEvent>()
            // Startup: spawn entities for all registered tools
            .add_systems(Startup, spawn_tool_entities)
            // Update: sync live telemetry → entity transforms + availability markers
            .add_systems(
                Update,
                (
                    spawn_tool_entities,
                    despawn_removed_tools,
                    sync_tool_transforms,
                    sync_tool_availability,
                )
                    .chain(),
            );
    }
}
