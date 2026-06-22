//! General SI dimension system — the **D3** foundation of the Data Platform
//! (`docs/architecture/DATA_PLATFORM_PLAN.md` §A.4).
//!
//! A [`Dimension`] is the exponent vector over the seven SI base dimensions, so
//! derived units compose by arithmetic — force = mass·length·time⁻², pressure =
//! force·area⁻¹, voltage = power·current⁻¹ — with **no per-unit newtypes**. It
//! sits beside the engine's meter-native [`crate::units`] module (which handles
//! *length-unit conversion*) and unifies it with cad's typed `Quantity` (which
//! a later increment refactors into a façade over this type).
//!
//! ## Migration safety (plan Risk 4)
//! The on-disk contract is a **string** everywhere — a named symbol where one
//! exists ([`Dimension::symbol`]), else the canonical exponent form
//! ([`Dimension::to_si_string`], e.g. `"si:L1M1T-2"`). Reshaping this type can
//! therefore never corrupt saved data; only the string round-trip is load-
//! bearing. This module is purely additive — it changes no existing behavior.
//!
//! Base order: `[L, M, T, I, K, N, J]` = length, mass, time, electric current,
//! thermodynamic temperature (Θ, written `K` for ASCII), amount of substance,
//! luminous intensity.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Exponent vector over the seven SI base dimensions. `i8` covers any real
/// derived unit (max |exponent| ≈ 4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Dimension(pub [i8; 7]);

/// ASCII letters for each base slot, in `[L, M, T, I, K, N, J]` order.
const BASE_SYMS: [u8; 7] = [b'L', b'M', b'T', b'I', b'K', b'N', b'J'];

impl Dimension {
    // ── Base dimensions ──────────────────────────────────────────────────────
    /// Pure number / ratio (all exponents zero).
    pub const DIMENSIONLESS: Self = Self([0, 0, 0, 0, 0, 0, 0]);
    /// Length (L).
    pub const LENGTH: Self = Self([1, 0, 0, 0, 0, 0, 0]);
    /// Mass (M).
    pub const MASS: Self = Self([0, 1, 0, 0, 0, 0, 0]);
    /// Time (T).
    pub const TIME: Self = Self([0, 0, 1, 0, 0, 0, 0]);
    /// Electric current (I).
    pub const CURRENT: Self = Self([0, 0, 0, 1, 0, 0, 0]);
    /// Thermodynamic temperature (Θ).
    pub const TEMPERATURE: Self = Self([0, 0, 0, 0, 1, 0, 0]);
    /// Amount of substance (N).
    pub const AMOUNT: Self = Self([0, 0, 0, 0, 0, 1, 0]);
    /// Luminous intensity (J).
    pub const LUMINOUS: Self = Self([0, 0, 0, 0, 0, 0, 1]);

    // ── Common derived dimensions (by composition) ───────────────────────────
    /// Area (L²).
    pub const AREA: Self = Self([2, 0, 0, 0, 0, 0, 0]);
    /// Volume (L³).
    pub const VOLUME: Self = Self([3, 0, 0, 0, 0, 0, 0]);
    /// Velocity (L·T⁻¹).
    pub const VELOCITY: Self = Self([1, 0, -1, 0, 0, 0, 0]);
    /// Acceleration (L·T⁻²).
    pub const ACCELERATION: Self = Self([1, 0, -2, 0, 0, 0, 0]);
    /// Force — newton (M·L·T⁻²).
    pub const FORCE: Self = Self([1, 1, -2, 0, 0, 0, 0]);
    /// Pressure / stress — pascal (M·L⁻¹·T⁻²).
    pub const PRESSURE: Self = Self([-1, 1, -2, 0, 0, 0, 0]);
    /// Energy — joule (M·L²·T⁻²).
    pub const ENERGY: Self = Self([2, 1, -2, 0, 0, 0, 0]);
    /// Power — watt (M·L²·T⁻³).
    pub const POWER: Self = Self([2, 1, -3, 0, 0, 0, 0]);
    /// Electric charge — coulomb (T·I).
    pub const CHARGE: Self = Self([0, 0, 1, 1, 0, 0, 0]);
    /// Electric potential — volt (M·L²·T⁻³·I⁻¹).
    pub const VOLTAGE: Self = Self([2, 1, -3, -1, 0, 0, 0]);
    /// Frequency — hertz (T⁻¹).
    pub const FREQUENCY: Self = Self([0, 0, -1, 0, 0, 0, 0]);
    /// Amount concentration — mol·m⁻³ (L⁻³·N).
    pub const CONCENTRATION: Self = Self([-3, 0, 0, 0, 0, 1, 0]);

