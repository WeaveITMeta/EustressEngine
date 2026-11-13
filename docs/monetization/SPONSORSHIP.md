# Engine Sponsorships

**A sponsorship is a gift that pays for building the engine. It is not an investment and it is not a currency.**

**Status:** code in the tree; not deployed. Deploy and the first live payment are McKale's call.

---

## What it is

| | Gift to the Treasury (`/bliss`) | Engine sponsorship (`/sponsor`) |
|---|---|---|
| Who it pays | Contributors, through the daily drip | Eustress: build servers, hosting, the work of making the engine |
| Split | 100% to the Treasury | 100% to Eustress's side, after the payment fee |
| Once or monthly | Both | Both |
| What the giver receives | Nothing spendable | Nothing spendable; optionally their username on the sponsor list |

A sponsor receives no Tickets, no Delta, no Bliss, no share of the company and no claim on revenue or profit. This is what keeps a sponsorship a gift and keeps it outside securities law: there is no expectation of profit. Do not add a perk that pays the sponsor back in money or in anything that converts to money. Selling ownership is a different instrument with its own rules: see [REGULATION_A_TIER_2.md](../finance/REGULATION_A_TIER_2.md).

## Tiers

| Tier | Amount |
|---|---|
| Supporter | $10 |
| Builder | $50 |
| Champion | $250 |
| Founding Sponsor | $1,000 |
| Custom | $5 to $100,000 |

Each is offered once or monthly. Prices are sent to Stripe inline, so no Stripe product or price object has to be created in test or live mode.

## How the money is recorded

A paid sponsorship is a Treasury deposit of kind `engine_sponsor`: the fee is Stripe's web fee (2.9% + $0.30), the Treasury's share is zero, and the rest is the platform's share. On a $50 sponsorship that is $1.75 in fees and $48.25 to Eustress. The deposit is keyed by the Checkout Session (or the invoice, for a monthly renewal), so a webhook Stripe delivers twice counts once. A refund or dispute is reversed by the Treasury's ordinary path, by payment intent, and comes out of Eustress's side.

On the Economics page, a real period's statement shows the kind as "Engine sponsorships", and the month chart draws it as its own source. It flows whole into Eustress's side, so in a period with sponsorships Eustress's side is larger than half of net. The contributors' drip does not change: a sponsorship neither adds to the Treasury nor takes from it.

## The sponsor list

`GET /api/sponsors` returns the tiers and the sponsors who asked to be listed: username, the month they first sponsored, and whether they sponsor monthly. Amounts are never shown. Only a signed-in account can be listed, and the list shows the account's username, never text typed at checkout. A banned or deleted account drops off. `PUT /api/sponsor/listing {listed}` lets a sponsor show or hide their own name at any time.

A refund does not remove a name from the list; the sponsor or an administrator hides it.

## Before it goes live

1. Stripe webhook events are the ones already subscribed (`checkout.session.completed`, `checkout.session.async_payment_succeeded`, `invoice.paid`, `charge.refunded`, `charge.dispute.*`). No new event, product or price is needed.
2. Run one test-mode sponsorship on staging, once and monthly, and read the Economics page's Actual view.
3. Tax: a sponsorship is revenue to the company, not a charitable donation. The page must not call it tax-deductible.
4. `/sponsor` is not in `sitemap.xml` or the SEO descriptions yet. Add both together (a test holds them in step) when the page should be indexed.

## Implementation map

| Piece | Location |
|---|---|
| Checkout, settling, the sponsor list | [`infrastructure/cloudflare/api/src/sponsor.mjs`](../../infrastructure/cloudflare/api/src/sponsor.mjs) |
| Webhook hand-off, routes | [`infrastructure/cloudflare/api/src/index.js`](../../infrastructure/cloudflare/api/src/index.js) |
| Deposit kind | [`infrastructure/cloudflare/api/src/treasury.mjs`](../../infrastructure/cloudflare/api/src/treasury.mjs) (`DEPOSIT_KINDS`) |
| Tests | [`infrastructure/cloudflare/api/tests/sponsor.test.mjs`](../../infrastructure/cloudflare/api/tests/sponsor.test.mjs) |
| Page | [`eustress/crates/web/src/pages/sponsor.rs`](../../eustress/crates/web/src/pages/sponsor.rs) |
