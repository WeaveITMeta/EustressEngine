# Commerce

Creators sell products inside the simulations they publish, and players pay in
Tickets. A product is either a **consumable** (coins, a boost: granted each time
it is bought) or a **pass** (VIP, a tool: owned once). Scripts use
`MarketplaceService` the way Roblox scripts do, in Luau or in Rune; the Commerce
API runs on Cloudflare; `eustress commerce` manages it all from a terminal the
way the Stripe CLI manages Stripe, and the website's Creator page does the same
in a browser.

Everything has a **test mode** and a **live mode**. Test purchases move no
Tickets, and test mode can shape draft products but never change what players
see or pay. Studio's own player only ever tests. Players who join a session
the creator hosts buy for real, with their own accounts.

---

## What a creator needs

- **A published Universe.** Products belong to its listing. The listing id is
  kept in `<Universe>/.eustress/sync.toml` (`remote.experience_id`), and every
  republish keeps it.
- **A published Space.** Each product is sold in one Space, which must be one
  of the Universe's published Spaces.
- **A listing approved in review**, for live sales. A draft can be tested
  before review; a quarantined or rejected listing sells nothing.

---

## Products

| Field | Rules |
|---|---|
| `name` | 1 to 60 characters |
| `price` | Whole Tickets, 1 to 1,000,000 |
| `type` | `consumable` (default) or `pass`; fixed once created |
| `space` | A published Space of the simulation |
| `number` | Given by the API, 1, 2, 3... per simulation: what scripts use |
| `description`, `icon`, `metadata` | Optional; up to 20 metadata pairs |
| `active` | `false` (a draft) until put on sale |

A simulation holds up to 500 products. A new product is a **draft**: it can be
tested in Studio but players cannot buy it. Putting a product on sale, or
changing or archiving one that is on sale, is a **live change**. Archiving
takes a product off sale; passes already bought stay owned.

---

## In a script

```lua
local MarketplaceService = game:GetService("MarketplaceService")
local Players = game:GetService("Players")

-- Grant what was bought. Return PurchaseGranted only once it is saved.
MarketplaceService.ProcessReceipt = function(receipt)
	local player = Players:GetPlayerByUserId(receipt.PlayerId)
	if not player then
		return Enum.ProductPurchaseDecision.NotProcessedYet
	end
	if receipt.ProductId == 1 then
		player.leaderstats.Coins.Value += 100
	end
	return Enum.ProductPurchaseDecision.PurchaseGranted
end

-- Somewhere a player asks to buy product 1:
MarketplaceService:PromptProductPurchase(player, 1)
```

| Member | What it does |
|---|---|
| `PromptProductPurchase(player, number)` | Buy a consumable |
| `PromptGamePassPurchase(player, number)` | Buy a pass |
| `PromptPurchase(player, number)` | Buy either |
| `ProcessReceipt` | Callback: grant a consumable, return a `ProductPurchaseDecision` |
| `GetProductInfo(number)` | `Name`, `Description`, `PriceInTickets` (also `PriceInRobux`), `ProductType`, `IsForSale`, `IconUrl`; errors for an unknown product |
| `UserOwnsGamePassAsync(userId, number)` | Whether the player owns the pass |
| `PlayerOwnsAsset(player, number)` | The same, by player |
| `PromptProductPurchaseFinished(userId, number, purchased)` | Signal |
| `PromptGamePassPurchaseFinished(player, number, purchased)` | Signal |
| `PromptPurchaseFinished(player, number, purchased)` | Signal |

`receipt` carries `PurchaseId`, `PlayerId`, `ProductId` (the number),
`CurrencySpent`, `CurrencyType` (`Enum.CurrencyType.Tickets`),
`SimulationId` (also as `PlaceIdWherePurchased`) and `Space`.

A receipt waits until a script sets `ProcessReceipt`, and each runs on its own
thread, so a callback may yield (to save to a DataStore, say). A receipt that
is not granted is offered again after the player's next purchase and in the
next session. Passes do not go through `ProcessReceipt`: owning one is the
grant, and `UserOwnsGamePassAsync` answers true from then on.

