//! # Avian collision → mesh-deformation bridge
//!
//! The vertex-deformation pipeline in
//! [`eustress_common::realism::deformation`] is driven by
//! [`ImpactDeformEvent`]s. Nothing in the engine ever wrote one, so impact
//! deformation — the headline behaviour, "hit a part and it dents" — could
//! never fire at runtime no matter how the pipeline was configured. The only
//! `ImpactDeformEvent` construction anywhere in the tree was example text on a
//! docs page (and it used the observer `commands.trigger` API, which does not
//! reach a `MessageReader` at all).
//!
//! This module is the missing producer. It mirrors the existing Avian → Luau
//! `Touched` bridge in [`crate::soul::rune_api`]: same event source, same
//! play-mode gating, and it lives engine-side because `eustress-common` only
//! has avian3d as an optional dependency while the engine depends on it
//! unconditionally. The message TYPES stay in common, so a headless host can
//! still drive deformation from its own producer.
//!
//! ## Why impulse, routed through energy
//!
//! [`CollisionStart`] fires *after* the solver has run, so the contact pair's
//! accumulated normal impulse is available and is the honest measure of how
//! hard the hit was. Converting that impulse to a force (`J/dt`) and using it
//! directly would make dent depth depend on the solver's timestep and substep
//! count — the same collision would dent differently at 30 Hz and 60 Hz.
//! Dissipated energy is timestep-independent, and is also what a permanent
//! dent physically stores, so the impulse is routed through
//! `E = J² / 2m_eff` and the depth comes out of contact mechanics:
//!
//! * below the Hertzian yield threshold → elastic indentation
//!   `δ = (E / ((8/15)·E*·√R))^(2/5)`, which springs back via
//!   `relax_elastic_deformation`;
//! * above it → plastic indentation `δ = √((E − E_y) / (π·R·H))` with Tabor
//!   hardness `H ≈ 3σ_y`, which is permanent.
//!
//! ## Dent depth is physically scaled, not cinematic
//!
//! These formulas produce *real* dent depths. A 1 kg object striking a steel
//! plate at 5 m/s leaves roughly a 0.15 mm dent — correct, and very close to
//! invisible on screen. [`DeformationConfig::scale`] is the artistic
//! multiplier for that: it is left at a physically honest `1.0` by default,
//! and a scene that wants visible damage should raise it (10–100× is a typical
//! game-feel range). The bridge logs each computed depth at `debug` level so
//! the value can be verified independently of whether it is visible.

use avian3d::prelude::*;
use bevy::prelude::*;

use eustress_common::classes::BasePart;
use eustress_common::realism::deformation::components::{
    DeformableMesh, DeformationConfig, FractureMeshEvent, ImpactDeformEvent,
};
use eustress_common::realism::materials::properties::MaterialProperties;

use crate::play_mode::PlayModeState;

// ── material fallback ────────────────────────────────────────────────────
//
// Most authored parts carry no `MaterialProperties`. Rather than skip them
// (which would make deformation look broken on exactly the common case),
// fall back to a rigid-plastic-like material — the closest analogue to the
// default "Plastic" part material.

/// Young's modulus (Pa) for a part with no authored material.
const FALLBACK_YOUNG_MODULUS: f32 = 2.0e9;
/// Poisson's ratio for a part with no authored material.
const FALLBACK_POISSON_RATIO: f32 = 0.35;
/// Yield strength (Pa) for a part with no authored material.
const FALLBACK_YIELD_STRENGTH: f32 = 4.0e7;
/// Fracture toughness K_IC (Pa·√m) for a part with no authored material.
const FALLBACK_FRACTURE_TOUGHNESS: f32 = 3.0e6;

/// The material constants the contact + fracture models need.
#[derive(Clone, Copy, Debug)]
struct ContactMaterial {
    young_modulus: f32,
    poisson_ratio: f32,
    yield_strength: f32,
    fracture_toughness: f32,
}

impl Default for ContactMaterial {
    fn default() -> Self {
        Self {
            young_modulus: FALLBACK_YOUNG_MODULUS,
            poisson_ratio: FALLBACK_POISSON_RATIO,
            yield_strength: FALLBACK_YIELD_STRENGTH,
            fracture_toughness: FALLBACK_FRACTURE_TOUGHNESS,
        }
    }
}

