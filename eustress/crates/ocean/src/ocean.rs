//! The ocean: settings, three cascades, and sampling on the CPU.
//!
//! # Tiles
//!
//! The largest tile spans 8 peak wavelengths, 8 lambda_p with
//! lambda_p = 2 pi g / wp^2 the deep-water wavelength at the spectral peak
//! (the longer of the wind sea's and the swell's, so a long swell is
//! resolved as well as the wind sea), clamped to [64 m, 4096 m]. Each next
//! tile is smaller by r = 7 + 1/phi^2 = 7.382, phi the golden ratio. The
//! fractional part of r is the number worst approximated by fractions, so
//! the edges of neighbouring tiles never line up, and come within 5% of the
//! smaller tile's width only once in 13 larger tiles: no repeat shows. With
//! the bands below, Medium covers wavelengths from 8 lambda_p down to
//! lambda_p / 209, which holds all but 3e-5 of a fully developed sea's
//! variance. Above 512 m of peak wavelength (a fully developed sea in wind
//! of about 25 m/s at Earth gravity) the 4096 m limit puts the peak fewer than
//! 8 lattice steps from the origin and the lattice carries less of the
//! spectrum; [`Ocean::significant_wave_height`] reports what it carries.
//!
//! # Bands
//!
//! Each cascade owns a disjoint annulus of |k|, so no wave is counted twice.
//! With dk_c = 2 pi / L_c the lattice step of cascade c and N the
//! resolution, cascade c owns
//!
//! sqrt(N / r) dk_c <= |k| < sqrt(N r) dk_c,
//!
//! the largest tile from |k| > 0. Since dk_{c+1} = r dk_c, each band ends
//! exactly where the next begins, at k_b = sqrt(N dk_c dk_{c+1}), the
//! geometric mean of the two resolution limits, where both neighbours
//! resolve the waves equally well: the coarser lattice samples them
//! sqrt(N / r) times per wavelength, and the band starts sqrt(N / r) lattice
//! steps above the finer lattice's dk. That is 4.2 at Medium (N = 128), 2.9
//! at Low and 5.9 at High; the finest band ends at the same sampling.
//!
//! # Sampling
//!
//! [`Ocean::displacement_at`] sums the cascades' displacements, each
//! bilinear between texels and wrapped with its tile period (coordinates
//! are reduced in f64, so positions far from the origin keep their
//! precision). The water that rests at p is displaced to p + D(p);
//! [`Ocean::height_at`] wants the height of the surface above a fixed world
//! point q, so it inverts the horizontal map by fixed-point iteration,
//! p_{n+1} = q - D_xz(p_n) from p_0 = q, six times, and returns the height
//! at p. Each pass shrinks the error by about the size of grad D, well
//! below 1 unless the surface is close to folding; where it folds (J < 0)
//! there is no single height and the iteration cannot converge.
//! [`Ocean::vertical_velocity_at`] is the change of that height between
//! the last two updates over their time step.
//!
//! # Surface
//!
//! [`Ocean::derivatives_at`] samples the cascades' slopes and folding
//! Jacobians at the same rest position p that [`Ocean::height_at`] inverts
//! to, the point whose water sits above or below the query. The slopes add
//! up; the Jacobians combine to first order, J = 1 + sum_c (J_c - 1), the
//! total a shader summing the cascades' textures also computes (the exact
//! Jacobian of the summed map has cross terms between cascades that no
//! texture carries). [`Ocean::normal_at`] is the unit vector along
//! (-dh/dx, 1, -dh/dz) of those slopes; [`Ocean::foam_at`] ramps foam in
//! below a threshold on J (see [`crate::surface`]). Both are taken with
//! respect to the rest coordinates, as the textures are: under choppy
//! displacement the drawn surface is stretched horizontally, which they
//! leave out, exactly as a shader that reads the same textures does, so the
//! CPU and the GPU agree. [`Ocean::shore_fade_depth`] sizes the fade of the
//! waves toward a shore.
//!
//! # Updates
//!
//! [`Ocean::update`] evaluates every cascade at one time: four inverse FFTs
//! each, rows in parallel. [`Ocean::set_settings`] rebuilds the amplitudes
//! only when a field that shapes the spectrum changed (all but the
//! choppiness); changing nothing does nothing, and a rebuild at the same
//! detail reuses every buffer. After a rebuild the fields read as still
//! water until the next update; after any change the vertical velocity
//! reads 0 until two updates have run.

use crate::cascade::{Cascade, Fields};
use crate::fft::Fft2d;
use crate::spectrum::{Spectrum, EARTH_GRAVITY, MAX_SPREAD};
use crate::surface::{
    foam_coverage, foam_threshold, normal_from_slopes, FOAM_OFF_THRESHOLD, MIN_SHORE_FADE_DEPTH, SHORE_FADE_WAVE_HEIGHTS,
};

