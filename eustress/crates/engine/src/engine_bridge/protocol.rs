//! JSON-RPC 2.0 protocol types + per-method handler dispatch.
//!
//! The wire format is plain JSON-RPC over newline-delimited JSON on a
//! TCP connection. Requests look like:
//!
//! ```json
//! {"jsonrpc":"2.0","id":1,"method":"sim.read","params":{"keys":["battery.voltage"]}}
//! ```
//!
//! Responses mirror the JSON-RPC spec:
//!
//! ```json
//! {"jsonrpc":"2.0","id":1,"result":{"values":{"battery.voltage":3.72}}}
//! ```
//!
//! Methods are modelled as a [`MethodName`] enum rather than strings
//! so the dispatcher is a single `match` on an enum variant — less
//! chance of typos and the compiler tracks exhaustiveness when new
//! methods are added.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Request / response frames
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeRequest {
    #[serde(default)]
    #[allow(dead_code)]
    pub jsonrpc: String,
    /// Request id — echoed back in the response. Can be a number, string,
    /// or null per JSON-RPC 2.0.
    pub id: Value,
    /// Method name, parsed into a [`MethodName`] at dispatch time.
    #[serde(deserialize_with = "deserialize_method")]
    pub method: MethodName,
    /// Method-specific parameters.
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

impl BridgeResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn error(id: Value, error: BridgeError) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(error) }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl BridgeError {
    pub fn method_not_found(name: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {}", name),
            data: None,
        }
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self { code: -32602, message: msg.into(), data: None }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self { code: -32603, message: msg.into(), data: None }
    }
}

// ---------------------------------------------------------------------------
// Method names
// ---------------------------------------------------------------------------

/// Enumerates every method the bridge exposes. Grow this as new live
/// surfaces land (ecs.raycast, embedvec.search, mention.resolve, …).
///
/// `Unknown(String)` captures unrecognised method names so the
/// dispatcher can return a proper "method not found" error instead of
/// failing to deserialise the request entirely.
#[derive(Debug, Clone)]
pub enum MethodName {
    Ping,
    SimRead,
    EcsQuery,
    /// Detailed live scene snapshot — per-entity class / transform / mesh
    /// / material / render+physics flags / parent / on-disk source, plus
    /// frame stats (FPS). This is the "AI inspects the running engine"
    /// surface: far richer than `ecs.query`'s id/name/class, so a data bug
    /// like a wrong `asset.mesh` (the V-Cell block-render regression) is
    /// visible directly. Read-only; safe to call every frame.
    EcsInspect,
    /// Set the active editor tool (select/move/scale/rotate) — the AI
    /// "equip tool" action; mirrors the Alt+Z/X/C/V shortcuts.
    ToolEquip,
    /// Replace the current selection with a set of entity ids — the AI
    /// "select object(s)" action. Drives gizmos + Properties downstream.
    SelectionSet,
    /// Read live editor state (active tool + current selection) so the AI
    /// can query the result of its own actions. The queryable complement.
    StateGet,
    /// Invoke any editor Action by name (Copy/Cut/Paste/Duplicate/Group/
    /// Ungroup/Delete/SelectAll/Undo/Redo/SaveScene/tool switches/…) — the
    /// AI "press the keyboard shortcut" surface. Writes the same
    /// `MenuActionEvent` the keybinding layer produces, so the FULL action
    /// path runs identically to a real key press. The core of systematic
    /// editor testing over MCP.
    ActionInvoke,
    /// Capture the primary window (3D viewport + UI overlay) to a PNG on
    /// disk and return its path — the AI's "eyes". The viewport renders to
    /// the window, so this is exactly what a human sees. Read the returned
    /// path with the file reader to view the frame.
    ViewportCapture,
    /// Place the independent AI camera (position + look-at/rotation) — the
    /// AI's own off-screen camera, independent of the user's viewport.
    AiCameraSetPose,
    /// Orbit the AI camera around a point (center/distance/yaw/pitch).
    AiCameraOrbit,
    /// Frame a named entity in the AI camera (auto-distance from its size).
    AiCameraFrame,
    /// Render the AI camera's independent view to a PNG and return the path —
    /// the AI's own eyes, separate from `viewport.capture` (which is the
    /// user's window). On-demand: powers the off-screen camera up only for
    /// the capture.
    AiCameraCapture,
    /// List every registered Workshop tool — MCP's `tools/list` proxies
    /// to this so external IDEs see the same 52+ tool surface Workshop has.
    ToolsList,
    /// Dispatch a tool by name — MCP's `tools/call` proxies to this, so
    /// external IDEs execute tools in-process inside the engine with
    /// full ECS access rather than re-implementing them out-of-process.
    ToolsCall,
    // ── Binary-ECS entity CRUD (Phase 3 — AI-on-binary) ──────────────
    // The in-engine path for MCP entity tools to locate + edit binary-ECS
    // cores (which live in Fjall, not on disk — the MCP server can't open
    // the single-writer DB). Each handler operates on the LIVE entity when
    // resident, else directly on the DB core (streamed-out / large Space).
    /// Create a binary-ECS entity (reuses `spawn_binary_instance`).
    EntityCreate,
    /// Read an entity as a TOML/JSON projection (the AI's editable view).
    EntityRead,
    /// Patch an entity's properties (position/size/color/material/…).
    EntityUpdate,
    /// Delete an entity (purges all DB stores; despawns if resident).
    EntityDelete,
    /// Find entities by uuid / path / class (identity indices ∪ live ECS).
    EntityFind,
    /// Add a CollectionService tag.
    EntityAddTag,
    /// Remove a CollectionService tag.
    EntityRemoveTag,
    /// Phase 3.5 — materialize a binary-ECS entity to an on-disk TOML folder
    /// (explicit "Export to disk"; preserves uuid + visuals).
    EntityPromote,
    /// Phase 3.5 — fold a bare, artifact-free FileSystem entity back to binary.
    EntityDemote,
    /// Tail the causal op-log (Phase 1, Way 8) — the most recent N mutation
    /// records (create/delete with provenance) as serde views. The AI's
    /// "what changed, in order, and why" read surface. Read-only.
    OplogTail,
    Unknown(String),
}

fn deserialize_method<'de, D>(de: D) -> Result<MethodName, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    Ok(match s.as_str() {
        "ping" => MethodName::Ping,
        "sim.read" => MethodName::SimRead,
        "ecs.query" => MethodName::EcsQuery,
        "ecs.inspect" => MethodName::EcsInspect,
        "tool.equip" => MethodName::ToolEquip,
        "selection.set" => MethodName::SelectionSet,
        "state.get" => MethodName::StateGet,
        "action.invoke" => MethodName::ActionInvoke,
        "viewport.capture" => MethodName::ViewportCapture,
        "ai_camera.set_pose" => MethodName::AiCameraSetPose,
        "ai_camera.orbit" => MethodName::AiCameraOrbit,
        "ai_camera.frame" => MethodName::AiCameraFrame,
        "ai_camera.capture" => MethodName::AiCameraCapture,
        "tools.list" => MethodName::ToolsList,
        "tools.call" => MethodName::ToolsCall,
        "entity.create" => MethodName::EntityCreate,
        "entity.read" => MethodName::EntityRead,
        "entity.update" => MethodName::EntityUpdate,
        "entity.delete" => MethodName::EntityDelete,
        "entity.find" => MethodName::EntityFind,
        "entity.add_tag" => MethodName::EntityAddTag,
        "entity.remove_tag" => MethodName::EntityRemoveTag,
        "entity.promote" => MethodName::EntityPromote,
        "entity.demote" => MethodName::EntityDemote,
        "oplog.tail" => MethodName::OplogTail,
        _ => MethodName::Unknown(s),
    })
}

// ---------------------------------------------------------------------------
// Handlers — Bevy-main-thread execution of each method
// ---------------------------------------------------------------------------

pub mod handlers {
    use super::*;
    use bevy::prelude::*;

