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
//! engine pulls ECS -> DataModel        (poses, input, mouse, camera,
//!                                       collisions)
//! PlayLuau::frame(raycaster, terrain)  (input + queued events, RenderStepped,
//!                                       Stepped, task scheduler, tweens,
//!                                       Heartbeat, events the scripts caused)
//! engine applies DataModel -> ECS      (spawns, writes, destroys, commands,
//!                                       terrain edits)
//! ```
//!
//! Every entry into the VM goes through [`instance::with_raycaster`] and
//! [`terrain::with_terrain_reader`], so `workspace:Raycast` and the
//! Terrain's reads answer synchronously against the current physics and
//! terrain.
//!
//! ## Differences from Roblox worth knowing
//!
//! - One process: Scripts and LocalScripts share this VM (each with its own
//!   environment and `script`), RemoteEvents loop back, `IsServer` and
//!   `IsClient` are both true.
//! - Units are metres (Eustress is metre-native), terrain voxel
//!   resolutions included.
//! - Terrain writes apply after the frame's scripts have run; a read in the
//!   same frame sees the terrain as the frame began.
//! - A script that runs 10 s without yielding (Roblox's budget) is stopped
//!   with "Script timeout at <script>:<line>" instead of freezing Studio,
//!   and a `pcall` around the loop can't hold the stop off.
//! - Output lines point at the code they came from: the script's file, the
//!   line and, for an error, the stack. Each script's start, the end of its
//!   top-level code, and a thread that will never resume are reported too.

pub mod convert;
pub mod instance;
pub mod terrain;
pub mod types_ext;
/// Studs at the boundary, for scripts written for Roblox.
pub mod studs;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use mlua::{Function, Lua, Table, Value, Variadic};

use crate::datamodel::{
    CommerceStatus, DmEvent, DmValue, EnumItem, InputPhase, InstanceId, OutputLevel, ProductKind, PurchasePrompt,
    ReceiptDecision, RemoteCall, RemoteReply, RemoteTarget, RemoteValue, SharedDataModel, INVOKE_TIMEOUT_SECS,
};
use crate::luau::types::{LuauCFrame, LuauColor3, LuauUDim2, LuauVector3, UserDataPeek};
use crate::scripting::{CFrame, Color3, Vector3};

pub use instance::{handle, with_raycaster, LInst, RayHit, RayQuery, RaycastFn};
pub use terrain::{with_terrain_reader, TerrainReadFn, TerrainView};

const PRELUDE: &str = include_str!("prelude.luau");

/// Longest a script may run without yielding before it is stopped, as in
/// Roblox. [`PlayLuau::set_script_timeout`] changes it for one VM.
const SCRIPT_TIMEOUT_MS: u64 = 10_000;

/// The prelude's chunk name, which stacks and positions skip.
const PRELUDE_CHUNK: &str = "EustressPlayPrelude";

/// Registry key of the prelude's `coroutine` wrapper, which ties a coroutine
/// to the script that made it.
const OWNED_COROUTINE: &str = "__eus_owned_coroutine";

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
    watchdog: Arc<Watchdog>,
    started: HashSet<InstanceId>,
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn lua_err(e: mlua::Error) -> String {
    e.to_string()
}

/// An mlua error as Roblox shows one: the message alone, without mlua's
/// "syntax error: " or "runtime error: " in front.
fn error_text(e: mlua::Error) -> String {
    match e {
        mlua::Error::SyntaxError { message, .. } => message,
        mlua::Error::RuntimeError(message) => message,
        e => e.to_string(),
    }
}

