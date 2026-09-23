//! Value types the Play VM adds on top of `luau::types`: Vector2, EnumItem,
//! Random, NumberRange, NumberSequence, ColorSequence, Ray, RaycastParams
//! and BrickColor.

use mlua::{FromLua, Lua, MetaMethod, Result as LuaResult, UserData, UserDataFields, UserDataMethods, Value};

use crate::datamodel::{EnumItem, InstanceId};
use crate::luau::types::{userdata_eq, LuauColor3, LuauVector3, UserDataPeek};
use crate::scripting::{Color3, NumberRange, Vector2, Vector3};

// ============================================================================
// Vector2
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LuauVector2(pub Vector2);

impl UserData for LuauVector2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Vector2");
        fields.add_field_method_get("X", |_, this| Ok(this.0.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.y));
        fields.add_field_method_get("Magnitude", |_, this| Ok(this.0.magnitude()));
        fields.add_field_method_get("Unit", |_, this| Ok(LuauVector2(this.0.unit())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Dot", |_, this, o: LuauVector2| Ok(this.0.dot(&o.0)));
        methods.add_method("Cross", |_, this, o: LuauVector2| Ok(this.0.cross(&o.0)));
        methods.add_method("Lerp", |_, this, (o, a): (LuauVector2, f64)| Ok(LuauVector2(this.0.lerp(&o.0, a))));
        methods.add_method("Max", |_, this, o: LuauVector2| Ok(LuauVector2(this.0.max(&o.0))));
        methods.add_method("Min", |_, this, o: LuauVector2| Ok(LuauVector2(this.0.min(&o.0))));
        methods.add_method("Abs", |_, this, ()| Ok(LuauVector2(this.0.abs())));
        methods.add_method("Floor", |_, this, ()| Ok(LuauVector2(this.0.floor())));
        methods.add_method("Ceil", |_, this, ()| Ok(LuauVector2(this.0.ceil())));
        methods.add_meta_method(MetaMethod::Add, |_, this, o: LuauVector2| Ok(LuauVector2(this.0 + o.0)));
        methods.add_meta_method(MetaMethod::Sub, |_, this, o: LuauVector2| Ok(LuauVector2(this.0 - o.0)));
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| Ok(LuauVector2(-this.0)));
        // Functions, not methods: `2 * v` arrives as (2, v).
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            match (vector2_operand(&a), vector2_operand(&b), number_operand(&a), number_operand(&b)) {
                (Some(x), Some(y), _, _) => Ok(LuauVector2(x * y)),
                (Some(x), None, _, Some(n)) => Ok(LuauVector2(x * n)),
                (None, Some(y), Some(n), _) => Ok(LuauVector2(y * n)),
                _ => Err(mlua::Error::RuntimeError("Vector2 * expects a number or Vector2".into())),
            }
        });
        methods.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| {
            match (vector2_operand(&a), vector2_operand(&b), number_operand(&a), number_operand(&b)) {
                (Some(x), Some(y), _, _) => Ok(LuauVector2(x / y)),
                (Some(x), None, _, Some(n)) => Ok(LuauVector2(x / n)),
                (None, Some(y), Some(n), _) => Ok(LuauVector2(Vector2::new(n / y.x, n / y.y))),
                _ => Err(mlua::Error::RuntimeError("Vector2 / expects a number or Vector2".into())),
            }
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauVector2>(&a, &b, |p, q| p.0 == q.0))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(format!("{}, {}", this.0.x, this.0.y)));
    }
}

fn vector2_operand(v: &Value) -> Option<Vector2> {
    match v {
        Value::UserData(ud) => ud.peek::<LuauVector2>().ok().map(|x| x.0),
        _ => None,
    }
}

fn number_operand(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

impl FromLua for LuauVector2 {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauVector2>()?),
            other => Err(conversion_error(&other, "Vector2")),
        }
    }
}

// ============================================================================
// EnumItem
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuauEnumItem(pub EnumItem);

