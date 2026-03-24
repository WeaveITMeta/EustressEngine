//! # Scripting Events System
//!
//! Roblox-compatible Signal/Connection pattern for script communication.
//! Supports Connect, Once, Wait, and Disconnect patterns.
//!
//! ## Table of Contents
//!
//! 1. **Connection** — Handle to a connected callback
//! 2. **Signal** — Event emitter that can have multiple listeners
//! 3. **ScriptSignal** — Thread-safe signal for cross-script communication
//! 4. **Built-in Signals** — Heartbeat, Stepped, RenderStepped, etc.

use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::collections::HashMap;

// ============================================================================
// 1. Connection
// ============================================================================

/// Unique identifier for a connection
pub type ConnectionId = u64;

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

fn next_connection_id() -> ConnectionId {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst)
}

/// Handle to a connected callback. Can be disconnected.
#[derive(Debug, Clone)]
pub struct Connection {
    id: ConnectionId,
    connected: Arc<AtomicBool>,
    signal_id: u64,
}

impl Connection {
    /// Create a new connection
    fn new(signal_id: u64) -> Self {
        Self {
            id: next_connection_id(),
            connected: Arc::new(AtomicBool::new(true)),
            signal_id,
        }
    }

    /// Get the connection ID
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// Check if still connected
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Disconnect this connection
    pub fn disconnect(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

// ============================================================================
// 2. Signal — Generic event emitter
// ============================================================================

/// Callback function type (boxed closure)
pub type Callback<T> = Box<dyn Fn(&T) + Send + Sync + 'static>;

/// A signal that can have multiple listeners.
/// Generic over the argument type T.
pub struct Signal<T: Clone + Send + Sync + 'static> {
    id: u64,
    listeners: Arc<Mutex<HashMap<ConnectionId, (Arc<AtomicBool>, Callback<T>, bool)>>>,
    waiters: Arc<Mutex<Vec<std::sync::mpsc::Sender<T>>>>,
}

static NEXT_SIGNAL_ID: AtomicU64 = AtomicU64::new(1);

impl<T: Clone + Send + Sync + 'static> Signal<T> {
    /// Create a new signal
    pub fn new() -> Self {
        Self {
            id: NEXT_SIGNAL_ID.fetch_add(1, Ordering::SeqCst),
            listeners: Arc::new(Mutex::new(HashMap::new())),
            waiters: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Connect a callback to this signal
    pub fn connect<F>(&self, callback: F) -> Connection
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        let connection = Connection::new(self.id);
        let mut listeners = self.listeners.lock().unwrap();
        listeners.insert(
            connection.id,
            (connection.connected.clone(), Box::new(callback), false),
        );
        connection
    }

    /// Connect a callback that will only fire once
    pub fn once<F>(&self, callback: F) -> Connection
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        let connection = Connection::new(self.id);
        let mut listeners = self.listeners.lock().unwrap();
        listeners.insert(
            connection.id,
            (connection.connected.clone(), Box::new(callback), true),
        );
        connection
    }

    /// Wait for the signal to fire (blocking)
    /// Returns the value that was fired
    pub fn wait(&self) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut waiters = self.waiters.lock().unwrap();
            waiters.push(tx);
        }
        rx.recv().expect("Signal wait channel closed")
    }

    /// Wait for the signal with timeout
    /// Returns None if timeout expires
    pub fn wait_timeout(&self, timeout: std::time::Duration) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut waiters = self.waiters.lock().unwrap();
            waiters.push(tx);
        }
        rx.recv_timeout(timeout).ok()
    }

    /// Fire the signal with a value
    pub fn fire(&self, value: T) {
        // Notify waiters
        {
            let mut waiters = self.waiters.lock().unwrap();
            waiters.retain(|tx| tx.send(value.clone()).is_ok());
        }

        // Call listeners
        let mut to_remove = Vec::new();
        {
            let listeners = self.listeners.lock().unwrap();
            for (id, (connected, callback, once)) in listeners.iter() {
                if connected.load(Ordering::SeqCst) {
                    callback(&value);
                    if *once {
                        connected.store(false, Ordering::SeqCst);
                        to_remove.push(*id);
                    }
                } else {
                    to_remove.push(*id);
                }
            }
        }

        // Remove disconnected/once listeners
        if !to_remove.is_empty() {
            let mut listeners = self.listeners.lock().unwrap();
            for id in to_remove {
                listeners.remove(&id);
            }
        }
    }

    /// Disconnect a specific connection by ID
    pub fn disconnect(&self, connection_id: ConnectionId) {
        let mut listeners = self.listeners.lock().unwrap();
        if let Some((connected, _, _)) = listeners.get(&connection_id) {
            connected.store(false, Ordering::SeqCst);
        }
        listeners.remove(&connection_id);
    }

    /// Disconnect all listeners
    pub fn disconnect_all(&self) {
        let mut listeners = self.listeners.lock().unwrap();
        for (_, (connected, _, _)) in listeners.iter() {
            connected.store(false, Ordering::SeqCst);
        }
        listeners.clear();
    }

    /// Get number of active connections
    pub fn connection_count(&self) -> usize {
        let listeners = self.listeners.lock().unwrap();
        listeners.iter().filter(|(_, (c, _, _))| c.load(Ordering::SeqCst)).count()
    }
}

