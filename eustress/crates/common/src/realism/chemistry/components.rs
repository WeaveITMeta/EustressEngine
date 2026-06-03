//! ECS components for chemical simulation — species, reactions, reactors.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const R: f32 = 8.314; // J/(mol·K)

// ── Chemical species (per entity or pooled) ───────────────────────

/// A single chemical species tracked in a simulation.
#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
pub struct ChemicalSpecies {
    pub name: String,
    pub molar_mass: f32,        // g/mol
    pub concentration: f32,     // mol/L (molar)
    pub delta_h_formation: f32, // J/mol (standard enthalpy of formation at 298K)
    pub delta_g_formation: f32, // J/mol (standard Gibbs of formation at 298K)
}

impl ChemicalSpecies {
    /// Create a new species with zero concentration and zero formation energies.
    pub fn new(name: &str, molar_mass: f32) -> Self {
        Self {
            name: name.to_string(),
            molar_mass,
            concentration: 0.0,
            delta_h_formation: 0.0,
            delta_g_formation: 0.0,
        }
    }

    /// H2O: M=18.015 g/mol, ΔH_f=-285,800 J/mol, ΔG_f=-237,100 J/mol
    pub fn water() -> Self {
        Self {
            name: "H2O".to_string(),
            molar_mass: 18.015,
            concentration: 0.0,
            delta_h_formation: -285_800.0,
            delta_g_formation: -237_100.0,
        }
    }

    /// CO2: M=44.01 g/mol, ΔH_f=-393,500 J/mol, ΔG_f=-394,400 J/mol
    pub fn co2() -> Self {
        Self {
            name: "CO2".to_string(),
            molar_mass: 44.01,
            concentration: 0.0,
            delta_h_formation: -393_500.0,
            delta_g_formation: -394_400.0,
        }
    }

    /// O2: M=32.0 g/mol, ΔH_f=0, ΔG_f=0 (reference element)
    pub fn oxygen() -> Self {
        Self {
            name: "O2".to_string(),
            molar_mass: 32.0,
            concentration: 0.0,
            delta_h_formation: 0.0,
            delta_g_formation: 0.0,
        }
    }

    /// N2: M=28.014 g/mol, ΔH_f=0, ΔG_f=0 (reference element)
    pub fn nitrogen() -> Self {
        Self {
            name: "N2".to_string(),
            molar_mass: 28.014,
            concentration: 0.0,
            delta_h_formation: 0.0,
            delta_g_formation: 0.0,
        }
    }

    /// H2: M=2.016 g/mol, ΔH_f=0, ΔG_f=0 (reference element)
    pub fn hydrogen() -> Self {
        Self {
            name: "H2".to_string(),
            molar_mass: 2.016,
            concentration: 0.0,
            delta_h_formation: 0.0,
            delta_g_formation: 0.0,
        }
    }

    /// CH4: M=16.043 g/mol, ΔH_f=-74,800 J/mol, ΔG_f=-50,750 J/mol
    pub fn methane() -> Self {
        Self {
            name: "CH4".to_string(),
            molar_mass: 16.043,
            concentration: 0.0,
            delta_h_formation: -74_800.0,
            delta_g_formation: -50_750.0,
        }
    }

    /// C2H5OH: M=46.068 g/mol, ΔH_f=-277,700 J/mol, ΔG_f=-174,780 J/mol
    pub fn ethanol() -> Self {
        Self {
            name: "C2H5OH".to_string(),
            molar_mass: 46.068,
            concentration: 0.0,
            delta_h_formation: -277_700.0,
            delta_g_formation: -174_780.0,
        }
    }

    /// C6H12O6: M=180.16 g/mol, ΔH_f=-1,274,000 J/mol, ΔG_f=-910,560 J/mol
    pub fn glucose() -> Self {
        Self {
            name: "C6H12O6".to_string(),
            molar_mass: 180.16,
            concentration: 0.0,
            delta_h_formation: -1_274_000.0,
            delta_g_formation: -910_560.0,
        }
    }

    /// NH3: M=17.031 g/mol, ΔH_f=-46,100 J/mol, ΔG_f=-16,450 J/mol
    pub fn ammonia() -> Self {
        Self {
            name: "NH3".to_string(),
            molar_mass: 17.031,
            concentration: 0.0,
            delta_h_formation: -46_100.0,
            delta_g_formation: -16_450.0,
        }
    }

