//! # Reading a Eustress Space's geometry — the shared subset
//!
//! ## Scope, stated up front
//!
//! This is **not** the full Space loader. Studio's `instance_loader` is ~3,900
//! lines covering CAD, GUI, terrain, decals, splats, nuclear state, Draco
//! meshes, streaming and editor write-back. None of that can move into
//! `eustress-common` without dragging Slint, `worlddb` and `radiance` with it.
//!
//! What this reads is the subset a *movement* Space is made of: `Part`
//! instances with a transform, a size, a colour and collision. That is enough
//! for the Client to open a real Space from the real on-disk format and play
//! it, which is the thing that was impossible before — `space_fetch` unpacked
//! an archive and nothing consumed the result.
//!
//! Anything outside the subset is **skipped and counted**, never guessed at.
//! [`SpaceGeometry::skipped`] reports what was ignored so a Space that quietly
//! half-loads is distinguishable from one that loaded.
//!
//! ## The format, as it is on disk
//!
//! * A Space root holds service directories: `Workspace/`, `Lighting/`, …
//! * Geometry lives under `Workspace/`.
//! * **Hierarchy is directory nesting.** A directory containing
//!   `_instance.toml` *is* that instance; a directory without one is a plain
//!   folder whose children are still nested under it. There is no `parent`
//!   field anywhere.
//! * A flat `<Name>.instance.toml` beside a directory is the same thing in
//!   one file. When both forms exist for one name the folder form wins, which
//!   is what the engine loader does — otherwise the entity spawns twice.
//! * `[transform] scale` is **the part's size in metres**, not a multiplier.
//!
//! ## The colour trap
//!
//! `[properties] color` is an array that may be 3 or 4 long and may be either
//! 0-255 integers or 0-1 floats. The rule — taken from the engine loader, not
//! invented here — is that an **all-integer** array means 0-255 and anything
//! containing a float means 0-1. A single `1.0` in an otherwise integer array
//! therefore reinterprets the whole thing, which is silent and total.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::Deserialize;

/// One `Part` read out of a Space.
#[derive(Debug, Clone)]
pub struct SpacePart {
    pub name: String,
    pub class_name: String,
    /// World transform, composed through directory nesting. `scale` carries
    /// the part's size in metres.
    pub transform: Transform,
    pub color: Color,
    pub material: String,
    pub anchored: bool,
    pub can_collide: bool,
    pub transparency: f32,
}

/// Everything the shared reader understood, plus an honest account of what it
/// did not.
#[derive(Debug, Clone, Default)]
pub struct SpaceGeometry {
    pub parts: Vec<SpacePart>,
    /// Instance files skipped, keyed by `class_name`, with counts. A movement
    /// Space should be all `Part`; anything else here is content the Client is
    /// silently not showing.
    pub skipped: BTreeMap<String, usize>,
    /// Files that failed to parse, with the reason. Never silently dropped:
    /// a Space that half-loads must be distinguishable from one that loaded.
    pub errors: Vec<String>,
}

impl SpaceGeometry {
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }
}

// ── Wire types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct InstanceFile {
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    transform: TransformBlock,
    #[serde(default)]
    properties: Properties,
}

#[derive(Debug, Deserialize, Default)]
struct Metadata {
    #[serde(default)]
    class_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransformBlock {
    #[serde(default = "zero3")]
    position: [f32; 3],
    #[serde(default = "ident_quat")]
    rotation: [f32; 4],
    #[serde(default = "one3")]
    scale: [f32; 3],
}

impl Default for TransformBlock {
    fn default() -> Self {
        Self { position: zero3(), rotation: ident_quat(), scale: one3() }
    }
}

fn zero3() -> [f32; 3] {
    [0.0; 3]
}
fn one3() -> [f32; 3] {
    [1.0; 3]
}
fn ident_quat() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Debug, Deserialize, Default)]
struct Properties {
    #[serde(default)]
    color: Option<Vec<toml::Value>>,
    #[serde(default = "yes")]
    anchored: bool,
    #[serde(default = "yes")]
    can_collide: bool,
    #[serde(default)]
    transparency: f32,
    #[serde(default)]
    material: Option<String>,
}

fn yes() -> bool {
    true
}

/// Decode the colour array under the engine's own rule.
fn decode_color(raw: &[toml::Value]) -> Color {
    if raw.len() < 3 {
        return Color::srgb(0.6, 0.6, 0.6);
    }
    // All-integer means 0-255; ANY float means 0-1. Mixing the two silently
    // reinterprets every channel, so the test is over the whole array.
    let all_int = raw.iter().all(|v| v.as_integer().is_some());
    let f = |i: usize| -> f32 {
        raw.get(i)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
            .unwrap_or(0.0) as f32
    };
    let div = if all_int { 255.0 } else { 1.0 };
    let a = if raw.len() >= 4 { f(3) / div } else { 1.0 };
    Color::srgba(f(0) / div, f(1) / div, f(2) / div, a)
}

