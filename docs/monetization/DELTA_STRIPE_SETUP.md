# Stripe prices for the subscription plans and Delta packs

**Status:** test mode is set up and checked (`all 14 prices are right`). Live mode is not set up; it is created only on McKale's word.

The Worker finds each price by its **lookup key** and reads what it grants from the price's **metadata**, so amounts can be tuned in Stripe without a code change. Pack keys name the tier, not the amount, for the same reason. `scripts/check-stripe-prices.mjs` checks the result against this table.

Delta is sold only through Stripe: on the web, and from Studio through the system browser. No app store sells Delta.

## What gets created

Five products, then fourteen prices.

| Product | Prices |
|---|---|
| Eustress Player Plus | monthly $4.99, yearly $49.99 |
| Eustress Creator | monthly $9.99, yearly $99.99 |
| Eustress Professional | monthly $129, yearly $1,290 |
| Eustress Bundle | monthly $12.99, yearly $129.99 |
| Eustress Delta | six one-time packs |

| Lookup key | Price | Metadata |
|---|---|---|
| `eustress_player_plus_monthly` | $4.99 a month | `eustress_plan=player_plus`, `tickets_per_invoice=450` |
| `eustress_player_plus_yearly` | $49.99 a year | `eustress_plan=player_plus`, `tickets_per_invoice=5400` |
| `eustress_creator_monthly` | $9.99 a month | `eustress_plan=creator`, `delta_per_invoice=1000` |
| `eustress_creator_yearly` | $99.99 a year | `eustress_plan=creator`, `delta_per_invoice=12000` |
| `eustress_professional_monthly` | $129 a month | `eustress_plan=professional`, `delta_per_invoice=16000` |
| `eustress_professional_yearly` | $1,290 a year | `eustress_plan=professional`, `delta_per_invoice=192000` |
| `eustress_bundle_monthly` | $12.99 a month | `eustress_plan=bundle`, `tickets_per_invoice=400`, `delta_per_invoice=900` |
| `eustress_bundle_yearly` | $129.99 a year | `eustress_plan=bundle`, `tickets_per_invoice=4800`, `delta_per_invoice=10800` |
| `eustress_delta_session` | $10 once | `type=delta_purchase`, `delta=1000` |
| `eustress_delta_project` | $25 once | `type=delta_purchase`, `delta=2600` |
| `eustress_delta_week` | $50 once | `type=delta_purchase`, `delta=5400` |
| `eustress_delta_sprint` | $100 once | `type=delta_purchase`, `delta=11200` |
| `eustress_delta_month` | $200 once | `type=delta_purchase`, `delta=23000` |
| `eustress_delta_studio` | $500 once | `type=delta_purchase`, `delta=60000` |

Amounts are in cents, in USD. A lookup key can belong to only one active price, so running the commands twice fails on the first duplicate instead of making a second set.

Creator, Professional and Bundle include Bliss cash-out (with identity verification). Player Plus does not.

## How long Delta lasts

- A pack, the free trial (1,000 Delta on identity verification) and Delta restored after a won dispute: 12 months.
- A plan grant: its own billing period plus one more, at most a month more. A monthly plan's grant is spent or gone by the end of the following month, so at most two grants are ever unspent at once.
- Charges draw the Delta that expires first. A refund removes the Delta that payment bought first.

## Before running anything

1. Which account? The Eustress account is the Stripe CLI profile `default`. Pass `--project-name default` on every command; the CLI's global active profile belongs to another business.
2. Which mode? The CLI uses **test mode** unless a command carries `--live`. Add `--live` only once McKale has approved live. Staging's Worker holds a test-mode key, so test mode is what staging needs.
3. Webhooks are set up separately (`checkout.session.completed`, `checkout.session.async_payment_succeeded`, `invoice.paid`, `customer.subscription.updated`, `customer.subscription.deleted`, `charge.refunded`, `charge.dispute.created`, `charge.dispute.closed`).

## The commands

Run in Git Bash. The `field` helper reads one field out of the CLI's JSON with node (this machine has no `jq`). For live, set `MODE=--live`. The six Delta packs alone are `node infrastructure/cloudflare/api/scripts/stripe-delta-packs.mjs` (add `--live` for live mode; it asks you to type `live` first; `--check` only checks; `--dry-run` prints the calls). It runs in PowerShell as it is: a `bash` script run from PowerShell is WSL's bash, which has neither node nor stripe.

