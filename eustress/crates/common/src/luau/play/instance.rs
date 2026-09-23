//! `Instance` handles for the Play VM.
//!
//! A handle is a userdata holding only an [`InstanceId`]; every read and
//! write goes to the shared [`DataModel`], so there is one source of truth
//! for both script languages and the engine. Handles are interned in a
//! weak-valued table, so the same instance is always the same Luau value:
//! `==` and table keys behave as in Roblox.

use mlua::{FromLua, Function, Lua, MetaMethod, Result as LuaResult, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use crate::datamodel::{
    class_is_a, is_base_part, DataModel, DmValue, EnumItem, HumanoidCommand, InstanceId, OutputLevel,
    PhysicsCommand, SharedDataModel, SoundAction, SoundCommand,
};
use crate::luau::types::{userdata_eq, LuauCFrame, LuauVector3, UserDataPeek};
use crate::scripting::{CFrame, Vector2, Vector3};

use super::convert::{enum_item, enum_name_arg, from_lua, to_lua};
use super::types_ext::{LuauRay, LuauRaycastParams, LuauVector2};

pub(crate) const HANDLES: &str = "__eus_handles";
pub(crate) const METHODS: &str = "__eus_methods";
pub(crate) const HOST: &str = "__eus_host";

/// A script's reference to one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LInst(pub InstanceId);

impl UserData for LInst {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: mlua::String| {
            let key = key.to_str()?;
            index(lua, this.0, &key)
        });
        methods.add_meta_method(MetaMethod::NewIndex, |lua, this, (key, value): (mlua::String, Value)| {
            let key = key.to_str()?;
            newindex(lua, this.0, &key, value)
        });
        // Luau calls `__eq` even for the same object (interned handles make
        // that the usual case), so this must never borrow an operand twice.
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LInst>(&a, &b, |p, q| p.0 == q.0))
        });
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            let dm = shared(lua)?;
            let g = dm.lock();
            Ok(g.name_of(this.0).unwrap_or("<destroyed>").to_string())
        });
    }
}

impl FromLua for LInst {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LInst>()?),
            other => Err(mlua::Error::RuntimeError(format!(
                "expected an Instance, got {} (did you use '.' instead of ':'?)",
                other.type_name()
            ))),
        }
    }
}

/// The DataModel this VM is bound to.
pub fn shared(lua: &Lua) -> LuaResult<SharedDataModel> {
    lua.app_data_ref::<SharedDataModel>()
        .map(|d| d.clone())
        .ok_or_else(|| mlua::Error::RuntimeError("no DataModel bound to this VM".into()))
}

fn with_dm<R>(lua: &Lua, f: impl FnOnce(&mut DataModel) -> R) -> LuaResult<R> {
    let dm = shared(lua)?;
    let mut g = dm.lock();
    Ok(f(&mut g))
}

/// Numeric key for Lua-side tables (exact below 2^53).
pub fn id_key(id: InstanceId) -> f64 {
    id.0 as f64
}

/// The interned handle for `id`.
pub fn handle(lua: &Lua, id: InstanceId) -> LuaResult<Value> {
    let cache: Table = lua.named_registry_value(HANDLES)?;
    let key = id_key(id);
    let existing: Value = cache.raw_get(key)?;
    if !existing.is_nil() {
        return Ok(existing);
    }
    let ud = Value::UserData(lua.create_userdata(LInst(id))?);
    cache.raw_set(key, ud.clone())?;
    Ok(ud)
}

fn opt_handle(lua: &Lua, id: Option<InstanceId>) -> LuaResult<Value> {
    match id {
        Some(id) => handle(lua, id),
        None => Ok(Value::Nil),
    }
}

fn handle_list(lua: &Lua, ids: &[InstanceId]) -> LuaResult<Table> {
    let t = lua.create_table_with_capacity(ids.len(), 0)?;
    for (i, id) in ids.iter().enumerate() {
        t.raw_set(i + 1, handle(lua, *id)?)?;
    }
    Ok(t)
}

fn host_table(lua: &Lua) -> LuaResult<Table> {
    lua.named_registry_value(HOST)
}

/// The instance and tag of a tag call, whichever form the script used:
/// `inst:AddTag(tag)` or `CollectionService:AddTag(inst, tag)`.
fn tag_target(this: LInst, first: Value, second: Option<String>) -> LuaResult<(InstanceId, String)> {
    match (first, second) {
        (Value::UserData(ud), Some(tag)) => Ok((ud.peek::<LInst>()?.0, tag)),
        (Value::String(s), _) => Ok((this.0, s.to_str()?.to_string())),
        (other, _) => Err(mlua::Error::RuntimeError(format!(
            "expected a tag (string), got {}",
            other.type_name()
        ))),
    }
}

/// Events every instance exposes, plus the class-specific ones.
fn is_event(class: &str, key: &str) -> bool {
    match key {
        "Changed" | "ChildAdded" | "ChildRemoved" | "DescendantAdded" | "DescendantRemoving" | "AncestryChanged"
        | "Destroying" | "AttributeChanged" => true,
        "Touched" | "TouchEnded" => is_base_part(class),
        "Died" | "HealthChanged" | "MoveToFinished" | "Running" | "Jumping" | "StateChanged" | "FreeFalling" => {
            class == "Humanoid"
        }
        "PlayerAdded" | "PlayerRemoving" => class == "Players",
        "CharacterAdded" | "CharacterRemoving" | "CharacterAppearanceLoaded" | "Chatted" | "Idled" => class == "Player",
        "MouseButton1Click" | "MouseButton1Down" | "MouseButton1Up" | "MouseButton2Click" | "Activated"
        | "MouseEnter" | "MouseLeave" => class_is_a(class, "GuiButton") || class_is_a(class, "GuiObject"),
        "Event" => class == "BindableEvent",
        "OnServerEvent" | "OnClientEvent" => class == "RemoteEvent",
        "InputBegan" | "InputEnded" | "InputChanged" | "JumpRequest" | "WindowFocused" | "WindowFocusReleased" => {
            class == "UserInputService" || class_is_a(class, "GuiObject")
        }
        "MouseClick" | "RightMouseClick" | "MouseHoverEnter" | "MouseHoverLeave" => class == "ClickDetector",
        "Triggered" | "TriggerEnded" | "PromptShown" | "PromptHidden" => class == "ProximityPrompt",
        "Ended" | "Played" | "Loaded" => class == "Sound",
        "Close" | "Loaded_" => class == "DataModel",
        _ => false,
    }
}

fn run_service_signal(key: &str) -> bool {
    matches!(key, "Heartbeat" | "Stepped" | "RenderStepped" | "PreSimulation" | "PostSimulation" | "PreRender")
}

// ============================================================================
// __index / __newindex
// ============================================================================

