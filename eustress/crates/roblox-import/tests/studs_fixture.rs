//! An imported place lines up with its geometry.
//!
//! A small place (a floor, a door, a crate and a script) goes through the
//! real importer into a Space, the Player's reader reads it back, and its
//! scripts run in a Play VM. The imported script is written for Roblox, so it
//! works in studs; the tree is metres. Whatever the script does in studs must
//! land where the same numbers put it on Roblox, scaled by the one stud,
//! `Unit::Stud`:
//!
//! - it raises the door 10 studs, and the door rises exactly 10 studs of
//!   metres;
//! - it casts a ray from the door's bottom face straight down, and the
//!   distance it reads is the gap the place was built with, in studs;
//! - it rests the crate on the floor from `floor.Position.Y +
//!   floor.Size.Y / 2 + crate.Size.Y / 2`, and the crate neither sinks nor
//!   floats (the gap is under 1 mm);
//! - a native script in the same place reads the door in metres.

use std::path::{Path, PathBuf};

use eustress_common::datamodel::{new_shared, InstanceId, OutputLevel, SharedDataModel};
use eustress_common::luau::play::{PlayLuau, RayHit, RayQuery, ScriptLaunch, TerrainReadFn};
use eustress_common::luau::play::studs::{ORIGIN_PROPERTY, ROBLOX_ORIGIN};
use eustress_common::scripting::Vector3;
use eustress_common::units::Unit;
use eustress_roblox_import::parser::{RobloxDom, RobloxFormat};
use eustress_roblox_import::{import_into_space, ImportOptions};
use rbx_dom_weak::types as rbx;
use rbx_dom_weak::{InstanceBuilder, WeakDom};

/// The place, in studs, as a Roblox builder would lay it out.
const FLOOR_Y: f64 = 0.0;
const FLOOR_SIZE: [f64; 3] = [40.0, 2.0, 40.0];
const DOOR_Y: f64 = 10.0;
const DOOR_SIZE: [f64; 3] = [4.0, 8.0, 1.0];
const CRATE_AT: [f64; 3] = [10.0, 20.0, 0.0];
const CRATE_SIZE: [f64; 3] = [2.0, 2.0, 2.0];

/// The gap under the door as built, in studs: the door's bottom face above
/// the floor's top face.
fn built_gap() -> f64 {
    (DOOR_Y - DOOR_SIZE[1] / 2.0) - (FLOOR_Y + FLOOR_SIZE[1] / 2.0)
}

const ROBLOX_SCRIPT: &str = r#"
local floor = workspace:WaitForChild("Floor")
local door = workspace:WaitForChild("Door")
local crate = workspace:WaitForChild("Crate")

local bottom = door.Position - Vector3.new(0, door.Size.Y / 2, 0)
local hit = workspace:Raycast(bottom, Vector3.new(0, -100, 0))
print("gap", hit and hit.Distance)

crate.CFrame = CFrame.new(crate.Position.X, floor.Position.Y + floor.Size.Y / 2 + crate.Size.Y / 2, crate.Position.Z)

door.Position += Vector3.new(0, 10, 0)
print("raised")
"#;

const NATIVE_SCRIPT: &str = r#"
print("door", workspace.Door.Position.Y)
"#;

fn part(name: &str, at: [f64; 3], size: [f64; 3]) -> InstanceBuilder {
    let v = |a: [f64; 3]| rbx::Vector3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    InstanceBuilder::new("Part")
        .with_name(name)
        .with_property("Anchored", true)
        .with_property("Size", v(size))
        .with_property("CFrame", rbx::CFrame::new(v(at), rbx::Matrix3::identity()))
}

