//! Rune bindings for the optics laws (geometric, wave, photon physics).
//!
//! Exposed to scripts under `eustress::realism::optics::*`. Each binding is a
//! thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::laws::optics::{geometric, wave, photons}`, because Rune
//! works in f64 while the realism kernel is f32. Integer-order parameters are
//! taken as i64 and narrowed to the kernel's i32/u32.
//!
//! Functions returning `Option` (`snell_refraction_angle`, `critical_angle`)
//! and the slice/Vec3 forms are intentionally not bound.

use rune::{ContextError, Module};
use crate::realism::laws::optics::{geometric, wave, photons};

// ----- geometric -----------------------------------------------------------

#[rune::function]
fn thin_lens_image_distance(focal_length: f64, object_distance: f64) -> f64 {
    geometric::thin_lens_image_distance(focal_length as f32, object_distance as f32) as f64
}

#[rune::function]
fn thin_lens_focal_length(image_distance: f64, object_distance: f64) -> f64 {
    geometric::thin_lens_focal_length(image_distance as f32, object_distance as f32) as f64
}

#[rune::function]
fn magnification(image_distance: f64, object_distance: f64) -> f64 {
    geometric::magnification(image_distance as f32, object_distance as f32) as f64
}

#[rune::function]
fn lensmaker_focal_length(n: f64, r1: f64, r2: f64) -> f64 {
    geometric::lensmaker_focal_length(n as f32, r1 as f32, r2 as f32) as f64
}

#[rune::function]
fn mirror_image_distance(focal_length: f64, object_distance: f64) -> f64 {
    geometric::mirror_image_distance(focal_length as f32, object_distance as f32) as f64
}

#[rune::function]
fn mirror_focal_length(radius: f64) -> f64 {
    geometric::mirror_focal_length(radius as f32) as f64
}

#[rune::function]
fn fresnel_reflectance_normal(n1: f64, n2: f64) -> f64 {
    geometric::fresnel_reflectance_normal(n1 as f32, n2 as f32) as f64
}

#[rune::function]
fn fresnel_transmittance_normal(n1: f64, n2: f64) -> f64 {
    geometric::fresnel_transmittance_normal(n1 as f32, n2 as f32) as f64
}

#[rune::function]
fn brewster_angle(n1: f64, n2: f64) -> f64 {
    geometric::brewster_angle(n1 as f32, n2 as f32) as f64
}

#[rune::function]
fn snells_window_radius(depth: f64, n_water: f64) -> f64 {
    geometric::snells_window_radius(depth as f32, n_water as f32) as f64
}

// ----- wave ----------------------------------------------------------------

#[rune::function]
fn double_slit_fringe_spacing(wavelength: f64, screen_distance: f64, slit_separation: f64) -> f64 {
    wave::double_slit_fringe_spacing(wavelength as f32, screen_distance as f32, slit_separation as f32)
        as f64
}

#[rune::function]
fn double_slit_angle(order: i64, wavelength: f64, slit_separation: f64) -> f64 {
    wave::double_slit_angle(order as i32, wavelength as f32, slit_separation as f32) as f64
}

#[rune::function]
fn single_slit_minima_angle(order: i64, wavelength: f64, slit_width: f64) -> f64 {
    wave::single_slit_minima_angle(order as i32, wavelength as f32, slit_width as f32) as f64
}

#[rune::function]
fn diffraction_grating_angle(order: i64, wavelength: f64, line_spacing: f64) -> f64 {
    wave::diffraction_grating_angle(order as i32, wavelength as f32, line_spacing as f32) as f64
}

#[rune::function]
fn thin_film_constructive_wavelength(n_film: f64, thickness: f64, order: i64) -> f64 {
    wave::thin_film_constructive_wavelength(n_film as f32, thickness as f32, order as u32) as f64
}

#[rune::function]
fn rayleigh_resolution_angle(wavelength: f64, aperture_diameter: f64) -> f64 {
    wave::rayleigh_resolution_angle(wavelength as f32, aperture_diameter as f32) as f64
}

#[rune::function]
fn bragg_angle(order: i64, wavelength: f64, plane_spacing: f64) -> f64 {
    wave::bragg_angle(order as i32, wavelength as f32, plane_spacing as f32) as f64
}

