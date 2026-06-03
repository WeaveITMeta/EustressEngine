//! Air-breathing jet propulsion — turbojet, turbofan.

/// Ideal turbojet thrust (cold-flow approximation): F = m_dot * (ve - v0).
pub fn turbojet_thrust(mass_flow: f32, v_exhaust: f32, v_inlet: f32) -> f32 {
    mass_flow * (v_exhaust - v_inlet)
}

/// Thrust accounting for added fuel mass:
/// F = (m_air + m_fuel) * ve - m_air * v0.
pub fn thrust_with_fuel(air_flow: f32, fuel_flow: f32, v_exhaust: f32, v_inlet: f32) -> f32 {
    (air_flow + fuel_flow) * v_exhaust - air_flow * v_inlet
}

/// Specific thrust: thrust per unit air mass flow.
pub fn specific_thrust(thrust: f32, air_mass_flow: f32) -> f32 {
    thrust / air_mass_flow
}

/// Thrust-specific fuel consumption: fuel flow per unit thrust.
pub fn tsfc(fuel_flow: f32, thrust: f32) -> f32 {
    fuel_flow / thrust
}

/// Propulsive (Froude) efficiency: 2 * v0 / (ve + v0).
pub fn propulsive_efficiency(v_exhaust: f32, v_flight: f32) -> f32 {
    2.0 * v_flight / (v_exhaust + v_flight)
}

/// Thermal efficiency of the jet core: kinetic-energy gain divided by fuel energy.
/// Per unit air mass: (0.5*(ve^2 - v0^2)) / (f * Q), with f the fuel-air ratio.
pub fn thermal_efficiency_jet(
    v_exhaust: f32,
    v_flight: f32,
    fuel_heating_value: f32,
    fuel_air_ratio: f32,
) -> f32 {
    let ke_gain = 0.5 * (v_exhaust * v_exhaust - v_flight * v_flight);
    let fuel_energy = fuel_air_ratio * fuel_heating_value;
    ke_gain / fuel_energy
}

/// Overall efficiency: product of propulsive and thermal efficiencies.
pub fn overall_efficiency(propulsive: f32, thermal: f32) -> f32 {
    propulsive * thermal
}

/// Turbofan thrust from separate core and fan (bypass) streams:
/// F = m_core*(v_core - v0) + m_fan*(v_fan - v0).
pub fn bypass_thrust(
    core_flow: f32,
    fan_flow: f32,
    v_core: f32,
    v_fan: f32,
    v_inlet: f32,
) -> f32 {
    core_flow * (v_core - v_inlet) + fan_flow * (v_fan - v_inlet)
}

/// Bypass ratio: fan (bypass) mass flow divided by core mass flow.
pub fn bypass_ratio(fan_flow: f32, core_flow: f32) -> f32 {
    fan_flow / core_flow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turbojet_thrust_basic() {
        // 50 kg/s accelerated from 200 to 600 m/s -> 20 kN.
        let f = turbojet_thrust(50.0, 600.0, 200.0);
        assert!((f - 20_000.0).abs() < 1e-1);
    }

    #[test]
    fn propulsive_efficiency_bounds() {
        // Matched exhaust and flight speed -> efficiency 1.0.
        let eta = propulsive_efficiency(300.0, 300.0);
        assert!((eta - 1.0).abs() < 1e-4);
        // Faster exhaust than flight -> efficiency below 1.
        let eta2 = propulsive_efficiency(600.0, 200.0);
        assert!(eta2 > 0.0 && eta2 < 1.0);
    }
}
