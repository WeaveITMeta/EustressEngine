//! Bevy integration for EustressStream.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use eustress_stream::{EustressStreamPlugin, StreamConfig, EustressStream};
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(EustressStreamPlugin::new(StreamConfig::default()))
//!     .add_systems(Update, my_system)
//!     .run();
//!
//! fn my_system(stream: Res<EustressStream>) {
//!     let producer = stream.producer("scene-updates");
//!     producer.send_bytes(bytes::Bytes::from_static(b"delta"));
//! }
//! ```

use bevy::prelude::*;

use crate::config::StreamConfig;
use crate::stream::EustressStream;

// ─────────────────────────────────────────────────────────────────────────────
// Resource: StreamMetrics — updated each frame
// ─────────────────────────────────────────────────────────────────────────────

/// Per-topic streaming diagnostics exposed as a Bevy Resource.
#[derive(Resource, Default, Debug)]
pub struct StreamMetrics {
    /// Messages published this frame (across all topics).
    pub messages_this_frame: u64,
    /// Cumulative messages published since startup.
    pub messages_total: u64,
    /// Bytes written to storage this frame.
    pub bytes_this_frame: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// EustressStreamPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Adds `EustressStream` as a Bevy `Resource` and registers diagnostics.
pub struct EustressStreamPlugin {
    config: StreamConfig,
}

impl EustressStreamPlugin {
    pub fn new(config: StreamConfig) -> Self {
        Self { config }
    }
}

impl Default for EustressStreamPlugin {
    fn default() -> Self {
        Self { config: StreamConfig::default() }
    }
}

impl Plugin for EustressStreamPlugin {
    fn build(&self, app: &mut App) {
        let stream = EustressStream::new(self.config.clone());
        app
            .insert_resource(stream)
            .init_resource::<StreamMetrics>()
            .add_systems(Last, reset_frame_metrics);
    }
}

fn reset_frame_metrics(mut metrics: ResMut<StreamMetrics>) {
    metrics.messages_this_frame = 0;
    metrics.bytes_this_frame    = 0;
}

// ─────────────────────────────────────────────────────────────────────────────
// SystemParam helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convenience type alias for reading the stream from a Bevy system.
pub type StreamRef<'w> = Res<'w, EustressStream>;

/// Convenience: subscribe inside a Startup system and store the id.
///
/// ```rust,no_run
/// fn setup(stream: Res<EustressStream>, mut commands: Commands) {
///     let id = stream.subscribe("events", |view| {
///         println!("{}", view.offset);
///     }).unwrap();
///     commands.insert_resource(MySubscription(id));
/// }
/// ```
#[derive(Resource)]
pub struct SubscriptionHandle {
    pub topic: String,
    pub id:    crate::topic::SubscriberId,
}
