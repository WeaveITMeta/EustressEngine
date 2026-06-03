//! # SI Unit System
//!
//! Type-safe unit system with conversions for physics simulations.
//!
//! ## Table of Contents
//!
//! 1. **Base Units** - Meter, Kilogram, Second, Kelvin, etc.
//! 2. **Derived Units** - Newton, Pascal, Joule, Watt, etc.
//! 3. **Conversions** - Imperial, CGS, and other unit systems
//! 4. **Dimensional Analysis** - Compile-time unit checking

use std::ops::{Add, Sub, Mul, Div, Neg};
use serde::{Deserialize, Serialize};
use bevy::prelude::*;

// ============================================================================
// Base SI Units (Newtypes for type safety)
// ============================================================================

/// Length in meters (m)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Meters(pub f32);

/// Mass in kilograms (kg)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Kilograms(pub f32);

/// Time in seconds (s)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Seconds(pub f32);

/// Temperature in Kelvin (K)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Kelvin(pub f32);

/// Amount of substance in moles (mol)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Moles(pub f32);

/// Electric current in Amperes (A)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Amperes(pub f32);

/// Luminous intensity in Candela (cd)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Candela(pub f32);

// ============================================================================
// Derived SI Units
// ============================================================================

/// Force in Newtons (N = kg·m/s²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Newtons(pub f32);

/// Pressure in Pascals (Pa = N/m² = kg/(m·s²))
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Pascals(pub f32);

/// Energy in Joules (J = N·m = kg·m²/s²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Joules(pub f32);

/// Power in Watts (W = J/s = kg·m²/s³)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Watts(pub f32);

/// Velocity in meters per second (m/s)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct MetersPerSecond(pub f32);

/// Acceleration in meters per second squared (m/s²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct MetersPerSecondSquared(pub f32);

/// Density in kilograms per cubic meter (kg/m³)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct KilogramsPerCubicMeter(pub f32);

/// Dynamic viscosity in Pascal-seconds (Pa·s)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct PascalSeconds(pub f32);

/// Entropy in Joules per Kelvin (J/K)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct JoulesPerKelvin(pub f32);

/// Specific heat capacity in Joules per kilogram-Kelvin (J/(kg·K))
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct JoulesPerKilogramKelvin(pub f32);

/// Thermal conductivity in Watts per meter-Kelvin (W/(m·K))
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct WattsPerMeterKelvin(pub f32);

/// Volume in cubic meters (m³)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct CubicMeters(pub f32);

/// Area in square meters (m²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct SquareMeters(pub f32);

// ============================================================================
// Operator Implementations
// ============================================================================

macro_rules! impl_unit_ops {
    ($unit:ty) => {
        impl Add for $unit {
            type Output = Self;
            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }
        
        impl Sub for $unit {
            type Output = Self;
            fn sub(self, rhs: Self) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }
        
        impl Mul<f32> for $unit {
            type Output = Self;
            fn mul(self, rhs: f32) -> Self::Output {
                Self(self.0 * rhs)
            }
        }
        
        impl Div<f32> for $unit {
            type Output = Self;
            fn div(self, rhs: f32) -> Self::Output {
                Self(self.0 / rhs)
            }
        }
        
        impl Neg for $unit {
            type Output = Self;
            fn neg(self) -> Self::Output {
                Self(-self.0)
            }
        }
        
        impl From<f32> for $unit {
            fn from(value: f32) -> Self {
                Self(value)
            }
        }
        
        impl From<$unit> for f32 {
            fn from(value: $unit) -> Self {
                value.0
            }
        }
    };
}

impl_unit_ops!(Meters);
impl_unit_ops!(Kilograms);
impl_unit_ops!(Seconds);
impl_unit_ops!(Kelvin);
impl_unit_ops!(Moles);
impl_unit_ops!(Amperes);
impl_unit_ops!(Candela);
impl_unit_ops!(Newtons);
impl_unit_ops!(Pascals);
impl_unit_ops!(Joules);
impl_unit_ops!(Watts);
impl_unit_ops!(MetersPerSecond);
impl_unit_ops!(MetersPerSecondSquared);
impl_unit_ops!(KilogramsPerCubicMeter);
impl_unit_ops!(PascalSeconds);
impl_unit_ops!(JoulesPerKelvin);
impl_unit_ops!(JoulesPerKilogramKelvin);
impl_unit_ops!(WattsPerMeterKelvin);
impl_unit_ops!(CubicMeters);
impl_unit_ops!(SquareMeters);

