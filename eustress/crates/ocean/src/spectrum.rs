//! The sea's spectrum: how the variance of the surface height is shared out
//! among wave frequencies, directions and wavenumbers.
//!
//! # Frequency: JONSWAP
//!
//! S(w) = alpha g^2 w^-5 exp(-1.25 (wp / w)^4) gamma^r,
//! r = exp(-(w - wp)^2 / (2 sigma^2 wp^2)), sigma = 0.07 for w <= wp, else 0.09,
//!
//! in m^2 s / rad, w the angular frequency (rad/s) and wp its peak
//! (Hasselmann et al. 1973). The w^-5 tail is Phillips' saturation range,
//! the exponential the low-frequency cutoff of Pierson and Moskowitz, and
//! gamma^r the peak enhancement of a growing sea.
//!
//! - Fully developed (no fetch, the default): gamma = 1, alpha = 0.0081 and
//!   wp = 0.877 g / U10 (Pierson and Moskowitz 1964), U10 the wind speed
//!   10 m above the water. The variance is alpha g^2 / (5 wp^4), so the
//!   significant height Hs = 4 sqrt(m0) is 0.2093 U10^2 / g, the familiar
//!   0.21 U^2 / g.
//! - Finite fetch F (m): alpha = 0.076 (U10^2 / (F g))^0.22,
//!   wp = 22 (g^2 / (U10 F))^(1/3), gamma = 3.3 (the JONSWAP means). The
//!   fetch is capped where wp falls to the fully developed peak, at
//!   F = (22 / 0.877)^3 U10^2 / g = 15 786 U10^2 / g (79 km at 7 m/s): a sea
//!   stops growing there, and the formulas are not extrapolated past it.
//! - Swell: a second, narrow component (gamma = 7) peaked at a period of
//!   12 s scaled by sqrt(9.80665 / g), so its wavelength, g T^2 / (2 pi) =
//!   225 m, is the same under any gravity. Its alpha is set numerically so
//!   the swell setting a in [0, 1] gives Hs = 2a metres. With the period
//!   scaled that way, alpha g^2 / wp^4, and with it the variance, does not
//!   depend on g, so the swell keeps its height and length on any world;
//!   only its period and speed change.
//!
//! # Direction: Longuet-Higgins spreading
//!
//! D(dtheta) = N cos^(2s)(dtheta / 2) = N ((1 + cos dtheta) / 2)^s, with
//! dtheta the angle between a wave's direction of travel and the mean
//! direction, and N set numerically (Simpson's rule) so D integrates to 1
//! over [-pi, pi]. The wind sea takes the spread setting (s = 8 by default)
//! and travels only in the wind's half plane, |dtheta| <= pi/2 (at s = 8 the
//! cut removes 0.07% of the energy before normalising; at s = 1, 18%). Swell
//! has a fixed s = 24, the narrow spread of young swell (Goda's s_max = 25),
//! and no cut.
//!
//! # Dispersion
//!
//! Linear waves over a flat bed of depth h: w(k) = sqrt(g k tanh(k h)),
//! group speed dw/dk = g (tanh(kh) + kh sech^2(kh)) / (2 w). Depths of
//! 1000 m or more (or infinite) are deep water, tanh = 1: w = sqrt(g k),
//! dw/dk = w / (2 k). Shallow water shortens and slows waves of a given
//! frequency; the frequency spectrum, and so the height, is left as it is
//! (no TMA depth limiting).
//!
//! # Wavenumber spectrum
//!
//! The FFT lattice samples wavevectors, so the spectrum is carried over with
//! the Jacobian of (w, theta) -> (kx, kz):
//!
//! S(kx, kz) = S(w(k)) D(theta) (dw/dk) / k   (m^4 / rad^2),
//!
//! whose integral over the plane, int int S k dk dtheta, is int S(w) dw: the
//! same variance, shared out over wavevectors.

use std::f64::consts::{PI, TAU};

use crate::ocean::OceanSettings;

