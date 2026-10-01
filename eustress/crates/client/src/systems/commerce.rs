//! # Buying in a host's session
//!
//! A script on the host can offer the player a product
//! (`NetNotice::PurchasePrompt`). The Player asks the player, buys with the
//! player's own account, live, and tells the host: whether it bought
//! ([`ClosePurchase`]) and the new purchase's id ([`SendReceipts`]). The host
//! checks that id with the Worker before it grants anything, so the Player's
//! word alone gets nothing.
//!
//! The Player buys only from a host that proved it is the listing's creator.
//! The host's Welcome carries an identity ticket for this connection's
//! certificate pin; the Player checks it with the Worker and compares the
//! account with the listing's author. Any other host could take the Tickets
//! and never grant the purchase, so it is refused.
//!
//! A signed-out player joins as a guest: it plays, and the host never offers
//! it anything. The signed-in account is the one Studio and the Player share
//! (`<local data>/EustressEngine/auth_token`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_networking::join_link::hex32;
use eustress_networking::{ClosePurchase, JoinLink, NetNotice, PlayerSession, SendReceipts};

use super::net_play::JoinTarget;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The ticket is fetched before joining, so a slow API delays the join by at
/// most this.
const TICKET_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a prompt waits for the host check before it is refused.
const HOST_CHECK_WAIT: Duration = Duration::from_secs(20);

pub struct PlayerCommercePlugin;

impl Plugin for PlayerCommercePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerCommerce>().add_systems(Update, drive_player_commerce);
    }
}

/// What the Player knows about buying in this session.
#[derive(Resource, Default)]
pub struct PlayerCommerce {
    /// Whether the host may sell to this player, once checked.
    host: HostCheck,
    /// Prompts the host sent, answered one at a time.
    asks: VecDeque<Ask>,
    /// A prompt is on screen or its purchase is under way.
    busy: bool,
    inbox: Arc<Mutex<Vec<Reply>>>,
}

#[derive(Default)]
enum HostCheck {
    /// Not joined yet.
    #[default]
    Waiting,
    Checking(Instant),
    /// The host is the listing's creator.
    Creator(Listing),
    Refused(String),
}

/// The listing the host serves, as the Worker describes it.
#[derive(Clone, Debug, PartialEq)]
struct Listing {
    sim_id: String,
    name: String,
    author: String,
}

#[derive(Clone, Copy, Debug)]
struct Ask {
    prompt: u32,
    product: u64,
    /// 0 any product, 1 a consumable, 2 a pass.
    expects: u8,
}

enum Reply {
    Host(Result<Listing, String>),
    /// The player answered prompt `prompt`: the new purchase's id when it
    /// bought.
    Answered { prompt: u32, bought: Result<Option<String>, String> },
}

/// An identity ticket for the signed-in player, bound to the host's
/// certificate pin: how the host learns which account joined. `None` when
/// signed out, when the host has no pin, or when the API does not answer in
/// time; the player then joins as a guest.
pub fn join_ticket(link: &JoinLink) -> Option<String> {
    let pin = link.pin?;
    let token = auth_token()?;
    let body = json!({ "audience": hex32(&pin) });
    match request(Some(&token), None, "POST", "/api/identity/ticket", Some(&body), TICKET_TIMEOUT) {
        Ok(reply) => reply.get("ticket").and_then(Value::as_str).map(str::to_string),
        Err(e) => {
            warn!("commerce: no identity ticket ({e}); joining as a guest, who cannot buy");
            None
        }
    }
}