    /// Multiply (add exponents) — the dimension of a product `a * b`.
    pub const fn mul(self, o: Self) -> Self {
        let a = self.0;
        let b = o.0;
        Self([
            a[0] + b[0],
            a[1] + b[1],
            a[2] + b[2],
            a[3] + b[3],
            a[4] + b[4],
            a[5] + b[5],
            a[6] + b[6],
        ])
    }

    /// Divide (subtract exponents) — the dimension of a quotient `a / b`.
    pub const fn div(self, o: Self) -> Self {
        let a = self.0;
        let b = o.0;
        Self([
            a[0] - b[0],
            a[1] - b[1],
            a[2] - b[2],
            a[3] - b[3],
            a[4] - b[4],
            a[5] - b[5],
            a[6] - b[6],
        ])
    }

    /// Raise to an integer power (multiply every exponent by `n`).
    pub const fn powi(self, n: i8) -> Self {
        let a = self.0;
        Self([
            a[0] * n,
            a[1] * n,
            a[2] * n,
            a[3] * n,
            a[4] * n,
            a[5] * n,
            a[6] * n,
        ])
    }

    /// Reciprocal dimension (`1 / self`).
    pub const fn inv(self) -> Self {
        Self::DIMENSIONLESS.div(self)
    }

    /// True when every exponent is zero (a pure number / ratio).
    pub fn is_dimensionless(&self) -> bool {
        self.0 == [0; 7]
    }