impl UserData for LuauEnumItem {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "EnumItem");
        fields.add_field_method_get("Name", |_, this| Ok(this.0.name.clone()));
        fields.add_field_method_get("Value", |_, this| Ok(enum_value(&this.0)));
        fields.add_field_method_get("EnumType", |_, this| Ok(this.0.enum_type.clone()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("IsA", |_, this, ty: String| Ok(this.0.enum_type == ty));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauEnumItem>(&a, &b, |p, q| p.0 == q.0))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0.to_string()));
    }
}

impl FromLua for LuauEnumItem {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauEnumItem>()?.clone()),
            other => Err(conversion_error(&other, "EnumItem")),
        }
    }
}

/// Numeric value for the enums scripts commonly compare numerically.
/// Unknown items get a stable small hash so `Value` is at least consistent.
pub fn enum_value(item: &EnumItem) -> f64 {
    let known: &[(&str, &[&str])] = &[
        ("EasingStyle", &["Linear", "Sine", "Back", "Quad", "Quart", "Quint", "Bounce", "Elastic", "Exponential", "Circular", "Cubic"]),
        ("EasingDirection", &["In", "Out", "InOut"]),
        ("RaycastFilterType", &["Exclude", "Include"]),
        ("UserInputState", &["Begin", "Change", "End", "Cancel", "None"]),
        ("CameraType", &["Fixed", "Attach", "Watch", "Track", "Follow", "Custom", "Scriptable", "Orbital"]),
        ("HumanoidStateType", &["FallingDown", "Ragdoll", "GettingUp", "Jumping", "Swimming", "Freefall", "Flying", "Landed", "Running", "RunningNoPhysics", "StrafingNoPhysics", "Climbing", "Seated", "PlatformStanding", "Dead", "Physics", "None"]),
    ];
    for (ty, names) in known {
        if *ty == item.enum_type {
            if let Some(i) = names.iter().position(|n| *n == item.name) {
                return i as f64;
            }
        }
    }
    let mut h: u32 = 2166136261;
    for b in item.name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 10_000) as f64
}

// ============================================================================
// Random
// ============================================================================

/// Roblox `Random`: a seeded PCG32 generator per object.
#[derive(Debug, Clone)]
pub struct LuauRandom {
    state: u64,
    inc: u64,
}

impl LuauRandom {
    pub fn new(seed: u64) -> Self {
        let mut r = Self { state: 0, inc: (seed << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed ^ 0x853c_49e6_748f_ea9b);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        let hi = (self.next_u32() >> 5) as u64; // 27 bits
        let lo = (self.next_u32() >> 6) as u64; // 26 bits
        ((hi << 26) | lo) as f64 / (1u64 << 53) as f64
    }
}

impl UserData for LuauRandom {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Random");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("NextNumber", |_, this, (min, max): (Option<f64>, Option<f64>)| {
            let (min, max) = (min.unwrap_or(0.0), max.unwrap_or(1.0));
            Ok(min + (max - min) * this.next_f64())
        });
        methods.add_method_mut("NextInteger", |_, this, (min, max): (f64, f64)| {
            let (lo, hi) = (min.floor().min(max.floor()), min.floor().max(max.floor()));
            let span = (hi - lo + 1.0).max(1.0);
            Ok((lo + (this.next_f64() * span).floor()).min(hi))
        });
        methods.add_method_mut("NextUnitVector", |_, this, ()| {
            // Uniform on the sphere.
            let z = 2.0 * this.next_f64() - 1.0;
            let t = std::f64::consts::TAU * this.next_f64();
            let r = (1.0 - z * z).max(0.0).sqrt();
            Ok(LuauVector3(Vector3::new(r * t.cos(), r * t.sin(), z)))
        });
        methods.add_method_mut("Shuffle", |_, this, tbl: mlua::Table| {
            let n = tbl.raw_len();
            for i in (2..=n).rev() {
                let j = 1 + (this.next_f64() * i as f64).floor() as usize;
                let a: Value = tbl.raw_get(i)?;
                let b: Value = tbl.raw_get(j)?;
                tbl.raw_set(i, b)?;
                tbl.raw_set(j, a)?;
            }
            Ok(())
        });
        methods.add_method("Clone", |_, this, ()| Ok(this.clone()));
    }
}

