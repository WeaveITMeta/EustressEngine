// =============================================================================
// Eustress Web - A menu of actions behind a "..." button
// =============================================================================
// Used on a project card. Three things the old inline menu got wrong:
//   - It sat inside the card, so the card's overflow clipped it (Archive was cut off). The open menu is placed in the
//     page's body (a portal) with fixed coordinates taken from the button, and flips above the button when there is no room
//     below.
//   - Its items ran their handlers after the menu had already been removed by the same click, so some did nothing (and the
//     page threw "closure invoked recursively or after being dropped"). An item closes the menu and runs its action once the
//     click has finished (utils::after_event).
//   - It had no keyboard model. Enter, Space or Down on the button opens it with the first item focused; Down, Up, Home and End
//     move; Escape closes and returns focus to the button; Tab closes. Items are buttons with role="menuitem".
// Every item does something: a link inside the site, a link to another application (Studio's eustress:// edit link), or a callback.
// =============================================================================

use leptos::portal::Portal;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::utils::after_event;

/// What an item does when chosen.
#[derive(Clone)]
pub enum Action {
    /// Go to another page of this site.
    Go(String),
    /// Hand a link to the browser: another application's scheme (`eustress://edit/<id>`) or another site.
    Open(String),
    /// Run a callback.
    Call(Callback<()>),
}

/// One item of the menu.
#[derive(Clone)]
pub struct Item {
    pub label: &'static str,
    /// An icon under /assets/icons/.
    pub icon: &'static str,
    pub action: Action,
    /// Drawn in the warning colour: it removes or undoes something.
    pub danger: bool,
    /// A rule above this item.
    pub divider_before: bool,
}

impl Item {
    pub fn new(label: &'static str, icon: &'static str, action: Action) -> Self {
        Self { label, icon, action, danger: false, divider_before: false }
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn divided(mut self) -> Self {
        self.divider_before = true;
        self
    }
}

/// Which item has the focus after `key`, among `len` items, when `current` has it (or nothing does). `None` when the key does not
/// move focus.
pub fn move_focus(current: Option<usize>, len: usize, key: &str) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match (key, current) {
        ("ArrowDown", None) | ("Home", _) => Some(0),
        ("ArrowDown", Some(i)) => Some((i + 1) % len),
        ("ArrowUp", None) | ("End", _) => Some(len - 1),
        ("ArrowUp", Some(i)) => Some((i + len - 1) % len),
        _ => None,
    }
}

/// Where the open menu goes: fixed coordinates from the button's box and the window's size. Below the button, right edges lined
/// up; above it when it would not fit below and does fit above; never off the left or right edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub top: f64,
    pub left: f64,
}

pub fn placement(button_top: f64, button_bottom: f64, button_right: f64, window_w: f64, window_h: f64, menu_w: f64, menu_h: f64) -> Placement {
    const GAP: f64 = 4.0;
    const EDGE: f64 = 8.0;
    let below = button_bottom + GAP;
    let fits_below = below + menu_h + EDGE <= window_h;
    let above = button_top - GAP - menu_h;
    let top = if fits_below || above < EDGE { below.min((window_h - menu_h - EDGE).max(EDGE)) } else { above };
    let left = (button_right - menu_w).clamp(EDGE, (window_w - menu_w - EDGE).max(EDGE));
    Placement { top, left }
}

const MENU_WIDTH: f64 = 208.0;
const ROW_HEIGHT: f64 = 40.0;

