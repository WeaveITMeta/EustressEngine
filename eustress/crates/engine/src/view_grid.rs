//! # Editor grids
//!
//! Two grids, one of which is shown at a time while `EditorSettings::show_grid`
//! is on:
//!
//! - The **ground grid**, for perspective and free orthographic views: lines
//!   every `EditorSettings::grid_size` metres on y = 0, a centimetre up so a
//!   baseplate's top face never hides it, centred under the orbit point and
//!   fading out towards its edge. Its spacing grows by powers of ten as the
//!   view pulls away. The red X and blue Z axes run through the origin and a
//!   short green Y axis stands at it.
//! - The **view-plane grid**, for orthographic axis views: every 2D view,
//!   and Front, Top, Right... in 3D orthographic.
//!
//! Axis views look straight along a world axis, which puts the ground grid
//! edge-on, so the view-plane grid lies on the view's own working plane, the
//! plane through the origin that 2D edits on. Parts in front of the plane
//! cover it and its lines cross parts behind it, so the grid also shows
//! which side of the plane each part is on. It sits a centimetre in front of
//! the plane, so a floor lying on the plane (a baseplate seen from above) or
//! a backdrop wall behind it never hides it. When the camera is behind the
//! plane, which only a 3D orthographic view can be, the grid is drawn at the
//! back of the view instead.
//!
//! Its spacing follows the zoom in powers of ten (roughly 10 to 100 cells
//! across the view height). In both grids each line's strength comes from
//! how far apart its own decade sits on screen: fine lines fade out as they
//! crowd together and coarser ones strengthen, so zooming never pops a grid
//! into a solid wash or makes it vanish. The lines through the origin take
//! the axis colours.
//!
//! Both are line-list meshes because Studio does not build bevy's gizmo
//! renderer (`bevy_gizmos_render`): `Gizmos` lines are recorded and never
//! drawn. They are on their own render layer, which only editor cameras add,
//! so the AI camera's captures never include them. Each mesh is rebuilt only
//! when what it depends on changes.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers, VisibilitySystems};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

use crate::camera_controller::{CameraView, EustressCamera, ORTHO_3D_DEPTH, PLANE_2D_DEPTH};
use crate::editor_settings::EditorSettings;

/// The render layer the grid is on. Editor cameras see it alongside the
/// scene's layer 0; the Slint overlay uses 31.
pub const VIEW_GRID_LAYER: usize = 30;

/// How far in front of the working plane the grid sits. A depth step across a
/// 20 km orthographic view is about a millimetre, so a face lying exactly on
/// the plane never flickers through the grid.
const PLANE_LIFT: f32 = 0.01;

pub struct ViewGridPlugin;

impl Plugin for ViewGridPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (show_view_grid_to_editor_cameras, sync_view_grid, sync_ground_grid)
                .before(VisibilitySystems::VisibilityPropagate)
                .run_if(resource_exists::<Assets<StandardMaterial>>),
        );
    }
}

#[derive(Component)]
struct ViewGrid;

/// The axis view `cam` renders from exactly, once it is fully orthographic.
pub fn orthographic_axis_view(cam: &EustressCamera) -> Option<CameraView> {
    if cam.ortho_blend < 1.0 {
        return None;
    }
    cam.exact_view().filter(|view| view.basis().is_some())
}

/// Everything the grid mesh depends on. The mesh is rebuilt when it changes.
#[derive(Clone, Copy, PartialEq, Debug)]
struct GridView {
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    pivot: Vec3,
    /// Visible height in metres.
    height: f32,
    aspect: f32,
    pixels_tall: f32,
    /// Where the lines lie along `forward`.
    depth: f32,
}

/// The grid's position along `forward`: just in front of the working plane
/// while that plane is inside the view's depth range, otherwise near the back
/// of the range. Mirrors the near and far planes `camera_controller` gives an
/// orthographic view (orthographic views have no dolly pull-back).
fn grid_depth(forward: Vec3, pivot: Vec3, distance: f32, is_2d: bool) -> f32 {
    let plane = -PLANE_LIFT;
    let camera = pivot.dot(forward) - distance;
    let (near, far, slab) = if is_2d {
        (distance - PLANE_2D_DEPTH, distance + PLANE_2D_DEPTH, PLANE_2D_DEPTH)
    } else {
        (0.0, distance + ORTHO_3D_DEPTH, ORTHO_3D_DEPTH)
    };
    if plane - camera > near + PLANE_LIFT && plane - camera < far {
        plane
    } else {
        pivot.dot(forward) + slab * 0.98
    }
}

