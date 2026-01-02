//! # Thermodynamics Laws
//!
//! Implementation of thermodynamic laws and equations.
//!
//! ## Table of Contents
//!
//! 1. **Ideal Gas Law** - PV = nRT
//! 2. **First Law** - Energy conservation (ΔU = Q - W)
//! 3. **Second Law** - Entropy (ΔS ≥ Q/T)
//! 4. **Third Law** - Absolute zero entropy
//! 5. **Heat Transfer** - Conduction, convection, radiation
//! 6. **Phase Transitions** - Latent heat, phase diagrams

use bevy::prelude::*;
use crate::realism::constants;

// ============================================================================
// Ideal Gas Law: PV = nRT
// ============================================================================

/// Calculate pressure using ideal gas law: P = nRT/V
/// 
/// # Arguments
/// * `n` - Amount of substance (moles)
/// * `t` - Temperature (Kelvin)
/// * `v` - Volume (m³)
/// 
/// # Returns
/// Pressure in Pascals
#[inline]
pub fn ideal_gas_pressure(n: f32, t: f32, v: f32) -> f32 {
    if v <= 0.0 {
        return f32::INFINITY;
    }
    (n * constants::R_F32 * t) / v
}

/// Calculate volume using ideal gas law: V = nRT/P
#[inline]
pub fn ideal_gas_volume(n: f32, t: f32, p: f32) -> f32 {
    if p <= 0.0 {
        return f32::INFINITY;
    }
    (n * constants::R_F32 * t) / p
}

/// Calculate temperature using ideal gas law: T = PV/(nR)
#[inline]
pub fn ideal_gas_temperature(p: f32, v: f32, n: f32) -> f32 {
    if n <= 0.0 {
        return f32::INFINITY;
    }
    (p * v) / (n * constants::R_F32)
}

/// Calculate moles using ideal gas law: n = PV/(RT)
#[inline]
pub fn ideal_gas_moles(p: f32, v: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return f32::INFINITY;
    }
    (p * v) / (constants::R_F32 * t)
}

// ============================================================================
// Van der Waals Equation (Real Gas)
// ============================================================================

/// Van der Waals equation for real gases: (P + a(n/V)²)(V - nb) = nRT
/// 
/// # Arguments
/// * `n` - Amount of substance (moles)
/// * `t` - Temperature (Kelvin)
/// * `v` - Volume (m³)
/// * `a` - Van der Waals constant a (Pa·m⁶/mol²)
/// * `b` - Van der Waals constant b (m³/mol)
/// 
/// # Returns
/// Pressure in Pascals
pub fn van_der_waals_pressure(n: f32, t: f32, v: f32, a: f32, b: f32) -> f32 {
    let v_eff = v - n * b;
    if v_eff <= 0.0 {
        return f32::INFINITY;
    }
    let n_over_v = n / v;
    (n * constants::R_F32 * t) / v_eff - a * n_over_v * n_over_v
}

/// Common Van der Waals constants (a, b) for gases
pub mod van_der_waals_constants {
    /// Helium (a in Pa·m⁶/mol², b in m³/mol)
    pub const HELIUM: (f32, f32) = (0.0346e-6, 23.7e-6);
    /// Hydrogen
    pub const HYDROGEN: (f32, f32) = (0.0245e-6, 26.6e-6);
    /// Nitrogen
    pub const NITROGEN: (f32, f32) = (0.137e-6, 38.7e-6);
    /// Oxygen
    pub const OXYGEN: (f32, f32) = (0.138e-6, 31.8e-6);
    /// Carbon dioxide
    pub const CO2: (f32, f32) = (0.364e-6, 42.7e-6);
    /// Water vapor
    pub const WATER: (f32, f32) = (0.554e-6, 30.5e-6);
}

// ============================================================================
// First Law of Thermodynamics: ΔU = Q - W
// ============================================================================

/// Calculate change in internal energy: ΔU = Q - W
/// 
/// # Arguments
/// * `heat_in` - Heat added to system (J)
/// * `work_out` - Work done by system (J)
/// 
/// # Returns
/// Change in internal energy (J)
#[inline]
pub fn internal_energy_change(heat_in: f32, work_out: f32) -> f32 {
    heat_in - work_out
}

