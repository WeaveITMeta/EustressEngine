//! Rocket propulsion — Tsiolkovsky, specific impulse, de Laval nozzle.

const G0: f32 = 9.80665;

/// Tsiolkovsky rocket equation: delta-v = ve * ln(mi / mf).
pub fn tsiolkovsky_delta_v(exhaust_velocity: f32, mass_initial: f32, mass_final: f32) -> f32 {
    exhaust_velocity * (mass_initial / mass_final).ln()
}

/// Delta-v expressed via specific impulse: Isp * g0 * ln(mi / mf).
pub fn delta_v_from_isp(isp: f32, mass_initial: f32, mass_final: f32) -> f32 {
    isp * G0 * (mass_initial / mass_final).ln()
}

/// Specific impulse from thrust and mass flow rate: Isp = F / (m_dot * g0).
pub fn specific_impulse(thrust: f32, mass_flow_rate: f32) -> f32 {
    thrust / (mass_flow_rate * G0)
}

/// Effective exhaust velocity from specific impulse: ve = Isp * g0.
pub fn exhaust_velocity_from_isp(isp: f32) -> f32 {
    isp * G0
}

/// Mass ratio required for a given delta-v: mi/mf = exp(dv / ve).
pub fn mass_ratio(delta_v: f32, exhaust_velocity: f32) -> f32 {
    (delta_v / exhaust_velocity).exp()
}

/// Propellant mass fraction for a given delta-v: 1 - exp(-dv / ve).
pub fn propellant_mass_fraction(delta_v: f32, exhaust_velocity: f32) -> f32 {
    1.0 - (-delta_v / exhaust_velocity).exp()
}

/// Total rocket thrust including pressure term:
/// F = m_dot * ve + (Pe - Pa) * Ae.
pub fn thrust(
    mass_flow_rate: f32,
    exhaust_velocity: f32,
    p_exit: f32,
    p_ambient: f32,
    area_exit: f32,
) -> f32 {
    mass_flow_rate * exhaust_velocity + (p_exit - p_ambient) * area_exit
}

/// Ideal nozzle exit velocity (isentropic expansion):
/// ve = sqrt( (2*gamma/(gamma-1)) * R * Tc * (1 - (Pe/Pc)^((gamma-1)/gamma)) ).
pub fn nozzle_exit_velocity(
    t_chamber: f32,
    gamma: f32,
    r_specific: f32,
    p_exit: f32,
    p_chamber: f32,
) -> f32 {
    let exponent = (gamma - 1.0) / gamma;
    let pressure_term = 1.0 - (p_exit / p_chamber).powf(exponent);
    let coefficient = (2.0 * gamma / (gamma - 1.0)) * r_specific * t_chamber;
    (coefficient * pressure_term).sqrt()
}

/// Nozzle area ratio (Ae/At) as a function of exit Mach number:
/// (1/Me) * [ (2/(g+1)) * (1 + (g-1)/2 * Me^2) ]^((g+1)/(2(g-1))).
pub fn area_ratio(mach_exit: f32, gamma: f32) -> f32 {
    let term = (2.0 / (gamma + 1.0)) * (1.0 + 0.5 * (gamma - 1.0) * mach_exit * mach_exit);
    let exponent = (gamma + 1.0) / (2.0 * (gamma - 1.0));
    (1.0 / mach_exit) * term.powf(exponent)
}

/// Characteristic velocity c*:
/// c* = sqrt(R * Tc) / (gamma * sqrt( (2/(g+1))^((g+1)/(g-1)) )).
pub fn characteristic_velocity(t_chamber: f32, gamma: f32, r_specific: f32) -> f32 {
    let exponent = (gamma + 1.0) / (gamma - 1.0);
    let denom = gamma * (2.0 / (gamma + 1.0)).powf(exponent).sqrt();
    (r_specific * t_chamber).sqrt() / denom
}

/// Total delta-v for a multistage vehicle: sum over stages of Isp_i * g0 * ln(MR_i).
/// Sums over the shorter of the two slices so mismatched inputs do not panic.
pub fn multistage_delta_v(stage_isps: &[f32], mass_ratios: &[f32]) -> f32 {
    stage_isps
        .iter()
        .zip(mass_ratios.iter())
        .map(|(&isp, &mr)| isp * G0 * mr.ln())
        .sum()
}

/// Thrust-to-weight ratio: F / (m * g0).
pub fn thrust_to_weight(thrust: f32, mass: f32) -> f32 {
    thrust / (mass * G0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsiolkovsky_basic_case() {
        // ve = 1000 m/s, mass ratio e:1 -> delta-v == ve.
        let dv = tsiolkovsky_delta_v(1000.0, core::f32::consts::E, 1.0);
        assert!((dv - 1000.0).abs() < 1e-1);
    }

    #[test]
    fn isp_round_trip() {
        // ve from Isp, then Isp back from thrust/mdot must agree.
        let isp = 300.0;
        let ve = exhaust_velocity_from_isp(isp);
        let mdot = 5.0;
        let thrust = mdot * ve;
        let recovered = specific_impulse(thrust, mdot);
        assert!((recovered - isp).abs() < 1e-2);
    }

    #[test]
    fn mass_ratio_and_fraction_consistent() {
        let dv = 4000.0;
        let ve = 3000.0;
        let mr = mass_ratio(dv, ve);
        let frac = propellant_mass_fraction(dv, ve);
        // Propellant fraction equals 1 - 1/mass_ratio.
        assert!((frac - (1.0 - 1.0 / mr)).abs() < 1e-4);
        assert!(mr > 1.0);
    }

    #[test]
    fn multistage_sums_stages() {
        let isps = [300.0, 350.0];
        let ratios = [2.0, 2.0];
        let total = multistage_delta_v(&isps, &ratios);
        let manual = 300.0 * G0 * 2.0_f32.ln() + 350.0 * G0 * 2.0_f32.ln();
        assert!((total - manual).abs() < 1e-1);
    }
}
