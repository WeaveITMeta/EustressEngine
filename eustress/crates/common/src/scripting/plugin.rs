//! # Scripting Services Bevy Plugin
//!
//! Connects the shared ScriptingServices to Bevy's frame loop.
//! Fires Heartbeat, Stepped, and RenderStepped events each frame.
//!
//! ## Table of Contents
//!
//! 1. **ScriptingServicesPlugin** — Bevy plugin for scripting services
//! 2. **Resources** — Bevy resources wrapping shared services
//! 3. **Systems** — Frame event firing and task scheduling

use bevy::prelude::*;
use std::sync::{Arc, RwLock};

use super::services::{RunService, TaskScheduler, DebrisService, TweenService, ScriptingServices, FrameTime};
use super::instance::InstanceRegistry;

// ============================================================================
// 1. Bevy Resources wrapping shared services
// ============================================================================

/// Bevy resource wrapping the shared RunService.
#[derive(Resource)]
pub struct RunServiceResource {
    pub inner: Arc<RwLock<RunService>>,
}

impl Default for RunServiceResource {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RunService::new())),
        }
    }
}

/// Bevy resource wrapping the shared TaskScheduler.
#[derive(Resource)]
pub struct TaskSchedulerResource {
    pub inner: Arc<RwLock<TaskScheduler>>,
}

impl Default for TaskSchedulerResource {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TaskScheduler::new())),
        }
    }
}

/// Bevy resource wrapping the shared DebrisService.
#[derive(Resource)]
pub struct DebrisServiceResource {
    pub inner: Arc<RwLock<DebrisService>>,
}

impl Default for DebrisServiceResource {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DebrisService::new())),
        }
    }
}

/// Bevy resource wrapping the shared TweenService.
#[derive(Resource)]
pub struct TweenServiceResource {
    pub inner: Arc<RwLock<TweenService>>,
}

impl Default for TweenServiceResource {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TweenService::new())),
        }
    }
}

/// Bevy resource wrapping the shared InstanceRegistry.
#[derive(Resource)]
pub struct InstanceRegistryResource {
    pub inner: Arc<RwLock<InstanceRegistry>>,
}

impl Default for InstanceRegistryResource {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InstanceRegistry::new())),
        }
    }
}

/// Combined scripting services resource for easy access.
#[derive(Resource)]
pub struct ScriptingServicesResource {
    pub run_service: Arc<RwLock<RunService>>,
    pub task_scheduler: Arc<RwLock<TaskScheduler>>,
    pub debris_service: Arc<RwLock<DebrisService>>,
    pub tween_service: Arc<RwLock<TweenService>>,
    pub instance_registry: Arc<RwLock<InstanceRegistry>>,
}

impl Default for ScriptingServicesResource {
    fn default() -> Self {
        Self {
            run_service: Arc::new(RwLock::new(RunService::new())),
            task_scheduler: Arc::new(RwLock::new(TaskScheduler::new())),
            debris_service: Arc::new(RwLock::new(DebrisService::new())),
            tween_service: Arc::new(RwLock::new(TweenService::new())),
            instance_registry: Arc::new(RwLock::new(InstanceRegistry::new())),
        }
    }
}

// ============================================================================
// 2. Bevy Messages for frame events
// ============================================================================

/// Message fired every frame after physics (Heartbeat equivalent).
#[derive(Message, Debug, Clone)]
pub struct HeartbeatEvent {
    /// Time since last frame in seconds
    pub delta_time: f64,
    /// Total elapsed time since start
    pub elapsed_time: f64,
}

/// Message fired every physics step (Stepped equivalent).
#[derive(Message, Debug, Clone)]
pub struct SteppedEvent {
    /// Fixed timestep delta
    pub delta_time: f64,
    /// Total simulation time
    pub simulation_time: f64,
}

/// Message fired before rendering (RenderStepped equivalent).
#[derive(Message, Debug, Clone)]
pub struct RenderSteppedEvent {
    /// Time since last render frame
    pub delta_time: f64,
}

// ============================================================================
// 3. ScriptingServicesPlugin
// ============================================================================

/// Bevy plugin that connects ScriptingServices to the frame loop.
/// 
/// Fires:
/// - `HeartbeatEvent` every frame in Update
/// - `SteppedEvent` every fixed timestep in FixedUpdate
/// - `RenderSteppedEvent` before rendering in PreUpdate
pub struct ScriptingServicesPlugin;

