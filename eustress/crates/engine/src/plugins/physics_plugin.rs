//! # Physics Plugin
//!
//! Registers PhysicsService and constraint classes, and connects the
//! PhysicsService service to the simulation.
//!
//! The chain is: `_service.toml` values load into the service entity's
//! `ServiceComponent`; the properties panel and the MCP bridge edit that
//! component; the systems here copy it into the `PhysicsService` resource; and
//! the resource drives Avian (substeps, clock speed, whether rigid bodies
//! step) plus every realism domain through `PhysicsDomain` run conditions.
//!
//! GRAVITY is not a PhysicsService setting. It is owned by `Workspace.gravity`
//! and written to Avian only by `sync_workspace_gravity_to_avian`, which runs
//! every frame. This plugin once wrote Avian's gravity from PhysicsService too;
//! the Workspace sync overwrote it on the next frame, so the edit was reported
//! as applied and silently undone. The MCP bridge still accepts `gravity`, but
//! routes it to `Workspace.gravity`.
//!
//! Every list of settings in this file is DERIVED from two definitions in
//! `eustress-common`: `PhysicsDomain::ALL` for the per-domain flags and
//! `PHYSICS_GENERAL_SETTINGS` for the rest. The seed, the sync, validation and
//! the bridge all walk those, so a new setting cannot be reachable from one
//! surface and missing from another.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use avian3d::prelude::{Physics, PhysicsSchedule, PhysicsStepSystems, PhysicsSystems, PhysicsTime, SubstepCount};
use bevy::prelude::*;
use eustress_common::classes::*;
use eustress_common::realism::{domain_active, PhysicsDomain};
use eustress_common::services::physics::*;
use eustress_common::services::workspace::Workspace;

use crate::space::service_loader::{PropertyValue, ServiceComponent};

/// The `class_name` the PhysicsService service entity carries.
pub(crate) const PHYSICS_SERVICE: &str = "PhysicsService";

/// Realism modules that exist but schedule no systems, so there is nothing
/// for a flag to stop. Reported by `physics.get` so an agent asking to turn
/// one off learns why it cannot, instead of assuming the tool is incomplete.
const NOT_TOGGLEABLE: &[&str] = &[
    "structures",
    "plasma",
    "propulsion",
    "control",
    "thermocycles",
    "numerics",
];

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resource
            .init_resource::<PhysicsService>()
            .register_type::<PhysicsService>()

            // Constraints
            .register_type::<Attachment>()
            .register_type::<WeldConstraint>()
            .register_type::<Motor6D>()

            // Physics components
            .register_type::<CollisionGroup>()
            .register_type::<PhysicsMaterial>()
            .register_type::<Constraint>()
            .register_type::<BodyVelocity>()
            .register_type::<BodyForce>()

            .add_systems(
                Update,
                (
                    seed_physics_service_properties,
                    sync_service_properties_to_physics,
                    apply_physics_service_to_avian,
                )
                    .chain(),
            )
            // The Space's saved gravity, when its Workspace service loads.
            .init_resource::<AppliedAuthoredGravity>()
            .add_systems(Update, apply_authored_gravity)
            // KINEMATICS gates Avian's step itself. This is a run condition on
            // the step, NOT a pause of `Time<Physics>`: play mode already owns
            // pausing and unpausing that clock, and a second owner would fight
            // it (turning Kinematics back on in Edit mode would have started
            // the simulation). Both must now allow a step for one to happen.
            .configure_sets(
                FixedPostUpdate,
                PhysicsSystems::StepSimulation.run_if(domain_active(PhysicsDomain::Kinematics)),
            );
        // What each physics step costs, always on.
        add_physics_step_timer(app);
        // Every part's collider weighs what its part does.
        add_part_density(app);
    }
}

// ============================================================================
// Part density
// ============================================================================

/// A part weighs its real density: [`BasePart::effective_density`], its
/// custom physical properties' density when it has one (an authored or
/// imported override), else its material's. The pass writes that into
/// `BasePart.density` (and so its mass) and into Avian's `ColliderDensity`.
/// Without it Avian gives every collider its default `ColliderDensity(1.0)`,
/// and a 4 × 1 × 2 m brick would weigh 8 kg.
///
/// It runs when the collider or the part arrives, whichever is second, and
/// whenever the part changes, so a material change in Studio or from a
/// script is followed without a hook of its own. Sensor (CanCollide off)
/// colliders carry the density too, for the system that gives them explicit
/// mass: Avian counts no mass for a sensor. Colliders that are not a part's
/// (fracture fragments, the avatar) keep what their spawner set.
pub(crate) fn add_part_density(app: &mut App) {
    app.add_observer(density_on_add::<avian3d::prelude::Collider>)
        .add_observer(density_on_add::<BasePart>)
        .add_systems(Update, follow_part_density);
}

fn density_on_add<C: Component>(
    add: On<bevy::ecs::lifecycle::Add, C>,
    mut parts: Query<(&mut BasePart, Option<&mut avian3d::prelude::ColliderDensity>)>,
) {
    if let Ok((mut part, collider)) = parts.get_mut(add.event().entity) {
        settle_part_density(&mut part, collider);
    }
}

fn follow_part_density(
    mut parts: Query<(&mut BasePart, Option<&mut avian3d::prelude::ColliderDensity>), Changed<BasePart>>,
) {
    for (mut part, collider) in &mut parts {
        settle_part_density(&mut part, collider);
    }
}

/// Each written only when it differs, so a settled part is never marked
/// changed and its collider's mass is never recomputed for nothing.
fn settle_part_density(part: &mut Mut<BasePart>, collider: Option<Mut<avian3d::prelude::ColliderDensity>>) {
    let wanted = part.effective_density();
    if part.density != wanted {
        part.set_density(wanted);
    }
    if let Some(mut collider) = collider {
        if collider.0 != wanted {
            collider.0 = wanted;
        }
    }
}

// ============================================================================
// Physics step timing
// ============================================================================

/// How long physics steps take, wall clock, for anything that weighs physics
/// cost: the Stress Test, the physics report.
///
/// A step is one run of Avian's `PhysicsSchedule`, all its substeps included,
/// timed from `PhysicsStepSystems::First` to `PhysicsStepSystems::Last`. A
/// frame runs as many as it has fixed ticks (0, 1 or 2 at 60 Hz), and none
/// while the physics clock is paused. Always on: two clock reads and no
/// allocation per step.
#[derive(Resource, Debug, Clone)]
pub struct PhysicsStepTimer {
    /// The last step's duration.
    pub last_step: Duration,
    /// This frame's steps: their total duration.
    pub frame_total: Duration,
    /// This frame's steps: how many ran.
    pub frame_steps: u32,
    /// Every step since the app started: how many ran. A cursor for
    /// [`Self::since`].
    pub steps: u64,
    /// Every step since the app started: their total duration.
    pub total: Duration,
    /// The last [`Self::HISTORY`] steps' durations, oldest first.
    recent: VecDeque<Duration>,
    started: Option<Instant>,
}

impl Default for PhysicsStepTimer {
    fn default() -> Self {
        Self {
            last_step: Duration::ZERO,
            frame_total: Duration::ZERO,
            frame_steps: 0,
            steps: 0,
            total: Duration::ZERO,
            recent: VecDeque::with_capacity(Self::HISTORY),
            started: None,
        }
    }
}

impl PhysicsStepTimer {
    /// Steps the history holds: over a minute at 60 Hz.
    pub const HISTORY: usize = 4096;

    /// The durations of the steps after step number `cursor` (a value of
    /// [`Self::steps`] read earlier), oldest first, and how many of those
    /// steps have already left the history. Reading at least once every
    /// [`Self::HISTORY`] steps loses none, so a baseline window and a run
    /// window each come out whole, for a mean and a p95 per step.
    pub fn since(&self, cursor: u64) -> (impl Iterator<Item = Duration> + '_, u64) {
        let wanted = self.steps.saturating_sub(cursor);
        let kept = wanted.min(self.recent.len() as u64);
        let skip = self.recent.len() - kept as usize;
        (self.recent.iter().skip(skip).copied(), wanted - kept)
    }

    fn record(&mut self, step: Duration) {
        self.last_step = step;
        self.frame_total += step;
        self.frame_steps += 1;
        self.steps += 1;
        self.total += step;
        if self.recent.len() == Self::HISTORY {
            self.recent.pop_front();
        }
        self.recent.push_back(step);
    }
}