// ============================================================================
// NumberRange / NumberSequence / ColorSequence
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LuauNumberRange(pub NumberRange);

impl UserData for LuauNumberRange {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "NumberRange");
        fields.add_field_method_get("Min", |_, this| Ok(this.0.min));
        fields.add_field_method_get("Max", |_, this| Ok(this.0.max));
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(format!("{} {}", this.0.min, this.0.max)));
    }
}

impl FromLua for LuauNumberRange {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauNumberRange>()?),
            Value::Number(n) => Ok(LuauNumberRange(NumberRange::single(n))),
            Value::Integer(i) => Ok(LuauNumberRange(NumberRange::single(i as f64))),
            other => Err(conversion_error(&other, "NumberRange")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LuauNumberSequence(pub Vec<(f64, f64)>);

impl UserData for LuauNumberSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "NumberSequence");
        fields.add_field_method_get("Keypoints", |lua, this| {
            let t = lua.create_table()?;
            for (i, (time, value)) in this.0.iter().enumerate() {
                let k = lua.create_table()?;
                k.set("Time", *time)?;
                k.set("Value", *value)?;
                k.set("Envelope", 0.0)?;
                t.set(i + 1, k)?;
            }
            Ok(t)
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LuauColorSequence(pub Vec<(f64, Color3)>);

impl UserData for LuauColorSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "ColorSequence");
        fields.add_field_method_get("Keypoints", |lua, this| {
            let t = lua.create_table()?;
            for (i, (time, c)) in this.0.iter().enumerate() {
                let k = lua.create_table()?;
                k.set("Time", *time)?;
                k.set("Value", LuauColor3(*c))?;
                t.set(i + 1, k)?;
            }
            Ok(t)
        });
    }
}

// ============================================================================
// Ray
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct LuauRay {
    pub origin: Vector3,
    pub direction: Vector3,
}

impl UserData for LuauRay {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "Ray");
        fields.add_field_method_get("Origin", |_, this| Ok(LuauVector3(this.origin)));
        fields.add_field_method_get("Direction", |_, this| Ok(LuauVector3(this.direction)));
        fields.add_field_method_get("Unit", |_, this| {
            Ok(LuauRay { origin: this.origin, direction: this.direction.unit() })
        });
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ClosestPoint", |_, this, p: LuauVector3| {
            let d = this.direction.unit();
            let t = (p.0 - this.origin).dot(&d).max(0.0);
            Ok(LuauVector3(this.origin + d * t))
        });
        methods.add_method("Distance", |_, this, p: LuauVector3| {
            let d = this.direction.unit();
            let t = (p.0 - this.origin).dot(&d).max(0.0);
            Ok((p.0 - (this.origin + d * t)).magnitude())
        });
    }
}

impl FromLua for LuauRay {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauRay>()?),
            other => Err(conversion_error(&other, "Ray")),
        }
    }
}

// ============================================================================
// RaycastParams
// ============================================================================

#[derive(Debug, Clone)]
pub struct LuauRaycastParams {
    /// `Exclude` (default) or `Include`.
    pub include: bool,
    pub filter: Vec<InstanceId>,
    pub ignore_water: bool,
    pub respect_can_collide: bool,
    pub collision_group: String,
}

impl Default for LuauRaycastParams {
    fn default() -> Self {
        Self {
            include: false,
            filter: Vec::new(),
            ignore_water: false,
            respect_can_collide: false,
            collision_group: "Default".into(),
        }
    }
}

