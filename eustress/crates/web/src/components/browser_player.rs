// The full-page browser Player. The Play chooser opens it when the visitor chose
// "In your browser": a hosted session passes its join link in memory, and a
// published world played alone passes only its Gallery id. The link is never
// written into the page, the address, or storage.

use leptos::prelude::*;
use leptos::task::spawn_local;

/// What the page asks `/player/host.js` to open. `join` is the Player link,
/// ticket included, and is not rendered.
#[derive(Clone)]
pub struct BrowserLaunch {
    pub sim_id: String,
    pub api_base: String,
    pub join: Option<String>,
}

#[component]
pub fn BrowserStage(launch: BrowserLaunch, on_close: Callback<()>) -> impl IntoView {
    let status = RwSignal::new("Loading the browser Player.".to_string());
    let playing = RwSignal::new(false);
    let container = NodeRef::<leptos::html::Div>::new();
    let launch = StoredValue::new(launch);
    let handle = StoredValue::new_local(None::<Controller>);
    let started = StoredValue::new(false);

    Effect::new(move |_| {
        if started.get_value() {
            return;
        }
        let Some(el) = container.get() else { return };
        started.set_value(true);
        let launch = launch.get_value();
        #[cfg(not(feature = "ssr"))]
        {
            let status = status;
            let playing = playing;
            let handle = handle;
            listen(&el, status, playing);
            spawn_local(async move {
                match start_player(&el, &launch).await {
                    Ok(controller) => handle.set_value(Some(controller)),
                    Err(()) => status.set("The browser Player could not load.".to_string()),
                }
            });
        }
        #[cfg(feature = "ssr")]
        {
            let _ = (el, launch, status, playing, handle);
        }
    });

    let leave = move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            if let Some(controller) = handle.get_value() {
                controller.stop();
            }
        }
        on_close.run(());
    };

    view! {
        <div class="bp-stage" role="dialog" aria-modal="true" aria-label="Browser Player">
            <div class="bp-bar">
                <p role="status">{move || status.get()}</p>
                <button class="btn btn-secondary" on:click=leave>"Leave"</button>
            </div>
            <div class="bp-canvas" node_ref=container></div>
            {move || playing.get().then(|| view! {
                <p class="bp-hint">"WASD moves your character. Shift sprints, Space jumps, and Ctrl crouches. Hold the right mouse button to look around. A Space that does not load a character flies the camera instead: click to look, Space and Ctrl go up and down, and Esc releases the mouse."</p>
            })}
        </div>
    }
}

#[cfg(not(feature = "ssr"))]
#[derive(Clone)]
struct Controller(wasm_bindgen::JsValue);

#[cfg(feature = "ssr")]
#[derive(Clone)]
struct Controller;

#[cfg(not(feature = "ssr"))]
impl Controller {
    fn stop(&self) {
        use wasm_bindgen::JsCast;
        let Ok(stop) = js_sys::Reflect::get(&self.0, &wasm_bindgen::JsValue::from_str("stop"))
        else {
            return;
        };
        let Some(stop) = stop.dyn_ref::<js_sys::Function>() else {
            return;
        };
        let _ = stop.call0(&self.0);
    }
}

#[cfg(not(feature = "ssr"))]
fn listen(el: &web_sys::HtmlDivElement, status: RwSignal<String>, playing: RwSignal<bool>) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let closure =
        Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(move |event: wasm_bindgen::JsValue| {
            let Ok(detail) =
                js_sys::Reflect::get(&event, &wasm_bindgen::JsValue::from_str("detail"))
            else {
                return;
            };
            let message =
                js_sys::Reflect::get(&detail, &wasm_bindgen::JsValue::from_str("message"))
                    .ok()
                    .and_then(|v| v.as_string());
            let state = js_sys::Reflect::get(&detail, &wasm_bindgen::JsValue::from_str("state"))
                .ok()
                .and_then(|v| v.as_string());
            if let Some(message) = message {
                status.set(message);
            }
            playing.set(state.as_deref() == Some("in-world"));
        });
    let _ = el.add_event_listener_with_callback(
        "eustress-player-status",
        closure.as_ref().unchecked_ref(),
    );
    closure.forget();
}

#[cfg(not(feature = "ssr"))]
async fn start_player(
    el: &web_sys::HtmlDivElement,
    launch: &BrowserLaunch,
) -> Result<Controller, ()> {
    use wasm_bindgen::JsCast;
    let imported = js_sys::eval("import('/player/host.js')").map_err(|_| ())?;
    let promise: js_sys::Promise = imported.dyn_into().map_err(|_| ())?;
    let namespace = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|_| ())?;
    let start = js_sys::Reflect::get(&namespace, &wasm_bindgen::JsValue::from_str("start"))
        .map_err(|_| ())?;
    let start: js_sys::Function = start.dyn_into().map_err(|_| ())?;
    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("container"),
        el.as_ref(),
    )
    .map_err(|_| ())?;
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("simId"),
        &wasm_bindgen::JsValue::from_str(&launch.sim_id),
    )
    .map_err(|_| ())?;
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("apiBase"),
        &wasm_bindgen::JsValue::from_str(&launch.api_base),
    )
    .map_err(|_| ())?;
    if let Some(join) = &launch.join {
        js_sys::Reflect::set(
            &args,
            &wasm_bindgen::JsValue::from_str("join"),
            &wasm_bindgen::JsValue::from_str(join),
        )
        .map_err(|_| ())?;
    }
    let result = start
        .call1(&wasm_bindgen::JsValue::UNDEFINED, &args)
        .map_err(|_| ())?;
    let controller = match result.dyn_into::<js_sys::Promise>() {
        Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|_| ())?,
        Err(value) => value,
    };
    Ok(Controller(controller))
}