impl From<&MaterialProperties> for ContactMaterial {
    fn from(m: &MaterialProperties) -> Self {
        let d = Self::default();
        // Guard each field independently: a partially-authored material with a
        // zero modulus would otherwise divide by zero downstream.
        Self {
            young_modulus: if m.young_modulus > 0.0 { m.young_modulus } else { d.young_modulus },
            poisson_ratio: if m.poisson_ratio > 0.0 && m.poisson_ratio < 0.5 {
                m.poisson_ratio
            } else {
                d.poisson_ratio
            },
            yield_strength: if m.yield_strength > 0.0 { m.yield_strength } else { d.yield_strength },
            fracture_toughness: if m.fracture_toughness > 0.0 {
                m.fracture_toughness
            } else {
                d.fracture_toughness
            },
        }
    }
}

/// Impact energy (J) above which the target should crack rather than dent.
///
/// Griffith: a crack only runs if the energy released covers the cost of the
/// new surface it creates. Converting the material's fracture toughness K_IC
/// to a critical energy release rate `G_c = K_IC² / E` gives the cost per m²,
/// and the crack has to open a full cross-section of the part.
///
/// This is a real criterion, not a tuned constant, and it separates materials
/// the way intuition expects: concrete (low K_IC, ~30 J/m²) shatters from a
/// modest hit, structural steel (~12 kJ/m²) needs a genuine collision.
fn fracture_energy_threshold(target: ContactMaterial, size: Vec3) -> f32 {
    let g_c = (target.fracture_toughness * target.fracture_toughness) / target.young_modulus.max(1.0);

    // Crack area ≈ the smallest cross-section of the part: a crack takes the
    // cheapest path through.
    let mut dims = [size.x.abs(), size.y.abs(), size.z.abs()];
    dims.sort_by(f32::total_cmp);
    let area = (dims[0] * dims[1]).max(1.0e-6);

    g_c * area
}

/// Result of the contact model: how deep, how wide, and whether it is
/// permanent.
#[derive(Clone, Copy, Debug)]
struct Dent {
    /// Indentation depth at the contact centre, in metres.
    depth_m: f32,
    /// Radius of the affected surface patch, in metres.
    radius_m: f32,
    /// `true` when the impact exceeded yield (plastic, permanent).
    permanent: bool,
}

/// Convert a solved contact impulse into an indentation.
///
/// `inv_mass_sum` is `1/m₁ + 1/m₂` — taking it in inverse form is what makes
/// static bodies (inverse mass 0) fall out naturally as "infinitely heavy".
fn dent_from_impulse(
    impulse: f32,
    inv_mass_sum: f32,
    target: ContactMaterial,
    impactor: ContactMaterial,
    impactor_radius_m: f32,
) -> Option<Dent> {
    if !(impulse > 0.0) || !(inv_mass_sum > 0.0) {
        // Zero impulse, or two static bodies — no energy to dissipate.
        return None;
    }

    let m_eff = 1.0 / inv_mass_sum;
    let energy = (impulse * impulse) / (2.0 * m_eff);
    if !energy.is_finite() || energy <= 0.0 {
        return None;
    }

    // Reduced contact modulus: 1/E* = (1-ν₁²)/E₁ + (1-ν₂²)/E₂
    let inv_e_star = (1.0 - target.poisson_ratio.powi(2)) / target.young_modulus
        + (1.0 - impactor.poisson_ratio.powi(2)) / impactor.young_modulus;
    if !(inv_e_star > 0.0) {
        return None;
    }
    let e_star = 1.0 / inv_e_star;

    let r = impactor_radius_m.max(1.0e-3);
    let sy = target.yield_strength.max(1.0);

    // Johnson's yield-onset energy, E_y ≈ 10·σy⁵·R³/E*⁴.
    //
    // Written as 10·R³·σy·(σy/E*)⁴ deliberately: σy⁵ for steel is ~9.8e41,
    // which overflows f32 (max ~3.4e38) and would silently make E_y infinite,
    // so every impact would be classified elastic and no part would ever take
    // a permanent dent. Folding the ratio first keeps every intermediate in
    // range.
    let ratio = sy / e_star;
    let e_yield = 10.0 * r.powi(3) * sy * ratio.powi(4);

    let (depth_m, permanent) = if energy > e_yield && e_yield.is_finite() {
        // Plastic: Tabor indentation hardness H ≈ 3σy.
        let h = 3.0 * sy;
        (((energy - e_yield) / (std::f32::consts::PI * r * h)).sqrt(), true)
    } else {
        // Hertzian elastic: E = (8/15)·E*·√R·δ^(5/2)  ⇒  δ = (E/((8/15)E*√R))^(2/5)
        let denom = (8.0 / 15.0) * e_star * r.sqrt();
        if !(denom > 0.0) {
            return None;
        }
        ((energy / denom).powf(0.4), false)
    };

    if !depth_m.is_finite() || depth_m <= 0.0 {
        return None;
    }

    // Contact patch radius for a spherical indenter of depth δ.
    let radius_m = (2.0 * r * depth_m).sqrt();
    if !radius_m.is_finite() || radius_m <= 0.0 {
        return None;
    }

    Some(Dent { depth_m, radius_m, permanent })
}