/// Standard gravity (m/s^2), the default.
pub const EARTH_GRAVITY: f32 = 9.80665;
/// Depth (m) at and beyond which water is deep: tanh(kh) = 1.
pub const DEEP_WATER_DEPTH: f32 = 1000.0;
/// Phillips constant of the fully developed sea.
pub const PM_ALPHA: f32 = 0.0081;
/// Fully developed peak frequency in units of g / U10.
pub const PM_PEAK: f32 = 0.877;
/// Peak enhancement of a fetch-limited sea.
pub const FETCH_GAMMA: f32 = 3.3;
/// Peak enhancement of the swell.
pub const SWELL_GAMMA: f32 = 7.0;
/// Swell peak period (s) at Earth gravity.
pub const SWELL_PERIOD: f32 = 12.0;
/// Swell significant height (m) at swell = 1.
pub const SWELL_HEIGHT: f32 = 2.0;
/// Spreading exponent s of the swell.
pub const SWELL_SPREAD: f32 = 24.0;
/// Largest spreading exponent accepted.
pub const MAX_SPREAD: f32 = 256.0;

/// Intervals of the numerical integrals (Simpson's rule).
const FREQUENCY_INTERVALS: usize = 8192;
const ANGLE_INTERVALS: usize = 4096;

/// Linear dispersion of surface gravity waves over a flat bed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dispersion {
    /// Gravity (m/s^2).
    pub gravity: f32,
    /// Depth (m); [`DEEP_WATER_DEPTH`] or more, or infinity, is deep water.
    pub depth: f32,
}

impl Dispersion {
    pub fn new(gravity: f32, depth: f32) -> Self {
        Self { gravity, depth }
    }

    /// Deep water.
    pub fn deep(gravity: f32) -> Self {
        Self::new(gravity, f32::INFINITY)
    }

    /// True when tanh(kh) is taken as 1 (a NaN depth counts as deep).
    pub fn is_deep(&self) -> bool {
        !(self.depth < DEEP_WATER_DEPTH)
    }

    /// tanh(kh) and kh sech^2(kh).
    fn depth_terms(&self, k: f64) -> (f64, f64) {
        if self.is_deep() {
            return (1.0, 0.0);
        }
        let kh = k * self.depth as f64;
        let t = kh.tanh();
        (t, kh * (1.0 - t * t))
    }

    /// Angular frequency w(k) = sqrt(g k tanh(kh)) (rad/s) of wavenumber k
    /// (rad/m).
    pub fn omega(&self, k: f32) -> f32 {
        self.omega64(k as f64) as f32
    }

    pub(crate) fn omega64(&self, k: f64) -> f64 {
        if !(k > 0.0) {
            return 0.0;
        }
        let (t, _) = self.depth_terms(k);
        (self.gravity as f64 * k * t).sqrt()
    }

    /// Group speed dw/dk (m/s). At k = 0 it is sqrt(g h), infinite in deep
    /// water.
    pub fn group_speed(&self, k: f32) -> f32 {
        self.group_speed64(k as f64) as f32
    }

    pub(crate) fn group_speed64(&self, k: f64) -> f64 {
        let g = self.gravity as f64;
        if !(k > 0.0) {
            return if self.is_deep() { f64::INFINITY } else { (g * self.depth as f64).sqrt() };
        }
        let (t, s) = self.depth_terms(k);
        g * (t + s) / (2.0 * self.omega64(k))
    }

    /// Phase speed w / k (m/s).
    pub fn phase_speed(&self, k: f32) -> f32 {
        let k = k as f64;
        if !(k > 0.0) {
            return self.group_speed64(0.0) as f32;
        }
        (self.omega64(k) / k) as f32
    }
}

/// One JONSWAP component in angular frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jonswap {
    /// Phillips constant alpha.
    pub alpha: f32,
    /// Peak angular frequency wp (rad/s).
    pub peak_frequency: f32,
    /// Peak enhancement gamma (1 for Pierson-Moskowitz).
    pub gamma: f32,
    /// Gravity (m/s^2).
    pub gravity: f32,
}

impl Jonswap {
    /// The fully developed sea of wind speed `wind_speed` (U10, m/s).
    pub fn pierson_moskowitz(wind_speed: f32, gravity: f32) -> Self {
        Self { alpha: PM_ALPHA, peak_frequency: PM_PEAK * gravity / wind_speed, gamma: 1.0, gravity }
    }

    /// The sea of wind speed `wind_speed` (U10, m/s) over `fetch` metres of
    /// open water, capped at the fully developed fetch.
    pub fn fetch_limited(wind_speed: f32, fetch: f32, gravity: f32) -> Self {
        let (u, g) = (wind_speed as f64, gravity as f64);
        let developed = (22.0 / PM_PEAK as f64).powi(3) * u * u / g;
        let f = (fetch as f64).min(developed);
        let alpha = 0.076 * (u * u / (f * g)).powf(0.22);
        let wp = 22.0 * (g * g / (u * f)).cbrt();
        Self { alpha: alpha as f32, peak_frequency: wp as f32, gamma: FETCH_GAMMA, gravity }
    }

