//! # Luau in Play
//!
//! One sandboxed Luau VM per Play session, bound to the live
//! [`DataModel`](crate::datamodel::DataModel). Scripts see Roblox globals
//! (`game`, `workspace`, `script`, `Instance`, `Enum`, `task`, the value
//! types) whose instances are handles into that tree, so a script moves real
//! parts, creates real parts, and reads positions physics produced.
//!
//! ## What the engine does each frame
//!
//! ```text
//! engine pulls ECS -> DataModel      (poses, input, mouse, camera, collisions)
//! PlayLuau::frame(raycaster)         (input + queued events, RenderStepped,
//!                                     Stepped, task scheduler, tweens,
//!                                     Heartbeat, events the scripts caused)
//! engine applies DataModel -> ECS    (spawns, writes, destroys, commands)
//! ```
//!
//! Every entry into the VM goes through [`instance::with_raycaster`], so
//! `workspace:Raycast` answers synchronously against the current physics.
//!
//! ## Differences from Roblox worth knowing
//!
//! - One process: Scripts and LocalScripts share this VM (each with its own
//!   environment and `script`), RemoteEvents loop back, `IsServer` and
//!   `IsClient` are both true.
//! - Units are metres (Eustress is metre-native).
//! - A script that runs 5 s without yielding is stopped with Roblox's
//!   "Script timeout" error instead of freezing Studio.

pub mod convert;
pub mod instance;
pub mod types_ext;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use mlua::{Function, Lua, Table, Value, Variadic};

use crate::datamodel::{DmEvent, DmValue, EnumItem, InputPhase, InstanceId, OutputLevel, SharedDataModel};
use crate::luau::types::{LuauCFrame, LuauColor3, LuauUDim2, LuauVector3, UserDataPeek};
use crate::scripting::{CFrame, Color3, Vector3};

pub use instance::{handle, with_raycaster, LInst, RayHit, RayQuery, RaycastFn};

const PRELUDE: &str = include_str!("prelude.luau");

/// Longest a script may run without yielding before it is stopped.
const SCRIPT_TIMEOUT_MS: u64 = 5_000;

/// A script to start on Play.
#[derive(Debug, Clone)]
pub struct ScriptLaunch {
    /// The script's instance (becomes `script`).
    pub instance: InstanceId,
    pub source: String,
    /// Shown in errors and tracebacks, e.g. `ServerScriptService.GameDirector`.
    pub chunk_name: String,
}

/// The Play VM.
pub struct PlayLuau {
    lua: Lua,
    dm: SharedDataModel,
    host: Table,
    event_cursor: u64,
    listeners: Arc<parking_lot::Mutex<HashSet<(u64, String)>>>,
    entry_started_ms: Arc<AtomicU64>,
    started: HashSet<InstanceId>,
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn lua_err(e: mlua::Error) -> String {
    e.to_string()
}

impl PlayLuau {
    /// A fresh VM bound to `dm`. `dm` should already hold the seeded tree.
    pub fn new(dm: SharedDataModel) -> Result<Self, String> {
        let lua = Lua::new();
        lua.sandbox(true).map_err(lua_err)?;
        lua.set_app_data(dm.clone());

        let listeners: Arc<parking_lot::Mutex<HashSet<(u64, String)>>> = Arc::new(parking_lot::Mutex::new(HashSet::new()));
        let entry_started_ms = Arc::new(AtomicU64::new(now_ms()));

        // Runaway-script guard. The interrupt fires often, so the clock is
        // read only every 4096 calls.
        {
            let started = entry_started_ms.clone();
            let ticks = Arc::new(AtomicU32::new(0));
            lua.set_interrupt(move |_| {
                if ticks.fetch_add(1, Ordering::Relaxed) & 4095 == 0 {
                    let elapsed = now_ms().saturating_sub(started.load(Ordering::Relaxed));
                    if elapsed > SCRIPT_TIMEOUT_MS {
                        started.store(now_ms(), Ordering::Relaxed);
                        return Err(mlua::Error::RuntimeError(
                            "Script timeout: exhausted allowed execution time".into(),
                        ));
                    }
                }
                Ok(mlua::VmState::Continue)
            });
        }

        let host = Self::install(&lua, &dm, &listeners).map_err(lua_err)?;
        Ok(Self {
            lua,
            dm,
            host,
            event_cursor: 0,
            listeners,
            entry_started_ms,
            started: HashSet::new(),
        })
    }