// ============================================================================
// Unit Conversions
// ============================================================================

impl Meters {
    /// Convert from feet
    pub fn from_feet(feet: f32) -> Self {
        Self(feet * 0.3048)
    }
    
    /// Convert to feet
    pub fn to_feet(self) -> f32 {
        self.0 / 0.3048
    }
    
    /// Convert from inches
    pub fn from_inches(inches: f32) -> Self {
        Self(inches * 0.0254)
    }
    
    /// Convert to inches
    pub fn to_inches(self) -> f32 {
        self.0 / 0.0254
    }
    
    /// Convert from centimeters
    pub fn from_cm(cm: f32) -> Self {
        Self(cm * 0.01)
    }
    
    /// Convert to centimeters
    pub fn to_cm(self) -> f32 {
        self.0 * 100.0
    }
    
    /// Convert from millimeters
    pub fn from_mm(mm: f32) -> Self {
        Self(mm * 0.001)
    }
    
    /// Convert to millimeters
    pub fn to_mm(self) -> f32 {
        self.0 * 1000.0
    }
    
    /// Convert from Eustress studs (1 stud = 0.28 meters, like Roblox)
    pub fn from_studs(studs: f32) -> Self {
        Self(studs * 0.28)
    }
    
    /// Convert to Eustress studs
    pub fn to_studs(self) -> f32 {
        self.0 / 0.28
    }
}

impl Kilograms {
    /// Convert from pounds
    pub fn from_pounds(lbs: f32) -> Self {
        Self(lbs * 0.453592)
    }
    
    /// Convert to pounds
    pub fn to_pounds(self) -> f32 {
        self.0 / 0.453592
    }
    
    /// Convert from grams
    pub fn from_grams(g: f32) -> Self {
        Self(g * 0.001)
    }
    
    /// Convert to grams
    pub fn to_grams(self) -> f32 {
        self.0 * 1000.0
    }
}

impl Kelvin {
    /// Convert from Celsius
    pub fn from_celsius(c: f32) -> Self {
        Self(c + 273.15)
    }
    
    /// Convert to Celsius
    pub fn to_celsius(self) -> f32 {
        self.0 - 273.15
    }
    
    /// Convert from Fahrenheit
    pub fn from_fahrenheit(f: f32) -> Self {
        Self((f - 32.0) * 5.0 / 9.0 + 273.15)
    }
    
    /// Convert to Fahrenheit
    pub fn to_fahrenheit(self) -> f32 {
        (self.0 - 273.15) * 9.0 / 5.0 + 32.0
    }
}

impl Pascals {
    /// Convert from atmospheres
    pub fn from_atm(atm: f32) -> Self {
        Self(atm * 101_325.0)
    }
    
    /// Convert to atmospheres
    pub fn to_atm(self) -> f32 {
        self.0 / 101_325.0
    }
    
    /// Convert from bar
    pub fn from_bar(bar: f32) -> Self {
        Self(bar * 100_000.0)
    }
    
    /// Convert to bar
    pub fn to_bar(self) -> f32 {
        self.0 / 100_000.0
    }
    
    /// Convert from PSI (pounds per square inch)
    pub fn from_psi(psi: f32) -> Self {
        Self(psi * 6894.76)
    }
    
    /// Convert to PSI
    pub fn to_psi(self) -> f32 {
        self.0 / 6894.76
    }
    
    /// Convert from megapascals
    pub fn from_mpa(mpa: f32) -> Self {
        Self(mpa * 1e6)
    }
    
    /// Convert to megapascals
    pub fn to_mpa(self) -> f32 {
        self.0 / 1e6
    }
    
