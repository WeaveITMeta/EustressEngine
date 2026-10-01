//! # Dynamic Unit System
//!
//! The canonical unit/conversion layer used across the engine, the
//! Properties panel, the TOML round-trip, and the script APIs.
//!
//! ## Architecture
//!
//! Three logical surfaces, one canonical unit, two boundaries:
//!
//! ```text
//!   [ Display Layer ]      ← user-picked unit (m / cm / mm / ft / in / studs)
//!         ▲
//!         │   convert + epsilon_snap (jitter-free formatter)
//!         ▼
//!   [ Engine-Native ]      ← always meters. 1 unit = 1 meter.
//!         ▲                  gravity in m/s², BasePart.size in m,
//!         │                  Transform.translation in m, raycasts in m,
//!         │                  gizmo math in m, mesh AABBs in m.
//!         │   convert (authored ↔ meters)
//!         ▼
//!   [ Authored Layer ]     ← whatever unit each _instance.toml carries
//!                            (recorded as `metadata.unit = "<symbol>"`,
//!                            mirrored on the entity as MeasureUnit).
//! ```
//!
//! The engine never carries a runtime "scale factor". Conversions
//! happen at exactly two pinch points:
//!
//! - **At load**: `authored unit → meters` once per dimensional field
//!   (`spawn_instance`, hot-load, paste, MCP `create_entity`).
//! - **At write**: `meters → authored unit` once per dimensional field
//!   (the unified writer + every save_*_changes path).
//!
//! Everything in between is straight f32 meters. Bevy / Avian / Slint /
//! the property panel never see a stud unless the user explicitly asks
//! for studs as a *display* unit.
//!
//! ## Stud value
//!
//! 1 stud ≡ 0.28 meters, Roblox's stud. Symbol `"stud"`.
//!
//! Files written before the stud became Roblox's declare `"studs"`,
//! which reads as [`Unit::LegacyStud`]: `9.815 / 196.8` meters
//! (≈ 0.04987 m), the ratio the studs-native build was tuned with
//! (gravity 196.8 stud/s² mirroring 9.815 m/s²). Such a file loads at
//! exactly the size it always had and is written back in its own
//! unit; the legacy stud is never offered for picking ([`Unit::PICKABLE`]).
//!
//! ## Floating-point hygiene
//!
//! Conversions go through `f64` internally regardless of caller width;
//! the f32 entry points (`convert_vec3_f32`, etc.) widen → convert →
//! narrow. This buys two extra decimal digits of headroom for
//! multi-step round-trips like `m → studs → m → ft → m`.
//!
//! Display-layer jitter is handled by `epsilon_snap_to_grain`, which
//! collapses values within ε of a clean grain (`0.001` by default) to
//! that grain. The model layer never sees snapped values — only the
//! Slint formatter does — so converting back doesn't lose precision.
//!
//! ## What lives where
//!
//! - [`Unit`]: the length units (Meter / Centimeter / Millimeter /
//!   Foot / Inch / Stud, plus LegacyStud for old files), and
//!   [`Unit::PICKABLE`], the ones a user may choose.
//! - [`MeasureUnit`] — Bevy `Component` carrying the entity's authored
//!   unit. Defaults to `Meter` so any spawn path that doesn't think
//!   about units gets the engine-native treatment for free.
//! - [`convert`], [`convert_vec3_f32`], [`convert_vec3_f64`] — value
//!   conversion. Identity when from == to.
//! - [`epsilon_snap_to_grain`] — display-only jitter killer.
//! - [`ENGINE_NATIVE_UNIT`] — `Unit::Meter`. The one constant the rest
//!   of the engine should reach for when asking "what unit does ECS
//!   store?"
//! - [`STANDARD_GRAVITY`]: g₀, 9.80665 m/s². The default gravity, and
//!   the constant for quantities defined at standard gravity.

use bevy::prelude::{Component, Resource};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Unit
// ─────────────────────────────────────────────────────────────────────────────

