//! # This machine's input, into a Play tree
//!
//! Keys, buttons, cursor, wheel and motion into `dm.input`, the camera that
//! is rendering into `workspace.CurrentCamera`, and the cursor's ray into
//! `dm.mouse`: the per-machine half of a Play frame's pull, written the same
//! way on both apps. Studio calls these from its pull with its 3D viewport's
//! sub-rect and what its panels took this frame; the Player calls them with
//! the whole window. Each app keeps what is its own: the clock, Studio's
//! injected input, and which camera is looking.
//!
//! Key and button names are Roblox's, from [`bevy_keycode_to_roblox`] and
//! [`bevy_mouse_to_roblox`], the table Studio's injected input reads back
//! through `roblox_to_bevy_keycode`.

use std::collections::BTreeSet;

use bevy::prelude::*;

use crate::datamodel::record::world_cframe;
use crate::datamodel::{DataModel, DmValue, EnumItem, InputEvent, InputPhase};
use crate::luau::{bevy_keycode_to_roblox, bevy_mouse_to_roblox};
use crate::play_session::ViewportBounds;
use crate::scripting::Vector2;

#[cfg(feature = "physics")]
use crate::classes::BasePart;
#[cfg(feature = "physics")]
use crate::datamodel::SharedDataModel;
#[cfg(feature = "physics")]
use crate::scripting::Vector3;
#[cfg(feature = "physics")]
use crate::terrain::TerrainChunkCollider;
#[cfg(feature = "physics")]
use avian3d::prelude::{Sensor, SpatialQuery, SpatialQueryFilter};

/// The part of the window the 3D view fills, in window logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewRect {
    pub origin: Vec2,
    pub size: Vec2,
}

impl ViewRect {
    /// The whole window, as the Player draws.
    pub fn full(window: &Window) -> Self {
        Self { origin: Vec2::ZERO, size: Vec2::new(window.width(), window.height()) }
    }

    /// Studio's 3D viewport: the sub-rect its layout reports, in physical
    /// pixels, or the whole window until the layout has reported one.
    pub fn of(window: &Window, bounds: Option<&ViewportBounds>) -> Self {
        let scale = window.scale_factor().max(0.0001);
        match bounds {
            Some(b) if b.width > 0.0 && b.height > 0.0 => Self {
                origin: Vec2::new(b.x / scale, b.y / scale),
                size: Vec2::new(b.width / scale, b.height / scale),
            },
            _ => Self::full(window),
        }
    }

    /// A window point, relative to the view's top-left.
    pub fn local(&self, window_point: Vec2) -> Vec2 {
        window_point - self.origin
    }
}

/// What this machine's own interface took this frame. A script still sees
/// the input, marked game-processed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Taken {
    /// The cursor is over an editor panel.
    pub over_panel: bool,
    /// The cursor is over a HUD element that takes the mouse.
    pub over_gui: bool,
    /// A text field holds the keyboard.
    pub typing: bool,
}

/// One frame of this machine's devices.
pub struct DeviceFrame<'a> {
    pub keyboard: Option<&'a ButtonInput<KeyCode>>,
    pub mouse: Option<&'a ButtonInput<MouseButton>>,
    /// The cursor relative to the view's top-left; `None` while it is
    /// outside the window.
    pub cursor: Option<Vec2>,
    /// This frame's mouse motion.
    pub motion: Vec2,
    /// This frame's wheel steps.
    pub wheel: f64,
    /// Cursor travel with no device behind it, from Studio's injected input.
    pub extra_motion: Vec2,
    pub rect: ViewRect,
    pub taken: Taken,
}