    /// Convert from gigapascals
    pub fn from_gpa(gpa: f32) -> Self {
        Self(gpa * 1e9)
    }
    
    /// Convert to gigapascals
    pub fn to_gpa(self) -> f32 {
        self.0 / 1e9
    }
}

impl Joules {
    /// Convert from calories
    pub fn from_calories(cal: f32) -> Self {
        Self(cal * 4.184)
    }
    
    /// Convert to calories
    pub fn to_calories(self) -> f32 {
        self.0 / 4.184
    }
    
    /// Convert from kilowatt-hours
    pub fn from_kwh(kwh: f32) -> Self {
        Self(kwh * 3.6e6)
    }
    
    /// Convert to kilowatt-hours
    pub fn to_kwh(self) -> f32 {
        self.0 / 3.6e6
    }
    
    /// Convert from electronvolts
    pub fn from_ev(ev: f32) -> Self {
        Self(ev * 1.602_176_634e-19)
    }
    
    /// Convert to electronvolts
    pub fn to_ev(self) -> f32 {
        self.0 / 1.602_176_634e-19
    }
}

impl MetersPerSecond {
    /// Convert from kilometers per hour
    pub fn from_kmh(kmh: f32) -> Self {
        Self(kmh / 3.6)
    }
    
    /// Convert to kilometers per hour
    pub fn to_kmh(self) -> f32 {
        self.0 * 3.6
    }
    
    /// Convert from miles per hour
    pub fn from_mph(mph: f32) -> Self {
        Self(mph * 0.44704)
    }
    
    /// Convert to miles per hour
    pub fn to_mph(self) -> f32 {
        self.0 / 0.44704
    }
    
    /// Convert from knots
    pub fn from_knots(knots: f32) -> Self {
        Self(knots * 0.514444)
    }
    
    /// Convert to knots
    pub fn to_knots(self) -> f32 {
        self.0 / 0.514444
    }
    
    /// Convert from Mach number (at sea level, 20°C)
    pub fn from_mach(mach: f32) -> Self {
        Self(mach * 343.0)
    }
    
    /// Convert to Mach number
    pub fn to_mach(self) -> f32 {
        self.0 / 343.0
    }
}

impl CubicMeters {
    /// Convert from liters
    pub fn from_liters(l: f32) -> Self {
        Self(l * 0.001)
    }
    
    /// Convert to liters
    pub fn to_liters(self) -> f32 {
        self.0 * 1000.0
    }
    
    /// Convert from gallons (US)
    pub fn from_gallons(gal: f32) -> Self {
        Self(gal * 0.00378541)
    }
    
    /// Convert to gallons (US)
    pub fn to_gallons(self) -> f32 {
        self.0 / 0.00378541
    }
}

// ============================================================================
// Dimensional Analysis Helpers
// ============================================================================

/// Calculate force from mass and acceleration: F = ma
pub fn force(mass: Kilograms, acceleration: MetersPerSecondSquared) -> Newtons {
    Newtons(mass.0 * acceleration.0)
}

/// Calculate pressure from force and area: P = F/A
pub fn pressure(force: Newtons, area: SquareMeters) -> Pascals {
    Pascals(force.0 / area.0)
}

/// Calculate work/energy from force and distance: W = F·d
pub fn work(force: Newtons, distance: Meters) -> Joules {
    Joules(force.0 * distance.0)
}

/// Calculate power from energy and time: P = E/t
pub fn power(energy: Joules, time: Seconds) -> Watts {
    Watts(energy.0 / time.0)
}

/// Calculate kinetic energy: KE = ½mv²
pub fn kinetic_energy(mass: Kilograms, velocity: MetersPerSecond) -> Joules {
    Joules(0.5 * mass.0 * velocity.0 * velocity.0)
}

/// Calculate density: ρ = m/V
pub fn density(mass: Kilograms, volume: CubicMeters) -> KilogramsPerCubicMeter {
    KilogramsPerCubicMeter(mass.0 / volume.0)
}

// ============================================================================
// Electromagnetic Units (STEM Stack — Phase B)
// ============================================================================

/// Electric charge in Coulombs (C)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Coulombs(pub f32);