```bash
set -euo pipefail
MODE=""   # --live for live mode
# The profile first, --live LAST: the Stripe CLI rejects --live anywhere before the subcommand.
s() { stripe --project-name default "$@" $MODE; }

# field <name>: that field of the JSON the CLI printed.
field() { node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);if(j.error){console.error(j.error.message);process.exit(1)}console.log(j[process.argv[1]])})' "$1"; }

product() { s products create -d "name=$1" | field id; }

PLAYER=$(product "Eustress Player Plus")
CREATOR=$(product "Eustress Creator")
PRO=$(product "Eustress Professional")
BUNDLE=$(product "Eustress Bundle")
DELTA=$(product "Eustress Delta")

# price <product> <key> <cents> <interval|-> [metadata key=value ...]
price() {
  local prod=$1 key=$2 cents=$3 interval=$4; shift 4
  local args=(-d "product=$prod" -d "currency=usd" -d "unit_amount=$cents" -d "lookup_key=$key")
  [ "$interval" != "-" ] && args+=(-d "recurring[interval]=$interval")
  for kv in "$@"; do args+=(-d "metadata[${kv%%=*}]=${kv#*=}"); done
  echo "$key $(s prices create "${args[@]}" | field id)"
}

price "$PLAYER"  eustress_player_plus_monthly   499    month eustress_plan=player_plus tickets_per_invoice=450
price "$PLAYER"  eustress_player_plus_yearly    4999   year  eustress_plan=player_plus tickets_per_invoice=5400
price "$CREATOR" eustress_creator_monthly       999    month eustress_plan=creator delta_per_invoice=1000
price "$CREATOR" eustress_creator_yearly        9999   year  eustress_plan=creator delta_per_invoice=12000
price "$PRO"     eustress_professional_monthly  12900  month eustress_plan=professional delta_per_invoice=16000
price "$PRO"     eustress_professional_yearly   129000 year  eustress_plan=professional delta_per_invoice=192000
price "$BUNDLE"  eustress_bundle_monthly        1299   month eustress_plan=bundle tickets_per_invoice=400 delta_per_invoice=900
price "$BUNDLE"  eustress_bundle_yearly         12999  year  eustress_plan=bundle tickets_per_invoice=4800 delta_per_invoice=10800

price "$DELTA"   eustress_delta_session  1000   - type=delta_purchase delta=1000
price "$DELTA"   eustress_delta_project  2500   - type=delta_purchase delta=2600
price "$DELTA"   eustress_delta_week     5000   - type=delta_purchase delta=5400
price "$DELTA"   eustress_delta_sprint   10000  - type=delta_purchase delta=11200
price "$DELTA"   eustress_delta_month    20000  - type=delta_purchase delta=23000
price "$DELTA"   eustress_delta_studio   50000  - type=delta_purchase delta=60000
```

Each `price` call prints `<lookup key> <price id>`.

## Check the result

```bash
stripe --project-name default prices list --limit 100 | node infrastructure/cloudflare/api/scripts/check-stripe-prices.mjs
```

It prints `all 14 prices are right`, or one line per problem (a missing key, a wrong amount, a wrong interval, a missing grant in the metadata, two active prices on one key, or an active `eustress_` key that is not in the table) and exits 1. Add `--live` at the end of the `stripe` command to check live mode (the CLI rejects it before the subcommand).

## Changing an amount

A price's amount cannot be edited: create a new price with `-d transfer_lookup_key=true` so the key moves to it, then archive the old one with `prices update <price id> --active=false`. A grant (the metadata) and a lookup key can be edited in place with `prices update`. The Worker caches the pack list for ten minutes.

## Tickets packs and treasury contributions

These two are sold from tables in the Worker (`TICKET_PACKAGES` and `STRIPE_PRICES` in `src/index.js`), not by lookup key, and their Stripe objects are live-mode only: checked read-only against live mode on 2026-10-05, all active, and the amounts match the tables (Tickets $4.99, $9.99, $19.99, $49.99, $99.99; treasury $25, $100, $500, $1,000 once, and monthly).

Under a **test-mode key** those live objects do not exist, so the Worker prices the purchase inline at the same amount (`isTestKey`, `TIER_CENTS`): a test-mode staging site can buy a Tickets pack or fund the treasury with no Stripe setup. A live key keeps using the live prices, unchanged. The webhook credits by the session's metadata (`package`, `tickets`, `user_id`), never by price id, so the credit is the same either way. `tests/stripe_test_mode_checkout.test.mjs` pins both.

Where Stripe returns the buyer:

| Purchase | Paid | Cancelled |
|---|---|---|
| Delta pack | `/delta/thanks` | `/delta?cancelled=1` |
| Tickets pack | `/tickets?purchased=true` | `/tickets?purchased=false` |
| Treasury contribution | `/bliss?funded=true` | `/bliss?funded=false` |

Each page says what the address says, and its buy button opens only an address on `checkout.stripe.com`.
