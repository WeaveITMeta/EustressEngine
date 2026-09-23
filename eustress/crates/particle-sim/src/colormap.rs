//! Colour maps for per-particle quantities (linear RGBA out).

/// Which quantity colours the particles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ColorMode {
    /// Each species in its own colour.
    #[default]
    Species,
    Speed,
    /// Kinetic temperature of each particle about its species' drift.
    Temperature,
    /// Sign and size of the charge (diverging).
    Charge,
    /// Fluid density.
    Density,
    /// Fluid pressure.
    Pressure,
    /// Magnitude of the total electric field at the particle.
    Field,
}

impl ColorMode {
    pub const ALL: [ColorMode; 7] = [
        Self::Species,
        Self::Speed,
        Self::Temperature,
        Self::Charge,
        Self::Density,
        Self::Pressure,
        Self::Field,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Species => "Species",
            Self::Speed => "Speed",
            Self::Temperature => "Temperature",
            Self::Charge => "Charge",
            Self::Density => "Density",
            Self::Pressure => "Pressure",
            Self::Field => "Field",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.as_str().eq_ignore_ascii_case(s))
    }
}

#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// Viridis, sampled at 9 stops (sRGB).
const VIRIDIS: [[f32; 3]; 9] = [
    [0.267, 0.005, 0.329],
    [0.283, 0.141, 0.458],
    [0.254, 0.265, 0.530],
    [0.207, 0.372, 0.553],
    [0.164, 0.471, 0.558],
    [0.128, 0.567, 0.551],
    [0.135, 0.659, 0.518],
    [0.478, 0.821, 0.318],
    [0.993, 0.906, 0.144],
];

// Cool-warm diverging (Moreland), sampled at 5 stops (sRGB).
const COOLWARM: [[f32; 3]; 5] = [
    [0.230, 0.299, 0.754],
    [0.552, 0.690, 0.996],
    [0.865, 0.865, 0.865],
    [0.958, 0.604, 0.482],
    [0.706, 0.016, 0.150],
];

fn sample(stops: &[[f32; 3]], t: f32) -> [f32; 4] {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let x = t * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    let f = x - i as f32;
    let a = stops[i];
    let b = stops[i + 1];
    [
        srgb_to_linear(a[0] + (b[0] - a[0]) * f),
        srgb_to_linear(a[1] + (b[1] - a[1]) * f),
        srgb_to_linear(a[2] + (b[2] - a[2]) * f),
        1.0,
    ]
}

/// Sequential map, t in 0..1.
pub fn viridis(t: f32) -> [f32; 4] {
    sample(&VIRIDIS, t)
}

/// Diverging map, t in -1..1 (0 is neutral grey).
pub fn coolwarm(t: f32) -> [f32; 4] {
    sample(&COOLWARM, 0.5 + 0.5 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_clamping() {
        assert_eq!(viridis(-1.0), viridis(0.0));
        assert_eq!(viridis(2.0), viridis(1.0));
        assert_eq!(viridis(f32::NAN), viridis(0.0));
        let mid = coolwarm(0.0);
        assert!((mid[0] - mid[2]).abs() < 1e-3, "neutral is grey");
        for m in ColorMode::ALL {
            assert_eq!(ColorMode::parse(m.as_str()), Some(m));
        }
    }
}