impl<T: Clone + Send + Sync + 'static> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            listeners: self.listeners.clone(),
            waiters: self.waiters.clone(),
        }
    }
}

// ============================================================================
// 3. ScriptSignal — Simplified signal for scripting
// ============================================================================

/// Argument types that can be passed through signals
#[derive(Debug, Clone)]
pub enum SignalArg {
    None,
    Number(f64),
    String(String),
    Bool(bool),
    Vector3(super::types::Vector3),
    CFrame(super::types::CFrame),
    Color3(super::types::Color3),
    EntityId(u64),
    Table(Vec<(String, SignalArg)>),
}

impl Default for SignalArg {
    fn default() -> Self {
        SignalArg::None
    }
}

/// A script-friendly signal that passes SignalArg values
pub type ScriptSignal = Signal<Vec<SignalArg>>;

// ============================================================================
// 4. BindableEvent — Script-created events
// ============================================================================

/// A bindable event that scripts can create and fire
#[derive(Clone)]
pub struct BindableEvent {
    signal: ScriptSignal,
    name: String,
}

impl BindableEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            signal: Signal::new(),
            name: name.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The Event signal that scripts connect to
    pub fn event(&self) -> &ScriptSignal {
        &self.signal
    }

    /// Fire the event with arguments
    pub fn fire(&self, args: Vec<SignalArg>) {
        self.signal.fire(args);
    }
}

// ============================================================================
// 5. BindableFunction — Script-created functions
// ============================================================================

/// Callback type for bindable functions
pub type BindableFunctionCallback = Arc<dyn Fn(Vec<SignalArg>) -> Vec<SignalArg> + Send + Sync>;

/// A bindable function that scripts can create and invoke
pub struct BindableFunction {
    name: String,
    callback: Arc<Mutex<Option<BindableFunctionCallback>>>,
}

impl BindableFunction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the OnInvoke callback
    pub fn set_on_invoke<F>(&self, callback: F)
    where
        F: Fn(Vec<SignalArg>) -> Vec<SignalArg> + Send + Sync + 'static,
    {
        let mut cb = self.callback.lock().unwrap();
        *cb = Some(Arc::new(callback));
    }

    /// Invoke the function with arguments
    pub fn invoke(&self, args: Vec<SignalArg>) -> Vec<SignalArg> {
        let cb = self.callback.lock().unwrap();
        if let Some(callback) = cb.as_ref() {
            callback(args)
        } else {
            vec![SignalArg::None]
        }
    }
}

impl Clone for BindableFunction {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            callback: self.callback.clone(),
        }
    }
}

// ============================================================================
// 6. RemoteEvent — Client/Server communication
// ============================================================================

/// Remote event for client-server communication
#[derive(Clone)]
pub struct RemoteEvent {
    name: String,
    on_server_event: ScriptSignal,
    on_client_event: ScriptSignal,
}

impl RemoteEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            on_server_event: Signal::new(),
            on_client_event: Signal::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Signal fired on server when client calls FireServer
    pub fn on_server_event(&self) -> &ScriptSignal {
        &self.on_server_event
    }

    /// Signal fired on client when server calls FireClient
    pub fn on_client_event(&self) -> &ScriptSignal {
        &self.on_client_event
    }

    /// Fire from client to server (adds player as first arg)
    pub fn fire_server(&self, player_id: u64, args: Vec<SignalArg>) {
        let mut full_args = vec![SignalArg::EntityId(player_id)];
        full_args.extend(args);
        self.on_server_event.fire(full_args);
    }

    /// Fire from server to specific client
    pub fn fire_client(&self, _player_id: u64, args: Vec<SignalArg>) {
        // In a real implementation, this would route to specific client
        self.on_client_event.fire(args);
    }

    /// Fire from server to all clients
    pub fn fire_all_clients(&self, args: Vec<SignalArg>) {
        self.on_client_event.fire(args);
    }
}

