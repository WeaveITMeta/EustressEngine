//! # Luau Scripting Types
//!
//! Wires the shared `eustress_common::scripting` types into the Luau VM.
//! Provides mlua UserData implementations for Vector3, CFrame, Color3, etc.
//!
//! ## Table of Contents
//!
//! 1. **LuauVector3** — UserData wrapper for Vector3
//! 2. **LuauCFrame** — UserData wrapper for CFrame
//! 3. **LuauColor3** — UserData wrapper for Color3
//! 4. **inject_types** — Inject type constructors into Luau globals

use crate::scripting::{Vector3, CFrame, Color3};

#[cfg(feature = "luau")]
use mlua::{UserData, UserDataMethods, UserDataFields, Lua, Result as LuaResult, Value, MetaMethod, FromLua};

/// Read a script value type out of its userdata without holding a borrow.
///
/// `AnyUserData::borrow` takes an exclusive lock under mlua's `send`
/// feature, so it fails whenever the same object is already borrowed: as
/// `self` inside its own metamethod (`v + v`, `v:Dot(v)`, `a == a`). A
/// scoped borrow takes a shared lock and copies the value out.
#[cfg(feature = "luau")]
pub trait UserDataPeek {
    fn peek<T: Clone + 'static>(&self) -> LuaResult<T>;
}

#[cfg(feature = "luau")]
impl UserDataPeek for mlua::AnyUserData {
    fn peek<T: Clone + 'static>(&self) -> LuaResult<T> {
        self.borrow_scoped::<T, T>(|v| v.clone())
    }
}

/// `__eq` for script value types. Luau calls `__eq` even when both
/// operands are the same object, so identity answers first and only two
/// distinct objects are compared by value.
#[cfg(feature = "luau")]
pub fn userdata_eq<T: Clone + 'static>(a: &Value, b: &Value, same: impl Fn(&T, &T) -> bool) -> bool {
    let (Value::UserData(x), Value::UserData(y)) = (a, b) else { return false };
    if x == y {
        return true;
    }
    match (x.peek::<T>(), y.peek::<T>()) {
        (Ok(p), Ok(q)) => same(&p, &q),
        _ => false,
    }
}

// ============================================================================
// 1. LuauVector3 — UserData wrapper for Vector3
// ============================================================================

/// Luau-compatible Vector3 wrapping the shared scripting type.
#[derive(Debug, Clone, Copy)]
pub struct LuauVector3(pub Vector3);

impl LuauVector3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(Vector3::new(x, y, z))
    }

    pub fn inner(&self) -> &Vector3 {
        &self.0
    }
}

impl From<Vector3> for LuauVector3 {
    fn from(v: Vector3) -> Self {
        Self(v)
    }
}

