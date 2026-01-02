//! # Property System
//!
//! Re-exports PropertyAccess implementations from common crate.

// Re-export property system types for convenience
pub use eustress_common::classes::{PropertyAccess, PropertyValue, PropertyDescriptor};

// The PropertyAccess implementations are now in eustress_common::properties
// to satisfy Rust's orphan rules.
