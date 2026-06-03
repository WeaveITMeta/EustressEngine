//! Rankine steam power cycle.

/// Carnot efficiency — the thermodynamic ceiling for a cycle operating
/// between two reservoirs. Temperatures in Kelvin.
/// eta = 1 - T_cold / T_hot.
pub fn carnot_efficiency(t_hot: f32, t_cold: f32) -> f32 {
    if t_hot <= 0.0 {
        return 0.0;
    }
    1.0 - (t_cold / t_hot)
}

/// Ideal Rankine cycle thermal efficiency from the four key enthalpies
/// (kJ/kg). Net work = turbine work - pump work; heat in = boiler heat.
/// eta = (w_turbine - w_pump) / q_in.
pub fn rankine_ideal_efficiency(
    h_turbine_in: f32,
    h_turbine_out: f32,
    h_pump_in: f32,
    h_pump_out: f32,
) -> f32 {
    let w_turbine = h_turbine_in - h_turbine_out;
    let w_pump = h_pump_out - h_pump_in;
    let q_in = h_turbine_in - h_pump_out;
    if q_in == 0.0 {
        return 0.0;
    }
    (w_turbine - w_pump) / q_in
}

/// Pump work per unit mass for an incompressible liquid (kJ/kg when
/// v_specific is m^3/kg and pressures are kPa). w = v * (P_high - P_low).
pub fn pump_work(v_specific: f32, p_high: f32, p_low: f32) -> f32 {
    v_specific * (p_high - p_low)
}

/// Specific work extracted by the turbine (kJ/kg). w = h_in - h_out.
pub fn turbine_work(h_in: f32, h_out: f32) -> f32 {
    h_in - h_out
}

/// Heat added in the boiler per unit mass (kJ/kg). q = h_out - h_in.
pub fn heat_added(h_boiler_out: f32, h_boiler_in: f32) -> f32 {
    h_boiler_out - h_boiler_in
}

/// Back work ratio — fraction of turbine output consumed by the pump.
/// bwr = w_pump / w_turbine.
pub fn back_work_ratio(w_pump: f32, w_turbine: f32) -> f32 {
    if w_turbine == 0.0 {
        return 0.0;
    }
    w_pump / w_turbine
}

/// Thermal efficiency of a Rankine cycle with reheat. Two turbine stages
/// share the load; total heat in is boiler heat plus reheat heat.
/// eta = (w_t1 + w_t2 - w_pump) / (q_in + q_reheat).
pub fn reheat_efficiency(
    w_turbine1: f32,
    w_turbine2: f32,
    w_pump: f32,
    q_in: f32,
    q_reheat: f32,
) -> f32 {
    let q_total = q_in + q_reheat;
    if q_total == 0.0 {
        return 0.0;
    }
    (w_turbine1 + w_turbine2 - w_pump) / q_total
}

/// Isentropic (adiabatic) turbine efficiency — ratio of actual work
/// extracted to the ideal isentropic work for the same pressure drop.
/// eta = (h_in - h_out_actual) / (h_in - h_out_ideal).
pub fn isentropic_turbine_efficiency(h_in: f32, h_out_actual: f32, h_out_ideal: f32) -> f32 {
    let ideal = h_in - h_out_ideal;
    if ideal == 0.0 {
        return 0.0;
    }
    (h_in - h_out_actual) / ideal
}

/// Generic thermal efficiency — net work output over heat input.
/// eta = w_net / q_in.
pub fn thermal_efficiency(w_net: f32, q_in: f32) -> f32 {
    if q_in == 0.0 {
        return 0.0;
    }
    w_net / q_in
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carnot_between_800k_and_300k() {
        // 1 - 300/800 = 0.625
        let eta = carnot_efficiency(800.0, 300.0);
        assert!((eta - 0.625).abs() < 1e-4);
    }

    #[test]
    fn turbine_and_pump_work_basics() {
        // Turbine drops 3000 -> 2200 kJ/kg => 800 kJ/kg.
        assert!((turbine_work(3000.0, 2200.0) - 800.0).abs() < 1e-3);
        // Pump: v=0.001 m^3/kg, 8000-10 kPa => ~7.99 kJ/kg.
        let wp = pump_work(0.001, 8000.0, 10.0);
        assert!((wp - 7.99).abs() < 1e-3);
    }
}
