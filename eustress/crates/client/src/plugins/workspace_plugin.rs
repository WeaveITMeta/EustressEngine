//! # Workspace Plugin (Client)
//! 
//! Registers Workspace service and core classes for the client runtime.

use bevy::prelude::*;
use eustress_common::classes::*;
use eustress_common::services::workspace::*;

#[allow(dead_code)]
pub struct WorkspacePlugin;

impl Plugin for WorkspacePlugin {
    fn build(&self, app: &mut App) {
        app
            // Resource
            .init_resource::<Workspace>()
            .register_type::<Workspace>()
            
            // Core classes
            .register_type::<Instance>()
            .register_type::<BasePart>()
            .register_type::<Part>()
            .register_type::<MeshPart>()
            .register_type::<Model>()
            .register_type::<Folder>()
            .register_type::<Humanoid>()
            
            // Container markers
            .register_type::<ServerStorage>()
            .register_type::<ReplicatedStorage>()
            .register_type::<StarterPack>()
            .register_type::<StarterGui>()
            .register_type::<StarterPlayer>();
    }
}