impl From<LuauVector3> for Vector3 {
    fn from(v: LuauVector3) -> Self {
        v.0
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauVector3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // `typeof(v) == "Vector3"`, as in Roblox.
        fields.add_meta_field("__type", "Vector3");
        fields.add_field_method_get("X", |_, this| Ok(this.0.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.y));
        fields.add_field_method_get("Z", |_, this| Ok(this.0.z));
        fields.add_field_method_get("Magnitude", |_, this| Ok(this.0.magnitude()));
        fields.add_field_method_get("Unit", |_, this| Ok(LuauVector3(this.0.unit())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dot product
        methods.add_method("Dot", |_, this, other: LuauVector3| {
            Ok(this.0.dot(&other.0))
        });

        // Cross product
        methods.add_method("Cross", |_, this, other: LuauVector3| {
            Ok(LuauVector3(this.0.cross(&other.0)))
        });

        // Linear interpolation
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuauVector3, f64)| {
            Ok(LuauVector3(this.0.lerp(&goal.0, alpha)))
        });

        // Fuzzy equality
        methods.add_method("FuzzyEq", |_, this, (other, epsilon): (LuauVector3, Option<f64>)| {
            let eps = epsilon.unwrap_or(1e-5);
            Ok(this.0.fuzzy_eq(&other.0, eps))
        });

        // Metamethods for operators
        methods.add_meta_method(MetaMethod::Add, |_, this, other: LuauVector3| {
            Ok(LuauVector3(this.0 + other.0))
        });

        methods.add_meta_method(MetaMethod::Sub, |_, this, other: LuauVector3| {
            Ok(LuauVector3(this.0 - other.0))
        });

        // `*` and `/` are functions, not methods: Luau passes the operands in
        // source order, so `2 * v` arrives as (2, v) and a method would try to
        // read the number as a Vector3.
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            match (vector3_operand(&a), vector3_operand(&b), number_operand(&a), number_operand(&b)) {
                // Component-wise multiplication
                (Some(x), Some(y), _, _) => Ok(LuauVector3(Vector3::new(x.x * y.x, x.y * y.y, x.z * y.z))),
                (Some(x), None, _, Some(n)) => Ok(LuauVector3(x * n)),
                (None, Some(y), Some(n), _) => Ok(LuauVector3(y * n)),
                _ => Err(mlua::Error::RuntimeError("Vector3 * expects a number or Vector3".into())),
            }
        });

        methods.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| {
            match (vector3_operand(&a), vector3_operand(&b), number_operand(&a), number_operand(&b)) {
                (Some(x), Some(y), _, _) => Ok(LuauVector3(Vector3::new(x.x / y.x, x.y / y.y, x.z / y.z))),
                (Some(x), None, _, Some(n)) => Ok(LuauVector3(x / n)),
                (None, Some(y), Some(n), _) => Ok(LuauVector3(Vector3::new(n / y.x, n / y.y, n / y.z))),
                _ => Err(mlua::Error::RuntimeError("Vector3 / expects a number or Vector3".into())),
            }
        });

        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| {
            Ok(LuauVector3(-this.0))
        });

        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauVector3>(&a, &b, |p, q| p.0 == q.0))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.x, this.0.y, this.0.z))
        });
    }
}

#[cfg(feature = "luau")]
fn vector3_operand(v: &Value) -> Option<Vector3> {
    match v {
        Value::UserData(ud) => ud.peek::<LuauVector3>().ok().map(|x| x.0),
        _ => None,
    }
}

#[cfg(feature = "luau")]
fn number_operand(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauVector3 {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let v = ud.peek::<LuauVector3>()?;
                Ok(v)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "Vector3".to_string(),
                message: Some("expected Vector3".to_string()),
            }),
        }
    }
}

// ============================================================================
// 2. LuauCFrame — UserData wrapper for CFrame
// ============================================================================

/// Luau-compatible CFrame wrapping the shared scripting type.
#[derive(Debug, Clone, Copy)]
pub struct LuauCFrame(pub CFrame);

impl LuauCFrame {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(CFrame::new(x, y, z))
    }

    /// Create an identity CFrame (position at origin, no rotation)
    pub fn identity() -> Self {
        Self(CFrame::new(0.0, 0.0, 0.0))
    }

    pub fn inner(&self) -> &CFrame {
        &self.0
    }
}

impl From<CFrame> for LuauCFrame {
    fn from(cf: CFrame) -> Self {
        Self(cf)
    }
}

