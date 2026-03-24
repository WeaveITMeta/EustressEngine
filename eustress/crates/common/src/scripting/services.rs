//! # Scripting Services
//!
//! Roblox-compatible service wrappers for scripting.
//! These provide the runtime infrastructure for game loops, timing, and scheduling.
//!
//! ## Table of Contents
//!
//! 1. **RunService** — Game loop events (Heartbeat, Stepped, RenderStepped)
//! 2. **task** — Coroutine scheduling (wait, spawn, defer, delay)
//! 3. **Debris** — Automatic cleanup service
//! 4. **TweenService** — Property animation

use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering as CmpOrdering;
use std::time::{Duration, Instant};

use super::events::{Signal, Connection, SignalArg};
use super::types::TweenInfo;

// ============================================================================
// 1. RunService — Game Loop Events
// ============================================================================

/// Delta time argument for frame events
#[derive(Debug, Clone, Copy)]
pub struct FrameTime {
    /// Time since last frame in seconds
    pub delta_time: f64,
    /// Total elapsed time since start
    pub elapsed_time: f64,
}

impl Default for FrameTime {
    fn default() -> Self {
        Self {
            delta_time: 0.016, // ~60fps
            elapsed_time: 0.0,
        }
    }
}

/// RunService provides game loop events matching Roblox API.
/// 
/// Events fire in this order each frame:
/// 1. Stepped (before physics)
/// 2. Physics simulation
/// 3. Heartbeat (after physics)
/// 4. RenderStepped (before render, client only)
#[derive(Clone)]
pub struct RunService {
    /// Fires every frame after physics simulation
    heartbeat: Signal<FrameTime>,
    /// Fires every frame before physics simulation
    stepped: Signal<FrameTime>,
    /// Fires every frame before rendering (client only)
    render_stepped: Signal<FrameTime>,
    /// Bound render step callbacks with priority
    render_step_bindings: Arc<Mutex<HashMap<String, (i32, Box<dyn Fn(f64) + Send + Sync>)>>>,
    /// Whether the game is running
    running: Arc<AtomicBool>,
    /// Whether this is a client context
    is_client: Arc<AtomicBool>,
    /// Whether this is a server context
    is_server: Arc<AtomicBool>,
    /// Whether running in Studio
    is_studio: Arc<AtomicBool>,
    /// Start time
    start_time: Arc<Mutex<Option<Instant>>>,
}

impl Default for RunService {
    fn default() -> Self {
        Self::new()
    }
}

impl RunService {
    pub fn new() -> Self {
        Self {
            heartbeat: Signal::new(),
            stepped: Signal::new(),
            render_stepped: Signal::new(),
            render_step_bindings: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            is_client: Arc::new(AtomicBool::new(true)),
            is_server: Arc::new(AtomicBool::new(false)),
            is_studio: Arc::new(AtomicBool::new(true)),
            start_time: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the Heartbeat signal (fires after physics each frame)
    pub fn heartbeat(&self) -> &Signal<FrameTime> {
        &self.heartbeat
    }

    /// Get the Stepped signal (fires before physics each frame)
    pub fn stepped(&self) -> &Signal<FrameTime> {
        &self.stepped
    }

    /// Get the RenderStepped signal (fires before render, client only)
    pub fn render_stepped(&self) -> &Signal<FrameTime> {
        &self.render_stepped
    }

    /// Bind a function to RenderStepped with a priority
    /// Lower priority numbers run first
    pub fn bind_to_render_step<F>(&self, name: &str, priority: i32, callback: F)
    where
        F: Fn(f64) + Send + Sync + 'static,
    {
        let mut bindings = self.render_step_bindings.lock().unwrap();
        bindings.insert(name.to_string(), (priority, Box::new(callback)));
    }

    /// Unbind a function from RenderStepped
    pub fn unbind_from_render_step(&self, name: &str) {
        let mut bindings = self.render_step_bindings.lock().unwrap();
        bindings.remove(name);
    }

    /// Check if running on client
    pub fn is_client(&self) -> bool {
        self.is_client.load(Ordering::SeqCst)
    }

    /// Check if running on server
    pub fn is_server(&self) -> bool {
        self.is_server.load(Ordering::SeqCst)
    }

    /// Check if running in Studio
    pub fn is_studio(&self) -> bool {
        self.is_studio.load(Ordering::SeqCst)
    }

    /// Check if the game is running (not paused/stopped)
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Set running state
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::SeqCst);
        if running {
            let mut start = self.start_time.lock().unwrap();
            if start.is_none() {
                *start = Some(Instant::now());
            }
        }
    }

