//! # Commerce in Play
//!
//! Studio's side of `MarketplaceService`
//! ([`eustress_common::datamodel::CommerceState`]). The first time a script
//! uses it, this loads the simulation's catalog, the passes the signed-in
//! creator owns, and purchases still waiting for `ProcessReceipt`. From then
//! on it sells what scripts prompt for and fulfills what `ProcessReceipt`
//! granted.
//!
//! ## Studio's own player: test purchases
//!
//! The local player is the signed-in creator, and its purchases are test
//! purchases: nothing moves, and every such request says `Eustress-Mode:
//! test`, so none reaches live data by leaving the header out. Studio asks
//! first, in a dialog (`ui/purchase_prompt.rs`, through [`PurchaseConfirm`]);
//! a headless engine has no dialog and buys at once.
//!
//! ## Players who joined a multiplayer host: real purchases
//!
//! A player's purchase is only as safe as the host that grants it, so only a
//! host signed in as the listing's creator sells to joined players. It proves
//! who it is to them with its own identity ticket
//! ([`announce_host_identity`]); a Player buys from no other host.
//!
//! - When a signed-in player joins, the host reads what that player owns in
//!   this simulation and what it bought that no session granted yet
//!   (`GET /api/commerce/players/{account}`, creator only, live). Its passes
//!   answer `UserOwnsGamePassAsync`; its ungranted purchases go to
//!   `ProcessReceipt`.
//! - A prompt for a joined player goes to its Player (`PromptPurchase`),
//!   which asks the player and buys with the player's own account, live.
//! - The Player then says whether it bought, and sends the new purchase id
//!   (`NetNotice::Receipts`). The host takes a purchase only when the Worker
//!   shows the verified player bought it
//!   (`GET /api/commerce/receipts/{sim}/{id}`), and closes the prompt as
//!   purchased only once the Worker shows it, never on the player's word.
//! - A consumable `ProcessReceipt` granted is marked fulfilled, live.
//!
//! Commerce needs the Universe published (products belong to its listing,
//! whose id is `remote.experience_id` in `<Universe>/.eustress/sync.toml`)
//! and a signed-in creator. Without either, scripts see the service as
//! unavailable and every prompt closes unpurchased.
//!
//! Requests run on short-lived threads and report back through an inbox the
//! frame system drains, so a frame never waits on the network.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use parking_lot::Mutex;
use serde_json::{json, Value};

use eustress_common::datamodel::{
    user_key, CommerceProduct, CommerceState, CommerceStatus, DataModel, OutputLevel, ProductKind,
    PromptOutcome, PurchasePrompt, Receipt,
};
use eustress_networking::join_link::hex32;
use eustress_networking::wire::PeerId;
use eustress_networking::{HostSession, NetNotice, PromptPurchase};

use super::remote_players::RemotePlayers;
use super::PlayDataModel;

const SOURCE: &str = "MarketplaceService";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a joined player's "I bought it" waits for the Worker to show the
/// purchase before the prompt closes unpurchased.
const CLAIM_WAIT: Duration = Duration::from_secs(30);
/// How long a joined player's new purchase can confirm a claim.
const ACCEPTED_KEPT: Duration = Duration::from_secs(300);

/// The session's commerce client. Reset on Stop, so each session starts idle.
#[derive(Resource, Default)]
pub struct PlayCommerce {
    api: Option<CommerceApi>,
    /// A script woke commerce and the load has been dealt with.
    started: bool,
    /// The signed-in account is the listing's creator, so this host may sell
    /// to players who joined it.
    creator: bool,
    inbox: Arc<Mutex<Vec<Reply>>>,
    /// Receipts handed to scripts, by purchase id, until `ProcessReceipt`
    /// answers.
    handed: HashMap<String, Receipt>,
    /// Receipts `ProcessReceipt` did not grant. As in Roblox, they are
    /// offered again after the player's next purchase (and next session).
    declined: Vec<Receipt>,
    /// Prompts offered to joined players, by the id sent with them.
    offered: HashMap<u32, Offer>,
    next_offer: u32,
    /// Joined players whose state was read this session.
    read: HashSet<PeerId>,
    /// Joined players to read again: they said they bought something.
    reread: HashSet<PeerId>,
    /// Purchase ids joined players sent, waiting for their identity to settle.
    waiting_receipts: Vec<(PeerId, Vec<String>)>,
    /// Joined players' purchases already checked or handed this session.
    seen: HashSet<String>,
    /// Joined players' purchases handed to scripts: whether each is live.
    remote_live: HashMap<String, bool>,
    /// Joined players' new purchases, which confirm what they said they bought.
    accepted: Vec<Accepted>,
    /// Said once per session that this host cannot sell to joined players.
    warned_not_creator: bool,
}

#[derive(Clone)]
struct CommerceApi {
    base: String,
    token: String,
    sim_id: String,
    space: Option<String>,
}

/// Which data a request acts on.
#[derive(Clone, Copy)]
enum Mode {
    Test,
    Live,
}

/// A prompt offered to a joined player.
struct Offer {
    prompt: PurchasePrompt,
    peer: PeerId,
    kind: ProductKind,
    sent: Instant,
    /// When the player said it bought. The prompt closes purchased only once
    /// the Worker shows the purchase.
    claimed: Option<Instant>,
}

/// A joined player's purchase that arrived after it said it bought something.
struct Accepted {
    peer: PeerId,
    product: u64,
    at: Instant,
    used: bool,
}