    /// Swell of the given amount in [0, 1]: Hs = 2 m times the amount.
    pub fn swell(amount: f32, gravity: f32) -> Self {
        let period = SWELL_PERIOD as f64 * (EARTH_GRAVITY as f64 / gravity as f64).sqrt();
        let unit = Self { alpha: 1.0, peak_frequency: (TAU / period) as f32, gamma: SWELL_GAMMA, gravity };
        let hs = SWELL_HEIGHT as f64 * amount as f64;
        let alpha = (hs / 4.0).powi(2) / unit.variance();
        Self { alpha: alpha as f32, ..unit }
    }

    /// S(w) (m^2 s / rad).
    pub fn density(&self, w: f32) -> f32 {
        self.density64(w as f64) as f32
    }

    pub(crate) fn density64(&self, w: f64) -> f64 {
        let wp = self.peak_frequency as f64;
        if !(w > 0.0) || !(wp > 0.0) {
            return 0.0;
        }
        let x = wp / w;
        let cutoff = 1.25 * x * x * x * x;
        if cutoff > 700.0 {
            return 0.0;
        }
        let g = self.gravity as f64;
        let mut s = self.alpha as f64 * g * g / (w * w * w * w * w) * (-cutoff).exp();
        if self.gamma != 1.0 {
            let sigma = if w <= wp { 0.07 } else { 0.09 };
            let d = (w - wp) / (sigma * wp);
            s *= (self.gamma as f64).powf((-0.5 * d * d).exp());
        }
        s
    }

    /// Deep-water wavelength at the peak, 2 pi g / wp^2 (m).
    pub fn peak_wavelength(&self) -> f32 {
        let wp = self.peak_frequency as f64;
        (TAU * self.gravity as f64 / (wp * wp)) as f32
    }

    /// int S(w) dw over [a, b] (m^2); b may be infinite.
    ///
    /// Simpson's rule in ln w over the part of [a, b] in [0.1 wp, 60 wp];
    /// below 0.1 wp the cutoff factor is exp(-12 500), and above 60 wp
    /// gamma^r = 1 and the cutoff is within 1e-7 of 1, which leaves the tail
    /// alpha g^2 / 4 (w_a^-4 - w_b^-4) in closed form.
    pub fn integral(&self, a: f64, b: f64) -> f64 {
        let wp = self.peak_frequency as f64;
        if !(wp > 0.0) || !(b > a) {
            return 0.0;
        }
        let top = 60.0 * wp;
        let lo = a.max(0.1 * wp);
        let hi = b.min(top);
        let mut sum = if hi > lo { simpson_log(|w| self.density64(w), lo, hi, FREQUENCY_INTERVALS) } else { 0.0 };
        let tail_lo = a.max(top);
        if b > tail_lo {
            let inv4 = |w: f64| if w.is_finite() { w.powi(-4) } else { 0.0 };
            let g = self.gravity as f64;
            sum += self.alpha as f64 * g * g / 4.0 * (inv4(tail_lo) - inv4(b));
        }
        sum
    }

    /// The variance m0 = int S(w) dw over all w (m^2).
    pub fn variance(&self) -> f64 {
        self.integral(0.0, f64::INFINITY)
    }
}

/// Longuet-Higgins directional spreading about a mean direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spreading {
    /// Unit vector in XZ the waves travel toward on average.
    pub direction: [f32; 2],
    /// Exponent s of cos^(2s)(dtheta / 2); 0 is isotropic.
    pub exponent: f32,
    /// Keep only |dtheta| <= pi/2.
    pub half_plane: bool,
    /// N, so that D integrates to 1.
    norm: f64,
}

impl Spreading {
    /// Spreading about `direction` (normalised; +X when it has no length).
    pub fn new(direction: [f32; 2], exponent: f32, half_plane: bool) -> Self {
        let direction = crate::ocean::unit_xz(direction);
        let exponent = if exponent.is_finite() { exponent.clamp(0.0, MAX_SPREAD) } else { 0.0 };
        let s = exponent as f64;
        let limit = if half_plane { PI / 2.0 } else { PI };
        let integral = simpson(|t| ((1.0 + t.cos()) / 2.0).powf(s), -limit, limit, ANGLE_INTERVALS);
        Self { direction, exponent, half_plane, norm: 1.0 / integral }
    }

