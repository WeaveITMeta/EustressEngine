//! # Commit-success flash (Phase 0 UX polish)
//!
//! 150ms `accent-green-bright` border pulse anchored to the
//! ToolOptionsBar whenever a ModalTool commits. Pure visual feedback
//! — no ECS or data effect, just "your action landed" signal.
//!
//! ## How it works
//!
//! `CommitFlashState { progress: f32 }` resource. On every
//! `ModalToolCommittedEvent`, set `progress = 1.0`. A per-frame
//! system linearly decays progress to 0 over 150ms. Slint reads the
//! value each frame and renders the border pulse at `opacity =
//! progress`.

use bevy::prelude::*;

#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CommitFlashState {
    /// 1.0 at commit, decays to 0 over `FLASH_DURATION` seconds.
    pub progress: f32,
}

const FLASH_DURATION: f32 = 0.15; // 150ms per TOOLSET_UX.md §2.4

pub struct CommitFlashPlugin;

impl Plugin for CommitFlashPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommitFlashState>()
            .add_systems(Update, (trigger_commit_flash, decay_commit_flash));
    }
}

fn trigger_commit_flash(
    mut events: MessageReader<crate::modal_tool::ModalToolCommittedEvent>,
    mut state: ResMut<CommitFlashState>,
) {
    for _ in events.read() {
        state.progress = 1.0;
    }
}

fn decay_commit_flash(
    time: Res<Time>,
    mut state: ResMut<CommitFlashState>,
) {
    if state.progress <= 0.0 { return; }
    state.progress -= time.delta_secs() / FLASH_DURATION;
    if state.progress < 0.0 { state.progress = 0.0; }
}
