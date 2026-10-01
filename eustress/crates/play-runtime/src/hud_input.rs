//! # Clicks and typing on the HUD
//!
//! One hit test for both apps, over what the HUD drew last frame
//! ([`crate::hud::HudLayout`]), so a click lands on exactly the rect that was
//! drawn. It runs in `PreUpdate`, after Bevy reads input and before the Play
//! frame's scripts, and writes the session's tree directly:
//!
//! * A press on a `TextButton` or `ImageButton` pushes `GuiActivated`.
//! * A press on a visible, `TextEditable` `TextBox` gives it the keyboard
//!   (`TextBoxFocused`; `ClearTextOnFocus` empties it first). Typed text and
//!   Backspace edit its `Text` with ordinary writes, so `Changed` fires. Enter
//!   submits and releases it (`TextBoxFocusLost { enter_pressed: true }`; a new
//!   line in a `MultiLine` box instead). Escape, or a press anywhere else,
//!   releases it with `enter_pressed: false`.
//! * Scripts' `CaptureFocus` and `ReleaseFocus` requests apply first, under the
//!   same rules. A box that stops existing or drawing gives the keyboard up.
//!
//! [`HudPointer`] says whether the cursor is over the HUD and whether a box
//! holds the keyboard, so the host marks that input game-processed and keeps
//! it from the camera, the avatar and the world. While [`GuiInputBlocked`] is
//! set (Studio's own UI has the mouse, a pause menu is open) the HUD takes no
//! input.

use std::collections::HashSet;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use eustress_common::datamodel::{DataModel, DmEvent, DmValue, FocusRequest, InstanceId};
use eustress_common::play_session::{GuiKeyboardFocus, PlayDataModel};

use crate::hud::{HudItem, HudLayout};

pub use eustress_common::play_session::HudPointer;

/// Set while something else owns the mouse and keyboard: Studio's panels and
/// popups, a pause menu, a purchase prompt.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct GuiInputBlocked(pub bool);

pub struct HudInputPlugin;

impl Plugin for HudInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudPointer>()
            .init_resource::<GuiInputBlocked>()
            .init_resource::<GuiKeyboardFocus>()
            .add_systems(PreUpdate, hud_input.after(bevy::input::InputSystems));
    }
}

/// The element on top under a viewport-local point, if one takes the mouse.
fn hit(layout: &HudLayout, at: Vec2) -> Option<&HudItem> {
    layout.items.iter().rev().find(|item| {
        item.mouse_filter != "ignore"
            && !item.class_type.eq_ignore_ascii_case("screengui")
            && item.rect.contains(at)
            && item.clip.is_none_or(|c| c.contains(at))
    })
}

fn is_button(class: &str) -> bool {
    class.eq_ignore_ascii_case("textbutton") || class.eq_ignore_ascii_case("imagebutton")
}

