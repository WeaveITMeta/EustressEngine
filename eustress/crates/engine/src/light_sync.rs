//! # Light classes in the editor
//!
//! The Bevy lights behind PointLight, SpotLight, SurfaceLight and
//! DirectionalLight are built and kept in step by
//! `eustress_common::plugins::light_classes`, which `SharedLightingPlugin`
//! adds for Studio and the Player alike (brightness model, faces, emitters
//! and the culler's budget are documented there). This plugin adds what only
//! the editor needs, because a light has no geometry of its own:
//!
//! - **A selected light shows its reach.** Bevy's light gizmo is switched on
//!   for it (`ShowLightGizmo` on the entity holding its Bevy light): the
//!   range sphere of a PointLight, the cone of a SpotLight or SurfaceLight,
//!   the direction of a DirectionalLight. `gizmo_tools` keeps
//!   `draw_all = false`, so unselected lights cost nothing.
//! - **A light that is not inside a part is marked and clickable.** It gets
//!   [`LightPickable`], which the viewport picker (`part_selection`) treats
//!   as a small box at the light, and a marker is drawn at it in Edit mode.
//!   A light inside a part is selected through its part, as in Roblox.
//!
//! `EUSTRESS_LIGHT_MARKERS=0` hides the markers.

use bevy::light::gizmos::{LightGizmoConfigGroup, ShowLightGizmo};
use bevy::prelude::*;

use eustress_common::classes::{
    BasePart, EustressDirectionalLight, EustressPointLight, EustressSpotLight, Instance,
    SurfaceLight,
};
use eustress_common::plugins::light_classes::{render_entity, LightEmitterLink};

use crate::selection_box::Selected;

/// Registers the editor's light systems.
pub struct LightClassPlugin;

impl Plugin for LightClassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                show_selected_light_gizmos,
                mark_pickable_lights,
                draw_light_markers.run_if(markers_enabled),
            ),
        );
        add_light_play_snapshot::<EustressPointLight>(app);
        add_light_play_snapshot::<EustressSpotLight>(app);
        add_light_play_snapshot::<SurfaceLight>(app);
        add_light_play_snapshot::<EustressDirectionalLight>(app);
    }
}

// ============================================================================
// Play / Stop
// ============================================================================

/// One light class's authoring components as they were when Play started.
/// Stop puts them back on every stop path, so a script that dims or recolours
/// a light during Play leaves the authored light as it was (and nothing can
/// save the played value). Lights spawned during Play are not in it; Stop
/// despawns those.
#[derive(Resource)]
struct LightPlaySnapshot<T: Send + Sync + 'static> {
    saved: Vec<(Entity, T)>,
}

impl<T: Send + Sync + 'static> Default for LightPlaySnapshot<T> {
    fn default() -> Self {
        Self { saved: Vec::new() }
    }
}

/// Snapshot `T` as Play starts from Edit (resuming from Pause also enters
/// Playing, and must keep the first snapshot), and restore it when Edit
/// mode returns, after the scripts' `on_exit` has run.
fn add_light_play_snapshot<T>(app: &mut App)
where
    T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + serde::Serialize,
{
    use crate::play_mode::PlayModeState;
    app.init_resource::<LightPlaySnapshot<T>>()
        .add_systems(
            OnTransition { exited: PlayModeState::Editing, entered: PlayModeState::Playing },
            snapshot_lights_on_play::<T>,
        )
        .add_systems(
            OnEnter(PlayModeState::Editing),
            restore_lights_on_stop::<T>.after(crate::soul::rune_api::cleanup_scripts_on_stop),
        );
}

fn snapshot_lights_on_play<T: Component + Clone>(
    lights: Query<(Entity, &T)>,
    mut snapshot: ResMut<LightPlaySnapshot<T>>,
) {
    snapshot.saved = lights.iter().map(|(e, c)| (e, c.clone())).collect();
}

fn restore_lights_on_stop<T>(mut snapshot: ResMut<LightPlaySnapshot<T>>, mut lights: Query<&mut T>)
where
    T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + serde::Serialize,
{
    for (entity, authored) in std::mem::take(&mut snapshot.saved) {
        let Ok(mut live) = lights.get_mut(entity) else { continue };
        // Written only when it differs, so an untouched light is not rebuilt.
        if !same_value(&*live, &authored) {
            *live = authored;
        }
    }
}

