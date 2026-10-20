//! # `eustress::dm`: Rune on the live Play DataModel
//!
//! The same tree Luau scripts read and write during Play
//! ([`eustress_common::datamodel`]), so the two languages share one world: a
//! part Rune recolours reads back recoloured in Luau the same frame, and a
//! BindableEvent Rune fires reaches Luau's `Event:Connect` handlers.
//!
//! Instances are passed as `i64` ids (the DataModel's `InstanceId` bits);
//! `0` means none. Rune's `on_update` builds a fresh VM each call, so a
//! script keeps its state in the tree (attributes) rather than in globals.
//! Outside a Play session every call returns its empty value.
//!
//! ```rune
//! use eustress::dm;
//!
//! pub fn on_update(dt) {
//!     let zombies = dm::find_path("Workspace.Zombies");
//!     for z in dm::children(zombies) {
//!         let root = dm::find_child(z, "HumanoidRootPart");
//!         if dm::get_attribute_bool(z, "Bounty") {
//!             dm::set_color(root, 1.0, 0.85, 0.2);
//!         }
//!     }
//! }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

use rune::{ContextError, Module};

use eustress_common::datamodel::{self, DataModel, DmEvent, DmValue, InstanceId, OutputLevel};
use eustress_common::scripting::{Color3, Vector3 as V3};

use super::rune_ecs_module::Vector3;

fn with<R>(default: R, f: impl FnOnce(&mut DataModel) -> R) -> R {
    match datamodel::active() {
        Some(dm) => f(&mut dm.lock()),
        None => default,
    }
}

fn id(v: i64) -> InstanceId {
    InstanceId(v as u64)
}

fn raw(id: InstanceId) -> i64 {
    id.0 as i64
}

fn opt(id: Option<InstanceId>) -> i64 {
    id.map(raw).unwrap_or(0)
}

// ── Finding instances ───────────────────────────────────────────────────

/// The `game` instance.
#[rune::function]
fn root() -> i64 {
    with(0, |g| raw(g.root()))
}

/// `game:GetService(name)`.
#[rune::function]
fn service(name: &str) -> i64 {
    with(0, |g| opt(g.get_service(name)))
}

/// `parent:FindFirstChild(name)`.
#[rune::function]
fn find_child(parent: i64, name: &str) -> i64 {
    with(0, |g| opt(g.find_first_child(id(parent), name, false)))
}

/// A dotted path from `game`: `"Workspace.Map.Barrels"`.
#[rune::function]
fn find_path(path: &str) -> i64 {
    with(0, |g| {
        let mut cur = g.root();
        for seg in path.split('.').filter(|s| !s.is_empty()) {
            let next = if cur == g.root() {
                g.find_service(seg).or_else(|| g.find_first_child(cur, seg, false))
            } else {
                g.find_first_child(cur, seg, false)
            };
            match next {
                Some(n) => cur = n,
                None => return 0,
            }
        }
        raw(cur)
    })
}

#[rune::function]
fn children(parent: i64) -> Vec<i64> {
    with(Vec::new(), |g| g.children(id(parent)).iter().map(|c| raw(*c)).collect())
}

#[rune::function]
fn descendants(parent: i64) -> Vec<i64> {
    with(Vec::new(), |g| g.descendants(id(parent)).into_iter().map(raw).collect())
}

/// `CollectionService:GetTagged(tag)`.
#[rune::function]
fn tagged(tag: &str) -> Vec<i64> {
    with(Vec::new(), |g| g.tagged(tag).into_iter().map(raw).collect())
}

#[rune::function]
fn exists(i: i64) -> bool {
    with(false, |g| g.exists(id(i)))
}

#[rune::function]
fn parent(i: i64) -> i64 {
    with(0, |g| opt(g.parent(id(i))))
}

#[rune::function]
fn instance_name(i: i64) -> String {
    with(String::new(), |g| g.name_of(id(i)).unwrap_or("").to_string())
}

