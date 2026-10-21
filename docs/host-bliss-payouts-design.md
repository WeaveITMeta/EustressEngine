# Host Bliss payouts

Status: **design only.** This branch does not add a hosting contribution
type, a host-quality score, or a payout. No Worker deploy, and no port,
DNS, or firewall change.

## Spec

McKale, 2026-10-20.

Award Bliss to good hosts for hosting. Hosting uptime and quality become
measurable contributions. They pay out through the contribution ledger and
Stripe Connect rail that already exists. They do not get a second payout
system.

"Good hosts" is the same idea as the best-host badge in
`docs/idle-host-preferences-design.md`: earned by uptime and quality. That
document spends the badge on which games Forge may warm. This document
spends the same measurements as a contribution that can pay. The two are
not the same feature. A host can be paid for uptime they already gave
without also having a preference list, and a preference list does not by
itself move money.

McKale states that this rail has been **live since July 1**. This tree does
not record that date. Nothing in the Worker hardcodes 1 July.
`bliss:genesis_date` is written, on the first settlement that finds it
empty, to that settlement's score date
(`infrastructure/cloudflare/api/src/treasury.mjs`, phase `init`). The
production value of that key was not read from here. **July 1 is
unverified**, including which year.

## The rail that already exists

Cited from this repository. Not a production probe.

### Ledger

`infrastructure/cloudflare/api/src/bliss.mjs` is the BLS ledger.

- Amounts are integer minor units. `BLISS_UNIT` is 100 (1 BLS = 100 minor).
- An append-only log is the truth. Keys: `entry:{userId}:{ts}:{id}`,
  `bal:{userId}` (cache), `cp:{userId}` (checkpoint).
- `ledgerAppend` writes an entry and refuses a second write of the same
  key. An emission credit uses id `dist` and timestamp
  `{scoreDate}T00:00:00.000Z`, so the same day credits once.
- `ledgerBalanceMinor` reads the cache, or derives the sum, and on a zero
  derived balance with a legacy `user.bliss_balance` calls
  `ledgerMigrateUser` once (`kind: 'migration_opening'`).
- `ledgerSpend` appends a negative entry. The comment in that function
  says KV has no compare-and-set, so two spends can race. It also says
  spending grants nothing today.

The nightly settlement credits the ledger from
`infrastructure/cloudflare/api/src/treasury.mjs`, phase `credit`:
`ledgerAppend` with `kind: 'emission'`, `ref` the score date, `id: 'dist'`,
then `ledgerReconcile`. Banned accounts are skipped. A missing user record
is marked `missing` and is not credited.

Emission math, in `bliss.mjs`: initial supply `BLISS_INITIAL_SUPPLY`
100,000,000, first-year rate `BLISS_INITIAL_RATE` 5%, halving every
`BLISS_HALVING_YEARS` (4), floor `BLISS_TAIL_RATE` 0.5%. A day mints
`ceiling × min(1, total_score / FULL_DAY_SCORE)` with `FULL_DAY_SCORE`
1,440. The same comment is at the top of the Bliss section in
`infrastructure/cloudflare/api/src/index.js` (around the `BLISS` banner
before `handleLedgerSpend`).

### What earns a score today

`handleCosign` in `infrastructure/cloudflare/api/src/index.js` accepts a
self-reported `contribution_type` and `duration_secs`. The weights in that
function are:

| Type | Weight |
|---|---|
| ActiveTime | 1.0 |
| Creation | 2.5 |
| Collaboration | 2.0 |
| Education | 2.2 |
| Development | 3.0 |
| Moderation | 1.5 |
| QualityAssurance | 1.8 |
| Optimization | 2.0 |
| Documentation | 1.5 |
| Custom | 1.0 |

There is **no hosting type**. Unknown types are rejected. The comment
above `handleCosign` says the client can forge any of these values, and
the witness bounds them: known type, duration clamped to 1–3600 seconds,
Full-node 1.1× only from server-observed `node-mode:{user}`, ActiveTime
capped by `presence:{date}:{user}`, daily score capped at
`MAX_DAILY_SCORE` (3200). The day is stored at `contrib:{day}:{userId}`
in `INVENTORY`.

That presence heartbeat is a signed-in account's witness presence. It is
not a game server's UDP session, and it is not listing uptime. Listing
uptime is the other clock: `live.mjs` drops a host 90 seconds after its
last `POST /live`. Those two clocks are not joined anywhere in this tree.

The website documents the same weight table and the same limits in
`eustress/crates/web/src/pages/docs_earning.rs` (the contributions table
and "Limits the Witness Enforces").

### USD through Stripe Connect

The same settlement, after the BLS credit, pays a USD drip.

- `stripeTransfer` in `treasury.mjs` `POST`s
  `https://api.stripe.com/v1/transfers` with an idempotency key
  `bliss-payout-{date}-{userId}`.
