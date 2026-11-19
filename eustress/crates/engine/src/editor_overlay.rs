//! # Editor overlay layer
//!
//! Studio's overlays are drawn on their own render layer,
//! [`EDITOR_OVERLAY_LAYER`]: the hover and selection wireframes and corner
//! dots (`selection_box`), the move, scale and rotate handles
//! (`move_handles`, `scale_handles`, `rotate_handles`, drawn by
//! `adornment_renderer`), the decal placement preview (`decal_place_tool`)
//! and the gizmo groups (`gizmo_tools`).
//!
//! Cameras that draw to a window (the editor viewport camera and the Play
//! cameras) render this layer alongside the scene's layer 0. A camera that
//! draws to an image, such as the AI capture camera, renders layer 0 only,
//! so a capture shows the scene and never depends on where the mouse rests
//! or what is selected in the viewport.
//!
//! A camera draws a mesh only when their `RenderLayers` intersect, and render
//! layers are not inherited, so every overlay mesh entity carries the layer
//! itself. Observers put it on as an overlay marker is added. They run while
//! the command that spawns the entity is applied, and their own commands are
//! applied before the next system runs, so no frame ever draws an overlay on
//! layer 0.
//!
//! Lighting does not change: a light serves a camera when their layers
//! intersect, the scene's lights are on layer 0, which every editor camera
//! still renders, and no light is on this layer. Overlay meshes are unlit and
//! cast no shadows.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use eustress_common::adornments::Adornment;

use crate::selection_box::{HoverAdornment, SelectionAdornment};

/// The render layer editor overlays are on. The other layers in use: 0 is the
/// scene, 30 the editor grids and terrain brush cursor
/// (`view_grid::VIEW_GRID_LAYER`), 31 the Slint overlay and 47 the traced sky
/// light (`raytracing::sky::TRACED_SKY_LAYER`).
pub const EDITOR_OVERLAY_LAYER: usize = 29;

/// Puts editor overlays on [`EDITOR_OVERLAY_LAYER`] and gives every camera
/// that draws to a window that layer.
pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(tag_meta_adornment)
            .add_observer(tag_overlay::<SelectionAdornment>)
            .add_observer(tag_overlay::<HoverAdornment>)
            .add_observer(show_overlays_to_window_camera);
    }
}

/// Put `entity` on the overlay layer. The observers call it only for an
/// entity with no render layers of its own: one that has some (the grids and
/// the brush cursor are on `VIEW_GRID_LAYER`) is already editor-only and
/// keeps them.
fn put_on_overlay_layer(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_insert(RenderLayers::layer(EDITOR_OVERLAY_LAYER));
}

/// Every overlay Studio spawns carries `Adornment { meta: true }`, the marker
/// that also hides it from the Explorer: selection and hover wireframes,
/// corner dots, and every handle root and handle mesh.
fn tag_meta_adornment(
    add: On<bevy::ecs::lifecycle::Add, Adornment>,
    mut commands: Commands,
    adornments: Query<&Adornment, Without<RenderLayers>>,
) {
    let entity = add.event().entity;
    if adornments.get(entity).is_ok_and(|adornment| adornment.meta) {
        put_on_overlay_layer(&mut commands, entity);
    }
}

/// An overlay marked by `M`, whether or not it also carries an `Adornment`.
fn tag_overlay<M: Component>(
    add: On<bevy::ecs::lifecycle::Add, M>,
    mut commands: Commands,
    untagged: Query<(), Without<RenderLayers>>,
) {
    let entity = add.event().entity;
    if untagged.get(entity).is_ok() {
        put_on_overlay_layer(&mut commands, entity);
    }
}

/// A 3D camera that draws to a window renders the overlay layer as well as
/// the ones it has: the editor viewport camera and the Play cameras show the
/// same overlays. A camera that draws to an image (the AI capture camera, a
/// portal, a thumbnail) keeps its own layers and so never draws an overlay.
/// `view_grid` adds the grid layer to the editor cameras afterwards and keeps
/// this one.
fn show_overlays_to_window_camera(
    add: On<bevy::ecs::lifecycle::Add, Camera3d>,
    mut commands: Commands,
    cameras: Query<(Option<&RenderLayers>, &RenderTarget), With<Camera3d>>,
) {
    let entity = add.event().entity;
    let Ok((layers, target)) = cameras.get(entity) else { return };
    if !matches!(target, RenderTarget::Window(_)) {
        return;
    }
    let layers = layers.cloned().unwrap_or_default().with(EDITOR_OVERLAY_LAYER);
    commands.entity(entity).try_insert(layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorOverlayPlugin);
        app
    }

    fn layers(app: &App, entity: Entity) -> Option<RenderLayers> {
        app.world().get::<RenderLayers>(entity).cloned()
    }

    #[test]
    fn overlays_are_on_the_overlay_layer_from_the_moment_they_spawn() {
        let mut app = app();
        let overlay = Some(RenderLayers::layer(EDITOR_OVERLAY_LAYER));
        let hover = app
            .world_mut()
            .spawn((HoverAdornment, Adornment { meta: true }, Transform::default()))
            .id();
        let selection = app.world_mut().spawn(SelectionAdornment).id();
        let handle = app.world_mut().spawn((Adornment { meta: true }, Transform::default())).id();
        // No update has run: the layer is on before any frame could draw it.
        assert_eq!(layers(&app, hover), overlay);
        assert_eq!(layers(&app, selection), overlay);
        assert_eq!(layers(&app, handle), overlay);
        app.update();
        assert_eq!(layers(&app, hover), overlay);
    }

    #[test]
    fn scene_entities_stay_on_the_scene_layer() {
        let mut app = app();
        let part = app.world_mut().spawn(Transform::default()).id();
        let authored = app.world_mut().spawn(Adornment { meta: false }).id();
        app.update();
        assert_eq!(layers(&app, part), None);
        assert_eq!(layers(&app, authored), None);
    }

    #[test]
    fn an_overlay_with_its_own_layer_keeps_it() {
        let mut app = app();
        let grid = app
            .world_mut()
            .spawn((Adornment { meta: true }, RenderLayers::layer(30)))
            .id();
        assert_eq!(layers(&app, grid), Some(RenderLayers::layer(30)));
    }

    #[test]
    fn only_cameras_that_draw_to_a_window_render_the_overlay_layer() {
        let mut app = app();
        // A camera's default target is the primary window.
        let viewport = app.world_mut().spawn(Camera3d::default()).id();
        let layered = app.world_mut().spawn((Camera3d::default(), RenderLayers::layer(30))).id();
        let capture = app
            .world_mut()
            .spawn((Camera3d::default(), RenderTarget::Image(Handle::<Image>::default().into())))
            .id();
        assert_eq!(
            layers(&app, viewport),
            Some(RenderLayers::default().with(EDITOR_OVERLAY_LAYER))
        );
        assert_eq!(
            layers(&app, layered),
            Some(RenderLayers::layer(30).with(EDITOR_OVERLAY_LAYER))
        );
        // A camera that draws to an image (the AI capture camera) stays on
        // layer 0 and never intersects the overlay layer.
        assert_eq!(layers(&app, capture), None);
        assert!(!RenderLayers::default().intersects(&RenderLayers::layer(EDITOR_OVERLAY_LAYER)));
    }
}
