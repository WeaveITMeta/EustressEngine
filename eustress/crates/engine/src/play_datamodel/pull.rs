//! ECS -> DataModel, once per frame before scripts run. Nothing here marks
//! the tree dirty, so none of it is written back.

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use avian3d::prelude::{CollisionEnd, CollisionStart, LinearVelocity, Sensor, SpatialQuery, SpatialQueryFilter};

use eustress_common::avatar::control::AvatarCamera;
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent};
use eustress_common::avatar::LocalAvatar;
use eustress_common::classes::BasePart;
use eustress_common::datamodel::{DmEvent, DmValue, EnumItem, InputEvent, InputPhase, InstanceId};
use eustress_common::luau::play::{RayHit, RayQuery};
use eustress_common::luau::{bevy_keycode_to_roblox, bevy_mouse_to_roblox};
use eustress_common::scripting::{CFrame, Vector2, Vector3};

use super::seed::world_cframe;
use super::PlayDataModel;

/// Where the cursor is, in the terms the camera needs.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct MouseRayState {
    /// Cursor relative to the 3D viewport's top-left, logical pixels.
    pub cursor: Option<Vec2>,
    /// The 3D viewport's top-left in window logical pixels.
    pub viewport_origin: Vec2,
    /// The 3D viewport's size in logical pixels.
    pub viewport_size: Vec2,
}

/// Input a bridge client injects to play-test a game (`input.inject`).
/// Keys and mouse buttons press and release Bevy's own inputs (so the
/// avatar walks on an injected W exactly as on a real one); the cursor,
/// its movement and the wheel join what scripts see. The real mouse never
/// moves. Names are Roblox's: key codes (`W`, `One`, `LeftShift`, `Space`)
/// and `MouseButton1`..`MouseButton3`.
#[derive(Resource, Default, Debug, Clone)]
pub struct InjectedInput {
    /// Cursor in viewport logical pixels; replaces the OS cursor while set.
    pub cursor: Option<Vec2>,
    /// Keys and buttons held down by injection.
    pub held: std::collections::BTreeSet<String>,
    /// Presses and releases for the next frame's input.
    pub press: Vec<String>,
    pub release: Vec<String>,
    /// Taps pressed this frame, released on the next.
    pub tap: Vec<String>,
    tap_due: Vec<String>,
    pub wheel: f64,
    /// Cursor travel since the last frame.
    pub moved: Vec2,
}

fn input_name(v: &serde_json::Value) -> Option<String> {
    // `Enum.KeyCode.W`, `KeyCode.W` and `W` all name the W key.
    v.as_str().map(|s| s.rsplit('.').next().unwrap_or(s).to_string()).filter(|s| !s.is_empty())
}

/// Apply one `input.inject` request. Every field is optional:
/// `cursor: [x, y] | null` (viewport logical pixels), `down` / `up` / `tap`
/// (arrays of names), `wheel` (steps, positive = away from the user) and
/// `clear: true` (release everything and hand the cursor back to the OS).
pub fn apply_injection(inj: &mut InjectedInput, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    if params.get("clear").and_then(|v| v.as_bool()) == Some(true) {
        let held: Vec<String> = std::mem::take(&mut inj.held).into_iter().collect();
        inj.release.extend(held);
        inj.press.clear();
        inj.tap.clear();
        inj.cursor = None;
    }
    if let Some(c) = params.get("cursor") {
        if c.is_null() {
            inj.cursor = None;
        } else {
            let (Some(x), Some(y)) = (c.get(0).and_then(|v| v.as_f64()), c.get(1).and_then(|v| v.as_f64())) else {
                return Err("`cursor` must be [x, y] in viewport pixels, or null".into());
            };
            if !x.is_finite() || !y.is_finite() {
                return Err("`cursor` must be finite".into());
            }
            let next = Vec2::new(x as f32, y as f32);
            if let Some(prev) = inj.cursor {
                inj.moved += next - prev;
            }
            inj.cursor = Some(next);
        }
    }
    let names = |key: &str| -> Result<Vec<String>, String> {
        match params.get(key) {
            None => Ok(Vec::new()),
            Some(serde_json::Value::Array(list)) => list
                .iter()
                .map(|n| input_name(n).ok_or_else(|| format!("`{key}` holds a non-string name")))
                .collect(),
            Some(_) => Err(format!("`{key}` must be an array of key or button names")),
        }
    };
    let known = |n: &str| {
        eustress_common::luau::roblox_to_bevy_keycode(n).is_some() || eustress_common::luau::roblox_to_bevy_mouse(n).is_some()
    };
    for key in ["down", "up", "tap"] {
        if let Some(bad) = names(key)?.into_iter().find(|n| !known(n)) {
            return Err(format!("unknown key or button '{bad}' (Roblox names: W, One, LeftShift, Space, MouseButton1)"));
        }
    }
    for n in names("down")? {
        if inj.held.insert(n.clone()) {
            inj.press.push(n);
        }
    }
    for n in names("up")? {
        if inj.held.remove(&n) {
            inj.release.push(n);
        }
    }
    for n in names("tap")? {
        inj.press.push(n.clone());
        inj.tap.push(n);
    }
    if let Some(w) = params.get("wheel").and_then(|v| v.as_f64()) {
        inj.wheel += w;
    }
    Ok(serde_json::json!({
        "cursor": inj.cursor.map(|c| [c.x, c.y]),
        "held": inj.held.iter().collect::<Vec<_>>(),
    }))
}