impl From<LuauCFrame> for CFrame {
    fn from(cf: LuauCFrame) -> Self {
        cf.0
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauCFrame {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "CFrame");
        fields.add_field_method_get("Position", |_, this| {
            Ok(LuauVector3(this.0.position))
        });
        fields.add_field_method_get("p", |_, this| {
            Ok(LuauVector3(this.0.position))
        });
        fields.add_field_method_get("Rotation", |_, this| {
            Ok(LuauCFrame(this.0.rotation_only()))
        });
        fields.add_field_method_get("XVector", |_, this| {
            Ok(LuauVector3(this.0.right_vector()))
        });
        fields.add_field_method_get("YVector", |_, this| {
            Ok(LuauVector3(this.0.up_vector()))
        });
        fields.add_field_method_get("ZVector", |_, this| {
            Ok(LuauVector3(this.0.back_vector()))
        });
        fields.add_field_method_get("X", |_, this| Ok(this.0.position.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.position.y));
        fields.add_field_method_get("Z", |_, this| Ok(this.0.position.z));
        fields.add_field_method_get("LookVector", |_, this| {
            Ok(LuauVector3(this.0.look_vector()))
        });
        fields.add_field_method_get("RightVector", |_, this| {
            Ok(LuauVector3(this.0.right_vector()))
        });
        fields.add_field_method_get("UpVector", |_, this| {
            Ok(LuauVector3(this.0.up_vector()))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Inverse
        methods.add_method("Inverse", |_, this, ()| {
            Ok(LuauCFrame(this.0.inverse()))
        });

        // Point transformations
        methods.add_method("PointToWorldSpace", |_, this, point: LuauVector3| {
            Ok(LuauVector3(this.0.point_to_world_space(point.0)))
        });

        methods.add_method("PointToObjectSpace", |_, this, point: LuauVector3| {
            Ok(LuauVector3(this.0.point_to_object_space(point.0)))
        });

        methods.add_method("VectorToWorldSpace", |_, this, vec: LuauVector3| {
            Ok(LuauVector3(this.0.vector_to_world_space(vec.0)))
        });

        methods.add_method("VectorToObjectSpace", |_, this, vec: LuauVector3| {
            Ok(LuauVector3(this.0.vector_to_object_space(vec.0)))
        });

        // CFrame transformations
        methods.add_method("ToWorldSpace", |_, this, cf: LuauCFrame| {
            Ok(LuauCFrame(this.0.to_world_space(cf.0)))
        });

        methods.add_method("ToObjectSpace", |_, this, cf: LuauCFrame| {
            Ok(LuauCFrame(this.0.to_object_space(cf.0)))
        });

        // Interpolation
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuauCFrame, f64)| {
            Ok(LuauCFrame(this.0.lerp(&goal.0, alpha)))
        });

        // Get components
        methods.add_method("GetComponents", |_, this, ()| {
            let (x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22) = this.0.components();
            Ok((x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22))
        });

        // Euler angles
        methods.add_method("ToEulerAnglesXYZ", |_, this, ()| {
            let (rx, ry, rz) = this.0.to_euler_angles_xyz();
            Ok((rx, ry, rz))
        });

        methods.add_method("ToEulerAnglesYXZ", |_, this, ()| {
            let (rx, ry, rz) = this.0.to_euler_angles_yxz();
            Ok((rx, ry, rz))
        });

        methods.add_method("ToOrientation", |_, this, ()| {
            let (rx, ry, rz) = this.0.to_euler_angles_yxz();
            Ok((rx, ry, rz))
        });

        // Axis-angle
        methods.add_method("ToAxisAngle", |_, this, ()| {
            let (axis, angle) = this.0.to_axis_angle();
            Ok((LuauVector3(axis), angle))
        });

        // Metamethods
        methods.add_meta_method(MetaMethod::Mul, |lua, this, value: Value| {
            match value {
                Value::UserData(ud) => {
                    if let Ok(other) = ud.peek::<LuauCFrame>() {
                        Ok(Value::UserData(lua.create_userdata(LuauCFrame(this.0 * other.0))?))
                    } else if let Ok(vec) = ud.peek::<LuauVector3>() {
                        // CFrame * Vector3 = transform point
                        Ok(Value::UserData(lua.create_userdata(LuauVector3(this.0.point_to_world_space(vec.0)))?))
                    } else {
                        Err(mlua::Error::RuntimeError("Expected CFrame or Vector3".into()))
                    }
                }
                _ => Err(mlua::Error::RuntimeError("Expected CFrame or Vector3".into())),
            }
        });

        methods.add_meta_method(MetaMethod::Add, |_, this, offset: LuauVector3| {
            Ok(LuauCFrame(this.0 + offset.0))
        });

        methods.add_meta_method(MetaMethod::Sub, |_, this, offset: LuauVector3| {
            Ok(LuauCFrame(this.0 - offset.0))
        });

        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauCFrame>(&a, &b, |p, q| p.0 == q.0))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let pos = this.0.position;
            Ok(format!("{}, {}, {}", pos.x, pos.y, pos.z))
        });
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauCFrame {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let cf = ud.peek::<LuauCFrame>()?;
                Ok(cf)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "CFrame".to_string(),
                message: Some("expected CFrame".to_string()),
            }),
        }
    }
}

