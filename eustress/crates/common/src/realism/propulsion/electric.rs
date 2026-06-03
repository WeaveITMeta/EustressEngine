//! Electric propulsion — ion and Hall thrusters.

const G0: f32 = 9.80665;
// Used by the unit tests below; kept as a named physical constant per the
// propulsion spec even though no public function consumes it directly.
#[allow(dead_code)]
const ELEMENTARY_CHARGE: f32 = 1.602_176_634e-19;

/// Ion exhaust velocity from accelerating voltage and charge-to-mass ratio:
/// ve = sqrt(2 * (q/m) * V).
pub fn ion_exhaust_velocity(accel_voltage: f32, charge_to_mass: f32) -> f32 {
    (2.0 * charge_to_mass * accel_voltage).sqrt()
}

/// Ion thrust: F = m_dot * ve.
pub fn ion_thrust(mass_flow: f32, exhaust_velocity: f32) -> f32 {
    mass_flow * exhaust_velocity
}

/// Thrust from beam current: F = I_b * sqrt(2 * m_ion * V / q).
pub fn thrust_from_beam_current(
    beam_current: f32,
    accel_voltage: f32,
    ion_mass: f32,
    charge: f32,
) -> f32 {
    beam_current * (2.0 * ion_mass * accel_voltage / charge).sqrt()
}

/// Jet (kinetic) power of the exhaust beam: P_jet = T * ve / 2.
pub fn thrust_power(thrust: f32, exhaust_velocity: f32) -> f32 {
    thrust * exhaust_velocity / 2.0
}

/// Thrust-to-power ratio: T / P_in.
pub fn thrust_to_power_ratio(thrust: f32, input_power: f32) -> f32 {
    thrust / input_power
}

/// Total thruster efficiency: (T * ve / 2) / P_in.
pub fn total_efficiency_electric(thrust: f32, exhaust_velocity: f32, input_power: f32) -> f32 {
    thrust_power(thrust, exhaust_velocity) / input_power
}

/// Specific impulse of an electric thruster: Isp = ve / g0.
pub fn specific_impulse_electric(exhaust_velocity: f32) -> f32 {
    exhaust_velocity / G0
}

/// Propellant (mass) utilization efficiency. The beam current implies an ion
/// mass flow of I_b * m_ion / q; dividing by the actual propellant mass flow
/// gives the fraction usefully ionized and accelerated.
pub fn propellant_utilization(
    beam_current: f32,
    mass_flow: f32,
    charge: f32,
    ion_mass: f32,
) -> f32 {
    let beam_mass_flow = beam_current * ion_mass / charge;
    beam_mass_flow / mass_flow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaust_velocity_and_isp() {
        // q/m for a singly-charged xenon ion, accelerated through 1000 V.
        let xenon_mass = 2.18e-25; // kg, approx Xe atomic mass
        let charge_to_mass = ELEMENTARY_CHARGE / xenon_mass;
        let ve = ion_exhaust_velocity(1000.0, charge_to_mass);
        // Should land in the tens of km/s range typical of ion drives.
        assert!(ve > 20_000.0 && ve < 60_000.0);
        let isp = specific_impulse_electric(ve);
        assert!((isp - ve / G0).abs() < 1e-1);
    }

    #[test]
    fn efficiency_is_jet_over_input() {
        // With input power exactly equal to jet power, efficiency is 1.0.
        let thrust = 0.1;
        let ve = 30_000.0;
        let jet = thrust_power(thrust, ve);
        let eta = total_efficiency_electric(thrust, ve, jet);
        assert!((eta - 1.0).abs() < 1e-4);
        // Half-efficient case.
        let eta2 = total_efficiency_electric(thrust, ve, jet * 2.0);
        assert!((eta2 - 0.5).abs() < 1e-4);
    }
}
