//! Settings > General (grid, snap, Auto-save), Graphics and Audio.
//!
//! The Settings dialog reaches these through the `Preferences` Slint global
//! (ui/slint/settings.slint). This plugin registers that global's callbacks
//! itself, so nothing here goes through `drain_slint_actions`:
//!
//! - `set(key, value)` changes one [`EditorSettings`] field, which
//!   `auto_save_settings` then writes to `settings.json`;
//! - `preview(key, value)` moves a volume live while its slider is dragged,
//!   without saving (the release sends `set`);
//! - `reset()` puts Graphics, Audio and Auto-save back to their defaults.
//!
//! Every value lives in `EditorSettings`; [`sync_to_slint`] pushes it to the
//! global whenever the settings change, from here or from anywhere else (the
//! Home tab's Snap button, Reset to defaults). The appliers below turn the
//! values into engine state, so they take effect at once and again on every
//! launch:
//!
//! | Setting | Applied to |
//! |---|---|
//! | Shadow quality | shadow-map sizes and the sun's shadow reach |
//! | Shadows | the render world's copy of every light (the lights keep their own flags) |
//! | Anti-aliasing | `Smaa` on every Studio camera |
//! | V-Sync | the primary window's present mode |
//! | Frame rate while editing | `IdleSettings::active_update_ms` |
//! | Volumes | `GlobalVolume` and every playing sink |
//!
//! A launch variable that measures one of these (`EUSTRESS_SMAA`,
//! `EUSTRESS_MAX_FPS`, `EUSTRESS_SHADOW_DISTANCE`) wins over the Settings
//! choice while it is set, so an A/B run measures what it asked for.

use std::sync::{Arc, Mutex};

use bevy::anti_alias::smaa::Smaa;
use bevy::audio::{AudioSink, AudioSinkPlayback, GlobalVolume, PlaybackMode, PlaybackSettings, SpatialAudioSink, Volume};
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap, PointLightShadowMap};
use bevy::pbr::{ExtractedDirectionalLight, ExtractedPointLight};
use bevy::log::warn;
use bevy::prelude::*;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::window::{PresentMode, PrimaryWindow};
use eustress_common::classes::Sound;
use eustress_common::services::sound::SoundMix;
use eustress_common::services::lighting::Sun;
use slint::{ComponentHandle, SharedString};

use crate::default_scene::StudioCamera;
use crate::editor_settings::EditorSettings;
use crate::ui::slint_ui::Preferences;
use crate::window_focus::IdleSettings;

/// Shadow quality presets, Low to Ultra: the sun's cascade size, the point
/// and spot cube-face size, how far from the camera the sun's shadows reach
/// (metres), and where its first cascade ends. High is the engine's own
/// defaults, so a new install draws exactly what it drew before.
static QUALITY: [ShadowPreset; 4] = [
    ShadowPreset { sun_map: 1024, point_map: 512, reach: 150.0, first_cascade: 30.0 },
    ShadowPreset { sun_map: 2048, point_map: 1024, reach: 400.0, first_cascade: 60.0 },
    ShadowPreset { sun_map: 2048, point_map: 1024, reach: 1000.0, first_cascade: 90.0 },
    ShadowPreset { sun_map: 4096, point_map: 2048, reach: 2000.0, first_cascade: 90.0 },
];

struct ShadowPreset {
    sun_map: usize,
    point_map: usize,
    reach: f32,
    first_cascade: f32,
}

fn preset(quality: u8) -> &'static ShadowPreset {
    &QUALITY[(quality as usize).min(QUALITY.len() - 1)]
}

/// Whether a launch variable is set, so it wins over the Settings choice.
fn launch_override(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

// ============================================================================
// Plugin
// ============================================================================

/// Added by `EditorSettingsPlugin`, which owns the settings it applies.
pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreferencesInbox>()
            .init_resource::<VolumePreview>()
            .init_resource::<ShadowsOff>()
            .add_systems(
                Update,
                (register_callbacks, apply_messages, sync_to_slint)
                    .chain()
                    .after(crate::ui::slint_ui::SlintSystems::Drain),
            )
            .add_systems(
                Update,
                (apply_display, apply_shadow_quality, apply_shadows_switch, apply_anti_aliasing, apply_volumes)
                    .after(apply_messages),
            );
    }

    /// The Shadows switch acts in the render world, which exists only once
    /// every plugin is built.
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ShadowsOff>()
            .add_systems(ExtractSchedule, extract_shadows_off)
            .add_systems(
                Render,
                suppress_extracted_shadows
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::CreateViews),
            );
    }
}

/// What the `Preferences` callbacks queue for [`apply_messages`].
enum PrefMsg {
    Set(String, String),
    Preview(String, String),
    Reset,
}

#[derive(Resource, Default)]
struct PreferencesInbox(Arc<Mutex<Vec<PrefMsg>>>);