/// Number of cascades.
pub const CASCADE_COUNT: usize = 3;
/// Ratio of consecutive tile sizes, 7 + 1/phi^2 = 7 + (3 - sqrt 5) / 2.
pub const TILE_RATIO: f64 = 7.381_966_011_250_105;
/// Largest tile, in peak wavelengths.
pub const TILE_PEAK_WAVELENGTHS: f64 = 8.0;
/// Smallest largest-tile size (m).
pub const MIN_TILE_SIZE: f32 = 64.0;
/// Largest largest-tile size (m).
pub const MAX_TILE_SIZE: f32 = 4096.0;
/// Fixed-point passes of the height inversion.
pub const INVERSION_ITERATIONS: usize = 6;
/// Wind speeds are clamped to [0, this] (m/s).
pub const MAX_WIND_SPEED: f32 = 50.0;
/// Choppiness is clamped to [0, this].
pub const MAX_CHOPPINESS: f32 = 2.0;
/// Below this gravity (m/s^2) nothing restores a disturbed surface: the sea
/// is flat.
pub const MIN_GRAVITY: f32 = 1e-3;
/// Gravity is clamped to [0, this] (m/s^2).
pub const MAX_GRAVITY: f32 = 1000.0;
/// Shallowest depth accepted (m).
pub const MIN_DEPTH: f32 = 0.1;
/// Shortest fetch accepted (m).
pub const MIN_FETCH: f32 = 1.0;
/// Default spreading exponent of the wind sea.
pub const DEFAULT_SPREAD: f32 = 8.0;

/// Lattice resolution of every cascade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Detail {
    /// 64 x 64.
    Low,
    /// 128 x 128.
    #[default]
    Medium,
    /// 256 x 256.
    High,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Low, Detail::Medium, Detail::High];

    /// Lattice side N.
    pub fn resolution(self) -> usize {
        match self {
            Detail::Low => 64,
            Detail::Medium => 128,
            Detail::High => 256,
        }
    }

    /// The name used in property text.
    pub fn name(self) -> &'static str {
        match self {
            Detail::Low => "Low",
            Detail::Medium => "Medium",
            Detail::High => "High",
        }
    }

    /// The detail with this name, ignoring case and surrounding space.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(name))
    }
}

/// Everything that shapes the sea. Out-of-range values are clamped (see
/// [`OceanSettings::sanitised`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OceanSettings {
    /// Wind speed 10 m above the water, U10 (m/s); 0 for no wind sea.
    pub wind_speed: f32,
    /// Direction the wind sea travels toward, a vector in XZ (normalised).
    pub wave_direction: [f32; 2],
    /// Open water upwind (m); None for a fully developed sea.
    pub fetch: Option<f32>,
    /// Swell amount in [0, 1]: significant height 2 m times this.
    pub swell: f32,
    /// Direction the swell travels toward, a vector in XZ (normalised).
    pub swell_direction: [f32; 2],
    /// Spreading exponent s of the wind sea, cos^(2s)(dtheta / 2): larger is
    /// narrower, 0 is every direction of the downwind half plane.
    pub spread: f32,
    /// Horizontal displacement scale in [0, 2]: 1 is physical, 0 none.
    pub choppiness: f32,
    /// Water depth (m); 1000 m or more, or infinity, is deep water.
    pub depth: f32,
    /// Gravity (m/s^2).
    pub gravity: f32,
    /// Lattice resolution.
    pub detail: Detail,
    /// Seed of the random amplitudes.
    pub seed: u64,
}

impl Default for OceanSettings {
    /// A 7 m/s fully developed sea travelling toward +X with 0.3 swell from
    /// the same direction, deep water, Earth gravity, Medium detail.
    fn default() -> Self {
        Self {
            wind_speed: 7.0,
            wave_direction: [1.0, 0.0],
            fetch: None,
            swell: 0.3,
            swell_direction: [1.0, 0.0],
            spread: DEFAULT_SPREAD,
            choppiness: 1.0,
            depth: f32::INFINITY,
            gravity: EARTH_GRAVITY,
            detail: Detail::Medium,
            seed: 0,
        }
    }
}