    fn install(lua: &Lua, dm: &SharedDataModel, listeners: &Arc<parking_lot::Mutex<HashSet<(u64, String)>>>) -> mlua::Result<Table> {
        let globals = lua.globals();

        // Registry caches.
        let handles = lua.create_table()?;
        let weak = lua.create_table()?;
        weak.raw_set("__mode", "v")?;
        handles.set_metatable(Some(weak));
        lua.set_named_registry_value(instance::HANDLES, handles)?;
        lua.set_named_registry_value("__eus_enum_items", lua.create_table()?)?;

        // Value types, then Roblox-compatible constructors that accept the
        // omitted arguments real scripts use (`Vector3.new()`).
        crate::luau::types::inject_types(lua)?;
        types_ext::install(lua, &globals)?;
        let v3: Table = globals.get("Vector3")?;
        v3.raw_set("new", lua.create_function(|_, (x, y, z): (Option<f64>, Option<f64>, Option<f64>)| {
            Ok(LuauVector3(Vector3::new(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0))))
        })?)?;
        v3.raw_set("FromNormalId", lua.create_function(|_, n: Value| {
            let name = convert::enum_name_arg(&n).unwrap_or_default();
            Ok(LuauVector3(match name.as_str() {
                "Right" => Vector3::new(1.0, 0.0, 0.0),
                "Left" => Vector3::new(-1.0, 0.0, 0.0),
                "Top" => Vector3::new(0.0, 1.0, 0.0),
                "Bottom" => Vector3::new(0.0, -1.0, 0.0),
                "Back" => Vector3::new(0.0, 0.0, 1.0),
                _ => Vector3::new(0.0, 0.0, -1.0),
            }))
        })?)?;
        let c3: Table = globals.get("Color3")?;
        c3.raw_set("new", lua.create_function(|_, (r, g, b): (Option<f64>, Option<f64>, Option<f64>)| {
            Ok(LuauColor3(Color3::new(r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0))))
        })?)?;
        c3.raw_set("fromRGB", lua.create_function(|_, (r, g, b): (Option<f64>, Option<f64>, Option<f64>)| {
            let c = |v: Option<f64>| v.unwrap_or(0.0).clamp(0.0, 255.0) / 255.0;
            Ok(LuauColor3(Color3::new(c(r), c(g), c(b))))
        })?)?;
        let ud2: Table = globals.get("UDim2")?;
        ud2.raw_set("new", lua.create_function(|_, (a, b, c, d): (Option<Value>, Option<Value>, Option<f64>, Option<f64>)| {
            // UDim2.new(UDim, UDim) or UDim2.new(xs, xo, ys, yo)
            let udim = |v: &Option<Value>| match v {
                Some(Value::UserData(ud)) => ud.peek::<crate::luau::types::LuauUDim>().ok().map(|u| (u.scale, u.offset)),
                _ => None,
            };
            if let Some((xs, xo)) = udim(&a) {
                let (ys, yo) = udim(&b).unwrap_or((0.0, 0.0));
                return Ok(LuauUDim2::new(xs, xo, ys, yo));
            }
            let num = |v: &Option<Value>| match v {
                Some(Value::Number(n)) => *n,
                Some(Value::Integer(i)) => *i as f64,
                _ => 0.0,
            };
            Ok(LuauUDim2::new(num(&a), num(&b), c.unwrap_or(0.0), d.unwrap_or(0.0)))
        })?)?;
        let cf: Table = globals.get("CFrame")?;
        cf.raw_set("fromMatrix", lua.create_function(|_, (p, x, y, z): (LuauVector3, LuauVector3, LuauVector3, Option<LuauVector3>)| {
            let z = z.map(|z| z.0).unwrap_or_else(|| x.0.cross(&y.0).unit());
            Ok(LuauCFrame(CFrame::from_matrix(p.0, x.0, y.0, z)))
        })?)?;
        cf.raw_set("lookAlong", lua.create_function(|_, (p, dir, up): (LuauVector3, LuauVector3, Option<LuauVector3>)| {
            Ok(LuauCFrame(CFrame::look_at(p.0, p.0 + dir.0, up.map(|u| u.0))))
        })?)?;

