//! # Physics Plugin
//!
//! Registers PhysicsService and constraint classes, and connects the
//! PhysicsService service to the simulation.
//!
//! The chain is: `_service.toml` values load into the service entity's
//! `ServiceComponent`; the properties panel and the MCP bridge edit that
//! component; the systems here copy it into the `PhysicsService` resource; and
//! the resource drives Avian (gravity, substeps, clock speed, whether rigid
//! bodies step) plus every realism domain through `PhysicsDomain` run
//! conditions.
//!
//! Every list of settings in this file is DERIVED from two definitions in
//! `eustress-common`: `PhysicsDomain::ALL` for the per-domain flags and
//! `PHYSICS_GENERAL_SETTINGS` for the rest. The seed, the sync, validation and
//! the bridge all walk those, so a new setting cannot be reachable from one
//! surface and missing from another.

use std::collections::HashMap;

use avian3d::prelude::{Gravity, Physics, PhysicsSystems, PhysicsTime, SubstepCount};
use bevy::prelude::*;
use eustress_common::classes::*;
use eustress_common::realism::{domain_active, PhysicsDomain};
use eustress_common::services::physics::*;

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
            // KINEMATICS gates Avian's step itself. This is a run condition on
            // the step, NOT a pause of `Time<Physics>`: play mode already owns
            // pausing and unpausing that clock, and a second owner would fight
            // it (turning Kinematics back on in Edit mode would have started
            // the simulation). Both must now allow a step for one to happen.
            .configure_sets(
                FixedPostUpdate,
                PhysicsSystems::StepSimulation.run_if(domain_active(PhysicsDomain::Kinematics)),
            );
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
    let g = d.gravity;
    let mut out = vec![
        ("enabled", PropertyValue::Bool(d.enabled)),
        ("gravity", PropertyValue::Vec3([g.x as f64, g.y as f64, g.z as f64])),
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
    if let Some(PropertyValue::Vec3(v)) = props.get("gravity") {
        let g = Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
        if g.is_finite() && g != p.gravity {
            p.gravity = g;
            changed.push("gravity");
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

/// Comma-separated list of every valid key, for error messages.
fn valid_keys() -> String {
    all_settings()
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(", ")
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

/// Validate a `physics.set` request into typed updates.
///
/// All or nothing: an unknown key, a wrong type, or an out-of-range value
/// fails the whole request before anything is applied, and the error says so.
/// Silently skipping an unrecognised key is how an agent's typo becomes a
/// change it believes was made and never was.
fn parse_update(params: &serde_json::Value) -> Result<Vec<(&'static str, PropertyValue)>, String> {
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
    let mut out: Vec<(&'static str, PropertyValue)> = Vec::new();
    let mut seen: HashMap<&'static str, &str> = HashMap::new();
    for (raw, value) in obj {
        let Some((key, kind)) = resolve_setting(raw) else {
            unknown.push(raw.as_str());
            continue;
        };
        if let Some(first) = seen.insert(key, raw.as_str()) {
            return Err(format!(
                "`{key}` was given twice (as `{first}` and `{raw}`); nothing was changed"
            ));
        }
        out.push((key, parse_value(key, kind, value).map_err(|e| format!("{e}; nothing was changed"))?));
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
fn physics_state_json(p: &PhysicsService) -> serde_json::Value {
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
        "gravity": [p.gravity.x, p.gravity.y, p.gravity.z],
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
    let p = world
        .get_resource::<PhysicsService>()
        .ok_or("the PhysicsService resource is not present in this engine")?;
    Ok(physics_state_json(p))
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
pub(crate) fn bridge_physics_set(
    world: &mut World,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let updates = parse_update(params)?;
    let update_map: HashMap<String, PropertyValue> = updates
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();

    // 1. The service entity. Missing when no Space is loaded (a headless host,
    //    or before the first Space opens); the resource still takes the edit.
    let mut to_save: Option<ServiceComponent> = None;
    {
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
    let changed = {
        let mut res = world
            .get_resource_mut::<PhysicsService>()
            .ok_or("the PhysicsService resource is not present in this engine")?;
        let changed = apply_properties(&update_map, res.bypass_change_detection());
        if !changed.is_empty() {
            res.set_changed();
        }
        changed
    };

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
        None => (
            false,
            None,
            None,
            Some("No PhysicsService entity is loaded (is a Space open?). Applied to the live simulation only; nothing was saved."),
        ),
    };

    if !changed.is_empty() {
        info!("PhysicsService (bridge): changed {:?}", changed);
    }

    let state = physics_state_json(world.resource::<PhysicsService>());
    Ok(serde_json::json!({
        "changed": changed,
        "persisted": persisted,
        "file": file,
        "save_error": save_error,
        "note": note,
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
/// not re-apply gravity and substeps to Avian.
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
            "PhysicsService: changed {:?}; enabled={} gravity={:?} time_scale={} substeps={} parallel={} {}",
            changed,
            p.enabled,
            p.gravity,
            p.time_scale,
            p.solver_substeps,
            p.parallel,
            flags.join(" "),
        );
    }
}

/// Push PhysicsService's tuning fields into Avian.
///
/// Before this, nothing did: the client's equivalent was `#[allow(dead_code)]`
/// and never scheduled, and the engine set gravity once at startup from a
/// constant. Editing gravity changed a resource no simulation code read.
///
/// Only runs when the resource changed. At startup its defaults equal the
/// engine's pinned values (gravity, `SubstepCount(6)`, speed 1.0), so the first
/// application is a no-op rather than a silent change to determinism.
fn apply_physics_service_to_avian(
    physics: Res<PhysicsService>,
    mut gravity: ResMut<Gravity>,
    mut substeps: ResMut<SubstepCount>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    if !physics.is_changed() {
        return;
    }
    if gravity.0 != physics.gravity {
        gravity.0 = physics.gravity;
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
        let map: HashMap<&str, PropertyValue> = u.into_iter().collect();
        assert_eq!(map.get("particle_simulation"), Some(&PropertyValue::Bool(false)));
        assert_eq!(map.get("time_scale"), Some(&PropertyValue::Float(0.5)));
        assert_eq!(map.get("chemistry"), Some(&PropertyValue::Bool(false)));
        assert_eq!(map.get("gravity"), Some(&PropertyValue::Vec3([0.0, -3.71, 0.0])));
        assert_eq!(map.get("solver_substeps"), Some(&PropertyValue::Int(8)));
    }

    #[test]
    fn update_accepts_a_settings_envelope() {
        let u = parse_update(&json!({ "settings": { "nuclear": false } })).expect("valid");
        assert_eq!(u, vec![("nuclear", PropertyValue::Bool(false))]);
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
            json!({ "gravity": -9.8 }),
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
        assert!((p.gravity.y + 1.62).abs() < 1e-6, "gravity not updated: {:?}", p.gravity);

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
        assert!(reply["note"].as_str().unwrap_or("").contains("nothing was saved"));
        assert!(!world.resource::<PhysicsService>().fluids);
    }

    #[test]
    fn bridge_get_reports_running_separately_from_the_flag() {
        let mut world = World::new();
        world.insert_resource(PhysicsService { enabled: false, ..Default::default() });
        let state = bridge_physics_get(&mut world).expect("get");
        let chem = state["domains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["key"] == "chemistry")
            .unwrap();
        assert_eq!(chem["on"], json!(true), "the flag itself is still on");
        assert_eq!(chem["running"], json!(false), "but the master switch stops it");
    }
}