/// Volumes while their slider is held, 0 to 1: heard at once, saved on
/// release.
#[derive(Resource, Default)]
struct VolumePreview {
    master: Option<f32>,
    effects: Option<f32>,
    music: Option<f32>,
}

/// The Shadows switch is off. Copied into the render world every frame.
#[derive(Resource, Default, Clone, Copy)]
struct ShadowsOff(bool);

// ============================================================================
// Settings dialog
// ============================================================================

/// Hook the global's callbacks once the Slint window exists.
fn register_callbacks(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    inbox: Res<PreferencesInbox>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(slint) = slint else { return };
    let g = slint.window.global::<Preferences>();
    let q = inbox.0.clone();
    g.on_set(move |key: SharedString, value: SharedString| {
        if let Ok(mut v) = q.lock() {
            v.push(PrefMsg::Set(key.to_string(), value.to_string()));
        }
    });
    let q = inbox.0.clone();
    g.on_preview(move |key: SharedString, value: SharedString| {
        if let Ok(mut v) = q.lock() {
            v.push(PrefMsg::Preview(key.to_string(), value.to_string()));
        }
    });
    let q = inbox.0.clone();
    g.on_reset(move || {
        if let Ok(mut v) = q.lock() {
            v.push(PrefMsg::Reset);
        }
    });
    *done = true;
}

fn flag(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

/// A 0 to 100 slider value as a 0 to 1 level.
fn level(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0))
}

/// Apply what the dialog asked for. A field is written only when its value
/// really changes, so the settings file is saved once per real change.
fn apply_messages(
    inbox: Res<PreferencesInbox>,
    mut settings: ResMut<EditorSettings>,
    mut preview: ResMut<VolumePreview>,
) {
    let msgs: Vec<PrefMsg> = match inbox.0.lock() {
        Ok(mut v) if !v.is_empty() => std::mem::take(&mut *v),
        _ => return,
    };
    for msg in msgs {
        match msg {
            PrefMsg::Preview(key, value) => {
                let Some(v) = level(&value) else { continue };
                match key.as_str() {
                    "master_volume" => preview.master = Some(v),
                    "effects_volume" => preview.effects = Some(v),
                    "music_volume" => preview.music = Some(v),
                    _ => {}
                }
            }
            PrefMsg::Set(key, value) => apply_set(&mut settings, &mut preview, &key, &value),
            PrefMsg::Reset => {
                let d = EditorSettings::default();
                *preview = VolumePreview::default();
                if settings.render_quality != d.render_quality {
                    settings.render_quality = d.render_quality;
                }
                if settings.shadows_enabled != d.shadows_enabled {
                    settings.shadows_enabled = d.shadows_enabled;
                }
                if settings.anti_aliasing != d.anti_aliasing {
                    settings.anti_aliasing = d.anti_aliasing;
                }
                if settings.vsync != d.vsync {
                    settings.vsync = d.vsync;
                }
                if settings.max_fps != d.max_fps {
                    settings.max_fps = d.max_fps;
                }
                if settings.master_volume != d.master_volume {
                    settings.master_volume = d.master_volume;
                }
                if settings.effects_volume != d.effects_volume {
                    settings.effects_volume = d.effects_volume;
                }
                if settings.music_volume != d.music_volume {
                    settings.music_volume = d.music_volume;
                }
                if settings.auto_save_enabled != d.auto_save_enabled {
                    settings.auto_save_enabled = d.auto_save_enabled;
                }
                if settings.auto_save_interval != d.auto_save_interval {
                    settings.auto_save_interval = d.auto_save_interval;
                }
            }
        }
    }
}

fn apply_set(settings: &mut ResMut<EditorSettings>, preview: &mut VolumePreview, key: &str, value: &str) {
    match key {
        "show_grid" => {
            let v = flag(value);
            if settings.show_grid != v {
                settings.show_grid = v;
            }
        }
        "snap_enabled" => {
            let v = flag(value);
            if settings.snap_enabled != v {
                settings.snap_enabled = v;
            }
        }
        "auto_save_enabled" => {
            let v = flag(value);
            if settings.auto_save_enabled != v {
                settings.auto_save_enabled = v;
            }
        }
        "auto_save_interval" => {
            let Ok(v) = value.trim().parse::<f32>() else { return };
            let v = v.clamp(30.0, 3600.0);
            if settings.auto_save_interval != v {
                settings.auto_save_interval = v;
            }
        }
        "render_quality" => {
            let Ok(v) = value.trim().parse::<u8>() else { return };
            let v = v.min(QUALITY.len() as u8 - 1);
            if settings.render_quality != v {
                settings.render_quality = v;
            }
        }
        "shadows_enabled" => {
            let v = flag(value);
            if settings.shadows_enabled != v {
                settings.shadows_enabled = v;
            }
        }
        "anti_aliasing" => {
            let v = flag(value);
            if settings.anti_aliasing != v {
                settings.anti_aliasing = v;
            }
        }
        "vsync" => {
            let v = flag(value);
            if settings.vsync != v {
                settings.vsync = v;
            }
        }
        "max_fps" => {
            let Ok(v) = value.trim().parse::<u32>() else { return };
            if settings.max_fps != v {
                settings.max_fps = v;
            }
        }
        "master_volume" => {
            let Some(v) = level(value) else { return };
            preview.master = None;
            if settings.master_volume != v {
                settings.master_volume = v;
            }
        }
        "effects_volume" => {
            let Some(v) = level(value) else { return };
            preview.effects = None;
            if settings.effects_volume != v {
                settings.effects_volume = v;
            }
        }
        "music_volume" => {
            let Some(v) = level(value) else { return };
            preview.music = None;
            if settings.music_volume != v {
                settings.music_volume = v;
            }
        }
        other => warn!("Settings: unknown preference {other:?}"),
    }
}

