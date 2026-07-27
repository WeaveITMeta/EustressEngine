//! # eustress-avatar-schema
//!
//! The avatar descriptor and its derived body metrics, with no Bevy and no
//! glam in the default build.
//!
//! ## Why this is a separate crate
//!
//! `eustress/crates/web` is pure CSR wasm (`leptos` + `wasm-bindgen`) and has
//! **no `eustress-common` dependency**. A descriptor type that derived
//! `bevy::Component` unconditionally could not compile there, so the website
//! would need its own hand-mirrored copy of the struct — reintroducing exactly
//! the drift this design exists to eliminate.
//!
//! Putting the descriptor in a Bevy-free crate makes "the website and the
//! engine agree" a **linker fact** rather than a convention someone has to
//! maintain. The engine enables the `bevy` feature to get `Component` and
//! `Reflect` derives; the web crate does not.
//!
//! ## What lives here
//!
//! * [`AvatarDescriptor`] — the complete authored avatar.
//! * [`BodyMetrics`] — every physical dimension, derived, never authored.
//! * [`ResolvedMotion`] — gait speeds and jump apex after overrides.
//!
//! All derivation is **pure**: no globals, no asset lookups, measured bind
//! height passed in as a parameter. That is what lets the parity test assert
//! bit-identical equality between the two shells rather than an epsilon.

pub mod descriptor;
pub mod metrics;

pub use descriptor::{
    AvatarDescriptor, AvatarError, BaseBody, BodyMorphs, FaceShape, ItemId, MotionOverrides,
    Norm01, Palette, SlotKind, Srgb8, AVATAR_SCHEMA_VERSION,
};
pub use metrics::{
    resolve, resolve_motion, BodyMetrics, ResolvedMotion, GRAVITY_MPS2, MAX_HEIGHT_M,
    MIN_HEIGHT_M, NOMINAL_BIND_HEIGHT_M, REFERENCE_LEG_LENGTH_M,
};
