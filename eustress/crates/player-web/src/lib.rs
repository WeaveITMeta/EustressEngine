//! # The browser Player
//!
//! The Player for a web page: the replica of a joined world the desktop
//! Player shows, drawn into a canvas in the page and joined over the
//! browser's WebTransport. It builds for `wasm32-unknown-unknown` once per
//! graphics API, `--features webgpu` for `/player/webgpu/` and
//! `--features webgl2` for `/player/webgl2/`; `/player/host.js` picks one and
//! calls its `start_player` export.
//!
//! - [`join`]: the join link, read as the desktop Player reads its own.
//! - [`states`]: what the page is told, by fixed state names only.
//! - [`solo`]: a published world played with no host, from its Gallery id.
//! - Browser builds only: `entry` (the export host.js calls), `app` (the
//!   Bevy app), `link` (the session's transport over `js/net.js`), `fetch`
//!   (downloading a published world), `explore` (looking around with no
//!   avatar), `world` (opening the world that arrives, from its records)
//!   and `page` (the page's state and stop switch).
//!
//! Nothing here logs, keeps or reports a join link, its key, its ticket or
//! its pin. A page runs one session; joining again reloads the page, so
//! nothing of the last session's app is left behind.

pub mod join;
/// A published world played alone: the Gallery id, the API, and assembling
/// its chunks. The browser download is [`fetch`], compiled for wasm32 only.
pub mod solo;
pub mod states;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod entry;
#[cfg(target_arch = "wasm32")]
mod explore;
#[cfg(target_arch = "wasm32")]
mod fetch;
#[cfg(target_arch = "wasm32")]
mod link;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
mod world;