impl Plugin for ScriptingServicesPlugin {
    fn build(&self, app: &mut App) {
        // Initialize resources
        let services = ScriptingServicesResource::default();
        
        app.insert_resource(RunServiceResource {
            inner: services.run_service.clone(),
        });
        app.insert_resource(TaskSchedulerResource {
            inner: services.task_scheduler.clone(),
        });
        app.insert_resource(DebrisServiceResource {
            inner: services.debris_service.clone(),
        });
        app.insert_resource(TweenServiceResource {
            inner: services.tween_service.clone(),
        });
        app.insert_resource(InstanceRegistryResource {
            inner: services.instance_registry.clone(),
        });
        app.insert_resource(services);

        // Register messages
        app.add_message::<HeartbeatEvent>();
        app.add_message::<SteppedEvent>();
        app.add_message::<RenderSteppedEvent>();

        // Add systems
        app.add_systems(PreUpdate, fire_render_stepped);
        app.add_systems(Update, (
            fire_heartbeat,
            process_task_scheduler,
            process_debris_service,
            process_tween_service,
        ));
        app.add_systems(FixedUpdate, fire_stepped);

        tracing::info!("ScriptingServicesPlugin initialized — RunService, TaskScheduler, Debris, Tween ready");
    }
}

// ============================================================================
// 4. Systems
// ============================================================================

/// Fire Heartbeat event every frame (after physics).
fn fire_heartbeat(
    time: Res<Time>,
    run_service: Res<RunServiceResource>,
    mut heartbeat_events: MessageWriter<HeartbeatEvent>,
) {
    let delta = time.delta_secs_f64();
    let elapsed = time.elapsed_secs_f64();

    // Fire heartbeat signal on RunService
    if let Ok(service) = run_service.inner.read() {
        service.heartbeat().fire(FrameTime { delta_time: delta, elapsed_time: elapsed });
    }

    // Fire the event
    heartbeat_events.write(HeartbeatEvent {
        delta_time: delta,
        elapsed_time: elapsed,
    });
}

/// Fire Stepped event every fixed timestep.
fn fire_stepped(
    time: Res<Time<Fixed>>,
    mut stepped_events: MessageWriter<SteppedEvent>,
) {
    let delta = time.delta_secs_f64();
    let elapsed = time.elapsed_secs_f64();

    stepped_events.write(SteppedEvent {
        delta_time: delta,
        simulation_time: elapsed,
    });
}

/// Fire RenderStepped event before rendering.
fn fire_render_stepped(
    time: Res<Time>,
    mut render_events: MessageWriter<RenderSteppedEvent>,
) {
    let delta = time.delta_secs_f64();

    render_events.write(RenderSteppedEvent {
        delta_time: delta,
    });
}

/// Process the task scheduler each frame.
fn process_task_scheduler(
    time: Res<Time>,
    task_scheduler: Res<TaskSchedulerResource>,
) {
    let delta = time.delta_secs_f64();

    if let Ok(scheduler) = task_scheduler.inner.read() {
        scheduler.process_delayed();
        scheduler.process_deferred();
    }
}

/// Process the debris service each frame.
fn process_debris_service(
    time: Res<Time>,
    debris_service: Res<DebrisServiceResource>,
    instance_registry: Res<InstanceRegistryResource>,
) {
    let delta = time.delta_secs_f64();

    if let Ok(debris) = debris_service.inner.read() {
        // Process debris items (destroys expired entities via callback)
        debris.process();
    }
}

/// Process the tween service each frame.
fn process_tween_service(
    time: Res<Time>,
    tween_service: Res<TweenServiceResource>,
) {
    let delta = time.delta_secs_f64();

    if let Ok(tweens) = tween_service.inner.read() {
        tweens.update();
    }
}

// ============================================================================
// 5. Helper functions for script execution
// ============================================================================

/// Get the instance registry Arc for passing to Rune scripts.
pub fn get_instance_registry(services: &ScriptingServicesResource) -> Arc<RwLock<InstanceRegistry>> {
    services.instance_registry.clone()
}

/// Get the run service Arc for script access.
pub fn get_run_service(services: &ScriptingServicesResource) -> Arc<RwLock<RunService>> {
    services.run_service.clone()
}

/// Get the task scheduler Arc for script access.
pub fn get_task_scheduler(services: &ScriptingServicesResource) -> Arc<RwLock<TaskScheduler>> {
    services.task_scheduler.clone()
}
