//! # The Studio overlay
//!
//! Draws what prefetch and simulation LOD decided, over the 3D view:
//! each predicted cell as a square on the ground (cyan when resident,
//! orange while still loading, brighter the sooner it is needed), and each
//! tiered body as a small sphere (green Full, yellow Reduced, blue
//! Dormant). Off unless `EUSTRESS_SIM_OVERLAY=1`, or the
//! [`WorldSimOverlay`] resource is switched on; it never runs in an app
//! without gizmos.

use bevy::gizmos::config::GizmoConfigStore;
use bevy::prelude::*;

use super::focus::StreamFocus;
use super::prefetch::PrefetchReports;
use super::sim_tier::SimTier;

/// Whether the overlay draws.
#[derive(Resource, Debug, Clone, Copy)]
pub struct WorldSimOverlay {
    pub enabled: bool,
    /// Most tiered bodies drawn per frame.
    pub max_bodies: usize,
}

impl Default for WorldSimOverlay {
    fn default() -> Self {
        let enabled = std::env::var("EUSTRESS_SIM_OVERLAY").map(|v| v.trim() == "1" || v.eq_ignore_ascii_case("on")).unwrap_or(false);
        Self { enabled, max_bodies: 2_000 }
    }
}

fn overlay_on(o: Res<WorldSimOverlay>) -> bool {
    o.enabled
}

/// Add the overlay (drawn only when enabled and gizmos exist).
pub fn add_overlay(app: &mut App) {
    app.init_resource::<WorldSimOverlay>().add_systems(
        PostUpdate,
        draw_world_sim_overlay.run_if(resource_exists::<GizmoConfigStore>).run_if(overlay_on),
    );
}

fn draw_world_sim_overlay(
    mut gizmos: Gizmos,
    overlay: Res<WorldSimOverlay>,
    reports: Option<Res<PrefetchReports>>,
    foci: Query<&GlobalTransform, With<StreamFocus>>,
    bodies: Query<(&GlobalTransform, &SimTier)>,
) {
    let ground_y = foci.iter().next().map_or(0.0, |t| t.translation().y);
    if let Some(reports) = reports {
        for report in reports.by_source.values() {
            for cell in &report.predicted {
                let lead = report.config.as_ref().map_or(2.0, |c| c.lead_time_secs.max(0.01));
                let urgency = (1.0 - (cell.eta_secs / lead)).clamp(0.2, 1.0);
                let color = if cell.resident {
                    Color::srgba(0.1, 0.9, 1.0, urgency)
                } else {
                    Color::srgba(1.0, 0.55, 0.1, urgency)
                };
                let c = Vec3::new(cell.center[0], if cell.center[1] == 0.0 { ground_y } else { cell.center[1] }, cell.center[2]);
                let iso = Isometry3d::new(c, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
                gizmos.rect(iso, Vec2::splat(cell.size * 0.96), color);
            }
            for f in &report.foci {
                let p = Vec3::from_array(f.pos);
                let v = Vec3::from_array(f.vel);
                gizmos.line(p, p + v * f.horizon_secs, Color::srgb(1.0, 1.0, 1.0));
            }
        }
    }
    for (t, tier) in bodies.iter().take(overlay.max_bodies) {
        let color = match tier {
            SimTier::Full => Color::srgb(0.2, 0.9, 0.3),
            SimTier::Reduced => Color::srgb(0.95, 0.85, 0.2),
            SimTier::Dormant => Color::srgb(0.3, 0.45, 1.0),
        };
        gizmos.sphere(Isometry3d::from_translation(t.translation()), 0.6, color);
    }
}
