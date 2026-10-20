//! Download a published world with the browser's `fetch` and hand it to the
//! same open path a host's world uses (`world.rs`).
//!
//! No token is required for a public Gallery world. A signed-in page's token
//! (`auth_token` in localStorage, the same key the site sends) is used only
//! after an unauthenticated request is refused, so a stale token cannot
//! block a public world. The token is never logged.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use bevy::prelude::*;
use eustress_networking::session::WorldDownloaded;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::page::PageState;
use crate::solo::{chunk_url, manifest_url, world_from_chunks};
use crate::states;

/// The Gallery id to download, until [`begin`] takes it.
#[derive(Resource)]
struct PendingSolo(String);

struct PendingFetch {
    slot: Rc<RefCell<Option<Result<Arc<eustress_networking::session::DownloadedWorld>, String>>>>,
}

/// Download `sim_id` from `api` once the app starts, then open it on a later frame.
pub(crate) fn add(app: &mut App, api: String, sim_id: String) {
    app.insert_resource(SoloApi(api))
        .insert_resource(PendingSolo(sim_id))
        .add_systems(Startup, begin)
        .add_systems(Update, poll);
}

fn begin(world: &mut World) {
    let Some(PendingSolo(sim_id)) = world.remove_resource::<PendingSolo>() else {
        return;
    };
    let api = world
        .get_resource::<SoloApi>()
        .map(|api| api.0.clone())
        .unwrap_or_default();
    let slot = Rc::new(RefCell::new(None));
    let deliver = slot.clone();
    world.insert_non_send(PendingFetch { slot });
    world
        .non_send_mut::<PageState>()
        .report(states::LOADING_WORLD);
    wasm_bindgen_futures::spawn_local(async move {
        let result = download(&api, &sim_id).await.map(Arc::new);
        *deliver.borrow_mut() = Some(result);
    });
}

/// Where the world is downloaded from.
#[derive(Resource, Clone)]
struct SoloApi(String);

fn poll(
    pending: NonSend<PendingFetch>,
    mut worlds: MessageWriter<WorldDownloaded>,
    mut page: NonSendMut<PageState>,
) {
    let Some(result) = pending.slot.borrow_mut().take() else {
        return;
    };
    match result {
        Ok(world) => {
            let files: usize = world
                .spaces
                .iter()
                .map(|(_, records)| records.len())
                .sum::<usize>()
                + world.assets.len();
            info!("space: downloaded {} ({} files)", world.universe, files);
            worlds.write(WorldDownloaded(world));
        }
        Err(reason) => {
            error!("space: {reason}");
            page.report(states::WORLD_FAILED);
        }
    }
}

async fn download(
    api: &str,
    sim_id: &str,
) -> Result<eustress_networking::session::DownloadedWorld, String> {
    let manifest_url = manifest_url(api, sim_id)
        .map_err(|_| "the page did not name a Gallery world".to_string())?;
    let token = stored_token();
    let (status, bytes) = get_public_then_signed(&manifest_url, token.as_deref()).await?;
    if status == 409 {
        return Err("this world is not in the current publish format".into());
    }
    if status != 200 {
        return Err(format!("the world manifest was not available ({status})"));
    }
    let manifest: eustress_echk::WorldManifest = serde_json::from_slice(&bytes)
        .map_err(|_| "the world manifest could not be read".to_string())?;
    manifest
        .validate()
        .map_err(|e| format!("the world manifest was refused: {e}"))?;
    let mut chunks = HashMap::new();
    for hash in manifest.hashes() {
        let url = chunk_url(api, sim_id, &hash)
            .map_err(|_| format!("chunk {hash} is not a content hash"))?;
        let (status, bytes) = get_public_then_signed(&url, token.as_deref()).await?;
        if status != 200 {
            return Err(format!("chunk {hash} was not available ({status})"));
        }
        chunks.insert(hash, bytes);
    }
    world_from_chunks(&bytes, &chunks)
}

/// The site's sign-in token, when this browser has one. Never logged.
fn stored_token() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let token = storage.get_item("auth_token").ok()??;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// A public world answers without a token. Only a refusal is retried with
/// the signed-in token, and only once.
async fn get_public_then_signed(url: &str, token: Option<&str>) -> Result<(u16, Vec<u8>), String> {
    let (status, bytes) = get(url, None).await?;
    if (status == 401 || status == 403) && token.is_some() {
        return get(url, token).await;
    }
    Ok((status, bytes))
}

async fn get(url: &str, token: Option<&str>) -> Result<(u16, Vec<u8>), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let mut init = web_sys::RequestInit::new();
    init.set_method("GET");
    init.set_redirect(web_sys::RequestRedirect::Error);
    if let Some(token) = token {
        let headers = web_sys::Headers::new().map_err(|_| "could not set headers".to_string())?;
        headers
            .set("Authorization", &format!("Bearer {token}"))
            .map_err(|_| "could not set headers".to_string())?;
        init.set_headers(&headers);
    }
    let request = web_sys::Request::new_with_str_and_init(url, &init)
        .map_err(|_| "could not start the download".to_string())?;
    let value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| "the download did not complete".to_string())?;
    let response: web_sys::Response = value
        .dyn_into()
        .map_err(|_| "the download did not complete".to_string())?;
    let status = response.status();
    if status != 200 {
        return Ok((status, Vec::new()));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|_| "could not read the download".to_string())?,
    )
    .await
    .map_err(|_| "could not read the download".to_string())?;
    Ok((status, js_sys::Uint8Array::new(&buffer).to_vec()))
}
