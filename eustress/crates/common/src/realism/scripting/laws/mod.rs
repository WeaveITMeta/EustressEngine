//! # Realism law Rune bindings
//!
//! Exposes the realism kernel's stateless law functions to Rune scripts under
//! domain sub-namespaces: `eustress::realism::<domain>::<fn>`.
//!
//! ```rune
//! use eustress::realism::electrical;
//! use eustress::realism::chemistry;
//! let i = electrical::ohm_current(12.0, 4.0);          // 3.0 A
//! let k = chemistry::arrhenius_rate(1.0e8, 50000.0, 298.15);
//! ```
//!
//! Each domain is a separate Rune `Module` (Rune sub-namespaces are distinct
//! modules). `realism_law_modules()` returns them all for installation into the
//! Rune context, alongside the engine ECS module. Bindings are f64 wrappers
//! over the f32 kernel laws (f64-native kernels pass through unchanged).

use rune::Module;
use tracing::{error, info};

pub mod electrical;
pub mod chemistry;
pub mod thermocycles;
pub mod structures;
pub mod propulsion;
pub mod optics;
pub mod acoustics;
pub mod nuclear;
pub mod plasma;
pub mod control;
pub mod numerics;
pub mod mechanics;
pub mod thermodynamics;

/// Build every realism law module. A module that fails to build is logged and
/// skipped rather than aborting the whole set.
pub fn realism_law_modules() -> Vec<Module> {
    let mut modules = Vec::new();

    let builders: &[(&str, fn() -> Result<Module, rune::ContextError>)] = &[
        ("electrical", electrical::create_module),
        ("chemistry", chemistry::create_module),
        ("thermocycles", thermocycles::create_module),
        ("structures", structures::create_module),
        ("propulsion", propulsion::create_module),
        ("optics", optics::create_module),
        ("acoustics", acoustics::create_module),
        ("nuclear", nuclear::create_module),
        ("plasma", plasma::create_module),
        ("control", control::create_module),
        ("numerics", numerics::create_module),
        ("mechanics", mechanics::create_module),
        ("thermodynamics", thermodynamics::create_module),
    ];

    for (name, build) in builders {
        match build() {
            Ok(m) => modules.push(m),
            Err(e) => error!("Failed to build realism::{} Rune module: {:?}", name, e),
        }
    }

    info!("Registered {} realism law Rune modules", modules.len());
    modules
}