/// Time every physics step into [`PhysicsStepTimer`], and start each frame's
/// count at zero.
pub(crate) fn add_physics_step_timer(app: &mut App) {
    app.init_resource::<PhysicsStepTimer>()
        .add_systems(First, start_frame_physics_steps)
        .add_systems(
            PhysicsSchedule,
            (
                start_physics_step.in_set(PhysicsStepSystems::First),
                finish_physics_step.in_set(PhysicsStepSystems::Last),
            ),
        );
}

fn start_frame_physics_steps(mut timer: ResMut<PhysicsStepTimer>) {
    timer.frame_total = Duration::ZERO;
    timer.frame_steps = 0;
}

fn start_physics_step(mut timer: ResMut<PhysicsStepTimer>) {
    timer.started = Some(Instant::now());
}

fn finish_physics_step(mut timer: ResMut<PhysicsStepTimer>) {
    if let Some(started) = timer.started.take() {
        timer.record(started.elapsed());
    }
}

// ============================================================================
// The settings surface
// ============================================================================

/// Every editable setting as `(key, kind)`: the general settings followed by
/// one boolean per domain.
fn all_settings() -> Vec<(&'static str, PhysicsSettingKind)> {
    let mut out: Vec<(&'static str, PhysicsSettingKind)> = PHYSICS_GENERAL_SETTINGS
        .iter()
        .map(|(k, kind, _)| (*k, *kind))
        .collect();
    out.extend(PhysicsDomain::ALL.iter().map(|d| (d.key(), PhysicsSettingKind::Bool)));
    out
}

/// Every editable property with its current value in `d`.
///
/// Used to seed a service entity that is missing keys. The panel's write path
/// only updates a key that ALREADY EXISTS in the service's property map, and
/// silently drops an edit to one that does not. A Space created before
/// PhysicsService existed has no `_service.toml` for it, so its entity spawns
/// with an empty map and every toggle in the panel would have accepted a click
/// and done nothing.
fn default_properties(d: &PhysicsService) -> Vec<(&'static str, PropertyValue)> {
    let mut out = vec![
        ("enabled", PropertyValue::Bool(d.enabled)),
        ("time_scale", PropertyValue::Float(d.time_scale as f64)),
        ("solver_substeps", PropertyValue::Int(d.solver_substeps as i64)),
        ("parallel", PropertyValue::Bool(d.parallel)),
    ];
    out.extend(
        PhysicsDomain::ALL
            .iter()
            .map(|dom| (dom.key(), PropertyValue::Bool(d.domain_flag(*dom)))),
    );
    out
}

/// Copy whatever settings `props` defines onto `p`, returning the keys whose
/// value actually changed.
///
/// The single mapping from service properties to the resource. The sync
/// system and the MCP bridge both call it, so the two cannot interpret a value
/// differently. Clamps are defensive, for hand-edited files: the bridge
/// rejects out-of-range input before it gets here.
fn apply_properties(
    props: &HashMap<String, PropertyValue>,
    p: &mut PhysicsService,
) -> Vec<&'static str> {
    let bool_of = |key: &str| match props.get(key) {
        Some(PropertyValue::Bool(b)) => Some(*b),
        _ => None,
    };
    let float_of = |key: &str| match props.get(key) {
        Some(PropertyValue::Float(v)) => Some(*v),
        Some(PropertyValue::Int(v)) => Some(*v as f64),
        _ => None,
    };

    let mut changed: Vec<&'static str> = Vec::new();

    if let Some(v) = bool_of("enabled") {
        if p.enabled != v {
            p.enabled = v;
            changed.push("enabled");
        }
    }
    if let Some(v) = bool_of("parallel") {
        if p.parallel != v {
            p.parallel = v;
            changed.push("parallel");
        }
    }
    if let Some(v) = float_of("time_scale") {
        // Negative or non-finite time would run the clock backwards or poison
        // it. `clamp` passes NaN through, which `is_finite` then rejects.
        let v = (v as f32).clamp(0.0, 100.0);
        if v.is_finite() && (v - p.time_scale).abs() > f32::EPSILON {
            p.time_scale = v;
            changed.push("time_scale");
        }
    }
    if let Some(v) = float_of("solver_substeps") {
        // Zero substeps is not "no physics" in Avian, it is a divide by zero in
        // the substep delta. Kinematics is the off switch.
        let v = (v.round() as i64).clamp(1, 64) as u32;
        if v != p.solver_substeps {
            p.solver_substeps = v;
            changed.push("solver_substeps");
        }
    }
    for domain in PhysicsDomain::ALL {
        if let Some(v) = bool_of(domain.key()) {
            if p.domain_flag(domain) != v {
                p.set_domain_flag(domain, v);
                changed.push(domain.key());
            }
        }
    }

    changed
}

/// Resolve an incoming setting name to its canonical key, accepting the
/// snake_case key (`time_scale`), the properties panel's display name
/// (`TimeScale`), and stray case or underscores.
fn resolve_setting(raw: &str) -> Option<(&'static str, PhysicsSettingKind)> {
    let squash = |s: &str| -> String {
        s.chars().filter(|c| *c != '_').flat_map(|c| c.to_lowercase()).collect()
    };
    let wanted = squash(raw);
    all_settings().into_iter().find(|(k, _)| squash(k) == wanted)
}

/// The one key the bridge accepts that is NOT a PhysicsService setting.
const GRAVITY_KEY: &str = "gravity";

/// What a `physics.set` reply says about any gravity it applied.
const GRAVITY_NOTE: &str = "Gravity was applied live to the Workspace and is not saved: reopening the Space applies the gravity its Workspace service saves, and Stop undoes a change made during Play.";

/// Added when the gravity has a part the player character does not follow.
const GRAVITY_PLAYER_NOTE: &str = "The player character falls straight down only: it follows the downward part of gravity and ignores any sideways or upward part, so with this gravity it moves differently from parts.";

/// Comma-separated list of every valid key, for error messages.
fn valid_keys() -> String {
    let mut keys: Vec<&str> = all_settings().iter().map(|(k, _)| *k).collect();
    keys.push(GRAVITY_KEY);
    keys.join(", ")
}

/// Parse a gravity value: a plain number is a DOWNWARD magnitude in m/s², the
/// way `workspace.Gravity` reads (9.80665 is Earth; negative falls upward); an
/// `[x, y, z]` array is the full vector. Both are what agents naturally send.
fn parse_gravity(value: &serde_json::Value) -> Result<Vec3, String> {
    if let Some(g) = value.as_f64() {
        if !g.is_finite() || g.abs() > 10_000.0 {
            return Err(format!("`gravity` must be a finite number of m/s² under 10000, got {value}"));
        }
        return Ok(Vec3::new(0.0, -(g as f32), 0.0));
    }
    match parse_value(GRAVITY_KEY, PhysicsSettingKind::Vec3, value)? {
        PropertyValue::Vec3(v) => {
            let g = Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
            if !g.is_finite() || g.length() > 10_000.0 {
                return Err(format!("`gravity` must be finite and under 10000 m/s², got {value}"));
            }
            Ok(g)
        }
        _ => Err(format!(
            "`gravity` must be a number (downward, m/s²) or [x, y, z]; got {value}"
        )),
    }
}

/// Convert one incoming JSON value to a property value, enforcing the
/// setting's type and range.
fn parse_value(
    key: &'static str,
    kind: PhysicsSettingKind,
    value: &serde_json::Value,
) -> Result<PropertyValue, String> {
    match kind {
        PhysicsSettingKind::Bool => {
            // Agents sometimes quote booleans; accept the unambiguous spellings.
            let b = match value {
                serde_json::Value::Bool(b) => Some(*b),
                serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "on" => Some(true),
                    "false" | "off" => Some(false),
                    _ => None,
                },
                _ => None,
            };
            b.map(PropertyValue::Bool)
                .ok_or_else(|| format!("`{key}` must be true or false, got {value}"))
        }
        PhysicsSettingKind::Float { min, max } => {
            let v = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("`{key}` must be a number, got {value}"))?;
            if v < min || v > max {
                return Err(format!("`{key}` must be between {min} and {max}, got {v}"));
            }
            Ok(PropertyValue::Float(v))
        }
        PhysicsSettingKind::Int { min, max } => {
            let v = value
                .as_f64()
                .filter(|v| v.is_finite() && v.fract() == 0.0)
                .ok_or_else(|| format!("`{key}` must be a whole number, got {value}"))?
                as i64;
            if v < min || v > max {
                return Err(format!("`{key}` must be between {min} and {max}, got {v}"));
            }
            Ok(PropertyValue::Int(v))
        }
        PhysicsSettingKind::Vec3 => {
            let parts: Option<Vec<f64>> = value.as_array().and_then(|a| {
                if a.len() != 3 {
                    return None;
                }
                a.iter().map(|v| v.as_f64().filter(|f| f.is_finite())).collect()
            });
            let parts = parts.ok_or_else(|| {
                format!("`{key}` must be [x, y, z] as three numbers, for example [0, -9.80665, 0]; got {value}")
            })?;
            Ok(PropertyValue::Vec3([parts[0], parts[1], parts[2]]))
        }
    }
}

