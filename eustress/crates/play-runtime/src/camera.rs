//! `workspace.CurrentCamera` in Play.
//!
//! With `CameraType = Scriptable` the script owns the play camera: its
//! `CFrame`, `FieldOfView`, and the Eustress `Projection` /
//! `OrthographicSize` properties are written onto the avatar camera, whose
//! orbit and follow systems stand down. Any other CameraType leaves the
//! avatar camera in charge (and `pull_mouse_hit` reports its pose back).
//!
//! Without an avatar (a Space with `Players.CharacterAutoLoads` off, or
//! after a script destroyed the character) the play camera is the
//! [`ScriptedPlayCamera`] Play starts with: it holds the editor's view until
//! a script makes the camera Scriptable, and stands aside while an avatar
//! camera exists.
//!
//! In every case the play camera renders into the visible 3D viewport, so
//! the player is centred in what the user sees rather than in the whole
//! window behind the editor panels.

use bevy::camera::{ScalingMode, Viewport};
use bevy::prelude::*;

use eustress_common::avatar::control::{AvatarCamera, AvatarCameraScripted, AVATAR_FOV_DEG};

use eustress_common::play_session::{PlayDataModel, PlayRole};

pub use eustress_common::play_session::ScriptedPlayCamera;

/// Show the avatar-free play camera exactly while there is no avatar camera,
/// so Play always renders through one camera: the avatar's when it has one.
pub fn show_script_camera_without_avatar(
    avatar_cams: Query<(), With<AvatarCamera>>,
    mut script_cams: Query<&mut Camera, (With<ScriptedPlayCamera>, Without<AvatarCamera>)>,
) {
    let want = avatar_cams.is_empty();
    for mut camera in script_cams.iter_mut() {
        if camera.is_active != want {
            camera.is_active = want;
        }
    }
}

/// A replica's play camera for every moment without an avatar. Studio's
/// `handle_start_play` spawns one; a joined Player gets it here, once,
/// posed at the Space's camera. [`show_script_camera_without_avatar`] keeps
/// it active exactly while no avatar camera exists.
pub fn ensure_replica_play_camera(
    mut commands: Commands,
    role: Option<Res<PlayRole>>,
    dm: Option<Res<PlayDataModel>>,
    existing: Query<(), With<ScriptedPlayCamera>>,
    host: Option<Res<eustress_common::avatar::AvatarHostConfig>>,
) {
    if role.as_deref().copied().unwrap_or_default() != PlayRole::Replica || !existing.is_empty() {
        return;
    }
    let Some(dm) = dm else { return };
    let pose = {
        let g = dm.dm.lock();
        g.current_camera().and_then(|c| g.get(c).and_then(|i| i.cframe())).map(|cf| cf.to_transform().with_scale(Vec3::ONE))
    };
    let order = host.as_deref().map_or(10, |h| h.seams.camera_order);
    commands.spawn((
        Camera3d::default(),
        Camera { order, is_active: false, ..default() },
        Projection::Perspective(PerspectiveProjection { fov: AVATAR_FOV_DEG.to_radians(), ..default() }),
        // The avatar camera's tonemapper, so both look the same.
        bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
        pose.unwrap_or_else(|| Transform::from_xyz(0.0, 12.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y)),
        ScriptedPlayCamera,
        Name::new("ScriptedPlayCamera"),
    ));
}

