//! # Commerce in Play
//!
//! Studio's side of `MarketplaceService`
//! ([`eustress_common::datamodel::CommerceState`]). The first time a script
//! uses it, this loads the simulation's catalog, the passes the signed-in
//! creator owns, and purchases still waiting for `ProcessReceipt`. From then
//! on it buys what scripts prompt for and fulfills what `ProcessReceipt`
//! granted.
//!
//! Studio makes test purchases only: nothing moves, and every request says
//! `Eustress-Mode: test`, so none reaches live data by leaving the header
//! out. A test purchase goes through without a dialog; the Output says what
//! was bought. Live purchases belong to the Player, which must ask the player
//! first.
//!
//! Commerce needs the Universe published (products belong to its listing,
//! whose id is `remote.experience_id` in `<Universe>/.eustress/sync.toml`)
//! and a signed-in creator. Without either, scripts see the service as
//! unavailable and every prompt closes unpurchased.
//!
//! Requests run on short-lived threads and report back through an inbox this
//! system drains, so a frame never waits on the network.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use parking_lot::Mutex;
use serde_json::{json, Value};

use eustress_common::datamodel::{
    CommerceProduct, CommerceState, CommerceStatus, DataModel, OutputLevel, ProductKind,
    PromptOutcome, PurchasePrompt, Receipt,
};

use super::PlayDataModel;

const SOURCE: &str = "MarketplaceService";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The session's commerce client. Reset on Stop, so each session starts idle.
#[derive(Resource, Default)]
pub struct PlayCommerce {
    api: Option<CommerceApi>,
    /// A script woke commerce and the load has been dealt with.
    started: bool,
    inbox: Arc<Mutex<Vec<Reply>>>,
    /// Receipts handed to scripts, by purchase id, until `ProcessReceipt`
    /// answers.
    handed: HashMap<String, Receipt>,
    /// Receipts `ProcessReceipt` did not grant. As in Roblox, they are
    /// offered again after the player's next purchase (and next session).
    declined: Vec<Receipt>,
}

#[derive(Clone)]
struct CommerceApi {
    base: String,
    token: String,
    sim_id: String,
    space: Option<String>,
}

/// What a request thread reports back.
enum Reply {
    Loaded {
        catalog: Vec<CommerceProduct>,
        pending: Vec<Value>,
        owned: Vec<u64>,
    },
    LoadFailed(String),
    Bought {
        prompt: PurchasePrompt,
        product: CommerceProduct,
        purchase: Value,
    },
    NotBought {
        prompt: PurchasePrompt,
        reason: String,
    },
    Fulfilled,
    FulfillFailed {
        purchase_id: String,
        reason: String,
    },
}

/// OnEnter(Editing): forget the session. Requests still out report to the
/// old inbox, which nothing reads.
pub fn reset_commerce(mut commerce: ResMut<PlayCommerce>) {
    *commerce = PlayCommerce::default();
}

/// Each Play frame, after the scripts: start commerce when a script asked
/// for it, send what scripts queued, and hand back what the API answered.
pub fn drive_commerce(
    dm: Option<Res<PlayDataModel>>,
    mut commerce: ResMut<PlayCommerce>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    // No script has used MarketplaceService: nothing is queued or out.
    if g.commerce.status == CommerceStatus::Idle {
        return;
    }
    let commerce = &mut *commerce;

    if g.commerce.status == CommerceStatus::Loading && !commerce.started {
        commerce.started = true;
        match session_api(space_root.as_deref(), auth.as_deref()) {
            Ok(api) => {
                let job = api.clone();
                spawn(&commerce.inbox, move || load(&job));
                commerce.api = Some(api);
            }
            Err(why) => {
                g.print(
                    OutputLevel::Warn,
                    SOURCE,
                    format!("purchases are unavailable in this session: {why}"),
                );
                g.commerce.status = CommerceStatus::Unavailable(why);
            }
        }
    }

    send_prompts(&mut g, commerce);

    for decision in std::mem::take(&mut g.commerce.decisions) {
        let receipt = commerce.handed.remove(&decision.purchase_id);
        if decision.granted {
            if let Some(api) = commerce.api.clone() {
                let purchase_id = decision.purchase_id;
                spawn(&commerce.inbox, move || fulfill(&api, purchase_id));
            }
        } else if let Some(receipt) = receipt {
            commerce.declined.push(receipt);
        }
    }

    let replies = std::mem::take(&mut *commerce.inbox.lock());
    for reply in replies {
        take_reply(&mut g, commerce, reply);
    }
}

