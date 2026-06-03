//! Otto and Diesel internal-combustion cycles.

/// Air-standard Otto cycle thermal efficiency (spark ignition).
/// eta = 1 - r^(1 - gamma), where r is the compression ratio.
pub fn otto_efficiency(compression_ratio: f32, gamma: f32) -> f32 {
    if compression_ratio <= 0.0 {
        return 0.0;
    }
    1.0 - compression_ratio.powf(1.0 - gamma)
}

/// Air-standard Diesel cycle thermal efficiency (compression ignition).
/// eta = 1 - (1 / r^(gamma - 1)) * (rc^gamma - 1) / (gamma * (rc - 1)),
/// where r is the compression ratio and rc is the cutoff ratio.
pub fn diesel_efficiency(compression_ratio: f32, cutoff_ratio: f32, gamma: f32) -> f32 {
    if compression_ratio <= 0.0 {
        return 0.0;
    }
    // As cutoff -> 1 the Diesel cycle reduces to the Otto cycle; the
    // bracketed term tends to 1, so guard the removable singularity.
    let denom = gamma * (cutoff_ratio - 1.0);
    let cutoff_term = if denom.abs() < 1e-6 {
        1.0
    } else {
        (cutoff_ratio.powf(gamma) - 1.0) / denom
    };
    let compression_term = 1.0 / compression_ratio.powf(gamma - 1.0);
    1.0 - compression_term * cutoff_term
}

/// Mean effective pressure — net work spread over the displacement volume.
/// mep = W_net / V_displacement.
pub fn mean_effective_pressure(work_net: f32, displacement_volume: f32) -> f32 {
    if displacement_volume == 0.0 {
        return 0.0;
    }
    work_net / displacement_volume
}

/// Engine power from brake mean effective pressure.
/// P = BMEP * V_d * (rpm / 60) / (strokes_per_cycle / 2).
/// For a 4-stroke engine strokes_per_cycle = 4, dividing revolutions by
/// two because one power stroke occurs every two crankshaft revolutions.
/// For a 2-stroke engine strokes_per_cycle = 2, giving one power stroke
/// per revolution.
pub fn engine_power(bmep: f32, displacement: f32, rpm: f32, strokes_per_cycle: f32) -> f32 {
    if strokes_per_cycle == 0.0 {
        return 0.0;
    }
    let revolutions_per_second = rpm / 60.0;
    bmep * displacement * revolutions_per_second / (strokes_per_cycle / 2.0)
}

/// Compression ratio from cylinder volumes at bottom and top dead center.
/// r = V_bdc / V_tdc.
pub fn compression_ratio_from_volumes(v_bdc: f32, v_tdc: f32) -> f32 {
    if v_tdc == 0.0 {
        return 0.0;
    }
    v_bdc / v_tdc
}

/// Peak (post-combustion) temperature of an Otto cycle (Kelvin).
/// Isentropic compression: T2 = T1 * r^(gamma - 1).
/// Constant-volume heat addition: T3 = T2 + q / cv.
pub fn otto_peak_temperature(
    t_intake: f32,
    compression_ratio: f32,
    gamma: f32,
    heat_added: f32,
    cv: f32,
) -> f32 {
    let t2 = if compression_ratio <= 0.0 {
        t_intake
    } else {
        t_intake * compression_ratio.powf(gamma - 1.0)
    };
    if cv == 0.0 {
        return t2;
    }
    t2 + heat_added / cv
}

/// Volumetric efficiency — actual air mass inducted over the theoretical
/// mass that would fill the displacement at intake conditions.
/// eta_v = m_actual / m_theoretical.
pub fn volumetric_efficiency(actual_air_mass: f32, theoretical_air_mass: f32) -> f32 {
    if theoretical_air_mass == 0.0 {
        return 0.0;
    }
    actual_air_mass / theoretical_air_mass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otto_efficiency_cr8() {
        // 1 - 8^(-0.4) ~ 0.5647.
        let eta = otto_efficiency(8.0, 1.4);
        assert!((eta - 0.565).abs() < 2e-3);
    }

    #[test]
    fn diesel_reduces_toward_otto_at_low_cutoff() {
        // At cutoff -> 1 the Diesel efficiency matches the Otto value.
        let otto = otto_efficiency(18.0, 1.4);
        let diesel = diesel_efficiency(18.0, 1.0000001, 1.4);
        assert!((diesel - otto).abs() < 2e-3);
    }
}