/// What a request thread reports back.
enum Reply {
    Loaded {
        catalog: Vec<CommerceProduct>,
        pending: Vec<Value>,
        owned: Vec<u64>,
        creator: bool,
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
    /// A joined player's passes and ungranted purchases, as the Worker has
    /// them. `fresh`: read because the player said it bought something.
    PlayerRead {
        peer: PeerId,
        account: String,
        fresh: bool,
        state: Result<Value, String>,
    },
    /// A purchase id a joined player sent, as the Worker describes it.
    Checked {
        peer: PeerId,
        purchase_id: String,
        receipt: Result<Value, String>,
    },
    Fulfilled,
    FulfillFailed {
        purchase_id: String,
        reason: String,
    },
}

/// The purchase Studio's player is being asked about. Studio's dialog
/// (`ui/purchase_prompt.rs`) creates this resource, shows `showing`, and
/// writes `answer`; without it (a headless engine) purchases go through
/// unasked.
#[derive(Resource, Default)]
pub struct PurchaseConfirm {
    /// The purchase on screen.
    pub showing: Option<(PurchasePrompt, CommerceProduct)>,
    waiting: VecDeque<(PurchasePrompt, CommerceProduct)>,
    /// The answer to `showing`: `true` buys. Set by the dialog.
    pub answer: Option<bool>,
    /// Changes whenever `showing` does, so the dialog knows to redraw.
    pub generation: u64,
}

impl PurchaseConfirm {
    fn ask(&mut self, prompt: PurchasePrompt, product: CommerceProduct) {
        self.waiting.push_back((prompt, product));
    }

    /// Show the next purchase when nothing is on screen.
    fn advance(&mut self) {
        if self.showing.is_none() {
            if let Some(next) = self.waiting.pop_front() {
                self.showing = Some(next);
                self.answer = None;
                self.generation += 1;
            }
        }
    }

    /// The purchase on screen and the player's answer, once there is one.
    fn take_answer(&mut self) -> Option<(PurchasePrompt, CommerceProduct, bool)> {
        let buy = self.answer.take()?;
        let (prompt, product) = self.showing.take()?;
        self.generation += 1;
        Some((prompt, product, buy))
    }

    fn clear(&mut self) {
        self.showing = None;
        self.waiting.clear();
        self.answer = None;
        self.generation += 1;
    }
}

/// OnEnter(Editing): forget the session. Requests still out report to the
/// old inbox, which nothing reads.
pub fn reset_commerce(mut commerce: ResMut<PlayCommerce>, confirm: Option<ResMut<PurchaseConfirm>>) {
    *commerce = PlayCommerce::default();
    if let Some(mut confirm) = confirm {
        confirm.clear();
    }
}

/// Each Play frame, after the scripts: start commerce when a script (or a
/// joined player's purchase) asked for it, send what scripts queued, and hand
/// back what the API and the players answered.
#[allow(clippy::too_many_arguments)]
pub fn drive_commerce(
    dm: Option<Res<PlayDataModel>>,
    mut commerce: ResMut<PlayCommerce>,
    mut confirm: Option<ResMut<PurchaseConfirm>>,
    remote: Res<RemotePlayers>,
    mut notices: MessageReader<NetNotice>,
    mut offers: MessageWriter<PromptPurchase>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    let notices: Vec<NetNotice> = notices.read().cloned().collect();
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    let commerce = &mut *commerce;
    let now = Instant::now();

    for notice in notices {
        match notice {
            NetNotice::Receipts { peer, purchase_ids } => {
                // Checking them needs the catalog and the creator check, so
                // they wake commerce as a script would.
                g.commerce.wake();
                commerce.waiting_receipts.push((peer, purchase_ids));
            }
            NetNotice::PurchaseClosed { peer, prompt, purchased } => {
                let Some(offer) = commerce.offered.get_mut(&prompt).filter(|o| o.peer == peer) else { continue };
                if !purchased {
                    if let Some(offer) = commerce.offered.remove(&prompt) {
                        close_prompt(&mut g.commerce, &offer.prompt, false);
                    }
                } else if offer.claimed.is_none() {
                    // The player's word starts a check; the Worker settles it.
                    offer.claimed = Some(now);
                    commerce.reread.insert(peer);
                }
            }
            NetNotice::PeerLeft { peer, .. } => {
                let mut gone: Vec<u32> = commerce.offered.iter().filter(|(_, o)| o.peer == peer).map(|(id, _)| *id).collect();
                gone.sort_unstable();
                for id in gone {
                    if let Some(offer) = commerce.offered.remove(&id) {
                        close_prompt(&mut g.commerce, &offer.prompt, false);
                    }
                }
                commerce.read.remove(&peer);
                commerce.reread.remove(&peer);
                commerce.accepted.retain(|a| a.peer != peer);
            }
            _ => {}
        }
    }

    // No script has used MarketplaceService: nothing is queued or out.
    if g.commerce.status == CommerceStatus::Idle {
        return;
    }

    if g.commerce.status == CommerceStatus::Loading && !commerce.started {
        commerce.started = true;
        match session_api(space_root.as_deref(), auth.as_deref()) {
            Ok(api) => {
                let job = api.clone();
                spawn(&commerce.inbox, move || load(&job));
                commerce.api = Some(api);
            }
            Err(why) => {
                g.print(OutputLevel::Warn, SOURCE, format!("purchases are unavailable in this session: {why}"));
                g.commerce.status = CommerceStatus::Unavailable(why);
            }
        }
    }

    if g.commerce.status == CommerceStatus::Ready {
        read_players(&mut g, commerce, &remote);
        check_remote_receipts(&mut g, commerce, &remote);
    }
    send_prompts(&mut g, commerce, confirm.as_deref_mut(), &remote, &mut offers, now);

    if let Some(confirm) = confirm.as_deref_mut() {
        if let Some((prompt, product, buy)) = confirm.take_answer() {
            match (buy, commerce.api.clone()) {
                (true, Some(api)) => {
                    g.print(
                        OutputLevel::Info,
                        SOURCE,
                        format!(
                            "Test purchase: {} (#{}) for {} Tickets. Studio test mode: no Tickets move.",
                            product.name, product.number, product.price
                        ),
                    );
                    spawn(&commerce.inbox, move || buy_as_creator(&api, prompt, product));
                }
                _ => {
                    g.print(OutputLevel::Info, SOURCE, format!("{} was not bought: the purchase was cancelled", product.name));
                    close_prompt(&mut g.commerce, &prompt, false);
                }
            }
        }
        confirm.advance();
    }

    for decision in std::mem::take(&mut g.commerce.decisions) {
        let receipt = commerce.handed.remove(&decision.purchase_id);
        let remote_live = commerce.remote_live.remove(&decision.purchase_id);
        if decision.granted {
            if let Some(api) = commerce.api.clone() {
                // A joined player's purchase is fulfilled in the mode it was
                // made in: live for a real purchase.
                let mode = if remote_live == Some(true) { Mode::Live } else { Mode::Test };
                let purchase_id = decision.purchase_id;
                spawn(&commerce.inbox, move || fulfill(&api, purchase_id, mode));
            }
        } else if remote_live.is_some() {
            // Offered again when that player's state is next read: after its
            // next purchase, or next session.
            commerce.seen.remove(&decision.purchase_id);
        } else if let Some(receipt) = receipt {
            commerce.declined.push(receipt);
        }
    }

    let replies = std::mem::take(&mut *commerce.inbox.lock());
    for reply in replies {
        take_reply(&mut g, commerce, &remote, reply);
    }
    expire_claims(&mut g, commerce, now);
}

/// Read each signed-in joined player's state once, and again after it says
/// it bought something. Only a session the listing's creator hosts may; any
/// other has nothing to wait for.
fn read_players(g: &mut DataModel, commerce: &mut PlayCommerce, remote: &RemotePlayers) {
    let Some(api) = commerce.api.clone().filter(|_| commerce.creator) else {
        g.commerce.peer_passes_pending.clear();
        commerce.reread.clear();
        return;
    };
    for player in remote.players() {
        let Some(account) = player.account.clone().filter(|_| player.settled()) else { continue };
        if !account.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            continue;
        }
        let fresh = commerce.reread.remove(&player.peer);
        if !commerce.read.insert(player.peer) && !fresh {
            continue;
        }
        let (api, peer) = (api.clone(), player.peer);
        spawn(&commerce.inbox, move || {
            let path = format!("/api/commerce/players/{account}?sim_id={}", api.sim_id);
            let state = api.call(Mode::Live, "GET", &path, None);
            Reply::PlayerRead { peer, account, fresh, state }
        });
    }
}