/// Buy what scripts prompted for, once the catalog is here. A prompt that
/// cannot be bought closes unpurchased, with the reason in the Output.
fn send_prompts(g: &mut DataModel, commerce: &mut PlayCommerce) {
    if g.commerce.prompts.is_empty()
        || matches!(
            g.commerce.status,
            CommerceStatus::Idle | CommerceStatus::Loading
        )
    {
        return;
    }
    let local = local_user_id(g);
    for prompt in std::mem::take(&mut g.commerce.prompts) {
        let refusal = match (&commerce.api, &g.commerce.status) {
            (Some(_), CommerceStatus::Ready) => refusal_for(&g.commerce, &prompt, local),
            (_, CommerceStatus::Unavailable(why)) => {
                Some(format!("purchases are unavailable: {why}"))
            }
            _ => Some("purchases are unavailable".to_string()),
        };
        if let Some(reason) = refusal {
            g.print(
                OutputLevel::Warn,
                SOURCE,
                format!("product {} was not bought: {reason}", prompt.product),
            );
            close_prompt(&mut g.commerce, &prompt, false);
            continue;
        }
        let (Some(api), Some(product)) = (
            commerce.api.clone(),
            g.commerce.product(prompt.product).cloned(),
        ) else {
            continue;
        };
        g.print(
            OutputLevel::Info,
            SOURCE,
            format!(
                "Test purchase: {} (#{}) for {} Tickets. Studio test mode: no Tickets move.",
                product.name, product.number, product.price
            ),
        );
        spawn(&commerce.inbox, move || buy(&api, prompt, product));
    }
}

/// Why `prompt` cannot be bought, or `None` when it can.
fn refusal_for(
    state: &CommerceState,
    prompt: &PurchasePrompt,
    local: Option<f64>,
) -> Option<String> {
    if Some(prompt.user_id) != local {
        return Some("in Studio only the local player can buy".to_string());
    }
    let Some(product) = state.product(prompt.product) else {
        return Some(
            "this simulation has no such product (`eustress commerce products list`)".to_string(),
        );
    };
    match (prompt.expects, product.kind) {
        (Some(ProductKind::Pass), ProductKind::Consumable) => {
            return Some("it is a product, not a pass: use PromptProductPurchase".to_string())
        }
        (Some(ProductKind::Consumable), ProductKind::Pass) => {
            return Some("it is a pass: use PromptGamePassPurchase".to_string())
        }
        _ => {}
    }
    if product.kind == ProductKind::Pass && state.owned_passes.contains(&product.number) {
        return Some("the pass is already owned".to_string());
    }
    None
}

fn close_prompt(state: &mut CommerceState, prompt: &PurchasePrompt, purchased: bool) {
    state.outcomes.push(PromptOutcome {
        user_id: prompt.user_id,
        product: prompt.product,
        expects: prompt.expects,
        purchased,
    });
}