/// A validated `physics.set` request.
#[derive(Debug, Default, PartialEq)]
struct ParsedUpdate {
    /// PhysicsService properties, applied to the service and saved with it.
    service: Vec<(&'static str, PropertyValue)>,
    /// Workspace gravity, applied live to `Workspace.gravity` and NOT saved.
    gravity: Option<Vec3>,
}

/// Validate a `physics.set` request into typed updates.
///
/// All or nothing: an unknown key, a wrong type, or an out-of-range value
/// fails the whole request before anything is applied, and the error says so.
/// Silently skipping an unrecognised key is how an agent's typo becomes a
/// change it believes was made and never was.
fn parse_update(params: &serde_json::Value) -> Result<ParsedUpdate, String> {
    let obj = params
        .as_object()
        .ok_or_else(|| format!("expected an object of settings, for example {{\"chemistry\": false}}; valid keys: {}", valid_keys()))?;
    // Tolerate the settings arriving wrapped in a `settings` envelope, a shape
    // agents reach for when a tool has one obvious argument.
    let obj = match obj.get("settings").and_then(|s| s.as_object()) {
        Some(inner) if obj.len() == 1 => inner,
        _ => obj,
    };
    if obj.is_empty() {
        return Err(format!("no settings given, nothing was changed; valid keys: {}", valid_keys()));
    }

    let mut unknown: Vec<&str> = Vec::new();
    let mut out = ParsedUpdate::default();
    let mut seen: HashMap<&'static str, &str> = HashMap::new();
    for (raw, value) in obj {
        if raw.replace('_', "").eq_ignore_ascii_case(GRAVITY_KEY) {
            if let Some(first) = seen.insert(GRAVITY_KEY, raw.as_str()) {
                return Err(format!(
                    "`gravity` was given twice (as `{first}` and `{raw}`); nothing was changed"
                ));
            }
            out.gravity = Some(parse_gravity(value).map_err(|e| format!("{e}; nothing was changed"))?);
            continue;
        }
        let Some((key, kind)) = resolve_setting(raw) else {
            unknown.push(raw.as_str());
            continue;
        };
        if let Some(first) = seen.insert(key, raw.as_str()) {
            return Err(format!(
                "`{key}` was given twice (as `{first}` and `{raw}`); nothing was changed"
            ));
        }
        out.service.push((key, parse_value(key, kind, value).map_err(|e| format!("{e}; nothing was changed"))?));
    }
    if !unknown.is_empty() {
        return Err(format!(
            "unknown setting(s) {unknown:?}, nothing was changed; valid keys: {}",
            valid_keys()
        ));
    }
    Ok(out)
}

/// The full current state, as reported by both bridge operations.
///
/// Each domain reports its own flag AND whether it is actually running, which
/// differ whenever the master switch is off. Reporting only the flag let
/// "chemistry: true" read as running when nothing was.
fn physics_state_json(p: &PhysicsService, workspace_gravity: Option<Vec3>) -> serde_json::Value {
    let domains: Vec<serde_json::Value> = PhysicsDomain::ALL
        .iter()
        .map(|d| {
            serde_json::json!({
                "key": d.key(),
                "label": d.label(),
                "on": p.domain_flag(*d),
                "running": p.domain_enabled(*d),
                "description": d.description(),
            })
        })
        .collect();
    serde_json::json!({
        "enabled": p.enabled,
        // The live Workspace gravity, which is what Avian applies. Reported
        // here so an agent sees it next to the rest of the physics state, but
        // owned by the Workspace, not by PhysicsService.
        "gravity": workspace_gravity.map(|g| [g.x, g.y, g.z]),
        "gravity_owner": "Workspace (workspace.Gravity). set_physics_settings applies it live and does not save it; the Space's saved gravity is the Workspace service's `gravity` (a vector in m/s²), and Stop restores the value Play started with.",
        "time_scale": p.time_scale,
        "solver_substeps": p.solver_substeps,
        "parallel": p.parallel,
        "domains": domains,
        "not_toggleable": NOT_TOGGLEABLE,
        "not_toggleable_reason": "These modules provide formulas that other code calls. They schedule no systems of their own, so there is no work for a flag to stop.",
    })
}

// ============================================================================
// Bridge operations (the engine side of the MCP tools)
// ============================================================================

/// `physics.get`: the current PhysicsService settings and what is running.
pub(crate) fn bridge_physics_get(world: &mut World) -> Result<serde_json::Value, String> {
    let gravity = world.get_resource::<Workspace>().map(|w| w.gravity);
    let p = world
        .get_resource::<PhysicsService>()
        .ok_or("the PhysicsService resource is not present in this engine")?;
    Ok(physics_state_json(p, gravity))
}

/// `physics.set`: apply a partial update to PhysicsService.
///
/// Writes the service ENTITY (what the properties panel shows and what gets
/// saved) and the live resource together. Writing only the resource would
/// leave the panel showing old values and be overwritten by the next sync
/// from the entity; writing only the entity would make the reply report the
/// state from before the change, because the sync runs later in the frame.
/// Both go through `apply_properties`, the same mapping the sync uses, so the
/// second application on the next sync finds nothing to change.
///
/// Persists to the Space's `PhysicsService/_service.toml` exactly as a panel
/// edit does, signed when logged in.
///
/// `gravity` is the exception. It is written to `Workspace.gravity`, the one
/// input `sync_workspace_gravity_to_avian` carries into Avian, applied live and
/// not saved. The gravity a Space saves is its Workspace service's `gravity`
/// ([`apply_authored_gravity`]), which reopening the Space applies again. Like
/// any gravity change, Stop undoes one made during Play ([`GravityPlaySnapshot`]).
pub(crate) fn bridge_physics_set(
    world: &mut World,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let parsed = parse_update(params)?;
    // Checked before anything is applied, so a request that cannot be honoured
    // in full changes nothing.
    if parsed.gravity.is_some() && !world.contains_resource::<Workspace>() {
        return Err(
            "`gravity` needs the Workspace resource, which this engine does not have; nothing was changed"
                .to_string(),
        );
    }
    let update_map: HashMap<String, PropertyValue> = parsed
        .service
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();

    // 1. The service entity. Missing when no Space is loaded (a headless host,
    //    or before the first Space opens); the resource still takes the edit.
    let mut to_save: Option<ServiceComponent> = None;
    if !update_map.is_empty() {
        let mut q = world.query::<&mut ServiceComponent>();
        for mut service in q.iter_mut(world) {
            if service.class_name != PHYSICS_SERVICE {
                continue;
            }
            for (k, v) in &update_map {
                service.properties.insert(k.clone(), v.clone());
            }
            to_save = Some((*service).clone());
            break;
        }
    }

    // 2. The live resource, now, so the reply reflects the change.
    let mut changed = {
        let mut res = world
            .get_resource_mut::<PhysicsService>()
            .ok_or("the PhysicsService resource is not present in this engine")?;
        let changed = apply_properties(&update_map, res.bypass_change_detection());
        if !changed.is_empty() {
            res.set_changed();
        }
        changed
    };

    // 2b. Gravity, into the Workspace (see this function's docs).
    let mut notes: Vec<&'static str> = Vec::new();
    if let Some(g) = parsed.gravity {
        let mut ws = world
            .get_resource_mut::<Workspace>()
            .ok_or("the Workspace resource disappeared mid-request")?;
        if ws.gravity != g {
            ws.gravity = g;
            changed.push(GRAVITY_KEY);
        }
        notes.push(GRAVITY_NOTE);
        if g.x != 0.0 || g.z != 0.0 || g.y > 0.0 {
            notes.push(GRAVITY_PLAYER_NOTE);
        }
    }

    // 3. Persist, with the same signature a panel edit gets.
    let (persisted, file, save_error, note) = match &to_save {
        Some(service) => {
            let stamp = world
                .get_resource::<crate::auth::AuthState>()
                .and_then(crate::space::instance_loader::current_stamp);
            let file = service.toml_path.display().to_string();
            match crate::space::service_loader::save_service_to_file_signed(service, stamp.as_ref()) {
                Ok(()) => (true, Some(file), None, None),
                Err(e) => (
                    false,
                    Some(file),
                    Some(e),
                    Some("Applied live, but the Space file could not be written, so it will revert when the Space reopens."),
                ),
            }
        }
        None if update_map.is_empty() => (false, None, None, None),
        None => (
            false,
            None,
            None,
            Some("No PhysicsService entity is loaded (is a Space open?). Applied to the live simulation only; nothing was saved."),
        ),
    };
    if let Some(n) = note {
        notes.insert(0, n);
    }

    if !changed.is_empty() {
        info!("PhysicsService (bridge): changed {:?}", changed);
    }

    let gravity = world.get_resource::<Workspace>().map(|w| w.gravity);
    let state = physics_state_json(world.resource::<PhysicsService>(), gravity);
    Ok(serde_json::json!({
        "changed": changed,
        "persisted": persisted,
        "file": file,
        "save_error": save_error,
        "notes": notes,
        "state": state,
    }))
}

// ============================================================================
// Systems
// ============================================================================

/// Fill in any PhysicsService property the Space's file does not define,
/// without touching values it does.
fn seed_physics_service_properties(
    mut services: Query<&mut ServiceComponent, Added<ServiceComponent>>,
) {
    let defaults = PhysicsService::default();
    for mut service in services.iter_mut() {
        if service.class_name != PHYSICS_SERVICE {
            continue;
        }
        let missing: Vec<(&'static str, PropertyValue)> = default_properties(&defaults)
            .into_iter()
            .filter(|(k, _)| !service.properties.contains_key(*k))
            .collect();
        if missing.is_empty() {
            continue;
        }
        let count = missing.len();
        for (k, v) in missing {
            service.properties.insert(k.to_string(), v);
        }
        info!("PhysicsService: seeded {count} missing properties with defaults");
    }
}

/// Copy PhysicsService property edits into the live `PhysicsService` resource.
///
/// Mirrors `sync_service_properties_to_lighting`. Flags the resource changed
/// only if a value really differs, so an unrelated edit to this service does
/// not re-apply substeps and clock speed to Avian.
fn sync_service_properties_to_physics(
    mut physics: ResMut<PhysicsService>,
    services: Query<&ServiceComponent, Changed<ServiceComponent>>,
) {
    for service in services.iter() {
        if service.class_name != PHYSICS_SERVICE {
            continue;
        }
        let changed = apply_properties(&service.properties, physics.bypass_change_detection());
        if changed.is_empty() {
            continue;
        }
        physics.set_changed();
        let p = &*physics;
        let flags: Vec<String> = PhysicsDomain::ALL
            .iter()
            .map(|d| format!("{}={}", d.key(), p.domain_flag(*d)))
            .collect();
        info!(
            "PhysicsService: changed {:?}; enabled={} time_scale={} substeps={} parallel={} {}",
            changed,
            p.enabled,
            p.time_scale,
            p.solver_substeps,
            p.parallel,
            flags.join(" "),
        );
    }
}

/// Push PhysicsService's tuning fields into Avian: solver substeps and the
/// physics clock's speed.
///
/// Before this, nothing did: the client's equivalent sat in a plugin nothing
/// adds. Gravity is deliberately NOT written here; it is owned by
/// `Workspace.gravity` and synced by `sync_workspace_gravity_to_avian`, and a
/// second writer here was overwritten by that sync on the very next frame.
///
/// Only runs when the resource changed. At startup its defaults equal the
/// engine's pinned values (`SubstepCount(6)`, speed 1.0), so the first
/// application is a no-op rather than a silent change to determinism.
fn apply_physics_service_to_avian(
    physics: Res<PhysicsService>,
    mut substeps: ResMut<SubstepCount>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    if !physics.is_changed() {
        return;
    }
    if substeps.0 != physics.solver_substeps {
        substeps.0 = physics.solver_substeps;
    }
    // Relative speed scales the physics clock without touching its pause
    // state, which play mode owns.
    let speed = physics.time_scale as f64;
    if (physics_time.relative_speed_f64() - speed).abs() > f64::EPSILON {
        physics_time.set_relative_speed_f64(speed);
    }
}

// ============================================================================
// Gravity across Play and Space switches
// ============================================================================

/// The Workspace gravity as a Play session found it, put back on every path
/// out of Play: the Stop button, MCP `stop_simulation`, the simulation's own
/// auto-stop, and the keyboard.
///
/// Gravity is simulation state, like a part's position. A script that gives
/// its level the Moon's gravity, or a tool trying a value mid-game, must not
/// leave the editor and the next Play on the Moon.
#[derive(Resource, Debug, Default)]
pub struct GravityPlaySnapshot {
    /// `Workspace.gravity` as the session left Edit mode; `None` outside one.
    pub saved: Option<Vec3>,
}

/// Save and restore the Workspace gravity around every Play session.
///
/// The save runs on the transition from Editing to Playing, which comes
/// before any script's first callback, and only from Editing: resuming from
/// Pause also enters Playing, and must keep the gravity the session started
/// with. The restore runs on entering Editing, where every stop path lands.
pub(crate) fn add_gravity_play_snapshot(app: &mut App) {
    use crate::play_mode::PlayModeState;
    app.init_resource::<GravityPlaySnapshot>()
        .add_systems(
            OnTransition { exited: PlayModeState::Editing, entered: PlayModeState::Playing },
            snapshot_gravity_on_play,
        )
        .add_systems(OnEnter(PlayModeState::Editing), restore_gravity_play_snapshot);
}

fn snapshot_gravity_on_play(workspace: Option<Res<Workspace>>, mut snapshot: ResMut<GravityPlaySnapshot>) {
    snapshot.saved = workspace.map(|ws| ws.gravity);
}

fn restore_gravity_play_snapshot(
    workspace: Option<ResMut<Workspace>>,
    mut snapshot: ResMut<GravityPlaySnapshot>,
) {
    let (Some(saved), Some(mut ws)) = (snapshot.saved.take(), workspace) else {
        return;
    };
    if ws.gravity != saved {
        info!("Gravity: Stop restored {:?} m/s² (Play left {:?})", saved, ws.gravity);
        ws.gravity = saved;
    }
}

/// Start a newly opened Space at standard gravity, until its Workspace
/// service loads and [`apply_authored_gravity`] sets the gravity it saves.
///
/// Without this, gravity set live in one Space carried into the next Space
/// opened in the same session, including one that saves no gravity. A Play
/// session that spans the switch restores to the new Space's gravity, not
/// the old one's.
pub(crate) fn reset_gravity_for_new_space(world: &mut World) {
    let standard = eustress_common::services::workspace::DEFAULT_GRAVITY;
    if let Some(mut ws) = world.get_resource_mut::<Workspace>() {
        if ws.gravity != standard {
            info!("Gravity: the new Space starts at {:?} m/s² (was {:?})", standard, ws.gravity);
            ws.gravity = standard;
        }
    }
    if let Some(mut snapshot) = world.get_resource_mut::<GravityPlaySnapshot>() {
        if snapshot.saved.is_some() {
            snapshot.saved = Some(standard);
        }
    }
    // The new Space's saved gravity applies even when it equals the old one's.
    if let Some(mut applied) = world.get_resource_mut::<AppliedAuthoredGravity>() {
        applied.0 = None;
    }
}

/// The Space's saved gravity as last applied, so that only a change to the
/// saved value moves `Workspace.gravity`: editing another Workspace property
/// during Play never undoes a gravity a script set.
#[derive(Resource, Debug, Default)]
pub struct AppliedAuthoredGravity(pub Option<Vec3>);

/// Apply the gravity a Space saves (the `gravity` key of its
/// `Workspace/_service.toml`) when the Workspace service loads, and again
/// when that key is edited.
///
/// Read through [`authored_gravity`], the rule the Player shares: a vector is
/// m/s² as written, and a stud-era value (a bare number, or the old
/// `[0.0, -196.2, 0.0]` default) is Roblox's gravity in an imported Space and
/// standard gravity anywhere else. A Roblox import keeps the gravity it was
/// tuned for (196.2 studs/s², 54.9 m/s² at 0.28 m studs); a native Space runs
/// at standard gravity. During Play the saved value also becomes what Stop
/// restores.
///
/// A stud-era value is replaced in the loaded properties by the vector it
/// reads as, without change detection: opening a Space writes no file, and
/// the next save of the service writes the vector. It has to, because a save
/// moves the properties under `[service]` and drops the `[properties.extras]`
/// table that marks an import, after which the old value would read as native.
///
/// [`authored_gravity`]: eustress_common::services::workspace::authored_gravity
fn apply_authored_gravity(
    mut services: Query<&mut ServiceComponent, Changed<ServiceComponent>>,
    mut applied: ResMut<AppliedAuthoredGravity>,
    workspace: Option<ResMut<Workspace>>,
    snapshot: Option<ResMut<GravityPlaySnapshot>>,
) {
    use eustress_common::services::workspace::{authored_gravity, AuthoredGravity};
    let Some(mut service) = services.iter_mut().find(|s| s.class_name == "Workspace") else {
        return;
    };
    let key = match service.properties.get(GRAVITY_KEY) {
        Some(PropertyValue::Vec3(v)) => Some(AuthoredGravity::from_vector(*v)),
        Some(PropertyValue::Float(f)) => Some(AuthoredGravity::Legacy(*f)),
        Some(PropertyValue::Int(i)) => Some(AuthoredGravity::Legacy(*i as f64)),
        _ => None,
    };
    let import_stud = match key {
        Some(AuthoredGravity::Legacy(_)) => roblox_import_stud(&service.toml_path),
        _ => None,
    };
    let gravity = authored_gravity(key, import_stud);
    if matches!(key, Some(AuthoredGravity::Legacy(_))) {
        // Each f32 through its shortest decimal, so the file reads 59.80176,
        // not 59.801761627197266.
        let decimal = |v: f32| if v == 0.0 { 0.0 } else { v.to_string().parse::<f64>().unwrap_or(v as f64) };
        let vector = PropertyValue::Vec3([decimal(gravity.x), decimal(gravity.y), decimal(gravity.z)]);
        service.bypass_change_detection().properties.insert(GRAVITY_KEY.to_string(), vector);
    }
    if applied.0 == Some(gravity) {
        return;
    }
    applied.0 = Some(gravity);
    if let Some(mut ws) = workspace {
        if ws.gravity != gravity {
            info!("Gravity: the Space saves {:?} m/s² (was {:?})", gravity, ws.gravity);
            ws.gravity = gravity;
        }
    }
    if let Some(mut snapshot) = snapshot {
        if snapshot.saved.is_some() {
            snapshot.saved = Some(gravity);
        }
    }
}

/// The stud an older Roblox import's lengths use, or `None` for a Workspace
/// service file that is not from a Roblox import.
///
/// Every import path writes the Roblox Workspace properties it has no field
/// for into a `[properties.extras]` table, and nothing native writes one; the
/// loaded properties drop tables, so the file itself is read. Every import
/// made before gravity was saved as a vector wrote its lengths in feet.
fn roblox_import_stud(service_file: &std::path::Path) -> Option<eustress_common::units::Unit> {
    let text = std::fs::read_to_string(service_file).ok()?;
    let file: toml::Value = toml::from_str(&text).ok()?;
    file.get("properties")?
        .get("extras")?
        .as_table()
        .map(|_| eustress_common::units::Unit::Foot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shipped template and the resource default must agree, or a Space
    /// created from the template behaves differently from one that fell back to
    /// the defaults. Two sources for one value is how gravity ended up at
    /// -196.2 in one place and -9.80665 in another.
    #[test]
    fn shipped_template_matches_resource_defaults() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../common/assets/service_templates/PhysicsService/_service.toml");
        let text = std::fs::read_to_string(&path).expect("read the PhysicsService template");
        let def = crate::space::service_loader::load_service_definition_from_str(&text)
            .expect("parse the PhysicsService template");

        // `[properties]` arrives as one table entry under `#[serde(flatten)]`.
        let table = match def.properties.get("properties") {
            Some(toml::Value::Table(t)) => t.clone(),
            other => panic!("template has no [properties] table: {other:?}"),
        };

        let defaults = PhysicsService::default();
        for (key, expected) in default_properties(&defaults) {
            let got = table
                .get(key)
                .unwrap_or_else(|| panic!("template is missing `{key}`"));
            match (&expected, got) {
                (PropertyValue::Bool(e), toml::Value::Boolean(g)) => {
                    assert_eq!(e, g, "`{key}` differs between template and default")
                }
                (PropertyValue::Float(e), toml::Value::Float(g)) => {
                    assert!((e - g).abs() < 1e-9, "`{key}`: template {g} vs default {e}")
                }
                (PropertyValue::Int(e), toml::Value::Integer(g)) => {
                    assert_eq!(e, g, "`{key}` differs between template and default")
                }
                (PropertyValue::Vec3(e), toml::Value::Array(g)) => {
                    let g: Vec<f64> = g.iter().filter_map(|v| v.as_float()).collect();
                    assert_eq!(g.len(), 3, "`{key}` must be three floats");
                    for i in 0..3 {
                        assert!(
                            (e[i] - g[i]).abs() < 1e-5,
                            "`{key}`[{i}]: template {} vs default {}",
                            g[i],
                            e[i]
                        );
                    }
                }
                (e, g) => panic!("`{key}` has mismatched types: default {e:?}, template {g:?}"),
            }
        }
    }

    /// The seed list, the validation list, and the two shared definitions must
    /// be the same set of keys. A key in one and not another is either a toggle
    /// the panel cannot edit or a setting MCP cannot reach.
    #[test]
    fn every_surface_covers_the_same_keys() {
        let mut seeded: Vec<&str> = default_properties(&PhysicsService::default())
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let mut valid: Vec<&str> = all_settings().into_iter().map(|(k, _)| k).collect();
        seeded.sort_unstable();
        valid.sort_unstable();
        assert_eq!(seeded, valid, "seeded keys and editable keys differ");
    }

    /// Every domain's flag gates that domain, and only that domain.
    #[test]
    fn every_domain_flag_gates_its_domain() {
        for domain in PhysicsDomain::ALL {
            let mut p = PhysicsService::default();
            assert!(p.domain_enabled(domain), "{} should default on", domain.label());
            p.set_domain_flag(domain, false);
            assert!(!p.domain_enabled(domain), "{} flag did not gate it", domain.label());
            for other in PhysicsDomain::ALL.into_iter().filter(|o| *o != domain) {
                assert!(
                    p.domain_enabled(other),
                    "turning off {} also stopped {}",
                    domain.label(),
                    other.label()
                );
            }
        }
    }

    /// The master switch stops everything, whatever the individual flags say.
    #[test]
    fn master_switch_overrides_every_domain() {
        let p = PhysicsService { enabled: false, ..Default::default() };
        for domain in PhysicsDomain::ALL {
            assert!(!p.domain_enabled(domain), "{} ran with the master switch off", domain.label());
        }
    }

    #[test]
    fn update_accepts_panel_names_and_quoted_booleans() {
        let u = parse_update(&json!({
            "ParticleSimulation": false,
            "TimeScale": 0.5,
            "chemistry": "off",
            "gravity": [0, -3.71, 0],
            "SolverSubsteps": 8,
        }))
        .expect("valid update");
        // Gravity is split out: it goes to the Workspace, never into the
        // PhysicsService property map.
        assert_eq!(u.gravity, Some(Vec3::new(0.0, -3.71, 0.0)));
        assert!(u.service.iter().all(|(k, _)| *k != GRAVITY_KEY), "gravity leaked into service settings");
        let map: HashMap<&str, PropertyValue> = u.service.into_iter().collect();
        assert_eq!(map.get("particle_simulation"), Some(&PropertyValue::Bool(false)));
        assert_eq!(map.get("time_scale"), Some(&PropertyValue::Float(0.5)));
        assert_eq!(map.get("chemistry"), Some(&PropertyValue::Bool(false)));
        assert_eq!(map.get("solver_substeps"), Some(&PropertyValue::Int(8)));
    }

    #[test]
    fn update_accepts_a_settings_envelope() {
        let u = parse_update(&json!({ "settings": { "nuclear": false } })).expect("valid");
        assert_eq!(u.service, vec![("nuclear", PropertyValue::Bool(false))]);
        assert_eq!(u.gravity, None);
    }

    /// A typo must fail loudly and change nothing. Skipping unknown keys is
    /// how an agent comes to believe a change was made that never was.
    #[test]
    fn update_rejects_unknown_keys_without_applying_anything() {
        let err = parse_update(&json!({ "chemistry": false, "plasma": false }))
            .expect_err("plasma is not toggleable");
        assert!(err.contains("plasma"), "error should name the bad key: {err}");
        assert!(err.contains("nothing was changed"), "error should say nothing changed: {err}");
    }

    #[test]
    fn update_rejects_bad_types_and_ranges() {
        for bad in [
            json!({ "chemistry": 1 }),
            json!({ "time_scale": -1.0 }),
            json!({ "time_scale": 1000.0 }),
            json!({ "solver_substeps": 0 }),
            json!({ "solver_substeps": 2.5 }),
            json!({ "gravity": [0, -9.8] }),
            json!({ "gravity": "down" }),
            json!({ "gravity": 1.0e9 }),
            json!({ "gravity": 3.7, "Gravity": 1.6 }),
            json!({}),
            json!([]),
            json!({ "chemistry": false, "Chemistry": true }),
        ] {
            assert!(parse_update(&bad).is_err(), "should have been rejected: {bad}");
        }
    }

    #[test]
    fn apply_reports_only_real_changes() {
        let mut p = PhysicsService::default();
        let mut props = HashMap::new();
        props.insert("chemistry".to_string(), PropertyValue::Bool(false));
        props.insert("kinematics".to_string(), PropertyValue::Bool(true)); // already true
        props.insert("solver_substeps".to_string(), PropertyValue::Int(6)); // already 6
        let changed = apply_properties(&props, &mut p);
        assert_eq!(changed, vec!["chemistry"]);
        assert!(!p.chemistry);
        assert!(apply_properties(&props, &mut p).is_empty(), "second apply is a no-op");
    }

    /// The whole bridge path against a real World: the entity, the resource and
    /// the file on disk must all end up agreeing.
    #[test]
    fn bridge_set_updates_entity_resource_and_file() {
        let dir = std::env::temp_dir().join(format!(
            "eustress_physics_bridge_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Deliberately NOT created: the save must create the service folder.
        let toml_path = dir.join("PhysicsService").join("_service.toml");

        let mut world = World::new();
        world.insert_resource(PhysicsService::default());
        world.insert_resource(Workspace::default());
        world.spawn(ServiceComponent {
            class_name: PHYSICS_SERVICE.to_string(),
            toml_path: toml_path.clone(),
            icon: "physicsservice".to_string(),
            properties: default_properties(&PhysicsService::default())
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            ..Default::default()
        });

        let reply = bridge_physics_set(&mut world, &json!({ "Chemistry": false, "gravity": [0, -1.62, 0] }))
            .expect("valid set");

        assert_eq!(reply["persisted"], json!(true), "reply: {reply}");
        let mut changed: Vec<String> = serde_json::from_value(reply["changed"].clone()).unwrap();
        changed.sort();
        assert_eq!(changed, vec!["chemistry".to_string(), "gravity".to_string()]);

        let p = world.resource::<PhysicsService>();
        assert!(!p.chemistry, "resource not updated");
        let ws = world.resource::<Workspace>();
        assert!((ws.gravity.y + 1.62).abs() < 1e-6, "Workspace gravity not updated: {:?}", ws.gravity);

        let mut q = world.query::<&ServiceComponent>();
        let svc = q.iter(&world).next().unwrap();
        assert_eq!(svc.properties.get("chemistry"), Some(&PropertyValue::Bool(false)), "entity not updated");

        let written = std::fs::read_to_string(&toml_path).expect("service file written");
        let reloaded = crate::space::service_loader::load_service_definition_from_str(&written)
            .expect("written file reloads");
        assert_eq!(
            reloaded.service.properties.get("chemistry"),
            Some(&toml::Value::Boolean(false)),
            "saved file does not carry the change:\n{written}"
        );
        // Gravity is not a PhysicsService property and must not be saved as one.
        assert!(
            reloaded.service.properties.get(GRAVITY_KEY).is_none(),
            "gravity was written into PhysicsService/_service.toml:
{written}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Without a loaded service entity the edit still reaches the simulation,
    /// and the reply says plainly that nothing was saved.
    #[test]
    fn bridge_set_without_a_space_applies_live_and_says_so() {
        let mut world = World::new();
        world.insert_resource(PhysicsService::default());
        let reply = bridge_physics_set(&mut world, &json!({ "fluids": false })).expect("valid");
        assert_eq!(reply["persisted"], json!(false));
        let notes: Vec<String> = serde_json::from_value(reply["notes"].clone()).unwrap();
        assert!(
            notes.iter().any(|n| n.contains("nothing was saved")),
            "notes should say nothing was saved: {notes:?}"
        );
        assert!(!world.resource::<PhysicsService>().fluids);
    }

    /// A bare number is a downward magnitude, the way `workspace.Gravity`
    /// reads; negative falls upward. An array is taken as the full vector.
    #[test]
    fn gravity_accepts_a_magnitude_or_a_vector() {
        assert_eq!(parse_gravity(&json!(1.62)).unwrap(), Vec3::new(0.0, -1.62, 0.0));
        assert_eq!(parse_gravity(&json!(-9.8)).unwrap(), Vec3::new(0.0, 9.8, 0.0));
        assert_eq!(parse_gravity(&json!([1, 0, 0])).unwrap(), Vec3::new(1.0, 0.0, 0.0));
        assert!(parse_gravity(&json!([0, 0])).is_err());
        assert!(parse_gravity(&json!(f64::MAX)).is_err());
    }

    /// Gravity alone touches neither the PhysicsService entity nor its file:
    /// there is no PhysicsService setting in the request to save.
    #[test]
    fn gravity_only_writes_the_workspace() {
        let dir = std::env::temp_dir().join(format!(
            "eustress_physics_gravity_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let toml_path = dir.join("PhysicsService").join("_service.toml");

        let mut world = World::new();
        world.insert_resource(PhysicsService::default());
        world.insert_resource(Workspace::default());
        world.spawn(ServiceComponent {
            class_name: PHYSICS_SERVICE.to_string(),
            toml_path: toml_path.clone(),
            ..Default::default()
        });

        let reply = bridge_physics_set(&mut world, &json!({ "gravity": 3.71 })).expect("valid");
        assert_eq!(reply["changed"], json!(["gravity"]));
        assert!((world.resource::<Workspace>().gravity.y + 3.71).abs() < 1e-6);
        assert!(!toml_path.exists(), "a gravity-only request wrote the PhysicsService file");
        let notes: Vec<String> = serde_json::from_value(reply["notes"].clone()).unwrap();
        assert!(notes.iter().any(|n| n.contains("not saved")), "notes: {notes:?}");
        assert!(
            !notes.iter().any(|n| n.contains("player character")),
            "straight-down gravity is one the player follows: {notes:?}"
        );
    }

    /// The player falls only along world down, so a gravity with any other
    /// part says how the player will differ from the parts around it.
    #[test]
    fn gravity_the_player_cannot_follow_says_so() {
        for gravity in [json!([3, -9.8, 0]), json!(-9.8)] {
            let mut world = World::new();
            world.insert_resource(PhysicsService::default());
            world.insert_resource(Workspace::default());
            let reply = bridge_physics_set(&mut world, &json!({ "gravity": gravity })).expect("valid");
            let notes: Vec<String> = serde_json::from_value(reply["notes"].clone()).unwrap();
            assert!(
                notes.iter().any(|n| n.contains("player character")),
                "gravity {gravity}: {notes:?}"
            );
        }
    }

    /// Driven through real state transitions: Play from Edit saves the
    /// gravity, pausing and resuming keeps it, and Stop puts it back, from
    /// Playing or from Paused. What Edit mode sets is kept.
    #[test]
    fn stop_restores_the_gravity_play_started_with() {
        use crate::play_mode::PlayModeState;
        let moon = Vec3::new(0.0, -1.62, 0.0);
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<PlayModeState>();
        app.insert_resource(Workspace { gravity: moon, ..Default::default() });
        add_gravity_play_snapshot(&mut app);
        app.update();

        fn go(app: &mut App, state: PlayModeState) {
            app.world_mut().resource_mut::<NextState<PlayModeState>>().set(state);
            app.update();
        }
        fn set(app: &mut App, gravity: Vec3) {
            app.world_mut().resource_mut::<Workspace>().gravity = gravity;
        }
        fn gravity(app: &App) -> Vec3 {
            app.world().resource::<Workspace>().gravity
        }

        go(&mut app, PlayModeState::Playing);
        set(&mut app, Vec3::ZERO);
        go(&mut app, PlayModeState::Paused);
        go(&mut app, PlayModeState::Playing);
        set(&mut app, Vec3::new(0.0, -30.0, 0.0));
        go(&mut app, PlayModeState::Editing);
        assert_eq!(gravity(&app), moon, "Stop did not restore the gravity Play started with");

        set(&mut app, Vec3::ZERO);
        app.update();
        assert_eq!(gravity(&app), Vec3::ZERO, "an Edit-mode change was undone");

        go(&mut app, PlayModeState::Playing);
        set(&mut app, moon);
        go(&mut app, PlayModeState::Paused);
        go(&mut app, PlayModeState::Editing);
        assert_eq!(gravity(&app), Vec3::ZERO, "Stop from Pause did not restore");
        assert!(app.world().resource::<GravityPlaySnapshot>().saved.is_none(), "the snapshot outlived its session");
    }

    /// A Space opens at standard gravity, and a session spanning the switch
    /// restores to that, not to the gravity of the Space it left.
    #[test]
    fn a_new_space_starts_at_standard_gravity() {
        let standard = Vec3::new(0.0, -eustress_common::units::STANDARD_GRAVITY_F32, 0.0);
        let mut world = World::new();
        world.insert_resource(Workspace { gravity: Vec3::new(0.0, -1.62, 0.0), ..Default::default() });
        world.insert_resource(GravityPlaySnapshot { saved: Some(Vec3::new(0.0, -1.62, 0.0)) });
        reset_gravity_for_new_space(&mut world);
        assert_eq!(world.resource::<Workspace>().gravity, standard);
        assert_eq!(world.resource::<GravityPlaySnapshot>().saved, Some(standard));

        world.insert_resource(GravityPlaySnapshot::default());
        reset_gravity_for_new_space(&mut world);
        assert_eq!(world.resource::<GravityPlaySnapshot>().saved, None, "outside Play there is no session to restore");
    }

    /// A world holding a Workspace service whose properties are `props`,
    /// loaded from `toml_path`.
    fn workspace_service_world(props: Vec<(&str, PropertyValue)>, toml_path: std::path::PathBuf) -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(Workspace { gravity: Vec3::new(0.0, -1.62, 0.0), ..Default::default() });
        world.init_resource::<AppliedAuthoredGravity>();
        let properties = props.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let e = world
            .spawn(ServiceComponent { class_name: "Workspace".into(), toml_path, properties, ..Default::default() })
            .id();
        (world, e)
    }

    fn apply(world: &mut World) {
        use bevy::ecs::system::RunSystemOnce;
        world.run_system_once(apply_authored_gravity).expect("the system runs");
    }

    fn saved_gravity(world: &World, e: Entity) -> Option<PropertyValue> {
        world.get::<ServiceComponent>(e).unwrap().properties.get(GRAVITY_KEY).cloned()
    }

    /// A native Space's old bare 196.2 was never read: it keeps standard
    /// gravity, and its loaded properties hold the vector the next save writes.
    #[test]
    fn a_native_space_keeps_standard_gravity() {
        // Both stud-era forms on disk: the bare template number, and the
        // vector the Tucson Spaces hold.
        for old in [PropertyValue::Float(196.2), PropertyValue::Vec3([0.0, -196.2, 0.0])] {
            let (mut world, e) = workspace_service_world(
                vec![("gravity", old.clone())],
                std::path::PathBuf::from("no/such/_service.toml"),
            );
            apply(&mut world);
            assert_eq!(
                world.resource::<Workspace>().gravity,
                eustress_common::services::workspace::DEFAULT_GRAVITY,
                "{old:?}"
            );
            assert_eq!(saved_gravity(&world, e), Some(PropertyValue::Vec3([0.0, -9.80665, 0.0])), "{old:?}");
        }
    }

    /// An older Roblox import, marked by its Workspace extras, falls at
    /// Roblox's 196.2 studs/s² in the feet its geometry was imported in.
    #[test]
    fn an_old_roblox_import_falls_at_roblox_gravity() {
        let dir = std::env::temp_dir().join(format!(
            "eustress_import_gravity_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("_service.toml");
        std::fs::write(
            &file,
            "[service]\nclass_name = \"Workspace\"\n\n[properties]\ngravity = 196.2\n\n[properties.extras]\nAirDensity = 0.0012\n",
        )
        .unwrap();
        let (mut world, e) = workspace_service_world(vec![("gravity", PropertyValue::Float(196.2))], file);
        apply(&mut world);
        let g = world.resource::<Workspace>().gravity;
        assert!((g.y + 196.2 * 0.3048).abs() < 1e-3, "{g:?}");
        match saved_gravity(&world, e) {
            Some(PropertyValue::Vec3(v)) => assert!((v[1] + 59.80176).abs() < 1e-4, "{v:?}"),
            other => panic!("the import's gravity was not normalised to a vector: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only a change to the saved gravity moves the live one: editing another
    /// Workspace property leaves a gravity a script set alone.
    #[test]
    fn only_the_saved_gravity_moves_the_live_one() {
        let (mut world, e) = workspace_service_world(
            vec![("gravity", PropertyValue::Vec3([0.0, -54.936, 0.0])), ("brightness", PropertyValue::Float(2.0))],
            std::path::PathBuf::from("no/such/_service.toml"),
        );
        apply(&mut world);
        assert_eq!(world.resource::<Workspace>().gravity, Vec3::new(0.0, -54.936, 0.0));

        world.resource_mut::<Workspace>().gravity = Vec3::ZERO;
        world.get_mut::<ServiceComponent>(e).unwrap().properties.insert("brightness".into(), PropertyValue::Float(3.0));
        apply(&mut world);
        assert_eq!(world.resource::<Workspace>().gravity, Vec3::ZERO, "an unrelated edit undid the script's gravity");

        world.get_mut::<ServiceComponent>(e).unwrap().properties.insert("gravity".into(), PropertyValue::Vec3([0.0, -1.62, 0.0]));
        apply(&mut world);
        assert_eq!(world.resource::<Workspace>().gravity, Vec3::new(0.0, -1.62, 0.0));
    }

    /// During Play a new saved gravity is also what Stop restores.
    #[test]
    fn during_play_the_saved_gravity_is_the_stop_target() {
        let (mut world, _) = workspace_service_world(
            vec![("gravity", PropertyValue::Vec3([0.0, -54.936, 0.0]))],
            std::path::PathBuf::from("no/such/_service.toml"),
        );
        world.insert_resource(GravityPlaySnapshot { saved: Some(Vec3::new(0.0, -9.80665, 0.0)) });
        apply(&mut world);
        assert_eq!(world.resource::<GravityPlaySnapshot>().saved, Some(Vec3::new(0.0, -54.936, 0.0)));
    }

    /// A request that cannot be honoured in full changes nothing at all,
    /// including the parts that on their own would have been fine.
    #[test]
    fn gravity_without_a_workspace_rejects_the_whole_request() {
        let mut world = World::new();
        world.insert_resource(PhysicsService::default());
        let err = bridge_physics_set(&mut world, &json!({ "chemistry": false, "gravity": 3.7 }))
            .expect_err("no Workspace resource");
        assert!(err.contains("nothing was changed"), "{err}");
        assert!(world.resource::<PhysicsService>().chemistry, "chemistry was applied anyway");
    }

    #[test]
    fn bridge_get_reports_running_separately_from_the_flag() {
        let mut world = World::new();
        world.insert_resource(PhysicsService { enabled: false, ..Default::default() });
        world.insert_resource(Workspace { gravity: Vec3::new(0.0, -1.62, 0.0), ..Default::default() });
        let state = bridge_physics_get(&mut world).expect("get");
        let g: Vec<f64> = serde_json::from_value(state["gravity"].clone()).unwrap();
        assert!((g[1] + 1.62).abs() < 1e-5, "gravity should come from the Workspace: {g:?}");
        let chem = state["domains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["key"] == "chemistry")
            .unwrap();
        assert_eq!(chem["on"], json!(true), "the flag itself is still on");
        assert_eq!(chem["running"], json!(false), "but the master switch stops it");
    }

    // ── Physics step timing ────────────────────────────────────────────────

    /// An app with the step timer and a `PhysicsSchedule` whose first and
    /// last step sets run in Avian's order.
    fn step_timer_app() -> App {
        // The multi-threaded executor dispatches onto this pool.
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        let mut app = App::new();
        add_physics_step_timer(&mut app);
        app.configure_sets(PhysicsSchedule, (PhysicsStepSystems::First, PhysicsStepSystems::Last).chain());
        app
    }

    fn physics_step(app: &mut App) {
        app.world_mut().run_schedule(PhysicsSchedule);
    }

    fn new_frame(app: &mut App) {
        app.world_mut().run_schedule(First);
    }

    /// Every step is timed once and summed into its frame; a new frame starts
    /// its count at zero and keeps the running totals.
    #[test]
    fn each_step_is_timed_and_summed_per_frame() {
        let mut app = step_timer_app();
        new_frame(&mut app);
        physics_step(&mut app);
        physics_step(&mut app);
        {
            let t = app.world().resource::<PhysicsStepTimer>();
            assert_eq!((t.steps, t.frame_steps), (2, 2));
            assert!(t.frame_total >= t.last_step);
            assert_eq!(t.total, t.frame_total, "one frame so far");
            assert_eq!(t.since(0).0.count(), 2);
        }
        new_frame(&mut app);
        let t = app.world().resource::<PhysicsStepTimer>();
        assert_eq!((t.frame_steps, t.frame_total), (0, Duration::ZERO), "a frame with no steps reads zero");
        assert_eq!(t.steps, 2, "the running count survives the frame");
    }

    /// A window read from a cursor returns exactly the steps after it, oldest
    /// first, and says how many already left the history.
    #[test]
    fn a_window_reads_the_steps_after_its_cursor() {
        let mut t = PhysicsStepTimer::default();
        for ms in 1..=5 {
            t.record(Duration::from_millis(ms));
        }
        let cursor = 2;
        let (window, lost) = t.since(cursor);
        assert_eq!(window.collect::<Vec<_>>(), [3, 4, 5].map(Duration::from_millis));
        assert_eq!(lost, 0);
        assert_eq!(t.since(t.steps).0.count(), 0, "nothing after the latest step");

        for _ in 0..PhysicsStepTimer::HISTORY {
            t.record(Duration::from_micros(10));
        }
        let (window, lost) = t.since(0);
        assert_eq!(window.count(), PhysicsStepTimer::HISTORY);
        assert_eq!(lost, 5, "the five oldest steps left the history");
    }

    // ── Part density ───────────────────────────────────────────────────────

    /// Eustress's material enum; Bevy's prelude also has a `Material`.
    use eustress_common::classes::Material as PartMaterial;

    fn density_app() -> App {
        let mut app = App::new();
        // Avian's collider plugin makes this required; its plugins are not
        // in this app.
        app.register_required_components::<avian3d::prelude::Collider, avian3d::prelude::ColliderDensity>();
        add_part_density(&mut app);
        app
    }

    /// A part as a toolbox insert makes it: the material set, the density
    /// left at BasePart's default (900), which the pass must correct.
    fn part_of(material: PartMaterial) -> BasePart {
        BasePart { material, ..Default::default() }
    }

    fn collider_density(app: &App, e: Entity) -> f32 {
        app.world().get::<avian3d::prelude::ColliderDensity>(e).expect("a collider has a density").0
    }

    fn part_density(app: &App, e: Entity) -> (f32, f32) {
        let part = app.world().get::<BasePart>(e).expect("a part");
        (part.density, part.mass)
    }

    /// A native Wood part weighs what wood does, 600 kg/m³, not Avian's
    /// default 1 kg/m³ nor BasePart's default 900, whichever of the part and
    /// its collider comes first.
    #[test]
    fn a_wood_parts_collider_is_as_dense_as_wood() {
        use avian3d::prelude::Collider;
        let mut app = density_app();
        let together = app.world_mut().spawn((part_of(PartMaterial::Wood), Collider::cuboid(4.0, 1.0, 2.0))).id();
        let collider_first = app.world_mut().spawn(Collider::cuboid(1.0, 1.0, 1.0)).id();
        app.world_mut().entity_mut(collider_first).insert(part_of(PartMaterial::Wood));
        let part_first = app.world_mut().spawn(part_of(PartMaterial::Wood)).id();
        app.world_mut().entity_mut(part_first).insert(Collider::cuboid(1.0, 1.0, 1.0));
        for e in [together, collider_first, part_first] {
            assert_eq!(collider_density(&app, e), 600.0);
            let (density, mass) = part_density(&app, e);
            let volume = app.world().get::<BasePart>(e).unwrap().volume();
            assert_eq!(density, 600.0);
            assert!((mass - 600.0 * volume).abs() < 1e-2, "mass {mass} follows the density");
        }
    }

    /// A part whose material changes weighs the new material; one with its
    /// own density keeps it through a material change; and a collider that
    /// is not a part's keeps what its spawner set.
    #[test]
    fn the_density_follows_the_part_and_leaves_other_colliders_alone() {
        use avian3d::prelude::{Collider, ColliderDensity};
        let mut app = density_app();
        let part = app.world_mut().spawn((part_of(PartMaterial::Wood), Collider::cuboid(1.0, 1.0, 1.0))).id();
        let fragment = app.world_mut().spawn((Collider::cuboid(1.0, 1.0, 1.0), ColliderDensity(2400.0))).id();
        app.update();
        app.world_mut().get_mut::<BasePart>(part).unwrap().material = PartMaterial::Metal;
        app.update();
        assert_eq!(collider_density(&app, part), 7850.0, "a material change");

        app.world_mut().get_mut::<BasePart>(part).unwrap().custom_physical_properties =
            Some(eustress_common::classes::PhysicalProperties { density: 1234.0, ..Default::default() });
        app.update();
        app.world_mut().get_mut::<BasePart>(part).unwrap().material = PartMaterial::Wood;
        app.update();
        assert_eq!(collider_density(&app, part), 1234.0, "its own density outlives the material");
        assert_eq!(part_density(&app, part).0, 1234.0);
        assert_eq!(collider_density(&app, fragment), 2400.0);
    }

    /// A CanCollide-off part's sensor collider carries its density too, for
    /// the explicit mass Avian does not give a sensor. A density that is not
    /// a number is replaced by the material's, never handed to Avian.
    #[test]
    fn a_sensor_carries_its_density_and_a_bad_one_is_replaced() {
        use avian3d::prelude::{Collider, Sensor};
        let mut app = density_app();
        let sensor = app.world_mut().spawn((part_of(PartMaterial::Concrete), Collider::cuboid(1.0, 1.0, 1.0), Sensor)).id();
        assert_eq!(collider_density(&app, sensor), 2400.0);
        let bad = app.world_mut().spawn((BasePart { density: f32::NAN, ..Default::default() }, Collider::cuboid(1.0, 1.0, 1.0))).id();
        assert_eq!(collider_density(&app, bad), 900.0, "Plastic's, the default material");
        assert_eq!(part_density(&app, bad).0, 900.0);
    }

    /// Always on means no allocation per step: the history is allocated once
    /// and never grows, however many steps run.
    #[test]
    fn the_history_never_grows() {
        let mut t = PhysicsStepTimer::default();
        let capacity = t.recent.capacity();
        assert!(capacity >= PhysicsStepTimer::HISTORY);
        for _ in 0..PhysicsStepTimer::HISTORY * 3 {
            t.record(Duration::from_micros(1));
        }
        assert_eq!(t.recent.capacity(), capacity);
        assert_eq!(t.recent.len(), PhysicsStepTimer::HISTORY);
    }
}