fn to_transform(t: &TransformBlock) -> Transform {
    Transform {
        translation: Vec3::from_array(t.position),
        rotation: Quat::from_xyzw(t.rotation[0], t.rotation[1], t.rotation[2], t.rotation[3])
            .normalize(),
        scale: Vec3::from_array(t.scale),
    }
}

// ── The walk ───────────────────────────────────────────────────────────────

/// Read every `Part` under `<space_root>/Workspace`.
///
/// Returns `Err` only when the Space itself is unusable; per-file problems are
/// collected into [`SpaceGeometry::errors`] so one malformed part cannot cost
/// you the whole course.
pub fn read_space_parts(space_root: &Path) -> Result<SpaceGeometry, String> {
    let workspace = space_root.join("Workspace");
    if !workspace.is_dir() {
        return Err(format!(
            "not a Space: {} has no Workspace/ directory",
            space_root.display()
        ));
    }
    let mut out = SpaceGeometry::default();
    walk(&workspace, Transform::IDENTITY, &mut out);
    Ok(out)
}

/// Compose a child transform onto a parent.
///
/// Deliberately NOT `Transform::mul_transform`: `scale` here is a *size in
/// metres*, not a multiplier, so scale must not propagate to children. Only
/// position and rotation compose.
fn compose(parent: &Transform, child: &Transform) -> Transform {
    Transform {
        translation: parent.translation + parent.rotation * child.translation,
        rotation: parent.rotation * child.rotation,
        scale: child.scale,
    }
}

fn walk(dir: &Path, parent: Transform, out: &mut SpaceGeometry) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        out.errors.push(format!("unreadable directory: {}", dir.display()));
        return;
    };

    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut flat: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            dirs.push(p);
        } else if name.ends_with(".instance.toml") {
            flat.push(p);
        }
    }

    // Folder form beats flat form for the same name, matching the engine —
    // otherwise a Space carrying both spawns the entity twice.
    let dir_names: Vec<String> = dirs
        .iter()
        .filter_map(|d| d.file_name()?.to_str().map(str::to_owned))
        .collect();

    for f in flat {
        let stem = f
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.trim_end_matches(".instance.toml").to_owned())
            .unwrap_or_default();
        if dir_names.iter().any(|d| *d == stem) {
            continue;
        }
        ingest(&f, &stem, &parent, out);
    }

    for d in dirs {
        let name = d
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_owned();
        let inst = d.join("_instance.toml");
        let here = if inst.is_file() {
            ingest(&inst, &name, &parent, out).unwrap_or(parent)
        } else {
            // A plain directory is a folder: no transform of its own, but its
            // children still nest under whatever contains it.
            parent
        };
        walk(&d, here, out);
    }
}

/// Parse one instance file. Returns the composed world transform so children
/// can nest under it.
fn ingest(
    path: &Path,
    fallback_name: &str,
    parent: &Transform,
    out: &mut SpaceGeometry,
) -> Option<Transform> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            out.errors.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let file: InstanceFile = match toml::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            out.errors.push(format!("{}: {e}", path.display()));
            return None;
        }
    };

    let world = compose(parent, &to_transform(&file.transform));
    let class = file.metadata.class_name.clone().unwrap_or_else(|| "Part".into());

    if class != "Part" {
        *out.skipped.entry(class).or_insert(0) += 1;
        return Some(world);
    }

    let p = &file.properties;
    out.parts.push(SpacePart {
        name: file.metadata.name.clone().unwrap_or_else(|| fallback_name.to_owned()),
        class_name: class,
        transform: world,
        color: p.color.as_deref().map(decode_color).unwrap_or(Color::srgb(0.6, 0.6, 0.6)),
        material: p.material.clone().unwrap_or_else(|| "Plastic".into()),
        anchored: p.anchored,
        can_collide: p.can_collide,
        transparency: p.transparency,
    });
    Some(world)
}

// ── Spawning ───────────────────────────────────────────────────────────────

/// Marks an entity spawned from a Space by [`spawn_space_parts`], so a shell
/// can clear the world without tracking handles itself.
#[cfg(feature = "physics")]
#[derive(Component, Debug)]
pub struct SpawnedFromSpace;

