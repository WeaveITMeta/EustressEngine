//! Portal — free-camera Space↔Space teleport for Studio.
//!
//! A portal is **not a new class**. It is any ordinary instance (in practice a
//! `Part`) carrying an `[attributes]` table:
//!
//! ```toml
//! [attributes]
//! portal_target = "Temple"             # destination Space NAME
//! portal_radius = 7.0                  # trigger radius, metres (default 6.0)
//! portal_arrival = [0.0, 8.0, 52.0]    # camera pivot on arrival (optional)
//! portal_arrival_yaw = 180.0           # arrival yaw, degrees (optional)
//! portal_label = "Water"               # shown in the toast (optional)
//! ```
//!
//! Only `portal_target` is required; everything else has a default.
//!
//! ## Why attributes and not `scene::PortalData`
//!
//! [`eustress_common::scene::PortalData`] exists but is **never constructed
//! anywhere in the workspace** — it belongs to the deprecated, UI-unreachable
//! `.scene.json` path. Reviving it would mean reviving that path. `[attributes]`
//! is already parsed off `_instance.toml` into a live [`Attributes`] component by
//! `instance_loader::attributes_from_toml_table`, so a portal needs zero schema
//! changes, survives save/load like any other instance, and is editable from the
//! Properties panel.
//!
//! ## Trigger model
//!
//! The **Studio free camera** flying inside the radius fires the jump. This is an
//! editor navigation aid, not a Play-mode mechanic — Play mode has its own
//! character and is deliberately untouched.
//!
//! Two guards stop a portal from bouncing you straight back, since an arrival
//! point necessarily sits near the destination's return portal:
//!
//! 1. **Arm-on-clear.** The trigger arms only once the camera is outside *every*
//!    portal volume (scaled by [`REARM_FACTOR`] for hysteresis). State starts
//!    disarmed, so a camera that begins inside a volume cannot fire on frame one.
//! 2. **Cooldown.** [`COOLDOWN_SECS`] of dead time covers the reload frames.
//!
//! The arrival pose is re-asserted for [`ARRIVAL_HOLD_FRAMES`] frames, so a
//! space-load path that resets the camera to a default view cannot clobber it.
//!
//! ## Why the [`IsPortal`] marker exists
//!
//! `instance_loader` inserts an `Attributes` component on **every** instance,
//! empty or not (deliberately — see `attributes_from_toml_table`, it makes the
//! live component a faithful mirror of disk for all classes). So a naive
//! `Query<(&GlobalTransform, &Attributes)>` matches the entire scene, and the
//! per-frame proximity test would be O(entities) with a hash lookup each — on a
//! ~119k-entity Space that is a real frame-time regression for a feature that
//! concerns ~16 objects. [`sync_portal_markers`] therefore tags the actual
//! portals via `Changed<Attributes>` (which fires on insert, so newly loaded
//! portals are tagged the frame after a load, and Properties-panel edits are
//! picked up too), and the hot path queries `With<IsPortal>` — O(portals).

use bevy::prelude::*;
use eustress_common::{AttributeValue, Attributes};
use std::path::PathBuf;

use crate::camera_controller::EustressCamera;
use crate::notifications::NotificationManager;
use crate::space::{SpaceRoot, UniverseRegistry};

/// Trigger radius when a portal declares no `portal_radius`.
const DEFAULT_RADIUS: f32 = 6.0;
/// The camera must leave a volume by this factor before portals re-arm.
const REARM_FACTOR: f32 = 1.6;
/// Dead time after a jump, in seconds.
const COOLDOWN_SECS: f32 = 1.25;
/// How long to keep re-asserting the arrival pose after a jump, in SECONDS of
/// wall clock — not frames.
///
/// `open_space` despawns the editor camera, and `camera_controller::
/// ensure_camera_exists` respawns it at `pivot = Vec3::ZERO, distance = 20`.
/// For the Temple that default is *inside* the rotunda — buried in the Ledger
/// block, ringed by columns — so losing the arrival pose does not merely look
/// wrong, it strands the camera inside geometry. A frame count (this was 8)
/// cannot cover that: a big Space streams in for a minute or more and the
/// respawn lands somewhere inside it, long after eight frames have burned. A
/// wall-clock window re-asserts the pose across the respawn regardless of how
/// slowly the load is ticking.
const ARRIVAL_HOLD_SECS: f32 = 5.0;
/// Orbit distance the camera lands at.
const ARRIVAL_DISTANCE: f32 = 26.0;
/// Arrival pitch, radians. Slightly above level so a wing reads down its axis.
const ARRIVAL_PITCH: f32 = 0.08;