        // Instances.
        instance::install_methods(lua)?;
        instance::install_instance_global(lua, &globals)?;

        // Host functions the prelude calls.
        let host_fns = lua.create_table()?;
        {
            let dm = dm.clone();
            host_fns.raw_set("output", lua.create_function(move |_, (level, text): (String, String)| {
                let level = match level.as_str() {
                    "error" => OutputLevel::Error,
                    "warn" => OutputLevel::Warn,
                    _ => OutputLevel::Info,
                };
                dm.lock().print(level, "Luau", text);
                Ok(())
            })?)?;
        }
        {
            let listeners = listeners.clone();
            let dm = dm.clone();
            host_fns.raw_set("listen", lua.create_function(move |_, (id, name): (f64, String)| {
                let id = InstanceId(id as u64);
                if matches!(name.as_str(), "Touched" | "TouchEnded") {
                    dm.lock().touch_watch.push(id);
                }
                listeners.lock().insert((id.0, name));
                Ok(())
            })?)?;
        }
        {
            let dm = dm.clone();
            host_fns.raw_set("watch", lua.create_function(move |_, id: f64| {
                dm.lock().watch_changes(InstanceId(id as u64));
                Ok(())
            })?)?;
        }
        host_fns.raw_set("enumItem", lua.create_function(|lua, (ty, name): (String, String)| {
            convert::enum_item(lua, &EnumItem::new(ty, name))
        })?)?;
        {
            let dm = dm.clone();
            host_fns.raw_set("alive", lua.create_function(move |_, v: Value| {
                Ok(match v {
                    Value::UserData(ud) => ud.peek::<LInst>().map(|i| dm.lock().exists(i.0)).unwrap_or(false),
                    _ => false,
                })
            })?)?;
        }
        host_fns.raw_set("idOf", lua.create_function(|_, inst: LInst| Ok(instance::id_key(inst.0)))?)?;
        {
            let dm = dm.clone();
            host_fns.raw_set("fired", lua.create_function(move |_, args: Variadic<Value>| {
                let mut it = args.into_iter();
                let Some(Value::UserData(ud)) = it.next() else { return Ok(()) };
                let Ok(event) = ud.peek::<LInst>().map(|i| i.0) else { return Ok(()) };
                let args: Vec<DmValue> = it.filter_map(|v| convert::from_lua(&v).ok()).collect();
                dm.lock().push_event(DmEvent::Fired { event, args, from_luau: true });
                Ok(())
            })?)?;
        }
        {
            let dm = dm.clone();
            host_fns.raw_set("compileModule", lua.create_function(move |lua, module: LInst| {
                let (source, name) = {
                    let g = dm.lock();
                    let src = g.get_prop(module.0, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    (src, g.full_name(module.0))
                };
                let env = make_env(lua, &dm, Some(module.0), &name)?;
                lua.load(source).set_name(name).set_environment(env).into_function()
            })?)?;
        }
        {
            let dm = dm.clone();
            host_fns.raw_set("mouseGet", lua.create_function(move |lua, key: String| mouse_get(lua, &dm, &key))?)?;
        }
        {
            let dm = dm.clone();
            host_fns.raw_set("mouseSet", lua.create_function(move |_, (key, value): (String, Value)| {
                let mut g = dm.lock();
                match key.as_str() {
                    "TargetFilter" => {
                        g.mouse.target_filter = match value {
                            Value::UserData(ud) => ud.peek::<LInst>().ok().map(|i| i.0),
                            _ => None,
                        };
                    }
                    "Icon" => {
                        if let Value::String(s) = value {
                            g.mouse.icon = s.to_str()?.to_string();
                        }
                    }
                    _ => {}
                }
                Ok(())
            })?)?;
        }
        globals.set("__host", host_fns)?;

        // `game` / `workspace` before the prelude, which reads them lazily.
        let (root, ws) = {
            let mut g = dm.lock();
            let root = g.root();
            let ws = g.get_service("Workspace");
            (root, ws)
        };
        let game = handle(lua, root)?;
        globals.set("game", game.clone())?;
        globals.set("Game", game)?;
        if let Some(ws) = ws {
            let w = handle(lua, ws)?;
            globals.set("workspace", w.clone())?;
            globals.set("Workspace", w)?;
        }

        // The prelude: scheduler, signals, Lua-side services.
        let host: Table = lua.load(PRELUDE).set_name("=EustressPlayPrelude").eval()?;
        lua.set_named_registry_value(instance::HOST, host.clone())?;

        // Globals from the prelude.
        globals.set("task", host.get::<Value>("task")?)?;
        globals.set("wait", host.get::<Value>("wait")?)?;
        globals.set("delay", host.get::<Value>("delay")?)?;
        globals.set("spawn", host.get::<Value>("spawn")?)?;
        globals.set("Enum", host.get::<Value>("Enum")?)?;
        globals.set("TweenInfo", host.get::<Value>("TweenInfo")?)?;
        globals.set("require", host.get::<Value>("require")?)?;
        globals.set("shared", lua.create_table()?)?;
        globals.set("_G", lua.create_table()?)?;
        {
            let dm = dm.clone();
            globals.set("time", lua.create_function(move |_, ()| Ok(dm.lock().frame.time))?)?;
        }
        {
            let dm = dm.clone();
            globals.set("elapsedTime", lua.create_function(move |_, ()| Ok(dm.lock().frame.time))?)?;
        }
        globals.set("tick", lua.create_function(|_, ()| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0))
        })?)?;
        let settings = lua.create_table()?;
        globals.set("settings", lua.create_function(move |_, ()| Ok(settings.clone()))?)?;

        // Lua-implemented instance methods join the Rust ones.
        let methods: Table = lua.named_registry_value(instance::METHODS)?;
        let lua_methods: Table = host.get("luaMethods")?;
        for pair in lua_methods.pairs::<String, Value>() {
            let (k, v) = pair?;
            methods.raw_set(k, v)?;
        }
        Ok(host)
    }

    /// The tree this VM is bound to.
    pub fn datamodel(&self) -> &SharedDataModel {
        &self.dm
    }

    fn begin_entry(&self) {
        self.entry_started_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// Start scripts. Each runs as its own thread with its own environment;
    /// a compile error is reported and the rest still start.
    pub fn run_scripts(&mut self, scripts: Vec<ScriptLaunch>, raycaster: &RaycastFn<'_>) {
        with_raycaster(raycaster, || {
            for s in scripts {
                if !self.started.insert(s.instance) {
                    continue;
                }
                self.begin_entry();
                if let Err(e) = self.spawn_one(&s) {
                    self.dm.lock().print(OutputLevel::Error, &s.chunk_name, e);
                }
            }
            let _ = self.dispatch_events();
        });
    }

    fn spawn_one(&self, s: &ScriptLaunch) -> Result<(), String> {
        let env = make_env(&self.lua, &self.dm, Some(s.instance), &s.chunk_name).map_err(lua_err)?;
        let f = self
            .lua
            .load(s.source.as_str())
            .set_name(s.chunk_name.as_str())
            .set_environment(env)
            .into_function()
            .map_err(lua_err)?;
        let sched: Table = self.host.get("Sched").map_err(lua_err)?;
        let spawn: Function = sched.get("spawn").map_err(lua_err)?;
        spawn.call::<Value>(f).map_err(lua_err)?;
        Ok(())
    }

    /// Run one frame: input, queued events, RunService signals, the task
    /// scheduler and tweens, then the events scripts caused this frame.
    pub fn frame(&mut self, raycaster: &RaycastFn<'_>) {
        with_raycaster(raycaster, || {
            self.begin_entry();
            let (now, dt, inputs) = {
                let g = self.dm.lock();
                (g.frame.time, g.frame.dt, g.input.events.clone())
            };
            if let Err(e) = self.dispatch_input(&inputs) {
                self.dm.lock().print(OutputLevel::Error, "Luau", format!("input dispatch: {}", e));
            }
            if let Err(e) = self.dispatch_events() {
                self.dm.lock().print(OutputLevel::Error, "Luau", format!("event dispatch: {}", e));
            }
            let frame: mlua::Result<Function> = self.host.get("frame");
            match frame {
                Ok(f) => {
                    if let Err(e) = f.call::<()>((now, dt)) {
                        self.dm.lock().print(OutputLevel::Error, "Luau", e.to_string());
                    }
                }
                Err(e) => self.dm.lock().print(OutputLevel::Error, "Luau", e.to_string()),
            }
            if let Err(e) = self.dispatch_events() {
                self.dm.lock().print(OutputLevel::Error, "Luau", format!("event dispatch: {}", e));
            }
        });
    }

    /// Fire only events that something listens to. Returns once the queue
    /// seen at entry has been handled.
    pub fn dispatch_events(&mut self) -> mlua::Result<()> {
        let (events, next) = self.dm.lock().events_since(self.event_cursor);
        self.event_cursor = next;
        if events.is_empty() {
            return Ok(());
        }
        let fire: Function = self.host.get("fireSignal")?;
        let fire_prop: Function = self.host.get("firePropertyChanged")?;
        let fire_attr: Function = self.host.get("fireAttributeChanged")?;
        let fire_tag: Function = self.host.get("fireTagSignal")?;
        let drop_signals: Function = self.host.get("dropSignals")?;
        let lua = &self.lua;
        let listening = |id: InstanceId, name: &str| -> bool {
            self.listeners.lock().contains(&(id.0, name.to_string()))
        };
        for ev in events {
            match ev {
                DmEvent::ChildAdded { parent, child } if listening(parent, "ChildAdded") => {
                    fire.call::<()>((instance::id_key(parent), "ChildAdded", handle(lua, child)?))?;
                }
                DmEvent::ChildRemoved { parent, child } if listening(parent, "ChildRemoved") => {
                    fire.call::<()>((instance::id_key(parent), "ChildRemoved", handle(lua, child)?))?;
                }
                DmEvent::DescendantAdded { ancestor, descendant } if listening(ancestor, "DescendantAdded") => {
                    fire.call::<()>((instance::id_key(ancestor), "DescendantAdded", handle(lua, descendant)?))?;
                }
                DmEvent::DescendantRemoving { ancestor, descendant } if listening(ancestor, "DescendantRemoving") => {
                    fire.call::<()>((instance::id_key(ancestor), "DescendantRemoving", handle(lua, descendant)?))?;
                }
                DmEvent::AncestryChanged { id, parent } if listening(id, "AncestryChanged") => {
                    let p = match parent {
                        Some(p) => handle(lua, p)?,
                        None => Value::Nil,
                    };
                    fire.call::<()>((instance::id_key(id), "AncestryChanged", handle(lua, id)?, p))?;
                }
                DmEvent::Destroying { id } => {
                    if listening(id, "Destroying") {
                        fire.call::<()>((instance::id_key(id), "Destroying"))?;
                    }
                    drop_signals.call::<()>(instance::id_key(id))?;
                }
                DmEvent::Changed { id, prop } => {
                    fire_prop.call::<()>((instance::id_key(id), prop.as_str()))?;
                    if prop == "Health" && listening(id, "HealthChanged") {
                        let hp = self.dm.lock().get_prop(id, "Health").and_then(|v| v.as_number()).unwrap_or(0.0);
                        fire.call::<()>((instance::id_key(id), "HealthChanged", hp))?;
                    }
                }
                DmEvent::AttributeChanged { id, name } => {
                    fire_attr.call::<()>((instance::id_key(id), name))?;
                }
                DmEvent::TagAdded { id, tag } => {
                    fire_tag.call::<()>((tag, "added", handle(lua, id)?))?;
                }
                DmEvent::TagRemoved { id, tag } => {
                    fire_tag.call::<()>((tag, "removed", handle(lua, id)?))?;
                }
                DmEvent::Touched { part, other } if listening(part, "Touched") => {
                    fire.call::<()>((instance::id_key(part), "Touched", handle(lua, other)?))?;
                }
                DmEvent::TouchEnded { part, other } if listening(part, "TouchEnded") => {
                    fire.call::<()>((instance::id_key(part), "TouchEnded", handle(lua, other)?))?;
                }
                DmEvent::PlayerAdded { player } => {
                    let players = self.dm.lock().find_service("Players");
                    if let Some(ps) = players {
                        if listening(ps, "PlayerAdded") {
                            fire.call::<()>((instance::id_key(ps), "PlayerAdded", handle(lua, player)?))?;
                        }
                    }
                }
                DmEvent::PlayerRemoving { player } => {
                    let players = self.dm.lock().find_service("Players");
                    if let Some(ps) = players {
                        if listening(ps, "PlayerRemoving") {
                            fire.call::<()>((instance::id_key(ps), "PlayerRemoving", handle(lua, player)?))?;
                        }
                    }
                }
                DmEvent::CharacterAdded { player, character } if listening(player, "CharacterAdded") => {
                    fire.call::<()>((instance::id_key(player), "CharacterAdded", handle(lua, character)?))?;
                }
                DmEvent::CharacterRemoving { player, character } if listening(player, "CharacterRemoving") => {
                    fire.call::<()>((instance::id_key(player), "CharacterRemoving", handle(lua, character)?))?;
                }
                DmEvent::Died { humanoid } if listening(humanoid, "Died") => {
                    fire.call::<()>((instance::id_key(humanoid), "Died"))?;
                }
                DmEvent::MoveToFinished { humanoid, reached } if listening(humanoid, "MoveToFinished") => {
                    fire.call::<()>((instance::id_key(humanoid), "MoveToFinished", reached))?;
                }
                DmEvent::GuiActivated { button } => {
                    for name in ["MouseButton1Down", "MouseButton1Up", "MouseButton1Click", "Activated"] {
                        if listening(button, name) {
                            fire.call::<()>((instance::id_key(button), name))?;
                        }
                    }
                }
                DmEvent::Fired { event, args, from_luau } if !from_luau && listening(event, "Event") => {
                    let mut vals = Vec::with_capacity(args.len());
                    for a in &args {
                        vals.push(convert::to_lua(lua, a)?);
                    }
                    let mut all: Vec<Value> = vec![Value::Number(instance::id_key(event)), Value::String(lua.create_string("Event")?)];
                    all.extend(vals);
                    fire.call::<()>(mlua::MultiValue::from_vec(all))?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn dispatch_input(&mut self, inputs: &[crate::datamodel::InputEvent]) -> mlua::Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let lua = &self.lua;
        let uis = self.dm.lock().find_service("UserInputService");
        let fire: Function = self.host.get("fireSignal")?;
        let actions: Function = self.host.get("dispatchActions")?;
        let mouse_signals: Table = self.host.get("mouseSignals")?;
        for ev in inputs {
            let obj = lua.create_table()?;
            obj.raw_set("UserInputType", convert::enum_item(lua, &EnumItem::new("UserInputType", ev.input_type.clone()))?)?;
            obj.raw_set("KeyCode", convert::enum_item(lua, &EnumItem::new("KeyCode", ev.key_code.clone()))?)?;
            let state = match ev.phase {
                InputPhase::Began => "Begin",
                InputPhase::Changed => "Change",
                InputPhase::Ended => "End",
            };
            let state_item = convert::enum_item(lua, &EnumItem::new("UserInputState", state))?;
            obj.raw_set("UserInputState", state_item.clone())?;
            obj.raw_set("Position", LuauVector3(Vector3::new(ev.x, ev.y, ev.wheel)))?;
            obj.raw_set("Delta", LuauVector3(Vector3::new(ev.dx, ev.dy, ev.wheel)))?;
            let meta = lua.create_table()?;
            meta.raw_set("__type", "InputObject")?;
            obj.set_metatable(Some(meta));

            if let Some(uis) = uis {
                let signal = match ev.phase {
                    InputPhase::Began => "InputBegan",
                    InputPhase::Changed => "InputChanged",
                    InputPhase::Ended => "InputEnded",
                };
                if self.listeners.lock().contains(&(uis.0, signal.to_string())) {
                    fire.call::<()>((instance::id_key(uis), signal, obj.clone(), ev.game_processed))?;
                }
            }
            if !ev.game_processed {
                actions.call::<()>((state_item, obj.clone()))?;
            }

            // Mouse object signals.
            let mouse_signal = match (ev.input_type.as_str(), ev.phase) {
                ("MouseButton1", InputPhase::Began) => Some("Button1Down"),
                ("MouseButton1", InputPhase::Ended) => Some("Button1Up"),
                ("MouseButton2", InputPhase::Began) => Some("Button2Down"),
                ("MouseButton2", InputPhase::Ended) => Some("Button2Up"),
                ("MouseMovement", _) => Some("Move"),
                ("MouseWheel", _) if ev.wheel > 0.0 => Some("WheelForward"),
                ("MouseWheel", _) if ev.wheel < 0.0 => Some("WheelBackward"),
                _ => None,
            };
            if let Some(name) = mouse_signal {
                if !ev.game_processed {
                    let sig: Table = mouse_signals.get(name)?;
                    let fire_fn: Function = sig.get("Fire")?;
                    fire_fn.call::<()>(sig)?;
                }
            }
        }
        Ok(())
    }

    /// Stop every thread and connection. The VM itself is dropped with `self`.
    pub fn stop(&mut self) {
        if let Ok(reset) = self.host.get::<Function>("reset") {
            let _ = reset.call::<()>(());
        }
        self.started.clear();
    }

    /// Where this VM has read the DataModel event queue up to, so the engine
    /// can trim events every reader has seen.
    pub fn event_cursor(&self) -> u64 {
        self.event_cursor
    }

    /// Luau heap in bytes (diagnostics).
    pub fn memory_used(&self) -> usize {
        self.lua.used_memory()
    }
}

/// A script's environment: its own `script`, `print` and `warn`, reading
/// everything else from the shared globals.
fn make_env(lua: &Lua, dm: &SharedDataModel, script: Option<InstanceId>, name: &str) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    if let Some(s) = script {
        env.raw_set("script", handle(lua, s)?)?;
    }
    for (fname, level) in [("print", OutputLevel::Info), ("warn", OutputLevel::Warn)] {
        let dm = dm.clone();
        let source = name.to_string();
        env.raw_set(fname, lua.create_function(move |lua, args: Variadic<Value>| {
            let text = join_args(lua, args)?;
            dm.lock().print(level, &source, text);
            Ok(())
        })?)?;
    }
    let meta = lua.create_table()?;
    meta.raw_set("__index", lua.globals())?;
    env.set_metatable(Some(meta));
    Ok(env)
}

fn join_args(lua: &Lua, args: Variadic<Value>) -> mlua::Result<String> {
    let mut parts = Vec::with_capacity(args.len());
    let mut tostring: Option<Function> = None;
    for v in args.iter() {
        match v {
            Value::String(s) => parts.push(s.to_str()?.to_string()),
            Value::Number(n) => parts.push(crate::datamodel::format_number(*n)),
            Value::Integer(i) => parts.push(i.to_string()),
            Value::Boolean(b) => parts.push(b.to_string()),
            Value::Nil => parts.push("nil".into()),
            other => {
                if tostring.is_none() {
                    tostring = Some(lua.globals().get("tostring")?);
                }
                let s: String = tostring.as_ref().map(|f| f.call(other.clone())).transpose()?.unwrap_or_default();
                parts.push(s);
            }
        }
    }
    Ok(parts.join(" "))
}

fn mouse_get(lua: &Lua, dm: &SharedDataModel, key: &str) -> mlua::Result<Value> {
    // Copy what is needed and release the lock before touching Lua.
    let (m, x, y, w, h) = {
        let g = dm.lock();
        (g.mouse.clone(), g.input.mouse_x, g.input.mouse_y, g.input.viewport_w, g.input.viewport_h)
    };
    let dir = m.ray_direction.unit();
    Ok(match key {
        "Hit" => {
            let p = if m.has_hit { m.hit_position } else { m.ray_origin + dir * 1000.0 };
            let mut cf = CFrame::look_at(m.ray_origin, p, None);
            cf.position = p;
            Value::UserData(lua.create_userdata(LuauCFrame(cf))?)
        }
        "Origin" => {
            let cf = CFrame::look_at(m.ray_origin, m.ray_origin + dir, None);
            Value::UserData(lua.create_userdata(LuauCFrame(cf))?)
        }
        "Target" => match m.target {
            Some(t) => handle(lua, t)?,
            None => Value::Nil,
        },
        "TargetFilter" => match m.target_filter {
            Some(t) => handle(lua, t)?,
            None => Value::Nil,
        },
        "TargetSurface" => convert::enum_item(lua, &EnumItem::new("NormalId", "Top"))?,
        "UnitRay" => Value::UserData(lua.create_userdata(types_ext::LuauRay { origin: m.ray_origin, direction: dir })?),
        "X" => Value::Number(x),
        "Y" => Value::Number(y),
        "ViewSizeX" => Value::Number(w),
        "ViewSizeY" => Value::Number(h),
        "Icon" => Value::String(lua.create_string(&m.icon)?),
        "Name" => Value::String(lua.create_string("Mouse")?),
        "ClassName" => Value::String(lua.create_string("PlayerMouse")?),
        _ => Value::Nil,
    })
}
