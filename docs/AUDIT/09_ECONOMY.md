# 09 — Economy & Monetization

> Bliss tokens (dual-nature: in-game arcade currency **and** Proof-of-Contribution
> cryptocurrency), Premium subscriptions, Marketplace, creator payouts, Steam IAP,
> Stripe Connect, refund / chargeback, tax / VAT / GST, fraud detection.

## Pass changelog

- **P2 (2026-05-14):** New system doc; 12 features, 8 cards expanded, 10 wiring gaps.
- **P4 (2026-05-14):** State corrections from secondary critique: Marketplace state **inflated** — purchase handler has no Bliss-debit; true state 🟡 40% (was 75%). Stripe Connect (Feature 3) is **gated by [08] KYC (Feature 14)** — not independent; effective state 🔴 0% until [08] lands. Bliss dual-nature carries **regulatory arbitrage risk** — needs legal counsel before public launch.

---

## Concept summary

**Bliss (BLS)** has two natures:
1. **In-game arcade currency** — Steam Wallet purchase ($0.99–$49.99), spent on cosmetics, marketplace items, creator tips. Cosmetic-only — **no pay-to-win**.
2. **Proof-of-Contribution cryptocurrency** — Light Nodes (auto-run while engine open) and Full Nodes (opt-in, +10% bonus, stores chain data) earn Bliss. Daily payouts at UTC midnight from a USD treasury that drips to active contributors via Stripe Connect.

**Premium subscriptions** (3 tiers): Player Plus ($4.99/mo) — 500 Bliss/mo + queue priority + cosmetics + 10 GB saves; Creator Pro ($9.99/mo) — 40% revenue share (vs. 25% free), 1 TB storage, advanced analytics, 500 Bliss/mo; Bundle ($12.99/mo) — both + 1000 Bliss/mo.

**Marketplace** (two tabs): Creator Content (assets, scripts, plugins, templates, with equity investment for Spaces) and Avatar Items (cosmetics).

State of implementation: **frontend ~90% complete** (marketplace, premium, bliss pages all built). **Backend ~5–10%** — marketplace listing/purchase endpoints work; everything else (Steam IAP, Stripe Connect, subscription lifecycle, refund, tax, fraud) is **stub or absent**. Critical path 12–16 weeks.

---

## Implementation snapshot

**Crates / files:**
- [eustress-bliss](../../eustress/crates/bliss/) — Light + Full Node infra, Ed25519 keypair, Cosign client to `witness.eustress.dev`, Axum API server (`/api/cosign`, `/api/identity/verify`)
- [eustress-backend/src/marketplace.rs](../../eustress/crates/backend/src/) — Listing, search, purchase (Bliss debit), purchase history, featured items
- [eustress-web/src/pages/marketplace.rs](../../eustress/crates/web/src/pages/), `bliss.rs`, `premium.rs` — full UI suite
- [docs/monetization/CURRENCY.md](../monetization/CURRENCY.md), [SUBSCRIPTIONS.md](../monetization/SUBSCRIPTIONS.md)

**Working:**
- ✅ Bliss Light/Full Node concepts coded
- ✅ Cosign client + witness Worker
- ✅ Marketplace UI (13 item categories)
- ✅ Premium tier page (comparison + billing toggle)
- ✅ `/api/marketplace/{list, search, purchase, featured}` endpoints

**Stub or absent:**
- 🔴 Steam IAP integration (docs only; no Steamworks FFI)
- 🔴 Stripe Connect onboarding (UI buttons; backend missing)
- 🔴 Subscription endpoints (`POST /subscribe`, `PUT /update`, `DELETE /cancel`)
- 🔴 `BlissDataStore` (`credit()`, `debit()`, `get_balance()`) — spec only
- 🔴 Revenue split logic in backend (formula in docs)
- 🔴 Payout schedule cron
- 🔴 Refund flow handler
- 🔴 Tax engine (VAT/GST/regional)
- 🔴 Fraud detection / rate limit on purchase

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Bliss in-game token | 🟡 60% |
| 2 | Steam wallet IAP | 🔴 5% |
| 3 | Stripe Connect (creator payouts) | 🔴 5% |
| 4 | Player Plus subscription | 🟡 40% (UI; no backend) |
| 5 | Creator Pro subscription | 🟡 40% (same) |
| 6 | Marketplace listing + purchase | 🟢 75% |
| 7 | Creator payouts ledger | 🔴 10% |
| 8 | Refund + chargeback handling | 🔴 0% |
| 9 | Subscription renewal / cancellation | 🔴 0% |
| 10 | Bliss balance ledger (credit/debit) | 🔴 0% |
| 11 | Tax compliance (VAT / GST) | 🔴 0% |
| 12 | Fraud detection + rate limiting | 🔴 0% |

