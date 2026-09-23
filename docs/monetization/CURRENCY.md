# Eustress Currency System

**Two currencies, one rule: Bliss is earned, Tickets are bought, and they never convert.**

**Last Updated:** July 05, 2026
**Status:** LIVE (Bliss earning + daily distribution deployed to production)

> **This document was rewritten on 2026-07-05.** The previous version described
> Bliss as "arcade-style tokens purchased via Steam Wallet" with $0.99–$49.99
> Bliss packages. **That was never the design and no such purchase path exists
> or will exist.** Bliss cannot be bought — by anyone, at any price. The
> purchasable currency is **Tickets**. Any surviving reference to buying Bliss
> is stale and should be deleted on sight.

---

## Table of Contents

1. [The Two Currencies](#the-two-currencies)
2. [Bliss (BLS) — Earned](#bliss-bls--earned)
3. [Tickets (TKT) — Purchased](#tickets-tkt--purchased)
4. [The Treasury](#the-treasury)
5. [Spending and Receipts](#spending-and-receipts)
6. [Implementation Map](#implementation-map)
7. [Child Safety](#child-safety)

---

## The Two Currencies

| | **Bliss (BLS)** | **Tickets (TKT)** |
|---|---|---|
| How you get it | **Earned only** — verified contribution | **Bought only** — USD via Stripe |
| Can you buy it? | **No. Never.** | Yes |
| Can you earn it? | Yes | No |
| Converts to the other? | **No** | **No** |
| What it represents | Your share of value you created | Prepaid spending power |
| Backed by | Daily emission + USD treasury drip | The dollars you paid |

**Why the separation is absolute.** The moment Bliss can be purchased, it stops
being a record of contribution and becomes a leaderboard you can buy your way
onto — which is precisely the failure mode that ruins contribution economies.
Keeping the earn-rail and the spend-rail disjoint is the single most important
invariant in this system. Do not add a purchase path for BLS.

---

## Bliss (BLS) — Earned

### Properties

| Property | Value |
|---|---|
| Symbol | BLS |
| Decimals | 18 (on-chain target; the ledger currently stores whole-BLS f64) |
| Initial supply | 100,000,000 |
| Hard cap | None — tail emission |
| Consensus | Proof-of-Contribution |
| Purchasable | **No** |

### Emission schedule

Tail emission, ported from `bliss-core` 0.1.1 `economics.rs`: 5% of supply in
year one, halving every 4 years, with a permanent 0.5% floor so there is always
something to earn for someone who shows up in year 20.

| Period | Annual rate |
|---|---|
| Years 0–3 | 5.0% |
| Years 4–7 | 2.5% |
| Years 8–11 | 1.25% |
| Years 12–15 | 0.625% |
| Years 16+ | 0.5% (floor, forever) |

**100% of emission goes to contributors.** Zero to the platform, zero to
investors, zero to staking.

### How it is earned

Work in Studio is attributed to a contribution type, co-signed by the witness,
and scored as `weight × minutes × node_bonus`:

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

Full-node operators earn +10%. At UTC midnight the day's emission is split by
`your_score / total_score` and credited as BLS.

### Anti-abuse (enforced server-side)

The client self-reports its work, so the **witness is the only trust boundary**.
Four invariants, all enforced in `handleCosign`:

1. Unknown contribution types are rejected (no silent 1.0x fallback).
2. The Full-node bonus comes from server-**observed** heartbeat mode, never the
   request body.
3. `ActiveTime` cannot exceed server-observed presence (wall-clock bounded).
4. Per-account daily score is capped (`MAX_DAILY_SCORE`), bounding the blast
   radius of any forged claim.

A per-account **share cap was deliberately rejected** — it would strip earnings
from the single most productive contributor and cannot stop sybil anyway.
Sybil's real boundary is the KYC'd payout rail.

**Still open:** artifact attestation. The witness bounds *how much* can be
claimed but cannot yet verify a Development/Creation claim actually happened.
Until that lands, "proof" is doing more work in the name than in the system.

---

## Tickets (TKT) — Purchased

Bought with USD via Stripe. Spent on marketplace items, passes, cosmetics, and
API usage. **Tickets are never earned by contributing, and never convert to
BLS.**

| Package | USD | Tickets | Bonus |
|---|---|---|---|
| Starter | $4.99 | 400 | — |
| Standard | $9.99 | 880 | +10% |
| Mega | $19.99 | 1,840 | +15% |
| Super | $49.99 | 5,000 | +25% |
| Ultra | $99.99 | 10,800 | +35% |

**Revenue split:** 50% of every Ticket dollar goes into the Bliss treasury (which
pays contributors), 50% is platform revenue.

---

## The Treasury

USD, funded by the Ticket split plus direct investor deposits. It drips to
contributors daily by the same score share used for BLS emission.

- **Normal drip:** 0.276%/day of the remaining balance (exponential decay)
- **Scarcity mode:** activates when the balance falls below 15% of its
  high-water mark — drip halves to 0.136%/day and the top 25% of contributors
  get a 2x weight boost
- **High-water-mark decay:** 0.171%/day, so a one-time deposit cannot pin the
  system in scarcity forever
- **Never emptied:** exponential decay approaches zero asymptotically

**Investors receive no tokens.** Funding the treasury raises the drip for every
contributor; the return is ecosystem growth, not extraction.

---

## Spending and Receipts

Both currencies are spent through the Worker, and every spend writes a receipt
only the buyer can read. The receipts are what the spending card on a person's
own profile and the `/purchases` page show: the title, icon and amount of each
purchase, the simulation it happened in, and the creator it supported.

| Spend | Endpoint | Where it goes |
|---|---|---|
| Tickets | `POST /api/tickets/spend` | 70% to the creator, 30% to the platform |
| Bliss | `POST /api/ledger/spend` | Burned; nobody is paid |

Both accept the same attribution, all of it optional:

| Field | Meaning |
|---|---|
| `simulation_id` | The published simulation the purchase happened in. The server reads its author as the creator. |
| `developer_id` (Tickets), `creator_id` (Bliss) | The creator when there is no simulation. With a simulation it must be the author, or the spend is refused. |
| `title` | What was bought, as the buyer should read it. Defaults to `Product {product_id}` for Tickets and to the `purpose` for Bliss. |
| `icon` | An image Eustress serves: `https://*.eustress.dev/...` or `/assets/...`. Any other URL is dropped, because the buyer's browser would fetch it. |
| `product_id` | The engine's product id, a number or a short slug. Required for Tickets. |

What the Worker enforces:

- A Tickets `price` is a whole number.
- Attribution is resolved before any money moves. An unknown simulation or
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

**Not wired yet:** in-simulation purchasing. `MarketplaceService:PromptPurchase`
in Luau and Rune logs the request and calls neither endpoint, so until it does,
receipts come only from direct calls to the two spend routes.

---

## Implementation Map

| Piece | Location |
|---|---|
| Contribution tracking | [`engine/src/bliss_tracker.rs`](../../eustress/crates/engine/src/bliss_tracker.rs) |
| Node / co-sign client | [`crates/bliss/src/`](../../eustress/crates/bliss/src/) |
| Witness ledger + crons | [`infrastructure/cloudflare/api/src/index.js`](../../infrastructure/cloudflare/api/src/index.js) |
| Canonical economics | crates.io `bliss-core` 0.1.1 `economics.rs` |
| Public dashboard | [`web/src/pages/bliss.rs`](../../eustress/crates/web/src/pages/bliss.rs) |
| Spend receipts and purchase history | [`infrastructure/cloudflare/api/src/purchases.mjs`](../../infrastructure/cloudflare/api/src/purchases.mjs) |
| Purchases page, profile spending card | [`web/src/pages/purchases.rs`](../../eustress/crates/web/src/pages/purchases.rs), [`web/src/pages/profile.rs`](../../eustress/crates/web/src/pages/profile.rs) |

**Honest framing for external audiences:** this is currently an off-chain,
trust-based ledger running on a single Cloudflare Worker with KV storage. There
is no chain, no consensus, and no decentralization; "co-signing" is one server
signing a hash. Describe it as a working contribution economy — not as a
"revolutionary cryptocurrency."

---

## Child Safety

Ticket purchases follow the platform's age gating: purchases are blocked for
child accounts and require parental approval plus spending limits for teens.
Earning BLS has no purchase surface, so it carries no spend risk — but USD
payout requires KYC, which is age-gated at 18+.

---

## Contact

**Monetization:** monetization@eustress.dev