    /// HCl: M=36.461 g/mol, ΔH_f=-92,300 J/mol, ΔG_f=-95,300 J/mol
    pub fn hcl() -> Self {
        Self {
            name: "HCl".to_string(),
            molar_mass: 36.461,
            concentration: 0.0,
            delta_h_formation: -92_300.0,
            delta_g_formation: -95_300.0,
        }
    }

    /// NaOH: M=40.0 g/mol, ΔH_f=-425,600 J/mol, ΔG_f=-379,490 J/mol
    pub fn naoh() -> Self {
        Self {
            name: "NaOH".to_string(),
            molar_mass: 40.0,
            concentration: 0.0,
            delta_h_formation: -425_600.0,
            delta_g_formation: -379_490.0,
        }
    }
}

// ── Mixture ───────────────────────────────────────────────────────

/// Mixture of chemical species at given T and P (Bevy Component).
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ChemicalMixture {
    pub species: Vec<ChemicalSpecies>,
    pub temperature_k: f32,
    pub pressure_pa: f32,
    pub volume_m3: f32,
    /// Instantaneous heat generated [W] — computed each frame by reaction systems.
    pub heat_generated_w: f32,
}

impl Default for ChemicalMixture {
    fn default() -> Self {
        Self {
            species: Vec::new(),
            temperature_k: 298.15,
            pressure_pa: 101_325.0,
            volume_m3: 1.0,
            heat_generated_w: 0.0,
        }
    }
}

impl ChemicalMixture {
    /// Sum of concentration × volume for all species [mol].
    pub fn total_moles(&self) -> f32 {
        self.species.iter().map(|s| s.concentration * self.volume_m3).sum()
    }

    /// Concentration of named species [mol/L], or None if absent.
    pub fn concentration(&self, name: &str) -> Option<f32> {
        self.species.iter().find(|s| s.name == name).map(|s| s.concentration)
    }

    /// Mole fraction of named species, computed from concentrations.
    /// Returns None if the species is absent or the total concentration is zero.
    pub fn mole_fraction(&self, name: &str) -> Option<f32> {
        let total: f32 = self.species.iter().map(|s| s.concentration).sum();
        if total == 0.0 {
            return None;
        }
        self.species
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.concentration / total)
    }

    /// Push a species into the mixture.  Duplicates are allowed and will
    /// both contribute to totals; use `set_concentration` to update existing.
    pub fn add_species(&mut self, s: ChemicalSpecies) {
        self.species.push(s);
    }

    /// Update the concentration of a named species in place.
    /// Clamps to 0 — concentration cannot go negative.
    /// If the species does not exist this is a no-op.
    pub fn set_concentration(&mut self, name: &str, c: f32) {
        if let Some(s) = self.species.iter_mut().find(|s| s.name == name) {
            s.concentration = c.max(0.0);
        }
    }
}

// ── Reaction ─────────────────────────────────────────────────────

/// A single chemical reaction (Component on the reaction entity).
///
/// The Arrhenius rate is computed each frame:
///   r = k(T) · ∏ [Rᵢ]^νᵢ
/// where k(T) = pre_exponential · exp(-activation_energy / (R·T)).
///
/// For reversible reactions the net rate is r_fwd − r_rev, where
///   r_rev = (r_fwd / equilibrium_constant)  (Van't Hoff approximation at 298 K).
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ChemicalReaction {
    pub name: String,
    pub reactant_names: Vec<String>,
    pub reactant_stoich: Vec<f32>,
    pub product_names: Vec<String>,
    pub product_stoich: Vec<f32>,
    /// Activation energy [J/mol].
    pub activation_energy: f32,
    /// Pre-exponential (frequency) factor — same units as the rate constant.
    pub pre_exponential: f32,
    /// Standard enthalpy of reaction [J/mol]; negative = exothermic.
    pub delta_h_rxn: f32,
    /// Net reaction rate [mol/(L·s)] — updated by the kinetics system each frame.
    pub rate: f32,
    pub is_reversible: bool,
    /// Thermodynamic equilibrium constant Kₑq at 298 K.
    pub equilibrium_constant: f32,
}