    /// Set client/server context
    pub fn set_context(&self, is_client: bool, is_server: bool, is_studio: bool) {
        self.is_client.store(is_client, Ordering::SeqCst);
        self.is_server.store(is_server, Ordering::SeqCst);
        self.is_studio.store(is_studio, Ordering::SeqCst);
    }

    /// Fire Stepped event (call from engine before physics)
    pub fn fire_stepped(&self, delta_time: f64) {
        let elapsed = self.elapsed_time();
        self.stepped.fire(FrameTime { delta_time, elapsed_time: elapsed });
    }

    /// Fire Heartbeat event (call from engine after physics)
    pub fn fire_heartbeat(&self, delta_time: f64) {
        let elapsed = self.elapsed_time();
        self.heartbeat.fire(FrameTime { delta_time, elapsed_time: elapsed });
    }

    /// Fire RenderStepped event (call from engine before render)
    pub fn fire_render_stepped(&self, delta_time: f64) {
        let elapsed = self.elapsed_time();
        
        // Fire bound callbacks in priority order
        let bindings = self.render_step_bindings.lock().unwrap();
        let mut sorted: Vec<_> = bindings.iter().collect();
        sorted.sort_by_key(|(_, (priority, _))| *priority);
        for (_, (_, callback)) in sorted {
            callback(delta_time);
        }
        
        // Fire signal
        self.render_stepped.fire(FrameTime { delta_time, elapsed_time: elapsed });
    }

    /// Get elapsed time since start
    pub fn elapsed_time(&self) -> f64 {
        let start = self.start_time.lock().unwrap();
        start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0)
    }
}

// ============================================================================
// 2. task — Coroutine Scheduling
// ============================================================================

/// Task identifier
pub type TaskId = u64;

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

fn next_task_id() -> TaskId {
    NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst)
}

/// A scheduled task
struct ScheduledTask {
    id: TaskId,
    execute_at: Instant,
    callback: Box<dyn FnOnce() + Send + 'static>,
}

impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ScheduledTask {}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Reverse order for min-heap behavior
        other.execute_at.cmp(&self.execute_at)
    }
}

/// Task scheduler matching Roblox task library.
/// Provides wait, spawn, defer, delay, and cancel.
#[derive(Clone)]
pub struct TaskScheduler {
    /// Pending delayed tasks (min-heap by execute_at)
    delayed_tasks: Arc<Mutex<BinaryHeap<ScheduledTask>>>,
    /// Deferred tasks (run at end of frame)
    deferred_tasks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>>,
    /// Cancelled task IDs
    cancelled: Arc<Mutex<std::collections::HashSet<TaskId>>>,
    /// Current frame time for wait calculations
    current_time: Arc<Mutex<Instant>>,
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            delayed_tasks: Arc::new(Mutex::new(BinaryHeap::new())),
            deferred_tasks: Arc::new(Mutex::new(Vec::new())),
            cancelled: Arc::new(Mutex::new(std::collections::HashSet::new())),
            current_time: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Wait for n seconds (blocking in current thread)
    /// Returns actual time waited
    pub fn wait(&self, seconds: f64) -> f64 {
        let duration = Duration::from_secs_f64(seconds.max(0.0));
        let start = Instant::now();
        std::thread::sleep(duration);
        start.elapsed().as_secs_f64()
    }

    /// Spawn a new task immediately (runs in parallel)
    /// Returns task ID that can be cancelled
    pub fn spawn<F>(&self, callback: F) -> TaskId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = next_task_id();
        let cancelled = self.cancelled.clone();
        
        std::thread::spawn(move || {
            // Check if cancelled before running
            if !cancelled.lock().unwrap().contains(&id) {
                callback();
            }
        });
        
