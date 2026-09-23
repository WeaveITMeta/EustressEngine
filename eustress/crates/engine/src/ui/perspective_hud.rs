//! Rust to Slint sync for the Perspective control.
//!
//! Reflects the editor camera's `EustressCamera` state (2D or 3D, the
//! projection, the axis view, the orthographic view height and the field of
//! view) into the `view-*` properties of `main.slint`, which the tab-bar
//! `ViewSelector` and the View menu display, and the Space's saved viewpoint
//! names into the View menu's VIEWPOINTS list.
//!
//! The other direction needs no code here: every control calls
//! `set-view-mode(command)`, and `drain_slint_actions` hands the string to
//! `camera_controller::ViewCommand::parse`.
//!
//! Properties are written only when their text changes, and the view-height
//! readout at most ten times a second while zooming, so a steady view never
//! repaints the chrome.

use bevy::prelude::*;

use crate::camera_controller::{CameraView, EustressCamera};
use eustress_common::units::{self, DisplayUnit, Unit};

/// Mounts the sync. Add after the Slint UI plugin, like the other sync
/// plugins: `SlintUiState` must exist for it to do anything.
pub struct PerspectiveHudPlugin;

impl Plugin for PerspectiveHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_perspective_to_slint, sync_viewpoints_to_slint)
                .after(super::slint_ui::SlintSystems::Drain),
        );
    }
}

/// The open Space's saved viewpoint names into the View menu's list, once
/// per change.
fn sync_viewpoints_to_slint(
    slint_context: Option<NonSend<super::SlintUiState>>,
    names: Option<Res<crate::saved_viewpoints::ViewpointNames>>,
    mut pushed: Local<Option<u64>>,
) {
    let (Some(slint_context), Some(names)) = (slint_context, names) else { return };
    if *pushed == Some(names.revision) {
        return;
    }
    *pushed = Some(names.revision);
    let model: Vec<slint::SharedString> = names.names.iter().map(|n| n.as_str().into()).collect();
    slint_context
        .window
        .set_viewpoint_names(slint::ModelRc::new(slint::VecModel::from(model)));
}

fn sync_perspective_to_slint(
    slint_context: Option<NonSend<super::SlintUiState>>,
    cameras: Query<&EustressCamera, With<Camera3d>>,
    display_unit: Option<Res<DisplayUnit>>,
    mut last_scale_push: Local<Option<std::time::Instant>>,
) {
    let Some(slint_context) = slint_context else { return };
    let Some(cam) = cameras.iter().next() else { return };
    let ui = &slint_context.window;

    let dimension = if cam.is_2d() { "2d" } else { "3d" };
    if ui.get_view_dimension().as_str() != dimension {
        ui.set_view_dimension(dimension.into());
    }

    let projection = if cam.wants_ortho() { "orthographic" } else { "perspective" };
    if ui.get_view_projection().as_str() != projection {
        ui.set_view_projection(projection.into());
    }

    // The axis the view looks along while settled on one; "custom" while
    // turning or free.
    let axis = match (cam.animating, cam.is_2d()) {
        (true, _) => CameraView::Custom,
        (false, true) => cam.plane_view,
        (false, false) => cam.current_view,
    };
    let axis = axis.name().to_ascii_lowercase();
    if ui.get_view_axis().as_str() != axis {
        ui.set_view_axis(axis.into());
    }

    let label = cam.view_label();
    if ui.get_view_label().as_str() != label {
        ui.set_view_label(label.into());
    }

    let fov = cam.fov.to_degrees().round();
    if (ui.get_view_fov() - fov).abs() > 0.25 {
        ui.set_view_fov(fov);
    }

    let unit = display_unit.map(|u| u.0).unwrap_or(units::ENGINE_NATIVE_UNIT);
    let scale = if cam.wants_ortho() {
        format_view_height(cam.ortho_height(), unit)
    } else {
        String::new()
    };
    if ui.get_view_scale().as_str() != scale {
        // Clearing or setting it with the mode change is immediate; a zoom
        // that keeps changing the number is capped at 10 Hz.
        let mode_flip = scale.is_empty() || ui.get_view_scale().is_empty();
        let due = last_scale_push
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(100))
            .unwrap_or(true);
        if mode_flip || due {
            ui.set_view_scale(scale.into());
            *last_scale_push = Some(std::time::Instant::now());
        }
    }
}

/// Visible view height in the user's display unit, three significant
/// figures, switching to kilometres past 10 km when the unit is metres.
fn format_view_height(meters: f32, unit: Unit) -> String {
    if !meters.is_finite() {
        return String::new();
    }
    let (value, symbol) = if unit == Unit::Meter && meters >= 10_000.0 {
        (meters as f64 / 1000.0, "km")
    } else {
        (units::convert(meters as f64, units::ENGINE_NATIVE_UNIT, unit), unit.symbol())
    };
    let digits = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.digits$} {symbol}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_height_reads_in_three_figures() {
        assert_eq!(format_view_height(24.0, Unit::Meter), "24.0 m");
        assert_eq!(format_view_height(1.234, Unit::Meter), "1.23 m");
        assert_eq!(format_view_height(350.0, Unit::Meter), "350 m");
        assert_eq!(format_view_height(12_500.0, Unit::Meter), "12.5 km");
    }
}