impl OceanSettings {
    /// These settings with every value in its valid range: wind in
    /// [0, 50] m/s, directions normalised (+X when zero), fetch of at least
    /// 1 m (None when not finite and positive), swell in [0, 1], spread in
    /// [0, 256], choppiness in [0, 2], depth of at least 0.1 m, gravity in
    /// [0, 1000] m/s^2. Non-finite values become wind 0, swell 0, spread 8,
    /// choppiness 1 and gravity 9.80665; a NaN depth is deep water.
    pub fn sanitised(&self) -> Self {
        fn finite_or(v: f32, default: f32) -> f32 {
            if v.is_finite() {
                v
            } else {
                default
            }
        }
        Self {
            wind_speed: finite_or(self.wind_speed, 0.0).clamp(0.0, MAX_WIND_SPEED),
            wave_direction: unit_xz(self.wave_direction),
            fetch: self.fetch.filter(|f| f.is_finite() && *f > 0.0).map(|f| f.max(MIN_FETCH)),
            swell: finite_or(self.swell, 0.0).clamp(0.0, 1.0),
            swell_direction: unit_xz(self.swell_direction),
            spread: finite_or(self.spread, DEFAULT_SPREAD).clamp(0.0, MAX_SPREAD),
            choppiness: finite_or(self.choppiness, 1.0).clamp(0.0, MAX_CHOPPINESS),
            depth: if self.depth.is_nan() { f32::INFINITY } else { self.depth.max(MIN_DEPTH) },
            gravity: finite_or(self.gravity, EARTH_GRAVITY).clamp(0.0, MAX_GRAVITY),
            detail: self.detail,
            seed: self.seed,
        }
    }

    /// The fields that shape the amplitudes: all but the choppiness, which
    /// only scales the horizontal displacement at evaluation.
    fn spectrum_fields(&self) -> Self {
        Self { choppiness: 0.0, ..*self }
    }
}

/// `v` normalised, or +X when it has no direction.
pub(crate) fn unit_xz(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] as f64).hypot(v[1] as f64);
    if len.is_finite() && len > 1e-12 {
        [(v[0] as f64 / len) as f32, (v[1] as f64 / len) as f32]
    } else {
        [1.0, 0.0]
    }
}

/// Tile sizes (m) of the cascades for a peak wavelength (m): 8 of them for
/// the largest, clamped to [64, 4096], each next smaller by [`TILE_RATIO`].
pub fn tile_sizes_for(peak_wavelength: f32) -> [f64; CASCADE_COUNT] {
    let lambda = peak_wavelength as f64;
    let largest = if lambda.is_finite() && lambda > 0.0 {
        (TILE_PEAK_WAVELENGTHS * lambda).clamp(MIN_TILE_SIZE as f64, MAX_TILE_SIZE as f64)
    } else {
        MIN_TILE_SIZE as f64
    };
    let mut tiles = [largest; CASCADE_COUNT];
    for c in 1..CASCADE_COUNT {
        tiles[c] = tiles[c - 1] / TILE_RATIO;
    }
    tiles
}

/// Band edges (rad/m): cascade c owns [edges[c], edges[c + 1]), with
/// edges[0] = 0 and edges[c + 1] = sqrt(N r) dk_c.
pub fn band_edges(resolution: usize, tiles: &[f64; CASCADE_COUNT]) -> [f32; CASCADE_COUNT + 1] {
    let top = (resolution as f64 * TILE_RATIO).sqrt();
    let mut edges = [0.0f32; CASCADE_COUNT + 1];
    for (c, tile) in tiles.iter().enumerate() {
        edges[c + 1] = (top * std::f64::consts::TAU / tile) as f32;
    }
    edges
}

/// A deterministic, spectrally synthesised sea.
#[derive(Clone, Debug)]
pub struct Ocean {
    settings: OceanSettings,
    spectrum: Spectrum,
    fft: Fft2d,
    cascades: Vec<Cascade>,
    time: f64,
    previous_time: f64,
    /// Updates since the fields last changed meaning.
    updates: u32,
    spectrum_builds: u64,
}

impl Ocean {
    /// A sea for `settings`, its fields still water until the first
    /// [`Ocean::update`].
    pub fn new(settings: OceanSettings) -> Self {
        let settings = settings.sanitised();
        let n = settings.detail.resolution();
        let mut ocean = Self {
            settings,
            spectrum: Spectrum::from_settings(&settings),
            fft: Fft2d::new(n),
            cascades: (0..CASCADE_COUNT as u32).map(|c| Cascade::new(c, n)).collect(),
            time: 0.0,
            previous_time: 0.0,
            updates: 0,
            spectrum_builds: 0,
        };
        ocean.build();
        ocean
    }

    fn build(&mut self) {
        let n = self.settings.detail.resolution();
        let tiles = tile_sizes_for(self.spectrum.peak_wavelength());
        let edges = band_edges(n, &tiles);
        for (c, cascade) in self.cascades.iter_mut().enumerate() {
            cascade.configure(tiles[c] as f32, edges[c], edges[c + 1]);
            cascade.build(&self.spectrum, self.settings.seed);
        }
        self.updates = 0;
        self.spectrum_builds += 1;
    }

    /// The settings in use (sanitised).
    pub fn settings(&self) -> &OceanSettings {
        &self.settings
    }