impl Coulombs {
    pub fn from_microcoulombs(uc: f32) -> Self { Self(uc * 1e-6) }
    pub fn to_microcoulombs(self) -> f32 { self.0 * 1e6 }
    pub fn from_nanocoulombs(nc: f32) -> Self { Self(nc * 1e-9) }
    pub fn to_nanocoulombs(self) -> f32 { self.0 * 1e9 }
}

/// Electric potential in Volts (V = J/C)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Volts(pub f32);

impl Volts {
    pub fn from_millivolts(mv: f32) -> Self { Self(mv * 1e-3) }
    pub fn to_millivolts(self) -> f32 { self.0 * 1e3 }
    pub fn from_kilovolts(kv: f32) -> Self { Self(kv * 1e3) }
    pub fn to_kilovolts(self) -> f32 { self.0 * 1e-3 }
}

/// Electric resistance in Ohms (Ω = V/A)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Ohms(pub f32);

impl Ohms {
    pub fn from_kilohms(kΩ: f32) -> Self { Self(kΩ * 1e3) }
    pub fn to_kilohms(self) -> f32 { self.0 * 1e-3 }
    pub fn from_megaohms(mΩ: f32) -> Self { Self(mΩ * 1e6) }
    pub fn to_megaohms(self) -> f32 { self.0 * 1e-6 }
    pub fn from_milliohms(mΩ: f32) -> Self { Self(mΩ * 1e-3) }
    pub fn to_milliohms(self) -> f32 { self.0 * 1e3 }
}

/// Electric conductance in Siemens (S = 1/Ω)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Siemens(pub f32);

/// Electrical capacitance in Farads (F = C/V)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Farads(pub f32);

impl Farads {
    pub fn from_microfarads(uf: f32) -> Self { Self(uf * 1e-6) }
    pub fn to_microfarads(self) -> f32 { self.0 * 1e6 }
    pub fn from_nanofarads(nf: f32) -> Self { Self(nf * 1e-9) }
    pub fn to_nanofarads(self) -> f32 { self.0 * 1e9 }
    pub fn from_picofarads(pf: f32) -> Self { Self(pf * 1e-12) }
    pub fn to_picofarads(self) -> f32 { self.0 * 1e12 }
}

/// Electrical inductance in Henries (H = V·s/A)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Henries(pub f32);

impl Henries {
    pub fn from_millihenries(mh: f32) -> Self { Self(mh * 1e-3) }
    pub fn to_millihenries(self) -> f32 { self.0 * 1e3 }
    pub fn from_microhenries(uh: f32) -> Self { Self(uh * 1e-6) }
    pub fn to_microhenries(self) -> f32 { self.0 * 1e6 }
}

/// Magnetic flux density in Tesla (T = Wb/m² = V·s/m²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Tesla(pub f32);

impl Tesla {
    pub fn from_millitesla(mt: f32) -> Self { Self(mt * 1e-3) }
    pub fn to_millitesla(self) -> f32 { self.0 * 1e3 }
    pub fn from_microtesla(ut: f32) -> Self { Self(ut * 1e-6) }
    pub fn to_microtesla(self) -> f32 { self.0 * 1e6 }
    pub fn from_gauss(g: f32) -> Self { Self(g * 1e-4) }
    pub fn to_gauss(self) -> f32 { self.0 * 1e4 }
}

/// Magnetic flux in Webers (Wb = V·s)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Webers(pub f32);

// ============================================================================
// Radiation / Nuclear Units (STEM Stack — Phase J)
// ============================================================================

/// Absorbed radiation dose in Gray (Gy = J/kg)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Gray(pub f32);

impl Gray {
    pub fn from_milligray(mgy: f32) -> Self { Self(mgy * 1e-3) }
    pub fn to_milligray(self) -> f32 { self.0 * 1e3 }
    pub fn from_rad(rad: f32) -> Self { Self(rad * 0.01) }   // 1 rad = 0.01 Gy
    pub fn to_rad(self) -> f32 { self.0 / 0.01 }
}

/// Dose equivalent in Sievert (Sv = J/kg, weighted for biological effect)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Sievert(pub f32);

