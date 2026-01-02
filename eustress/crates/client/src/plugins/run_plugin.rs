//! # Run Plugin (Client)
//! 
//! Registers RunService and manages game state.

use bevy::prelude::*;
use eustress_common::services::run::*;

#[allow(dead_code)]
pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        // Register types and events
        RunServiceTypes::register(app);
        
        // Set to client mode
        app.insert_resource(RunService::client());
        
        // Systems
        app.add_systems(Update, (
            update_run_service,
            emit_heartbeat,
        ));
    }
}

/// Update RunService timing
#[allow(dead_code)]
fn update_run_service(
    mut run_service: ResMut<RunService>,
    time: Res<Time>,
) {
    run_service.delta_time = time.delta_secs();
    run_service.time_elapsed += time.delta_secs() as f64;
    run_service.frame_count += 1;
}

/// Emit heartbeat events
#[allow(dead_code)]
fn emit_heartbeat(
    run_service: Res<RunService>,
    mut heartbeat: MessageWriter<HeartbeatEvent>,
) {
    if run_service.running {
        heartbeat.write(HeartbeatEvent {
            delta: run_service.delta_time,
        });
    }
}
