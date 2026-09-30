//! # Soul Units System
//!
//! Universal unit conversion for Soul scripts.
//! Write "jump 10 feet" or "move at 50 km/h" and Soul converts to engine
//! units, which are meters: the engine is meter-native.
//!
//! ## Conversion Reference
//!
//! | Unit | To Meters |
//! |------|-----------|
//! | 1 unit | 1.0 |
//! | 1 meter | 1.0 |
//! | 1 stud | 0.28 (`crate::units::Unit::Stud`, Roblox's stud) |
//! | 1 foot | 0.3048 |
//! | 1 yard | 0.9144 |
//! | 1 inch | 0.0254 |
//! | 1 kilometer | 1000.0 |
//! | 1 mile | 1609.34 |

use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ============================================================================
// Constants
// ============================================================================

/// 1 foot = 0.3048 meters
pub const FOOT_TO_METERS: f32 = 0.3048;
/// 1 yard = 0.9144 meters
pub const YARD_TO_METERS: f32 = 0.9144;
/// 1 inch = 0.0254 meters
pub const INCH_TO_METERS: f32 = 0.0254;
/// 1 mile = 1609.34 meters
pub const MILE_TO_METERS: f32 = 1609.34;
/// 1 kilometer = 1000 meters
pub const KM_TO_METERS: f32 = 1000.0;

/// Meters per stud: the engine's one stud, [`crate::units::Unit::Stud`].
const METERS_PER_STUD: f32 = crate::units::Unit::Stud.to_meters() as f32;

// ============================================================================
// Unit Types
// ============================================================================

/// Distance unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceUnit {
    /// Engine units: meters, since the engine is meter-native
    Units,
    /// Studs, the engine's one stud ([`crate::units::Unit::Stud`])
    Studs,
    Meters,
    Feet,
    Yards,
    Inches,
    Kilometers,
    Miles,
    Centimeters,
    Millimeters,
}

impl DistanceUnit {
    /// Convert to meters
    pub fn to_meters(&self, value: f32) -> f32 {
        match self {
            DistanceUnit::Units => value,
            DistanceUnit::Studs => value * METERS_PER_STUD,
            DistanceUnit::Meters => value,
            DistanceUnit::Feet => value * FOOT_TO_METERS,
            DistanceUnit::Yards => value * YARD_TO_METERS,
            DistanceUnit::Inches => value * INCH_TO_METERS,
            DistanceUnit::Kilometers => value * KM_TO_METERS,
            DistanceUnit::Miles => value * MILE_TO_METERS,
            DistanceUnit::Centimeters => value * 0.01,
            DistanceUnit::Millimeters => value * 0.001,
        }
    }
    
    /// Convert to engine units (meters)
    pub fn to_units(&self, value: f32) -> f32 {
        self.to_meters(value)
    }

    /// Convert from meters
    pub fn from_meters(&self, meters: f32) -> f32 {
        match self {
            DistanceUnit::Units => meters,
            DistanceUnit::Studs => meters / METERS_PER_STUD,
            DistanceUnit::Meters => meters,
            DistanceUnit::Feet => meters / FOOT_TO_METERS,
            DistanceUnit::Yards => meters / YARD_TO_METERS,
            DistanceUnit::Inches => meters / INCH_TO_METERS,
            DistanceUnit::Kilometers => meters / KM_TO_METERS,
            DistanceUnit::Miles => meters / MILE_TO_METERS,
            DistanceUnit::Centimeters => meters * 100.0,
            DistanceUnit::Millimeters => meters * 1000.0,
        }
    }
}

