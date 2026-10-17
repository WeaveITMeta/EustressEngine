//! # The session's transport in a browser
//!
//! The session talks to a transport only through the two channels of its
//! link (eustress-networking session.rs `link_pair`), as it does on the
//! desktop over wtransport. Here the transport is `js/net.js`'s WebTransport:
//! - [`begin`] starts the join and asks net.js to open the session, pinned to
//!   the host's certificate. Once it is up the session hears
//!   [`LinkEvent::Opened`], with datagrams (net.js requires them), and sends
//!   its `Hello` on its next frame; the host closes a session that has sent
//!   nothing in 10 s.
//! - Stream bytes arrive in chunks of any size and are put back together
//!   into frame bodies ([`FrameAssembler`]); datagrams pass through.
//! - [`pump`] sends the session's commands at the end of each frame: frames
//!   on the stream, datagrams as datagrams (one over the session's maximum
//!   is dropped and costs only itself), and Close or Shutdown closes the
//!   session once the stream's queued frames are out, a Goodbye included
//!   (net.js waits up to the desktop's close grace, 250 ms).
//!
//! As on the desktop (eustress-networking native.rs), a session that does
//! not open is [`LinkEvent::Failed`], with net.js's fixed code, and every
//! end after it opened is [`LinkEvent::Closed`], an unreadable stream
//! included. Neither carries the URL, whose path holds the join key.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bevy::prelude::*;
use eustress_networking::session::{begin_join, link_pair, LinkCommand, LinkEnds, LinkEvent, HOST_CONN};
use eustress_networking::wire::FrameAssembler;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::join::JoinPlan;
use crate::page::PageState;
use crate::states;

#[wasm_bindgen(module = "/js/net.js")]
extern "C" {
    /// net.js `openLink(url, pin, handlers)`: the open session, or a
    /// rejection carrying one of [`NET_FAILURES`].
    #[wasm_bindgen(js_name = openLink, catch)]
    async fn open_link(url: &str, pin: js_sys::Uint8Array, handlers: &JsValue) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    /// The session net.js opened: `{ sendFrame, sendDatagram, maxDatagramSize, close }`.
    type OpenSession;
    #[wasm_bindgen(method, js_name = sendFrame)]
    fn send_frame(this: &OpenSession, bytes: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = sendDatagram)]
    fn send_datagram(this: &OpenSession, bytes: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(method, getter = maxDatagramSize)]
    fn max_datagram_size(this: &OpenSession) -> f64;
    #[wasm_bindgen(method)]
    fn close(this: &OpenSession);
}

/// net.js's `FAILURES`, the only codes a failure here reports.
const NET_FAILURES: [&str; 8] = [
    "no-webtransport",
    "bad-arguments",
    "connect-failed",
    "connect-timeout",
    "stream-failed",
    "no-datagrams",
    "datagram-too-large",
    "closed",
];

/// The join to begin, until [`begin`] takes it.
#[derive(Resource)]
struct PendingJoin {
    plan: JoinPlan,
    name: String,
}

/// What net.js's handlers, the opening task and the app share.
#[derive(Default)]
struct Bridge {
    session: RefCell<Option<OpenSession>>,
    ended: Cell<bool>,
}

impl Bridge {
    /// Close the session. What Close, Shutdown, an unreadable stream and the
    /// app's end all come to.
    fn end(&self) {
        self.ended.set(true);
        if let Some(session) = self.session.borrow_mut().take() {
            session.close();
        }
    }
}

/// The transport's end of the session's link, kept by the app.
struct WebLink {
    ends: LinkEnds,
    bridge: Rc<Bridge>,
    /// Takes what a send's promise rejects with: a send that fails (the
    /// session closing, a datagram too large) costs only itself.
    ignore: Closure<dyn FnMut(JsValue)>,
}

impl Drop for WebLink {
    /// The app is ending: the host hears a clean close, not a timeout.
    fn drop(&mut self) {
        self.bridge.end();
    }
}

/// Join with `plan` as `name` once the app starts.
pub(crate) fn add(app: &mut App, plan: JoinPlan, name: String) {
    app.insert_resource(PendingJoin { plan, name })
        .add_systems(Startup, begin)
        // After the session's systems, so a frame's commands leave in it.
        .add_systems(Last, pump);
}

