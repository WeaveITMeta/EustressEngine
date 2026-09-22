//! # Moderation dossier and publish-time capture set
//!
//! What the Gallery's moderation pipeline reads about a Universe, built here at
//! publish from the live World, because the Worker that classifies it has no
//! engine and the classifier that screens it (Jev) reads text only. See
//! `docs/architecture/MODERATION_PIPELINE.md` section 4 for the schema and the
//! trust boundary, and `infrastructure/cloudflare/api/src/moderation.mjs` for
//! the consumer.
//!
//! Two artifacts leave this module:
//!
//! 1. **The dossier** (`.eustress/moderation-dossier.json`): a measured digest
//!    of the scene (counts, variety, defaults, duplicates, bounds), every
//!    human-visible string, the head of every script, asset names, and the
//!    off-platform signals found in them. Built synchronously in `do_publish`
//!    while it still holds `&mut World`.
//! 2. **The capture set** (`.eustress/moderation/capture-N.png`): an orbit of
//!    the scene through the off-screen AI camera, driven over frames by
//!    [`PublishCapturePlugin`] because a capture takes ~10 frames of warmup
//!    and readback. The upload thread waits on the shared [`CaptureStatus`].
//!
//! Nothing here judges. The digest is what the policy calls evidence; the
//! verdict is issued server-side.

