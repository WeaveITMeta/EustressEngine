//! Dialog / DialogChoice runtime — NPC conversation panel.
//!
//! Roblox `Dialog` shows a conversation UI when the local character is within
//! `conversation_distance` of the Dialog's parent (an NPC). The panel shows
//! the `initial_prompt` and one button per child `DialogChoice`; picking a
//! choice shows its `response_dialog` and fires the choice event. The dialog
//! has `goodbye_dialog` for the end state.
//!
//! ## Systems
//!
//! - [`dialog_proximity_system`] — opens the nearest in-range Dialog into
//!   [`ActiveDialog`] (and builds the panel UI), or closes the panel when the
//!   character walks away. Mirrors Roblox's auto-open behavior.
//! - [`dialog_choice_click_system`] — handles clicks on the choice buttons:
//!   fires the choice event on the [`EventBus`] and advances the conversation
//!   (shows the response, then re-offers the choices). The "Goodbye" button
//!   closes the dialog.
//!
//! ## UI
//!
//! Built from raw Bevy UI nodes (the same primitives the `ScreenGui` spawner
//! uses — `Node` + `BackgroundColor` + `GlobalZIndex`). The panel is a single
//! on-demand root marked [`DialogPanelRoot`]; choice rows carry
//! [`DialogChoiceButton`] with the source `DialogChoice` entity so the click
//! system can resolve which choice was picked.
//!
//! ## Events
//!
//! Choice selection fires on the [`EventBus`]:
//! - per-dialog:  `Dialog.<dialog_bits>.ChoiceSelected`
//! - per-choice:  `DialogChoice.<choice_bits>.Selected`
//!
//! Payload: `[EntityId(dialog), EntityId(choice), EntityId(character)]`.
//!
//! ## No-op safety
//!
//! Resolves the local character via [`LocalCharacter`]; with no character the
//! panel closes and the systems return early.
//!
//! [`EventBus`]: eustress_common::events::EventBus

use bevy::prelude::*;

use eustress_common::classes::{Dialog, DialogChoice};
use eustress_common::events::EventBusResource;
use eustress_common::scripting::events::SignalArg;

use super::LocalCharacter;

/// The currently-open dialog (if any), plus its conversation state.
#[derive(Resource, Default)]
pub struct ActiveDialog {
    /// The open Dialog entity, or `None` (panel closed).
    pub dialog: Option<Entity>,
    /// True once the panel UI has been built for `dialog` (so we don't
    /// rebuild every frame).
    pub panel_built: bool,
}

/// Marker on the on-demand dialog panel UI root.
#[derive(Component)]
pub struct DialogPanelRoot;

/// Marker on a choice button row, carrying the source [`DialogChoice`] entity
/// it represents (or `None` for the synthetic "Goodbye" button).
#[derive(Component)]
pub struct DialogChoiceButton {
    /// The DialogChoice entity this button selects, or `None` = "Goodbye".
    pub choice: Option<Entity>,
}

/// Marker on the panel's NPC-line text node (the prompt / response line).
#[derive(Component)]
pub struct DialogPromptText;

/// Opens/closes the dialog panel based on the local character's distance to
/// each Dialog's parent, and builds the panel UI when newly opened.
pub fn dialog_proximity_system(
    mut commands: Commands,
    local: LocalCharacter,
    transforms: Query<&GlobalTransform>,
    dialogs: Query<(Entity, &Dialog, &GlobalTransform)>,
    children: Query<&Children>,
    choices: Query<(Entity, &DialogChoice)>,
    existing_panel: Query<Entity, With<DialogPanelRoot>>,
    mut active: ResMut<ActiveDialog>,
) {
    let Some(character) = local.entity() else {
        close_panel(&mut commands, &existing_panel, &mut active);
        return;
    };
    let Ok(char_transform) = transforms.get(character) else {
        return;
    };
    let char_pos = char_transform.translation();

    // Find the nearest in-range, not-in-use Dialog.
    let mut best: Option<(Entity, f32)> = None;
    for (entity, dialog, gt) in dialogs.iter() {
        if dialog.in_use && active.dialog != Some(entity) {
            continue; // another conversation owns it
        }
        let dist = char_pos.distance(gt.translation());
        if dist > dialog.conversation_distance {
            continue;
        }
        if best.as_ref().map_or(true, |(_, best_dist)| dist < *best_dist) {
            best = Some((entity, dist));
        }
    }

    match best {
        Some((dialog_entity, _)) => {
            // Switch / open.
            if active.dialog != Some(dialog_entity) {
                close_panel(&mut commands, &existing_panel, &mut active);
                active.dialog = Some(dialog_entity);
                active.panel_built = false;
            }
            if !active.panel_built {
                if let Ok((_, dialog, _)) = dialogs.get(dialog_entity) {
                    build_panel(&mut commands, dialog_entity, dialog, &children, &choices);
                    active.panel_built = true;
                }
            }
        }
        None => {
            // Out of range of everything — close.
            close_panel(&mut commands, &existing_panel, &mut active);
        }
    }
}

