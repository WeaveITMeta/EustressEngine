//! Backend-neutral conversions for primitive Rust types.
//!
//! Lets host functions take and return plain Rust values without naming a
//! backend. Each backend only has to provide the `ScriptVm` surface.

use super::{
    FromScriptArgs, FromScriptValue, IntoScriptValue, IntoScriptValues, ScriptVm, Value, VmError,
};

impl FromScriptValue for bool {
    fn from_script<V: ScriptVm>(v: Value<V>) -> Result<Self, VmError> {
        match v {
            Value::Boolean(b) => Ok(b),
            Value::Nil => Ok(false),
            _ => Err(VmError::Conversion { from: "value", to: "bool" }),
        }
    }
}

impl FromScriptValue for i64 {
    fn from_script<V: ScriptVm>(v: Value<V>) -> Result<Self, VmError> {
        match v {
            Value::Integer(i) => Ok(i),
            Value::Number(n) if n.fract() == 0.0 => Ok(n as i64),
            _ => Err(VmError::Conversion { from: "value", to: "i64" }),
        }
    }
}

impl FromScriptValue for f64 {
    fn from_script<V: ScriptVm>(v: Value<V>) -> Result<Self, VmError> {
        match v {
            Value::Number(n) => Ok(n),
            Value::Integer(i) => Ok(i as f64),
            _ => Err(VmError::Conversion { from: "value", to: "f64" }),
        }
    }
}

impl FromScriptValue for String {
    fn from_script<V: ScriptVm>(v: Value<V>) -> Result<Self, VmError> {
        match v {
            Value::String(s) => Ok(s),
            _ => Err(VmError::Conversion { from: "value", to: "String" }),
        }
    }
}

impl IntoScriptValue for bool {
    fn into_script<V: ScriptVm>(self, _vm: &V) -> Result<Value<V>, VmError> {
        Ok(Value::Boolean(self))
    }
}

impl IntoScriptValue for i64 {
    fn into_script<V: ScriptVm>(self, _vm: &V) -> Result<Value<V>, VmError> {
        Ok(Value::Integer(self))
    }
}

impl IntoScriptValue for f64 {
    fn into_script<V: ScriptVm>(self, _vm: &V) -> Result<Value<V>, VmError> {
        Ok(Value::Number(self))
    }
}

impl IntoScriptValue for String {
    fn into_script<V: ScriptVm>(self, _vm: &V) -> Result<Value<V>, VmError> {
        Ok(Value::String(self))
    }
}

impl<T: FromScriptValue> FromScriptArgs for Vec<T> {
    fn from_args<V: ScriptVm>(args: Vec<Value<V>>) -> Result<Self, VmError> {
        args.into_iter().map(T::from_script).collect()
    }
}

impl<T: IntoScriptValue> IntoScriptValues for Vec<T> {
    fn into_values<V: ScriptVm>(self, vm: &V) -> Result<Vec<Value<V>>, VmError> {
        self.into_iter().map(|t| t.into_script(vm)).collect()
    }
}

impl IntoScriptValues for () {
    fn into_values<V: ScriptVm>(self, _vm: &V) -> Result<Vec<Value<V>>, VmError> {
        Ok(Vec::new())
    }
}

impl<T: IntoScriptValue> IntoScriptValue for Option<T> {
    fn into_script<V: ScriptVm>(self, vm: &V) -> Result<Value<V>, VmError> {
        match self {
            Some(t) => t.into_script(vm),
            None => Ok(Value::Nil),
        }
    }
}
