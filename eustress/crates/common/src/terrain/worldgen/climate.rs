//! Pass 4 — climate: temperature, orographic moisture, Whittaker biome.
//!
//! - **Temperature** (°C): `base_temp_c − lapse_rate·max(h−sea,0) −
//!   latitude_gradient·wz` — colder with altitude and toward +Z ("north").
//!   `wz` is the ABSOLUTE world-Z of each row (supplied by `world_z_of_row`),
//!   so the latitude field is a global function of world coordinates and two
//!   regions sampling the same row agree exactly (seam-safe).
//! - **Moisture** [0,1]: an **orographic sweep** along the prevailing wind:
//!   an air parcel starts at `moisture_capacity`, deposits rain as it is
//!   forced up slopes (deposition ∝ uphill gradient · orographic_efficiency),
//!   recharges over water (sea cells and high-accumulation river cells), and
//!   is depleted in the lee of ridges — a real rain shadow, not noise.
//!   Implemented as deterministic line sweeps in wind order (each wind-aligned
//!   scanline is independent ⇒ parallel-safe and seam-friendly given the
//!   apron overlap the pipeline provides).
//!
//!   ### Sweep geometry
//!   Rays march along the wind's **dominant axis**, one cell per step,
//!   drifting `s = w_secondary / |w_dominant|` cells (|s| ≤ 1) along the
//!   perpendicular axis per step. Rays are indexed by an INTEGER
//!   perpendicular offset `k` and the visited perpendicular cell is
//!   `round(k + s·t)`; for any cell there is exactly one integer `k` whose
//!   ray rounds into it at that step, so every grid cell is visited by
//!   **exactly one ray, exactly once** — no write conflicts, no averaging,
//!   and the fixed `k`-ascending / `t`-ascending order makes the pass
//!   bit-deterministic.
//!
//!   ### Parcel model (per ray)
//!   1. start at `moisture_capacity`;
//!   2. **recharge** to capacity over open water: sea cells
//!      (`h < sea_level`) and river/lake cells
//!      (`accum ≥ river_recharge_accum`);
//!   3. **deposit** `w · min(uphill_gradient · orographic_efficiency, 1)`
//!      whenever the march gains altitude — parcels crest ridges nearly
//!      empty, which is exactly the rain shadow;
//!   4. lose a small background **drizzle** ([`DRIZZLE_PER_M`]) every step so
//!      moisture decays with continentality (distance from recharge);
//!   5. the cell's raw moisture is
//!      `deposit·DEPOSIT_GAIN + w·AMBIENT_HUMIDITY`, floored at
//!      `base_moisture` and clamped to 1. These are FIXED constants — there
//!      is deliberately **no grid-global normalisation** (a per-grid max
//!      would make a cell's value depend on which region ran the pass and
//!      would tear seams).
//! - **Biome** — Whittaker-style classification of (temperature, moisture),
//!   with Ocean/Beach/Glacier special-cased by height and temperature.
//!
//! Determinism contract: fixed sweep order, no RNG.

use super::hydrology::FlowField;

#[derive(Clone, Debug)]
pub struct ClimateParams {
    /// Sea-level air temperature at wz = 0 (°C).
    pub base_temp_c: f32,
    /// °C lost per metre of altitude (standard lapse ≈ 0.0065).
    pub lapse_rate_c_per_m: f32,
    /// °C lost per metre of world +Z (the latitude gradient).
    pub latitude_gradient_c_per_m: f32,
    /// Prevailing wind direction (unit-ish vector, world XZ).
    pub wind_dx: f32,
    pub wind_dz: f32,
    /// Air-parcel moisture capacity (arbitrary units, ≥ 1.0).
    pub moisture_capacity: f32,
    /// How aggressively uphill motion wrings moisture out (0..1).
    pub orographic_efficiency: f32,
    /// Baseline ambient moisture floor [0,1].
    pub base_moisture: f32,
    /// Accumulation threshold above which a cell recharges the air parcel
    /// like open water (rivers/lakes).
    pub river_recharge_accum: f32,
}