    /// D at angle `dtheta` (rad) from the mean direction (1/rad).
    pub fn density_at_angle(&self, dtheta: f32) -> f32 {
        self.density_cos((dtheta as f64).cos()) as f32
    }

    /// D for a wave travelling along the wavevector (kx, kz) (1/rad).
    pub fn density(&self, kx: f32, kz: f32) -> f32 {
        self.density64(kx as f64, kz as f64) as f32
    }

    pub(crate) fn density64(&self, kx: f64, kz: f64) -> f64 {
        let k = (kx * kx + kz * kz).sqrt();
        if !(k > 0.0) {
            return 0.0;
        }
        self.density_cos((kx * self.direction[0] as f64 + kz * self.direction[1] as f64) / k)
    }

    /// D from cos(dtheta), using cos^2(dtheta / 2) = (1 + cos dtheta) / 2.
    fn density_cos(&self, c: f64) -> f64 {
        let c = c.clamp(-1.0, 1.0);
        if self.half_plane && c < 0.0 {
            return 0.0;
        }
        self.norm * ((1.0 + c) * 0.5).powf(self.exponent as f64)
    }
}

/// One component of a sea: its frequency spectrum and its spreading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Component {
    pub frequency: Jonswap,
    pub spreading: Spreading,
}

/// A sea: a wind sea and a swell, either of which may be absent, over one
/// dispersion relation.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    pub dispersion: Dispersion,
    pub wind_sea: Option<Component>,
    pub swell: Option<Component>,
}

impl Spectrum {
    /// The sea that sanitised settings describe (see
    /// [`OceanSettings::sanitised`]). No wind and no swell, or gravity below
    /// [`crate::ocean::MIN_GRAVITY`], is an empty spectrum: a flat sea.
    pub fn from_settings(settings: &OceanSettings) -> Self {
        let g = settings.gravity;
        let waves = g >= crate::ocean::MIN_GRAVITY;
        let wind_sea = (waves && settings.wind_speed > 0.0).then(|| Component {
            frequency: match settings.fetch {
                Some(fetch) => Jonswap::fetch_limited(settings.wind_speed, fetch, g),
                None => Jonswap::pierson_moskowitz(settings.wind_speed, g),
            },
            spreading: Spreading::new(settings.wave_direction, settings.spread, true),
        });
        let swell = (waves && settings.swell > 0.0).then(|| Component {
            frequency: Jonswap::swell(settings.swell, g),
            spreading: Spreading::new(settings.swell_direction, SWELL_SPREAD, false),
        });
        Self { dispersion: Dispersion::new(g, settings.depth), wind_sea, swell }
    }

    /// True when there are no waves at all.
    pub fn is_empty(&self) -> bool {
        self.wind_sea.is_none() && self.swell.is_none()
    }

    fn components(&self) -> impl Iterator<Item = &Component> {
        self.wind_sea.iter().chain(self.swell.iter())
    }

    /// S(w) summed over the components (m^2 s / rad).
    pub fn frequency_density(&self, w: f32) -> f32 {
        self.components().map(|c| c.frequency.density64(w as f64)).sum::<f64>() as f32
    }

    /// S(kx, kz) = S(w(k)) D(theta) (dw/dk) / k (m^4 / rad^2).
    pub fn wavenumber_density(&self, kx: f32, kz: f32) -> f32 {
        self.wavenumber_density64(kx as f64, kz as f64) as f32
    }

    pub(crate) fn wavenumber_density64(&self, kx: f64, kz: f64) -> f64 {
        let k = (kx * kx + kz * kz).sqrt();
        if !(k > 0.0) || self.is_empty() {
            return 0.0;
        }
        let w = self.dispersion.omega64(k);
        let jacobian = self.dispersion.group_speed64(k) / k;
        let s: f64 = self.components().map(|c| c.frequency.density64(w) * c.spreading.density64(kx, kz)).sum();
        s * jacobian
    }

    /// Variance (m^2) of the wavevectors with k_lo <= |k| < k_hi: since D
    /// integrates to 1, the frequency integral between w(k_lo) and w(k_hi).
    pub fn variance_in_band(&self, k_lo: f32, k_hi: f32) -> f64 {
        let w_lo = self.dispersion.omega64(k_lo.max(0.0) as f64);
        let w_hi = if k_hi.is_finite() { self.dispersion.omega64(k_hi as f64) } else { f64::INFINITY };
        self.components().map(|c| c.frequency.integral(w_lo, w_hi)).sum()
    }

    /// Variance of the whole spectrum (m^2).
    pub fn variance(&self) -> f64 {
        self.components().map(|c| c.frequency.variance()).sum()
    }

