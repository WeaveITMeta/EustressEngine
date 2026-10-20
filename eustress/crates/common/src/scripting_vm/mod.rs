//! Backend-neutral Luau scripting interface.
//!
//! The script API (host functions, userdata, instance proxies, budget,
//! sandbox) is written once against [`ScriptVm`]. Backends implement it:
//! `mlua` for native builds, and a pure-Rust backend for wasm32 (to come).
//! Nothing in this module names `mlua::` directly.
//!
//! Status: interface plus mlua backend. Engine call sites not yet migrated.
//! See `DRAFT_INTERFACE.md` in this folder for the surface it covers.

use std::fmt;

#[cfg(feature = "mlua")]
pub mod mlua_backend;
pub mod convert;

/// One error type for every backend.
#[derive(Debug, Clone)]
pub enum VmError {
    /// Script raised a runtime error.
    Runtime(String),
    /// Source failed to parse or compile.
    Syntax { message: String },
    /// A value could not be converted to the requested Rust type.
    Conversion { from: &'static str, to: &'static str },
    /// Host-side failure wrapped as a message.
    External(String),
    /// Feature or capability not available on this backend.
    Unsupported(&'static str),
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmError::Runtime(m) => write!(f, "runtime error: {m}"),
            VmError::Syntax { message } => write!(f, "syntax error: {message}"),
            VmError::Conversion { from, to } => write!(f, "cannot convert {from} to {to}"),
            VmError::External(m) => write!(f, "{m}"),
            VmError::Unsupported(what) => write!(f, "unsupported on this backend: {what}"),
        }
    }
}

impl std::error::Error for VmError {}

/// Backend-neutral script value. Each backend converts to and from its own
/// representation at the boundary.
#[derive(Clone)]
pub enum Value<V: ScriptVm> {
    Nil,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Table(V::Table),
    Function(V::Function),
    UserData(V::UserData),
}

/// Host types exposed to scripts. Replaces `mlua::UserData`; each backend
/// reads these hooks and builds its own userdata metatable from them.
/// `Send` is required because the mlua backend runs with its `send` feature.
pub trait ScriptUserData: Sized + Send + 'static {
    /// Type name shown in errors and `typeof`-style checks.
    const TYPE_NAME: &'static str;

    /// Register readable and writable fields.
    fn add_fields<F: FieldRegistry<Self>>(_fields: &mut F) {}

    /// Register methods callable as `obj:method(...)`.
    fn add_methods<M: MethodRegistry<Self>>(_methods: &mut M) {}

    /// Operator metamethods: `__eq`, `__add`, `__tostring`, and so on.
    fn add_meta<M: MetaRegistry<Self>>(_meta: &mut M) {}
}

pub trait FieldRegistry<T> {
    fn get<R, F>(&mut self, name: &str, f: F)
    where
        F: Fn(&T) -> R + Send + 'static,
        R: IntoScriptValue;
    fn set<A, F>(&mut self, name: &str, f: F)
    where
        F: Fn(&mut T, A) + Send + 'static,
        A: FromScriptValue;
}

pub trait MethodRegistry<T> {
    fn add<F, A, R>(&mut self, name: &str, f: F)
    where
        F: Fn(&T, A) -> R + Send + 'static,
        A: FromScriptArgs,
        R: IntoScriptValues;
}

pub trait MetaRegistry<T> {
    fn binary<F>(&mut self, op: MetaOp, f: F)
    where
        F: Fn(&T, &T) -> Result<bool, VmError> + Send + 'static;
    fn to_string<F>(&mut self, f: F)
    where
        F: Fn(&T) -> String + Send + 'static;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaOp {
    Eq,
    Add,
    Sub,
    Mul,
    Div,
    Unm,
}

pub trait FromScriptValue: Sized {
    fn from_script<V: ScriptVm>(v: Value<V>) -> Result<Self, VmError>;
}

pub trait IntoScriptValue {
    fn into_script<V: ScriptVm>(self, vm: &V) -> Result<Value<V>, VmError>;
}

pub trait FromScriptArgs: Sized {
    fn from_args<V: ScriptVm>(args: Vec<Value<V>>) -> Result<Self, VmError>;
}

pub trait IntoScriptValues {
    fn into_values<V: ScriptVm>(self, vm: &V) -> Result<Vec<Value<V>>, VmError>;
}

/// The VM contract. A backend implements this once; the script API above it
/// is written against this trait alone.
pub trait ScriptVm: Sized + 'static {
    type Table: Clone;
    type Function: Clone;
    type UserData: Clone;

    /// Build a fresh VM with the Luau standard library.
    fn new() -> Result<Self, VmError>;

    fn globals(&self) -> Self::Table;
    fn create_table(&self) -> Result<Self::Table, VmError>;

    /// Compile and load a chunk. Syntax errors come back as
    /// [`VmError::Syntax`]; this is also the editor's diagnostics path.
    fn load(&self, source: &str, chunk_name: &str) -> Result<Self::Function, VmError>;

    fn create_function<F>(&self, f: F) -> Result<Self::Function, VmError>
    where
        F: Fn(&Self, Vec<Value<Self>>) -> Result<Vec<Value<Self>>, VmError> + Send + 'static;

    fn create_userdata<T: ScriptUserData>(&self, t: T) -> Result<Self::UserData, VmError>;

    /// Typed side channel for host state reachable from host functions.
    fn set_app_data<T: Send + 'static>(&self, t: T);
    fn with_app_data<T: 'static, R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R;

    /// Budget hook. Mlua calls this per instruction; a backend may call it
    /// at coarser points (for example per host call) and document that.
    /// Returning `false` stops the running script with an error.
    fn set_interrupt(&self, f: impl Fn() -> bool + Send + 'static) -> Result<(), VmError>;

    /// Sandboxing for untrusted script environments.
    fn sandbox(&self, on: bool) -> Result<(), VmError>;

    fn used_memory(&self) -> usize;
}