/// Calculate work done in isobaric (constant pressure) process: W = PΔV
#[inline]
pub fn work_isobaric(pressure: f32, delta_volume: f32) -> f32 {
    pressure * delta_volume
}

/// Calculate work done in isothermal (constant temperature) process: W = nRT·ln(V₂/V₁)
pub fn work_isothermal(n: f32, t: f32, v1: f32, v2: f32) -> f32 {
    if v1 <= 0.0 || v2 <= 0.0 {
        return 0.0;
    }
    n * constants::R_F32 * t * (v2 / v1).ln()
}

/// Calculate work done in adiabatic process: W = (P₁V₁ - P₂V₂)/(γ - 1)
pub fn work_adiabatic(p1: f32, v1: f32, p2: f32, v2: f32, gamma: f32) -> f32 {
    if (gamma - 1.0).abs() < 1e-6 {
        return 0.0;
    }
    (p1 * v1 - p2 * v2) / (gamma - 1.0)
}

// ============================================================================
// Heat Capacity
// ============================================================================

/// Heat capacity at constant volume for ideal monatomic gas: Cv = (3/2)nR
#[inline]
pub fn heat_capacity_monatomic_cv(n: f32) -> f32 {
    1.5 * n * constants::R_F32
}

/// Heat capacity at constant pressure for ideal monatomic gas: Cp = (5/2)nR
#[inline]
pub fn heat_capacity_monatomic_cp(n: f32) -> f32 {
    2.5 * n * constants::R_F32
}

/// Heat capacity at constant volume for ideal diatomic gas: Cv = (5/2)nR
#[inline]
pub fn heat_capacity_diatomic_cv(n: f32) -> f32 {
    2.5 * n * constants::R_F32
}

/// Heat capacity at constant pressure for ideal diatomic gas: Cp = (7/2)nR
#[inline]
pub fn heat_capacity_diatomic_cp(n: f32) -> f32 {
    3.5 * n * constants::R_F32
}

/// Calculate heat required: Q = mcΔT
#[inline]
pub fn heat_required(mass: f32, specific_heat: f32, delta_temp: f32) -> f32 {
    mass * specific_heat * delta_temp
}

/// Calculate temperature change from heat: ΔT = Q/(mc)
#[inline]
pub fn temperature_change(heat: f32, mass: f32, specific_heat: f32) -> f32 {
    if mass <= 0.0 || specific_heat <= 0.0 {
        return 0.0;
    }
    heat / (mass * specific_heat)
}

// ============================================================================
// Second Law of Thermodynamics: Entropy
// ============================================================================

/// Entropy change for reversible process: ΔS = Q/T
#[inline]
pub fn entropy_change_reversible(heat: f32, temperature: f32) -> f32 {
    if temperature <= 0.0 {
        return f32::INFINITY;
    }
    heat / temperature
}

/// Entropy change for ideal gas (isothermal): ΔS = nR·ln(V₂/V₁)
pub fn entropy_change_isothermal(n: f32, v1: f32, v2: f32) -> f32 {
    if v1 <= 0.0 || v2 <= 0.0 {
        return 0.0;
    }
    n * constants::R_F32 * (v2 / v1).ln()
}

/// Entropy change for ideal gas (general): ΔS = nCv·ln(T₂/T₁) + nR·ln(V₂/V₁)
pub fn entropy_change_general(n: f32, cv: f32, t1: f32, t2: f32, v1: f32, v2: f32) -> f32 {
    if t1 <= 0.0 || t2 <= 0.0 || v1 <= 0.0 || v2 <= 0.0 {
        return 0.0;
    }
    n * cv * (t2 / t1).ln() + n * constants::R_F32 * (v2 / v1).ln()
}

/// Carnot efficiency: η = 1 - T_cold/T_hot
#[inline]
pub fn carnot_efficiency(t_cold: f32, t_hot: f32) -> f32 {
    if t_hot <= 0.0 || t_cold >= t_hot {
        return 0.0;
    }
    1.0 - (t_cold / t_hot)
}

/// Coefficient of performance for refrigerator: COP = T_cold/(T_hot - T_cold)
#[inline]
pub fn cop_refrigerator(t_cold: f32, t_hot: f32) -> f32 {
    let delta_t = t_hot - t_cold;
    if delta_t <= 0.0 {
        return f32::INFINITY;
    }
    t_cold / delta_t
}