/// The colour of the line through the origin running along `direction`:
/// red X, green Y, blue Z.
fn axis_color(direction: Vec3) -> Color {
    let a = direction.abs();
    if a.x > 0.5 {
        Color::srgba(1.0, 0.25, 0.25, 0.9)
    } else if a.y > 0.5 {
        Color::srgba(0.3, 0.9, 0.3, 0.9)
    } else {
        Color::srgba(0.3, 0.45, 1.0, 0.9)
    }
}

/// Opacity of grid line `k` (in units of the minor spacing) when minor lines
/// sit `minor_px` pixels apart. Every tenth and hundredth line belongs to a
/// coarser decade that sits ten or a hundred times further apart on screen,
/// so fine lines fade out as they crowd and coarse ones stay.
fn decade_alpha(minor_px: f32, k: i64) -> f32 {
    let decade = if k % 100 == 0 {
        100.0
    } else if k % 10 == 0 {
        10.0
    } else {
        1.0
    };
    ((minor_px * decade - 8.0) / 50.0).clamp(0.0, 1.0) * 0.45
}

/// Linear RGBA, the form a mesh's vertex colours take.
fn rgba(color: Color) -> [f32; 4] {
    let c = color.to_linear();
    [c.red, c.green, c.blue, c.alpha]
}

impl GridView {
    fn of(cam: &EustressCamera, camera: &Camera) -> Option<Self> {
        let (forward, up) = orthographic_axis_view(cam)?.basis()?;
        let size = camera.logical_viewport_size()?;
        let height = cam.ortho_height();
        if !(height.is_finite() && height > 0.0 && size.y > 0.0) {
            return None;
        }
        Some(Self {
            right: forward.cross(up),
            up,
            forward,
            pivot: cam.pivot,
            height,
            aspect: size.x / size.y,
            pixels_tall: size.y,
            depth: grid_depth(forward, cam.pivot, cam.distance, cam.is_2d()),
        })
    }

    /// The grid entity's position: on the grid's plane, straight in line
    /// with the pivot. Transparent meshes sort by their origin, so this sorts
    /// the grid against other blended meshes by the plane's own depth.
    fn center(&self) -> Vec3 {
        self.pivot + self.forward * (self.depth - self.pivot.dot(self.forward))
    }

    /// Line-list positions relative to [`Self::center`], and a linear RGBA
    /// colour per vertex.
    fn lines(&self) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        let minor = 10f32.powf((self.height / 10.0).log10().floor());
        let minor_px = self.pixels_tall * minor / self.height;
        let center = self.center();
        let (center_u, center_v) = (center.dot(self.right), center.dot(self.up));
        let (half_w, half_h) = (self.height * self.aspect * 0.55, self.height * 0.55);

        let mut positions = Vec::new();
        let mut colors = Vec::new();
        // Lines of constant `right` coordinate run along `up`, and vice versa.
        for (across, along, center_across, half_across, half_along) in [
            (self.right, self.up, center_u, half_w, half_h),
            (self.up, self.right, center_v, half_h, half_w),
        ] {
            let first = ((center_across - half_across) / minor).floor() as i64;
            let last = ((center_across + half_across) / minor).ceil() as i64;
            if last - first > 1_000 {
                continue;
            }
            for k in first..=last {
                let color = if k == 0 {
                    axis_color(along)
                } else {
                    let alpha = decade_alpha(minor_px, k);
                    if alpha < 0.01 {
                        continue;
                    }
                    Color::srgba(0.55, 0.55, 0.6, alpha)
                };
                let color = rgba(color);
                let offset = across * (k as f32 * minor - center_across);
                positions.push((offset - along * half_along).to_array());
                positions.push((offset + along * half_along).to_array());
                colors.extend([color, color]);
            }
        }
        (positions, colors)
    }
}

#[derive(Component)]
struct GroundGrid;

/// How far above y = 0 the ground grid lies, so a baseplate whose top face is
/// at 0 never hides it.
const GROUND_LIFT: f32 = 0.01;

