# Creator Cash-Out: one program, two itemized lines

Status: **DRAFT for McKale's approval. Nothing is built. No rate, term or "worth" wording is public until counsel has seen it.**
Drafted by the Website session on 2026-10-06 from McKale's decisions that day, checked against the code, and reviewed by an Opus
advisor. Code owners: `treasury.mjs` and `bliss.mjs` (the Tickets, Bliss & Delta session), `commerce.mjs` (the CLI session).

## What McKale decided (2026-10-06)

1. A Ticket sale pays the storefront or app-store fee off the top, then splits the net 50% into the Bliss treasury and 50% to Eustress.
2. A Ticket spent in a simulation pays the creator 70%; the other 30% leaves circulation.
3. Tickets are backed by **15% of Eustress's half of each Ticket sale**, taken at the point of sale.
4. Creators cash out through a **cash-out program**; Tickets and Bliss revenue are **itemized separately inside the same program**.
5. The Ticket Sales Reserve is also funded from the **Tickets share of plan invoices** (Player Plus and Bundle), not only from Ticket packs.
6. Taxes come **after operating expenses** (Engine Expenses). Whether the Treasury allocation and the reserve count as expenses is open:
   an accountant decides the timing.
7. **Bliss Treasury investments** are modeled as outside money that enters the Treasury and can be repaid, like stock. This is not a
   build decision: securities counsel comes before anyone is offered one.

## How money moves today (verified in the code)

| Step | What happens | On a $9.99 web pack (880 Tickets) |
|---|---|---|
| Buy | Stripe 2.9% + $0.30 comes off the top (30% through an app store, modeled but no app-store purchase flow exists) | fee $0.59, net $9.40 |
| Split | `splitSale`: half of the net to the Bliss treasury, half to Eustress | $4.70 and $4.70 (through a 30% app store: $3.49 and $3.50) |
| Spend | The buyer pays P Tickets, earned ones first. The creator is credited floor(0.70 x P) as **earned** Tickets. The other 30% is credited to no one. | 880 spent: 616 earned by the creator, 264 gone |
| Score | After the 72 hour refund window, only the part paid with bought Tickets scores: floor(paid x 0.70), capped per buyer, creator and day. The score shares the daily Bliss emission and the USD drip. | |
| Cash out | None. Earned Tickets can only be spent again. A creator's USD arrives only through the Bliss drip. | |

The fee is already paid at the point of sale, before the split, so both halves bear it in proportion. A Ticket sold through an app
store should carry fewer Tickets per dollar, so the net per Ticket matches the web; that pricing is not built.

## Where the money sits: four Mercury accounts (McKale, 2026-10-06)

| Account | Receives | Pays |
|---|---|---|
| **Bliss Treasury** | half of every net sale (Tickets, Delta packs, plans), plus direct treasury gifts in full, plus any investments (repayable; see below) | contributors' daily USD drip (the Bliss line), and repayments to investors |
| **Ticket Sales Reserve** | 15% of Eustress's half of each **Ticket** sale (7.5% of net), including the Tickets share of plan invoices | creators' Tickets cash-out (the Tickets line) |
| **Engine Expenses** | what the engine costs each month, out of Eustress's half after the reserve | servers, Cloudflare, refunds, Stripe's fee on treasury gifts, and the rest of running the engine |
| **Taxes** | a set-aside from the operating profit left after Engine Expenses (the rate is a placeholder for the accountant) | tax payments |

The chart that shows this with sliders is `eustress_money_flow` (version 2 went to McKale on 2026-10-06: the company month
with taxes after Engine Expenses, repayable Treasury investments, the plan-invoice reserve, and a one-creator earnings view; HTML and
PNG). Two rules the accounts force, both open: **who pays a refund** (the code takes it back from both halves in proportion; McKale
described refunds as an Engine Expenses cost), and **who pays Stripe's fee on a treasury gift** (the treasury is credited the whole
gift, so the fee has to come from Engine Expenses).

