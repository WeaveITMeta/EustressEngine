//! # Interaction runtime — Wave 6.D
//!
//! Runtime behavior for the 11 interaction / character-appearance classes
//! whose spawners live in [`crate::spawners::interaction`]. The spawners
//! attach the *config* components; the systems here make them *do* something
//! at play time.
//!
//! ## Submodules
//!
//! - [`equip`]    — Tool / Accessory equip-unequip, a number-key hotbar, and
//!   `Activated` firing onto the [`EventBus`].
//! - [`click`]    — ClickDetector: camera raycast → `MouseClick`,
//!   `MouseHoverEnter`, `MouseHoverLeave`.
//! - [`proximity`]— ProximityPrompt: per-frame distance (+ optional
//!   line-of-sight) check, a prompt overlay, hold-to-`Triggered`.
//! - [`dialog_ui`]— Dialog / DialogChoice: a conversation panel built from
//!   the initial prompt and the child `DialogChoice` rows.
//! - [`appearance`]— BodyColors / CharacterMesh / Shirt / Pants /
//!   ShirtGraphic applied to the local character's rendered limbs.
//!
//! ## Local-character resolution
//!
//! Every interactive system needs "the local player's character entity". In
//! the editor's Play Mode that is the entity carrying
//! [`PlayModeCharacter`][pmc] (spawned by `play_mode_runtime`), which also
//! carries [`Character`]. The shared [`local_character`] helper resolves it
//! and the systems **no-op gracefully** when no character exists yet (editor
//! mode, pre-spawn). It also consults [`PlayerService::local_player`] →
//! `Player.character` as a fallback for the networked-client path.
//!
//! ## State gating
//!
//! ClickDetector, ProximityPrompt, Dialog, and the hotbar/equip activation
//! run only while [`PlayModeState::Playing`] — they're gameplay, not editing.
//! Appearance application runs in every state (a `BodyColors` dropped onto a
//! character in the editor should recolor immediately for authoring preview)
//! but is change-detected so it costs nothing on idle frames.
//!
//! [pmc]: eustress_common::plugins::character_plugin::PlayModeCharacter
//! [`Character`]: eustress_common::services::player::Character
//! [`EventBus`]: eustress_common::events::EventBus
//! [`PlayerService::local_player`]: eustress_common::services::player::PlayerService

use bevy::prelude::*;

use eustress_common::plugins::character_plugin::PlayModeCharacter;
use eustress_common::services::player::PlayerService;

pub mod appearance;
pub mod click;
pub mod dialog_ui;
pub mod equip;
pub mod proximity;

// ─────────────────────────────────────────────────────────────────────────
// Local-character resolution (shared by every interaction system)
// ─────────────────────────────────────────────────────────────────────────

/// A read-only system-param bundle that resolves "the local player's
/// character entity" once, so each interaction system doesn't re-derive it.
///
/// Resolution order:
/// 1. The entity carrying [`PlayModeCharacter`] (editor Play Mode — the
///    common case). If several exist (shouldn't), the first is used.
/// 2. `PlayerService.local_player` → that `Player`'s `.character` field
///    (networked-client path).
///
/// Returns `None` when no character exists yet — every caller treats that as
/// "no-op this frame".
#[derive(bevy::ecs::system::SystemParam)]
pub struct LocalCharacter<'w, 's> {
    /// Play-mode character marker query (primary source).
    pub play_mode_chars: Query<'w, 's, Entity, With<PlayModeCharacter>>,
    /// PlayerService for the networked-client fallback.
    pub player_service: Option<Res<'w, PlayerService>>,
    /// Player components for the fallback lookup.
    pub players: Query<'w, 's, &'static eustress_common::services::player::Player>,
}

impl LocalCharacter<'_, '_> {
    /// Resolve the local character entity, or `None` if there isn't one.
    pub fn entity(&self) -> Option<Entity> {
        if let Some(e) = self.play_mode_chars.iter().next() {
            return Some(e);
        }
        // Networked-client fallback: PlayerService.local_player → Player.character.
        let service = self.player_service.as_ref()?;
        let local_player = service.local_player?;
        let player = self.players.get(local_player).ok()?;
        player.character
    }
}

/// Free-function variant for systems that already hold the two queries and
/// don't want the [`LocalCharacter`] system-param. Mirrors
/// [`LocalCharacter::entity`].
pub fn resolve_local_character(
    play_mode_chars: &Query<Entity, With<PlayModeCharacter>>,
    player_service: Option<&PlayerService>,
    players: &Query<&eustress_common::services::player::Player>,
) -> Option<Entity> {
    if let Some(e) = play_mode_chars.iter().next() {
        return Some(e);
    }
    let service = player_service?;
    let local_player = service.local_player?;
    players.get(local_player).ok().and_then(|p| p.character)
}

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

/// Adds every Wave 6.D interaction runtime system.
///
/// Mount via `app.add_plugins(InteractionPlugin)` in `SlintUiPlugin::build`
/// (alongside the spawner sub-plugins). Requires:
/// - the [`EventBusResource`][eustress_common::events::EventBusResource]
///   (mounted by `EventBusPlugin` — add it first if not already present;
///   `InteractionPlugin::build` inserts it defensively if missing),
/// - the [`PlayModeState`] states enum (registered by the play-mode plugin),
/// - Avian's `SpatialQuery` (the physics plugin).
///
/// Gameplay systems (`click`, `proximity`, `dialog_ui`, equip activation /
/// hotbar) are gated to [`PlayModeState::Playing`]. Appearance + equip-weld
/// upkeep run in all states (change-detected, so cheap when idle).
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        use crate::play_mode::PlayModeState;
        use eustress_common::events::EventBusResource;

        // The EventBus is the firing surface for MouseClick / Triggered /
        // Activated / dialog-choice events. Insert defensively so the plugin
        // works even if EventBusPlugin hasn't been added yet (idempotent —
        // init_resource only inserts when absent).
        app.init_resource::<EventBusResource>();

        // Resources owned by this module's submodules.
        app.init_resource::<equip::Hotbar>();
        app.init_resource::<proximity::ActivePrompt>();
        app.init_resource::<dialog_ui::ActiveDialog>();
        app.init_resource::<click::HoverState>();

        // ── Always-on (every state), change-detected ────────────────────
        app.add_systems(
            Update,
            (
                appearance::apply_body_colors_system,
                appearance::apply_clothing_system,
                appearance::apply_character_mesh_system,
                equip::weld_equipped_tools_system,
            ),
        );

        // ── Gameplay only (PlayModeState::Playing) ───────────────────────
        app.add_systems(
            Update,
            (
                equip::hotbar_select_system,
                equip::tool_activate_system,
                click::click_detector_system,
                click::click_hover_system,
                proximity::proximity_prompt_system,
                proximity::proximity_overlay_system,
                dialog_ui::dialog_proximity_system,
                dialog_ui::dialog_choice_click_system,
            )
                .run_if(in_state(PlayModeState::Playing)),
        );

        tracing::info!("InteractionPlugin initialized (Wave 6.D)");
    }
}