        id
    }

    /// Defer a task to run at the end of the current frame
    /// Returns task ID
    pub fn defer<F>(&self, callback: F) -> TaskId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = next_task_id();
        let cancelled = self.cancelled.clone();
        
        let wrapped = move || {
            if !cancelled.lock().unwrap().contains(&id) {
                callback();
            }
        };
        
        let mut deferred = self.deferred_tasks.lock().unwrap();
        deferred.push(Box::new(wrapped));
        
        id
    }

    /// Delay a task by n seconds
    /// Returns task ID that can be cancelled
    pub fn delay<F>(&self, seconds: f64, callback: F) -> TaskId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = next_task_id();
        let execute_at = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        let cancelled = self.cancelled.clone();
        
        let wrapped = move || {
            if !cancelled.lock().unwrap().contains(&id) {
                callback();
            }
        };
        
        let task = ScheduledTask {
            id,
            execute_at,
            callback: Box::new(wrapped),
        };
        
        let mut delayed = self.delayed_tasks.lock().unwrap();
        delayed.push(task);
        
        id
    }

    /// Cancel a task by ID
    pub fn cancel(&self, task_id: TaskId) {
        let mut cancelled = self.cancelled.lock().unwrap();
        cancelled.insert(task_id);
    }

    /// Process deferred tasks (call at end of frame)
    pub fn process_deferred(&self) {
        let tasks: Vec<_> = {
            let mut deferred = self.deferred_tasks.lock().unwrap();
            std::mem::take(&mut *deferred)
        };
        
        for task in tasks {
            task();
        }
    }

    /// Process delayed tasks that are ready (call each frame)
    pub fn process_delayed(&self) {
        let now = Instant::now();
        *self.current_time.lock().unwrap() = now;
        
        let mut ready_tasks = Vec::new();
        
        {
            let mut delayed = self.delayed_tasks.lock().unwrap();
            while let Some(task) = delayed.peek() {
                if task.execute_at <= now {
                    ready_tasks.push(delayed.pop().unwrap());
                } else {
                    break;
                }
            }
        }
        
        // Execute ready tasks
        for task in ready_tasks {
            let cancelled = self.cancelled.lock().unwrap();
            if !cancelled.contains(&task.id) {
                drop(cancelled); // Release lock before callback
                (task.callback)();
            }
        }
        
        // Clean up old cancelled IDs periodically
        let mut cancelled = self.cancelled.lock().unwrap();
        if cancelled.len() > 1000 {
            cancelled.clear();
        }
    }

    /// Synchronize (placeholder for Parallel Luau)
    pub fn synchronize(&self) {
        // In Roblox, this switches from parallel to serial execution
        // For now, this is a no-op
    }

    /// Desynchronize (placeholder for Parallel Luau)
    pub fn desynchronize(&self) {
        // In Roblox, this switches from serial to parallel execution
        // For now, this is a no-op
    }
}

// ============================================================================
// 3. Debris — Automatic Cleanup Service
// ============================================================================

/// Item scheduled for destruction
struct DebrisItem {
    entity_id: u64,
    destroy_at: Instant,
}

/// Debris service for automatic cleanup of entities.
#[derive(Clone)]
pub struct DebrisService {
    items: Arc<Mutex<Vec<DebrisItem>>>,
    /// Callback to destroy entities (set by engine)
    destroy_callback: Arc<Mutex<Option<Box<dyn Fn(u64) + Send + Sync>>>>,
}

impl Default for DebrisService {
    fn default() -> Self {
        Self::new()
    }
}