fn drive_player_commerce(
    mut notices: MessageReader<NetNotice>,
    session: Option<Res<PlayerSession>>,
    target: Option<Res<JoinTarget>>,
    mut commerce: ResMut<PlayerCommerce>,
    mut closes: MessageWriter<ClosePurchase>,
    mut receipts: MessageWriter<SendReceipts>,
) {
    let commerce = &mut *commerce;
    for notice in notices.read() {
        match notice {
            NetNotice::Joined { .. } => {
                let sim_id = session.as_ref().and_then(|s| s.sim_id());
                let ticket = session.as_ref().and_then(|s| s.host_identity());
                let pin = target.as_ref().and_then(|t| t.link.pin);
                match host_to_check(sim_id, ticket, pin) {
                    Ok((sim_id, ticket, pin)) => {
                        commerce.host = HostCheck::Checking(Instant::now());
                        let inbox = commerce.inbox.clone();
                        spawn(inbox, move || Reply::Host(check_host(&ticket, &hex32(&pin), &sim_id)));
                    }
                    Err(why) => commerce.host = HostCheck::Refused(why.to_string()),
                }
            }
            NetNotice::PurchasePrompt { prompt, product, expects } => {
                commerce.asks.push_back(Ask { prompt: *prompt, product: *product, expects: *expects });
            }
            NetNotice::Disconnected { .. } | NetNotice::JoinFailed { .. } => {
                commerce.asks.clear();
                commerce.host = HostCheck::Waiting;
            }
            _ => {}
        }
    }

    let replies = std::mem::take(&mut *commerce.inbox.lock().unwrap_or_else(|e| e.into_inner()));
    for reply in replies {
        match reply {
            Reply::Host(Ok(listing)) => {
                info!("commerce: {} hosts {} as its creator; purchases here are real", listing.author, listing.name);
                commerce.host = HostCheck::Creator(listing);
            }
            Reply::Host(Err(why)) => {
                warn!("commerce: this host cannot sell to you: {why}");
                commerce.host = HostCheck::Refused(why);
            }
            Reply::Answered { prompt, bought } => {
                commerce.busy = false;
                match bought {
                    Ok(Some(purchase_id)) => {
                        info!("commerce: bought ({purchase_id})");
                        closes.write(ClosePurchase { prompt, purchased: true });
                        receipts.write(SendReceipts { purchase_ids: vec![purchase_id] });
                    }
                    Ok(None) => {
                        closes.write(ClosePurchase { prompt, purchased: false });
                    }
                    Err(why) => {
                        warn!("commerce: not bought: {why}");
                        closes.write(ClosePurchase { prompt, purchased: false });
                    }
                }
            }
        }
    }

    // One prompt at a time, as the host's scripts expect.
    while !commerce.busy {
        let Some(ask) = commerce.asks.front().copied() else { break };
        let listing = match next_step(&commerce.host, Instant::now()) {
            NextStep::Wait => break,
            NextStep::Ask(listing) => listing.clone(),
            NextStep::Refuse(why) => {
                info!("commerce: the host offered product {}; not buying from it: {why}", ask.product);
                commerce.asks.pop_front();
                closes.write(ClosePurchase { prompt: ask.prompt, purchased: false });
                continue;
            }
        };
        commerce.asks.pop_front();
        let Some(token) = auth_token() else {
            info!("commerce: sign in to Eustress to buy in this session");
            closes.write(ClosePurchase { prompt: ask.prompt, purchased: false });
            continue;
        };
        commerce.busy = true;
        let inbox = commerce.inbox.clone();
        spawn(inbox, move || Reply::Answered { prompt: ask.prompt, bought: ask_and_buy(&token, &listing, ask) });
    }
}

/// What happens to the next prompt, given what is known of the host.
#[derive(Debug, PartialEq)]
enum NextStep<'a> {
    /// The host is not checked yet: the prompt waits.
    Wait,
    /// Refused, and why.
    Refuse(&'a str),
    /// The host is the listing's creator: ask the player.
    Ask(&'a Listing),
}

/// Only a host verified as the listing's creator sells. One that could not be
/// checked within [`HOST_CHECK_WAIT`] is refused, as is every prompt of a
/// refused host.
fn next_step(host: &HostCheck, now: Instant) -> NextStep<'_> {
    match host {
        HostCheck::Creator(listing) => NextStep::Ask(listing),
        HostCheck::Waiting => NextStep::Wait,
        HostCheck::Checking(since) if now.saturating_duration_since(*since) < HOST_CHECK_WAIT => NextStep::Wait,
        HostCheck::Checking(_) => NextStep::Refuse("the host could not be checked in time"),
        HostCheck::Refused(why) => NextStep::Refuse(why),
    }
}

/// What checking a host needs: a listing id that is one (it goes into a URL
/// path), the host's identity ticket, and this connection's certificate pin.
fn host_to_check(
    sim_id: Option<&str>,
    ticket: Option<&str>,
    pin: Option<[u8; 32]>,
) -> Result<(String, String, [u8; 32]), &'static str> {
    let sim_id = sim_id
        .filter(|id| (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .ok_or("the host is not serving a published simulation")?;
    let ticket = ticket.ok_or("the host did not say who it is")?;
    let pin = pin.ok_or("the join link has no certificate pin")?;
    Ok((sim_id.to_string(), ticket.to_string(), pin))
}

/// Why a prompt asked for the other kind of product (`expects`: 1 a
/// consumable, 2 a pass), or `None`.
fn kind_refusal(expects: u8, is_pass: bool) -> Option<&'static str> {
    match (expects, is_pass) {
        (1, true) => Some("is a pass, but the host asked for a product"),
        (2, false) => Some("is a product, but the host asked for a pass"),
        _ => None,
    }
}

