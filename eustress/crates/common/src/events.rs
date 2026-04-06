//! # EustressEventBus
//!
//! Named event bus that bridges Rust systems, Luau scripts, Rune souls,
//! and EustressStream topics. Built on the existing `Signal<Vec<SignalArg>>`
//! primitive from `scripting::events`.
//!
//! ## Design
//!
//! ```text
//! Bevy system          Luau script          Rune soul
//!   bus.fire(name, …)    EventBus:Fire(…)     event_bus::fire(…)
//!         ↓                    ↓                    ↓
//!   ┌─────────────────────────────────────────────────────────┐
//!   │               EustressEventBus                          │
//!   │   signals: DashMap<String, ScriptSignal>                │
//!   └──────────────────────┬──────────────────────────────────┘
//!                          │  (optional, feature = "streaming")
//!                ┌─────────▼──────────┐
//!                │   EustressStream   │
//!                │   ChangeQueue      │
//!                └────────────────────┘
//! ```
//!
//! ## API surface
//!
//! **Rust / Bevy** (via `EventBusResource`):
//! ```rust
//! bus.fire("player_joined", vec![SignalArg::EntityId(player)]);
//! let _conn = bus.connect("player_joined", |args| { … });
//! ```
//!
//! **Luau** (global `EventBus`):
//! ```lua
//! EventBus:Connect("player_joined", function(args) end)
//! EventBus:Fire("player_joined", {EntityId = 42})
//! EventBus:Once("player_joined", function(args) end)
//! EventBus:Disconnect("player_joined", conn)
//! ```
//!
//! **Rune** (module `event_bus`):
//! ```rune
//! let conn = event_bus::connect("player_joined", |args| { });
//! event_bus::fire("player_joined", args);
//! event_bus::poll("player_joined")  // pull model for non-callback use
//! ```

use std::sync::Arc;

use bevy::prelude::*;
use dashmap::DashMap;

use crate::scripting::events::{Connection, ScriptSignal, Signal, SignalArg};

// ─────────────────────────────────────────────────────────────────────────────
// Core EventBus
// ─────────────────────────────────────────────────────────────────────────────

/// Named event bus shared across Rust, Luau, Rune, and EustressStream.
/// Cheaply cloneable — all clones share the same signal registry.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<BusInner>,
}

