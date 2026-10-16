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
use crate::ui::display_text::{shown_block, shown_line};

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
    /// `blake3:<hex>` of the committed world manifest. Filled by the upload
    /// thread once the world is baked; the World has no idea what it will hash to.
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
        app.init_resource::<ListingReviewState>()
            .init_resource::<ListingUiInbox>()
            // Idempotent; MultiplayerPlugin registers it too. Registered here
            // so the publish reporter's writer is valid in any app.
            .add_message::<crate::multiplayer::RefreshMyListings>()
            .add_systems(Update, (drive_publish_captures, report_publish_progress, refresh_listing_review).chain())
            .add_systems(Update, (register_listing_callbacks, sync_listing_review_ui, sync_publish_dialog_ui).chain().after(refresh_listing_review));
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


// ---------------------------------------------------------------------------
// The listing review, as Studio shows it to the author
// ---------------------------------------------------------------------------
//
// The Gallery decides a listing on its own clock: the judge in seconds, a
// person within a day, an appeal later still. Studio learns the outcome here,
// not only in the few seconds after a publish:
//
// - the latest review lives in `<universe>/.eustress/review.toml`, so a
//   restart or a reopened Universe shows it at once;
// - it is refreshed when a Universe opens, when the Publish dialog opens, and
//   every five minutes while the review is still waiting on something;
// - an author-facing change raises one notification.
//
// Everything stored and shown is the API's author view
// (`authorView` in infrastructure/cloudflare/api/src/moderation.mjs), whose
// strings are already author-safe. Held and quarantined listings both arrive
// as `in_review`; nothing here tries, or is able, to tell them apart.

use serde::Deserialize;

pub const REVIEW_FILE: &str = "review.toml";
/// How often a waiting review is re-read while Studio is open.
pub const REVIEW_POLL: Duration = Duration::from_secs(300);

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ReviewReason {
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub what_to_change: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ListingReview {
    pub sim_id: String,
    /// One of in_review, listed, approved_private, not_listed,
    /// changes_requested, appeal_in_review.
    pub status: String,
    pub headline: String,
    pub updated_at: String,
    /// The API this review was read from. Only review.toml carries it: `save`
    /// stamps it, `load` checks it and clears it, so a Studio pointed at
    /// another API (staging) never shows, or follows the listing id of,
    /// production's cached review. Empty in memory.
    #[serde(default)]
    pub api: String,
    #[serde(default)]
    pub can_appeal: bool,
    /// "All ages", "Teen (13+)", ... Set only for listed and approved_private.
    #[serde(default)]
    pub rating_label: String,
    #[serde(default)]
    pub suggested_edit: String,
    #[serde(default)]
    pub reasons: Vec<ReviewReason>,
}

/// Map whatever status the API sent to an author status. The current Worker
/// already sends author statuses; this also covers an older Worker that sent
/// internal ones, so no internal status can reach the author through Studio.
pub fn author_status(raw: &str, is_public: bool) -> &'static str {
    match raw {
        "listed" => "listed",
        "approved_private" => "approved_private",
        "not_listed" | "rejected" => "not_listed",
        "changes_requested" => "changes_requested",
        "appeal_in_review" | "appealed" => "appeal_in_review",
        "approved" if is_public => "listed",
        "approved" => "approved_private",
        // in_review, pending, classifying, held, quarantined, unreviewed and
        // anything unknown: all read as "in review".
        _ => "in_review",
    }
}

pub fn rating_label(rating: &str) -> &'static str {
    match rating {
        "all_ages" => "All ages",
        "teen_13" => "Teen (13+)",
        "mature_17" => "Mature (17+)",
        "adult_18" => "Adults only (18+)",
        _ => "",
    }
}

fn default_headline(status: &str) -> &'static str {
    match status {
        "listed" => "Your Universe is listed in the Gallery.",
        "approved_private" => "Your Universe passed review. It is private, so it is not shown in the Gallery.",
        "not_listed" => "Your Universe was not listed in the Gallery.",
        "changes_requested" => "Your Universe needs a few changes before it can be listed.",
        "appeal_in_review" => "Your appeal is with a reviewer.",
        _ => "We are reviewing your listing.",
    }
}

impl ListingReview {
    /// Build from the body of `GET /api/simulations/{id}/moderation`.
    pub fn from_api(sim_id: &str, body: &serde_json::Value, is_public: bool) -> Self {
        let s = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = author_status(body.get("status").and_then(|v| v.as_str()).unwrap_or(""), is_public).to_string();
        let decided = matches!(status.as_str(), "not_listed" | "changes_requested");
        let reasons = if decided {
            body.get("reasons")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|r| {
                            let t = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let title = t("title");
                            (!title.is_empty()).then(|| ReviewReason { category: t("category"), title, why: t("why"), what_to_change: t("what_to_change") })
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // The API's headline is author-safe only when it came with an author
        // status; an older Worker's internal status gets Studio's own line.
        let raw = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let headline = match s("headline") {
            h if !h.is_empty() && raw == status => h,
            _ => default_headline(&status).to_string(),
        };
        Self {
            api: String::new(),
            sim_id: sim_id.to_string(),
            rating_label: if matches!(status.as_str(), "listed" | "approved_private") { rating_label(&s("rating")).to_string() } else { String::new() },
            can_appeal: decided && body.get("can_appeal").and_then(|v| v.as_bool()).unwrap_or(false),
            suggested_edit: if decided { s("suggested_edit") } else { String::new() },
            updated_at: s("updated_at"),
            headline,
            reasons,
            status,
        }
    }

    /// Still waiting on the Gallery or on the author: worth re-reading.
    pub fn is_waiting(&self) -> bool {
        matches!(self.status.as_str(), "in_review" | "changes_requested" | "appeal_in_review")
    }

    /// The one line a notification or the publish progress shows.
    pub fn summary(&self) -> String {
        let first = self.reasons.first();
        match self.status.as_str() {
            "listed" if !self.rating_label.is_empty() => format!("Listed in the Gallery ({})", self.rating_label),
            "listed" => "Listed in the Gallery".to_string(),
            "approved_private" => "Passed review (private, not shown in the Gallery)".to_string(),
            "not_listed" => match first {
                Some(r) if !r.what_to_change.is_empty() => format!("Not listed: {}. {}", r.title, r.what_to_change),
                Some(r) => format!("Not listed: {}", r.title),
                None => "Not listed. See the review notes in your projects.".to_string(),
            },
            "changes_requested" => match first {
                Some(r) if !r.what_to_change.is_empty() => format!("Changes requested: {}", r.what_to_change),
                _ => "Changes requested before listing".to_string(),
            },
            "appeal_in_review" => "Appeal in review".to_string(),
            // The API's own line says whether the review is still running
            // ("under a minute") or a person has it. Both are author-safe, and
            // neither says why a case is with a person.
            _ if !self.headline.is_empty() => self.headline.clone(),
            _ => "Review pending".to_string(),
        }
    }

    /// The reasons under a decision, one Output line each: what is wrong, why,
    /// and what to change. Empty until there is a decision to explain.
    pub fn detail_lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        for r in &self.reasons {
            v.push(format!("  {}", r.title));
            if !r.why.is_empty() {
                v.push(format!("    Why: {}", r.why));
            }
            if !r.what_to_change.is_empty() {
                v.push(format!("    What to change: {}", r.what_to_change));
            }
        }
        if !self.suggested_edit.is_empty() {
            v.push(format!("  Suggested edit: {}", self.suggested_edit));
        }
        if self.can_appeal {
            v.push("  If you think this is a mistake, you can appeal from the Publish dialog or your projects page.".to_string());
        }
        v
    }

    /// "Updated 6:26 PM" for today (12-hour local time), "Updated Sep 25"
    /// before that, "" if unknown.
    pub fn updated_label(&self, now: chrono::DateTime<chrono::Local>) -> String {
        let Ok(at) = chrono::DateTime::parse_from_rfc3339(&self.updated_at) else { return String::new() };
        let local = at.with_timezone(&chrono::Local);
        if local.date_naive() == now.date_naive() {
            format!("Updated {}", local.format("%-I:%M %p"))
        } else {
            format!("Updated {}", local.format("%b %-d"))
        }
    }

    pub fn path(universe_root: &Path) -> PathBuf {
        universe_root.join(".eustress").join(REVIEW_FILE)
    }

    pub fn load(universe_root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(universe_root)).ok()?;
        let mut review: Self = toml::from_str(&text).ok()?;
        if !review_belongs_to_api(&review.api, &review_api_base(), eustress_common::api_base::PRODUCTION_API) {
            return None;
        }
        review.api.clear();
        Some(review)
    }

