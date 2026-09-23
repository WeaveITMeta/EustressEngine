//! Property values as scripts see them.
//!
//! One enum for both runtimes: the Luau binding converts it to userdata
//! (`Vector3`, `CFrame`, ...) and the Rune binding to Rune values, so a
//! property written from one language reads back identically in the other.

use crate::scripting::{CFrame, Color3, NumberRange, UDim, UDim2, Vector2, Vector3};

use super::InstanceId;

/// A Roblox `EnumItem`: `Enum.Material.Neon` is `{ enum_type: "Material", name: "Neon" }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumItem {
    pub enum_type: String,
    pub name: String,
}

impl EnumItem {
    pub fn new(enum_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self { enum_type: enum_type.into(), name: name.into() }
    }

    /// Parse `"Enum.Material.Neon"`, `"Material.Neon"` or a bare `"Neon"`
    /// (the latter with `default_type`).
    pub fn parse(text: &str, default_type: &str) -> Self {
        let trimmed = text.trim().trim_start_matches("Enum.");
        match trimmed.split_once('.') {
            Some((ty, name)) => Self::new(ty, name),
            None => Self::new(default_type, trimmed),
        }
    }
}

impl std::fmt::Display for EnumItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Enum.{}.{}", self.enum_type, self.name)
    }
}

/// A property, attribute, or argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum DmValue {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Vector2(Vector2),
    Vector3(Vector3),
    CFrame(CFrame),
    Color3(Color3),
    UDim(UDim),
    UDim2(UDim2),
    NumberRange(NumberRange),
    Enum(EnumItem),
    /// A reference to another instance (`Model.PrimaryPart`, `ObjectValue.Value`).
    Instance(InstanceId),
    /// `NumberSequence` keypoints as `(time, value)`, time in 0..1.
    NumberSequence(Vec<(f64, f64)>),
    /// `ColorSequence` keypoints as `(time, color)`, time in 0..1.
    ColorSequence(Vec<(f64, Color3)>),
}

impl Default for DmValue {
    fn default() -> Self {
        DmValue::Nil
    }
}

impl DmValue {
    pub fn is_nil(&self) -> bool {
        matches!(self, DmValue::Nil)
    }

