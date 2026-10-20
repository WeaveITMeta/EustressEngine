//! `workspace.Terrain`: Roblox's terrain API over the Space's terrain.
//!
//! The Terrain is one instance of class `Terrain` under Workspace, set up by
//! [`ensure_terrain_instance`] when the session is seeded. It has no entity
//! of its own: the Space's terrain root draws and collides the ground, and
//! the instance is the scripts' handle on it. As in Roblox, its Parent is
//! locked, so it can be neither moved nor destroyed, and it can be neither
//! cloned nor made with `Instance.new`.
//!
//! ## Units
//!
//! Positions, sizes and every `resolution` argument are world units
//! (metres), like every other position a script sees. Roblox scripts pass 4
//! (studs) as the voxel resolution; any positive resolution works.
//! `ReadVoxels` and `WriteVoxels` lay their voxels out from the region's low
//! corner, a region that is not a whole number of voxels rounding up. The
//! grid of `WorldToCell` and `CellCenterToWorld` is the terrain's volume
//! lattice ([`lattice_cell_size`]): cell `n` spans `n * cell` to
//! `(n + 1) * cell` on each axis.
//!
//! ## Writes
//!
//! `FillBall`, `FillBlock`, `FillCylinder`, `FillRegion`, `ReplaceMaterial`,
//! `WriteVoxels` and `Clear` check their arguments at the call, so a bad one
//! raises there, and queue a [`TerrainCommand`] on the DataModel. The engine
//! applies the queue after the frame's scripts have run, and reports a
//! command it refuses in the Output, since the call has returned by then (a
//! Roblox terrain write does not report back either). A fill of zero or
//! negative size changes nothing and queues nothing. `FillRegion` and
//! `ReplaceMaterial` check their `resolution` and use the region as given.
//! `ReplaceMaterial` from Air to Water queues a `FillWater` and from Water to
//! Air a `DrainWater` (Roblox's Sea Level pairs, which touch water alone);
//! any other pair with Air or Water raises.
//! Without a terrain root every write raises "this Space has no terrain".
//! `SetMaterialColor` checks its arguments and leaves the colour as it is
//! (colours come from the Space's material slots), warning once a session.
//!
//! ## Reads
//!
//! `ReadVoxels`, the cell conversions, `GetMaterialColor` and the material
//! of a raycast that hits the ground read the live terrain through the
//! engine's reader ([`with_terrain_reader`]), as it stood when the frame's
//! scripts began: a write queued earlier in the same frame reads back from
//! the next frame on. Without a terrain root every voxel reads as empty Air,
//! the cells are those of a default terrain and material colours are the
//! built-in ones.

use std::cell::Cell;

use bevy::math::{Quat, UVec3, Vec3};
use mlua::{Lua, Result as LuaResult, Table, Value};

use crate::datamodel::{DataModel, DmValue, EnumItem, InstanceId, OutputLevel};
use crate::luau::types::{LuauCFrame, LuauColor3, LuauVector3, UserDataPeek};
use crate::scripting::{CFrame, Color3, Vector3};
use crate::terrain::api::{read_voxels, TerrainCommand, TerrainFill, MAX_SCRIPT_VOXELS};
use crate::terrain::{
    lattice_cell_size, material_at, material_at_world, TerrainConfig, TerrainData, TerrainMaterial,
    TerrainMaterialSlots, TerrainSlotPalette, TerrainVolume, TerrainVoxelWater,
};

use super::convert::enum_item;
use super::instance::{with_dm, LInst};
use super::types_ext::{LuauEnumItem, LuauRegion3};

// ============================================================================
// The Terrain instance
// ============================================================================

/// Give `workspace` its Terrain, once: the Terrain instance the Space seeded
/// (an imported place's) or a new virtual one, which sits at the origin,
/// `Anchored` and `Locked` like Roblox's. Either way it becomes Workspace's
/// first child, so `workspace.Terrain` finds it before any user folder of
/// that name; its Parent is locked, so it can be neither moved nor
/// destroyed; and it is not archivable, so it cannot be cloned. Returns its
/// id.
pub fn ensure_terrain_instance(dm: &mut DataModel, workspace: InstanceId) -> InstanceId {
    let id = match dm.find_first_child_of_class(workspace, "Terrain", false) {
        Some(existing) => existing,
        None => {
            let id = dm.create_virtual("Terrain", "Terrain", Some(workspace));
            if let Some(terrain) = dm.get_mut(id) {
                terrain.props.insert("CFrame".into(), DmValue::CFrame(CFrame::IDENTITY));
                terrain.props.insert("Anchored".into(), DmValue::Bool(true));
                terrain.props.insert("Locked".into(), DmValue::Bool(true));
            }
            id
        }
    };
    if let Some(terrain) = dm.get_mut(id) {
        terrain.parent_locked = true;
        terrain.archivable = false;
    }
    let moved = match dm.get_mut(workspace) {
        Some(ws) if ws.children.first() != Some(&id) => {
            ws.children.retain(|c| *c != id);
            ws.children.insert(0, id);
            true
        }
        _ => false,
    };
    if moved {
        dm.structure_version += 1;
    }
    id
}

/// The Terrain under Workspace, if the session has one.
pub fn terrain_instance(dm: &DataModel) -> Option<InstanceId> {
    let workspace = dm.find_service("Workspace")?;
    dm.find_first_child_of_class(workspace, "Terrain", false)
}

// ============================================================================
// Live terrain reads through the engine
// ============================================================================

/// The live terrain, as a script reads it.
#[derive(Clone, Copy)]
pub struct TerrainView<'t> {
    pub config: &'t TerrainConfig,
    /// The ground every reader sees: the bake when the root has layers
    /// (`terrain::surface_data`).
    pub data: &'t TerrainData,
    pub volume: &'t TerrainVolume,
    pub water: Option<&'t TerrainVoxelWater>,
    /// The material slot table, which names the built-in material a Space's
    /// custom slot starts from.
    pub slots: Option<&'t TerrainMaterialSlots>,
}