#[rune::function]
fn class_name(i: i64) -> String {
    with(String::new(), |g| g.class_of(id(i)).unwrap_or("").to_string())
}

#[rune::function]
fn is_a(i: i64, class: &str) -> bool {
    with(false, |g| g.is_a(id(i), class))
}

// ── Properties ──────────────────────────────────────────────────────────

#[rune::function]
fn get_number(i: i64, prop: &str) -> f64 {
    with(0.0, |g| g.get_prop(id(i), prop).and_then(|v| v.as_number()).unwrap_or(0.0))
}

#[rune::function]
fn set_number(i: i64, prop: &str, value: f64) -> bool {
    with(false, |g| g.set_prop(id(i), prop, DmValue::Number(value)).is_ok())
}

#[rune::function]
fn get_bool(i: i64, prop: &str) -> bool {
    with(false, |g| g.get_prop(id(i), prop).and_then(|v| v.as_bool()).unwrap_or(false))
}

#[rune::function]
fn set_bool(i: i64, prop: &str, value: bool) -> bool {
    with(false, |g| g.set_prop(id(i), prop, DmValue::Bool(value)).is_ok())
}

#[rune::function]
fn get_string(i: i64, prop: &str) -> String {
    with(String::new(), |g| {
        g.get_prop(id(i), prop).map(|v| match v {
            DmValue::String(s) => s,
            other => other.display(),
        })
        .unwrap_or_default()
    })
}

/// Strings also set enum properties: `set_string(part, "Material", "Neon")`.
#[rune::function]
fn set_string(i: i64, prop: &str, value: &str) -> bool {
    with(false, |g| g.set_prop(id(i), prop, DmValue::String(value.to_string())).is_ok())
}

#[rune::function]
fn get_vector3(i: i64, prop: &str) -> Vector3 {
    let v = with(V3::ZERO, |g| g.get_prop(id(i), prop).and_then(|v| v.as_vector3()).unwrap_or(V3::ZERO));
    Vector3 { x: v.x, y: v.y, z: v.z }
}

#[rune::function]
fn set_vector3(i: i64, prop: &str, value: &Vector3) -> bool {
    with(false, |g| g.set_prop(id(i), prop, DmValue::Vector3(V3::new(value.x, value.y, value.z))).is_ok())
}

/// A part's (or model's PrimaryPart's) position.
#[rune::function]
fn get_position(i: i64) -> Vector3 {
    let v = with(V3::ZERO, |g| {
        g.get_prop(id(i), "Position")
            .and_then(|v| v.as_vector3())
            .or_else(|| match g.get_prop(id(i), "PrimaryPart") {
                Some(DmValue::Instance(pp)) => g.get_prop(pp, "Position").and_then(|v| v.as_vector3()),
                _ => None,
            })
            .unwrap_or(V3::ZERO)
    });
    Vector3 { x: v.x, y: v.y, z: v.z }
}

#[rune::function]
fn set_position(i: i64, value: &Vector3) -> bool {
    with(false, |g| g.set_prop(id(i), "Position", DmValue::Vector3(V3::new(value.x, value.y, value.z))).is_ok())
}

/// `part.Color = Color3.new(r, g, b)` with components in 0..1.
#[rune::function]
fn set_color(i: i64, r: f64, gr: f64, b: f64) -> bool {
    with(false, |g| g.set_prop(id(i), "Color", DmValue::Color3(Color3::new(r, gr, b))).is_ok())
}

/// An instance-valued property (`Attachment0`, `Part0`, `PrimaryPart`,
/// `Occupant`, `Adornee`, an ObjectValue's `Value`); `0` when it is empty.
#[rune::function]
fn get_instance(i: i64, prop: &str) -> i64 {
    with(0, |g| match g.get_prop(id(i), prop) {
        Some(DmValue::Instance(other)) => raw(other),
        _ => 0,
    })
}