fn index(lua: &Lua, id: InstanceId, key: &str) -> LuaResult<Value> {
    // Class and liveness in one lock.
    let (class, alive) = {
        let dm = shared(lua)?;
        let g = dm.lock();
        match g.get(id) {
            Some(i) => (i.class_name.clone(), !i.destroyed),
            None => return Ok(Value::Nil),
        }
    };

    // 1. Methods (Lua-implemented ones live in the same table).
    if method_applies(&class, key) {
        let methods: Table = lua.named_registry_value(METHODS)?;
        let f: Value = methods.raw_get(key)?;
        if !f.is_nil() {
            return Ok(f);
        }
    }

    // 2. Service members implemented in the prelude.
    if let Some(v) = service_member(lua, &class, key)? {
        return Ok(v);
    }

    // 3. Signals.
    if alive && is_event(&class, key) {
        if key == "Changed" || key == "HealthChanged" {
            with_dm(lua, |dm| dm.watch_changes(id))?;
        }
        let host = host_table(lua)?;
        let get: Function = host.raw_get("getSignal")?;
        return get.call((id_key(id), key));
    }

    // 4. Properties.
    let (prop, child) = {
        let dm = shared(lua)?;
        let g = dm.lock();
        let prop = g.get_prop(id, key);
        let child = if prop.is_none() { g.find_first_child(id, key, false) } else { None };
        (prop, child)
    };
    if let Some(v) = prop {
        return to_lua(lua, &v);
    }

    // 5. Children by name.
    if let Some(c) = child {
        return handle(lua, c);
    }
    Ok(Value::Nil)
}

/// The Humanoid property behind `SetStateEnabled(state, ...)`: Jumping and
/// Climbing are the engine's JumpEnabled and ClimbingEnabled; any other state
/// is remembered under a hidden name that nothing applies.
fn state_enabled_prop(state: &str) -> String {
    match state {
        "Jumping" => "JumpEnabled".into(),
        "Climbing" => "ClimbingEnabled".into(),
        other => format!("__StateEnabled{other}"),
    }
}

fn newindex(lua: &Lua, id: InstanceId, key: &str, value: Value) -> LuaResult<()> {
    // Callback-valued members live Lua-side (BindableFunction.OnInvoke ...).
    if let Value::Function(f) = &value {
        let host = host_table(lua)?;
        let set: Function = host.raw_get("setCallback")?;
        return set.call((id_key(id), key, f.clone()));
    }
    // Signals cannot be assigned.
    let class = with_dm(lua, |dm| dm.class_of(id).map(str::to_string))?;
    let Some(class) = class else {
        return Err(mlua::Error::RuntimeError(format!("cannot set {} of a destroyed instance", key)));
    };
    if is_event(&class, key) {
        return Err(mlua::Error::RuntimeError(format!("{} is a signal and cannot be assigned", key)));
    }
    if class == "UserInputService" && key == "MouseIconEnabled" {
        if let Value::Boolean(b) = value {
            with_dm(lua, |dm| dm.input.mouse_icon_enabled = b)?;
        }
    }
    let v = from_lua(&value).map_err(|e| mlua::Error::RuntimeError(format!("{}: {}", key, e)))?;
    with_dm(lua, |dm| dm.set_prop(id, key, v))?.map_err(mlua::Error::RuntimeError)
}

