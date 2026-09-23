//! # View-plane grid
//!
//! The grid of an orthographic axis view: every 2D view, and Front, Top,
//! Right... in 3D orthographic. Those views look straight along a world axis,
//! which puts the ground grid edge-on, so this grid lies on the view's own
//! working plane, the plane through the origin that 2D edits on. Parts in
//! front of the plane cover it and its lines cross parts behind it, so the
//! grid also shows which side of the plane each part is on. It sits a
//! centimetre in front of the plane, so a floor lying on the plane (a
//! baseplate seen from above) or a backdrop wall behind it never hides it.
//! When the camera is behind the plane, which only a 3D orthographic view
//! can be, the grid is drawn at the back of the view instead.
//!
//! Spacing follows the zoom in powers of ten (roughly 10 to 100 cells across
//! the view height), and each line's strength comes from how far apart its
//! own decade sits on screen: fine lines fade out as they crowd together and
//! coarser ones strengthen, so zooming never pops the grid into a solid wash
//! or makes it vanish. The lines through the origin take the axis colours.
//!
//! It is a line-list mesh because Studio does not build bevy's gizmo renderer
//! (`bevy_gizmos_render`): `Gizmos` lines are recorded and never drawn. It is
//! on its own render layer, which only editor cameras add, so the AI camera's
//! captures never include it. The mesh is rebuilt only when the view changes.

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
            (show_view_grid_to_editor_cameras, sync_view_grid)
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

        let axis_color = |direction: Vec3| {
            let a = direction.abs();
            if a.x > 0.5 {
                Color::srgba(1.0, 0.25, 0.25, 0.9)
            } else if a.y > 0.5 {
                Color::srgba(0.3, 0.9, 0.3, 0.9)
            } else {
                Color::srgba(0.3, 0.45, 1.0, 0.9)
            }
        };

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
                    let decade = if k % 100 == 0 {
                        100.0
                    } else if k % 10 == 0 {
                        10.0
                    } else {
                        1.0
                    };
                    let alpha = ((minor_px * decade - 8.0) / 50.0).clamp(0.0, 1.0) * 0.45;
                    if alpha < 0.01 {
                        continue;
                    }
                    Color::srgba(0.55, 0.55, 0.6, alpha)
                };
                let color = color.to_linear();
                let color = [color.red, color.green, color.blue, color.alpha];
                let offset = across * (k as f32 * minor - center_across);
                positions.push((offset - along * half_along).to_array());
                positions.push((offset + along * half_along).to_array());
                colors.extend([color, color]);
            }
        }
        (positions, colors)
    }
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
        spawn_view_grid(&mut commands, &mut meshes, &mut materials);
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

fn spawn_view_grid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
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
        Name::new("ViewGrid"),
        ViewGrid,
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
}