/// Sets an instance-valued property, such as a constraint's `Attachment0`;
/// `0` clears it.
#[rune::function]
fn set_instance(i: i64, prop: &str, other: i64) -> bool {
    let value = if other == 0 { DmValue::Nil } else { DmValue::Instance(id(other)) };
    with(false, |g| g.set_prop(id(i), prop, value).is_ok())
}

// ── Attributes ──────────────────────────────────────────────────────────

#[rune::function]
fn get_attribute_number(i: i64, name: &str) -> f64 {
    with(0.0, |g| g.get_attribute(id(i), name).and_then(|v| v.as_number()).unwrap_or(0.0))
}

#[rune::function]
fn set_attribute_number(i: i64, name: &str, value: f64) -> bool {
    with(false, |g| g.set_attribute(id(i), name, DmValue::Number(value)).is_ok())
}

#[rune::function]
fn get_attribute_bool(i: i64, name: &str) -> bool {
    with(false, |g| g.get_attribute(id(i), name).and_then(|v| v.as_bool()).unwrap_or(false))
}

#[rune::function]
fn set_attribute_bool(i: i64, name: &str, value: bool) -> bool {
    with(false, |g| g.set_attribute(id(i), name, DmValue::Bool(value)).is_ok())
}

#[rune::function]
fn get_attribute_string(i: i64, name: &str) -> String {
    with(String::new(), |g| {
        g.get_attribute(id(i), name).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
    })
}

#[rune::function]
fn set_attribute_string(i: i64, name: &str, value: &str) -> bool {
    with(false, |g| g.set_attribute(id(i), name, DmValue::String(value.to_string())).is_ok())
}

// ── Structure ───────────────────────────────────────────────────────────

/// `Instance.new(class)`: detached until parented.
#[rune::function]
fn create(class: &str) -> i64 {
    with(0, |g| raw(g.create(class)))
}

/// `instance.Parent = parent` (`0` detaches).
#[rune::function]
fn set_parent(i: i64, parent: i64) -> bool {
    with(false, |g| {
        let p = if parent == 0 { None } else { Some(id(parent)) };
        g.set_parent(id(i), p).is_ok()
    })
}

#[rune::function]
fn clone_instance(i: i64) -> i64 {
    with(0, |g| opt(g.clone_instance(id(i))))
}

#[rune::function]
fn destroy(i: i64) {
    with((), |g| g.destroy(id(i)))
}

// ── Signals, time, output ───────────────────────────────────────────────

/// `bindable:Fire(message)`: Luau `Event:Connect` handlers receive the string.
#[rune::function]
fn fire(event: i64, message: &str) {
    with((), |g| {
        g.push_event(DmEvent::Fired {
            event: id(event),
            args: vec![DmValue::String(message.to_string())],
            from_luau: false,
        })
    })
}

/// Seconds since Play started.
#[rune::function]
fn now() -> f64 {
    with(0.0, |g| g.frame.time)
}

/// Seconds since the previous frame.
#[rune::function]
fn delta() -> f64 {
    with(0.0, |g| g.frame.dt)
}

/// A line in the Output panel.
#[rune::function]
fn log(text: &str) {
    with((), |g| g.print(OutputLevel::Info, "Rune", text.to_string()))
}

const RNG_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
static RNG: AtomicU64 = AtomicU64::new(RNG_SEED);