// ============================================================================
// 3. LuauColor3 — UserData wrapper for Color3
// ============================================================================

/// Luau-compatible Color3 wrapping the shared scripting type.
#[derive(Debug, Clone, Copy)]
pub struct LuauColor3(pub Color3);

impl LuauColor3 {
    pub fn new(r: f64, g: f64, b: f64) -> Self {
        Self(Color3::new(r, g, b))
    }

    pub fn inner(&self) -> &Color3 {
        &self.0
    }
}

impl From<Color3> for LuauColor3 {
    fn from(c: Color3) -> Self {
        Self(c)
    }
}

impl From<LuauColor3> for Color3 {
    fn from(c: LuauColor3) -> Self {
        c.0
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauColor3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Color3");
        fields.add_field_method_get("R", |_, this| Ok(this.0.r));
        fields.add_field_method_get("G", |_, this| Ok(this.0.g));
        fields.add_field_method_get("B", |_, this| Ok(this.0.b));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Interpolation
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuauColor3, f64)| {
            Ok(LuauColor3(this.0.lerp(&goal.0, alpha)))
        });

        // HSV conversion
        methods.add_method("ToHSV", |_, this, ()| {
            let (h, s, v) = this.0.to_hsv();
            Ok((h, s, v))
        });

        // Hex conversion
        methods.add_method("ToHex", |_, this, ()| {
            Ok(this.0.to_hex())
        });

        // Metamethods
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauColor3>(&a, &b, |p, q| p.0 == q.0))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.r, this.0.g, this.0.b))
        });
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauColor3 {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let c = ud.peek::<LuauColor3>()?;
                Ok(c)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "Color3".to_string(),
                message: Some("expected Color3".to_string()),
            }),
        }
    }
}

// ============================================================================
// 4. inject_types — Inject type constructors into Luau globals
// ============================================================================