/// Handles clicks on dialog choice buttons: fires the choice event and
/// advances the conversation. The "Goodbye" button closes the dialog.
pub fn dialog_choice_click_system(
    mut commands: Commands,
    local: LocalCharacter,
    interactions: Query<(&Interaction, &DialogChoiceButton), Changed<Interaction>>,
    choices: Query<&DialogChoice>,
    existing_panel: Query<Entity, With<DialogPanelRoot>>,
    mut prompt_text: Query<&mut Text, With<DialogPromptText>>,
    bus: Res<EventBusResource>,
    mut active: ResMut<ActiveDialog>,
) {
    let Some(character) = local.entity() else { return };
    let Some(dialog_entity) = active.dialog else { return };

    for (interaction, button) in interactions.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.choice {
            // A real choice — fire the event + show its response.
            Some(choice_entity) => {
                let payload = vec![
                    SignalArg::EntityId(dialog_entity.to_bits()),
                    SignalArg::EntityId(choice_entity.to_bits()),
                    SignalArg::EntityId(character.to_bits()),
                ];
                bus.0.fire(
                    &format!("Dialog.{}.ChoiceSelected", dialog_entity.to_bits()),
                    payload.clone(),
                );
                bus.0.fire(
                    &format!("DialogChoice.{}.Selected", choice_entity.to_bits()),
                    payload,
                );

                // Show the response line in the prompt text node.
                if let Ok(choice) = choices.get(choice_entity) {
                    for mut text in prompt_text.iter_mut() {
                        **text = choice.response_dialog.clone();
                    }
                }
                tracing::debug!(
                    "[Dialog] choice {:?} selected on dialog {:?}",
                    choice_entity,
                    dialog_entity
                );
            }
            // "Goodbye" — close the conversation.
            None => {
                close_panel(&mut commands, &existing_panel, &mut active);
            }
        }
    }
}

/// Despawn the panel root (if present) and clear [`ActiveDialog`].
fn close_panel(
    commands: &mut Commands,
    existing_panel: &Query<Entity, With<DialogPanelRoot>>,
    active: &mut ActiveDialog,
) {
    for root in existing_panel.iter() {
        commands.entity(root).despawn();
    }
    active.dialog = None;
    active.panel_built = false;
}

/// Build the conversation panel: a bottom-anchored card with the NPC's
/// `initial_prompt` and one button per child [`DialogChoice`], plus a
/// terminal "Goodbye" button.
fn build_panel(
    commands: &mut Commands,
    dialog_entity: Entity,
    dialog: &Dialog,
    children: &Query<&Children>,
    choices: &Query<(Entity, &DialogChoice)>,
) {
    // Collect child DialogChoices of this dialog (Roblox nests them under the
    // Dialog). Up to a sane cap.
    let mut choice_rows: Vec<(Entity, String)> = Vec::new();
    if let Ok(kids) = children.get(dialog_entity) {
        for child in kids.iter() {
            if let Ok((e, choice)) = choices.get(child) {
                choice_rows.push((e, choice.user_dialog.clone()));
                if choice_rows.len() >= 8 {
                    break;
                }
            }
        }
    }

    let goodbye = dialog.goodbye_dialog.clone();
    let prompt = dialog.initial_prompt.clone();

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(24.0),
                left: Val::Percent(25.0),
                width: Val::Percent(50.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.92)),
            GlobalZIndex(130),
            DialogPanelRoot,
            Name::new("DialogPanel"),
        ))
        .with_children(|parent| {
            // NPC prompt line.
            parent.spawn((
                Text::new(prompt),
                TextColor(Color::WHITE),
                TextFont {
                    font_size: bevy::text::FontSize::Px(16.0),
                    ..default()
                },
                DialogPromptText,
            ));

            // One button per choice.
            for (choice_entity, label) in choice_rows {
                spawn_choice_button(parent, Some(choice_entity), &label);
            }

            // Terminal "Goodbye" button.
            spawn_choice_button(parent, None, &goodbye);
        });
}

/// Spawn a single clickable choice row under the panel.
fn spawn_choice_button(
    parent: &mut ChildSpawnerCommands,
    choice: Option<Entity>,
    label: &str,
) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.15, 0.22, 1.0)),
            DialogChoiceButton { choice },
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label.to_string()),
                TextColor(Color::srgb(0.85, 0.9, 1.0)),
                TextFont {
                    font_size: bevy::text::FontSize::Px(14.0),
                    ..default()
                },
            ));
        });
}