impl Default for ClimateParams {
    fn default() -> Self {
        Self {
            base_temp_c: 18.0,
            lapse_rate_c_per_m: 0.0065,
            latitude_gradient_c_per_m: 0.004,
            wind_dx: 1.0,
            wind_dz: 0.25,
            moisture_capacity: 1.0,
            orographic_efficiency: 0.6,
            base_moisture: 0.15,
            river_recharge_accum: 400.0,
        }
    }
}

/// Per-cell climate rasters (row-major, same layout as the heightfield).
#[derive(Clone, Debug)]
pub struct ClimateField {
    pub temperature_c: Vec<f32>,
    /// Normalised moisture [0,1].
    pub moisture: Vec<f32>,
}

/// Whittaker-ish biome classes used by the material pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Ocean,
    Beach,
    Desert,
    Shrubland,
    Grassland,
    Savanna,
    TemperateForest,
    RainForest,
    BorealForest,
    Tundra,
    AlpineRock,
    Glacier,
}

// ---------------------------------------------------------------------------
// Moisture-model tuning constants (fixed by design — see module docs for why
// there is no per-grid normalisation).
// ---------------------------------------------------------------------------

/// Gain applied to per-cell rain deposition when mapping into the normalised
/// moisture raster. Deposits on realistic slopes are a small fraction of the
/// parcel per step; this gain spreads them across the usable [0,1] range so
/// windward slopes actually reach forest/rain-forest moisture.
const DEPOSIT_GAIN: f64 = 2.5;

/// Weight of the parcel's REMAINING humidity in a cell's moisture — humid
/// maritime air keeps flats near water moist even without uphill rain, while
/// depleted lee-side parcels leave the land dry (the rain-shadow signal).
const AMBIENT_HUMIDITY: f64 = 0.35;

/// Background drizzle: fraction of the parcel lost per metre marched
/// (e-fold ≈ 4 km). This is what makes moisture decay with continentality —
/// deep interiors dry out even without a blocking ridge.
const DRIZZLE_PER_M: f64 = 1.0 / 4000.0;

// ---------------------------------------------------------------------------
// Whittaker-table thresholds (°C / normalised moisture / metres).
// ---------------------------------------------------------------------------

/// Vertical band above sea level that classifies as [`Biome::Beach`].
const BEACH_BAND_M: f32 = 2.0;
/// Below this temperature, moist land holds permanent ice ([`Biome::Glacier`]).
const GLACIER_TEMP_C: f32 = -10.0;
/// Minimum moisture for glacier formation (ice needs precipitation).
const GLACIER_MIN_MOISTURE: f32 = 0.30;
/// Cold + dry + high → [`Biome::AlpineRock`] (wind-scoured summits).
const ALPINE_TEMP_C: f32 = -4.0;
const ALPINE_MAX_MOISTURE: f32 = 0.30;
const ALPINE_MIN_ALT_M: f32 = 800.0;
/// Below this mean temperature, low ground reads as [`Biome::Tundra`].
const TUNDRA_TEMP_C: f32 = -5.0;
/// Upper bound of the boreal band (taiga / cold steppe).
const BOREAL_TEMP_C: f32 = 5.0;
/// Lower bound of the hot band (savanna / tropical forest).
const HOT_TEMP_C: f32 = 20.0;

