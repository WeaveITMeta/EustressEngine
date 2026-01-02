//! # Rune API
//!
//! Physics functions and types exposed to Rune scripts.
//!
//! ## Usage
//!
//! Scripts can access physics functions through the `physics` module
//! and entity functions through the `entity` module.

use std::collections::HashMap;

/// Physics constants exposed to Rune
pub mod constants {
    /// Universal gas constant (J/(mol·K))
    pub const R: f64 = 8.314462618;
    /// Gravitational constant (m³/(kg·s²))
    pub const G: f64 = 6.67430e-11;
    /// Boltzmann constant (J/K)
    pub const K_B: f64 = 1.380649e-23;
    /// Speed of light (m/s)
    pub const C: f64 = 299792458.0;
    /// Standard gravity (m/s²)
    pub const G_EARTH: f64 = 9.81;
    /// Standard atmospheric pressure (Pa)
    pub const ATM: f64 = 101325.0;
    /// Water density at 4°C (kg/m³)
    pub const WATER_DENSITY: f64 = 1000.0;
    /// Air density at sea level (kg/m³)
    pub const AIR_DENSITY: f64 = 1.225;
}

/// Physics functions exposed to Rune
pub mod physics {
    use super::constants;
    
    /// Ideal gas pressure: P = nRT/V
    pub fn ideal_gas_pressure(n: f64, t: f64, v: f64) -> f64 {
        if v <= 0.0 {
            return f64::INFINITY;
        }
        (n * constants::R * t) / v
    }
    
    /// Ideal gas volume: V = nRT/P
    pub fn ideal_gas_volume(n: f64, t: f64, p: f64) -> f64 {
        if p <= 0.0 {
            return f64::INFINITY;
        }
        (n * constants::R * t) / p
    }
    
    /// Kinetic energy: KE = 0.5*m*v²
    pub fn kinetic_energy(mass: f64, velocity: f64) -> f64 {
        0.5 * mass * velocity * velocity
    }
    
    /// Gravitational force: F = G*m1*m2/r²
    pub fn gravitational_force(m1: f64, m2: f64, r: f64) -> f64 {
        if r <= 0.0 {
            return f64::INFINITY;
        }
        (constants::G * m1 * m2) / (r * r)
    }
    
    /// Drag force: Fd = 0.5*ρ*v²*Cd*A
    pub fn drag_force(density: f64, velocity: f64, cd: f64, area: f64) -> f64 {
        0.5 * density * velocity * velocity * cd * area
    }
    
    /// Buoyancy force: Fb = ρ*V*g
    pub fn buoyancy_force(fluid_density: f64, volume: f64) -> f64 {
        fluid_density * volume * constants::G_EARTH
    }
    
    /// Carnot efficiency: η = 1 - Tc/Th
    pub fn carnot_efficiency(t_cold: f64, t_hot: f64) -> f64 {
        if t_hot <= 0.0 || t_cold >= t_hot {
            return 0.0;
        }
        1.0 - (t_cold / t_hot)
    }
    
    /// Entropy change: ΔS = Q/T
    pub fn entropy_change(heat: f64, temperature: f64) -> f64 {
        if temperature <= 0.0 {
            return f64::INFINITY;
        }
        heat / temperature
    }
    
    /// Von Mises stress from principal stresses
    pub fn von_mises_stress(s1: f64, s2: f64, s3: f64) -> f64 {
        let term = (s1 - s2).powi(2) + (s2 - s3).powi(2) + (s3 - s1).powi(2);
        (term / 2.0).sqrt()
    }
    
    /// Check yield condition
    pub fn check_yield(von_mises: f64, yield_strength: f64) -> bool {
        von_mises >= yield_strength
    }
    
    /// Reynolds number: Re = ρvL/μ
    pub fn reynolds_number(density: f64, velocity: f64, length: f64, viscosity: f64) -> f64 {
        if viscosity <= 0.0 {
            return f64::INFINITY;
        }
        density * velocity * length / viscosity
    }
    