/// Equality through the serialized form (the light components hold `Color`
/// and derive no `PartialEq`).
fn same_value<T: serde::Serialize>(a: &T, b: &T) -> bool {
    match (toml::Value::try_from(a), toml::Value::try_from(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// A light the viewport can pick directly: one that is not inside a part.
#[derive(Component, Debug, Clone, Copy)]
pub struct LightPickable;

/// Marks a `ShowLightGizmo` this plugin added for the selection, so it is
/// the only one it takes away.
#[derive(Component)]
struct SelectionLightGizmo;

/// Entities of any light class.
type LightClass = Or<(
    With<EustressPointLight>,
    With<EustressSpotLight>,
    With<SurfaceLight>,
    With<EustressDirectionalLight>,
)>;

/// Markers are drawn for lights within this distance of the camera.
const MARKER_RANGE_M: f32 = 150.0;

/// Marker size per metre of distance, so it keeps its size on screen.
const MARKER_SCALE_PER_M: f32 = 0.02;

/// The pick box around a free light, per metre of distance from the ray's
/// origin, and its bounds in metres. `part_selection` reads these.
pub const PICK_SIZE_PER_M: f32 = 0.04;
pub const PICK_SIZE_MIN_M: f32 = 0.3;
pub const PICK_SIZE_MAX_M: f32 = 4.0;

/// Full size of the pick box around a free light seen from `distance`.
pub fn pick_box_size(distance: f32) -> f32 {
    (distance * PICK_SIZE_PER_M).clamp(PICK_SIZE_MIN_M, PICK_SIZE_MAX_M)
}

fn markers_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("EUSTRESS_LIGHT_MARKERS").ok().as_deref().map(str::trim),
            Some("0") | Some("false") | Some("off")
        )
    })
}

/// Turn Bevy's light gizmo on for selected lights and off when they are
/// deselected. A SpotLight's or SurfaceLight's Bevy light is on its emitter,
/// which appears a frame after the light; the gizmo moves there when it does.
fn show_selected_light_gizmos(
    mut commands: Commands,
    selected: Query<(Entity, Option<&LightEmitterLink>), (With<Selected>, LightClass)>,
    shown: Query<Entity, With<SelectionLightGizmo>>,
) {
    let want: Vec<Entity> = selected.iter().map(|(e, link)| render_entity(e, link)).collect();
    for entity in &shown {
        if !want.contains(&entity) {
            if let Ok(mut ec) = commands.get_entity(entity) {
                ec.remove::<(ShowLightGizmo, SelectionLightGizmo)>();
            }
        }
    }
    for entity in want {
        if shown.get(entity).is_err() {
            if let Ok(mut ec) = commands.get_entity(entity) {
                ec.try_insert((ShowLightGizmo::default(), SelectionLightGizmo));
            }
        }
    }
}

/// Keep [`LightPickable`] on exactly the lights that are not inside a part.
/// Runs a pass only when a light spawned or something was reparented.
#[allow(clippy::type_complexity)]
fn mark_pickable_lights(
    mut commands: Commands,
    lights: Query<(Entity, Option<&ChildOf>, Has<LightPickable>), LightClass>,
    parts: Query<(), With<BasePart>>,
    changed: Query<(), (LightClass, Or<(Added<Instance>, Changed<ChildOf>)>)>,
    mut unparented: RemovedComponents<ChildOf>,
) {
    let reparented = unparented.read().count() > 0;
    if changed.is_empty() && !reparented {
        return;
    }
    for (entity, child_of, pickable) in &lights {
        let in_part = child_of.is_some_and(|c| parts.contains(c.parent()));
        if in_part == pickable {
            let Ok(mut ec) = commands.get_entity(entity) else { continue };
            if in_part {
                ec.remove::<LightPickable>();
            } else {
                ec.try_insert(LightPickable);
            }
        }
    }
}

