//! `DmValue` <-> Luau value conversion.

use mlua::{Lua, Result as LuaResult, Value};

use crate::datamodel::{DmValue, EnumItem, RemoteValue};
use crate::luau::types::{LuauCFrame, LuauColor3, LuauUDim, LuauUDim2, LuauVector3, UserDataPeek};
use crate::scripting::{UDim, UDim2};

use super::instance::{handle, LInst};
use super::types_ext::{
    LuauBrickColor, LuauColorSequence, LuauEnumItem, LuauNumberRange, LuauNumberSequence, LuauVector2,
};

/// A DataModel value as a Luau value. Instances become their interned
/// handle, enum items the interned `EnumItem`.
pub fn to_lua(lua: &Lua, value: &DmValue) -> LuaResult<Value> {
    Ok(match value {
        DmValue::Nil => Value::Nil,
        DmValue::Bool(b) => Value::Boolean(*b),
        DmValue::Number(n) => Value::Number(*n),
        DmValue::String(s) => Value::String(lua.create_string(s)?),
        DmValue::Vector2(v) => Value::UserData(lua.create_userdata(LuauVector2(*v))?),
        DmValue::Vector3(v) => Value::UserData(lua.create_userdata(LuauVector3(*v))?),
        DmValue::CFrame(cf) => Value::UserData(lua.create_userdata(LuauCFrame(*cf))?),
        DmValue::Color3(c) => Value::UserData(lua.create_userdata(LuauColor3(*c))?),
        DmValue::UDim(u) => Value::UserData(lua.create_userdata(LuauUDim::new(u.scale, u.offset))?),
        DmValue::UDim2(u) => Value::UserData(lua.create_userdata(LuauUDim2::new(
            u.x.scale, u.x.offset, u.y.scale, u.y.offset,
        ))?),
        DmValue::NumberRange(r) => Value::UserData(lua.create_userdata(LuauNumberRange(*r))?),
        DmValue::Enum(e) => enum_item(lua, e)?,
        DmValue::Instance(id) => handle(lua, *id)?,
        DmValue::NumberSequence(k) => Value::UserData(lua.create_userdata(LuauNumberSequence(k.clone()))?),
        DmValue::ColorSequence(k) => Value::UserData(lua.create_userdata(LuauColorSequence(k.clone()))?),
    })
}

/// A Luau value as a DataModel value. Tables and functions are not
/// property values and are rejected.
pub fn from_lua(value: &Value) -> LuaResult<DmValue> {
    Ok(match value {
        Value::Nil => DmValue::Nil,
        Value::Boolean(b) => DmValue::Bool(*b),
        Value::Integer(i) => DmValue::Number(*i as f64),
        Value::Number(n) => DmValue::Number(*n),
        Value::String(s) => DmValue::String(s.to_str()?.to_string()),
        Value::UserData(ud) => {
            if let Ok(v) = ud.peek::<LInst>() {
                DmValue::Instance(v.0)
            } else if let Ok(v) = ud.peek::<LuauVector3>() {
                DmValue::Vector3(v.0)
            } else if let Ok(v) = ud.peek::<LuauCFrame>() {
                DmValue::CFrame(v.0)
            } else if let Ok(v) = ud.peek::<LuauColor3>() {
                DmValue::Color3(v.0)
            } else if let Ok(v) = ud.peek::<LuauUDim2>() {
                DmValue::UDim2(UDim2::new(v.x_scale, v.x_offset, v.y_scale, v.y_offset))
            } else if let Ok(v) = ud.peek::<LuauUDim>() {
                DmValue::UDim(UDim::new(v.scale, v.offset))
            } else if let Ok(v) = ud.peek::<LuauVector2>() {
                DmValue::Vector2(v.0)
            } else if let Ok(v) = ud.peek::<LuauEnumItem>() {
                DmValue::Enum(v.0.clone())
            } else if let Ok(v) = ud.peek::<LuauNumberRange>() {
                DmValue::NumberRange(v.0)
            } else if let Ok(v) = ud.peek::<LuauNumberSequence>() {
                DmValue::NumberSequence(v.0.clone())
            } else if let Ok(v) = ud.peek::<LuauColorSequence>() {
                DmValue::ColorSequence(v.0.clone())
            } else if let Ok(v) = ud.peek::<LuauBrickColor>() {
                DmValue::Color3(v.color)
            } else {
                return Err(mlua::Error::RuntimeError("unsupported userdata as a property value".into()));
            }
        }
        other => {
            return Err(mlua::Error::RuntimeError(format!(
                "a {} cannot be a property value",
                other.type_name()
            )))
        }
    })
}