---

## Detailed per-feature cards (top 8)

### Feature 2 — Steam Wallet IAP

**State:** 🔴 5% · **Effort:** L (3–4 weeks) · **Risk:** Med · **Touches:** [06], [08], [09]
**Sub-features:** Steamworks SDK FFI · `SteamIapService` · item definitions JSON · webhook receiver · purchase callback grant · child purchase block (COPPA)

**Concept.** A user clicks Buy 500 Bliss → opens Steam overlay → completes purchase → Steam fires `purchase_completed` webhook → backend grants Bliss + records transaction. Documented in [CURRENCY.md](../monetization/CURRENCY.md) with API examples. No Rust integration today.

**Forecasted feedback (R)**
- R2.1 Steamworks SDK is C++ via FFI — Rust bindings exist but require manual wrapping.
- R2.2 Steam ID ↔ Eustress user ID mapping table needs to exist before first IAP.
- R2.3 IAP webhook must be idempotent (Steam retries).
- R2.4 Child account purchase block requires `is_child` check via KYC ([08_IDENTITY] Feature 14 COPPA).
- R2.5 Steam takes 30% cut — built into pricing math but worth documenting.
- R2.6 Refund: Steam standard is 14 days; matched at the IAP layer.

**Implications (I)**
- *Architectural:* Steam IAP is the only currently-targeted purchase rail; Stripe is for creator payouts not purchases.
- *Cross-system:* [08_IDENTITY] gates child purchases; [10_TELEMETRY] receives purchase events.
- *Migration:* none — green-field.
- *Operational cost:* Steam's 30% is the largest line item; net margin ~70% on Bliss tier pricing.
- *Support burden:* refund inquiries, "I didn't get my Bliss" tickets — needs a status page.
- *Strategic:* Steam is the natural distribution; alternative (Stripe direct) loses access to Steam catalog.

**Risks (X)**
- X2.1 Replay attack on webhook → granting duplicate Bliss.
- X2.2 Without webhook signature verify, anyone can claim purchases.

**Mitigations (M)**
- M2.1 HMAC-signed webhook receiver with idempotency keys.
- M2.2 Reconcile daily with Steam's billing API.

---

### Feature 3 — Stripe Connect (creator payouts)

**State:** 🔴 5% · **Effort:** XL (10–12 weeks incl. KYC + 1099) · **Risk:** Critical (regulatory) · **Touches:** [06], [08], [09]
**Sub-features:** Stripe Connect OAuth onboarding · bank account verification · daily payout cron · contribution scoring · IRS 1099-NEC generation · multi-region

**Concept.** Creators link a Stripe Connect account; the platform sends earnings (marketplace sales + Proof-of-Contribution Bliss rewards) to creator's bank. Daily payout at UTC midnight from the treasury.

**Forecasted feedback (R)**
- R3.1 Endpoints (`/api/stripe/connect/onboard`, `/api/stripe/checkout`) **don't exist** despite UI buttons.
- R3.2 KYC required for payout (Stripe handles part; we layer on additional checks).
- R3.3 Money transmission license required in many US states; varies by state.
- R3.4 1099-NEC mandatory for US creators earning > $600/yr.
- R3.5 EU creators trigger VAT MOSS obligations.
- R3.6 Spot exchange rates if creator's bank is non-USD.

**Implications (I)**
- *Architectural:* `creator_payouts` table needs: `(creator_id, amount, status, scheduled_at, sent_at, stripe_payout_id)`. Missing today.
- *Cross-system:* [08_IDENTITY] Feature 14 (KYC) gates payout eligibility.
- *Compliance:* the largest regulatory surface area on the platform.
- *Operational cost:* Stripe fees (0.25% + $0.25 per payout) + state money-transmitter compliance.
- *Support burden:* "Where's my money" is the highest-emotion ticket category.
- *Strategic:* without payouts, the platform has no creator economy.

**Risks (X)**
- X3.1 Money-transmission compliance failure → state-level legal action.
- X3.2 1099 mis-filing → IRS penalties + creator anger.
- X3.3 Chargebacks on already-paid-out creator earnings.

**Mitigations (M)**
- M3.1 Use Stripe Tax for VAT/MOSS handling.
- M3.2 Engage a money-transmission attorney *before* first US payout.
- M3.3 Hold-period (e.g. 14 days) before creator earnings become payable to absorb chargebacks.

---

### Feature 6 — Marketplace listing + purchase