/// The engine's terrain reader for the current frame. It calls `visit` once
/// with the Space's terrain, or never when the Space has none. It borrows the
/// frame's terrain queries, hence the lifetime.
pub type TerrainReadFn<'a> = dyn Fn(&mut dyn for<'t> FnMut(TerrainView<'t>)) + 'a;

thread_local! {
    static TERRAIN_READER: Cell<Option<*const TerrainReadFn<'static>>> = const { Cell::new(None) };
}

/// Make `f` the terrain reader for everything run inside `body`. Every entry
/// into the VM goes through this, so the Terrain's reads answer against the
/// live terrain, synchronously.
pub fn with_terrain_reader<R>(f: &TerrainReadFn<'_>, body: impl FnOnce() -> R) -> R {
    struct Reset(Option<*const TerrainReadFn<'static>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TERRAIN_READER.with(|c| c.set(self.0));
        }
    }
    // SAFETY: the pointer is only dereferenced while `body` runs, and the
    // guard restores the previous value before `f` goes out of scope, even
    // if `body` unwinds.
    let erased: *const TerrainReadFn<'static> =
        unsafe { std::mem::transmute::<&TerrainReadFn<'_>, &'static TerrainReadFn<'static>>(f) };
    let prev = TERRAIN_READER.with(|c| c.replace(Some(erased)));
    let _reset = Reset(prev);
    body()
}

/// Run `f` on the live terrain. `None` when the Space has no terrain, or
/// outside a script run.
pub fn read_terrain<R>(f: impl FnOnce(TerrainView<'_>) -> R) -> Option<R> {
    let reader = TERRAIN_READER.with(|c| c.get())?;
    // SAFETY: see `with_terrain_reader`.
    let reader: &TerrainReadFn<'static> = unsafe { &*reader };
    let mut f = Some(f);
    let mut out = None;
    reader(&mut |view: TerrainView<'_>| {
        if let Some(f) = f.take() {
            out = Some(f(view));
        }
    });
    out
}

/// The material of the ground at `p`, a point on the terrain's surface (a
/// raycast hit), as an `Enum.Material` value: the material an edit wrote
/// nearest the point where there is one, else the material map's cell under
/// it.
pub fn ground_material(p: Vector3) -> DmValue {
    let material = read_terrain(|t| surface_material(t, p.to_vec3())).unwrap_or(TerrainMaterial::Grass);
    DmValue::Enum(EnumItem::new("Material", material.name()))
}

fn surface_material(t: TerrainView<'_>, p: Vec3) -> TerrainMaterial {
    if let Some(m) = material_at(t.config, t.volume, p) {
        return m;
    }
    // No material layer (procedural terrain) reads as grass.
    let Some(sample) = material_at_world(t.config, t.data, p.x, p.z) else {
        return TerrainMaterial::Grass;
    };
    TerrainMaterial::from_u8(sample.primary)
        .or_else(|| t.slots.and_then(|slots| slots.get(sample.primary)).map(|slot| slot.base))
        .unwrap_or(TerrainMaterial::Grass)
}

/// Edge of one terrain grid cell in world units: the volume lattice spacing
/// of the Space's terrain, or of a default terrain without one.
fn cell_size() -> f64 {
    let cell = read_terrain(|t| lattice_cell_size(t.config)).unwrap_or_else(|| lattice_cell_size(&TerrainConfig::default()));
    f64::from(cell)
}

// ============================================================================
// Methods
// ============================================================================

/// Add the Terrain's methods to the shared method table
/// (`instance::method_applies` exposes them on class `Terrain` only).
/// `Clear` is shared with ParticleEmitter and dispatches in `instance`.
pub(super) fn install_methods(lua: &Lua, t: &Table) -> LuaResult<()> {
    t.raw_set(
        "FillBall",
        lua.create_function(|lua, (_this, center, radius, material): (LInst, LuauVector3, f64, Value)| {
            let fill = material_arg("FillBall", "material", &material)?;
            let center = vector_arg("FillBall", "center", center.0)?;
            let radius = number_arg("FillBall", "radius", radius)?;
            if radius <= 0.0 {
                return Ok(());
            }
            queue(lua, "FillBall", TerrainCommand::FillBall { center, radius, fill })
        })?,
    )?;
    t.raw_set(
        "FillBlock",
        lua.create_function(|lua, (_this, cframe, size, material): (LInst, LuauCFrame, LuauVector3, Value)| {
            let fill = material_arg("FillBlock", "material", &material)?;
            let (center, rotation) = pose_arg("FillBlock", &cframe.0)?;
            let size = vector_arg("FillBlock", "size", size.0)?;
            if size.min_element() <= 0.0 {
                return Ok(());
            }
            queue(lua, "FillBlock", TerrainCommand::FillBlock { center, rotation, size, fill })
        })?,
    )?;
    // Cylinders run along the CFrame's Y axis.
    t.raw_set(
        "FillCylinder",
        lua.create_function(
            |lua, (_this, cframe, height, radius, material): (LInst, LuauCFrame, f64, f64, Value)| {
                let fill = material_arg("FillCylinder", "material", &material)?;
                let (center, rotation) = pose_arg("FillCylinder", &cframe.0)?;
                let height = number_arg("FillCylinder", "height", height)?;
                let radius = number_arg("FillCylinder", "radius", radius)?;
                if height <= 0.0 || radius <= 0.0 {
                    return Ok(());
                }
                queue(lua, "FillCylinder", TerrainCommand::FillCylinder { center, rotation, height, radius, fill })
            },
        )?,
    )?;
    t.raw_set(
        "FillRegion",
        lua.create_function(|lua, (_this, region, resolution, material): (LInst, LuauRegion3, f64, Value)| {
            let fill = material_arg("FillRegion", "material", &material)?;
            resolution_arg("FillRegion", resolution)?;
            let (min, max) = region_arg("FillRegion", &region)?;
            if (max - min).min_element() <= 0.0 {
                return Ok(());
            }
            queue(lua, "FillRegion", TerrainCommand::FillRegion { min, max, fill })
        })?,
    )?;
    // Roblox's two Sea Level pairs touch water alone: Air to Water fills the
    // air under the region's top with water (FillWater), Water to Air drains
    // the region (DrainWater), and neither carves the ground. Every other
    // pair must be solid ground on both sides: Air and Water raise, as in
    // Roblox.
    t.raw_set(
        "ReplaceMaterial",
        lua.create_function(
            |lua, (_this, region, resolution, source, target): (LInst, LuauRegion3, f64, Value, Value)| {
                let sea_level = match (
                    material_arg("ReplaceMaterial", "sourceMaterial", &source),
                    material_arg("ReplaceMaterial", "targetMaterial", &target),
                ) {
                    (Ok(TerrainFill::Air), Ok(TerrainFill::Water)) => Some(true),
                    (Ok(TerrainFill::Water), Ok(TerrainFill::Air)) => Some(false),
                    _ => None,
                };
                if let Some(fill) = sea_level {
                    resolution_arg("ReplaceMaterial", resolution)?;
                    let (min, max) = region_arg("ReplaceMaterial", &region)?;
                    if (max - min).min_element() <= 0.0 {
                        return Ok(());
                    }
                    let command = if fill {
                        TerrainCommand::FillWater { min, max }
                    } else {
                        TerrainCommand::DrainWater { min, max }
                    };
                    return queue(lua, "ReplaceMaterial", command);
                }
                let from = solid_material_arg("ReplaceMaterial", "sourceMaterial", &source)?;
                let to = solid_material_arg("ReplaceMaterial", "targetMaterial", &target)?;
                resolution_arg("ReplaceMaterial", resolution)?;
                let (min, max) = region_arg("ReplaceMaterial", &region)?;
                if (max - min).min_element() <= 0.0 {
                    return Ok(());
                }
                queue(lua, "ReplaceMaterial", TerrainCommand::ReplaceMaterial { min, max, from, to })
            },
        )?,
    )?;
    // `(materials, occupancies)`: `[x][y][z]` tables from 1, each with the
    // voxel counts as `Size`; materials are Enum.Material items (Air, Water
    // or the ground's), occupancies 0 to 1.
    t.raw_set(
        "ReadVoxels",
        lua.create_function(|lua, (_this, region, resolution): (LInst, LuauRegion3, f64)| {
            let grid = voxel_grid("ReadVoxels", &region, resolution)?;
            let read = if grid.count() == 0 {
                None
            } else {
                read_terrain(|t| read_voxels(t.config, t.data, t.volume, t.water, grid.min, grid.resolution, grid.size))
            };
            let (fills, occupancies) = read.unwrap_or_default();
            voxel_tables(lua, grid.size, &fills, &occupancies)
        })?,
    )?;
    // The same `[x][y][z]` shape as ReadVoxels, every level exactly as long
    // as the region is in voxels.
    t.raw_set(
        "WriteVoxels",
        lua.create_function(
            |lua, (_this, region, resolution, materials, occupancies): (LInst, LuauRegion3, f64, Table, Table)| {
                let grid = voxel_grid("WriteVoxels", &region, resolution)?;
                let count = grid.count();
                let mut fills = vec![TerrainFill::Air; count];
                let mut occupied = vec![0.0f32; count];
                // Enum items and strings are interned, so each distinct
                // material value is parsed once.
                let mut parsed: Vec<(*const std::ffi::c_void, TerrainFill)> = Vec::new();
                for_each_voxel("WriteVoxels", "materials", &materials, grid.size, |i, value| {
                    let key = value.to_pointer();
                    let known = if key.is_null() { None } else { parsed.iter().find(|(k, _)| *k == key) };
                    let fill = match known {
                        Some((_, fill)) => *fill,
                        None => {
                            let fill = material_arg("WriteVoxels", "materials", &value)?;
                            if !key.is_null() {
                                parsed.push((key, fill));
                            }
                            fill
                        }
                    };
                    fills[i] = fill;
                    Ok(())
                })?;
                for_each_voxel("WriteVoxels", "occupancies", &occupancies, grid.size, |i, value| {
                    let n = match value {
                        Value::Number(n) => n,
                        Value::Integer(n) => n as f64,
                        other => {
                            return Err(rt(format!(
                                "WriteVoxels: occupancies must hold numbers, got {}",
                                other.type_name()
                            )))
                        }
                    };
                    if !n.is_finite() {
                        return Err(rt("WriteVoxels: occupancies must be finite numbers".into()));
                    }
                    occupied[i] = n.clamp(0.0, 1.0) as f32;
                    Ok(())
                })?;
                if count == 0 {
                    return Ok(());
                }
                queue(
                    lua,
                    "WriteVoxels",
                    TerrainCommand::WriteVoxels {
                        min: grid.min,
                        resolution: grid.resolution,
                        size: grid.size,
                        materials: fills,
                        occupancies: occupied,
                    },
                )
            },
        )?,
    )?;
    // Cell indices are whole numbers; a fraction drops to the cell below.
    t.raw_set(
        "CellCenterToWorld",
        lua.create_function(|_, (_this, x, y, z): (LInst, f64, f64, f64)| {
            let cell = cell_size();
            let center = |n: f64| (n.floor() + 0.5) * cell;
            Ok(LuauVector3(Vector3::new(center(x), center(y), center(z))))
        })?,
    )?;
    t.raw_set(
        "CellCornerToWorld",
        lua.create_function(|_, (_this, x, y, z): (LInst, f64, f64, f64)| {
            let cell = cell_size();
            let corner = |n: f64| n.floor() * cell;
            Ok(LuauVector3(Vector3::new(corner(x), corner(y), corner(z))))
        })?,
    )?;
    t.raw_set(
        "WorldToCell",
        lua.create_function(|_, (_this, position): (LInst, LuauVector3)| {
            Ok(LuauVector3(world_to_cell(position.0, cell_size())))
        })?,
    )?;
    t.raw_set(
        "WorldToCellPreferSolid",
        lua.create_function(|_, (_this, position): (LInst, LuauVector3)| {
            Ok(LuauVector3(world_to_cell_preferring(position.0, true)))
        })?,
    )?;
    t.raw_set(
        "WorldToCellPreferEmpty",
        lua.create_function(|_, (_this, position): (LInst, LuauVector3)| {
            Ok(LuauVector3(world_to_cell_preferring(position.0, false)))
        })?,
    )?;
    // The material's swatch: the colour its slot reads as from a distance.
    t.raw_set(
        "GetMaterialColor",
        lua.create_function(|_, (_this, material): (LInst, Value)| {
            let slot = match material_arg("GetMaterialColor", "material", &material)? {
                TerrainFill::Material(m) => m.to_u8(),
                TerrainFill::Water => TerrainMaterial::Water.to_u8(),
                TerrainFill::Air => return Err(rt("GetMaterialColor: Air has no colour".into())),
            };
            let [r, g, b] = read_terrain(|t| t.data.slot_palette.srgb(slot))
                .unwrap_or_else(|| TerrainSlotPalette::default().srgb(slot));
            Ok(LuauColor3(Color3::new(f64::from(r), f64::from(g), f64::from(b))))
        })?,
    )?;
    // Material colours come from the Space's material slots and its imported
    // colours, which a script does not change: the call checks its arguments,
    // leaves the colour as it is and says so once per session, so a ported
    // script that themes its terrain at startup keeps running.
    t.raw_set(
        "SetMaterialColor",
        lua.create_function(|lua, (_this, material, _color): (LInst, Value, LuauColor3)| {
            if let TerrainFill::Air = material_arg("SetMaterialColor", "material", &material)? {
                return Err(rt("SetMaterialColor: Air has no colour".into()));
            }
            const WARNED: &str = "eustress.terrain.set_material_color_warned";
            if !lua.named_registry_value::<bool>(WARNED).unwrap_or(false) {
                lua.set_named_registry_value(WARNED, true)?;
                with_dm(lua, |dm| {
                    dm.print(
                        OutputLevel::Warn,
                        "Terrain",
                        "Terrain:SetMaterialColor leaves colours unchanged; set them in the Space's terrain materials",
                    )
                })?;
            }
            Ok(())
        })?,
    )?;
    Ok(())
}

/// `Terrain:Clear()`: queue emptying the whole terrain.
pub(super) fn clear(lua: &Lua) -> LuaResult<()> {
    queue(lua, "Clear", TerrainCommand::Clear)
}

/// Queue `command` for the engine, or raise when the Space has no terrain.
fn queue(lua: &Lua, method: &str, command: TerrainCommand) -> LuaResult<()> {
    if read_terrain(|_| ()).is_none() {
        return Err(rt(format!("{method}: this Space has no terrain")));
    }
    with_dm(lua, |dm| dm.terrain_commands.push(command))
}

fn world_to_cell(p: Vector3, cell: f64) -> Vector3 {
    Vector3::new((p.x / cell).floor(), (p.y / cell).floor(), (p.z / cell).floor())
}

/// `WorldToCellPreferSolid` / `WorldToCellPreferEmpty`: the cell holding `p`,
/// or, when `p` lies on a cell boundary, the first of the cells meeting there
/// that is solid (or empty) as wanted. A cell is solid when its voxel reads
/// as anything but Air.
fn world_to_cell_preferring(p: Vector3, solid: bool) -> Vector3 {
    let cell = cell_size();
    let base = world_to_cell(p, cell);
    // Per axis: the cell holding the coordinate, and on a boundary also the
    // cell before it.
    let options = |v: f64, floor: f64| -> Vec<f64> {
        let g = v / cell;
        if (g - g.round()).abs() <= 1e-6 * g.abs().max(1.0) {
            vec![g.round(), g.round() - 1.0]
        } else {
            vec![floor]
        }
    };
    let (xs, ys, zs) = (options(p.x, base.x), options(p.y, base.y), options(p.z, base.z));
    if xs.len() * ys.len() * zs.len() == 1 {
        return base;
    }
    let found = read_terrain(|t| {
        for &x in &xs {
            for &y in &ys {
                for &z in &zs {
                    let c = Vector3::new(x, y, z);
                    let (fills, _) =
                        read_voxels(t.config, t.data, t.volume, t.water, (c * cell).to_vec3(), cell as f32, UVec3::ONE);
                    let is_solid = fills.first().map_or(false, |f| *f != TerrainFill::Air);
                    if is_solid == solid {
                        return Some(c);
                    }
                }
            }
        }
        None
    });
    found.flatten().unwrap_or(base)
}

// ============================================================================
// Voxel grids
// ============================================================================

/// The voxels a region holds at a resolution: `size` voxels of `resolution`
/// world units from `min`.
struct VoxelGrid {
    min: Vec3,
    resolution: f32,
    size: UVec3,
}

impl VoxelGrid {
    fn count(&self) -> usize {
        self.size.x as usize * self.size.y as usize * self.size.z as usize
    }
}

/// The voxel grid of `region` at `resolution`, rounded up to whole voxels.
/// Refused past [`MAX_SCRIPT_VOXELS`], an empty axis counting as one voxel
/// (it still costs a table per row).
fn voxel_grid(method: &str, region: &LuauRegion3, resolution: f64) -> LuaResult<VoxelGrid> {
    let resolution = resolution_arg(method, resolution)?;
    let (min, max) = region_arg(method, region)?;
    let step = f64::from(resolution);
    // A hair under a whole voxel is float noise, not another voxel.
    let voxels = |extent: f32| (f64::from(extent) / step - 1e-6).ceil().max(0.0);
    let extent = max - min;
    let (x, y, z) = (voxels(extent.x), voxels(extent.y), voxels(extent.z));
    let cost = x.max(1.0) * y.max(1.0) * z.max(1.0);
    if !(cost <= f64::from(MAX_SCRIPT_VOXELS)) {
        return Err(rt(format!(
            "{method}: a {x} x {y} x {z} voxel region is more than one call may touch ({MAX_SCRIPT_VOXELS} voxels)"
        )));
    }
    Ok(VoxelGrid { min, resolution, size: UVec3::new(x as u32, y as u32, z as u32) })
}

/// ReadVoxels' result. `fills` and `occupancies` are indexed
/// `x + size.x * (y + size.y * z)`; a missing entry reads as empty Air.
fn voxel_tables(lua: &Lua, size: UVec3, fills: &[TerrainFill], occupancies: &[f32]) -> LuaResult<(Table, Table)> {
    let (nx, ny, nz) = (size.x as usize, size.y as usize, size.z as usize);
    let materials = lua.create_table_with_capacity(nx, 1)?;
    let occupied = lua.create_table_with_capacity(nx, 1)?;
    // One enum item per material, looked up once.
    let mut items: Vec<(&'static str, Value)> = Vec::new();
    for x in 0..nx {
        let material_plane = lua.create_table_with_capacity(ny, 0)?;
        let occupancy_plane = lua.create_table_with_capacity(ny, 0)?;
        for y in 0..ny {
            let material_row = lua.create_table_with_capacity(nz, 0)?;
            let occupancy_row = lua.create_table_with_capacity(nz, 0)?;
            for z in 0..nz {
                let i = x + nx * (y + ny * z);
                let name = fills.get(i).copied().unwrap_or(TerrainFill::Air).name();
                let item = match items.iter().find(|(n, _)| *n == name) {
                    Some((_, item)) => item.clone(),
                    None => {
                        let item = enum_item(lua, &EnumItem::new("Material", name))?;
                        items.push((name, item.clone()));
                        item
                    }
                };
                material_row.raw_set(z + 1, item)?;
                occupancy_row.raw_set(z + 1, f64::from(occupancies.get(i).copied().unwrap_or(0.0)))?;
            }
            material_plane.raw_set(y + 1, material_row)?;
            occupancy_plane.raw_set(y + 1, occupancy_row)?;
        }
        materials.raw_set(x + 1, material_plane)?;
        occupied.raw_set(x + 1, occupancy_plane)?;
    }
    let counts = LuauVector3(Vector3::new(nx as f64, ny as f64, nz as f64));
    materials.raw_set("Size", counts)?;
    occupied.raw_set("Size", counts)?;
    Ok((materials, occupied))
}

/// Visit `table[x][y][z]` for every voxel of a grid of `size`, passing the
/// index `x + size.x * (y + size.y * z)`, after checking that every level
/// holds exactly as many entries as the grid has voxels along its axis.
fn for_each_voxel(
    method: &str,
    what: &str,
    table: &Table,
    size: UVec3,
    mut visit: impl FnMut(usize, Value) -> LuaResult<()>,
) -> LuaResult<()> {
    let (nx, ny, nz) = (size.x as usize, size.y as usize, size.z as usize);
    let level = |value: Value, axis: &str, len: usize| -> LuaResult<Table> {
        let Value::Table(entries) = value else {
            return Err(rt(format!("{method}: {what} must be nested tables indexed [x][y][z]")));
        };
        let found = entries.raw_len();
        if found != len {
            return Err(rt(format!("{method}: {what} has {found} entries along {axis}; the region holds {len}")));
        }
        Ok(entries)
    };
    let top = level(Value::Table(table.clone()), "X", nx)?;
    for x in 0..nx {
        let plane = level(top.raw_get(x + 1)?, "Y", ny)?;
        for y in 0..ny {
            let row = level(plane.raw_get(y + 1)?, "Z", nz)?;
            for z in 0..nz {
                visit(x + nx * (y + ny * z), row.raw_get(z + 1)?)?;
            }
        }
    }
    Ok(())
}

// ============================================================================
// Arguments
// ============================================================================

fn rt(message: String) -> mlua::Error {
    mlua::Error::RuntimeError(message)
}

/// A material argument: `Enum.Material.Rock`, `"Rock"` or
/// `"Enum.Material.Rock"`.
fn material_arg(method: &str, what: &str, value: &Value) -> LuaResult<TerrainFill> {
    let name = match value {
        Value::UserData(ud) => match ud.peek::<LuauEnumItem>() {
            Ok(item) if item.0.enum_type == "Material" || item.0.enum_type.is_empty() => item.0.name,
            Ok(item) => return Err(rt(format!("{method}: {what} must be an Enum.Material, got {}", item.0))),
            Err(_) => return Err(rt(format!("{method}: {what} must be an Enum.Material, got userdata"))),
        },
        Value::String(s) => s.to_str()?.to_string(),
        other => return Err(rt(format!("{method}: {what} must be an Enum.Material, got {}", other.type_name()))),
    };
    TerrainFill::from_material_name(&name).ok_or_else(|| rt(format!("{method}: {name} is not a terrain material")))
}

/// A material argument that must be solid ground (neither Air nor Water).
fn solid_material_arg(method: &str, what: &str, value: &Value) -> LuaResult<TerrainMaterial> {
    match material_arg(method, what, value)? {
        TerrainFill::Material(m) if m != TerrainMaterial::Water => Ok(m),
        other => Err(rt(format!("{method}: {what} must be a solid material, got {}", other.name()))),
    }
}

/// A finite number argument.
fn number_arg(method: &str, what: &str, value: f64) -> LuaResult<f32> {
    let n = value as f32;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(rt(format!("{method}: {what} must be a finite number")))
    }
}

/// A finite position or size argument.
fn vector_arg(method: &str, what: &str, value: Vector3) -> LuaResult<Vec3> {
    let v = value.to_vec3();
    if v.is_finite() {
        Ok(v)
    } else {
        Err(rt(format!("{method}: {what} must be finite")))
    }
}

/// A CFrame argument as its position and rotation.
fn pose_arg(method: &str, cframe: &CFrame) -> LuaResult<(Vec3, Quat)> {
    let pose = cframe.to_transform();
    if !(pose.translation.is_finite() && pose.rotation.is_finite() && pose.rotation.length_squared() > 1e-12) {
        return Err(rt(format!("{method}: cframe must be finite")));
    }
    Ok((pose.translation, pose.rotation.normalize()))
}

/// A region argument's corners.
fn region_arg(method: &str, region: &LuauRegion3) -> LuaResult<(Vec3, Vec3)> {
    Ok((vector_arg(method, "region", region.min)?, vector_arg(method, "region", region.max)?))
}

/// A voxel resolution: a positive number of world units.
fn resolution_arg(method: &str, resolution: f64) -> LuaResult<f32> {
    let r = resolution as f32;
    if r.is_finite() && r > 0.0 {
        Ok(r)
    } else {
        Err(rt(format!("{method}: resolution must be a positive number of world units")))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::{new_shared, OutputLevel, SharedDataModel};
    use crate::luau::play::{PlayLuau, RaycastFn, ScriptLaunch};

    /// A session tree (Workspace with its Terrain) and a VM bound to it.
    struct Session {
        dm: SharedDataModel,
        vm: PlayLuau,
        workspace: InstanceId,
        terrain: InstanceId,
    }

    fn session() -> Session {
        let dm = new_shared();
        let (workspace, terrain) = {
            let mut g = dm.lock();
            let workspace = g.get_service("Workspace").expect("Workspace");
            (workspace, ensure_terrain_instance(&mut g, workspace))
        };
        let vm = PlayLuau::new(dm.clone()).expect("the Play VM starts");
        Session { dm, vm, workspace, terrain }
    }

    impl Session {
        /// Run `source` as a server script, over a default terrain when
        /// `with_terrain`, and return what it printed and raised.
        fn run(&mut self, source: &str, with_terrain: bool) -> Vec<(OutputLevel, String)> {
            let script = {
                let mut g = self.dm.lock();
                let service = g.get_service("ServerScriptService").expect("ServerScriptService");
                g.create_virtual("Script", "TerrainTest", Some(service))
            };
            let config = TerrainConfig::default();
            let data = TerrainData::default();
            let volume = TerrainVolume::default();
            let reader: &TerrainReadFn<'_> = &|visit| {
                if with_terrain {
                    visit(TerrainView { config: &config, data: &data, volume: &volume, water: None, slots: None });
                }
            };
            let no_rays: &RaycastFn<'_> = &|_| None;
            let launch = ScriptLaunch { instance: script, source: source.to_string(), chunk_name: "TerrainTest".into() };
            self.vm.run_scripts(vec![launch], no_rays, reader);
            self.dm.lock().output.drain(..).map(|line| (line.level, line.text)).collect()
        }

        fn commands(&self) -> Vec<TerrainCommand> {
            self.dm.lock().terrain_commands.clone()
        }
    }

    /// The script ran to its `print("done")` without raising.
    fn assert_finished(lines: &[(OutputLevel, String)]) {
        let errors: Vec<&str> =
            lines.iter().filter(|(level, _)| *level == OutputLevel::Error).map(|(_, text)| text.as_str()).collect();
        assert!(errors.is_empty(), "the script raised: {errors:?}");
        assert!(lines.iter().any(|(_, text)| text == "done"), "the script did not finish: {lines:?}");
    }

    #[test]
    fn region3_size_cframe_and_expand_to_grid() {
        let mut s = session();
        let lines = s.run(
            r#"
            local r = Region3.new(Vector3.new(1, 2, 3), Vector3.new(5, 6, 9))
            assert(typeof(r) == "Region3", "typeof")
            assert(r.Size == Vector3.new(4, 4, 6), "Size " .. tostring(r.Size))
            assert(r.CFrame.Position == Vector3.new(3, 4, 6), "CFrame " .. tostring(r.CFrame.Position))
            local swapped = Region3.new(Vector3.new(5, 6, 9), Vector3.new(1, 2, 3))
            assert(swapped.Size == r.Size and swapped.CFrame == r.CFrame, "corners in either order")
            local g = Region3.new(Vector3.new(1, -1, 3), Vector3.new(5, 6, 9)):ExpandToGrid(4)
            assert(g.Size == Vector3.new(8, 12, 12), "grid size " .. tostring(g.Size))
            assert(g.CFrame.Position == Vector3.new(4, 2, 6), "grid centre " .. tostring(g.CFrame.Position))
            assert(not pcall(function() r:ExpandToGrid(0) end), "a zero resolution raises")
            print("done")
            "#,
            false,
        );
        assert_finished(&lines);
    }

    #[test]
    fn workspace_terrain_is_the_virtual_terrain() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            assert(terrain ~= nil, "workspace.Terrain")
            assert(terrain.ClassName == "Terrain" and terrain.Name == "Terrain", "class and name")
            assert(terrain:IsA("BasePart"), "a BasePart, as in Roblox")
            assert(terrain.Parent == workspace, "under Workspace")
            assert(workspace:GetChildren()[1] == terrain, "Workspace's first child")
            assert(workspace:FindFirstChildOfClass("Terrain") == terrain, "found by class")
            print("done")
            "#,
            false,
        );
        assert_finished(&lines);
        let g = s.dm.lock();
        assert_eq!(terrain_instance(&g), Some(s.terrain));
        assert_eq!(g.class_of(s.terrain), Some("Terrain"));
        assert_eq!(g.entity_of(s.terrain), None, "no entity of its own");
    }

    #[test]
    fn fill_ball_queues_one_converted_command() {
        let mut s = session();
        let lines = s.run(
            r#"
            workspace.Terrain:FillBall(Vector3.new(1, 2, 3), 4.5, Enum.Material.Rock)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        assert_eq!(
            s.commands(),
            vec![TerrainCommand::FillBall {
                center: Vec3::new(1.0, 2.0, 3.0),
                radius: 4.5,
                fill: TerrainFill::Material(TerrainMaterial::Rock),
            }]
        );
    }

    #[test]
    fn fill_block_takes_the_cframe_rotation() {
        let mut s = session();
        let lines = s.run(
            r#"
            local cf = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.pi / 2, 0)
            workspace.Terrain:FillBlock(cf, Vector3.new(4, 2, 6), Enum.Material.Sand)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        let commands = s.commands();
        assert_eq!(commands.len(), 1, "{commands:?}");
        let TerrainCommand::FillBlock { center, rotation, size, fill } = &commands[0] else {
            panic!("expected FillBlock, got {:?}", commands[0]);
        };
        assert!(center.distance(Vec3::new(1.0, 2.0, 3.0)) < 1e-5, "centre {center}");
        assert_eq!(*size, Vec3::new(4.0, 2.0, 6.0));
        assert_eq!(*fill, TerrainFill::Material(TerrainMaterial::Sand));
        let quarter_turn_about_y = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(rotation.dot(quarter_turn_about_y).abs() > 1.0 - 1e-5, "rotation {rotation:?}");
    }

    #[test]
    fn replace_material_refuses_air_and_water_outside_the_sea_level_pairs() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            local region = Region3.new(Vector3.new(0, 0, 0), Vector3.new(8, 8, 8))
            local ok, err = pcall(function()
                terrain:ReplaceMaterial(region, 4, Enum.Material.Air, Enum.Material.Rock)
            end)
            assert(not ok and string.find(tostring(err), "solid"), "an Air source raises: " .. tostring(err))
            ok = pcall(function()
                terrain:ReplaceMaterial(region, 4, Enum.Material.Grass, Enum.Material.Water)
            end)
            assert(not ok, "a Water target raises")
            ok = pcall(function()
                terrain:ReplaceMaterial(region, 4, Enum.Material.Water, Enum.Material.Water)
            end)
            assert(not ok, "Water to Water raises")
            ok = pcall(function()
                terrain:ReplaceMaterial(region, 4, Enum.Material.Air, Enum.Material.Air)
            end)
            assert(not ok, "Air to Air raises")
            terrain:ReplaceMaterial(region, 4, Enum.Material.Grass, Enum.Material.Rock)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        assert_eq!(
            s.commands(),
            vec![TerrainCommand::ReplaceMaterial {
                min: Vec3::ZERO,
                max: Vec3::splat(8.0),
                from: TerrainMaterial::Grass,
                to: TerrainMaterial::Rock,
            }]
        );
    }

    #[test]
    fn air_to_water_fills_water_under_the_region_top() {
        let mut s = session();
        let lines = s.run(
            r#"
            local region = Region3.new(Vector3.new(-16, -8, -16), Vector3.new(16, 4, 16))
            workspace.Terrain:ReplaceMaterial(region, 4, Enum.Material.Air, Enum.Material.Water)
            -- A flat region changes nothing and queues nothing.
            workspace.Terrain:ReplaceMaterial(Region3.new(Vector3.new(0, 0, 0), Vector3.new(4, 0, 4)), 4, Enum.Material.Air, Enum.Material.Water)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        assert_eq!(
            s.commands(),
            vec![TerrainCommand::FillWater { min: Vec3::new(-16.0, -8.0, -16.0), max: Vec3::new(16.0, 4.0, 16.0) }]
        );
    }

    #[test]
    fn water_to_air_drains_the_region() {
        let mut s = session();
        let lines = s.run(
            r#"
            local region = Region3.new(Vector3.new(0, 0, 0), Vector3.new(8, 8, 8))
            workspace.Terrain:ReplaceMaterial(region, 4, Enum.Material.Water, Enum.Material.Air)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        // A drain, never an Air fill: nothing is carved.
        let commands = s.commands();
        assert_eq!(commands, vec![TerrainCommand::DrainWater { min: Vec3::ZERO, max: Vec3::splat(8.0) }]);
        assert!(!commands.iter().any(|c| matches!(c, TerrainCommand::FillRegion { .. })));
    }

    #[test]
    fn write_voxels_checks_every_dimension() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            -- 2 x 1 x 1 voxels of 4 units.
            local region = Region3.new(Vector3.new(0, 0, 0), Vector3.new(8, 4, 4))
            local occupancies = { { { 1 } }, { { 0.5 } } }
            local rock, air = Enum.Material.Rock, Enum.Material.Air
            assert(not pcall(function()
                terrain:WriteVoxels(region, 4, { { { rock } } }, occupancies)
            end), "one X entry for two raises")
            assert(not pcall(function()
                terrain:WriteVoxels(region, 4, { { { rock } }, { { rock, rock } } }, occupancies)
            end), "two Z entries for one raise")
            assert(not pcall(function()
                terrain:WriteVoxels(region, 4, { { { rock } }, { { air } } }, { { { 1 } } })
            end), "too few occupancies raise")
            terrain:WriteVoxels(region, 4, { { { rock } }, { { air } } }, occupancies)
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        assert_eq!(
            s.commands(),
            vec![TerrainCommand::WriteVoxels {
                min: Vec3::ZERO,
                resolution: 4.0,
                size: UVec3::new(2, 1, 1),
                materials: vec![TerrainFill::Material(TerrainMaterial::Rock), TerrainFill::Air],
                occupancies: vec![1.0, 0.5],
            }]
        );
    }

    #[test]
    fn the_terrain_cannot_be_destroyed_moved_or_cloned() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            local ok, err = pcall(function() terrain:Destroy() end)
            assert(not ok and string.find(tostring(err), "locked"), "Destroy raises: " .. tostring(err))
            assert(terrain.Parent == workspace and workspace.Terrain == terrain, "still in Workspace")
            assert(not pcall(function() terrain.Parent = nil end), "its Parent is locked")
            assert(terrain:Clone() == nil, "it cannot be cloned")
            assert(not pcall(function() Instance.new("Terrain") end), "there is only one")
            workspace:ClearAllChildren()
            assert(workspace.Terrain == terrain, "ClearAllChildren keeps it")
            print("done")
            "#,
            false,
        );
        assert_finished(&lines);
        let g = s.dm.lock();
        assert!(g.exists(s.terrain));
        assert_eq!(g.parent(s.terrain), Some(s.workspace));
    }

    #[test]
    fn without_a_terrain_writes_raise_and_reads_are_empty() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            local ok, err = pcall(function() terrain:FillBall(Vector3.zero, 4, Enum.Material.Grass) end)
            assert(not ok and string.find(tostring(err), "no terrain"), "FillBall raises: " .. tostring(err))
            assert(not pcall(function() terrain:Clear() end), "Clear raises too")
            local materials, occupancies = terrain:ReadVoxels(Region3.new(Vector3.zero, Vector3.new(8, 4, 4)), 4)
            assert(materials.Size == Vector3.new(2, 1, 1) and occupancies.Size == materials.Size, "Size")
            assert(materials[2][1][1] == Enum.Material.Air and occupancies[2][1][1] == 0, "empty voxels")
            print("done")
            "#,
            false,
        );
        assert_finished(&lines);
        assert!(s.commands().is_empty());
    }

    #[test]
    fn cells_follow_the_terrain_lattice() {
        let mut s = session();
        // The default terrain: 64 m chunks of 32 lattice cells, 2 m a cell.
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            assert(terrain:WorldToCell(Vector3.new(3, -1, 4.5)) == Vector3.new(1, -1, 2), "WorldToCell")
            assert(terrain:CellCenterToWorld(1, -1, 2) == Vector3.new(3, -1, 5), "CellCenterToWorld")
            assert(terrain:CellCornerToWorld(1, -1, 2) == Vector3.new(2, -2, 4), "CellCornerToWorld")
            assert(typeof(terrain:GetMaterialColor(Enum.Material.Grass)) == "Color3", "GetMaterialColor")
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
    }

    #[test]
    fn set_material_color_keeps_the_colour_and_warns_once() {
        let mut s = session();
        let lines = s.run(
            r#"
            local terrain = workspace.Terrain
            local before = terrain:GetMaterialColor(Enum.Material.Grass)
            terrain:SetMaterialColor(Enum.Material.Grass, Color3.new(1, 0, 0))
            terrain:SetMaterialColor(Enum.Material.Rock, Color3.new(0, 0, 1))
            local after = terrain:GetMaterialColor(Enum.Material.Grass)
            assert(after.R == before.R and after.G == before.G and after.B == before.B, "the colour is unchanged")
            local ok = pcall(function()
                terrain:SetMaterialColor(Enum.Material.Air, Color3.new(1, 1, 1))
            end)
            assert(not ok, "Air has no colour")
            print("done")
            "#,
            true,
        );
        assert_finished(&lines);
        let warnings = lines
            .iter()
            .filter(|(level, text)| *level == OutputLevel::Warn && text.contains("SetMaterialColor"))
            .count();
        assert_eq!(warnings, 1, "one warning a session: {lines:?}");
        assert!(s.commands().is_empty(), "nothing is queued");
    }

    #[test]
    fn ensure_terrain_instance_keeps_one_locked_terrain_first() {
        let mut dm = DataModel::new();
        let workspace = dm.get_service("Workspace").expect("Workspace");
        // A user folder that happens to be called Terrain.
        dm.create_virtual("Folder", "Terrain", Some(workspace));
        let terrain = ensure_terrain_instance(&mut dm, workspace);
        assert_eq!(dm.children(workspace).first(), Some(&terrain));
        assert_eq!(dm.find_first_child(workspace, "Terrain", false), Some(terrain));
        assert_eq!(ensure_terrain_instance(&mut dm, workspace), terrain, "only one Terrain");
        assert!(dm.set_parent(terrain, None).is_err(), "its Parent is locked");
        assert!(dm.clone_instance(terrain).is_none(), "it cannot be cloned");
        assert!(dm.take_spawns().is_empty(), "it never spawns an entity");
        // Rune destroys through the tree directly; the Terrain stays for it
        // too, and ClearAllChildren takes only the folder.
        dm.destroy(terrain);
        dm.clear_all_children(workspace);
        assert!(dm.exists(terrain), "it cannot be destroyed");
        assert_eq!(dm.children(workspace), &[terrain][..]);
    }
}