/// Compute temperature + moisture rasters. `world_z_of_row(iz)` supplies the
/// absolute world-Z of each row so latitude is globally consistent across
/// regions (pass `|iz| p.world_z(iz)` from `GenParams`).
#[allow(clippy::too_many_arguments)]
pub fn compute_climate(
    heights: &[f32],
    res_x: u32,
    res_z: u32,
    sea_level: f32,
    cell_size_m: f32,
    flow: &FlowField,
    params: &ClimateParams,
    world_z_of_row: &dyn Fn(u32) -> f64,
) -> ClimateField {
    let n = (res_x as usize) * (res_z as usize);
    debug_assert_eq!(heights.len(), n, "heights must be res_x*res_z");
    debug_assert_eq!(flow.accum.len(), n, "flow raster must match the grid");

    // --- Temperature: lapse with altitude, gradient with latitude ---------
    let mut temperature_c = vec![0.0f32; n];
    for iz in 0..res_z {
        let lat_c = params.latitude_gradient_c_per_m as f64 * world_z_of_row(iz);
        for ix in 0..res_x {
            let c = (iz as usize) * (res_x as usize) + ix as usize;
            // Submerged cells clamp to sea level — air over water sits at
            // the sea-level temperature for its latitude.
            let alt = (heights[c] - sea_level).max(0.0) as f64;
            temperature_c[c] = (params.base_temp_c as f64
                - params.lapse_rate_c_per_m as f64 * alt
                - lat_c) as f32;
        }
    }

    // --- Moisture: deterministic orographic wind sweep --------------------
    let mut moisture = vec![0.0f32; n];
    orographic_sweep(
        heights,
        res_x,
        res_z,
        sea_level,
        cell_size_m,
        flow,
        params,
        &mut moisture,
    );

    ClimateField {
        temperature_c,
        moisture,
    }
}

/// The wind-aligned line sweep described in the module docs. Visits every
/// cell exactly once (integer-offset ray family), in a fixed order, writing
/// the normalised moisture raster in place.
#[allow(clippy::too_many_arguments)]
fn orographic_sweep(
    heights: &[f32],
    res_x: u32,
    res_z: u32,
    sea_level: f32,
    cell_size_m: f32,
    flow: &FlowField,
    params: &ClimateParams,
    moisture: &mut [f32],
) {
    if res_x == 0 || res_z == 0 {
        return;
    }
    let rx = res_x as usize;

    // Wind → dominant-axis march + perpendicular drift.
    let (mut wx, mut wz) = (params.wind_dx as f64, params.wind_dz as f64);
    if wx == 0.0 && wz == 0.0 {
        wx = 1.0; // degenerate wind: sweep +X so the pass stays total
    }
    let x_dominant = wx.abs() >= wz.abs();
    let (p_len, s_len) = if x_dominant {
        (res_x, res_z)
    } else {
        (res_z, res_x)
    };
    let (wp, ws) = if x_dominant { (wx, wz) } else { (wz, wx) };
    let dir_positive = wp >= 0.0;
    // Perpendicular drift per marching step; |s| ≤ 1 because we march the
    // dominant axis.
    let s = ws / wp.abs();
    let step_len_m = (cell_size_m.max(1e-6) as f64) * (1.0 + s * s).sqrt();
    let drizzle_frac = (step_len_m * DRIZZLE_PER_M).min(1.0);

    let capacity = params.moisture_capacity.max(0.0) as f64;
    let efficiency = params.orographic_efficiency.max(0.0) as f64;
    let floor = params.base_moisture;

    // Integer ray-offset family covering every cell (see module docs). The
    // ±1 padding keeps rounding at the extremes inside the family.
    let span = s.abs() * (p_len - 1) as f64;
    let k_min = (-span).floor() as i64 - 1;
    let k_max = (s_len - 1) as i64 + span.ceil() as i64 + 1;

    for k in k_min..=k_max {
        let mut w = capacity;
        let mut prev_h: Option<f32> = None;
        for t in 0..p_len {
            let p = if dir_positive { t } else { p_len - 1 - t };
            let sec = ((k as f64 + s * t as f64) + 0.5).floor() as i64;
            if sec < 0 || sec >= s_len as i64 {
                // Off-grid stretch of the ray (drift is monotonic, so a ray
                // enters and leaves the grid at most once).
                prev_h = None;
                continue;
            }
            let (ix, iz) = if x_dominant {
                (p, sec as u32)
            } else {
                (sec as u32, p)
            };
            let c = (iz as usize) * rx + ix as usize;
            let h = heights[c];

            // 2 — recharge over open water (sea + rivers/lakes).
            if h < sea_level || flow.accum[c] >= params.river_recharge_accum {
                w = capacity;
            }

            // 3 — orographic deposition on uphill motion.
            let mut deposit = 0.0f64;
            if let Some(ph) = prev_h {
                let dh = (h - ph) as f64;
                if dh > 0.0 {
                    let frac = ((dh / step_len_m) * efficiency).min(1.0);
                    deposit = w * frac;
                    w -= deposit;
                }
            }

            // 4 — background drizzle → continentality decay.
            let dz = w * drizzle_frac;
            w -= dz;
            deposit += dz;

            // 5 — fixed-constant mapping into [base_moisture, 1].
            let raw = (deposit * DEPOSIT_GAIN + w * AMBIENT_HUMIDITY) as f32;
            moisture[c] = raw.max(floor).min(1.0);
            prev_h = Some(h);
        }
    }
}