impl Default for ChemicalReaction {
    fn default() -> Self {
        Self {
            name: "unnamed_reaction".to_string(),
            reactant_names: Vec::new(),
            reactant_stoich: Vec::new(),
            product_names: Vec::new(),
            product_stoich: Vec::new(),
            activation_energy: 50_000.0,
            pre_exponential: 1.0e8,
            delta_h_rxn: 0.0,
            rate: 0.0,
            is_reversible: false,
            equilibrium_constant: 1.0,
        }
    }
}

impl ChemicalReaction {
    /// Arrhenius rate constant at temperature `t_kelvin`.
    #[inline]
    pub fn rate_constant(&self, t_kelvin: f32) -> f32 {
        self.pre_exponential * (-self.activation_energy / (R * t_kelvin)).exp()
    }

    /// Forward rate from a mixture's current concentrations and temperature.
    /// Uses stoichiometric coefficients as reaction orders (elementary assumption).
    pub fn forward_rate(&self, mixture: &ChemicalMixture) -> f32 {
        let k = self.rate_constant(mixture.temperature_k);
        let conc_product: f32 = self
            .reactant_names
            .iter()
            .zip(self.reactant_stoich.iter())
            .map(|(name, &stoich)| {
                mixture.concentration(name).unwrap_or(0.0).powf(stoich)
            })
            .product();
        k * conc_product
    }

    /// Net rate accounting for reversibility.
    ///
    /// For irreversible reactions this equals `forward_rate`.
    /// For reversible reactions: r_net = r_fwd − r_rev, where
    ///   r_rev = r_fwd / Kₑq  (simplified; uses 298 K constant).
    pub fn net_rate(&self, mixture: &ChemicalMixture) -> f32 {
        let r_fwd = self.forward_rate(mixture);
        if !self.is_reversible || self.equilibrium_constant == 0.0 {
            return r_fwd;
        }
        let r_rev = r_fwd / self.equilibrium_constant;
        (r_fwd - r_rev).max(0.0)
    }

    /// Instantaneous heat release rate [W] given the current net rate and reactor volume.
    ///
    /// Q_dot = −ΔH_rxn · r_net · volume  (negative ΔH → exothermic → positive heat out)
    #[inline]
    pub fn heat_release_w(&self, r_net: f32, volume_m3: f32) -> f32 {
        -self.delta_h_rxn * r_net * volume_m3
    }
}

// ── CSTR Reactor ──────────────────────────────────────────────────

/// Continuous Stirred-Tank Reactor (Component).
///
/// At steady state: τ = V / Q  (residence time)
/// Species balance: dCᵢ/dt = (Cᵢ_feed − Cᵢ) / τ  ±  stoich · r
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CstrReactor {
    /// Reactor working volume [m³].
    pub volume_m3: f32,
    /// Volumetric feed flow rate [m³/s].
    pub feed_flow_m3_s: f32,
    /// Feed concentrations keyed by species name [mol/L].
    pub feed_concentrations: HashMap<String, f32>,
    /// Heat removal by cooling jacket [W]; positive = heat removed.
    pub heat_removal_w: f32,
    /// Conversion of the key reactant (0–1) — computed by the reactor system.
    pub conversion: f32,
}

impl Default for CstrReactor {
    fn default() -> Self {
        Self {
            volume_m3: 1.0,
            feed_flow_m3_s: 1.0e-4,
            feed_concentrations: HashMap::new(),
            heat_removal_w: 0.0,
            conversion: 0.0,
        }
    }
}

impl CstrReactor {
    /// Residence time τ = V / Q [s].
    #[inline]
    pub fn residence_time_s(&self) -> f32 {
        if self.feed_flow_m3_s > 0.0 {
            self.volume_m3 / self.feed_flow_m3_s
        } else {
            f32::INFINITY
        }
    }

    /// Steady-state dilution rate D = 1/τ [s⁻¹].
    #[inline]
    pub fn dilution_rate(&self) -> f32 {
        if self.volume_m3 > 0.0 {
            self.feed_flow_m3_s / self.volume_m3
        } else {
            0.0
        }
    }

