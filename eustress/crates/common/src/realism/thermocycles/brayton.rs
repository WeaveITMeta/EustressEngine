//! Brayton gas-turbine cycle.

const GAMMA_AIR: f32 = 1.4;

/// Air-standard Brayton cycle thermal efficiency as a function of the
/// compressor pressure ratio. eta = 1 - r^((1 - gamma) / gamma).
pub fn brayton_efficiency(pressure_ratio: f32, gamma: f32) -> f32 {
    if pressure_ratio <= 0.0 {
        return 0.0;
    }
    let exponent = (1.0 - gamma) / gamma;
    1.0 - pressure_ratio.powf(exponent)
}

/// Temperature after isentropic compression (Kelvin).
/// T2 = T1 * r^((gamma - 1) / gamma).
pub fn temperature_after_compression(t_inlet: f32, pressure_ratio: f32, gamma: f32) -> f32 {
    if pressure_ratio <= 0.0 {
        return t_inlet;
    }
    let exponent = (gamma - 1.0) / gamma;
    t_inlet * pressure_ratio.powf(exponent)
}

/// Temperature after isentropic expansion through the turbine (Kelvin).
/// The pressure ratio across the turbine equals the compressor ratio for
/// an ideal cycle, so T4 = T3 / r^((gamma - 1) / gamma).
pub fn temperature_after_expansion(t_inlet: f32, pressure_ratio: f32, gamma: f32) -> f32 {
    if pressure_ratio <= 0.0 {
        return t_inlet;
    }
    let exponent = (gamma - 1.0) / gamma;
    t_inlet / pressure_ratio.powf(exponent)
}

/// Specific compressor work (kJ/kg when cp is kJ/(kg.K)).
/// w_c = cp * (T2 - T1).
pub fn compressor_work(cp: f32, t1: f32, t2: f32) -> f32 {
    cp * (t2 - t1)
}

/// Specific turbine work (kJ/kg). w_t = cp * (T3 - T4).
pub fn turbine_work(cp: f32, t3: f32, t4: f32) -> f32 {
    cp * (t3 - t4)
}

/// Net specific work of the cycle. w_net = w_turbine - w_compressor.
pub fn net_specific_work(turbine_work: f32, compressor_work: f32) -> f32 {
    turbine_work - compressor_work
}

/// Pressure ratio that maximizes net specific work for given temperature
/// limits. r_opt = (T_max / T_min)^(gamma / (2 (gamma - 1))).
pub fn optimal_pressure_ratio(t_max: f32, t_min: f32, gamma: f32) -> f32 {
    if t_min <= 0.0 || gamma <= 1.0 {
        return 1.0;
    }
    let exponent = gamma / (2.0 * (gamma - 1.0));
    (t_max / t_min).powf(exponent)
}

/// Isentropic compressor efficiency — ideal temperature rise over actual.
/// eta = (T2_ideal - T1) / (T2_actual - T1).
pub fn isentropic_compressor_efficiency(t1: f32, t2_ideal: f32, t2_actual: f32) -> f32 {
    let actual = t2_actual - t1;
    if actual == 0.0 {
        return 0.0;
    }
    (t2_ideal - t1) / actual
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efficiency_pressure_ratio_10() {
        // 1 - 10^(-0.4/1.4) = 1 - 10^(-0.285714) ~ 0.4821.
        let eta = brayton_efficiency(10.0, GAMMA_AIR);
        assert!((eta - 0.4821).abs() < 1e-3);
    }

    #[test]
    fn compression_temperature_rise() {
        // T1=300K, r=10 => T2 = 300 * 10^(0.285714) ~ 579.2 K.
        let t2 = temperature_after_compression(300.0, 10.0, GAMMA_AIR);
        assert!((t2 - 579.2).abs() < 0.5);
    }
}