**State:** 🟢 75% · **Effort:** M (close the gaps) · **Risk:** Med · **Touches:** [04], [06], [09]
**Sub-features:** Listing CRUD · purchase (Bliss debit) · search / filter · featured curation · purchase history · equity investment in Spaces

**Concept.** Creators list assets (models, scripts, plugins, audio, textures, templates) or Avatar items (cosmetics). Buyers spend Bliss; revenue split (25% free / 40% Pro creator). Spaces support fractional equity investment.

**Forecasted feedback (R)**
- R6.1 No creator approval flow — anyone can list (spam / DMCA risk).
- R6.2 Equity tracking field exists but no backend ledger.
- R6.3 Revenue split computed but not invoked on purchase.
- R6.4 No marketplace listing moderation queue.
- R6.5 Bliss balance check not atomic — concurrent purchases race.

**Implications (I)**
- *Cross-system:* [04_ASSETS] enforces upload → marketplace flow; [08_IDENTITY] handles DMCA + content moderation.
- *Strategic:* marketplace is the creator-economy flywheel.

**Risks (X)**
- X6.1 Stolen / DMCA'd content listed → legal liability.
- X6.2 Race conditions in concurrent buy → balance over-spend.

**Mitigations (M)**
- M6.1 Creator approval queue; auto-approve verified KYC creators.
- M6.2 `SELECT FOR UPDATE` lock on balance during debit.

---

### Feature 4 / 5 — Player Plus & Creator Pro subscriptions

**State:** 🟡 40% (UI yes, backend no) · **Effort:** L (5–6 weeks) · **Risk:** High · **Touches:** [06], [08], [09]
**Sub-features:** subscription state machine (Active / PastDue / Canceled / Expired) · monthly Bliss grant cron · revenue-share boost flag · Steam subscription webhook · churn tracking

**Concept.** A user subscribes via Steam (or Stripe for non-Steam regions). Monthly billing fires webhooks → state machine updates → monthly Bliss allowance (500 or 1000) credits. Cancellation: state → Canceled, runs to end of period, then Expired.

**Forecasted feedback (R)**
- R4.1 No `/api/subscriptions` endpoint.
- R4.2 No DB `subscriptions` table.
- R4.3 No Steam webhook handler for `subscription_renewed / canceled / payment_failed`.
- R4.4 Monthly 500-Bliss grant defined but no cron.
- R4.5 Churn analytics absent.
- R4.6 FTC requires one-click cancellation.

**Implications (I)**
- *Architectural:* subscription is its own state machine (~5 states + retry); not derivable from Bliss balance.
- *Cross-system:* [09] revenue-share boost flag is read by Marketplace purchase split logic (Feature 6).
- *Operational cost:* Player Plus generates ~$3.49/mo net; costs ~$0.44 to deliver 500 Bliss; high margin if churn <5%.
- *Strategic:* SaaS-style recurring revenue smoothes lumpy IAP earnings.

**Risks (X)**
- X4.1 Payment-failure → user keeps using paid features.
- X4.2 Cancellation friction → FTC complaint.

**Mitigations (M)**
- M4.1 PastDue grace period (3 days) before downgrade.
- M4.2 One-click cancel; confirmation, not friction.

---

### Feature 10 — Bliss balance ledger

**State:** 🔴 0% · **Effort:** S (2 weeks) · **Risk:** High · **Touches:** [09]
**Sub-features:** `bliss_balances` table · `bliss_transactions` ledger (audit) · ACID credit/debit · daily reconciliation · concurrent-purchase lock

**Concept.** Every Bliss is double-entry: a credit (from purchase or Proof-of-Contribution payout) and a debit (spend or transfer). The ledger reconstructs any balance; balances table is a cached view.

**Forecasted feedback (R)**
- R10.1 Today's purchase stub calls `state.db.purchase_item()` with no actual SQL.
- R10.2 Concurrent purchase race: check-then-debit needs row lock.
- R10.3 No audit log → can't detect fraud or replay history.
- R10.4 Future BLS cryptocurrency on-chain settlement might want this ledger to be the off-chain mirror.

**Implications (I)** — *Architectural:* fundamental — every other money feature reads/writes here.

**Risks (X)** — X10.1 Over-spend bug = financial loss.

**Mitigations (M)** — M10.1 ACID transactions; nightly reconcile; observability via [10_TELEMETRY].

---

### Feature 8 — Refund + chargeback

**State:** 🔴 0% · **Effort:** L (6–8 weeks) · **Risk:** Very High · **Touches:** [09]
**Sub-features:** refund policy doc · `charge.dispute.created` webhook (Stripe) · Bliss credit reversal · creator payout reversal · fraud scoring

