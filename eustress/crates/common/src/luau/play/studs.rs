//! Studs at the Luau boundary, for scripts written for Roblox.
//!
//! The engine is metres. A script the importer marks as written for Roblox
//! (its `ScriptOrigin` is `"roblox"`, from `[script] origin` in its file)
//! lives in a stud world: every length it reads or writes converts as it
//! crosses the boundary, and its own arithmetic stays in studs, so
//! `part.Position.Y + part.Size.Y / 2` is the top of the part in studs, as on
//! Roblox. Design: `docs/design/ROBLOX_STUDS_BOUNDARY.md`.
//!
//! What converts is decided by each property's unit, never by its value:
//! [`UNITS`] classifies every property of the classes in scope whose type
//! could carry a length. A length, a speed and an acceleration are multiplied
//! by the stud on the way in and divided on the way out; a CFrame's
//! translation converts and its rotation does not. Mass is left as it is.
//! `JumpPower` is a launch speed like any other: the avatar takes off from
//! whichever of it and `JumpHeight` `UseJumpPower` selects, under the Space's
//! live gravity.
//!
//! The spatial calls (raycasts, bounds queries, pivots, bounding boxes,
//! `MoveTo`, camera rays, `DistanceFromCharacter`) convert their arguments and
//! results by [`script_scale`], and the mouse converts `Hit`, `Origin` and
//! `UnitRay`. The speeds `Humanoid.Running` and `Climbing` report reach such a
//! script's handlers in studs a second (the prelude divides them).
//!
//! Whose numbers these are is decided by the script that owns the running
//! thread; a tween carries the units of the script that made it. A native
//! script, written in metres, sees the engine unchanged: its scale is exactly
//! 1. The stud is [`Unit::Stud`](crate::units::Unit::Stud).

use std::collections::HashMap;
use std::sync::OnceLock;

use mlua::{Function, Result as LuaResult, Table};

use crate::datamodel::{class_is_a, DataModel, DmValue, InstanceId};
use crate::scripting::{CFrame, Vector3};

/// The script property that says whose units a script is written in, and its
/// value for a script written for Roblox.
pub const ORIGIN_PROPERTY: &str = "ScriptOrigin";
pub const ROBLOX_ORIGIN: &str = "roblox";

/// Metres in one stud.
pub fn stud_metres() -> f64 {
    crate::units::Unit::Stud.to_meters()
}

/// Whether `script` is written for Roblox, in studs.
pub fn script_writes_studs(dm: &DataModel, script: InstanceId) -> bool {
    dm.get_prop(script, ORIGIN_PROPERTY).is_some_and(|v| v.as_str() == Some(ROBLOX_ORIGIN))
}

/// What a property measures, as far as studs are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// A length or a position: × stud.
    Length,
    /// A CFrame: its translation × stud, its rotation as is.
    Frame,
    /// A speed or a velocity: × stud.
    Speed,
    /// An acceleration: × stud.
    Acceleration,
    /// A ray whose direction is a unit vector: its origin × stud.
    UnitRay,
    /// A mass or a density, left as it is: Roblox's mass unit follows its own
    /// density, which converting lengths does not reach.
    Mass,
    /// No length in it: angles, angular speeds, ratios, pixels, times, unit
    /// directions.
    Unitless,
}

impl Unit {
    /// Whether values of this unit convert at the boundary.
    pub fn converts(self) -> bool {
        !matches!(self, Unit::Mass | Unit::Unitless)
    }
}

use Unit::*;