    /// The conventional SI symbol for this dimension when it has a well-known
    /// one (`"m"`, `"kg"`, `"N"`, `"Pa"`, `"V"`, …); `Some("")` for
    /// dimensionless; `None` for an exotic composite (use [`to_si_string`]).
    ///
    /// [`to_si_string`]: Dimension::to_si_string
    pub fn symbol(&self) -> Option<&'static str> {
        if self.is_dimensionless() {
            return Some("");
        }
        NAMED.iter().find(|(d, _)| d == self).map(|(_, s)| *s)
    }

    /// Canonical exponent string, e.g. `"si:L1M1T-2"` for force, `"si:"` for
    /// dimensionless. The unambiguous on-disk fallback when no named symbol
    /// fits. Inverse of [`Dimension::parse_si_string`].
    pub fn to_si_string(&self) -> String {
        let mut s = String::from("si:");
        for (i, &e) in self.0.iter().enumerate() {
            if e != 0 {
                s.push(BASE_SYMS[i] as char);
                // i8 -> decimal; the `-` sign is preserved for negative powers.
                s.push_str(&e.to_string());
            }
        }
        s
    }

    /// Parse the canonical [`Dimension::to_si_string`] form (`"si:L1M1T-2"`).
    /// Returns `None` if the prefix or grammar is wrong.
    pub fn parse_si_string(s: &str) -> Option<Self> {
        let body = s.strip_prefix("si:")?;
        if body.is_empty() {
            return Some(Self::DIMENSIONLESS);
        }
        let bytes = body.as_bytes();
        let mut dim = [0i8; 7];
        let mut i = 0;
        while i < bytes.len() {
            let slot = BASE_SYMS.iter().position(|&b| b == bytes[i])?;
            i += 1;
            let start = i;
            // Optional leading sign. `to_si_string` only ever emits '-', but we
            // accept a '+' too so a hand-written `si:` string still parses.
            if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i == start {
                return None; // a base letter with no exponent
            }
            dim[slot] = body[start..i].parse().ok()?;
        }
        Some(Self(dim))
    }

    /// Best-effort mapping from a unit symbol to its dimension — a convenience
    /// for ingest (plan §3.5.4 touchpoint 3). Recognizes the common SI and
    /// imperial symbols; angles (rad/deg) and pH are dimensionless; falls back
    /// to [`Dimension::parse_si_string`] for the canonical `"si:…"` form.
    ///
    /// NOT authoritative for *magnitude* conversion (that is [`crate::units`]
    /// and the cad `Quantity` parse table) — only for the dimension itself.
    pub fn from_unit_symbol(sym: &str) -> Option<Self> {
        let s = sym.trim().to_ascii_lowercase();
        Some(match s.as_str() {
            "" | "1" | "scalar" | "ratio" | "count" => Self::DIMENSIONLESS,
            // angle is dimensionless
            "rad" | "radian" | "radians" | "deg" | "degree" | "degrees" | "°" => Self::DIMENSIONLESS,
            // pH is dimensionless (−log₁₀ activity) — a named formatter, not a unit
            "ph" => Self::DIMENSIONLESS,
            // length
            "m" | "meter" | "meters" | "metre" | "mm" | "millimeter" | "cm" | "centimeter"
            | "km" | "kilometer" | "in" | "inch" | "inches" | "ft" | "foot" | "feet" | "yd"
            | "yard" | "stud" | "studs" => Self::LENGTH,
            // mass
            "kg" | "kilogram" | "g" | "gram" | "grams" | "mg" | "lb" | "pound" | "pounds" => Self::MASS,
            // time
            "s" | "sec" | "second" | "seconds" | "ms" | "us" | "ns" | "min" | "minute" | "hr"
            | "h" | "hour" => Self::TIME,
            // temperature
            "k" | "kelvin" | "°c" | "celsius" | "degc" | "°f" | "fahrenheit" => Self::TEMPERATURE,
            // current
            "a" | "amp" | "ampere" | "amps" | "ma" => Self::CURRENT,
            // amount
            "mol" | "mole" | "moles" | "mmol" => Self::AMOUNT,
            // luminous intensity
            "cd" | "candela" => Self::LUMINOUS,
            // derived
            "n" | "newton" | "newtons" | "kn" | "lbf" => Self::FORCE,
            "pa" | "pascal" | "kpa" | "mpa" | "gpa" | "psi" | "bar" | "atm" => Self::PRESSURE,
            "j" | "joule" | "joules" | "kj" | "mj" | "wh" | "kwh" | "ev" => Self::ENERGY,
            "w" | "watt" | "watts" | "kw" | "mw" | "gw" => Self::POWER,
            "v" | "volt" | "volts" | "mv" | "kv" => Self::VOLTAGE,
            "hz" | "hertz" | "khz" | "mhz" | "ghz" | "rpm" => Self::FREQUENCY,
            "coulomb" | "coulombs" => Self::CHARGE,
            "m/s" | "mps" => Self::VELOCITY,
            "m/s2" | "m/s^2" => Self::ACCELERATION,
            "m2" | "m^2" => Self::AREA,
            "m3" | "m^3" => Self::VOLUME,
            "mol/m3" | "mol/m^3" => Self::CONCENTRATION,
            _ => return Self::parse_si_string(sym),
        })
    }
}

impl Default for Dimension {
    fn default() -> Self {
        Self::DIMENSIONLESS
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.symbol() {
            Some("") => write!(f, "(dimensionless)"),
            Some(s) => write!(f, "{s}"),
            None => write!(f, "{}", self.to_si_string()),
        }
    }
}