/// Start the join, and open the session net.js carries it on.
fn begin(world: &mut World) {
    let Some(PendingJoin { plan, name }) = world.remove_resource::<PendingJoin>() else {
        return;
    };
    let JoinPlan {
        url,
        remote,
        pin,
        identity,
    } = plan;
    let (link, ends) = link_pair();
    begin_join(&mut world.commands(), link, &name, identity);
    world.flush();

    let bridge = Rc::new(Bridge::default());
    let send = {
        let events = ends.events.clone();
        move |event: LinkEvent| {
            let _ = events.send(event);
        }
    };
    let callbacks = handlers(send.clone(), bridge.clone());
    let opening = bridge.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let pin = js_sys::Uint8Array::from(&pin[..]);
        match open_link(&url, pin, &callbacks).await {
            Ok(opened) => {
                let session: OpenSession = opened.unchecked_into();
                if opening.ended.get() {
                    // The page stopped the session while it opened.
                    session.close();
                    return;
                }
                *opening.session.borrow_mut() = Some(session);
                send(LinkEvent::Opened {
                    conn: HOST_CONN,
                    remote,
                    datagrams: true,
                });
            }
            Err(failure) => {
                let code = js_sys::Reflect::get(&failure, &JsValue::from_str("code"))
                    .ok()
                    .and_then(|code| code.as_string())
                    .filter(|code| NET_FAILURES.contains(&code.as_str()))
                    .unwrap_or_else(|| "connect-failed".to_string());
                send(LinkEvent::Failed {
                    reason: format!("could not reach the host ({code})"),
                });
            }
        }
    });

    world.non_send_resource_mut::<PageState>().report(states::JOINING);
    world.insert_non_send_resource(WebLink {
        ends,
        bridge,
        ignore: Closure::new(|_: JsValue| {}),
    });
}

/// The handlers net.js calls for as long as the session lives.
fn handlers(send: impl Fn(LinkEvent) + Clone + 'static, bridge: Rc<Bridge>) -> JsValue {
    let on_bytes = {
        let send = send.clone();
        let bridge = bridge.clone();
        let mut assembler = FrameAssembler::default();
        Closure::<dyn FnMut(js_sys::Uint8Array)>::new(move |chunk: js_sys::Uint8Array| {
            assembler.push(&chunk.to_vec());
            loop {
                match assembler.next_body() {
                    Ok(Some(body)) => send(LinkEvent::Frame { conn: HOST_CONN, body }),
                    Ok(None) => break,
                    // Once open, every end is Closed, as on the desktop
                    // (native.rs ends pump_connection on a frame it cannot
                    // read, then reports Closed).
                    Err(_) => {
                        send(LinkEvent::Closed {
                            conn: HOST_CONN,
                            reason: "the host sent an unreadable stream".into(),
                        });
                        bridge.end();
                        break;
                    }
                }
            }
        })
    };
    let on_datagram = {
        let send = send.clone();
        Closure::<dyn FnMut(js_sys::Uint8Array)>::new(move |datagram: js_sys::Uint8Array| {
            send(LinkEvent::Datagram {
                conn: HOST_CONN,
                bytes: datagram.to_vec(),
            })
        })
    };
    let on_closed = Closure::<dyn FnMut(JsValue)>::new(move |_code: JsValue| {
        bridge.ended.set(true);
        bridge.session.borrow_mut().take();
        send(LinkEvent::Closed {
            conn: HOST_CONN,
            reason: "closed".into(),
        });
    });
    let object = js_sys::Object::new();
    for (name, handler) in [
        ("onStreamBytes", on_bytes.into_js_value()),
        ("onDatagram", on_datagram.into_js_value()),
        ("onClosed", on_closed.into_js_value()),
    ] {
        let _ = js_sys::Reflect::set(&object, &JsValue::from_str(name), &handler);
    }
    object.into()
}

/// Send what the session asked for this frame.
fn pump(link: Option<NonSend<WebLink>>) {
    let Some(link) = link else { return };
    loop {
        let command = match link.ends.commands.try_recv() {
            Ok(command) => command,
            Err(empty_or_gone) => {
                // The session's NetLink is gone. Its drop sends Shutdown
                // first, so this only makes sure.
                if empty_or_gone.is_disconnected() {
                    link.bridge.end();
                }
                break;
            }
        };
        match command {
            LinkCommand::Frame { frame, .. } => {
                if let Some(session) = link.bridge.session.borrow().as_ref() {
                    let _ = session
                        .send_frame(&js_sys::Uint8Array::from(&frame[..]))
                        .catch(&link.ignore);
                }
            }
            LinkCommand::Datagram { bytes, .. } => {
                if let Some(session) = link.bridge.session.borrow().as_ref() {
                    if bytes.len() as f64 <= session.max_datagram_size() {
                        let _ = session
                            .send_datagram(&js_sys::Uint8Array::from(&bytes[..]))
                            .catch(&link.ignore);
                    }
                }
            }
            LinkCommand::Close { .. } | LinkCommand::Shutdown => link.bridge.end(),
        }
    }
}
