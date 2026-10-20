//! mlua backend for [`ScriptVm`]. Native builds only.
//!
//! Implements the VM lifecycle, function registration, app data, and the
//! userdata bridge: fields, methods, `__eq`, `__tostring`. Arithmetic
//! metamethods are registered but raise a runtime error when called, since
//! the neutral `MetaRegistry::binary` returns `bool` and cannot express them.

use super::{
    FieldRegistry, FromScriptArgs, FromScriptValue, IntoScriptValue, IntoScriptValues, MetaOp,
    MetaRegistry, MethodRegistry, ScriptUserData, ScriptVm, Value, VmError,
};
use std::cell::RefCell;
use std::marker::PhantomData;

/// Thin wrapper over `mlua::Lua`. Nothing outside this file names mlua.
pub struct MluaVm {
    lua: mlua::Lua,
}

fn map_err(e: mlua::Error) -> VmError {
    match e {
        mlua::Error::SyntaxError { message, .. } => VmError::Syntax { message },
        mlua::Error::RuntimeError(m) => VmError::Runtime(m),
        other => VmError::External(other.to_string()),
    }
}

/// Host-side error to the mlua error a script sees.
fn to_lua_err(e: VmError) -> mlua::Error {
    mlua::Error::RuntimeError(e.to_string())
}

/// mlua value to backend-neutral value. Light userdata, threads, and errors
/// have no neutral form yet, so they are rejected rather than silently dropped.
fn to_script(v: mlua::Value) -> Result<Value<MluaVm>, VmError> {
    Ok(match v {
        mlua::Value::Nil => Value::Nil,
        mlua::Value::Boolean(b) => Value::Boolean(b),
        mlua::Value::Integer(i) => Value::Integer(i64::from(i)),
        mlua::Value::Number(n) => Value::Number(n),
        mlua::Value::String(s) => Value::String(
            s.to_str()
                .map_err(|e| VmError::External(e.to_string()))?
                .to_string(),
        ),
        mlua::Value::Table(t) => Value::Table(t),
        mlua::Value::Function(f) => Value::Function(f),
        mlua::Value::UserData(u) => Value::UserData(u),
        _ => return Err(VmError::Unsupported("script value kind not yet mapped")),
    })
}

/// Backend-neutral value to mlua value.
fn from_script(lua: &mlua::Lua, v: Value<MluaVm>) -> Result<mlua::Value, VmError> {
    Ok(match v {
        Value::Nil => mlua::Value::Nil,
        Value::Boolean(b) => mlua::Value::Boolean(b),
        // Luau integers are 32-bit; out-of-range values fall back to a number
        // so nothing is silently truncated.
        Value::Integer(i) => match i32::try_from(i) {
            Ok(small) => mlua::Value::Integer(small),
            Err(_) => mlua::Value::Number(i as f64),
        },
        Value::Number(n) => mlua::Value::Number(n),
        Value::String(s) => mlua::Value::String(
            lua.create_string(s.as_bytes())
                .map_err(|e| VmError::External(e.to_string()))?,
        ),
        Value::Table(t) => mlua::Value::Table(t),
        Value::Function(f) => mlua::Value::Function(f),
        Value::UserData(u) => mlua::Value::UserData(u),
    })
}

/// Host payload stored inside a Lua userdata. Scripts reach it through
/// `RefCell`, so field setters and methods get `&mut T` or `&T` safely.
pub struct Bridge<T: ScriptUserData>(RefCell<T>);

/// Adapts mlua's field registry to [`FieldRegistry`] for one host type.
struct FieldAdapter<'a, F, T> {
    inner: &'a mut F,
    _t: PhantomData<fn() -> T>,
}

