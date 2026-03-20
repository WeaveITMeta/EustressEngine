//! # Client-Server Bridge
//!
//! Communication bridge for RemoteEvent, RemoteFunction, BindableEvent, and BindableFunction.
//! Routes messages between Luau scripts across client/server boundaries.
//!
//! ## Table of Contents
//!
//! 1. **LuauValue** — Serializable value type for cross-boundary data
//! 2. **RemoteEventBus** — Queues RemoteEvent fires for network replication
//! 3. **RemoteEventFired** — Bevy event when a RemoteEvent is fired
//! 4. **RemoteFunctionInvoked** — Bevy event when a RemoteFunction is called
//! 5. **BindableEventBus** — Queues BindableEvent fires for in-process delivery
//! 6. **BindableEventFired** — Bevy event when a BindableEvent is fired
//! 7. **BindableFunctionInvoked** — Bevy event when a BindableFunction is called
//! 8. **BridgeDirection** — Server→Client, Client→Server routing

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Serializable Luau Value
// ============================================================================

/// A serializable value that can cross the client-server boundary.
/// Maps to Luau types: nil, bool, number, string, table (as HashMap).
/// Intentionally limited — no functions, userdata, or threads cross the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LuauValue {
    /// Luau `nil`
    Nil,
    /// Luau `boolean`
    Bool(bool),
    /// Luau `number` (f64)
    Number(f64),
    /// Luau `string`
    String(String),
    /// Luau integer (i64) — Luau supports integer subtype
    Integer(i64),
    /// Luau `table` (dictionary form)
    Table(HashMap<String, LuauValue>),
    /// Luau `table` (array form)
    Array(Vec<LuauValue>),
    /// Vector3 (common in game scripts)
    Vector3(f32, f32, f32),
    /// CFrame (position + rotation, serialized as 12 floats)
    CFrame([f32; 12]),
    /// Entity reference (Bevy Entity bits)
    EntityRef(u64),
}

impl Default for LuauValue {
    fn default() -> Self {
        LuauValue::Nil
    }
}

// ============================================================================
// Bridge Direction
// ============================================================================

/// Direction of a remote message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeDirection {
    /// Server fires to all clients (or specific client)
    ServerToClient,
    /// Client fires to server
    ClientToServer,
}

// ============================================================================
// Remote Event Bus and Events
// ============================================================================

/// Bus for queuing RemoteEvent fires before they are dispatched as Bevy events.
/// In a networked game, these would be serialized and sent over QUIC.
#[derive(Resource, Default)]
pub struct RemoteEventBus {
    /// Pending remote event fires
    pub pending: Vec<RemoteEventFired>,
}

impl RemoteEventBus {
    /// Queue a remote event fire from script code
    pub fn fire(&mut self, event_name: &str, direction: BridgeDirection, args: Vec<LuauValue>) {
        self.pending.push(RemoteEventFired {
            event_name: event_name.to_string(),
            direction,
            sender: None,
            args,
        });
    }

    /// Queue a remote event fire targeting a specific player
    pub fn fire_to_client(&mut self, event_name: &str, player_entity: Entity, args: Vec<LuauValue>) {
        self.pending.push(RemoteEventFired {
            event_name: event_name.to_string(),
            direction: BridgeDirection::ServerToClient,
            sender: Some(player_entity),
            args,
        });
    }
}

/// Bevy message fired when a RemoteEvent is triggered
#[derive(Message, Debug, Clone)]
pub struct RemoteEventFired {
    /// Name of the RemoteEvent entity
    pub event_name: String,
    /// Direction of the fire
    pub direction: BridgeDirection,
    /// Sender entity (player for client→server, None for server→all)
    pub sender: Option<Entity>,
    /// Arguments passed with the event
    pub args: Vec<LuauValue>,
}

/// Bevy message fired when a RemoteFunction is invoked
#[derive(Message, Debug, Clone)]
pub struct RemoteFunctionInvoked {
    /// Name of the RemoteFunction entity
    pub function_name: String,
    /// Direction of the invocation
    pub direction: BridgeDirection,
    /// Invoker entity (player for client→server)
    pub invoker: Option<Entity>,
    /// Arguments passed to the function
    pub args: Vec<LuauValue>,
    /// Unique request ID for correlating responses
    pub request_id: u64,
}

/// Response to a RemoteFunction invocation
#[derive(Debug, Clone)]
pub struct RemoteFunctionResponse {
    /// Unique request ID matching the invocation
    pub request_id: u64,
    /// Return value (or error)
    pub result: Result<LuauValue, String>,
}

// ============================================================================
// Bindable Event Bus and Events
// ============================================================================

/// Bus for queuing BindableEvent fires (in-process only, no network).
#[derive(Resource, Default)]
pub struct BindableEventBus {
    /// Pending bindable event fires
    pub pending: Vec<BindableEventFired>,
}

impl BindableEventBus {
    /// Queue a bindable event fire from script code
    pub fn fire(&mut self, event_name: &str, args: Vec<LuauValue>) {
        self.pending.push(BindableEventFired {
            event_name: event_name.to_string(),
            args,
        });
    }
}

/// Bevy message fired when a BindableEvent is triggered (in-process)
#[derive(Message, Debug, Clone)]
pub struct BindableEventFired {
    /// Name of the BindableEvent entity
    pub event_name: String,
    /// Arguments passed with the event
    pub args: Vec<LuauValue>,
}

/// Bevy message fired when a BindableFunction is invoked (in-process)
#[derive(Message, Debug, Clone)]
pub struct BindableFunctionInvoked {
    /// Name of the BindableFunction entity
    pub function_name: String,
    /// Arguments passed to the function
    pub args: Vec<LuauValue>,
    /// Unique request ID for correlating responses
    pub request_id: u64,
}

// ============================================================================
// Network Packet Types (for QUIC serialization)
// ============================================================================

/// Wire format for remote event/function messages sent over QUIC.
/// Used by the play server for client↔server Luau communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LuauNetworkPacket {
    /// RemoteEvent fire
    RemoteEventFire {
        /// Event name
        event_name: String,
        /// Serialized arguments
        args: Vec<LuauValue>,
    },

    /// RemoteFunction invocation request
    RemoteFunctionInvoke {
        /// Function name
        function_name: String,
        /// Serialized arguments
        args: Vec<LuauValue>,
        /// Request ID for response correlation
        request_id: u64,
    },

    /// RemoteFunction response
    RemoteFunctionReturn {
        /// Request ID matching the invocation
        request_id: u64,
        /// Return value or error string
        result: Result<LuauValue, String>,
    },
}