/// Every property of the classes in scope whose type could carry a length
/// (numbers, Vector3, CFrame, Ray), classified: the class that declares it,
/// the property, its unit. A test in `eustress-roblox-import` lists the
/// reflection database's candidates for these classes and fails on one this
/// table leaves out.
pub static UNITS: &[(&str, &str, Unit)] = &[
    // BasePart
    ("BasePart", "AssemblyAngularVelocity", Unitless),
    ("BasePart", "AssemblyCenterOfMass", Length),
    ("BasePart", "AssemblyLinearVelocity", Speed),
    ("BasePart", "AssemblyMass", Mass),
    ("BasePart", "BackParamA", Unitless),
    ("BasePart", "BackParamB", Unitless),
    ("BasePart", "BottomParamA", Unitless),
    ("BasePart", "BottomParamB", Unitless),
    ("BasePart", "CFrame", Frame),
    ("BasePart", "CenterOfMass", Length),
    ("BasePart", "CollisionGroupId", Unitless),
    ("BasePart", "Elasticity", Unitless),
    ("BasePart", "ExtentsCFrame", Frame),
    ("BasePart", "ExtentsSize", Length),
    ("BasePart", "Friction", Unitless),
    ("BasePart", "FrontParamA", Unitless),
    ("BasePart", "FrontParamB", Unitless),
    ("BasePart", "LeftParamA", Unitless),
    ("BasePart", "LeftParamB", Unitless),
    ("BasePart", "LocalTransparencyModifier", Unitless),
    ("BasePart", "Mass", Mass),
    ("BasePart", "Orientation", Unitless),
    ("BasePart", "PivotOffset", Frame),
    ("BasePart", "Position", Length),
    ("BasePart", "ReceiveAge", Unitless),
    ("BasePart", "Reflectance", Unitless),
    ("BasePart", "ResizeIncrement", Length),
    ("BasePart", "RightParamA", Unitless),
    ("BasePart", "RightParamB", Unitless),
    ("BasePart", "RootPriority", Unitless),
    ("BasePart", "RotVelocity", Unitless),
    ("BasePart", "Rotation", Unitless),
    ("BasePart", "Size", Length),
    ("BasePart", "SpecificGravity", Mass),
    ("BasePart", "TopParamA", Unitless),
    ("BasePart", "TopParamB", Unitless),
    ("BasePart", "Transparency", Unitless),
    ("BasePart", "Velocity", Speed),
    ("BasePart", "WorldPosition", Length),
    ("MeshPart", "JointOffset", Length),
    ("TriangleMeshPart", "MeshSize", Length),
    // Model
    ("Model", "Scale", Unitless),
    ("Model", "WorldPivot", Frame),
    // Attachment
    ("Attachment", "Axis", Unitless),
    ("Attachment", "CFrame", Frame),
    ("Attachment", "Orientation", Unitless),
    ("Attachment", "Position", Length),
    ("Attachment", "Rotation", Unitless),
    ("Attachment", "SecondaryAxis", Unitless),
    ("Attachment", "WorldAxis", Unitless),
    ("Attachment", "WorldCFrame", Frame),
    ("Attachment", "WorldOrientation", Unitless),
    ("Attachment", "WorldPosition", Length),
    ("Attachment", "WorldRotation", Unitless),
    ("Attachment", "WorldSecondaryAxis", Unitless),
    // Camera
    ("Camera", "CFrame", Frame),
    ("Camera", "CoordinateFrame", Frame),
    ("Camera", "DiagonalFieldOfView", Unitless),
    ("Camera", "FieldOfView", Unitless),
    ("Camera", "Focus", Frame),
    ("Camera", "focus", Frame),
    ("Camera", "HeadScale", Unitless),
    ("Camera", "MaxAxisFieldOfView", Unitless),
    ("Camera", "NearPlaneZ", Length),
    ("Camera", "OrthographicSize", Length),
    // Workspace
    ("Workspace", "AirDensity", Mass),
    ("Workspace", "AirTurbulenceIntensity", Unitless),
    ("Workspace", "DistributedGameTime", Unitless),
    ("Workspace", "FallenPartsDestroyHeight", Length),
    ("Workspace", "GlobalWind", Speed),
    ("Workspace", "Gravity", Acceleration),
    ("Workspace", "InsertPoint", Length),
    // The mouse (a script's PlayerMouse)
    ("Mouse", "Hit", Frame),
    ("Mouse", "hit", Frame),
    ("Mouse", "Origin", Frame),
    ("Mouse", "UnitRay", UnitRay),
    ("Mouse", "ViewSizeX", Unitless),
    ("Mouse", "ViewSizeY", Unitless),
    ("Mouse", "X", Unitless),
    ("Mouse", "Y", Unitless),
    // Humanoid
    ("Humanoid", "CameraOffset", Length),
    ("Humanoid", "Health", Unitless),
    ("Humanoid", "HealthDisplayDistance", Length),
    ("Humanoid", "HipHeight", Length),
    ("Humanoid", "JumpHeight", Length),
    ("Humanoid", "JumpPower", Speed),
    ("Humanoid", "MaxHealth", Unitless),
    ("Humanoid", "maxHealth", Unitless),
    ("Humanoid", "MaxSlopeAngle", Unitless),
    ("Humanoid", "MoveDirection", Unitless),
    ("Humanoid", "NameDisplayDistance", Length),
    ("Humanoid", "TargetPoint", Length),
    ("Humanoid", "WalkSpeed", Speed),
    ("Humanoid", "WalkToPoint", Length),
    // StarterPlayer
    ("StarterPlayer", "CameraMaxZoomDistance", Length),
    ("StarterPlayer", "CameraMinZoomDistance", Length),
    ("StarterPlayer", "CharacterJumpHeight", Length),
    ("StarterPlayer", "CharacterJumpPower", Speed),
    ("StarterPlayer", "CharacterMaxSlopeAngle", Unitless),
    ("StarterPlayer", "CharacterWalkSpeed", Speed),
    ("StarterPlayer", "HealthDisplayDistance", Length),
    ("StarterPlayer", "NameDisplayDistance", Length),
];