**The Economics page (2026-10-06).** The site has a model for testing at `/economics` (`pages/economics.rs`, `economics_model.rs`,
`economics_chart.rs` and `economics_kit.rs` in `crates/web`): a month of receipts, the fee, the 50/50 split, the Treasury's daily payout,
Bliss for verified contributors and what a creator earns from a sale, each with sliders. It carries only rules the Worker applies today: no
reserve, no cash-out, no Treasury investment, no cost and no tax. It is unlisted (not in the nav, the footer, the sitemap or llms.txt),
`_headers` tells search engines to skip it, and a test fails if a listing appears. The full version of this draft, with everything this
document proposes, is `economics_draft*.rs` at `/economics-draft`, compiled only with `--features economics-draft`, so no shipped bundle
holds it. Nothing from the draft goes on a public page until counsel has seen the wording.
Since 2026-10-07 the page has two modes. Theoretical is the model; Actual reads the platform (`/api/economics/actual`, `economics.mjs` in the Worker): the Treasury, its daily payouts and Bliss for everyone, and for an administrator only the receipts by kind (from the Treasury's month totals), the cost ledger entered from the bills, and labor as a share of Eustress's half. Quarterly statements download as CSV. No server cost is measured yet; Theoretical costs follow docs/architecture/HOSTING_COST_PLAN.md.

## The program

One enrollment, one statement, one payout account. Two lines that never mix.

| | **Bliss line** | **Tickets line** |
|---|---|---|
| Paid from | the Bliss treasury (half of every Ticket sale's net, plus direct contributions) | the **Ticket reserve** (15% of Eustress's half of each Ticket sale and of the Tickets share of plan invoices, so 7.5% of that net) |
| What is paid | the creator's share of the day's drip, by score (as built) | eligible earned Tickets x the published rate |
| Rate | the drip formula (as built) | start at **$1.50 per 1,000 Tickets**, and only ever raise it |
| Hold | none (as built) | 120 days from the sale (covers a card dispute on the Ticket purchase) |
| Cadence | daily (as built) | monthly, by the 15th, minimum $25 |
| Short of money | stays in the treasury (as built) | every claim is paid pro rata and the rest carries to the next cycle |

Shared by both lines: the enrollment gate (a Creator, Professional or Bundle plan, the Stripe Connect identity check, a W-9), one
payout account per person, the payout-bank lock after a key replacement, one rule for related-party sales, one reversal path and
one fraud graph. Each month's **statement** lists both lines and any adjustment under the line it came from. A negative Tickets
balance (a chargeback after cash-out) is paid down by future Tickets credit and never taken from the Bliss line. **Tickets never
convert to Bliss and Bliss never converts to Tickets**: the program pays USD out of each pool and converts nothing.

*Illustration, not a quote:* 12,000 eligible Tickets x $1.50 per 1,000 is $18.00, under the $25 minimum, so it carries to the next
cycle; the Bliss line for the same month is listed beside it and was already paid daily.

## The Tickets line, in detail

- **Eligible:** Tickets a creator earned from a sale that a buyer paid for with **bought** Tickets, after the hold, while the creator
  is enrolled. These are the `scoringTickets` amounts the value score already uses, so earned-first spending and circular trades
  between alternate accounts never create a claim. A creator whose plan lapses keeps the credit already earned.
- **Redeemable balance:** the credit sits in its own balance that spending never draws from. The creator may move it to spendable
  Tickets, one way only, so shopping with earnings cannot silently use up the cash-out.
- **Web Tickets only.** App-store Tickets are not eligible.
- **Reserve:** `splitSale` splits Eustress's half 85/15; the 15% goes to a separate account Eustress owns, funded from Ticket
  purchases and from the Tickets share of plan invoices (see "Abuse and failure"). The Bliss treasury stays exactly 50% of net. A
  refund or dispute takes back the reserve's share the way it takes back the treasury's. The reserve never goes negative, never
  borrows from the treasury, never returns to operating money; a surplus raises the rate. Stripe's payout fees and losses after a
  payout come out of operating revenue.

## What the money buys, in numbers

At 15%, the reserve is $0.70 on a $9.99 web pack (7.0% of the sale). Per Ticket sold: Starter $0.00085, Standard $0.00080, Mega
$0.00078, Super $0.00072, Ultra $0.00067. If a share **s** of Tickets sold is spent on creator products and a share **c** of what
creators earn is cashed out, the reserve carries a rate of 0.0008 / (0.7 x s x c) per Ticket:

| s x c | rate the reserve carries | cash per $1 a buyer spent | |
|---|---|---|---|
| 1.0 (everything spent, everything cashed out) | $0.0011 | 7 cents | the floor |
| 0.76 | **$0.0015** | **9 cents** | the recommended start (13% of the Standard price per Ticket) |
| 0.33 | $0.0035 | 22 cents | about a fifth of every dollar spent |

So with a 15% reserve, a creator who cashes out everything gets about **9 cents in cash for each dollar a buyer spends**; the rest of
the 70% is spending power inside Eustress. Paying 22 cents on the dollar at a realistic s x c of 0.6 would take about 28% of
Eustress's half, not 15%.

## Why not the alternatives

- **A fixed rate with no pro-rata guard** becomes an unfunded promise once s x c climbs.
- **A floating rate** gives windfall rates to the first few claims, which rewards self-dealing, and a rate tied to sales reads like an
  investment.
- **A separate second program** doubles the enrollment, the identity checks and the tax records for the same person.

## Abuse and failure

- **Self-dealing loses money:** cashing out returns 7 to 11% of what the Tickets cost. Related-party sales (shared card, bank,
  device or identity) earn no credit and no score.
- **Stolen cards** are the real risk: fraudsters buy Tickets, spend them with an accomplice creator, and the disputes follow. Guards:
  the 120 day hold, cancelling a buyer's newest pending credit on a refund or dispute, 3-D Secure on Ticket packs, and an automatic
  pause on payouts if disputes reach 0.5%.
- **Today a dispute on a Ticket pack reverses neither the creator's Tickets nor the score already filed.** Cash-out must not inherit
  that gap; it is a build item before any credit is real.
- **Tickets that a plan grants are marked paid.** Player Plus (450 a month) and Bundle (400) credit their Tickets as paid, so a
  creator who sells to them earns redeemable credit. **Decided 2026-10-06: the reserve is funded from the Tickets share of those
  invoices.** The share needs a yardstick price per Ticket. The chart uses the cheapest pack ($0.00926 in the Ultra pack), which makes
  Player Plus's 450 Tickets 83% of its $4.99 invoice and Bundle's 400 Tickets 29% of its $12.99. At the Standard pack price
  ($0.01135) Player Plus would be 102% of its invoice, so its share caps at 100%. The yardstick is an allocation that an accountant
  confirms. Build item: `splitSale` for a `subscription` carves the reserve from that share, with the same reversals as a pack.
- **The rate outruns the reserve:** start low, only raise it, publish coverage monthly; if three cycles in a row fall short, raise the
  funding share.

## Treasury investments and taxes (McKale, 2026-10-06; both open)

**Treasury investments.** McKale: treat them like stocks that can be repaid. The chart models outside money that enters the Bliss
Treasury as capital, not revenue, with a monthly repayment of principal and an optional return, both paid out of the Treasury. No
payment fee is modeled; a card fee would come from Engine Expenses. A repayment comes out of what contributors would have been paid,
dollar for dollar, and a return makes it cost more. With no ring-fenced account, the 0.276% daily drip would pass investors'
principal to contributors within months. Nothing is built and nothing is offered on any page. **Securities counsel comes before any
offer:** money pooled from the public, repayable, with any return that comes from Eustress's work, is the shape securities law
regulates, and a repayable deposit with no return is still a loan from the public. The `treasury_fund` gift stays a gift: no
return and no repayment.

**Taxes come after operating expenses.** In the chart, gross receipts less payment fees is net; half of net is the Treasury and the
other half is Eustress's; the reserve and Engine Expenses come out of Eustress's half; what remains is operating profit, and tax is
charged on that. Is the Treasury an expense? The Treasury allocation is the strongest case for one: a fixed share of revenue owed to
contributors. The reserve is the weaker case: money set aside for payments that depend on what creators sell and cash out, so expect
it to be taxed until it pays out. On the default month that is $9,064 of tax if both are deducted when funded and $11,094 if only the
payout is deducted (the $8,121 reserve stays taxable until a creator cashes out). An accountant decides the timing and method, and
what the Taxes account holds (entity tax, or distributions for the members' tax). Investor principal and its repayment are neither
income nor expense.

## Counsel, and what may be said

Facts for counsel: Tickets are closed-loop; buyers can never cash out; only creators can, one way, and receive about a tenth of what
the buyer paid; payouts run on Stripe Connect; the reserve is Eustress's own money. Questions: does this make Eustress a money
transmitter, a stored-value issuer or a prepaid-access provider, or do the closed-loop, agent-of-payee or processor exemptions
apply; should Eustress be the seller of record paying creators as licensors; which tax form applies; is the credit a debt, and does
it become unclaimed property when dormant. Have counsel review the Bliss payouts in the same engagement.

The only public wording to start from: *"Tickets you buy have no cash value. Enrolled creators may redeem eligible earned Tickets
through the Creator Cash-Out Program at the published rate, subject to holds and the program reserve."* The reserve funds the
program; it does not "back" Tickets, and "worth USD" is never said about bought Tickets.

## Order of work

1. **Now, internal only:** the 85/15 split with its reversals (the plan-invoice carve-out included), the dispute gap above, and a
   shadow credit ledger that measures s x c and fraud without paying anyone.
2. **After counsel:** invite-only enrollment, the Redeemable balance, the statement, monthly payout batches that McKale approves by
   hand, the starting rate, the pro-rata fill and a monthly coverage report.
3. **Not before counsel:** the program terms, any published rate, any "worth" or "backed" copy, enrollment, and any USD transfer.

## Docs and pages that are wrong today

- `terms.rs` section 7, Creator Revenue ("25% free tier, 40% Creator ... paid in Bliss"): the code pays a flat 70%, in Tickets.
- `terms.rs` section 5, "Virtual Currency (Bliss)": it describes the **bought** currency but calls it Bliss ("purchased via our payment
  system", "cannot be exchanged for real money"). Bliss cannot be bought and pays USD; the bought currency is Tickets, and **Tickets
  appear nowhere in the Terms**. Counsel needs both currencies defined correctly before any cash-out wording.
- `premium.rs` (40% in the plan lists and the comparison row) and `SUBSCRIPTIONS.md` (the same 25% and 40%).
- The Tickets column of the table in `CURRENCY.md` ("Can you earn it? No"): creators earn Tickets from sales.

The section 7 numbers change when McKale gives the creator share; the section 5 mix-up is wrong whatever he decides.

## Decisions for McKale

1. The reserve share (15% as decided, or more) and the starting rate ($1.50 per 1,000 Tickets).
2. The 120 day hold, and a Redeemable balance that is not spendable until the creator moves it.
3. Both lines pay the same creator for the same sale (the Bliss line by score from the treasury, the Tickets line from the reserve):
   keep both (recommended; no dollar counts twice).
4. Web Tickets only.
5. Creators get 70% of a sale as spending power and about 9 cents in cash per dollar: is that the promise?
6. Tickets granted by plans: the reserve is funded from the Tickets share of those invoices (decided 2026-10-06). Open: the yardstick
   price per Ticket (the chart's default is the cheapest pack).
7. Refunds and Stripe's fee on treasury gifts: which account pays?
8. Treasury investments: offered at all (counsel first), repaid how fast, with a return or without one.
9. Taxes: Treasury and reserve deducted when funded or when paid out (the accountant's call).