/// Check the purchase ids joined players sent, once their identity settled.
fn check_remote_receipts(g: &mut DataModel, commerce: &mut PlayCommerce, remote: &RemotePlayers) {
    if commerce.waiting_receipts.is_empty() {
        return;
    }
    let Some(api) = commerce.api.clone().filter(|_| commerce.creator) else {
        commerce.waiting_receipts.clear();
        if !commerce.warned_not_creator {
            commerce.warned_not_creator = true;
            g.print(
                OutputLevel::Warn,
                SOURCE,
                "players' purchases are only granted in sessions the listing's creator hosts; this account is not its creator",
            );
        }
        return;
    };
    let mut later = Vec::new();
    for (peer, ids) in std::mem::take(&mut commerce.waiting_receipts) {
        match remote.by_peer(peer) {
            None => continue,
            Some(player) if !player.settled() => {
                later.push((peer, ids));
                continue;
            }
            Some(_) => {}
        }
        for purchase_id in ids {
            if !commerce.seen.insert(purchase_id.clone()) {
                continue;
            }
            let api = api.clone();
            spawn(&commerce.inbox, move || {
                let path = format!("/api/commerce/receipts/{}/{}", api.sim_id, purchase_id);
                let receipt = request(&api.base, None, None, "GET", &path, None);
                Reply::Checked { peer, purchase_id, receipt }
            });
        }
    }
    commerce.waiting_receipts = later;
}

/// Sell what scripts prompted for, once the catalog is here: Studio's own
/// player through the dialog, a joined player through its Player. A prompt
/// that cannot be sold closes unpurchased, with the reason in the Output.
fn send_prompts(
    g: &mut DataModel,
    commerce: &mut PlayCommerce,
    mut confirm: Option<&mut PurchaseConfirm>,
    remote: &RemotePlayers,
    offers: &mut MessageWriter<PromptPurchase>,
    now: Instant,
) {
    if g.commerce.prompts.is_empty() || matches!(g.commerce.status, CommerceStatus::Idle | CommerceStatus::Loading) {
        return;
    }
    let local = local_user_id(g);
    let mut later = Vec::new();
    for prompt in std::mem::take(&mut g.commerce.prompts) {
        let ready = matches!((&commerce.api, &g.commerce.status), (Some(_), CommerceStatus::Ready));
        // A joined player whose passes are still on their way is asked once
        // they arrive, so it is never offered a pass it owns.
        if ready && Some(prompt.user_id) != local && g.commerce.passes_pending(prompt.user_id) {
            later.push(prompt);
            continue;
        }
        let decision = match &g.commerce.status {
            CommerceStatus::Unavailable(why) => Err(format!("purchases are unavailable: {why}")),
            _ if !ready => Err("purchases are unavailable".to_string()),
            _ if Some(prompt.user_id) == local => local_refusal(&g.commerce, &prompt).map_or(Ok(None), Err),
            _ => {
                let buyer = remote.by_user_id(prompt.user_id).map(|p| Buyer {
                    peer: p.peer,
                    name: &p.name,
                    account: p.account.as_deref(),
                    user_id: p.user_id,
                });
                remote_refusal(&g.commerce, &prompt, buyer.as_ref(), commerce.creator).map(Some)
            }
        };
        let Some(product) = g.commerce.product(prompt.product).cloned() else {
            let reason = decision.err().unwrap_or_else(|| "this simulation has no such product".to_string());
            refuse(g, &prompt, &reason);
            continue;
        };
        match decision {
            Err(reason) => refuse(g, &prompt, &reason),
            // Studio's own player: asked in the dialog, or bought at once
            // with no dialog to ask in.
            Ok(None) => match confirm.as_deref_mut() {
                Some(confirm) => confirm.ask(prompt, product),
                None => {
                    if let Some(api) = commerce.api.clone() {
                        spawn(&commerce.inbox, move || buy_as_creator(&api, prompt, product));
                    }
                }
            },
            // A joined player: its Player asks and buys.
            Ok(Some(peer)) => {
                commerce.next_offer = commerce.next_offer.wrapping_add(1).max(1);
                let id = commerce.next_offer;
                let name = remote.by_peer(peer).map(|p| p.name.clone()).unwrap_or_default();
                g.print(OutputLevel::Info, SOURCE, format!("offered {} (#{}) to {name}", product.name, product.number));
                offers.write(PromptPurchase { peer, prompt: id, product: product.number, expects: expects_code(prompt.expects) });
                commerce.offered.insert(id, Offer { prompt, peer, kind: product.kind, sent: now, claimed: None });
            }
        }
    }
    g.commerce.prompts.extend(later);
}