/// `dm.input` and this frame's input events, which InputBegan, Button1Down
/// and the rest fire from. Keys held while typing and buttons held over a
/// panel are left out of the held sets; their events still arrive, marked
/// game-processed.
pub fn write_input(g: &mut DataModel, f: &DeviceFrame) {
    let Taken { over_panel, over_gui, typing } = f.taken;
    let (cx, cy) = f.cursor.map(|c| (c.x as f64, c.y as f64)).unwrap_or((0.0, 0.0));
    let delta = f.motion;

    let event = |phase, input_type: &str, key: &str, processed: bool, wheel_steps: f64| InputEvent {
        phase,
        input_type: input_type.to_string(),
        key_code: key.to_string(),
        x: cx,
        y: cy,
        dx: delta.x as f64,
        dy: delta.y as f64,
        wheel: wheel_steps,
        game_processed: processed,
    };
    let mut events: Vec<InputEvent> = Vec::new();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    let mut buttons: BTreeSet<String> = BTreeSet::new();

    if let Some(kb) = f.keyboard {
        if !typing {
            keys.extend(kb.get_pressed().filter_map(|k| bevy_keycode_to_roblox(*k)).map(str::to_string));
        }
        for k in kb.get_just_pressed() {
            if let Some(name) = bevy_keycode_to_roblox(*k) {
                events.push(event(InputPhase::Began, "Keyboard", name, typing, 0.0));
            }
        }
        for k in kb.get_just_released() {
            if let Some(name) = bevy_keycode_to_roblox(*k) {
                events.push(event(InputPhase::Ended, "Keyboard", name, typing, 0.0));
            }
        }
    }
    if let Some(mb) = f.mouse {
        if !over_panel {
            buttons.extend(mb.get_pressed().filter_map(|b| bevy_mouse_to_roblox(*b)).map(str::to_string));
        }
        let processed = over_panel || over_gui;
        for b in mb.get_just_pressed() {
            if let Some(name) = bevy_mouse_to_roblox(*b) {
                events.push(event(InputPhase::Began, name, "Unknown", processed, 0.0));
            }
        }
        for b in mb.get_just_released() {
            if let Some(name) = bevy_mouse_to_roblox(*b) {
                events.push(event(InputPhase::Ended, name, "Unknown", processed, 0.0));
            }
        }
    }
    if delta != Vec2::ZERO && f.cursor.is_some() {
        events.push(event(InputPhase::Changed, "MouseMovement", "Unknown", over_panel, 0.0));
    }
    if f.extra_motion != Vec2::ZERO {
        events.push(InputEvent {
            dx: f.extra_motion.x as f64,
            dy: f.extra_motion.y as f64,
            ..event(InputPhase::Changed, "MouseMovement", "Unknown", false, 0.0)
        });
    }
    if f.wheel != 0.0 {
        events.push(event(InputPhase::Changed, "MouseWheel", "Unknown", over_panel || over_gui, f.wheel.signum()));
    }

    g.input.viewport_w = f.rect.size.x as f64;
    g.input.viewport_h = f.rect.size.y as f64;
    g.input.viewport_focused = !over_panel && !typing;
    g.input.mouse_dx = delta.x as f64;
    g.input.mouse_dy = delta.y as f64;
    if f.cursor.is_some() {
        g.input.mouse_x = cx;
        g.input.mouse_y = cy;
    }
    g.input.wheel = f.wheel;
    g.input.keys = keys;
    g.input.buttons = buttons;
    g.input.events = events;
}

/// The camera state scripts read while they are not scripting it. From the
/// camera that is rendering: `CurrentCamera.CFrame`, and `FieldOfView` and
/// `Projection` from its projection, unless its `CameraType` is
/// `Scriptable`, when the script's own writes stand. `ViewportSize` is the
/// view's size either way.
pub fn write_camera(g: &mut DataModel, pose: &GlobalTransform, projection: &Projection, rect: ViewRect) {
    let Some(cam) = g.current_camera() else { return };
    let scripted = g.get_prop(cam, "CameraType").and_then(|v| v.as_enum_name().map(str::to_string)).as_deref()
        == Some("Scriptable");
    if !scripted {
        g.set_prop_from_engine(cam, "CFrame", DmValue::CFrame(world_cframe(pose)));
        match projection {
            Projection::Perspective(p) => {
                g.set_prop_from_engine(cam, "FieldOfView", DmValue::Number(p.fov.to_degrees() as f64));
                g.set_prop_from_engine(cam, "Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Perspective")));
            }
            Projection::Orthographic(_) => {
                g.set_prop_from_engine(cam, "Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Orthographic")));
            }
            _ => {}
        }
    }
    g.set_prop_from_engine(cam, "ViewportSize", DmValue::Vector2(Vector2::new(rect.size.x as f64, rect.size.y as f64)));
}

/// How far the cursor's ray reaches.
#[cfg(feature = "physics")]
pub const MOUSE_REACH: f32 = 2000.0;

