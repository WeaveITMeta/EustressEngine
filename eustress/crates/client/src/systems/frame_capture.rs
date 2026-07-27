//! # Frame-burst capture
//!
//! Writes a run of PNG frames to disk so an agent can *look* at the running
//! client instead of inferring behaviour from logs.
//!
//! A burst of stills beats a video file here: an agent can read PNGs directly,
//! and stepping through consecutive frames is what makes a gait legible —
//! whether the legs alternate, whether a foot slides, whether the mesh drifts
//! off the capsule and snaps back.
//!
//! ## Triggers
//!
//! * `EUSTRESS_CAPTURE=<count>[@<every_n_frames>]` — arm at startup.
//!   `EUSTRESS_CAPTURE=12@6` writes 12 frames, one every 6 frames (~0.2 s
//!   apart at 60 Hz, so 12 frames spans ~2 s — about two walk cycles).
//! * **F9** — arm the same burst on demand, for capturing a specific moment
//!   such as a jump.
//!
//! Frames land in `EUSTRESS_CAPTURE_DIR` if set, otherwise a `capture/`
//! directory beside the executable.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::path::PathBuf;

/// Default burst shape when only a count is given.
const DEFAULT_EVERY: u32 = 6;
/// Frames to wait after arming, so the avatar has spawned and settled.
const ARM_DELAY_FRAMES: u32 = 90;

#[derive(Resource, Debug)]
pub struct FrameCapture {
    remaining: u32,
    every: u32,
    tick: u32,
    delay: u32,
    index: u32,
    dir: PathBuf,
    announced: bool,
}

impl FrameCapture {
    fn armed(count: u32, every: u32, delay: u32) -> Self {
        Self {
            remaining: count,
            every: every.max(1),
            tick: 0,
            delay,
            index: 0,
            dir: capture_dir(),
            announced: false,
        }
    }
}

fn capture_dir() -> PathBuf {
    if let Ok(d) = std::env::var("EUSTRESS_CAPTURE_DIR") {
        return PathBuf::from(d);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("capture")))
        .unwrap_or_else(|| PathBuf::from("capture"))
}

pub struct FrameCapturePlugin;

impl Plugin for FrameCapturePlugin {
    fn build(&self, app: &mut App) {
        // `EUSTRESS_CAPTURE=12` or `EUSTRESS_CAPTURE=12@6`
        let armed = std::env::var("EUSTRESS_CAPTURE").ok().map(|spec| {
            let (count, every) = match spec.split_once('@') {
                Some((c, e)) => (c.parse().unwrap_or(12), e.parse().unwrap_or(DEFAULT_EVERY)),
                None => (spec.parse().unwrap_or(12), DEFAULT_EVERY),
            };
            FrameCapture::armed(count, every, ARM_DELAY_FRAMES)
        });

        if let Some(c) = armed {
            app.insert_resource(c);
        }
        app.add_systems(Update, (arm_on_hotkey, drive_capture));
    }
}

fn arm_on_hotkey(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F9) {
        // No delay for a manual capture — the operator picked the moment.
        commands.insert_resource(FrameCapture::armed(12, DEFAULT_EVERY, 0));
        info!("📸 F9 — capturing 12 frames");
    }
}

fn drive_capture(mut commands: Commands, capture: Option<ResMut<FrameCapture>>) {
    let Some(mut cap) = capture else { return };

    if cap.delay > 0 {
        cap.delay -= 1;
        return;
    }
    if cap.remaining == 0 {
        return;
    }

    if !cap.announced {
        if let Err(e) = std::fs::create_dir_all(&cap.dir) {
            error!("frame capture: cannot create {:?}: {e}", cap.dir);
            cap.remaining = 0;
            return;
        }
        info!(
            "📸 capturing {} frames (every {}) to {:?}",
            cap.remaining, cap.every, cap.dir
        );
        cap.announced = true;
    }

    cap.tick += 1;
    if cap.tick % cap.every != 0 {
        return;
    }

    let path = cap.dir.join(format!("frame_{:03}.png", cap.index));
    cap.index += 1;
    cap.remaining -= 1;

    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));

    if cap.remaining == 0 {
        info!("📸 capture complete — {} frames in {:?}", cap.index, cap.dir);
    }
}