fn take_reply(g: &mut DataModel, commerce: &mut PlayCommerce, reply: Reply) {
    match reply {
        Reply::Loaded { catalog, pending, owned } => {
            let count = catalog.len();
            g.commerce.catalog = catalog;
            g.commerce.owned_passes = owned.into_iter().collect();
            g.commerce.status = CommerceStatus::Ready;
            let user_id = local_user_id(g).unwrap_or(0.0);
            let receipts: Vec<Receipt> = pending.iter().filter_map(|r| receipt_from_pending(r, user_id)).collect();
            let waiting = receipts.len();
            for receipt in receipts {
                hand_to_scripts(&mut g.commerce, commerce, receipt);
            }
            let mut line = format!("{count} product(s) loaded (test mode)");
            if waiting > 0 {
                line.push_str(&format!("; {waiting} earlier purchase(s) waiting for ProcessReceipt"));
            }
            g.print(OutputLevel::Info, SOURCE, line);
        }
        Reply::LoadFailed(why) => {
            g.print(OutputLevel::Warn, SOURCE, format!("could not load the catalog: {why}"));
            g.commerce.status = CommerceStatus::Unavailable(why);
        }
        Reply::Bought { prompt, product, purchase } => {
            let purchase_id = purchase.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            match product.kind {
                // Owning a pass is the grant, so it is fulfilled at once.
                ProductKind::Pass => {
                    g.commerce.owned_passes.insert(product.number);
                    if let Some(api) = commerce.api.clone() {
                        spawn(&commerce.inbox, move || fulfill(&api, purchase_id));
                    }
                }
                ProductKind::Consumable => {
                    let receipt = Receipt {
                        purchase_id,
                        user_id: prompt.user_id,
                        product: product.number,
                        price: product.price,
                        sim_id: purchase.get("sim_id").and_then(Value::as_str).unwrap_or_default().to_string(),
                        space: purchase.get("space").and_then(Value::as_str).map(str::to_string),
                    };
                    hand_to_scripts(&mut g.commerce, commerce, receipt);
                }
            }
            for receipt in std::mem::take(&mut commerce.declined) {
                hand_to_scripts(&mut g.commerce, commerce, receipt);
            }
            close_prompt(&mut g.commerce, &prompt, true);
        }
        Reply::NotBought { prompt, reason } => {
            g.print(OutputLevel::Warn, SOURCE, format!("product {} was not bought: {reason}", prompt.product));
            close_prompt(&mut g.commerce, &prompt, false);
        }
        Reply::Fulfilled => {}
        Reply::FulfillFailed { purchase_id, reason } => g.print(
            OutputLevel::Warn,
            SOURCE,
            format!("could not record {purchase_id} as granted ({reason}); ProcessReceipt will see it again next session"),
        ),
    }
}

fn hand_to_scripts(state: &mut CommerceState, commerce: &mut PlayCommerce, receipt: Receipt) {
    commerce
        .handed
        .insert(receipt.purchase_id.clone(), receipt.clone());
    state.receipts.push_back(receipt);
}

/// The local player's `UserId`: in Studio, the signed-in creator.
fn local_user_id(g: &DataModel) -> Option<f64> {
    g.local_player
        .and_then(|p| g.get_prop(p, "UserId"))
        .and_then(|v| v.as_number())
}

// ─────────────────────────────────────────────────────────────────────────────
// The Commerce API
// ─────────────────────────────────────────────────────────────────────────────

/// Whether this session can sell, and to whom it talks.
fn session_api(
    space_root: Option<&crate::space::SpaceRoot>,
    auth: Option<&crate::auth::AuthState>,
) -> Result<CommerceApi, String> {
    let token = auth
        .and_then(|a| a.token.clone())
        .filter(|t| !t.trim().is_empty())
        .ok_or("sign in to Eustress to test purchases")?;
    let space_root = space_root.map(|s| s.0.clone()).ok_or("no Space is open")?;
    let universe =
        crate::space::universe_root_for_path(&space_root).unwrap_or_else(|| space_root.clone());
    let sim_id = published_id(&universe)
        .ok_or("publish this Universe first: products belong to a published simulation")?;
    Ok(CommerceApi {
        base: api_base(),
        token,
        sim_id,
        space: space_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned()),
    })
}