/// A small icon at every free light near the camera, in the light's colour
/// (faint when it is switched off): a spoked ball for a PointLight, a ball
/// with its beam for a SpotLight or SurfaceLight, parallel rays for a
/// DirectionalLight. Edit mode only.
#[allow(clippy::type_complexity)]
fn draw_light_markers(
    cameras: Query<(&Camera, &GlobalTransform)>,
    lights: Query<
        (
            &GlobalTransform,
            Option<&EustressPointLight>,
            Option<&EustressSpotLight>,
            Option<&SurfaceLight>,
            Option<&EustressDirectionalLight>,
            Option<&LightEmitterLink>,
        ),
        With<LightPickable>,
    >,
    emitters: Query<&GlobalTransform, Without<LightPickable>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut gizmos: Gizmos<LightGizmoConfigGroup>,
) {
    if !crate::play_mode::editor_input_enabled(play_state) {
        return;
    }
    let Some(eye) = cameras
        .iter()
        .find(|(c, _)| c.order == 0)
        .map(|(_, gt)| gt.translation())
    else {
        return;
    };
    for (gt, point, spot, surface, directional, link) in &lights {
        let at = gt.translation();
        let distance = at.distance(eye);
        if distance > MARKER_RANGE_M {
            continue;
        }
        let s = (distance * MARKER_SCALE_PER_M).clamp(0.12, 2.0);
        let (color, enabled) = if let Some(l) = point {
            (l.color, l.enabled)
        } else if let Some(l) = spot {
            (l.color, l.enabled)
        } else if let Some(l) = surface {
            (l.color, l.enabled)
        } else if let Some(l) = directional {
            (l.color, l.enabled)
        } else {
            continue;
        };
        let color = if enabled { color.with_alpha(1.0) } else { color.with_alpha(0.3) };

        if point.is_some() {
            gizmos.sphere(at, s * 0.5, color).resolution(10);
            for axis in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
                gizmos.line(at + axis * s * 0.7, at + axis * s * 1.2, color);
            }
        } else if spot.is_some() || surface.is_some() {
            // The beam leaves along the emitter's -Z, which the face turns.
            let beam_rotation = link
                .and_then(|l| emitters.get(l.0).ok())
                .map(|e| e.compute_transform().rotation)
                .unwrap_or_else(|| gt.compute_transform().rotation);
            let axis = beam_rotation * Vec3::NEG_Z;
            gizmos.sphere(at, s * 0.4, color).resolution(10);
            gizmos.arrow(at + axis * s * 0.5, at + axis * s * 2.0, color).with_tip_length(s * 0.4);
            gizmos.circle(Isometry3d::new(at + axis * s * 2.0, beam_rotation), s * 0.6, color);
        } else {
            let rotation = gt.compute_transform().rotation;
            let axis = rotation * Vec3::NEG_Z;
            let side = rotation * Vec3::X * s * 0.5;
            for offset in [-side, Vec3::ZERO, side] {
                gizmos
                    .arrow(at + offset, at + offset + axis * s * 2.5, color)
                    .with_tip_length(s * 0.4);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play_mode::PlayModeState;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin));
        app.init_state::<PlayModeState>()
            .init_resource::<LightPlaySnapshot<EustressPointLight>>()
            .add_systems(
                OnTransition { exited: PlayModeState::Editing, entered: PlayModeState::Playing },
                snapshot_lights_on_play::<EustressPointLight>,
            )
            .add_systems(OnEnter(PlayModeState::Editing), restore_lights_on_stop::<EustressPointLight>);
        app.update();
        app
    }

    fn enter(app: &mut App, state: PlayModeState) {
        app.world_mut().resource_mut::<NextState<PlayModeState>>().set(state);
        app.update();
    }

    #[test]
    fn stop_puts_back_the_light_play_changed() {
        let mut app = app();
        let lamp = app
            .world_mut()
            .spawn(EustressPointLight { brightness: 2.0, ..Default::default() })
            .id();
        enter(&mut app, PlayModeState::Playing);
        app.world_mut().get_mut::<EustressPointLight>(lamp).unwrap().brightness = 9.0;
        // Pausing and resuming keeps the snapshot Play started with.
        enter(&mut app, PlayModeState::Paused);
        enter(&mut app, PlayModeState::Playing);
        app.world_mut().get_mut::<EustressPointLight>(lamp).unwrap().range = 1.0;
        enter(&mut app, PlayModeState::Editing);
        let light = app.world().get::<EustressPointLight>(lamp).unwrap();
        assert_eq!(light.brightness, 2.0);
        assert_eq!(light.range, EustressPointLight::default().range);
    }

    #[test]
    fn a_light_spawned_during_play_is_left_alone() {
        let mut app = app();
        enter(&mut app, PlayModeState::Playing);
        let flare = app
            .world_mut()
            .spawn(EustressPointLight { brightness: 5.0, ..Default::default() })
            .id();
        enter(&mut app, PlayModeState::Editing);
        assert_eq!(app.world().get::<EustressPointLight>(flare).unwrap().brightness, 5.0);
    }
}