impl FromStr for DistanceUnit {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unit" | "units" => Ok(DistanceUnit::Units),
            "stud" | "studs" => Ok(DistanceUnit::Studs),
            "m" | "meter" | "meters" | "metre" | "metres" => Ok(DistanceUnit::Meters),
            "ft" | "foot" | "feet" => Ok(DistanceUnit::Feet),
            "yd" | "yard" | "yards" => Ok(DistanceUnit::Yards),
            "in" | "inch" | "inches" => Ok(DistanceUnit::Inches),
            "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => Ok(DistanceUnit::Kilometers),
            "mi" | "mile" | "miles" => Ok(DistanceUnit::Miles),
            "cm" | "centimeter" | "centimeters" => Ok(DistanceUnit::Centimeters),
            "mm" | "millimeter" | "millimeters" => Ok(DistanceUnit::Millimeters),
            _ => Err(format!("Unknown distance unit: {}", s)),
        }
    }
}

/// Speed unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeedUnit {
    /// Engine units per second: meters per second
    UnitsPerSecond,
    /// Studs per second ([`crate::units::Unit::Stud`])
    StudsPerSecond,
    MetersPerSecond,
    FeetPerSecond,
    KilometersPerHour,
    MilesPerHour,
    Knots,
}

impl SpeedUnit {
    /// Convert to meters per second
    pub fn to_mps(&self, value: f32) -> f32 {
        match self {
            SpeedUnit::UnitsPerSecond => value,
            SpeedUnit::StudsPerSecond => value * METERS_PER_STUD,
            SpeedUnit::MetersPerSecond => value,
            SpeedUnit::FeetPerSecond => value * FOOT_TO_METERS,
            SpeedUnit::KilometersPerHour => value / 3.6,
            SpeedUnit::MilesPerHour => value * 0.44704,
            SpeedUnit::Knots => value * 0.51444,
        }
    }

    /// Convert to engine units per second (meters per second)
    pub fn to_units_per_second(&self, value: f32) -> f32 {
        self.to_mps(value)
    }

    /// Convert from meters per second
    pub fn from_mps(&self, mps: f32) -> f32 {
        match self {
            SpeedUnit::UnitsPerSecond => mps,
            SpeedUnit::StudsPerSecond => mps / METERS_PER_STUD,
            SpeedUnit::MetersPerSecond => mps,
            SpeedUnit::FeetPerSecond => mps / FOOT_TO_METERS,
            SpeedUnit::KilometersPerHour => mps * 3.6,
            SpeedUnit::MilesPerHour => mps / 0.44704,
            SpeedUnit::Knots => mps / 0.51444,
        }
    }
}

impl FromStr for SpeedUnit {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase().replace(" ", "").replace("/", "");
        match s.as_str() {
            "unitss" | "unitsec" | "unitspersecond" | "units" => Ok(SpeedUnit::UnitsPerSecond),
            "studss" | "studsec" | "studspersecond" | "studs" => Ok(SpeedUnit::StudsPerSecond),
            "ms" | "mps" | "metersec" | "meterspersecond" => Ok(SpeedUnit::MetersPerSecond),
            "fts" | "fps" | "feetpersecond" | "footpersecond" => Ok(SpeedUnit::FeetPerSecond),
            "kmh" | "kph" | "kilometersperhour" => Ok(SpeedUnit::KilometersPerHour),
            "mph" | "milesperhour" => Ok(SpeedUnit::MilesPerHour),
            "kn" | "kt" | "knots" | "knot" => Ok(SpeedUnit::Knots),
            _ => Err(format!("Unknown speed unit: {}", s)),
        }
    }
}

/// Angle unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AngleUnit {
    Degrees,
    Radians,
    Turns,
    Gradians,
}

impl AngleUnit {
    /// Convert to radians
    pub fn to_radians(&self, value: f32) -> f32 {
        match self {
            AngleUnit::Degrees => value.to_radians(),
            AngleUnit::Radians => value,
            AngleUnit::Turns => value * std::f32::consts::TAU,
            AngleUnit::Gradians => value * std::f32::consts::PI / 200.0,
        }
    }
    
    /// Convert to degrees
    pub fn to_degrees(&self, value: f32) -> f32 {
        match self {
            AngleUnit::Degrees => value,
            AngleUnit::Radians => value.to_degrees(),
            AngleUnit::Turns => value * 360.0,
            AngleUnit::Gradians => value * 0.9,
        }
    }
}