/// Inject Vector3, CFrame, Color3 constructors into Luau globals.
#[cfg(feature = "luau")]
pub fn inject_types(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();

    // Vector3 constructor table
    let vector3_table = lua.create_table()?;
    
    vector3_table.set("new", lua.create_function(|_, (x, y, z): (f64, f64, f64)| {
        Ok(LuauVector3::new(x, y, z))
    })?)?;

    // Vector3 constants
    vector3_table.set("zero", LuauVector3::new(0.0, 0.0, 0.0))?;
    vector3_table.set("one", LuauVector3::new(1.0, 1.0, 1.0))?;
    vector3_table.set("xAxis", LuauVector3::new(1.0, 0.0, 0.0))?;
    vector3_table.set("yAxis", LuauVector3::new(0.0, 1.0, 0.0))?;
    vector3_table.set("zAxis", LuauVector3::new(0.0, 0.0, 1.0))?;

    globals.set("Vector3", vector3_table)?;

    // CFrame constructor table
    let cframe_table = lua.create_table()?;

    cframe_table.set("new", lua.create_function(|_, args: mlua::Variadic<Value>| {
        let args: Vec<Value> = args.into_iter().collect();
        // CFrame.new(pos) / CFrame.new(pos, lookAt): the Vector3 forms are
        // the most common in real scripts.
        if let Some(Value::UserData(first)) = args.first() {
            let pos = first.peek::<LuauVector3>()?.0;
            return match args.get(1) {
                Some(Value::UserData(second)) => {
                    let target = second.peek::<LuauVector3>()?.0;
                    Ok(LuauCFrame(CFrame::look_at(pos, target, None)))
                }
                _ => Ok(LuauCFrame(CFrame::from_position(pos))),
            };
        }
        let mut nums = Vec::with_capacity(args.len());
        for v in &args {
            match v {
                Value::Number(n) => nums.push(*n),
                Value::Integer(i) => nums.push(*i as f64),
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "CFrame.new: expected number, got {}",
                        other.type_name()
                    )))
                }
            }
        }
        match nums.len() {
            0 => Ok(LuauCFrame(CFrame::IDENTITY)),
            3 => Ok(LuauCFrame::new(nums[0], nums[1], nums[2])),
            // x, y, z, qx, qy, qz, qw
            7 => {
                let mut cf = CFrame::from_quaternion([nums[3], nums[4], nums[5], nums[6]]);
                cf.position = Vector3::new(nums[0], nums[1], nums[2]);
                Ok(LuauCFrame(cf))
            }
            // x, y, z, R00, R01, R02, R10, R11, R12, R20, R21, R22 (row-major)
            12 => Ok(LuauCFrame(CFrame::from_rotation_matrix(
                Vector3::new(nums[0], nums[1], nums[2]),
                [
                    [nums[3], nums[4], nums[5]],
                    [nums[6], nums[7], nums[8]],
                    [nums[9], nums[10], nums[11]],
                ],
            ))),
            _ => Err(mlua::Error::RuntimeError(
                "CFrame.new expects 0, 3, 7 or 12 numbers, or one or two Vector3".into()
            )),
        }
    })?)?;

    cframe_table.set("Angles", lua.create_function(|_, (rx, ry, rz): (f64, f64, f64)| {
        Ok(LuauCFrame(CFrame::angles(rx, ry, rz)))
    })?)?;

    cframe_table.set("fromEulerAnglesXYZ", lua.create_function(|_, (rx, ry, rz): (f64, f64, f64)| {
        Ok(LuauCFrame(CFrame::from_euler_angles_xyz(rx, ry, rz)))
    })?)?;

    cframe_table.set("fromEulerAnglesYXZ", lua.create_function(|_, (rx, ry, rz): (f64, f64, f64)| {
        Ok(LuauCFrame(CFrame::from_euler_angles_yxz(rx, ry, rz)))
    })?)?;

    cframe_table.set("fromOrientation", lua.create_function(|_, (rx, ry, rz): (f64, f64, f64)| {
        Ok(LuauCFrame(CFrame::from_euler_angles_yxz(rx, ry, rz)))
    })?)?;

    cframe_table.set("fromAxisAngle", lua.create_function(|_, (axis, angle): (LuauVector3, f64)| {
        Ok(LuauCFrame(CFrame::from_axis_angle(axis.0, angle)))
    })?)?;

    cframe_table.set("lookAt", lua.create_function(|_, (pos, target, up): (LuauVector3, LuauVector3, Option<LuauVector3>)| {
        let up_vec = up.map(|u| u.0);
        Ok(LuauCFrame(CFrame::look_at(pos.0, target.0, up_vec)))
    })?)?;

    // CFrame constants
    cframe_table.set("identity", LuauCFrame(CFrame::IDENTITY))?;

    globals.set("CFrame", cframe_table)?;

    // Color3 constructor table
    let color3_table = lua.create_table()?;

    color3_table.set("new", lua.create_function(|_, (r, g, b): (f64, f64, f64)| {
        Ok(LuauColor3::new(r, g, b))
    })?)?;

    color3_table.set("fromRGB", lua.create_function(|_, (r, g, b): (u8, u8, u8)| {
        Ok(LuauColor3(Color3::from_rgb(r, g, b)))
    })?)?;

    color3_table.set("fromHSV", lua.create_function(|_, (h, s, v): (f64, f64, f64)| {
        Ok(LuauColor3(Color3::from_hsv(h, s, v)))
    })?)?;

    color3_table.set("fromHex", lua.create_function(|_, hex: String| {
        match Color3::from_hex(&hex) {
            Some(c) => Ok(LuauColor3(c)),
            None => Err(mlua::Error::RuntimeError(format!("Invalid hex color: {}", hex))),
        }
    })?)?;

    globals.set("Color3", color3_table)?;

    // ========================================================================
    // P1 Types: UDim, UDim2, TweenInfo
    // ========================================================================

    // UDim constructor table
    let udim_table = lua.create_table()?;

    udim_table.set("new", lua.create_function(|_, (scale, offset): (f64, f64)| {
        Ok(LuauUDim::new(scale, offset))
    })?)?;

    globals.set("UDim", udim_table)?;

    // UDim2 constructor table
    let udim2_table = lua.create_table()?;

    udim2_table.set("new", lua.create_function(|_, (xs, xo, ys, yo): (f64, f64, f64, f64)| {
        Ok(LuauUDim2::new(xs, xo, ys, yo))
    })?)?;

    udim2_table.set("fromScale", lua.create_function(|_, (xs, ys): (f64, f64)| {
        Ok(LuauUDim2::from_scale(xs, ys))
    })?)?;

    udim2_table.set("fromOffset", lua.create_function(|_, (xo, yo): (f64, f64)| {
        Ok(LuauUDim2::from_offset(xo, yo))
    })?)?;

    globals.set("UDim2", udim2_table)?;

    // TweenInfo constructor table
    let tweeninfo_table = lua.create_table()?;

    tweeninfo_table.set("new", lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
        let args: Vec<mlua::Value> = args.into_iter().collect();
        
        let time = match args.get(0) {
            Some(mlua::Value::Number(n)) => *n,
            Some(mlua::Value::Integer(i)) => *i as f64,
            _ => 1.0,
        };
        
        let easing_style = match args.get(1) {
            Some(mlua::Value::Integer(i)) => *i as i32,
            Some(mlua::Value::Number(n)) => *n as i32,
            _ => 0,
        };
        
        let easing_direction = match args.get(2) {
            Some(mlua::Value::Integer(i)) => *i as i32,
            Some(mlua::Value::Number(n)) => *n as i32,
            _ => 1,
        };
        
        let repeat_count = match args.get(3) {
            Some(mlua::Value::Integer(i)) => *i as i32,
            Some(mlua::Value::Number(n)) => *n as i32,
            _ => 0,
        };
        
        let reverses = match args.get(4) {
            Some(mlua::Value::Boolean(b)) => *b,
            _ => false,
        };
        
        let delay_time = match args.get(5) {
            Some(mlua::Value::Number(n)) => *n,
            Some(mlua::Value::Integer(i)) => *i as f64,
            _ => 0.0,
        };
        
        Ok(LuauTweenInfo {
            time,
            easing_style,
            easing_direction,
            repeat_count,
            reverses,
            delay_time,
        })
    })?)?;

    globals.set("TweenInfo", tweeninfo_table)?;

    Ok(())
}