/// Shared xorshift, reseeded from the clock on first use.
fn next_u64() -> u64 {
    let mut x = RNG.load(Ordering::Relaxed);
    if x == RNG_SEED {
        x ^= std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG.store(x, Ordering::Relaxed);
    x
}

/// Uniform in [0, 1).
#[rune::function]
fn random() -> f64 {
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A uniform index in `0..n` (`0` when `n` is not positive): picking a
/// random element needs no float/integer conversion in the script.
#[rune::function]
fn random_index(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    (next_u64() % n as u64) as i64
}

// ── Input: this frame's keyboard and mouse, exactly as Luau sees them ───

/// `UserInputService:IsKeyDown(Enum.KeyCode.W)` is `dm::key_down("W")`:
/// Roblox `KeyCode` names ("Space", "LeftShift", "One").
#[rune::function]
fn key_down(name: &str) -> bool {
    with(false, |g| g.input.keys.contains(name))
}

/// `UserInputService:IsMouseButtonPressed(...)` is
/// `dm::mouse_down("MouseButton1")`.
#[rune::function]
fn mouse_down(name: &str) -> bool {
    with(false, |g| g.input.buttons.contains(name))
}

/// The cursor in viewport pixels, origin top-left.
#[rune::function]
fn mouse_position() -> (f64, f64) {
    with((0.0, 0.0), |g| (g.input.mouse_x, g.input.mouse_y))
}

/// `Mouse.Hit.Position`: where the cursor's ray meets the world.
#[rune::function]
fn mouse_hit() -> Vector3 {
    let v = with(V3::ZERO, |g| g.mouse.hit_position);
    Vector3 { x: v.x, y: v.y, z: v.z }
}

/// `Mouse.Target`: the part under the cursor, or 0.
#[rune::function]
fn mouse_target() -> i64 {
    with(0, |g| opt(g.mouse.target))
}

// ── Animation: Animator tracks, as Luau's AnimationTrack ────────────────

/// A track call's error goes to Output as a warning; the call returns false.
fn done(g: &mut DataModel, result: Result<(), String>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            g.print(OutputLevel::Warn, "Rune", e);
            false
        }
    }
}

/// `animator:LoadAnimation(animation)`: a track, or 0 with the reason in
/// Output. A Humanoid or AnimationController stands for its Animator.
#[rune::function]
fn load_animation(animator: i64, animation: i64) -> i64 {
    with(0, |g| match g.load_animation(id(animator), id(animation)) {
        Ok(track) => raw(track),
        Err(e) => {
            g.print(OutputLevel::Warn, "Rune", e);
            0
        }
    })
}

/// The same, from a content id (`space://...`, `rig://walk`), with no
/// Animation instance.
#[rune::function]
fn load_animation_id(animator: i64, content: &str) -> i64 {
    with(0, |g| match g.load_animation_content(id(animator), content) {
        Ok(track) => raw(track),
        Err(e) => {
            g.print(OutputLevel::Warn, "Rune", e);
            0
        }
    })
}

/// `track:Play(fade, weight, speed)`.
#[rune::function]
fn play(track: i64, fade: f64, weight: f64, speed: f64) -> bool {
    with(false, |g| {
        let r = g.play_track(id(track), fade as f32, weight as f32, speed as f32);
        done(g, r)
    })
}

/// `track:Stop(fade)`.
#[rune::function]
fn stop(track: i64, fade: f64) -> bool {
    with(false, |g| {
        let r = g.stop_track(id(track), fade as f32);
        done(g, r)
    })
}

/// `track:AdjustSpeed(speed)`.
#[rune::function]
fn adjust_speed(track: i64, speed: f64) -> bool {
    with(false, |g| {
        let r = g.adjust_track_speed(id(track), speed as f32);
        done(g, r)
    })
}

/// `track:AdjustWeight(weight, fade)`.
#[rune::function]
fn adjust_weight(track: i64, weight: f64, fade: f64) -> bool {
    with(false, |g| {
        let r = g.adjust_track_weight(id(track), weight as f32, fade as f32);
        done(g, r)
    })
}

/// `track.TimePosition`, seconds.
#[rune::function]
fn track_time(track: i64) -> f64 {
    with(0.0, |g| g.get_prop(id(track), "TimePosition").and_then(|v| v.as_number()).unwrap_or(0.0))
}