/// What the cursor's ray reads about each collider it meets.
#[cfg(feature = "physics")]
pub type MouseColliders<'w, 's> =
    Query<'w, 's, (Option<&'static BasePart>, Has<Sensor>, Has<TerrainChunkCollider>)>;

/// The ray from `camera` through a view-relative cursor. `None` when there
/// is no cursor, or when the camera cannot make a ray that is all finite
/// numbers (a camera transform holding NaN, a view with no size).
pub fn cursor_ray(camera: &Camera, camera_at: &GlobalTransform, rect: ViewRect, cursor: Option<Vec2>) -> Option<Ray3d> {
    // `viewport_to_world` takes WINDOW logical pixels and subtracts the
    // camera's own viewport origin itself, so a camera drawn into the view's
    // sub-rect and one drawn over the whole window both take the window point.
    let ray = camera.viewport_to_world(camera_at, cursor? + rect.origin).ok()?;
    ray.origin.is_finite().then_some(ray)
}

/// `Mouse.Hit`, `Target` and `UnitRay`: the cursor's ray through `camera`,
/// cast against the physics world. The local avatar never blocks its own
/// cursor, and neither does `Mouse.TargetFilter` or anything under it; a
/// sensor, and a part that is both invisible and not collidable, let the ray
/// through. A terrain chunk has no instance, so a hit on one targets the
/// Terrain. Without a ray, `dm.mouse` keeps the last hit.
#[cfg(feature = "physics")]
#[allow(clippy::too_many_arguments)]
pub fn write_mouse(
    dm: &SharedDataModel,
    camera: &Camera,
    camera_at: &GlobalTransform,
    rect: ViewRect,
    cursor: Option<Vec2>,
    spatial: &SpatialQuery,
    colliders: &MouseColliders,
    local_avatar: impl IntoIterator<Item = Entity>,
) {
    let Some(ray) = cursor_ray(camera, camera_at, rect, cursor) else { return };

    let mut excluded: Vec<Entity> = local_avatar.into_iter().collect();
    {
        let g = dm.lock();
        if let Some(f) = g.mouse.target_filter {
            for id in std::iter::once(f).chain(g.descendants(f)) {
                if let Some(e) = g.entity_of(id) {
                    excluded.push(Entity::from_bits(e));
                }
            }
        }
    }
    // One call: `with_excluded_entities` replaces the set rather than adding.
    let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);
    let hit = spatial.cast_ray_predicate(ray.origin, ray.direction, MOUSE_REACH, true, &filter, &|e| {
        colliders.get(e).map_or(true, |(bp, sensor, _)| !sensor && bp.map_or(true, |b| b.transparency < 1.0 || b.can_collide))
    });
    // A hit that is not all finite numbers is no hit to report: keep the last.
    if hit.as_ref().is_some_and(|h| !h.distance.is_finite() || !h.normal.is_finite()) {
        return;
    }

    let mut g = dm.lock();
    g.mouse.ray_origin = Vector3::from_vec3(ray.origin);
    g.mouse.ray_direction = Vector3::from_vec3(*ray.direction);
    match hit {
        Some(h) => {
            g.mouse.has_hit = true;
            g.mouse.hit_position = Vector3::from_vec3(ray.origin + *ray.direction * h.distance);
            g.mouse.hit_normal = Vector3::from_vec3(h.normal);
            let on_terrain = colliders.get(h.entity).map_or(false, |(_, _, terrain)| terrain);
            g.mouse.target = match g.by_entity(h.entity.to_bits()) {
                Some(target) => Some(target),
                None if on_terrain => crate::luau::play::terrain::terrain_instance(&g),
                None => None,
            };
        }
        None => {
            g.mouse.has_hit = false;
            g.mouse.target = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luau::{roblox_to_bevy_keycode, roblox_to_bevy_mouse};

    const RECT: ViewRect = ViewRect { origin: Vec2::new(200.0, 100.0), size: Vec2::new(800.0, 500.0) };

    fn frame<'a>(kb: &'a ButtonInput<KeyCode>, mb: &'a ButtonInput<MouseButton>, taken: Taken) -> DeviceFrame<'a> {
        DeviceFrame {
            keyboard: Some(kb),
            mouse: Some(mb),
            cursor: Some(Vec2::new(400.0, 250.0)),
            motion: Vec2::ZERO,
            wheel: 0.0,
            extra_motion: Vec2::ZERO,
            rect: RECT,
            taken,
        }
    }

    fn began(g: &DataModel, input_type: &str, key: &str) -> Option<bool> {
        g.input
            .events
            .iter()
            .find(|e| e.phase == InputPhase::Began && e.input_type == input_type && e.key_code == key)
            .map(|e| e.game_processed)
    }

    /// The names a script reads are Roblox's, and each maps back to the same
    /// device through the table Studio's injected input presses with.
    #[test]
    fn key_and_button_names_are_roblox_names_both_ways() {
        let keys = [
            (KeyCode::KeyW, "W"),
            (KeyCode::Space, "Space"),
            (KeyCode::ShiftLeft, "LeftShift"),
            (KeyCode::Digit1, "One"),
            (KeyCode::Enter, "Return"),
            (KeyCode::ArrowUp, "Up"),
            (KeyCode::F5, "F5"),
            (KeyCode::Backslash, "BackSlash"),
        ];
        for (key, name) in keys {
            let mut kb = ButtonInput::<KeyCode>::default();
            let mb = ButtonInput::<MouseButton>::default();
            kb.press(key);
            let mut g = DataModel::new();
            write_input(&mut g, &frame(&kb, &mb, Taken::default()));
            assert_eq!(g.input.keys, BTreeSet::from([name.to_string()]), "{key:?} is held as {name}");
            assert_eq!(began(&g, "Keyboard", name), Some(false), "{key:?} begins as {name}");
            assert_eq!(roblox_to_bevy_keycode(name), Some(key), "{name} presses {key:?}");
        }
        for (button, name) in
            [(MouseButton::Left, "MouseButton1"), (MouseButton::Right, "MouseButton2"), (MouseButton::Middle, "MouseButton3")]
        {
            let kb = ButtonInput::<KeyCode>::default();
            let mut mb = ButtonInput::<MouseButton>::default();
            mb.press(button);
            let mut g = DataModel::new();
            write_input(&mut g, &frame(&kb, &mb, Taken::default()));
            assert_eq!(g.input.buttons, BTreeSet::from([name.to_string()]));
            assert_eq!(began(&g, name, "Unknown"), Some(false), "{button:?} begins as {name}");
            assert_eq!(roblox_to_bevy_mouse(name), Some(button));
        }
    }

    /// What the machine's own interface took still reaches scripts, marked
    /// game-processed, and is left out of what IsKeyDown and
    /// IsMouseButtonPressed read.
    #[test]
    fn input_the_interface_took_arrives_game_processed() {
        let mut kb = ButtonInput::<KeyCode>::default();
        let mut mb = ButtonInput::<MouseButton>::default();
        kb.press(KeyCode::KeyE);
        mb.press(MouseButton::Left);

        let mut g = DataModel::new();
        write_input(&mut g, &frame(&kb, &mb, Taken { typing: true, ..default() }));
        assert!(g.input.keys.is_empty(), "a key typed into a text field is not held for gameplay");
        assert_eq!(began(&g, "Keyboard", "E"), Some(true));
        assert_eq!(began(&g, "MouseButton1", "Unknown"), Some(false), "typing leaves the mouse alone");
        assert!(!g.input.viewport_focused);

        let mut g = DataModel::new();
        write_input(&mut g, &frame(&kb, &mb, Taken { over_panel: true, ..default() }));
        assert!(g.input.buttons.is_empty(), "a click on a panel is not held for gameplay");
        assert_eq!(began(&g, "MouseButton1", "Unknown"), Some(true));
        assert_eq!(began(&g, "Keyboard", "E"), Some(false));

        let mut g = DataModel::new();
        write_input(&mut g, &frame(&kb, &mb, Taken { over_gui: true, ..default() }));
        assert_eq!(g.input.buttons, BTreeSet::from(["MouseButton1".to_string()]), "a HUD click is still held");
        assert_eq!(began(&g, "MouseButton1", "Unknown"), Some(true));
        assert!(g.input.viewport_focused);

        let mut g = DataModel::new();
        write_input(&mut g, &frame(&kb, &mb, Taken::default()));
        assert_eq!(began(&g, "MouseButton1", "Unknown"), Some(false));
        assert_eq!((g.input.viewport_w, g.input.viewport_h), (800.0, 500.0));
        assert_eq!((g.input.mouse_x, g.input.mouse_y), (400.0, 250.0), "the cursor is view-relative");
    }

    /// `CurrentCamera` follows the rendering camera, until a script takes it.
    #[test]
    fn the_current_camera_follows_the_view_until_a_script_takes_it() {
        use bevy::camera::PerspectiveProjection;
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let cam = g.create("Camera");
        g.set_parent(cam, Some(ws)).unwrap();
        g.set_prop(ws, "CurrentCamera", DmValue::Instance(cam)).unwrap();
        assert_eq!(g.current_camera(), Some(cam));
        let position = |g: &DataModel| match g.get_prop(cam, "CFrame") {
            Some(DmValue::CFrame(cf)) => cf.position.to_vec3(),
            other => panic!("CFrame is {other:?}"),
        };
        let viewport = |g: &DataModel| match g.get_prop(cam, "ViewportSize") {
            Some(DmValue::Vector2(v)) => (v.x, v.y),
            other => panic!("ViewportSize is {other:?}"),
        };
        let lens = Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), ..default() });

        write_camera(&mut g, &GlobalTransform::from_xyz(1.0, 2.0, 3.0), &lens, RECT);
        assert_eq!(position(&g), Vec3::new(1.0, 2.0, 3.0));
        let fov = g.get_prop(cam, "FieldOfView").and_then(|v| v.as_number()).unwrap();
        assert!((fov - 70.0).abs() < 1e-3, "FieldOfView is in degrees: {fov}");
        assert_eq!(viewport(&g), (800.0, 500.0));

        g.set_prop(cam, "CameraType", DmValue::Enum(EnumItem::new("CameraType", "Scriptable"))).unwrap();
        g.set_prop(cam, "CFrame", DmValue::CFrame(world_cframe(&GlobalTransform::from_xyz(9.0, 9.0, 9.0)))).unwrap();
        let wide = ViewRect { origin: Vec2::ZERO, size: Vec2::new(1280.0, 720.0) };
        write_camera(&mut g, &GlobalTransform::from_xyz(1.0, 2.0, 3.0), &lens, wide);
        assert_eq!(position(&g), Vec3::new(9.0, 9.0, 9.0), "a Scriptable camera keeps the script's CFrame");
        assert_eq!(viewport(&g), (1280.0, 720.0), "ViewportSize follows the view either way");
    }

    /// A camera drawn into `rect` of a `window`-sized target, at the origin
    /// looking down -Z, with a 45 degree vertical field of view.
    fn camera_in(rect: ViewRect, window: UVec2) -> Camera {
        use bevy::camera::{CameraProjection, ComputedCameraValues, PerspectiveProjection, RenderTargetInfo, Viewport};
        let projection = PerspectiveProjection {
            fov: 45f32.to_radians(),
            aspect_ratio: rect.size.x / rect.size.y,
            ..default()
        };
        Camera {
            viewport: Some(Viewport {
                physical_position: rect.origin.as_uvec2(),
                physical_size: rect.size.as_uvec2(),
                ..default()
            }),
            computed: ComputedCameraValues {
                clip_from_view: projection.get_clip_from_view(),
                target_info: Some(RenderTargetInfo { physical_size: window, scale_factor: 1.0 }),
                ..default()
            },
            ..default()
        }
    }

    #[test]
    fn the_cursor_ray_leaves_through_the_view_rect_not_the_window() {
        let camera = camera_in(RECT, UVec2::new(1280, 720));
        let centre = RECT.size / 2.0;
        let ray = cursor_ray(&camera, &GlobalTransform::IDENTITY, RECT, Some(centre)).expect("a ray");
        assert!(ray.direction.dot(Vec3::NEG_Z) > 0.9999, "the view's centre looks straight ahead: {:?}", ray.direction);

        // Read as a window point, the same cursor would be well off-centre.
        let off = cursor_ray(&camera, &GlobalTransform::IDENTITY, ViewRect { origin: Vec2::ZERO, ..RECT }, Some(centre))
            .expect("a ray");
        assert!(off.direction.dot(Vec3::NEG_Z) < 0.99, "the rect's origin moves the ray");

        assert!(cursor_ray(&camera, &GlobalTransform::IDENTITY, RECT, None).is_none(), "no cursor, no ray");
        let broken = GlobalTransform::from_translation(Vec3::NAN);
        assert!(cursor_ray(&camera, &broken, RECT, Some(centre)).is_none(), "a NaN camera makes no ray");
    }

    #[cfg(feature = "physics")]
    mod hits {
        use super::*;
        use avian3d::prelude::*;
        use bevy::ecs::system::RunSystemOnce;

        /// Just enough of an app for Avian to answer a ray: the same pieces
        /// the avatar tests add one by one, for the same reasons.
        fn physics_app() -> App {
            let mut app = App::new();
            bevy::tasks::IoTaskPool::get_or_init(Default::default);
            bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
            bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
            app.add_plugins((
                bevy::time::TimePlugin,
                bevy::transform::TransformPlugin,
                bevy::asset::AssetPlugin::default(),
                bevy::diagnostic::DiagnosticsPlugin,
            ));
            app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
            app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
            app.init_resource::<avian3d::collision::CollisionDiagnostics>();
            app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
            app.init_asset::<Mesh>();
            app.init_asset::<WorldAsset>();
            app.insert_resource(Time::<Fixed>::from_hz(60.0));
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(
                1.0 / 60.0,
            )));
            app.add_plugins(PhysicsPlugins::default());
            app
        }

        /// A 4 m square wall, 1 m thick, whose near face is 9.5 m ahead.
        fn wall(app: &mut App) -> Entity {
            app.world_mut()
                .spawn((Transform::from_xyz(0.0, 0.0, -10.0), Collider::cuboid(4.0, 4.0, 1.0), RigidBody::Static))
                .id()
        }

        fn cast(app: &mut App, dm: &SharedDataModel, camera_at: GlobalTransform, cursor: Vec2) {
            let dm = dm.clone();
            app.world_mut()
                .run_system_once(move |spatial: SpatialQuery, colliders: MouseColliders| {
                    let camera = camera_in(RECT, UVec2::new(1280, 720));
                    write_mouse(&dm, &camera, &camera_at, RECT, Some(cursor), &spatial, &colliders, []);
                })
                .expect("the cast runs");
        }

        #[test]
        fn a_click_at_the_views_centre_hits_the_wall_ahead() {
            let mut app = physics_app();
            let wall = wall(&mut app);
            for _ in 0..3 {
                app.update();
            }
            let mut g = DataModel::new();
            let part = g.create("Part");
            g.bind_entity(part, wall.to_bits());
            let dm: SharedDataModel = std::sync::Arc::new(parking_lot::Mutex::new(g));

            cast(&mut app, &dm, GlobalTransform::IDENTITY, RECT.size / 2.0);
            let g = dm.lock();
            assert!(g.mouse.has_hit, "the ray through the view's centre meets the wall");
            let at = g.mouse.hit_position.to_vec3();
            assert!(at.distance(Vec3::new(0.0, 0.0, -9.5)) < 1e-3, "Mouse.Hit is on the near face: {at}");
            assert!(g.mouse.hit_normal.to_vec3().dot(Vec3::Z) > 0.999);
            assert_eq!(g.mouse.target, Some(part), "Mouse.Target is the wall's part");
            drop(g);

            // Near the view's corner the ray passes beside the wall.
            cast(&mut app, &dm, GlobalTransform::IDENTITY, Vec2::new(10.0, 10.0));
            let g = dm.lock();
            assert!(!g.mouse.has_hit);
            assert_eq!(g.mouse.target, None);
        }

        #[test]
        fn a_camera_that_cannot_make_a_ray_keeps_the_last_hit() {
            let mut app = physics_app();
            wall(&mut app);
            for _ in 0..3 {
                app.update();
            }
            let mut g = DataModel::new();
            g.mouse.has_hit = true;
            g.mouse.hit_position = Vector3::from_vec3(Vec3::new(1.0, 2.0, 3.0));
            let dm: SharedDataModel = std::sync::Arc::new(parking_lot::Mutex::new(g));

            cast(&mut app, &dm, GlobalTransform::from_translation(Vec3::NAN), RECT.size / 2.0);
            let g = dm.lock();
            assert!(g.mouse.has_hit);
            assert_eq!(g.mouse.hit_position.to_vec3(), Vec3::new(1.0, 2.0, 3.0), "no NaN reaches Mouse.Hit");
            assert!(g.mouse.ray_origin.to_vec3().is_finite());
        }
    }
}