    /// Compute the species concentration derivatives dCᵢ/dt for the CSTR,
    /// given the mixture state and a single reaction.
    ///
    /// Returns a map of species name → dC/dt [mol/(L·s)].
    pub fn concentration_derivatives(
        &self,
        mixture: &ChemicalMixture,
        reaction: &ChemicalReaction,
    ) -> HashMap<String, f32> {
        let mut derivs: HashMap<String, f32> = HashMap::new();
        let d = self.dilution_rate();
        let r = reaction.net_rate(mixture);

        // Dilution term for all species present in mixture
        for s in &mixture.species {
            let c_feed = self.feed_concentrations.get(&s.name).copied().unwrap_or(0.0);
            let dc_dt = d * (c_feed - s.concentration);
            derivs.insert(s.name.clone(), dc_dt);
        }

        // Also account for feed species not yet in the mixture
        for (name, &c_feed) in &self.feed_concentrations {
            if !derivs.contains_key(name.as_str()) {
                let c_current = mixture.concentration(name).unwrap_or(0.0);
                derivs.insert(name.clone(), d * (c_feed - c_current));
            }
        }

        // Reaction term: reactants consumed, products formed
        for (name, &stoich) in reaction
            .reactant_names
            .iter()
            .zip(reaction.reactant_stoich.iter())
        {
            *derivs.entry(name.clone()).or_insert(0.0) -= stoich * r;
        }
        for (name, &stoich) in reaction
            .product_names
            .iter()
            .zip(reaction.product_stoich.iter())
        {
            *derivs.entry(name.clone()).or_insert(0.0) += stoich * r;
        }

        derivs
    }
}

// ── Batch Reactor ─────────────────────────────────────────────────

/// Batch reactor (Component).
///
/// No feed or effluent — species concentrations evolve purely from reaction:
///   dCᵢ/dt = ±stoich · r(T, C)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BatchReactor {
    /// Batch volume [m³].
    pub volume_m3: f32,
    /// Heat removed by cooling jacket [W]; positive = heat removed.
    pub heat_removal_w: f32,
    /// Wall-clock time elapsed since batch start [s].
    pub time_elapsed_s: f32,
    /// Target fractional conversion (0–1) for the key reactant.
    pub target_conversion: f32,
    /// Achieved fractional conversion — computed each frame.
    pub achieved_conversion: f32,
}

impl Default for BatchReactor {
    fn default() -> Self {
        Self {
            volume_m3: 1.0,
            heat_removal_w: 0.0,
            time_elapsed_s: 0.0,
            target_conversion: 0.95,
            achieved_conversion: 0.0,
        }
    }
}

impl BatchReactor {
    /// True when `achieved_conversion >= target_conversion`.
    #[inline]
    pub fn is_complete(&self) -> bool {
        self.achieved_conversion >= self.target_conversion
    }

    /// Compute the species concentration derivatives dCᵢ/dt for the batch,
    /// given the mixture state and a single reaction.
    ///
    /// Returns a map of species name → dC/dt [mol/(L·s)].
    pub fn concentration_derivatives(
        &self,
        mixture: &ChemicalMixture,
        reaction: &ChemicalReaction,
    ) -> HashMap<String, f32> {
        let mut derivs: HashMap<String, f32> = HashMap::new();
        let r = reaction.net_rate(mixture);

        for (name, &stoich) in reaction
            .reactant_names
            .iter()
            .zip(reaction.reactant_stoich.iter())
        {
            *derivs.entry(name.clone()).or_insert(0.0) -= stoich * r;
        }
        for (name, &stoich) in reaction
            .product_names
            .iter()
            .zip(reaction.product_stoich.iter())
        {
            *derivs.entry(name.clone()).or_insert(0.0) += stoich * r;
        }

        derivs
    }

    /// Update `achieved_conversion` given the initial concentration of the key
    /// reactant and its current concentration.
    pub fn update_conversion(&mut self, c0: f32, c_current: f32) {
        if c0 > 0.0 {
            self.achieved_conversion = ((c0 - c_current) / c0).clamp(0.0, 1.0);
        }
    }
}

// ── Plug Flow Reactor (bonus — fully specified, not stubbed) ──────

/// Plug Flow Reactor (PFR) — Component.
///
/// Spatial integration along the reactor axis is represented by `n_slices`
/// discretised cells.  Each cell holds the local concentration vector; the
/// reaction system integrates species balances cell-by-cell each frame.
///
/// dFᵢ/dV = r_i   (molar flow Fᵢ = Cᵢ · Q)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PfrReactor {
    /// Total reactor volume [m³].
    pub volume_m3: f32,
    /// Volumetric flow rate [m³/s].
    pub flow_rate_m3_s: f32,
    /// Number of spatial slices for finite-difference integration.
    pub n_slices: usize,
    /// Per-slice concentration maps [mol/L].  Length == n_slices after init.
    pub slices: Vec<HashMap<String, f32>>,
    /// Feed concentrations entering at slice 0 [mol/L].
    pub feed_concentrations: HashMap<String, f32>,
    /// Exit conversion of key reactant (0–1) — updated each frame.
    pub exit_conversion: f32,
}

