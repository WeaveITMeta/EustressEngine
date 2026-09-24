# Eustress Currency System

**Two currencies, one rule: Bliss is earned, Tickets are bought, and they never convert.**

**Last Updated:** September 23, 2026
**Status:** LIVE (Bliss earning and the nightly settlement run in production)

Bliss cannot be bought, by anyone, at any price. The purchasable currency is
Tickets.

---

## Table of Contents

1. [The Two Currencies](#the-two-currencies)
2. [Bliss (BLS): Earned](#bliss-bls-earned)
3. [Tickets (TKT): Purchased](#tickets-tkt-purchased)
4. [The Treasury](#the-treasury)
5. [Spending and Receipts](#spending-and-receipts)
6. [Implementation Map](#implementation-map)
7. [Child Safety](#child-safety)

---

## The Two Currencies

| | **Bliss (BLS)** | **Tickets (TKT)** |
|---|---|---|
| How you get it | **Earned only**, by contribution the witness co-signs | **Bought only**, with USD |
| Can you buy it? | **No. Never.** | Yes |
| Can you earn it? | Yes | No |
| Converts to the other? | **No** | **No** |
| What it represents | Your share of value you created | Prepaid spending power |
| Backed by | The daily emission, with the USD treasury drip paid beside it | The dollars you paid |

**Why the separation is absolute.** The moment Bliss can be purchased, it stops
being a record of contribution and becomes a leaderboard you can buy your way
onto, which is the failure mode that ruins contribution economies. Keeping the
earn-rail and the spend-rail disjoint is the single most important invariant in
this system. Do not add a purchase path for BLS.

---

## Bliss (BLS): Earned

### Properties

| Property | Value |
|---|---|
| Symbol | BLS |
| Decimals | 2 (1 BLS = 100 minor units; every stored amount is a whole number of them) |
| Initial supply | 100,000,000 |
| Hard cap | None: tail emission |
| Consensus | Proof-of-Contribution |
| Purchasable | **No** |
| Transferable between accounts | No. The only way BLS leaves an account is a spend, which burns it. |

The ledger is append-only: every credit and spend is an entry, and a balance
is the sum of an account's entries, so any balance can be rebuilt and audited
entry by entry. The ledger is public: `/api/ledger/summary`,
`/api/ledger/distribution/{date}`, `/api/ledger/history/{account}` and
`/api/ledger/leaderboard` need no sign-in.

### Emission schedule

Tail emission, ported from `bliss-core` 0.1.1 `economics.rs`: 5% of supply in
year one, halving every 4 years, with a permanent 0.5% floor so there is always
something to earn for someone who shows up in year 20.

| Period | Annual rate |
|---|---|
| Years 0 to 3 | 5.0% |
| Years 4 to 7 | 2.5% |
| Years 8 to 11 | 1.25% |
| Years 12 to 15 | 0.625% |
| Years 16+ | 0.5% (floor, forever) |

**100% of emission goes to contributors.** Zero to the platform, zero to
funders, zero to staking.

The yearly rate sets each day's **ceiling**: supply times the rate, divided by
365. A day mints `ceiling x min(1, day score / 1,440)`, and the rest of the
ceiling is never created. 1,440 is a full day of work: 8 hours at the top
weight. A day with less work than that mints proportionally less, so supply
follows contribution rather than the calendar, and one person alone on a quiet
day earns for the minutes they worked, not the whole day's emission.

### How it is earned

Work in Studio is attributed to a contribution type, co-signed by the witness,
and scored as `weight x minutes x node_bonus`:

| Type | Weight | Signal |
|---|---|---|
| Development | 3.0x | Script editing |
| Creation | 2.5x | Undoable scene edits |
| Education | 2.2x | Teaching / tutorials |
| Collaboration | 2.0x | Review, pairing |
| Optimization | 2.0x | Performance work |
| QualityAssurance | 1.8x | Testing, repro'd bug reports |
| Moderation | 1.5x | Community safety |
| Documentation | 1.5x | Docs, guides, translation |
| ActiveTime | 1.0x | Focused, input-active session time |

Full-node operators earn +10%.

**Value score.** A creator also scores for what people buy from them: 0.5 per
Ticket of the creator's share of a sale. Only Tickets the buyer bought with
money count (Tickets earned from sales and spent again score nothing), a sale
scores only once its 72-hour refund window has closed, and a purchase from
your own account never scores. When a day is summed, one buyer counts toward
one creator for at most 1,000 of the creator's Tickets, and a creator's value
score for the day is at most 9,600.

After UTC midnight the Treasury settles the day before: it sums each account's
effort and value score, credits each account its share of the day's mint
(`your_score / total_score`), and records the day at
`/api/ledger/distribution/{date}`. Each credit has a fixed key, so a
settlement that stops part way resumes without crediting anyone twice.

### Anti-abuse (enforced server-side)

The client self-reports its work, so the **witness is the only trust boundary**.
Four invariants, all enforced in `handleCosign`:

1. Unknown contribution types are rejected (no silent 1.0x fallback).
2. The Full-node bonus comes from server-**observed** heartbeat mode, never the
   request body.
3. `ActiveTime` cannot exceed server-observed presence (wall-clock bounded).
4. Per-account daily score is capped (`MAX_DAILY_SCORE`, 3,200), bounding the
   blast radius of any forged claim.

Each claim's hash is credited once: the day's record holds the hashes it
credited, written in the same write as the score. Claims are rate-limited per
account.

A per-account **share cap was deliberately rejected**: it would strip earnings
from the single most productive contributor and cannot stop sybil anyway.
Sybil's real boundary is the KYC'd payout rail.

**Still open:** artifact attestation. The witness bounds *how much* can be
claimed but cannot yet verify a Development/Creation claim actually happened.
Until that lands, "proof" is doing more work in the name than in the system.

---

## Tickets (TKT): Purchased

Bought with USD via Stripe. Spent on products creators sell in their
simulations. **Tickets are never earned by contributing, and never convert to
BLS.**

| Package | USD | Tickets | Bonus |
|---|---|---|---|
| Starter | $4.99 | 400 | none |
| Standard | $9.99 | 880 | +10% |
| Mega | $19.99 | 1,840 | +15% |
| Super | $49.99 | 5,000 | +25% |
| Ultra | $99.99 | 10,800 | +35% |

**Revenue split.** The storefront's fee comes off the top (Stripe's 2.9% +
$0.30 on the web; 30% through an app store), and the net is split in half:
50% into the Bliss treasury, which pays contributors, and 50% platform
revenue. On a $9.99 web purchase that is $4.70 to the treasury; through a 30%
app store, $3.49.

**Refunds and chargebacks.** Tickets are credited only once a payment has
arrived. If a payment is later refunded or disputed, the treasury gives back
its share and the buyer's balance gives back the Tickets that payment bought,
going negative if they were already spent. A dispute decided in the
platform's favour restores both.

---

## The Treasury

USD, held in integer cents, funded by the Ticket split plus direct
contributions to the treasury. Every deposit, refund, dispute and payout is
recorded once, keyed by where it came from, so a payment Stripe reports twice
counts once. It pays contributors daily by the same score share used for BLS
emission.

- **Normal drip:** 0.276%/day of the remaining balance (exponential decay)
- **Scarcity mode:** activates when the balance falls to 15% of its high-water
  mark or below: the drip slows to 0.136%/day and the top 25% of contributors
  get a 2x weight boost
- **High-water-mark decay:** 0.171%/day, so a one-time deposit cannot pin the
  system in scarcity forever
- **Effort-gated:** a day with less than a full day of work (1,440 score) pays
  that fraction of the drip, the same rule as the emission; the rest stays in
  the treasury
- **Paid to connected accounts:** shares are figured across every contributor
  in good standing, and paid by Stripe transfer to those with a verified
  (KYC'd, 18+) Stripe account. A share owed to someone without one, or under
  Stripe's $0.50 minimum, stays in the treasury; it is not handed to others.
- **Never emptied:** exponential decay approaches zero asymptotically

**Funders receive no tokens.** Contributing to the treasury earns no BLS; it
raises the drip for every contributor, and the return is ecosystem growth,
not extraction.

---

## Spending and Receipts

Both currencies are spent through the Worker, and every spend writes a receipt
only the buyer can read. The receipts are what the spending card on a person's
own profile and the `/purchases` page show: the title, icon and amount of each
purchase, the simulation it happened in, and the creator it supported.

| Spend | Endpoint | Where it goes |
|---|---|---|
| Tickets | `POST /api/commerce/purchases` | 70% to the creator, 30% to the platform |
| Bliss | `POST /api/ledger/spend` | Burned; nobody is paid |

Tickets are spent on a product a creator listed in a published simulation, at
the price the product carries. The Commerce API, its Ticket wallets and its
rules are in [COMMERCE.md](COMMERCE.md).

A Bliss spend accepts attribution, all of it optional:

| Field | Meaning |
|---|---|
| `simulation_id` | The published simulation the purchase happened in. The server reads its author as the creator. |
| `creator_id` | The creator when there is no simulation. With a simulation it must be the author, or the spend is refused. |
| `title` | What was bought, as the buyer should read it. Defaults to the `purpose`. |
| `icon` | An image Eustress serves: `https://*.eustress.dev/...` or `/assets/...`. Any other URL is dropped, because the buyer's browser would fetch it. |
| `product_id` | The engine's product id, a number or a short slug. |

What the Worker enforces:

- Attribution is resolved before anything is spent. An unknown simulation or
  creator (404), or a creator who did not publish the simulation (400),
  refuses the spend.
- A Bliss spend that carries a `ref` spends once: a later call with the same
  `ref` gets 409. Two calls racing within the same instant can still both
  land, because KV has no compare-and-set.

Receipts live in the `INVENTORY` namespace as
`purchase:{buyer}:{inverted ms}:{id}`, with the display fields in KV metadata so
one `list()` reads a thousand of them, newest first. Two routes read them, both
for the bearer's own account and neither taking an account id:

- `GET /api/purchases` returns every receipt with totals per creator and per
  simulation.
- `GET /api/purchases/summary` returns the totals alone.

Tickets and Bliss are never added together. Every total carries both.

In a simulation, Tickets are spent through `MarketplaceService` in Luau, which
Studio runs against the Commerce API in test mode (see COMMERCE.md for what the
Player still needs before players can buy for real).

---

## Implementation Map

| Piece | Location |
|---|---|
| Contribution tracking | [`engine/src/bliss_tracker.rs`](../../eustress/crates/engine/src/bliss_tracker.rs) |
| Node / co-sign client | [`crates/bliss/src/`](../../eustress/crates/bliss/src/) |
| Witness routes: cosign, heartbeat, Stripe webhook, ledger reads | [`infrastructure/cloudflare/api/src/index.js`](../../infrastructure/cloudflare/api/src/index.js) |
| BLS ledger, emission schedule, value score | [`infrastructure/cloudflare/api/src/bliss.mjs`](../../infrastructure/cloudflare/api/src/bliss.mjs) |
| Treasury and the nightly settlement | [`infrastructure/cloudflare/api/src/treasury.mjs`](../../infrastructure/cloudflare/api/src/treasury.mjs) |
| Canonical economics | crates.io `bliss-core` 0.1.1 `economics.rs` |
| Public dashboard | [`web/src/pages/bliss.rs`](../../eustress/crates/web/src/pages/bliss.rs) |
| Spend receipts and purchase history | [`infrastructure/cloudflare/api/src/purchases.mjs`](../../infrastructure/cloudflare/api/src/purchases.mjs) |
| Commerce: products, purchases, Ticket wallets | [`infrastructure/cloudflare/api/src/commerce.mjs`](../../infrastructure/cloudflare/api/src/commerce.mjs), [COMMERCE.md](COMMERCE.md) |
| Purchases page, profile spending card | [`web/src/pages/purchases.rs`](../../eustress/crates/web/src/pages/purchases.rs), [`web/src/pages/profile.rs`](../../eustress/crates/web/src/pages/profile.rs) |

**Honest framing for external audiences:** this is currently an off-chain,
trust-based ledger running on a single Cloudflare Worker with KV and Durable
Object storage. There is no chain, no consensus, and no decentralization;
"co-signing" is one server signing a hash. Describe it as a working
contribution economy, not as a "revolutionary cryptocurrency."

---

## Child Safety

An account can only be created at 18 or older (or the local age of majority,
where that is higher), so every account that can buy Tickets is an adult.
Earning BLS has no purchase surface, so it carries no spend risk, and a USD
payout additionally requires KYC with the age read from the identity
document.

---

## Contact

**Monetization:** monetization@eustress.dev