    /// Roblox `typeof` name.
    pub fn type_name(&self) -> &'static str {
        match self {
            DmValue::Nil => "nil",
            DmValue::Bool(_) => "boolean",
            DmValue::Number(_) => "number",
            DmValue::String(_) => "string",
            DmValue::Vector2(_) => "Vector2",
            DmValue::Vector3(_) => "Vector3",
            DmValue::CFrame(_) => "CFrame",
            DmValue::Color3(_) => "Color3",
            DmValue::UDim(_) => "UDim",
            DmValue::UDim2(_) => "UDim2",
            DmValue::NumberRange(_) => "NumberRange",
            DmValue::Enum(_) => "EnumItem",
            DmValue::Instance(_) => "Instance",
            DmValue::NumberSequence(_) => "NumberSequence",
            DmValue::ColorSequence(_) => "ColorSequence",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            DmValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            DmValue::Number(n) => Some(*n),
            DmValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            DmValue::String(s) => Some(s.as_str()),
            DmValue::Enum(e) => Some(e.name.as_str()),
            _ => None,
        }
    }

    pub fn as_vector3(&self) -> Option<Vector3> {
        match self {
            DmValue::Vector3(v) => Some(*v),
            DmValue::CFrame(cf) => Some(cf.position),
            _ => None,
        }
    }

    pub fn as_cframe(&self) -> Option<CFrame> {
        match self {
            DmValue::CFrame(cf) => Some(*cf),
            DmValue::Vector3(v) => Some(CFrame::from_position(*v)),
            _ => None,
        }
    }

    pub fn as_color3(&self) -> Option<Color3> {
        match self {
            DmValue::Color3(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_udim2(&self) -> Option<UDim2> {
        match self {
            DmValue::UDim2(u) => Some(*u),
            _ => None,
        }
    }

    pub fn as_instance(&self) -> Option<InstanceId> {
        match self {
            DmValue::Instance(id) => Some(*id),
            _ => None,
        }
    }

    /// Enum item name, accepting a plain string too (`part.Material = "Neon"`).
    pub fn as_enum_name(&self) -> Option<&str> {
        match self {
            DmValue::Enum(e) => Some(e.name.as_str()),
            DmValue::String(s) => Some(s.trim_start_matches("Enum.").rsplit('.').next().unwrap_or(s)),
            _ => None,
        }
    }

    /// Short human-readable rendering for logs and `tostring`.
    pub fn display(&self) -> String {
        match self {
            DmValue::Nil => "nil".into(),
            DmValue::Bool(b) => b.to_string(),
            DmValue::Number(n) => format_number(*n),
            DmValue::String(s) => s.clone(),
            DmValue::Vector2(v) => format!("{}, {}", format_number(v.x), format_number(v.y)),
            DmValue::Vector3(v) => format!(
                "{}, {}, {}",
                format_number(v.x),
                format_number(v.y),
                format_number(v.z)
            ),
            DmValue::CFrame(cf) => format!(
                "{}, {}, {}",
                format_number(cf.position.x),
                format_number(cf.position.y),
                format_number(cf.position.z)
            ),
            DmValue::Color3(c) => format!("{}, {}, {}", format_number(c.r), format_number(c.g), format_number(c.b)),
            DmValue::UDim(u) => format!("{}, {}", format_number(u.scale), format_number(u.offset)),
            DmValue::UDim2(u) => format!(
                "{{{}, {}}}, {{{}, {}}}",
                format_number(u.x.scale),
                format_number(u.x.offset),
                format_number(u.y.scale),
                format_number(u.y.offset)
            ),
            DmValue::NumberRange(r) => format!("{} {}", format_number(r.min), format_number(r.max)),
            DmValue::Enum(e) => e.to_string(),
            DmValue::Instance(id) => format!("Instance({})", id.0),
            DmValue::NumberSequence(k) => format!("NumberSequence({} keypoints)", k.len()),
            DmValue::ColorSequence(k) => format!("ColorSequence({} keypoints)", k.len()),
        }
    }
}

/// Numbers print without a trailing `.0` for integers, like Luau's `tostring`.
pub fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

impl From<bool> for DmValue {
    fn from(v: bool) -> Self {
        DmValue::Bool(v)
    }
}
impl From<f64> for DmValue {
    fn from(v: f64) -> Self {
        DmValue::Number(v)
    }
}
impl From<f32> for DmValue {
    fn from(v: f32) -> Self {
        DmValue::Number(v as f64)
    }
}
impl From<i64> for DmValue {
    fn from(v: i64) -> Self {
        DmValue::Number(v as f64)
    }
}
impl From<&str> for DmValue {
    fn from(v: &str) -> Self {
        DmValue::String(v.to_string())
    }
}
impl From<String> for DmValue {
    fn from(v: String) -> Self {
        DmValue::String(v)
    }
}
impl From<Vector3> for DmValue {
    fn from(v: Vector3) -> Self {
        DmValue::Vector3(v)
    }
}
impl From<CFrame> for DmValue {
    fn from(v: CFrame) -> Self {
        DmValue::CFrame(v)
    }
}
impl From<Color3> for DmValue {
    fn from(v: Color3) -> Self {
        DmValue::Color3(v)
    }
}
impl From<UDim2> for DmValue {
    fn from(v: UDim2) -> Self {
        DmValue::UDim2(v)
    }
}
impl From<EnumItem> for DmValue {
    fn from(v: EnumItem) -> Self {
        DmValue::Enum(v)
    }
}
impl From<InstanceId> for DmValue {
    fn from(v: InstanceId) -> Self {
        DmValue::Instance(v)
    }
}