#[component]
pub fn ActionMenu(
    /// What the button is called for a screen reader ("Options for Box Head").
    #[prop(into)]
    label: String,
    items: Vec<Item>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let place = RwSignal::new(Placement { top: 0.0, left: 0.0 });
    let trigger = NodeRef::<leptos::html::Button>::new();
    let refs: Vec<NodeRef<leptos::html::Button>> = items.iter().map(|_| NodeRef::new()).collect();
    let refs = StoredValue::new_local(refs);
    let count = items.len();
    let items = StoredValue::new_local(items);
    let navigate = StoredValue::new_local(use_navigate());

    let focus_item = move |i: usize| {
        refs.with_value(|r| {
            if let Some(button) = r.get(i).and_then(|n| n.get_untracked()) {
                let _ = button.focus();
            }
        });
    };

    let open_menu = move || {
        let (top, bottom, right) = trigger.get_untracked().map(|b| {
            let r = b.get_bounding_client_rect();
            (r.top(), r.bottom(), r.right())
        }).unwrap_or((0.0, 0.0, MENU_WIDTH));
        let (w, h) = web_sys::window().map(|w| (
            w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1024.0),
            w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(768.0),
        )).unwrap_or((1024.0, 768.0));
        let menu_h = count as f64 * ROW_HEIGHT + 16.0;
        place.set(placement(top, bottom, right, w, h, MENU_WIDTH, menu_h));
        open.set(true);
        // The menu is in the page once this click has finished; the first item takes the focus then.
        after_event(move || focus_item(0));
    };

    // Closing and running an action both wait for the click to finish: the click is still travelling through elements this removes.
    let close = move |return_focus: bool| {
        after_event(move || {
            open.set(false);
            if return_focus {
                if let Some(b) = trigger.get_untracked() {
                    let _ = b.focus();
                }
            }
        });
    };

    let choose = move |i: usize| {
        let Some(item) = items.with_value(|all| all.get(i).cloned()) else { return };
        after_event(move || {
            open.set(false);
            match item.action {
                Action::Go(path) => navigate.with_value(|go| go(&path, Default::default())),
                Action::Open(link) => {
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().set_href(&link);
                    }
                }
                Action::Call(callback) => callback.run(()),
            }
        });
    };

    let on_trigger_key = move |ev: leptos::ev::KeyboardEvent| {
        if matches!(ev.key().as_str(), "ArrowDown" | "ArrowUp" | "Enter" | " ") && !open.get_untracked() {
            ev.prevent_default();
            open_menu();
        }
    };

    let on_menu_key = move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();
        match key.as_str() {
            "Escape" => {
                ev.prevent_default();
                close(true);
            }
            "Tab" => close(false),
            _ => {
                let current = refs.with_value(|r| {
                    let active = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element());
                    r.iter().position(|n| match (n.get_untracked(), &active) {
                        (Some(button), Some(a)) => {
                            let el: &web_sys::Element = button.as_ref();
                            el == a
                        }
                        _ => false,
                    })
                });
                if let Some(next) = move_focus(current, count, &key) {
                    ev.prevent_default();
                    focus_item(next);
                }
            }
        }
    };

    let rows = items;

    view! {
        <button
            class="menu-trigger"
            node_ref=trigger
            aria-haspopup="menu"
            aria-expanded=move || open.get().to_string()
            aria-label=label
            title="More options"
            on:click=move |_| if open.get_untracked() { close(false) } else { open_menu() }
            on:keydown=on_trigger_key
        >
            <img src="/assets/icons/more.svg" alt="" />
        </button>
        <Show when=move || open.get()>
            <Portal>
                <div class="am-layer">
                    <div class="am-backdrop" on:click=move |_| close(false)></div>
                    <div
                        class="am-menu"
                        role="menu"
                        style=move || { let p = place.get(); format!("top:{}px;left:{}px;width:{}px", p.top, p.left, MENU_WIDTH) }
                        on:keydown=on_menu_key
                    >
                        {rows.get_value().into_iter().enumerate().map(|(i, item)| {
                            let node = refs.with_value(|r| r[i]);
                            let class = if item.danger { "am-item danger" } else { "am-item" };
                            view! {
                                {item.divider_before.then(|| view! { <div class="am-divider" role="separator"></div> })}
                                <button class=class role="menuitem" node_ref=node on:click=move |_| choose(i)>
                                    <img src=format!("/assets/icons/{}", item.icon) alt="" />
                                    {item.label}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                </div>
            </Portal>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_keys_move_around_the_items_and_wrap() {
        assert_eq!(move_focus(None, 4, "ArrowDown"), Some(0));
        assert_eq!(move_focus(None, 4, "ArrowUp"), Some(3));
        assert_eq!(move_focus(Some(0), 4, "ArrowDown"), Some(1));
        assert_eq!(move_focus(Some(3), 4, "ArrowDown"), Some(0), "past the last is the first");
        assert_eq!(move_focus(Some(0), 4, "ArrowUp"), Some(3), "before the first is the last");
        assert_eq!(move_focus(Some(2), 4, "Home"), Some(0));
        assert_eq!(move_focus(Some(1), 4, "End"), Some(3));
        for other in ["a", "Enter", "Escape", "Tab", "ArrowLeft"] {
            assert_eq!(move_focus(Some(1), 4, other), None, "{other} does not move focus");
        }
        assert_eq!(move_focus(None, 0, "ArrowDown"), None, "no items, nowhere to go");
    }

    #[test]
    fn the_menu_opens_below_the_button_with_its_right_edge_under_it_and_flips_above_when_there_is_no_room() {
        // Plenty of room: just under the button, right edges together.
        let p = placement(100.0, 130.0, 500.0, 1200.0, 800.0, 208.0, 200.0);
        assert_eq!((p.top, p.left), (134.0, 292.0));
        // At the bottom of the window: above the button instead (this is what a clipped Archive needed).
        let p = placement(700.0, 730.0, 500.0, 1200.0, 800.0, 208.0, 200.0);
        assert_eq!(p.top, 700.0 - 4.0 - 200.0);
        // No room above either (a short window): kept inside the window, not off it.
        let p = placement(50.0, 80.0, 500.0, 1200.0, 300.0, 208.0, 400.0);
        assert!(p.top >= 8.0 && p.top <= 300.0, "{}", p.top);
        // Never off the left or the right edge.
        assert_eq!(placement(100.0, 130.0, 100.0, 1200.0, 800.0, 208.0, 200.0).left, 8.0);
        assert_eq!(placement(100.0, 130.0, 1500.0, 1200.0, 800.0, 208.0, 200.0).left, 1200.0 - 208.0 - 8.0);
    }

    #[test]
    fn an_item_says_what_it_is_and_what_it_does() {
        let archive = Item::new("Archive", "archive.svg", Action::Call(Callback::new(|_| {}))).danger().divided();
        assert!(archive.danger && archive.divider_before);
        let edit = Item::new("Edit", "edit.svg", Action::Open("eustress://edit/abc".to_string()));
        assert!(!edit.danger && !edit.divider_before);
        assert!(matches!(edit.action, Action::Open(ref l) if l == "eustress://edit/abc"));
    }
}