/// Pieces each ground line is cut into, so its opacity can fall off towards
/// the edge of the grid instead of ending in a hard square.
const GROUND_SEGMENTS: usize = 8;

/// Everything the ground grid mesh depends on, quantised so orbiting and
/// small zooms reuse the mesh instead of rebuilding it every frame.
#[derive(Clone, Copy, PartialEq, Debug)]
struct GroundView {
    /// Where the grid is centred on y = 0 (x, z), snapped to its coarse lines.
    center: Vec2,
    /// Line spacing: the user's grid size times a power of ten.
    minor: f32,
    /// On-screen pixels between minor lines at the pivot, in quarter pixels.
    minor_px: f32,
    /// Radius the lines fade out over.
    extent: f32,
}

impl GroundView {
    /// The ground grid for a perspective or free orthographic view. `None` in
    /// an orthographic axis view, where the view-plane grid takes over.
    fn of(cam: &EustressCamera, camera: &Camera, cell: f32) -> Option<Self> {
        if orthographic_axis_view(cam).is_some() {
            return None;
        }
        let size = camera.logical_viewport_size()?;
        // How far away the ground in view is: the orbit distance, or the
        // camera's height when it looks down from higher than that.
        let (_, eye) = cam.frame();
        let reach = cam.distance.max(eye.y.abs());
        Self::build(cell, cam.pivot, reach, cam.fov, size.y)
    }

    fn build(cell: f32, pivot: Vec3, reach: f32, fov: f32, pixels_tall: f32) -> Option<Self> {
        if !(cell.is_finite() && cell > 0.0 && reach.is_finite() && pixels_tall > 0.0) {
            return None;
        }
        let reach = reach.max(cell);
        // `cell`-sized lines up close, then ten times coarser for every
        // decade of distance, so a few dozen to a few hundred cells span the
        // ground in view.
        let minor = cell * 10f32.powf((reach / (cell * 30.0)).log10().floor().max(0.0));
        let seen = 2.0 * reach * (fov * 0.5).tan();
        let minor_px = (pixels_tall * minor / seen * 4.0).round() / 4.0;
        let coarse = minor * 10.0;
        let extent = ((reach * 8.0).min(minor * 150.0) / coarse).ceil().max(2.0) * coarse;
        let center = Vec2::new(
            (pivot.x / coarse).round() * coarse,
            (pivot.z / coarse).round() * coarse,
        );
        Some(Self { center, minor, minor_px, extent })
    }

    fn translation(&self) -> Vec3 {
        Vec3::new(self.center.x, GROUND_LIFT, self.center.y)
    }

    /// Line-list positions relative to [`Self::translation`], and a linear
    /// RGBA colour per vertex. Grid lines skip the origin; the X and Z axes
    /// take its place in their colours, and a short green Y axis stands at it.
    fn lines(&self) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let fade = |p: Vec2| (1.0 - (p.length() / self.extent).powi(2)).max(0.0);
        let mut push_faded = |from: Vec2, to: Vec2, color: Color| {
            let [red, green, blue, alpha] = rgba(color);
            for s in 0..GROUND_SEGMENTS {
                let a = from.lerp(to, s as f32 / GROUND_SEGMENTS as f32);
                let b = from.lerp(to, (s + 1) as f32 / GROUND_SEGMENTS as f32);
                for p in [a, b] {
                    positions.push([p.x, 0.0, p.y]);
                    colors.push([red, green, blue, alpha * fade(p)]);
                }
            }
        };

        let n = (self.extent / self.minor).round() as i64;
        let first_x = (self.center.x / self.minor).round() as i64;
        let first_z = (self.center.y / self.minor).round() as i64;
        for k in -n..=n {
            // A line of constant x (running along z), then one of constant z.
            for (index, offset_axis) in [(first_x + k, Vec2::X), (first_z + k, Vec2::Y)] {
                if index == 0 {
                    continue;
                }
                let alpha = decade_alpha(self.minor_px, index);
                if alpha < 0.01 {
                    continue;
                }
                let along = Vec2::new(offset_axis.y, offset_axis.x);
                let offset = offset_axis * (k as f32 * self.minor);
                push_faded(
                    offset - along * self.extent,
                    offset + along * self.extent,
                    Color::srgba(0.55, 0.55, 0.6, alpha),
                );
            }
        }