// ── systems ──────────────────────────────────────────────────────────────

/// Avian only writes [`CollisionStart`] for colliders that opted in, so a
/// deformable part with no [`CollisionEventsEnabled`] receives exactly zero
/// impacts — the failure is silent and looks identical to "deformation is
/// broken". Attach it to every deformable as it appears.
///
/// The Luau `Touched` bridge already adds this component to scene parts when a
/// play session starts, but deformables created later (or in a session with no
/// scripts) would otherwise be missed.
pub fn enable_collision_events_for_deformables(
    mut commands: Commands,
    query: Query<Entity, (With<DeformableMesh>, Without<CollisionEventsEnabled>)>,
) {
    for entity in query.iter() {
        commands.entity(entity).insert(CollisionEventsEnabled);
    }
}

/// Minimum closing speed (m/s) that counts as an impact rather than resting
/// contact. A box sitting on a plate carries a large steady normal impulse
/// (it is holding the weight up) but essentially zero approach speed.
const MIN_IMPACT_APPROACH_SPEED: f32 = 0.5;

/// Seconds a part is immune from further impact processing after one is
/// registered, so a single collision produces ONE dent instead of one per
/// frame for as long as the bodies stay in contact.
const IMPACT_COOLDOWN_SECS: f32 = 0.25;

