//! # Symbolica Integration
//!
//! Real-time symbolic equation solving using Symbolica.
//!
//! ## Table of Contents
//!
//! 1. **PhysicsExpressions** - Pre-compiled physics expressions
//! 2. **ConstraintSolver** - Real-time constraint solving
//! 3. **Codegen** - Runtime code generation
//!
//! ## Architecture
//!
//! Symbolica enables:
//! - **Symbolic derivation** of physics equations at compile-time
//! - **Real-time solving** of constraint systems
//! - **Code generation** for optimized numerical evaluation
//! - **Exact arithmetic** avoiding floating-point drift

#[cfg(feature = "realism-symbolic")]
pub mod solver;
#[cfg(feature = "realism-symbolic")]
pub mod expressions;
#[cfg(feature = "realism-symbolic")]
pub mod codegen;
#[cfg(feature = "realism-symbolic")]
pub mod nonlinear;

pub mod prelude {
    #[cfg(feature = "realism-symbolic")]
    pub use super::solver::*;
    #[cfg(feature = "realism-symbolic")]
    pub use super::expressions::*;
    #[cfg(feature = "realism-symbolic")]
    pub use super::nonlinear::*;
    #[cfg(feature = "realism-symbolic")]
    pub use super::SymbolicPlugin;
}

use bevy::prelude::*;

/// Symbolic mathematics plugin
#[cfg(feature = "realism-symbolic")]
pub struct SymbolicPlugin;

#[cfg(feature = "realism-symbolic")]
impl Plugin for SymbolicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<expressions::PhysicsExpressions>();
        info!("SymbolicPlugin initialized - Symbolica ready for real-time solving");
    }
}

/// Placeholder plugin when feature is disabled
#[cfg(not(feature = "realism-symbolic"))]
pub struct SymbolicPlugin;

#[cfg(not(feature = "realism-symbolic"))]
impl Plugin for SymbolicPlugin {
    fn build(&self, _app: &mut App) {
        info!("SymbolicPlugin disabled - enable 'realism-symbolic' feature");
    }
}