fn refuse(g: &mut DataModel, prompt: &PurchasePrompt, reason: &str) {
    g.print(OutputLevel::Warn, SOURCE, format!("product {} was not sold: {reason}", prompt.product));
    close_prompt(&mut g.commerce, prompt, false);
}

/// What the wire says a prompt expects: 0 any product, 1 a consumable, 2 a pass.
fn expects_code(expects: Option<ProductKind>) -> u8 {
    match expects {
        None => 0,
        Some(ProductKind::Consumable) => 1,
        Some(ProductKind::Pass) => 2,
    }
}

/// Why Studio's own player cannot buy `prompt`, or `None` when it can.
fn local_refusal(state: &CommerceState, prompt: &PurchasePrompt) -> Option<String> {
    let product = state.product(prompt.product)?;
    kind_refusal(prompt, product)
        .or_else(|| (product.kind == ProductKind::Pass && state.owned_passes.contains(&product.number)).then(|| "the pass is already owned".to_string()))
}

/// What commerce needs to know about a joined player.
struct Buyer<'a> {
    peer: PeerId,
    name: &'a str,
    account: Option<&'a str>,
    user_id: f64,
}

/// Why a joined player cannot be offered `prompt`, or the peer to offer it to.
fn remote_refusal(state: &CommerceState, prompt: &PurchasePrompt, buyer: Option<&Buyer>, creator: bool) -> Result<PeerId, String> {
    let buyer = buyer.ok_or_else(|| "no player in this session has that UserId".to_string())?;
    if !creator {
        return Err("purchases are only offered to players in sessions the listing's creator hosts".to_string());
    }
    if buyer.account.is_none() {
        return Err(format!("{} is not signed in, so cannot buy", buyer.name));
    }
    let product = state.product(prompt.product).ok_or_else(|| "this simulation has no such product".to_string())?;
    if !product.active {
        return Err("it is a draft: put it on sale before players can buy it".to_string());
    }
    if let Some(why) = kind_refusal(prompt, product) {
        return Err(why);
    }
    if product.kind == ProductKind::Pass && state.owns_pass(buyer.user_id, None, product.number) {
        return Err(format!("{} already owns the pass", buyer.name));
    }
    Ok(buyer.peer)
}

/// A prompt that asked for the other kind of product.
fn kind_refusal(prompt: &PurchasePrompt, product: &CommerceProduct) -> Option<String> {
    match (prompt.expects, product.kind) {
        (Some(ProductKind::Pass), ProductKind::Consumable) => Some("it is a product, not a pass: use PromptProductPurchase".to_string()),
        (Some(ProductKind::Consumable), ProductKind::Pass) => Some("it is a pass: use PromptGamePassPurchase".to_string()),
        _ => None,
    }
}

fn close_prompt(state: &mut CommerceState, prompt: &PurchasePrompt, purchased: bool) {
    state.outcomes.push(PromptOutcome {
        user_id: prompt.user_id,
        product: prompt.product,
        expects: prompt.expects,
        purchased,
    });
}

