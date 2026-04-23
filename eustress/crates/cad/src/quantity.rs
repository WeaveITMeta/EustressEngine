//! Unit-tagged scalar — [`Quantity`]. Prevents accidental
//! inches-as-meters errors and keeps TOML self-describing.
//!
//! Design: every `Quantity` stores its value in a canonical SI base
//! (meters for lengths, radians for angles, kg for mass, newtons for
//! force) plus the unit it was authored in. Display + TOML round-trip
//! preserve the authored unit; arithmetic operates on SI.

use serde::{Deserialize, Serialize};

/// Authored unit family — length OR angle OR mass OR force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Length(LengthUnit),
    Angle(AngleUnit),
    Mass(MassUnit),
    Force(ForceUnit),
    /// Dimensionless — counts, ratios.
    Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit { Meter, Millimeter, Centimeter, Kilometer, Inch, Foot, Yard, Stud }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AngleUnit { Radian, Degree }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MassUnit { Kilogram, Gram, Pound }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceUnit { Newton, PoundForce }

/// A scalar value + the unit it was authored in.
///
/// TOML representation:
/// ```toml
/// length = { value = 50.0, unit = { length = "millimeter" } }
/// # or shorter:
/// length = "50 mm"
/// ```
///
/// The tagged form is the canonical serialization; the string form
/// is an ergonomic shortcut parsed by [`Quantity::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

impl Quantity {
    pub fn meters(v: f64) -> Self     { Self { value: v, unit: Unit::Length(LengthUnit::Meter) } }
    pub fn millimeters(v: f64) -> Self { Self { value: v, unit: Unit::Length(LengthUnit::Millimeter) } }
    pub fn studs(v: f64) -> Self      { Self { value: v, unit: Unit::Length(LengthUnit::Stud) } }
    pub fn inches(v: f64) -> Self     { Self { value: v, unit: Unit::Length(LengthUnit::Inch) } }
    pub fn feet(v: f64) -> Self       { Self { value: v, unit: Unit::Length(LengthUnit::Foot) } }
    pub fn degrees(v: f64) -> Self    { Self { value: v, unit: Unit::Angle(AngleUnit::Degree) } }
    pub fn radians(v: f64) -> Self    { Self { value: v, unit: Unit::Angle(AngleUnit::Radian) } }
    pub fn scalar(v: f64) -> Self     { Self { value: v, unit: Unit::Scalar } }

    /// Canonical SI value:
    /// - lengths → meters (1 stud = 1 meter in Eustress world coords)
    /// - angles → radians
    /// - mass → kg
    /// - force → newtons
    /// - scalar → as-is
    pub fn to_si(&self) -> f64 {
        match self.unit {
            Unit::Length(u) => self.value * length_to_meters(u),
            Unit::Angle(u)  => self.value * angle_to_radians(u),
            Unit::Mass(u)   => self.value * mass_to_kg(u),
            Unit::Force(u)  => self.value * force_to_newtons(u),
            Unit::Scalar    => self.value,
        }
    }

    /// Parse the ergonomic string form — `"50 mm"`, `"90 deg"`,
    /// `"1.5m"`, `"2 studs"`. Returns `None` if the string doesn't
    /// match. Unit is optional; bare number defaults to `Scalar`.
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() { return None; }

        // Split into numeric prefix + unit suffix (suffix may have a
        // leading space).
        let split_at = trimmed
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit() && *c != '.' && *c != '-' && *c != '+' && *c != 'e' && *c != 'E')
            .map(|(i, _)| i)
            .unwrap_or(trimmed.len());
        let (num, unit) = trimmed.split_at(split_at);
        let value: f64 = num.trim().parse().ok()?;
        let unit_str = unit.trim().to_ascii_lowercase();

        let unit = match unit_str.as_str() {
            ""                           => Unit::Scalar,
            "m" | "meter" | "meters"     => Unit::Length(LengthUnit::Meter),
            "mm" | "millimeter" | "millimeters" => Unit::Length(LengthUnit::Millimeter),
            "cm" | "centimeter" | "centimeters" => Unit::Length(LengthUnit::Centimeter),
            "km" | "kilometer" | "kilometers"   => Unit::Length(LengthUnit::Kilometer),
            "in" | "inch" | "inches"     => Unit::Length(LengthUnit::Inch),
            "ft" | "foot" | "feet"       => Unit::Length(LengthUnit::Foot),
            "yd" | "yard" | "yards"      => Unit::Length(LengthUnit::Yard),
            "stud" | "studs"             => Unit::Length(LengthUnit::Stud),
            "rad" | "radian" | "radians" => Unit::Angle(AngleUnit::Radian),
            "deg" | "°" | "degree" | "degrees" => Unit::Angle(AngleUnit::Degree),
            "kg" | "kilogram" | "kilograms" => Unit::Mass(MassUnit::Kilogram),
            "g" | "gram" | "grams"       => Unit::Mass(MassUnit::Gram),
            "lb" | "pound" | "pounds"    => Unit::Mass(MassUnit::Pound),
            "n" | "newton" | "newtons"   => Unit::Force(ForceUnit::Newton),
            "lbf"                        => Unit::Force(ForceUnit::PoundForce),
            _ => return None,
        };

        Some(Self { value, unit })
    }
}

fn length_to_meters(u: LengthUnit) -> f64 {
    match u {
        LengthUnit::Meter      => 1.0,
        LengthUnit::Millimeter => 0.001,
        LengthUnit::Centimeter => 0.01,
        LengthUnit::Kilometer  => 1000.0,
        LengthUnit::Inch       => 0.0254,
        LengthUnit::Foot       => 0.3048,
        LengthUnit::Yard       => 0.9144,
        LengthUnit::Stud       => 1.0,   // Eustress convention: 1 stud = 1 m
    }
}

fn angle_to_radians(u: AngleUnit) -> f64 {
    match u {
        AngleUnit::Radian => 1.0,
        AngleUnit::Degree => std::f64::consts::PI / 180.0,
    }
}

fn mass_to_kg(u: MassUnit) -> f64 {
    match u {
        MassUnit::Kilogram => 1.0,
        MassUnit::Gram     => 0.001,
        MassUnit::Pound    => 0.45359237,
    }
}

fn force_to_newtons(u: ForceUnit) -> f64 {
    match u {
        ForceUnit::Newton     => 1.0,
        ForceUnit::PoundForce => 4.4482216152605,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_common_forms() {
        assert_eq!(Quantity::parse("50 mm").unwrap().to_si(), 0.05);
        assert_eq!(Quantity::parse("1.5m").unwrap().to_si(), 1.5);
        assert_eq!(Quantity::parse("90 deg").unwrap().to_si(),
                   90.0_f64.to_radians());
        assert_eq!(Quantity::parse("2 studs").unwrap().to_si(), 2.0);
        assert_eq!(Quantity::parse("12 in").unwrap().to_si(), 0.3048);
    }
}
