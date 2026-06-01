//! # `physics` — engine-side physics runtime systems
//!
//! Wave 6.B introduces this module to hold the *runtime* half of the
//! constraint/mover system: Bevy systems that read the Phase-0 mover
//! configuration components placed by
//! [`crate::spawners::constraints`] and push the corresponding force /
//! velocity / torque onto Avian rigid bodies each physics frame.
//!
//! The rigid *joints* (Weld/Hinge/Distance/Rod/Cylindrical/…) need no
//! runtime system — Avian's solver drives the joint components directly.
//! Only the data-driven *movers* (`VectorForce`, `AlignPosition`,
//! `LinearVelocity`, the legacy `Body*` objects, …) need this per-frame
//! actuation pass.
//!
//! ## Mounting
//!
//! Add [`movers::MoversPlugin`] to the app (alongside the spawner
//! plugins). It gates all its systems to `PlayModeState::Playing`.

pub mod movers;

pub use movers::MoversPlugin;