impl FromStr for AngleUnit {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "deg" | "degree" | "degrees" | "°" => Ok(AngleUnit::Degrees),
            "rad" | "radian" | "radians" => Ok(AngleUnit::Radians),
            "turn" | "turns" | "rotation" | "rotations" => Ok(AngleUnit::Turns),
            "grad" | "gradian" | "gradians" | "gon" => Ok(AngleUnit::Gradians),
            _ => Err(format!("Unknown angle unit: {}", s)),
        }
    }
}

/// Time unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeUnit {
    Seconds,
    Milliseconds,
    Minutes,
    Hours,
    Frames, // Assuming 60 FPS
}

impl TimeUnit {
    /// Convert to seconds
    pub fn to_seconds(&self, value: f32) -> f32 {
        match self {
            TimeUnit::Seconds => value,
            TimeUnit::Milliseconds => value / 1000.0,
            TimeUnit::Minutes => value * 60.0,
            TimeUnit::Hours => value * 3600.0,
            TimeUnit::Frames => value / 60.0, // 60 FPS
        }
    }
    
    /// Convert from seconds
    pub fn from_seconds(&self, seconds: f32) -> f32 {
        match self {
            TimeUnit::Seconds => seconds,
            TimeUnit::Milliseconds => seconds * 1000.0,
            TimeUnit::Minutes => seconds / 60.0,
            TimeUnit::Hours => seconds / 3600.0,
            TimeUnit::Frames => seconds * 60.0,
        }
    }
}

impl FromStr for TimeUnit {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "s" | "sec" | "second" | "seconds" => Ok(TimeUnit::Seconds),
            "ms" | "millisecond" | "milliseconds" => Ok(TimeUnit::Milliseconds),
            "min" | "minute" | "minutes" => Ok(TimeUnit::Minutes),
            "h" | "hr" | "hour" | "hours" => Ok(TimeUnit::Hours),
            "f" | "frame" | "frames" => Ok(TimeUnit::Frames),
            _ => Err(format!("Unknown time unit: {}", s)),
        }
    }
}

/// Rotation speed unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationSpeedUnit {
    DegreesPerSecond,
    RadiansPerSecond,
    RPM, // Rotations per minute
    TurnsPerSecond,
}

impl RotationSpeedUnit {
    /// Convert to radians per second
    pub fn to_radians_per_second(&self, value: f32) -> f32 {
        match self {
            RotationSpeedUnit::DegreesPerSecond => value.to_radians(),
            RotationSpeedUnit::RadiansPerSecond => value,
            RotationSpeedUnit::RPM => value * std::f32::consts::TAU / 60.0,
            RotationSpeedUnit::TurnsPerSecond => value * std::f32::consts::TAU,
        }
    }
    
    /// Convert to degrees per second
    pub fn to_degrees_per_second(&self, value: f32) -> f32 {
        self.to_radians_per_second(value).to_degrees()
    }
}

impl FromStr for RotationSpeedUnit {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase().replace(" ", "").replace("/", "");
        match s.as_str() {
            "degs" | "degpersec" | "degreespersecond" => Ok(RotationSpeedUnit::DegreesPerSecond),
            "rads" | "radpersec" | "radianspersecond" => Ok(RotationSpeedUnit::RadiansPerSecond),
            "rpm" | "rotationsperminute" => Ok(RotationSpeedUnit::RPM),
            "tps" | "turnspersecond" => Ok(RotationSpeedUnit::TurnsPerSecond),
            _ => Err(format!("Unknown rotation speed unit: {}", s)),
        }
    }
}

// ============================================================================
// Parsed Value with Unit
// ============================================================================

/// A numeric value with its unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitValue {
    pub value: f32,
    pub unit: String,
}

impl UnitValue {
    pub fn new(value: f32, unit: &str) -> Self {
        Self {
            value,
            unit: unit.to_string(),
        }
    }
    
