//! `start_player`, the export `/player/host.js` calls.

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::join::plan_join;

/// The canvas the Player draws into, made inside the page's `canvasParent`.
const CANVAS_ID: &str = "eustress-player-canvas";

/// The name a joining player shows without one from the page, as the
/// desktop Player's last fallback (client `main.rs`).
const DEFAULT_NAME: &str = "Player";

thread_local! {
    /// Whether this page has started its one session.
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

/// The page's handle on the session.
#[wasm_bindgen]
pub struct PlayerHandle {
    stop: Rc<Cell<bool>>,
    canvas: web_sys::HtmlCanvasElement,
    _focus: Closure<dyn FnMut(web_sys::Event)>,
}

#[wasm_bindgen]
impl PlayerHandle {
    /// End the session: the app ends on its next frame, which closes the
    /// connection to the host, and the canvas leaves the page.
    pub fn stop(&self) {
        self.stop.set(true);
        self.canvas.remove();
    }
}

/// `start_player({ canvasParent, apiBase, assetBase, join, onState, name? })`:
/// join the session `join` names and draw it inside `canvasParent`.
/// `onState` hears every state by name ([`crate::states`]). Every error is a
/// fixed code; none quotes an argument.
///
/// A published world played alone (`simId`, with no `join`) is not in this
/// build: a page starts a session with a join link.
#[wasm_bindgen]
pub fn start_player(args: JsValue) -> Result<PlayerHandle, JsValue> {
    console_error_panic_hook::set_once();
    let fail = |code: &str| JsValue::from_str(code);
    if STARTED.with(Cell::get) {
        return Err(fail("already-started"));
    }
    let field = |name: &str| js_sys::Reflect::get(&args, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED);
    let parent: web_sys::HtmlElement = field("canvasParent").dyn_into().map_err(|_| fail("bad-arguments"))?;
    let api_base = field("apiBase").as_string().ok_or_else(|| fail("bad-arguments"))?;
    let asset_base = field("assetBase").as_string().ok_or_else(|| fail("bad-arguments"))?;
    let on_state: js_sys::Function = field("onState").dyn_into().map_err(|_| fail("bad-arguments"))?;
    let name = field("name")
        .as_string()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_NAME.to_string());
    let join = field("join").as_string().ok_or_else(|| fail("no-join-link"))?;
    let plan = plan_join(&join, &api_base).map_err(|refusal| fail(refusal.code()))?;
    drop(join);

    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| fail("no-document"))?;
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .ok()
        .and_then(|element| element.dyn_into().ok())
        .ok_or_else(|| fail("no-canvas"))?;
    canvas.set_id(CANVAS_ID);
    let style = canvas.style();
    for (property, value) in [
        ("width", "100%"),
        ("height", "100%"),
        ("display", "block"),
        ("outline", "none"),
    ] {
        let _ = style.set_property(property, value);
    }
    // Keys reach the canvas only while it has the focus: it can take it, it
    // takes it now, and a click gives it back.
    canvas.set_tab_index(0);
    parent.append_child(&canvas).map_err(|_| fail("no-canvas"))?;
    let focus = {
        let canvas = canvas.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let _ = canvas.focus();
        })
    };
    let _ = canvas.add_event_listener_with_callback("pointerdown", focus.as_ref().unchecked_ref());
    let _ = canvas.focus();

    let stop = Rc::new(Cell::new(false));
    STARTED.with(|started| started.set(true));
    let mut app = crate::app::build(crate::app::Start {
        canvas: format!("#{CANVAS_ID}"),
        asset_base,
        plan,
        name,
        on_state,
        stop: stop.clone(),
    });
    // In a browser this hands the app to the page's event loop and returns.
    let _ = app.run();
    Ok(PlayerHandle {
        stop,
        canvas,
        _focus: focus,
    })
}