/// Marks an instance whose `[attributes]` currently declare a usable
/// `portal_target`. Maintained by [`sync_portal_markers`]; never authored.
#[derive(Component)]
pub struct IsPortal;

/// Attribute key that makes an instance a portal.
pub const ATTR_TARGET: &str = "portal_target";
pub const ATTR_RADIUS: &str = "portal_radius";
pub const ATTR_ARRIVAL: &str = "portal_arrival";
pub const ATTR_ARRIVAL_YAW: &str = "portal_arrival_yaw";
pub const ATTR_LABEL: &str = "portal_label";

#[derive(Resource, Debug)]
pub struct PortalState {
    /// A portal can only fire while armed; see the arm-on-clear rule above.
    armed: bool,
    cooldown: f32,
    /// `(pivot, yaw_degrees)` to force onto the camera once the jump has landed.
    pending_arrival: Option<(Vec3, Option<f32>)>,
    /// Seconds of wall clock left to keep WATCHING for a camera that still
    /// needs the arrival pose. Not a period of continuous re-assertion — see
    /// [`apply_portal_arrival`].
    arrival_hold: f32,
    /// The camera the arrival pose has already been written to. A camera
    /// despawned and respawned by the space load comes back with a NEW
    /// `Entity` id, and that difference is the signal to write the pose again.
    /// Without this the system rewrote the pose every frame for the whole hold
    /// window, overwriting the user's own mouse-look and WASD — the jump
    /// landed correctly and then the camera fought you for five seconds.
    arrival_applied_to: Option<Entity>,
    /// Set false by [`PortalPlugin`] consumers that want portals inert (e.g. a
    /// future Play-mode guard). Public so the ribbon can expose a toggle later.
    pub enabled: bool,
}

impl Default for PortalState {
    fn default() -> Self {
        // DISARMED on purpose: arrivals are authored near portals, so the
        // camera's initial pose may well sit inside a volume.
        Self {
            armed: false,
            cooldown: 0.0,
            pending_arrival: None,
            arrival_hold: 0.0,
            arrival_applied_to: None,
            enabled: true,
        }
    }
}

/// One portal resolved out of the ECS for this frame's proximity test.
struct PortalHit {
    target: String,
    label: String,
    arrival: Option<Vec3>,
    arrival_yaw: Option<f32>,
    /// Squared distance from the camera, for picking the nearest.
    dist_sq: f32,
}