/// Spawn every part in `geo` as a box with a matching collider.
///
/// Lives in `common` rather than in a shell so that "the Client sees what
/// Studio sees" is a property of one function instead of two implementations
/// agreeing by inspection.
///
/// ## The collider is a UNIT cube, and that is not a mistake
///
/// Avian applies the entity's `Transform.scale` to its collider — see
/// `update_collider_scale` in avian3d-0.7.0 `collision/collider/backend.rs:460`,
/// gated on `transform_to_collider_scale`, which defaults to `true`
/// (`physics_transform/mod.rs:162`).
///
/// A part's size is carried in `Transform.scale`, so passing that size to
/// `Collider::cuboid` as well multiplies it in twice: the collider ends up
/// **size²**. The course's 160 m ground plate got a 25 600 m collider, which
/// swallowed the whole level — the character stood on an invisible surface far
/// above the visible one and was shoved around by boxes metres from anything
/// on screen.
///
/// `Collider::cuboid` itself takes FULL lengths and halves them internally
/// (`parry/mod.rs:747`), so a unit cube scaled by `Transform.scale` is exactly
/// the visible box.
#[cfg(feature = "physics")]
pub fn spawn_space_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    geo: &SpaceGeometry,
) -> usize {
    use avian3d::prelude::{Collider, RigidBody};

    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut n = 0;

    for p in &geo.parts {
        let s = p.transform.scale;
        // A zero or negative extent produces a degenerate collider that Avian
        // reports as NaN contacts rather than rejecting.
        if !s.is_finite() || s.min_element() <= 1e-4 {
            continue;
        }

        let mut e = commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: p.color,
                perceptual_roughness: 0.85,
                alpha_mode: if p.transparency > 0.0 {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                ..default()
            })),
            *&p.transform,
            Name::new(p.name.clone()),
            SpawnedFromSpace,
        ));

        if p.can_collide {
            e.insert((Collider::cuboid(1.0, 1.0, 1.0), RigidBody::Static));
        }
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_colour_arrays_are_0_255_and_float_arrays_are_0_1() {
        let ints: Vec<toml::Value> = vec![255.into(), 128.into(), 0.into()];
        let c = decode_color(&ints).to_srgba();
        assert!((c.red - 1.0).abs() < 1e-3, "255 should be full red, got {}", c.red);
        assert!((c.green - 0.502).abs() < 1e-2);

        let floats: Vec<toml::Value> = vec![
            toml::Value::Float(0.5),
            toml::Value::Float(0.25),
            toml::Value::Float(1.0),
        ];
        let c = decode_color(&floats).to_srgba();
        assert!((c.red - 0.5).abs() < 1e-3, "float array must not be divided by 255");
    }

    /// The trap: one float in an otherwise-integer array reinterprets EVERY
    /// channel. Pinned so the rule cannot be softened into per-element guessing.
    #[test]
    fn one_float_reinterprets_the_whole_array() {
        let mixed: Vec<toml::Value> = vec![200.into(), 120.into(), toml::Value::Float(1.0)];
        let c = decode_color(&mixed).to_srgba();
        assert!(
            c.red > 1.0,
            "mixed array must be read as 0-1 (and so overflow), not silently as 0-255"
        );
    }

    #[test]
    fn alpha_defaults_to_opaque_when_absent() {
        let rgb: Vec<toml::Value> = vec![
            toml::Value::Float(0.2),
            toml::Value::Float(0.2),
            toml::Value::Float(0.2),
        ];
        assert!((decode_color(&rgb).to_srgba().alpha - 1.0).abs() < 1e-6);
    }

    /// `scale` is a SIZE, so it must not propagate: a 4 m box inside a folder
    /// nested in another folder is still 4 m.
    #[test]
    fn nesting_composes_position_and_rotation_but_never_size() {
        let parent = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(3.0),
        };
        let child = Transform {
            translation: Vec3::new(0.0, 0.0, -2.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(4.0, 0.5, 1.5),
        };
        let w = compose(&parent, &child);

        assert_eq!(w.scale, child.scale, "parent scale leaked into child size");
        // +90° about Y maps -Z to -X.
        assert!((w.translation.x - 8.0).abs() < 1e-4, "got {:?}", w.translation);
        assert!(w.translation.z.abs() < 1e-4);
    }

    #[test]
    fn a_directory_without_workspace_is_not_a_space() {
        let tmp = std::env::temp_dir().join("eustress_space_read_neg");
        let _ = std::fs::create_dir_all(&tmp);
        assert!(read_space_parts(&tmp).is_err());
    }

    /// End-to-end against the real authored Space when it is present. Skipped
    /// rather than failed on a machine that has not generated it — but when it
    /// IS there, this is the only test that proves the reader agrees with the
    /// format actually on disk.
    #[test]
    fn the_climbing_course_reads_if_it_is_installed() {
        let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
        else {
            return;
        };
        let root = PathBuf::from(home)
            .join("Documents/Eustress/Movement/Spaces/Climbing");
        if !root.join("Workspace").is_dir() {
            return;
        }

        let geo = read_space_parts(&root).expect("Climbing Space failed to read");
        assert!(geo.errors.is_empty(), "parse errors: {:?}", geo.errors);
        assert!(
            geo.parts.len() >= 40,
            "expected the full course, got {} parts",
            geo.parts.len()
        );
        // The course is deliberately built so specific heights exist; if the
        // nesting maths is wrong these land somewhere else entirely.
        let ground = geo.parts.iter().find(|p| p.name == "Ground").expect("no Ground");
        assert!((ground.transform.translation.y + 0.5).abs() < 1e-3);
        assert!(ground.transform.scale.x > 100.0, "Ground lost its size");
    }
}