impl DebrisService {
    pub fn new() -> Self {
        Self {
            items: Arc::new(Mutex::new(Vec::new())),
            destroy_callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the callback used to destroy entities
    pub fn set_destroy_callback<F>(&self, callback: F)
    where
        F: Fn(u64) + Send + Sync + 'static,
    {
        let mut cb = self.destroy_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Add an entity to be destroyed after `lifetime` seconds
    pub fn add_item(&self, entity_id: u64, lifetime: f64) {
        let destroy_at = Instant::now() + Duration::from_secs_f64(lifetime.max(0.0));
        let mut items = self.items.lock().unwrap();
        items.push(DebrisItem { entity_id, destroy_at });
    }

    /// Process and destroy items that have expired (call each frame)
    pub fn process(&self) {
        let now = Instant::now();
        let mut to_destroy = Vec::new();
        
        {
            let mut items = self.items.lock().unwrap();
            items.retain(|item| {
                if item.destroy_at <= now {
                    to_destroy.push(item.entity_id);
                    false
                } else {
                    true
                }
            });
        }
        
        // Destroy expired items
        let callback = self.destroy_callback.lock().unwrap();
        if let Some(destroy) = callback.as_ref() {
            for entity_id in to_destroy {
                destroy(entity_id);
            }
        }
    }
}

// ============================================================================
// 4. TweenService — Property Animation
// ============================================================================

/// Tween state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweenStatus {
    Playing,
    Paused,
    Cancelled,
    Completed,
}

/// A single tween animation
pub struct Tween {
    id: u64,
    info: TweenInfo,
    status: Arc<Mutex<TweenStatus>>,
    start_time: Arc<Mutex<Option<Instant>>>,
    pause_time: Arc<Mutex<Option<Instant>>>,
    paused_elapsed: Arc<Mutex<f64>>,
    completed_signal: Signal<()>,
    /// Current repeat iteration
    current_repeat: Arc<Mutex<i32>>,
    /// Whether currently in reverse direction
    reversing: Arc<Mutex<bool>>,
    /// Property update callback
    update_callback: Arc<Mutex<Option<Box<dyn Fn(f64) + Send + Sync>>>>,
}

static NEXT_TWEEN_ID: AtomicU64 = AtomicU64::new(1);

impl Tween {
    fn new(info: TweenInfo) -> Self {
        Self {
            id: NEXT_TWEEN_ID.fetch_add(1, Ordering::SeqCst),
            info,
            status: Arc::new(Mutex::new(TweenStatus::Paused)),
            start_time: Arc::new(Mutex::new(None)),
            pause_time: Arc::new(Mutex::new(None)),
            paused_elapsed: Arc::new(Mutex::new(0.0)),
            completed_signal: Signal::new(),
            current_repeat: Arc::new(Mutex::new(0)),
            reversing: Arc::new(Mutex::new(false)),
            update_callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Get tween ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get current status
    pub fn status(&self) -> TweenStatus {
        *self.status.lock().unwrap()
    }

    /// Get Completed signal
    pub fn completed(&self) -> &Signal<()> {
        &self.completed_signal
    }

    /// Set the update callback (called with alpha 0-1)
    pub fn set_update_callback<F>(&self, callback: F)
    where
        F: Fn(f64) + Send + Sync + 'static,
    {
        let mut cb = self.update_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Play the tween
    pub fn play(&self) {
        let mut status = self.status.lock().unwrap();
        if *status == TweenStatus::Paused {
            *status = TweenStatus::Playing;
            
            let mut start = self.start_time.lock().unwrap();
            if start.is_none() {
                // First play - apply delay
                *start = Some(Instant::now() + Duration::from_secs_f64(self.info.delay_time));
            } else {
                // Resuming from pause
                let pause = self.pause_time.lock().unwrap();
                if let Some(pause_instant) = *pause {
                    let paused_duration = pause_instant.elapsed().as_secs_f64();
                    *self.paused_elapsed.lock().unwrap() += paused_duration;
                }
            }
            
            *self.pause_time.lock().unwrap() = None;
        }
    }

    /// Pause the tween
    pub fn pause(&self) {
        let mut status = self.status.lock().unwrap();
        if *status == TweenStatus::Playing {
            *status = TweenStatus::Paused;
            *self.pause_time.lock().unwrap() = Some(Instant::now());
        }
    }

    /// Cancel the tween
    pub fn cancel(&self) {
        let mut status = self.status.lock().unwrap();
        *status = TweenStatus::Cancelled;
    }

    /// Update the tween (call each frame)
    /// Returns true if still active, false if completed/cancelled
    pub fn update(&self) -> bool {
        let status = *self.status.lock().unwrap();
        if status != TweenStatus::Playing {
            return status != TweenStatus::Completed && status != TweenStatus::Cancelled;
        }

        let start = *self.start_time.lock().unwrap();
        let Some(start_instant) = start else {
            return true;
        };

        // Check if still in delay
        if Instant::now() < start_instant {
            return true;
        }

        let paused_elapsed = *self.paused_elapsed.lock().unwrap();
        let elapsed = start_instant.elapsed().as_secs_f64() - paused_elapsed;
        
        let duration = self.info.time;
        let mut alpha = if duration > 0.0 { elapsed / duration } else { 1.0 };

        // Handle reversing
        let reversing = *self.reversing.lock().unwrap();
        if reversing {
            alpha = 1.0 - alpha;
        }

        // Clamp and apply easing
        alpha = alpha.clamp(0.0, 1.0);
        let eased_alpha = self.info.ease(alpha);

        // Call update callback
        let callback = self.update_callback.lock().unwrap();
        if let Some(cb) = callback.as_ref() {
            cb(eased_alpha);
        }

        // Check if iteration complete
        if elapsed >= duration {
            let mut current_repeat = self.current_repeat.lock().unwrap();
            let repeat_count = self.info.repeat_count;

            if self.info.reverses {
                let mut reversing = self.reversing.lock().unwrap();
                *reversing = !*reversing;
                
                if !*reversing {
                    // Completed a full cycle
                    *current_repeat += 1;
                }
            } else {
                *current_repeat += 1;
            }

            // Check if all repeats done
            if repeat_count >= 0 && *current_repeat > repeat_count {
                let mut status = self.status.lock().unwrap();
                *status = TweenStatus::Completed;
                self.completed_signal.fire(());
                return false;
            }

            // Reset for next iteration
            *self.start_time.lock().unwrap() = Some(Instant::now());
            *self.paused_elapsed.lock().unwrap() = 0.0;
        }

        true
    }
}

impl Clone for Tween {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            info: self.info,
            status: self.status.clone(),
            start_time: self.start_time.clone(),
            pause_time: self.pause_time.clone(),
            paused_elapsed: self.paused_elapsed.clone(),
            completed_signal: self.completed_signal.clone(),
            current_repeat: self.current_repeat.clone(),
            reversing: self.reversing.clone(),
            update_callback: self.update_callback.clone(),
        }
    }
}

/// TweenService for creating and managing tweens
#[derive(Clone)]
pub struct TweenService {
    active_tweens: Arc<Mutex<HashMap<u64, Tween>>>,
}

impl Default for TweenService {
    fn default() -> Self {
        Self::new()
    }
}

impl TweenService {
    pub fn new() -> Self {
        Self {
            active_tweens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new tween
    pub fn create(&self, info: TweenInfo) -> Tween {
        let tween = Tween::new(info);
        let mut tweens = self.active_tweens.lock().unwrap();
        tweens.insert(tween.id, tween.clone());
        tween
    }

    /// Update all active tweens (call each frame)
    pub fn update(&self) {
        let mut to_remove = Vec::new();
        
        {
            let tweens = self.active_tweens.lock().unwrap();
            for (id, tween) in tweens.iter() {
                if !tween.update() {
                    to_remove.push(*id);
                }
            }
        }
        
        // Remove completed tweens
        if !to_remove.is_empty() {
            let mut tweens = self.active_tweens.lock().unwrap();
            for id in to_remove {
                tweens.remove(&id);
            }
        }
    }

    /// Get number of active tweens
    pub fn active_count(&self) -> usize {
        self.active_tweens.lock().unwrap().len()
    }
}

// ============================================================================
// 5. Global Service Registry
// ============================================================================

/// Global registry of all scripting services
#[derive(Clone)]
pub struct ScriptingServices {
    pub run_service: RunService,
    pub task: TaskScheduler,
    pub debris: DebrisService,
    pub tween_service: TweenService,
}

impl Default for ScriptingServices {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptingServices {
    pub fn new() -> Self {
        Self {
            run_service: RunService::new(),
            task: TaskScheduler::new(),
            debris: DebrisService::new(),
            tween_service: TweenService::new(),
        }
    }

    /// Process all services (call each frame)
    pub fn update(&self, delta_time: f64) {
        // Process delayed tasks
        self.task.process_delayed();
        
        // Process debris
        self.debris.process();
        
        // Update tweens
        self.tween_service.update();
        
        // Fire heartbeat (after physics, which should be called separately)
        // Note: Stepped should be called before physics by the engine
        self.run_service.fire_heartbeat(delta_time);
    }

    /// Process deferred tasks (call at end of frame)
    pub fn end_frame(&self) {
        self.task.process_deferred();
    }

    /// Fire stepped event (call before physics)
    pub fn pre_physics(&self, delta_time: f64) {
        self.run_service.fire_stepped(delta_time);
    }

    /// Fire render stepped event (call before render)
    pub fn pre_render(&self, delta_time: f64) {
        self.run_service.fire_render_stepped(delta_time);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    #[test]
    fn test_run_service_heartbeat() {
        let run_service = RunService::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let _conn = run_service.heartbeat().connect(move |frame| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            assert!(frame.delta_time > 0.0);
        });

        run_service.set_running(true);
        run_service.fire_heartbeat(0.016);
        run_service.fire_heartbeat(0.016);

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_task_defer() {
        let scheduler = TaskScheduler::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        scheduler.defer(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        scheduler.process_deferred();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_task_cancel() {
        let scheduler = TaskScheduler::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let task_id = scheduler.defer(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        scheduler.cancel(task_id);
        scheduler.process_deferred();
        
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_tween_linear() {
        let service = TweenService::new();
        let info = TweenInfo {
            time: 0.1,
            ..Default::default()
        };
        
        let tween = service.create(info);
        let value = Arc::new(Mutex::new(0.0));
        let value_clone = value.clone();
        
        tween.set_update_callback(move |alpha| {
            *value_clone.lock().unwrap() = alpha;
        });
        
        tween.play();
        
        // Simulate some updates
        std::thread::sleep(Duration::from_millis(50));
        tween.update();
        
        let current = *value.lock().unwrap();
        assert!(current > 0.0 && current <= 1.0);
    }
}