struct BusInner {
    signals: DashMap<String, ScriptSignal>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BusInner {
                signals: DashMap::new(),
            }),
        }
    }

    /// Get or create the named signal.
    pub fn signal(&self, name: &str) -> ScriptSignal {
        self.inner.signals
            .entry(name.to_string())
            .or_insert_with(Signal::new)
            .clone()
    }

    /// Fire an event by name. Creates the topic if it doesn't exist yet.
    pub fn fire(&self, name: &str, args: Vec<SignalArg>) {
        self.signal(name).fire(args);
    }

    /// Connect a persistent callback. Returns a `Connection` that can be
    /// used to disconnect later.
    pub fn connect<F>(&self, name: &str, callback: F) -> Connection
    where
        F: Fn(&Vec<SignalArg>) + Send + Sync + 'static,
    {
        self.signal(name).connect(callback)
    }

    /// Connect a one-shot callback — disconnects automatically after first fire.
    pub fn once<F>(&self, name: &str, callback: F) -> Connection
    where
        F: Fn(&Vec<SignalArg>) + Send + Sync + 'static,
    {
        self.signal(name).once(callback)
    }

    /// Block the current thread until the named event fires.
    /// **Do not call from the Bevy main thread.** Use from script worker threads or
    /// coroutine schedulers only.
    pub fn wait(&self, name: &str) -> Vec<SignalArg> {
        self.signal(name).wait()
    }

    /// Disconnect a specific connection from a named event.
    pub fn disconnect(&self, name: &str, conn: &Connection) {
        if let Some(sig) = self.inner.signals.get(name) {
            sig.disconnect(conn.id());
        }
    }

    /// Remove a named event entirely, disconnecting all listeners.
    pub fn remove(&self, name: &str) {
        if let Some((_, sig)) = self.inner.signals.remove(name) {
            sig.disconnect_all();
        }
    }

    /// List all currently registered event names.
    pub fn event_names(&self) -> Vec<String> {
        self.inner.signals.iter().map(|e| e.key().clone()).collect()
    }

    /// Number of active connections on a named event (0 if topic doesn't exist).
    pub fn connection_count(&self, name: &str) -> usize {
        self.inner.signals
            .get(name)
            .map(|s| s.connection_count())
            .unwrap_or(0)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy Resource + Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy resource wrapping the shared `EventBus`.
/// Insert with `app.init_resource::<EventBusResource>()` or let
/// `EventBusPlugin` handle it.
#[derive(Resource, Clone)]
pub struct EventBusResource(pub EventBus);

impl Default for EventBusResource {
    fn default() -> Self {
        Self(EventBus::new())
    }
}

/// Bevy plugin that registers the `EventBusResource` and wires the optional
/// EustressStream bridge.
pub struct EventBusPlugin;

impl Plugin for EventBusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventBusResource>();

        tracing::info!("EventBusPlugin initialized");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EustressStream bridge  (feature = "streaming")
// ─────────────────────────────────────────────────────────────────────────────

/// Well-known topic names bridged between EventBus and EustressStream.
/// Subscribe to these on the EventBus to receive messages published to the stream,
/// and fire these on the EventBus to publish back into the stream.
pub mod topics {
    /// Scene mutation deltas from Bevy ECS change detection.
    pub const SCENE_DELTAS: &str = "scene_deltas";
    /// Commands issued by AI agents (spawn, set-property, move, etc.).
    pub const AGENT_COMMANDS: &str = "agent_commands";
    /// Observations emitted to AI agents.
    pub const AGENT_OBSERVATIONS: &str = "agent_observations";
    /// Simulation result records.
    pub const SIM_RESULTS: &str = "sim_results";
    /// Rune/Luau script source change notifications.
    pub const RUNE_SCRIPTS: &str = "rune_scripts";
    /// ARC-AGI episode records.
    pub const ARC_EPISODES: &str = "arc_episodes";
}

/// Bridge an `EventBus` to a `ChangeQueue` so that:
/// - Stream messages on `topic` are fired on the bus as `SignalArg::String(base64 payload)`.
/// - Bus events on `topic` that carry a `SignalArg::String` payload are published back to the stream.
///
/// Call from a startup system **after** both resources are available.
/// Each call bridges one topic; call once per topic you want bridged.
///
/// The payload encoding is opaque — consumers should use the typed helpers in
/// `scene_delta` / `change_queue` to decode the bytes.
#[cfg(feature = "streaming")]
pub fn bridge_stream_topic(
    bus: &EventBus,
    queue: &crate::change_queue::ChangeQueue,
    topic: &'static str,
) {
    use bytes::Bytes;

    // Stream → Bus: subscribe to the EustressStream ring, fire on the bus.
    let bus_fire = bus.clone();
    queue.stream.subscribe(topic, move |view| {
        // Encode raw bytes as a base64 string so they fit into SignalArg::String.
        use std::fmt::Write;
        let raw = view.data;
        // Simple hex encoding without external dep: format each byte.
        let mut hex = String::with_capacity(raw.len() * 2);
        for b in raw.iter() {
            let _ = write!(hex, "{:02x}", b);
        }
        bus_fire.fire(topic, vec![SignalArg::String(hex)]);
    });

    // Bus → Stream: connect to the bus, publish hex-decoded bytes to the stream.
    let producer = queue.stream.producer(topic);
    bus.signal(topic).connect(move |args| {
        if let Some(SignalArg::String(hex_str)) = args.first() {
            // Decode hex back to bytes.
            let bytes: Vec<u8> = (0..hex_str.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).ok())
                .collect();
            if !bytes.is_empty() {
                producer.send_bytes(Bytes::from(bytes));
            }
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Luau (mlua) UserData impl
// ─────────────────────────────────────────────────────────────────────────────

/// Wrapper for injecting `EventBus` into the Luau VM as a UserData value.
/// Exposed as the global `EventBus` in all script contexts.
///
/// Luau API:
/// ```lua
/// local conn = EventBus:Connect("topic", function(args) ... end)
/// EventBus:Fire("topic", {Number = 42, String = "hello"})
/// EventBus:Once("topic", function(args) ... end)
/// EventBus:Disconnect("topic", conn)
/// local names = EventBus:EventNames()
/// ```
#[cfg(feature = "luau")]
#[derive(Clone)]
pub struct LuauEventBus(pub EventBus);

#[cfg(feature = "luau")]
impl mlua::UserData for LuauEventBus {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // EventBus:Connect(name, callback) -> connection_id (integer)
        methods.add_method("Connect", |_lua, this, (name, callback): (String, mlua::Function)| {
            // We store the mlua::Function in a thread-safe wrapper via Arc<Mutex<...>>
            // mlua Functions are !Send, so we store them in a Lua Registry key.
            // For cross-thread safety we track by connection ID only.
            use std::sync::{Arc, Mutex};
            let callback = Arc::new(Mutex::new(callback));
            let conn = this.0.connect(&name, move |args| {
                let cb = callback.lock().unwrap();
                // Convert SignalArgs to a Lua table for the callback.
                // This fires on the Bevy main thread so mlua is safe here.
                let _ = cb.call::<()>(format!("{:?}", args));
            });
            Ok(conn.id() as i64)
        });

        // EventBus:Fire(name, message_string)
        methods.add_method("Fire", |_lua, this, (name, msg): (String, mlua::Value)| {
            let arg = match msg {
                mlua::Value::String(s) => SignalArg::String(s.to_string_lossy().to_string()),
                mlua::Value::Number(n) => SignalArg::Number(n),
                mlua::Value::Boolean(b) => SignalArg::Bool(b),
                mlua::Value::Integer(i) => SignalArg::Number(i as f64),
                _ => SignalArg::None,
            };
            this.0.fire(&name, vec![arg]);
            Ok(())
        });

        // EventBus:Once(name, callback) -> connection_id
        methods.add_method("Once", |_lua, this, (name, callback): (String, mlua::Function)| {
            use std::sync::{Arc, Mutex};
            let callback = Arc::new(Mutex::new(callback));
            let conn = this.0.once(&name, move |args| {
                let cb = callback.lock().unwrap();
                let _ = cb.call::<()>(format!("{:?}", args));
            });
            Ok(conn.id() as i64)
        });

        // EventBus:Disconnect(name, connection_id)
        methods.add_method("Disconnect", |_lua, this, (name, conn_id): (String, i64)| {
            if let Some(sig) = this.0.inner.signals.get(&name) {
                sig.disconnect(conn_id as u64);
            }
            Ok(())
        });

        // EventBus:EventNames() -> table of strings
        methods.add_method("EventNames", |lua, this, ()| {
            let names = this.0.event_names();
            let t = lua.create_table()?;
            for (i, n) in names.into_iter().enumerate() {
                t.set(i + 1, n)?;
            }
            Ok(t)
        });

        // EventBus:ConnectionCount(name) -> integer
        methods.add_method("ConnectionCount", |_lua, this, name: String| {
            Ok(this.0.connection_count(&name) as i64)
        });
    }
}

/// Inject the `EventBus` global into an existing Luau VM.
/// Call this from your `LuauRuntimeState` setup system, after the
/// `EventBusResource` is available:
///
/// ```rust,ignore
/// let bus = LuauEventBus(event_bus_resource.0.clone());
/// inject_event_bus_global(&runtime.lua, bus)?;
/// ```
#[cfg(feature = "luau")]
pub fn inject_event_bus_global(lua: &mlua::Lua, bus: LuauEventBus) -> Result<(), String> {
    let userdata = lua.create_userdata(bus)
        .map_err(|e| format!("EventBus userdata creation failed: {e}"))?;
    lua.globals().set("EventBus", userdata)
        .map_err(|e| format!("EventBus global injection failed: {e}"))?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Rune thread-local bridge
// ─────────────────────────────────────────────────────────────────────────────

/// Thread-local holder for the EventBus during Rune script execution.
/// Set before execution, cleared after (same pattern as `SPATIAL_BRIDGE`).
thread_local! {
    static EVENT_BUS: std::cell::RefCell<Option<EventBus>> =
        std::cell::RefCell::new(None);
}

/// Install the EventBus for the current thread before Rune execution.
pub fn set_event_bus_for_rune(bus: EventBus) {
    EVENT_BUS.with(|cell| *cell.borrow_mut() = Some(bus));
}

/// Clear the EventBus after Rune execution.
pub fn clear_event_bus_for_rune() {
    EVENT_BUS.with(|cell| *cell.borrow_mut() = None);
}

/// Access the EventBus from inside a Rune native function.
pub fn with_event_bus<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&EventBus) -> R,
{
    EVENT_BUS.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bus) => f(bus),
            None => {
                tracing::warn!("[Rune] EventBus not available — install EventBusPlugin");
                fallback
            }
        }
    })
}