    /// Try to convert to engine units, meters (for distances)
    pub fn to_units(&self) -> Result<f32, String> {
        let unit: DistanceUnit = self.unit.parse()?;
        Ok(unit.to_units(self.value))
    }

    /// Try to convert to meters
    pub fn to_meters(&self) -> Result<f32, String> {
        let unit: DistanceUnit = self.unit.parse()?;
        Ok(unit.to_meters(self.value))
    }
    
    /// Try to convert to engine units per second, meters per second (for speeds)
    pub fn to_units_per_second(&self) -> Result<f32, String> {
        let unit: SpeedUnit = self.unit.parse()?;
        Ok(unit.to_units_per_second(self.value))
    }

    /// Try to convert to radians (for angles)
    pub fn to_radians(&self) -> Result<f32, String> {
        let unit: AngleUnit = self.unit.parse()?;
        Ok(unit.to_radians(self.value))
    }
    
    /// Try to convert to seconds (for time)
    pub fn to_seconds(&self) -> Result<f32, String> {
        let unit: TimeUnit = self.unit.parse()?;
        Ok(unit.to_seconds(self.value))
    }
}

// ============================================================================
// Unit Parser
// ============================================================================

/// Parse a value with unit from text (e.g., "10 feet", "50 km/h")
pub fn parse_unit_value(text: &str) -> Option<UnitValue> {
    let text = text.trim();
    
    // Find where the number ends and unit begins
    let mut split_idx = 0;
    let mut found_digit = false;
    
    for (i, c) in text.char_indices() {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            found_digit = true;
            split_idx = i + c.len_utf8();
        } else if found_digit && (c.is_alphabetic() || c == '/') {
            break;
        }
    }
    
    if split_idx == 0 {
        return None;
    }
    
    let value_str = text[..split_idx].trim();
    let unit_str = text[split_idx..].trim();
    
    let value: f32 = value_str.parse().ok()?;
    
    if unit_str.is_empty() {
        // A bare number is in engine units (meters)
        Some(UnitValue::new(value, "units"))
    } else {
        Some(UnitValue::new(value, unit_str))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_distance_conversion() {
        // Engine units are meters.
        assert!((DistanceUnit::Feet.to_units(10.0) - 3.048).abs() < 1e-4);
        assert_eq!(DistanceUnit::Meters.to_units(1.0), 1.0);
        assert_eq!(DistanceUnit::Units.to_meters(2.5), 2.5);
    }

    /// Studs are the engine's one stud, Roblox's 0.28 m.
    #[test]
    fn test_studs_are_the_engine_stud() {
        assert!((DistanceUnit::Studs.to_meters(10.0) - 2.8).abs() < 1e-5);
        assert!((DistanceUnit::Studs.from_meters(2.8) - 10.0).abs() < 1e-4);
        assert!((SpeedUnit::StudsPerSecond.to_mps(10.0) - 2.8).abs() < 1e-5);
        assert_eq!("studs".parse::<DistanceUnit>(), Ok(DistanceUnit::Studs));
        assert_eq!("units".parse::<DistanceUnit>(), Ok(DistanceUnit::Units));
    }

    #[test]
    fn test_speed_conversion() {
        let kmh = SpeedUnit::KilometersPerHour;
        let mps = kmh.to_units_per_second(36.0); // 36 km/h = 10 m/s
        assert!((mps - 10.0).abs() < 1e-4);
    }

    #[test]
    fn test_parse_unit_value() {
        let uv = parse_unit_value("10 feet").unwrap();
        assert_eq!(uv.value, 10.0);
        assert_eq!(uv.unit, "feet");

        let uv = parse_unit_value("50 km/h").unwrap();
        assert_eq!(uv.value, 50.0);
        assert_eq!(uv.unit, "km/h");

        // A bare number is in engine units, meters.
        assert_eq!(parse_unit_value("3").unwrap().to_meters(), Ok(3.0));

        let uv = parse_unit_value("10 studs").unwrap();
        assert!((uv.to_meters().unwrap() - 2.8).abs() < 1e-5);
    }
}
