//! What a renderer failure means, and what the process should do about it.
//!
//! A lost GPU device (`wgpu` `DeviceLost`, "driver implementation is at
//! fault") is not an out-of-memory condition. Bevy's default handler quits
//! the whole app, and the next `Buffer::get_mapped_range` inside
//! `bevy_pbr`'s GPU light clustering then panics. A hosted session does not
//! need that renderer: drawing stops, and the simulation keeps running.
//! `--server` never starts the renderer, so this path does not apply there.

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

/// Bevy calls this for every `RenderError`. A lost device stops drawing and
/// does not close the app, so a play session that is hosting keeps its
/// players. Out of memory, and any other renderer fault, still ends the process.
pub fn on_render_error(
    error: &bevy::render::error_handler::RenderError,
    main: &mut World,
    _render: &mut World,
) -> bevy::render::error_handler::RenderErrorPolicy {
    use bevy::render::error_handler::{ErrorType, RenderErrorPolicy};
    if error.ty == ErrorType::DeviceLost || is_gpu_device_lost(&error.description) {
        error!(
            target: "eustress_engine::crash",
            reason = %error.description,
            "GPU device lost — drawing stops; the simulation keeps running. Host with --server to avoid the GPU entirely."
        );
        return RenderErrorPolicy::StopRendering;
    }
    if error.ty == ErrorType::OutOfMemory {
        error!(
            target: "eustress_engine::crash",
            "GPU out of memory — quitting"
        );
        main.write_message(AppExit::error());
        return RenderErrorPolicy::StopRendering;
    }
    error!(
        target: "eustress_engine::crash",
        kind = ?error.ty,
        reason = %error.description,
        "render error — quitting"
    );
    main.write_message(AppExit::error());
    RenderErrorPolicy::StopRendering
}

/// Turn GPU light clustering off once Play starts.
///
/// The mapped-range panic is inside that pass. CPU clustering is the
/// fallback Bevy already uses when the device cannot compute, and it does
/// not map that buffer. Edit mode is left on the GPU path.
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
