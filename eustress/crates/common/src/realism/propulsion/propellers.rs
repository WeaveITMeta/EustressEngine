//! Propeller and rotor aerodynamics — coefficient method.

/// Thrust coefficient: Ct = T / (rho * n^2 * D^4), with n in revolutions/sec.
pub fn thrust_coefficient(thrust: f32, density: f32, rps: f32, diameter: f32) -> f32 {
    thrust / (density * rps * rps * diameter.powi(4))
}

/// Power coefficient: Cp = P / (rho * n^3 * D^5).
pub fn power_coefficient(power: f32, density: f32, rps: f32, diameter: f32) -> f32 {
    power / (density * rps.powi(3) * diameter.powi(5))
}

/// Advance ratio: J = V / (n * D).
pub fn advance_ratio(velocity: f32, rps: f32, diameter: f32) -> f32 {
    velocity / (rps * diameter)
}

/// Propeller efficiency: eta = Ct * J / Cp.
pub fn propeller_efficiency(thrust_coeff: f32, power_coeff: f32, advance_ratio: f32) -> f32 {
    thrust_coeff * advance_ratio / power_coeff
}

/// Recover thrust from its coefficient: T = Ct * rho * n^2 * D^4.
pub fn thrust_from_coefficient(ct: f32, density: f32, rps: f32, diameter: f32) -> f32 {
    ct * density * rps * rps * diameter.powi(4)
}

/// Recover power from its coefficient: P = Cp * rho * n^3 * D^5.
pub fn power_from_coefficient(cp: f32, density: f32, rps: f32, diameter: f32) -> f32 {
    cp * density * rps.powi(3) * diameter.powi(5)
}

/// Actuator-disk induced velocity in hover: vi = sqrt(T / (2 * rho * A)).
pub fn disk_actuator_induced_velocity(thrust: f32, density: f32, disk_area: f32) -> f32 {
    (thrust / (2.0 * density * disk_area)).sqrt()
}

/// Ideal (induced) hover power: P_ideal = T * sqrt(T / (2 * rho * A)).
pub fn ideal_hover_power(thrust: f32, density: f32, disk_area: f32) -> f32 {
    thrust * (thrust / (2.0 * density * disk_area)).sqrt()
}

/// Figure of merit: ideal hover power divided by actual power.
pub fn figure_of_merit(ideal_power: f32, actual_power: f32) -> f32 {
    ideal_power / actual_power
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficient_round_trip() {
        // Ct from T must reconstruct T exactly.
        let (rho, n, d) = (1.225, 30.0, 1.5);
        let thrust = 800.0;
        let ct = thrust_coefficient(thrust, rho, n, d);
        let recovered = thrust_from_coefficient(ct, rho, n, d);
        assert!((recovered - thrust).abs() < 1e-1);
    }

    #[test]
    fn hover_power_matches_induced_velocity() {
        // P_ideal should equal T * vi by construction.
        let (rho, area, thrust) = (1.225, 7.0, 1000.0);
        let vi = disk_actuator_induced_velocity(thrust, rho, area);
        let p = ideal_hover_power(thrust, rho, area);
        assert!((p - thrust * vi).abs() < 1e-1);
    }
}