/// The interned `EnumItem` userdata for `item`, so equal items are the
/// same Luau value.
pub fn enum_item(lua: &Lua, item: &EnumItem) -> LuaResult<Value> {
    let cache: mlua::Table = lua.named_registry_value("__eus_enum_items")?;
    let key = format!("{}.{}", item.enum_type, item.name);
    let existing: Value = cache.raw_get(key.as_str())?;
    if !existing.is_nil() {
        return Ok(existing);
    }
    let ud = Value::UserData(lua.create_userdata(LuauEnumItem(item.clone()))?);
    cache.raw_set(key, ud.clone())?;
    Ok(ud)
}

/// Accept `Enum.KeyCode.W`, `"W"` or `"KeyCode.W"` and return the item name.
pub fn enum_name_arg(value: &Value) -> Option<String> {
    match value {
        Value::UserData(ud) => ud.peek::<LuauEnumItem>().ok().map(|e| e.0.name.clone()),
        Value::String(s) => s
            .to_str()
            .ok()
            .map(|s| s.trim_start_matches("Enum.").rsplit('.').next().unwrap_or("").to_string()),
        _ => None,
    }
}

/// A remote argument as a Luau value.
pub fn remote_to_lua(lua: &Lua, value: &RemoteValue) -> LuaResult<Value> {
    Ok(match value {
        RemoteValue::Value(v) => to_lua(lua, v)?,
        RemoteValue::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i as i64 + 1, remote_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        RemoteValue::Map(entries) => {
            let t = lua.create_table()?;
            for (k, v) in entries {
                t.raw_set(k.as_str(), remote_to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}

/// A Luau value as a remote argument.
///
/// Scalars go through [`from_lua`]. A table becomes an array when its keys
/// run `1..n` and a map when they are all strings; a table mixing the two is
/// an error, as in Roblox. Functions and threads pass as nil rather than
/// failing the whole call, which is also what Roblox does.
pub fn remote_from_lua(value: &Value) -> LuaResult<RemoteValue> {
    match value {
        Value::Function(_) | Value::Thread(_) => Ok(RemoteValue::NIL),
        Value::Table(t) => table_to_remote(t),
        other => Ok(RemoteValue::Value(from_lua(other)?)),
    }
}

/// Split a table into the array part or the string-keyed part, refusing one
/// that holds both. Map keys come out sorted, so the same table always
/// encodes the same way and the determinism check stays honest.
fn table_to_remote(t: &mlua::Table) -> LuaResult<RemoteValue> {
    let len = t.raw_len();
    let mut array = Vec::with_capacity(len);
    for i in 1..=len {
        let v: Value = t.raw_get(i as i64)?;
        array.push(remote_from_lua(&v)?);
    }

    let mut map: Vec<(String, RemoteValue)> = Vec::new();
    for pair in t.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            // Already taken by the array walk above.
            Value::Integer(i) if i >= 1 && (i as usize) <= len => {}
            Value::Number(n) if n.fract() == 0.0 && n >= 1.0 && (n as usize) <= len => {}
            Value::String(s) => map.push((s.to_str()?.to_string(), remote_from_lua(&value)?)),
            other => {
                return Err(mlua::Error::RuntimeError(format!(
                    "a remote argument table cannot have a {} key",
                    other.type_name()
                )))
            }
        }
    }

    if !array.is_empty() && !map.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "a remote argument table cannot mix array and string keys".into(),
        ));
    }
    if map.is_empty() {
        Ok(RemoteValue::Array(array))
    } else {
        map.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(RemoteValue::Map(map))
    }
}
