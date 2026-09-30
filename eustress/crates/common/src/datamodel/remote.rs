//! Remote calls crossing the client and server boundary.
//!
//! `RemoteEvent` and `RemoteFunction` loop back inside one VM while a Space
//! runs on its own. Once a session is up the shell sets
//! [`DataModel::networked`](super::DataModel::networked), and the calls
//! scripts make leave through [`DataModel::remote_out`](super::DataModel::remote_out)
//! instead, while calls that arrived wait in
//! [`DataModel::remote_in`](super::DataModel::remote_in) for the VM to fire
//! `OnServerEvent` and `OnClientEvent`.
//!
//! These types stay in terms of the tree: an [`InstanceId`] names the remote
//! and the player, and a [`DmValue`] carries a scalar. The networking crate
//! translates each one to its wire form (`NetId` and `WireValue`), which it
//! can do because it depends on this crate and never the other way round.

use super::{DmValue, InstanceId};

/// How long a parked `InvokeServer` waits for its answer before it raises.
pub const INVOKE_TIMEOUT_SECS: f64 = 30.0;

/// An argument to a remote call.
///
/// Luau tables convert the way Roblox converts them: a table keyed `1..n`
/// becomes [`RemoteValue::Array`], a table with string keys becomes
/// [`RemoteValue::Map`], and a table mixing the two is an error. Functions
/// and threads pass as [`DmValue::Nil`].
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteValue {
    /// A scalar, including an instance reference.
    Value(DmValue),
    /// A Luau array.
    Array(Vec<RemoteValue>),
    /// A Luau table with string keys, sorted so the encoding is stable.
    Map(Vec<(String, RemoteValue)>),
}

impl RemoteValue {
    pub const NIL: Self = RemoteValue::Value(DmValue::Nil);

    /// The scalar this holds, or `None` for an array or a map.
    pub fn as_value(&self) -> Option<&DmValue> {
        match self {
            RemoteValue::Value(v) => Some(v),
            _ => None,
        }
    }

    /// Roblox `typeof` name, for the signature check on a typed remote.
    pub fn type_name(&self) -> &'static str {
        match self {
            RemoteValue::Value(v) => v.type_name(),
            RemoteValue::Array(_) | RemoteValue::Map(_) => "table",
        }
    }
}

impl From<DmValue> for RemoteValue {
    fn from(value: DmValue) -> Self {
        RemoteValue::Value(value)
    }
}

/// Who a call is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTarget {
    /// `FireServer` or `InvokeServer`, made on a player.
    Server,
    /// `FireClient(player, ...)`, made on the host. A call aimed at the
    /// host's own player loops back instead of going out.
    Client(InstanceId),
    /// `FireAllClients(...)`, made on the host.
    AllClients,
}

/// A call a script made that is leaving this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCall {
    /// The `RemoteEvent` or `RemoteFunction` it was made on.
    pub remote: InstanceId,
    pub target: RemoteTarget,
    pub args: Vec<RemoteValue>,
    /// Set by `InvokeServer`. The VM keeps the calling coroutine parked until
    /// a [`RemoteReply`] carrying this id lands in
    /// [`DataModel::reply_in`](super::DataModel::reply_in), or the wait
    /// passes [`INVOKE_TIMEOUT_SECS`].
    pub invocation: Option<u64>,
}

/// A call that arrived, for the VM to drain and fire.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDelivery {
    pub remote: InstanceId,
    /// The `Player` it came from, on a host. `None` on a player, where every
    /// delivery comes from the host.
    pub from: Option<InstanceId>,
    pub args: Vec<RemoteValue>,
    /// Set when the sender is parked on an answer, so the VM sends the return
    /// of `OnServerInvoke` back as a [`RemoteReply`].
    pub invocation: Option<u64>,
}

/// The answer to an `InvokeServer`, in either direction.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteReply {
    /// The id from the [`RemoteCall`] this answers.
    pub invocation: u64,
    /// The player to answer, on a host. `None` on a player.
    pub to: Option<InstanceId>,
    /// `Err` carries the message the parked `InvokeServer` raises.
    pub result: Result<Vec<RemoteValue>, String>,
}