fn index() -> &'static HashMap<&'static str, Vec<(&'static str, Unit)>> {
    static INDEX: OnceLock<HashMap<&'static str, Vec<(&'static str, Unit)>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map: HashMap<&'static str, Vec<(&'static str, Unit)>> = HashMap::new();
        for (class, key, unit) in UNITS {
            map.entry(*key).or_default().push((*class, *unit));
        }
        map
    })
}

/// The unit of `class.key`, from the class that declares it; `None` for a
/// property the table does not classify.
pub fn unit_of(class: &str, key: &str) -> Option<Unit> {
    index().get(key)?.iter().find(|(decl, _)| *decl == class || class_is_a(class, decl)).map(|(_, u)| *u)
}

/// The static name of `key` as the table spells it.
fn static_key(key: &str) -> Option<&'static str> {
    index().get_key_value(key).map(|(k, _)| *k)
}

/// Whether `class.key` converts for a script written in studs.
pub fn converts(class: &str, key: &str) -> bool {
    unit_of(class, key).is_some_and(Unit::converts)
}

/// `v` in a unit, times `k`: a CFrame's translation only, a number or a
/// Vector3 whole. Values of other types pass unchanged.
pub fn scale_value(v: &DmValue, unit: Unit, k: f64) -> DmValue {
    match (unit, v) {
        (Unit::Frame, DmValue::CFrame(cf)) => DmValue::CFrame(scale_frame(*cf, k)),
        (Length | Speed | Acceleration, DmValue::Number(n)) => DmValue::Number(n * k),
        (Length | Speed | Acceleration, DmValue::Vector3(v)) => DmValue::Vector3(*v * k),
        _ => v.clone(),
    }
}

/// A CFrame with its translation times `k`.
pub fn scale_frame(cf: CFrame, k: f64) -> CFrame {
    let mut out = cf;
    out.position = cf.position * k;
    out
}

/// A position or a vector times `k`.
pub fn scale_vec(v: Vector3, k: f64) -> Vector3 {
    v * k
}

/// What a studs-written value for `class.key` stores: each property and its
/// value in metres. `None` for a property that does not convert.
pub fn writes_from_studs(class: &str, key: &str, v: &DmValue) -> Option<Vec<(&'static str, DmValue)>> {
    let unit = unit_of(class, key).filter(|u| u.converts())?;
    let name = static_key(key)?;
    Some(vec![(name, scale_value(v, unit, stud_metres()))])
}

/// What a script written in studs reads for `class.key`: the stored value in
/// studs. `None` for a property that does not convert or is not set.
pub fn read_in_studs(dm: &DataModel, id: InstanceId, class: &str, key: &str) -> Option<DmValue> {
    let unit = unit_of(class, key).filter(|u| u.converts())?;
    Some(scale_value(&dm.get_prop(id, key)?, unit, 1.0 / stud_metres()))
}

/// Whether the code running now belongs to a script written in studs: asks
/// the prelude, which knows the running thread's owner.
pub fn running_in_studs(host: &Table) -> LuaResult<bool> {
    let f: Function = host.raw_get("ownerUsesStuds")?;
    f.call::<bool>(())
}

