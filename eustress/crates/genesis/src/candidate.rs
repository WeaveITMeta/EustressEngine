//! The architecture-design candidate schema (Phase 5 / Way A1, A2).
//!
//! A candidate is a buildable structure expressed as the four optimization axes
//! the world-model loop searches over: STRUCTURE (nodes + members), MATERIAL
//! (per-member material), FIXTURES/BONDS (joints between members), and STYLE (a
//! latent the generator can mimic or invent). Pure data (serde, no engine
//! types) so the generative loop, FEA, and scoring run headless + deterministic.

use serde::{Deserialize, Serialize};

/// A structural node (joint) in world space.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Node {
    /// Position (m), world axes.
    pub pos: [f32; 3],
    /// How the node is restrained (a ground support removes degrees of freedom).
    pub support: Support,
    /// External load applied at the node (force, N, world axes).
    pub load: [f32; 3],
}

/// Support condition at a node — restraint for stability + FEA boundary cond.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Support {
    /// Free node — no restraint.
    #[default]
    Free,
    /// Pinned — translation fixed, rotation free.
    Pinned,
    /// Fully fixed (encastre).
    Fixed,
}

/// Fixture / bond type at a member end — the "fixtures/bonds" optimization axis.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BondKind {
    /// Pin joint (transmits force, not moment).
    #[default]
    Pinned,
    /// Rigid / welded joint (transmits moment).
    Rigid,
    /// Bolted (rigid with a strength limit).
    Bolted,
    /// Adhesive bond.
    Bonded,
}

/// A structural member (bar/beam) connecting two nodes, with a section + material.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub from: usize,
    pub to: usize,
    /// Cross-section area (m^2) — the member sizing variable the optimizer tunes.
    pub area: f32,
    /// Index into [`ArchCandidate::materials`].
    pub material: usize,
    /// How this member bonds to its end nodes — the fixtures/bonds axis.
    pub bond: BondKind,
}

/// Material properties — the material optimization axis.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MaterialSpec {
    pub name: String,
    /// Density (kg/m^3).
    pub density: f32,
    /// Young's modulus E (Pa).
    pub youngs_modulus: f32,
    /// Yield strength (Pa).
    pub yield_strength: f32,
    /// Relative cost per kg (for efficiency scoring).
    pub cost_per_kg: f32,
}

impl MaterialSpec {
    /// A canonical palette so the loop has real materials to assign from.
    pub fn steel() -> Self {
        Self { name: "Steel".into(), density: 7850.0, youngs_modulus: 200e9, yield_strength: 250e6, cost_per_kg: 1.0 }
    }
    pub fn aluminium() -> Self {
        Self { name: "Aluminium".into(), density: 2700.0, youngs_modulus: 69e9, yield_strength: 240e6, cost_per_kg: 2.2 }
    }
    pub fn timber() -> Self {
        Self { name: "Timber".into(), density: 500.0, youngs_modulus: 11e9, yield_strength: 40e6, cost_per_kg: 0.6 }
    }
}

/// A style latent — the "invent or mimic an architectural style" axis (Way A1).
/// Each scalar nudges the generator (slenderness, triangulation, symmetry); the
/// free `latent` is filled by a learned style encoder later.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct StyleParams {
    pub slenderness: f32,
    pub triangulation: f32,
    pub symmetry: f32,
    pub latent: Vec<f32>,
}

/// One generated architectural candidate.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArchCandidate {
    pub id: u64,
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub materials: Vec<MaterialSpec>,
    pub style: StyleParams,
}

impl ArchCandidate {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            nodes: Vec::new(),
            members: Vec::new(),
            materials: Vec::new(),
            style: StyleParams::default(),
        }
    }

    /// Euclidean length of a member (m).
    pub fn member_length(&self, m: &Member) -> f32 {
        let a = self.nodes[m.from].pos;
        let b = self.nodes[m.to].pos;
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }

    /// Total structural mass (kg) = sum of member volume * material density.
    pub fn total_mass(&self) -> f32 {
        self.members
            .iter()
            .map(|m| self.member_length(m) * m.area * self.materials[m.material].density)
            .sum()
    }

    /// Total material cost (relative units).
    pub fn total_cost(&self) -> f32 {
        self.members
            .iter()
            .map(|m| {
                let mat = &self.materials[m.material];
                self.member_length(m) * m.area * mat.density * mat.cost_per_kg
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_node_bar() -> ArchCandidate {
        let mut c = ArchCandidate::new(1);
        c.materials.push(MaterialSpec::steel());
        c.nodes.push(Node { pos: [0.0, 0.0, 0.0], support: Support::Fixed, load: [0.0; 3] });
        c.nodes.push(Node { pos: [2.0, 0.0, 0.0], support: Support::Free, load: [1000.0, 0.0, 0.0] });
        c.members.push(Member { from: 0, to: 1, area: 0.01, material: 0, bond: BondKind::Pinned });
        c
    }

    #[test]
    fn mass_and_length() {
        let c = two_node_bar();
        assert!((c.member_length(&c.members[0]) - 2.0).abs() < 1e-5);
        // mass = L * A * rho = 2 * 0.01 * 7850 = 157 kg
        assert!((c.total_mass() - 157.0).abs() < 1e-2);
    }

    #[test]
    fn serde_round_trips() {
        let c = two_node_bar();
        let json = serde_json::to_string(&c).unwrap();
        let back: ArchCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}
