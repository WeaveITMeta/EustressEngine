//! Studio's purchase dialog (`purchase_prompt.slint`): what a script's
//! `PromptProductPurchase` or `PromptGamePassPurchase` shows the player Studio
//! plays as.
//!
//! Commerce (`play_datamodel/commerce.rs`) queues each purchase in
//! [`PurchaseConfirm`]; this shows the one on screen and hands back the
//! player's answer. Studio's purchases are test purchases, and the dialog
//! says so. While it is up the character stands still and the cursor is
//! free, so a first-person game can be clicked out of; both come back as they
//! were when it closes.
//!
//! A separate plugin, like the Timeline's sync: it needs none of the drain's
//! event writers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use eustress_common::avatar::control::AvatarInputEnabled;
use eustress_common::datamodel::ProductKind;

use crate::play_datamodel::commerce::PurchaseConfirm;

/// Must be added after the Slint UI plugin, so `SlintUiState` exists.
pub struct PurchasePromptUiPlugin;

impl Plugin for PurchasePromptUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PurchaseConfirm>()
            .init_resource::<PurchasePromptAnswers>()
            .add_systems(
                Update,
                (note_purchase_shown, register_purchase_prompt, apply_purchase_answers, sync_purchase_prompt).chain(),
            );
    }
}

/// One past the last frame (`FrameCount`) a purchase was on screen; 0 before
/// the first.
static SHOWN_UNTIL: AtomicU64 = AtomicU64::new(0);

/// Whether the purchase dialog still holds the keyboard in `frame`: while it
/// is up, and on the frame after, because the Escape that cancels it hides it
/// before Studio's focus check runs, and that Escape must not also stop Play.
pub fn holds_keys(frame: u32) -> bool {
    let until = SHOWN_UNTIL.load(Ordering::Relaxed);
    until != 0 && u64::from(frame) <= until
}

fn note_purchase_shown(confirm: Res<PurchaseConfirm>, frames: Res<FrameCount>) {
    if confirm.showing.is_some() {
        SHOWN_UNTIL.store(u64::from(frames.0) + 1, Ordering::Relaxed);
    }
}

/// Filled by the dialog's callback, drained by `apply_purchase_answers`.
#[derive(Resource, Default)]
struct PurchasePromptAnswers(Arc<Mutex<Vec<bool>>>);

/// What the dialog took from the game while it is up, to give back.
struct Held {
    input: Option<bool>,
    cursor: Option<(CursorGrabMode, bool)>,
}

/// Hook the dialog's callback once the Slint window exists.
fn register_purchase_prompt(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    answers: Res<PurchasePromptAnswers>,
    mut registered: Local<bool>,
) {
    if *registered {
        return;
    }
    let Some(slint) = slint else { return };
    let queue = answers.0.clone();
    slint.window.on_purchase_prompt_answered(move |buy| {
        if let Ok(mut pending) = queue.lock() {
            pending.push(buy);
        }
    });
    *registered = true;
}

/// The player answered the purchase on screen.
fn apply_purchase_answers(answers: Res<PurchasePromptAnswers>, mut confirm: ResMut<PurchaseConfirm>) {
    let answered: Vec<bool> = match answers.0.lock() {
        Ok(mut pending) if !pending.is_empty() => std::mem::take(&mut *pending),
        _ => return,
    };
    // One purchase is on screen at a time, and the dialog closes on its first
    // answer; a purchase cleared by Stop takes none.
    if confirm.showing.is_some() && confirm.answer.is_none() {
        confirm.answer = answered.first().copied();
    }
}

/// Show the purchase on screen, or hide the dialog, whenever it changes.
fn sync_purchase_prompt(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    confirm: Res<PurchaseConfirm>,
    mut input: Option<ResMut<AvatarInputEnabled>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut shown: Local<Option<u64>>,
    mut held: Local<Option<Held>>,
) {
    let Some(slint) = slint else { return };
    if *shown == Some(confirm.generation) {
        return;
    }
    *shown = Some(confirm.generation);
    let ui = &slint.window;
    let Some((_, product)) = &confirm.showing else {
        ui.set_show_purchase_prompt(false);
        if let Some(held) = held.take() {
            if let (Some(was), Some(input)) = (held.input, input.as_deref_mut()) {
                input.0 = was;
            }
            // Only while Play runs: a Stop hands the cursor back to the
            // editor, and a first-person lock put back here would outlive it.
            let playing = play_state.as_ref().is_some_and(|s| {
                matches!(s.get(), crate::play_mode::PlayModeState::Playing | crate::play_mode::PlayModeState::Paused)
            });
            if let (true, Some((grab, visible)), Ok(mut cursor)) = (playing, held.cursor, cursors.single_mut()) {
                cursor.grab_mode = grab;
                cursor.visible = visible;
            }
        }
        return;
    };
    ui.set_purchase_prompt_product(product.name.as_str().into());
    ui.set_purchase_prompt_kind(kind_text(product.kind).into());
    ui.set_purchase_prompt_description(product.description.as_str().into());
    ui.set_purchase_prompt_price(price_text(product.price).into());
    ui.set_purchase_prompt_test_mode(true);
    ui.set_show_purchase_prompt(true);
    if held.is_none() {
        let was_input = input.as_deref_mut().map(|input| std::mem::replace(&mut input.0, false));
        let was_cursor = cursors.single_mut().ok().map(|mut cursor| {
            let was = (cursor.grab_mode, cursor.visible);
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            was
        });
        *held = Some(Held { input: was_input, cursor: was_cursor });
    }
}

fn kind_text(kind: ProductKind) -> &'static str {
    match kind {
        ProductKind::Consumable => "Product",
        ProductKind::Pass => "Pass: owned once, kept every session",
    }
}

/// "1,250 Tickets".
fn price_text(tickets: u64) -> String {
    let digits = tickets.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped} {}", if tickets == 1 { "Ticket" } else { "Tickets" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_read_in_whole_tickets_with_grouped_thousands() {
        assert_eq!(price_text(0), "0 Tickets");
        assert_eq!(price_text(1), "1 Ticket");
        assert_eq!(price_text(50), "50 Tickets");
        assert_eq!(price_text(999), "999 Tickets");
        assert_eq!(price_text(1_000), "1,000 Tickets");
        assert_eq!(price_text(1_250_000), "1,250,000 Tickets");
    }
}