// ============================================================================
// 7. RemoteFunction — Client/Server RPC
// ============================================================================

/// Remote function for client-server RPC
pub struct RemoteFunction {
    name: String,
    on_server_invoke: Arc<Mutex<Option<BindableFunctionCallback>>>,
    on_client_invoke: Arc<Mutex<Option<BindableFunctionCallback>>>,
}

impl RemoteFunction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            on_server_invoke: Arc::new(Mutex::new(None)),
            on_client_invoke: Arc::new(Mutex::new(None)),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set server-side handler
    pub fn set_on_server_invoke<F>(&self, callback: F)
    where
        F: Fn(Vec<SignalArg>) -> Vec<SignalArg> + Send + Sync + 'static,
    {
        let mut cb = self.on_server_invoke.lock().unwrap();
        *cb = Some(Arc::new(callback));
    }

    /// Set client-side handler
    pub fn set_on_client_invoke<F>(&self, callback: F)
    where
        F: Fn(Vec<SignalArg>) -> Vec<SignalArg> + Send + Sync + 'static,
    {
        let mut cb = self.on_client_invoke.lock().unwrap();
        *cb = Some(Arc::new(callback));
    }

    /// Invoke server from client
    pub fn invoke_server(&self, player_id: u64, args: Vec<SignalArg>) -> Vec<SignalArg> {
        let cb = self.on_server_invoke.lock().unwrap();
        if let Some(callback) = cb.as_ref() {
            let mut full_args = vec![SignalArg::EntityId(player_id)];
            full_args.extend(args);
            callback(full_args)
        } else {
            vec![SignalArg::None]
        }
    }

    /// Invoke client from server
    pub fn invoke_client(&self, _player_id: u64, args: Vec<SignalArg>) -> Vec<SignalArg> {
        let cb = self.on_client_invoke.lock().unwrap();
        if let Some(callback) = cb.as_ref() {
            callback(args)
        } else {
            vec![SignalArg::None]
        }
    }
}

impl Clone for RemoteFunction {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            on_server_invoke: self.on_server_invoke.clone(),
            on_client_invoke: self.on_client_invoke.clone(),
        }
    }
}

// ============================================================================
// 8. PropertyChangedSignal
// ============================================================================

/// Signal that fires when a property changes
#[derive(Clone)]
pub struct PropertyChangedSignal {
    property_name: String,
    signal: Signal<SignalArg>,
}

impl PropertyChangedSignal {
    pub fn new(property_name: impl Into<String>) -> Self {
        Self {
            property_name: property_name.into(),
            signal: Signal::new(),
        }
    }

    pub fn property_name(&self) -> &str {
        &self.property_name
    }

    pub fn connect<F>(&self, callback: F) -> Connection
    where
        F: Fn(&SignalArg) + Send + Sync + 'static,
    {
        self.signal.connect(callback)
    }

    pub fn fire(&self, new_value: SignalArg) {
        self.signal.fire(new_value);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    #[test]
    fn test_signal_connect_fire() {
        let signal: Signal<i32> = Signal::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let _conn = signal.connect(move |value| {
            counter_clone.fetch_add(*value, Ordering::SeqCst);
        });

        signal.fire(5);
        signal.fire(3);

        assert_eq!(counter.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn test_signal_once() {
        let signal: Signal<i32> = Signal::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let _conn = signal.once(move |value| {
            counter_clone.fetch_add(*value, Ordering::SeqCst);
        });

        signal.fire(5);
        signal.fire(3); // Should not fire

        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_signal_disconnect() {
        let signal: Signal<i32> = Signal::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let conn = signal.connect(move |value| {
            counter_clone.fetch_add(*value, Ordering::SeqCst);
        });

        signal.fire(5);
        conn.disconnect();
        signal.fire(3); // Should not fire

        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_bindable_event() {
        let event = BindableEvent::new("TestEvent");
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let _conn = event.event().connect(move |args| {
            if let Some(SignalArg::Number(n)) = args.first() {
                counter_clone.fetch_add(*n as i32, Ordering::SeqCst);
            }
        });

        event.fire(vec![SignalArg::Number(10.0)]);
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_bindable_function() {
        let func = BindableFunction::new("TestFunc");
        
        func.set_on_invoke(|args| {
            if let Some(SignalArg::Number(n)) = args.first() {
                vec![SignalArg::Number(n * 2.0)]
            } else {
                vec![SignalArg::None]
            }
        });

        let result = func.invoke(vec![SignalArg::Number(5.0)]);
        if let SignalArg::Number(n) = result[0] {
            assert_eq!(n, 10.0);
        } else {
            panic!("Expected number result");
        }
    }
}
