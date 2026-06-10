//! # Luau Runtime
//!
//! mlua-based Luau virtual machine with sandboxing and ECS integration.
//!
//! ## Table of Contents
//!
//! 1. **LuauRuntime** — Manages the mlua Lua VM instance with Luau backend
//! 2. **LuauRuntimeState** — Bevy resource wrapping the runtime
//! 3. **ScriptExecutionQueue** — Queued script chunks awaiting execution
//! 4. **Events** — Script lifecycle events

use bevy::prelude::*;
use std::collections::HashMap;

/// Instance data extracted from the Luau VM after script execution.
/// Used to bridge Luau Instance.new() to Bevy ECS entity spawning.
///
/// Despite the `Luau` prefix, this struct is the SHARED bridge for both
/// Luau and Rune one-shot drains — both languages converge here on the
/// way to the engine spawner. Field changes affect both pipelines.
#[derive(Debug, Clone)]
pub struct LuauCreatedInstance {
    pub class_name: String,
    pub name: String,
    pub position: [f32; 3],
    /// Quaternion `[x, y, z, w]`. Identity = `[0, 0, 0, 1]`. Either
    /// drain may populate this from a `CFrame` userdata or by
    /// converting an `Orientation` Vector3 of Euler degrees to a
    /// quaternion. Without this field, scripts could not produce
    /// rotated parts — every Luau/Rune procgen scene was forced to
    /// axis-aligned cubes regardless of script intent.
    pub rotation: [f32; 4],
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: String,
    /// Part shape string — "Block", "Ball", "Cylinder", "Wedge",
    /// "CornerWedge", "Cone". Maps to the correct primitive GLB.
    pub shape: String,
    pub transparency: f32,
    pub anchored: bool,
    pub can_collide: bool,

    // BillboardGui properties (optional)
    /// StudsOffset for BillboardGui (Vector3)
    pub units_offset: Option<[f32; 3]>,
    /// Size for BillboardGui as [scale_x, offset_x, scale_y, offset_y]
    pub ui_size: Option<[f32; 4]>,
    /// Adornee name for BillboardGui (parent Part name to attach to)
    pub adornee_name: Option<String>,
    /// AlwaysOnTop flag for BillboardGui
    pub always_on_top: Option<bool>,
    /// MaxDistance for BillboardGui
    pub max_distance: Option<f32>,

    // TextLabel properties (optional)
    /// Text content for TextLabel
    pub text: Option<String>,
    /// TextColor3 for TextLabel
    pub text_color: Option<[f32; 3]>,
    /// TextSize for TextLabel
    pub text_size: Option<f32>,
    /// Font for TextLabel
    pub font: Option<String>,

    // Parent tracking
    /// Luau _entityId of this instance (for parent resolution)
    pub luau_entity_id: i64,
    /// Luau _entityId of the Parent instance (for TextLabel → BillboardGui linking)
    pub parent_entity_id: Option<i64>,

    /// CollectionService tags set on the instance via `CollectionService:AddTag`
    /// (Luau) or `CollectionService::AddTag` (Rune). Carried through the drain
    /// so the spawner persists them to `_instance.toml`'s `tags` array and
    /// attaches the ECS [`Tags`](crate::attributes::Tags) component — the
    /// same storage backing the MCP `add_tag` / `get_tagged_entities` tools.
    ///
    /// Empty when the script set no tags. Order preserved from script
    /// insertion to keep diffs deterministic (it's a set semantically, but a
    /// `Vec` for stable serialisation).
    pub tags: Vec<String>,

    /// Attributes set on the instance via `inst:SetAttribute(name, value)`
    /// while the script ran (the raw `_attr_<name>` fields on the instance
    /// table, converted to typed [`AttributeValue`](crate::attributes::AttributeValue)s).
    /// Sorted by name for deterministic output. The spawner persists these
    /// to `_instance.toml`'s `[attributes]` table / the binary core's
    /// `__attributes` cold tail, exactly like tags. Empty when the script
    /// set none (or only set values with no typed mapping).
    pub attributes: Vec<(String, crate::attributes::AttributeValue)>,
}

// Thread-local output buffer for capturing print/warn/error from Luau scripts.
// Drained after execute_chunk() to return output to the caller.
thread_local! {
    static LUAU_OUTPUT: std::cell::RefCell<Vec<(String, bool)>> = std::cell::RefCell::new(Vec::new());
}

/// Drain all captured Luau output since the last call.
/// Returns (text, is_error) pairs.
pub fn drain_luau_output() -> Vec<(String, bool)> {
    LUAU_OUTPUT.with(|buf| buf.borrow_mut().drain(..).collect())
}

// ============================================================================
// Engine ↔ VM attribute seam (GetAttribute / SetAttribute on ECS entities)
// ============================================================================
//
// The Instance arms `GetAttribute` / `SetAttribute` / `GetAttributes` operate
// on VM-local raw `_attr_<name>` fields — which is correct for script-created
// instances but blind to the engine's ECS `Attributes` components. This seam
// follows the SAME snapshot+drain architecture as CollectionService tags
// (`seed_existing_tags` → script runs → `drain_created_instances`):
//
//  * READ:  an engine system (`sync_luau_attribute_snapshot` in
//    `engine/space/instance_loader.rs`) publishes a uuid-keyed snapshot of
//    every live entity's non-empty `Attributes` component here. The
//    `GetAttribute`/`GetAttributes` arms fall back to it for any instance
//    table stamped with a `_uuid` (the importer / instance_loader stamps
//    pre-existing instances — the same field `FindByUUID` resolves).
//  * WRITE: the `SetAttribute` arm, when the receiver carries a `_uuid`,
//    pushes a typed [`EngineAttributeWrite`] into a process-global queue.
//    An engine system (`apply_luau_attribute_writes`) drains it each frame
//    and applies the values to the entity's `Attributes` component — whose
//    `Changed<Attributes>` flag then drives the EXISTING persistence seams
//    (`save_tags_and_attributes_changes` for TOML entities, the binary-ECS
//    save mirror for cores).
//
// Keyed by UUID (not entity bits) deliberately: script-created instances use
// a small VM-local `_entityId` counter that can collide with real entity
// bits, while a `_uuid` exists ONLY on engine-seeded handles — so the seam
// can never misroute a VM-local write onto an unrelated ECS entity.
//
// `Mutex` statics (not the `LUAU_OUTPUT` thread_local pattern) because seed
// and drain happen in DIFFERENT Bevy systems, which the multithreaded
// executor may run on different threads from the script execution itself.

/// One attribute write a script performed on an engine-seeded instance
/// (`value: None` = the script removed the attribute via `SetAttribute(name, nil)`).
#[derive(Debug, Clone)]
pub struct EngineAttributeWrite {
    /// The instance's stable UUID (the `_uuid` raw field / `Instance.uuid`).
    pub uuid: String,
    /// Attribute name.
    pub name: String,
    /// New typed value, or `None` to remove the attribute.
    pub value: Option<crate::attributes::AttributeValue>,
}

/// uuid → (name → value) snapshot of live ECS attributes, seeded by the
/// engine. `None` until first seeded.
static ENGINE_ATTR_SNAPSHOT: std::sync::Mutex<
    Option<HashMap<String, HashMap<String, crate::attributes::AttributeValue>>>,
> = std::sync::Mutex::new(None);

/// Pending script → engine attribute writes awaiting the engine drain.
static ENGINE_ATTR_WRITES: std::sync::Mutex<Vec<EngineAttributeWrite>> =
    std::sync::Mutex::new(Vec::new());

/// Publish (replace) the engine-side attribute snapshot. Call whenever any
/// `Attributes` component changes; cheap to call with an unchanged map.
pub fn seed_engine_attribute_snapshot(
    snapshot: HashMap<String, HashMap<String, crate::attributes::AttributeValue>>,
) {
    if let Ok(mut guard) = ENGINE_ATTR_SNAPSHOT.lock() {
        *guard = Some(snapshot);
    }
}

/// Read one attribute for `uuid` from the engine snapshot.
pub fn engine_attribute_get(uuid: &str, name: &str) -> Option<crate::attributes::AttributeValue> {
    ENGINE_ATTR_SNAPSHOT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(uuid).and_then(|a| a.get(name).cloned())))
}

/// Read ALL attributes for `uuid` from the engine snapshot (sorted by name
/// for deterministic iteration in `GetAttributes`).
pub fn engine_attributes_all(uuid: &str) -> Vec<(String, crate::attributes::AttributeValue)> {
    let mut out: Vec<(String, crate::attributes::AttributeValue)> = ENGINE_ATTR_SNAPSHOT
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .and_then(|m| m.get(uuid).map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Shadow-update the snapshot after a script-side `SetAttribute` on an
/// engine-seeded instance, so a `GetAttribute` later in the SAME run (or in
/// another fresh VM created before the engine drain) observes the write.
fn engine_attribute_shadow(uuid: &str, name: &str, value: Option<&crate::attributes::AttributeValue>) {
    if let Ok(mut guard) = ENGINE_ATTR_SNAPSHOT.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        match value {
            Some(v) => {
                map.entry(uuid.to_string())
                    .or_default()
                    .insert(name.to_string(), v.clone());
            }
            None => {
                if let Some(attrs) = map.get_mut(uuid) {
                    attrs.remove(name);
                }
            }
        }
    }
}

/// Queue a script-side attribute write for the engine to apply.
fn push_engine_attribute_write(write: EngineAttributeWrite) {
    if let Ok(mut guard) = ENGINE_ATTR_WRITES.lock() {
        guard.push(write);
    }
}

/// Drain all pending script → engine attribute writes (engine side; called
/// by `apply_luau_attribute_writes` each frame).
pub fn drain_engine_attribute_writes() -> Vec<EngineAttributeWrite> {
    ENGINE_ATTR_WRITES
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

/// [`AttributeValue`](crate::attributes::AttributeValue) → Lua value, using
/// the SAME conventions the rest of the bindings use (Vector3 → `LuauVector3`
/// userdata, Color/Color3 → `LuauColor3`, CFrame → `LuauCFrame`, UDim2 →
/// `LuauUDim2`, BrickColor → integer palette number).
///
/// Types with no Lua binding yet map as follows:
///  * `Vector2` → a plain read-only table `{X=…, Y=…}` (no LuauVector2 type
///    exists; a table read can't be silently written back as a different
///    typed attribute, which a zero-padded Vector3 could).
///  * `Object` / `EntityRef` → `nil`. The importer stores instance refs as
///    UUID *strings* (resolved via `FindByUUID`), so a live-Entity payload
///    only appears via engine-internal seeding — unresolvable from the VM.
///  * `Rect` / `Font` / `NumberRange` / `NumberSequence` / `ColorSequence`
///    → `nil` (no binding types yet).
#[cfg(feature = "luau")]
pub fn attribute_value_to_lua(
    lua: &mlua::Lua,
    value: &crate::attributes::AttributeValue,
) -> mlua::Result<mlua::Value> {
    use crate::attributes::AttributeValue as A;
    Ok(match value {
        A::Bool(b) => mlua::Value::Boolean(*b),
        // mlua's `Integer` width is build-configurable (i32 here); the
        // attribute stores i64 — saturate rather than wrap on overflow.
        A::Int(i) => mlua::Value::Integer(
            (*i).clamp(mlua::Integer::MIN as i64, mlua::Integer::MAX as i64) as mlua::Integer,
        ),
        A::Number(n) => mlua::Value::Number(*n),
        A::String(s) => mlua::Value::String(lua.create_string(s)?),
        A::Vector3(v) => mlua::Value::UserData(lua.create_userdata(
            super::types::LuauVector3::new(v.x as f64, v.y as f64, v.z as f64),
        )?),
        A::Vector2(v) => {
            let t = lua.create_table()?;
            t.set("X", v.x as f64)?;
            t.set("Y", v.y as f64)?;
            mlua::Value::Table(t)
        }
        A::Color(c) | A::Color3(c) => {
            let s = c.to_srgba();
            mlua::Value::UserData(lua.create_userdata(super::types::LuauColor3::new(
                s.red as f64,
                s.green as f64,
                s.blue as f64,
            ))?)
        }
        A::BrickColor(n) => mlua::Value::Integer(*n as mlua::Integer),
        A::CFrame(t) => mlua::Value::UserData(lua.create_userdata(super::types::LuauCFrame(
            crate::scripting::CFrame::from_transform(t),
        ))?),
        A::UDim2 { x_scale, x_offset, y_scale, y_offset } => mlua::Value::UserData(
            lua.create_userdata(super::types::LuauUDim2::new(
                *x_scale as f64,
                *x_offset as f64,
                *y_scale as f64,
                *y_offset as f64,
            ))?,
        ),
        // No Lua-side representation yet — read as nil rather than erroring,
        // matching the missing-attribute contract.
        A::Object(_) | A::EntityRef(_) => mlua::Value::Nil,
        A::Rect { .. }
        | A::Font { .. }
        | A::NumberRange { .. }
        | A::NumberSequence(_)
        | A::ColorSequence(_) => mlua::Value::Nil,
    })
}

/// Lua value → typed [`AttributeValue`](crate::attributes::AttributeValue).
///
/// `Ok(None)` for `nil` (= remove). `Err(type_name)` for values with no
/// attribute mapping (function, thread, plain table, Instance, unknown
/// userdata) — the `SetAttribute` arm surfaces that as a Lua error naming
/// the type, matching Roblox's unsupported-attribute-type error.
#[cfg(feature = "luau")]
pub fn lua_value_to_attribute(
    value: &mlua::Value,
) -> Result<Option<crate::attributes::AttributeValue>, String> {
    use crate::attributes::AttributeValue as A;
    match value {
        mlua::Value::Nil => Ok(None),
        mlua::Value::Boolean(b) => Ok(Some(A::Bool(*b))),
        mlua::Value::Integer(i) => Ok(Some(A::Int(*i as i64))),
        mlua::Value::Number(n) => Ok(Some(A::Number(*n))),
        mlua::Value::String(s) => Ok(Some(A::String(s.to_string_lossy().to_string()))),
        mlua::Value::UserData(ud) => {
            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                Ok(Some(A::Vector3(Vec3::new(
                    v.0.x as f32,
                    v.0.y as f32,
                    v.0.z as f32,
                ))))
            } else if let Ok(c) = ud.borrow::<super::types::LuauColor3>() {
                Ok(Some(A::Color3(Color::srgb(
                    c.0.r as f32,
                    c.0.g as f32,
                    c.0.b as f32,
                ))))
            } else if let Ok(cf) = ud.borrow::<super::types::LuauCFrame>() {
                Ok(Some(A::CFrame(cf.0.to_transform())))
            } else if let Ok(u) = ud.borrow::<super::types::LuauUDim2>() {
                Ok(Some(A::UDim2 {
                    x_scale: u.x_scale as f32,
                    x_offset: u.x_offset as f32,
                    y_scale: u.y_scale as f32,
                    y_offset: u.y_offset as f32,
                }))
            } else {
                Err("userdata".to_string())
            }
        }
        other => Err(other.type_name().to_string()),
    }
}

/// Convert Roblox-style Euler angles (degrees, YXZ-intrinsic per Roblox's
/// `CFrame.fromOrientation` convention) to a quaternion `[x, y, z, w]`
/// in Bevy's coordinate space.
///
/// **Y axis is negated for Roblox parity.** Despite both engines being
/// nominally right-handed Y-up, their yaw conventions empirically run
/// in opposite directions in Bevy/glam vs Roblox Studio (likely a
/// camera-forward-direction quirk between the two — Roblox treats +Z
/// as "back" while glam's standard rotation matrix takes +X to -Z on
/// positive yaw, producing visually mirrored rotation around Y from a
/// Studio viewer's perspective). Without the Y negation, the same
/// `Orientation = Vector3.new(0, 90, 0)` Luau code visibly rotates
/// parts the OPPOSITE way in Eustress vs Roblox — breaking the goal
/// of "same script → same scene." The negation closes that gap.
#[inline]
fn euler_deg_to_quat(deg_x: f64, deg_y: f64, deg_z: f64) -> [f32; 4] {
    let rx = (deg_x as f32).to_radians() * 0.5;
    let ry = (deg_y as f32).to_radians() * 0.5;
    let rz = (deg_z as f32).to_radians() * 0.5;
    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());
    let qx = cy * sx * cz + sy * cx * sz;
    let qy = -(sy * cx * cz - cy * sx * sz);
    let qz = cy * cx * sz - sy * sx * cz;
    let qw = cy * cx * cz + sy * sx * sz;
    [qx, qy, qz, qw]
}

// ============================================================================
// Luau Runtime — mlua VM wrapper
// ============================================================================

/// Luau virtual machine wrapper built on mlua.
/// Provides sandboxed execution, module caching, and Eustress API injection.
pub struct LuauRuntime {
    /// The mlua Lua instance (Luau backend)
    #[cfg(feature = "luau")]
    lua: mlua::Lua,

    /// Cached module return values (for `require()`)
    module_cache: HashMap<String, Vec<u8>>,

    /// Execution statistics
    pub stats: LuauRuntimeStats,
}

/// Runtime execution statistics
#[derive(Debug, Clone, Default)]
pub struct LuauRuntimeStats {
    /// Total chunks executed
    pub chunks_executed: u64,
    /// Successful executions
    pub successful: u64,
    /// Failed executions
    pub failed: u64,
    /// Total execution time in microseconds
    pub total_time_us: u64,
    /// Modules loaded via require()
    pub modules_loaded: u64,
}

impl LuauRuntime {
    /// Create a new Luau runtime with sandboxed globals
    #[cfg(feature = "luau")]
    pub fn new() -> Result<Self, String> {
        let lua = mlua::Lua::new();

        // Enable Luau sandboxing — restricts dangerous operations
        lua.sandbox(true).map_err(|error| format!("Failed to enable Luau sandbox: {}", error))?;

        // Inject Eustress global stubs into the VM
        Self::inject_eustress_globals(&lua)?;

        Ok(Self {
            lua,
            module_cache: HashMap::new(),
            stats: LuauRuntimeStats::default(),
        })
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn new() -> Result<Self, String> {
        Err("Luau feature is not enabled. Rebuild with --features luau".to_string())
    }

    /// Execute a chunk of Luau source code
    #[cfg(feature = "luau")]
    pub fn execute_chunk(&mut self, source: &str, chunk_name: &str) -> Result<(), String> {
        let start = std::time::Instant::now();
        self.stats.chunks_executed += 1;

        let result = self.lua.load(source)
            .set_name(chunk_name)
            .exec()
            .map_err(|error| format!("Luau execution error in '{}': {}", chunk_name, error));

        let elapsed = start.elapsed().as_micros() as u64;
        self.stats.total_time_us += elapsed;

        match &result {
            Ok(()) => self.stats.successful += 1,
            Err(_) => self.stats.failed += 1,
        }

        result
    }

    /// Drain all instances created during script execution.
    /// Returns a list of (class_name, properties) for spawning in ECS.
    #[cfg(feature = "luau")]
    pub fn drain_created_instances(&self) -> Vec<LuauCreatedInstance> {
        let mut instances = Vec::new();
        let globals = self.lua.globals();
        let Ok(registry) = globals.get::<mlua::Table>("__INSTANCE_REGISTRY__") else { return instances };

        for pair in registry.pairs::<i64, mlua::Table>() {
            let Ok((_, inst)) = pair else { continue };

            let class_name: String = inst.get("_className").unwrap_or_default();
            let name: String = inst.get("Name").unwrap_or_else(|_| class_name.clone());

            // Extract Part-specific properties
            let material: String = inst.get("Material").unwrap_or_else(|_| "Plastic".to_string());
            let shape: String = inst.get("Shape").unwrap_or_else(|_| "Block".to_string());
            let transparency: f64 = inst.get("Transparency").unwrap_or(0.0);
            let anchored: bool = inst.get("Anchored").unwrap_or(false);
            let can_collide: bool = inst.get("CanCollide").unwrap_or(true);

            // Extract position from Vector3 userdata or default
            let position = inst.get::<mlua::AnyUserData>("Position").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauVector3>().ok().map(|v| [v.0.x as f32, v.0.y as f32, v.0.z as f32]))
                .unwrap_or([0.0, 0.0, 0.0]);

            let size = inst.get::<mlua::AnyUserData>("Size").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauVector3>().ok().map(|v| [v.0.x as f32, v.0.y as f32, v.0.z as f32]))
                .unwrap_or([4.0, 1.0, 2.0]);

            let color = inst.get::<mlua::AnyUserData>("Color").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauColor3>().ok().map(|c| [c.0.r as f32, c.0.g as f32, c.0.b as f32, 1.0]))
                .unwrap_or([0.639, 0.635, 0.647, 1.0]);