// ============================================================================
// P1: LuauUDim — UserData wrapper for UDim
// ============================================================================

/// Luau-compatible UDim (scale + offset).
#[derive(Debug, Clone, Copy)]
pub struct LuauUDim {
    pub scale: f64,
    pub offset: f64,
}

impl LuauUDim {
    pub fn new(scale: f64, offset: f64) -> Self {
        Self { scale, offset }
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauUDim {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "UDim");
        fields.add_field_method_get("Scale", |_, this| Ok(this.scale));
        fields.add_field_method_get("Offset", |_, this| Ok(this.offset));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Add, |_, this, other: LuauUDim| {
            Ok(LuauUDim::new(this.scale + other.scale, this.offset + other.offset))
        });

        methods.add_meta_method(MetaMethod::Sub, |_, this, other: LuauUDim| {
            Ok(LuauUDim::new(this.scale - other.scale, this.offset - other.offset))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}", this.scale, this.offset))
        });
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauUDim {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let u = ud.peek::<LuauUDim>()?;
                Ok(u)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "UDim".to_string(),
                message: Some("expected UDim".to_string()),
            }),
        }
    }
}

// ============================================================================
// P1: LuauUDim2 — UserData wrapper for UDim2
// ============================================================================

/// Luau-compatible UDim2 (X and Y UDims).
#[derive(Debug, Clone, Copy)]
pub struct LuauUDim2 {
    pub x_scale: f64,
    pub x_offset: f64,
    pub y_scale: f64,
    pub y_offset: f64,
}

impl LuauUDim2 {
    pub fn new(x_scale: f64, x_offset: f64, y_scale: f64, y_offset: f64) -> Self {
        Self { x_scale, x_offset, y_scale, y_offset }
    }

    pub fn from_scale(x_scale: f64, y_scale: f64) -> Self {
        Self { x_scale, x_offset: 0.0, y_scale, y_offset: 0.0 }
    }