pub fn apply_scripted_camera(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    role: Option<Res<PlayRole>>,
    bounds: Option<Res<eustress_common::play_session::ViewportBounds>>,
    mut cams: Query<
        (Entity, &mut Camera, &mut Transform, &mut Projection, Option<&mut AvatarCamera>, Has<AvatarCameraScripted>),
        Or<(With<AvatarCamera>, With<ScriptedPlayCamera>)>,
    >,
) {
    let Some(dm) = dm else { return };
    // The avatar's camera when there is one, else the avatar-free camera.
    let target = cams
        .iter()
        .find(|(_, _, _, _, avatar, _)| avatar.is_some())
        .or_else(|| cams.iter().next())
        .map(|(e, ..)| e);
    let Some(Ok((entity, mut camera, mut tf, mut projection, mut avatar_cam, scripted))) = target.map(|e| cams.get_mut(e)) else {
        return;
    };

    // Render into the viewport rect.
    if let Some(b) = bounds.as_deref() {
        if b.width > 1.0 && b.height > 1.0 {
            let vp = Viewport {
                physical_position: UVec2::new(b.x.max(0.0) as u32, b.y.max(0.0) as u32),
                physical_size: UVec2::new(b.width as u32, b.height as u32),
                ..default()
            };
            let changed = camera
                .viewport
                .as_ref()
                .map_or(true, |v| v.physical_position != vp.physical_position || v.physical_size != vp.physical_size);
            if changed {
                camera.viewport = Some(vp);
            }
        }
    }

    let (camera_type, cf, fov, ortho, ortho_size) = {
        let g = dm.dm.lock();
        let Some(cam) = g.current_camera() else { return };
        let camera_type = g
            .get_prop(cam, "CameraType")
            .and_then(|v| v.as_enum_name().map(str::to_string))
            .unwrap_or_else(|| "Custom".to_string());
        let cf = g.get(cam).and_then(|i| i.cframe());
        let fov = g.get_prop(cam, "FieldOfView").and_then(|v| v.as_number()).unwrap_or(70.0) as f32;
        let ortho = g.get_prop(cam, "Projection").and_then(|v| v.as_enum_name().map(|s| s == "Orthographic")).unwrap_or(false);
        let size = g.get_prop(cam, "OrthographicSize").and_then(|v| v.as_number()).unwrap_or(40.0) as f32;
        (camera_type, cf, fov, ortho, size)
    };

    // A replica with no avatar shows the Space's camera whatever its type,
    // as the Space's own view. Everywhere else a script owns the camera only
    // while it is Scriptable.
    let space_view = role.as_deref().copied().unwrap_or_default() == PlayRole::Replica && avatar_cam.is_none();
    if camera_type != "Scriptable" && !space_view {
        if scripted {
            commands.entity(entity).remove::<AvatarCameraScripted>();
            *projection = Projection::Perspective(PerspectiveProjection {
                fov: AVATAR_FOV_DEG.to_radians(),
                ..default()
            });
        }
        return;
    }
    if !scripted && !space_view {
        commands.entity(entity).insert(AvatarCameraScripted);
    }
    let Some(cf) = cf else { return };
    let t = cf.to_transform();
    tf.translation = t.translation;
    tf.rotation = t.rotation;

    // Keep WASD relative to the screen: forward is the view direction on
    // the ground, or the screen's up for a straight-down camera.
    let look = cf.look_vector();
    let up = cf.up_vector();
    let (fx, fz) = if look.y.abs() < 0.98 { (look.x, look.z) } else { (up.x, up.z) };
    if fx.abs() + fz.abs() > 1e-6 {
        if let Some(avatar_cam) = avatar_cam.as_mut() {
            avatar_cam.yaw = (-(fx as f32)).atan2(-(fz as f32));
        }
    }

    if ortho {
        let want = ortho_size.max(0.1);
        let needs = match &*projection {
            Projection::Orthographic(o) => {
                !matches!(o.scaling_mode, ScalingMode::FixedVertical { viewport_height } if (viewport_height - want).abs() < 1e-4)
            }
            _ => true,
        };
        if needs {
            let mut o = OrthographicProjection::default_3d();
            o.scaling_mode = ScalingMode::FixedVertical { viewport_height: want };
            o.near = -500.0;
            o.far = 5000.0;
            *projection = Projection::Orthographic(o);
        }
    } else {
        let want = fov.clamp(1.0, 120.0).to_radians();
        let needs = match &*projection {
            Projection::Perspective(p) => (p.fov - want).abs() > 1e-4,
            _ => true,
        };
        if needs {
            *projection = Projection::Perspective(PerspectiveProjection { fov: want, ..default() });
        }
    }
}