impl UserData for LuauRaycastParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "RaycastParams");
        fields.add_field_method_get("FilterType", |_, this| {
            Ok(LuauEnumItem(EnumItem::new("RaycastFilterType", if this.include { "Include" } else { "Exclude" })))
        });
        fields.add_field_method_set("FilterType", |_, this, v: Value| {
            let name = match &v {
                Value::UserData(ud) => ud.peek::<LuauEnumItem>()?.0.name.clone(),
                Value::String(s) => s.to_str()?.to_string(),
                _ => String::new(),
            };
            // Blacklist/Whitelist are the pre-2022 names.
            this.include = matches!(name.as_str(), "Include" | "Whitelist");
            Ok(())
        });
        fields.add_field_method_get("FilterDescendantsInstances", |lua, this| {
            let t = lua.create_table()?;
            for (i, id) in this.filter.iter().enumerate() {
                t.set(i + 1, super::instance::handle(lua, *id)?)?;
            }
            Ok(t)
        });
        fields.add_field_method_set("FilterDescendantsInstances", |_, this, t: Option<mlua::Table>| {
            this.filter.clear();
            if let Some(t) = t {
                for v in t.sequence_values::<Value>() {
                    if let Value::UserData(ud) = v? {
                        if let Ok(inst) = ud.peek::<super::instance::LInst>() {
                            this.filter.push(inst.0);
                        }
                    }
                }
            }
            Ok(())
        });
        fields.add_field_method_get("IgnoreWater", |_, this| Ok(this.ignore_water));
        fields.add_field_method_set("IgnoreWater", |_, this, v: bool| {
            this.ignore_water = v;
            Ok(())
        });
        fields.add_field_method_get("RespectCanCollide", |_, this| Ok(this.respect_can_collide));
        fields.add_field_method_set("RespectCanCollide", |_, this, v: bool| {
            this.respect_can_collide = v;
            Ok(())
        });
        fields.add_field_method_get("CollisionGroup", |_, this| Ok(this.collision_group.clone()));
        fields.add_field_method_set("CollisionGroup", |_, this, v: String| {
            this.collision_group = v;
            Ok(())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("AddToFilter", |_, this, v: Value| {
            match v {
                Value::UserData(ud) => {
                    if let Ok(inst) = ud.peek::<super::instance::LInst>() {
                        this.filter.push(inst.0);
                    }
                }
                Value::Table(t) => {
                    for item in t.sequence_values::<Value>() {
                        if let Value::UserData(ud) = item? {
                            if let Ok(inst) = ud.peek::<super::instance::LInst>() {
                                this.filter.push(inst.0);
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        });
    }
}

impl FromLua for LuauRaycastParams {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauRaycastParams>()?.clone()),
            other => Err(conversion_error(&other, "RaycastParams")),
        }
    }
}

// ============================================================================
// BrickColor (a small named palette; enough for scripts that use names)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct LuauBrickColor {
    pub name: String,
    pub color: Color3,
}

const BRICK_COLORS: &[(&str, u8, u8, u8)] = &[
    ("White", 242, 243, 243),
    ("Institutional white", 248, 248, 248),
    ("Medium stone grey", 163, 162, 165),
    ("Dark stone grey", 99, 95, 98),
    ("Black", 27, 42, 53),
    ("Really black", 17, 17, 17),
    ("Bright red", 196, 40, 28),
    ("Really red", 255, 0, 0),
    ("Bright orange", 218, 133, 65),
    ("Bright yellow", 245, 205, 48),
    ("New Yeller", 255, 255, 0),
    ("Bright green", 75, 151, 75),
    ("Lime green", 0, 255, 0),
    ("Dark green", 40, 127, 71),
    ("Bright blue", 13, 105, 172),
    ("Really blue", 0, 0, 255),
    ("Cyan", 4, 175, 236),
    ("Toothpaste", 0, 255, 255),
    ("Bright violet", 107, 50, 124),
    ("Magenta", 170, 0, 170),
    ("Hot pink", 255, 0, 191),
    ("Reddish brown", 105, 64, 40),
    ("Brown", 124, 92, 70),
    ("Nougat", 204, 142, 105),
    ("Sand green", 120, 144, 130),
    ("Olive", 193, 190, 66),
    ("Camo", 58, 125, 21),
    ("Maroon", 117, 0, 0),
    ("Crimson", 151, 0, 0),
    ("Gold", 239, 184, 56),
];

impl LuauBrickColor {
    pub fn named(name: &str) -> Self {
        for (n, r, g, b) in BRICK_COLORS {
            if n.eq_ignore_ascii_case(name) {
                return Self { name: n.to_string(), color: Color3::from_rgb(*r, *g, *b) };
            }
        }
        Self::named("Medium stone grey")
    }

    /// The palette entry nearest to `c`.
    pub fn nearest(c: Color3) -> Self {
        let mut best = ("Medium stone grey", 163u8, 162u8, 165u8);
        let mut best_d = f64::MAX;
        for e in BRICK_COLORS {
            let d = (c.r * 255.0 - e.1 as f64).powi(2)
                + (c.g * 255.0 - e.2 as f64).powi(2)
                + (c.b * 255.0 - e.3 as f64).powi(2);
            if d < best_d {
                best_d = d;
                best = *e;
            }
        }
        Self { name: best.0.to_string(), color: Color3::from_rgb(best.1, best.2, best.3) }
    }
}

impl UserData for LuauBrickColor {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__type", "BrickColor");
        fields.add_field_method_get("Name", |_, this| Ok(this.name.clone()));
        fields.add_field_method_get("Color", |_, this| Ok(LuauColor3(this.color)));
        fields.add_field_method_get("r", |_, this| Ok(this.color.r));
        fields.add_field_method_get("g", |_, this| Ok(this.color.g));
        fields.add_field_method_get("b", |_, this| Ok(this.color.b));
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.name.clone()));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(userdata_eq::<LuauBrickColor>(&a, &b, |p, q| p.name == q.name))
        });
    }
}

