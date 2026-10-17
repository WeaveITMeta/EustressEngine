//! The page's side of the app: the state `onState` last heard, and the stop
//! switch `PlayerHandle::stop` sets. Both hold the page's objects, so they
//! live in the app as non-send resources.

use std::cell::Cell;
use std::rc::Rc;

use bevy::prelude::*;
use wasm_bindgen::JsValue;

use crate::states;

/// What the page has been told.
pub(crate) struct PageState {
    on_state: js_sys::Function,
    current: &'static str,
}

impl PageState {
    /// host.js reports `starting` itself, before the module runs.
    pub(crate) fn new(on_state: js_sys::Function) -> Self {
        Self {
            on_state,
            current: "starting",
        }
    }

    /// Tell the page `state`, once per change. A state is one of the fixed
    /// names in [`states`], never text.
    pub(crate) fn report(&mut self, state: &'static str) {
        if self.current == state {
            return;
        }
        self.current = state;
        let _ = self.on_state.call1(&JsValue::NULL, &JsValue::from_str(state));
    }

    /// Whether the world has opened and the session not ended since.
    pub(crate) fn in_world(&self) -> bool {
        self.current == states::IN_WORLD
    }
}

/// Set by the page to end the session.
pub(crate) struct StopSwitch(pub(crate) Rc<Cell<bool>>);

/// Ends the app once the page has asked, at the end of a frame.
pub(crate) fn stop_when_asked(switch: NonSend<StopSwitch>, mut exit: MessageWriter<AppExit>) {
    if switch.0.get() {
        exit.write(AppExit::Success);
    }
}