        // The origin, relative to the grid's centre.
        let origin = -self.center;
        if origin.y.abs() <= self.extent {
            let z = Vec2::new(0.0, origin.y);
            push_faded(z - Vec2::X * self.extent, z + Vec2::X * self.extent, axis_color(Vec3::X));
        }
        if origin.x.abs() <= self.extent {
            let x = Vec2::new(origin.x, 0.0);
            push_faded(x - Vec2::Y * self.extent, x + Vec2::Y * self.extent, axis_color(Vec3::Z));
        }
        if origin.length() <= self.extent {
            let up = rgba(axis_color(Vec3::Y));
            positions.push([origin.x, 0.0, origin.y]);
            positions.push([origin.x, (self.minor * 5.0).max(5.0), origin.y]);
            colors.extend([up, up]);
        }
        (positions, colors)
    }
}

fn sync_ground_grid(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    cameras: Query<(&EustressCamera, &Camera), With<Camera3d>>,
    mut grid: Query<(&Mesh3d, &mut Transform, &mut Visibility), With<GroundGrid>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut built_for: Local<Option<GroundView>>,
) {
    if grid.is_empty() {
        spawn_grid_mesh(&mut commands, &mut meshes, &mut materials, "GroundGrid", GroundGrid);
        return;
    }
    let Ok((mesh, mut transform, mut visibility)) = grid.single_mut() else { return };

    let view = settings
        .show_grid
        .then(|| {
            cameras
                .iter()
                .filter(|(_, camera)| camera.is_active)
                .find_map(|(cam, camera)| GroundView::of(cam, camera, settings.grid_size))
        })
        .flatten();
    let Some(view) = view else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };

    if *built_for != Some(view) {
        let (positions, colors) = view.lines();
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else { return };
        if positions.is_empty() {
            visibility.set_if_neq(Visibility::Hidden);
            return;
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        transform.translation = view.translation();
        *built_for = Some(view);
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// Give each editor camera the grid's layer alongside the ones it has.
fn show_view_grid_to_editor_cameras(
    mut commands: Commands,
    cameras: Query<(Entity, Option<&RenderLayers>), Added<EustressCamera>>,
) {
    for (entity, layers) in &cameras {
        let layers = layers.cloned().unwrap_or_default().with(VIEW_GRID_LAYER);
        commands.entity(entity).insert(layers);
    }
}

fn sync_view_grid(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    cameras: Query<(&EustressCamera, &Camera), With<Camera3d>>,
    mut grid: Query<(&Mesh3d, &mut Transform, &mut Visibility), With<ViewGrid>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut built_for: Local<Option<GridView>>,
) {
    if grid.is_empty() {
        spawn_grid_mesh(&mut commands, &mut meshes, &mut materials, "ViewGrid", ViewGrid);
        return;
    }
    let Ok((mesh, mut transform, mut visibility)) = grid.single_mut() else { return };

    let view = settings
        .show_grid
        .then(|| {
            cameras
                .iter()
                .filter(|(_, camera)| camera.is_active)
                .find_map(|(cam, camera)| GridView::of(cam, camera))
        })
        .flatten();
    let Some(view) = view else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };

    if *built_for != Some(view) {
        let (positions, colors) = view.lines();
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else { return };
        if positions.is_empty() {
            visibility.set_if_neq(Visibility::Hidden);
            return;
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        transform.translation = view.center();
        *built_for = Some(view);
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// Spawn a hidden, empty grid mesh carrying `marker`: an unlit, blended
/// line list with per-vertex colours on the grid layer.
fn spawn_grid_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    name: &'static str,
    marker: impl Component,
) {
    let mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 2])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 2]);
    let material = StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        fog_enabled: false,
        ..default()
    };
    commands.spawn((
        Name::new(name),
        marker,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(material)),
        Transform::IDENTITY,
        Visibility::Hidden,
        RenderLayers::layer(VIEW_GRID_LAYER),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        eustress_common::adornments::Adornment { meta: true },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2D view of `plane`, `height` metres tall, 20 m from its pivot.
    fn view_2d(plane: CameraView, height: f32, pivot: Vec3) -> GridView {
        let (forward, up) = plane.basis().unwrap();
        GridView {
            right: forward.cross(up),
            up,
            forward,
            pivot,
            height,
            aspect: 16.0 / 9.0,
            pixels_tall: 900.0,
            depth: grid_depth(forward, pivot, 20.0, true),
        }
    }

    fn front(height: f32, pivot: Vec3) -> GridView {
        view_2d(CameraView::Front, height, pivot)
    }

    /// Vertical lines' x positions in world space, sorted.
    fn vertical_xs(view: GridView) -> Vec<f32> {
        let center = view.center();
        let mut xs: Vec<f32> = view
            .lines()
            .0
            .chunks(2)
            .filter(|p| p[0][0] == p[1][0])
            .map(|p| p[0][0] + center.x)
            .collect();
        xs.sort_by(f32::total_cmp);
        xs
    }

    #[test]
    fn the_grid_lies_just_in_front_of_the_working_plane() {
        // Front looks down -Z from +Z, so "in front" is +Z.
        let view = front(24.0, Vec3::new(3.0, 2.0, 0.0));
        assert_eq!(view.right, Vec3::X);
        assert_eq!(view.center(), Vec3::new(3.0, 2.0, PLANE_LIFT));
        let (positions, colors) = view.lines();
        assert!(!positions.is_empty());
        assert_eq!(positions.len(), colors.len());
        // Every vertex lies in the centre's plane.
        assert!(positions.iter().all(|p| p[2] == 0.0), "{positions:?}");
    }

    #[test]
    fn from_above_the_grid_lies_on_top_of_the_floor() {
        // A baseplate's top face at y = 0 must not hide the plan grid.
        let view = view_2d(CameraView::Top, 24.0, Vec3::new(1.0, 0.0, -4.0));
        assert_eq!(view.center(), Vec3::new(1.0, PLANE_LIFT, -4.0));
    }

    #[test]
    fn a_camera_behind_the_plane_gets_the_grid_as_a_backdrop() {
        let (forward, _) = CameraView::Front.basis().unwrap();
        // 3D orthographic with the pivot 5 m in front of the plane: the
        // camera sees the plane, so the grid is on it.
        assert_eq!(grid_depth(forward, Vec3::new(0.0, 0.0, 5.0), 20.0, false), -PLANE_LIFT);
        // Pivot 50 m behind the plane and the camera 20 m back from it: the
        // plane is behind the camera, so the grid backs the view instead.
        let pivot = Vec3::new(0.0, 0.0, -50.0);
        let depth = grid_depth(forward, pivot, 20.0, false);
        assert_eq!(depth, pivot.dot(forward) + ORTHO_3D_DEPTH * 0.98);
        // And that is inside the view: beyond the camera, short of the far plane.
        let camera = pivot.dot(forward) - 20.0;
        assert!(depth - camera > 0.0 && depth - camera < 20.0 + ORTHO_3D_DEPTH);
    }

    #[test]
    fn right_and_top_views_are_right_handed() {
        let right = |view: CameraView| {
            let (forward, up) = view.basis().unwrap();
            forward.cross(up)
        };
        // Looking down -X from +X, screen right is -Z; looking down from
        // above with -Z up the screen, screen right is +X.
        assert_eq!(right(CameraView::Right), Vec3::NEG_Z);
        assert_eq!(right(CameraView::Top), Vec3::X);
    }

    #[test]
    fn origin_lines_take_the_axis_colours() {
        let view = front(24.0, Vec3::ZERO);
        let center = view.center();
        let (positions, colors) = view.lines();
        let rgba = |c: Color| {
            let c = c.to_linear();
            [c.red, c.green, c.blue, c.alpha]
        };
        let line = |on_axis: fn([f32; 3]) -> bool| {
            positions
                .chunks(2)
                .zip(colors.chunks(2))
                .find(|(p, _)| {
                    p.iter().all(|v| on_axis([v[0] + center.x, v[1] + center.y, v[2]]))
                })
                .map(|(_, c)| c[0])
                .unwrap()
        };
        // The line along X through the origin is red, the one along Y green.
        assert_eq!(line(|v| v[1] == 0.0), rgba(Color::srgba(1.0, 0.25, 0.25, 0.9)));
        assert_eq!(line(|v| v[0] == 0.0), rgba(Color::srgba(0.3, 0.9, 0.3, 0.9)));
    }

    #[test]
    fn spacing_tracks_the_zoom_in_decades() {
        // 24 m tall: 1 m cells. 240 m tall: 10 m cells, and the same number
        // of lines.
        assert_eq!(
            front(24.0, Vec3::ZERO).lines().0.len(),
            front(240.0, Vec3::ZERO).lines().0.len()
        );
        let xs = vertical_xs(front(240.0, Vec3::ZERO));
        assert!(xs.len() > 10);
        assert!(xs.windows(2).all(|w| ((w[1] - w[0]) - 10.0).abs() < 1e-3), "{xs:?}");
    }

    #[test]
    fn crowded_minor_lines_drop_out() {
        // 99 m tall on 900 px: 1 m cells would be 9 px apart, too dense to
        // draw, so only the 10 m lines remain.
        let xs = vertical_xs(front(99.0, Vec3::ZERO));
        assert!(xs.len() > 5);
        assert!(xs.iter().all(|x| (x / 10.0).fract().abs() < 1e-4), "{xs:?}");
    }

    fn ground(cell: f32, pivot: Vec3, reach: f32) -> GroundView {
        GroundView::build(cell, pivot, reach, 70f32.to_radians(), 900.0).unwrap()
    }

    #[test]
    fn ground_spacing_is_the_grid_size_up_close_and_grows_by_decades() {
        assert_eq!(ground(1.0, Vec3::ZERO, 20.0).minor, 1.0);
        assert_eq!(ground(0.5, Vec3::ZERO, 12.0).minor, 0.5);
        assert_eq!(ground(1.0, Vec3::ZERO, 400.0).minor, 10.0);
        assert_eq!(ground(1.0, Vec3::ZERO, 4_000.0).minor, 100.0);
        // A nonsense grid size draws nothing rather than a million lines.
        assert!(GroundView::build(0.0, Vec3::ZERO, 20.0, 1.2, 900.0).is_none());
    }

    #[test]
    fn ground_lines_lie_on_the_ground_and_fade_out_at_the_edge() {
        let view = ground(1.0, Vec3::new(3.0, 0.0, -2.0), 20.0);
        assert_eq!(view.translation().y, GROUND_LIFT);
        let (positions, colors) = view.lines();
        assert_eq!(positions.len(), colors.len());
        let flat: Vec<_> = positions.iter().zip(&colors).filter(|(p, _)| p[1] == 0.0).collect();
        // Everything but the top of the Y axis lies on the ground.
        assert_eq!(flat.len(), positions.len() - 1);
        for (p, c) in &flat {
            let r = Vec2::new(p[0], p[2]).length();
            if r >= view.extent {
                assert_eq!(c[3], 0.0, "{p:?} is past the edge but visible");
            }
        }
        // Near the middle, lines are there to see.
        assert!(flat.iter().any(|(p, c)| Vec2::new(p[0], p[2]).length() < view.extent * 0.3 && c[3] > 0.1));
    }

    #[test]
    fn the_origin_gets_axes_in_their_colours() {
        let view = ground(1.0, Vec3::ZERO, 20.0);
        let (positions, colors) = view.lines();
        let bright = |c: Color| {
            let [r, g, b, _] = rgba(c);
            colors.iter().any(|v| v[0] == r && v[1] == g && v[2] == b && v[3] > 0.5)
        };
        assert!(bright(axis_color(Vec3::X)), "red X axis");
        assert!(bright(axis_color(Vec3::Z)), "blue Z axis");
        let y_top = positions.iter().position(|p| p[1] > 0.0).expect("green Y axis");
        assert_eq!(colors[y_top], rgba(axis_color(Vec3::Y)));
        assert_eq!(positions[y_top], [0.0, 5.0, 0.0]);

        // Far from the origin, no axes at all.
        let (positions, colors) = ground(1.0, Vec3::new(5_000.0, 0.0, 5_000.0), 20.0).lines();
        assert!(positions.iter().all(|p| p[1] == 0.0));
        let red = rgba(axis_color(Vec3::X));
        assert!(colors.iter().all(|v| v[..3] != red[..3]));
    }

    #[test]
    fn an_orbit_reuses_the_ground_mesh() {
        // Sliding the pivot within one coarse cell keeps the same grid.
        assert_eq!(ground(1.0, Vec3::new(0.2, 0.0, 0.1), 20.0), ground(1.0, Vec3::new(-0.3, 0.0, 0.4), 20.0));
    }
}