fn attr_str(attrs: &Attributes, key: &str) -> Option<String> {
    match attrs.values.get(key)? {
        AttributeValue::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// True when these attributes declare a non-blank `portal_target`.
fn declares_portal(attrs: &Attributes) -> bool {
    attr_str(attrs, ATTR_TARGET).is_some_and(|s| !s.trim().is_empty())
}

/// Keeps [`IsPortal`] in sync with the `portal_target` attribute. Driven by
/// `Changed<Attributes>`, so this costs nothing in steady state: it does real
/// work only on the frame a Space loads (every `Attributes` is freshly inserted)
/// and whenever someone edits an attribute in the Properties panel.
fn sync_portal_markers(
    mut commands: Commands,
    changed: Query<(Entity, &Attributes, Has<IsPortal>), Changed<Attributes>>,
) {
    for (entity, attrs, marked) in &changed {
        match (declares_portal(attrs), marked) {
            (true, false) => {
                commands.entity(entity).insert(IsPortal);
            }
            // Clearing `portal_target` must actually retire the portal, or a
            // deleted target would keep firing.
            (false, true) => {
                commands.entity(entity).remove::<IsPortal>();
            }
            _ => {}
        }
    }
}

fn attr_f32(attrs: &Attributes, key: &str) -> Option<f32> {
    match attrs.values.get(key)? {
        AttributeValue::Number(n) => Some(*n as f32),
        AttributeValue::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn attr_vec3(attrs: &Attributes, key: &str) -> Option<Vec3> {
    match attrs.values.get(key)? {
        AttributeValue::Vector3(v) => Some(*v),
        _ => None,
    }
}

/// Resolve a Space *name* to its directory. Prefers the Universe that already
/// contains the open Space so portal targets stay Universe-local; falls back to
/// a workspace-wide search (with a warning) so a cross-Universe portal still
/// works rather than silently doing nothing.
fn resolve_space(reg: &UniverseRegistry, current: &PathBuf, name: &str) -> Option<PathBuf> {
    let eq = |a: &str| a.eq_ignore_ascii_case(name);

    if let Some(universe) = reg.universe_for_space(current) {
        if let Some(s) = universe.spaces.iter().find(|s| eq(&s.name)) {
            return Some(s.path.clone());
        }
    }
    for universe in &reg.universes {
        if let Some(s) = universe.spaces.iter().find(|s| eq(&s.name)) {
            warn!(
                "portal: '{}' resolved outside the current Universe (found in '{}')",
                name, universe.name
            );
            return Some(s.path.clone());
        }
    }
    None
}

/// Exclusive because [`crate::space::space_ops::open_space`] takes `&mut World`.
fn portal_teleport_system(world: &mut World) {
    // ── Tick cooldown ───────────────────────────────────────────────────────
    let dt = world.resource::<Time>().delta_secs();
    {
        let mut st = world.resource_mut::<PortalState>();
        if !st.enabled {
            return;
        }
        if st.cooldown > 0.0 {
            st.cooldown = (st.cooldown - dt).max(0.0);
        }
    }

    // ── Where is the free camera? ───────────────────────────────────────────
    // Filtered on `EustressCamera` so the separate AI camera (see
    // `ai_camera.rs`, also a Camera3d) can never trigger a portal.
    let cam_pos = {
        let mut q = world.query_filtered::<&GlobalTransform, With<EustressCamera>>();
        match q.iter(world).next() {
            Some(gt) => gt.translation(),
            None => return,
        }
    };

    // ── Collect portals and their distance to the camera ────────────────────
    // `With<IsPortal>` keeps this O(portals), not O(entities) — see the module
    // doc for why that distinction matters here.
    let mut nearest: Option<PortalHit> = None;
    let mut any_in_rearm_band = false;
    {
        let mut q = world.query_filtered::<(&GlobalTransform, &Attributes), With<IsPortal>>();
        for (gt, attrs) in q.iter(world) {
            let Some(target) = attr_str(attrs, ATTR_TARGET) else { continue };
            let radius = attr_f32(attrs, ATTR_RADIUS).unwrap_or(DEFAULT_RADIUS).max(0.5);
            let dist_sq = gt.translation().distance_squared(cam_pos);

            let in_rearm_band = dist_sq <= (radius * REARM_FACTOR).powi(2);
            if in_rearm_band {
                any_in_rearm_band = true;
            }
            if dist_sq > radius * radius {
                continue;
            }
            if nearest.as_ref().is_some_and(|n| n.dist_sq <= dist_sq) {
                continue;
            }
            nearest = Some(PortalHit {
                label: attr_str(attrs, ATTR_LABEL).unwrap_or_else(|| target.clone()),
                target,
                arrival: attr_vec3(attrs, ATTR_ARRIVAL),
                arrival_yaw: attr_f32(attrs, ATTR_ARRIVAL_YAW),
                dist_sq,
            });
        }
    }

    // ── Arm-on-clear ────────────────────────────────────────────────────────
    if !any_in_rearm_band {
        let mut st = world.resource_mut::<PortalState>();
        if !st.armed {
            st.armed = true;
        }
    }

    let Some(hit) = nearest else { return };
    {
        let st = world.resource::<PortalState>();
        if !st.armed || st.cooldown > 0.0 {
            return;
        }
    }

    // ── Resolve the destination before committing to anything ───────────────
    let current = match world.get_resource::<SpaceRoot>() {
        Some(sr) => sr.0.clone(),
        None => return,
    };
    let target_path = {
        let Some(reg) = world.get_resource::<UniverseRegistry>() else { return };
        resolve_space(reg, &current, &hit.target)
    };
    let Some(target_path) = target_path else {
        // Disarm so this doesn't warn every frame while the camera sits in the
        // volume, and tell the user rather than failing silently.
        {
            let mut st = world.resource_mut::<PortalState>();
            st.armed = false;
            st.cooldown = COOLDOWN_SECS;
        }
        warn!("portal: no Space named '{}' found", hit.target);
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.warning(format!("Portal target Space '{}' not found", hit.target));
        }
        return;
    };

    if target_path == current {
        let mut st = world.resource_mut::<PortalState>();
        st.armed = false;
        st.cooldown = COOLDOWN_SECS;
        return;
    }

    // ── Jump ────────────────────────────────────────────────────────────────
    {
        let mut st = world.resource_mut::<PortalState>();
        st.armed = false;
        st.cooldown = COOLDOWN_SECS;
        st.pending_arrival = Some((
            hit.arrival.unwrap_or(Vec3::ZERO),
            hit.arrival_yaw,
        ));
        st.arrival_hold = ARRIVAL_HOLD_SECS;
        // MUST clear, or a camera that survives the space switch keeps its old
        // id, matches `arrival_applied_to`, and the new arrival pose is skipped
        // entirely — you would step through the portal and not move.
        st.arrival_applied_to = None;
    }

    info!("portal: → Space '{}' ({})", hit.target, hit.label);
    crate::space::space_ops::open_space(world, &target_path);

    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        n.info(format!("Portal → {}", hit.label));
    }
}

/// Place the arrival pose ONCE per camera, for up to [`ARRIVAL_HOLD_SECS`]
/// after a jump.
///
/// The window exists because `open_space` despawns the editor camera and
/// `ensure_camera_exists` respawns it — at `pivot = Vec3::ZERO`, which for the
/// Temple is inside the Ledger block — and on a Space that streams for a minute
/// that respawn can land well after the jump. So we keep watching.
///
/// But we write to each camera exactly once. Re-asserting every frame across
/// the whole window (the first version of this) meant the camera arrived
/// correctly and then overwrote the user's own mouse-look and WASD on every
/// subsequent frame — five seconds of the camera fighting you. Tracking WHICH
/// camera has been placed keeps the respawn covered without ever contesting
/// live input: a respawn brings a new `Entity` id, an unchanged id means the
/// user is in control.
fn apply_portal_arrival(
    time: Res<Time>,
    mut state: ResMut<PortalState>,
    mut cams: Query<(Entity, &mut EustressCamera)>,
) {
    if state.arrival_hold <= 0.0 {
        return;
    }
    let Some((pivot, yaw_deg)) = state.pending_arrival else {
        state.arrival_hold = 0.0;
        return;
    };

    let already = state.arrival_applied_to;
    let mut placed = None;
    for (entity, mut cam) in cams.iter_mut() {
        if already == Some(entity) {
            continue; // already placed this camera — hands off.
        }
        cam.pivot = pivot;
        cam.distance = ARRIVAL_DISTANCE;
        if let Some(y) = yaw_deg {
            let r = y.to_radians();
            cam.yaw = r;
            cam.target_yaw = r;
        }
        // A respawned camera comes back with `pitch = -0.5` looking down at an
        // origin pivot; re-level it so the arrival looks along the wing.
        cam.pitch = ARRIVAL_PITCH;
        cam.target_pitch = ARRIVAL_PITCH;
        cam.animating = false;
        cam.enabled = true;
        placed = Some(entity);
    }
    if placed.is_some() {
        state.arrival_applied_to = placed;
    }

    state.arrival_hold -= time.delta_secs();
    if state.arrival_hold <= 0.0 {
        state.pending_arrival = None;
        state.arrival_applied_to = None;
    }
}

pub struct PortalPlugin;

impl Plugin for PortalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortalState>()
            // Order matters twice over: markers must be current before the
            // proximity test reads them, and arrival must land before the test
            // so the freshly placed camera is not measured against a stale pose.
            .add_systems(
                Update,
                (sync_portal_markers, apply_portal_arrival, portal_teleport_system).chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::universe_registry::{SpaceInfo, UniverseInfo};

    fn reg() -> UniverseRegistry {
        let mut r = UniverseRegistry::default();
        r.universes = vec![
            UniverseInfo {
                path: PathBuf::from("/w/Tucson"),
                name: "Tucson".into(),
                spaces: vec![
                    SpaceInfo { path: PathBuf::from("/w/Tucson/Spaces/Temple"), name: "Temple".into() },
                    SpaceInfo { path: PathBuf::from("/w/Tucson/Spaces/S02-Permit-Queue"), name: "S02-Permit-Queue".into() },
                ],
            },
            UniverseInfo {
                path: PathBuf::from("/w/Other"),
                name: "Other".into(),
                spaces: vec![SpaceInfo {
                    path: PathBuf::from("/w/Other/Spaces/Elsewhere"),
                    name: "Elsewhere".into(),
                }],
            },
        ];
        r
    }

    #[test]
    fn resolves_within_the_current_universe() {
        let r = reg();
        let cur = PathBuf::from("/w/Tucson/Spaces/Temple");
        assert_eq!(
            resolve_space(&r, &cur, "S02-Permit-Queue"),
            Some(PathBuf::from("/w/Tucson/Spaces/S02-Permit-Queue"))
        );
    }

    #[test]
    fn space_names_are_case_insensitive() {
        let r = reg();
        let cur = PathBuf::from("/w/Tucson/Spaces/Temple");
        assert_eq!(
            resolve_space(&r, &cur, "temple"),
            Some(PathBuf::from("/w/Tucson/Spaces/Temple"))
        );
    }

    #[test]
    fn falls_back_across_universes_rather_than_failing() {
        let r = reg();
        let cur = PathBuf::from("/w/Tucson/Spaces/Temple");
        assert_eq!(
            resolve_space(&r, &cur, "Elsewhere"),
            Some(PathBuf::from("/w/Other/Spaces/Elsewhere"))
        );
    }

    #[test]
    fn unknown_space_resolves_to_none() {
        let r = reg();
        let cur = PathBuf::from("/w/Tucson/Spaces/Temple");
        assert_eq!(resolve_space(&r, &cur, "NoSuchSpace"), None);
    }

    #[test]
    fn portal_state_starts_disarmed() {
        // A camera whose initial pose sits inside a portal volume must not fire
        // on frame one — arrivals are authored near portals by design.
        let st = PortalState::default();
        assert!(!st.armed);
        assert!(st.enabled);
        assert_eq!(st.arrival_hold, 0.0);
        assert!(st.pending_arrival.is_none());
    }

    #[test]
    fn declares_portal_requires_a_non_blank_string_target() {
        let mut a = Attributes::new();
        assert!(!declares_portal(&a), "no attributes at all is not a portal");

        // Every instance carries an Attributes component, empty or not — an
        // empty one must never be tagged, or the marker optimisation is moot.
        a.set("unrelated", AttributeValue::String("x".into()));
        assert!(!declares_portal(&a));

        a.set(ATTR_TARGET, AttributeValue::String("   ".into()));
        assert!(!declares_portal(&a), "whitespace-only target is not a portal");

        a.set(ATTR_TARGET, AttributeValue::Number(3.0));
        assert!(!declares_portal(&a), "a Number target is not a Space name");

        a.set(ATTR_TARGET, AttributeValue::String("Temple".into()));
        assert!(declares_portal(&a));
    }

    /// The marker must be *removed* when `portal_target` is cleared, or a
    /// retired portal keeps firing at whatever it last pointed to.
    #[test]
    fn sync_portal_markers_adds_and_removes() {
        let mut app = App::new();
        app.add_systems(Update, sync_portal_markers);

        let mut attrs = Attributes::new();
        attrs.set(ATTR_TARGET, AttributeValue::String("Temple".into()));
        let e = app.world_mut().spawn(attrs).id();

        app.update();
        assert!(app.world().get::<IsPortal>(e).is_some(), "tagged on insert");

        // Clear the target — `Changed<Attributes>` fires on mutable access.
        app.world_mut().get_mut::<Attributes>(e).unwrap().values.remove(ATTR_TARGET);
        app.update();
        assert!(app.world().get::<IsPortal>(e).is_none(), "untagged once cleared");

        let plain = app.world_mut().spawn(Attributes::new()).id();
        app.update();
        assert!(
            app.world().get::<IsPortal>(plain).is_none(),
            "an empty Attributes component must never be tagged"
        );
    }

    #[test]
    fn attribute_readers_ignore_wrong_types() {
        let mut a = Attributes::new();
        a.set(ATTR_TARGET, AttributeValue::Number(3.0));
        a.set(ATTR_RADIUS, AttributeValue::String("wide".into()));
        assert_eq!(attr_str(&a, ATTR_TARGET), None, "a Number is not a target name");
        assert_eq!(attr_f32(&a, ATTR_RADIUS), None, "a String is not a radius");

        a.set(ATTR_TARGET, AttributeValue::String("Temple".into()));
        a.set(ATTR_RADIUS, AttributeValue::Int(9));
        assert_eq!(attr_str(&a, ATTR_TARGET).as_deref(), Some("Temple"));
        assert_eq!(attr_f32(&a, ATTR_RADIUS), Some(9.0), "Int coerces to radius");
    }
}