/// Push the settings to the dialog when they change. A volume whose slider
/// is held is left alone, so the knob never jumps under the pointer.
fn sync_to_slint(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    settings: Res<EditorSettings>,
    preview: Res<VolumePreview>,
    mut pushed: Local<bool>,
) {
    let Some(slint) = slint else { return };
    if *pushed && !settings.is_changed() {
        return;
    }
    *pushed = true;
    let g = slint.window.global::<Preferences>();
    g.set_show_grid(settings.show_grid);
    g.set_snap(settings.snap_enabled);
    g.set_auto_save(settings.auto_save_enabled);
    g.set_auto_save_minutes((settings.auto_save_interval / 60.0).round() as i32);
    g.set_quality(settings.render_quality.min(QUALITY.len() as u8 - 1) as i32);
    g.set_shadows(settings.shadows_enabled);
    g.set_anti_aliasing(settings.anti_aliasing);
    g.set_vsync(settings.vsync);
    g.set_max_fps(settings.max_fps.min(i32::MAX as u32) as i32);
    if preview.master.is_none() {
        g.set_master_volume(settings.master_volume * 100.0);
    }
    if preview.effects.is_none() {
        g.set_effects_volume(settings.effects_volume * 100.0);
    }
    if preview.music.is_none() {
        g.set_music_volume(settings.music_volume * 100.0);
    }
}

// ============================================================================
// Graphics
// ============================================================================

/// V-Sync and the editing frame rate.
fn apply_display(
    settings: Res<EditorSettings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    idle: Option<ResMut<IdleSettings>>,
    mut applied: Local<bool>,
) {
    if *applied && !settings.is_changed() {
        return;
    }
    *applied = true;

    let mode = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    for mut window in &mut windows {
        if window.present_mode != mode {
            window.present_mode = mode;
        }
    }

    if !launch_override("EUSTRESS_MAX_FPS") {
        if let Some(mut idle) = idle {
            let ms = if settings.max_fps == 0 {
                0
            } else {
                (1000.0 / settings.max_fps as f32).round().max(1.0) as u64
            };
            if idle.active_update_ms != ms {
                idle.active_update_ms = ms;
            }
        }
    }
}

/// Shadow quality: the shadow-map sizes, and the sun's reach. The sun is
/// set again whenever it (re)spawns with a Space.
fn apply_shadow_quality(
    settings: Res<EditorSettings>,
    sun_map: Option<ResMut<DirectionalLightShadowMap>>,
    point_map: Option<ResMut<PointLightShadowMap>>,
    mut suns: Query<&mut CascadeShadowConfig, With<Sun>>,
    mut applied: Local<bool>,
) {
    let changed = !*applied || settings.is_changed();
    *applied = true;
    let p = preset(settings.render_quality);

    if changed {
        if let Some(mut sun_map) = sun_map {
            if sun_map.size != p.sun_map {
                sun_map.size = p.sun_map;
            }
        }
        if let Some(mut point_map) = point_map {
            if point_map.size != p.point_map {
                point_map.size = p.point_map;
            }
        }
    }

    if launch_override("EUSTRESS_SHADOW_DISTANCE") {
        return;
    }
    for mut config in &mut suns {
        if !changed && !config.is_added() {
            continue;
        }
        if config.bounds.last().copied() == Some(p.reach) {
            continue;
        }
        *config = CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            maximum_distance: p.reach,
            first_cascade_far_bound: p.first_cascade,
            overlap_proportion: 0.25,
            ..default()
        }
        .build();
    }
}