/// Members of services that the Lua prelude implements.
fn service_member(lua: &Lua, class: &str, key: &str) -> LuaResult<Option<Value>> {
    let host = host_table(lua)?;
    let table_name = match class {
        "RunService" if run_service_signal(key) => {
            let signals: Table = host.raw_get("RunServiceSignals")?;
            return Ok(Some(signals.raw_get(key)?));
        }
        "TweenService" => "TweenService",
        "Debris" => "Debris",
        "ContextActionService" => "ContextActionService",
        _ => return Ok(None),
    };
    let t: Value = host.raw_get(table_name)?;
    if let Value::Table(t) = t {
        let v: Value = t.raw_get(key)?;
        if !v.is_nil() {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Whether `key` names a method on `class`, so a child called "Play" is not
/// shadowed on a Part.
fn method_applies(class: &str, key: &str) -> bool {
    match key {
        "Destroy" | "Clone" | "ClearAllChildren" | "FindFirstChild" | "FindFirstChildOfClass"
        | "FindFirstChildWhichIsA" | "FindFirstAncestor" | "FindFirstAncestorOfClass" | "FindFirstAncestorWhichIsA"
        | "FindFirstDescendant" | "GetChildren" | "GetDescendants" | "IsA" | "IsDescendantOf" | "IsAncestorOf"
        | "GetFullName" | "WaitForChild" | "GetAttribute" | "SetAttribute" | "GetAttributes"
        | "GetAttributeChangedSignal" | "GetPropertyChangedSignal" | "AddTag" | "RemoveTag" | "HasTag" | "GetTags"
        | "GetDebugId" | "isA" | "findFirstChild" | "children" | "remove" | "Remove" => true,
        "GetPivot" | "PivotTo" => is_base_part(class) || class_is_a(class, "Model"),
        "ApplyImpulse" | "ApplyAngularImpulse" | "ApplyImpulseAtPosition" | "GetMass" | "SetNetworkOwner"
        | "GetNetworkOwner" | "SetNetworkOwnershipAuto" | "BreakJoints" | "GetTouchingParts" | "GetConnectedParts"
        | "GetRootPart" | "CanCollideWith" => is_base_part(class),
        "GetBoundingBox" | "GetExtentsSize" | "MoveTo" | "SetPrimaryPartCFrame" | "GetPrimaryPartCFrame"
        | "TranslateBy" | "GetModelCFrame" => class_is_a(class, "Model") || (key == "MoveTo" && class == "Humanoid"),
        "TakeDamage" | "Move" | "ChangeState" | "GetState" | "LoadAnimation" | "UnequipTools" | "EquipTool"
        | "GetPlayingAnimationTracks" | "SetStateEnabled" | "GetStateEnabled" => {
            class == "Humanoid" || (key == "LoadAnimation" && class == "Animator")
        }
        "Play" | "Stop" | "Pause" | "Resume" => class == "Sound",
        "Emit" | "Clear" => class == "ParticleEmitter",
        "Fire" => class == "BindableEvent",
        "Invoke" => class == "BindableFunction",
        "FireServer" | "FireClient" | "FireAllClients" => class == "RemoteEvent",
        "InvokeServer" | "InvokeClient" => class == "RemoteFunction",
        "GetPlayers" | "GetPlayerFromCharacter" | "GetPlayerByUserId" => class == "Players",
        "GetMouse" | "LoadCharacter" | "Kick" | "GetRankInGroup" | "IsInGroup" | "DistanceFromCharacter" => {
            class == "Player"
        }
        "ViewportPointToRay" | "ScreenPointToRay" | "WorldToViewportPoint" | "WorldToScreenPoint" => class == "Camera",
        "Raycast" | "GetServerTimeNow" | "GetPartBoundsInRadius" | "GetPartBoundsInBox" | "FindPartOnRay"
        | "FindPartOnRayWithIgnoreList" | "Spherecast" | "Blockcast" | "GetRealPhysicsFPS" => class == "Workspace",
        "GetService" | "FindService" | "IsLoaded" | "BindToClose" => class == "DataModel",
        "IsKeyDown" | "IsMouseButtonPressed" | "GetMouseLocation" | "GetMouseDelta" | "GetKeysPressed"
        | "GetMouseButtonsPressed" | "GetLastInputType" | "GetFocusedTextBox" | "IsGamepadButtonDown"
        | "GetConnectedGamepads" => class == "UserInputService",
        "IsClient" | "IsServer" | "IsStudio" | "IsRunning" | "IsRunMode" | "IsEdit" | "BindToRenderStep"
        | "UnbindFromRenderStep" => class == "RunService",
        "GetTagged" | "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" | "GetAllTags" => {
            class == "CollectionService"
        }
        "JSONEncode" | "JSONDecode" | "GenerateGUID" | "UrlEncode" => class == "HttpService",
        "PlayLocalSound" => class == "SoundService",
        _ => false,
    }
}

// ============================================================================
// Method table
// ============================================================================

macro_rules! method {
    ($lua:expr, $t:expr, $name:expr, $f:expr) => {
        $t.raw_set($name, $lua.create_function($f)?)?;
    };
}

/// Build the shared method table. Called once per VM.
pub fn install_methods(lua: &Lua) -> LuaResult<()> {
    let t = lua.create_table()?;

    // ── Instance ─────────────────────────────────────────────────────────
    method!(lua, t, "Destroy", |lua, this: LInst| {
        with_dm(lua, |dm| dm.destroy(this.0))?;
        Ok(())
    });
    method!(lua, t, "Remove", |lua, this: LInst| {
        with_dm(lua, |dm| dm.set_parent(this.0, None))?.map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "remove", |lua, this: LInst| {
        with_dm(lua, |dm| dm.set_parent(this.0, None))?.map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "Clone", |lua, this: LInst| {
        let c = with_dm(lua, |dm| dm.clone_instance(this.0))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "ClearAllChildren", |lua, this: LInst| {
        with_dm(lua, |dm| dm.clear_all_children(this.0))?;
        Ok(())
    });
    method!(lua, t, "FindFirstChild", |lua, (this, name, recursive): (LInst, String, Option<bool>)| {
        let c = with_dm(lua, |dm| dm.find_first_child(this.0, &name, recursive.unwrap_or(false)))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "findFirstChild", |lua, (this, name, recursive): (LInst, String, Option<bool>)| {
        let c = with_dm(lua, |dm| dm.find_first_child(this.0, &name, recursive.unwrap_or(false)))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstDescendant", |lua, (this, name): (LInst, String)| {
        let c = with_dm(lua, |dm| dm.find_first_child(this.0, &name, true))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstChildOfClass", |lua, (this, class): (LInst, String)| {
        let c = with_dm(lua, |dm| dm.find_first_child_of_class(this.0, &class, false))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstChildWhichIsA", |lua, (this, class, recursive): (LInst, String, Option<bool>)| {
        let c = with_dm(lua, |dm| dm.find_first_child_which_is_a(this.0, &class, recursive.unwrap_or(false)))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstAncestor", |lua, (this, name): (LInst, String)| {
        let c = with_dm(lua, |dm| dm.find_first_ancestor(this.0, &name))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstAncestorOfClass", |lua, (this, class): (LInst, String)| {
        let c = with_dm(lua, |dm| {
            let mut cur = dm.parent(this.0);
            while let Some(p) = cur {
                if dm.class_of(p) == Some(class.as_str()) {
                    return Some(p);
                }
                cur = dm.parent(p);
            }
            None
        })?;
        opt_handle(lua, c)
    });
    method!(lua, t, "FindFirstAncestorWhichIsA", |lua, (this, class): (LInst, String)| {
        let c = with_dm(lua, |dm| dm.find_first_ancestor_which_is_a(this.0, &class))?;
        opt_handle(lua, c)
    });
    method!(lua, t, "GetChildren", |lua, this: LInst| {
        let kids = with_dm(lua, |dm| dm.children(this.0).to_vec())?;
        handle_list(lua, &kids)
    });
    method!(lua, t, "children", |lua, this: LInst| {
        let kids = with_dm(lua, |dm| dm.children(this.0).to_vec())?;
        handle_list(lua, &kids)
    });
    method!(lua, t, "GetDescendants", |lua, this: LInst| {
        let all = with_dm(lua, |dm| dm.descendants(this.0))?;
        handle_list(lua, &all)
    });
    method!(lua, t, "IsA", |lua, (this, class): (LInst, String)| {
        with_dm(lua, |dm| dm.is_a(this.0, &class))
    });
    method!(lua, t, "isA", |lua, (this, class): (LInst, String)| {
        with_dm(lua, |dm| dm.is_a(this.0, &class))
    });
    method!(lua, t, "IsDescendantOf", |lua, (this, other): (LInst, LInst)| {
        with_dm(lua, |dm| dm.is_descendant_of(this.0, other.0))
    });
    method!(lua, t, "IsAncestorOf", |lua, (this, other): (LInst, LInst)| {
        with_dm(lua, |dm| dm.is_descendant_of(other.0, this.0))
    });
    method!(lua, t, "GetFullName", |lua, this: LInst| with_dm(lua, |dm| dm.full_name(this.0)));
    method!(lua, t, "GetDebugId", |_, this: LInst| Ok(format!("{}", this.0 .0)));
    method!(lua, t, "GetAttribute", |lua, (this, name): (LInst, String)| {
        let v = with_dm(lua, |dm| dm.get_attribute(this.0, &name))?;
        match v {
            Some(v) => to_lua(lua, &v),
            None => Ok(Value::Nil),
        }
    });
    method!(lua, t, "SetAttribute", |lua, (this, name, value): (LInst, String, Value)| {
        let v = from_lua(&value)?;
        with_dm(lua, |dm| dm.set_attribute(this.0, &name, v))?.map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "GetAttributes", |lua, this: LInst| {
        let attrs = with_dm(lua, |dm| dm.attributes(this.0))?;
        let t = lua.create_table()?;
        for (k, v) in attrs {
            t.raw_set(k, to_lua(lua, &v)?)?;
        }
        Ok(t)
    });
    method!(lua, t, "GetAttributeChangedSignal", |lua, (this, name): (LInst, String)| {
        let host = host_table(lua)?;
        let f: Function = host.raw_get("getAttributeSignal")?;
        f.call::<Value>((id_key(this.0), name))
    });
    method!(lua, t, "GetPropertyChangedSignal", |lua, (this, prop): (LInst, String)| {
        with_dm(lua, |dm| dm.watch_changes(this.0))?;
        let host = host_table(lua)?;
        let f: Function = host.raw_get("getPropertySignal")?;
        f.call::<Value>((id_key(this.0), prop))
    });
    // Both forms: `inst:AddTag(tag)` and `CollectionService:AddTag(inst, tag)`.
    method!(lua, t, "AddTag", |lua, (this, first, second): (LInst, Value, Option<String>)| {
        let (id, tag) = tag_target(this, first, second)?;
        with_dm(lua, |dm| dm.add_tag(id, &tag))?;
        Ok(())
    });
    method!(lua, t, "RemoveTag", |lua, (this, first, second): (LInst, Value, Option<String>)| {
        let (id, tag) = tag_target(this, first, second)?;
        with_dm(lua, |dm| dm.remove_tag(id, &tag))?;
        Ok(())
    });
    method!(lua, t, "HasTag", |lua, (this, first, second): (LInst, Value, Option<String>)| {
        let (id, tag) = tag_target(this, first, second)?;
        with_dm(lua, |dm| dm.has_tag(id, &tag))
    });
    method!(lua, t, "GetTags", |lua, (this, target): (LInst, Option<LInst>)| {
        let id = target.map_or(this.0, |t| t.0);
        let tags = with_dm(lua, |dm| dm.tags_of(id))?;
        lua.create_sequence_from(tags)
    });

    // ── PVInstance: pivots ───────────────────────────────────────────────
    method!(lua, t, "GetPivot", |lua, this: LInst| {
        let cf = with_dm(lua, |dm| pivot_of(dm, this.0))?;
        Ok(LuaCFrameOut(cf))
    });
    method!(lua, t, "PivotTo", |lua, (this, cf): (LInst, LuauCFrame)| {
        with_dm(lua, |dm| pivot_to(dm, this.0, cf.0))?.map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "SetPrimaryPartCFrame", |lua, (this, cf): (LInst, LuauCFrame)| {
        with_dm(lua, |dm| pivot_to(dm, this.0, cf.0))?.map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "GetPrimaryPartCFrame", |lua, this: LInst| {
        let cf = with_dm(lua, |dm| pivot_of(dm, this.0))?;
        Ok(LuaCFrameOut(cf))
    });
    method!(lua, t, "GetModelCFrame", |lua, this: LInst| {
        let cf = with_dm(lua, |dm| pivot_of(dm, this.0))?;
        Ok(LuaCFrameOut(cf))
    });
    method!(lua, t, "TranslateBy", |lua, (this, v): (LInst, LuauVector3)| {
        with_dm(lua, |dm| {
            let cf = pivot_of(dm, this.0);
            pivot_to(dm, this.0, cf + v.0)
        })?
        .map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "GetBoundingBox", |lua, this: LInst| {
        let (center, size) = with_dm(lua, |dm| bounding_box(dm, this.0))?;
        Ok((LuauCFrame(CFrame::from_position(center)), LuauVector3(size)))
    });
    method!(lua, t, "GetExtentsSize", |lua, this: LInst| {
        let (_, size) = with_dm(lua, |dm| bounding_box(dm, this.0))?;
        Ok(LuauVector3(size))
    });

    // `MoveTo` is shared by Model (teleport the pivot) and Humanoid (walk).
    method!(lua, t, "MoveTo", |lua, (this, target, _part): (LInst, LuauVector3, Option<Value>)| {
        with_dm(lua, |dm| {
            if dm.class_of(this.0) == Some("Humanoid") {
                dm.humanoid_commands.push(HumanoidCommand::MoveTo { humanoid: this.0, target: target.0 });
                // Walk-to is on the ground plane; MoveDirection updates as it goes.
                Ok(())
            } else {
                let cf = pivot_of(dm, this.0);
                let moved = CFrame::from_position(target.0) * cf.rotation_only();
                pivot_to(dm, this.0, moved)
            }
        })?
        .map_err(mlua::Error::RuntimeError)
    });

    // ── BasePart ─────────────────────────────────────────────────────────
    method!(lua, t, "ApplyImpulse", |lua, (this, v): (LInst, LuauVector3)| {
        with_dm(lua, |dm| dm.physics_commands.push(PhysicsCommand::ApplyImpulse { part: this.0, impulse: v.0 }))?;
        Ok(())
    });
    method!(lua, t, "ApplyImpulseAtPosition", |lua, (this, v, _p): (LInst, LuauVector3, LuauVector3)| {
        with_dm(lua, |dm| dm.physics_commands.push(PhysicsCommand::ApplyImpulse { part: this.0, impulse: v.0 }))?;
        Ok(())
    });
    method!(lua, t, "ApplyAngularImpulse", |lua, (this, v): (LInst, LuauVector3)| {
        with_dm(lua, |dm| dm.physics_commands.push(PhysicsCommand::ApplyAngularImpulse { part: this.0, impulse: v.0 }))?;
        Ok(())
    });
    method!(lua, t, "GetMass", |lua, this: LInst| {
        with_dm(lua, |dm| {
            if let Some(m) = dm.get_prop(this.0, "Mass").and_then(|v| v.as_number()) {
                return m;
            }
            let size = dm.get_prop(this.0, "Size").and_then(|v| v.as_vector3()).unwrap_or(Vector3::ONE);
            900.0 * size.x * size.y * size.z
        })
    });
    method!(lua, t, "SetNetworkOwner", |_, _args: Variadic<Value>| Ok(()));
    method!(lua, t, "SetNetworkOwnershipAuto", |_, _args: Variadic<Value>| Ok(()));
    method!(lua, t, "GetNetworkOwner", |_, _this: LInst| Ok(Value::Nil));
    method!(lua, t, "BreakJoints", |_, _this: LInst| Ok(()));
    method!(lua, t, "CanCollideWith", |_, _args: Variadic<Value>| Ok(true));
    method!(lua, t, "GetRootPart", |lua, this: LInst| handle(lua, this.0));
    method!(lua, t, "GetConnectedParts", |lua, this: LInst| handle_list(lua, &[this.0]));
    method!(lua, t, "GetTouchingParts", |lua, _this: LInst| lua.create_table());

    // ── Humanoid ─────────────────────────────────────────────────────────
    method!(lua, t, "TakeDamage", |lua, (this, amount): (LInst, f64)| {
        with_dm(lua, |dm| {
            let hp = dm.get_prop(this.0, "Health").and_then(|v| v.as_number()).unwrap_or(0.0);
            dm.set_prop(this.0, "Health", DmValue::Number((hp - amount).max(0.0)))
        })?
        .map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "Move", |lua, (this, dir, _relative): (LInst, LuauVector3, Option<bool>)| {
        with_dm(lua, |dm| dm.humanoid_commands.push(HumanoidCommand::Move { humanoid: this.0, direction: dir.0 }))?;
        Ok(())
    });
    method!(lua, t, "ChangeState", |lua, (this, state): (LInst, Value)| {
        let name = enum_name_arg(&state).unwrap_or_default();
        with_dm(lua, |dm| match name.as_str() {
            "Dead" => dm.set_prop(this.0, "Health", DmValue::Number(0.0)),
            "Jumping" => {
                dm.humanoid_commands.push(HumanoidCommand::Jump { humanoid: this.0 });
                Ok(())
            }
            _ => Ok(()),
        })?
        .map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "GetState", |lua, this: LInst| {
        let dead = with_dm(lua, |dm| dm.get_prop(this.0, "Health").and_then(|v| v.as_number()).unwrap_or(1.0) <= 0.0)?;
        enum_item(lua, &EnumItem::new("HumanoidStateType", if dead { "Dead" } else { "Running" }))
    });
    // Jumping and Climbing switch the character's movement verbs (the same
    // JumpEnabled / ClimbingEnabled properties a script can set directly);
    // other states are accepted and remembered.
    method!(lua, t, "SetStateEnabled", |lua, (this, state, enabled): (LInst, Value, bool)| {
        let prop = state_enabled_prop(&enum_name_arg(&state).unwrap_or_default());
        with_dm(lua, |dm| dm.set_prop(this.0, &prop, DmValue::Bool(enabled)))?
            .map_err(mlua::Error::RuntimeError)
    });
    method!(lua, t, "GetStateEnabled", |lua, (this, state): (LInst, Value)| {
        let prop = state_enabled_prop(&enum_name_arg(&state).unwrap_or_default());
        with_dm(lua, |dm| dm.get_prop(this.0, &prop).and_then(|v| v.as_bool()).unwrap_or(true))
    });
    method!(lua, t, "UnequipTools", |_, _this: LInst| Ok(()));
    method!(lua, t, "EquipTool", |_, _args: Variadic<Value>| Ok(()));
    method!(lua, t, "GetPlayingAnimationTracks", |lua, _this: LInst| lua.create_table());
    method!(lua, t, "LoadAnimation", |lua, (_this, _anim): (LInst, Value)| {
        // Rig animation is not exposed to scripts yet; a track that accepts
        // the calls keeps ported scripts running.
        let host = host_table(lua)?;
        let f: Function = host.raw_get("stubAnimationTrack")?;
        f.call::<Value>(())
    });

    // ── Sound / ParticleEmitter ──────────────────────────────────────────
    method!(lua, t, "Play", |lua, this: LInst| sound(lua, this.0, SoundAction::Play));
    method!(lua, t, "Stop", |lua, this: LInst| sound(lua, this.0, SoundAction::Stop));
    method!(lua, t, "Pause", |lua, this: LInst| sound(lua, this.0, SoundAction::Pause));
    method!(lua, t, "Resume", |lua, this: LInst| sound(lua, this.0, SoundAction::Resume));
    method!(lua, t, "Emit", |lua, (this, n): (LInst, Option<f64>)| {
        with_dm(lua, |dm| dm.particle_emits.push((this.0, n.unwrap_or(16.0).max(0.0) as u32)))?;
        Ok(())
    });
    method!(lua, t, "Clear", |_, _this: LInst| Ok(()));

    // ── Players / Player ─────────────────────────────────────────────────
    method!(lua, t, "GetPlayers", |lua, this: LInst| {
        let players = with_dm(lua, |dm| {
            dm.children(this.0).iter().copied().filter(|c| dm.class_of(*c) == Some("Player")).collect::<Vec<_>>()
        })?;
        handle_list(lua, &players)
    });
    method!(lua, t, "GetPlayerFromCharacter", |lua, (this, character): (LInst, Option<LInst>)| {
        let Some(character) = character else { return Ok(Value::Nil) };
        let p = with_dm(lua, |dm| {
            dm.children(this.0).iter().copied().find(|p| {
                dm.get_prop(*p, "Character").and_then(|v| v.as_instance()) == Some(character.0)
            })
        })?;
        opt_handle(lua, p)
    });
    method!(lua, t, "GetPlayerByUserId", |lua, (this, user_id): (LInst, f64)| {
        let p = with_dm(lua, |dm| {
            dm.children(this.0).iter().copied().find(|p| {
                dm.get_prop(*p, "UserId").and_then(|v| v.as_number()) == Some(user_id)
            })
        })?;
        opt_handle(lua, p)
    });
    method!(lua, t, "GetMouse", |lua, _this: LInst| {
        let host = host_table(lua)?;
        let m: Value = host.raw_get("mouse")?;
        Ok(m)
    });
    method!(lua, t, "LoadCharacter", |lua, this: LInst| {
        with_dm(lua, |dm| {
            dm.print(OutputLevel::Info, "Players", "LoadCharacter: respawn requested");
            let _ = dm.set_prop(this.0, "__respawn", DmValue::Bool(true));
        })?;
        Ok(())
    });
    method!(lua, t, "Kick", |lua, (_this, msg): (LInst, Option<String>)| {
        with_dm(lua, |dm| {
            dm.print(OutputLevel::Warn, "Players", format!("Kick: {}", msg.unwrap_or_default()))
        })?;
        Ok(())
    });
    method!(lua, t, "DistanceFromCharacter", |lua, (this, p): (LInst, LuauVector3)| {
        with_dm(lua, |dm| {
            let ch = dm.get_prop(this.0, "Character").and_then(|v| v.as_instance());
            let root = ch.and_then(|c| dm.find_first_child(c, "HumanoidRootPart", false));
            match root.and_then(|r| dm.get(r).and_then(|i| i.cframe())) {
                Some(cf) => (cf.position - p.0).magnitude(),
                None => 0.0,
            }
        })
    });
    method!(lua, t, "GetRankInGroup", |_, _args: Variadic<Value>| Ok(0));
    method!(lua, t, "IsInGroup", |_, _args: Variadic<Value>| Ok(false));

    // ── Camera ───────────────────────────────────────────────────────────
    method!(lua, t, "ViewportPointToRay", |lua, (_this, x, y, _depth): (LInst, f64, f64, Option<f64>)| {
        let (o, d) = with_dm(lua, |dm| dm.viewport_point_to_ray(x, y))?;
        Ok(LuauRay { origin: o, direction: d })
    });
    method!(lua, t, "ScreenPointToRay", |lua, (_this, x, y, _depth): (LInst, f64, f64, Option<f64>)| {
        let (o, d) = with_dm(lua, |dm| dm.viewport_point_to_ray(x, y))?;
        Ok(LuauRay { origin: o, direction: d })
    });
    method!(lua, t, "WorldToViewportPoint", |lua, (_this, p): (LInst, LuauVector3)| {
        let (v, on) = with_dm(lua, |dm| dm.world_to_viewport_point(p.0))?;
        Ok((LuauVector3(v), on))
    });
    method!(lua, t, "WorldToScreenPoint", |lua, (_this, p): (LInst, LuauVector3)| {
        let (v, on) = with_dm(lua, |dm| dm.world_to_viewport_point(p.0))?;
        Ok((LuauVector3(v), on))
    });

    // ── Workspace ────────────────────────────────────────────────────────
    method!(lua, t, "Raycast", |lua, (_this, origin, direction, params): (LInst, LuauVector3, LuauVector3, Option<LuauRaycastParams>)| {
        raycast(lua, origin.0, direction.0, params.unwrap_or_default())
    });
    method!(lua, t, "GetServerTimeNow", |lua, _this: LInst| with_dm(lua, |dm| dm.frame.time));
    method!(lua, t, "GetRealPhysicsFPS", |_, _this: LInst| Ok(60.0));
    method!(lua, t, "GetPartBoundsInRadius", |lua, (_this, center, radius, params): (LInst, LuauVector3, f64, Option<LuauRaycastParams>)| {
        let hits = with_dm(lua, |dm| parts_in_radius(dm, center.0, radius, params.as_ref()))?;
        handle_list(lua, &hits)
    });
    method!(lua, t, "GetPartBoundsInBox", |lua, (_this, cf, size, params): (LInst, LuauCFrame, LuauVector3, Option<LuauRaycastParams>)| {
        let hits = with_dm(lua, |dm| parts_in_box(dm, cf.0, size.0, params.as_ref()))?;
        handle_list(lua, &hits)
    });
    method!(lua, t, "FindPartOnRay", |lua, (_this, ray, ignore): (LInst, LuauRay, Option<LInst>)| {
        let mut params = LuauRaycastParams::default();
        if let Some(i) = ignore {
            params.filter.push(i.0);
        }
        legacy_find_part(lua, ray, params)
    });
    method!(lua, t, "FindPartOnRayWithIgnoreList", |lua, (_this, ray, ignore): (LInst, LuauRay, Option<Table>)| {
        let mut params = LuauRaycastParams::default();
        if let Some(t) = ignore {
            for v in t.sequence_values::<Value>() {
                if let Value::UserData(ud) = v? {
                    if let Ok(i) = ud.peek::<LInst>() {
                        params.filter.push(i.0);
                    }
                }
            }
        }
        legacy_find_part(lua, ray, params)
    });

    // ── DataModel (game) ─────────────────────────────────────────────────
    method!(lua, t, "GetService", |lua, (_this, name): (LInst, String)| {
        let s = with_dm(lua, |dm| dm.get_service(&name))?;
        match s {
            Some(s) => handle(lua, s),
            None => Err(mlua::Error::RuntimeError(format!("'{}' is not a valid Service name", name))),
        }
    });
    method!(lua, t, "FindService", |lua, (_this, name): (LInst, String)| {
        let s = with_dm(lua, |dm| dm.find_service(&name))?;
        opt_handle(lua, s)
    });
    method!(lua, t, "IsLoaded", |_, _this: LInst| Ok(true));
    method!(lua, t, "BindToClose", |_, _args: Variadic<Value>| Ok(()));

    // ── UserInputService ─────────────────────────────────────────────────
    method!(lua, t, "IsKeyDown", |lua, (_this, key): (LInst, Value)| {
        let name = enum_name_arg(&key).unwrap_or_default();
        with_dm(lua, |dm| dm.input.keys.contains(&name))
    });
    method!(lua, t, "IsMouseButtonPressed", |lua, (_this, b): (LInst, Value)| {
        let name = enum_name_arg(&b).unwrap_or_default();
        with_dm(lua, |dm| dm.input.buttons.contains(&name))
    });
    method!(lua, t, "GetMouseLocation", |lua, _this: LInst| {
        let (x, y) = with_dm(lua, |dm| (dm.input.mouse_x, dm.input.mouse_y))?;
        Ok(LuauVector2(Vector2::new(x, y)))
    });
    method!(lua, t, "GetMouseDelta", |lua, _this: LInst| {
        let (x, y) = with_dm(lua, |dm| (dm.input.mouse_dx, dm.input.mouse_dy))?;
        Ok(LuauVector2(Vector2::new(x, y)))
    });
    method!(lua, t, "GetKeysPressed", |lua, _this: LInst| {
        let keys: Vec<String> = with_dm(lua, |dm| dm.input.keys.iter().cloned().collect())?;
        let out = lua.create_table()?;
        for (i, k) in keys.iter().enumerate() {
            let o = lua.create_table()?;
            o.raw_set("KeyCode", enum_item(lua, &EnumItem::new("KeyCode", k.clone()))?)?;
            o.raw_set("UserInputType", enum_item(lua, &EnumItem::new("UserInputType", "Keyboard"))?)?;
            out.raw_set(i + 1, o)?;
        }
        Ok(out)
    });
    method!(lua, t, "GetMouseButtonsPressed", |lua, _this: LInst| {
        let buttons: Vec<String> = with_dm(lua, |dm| dm.input.buttons.iter().cloned().collect())?;
        let out = lua.create_table()?;
        for (i, b) in buttons.iter().enumerate() {
            let o = lua.create_table()?;
            o.raw_set("UserInputType", enum_item(lua, &EnumItem::new("UserInputType", b.clone()))?)?;
            out.raw_set(i + 1, o)?;
        }
        Ok(out)
    });
    method!(lua, t, "GetLastInputType", |lua, _this: LInst| {
        enum_item(lua, &EnumItem::new("UserInputType", "Keyboard"))
    });
    method!(lua, t, "GetFocusedTextBox", |_, _this: LInst| Ok(Value::Nil));
    method!(lua, t, "IsGamepadButtonDown", |_, _args: Variadic<Value>| Ok(false));
    method!(lua, t, "GetConnectedGamepads", |lua, _this: LInst| lua.create_table());

    // ── RunService ───────────────────────────────────────────────────────
    method!(lua, t, "IsClient", |_, _this: LInst| Ok(true));
    method!(lua, t, "IsServer", |_, _this: LInst| Ok(true));
    method!(lua, t, "IsStudio", |_, _this: LInst| Ok(true));
    method!(lua, t, "IsRunning", |_, _this: LInst| Ok(true));
    method!(lua, t, "IsRunMode", |_, _this: LInst| Ok(false));
    method!(lua, t, "IsEdit", |_, _this: LInst| Ok(false));
    method!(lua, t, "BindToRenderStep", |lua, (_this, name, priority, f): (LInst, String, f64, Function)| {
        let host = host_table(lua)?;
        let bind: Function = host.raw_get("bindToRenderStep")?;
        bind.call::<()>((name, priority, f))
    });
    method!(lua, t, "UnbindFromRenderStep", |lua, (_this, name): (LInst, String)| {
        let host = host_table(lua)?;
        let unbind: Function = host.raw_get("unbindFromRenderStep")?;
        unbind.call::<()>(name)
    });

    // ── CollectionService ────────────────────────────────────────────────
    method!(lua, t, "GetTagged", |lua, (_this, tag): (LInst, String)| {
        let ids = with_dm(lua, |dm| dm.tagged(&tag))?;
        handle_list(lua, &ids)
    });
    method!(lua, t, "GetAllTags", |lua, _this: LInst| {
        let tags: Vec<String> = with_dm(lua, |dm| {
            let mut all = std::collections::BTreeSet::new();
            for d in dm.descendants(dm.root()) {
                for t in dm.tags_of(d) {
                    all.insert(t);
                }
            }
            all.into_iter().collect()
        })?;
        lua.create_sequence_from(tags)
    });
    method!(lua, t, "GetInstanceAddedSignal", |lua, (_this, tag): (LInst, String)| {
        let host = host_table(lua)?;
        let f: Function = host.raw_get("getTagSignal")?;
        f.call::<Value>((tag, "added"))
    });
    method!(lua, t, "GetInstanceRemovedSignal", |lua, (_this, tag): (LInst, String)| {
        let host = host_table(lua)?;
        let f: Function = host.raw_get("getTagSignal")?;
        f.call::<Value>((tag, "removed"))
    });

    // ── HttpService ──────────────────────────────────────────────────────
    method!(lua, t, "JSONEncode", |lua, (_this, v): (LInst, Value)| {
        use mlua::LuaSerdeExt;
        let json: serde_json::Value = lua.from_value(v)?;
        serde_json::to_string(&json).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
    });
    method!(lua, t, "JSONDecode", |lua, (_this, s): (LInst, String)| {
        use mlua::LuaSerdeExt;
        let json: serde_json::Value =
            serde_json::from_str(&s).map_err(|e| mlua::Error::RuntimeError(format!("Can't parse JSON: {}", e)))?;
        lua.to_value(&json)
    });
    method!(lua, t, "GenerateGUID", |_, (_this, wrap): (LInst, Option<bool>)| {
        let mut r = super::types_ext::LuauRandom::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(7),
        );
        let hex: String = (0..32).map(|_| format!("{:x}", r.next_u32() % 16)).collect();
        let g = format!("{}-{}-4{}-{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[16..20], &hex[20..32]).to_uppercase();
        Ok(if wrap.unwrap_or(true) { format!("{{{}}}", g) } else { g })
    });
    method!(lua, t, "UrlEncode", |_, (_this, s): (LInst, String)| {
        Ok(s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
            .collect::<String>())
    });

    // ── SoundService ─────────────────────────────────────────────────────
    method!(lua, t, "PlayLocalSound", |lua, (_this, s): (LInst, LInst)| sound(lua, s.0, SoundAction::Play));

    lua.set_named_registry_value(METHODS, t)?;
    Ok(())
}

/// Wrapper so methods can return a CFrame without naming the userdata type.
struct LuaCFrameOut(CFrame);

impl mlua::IntoLua for LuaCFrameOut {
    fn into_lua(self, lua: &Lua) -> LuaResult<Value> {
        Ok(Value::UserData(lua.create_userdata(LuauCFrame(self.0))?))
    }
}

fn sound(lua: &Lua, id: InstanceId, action: SoundAction) -> LuaResult<()> {
    with_dm(lua, |dm| {
        let playing = matches!(action, SoundAction::Play | SoundAction::Resume);
        let _ = dm.set_prop(id, "Playing", DmValue::Bool(playing));
        if action == SoundAction::Play {
            let _ = dm.set_prop(id, "TimePosition", DmValue::Number(0.0));
        }
        dm.sound_commands.push(SoundCommand { sound: id, action });
    })
}

// ============================================================================
// Pivots and bounds
// ============================================================================

/// `GetPivot`: a part's CFrame; a model's PrimaryPart CFrame, else the
/// centre of its parts' bounds.
pub fn pivot_of(dm: &DataModel, id: InstanceId) -> CFrame {
    if let Some(cf) = dm.get(id).and_then(|i| if is_base_part(&i.class_name) { i.cframe() } else { None }) {
        return cf;
    }
    if let Some(DmValue::Instance(pp)) = dm.get_prop(id, "PrimaryPart") {
        if let Some(cf) = dm.get(pp).and_then(|i| i.cframe()) {
            return cf;
        }
    }
    if let Some(DmValue::CFrame(cf)) = dm.get_prop(id, "WorldPivot") {
        if cf != CFrame::IDENTITY {
            return cf;
        }
    }
    let (center, _) = bounding_box(dm, id);
    CFrame::from_position(center)
}

/// `PivotTo`: move every part so the pivot lands on `target`, keeping
/// their placement relative to it.
pub fn pivot_to(dm: &mut DataModel, id: InstanceId, target: CFrame) -> Result<(), String> {
    let current = pivot_of(dm, id);
    let delta = target * current.inverse();
    let mut parts: Vec<InstanceId> = Vec::new();
    if dm.get(id).map_or(false, |i| is_base_part(&i.class_name)) {
        parts.push(id);
    }
    for d in dm.descendants(id) {
        if dm.get(d).map_or(false, |i| is_base_part(&i.class_name)) {
            parts.push(d);
        }
    }
    for p in parts {
        if let Some(cf) = dm.get(p).and_then(|i| i.cframe()) {
            dm.set_prop(p, "CFrame", DmValue::CFrame(delta * cf))?;
        }
    }
    if dm.get(id).map_or(false, |i| class_is_a(&i.class_name, "Model")) {
        let _ = dm.set_prop(id, "WorldPivot", DmValue::CFrame(target));
    }
    Ok(())
}

/// Axis-aligned bounds of every part at or under `id`.
pub fn bounding_box(dm: &DataModel, id: InstanceId) -> (Vector3, Vector3) {
    let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    let mut visit = |inst: InstanceId| {
        let Some(i) = dm.get(inst) else { return };
        if !is_base_part(&i.class_name) {
            return;
        }
        let Some(cf) = i.cframe() else { return };
        let size = i.props.get("Size").and_then(DmValue::as_vector3).unwrap_or(Vector3::ONE);
        let h = size * 0.5;
        let r = cf.right_vector();
        let u = cf.up_vector();
        let b = cf.back_vector();
        let ext = Vector3::new(
            (r.x * h.x).abs() + (u.x * h.y).abs() + (b.x * h.z).abs(),
            (r.y * h.x).abs() + (u.y * h.y).abs() + (b.y * h.z).abs(),
            (r.z * h.x).abs() + (u.z * h.y).abs() + (b.z * h.z).abs(),
        );
        min = min.min(&(cf.position - ext));
        max = max.max(&(cf.position + ext));
        any = true;
    };
    visit(id);
    for d in dm.descendants(id) {
        visit(d);
    }
    if !any {
        return (Vector3::ZERO, Vector3::ZERO);
    }
    ((min + max) * 0.5, max - min)
}

fn filter_rejects(dm: &DataModel, id: InstanceId, params: Option<&LuauRaycastParams>) -> bool {
    let Some(p) = params else { return false };
    let listed = p.filter.iter().any(|f| *f == id || dm.is_descendant_of(id, *f));
    if p.include {
        !listed
    } else {
        listed
    }
}

fn parts_in_radius(dm: &DataModel, center: Vector3, radius: f64, params: Option<&LuauRaycastParams>) -> Vec<InstanceId> {
    let Some(ws) = dm.find_service("Workspace") else { return Vec::new() };
    dm.descendants(ws)
        .into_iter()
        .filter(|d| {
            let Some(i) = dm.get(*d) else { return false };
            if !is_base_part(&i.class_name) || filter_rejects(dm, *d, params) {
                return false;
            }
            let Some(cf) = i.cframe() else { return false };
            let size = i.props.get("Size").and_then(DmValue::as_vector3).unwrap_or(Vector3::ONE);
            // Sphere against the part's bounding sphere.
            (cf.position - center).magnitude() <= radius + size.magnitude() * 0.5
        })
        .collect()
}

fn parts_in_box(dm: &DataModel, cf: CFrame, size: Vector3, params: Option<&LuauRaycastParams>) -> Vec<InstanceId> {
    let Some(ws) = dm.find_service("Workspace") else { return Vec::new() };
    let h = size * 0.5;
    dm.descendants(ws)
        .into_iter()
        .filter(|d| {
            let Some(i) = dm.get(*d) else { return false };
            if !is_base_part(&i.class_name) || filter_rejects(dm, *d, params) {
                return false;
            }
            let Some(pcf) = i.cframe() else { return false };
            let psize = i.props.get("Size").and_then(DmValue::as_vector3).unwrap_or(Vector3::ONE);
            let local = cf.point_to_object_space(pcf.position);
            let r = psize.magnitude() * 0.5;
            local.x.abs() <= h.x + r && local.y.abs() <= h.y + r && local.z.abs() <= h.z + r
        })
        .collect()
}

// ============================================================================
// Raycasts through the engine hook
// ============================================================================

/// One ray against the physics world, as the engine hook receives it.
#[derive(Debug, Clone)]
pub struct RayQuery {
    pub origin: Vector3,
    /// Direction scaled to the ray's length (Roblox semantics).
    pub direction: Vector3,
    /// `true`: only `filter` entities can be hit; `false`: they are skipped.
    pub include: bool,
    /// Entity bits of the filter instances and all their descendants.
    pub filter: Vec<u64>,
    pub respect_can_collide: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity: u64,
    pub position: Vector3,
    pub normal: Vector3,
    pub distance: f64,
}

/// The engine's ray caster for the current frame. It borrows the frame's
/// physics queries, hence the lifetime.
pub type RaycastFn<'a> = dyn Fn(&RayQuery) -> Option<RayHit> + 'a;

thread_local! {
    static RAYCASTER: std::cell::Cell<Option<*const RaycastFn<'static>>> = const { std::cell::Cell::new(None) };
}

/// Make `f` the ray caster for everything run inside `body`. Every entry
/// into the VM goes through this, so `workspace:Raycast` answers against
/// the current physics state, synchronously.
pub fn with_raycaster<R>(f: &RaycastFn<'_>, body: impl FnOnce() -> R) -> R {
    struct Reset(Option<*const RaycastFn<'static>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            RAYCASTER.with(|c| c.set(self.0));
        }
    }
    // SAFETY: the pointer is only dereferenced while `body` runs, and the
    // guard restores the previous value before `f` goes out of scope, even
    // if `body` unwinds.
    let erased: *const RaycastFn<'static> =
        unsafe { std::mem::transmute::<&RaycastFn<'_>, &'static RaycastFn<'static>>(f) };
    let prev = RAYCASTER.with(|c| c.replace(Some(erased)));
    let _reset = Reset(prev);
    body()
}

fn cast(query: &RayQuery) -> Option<RayHit> {
    RAYCASTER.with(|c| c.get()).and_then(|p| {
        // SAFETY: see `with_raycaster`.
        let f: &RaycastFn<'static> = unsafe { &*p };
        f(query)
    })
}

fn raycast(lua: &Lua, origin: Vector3, direction: Vector3, params: LuauRaycastParams) -> LuaResult<Value> {
    let query = with_dm(lua, |dm| {
        let mut filter = Vec::new();
        for f in &params.filter {
            if let Some(e) = dm.entity_of(*f) {
                filter.push(e);
            }
            for d in dm.descendants(*f) {
                if let Some(e) = dm.entity_of(d) {
                    filter.push(e);
                }
            }
        }
        RayQuery {
            origin,
            direction,
            include: params.include,
            filter,
            respect_can_collide: params.respect_can_collide,
        }
    })?;
    let Some(hit) = cast(&query) else { return Ok(Value::Nil) };
    let (inst, material) = with_dm(lua, |dm| {
        let inst = dm.by_entity(hit.entity);
        let material = inst
            .and_then(|i| dm.get_prop(i, "Material"))
            .unwrap_or(DmValue::Enum(EnumItem::new("Material", "Plastic")));
        (inst, material)
    })?;
    let result = lua.create_table()?;
    result.raw_set("Instance", opt_handle(lua, inst)?)?;
    result.raw_set("Position", LuauVector3(hit.position))?;
    result.raw_set("Normal", LuauVector3(hit.normal))?;
    result.raw_set("Distance", hit.distance)?;
    result.raw_set("Material", to_lua(lua, &material)?)?;
    let meta = lua.create_table()?;
    meta.raw_set("__type", "RaycastResult")?;
    result.set_metatable(Some(meta));
    Ok(Value::Table(result))
}

fn legacy_find_part(lua: &Lua, ray: LuauRay, params: LuauRaycastParams) -> LuaResult<(Value, LuauVector3, LuauVector3)> {
    let end = ray.origin + ray.direction;
    let hit = raycast(lua, ray.origin, ray.direction, params)?;
    if let Value::Table(t) = hit {
        let inst: Value = t.raw_get("Instance")?;
        let pos: LuauVector3 = t.raw_get("Position")?;
        let n: LuauVector3 = t.raw_get("Normal")?;
        return Ok((inst, pos, n));
    }
    Ok((Value::Nil, LuauVector3(end), LuauVector3(Vector3::ZERO)))
}

/// `Instance.new(class, parent?)`.
pub fn install_instance_global(lua: &Lua, globals: &Table) -> LuaResult<()> {
    let instance = lua.create_table()?;
    instance.raw_set(
        "new",
        lua.create_function(|lua, (class, parent): (String, Option<LInst>)| {
            let id = with_dm(lua, |dm| {
                let id = dm.create(&class);
                if let Some(p) = parent {
                    let _ = dm.set_parent(id, Some(p.0));
                }
                id
            })?;
            handle(lua, id)
        })?,
    )?;
    globals.set("Instance", instance)?;
    Ok(())
}

/// The interned `Enum.<ty>.<name>` item.
pub fn enum_value_of(lua: &Lua, ty: &str, name: &str) -> LuaResult<Value> {
    enum_item(lua, &EnumItem::new(ty, name))
}