/// A unit of length. The SI cascade, the imperial pair and Roblox's
/// stud cover the authoring cases; [`Unit::PICKABLE`] lists them.
/// Adding a variant is a row in `to_meters`, `symbol`, `display_name`,
/// `from_symbol`, `from_any` and `display_grain`.
///
/// `Stud` lets Roblox-shape Spaces keep their values intact, with
/// `unit = "stud"` declared on disk; the engine converts on load.
/// `LegacyStud` only reads (and writes back) files that declare the
/// older `"studs"`. It stays the LAST variant: the enum serializes by
/// index in some stores (DisplayUnit settings, worldDB MeasureUnit),
/// so existing indices must not move. There is intentionally no
/// `Yard` or `Kilometer`: too rare to merit the surface area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Unit {
    Meter,
    Centimeter,
    Millimeter,
    Foot,
    Inch,
    /// Roblox's stud, 0.28 m. Symbol `"stud"`.
    Stud,
    /// The stud older files declare, `9.815 / 196.8` m. Symbol
    /// `"studs"`. Read and written back, never offered.
    LegacyStud,
}

impl Default for Unit {
    /// `Meter` matches the engine-native unit. Files without a
    /// `metadata.unit` field, components freshly spawned with no
    /// override, and Space scaffolds that don't declare a default all
    /// land here. Picking the same default as the engine means the
    /// "no-conversion" path is also the most-common path.
    fn default() -> Self { Unit::Meter }
}

impl Unit {
    /// The units a user may choose: every display and authoring picker
    /// offers exactly these, in this order. [`Unit::LegacyStud`] is not
    /// one of them.
    pub const PICKABLE: &'static [Unit] =
        &[Unit::Meter, Unit::Centimeter, Unit::Millimeter, Unit::Foot, Unit::Inch, Unit::Stud];

    /// Meters per one unit of `self`. The single source of truth for
    /// every numeric conversion in the engine.
    ///
    /// Exact wherever the unit has an exact definition:
    /// - Foot  = 0.3048 m   (by the 1959 International Yard agreement)
    /// - Inch  = 0.0254 m   (foot / 12)
    /// - Stud  = 0.28 m     (Roblox's stud)
    /// - LegacyStud = 9.815 / 196.8 m, kept exact so a file that
    ///   declares it loads at exactly the size it was written at.
    pub const fn to_meters(self) -> f64 {
        match self {
            Unit::Meter      => 1.0,
            Unit::Centimeter => 0.01,
            Unit::Millimeter => 0.001,
            Unit::Foot       => 0.3048,
            Unit::Inch       => 0.0254,
            Unit::Stud       => 0.28,
            // Const-evaluated: 9.815 / 196.8 ≈ 0.04987297560975609756...
            Unit::LegacyStud => 9.815_f64 / 196.8_f64,
        }
    }