fn flag(g: &DataModel, id: InstanceId, prop: &str, default: bool) -> bool {
    g.get_prop(id, prop).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Whether a TextBox may hold the keyboard: it exists, is editable, and the
/// HUD drew it this frame (so it is visible, in a drawn ScreenGui).
fn can_focus(g: &DataModel, id: InstanceId, drawn: &HashSet<u64>) -> bool {
    g.exists(id)
        && g.class_of(id) == Some("TextBox")
        && flag(g, id, "TextEditable", true)
        && g.entity_of(id).is_some_and(|e| drawn.contains(&e))
}

/// Move the keyboard to `next` (or nowhere), firing the box events.
fn set_focus(g: &mut DataModel, next: Option<InstanceId>, enter_pressed: bool) {
    if g.focused_textbox == next {
        return;
    }
    if let Some(old) = g.focused_textbox.take() {
        g.push_event(DmEvent::TextBoxFocusLost { textbox: old, enter_pressed });
    }
    if let Some(new) = next {
        g.focused_textbox = Some(new);
        if flag(g, new, "ClearTextOnFocus", true) {
            let _ = g.set_prop(new, "Text", DmValue::String(String::new()));
        }
        g.push_event(DmEvent::TextBoxFocused { textbox: new });
    }
}

#[allow(clippy::too_many_arguments)]
fn hud_input(
    tree: Option<Res<PlayDataModel>>,
    layout: Res<HudLayout>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: MessageReader<KeyboardInput>,
    blocked: Res<GuiInputBlocked>,
    mut pointer: ResMut<HudPointer>,
    mut keyboard: ResMut<GuiKeyboardFocus>,
) {
    pointer.activated.clear();
    let Some(tree) = tree else {
        keys.clear();
        pointer.over_gui = false;
        pointer.typing = false;
        if keyboard.0 {
            keyboard.0 = false;
        }
        return;
    };
    let mut g = tree.dm.lock();
    let drawn: HashSet<u64> = layout.items.iter().map(|i| i.entity.to_bits()).collect();

    // Scripts' requests first, then the held box must still be drawable.
    for request in std::mem::take(&mut g.focus_requests) {
        match request {
            FocusRequest::Capture { textbox } => {
                if can_focus(&g, textbox, &drawn) {
                    set_focus(&mut g, Some(textbox), false);
                }
            }
            FocusRequest::Release { textbox, submitted } => {
                if g.focused_textbox == Some(textbox) {
                    set_focus(&mut g, None, submitted);
                }
            }
        }
    }
    if let Some(held) = g.focused_textbox {
        if !can_focus(&g, held, &drawn) {
            set_focus(&mut g, None, false);
        }
    }

    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .map(|c| c - layout.origin)
        .filter(|c| c.x >= 0.0 && c.y >= 0.0 && c.x <= layout.viewport.x && c.y <= layout.viewport.y);
    let under = if blocked.0 { None } else { cursor.and_then(|c| hit(&layout, c)) };
    pointer.over_gui = under.is_some_and(|item| item.mouse_filter != "pass");

    if !blocked.0 && mouse.just_pressed(MouseButton::Left) {
        let target = under.and_then(|item| Some((item, g.by_entity(item.entity.to_bits())?)));
        let mut keep_focus = false;
        if let Some((item, id)) = target {
            if is_button(&item.class_type) {
                g.push_event(DmEvent::GuiActivated { button: id });
                pointer.activated.push(item.entity);
            } else if item.class_type.eq_ignore_ascii_case("textbox") && can_focus(&g, id, &drawn) {
                set_focus(&mut g, Some(id), false);
                keep_focus = true;
            }
        }
        if !keep_focus && g.focused_textbox.is_some() {
            set_focus(&mut g, None, false);
        }
    }

    // Typing into the held box.
    let events: Vec<KeyboardInput> = keys.read().cloned().collect();
    if let (Some(textbox), false) = (g.focused_textbox, blocked.0) {
        let multi_line = flag(&g, textbox, "MultiLine", false);
        let mut text = g.get_prop(textbox, "Text").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let before = text.clone();
        let mut release: Option<bool> = None;
        for event in events.iter().filter(|e| e.state.is_pressed()) {
            match &event.logical_key {
                Key::Enter if multi_line => text.push('\n'),
                Key::Enter => {
                    release = Some(true);
                    break;
                }
                Key::Escape => {
                    release = Some(false);
                    break;
                }
                Key::Backspace => {
                    text.pop();
                }
                _ => {
                    if let Some(typed) = &event.text {
                        text.extend(typed.chars().filter(|c| !c.is_control()));
                    }
                }
            }
        }
        if text != before {
            let _ = g.set_prop(textbox, "Text", DmValue::String(text));
        }
        if let Some(enter_pressed) = release {
            set_focus(&mut g, None, enter_pressed);
        }
    }

    pointer.typing = g.focused_textbox.is_some();
    if keyboard.0 != pointer.typing {
        keyboard.0 = pointer.typing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(entity: u32, rect: Rect, clip: Option<Rect>, class: &str, filter: &str) -> HudItem {
        HudItem {
            entity: Entity::from_raw_u32(entity).unwrap(),
            rect,
            clip,
            class_type: class.into(),
            mouse_filter: filter.into(),
        }
    }

    #[test]
    fn the_top_element_under_the_cursor_takes_the_click() {
        let layout = HudLayout {
            viewport: Vec2::new(800.0, 600.0),
            origin: Vec2::ZERO,
            scale: 1.0,
            items: vec![
                item(1, Rect::new(0.0, 0.0, 200.0, 200.0), None, "Frame", "stop"),
                item(2, Rect::new(50.0, 50.0, 150.0, 80.0), None, "TextButton", "stop"),
                // On top, but invisible to the mouse.
                item(3, Rect::new(0.0, 0.0, 800.0, 600.0), None, "TextLabel", "ignore"),
                // Clipped away where the cursor is.
                item(4, Rect::new(60.0, 60.0, 90.0, 70.0), Some(Rect::new(0.0, 0.0, 40.0, 40.0)), "TextButton", "stop"),
            ],
        };
        let under = hit(&layout, Vec2::new(70.0, 65.0)).unwrap();
        assert_eq!(under.entity.index_u32(), 2);
        assert_eq!(hit(&layout, Vec2::new(10.0, 10.0)).unwrap().entity.index_u32(), 1);
        assert!(hit(&layout, Vec2::new(500.0, 500.0)).is_none());
    }

    #[test]
    fn focus_moves_with_its_events_and_clears_the_text() {
        let mut g = DataModel::new();
        let a = g.create("TextBox");
        let b = g.create("TextBox");
        g.set_prop(a, "Text", DmValue::String("old".into())).unwrap();
        g.set_prop(b, "ClearTextOnFocus", DmValue::Bool(false)).unwrap();
        g.set_prop(b, "Text", DmValue::String("kept".into())).unwrap();
        let start = g.event_cursor();

        set_focus(&mut g, Some(a), false);
        assert_eq!(g.focused_textbox, Some(a));
        assert_eq!(g.get_prop(a, "Text").and_then(|v| v.as_str().map(str::to_string)).as_deref(), Some(""));
        set_focus(&mut g, Some(b), false);
        set_focus(&mut g, None, true);
        assert_eq!(g.get_prop(b, "Text").and_then(|v| v.as_str().map(str::to_string)).as_deref(), Some("kept"));

        let (events, _) = g.events_since(start);
        let focus: Vec<_> = events
            .into_iter()
            .filter(|e| matches!(e, DmEvent::TextBoxFocused { .. } | DmEvent::TextBoxFocusLost { .. }))
            .collect();
        assert!(matches!(focus[0], DmEvent::TextBoxFocused { textbox } if textbox == a));
        assert!(matches!(focus[1], DmEvent::TextBoxFocusLost { textbox, enter_pressed: false } if textbox == a));
        assert!(matches!(focus[2], DmEvent::TextBoxFocused { textbox } if textbox == b));
        assert!(matches!(focus[3], DmEvent::TextBoxFocusLost { textbox, enter_pressed: true } if textbox == b));
        assert_eq!(focus.len(), 4);
    }
}