    /// Trivial health check — lets siblings verify the bridge is alive
    /// without touching any engine state.
    pub fn ping(req: &BridgeRequest) -> BridgeResponse {
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({ "pong": true, "engine": "eustress" }),
        )
    }

    /// Tail the causal op-log — the most recent `limit` mutation records
    /// (default 50, capped 1000), oldest-first, as serde `MutationView`s. No
    /// World access (reads the active-DB op-log static). Read-only.
    pub fn oplog_tail(req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("oplog.tail: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n.min(1000) as usize)
                .unwrap_or(50);
            let views = crate::space::active_db::tail_mutations(limit);
            match serde_json::to_value(&views) {
                Ok(json) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "count": views.len(), "mutations": json }),
                ),
                Err(e) => BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal(format!("oplog.tail serialize: {e}")),
                ),
            }
        }
    }

    // ── Binary-ECS entity CRUD (Phase 3 — AI-on-binary) ──────────────────
    // The in-engine path for the MCP entity tools to locate + edit binary
    // cores (which live in Fjall, not on disk). Each handler operates on the
    // LIVE entity when resident, else directly on the DB core.
    /// Parse a `[x, y, z]` JSON array param into `[f32; 3]`, else `default`.
    /// (Named `param_vec3` to avoid colliding with the ai_camera handlers'
    /// existing 1-arg `parse_vec3(Option<&Value>) -> Option<Vec3>`.)
    #[cfg(feature = "world-db")]
    fn param_vec3(params: &Value, key: &str, default: [f32; 3]) -> [f32; 3] {
        params
            .get(key)
            .and_then(|v| v.as_array())
            .and_then(|a| {
                if a.len() >= 3 {
                    Some([
                        a[0].as_f64()? as f32,
                        a[1].as_f64()? as f32,
                        a[2].as_f64()? as f32,
                    ])
                } else {
                    None
                }
            })
            .unwrap_or(default)
    }

    /// Create a binary-ECS Part — reuses `spawn_binary_instance` (the same
    /// path the Insert menu uses), so it's persisted + identity-indexed
    /// identically. Only bare Parts go binary; other classes return
    /// `routed:"filesystem"` so the MCP wrapper does a disk create (matching
    /// the Insert-menu split). Params mirror the disk `create_entity`.
    pub fn entity_create(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.create: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            use bevy::ecs::system::SystemState;
            use crate::space::instance_loader::{
                AssetReference, InstanceDefinition, InstanceMetadata, InstanceProperties,
                PrimitiveMeshCache, TransformData,
            };
            use crate::space::material_loader::MaterialRegistry;
            use crate::space::service_loader::ServiceComponent;

            let class = req.params.get("class").and_then(|v| v.as_str()).unwrap_or("Part");
            if !class.eq_ignore_ascii_case("Part") {
                return BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({
                        "routed": "filesystem",
                        "reason": format!("class '{class}' uses the filesystem representation"),
                    }),
                );
            }
            let shape = req.params.get("shape").and_then(|v| v.as_str()).unwrap_or("block");
            let mesh = match shape.to_ascii_lowercase().as_str() {
                "ball" | "sphere" => "parts/ball.glb",
                "cylinder" => "parts/cylinder.glb",
                "wedge" => "parts/wedge.glb",
                "cornerwedge" | "corner_wedge" => "parts/corner_wedge.glb",
                "cone" => "parts/cone.glb",
                _ => "parts/block.glb",
            };
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("Part").to_string();
            let position = param_vec3(&req.params, "position", [0.0, 0.0, 0.0]);
            let size = param_vec3(&req.params, "size", [1.0, 1.0, 1.0]);
            let color = param_vec3(&req.params, "color", [0.639, 0.635, 0.647]);
            let material = req
                .params
                .get("material")
                .and_then(|v| v.as_str())
                .unwrap_or("Plastic")
                .to_string();
            let anchored = req.params.get("anchored").and_then(|v| v.as_bool()).unwrap_or(true);
            let can_collide = req.params.get("can_collide").and_then(|v| v.as_bool()).unwrap_or(true);
            let now = chrono::Utc::now().to_rfc3339();

            let def = InstanceDefinition {
                nuclear: None,
                plasma: None,
                asset: Some(AssetReference { mesh: mesh.to_string(), scene: "Scene0".to_string() }),
                transform: TransformData {
                    position,
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: size,
                },
                properties: InstanceProperties {
                    color: [color[0], color[1], color[2], 1.0],
                    transparency: 0.0,
                    anchored,
                    can_collide,
                    cast_shadow: true,
                    reflectance: 0.0,
                    material,
                    locked: false,
                    physics: None,
                    respect_gltf_materials: false,
                },
                metadata: InstanceMetadata {
                    class_name: "Part".to_string(),
                    archivable: true,
                    name: Some(name),
                    created: now.clone(),
                    last_modified: now,
                    created_by: None,
                    modifications: Vec::new(),
                    unit: None,
                    uuid: None,
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

            enum Outcome {
                NoWorkspace,
                Done(Option<crate::space::world_db_binary::SpawnedBinary>),
            }
            let mut state: SystemState<(
                Commands,
                Res<AssetServer>,
                ResMut<Assets<StandardMaterial>>,
                ResMut<MaterialRegistry>,
                ResMut<PrimitiveMeshCache>,
                Res<crate::space::SpaceRoot>,
                Query<(Entity, &ServiceComponent)>,
            )> = SystemState::new(world);
            let outcome = {
                let (mut commands, asset_server, mut materials, mut material_registry, mut mesh_cache, space_root, services) =
                    state.get_mut(world).unwrap(); // 0.19: SystemState::get_mut now returns Result
                match services
                    .iter()
                    .find(|(_, s)| s.class_name == "Workspace")
                    .map(|(e, _)| e)
                {
                    None => Outcome::NoWorkspace,
                    Some(ws) => Outcome::Done(crate::space::world_db_binary::spawn_binary_instance(
                        &mut commands,
                        &asset_server,
                        &mut materials,
                        &mut material_registry,
                        &mut mesh_cache,
                        &space_root.0,
                        ws,
                        def,
                    )),
                }
            };
            // Flush the deferred spawn/insert commands (mandatory — else the
            // entity is dropped). The DB write inside spawn_binary_instance is
            // synchronous, so the core persists regardless.
            state.apply(world);

            return match outcome {
                Outcome::NoWorkspace => BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal("entity.create: Workspace service not ready"),
                ),
                Outcome::Done(Some(s)) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({
                        "routed": "binary",
                        "uuid": s.uuid,
                        "stored_id": s.stored_id,
                        "entity": format!("{}v{}", s.entity.index(), s.entity.generation()),
                        "position": s.pos,
                    }),
                ),
                Outcome::Done(None) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({
                        "routed": "filesystem",
                        "reason": "router kept this on the filesystem (custom mesh / file-natured)",
                    }),
                ),
            };
        }
    }
    /// Read an entity as an editable TOML/JSON projection — the AI's
    /// "open the file" for a binary core. Resident-first (projects the LIVE
    /// components, freshest), else reads the DB core by uuid (streamed-out /
    /// large Space). Params: `uuid` (preferred) | `name`.
    pub fn entity_read(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.read: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            use eustress_common::classes::{BasePart, Instance};
            use eustress_common::Tags;

            let uuid = req.params.get("uuid").and_then(|v| v.as_str()).map(str::to_string);
            let name = req.params.get("name").and_then(|v| v.as_str()).map(str::to_string);
            if uuid.is_none() && name.is_none() {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params("entity.read: provide `uuid` or `name`"),
                );
            }

            // Resident-first: project the LIVE entity (freshest — avoids the
            // ≤1-frame mirror lag). `core_from_components` is the same bake the
            // save mirror uses, so the projection matches the persisted core.
            let mut q = world.query::<(
                &Instance,
                &Transform,
                Option<&BasePart>,
                Option<&Tags>,
                Option<&crate::spawn::MeshSource>,
            )>();
            let resident: Option<eustress_worlddb::ArchInstanceCore> =
                q.iter(world).find_map(|(inst, tf, bp, tags, mesh)| {
                    let hit = match (&uuid, &name) {
                        (Some(u), _) => &inst.uuid == u,
                        (None, Some(n)) => &inst.name == n,
                        _ => false,
                    };
                    if !hit {
                        return None;
                    }
                    let mesh_path = mesh.map(|m| m.path.as_str()).unwrap_or("");
                    Some(crate::space::world_db_binary::core_from_components(
                        inst, tf, bp, tags, mesh_path,
                    ))
                });

            let (resident_flag, core) = match resident {
                Some(core) => (true, Some(core)),
                None => {
                    // Non-resident: read the DB core by uuid (the durable key;
                    // a name match needs a live entity).
                    let core = uuid.as_deref().and_then(|u| {
                        let db = world
                            .get_resource::<crate::space::world_db_plugin::WorldDbHandle>()
                            .and_then(|h| h.0.clone());
                        db.and_then(|db| {
                            crate::space::world_db_binary::find_entity_by_uuid(db.as_ref(), u)
                                .ok()
                                .flatten()
                        })
                    });
                    (false, core)
                }
            };

            let Some(core) = core else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params(
                        "entity.read: no entity found (resident or in DB) for the given uuid/name",
                    ),
                );
            };

            let def = crate::space::arch_instance::arch_to_instance(&core);
            let definition = serde_json::to_value(&def).unwrap_or(Value::Null);
            let toml = toml::to_string_pretty(&def).ok();
            return BridgeResponse::ok(
                req.id.clone(),
                serde_json::json!({
                    "uuid": def.metadata.uuid,
                    "name": def.metadata.name,
                    "class": def.metadata.class_name,
                    "resident": resident_flag,
                    "definition": definition,
                    "toml": toml,
                }),
            );
        }
    }
    /// Optional `[x,y,z]` patch field → `Some([f32;3])` when present+valid.
    #[cfg(feature = "world-db")]
    fn opt_vec3(params: &Value, key: &str) -> Option<[f32; 3]> {
        params.get(key).and_then(|v| v.as_array()).and_then(|a| {
            if a.len() >= 3 {
                Some([
                    a[0].as_f64()? as f32,
                    a[1].as_f64()? as f32,
                    a[2].as_f64()? as f32,
                ])
            } else {
                None
            }
        })
    }

    /// Patch an entity's properties. RESIDENT: mutate the live components
    /// (Transform/BasePart) — `Changed<>` propagates to material_sync
    /// (render) and the mirror (persists BOTH cores). NON-RESIDENT: mutate
    /// the DB core and `mirror_binary_core` it. Patch fields (all optional):
    /// position/size/color/material/transparency/anchored/can_collide.
    /// (Stream-delta + undo emission is the remaining `apply_entity_patch`
    /// polish; render + persistence already propagate via change detection.)
    pub fn entity_update(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.update: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            use eustress_common::classes::{BasePart, Instance};
            use eustress_common::instance_create::uuid_hex_to_bytes;

            let uuid = req.params.get("uuid").and_then(|v| v.as_str()).map(str::to_string);
            let name = req.params.get("name").and_then(|v| v.as_str()).map(str::to_string);
            if uuid.is_none() && name.is_none() {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params("entity.update: provide `uuid` or `name`"),
                );
            }
            let position = opt_vec3(&req.params, "position");
            let size = opt_vec3(&req.params, "size");
            let color = opt_vec3(&req.params, "color");
            let material = req.params.get("material").and_then(|v| v.as_str()).map(str::to_string);
            let transparency = req.params.get("transparency").and_then(|v| v.as_f64()).map(|f| f as f32);
            let anchored = req.params.get("anchored").and_then(|v| v.as_bool());
            let can_collide = req.params.get("can_collide").and_then(|v| v.as_bool());

            // Resolve a resident entity by uuid/name, noting whether it's a
            // binary-ECS entity (the only kind the mirror persists).
            let mut q = world.query::<(
                Entity,
                &Instance,
                bevy::prelude::Has<crate::space::world_db_binary::BinaryEcsInstance>,
            )>();
            let target = q.iter(world).find_map(|(e, inst, is_binary)| {
                let hit = match (&uuid, &name) {
                    (Some(u), _) => &inst.uuid == u,
                    (None, Some(n)) => &inst.name == n,
                    _ => false,
                };
                if hit {
                    Some((e, is_binary))
                } else {
                    None
                }
            });

            // A resident FileSystem (TOML) entity isn't persisted by the
            // binary mirror — hand it to the disk tool (the wrapper acts on
            // `routed:"filesystem"`).
            if let Some((_, false)) = target {
                return BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "routed": "filesystem" }),
                );
            }
            if let Some((entity, _)) = target {
                // RESIDENT binary — mutate components (Changed → render + persist).
                let mut ent = world.entity_mut(entity);
                if position.is_some() || size.is_some() {
                    if let Some(mut tf) = ent.get_mut::<Transform>() {
                        if let Some(p) = position {
                            tf.translation = Vec3::from_array(p);
                        }
                        if let Some(s) = size {
                            tf.scale = Vec3::from_array(s);
                        }
                    }
                }
                if let Some(mut bp) = ent.get_mut::<BasePart>() {
                    if let Some(c) = color {
                        bp.color = Color::srgb(c[0], c[1], c[2]);
                    }
                    if let Some(t) = transparency {
                        bp.transparency = t;
                    }
                    if let Some(a) = anchored {
                        bp.anchored = a;
                    }
                    if let Some(cc) = can_collide {
                        bp.can_collide = cc;
                    }
                    if let Some(ref m) = material {
                        bp.material_name = m.clone();
                    }
                }
                return BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "updated": true, "resident": true }),
                );
            }

            // NON-RESIDENT — read → mutate the DB core → mirror both stores.
            let Some(uuid_hex) = uuid else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params(
                        "entity.update: not resident; provide `uuid` to edit it in the DB",
                    ),
                );
            };
            let db = world
                .get_resource::<crate::space::world_db_plugin::WorldDbHandle>()
                .and_then(|h| h.0.clone());
            let Some(db) = db else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal("entity.update: no WorldDb"),
                );
            };
            match crate::space::world_db_binary::find_entity_by_uuid(db.as_ref(), &uuid_hex) {
                Ok(Some(mut core)) => {
                    let old_pos = core.t;
                    if let Some(p) = position {
                        core.t = p;
                    }
                    if let Some(s) = size {
                        core.s = s;
                    }
                    if let Some(c) = color {
                        core.color = [c[0], c[1], c[2], core.color[3]];
                    }
                    if let Some(t) = transparency {
                        core.transparency = t;
                    }
                    if let Some(a) = anchored {
                        core.anchored = a;
                    }
                    if let Some(cc) = can_collide {
                        core.can_collide = cc;
                    }
                    if let Some(m) = material {
                        core.material = m;
                    }
                    let encoded = match eustress_worlddb::encode_instance_core(&core) {
                        Ok(b) => b,
                        Err(e) => {
                            return BridgeResponse::error(
                                req.id.clone(),
                                BridgeError::internal(format!("entity.update: encode failed: {e}")),
                            )
                        }
                    };
                    let uuid_bytes = uuid_hex_to_bytes(&uuid_hex).unwrap_or([0u8; 16]);
                    let stored_id =
                        u64::from_be_bytes(uuid_bytes[0..8].try_into().unwrap_or([0u8; 8]));
                    crate::space::active_db::mirror_binary_core(
                        stored_id,
                        Some(&uuid_bytes),
                        old_pos,
                        core.t,
                        &encoded,
                    );
                    BridgeResponse::ok(
                        req.id.clone(),
                        serde_json::json!({ "updated": true, "resident": false }),
                    )
                }
                Ok(None) => BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params("entity.update: no entity for that uuid"),
                ),
                Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
            }
        }
    }
    /// Delete a binary-ECS entity — purges all five DB stores (no
    /// resurrection on reload) and despawns the live entity if resident.
    /// Resident: resolve uuid/name → live entity (pos from the marker's
    /// `morton_pos`, the last-persisted key). Non-resident: read the DB core
    /// by uuid for its position/class. Params: `uuid` (preferred) | `name`.
    pub fn entity_delete(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.delete: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            use crate::space::world_db_binary::BinaryEcsInstance;
            use eustress_common::classes::Instance;
            use eustress_common::instance_create::uuid_hex_to_bytes;

            let uuid = req.params.get("uuid").and_then(|v| v.as_str()).map(str::to_string);
            let name = req.params.get("name").and_then(|v| v.as_str()).map(str::to_string);
            if uuid.is_none() && name.is_none() {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params("entity.delete: provide `uuid` or `name`"),
                );
            }

            // Resident-first (note binary vs FileSystem).
            let mut q = world.query::<(Entity, &Instance, Option<&BinaryEcsInstance>)>();
            let found = q.iter(world).find_map(|(e, inst, bin)| {
                let hit = match (&uuid, &name) {
                    (Some(u), _) => &inst.uuid == u,
                    (None, Some(n)) => &inst.name == n,
                    _ => false,
                };
                if hit {
                    Some((
                        e,
                        bin.map(|b| (b.stored_id, b.morton_pos)),
                        inst.class_name.as_str().to_string(),
                        inst.uuid.clone(),
                    ))
                } else {
                    None
                }
            });

            if let Some((entity, bin_opt, class, uuid_hex)) = found {
                match bin_opt {
                    Some((stored_id, morton_pos)) => {
                        let uuid_bytes = uuid_hex_to_bytes(&uuid_hex).unwrap_or([0u8; 16]);
                        let rel = format!("Workspace/__bin_{}_{:016x}/_instance.toml", class, stored_id);
                        crate::space::active_db::delete_binary_instance(
                            stored_id,
                            &uuid_bytes,
                            &class,
                            morton_pos,
                            &rel,
                        );
                        world.despawn(entity);
                        return BridgeResponse::ok(
                            req.id.clone(),
                            serde_json::json!({ "deleted": true, "resident": true, "uuid": uuid_hex }),
                        );
                    }
                    // Resident FileSystem (TOML) entity → disk tool deletes it.
                    None => {
                        return BridgeResponse::ok(
                            req.id.clone(),
                            serde_json::json!({ "routed": "filesystem" }),
                        );
                    }
                }
            }

            // Non-resident: delete the DB core by uuid.
            let Some(uuid_hex) = uuid else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params(
                        "entity.delete: not resident; provide `uuid` to delete it from the DB",
                    ),
                );
            };
            let db = world
                .get_resource::<crate::space::world_db_plugin::WorldDbHandle>()
                .and_then(|h| h.0.clone());
            let Some(db) = db else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal("entity.delete: no WorldDb"),
                );
            };
            match crate::space::world_db_binary::find_entity_by_uuid(db.as_ref(), &uuid_hex) {
                Ok(Some(core)) => {
                    let uuid_bytes = uuid_hex_to_bytes(&uuid_hex).unwrap_or([0u8; 16]);
                    let stored_id =
                        u64::from_be_bytes(uuid_bytes[0..8].try_into().unwrap_or([0u8; 8]));
                    let rel = format!(
                        "Workspace/__bin_{}_{:016x}/_instance.toml",
                        core.class_name, stored_id
                    );
                    crate::space::active_db::delete_binary_instance(
                        stored_id,
                        &uuid_bytes,
                        &core.class_name,
                        core.t,
                        &rel,
                    );
                    BridgeResponse::ok(
                        req.id.clone(),
                        serde_json::json!({ "deleted": true, "resident": false, "uuid": uuid_hex }),
                    )
                }
                Ok(None) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "deleted": false, "reason": "not found" }),
                ),
                Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
            }
        }
    }
    /// Find entities by `uuid` | `path` | `class` via the Phase-1 identity
    /// indices (post mirror-fix these are reliable for resident AND
    /// non-resident entities). Returns uuid-handle summaries. Params:
    /// exactly one of `uuid` / `path` / `class`, optional `limit` (200).
    pub fn entity_find(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.find: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            let db = world
                .get_resource::<crate::space::world_db_plugin::WorldDbHandle>()
                .and_then(|h| h.0.clone());
            let Some(db) = db else {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal("entity.find: no WorldDb (legacy disk Space?)"),
                );
            };
            let limit = req.params.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;

            let cores: Vec<eustress_worlddb::ArchInstanceCore> =
                if let Some(u) = req.params.get("uuid").and_then(|v| v.as_str()) {
                    match crate::space::world_db_binary::find_entity_by_uuid(db.as_ref(), u) {
                        Ok(c) => c.into_iter().collect(),
                        Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
                    }
                } else if let Some(p) = req.params.get("path").and_then(|v| v.as_str()) {
                    match crate::space::world_db_binary::find_entity_by_path(db.as_ref(), p) {
                        Ok(c) => c.into_iter().collect(),
                        Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
                    }
                } else if let Some(c) = req.params.get("class").and_then(|v| v.as_str()) {
                    match crate::space::world_db_binary::find_entities_by_class(db.as_ref(), c) {
                        Ok(v) => v,
                        Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
                    }
                } else {
                    return BridgeResponse::error(
                        req.id.clone(),
                        BridgeError::invalid_params("entity.find: provide `uuid`, `path`, or `class`"),
                    );
                };

            let total = cores.len();
            let entities: Vec<Value> = cores
                .iter()
                .take(limit)
                .map(|core| {
                    let def = crate::space::arch_instance::arch_to_instance(core);
                    serde_json::json!({
                        "uuid": def.metadata.uuid,
                        "name": def.metadata.name,
                        "class": def.metadata.class_name,
                        "position": def.transform.position,
                        "mesh": def.asset.as_ref().map(|a| a.mesh.clone()),
                    })
                })
                .collect();
            return BridgeResponse::ok(
                req.id.clone(),
                serde_json::json!({
                    "entities": entities,
                    "total": total,
                    "returned": entities.len(),
                }),
            );
        }
    }
    /// Shared add/remove tag op. Resident: mutate the `Tags` component
    /// (Changed → mirror persists). Non-resident: mutate `core.tags` and
    /// re-mirror. Params: `uuid`|`name`, `tag`.
    #[cfg(feature = "world-db")]
    fn tag_op(world: &mut World, req: &BridgeRequest, add: bool) -> BridgeResponse {
        use eustress_common::classes::Instance;
        use eustress_common::instance_create::uuid_hex_to_bytes;
        use eustress_common::Tags;

        let uuid = req.params.get("uuid").and_then(|v| v.as_str()).map(str::to_string);
        let name = req.params.get("name").and_then(|v| v.as_str()).map(str::to_string);
        let Some(tag) = req.params.get("tag").and_then(|v| v.as_str()).map(str::to_string) else {
            return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params("tag op: provide `tag`"));
        };
        if uuid.is_none() && name.is_none() {
            return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params("tag op: provide `uuid` or `name`"));
        }

        let mut q = world.query::<(
            Entity,
            &Instance,
            bevy::prelude::Has<crate::space::world_db_binary::BinaryEcsInstance>,
        )>();
        let target = q.iter(world).find_map(|(e, inst, is_binary)| {
            let hit = match (&uuid, &name) {
                (Some(u), _) => &inst.uuid == u,
                (None, Some(n)) => &inst.name == n,
                _ => false,
            };
            if hit { Some((e, is_binary)) } else { None }
        });

        // Resident FileSystem (TOML) entity → disk tool handles tags.
        if let Some((_, false)) = target {
            return BridgeResponse::ok(req.id.clone(), serde_json::json!({ "routed": "filesystem" }));
        }
        if let Some((entity, _)) = target {
            let mut ent = world.entity_mut(entity);
            match ent.get_mut::<Tags>() {
                Some(mut tags) => {
                    if add {
                        if !tags.0.iter().any(|t| t == &tag) {
                            tags.0.push(tag.clone());
                        }
                    } else {
                        tags.0.retain(|t| t != &tag);
                    }
                }
                None => {
                    if add {
                        ent.insert(Tags(vec![tag.clone()]));
                    }
                }
            }
            return BridgeResponse::ok(
                req.id.clone(),
                serde_json::json!({ "ok": true, "resident": true, "add": add, "tag": tag }),
            );
        }

        // Non-resident.
        let Some(uuid_hex) = uuid else {
            return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params("tag op: not resident; provide `uuid`"));
        };
        let db = world
            .get_resource::<crate::space::world_db_plugin::WorldDbHandle>()
            .and_then(|h| h.0.clone());
        let Some(db) = db else {
            return BridgeResponse::error(req.id.clone(), BridgeError::internal("tag op: no WorldDb"));
        };
        match crate::space::world_db_binary::find_entity_by_uuid(db.as_ref(), &uuid_hex) {
            Ok(Some(mut core)) => {
                if add {
                    if !core.tags.iter().any(|t| t == &tag) {
                        core.tags.push(tag.clone());
                    }
                } else {
                    core.tags.retain(|t| t != &tag);
                }
                let encoded = match eustress_worlddb::encode_instance_core(&core) {
                    Ok(b) => b,
                    Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::internal(format!("tag op: encode failed: {e}"))),
                };
                let uuid_bytes = uuid_hex_to_bytes(&uuid_hex).unwrap_or([0u8; 16]);
                let stored_id = u64::from_be_bytes(uuid_bytes[0..8].try_into().unwrap_or([0u8; 8]));
                crate::space::active_db::mirror_binary_core(stored_id, Some(&uuid_bytes), core.t, core.t, &encoded);
                BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "ok": true, "resident": false, "add": add, "tag": tag }),
                )
            }
            Ok(None) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params("tag op: no entity for that uuid")),
            Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
        }
    }

    pub fn entity_add_tag(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(req.id.clone(), BridgeError::internal("entity.add_tag: world-db disabled"));
        }
        #[cfg(feature = "world-db")]
        {
            return tag_op(world, req, true);
        }
    }

    pub fn entity_remove_tag(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(req.id.clone(), BridgeError::internal("entity.remove_tag: world-db disabled"));
        }
        #[cfg(feature = "world-db")]
        {
            return tag_op(world, req, false);
        }
    }

    /// Read one or more simulation watchpoint values. Mirrors the
    /// `runtime-snapshot.json` payload but served live from the
    /// in-memory resource — no 250 ms staleness window.
    ///
    /// Params: `{ "keys": ["battery.voltage", ...] }` — omit `keys`
    /// to get every watchpoint.
    pub fn sim_read(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        // `SimValuesResource` (HashMap<String, f64>) is the authoritative
        // in-memory store the Rune runtime writes to on each sim tick.
        let Some(sim) = world.get_resource::<crate::simulation::plugin::SimValuesResource>() else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("SimValuesResource not available"),
            );
        };

        let filter: Option<Vec<String>> = req
            .params
            .get("keys")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let values: serde_json::Map<String, Value> = match filter {
            Some(keys) => keys
                .into_iter()
                .filter_map(|k| sim.0.get(&k).map(|v| (k, Value::from(*v))))
                .collect(),
            None => sim
                .0
                .iter()
                .map(|(k, v)| (k.clone(), Value::from(*v)))
                .collect(),
        };

        BridgeResponse::ok(req.id.clone(), Value::Object(values))
    }

    /// Return a shallow summary of entities matching a selector.
    ///
    /// Params:
    /// ```text
    /// {
    ///   "class":    "Part",      // optional — filter by class name
    ///   "offset":   0,           // optional — paginate, default 0
    ///   "limit":    10_000,      // optional — cap per-call, default 10_000, max 1_000_000
    ///   "encoding": "json"       // "json" | "bincode-base64" — default json
    /// }
    /// ```
    ///
    /// Clients that want the whole universe walk pages of `limit`
    /// until `offset + returned_count >= total`. Bincode over base64
    /// is ~5-10x smaller + faster than the JSON variant; use it for
    /// pages > 10k.
    ///
    /// Result:
    /// ```text
    /// {
    ///   "entities": [{ "id": "123v0", "name": "...", "class": "Part" }],
    ///   "offset": 0,
    ///   "total": 5_000_000,        // full matching count before pagination
    ///   "returned": 10_000,
    ///   "has_more": true,
    ///   "encoding": "json" | "bincode-base64"
    /// }
    /// ```
    pub fn ecs_query(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        // ── Parse params ─────────────────────────────────────────
        let class_filter: Option<String> = req
            .params
            .get("class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let offset = req.params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        // Hard ceiling at 1M to prevent a single call from eating memory.
        // Page if you need more; the old 1k cap is gone.
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|l| l as usize)
            .unwrap_or(10_000)
            .min(1_000_000);

        let encoding = req
            .params
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("json");

        // ── Collect matching entities in stable entity-id order ──
        //
        // We iterate once to count `total` (so clients know how many
        // pages exist) and also to slice the requested window.  Bevy's
        // query iterator is single-pass, so we materialize ids first.
        #[derive(serde::Serialize, Clone)]
        struct EntitySummary {
            id: String,
            name: String,
            class: Option<String>,
        }

        let mut q = world.query::<(Entity, &Name, Option<&eustress_common::classes::Instance>)>();
        let matches: Vec<EntitySummary> = q
            .iter(world)
            .filter_map(|(entity, name, instance)| {
                let class = instance.map(|i| i.class_name.as_str().to_string());
                if let Some(ref want) = class_filter {
                    if class.as_deref() != Some(want.as_str()) {
                        return None;
                    }
                }
                Some(EntitySummary {
                    id: format!("{}v{}", entity.index(), entity.generation()),
                    name: name.as_str().to_string(),
                    class,
                })
            })
            .collect();

        let total = matches.len();
        let window: Vec<&EntitySummary> = matches
            .iter()
            .skip(offset)
            .take(limit)
            .collect();
        let returned = window.len();
        let has_more = offset + returned < total;

        // ── Encode the window ────────────────────────────────────
        //
        // JSON is default + debuggable. `bincode-base64` is the bulk
        // path — ~6x smaller on localhost, and the base64 round-trip
        // over JSON-RPC keeps us in one transport. A dedicated binary
        // frame would shave the base64 overhead later; not worth it
        // until we profile pagination under real load.
        let entities_value = match encoding {
            "bincode-base64" => {
                let owned: Vec<EntitySummary> =
                    window.iter().map(|e| (*e).clone()).collect();
                // bincode 1.x API — `serialize` returns `Vec<u8>`.
                match bincode::serialize(&owned) {
                    Ok(bytes) => {
                        use base64::Engine as _;
                        Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
                    }
                    Err(e) => {
                        return BridgeResponse::error(
                            req.id.clone(),
                            BridgeError::internal(format!("bincode encode failed: {}", e)),
                        );
                    }
                }
            }
            _ => {
                // JSON default.
                match serde_json::to_value(&window) {
                    Ok(v) => v,
                    Err(e) => {
                        return BridgeResponse::error(
                            req.id.clone(),
                            BridgeError::internal(format!("json encode failed: {}", e)),
                        );
                    }
                }
            }
        };

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "entities": entities_value,
                "offset":   offset,
                "total":    total,
                "returned": returned,
                "has_more": has_more,
                "encoding": encoding,
            }),
        )
    }

    /// Parse `uuid` (preferred) | `name` from a promote/demote request into a
    /// `promote::EntityRef`. uuid → addressed directly (resident-resolved in the
    /// helper); name → resolved to a resident `Entity` here.
    #[cfg(feature = "world-db")]
    fn resolve_promote_target(
        world: &mut World,
        req: &BridgeRequest,
    ) -> Result<crate::space::promote::EntityRef, String> {
        use crate::space::promote::EntityRef;
        use eustress_common::classes::Instance;
        if let Some(u) = req.params.get("uuid").and_then(|v| v.as_str()) {
            return Ok(EntityRef::Uuid(u.to_string()));
        }
        if let Some(n) = req.params.get("name").and_then(|v| v.as_str()) {
            let n = n.to_string();
            let mut q = world.query::<(Entity, &Instance)>();
            return q
                .iter(world)
                .find(|(_, i)| i.name == n)
                .map(|(e, _)| EntityRef::Entity(e))
                .ok_or_else(|| format!("no resident entity named {n:?} (stream it in first)"));
        }
        Err("provide `uuid` or `name`".to_string())
    }

    /// `entity.promote` — Phase 3.5. Materialize a binary-ECS entity into an
    /// on-disk `Workspace/<Name>/_instance.toml` folder (explicit "Export to
    /// disk"). Preserves the uuid + visuals; the entity stays live. Params:
    /// `uuid` (preferred) | `name`. Errors if the target isn't resident /
    /// isn't binary-backed.
    pub fn entity_promote(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.promote: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            let target = match resolve_promote_target(world, req) {
                Ok(t) => t,
                Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
            };
            match crate::space::promote::promote_to_filesystem(world, target) {
                Ok(folder) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({
                        "promoted": true,
                        "path": folder.to_string_lossy(),
                    }),
                ),
                Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
            }
        }
    }

    /// `entity.demote` — Phase 3.5. Fold a bare, artifact-free FileSystem entity
    /// back into a binary core (deletes its disk folder). Params: `uuid` | `name`.
    pub fn entity_demote(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        #[cfg(not(feature = "world-db"))]
        {
            let _ = world;
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("entity.demote: world-db disabled"),
            );
        }
        #[cfg(feature = "world-db")]
        {
            let target = match resolve_promote_target(world, req) {
                Ok(t) => t,
                Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
            };
            match crate::space::promote::demote_to_binary(world, target) {
                Ok(()) => BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "demoted": true }),
                ),
                Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::internal(e)),
            }
        }
    }

    /// `ecs.inspect` — a DETAILED live scene snapshot for AI debugging.
    ///
    /// Unlike `ecs.query` (id/name/class), this returns the fields that
    /// actually surface bugs: the resolved mesh (the V-Cell block-render
    /// regression was a wrong `mesh`), material, color, transform, the
    /// render/physics flags, parent, and the on-disk source path. Plus
    /// top-level frame stats (`fps`) so perf regressions show too.
    ///
    /// Params (all optional): `class` (exact class filter),
    /// `name_contains` (case-insensitive substring), `offset`, `limit`
    /// (default 200, max 5000). Read-only.
    pub fn ecs_inspect(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        use bevy::prelude::{ChildOf, Transform, Visibility};

        let class_filter: Option<String> = req
            .params
            .get("class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let name_filter: Option<String> = req
            .params
            .get("name_contains")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase());
        let offset = req.params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|l| l as usize)
            .unwrap_or(200)
            .min(5_000);

        // Cloned out of the World before the query borrow so we can turn
        // absolute InstanceFile paths into Space-relative ones.
        let space_root = world
            .get_resource::<crate::space::SpaceRoot>()
            .map(|sr| sr.0.clone());

        #[derive(serde::Serialize)]
        struct TransformJson {
            pos: [f32; 3],
            rot: [f32; 4],
            scale: [f32; 3],
        }
        #[derive(serde::Serialize)]
        struct EntityDetail {
            id: String,
            name: String,
            class: Option<String>,
            /// The resolved mesh reference (engine primitive `parts/*.glb`
            /// or a custom/relative path). The field the V-Cell bug lived in.
            mesh: Option<String>,
            material: Option<String>,
            color: Option<[f32; 4]>,
            transparency: Option<f32>,
            size: Option<[f32; 3]>,
            transform: Option<TransformJson>,
            visible: Option<bool>,
            anchored: Option<bool>,
            can_collide: Option<bool>,
            cast_shadow: Option<bool>,
            locked: Option<bool>,
            parent: Option<String>,
            /// Space-relative on-disk source (`None` for a binary-ECS
            /// entity with only a synthetic path, or a synthetic path string).
            source: Option<String>,
        }

        let mut q = world.query::<(
            Entity,
            &Name,
            Option<&eustress_common::classes::Instance>,
            Option<&Transform>,
            Option<&eustress_common::classes::BasePart>,
            Option<&crate::spawn::MeshSource>,
            Option<&ChildOf>,
            Option<&crate::space::instance_loader::InstanceFile>,
            Option<&Visibility>,
        )>();

        let all: Vec<EntityDetail> = q
            .iter(world)
            .filter_map(|(entity, name, inst, tf, bp, mesh, child_of, file, vis)| {
                let class = inst.map(|i| i.class_name.as_str().to_string());
                if let Some(ref want) = class_filter {
                    if class.as_deref() != Some(want.as_str()) {
                        return None;
                    }
                }
                let name_s = name.as_str().to_string();
                if let Some(ref want) = name_filter {
                    if !name_s.to_lowercase().contains(want) {
                        return None;
                    }
                }
                let color = bp.map(|b| {
                    let c = b.color.to_srgba();
                    [c.red, c.green, c.blue, c.alpha]
                });
                let source = file.map(|f| {
                    let p = &f.toml_path;
                    match &space_root {
                        Some(root) => p
                            .strip_prefix(root)
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/")),
                        None => p.to_string_lossy().replace('\\', "/"),
                    }
                });
                Some(EntityDetail {
                    id: format!("{}v{}", entity.index(), entity.generation()),
                    name: name_s,
                    class,
                    mesh: mesh.map(|m| m.path.clone()),
                    material: bp.map(|b| b.material_name.clone()),
                    color,
                    transparency: bp.map(|b| b.transparency),
                    size: bp.map(|b| [b.size.x, b.size.y, b.size.z]),
                    transform: tf.map(|t| TransformJson {
                        pos: [t.translation.x, t.translation.y, t.translation.z],
                        rot: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                        scale: [t.scale.x, t.scale.y, t.scale.z],
                    }),
                    visible: vis.map(|v| !matches!(v, Visibility::Hidden)),
                    anchored: bp.map(|b| b.anchored),
                    can_collide: bp.map(|b| b.can_collide),
                    cast_shadow: bp.map(|b| b.cast_shadow),
                    locked: bp.map(|b| b.locked),
                    parent: child_of
                        .map(|c| c.0)
                        .map(|p| format!("{}v{}", p.index(), p.generation())),
                    source,
                })
            })
            .collect();

        let total = all.len();
        let window: Vec<&EntityDetail> = all.iter().skip(offset).take(limit).collect();
        let returned = window.len();

        // Frame stats — best-effort (None if diagnostics aren't ready).
        let fps = world
            .get_resource::<bevy::diagnostic::DiagnosticsStore>()
            .and_then(|store| {
                store
                    .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
                    .and_then(|d| d.smoothed())
            });

        // R2 draw-collapse signal: number of distinct StandardMaterial assets.
        // The old per-entity material clone made this ≈ entity_count (one bind
        // group → one draw call each — the ~60K-entity scale wall). R2.1's
        // handle-sharing collapses it to ≈ distinct appearances (≤4096 in dense
        // mode), which is what lets Bevy batch entities into a handful of
        // GPU-driven indirect draws. `material_dedup` is the registry's dedup
        // cache size; a large gap between the two would flag a sharing leak.
        let material_count = world
            .get_resource::<bevy::asset::Assets<bevy::pbr::StandardMaterial>>()
            .map(|a| a.len());
        let material_dedup = world
            .get_resource::<crate::space::material_loader::MaterialRegistry>()
            .map(|r| r.dedup_cache_len());

        let entities_value = match serde_json::to_value(&window) {
            Ok(v) => v,
            Err(e) => {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal(format!("json encode failed: {}", e)),
                );
            }
        };

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "entities":     entities_value,
                "offset":       offset,
                "total":        total,
                "returned":     returned,
                "has_more":     offset + returned < total,
                "fps":          fps,
                "entity_count": total,
                "material_count": material_count,
                "material_dedup": material_dedup,
            }),
        )
    }

    /// `tool.equip` — set the active editor tool. Param `tool`:
    /// "select" | "move" | "scale" | "rotate". Mutates the same
    /// `StudioState.current_tool` the Alt+Z/X/C/V shortcuts drive, so the
    /// gizmos + tool systems react exactly as if a human pressed the key.
    pub fn tool_equip(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let want = req
            .params
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let tool = match want.as_str() {
            "select" => crate::ui::Tool::Select,
            "move" => crate::ui::Tool::Move,
            "scale" => crate::ui::Tool::Scale,
            "rotate" => crate::ui::Tool::Rotate,
            other => {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal(format!(
                        "unknown tool '{}' (expected select|move|scale|rotate)",
                        other
                    )),
                );
            }
        };
        match world.get_resource_mut::<crate::ui::StudioState>() {
            Some(mut state) => {
                state.current_tool = tool;
                BridgeResponse::ok(req.id.clone(), serde_json::json!({ "equipped": want }))
            }
            None => BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("StudioState resource not available"),
            ),
        }
    }

    /// `selection.set` — replace the current selection. Params: `ids`
    /// (array of "indexVgeneration" id strings, as returned by
    /// `ecs.inspect`/`ecs.query`) or `id` (single). Empty clears it.
    /// Writes the shared `SelectionManager`, so gizmos + the Properties
    /// panel update downstream exactly like a click-select.
    pub fn selection_set(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let ids: Vec<String> = req
            .params
            .get("ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .or_else(|| {
                req.params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| vec![s.to_string()])
            })
            .unwrap_or_default();

        match world.get_resource::<crate::selection_sync::SelectionSyncManager>() {
            Some(mgr) => {
                mgr.0.write().set_selected(ids.clone());
                BridgeResponse::ok(
                    req.id.clone(),
                    serde_json::json!({ "selected": ids, "count": ids.len() }),
                )
            }
            None => BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("SelectionSyncManager resource not available"),
            ),
        }
    }

    /// `state.get` — live editor state (active tool + current selection)
    /// so the AI can read back the result of its own actions.
    pub fn state_get(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let current_tool = world.get_resource::<crate::ui::StudioState>().map(|s| {
            match s.current_tool {
                crate::ui::Tool::Select => "select",
                crate::ui::Tool::Move => "move",
                crate::ui::Tool::Scale => "scale",
                crate::ui::Tool::Rotate => "rotate",
                _ => "other",
            }
            .to_string()
        });
        let selected: Vec<String> = world
            .get_resource::<crate::selection_sync::SelectionSyncManager>()
            .map(|m| m.0.read().get_selected())
            .unwrap_or_default();
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "current_tool":   current_tool,
                "selected":       selected,
                "selected_count": selected.len(),
            }),
        )
    }

    /// `action.invoke` — fire any editor `Action` by its enum-variant name
    /// (param `action`, e.g. "Copy", "Cut", "Paste", "Duplicate", "Group",
    /// "Ungroup", "Delete", "SelectAll", "Undo", "Redo", "SaveScene",
    /// "MoveTool"…). `Action` derives `Deserialize`, so the variant name
    /// parses directly. Writes the SAME `MenuActionEvent` the keybinding
    /// dispatch produces, so the handler chain runs exactly as if the
    /// shortcut were pressed — the AI's "press the shortcut" surface for
    /// systematic editor testing.
    ///
    /// NOTE: some actions are destructive (Delete/Cut). This is a
    /// deliberately-driven test surface (`requires_approval` is handled at
    /// the MCP-tool layer), so the handler does not gate them here.
    pub fn action_invoke(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let name = req
            .params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let action: crate::keybindings::Action =
            match serde_json::from_value(serde_json::Value::String(name.to_string())) {
                Ok(a) => a,
                Err(_) => {
                    return BridgeResponse::error(
                        req.id.clone(),
                        BridgeError::internal(format!(
                            "unknown action '{}' — use the Action enum variant name \
                             (Copy, Cut, Paste, Duplicate, Group, Ungroup, Delete, \
                             SelectAll, Undo, Redo, SaveScene, MoveTool, ScaleTool, …)",
                            name
                        )),
                    );
                }
            };
        world.write_message(crate::ui::MenuActionEvent::new(action));
        BridgeResponse::ok(req.id.clone(), serde_json::json!({ "invoked": name }))
    }

    /// `viewport.capture` — screenshot the primary window (3D viewport +
    /// Slint overlay = what a human sees) to `<space>/.eustress/capture.png`
    /// and return the absolute path. The caller reads that path to view the
    /// frame. Mirrors `file_event_handler::capture_thumbnail_from_viewport`
    /// (Bevy 0.18 `Screenshot::primary_window()` + `ScreenshotCaptured`
    /// observer; GPU readback completes next frame, saved off-thread), but
    /// full-resolution and to a fixed path.
    ///
    /// The old file is removed up front, so reading the path before the new
    /// frame lands fails clearly (retry) rather than returning a stale image.
    pub fn viewport_capture(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(sr) => sr.0.clone(),
            None => {
                return BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::internal("SpaceRoot resource not available"),
                );
            }
        };
        let dir = space_root.join(".eustress");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("capture.png");
        // Clear the stale frame so a premature read errors instead of
        // returning the previous capture.
        let _ = std::fs::remove_file(&path);

        let save_path = path.clone();
        world
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(
                move |trigger: bevy::ecs::observer::On<
                    bevy::render::view::screenshot::ScreenshotCaptured,
                >| {
                    let img = trigger.image.clone();
                    let p = save_path.clone();
                    // Save off the render thread (GPU readback → PNG encode).
                    bevy::tasks::AsyncComputeTaskPool::get()
                        .spawn(async move {
                            match img.try_into_dynamic() {
                                Ok(dyn_img) => {
                                    if let Err(e) = dyn_img.save(&p) {
                                        tracing::warn!("viewport.capture save failed: {}", e);
                                    } else {
                                        tracing::info!("viewport.capture → {:?}", p);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("viewport.capture conversion failed: {}", e)
                                }
                            }
                        })
                        .detach();
                },
            );

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "path": path.to_string_lossy(),
                "note": "screenshot queued; PNG lands within ~1-2 frames — read the path after a brief moment",
            }),
        )
    }

    // ── AI camera — the AI's independent off-screen view ─────────────────

    /// Parse a `[x, y, z]` JSON array into a `Vec3`.
    fn parse_vec3(v: Option<&Value>) -> Option<Vec3> {
        let a = v?.as_array()?;
        if a.len() < 3 {
            return None;
        }
        Some(Vec3::new(
            a[0].as_f64()? as f32,
            a[1].as_f64()? as f32,
            a[2].as_f64()? as f32,
        ))
    }

    /// Write a pose onto the AI camera's `Transform`. Returns false if the
    /// camera entity isn't present.
    fn set_ai_camera_pose(world: &mut World, pos: Vec3, look_at: Option<Vec3>, rotation: Option<Quat>) -> bool {
        let mut q = world.query_filtered::<&mut Transform, With<crate::ai_camera::AiCamera>>();
        for mut tf in q.iter_mut(world) {
            tf.translation = pos;
            if let Some(target) = look_at {
                tf.look_at(target, Vec3::Y);
            } else if let Some(r) = rotation {
                tf.rotation = r;
            }
            return true;
        }
        false
    }

    /// `ai_camera.set_pose` — place the AI camera. Params: `position`
    /// `[x,y,z]` (required); plus either `look_at` `[x,y,z]` or `rotation`
    /// `[x,y,z,w]`. Independent of the user's camera.
    pub fn ai_camera_set_pose(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(pos) = parse_vec3(req.params.get("position")) else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params("`position` [x,y,z] is required"),
            );
        };
        let look_at = parse_vec3(req.params.get("look_at"));
        let rotation = req.params.get("rotation").and_then(|v| v.as_array()).and_then(|a| {
            if a.len() >= 4 {
                Some(Quat::from_xyzw(
                    a[0].as_f64()? as f32,
                    a[1].as_f64()? as f32,
                    a[2].as_f64()? as f32,
                    a[3].as_f64()? as f32,
                ))
            } else {
                None
            }
        });
        if !set_ai_camera_pose(world, pos, look_at, rotation) {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("AI camera entity not found"),
            );
        }
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "position": [pos.x, pos.y, pos.z],
                "look_at": look_at.map(|t| [t.x, t.y, t.z]),
            }),
        )
    }

    /// `ai_camera.orbit` — orbit the AI camera around a point. Params:
    /// `center` `[x,y,z]` (default origin), `distance` (default 15),
    /// `yaw_deg` (default 45), `pitch_deg` (default 30).
    pub fn ai_camera_orbit(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let center = parse_vec3(req.params.get("center")).unwrap_or(Vec3::ZERO);
        let distance =
            req.params.get("distance").and_then(|v| v.as_f64()).unwrap_or(15.0) as f32;
        let yaw = (req.params.get("yaw_deg").and_then(|v| v.as_f64()).unwrap_or(45.0) as f32)
            .to_radians();
        let pitch = (req.params.get("pitch_deg").and_then(|v| v.as_f64()).unwrap_or(30.0) as f32)
            .to_radians();
        let dir = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
        let pos = center + dir * distance;
        if !set_ai_camera_pose(world, pos, Some(center), None) {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("AI camera entity not found"),
            );
        }
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "position": [pos.x, pos.y, pos.z],
                "center": [center.x, center.y, center.z],
                "distance": distance,
            }),
        )
    }

    /// `ai_camera.frame` — frame a named entity in the AI camera. Param:
    /// `name` (entity Name). Looks at the entity from a distance derived from
    /// its size, so the AI can "go look at" a specific part.
    pub fn ai_camera_frame(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(name) = req.params.get("name").and_then(|v| v.as_str()).map(|s| s.to_string())
        else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params("`name` (entity name) is required"),
            );
        };
        let mut tq = world
            .query::<(&Name, &GlobalTransform, Option<&eustress_common::classes::BasePart>)>();
        let mut target: Option<(Vec3, f32)> = None;
        for (n, gt, bp) in tq.iter(world) {
            if n.as_str() == name {
                let extent = bp.map(|b| b.size.max_element()).unwrap_or(2.0);
                target = Some((gt.translation(), extent));
                break;
            }
        }
        let Some((center, extent)) = target else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params(format!("no entity named '{name}'")),
            );
        };
        let distance = (extent * 2.5).max(6.0);
        let dir = Vec3::new(1.0, 0.8, 1.0).normalize();
        let pos = center + dir * distance;
        if !set_ai_camera_pose(world, pos, Some(center), None) {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("AI camera entity not found"),
            );
        }
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "framed": name,
                "position": [pos.x, pos.y, pos.z],
                "center": [center.x, center.y, center.z],
            }),
        )
    }

    /// `ai_camera.capture` — render the AI camera's independent view to a PNG
    /// and return its path (the AI's own eyes, distinct from
    /// `viewport.capture` = the user's window). On-demand: powers the
    /// off-screen camera up for the capture, then back down.
    pub fn ai_camera_capture(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let space_root = world
            .get_resource::<crate::space::SpaceRoot>()
            .map(|sr| sr.0.clone())
            .unwrap_or_else(crate::space::default_space_root);
        let dir = space_root.join(".eustress");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("ai_camera.png");
        // Clear the stale frame so a premature read errors instead of
        // returning the previous capture.
        let _ = std::fs::remove_file(&path);
        let Some(mut state) = world.get_resource_mut::<crate::ai_camera::AiCameraState>() else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("AiCameraState not available"),
            );
        };
        crate::ai_camera::request_capture(&mut *state, path.clone());
        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "path": path.to_string_lossy(),
                "note": "AI camera capture queued; PNG lands within ~3 frames — read the path after a brief moment",
            }),
        )
    }

    // ── tools.list / tools.call — proxy ToolRegistry over the bridge ──
    //
    // These two methods collapse the Workshop/MCP split: external IDEs
    // connected via MCP no longer need their own tool implementations,
    // they call `tools/list` + `tools/call` which forward to the
    // engine's in-process `ToolRegistry`. Tool handlers run on the
    // Bevy main thread with full live ECS access, then the result
    // travels back over TCP.

    /// Return every registered tool's public metadata (name,
    /// description, JSON Schema). Sibling processes use this to
    /// advertise tools to their own clients (MCP `tools/list`, Claude
    /// Desktop's tool picker, etc.) without duplicating the schema.
    ///
    /// Result:
    /// ```text
    /// {
    ///   "tools": [
    ///     { "name": "create_entity", "description": "...", "input_schema": {...}, "requires_approval": false },
    ///     ...
    ///   ]
    /// }
    /// ```
    pub fn tools_list(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(registry) =
            world.get_resource::<crate::workshop::tools::ToolRegistry>()
        else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("ToolRegistry not available"),
            );
        };

        // `tools_for_modes` with an empty slice returns every General
        // tool; we want the whole catalogue, so pass every known mode.
        let all_modes: Vec<crate::workshop::modes::WorkshopMode> =
            crate::workshop::modes::WorkshopMode::ALL.iter().copied().collect();

        let defs = registry.tools_for_modes(&all_modes);
        let mut seen = std::collections::HashSet::new();
        let tools: Vec<Value> = defs
            .into_iter()
            .filter(|d| seen.insert(d.name)) // dedupe — same tool may appear in multiple modes
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "description": d.description,
                    "input_schema": d.input_schema,
                    "requires_approval": d.requires_approval,
                })
            })
            .collect();

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({ "tools": tools }),
        )
    }

    /// Dispatch a tool by name. Params mirror the Anthropic tool_use
    /// frame shape so Claude-compatible callers need zero translation:
    ///
    /// ```text
    /// { "name": "create_entity", "input": { ... }, "tool_use_id": "toolu_abc" }
    /// ```
    ///
    /// `tool_use_id` is optional — when absent we synthesize one so
    /// `ToolResult.tool_use_id` is always populated. `ToolContext` is
    /// built from `SpaceRoot` + `AuthState` just like the Workshop
    /// dispatcher, so the tool behaves identically whether it was
    /// invoked in-process by Claude or remotely by an external IDE.
    pub fn tools_call(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
        let Some(name) = req.params.get("name").and_then(|v| v.as_str()) else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params("missing 'name' (tool id)"),
            );
        };
        let input = req.params.get("input").cloned().unwrap_or(Value::Null);
        let use_id = req
            .params
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("bridge_{}", rand_id()));

        // Build a ToolContext from current resources. We can't take
        // `Res` here because we're already holding `&mut World` — go
        // through `world.get_resource` which returns `Option<&T>`.
        let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>() else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("SpaceRoot not loaded — no Space is active"),
            );
        };
        let space_root_path = space_root.0.clone();
        let universe_root = crate::space::universe_root_for_path(&space_root_path)
            .unwrap_or_else(|| space_root_path.clone());

        let (user_id, username) = world
            .get_resource::<crate::auth::AuthState>()
            .and_then(|a| a.user.as_ref().map(|u| (Some(u.id.clone()), Some(u.username.clone()))))
            .unwrap_or((None, None));

        let display_unit_sym = world
            .get_resource::<eustress_common::units::DisplayUnit>()
            .map(|d| d.0.symbol().to_string());

        let ctx = crate::workshop::tools::ToolContext {
            space_root: space_root_path,
            universe_root,
            user_id,
            username,
            luau_executor: None,
            display_unit: display_unit_sym,
        };

        let Some(registry) =
            world.get_resource::<crate::workshop::tools::ToolRegistry>()
        else {
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::internal("ToolRegistry not available"),
            );
        };

        let result = registry.dispatch(name, &use_id, input, &ctx);

        BridgeResponse::ok(
            req.id.clone(),
            serde_json::json!({
                "tool_name":       result.tool_name,
                "tool_use_id":     result.tool_use_id,
                "success":         result.success,
                "content":         result.content,
                "structured_data": result.structured_data,
                "stream_topic":    result.stream_topic,
            }),
        )
    }

    /// Cheap per-request id without pulling in uuid here. The bridge
    /// ID space is private to sibling-initiated calls, so a random
    /// 64-bit int collision is astronomically unlikely.
    fn rand_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{:x}", nanos)
    }
}