    /// 4 sqrt(m0) of the whole spectrum (m).
    pub fn significant_wave_height(&self) -> f32 {
        (4.0 * self.variance().sqrt()) as f32
    }

    /// The longest deep-water peak wavelength present, wind sea or swell (m);
    /// 0 for a flat sea.
    pub fn peak_wavelength(&self) -> f32 {
        self.components().map(|c| c.frequency.peak_wavelength()).fold(0.0, f32::max)
    }
}

/// int_a^b f(t) dt by the composite Simpson rule.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, intervals: usize) -> f64 {
    // An even number of intervals, at least 2.
    let n = (intervals.max(2) + 1) & !1;
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for i in 1..n {
        sum += if i % 2 == 1 { 4.0 } else { 2.0 } * f(a + h * i as f64);
    }
    sum * h / 3.0
}

/// int_a^b f(w) dw by the composite Simpson rule in u = ln w (0 < a < b).
fn simpson_log(f: impl Fn(f64) -> f64, a: f64, b: f64, intervals: usize) -> f64 {
    simpson(
        |u| {
            let w = u.exp();
            f(w) * w
        },
        a.ln(),
        b.ln(),
        intervals,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = EARTH_GRAVITY;

    /// Simpson's rule on a smooth integrand is exact to round-off here; the
    /// closed form is alpha g^2 / (5 wp^4).
    #[test]
    fn pierson_moskowitz_variance_is_analytic() {
        for (u, g) in [(5.0f32, G), (10.0, G), (20.0, G), (7.0, 1.62), (7.0, 24.79)] {
            let s = Jonswap::pierson_moskowitz(u, g);
            let wp = s.peak_frequency as f64;
            let exact = s.alpha as f64 * (g as f64).powi(2) / (5.0 * wp.powi(4));
            let got = s.variance();
            assert!((got / exact - 1.0).abs() < 1e-6, "U {u}, g {g}: {got} vs {exact}");
        }
    }

    /// 4 sqrt(alpha g^2 / (5 wp^4)) = 0.2093 U^2 / g, within 1% of the
    /// rounded 0.21.
    #[test]
    fn pierson_moskowitz_height_is_0_21_u2_over_g() {
        for u in [3.0f32, 7.0, 16.0] {
            let hs = 4.0 * Jonswap::pierson_moskowitz(u, G).variance().sqrt();
            let rule = 0.21 * (u * u / G) as f64;
            assert!((hs / rule - 1.0).abs() < 0.01, "U {u}: Hs {hs} vs {rule}");
        }
    }

    #[test]
    fn jonswap_peaks_at_the_peak_frequency() {
        for gamma in [1.0f32, 3.3, 7.0] {
            let s = Jonswap { alpha: 0.01, peak_frequency: 1.3, gamma, gravity: G };
            let peak = s.density(1.3);
            assert!(peak > s.density(1.3 * 0.99) && peak > s.density(1.3 * 1.01), "gamma {gamma}");
        }
        // The enhancement multiplies the peak by gamma.
        let pm = Jonswap { alpha: 0.01, peak_frequency: 1.3, gamma: 1.0, gravity: G };
        let js = Jonswap { gamma: 3.3, ..pm };
        assert!((js.density(1.3) / pm.density(1.3) - 3.3).abs() < 1e-4);
    }

    #[test]
    fn a_longer_fetch_grows_a_longer_higher_sea() {
        let u = 10.0;
        let short = Jonswap::fetch_limited(u, 10_000.0, G);
        let long = Jonswap::fetch_limited(u, 50_000.0, G);
        assert!(long.peak_frequency < short.peak_frequency);
        assert!(long.variance() > short.variance());
        // Past full development (15 786 U^2 / g = 161 km here) the peak is
        // the fully developed one.
        let ocean_wide = Jonswap::fetch_limited(u, 1.0e7, G);
        let pm = Jonswap::pierson_moskowitz(u, G);
        assert!((ocean_wide.peak_frequency / pm.peak_frequency - 1.0).abs() < 1e-5);
    }

    #[test]
    fn swell_height_and_length_follow_the_setting_under_any_gravity() {
        for g in [G, 1.62, 24.79] {
            for amount in [0.25f32, 0.5, 1.0] {
                let s = Jonswap::swell(amount, g);
                let hs = 4.0 * s.variance().sqrt();
                assert!((hs / (2.0 * amount as f64) - 1.0).abs() < 1e-4, "g {g}, swell {amount}: Hs {hs}");
                let length = s.peak_wavelength() as f64;
                let expect = 9.80665 * 144.0 / TAU;
                assert!((length / expect - 1.0).abs() < 1e-5, "g {g}: swell length {length}");
            }
        }
    }

    /// A midpoint rule with 200 000 points, independent of the Simpson rule
    /// that normalised D; its error, at most a step times the half-plane
    /// jump, is below 1e-5.
    #[test]
    fn spreading_integrates_to_one() {
        for s in [0.0f32, 1.0, 8.0, 24.0, 100.0] {
            for half_plane in [false, true] {
                let d = Spreading::new([0.6, -0.8], s, half_plane);
                let n = 200_000;
                let h = TAU / n as f64;
                let total: f64 = (0..n).map(|i| d.density_at_angle((-PI + h * (i as f64 + 0.5)) as f32) as f64 * h).sum();
                assert!((total - 1.0).abs() < 1e-4, "s {s}, half plane {half_plane}: {total}");
            }
        }
    }

    #[test]
    fn wind_sea_stays_in_its_half_plane_and_points_downwind() {
        let d = Spreading::new([0.0, 2.0], 8.0, true);
        assert_eq!(d.direction, [0.0, 1.0]);
        assert!(d.density(0.0, 1.0) > d.density(0.5, 1.0));
        assert!(d.density(1.0, 0.01) > 0.0);
        assert_eq!(d.density(1.0, -0.01), 0.0);
        assert_eq!(d.density(0.0, -1.0), 0.0);
        let swell = Spreading::new([0.0, 1.0], 1.0, false);
        assert!(swell.density(1.0, -0.01) > 0.0);
    }

    #[test]
    fn dispersion_matches_its_limits() {
        let deep = Dispersion::deep(G);
        for k in [0.01f32, 0.3, 5.0] {
            let w = deep.omega(k) as f64;
            assert!((w * w / (G as f64 * k as f64) - 1.0).abs() < 1e-6);
            assert!((deep.group_speed(k) / deep.phase_speed(k) - 0.5).abs() < 1e-6);
        }
        // Finite depth: group speed is dw/dk (central difference).
        let finite = Dispersion::new(G, 6.0);
        for k in [0.05f64, 0.2, 0.8] {
            let dk = 1e-5 * k;
            let fd = (finite.omega64(k + dk) - finite.omega64(k - dk)) / (2.0 * dk);
            assert!((finite.group_speed64(k) / fd - 1.0).abs() < 1e-6, "k {k}");
            assert!(finite.omega64(k) < deep.omega64(k));
        }
        // Shallow limit kh -> 0: phase and group speed tend to sqrt(g h).
        let shallow = Dispersion::new(G, 2.0);
        let c0 = (G as f64 * 2.0).sqrt();
        let k = 0.005 / 2.0;
        assert!((shallow.phase_speed(k as f32) as f64 / c0 - 1.0).abs() < 1e-4);
        assert!((shallow.group_speed(k as f32) as f64 / c0 - 1.0).abs() < 1e-4);
        assert!(Dispersion::new(G, 1000.0).is_deep() && !Dispersion::new(G, 999.0).is_deep());
    }

    /// The wavenumber spectrum integrated over the plane in polar
    /// coordinates (Simpson in ln k and in angle, the angle grid holding the
    /// half-plane edges) gives back the frequency integral over the same
    /// band: the Jacobian (dw/dk) / k and the normalisation of D are right.
    #[test]
    fn wavenumber_spectrum_integrates_to_the_frequency_spectrum() {
        for depth in [f32::INFINITY, 12.0] {
            let settings = OceanSettings { wind_speed: 8.0, swell: 0.6, swell_direction: [0.0, 1.0], depth, ..Default::default() }
                .sanitised();
            let spectrum = Spectrum::from_settings(&settings);
            let kp = (TAU / spectrum.wind_sea.unwrap().frequency.peak_wavelength() as f64) as f32;
            let (k_lo, k_hi) = (0.05 * kp, 40.0 * kp);
            let polar = simpson_log(
                |k| {
                    k * simpson(|t| spectrum.wavenumber_density64(k * t.cos(), k * t.sin()), -PI, PI, 720)
                },
                k_lo as f64,
                k_hi as f64,
                2000,
            );
            let band = spectrum.variance_in_band(k_lo, k_hi);
            assert!((polar / band - 1.0).abs() < 1e-3, "depth {depth}: polar {polar} vs band {band}");
        }
    }
}