    /// Applies new settings. Rebuilds the amplitudes only when a field other
    /// than the choppiness changed, reallocates only when the detail
    /// changed, and does nothing when nothing changed.
    pub fn set_settings(&mut self, settings: OceanSettings) {
        let new = settings.sanitised();
        if new == self.settings {
            return;
        }
        let old = std::mem::replace(&mut self.settings, new);
        if new.detail != old.detail {
            let n = new.detail.resolution();
            self.fft = Fft2d::new(n);
            self.cascades = (0..CASCADE_COUNT as u32).map(|c| Cascade::new(c, n)).collect();
        }
        if new.spectrum_fields() != old.spectrum_fields() {
            self.spectrum = Spectrum::from_settings(&new);
            self.build();
        } else {
            // Only the choppiness: same amplitudes, new fields from the next
            // update on.
            self.updates = 0;
        }
    }

    /// Evaluates the sea at `time_s` (s). A flat sea only records the time.
    pub fn update(&mut self, time_s: f64) {
        self.previous_time = self.time;
        self.time = time_s;
        self.updates = self.updates.saturating_add(1);
        if self.is_flat() {
            return;
        }
        let choppiness = self.settings.choppiness;
        for cascade in &mut self.cascades {
            cascade.evaluate(time_s, choppiness, &self.fft);
        }
    }

    /// Time of the latest update (s).
    pub fn time(&self) -> f64 {
        self.time
    }

    /// The cascades, largest tile first, with their fields for upload.
    pub fn cascades(&self) -> &[Cascade] {
        &self.cascades
    }

    /// The spectrum the amplitudes were drawn from.
    pub fn spectrum(&self) -> &Spectrum {
        &self.spectrum
    }

    /// True for a sea without waves: no wind and no swell (or no gravity).
    /// Its fields stay exactly zero (J = 1) and updates do no work.
    pub fn is_flat(&self) -> bool {
        self.spectrum.is_empty()
    }

    fn sample(&self, fields: Fields, x: f64, z: f64) -> [f32; 3] {
        let mut sum = [0.0f32; 3];
        for cascade in &self.cascades {
            let d = cascade.sample_fields(fields, x, z);
            sum[0] += d[0];
            sum[1] += d[1];
            sum[2] += d[2];
        }
        sum
    }

    /// The rest position p with p + D_xz(p) = (x, z), by fixed-point
    /// iteration, and the displacement there.
    fn invert(&self, fields: Fields, x: f64, z: f64) -> ([f64; 2], [f32; 3]) {
        let (mut px, mut pz) = (x, z);
        for _ in 0..INVERSION_ITERATIONS {
            let d = self.sample(fields, px, pz);
            px = x - d[0] as f64;
            pz = z - d[2] as f64;
        }
        ([px, pz], self.sample(fields, px, pz))
    }

    /// Displacement (Dx, height, Dz) (m) of the water that rests at world
    /// (x, z): it sits at (x + Dx, height, z + Dz).
    pub fn displacement_at(&self, x: f32, z: f32) -> [f32; 3] {
        self.sample(Fields::Current, x as f64, z as f64)
    }