/// PreUpdate, after Bevy reads the devices: injected presses and releases
/// land in the same `ButtonInput`s the real keyboard and mouse fill, so the
/// avatar and scripts cannot tell them apart. A tap is down for one frame.
pub fn apply_injected_buttons(
    mut injected: ResMut<InjectedInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    if injected.press.is_empty() && injected.release.is_empty() && injected.tap.is_empty() && injected.tap_due.is_empty() {
        return;
    }
    let due = std::mem::take(&mut injected.tap_due);
    let press = std::mem::take(&mut injected.press);
    let release = std::mem::take(&mut injected.release);
    for n in &press {
        if let Some(k) = eustress_common::luau::roblox_to_bevy_keycode(n) {
            keys.press(k);
        } else if let Some(b) = eustress_common::luau::roblox_to_bevy_mouse(n) {
            buttons.press(b);
        }
    }
    for n in release.iter().chain(due.iter()) {
        if let Some(k) = eustress_common::luau::roblox_to_bevy_keycode(n) {
            keys.release(k);
        } else if let Some(b) = eustress_common::luau::roblox_to_bevy_mouse(n) {
            buttons.release(b);
        }
    }
    injected.tap_due = std::mem::take(&mut injected.tap);
}

/// The visible 3D viewport in window logical pixels.
fn viewport_rect(window: &Window, bounds: Option<&crate::ui::ViewportBounds>) -> (Vec2, Vec2) {
    let scale = window.scale_factor().max(0.0001);
    match bounds {
        Some(b) if b.width > 0.0 && b.height > 0.0 => (
            Vec2::new(b.x / scale, b.y / scale),
            Vec2::new(b.width / scale, b.height / scale),
        ),
        _ => (Vec2::ZERO, Vec2::new(window.width(), window.height())),
    }
}

