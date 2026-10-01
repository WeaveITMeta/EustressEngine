//! ECS -> DataModel, once per frame before scripts run. Nothing here marks
//! the tree dirty, so none of it is written back.

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use avian3d::prelude::{CollisionEnd, CollisionStart, LinearVelocity, Sensor, SpatialQuery, SpatialQueryFilter};

use eustress_common::animation::character::{furnish_character, locomotion_sample};
use eustress_common::animation::humanoid::{report_state, LocomotionSample};
use eustress_common::avatar::abilities::AvatarAbilities;
use eustress_common::avatar::climb::AvatarClimb;
use eustress_common::avatar::control::AvatarCamera;
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use eustress_common::avatar::LocalAvatar;
use eustress_common::classes::BasePart;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId};
use eustress_common::luau::play::terrain::terrain_instance;
use eustress_common::luau::play::{RayHit, RayQuery};
use eustress_common::machine_input::{write_camera, write_input, write_mouse, DeviceFrame, Taken, ViewRect};
use eustress_common::scripting::{CFrame, Vector3};
use eustress_common::terrain::TerrainChunkCollider;

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
    hud: Option<Res<eustress_play_runtime::hud_input::HudPointer>>,
    mut ray_state: ResMut<MouseRayState>,
    mut injected: ResMut<InjectedInput>,
) {
    let Some(dm) = dm else {
        wheel.clear();
        return;
    };
    let window = windows.single().ok();
    let rect = window
        .map(|w| ViewRect::of(w, bounds.as_deref()))
        .unwrap_or(ViewRect { origin: Vec2::ZERO, size: Vec2::new(1280.0, 720.0) });
    // An injected cursor stands in for the OS one while it is set.
    let injecting = injected.cursor.is_some();
    let cursor = injected.cursor.or_else(|| window.and_then(|w| w.cursor_position()).map(|c| rect.local(c)));
    *ray_state = MouseRayState { cursor, viewport_origin: rect.origin, viewport_size: rect.size };

    let taken = Taken {
        // Where the real mouse is says nothing about an injected cursor.
        over_panel: !injecting && focus.as_deref().map_or(false, |f| f.has_focus),
        over_gui: !injecting && focus.as_deref().map_or(false, |f| f.gui_element_hit),
        // A HUD TextBox holding the keyboard processes the keys as much as
        // a Studio text field does.
        typing: focus.as_deref().map_or(false, |f| f.text_input_focused)
            || hud.as_deref().is_some_and(|h| h.typing),
    };
    let mut steps = std::mem::take(&mut injected.wheel);
    for w in wheel.read() {
        steps += w.y as f64;
    }
    // Injected keys and buttons arrive through the ButtonInputs; injected
    // cursor travel has no device behind it.
    let frame = DeviceFrame {
        keyboard: keyboard.as_deref(),
        mouse: mouse.as_deref(),
        cursor,
        motion: motion.as_deref().map(|m| m.delta).unwrap_or(Vec2::ZERO),
        wheel: steps,
        extra_motion: std::mem::take(&mut injected.moved),
        rect,
        taken,
    };

    let mut g = dm.dm.lock();
    g.frame.dt = time.delta_secs_f64();
    g.frame.time += g.frame.dt;
    g.frame.frame += 1;
    write_input(&mut g, &frame);
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

/// Avian contacts -> `Touched` / `TouchEnded` on both parts. A terrain chunk
/// collider has no instance of its own and touches as the Terrain.
pub fn pull_collisions(
    dm: Option<Res<PlayDataModel>>,
    mut started: MessageReader<CollisionStart>,
    mut ended: MessageReader<CollisionEnd>,
    terrain_colliders: Query<(), With<TerrainChunkCollider>>,
) {
    let Some(dm) = dm else {
        started.clear();
        ended.clear();
        return;
    };
    let mut g = dm.dm.lock();
    let terrain = terrain_instance(&g);
    let resolve = |g: &eustress_common::datamodel::DataModel, collider: Entity, body: Option<Entity>| -> Option<InstanceId> {
        g.by_entity(collider.to_bits())
            .or_else(|| body.and_then(|b| g.by_entity(b.to_bits())))
            .or_else(|| terrain.filter(|_| terrain_colliders.contains(collider)))
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
/// Joined players get theirs the same way, on the avatar that stands for
/// them here (`remote_players::pull_remote_characters`).
#[allow(clippy::type_complexity)]
pub fn pull_character(
    dm: Option<Res<PlayDataModel>>,
    avatars: Query<
        (
            Entity,
            &Transform,
            &AvatarBody,
            &AvatarIntent,
            Option<&AvatarAbilities>,
            Option<&AvatarLocomotion>,
            Option<&AvatarClimb>,
        ),
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
            retire_character(&mut g, player, model);
            *bound = None;
        }
    }
    let Some((body, tf, avatar_body, intent, abilities, loco, climb)) = avatar else { return };
    // The character spawns once the player has joined, so `CharacterAdded`
    // reaches the handlers `PlayerAdded` connected.
    if bound.is_none() && !g.in_tree(player) {
        return;
    }
    if bound.is_none() {
        let abilities = abilities.copied().unwrap_or_default();
        let Some(model) = build_character(&mut g, player, body, tf, avatar_body, abilities) else { return };
        *bound = Some((body, model));
        info!("🧍 Character bound to the local avatar ({:?})", body);
    }
    let Some((_, model)) = *bound else { return };
    place_character(&mut g, model, tf, avatar_body, intent, loco.map(|l| locomotion_sample(l, climb)));
}

/// A character's root pose: the avatar body's.
fn root_cframe(tf: &Transform) -> CFrame {
    let mut cf = CFrame::from_quaternion([tf.rotation.x as f64, tf.rotation.y as f64, tf.rotation.z as f64, tf.rotation.w as f64]);
    cf.position = Vector3::from_vec3(tf.translation);
    cf
}

/// How far above the root a character's `Head` sits: the avatar's eyes.
fn head_offset(avatar: &AvatarBody) -> Vector3 {
    Vector3::new(0.0, (avatar.metrics.eye_height - avatar.metrics.capsule_half_extent()) as f64, 0.0)
}

/// A `Character` for `player`, on the avatar `body` that stands for it on
/// this machine: a Model in Workspace named after the player, its
/// `HumanoidRootPart` bound to `body` (touches and raycasts on the avatar
/// resolve to it), a `Head`, and a `Humanoid` with the avatar's movement
/// settings and abilities. Sets `Player.Character` and fires
/// `CharacterAdded`. `None` when the tree has no Workspace.
pub(super) fn build_character(
    g: &mut DataModel,
    player: InstanceId,
    body: Entity,
    tf: &Transform,
    avatar: &AvatarBody,
    abilities: AvatarAbilities,
) -> Option<InstanceId> {
    let ws = g.find_service("Workspace")?;
    let root_cf = root_cframe(tf);
    let name = g.name_of(player).unwrap_or("Player").to_string();
    let model = g.create_virtual("Model", &name, Some(ws));
    let height = (avatar.metrics.capsule_half_extent() * 2.0) as f64;
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
    head_cf.position = root_cf.position + head_offset(avatar);
    let head = g.create_virtual("Part", "Head", Some(model));
    let _ = g.set_prop(head, "CFrame", DmValue::CFrame(head_cf));
    let _ = g.set_prop(head, "Size", DmValue::Vector3(Vector3::new(0.3, 0.3, 0.3)));
    let _ = g.set_prop(head, "Transparency", DmValue::Number(1.0));
    let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(model));
    let _ = g.set_prop(humanoid, "WalkSpeed", DmValue::Number(avatar.motion.walk_speed as f64));
    let _ = g.set_prop(humanoid, "JumpHeight", DmValue::Number(avatar.motion.jump_apex_m as f64));
    // A launch speed is JumpPower under UseJumpPower; without one, JumpPower
    // keeps the class default and JumpHeight rules.
    let _ = g.set_prop(humanoid, "UseJumpPower", DmValue::Bool(avatar.motion.jump_speed_mps.is_some()));
    if let Some(speed) = avatar.motion.jump_speed_mps {
        let _ = g.set_prop(humanoid, "JumpPower", DmValue::Number(speed as f64));
    }
    // The movement verbs this character starts with (the Space's
    // StarterPlayer switches), so a script reads what is actually in force.
    for name in AvatarAbilities::PROPERTIES {
        let _ = g.set_prop(humanoid, name, DmValue::Bool(abilities.get(name).unwrap_or(true)));
    }
    let _ = g.set_prop(model, "PrimaryPart", DmValue::Instance(root));
    // The Animator, the pacing attributes, and the Space's character scripts
    // or the default Animate, in place before `CharacterAdded`.
    furnish_character(g, model, humanoid, avatar.motion.capped_run_and_sprint().0 as f64, avatar.metrics.stride_scale as f64);
    let _ = g.set_prop(player, "Character", DmValue::Instance(model));
    // Setup writes are not script writes: nothing to apply.
    let _ = g.take_dirty_of(model);
    let _ = g.take_dirty_of(head);
    let _ = g.take_dirty_of(humanoid);
    let _ = g.take_dirty_of(player);
    g.push_event(DmEvent::CharacterAdded { player, character: model });
    Some(model)
}