A `Prompt...PurchaseFinished` signal says `purchased` only for a purchase the
API holds: for a player on another machine, only once the host has checked the
purchase with the API (see [Players](#players)). A script can grant a pass's
perk on that signal, as Roblox's own samples do.

### In Studio

Nothing loads until a script uses `MarketplaceService`, so a session that sells
nothing makes no request. Studio's own player is the signed-in creator. Each
prompt opens a dialog in Play with the product, its kind, its price and a note
that this is a test purchase; **Buy** (or Enter) makes the test purchase,
**Cancel** (or Escape, or a click outside) closes the prompt unpurchased. The
dialog frees the cursor and holds the character still while it is up. A
headless engine has no dialog and buys at once.

Purchases still waiting from earlier sessions reach `ProcessReceipt` when the
script sets it. Without a published Universe or a sign-in, the Output says why
purchases are unavailable and every prompt closes unpurchased.

### In Rune

Rune scripts reach the same catalog and receipts through the `eustress` module.
Players are named by `UserId` and products by number, as in Luau.

```rune
use eustress::{marketplace_prompt_product_purchase, players_get_local_player};

pub fn on_update(dt) {
    // Somewhere a player asks to buy product 1:
    if let Some(player) = players_get_local_player() {
        marketplace_prompt_product_purchase(player.user_id, 1);
    }
}

// Grant what was bought: true once it is saved.
pub fn process_receipt(receipt) {
    if receipt.product_id == 1 {
        // add 100 coins for receipt.player_id
    }
    true
}
```

| Function | What it does |
|---|---|
| `marketplace_prompt_product_purchase(user_id, number)` | Buy a consumable; `true` when the prompt was queued |
| `marketplace_prompt_game_pass_purchase(user_id, number)` | Buy a pass |
| `marketplace_prompt_purchase(user_id, number)` | Buy either |
| `marketplace_get_product_info(number)` | `product_id`, `name`, `description`, `price_in_tickets`, `is_for_sale`, `product_type`; `None` while the catalog loads |
| `marketplace_player_owns_game_pass(user_id, number)` | Whether the player owns the pass |
| `marketplace_passes_pending(user_id)` | A player who just joined: its passes are still on their way |
| `marketplace_status()` | `"loading"`, `"ready"` or `"unavailable: <why>"` |
| `players_get_players()`, `players_get_player_by_user_id(user_id)`, `players_get_local_player()` | `user_id`, `name` and the `Player` `instance` (for `eustress::dm`) |

`pub fn process_receipt(receipt)` is an entry point, like `on_update`: the
engine calls it after `on_update` with a receipt carrying `purchase_id`,
`player_id`, `product_id`, `currency_spent`, `simulation_id` and `space`.
Returning `true` grants it; anything else, or an error, leaves it waiting.
A simulation has one receipt handler, as in Roblox: when a Luau script sets
`ProcessReceipt`, Luau answers and Rune's `process_receipt` is not called. When
several Rune scripts define it, the first loaded answers.

A Rune call cannot wait, so a script asks again on a later frame while
`marketplace_status()` is `"loading"` or `marketplace_passes_pending` is true.
Prompts count only in a Play frame (`on_update`, a button click); the editor's
script analyzer runs `on_update` for its checks and never prompts anyone. Rune
does not receive the `Prompt...Finished` signals: a consumable reaches
`process_receipt`, and a pass shows in `marketplace_player_owns_game_pass`.

---

## Players

A player's purchase is only as safe as the host that grants it: a host could
take the Tickets and never grant the purchase. So **only a host signed in as
the listing's creator sells to players who join it**, and each side proves who
it is with an **identity ticket**.

- **An identity ticket** is a short-lived statement from the API that an
  account is who it says, for one connection. `POST /api/identity/ticket
  {audience}` issues one to a signed-in account; its audience is the host's
  certificate pin as 64 hex characters, and it expires after 10 minutes.
  `POST /api/identity/verify {ticket, audience}` answers
  `{account_id, username, expires}` for anyone holding it.
- **The host** fetches its own ticket when it starts listening, and every
  Welcome carries it. It verifies each joining player's ticket the same way,
  and a verified player's `UserId` is derived from its account, so it is the
  same number every session. A player with no ticket, or one that fails,
  plays as a guest (`UserId` 2^52 and up) and is never offered anything.
- **The Player** signs in with the account Studio uses. It fetches its ticket
  before joining, and after joining checks the host's ticket: the account must
  be the listing's author. It buys from no other host.

When a signed-in player joins, the creator's host reads what that player owns
in the simulation and what it bought that no session has granted yet
(`GET players/{account_id}`). Its passes answer `UserOwnsGamePassAsync`, which
waits (up to 10 s) until they arrive, and its ungranted consumables go to
`ProcessReceipt`.

A prompt for a joined player travels to its Player, which shows the product,
its price and the player's balance in a native dialog, and buys with the
player's account, live. The Player then tells the host whether it bought and
sends the new purchase id. The host grants nothing on the Player's word: it
reads the purchase from the API (`GET receipts/{sim_id}/{purchase_id}`), takes
it only when its buyer is that player's verified account, and closes the
prompt as purchased only then. A claim with no purchase behind it closes
unpurchased after 30 seconds. A consumable that `ProcessReceipt` granted is
marked fulfilled, live, by the creator's host.

Solo play of a published simulation (`eustress-client --sim`) runs no scripts,
so nothing is sold there: scripts run on a host.

---

## On the website

- **Creator** (`/creator`): every simulation the account published and whether
  it sells; for each, its products (create a draft, edit, put on sale or take
  off sale), its sales (totals, what is still waiting for `ProcessReceipt`, and
  refunds), its events, and its webhook endpoints. A switch picks test or live
  mode, as `--live` does in the CLI; live changes are marked as such.
- **Passes on a listing's page**: the passes a simulation has on sale, with
  **Buy** for a signed-in player. A pass bought there is the player's in every
  session. Consumables are sold inside the simulation only, where its scripts
  grant them.
- **API keys** (Projects page): commerce keys in test or live mode, and revoke.

---

## The CLI

`login` mints a commerce API key from your Studio sign-in and stores it in
`<config>/eustress/cli.toml`. Commands run in test mode unless given `--live`,
which needs a live key (`login --live`). Inside a published Universe's folder,
`--sim` and `--space` default to that Universe and Space.

| Command | Does |
|---|---|
| `eustress commerce login [--live]` | Mint and store keys (`--api-key` stores one you have) |
| `eustress commerce logout` | Revoke and forget the stored keys |
| `eustress commerce whoami` | Account, mode, published simulations and their Spaces |
| `eustress commerce balance` | Your Ticket balance |
| `eustress commerce catalog` | What players see (drafts too, for the creator) |
| `eustress commerce products list\|create\|get\|update\|archive` | Manage products |
| `eustress commerce purchases list\|get\|refund\|fulfill` | Sales of your products |
| `eustress commerce purchases create --product 1` | A test purchase; Studio grants it next Play |
| `eustress commerce events list\|get\|resend` | Everything that happened |
| `eustress commerce listen --forward-to localhost:4242/hooks` | Stream events, signed, to a local server |
| `eustress commerce trigger purchase.succeeded` | An event with a made-up buyer; nothing moves |
| `eustress commerce webhooks list\|create\|delete` | Endpoints that receive events |
| `eustress commerce keys list\|create\|revoke` | API keys (needs the Studio sign-in) |

Every command takes `--json`. `EUSTRESS_API_URL` points the CLI at another API,
such as a local `wrangler dev` on `http://127.0.0.1:8787`; Studio and the
Player read the same variable.

A first product, end to end:

```text
eustress commerce login
cd MyUniverse/Spaces/Lobby
eustress commerce products create --name "100 Coins" --price 50
eustress commerce purchases create --product 1        # then Play in Studio
eustress commerce products update 1 --active true --live
```

---

## Events and webhooks

| Event | When |
|---|---|
| `product.created`, `product.updated`, `product.archived` | A product changed (visible in both modes) |
| `purchase.succeeded` | A purchase went through |
| `purchase.failed` | A purchase was refused (not enough Tickets, a pass already owned) |
| `purchase.fulfilled` | The simulation granted it |
| `purchase.refunded` | The creator refunded it |

Events are kept 30 days. A webhook endpoint is a public `https` URL on the
default port. Each delivery is a `POST` of the event with this header, the
scheme Stripe uses:

```text
Eustress-Signature: t=1790000000,v1=<hex HMAC-SHA256 of "1790000000.<raw body>" under the endpoint's whsec_ secret>
```

```js
import crypto from 'node:crypto';

function verify(rawBody, header, secret, toleranceS = 300) {
  const parts = Object.fromEntries(header.split(',').map((p) => p.split('=')));
  if (!(Math.abs(Date.now() / 1000 - Number(parts.t)) <= toleranceS)) return false;
  const expected = crypto.createHmac('sha256', secret).update(`${parts.t}.${rawBody}`).digest();
  const given = Buffer.from(parts.v1 || '', 'hex');
  return given.length === expected.length && crypto.timingSafeEqual(given, expected);
}
```

A delivery that does not get a 2xx is retried after 1 minute, 5 minutes,
30 minutes, 2, 5, 10 and 24 hours. `listen` signs what it forwards the same way
with a stable per-mode secret (`listen --print-secret`), so a local server
verifies exactly as it will in production.

---

## The API

Base URL `https://api.eustress.dev`; the commerce routes are under
`/api/commerce/`, identity tickets under `/api/identity/`.

**Authentication.** An API key, `ek_test_...` or `ek_live_...`, as a bearer
token: its prefix decides the mode, and it must be a `commerce` key. Or a
signed-in session, which is in test mode unless it sends `Eustress-Mode: live`.
Keys are created and revoked only from a session (`/api/keys`); a key never
mints another.

| Route | |
|---|---|
| `GET catalog/{sim_id}` | Public: products on sale (the creator also sees drafts) |
| `GET receipts/{sim_id}/{purchase_id}` | Public: one purchase, for a host checking a player's; the id is the capability, and reading is all it allows |
| `GET account` | The account, mode, and published simulations with Spaces |
| `GET/POST products`, `GET/POST/DELETE products/{id}` | Products (`DELETE` archives) |
| `POST purchases` | Buy: `{sim_id, product, expected_price, idempotency_key, space?}` |
| `GET purchases`, `GET purchases/{id}` | The creator's sales |
| `POST purchases/{id}/fulfill` | `{sim_id}`, by the buyer or the creator |
| `POST purchases/{id}/refund` | By the creator, within 72 hours |
| `GET players/{account_id}?sim_id` | The creator only: that player's passes and ungranted purchases in the creator's simulation |
| `GET me/balance`, `me/pending?sim_id`, `me/entitlements?sim_id` | The caller's own |
| `GET events`, `events/{id}`, `POST events/{id}/resend` | Events |
| `GET events/stream?after=&wait=&types=` | Long poll, up to 25 s, from a cursor |
| `GET/POST webhook_endpoints`, `DELETE webhook_endpoints/{id}` | Endpoints |
| `GET listen/secret` | The listen signing secret for the mode |
| `POST test_helpers/trigger` | Test mode: `{event, sim_id, product}` |
| `POST /api/identity/ticket` | A signed-in account's ticket: `{audience}` (64 hex) |
| `POST /api/identity/verify` | Public: `{ticket, audience}` to `{account_id, username, expires}` |

Ids carry a prefix (`prod_`, `pur_`, `evt_`, `we_`, `key_`). Lists are
`{object: "list", data, has_more}` and page with `starting_after`. Errors are
`{error, code, param?}`. A purchase needs an `idempotency_key`; the same key
returns the first result and charges once.

**Buying.** A live purchase is made by a signed-in player with
`Eustress-Mode: live`, never with an API key: in a host's session through the
Player, or a pass on the listing's page. A creator cannot buy their own
products live. The buyer sends the price they were shown (`expected_price`),
and a price that changed in the meantime is refused (`price_changed`), so
nobody pays a price they did not see. In test mode only the simulation's
creator can buy.

---

## Money

- A live sale debits the buyer the price and credits the creator 70% of it,
  rounded down; the rest is the platform's.
- The creator can refund a live sale for 72 hours: the buyer gets the Tickets
  back and the creator's share is taken back, even past zero.
- Each account's Tickets live in one Wallet, which every balance change goes
  through: Stripe credits, chargebacks, purchases and refunds. A wallet knows
  which of its Tickets were **paid** for with money and which were **earned**
  from sales, and spends earned ones first.
- A sale earns the creator **value score** (the Bliss contribution for sales)
  only a minute after its refund window closes, and only for the part the
  buyer paid with paid Tickets. Tickets passed back and forth between accounts
  therefore score nothing. Scores are filed by `creditValueScore` in
  `bliss.mjs`, and the Treasury's nightly settlement applies the per-buyer and
  per-creator daily caps when it sums the day.
- Buyers are adults: registration requires 18 or the local age of majority
  (see Child Safety in [CURRENCY.md](CURRENCY.md)). Admitting younger accounts
  would need an age gate on purchases first.
- A Stripe payment for Tickets that is refunded or disputed is reversed in two
  places: the Treasury gives back its share of the payment, and the buyer's
  wallet gives back the Tickets it bought, paid Tickets first, going negative
  if they were spent. A dispute decided in the platform's favour restores both.

---

## MCP

The MCP server's commerce tools put live access in the tool name, because the
capability map classifies tools by name. The set proposed by the MCP session:

| Tools | Class |
|---|---|
| `commerce_catalog`, `commerce_account`, `commerce_balance`, `commerce_entitlements`, `commerce_products_list`, `commerce_products_get`, `commerce_purchases_list`, `commerce_purchases_get`, `commerce_events_list`, `commerce_events_get`, `commerce_events_poll`, `commerce_keys_list`, `commerce_webhooks_list` | Read |
| `commerce_trigger`, `commerce_products_create`, `commerce_products_update`, `commerce_products_archive`, `commerce_purchases_fulfill` (test mode) | Write |
| `commerce_products_create_live`, `commerce_products_update_live`, `commerce_products_archive_live`, `commerce_purchases_refund_live`, `commerce_keys_create_live`, `commerce_webhooks_create`, `commerce_webhooks_delete`, `commerce_keys_revoke` | Destructive |

The Write tools are Write **because test mode cannot touch a product that is on
sale**: the Worker answers `livemode_required` (see Products). If that rule is
ever relaxed, those tools become changes to what players see and pay, and must
be reclassified as Destructive first. `commerce_trigger` and
`commerce_purchases_fulfill` rest the same way on two other rules: triggers run
in test mode only, and a test key fulfills only test purchases.

There is no tool to buy. The tools are not built yet; building them is waiting
on a decision.

---

## Where it lives

| Piece | Location |
|---|---|
| Commerce API, Wallet and CommerceHub Durable Objects | [`infrastructure/cloudflare/api/src/commerce.mjs`](../../infrastructure/cloudflare/api/src/commerce.mjs) |
| Identity tickets | [`infrastructure/cloudflare/api/src/identity.mjs`](../../infrastructure/cloudflare/api/src/identity.mjs) |
| Their tests | [`tests/commerce.test.mjs`](../../infrastructure/cloudflare/api/tests/commerce.test.mjs), [`tests/identity.test.mjs`](../../infrastructure/cloudflare/api/tests/identity.test.mjs) |
| Value score | [`infrastructure/cloudflare/api/src/bliss.mjs`](../../infrastructure/cloudflare/api/src/bliss.mjs) |
| Bindings and migration | [`infrastructure/cloudflare/api/wrangler.toml`](../../infrastructure/cloudflare/api/wrangler.toml) |
| CLI | [`eustress/crates/cli/src/commerce.rs`](../../eustress/crates/cli/src/commerce.rs) |
| Play queues | [`eustress/crates/common/src/datamodel/commerce.rs`](../../eustress/crates/common/src/datamodel/commerce.rs) |
| `MarketplaceService` in Luau | [`eustress/crates/common/src/luau/play/prelude.luau`](../../eustress/crates/common/src/luau/play/prelude.luau), [`mod.rs`](../../eustress/crates/common/src/luau/play/mod.rs) |
| `MarketplaceService` in Rune | [`eustress/crates/engine/src/soul/rune_ecs_module.rs`](../../eustress/crates/engine/src/soul/rune_ecs_module.rs), dispatched from [`soul/rune_play.rs`](../../eustress/crates/engine/src/soul/rune_play.rs) |
| Studio and host commerce | [`eustress/crates/engine/src/play_datamodel/commerce.rs`](../../eustress/crates/engine/src/play_datamodel/commerce.rs) |
| Joined players in the Play tree | [`eustress/crates/engine/src/play_datamodel/remote_players.rs`](../../eustress/crates/engine/src/play_datamodel/remote_players.rs) |
| Studio's purchase dialog | [`eustress/crates/engine/src/ui/purchase_prompt.rs`](../../eustress/crates/engine/src/ui/purchase_prompt.rs), [`ui/slint/purchase_prompt.slint`](../../eustress/crates/engine/ui/slint/purchase_prompt.slint) |
| The Player's purchases | [`eustress/crates/client/src/systems/commerce.rs`](../../eustress/crates/client/src/systems/commerce.rs) |
| Session messages (`PromptPurchase`, `ClosePurchase`, `SendReceipts`) | [`eustress/crates/common/eustress-networking/src/session.rs`](../../eustress/crates/common/eustress-networking/src/session.rs) |
| Website | [`web/src/pages/creator.rs`](../../eustress/crates/web/src/pages/creator.rs), [`components/pass_store.rs`](../../eustress/crates/web/src/components/pass_store.rs), [`api/commerce.rs`](../../eustress/crates/web/src/api/commerce.rs) |

Deploying the Worker creates the Wallet and CommerceHub classes through the
`v1-commerce` migration in `wrangler.toml`. A Worker running without those
bindings answers every commerce route with `503 commerce_unavailable`, and
without `JWT_SECRET` the identity routes answer `503 identity_unavailable`.

## Not built yet

- **The browser Player.** Its purchases follow the native Player's contract:
  an identity ticket bound to the host's pin before joining, the host checked
  against the listing's author, a live purchase with `expected_price`, then
  `ClosePurchase` and `SendReceipts`. Until it exists, a browser buys passes on
  the listing's page.
- **Selling in a session someone else hosts.** Only the listing's creator can
  host a session that sells; there is no way for a creator to vouch for another
  host.
- **A pass refunded mid-session** stays owned in that session; the next join
  reads it as refunded.