#[rune::function]
fn optical_path_difference(n: f64, geometric_path: f64) -> f64 {
    wave::optical_path_difference(n as f32, geometric_path as f32) as f64
}

#[rune::function]
fn michelson_fringe_shift(mirror_displacement: f64, wavelength: f64) -> f64 {
    wave::michelson_fringe_shift(mirror_displacement as f32, wavelength as f32) as f64
}

// ----- photons -------------------------------------------------------------

#[rune::function]
fn photon_energy(frequency: f64) -> f64 {
    photons::photon_energy(frequency as f32) as f64
}

#[rune::function]
fn photon_energy_from_wavelength(wavelength: f64) -> f64 {
    photons::photon_energy_from_wavelength(wavelength as f32) as f64
}

#[rune::function]
fn photon_momentum(wavelength: f64) -> f64 {
    photons::photon_momentum(wavelength as f32) as f64
}

#[rune::function]
fn photoelectric_max_ke(frequency: f64, work_function_joules: f64) -> f64 {
    photons::photoelectric_max_ke(frequency as f32, work_function_joules as f32) as f64
}

#[rune::function]
fn threshold_frequency(work_function_joules: f64) -> f64 {
    photons::threshold_frequency(work_function_joules as f32) as f64
}

#[rune::function]
fn compton_wavelength_shift(scattering_angle_rad: f64) -> f64 {
    photons::compton_wavelength_shift(scattering_angle_rad as f32) as f64
}

#[rune::function]
fn wien_peak_wavelength(temperature: f64) -> f64 {
    photons::wien_peak_wavelength(temperature as f32) as f64
}

#[rune::function]
fn stefan_boltzmann_power(emissivity: f64, area: f64, temperature: f64) -> f64 {
    photons::stefan_boltzmann_power(emissivity as f32, area as f32, temperature as f32) as f64
}

#[rune::function]
fn planck_spectral_radiance(wavelength: f64, temperature: f64) -> f64 {
    photons::planck_spectral_radiance(wavelength as f32, temperature as f32) as f64
}

#[rune::function]
fn beer_lambert_transmission(incident: f64, absorption_coeff: f64, path_length: f64) -> f64 {
    photons::beer_lambert_transmission(incident as f32, absorption_coeff as f32, path_length as f32)
        as f64
}

#[rune::function]
fn ev_to_joules(ev: f64) -> f64 {
    photons::ev_to_joules(ev as f32) as f64
}

#[rune::function]
fn joules_to_ev(j: f64) -> f64 {
    photons::joules_to_ev(j as f32) as f64
}

/// Build the `eustress::realism::optics` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "optics"])?;
    // geometric
    m.function_meta(thin_lens_image_distance)?;
    m.function_meta(thin_lens_focal_length)?;
    m.function_meta(magnification)?;
    m.function_meta(lensmaker_focal_length)?;
    m.function_meta(mirror_image_distance)?;
    m.function_meta(mirror_focal_length)?;
    m.function_meta(fresnel_reflectance_normal)?;
    m.function_meta(fresnel_transmittance_normal)?;
    m.function_meta(brewster_angle)?;
    m.function_meta(snells_window_radius)?;
    // wave
    m.function_meta(double_slit_fringe_spacing)?;
    m.function_meta(double_slit_angle)?;
    m.function_meta(single_slit_minima_angle)?;
    m.function_meta(diffraction_grating_angle)?;
    m.function_meta(thin_film_constructive_wavelength)?;
    m.function_meta(rayleigh_resolution_angle)?;
    m.function_meta(bragg_angle)?;
    m.function_meta(optical_path_difference)?;
    m.function_meta(michelson_fringe_shift)?;
    // photons
    m.function_meta(photon_energy)?;
    m.function_meta(photon_energy_from_wavelength)?;
    m.function_meta(photon_momentum)?;
    m.function_meta(photoelectric_max_ke)?;
    m.function_meta(threshold_frequency)?;
    m.function_meta(compton_wavelength_shift)?;
    m.function_meta(wien_peak_wavelength)?;
    m.function_meta(stefan_boltzmann_power)?;
    m.function_meta(planck_spectral_radiance)?;
    m.function_meta(beer_lambert_transmission)?;
    m.function_meta(ev_to_joules)?;
    m.function_meta(joules_to_ev)?;
    Ok(m)
}
