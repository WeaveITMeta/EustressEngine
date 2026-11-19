//! What a renderer failure means, and what the process should do about it.
//!
//! A lost GPU device (`wgpu` `DeviceLost`, "driver implementation is at
//! fault") is not an out-of-memory condition. Studio answers it with the
//! Player's shared recovery (`eustress_play_runtime::gpu_recovery`, added in
//! `main.rs`): the frame that notices the loss draws nothing more, the
//! renderer is rebuilt once the driver is back, and the simulation, scripts
//! and a hosted session keep running throughout. [`show_renderer_health`]
//! tells the user. A panic that still escapes is classified here for the exit
//! path in `main.rs`. `--server` never starts the renderer, so none of this
//! applies there.

use bevy::prelude::*;

/// How a panic payload should be treated. Matched on the text wgpu and Bevy
/// put in the panic, which is all `catch_unwind` can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicKind {
    /// A real allocation failure. Exit nonzero.
    OutOfMemory,
    /// The GPU device is gone. Not an allocation failure.
    GpuDeviceLost,
    /// The window's swap chain blinked (minimize, mode change). Exit cleanly.
    TransientSurface,
    /// Anything else. Re-raised so it is not swallowed.
    Other,
}

/// Classify a panic payload. Device-lost is checked before the older
/// "anything with Buffer in it is out of memory" reading, which mislabeled
/// the clustering panic.
pub fn classify_panic(msg: &str) -> PanicKind {
    if is_gpu_device_lost(msg) {
        return PanicKind::GpuDeviceLost;
    }
    if msg.contains("Out of memory")
        || msg.contains("OutOfMemory")
        || msg.contains("out of device memory")
        || msg.contains("OutOfDeviceMemory")
    {
        return PanicKind::OutOfMemory;
    }
    if msg.contains("swap chain")
        || msg.contains("Acquiring a texture")
        || msg.contains("Surface timed out")
        || msg.contains("Outdated")
    {
        return PanicKind::TransientSurface;
    }
    PanicKind::Other
}

/// `true` when `msg` is wgpu reporting that the device itself is gone, or
/// the validation panic `bevy_pbr` raises in GPU light clustering on the
/// frame after that.
pub fn is_gpu_device_lost(msg: &str) -> bool {
    msg.contains("DeviceLost")
        || msg.contains("Device is lost")
        || msg.contains("device is lost")
        || msg.contains("driver implementation is at fault")
        || (msg.contains("get_mapped_range") && msg.contains("Validation"))
}

/// Studio's plain notice while the renderer recovers from a graphics driver
/// reset, or after it has stopped (`eustress_play_runtime::gpu_recovery`).
///
/// The window title carries it, because Windows draws the title bar while
/// Studio's own drawing (the viewport and every panel) is paused. A
/// notification follows, which shows once the panels draw again. The title
/// goes back to the Space's own once drawing returns, unless the Space
/// changed meanwhile and `update_window_title` already set a new one.
pub fn show_renderer_health(
    health: Option<Res<eustress_play_runtime::gpu_recovery::RendererHealth>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut notifications: Option<ResMut<crate::notifications::NotificationManager>>,
    mut plain_title: Local<Option<String>>,
) {
    use eustress_play_runtime::gpu_recovery::{health_notice, RendererHealth};
    let Some(health) = health else { return };
    if !health.is_changed() {
        return;
    }
    match health_notice(*health) {
        Some(notice) => {
            for mut window in &mut windows {
                let plain = plain_title.get_or_insert_with(|| window.title.clone()).clone();
                window.title = format!("{notice} | {plain}");
            }
            if let Some(notifications) = notifications.as_deref_mut() {
                if *health == RendererHealth::Stopped {
                    notifications.error(notice);
                } else {
                    notifications.warning(notice);
                }
            }
        }
        None => {
            let Some(plain) = plain_title.take() else { return };
            for mut window in &mut windows {
                if window.title.ends_with(&format!(" | {plain}")) {
                    window.title = plain.clone();
                }
            }
            if let Some(notifications) = notifications.as_deref_mut() {
                notifications.success("The graphics driver reset and drawing is back. The simulation kept running.");
            }
        }
    }
}

/// Turn GPU light clustering off once Play starts.
///
/// This was the guard against the mapped-range panic of a lost device. The
/// 11-18 loss panicked in `prepare_clusters_for_cpu_clustering` too, so it
/// does not protect against that; the render-set gate in
/// `eustress_play_runtime::gpu_recovery` does. Edit mode is left on the GPU
/// path.
pub fn prefer_cpu_clustering_in_play(
    state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut settings: Option<ResMut<bevy::light::cluster::GlobalClusterSettings>>,
    mut applied: Local<bool>,
) {
    if *applied {
        return;
    }
    let playing = state
        .as_ref()
        .is_some_and(|s| *s.get() == crate::play_mode::PlayModeState::Playing);
    if !playing {
        return;
    }
    let Some(mut settings) = settings else { return };
    if settings.gpu_clustering.take().is_some() {
        info!(
            "GPU light clustering is off for this play session, so a lost device cannot panic inside it"
        );
    }
    *applied = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_device_is_not_reported_as_out_of_memory() {
        let lost = "wgpu 29.0.4 DeviceLost: driver implementation is at fault";
        assert_eq!(classify_panic(lost), PanicKind::GpuDeviceLost);
        let follow = "Validation Error\n\nCaused by:\n    In Buffer::get_mapped_range";
        assert_eq!(classify_panic(follow), PanicKind::GpuDeviceLost);
        assert_eq!(
            classify_panic("wgpu error: OutOfMemory"),
            PanicKind::OutOfMemory
        );
        assert_eq!(
            classify_panic("swap chain outdated"),
            PanicKind::TransientSurface
        );
        assert_eq!(classify_panic("index out of bounds"), PanicKind::Other);
    }
}
