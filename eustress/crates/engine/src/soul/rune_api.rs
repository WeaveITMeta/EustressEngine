//! # Rune Script API — Engine integration
//!
//! Wraps the common `RuneRuntimeState` with engine-specific concerns:
//! - Queries `SoulScriptData` components to gather scripts for compilation
//! - Installs the engine's full ECS module (with spatial queries, camera, etc.)
//! - Wires into `PlayModeState` transitions

use bevy::prelude::*;
use std::collections::HashMap;

// Re-export the common runtime for external use
pub use eustress_common::soul::rune_runtime::{
    RuneRuntimeState, RuneModuleRegistry, ScriptSource,
    run_script_init, run_script_update, cleanup_scripts as cleanup_scripts_on_stop,
};

/// Rune script execution engine (legacy compat)
#[derive(Debug, Default)]
pub struct RuneScriptEngine {
    pub modules: HashMap<String, String>,
}

impl RuneScriptEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Script command for execution
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    Spawn { class: String, name: String },
    Destroy { entity: Entity },
    SetProperty { entity: Entity, property: String, value: String },
    PlaySound { path: String },
    Log { message: String },
}

/// Physics spawn configuration
#[derive(Debug, Clone, Default)]
pub struct SpawnPhysics {
    pub enabled: bool,
    pub mass: f32,
    pub friction: f32,
}

/// Entity data for scripts
#[derive(Debug, Clone, Default)]
pub struct EntityData {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// Input data for scripts
#[derive(Debug, Clone, Default)]
pub struct InputData {
    pub mouse_position: Vec2,
    pub keys_pressed: Vec<String>,
}

/// Physics data for scripts
#[derive(Debug, Clone, Default)]
pub struct PhysicsData {
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

// ============================================================================
// Engine-specific systems — query SoulScriptData + install ECS module
// ============================================================================

/// System: register the engine's ECS module into the RuneModuleRegistry.
/// Called once at startup so modules are ready when play mode starts.
pub fn register_engine_rune_modules(
    mut module_registry: ResMut<RuneModuleRegistry>,
) {
    #[cfg(feature = "realism-scripting")]
    {
        match super::rune_ecs_module::create_ecs_module() {
            Ok(module) => {
                module_registry.add_module(module);
                info!("✅ Registered engine ECS module for Rune runtime");
            }
            Err(e) => {
                error!("Failed to create engine ECS module: {}", e);
            }
        }
    }
}

/// System: compile all SoulScriptData entities when entering Playing state.
/// Gathers script sources from ECS and delegates to common runtime.
pub fn compile_scripts_on_play(
    scripts: Query<(Entity, &Name, &super::SoulScriptData)>,
    mut runtime: ResMut<RuneRuntimeState>,
    module_registry: Res<RuneModuleRegistry>,
) {
    let sources: Vec<ScriptSource> = scripts.iter()
        .filter(|(_, _, data)| !data.source.is_empty() && data.run_context == super::SoulRunContext::Rune)
        .map(|(entity, name, data)| ScriptSource {
            entity_index: entity.index(),
            name: name.as_str().to_string(),
            source: data.source.clone(),
        })
        .collect();

    #[cfg(feature = "realism-scripting")]
    {
        eustress_common::soul::rune_runtime::compile_scripts(
            &mut runtime,
            &module_registry,
            &sources,
        );
    }

    let _ = sources; // suppress warning when feature disabled
}

// Legacy stubs for compatibility
pub fn execute_rune_script(_source: &str, _context: &mut super::soul_context::SoulContext) -> Result<(), String> {
    Ok(())
}

pub fn validate_rune_script(_source: &str) -> Result<(), Vec<String>> {
    Ok(())
}

pub fn update_world_state(_world: &World) {}
pub fn update_input_state(_input: &ButtonInput<KeyCode>) {}
pub fn update_mouse_raycast(_ray: Option<Ray3d>) {}