fn place() -> WeakDom {
    let workspace = InstanceBuilder::new("Workspace")
        .with_name("Workspace")
        .with_child(part("Floor", [0.0, FLOOR_Y, 0.0], FLOOR_SIZE))
        .with_child(part("Door", [0.0, DOOR_Y, 0.0], DOOR_SIZE))
        .with_child(part("Crate", CRATE_AT, CRATE_SIZE));
    let scripts = InstanceBuilder::new("ServerScriptService").with_name("ServerScriptService").with_child(
        InstanceBuilder::new("Script").with_name("Fixture").with_property("Source", ROBLOX_SCRIPT.to_string()),
    );
    WeakDom::new(InstanceBuilder::new("DataModel").with_child(workspace).with_child(scripts))
}

/// A fresh folder for the Space, gone again when the test ends.
struct TempSpace(PathBuf);

impl TempSpace {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("eustress-studs-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp folder");
        Self(dir)
    }
}

impl Drop for TempSpace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn child(dm: &SharedDataModel, parent: InstanceId, name: &str) -> InstanceId {
    dm.lock().find_first_child(parent, name, false).unwrap_or_else(|| panic!("{name} was imported"))
}

fn position(dm: &SharedDataModel, id: InstanceId) -> Vector3 {
    dm.lock().get_prop(id, "CFrame").and_then(|v| v.as_cframe()).expect("a CFrame").position
}

fn size(dm: &SharedDataModel, id: InstanceId) -> Vector3 {
    dm.lock().get_prop(id, "Size").and_then(|v| v.as_vector3()).expect("a Size")
}