    pub fn from_offset(x_offset: f64, y_offset: f64) -> Self {
        Self { x_scale: 0.0, x_offset, y_scale: 0.0, y_offset }
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauUDim2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "UDim2");
        fields.add_field_method_get("X", |_, this| {
            Ok(LuauUDim::new(this.x_scale, this.x_offset))
        });
        fields.add_field_method_get("Y", |_, this| {
            Ok(LuauUDim::new(this.y_scale, this.y_offset))
        });
        fields.add_field_method_get("Width", |_, this| {
            Ok(LuauUDim::new(this.x_scale, this.x_offset))
        });
        fields.add_field_method_get("Height", |_, this| {
            Ok(LuauUDim::new(this.y_scale, this.y_offset))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuauUDim2, f64)| {
            Ok(LuauUDim2 {
                x_scale: this.x_scale + (goal.x_scale - this.x_scale) * alpha,
                x_offset: this.x_offset + (goal.x_offset - this.x_offset) * alpha,
                y_scale: this.y_scale + (goal.y_scale - this.y_scale) * alpha,
                y_offset: this.y_offset + (goal.y_offset - this.y_offset) * alpha,
            })
        });

        methods.add_meta_method(MetaMethod::Add, |_, this, other: LuauUDim2| {
            Ok(LuauUDim2 {
                x_scale: this.x_scale + other.x_scale,
                x_offset: this.x_offset + other.x_offset,
                y_scale: this.y_scale + other.y_scale,
                y_offset: this.y_offset + other.y_offset,
            })
        });

        methods.add_meta_method(MetaMethod::Sub, |_, this, other: LuauUDim2| {
            Ok(LuauUDim2 {
                x_scale: this.x_scale - other.x_scale,
                x_offset: this.x_offset - other.x_offset,
                y_scale: this.y_scale - other.y_scale,
                y_offset: this.y_offset - other.y_offset,
            })
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{{{}, {}}}, {{{}, {}}}", 
                this.x_scale, this.x_offset, this.y_scale, this.y_offset))
        });
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauUDim2 {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let u = ud.peek::<LuauUDim2>()?;
                Ok(u)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "UDim2".to_string(),
                message: Some("expected UDim2".to_string()),
            }),
        }
    }
}

// ============================================================================
// P1: LuauTweenInfo — UserData wrapper for TweenInfo
// ============================================================================

/// Luau-compatible TweenInfo.
#[derive(Debug, Clone, Copy)]
pub struct LuauTweenInfo {
    pub time: f64,
    pub easing_style: i32,
    pub easing_direction: i32,
    pub repeat_count: i32,
    pub reverses: bool,
    pub delay_time: f64,
}

impl Default for LuauTweenInfo {
    fn default() -> Self {
        Self {
            time: 1.0,
            easing_style: 0,
            easing_direction: 1,
            repeat_count: 0,
            reverses: false,
            delay_time: 0.0,
        }
    }
}

#[cfg(feature = "luau")]
impl UserData for LuauTweenInfo {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Time", |_, this| Ok(this.time));
        fields.add_field_method_get("EasingStyle", |_, this| Ok(this.easing_style));
        fields.add_field_method_get("EasingDirection", |_, this| Ok(this.easing_direction));
        fields.add_field_method_get("RepeatCount", |_, this| Ok(this.repeat_count));
        fields.add_field_method_get("Reverses", |_, this| Ok(this.reverses));
        fields.add_field_method_get("DelayTime", |_, this| Ok(this.delay_time));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("TweenInfo({}, {}, {}, {}, {}, {})",
                this.time, this.easing_style, this.easing_direction,
                this.repeat_count, this.reverses, this.delay_time))
        });
    }
}

#[cfg(feature = "luau")]
impl FromLua for LuauTweenInfo {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => {
                let t = ud.peek::<LuauTweenInfo>()?;
                Ok(t)
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "TweenInfo".to_string(),
                message: Some("expected TweenInfo".to_string()),
            }),
        }
    }
}

/// Fallback when luau feature is not enabled
#[cfg(not(feature = "luau"))]
pub fn inject_types(_lua: &()) -> Result<(), String> {
    Ok(())
}