/// Coefficient of performance for heat pump: COP = T_hot/(T_hot - T_cold)
#[inline]
pub fn cop_heat_pump(t_cold: f32, t_hot: f32) -> f32 {
    let delta_t = t_hot - t_cold;
    if delta_t <= 0.0 {
        return f32::INFINITY;
    }
    t_hot / delta_t
}

// ============================================================================
// Heat Transfer
// ============================================================================

/// Heat conduction (Fourier's Law): Q/t = -kA(dT/dx)
/// 
/// # Arguments
/// * `k` - Thermal conductivity (W/(m·K))
/// * `area` - Cross-sectional area (m²)
/// * `delta_temp` - Temperature difference (K)
/// * `thickness` - Material thickness (m)
/// 
/// # Returns
/// Heat transfer rate (W)
#[inline]
pub fn heat_conduction_rate(k: f32, area: f32, delta_temp: f32, thickness: f32) -> f32 {
    if thickness <= 0.0 {
        return f32::INFINITY;
    }
    k * area * delta_temp / thickness
}

/// Heat convection (Newton's Law of Cooling): Q/t = hA(T_surface - T_fluid)
/// 
/// # Arguments
/// * `h` - Convective heat transfer coefficient (W/(m²·K))
/// * `area` - Surface area (m²)
/// * `t_surface` - Surface temperature (K)
/// * `t_fluid` - Fluid temperature (K)
/// 
/// # Returns
/// Heat transfer rate (W)
#[inline]
pub fn heat_convection_rate(h: f32, area: f32, t_surface: f32, t_fluid: f32) -> f32 {
    h * area * (t_surface - t_fluid)
}

/// Heat radiation (Stefan-Boltzmann Law): Q/t = εσA(T⁴ - T_env⁴)
/// 
/// # Arguments
/// * `emissivity` - Surface emissivity (0-1)
/// * `area` - Surface area (m²)
/// * `t_surface` - Surface temperature (K)
/// * `t_environment` - Environment temperature (K)
/// 
/// # Returns
/// Heat transfer rate (W)
pub fn heat_radiation_rate(emissivity: f32, area: f32, t_surface: f32, t_environment: f32) -> f32 {
    let t_s4 = t_surface.powi(4);
    let t_e4 = t_environment.powi(4);
    emissivity * constants::STEFAN_BOLTZMANN_F32 * area * (t_s4 - t_e4)
}

// ============================================================================
// Phase Transitions
// ============================================================================

/// Heat required for phase change: Q = mL
/// 
/// # Arguments
/// * `mass` - Mass of substance (kg)
/// * `latent_heat` - Latent heat of transition (J/kg)
/// 
/// # Returns
/// Heat required (J)
#[inline]
pub fn heat_phase_change(mass: f32, latent_heat: f32) -> f32 {
    mass * latent_heat
}

/// Phase of water based on temperature and pressure (simplified)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum WaterPhase {
    Solid,
    Liquid,
    Gas,
    Supercritical,
}

/// Determine water phase (simplified model)
pub fn water_phase(temperature: f32, pressure: f32) -> WaterPhase {
    let t_celsius = temperature - 273.15;
    let p_atm = pressure / 101_325.0;
    
    // Critical point: 374°C, 218 atm
    if t_celsius > 374.0 && p_atm > 218.0 {
        return WaterPhase::Supercritical;
    }
    
    // Simplified phase boundaries
    if t_celsius < 0.0 {
        WaterPhase::Solid
    } else if t_celsius < 100.0 * p_atm.powf(0.1) {
        WaterPhase::Liquid
    } else {
        WaterPhase::Gas
    }
}

// ============================================================================
// Enthalpy
// ============================================================================

/// Enthalpy: H = U + PV
#[inline]
pub fn enthalpy(internal_energy: f32, pressure: f32, volume: f32) -> f32 {
    internal_energy + pressure * volume
}

/// Enthalpy change at constant pressure: ΔH = Qp
#[inline]
pub fn enthalpy_change_isobaric(heat_at_constant_pressure: f32) -> f32 {
    heat_at_constant_pressure
}

/// Gibbs free energy: G = H - TS
#[inline]
pub fn gibbs_free_energy(enthalpy: f32, temperature: f32, entropy: f32) -> f32 {
    enthalpy - temperature * entropy
}

