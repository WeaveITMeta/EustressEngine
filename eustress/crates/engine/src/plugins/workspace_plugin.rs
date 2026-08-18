//! # Workspace Plugin
//! 
//! Registers Workspace service and core Instance classes from common.

use bevy::prelude::*;
use eustress_common::classes::*;
use eustress_common::services::workspace::*;

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
            .register_type::<Model>()
            .register_type::<Folder>()
            .register_type::<Humanoid>()
            
            // Container markers
            .register_type::<ServerStorage>()
            .register_type::<ReplicatedStorage>()
            .register_type::<StarterPack>()
            .register_type::<StarterGui>()
            .register_type::<StarterPlayer>()

            // The one path from Workspace.gravity to Avian's Gravity, unit-
            // converted through `eustress_common::units`. Runs in Update
            // alongside the Rune bridge, which writes Workspace.gravity rather
            // than Gravity directly, so there is exactly one writer of the Avian
            // resource. Cost of Update over a fixed-step schedule is at most one
            // fixed step of lag on a gravity change; gravity is constant in
            // every shipped scene today.
            .add_systems(Update, sync_workspace_gravity_to_avian);
    }
}