**Concept.** Steam handles its own IAP refunds; we mirror state. Stripe sends chargeback webhooks → reverse Bliss grants, reverse creator payouts, alert creator.

**Forecasted feedback (R)**
- R8.1 No policy doc.
- R8.2 No Stripe chargeback webhook handler.
- R8.3 No fraud scoring → 1%+ chargeback rate hurts at scale.
- R8.4 Creator's payout already wired to bank? Use a hold period.

**Implications (I)** — *Operational cost:* Stripe chargebacks $15–$100 each + lost revenue.

**Risks (X)** — X8.1 Pattern of high chargebacks blacklists the merchant.

**Mitigations (M)** — M8.1 14-day hold on creator earnings; fraud-velocity score on accounts; clear refund policy.

---

### Feature 11 — Tax compliance (VAT / GST)

**State:** 🔴 0% · **Effort:** L (4–6 weeks) · **Risk:** Critical · **Touches:** [09]
**Sub-features:** Stripe Tax integration · per-region tax rate · invoice generation · per-jurisdiction filing prep

**Concept.** EU VAT, UK VAT, Canada GST, Australia GST, others — digital-goods tax obligations vary. Stripe Tax handles 200+ jurisdictions if enabled.

**Forecasted feedback (R)**
- R11.1 Not mentioned in any doc or code.
- R11.2 Tax-inclusive vs. tax-exclusive pricing changes conversion math.
- R11.3 Invoices must itemise tax.
- R11.4 Filing prep (quarterly EU VAT MOSS) is its own workflow.

**Implications (I)** — *Compliance:* non-compliance penalties = % of revenue.

**Risks (X)** — X11.1 EU VAT audit = up to 20% of EU revenue penalty.

**Mitigations (M)** — M11.1 Enable Stripe Tax in dashboard; pass tax_code on line items.

---

### Feature 12 — Fraud detection

**State:** 🔴 0% · **Effort:** L · **Risk:** High · **Touches:** [08], [09]
**Sub-features:** velocity check · geolocation mismatch · device fingerprint · rate-limit per endpoint · ML scoring (later)

**Concept.** First-line defence: rate limits on purchase / payout / refund endpoints. Second: velocity scoring (10 purchases in 1 minute = suspicious). Third: ML scoring on cumulative features.

**Forecasted feedback (R)**
- R12.1 No rate limit on purchase endpoint.
- R12.2 No velocity tracking.
- R12.3 No IP / device fingerprint.
- R12.4 ML scoring is post-MVP.

**Implications (I)** — *Operational cost:* Stripe Radar built-in fraud is cheap; bespoke ML is later.

**Risks (X)** — X12.1 Coordinated card-testing attacks rack up authorisation fees.

**Mitigations (M)** — M12.1 Enable Stripe Radar; rate-limit per IP; CAPTCHA on first purchase.

---

## Wiring / import gaps (top 10)

1. Stripe Connect onboarding endpoint
2. Steam IAP webhook receiver (HMAC-signed)
3. Bliss debit ledger (SQL schema + ACID)
4. Creator payout schedule cron (daily UTC midnight)
5. Refund webhook + Bliss/payout reversal
6. Stripe chargeback handler
7. Stripe Tax integration
8. Fraud detection (rate-limit, velocity, Radar)
9. Subscription lifecycle (state machine + webhook)
10. Marketplace listing approval flow + creator KYC gate

---

## Cross-system dependencies

- **[06_WEBSITE]** Bliss / Premium / Marketplace UIs; checkout pages.
- **[08_IDENTITY]** KYC gates payout; child purchase block; signed asset chain for marketplace.
- **[04_ASSETS]** marketplace items = assets with sale metadata.
- **[10_TELEMETRY]** Bliss + revenue + churn events.
- **[12_INFRASTRUCTURE]** Stripe + Steam secret management (Vault).

---

## Open policy questions

- Q9.1 Regional pricing tiers (India ₹99/mo vs $4.99/mo)?
- Q9.2 Featured-creator bonus (50%? cap?).
- Q9.3 Minimum payout balance ($50? $100?).
- Q9.4 Refund window (Steam 14 days; marketplace shorter?).
- Q9.5 Marketplace moderation: human, automated, hybrid?
- Q9.6 Equity stakes in Spaces — profit-share % vs. buy-back?
- Q9.7 Gifting / bundles allowed?
- Q9.8 Regional payment methods (iDEAL EU, UPI India, crypto)?
- Q9.9 Creator KYC threshold before first payout?
- Q9.10 Bliss expiry vs. permanent (Proof-of-Contribution tail emission)?