    pub fn save(&self, universe_root: &Path) -> Result<(), String> {
        let path = Self::path(universe_root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        let mut stamped = self.clone();
        stamped.api = review_api_base();
        let text = toml::to_string_pretty(&stamped).map_err(|e| format!("serialize review: {e}"))?;
        // Write-then-rename, so a crash mid-write never leaves a torn file
        // that the next open would silently drop.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("rename {}: {e}", path.display()))
    }
}

/// `GET /api/simulations/{id}/moderation`.
pub fn fetch_review(api: &str, sim_id: &str, token: &str, is_public: bool) -> Result<ListingReview, String> {
    let resp = ureq::get(&format!("{api}/api/simulations/{sim_id}/moderation"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("review status unavailable (HTTP {code})"),
            other => format!("review status unavailable: {other}"),
        })?;
    let body: serde_json::Value = resp.into_json().map_err(|e| format!("review status: bad JSON: {e}"))?;
    Ok(ListingReview::from_api(sim_id, &body, is_public))
}

/// `POST /api/simulations/{id}/appeal`. The error text on a refusal is the
/// API's own author-facing message, shown verbatim under the appeal box.
pub fn post_appeal(api: &str, sim_id: &str, token: &str, text: &str) -> Result<(), String> {
    match ureq::post(&format!("{api}/api/simulations/{sim_id}/appeal"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({ "text": text }).to_string())
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, resp)) => {
            let body: serde_json::Value = resp.into_json().unwrap_or_default();
            Err(body.get("error").and_then(|v| v.as_str()).map(str::to_string)
                .unwrap_or_else(|| format!("The appeal could not be sent (HTTP {code}).")))
        }
        Err(e) => Err(format!("The appeal could not be sent: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Telling the author how a publish went
// ---------------------------------------------------------------------------
//
// The publish runs on its own thread and reports through `PublishProgress`
// (file_event_handler.rs). Nothing used to read it: the dialog closes on click,
// failures went only to the log, and a first publish's review stayed hidden
// until the dialog was reopened. `report_publish_progress` reads it every
// frame and
//   - writes each stage to Output as it changes,
//   - on completion writes the result to Output and raises one notification
//     (the AI's decision with its reasons when it denies, "a person is
//     reviewing" when it holds, never the reason for a hold),
//   - hands the review to `ListingReviewState` at once, so the Publish dialog
//     and History show it without being reopened.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// What a finished publish says, for Output (every line) and a notification
/// (the one-line `toast`).
#[derive(Debug, Clone, PartialEq)]
pub struct PublishResult {
    pub level: PublishLevel,
    pub toast: String,
    pub lines: Vec<String>,
}

/// Map a finished publish to its text. Pure, so the wording is tested without
/// a window. Only author-facing review fields are read: a hold arrives as
/// `in_review` and says only that a person has it.
pub fn publish_result(p: &crate::ui::file_event_handler::PublishProgress) -> PublishResult {
    if let Some(e) = &p.error {
        let line = format!("Publish failed: {e}");
        return PublishResult { level: PublishLevel::Error, toast: line.clone(), lines: vec![line] };
    }
    if p.no_changes {
        let line = "Nothing changed since the last publish, so nothing was uploaded.".to_string();
        return PublishResult { level: PublishLevel::Info, toast: line.clone(), lines: vec![line] };
    }
    if let Some(r) = &p.review {
        let level = match r.status.as_str() {
            "listed" | "approved_private" => PublishLevel::Success,
            "not_listed" | "changes_requested" => PublishLevel::Warning,
            _ => PublishLevel::Info,
        };
        let id = p.sim_id.as_deref().unwrap_or("");
        let mut lines = vec![format!("Published {id}: {}", r.summary())];
        lines.extend(r.detail_lines());
        return PublishResult { level, toast: format!("Gallery review: {}", r.summary()), lines };
    }
    // Published, but review could not be started from Studio: the stage text
    // already says what happened and what it means for the listing.
    PublishResult { level: PublishLevel::Warning, toast: p.stage.clone(), lines: vec![p.stage.clone()] }
}

#[derive(Default)]
struct PublishSeen {
    handle: usize,
    stage: String,
    reported: bool,
}

/// A finished publish changed a listing on the server: it got there, and
/// something went up. The author's listings (Publish As's rows) are then
/// asked for again, so an open Publish dialog shows the update in place.
fn publish_changed_listing(p: &crate::ui::file_event_handler::PublishProgress) -> bool {
    p.error.is_none() && !p.no_changes && p.sim_id.is_some()
}

fn report_publish_progress(
    handle: Option<Res<crate::ui::file_event_handler::PublishProgressHandle>>,
    mut seen: Local<PublishSeen>,
    mut state: ResMut<ListingReviewState>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    mut notes: Option<ResMut<crate::notifications::NotificationManager>>,
    mut refresh: MessageWriter<crate::multiplayer::RefreshMyListings>,
) {
    let Some(handle) = handle else { return };
    // Each publish replaces the resource; a new allocation means a new publish.
    let id = Arc::as_ptr(&handle.0) as usize;
    if id != seen.handle {
        *seen = PublishSeen { handle: id, ..Default::default() };
    }
    if seen.reported {
        return;
    }
    // Never block a frame on the publish thread.
    let snapshot = match handle.0.try_lock() {
        Ok(p) => p.clone(),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().clone(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };

    if !snapshot.complete {
        if snapshot.stage != seen.stage {
            seen.stage = snapshot.stage.clone();
            if let Some(out) = output.as_mut() {
                out.info(format!("Publish: {}", snapshot.stage));
            }
        }
        return;
    }

    seen.reported = true;
    if publish_changed_listing(&snapshot) {
        refresh.write(crate::multiplayer::RefreshMyListings);
    }
    let result = publish_result(&snapshot);
    if let Some(out) = output.as_mut() {
        for (i, line) in result.lines.iter().enumerate() {
            // The headline carries the level; the reason lines under it read
            // as plain text.
            match (i, result.level) {
                (0, PublishLevel::Error) => out.error(line.clone()),
                (0, PublishLevel::Warning) => out.warn(line.clone()),
                _ => out.info(line.clone()),
            }
        }
    }
    if let Some(n) = notes.as_mut() {
        match result.level {
            PublishLevel::Success => n.success(result.toast.clone()),
            PublishLevel::Warning => n.warning(result.toast.clone()),
            PublishLevel::Error => n.error(result.toast.clone()),
            PublishLevel::Info => n.info(result.toast.clone()),
        }
    }
    // The review is shown without reopening the dialog. No second
    // notification: the one above is the author's.
    if let Some(r) = snapshot.review {
        state.adopt(r);
    }
}

/// Whether a cached review was written for the API in use. A file from before
/// the API was recorded can only have come from production.
pub fn review_belongs_to_api(file_api: &str, current: &str, production: &str) -> bool {
    if file_api.trim().is_empty() {
        current == production
    } else {
        file_api == current
    }
}

/// How `review.toml` changed since the last look. The first look only sets the
/// baseline, so a file that was already there when Studio opened is not news;
/// a file that APPEARS afterwards (the first publish of a Universe) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewFileChange {
    Baseline,
    Unchanged,
    Changed,
}

pub fn review_file_change(
    baselined: bool,
    last: Option<std::time::SystemTime>,
    now: Option<std::time::SystemTime>,
) -> ReviewFileChange {
    if !baselined {
        ReviewFileChange::Baseline
    } else if now.is_some() && now != last {
        ReviewFileChange::Changed
    } else {
        ReviewFileChange::Unchanged
    }
}

// ---------------------------------------------------------------------------
// Asking for a review again
// ---------------------------------------------------------------------------
//
// A review that has sat "in review" for a long time is usually stuck, not slow
// (a stalled first run left Box Head there on staging, and Studio had no way
// to ask again). `POST /api/simulations/{id}/submit` is the author's request
// to review. The Worker starts a run only when there is something to run (a
// stuck review qualifies after five minutes), spaces and caps the requests,
// and answers a held or quarantined case exactly as it answers any other
// in-review one, so nothing here can tell them apart or needs to.

fn rereview_after() -> chrono::Duration {
    // A little longer than the server's five minutes, so the button is never
    // offered for a request the server would only answer with "not yet".
    chrono::Duration::minutes(6)
}

/// Whether to offer "Request review again" for this review at `now`.
pub fn can_request_review_again(review: &ListingReview, now: chrono::DateTime<chrono::Utc>) -> bool {
    if review.status != "in_review" || review.sim_id.is_empty() {
        return false;
    }
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(&review.updated_at) else { return false };
    now.signed_duration_since(at.with_timezone(&chrono::Utc)) >= rereview_after()
}

/// What `POST /submit` answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubmitAnswer {
    pub code: u16,
    /// A review actually ran (or is running), as opposed to "already on it".
    pub rerun: bool,
    pub status: String,
    /// The API's own author-facing text on a refusal, verbatim.
    pub error: String,
}

/// Read what `POST /submit` answered. A 202 means a review started and is
/// still running, which is also all an older Worker (no `rerun` field) ever
/// said, so a missing field is read from the code rather than assumed false.
pub fn submit_answer(code: u16, body: &serde_json::Value) -> SubmitAnswer {
    SubmitAnswer {
        code,
        rerun: body.get("rerun").and_then(|v| v.as_bool()).unwrap_or(code == 202),
        status: body.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        error: body.get("error").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    }
}

pub fn post_submit(api: &str, sim_id: &str, token: &str) -> Result<SubmitAnswer, String> {
    // No client-side timeout on purpose: the Worker holds this response while
    // the review runs, which can take a minute.
    match ureq::post(&format!("{api}/api/simulations/{sim_id}/submit"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(resp) => {
            let code = resp.status();
            let body: serde_json::Value = resp.into_json().unwrap_or_default();
            Ok(submit_answer(code, &body))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body: serde_json::Value = resp.into_json().unwrap_or_default();
            Ok(submit_answer(code, &body))
        }
        Err(e) => Err(format!("The request could not be sent: {e}")),
    }
}

/// The one line shown under the button. Never says why a review is stuck.
pub fn rerequest_note(a: &SubmitAnswer) -> String {
    match a.code {
        200 | 202 if a.rerun => "Review requested. This usually takes under a minute.".to_string(),
        200 | 202 => "A reviewer is already working on it.".to_string(),
        429 if a.error.is_empty() => "Try again in a minute.".to_string(),
        429 => a.error.clone(),
        code if a.error.is_empty() => format!("The request could not be sent (HTTP {code})."),
        _ => a.error.clone(),
    }
}

enum ReviewJob {
    Fetched(Result<ListingReview, String>),
    Appealed(Result<(), String>),
    Resubmitted(Result<SubmitAnswer, String>),
}

/// The Studio side of the listing review: what is shown, and what is in
/// flight. The Publish dialog and History read `review`; `dirty` tells the UI
/// sync to push it.
#[derive(Resource, Default)]
pub struct ListingReviewState {
    pub review: Option<ListingReview>,
    pub dirty: bool,
    pub appeal_busy: bool,
    pub appeal_error: String,
    pub rerequest_busy: bool,
    pub rerequest_note: String,
    pending_rerequest: bool,
    refresh_requested: bool,
    pending_appeal: Option<String>,
    last_fetch: Option<Instant>,
    last_file_check: Option<Instant>,
    file_mtime: Option<std::time::SystemTime>,
    file_baselined: bool,
    universe_root: Option<PathBuf>,
    inflight: Option<Arc<Mutex<Option<ReviewJob>>>>,
}

impl ListingReviewState {
    /// Re-read the review now (the Publish dialog opened, a publish finished).
    pub fn request_refresh(&mut self) {
        self.refresh_requested = true;
    }

    /// Send an appeal. The UI only offers it with 10 to 2000 characters.
    pub fn request_appeal(&mut self, text: String) {
        self.appeal_error.clear();
        self.appeal_busy = true;
        self.pending_appeal = Some(text);
        self.dirty = true;
    }

    /// Ask for the review again (the "Request review again" button).
    pub fn request_rereview(&mut self) {
        self.rerequest_note.clear();
        self.rerequest_busy = true;
        self.pending_rerequest = true;
        self.dirty = true;
    }

    /// Adopt a review the publish thread just wrote.
    pub fn adopt(&mut self, review: ListingReview) {
        self.review = Some(review);
        self.last_fetch = Some(Instant::now());
        self.dirty = true;
    }

    /// The URL "Open on eustress.dev" opens, when there is a listing.
    pub fn review_url(&self) -> Option<String> {
        let id = &self.review.as_ref()?.sim_id;
        if id.is_empty() {
            return None;
        }
        Some(format!("{}/projects?review={}", review_site_base()?, urlencoding::encode(id)))
    }
}

/// The API the review is read from: the shared base, so `EUSTRESS_API_URL`
/// points the review at staging exactly as it does publishing.
pub fn review_api_base() -> String {
    eustress_common::api_base::api_base().to_string()
}

/// The site "Open on eustress.dev" opens: the website of the API the listing
/// lives on (`eustress_common::api_base::site_base`), or none for an API with
/// no known site, so a staging listing never opens production's site.
pub fn review_site_base() -> Option<String> {
    eustress_common::api_base::site_base()
}

/// The listing id this Universe publishes into.
fn listing_id(universe_root: &Path, review: Option<&ListingReview>) -> Option<String> {
    if let Some(r) = review.filter(|r| !r.sim_id.is_empty()) {
        return Some(r.sim_id.clone());
    }
    eustress_common::load_toml_file::<eustress_common::SyncManifest>(&universe_root.join(".eustress").join("sync.toml"))
        .ok()
        .and_then(|s| s.remote.listing_id().map(str::to_string))
}

fn listing_is_public(universe_root: &Path) -> bool {
    eustress_common::load_toml_file::<eustress_common::PublishManifest>(&universe_root.join(".eustress").join("publish.toml"))
        .map(|m| m.visibility.is_public)
        .unwrap_or(true)
}

/// Poll finished jobs, then start the next one: an appeal first, else a
/// refresh when one was asked for, the Universe changed, or a waiting review
/// is due. One request in flight at a time, on its own thread.
pub fn refresh_listing_review(
    mut state: ResMut<ListingReviewState>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut notes: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    // A different Universe: show what it last knew, then ask for fresh.
    let universe = space_root
        .as_ref()
        .map(|sr| crate::space::universe_root_for_path(&sr.0).unwrap_or_else(|| sr.0.clone()));
    if universe != state.universe_root {
        state.review = universe.as_deref().and_then(ListingReview::load);
        state.universe_root = universe.clone();
        state.file_mtime = None;
        state.file_baselined = false;
        state.last_file_check = None;
        state.appeal_error.clear();
        state.appeal_busy = false;
        state.pending_appeal = None;
        state.rerequest_busy = false;
        state.rerequest_note.clear();
        state.pending_rerequest = false;
        state.inflight = None;
        state.dirty = true;
        state.refresh_requested = true;
    }
    let Some(universe) = universe else { return };

    // review.toml changed on disk: the publish thread wrote it, or another
    // Studio on this Universe did. The first look only sets the baseline, so a
    // file already there at open is not news; one that APPEARS afterwards (the
    // first publish of a Universe) is, and used to be skipped as "first look".
    if state.last_file_check.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
        state.last_file_check = Some(Instant::now());
        let mtime = std::fs::metadata(ListingReview::path(&universe)).and_then(|m| m.modified()).ok();
        match review_file_change(state.file_baselined, state.file_mtime, mtime) {
            ReviewFileChange::Baseline => {
                state.file_mtime = mtime;
                state.file_baselined = true;
            }
            ReviewFileChange::Changed => {
                state.file_mtime = mtime;
                if let Some(r) = ListingReview::load(&universe) {
                    if state.review.as_ref() != Some(&r) {
                        state.adopt(r);
                    }
                }
            }
            ReviewFileChange::Unchanged => {}
        }
    }

    if let Some(slot) = state.inflight.clone() {
        let done = slot.lock().ok().and_then(|mut s| s.take());
        let Some(job) = done else { return };
        state.inflight = None;
        match job {
            ReviewJob::Fetched(Ok(fresh)) => {
                let changed = state.review.as_ref().map(|old| old.updated_at != fresh.updated_at || old.status != fresh.status);
                if changed == Some(true) {
                    if let Some(n) = notes.as_mut() {
                        match fresh.status.as_str() {
                            "listed" | "approved_private" => n.success(format!("Gallery review: {}", fresh.summary())),
                            "not_listed" | "changes_requested" => n.warning(format!("Gallery review: {}", fresh.summary())),
                            _ => n.info(format!("Gallery review: {}", fresh.summary())),
                        }
                    }
                }
                if let Err(e) = fresh.save(&universe) {
                    warn!("listing review: {e}");
                }
                if changed == Some(true) {
                    state.rerequest_note.clear();
                }
                state.review = Some(fresh);
                state.dirty = true;
            }
            ReviewJob::Fetched(Err(e)) => debug!("listing review: {e}"),
            ReviewJob::Appealed(Ok(())) => {
                if let Some(r) = state.review.as_mut() {
                    r.status = "appeal_in_review".into();
                    r.headline = default_headline("appeal_in_review").into();
                    r.can_appeal = false;
                    let _ = r.save(&universe);
                }
                state.appeal_busy = false;
                state.dirty = true;
                state.refresh_requested = true;
            }
            ReviewJob::Appealed(Err(e)) => {
                state.appeal_error = e;
                state.appeal_busy = false;
                state.dirty = true;
            }
            ReviewJob::Resubmitted(result) => {
                state.rerequest_busy = false;
                state.dirty = true;
                match result {
                    Ok(answer) => {
                        state.rerequest_note = rerequest_note(&answer);
                        // A run that happened has an answer to read now.
                        if answer.rerun {
                            state.refresh_requested = true;
                        }
                    }
                    Err(e) => state.rerequest_note = e,
                }
            }
        }
        return;
    }

    let due = state.review.as_ref().is_some_and(|r| r.is_waiting())
        && state.last_fetch.map_or(true, |t| t.elapsed() >= REVIEW_POLL);
    if state.pending_appeal.is_none() && !state.pending_rerequest && !state.refresh_requested && !due {
        return;
    }
    let Some(token) = auth.as_ref().and_then(|a| a.token.clone()) else {
        state.refresh_requested = false;
        if state.pending_appeal.take().is_some() {
            state.appeal_busy = false;
            state.appeal_error = "Sign in to send an appeal.".into();
            state.dirty = true;
        }
        if std::mem::take(&mut state.pending_rerequest) {
            state.rerequest_busy = false;
            state.rerequest_note = "Sign in to request a review.".into();
            state.dirty = true;
        }
        return;
    };
    let Some(sim_id) = listing_id(&universe, state.review.as_ref()) else {
        state.refresh_requested = false;
        state.pending_appeal = None;
        if std::mem::take(&mut state.pending_rerequest) {
            state.rerequest_busy = false;
            state.dirty = true;
        }
        return;
    };

    let slot = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let api = review_api_base();
    if let Some(text) = state.pending_appeal.take() {
        std::thread::spawn(move || {
            let r = post_appeal(&api, &sim_id, &token, &text);
            if let Ok(mut s) = out.lock() { *s = Some(ReviewJob::Appealed(r)); }
        });
    } else if std::mem::take(&mut state.pending_rerequest) {
        std::thread::spawn(move || {
            let r = post_submit(&api, &sim_id, &token);
            if let Ok(mut s) = out.lock() { *s = Some(ReviewJob::Resubmitted(r)); }
        });
    } else {
        state.refresh_requested = false;
        state.last_fetch = Some(Instant::now());
        let is_public = listing_is_public(&universe);
        std::thread::spawn(move || {
            let r = fetch_review(&api, &sim_id, &token, is_public);
            if let Ok(mut s) = out.lock() { *s = Some(ReviewJob::Fetched(r)); }
        });
    }
    state.inflight = Some(slot);
}


// ---------------------------------------------------------------------------
// The listing-status row (Publish dialog and History)
// ---------------------------------------------------------------------------
//
// UI's `ListingStatusRow` (listing_status.slint) binds the root `listing-*`
// properties on StudioWindow; this pushes the review into them and takes its
// two actions back. Same shape as `terrain_tools_ui`: callbacks hooked once
// the window exists, messages drained on the ECS side.

use crate::ui::slint_ui::{ListingReason, StudioWindow};
use slint::{ModelRc, SharedString, VecModel};

enum ListingUiMsg {
    Appeal(String),
    OpenReview,
    Rerequest,
}

#[derive(Resource, Default)]
pub struct ListingUiInbox(Arc<Mutex<Vec<ListingUiMsg>>>);

fn register_listing_callbacks(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    inbox: Res<ListingUiInbox>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(slint) = slint else { return };
    let ui: &StudioWindow = &slint.window;
    let q = inbox.0.clone();
    ui.on_listing_appeal(move |text: SharedString| {
        if let Ok(mut q) = q.lock() {
            q.push(ListingUiMsg::Appeal(text.to_string()));
        }
    });
    let q = inbox.0.clone();
    ui.on_listing_open_review(move || {
        if let Ok(mut q) = q.lock() {
            q.push(ListingUiMsg::OpenReview);
        }
    });
    let q = inbox.0.clone();
    ui.on_listing_rerequest(move || {
        if let Ok(mut q) = q.lock() {
            q.push(ListingUiMsg::Rerequest);
        }
    });
    *done = true;
}

/// Drain the row's actions, refresh when the Publish dialog opens, and push
/// the review into the properties whenever it changed.
fn sync_listing_review_ui(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    inbox: Res<ListingUiInbox>,
    mut state: ResMut<ListingReviewState>,
    mut dialog_was_open: Local<bool>,
    mut can_rerequest_shown: Local<Option<bool>>,
    mut notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(slint) = slint else { return };
    let ui: &StudioWindow = &slint.window;

    let msgs: Vec<ListingUiMsg> = inbox.0.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
    for msg in msgs {
        match msg {
            // The row only enables Send between 10 and 2000 characters; the
            // API checks again and its refusal shows under the box.
            ListingUiMsg::Appeal(text) => state.request_appeal(text),
            ListingUiMsg::Rerequest => state.request_rereview(),
            ListingUiMsg::OpenReview => {
                if let Some(url) = state.review_url() {
                    if let Err(e) = open::that(&url) {
                        warn!("listing review: could not open {url}: {e}");
                    }
                } else if review_site_base().is_none() {
                    let message = format!(
                        "No website is known for {}; set {} to open this listing's page.",
                        review_api_base(),
                        eustress_common::api_base::SITE_URL_VAR
                    );
                    warn!("listing review: {message}");
                    if let Some(n) = notifications.as_mut() {
                        n.warning(message);
                    }
                }
            }
        }
    }

    let open = ui.get_show_publish_dialog();
    if open && !*dialog_was_open {
        state.request_refresh();
    }
    *dialog_was_open = open;

    // Time passes without the review changing: the button appears once a
    // waiting review has waited long enough, so this is checked every frame.
    let can_rerequest = !state.rerequest_busy
        && state.review.as_ref().is_some_and(|r| can_request_review_again(r, chrono::Utc::now()));
    if *can_rerequest_shown != Some(can_rerequest) {
        *can_rerequest_shown = Some(can_rerequest);
        ui.set_listing_can_rerequest(can_rerequest);
    }

    if !state.dirty {
        return;
    }
    state.dirty = false;
    match state.review.as_ref() {
        Some(r) => {
            ui.set_listing_status(shown_line(&r.status));
            ui.set_listing_headline(shown_line(&r.headline));
            ui.set_listing_rating(r.rating_label.as_str().into());
            ui.set_listing_updated(r.updated_label(chrono::Local::now()).into());
            let reasons: Vec<ListingReason> = r
                .reasons
                .iter()
                .map(|x| ListingReason {
                    category: x.category.as_str().into(),
                    title: shown_block(&x.title),
                    why: shown_block(&x.why),
                    what_to_change: shown_block(&x.what_to_change),
                })
                .collect();
            ui.set_listing_reasons(ModelRc::new(VecModel::from(reasons)));
            ui.set_listing_suggested_edit(shown_block(&r.suggested_edit));
            ui.set_listing_can_appeal(r.can_appeal && !state.appeal_busy);
        }
        None => {
            // "" is the row's "Not published yet".
            ui.set_listing_status(SharedString::new());
            ui.set_listing_headline(SharedString::new());
            ui.set_listing_rating(SharedString::new());
            ui.set_listing_updated(SharedString::new());
            ui.set_listing_reasons(ModelRc::new(VecModel::from(Vec::<ListingReason>::new())));
            ui.set_listing_suggested_edit(SharedString::new());
            ui.set_listing_can_appeal(false);
        }
    }
    ui.set_listing_appeal_busy(state.appeal_busy);
    ui.set_listing_appeal_error(shown_block(&state.appeal_error));
    ui.set_listing_rerequest_busy(state.rerequest_busy);
    ui.set_listing_rerequest_note(shown_block(&state.rerequest_note));
}

// ---------------------------------------------------------------------------
// Publish progress inside the dialog
// ---------------------------------------------------------------------------
//
// The dialog stays open while a publish runs (UI's decision). While it runs,
// its progress line replaces the "Target: ..." subtitle; when it fails, the
// error shows in the dialog's error area. When it finishes, the result is the
// listing row, which `report_publish_progress` already fed. `sync-status` is
// not touched: slint_ui.rs rewrites it every frame as the account label.

/// "Uploading Universe... 45%". No percent until there is one.
pub fn publish_progress_text(stage: &str, percent: f32) -> String {
    let stage = stage.trim();
    if !percent.is_finite() || percent <= 0.0 {
        return stage.to_string();
    }
    format!("{stage} {:.0}%", percent.clamp(0.0, 100.0))
}

/// (is publishing, progress line, error line) for the dialog.
pub fn publish_dialog_state(p: &crate::ui::file_event_handler::PublishProgress) -> (bool, String, String) {
    if !p.complete {
        (true, publish_progress_text(&p.stage, p.percent), String::new())
    } else {
        (false, String::new(), p.error.clone().unwrap_or_default())
    }
}

#[derive(Default)]
struct PublishDialogShown {
    running: Option<bool>,
    progress: String,
    error: String,
    dialog_was_open: bool,
    /// The publish whose error was already on screen when the dialog was
    /// reopened: a failure from earlier is not shown again days later.
    error_seen_for: usize,
}

fn sync_publish_dialog_ui(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    handle: Option<Res<crate::ui::file_event_handler::PublishProgressHandle>>,
    mut shown: Local<PublishDialogShown>,
) {
    let Some(slint) = slint else { return };
    let ui: &StudioWindow = &slint.window;

    let (id, state) = match handle.as_ref() {
        Some(h) => {
            let id = Arc::as_ptr(&h.0) as usize;
            let state = match h.0.try_lock() {
                Ok(p) => publish_dialog_state(&p),
                Err(std::sync::TryLockError::Poisoned(e)) => publish_dialog_state(&e.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => return,
            };
            (id, state)
        }
        None => (0, (false, String::new(), String::new())),
    };
    let (running, progress, mut error) = state;

    // Reopening the dialog on a publish that already finished starts it clean.
    let open = ui.get_show_publish_dialog();
    if open && !shown.dialog_was_open && !running {
        shown.error_seen_for = id;
    }
    shown.dialog_was_open = open;
    if shown.error_seen_for == id && id != 0 && !running {
        error.clear();
    }

    if shown.running != Some(running) {
        shown.running = Some(running);
        ui.set_publish_is_publishing(running);
    }
    if shown.progress != progress {
        ui.set_publish_progress(progress.as_str().into());
        shown.progress = progress;
    }
    if shown.error != error {
        ui.set_publish_error(shown_block(&error));
        shown.error = error;
    }
}

// ---------------------------------------------------------------------------
// The listing's thumbnail
// ---------------------------------------------------------------------------
//
// The thumbnail is the hero image of the Gallery card, so it is one of the
// publish orbit's off-screen renders: the AI camera's 1280x720 image of the
// scene, with no Studio window, panel or dialog in it. It is never a capture
// of the window. An author's own picture in `<universe>/.eustress/thumbnail.*`
// wins over the generated one.

/// The smallest thumbnail the Gallery takes: the AI camera's own size, 16:9.
pub const THUMBNAIL_MIN_WIDTH: u32 = 1280;
pub const THUMBNAIL_MIN_HEIGHT: u32 = 720;
/// The previous thumbnail was a 512x288 screenshot of the whole Studio window.
const LEGACY_WINDOW_THUMBNAIL: (u32, u32) = (512, 288);
/// Where the generated thumbnail is kept, next to an author's own.
pub const AUTO_THUMBNAIL_FILE: &str = "thumbnail-auto.png";

/// The orbit capture with the most detail, taken as the largest file: a PNG of
/// bare sky or one wall compresses small, a view of the built scene does not.
/// Ties go to the earlier pose, so the choice is stable.
pub fn best_capture(captures: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(u64, &PathBuf)> = None;
    for path in captures {
        let Some(len) = std::fs::metadata(path).ok().map(|m| m.len()).filter(|&n| n > 0) else { continue };
        if best.map_or(true, |(b, _)| len > b) {
            best = Some((len, path));
        }
    }
    best.map(|(_, p)| p.clone())
}

/// Write the listing thumbnail from the best of `captures` to `out`: a PNG at
/// 16:9 and at least 1280x720. A capture is never scaled up; one that is too
/// small is refused, and the listing keeps the thumbnail it has.
pub fn write_listing_thumbnail(captures: &[PathBuf], out: &Path) -> Result<(), String> {
    let source = best_capture(captures).ok_or_else(|| "no capture to make a thumbnail from".to_string())?;
    let (w, h) = image::image_dimensions(&source).map_err(|e| format!("read {}: {e}", source.display()))?;
    if w < THUMBNAIL_MIN_WIDTH || h < THUMBNAIL_MIN_HEIGHT {
        return Err(format!("the capture is {w}x{h}, smaller than {THUMBNAIL_MIN_WIDTH}x{THUMBNAIL_MIN_HEIGHT}"));
    }
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Already 16:9: the capture is the thumbnail.
    if w * 9 == h * 16 {
        std::fs::copy(&source, out).map_err(|e| format!("copy to {}: {e}", out.display()))?;
        return Ok(());
    }
    // Otherwise the centre, cropped to 16:9.
    let (cw, ch) = if w * 9 > h * 16 { (h * 16 / 9, h) } else { (w, w * 9 / 16) };
    if cw < THUMBNAIL_MIN_WIDTH || ch < THUMBNAIL_MIN_HEIGHT {
        return Err(format!("the 16:9 centre of a {w}x{h} capture is smaller than {THUMBNAIL_MIN_WIDTH}x{THUMBNAIL_MIN_HEIGHT}"));
    }
    let img = image::open(&source).map_err(|e| format!("decode {}: {e}", source.display()))?;
    img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch)
        .save(out)
        .map_err(|e| format!("write {}: {e}", out.display()))
}

/// Whether `path` is the old window-screenshot thumbnail: exactly 512x288,
/// which a picture an author chose is unlikely to be.
pub fn is_legacy_window_thumbnail(path: &Path) -> bool {
    image::image_dimensions(path).map(|d| d == LEGACY_WINDOW_THUMBNAIL).unwrap_or(false)
}

/// Remove an old window-screenshot thumbnail from `state_dir` (`<universe>/.eustress`),
/// so the upload step does not send it as the author's own.
pub fn remove_legacy_window_thumbnail(state_dir: &Path) {
    for ext in ["png", "jpg", "webp"] {
        let path = state_dir.join(format!("thumbnail.{ext}"));
        if path.exists() && is_legacy_window_thumbnail(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Whether the author keeps their own thumbnail picture in `state_dir`.
pub fn has_author_thumbnail(state_dir: &Path) -> bool {
    ["png", "jpg", "webp"].iter().any(|ext| state_dir.join(format!("thumbnail.{ext}")).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_statuses_never_reveal_more_than_in_review() {
        for raw in ["in_review", "pending", "classifying", "held", "quarantined", "unreviewed", "", "something_new"] {
            assert_eq!(author_status(raw, true), "in_review", "{raw}");
        }
        assert_eq!(author_status("approved", true), "listed");
        assert_eq!(author_status("approved", false), "approved_private");
        assert_eq!(author_status("rejected", true), "not_listed");
        assert_eq!(author_status("appealed", true), "appeal_in_review");
    }

    #[test]
    fn review_from_an_old_worker_uses_studio_wording() {
        let body = serde_json::json!({ "status": "quarantined", "headline": "under legal review", "reasons": [{ "code": "under_legal_review" }] });
        let r = ListingReview::from_api("s1", &body, true);
        assert_eq!(r.status, "in_review");
        assert_eq!(r.headline, "We are reviewing your listing.");
        assert!(r.reasons.is_empty());
        assert!(!r.can_appeal);
        assert_eq!(r.summary(), "We are reviewing your listing.");
        assert!(!format!("{r:?}").to_lowercase().contains("legal"));
    }

    #[test]
    fn review_from_the_author_view() {
        let body = serde_json::json!({
            "status": "not_listed", "headline": "Your Universe was not listed in the Gallery.",
            "reasons": [{ "category": "quality_asset_flip", "title": "Reads as an unmodified template or asset pack",
                          "why": "The Gallery lists original work, however simple.", "what_to_change": "Add your own arrangement." }],
            "suggested_edit": "Restyle the plaza.", "can_appeal": true, "rating": "teen_13", "updated_at": "2026-09-26T09:14:00Z"
        });
        let r = ListingReview::from_api("s1", &body, true);
        assert_eq!(r.status, "not_listed");
        assert_eq!(r.reasons.len(), 1);
        assert!(r.can_appeal);
        assert_eq!(r.rating_label, "", "a rating is shown only once listed");
        assert_eq!(r.summary(), "Not listed: Reads as an unmodified template or asset pack. Add your own arrangement.");
        assert!(r.is_waiting() == false);

        let listed = ListingReview::from_api("s1", &serde_json::json!({ "status": "listed", "rating": "teen_13", "reasons": [{ "title": "x" }], "can_appeal": true }), true);
        assert_eq!(listed.rating_label, "Teen (13+)");
        assert!(listed.reasons.is_empty() && !listed.can_appeal);
        assert_eq!(listed.summary(), "Listed in the Gallery (Teen (13+))");
    }

    #[test]
    fn review_round_trips_through_review_toml_and_labels_its_time() {
        let dir = std::env::temp_dir().join(format!("eustress-review-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = ListingReview {
            sim_id: "s1".into(), status: "changes_requested".into(), headline: "h".into(),
            updated_at: chrono::Local::now().to_rfc3339(), api: String::new(), can_appeal: true, rating_label: String::new(),
            suggested_edit: "e".into(),
            reasons: vec![ReviewReason { category: "coppa_links".into(), title: "t".into(), why: "w".into(), what_to_change: "c".into() }],
        };
        r.save(&dir).unwrap();
        assert_eq!(ListingReview::load(&dir), Some(r.clone()));
        assert!(r.is_waiting());
        assert!(r.updated_label(chrono::Local::now()).starts_with("Updated ") && r.updated_label(chrono::Local::now()).contains(':'));
        let old = ListingReview { updated_at: "2026-01-05T12:00:00Z".into(), ..r };
        assert_eq!(old.updated_label(chrono::Local::now()), "Updated Jan 5");
        let at = |h: u32, m: u32| {
            let today = chrono::Local::now().date_naive().and_hms_opt(h, m, 0).unwrap();
            today.and_local_timezone(chrono::Local).earliest().unwrap()
        };
        let evening = ListingReview { updated_at: at(18, 26).to_rfc3339(), ..old.clone() };
        assert_eq!(evening.updated_label(at(18, 30)), "Updated 6:26 PM", "12-hour local time");
        let morning = ListingReview { updated_at: at(9, 5).to_rfc3339(), ..old.clone() };
        assert_eq!(morning.updated_label(at(9, 30)), "Updated 9:05 AM");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        // The empty-scene orbit is exactly the 12 m floor by construction, but
        // the f32 unit direction's length rounds just under 1, so compare with
        // a tolerance rather than at the boundary.
        let d = Vec3::from_array(empty[0].position).length();
        assert!((d - 12.0).abs() < 1e-3, "empty-scene orbit distance {d}");
    }

    fn finished(p: impl FnOnce(&mut crate::ui::file_event_handler::PublishProgress)) -> crate::ui::file_event_handler::PublishProgress {
        let mut x = crate::ui::file_event_handler::PublishProgress { complete: true, percent: 100.0, ..Default::default() };
        p(&mut x);
        x
    }

    fn review_from(body: serde_json::Value) -> ListingReview {
        ListingReview::from_api("sim-1", &body, true)
    }

    #[test]
    fn a_failed_publish_says_so_and_why() {
        let r = publish_result(&finished(|p| { p.stage = "Failed".into(); p.error = Some("Create listing failed: timeout".into()); }));
        assert_eq!(r.level, PublishLevel::Error);
        assert_eq!(r.toast, "Publish failed: Create listing failed: timeout");
        assert_eq!(r.lines, vec!["Publish failed: Create listing failed: timeout".to_string()]);
    }

    #[test]
    fn an_unchanged_publish_is_not_an_error() {
        let r = publish_result(&finished(|p| { p.stage = "No changes since the last publish".into(); p.no_changes = true; }));
        assert_eq!(r.level, PublishLevel::Info);
        assert!(r.toast.starts_with("Nothing changed"));
    }

    #[test]
    fn an_approval_is_a_success_with_the_rating() {
        let rev = review_from(serde_json::json!({ "status": "listed", "rating": "teen_13", "headline": "Your Universe is listed in the Gallery." }));
        let r = publish_result(&finished(|p| { p.sim_id = Some("sim-1".into()); p.review = Some(rev); }));
        assert_eq!(r.level, PublishLevel::Success);
        assert_eq!(r.toast, "Gallery review: Listed in the Gallery (Teen (13+))");
        assert_eq!(r.lines, vec!["Published sim-1: Listed in the Gallery (Teen (13+))".to_string()]);
    }

    #[test]
    fn a_denial_names_what_to_change_in_output_and_in_the_toast() {
        let rev = review_from(serde_json::json!({
            "status": "not_listed", "headline": "Your Universe was not listed in the Gallery.",
            "reasons": [{ "category": "quality_asset_flip", "title": "Reads as an unmodified template or asset pack",
                          "why": "The Gallery lists original work, however simple.", "what_to_change": "Add your own arrangement." }],
            "suggested_edit": "Restyle the plaza.", "can_appeal": true
        }));
        let r = publish_result(&finished(|p| { p.sim_id = Some("sim-1".into()); p.review = Some(rev); }));
        assert_eq!(r.level, PublishLevel::Warning);
        assert_eq!(r.toast, "Gallery review: Not listed: Reads as an unmodified template or asset pack. Add your own arrangement.");
        let text = r.lines.join("\n");
        assert!(text.contains("Why: The Gallery lists original work, however simple."));
        assert!(text.contains("What to change: Add your own arrangement."));
        assert!(text.contains("Suggested edit: Restyle the plaza."));
        assert!(text.contains("appeal"));
    }

    #[test]
    fn changes_requested_is_a_warning_with_the_change() {
        let rev = review_from(serde_json::json!({
            "status": "changes_requested",
            "reasons": [{ "category": "coppa_links", "title": "Links in a children's experience", "what_to_change": "Remove the links." }]
        }));
        let r = publish_result(&finished(|p| { p.sim_id = Some("s".into()); p.review = Some(rev); }));
        assert_eq!(r.level, PublishLevel::Warning);
        assert_eq!(r.toast, "Gallery review: Changes requested: Remove the links.");
    }

    #[test]
    fn a_hold_says_a_person_is_reviewing_and_never_why() {
        // An old Worker's internal statuses must read the same as the new one's.
        for status in ["held", "quarantined", "pending", "classifying"] {
            let rev = review_from(serde_json::json!({ "status": status, "headline": "internal text about legal review", "reasons": [{ "code": "judge_flagged", "title": "csam" }] }));
            let r = publish_result(&finished(|p| { p.sim_id = Some("s".into()); p.review = Some(rev); }));
            assert_eq!(r.level, PublishLevel::Info, "{status}");
            let all = format!("{} {}", r.toast, r.lines.join(" ")).to_lowercase();
            for banned in ["legal", "quarantin", "csam", "judge", "held", "internal"] {
                assert!(!all.contains(banned), "{status}: {all}");
            }
            assert!(r.lines.len() == 1, "no reason lines for a review that is not decided: {status}");
        }
        // The new Worker's own line keeps the distinction it intends: still running vs a person has it.
        let running = review_from(serde_json::json!({ "status": "in_review", "headline": "We are reviewing your listing. This usually takes under a minute." }));
        assert_eq!(running.summary(), "We are reviewing your listing. This usually takes under a minute.");
        let person = review_from(serde_json::json!({ "status": "in_review", "headline": "A person is reviewing your listing. You will hear from us when it is decided." }));
        assert_eq!(person.summary(), "A person is reviewing your listing. You will hear from us when it is decided.");
    }

    #[test]
    fn a_published_listing_whose_review_never_started_says_so() {
        let r = publish_result(&finished(|p| {
            p.sim_id = Some("sim-1".into());
            p.stage = "Published sim-1. The Gallery review could not be started from Studio (submit: timeout). The listing stays unlisted until it is reviewed.".into();
        }));
        assert_eq!(r.level, PublishLevel::Warning);
        assert!(r.toast.contains("stays unlisted until it is reviewed"));
    }

    #[test]
    fn a_review_file_that_appears_after_open_is_a_change() {
        use std::time::{Duration as D, UNIX_EPOCH};
        let t0 = Some(UNIX_EPOCH + D::from_secs(100));
        let t1 = Some(UNIX_EPOCH + D::from_secs(200));
        // Studio opened with no review.toml: baseline, nothing adopted.
        assert_eq!(review_file_change(false, None, None), ReviewFileChange::Baseline);
        // The first publish writes it: this is the case that used to be skipped.
        assert_eq!(review_file_change(true, None, t0), ReviewFileChange::Changed);
        // Nothing moved.
        assert_eq!(review_file_change(true, t0, t0), ReviewFileChange::Unchanged);
        // Rewritten by a later decision.
        assert_eq!(review_file_change(true, t0, t1), ReviewFileChange::Changed);
        // A file already there at open is only a baseline, not news.
        assert_eq!(review_file_change(false, None, t0), ReviewFileChange::Baseline);
        // Deleted: not a change to adopt.
        assert_eq!(review_file_change(true, t0, None), ReviewFileChange::Unchanged);
    }

    #[test]
    fn the_progress_line_names_the_stage_and_the_percent() {
        assert_eq!(publish_progress_text("Uploading Universe...", 45.4), "Uploading Universe... 45%");
        assert_eq!(publish_progress_text("Packaging...", 0.0), "Packaging...");
        assert_eq!(publish_progress_text("  Under review...  ", 98.0), "Under review... 98%");
        assert_eq!(publish_progress_text("Uploading", 140.0), "Uploading 100%", "clamped");
        assert_eq!(publish_progress_text("Uploading", f32::NAN), "Uploading", "no NaN on screen");
    }

    #[test]
    fn the_dialog_shows_progress_while_running_and_an_error_when_it_fails() {
        use crate::ui::file_event_handler::PublishProgress;
        let running = PublishProgress { stage: "Uploading Universe...".into(), percent: 45.0, ..Default::default() };
        assert_eq!(publish_dialog_state(&running), (true, "Uploading Universe... 45%".to_string(), String::new()));
        let done = PublishProgress { complete: true, stage: "Published x: Listed in the Gallery".into(), percent: 100.0, ..Default::default() };
        assert_eq!(publish_dialog_state(&done), (false, String::new(), String::new()), "the result is the listing row, not this line");
        let failed = PublishProgress { complete: true, stage: "Failed".into(), error: Some("Create listing failed: timeout".into()), ..Default::default() };
        assert_eq!(publish_dialog_state(&failed), (false, String::new(), "Create listing failed: timeout".to_string()));
        let unchanged = PublishProgress { complete: true, no_changes: true, stage: "No changes since the last publish".into(), ..Default::default() };
        assert_eq!(publish_dialog_state(&unchanged), (false, String::new(), String::new()), "nothing changed is not an error");
    }

    #[test]
    fn a_cached_review_belongs_to_the_api_that_wrote_it() {
        let prod = "https://api.eustress.dev";
        let staging = "https://api-staging.eustress.dev";
        assert!(review_belongs_to_api(prod, prod, prod));
        assert!(review_belongs_to_api(staging, staging, prod));
        assert!(!review_belongs_to_api(prod, staging, prod), "production's review is not shown on staging");
        assert!(!review_belongs_to_api(staging, prod, prod), "and the other way round");
        // A file from before the API was recorded can only be production's.
        assert!(review_belongs_to_api("", prod, prod));
        assert!(!review_belongs_to_api("", staging, prod));
    }

    #[test]
    fn a_review_file_written_for_another_api_is_ignored_and_one_for_this_api_is_not() {
        let dir = std::env::temp_dir().join(format!("eustress-review-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".eustress")).unwrap();
        let file = |api: &str| format!("sim_id = \"s1\"\nstatus = \"listed\"\nheadline = \"h\"\nupdated_at = \"2026-01-05T12:00:00Z\"\napi = \"{api}\"\n");
        std::fs::write(ListingReview::path(&dir), file("https://other.invalid")).unwrap();
        assert_eq!(ListingReview::load(&dir), None, "a review from another API is not loaded");
        std::fs::write(ListingReview::path(&dir), file(&review_api_base())).unwrap();
        let r = ListingReview::load(&dir).expect("this API's own review loads");
        assert_eq!(r.sim_id, "s1");
        assert!(r.api.is_empty(), "the stamp stays in the file, not in memory");
        // Saving stamps the API in use.
        r.save(&dir).unwrap();
        let text = std::fs::read_to_string(ListingReview::path(&dir)).unwrap();
        assert!(text.contains(&format!("api = \"{}\"", review_api_base())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_button_is_offered_only_to_a_review_that_has_waited_long_enough() {
        use chrono::{Duration as D, Utc};
        let now = Utc::now();
        let at = |mins: i64| (now - D::minutes(mins)).to_rfc3339();
        let r = |status: &str, mins: i64| ListingReview { sim_id: "s".into(), status: status.into(), updated_at: at(mins), ..Default::default() };
        assert!(can_request_review_again(&r("in_review", 7), now));
        assert!(can_request_review_again(&r("in_review", 600), now));
        assert!(!can_request_review_again(&r("in_review", 3), now), "a young review may well be mid-run");
        for status in ["listed", "approved_private", "not_listed", "changes_requested", "appeal_in_review"] {
            assert!(!can_request_review_again(&r(status, 600), now), "{status}");
        }
        let mut no_id = r("in_review", 600);
        no_id.sim_id.clear();
        assert!(!can_request_review_again(&no_id, now));
        let mut bad = r("in_review", 600);
        bad.updated_at = "not a date".into();
        assert!(!can_request_review_again(&bad, now));
    }

    #[test]
    fn the_note_under_the_button_reads_the_same_for_every_case_a_person_might_hold() {
        let a = |code: u16, rerun: bool, error: &str| SubmitAnswer { code, rerun, status: "in_review".into(), error: error.into() };
        assert_eq!(rerequest_note(&a(200, true, "")), "Review requested. This usually takes under a minute.");
        assert_eq!(rerequest_note(&a(202, true, "")), "Review requested. This usually takes under a minute.");
        // Not run: the same words whether the AI is mid-run, a person has it, or it is held for a legal reason.
        assert_eq!(rerequest_note(&a(200, false, "")), "A reviewer is already working on it.");
        assert_eq!(rerequest_note(&a(429, false, "")), "Try again in a minute.");
        assert_eq!(rerequest_note(&a(429, false, "Too many review requests today. Try again tomorrow.")), "Too many review requests today. Try again tomorrow.");
        assert_eq!(rerequest_note(&a(500, false, "")), "The request could not be sent (HTTP 500).");
        assert_eq!(rerequest_note(&a(409, false, "Upload the .pak before submitting for review")), "Upload the .pak before submitting for review");
        for code in [200u16, 202] {
            let text = rerequest_note(&a(code, false, "")).to_lowercase();
            for banned in ["legal", "quarantin", "held", "csam", "hold"] {
                assert!(!text.contains(banned), "{text}");
            }
        }
    }

    #[test]
    fn a_submit_answer_is_read_from_the_body_and_an_old_worker_from_its_code() {
        let new = submit_answer(200, &serde_json::json!({ "status": "in_review", "rerun": false }));
        assert!(!new.rerun, "the new Worker says it did not run");
        assert!(submit_answer(200, &serde_json::json!({ "status": "listed", "rerun": true })).rerun);
        // An older Worker answered 202 with no rerun field, and only when it started a review.
        let old = submit_answer(202, &serde_json::json!({ "status": "classifying", "sim_id": "s" }));
        assert!(old.rerun);
        assert_eq!(rerequest_note(&old), "Review requested. This usually takes under a minute.");
        let refused = submit_answer(429, &serde_json::json!({ "error": "Too many review requests today. Try again tomorrow.", "retry_after_seconds": 86400 }));
        assert!(!refused.rerun);
        assert_eq!(rerequest_note(&refused), "Too many review requests today. Try again tomorrow.");
        assert_eq!(submit_answer(500, &serde_json::json!(null)), SubmitAnswer { code: 500, ..Default::default() });
    }

    // ── the listing thumbnail ──

    fn thumb_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-thumb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A PNG of `w` x `h`: busy (many colours, a large file) or flat (a small one).
    fn thumb_png(path: &Path, w: u32, h: u32, busy: bool) {
        image::RgbImage::from_fn(w, h, |x, y| {
            if busy {
                image::Rgb([(x * 7) as u8, (y * 3) as u8, (x ^ y) as u8])
            } else {
                image::Rgb([20, 20, 20])
            }
        })
        .save(path)
        .unwrap();
    }

    #[test]
    fn the_thumbnail_is_the_most_detailed_orbit_render_at_16_by_9() {
        let dir = thumb_dir("best");
        let flat = dir.join("capture-0.png");
        let busy = dir.join("capture-1.png");
        let empty = dir.join("capture-2.png");
        std::fs::write(&empty, b"").unwrap();
        thumb_png(&flat, 1280, 720, false);
        thumb_png(&busy, 1280, 720, true);
        let missing = dir.join("capture-3.png");
        let all = vec![flat.clone(), busy.clone(), empty, missing];
        assert_eq!(best_capture(&all), Some(busy.clone()), "the largest file; empty and missing are skipped");
        assert_eq!(best_capture(&[]), None);

        let out = dir.join(AUTO_THUMBNAIL_FILE);
        write_listing_thumbnail(&all, &out).unwrap();
        assert_eq!(image::image_dimensions(&out).unwrap(), (1280, 720), "16:9, at least 1280x720");
    }

    #[test]
    fn a_wider_or_taller_render_is_cropped_to_16_by_9_and_a_small_one_is_refused() {
        let dir = thumb_dir("crop");
        let tall = dir.join("tall.png");
        thumb_png(&tall, 1600, 1200, true);
        let out = dir.join("out.png");
        write_listing_thumbnail(&[tall], &out).unwrap();
        let (w, h) = image::image_dimensions(&out).unwrap();
        assert_eq!((w, h), (1600, 900));
        assert_eq!(w * 9, h * 16);

        let small = dir.join("small.png");
        thumb_png(&small, 640, 360, true);
        assert!(write_listing_thumbnail(&[small], &dir.join("never.png")).is_err(), "never scaled up");
        assert!(!dir.join("never.png").exists());
        assert!(write_listing_thumbnail(&[], &dir.join("none.png")).is_err());
    }

    #[test]
    fn the_old_window_screenshot_is_removed_and_an_authors_own_picture_is_kept() {
        let legacy_dir = thumb_dir("legacy");
        thumb_png(&legacy_dir.join("thumbnail.png"), 512, 288, true);
        assert!(is_legacy_window_thumbnail(&legacy_dir.join("thumbnail.png")));
        assert!(has_author_thumbnail(&legacy_dir));
        remove_legacy_window_thumbnail(&legacy_dir);
        assert!(!legacy_dir.join("thumbnail.png").exists(), "the old Studio-window screenshot is gone");
        assert!(!has_author_thumbnail(&legacy_dir), "so the generated one takes its place");

        let own_dir = thumb_dir("own");
        thumb_png(&own_dir.join("thumbnail.png"), 1920, 1080, true);
        remove_legacy_window_thumbnail(&own_dir);
        assert!(own_dir.join("thumbnail.png").exists(), "a picture the author chose stays");
        assert!(!is_legacy_window_thumbnail(&own_dir.join("missing.png")));
    }

    #[test]
    fn publishing_never_screenshots_the_studio_window() {
        // The thumbnail is an off-screen render of the scene. A capture of the
        // window would carry Studio's panels and the open Publish dialog.
        let needle = concat!("Screenshot::", "primary_window");
        for (name, source) in [
            ("ui/file_event_handler.rs", include_str!("ui/file_event_handler.rs")),
            ("moderation_dossier.rs", include_str!("moderation_dossier.rs")),
        ] {
            assert!(!source.contains(needle), "{name} captures the window");
        }
    }

    /// Only a publish that changed a listing on the server asks for the
    /// author's listings again (so an open Publish dialog updates in place).
    #[test]
    fn a_successful_publish_refreshes_the_listings() {
        assert!(publish_changed_listing(&finished(|p| p.sim_id = Some("sim-1".into()))));
        assert!(!publish_changed_listing(&finished(|p| {
            p.sim_id = Some("sim-1".into());
            p.error = Some("Committing the world failed".into());
        })));
        assert!(!publish_changed_listing(&finished(|p| {
            p.sim_id = Some("sim-1".into());
            p.no_changes = true;
        })));
        assert!(!publish_changed_listing(&finished(|_| {})), "never reached the server");
    }
}