/// Whittaker classification of one cell.
///
/// Priority order (first match wins):
/// 1. `h < sea` → [`Biome::Ocean`];
/// 2. within [`BEACH_BAND_M`] of sea → [`Biome::Beach`];
/// 3. very cold AND moist → [`Biome::Glacier`] (ice needs precipitation);
/// 4. cold AND dry AND high → [`Biome::AlpineRock`] (scoured summits);
/// 5. cold → [`Biome::Tundra`];
/// 6. then the temperature × moisture Whittaker table:
///
/// | temp band            | dry → wet                                        |
/// |----------------------|--------------------------------------------------|
/// | boreal (< 5 °C)      | Desert · Grassland · BorealForest                |
/// | temperate (< 20 °C)  | Desert · Shrubland · Grassland · TemperateForest · RainForest |
/// | hot (≥ 20 °C)        | Desert · Shrubland · Savanna · RainForest        |
pub fn classify_biome(temp_c: f32, moisture: f32, height: f32, sea_level: f32) -> Biome {
    if height < sea_level {
        return Biome::Ocean;
    }
    if height < sea_level + BEACH_BAND_M {
        return Biome::Beach;
    }
    if temp_c < GLACIER_TEMP_C && moisture >= GLACIER_MIN_MOISTURE {
        return Biome::Glacier;
    }
    if temp_c < ALPINE_TEMP_C
        && moisture < ALPINE_MAX_MOISTURE
        && height - sea_level > ALPINE_MIN_ALT_M
    {
        return Biome::AlpineRock;
    }
    if temp_c < TUNDRA_TEMP_C {
        return Biome::Tundra;
    }
    if temp_c < BOREAL_TEMP_C {
        // Boreal band: taiga where moist, cold steppe where drier. Judge
        // fix (tan "beach at altitude" banding): a cold-dry upland is barren
        // GROUND (shrub steppe), never a sand Desert — sand belongs to hot
        // basins and low-energy shorelines only.
        if moisture >= 0.35 {
            Biome::BorealForest
        } else if moisture >= 0.15 {
            Biome::Grassland
        } else {
            Biome::Shrubland
        }
    } else if temp_c < HOT_TEMP_C {
        // Temperate band.
        if moisture >= 0.80 {
            Biome::RainForest // temperate rainforest folds into RainForest
        } else if moisture >= 0.45 {
            Biome::TemperateForest
        } else if moisture >= 0.25 {
            Biome::Grassland
        } else if moisture >= 0.15 {
            Biome::Shrubland
        } else {
            Biome::Desert
        }
    } else {
        // Hot band.
        if moisture >= 0.65 {
            Biome::RainForest
        } else if moisture >= 0.40 {
            Biome::Savanna
        } else if moisture >= 0.20 {
            Biome::Shrubland
        } else {
            Biome::Desert
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::worldgen::hydrology::compute_flow;
    use crate::terrain::worldgen::{generate_base, GenParams};

    const CELL: f32 = 4.0;

    fn flow_for(heights: &[f32], res_x: u32, res_z: u32) -> FlowField {
        compute_flow(heights, res_x, res_z, &[])
    }

    fn base_terrain(seed: u64, res: u32) -> Vec<f32> {
        generate_base(&GenParams {
            seed,
            res_x: res,
            res_z: res,
            size_x: (res - 1) as f64 * CELL as f64,
            size_z: (res - 1) as f64 * CELL as f64,
            ridge_blend: 0.6,
            ..Default::default()
        })
        .heights
    }

    #[test]
    fn climate_is_bit_deterministic_and_in_range() {
        let res = 48u32;
        let heights = base_terrain(5, res);
        let flow = flow_for(&heights, res, res);
        let params = ClimateParams {
            wind_dx: 1.0,
            wind_dz: 0.7, // diagonal drift exercises the ray-family rounding
            ..Default::default()
        };
        let wz = |iz: u32| iz as f64 * CELL as f64;
        let a = compute_climate(&heights, res, res, 0.0, CELL, &flow, &params, &wz);
        let b = compute_climate(&heights, res, res, 0.0, CELL, &flow, &params, &wz);
        for i in 0..heights.len() {
            assert_eq!(
                a.temperature_c[i].to_bits(),
                b.temperature_c[i].to_bits(),
                "temperature[{i}] not bit-identical"
            );
            assert_eq!(
                a.moisture[i].to_bits(),
                b.moisture[i].to_bits(),
                "moisture[{i}] not bit-identical"
            );
            assert!(a.temperature_c[i].is_finite(), "temperature[{i}] not finite");
            // The floor also proves sweep coverage: an unvisited cell would
            // keep its 0.0 init, below base_moisture.
            assert!(
                a.moisture[i] >= params.base_moisture && a.moisture[i] <= 1.0,
                "moisture[{i}] = {} outside [base_moisture, 1]",
                a.moisture[i]
            );
        }
    }

    #[test]
    fn every_wind_direction_covers_every_cell() {
        // Asymmetric grid + all four dominant-axis winds: the ray family must
        // visit every cell exactly once. An unvisited cell would keep its 0.0
        // init — below the base_moisture floor — so the floor doubles as a
        // coverage detector.
        let (rx, rz) = (33u32, 17u32);
        let heights = vec![10.0f32; (rx * rz) as usize];
        let flow = flow_for(&heights, rx, rz);
        for (dx, dz) in [(1.0, 0.3), (-1.0, 0.3), (0.3, 1.0), (0.3, -1.0)] {
            let params = ClimateParams {
                wind_dx: dx,
                wind_dz: dz,
                ..Default::default()
            };
            let c = compute_climate(&heights, rx, rz, 0.0, CELL, &flow, &params, &|_| 0.0);
            for (i, &m) in c.moisture.iter().enumerate() {
                assert!(
                    m >= params.base_moisture,
                    "wind ({dx},{dz}) left cell {i} unvisited: moisture {m}"
                );
            }
        }
    }

    #[test]
    fn temperature_lapses_with_altitude_and_latitude() {
        let (rx, rz) = (8u32, 8u32);
        let mut heights = vec![0.0f32; (rx * rz) as usize];
        for iz in 0..rz {
            heights[(iz * rx + 6) as usize] = 1000.0; // plateau strip
            heights[(iz * rx + 3) as usize] = -50.0; // submerged strip
        }
        let flow = flow_for(&heights, rx, rz);
        let params = ClimateParams::default();
        // Rows are 10 km of world-Z apart so the latitude term is obvious.
        let c = compute_climate(&heights, rx, rz, 0.0, CELL, &flow, &params, &|iz| {
            iz as f64 * 10_000.0
        });

        // Altitude: 1000 m must cool by ~6.5 °C within the same row.
        let low = c.temperature_c[1];
        let high = c.temperature_c[6];
        assert!(
            high < low - 5.0,
            "altitude did not cool: {high} !< {low} - 5"
        );

        // Latitude: same column, far-north row much colder than row 0.
        let south = c.temperature_c[1];
        let north = c.temperature_c[(7 * rx + 1) as usize];
        assert!(
            north < south - 100.0,
            "latitude gradient missing: north {north} !< south {south} - 100"
        );

        // Sub-sea altitude clamps to sea level: submerged strip matches the
        // h = 0 column exactly (same latitude term, zero lapse term).
        assert_eq!(
            c.temperature_c[1].to_bits(),
            c.temperature_c[3].to_bits(),
            "submerged cell must clamp altitude to sea level"
        );
    }

    #[test]
    fn rain_shadow_lee_is_drier_than_windward() {
        // Gentle tent ridge across X, wind due +X: the windward slope wrings
        // the parcel out, the lee side must end up measurably drier.
        let (rx, rz) = (64u32, 16u32);
        let mut heights = vec![0.0f32; (rx * rz) as usize];
        for iz in 0..rz {
            for ix in 0..rx {
                let d = (ix as i32 - 32).unsigned_abs() as f32;
                heights[(iz * rx + ix) as usize] = (32.0 - d) * 0.2;
            }
        }
        let flow = flow_for(&heights, rx, rz);
        let params = ClimateParams {
            wind_dx: 1.0,
            wind_dz: 0.0,
            ..Default::default()
        };
        // Sea level well below the terrain so nothing recharges mid-sweep.
        let c = compute_climate(&heights, rx, rz, -50.0, CELL, &flow, &params, &|_| 0.0);

        let mean = |x0: u32, x1: u32| -> f64 {
            let mut sum = 0.0f64;
            let mut count = 0usize;
            for iz in 0..rz {
                for ix in x0..x1 {
                    sum += c.moisture[(iz * rx + ix) as usize] as f64;
                    count += 1;
                }
            }
            sum / count as f64
        };
        let windward = mean(4, 28);
        let lee = mean(36, 60);
        assert!(
            windward > lee + 0.05 && windward > lee * 1.3,
            "no rain shadow: windward {windward:.3} vs lee {lee:.3}"
        );
    }

    #[test]
    fn whittaker_table_sanity() {
        use Biome::*;
        let sea = 0.0;
        // Special cases by height.
        assert_eq!(classify_biome(15.0, 0.5, -5.0, sea), Ocean);
        assert_eq!(classify_biome(15.0, 0.5, 1.0, sea), Beach);
        // Ice and summits.
        assert_eq!(classify_biome(-15.0, 0.6, 100.0, sea), Glacier);
        assert_eq!(classify_biome(-6.0, 0.2, 1500.0, sea), AlpineRock);
        // Cold + dry at LOW altitude is tundra, not alpine rock.
        assert_eq!(classify_biome(-8.0, 0.1, 100.0, sea), Tundra);
        // Boreal band.
        assert_eq!(classify_biome(0.0, 0.5, 100.0, sea), BorealForest);
        // Temperate band.
        assert_eq!(classify_biome(12.0, 0.55, 100.0, sea), TemperateForest);
        assert_eq!(classify_biome(10.0, 0.3, 100.0, sea), Grassland);
        // Hot band.
        assert_eq!(classify_biome(28.0, 0.9, 100.0, sea), RainForest);
        assert_eq!(classify_biome(25.0, 0.5, 100.0, sea), Savanna);
        assert_eq!(classify_biome(22.0, 0.25, 100.0, sea), Shrubland);
        assert_eq!(classify_biome(30.0, 0.05, 100.0, sea), Desert);
    }
}