    /// Height of the water surface above the rest level at world (x, z) (m),
    /// accounting for the horizontal displacement.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        if self.is_flat() {
            return 0.0;
        }
        self.invert(Fields::Current, x as f64, z as f64).1[1]
    }

    /// The point of the displaced surface above or below world (x, z):
    /// [x', height, z'] with (x', z') = (x, z) to within the inversion's
    /// accuracy, and height = [`Ocean::height_at`].
    pub fn surface_point(&self, x: f32, z: f32) -> [f32; 3] {
        let ([px, pz], d) = self.invert(Fields::Current, x as f64, z as f64);
        [(px + d[0] as f64) as f32, d[1], (pz + d[2] as f64) as f32]
    }

    /// Rate of rise of the water surface at world (x, z) (m/s): the change of
    /// [`Ocean::height_at`] between the last two updates over their time
    /// step. 0 before two updates (counted from the last settings change)
    /// and for a flat sea.
    pub fn vertical_velocity_at(&self, x: f32, z: f32) -> f32 {
        let dt = self.time - self.previous_time;
        if self.updates < 2 || dt == 0.0 || !dt.is_finite() || self.is_flat() {
            return 0.0;
        }
        let now = self.invert(Fields::Current, x as f64, z as f64).1[1] as f64;
        let before = self.invert(Fields::Previous, x as f64, z as f64).1[1] as f64;
        ((now - before) / dt) as f32
    }

    /// [`Ocean::height_at`] and [`Ocean::vertical_velocity_at`] together (m,
    /// m/s), sharing the inversion of the current fields: the same values,
    /// bit for bit, at two inversions instead of three. Buoyancy wants both
    /// at every sample.
    pub fn height_and_vertical_velocity_at(&self, x: f32, z: f32) -> (f32, f32) {
        if self.is_flat() {
            return (0.0, 0.0);
        }
        let now = self.invert(Fields::Current, x as f64, z as f64).1[1];
        let dt = self.time - self.previous_time;
        if self.updates < 2 || dt == 0.0 || !dt.is_finite() {
            return (now, 0.0);
        }
        let before = self.invert(Fields::Previous, x as f64, z as f64).1[1] as f64;
        (now, ((now as f64 - before) / dt) as f32)
    }

    /// Slopes and folding Jacobian [dh/dx, dh/dz, J] of the surface above or
    /// below world (x, z): the cascades' derivatives at the rest position
    /// that [`Ocean::height_at`] inverts to, the slopes summed and
    /// J = 1 + sum over the cascades of (J_c - 1). [0, 0, 1] for a flat sea.
    pub fn derivatives_at(&self, x: f32, z: f32) -> [f32; 3] {
        if self.is_flat() {
            return [0.0, 0.0, 1.0];
        }
        let ([px, pz], _) = self.invert(Fields::Current, x as f64, z as f64);
        let mut total = [0.0f32, 0.0, 1.0];
        for cascade in &self.cascades {
            let d = cascade.sample_derivatives64(px, pz);
            total[0] += d[0];
            total[1] += d[1];
            total[2] += d[2] - 1.0;
        }
        total
    }

    /// Unit normal of the surface above or below world (x, z): the unit
    /// vector along (-dh/dx, 1, -dh/dz) of the slopes of
    /// [`Ocean::derivatives_at`]. Straight up for a flat sea.
    pub fn normal_at(&self, x: f32, z: f32) -> [f32; 3] {
        let [slope_x, slope_z, _] = self.derivatives_at(x, z);
        normal_from_slopes(slope_x, slope_z)
    }

    /// Folding Jacobian of the surface above or below world (x, z), to first
    /// order: 1 + sum over the cascades of (J_c - 1), the total that a shader
    /// summing the cascades' derivative textures also computes. Below 1
    /// where the surface is compressed toward a crest, below 0 where it
    /// folds over; 1 for a flat sea.
    pub fn jacobian_at(&self, x: f32, z: f32) -> f32 {
        self.derivatives_at(x, z)[2]
    }

    /// Foam coverage in [0, 1] at world (x, z) for a foam setting in [0, 1]:
    /// [`foam_coverage`] of [`Ocean::jacobian_at`] against the
    /// [`foam_threshold`] of the setting. 0 everywhere at setting 0.
    pub fn foam_at(&self, x: f32, z: f32, foam_setting: f32) -> f32 {
        let threshold = foam_threshold(foam_setting);
        if !(threshold > FOAM_OFF_THRESHOLD) {
            return 0.0;
        }
        foam_coverage(self.jacobian_at(x, z), threshold)
    }

    /// Depth of water (m) over which the waves fade out toward a shore:
    /// 1.5 significant wave heights, and at least 2 m. Drawing and buoyancy
    /// fade by the same [`crate::surface::shore_fade`] of this depth, so
    /// crests never cut through land and a hull on a beach feels only the
    /// waves that are drawn there; see [`crate::surface`] for why.
    pub fn shore_fade_depth(&self) -> f32 {
        (SHORE_FADE_WAVE_HEIGHTS * self.significant_wave_height()).max(MIN_SHORE_FADE_DEPTH)
    }

    /// Expected variance of the surface height (m^2): the spectrum summed
    /// over the modes the cascades carry, not measured from a realisation.
    pub fn expected_variance(&self) -> f64 {
        self.cascades.iter().map(|c| c.expected_variance()).sum()
    }

    /// Significant wave height 4 sqrt(expected variance) (m).
    pub fn significant_wave_height(&self) -> f32 {
        (4.0 * self.expected_variance().sqrt()) as f32
    }

    /// The deep-water peak wavelength the tiles are sized from (m): the
    /// longer of the wind sea's and the swell's 2 pi g / wp^2; 0 when flat.
    pub fn peak_wavelength(&self) -> f32 {
        self.spectrum.peak_wavelength()
    }

    /// Tile sizes of the cascades (m), largest first.
    pub fn tile_sizes(&self) -> [f32; CASCADE_COUNT] {
        let mut sizes = [0.0; CASCADE_COUNT];
        for (s, c) in sizes.iter_mut().zip(&self.cascades) {
            *s = c.tile_size();
        }
        sizes
    }

    /// How many times the amplitudes have been drawn: once at construction
    /// and once per spectrum-changing [`Ocean::set_settings`].
    pub fn spectrum_builds(&self) -> u64 {
        self.spectrum_builds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_span_eight_peak_wavelengths_within_limits() {
        let t = tile_sizes_for(40.0);
        assert!((t[0] - 320.0).abs() < 1e-3);
        assert!((t[0] / t[1] - TILE_RATIO).abs() < 1e-9 && (t[1] / t[2] - TILE_RATIO).abs() < 1e-9);
        assert_eq!(tile_sizes_for(1.0)[0], 64.0);
        assert_eq!(tile_sizes_for(1.0e4)[0], 4096.0);
        assert_eq!(tile_sizes_for(0.0)[0], 64.0);
        assert_eq!(tile_sizes_for(f32::NAN)[0], 64.0);
    }

    /// The ratio is 7 + 1/phi^2 and no small multiple of it is an integer.
    #[test]
    fn tile_ratio_avoids_alignment() {
        let phi = (1.0 + 5.0f64.sqrt()) / 2.0;
        assert!((TILE_RATIO - (7.0 + 1.0 / (phi * phi))).abs() < 1e-12);
        for q in 1..13 {
            let x = q as f64 * TILE_RATIO;
            assert!((x - x.round()).abs() > 0.05, "{q} tiles nearly align");
        }
    }

    #[test]
    fn bands_meet_edge_to_edge_at_the_resolution_rule() {
        for detail in Detail::ALL {
            let n = detail.resolution();
            let tiles = tile_sizes_for(100.0);
            let edges = band_edges(n, &tiles);
            assert_eq!(edges[0], 0.0);
            for c in 0..CASCADE_COUNT {
                let dk = std::f64::consts::TAU / tiles[c];
                // Top of band c: sqrt(N / r) samples per wavelength.
                let samples = n as f64 * dk / edges[c + 1] as f64;
                assert!((samples - (n as f64 / TILE_RATIO).sqrt()).abs() < 1e-4, "{detail:?} cascade {c}: {samples}");
                if c > 0 {
                    // Bottom of band c: sqrt(N / r) lattice steps.
                    let steps = edges[c] as f64 / dk;
                    assert!((steps - (n as f64 / TILE_RATIO).sqrt()).abs() < 1e-4, "{detail:?} cascade {c}: {steps}");
                }
            }
        }
    }

    #[test]
    fn sanitising_clamps_and_normalises() {
        let s = OceanSettings {
            wind_speed: f32::NAN,
            wave_direction: [0.0, 0.0],
            fetch: Some(-5.0),
            swell: 3.0,
            swell_direction: [3.0, 4.0],
            spread: -1.0,
            choppiness: 9.0,
            depth: -2.0,
            gravity: f32::INFINITY,
            detail: Detail::High,
            seed: 5,
        }
        .sanitised();
        assert_eq!(s.wind_speed, 0.0);
        assert_eq!(s.wave_direction, [1.0, 0.0]);
        assert_eq!(s.fetch, None);
        assert_eq!(s.swell, 1.0);
        assert!((s.swell_direction[0] - 0.6).abs() < 1e-7 && (s.swell_direction[1] - 0.8).abs() < 1e-7);
        assert_eq!(s.spread, 0.0);
        assert_eq!(s.choppiness, 2.0);
        assert_eq!(s.depth, MIN_DEPTH);
        assert_eq!(s.gravity, EARTH_GRAVITY);
        assert_eq!(OceanSettings { fetch: Some(0.5), ..Default::default() }.sanitised().fetch, Some(1.0));
        assert_eq!(OceanSettings::default().sanitised(), OceanSettings::default());
    }

    #[test]
    fn detail_names_round_trip() {
        for d in Detail::ALL {
            assert_eq!(Detail::from_name(&d.name().to_lowercase()), Some(d));
        }
        assert_eq!(Detail::from_name("Ultra"), None);
        assert_eq!(Detail::default().resolution(), 128);
    }

    use crate::fft::Complex;
    use crate::spectrum::Dispersion;

    /// Slopes, Jacobians and normals of the lone-wave sea against the
    /// analytic ones. The worst error of bilinear texels 2.5 m apart plus the
    /// six-pass inversion, simulated over 20 000 points of this sea, is
    /// 2.0e-3; foam, a third as steep in J, 5.2e-3.
    const SURFACE_TOLERANCE: f64 = 5e-3;
    const FOAM_TOLERANCE: f64 = 0.02;

    /// A sea of two lone waves on a 320 m tile at choppiness 1: steepness
    /// `steepness[0]` toward +X on the first cascade, `steepness[1]` toward
    /// +Z on the second, the third silent. Returns it with the (amplitude,
    /// wavenumber) of each wave. Each wave moves water only along its own
    /// direction, so the rest position under a world point is found axis by
    /// axis.
    fn two_lone_waves(steepness: [f64; 2]) -> (Ocean, [(f64, f64); 2]) {
        let mut ocean = Ocean::new(OceanSettings::default());
        let deep = Dispersion::deep(EARTH_GRAVITY);
        let modes = [(4i32, 0i32), (0, 3), (1, 0)];
        let mut waves = [(0.0f64, 0.0f64); 2];
        for (c, cascade) in ocean.cascades.iter_mut().enumerate() {
            cascade.configure(320.0, 0.0, f32::INFINITY);
            let (fx, fz) = modes[c];
            let [kx, kz] = cascade.wavevector(fx, fz);
            let k = (kx as f64).hypot(kz as f64);
            let amplitude = if c < 2 { steepness[c] / k } else { 0.0 };
            if c < 2 {
                waves[c] = (amplitude, k);
            }
            cascade.set_single_mode(fx, fz, Complex::new((0.5 * amplitude) as f32, 0.0), &deep);
        }
        ocean.update(0.0);
        (ocean, waves)
    }

    /// The rest coordinate p whose water a lone wave of amplitude a and
    /// wavenumber k moves to q at choppiness 1, p - a sin(k p) = q, by
    /// Newton's method (the map is increasing while a k < 1).
    fn lone_wave_rest(q: f64, a: f64, k: f64) -> f64 {
        let mut p = q;
        for _ in 0..50 {
            p -= (p - a * (k * p).sin() - q) / (1.0 - a * k * (k * p).cos());
        }
        p
    }

    /// Under two lone waves of steepness 0.3 and 0.2 the surface queries
    /// match the analytic ones at the rest position under each point: slopes
    /// -a k sin(k p) per wave, the first-order Jacobian
    /// 1 - a1 k1 cos(k1 p_x) - a2 k2 cos(k2 p_z), its normal and its foam.
    /// Sampling at the query point instead of the rest position would miss by
    /// up to a k^2 times the 3.8 m displacement, 0.09 in slope.
    #[test]
    fn lone_waves_give_the_analytic_slopes_jacobian_normal_and_foam() {
        let (ocean, [(a1, k1), (a2, k2)]) = two_lone_waves([0.3, 0.2]);
        let points = [(0.0f32, 0.0f32), (9.0, 26.7), (20.0, 53.3), (31.0, 80.0), (40.0, -11.0), (-17.5, 13.0), (63.0, 40.0)];
        let (mut steepest, mut least_jacobian, mut foamy) = (0.0f64, f64::MAX, 0);
        for (x, z) in points {
            let (px, pz) = (lone_wave_rest(x as f64, a1, k1), lone_wave_rest(z as f64, a2, k2));
            let slope = [-a1 * k1 * (k1 * px).sin(), -a2 * k2 * (k2 * pz).sin()];
            let jacobian = 1.0 - a1 * k1 * (k1 * px).cos() - a2 * k2 * (k2 * pz).cos();
            let length = (slope[0] * slope[0] + 1.0 + slope[1] * slope[1]).sqrt();
            let normal = [-slope[0] / length, 1.0 / length, -slope[1] / length];
            let foam = ((0.9 - jacobian) / 0.3).clamp(0.0, 1.0);
            let d = ocean.derivatives_at(x, z);
            assert!(
                (d[0] as f64 - slope[0]).abs() < SURFACE_TOLERANCE && (d[1] as f64 - slope[1]).abs() < SURFACE_TOLERANCE,
                "({x}, {z}): slopes {d:?} vs {slope:?}"
            );
            assert!((ocean.jacobian_at(x, z) as f64 - jacobian).abs() < SURFACE_TOLERANCE, "({x}, {z}): J {} vs {jacobian}", d[2]);
            let n = ocean.normal_at(x, z);
            for i in 0..3 {
                assert!((n[i] as f64 - normal[i]).abs() < SURFACE_TOLERANCE, "({x}, {z}): normal {n:?} vs {normal:?}");
            }
            let got = ocean.foam_at(x, z, 1.0);
            assert!((got as f64 - foam).abs() < FOAM_TOLERANCE, "({x}, {z}): foam {got} vs {foam}");
            assert_eq!(ocean.foam_at(x, z, 0.0), 0.0);
            steepest = steepest.max(slope[0].abs().max(slope[1].abs()));
            least_jacobian = least_jacobian.min(jacobian);
            foamy += (foam > 0.0) as u32;
        }
        assert!(steepest > 0.2 && least_jacobian < 0.6 && foamy >= 2, "the points need steep, compressed and foaming water");
    }

    /// The shared query returns what the two separate queries return, bit
    /// for bit: before a second update (no rate yet), after it, and for a
    /// flat sea.
    #[test]
    fn height_and_vertical_velocity_together_match_the_separate_queries() {
        let mut ocean = Ocean::new(OceanSettings { seed: 3, ..Default::default() });
        let points = [(0.0f32, 0.0f32), (31.5, -12.25), (-250.0, 400.0), (1234.5, -987.0)];
        ocean.update(10.0);
        for (x, z) in points {
            let (h, v) = ocean.height_and_vertical_velocity_at(x, z);
            assert_eq!(h.to_bits(), ocean.height_at(x, z).to_bits());
            assert_eq!(v.to_bits(), ocean.vertical_velocity_at(x, z).to_bits());
        }
        ocean.update(10.05);
        let mut moving = false;
        for (x, z) in points {
            let (h, v) = ocean.height_and_vertical_velocity_at(x, z);
            assert_eq!(h.to_bits(), ocean.height_at(x, z).to_bits(), "({x}, {z})");
            assert_eq!(v.to_bits(), ocean.vertical_velocity_at(x, z).to_bits(), "({x}, {z})");
            moving |= v.abs() > 1e-3;
        }
        assert!(moving, "the sea should be moving");
        let flat = Ocean::new(OceanSettings { wind_speed: 0.0, swell: 0.0, ..Default::default() });
        assert_eq!(flat.height_and_vertical_velocity_at(5.0, 6.0), (0.0, 0.0));
    }

    #[test]
    fn a_flat_sea_is_level_and_foamless() {
        let mut flat = Ocean::new(OceanSettings { wind_speed: 0.0, swell: 0.0, ..Default::default() });
        flat.update(3.0);
        assert_eq!(flat.derivatives_at(4.0, -9.0), [0.0, 0.0, 1.0]);
        assert_eq!(flat.normal_at(4.0, -9.0), [0.0, 1.0, 0.0]);
        assert_eq!(flat.jacobian_at(4.0, -9.0), 1.0);
        assert_eq!(flat.foam_at(4.0, -9.0, 1.0), 0.0);
        assert_eq!(flat.shore_fade_depth(), MIN_SHORE_FADE_DEPTH);
    }

    /// 1.5 significant heights, but never under 2 m: the default sea (Hs
    /// about 1.2 m) fades over 2 m, a 24 m/s storm over 1.5 Hs.
    #[test]
    fn shore_fade_depth_is_one_and_a_half_wave_heights_and_at_least_two_metres() {
        let breeze = Ocean::new(OceanSettings::default());
        assert!(1.5 * breeze.significant_wave_height() < 2.0, "Hs {}", breeze.significant_wave_height());
        assert_eq!(breeze.shore_fade_depth(), 2.0);
        let storm = Ocean::new(OceanSettings { wind_speed: 24.0, swell: 0.0, ..Default::default() });
        let hs = storm.significant_wave_height();
        assert!(hs > 5.0, "storm Hs {hs}");
        assert_eq!(storm.shore_fade_depth(), 1.5 * hs);
    }

    /// On a real sea the normal is a unit vector leaning off vertical by the
    /// summed slopes, foam lies in [0, 1], grows with the setting, follows
    /// the Jacobian through [`foam_coverage`] and is off at setting 0; at
    /// setting 1 a 7 m/s sea foams somewhere (its first-order J has a
    /// standard deviation near 0.15, so about a quarter of it is below 0.9).
    #[test]
    fn normals_are_unit_and_foam_follows_the_jacobian_on_a_real_sea() {
        let mut ocean = Ocean::new(OceanSettings { seed: 8, ..Default::default() });
        ocean.update(17.0);
        let mut foamy = 0;
        for i in 1..=400u32 {
            // A golden-ratio sequence: well spread points.
            let x = -300.0 + 600.0 * (i as f64 * 0.618_033_988_75).fract() as f32;
            let z = -300.0 + 600.0 * (i as f64 * 0.754_877_666_25).fract() as f32;
            let n = ocean.normal_at(x, z);
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((length - 1.0).abs() < 1e-5 && n[1] > 0.0, "({x}, {z}): {n:?}");
            let [slope_x, slope_z, jacobian] = ocean.derivatives_at(x, z);
            assert_eq!(n, normal_from_slopes(slope_x, slope_z));
            assert_eq!(ocean.jacobian_at(x, z), jacobian);
            let (full, half) = (ocean.foam_at(x, z, 1.0), ocean.foam_at(x, z, 0.5));
            assert!((0.0..=1.0).contains(&full) && (0.0..=full).contains(&half), "({x}, {z}): {full} {half}");
            assert_eq!(full, foam_coverage(jacobian, foam_threshold(1.0)));
            assert_eq!(ocean.foam_at(x, z, 0.0), 0.0);
            foamy += (full > 0.0) as u32;
        }
        assert!(foamy > 0, "no foam anywhere at setting 1");
    }
}