/// The Universe's listing id, if it has been published.
fn published_id(universe: &Path) -> Option<String> {
    let path = universe.join(".eustress").join("sync.toml");
    let sync = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path).ok()?;
    let id = sync.remote.experience_id?.trim().to_string();
    let valid =
        (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    valid.then_some(id)
}

/// The API, or `EUSTRESS_API_URL` (a local `wrangler dev`, as the CLI takes).
fn api_base() -> String {
    std::env::var("EUSTRESS_API_URL")
        .ok()
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "https://api.eustress.dev".to_string())
}

impl CommerceApi {
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let request = ureq::request(method, &format!("{}{}", self.base, path))
            .set("Authorization", &format!("Bearer {}", self.token))
            // Studio only tests; say so, so no request can fall into live.
            .set("Eustress-Mode", "test")
            .timeout(REQUEST_TIMEOUT);
        let result = match body {
            Some(body) => request.send_json(body),
            None => request.call(),
        };
        match result {
            Ok(response) => response
                .into_json::<Value>()
                .map_err(|e| format!("unreadable reply: {e}")),
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_json::<Value>().unwrap_or(Value::Null);
                let message = body
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed");
                let code = body.get("code").and_then(Value::as_str).unwrap_or("");
                Err(format!("{message} [{status} {code}]"))
            }
            Err(e) => Err(format!("could not reach {}: {e}", self.base)),
        }
    }
}

fn spawn(inbox: &Arc<Mutex<Vec<Reply>>>, job: impl FnOnce() -> Reply + Send + 'static) {
    let inbox = inbox.clone();
    let started = std::thread::Builder::new()
        .name("eustress-commerce".into())
        .spawn(move || {
            let reply = job();
            inbox.lock().push(reply);
        });
    if let Err(e) = started {
        warn!("commerce: could not start a request thread: {e}");
    }
}

fn load(api: &CommerceApi) -> Reply {
    let catalog = match api.call(
        "GET",
        &format!("/api/commerce/catalog/{}", api.sim_id),
        None,
    ) {
        Ok(list) => list,
        Err(why) => return Reply::LoadFailed(why),
    };
    let catalog: Vec<CommerceProduct> = items(&catalog)
        .iter()
        .filter_map(product_from_json)
        .collect();
    // A failure below costs the earlier receipts or passes for this session,
    // never the catalog.
    let pending = api
        .call(
            "GET",
            &format!("/api/commerce/me/pending?sim_id={}", api.sim_id),
            None,
        )
        .map(|list| items(&list))
        .unwrap_or_default();
    let owned = api
        .call(
            "GET",
            &format!("/api/commerce/me/entitlements?sim_id={}", api.sim_id),
            None,
        )
        .map(|list| {
            items(&list)
                .iter()
                .filter_map(|e| e.get("number").and_then(Value::as_u64))
                .collect()
        })
        .unwrap_or_default();
    Reply::Loaded {
        catalog,
        pending,
        owned,
    }
}

fn buy(api: &CommerceApi, prompt: PurchasePrompt, product: CommerceProduct) -> Reply {
    let body = json!({
        "sim_id": api.sim_id,
        "product": product.number,
        "expected_price": product.price,
        "idempotency_key": format!("studio_{}", uuid::Uuid::new_v4().simple()),
        "space": api.space,
    });
    match api.call("POST", "/api/commerce/purchases", Some(&body)) {
        Ok(reply) => Reply::Bought {
            prompt,
            product,
            purchase: reply.get("purchase").cloned().unwrap_or(Value::Null),
        },
        Err(reason) => Reply::NotBought { prompt, reason },
    }
}

fn fulfill(api: &CommerceApi, purchase_id: String) -> Reply {
    let path = format!("/api/commerce/purchases/{purchase_id}/fulfill");
    match api.call("POST", &path, Some(&json!({ "sim_id": api.sim_id }))) {
        Ok(_) => Reply::Fulfilled,
        Err(reason) => Reply::FulfillFailed {
            purchase_id,
            reason,
        },
    }
}