- `payout_setup` sizes the pool as `floor(drip_cents × utilization)`.
  Rates: `TREASURY_DRIP_RATE` 0.276% a day, `TREASURY_SCARCITY_RATE` 0.136%
  at or below `TREASURY_SCARCITY_RATIO` (15% of the high-water mark).
  While scarce, the top `TREASURY_TOP_FRACTION` (25%) of contributors by
  score are weighted by `TREASURY_TOP_BOOST` (2). The high-water mark
  decays by `TREASURY_HWM_DECAY` (0.171% a day).
- The comment on `payout_setup` says the weights count every contributor
  in good standing, **connected or not**, so a share that cannot be paid
  stays in the treasury instead of being given to someone else.
- `payout_plan` then marks a row `unconnected` when `u.connect` is
  missing, `no_plan` when `cashout_plan` is false, and
  `below_minimum` when the cents are under `STRIPE_MIN_TRANSFER_CENTS`
  (50). Those rows are not transferred. `payout_finish` adds
  `unconnected` and `no_plan` cents into the day's totals. They are not
  paid out.
- `credit` copies `user.stripe_connect_id` onto the row and sets
  `cashout_plan` from `planAllowsCashout`
  (`infrastructure/cloudflare/api/src/delta.mjs`). `CASHOUT_PLANS` is
  `creator`, `professional`, `bundle`.

Onboarding is `handleStripeConnectOnboard` in `index.js`. It refuses to
run without `STRIPE_SECRET_KEY` (503). It requires an identity-file
session, a payout pin that is not held
(`docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md` is what the
comment points at), `requireVerifiedAdult`, and `planAllowsCashout`. It
creates a Stripe Connect **Custom** account (`type: custom`,
`capabilities[transfers][requested]`) and an account link back to
`/bliss`. The id is stored as `user.stripe_connect_id`.

The cron that starts yesterday's settlement is hourly, not only at
midnight: `infrastructure/cloudflare/api/wrangler.toml` `[triggers] crons =
["0 * * * *"]`, and `scheduled` in `index.js` calls
`treasuryCall(env, 'start_settlement', { date: scoreDate })`. The comment
there says starting twice does nothing. An admin can start the same job
with `handleDailyPayout`.

`eustress/crates/web/src/pages/docs_earning.rs` ("USD Payouts") describes
this drip and says a connected account is required to take part in the
split. That sentence does not match `payout_setup`: the code still counts
an unconnected contributor in the weight total, then refuses to transfer
their cents. **Which of those a reader should believe is the code.** The
page was not re-checked against a running site.

### What the older audit pages say

`docs/AUDIT/09_ECONOMY.md` and `docs/AUDIT/MASTER.md` call Stripe Connect
a stub and the economy backend roughly 5–10% done (notes dated around
2026-05-14 in the economy audit). The Worker source above is past that
description: onboard, transfer, idempotency, and holds are in the tree.
Those audit pages are **not** a measurement of production, and they
disagree with the current source. They are cited so a reader does not
treat them as the rail.

## What "hosting pays" would have to be

Not built. The later branch adds a contribution the witness measures,
then lets the settlement that already exists split BLS and USD. It does
not call Stripe on its own.

- **Uptime** is time a host's listing stayed live: heartbeats accepted by
  `live.mjs`, bounded the way ActiveTime is bounded by presence. A host
  that stops beating stops accruing. The 90 second TTL is the liveness
  rule today. It is not yet an accrued duration.
- **Quality** is the other input McKale named. No definition is in this
  tree (tick time, crash, player report, badge). **Unverified and
  unspecified.** Until it is defined, a payout formula that multiplies by
  quality has nothing to multiply.
- The score is written by the witness into the same day record the
  settlement already sums (`contrib:{day}:{userId}`), or into a sibling
  key the settlement is taught to read. A new `contribution_type` has to
  be added to the weight table in `handleCosign` **and** to the table in
  `docs_earning.rs`, or the settlement reads a server-only key and the
  client cannot submit it. The second is the one that matches the cosign
  security note: the client must not be the source of hosting minutes.
- The account that is paid is the listing author's account, because that
  is the account `stripe_connect_id` and `planAllowsCashout` are checked
  on. A machine that hosts for someone else's listing is **not**
  specified. The live route only lets the author, or a server credential
  tied to that author, mark a listing live.
- BLS credit and USD transfer stay the settlement's job. Idempotency keys
  and the ledger's once-per-day emission id stay as they are. A hosting
  score that is added after the settlement ran for that date does not get
  a second payout for the same day. `MAX_CREDIT_AGE_DAYS` is 5, and a
  score date older than that fails the credit phase rather than landing
  behind a ledger checkpoint.
- USD still requires the Connect account, the cash-out plan, the age
  gate, and at least 50 cents. A host who earns BLS and has none of those
  is the unconnected row the treasury already knows how to leave in the
  pool.

## Open, on purpose

- The weight of a hosting minute, next to Development at 3.0 and
  ActiveTime at 1.0. Not chosen here.
- How quality changes that weight. Not defined here.
- Whether several hosts of one listing each earn, or only the author.
  The live registry is still one `host` record per listing.
- July 1 as the day the rail went live. Stated by McKale. Not in this
  tree.
- No check was made against `api.eustress.dev` or Stripe. Code present
  in the Worker is not the same fact as a transfer that has been paid.