impl Default for PfrReactor {
    fn default() -> Self {
        Self {
            volume_m3: 1.0,
            flow_rate_m3_s: 1.0e-4,
            n_slices: 10,
            slices: Vec::new(),
            feed_concentrations: HashMap::new(),
            exit_conversion: 0.0,
        }
    }
}

impl PfrReactor {
    /// Initialise all slices to the feed concentrations.
    pub fn initialise(&mut self) {
        self.slices = vec![self.feed_concentrations.clone(); self.n_slices];
    }

    /// Slice volume [m³].
    #[inline]
    pub fn slice_volume(&self) -> f32 {
        if self.n_slices > 0 {
            self.volume_m3 / self.n_slices as f32
        } else {
            self.volume_m3
        }
    }

    /// Residence time across the whole reactor τ = V / Q [s].
    #[inline]
    pub fn residence_time_s(&self) -> f32 {
        if self.flow_rate_m3_s > 0.0 {
            self.volume_m3 / self.flow_rate_m3_s
        } else {
            f32::INFINITY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn species_constructors_non_zero_enthalpy() {
        assert!(ChemicalSpecies::water().delta_h_formation < 0.0);
        assert!(ChemicalSpecies::methane().delta_h_formation < 0.0);
        assert_eq!(ChemicalSpecies::oxygen().delta_h_formation, 0.0);
    }

    #[test]
    fn mixture_concentration_lookup() {
        let mut m = ChemicalMixture::default();
        let mut h2o = ChemicalSpecies::water();
        h2o.concentration = 2.5;
        m.add_species(h2o);
        assert_eq!(m.concentration("H2O"), Some(2.5));
        assert_eq!(m.concentration("N2"), None);
    }

    #[test]
    fn mixture_set_concentration_clamps() {
        let mut m = ChemicalMixture::default();
        let s = ChemicalSpecies::water();
        m.add_species(s);
        m.set_concentration("H2O", -1.0);
        assert_eq!(m.concentration("H2O"), Some(0.0));
    }

    #[test]
    fn mixture_mole_fraction_sums_to_one() {
        let mut m = ChemicalMixture::default();
        let mut h2 = ChemicalSpecies::hydrogen();
        h2.concentration = 1.0;
        let mut o2 = ChemicalSpecies::oxygen();
        o2.concentration = 1.0;
        m.add_species(h2);
        m.add_species(o2);
        let xh2 = m.mole_fraction("H2").unwrap();
        let xo2 = m.mole_fraction("O2").unwrap();
        assert!((xh2 + xo2 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reaction_rate_constant_increases_with_temperature() {
        let rxn = ChemicalReaction::default();
        let k_low = rxn.rate_constant(300.0);
        let k_high = rxn.rate_constant(600.0);
        assert!(k_high > k_low);
    }

    #[test]
    fn cstr_residence_time() {
        let cstr = CstrReactor {
            volume_m3: 1.0,
            feed_flow_m3_s: 0.01,
            ..Default::default()
        };
        assert!((cstr.residence_time_s() - 100.0).abs() < 1e-5);
    }

    #[test]
    fn batch_reactor_conversion_update() {
        let mut batch = BatchReactor::default();
        batch.update_conversion(1.0, 0.1);
        assert!((batch.achieved_conversion - 0.9).abs() < 1e-5);
    }

    #[test]
    fn batch_reactor_is_complete() {
        let mut batch = BatchReactor { target_conversion: 0.95, ..Default::default() };
        batch.achieved_conversion = 0.96;
        assert!(batch.is_complete());
    }

    #[test]
    fn pfr_initialise_fills_slices() {
        let mut pfr = PfrReactor {
            n_slices: 5,
            feed_concentrations: {
                let mut m = HashMap::new();
                m.insert("H2O".to_string(), 1.0_f32);
                m
            },
            ..Default::default()
        };
        pfr.initialise();
        assert_eq!(pfr.slices.len(), 5);
        assert_eq!(pfr.slices[0].get("H2O"), Some(&1.0_f32));
    }
}
