//! Heat exchanger analysis — LMTD and NTU-effectiveness.

/// Log mean temperature difference for a two-stream heat exchanger.
/// LMTD = (dt1 - dt2) / ln(dt1 / dt2). When the two terminal differences
/// are equal the expression is indeterminate and the LMTD equals that
/// common value, which this function returns directly.
pub fn lmtd(delta_t1: f32, delta_t2: f32) -> f32 {
    if (delta_t1 - delta_t2).abs() < 1e-6 {
        return delta_t1;
    }
    if delta_t1 <= 0.0 || delta_t2 <= 0.0 {
        return 0.0;
    }
    (delta_t1 - delta_t2) / (delta_t1 / delta_t2).ln()
}

/// Heat transfer rate via the LMTD method. Q = U * A * LMTD.
pub fn heat_transfer_lmtd(u: f32, area: f32, lmtd: f32) -> f32 {
    u * area * lmtd
}

/// Surface area required to transfer a given heat rate. A = Q / (U * LMTD).
pub fn required_area(q: f32, u: f32, lmtd: f32) -> f32 {
    let denom = u * lmtd;
    if denom == 0.0 {
        return 0.0;
    }
    q / denom
}

/// Number of transfer units. NTU = UA / C_min.
pub fn ntu(ua: f32, c_min: f32) -> f32 {
    if c_min == 0.0 {
        return 0.0;
    }
    ua / c_min
}

/// Stream capacity rate. C = m_dot * cp.
pub fn capacity_rate(mass_flow: f32, cp: f32) -> f32 {
    mass_flow * cp
}

/// Effectiveness of a parallel-flow heat exchanger.
/// eps = (1 - exp(-NTU (1 + Cr))) / (1 + Cr), where Cr = C_min / C_max.
pub fn effectiveness_parallel_flow(ntu: f32, c_ratio: f32) -> f32 {
    let denom = 1.0 + c_ratio;
    if denom == 0.0 {
        return 0.0;
    }
    (1.0 - (-ntu * denom).exp()) / denom
}

/// Effectiveness of a counter-flow heat exchanger.
/// For Cr = 1: eps = NTU / (1 + NTU).
/// Otherwise: eps = (1 - exp(-NTU (1 - Cr)))
///                 / (1 - Cr * exp(-NTU (1 - Cr))).
pub fn effectiveness_counter_flow(ntu: f32, c_ratio: f32) -> f32 {
    if (c_ratio - 1.0).abs() < 1e-6 {
        return ntu / (1.0 + ntu);
    }
    let exp_term = (-ntu * (1.0 - c_ratio)).exp();
    let denom = 1.0 - c_ratio * exp_term;
    if denom == 0.0 {
        return 0.0;
    }
    (1.0 - exp_term) / denom
}

/// Actual heat transfer from effectiveness and inlet temperatures.
/// Q = eps * C_min * (T_hot_in - T_cold_in).
pub fn effectiveness_to_heat(
    effectiveness: f32,
    c_min: f32,
    t_hot_in: f32,
    t_cold_in: f32,
) -> f32 {
    effectiveness * c_min * (t_hot_in - t_cold_in)
}

/// Maximum thermodynamically possible heat transfer.
/// Q_max = C_min * (T_hot_in - T_cold_in).
pub fn max_possible_heat(c_min: f32, t_hot_in: f32, t_cold_in: f32) -> f32 {
    c_min * (t_hot_in - t_cold_in)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lmtd_equal_terminal_differences() {
        // Indeterminate form resolves to the common terminal difference.
        assert!((lmtd(20.0, 20.0) - 20.0).abs() < 1e-4);
        // dt1=30, dt2=10 => (30-10)/ln(3) ~ 18.205.
        assert!((lmtd(30.0, 10.0) - 18.205).abs() < 1e-2);
    }

    #[test]
    fn counter_flow_balanced_capacity() {
        // Cr=1, NTU=2 => 2/3 ~ 0.6667.
        let eps = effectiveness_counter_flow(2.0, 1.0);
        assert!((eps - 0.66667).abs() < 1e-3);
    }
}