/// Translate Avian contacts into [`ImpactDeformEvent`]s.
///
/// ## Why this polls the contact graph instead of reading `CollisionStart`
///
/// The first implementation keyed off the `CollisionStart` message. That was
/// wrong twice over, and produced exactly zero dents on a scene where bodies
/// demonstrably collided:
///
/// 1. `CollisionStart` fires ONCE, on the frame contact begins — and a
///    brand-new contact's *accumulated* normal impulse at that instant can
///    still be ~0 (speculative contacts are reported before the bodies have
///    actually been pushed apart). The old code bailed on `impulse <= 0.0`,
///    so the single opportunity was discarded and no later frame ever
///    retried.
/// 2. Avian only emits those messages for colliders carrying
///    `CollisionEventsEnabled`. That made the whole feature depend on a
///    component landing before the collision — a silent, all-or-nothing
///    ordering dependency.
///
/// Polling [`Collisions`] avoids both: the contact graph is populated for every
/// touching pair regardless of `CollisionEventsEnabled`, and it is available on
/// every frame of the contact, so the impact is measured while it is actually
/// happening rather than at one arbitrary instant.
///
/// Resting contact is rejected by CLOSING SPEED, not by impulse: `normal_speed`
/// is computed pre-solve and is strongly negative for an approaching body and
/// ~0 for one already at rest. Using impulse alone could not tell "dropped from
/// height" apart from "heavy box sitting still".
#[allow(clippy::too_many_arguments)]
pub fn bridge_collisions_to_deformation(
    collisions: Collisions,
    deformables: Query<(&GlobalTransform, &BasePart, Option<&MaterialProperties>), With<DeformableMesh>>,
    others: Query<(Option<&BasePart>, Option<&MaterialProperties>)>,
    masses: Query<&ComputedMass>,
    config: Res<DeformationConfig>,
    time: Res<Time>,
    mut cooldowns: Local<std::collections::HashMap<Entity, f32>>,
    mut impacts: MessageWriter<ImpactDeformEvent>,
    mut fractures: MessageWriter<FractureMeshEvent>,
) {
    if !config.enabled {
        cooldowns.clear();
        return;
    }

    // Age out cooldowns.
    let dt = time.delta_secs();
    cooldowns.retain(|_, t| {
        *t -= dt;
        *t > 0.0
    });

    for pair in collisions.iter() {
        // Only pairs that are genuinely touching carry meaningful contact data.
        if !pair.flags.contains(ContactPairFlags::TOUCHING) {
            continue;
        }

        // Closing speed: most-negative `normal_speed` across the manifold.
        // Negative = approaching.
        let approach = pair
            .manifolds
            .iter()
            .flat_map(|m| m.points.iter())
            .map(|p| p.normal_speed)
            .fold(0.0_f32, f32::min);
        if approach > -MIN_IMPACT_APPROACH_SPEED {
            continue; // resting or separating, not an impact
        }

        let impulse = pair.total_normal_impulse_magnitude();
        if !(impulse > 0.0) {
            continue;
        }

        let ev = pair;

        // Effective mass from INVERSE masses so static bodies (inverse mass 0)
        // contribute nothing and read as infinitely heavy.
        let inv_mass = |body: Option<Entity>| -> f32 {
            body.and_then(|b| masses.get(b).ok())
                .map(|m| m.inverse())
                .unwrap_or(0.0)
        };
        let inv_mass_sum = inv_mass(ev.body1) + inv_mass(ev.body2);

        // Either side of the collision may be deformable — dent both.
        for (target_collider, other_collider) in
            [(ev.collider1, ev.collider2), (ev.collider2, ev.collider1)]
        {
            let Ok((gt, base_part, target_mat)) = deformables.get(target_collider) else {
                continue;
            };

            // One dent per impact, not one per frame of contact.
            if cooldowns.contains_key(&target_collider) {
                continue;
            }

            let target: ContactMaterial = target_mat.map(Into::into).unwrap_or_default();
            let (other_size, other_mat) = others
                .get(other_collider)
                .map(|(bp, mat)| (bp.map(|b| b.size), mat))
                .unwrap_or((None, None));
            let impactor: ContactMaterial = other_mat.map(Into::into).unwrap_or_default();

            // Treat the impactor as a sphere of roughly its smallest half-extent.
            let impactor_radius_m = other_size
                .map(|s| s.min_element() * 0.5)
                .filter(|r| *r > 0.0)
                .unwrap_or(0.25);

            let Some(dent) =
                dent_from_impulse(impulse, inv_mass_sum, target, impactor, impactor_radius_m)
            else {
                continue;
            };

            // Deepest contact point, in world space — a glancing multi-point
            // manifold should dent (or crack) where it actually bit hardest,
            // not at whichever point happens to be stored first. Needed by
            // both the fracture and dent paths below.
            let Some(world_point) = pair
                .manifolds
                .iter()
                .flat_map(|m| m.points.iter())
                .max_by(|a, b| a.penetration.total_cmp(&b.penetration))
                .map(|p| p.point)
            else {
                continue;
            };

            // ── crack instead of dent when the hit is hard enough ─────────
            //
            // Recompute the collision energy here (the same quantity
            // `dent_from_impulse` used) and compare it against the Griffith
            // threshold. Above it, the part splits and the dent is skipped —
            // denting a body that is about to be replaced by two fragments
            // would be wasted work and a visible double-response.
            let m_eff = if inv_mass_sum > 0.0 { 1.0 / inv_mass_sum } else { 0.0 };
            let energy = if m_eff > 0.0 {
                (impulse * impulse) / (2.0 * m_eff)
            } else {
                0.0
            };
            let threshold = fracture_energy_threshold(target, base_part.size);

            if energy > threshold {
                // A crack runs perpendicular to the tensile stress the impact
                // sets up, i.e. the fracture PLANE contains the impact
                // direction — an object struck from above splits down through
                // itself rather than being sliced horizontally. So the plane's
                // normal is perpendicular to the impact direction.
                let impact_dir = pair
                    .total_normal_impulse()
                    .normalize_or_zero();
                let crack_normal = if impact_dir == Vec3::ZERO {
                    Vec3::X
                } else {
                    impact_dir.any_orthonormal_vector()
                };

                info!(
                    "💢 fracture-trigger: entity={target_collider:?} approach={approach:.2}m/s \
                     J={impulse:.1}N·s E={energy:.1}J > threshold {threshold:.1}J → crack normal {crack_normal:?}"
                );

                cooldowns.insert(target_collider, IMPACT_COOLDOWN_SECS);
                fractures.write(FractureMeshEvent {
                    entity: target_collider,
                    origin: world_point.into(),
                    normal: crack_normal,
                    direction: impact_dir,
                    energy,
                });
                continue;
            }

            // Everything above is in world metres; the deformation pipeline
            // works in the mesh's LOCAL space (primitive parts are unit meshes
            // scaled by their transform), so convert both the point and the
            // magnitudes rather than mixing units.
            let affine = gt.affine();
            let inv_affine = affine.inverse();
            let local_point = inv_affine.transform_point3(world_point.into());

            // Dent inward: from the contact point toward the mesh origin. Using
            // the geometry rather than the contact normal avoids depending on
            // Avian's manifold normal orientation convention, which flips with
            // collider ordering.
            let local_dir = (-local_point).normalize_or_zero();
            if local_dir == Vec3::ZERO {
                continue;
            }

            // Clamp so no single impact can turn a part inside out. The
            // pipeline clamps again per-vertex; this keeps the event itself
            // sane for any other consumer.
            let max_depth_m = base_part.size.min_element() * 0.25;
            let depth_m = dent.depth_m.min(max_depth_m);
            if !(depth_m > 0.0) || !(dent.radius_m > 0.0) {
                continue;
            }

            // Emit METRES, not local units. Converting here used the scale
            // along the dent DIRECTION (0.6 on a thin plate) while the falloff
            // actually spreads across the perpendicular axes (6 and 4), which
            // stretched the affected region ~10x along X. The consumer owns the
            // conversion now, because it is the one that knows each vertex's
            // position and can scale the offset per-axis.

            info!(
                "🩹 impact-deform: entity={target_collider:?} approach={approach:.2}m/s \
                 J={impulse:.1}N·s depth={depth_m:.4}m radius={:.3}m permanent={} (scale={})",
                dent.radius_m, dent.permanent, config.scale
            );

            cooldowns.insert(target_collider, IMPACT_COOLDOWN_SECS);
            impacts.write(ImpactDeformEvent {
                entity: target_collider,
                // Local mesh space — matches the vertex positions.
                point: local_point,
                // Direction is local; the LENGTH is the dent depth in METRES.
                force: local_dir * depth_m,
                // Metres.
                radius: dent.radius_m,
                permanent: dent.permanent,
            });
        }
    }
}