fn items(list: &Value) -> Vec<Value> {
    list.get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn product_from_json(p: &Value) -> Option<CommerceProduct> {
    let text = |key: &str| {
        p.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(CommerceProduct {
        id: p.get("id")?.as_str()?.to_string(),
        number: p.get("number")?.as_u64()?,
        name: text("name"),
        description: text("description"),
        price: p.get("price")?.as_u64()?,
        kind: ProductKind::from_api(
            p.get("type")
                .and_then(Value::as_str)
                .unwrap_or("consumable"),
        ),
        icon: p.get("icon").and_then(Value::as_str).map(str::to_string),
        active: p.get("active").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// A receipt from `/me/pending`: a purchase an earlier session never granted.
fn receipt_from_pending(r: &Value, user_id: f64) -> Option<Receipt> {
    Some(Receipt {
        purchase_id: r.get("purchase_id")?.as_str()?.to_string(),
        user_id,
        product: r.get("product")?.get("number")?.as_u64()?,
        price: r.get("amount").and_then(Value::as_u64).unwrap_or(0),
        sim_id: r
            .get("sim_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        space: r.get("space").and_then(Value::as_str).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(number: u64, kind: ProductKind) -> CommerceProduct {
        CommerceProduct {
            id: format!("prod_{number}"),
            number,
            name: format!("P{number}"),
            description: String::new(),
            price: 10,
            kind,
            icon: None,
            active: true,
        }
    }

    fn prompt(product: u64, expects: Option<ProductKind>) -> PurchasePrompt {
        PurchasePrompt {
            user_id: 1.0,
            product,
            expects,
        }
    }

    #[test]
    fn prompts_are_refused_for_the_wrong_player_product_or_kind() {
        let mut state = CommerceState::default();
        state.catalog = vec![
            product(1, ProductKind::Consumable),
            product(2, ProductKind::Pass),
        ];
        assert_eq!(
            refusal_for(&state, &prompt(1, Some(ProductKind::Consumable)), Some(1.0)),
            None
        );
        assert!(
            refusal_for(&state, &prompt(1, None), Some(2.0)).is_some(),
            "another player"
        );
        assert!(
            refusal_for(&state, &prompt(9, None), Some(1.0)).is_some(),
            "no such product"
        );
        assert!(refusal_for(&state, &prompt(1, Some(ProductKind::Pass)), Some(1.0)).is_some());
        assert!(
            refusal_for(&state, &prompt(2, Some(ProductKind::Consumable)), Some(1.0)).is_some()
        );
        assert_eq!(
            refusal_for(&state, &prompt(2, Some(ProductKind::Pass)), Some(1.0)),
            None
        );
        state.owned_passes.insert(2);
        assert!(
            refusal_for(&state, &prompt(2, None), Some(1.0)).is_some(),
            "already owned"
        );
    }

    #[test]
    fn catalog_and_receipt_json_parse() {
        let p = product_from_json(&json!({
            "id": "prod_x", "number": 3, "name": "Coins", "price": 50, "type": "pass", "active": true,
        }))
        .unwrap();
        assert_eq!(
            (p.number, p.price, p.kind, p.active),
            (3, 50, ProductKind::Pass, true)
        );
        assert!(product_from_json(&json!({ "id": "prod_x" })).is_none());
        let r = receipt_from_pending(
            &json!({ "purchase_id": "pur_x", "product": { "number": 3 }, "amount": 50, "sim_id": "abc", "space": "Lobby" }),
            7.0,
        )
        .unwrap();
        assert_eq!(
            (r.purchase_id.as_str(), r.product, r.price, r.user_id),
            ("pur_x", 3, 50, 7.0)
        );
        assert_eq!(r.space.as_deref(), Some("Lobby"));
    }
}
