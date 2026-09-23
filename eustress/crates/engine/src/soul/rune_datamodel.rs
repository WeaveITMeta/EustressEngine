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
    Ok(m)
}