/// Metres per unit of the running script: the stud for a script written in
/// studs, exactly 1 for a native one. Multiply what the script hands the
/// engine, divide what the engine hands it back.
pub fn script_scale(host: &Table) -> LuaResult<f64> {
    Ok(if running_in_studs(host)? { stud_metres() } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::{new_shared, OutputLevel, SharedDataModel};
    use crate::luau::play::{instance, PlayLuau, RayHit, RayQuery, ScriptLaunch, TerrainReadFn};

    fn no_rays(_: &RayQuery) -> Option<RayHit> {
        None
    }

    /// The ground: a plane at y = 0 that every downward ray meets.
    fn ground(q: &RayQuery) -> Option<RayHit> {
        let (o, d) = (q.origin, q.direction);
        if d.y >= 0.0 || o.y < 0.0 {
            return None;
        }
        let t = -o.y / d.y;
        (t <= 1.0).then(|| RayHit {
            entity: 0,
            position: o + d * t,
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: (d * t).magnitude(),
            terrain: false,
        })
    }

    /// A VM, a Humanoid in the Workspace, and two scripts: one written for
    /// Roblox, one native.
    fn session() -> (PlayLuau, SharedDataModel, InstanceId, InstanceId, InstanceId) {
        let dm = new_shared();
        let (hum, roblox, native) = {
            let mut g = dm.lock();
            let ws = g.get_service("Workspace").unwrap();
            let hum = g.create_virtual("Humanoid", "Humanoid", Some(ws));
            let sss = g.get_service("ServerScriptService").unwrap();
            let roblox = g.create_virtual("Script", "Imported", Some(sss));
            g.set_prop(roblox, ORIGIN_PROPERTY, DmValue::String(ROBLOX_ORIGIN.into())).unwrap();
            let native = g.create_virtual("Script", "Native", Some(sss));
            (hum, roblox, native)
        };
        let vm = PlayLuau::new(dm.clone()).expect("the prelude loads");
        (vm, dm, hum, roblox, native)
    }

    fn run_with(vm: &mut PlayLuau, dm: &SharedDataModel, script: InstanceId, source: &str, rays: &crate::luau::play::RaycastFn<'_>) {
        let name = dm.lock().full_name(script);
        let launch = ScriptLaunch { instance: script, source: source.to_string(), chunk_name: name };
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.run_scripts(vec![launch], rays, no_terrain);
        let errors: Vec<String> =
            dm.lock().output.iter().filter(|l| l.level == OutputLevel::Error).map(|l| l.text.clone()).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    fn run(vm: &mut PlayLuau, dm: &SharedDataModel, script: InstanceId, source: &str) {
        run_with(vm, dm, script, source, &no_rays);
    }

    fn metres(dm: &SharedDataModel, id: InstanceId, key: &str) -> f64 {
        dm.lock().get_prop(id, key).and_then(|v| v.as_number()).unwrap()
    }

    fn printed(dm: &SharedDataModel) -> Vec<String> {
        dm.lock().output.iter().map(|l| l.text.clone()).collect()
    }

    /// A part at (1, 2, 3) m, 4 × 1 × 2 m, in the Workspace.
    fn part(dm: &SharedDataModel) -> InstanceId {
        let mut g = dm.lock();
        let ws = g.get_service("Workspace").unwrap();
        let p = g.create("Part");
        g.set_prop(p, "Name", DmValue::String("Crate".into())).unwrap();
        g.set_prop(p, "Position", DmValue::Vector3(Vector3::new(1.0, 2.0, 3.0))).unwrap();
        g.set_prop(p, "Size", DmValue::Vector3(Vector3::new(4.0, 1.0, 2.0))).unwrap();
        g.set_parent(p, Some(ws)).unwrap();
        p
    }

    #[test]
    fn every_unit_converts_its_own_way() {
        let s = stud_metres();
        let n = DmValue::Number(2.0);
        assert_eq!(scale_value(&n, Length, s), DmValue::Number(2.0 * s));
        assert_eq!(scale_value(&n, Unitless, s), n, "unitless values pass unchanged");
        assert_eq!(scale_value(&n, Mass, s), n, "mass is left as it is");
        let cf = CFrame::new(1.0, 2.0, 3.0) * CFrame::from_euler_angles_xyz(0.3, 0.0, 0.0);
        let DmValue::CFrame(out) = scale_value(&DmValue::CFrame(cf), Frame, s) else { panic!() };
        assert!((out.position.y - 2.0 * s).abs() < 1e-12, "a frame's translation converts");
        assert_eq!(out.look_vector(), cf.look_vector(), "and its rotation does not");
        assert_eq!(unit_of("Part", "Position"), Some(Length), "a Part is a BasePart");
        assert_eq!(unit_of("MeshPart", "MeshSize"), Some(Length));
        assert_eq!(unit_of("Workspace", "WorldPivot"), Some(Frame), "the Workspace is a Model");
        assert_eq!(unit_of("Part", "Orientation"), Some(Unitless));
        assert_eq!(unit_of("Part", "Nonsense"), None);
    }

    #[test]
    fn a_roblox_script_moves_a_humanoid_in_studs_and_reads_studs_back() {
        let (mut vm, dm, hum, roblox, _) = session();
        run(&mut vm, &dm, roblox, "local h = workspace.Humanoid\n\
            h.WalkSpeed = 16\nh.HipHeight = 2\nh.JumpPower = 50\n\
            print(string.format('%.3f %.3f %.3f', h.WalkSpeed, h.HipHeight, h.JumpPower))");
        let s = stud_metres();
        assert!((metres(&dm, hum, "WalkSpeed") - 16.0 * s).abs() < 1e-9);
        assert!((metres(&dm, hum, "HipHeight") - 2.0 * s).abs() < 1e-9);
        assert!((metres(&dm, hum, "JumpPower") - 50.0 * s).abs() < 1e-9, "JumpPower is a launch speed in m/s");
        assert!(printed(&dm).contains(&"16.000 2.000 50.000".to_string()), "{:?}", printed(&dm));
    }

    #[test]
    fn a_native_script_sees_metres_unchanged() {
        let (mut vm, dm, hum, _, native) = session();
        let p = part(&dm);
        run(&mut vm, &dm, native, "local h = workspace.Humanoid\nh.WalkSpeed = 8\nh.JumpPower = 14\n\
            local c = workspace.Crate\nc.Position = c.Position + Vector3.new(0, 1, 0)\nprint(h.WalkSpeed, c.Position.Y)");
        assert_eq!(metres(&dm, hum, "WalkSpeed"), 8.0);
        assert_eq!(metres(&dm, hum, "JumpPower"), 14.0);
        assert_eq!(metres(&dm, hum, "JumpHeight"), 2.0, "a native JumpPower leaves the height alone");
        let pos = dm.lock().get_prop(p, "Position").and_then(|v| v.as_vector3()).unwrap();
        assert_eq!(pos.y, 3.0, "one metre up, exactly");
        assert!(printed(&dm).contains(&"8 3".to_string()), "{:?}", printed(&dm));
    }

    #[test]
    fn jump_power_is_a_launch_speed_and_leaves_the_height_alone() {
        // Whichever UseJumpPower says, JumpPower converts on its own; the
        // avatar picks the value to jump by, under live gravity.
        for use_power in [true, false] {
            let (mut vm, dm, hum, roblox, _) = session();
            dm.lock().set_prop(hum, "UseJumpPower", DmValue::Bool(use_power)).unwrap();
            run(&mut vm, &dm, roblox, "workspace.Humanoid.JumpPower = 50");
            assert!((metres(&dm, hum, "JumpPower") - 50.0 * stud_metres()).abs() < 1e-9);
            assert_eq!(metres(&dm, hum, "JumpHeight"), 2.0, "UseJumpPower = {use_power}");
        }
    }

    #[test]
    fn a_roblox_script_sees_parts_in_studs() {
        let (mut vm, dm, _, roblox, _) = session();
        let p = part(&dm);
        let s = stud_metres();
        run(&mut vm, &dm, roblox, "local c = workspace.Crate\n\
            print(string.format('%.4f %.4f', c.Position.Y, c.Size.X))\n\
            c.Position = c.Position + Vector3.new(0, 10, 0)\n\
            c.CFrame = c.CFrame * CFrame.new(1, 0, 0)\n\
            workspace.Gravity = 196.2");
        assert!(printed(&dm).contains(&format!("{:.4} {:.4}", 2.0 / s, 4.0 / s)), "{:?}", printed(&dm));
        let pos = dm.lock().get_prop(p, "Position").and_then(|v| v.as_vector3()).unwrap();
        assert!((pos.y - (2.0 + 10.0 * s)).abs() < 1e-9, "ten studs up is 10 × stud metres: {pos:?}");
        assert!((pos.x - (1.0 + 1.0 * s)).abs() < 1e-9, "a CFrame offset of one stud: {pos:?}");
        let ws = dm.lock().find_service("Workspace").unwrap();
        assert!((metres(&dm, ws, "Gravity") - 196.2 * s).abs() < 1e-9, "gravity is an acceleration");
    }

    #[test]
    fn the_spatial_api_takes_and_gives_studs() {
        let (mut vm, dm, _, roblox, native) = session();
        let crate_part = part(&dm);
        {
            // GetBoundingBox is a Model's, as in Roblox: the crate goes in one.
            let mut g = dm.lock();
            let ws = g.get_service("Workspace").unwrap();
            let stack = g.create("Model");
            g.set_prop(stack, "Name", DmValue::String("Stack".into())).unwrap();
            g.set_parent(stack, Some(ws)).unwrap();
            g.set_parent(crate_part, Some(stack)).unwrap();
        }
        let s = stud_metres();
        // A ray from 20 studs up, 100 studs long, straight down to the ground.
        run_with(&mut vm, &dm, roblox, "local r = workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -100, 0))\n\
            print(string.format('roblox %.4f %.4f', r.Distance, r.Position.Y))\n\
            local cf, size = workspace.Stack:GetBoundingBox()\n\
            print(string.format('box %.4f', size.X))\n\
            workspace.Stack.Crate:PivotTo(CFrame.new(0, 5, 0))", &ground);
        run_with(&mut vm, &dm, native, "local r = workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -100, 0))\n\
            print(string.format('native %.4f', r.Distance))", &ground);
        let out = printed(&dm);
        assert!(out.contains(&"roblox 20.0000 0.0000".to_string()), "the distance comes back in studs: {out:?}");
        assert!(out.contains(&"native 20.0000".to_string()), "{out:?}");
        assert!(out.contains(&format!("box {:.4}", 4.0 / s)), "{out:?}");
        let g = dm.lock();
        let pos = g.get_prop(crate_part, "Position").and_then(|v| v.as_vector3()).unwrap();
        assert!((pos.y - 5.0 * s).abs() < 1e-6, "PivotTo five studs up: {pos:?}");
    }

    #[test]
    fn the_mouse_hands_a_roblox_script_studs() {
        let (mut vm, dm, _, roblox, _) = session();
        {
            let mut g = dm.lock();
            let players = g.get_service("Players").unwrap();
            let me = g.create_virtual("Player", "Tester", Some(players));
            g.local_player = Some(me);
            g.mouse.has_hit = true;
            g.mouse.hit_position = Vector3::new(0.0, 2.8, 0.0);
            g.mouse.ray_origin = Vector3::new(0.0, 5.6, 0.0);
            g.mouse.ray_direction = Vector3::new(0.0, -1.0, 0.0);
        }
        let s = stud_metres();
        run(&mut vm, &dm, roblox, "local mouse = game:GetService('Players'):GetPlayers()[1]:GetMouse()\n\
            print(string.format('hit %.4f origin %.4f', mouse.Hit.Position.Y, mouse.UnitRay.Origin.Y))");
        let out = printed(&dm);
        assert!(out.contains(&format!("hit {:.4} origin {:.4}", 2.8 / s, 5.6 / s)), "{out:?}");
    }

    #[test]
    fn humanoid_speeds_reach_a_roblox_handler_in_studs() {
        let (mut vm, dm, hum, roblox, native) = session();
        run(&mut vm, &dm, roblox, "workspace.Humanoid.Running:Connect(function(v) print(string.format('roblox %.4f', v)) end)");
        run(&mut vm, &dm, native, "workspace.Humanoid.Running:Connect(function(v) print(string.format('native %.4f', v)) end)");
        let fire: mlua::Function = vm.host.get("fireSignal").unwrap();
        fire.call::<()>((instance::id_key(hum), "Running", 4.48)).unwrap();
        let out = printed(&dm);
        assert!(out.contains(&format!("roblox {:.4}", 4.48 / stud_metres())), "{out:?}");
        assert!(out.contains(&"native 4.4800".to_string()), "{out:?}");
    }

    #[test]
    fn a_tween_a_roblox_script_starts_converts_too() {
        // The tween writes from the prelude, on no script's thread.
        let (mut vm, dm, hum, roblox, _) = session();
        run(&mut vm, &dm, roblox, "local TS = game:GetService('TweenService')\n\
            TS:Create(workspace.Humanoid, TweenInfo.new(0), {WalkSpeed = 20}):Play()");
        // A test tree's clock stands still; a tween needs time to pass.
        dm.lock().frame.dt = 0.1;
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        for _ in 0..3 {
            vm.frame(&no_rays, no_terrain);
        }
        assert!((metres(&dm, hum, "WalkSpeed") - 20.0 * stud_metres()).abs() < 1e-6, "{}", metres(&dm, hum, "WalkSpeed"));
    }
}