    /// Terminal velocity: v = √(2mg/(ρCdA))
    pub fn terminal_velocity(mass: f64, density: f64, cd: f64, area: f64) -> f64 {
        let denom = density * cd * area;
        if denom <= 0.0 {
            return f64::INFINITY;
        }
        (2.0 * mass * constants::G_EARTH / denom).sqrt()
    }
}

/// Script function registry
#[derive(Default)]
pub struct FunctionRegistry {
    /// Registered functions by name
    functions: HashMap<String, FunctionInfo>,
}

/// Information about a registered function
#[derive(Clone)]
pub struct FunctionInfo {
    /// Function name
    pub name: String,
    /// Parameter names
    pub params: Vec<String>,
    /// Return type description
    pub returns: String,
    /// Documentation
    pub doc: String,
}

impl FunctionRegistry {
    /// Create registry with all physics functions
    pub fn with_physics() -> Self {
        let mut registry = Self::default();
        
        registry.register(FunctionInfo {
            name: "ideal_gas_pressure".to_string(),
            params: vec!["n".to_string(), "t".to_string(), "v".to_string()],
            returns: "f64".to_string(),
            doc: "Calculate pressure using ideal gas law: P = nRT/V".to_string(),
        });
        
        registry.register(FunctionInfo {
            name: "kinetic_energy".to_string(),
            params: vec!["mass".to_string(), "velocity".to_string()],
            returns: "f64".to_string(),
            doc: "Calculate kinetic energy: KE = 0.5*m*v²".to_string(),
        });
        
        registry.register(FunctionInfo {
            name: "drag_force".to_string(),
            params: vec!["density".to_string(), "velocity".to_string(), "cd".to_string(), "area".to_string()],
            returns: "f64".to_string(),
            doc: "Calculate drag force: Fd = 0.5*ρ*v²*Cd*A".to_string(),
        });
        
        registry.register(FunctionInfo {
            name: "buoyancy_force".to_string(),
            params: vec!["fluid_density".to_string(), "volume".to_string()],
            returns: "f64".to_string(),
            doc: "Calculate buoyancy force: Fb = ρ*V*g".to_string(),
        });
        
        registry.register(FunctionInfo {
            name: "von_mises_stress".to_string(),
            params: vec!["s1".to_string(), "s2".to_string(), "s3".to_string()],
            returns: "f64".to_string(),
            doc: "Calculate von Mises equivalent stress from principal stresses".to_string(),
        });
        
        registry
    }
    
    /// Register a function
    pub fn register(&mut self, info: FunctionInfo) {
        self.functions.insert(info.name.clone(), info);
    }
    
    /// Get function info
    pub fn get(&self, name: &str) -> Option<&FunctionInfo> {
        self.functions.get(name)
    }
    
    /// List all functions
    pub fn list(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }
    
    /// Generate documentation
    pub fn generate_docs(&self) -> String {
        let mut docs = String::from("# Physics API\n\n");
        
        for (name, info) in &self.functions {
            docs.push_str(&format!("## {}\n\n", name));
            docs.push_str(&format!("{}\n\n", info.doc));
            docs.push_str("**Parameters:**\n");
            for param in &info.params {
                docs.push_str(&format!("- `{}`\n", param));
            }
            docs.push_str(&format!("\n**Returns:** `{}`\n\n", info.returns));
        }
        
        docs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ideal_gas() {
        let p = physics::ideal_gas_pressure(1.0, 273.15, 0.0224);
        assert!((p - 101325.0).abs() < 1000.0);
    }
    
    #[test]
    fn test_kinetic_energy() {
        let ke = physics::kinetic_energy(2.0, 10.0);
        assert!((ke - 100.0).abs() < 0.01);
    }
    
    #[test]
    fn test_registry() {
        let registry = FunctionRegistry::with_physics();
        assert!(registry.get("ideal_gas_pressure").is_some());
        assert!(registry.get("kinetic_energy").is_some());
    }
}