    /// Disk symbol — the string stored in `metadata.unit = "<…>"`.
    /// Round-trips with [`Unit::from_symbol`].
    pub const fn symbol(self) -> &'static str {
        match self {
            Unit::Meter      => "m",
            Unit::Centimeter => "cm",
            Unit::Millimeter => "mm",
            Unit::Foot       => "ft",
            Unit::Inch       => "in",
            Unit::Stud       => "stud",
            Unit::LegacyStud => "studs",
        }
    }

    /// Human-readable name for the status-bar dropdown and toasts.
    pub const fn display_name(self) -> &'static str {
        match self {
            Unit::Meter      => "Meters",
            Unit::Centimeter => "Centimeters",
            Unit::Millimeter => "Millimeters",
            Unit::Foot       => "Feet",
            Unit::Inch       => "Inches",
            Unit::Stud       => "Studs",
            Unit::LegacyStud => "Studs (legacy)",
        }
    }

    /// Strict symbol lookup — only the canonical strings emitted by
    /// [`Unit::symbol`]. Use this for disk parsing where you want
    /// to fail loudly on unknown values. `"studs"` is the legacy stud,
    /// so a file keeps the size it was written at; a user's choice goes
    /// through [`Unit::from_any`] or a symbol from [`Unit::PICKABLE`].
    pub fn from_symbol(s: &str) -> Option<Unit> {
        match s {
            "m"     => Some(Unit::Meter),
            "cm"    => Some(Unit::Centimeter),
            "mm"    => Some(Unit::Millimeter),
            "ft"    => Some(Unit::Foot),
            "in"    => Some(Unit::Inch),
            "stud"  => Some(Unit::Stud),
            "studs" => Some(Unit::LegacyStud),
            _ => None,
        }
    }

    /// Lenient lookup — accepts the canonical symbol AND every
    /// reasonable alternative (full name, singular, plural,
    /// capitalised). Use this for user-typed input (MCP tool args,
    /// command bar, scripts). "stud" and "studs" both mean Roblox's
    /// stud here; nothing a user types names the legacy stud.
    pub fn from_any(s: &str) -> Option<Unit> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "m" | "meter" | "meters" | "metre" | "metres" => Some(Unit::Meter),
            "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => Some(Unit::Centimeter),
            "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => Some(Unit::Millimeter),
            "ft" | "foot" | "feet" => Some(Unit::Foot),
            "in" | "inch" | "inches" => Some(Unit::Inch),
            "stud" | "studs" => Some(Unit::Stud),
            _ => None,
        }
    }

    /// Natural snap-grain for display rounding in this unit. Used by
    /// the Properties-panel formatter via [`epsilon_snap_to_grain`]
    /// to render `5.0 ft` cleanly instead of `5.000000123 ft` after a
    /// round-trip through meters.
    ///
    /// Sub-millimeter precision on display is below what any author
    /// can see or care about; the underlying model values stay full
    /// precision.
    pub const fn display_grain(self) -> f64 {
        match self {
            // 1 mm precision for the metric units (cm.grain = 0.1 cm =
            // 1 mm, etc.); a thousandth of the unit for feet, inches and
            // both studs, which is under a millimetre for each.
            Unit::Meter      => 0.001,
            Unit::Centimeter => 0.1,    // = 1 mm
            Unit::Millimeter => 1.0,    // already mm
            Unit::Foot       => 0.001,
            Unit::Inch       => 0.001,
            Unit::Stud       => 0.001,
            Unit::LegacyStud => 0.001,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine-native pin
// ─────────────────────────────────────────────────────────────────────────────

/// The unit Bevy / Avian / the ECS think in. `1 unit = 1 meter`.
///
/// Every dimensional value living in `BasePart.size`,
/// `Transform.translation`, `Transform.scale`, Avian colliders, mesh
/// AABBs, raycast distances, gizmo offsets — all are in this unit.
/// Other surfaces (disk, Properties panel) convert at their boundary.
pub const ENGINE_NATIVE_UNIT: Unit = Unit::Meter;

// ─────────────────────────────────────────────────────────────────────────────
// Standard gravity
// ─────────────────────────────────────────────────────────────────────────────

/// Standard gravity, g₀, in m/s²: exactly 9.80665 by definition.
///
/// Two kinds of code use it, and they must not be confused:
///
/// - **Defaults.** A new Space's `Workspace.gravity` and the Avian `Gravity`
///   inserted at startup start here. Once a Space runs, the live value is
///   `Workspace.gravity`, which `sync_workspace_gravity_to_avian` copies into
///   Avian's `Gravity` every frame; scripts and tools change it there.
/// - **Quantities defined at standard gravity.** A character's authored jump
///   apex, specific impulse, or the depth-pressure of a formula documented
///   for Earth. These stay fixed when a Space changes its gravity.
///
/// Anything that simulates a falling or floating body reads the live gravity
/// instead, so it follows a Space set to the Moon.
pub const STANDARD_GRAVITY: f64 = 9.80665;

/// [`STANDARD_GRAVITY`] as `f32`, for Bevy and Avian vectors.
pub const STANDARD_GRAVITY_F32: f32 = STANDARD_GRAVITY as f32;

// ─────────────────────────────────────────────────────────────────────────────
// MeasureUnit component
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy component recording the unit an entity was authored in. Set
/// at spawn time from the file's `metadata.unit`; read whenever the
/// engine writes a dimensional value back to disk.
///
/// Default is [`Unit::Meter`] — the engine-native unit. Entities spawned
/// without an explicit unit go through the engine identity-conversion
/// path on the next write, so they stay consistent.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct MeasureUnit(pub Unit);

impl MeasureUnit {
    pub fn new(unit: Unit) -> Self { Self(unit) }
    pub fn get(&self) -> Unit { self.0 }
}

/// Bevy Resource tracking the unit the user currently wants displayed
/// in the Properties panel, status-bar readouts, gizmo overlays, and
/// the Measure tool. Defaults to [`Unit::Meter`] so first-launch
/// sessions match the engine-native unit.
///
/// `DisplayUnit` is independent of [`MeasureUnit`]: changing it never
/// touches disk and never mutates an entity's authored unit — it's a
/// purely cosmetic, session-scoped projection. The Properties panel
/// reads each entity's `MeasureUnit` to know how the file stores
/// values, converts engine-native meters to `DisplayUnit` on render,
/// and the user's typed input is parsed back through `DisplayUnit →
/// Meter` before reaching ECS / disk.
///
/// Persistence is the caller's responsibility — typically the
/// engine's user-settings TOML on disk.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct DisplayUnit(pub Unit);

impl DisplayUnit {
    pub fn new(unit: Unit) -> Self { Self(unit) }
    pub fn get(&self) -> Unit { self.0 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scalar conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a length value between units. f64 internally for headroom
/// against multi-step chains (`m → ft → studs → m`); narrow at the
/// caller if needed.
///
/// Identity path: `from == to` returns `v` bit-exact.
#[inline]
pub fn convert(v: f64, from: Unit, to: Unit) -> f64 {
    if from == to { return v; }
    v * (from.to_meters() / to.to_meters())
}

/// f32 entry point. Widens to f64 for the conversion to keep the
/// extra ~6 digits of precision, then narrows. Hot-path on every
/// dimensional disk write; benchmarked at <1 ns per call.
#[inline]
pub fn convert_f32(v: f32, from: Unit, to: Unit) -> f32 {
    if from == to { return v; }
    convert(v as f64, from, to) as f32
}

/// Convert a 3-tuple of lengths. Component-wise; no cross-mixing.
/// Used for `position` / `scale` / dimensional offsets.
#[inline]
pub fn convert_vec3_f32(v: [f32; 3], from: Unit, to: Unit) -> [f32; 3] {
    if from == to { return v; }
    [
        convert_f32(v[0], from, to),
        convert_f32(v[1], from, to),
        convert_f32(v[2], from, to),
    ]
}

/// f64 variant for high-precision callers (the migration tool, the
/// MCP layer that ships numbers across the wire as f64).
#[inline]
pub fn convert_vec3_f64(v: [f64; 3], from: Unit, to: Unit) -> [f64; 3] {
    if from == to { return v; }
    [convert(v[0], from, to), convert(v[1], from, to), convert(v[2], from, to)]
}

// ─────────────────────────────────────────────────────────────────────────────
// Disk-persistence boundary helpers
// ─────────────────────────────────────────────────────────────────────────────
//
// Every dimensional value crossing the disk boundary should go through
// one of these helpers — never raw `convert*`. The named direction
// makes audits trivial: a `grep engine_to_authored` returns the entire
// set of disk-write surfaces; a `grep authored_to_engine` returns
// every loader. The two functions are tiny wrappers around `convert*`,
// but adopting them by convention prevents the multi-write-surface
// bug class flagged as risk R-Stage-4 in the rollout plan.

/// Inverse of the load conversion: take an engine-native (meter) value
/// and emit it in the unit the file is authored in. Use at every save
/// site that writes a `Transform`, `BasePart.size`, `units_offset`, or
/// other length-typed field back to disk.
#[inline]
pub fn engine_to_authored_f32(v: f32, authored: Unit) -> f32 {
    convert_f32(v, ENGINE_NATIVE_UNIT, authored)
}

/// Vec3 disk-write helper. Component-wise; same identity short-circuit
/// as the underlying `convert_vec3_f32` so meter-authored files pay
/// zero cost.
#[inline]
pub fn engine_to_authored_vec3_f32(v: [f32; 3], authored: Unit) -> [f32; 3] {
    convert_vec3_f32(v, ENGINE_NATIVE_UNIT, authored)
}

/// Load-side companion. The loaders already call `convert_vec3_f32`
/// directly; this alias exists so future readers can `grep` for both
/// boundaries with a single keyword pair.
#[inline]
pub fn authored_to_engine_vec3_f32(v: [f32; 3], authored: Unit) -> [f32; 3] {
    convert_vec3_f32(v, authored, ENGINE_NATIVE_UNIT)
}

/// Scalar load-side helper.
#[inline]
pub fn authored_to_engine_f32(v: f32, authored: Unit) -> f32 {
    convert_f32(v, authored, ENGINE_NATIVE_UNIT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Acceleration (gravity) conversion
// ─────────────────────────────────────────────────────────────────────────────
//
// Acceleration is a length per time². Because the engine's time unit is the
// same second everywhere, converting an acceleration between unit systems is
// the identical length ratio used by `convert_f32` — the s² cancels. So gravity
// in studs/s² → m/s² is just the stud→meter ratio applied to the magnitude, and
// the generic length helpers below give us this for free without adding an
// acceleration variant to `Unit`. These named wrappers exist so gravity-sync
// code reads (and `grep`s) as an acceleration conversion, not a length one.

/// Convert a scalar acceleration (e.g. gravity magnitude) authored in `from`
/// to the value in `to`. Same numeric ratio as [`convert_f32`] because the time
/// component is shared across unit systems.
#[inline]
pub fn convert_accel_f32(v: f32, from: Unit, to: Unit) -> f32 {
    convert_f32(v, from, to)
}

/// Convert an acceleration vector (e.g. an Avian gravity vector) authored in
/// `from` to engine-native (meter) units. Component-wise; identity when
/// `from == ENGINE_NATIVE_UNIT`.
#[inline]
pub fn accel_to_engine_vec3_f32(v: [f32; 3], from: Unit) -> [f32; 3] {
    convert_vec3_f32(v, from, ENGINE_NATIVE_UNIT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Velocity conversion
// ─────────────────────────────────────────────────────────────────────────────
//
// Velocity is length per time; the shared second cancels, so it's the same
// length ratio as `convert_f32` (exactly like acceleration above). These named
// wrappers exist so velocity-handling code reads as a velocity conversion.
// NOTE: ANGULAR velocity (rad/s) is length-invariant — never route it here.

/// Convert a scalar velocity (a speed) authored in `from` to the value in `to`.
#[inline]
pub fn convert_velocity_f32(v: f32, from: Unit, to: Unit) -> f32 {
    convert_f32(v, from, to)
}

/// Convert a velocity vector (e.g. an Avian `LinearVelocity`) authored in `from`
/// to engine-native (meter) units. Component-wise; identity when
/// `from == ENGINE_NATIVE_UNIT`.
#[inline]
pub fn velocity_authored_to_engine_vec3_f32(v: [f32; 3], from: Unit) -> [f32; 3] {
    convert_vec3_f32(v, from, ENGINE_NATIVE_UNIT)
}

/// Inverse of [`velocity_authored_to_engine_vec3_f32`] for write-back: convert
/// an engine-native velocity vector to the authored unit.
#[inline]
pub fn velocity_engine_to_authored_vec3_f32(v: [f32; 3], authored: Unit) -> [f32; 3] {
    convert_vec3_f32(v, ENGINE_NATIVE_UNIT, authored)
}

// ─────────────────────────────────────────────────────────────────────────────
// Display-side formatting
// ─────────────────────────────────────────────────────────────────────────────

/// Format an engine-native (meter) length for display in `unit`.
/// Used by gizmo readouts, the Measure tool, and the Properties
/// panel preview. Applies [`epsilon_snap_to_grain`] so values within
/// ε of a clean grain render cleanly (`5.0 ft` not `4.9999998 ft`)
/// without round-tripping through the panel. Default precision is
/// 3 decimal places — comfortable for mm-resolution authoring.
pub fn format_length_in(meters: f64, unit: Unit) -> String {
    let v = convert(meters, ENGINE_NATIVE_UNIT, unit);
    let grain = unit.display_grain();
    let v = epsilon_snap_to_grain(v, grain);
    format!("{:.3} {}", v, unit.symbol())
}

/// f32 entry point — wraps `format_length_in` for the common case
/// where the caller already has an `f32` (gizmo deltas, transform
/// components, etc.).
#[inline]
pub fn format_length_in_f32(meters: f32, unit: Unit) -> String {
    format_length_in(meters as f64, unit)
}

// ─────────────────────────────────────────────────────────────────────────────
// Display-layer jitter snap
// ─────────────────────────────────────────────────────────────────────────────

/// Snap a value to its nearest multiple of `grain` when the residual
/// is within a tiny tolerance of that multiple. Used by the Properties
/// panel's display formatter so a `5.0 ft` value that has decayed to
/// `5.0000000131 ft` through `ft → m → ft` renders as `5.000` without
/// the model layer ever losing precision.
///
/// `grain` is typically `Unit::display_grain()` (1 mm for metric, 0.001
/// for imperial). Pass a different grain to enforce sharper rounding —
/// e.g. integer-only snap with `grain = 1.0`.
///
/// The tolerance is `grain * 1e-3`, so values farther than 0.001 *
/// `grain` from a clean multiple are returned untouched. This keeps
/// `0.5005 m` displaying as `0.5005 m`, not snapped to `0.500`.
#[inline]
pub fn epsilon_snap_to_grain(v: f64, grain: f64) -> f64 {
    if !v.is_finite() || grain <= 0.0 { return v; }
    let n = (v / grain).round();
    let snapped = n * grain;
    let tol = grain * 1e-3;
    if (v - snapped).abs() <= tol { snapped } else { v }
}

/// Convenience: snap for display in the given unit. Equivalent to
/// `epsilon_snap_to_grain(v, unit.display_grain())`.
#[inline]
pub fn epsilon_snap_for_display(v: f64, unit: Unit) -> f64 {
    epsilon_snap_to_grain(v, unit.display_grain())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const FUZZ: f64 = 1e-10;

    /// Velocity vectors round-trip authored->engine->authored (studs<->meters),
    /// and are identity when authored == engine-native. Acceleration/gravity
    /// share the same length-ratio path (exercised by gravity sync).
    #[test]
    fn velocity_vec3_round_trip() {
        let v = [12.5_f32, -3.25, 100.0];
        let eng = velocity_authored_to_engine_vec3_f32(v, Unit::Stud);
        let back = velocity_engine_to_authored_vec3_f32(eng, Unit::Stud);
        for i in 0..3 {
            assert!(
                (v[i] - back[i]).abs() < 1e-3,
                "velocity round-trip studs->m->studs failed at {i}: {} vs {}",
                v[i], back[i]
            );
        }
        // Identity when authored unit == engine-native (no conversion).
        assert_eq!(velocity_authored_to_engine_vec3_f32(v, ENGINE_NATIVE_UNIT), v);
    }

    /// Every variant, the legacy stud included.
    const ALL: [Unit; 7] = [Unit::Meter, Unit::Centimeter, Unit::Millimeter,
                            Unit::Foot, Unit::Inch, Unit::Stud, Unit::LegacyStud];

    /// Identity conversion is bit-exact.
    #[test]
    fn identity_is_bit_exact() {
        for &u in &ALL {
            assert_eq!(convert(3.141_592_653_589_793, u, u), 3.141_592_653_589_793);
        }
    }

    /// Every unit pair round-trips a→b→a within f64 precision.
    #[test]
    fn round_trip_all_pairs() {
        let units = ALL;
        for &from in &units {
            for &to in &units {
                let v = 12.345_678_9_f64;
                let there = convert(v, from, to);
                let back  = convert(there, to, from);
                assert!((v - back).abs() < FUZZ,
                    "round-trip {from:?} → {to:?} → {from:?}: {v} → {there} → {back} (Δ={})",
                    (v - back).abs());
            }
        }
    }

    /// Exact SI/imperial constants — these are by definition.
    #[test]
    fn exact_si_imperial_constants() {
        assert_eq!(Unit::Foot.to_meters(), 0.3048);
        assert_eq!(Unit::Inch.to_meters(), 0.0254);
        assert_eq!(Unit::Centimeter.to_meters(), 0.01);
        assert_eq!(Unit::Millimeter.to_meters(), 0.001);
        // 1 ft = 12 in (within fp precision)
        assert!((Unit::Foot.to_meters() - 12.0 * Unit::Inch.to_meters()).abs() < FUZZ);
    }

    /// Crates below `common` cannot name [`STANDARD_GRAVITY`], so they keep
    /// their own copies; every copy must be the same value.
    #[test]
    fn every_standard_gravity_copy_agrees() {
        assert_eq!(STANDARD_GRAVITY, 9.80665);
        assert_eq!(eustress_avatar_schema::GRAVITY_MPS2, STANDARD_GRAVITY_F32);
        assert_eq!(
            eustress_particle_sim::SimParams::default().gravity.to_array(),
            [0.0, -STANDARD_GRAVITY_F32, 0.0],
        );
        assert_eq!(
            crate::services::workspace::Workspace::default().gravity.to_array(),
            [0.0, -STANDARD_GRAVITY_F32, 0.0],
        );
    }

    /// A stud is Roblox's, 0.28 m exactly.
    #[test]
    fn stud_is_roblox_stud() {
        assert_eq!(Unit::Stud.to_meters(), 0.28);
        assert!((convert(10.0, Unit::Stud, Unit::Meter) - 2.8).abs() < FUZZ);
    }

    /// The legacy stud is bit for bit the ratio older files were written
    /// with, so every one of them loads at the size it always had.
    #[test]
    fn a_legacy_studs_file_keeps_its_size() {
        let old_ratio = 9.815_f64 / 196.8_f64;
        assert_eq!(Unit::LegacyStud.to_meters().to_bits(), old_ratio.to_bits());
        let legacy = Unit::from_symbol("studs").expect("\"studs\" still parses");
        for v in [0.0, 1.0, 196.8, 12.345_678_9, -40.5, 2048.0] {
            assert_eq!(convert(v, legacy, Unit::Meter), v * old_ratio, "{v} studs");
        }
        // Written back in its own unit: the symbol stays "studs".
        assert_eq!(legacy.symbol(), "studs");
    }

    /// Files name the legacy stud "studs"; nothing a user types or picks
    /// names it.
    #[test]
    fn stud_symbols_map_to_the_right_stud() {
        assert_eq!(Unit::from_symbol("studs"), Some(Unit::LegacyStud));
        assert_eq!(Unit::from_symbol("stud"), Some(Unit::Stud));
        assert_eq!(Unit::from_any("studs"), Some(Unit::Stud));
        assert_eq!(Unit::from_any("stud"), Some(Unit::Stud));
        assert_eq!(Unit::from_any("Studs"), Some(Unit::Stud));
        assert!(!Unit::PICKABLE.contains(&Unit::LegacyStud));
        assert!(Unit::PICKABLE.contains(&Unit::Stud));
    }

    /// Every pickable unit survives a trip through its symbol and back
    /// through the lenient parser, so a picker that sends symbols works.
    #[test]
    fn every_pickable_unit_round_trips() {
        assert_eq!(Unit::PICKABLE.len(), ALL.len() - 1);
        for &u in Unit::PICKABLE {
            assert_eq!(Unit::from_symbol(u.symbol()), Some(u), "{u:?}");
            assert_eq!(Unit::from_any(u.symbol()), Some(u), "{u:?}");
        }
    }

    #[test]
    fn symbol_round_trip() {
        for &u in &ALL {
            let s = u.symbol();
            assert_eq!(Unit::from_symbol(s), Some(u), "symbol {} should reparse", s);
        }
    }

    #[test]
    fn from_any_accepts_aliases() {
        assert_eq!(Unit::from_any("meters"), Some(Unit::Meter));
        assert_eq!(Unit::from_any("METERS"), Some(Unit::Meter));
        assert_eq!(Unit::from_any("Feet"),   Some(Unit::Foot));
        assert_eq!(Unit::from_any("inch"),   Some(Unit::Inch));
        assert_eq!(Unit::from_any("studs"),  Some(Unit::Stud));
        assert_eq!(Unit::from_any("nope"),   None);
    }

    #[test]
    fn vec3_conversion_componentwise() {
        let v = [1.0_f32, 2.0, 3.0];
        let out = convert_vec3_f32(v, Unit::Meter, Unit::Foot);
        // 1 m = 3.2808... ft, 2 m = 6.5617... ft, 3 m = 9.8425... ft
        assert!((out[0] - 3.280_839_9).abs() < 1e-4);
        assert!((out[1] - 6.561_679_8).abs() < 1e-4);
        assert!((out[2] - 9.842_519_7).abs() < 1e-4);
    }

    /// The user's headline jitter case: 5.0 ft → meters → studs → meters
    /// → ft, then snapped to 1 mm display grain, recovers 5.0 ft.
    #[test]
    fn jitter_round_trip_snaps_to_clean_five_ft() {
        let v_ft = 5.0_f64;
        let v_m = convert(v_ft, Unit::Foot, Unit::Meter);
        let v_studs = convert(v_m, Unit::Meter, Unit::Stud);
        let v_m_back = convert(v_studs, Unit::Stud, Unit::Meter);
        let v_ft_back = convert(v_m_back, Unit::Meter, Unit::Foot);
        // Without snap, micro-residual remains.
        let snapped = epsilon_snap_for_display(v_ft_back, Unit::Foot);
        assert_eq!(snapped, 5.0,
            "round-trip 5ft → m → studs → m → ft should snap to 5.0 (raw={v_ft_back})");
    }

    /// Snap is conservative: 0.5005 m doesn't get rounded to 0.5005...
    /// no wait, it gets rounded to 0.5005 already because grain is 0.001 m.
    /// The point: a value that is genuinely between grains stays as-is.
    #[test]
    fn snap_leaves_non_grain_values_alone() {
        let v = 0.500_5_f64;
        assert_eq!(epsilon_snap_to_grain(v, 0.001), 0.5005); // exact-grain → keeps
        let v = 0.500_53_f64;
        let snapped = epsilon_snap_to_grain(v, 0.001);
        // Within 1e-6 (= grain * 1e-3) of 0.5005 → snaps. 0.50053 is 3e-5
        // away from 0.5005 → does NOT snap → stays 0.50053.
        assert!((snapped - 0.500_53).abs() < 1e-12);
    }

    #[test]
    fn snap_handles_nan_and_zero_grain() {
        assert!(epsilon_snap_to_grain(f64::NAN, 0.001).is_nan());
        assert_eq!(epsilon_snap_to_grain(1.5, 0.0), 1.5); // grain ≤ 0 → no-op
    }

    #[test]
    fn measure_unit_default_is_meter() {
        assert_eq!(MeasureUnit::default(), MeasureUnit(Unit::Meter));
    }
}