/// The Shadows switch. Lights keep their own shadow flags, which the light
/// classes, the light culler and the budget all manage; the switch clears
/// the render world's copy instead ([`suppress_extracted_shadows`]), so
/// turning it back on restores exactly what each light had.
fn apply_shadows_switch(
    settings: Res<EditorSettings>,
    mut off: ResMut<ShadowsOff>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut dirs: Query<&mut DirectionalLight>,
) {
    let want_off = !settings.shadows_enabled;
    if off.0 == want_off {
        return;
    }
    off.0 = want_off;
    if !want_off {
        // Lights reach the render world only when they change: touch each,
        // so its own flag is copied over the cleared one.
        for mut light in &mut points {
            light.set_changed();
        }
        for mut light in &mut spots {
            light.set_changed();
        }
        for mut light in &mut dirs {
            light.set_changed();
        }
    }
}

fn extract_shadows_off(mut commands: Commands, off: Extract<Option<Res<ShadowsOff>>>) {
    let off = matches!(&*off, Some(o) if o.0);
    commands.insert_resource(ShadowsOff(off));
}

/// With Shadows off, no light in the render world casts: `prepare_lights`
/// then builds no shadow views or maps at all.
fn suppress_extracted_shadows(
    off: Option<Res<ShadowsOff>>,
    mut points: Query<&mut ExtractedPointLight>,
    mut dirs: Query<&mut ExtractedDirectionalLight>,
) {
    if !off.map_or(false, |o| o.0) {
        return;
    }
    for mut light in &mut points {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut dirs {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
}

/// SMAA on every Studio camera, or on none. It is a post pass, outside the
/// shared mesh-view layout (see `photoreal`), so it can change at runtime.
/// A camera that spawns later gets `photoreal`'s launch default first, and
/// this corrects it on the next frame.
fn apply_anti_aliasing(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    cameras: Query<(Entity, Has<Smaa>), With<StudioCamera>>,
) {
    if launch_override("EUSTRESS_SMAA") {
        return;
    }
    for (entity, has) in &cameras {
        if settings.anti_aliasing && !has {
            commands.entity(entity).insert(Smaa::default());
        } else if !settings.anti_aliasing && has {
            commands.entity(entity).remove::<Smaa>();
        }
    }
}

// ============================================================================
// Audio
// ============================================================================

/// Which volume slider a sink with no Sound answers to, beside Master: a
/// looping one is music, the rest are effects.
fn music_like(playback: Option<&PlaybackSettings>) -> bool {
    matches!(playback.map(|p| p.mode), Some(PlaybackMode::Loop))
}

/// The sliders reach sounds two ways. A Sound instance is the shared Sound
/// player's (`eustress_play_runtime::sound_player`), which multiplies its
/// Volume and roll-off by `SoundMix` (Master and its group's slider) every
/// frame. Every other sink plays at its own volume times Master times the
/// Effects or Music slider, set when it starts, when its settings change,
/// and all at once when a slider moves. `GlobalVolume` carries Master too, so
/// a sound is at the Master level from its first frame.
#[allow(clippy::type_complexity)]
fn apply_volumes(
    settings: Res<EditorSettings>,
    preview: Res<VolumePreview>,
    global: Option<ResMut<GlobalVolume>>,
    mix: Option<ResMut<SoundMix>>,
    mut sinks: Query<(&mut AudioSink, Option<Ref<PlaybackSettings>>), (Without<SpatialAudioSink>, Without<Sound>)>,
    mut spatial: Query<(&mut SpatialAudioSink, Option<Ref<PlaybackSettings>>), (Without<AudioSink>, Without<Sound>)>,
    mut last: Local<Option<(f32, f32, f32)>>,
) {
    let master = preview.master.unwrap_or(settings.master_volume);
    let effects = preview.effects.unwrap_or(settings.effects_volume);
    let music = preview.music.unwrap_or(settings.music_volume);
    let levels = (master, effects, music);
    let all = *last != Some(levels);
    *last = Some(levels);

    if all {
        if let Some(mut global) = global {
            global.volume = Volume::Linear(master);
        }
    }
    // Every frame, not only on a slider move: the player's resource may
    // appear after the first levels were read.
    if let Some(mut mix) = mix {
        mix.set_if_neq(SoundMix { master, effects, music });
    }
    let gain = |playback: Option<&PlaybackSettings>| -> Volume {
        let base = playback.map_or(Volume::Linear(1.0), |p| p.volume);
        let group = if music_like(playback) { music } else { effects };
        base * Volume::Linear(master * group)
    };
    for (mut sink, playback) in &mut sinks {
        let fresh = sink.is_added() || playback.as_ref().is_some_and(|p| p.is_changed());
        if all || fresh {
            let v = gain(playback.as_deref());
            sink.set_volume(v);
        }
    }
    for (mut sink, playback) in &mut spatial {
        let fresh = sink.is_added() || playback.as_ref().is_some_and(|p| p.is_changed());
        if all || fresh {
            let v = gain(playback.as_deref());
            sink.set_volume(v);
        }
    }
}
