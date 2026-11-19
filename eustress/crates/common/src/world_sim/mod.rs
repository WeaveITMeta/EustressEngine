//! # World simulation at scale: prefetch, simulation LOD, the coupled frame
//!
//! What Eustress takes from Rockstar's RAGE, on top of the streaming and
//! physics it already has (`docs/architecture/RAGE_STREAMING_SIM_LOD_JOB_GRAPH.md`):
//!
//! - [`focus`]: the points the world is streamed and simulated around, and
//!   their measured velocities.
//! - [`prefetch`]: predictive prefetch. Cells a focus will need within a
//!   lead time, by deadline, with hysteresis, an I/O budget, a pin set no
//!   evictor may drop, and hit and miss counters.
//! - [`sim_tier`]: simulation LOD policy. Full, Reduced and Dormant tiers by
//!   distance and on-screen size, with hysteresis, a Full cap and whole
//!   physics islands. The play runtime applies it to Avian.
//! - [`frame_graph`]: the frame's world-simulation sets in one explicit
//!   order, and a timeline of physics, world simulation and the Play frame.
//! - [`overlay`]: the Studio overlay that draws all of it.
//!
//! Every system ships with its inspector, Rockstar style: [`inspect_json`]
//! is what the engine bridge's `sim.inspect` returns and the MCP tool
//! `inspect_world_sim` shows.

pub mod focus;
pub mod frame_graph;
pub mod overlay;
pub mod prefetch;
pub mod sim_tier;

use bevy::prelude::*;

pub use focus::{FocusMotion, FocusTracker, StreamFocus};
pub use frame_graph::{configure_world_sim_sets, FrameGraphTimeline, WorldSimSet};
pub use prefetch::{Focus, PrefetchConfig, PrefetchPlanner, PrefetchReport, PrefetchReports};
pub use sim_tier::{SimTier, SimTierConfig, SimTierReport, SIM_TIER_PROP};

/// The shared half of world simulation: its sets, the focus tracker, the
/// reports the inspector reads, the frame timeline and the overlay. Added
/// by every plugin that uses any of it (the instance streamer, the play
/// runtime's simulation LOD); each adds it only if it is not there yet.
pub struct WorldSimCorePlugin;

impl Plugin for WorldSimCorePlugin {
    fn build(&self, app: &mut App) {
        configure_world_sim_sets(app);
        app.register_type::<StreamFocus>()
            .register_type::<SimTier>()
            .init_resource::<FocusTracker>()
            .init_resource::<PrefetchReports>()
            .init_resource::<SimTierReport>();
        frame_graph::add_frame_graph_markers(app);
        overlay::add_overlay(app);
    }
}

/// Add [`WorldSimCorePlugin`] unless an earlier plugin did.
pub fn ensure_core(app: &mut App) {
    if !app.is_plugin_added::<WorldSimCorePlugin>() {
        app.add_plugins(WorldSimCorePlugin);
    }
}

/// Everything the inspector shows, as JSON: each prefetch source's report,
/// simulation LOD's report and config, up to `limit` tiered bodies (with
/// their names and positions), and the frame timeline.
pub fn inspect_json(world: &mut World, limit: usize) -> serde_json::Value {
    let prefetch: Vec<serde_json::Value> = world
        .get_resource::<PrefetchReports>()
        .map(|r| r.by_source.values().map(|p| serde_json::to_value(p).unwrap_or_default()).collect())
        .unwrap_or_default();
    let lod = world.get_resource::<SimTierReport>().map(|r| serde_json::to_value(r).unwrap_or_default());
    let lod_config = world.get_resource::<SimTierConfig>().map(|c| serde_json::to_value(c).unwrap_or_default());
    let timeline = world.get_resource::<FrameGraphTimeline>().map(|t| serde_json::to_value(t.report()).unwrap_or_default());
    let overlay = world.get_resource::<overlay::WorldSimOverlay>().map(|o| o.enabled);

    let mut q = world.query::<(Entity, &SimTier, &GlobalTransform, Option<&Name>)>();
    let mut bodies: Vec<serde_json::Value> = Vec::new();
    let mut total = 0usize;
    for (e, tier, t, name) in q.iter(world) {
        total += 1;
        if bodies.len() < limit {
            bodies.push(serde_json::json!({
                "entity": e.to_bits(),
                "name": name.map(|n| n.as_str().to_string()),
                "tier": tier.as_str(),
                "position": t.translation().to_array(),
            }));
        }
    }
    serde_json::json!({
        "prefetch": prefetch,
        "sim_lod": lod,
        "sim_lod_config": lod_config,
        "tiered_bodies_total": total,
        "tiered_bodies": bodies,
        "frame_graph": timeline,
        "overlay_enabled": overlay,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rage_inspector_lists_tiered_bodies_and_reports() {
        let mut app = App::new();
        ensure_core(&mut app);
        ensure_core(&mut app);
        app.world_mut().spawn((SimTier::Dormant, GlobalTransform::from_xyz(1.0, 2.0, 3.0), Name::new("Crate")));
        app.world_mut().spawn((SimTier::Full, GlobalTransform::default()));
        let v = inspect_json(app.world_mut(), 1);
        assert_eq!(v["tiered_bodies_total"], 2);
        assert_eq!(v["tiered_bodies"].as_array().map(|a| a.len()), Some(1), "limit honoured");
        assert!(v["sim_lod"].is_object());
        assert!(v["frame_graph"].is_object());
    }
}