impl<'a, F, T> FieldRegistry<T> for FieldAdapter<'a, F, T>
where
    T: ScriptUserData,
    F: mlua::UserDataFields<Bridge<T>>,
{
    fn get<R, G>(&mut self, name: &str, f: G)
    where
        G: Fn(&T) -> R + Send + 'static,
        R: IntoScriptValue,
    {
        self.inner
            .add_field_method_get(name.to_string(), move |lua: &mlua::Lua, b: &Bridge<T>| {
                let vm = MluaVm { lua: lua.clone() };
                let r = f(&b.0.borrow());
                let v = R::into_script(r, &vm).map_err(to_lua_err)?;
                from_script(lua, v).map_err(to_lua_err)
            });
    }

    fn set<A, S>(&mut self, name: &str, f: S)
    where
        S: Fn(&mut T, A) + Send + 'static,
        A: FromScriptValue,
    {
        self.inner.add_field_method_set(
            name.to_string(),
            move |_lua: &mlua::Lua, b: &mut Bridge<T>, a: mlua::Value| {
                let v = to_script(a).map_err(to_lua_err)?;
                let a = A::from_script(v).map_err(to_lua_err)?;
                f(b.0.get_mut(), a);
                Ok(())
            },
        );
    }
}

/// Adapts mlua's method registry to [`MethodRegistry`] for one host type.
struct MethodAdapter<'a, F, T> {
    inner: &'a mut F,
    _t: PhantomData<fn() -> T>,
}

impl<'a, F, T> MethodRegistry<T> for MethodAdapter<'a, F, T>
where
    T: ScriptUserData,
    F: mlua::UserDataMethods<Bridge<T>>,
{
    fn add<M, A, R>(&mut self, name: &str, f: M)
    where
        M: Fn(&T, A) -> R + Send + 'static,
        A: FromScriptArgs,
        R: IntoScriptValues,
    {
        self.inner.add_method(
            name.to_string(),
            move |lua: &mlua::Lua, b: &Bridge<T>, args: mlua::MultiValue| {
                let vals: Vec<Value<MluaVm>> = args
                    .into_iter()
                    .map(to_script)
                    .collect::<Result<_, _>>()
                    .map_err(to_lua_err)?;
                let a = A::from_args(vals).map_err(to_lua_err)?;
                let r = f(&b.0.borrow(), a);
                let vm = MluaVm { lua: lua.clone() };
                let out = R::into_values(r, &vm).map_err(to_lua_err)?;
                let mut mv = Vec::with_capacity(out.len());
                for v in out {
                    mv.push(from_script(lua, v).map_err(to_lua_err)?);
                }
                Ok(mlua::MultiValue::from_vec(mv))
            },
        );
    }
}

/// Adapts mlua's metamethod registration to [`MetaRegistry`] for one host type.
struct MetaAdapter<'a, F, T> {
    inner: &'a mut F,
    _t: PhantomData<fn() -> T>,
}

impl<'a, F, T> MetaRegistry<T> for MetaAdapter<'a, F, T>
where
    T: ScriptUserData,
    F: mlua::UserDataMethods<Bridge<T>>,
{
    fn binary<G>(&mut self, op: MetaOp, f: G)
    where
        G: Fn(&T, &T) -> Result<bool, VmError> + Send + 'static,
    {
        match op {
            MetaOp::Eq => {
                self.inner.add_meta_function(
                    "__eq",
                    move |_lua: &mlua::Lua, (a, b): (mlua::AnyUserData, mlua::AnyUserData)| {
                        let ba = a.borrow::<Bridge<T>>()?;
                        let bb = b.borrow::<Bridge<T>>()?;
                        let result = {
                            let ia = ba.0.borrow();
                            let ib = bb.0.borrow();
                            f(&ia, &ib)
                        };
                        result.map_err(to_lua_err)
                    },
                );
            }
            // Arithmetic needs a value-returning shape that `binary` cannot
            // express yet. Fail loudly when a script uses it.
            other => {
                let name = match other {
                    MetaOp::Add => "__add",
                    MetaOp::Sub => "__sub",
                    MetaOp::Mul => "__mul",
                    MetaOp::Div => "__div",
                    MetaOp::Unm => "__unm",
                    MetaOp::Eq => "__eq",
                };
                self.inner.add_meta_function(
                    name,
                    move |_lua: &mlua::Lua, _args: mlua::MultiValue| -> mlua::Result<()> {
                        Err(mlua::Error::RuntimeError(format!(
                            "operator {name} is not supported on this backend yet"
                        )))
                    },
                );
            }
        }
    }

    fn to_string<G>(&mut self, f: G)
    where
        G: Fn(&T) -> String + Send + 'static,
    {
        self.inner.add_meta_method(
            "__tostring",
            move |_lua: &mlua::Lua, b: &Bridge<T>, (): ()| Ok(f(&b.0.borrow())),
        );
    }
}