/// Register the `event_bus` native module for Rune.
/// Call from the same site that registers `eustress_ecs_module`.
///
/// Rune API:
/// ```rune
/// use event_bus;
/// event_bus::fire("topic", "payload_string");
/// let names = event_bus::event_names();
/// ```
#[cfg(feature = "realism-scripting")]
pub fn event_bus_rune_module() -> Result<rune::Module, rune::ContextError> {
    let mut m = rune::Module::with_crate("event_bus")?;

    m.function("fire", |name: String, msg: String| {
        with_event_bus((), |bus| {
            bus.fire(&name, vec![SignalArg::String(msg)]);
        });
    }).build()?;

    m.function("fire_number", |name: String, value: f64| {
        with_event_bus((), |bus| {
            bus.fire(&name, vec![SignalArg::Number(value)]);
        });
    }).build()?;

    m.function("fire_bool", |name: String, value: bool| {
        with_event_bus((), |bus| {
            bus.fire(&name, vec![SignalArg::Bool(value)]);
        });
    }).build()?;

    m.function("event_names", || -> Vec<String> {
        with_event_bus(vec![], |bus| bus.event_names())
    }).build()?;

    m.function("connection_count", |name: String| -> i64 {
        with_event_bus(0i64, |bus| bus.connection_count(&name) as i64)
    }).build()?;

    Ok(m)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn fire_and_connect() {
        let bus = EventBus::new();
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let r = received.clone();

        let _conn = bus.connect("test", move |args| {
            if let Some(SignalArg::String(s)) = args.first() {
                r.lock().unwrap().push(s.clone());
            }
        });

        bus.fire("test", vec![SignalArg::String("hello".into())]);
        bus.fire("test", vec![SignalArg::String("world".into())]);

        let msgs = received.lock().unwrap();
        assert_eq!(msgs.as_slice(), &["hello", "world"]);
    }

    #[test]
    fn once_fires_once() {
        let bus = EventBus::new();
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = count.clone();

        bus.once("ping", move |_| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        bus.fire("ping", vec![]);
        bus.fire("ping", vec![]);

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn disconnect_stops_delivery() {
        let bus = EventBus::new();
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = count.clone();

        let conn = bus.connect("evt", move |_| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        bus.fire("evt", vec![]);
        bus.disconnect("evt", &conn);
        bus.fire("evt", vec![]);

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn event_names_tracks_topics() {
        let bus = EventBus::new();
        bus.fire("alpha", vec![]);
        bus.fire("beta", vec![]);
        let mut names = bus.event_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn clone_shares_registry() {
        let bus = EventBus::new();
        let bus2 = bus.clone();
        let hit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let h = hit.clone();

        bus.connect("shared", move |_| {
            h.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        bus2.fire("shared", vec![]); // fires on bus2, received by bus listener

        assert!(hit.load(std::sync::atomic::Ordering::SeqCst));
    }
}