            // Rotation — read from `Orientation` Vector3 (Euler degrees)
            // since that's the Roblox-Part convention and is already
            // populated by `apply_class_defaults` for every spawned Part.
            // Identity quaternion when absent.
            let rotation = inst.get::<mlua::AnyUserData>("Orientation").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauVector3>().ok()
                    .map(|v| euler_deg_to_quat(v.0.x, v.0.y, v.0.z)))
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);

            // BillboardGui properties
            let units_offset = inst.get::<mlua::AnyUserData>("StudsOffset").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauVector3>().ok()
                    .map(|v| [v.0.x as f32, v.0.y as f32, v.0.z as f32]));

            let ui_size = inst.get::<mlua::AnyUserData>("Size").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauUDim2>().ok()
                    .map(|s| [s.x_scale as f32, s.x_offset as f32, s.y_scale as f32, s.y_offset as f32]));

            let adornee_name = inst.get::<mlua::Value>("Adornee").ok()
                .and_then(|v| match v {
                    mlua::Value::Table(t) => t.get::<String>("Name").ok(),
                    _ => None,
                });

            let always_on_top = inst.get("AlwaysOnTop").ok();
            let max_distance = inst.get::<f64>("MaxDistance").ok().map(|v| v as f32);

            // TextLabel properties
            let text = inst.get::<String>("Text").ok();
            let text_color = inst.get::<mlua::AnyUserData>("TextColor3").ok()
                .and_then(|ud| ud.borrow::<super::types::LuauColor3>().ok()
                    .map(|c| [c.0.r as f32, c.0.g as f32, c.0.b as f32]));
            let text_size = inst.get::<f64>("TextSize").ok().map(|v| v as f32);
            let font = inst.get::<String>("Font").ok();

            let luau_entity_id: i64 = inst.get("_entityId").unwrap_or(0);
            let parent_entity_id = inst.get::<mlua::Value>("Parent").ok()
                .and_then(|v| match v {
                    mlua::Value::Table(t) => t.get::<i64>("_entityId").ok(),
                    _ => None,
                });

            // CollectionService:AddTag stores tags on `inst._tags` as a
            // set-style table (`_tags["mindmap-node"] = true`). Collect
            // them into a deterministic Vec — the spawner persists this
            // to `_instance.toml`'s `tags = [...]` array, which Bevy then
            // hydrates into the ECS `Tags` component on load.
            let tags: Vec<String> = inst.get::<Option<mlua::Table>>("_tags")
                .ok()
                .flatten()
                .map(|t| {
                    let mut v: Vec<String> = t.pairs::<String, bool>()
                        .filter_map(|p| p.ok())
                        .filter(|(_, on)| *on)
                        .map(|(k, _)| k)
                        .collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();

            // SetAttribute stores raw `_attr_<name>` fields on the instance
            // table; convert each to a typed AttributeValue so the spawner
            // can persist them ([attributes] TOML table / binary cold tail)
            // alongside tags. Values with no typed mapping are skipped
            // (SetAttribute already rejects unsupported types, so in
            // practice everything here converts). Sorted for determinism.
            let attributes: Vec<(String, crate::attributes::AttributeValue)> = {
                let mut list: Vec<(String, crate::attributes::AttributeValue)> = inst
                    .pairs::<String, mlua::Value>()
                    .filter_map(|p| p.ok())
                    .filter_map(|(k, v)| {
                        let name = k.strip_prefix("_attr_")?.to_string();
                        match lua_value_to_attribute(&v) {
                            Ok(Some(av)) => Some((name, av)),
                            _ => None,
                        }
                    })
                    .collect();
                list.sort_by(|a, b| a.0.cmp(&b.0));
                list
            };

            instances.push(LuauCreatedInstance {
                class_name,
                name,
                position,
                rotation,
                size,
                color,
                material,
                shape,
                transparency: transparency as f32,
                anchored,
                can_collide,
                units_offset,
                ui_size,
                adornee_name,
                always_on_top,
                max_distance,
                text,
                text_color,
                text_size,
                font,
                luau_entity_id,
                parent_entity_id,
                tags,
                attributes,
            });
        }

        instances
    }

    /// Seed `__EXISTING_TAGS__` with a snapshot of live ECS tags so a script's
    /// `CollectionService:GetTagged(tag)` call returns engine-side entity ids
    /// in addition to script-created instances. Call before each script run;
    /// the snapshot lives until [`clear_existing_tags`](Self::clear_existing_tags)
    /// (or next seed call).
    ///
    /// `snapshot`: `tag -> [entity_id_1, entity_id_2, ...]`. Entity ids are
    /// the same i64 values returned by `Instance.entity_id` in Luau, the
    /// engine's ECS Entity (cast to i64), or MCP's `entity_id` field.
    #[cfg(feature = "luau")]
    pub fn seed_existing_tags(&self, snapshot: &HashMap<String, Vec<i64>>) -> Result<(), String> {
        let globals = self.lua.globals();
        let tbl = self.lua.create_table()
            .map_err(|e| format!("Failed to create existing tags table: {}", e))?;
        for (tag, ids) in snapshot {
            let arr = self.lua.create_table()
                .map_err(|e| format!("Failed to create tag id array: {}", e))?;
            for (i, id) in ids.iter().enumerate() {
                arr.set((i + 1) as i64, *id)
                    .map_err(|e| format!("Failed to set tag id: {}", e))?;
            }
            tbl.set(tag.as_str(), arr)
                .map_err(|e| format!("Failed to set tag entry: {}", e))?;
        }
        globals.set("__EXISTING_TAGS__", tbl)
            .map_err(|e| format!("Failed to set __EXISTING_TAGS__: {}", e))?;
        Ok(())
    }

    /// Clear the engine-supplied tag snapshot. Defensive — the next seed
    /// call replaces it anyway, but useful at execution boundaries to
    /// avoid scripts seeing stale data.
    #[cfg(feature = "luau")]
    pub fn clear_existing_tags(&self) -> Result<(), String> {
        let globals = self.lua.globals();
        let empty = self.lua.create_table()
            .map_err(|e| format!("Failed to create empty tags table: {}", e))?;
        globals.set("__EXISTING_TAGS__", empty)
            .map_err(|e| format!("Failed to clear __EXISTING_TAGS__: {}", e))?;
        Ok(())
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn drain_created_instances(&self) -> Vec<LuauCreatedInstance> { Vec::new() }

    /// Fallback seed when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn seed_existing_tags(&self, _snapshot: &HashMap<String, Vec<i64>>) -> Result<(), String> {
        Ok(())
    }

    /// Fallback clear when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn clear_existing_tags(&self) -> Result<(), String> { Ok(()) }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn execute_chunk(&mut self, _source: &str, _chunk_name: &str) -> Result<(), String> {
        Err("Luau feature is not enabled".to_string())
    }

    /// Load a ModuleScript and cache its return value in the Lua registry.
    /// The module's return value is stored as a registry key for `require()` resolution.
    #[cfg(feature = "luau")]
    pub fn load_module(&mut self, name: &str, source: &str) -> Result<(), String> {
        // Execute the module chunk — it should return exactly one value
        let value = self.lua.load(source)
            .set_name(name)
            .eval::<mlua::Value>()
            .map_err(|error| format!("Module '{}' failed to load: {}", name, error))?;

        // Store the return value in the Lua registry keyed by module name.
        // This allows `require()` to retrieve it without re-execution.
        let registry_key = self.lua.create_registry_value(value)
            .map_err(|error| format!("Module '{}' registry store failed: {}", name, error))?;

        // Serialize the registry key index for our cache tracking
        let key_bytes = format!("{:?}", registry_key).into_bytes();
        self.module_cache.insert(name.to_string(), key_bytes);
        self.stats.modules_loaded += 1;

        Ok(())
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn load_module(&mut self, _name: &str, _source: &str) -> Result<(), String> {
        Err("Luau feature is not enabled".to_string())
    }

    /// Check if a module is cached
    pub fn is_module_cached(&self, name: &str) -> bool {
        self.module_cache.contains_key(name)
    }

    /// Clear the module cache (forces re-require on next access)
    pub fn clear_module_cache(&mut self) {
        self.module_cache.clear();
    }

    /// Inject Eustress-specific globals into the Luau VM.
    /// These provide the Roblox-compatible API surface:
    /// - `game` — service hierarchy root
    /// - `workspace` — alias for game.Workspace
    /// - `script` — reference to the currently executing script
    /// - `print` / `warn` / `error` — output to Eustress console
    /// - `wait` / `task` — coroutine scheduling
    /// - `Instance.new()` — entity creation
    /// - `Vector3`, `CFrame`, `Color3` — data types from shared scripting module
    #[cfg(feature = "luau")]
    fn inject_eustress_globals(lua: &mlua::Lua) -> Result<(), String> {
        // Inject shared scripting types (Vector3, CFrame, Color3)
        super::types::inject_types(lua)
            .map_err(|e| format!("Failed to inject scripting types: {}", e))?;

        // Inject each API subsystem via separate functions to avoid compiler stack overflow
        Self::inject_core_globals(lua)?;
        Self::inject_instance_api(lua)?;
        Self::inject_task_library(lua)?;
        Self::inject_run_service(lua)?;
        Self::inject_user_input_service(lua)?;
        Self::inject_players_service(lua)?;
        Self::inject_storage_services(lua)?;
        Self::inject_tween_service(lua)?;
        Self::inject_data_services(lua)?;
        Self::inject_http_service(lua)?;
        Self::inject_collection_service(lua)?;
        Self::inject_sound_service(lua)?;
        Self::inject_camera_api(lua)?;
        Self::inject_mouse_api(lua)?;
        Self::inject_animation_api(lua)?;
        Self::inject_humanoid_api(lua)?;
        Self::inject_marketplace_service(lua)?;
        Self::inject_simulation_service(lua)?;
        Self::inject_workspace_query(lua)?;
        Self::inject_spatial_queries(lua)?;
        #[cfg(feature = "gui")]
        Self::inject_gui_api(lua)?;
        Self::inject_event_system(lua)?;

        Ok(())
    }

    // ========================================================================
    // Signal helper — creates a Roblox-compatible RBXScriptSignal table
    // ========================================================================
    #[cfg(feature = "luau")]
    fn create_signal(lua: &mlua::Lua) -> Result<mlua::Table, String> {
        let signal = lua.create_table()
            .map_err(|e| format!("Failed to create signal: {}", e))?;

        let connections = lua.create_table()
            .map_err(|e| format!("Failed to create signal connections: {}", e))?;
        signal.set("_connections", connections)
            .map_err(|e| format!("Failed to set _connections: {}", e))?;
        signal.set("_nextId", 1i64)
            .map_err(|e| format!("Failed to set _nextId: {}", e))?;

        // Signal:Connect(callback) -> Connection
        let connect_fn = lua.create_function(|lua, (this, callback): (mlua::Table, mlua::Function)| {
            let connections: mlua::Table = this.get("_connections")?;
            let next_id: i64 = this.get("_nextId")?;
            this.set("_nextId", next_id + 1)?;
            connections.set(next_id, callback)?;

            let connection = lua.create_table()?;
            connection.set("_id", next_id)?;
            connection.set("_signal", this.clone())?;
            connection.set("Connected", true)?;
            connection.set("Disconnect", lua.create_function(|_, conn: mlua::Table| {
                let id: i64 = conn.get("_id")?;
                let signal: mlua::Table = conn.get("_signal")?;
                let conns: mlua::Table = signal.get("_connections")?;
                conns.set(id, mlua::Value::Nil)?;
                conn.set("Connected", false)?;
                Ok(())
            })?)?;
            Ok(connection)
        }).map_err(|e| format!("Failed to create Connect: {}", e))?;
        signal.set("Connect", connect_fn)
            .map_err(|e| format!("Failed to set Connect: {}", e))?;

        // Signal:Once(callback) -> Connection (auto-disconnect after first fire)
        let once_fn = lua.create_function(|lua, (this, callback): (mlua::Table, mlua::Function)| {
            let connections: mlua::Table = this.get("_connections")?;
            let next_id: i64 = this.get("_nextId")?;
            this.set("_nextId", next_id + 1)?;
            let wrapped = lua.create_function(move |_lua, args: mlua::MultiValue| {
                callback.call::<mlua::MultiValue>(args.clone())
            })?;
            connections.set(next_id, wrapped)?;

            let connection = lua.create_table()?;
            connection.set("_id", next_id)?;
            connection.set("_signal", this.clone())?;
            connection.set("Connected", true)?;
            connection.set("Disconnect", lua.create_function(|_, conn: mlua::Table| {
                let id: i64 = conn.get("_id")?;
                let signal: mlua::Table = conn.get("_signal")?;
                let conns: mlua::Table = signal.get("_connections")?;
                conns.set(id, mlua::Value::Nil)?;
                conn.set("Connected", false)?;
                Ok(())
            })?)?;
            Ok(connection)
        }).map_err(|e| format!("Failed to create Once: {}", e))?;
        signal.set("Once", once_fn)
            .map_err(|e| format!("Failed to set Once: {}", e))?;

        // Signal:Wait() -> returns when signal fires (stub — returns immediately)
        let wait_fn = lua.create_function(|_, _this: mlua::Table| {
            // TODO: Integrate with coroutine scheduler to actually yield
            Ok(0.0f64)
        }).map_err(|e| format!("Failed to create Wait: {}", e))?;
        signal.set("Wait", wait_fn)
            .map_err(|e| format!("Failed to set Wait: {}", e))?;

        // Signal:Fire(...) — fire all connected callbacks with given arguments
        let fire_fn = lua.create_function(|_, (this, args): (mlua::Table, mlua::MultiValue)| {
            let connections: mlua::Table = this.get("_connections")?;
            for pair in connections.pairs::<i64, mlua::Function>() {
                if let Ok((_, callback)) = pair {
                    let _ = callback.call::<()>(args.clone());
                }
            }
            Ok(())
        }).map_err(|e| format!("Failed to create Fire: {}", e))?;
        signal.set("Fire", fire_fn)
            .map_err(|e| format!("Failed to set Fire: {}", e))?;

        Ok(signal)
    }

    // ========================================================================
    // Core globals: print, warn, game, workspace
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_core_globals(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // Override print to route to Eustress output log + capture buffer
        let print_function = lua.create_function(|_, args: mlua::MultiValue| {
            let output: Vec<String> = args.iter().map(|value| format!("{}", value.to_string().unwrap_or_default())).collect();
            let line = output.join("\t");
            tracing::info!("[Luau] {}", line);
            LUAU_OUTPUT.with(|buf| buf.borrow_mut().push((line, false)));
            Ok(())
        }).map_err(|error| format!("Failed to create print function: {}", error))?;
        globals.set("print", print_function)
            .map_err(|error| format!("Failed to set print: {}", error))?;

        // Override warn to route to Eustress warning log + capture buffer
        let warn_function = lua.create_function(|_, args: mlua::MultiValue| {
            let output: Vec<String> = args.iter().map(|value| format!("{}", value.to_string().unwrap_or_default())).collect();
            let line = output.join("\t");
            tracing::warn!("[Luau] {}", line);
            LUAU_OUTPUT.with(|buf| buf.borrow_mut().push((format!("⚠ {}", line), false)));
            Ok(())
        }).map_err(|error| format!("Failed to create warn function: {}", error))?;
        globals.set("warn", warn_function)
            .map_err(|error| format!("Failed to set warn: {}", error))?;

        // typeof() — Roblox type checking
        let typeof_fn = lua.create_function(|_, value: mlua::Value| {
            let type_name = match &value {
                mlua::Value::Nil => "nil",
                mlua::Value::Boolean(_) => "boolean",
                mlua::Value::Integer(_) => "number",
                mlua::Value::Number(_) => "number",
                mlua::Value::String(_) => "string",
                mlua::Value::Table(t) => {
                    // Check for known class types
                    if let Ok(class) = t.raw_get::<String>("_className") {
                        return Ok(class);
                    }
                    "table"
                }
                mlua::Value::Function(_) => "function",
                mlua::Value::Thread(_) => "thread",
                mlua::Value::UserData(ud) => {
                    if ud.is::<super::types::LuauVector3>() { return Ok("Vector3".to_string()); }
                    if ud.is::<super::types::LuauCFrame>() { return Ok("CFrame".to_string()); }
                    if ud.is::<super::types::LuauColor3>() { return Ok("Color3".to_string()); }
                    if ud.is::<super::types::LuauUDim2>() { return Ok("UDim2".to_string()); }
                    if ud.is::<super::types::LuauTweenInfo>() { return Ok("TweenInfo".to_string()); }
                    "userdata"
                }
                _ => "userdata",
            };
            Ok(type_name.to_string())
        }).map_err(|error| format!("Failed to create typeof: {}", error))?;
        globals.set("typeof", typeof_fn)
            .map_err(|error| format!("Failed to set typeof: {}", error))?;

        // Stub `game` as an empty table (populated per-script by bridge)
        let game_table = lua.create_table()
            .map_err(|error| format!("Failed to create game table: {}", error))?;
        globals.set("game", game_table)
            .map_err(|error| format!("Failed to set game: {}", error))?;

        // workspace table with Gravity property
        let workspace_table = lua.create_table()
            .map_err(|error| format!("Failed to create workspace table: {}", error))?;
        workspace_table.set("Gravity", 9.80665f64)
            .map_err(|error| format!("Failed to set workspace.Gravity: {}", error))?;
        globals.set("workspace", workspace_table)
            .map_err(|error| format!("Failed to set workspace: {}", error))?;

        // Units namespace — script-facing helpers for explicit unit
        // conversion. All numeric values seen by scripts (positions,
        // sizes, raycast distances, gravity, …) are in engine-native
        // meters; this namespace is the escape hatch for displaying or
        // parsing values in another unit (e.g. drawing a "5.0 ft"
        // label on a part whose Position is 1.524 m).
        //
        //   Units.from_meters(value, symbol)   meters → unit
        //   Units.to_meters(value, symbol)     unit  → meters
        //
        // Unknown symbols return the input value unchanged plus a
        // second `false` return so scripts can validate without raising.
        let units_table = lua.create_table()
            .map_err(|error| format!("Failed to create Units table: {}", error))?;

        let from_meters = lua.create_function(|_, (value, symbol): (f64, String)| {
            match crate::units::Unit::from_any(&symbol) {
                Some(u) => Ok((crate::units::convert(value, crate::units::ENGINE_NATIVE_UNIT, u), true)),
                None => Ok((value, false)),
            }
        }).map_err(|e| format!("Failed to create Units.from_meters: {}", e))?;
        units_table.set("from_meters", from_meters)
            .map_err(|e| format!("Failed to set Units.from_meters: {}", e))?;

        let to_meters = lua.create_function(|_, (value, symbol): (f64, String)| {
            match crate::units::Unit::from_any(&symbol) {
                Some(u) => Ok((crate::units::convert(value, u, crate::units::ENGINE_NATIVE_UNIT), true)),
                None => Ok((value, false)),
            }
        }).map_err(|e| format!("Failed to create Units.to_meters: {}", e))?;
        units_table.set("to_meters", to_meters)
            .map_err(|e| format!("Failed to set Units.to_meters: {}", e))?;

        globals.set("Units", units_table)
            .map_err(|e| format!("Failed to set Units: {}", e))?;

        // tick() — Roblox-compatible time function (seconds since Unix epoch)
        let tick_fn = lua.create_function(|_, ()| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            Ok(now.as_secs_f64())
        }).map_err(|error| format!("Failed to create tick: {}", error))?;
        globals.set("tick", tick_fn)
            .map_err(|error| format!("Failed to set tick: {}", error))?;

        // Enum — Roblox-style enum namespace injected as pure Lua so we
        // avoid mlua metatable API differences across versions. Unknown
        // Enum.Foo.Bar accesses return a string token "Foo.Bar" so scripts
        // that use Enum values for comparison or assignment don't error out.
        lua.load(r#"
Enum = setmetatable({}, {
    __index = function(self, category)
        local sub = rawget(self, category)
        if sub then return sub end
        sub = setmetatable({}, {
            __index = function(_, key)
                return category .. "." .. key
            end
        })
        rawset(self, category, sub)
        return sub
    end
})
"#).exec().map_err(|e| format!("Failed to inject Enum: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Instance API: Instance.new, Clone, Destroy, FindFirstChild, etc.
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_instance_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P0: Instance API — Core entity creation and manipulation
        // ====================================================================
        
        // Global instance registry (entity_id -> instance table)
        let instance_registry = lua.create_table()
            .map_err(|e| format!("Failed to create instance registry: {}", e))?;
        globals.set("__INSTANCE_REGISTRY__", instance_registry)
            .map_err(|e| format!("Failed to set instance registry: {}", e))?;
        
        // Next entity ID counter
        globals.set("__NEXT_ENTITY_ID__", 1i64)
            .map_err(|e| format!("Failed to set entity ID counter: {}", e))?;

        // Instance table with constructor
        let instance_table = lua.create_table()
            .map_err(|e| format!("Failed to create Instance table: {}", e))?;

        // Instance.new(className, parent?) -> Instance
        let instance_new = lua.create_function(|lua, (class_name, parent): (String, Option<mlua::Table>)| {
            let globals = lua.globals();
            
            // Get next entity ID
            let entity_id: i64 = globals.get("__NEXT_ENTITY_ID__")?;
            globals.set("__NEXT_ENTITY_ID__", entity_id + 1)?;
            
            // Create instance table
            let instance = lua.create_table()?;
            instance.set("_entityId", entity_id)?;
            instance.set("_className", class_name.clone())?;
            instance.set("Name", class_name.clone())?;
            instance.set("ClassName", class_name.clone())?;
            instance.set("Parent", mlua::Value::Nil)?;
            instance.set("Archivable", true)?;
            
            // Children storage
            let children = lua.create_table()?;
            instance.set("_children", children)?;
            
            // Properties storage
            let properties = lua.create_table()?;
            instance.set("_properties", properties)?;
            
            // Add class-specific default properties
            match class_name.as_str() {
                "Part" | "MeshPart" | "WedgePart" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauVector3::new(4.0, 1.0, 2.0))?)?;
                    instance.set("CFrame", lua.create_userdata(super::types::LuauCFrame::identity())?)?;
                    instance.set("Anchored", false)?;
                    instance.set("CanCollide", true)?;
                    instance.set("Transparency", 0.0f64)?;
                    instance.set("Color", lua.create_userdata(super::types::LuauColor3::new(0.639, 0.635, 0.647))?)?;
                    instance.set("Material", "Plastic")?;
                }
                "Model" => {
                    instance.set("PrimaryPart", mlua::Value::Nil)?;
                }
                "Script" | "LocalScript" => {
                    instance.set("Source", "")?;
                    instance.set("Enabled", true)?;
                }
                "ModuleScript" => {
                    instance.set("Source", "")?;
                }
                "Humanoid" => {
                    instance.set("Health", 100.0f64)?;
                    instance.set("MaxHealth", 100.0f64)?;
                    instance.set("WalkSpeed", 16.0f64)?;
                    instance.set("JumpPower", 50.0f64)?;
                    instance.set("JumpHeight", 7.2f64)?;
                }
                "Animation" => {
                    instance.set("AnimationId", "")?;
                }
                "Sound" => {
                    instance.set("SoundId", "")?;
                    instance.set("Volume", 1.0f64)?;
                    instance.set("Playing", false)?;
                    instance.set("Looped", false)?;
                }
                "ClickDetector" => {
                    instance.set("MaxActivationDistance", 32.0f64)?;
                }
                "ScreenGui" => {
                    instance.set("Enabled", true)?;
                    instance.set("DisplayOrder", 0i64)?;
                    instance.set("IgnoreGuiInset", false)?;
                    instance.set("ResetOnSpawn", true)?;
                    instance.set("ZIndexBehavior", "Sibling")?;
                }
                "SurfaceGui" => {
                    instance.set("Enabled", true)?;
                    instance.set("Face", "Front")?;
                    instance.set("Active", true)?;
                    instance.set("Adornee", mlua::Value::Nil)?;
                    instance.set("AlwaysOnTop", false)?;
                    instance.set("LightInfluence", 0.0f64)?;
                    instance.set("SizingMode", "FixedSize")?;
                    instance.set("CanvasSize", lua.create_userdata(super::types::LuauVector3::new(800.0, 600.0, 0.0))?)?;
                }
                "BillboardGui" => {
                    instance.set("Enabled", true)?;
                    instance.set("Active", true)?;
                    instance.set("Adornee", mlua::Value::Nil)?;
                    instance.set("AlwaysOnTop", false)?;
                    instance.set("LightInfluence", 0.0f64)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 200.0, 0.0, 50.0))?)?;
                    instance.set("StudsOffset", lua.create_userdata(super::types::LuauVector3::new(0.0, 2.0, 0.0))?)?;
                    instance.set("MaxDistance", 100.0f64)?;
                }
                "Frame" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 100.0, 0.0, 100.0))?)?;
                    instance.set("AnchorPoint", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("BorderColor3", lua.create_userdata(super::types::LuauColor3::new(0.105, 0.164, 0.207))?)?;
                    instance.set("BorderSizePixel", 1i64)?;
                    instance.set("ClipsDescendants", false)?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                    instance.set("Rotation", 0.0f64)?;
                }
                "TextLabel" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 200.0, 0.0, 50.0))?)?;
                    instance.set("AnchorPoint", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("BorderSizePixel", 1i64)?;
                    instance.set("Text", "")?;
                    instance.set("TextColor3", lua.create_userdata(super::types::LuauColor3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("TextSize", 14.0f64)?;
                    instance.set("Font", "SourceSans")?;
                    instance.set("TextWrapped", false)?;
                    instance.set("TextScaled", false)?;
                    instance.set("TextXAlignment", "Center")?;
                    instance.set("TextYAlignment", "Center")?;
                    instance.set("TextTransparency", 0.0f64)?;
                    instance.set("TextTruncate", "None")?;
                    instance.set("RichText", false)?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "TextButton" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 200.0, 0.0, 50.0))?)?;
                    instance.set("AnchorPoint", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("BorderSizePixel", 1i64)?;
                    instance.set("Text", "Button")?;
                    instance.set("TextColor3", lua.create_userdata(super::types::LuauColor3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("TextSize", 14.0f64)?;
                    instance.set("Font", "SourceSans")?;
                    instance.set("AutoButtonColor", true)?;
                    instance.set("Active", true)?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "TextBox" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 200.0, 0.0, 50.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("Text", "")?;
                    instance.set("PlaceholderText", "")?;
                    instance.set("PlaceholderColor3", lua.create_userdata(super::types::LuauColor3::new(0.7, 0.7, 0.7))?)?;
                    instance.set("TextColor3", lua.create_userdata(super::types::LuauColor3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("TextSize", 14.0f64)?;
                    instance.set("Font", "SourceSans")?;
                    instance.set("ClearTextOnFocus", true)?;
                    instance.set("MultiLine", false)?;
                    instance.set("TextEditable", true)?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "ImageLabel" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 100.0, 0.0, 100.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("Image", "")?;
                    instance.set("ImageColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("ImageTransparency", 0.0f64)?;
                    instance.set("ScaleType", "Stretch")?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "ImageButton" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 100.0, 0.0, 100.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("Image", "")?;
                    instance.set("ImageColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("ImageTransparency", 0.0f64)?;
                    instance.set("AutoButtonColor", true)?;
                    instance.set("Active", true)?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "ScrollingFrame" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 200.0, 0.0, 200.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                    instance.set("CanvasSize", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 2.0, 0.0))?)?;
                    instance.set("CanvasPosition", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("ScrollBarThickness", 12i64)?;
                    instance.set("ScrollBarImageColor3", lua.create_userdata(super::types::LuauColor3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("ScrollingDirection", "XY")?;
                    instance.set("ScrollingEnabled", true)?;
                    instance.set("ElasticBehavior", "WhenScrollable")?;
                    instance.set("LayoutOrder", 0i64)?;
                    instance.set("ZIndex", 1i64)?;
                }
                "UIListLayout" => {
                    instance.set("FillDirection", "Vertical")?;
                    instance.set("HorizontalAlignment", "Left")?;
                    instance.set("VerticalAlignment", "Top")?;
                    instance.set("SortOrder", "LayoutOrder")?;
                    instance.set("Padding", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Wraps", false)?;
                }
                "UIGridLayout" => {
                    instance.set("CellSize", lua.create_userdata(super::types::LuauUDim2::new(0.0, 100.0, 0.0, 100.0))?)?;
                    instance.set("CellPadding", lua.create_userdata(super::types::LuauUDim2::new(0.0, 5.0, 0.0, 5.0))?)?;
                    instance.set("FillDirection", "Horizontal")?;
                    instance.set("FillDirectionMaxCells", 0i64)?;
                    instance.set("HorizontalAlignment", "Left")?;
                    instance.set("VerticalAlignment", "Top")?;
                    instance.set("SortOrder", "LayoutOrder")?;
                }
                "UIPadding" => {
                    instance.set("PaddingTop", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("PaddingBottom", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("PaddingLeft", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("PaddingRight", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                }
                "UICorner" => {
                    instance.set("CornerRadius", lua.create_userdata(super::types::LuauUDim2::new(0.0, 8.0, 0.0, 0.0))?)?;
                }
                "UIStroke" => {
                    instance.set("Color", lua.create_userdata(super::types::LuauColor3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Thickness", 1.0f64)?;
                    instance.set("Transparency", 0.0f64)?;
                    instance.set("ApplyStrokeMode", "Contextual")?;
                    instance.set("LineJoinMode", "Round")?;
                }
                "UIAspectRatioConstraint" => {
                    instance.set("AspectRatio", 1.0f64)?;
                    instance.set("AspectType", "FitWithinMaxSize")?;
                    instance.set("DominantAxis", "Width")?;
                }
                "UISizeConstraint" => {
                    instance.set("MaxSize", lua.create_userdata(super::types::LuauVector3::new(f64::MAX, f64::MAX, 0.0))?)?;
                    instance.set("MinSize", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                }
                "UITextSizeConstraint" => {
                    instance.set("MaxTextSize", 100i64)?;
                    instance.set("MinTextSize", 1i64)?;
                }
                _ => {}
            }
            
            // Register instance
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            registry.set(entity_id, instance.clone())?;
            
            // Set parent if provided
            if let Some(parent_table) = parent {
                instance.set("Parent", parent_table.clone())?;
                let parent_children: mlua::Table = parent_table.get("_children")?;
                parent_children.set(entity_id, instance.clone())?;
            }
            
            Ok(instance)
        }).map_err(|e| format!("Failed to create Instance.new: {}", e))?;
        instance_table.set("new", instance_new)
            .map_err(|e| format!("Failed to set Instance.new: {}", e))?;

        globals.set("Instance", instance_table)
            .map_err(|e| format!("Failed to set Instance: {}", e))?;

        // ====================================================================
        // Instance methods (added to each instance via metatable)
        // ====================================================================
        
        // Create instance metatable with methods
        let instance_mt = lua.create_table()
            .map_err(|e| format!("Failed to create instance metatable: {}", e))?;
        
        // __index metamethod for method lookup
        let instance_index = lua.create_function(|lua, (this, key): (mlua::Table, String)| {
            // First check if it's a direct property
            let raw_value: mlua::Value = this.raw_get(key.clone())?;
            if raw_value != mlua::Value::Nil {
                return Ok(raw_value);
            }
            
            // Otherwise return method functions
            match key.as_str() {
                "Clone" => {
                    let clone_fn = lua.create_function(|lua, this: mlua::Table| {
                        let globals = lua.globals();
                        let class_name: String = this.get("_className")?;
                        
                        // Get next entity ID
                        let entity_id: i64 = globals.get("__NEXT_ENTITY_ID__")?;
                        globals.set("__NEXT_ENTITY_ID__", entity_id + 1)?;
                        
                        // Create new instance
                        let clone = lua.create_table()?;
                        clone.set("_entityId", entity_id)?;
                        clone.set("_className", class_name.clone())?;
                        
                        // Copy properties
                        for pair in this.pairs::<mlua::Value, mlua::Value>() {
                            let (k, v) = pair?;
                            if let mlua::Value::String(key_str) = &k {
                                let key = key_str.to_str()?;
                                if !key.starts_with('_') && key != "Parent" {
                                    clone.set(k, v)?;
                                }
                            }
                        }
                        
                        clone.set("Parent", mlua::Value::Nil)?;
                        clone.set("_children", lua.create_table()?)?;
                        
                        // Register clone
                        let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
                        registry.set(entity_id, clone.clone())?;
                        
                        Ok(clone)
                    })?;
                    Ok(mlua::Value::Function(clone_fn))
                }
                "Destroy" => {
                    let destroy_fn = lua.create_function(|lua, this: mlua::Table| {
                        let globals = lua.globals();
                        let entity_id: i64 = this.get("_entityId")?;
                        
                        // Remove from parent's children
                        let parent: mlua::Value = this.get("Parent")?;
                        if let mlua::Value::Table(parent_table) = parent {
                            let parent_children: mlua::Table = parent_table.get("_children")?;
                            parent_children.set(entity_id, mlua::Value::Nil)?;
                        }
                        
                        // Recursively destroy children
                        let children: mlua::Table = this.get("_children")?;
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let destroy: mlua::Function = child.get("Destroy")?;
                            destroy.call::<()>(child)?;
                        }
                        
                        // Remove from registry
                        let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
                        registry.set(entity_id, mlua::Value::Nil)?;
                        
                        tracing::info!("[Luau Instance] Destroyed entity {}", entity_id);
                        Ok(())
                    })?;
                    Ok(mlua::Value::Function(destroy_fn))
                }
                "FindFirstChild" => {
                    let find_fn = lua.create_function(|_, (this, name, recursive): (mlua::Table, String, Option<bool>)| {
                        let recursive = recursive.unwrap_or(false);
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let child_name: String = child.get("Name")?;
                            if child_name == name {
                                return Ok(mlua::Value::Table(child));
                            }
                            
                            if recursive {
                                let find_child: mlua::Function = child.get("FindFirstChild")?;
                                let result: mlua::Value = find_child.call((child.clone(), name.clone(), Some(true)))?;
                                if result != mlua::Value::Nil {
                                    return Ok(result);
                                }
                            }
                        }
                        
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_fn))
                }
                "FindFirstChildOfClass" => {
                    let find_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let child_class: String = child.get("_className")?;
                            if child_class == class_name {
                                return Ok(mlua::Value::Table(child));
                            }
                        }
                        
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_fn))
                }
                "GetChildren" => {
                    let get_fn = lua.create_function(|lua, this: mlua::Table| {
                        let children: mlua::Table = this.get("_children")?;
                        let result = lua.create_table()?;
                        let mut idx = 1;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            result.set(idx, child)?;
                            idx += 1;
                        }
                        
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_fn))
                }
                "GetDescendants" => {
                    let get_fn = lua.create_function(|lua, this: mlua::Table| {
                        let result = lua.create_table()?;
                        let mut idx = 1;
                        
                        fn collect_descendants(table: &mlua::Table, result: &mlua::Table, idx: &mut i32) -> mlua::Result<()> {
                            let children: mlua::Table = table.get("_children")?;
                            for pair in children.pairs::<i64, mlua::Table>() {
                                let (_, child) = pair?;
                                result.set(*idx, child.clone())?;
                                *idx += 1;
                                collect_descendants(&child, result, idx)?;
                            }
                            Ok(())
                        }
                        
                        collect_descendants(&this, &result, &mut idx)?;
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_fn))
                }
                "IsA" => {
                    let is_a_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let this_class: String = this.get("_className")?;
                        
                        // Direct match
                        if this_class == class_name {
                            return Ok(true);
                        }
                        
                        // Inheritance checks
                        let result = match class_name.as_str() {
                            "Instance" => true,
                            "BasePart" => matches!(this_class.as_str(), 
                                "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" | "SpawnLocation" | "Seat"),
                            "PVInstance" => matches!(this_class.as_str(),
                                "Part" | "MeshPart" | "Model" | "BasePart"),
                            "GuiObject" => matches!(this_class.as_str(),
                                "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton"),
                            "LuaSourceContainer" => matches!(this_class.as_str(),
                                "Script" | "LocalScript" | "ModuleScript"),
                            _ => false,
                        };
                        
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(is_a_fn))
                }
                "IsDescendantOf" => {
                    let is_desc_fn = lua.create_function(|_, (this, ancestor): (mlua::Table, mlua::Table)| {
                        let ancestor_id: i64 = ancestor.get("_entityId")?;
                        let mut current: mlua::Value = this.get("Parent")?;
                        
                        while let mlua::Value::Table(parent) = current {
                            let parent_id: i64 = parent.get("_entityId")?;
                            if parent_id == ancestor_id {
                                return Ok(true);
                            }
                            current = parent.get("Parent")?;
                        }
                        
                        Ok(false)
                    })?;
                    Ok(mlua::Value::Function(is_desc_fn))
                }
                "GetFullName" => {
                    let get_name_fn = lua.create_function(|_, this: mlua::Table| {
                        let mut parts: Vec<String> = Vec::new();
                        let mut current = mlua::Value::Table(this);
                        
                        while let mlua::Value::Table(inst) = current {
                            let name: String = inst.get("Name")?;
                            parts.push(name);
                            current = inst.get("Parent")?;
                        }
                        
                        parts.reverse();
                        Ok(parts.join("."))
                    })?;
                    Ok(mlua::Value::Function(get_name_fn))
                }
                "ClearAllChildren" => {
                    let clear_fn = lua.create_function(|_, this: mlua::Table| {
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let destroy: mlua::Function = child.get("Destroy")?;
                            destroy.call::<()>(child)?;
                        }
                        
                        Ok(())
                    })?;
                    Ok(mlua::Value::Function(clear_fn))
                }
                "GetAttribute" => {
                    let get_attr_fn = lua.create_function(|lua, (this, name): (mlua::Table, String)| {
                        let attrs_key = format!("_attr_{}", name);
                        let val: mlua::Value = this.raw_get(attrs_key)?;
                        if !matches!(val, mlua::Value::Nil) {
                            return Ok(val);
                        }
                        // Engine fallback: an instance table stamped with a
                        // `_uuid` mirrors a live ECS entity — read its
                        // `Attributes` component from the engine snapshot
                        // (see the "Engine ↔ VM attribute seam" section).
                        // Missing attribute stays nil.
                        let uuid: String = this.raw_get("_uuid").unwrap_or_default();
                        if !uuid.is_empty() {
                            if let Some(av) = engine_attribute_get(&uuid, &name) {
                                return attribute_value_to_lua(lua, &av);
                            }
                        }
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(get_attr_fn))
                }
                "SetAttribute" => {
                    let set_attr_fn = lua.create_function(|lua, (this, name, value): (mlua::Table, String, mlua::Value)| {
                        // Typed conversion FIRST so an unsupported value type
                        // (function, thread, plain table, Instance) raises a
                        // Lua error naming the type — Roblox parity — instead
                        // of silently storing a value no persistence path can
                        // represent. `nil` converts to None (= remove).
                        let typed = lua_value_to_attribute(&value).map_err(|type_name| {
                            mlua::Error::RuntimeError(format!(
                                "SetAttribute: unsupported value type '{}' for attribute '{}'",
                                type_name, name
                            ))
                        })?;
                        let attrs_key = format!("_attr_{}", name);
                        // Detect a real change so the signal only fires on change
                        // (matching Roblox AttributeChanged semantics).
                        let prev: mlua::Value = this.raw_get(attrs_key.clone())?;
                        let changed = prev != value;
                        this.raw_set(attrs_key, value)?;
                        // Engine write-back: a `_uuid`-stamped table mirrors a
                        // live ECS entity — queue the typed write for the
                        // engine drain (`apply_luau_attribute_writes`) and
                        // shadow the snapshot so same-run reads stay coherent.
                        // VM-local instances (no `_uuid`) skip this and flow
                        // through `drain_created_instances().attributes`.
                        if changed {
                            let uuid: String = this.raw_get("_uuid").unwrap_or_default();
                            if !uuid.is_empty() {
                                engine_attribute_shadow(&uuid, &name, typed.as_ref());
                                push_engine_attribute_write(EngineAttributeWrite {
                                    uuid,
                                    name: name.clone(),
                                    value: typed,
                                });
                            }
                        }
                        // Fire the per-attribute changed signal for in-runtime
                        // (script-driven) attribute writes. This is the firing
                        // path for GetAttributeChangedSignal when the change
                        // originates from Luau. Engine-driven attribute changes
                        // (the Changed<Attributes> observer in instance_loader.rs,
                        // which this layer must NOT edit) still need the
                        // engine-side hook documented at GetAttributeChangedSignal.
                        if changed {
                            let entity_id: i64 = this.raw_get("_entityId").unwrap_or(0);
                            if entity_id != 0 {
                                let globals = lua.globals();
                                if let Ok(fire_fn) = globals.get::<mlua::Function>("__fire_event__") {
                                    let event_name = format!("AttributeChanged:{}", name);
                                    let _ = fire_fn.call::<()>((entity_id, event_name, name.clone()));
                                }
                            }
                        }
                        Ok(())
                    })?;
                    Ok(mlua::Value::Function(set_attr_fn))
                }
                "GetAttributes" => {
                    let get_attrs_fn = lua.create_function(|lua, this: mlua::Table| {
                        let result = lua.create_table()?;
                        // Engine layer first (uuid-stamped tables mirror live
                        // ECS entities) so script-side `_attr_*` writes below
                        // overlay it — same precedence as `GetAttribute`.
                        let uuid: String = this.raw_get("_uuid").unwrap_or_default();
                        if !uuid.is_empty() {
                            for (attr_name, av) in engine_attributes_all(&uuid) {
                                let lv = attribute_value_to_lua(lua, &av)?;
                                if !matches!(lv, mlua::Value::Nil) {
                                    result.set(attr_name, lv)?;
                                }
                            }
                        }
                        for pair in this.pairs::<String, mlua::Value>() {
                            let (k, v) = pair?;
                            if let Some(attr_name) = k.strip_prefix("_attr_") {
                                result.set(attr_name.to_string(), v)?;
                            }
                        }
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_attrs_fn))
                }
                // GetAttributeChangedSignal(name) -> RBXScriptSignal
                //
                // Mirrors instance.Changed / GetPropertyChangedSignal: returns a
                // signal from the shared event registry keyed by this entity and
                // the per-attribute event name "AttributeChanged:<name>". The
                // returned table exposes :Connect / :Once / :Wait exactly like
                // the other instance signals, so user code can connect today.
                //
                // FIRING — two paths:
                //  (1) Script-driven: the "SetAttribute" arm above fires this
                //      signal via `__fire_event__(entity_id, "AttributeChanged:<name>", name)`
                //      whenever a Luau script changes the attribute value. This
                //      path works NOW (covers the importer's folded ValueObject
                //      writes that the compat rewrite turns into :SetAttribute).
                //  (2) Engine-driven: when the attribute is mutated from the ECS
                //      side (the `Changed<Attributes>` observer in
                //      `engine/instance_loader.rs`, which THIS layer must not
                //      edit), nothing in the Luau VM observes it. To make engine
                //      mutations fire too, that observer must call the same Luau
                //      global: `__fire_event__(entity_id, "AttributeChanged:" .. name, name)`
                //      after writing the `_attr_<name>` field on the instance
                //      table (or via a small Rust helper that does so). Until that
                //      hook is added, only path (1) fires. See report.
                "GetAttributeChangedSignal" => {
                    let get_attr_sig_fn = lua.create_function(|lua, (this, attr_name): (mlua::Table, String)| {
                        let entity_id: i64 = this.raw_get("_entityId").unwrap_or(0);
                        let globals = lua.globals();
                        let get_or_create: mlua::Function = globals.get("__get_or_create_event__")?;
                        // Namespaced event key so each attribute gets its own signal.
                        let event_name = format!("AttributeChanged:{}", attr_name);
                        let signal: mlua::Table = get_or_create.call((entity_id, event_name))?;
                        Ok(signal)
                    })?;
                    Ok(mlua::Value::Function(get_attr_sig_fn))
                }
                // GetUuid() -> string
                //
                // Returns the stable per-instance UUID string stored at the raw
                // `_uuid` field, or "" if this instance has no UUID (e.g. a
                // script-created `Instance.new` that was never persisted). The
                // ObjectValue assignment rewrite emits `inst:GetUuid()` so a live
                // instance reference can be stored back into a UUID-string
                // attribute. The importer / instance loader is responsible for
                // stamping `_uuid` on instance tables it seeds (see report).
                "GetUuid" => {
                    let get_uuid_fn = lua.create_function(|_, this: mlua::Table| {
                        let uuid: String = this.raw_get("_uuid").unwrap_or_default();
                        Ok(uuid)
                    })?;
                    Ok(mlua::Value::Function(get_uuid_fn))
                }
                "GetDescendants" => {
                    let get_desc_fn = lua.create_function(|lua, this: mlua::Table| {
                        let result = lua.create_table()?;
                        let mut idx = 1;
                        fn collect_descendants(table: &mlua::Table, result: &mlua::Table, idx: &mut i64) -> mlua::Result<()> {
                            if let Ok(children) = table.raw_get::<mlua::Table>("_children") {
                                for pair in children.pairs::<mlua::Value, mlua::Table>() {
                                    let (_, child) = pair?;
                                    result.set(*idx, child.clone())?;
                                    *idx += 1;
                                    collect_descendants(&child, result, idx)?;
                                }
                            }
                            Ok(())
                        }
                        collect_descendants(&this, &result, &mut idx)?;
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_desc_fn))
                }
                "GetFullName" => {
                    let get_full_name_fn = lua.create_function(|_, this: mlua::Table| {
                        let mut parts: Vec<String> = Vec::new();
                        let mut current = this;
                        loop {
                            let name: String = current.raw_get("Name").unwrap_or_else(|_| "???".to_string());
                            parts.push(name);
                            match current.raw_get::<mlua::Value>("Parent") {
                                Ok(mlua::Value::Table(parent)) => current = parent,
                                _ => break,
                            }
                        }
                        parts.reverse();
                        Ok(parts.join("."))
                    })?;
                    Ok(mlua::Value::Function(get_full_name_fn))
                }
                // WaitForChild(name, timeout?) — returns child or errors
                "WaitForChild" => {
                    let wait_for_child_fn = lua.create_function(|_, (this, name, _timeout): (mlua::Table, String, Option<f64>)| {
                        // Immediate lookup (TODO: integrate with coroutine scheduler for actual waiting)
                        let children: mlua::Table = this.raw_get("_children")?;
                        for pair in children.pairs::<mlua::Value, mlua::Table>() {
                            if let Ok((_, child)) = pair {
                                let child_name: String = child.raw_get("Name").unwrap_or_default();
                                if child_name == name {
                                    return Ok(mlua::Value::Table(child));
                                }
                            }
                        }
                        Err(mlua::Error::RuntimeError(format!(
                            "Infinite yield possible on '{}:WaitForChild(\"{}\")'", 
                            this.raw_get::<String>("Name").unwrap_or_default(), name
                        )))
                    })?;
                    Ok(mlua::Value::Function(wait_for_child_fn))
                }
                // FindFirstAncestor(name) — walks up Parent chain
                "FindFirstAncestor" => {
                    let find_ancestor_fn = lua.create_function(|_, (this, name): (mlua::Table, String)| {
                        let mut current: mlua::Value = this.raw_get("Parent")?;
                        while let mlua::Value::Table(parent) = current {
                            let parent_name: String = parent.raw_get("Name").unwrap_or_default();
                            if parent_name == name {
                                return Ok(mlua::Value::Table(parent));
                            }
                            current = parent.raw_get("Parent")?;
                        }
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_ancestor_fn))
                }
                // FindFirstAncestorOfClass(className)
                "FindFirstAncestorOfClass" => {
                    let find_ancestor_class_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let mut current: mlua::Value = this.raw_get("Parent")?;
                        while let mlua::Value::Table(parent) = current {
                            let parent_class: String = parent.raw_get("_className").unwrap_or_default();
                            if parent_class == class_name {
                                return Ok(mlua::Value::Table(parent));
                            }
                            current = parent.raw_get("Parent")?;
                        }
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_ancestor_class_fn))
                }
                // FindFirstAncestorWhichIsA(className) — includes inheritance
                "FindFirstAncestorWhichIsA" => {
                    let find_ancestor_isa_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let mut current: mlua::Value = this.raw_get("Parent")?;
                        while let mlua::Value::Table(parent) = current {
                            let parent_class: String = parent.raw_get("_className").unwrap_or_default();
                            if parent_class == class_name {
                                return Ok(mlua::Value::Table(parent));
                            }
                            // Inheritance check
                            let matches = match class_name.as_str() {
                                "BasePart" => matches!(parent_class.as_str(),
                                    "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" | "SpawnLocation" | "Seat"),
                                "PVInstance" => matches!(parent_class.as_str(),
                                    "Part" | "MeshPart" | "Model" | "BasePart"),
                                "GuiObject" => matches!(parent_class.as_str(),
                                    "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton"),
                                _ => false,
                            };
                            if matches {
                                return Ok(mlua::Value::Table(parent));
                            }
                            current = parent.raw_get("Parent")?;
                        }
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_ancestor_isa_fn))
                }
                // Event signal access: Changed, ChildAdded, ChildRemoved, Touched, TouchEnded
                "Changed" | "ChildAdded" | "ChildRemoved" | "Touched" | "TouchEnded" 
                | "AncestryChanged" | "DescendantAdded" | "DescendantRemoving" => {
                    let event_name_owned = key.to_string();
                    let entity_id: i64 = this.raw_get("_entityId").unwrap_or(0);
                    let globals = lua.globals();
                    let get_or_create: mlua::Function = globals.get("__get_or_create_event__")?;
                    let signal: mlua::Table = get_or_create.call((entity_id, event_name_owned))?;
                    Ok(mlua::Value::Table(signal))
                }
                _ => {
                    // Try finding a child with this name (implicit child access)
                    let children: mlua::Table = this.raw_get("_children")?;
                    for pair in children.pairs::<mlua::Value, mlua::Table>() {
                        if let Ok((_, child)) = pair {
                            let child_name: String = child.raw_get("Name").unwrap_or_default();
                            if child_name == key {
                                return Ok(mlua::Value::Table(child));
                            }
                        }
                    }
                    Ok(mlua::Value::Nil)
                }
            }
        }).map_err(|e| format!("Failed to create instance __index: {}", e))?;
        
        instance_mt.set("__index", instance_index)
            .map_err(|e| format!("Failed to set instance __index: {}", e))?;

        // __newindex: intercept property writes on instances.
        // For BasePart properties (Position, Size, Color, Material, Anchored,
        // Transparency, CanCollide), log the change for the engine to apply.
        // All other properties are set directly on the table.
        let instance_newindex = lua.create_function(|lua, (this, key, value): (mlua::Table, String, mlua::Value)| {
            // Helper: fire the Changed event for this instance if listeners exist
            let fire_changed = |lua: &mlua::Lua, inst: &mlua::Table, property: &str| -> mlua::Result<()> {
                let entity_id: i64 = inst.raw_get("_entityId").unwrap_or(0);
                if entity_id == 0 { return Ok(()); }
                let globals = lua.globals();
                if let Ok(fire_fn) = globals.get::<mlua::Function>("__fire_event__") {
                    let _ = fire_fn.call::<()>((entity_id, "Changed", property.to_string()));
                }
                Ok(())
            };

            match key.as_str() {
                // BasePart tracked properties — set + log + fire Changed
                "Position" | "Size" | "CFrame" | "Material" | "Transparency" 
                | "Anchored" | "CanCollide" | "Reflectance" | "Velocity"
                | "RotVelocity" | "Massless" | "RootPriority" => {
                    this.raw_set(key.clone(), value)?;
                    let name: String = this.raw_get("Name").unwrap_or_default();
                    tracing::debug!("[Luau] {}.{} changed", name, key);
                    fire_changed(lua, &this, &key)?;
                }
                "Color" | "Color3" | "BrickColor" => {
                    this.raw_set("Color", value)?;
                    let name: String = this.raw_get("Name").unwrap_or_default();
                    tracing::debug!("[Luau] {}.Color changed", name);
                    fire_changed(lua, &this, "Color")?;
                }
                // Name change — fire Changed
                "Name" => {
                    this.raw_set(key.clone(), value)?;
                    fire_changed(lua, &this, &key)?;
                }
                // Parent reparenting — fire AncestryChanged + ChildAdded/ChildRemoved
                "Parent" => {
                    let entity_id: i64 = this.raw_get("_entityId").unwrap_or(0);

                    // Remove from old parent's children and fire ChildRemoved
                    let old_parent: mlua::Value = this.raw_get("Parent")?;
                    if let mlua::Value::Table(ref old_pt) = old_parent {
                        if let Ok(old_children) = old_pt.raw_get::<mlua::Table>("_children") {
                            old_children.set(entity_id, mlua::Value::Nil)?;
                        }
                        // Fire ChildRemoved on old parent
                        let old_parent_id: i64 = old_pt.raw_get("_entityId").unwrap_or(0);
                        if old_parent_id != 0 {
                            let globals = lua.globals();
                            if let Ok(fire_fn) = globals.get::<mlua::Function>("__fire_event__") {
                                let _ = fire_fn.call::<()>((old_parent_id, "ChildRemoved", this.clone()));
                            }
                        }
                    }

                    // Set new parent
                    this.raw_set("Parent", value.clone())?;

                    // Add to new parent's children and fire ChildAdded
                    if let mlua::Value::Table(ref new_pt) = value {
                        if let Ok(new_children) = new_pt.raw_get::<mlua::Table>("_children") {
                            new_children.set(entity_id, this.clone())?;
                        }
                        // Fire ChildAdded on new parent
                        let new_parent_id: i64 = new_pt.raw_get("_entityId").unwrap_or(0);
                        if new_parent_id != 0 {
                            let globals = lua.globals();
                            if let Ok(fire_fn) = globals.get::<mlua::Function>("__fire_event__") {
                                let _ = fire_fn.call::<()>((new_parent_id, "ChildAdded", this.clone()));
                            }
                        }
                    }

                    // Fire AncestryChanged on the instance itself
                    fire_changed(lua, &this, "Parent")?;
                    if entity_id != 0 {
                        let globals = lua.globals();
                        if let Ok(fire_fn) = globals.get::<mlua::Function>("__fire_event__") {
                            let _ = fire_fn.call::<()>((entity_id, "AncestryChanged", (this.clone(), value)));
                        }
                    }
                }
                _ => {
                    // Default: set directly on the table, fire Changed
                    this.raw_set(key.clone(), value)?;
                    fire_changed(lua, &this, &key)?;
                }
            }
            Ok(())
        }).map_err(|e| format!("Failed to create instance __newindex: {}", e))?;

        instance_mt.set("__newindex", instance_newindex)
            .map_err(|e| format!("Failed to set instance __newindex: {}", e))?;

        // Store metatable for use by Instance.new
        globals.set("__INSTANCE_MT__", instance_mt)
            .map_err(|e| format!("Failed to set instance metatable: {}", e))?;

        // ====================================================================
        // FindByUUID(uuid) -> Instance? — ObjectValue resolver
        // ====================================================================
        //
        // The importer folds Roblox `ObjectValue`s into a UUID-string attribute
        // on the parent. The value-object rewrite (compat.rs CONTRACT D) turns a
        // read of such a `.Value` into `FindByUUID(parent:GetAttribute("Name"))`.
        // This resolves that UUID string back to the live instance table.
        //
        // RESOLUTION STRATEGY (in-runtime, no engine World access from here):
        // scan `__INSTANCE_REGISTRY__` for the table whose raw `_uuid` field
        // equals `uuid`; return it, or `nil`. The registry is the runtime's
        // authoritative entity↔table map (keyed by `_entityId`); a UUID index is
        // not maintained, so this is a linear scan. For the import use case the
        // registry holds the loaded scene, which is the correct lookup domain.
        //
        // LIMITATION: instance tables only carry `_uuid` if something stamped it
        // (the importer / instance_loader when seeding pre-existing instances).
        // A bare `Instance.new` does not get a `_uuid`, so it is unresolvable by
        // UUID until persisted — acceptable, since only persisted ObjectValue
        // targets have UUIDs to begin with. An empty/`nil` uuid returns `nil`.
        let find_by_uuid = lua.create_function(|lua, uuid: Option<String>| {
            let uuid = match uuid {
                Some(u) if !u.is_empty() => u,
                _ => return Ok(mlua::Value::Nil),
            };
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            for pair in registry.pairs::<i64, mlua::Table>() {
                if let Ok((_, inst)) = pair {
                    let inst_uuid: String = inst.raw_get("_uuid").unwrap_or_default();
                    if inst_uuid == uuid {
                        return Ok(mlua::Value::Table(inst));
                    }
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create FindByUUID: {}", e))?;

        // Expose as the bare global the rewrite emits. This is the canonical
        // surface (the value-object rewrite only ever emits the bare global).
        globals.set("FindByUUID", find_by_uuid.clone())
            .map_err(|e| format!("Failed to set FindByUUID global: {}", e))?;

        // Also expose as a convenience method on `game` and `workspace` (both
        // exist from inject_core_globals). A dedicated variadic wrapper accepts
        // BOTH `game.FindByUUID(uuid)` (dot: one string arg) and
        // `game:FindByUUID(uuid)` (colon: implicit self table + string) by
        // resolving the LAST string argument and delegating to the global.
        let find_by_uuid_method = lua.create_function(|lua, args: mlua::MultiValue| {
            // Resolve the LAST string argument (so the dot form's sole arg and
            // the colon form's trailing arg both land on the uuid).
            let mut uuid: Option<String> = None;
            for v in args.into_iter() {
                if let mlua::Value::String(s) = v {
                    uuid = Some(s.to_string_lossy().to_string());
                }
            }
            let globals = lua.globals();
            let global_fn: mlua::Function = globals.get("FindByUUID")?;
            global_fn.call::<mlua::Value>(uuid)
        }).map_err(|e| format!("Failed to create FindByUUID method: {}", e))?;
        if let Ok(game_tbl) = globals.get::<mlua::Table>("game") {
            let _ = game_tbl.set("FindByUUID", find_by_uuid_method.clone());
        }
        if let Ok(ws_tbl) = globals.get::<mlua::Table>("workspace") {
            let _ = ws_tbl.set("FindByUUID", find_by_uuid_method.clone());
        }

        Ok(())
    }

    // ========================================================================
    // task library: task.wait, task.spawn, task.defer, task.delay
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_task_library(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // Stub `task` library for coroutine scheduling
        let task_table = lua.create_table()
            .map_err(|error| format!("Failed to create task table: {}", error))?;

        // task.wait(seconds) — yields current thread
        let task_wait = lua.create_function(|_, seconds: Option<f64>| {
            let _duration = seconds.unwrap_or(0.0);
            // TODO: Integrate with Bevy frame scheduling
            // For now, this is a no-op that returns immediately
            Ok(())
        }).map_err(|error| format!("Failed to create task.wait: {}", error))?;
        task_table.set("wait", task_wait)
            .map_err(|error| format!("Failed to set task.wait: {}", error))?;

        // task.spawn(function, ...) — execute function immediately in a new logical thread
        let task_spawn = lua.create_function(|_, (function, args): (mlua::Function, mlua::MultiValue)| {
            // Execute immediately (proper coroutine scheduling is TODO)
            let _ = function.call::<mlua::MultiValue>(args);
            Ok(())
        }).map_err(|error| format!("Failed to create task.spawn: {}", error))?;
        task_table.set("spawn", task_spawn)
            .map_err(|error| format!("Failed to set task.spawn: {}", error))?;

        // task.defer(function, ...) — defer execution to end of current resumption cycle
        let task_defer = lua.create_function(|_, (function, args): (mlua::Function, mlua::MultiValue)| {
            // Execute immediately for now (proper deferral is TODO)
            let _ = function.call::<mlua::MultiValue>(args);
            Ok(())
        }).map_err(|error| format!("Failed to create task.defer: {}", error))?;
        task_table.set("defer", task_defer)
            .map_err(|error| format!("Failed to set task.defer: {}", error))?;

        // task.delay(seconds, function, ...) — execute function after delay
        let task_delay = lua.create_function(|_, (_seconds, function, args): (f64, mlua::Function, mlua::MultiValue)| {
            // Execute immediately for now (proper timer scheduling is TODO)
            // In production this would queue into a timer system
            let _ = function.call::<mlua::MultiValue>(args);
            Ok(())
        }).map_err(|error| format!("Failed to create task.delay: {}", error))?;
        task_table.set("delay", task_delay)
            .map_err(|error| format!("Failed to set task.delay: {}", error))?;

        // task.cancel(thread) — cancel a spawned/delayed thread
        let task_cancel = lua.create_function(|_, _thread: mlua::Value| {
            // TODO: Integrate with coroutine scheduler
            Ok(())
        }).map_err(|error| format!("Failed to create task.cancel: {}", error))?;
        task_table.set("cancel", task_cancel)
            .map_err(|error| format!("Failed to set task.cancel: {}", error))?;

        globals.set("task", task_table)
            .map_err(|error| format!("Failed to set task: {}", error))?;

        // Legacy `wait()` global (deprecated in Roblox, but widely used)
        let legacy_wait = lua.create_function(|_, seconds: Option<f64>| {
            let _duration = seconds.unwrap_or(0.03); // ~1 frame at 30fps
            Ok(seconds.unwrap_or(0.03))
        }).map_err(|error| format!("Failed to create wait: {}", error))?;
        globals.set("wait", legacy_wait)
            .map_err(|error| format!("Failed to set wait: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // TweenService
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_tween_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P1: TweenService
        // ====================================================================
        let tween_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create TweenService table: {}", error))?;

        // TweenService:Create(tweenInfo) -> Tween
        let tween_create = lua.create_function(|lua, info: super::types::LuauTweenInfo| {
            // Create a tween table with play/pause/cancel methods
            let tween = lua.create_table()?;
            tween.set("_info", info)?;
            tween.set("_status", 1i32)?; // 1 = Paused
            
            tween.set("Play", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 0i32)?; // 0 = Playing
                Ok(())
            })?)?;
            
            tween.set("Pause", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 1i32)?; // 1 = Paused
                Ok(())
            })?)?;
            
            tween.set("Cancel", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 2i32)?; // 2 = Cancelled
                Ok(())
            })?)?;
            
            Ok(tween)
        }).map_err(|error| format!("Failed to create TweenService:Create: {}", error))?;
        tween_service_table.set("Create", tween_create)
            .map_err(|error| format!("Failed to set TweenService.Create: {}", error))?;

        globals.set("TweenService", tween_service_table)
            .map_err(|error| format!("Failed to set TweenService: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // RunService — frame-based event signals
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_run_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P0: RunService — Frame-based event signals
        // ====================================================================
        let run_service_table = lua.create_table()
            .map_err(|e| format!("Failed to create RunService table: {}", e))?;

        // RunService.Heartbeat — fires every frame after physics
        let heartbeat = Self::create_signal(lua)?;
        run_service_table.set("Heartbeat", heartbeat)
            .map_err(|e| format!("Failed to set Heartbeat: {}", e))?;

        // RunService.Stepped — fires every frame before physics
        let stepped = Self::create_signal(lua)?;
        run_service_table.set("Stepped", stepped)
            .map_err(|e| format!("Failed to set Stepped: {}", e))?;

        // RunService.RenderStepped — fires every frame before rendering (client only)
        let render_stepped = Self::create_signal(lua)?;
        run_service_table.set("RenderStepped", render_stepped)
            .map_err(|e| format!("Failed to set RenderStepped: {}", e))?;

        // RunService:IsClient() -> bool
        let is_client = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsClient: {}", e))?;
        run_service_table.set("IsClient", is_client)
            .map_err(|e| format!("Failed to set IsClient: {}", e))?;

        // RunService:IsServer() -> bool
        let is_server = lua.create_function(|_, ()| Ok(false))
            .map_err(|e| format!("Failed to create IsServer: {}", e))?;
        run_service_table.set("IsServer", is_server)
            .map_err(|e| format!("Failed to set IsServer: {}", e))?;

        // RunService:IsStudio() -> bool
        let is_studio = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsStudio: {}", e))?;
        run_service_table.set("IsStudio", is_studio)
            .map_err(|e| format!("Failed to set IsStudio: {}", e))?;

        // RunService:IsRunning() -> bool
        let is_running = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsRunning: {}", e))?;
        run_service_table.set("IsRunning", is_running)
            .map_err(|e| format!("Failed to set IsRunning: {}", e))?;

        // RunService:BindToRenderStep(name, priority, callback)
        let bind_to_render = lua.create_function(|lua, (name, _priority, callback): (String, i32, mlua::Function)| {
            // Store in a global table for render step bindings
            let globals = lua.globals();
            let bindings: mlua::Table = globals.get::<mlua::Table>("__RENDER_STEP_BINDINGS__")
                .unwrap_or_else(|_| {
                    let t = lua.create_table().unwrap();
                    globals.set("__RENDER_STEP_BINDINGS__", t.clone()).ok();
                    t
                });
            bindings.set(name, callback)?;
            Ok(())
        }).map_err(|e| format!("Failed to create BindToRenderStep: {}", e))?;
        run_service_table.set("BindToRenderStep", bind_to_render)
            .map_err(|e| format!("Failed to set BindToRenderStep: {}", e))?;

        // RunService:UnbindFromRenderStep(name)
        let unbind_from_render = lua.create_function(|lua, name: String| {
            let globals = lua.globals();
            if let Ok(bindings) = globals.get::<mlua::Table>("__RENDER_STEP_BINDINGS__") {
                bindings.set(name, mlua::Value::Nil)?;
            }
            Ok(())
        }).map_err(|e| format!("Failed to create UnbindFromRenderStep: {}", e))?;
        run_service_table.set("UnbindFromRenderStep", unbind_from_render)
            .map_err(|e| format!("Failed to set UnbindFromRenderStep: {}", e))?;

        globals.set("RunService", run_service_table)
            .map_err(|e| format!("Failed to set RunService: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // UserInputService
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_user_input_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P1: UserInputService
        // ====================================================================
        let uis_table = lua.create_table()
            .map_err(|error| format!("Failed to create UserInputService table: {}", error))?;

        // UserInputService:IsKeyDown(keyCode) -> bool
        let is_key_down = lua.create_function(|_, _key_code: i32| {
            // TODO: Wire to actual input state
            Ok(false)
        }).map_err(|error| format!("Failed to create IsKeyDown: {}", error))?;
        uis_table.set("IsKeyDown", is_key_down)
            .map_err(|error| format!("Failed to set IsKeyDown: {}", error))?;

        // UserInputService:IsMouseButtonPressed(button) -> bool
        let is_mouse_pressed = lua.create_function(|_, _button: i32| {
            Ok(false)
        }).map_err(|error| format!("Failed to create IsMouseButtonPressed: {}", error))?;
        uis_table.set("IsMouseButtonPressed", is_mouse_pressed)
            .map_err(|error| format!("Failed to set IsMouseButtonPressed: {}", error))?;

        // UserInputService:GetMouseLocation() -> Vector2 (as table)
        let get_mouse_location = lua.create_function(|lua, ()| {
            let result = lua.create_table()?;
            result.set("X", 0.0f64)?;
            result.set("Y", 0.0f64)?;
            Ok(result)
        }).map_err(|error| format!("Failed to create GetMouseLocation: {}", error))?;
        uis_table.set("GetMouseLocation", get_mouse_location)
            .map_err(|error| format!("Failed to set GetMouseLocation: {}", error))?;

        // UserInputService:GetMouseDelta() -> Vector2 (as table)
        let get_mouse_delta = lua.create_function(|lua, ()| {
            let result = lua.create_table()?;
            result.set("X", 0.0f64)?;
            result.set("Y", 0.0f64)?;
            Ok(result)
        }).map_err(|error| format!("Failed to create GetMouseDelta: {}", error))?;
        uis_table.set("GetMouseDelta", get_mouse_delta)
            .map_err(|error| format!("Failed to set GetMouseDelta: {}", error))?;

        globals.set("UserInputService", uis_table)
            .map_err(|error| format!("Failed to set UserInputService: {}", error))?;

        // Debris service (bundled here since it's small)
        // ====================================================================
        let debris_table = lua.create_table()
            .map_err(|error| format!("Failed to create Debris table: {}", error))?;

        // Debris:AddItem(instance, lifetime)
        let add_item = lua.create_function(|_, (_instance, _lifetime): (mlua::Value, f64)| {
            // TODO: Wire to DebrisService
            Ok(())
        }).map_err(|error| format!("Failed to create Debris:AddItem: {}", error))?;
        debris_table.set("AddItem", add_item)
            .map_err(|error| format!("Failed to set Debris.AddItem: {}", error))?;

        globals.set("Debris", debris_table)
            .map_err(|error| format!("Failed to set Debris: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // Players Service
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_players_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P1: Players Service
        // ====================================================================
        let players_table = lua.create_table()
            .map_err(|e| format!("Failed to create Players table: {}", e))?;

        // Create a default LocalPlayer instance
        let local_player = lua.create_table()
            .map_err(|e| format!("Failed to create LocalPlayer: {}", e))?;
        local_player.set("_entityId", 1i64).map_err(|e| format!("Failed to set _entityId: {}", e))?;
        local_player.set("_className", "Player").map_err(|e| format!("Failed to set _className: {}", e))?;
        local_player.set("Name", "LocalPlayer").map_err(|e| format!("Failed to set Name: {}", e))?;
        local_player.set("UserId", 1i64).map_err(|e| format!("Failed to set UserId: {}", e))?;
        local_player.set("DisplayName", "Player").map_err(|e| format!("Failed to set DisplayName: {}", e))?;
        local_player.set("Character", mlua::Value::Nil).map_err(|e| format!("Failed to set Character: {}", e))?;
        local_player.set("Team", mlua::Value::Nil).map_err(|e| format!("Failed to set Team: {}", e))?;
        
        // Player methods
        let get_mouse = lua.create_function(|lua, _this: mlua::Table| {
            let globals = lua.globals();
            globals.get::<mlua::Table>("Mouse")
        }).map_err(|e| format!("Failed to create GetMouse: {}", e))?;
        local_player.set("GetMouse", get_mouse).map_err(|e| format!("Failed to set GetMouse: {}", e))?;
        
        let kick = lua.create_function(|_, (_this, _message): (mlua::Table, Option<String>)| {
            tracing::warn!("[Luau] Player:Kick() called - no-op in Eustress");
            Ok(())
        }).map_err(|e| format!("Failed to create Kick: {}", e))?;
        local_player.set("Kick", kick).map_err(|e| format!("Failed to set Kick: {}", e))?;

        players_table.set("LocalPlayer", local_player)
            .map_err(|e| format!("Failed to set LocalPlayer: {}", e))?;

        // Players storage for multiplayer
        let players_list = lua.create_table()
            .map_err(|e| format!("Failed to create players list: {}", e))?;
        players_table.set("_players", players_list)
            .map_err(|e| format!("Failed to set _players: {}", e))?;

        // Players:GetPlayers() -> {Player}
        let get_players = lua.create_function(|lua, this: mlua::Table| {
            let players: mlua::Table = this.get("_players")?;
            let result = lua.create_table()?;
            let mut idx = 1;
            for pair in players.pairs::<i64, mlua::Table>() {
                let (_, player) = pair?;
                result.set(idx, player)?;
                idx += 1;
            }
            // Always include LocalPlayer
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            result.set(idx, local_player)?;
            Ok(result)
        }).map_err(|e| format!("Failed to create GetPlayers: {}", e))?;
        players_table.set("GetPlayers", get_players)
            .map_err(|e| format!("Failed to set GetPlayers: {}", e))?;

        // Players:GetPlayerByUserId(userId) -> Player?
        let get_by_id = lua.create_function(|_, (this, user_id): (mlua::Table, i64)| {
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            let local_id: i64 = local_player.get("UserId")?;
            if local_id == user_id {
                return Ok(mlua::Value::Table(local_player));
            }
            let players: mlua::Table = this.get("_players")?;
            for pair in players.pairs::<i64, mlua::Table>() {
                let (_, player) = pair?;
                let pid: i64 = player.get("UserId")?;
                if pid == user_id {
                    return Ok(mlua::Value::Table(player));
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create GetPlayerByUserId: {}", e))?;
        players_table.set("GetPlayerByUserId", get_by_id)
            .map_err(|e| format!("Failed to set GetPlayerByUserId: {}", e))?;

        // Players:GetPlayerFromCharacter(character) -> Player?
        let get_from_char = lua.create_function(|_, (this, character): (mlua::Table, mlua::Table)| {
            let char_id: i64 = character.get("_entityId")?;
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            if let Ok(local_char) = local_player.get::<mlua::Table>("Character") {
                let local_char_id: i64 = local_char.get("_entityId")?;
                if local_char_id == char_id {
                    return Ok(mlua::Value::Table(local_player));
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create GetPlayerFromCharacter: {}", e))?;
        players_table.set("GetPlayerFromCharacter", get_from_char)
            .map_err(|e| format!("Failed to set GetPlayerFromCharacter: {}", e))?;

        // PlayerAdded/PlayerRemoving signals
        let player_added = Self::create_signal(lua)?;
        players_table.set("PlayerAdded", player_added)
            .map_err(|e| format!("Failed to set PlayerAdded: {}", e))?;

        let player_removing = Self::create_signal(lua)?;
        players_table.set("PlayerRemoving", player_removing)
            .map_err(|e| format!("Failed to set PlayerRemoving: {}", e))?;

        globals.set("Players", players_table)
            .map_err(|e| format!("Failed to set Players: {}", e))?;

        // Also set as game:GetService("Players") compatible
        let game: mlua::Table = globals.get("game")
            .map_err(|e| format!("Failed to get game: {}", e))?;
        let players_ref: mlua::Table = globals.get("Players")
            .map_err(|e| format!("Failed to get Players: {}", e))?;
        game.set("Players", players_ref)
            .map_err(|e| format!("Failed to set game.Players: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Storage Services: ReplicatedStorage, ServerStorage, ServerScriptService,
    //                   StarterGui, StarterPlayer, StarterPack, Lighting,
    //                   game:GetService()
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_storage_services(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();
        let game: mlua::Table = globals.get("game")
            .map_err(|e| format!("Failed to get game: {}", e))?;

        // ====================================================================
        // P1: ReplicatedStorage — Shared data container
        // ====================================================================
        let replicated_storage = lua.create_table()
            .map_err(|e| format!("Failed to create ReplicatedStorage: {}", e))?;
        replicated_storage.set("_entityId", 100001i64).map_err(|e| format!("{}", e))?;
        replicated_storage.set("_className", "ReplicatedStorage").map_err(|e| format!("{}", e))?;
        replicated_storage.set("Name", "ReplicatedStorage").map_err(|e| format!("{}", e))?;
        replicated_storage.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ReplicatedStorage", replicated_storage.clone())
            .map_err(|e| format!("Failed to set ReplicatedStorage: {}", e))?;
        game.set("ReplicatedStorage", replicated_storage)
            .map_err(|e| format!("Failed to set game.ReplicatedStorage: {}", e))?;

        // ====================================================================
        // P1: ServerStorage — Server-only data container
        // ====================================================================
        let server_storage = lua.create_table()
            .map_err(|e| format!("Failed to create ServerStorage: {}", e))?;
        server_storage.set("_entityId", 100002i64).map_err(|e| format!("{}", e))?;
        server_storage.set("_className", "ServerStorage").map_err(|e| format!("{}", e))?;
        server_storage.set("Name", "ServerStorage").map_err(|e| format!("{}", e))?;
        server_storage.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ServerStorage", server_storage.clone())
            .map_err(|e| format!("Failed to set ServerStorage: {}", e))?;
        game.set("ServerStorage", server_storage)
            .map_err(|e| format!("Failed to set game.ServerStorage: {}", e))?;

        // ====================================================================
        // P1: ServerScriptService — Server scripts container
        // ====================================================================
        let server_script_service = lua.create_table()
            .map_err(|e| format!("Failed to create ServerScriptService: {}", e))?;
        server_script_service.set("_entityId", 100003i64).map_err(|e| format!("{}", e))?;
        server_script_service.set("_className", "ServerScriptService").map_err(|e| format!("{}", e))?;
        server_script_service.set("Name", "ServerScriptService").map_err(|e| format!("{}", e))?;
        server_script_service.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ServerScriptService", server_script_service.clone())
            .map_err(|e| format!("Failed to set ServerScriptService: {}", e))?;
        game.set("ServerScriptService", server_script_service)
            .map_err(|e| format!("Failed to set game.ServerScriptService: {}", e))?;

        // ====================================================================
        // P1: StarterGui / StarterPlayer / StarterPack
        // ====================================================================
        let starter_gui = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_gui.set("_entityId", 100004i64).map_err(|e| format!("{}", e))?;
        starter_gui.set("_className", "StarterGui").map_err(|e| format!("{}", e))?;
        starter_gui.set("Name", "StarterGui").map_err(|e| format!("{}", e))?;
        starter_gui.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterGui", starter_gui.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterGui", starter_gui).map_err(|e| format!("{}", e))?;

        let starter_player = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_player.set("_entityId", 100005i64).map_err(|e| format!("{}", e))?;
        starter_player.set("_className", "StarterPlayer").map_err(|e| format!("{}", e))?;
        starter_player.set("Name", "StarterPlayer").map_err(|e| format!("{}", e))?;
        starter_player.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterPlayer", starter_player.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterPlayer", starter_player).map_err(|e| format!("{}", e))?;

        let starter_pack = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_pack.set("_entityId", 100006i64).map_err(|e| format!("{}", e))?;
        starter_pack.set("_className", "StarterPack").map_err(|e| format!("{}", e))?;
        starter_pack.set("Name", "StarterPack").map_err(|e| format!("{}", e))?;
        starter_pack.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterPack", starter_pack.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterPack", starter_pack).map_err(|e| format!("{}", e))?;

        // ====================================================================
        // P1: Lighting service
        // ====================================================================
        let lighting = lua.create_table().map_err(|e| format!("{}", e))?;
        lighting.set("_entityId", 100007i64).map_err(|e| format!("{}", e))?;
        lighting.set("_className", "Lighting").map_err(|e| format!("{}", e))?;
        lighting.set("Name", "Lighting").map_err(|e| format!("{}", e))?;
        lighting.set("Ambient", lua.create_userdata(super::types::LuauColor3::new(0.5, 0.5, 0.5)).map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        lighting.set("Brightness", 2.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("ClockTime", 14.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("GeographicLatitude", 41.733f64).map_err(|e| format!("{}", e))?;
        lighting.set("TimeOfDay", "14:00:00").map_err(|e| format!("{}", e))?;
        lighting.set("FogColor", lua.create_userdata(super::types::LuauColor3::new(0.75, 0.75, 0.75)).map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        lighting.set("FogEnd", 100000.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("FogStart", 0.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("Lighting", lighting.clone()).map_err(|e| format!("{}", e))?;
        game.set("Lighting", lighting).map_err(|e| format!("{}", e))?;

        // ====================================================================
        // game:GetService(serviceName) -> Service
        // ====================================================================
        let get_service = lua.create_function(|_, (this, service_name): (mlua::Table, String)| {
            let service: mlua::Value = this.get(service_name.clone())?;
            if service == mlua::Value::Nil {
                return Err(mlua::Error::RuntimeError(format!("Service '{}' not found", service_name)));
            }
            Ok(service)
        }).map_err(|e| format!("Failed to create GetService: {}", e))?;
        game.set("GetService", get_service)
            .map_err(|e| format!("Failed to set GetService: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // DataStoreService + Debris (data services)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_data_services(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P2: DataStoreService
        // ====================================================================
        let datastore_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create DataStoreService table: {}", error))?;

        // DataStoreService:GetDataStore(name, scope?) -> DataStore
        let get_datastore = lua.create_function(|lua, (name, scope): (String, Option<String>)| {
            let store = lua.create_table()?;
            let full_name = match scope {
                Some(s) => format!("{}_{}", name, s),
                None => name,
            };
            store.set("_name", full_name.clone())?;
            store.set("_cache", lua.create_table()?)?;
            
            // GetAsync
            store.set("GetAsync", lua.create_function(|_, (this, key): (mlua::Table, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                let value: Option<String> = cache.get(key)?;
                Ok(value)
            })?)?;
            
            // SetAsync
            store.set("SetAsync", lua.create_function(|_, (this, key, value): (mlua::Table, String, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                cache.set(key, value)?;
                Ok(())
            })?)?;
            
            // RemoveAsync
            store.set("RemoveAsync", lua.create_function(|_, (this, key): (mlua::Table, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                let old: Option<String> = cache.get(key.clone())?;
                cache.set(key, mlua::Value::Nil)?;
                Ok(old)
            })?)?;
            
            // IncrementAsync
            store.set("IncrementAsync", lua.create_function(|_, (this, key, delta): (mlua::Table, String, i64)| {
                let cache: mlua::Table = this.get("_cache")?;
                let current: i64 = cache.get::<Option<i64>>(key.clone())?.unwrap_or(0);
                let new_value = current + delta;
                cache.set(key, new_value)?;
                Ok(new_value)
            })?)?;
            
            Ok(store)
        }).map_err(|error| format!("Failed to create GetDataStore: {}", error))?;
        datastore_service_table.set("GetDataStore", get_datastore)
            .map_err(|error| format!("Failed to set GetDataStore: {}", error))?;

        // DataStoreService:GetOrderedDataStore(name, scope?) -> OrderedDataStore
        let get_ordered = lua.create_function(|lua, (name, scope): (String, Option<String>)| {
            let store = lua.create_table()?;
            let full_name = match scope {
                Some(s) => format!("{}_{}", name, s),
                None => name,
            };
            store.set("_name", full_name)?;
            store.set("_entries", lua.create_table()?)?;
            
            // SetAsync
            store.set("SetAsync", lua.create_function(|_, (this, key, value): (mlua::Table, String, i64)| {
                let entries: mlua::Table = this.get("_entries")?;
                entries.set(key, value)?;
                Ok(())
            })?)?;
            
            // GetSortedAsync
            store.set("GetSortedAsync", lua.create_function(|lua, (this, ascending, page_size): (mlua::Table, bool, i64)| {
                let entries: mlua::Table = this.get("_entries")?;
                let mut items: Vec<(String, i64)> = Vec::new();
                
                for pair in entries.pairs::<String, i64>() {
                    if let Ok((k, v)) = pair {
                        items.push((k, v));
                    }
                }
                
                if ascending {
                    items.sort_by(|a, b| a.1.cmp(&b.1));
                } else {
                    items.sort_by(|a, b| b.1.cmp(&a.1));
                }
                
                items.truncate(page_size as usize);
                
                let result = lua.create_table()?;
                for (i, (key, value)) in items.into_iter().enumerate() {
                    let entry = lua.create_table()?;
                    entry.set("key", key)?;
                    entry.set("value", value)?;
                    result.set(i + 1, entry)?;
                }
                
                Ok(result)
            })?)?;
            
            Ok(store)
        }).map_err(|error| format!("Failed to create GetOrderedDataStore: {}", error))?;
        datastore_service_table.set("GetOrderedDataStore", get_ordered)
            .map_err(|error| format!("Failed to set GetOrderedDataStore: {}", error))?;

        globals.set("DataStoreService", datastore_service_table)
            .map_err(|error| format!("Failed to set DataStoreService: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // HttpService — HTTP requests, JSON, GUID, URL encoding
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_http_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P2: HttpService — Full Roblox Parity
        // ====================================================================
        let http_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create HttpService table: {}", error))?;

        // HttpService:GetAsync(url) -> string?
        let http_get = lua.create_function(|_, url: String| {
            match ureq::get(&url).call() {
                Ok(response) => Ok(response.into_string().ok()),
                Err(_) => Ok(None),
            }
        }).map_err(|error| format!("Failed to create HttpService:GetAsync: {}", error))?;
        http_service_table.set("GetAsync", http_get)
            .map_err(|error| format!("Failed to set HttpService.GetAsync: {}", error))?;

        // HttpService:PostAsync(url, body) -> string?
        let http_post = lua.create_function(|_, (url, body): (String, String)| {
            match ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body)
            {
                Ok(response) => Ok(response.into_string().ok()),
                Err(_) => Ok(None),
            }
        }).map_err(|error| format!("Failed to create HttpService:PostAsync: {}", error))?;
        http_service_table.set("PostAsync", http_post)
            .map_err(|error| format!("Failed to set HttpService.PostAsync: {}", error))?;

        // HttpService:RequestAsync(options) -> {Success, StatusCode, StatusMessage, Headers, Body}
        let http_request = lua.create_function(|lua, options: mlua::Table| {
            let url: String = options.get("Url")?;
            let method: String = options.get::<Option<String>>("Method")?.unwrap_or_else(|| "GET".to_string());
            let body: Option<String> = options.get("Body")?;
            let headers: Option<mlua::Table> = options.get("Headers")?;
            
            let mut request = match method.to_uppercase().as_str() {
                "GET" => ureq::get(&url),
                "POST" => ureq::post(&url),
                "PUT" => ureq::put(&url),
                "DELETE" => ureq::delete(&url),
                "PATCH" => ureq::patch(&url),
                "HEAD" => ureq::head(&url),
                _ => ureq::get(&url),
            };
            
            // Apply custom headers
            if let Some(hdrs) = headers {
                for pair in hdrs.pairs::<String, String>() {
                    if let Ok((key, value)) = pair {
                        request = request.set(&key, &value);
                    }
                }
            }
            
            // Set default content-type for body requests
            if body.is_some() {
                request = request.set("Content-Type", "application/json");
            }
            
            let result = match &body {
                Some(b) => request.send_string(b),
                None => request.call(),
            };
            
            let response_table = lua.create_table()?;
            
            match result {
                Ok(response) => {
                    let status = response.status();
                    response_table.set("Success", status >= 200 && status < 300)?;
                    response_table.set("StatusCode", status as i64)?;
                    response_table.set("StatusMessage", response.status_text())?;
                    
                    let headers_table = lua.create_table()?;
                    for name in response.headers_names() {
                        if let Some(value) = response.header(&name) {
                            headers_table.set(name, value)?;
                        }
                    }
                    response_table.set("Headers", headers_table)?;
                    response_table.set("Body", response.into_string().unwrap_or_default())?;
                }
                Err(ureq::Error::Status(code, response)) => {
                    response_table.set("Success", false)?;
                    response_table.set("StatusCode", code as i64)?;
                    response_table.set("StatusMessage", response.status_text())?;
                    response_table.set("Headers", lua.create_table()?)?;
                    response_table.set("Body", response.into_string().unwrap_or_default())?;
                }
                Err(_) => {
                    response_table.set("Success", false)?;
                    response_table.set("StatusCode", 0)?;
                    response_table.set("StatusMessage", "Connection failed")?;
                    response_table.set("Headers", lua.create_table()?)?;
                    response_table.set("Body", "")?;
                }
            }
            
            Ok(response_table)
        }).map_err(|error| format!("Failed to create RequestAsync: {}", error))?;
        http_service_table.set("RequestAsync", http_request)
            .map_err(|error| format!("Failed to set RequestAsync: {}", error))?;

        // HttpService:UrlEncode(input) -> string
        let url_encode = lua.create_function(|_, input: String| {
            let mut encoded = String::with_capacity(input.len() * 3);
            for byte in input.bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        encoded.push(byte as char);
                    }
                    _ => {
                        encoded.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
            Ok(encoded)
        }).map_err(|error| format!("Failed to create UrlEncode: {}", error))?;
        http_service_table.set("UrlEncode", url_encode)
            .map_err(|error| format!("Failed to set UrlEncode: {}", error))?;

        // HttpService:GenerateGUID(wrapInCurlyBraces) -> string
        let generate_guid = lua.create_function(|_, wrap: Option<bool>| {
            let uuid = uuid::Uuid::new_v4();
            if wrap.unwrap_or(true) {
                Ok(format!("{{{}}}", uuid))
            } else {
                Ok(uuid.to_string())
            }
        }).map_err(|error| format!("Failed to create GenerateGUID: {}", error))?;
        http_service_table.set("GenerateGUID", generate_guid)
            .map_err(|error| format!("Failed to set GenerateGUID: {}", error))?;

        // HttpService:JSONEncode(value) -> string
        let json_encode = lua.create_function(|_, value: String| {
            Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        }).map_err(|error| format!("Failed to create JSONEncode: {}", error))?;
        http_service_table.set("JSONEncode", json_encode)
            .map_err(|error| format!("Failed to set JSONEncode: {}", error))?;

        // HttpService:JSONDecode(json) -> string?
        let json_decode = lua.create_function(|_, json: String| {
            let trimmed = json.trim();
            if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
                Ok(Some(trimmed[1..trimmed.len()-1].to_string()))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }).map_err(|error| format!("Failed to create JSONDecode: {}", error))?;
        http_service_table.set("JSONDecode", json_decode)
            .map_err(|error| format!("Failed to set JSONDecode: {}", error))?;

        globals.set("HttpService", http_service_table)
            .map_err(|error| format!("Failed to set HttpService: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // CollectionService (Tags)
    //
    // Tags live on each instance's own `_tags` table (set-style: `_tags[tag] = true`).
    // The drain phase walks every instance in `__INSTANCE_REGISTRY__`, collects
    // `_tags` into a `Vec<String>`, and the spawner persists that to the entity's
    // `_instance.toml` `tags = [...]` array. Bevy then hydrates the ECS
    // [`Tags`](crate::attributes::Tags) component from the TOML — the exact same
    // path the MCP `add_tag` / `get_tagged_entities` tools use, so script-set
    // tags are queryable across systems instead of dying with the VM.
    //
    // `GetTagged` also consults `__EXISTING_TAGS__` — a per-frame snapshot of
    // the engine's ECS Tags injected before each script run — so scripts can
    // query tags placed by the engine, by MCP, or by previous script runs.
    //
    // API surface accepts both Roblox-style method calls (`Service:Foo(arg)`,
    // which passes `self` first) and dot-style calls. Each function checks the
    // first arg's shape: a Lua table is treated as an Instance; an integer is
    // treated as a numeric entity-id; a string for `GetTagged` is the tag.
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_collection_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        let collection_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create CollectionService table: {}", error))?;

        // `__EXISTING_TAGS__` is a `{ tag -> {entity_id_1, entity_id_2, ...} }`
        // table that the engine populates before each script run from the
        // live ECS Tags. Scripts can query, but writes here do not flow back
        // to ECS — script-spawned tags persist via the drain path described
        // above. Initialised here so `GetTagged` doesn't error when the
        // engine hasn't seeded it yet (e.g. tests).
        let existing_present = globals.get::<mlua::Value>("__EXISTING_TAGS__")
            .map(|v| !matches!(v, mlua::Value::Nil))
            .unwrap_or(false);
        if !existing_present {
            let existing = lua.create_table()
                .map_err(|error| format!("Failed to create existing tags table: {}", error))?;
            globals.set("__EXISTING_TAGS__", existing)
                .map_err(|error| format!("Failed to set existing tags table: {}", error))?;
        }

        // Helper: parse the first argument as either a Lua table (Instance,
        // with `_entityId` and `_tags`), an integer (numeric id; resolved
        // back to a registry table when possible), or a `nil`/missing arg
        // that means "this was called dot-style without self". Returns the
        // instance table to operate on, or `None` if the argument was an
        // unbound integer id (no registry entry — caller chooses what to do).
        fn resolve_instance(
            lua: &mlua::Lua,
            first: mlua::Value,
        ) -> mlua::Result<Option<mlua::Table>> {
            match first {
                mlua::Value::Table(t) => {
                    // Could be `self` (the CollectionService table itself —
                    // detected by absence of `_entityId`) or the Instance.
                    if t.contains_key("_entityId")? {
                        Ok(Some(t))
                    } else {
                        Ok(None) // self table; caller will read remaining args
                    }
                }
                mlua::Value::Integer(id) => {
                    let registry: mlua::Table = lua.globals().get("__INSTANCE_REGISTRY__")?;
                    Ok(registry.get::<Option<mlua::Table>>(id)?)
                }
                _ => Ok(None),
            }
        }

        // Helper closure: from a starting `iter`, resolve the (instance, tag)
        // pair handling both `Service:Foo(inst, tag)` (passes self first) and
        // `Service.Foo(inst, tag)` (no self) call styles.
        fn parse_inst_and_tag(
            lua: &mlua::Lua,
            args: mlua::MultiValue,
            fn_name: &str,
        ) -> mlua::Result<(mlua::Table, String)> {
            let mut iter = args.into_iter();
            let first = iter.next().unwrap_or(mlua::Value::Nil);
            let inst_opt = resolve_instance(lua, first)?;
            let instance = if let Some(inst) = inst_opt {
                inst
            } else {
                let second = iter.next().unwrap_or(mlua::Value::Nil);
                resolve_instance(lua, second)?.ok_or_else(|| mlua::Error::RuntimeError(
                    format!("{}: instance must be an Instance table or numeric id", fn_name)))?
            };
            let tag = match iter.next() {
                Some(mlua::Value::String(s)) => s.to_str()?.to_string(),
                _ => return Err(mlua::Error::RuntimeError(
                    format!("{}: missing tag (string)", fn_name))),
            };
            Ok((instance, tag))
        }

        // CollectionService:AddTag(instance, tag) -- method or dot style
        let add_tag = lua.create_function(|lua, args: mlua::MultiValue| {
            let (instance, tag) = parse_inst_and_tag(lua, args, "CollectionService:AddTag")?;
            // Lookup-or-create the per-instance `_tags` set.
            let tags: mlua::Table = match instance.get::<Option<mlua::Table>>("_tags")? {
                Some(t) => t,
                None => {
                    let t = lua.create_table()?;
                    instance.set("_tags", t.clone())?;
                    t
                }
            };
            tags.set(tag, true)?;
            Ok(())
        }).map_err(|error| format!("Failed to create AddTag: {}", error))?;
        collection_service_table.set("AddTag", add_tag)
            .map_err(|error| format!("Failed to set AddTag: {}", error))?;

        // CollectionService:RemoveTag(instance, tag)
        let remove_tag = lua.create_function(|lua, args: mlua::MultiValue| {
            let (instance, tag) = parse_inst_and_tag(lua, args, "CollectionService:RemoveTag")?;
            if let Some(tags) = instance.get::<Option<mlua::Table>>("_tags")? {
                tags.set(tag, mlua::Value::Nil)?;
            }
            Ok(())
        }).map_err(|error| format!("Failed to create RemoveTag: {}", error))?;
        collection_service_table.set("RemoveTag", remove_tag)
            .map_err(|error| format!("Failed to set RemoveTag: {}", error))?;

        // CollectionService:HasTag(instance, tag) -> bool
        let has_tag = lua.create_function(|lua, args: mlua::MultiValue| {
            let (instance, tag) = parse_inst_and_tag(lua, args, "CollectionService:HasTag")?;
            if let Some(tags) = instance.get::<Option<mlua::Table>>("_tags")? {
                let has: bool = tags.get::<Option<bool>>(tag)?.unwrap_or(false);
                Ok(has)
            } else {
                Ok(false)
            }
        }).map_err(|error| format!("Failed to create HasTag: {}", error))?;
        collection_service_table.set("HasTag", has_tag)
            .map_err(|error| format!("Failed to set HasTag: {}", error))?;

        // CollectionService:GetTagged(tag) -> {instances}
        // Returns matching Instance tables from the script's registry PLUS
        // numeric entity-ids from `__EXISTING_TAGS__` (engine ECS snapshot).
        // Numeric ids are appended after script-side instances so the
        // first elements are always usable as Instance tables.
        let get_tagged = lua.create_function(|lua, args: mlua::MultiValue| {
            // Sniff for the leading self table.
            let mut iter = args.into_iter();
            let first = iter.next().unwrap_or(mlua::Value::Nil);
            let tag: String = match first {
                mlua::Value::String(s) => s.to_str()?.to_string(),
                mlua::Value::Table(_) => {
                    // method-style; real tag is next arg
                    let next = iter.next().unwrap_or(mlua::Value::Nil);
                    if let mlua::Value::String(s) = next {
                        s.to_str()?.to_string()
                    } else {
                        return Err(mlua::Error::RuntimeError(
                            "CollectionService:GetTagged: tag must be a string".into()));
                    }
                }
                _ => return Err(mlua::Error::RuntimeError(
                    "CollectionService:GetTagged: tag must be a string".into())),
            };

            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            let result = lua.create_table()?;
            let mut index = 1i64;

            // Script-side matches first.
            for pair in registry.pairs::<i64, mlua::Table>() {
                let Ok((_id, inst)) = pair else { continue };
                if let Some(tags) = inst.get::<Option<mlua::Table>>("_tags")? {
                    if tags.get::<Option<bool>>(tag.clone())?.unwrap_or(false) {
                        result.set(index, inst)?;
                        index += 1;
                    }
                }
            }

            // Engine-side matches from the pre-script snapshot. Returned
            // as integer entity-ids — scripts that need richer behaviour
            // can pass these to engine-bound APIs (e.g. future ECS-backed
            // helpers); for `for ... in ipairs` iteration both shapes
            // coexist in the same returned table.
            if let Ok(existing) = globals.get::<mlua::Table>("__EXISTING_TAGS__") {
                if let Ok(ids) = existing.get::<mlua::Table>(tag) {
                    for pair in ids.pairs::<i64, i64>() {
                        let Ok((_k, id)) = pair else { continue };
                        result.set(index, id)?;
                        index += 1;
                    }
                }
            }

            Ok(result)
        }).map_err(|error| format!("Failed to create GetTagged: {}", error))?;
        collection_service_table.set("GetTagged", get_tagged)
            .map_err(|error| format!("Failed to set GetTagged: {}", error))?;

        globals.set("CollectionService", collection_service_table.clone())
            .map_err(|error| format!("Failed to set CollectionService: {}", error))?;

        // Also register on the `game` table so `game:GetService("CollectionService")`
        // works — Roblox-parity entry point that previously errored
        // `Service 'CollectionService' not found`. The two routes share the
        // same backing table, so AddTag/GetTagged behaviour is identical.
        if let Ok(game) = globals.get::<mlua::Table>("game") {
            game.set("CollectionService", collection_service_table)
                .map_err(|error| format!("Failed to set game.CollectionService: {}", error))?;
        }

        Ok(())
    }

    // ========================================================================
    // SoundService
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_sound_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P2: SoundService / Sound
        // ====================================================================
        let sound_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create SoundService table: {}", error))?;

        // SoundService:PlayLocalSound(sound)
        let play_local = lua.create_function(|_, sound: mlua::Table| {
            let sound_id: String = sound.get::<Option<String>>("SoundId")?.unwrap_or_default();
            tracing::info!("[Luau Sound] Playing: {}", sound_id);
            sound.set("Playing", true)?;
            Ok(())
        }).map_err(|error| format!("Failed to create PlayLocalSound: {}", error))?;
        sound_service_table.set("PlayLocalSound", play_local)
            .map_err(|error| format!("Failed to set PlayLocalSound: {}", error))?;

        globals.set("SoundService", sound_service_table)
            .map_err(|error| format!("Failed to set SoundService: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // Camera API (workspace.CurrentCamera)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_camera_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P3: Camera API (workspace.CurrentCamera)
        // ====================================================================
        let camera_table = lua.create_table()
            .map_err(|error| format!("Failed to create Camera table: {}", error))?;

        // Camera.CFrame — current camera position/orientation
        let camera_cframe = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Camera.CFrame: {}", error))?;
        camera_table.set("CFrame", camera_cframe)
            .map_err(|error| format!("Failed to set Camera.CFrame: {}", error))?;

        // Camera.FieldOfView — field of view in degrees
        camera_table.set("FieldOfView", 70.0f64)
            .map_err(|error| format!("Failed to set Camera.FieldOfView: {}", error))?;

        // Camera.CameraType — "Custom", "Scriptable", "Follow", etc.
        camera_table.set("CameraType", "Custom")
            .map_err(|error| format!("Failed to set Camera.CameraType: {}", error))?;

        // Camera.CameraSubject — the object the camera follows (nil by default)
        camera_table.set("CameraSubject", mlua::Value::Nil)
            .map_err(|error| format!("Failed to set Camera.CameraSubject: {}", error))?;

        // Camera.Focus — focus point CFrame
        let focus_cframe = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Camera.Focus: {}", error))?;
        camera_table.set("Focus", focus_cframe)
            .map_err(|error| format!("Failed to set Camera.Focus: {}", error))?;

        // Camera.ViewportSize — Vector2 of viewport dimensions
        let viewport_size = lua.create_table()
            .map_err(|error| format!("Failed to create ViewportSize: {}", error))?;
        viewport_size.set("X", 1920.0f64)
            .map_err(|error| format!("Failed to set ViewportSize.X: {}", error))?;
        viewport_size.set("Y", 1080.0f64)
            .map_err(|error| format!("Failed to set ViewportSize.Y: {}", error))?;
        camera_table.set("ViewportSize", viewport_size)
            .map_err(|error| format!("Failed to set Camera.ViewportSize: {}", error))?;

        // Camera:WorldToScreenPoint(worldPoint) -> Vector3, bool
        let world_to_screen = lua.create_function(|lua, point: super::types::LuauVector3| {
            // TODO: Wire to actual camera projection
            let result = lua.create_table()?;
            result.set("X", point.0.x)?;
            result.set("Y", point.0.y)?;
            result.set("Z", point.0.z)?;
            Ok((result, true)) // (screenPoint, onScreen)
        }).map_err(|error| format!("Failed to create WorldToScreenPoint: {}", error))?;
        camera_table.set("WorldToScreenPoint", world_to_screen)
            .map_err(|error| format!("Failed to set WorldToScreenPoint: {}", error))?;

        // Camera:ScreenPointToRay(x, y, depth) -> Ray
        let screen_to_ray = lua.create_function(|lua, (x, y, _depth): (f64, f64, Option<f64>)| {
            // TODO: Wire to actual camera unprojection
            let ray = lua.create_table()?;
            let origin = lua.create_table()?;
            origin.set("X", 0.0f64)?;
            origin.set("Y", 0.0f64)?;
            origin.set("Z", 0.0f64)?;
            let direction = lua.create_table()?;
            direction.set("X", x / 1920.0)?;
            direction.set("Y", y / 1080.0)?;
            direction.set("Z", 1.0f64)?;
            ray.set("Origin", origin)?;
            ray.set("Direction", direction)?;
            Ok(ray)
        }).map_err(|error| format!("Failed to create ScreenPointToRay: {}", error))?;
        camera_table.set("ScreenPointToRay", screen_to_ray)
            .map_err(|error| format!("Failed to set ScreenPointToRay: {}", error))?;

        // Camera:ViewportPointToRay(x, y, depth) -> Ray
        let viewport_to_ray = lua.create_function(|lua, (x, y, _depth): (f64, f64, Option<f64>)| {
            let ray = lua.create_table()?;
            let origin = lua.create_table()?;
            origin.set("X", 0.0f64)?;
            origin.set("Y", 0.0f64)?;
            origin.set("Z", 0.0f64)?;
            let direction = lua.create_table()?;
            direction.set("X", x)?;
            direction.set("Y", y)?;
            direction.set("Z", 1.0f64)?;
            ray.set("Origin", origin)?;
            ray.set("Direction", direction)?;
            Ok(ray)
        }).map_err(|error| format!("Failed to create ViewportPointToRay: {}", error))?;
        camera_table.set("ViewportPointToRay", viewport_to_ray)
            .map_err(|error| format!("Failed to set ViewportPointToRay: {}", error))?;

        // Set workspace.CurrentCamera
        let workspace: mlua::Table = globals.get("workspace")
            .map_err(|error| format!("Failed to get workspace: {}", error))?;
        workspace.set("CurrentCamera", camera_table)
            .map_err(|error| format!("Failed to set workspace.CurrentCamera: {}", error))?;

        Ok(())
    }

    // ========================================================================
    // Mouse API
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_mouse_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P3: Mouse API (game.Players.LocalPlayer:GetMouse())
        // ====================================================================
        let mouse_table = lua.create_table()
            .map_err(|error| format!("Failed to create Mouse table: {}", error))?;

        // Mouse.X, Mouse.Y — current position
        mouse_table.set("X", 0.0f64)
            .map_err(|error| format!("Failed to set Mouse.X: {}", error))?;
        mouse_table.set("Y", 0.0f64)
            .map_err(|error| format!("Failed to set Mouse.Y: {}", error))?;

        // Mouse.Hit — CFrame of where mouse ray intersects world
        let mouse_hit = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Mouse.Hit: {}", error))?;
        mouse_table.set("Hit", mouse_hit)
            .map_err(|error| format!("Failed to set Mouse.Hit: {}", error))?;

        // Mouse.Target — Part the mouse is hovering over (nil if none)
        mouse_table.set("Target", mlua::Value::Nil)
            .map_err(|error| format!("Failed to set Mouse.Target: {}", error))?;

        // Mouse.TargetSurface — Enum.NormalId of surface (stub as string)
        mouse_table.set("TargetSurface", "Front")
            .map_err(|error| format!("Failed to set Mouse.TargetSurface: {}", error))?;

        // Mouse.UnitRay — Ray from camera through mouse position
        let unit_ray = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay: {}", error))?;
        let origin = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay.Origin: {}", error))?;
        origin.set("X", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        origin.set("Y", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        origin.set("Z", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        let direction = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay.Direction: {}", error))?;
        direction.set("X", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        direction.set("Y", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        direction.set("Z", 1.0f64).map_err(|e| format!("Failed: {}", e))?;
        unit_ray.set("Origin", origin).map_err(|e| format!("Failed: {}", e))?;
        unit_ray.set("Direction", direction).map_err(|e| format!("Failed: {}", e))?;
        mouse_table.set("UnitRay", unit_ray)
            .map_err(|error| format!("Failed to set Mouse.UnitRay: {}", error))?;

        // Mouse.Icon — cursor icon (string path)
        mouse_table.set("Icon", "")
            .map_err(|error| format!("Failed to set Mouse.Icon: {}", error))?;

        // Store mouse table for LocalPlayer:GetMouse()
        globals.set("_EustressMouse", mouse_table)
            .map_err(|error| format!("Failed to set _EustressMouse: {}", error))?;

        // Also store as global Mouse for compatibility
        let mouse_ref: mlua::Table = globals.get("_EustressMouse")
            .map_err(|e| format!("Failed to get _EustressMouse: {}", e))?;
        globals.set("Mouse", mouse_ref)
            .map_err(|e| format!("Failed to set Mouse: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Animation API (Animator, AnimationTrack)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_animation_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P3: Animation API (Animator, AnimationTrack)
        // ====================================================================
        // In Roblox, animations are loaded via Animator:LoadAnimation(Animation)
        // which returns an AnimationTrack. The track can be played, stopped, etc.
        
        // Create Animator prototype for humanoid.Animator
        let animator_proto = lua.create_table()
            .map_err(|e| format!("Failed to create Animator proto: {}", e))?;
        
        // Animator:LoadAnimation(animation) -> AnimationTrack
        let load_animation = lua.create_function(|lua, (animator, animation): (mlua::Table, mlua::Table)| {
            // Create an AnimationTrack table
            let track = lua.create_table()?;
            
            // Copy animation ID from the Animation instance
            let anim_id: String = animation.get::<Option<String>>("AnimationId")?.unwrap_or_default();
            track.set("Animation", animation)?;
            track.set("_animationId", anim_id)?;
            
            // AnimationTrack properties
            track.set("IsPlaying", false)?;
            track.set("Length", 1.0f64)?;
            track.set("Looped", false)?;
            track.set("Priority", 1i32)?; // Enum.AnimationPriority.Core = 1
            track.set("Speed", 1.0f64)?;
            track.set("TimePosition", 0.0f64)?;
            track.set("WeightCurrent", 0.0f64)?;
            track.set("WeightTarget", 1.0f64)?;
            
            // AnimationTrack:Play(fadeTime, weight, speed)
            track.set("Play", lua.create_function(|_, (this, fade_time, weight, speed): (mlua::Table, Option<f64>, Option<f64>, Option<f64>)| {
                let _fade = fade_time.unwrap_or(0.1);
                let _weight = weight.unwrap_or(1.0);
                let _speed = speed.unwrap_or(1.0);
                this.set("IsPlaying", true)?;
                this.set("WeightTarget", _weight)?;
                this.set("Speed", _speed)?;
                tracing::info!("[Luau Animation] Playing animation");
                Ok(())
            })?)?;
            
            // AnimationTrack:Stop(fadeTime)
            track.set("Stop", lua.create_function(|_, (this, _fade_time): (mlua::Table, Option<f64>)| {
                this.set("IsPlaying", false)?;
                this.set("WeightTarget", 0.0f64)?;
                tracing::info!("[Luau Animation] Stopping animation");
                Ok(())
            })?)?;
            
            // AnimationTrack:AdjustSpeed(speed)
            track.set("AdjustSpeed", lua.create_function(|_, (this, speed): (mlua::Table, f64)| {
                this.set("Speed", speed)?;
                Ok(())
            })?)?;
            
            // AnimationTrack:AdjustWeight(weight, fadeTime)
            track.set("AdjustWeight", lua.create_function(|_, (this, weight, _fade_time): (mlua::Table, f64, Option<f64>)| {
                this.set("WeightTarget", weight)?;
                Ok(())
            })?)?;
            
            // AnimationTrack:GetMarkerReachedSignal(name) -> RBXScriptSignal (stub)
            track.set("GetMarkerReachedSignal", lua.create_function(|lua, (_this, _name): (mlua::Table, String)| {
                // Return a stub signal table
                let signal = lua.create_table()?;
                signal.set("Connect", lua.create_function(|_, (_sig, _callback): (mlua::Table, mlua::Function)| {
                    // Stub: would connect to keyframe marker events
                    Ok(())
                })?)?;
                Ok(signal)
            })?)?;
            
            Ok(track)
        }).map_err(|e| format!("Failed to create LoadAnimation: {}", e))?;
        animator_proto.set("LoadAnimation", load_animation)
            .map_err(|e| format!("Failed to set LoadAnimation: {}", e))?;
        
        // Store animator prototype for Instance system
        globals.set("_EustressAnimatorProto", animator_proto)
            .map_err(|e| format!("Failed to set _EustressAnimatorProto: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Humanoid API (character control)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_humanoid_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // P3: Humanoid API (stub for character control)
        // ====================================================================
        // Humanoid is typically accessed via character.Humanoid
        // Create a prototype table that can be used when creating Humanoid instances
        
        let humanoid_proto = lua.create_table()
            .map_err(|e| format!("Failed to create Humanoid proto: {}", e))?;
        
        // Humanoid properties (defaults)
        humanoid_proto.set("Health", 100.0f64)
            .map_err(|e| format!("Failed to set Health: {}", e))?;
        humanoid_proto.set("MaxHealth", 100.0f64)
            .map_err(|e| format!("Failed to set MaxHealth: {}", e))?;
        humanoid_proto.set("WalkSpeed", 16.0f64)
            .map_err(|e| format!("Failed to set WalkSpeed: {}", e))?;
        humanoid_proto.set("JumpPower", 50.0f64)
            .map_err(|e| format!("Failed to set JumpPower: {}", e))?;
        humanoid_proto.set("JumpHeight", 7.2f64)
            .map_err(|e| format!("Failed to set JumpHeight: {}", e))?;
        humanoid_proto.set("HipHeight", 2.0f64)
            .map_err(|e| format!("Failed to set HipHeight: {}", e))?;
        humanoid_proto.set("AutoRotate", true)
            .map_err(|e| format!("Failed to set AutoRotate: {}", e))?;
        humanoid_proto.set("AutoJumpEnabled", true)
            .map_err(|e| format!("Failed to set AutoJumpEnabled: {}", e))?;
        
        // Humanoid:TakeDamage(amount)
        let take_damage = lua.create_function(|_, (this, amount): (mlua::Table, f64)| {
            let current: f64 = this.get("Health")?;
            let new_health = (current - amount).max(0.0);
            this.set("Health", new_health)?;
            if new_health <= 0.0 {
                tracing::info!("[Luau Humanoid] Character died");
                // TODO: Fire Died event
            }
            Ok(())
        }).map_err(|e| format!("Failed to create TakeDamage: {}", e))?;
        humanoid_proto.set("TakeDamage", take_damage)
            .map_err(|e| format!("Failed to set TakeDamage: {}", e))?;
        
        // Humanoid:MoveTo(position, part)
        let move_to = lua.create_function(|_, (_this, position, _part): (mlua::Table, super::types::LuauVector3, Option<mlua::Value>)| {
            tracing::info!("[Luau Humanoid] MoveTo: {:?}", position.0);
            // TODO: Wire to character controller pathfinding
            Ok(())
        }).map_err(|e| format!("Failed to create MoveTo: {}", e))?;
        humanoid_proto.set("MoveTo", move_to)
            .map_err(|e| format!("Failed to set MoveTo: {}", e))?;
        
        // Humanoid:Move(moveDirection, relativeToCamera)
        let move_fn = lua.create_function(|_, (_this, direction, _relative): (mlua::Table, super::types::LuauVector3, Option<bool>)| {
            tracing::info!("[Luau Humanoid] Move: {:?}", direction.0);
            // TODO: Wire to character controller
            Ok(())
        }).map_err(|e| format!("Failed to create Move: {}", e))?;
        humanoid_proto.set("Move", move_fn)
            .map_err(|e| format!("Failed to set Move: {}", e))?;
        
        // Humanoid:ChangeState(state)
        let change_state = lua.create_function(|_, (_this, state): (mlua::Table, i32)| {
            tracing::info!("[Luau Humanoid] ChangeState: {}", state);
            // TODO: Wire to animation state machine
            Ok(())
        }).map_err(|e| format!("Failed to create ChangeState: {}", e))?;
        humanoid_proto.set("ChangeState", change_state)
            .map_err(|e| format!("Failed to set ChangeState: {}", e))?;
        
        // Humanoid:GetState() -> Enum.HumanoidStateType
        let get_state = lua.create_function(|_, _this: mlua::Table| {
            // Return Running state (8) as default
            Ok(8i32)
        }).map_err(|e| format!("Failed to create GetState: {}", e))?;
        humanoid_proto.set("GetState", get_state)
            .map_err(|e| format!("Failed to set GetState: {}", e))?;
        
        // Store humanoid prototype for Instance system
        globals.set("_EustressHumanoidProto", humanoid_proto)
            .map_err(|e| format!("Failed to set _EustressHumanoidProto: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // MarketplaceService
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_marketplace_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // MarketplaceService — Roblox-compatible marketplace API (Tickets)
        // ====================================================================
        let marketplace_service = lua.create_table()
            .map_err(|e| format!("Failed to create MarketplaceService: {}", e))?;

        // MarketplaceService:PromptPurchase(player, productId)
        marketplace_service.set("PromptPurchase", lua.create_function(|_, (player, product_id): (mlua::Table, i64)| {
            let entity_id: i64 = player.get("_entityId").unwrap_or(0);
            tracing::info!("[Luau] MarketplaceService:PromptPurchase({}, {})", entity_id, product_id);
            // TODO: fire PromptPurchaseEvent via EventBus
            Ok(true)
        }).map_err(|e| format!("PromptPurchase: {}", e))?)
            .map_err(|e| format!("set PromptPurchase: {}", e))?;

        // MarketplaceService:GetProductInfo(productId)
        marketplace_service.set("GetProductInfo", lua.create_function(|lua, product_id: i64| {
            let info = lua.create_table()?;
            info.set("ProductId", product_id)?;
            info.set("Name", "")?;
            info.set("Description", "")?;
            info.set("PriceInTickets", 0)?;
            info.set("IsForSale", false)?;
            info.set("ProductType", "DeveloperProduct")?;
            // TODO: populate from MarketplaceService bridge
            Ok(info)
        }).map_err(|e| format!("GetProductInfo: {}", e))?)
            .map_err(|e| format!("set GetProductInfo: {}", e))?;

        // MarketplaceService:PlayerOwnsGamePass(player, passId)
        marketplace_service.set("PlayerOwnsGamePass", lua.create_function(|_, (_player, _pass_id): (mlua::Table, i64)| {
            // TODO: check via MarketplaceService bridge
            Ok(false)
        }).map_err(|e| format!("PlayerOwnsGamePass: {}", e))?)
            .map_err(|e| format!("set PlayerOwnsGamePass: {}", e))?;

        // MarketplaceService:GetTicketBalance(player)
        marketplace_service.set("GetTicketBalance", lua.create_function(|_, _player: mlua::Table| {
            // TODO: read from bridge
            Ok(0i64)
        }).map_err(|e| format!("GetTicketBalance: {}", e))?)
            .map_err(|e| format!("set GetTicketBalance: {}", e))?;

        // MarketplaceService.PromptPurchaseFinished (signal stub)
        let pf_signal = lua.create_table()
            .map_err(|e| format!("PromptPurchaseFinished table: {}", e))?;
        pf_signal.set("Connect", lua.create_function(|_, (_self_table, _callback): (mlua::Table, mlua::Function)| {
            tracing::info!("[Luau] MarketplaceService.PromptPurchaseFinished:Connect()");
            Ok(())
        }).map_err(|e| format!("PF Connect: {}", e))?)
            .map_err(|e| format!("set PF Connect: {}", e))?;
        marketplace_service.set("PromptPurchaseFinished", pf_signal)
            .map_err(|e| format!("set PromptPurchaseFinished: {}", e))?;

        globals.set("MarketplaceService", marketplace_service)
            .map_err(|e| format!("Failed to set MarketplaceService: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // SimulationService — read/write watchpoint values (bridge with MCP tools)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_simulation_service(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // SimulationService — read/write watchpoint values (bridge with MCP tools)
        // ====================================================================
        let sim_service = lua.create_table()
            .map_err(|e| format!("Failed to create SimulationService: {}", e))?;

        // SimulationService:GetValue(key) -> number
        sim_service.set("GetValue", lua.create_function(|_, key: String| {
            // Reads from shared thread-local sim values
            Ok(0.0f64) // TODO: bridge with SIM_VALUES thread-local
        }).map_err(|e| format!("GetValue: {}", e))?)
            .map_err(|e| format!("set GetValue: {}", e))?;

        // SimulationService:SetValue(key, value)
        sim_service.set("SetValue", lua.create_function(|_, (key, value): (String, f64)| {
            tracing::debug!("[Luau] SimulationService:SetValue({}, {})", key, value);
            // TODO: bridge with SIM_VALUES thread-local
            Ok(())
        }).map_err(|e| format!("SetValue: {}", e))?)
            .map_err(|e| format!("set SetValue: {}", e))?;

        // SimulationService:ListValues() -> table
        sim_service.set("ListValues", lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            // TODO: populate from SIM_VALUES
            Ok(t)
        }).map_err(|e| format!("ListValues: {}", e))?)
            .map_err(|e| format!("set ListValues: {}", e))?;

        globals.set("SimulationService", sim_service)
            .map_err(|e| format!("Failed to set SimulationService: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // WorkspaceQuery — entity search + file access (bridge with MCP tools)
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_workspace_query(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // WorkspaceQuery — entity search + file access (bridge with MCP tools)
        // ====================================================================
        let workspace_query = lua.create_table()
            .map_err(|e| format!("Failed to create WorkspaceQuery: {}", e))?;

        // WorkspaceQuery:QueryEntities(classFilter?) -> {{name, class}}
        workspace_query.set("QueryEntities", lua.create_function(|lua, class_filter: Option<String>| {
            let t = lua.create_table()?;
            // File-system query of Workspace — same logic as MCP query_entities tool
            // TODO: bridge with Space root path
            Ok(t)
        }).map_err(|e| format!("QueryEntities: {}", e))?)
            .map_err(|e| format!("set QueryEntities: {}", e))?;

        // WorkspaceQuery:ReadFile(relativePath) -> string
        workspace_query.set("ReadFile", lua.create_function(|_, path: String| {
            if path.contains("..") {
                return Ok(String::new());
            }
            // TODO: bridge with Space root path for sandboxed read
            Ok(String::new())
        }).map_err(|e| format!("ReadFile: {}", e))?)
            .map_err(|e| format!("set ReadFile: {}", e))?;

        // WorkspaceQuery:WriteFile(relativePath, content) -> bool
        workspace_query.set("WriteFile", lua.create_function(|_, (path, content): (String, String)| {
            if path.contains("..") {
                return Ok(false);
            }
            // TODO: bridge with Space root path for sandboxed write
            tracing::debug!("[Luau] WorkspaceQuery:WriteFile({}, {} bytes)", path, content.len());
            Ok(false) // stub until bridge is connected
        }).map_err(|e| format!("WriteFile: {}", e))?)
            .map_err(|e| format!("set WriteFile: {}", e))?;

        // WorkspaceQuery:QueryMaterial(materialName) -> {roughness, metallic, reflectance}
        workspace_query.set("QueryMaterial", lua.create_function(|lua, material_name: String| {
            let mat = crate::classes::Material::from_string(&material_name);
            let (roughness, metallic, reflectance) = mat.pbr_params();
            let t = lua.create_table()?;
            t.set("roughness", roughness as f64)?;
            t.set("metallic", metallic as f64)?;
            t.set("reflectance", reflectance as f64)?;
            Ok(t)
        }).map_err(|e| format!("QueryMaterial: {}", e))?)
            .map_err(|e| format!("set QueryMaterial: {}", e))?;

        globals.set("WorkspaceQuery", workspace_query)
            .map_err(|e| format!("Failed to set WorkspaceQuery: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Spatial Queries — workspace:GetPartBoundsInBox/Radius, Blockcast, etc.
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_spatial_queries(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();
        let workspace: mlua::Table = globals.get("workspace")
            .map_err(|e| format!("Failed to get workspace for spatial queries: {}", e))?;

        // Helper: collect all BasePart instances from the registry
        // Returns Vec of (entity_id, instance_table) for parts only
        fn is_base_part(class: &str) -> bool {
            matches!(class, "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" | "SpawnLocation" | "Seat")
        }

        // workspace:GetPartBoundsInBox(cframe, size, overlapParams?) -> {BasePart}
        // Returns all BaseParts whose bounding box overlaps the query box
        let get_in_box = lua.create_function(|lua, (cframe, size, _params): (mlua::Value, mlua::Value, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            let result = lua.create_table()?;
            let mut idx = 1i64;

            // Extract query box center from CFrame userdata
            let (qx, qy, qz) = if let mlua::Value::UserData(ref ud) = cframe {
                if let Ok(cf) = ud.borrow::<super::types::LuauCFrame>() {
                    (cf.0.position.x, cf.0.position.y, cf.0.position.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            // Extract query half-extents from Size Vector3
            let (hx, hy, hz) = if let mlua::Value::UserData(ref ud) = size {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            // AABB overlap test against each BasePart
            for pair in registry.pairs::<i64, mlua::Table>() {
                if let Ok((_, inst)) = pair {
                    let class: String = inst.raw_get("_className").unwrap_or_default();
                    if !is_base_part(&class) { continue; }

                    // Part position
                    let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                        if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                            (v.0.x, v.0.y, v.0.z)
                        } else { continue; }
                    } else { continue; };

                    // Part half-size
                    let (sx, sy, sz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Size") {
                        if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                            (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                        } else { (2.0, 0.5, 1.0) }
                    } else { (2.0, 0.5, 1.0) };

                    // AABB overlap check
                    if (qx - hx) <= (px + sx) && (qx + hx) >= (px - sx)
                        && (qy - hy) <= (py + sy) && (qy + hy) >= (py - sy)
                        && (qz - hz) <= (pz + sz) && (qz + hz) >= (pz - sz)
                    {
                        result.set(idx, inst)?;
                        idx += 1;
                    }
                }
            }
            Ok(result)
        }).map_err(|e| format!("Failed to create GetPartBoundsInBox: {}", e))?;
        workspace.set("GetPartBoundsInBox", get_in_box)
            .map_err(|e| format!("Failed to set GetPartBoundsInBox: {}", e))?;

        // workspace:GetPartBoundsInRadius(position, radius, overlapParams?) -> {BasePart}
        let get_in_radius = lua.create_function(|lua, (position, radius, _params): (mlua::Value, f64, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            let result = lua.create_table()?;
            let mut idx = 1i64;

            let (qx, qy, qz) = if let mlua::Value::UserData(ref ud) = position {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            let radius_sq = radius * radius;

            for pair in registry.pairs::<i64, mlua::Table>() {
                if let Ok((_, inst)) = pair {
                    let class: String = inst.raw_get("_className").unwrap_or_default();
                    if !is_base_part(&class) { continue; }

                    let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                        if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                            (v.0.x, v.0.y, v.0.z)
                        } else { continue; }
                    } else { continue; };

                    let dx = px - qx;
                    let dy = py - qy;
                    let dz = pz - qz;
                    let dist_sq = dx * dx + dy * dy + dz * dz;

                    if dist_sq <= radius_sq {
                        result.set(idx, inst)?;
                        idx += 1;
                    }
                }
            }
            Ok(result)
        }).map_err(|e| format!("Failed to create GetPartBoundsInRadius: {}", e))?;
        workspace.set("GetPartBoundsInRadius", get_in_radius)
            .map_err(|e| format!("Failed to set GetPartBoundsInRadius: {}", e))?;

        // workspace:GetPartsInPart(part, overlapParams?) -> {BasePart}
        // Returns all BaseParts overlapping the given part's bounding box
        let get_parts_in_part = lua.create_function(|lua, (part, _params): (mlua::Table, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            let result = lua.create_table()?;
            let mut idx = 1i64;

            let part_id: i64 = part.raw_get("_entityId").unwrap_or(0);

            // Get query part position and half-size
            let (qx, qy, qz) = if let Ok(mlua::Value::UserData(ud)) = part.raw_get::<mlua::Value>("Position") {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { return Ok(result); }
            } else { return Ok(result); };

            let (hx, hy, hz) = if let Ok(mlua::Value::UserData(ud)) = part.raw_get::<mlua::Value>("Size") {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                } else { (2.0, 0.5, 1.0) }
            } else { (2.0, 0.5, 1.0) };

            for pair in registry.pairs::<i64, mlua::Table>() {
                if let Ok((eid, inst)) = pair {
                    if eid == part_id { continue; } // Skip self
                    let class: String = inst.raw_get("_className").unwrap_or_default();
                    if !is_base_part(&class) { continue; }

                    let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                        if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                            (v.0.x, v.0.y, v.0.z)
                        } else { continue; }
                    } else { continue; };

                    let (sx, sy, sz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Size") {
                        if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                            (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                        } else { (2.0, 0.5, 1.0) }
                    } else { (2.0, 0.5, 1.0) };

                    // AABB overlap
                    if (qx - hx) <= (px + sx) && (qx + hx) >= (px - sx)
                        && (qy - hy) <= (py + sy) && (qy + hy) >= (py - sy)
                        && (qz - hz) <= (pz + sz) && (qz + hz) >= (pz - sz)
                    {
                        result.set(idx, inst)?;
                        idx += 1;
                    }
                }
            }
            Ok(result)
        }).map_err(|e| format!("Failed to create GetPartsInPart: {}", e))?;
        workspace.set("GetPartsInPart", get_parts_in_part)
            .map_err(|e| format!("Failed to set GetPartsInPart: {}", e))?;

        // workspace:Blockcast(cframe, size, direction, params?) -> RaycastResult?
        // Sweeps an AABB along a direction and returns the first hit
        let blockcast = lua.create_function(|lua, (cframe, size, direction, _params): (mlua::Value, mlua::Value, mlua::Value, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;

            // Extract origin from CFrame
            let (ox, oy, oz) = if let mlua::Value::UserData(ref ud) = cframe {
                if let Ok(cf) = ud.borrow::<super::types::LuauCFrame>() {
                    (cf.0.position.x, cf.0.position.y, cf.0.position.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            // Extract half-extents from Size
            let (hx, hy, hz) = if let mlua::Value::UserData(ref ud) = size {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            // Extract direction vector
            let (dx, dy, dz) = if let mlua::Value::UserData(ref ud) = direction {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            let dir_len = (dx * dx + dy * dy + dz * dz).sqrt();
            if dir_len < 1e-10 { return Ok(mlua::Value::Nil); }

            // Simple swept AABB: sample along direction and check overlaps
            let steps = 20i32;
            let step_size = dir_len / steps as f64;

            for step in 0..=steps {
                let t = step as f64 * step_size / dir_len;
                let cx = ox + dx * t;
                let cy = oy + dy * t;
                let cz = oz + dz * t;

                for pair in registry.pairs::<i64, mlua::Table>() {
                    if let Ok((_, inst)) = pair {
                        let class: String = inst.raw_get("_className").unwrap_or_default();
                        if !is_base_part(&class) { continue; }

                        let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x, v.0.y, v.0.z)
                            } else { continue; }
                        } else { continue; };

                        let (sx, sy, sz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Size") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                            } else { (2.0, 0.5, 1.0) }
                        } else { (2.0, 0.5, 1.0) };

                        if (cx - hx) <= (px + sx) && (cx + hx) >= (px - sx)
                            && (cy - hy) <= (py + sy) && (cy + hy) >= (py - sy)
                            && (cz - hz) <= (pz + sz) && (cz + hz) >= (pz - sz)
                        {
                            let hit = lua.create_table()?;
                            hit.set("Instance", inst)?;
                            hit.set("Position", lua.create_userdata(
                                super::types::LuauVector3::new(cx, cy, cz)
                            )?)?;
                            hit.set("Normal", lua.create_userdata(
                                super::types::LuauVector3::new(0.0, 1.0, 0.0)
                            )?)?;
                            hit.set("Distance", t * dir_len)?;
                            hit.set("Material", "Plastic")?;
                            return Ok(mlua::Value::Table(hit));
                        }
                    }
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create Blockcast: {}", e))?;
        workspace.set("Blockcast", blockcast)
            .map_err(|e| format!("Failed to set Blockcast: {}", e))?;

        // workspace:Spherecast(position, radius, direction, params?) -> RaycastResult?
        let spherecast = lua.create_function(|lua, (position, radius, direction, _params): (mlua::Value, f64, mlua::Value, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;

            let (ox, oy, oz) = if let mlua::Value::UserData(ref ud) = position {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            let (dx, dy, dz) = if let mlua::Value::UserData(ref ud) = direction {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            let dir_len = (dx * dx + dy * dy + dz * dz).sqrt();
            if dir_len < 1e-10 { return Ok(mlua::Value::Nil); }

            // Sphere sweep: sample along ray, check sphere-AABB overlap
            let steps = 20i32;
            for step in 0..=steps {
                let t = step as f64 / steps as f64;
                let cx = ox + dx * t;
                let cy = oy + dy * t;
                let cz = oz + dz * t;

                for pair in registry.pairs::<i64, mlua::Table>() {
                    if let Ok((_, inst)) = pair {
                        let class: String = inst.raw_get("_className").unwrap_or_default();
                        if !is_base_part(&class) { continue; }

                        let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x, v.0.y, v.0.z)
                            } else { continue; }
                        } else { continue; };

                        let (sx, sy, sz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Size") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                            } else { (2.0, 0.5, 1.0) }
                        } else { (2.0, 0.5, 1.0) };

                        // Closest point on AABB to sphere center
                        let closest_x = cx.max(px - sx).min(px + sx);
                        let closest_y = cy.max(py - sy).min(py + sy);
                        let closest_z = cz.max(pz - sz).min(pz + sz);

                        let ddx = closest_x - cx;
                        let ddy = closest_y - cy;
                        let ddz = closest_z - cz;
                        let dist_sq = ddx * ddx + ddy * ddy + ddz * ddz;

                        if dist_sq <= radius * radius {
                            let hit = lua.create_table()?;
                            hit.set("Instance", inst)?;
                            hit.set("Position", lua.create_userdata(
                                super::types::LuauVector3::new(closest_x, closest_y, closest_z)
                            )?)?;
                            hit.set("Normal", lua.create_userdata(
                                super::types::LuauVector3::new(0.0, 1.0, 0.0)
                            )?)?;
                            hit.set("Distance", t * dir_len)?;
                            hit.set("Material", "Plastic")?;
                            return Ok(mlua::Value::Table(hit));
                        }
                    }
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create Spherecast: {}", e))?;
        workspace.set("Spherecast", spherecast)
            .map_err(|e| format!("Failed to set Spherecast: {}", e))?;

        // workspace:Shapecast(part, direction, params?) -> RaycastResult?
        // Generic shape cast using the part's own bounding box as the shape
        let shapecast = lua.create_function(|lua, (part, direction, _params): (mlua::Table, mlua::Value, Option<mlua::Table>)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            let part_id: i64 = part.raw_get("_entityId").unwrap_or(0);

            let (ox, oy, oz) = if let Ok(mlua::Value::UserData(ud)) = part.raw_get::<mlua::Value>("Position") {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { return Ok(mlua::Value::Nil); }
            } else { return Ok(mlua::Value::Nil); };

            let (hx, hy, hz) = if let Ok(mlua::Value::UserData(ud)) = part.raw_get::<mlua::Value>("Size") {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                } else { (2.0, 0.5, 1.0) }
            } else { (2.0, 0.5, 1.0) };

            let (dx, dy, dz) = if let mlua::Value::UserData(ref ud) = direction {
                if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                    (v.0.x, v.0.y, v.0.z)
                } else { (0.0, 0.0, 0.0) }
            } else { (0.0, 0.0, 0.0) };

            let dir_len = (dx * dx + dy * dy + dz * dz).sqrt();
            if dir_len < 1e-10 { return Ok(mlua::Value::Nil); }

            let steps = 20i32;
            for step in 0..=steps {
                let t = step as f64 / steps as f64;
                let cx = ox + dx * t;
                let cy = oy + dy * t;
                let cz = oz + dz * t;

                for pair in registry.pairs::<i64, mlua::Table>() {
                    if let Ok((eid, inst)) = pair {
                        if eid == part_id { continue; }
                        let class: String = inst.raw_get("_className").unwrap_or_default();
                        if !is_base_part(&class) { continue; }

                        let (px, py, pz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Position") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x, v.0.y, v.0.z)
                            } else { continue; }
                        } else { continue; };

                        let (sx, sy, sz) = if let Ok(mlua::Value::UserData(ud)) = inst.raw_get::<mlua::Value>("Size") {
                            if let Ok(v) = ud.borrow::<super::types::LuauVector3>() {
                                (v.0.x.abs() / 2.0, v.0.y.abs() / 2.0, v.0.z.abs() / 2.0)
                            } else { (2.0, 0.5, 1.0) }
                        } else { (2.0, 0.5, 1.0) };

                        if (cx - hx) <= (px + sx) && (cx + hx) >= (px - sx)
                            && (cy - hy) <= (py + sy) && (cy + hy) >= (py - sy)
                            && (cz - hz) <= (pz + sz) && (cz + hz) >= (pz - sz)
                        {
                            let hit = lua.create_table()?;
                            hit.set("Instance", inst)?;
                            hit.set("Position", lua.create_userdata(
                                super::types::LuauVector3::new(cx, cy, cz)
                            )?)?;
                            hit.set("Normal", lua.create_userdata(
                                super::types::LuauVector3::new(0.0, 1.0, 0.0)
                            )?)?;
                            hit.set("Distance", t * dir_len)?;
                            hit.set("Material", "Plastic")?;
                            return Ok(mlua::Value::Table(hit));
                        }
                    }
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create Shapecast: {}", e))?;
        workspace.set("Shapecast", shapecast)
            .map_err(|e| format!("Failed to set Shapecast: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // GUI Scripting API — Roblox-compatible UI manipulation
    // ========================================================================
    #[cfg(all(feature = "luau", feature = "gui"))]
    fn inject_gui_api(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // ====================================================================
        // GUI Scripting API — Roblox-compatible UI manipulation
        // ====================================================================
        // Mirrors the Rune GUI API. Both runtimes push to the same
        // GUI_COMMANDS thread-local queue in eustress_common::gui.

        let gui_table = lua.create_table()
            .map_err(|e| format!("Failed to create gui table: {}", e))?;

        // gui.set_text(name, text)
        gui_table.set("set_text", lua.create_function(|_, (name, text): (String, String)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetText {
                name, text,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_text: {}", e))?)
            .map_err(|e| format!("set gui.set_text: {}", e))?;

        // gui.get_text(name) -> string
        gui_table.set("get_text", lua.create_function(|_, name: String| {
            Ok(crate::gui::gui_snapshot_get(&name))
        }).map_err(|e| format!("gui.get_text: {}", e))?)
            .map_err(|e| format!("set gui.get_text: {}", e))?;

        // gui.set_visible(name, visible)
        gui_table.set("set_visible", lua.create_function(|_, (name, visible): (String, bool)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetVisible {
                name, visible,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_visible: {}", e))?)
            .map_err(|e| format!("set gui.set_visible: {}", e))?;

        // gui.set_bg_color(name, r, g, b, a)
        gui_table.set("set_bg_color", lua.create_function(|_, (name, r, g, b, a): (String, f64, f64, f64, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetBgColor {
                name, r: r as f32, g: g as f32, b: b as f32, a: a as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_bg_color: {}", e))?)
            .map_err(|e| format!("set gui.set_bg_color: {}", e))?;

        // gui.set_text_color(name, r, g, b, a)
        gui_table.set("set_text_color", lua.create_function(|_, (name, r, g, b, a): (String, f64, f64, f64, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetTextColor {
                name, r: r as f32, g: g as f32, b: b as f32, a: a as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_text_color: {}", e))?)
            .map_err(|e| format!("set gui.set_text_color: {}", e))?;

        // gui.set_border_color(name, r, g, b, a)
        gui_table.set("set_border_color", lua.create_function(|_, (name, r, g, b, a): (String, f64, f64, f64, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetBorderColor {
                name, r: r as f32, g: g as f32, b: b as f32, a: a as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_border_color: {}", e))?)
            .map_err(|e| format!("set gui.set_border_color: {}", e))?;

        // gui.set_position(name, x, y)
        gui_table.set("set_position", lua.create_function(|_, (name, x, y): (String, f64, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetPosition {
                name, x: x as f32, y: y as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_position: {}", e))?)
            .map_err(|e| format!("set gui.set_position: {}", e))?;

        // gui.set_size(name, w, h)
        gui_table.set("set_size", lua.create_function(|_, (name, w, h): (String, f64, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetSize {
                name, w: w as f32, h: h as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_size: {}", e))?)
            .map_err(|e| format!("set gui.set_size: {}", e))?;

        // gui.set_font_size(name, size)
        gui_table.set("set_font_size", lua.create_function(|_, (name, size): (String, f64)| {
            crate::gui::push_gui_command(crate::gui::GuiCommand::SetFontSize {
                name, size: size as f32,
            });
            Ok(())
        }).map_err(|e| format!("gui.set_font_size: {}", e))?)
            .map_err(|e| format!("set gui.set_font_size: {}", e))?;

        globals.set("gui", gui_table)
            .map_err(|e| format!("Failed to set gui: {}", e))?;

        Ok(())
    }

    // ========================================================================
    // Event System — instance.Changed, ChildAdded/ChildRemoved, Touched, etc.
    // ========================================================================
    #[cfg(feature = "luau")]
    fn inject_event_system(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // Global event signal registry: maps entity_id -> { event_name -> signal }
        let event_registry = lua.create_table()
            .map_err(|e| format!("Failed to create event registry: {}", e))?;
        globals.set("__EVENT_REGISTRY__", event_registry)
            .map_err(|e| format!("Failed to set event registry: {}", e))?;

        // Helper Lua function: get_or_create_signal(entityId, eventName)
        // Returns the signal for that entity+event, creating one if needed.
        let get_or_create_event = lua.create_function(|lua, (entity_id, event_name): (i64, String)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__EVENT_REGISTRY__")?;

            // Get or create entity entry
            let entity_events: mlua::Table = match registry.get::<Option<mlua::Table>>(entity_id)? {
                Some(t) => t,
                None => {
                    let t = lua.create_table()?;
                    registry.set(entity_id, t.clone())?;
                    t
                }
            };

            // Get or create signal for this event
            match entity_events.get::<Option<mlua::Table>>(event_name.clone())? {
                Some(signal) => Ok(signal),
                None => {
                    // Create a new signal inline (lightweight version)
                    let signal = lua.create_table()?;
                    let connections = lua.create_table()?;
                    signal.set("_connections", connections)?;
                    signal.set("_nextId", 1i64)?;

                    signal.set("Connect", lua.create_function(|lua, (this, callback): (mlua::Table, mlua::Function)| {
                        let conns: mlua::Table = this.get("_connections")?;
                        let id: i64 = this.get("_nextId")?;
                        this.set("_nextId", id + 1)?;
                        conns.set(id, callback)?;
                        let conn = lua.create_table()?;
                        conn.set("_id", id)?;
                        conn.set("_signal", this.clone())?;
                        conn.set("Connected", true)?;
                        conn.set("Disconnect", lua.create_function(|_, c: mlua::Table| {
                            let cid: i64 = c.get("_id")?;
                            let sig: mlua::Table = c.get("_signal")?;
                            let cs: mlua::Table = sig.get("_connections")?;
                            cs.set(cid, mlua::Value::Nil)?;
                            c.set("Connected", false)?;
                            Ok(())
                        })?)?;
                        Ok(conn)
                    })?)?;

                    signal.set("Wait", lua.create_function(|_, _this: mlua::Table| {
                        Ok(0.0f64)
                    })?)?;

                    entity_events.set(event_name, signal.clone())?;
                    Ok(signal)
                }
            }
        }).map_err(|e| format!("Failed to create __get_or_create_event__: {}", e))?;
        globals.set("__get_or_create_event__", get_or_create_event)
            .map_err(|e| format!("Failed to set __get_or_create_event__: {}", e))?;

        // Helper Lua function: fire_event(entityId, eventName, ...)
        let fire_event = lua.create_function(|lua, (entity_id, event_name, args): (i64, String, mlua::MultiValue)| {
            let globals = lua.globals();
            let registry: mlua::Table = globals.get("__EVENT_REGISTRY__")?;
            if let Some(entity_events) = registry.get::<Option<mlua::Table>>(entity_id)? {
                if let Some(signal) = entity_events.get::<Option<mlua::Table>>(event_name)? {
                    let connections: mlua::Table = signal.get("_connections")?;
                    for pair in connections.pairs::<i64, mlua::Function>() {
                        if let Ok((_, callback)) = pair {
                            let _ = callback.call::<()>(args.clone());
                        }
                    }
                }
            }
            Ok(())
        }).map_err(|e| format!("Failed to create __fire_event__: {}", e))?;
        globals.set("__fire_event__", fire_event)
            .map_err(|e| format!("Failed to set __fire_event__: {}", e))?;

        Ok(())
    }
}

// ============================================================================
// Bevy Resources
// ============================================================================

/// Bevy resource wrapping the Luau runtime state
#[derive(Resource, Default)]
pub struct LuauRuntimeState {
    /// The Luau runtime instance (initialized lazily)
    pub runtime: Option<LuauRuntime>,
    /// Has the runtime been initialized?
    pub initialized: bool,
}

/// Queue of script execution requests processed each frame
#[derive(Resource, Default)]
pub struct ScriptExecutionQueue {
    /// Pending execution requests
    pub pending: Vec<ScriptExecutionRequest>,
}

/// A request to execute a Luau script chunk
#[derive(Debug, Clone)]
pub struct ScriptExecutionRequest {
    /// Human-readable script name (for error reporting)
    pub script_name: String,
    /// Luau source code to execute
    pub source: String,
    /// Entity that owns this script (for context injection)
    pub entity: Option<Entity>,
}

impl ScriptExecutionQueue {
    /// Enqueue a script for execution next frame
    pub fn enqueue(&mut self, name: &str, source: &str, entity: Option<Entity>) {
        self.pending.push(ScriptExecutionRequest {
            script_name: name.to_string(),
            source: source.to_string(),
            entity,
        });
    }
}

// ============================================================================
// Events
// ============================================================================

/// Message: A Luau script was loaded
#[derive(Message, Debug, Clone)]
pub struct LuauScriptLoadEvent {
    /// Script name
    pub script_name: String,
    /// Entity the script belongs to
    pub entity: Entity,
    /// Source file path (if loaded from file)
    pub source_path: Option<String>,
}

/// Message: A Luau script error occurred
#[derive(Message, Debug, Clone)]
pub struct LuauScriptErrorEvent {
    /// Script name
    pub script_name: String,
    /// Error message
    pub error: String,
    /// Line number (if available)
    pub line: Option<u32>,
}
