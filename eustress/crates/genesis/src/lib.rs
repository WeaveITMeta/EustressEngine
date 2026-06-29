//! `eustress-genesis` — the world-model generative spine (Phases 4-6 of the
//! Master Roadmap).
//!
//! This is the headless, deterministic core behind the physics-driven
//! generative architecture-design loop that REPLACES reinforcement learning:
//! generate candidates -> evaluate with closed-form fitness (then Avian, then
//! FEA) -> score -> optimize. It is pure logic (serde, no Bevy / engine types)
//! so it unit-tests and replays deterministically; the Bevy `GenerativeArchPlugin`
//! (Phase 5) and the engine wiring layer on top.
//!
//! Modules:
//! - [`candidate`] — the design schema: the four optimization axes (STRUCTURE,
//!   MATERIAL, FIXTURES/BONDS, STYLE).
//! - [`fitness`]   — closed-form scoring (the first rung of the eval ladder).
//! - [`optimizer`] — the generate->score->optimize loop + a baseline optimizer.
//! - [`fea`]       — 1D linear finite-element MVP (the physical verifier, Phase 4).
//! - [`ingest`]    — vendor-agnostic generation + provenance contracts (Phase 6).

pub mod candidate;
pub mod fea;
pub mod fitness;
pub mod ingest;
pub mod optimizer;

pub use candidate::{ArchCandidate, BondKind, MaterialSpec, Member, Node, StyleParams, Support};
pub use fea::{BarElement, Fea1d, FeaResult};
pub use fitness::{ClosedFormFitness, Fitness, Score, Weights};
pub use ingest::{
    AssetKind, GeneratedAsset, GenerationBackend, GenerationError, IngestSource, NullBackend,
};
pub use optimizer::{run_loop, Evaluated, HillClimb, Optimizer};