/// Helmholtz free energy: F = U - TS
#[inline]
pub fn helmholtz_free_energy(internal_energy: f32, temperature: f32, entropy: f32) -> f32 {
    internal_energy - temperature * entropy
}

// ============================================================================
// Thermodynamic State
// ============================================================================

/// Complete thermodynamic state for a system
#[derive(Debug, Clone, Copy, Reflect)]
pub struct ThermodynamicStateData {
    /// Temperature (K)
    pub temperature: f32,
    /// Pressure (Pa)
    pub pressure: f32,
    /// Volume (m³)
    pub volume: f32,
    /// Internal energy (J)
    pub internal_energy: f32,
    /// Entropy (J/K)
    pub entropy: f32,
    /// Enthalpy (J)
    pub enthalpy: f32,
    /// Gibbs free energy (J)
    pub gibbs: f32,
    /// Amount of substance (mol)
    pub moles: f32,
}

impl ThermodynamicStateData {
    /// Create state for ideal gas at given conditions
    pub fn ideal_gas(moles: f32, temperature: f32, volume: f32) -> Self {
        let pressure = ideal_gas_pressure(moles, temperature, volume);
        // For monatomic ideal gas: U = (3/2)nRT
        let internal_energy = 1.5 * moles * constants::R_F32 * temperature;
        let enthalpy_val = enthalpy(internal_energy, pressure, volume);
        // Reference entropy (arbitrary reference point)
        let entropy = moles * constants::R_F32 * (temperature / 298.15).ln();
        let gibbs = gibbs_free_energy(enthalpy_val, temperature, entropy);
        
        Self {
            temperature,
            pressure,
            volume,
            internal_energy,
            entropy,
            enthalpy: enthalpy_val,
            gibbs,
            moles,
        }
    }
    
    /// Update state after heat addition at constant volume
    pub fn add_heat_isochoric(&mut self, heat: f32) {
        let cv = heat_capacity_monatomic_cv(self.moles);
        let delta_t = heat / cv;
        self.temperature += delta_t;
        self.internal_energy += heat;
        self.pressure = ideal_gas_pressure(self.moles, self.temperature, self.volume);
        self.entropy += entropy_change_reversible(heat, self.temperature);
        self.enthalpy = enthalpy(self.internal_energy, self.pressure, self.volume);
        self.gibbs = gibbs_free_energy(self.enthalpy, self.temperature, self.entropy);
    }
    
    /// Update state after heat addition at constant pressure
    pub fn add_heat_isobaric(&mut self, heat: f32) {
        let cp = heat_capacity_monatomic_cp(self.moles);
        let delta_t = heat / cp;
        self.temperature += delta_t;
        self.volume = ideal_gas_volume(self.moles, self.temperature, self.pressure);
        self.internal_energy = 1.5 * self.moles * constants::R_F32 * self.temperature;
        self.entropy += entropy_change_reversible(heat, self.temperature);
        self.enthalpy = enthalpy(self.internal_energy, self.pressure, self.volume);
        self.gibbs = gibbs_free_energy(self.enthalpy, self.temperature, self.entropy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ideal_gas_law() {
        // 1 mol at STP should give ~101325 Pa
        let p = ideal_gas_pressure(1.0, 273.15, 0.0224);
        assert!((p - 101_325.0).abs() < 1000.0);
    }
    
    #[test]
    fn test_carnot_efficiency() {
        // Hot = 500K, Cold = 300K -> η = 0.4
        let eff = carnot_efficiency(300.0, 500.0);
        assert!((eff - 0.4).abs() < 0.01);
    }
    
    #[test]
    fn test_heat_conduction() {
        // Steel plate: k=50, A=1m², ΔT=100K, thickness=0.01m
        let q = heat_conduction_rate(50.0, 1.0, 100.0, 0.01);
        assert!((q - 500_000.0).abs() < 1.0);
    }
    
    #[test]
    fn test_water_phase() {
        assert_eq!(water_phase(263.15, 101_325.0), WaterPhase::Solid);
        assert_eq!(water_phase(293.15, 101_325.0), WaterPhase::Liquid);
        assert_eq!(water_phase(393.15, 101_325.0), WaterPhase::Gas);
    }
}