/// Time, input, and the viewport.
#[allow(clippy::too_many_arguments)]
pub fn pull_frame_state(
    dm: Option<Res<PlayDataModel>>,
    time: Res<Time>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut wheel: MessageReader<MouseWheel>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bounds: Option<Res<crate::ui::ViewportBounds>>,
    focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut ray_state: ResMut<MouseRayState>,
    mut injected: ResMut<InjectedInput>,
) {
    let Some(dm) = dm else {
        wheel.clear();
        return;
    };
    let window = windows.single().ok();
    let (origin, size) = window.map(|w| viewport_rect(w, bounds.as_deref())).unwrap_or((Vec2::ZERO, Vec2::new(1280.0, 720.0)));
    // An injected cursor stands in for the OS one while it is set.
    let injecting = injected.cursor.is_some();
    let cursor = injected.cursor.or_else(|| window.and_then(|w| w.cursor_position()).map(|c| c - origin));
    *ray_state = MouseRayState { cursor, viewport_origin: origin, viewport_size: size };

    // Where the real mouse is says nothing about an injected cursor.
    let over_panel = !injecting && focus.as_deref().map_or(false, |f| f.has_focus);
    let over_gui = !injecting && focus.as_deref().map_or(false, |f| f.gui_element_hit);
    let typing = focus.as_deref().map_or(false, |f| f.text_input_focused);
    let (cx, cy) = cursor.map(|c| (c.x as f64, c.y as f64)).unwrap_or((0.0, 0.0));
    let delta = motion.as_deref().map(|m| m.delta).unwrap_or(Vec2::ZERO);

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
    let mut keys_down: Vec<String> = Vec::new();
    let mut buttons_down: Vec<String> = Vec::new();

    if let Some(kb) = keyboard.as_deref() {
        if !typing {
            for k in kb.get_pressed() {
                if let Some(name) = bevy_keycode_to_roblox(*k) {
                    keys_down.push(name.to_string());
                }
            }
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
    if let Some(mb) = mouse.as_deref() {
        if !over_panel {
            for b in mb.get_pressed() {
                if let Some(name) = bevy_mouse_to_roblox(*b) {
                    buttons_down.push(name.to_string());
                }
            }
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
    if delta != Vec2::ZERO && cursor.is_some() {
        events.push(event(InputPhase::Changed, "MouseMovement", "Unknown", over_panel, 0.0));
    }

    // Injected cursor travel (injected keys and buttons already arrived
    // through the ButtonInputs above).
    if injected.moved != Vec2::ZERO {
        let moved = std::mem::take(&mut injected.moved);
        events.push(InputEvent {
            phase: InputPhase::Changed,
            input_type: "MouseMovement".into(),
            key_code: "Unknown".into(),
            x: cx,
            y: cy,
            dx: moved.x as f64,
            dy: moved.y as f64,
            wheel: 0.0,
            game_processed: false,
        });
    }

    let mut steps = std::mem::take(&mut injected.wheel);
    for w in wheel.read() {
        steps += w.y as f64;
    }
    if steps != 0.0 {
        events.push(event(InputPhase::Changed, "MouseWheel", "Unknown", over_panel || over_gui, steps.signum()));
    }

    let mut g = dm.dm.lock();
    g.frame.dt = time.delta_secs_f64();
    g.frame.time += g.frame.dt;
    g.frame.frame += 1;
    g.input.viewport_w = size.x as f64;
    g.input.viewport_h = size.y as f64;
    g.input.viewport_focused = !over_panel && !typing;
    g.input.mouse_dx = delta.x as f64;
    g.input.mouse_dy = delta.y as f64;
    if cursor.is_some() {
        g.input.mouse_x = cx;
        g.input.mouse_y = cy;
    }
    g.input.wheel = steps;
    g.input.keys = keys_down.into_iter().collect();
    g.input.buttons = buttons_down.into_iter().collect();
    g.input.events = events;
}

/// Poses that physics (or anything else) changed since last frame.
pub fn pull_poses(
    dm: Option<Res<PlayDataModel>>,
    moved: Query<(Entity, &Transform, Option<&ChildOf>, Option<&LinearVelocity>), (Changed<Transform>, With<BasePart>)>,
    globals: Query<&GlobalTransform>,
) {
    let Some(dm) = dm else { return };
    if moved.is_empty() {
        return;
    }
    let mut g = dm.dm.lock();
    for (e, t, parent, vel) in moved.iter() {
        let Some(id) = g.by_entity(e.to_bits()) else { continue };
        let world = match parent.and_then(|p| globals.get(p.parent()).ok()) {
            Some(pg) => pg.mul_transform(*t),
            None => GlobalTransform::from(*t),
        };
        g.set_prop_from_engine(id, "CFrame", DmValue::CFrame(world_cframe(&world)));
        if let Some(v) = vel {
            g.set_prop_from_engine(id, "AssemblyLinearVelocity", DmValue::Vector3(Vector3::from_vec3(v.0)));
        }
    }
}

/// Avian contacts -> `Touched` / `TouchEnded` on both parts.
pub fn pull_collisions(
    dm: Option<Res<PlayDataModel>>,
    mut started: MessageReader<CollisionStart>,
    mut ended: MessageReader<CollisionEnd>,
) {
    let Some(dm) = dm else {
        started.clear();
        ended.clear();
        return;
    };
    let mut g = dm.dm.lock();
    let resolve = |g: &eustress_common::datamodel::DataModel, collider: Entity, body: Option<Entity>| -> Option<InstanceId> {
        g.by_entity(collider.to_bits()).or_else(|| body.and_then(|b| g.by_entity(b.to_bits())))
    };
    for ev in started.read() {
        let (Some(a), Some(b)) = (resolve(&g, ev.collider1, ev.body1), resolve(&g, ev.collider2, ev.body2)) else { continue };
        let touch_a = g.get_prop(a, "CanTouch").and_then(|v| v.as_bool()).unwrap_or(true);
        let touch_b = g.get_prop(b, "CanTouch").and_then(|v| v.as_bool()).unwrap_or(true);
        if touch_a && touch_b {
            g.push_event(DmEvent::Touched { part: a, other: b });
            g.push_event(DmEvent::Touched { part: b, other: a });
        }
    }
    for ev in ended.read() {
        let (Some(a), Some(b)) = (resolve(&g, ev.collider1, ev.body1), resolve(&g, ev.collider2, ev.body2)) else { continue };
        g.push_event(DmEvent::TouchEnded { part: a, other: b });
        g.push_event(DmEvent::TouchEnded { part: b, other: a });
    }
}

/// The local avatar as the player's `Character`: a Model in Workspace with a
/// `HumanoidRootPart` bound to the avatar body, a `Head`, and a `Humanoid`.
pub fn pull_character(
    dm: Option<Res<PlayDataModel>>,
    avatars: Query<
        (Entity, &Transform, &AvatarBody, &AvatarIntent, Option<&eustress_common::avatar::abilities::AvatarAbilities>),
        With<LocalAvatar>,
    >,
    mut bound: Local<Option<(Entity, InstanceId)>>,
    mut session: Local<usize>,
) {
    let Some(dm) = dm else {
        *bound = None;
        return;
    };
    // This system only runs while Playing, so its locals outlive a session.
    // The avatar entity survives Stop, so without this the next session's
    // tree would inherit a character id that belongs to the previous one.
    let tree = std::sync::Arc::as_ptr(&dm.dm) as usize;
    if *session != tree {
        *session = tree;
        *bound = None;
    }
    let mut g = dm.dm.lock();
    let Some(player) = g.local_player else { return };
    // Keep the body the character is bound to for as long as it exists. With
    // more than one local avatar alive, `iter().next()` alone alternated
    // between them and rebuilt the character on every switch.
    let avatar = bound
        .and_then(|(body, _)| avatars.get(body).ok())
        .or_else(|| avatars.iter().next());

    // The avatar went away (respawn or despawn): retire the character.
    if let Some((body, model)) = *bound {
        if avatar.map(|(e, ..)| e) != Some(body) {
            if let Some(root) = g.find_first_child(model, "HumanoidRootPart", false) {
                g.unbind_entity(root);
            }
            g.push_event(DmEvent::CharacterRemoving { player, character: model });
            g.destroy(model);
            let _ = g.set_prop(player, "Character", DmValue::Nil);
            let _ = g.take_dirty_of(player);
            *bound = None;
        }
    }
    let Some((body, tf, avatar_body, intent, abilities)) = avatar else { return };
    // The character spawns once the player has joined, so `CharacterAdded`
    // reaches the handlers `PlayerAdded` connected.
    if bound.is_none() && !g.in_tree(player) {
        return;
    }

    let half = avatar_body.metrics.capsule_half_extent();
    let root_cf = {
        let mut cf = CFrame::from_quaternion([tf.rotation.x as f64, tf.rotation.y as f64, tf.rotation.z as f64, tf.rotation.w as f64]);
        cf.position = Vector3::from_vec3(tf.translation);
        cf
    };
    let eye = (avatar_body.metrics.eye_height - half) as f64;

    if bound.is_none() {
        let ws = match g.find_service("Workspace") {
            Some(ws) => ws,
            None => return,
        };
        let name = g.name_of(player).unwrap_or("Player").to_string();
        let model = g.create_virtual("Model", &name, Some(ws));
        let height = (half * 2.0) as f64;
        let root = g.create_bound(
            "Part",
            "HumanoidRootPart",
            body.to_bits(),
            Some(model),
            vec![
                ("CFrame".into(), DmValue::CFrame(root_cf)),
                ("Size".into(), DmValue::Vector3(Vector3::new(0.6, height, 0.4))),
                ("Transparency".into(), DmValue::Number(1.0)),
                ("CanCollide".into(), DmValue::Bool(true)),
                ("Anchored".into(), DmValue::Bool(false)),
            ],
        );
        let mut head_cf = root_cf;
        head_cf.position = root_cf.position + Vector3::new(0.0, eye, 0.0);
        let head = g.create_virtual("Part", "Head", Some(model));
        let _ = g.set_prop(head, "CFrame", DmValue::CFrame(head_cf));
        let _ = g.set_prop(head, "Size", DmValue::Vector3(Vector3::new(0.3, 0.3, 0.3)));
        let _ = g.set_prop(head, "Transparency", DmValue::Number(1.0));
        let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(model));
        let _ = g.set_prop(humanoid, "WalkSpeed", DmValue::Number(avatar_body.motion.walk_speed as f64));
        let _ = g.set_prop(humanoid, "JumpHeight", DmValue::Number(avatar_body.motion.jump_apex_m as f64));
        // The movement verbs this character starts with (the Space's
        // StarterPlayer switches), so a script reads what is actually in force.
        let abilities = abilities.copied().unwrap_or_default();
        for name in eustress_common::avatar::abilities::AvatarAbilities::PROPERTIES {
            let _ = g.set_prop(humanoid, name, DmValue::Bool(abilities.get(name).unwrap_or(true)));
        }
        let _ = g.set_prop(model, "PrimaryPart", DmValue::Instance(root));
        let _ = g.set_prop(player, "Character", DmValue::Instance(model));
        // Setup writes are not script writes: nothing to apply.
        let _ = g.take_dirty_of(model);
        let _ = g.take_dirty_of(head);
        let _ = g.take_dirty_of(humanoid);
        let _ = g.take_dirty_of(player);
        g.push_event(DmEvent::CharacterAdded { player, character: model });
        *bound = Some((body, model));
        info!("🧍 Character bound to the local avatar ({:?})", body);
    }

    let Some((_, model)) = *bound else { return };
    if let Some(root) = g.find_first_child(model, "HumanoidRootPart", false) {
        g.set_prop_from_engine(root, "CFrame", DmValue::CFrame(root_cf));
    }
    if let Some(head) = g.find_first_child(model, "Head", false) {
        let mut head_cf = root_cf;
        head_cf.position = root_cf.position + Vector3::new(0.0, eye, 0.0);
        g.set_prop_from_engine(head, "CFrame", DmValue::CFrame(head_cf));
    }
    if let Some(h) = g.find_first_child_of_class(model, "Humanoid", false) {
        let d = intent.direction;
        g.set_prop_from_engine(h, "MoveDirection", DmValue::Vector3(Vector3::new(d.x as f64, 0.0, d.z as f64)));
    }
}

/// ScreenGui button clicks -> `MouseButton1Click` / `Activated`.
pub fn pull_gui_clicks(dm: Option<Res<PlayDataModel>>, focus: Option<Res<crate::ui::SlintUIFocus>>) {
    let (Some(dm), Some(focus)) = (dm, focus) else { return };
    let Some(entity) = focus.gui_clicked_entity else { return };
    let mut g = dm.dm.lock();
    if let Some(button) = g.by_entity(entity.to_bits()) {
        g.push_event(DmEvent::GuiActivated { button });
    }
}

/// The cursor's ray through the play camera, `Mouse.Hit` / `Mouse.Target`,
/// and the camera state scripts read while they are not scripting it.
#[allow(clippy::too_many_arguments)]
pub fn pull_mouse_hit(
    dm: Option<Res<PlayDataModel>>,
    ray_state: Res<MouseRayState>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection), With<AvatarCamera>>,
    editor_cameras: Query<(&Camera, &GlobalTransform, &Projection), (With<crate::camera_controller::EustressCamera>, Without<AvatarCamera>)>,
    spatial: SpatialQuery,
    colliders: Query<(Option<&BasePart>, Has<Sensor>)>,
    avatars: Query<Entity, With<LocalAvatar>>,
) {
    let Some(dm) = dm else { return };
    let cam = cameras
        .iter()
        .find(|(c, ..)| c.is_active)
        .or_else(|| editor_cameras.iter().find(|(c, ..)| c.is_active));
    let Some((camera, cam_gt, projection)) = cam else { return };

    // Camera state for scripts (the scripted-camera system overrides it when
    // CameraType is Scriptable).
    {
        let mut g = dm.dm.lock();
        if let Some(cam_id) = g.current_camera() {
            let scripted = g.get_prop(cam_id, "CameraType").and_then(|v| v.as_enum_name().map(str::to_string))
                == Some("Scriptable".to_string());
            if !scripted {
                g.set_prop_from_engine(cam_id, "CFrame", DmValue::CFrame(world_cframe(cam_gt)));
                match projection {
                    Projection::Perspective(p) => {
                        g.set_prop_from_engine(cam_id, "FieldOfView", DmValue::Number(p.fov.to_degrees() as f64));
                        g.set_prop_from_engine(cam_id, "Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Perspective")));
                    }
                    Projection::Orthographic(_) => {
                        g.set_prop_from_engine(cam_id, "Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Orthographic")));
                    }
                    _ => {}
                }
            }
            let (w, h) = (g.input.viewport_w, g.input.viewport_h);
            g.set_prop_from_engine(cam_id, "ViewportSize", DmValue::Vector2(Vector2::new(w, h)));
        }
    }

    let Some(cursor) = ray_state.cursor else { return };
    // `viewport_to_world` takes WINDOW logical pixels and subtracts the
    // camera's own viewport origin itself, so both the play camera (drawn
    // into the viewport rect) and a full-window camera get the window point.
    let point = cursor + ray_state.viewport_origin;
    let Ok(ray) = camera.viewport_to_world(cam_gt, point) else { return };

    // The local character never blocks its own cursor (Roblox ignores it for
    // Mouse.Hit on the local client), and neither does Mouse.TargetFilter.
    let mut excluded: Vec<Entity> = avatars.iter().collect();
    {
        let g = dm.dm.lock();
        if let Some(f) = g.mouse.target_filter {
            if let Some(e) = g.entity_of(f) {
                excluded.push(Entity::from_bits(e));
            }
            for d in g.descendants(f) {
                if let Some(e) = g.entity_of(d) {
                    excluded.push(Entity::from_bits(e));
                }
            }
        }
    }
    // One call: `with_excluded_entities` replaces the set rather than adding.
    let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);
    let hit = spatial.cast_ray_predicate(ray.origin, ray.direction, 2000.0, true, &filter, &|e| {
        colliders.get(e).map_or(true, |(bp, sensor)| !sensor && bp.map_or(true, |b| b.transparency < 1.0 || b.can_collide))
    });

    let mut g = dm.dm.lock();
    g.mouse.ray_origin = Vector3::from_vec3(ray.origin);
    g.mouse.ray_direction = Vector3::from_vec3(*ray.direction);
    match hit {
        Some(h) => {
            let p = ray.origin + *ray.direction * h.distance;
            g.mouse.has_hit = true;
            g.mouse.hit_position = Vector3::from_vec3(p);
            g.mouse.hit_normal = Vector3::from_vec3(h.normal);
            g.mouse.target = g.by_entity(h.entity.to_bits());
        }
        None => {
            g.mouse.has_hit = false;
            g.mouse.target = None;
        }
    }
}