/// The host is the listing's creator: its ticket, for this connection's pin,
/// names the listing's author.
fn check_host(ticket: &str, audience: &str, sim_id: &str) -> Result<Listing, String> {
    let who = request(None, None, "POST", "/api/identity/verify", Some(&json!({ "ticket": ticket, "audience": audience })), REQUEST_TIMEOUT)?;
    let host_account = who.get("account_id").and_then(Value::as_str).ok_or("the host's ticket names no account")?;
    let sim = request(None, None, "GET", &format!("/api/simulations/{sim_id}"), None, REQUEST_TIMEOUT)?;
    let author = sim.get("author_id").and_then(Value::as_str).ok_or("the listing names no author")?;
    if author != host_account {
        return Err("the host is not this listing's creator".into());
    }
    let text = |key: &str| sim.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Ok(Listing { sim_id: sim_id.to_string(), name: text("name"), author: text("author_name") })
}

/// Ask the player about the product, and buy it live if the player agrees.
/// `Ok(None)`: the player did not buy.
fn ask_and_buy(token: &str, listing: &Listing, ask: Ask) -> Result<Option<String>, String> {
    let catalog = request(None, None, "GET", &format!("/api/commerce/catalog/{}", listing.sim_id), None, REQUEST_TIMEOUT)?;
    let product = catalog
        .get("data")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|p| p.get("number").and_then(Value::as_u64) == Some(ask.product)))
        .cloned()
        .ok_or_else(|| format!("{} sells no product {} right now", listing.name, ask.product))?;
    let name = product.get("name").and_then(Value::as_str).unwrap_or("this product").to_string();
    let price = product.get("price").and_then(Value::as_u64).ok_or("the product has no price")?;
    let pass = product.get("type").and_then(Value::as_str) == Some("pass");
    if let Some(why) = kind_refusal(ask.expects, pass) {
        return Err(format!("{name} {why}"));
    }
    let balance = request(Some(token), Some(true), "GET", "/api/commerce/me/balance", None, REQUEST_TIMEOUT)?
        .get("tickets")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if balance < price {
        tell(
            "Not enough Tickets",
            &format!("{name} costs {}. You have {}.\n\nGet Tickets at eustress.dev, then try again.", tickets(price), tickets(balance)),
        );
        return Ok(None);
    }
    let description = product.get("description").and_then(Value::as_str).filter(|d| !d.is_empty());
    let mut text = format!("{name}{}\n\n", if pass { " (a pass: yours to keep)" } else { "" });
    if let Some(description) = description {
        text.push_str(description);
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "Price: {}\nYou have {}; {} after.\n\nSold by {} in {}.",
        tickets(price),
        tickets(balance),
        tickets(balance - price),
        listing.author,
        listing.name
    ));
    if !confirm(&format!("Buy {name}?"), &text) {
        return Ok(None);
    }
    let body = json!({
        "sim_id": listing.sim_id,
        "product": ask.product,
        "expected_price": price,
        "idempotency_key": format!("player_{}", uuid::Uuid::new_v4().simple()),
    });
    match request(Some(token), Some(true), "POST", "/api/commerce/purchases", Some(&body), REQUEST_TIMEOUT) {
        Ok(reply) => reply
            .get("purchase")
            .and_then(|p| p.get("id"))
            .and_then(Value::as_str)
            .map(|id| Some(id.to_string()))
            .ok_or_else(|| "the purchase went through but the reply named no purchase".to_string()),
        Err(why) => {
            tell("Purchase failed", &format!("{name} was not bought: {why}"));
            Err(why)
        }
    }
}

/// "1 Ticket", "1,250 Tickets".
fn tickets(n: u64) -> String {
    let digits = n.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped} {}", if n == 1 { "Ticket" } else { "Tickets" })
}

/// Buy or Cancel. Windows without common controls v6 shows OK and Cancel for
/// the two custom buttons, so OK counts as Buy.
fn confirm(title: &str, text: &str) -> bool {
    let answer = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::OkCancelCustom("Buy".into(), "Cancel".into()))
        .show();
    matches!(answer, rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes)
        || matches!(&answer, rfd::MessageDialogResult::Custom(label) if label == "Buy")
}

