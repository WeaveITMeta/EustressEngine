//! # Compliance
//!
//! Machine-readable control registers for the regulatory regimes Eustress is
//! assessed against, kept in code so that evidence is CI-verifiable and
//! compliance drift breaks the build rather than rotting inside a document.
//!
//! Currently: [`cmmc`] — CMMC Levels 1 and 2 (FAR 52.204-21 / NIST SP 800-171).

pub mod cmmc;

pub use cmmc::{
    assess, level_1_controls, sprs_score, Assessment, CmmcLevel, Control, Domain, Evidence,
    Responsibility, Status,
};
