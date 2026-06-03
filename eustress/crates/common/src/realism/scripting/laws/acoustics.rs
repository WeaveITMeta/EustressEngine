//! Rune bindings for the acoustics laws (waves, propagation, room acoustics).
//!
//! Exposed to scripts under `eustress::realism::acoustics::*`. Each binding is
//! a thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::laws::acoustics::{waves, propagation, rooms}`, because Rune
//! works in f64 while the realism kernel is f32. Integer-mode parameters are
//! taken as i64 and narrowed to the kernel's u32.
//!
//! Functions returning `Option` (`mach_cone_angle`) and the slice forms
//! (`combine_sound_levels_db`, `total_absorption`) are intentionally not bound.

use rune::{ContextError, Module};
use crate::realism::laws::acoustics::{waves, propagation, rooms};

// ----- waves ---------------------------------------------------------------

#[rune::function]
fn sound_speed_solid(youngs_modulus: f64, density: f64) -> f64 {
    waves::sound_speed_solid(youngs_modulus as f32, density as f32) as f64
}

#[rune::function]
fn sound_speed_fluid(bulk_modulus: f64, density: f64) -> f64 {
    waves::sound_speed_fluid(bulk_modulus as f32, density as f32) as f64
}

#[rune::function]
fn sound_speed_ideal_gas(gamma: f64, r_specific: f64, temperature: f64) -> f64 {
    waves::sound_speed_ideal_gas(gamma as f32, r_specific as f32, temperature as f32) as f64
}

#[rune::function]
fn sound_speed_air(temperature_celsius: f64) -> f64 {
    waves::sound_speed_air(temperature_celsius as f32) as f64
}

#[rune::function]
fn acoustic_impedance(density: f64, sound_speed: f64) -> f64 {
    waves::acoustic_impedance(density as f32, sound_speed as f32) as f64
}

#[rune::function]
fn sound_intensity(pressure_amplitude: f64, impedance: f64) -> f64 {
    waves::sound_intensity(pressure_amplitude as f32, impedance as f32) as f64
}

#[rune::function]
fn sound_pressure_level(pressure: f64) -> f64 {
    waves::sound_pressure_level(pressure as f32) as f64
}

#[rune::function]
fn sound_intensity_level(intensity: f64) -> f64 {
    waves::sound_intensity_level(intensity as f32) as f64
}

#[rune::function]
fn wavelength_acoustic(sound_speed: f64, frequency: f64) -> f64 {
    waves::wavelength_acoustic(sound_speed as f32, frequency as f32) as f64
}

// ----- propagation ---------------------------------------------------------

#[rune::function]
fn doppler_observed_frequency(
    source_freq: f64,
    sound_speed: f64,
    observer_velocity: f64,
    source_velocity: f64,
) -> f64 {
    propagation::doppler_observed_frequency(
        source_freq as f32,
        sound_speed as f32,
        observer_velocity as f32,
        source_velocity as f32,
    ) as f64
}

#[rune::function]
fn mach_number(velocity: f64, sound_speed: f64) -> f64 {
    propagation::mach_number(velocity as f32, sound_speed as f32) as f64
}

#[rune::function]
fn spherical_spreading_intensity(source_intensity: f64, r0: f64, r: f64) -> f64 {
    propagation::spherical_spreading_intensity(source_intensity as f32, r0 as f32, r as f32) as f64
}

#[rune::function]
fn spherical_spreading_db_loss(r0: f64, r: f64) -> f64 {
    propagation::spherical_spreading_db_loss(r0 as f32, r as f32) as f64
}

#[rune::function]
fn atmospheric_absorption_db(distance: f64, absorption_coeff_db_per_m: f64) -> f64 {
    propagation::atmospheric_absorption_db(distance as f32, absorption_coeff_db_per_m as f32) as f64
}

#[rune::function]
fn inverse_square_law(power: f64, distance: f64) -> f64 {
    propagation::inverse_square_law(power as f32, distance as f32) as f64
}

// ----- rooms ---------------------------------------------------------------

#[rune::function]
fn sabine_reverberation_time(volume: f64, total_absorption: f64) -> f64 {
    rooms::sabine_reverberation_time(volume as f32, total_absorption as f32) as f64
}

#[rune::function]
fn eyring_reverberation_time(volume: f64, surface_area: f64, mean_absorption: f64) -> f64 {
    rooms::eyring_reverberation_time(volume as f32, surface_area as f32, mean_absorption as f32) as f64
}

#[rune::function]
fn critical_distance(room_constant: f64) -> f64 {
    rooms::critical_distance(room_constant as f32) as f64
}

#[rune::function]
fn room_constant(total_absorption: f64, mean_absorption: f64) -> f64 {
    rooms::room_constant(total_absorption as f32, mean_absorption as f32) as f64
}

#[rune::function]
fn schroeder_frequency(reverberation_time: f64, volume: f64) -> f64 {
    rooms::schroeder_frequency(reverberation_time as f32, volume as f32) as f64
}

#[rune::function]
fn axial_mode_frequency(mode: i64, dimension: f64) -> f64 {
    rooms::axial_mode_frequency(mode as u32, dimension as f32) as f64
}

#[rune::function]
fn mode_count_below(frequency: f64, volume: f64) -> f64 {
    rooms::mode_count_below(frequency as f32, volume as f32) as f64
}

/// Build the `eustress::realism::acoustics` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "acoustics"])?;
    // waves
    m.function_meta(sound_speed_solid)?;
    m.function_meta(sound_speed_fluid)?;
    m.function_meta(sound_speed_ideal_gas)?;
    m.function_meta(sound_speed_air)?;
    m.function_meta(acoustic_impedance)?;
    m.function_meta(sound_intensity)?;
    m.function_meta(sound_pressure_level)?;
    m.function_meta(sound_intensity_level)?;
    m.function_meta(wavelength_acoustic)?;
    // propagation
    m.function_meta(doppler_observed_frequency)?;
    m.function_meta(mach_number)?;
    m.function_meta(spherical_spreading_intensity)?;
    m.function_meta(spherical_spreading_db_loss)?;
    m.function_meta(atmospheric_absorption_db)?;
    m.function_meta(inverse_square_law)?;
    // rooms
    m.function_meta(sabine_reverberation_time)?;
    m.function_meta(eyring_reverberation_time)?;
    m.function_meta(critical_distance)?;
    m.function_meta(room_constant)?;
    m.function_meta(schroeder_frequency)?;
    m.function_meta(axial_mode_frequency)?;
    m.function_meta(mode_count_below)?;
    Ok(m)
}