fn tell(title: &str, text: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// The signed-in account's token, shared with Studio.
fn auth_token() -> Option<String> {
    let path = dirs::data_local_dir()?.join("EustressEngine").join("auth_token");
    let token = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// The API in use: `EUSTRESS_API_URL` when it is an accepted base, else
/// production (`eustress_common::api_base`).
fn api_base() -> String {
    eustress_common::api_base::api_base().to_string()
}

/// One request. `live`: a commerce request names its mode, and a player's
/// purchases are always live.
fn request(token: Option<&str>, live: Option<bool>, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value, String> {
    let base = api_base();
    let client = reqwest::blocking::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut request = client.request(method, format!("{base}{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(live) = live {
        request = request.header("Eustress-Mode", if live { "live" } else { "test" });
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().map_err(|e| format!("could not reach {base}: {e}"))?;
    let status = response.status();
    let reply: Value = response.json().unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(reply);
    }
    let message = reply.get("error").and_then(Value::as_str).unwrap_or("request failed");
    let code = reply.get("code").and_then(Value::as_str).unwrap_or("");
    Err(format!("{message} [{} {code}]", status.as_u16()))
}

fn spawn(inbox: Arc<Mutex<Vec<Reply>>>, job: impl FnOnce() -> Reply + Send + 'static) {
    let started = std::thread::Builder::new().name("eustress-player-commerce".into()).spawn(move || {
        let reply = job();
        inbox.lock().unwrap_or_else(|e| e.into_inner()).push(reply);
    });
    if let Err(e) = started {
        warn!("commerce: could not start a request thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_read_in_whole_tickets_with_grouped_thousands() {
        assert_eq!(tickets(1), "1 Ticket");
        assert_eq!(tickets(50), "50 Tickets");
        assert_eq!(tickets(1_250), "1,250 Tickets");
        assert_eq!(tickets(1_000_000), "1,000,000 Tickets");
    }

    #[test]
    fn it_buys_only_from_a_host_verified_as_the_listings_creator() {
        let listing = Listing { sim_id: "aaaaaaaa-1111".into(), name: "Sim".into(), author: "ada".into() };
        let t0 = Instant::now();
        assert_eq!(next_step(&HostCheck::Creator(listing.clone()), t0), NextStep::Ask(&listing));
        assert_eq!(next_step(&HostCheck::Waiting, t0), NextStep::Wait, "not joined yet");
        assert_eq!(next_step(&HostCheck::Checking(t0), t0 + HOST_CHECK_WAIT / 2), NextStep::Wait);
        assert!(
            matches!(next_step(&HostCheck::Checking(t0), t0 + HOST_CHECK_WAIT + Duration::from_secs(1)), NextStep::Refuse(_)),
            "a host that cannot be checked in time sells nothing"
        );
        assert_eq!(
            next_step(&HostCheck::Refused("the host is not this listing's creator".into()), t0),
            NextStep::Refuse("the host is not this listing's creator")
        );
    }

    #[test]
    fn a_host_is_checked_only_with_a_real_listing_id_a_ticket_and_a_pin() {
        let pin = [7u8; 32];
        let sim = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
        assert_eq!(host_to_check(Some(sim), Some("eit1.x.y"), Some(pin)), Ok((sim.to_string(), "eit1.x.y".to_string(), pin)));
        assert!(host_to_check(None, Some("eit1.x.y"), Some(pin)).is_err(), "no listing");
        assert!(host_to_check(Some(sim), None, Some(pin)).is_err(), "no ticket");
        assert!(host_to_check(Some(sim), Some("eit1.x.y"), None).is_err(), "no pin");
        // A listing id is interpolated into a URL path: nothing else passes.
        assert!(host_to_check(Some("../../api/keys"), Some("eit1.x.y"), Some(pin)).is_err());
        assert!(host_to_check(Some("aaaaaaaa/11"), Some("eit1.x.y"), Some(pin)).is_err());
        assert!(host_to_check(Some("abc"), Some("eit1.x.y"), Some(pin)).is_err(), "too short");
        assert!(host_to_check(Some(&"a".repeat(65)), Some("eit1.x.y"), Some(pin)).is_err(), "too long");
    }

    #[test]
    fn a_prompt_for_the_other_kind_of_product_is_refused() {
        for (expects, is_pass) in [(0, false), (0, true), (1, false), (2, true)] {
            assert_eq!(kind_refusal(expects, is_pass), None, "expects {expects}, pass {is_pass}");
        }
        assert!(kind_refusal(1, true).is_some(), "a product prompt for a pass");
        assert!(kind_refusal(2, false).is_some(), "a pass prompt for a product");
    }
}