impl<T: ScriptUserData> mlua::UserData for Bridge<T> {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        let mut adapter = FieldAdapter::<F, T> {
            inner: fields,
            _t: PhantomData,
        };
        T::add_fields(&mut adapter);
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        {
            let mut m = MethodAdapter::<M, T> {
                inner: methods,
                _t: PhantomData,
            };
            T::add_methods(&mut m);
        }
        let mut meta = MetaAdapter::<M, T> {
            inner: methods,
            _t: PhantomData,
        };
        T::add_meta(&mut meta);
    }
}

impl ScriptVm for MluaVm {
    type Table = mlua::Table;
    type Function = mlua::Function;
    type UserData = mlua::AnyUserData;

    fn new() -> Result<Self, VmError> {
        Ok(Self {
            lua: mlua::Lua::new(),
        })
    }

    fn globals(&self) -> Self::Table {
        self.lua.globals()
    }

    fn create_table(&self) -> Result<Self::Table, VmError> {
        self.lua.create_table().map_err(map_err)
    }

    fn load(&self, source: &str, chunk_name: &str) -> Result<Self::Function, VmError> {
        self.lua
            .load(source)
            .set_name(chunk_name)
            .into_function()
            .map_err(map_err)
    }

    fn create_function<F>(&self, f: F) -> Result<Self::Function, VmError>
    where
        F: Fn(&Self, Vec<Value<Self>>) -> Result<Vec<Value<Self>>, VmError> + Send + 'static,
    {
        self.lua
            .create_function(move |lua, args: mlua::MultiValue| {
                let vm = MluaVm { lua: lua.clone() };
                let vals: Vec<Value<MluaVm>> = args
                    .into_iter()
                    .map(to_script)
                    .collect::<Result<_, _>>()
                    .map_err(to_lua_err)?;
                let out = f(&vm, vals).map_err(to_lua_err)?;
                let mut mv = Vec::with_capacity(out.len());
                for v in out {
                    mv.push(from_script(lua, v).map_err(to_lua_err)?);
                }
                Ok(mlua::MultiValue::from_vec(mv))
            })
            .map_err(map_err)
    }

    fn create_userdata<T: ScriptUserData>(&self, t: T) -> Result<Self::UserData, VmError> {
        self.lua
            .create_userdata(Bridge(RefCell::new(t)))
            .map_err(map_err)
    }

    fn set_app_data<T: Send + 'static>(&self, t: T) {
        self.lua.set_app_data(t);
    }

    fn with_app_data<T: 'static, R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        match self.lua.app_data_ref::<T>() {
            Some(guard) => f(Some(&*guard)),
            None => f(None),
        }
    }

    fn set_interrupt(&self, f: impl Fn() -> bool + Send + 'static) -> Result<(), VmError> {
        self.lua.set_interrupt(move |_| {
            if f() {
                Ok(mlua::VmState::Continue)
            } else {
                Err(mlua::Error::RuntimeError("script budget exceeded".into()))
            }
        });
        Ok(())
    }

    fn sandbox(&self, on: bool) -> Result<(), VmError> {
        self.lua.sandbox(on).map_err(map_err)
    }

    fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }
}