/// The line a message's `Chunk:12:` position names, or 0.
fn error_line(message: &str, chunk: &str) -> u32 {
    message
        .strip_prefix(chunk)
        .and_then(|rest| rest.strip_prefix(':'))
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// The runaway-script guard the VM's interrupt consults.
struct Watchdog {
    /// How long a script may run without yielding.
    budget_ms: AtomicU64,
    /// When the host last resumed a script or began an entry.
    started_ms: AtomicU64,
    /// Interrupts so far. The interrupt fires at every call, return and loop
    /// back-edge, so the clock is read only on every 4096th.
    ticks: AtomicU32,
    /// The running resume has used up its budget. Every script thread under
    /// it then stops at its next interrupt, so a `pcall` around the loop
    /// can't catch the timeout and carry on.
    tripped: AtomicBool,
    /// The timeout, naming where the script was.
    message: parking_lot::Mutex<String>,
    /// The VM's main thread, by address: the host's own code, which a
    /// script's timeout never stops.
    main: usize,
}

impl Watchdog {
    /// A fresh budget, as the host resumes a script or begins an entry.
    fn restart(&self) {
        self.started_ms.store(now_ms(), Ordering::Relaxed);
        self.tripped.store(false, Ordering::Relaxed);
    }

    /// The interrupt: carry on, or stop the running script.
    fn check(&self, lua: &Lua) -> mlua::Result<mlua::VmState> {
        let tripped = self.tripped.load(Ordering::Relaxed);
        if !tripped {
            if (self.ticks.fetch_add(1, Ordering::Relaxed) & 4095) != 0 {
                return Ok(mlua::VmState::Continue);
            }
            let ran = now_ms().saturating_sub(self.started_ms.load(Ordering::Relaxed));
            if ran <= self.budget_ms.load(Ordering::Relaxed) {
                return Ok(mlua::VmState::Continue);
            }
        }
        if lua.current_thread().to_pointer() as usize == self.main {
            // The host's own code. After a timeout the stopped script has
            // unwound to here, and the scheduler carries on; otherwise the
            // host's entry itself ran long, and that stops.
            self.restart();
            return if tripped {
                Ok(mlua::VmState::Continue)
            } else {
                Err(mlua::Error::RuntimeError(self.timeout_message(lua)))
            };
        }
        if !tripped {
            *self.message.lock() = self.timeout_message(lua);
            self.tripped.store(true, Ordering::Relaxed);
        }
        Err(mlua::Error::RuntimeError(self.message.lock().clone()))
    }

    /// Where the script was when its budget ran out.
    fn timeout_message(&self, lua: &Lua) -> String {
        let secs = self.budget_ms.load(Ordering::Relaxed) as f64 / 1000.0;
        match script_position(lua, 0) {
            Some((chunk, line)) => format!(
                "Script timeout at {chunk}:{line}: exhausted allowed execution time ({secs} s without yielding)"
            ),
            None => format!("Script timeout: exhausted allowed execution time ({secs} s without yielding)"),
        }
    }
}

/// The innermost script code on the running thread's stack, from level
/// `from` out: its chunk name (the script's path) and line. Host functions
/// and the prelude are skipped.
fn script_position(lua: &Lua, from: usize) -> Option<(String, u32)> {
    for level in from..from + 32 {
        let frame = lua.inspect_stack(level)?;
        let line = frame.curr_line();
        if line <= 0 {
            continue;
        }
        let source = frame.source();
        match source.short_src.as_deref() {
            Some(chunk) if chunk != PRELUDE_CHUNK && chunk != "[C]" => return Some((chunk.to_string(), line as u32)),
            _ => {}
        }
    }
    None
}

/// Which script each compiled chunk is, by chunk name (the script's path),
/// so an Output line points at the file of the code it came from, a
/// ModuleScript's included.
#[derive(Default)]
struct ChunkScripts(parking_lot::Mutex<HashMap<String, InstanceId>>);

fn register_chunk(lua: &Lua, chunk: &str, script: InstanceId) {
    if let Some(chunks) = lua.app_data_ref::<ChunkScripts>() {
        chunks.0.lock().insert(chunk.to_string(), script);
    }
}

fn script_of_chunk(lua: &Lua, chunk: &str) -> Option<InstanceId> {
    let chunks = lua.app_data_ref::<ChunkScripts>()?;
    let script = chunks.0.lock().get(chunk).copied();
    script
}

impl PlayLuau {
    /// A fresh VM bound to `dm`. `dm` should already hold the seeded tree.
    pub fn new(dm: SharedDataModel) -> Result<Self, String> {
        let lua = Lua::new();
        lua.sandbox(true).map_err(lua_err)?;
        lua.set_app_data(dm.clone());

        let listeners: Arc<parking_lot::Mutex<HashSet<(u64, String)>>> = Arc::new(parking_lot::Mutex::new(HashSet::new()));
        lua.set_app_data(ChunkScripts::default());

        // Runaway-script guard.
        let watchdog = Arc::new(Watchdog {
            budget_ms: AtomicU64::new(SCRIPT_TIMEOUT_MS),
            started_ms: AtomicU64::new(now_ms()),
            ticks: AtomicU32::new(0),
            tripped: AtomicBool::new(false),
            message: parking_lot::Mutex::new(String::new()),
            main: lua.current_thread().to_pointer() as usize,
        });
        {
            let watchdog = watchdog.clone();
            lua.set_interrupt(move |lua| watchdog.check(lua));
        }

        let host = Self::install(&lua, &dm, &listeners, &watchdog).map_err(lua_err)?;
        Ok(Self {
            lua,
            dm,
            host,
            event_cursor: 0,
            listeners,
            watchdog,
            started: HashSet::new(),
        })
    }

    fn install(
        lua: &Lua,
        dm: &SharedDataModel,
        listeners: &Arc<parking_lot::Mutex<HashSet<(u64, String)>>>,
        watchdog: &Arc<Watchdog>,
    ) -> mlua::Result<Table> {
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
        // Metres in a stud, for the speeds Humanoid signals hand a script
        // written for Roblox.
        host_fns.raw_set("studMetres", studs::stud_metres())?;
        {
            let dm = dm.clone();
            // `host.output(level, text, source?, chunk?, line?, stack?)`: the
            // line points at `chunk`'s script, whose file Output shows.
            type OutputArgs = (String, String, Option<String>, Option<String>, Option<f64>, Option<Vec<String>>);
            host_fns.raw_set("output", lua.create_function(move |lua, (level, text, source, chunk, line, stack): OutputArgs| {
                let level = match level.as_str() {
                    "error" => OutputLevel::Error,
                    "warn" => OutputLevel::Warn,
                    _ => OutputLevel::Info,
                };
                let script = chunk.as_deref().and_then(|c| script_of_chunk(lua, c));
                let line = line.map_or(0, |l| l.max(0.0) as u32);
                let source = source.as_deref().unwrap_or("Luau");
                dm.lock().print_from(level, script, source, text, line, stack.unwrap_or_default());
                Ok(())
            })?)?;
        }
        {
            // A script's top-level code finishing or stopping.
            let dm = dm.clone();
            host_fns.raw_set("lifecycle", lua.create_function(move |_, (text, source, owner): (String, Option<String>, Option<f64>)| {
                let script = owner.map(|key| InstanceId(key as u64));
                dm.lock().print_lifecycle(OutputLevel::Info, script, source.as_deref().unwrap_or("Luau"), text);
                Ok(())
            })?)?;
        }
        {
            // A fresh run budget, as the host resumes a script.
            let watchdog = watchdog.clone();
            host_fns.raw_set("newBudget", lua.create_function(move |_, ()| {
                watchdog.restart();
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
                register_chunk(lua, &name, module.0);
                lua.load(source).set_name(format!("={name}")).set_environment(env).into_function()
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
        install_remote(lua, &host_fns, dm)?;
        install_commerce(lua, &host_fns, dm)?;
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
        let host: Table = lua.load(PRELUDE).set_name(format!("={PRELUDE_CHUNK}")).eval()?;
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
        // Scripts see `coroutine` through the prelude's owned wrapper, so a
        // coroutine a script makes stops with that script.
        lua.set_named_registry_value(OWNED_COROUTINE, host.get::<Value>("ownedCoroutine")?)?;
        Ok(host)
    }

    /// The tree this VM is bound to.
    pub fn datamodel(&self) -> &SharedDataModel {
        &self.dm
    }

    fn begin_entry(&self) {
        self.watchdog.restart();
    }

    /// Start scripts. Each runs as its own thread with its own environment;
    /// a compile error is reported and the rest still start.
    pub fn run_scripts(&mut self, scripts: Vec<ScriptLaunch>, raycaster: &RaycastFn<'_>, terrain: &TerrainReadFn<'_>) {
        with_terrain_reader(terrain, || {
            with_raycaster(raycaster, || {
                for s in scripts {
                    if !self.started.insert(s.instance) {
                        continue;
                    }
                    self.begin_entry();
                    if let Err(e) = self.spawn_one(&s) {
                        // A compile error names its line: "Path:5: Expected 'end' ...".
                        let line = error_line(&e, &s.chunk_name);
                        let text = format!("{e} (the script did not start)");
                        self.dm.lock().print_from(OutputLevel::Error, Some(s.instance), &s.chunk_name, text, line, Vec::new());
                    }
                }
                let _ = self.dispatch_events();
            })
        });
    }

    fn spawn_one(&self, s: &ScriptLaunch) -> Result<(), String> {
        let env = make_env(&self.lua, &self.dm, Some(s.instance), &s.chunk_name).map_err(lua_err)?;
        let f = self
            .lua
            .load(s.source.as_str())
            .set_name(format!("={}", s.chunk_name))
            .set_environment(env)
            .into_function()
            .map_err(error_text)?;
        register_chunk(&self.lua, &s.chunk_name, s.instance);
        self.dm.lock().print_lifecycle(OutputLevel::Info, Some(s.instance), &s.chunk_name, "Started");
        // Through startScript, so every thread and connection the script
        // makes is its own and stops when the script is destroyed.
        let start: Function = self.host.get("startScript").map_err(lua_err)?;
        // A script written for Roblox works in studs; the boundary converts
        // for every thread it owns.
        let in_studs = studs::script_writes_studs(&self.dm.lock(), s.instance);
        start.call::<Value>((instance::id_key(s.instance), f, in_studs, s.chunk_name.as_str())).map_err(lua_err)?;
        Ok(())
    }

    /// Run one frame: input, queued events, RunService signals, the task
    /// scheduler and tweens, then the events scripts caused this frame.
    pub fn frame(&mut self, raycaster: &RaycastFn<'_>, terrain: &TerrainReadFn<'_>) {
        with_terrain_reader(terrain, || {
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
                // Before the scheduler step below, so a handler this delivery
                // starts, and a parked `InvokeServer` its answer frees, both
                // run in this frame rather than the next.
                if let Err(e) = self.dispatch_remotes() {
                    self.dm.lock().print(OutputLevel::Error, "Luau", format!("remote dispatch: {}", e));
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
            })
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
        let stop_script: Function = self.host.get("stopScript")?;
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
                    // With its name, so a thread left waiting on one of its
                    // signals is reported: it never resumes.
                    let name = self.dm.lock().get(id).map(|i| i.name.clone());
                    drop_signals.call::<()>((instance::id_key(id), true, name))?;
                    // A running script that is destroyed stops, as in Roblox:
                    // its threads end and its connections drop, wherever they
                    // were made. A PlayerGui reset on respawn destroys the old
                    // copies, and without this each one's HUD script would
                    // keep running beside the fresh copy.
                    if self.started.contains(&id) {
                        stop_script.call::<()>(instance::id_key(id))?;
                    }
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
                // Animation and Humanoid state events: a named signal and its
                // arguments.
                // TextBox focus, which only the GUI hit test changes.
                DmEvent::TextBoxFocused { textbox } if listening(textbox, "Focused") => {
                    fire.call::<()>((instance::id_key(textbox), "Focused"))?;
                }
                DmEvent::TextBoxFocusLost { textbox, enter_pressed } if listening(textbox, "FocusLost") => {
                    fire.call::<()>((instance::id_key(textbox), "FocusLost", enter_pressed))?;
                }
                DmEvent::Signal { id, name, args } if listening(id, &name) => {
                    let mut all: Vec<Value> = Vec::with_capacity(args.len() + 2);
                    all.push(Value::Number(instance::id_key(id)));
                    all.push(Value::String(lua.create_string(&name)?));
                    for a in &args {
                        all.push(convert::to_lua(lua, a)?);
                    }
                    fire.call::<()>(mlua::MultiValue::from_vec(all))?;
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

    /// Fire the remote calls and answers the session delivered. Handlers run
    /// on the scheduler, so one may yield, and an answer frees whichever
    /// `InvokeServer` was parked on it.
    pub fn dispatch_remotes(&mut self) -> mlua::Result<()> {
        let (calls, replies) = {
            let mut g = self.dm.lock();
            if g.remote_in.is_empty() && g.reply_in.is_empty() {
                return Ok(());
            }
            (std::mem::take(&mut g.remote_in), std::mem::take(&mut g.reply_in))
        };
        let lua = &self.lua;

        if !calls.is_empty() {
            let deliver: Function = self.host.get("deliverRemote")?;
            for call in calls {
                let args: Variadic<Value> = call
                    .args
                    .iter()
                    .map(|a| convert::remote_to_lua(lua, a))
                    .collect::<mlua::Result<Vec<_>>>()?
                    .into_iter()
                    .collect();
                let from = match call.from {
                    Some(player) => handle(lua, player)?,
                    None => Value::Nil,
                };
                let invocation = match call.invocation {
                    Some(id) => Value::String(lua.create_string(id.to_string())?),
                    None => Value::Nil,
                };
                deliver.call::<()>((instance::id_key(call.remote), from, invocation, args))?;
            }
        }

        if !replies.is_empty() {
            let deliver: Function = self.host.get("deliverReply")?;
            for reply in replies {
                let id = reply.invocation.to_string();
                match reply.result {
                    Ok(values) => {
                        let args: Variadic<Value> = values
                            .iter()
                            .map(|a| convert::remote_to_lua(lua, a))
                            .collect::<mlua::Result<Vec<_>>>()?
                            .into_iter()
                            .collect();
                        deliver.call::<()>((id, true, Value::Nil, args))?;
                    }
                    Err(message) => {
                        deliver.call::<()>((id, false, message, Variadic::<Value>::new()))?;
                    }
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

    /// How long a script may run without yielding before it is stopped
    /// (10 s unless set).
    pub fn set_script_timeout(&self, budget: std::time::Duration) {
        self.watchdog.budget_ms.store(budget.as_millis() as u64, Ordering::Relaxed);
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
            // Output points at the call: the script of its chunk, and the line.
            let (at, line) = match script_position(lua, 0) {
                Some((chunk, line)) => (script_of_chunk(lua, &chunk).or(script), line),
                None => (script, 0),
            };
            dm.lock().print_from(level, at, &source, text, line, Vec::new());
            Ok(())
        })?)?;
    }
    if let Ok(Value::Table(owned)) = lua.named_registry_value::<Value>(OWNED_COROUTINE) {
        env.raw_set("coroutine", owned)?;
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

/// Host functions behind `MarketplaceService` (its Luau half is
/// `Host.MarketplaceService` in the prelude). They read and queue only; the
/// engine talks to the Commerce API (see `datamodel::commerce`). Every one
/// wakes commerce, so the catalog loads the first time a script asks.
/// Remote calls: what `FireServer`, `FireClient`, `FireAllClients`,
/// `InvokeServer` and the return of `OnServerInvoke` push onto the DataModel
/// for the session to send. With no session up the prelude never calls these
/// and keeps looping remotes back inside the VM, the way a Space playing on
/// its own always has.
///
/// An invocation id crosses this boundary as a string. The host builds one
/// from the peer and the call number, which can pass the range a Luau number
/// holds exactly, and the prelude only ever echoes it back.
fn install_remote(lua: &Lua, host_fns: &Table, dm: &SharedDataModel) -> mlua::Result<()> {
    // How long a parked `InvokeServer` waits, read by the prelude so the
    // limit lives in one place.
    host_fns.raw_set("remoteInvokeTimeout", INVOKE_TIMEOUT_SECS)?;
    {
        let dm = dm.clone();
        host_fns.raw_set(
            "remoteNetworked",
            lua.create_function(move |_, ()| Ok(dm.lock().networked))?,
        )?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set(
            "remoteIsServer",
            lua.create_function(move |_, ()| Ok(dm.lock().is_server))?,
        )?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set(
            "remoteFire",
            lua.create_function(
                move |_, (remote, target, player, args): (LInst, String, Option<LInst>, Variadic<Value>)| {
                    let target = match target.as_str() {
                        "Server" => RemoteTarget::Server,
                        "All" => RemoteTarget::AllClients,
                        _ => match player {
                            Some(p) => RemoteTarget::Client(p.0),
                            None => {
                                return Err(mlua::Error::RuntimeError(
                                    "FireClient needs a player".into(),
                                ))
                            }
                        },
                    };
                    let args = remote_args(&args)?;
                    dm.lock().remote_out.push(RemoteCall {
                        remote: remote.0,
                        target,
                        args,
                        invocation: None,
                    });
                    Ok(())
                },
            )?,
        )?;
    }
    {
        let dm = dm.clone();
        // A Player's id has to fit the u32 call number on the wire, where 0
        // means an event, so these run from 1 upward and wrap.
        let next = Arc::new(AtomicU64::new(0));
        host_fns.raw_set(
            "remoteInvoke",
            lua.create_function(move |_, (remote, args): (LInst, Variadic<Value>)| {
                let n = next.fetch_add(1, Ordering::Relaxed);
                let id = n % (u32::MAX as u64 - 1) + 1;
                let args = remote_args(&args)?;
                dm.lock().remote_out.push(RemoteCall {
                    remote: remote.0,
                    target: RemoteTarget::Server,
                    args,
                    invocation: Some(id),
                });
                Ok(id.to_string())
            })?,
        )?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set(
            "remoteReply",
            lua.create_function(
                move |_, (invocation, player, ok, args): (String, Option<LInst>, bool, Variadic<Value>)| {
                    let invocation = invocation.parse::<u64>().map_err(|_| {
                        mlua::Error::RuntimeError("a reply carried an unreadable invocation id".into())
                    })?;
                    let result = if ok {
                        Ok(remote_args(&args)?)
                    } else {
                        Err(match args.first() {
                            Some(Value::String(s)) => s.to_str()?.to_string(),
                            _ => "the server refused the call".to_string(),
                        })
                    };
                    dm.lock().reply_out.push(RemoteReply {
                        invocation,
                        to: player.map(|p| p.0),
                        result,
                    });
                    Ok(())
                },
            )?,
        )?;
    }
    Ok(())
}

/// Every argument converted for the wire.
fn remote_args(args: &[Value]) -> mlua::Result<Vec<RemoteValue>> {
    args.iter().map(convert::remote_from_lua).collect()
}

fn install_commerce(lua: &Lua, host_fns: &Table, dm: &SharedDataModel) -> mlua::Result<()> {
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceStatus", lua.create_function(move |_, ()| {
            let mut g = dm.lock();
            g.commerce.wake();
            Ok(match &g.commerce.status {
                CommerceStatus::Idle | CommerceStatus::Loading => "loading".to_string(),
                CommerceStatus::Ready => "ready".to_string(),
                CommerceStatus::Unavailable(why) => format!("unavailable: {why}"),
            })
        })?)?;
    }
    {
        let dm = dm.clone();
        // A script set ProcessReceipt: receipts are Luau's to answer.
        host_fns.raw_set("commerceWanted", lua.create_function(move |_, ()| {
            let mut g = dm.lock();
            g.commerce.luau_process_receipt = true;
            g.commerce.wake();
            Ok(())
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commercePrompt", lua.create_function(move |_, (player, product, prompt): (Value, Value, String)| {
            let user_id = user_id_arg(&dm, &player)?;
            let product = product_arg(&product)?;
            let expects = match prompt.as_str() {
                "pass" => Some(ProductKind::Pass),
                "consumable" => Some(ProductKind::Consumable),
                _ => None,
            };
            let mut g = dm.lock();
            g.commerce.wake();
            g.commerce.prompts.push(PurchasePrompt { user_id, product, expects });
            Ok(())
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceProduct", lua.create_function(move |lua, id: Value| {
            let Ok(number) = product_arg(&id) else { return Ok(Value::Nil) };
            let product = dm.lock().commerce.product(number).cloned();
            let Some(p) = product else { return Ok(Value::Nil) };
            let info = lua.create_table()?;
            info.raw_set("Name", p.name)?;
            info.raw_set("Description", p.description)?;
            info.raw_set("PriceInTickets", p.price as f64)?;
            // Where scripts ported from Roblox read the price.
            info.raw_set("PriceInRobux", p.price as f64)?;
            info.raw_set("ProductId", p.number as f64)?;
            info.raw_set("TargetId", p.number as f64)?;
            info.raw_set("IsForSale", p.active)?;
            info.raw_set(
                "ProductType",
                match p.kind {
                    ProductKind::Consumable => "Developer Product",
                    ProductKind::Pass => "Game Pass",
                },
            )?;
            info.raw_set("IconImageAssetId", 0)?;
            if let Some(icon) = p.icon {
                info.raw_set("IconUrl", icon)?;
            }
            info.raw_set("EustressProductId", p.id)?;
            Ok(Value::Table(info))
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceOwns", lua.create_function(move |_, (user, pass): (Value, Value)| {
            let user_id = user_id_arg(&dm, &user)?;
            let number = product_arg(&pass)?;
            let mut g = dm.lock();
            g.commerce.wake();
            // The local player's passes are the signed-in account's; a joined
            // player's are the ones its verified purchases proved.
            let local = g.local_player.and_then(|p| g.get_prop(p, "UserId")).and_then(|v| v.as_number());
            Ok(g.commerce.owns_pass(user_id, local, number))
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commercePassesPending", lua.create_function(move |_, user: Value| {
            let user_id = user_id_arg(&dm, &user)?;
            Ok(dm.lock().commerce.passes_pending(user_id))
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commercePending", lua.create_function(move |_, ()| {
            Ok(dm.lock().commerce.waiting_for_scripts() as f64)
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceTakeReceipts", lua.create_function(move |lua, ()| {
            let receipts: Vec<_> = dm.lock().commerce.receipts.drain(..).collect();
            let out = lua.create_table()?;
            for (i, r) in receipts.into_iter().enumerate() {
                // Roblox's receiptInfo, with Tickets for the currency.
                let info = lua.create_table()?;
                info.raw_set("PurchaseId", r.purchase_id)?;
                info.raw_set("PlayerId", r.user_id)?;
                info.raw_set("ProductId", r.product as f64)?;
                info.raw_set("CurrencySpent", r.price as f64)?;
                info.raw_set("CurrencyType", convert::enum_item(lua, &EnumItem::new("CurrencyType", "Tickets"))?)?;
                info.raw_set("PlaceIdWherePurchased", r.sim_id.as_str())?;
                info.raw_set("SimulationId", r.sim_id)?;
                if let Some(space) = r.space {
                    info.raw_set("Space", space)?;
                }
                out.raw_set(i + 1, info)?;
            }
            Ok(out)
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceTakeOutcomes", lua.create_function(move |lua, ()| {
            let outcomes = std::mem::take(&mut dm.lock().commerce.outcomes);
            let out = lua.create_table()?;
            for (i, o) in outcomes.into_iter().enumerate() {
                let t = lua.create_table()?;
                t.raw_set("userId", o.user_id)?;
                t.raw_set("productId", o.product as f64)?;
                t.raw_set(
                    "prompt",
                    match o.expects {
                        Some(ProductKind::Pass) => "pass",
                        Some(ProductKind::Consumable) => "consumable",
                        None => "any",
                    },
                )?;
                t.raw_set("purchased", o.purchased)?;
                out.raw_set(i + 1, t)?;
            }
            Ok(out)
        })?)?;
    }
    {
        let dm = dm.clone();
        host_fns.raw_set("commerceDecision", lua.create_function(move |_, (purchase_id, granted): (String, bool)| {
            dm.lock().commerce.decisions.push(ReceiptDecision { purchase_id, granted });
            Ok(())
        })?)?;
    }
    Ok(())
}

/// A `UserId`: a Player handle's, or a number given directly.
fn user_id_arg(dm: &SharedDataModel, v: &Value) -> mlua::Result<f64> {
    match v {
        Value::Integer(i) => Ok(*i as f64),
        Value::Number(n) => Ok(*n),
        Value::UserData(ud) => {
            let id = ud.peek::<LInst>()?.0;
            let g = dm.lock();
            if g.class_of(id) != Some("Player") {
                return Err(mlua::Error::RuntimeError("expected a Player".into()));
            }
            Ok(g.get_prop(id, "UserId").and_then(|v| v.as_number()).unwrap_or(0.0))
        }
        other => Err(mlua::Error::RuntimeError(format!("expected a Player, got {}", other.type_name()))),
    }
}

/// A product number, as scripts pass one to `PromptProductPurchase`.
fn product_arg(v: &Value) -> mlua::Result<u64> {
    let n = match v {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        Value::String(s) => s.to_str()?.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    };
    if n >= 1.0 && n.fract() == 0.0 && n <= 999_999_999.0 {
        Ok(n as u64)
    } else {
        Err(mlua::Error::RuntimeError(format!(
            "expected a product number (a whole number from 1), got {}",
            v.type_name()
        )))
    }
}

fn mouse_get(lua: &Lua, dm: &SharedDataModel, key: &str) -> mlua::Result<Value> {
    // Hit, Origin and UnitRay's origin are positions, in the caller's units.
    let k = if matches!(key, "Hit" | "Origin" | "UnitRay") {
        studs::script_scale(&instance::host_table(lua)?)?
    } else {
        1.0
    };
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
            Value::UserData(lua.create_userdata(LuauCFrame(studs::scale_frame(cf, 1.0 / k)))?)
        }
        "Origin" => {
            let cf = CFrame::look_at(m.ray_origin, m.ray_origin + dir, None);
            Value::UserData(lua.create_userdata(LuauCFrame(studs::scale_frame(cf, 1.0 / k)))?)
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
        "UnitRay" => Value::UserData(lua.create_userdata(types_ext::LuauRay { origin: m.ray_origin * (1.0 / k), direction: dir })?),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::{CommerceProduct, PromptOutcome, Receipt};

    fn no_rays(_: &RayQuery) -> Option<RayHit> {
        None
    }

    /// A VM over a tree whose local player has UserId 42, and a Script to run
    /// code as.
    fn session() -> (PlayLuau, SharedDataModel, InstanceId) {
        let dm = crate::datamodel::new_shared();
        let script = {
            let mut g = dm.lock();
            let players = g.get_service("Players").expect("Players");
            let player = g.create_virtual("Player", "Creator", Some(players));
            g.set_prop_from_engine(player, "UserId", DmValue::Number(42.0));
            g.local_player = Some(player);
            let service = g.get_service("ServerScriptService").expect("ServerScriptService");
            g.create_virtual("Script", "Shop", Some(service))
        };
        let vm = PlayLuau::new(dm.clone()).expect("the prelude loads");
        (vm, dm, script)
    }

    fn run(vm: &mut PlayLuau, script: InstanceId, source: &str) {
        let launch = ScriptLaunch { instance: script, source: source.to_string(), chunk_name: "Shop".into() };
        // A Space without terrain: the reader never visits.
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.run_scripts(vec![launch], &no_rays, no_terrain);
    }

    /// What the scripts wrote, without the host's lifecycle lines.
    fn output(dm: &SharedDataModel) -> Vec<String> {
        dm.lock().output.iter().filter(|l| !l.lifecycle).map(|l| l.text.clone()).collect()
    }

    #[test]
    fn marketplace_prompts_reach_the_commerce_queue() {
        let (mut vm, dm, script) = session();
        assert_eq!(dm.lock().commerce.status, CommerceStatus::Idle);
        run(&mut vm, script, r#"
            local MarketplaceService = game:GetService("MarketplaceService")
            local player = game:GetService("Players"):GetPlayers()[1]
            MarketplaceService:PromptProductPurchase(player, 3)
            MarketplaceService:PromptGamePassPurchase(player, 7)
        "#);
        let g = dm.lock();
        assert!(g.output.iter().all(|l| l.level != OutputLevel::Error), "{:?}", g.output);
        assert_eq!(g.commerce.status, CommerceStatus::Loading, "the first use wakes commerce");
        let asked: Vec<_> = g.commerce.prompts.iter().map(|p| (p.user_id, p.product, p.expects)).collect();
        assert_eq!(asked, vec![(42.0, 3, Some(ProductKind::Consumable)), (42.0, 7, Some(ProductKind::Pass))]);
    }

    #[test]
    fn receipts_wait_for_process_receipt_and_report_its_decision() {
        let (mut vm, dm, script) = session();
        dm.lock().commerce.receipts.push_back(Receipt {
            purchase_id: "pur_a".into(),
            user_id: 42.0,
            product: 3,
            price: 50,
            sim_id: "sim".into(),
            space: None,
        });
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.frame(&no_rays, no_terrain);
        assert_eq!(dm.lock().commerce.receipts.len(), 1, "with no ProcessReceipt the receipt waits");

        run(&mut vm, script, r#"
            local MarketplaceService = game:GetService("MarketplaceService")
            MarketplaceService.ProcessReceipt = function(info)
                if info.ProductId == 3 and info.CurrencySpent == 50 and info.PlayerId == 42 then
                    return Enum.ProductPurchaseDecision.PurchaseGranted
                end
                return Enum.ProductPurchaseDecision.NotProcessedYet
            end
            MarketplaceService.PromptProductPurchaseFinished:Connect(function(userId, productId, purchased)
                print("finished", userId, productId, purchased)
            end)
        "#);
        assert_eq!(dm.lock().commerce.status, CommerceStatus::Loading, "setting ProcessReceipt wakes commerce");
        assert!(dm.lock().commerce.luau_process_receipt, "and makes receipts Luau's to answer");
        dm.lock().commerce.outcomes.push(PromptOutcome {
            user_id: 42.0,
            product: 3,
            expects: Some(ProductKind::Consumable),
            purchased: true,
        });
        vm.frame(&no_rays, no_terrain);

        let g = dm.lock();
        assert!(g.commerce.receipts.is_empty());
        assert_eq!(g.commerce.decisions, vec![ReceiptDecision { purchase_id: "pur_a".into(), granted: true }]);
        assert!(g.output.iter().any(|l| l.text == "finished 42 3 true"), "{:?}", g.output);
    }

    #[test]
    fn product_info_and_pass_ownership_come_from_the_catalog() {
        let (mut vm, dm, script) = session();
        {
            let mut g = dm.lock();
            g.commerce.status = CommerceStatus::Ready;
            g.commerce.catalog = vec![CommerceProduct {
                id: "prod_x".into(),
                number: 7,
                name: "VIP".into(),
                description: String::new(),
                price: 99,
                kind: ProductKind::Pass,
                icon: None,
                active: true,
            }];
            g.commerce.owned_passes.insert(7);
        }
        run(&mut vm, script, r#"
            local MarketplaceService = game:GetService("MarketplaceService")
            local info = MarketplaceService:GetProductInfo(7)
            print(info.Name, info.PriceInRobux, info.ProductType)
            print(MarketplaceService:UserOwnsGamePassAsync(42, 7), MarketplaceService:UserOwnsGamePassAsync(1, 7))
            print(pcall(function() return MarketplaceService:GetProductInfo(8) end) == false)
        "#);
        assert_eq!(output(&dm), vec!["VIP 99 Game Pass", "true false", "true"]);
    }

    /// Start `source` as a new Script in ServerScriptService, its source
    /// read from `file` when one is given.
    fn start(vm: &mut PlayLuau, dm: &SharedDataModel, name: &str, file: Option<&str>, source: &str) -> InstanceId {
        let id = {
            let mut g = dm.lock();
            let service = g.get_service("ServerScriptService").expect("ServerScriptService");
            let id = g.create_virtual("Script", name, Some(service));
            if let Some(file) = file {
                g.set_script_file(id, file);
            }
            id
        };
        let launch = ScriptLaunch { instance: id, source: source.to_string(), chunk_name: format!("ServerScriptService.{name}") };
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.run_scripts(vec![launch], &no_rays, no_terrain);
        id
    }

    fn step(vm: &mut PlayLuau) {
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.frame(&no_rays, no_terrain);
    }

    fn lines(dm: &SharedDataModel) -> Vec<crate::datamodel::OutputLine> {
        dm.lock().output.clone()
    }

    #[test]
    fn a_runaway_loop_stops_where_it_was_and_pcall_cannot_hold_it() {
        let (mut vm, dm, _) = session();
        vm.set_script_timeout(std::time::Duration::from_millis(200));
        start(&mut vm, &dm, "Runaway", None, "local ok = pcall(function()\n    while true do end\nend)\nprint(\"carried on\", ok)\n");
        let out = lines(&dm);
        let err = out.iter().find(|l| l.level == OutputLevel::Error).expect("the loop is stopped");
        assert!(
            err.text.starts_with("Script timeout at ServerScriptService.Runaway:2: exhausted allowed execution time"),
            "{}",
            err.text
        );
        assert_eq!((err.source.as_str(), err.line), ("ServerScriptService.Runaway", 2));
        assert!(out.iter().all(|l| !l.text.starts_with("carried on")), "a pcall does not catch the timeout: {out:?}");
    }

    #[test]
    fn the_budget_is_per_resume_and_play_goes_on_after_a_timeout() {
        let (mut vm, dm, _) = session();
        vm.set_script_timeout(std::time::Duration::from_millis(500));
        start(&mut vm, &dm, "Ticker", None, "game:GetService(\"RunService\").Heartbeat:Connect(function() print(\"tick\") end)\n");
        start(&mut vm, &dm, "Runaway", None, "while true do end\n");
        assert_eq!(lines(&dm).iter().filter(|l| l.level == OutputLevel::Error).count(), 1);
        step(&mut vm);
        assert!(output(&dm).contains(&"tick".to_string()), "the other scripts carry on: {:?}", lines(&dm));

        // Busy 50 ms at a time with a yield in between: 200 ms in all, and
        // never 500 ms in one resume.
        start(&mut vm, &dm, "Paced", None, r#"
            for i = 1, 4 do
                local t = os.clock()
                while os.clock() - t < 0.05 do end
                task.wait()
            end
            print("paced")
        "#);
        for _ in 0..6 {
            step(&mut vm);
        }
        assert!(output(&dm).contains(&"paced".to_string()), "{:?}", lines(&dm));
        assert_eq!(lines(&dm).iter().filter(|l| l.level == OutputLevel::Error).count(), 1, "{:?}", lines(&dm));
    }

    #[test]
    fn an_error_names_its_line_and_stack() {
        let (mut vm, dm, _) = session();
        start(&mut vm, &dm, "Boom", None, "local function explode()\n    local t = nil\n    return t.x\nend\nexplode()\n");
        let out = lines(&dm);
        let err = out.iter().find(|l| l.level == OutputLevel::Error).expect("an error");
        assert!(err.text.starts_with("ServerScriptService.Boom:3: attempt to index nil"), "{}", err.text);
        assert_eq!((err.source.as_str(), err.line), ("ServerScriptService.Boom", 3));
        assert_eq!(err.stack, vec!["ServerScriptService.Boom:3 function explode", "ServerScriptService.Boom:5"]);
        let last = out.last().expect("a last line");
        assert!(last.lifecycle && last.text == "Stopped by the error above; nothing left running", "{last:?}");
    }

    #[test]
    fn a_host_error_reads_as_roblox_shows_it() {
        let (mut vm, dm, _) = session();
        // Workspace has its one Terrain, and the host refuses another.
        start(&mut vm, &dm, "Missing", None, "\nlocal ground = Instance.new(\"Terrain\")\n");
        let err = lines(&dm).into_iter().find(|l| l.level == OutputLevel::Error).expect("an error");
        assert_eq!(err.text, "ServerScriptService.Missing:2: Unable to create an Instance of type \"Terrain\"");
        assert_eq!(err.line, 2);
    }

    #[test]
    fn a_print_points_at_its_line_and_the_scripts_file() {
        let (mut vm, dm, _) = session();
        let file = "C:/Space/ServerScriptService/Hello.server.luau";
        let id = start(&mut vm, &dm, "Hello", Some(file), "\n\nprint(\"hi\")\n");
        let out = lines(&dm);
        let hi = out.iter().find(|l| l.text == "hi").expect("the print");
        assert_eq!((hi.source.as_str(), hi.file.as_str(), hi.line, hi.lifecycle), ("ServerScriptService.Hello", file, 3, false));
        let started = out.iter().find(|l| l.lifecycle && l.text == "Started").expect("a start line");
        assert_eq!(started.file, file);
        let finished = out.iter().find(|l| l.lifecycle && l.text.starts_with("Finished")).expect("a finish line");
        assert_eq!(finished.text, "Finished its top-level code; nothing left running");

        // A copy of the script (a PlayerGui's, a character's) points at the same file.
        let copy = dm.lock().clone_instance(id).expect("a copy");
        assert_eq!(dm.lock().script_file(copy), Some(file));
    }

    #[test]
    fn the_finish_line_says_what_is_still_running() {
        let (mut vm, dm, _) = session();
        start(&mut vm, &dm, "Listener", None, r#"
            game:GetService("RunService").Heartbeat:Connect(function() end)
            task.spawn(function() task.wait(60) end)
        "#);
        let out = lines(&dm);
        let finished = out.iter().find(|l| l.lifecycle && l.text.starts_with("Finished")).expect("a finish line");
        assert_eq!(finished.text, "Finished its top-level code; 1 connection and 1 thread still running");
    }

    #[test]
    fn a_compile_error_says_the_script_did_not_start() {
        let (mut vm, dm, _) = session();
        start(&mut vm, &dm, "Broken", None, "local x = (\n");
        let out = lines(&dm);
        let err = out.iter().find(|l| l.level == OutputLevel::Error).expect("an error");
        assert!(err.text.starts_with("ServerScriptService.Broken:"), "{}", err.text);
        assert!(err.text.ends_with("(the script did not start)"), "{}", err.text);
        assert!(err.line >= 1, "{err:?}");
        assert!(out.iter().all(|l| !l.lifecycle), "a script that never started has no lifecycle lines: {out:?}");
    }

    #[test]
    fn a_wait_on_a_destroyed_instance_is_reported() {
        let (mut vm, dm, _) = session();
        start(&mut vm, &dm, "Waiter", None, r#"
            local part = Instance.new("Part")
            part.Parent = workspace
            task.spawn(function()
                part.Touched:Wait()
            end)
            part:Destroy()
        "#);
        step(&mut vm);
        let out = lines(&dm);
        let warn = out.iter().find(|l| l.level == OutputLevel::Warn).expect("a warning");
        assert_eq!(warn.text, "Infinite yield: this thread waits on the Touched event of Part, which was destroyed, so it never resumes");
        assert_eq!((warn.source.as_str(), warn.line), ("ServerScriptService.Waiter", 5));
    }

    /// A part in the Workspace.
    fn mass_part(dm: &SharedDataModel, name: &str, size: f64, material: &str, shape: Option<&str>, anchored: bool) -> InstanceId {
        let mut g = dm.lock();
        let ws = g.get_service("Workspace").expect("Workspace");
        let p = g.create_virtual("Part", name, Some(ws));
        g.set_prop(p, "Size", DmValue::Vector3(Vector3::new(size, size, size))).expect("Size");
        g.set_prop(p, "Material", DmValue::Enum(EnumItem::new("Material", material))).expect("Material");
        if let Some(shape) = shape {
            g.set_prop(p, "Shape", DmValue::Enum(EnumItem::new("PartType", shape))).expect("Shape");
        }
        g.set_prop(p, "Anchored", DmValue::Bool(anchored)).expect("Anchored");
        p
    }

    #[test]
    fn a_parts_mass_is_its_density_times_its_collider_volume() {
        let (mut vm, dm, _) = session();
        let crate_part = mass_part(&dm, "Crate", 1.0, "Wood", None, false);
        mass_part(&dm, "Post", 1.0, "Wood", None, true);
        mass_part(&dm, "Rock", 2.0, "Plastic", Some("Ball"), false);
        start(&mut vm, &dm, "Weigh", None, r#"
            local c = workspace.Crate
            print(c:GetMass(), c.Mass, c.AssemblyMass)
            print(workspace.Post:GetMass(), workspace.Post.AssemblyMass == math.huge)
            print(string.format("%.3f", workspace.Rock:GetMass()))
            print(pcall(function() c.Mass = 1 end))
        "#);
        {
            // What the reader seeds for a custom density, and what the
            // physics engine reports for the crate's body.
            let mut g = dm.lock();
            g.set_prop_from_engine(crate_part, crate::datamodel::PART_DENSITY, DmValue::Number(1000.0));
            g.set_prop_from_engine(crate_part, "AssemblyMass", DmValue::Number(1500.0));
        }
        start(&mut vm, &dm, "Again", None, "print(workspace.Crate:GetMass(), workspace.Crate.AssemblyMass)");
        let out = output(&dm);
        assert_eq!(out[0], "600 600 600", "a 1 m Wood cube, alone in its assembly: {out:?}");
        assert_eq!(out[1], "600 true", "an anchored assembly is infinite: {out:?}");
        assert_eq!(out[2], format!("{:.3}", 900.0 * 4.0 / 3.0 * std::f64::consts::PI), "a Ball is a sphere: {out:?}");
        assert!(out[3].starts_with("false") && out[3].contains("Mass is read-only"), "{out:?}");
        assert_eq!(out[4], "1000 1500", "{out:?}");
    }

    #[test]
    fn a_bare_coroutine_yield_left_alone_is_reported() {
        let (mut vm, dm, _) = session();
        start(&mut vm, &dm, "Yielder", None, "coroutine.yield()\nprint(\"never\")\n");
        step(&mut vm);
        assert!(lines(&dm).iter().all(|l| l.level != OutputLevel::Warn), "not before 5 s");
        dm.lock().frame.time = 6.0;
        step(&mut vm);
        let out = lines(&dm);
        let warn = out.iter().find(|l| l.level == OutputLevel::Warn).expect("a warning");
        assert_eq!(
            warn.text,
            "Infinite yield possible: this thread called coroutine.yield() and nothing has resumed it for 5 s"
        );
        assert_eq!(warn.line, 1);
    }
}
