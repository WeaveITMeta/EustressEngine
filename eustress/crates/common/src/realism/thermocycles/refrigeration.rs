//! Vapor-compression refrigeration and heat pumps.

/// Coefficient of performance of a refrigerator — useful cooling per unit
/// of compressor work. COP_R = Q_cold / W.
pub fn cop_refrigerator(q_cold: f32, w_compressor: f32) -> f32 {
    if w_compressor == 0.0 {
        return 0.0;
    }
    q_cold / w_compressor
}

/// Coefficient of performance of a heat pump — useful heating per unit of
/// compressor work. COP_HP = Q_hot / W.
pub fn cop_heat_pump(q_hot: f32, w_compressor: f32) -> f32 {
    if w_compressor == 0.0 {
        return 0.0;
    }
    q_hot / w_compressor
}

/// Maximum (Carnot) COP for a refrigerator between two reservoirs (Kelvin).
/// COP_R,max = T_cold / (T_hot - T_cold).
pub fn carnot_cop_refrigerator(t_cold: f32, t_hot: f32) -> f32 {
    let span = t_hot - t_cold;
    if span == 0.0 {
        return 0.0;
    }
    t_cold / span
}

/// Maximum (Carnot) COP for a heat pump between two reservoirs (Kelvin).
/// COP_HP,max = T_hot / (T_hot - T_cold).
pub fn carnot_cop_heat_pump(t_cold: f32, t_hot: f32) -> f32 {
    let span = t_hot - t_cold;
    if span == 0.0 {
        return 0.0;
    }
    t_hot / span
}

/// Refrigeration effect — specific heat absorbed in the evaporator (kJ/kg).
/// q_L = h_evap_out - h_evap_in.
pub fn refrigeration_effect(h_evap_out: f32, h_evap_in: f32) -> f32 {
    h_evap_out - h_evap_in
}

/// Specific compressor work for the vapor-compression cycle (kJ/kg).
/// w = h_comp_out - h_comp_in.
pub fn compressor_work_vc(h_comp_out: f32, h_comp_in: f32) -> f32 {
    h_comp_out - h_comp_in
}

/// Refrigerant mass flow rate required to meet a cooling load.
/// m_dot = cooling_capacity / refrigeration_effect (kg/s when capacity is
/// kW and effect is kJ/kg).
pub fn mass_flow_rate_refrigerant(cooling_capacity: f32, refrigeration_effect: f32) -> f32 {
    if refrigeration_effect == 0.0 {
        return 0.0;
    }
    cooling_capacity / refrigeration_effect
}

/// Relationship between the two coefficients of performance for the same
/// machine. COP_HP = COP_R + 1.
pub fn cop_relation(cop_refrigerator: f32) -> f32 {
    cop_refrigerator + 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carnot_cops_are_consistent() {
        // T_cold=263.15K, T_hot=303.15K, span=40K.
        let cop_r = carnot_cop_refrigerator(263.15, 303.15);
        let cop_hp = carnot_cop_heat_pump(263.15, 303.15);
        assert!((cop_r - 6.57875).abs() < 1e-3);
        // The heat-pump COP must exceed the refrigerator COP by exactly 1.
        assert!((cop_hp - (cop_r + 1.0)).abs() < 1e-3);
    }

    #[test]
    fn effect_and_flow() {
        // Effect 240 - 60 = 180 kJ/kg; 18 kW load => 0.1 kg/s.
        let effect = refrigeration_effect(240.0, 60.0);
        assert!((effect - 180.0).abs() < 1e-3);
        let m_dot = mass_flow_rate_refrigerant(18.0, effect);
        assert!((m_dot - 0.1).abs() < 1e-4);
    }
}