/// A ray against the parts' boxes (the fixture's parts are unrotated), in
/// metres, skipping any part the ray starts on or in.
fn cast(boxes: &[(u64, Vector3, Vector3)], q: &RayQuery) -> Option<RayHit> {
    let (o, d) = (q.origin, q.direction);
    let mut best: Option<RayHit> = None;
    for &(entity, at, size) in boxes {
        let lo = [at.x - size.x / 2.0, at.y - size.y / 2.0, at.z - size.z / 2.0];
        let hi = [at.x + size.x / 2.0, at.y + size.y / 2.0, at.z + size.z / 2.0];
        let (oa, da) = ([o.x, o.y, o.z], [d.x, d.y, d.z]);
        if (0..3).all(|i| oa[i] >= lo[i] - 1e-9 && oa[i] <= hi[i] + 1e-9) {
            continue;
        }
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut normal = [0.0; 3];
        let mut ok = true;
        for i in 0..3 {
            if da[i].abs() < 1e-12 {
                if oa[i] < lo[i] || oa[i] > hi[i] {
                    ok = false;
                }
                continue;
            }
            let (a, b) = ((lo[i] - oa[i]) / da[i], (hi[i] - oa[i]) / da[i]);
            let (near, far) = if a < b { (a, b) } else { (b, a) };
            if near > t0 {
                t0 = near;
                normal = [0.0; 3];
                normal[i] = -da[i].signum();
            }
            t1 = t1.min(far);
        }
        if !ok || t0 > t1 {
            continue;
        }
        let distance = t0 * (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        if best.as_ref().map_or(true, |b| distance < b.distance) {
            best = Some(RayHit {
                entity,
                position: Vector3::new(o.x + d.x * t0, o.y + d.y * t0, o.z + d.z * t0),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                distance,
                terrain: false,
            });
        }
    }
    best
}

fn printed(dm: &SharedDataModel, word: &str) -> Option<f64> {
    dm.lock()
        .output
        .iter()
        .find_map(|l| l.text.strip_prefix(word).and_then(|rest| rest.trim().parse().ok()))
}

#[test]
fn an_imported_place_lines_up_with_its_geometry() {
    let space = TempSpace::new();
    let dom = RobloxDom::from_dom(place(), RobloxFormat::BinaryPlace, PathBuf::from("studs_fixture.rbxl"));
    import_into_space(&dom, &space.0, ImportOptions::default()).expect("the place imports");
    let tree = eustress_common::tree_read::read_space_dir(Path::new(&space.0)).expect("the Space reads");

    let dm = new_shared();
    *dm.lock() = tree.dm;
    let stud = Unit::Stud.to_meters();
    let (workspace, service) = {
        let mut g = dm.lock();
        (g.get_service("Workspace").expect("Workspace"), g.get_service("ServerScriptService").expect("ServerScriptService"))
    };
    let (floor, door, crate_) = (child(&dm, workspace, "Floor"), child(&dm, workspace, "Door"), child(&dm, workspace, "Crate"));

    // The importer placed the geometry at the stud's scale.
    let door_before = position(&dm, door);
    assert!((door_before.y - DOOR_Y * stud).abs() < 1e-6, "the door sits at {} m", door_before.y);
    assert!((size(&dm, floor).y - FLOOR_SIZE[1] * stud).abs() < 1e-6, "the floor is {} m thick", size(&dm, floor).y);

    // The imported script is marked as written for Roblox.
    let fixture = child(&dm, service, "Fixture");
    let (source, origin) = {
        let g = dm.lock();
        let source = g.get_prop(fixture, "Source").and_then(|v| v.as_str().map(str::to_string)).expect("a Source");
        let origin = g.get_prop(fixture, ORIGIN_PROPERTY).and_then(|v| v.as_str().map(str::to_string));
        (source, origin)
    };
    assert_eq!(origin.as_deref(), Some(ROBLOX_ORIGIN), "the importer marks the script as written for Roblox");

    // A native script beside it, written in metres.
    let native = dm.lock().create_virtual("Script", "Native", Some(service));

    // Bind each part to an entity, so a hit names its instance.
    let boxes: Vec<(u64, Vector3, Vector3)> = {
        let mut g = dm.lock();
        [floor, door, crate_]
            .into_iter()
            .map(|id| {
                g.bind_entity(id, id.0);
                (id.0, id)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(bits, id)| {
                let at = g.get_prop(id, "CFrame").and_then(|v| v.as_cframe()).expect("a CFrame").position;
                let size = g.get_prop(id, "Size").and_then(|v| v.as_vector3()).expect("a Size");
                (bits, at, size)
            })
            .collect()
    };

    let mut vm = PlayLuau::new(dm.clone()).expect("the Play VM starts");
    let rays = |q: &RayQuery| cast(&boxes, q);
    let no_terrain: &TerrainReadFn<'_> = &|_| {};
    vm.run_scripts(
        vec![
            ScriptLaunch { instance: fixture, source, chunk_name: "ServerScriptService.Fixture".into() },
            ScriptLaunch { instance: native, source: NATIVE_SCRIPT.into(), chunk_name: "ServerScriptService.Native".into() },
        ],
        &rays,
        no_terrain,
    );
    let errors: Vec<String> =
        dm.lock().output.iter().filter(|l| l.level == OutputLevel::Error).map(|l| l.text.clone()).collect();
    assert!(errors.is_empty(), "the scripts ran clean: {errors:?}");

    // The ray reads the gap as built, in studs.
    let gap = printed(&dm, "gap").expect("the script printed the gap");
    assert!((gap - built_gap()).abs() < 1e-6, "the ray reads {gap} studs, built {}", built_gap());

    // The door rose exactly 10 studs.
    let door_after = position(&dm, door);
    assert!(
        (door_after.y - door_before.y - 10.0 * stud).abs() < 1e-9,
        "the door rose {} m, 10 studs is {} m",
        door_after.y - door_before.y,
        10.0 * stud
    );

    // The crate rests on the floor: no overlap, no gap of a millimetre.
    let floor_top = position(&dm, floor).y + size(&dm, floor).y / 2.0;
    let crate_bottom = position(&dm, crate_).y - size(&dm, crate_).y / 2.0;
    assert!((crate_bottom - floor_top).abs() < 1e-3, "the crate's bottom is {} m off the floor", crate_bottom - floor_top);

    // The native script reads the same door in metres.
    let native_y = printed(&dm, "door").expect("the native script printed the door");
    assert!((native_y - door_after.y).abs() < 1e-9, "the native script reads {native_y} m, the tree holds {} m", door_after.y);
}