impl FromLua for LuauBrickColor {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.peek::<LuauBrickColor>()?.clone()),
            Value::String(s) => Ok(LuauBrickColor::named(&s.to_str()?)),
            other => Err(conversion_error(&other, "BrickColor")),
        }
    }
}

pub fn conversion_error(value: &Value, to: &str) -> mlua::Error {
    mlua::Error::FromLuaConversionError {
        from: value.type_name(),
        to: to.to_string(),
        message: Some(format!("expected {}", to)),
    }
}

/// Install the constructors for these types as globals.
pub fn install(lua: &Lua, globals: &mlua::Table) -> LuaResult<()> {
    // Vector2
    let v2 = lua.create_table()?;
    v2.set("new", lua.create_function(|_, (x, y): (Option<f64>, Option<f64>)| {
        Ok(LuauVector2(Vector2::new(x.unwrap_or(0.0), y.unwrap_or(0.0))))
    })?)?;
    v2.set("zero", LuauVector2(Vector2::ZERO))?;
    v2.set("one", LuauVector2(Vector2::ONE))?;
    v2.set("xAxis", LuauVector2(Vector2::X_AXIS))?;
    v2.set("yAxis", LuauVector2(Vector2::Y_AXIS))?;
    globals.set("Vector2", v2)?;

    // Random
    let random = lua.create_table()?;
    random.set("new", lua.create_function(|_, seed: Option<f64>| {
        let seed = seed.map(|s| s as i64 as u64).unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x5eed)
        });
        Ok(LuauRandom::new(seed))
    })?)?;
    globals.set("Random", random)?;

    // NumberRange
    let nr = lua.create_table()?;
    nr.set("new", lua.create_function(|_, (a, b): (f64, Option<f64>)| {
        Ok(LuauNumberRange(NumberRange::new(a, b.unwrap_or(a))))
    })?)?;
    globals.set("NumberRange", nr)?;

    // NumberSequence.new(n) | new(a, b) | new({NumberSequenceKeypoint...})
    let ns = lua.create_table()?;
    ns.set("new", lua.create_function(|_, args: mlua::Variadic<Value>| {
        let args: Vec<Value> = args.into_iter().collect();
        let num = |v: &Value| match v {
            Value::Number(n) => Some(*n),
            Value::Integer(i) => Some(*i as f64),
            _ => None,
        };
        match args.as_slice() {
            [a] if num(a).is_some() => {
                let a = num(a).unwrap_or(0.0);
                Ok(LuauNumberSequence(vec![(0.0, a), (1.0, a)]))
            }
            [a, b] if num(a).is_some() && num(b).is_some() => {
                Ok(LuauNumberSequence(vec![(0.0, num(a).unwrap_or(0.0)), (1.0, num(b).unwrap_or(0.0))]))
            }
            [Value::Table(t)] => {
                let mut keys = Vec::new();
                for k in t.sequence_values::<mlua::Table>() {
                    let k = k?;
                    keys.push((k.get::<f64>("Time")?, k.get::<f64>("Value")?));
                }
                Ok(LuauNumberSequence(keys))
            }
            _ => Err(mlua::Error::RuntimeError("NumberSequence.new: bad arguments".into())),
        }
    })?)?;
    globals.set("NumberSequence", ns)?;
    let nsk = lua.create_table()?;
    nsk.set("new", lua.create_function(|lua, (time, value, env): (f64, f64, Option<f64>)| {
        let k = lua.create_table()?;
        k.set("Time", time)?;
        k.set("Value", value)?;
        k.set("Envelope", env.unwrap_or(0.0))?;
        Ok(k)
    })?)?;
    globals.set("NumberSequenceKeypoint", nsk)?;

    // ColorSequence.new(c) | new(a, b) | new({ColorSequenceKeypoint...})
    let cs = lua.create_table()?;
    cs.set("new", lua.create_function(|_, args: mlua::Variadic<Value>| {
        let args: Vec<Value> = args.into_iter().collect();
        let col = |v: &Value| match v {
            Value::UserData(ud) => ud.peek::<LuauColor3>().ok().map(|c| c.0),
            _ => None,
        };
        match args.as_slice() {
            [a] if col(a).is_some() => {
                let a = col(a).unwrap_or_default();
                Ok(LuauColorSequence(vec![(0.0, a), (1.0, a)]))
            }
            [a, b] if col(a).is_some() && col(b).is_some() => {
                Ok(LuauColorSequence(vec![(0.0, col(a).unwrap_or_default()), (1.0, col(b).unwrap_or_default())]))
            }
            [Value::Table(t)] => {
                let mut keys = Vec::new();
                for k in t.sequence_values::<mlua::Table>() {
                    let k = k?;
                    let c: Value = k.get("Value")?;
                    keys.push((k.get::<f64>("Time")?, col(&c).unwrap_or_default()));
                }
                Ok(LuauColorSequence(keys))
            }
            _ => Err(mlua::Error::RuntimeError("ColorSequence.new: bad arguments".into())),
        }
    })?)?;
    globals.set("ColorSequence", cs)?;
    let csk = lua.create_table()?;
    csk.set("new", lua.create_function(|lua, (time, color): (f64, crate::luau::types::LuauColor3)| {
        let k = lua.create_table()?;
        k.set("Time", time)?;
        k.set("Value", color)?;
        Ok(k)
    })?)?;
    globals.set("ColorSequenceKeypoint", csk)?;

    // Ray
    let ray = lua.create_table()?;
    ray.set("new", lua.create_function(|_, (o, d): (LuauVector3, LuauVector3)| {
        Ok(LuauRay { origin: o.0, direction: d.0 })
    })?)?;
    globals.set("Ray", ray)?;

    // RaycastParams
    let rp = lua.create_table()?;
    rp.set("new", lua.create_function(|_, ()| Ok(LuauRaycastParams::default()))?)?;
    globals.set("RaycastParams", rp)?;

    // BrickColor
    let bc = lua.create_table()?;
    bc.set("new", lua.create_function(|_, v: Value| match v {
        Value::String(s) => Ok(LuauBrickColor::named(&s.to_str()?)),
        Value::UserData(ud) => Ok(LuauBrickColor::nearest(ud.peek::<LuauColor3>()?.0)),
        _ => Ok(LuauBrickColor::named("Medium stone grey")),
    })?)?;
    bc.set("Random", lua.create_function(|_, ()| {
        let i = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize)
            % BRICK_COLORS.len();
        Ok(LuauBrickColor::named(BRICK_COLORS[i].0))
    })?)?;
    for (name, _, _, _) in BRICK_COLORS.iter().take(8) {
        let key = name.replace(' ', "");
        let n = name.to_string();
        bc.set(key, lua.create_function(move |_, ()| Ok(LuauBrickColor::named(&n)))?)?;
    }
    globals.set("BrickColor", bc)?;
    Ok(())
}
