# Regulation A Tier 2: Selling Ownership in Eustress Directly

**A step-by-step workflow for an Arizona LLC to sell ownership to the public from its own website, without an exchange or an underwriter.**

**Status:** planning document. Nothing here has been filed or decided.
**This is not legal advice.** Every rule below is stated from general knowledge of SEC Regulation A and must be confirmed by a securities lawyer before it is relied on. Dollar limits and form details change.

---

## Where we are

| Stage | State |
|---|---|
| 0. Sell product, not ownership (Tickets, Delta, plans, sponsorships) | **In progress** |
| 1. Readiness gates | Not started |
| 2. Entity and structure decisions | Not started |
| 3. Service providers hired | Not started |
| 4. Audit | Not started |
| 5. Testing the waters | Not started |
| 6. Form 1-A drafted and filed | Not started |
| 7. SEC review and qualification | Not started |
| 8. Offering open | Not started |
| 9. Ongoing reporting | Not started |
| 10. Liquidity for holders | Not started |

Update this table as stages move. One stage is "In progress" at a time.

---

## What Regulation A Tier 2 is, in plain words

A way to sell up to **$75 million** of ownership in any 12 months to **anyone**, not only wealthy investors, after the SEC reviews and "qualifies" an offering document. It is often called a mini-IPO. The company can sell from its own website. Nothing is listed on an exchange.

What it costs in obligations: audited financial statements, an offering document the company is legally liable for, and public reports twice a year for as long as the securities are outstanding.

