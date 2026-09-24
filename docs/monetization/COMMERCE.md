# Commerce

Creators sell products inside the simulations they publish, and players pay in
Tickets. A product is either a **consumable** (coins, a boost: granted each time
it is bought) or a **pass** (VIP, a tool: owned once). Scripts use
`MarketplaceService` the way Roblox scripts do, the Commerce API runs on
Cloudflare, and `eustress commerce` manages it all from a terminal the way the
Stripe CLI manages Stripe.

Everything has a **test mode** and a **live mode**. Test purchases move no
Tickets, and test mode can shape draft products but never change what players
see or pay. Studio only ever tests.

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

### In Studio

Nothing loads until a script uses `MarketplaceService`, so a session that sells
nothing makes no request. Studio buys as the signed-in creator, in test mode,
without a dialog; the Output names each test purchase. Purchases still waiting
from earlier sessions reach `ProcessReceipt` when the script sets it. Without a
published Universe or a sign-in, the Output says why purchases are unavailable
and every prompt closes unpurchased.

Rune scripts do not have `MarketplaceService`.

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
such as a local `wrangler dev` on `http://127.0.0.1:8787`; Studio reads the
same variable.

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

Base URL `https://api.eustress.dev`, every route under `/api/commerce/`.

**Authentication.** An API key, `ek_test_...` or `ek_live_...`, as a bearer
token: its prefix decides the mode, and it must be a `commerce` key. Or a
signed-in session, which is in test mode unless it sends `Eustress-Mode: live`.
Keys are created and revoked only from a session (`/api/keys`); a key never
mints another.

| Route | |
|---|---|
| `GET catalog/{sim_id}` | Public: products on sale (the creator also sees drafts) |
| `GET account` | The account, mode, and published simulations with Spaces |
| `GET/POST products`, `GET/POST/DELETE products/{id}` | Products (`DELETE` archives) |
| `POST purchases` | Buy: `{sim_id, product, expected_price, idempotency_key, space?}` |
| `GET purchases`, `GET purchases/{id}` | The creator's sales |
| `POST purchases/{id}/fulfill` | `{sim_id}`, by the buyer or the creator |
| `POST purchases/{id}/refund` | By the creator, within 72 hours |
| `GET me/balance`, `me/pending?sim_id`, `me/entitlements?sim_id` | The caller's own |
| `GET events`, `events/{id}`, `POST events/{id}/resend` | Events |
| `GET events/stream?after=&wait=&types=` | Long poll, up to 25 s, from a cursor |
| `GET/POST webhook_endpoints`, `DELETE webhook_endpoints/{id}` | Endpoints |
| `GET listen/secret` | The listen signing secret for the mode |
| `POST test_helpers/trigger` | Test mode: `{event, sim_id, product}` |

Ids carry a prefix (`prod_`, `pur_`, `evt_`, `we_`, `key_`). Lists are
`{object: "list", data, has_more}` and page with `starting_after`. Errors are
`{error, code, param?}`. A purchase needs an `idempotency_key`; the same key
returns the first result and charges once.

**Buying.** A live purchase is made by a signed-in player with
`Eustress-Mode: live`, never with an API key, and a creator cannot buy their
own products live. The buyer sends the price they were shown
(`expected_price`), and a price that changed in the meantime is refused
(`price_changed`), so nobody pays a price they did not see. In test mode only
the simulation's creator can buy.

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
| Its tests | [`infrastructure/cloudflare/api/tests/commerce.test.mjs`](../../infrastructure/cloudflare/api/tests/commerce.test.mjs) |
| Value score | [`infrastructure/cloudflare/api/src/bliss.mjs`](../../infrastructure/cloudflare/api/src/bliss.mjs) |
| Bindings and migration | [`infrastructure/cloudflare/api/wrangler.toml`](../../infrastructure/cloudflare/api/wrangler.toml) |
| CLI | [`eustress/crates/cli/src/commerce.rs`](../../eustress/crates/cli/src/commerce.rs) |
| Play queues | [`eustress/crates/common/src/datamodel/commerce.rs`](../../eustress/crates/common/src/datamodel/commerce.rs) |
| `MarketplaceService` in Luau | [`eustress/crates/common/src/luau/play/prelude.luau`](../../eustress/crates/common/src/luau/play/prelude.luau), [`mod.rs`](../../eustress/crates/common/src/luau/play/mod.rs) |
| Studio's commerce client | [`eustress/crates/engine/src/play_datamodel/commerce.rs`](../../eustress/crates/engine/src/play_datamodel/commerce.rs) |

Deploying the Worker creates the Wallet and CommerceHub classes through the
`v1-commerce` migration in `wrangler.toml`. A Worker running without those
bindings answers every commerce route with `503 commerce_unavailable`.

## Not built yet

- **Live purchases in the Player.** The Player has to show the product and its
  price, ask the player, and buy with `Eustress-Mode: live`. Until it does,
  players cannot buy.
- **A confirmation dialog in Studio.** Test purchases go through directly.
- **Rune.** Its `MarketplaceService` functions are not connected.
- **Creator views on the website.** Products and sales are managed from the
  CLI; the website's key list shows commerce keys.
