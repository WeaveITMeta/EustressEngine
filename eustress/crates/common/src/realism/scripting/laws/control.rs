//! Rune bindings for the control-systems laws.
//!
//! Exposed to scripts under `eustress::realism::control::*`. Each binding is a
//! thin f64 wrapper around the f32 kernel law in
//! `crate::realism::control::{frequency, discrete}`, because Rune works in f64
//! while the realism kernel is f32.
//!
//! Only pure scalar→scalar helpers are bound here. The transfer-function,
//! Bode, stability-margin, bilinear-transform, PID, and IIR/filter routines
//! operate on slices, return tuples/`Option`, or mutate state, so they are not
//! exposed to Rune.

use rune::{ContextError, Module};
use crate::realism::control::{frequency, discrete};

// ── frequency ────────────────────────────────────────────────────

#[rune::function]
fn mag_to_db(magnitude: f64) -> f64 {
    frequency::mag_to_db(magnitude as f32) as f64
}

#[rune::function]
fn db_to_mag(db: f64) -> f64 {
    frequency::db_to_mag(db as f32) as f64
}

#[rune::function]
fn first_order_step(k: f64, tau: f64, t: f64) -> f64 {
    frequency::first_order_step(k as f32, tau as f32, t as f32) as f64
}

#[rune::function]
fn time_constant_from_63pct(t_63pct: f64) -> f64 {
    frequency::time_constant_from_63pct(t_63pct as f32) as f64
}

#[rune::function]
fn damping_from_overshoot(overshoot_pct: f64) -> f64 {
    frequency::damping_from_overshoot(overshoot_pct as f32) as f64
}

#[rune::function]
fn peak_time(omega_n: f64, zeta: f64) -> f64 {
    frequency::peak_time(omega_n as f32, zeta as f32) as f64
}

#[rune::function]
fn settling_time_2pct(omega_n: f64, zeta: f64) -> f64 {
    frequency::settling_time_2pct(omega_n as f32, zeta as f32) as f64
}

// ── discrete ─────────────────────────────────────────────────────

#[rune::function]
fn prewarp_frequency(omega_c: f64, sample_period: f64) -> f64 {
    discrete::prewarp_frequency(omega_c as f32, sample_period as f32) as f64
}

/// Build the `eustress::realism::control` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "control"])?;
    m.function_meta(mag_to_db)?;
    m.function_meta(db_to_mag)?;
    m.function_meta(first_order_step)?;
    m.function_meta(time_constant_from_63pct)?;
    m.function_meta(damping_from_overshoot)?;
    m.function_meta(peak_time)?;
    m.function_meta(settling_time_2pct)?;
    m.function_meta(prewarp_frequency)?;
    Ok(m)
}
