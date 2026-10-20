//! Terrain edits a Play session's scripts queued (`workspace.Terrain:FillBall`,
//! ...), applied after the draw step through the editor's terrain commands.
//! Only the authority edits terrain; the edits are not replicated yet.

use bevy::prelude::*;

use eustress_common::datamodel::{DataModel, OutputLevel, SharedDataModel};
use eustress_common::terrain::api::TerrainCommand;

use super::PlayDataModel;
use crate::terrain_commands::{apply_terrain_commands, TerrainCommandOrigin};

/// The terrain refusal the Output last showed, and the session it was in.
/// The session's tree is held weakly: that keeps its allocation, so a later
/// session's tree never lands at the same address and reads as the same one.
#[derive(Default)]
pub struct ReportedTerrainRefusal {
    session: std::sync::Weak<parking_lot::Mutex<DataModel>>,
    line: String,
}

/// Apply this frame's script terrain edits, if any.
pub fn apply_script_terrain_edits(world: &mut World, mut reported: Local<ReportedTerrainRefusal>) {
    let Some(dm) = world.get_resource::<PlayDataModel>().map(|r| r.dm.clone()) else { return };
    let commands = std::mem::take(&mut dm.lock().terrain_commands);
    if !commands.is_empty() {
        apply_edits(world, &dm, commands, &mut reported);
    }
}

/// Apply the terrain edits scripts queued this frame, in call order. The
/// calls returned long ago (Roblox's terrain writes do not report back
/// either), so a refused edit is logged, and the Output shows the frame's
/// first refusal, once while the same one keeps coming.
fn apply_edits(
    world: &mut World,
    dm: &SharedDataModel,
    commands: Vec<TerrainCommand>,
    reported: &mut ReportedTerrainRefusal,
) {
    let kinds: Vec<&'static str> = commands.iter().map(terrain_command_kind).collect();
    let results = apply_terrain_commands(world, commands, TerrainCommandOrigin::Script);
    let mut first: Option<String> = None;
    let mut refused = 0usize;
    for (kind, result) in kinds.iter().zip(results) {
        if let Err(e) = result {
            warn!("Terrain:{kind} from a script was not applied: {e}");
            refused += 1;
            if first.is_none() {
                first = Some(format!("Terrain:{kind} was not applied: {e}"));
            }
        }
    }
    let Some(first) = first else {
        reported.line.clear();
        return;
    };
    if std::ptr::eq(reported.session.as_ptr(), std::sync::Arc::as_ptr(dm)) && reported.line == first {
        return;
    }
    let line = if refused > 1 { format!("{first} (and {} more this frame)", refused - 1) } else { first.clone() };
    reported.session = std::sync::Arc::downgrade(dm);
    reported.line = first;
    dm.lock().print(OutputLevel::Warn, "Terrain", line);
}

/// The Terrain method a command came from, for reports.
fn terrain_command_kind(command: &TerrainCommand) -> &'static str {
    match command {
        TerrainCommand::FillBall { .. } => "FillBall",
        TerrainCommand::FillBlock { .. } => "FillBlock",
        TerrainCommand::FillCylinder { .. } => "FillCylinder",
        TerrainCommand::FillRegion { .. } => "FillRegion",
        // Roblox's ReplaceMaterial between Air and Water.
        TerrainCommand::ReplaceMaterial { .. } | TerrainCommand::FillWater { .. } | TerrainCommand::DrainWater { .. } => {
            "ReplaceMaterial"
        }
        TerrainCommand::WriteVoxels { .. } => "WriteVoxels",
        TerrainCommand::Sculpt { .. } => "Sculpt",
        TerrainCommand::Paint { .. } => "Paint",
        TerrainCommand::Clear => "Clear",
    }
}