/// Keep a character on its avatar: the root and head where the body is,
/// `Humanoid.MoveDirection` from its intent, and, given the avatar's
/// movement, the Humanoid's state and signals (`Running`, `Jumping`,
/// `FreeFalling`, `Climbing`, `StateChanged`). Engine writes, never dirty.
pub(super) fn place_character(
    g: &mut DataModel,
    model: InstanceId,
    tf: &Transform,
    avatar: &AvatarBody,
    intent: &AvatarIntent,
    movement: Option<LocomotionSample>,
) {
    let root_cf = root_cframe(tf);
    if let Some(root) = g.find_first_child(model, "HumanoidRootPart", false) {
        g.set_prop_from_engine(root, "CFrame", DmValue::CFrame(root_cf));
    }
    if let Some(head) = g.find_first_child(model, "Head", false) {
        let mut head_cf = root_cf;
        head_cf.position = root_cf.position + head_offset(avatar);
        g.set_prop_from_engine(head, "CFrame", DmValue::CFrame(head_cf));
    }
    if let Some(h) = g.find_first_child_of_class(model, "Humanoid", false) {
        let d = intent.direction;
        g.set_prop_from_engine(h, "MoveDirection", DmValue::Vector3(Vector3::new(d.x as f64, 0.0, d.z as f64)));
        eustress_common::animation::character::follow_run_speed(g, h, avatar.motion.capped_run_and_sprint().0 as f64);
        if let Some(sample) = movement {
            let dead = g.get_prop(h, "Health").and_then(|v| v.as_number()).is_some_and(|hp| hp <= 0.0);
            report_state(g, h, LocomotionSample { dead, ..sample });
        }
    }
}