| | Tier 2 |
|---|---|
| Limit | $75M per 12 months |
| Who can buy | Anyone. A buyer who is not accredited (not wealthy by the SEC's test) may invest at most 10% of the greater of their annual income or net worth |
| Financial statements | Two years, audited |
| State review of the offering | Preempted: the states do not review it, but they can require a notice filing and a fee |
| Ongoing reports | Annual (Form 1-K), semiannual (Form 1-SA), current events (Form 1-U) |

---

## Stage 0: sell product first

Sell things people use: Tickets, Delta, plans, and engine sponsorships ([SPONSORSHIP.md](../monetization/SPONSORSHIP.md)). None of these is a security, because none promises a profit. This stage produces the two things an offering needs: revenue to audit, and a list of customers who already trust the company.

**Hard rule for this stage:** say nothing in public about a future chance to invest. A public statement that conditions the market for an unregistered offering can itself be a violation. Testing the waters (Stage 5) is the lawful way to ask, and it has its own rules.

## Stage 1: readiness gates

Do not start Stage 2 until every line is true.

- [ ] Revenue exists and is recorded in books an auditor can audit (accrual accounting, bank statements, Stripe reports reconciled monthly).
- [ ] The public launch has happened and the product has paying users.
- [ ] A securities lawyer who has taken a Regulation A offering to qualification is engaged.
- [ ] There is a named use of proceeds: what the money builds, in dollars per item.
- [ ] No "bad actor" problem: no covered person (owner of 20% or more, officer, manager, promoter) has a disqualifying event such as a securities-related conviction or order. Counsel runs this check.
- [ ] Budget set aside for the offering itself. On the in-house path below the floor is roughly $20,000 to $60,000 before the first dollar is raised, most of it the audit.

## Stage 2: entity and structure decisions

An Arizona LLC can use Regulation A. It sells **membership units**, not stock. Three decisions come first, and each is McKale's with counsel and a tax adviser.

**Decision A: stay an LLC or convert to a corporation.**

| Option | What it means | Trade-off |
|---|---|---|
| LLC taxed as a partnership (the default) | Profit and loss pass through to every holder each year | Every investor gets a Schedule K-1 tax form yearly. With thousands of small holders this is costly and investors dislike it |
| LLC that elects to be taxed as a corporation | Still an LLC under Arizona law, taxed like a C corporation | Investors get a simple 1099 only when distributions are paid. Keeps the LLC's flexible operating agreement |
| Convert to a C corporation (Arizona or Delaware) | Sells shares of stock | The form investors, transfer agents and later acquirers know best. Conversion has legal cost and tax consequences |

For a public raise from many small holders, the first row is the one to avoid.

**Decision B: what the unit is.** This is where the "stock is participation, not speculation" design lives. Options that can be combined:

- **Non-voting units**, so control stays with the founder.
- **Transfer restrictions**, so units cannot be freely traded; the exit is the company's own repurchase program (Stage 10).
- **A distribution policy**: a stated share of profit paid to holders, so the return is from the business and not from resale.
- **Holder perks** in product (credits, early access). Perks are allowed, must be described in the offering document, and do not change the unit's status as a security.

Whatever is chosen is written into the operating agreement (or charter) before filing, and described in full in the offering document.

**Decision C: price and amount.** With no market, the company sets the price and must be able to explain how. Decide the maximum raise, the minimum (if any, with escrow until it is met), and the price per unit.

## Stage 3: service providers

| Provider | What they do | Required? |
|---|---|---|
| Securities counsel | Drafts the offering document, answers SEC comments | Yes in practice |
| Independent auditor | Audits two years of financial statements under US auditing standards | Yes |
| SEC-registered transfer agent | Keeps the official list of holders | Needed for the holder-count exemption below |
| Escrow agent or bank | Holds investor money until a closing | Needed if there is a minimum raise; good practice always |
| EDGAR filing agent | Formats and submits the filings to the SEC's system | Practical necessity |
| Broker-dealer of record | A registered firm that handles investor onboarding and compliance for a fee of roughly 1% to 3% | Optional federally. **Some states require a registered broker-dealer or an issuer-dealer registration for issuer-direct sales, and Arizona has been reported to be one of them. Confirm with counsel before choosing to go without one** |
| KYC/AML and payments | Identity checks and money movement for investors | Yes; the transfer agent or broker-dealer often provides it |

The company also needs EDGAR access codes (Form ID) before anything can be filed.

## Stage 4: audit

Two fiscal years of financial statements, audited. A company younger than two years audits from its formation date. Start early: a first audit of a company that has never been audited commonly takes two to four months.

## Stage 5: testing the waters

Before or after filing, the company may ask the public whether they would be interested, and collect non-binding indications. Rules:

- Every communication carries the required legend: no money is being taken, none will be accepted until qualification, and an indication of interest is not a commitment.
- The materials are filed with the SEC as exhibits to the offering document.
- Everything said is subject to the antifraud rules. No projections that cannot be supported.

This is also the first real measurement of demand, and it can run from the website.

## Stage 6: Form 1-A

The offering document, in three parts:

1. **Notification**: basic facts about the company and the offering.
2. **Offering circular**: the business, the risks, the use of proceeds, the owners and managers and what they are paid, related-party dealings, the securities being sold, and the audited financial statements.
3. **Exhibits**: the operating agreement, the subscription agreement, material contracts, counsel's legality opinion, the auditor's consent, and the testing-the-waters materials.

A first-time issuer can submit a draft for non-public review. It must be public for at least 21 days before qualification.

## Stage 7: SEC review and qualification

The SEC staff sends comment letters; the company answers and amends. Typical first comments arrive in about four weeks, and the whole review commonly runs three to six months. The offering can begin only after the SEC issues a notice of qualification.

In parallel: state notice filings and fees where the units will be sold.

## Stage 8: the offering, on the Eustress website

What the site may do: show the qualified offering circular, verify the investor's identity, apply the 10% limit for non-accredited investors, take a signed subscription agreement, send money to escrow, and record the purchase with the transfer agent.

What the site may not do: match one investor's sale with another investor's purchase. That is operating an exchange and needs a registered broker-dealer and trading system. See Stage 10.

**Surfaces.** The web flow, an API, a CLI and an MCP server are all the same legal act and carry the same rules:

- Read-only data is open to everyone: the offering circular, filed reports, budgets and milestone reports.
- A purchase always requires a verified person, the delivered offering circular, the investment-limit check and a signed subscription. An automated agent can prepare an order; the investor confirms it.
- Every statement any surface makes about the offering is an offering communication. Generated text about the offering is not allowed to improvise.

Sales may continue for up to three years on a continuous basis if the offering is set up that way, with the circular kept current.

## Stage 9: ongoing reporting

| Report | When | What |
|---|---|---|
| Form 1-K | Within 120 days after fiscal year end | Annual report with audited financial statements |
| Form 1-SA | Within 90 days after the first half | Semiannual report, unaudited financials |
| Form 1-U | Within 4 business days of the event | Material events: a change of control, of auditor, a bankruptcy, a departure of a principal officer, and similar |
| Form 1-Z | At the end | Exit report when the offering is finished or reporting ends |

**The higher standard, as a practice on top of the rules:** tie each raise to a named budget with deliverables, and publish progress against it with each report. The rules require use-of-proceeds disclosure; reporting against it line by line is the company's own commitment.

## Stage 10: liquidity without a ticker

Holders need a way out, or they will pay less going in. Options, in plain words:

| Option | What it is | Notes |
|---|---|---|
| Company repurchase program | The company offers to buy units back at set times at a stated formula price | Keeps everything in-house. It is an issuer tender offer and has its own rules. Needs cash set aside |
| Distributions | Holders are paid from profit and hold for income | No exit needed for holders who are content with income |
| A registered alternative trading system | An already-registered venue where holders can sell to each other | The company does not run it. Brings back a market price |
| Do nothing | Units are simply illiquid | Lawful if disclosed, and it lowers what investors will pay |

**Never build the matching engine on the Eustress site.** That is the one line that turns the company into an unregistered exchange.

## The holder-count trigger and its exemption

A company with more than $10 million in assets and either 2,000 holders of record, or 500 who are not accredited, must register under the Exchange Act and become a full public reporting company. Units sold in a Tier 2 offering do not count toward those numbers while all of these hold:

1. The company is current in its Regulation A reports (Stage 9).
2. A transfer agent registered with the SEC keeps the holder records.
3. The company's public float is under $75 million, or, if it has no public float, its annual revenue is under $50 million.

A company that grows past the size limit gets a two-year transition before it must register.

## Costs: the in-house path

The plan is to do everything the law allows in house, with code. Two things cannot be done in house at any price, because the rules require an outside party. Everything else can.

### What cannot be in house

| Item | Why | Floor |
|---|---|---|
| The audit | The auditor must be independent of the company. Software cannot sign an audit opinion | $10,000 to $30,000 for a small company with clean, reconciled books |
| The legality opinion | Form 1-A requires an exhibit in which a licensed lawyer states the units are validly issued | $3,000 to $15,000 as a limited engagement, more if the lawyer also reviews the circular |
| State notice fees | Each state where units are sold charges a filing fee | A few hundred dollars per state; sell in fewer states at first to hold this down |
| Identity checks | A vendor charges per investor verified | About $1 to $3 per investor |

### What moves in house, and what replaces it

| Usual cost | Usual price | In-house replacement | New cost |
|---|---|---|---|
| Counsel drafting the whole offering | $75,000 to $200,000 | The company drafts the circular from its own data; a lawyer gives the opinion and a limited review | The opinion above |
| EDGAR filing agent | $5,000 to $20,000 | File directly. The company gets its own EDGAR codes; Form 1-A is entered on the SEC's site and the documents are attached | $0. Regulation A has no SEC filing fee |
| Transfer agent | $5,000 to $25,000 a year | The company registers as its own transfer agent with the SEC (Form TA-1) and keeps the holder ledger in its own system | $0 in fees. It brings record-keeping and reporting duties of its own; counsel confirms a self-registered agent satisfies the holder-count exemption |
| Escrow | $5,000 to $15,000 | No minimum raise, so each purchase closes as it is accepted | $0. Counsel confirms; some states may still expect escrow |
| Broker-dealer of record | 1% to 3% of the raise | Issuer-direct sales by the company's own people, who are paid no commission | $0, in states that allow it. The Arizona question in Stage 3 decides this |
| Marketing | 5% to 15% of the raise | The company's own channels: the site, the customer list, the email engine | Time, and email costs |
| Audit preparation | Part of a $25,000 to $80,000 audit | Books that reconcile themselves (below), so the auditor tests instead of rebuilding | The audit floor above |

### The automation to build

Each of these is code in this repository, and each removes a line from the table above.

1. **Books an auditor can test.** The Treasury already records every deposit, fee, refund and payout once, by source. The monthly close turns a month of those records and the cost ledger into a journal and a trial balance, and checks the month against Stripe's balance transactions. **Built, not deployed:** `GET /api/admin/books/close?month=YYYY-MM` (add `&reconcile=1` for the Stripe check, `&format=csv` for the journal as a file), in [`books.mjs`](../../infrastructure/cloudflare/api/src/books.mjs). Still to add: a per-payment detail export, and the accounting policy for prepaid Tickets, Delta and plans, which is the accountant's decision.
2. **Holder ledger.** An append-only record of who holds how many units, with every issuance, transfer and repurchase as an entry. This is the transfer agent's core record. **Built, not deployed:** [`holder_ledger.mjs`](../../infrastructure/cloudflare/api/src/holder_ledger.mjs). Administrators record entries under `/api/admin/holders/*`; a holder reads their own at `/api/holdings`. Each entry is hashed together with the one before it, and `/api/admin/holders/verify` recomputes the chain and every balance. It counts holders of record and non-accredited holders beside the 2,000 and 500 limits. It sells nothing and takes no money: issuing units to the public waits on a qualified offering. Still to add: one class of unit only, no address on file (a transfer agent needs one), and no export of the holder list.
3. **Subscription flow.** Identity check, the 10% limit for non-accredited investors, delivery of the circular, a signed subscription agreement, payment, and the ledger entry. **Built, not deployed:** [`subscriptions.mjs`](../../infrastructure/cloudflare/api/src/subscriptions.mjs). How it works:
   - **Nothing is offered until an administrator opens an offering.** With none on file, or a draft, `GET /api/offering` answers that there is none. Opening one requires the SEC file number (`024-` and five digits) and the date of the notice of qualification, the circular and the subscription agreement each with the SHA-256 of its file, at least one state, and payment instructions. The code cannot check the file number against the SEC; the person who opens the offering carries that.
   - **Testing the waters (Stage 5)** is its own status. It takes one non-binding indication of interest per person at `/api/offering/interest`, shows the required legend, and accepts no subscription.
   - **A subscriber** is a signed-in account with a verified identity, a natural person, resident in the United States, in a state the offering names.
   - **The 10% limit.** A subscriber who is not accredited states annual income and net worth; the limit is 10% of the greater, counted across all of their subscriptions in the offering. Accredited status is the subscriber's own statement.
   - **Prepare, then sign.** `POST /api/subscriptions` prepares one and commits nobody, so the site, the API, a command line or an agent can do it. `POST /api/subscriptions/{id}/sign` is the person's act and needs a session that is good at that moment: their typed legal name, the hashes of the circular and agreement they were shown, and five statements, each true. Signing reserves the units, and no more are reserved or sold than the offering has.
   - **No money moves through the code.** The subscriber pays by bank transfer, quoting a payment reference. An administrator records that the exact amount arrived, then accepts.
   - **Acceptance** records the holder and issues the units on the holder ledger in the same write, so a subscription is never accepted without its units or issued twice. It is refused while the offering is paused or closed, and when the circular changed after the subscriber signed, until they sign the current one.
   - **Leaving.** A subscriber can cancel before acceptance; an administrator can reject with a reason or expire signed subscriptions whose funds never arrived. Each frees the units. If funds had arrived, the subscription is marked as owed a refund until an administrator records the refund.
   
   Still to add: the pages on the site, the CLI and MCP tools (the API they would call is in place), email notices to the subscriber, subscribers that are companies or trusts, residents of other countries, and a card or bank-debit payment path, which waits on a payment provider that permits securities sales. Counsel confirms: the legend wording, that a typed-name signature and self-stated accredited status are sufficient, and whether any state where units are sold expects escrow.
4. **Circular and report generator.** The offering circular, Form 1-K and Form 1-SA drafted from the books, the holder ledger and the use-of-proceeds budget, in the format EDGAR accepts. A person reads and signs every one.
5. **Deadline tracker.** The report dates in Stage 9 and each state's renewal date, with reminders.
6. **Budget-to-actual report.** Each raise's named budget against what was spent, published with every report.

### The honest floor

| | Usual path | In-house path | Traditional IPO (Form S-1, listed on Nasdaq) |
|---|---|---|---|
| Before the first dollar is raised | $150,000 to $400,000 | About $20,000 to $60,000 | About $530,000 to $2.5 million, plus the underwriting discount below |
| Each year after | $50,000 to $200,000 | About $10,000 to $35,000 | About $500,000 to $1.5 million or more (estimate) |
| Selling cost, taken out of the money raised | Broker-dealer 1% to 3%, plus marketing 5% to 15% of the raise | Time, and email costs | Underwriting discount of 7% to 8% of the raise: $1.4 million to $1.6 million on a $20 million raise |
| SEC registration fee | None | None | $87 per $1 million registered (rate from October 1, 2026): $1,740 on $20 million |
| FINRA filing fee | Owed when a broker-dealer takes part: $500 plus 0.015% of the offering | None | $500 plus 0.015% of the offering: $3,500 on $20 million |
| Exchange listing | None. Nothing is listed | None. Nothing is listed | Nasdaq Capital Market: $50,000 entry fee (up to 15 million shares; $75,000 above that), then $56,000 to $86,500 a year |
| Directors and officers insurance | Optional; not priced in this plan | Optional; not priced in this plan | Not required by law; about $200,000 to $750,000 a year for $10 million to $20 million of coverage |
| Yearly audit | Part of the yearly range above | Part of the yearly range above | Median $262,000 for a small public company (non-accelerated filer) |

The audit is the floor and it does not go to zero. What is traded for the savings is risk: with less lawyer review, the founder carries more of the liability described below for what the documents say. A middle path is to draft in house and pay a lawyer for one full read of the finished circular before filing.

These are planning ranges, not quotes.

What the third column shows, in dollars: on a $20 million raise, a traditional IPO costs about $1.9 million to $4.1 million before the company keeps any of the money (fixed costs plus the 7% to 8% underwriting discount), then about $500,000 to $1.5 million every year after. The in-house path costs about $20,000 to $60,000 up front and $10,000 to $35,000 a year: roughly 30 to 200 times less up front, and 15 to 150 times less each year. What the IPO buys for that is a ticker, a market price, and an underwriter who finds the buyers. How the IPO figures were built: the low end of the up-front range is one real 2026 filing for a $15 million Nasdaq Capital Market IPO ($530,882 of costs other than the discount); the high end is the $2.5 million average from the 2011 IPO Task Force survey. The low end of the yearly range adds only the audit, insurance and listing lines, and leaves out securities counsel, Sarbanes-Oxley work, investor relations, an EDGAR filing agent and a transfer agent; the high end is the same survey's $1.5 million average, from 2011 data. The yearly figure is an estimate.

Sources for the traditional IPO column (checked November 2026):

- Underwriting discount: [PwC, "Considering an IPO? First, understand the costs"](https://www.pwc.com/us/en/services/consulting/deals/library/cost-of-an-ipo.html) (average 4% to 7% of gross proceeds across 1,377 US IPOs from 2015 to 2025, deals under $25 million excluded); [Jay Ritter, "Gross Spread for Moderate Size IPOs, 1980-2025"](https://site.warrington.ufl.edu/ritter/files/IPOs-gross-spreads.pdf) (7% is the standard spread on $39 million to $157 million deals); two 2026 Nasdaq Capital Market IPO filings at 8%: [Amatuhi Holdings S-1/A](https://www.sec.gov/Archives/edgar/data/2078570/000149315226005914/forms-1a.htm) and [Advasa Holdings S-1/A](https://www.sec.gov/Archives/edgar/data/2084227/000149315226004058/forms-1a.htm).
- Fixed costs of one small IPO: [Amatuhi Holdings S-1/A, Item 13](https://www.sec.gov/Archives/edgar/data/2078570/000149315226005914/forms-1a.htm) (February 2026; $15 million offering; $530,882 total, including $200,000 accounting, $75,000 legal, $50,000 Nasdaq entry fee).
- Average cost to go public and to stay public: [IPO Task Force, "Rebuilding the IPO On-Ramp," October 2011](https://www.sec.gov/info/smallbus/acsec/rebuilding_the_ipo_on-ramp.pdf) ($2.5 million to go public, $1.5 million a year after; survey data from 2011).
- SEC registration fee: [SEC Fee Rate Advisory, fiscal year 2027](https://www.sec.gov/rules-regulations/fee-rate-advisories/2027-1) ($87.00 per $1 million, effective October 1, 2026).
- FINRA filing fee: [TheCorporateCounsel.net, June 2025](https://www.thecorporatecounsel.net/blog/2025/06/finra-fees-cap-for-corporate-financing-filings-increasing-by-400-on-july-1st.html) ($500 plus 0.015%); [Alston & Bird, October 2026](https://www.alston.com/en/insights/publications/2026/10/offering-review-fee-change-delay-2029) (cap stays $225,000 through 2028); [FINRA Regulation A FAQ](https://www.finra.org/rules-guidance/key-topics/public-offerings/regulation-A-faqs) (a Regulation A offering with a participating broker-dealer is filed with FINRA).
- Nasdaq fees: [Nasdaq fee filing SR-NASDAQ-2025-099, Federal Register, December 15, 2025](https://www.govinfo.gov/content/pkg/FR-2025-12-15/pdf/2025-22722.pdf) (Capital Market annual fee $56,000, $72,500 or $86,500 by shares outstanding, from January 1, 2026; Capital Market entry fees unchanged); [Securities Lawyer 101, June 2026](https://www.securitieslawyer101.com/2026/06/03/nasdaq-compliance-capital-market-listing-requirements/) (entry fee $50,000 up to 15 million shares, $75,000 above).
- Directors and officers insurance: [Alliance Risk, April 2026](https://joinalliancerisk.com/d-and-o-insurance-public-companies/) (broker estimate for $100 million to $500 million companies).
- Yearly audit: [Ideagen Audit Analytics, April 2026](https://www.ideagen.com/thought-leadership/blog/what-smaller-reporting-companies-actually-pay-for-audits) (median audit fee for non-accelerated smaller reporting companies, registrants as of February 2026).

## Liability, plainly

The company and the people who sign the offering document are liable for material misstatements and omissions in it and in every offering communication. This is personal exposure for the founder. It is the main reason every claim in the circular, on the site, and from any API or agent surface is reviewed before it is published.

## Alternatives to hold next to this one

| Path | When it fits better |
|---|---|
| Regulation Crowdfunding (up to $5M a year, through a registered funding portal) | A first, smaller raise. Reviewed financials can be enough at lower amounts; the sale runs on the portal's site, not ours |
| Regulation D Rule 506(c) (no limit, accredited investors only, verified) | A fast raise from a small number of wealthy investors, with no SEC review and a short notice filing |
| Sponsorships and product sales only | As long as revenue covers the plan. No securities, no reports, no liability of this kind |

## Questions for counsel (first meeting)

1. Does Arizona require a broker-dealer or issuer-dealer registration for issuer-direct Tier 2 sales to Arizona residents? Which other states do?
2. LLC taxed as a corporation, or convert to a C corporation: which, and in which state?
3. Can the units be non-voting and transfer-restricted with a company repurchase program, and how must the program be structured?
4. What may the website say today, before any filing, without conditioning the market?
5. How do Tickets, Delta, Bliss and the Treasury need to be described in the circular, and does any of them raise its own securities question?
6. What are the rules for selling to investors outside the United States?
7. May an API, CLI or MCP surface take orders, and what confirmation must the investor give?