use bevy::prelude::*;
// `Material` is aliased because bevy's prelude exports the PBR `Material` trait
// under the same name; the part enum is the one this module reads.
use eustress_common::classes::{
    BasePart, ClassName, Decal, Dialog, Instance, Material as PartMaterial, Sound, TextButton, TextLabel,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ai_camera::{request_capture, AiCamera, AiCameraState};

/// Schema version the Worker validates (`DOSSIER_VERSION` in moderation.mjs).
pub const DOSSIER_VERSION: u32 = 1;
/// Poses in the publish orbit. Four quadrants at a raised pitch cover a scene
/// from every side without a fifth top-down shot that mostly shows roofs.
pub const CAPTURE_COUNT: usize = 4;

const MAX_STRINGS: usize = 400;
const MAX_STRING_CHARS: usize = 240;
const MAX_SCRIPTS: usize = 40;
const MAX_SCRIPT_BYTES_EACH: usize = 12 * 1024;
const MAX_SCRIPT_BYTES_TOTAL: usize = 160 * 1024;
const MAX_ASSET_NAMES: usize = 200;
const MAX_HISTOGRAM_CLASSES: usize = 24;
/// The orbit gives up after this and submits with what it has: a publish must
/// never hang on a render that will not come.
const CAPTURE_BUDGET: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Default)]
pub struct DossierListing {
    pub name: String,
    pub description: String,
    pub genre: String,
    pub is_public: bool,
    pub open_source: bool,
    pub studio_editable: bool,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct ClassCount {
    pub class_name: String,
    pub count: usize,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub extent_m: f32,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Digest {
    pub entity_count: usize,
    pub part_count: usize,
    pub class_histogram: Vec<ClassCount>,
    pub bounds: Option<Bounds>,
    pub hierarchy_depth: usize,
    pub unique_materials: usize,
    pub unique_colors: usize,
    pub default_material_fraction: f32,
    pub default_color_fraction: f32,
    pub default_name_fraction: f32,
    pub duplicate_transform_fraction: f32,
    pub transparent_part_fraction: f32,
    pub script_count: usize,
    pub script_lines: usize,
    pub text_string_count: usize,
    pub spawn_count: usize,
    pub gui_count: usize,
    pub sound_count: usize,
    pub decal_count: usize,
    pub mesh_count: usize,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct ScriptEntry {
    pub path: String,
    pub language: String,
    pub lines: usize,
    pub bytes: usize,
    pub truncated: bool,
    pub source: String,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Assets {
    pub sounds: Vec<String>,
    pub decals: Vec<String>,
    pub meshes: Vec<String>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Signals {
    pub urls: Vec<String>,
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub discord_invites: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CapturePose {
    pub label: String,
    pub position: [f32; 3],
    pub look_at: [f32; 3],
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct CapturePlan {
    pub planned: usize,
    pub poses: Vec<CapturePose>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Dossier {
    pub dossier_version: u32,
    pub engine_semver: String,
    pub generated_at: String,
    /// `blake3:<hex>` of the .pak. Filled by the upload thread once the package
    /// exists; the World has no idea what the archive will hash to.
    pub content_root: Option<String>,
    pub listing: DossierListing,
    pub digest: Digest,
    pub strings: Vec<String>,
    pub scripts: Vec<ScriptEntry>,
    pub assets: Assets,
    pub signals: Signals,
    pub captures: CapturePlan,
}

// ---------------------------------------------------------------------------
// Building the dossier from the World
// ---------------------------------------------------------------------------

/// Bevy's default part tint, `BasePart::default().color`, as 8-bit sRGB.
const DEFAULT_PART_RGB: [u8; 3] = [163, 162, 165];

fn rgb8(color: &Color) -> [u8; 3] {
    let s = color.to_srgba();
    [
        (s.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (s.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (s.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// "Part", "Part (3)", "Part12", "MeshPart 2": the names the engine assigns
/// when nobody chose one. A scene where every name is one of these has had no
/// naming pass at all, which is one of the policy's criterion-5 tells.
fn is_default_name(name: &str, class: &str) -> bool {
    let n = name.trim();
    if n.is_empty() || n == class {
        return true;
    }
    match n.strip_prefix(class) {
        Some(rest) => rest
            .chars()
            .all(|c| c.is_ascii_digit() || c == ' ' || c == '(' || c == ')' || c == '_' || c == '-'),
        None => false,
    }
}

fn push_string(set: &mut HashSet<String>, out: &mut Vec<String>, s: &str) {
    let t = s.trim();
    if t.is_empty() || out.len() >= MAX_STRINGS {
        return;
    }
    let clipped: String = t.chars().take(MAX_STRING_CHARS).collect();
    if set.insert(clipped.clone()) {
        out.push(clipped);
    }
}

fn push_asset(set: &mut HashSet<String>, out: &mut Vec<String>, s: &str) {
    let t = s.trim();
    if t.is_empty() || out.len() >= MAX_ASSET_NAMES {
        return;
    }
    if set.insert(t.to_string()) {
        out.push(t.to_string());
    }
}

/// Build the dossier. Read-only over the World; the `&mut` is what
/// `World::query` needs to register the query state.
pub fn build_dossier(world: &mut World, universe_root: &Path, listing: DossierListing) -> Dossier {
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    let mut parents: HashMap<Entity, Entity> = HashMap::new();
    let mut entity_count = 0usize;
    let mut part_count = 0usize;
    let mut default_material = 0usize;
    let mut default_color = 0usize;
    let mut default_name = 0usize;
    let mut transparent = 0usize;
    let mut materials: HashSet<String> = HashSet::new();
    let mut colors: HashSet<[u8; 3]> = HashSet::new();
    let mut transforms: HashSet<(String, [i64; 3], [i64; 3])> = HashSet::new();
    let mut transform_samples = 0usize;
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut any_pos = false;
    let mut strings_seen: HashSet<String> = HashSet::new();
    let mut strings: Vec<String> = Vec::new();
    let mut text_string_count = 0usize;
    let mut gui_count = 0usize;
    let mut assets = Assets::default();
    let mut seen_sounds = HashSet::new();
    let mut seen_decals = HashSet::new();
    let mut seen_meshes = HashSet::new();

    let mut q = world.query::<(
        Entity,
        &Instance,
        Option<&BasePart>,
        Option<&GlobalTransform>,
        Option<&ChildOf>,
        Option<&TextLabel>,
        Option<&TextButton>,
        Option<&Dialog>,
        Option<&Sound>,
        Option<&Decal>,
        Option<&crate::spawn::MeshSource>,
    )>();

    for (entity, inst, bp, gt, child_of, label, button, dialog, sound, decal, mesh) in q.iter(world) {
        // Editor furniture carries `Instance` too (the AI camera is a Camera
        // named "AI Camera"); it is not the author's content.
        if matches!(inst.class_name, ClassName::Camera) {
            continue;
        }
        entity_count += 1;
        let class = inst.class_name.as_str().to_string();
        *hist.entry(class.clone()).or_insert(0) += 1;
        if let Some(c) = child_of {
            parents.insert(entity, c.0);
        }
        if is_default_name(&inst.name, &class) {
            default_name += 1;
        } else {
            push_string(&mut strings_seen, &mut strings, &inst.name);
        }

        if let Some(bp) = bp {
            part_count += 1;
            let material_key = if bp.material_name.is_empty() {
                bp.material.as_str().to_string()
            } else {
                bp.material_name.clone()
            };
            if matches!(bp.material, PartMaterial::Plastic) && bp.material_name.is_empty() {
                default_material += 1;
            }
            materials.insert(material_key);
            let rgb = rgb8(&bp.color);
            if rgb == DEFAULT_PART_RGB {
                default_color += 1;
            }
            colors.insert(rgb);
            if bp.transparency > 0.05 {
                transparent += 1;
            }
            if let Some(gt) = gt {
                let t = gt.translation();
                // Centimetre quantization: two parts closer than that with the
                // same size are the same placement for filler purposes.
                let key = (
                    class.clone(),
                    [(t.x * 100.0) as i64, (t.y * 100.0) as i64, (t.z * 100.0) as i64],
                    [(bp.size.x * 100.0) as i64, (bp.size.y * 100.0) as i64, (bp.size.z * 100.0) as i64],
                );
                transforms.insert(key);
                transform_samples += 1;
            }
        }
        if let Some(gt) = gt {
            let t = gt.translation();
            if t.is_finite() {
                min = min.min(t);
                max = max.max(t);
                any_pos = true;
            }
        }
        if let Some(l) = label {
            gui_count += 1;
            text_string_count += 1;
            push_string(&mut strings_seen, &mut strings, &l.text);
        }
        if let Some(b) = button {
            gui_count += 1;
            text_string_count += 1;
            push_string(&mut strings_seen, &mut strings, &b.text);
        }
        if let Some(d) = dialog {
            text_string_count += 2;
            push_string(&mut strings_seen, &mut strings, &d.initial_prompt);
            push_string(&mut strings_seen, &mut strings, &d.goodbye_dialog);
        }
        if let Some(s) = sound {
            push_asset(&mut seen_sounds, &mut assets.sounds, &s.sound_id);
        }
        if let Some(d) = decal {
            push_asset(&mut seen_decals, &mut assets.decals, &d.texture);
        }
        if let Some(m) = mesh {
            push_asset(&mut seen_meshes, &mut assets.meshes, &m.path);
        }
    }

    // Depth: walk each entity to a root, capped so a cycle in a corrupt scene
    // cannot spin.
    let mut depth = 0usize;
    for start in parents.keys() {
        let mut d = 0usize;
        let mut cur = *start;
        while let Some(p) = parents.get(&cur) {
            d += 1;
            cur = *p;
            if d > 64 {
                break;
            }
        }
        depth = depth.max(d);
    }

    let mut class_histogram: Vec<ClassCount> = hist
        .into_iter()
        .map(|(class_name, count)| ClassCount { class_name, count })
        .collect();
    class_histogram.sort_by(|a, b| b.count.cmp(&a.count).then(a.class_name.cmp(&b.class_name)));
    let spawn_count = class_histogram
        .iter()
        .find(|c| c.class_name == "SpawnLocation")
        .map(|c| c.count)
        .unwrap_or(0);
    let sound_count = class_histogram.iter().find(|c| c.class_name == "Sound").map(|c| c.count).unwrap_or(0);
    let decal_count = class_histogram.iter().find(|c| c.class_name == "Decal").map(|c| c.count).unwrap_or(0);
    let mesh_count = class_histogram.iter().find(|c| c.class_name == "MeshPart").map(|c| c.count).unwrap_or(0);
    class_histogram.truncate(MAX_HISTOGRAM_CLASSES);

    let bounds = any_pos.then(|| Bounds {
        min: min.to_array(),
        max: max.to_array(),
        extent_m: (max - min).max_element().max(0.0),
    });

    let frac = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f32 / d as f32 };
    let scripts = collect_scripts(universe_root);
    let script_lines: usize = scripts.iter().map(|s| s.lines).sum();
    let mut signals = Signals::default();
    for s in &strings {
        scan_signals(s, &mut signals);
    }
    for s in &scripts {
        scan_signals(&s.source, &mut signals);
    }
    scan_signals(&listing.description, &mut signals);
    dedup_signals(&mut signals);

    let poses = capture_poses(bounds.as_ref());

    Dossier {
        dossier_version: DOSSIER_VERSION,
        engine_semver: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        content_root: None,
        listing,
        digest: Digest {
            entity_count,
            part_count,
            class_histogram,
            bounds,
            hierarchy_depth: depth,
            unique_materials: materials.len(),
            unique_colors: colors.len(),
            default_material_fraction: frac(default_material, part_count),
            default_color_fraction: frac(default_color, part_count),
            default_name_fraction: frac(default_name, entity_count),
            duplicate_transform_fraction: if transform_samples == 0 {
                0.0
            } else {
                1.0 - transforms.len() as f32 / transform_samples as f32
            },
            transparent_part_fraction: frac(transparent, part_count),
            script_count: scripts.len(),
            script_lines,
            text_string_count,
            spawn_count,
            gui_count,
            sound_count,
            decal_count,
            mesh_count,
        },
        strings,
        scripts,
        assets,
        signals,
        captures: CapturePlan { planned: poses.len(), poses },
    }
}

/// Every script under the Universe, in path order, head-truncated to the
/// per-file and total caps. Sorting first keeps the selection deterministic
/// when the total cap bites.
fn collect_scripts(universe_root: &Path) -> Vec<ScriptEntry> {
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_scripts(universe_root, universe_root, &mut paths, 0);
    paths.sort();
    let mut out = Vec::new();
    let mut total = 0usize;
    for path in paths.into_iter().take(MAX_SCRIPTS) {
        let Ok(raw) = std::fs::read(&path) else { continue };
        let source = String::from_utf8_lossy(&raw);
        let lines = source.lines().count();
        let bytes = raw.len();
        let remaining = MAX_SCRIPT_BYTES_TOTAL.saturating_sub(total);
        if remaining == 0 {
            break;
        }
        let cap = MAX_SCRIPT_BYTES_EACH.min(remaining);
        let mut head: String = source.chars().take(cap).collect();
        // Cut on a char boundary already; also avoid ending mid-line so the
        // classifier never sees a torn token that looks like something else.
        if head.len() < source.len() {
            if let Some(nl) = head.rfind('\n') {
                head.truncate(nl);
            }
        }
        total += head.len();
        let rel = path.strip_prefix(universe_root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let language = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "luau" | "lua" => "luau",
            _ => "rune",
        };
        out.push(ScriptEntry {
            path: rel,
            language: language.to_string(),
            lines,
            bytes,
            truncated: head.len() < source.len(),
            source: head,
        });
    }
    out
}

fn walk_scripts(dir: &Path, root: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 24 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            // `.eustress` holds publish state and previous dossiers; `assets`
            // is binary; the rest is never authored content.
            if matches!(name.as_str(), ".eustress" | ".git" | "target" | "node_modules" | "assets") {
                continue;
            }
            walk_scripts(&path, root, out, depth + 1);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("luau") | Some("lua") | Some("rn") | Some("rune")
        ) {
            out.push(path);
        }
    }
}

/// Off-platform signals. Hand-rolled scanners rather than a regex crate: the
/// engine does not link one, and the patterns are simple enough that a false
/// positive costs one classifier question, not a listing.
fn scan_signals(text: &str, signals: &mut Signals) {
    let lower = text.to_ascii_lowercase();
    for (i, _) in lower.match_indices("http") {
        let tail = &text[i..];
        if tail.starts_with("http://") || tail.starts_with("https://") {
            let end = tail
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')' || c == '>' || c == '`')
                .unwrap_or(tail.len());
            let url = &tail[..end.min(200)];
            if url.len() > 10 {
                signals.urls.push(url.to_string());
            }
        }
    }
    for (i, _) in lower.match_indices("discord.gg/") {
        let tail = &text[i..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '/' || c == '-'))
            .unwrap_or(tail.len());
        signals.discord_invites.push(tail[..end.min(60)].to_string());
    }
    for (i, _) in text.match_indices('@') {
        let before: String = text[..i]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let after: String = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            .collect();
        if before.len() >= 2 && after.contains('.') && after.len() >= 4 && !after.ends_with('.') {
            signals.emails.push(format!("{before}@{after}"));
        }
    }
    // Phone-shaped: 10 to 15 digits with at least one and at most a few
    // separators. A decimal point ends a run rather than joining it, so a
    // coordinate list never reads as a number, and a bare hash has no
    // separators at all.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() || bytes[i] == b'+' {
            let start = i;
            let mut digits = 0usize;
            let mut seps = 0usize;
            let mut j = i;
            while j < bytes.len() {
                let c = bytes[j];
                if c.is_ascii_digit() {
                    digits += 1;
                } else if matches!(c, b' ' | b'-' | b'(' | b')' | b'+') {
                    seps += 1;
                    if seps > 6 {
                        break;
                    }
                } else {
                    break;
                }
                j += 1;
            }
            if (10..=15).contains(&digits) && seps >= 1 {
                signals.phones.push(text[start..j].trim().to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
}

fn dedup_signals(s: &mut Signals) {
    for list in [&mut s.urls, &mut s.emails, &mut s.phones, &mut s.discord_invites] {
        let mut seen = HashSet::new();
        list.retain(|v| seen.insert(v.clone()));
        list.truncate(50);
    }
}

/// Four quadrant poses on an orbit around the scene, raised 25 degrees, at a
/// distance that frames the whole extent. An empty scene still gets an orbit
/// around the origin so the judge sees that it is empty.
pub fn capture_poses(bounds: Option<&Bounds>) -> Vec<CapturePose> {
    let (center, extent) = match bounds {
        Some(b) => {
            let min = Vec3::from_array(b.min);
            let max = Vec3::from_array(b.max);
            ((min + max) * 0.5, b.extent_m)
        }
        None => (Vec3::ZERO, 0.0),
    };
    let distance = (extent * 1.15).max(12.0);
    let pitch = 25f32.to_radians();
    [45f32, 135.0, 225.0, 315.0]
        .iter()
        .take(CAPTURE_COUNT)
        .map(|yaw_deg| {
            let yaw = yaw_deg.to_radians();
            let dir = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
            let position = center + dir * distance;
            CapturePose {
                label: format!("orbit yaw {yaw_deg} pitch 25 distance {distance:.1} m"),
                position: position.to_array(),
                look_at: center.to_array(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The capture job (over frames)
// ---------------------------------------------------------------------------

/// Shared with the upload thread. `done` flips exactly once, after the last
/// pose landed or the budget ran out; `paths` holds whatever was captured.
#[derive(Default, Debug)]
pub struct CaptureStatus {
    pub done: bool,
    pub paths: Vec<PathBuf>,
}

#[derive(Resource)]
pub struct PublishCaptureJob {
    poses: Vec<CapturePose>,
    out_dir: PathBuf,
    next: usize,
    waiting_on: Option<PathBuf>,
    last_size: Option<u64>,
    started: Instant,
    status: Arc<Mutex<CaptureStatus>>,
}

/// Start the orbit. Clears stale captures first so a previous publish's frames
/// can never be mistaken for this one's.
pub fn start_capture_job(
    world: &mut World,
    poses: Vec<CapturePose>,
    out_dir: PathBuf,
) -> Arc<Mutex<CaptureStatus>> {
    let status = Arc::new(Mutex::new(CaptureStatus::default()));
    let _ = std::fs::create_dir_all(&out_dir);
    for n in 0..8 {
        let _ = std::fs::remove_file(out_dir.join(format!("capture-{n}.png")));
    }
    if poses.is_empty() || world.get_resource::<AiCameraState>().is_none() {
        if let Ok(mut s) = status.lock() {
            s.done = true;
        }
        return status;
    }
    world.insert_resource(PublishCaptureJob {
        poses,
        out_dir,
        next: 0,
        waiting_on: None,
        last_size: None,
        started: Instant::now(),
        status: status.clone(),
    });
    status
}

pub struct PublishCapturePlugin;

impl Plugin for PublishCapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, drive_publish_captures);
    }
}

fn finish(job: &PublishCaptureJob, commands: &mut Commands) {
    if let Ok(mut s) = job.status.lock() {
        s.done = true;
    }
    commands.remove_resource::<PublishCaptureJob>();
}

/// One step per frame: wait for the in-flight PNG to land and settle, then
/// pose the AI camera for the next shot and queue its capture.
fn drive_publish_captures(
    mut commands: Commands,
    job: Option<ResMut<PublishCaptureJob>>,
    mut state: ResMut<AiCameraState>,
    mut cam: Query<&mut Transform, With<AiCamera>>,
) {
    let Some(mut job) = job else { return };

    if job.started.elapsed() > CAPTURE_BUDGET {
        warn!(
            "publish captures: budget exhausted after {} of {} poses; submitting with what landed",
            job.next.saturating_sub(usize::from(job.waiting_on.is_some())),
            job.poses.len()
        );
        finish(&job, &mut commands);
        return;
    }

    if let Some(path) = job.waiting_on.clone() {
        // The observer writes the PNG on a task pool; treat it as landed once
        // it exists and its size held still for a frame.
        let size = std::fs::metadata(&path).ok().map(|m| m.len()).filter(|&n| n > 0);
        match (size, job.last_size) {
            (Some(now), Some(prev)) if now == prev => {
                if let Ok(mut s) = job.status.lock() {
                    s.paths.push(path);
                }
                job.waiting_on = None;
                job.last_size = None;
            }
            (now, _) => {
                job.last_size = now;
                return;
            }
        }
    }

    if job.next >= job.poses.len() {
        info!("publish captures: {} pose(s) captured", job.poses.len());
        finish(&job, &mut commands);
        return;
    }

    let pose = job.poses[job.next].clone();
    let Ok(mut tf) = cam.single_mut() else {
        warn!("publish captures: AI camera missing; submitting without a capture set");
        finish(&job, &mut commands);
        return;
    };
    tf.translation = Vec3::from_array(pose.position);
    tf.look_at(Vec3::from_array(pose.look_at), Vec3::Y);
    let path = job.out_dir.join(format!("capture-{}.png", job.next));
    request_capture(&mut state, path.clone());
    job.waiting_on = Some(path);
    job.last_size = None;
    job.next += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_are_recognized() {
        assert!(is_default_name("Part", "Part"));
        assert!(is_default_name("Part (12)", "Part"));
        assert!(is_default_name("MeshPart 3", "MeshPart"));
        assert!(is_default_name("", "Part"));
        assert!(!is_default_name("Harbor crane", "Part"));
        assert!(!is_default_name("Partition wall", "Part"));
    }

    #[test]
    fn signals_find_links_contacts_and_phones_but_not_coordinates() {
        let mut s = Signals::default();
        scan_signals(
            "join https://example.com/x?y=1 or discord.gg/abc12 mail me ann.lee+x@mail.example.org call +1 (520) 555-0142 at 12.5, 300.25, 7",
            &mut s,
        );
        assert_eq!(s.urls, vec!["https://example.com/x?y=1"]);
        assert_eq!(s.discord_invites, vec!["discord.gg/abc12"]);
        assert_eq!(s.emails, vec!["ann.lee+x@mail.example.org"]);
        assert_eq!(s.phones, vec!["+1 (520) 555-0142"]);
        let mut none = Signals::default();
        scan_signals("position 1234.5 6789.0 and hash 0123456789abcdef", &mut none);
        assert!(none.phones.is_empty());
        assert!(none.urls.is_empty());
    }

    #[test]
    fn orbit_frames_the_scene_from_four_sides() {
        let b = Bounds { min: [-10.0, 0.0, -10.0], max: [10.0, 4.0, 10.0], extent_m: 20.0 };
        let poses = capture_poses(Some(&b));
        assert_eq!(poses.len(), CAPTURE_COUNT);
        for p in &poses {
            assert_eq!(p.look_at, [0.0, 2.0, 0.0]);
            let d = Vec3::from_array(p.position).distance(Vec3::from_array(p.look_at));
            assert!((d - 23.0).abs() < 0.05, "distance {d}");
            assert!(p.position[1] > 2.0, "every pose looks down onto the scene");
        }
        let empty = capture_poses(None);
        assert_eq!(empty.len(), CAPTURE_COUNT);
        assert!(Vec3::from_array(empty[0].position).length() >= 12.0);
    }
}