impl Sievert {
    pub fn from_millisievert(msv: f32) -> Self { Self(msv * 1e-3) }
    pub fn to_millisievert(self) -> f32 { self.0 * 1e3 }
    pub fn from_microsievert(usv: f32) -> Self { Self(usv * 1e-6) }
    pub fn to_microsievert(self) -> f32 { self.0 * 1e6 }
    pub fn from_rem(rem: f32) -> Self { Self(rem * 0.01) }   // 1 rem = 0.01 Sv
    pub fn to_rem(self) -> f32 { self.0 / 0.01 }
}

/// Radioactivity in Becquerel (Bq = decays/s)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Becquerel(pub f32);

impl Becquerel {
    pub fn from_megabecquerel(mbq: f32) -> Self { Self(mbq * 1e6) }
    pub fn to_megabecquerel(self) -> f32 { self.0 * 1e-6 }
    pub fn from_curie(ci: f32) -> Self { Self(ci * 3.7e10) }  // 1 Ci = 3.7e10 Bq
    pub fn to_curie(self) -> f32 { self.0 / 3.7e10 }
}

// ============================================================================
// Photometric / Optical Units (STEM Stack — Phase H)
// ============================================================================

/// Luminous flux in Lumen (lm = cd·sr)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Lumen(pub f32);

/// Illuminance in Lux (lx = lm/m²)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Reflect, Serialize, Deserialize)]
pub struct Lux(pub f32);

impl Lux {
    pub fn from_foot_candles(fc: f32) -> Self { Self(fc * 10.764) }
    pub fn to_foot_candles(self) -> f32 { self.0 / 10.764 }
}

/// Radiation frequency / wavenumber helper (dimensionless ratio)
/// Wavelength in nanometers for optical work
pub fn wavelength_nm_to_m(nm: f32) -> f32 { nm * 1e-9 }
pub fn wavelength_m_to_nm(m: f32) -> f32 { m * 1e9 }
pub fn freq_to_wavelength_m(freq_hz: f32) -> f32 { 299_792_458.0 / freq_hz }
pub fn wavelength_m_to_freq(wl_m: f32) -> f32 { 299_792_458.0 / wl_m }

// ============================================================================
// Chemical / Molar Units (STEM Stack — Phase C)
// ============================================================================

/// Amount of substance in moles (mol) — already have Moles but extend it
pub fn molarity_to_mass_conc(molarity_mol_l: f32, molar_mass_g_mol: f32) -> f32 {
    molarity_mol_l * molar_mass_g_mol  // g/L
}
pub fn mass_conc_to_molarity(mass_conc_g_l: f32, molar_mass_g_mol: f32) -> f32 {
    mass_conc_g_l / molar_mass_g_mol   // mol/L
}
pub fn ppm_to_molarity(ppm: f32, molar_mass_g_mol: f32) -> f32 {
    ppm / molar_mass_g_mol / 1000.0    // assumes water density = 1 kg/L
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_length_conversions() {
        let m = Meters(1.0);
        assert!((m.to_feet() - 3.28084).abs() < 0.001);
        assert!((Meters::from_feet(3.28084).0 - 1.0).abs() < 0.001);
    }
    
    #[test]
    fn test_temperature_conversions() {
        let k = Kelvin::from_celsius(0.0);
        assert!((k.0 - 273.15).abs() < 0.01);
        
        let k2 = Kelvin::from_fahrenheit(32.0);
        assert!((k2.0 - 273.15).abs() < 0.01);
    }
    
    #[test]
    fn test_pressure_conversions() {
        let p = Pascals::from_atm(1.0);
        assert!((p.0 - 101_325.0).abs() < 1.0);
    }
    
    #[test]
    fn test_dimensional_analysis() {
        let f = force(Kilograms(10.0), MetersPerSecondSquared(9.81));
        assert!((f.0 - 98.1).abs() < 0.01);
        
        let ke = kinetic_energy(Kilograms(1.0), MetersPerSecond(10.0));
        assert!((ke.0 - 50.0).abs() < 0.01);
    }
}