/// `workspace:Raycast` against Avian, synchronously.
pub fn cast_ray(
    spatial: &SpatialQuery,
    colliders: &Query<(Option<&BasePart>, Has<Sensor>)>,
    q: &RayQuery,
) -> Option<RayHit> {
    let origin = q.origin.to_vec3();
    let dir = q.direction.to_vec3();
    let len = dir.length();
    if !len.is_finite() || len < 1e-6 {
        return None;
    }
    let direction = Dir3::new(dir / len).ok()?;
    let listed: std::collections::HashSet<u64> = q.filter.iter().copied().collect();
    let mut filter = SpatialQueryFilter::default();
    if !q.include && !listed.is_empty() {
        filter = filter.with_excluded_entities(listed.iter().map(|b| Entity::from_bits(*b)));
    }
    let hit = spatial.cast_ray_predicate(origin, direction, len, true, &filter, &|e| {
        if q.include && !listed.contains(&e.to_bits()) {
            return false;
        }
        if q.respect_can_collide {
            if let Ok((bp, sensor)) = colliders.get(e) {
                if sensor || bp.map_or(false, |b| !b.can_collide) {
                    return false;
                }
            }
        }
        true
    })?;
    let p = origin + *direction * hit.distance;
    Some(RayHit {
        entity: hit.entity.to_bits(),
        position: Vector3::from_vec3(p),
        normal: Vector3::from_vec3(hit.normal),
        distance: hit.distance as f64,
    })
}