/// Switch the deformation pipeline on when Play starts.
fn enable_deformation_on_play(mut config: ResMut<DeformationConfig>) {
    config.enabled = true;
    info!("🩹 Deformation enabled for play session");
}

/// Switch the pipeline off on Stop and restore every deformed mesh.
///
/// "Stop always restores" is an engine invariant: runtime damage must not
/// survive into Edit mode, where it would be indistinguishable from authored
/// geometry and could be saved as if it were.
fn disable_deformation_on_stop(mut config: ResMut<DeformationConfig>) {
    config.enabled = false;
}

/// Edit mode does not simulate, so deformation starts switched off — matching
/// `play_mode::pause_physics_on_startup`, which pauses `Time<Physics>` for the
/// same reason.
fn disable_deformation_on_startup(mut config: ResMut<DeformationConfig>) {
    config.enabled = false;
}

// ── plugin ───────────────────────────────────────────────────────────────

/// Wires Avian collisions into the vertex-deformation pipeline and gates that
/// pipeline to play sessions.
pub struct DeformationBridgePlugin;

impl Plugin for DeformationBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, disable_deformation_on_startup)
            .add_systems(OnEnter(PlayModeState::Playing), enable_deformation_on_play)
            .add_systems(
                OnExit(PlayModeState::Playing),
                (
                    disable_deformation_on_stop,
                    eustress_common::realism::deformation::systems::restore_all_deformables,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    enable_collision_events_for_deformables,
                    bridge_collisions_to_deformation,
                )
                    .chain()
                    .run_if(in_state(PlayModeState::Playing)),
            );
    }
}