/// Retire `player`'s character `model`, whose avatar went away. The root is
/// unbound first, so destroying the model never despawns the avatar.
pub(super) fn retire_character(g: &mut DataModel, player: InstanceId, model: InstanceId) {
    if let Some(root) = g.find_first_child(model, "HumanoidRootPart", false) {
        g.unbind_entity(root);
    }
    g.push_event(DmEvent::CharacterRemoving { player, character: model });
    g.destroy(model);
    let _ = g.set_prop(player, "Character", DmValue::Nil);
    let _ = g.take_dirty_of(player);
}

/// The cursor's ray through the play camera, `Mouse.Hit` / `Mouse.Target`,
/// and the camera state scripts read while they are not scripting it.
#[allow(clippy::too_many_arguments)]
pub fn pull_mouse_hit(
    dm: Option<Res<PlayDataModel>>,
    ray_state: Res<MouseRayState>,
    cameras: Query<
        (&Camera, &GlobalTransform, &Projection),
        Or<(With<AvatarCamera>, With<super::camera::ScriptedPlayCamera>)>,
    >,
    editor_cameras: Query<
        (&Camera, &GlobalTransform, &Projection),
        (
            With<crate::camera_controller::EustressCamera>,
            Without<AvatarCamera>,
            Without<super::camera::ScriptedPlayCamera>,
        ),
    >,
    spatial: SpatialQuery,
    colliders: eustress_common::machine_input::MouseColliders,
    avatars: Query<Entity, With<LocalAvatar>>,
) {
    let Some(dm) = dm else { return };
    let cam = cameras
        .iter()
        .find(|(c, ..)| c.is_active)
        .or_else(|| editor_cameras.iter().find(|(c, ..)| c.is_active));
    let Some((camera, cam_gt, projection)) = cam else { return };
    let rect = ViewRect { origin: ray_state.viewport_origin, size: ray_state.viewport_size };
    // Camera state for scripts (the scripted-camera system overrides it when
    // CameraType is Scriptable), then Mouse.Hit, Target and UnitRay.
    write_camera(&mut dm.dm.lock(), cam_gt, projection, rect);
    write_mouse(&dm.dm, camera, cam_gt, rect, ray_state.cursor, &spatial, &colliders, avatars.iter());
}

/// `workspace:Raycast` against Avian, synchronously. Terrain chunk colliders
/// have no instance: the filter takes or skips them together, as the Terrain.
pub fn cast_ray(
    spatial: &SpatialQuery,
    colliders: &Query<(Option<&BasePart>, Has<Sensor>, Has<TerrainChunkCollider>)>,
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
        let (part, sensor, terrain) = colliders.get(e).unwrap_or((None, false, false));
        if terrain {
            // Include lists take the Terrain only when they name it; exclude
            // lists skip it only when they do.
            return q.include == q.terrain_listed;
        }
        if q.include && !listed.contains(&e.to_bits()) {
            return false;
        }
        if q.respect_can_collide && (sensor || part.map_or(false, |b| !b.can_collide)) {
            return false;
        }
        true
    })?;
    let p = origin + *direction * hit.distance;
    Some(RayHit {
        entity: hit.entity.to_bits(),
        position: Vector3::from_vec3(p),
        normal: Vector3::from_vec3(hit.normal),
        distance: hit.distance as f64,
        terrain: colliders.get(hit.entity).map_or(false, |(_, _, terrain)| terrain),
    })
}