fn take_reply(g: &mut DataModel, commerce: &mut PlayCommerce, remote: &RemotePlayers, reply: Reply) {
    match reply {
        Reply::Loaded { catalog, pending, owned, creator } => {
            let count = catalog.len();
            g.commerce.catalog = catalog;
            g.commerce.owned_passes = owned.into_iter().collect();
            g.commerce.status = CommerceStatus::Ready;
            commerce.creator = creator;
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
                        spawn(&commerce.inbox, move || fulfill(&api, purchase_id, Mode::Test));
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
        Reply::PlayerRead { peer, account, fresh, state } => {
            let Some(player) = remote.by_peer(peer).filter(|p| p.account.as_deref() == Some(account.as_str())) else { return };
            let key = user_key(player.user_id);
            match state {
                Ok(state) => {
                    let list = |field: &str| state.get(field).and_then(Value::as_array).cloned().unwrap_or_default();
                    if let Some(key) = key {
                        let owned: Vec<u64> = list("entitlements").iter().filter_map(|e| e.get("number").and_then(Value::as_u64)).collect();
                        g.commerce.peer_passes.entry(key).or_default().extend(owned);
                    }
                    for pending in list("pending") {
                        if pending.get("buyer_id").and_then(Value::as_str) != Some(account.as_str()) {
                            continue;
                        }
                        let Some(receipt) = receipt_from_pending(&pending, player.user_id) else { continue };
                        if !commerce.seen.insert(receipt.purchase_id.clone()) {
                            continue;
                        }
                        let live = pending.get("livemode").and_then(Value::as_bool).unwrap_or(false);
                        // Only a purchase read after the player said it
                        // bought can confirm that it did.
                        take_remote_consumable(&mut g.commerce, commerce, peer, receipt, live, fresh);
                    }
                }
                Err(why) => g.print(OutputLevel::Warn, SOURCE, format!("could not read {}'s purchases: {why}", player.name)),
            }
            if let Some(key) = key {
                g.commerce.peer_passes_pending.remove(&key);
            }
            settle_claims(&mut g.commerce, commerce, peer, player.user_id);
        }
        Reply::Checked { peer, purchase_id, receipt } => {
            let Some(player) = remote.by_peer(peer) else { return };
            let receipt = match receipt {
                Ok(receipt) => receipt,
                Err(why) => {
                    // Checked again if the player sends it again.
                    commerce.seen.remove(&purchase_id);
                    g.print(OutputLevel::Warn, SOURCE, format!("could not check {}'s purchase {purchase_id}: {why}", player.name));
                    return;
                }
            };
            match accept_remote_receipt(&receipt, player.account.as_deref(), player.user_id, &purchase_id) {
                Some(RemoteGrant::Pass(number)) => {
                    if let Some(key) = user_key(player.user_id) {
                        g.commerce.peer_passes.entry(key).or_default().insert(number);
                    }
                }
                Some(RemoteGrant::Consumable(receipt, live)) => {
                    take_remote_consumable(&mut g.commerce, commerce, peer, receipt, live, true);
                }
                None => {}
            }
            settle_claims(&mut g.commerce, commerce, peer, player.user_id);
        }
        Reply::Fulfilled => {}
        Reply::FulfillFailed { purchase_id, reason } => g.print(
            OutputLevel::Warn,
            SOURCE,
            format!("could not record {purchase_id} as granted ({reason}); ProcessReceipt will see it again next session"),
        ),
    }
}

/// Hand a joined player's consumable to `ProcessReceipt`. A `fresh` one (it
/// arrived after the player said it bought) can confirm that claim.
fn take_remote_consumable(state: &mut CommerceState, commerce: &mut PlayCommerce, peer: PeerId, receipt: Receipt, live: bool, fresh: bool) {
    if fresh {
        commerce.accepted.push(Accepted { peer, product: receipt.product, at: Instant::now(), used: false });
    }
    commerce.remote_live.insert(receipt.purchase_id.clone(), live);
    hand_to_scripts(state, commerce, receipt);
}

/// Close as purchased the prompts `peer` said it bought, once the Worker
/// shows it: the pass is owned, or a purchase of the product arrived after
/// the prompt was offered.
fn settle_claims(state: &mut CommerceState, commerce: &mut PlayCommerce, peer: PeerId, user_id: f64) {
    let mut claimed: Vec<u32> = commerce
        .offered
        .iter()
        .filter(|(_, o)| o.peer == peer && o.claimed.is_some())
        .map(|(id, _)| *id)
        .collect();
    claimed.sort_unstable();
    for id in claimed {
        let Some(offer) = commerce.offered.get(&id) else { continue };
        let confirmed = match offer.kind {
            ProductKind::Pass => state.owns_pass(user_id, None, offer.prompt.product),
            ProductKind::Consumable => {
                let found = commerce
                    .accepted
                    .iter_mut()
                    .find(|a| a.peer == peer && a.product == offer.prompt.product && a.at >= offer.sent && !a.used);
                match found {
                    Some(accepted) => {
                        accepted.used = true;
                        true
                    }
                    None => false,
                }
            }
        };
        if confirmed {
            if let Some(offer) = commerce.offered.remove(&id) {
                close_prompt(state, &offer.prompt, true);
            }
        }
    }
}

/// A prompt a player said it bought, with no purchase to show for it after
/// [`CLAIM_WAIT`], closes unpurchased.
fn expire_claims(g: &mut DataModel, commerce: &mut PlayCommerce, now: Instant) {
    let mut late: Vec<u32> = commerce
        .offered
        .iter()
        .filter(|(_, o)| o.claimed.is_some_and(|at| now.saturating_duration_since(at) > CLAIM_WAIT))
        .map(|(id, _)| *id)
        .collect();
    late.sort_unstable();
    for id in late {
        if let Some(offer) = commerce.offered.remove(&id) {
            g.print(
                OutputLevel::Warn,
                SOURCE,
                format!("a player said it bought product {}, but the purchase never arrived; the prompt closed unpurchased", offer.prompt.product),
            );
            close_prompt(&mut g.commerce, &offer.prompt, false);
        }
    }
    commerce.accepted.retain(|a| !a.used && now.saturating_duration_since(a.at) < ACCEPTED_KEPT);
}

/// What a joined player's purchase gives it.
#[derive(Debug, PartialEq)]
enum RemoteGrant {
    /// A pass it owns.
    Pass(u64),
    /// A consumable to hand to `ProcessReceipt`, and whether it is live.
    Consumable(Receipt, bool),
}

/// Accept a joined player's purchase, as the Worker describes it: it must be
/// that player's own (its verified account bought it), unrefunded, and, for a
/// consumable, not yet granted.
fn accept_remote_receipt(receipt: &Value, account: Option<&str>, user_id: f64, purchase_id: &str) -> Option<RemoteGrant> {
    let buyer = receipt.get("buyer_id")?.as_str()?;
    if account != Some(buyer) || receipt.get("status")?.as_str()? != "succeeded" {
        return None;
    }
    let product = receipt.get("product")?;
    let number = product.get("number")?.as_u64()?;
    if product.get("type").and_then(Value::as_str) == Some("pass") {
        return Some(RemoteGrant::Pass(number));
    }
    if receipt.get("fulfilled").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    Some(RemoteGrant::Consumable(
        Receipt {
            purchase_id: purchase_id.to_string(),
            user_id,
            product: number,
            price: receipt.get("amount").and_then(Value::as_u64).unwrap_or(0),
            sim_id: receipt.get("sim_id").and_then(Value::as_str).unwrap_or_default().to_string(),
            space: receipt.get("space").and_then(Value::as_str).map(str::to_string),
        },
        receipt.get("livemode").and_then(Value::as_bool).unwrap_or(false),
    ))
}

fn hand_to_scripts(state: &mut CommerceState, commerce: &mut PlayCommerce, receipt: Receipt) {
    commerce.handed.insert(receipt.purchase_id.clone(), receipt.clone());
    state.receipts.push_back(receipt);
}

/// The local player's `UserId`: in Studio, the signed-in creator.
fn local_user_id(g: &DataModel) -> Option<f64> {
    g.local_player.and_then(|p| g.get_prop(p, "UserId")).and_then(|v| v.as_number())
}

// ─────────────────────────────────────────────────────────────────────────────
// The host's own identity
// ─────────────────────────────────────────────────────────────────────────────

/// When this host starts listening, fetch its identity ticket (audience: its
/// certificate pin) and give it to the session, so every Welcome carries it.
/// Joining players check it before they buy. Runs every frame, Play or not.
pub fn announce_host_identity(
    mut notices: MessageReader<NetNotice>,
    auth: Option<Res<crate::auth::AuthState>>,
    host: Option<ResMut<HostSession>>,
    mut pending: Local<Option<Arc<Mutex<Option<String>>>>>,
) {
    for notice in notices.read() {
        let NetNotice::Hosting { pin, .. } = notice else { continue };
        let Some(token) = auth.as_ref().and_then(|a| a.token.clone()).filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let slot = Arc::new(Mutex::new(None));
        let out = slot.clone();
        let audience = hex32(pin);
        let started = std::thread::Builder::new().name("eustress-host-identity".into()).spawn(move || {
            match fetch_identity_ticket(&token, &audience) {
                Ok(ticket) => *out.lock() = Some(ticket),
                Err(e) => warn!("multiplayer: no host identity ticket ({e}); players cannot buy in this session"),
            }
        });
        if started.is_ok() {
            *pending = Some(slot);
        }
    }
    let ready = pending.as_ref().and_then(|slot| slot.lock().take());
    if let Some(ticket) = ready {
        *pending = None;
        if let Some(mut host) = host {
            host.set_identity(Some(ticket));
        }
    }
}

/// An identity ticket for the signed-in account (`token`), for the host
/// connection named by `audience` (a certificate pin, as 64 hex characters).
pub fn fetch_identity_ticket(token: &str, audience: &str) -> Result<String, String> {
    let reply = request(&api_base(), Some(token), None, "POST", "/api/identity/ticket", Some(&json!({ "audience": audience })))?;
    reply
        .get("ticket")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the API returned no ticket".to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// The Commerce API
// ─────────────────────────────────────────────────────────────────────────────

/// Whether this session can sell, and to whom it talks.
fn session_api(space_root: Option<&crate::space::SpaceRoot>, auth: Option<&crate::auth::AuthState>) -> Result<CommerceApi, String> {
    let token = auth
        .and_then(|a| a.token.clone())
        .filter(|t| !t.trim().is_empty())
        .ok_or("sign in to Eustress to test purchases")?;
    let space_root = space_root.map(|s| s.0.clone()).ok_or("no Space is open")?;
    let universe = crate::space::universe_root_for_path(&space_root).unwrap_or_else(|| space_root.clone());
    let sim_id = published_id(&universe).ok_or("publish this Universe first: products belong to a published simulation")?;
    Ok(CommerceApi {
        base: api_base(),
        token,
        sim_id,
        space: space_root.file_name().map(|n| n.to_string_lossy().into_owned()),
    })
}

/// The Universe's listing id, if it has been published.
fn published_id(universe: &Path) -> Option<String> {
    let path = universe.join(".eustress").join("sync.toml");
    let sync = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path).ok()?;
    let id = sync.remote.experience_id?.trim().to_string();
    let valid = (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    valid.then_some(id)
}

/// The API in use: `EUSTRESS_API_URL` when it is an accepted base, else
/// production (`eustress_common::api_base`).
pub(crate) fn api_base() -> String {
    eustress_common::api_base::api_base().to_string()
}

impl CommerceApi {
    fn call(&self, mode: Mode, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        request(&self.base, Some(&self.token), Some(mode), method, path, body)
    }
}

/// One request. A signed-in commerce request always names its mode, so none
/// can fall into live by leaving the header out; only joined players'
/// purchases are ever read or fulfilled `Live`.
fn request(base: &str, token: Option<&str>, mode: Option<Mode>, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let mut request = ureq::request(method, &format!("{base}{path}")).timeout(REQUEST_TIMEOUT);
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    if let Some(mode) = mode {
        request = request.set("Eustress-Mode", if matches!(mode, Mode::Live) { "live" } else { "test" });
    }
    let result = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    match result {
        Ok(response) => response.into_json::<Value>().map_err(|e| format!("unreadable reply: {e}")),
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_json::<Value>().unwrap_or(Value::Null);
            let message = body.get("error").and_then(Value::as_str).unwrap_or("request failed");
            let code = body.get("code").and_then(Value::as_str).unwrap_or("");
            Err(format!("{message} [{status} {code}]"))
        }
        Err(e) => Err(format!("could not reach {base}: {e}")),
    }
}

fn spawn(inbox: &Arc<Mutex<Vec<Reply>>>, job: impl FnOnce() -> Reply + Send + 'static) {
    let inbox = inbox.clone();
    let started = std::thread::Builder::new().name("eustress-commerce".into()).spawn(move || {
        let reply = job();
        inbox.lock().push(reply);
    });
    if let Err(e) = started {
        warn!("commerce: could not start a request thread: {e}");
    }
}

fn load(api: &CommerceApi) -> Reply {
    let catalog = match api.call(Mode::Test, "GET", &format!("/api/commerce/catalog/{}", api.sim_id), None) {
        Ok(list) => list,
        Err(why) => return Reply::LoadFailed(why),
    };
    let catalog: Vec<CommerceProduct> = items(&catalog).iter().filter_map(product_from_json).collect();
    // A failure below costs the earlier receipts, the passes, or selling to
    // joined players for this session, never the catalog.
    let pending = api
        .call(Mode::Test, "GET", &format!("/api/commerce/me/pending?sim_id={}", api.sim_id), None)
        .map(|list| items(&list))
        .unwrap_or_default();
    let owned = api
        .call(Mode::Test, "GET", &format!("/api/commerce/me/entitlements?sim_id={}", api.sim_id), None)
        .map(|list| items(&list).iter().filter_map(|e| e.get("number").and_then(Value::as_u64)).collect())
        .unwrap_or_default();
    // The account's own simulations: whether it is this listing's creator.
    let creator = api
        .call(Mode::Test, "GET", "/api/commerce/account", None)
        .map(|account| {
            account
                .get("simulations")
                .and_then(Value::as_array)
                .is_some_and(|sims| sims.iter().any(|s| s.get("id").and_then(Value::as_str) == Some(api.sim_id.as_str())))
        })
        .unwrap_or(false);
    Reply::Loaded { catalog, pending, owned, creator }
}

/// Studio's own player buys: a test purchase, as the creator.
fn buy_as_creator(api: &CommerceApi, prompt: PurchasePrompt, product: CommerceProduct) -> Reply {
    let body = json!({
        "sim_id": api.sim_id,
        "product": product.number,
        "expected_price": product.price,
        "idempotency_key": format!("studio_{}", uuid::Uuid::new_v4().simple()),
        "space": api.space,
    });
    match api.call(Mode::Test, "POST", "/api/commerce/purchases", Some(&body)) {
        Ok(reply) => Reply::Bought { prompt, product, purchase: reply.get("purchase").cloned().unwrap_or(Value::Null) },
        Err(reason) => Reply::NotBought { prompt, reason },
    }
}

fn fulfill(api: &CommerceApi, purchase_id: String, mode: Mode) -> Reply {
    let path = format!("/api/commerce/purchases/{purchase_id}/fulfill");
    match api.call(mode, "POST", &path, Some(&json!({ "sim_id": api.sim_id }))) {
        Ok(_) => Reply::Fulfilled,
        Err(reason) => Reply::FulfillFailed { purchase_id, reason },
    }
}

fn items(list: &Value) -> Vec<Value> {
    list.get("data").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn product_from_json(p: &Value) -> Option<CommerceProduct> {
    let text = |key: &str| p.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Some(CommerceProduct {
        id: p.get("id")?.as_str()?.to_string(),
        number: p.get("number")?.as_u64()?,
        name: text("name"),
        description: text("description"),
        price: p.get("price")?.as_u64()?,
        kind: ProductKind::from_api(p.get("type").and_then(Value::as_str).unwrap_or("consumable")),
        icon: p.get("icon").and_then(Value::as_str).map(str::to_string),
        active: p.get("active").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// A receipt from a pending list: a purchase no session granted yet.
fn receipt_from_pending(r: &Value, user_id: f64) -> Option<Receipt> {
    Some(Receipt {
        purchase_id: r.get("purchase_id")?.as_str()?.to_string(),
        user_id,
        product: r.get("product")?.get("number")?.as_u64()?,
        price: r.get("amount").and_then(Value::as_u64).unwrap_or(0),
        sim_id: r.get("sim_id").and_then(Value::as_str).unwrap_or_default().to_string(),
        space: r.get("space").and_then(Value::as_str).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(number: u64, kind: ProductKind, active: bool) -> CommerceProduct {
        CommerceProduct {
            id: format!("prod_{number}"),
            number,
            name: format!("P{number}"),
            description: String::new(),
            price: 10,
            kind,
            icon: None,
            active,
        }
    }

    fn prompt(user_id: f64, product: u64, expects: Option<ProductKind>) -> PurchasePrompt {
        PurchasePrompt { user_id, product, expects }
    }

    fn catalog() -> CommerceState {
        let mut state = CommerceState::default();
        state.catalog = vec![
            product(1, ProductKind::Consumable, true),
            product(2, ProductKind::Pass, true),
            product(3, ProductKind::Consumable, false),
        ];
        state
    }

    /// A prompt offered to `peer` at `sent`, which the player says it bought.
    fn claimed_offer(commerce: &mut PlayCommerce, id: u32, peer: PeerId, number: u64, kind: ProductKind, sent: Instant) {
        commerce.offered.insert(id, Offer { prompt: prompt(42.0, number, None), peer, kind, sent, claimed: Some(sent) });
    }

    #[test]
    fn studio_refuses_the_wrong_product_kind_or_an_owned_pass() {
        let mut state = catalog();
        assert_eq!(local_refusal(&state, &prompt(1.0, 1, Some(ProductKind::Consumable))), None);
        assert_eq!(local_refusal(&state, &prompt(1.0, 3, None)), None, "the creator may test a draft");
        assert!(local_refusal(&state, &prompt(1.0, 1, Some(ProductKind::Pass))).is_some());
        assert!(local_refusal(&state, &prompt(1.0, 2, Some(ProductKind::Consumable))).is_some());
        assert_eq!(local_refusal(&state, &prompt(1.0, 2, Some(ProductKind::Pass))), None);
        state.owned_passes.insert(2);
        assert!(local_refusal(&state, &prompt(1.0, 2, None)).is_some(), "already owned");
    }

    #[test]
    fn a_joined_player_is_offered_only_what_it_can_buy_from_a_creator_host() {
        let mut state = catalog();
        let signed_in = Buyer { peer: 7, name: "ada", account: Some("acct-ada"), user_id: 42.0 };
        let guest = Buyer { peer: 8, name: "guest", account: None, user_id: 43.0 };
        assert_eq!(remote_refusal(&state, &prompt(42.0, 1, None), Some(&signed_in), true), Ok(7));
        assert!(remote_refusal(&state, &prompt(42.0, 1, None), Some(&signed_in), false).is_err(), "not the creator's session");
        assert!(remote_refusal(&state, &prompt(43.0, 1, None), Some(&guest), true).is_err(), "not signed in");
        assert!(remote_refusal(&state, &prompt(42.0, 1, None), None, true).is_err(), "no such player");
        assert!(remote_refusal(&state, &prompt(42.0, 3, None), Some(&signed_in), true).is_err(), "a draft");
        assert!(remote_refusal(&state, &prompt(42.0, 1, Some(ProductKind::Pass)), Some(&signed_in), true).is_err());
        state.peer_passes.entry(42).or_default().insert(2);
        assert!(remote_refusal(&state, &prompt(42.0, 2, None), Some(&signed_in), true).is_err(), "already owned");
        assert_eq!(expects_code(Some(ProductKind::Pass)), 2);
    }

    #[test]
    fn a_joined_players_purchase_is_granted_only_when_it_is_its_own() {
        let receipt = json!({
            "buyer_id": "acct-ada", "status": "succeeded", "fulfilled": false, "livemode": true,
            "product": { "number": 1, "type": "consumable" }, "amount": 50, "sim_id": "sim", "space": "Lobby",
        });
        match accept_remote_receipt(&receipt, Some("acct-ada"), 42.0, "pur_a") {
            Some(RemoteGrant::Consumable(r, live)) => {
                assert!(live);
                assert_eq!((r.purchase_id.as_str(), r.user_id, r.product, r.price), ("pur_a", 42.0, 1, 50));
            }
            other => panic!("expected a consumable, got {other:?}"),
        }
        assert_eq!(accept_remote_receipt(&receipt, Some("acct-bob"), 42.0, "pur_a"), None, "someone else's");
        assert_eq!(accept_remote_receipt(&receipt, None, 42.0, "pur_a"), None, "a guest");
        let mut granted = receipt.clone();
        granted["fulfilled"] = json!(true);
        assert_eq!(accept_remote_receipt(&granted, Some("acct-ada"), 42.0, "pur_a"), None, "already granted");
        let mut refunded = receipt.clone();
        refunded["status"] = json!("refunded");
        assert_eq!(accept_remote_receipt(&refunded, Some("acct-ada"), 42.0, "pur_a"), None);
        let mut pass = receipt.clone();
        pass["product"] = json!({ "number": 2, "type": "pass" });
        pass["fulfilled"] = json!(true);
        assert_eq!(accept_remote_receipt(&pass, Some("acct-ada"), 42.0, "pur_b"), Some(RemoteGrant::Pass(2)));
    }

    #[test]
    fn a_claimed_purchase_closes_purchased_only_when_the_worker_shows_it() {
        let mut state = catalog();
        let mut commerce = PlayCommerce::default();
        let t0 = Instant::now();
        claimed_offer(&mut commerce, 1, 7, 1, ProductKind::Consumable, t0);
        claimed_offer(&mut commerce, 2, 7, 2, ProductKind::Pass, t0);

        // The player's word alone closes nothing.
        settle_claims(&mut state, &mut commerce, 7, 42.0);
        assert!(state.outcomes.is_empty());

        // A purchase from before the offer does not count, nor one another
        // player made.
        commerce.accepted.push(Accepted { peer: 7, product: 1, at: t0 - Duration::from_millis(1), used: false });
        commerce.accepted.push(Accepted { peer: 8, product: 1, at: t0 + Duration::from_secs(1), used: false });
        settle_claims(&mut state, &mut commerce, 7, 42.0);
        assert!(state.outcomes.is_empty());

        // A new purchase of the product does, once; an owned pass does.
        commerce.accepted.push(Accepted { peer: 7, product: 1, at: t0 + Duration::from_secs(2), used: false });
        state.peer_passes.entry(42).or_default().insert(2);
        settle_claims(&mut state, &mut commerce, 7, 42.0);
        let closed: Vec<_> = state.outcomes.iter().map(|o| (o.product, o.purchased)).collect();
        assert_eq!(closed, vec![(1, true), (2, true)]);
        assert!(commerce.offered.is_empty());
        assert_eq!(commerce.accepted.iter().filter(|a| a.used).count(), 1);
    }

    #[test]
    fn a_claim_with_no_purchase_closes_unpurchased_after_the_wait() {
        let mut g = DataModel::new();
        let mut commerce = PlayCommerce::default();
        let t0 = Instant::now();
        claimed_offer(&mut commerce, 1, 7, 1, ProductKind::Consumable, t0);
        expire_claims(&mut g, &mut commerce, t0 + CLAIM_WAIT / 2);
        assert!(g.commerce.outcomes.is_empty());
        expire_claims(&mut g, &mut commerce, t0 + CLAIM_WAIT + Duration::from_secs(1));
        assert_eq!(g.commerce.outcomes.iter().map(|o| o.purchased).collect::<Vec<_>>(), vec![false]);
        assert!(commerce.offered.is_empty());
    }

    #[test]
    fn the_dialog_shows_one_purchase_at_a_time_in_order() {
        let mut confirm = PurchaseConfirm::default();
        confirm.ask(prompt(1.0, 1, None), product(1, ProductKind::Consumable, true));
        confirm.ask(prompt(1.0, 2, None), product(2, ProductKind::Pass, true));
        assert!(confirm.showing.is_none());
        confirm.advance();
        let shown = confirm.generation;
        assert_eq!(confirm.showing.as_ref().map(|(p, _)| p.product), Some(1));
        assert!(confirm.take_answer().is_none(), "no answer yet");
        confirm.answer = Some(false);
        let (asked, _, buy) = confirm.take_answer().expect("answered");
        assert_eq!((asked.product, buy), (1, false));
        confirm.advance();
        assert!(confirm.generation > shown);
        assert_eq!(confirm.showing.as_ref().map(|(p, _)| p.product), Some(2));
        confirm.clear();
        assert!(confirm.showing.is_none());
        confirm.advance();
        assert!(confirm.showing.is_none(), "cleared, nothing left to show");
    }

    #[test]
    fn catalog_and_receipt_json_parse() {
        let p = product_from_json(&json!({
            "id": "prod_x", "number": 3, "name": "Coins", "price": 50, "type": "pass", "active": true,
        }))
        .unwrap();
        assert_eq!((p.number, p.price, p.kind, p.active), (3, 50, ProductKind::Pass, true));
        assert!(product_from_json(&json!({ "id": "prod_x" })).is_none());
        let r = receipt_from_pending(
            &json!({ "purchase_id": "pur_x", "product": { "number": 3 }, "amount": 50, "sim_id": "abc", "space": "Lobby" }),
            7.0,
        )
        .unwrap();
        assert_eq!((r.purchase_id.as_str(), r.product, r.price, r.user_id), ("pur_x", 3, 50, 7.0));
        assert_eq!(r.space.as_deref(), Some("Lobby"));
    }
}