/// Conventional symbols for the well-known dimensions (canonical SI units).
const NAMED: &[(Dimension, &str)] = &[
    (Dimension::LENGTH, "m"),
    (Dimension::MASS, "kg"),
    (Dimension::TIME, "s"),
    (Dimension::CURRENT, "A"),
    (Dimension::TEMPERATURE, "K"),
    (Dimension::AMOUNT, "mol"),
    (Dimension::LUMINOUS, "cd"),
    (Dimension::AREA, "m^2"),
    (Dimension::VOLUME, "m^3"),
    (Dimension::VELOCITY, "m/s"),
    (Dimension::ACCELERATION, "m/s^2"),
    (Dimension::FORCE, "N"),
    (Dimension::PRESSURE, "Pa"),
    (Dimension::ENERGY, "J"),
    (Dimension::POWER, "W"),
    (Dimension::CHARGE, "C"),
    (Dimension::VOLTAGE, "V"),
    (Dimension::FREQUENCY, "Hz"),
    (Dimension::CONCENTRATION, "mol/m^3"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_dimensions_compose_from_bases() {
        assert_eq!(Dimension::FORCE, Dimension::MASS.mul(Dimension::LENGTH).mul(Dimension::TIME.powi(-2)));
        assert_eq!(Dimension::PRESSURE, Dimension::FORCE.div(Dimension::AREA));
        assert_eq!(Dimension::ENERGY, Dimension::FORCE.mul(Dimension::LENGTH));
        assert_eq!(Dimension::POWER, Dimension::ENERGY.div(Dimension::TIME));
        assert_eq!(Dimension::VOLTAGE, Dimension::POWER.div(Dimension::CURRENT));
        assert_eq!(Dimension::VELOCITY, Dimension::LENGTH.div(Dimension::TIME));
        assert_eq!(Dimension::AREA, Dimension::LENGTH.powi(2));
        assert_eq!(Dimension::FREQUENCY, Dimension::TIME.inv());
        assert_eq!(Dimension::CHARGE, Dimension::CURRENT.mul(Dimension::TIME));
    }

    #[test]
    fn dimensionless_is_zero_vector() {
        assert!(Dimension::DIMENSIONLESS.is_dimensionless());
        assert!(Dimension::default().is_dimensionless());
        assert!(!Dimension::FORCE.is_dimensionless());
        // ratio of like dimensions cancels to dimensionless
        assert!(Dimension::LENGTH.div(Dimension::LENGTH).is_dimensionless());
    }

    #[test]
    fn si_string_round_trips() {
        for d in [
            Dimension::DIMENSIONLESS,
            Dimension::LENGTH,
            Dimension::FORCE,
            Dimension::PRESSURE,
            Dimension::VOLTAGE,
            Dimension::CONCENTRATION,
            Dimension([4, -3, 2, -1, 1, 0, 0]),
        ] {
            let s = d.to_si_string();
            assert_eq!(Dimension::parse_si_string(&s), Some(d), "round-trip failed for {s}");
        }
        assert_eq!(Dimension::FORCE.to_si_string(), "si:L1M1T-2");
        assert_eq!(Dimension::DIMENSIONLESS.to_si_string(), "si:");
    }

    #[test]
    fn parse_si_string_rejects_garbage() {
        assert_eq!(Dimension::parse_si_string("L1M1"), None); // missing prefix
        assert_eq!(Dimension::parse_si_string("si:Q3"), None); // unknown base letter
        assert_eq!(Dimension::parse_si_string("si:L"), None); // letter without exponent
    }

    #[test]
    fn named_symbols_resolve_both_ways() {
        assert_eq!(Dimension::FORCE.symbol(), Some("N"));
        assert_eq!(Dimension::PRESSURE.symbol(), Some("Pa"));
        assert_eq!(Dimension::DIMENSIONLESS.symbol(), Some(""));
        assert_eq!(Dimension([4, -3, 2, -1, 1, 0, 0]).symbol(), None);
    }

    #[test]
    fn unit_symbols_map_to_dimensions() {
        assert_eq!(Dimension::from_unit_symbol("psi"), Some(Dimension::PRESSURE));
        assert_eq!(Dimension::from_unit_symbol("MPa"), Some(Dimension::PRESSURE));
        assert_eq!(Dimension::from_unit_symbol("V"), Some(Dimension::VOLTAGE));
        assert_eq!(Dimension::from_unit_symbol("ft"), Some(Dimension::LENGTH));
        assert_eq!(Dimension::from_unit_symbol("deg"), Some(Dimension::DIMENSIONLESS));
        assert_eq!(Dimension::from_unit_symbol("pH"), Some(Dimension::DIMENSIONLESS));
        assert_eq!(Dimension::from_unit_symbol("si:L1M1T-2"), Some(Dimension::FORCE));
        assert_eq!(Dimension::from_unit_symbol("nonsense"), None);
    }
}