/// `track.TimePosition = t`: a seek, which fires no events for what it skips.
#[rune::function]
fn set_track_time(track: i64, t: f64) -> bool {
    with(false, |g| {
        let r = g.set_prop(id(track), "TimePosition", DmValue::Number(t));
        done(g, r)
    })
}

/// `track.Length`, seconds: 0 until the clip has loaded.
#[rune::function]
fn track_length(track: i64) -> f64 {
    with(0.0, |g| g.get_prop(id(track), "Length").and_then(|v| v.as_number()).unwrap_or(0.0))
}

/// `track.IsPlaying`.
#[rune::function]
fn is_playing(track: i64) -> bool {
    with(false, |g| g.get_prop(id(track), "IsPlaying").and_then(|v| v.as_bool()).unwrap_or(false))
}

/// `animator:GetPlayingAnimationTracks()`: playing, or still fading out.
#[rune::function]
fn playing_tracks(animator: i64) -> Vec<i64> {
    with(Vec::new(), |g| g.playing_tracks(id(animator)).into_iter().map(raw).collect())
}

/// This frame's track events, in the order they fired: the track, the kind
/// (`stopped`, `ended`, `looped`, `keyframe`, `marker`), the keyframe's or
/// marker's name, and the marker's value. Rune polls where Luau connects.
#[rune::function]
fn animation_events() -> Vec<(i64, String, String, String)> {
    with(Vec::new(), |g| {
        g.animation
            .frame_events()
            .iter()
            .map(|e| (raw(e.track), e.kind.to_string(), e.name.clone(), e.value.clone()))
            .collect()
    })
}

/// The `eustress::dm` module.
pub fn create_datamodel_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["dm"])?;
    m.function_meta(root)?;
    m.function_meta(service)?;
    m.function_meta(find_child)?;
    m.function_meta(find_path)?;
    m.function_meta(children)?;
    m.function_meta(descendants)?;
    m.function_meta(tagged)?;
    m.function_meta(exists)?;
    m.function_meta(parent)?;
    m.function_meta(instance_name)?;
    m.function_meta(class_name)?;
    m.function_meta(is_a)?;
    m.function_meta(get_number)?;
    m.function_meta(set_number)?;
    m.function_meta(get_bool)?;
    m.function_meta(set_bool)?;
    m.function_meta(get_string)?;
    m.function_meta(set_string)?;
    m.function_meta(get_vector3)?;
    m.function_meta(set_vector3)?;
    m.function_meta(get_position)?;
    m.function_meta(set_position)?;
    m.function_meta(set_color)?;
    m.function_meta(get_instance)?;
    m.function_meta(set_instance)?;
    m.function_meta(get_attribute_number)?;
    m.function_meta(set_attribute_number)?;
    m.function_meta(get_attribute_bool)?;
    m.function_meta(set_attribute_bool)?;
    m.function_meta(get_attribute_string)?;
    m.function_meta(set_attribute_string)?;
    m.function_meta(create)?;
    m.function_meta(set_parent)?;
    m.function_meta(clone_instance)?;
    m.function_meta(destroy)?;
    m.function_meta(fire)?;
    m.function_meta(now)?;
    m.function_meta(delta)?;
    m.function_meta(log)?;
    m.function_meta(random)?;
    m.function_meta(random_index)?;
    m.function_meta(key_down)?;
    m.function_meta(mouse_down)?;
    m.function_meta(mouse_position)?;
    m.function_meta(mouse_hit)?;
    m.function_meta(mouse_target)?;
    m.function_meta(load_animation)?;
    m.function_meta(load_animation_id)?;
    m.function_meta(play)?;
    m.function_meta(stop)?;
    m.function_meta(adjust_speed)?;
    m.function_meta(adjust_weight)?;
    m.function_meta(track_time)?;
    m.function_meta(set_track_time)?;
    m.function_meta(track_length)?;
    m.function_meta(is_playing)?;
    m.function_meta(playing_tracks)?;
    m.function_meta(animation_events)?;
    Ok(m)
}
