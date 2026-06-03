//! Rune bindings for the classical (Newtonian) mechanics laws.
//!
//! Exposed to scripts under `eustress::realism::mechanics::*`. Each binding is a
//! thin f64 wrapper around the f32 kernel law in
//! `crate::realism::laws::mechanics`, because Rune works in f64 while the realism
//! kernel is f32.
//!
//! Only the all-scalar-parameter, scalar-returning laws are bound here. The many
//! vector-valued laws (force, momentum, torque, collision resolution, force/field
//! vectors, etc.) take or return `Vec3` and are intentionally not exposed.

use rune::{ContextError, Module};
use crate::realism::laws::mechanics as mech;

// ----------------------------------------------------------------------------
// Kinematics
// ----------------------------------------------------------------------------

#[rune::function]
fn velocity_from_displacement(initial_velocity: f64, acceleration: f64, displacement: f64) -> f64 {
    mech::velocity_from_displacement(initial_velocity as f32, acceleration as f32, displacement as f32) as f64
}

// ----------------------------------------------------------------------------
// Energy
// ----------------------------------------------------------------------------

#[rune::function]
fn kinetic_energy_scalar(mass: f64, speed: f64) -> f64 {
    mech::kinetic_energy_scalar(mass as f32, speed as f32) as f64
}

#[rune::function]
fn gravitational_potential_energy(mass: f64, gravity: f64, height: f64) -> f64 {
    mech::gravitational_potential_energy(mass as f32, gravity as f32, height as f32) as f64
}

#[rune::function]
fn elastic_potential_energy(spring_constant: f64, displacement: f64) -> f64 {
    mech::elastic_potential_energy(spring_constant as f32, displacement as f32) as f64
}

#[rune::function]
fn work_at_angle(force_magnitude: f64, displacement: f64, angle_radians: f64) -> f64 {
    mech::work_at_angle(force_magnitude as f32, displacement as f32, angle_radians as f32) as f64
}

#[rune::function]
fn power_from_work(work: f64, time: f64) -> f64 {
    mech::power_from_work(work as f32, time as f32) as f64
}

// ----------------------------------------------------------------------------
// Gravity
// ----------------------------------------------------------------------------

#[rune::function]
fn gravitational_force(m1: f64, m2: f64, distance: f64) -> f64 {
    mech::gravitational_force(m1 as f32, m2 as f32, distance as f32) as f64
}

#[rune::function]
fn surface_gravity(mass: f64, radius: f64) -> f64 {
    mech::surface_gravity(mass as f32, radius as f32) as f64
}

#[rune::function]
fn escape_velocity(mass: f64, radius: f64) -> f64 {
    mech::escape_velocity(mass as f32, radius as f32) as f64
}

#[rune::function]
fn orbital_velocity(central_mass: f64, orbital_radius: f64) -> f64 {
    mech::orbital_velocity(central_mass as f32, orbital_radius as f32) as f64
}

#[rune::function]
fn orbital_period(central_mass: f64, semi_major_axis: f64) -> f64 {
    mech::orbital_period(central_mass as f32, semi_major_axis as f32) as f64
}

// ----------------------------------------------------------------------------
// Friction
// ----------------------------------------------------------------------------

#[rune::function]
fn static_friction_max(coefficient: f64, normal_force: f64) -> f64 {
    mech::static_friction_max(coefficient as f32, normal_force as f32) as f64
}

#[rune::function]
fn kinetic_friction(coefficient: f64, normal_force: f64) -> f64 {
    mech::kinetic_friction(coefficient as f32, normal_force as f32) as f64
}

// ----------------------------------------------------------------------------
// Spring forces
// ----------------------------------------------------------------------------

#[rune::function]
fn spring_force(spring_constant: f64, displacement: f64) -> f64 {
    mech::spring_force(spring_constant as f32, displacement as f32) as f64
}

#[rune::function]
fn damped_spring_force(spring_constant: f64, damping: f64, displacement: f64, velocity: f64) -> f64 {
    mech::damped_spring_force(spring_constant as f32, damping as f32, displacement as f32, velocity as f32) as f64
}

// ----------------------------------------------------------------------------
// Moment of inertia (scalar shapes)
// ----------------------------------------------------------------------------

#[rune::function]
fn moment_of_inertia_solid_sphere(mass: f64, radius: f64) -> f64 {
    mech::moment_of_inertia::solid_sphere(mass as f32, radius as f32) as f64
}

#[rune::function]
fn moment_of_inertia_hollow_sphere(mass: f64, radius: f64) -> f64 {
    mech::moment_of_inertia::hollow_sphere(mass as f32, radius as f32) as f64
}

#[rune::function]
fn moment_of_inertia_solid_cylinder(mass: f64, radius: f64) -> f64 {
    mech::moment_of_inertia::solid_cylinder(mass as f32, radius as f32) as f64
}

#[rune::function]
fn moment_of_inertia_hollow_cylinder(mass: f64, radius: f64) -> f64 {
    mech::moment_of_inertia::hollow_cylinder(mass as f32, radius as f32) as f64
}

#[rune::function]
fn moment_of_inertia_solid_rod_center(mass: f64, length: f64) -> f64 {
    mech::moment_of_inertia::solid_rod_center(mass as f32, length as f32) as f64
}

#[rune::function]
fn moment_of_inertia_solid_rod_end(mass: f64, length: f64) -> f64 {
    mech::moment_of_inertia::solid_rod_end(mass as f32, length as f32) as f64
}

#[rune::function]
fn moment_of_inertia_rectangular_plate(mass: f64, width: f64, height: f64) -> f64 {
    mech::moment_of_inertia::rectangular_plate(mass as f32, width as f32, height as f32) as f64
}

/// Build the `eustress::realism::mechanics` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "mechanics"])?;
    m.function_meta(velocity_from_displacement)?;
    m.function_meta(kinetic_energy_scalar)?;
    m.function_meta(gravitational_potential_energy)?;
    m.function_meta(elastic_potential_energy)?;
    m.function_meta(work_at_angle)?;
    m.function_meta(power_from_work)?;
    m.function_meta(gravitational_force)?;
    m.function_meta(surface_gravity)?;
    m.function_meta(escape_velocity)?;
    m.function_meta(orbital_velocity)?;
    m.function_meta(orbital_period)?;
    m.function_meta(static_friction_max)?;
    m.function_meta(kinetic_friction)?;
    m.function_meta(spring_force)?;
    m.function_meta(damped_spring_force)?;
    m.function_meta(moment_of_inertia_solid_sphere)?;
    m.function_meta(moment_of_inertia_hollow_sphere)?;
    m.function_meta(moment_of_inertia_solid_cylinder)?;
    m.function_meta(moment_of_inertia_hollow_cylinder)?;
    m.function_meta(moment_of_inertia_solid_rod_center)?;
    m.function_meta(moment_of_inertia_solid_rod_end)?;
    m.function_meta(moment_of_inertia_rectangular_plate)?;
    Ok(m)
}
