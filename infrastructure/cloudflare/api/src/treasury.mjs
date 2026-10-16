// =============================================================================
// Treasury: the USD pool contributors are paid from, and the nightly settlement
// =============================================================================
//
// One Durable Object holds the treasury. Money needs what KV cannot give: KV
// has no compare-and-set, it throws on a second write to one key within a
// second, and a Stripe delivery retried after a partial failure would add a
// deposit twice. Here every change to the pool is one operation, applied in
// integer cents and keyed by its source (a Checkout Session, an invoice, a
// refund, a dispute), so a repeat of it changes nothing.
//
// The same object runs the nightly settlement of each score date:
//
//   init            fix the day's emission ceiling from the supply
//   collect_effort  read the day's effort scores (contrib:{day}:*)
//   collect_value   read the day's filed sales (vscore:{day}:*)
//   value_finalize  sum each creator's sales with the caps applied
//   total           the day's network score, and the emission pool
//   backfill        give ledger entries without metadata their metadata
//   credit          each contributor's share of the emission, to the ledger
//   supply          the supply counters and the day's distribution record
//   payout_setup    the USD drip, effort-gated like the emission
//   payout_plan     each contributor's share of the drip
//   payout          Stripe transfers to connected contributors; a share for an
//                   account under a key-replacement freeze becomes a hold
//   payout_finish   the high-water mark and the day's payout record
//   release_holds   pay the holds (from any day) whose freeze has ended
//   record_holds    the paid holds into their days' payout records
//   backup         the BLS ledger to R2, one part per page of entries
//   cleanup         this object's working records for the day
//
// Each step does a bounded amount of work on the object's alarm and saves
// where it stopped, so no invocation comes near the 1,000 KV operations one
// invocation may make, and a crash resumes from the last saved step. Steps
// run one at a time, so a run never races itself, and score dates run in
// order, because each day's emission is computed from the supply the day
// before left.
//
// Storage:
//   state                           { cents, hwm_cents, platform_cents,
//                                     fees_cents, paid_cents, reversed_cents,
//                                     deposit_count, version, seeded_at }
//   dep:{ref}                       a deposit and how much of it is reversed
//   pi:{payment_intent}             deposit ref by PaymentIntent, for refunds
//   rev:{ref}                       a reversal (refund, dispute) or restoration
//   agg:{YYYY-MM}                   one month's running totals: deposits by kind,
//                                     reversals and restorations by the kind of
//                                     deposit they touch
//   active                          the score date being settled
//   queue:{date}                    a score date waiting its turn
//   job:{date}                      one score date's settlement
//   u:{date}:{userId}               one contributor's part of it
//   vs:{date}:{creator}:{buyer}:{ref}  one filed sale's Tickets
// =============================================================================

import {
  BLISS_UNIT, FULL_DAY_SCORE, FOLD_HORIZON_MS, VALUE_SCORE_PER_TICKET,
  VALUE_CAP_TICKETS_PER_BUYER, VALUE_CAP_SCORE_PER_CREATOR,
  blissEmissionRate, yearsSince, readSupplyMinor, readDistributedMinor,
  ledgerAppend, ledgerReconcile, ledgerMigrateUser, readEntry, backfillEntryMetadata,
  readContrib, readSale, parseSaleKey, fromMinor,
} from './bliss.mjs';
import { planAllowsCashout } from './delta.mjs';
import { valueFrozenUntil, payoutPinnedUntil } from './fresh_auth.mjs';

// -----------------------------------------------------------------------------
// Policy
// -----------------------------------------------------------------------------

/// Contributors' share of a Ticket sale's net: what arrives after the
/// storefront or processor takes its fee. On the web that is about 48% of
/// gross; through a 30% app store, 35%.
export const TREASURY_SPLIT = 0.50;

/// What a deposit can be: a Ticket pack, a Delta pack, a subscription
/// invoice, or a direct contribution to the treasury.
export const DEPOSIT_KINDS = ['ticket_purchase', 'delta_purchase', 'subscription', 'treasury_fund'];

/// Storefront or processor fee by sales channel, taken before the split. An
/// unknown channel is charged web pricing, never assumed free.
export const CHANNEL_FEES = {
  web: { rate: 0.029, flat_cents: 30 }, // Stripe standard
  ios: { rate: 0.30, flat_cents: 0 }, // App Store
  android: { rate: 0.30, flat_cents: 0 }, // Play Store
  steam: { rate: 0.30, flat_cents: 0 },
};

export const TREASURY_DRIP_RATE = 0.00276; // 0.276% of the treasury a day
export const TREASURY_SCARCITY_RATE = 0.00136; // 0.136% a day while scarce
export const TREASURY_SCARCITY_RATIO = 0.15; // scarce at or below 15% of the high-water mark
export const TREASURY_TOP_BOOST = 2.0; // weight of the top contributors while scarce
export const TREASURY_TOP_FRACTION = 0.25; // "top" is the top 25% by score
export const TREASURY_HWM_DECAY = 0.00171; // the high-water mark decays 0.171% a day

/// Stripe does not transfer less than this; a smaller share stays in the pool.
export const STRIPE_MIN_TRANSFER_CENTS = 50;

/// A score date older than this is not credited, so every emission credit
/// lands well inside the ledger's FOLD_HORIZON_MS (7 days).
export const MAX_CREDIT_AGE_DAYS = 5;

// Work per step. Each is sized so a step stays far below the 1,000 KV
// operations an invocation may make, even when every item needs a read.
const KV_BUDGET = 900;
const COLLECT_PAGE = 400;
const VALUE_PAGE = 400;
const STORAGE_PAGE = 1000;
const CREDIT_BATCH = 40;
const PAYOUT_BATCH = 20;
/// The share of a day's transfers Stripe would not take above which the
/// payout record and cron-health raise an alert.
const STRIPE_SHORTFALL_ALERT_SHARE = 0.2;
/// A hold made from a day's failed transfer is first tried this long after,
/// so a request that timed out on our side but completed at Stripe has long
/// finished before the hold looks for it.
const HOLD_FIRST_TRY_DELAY_MS = 3600e3;
/// After this many attempts, a rejection that is neither the destination's
/// fault nor ours (a short balance, a bad key) waits for review.
export const HOLD_MAX_REJECTED_ATTEMPTS = 10;
/// A payout waiting out a key-replacement freeze, one per account and day.
const holdKey = (userId, date) => `hold:${userId}:${date}`;
/// A hold set aside for review goes back to the treasury after this long.
export const HOLD_REVIEW_RETENTION_MS = 90 * 86400e3;
const BACKFILL_PAGE = 400;
const BACKUP_PAGE_CLEAN = 1000;
const BACKUP_PAGE_LEGACY = 400;
const PAYOUT_MAX_ATTEMPTS = 3;

// -----------------------------------------------------------------------------
// Money math
// -----------------------------------------------------------------------------

const isCents = (n) => Number.isSafeInteger(n) && n >= 0;

/// A Ticket sale of `grossCents` split into the storefront fee, the
/// contributors' treasury share and the platform's share, in whole cents that
/// add up to the gross.
export function splitSale(grossCents, channel) {
  const gross = Math.max(0, Math.trunc(Number(grossCents) || 0));
  const f = CHANNEL_FEES[channel] || CHANNEL_FEES.web;
  const fee = Math.min(gross, Math.round(gross * f.rate) + f.flat_cents);
  const net = gross - fee;
  const treasury = Math.floor(net * TREASURY_SPLIT);
  return { gross_cents: gross, fee_cents: fee, net_cents: net, treasury_cents: treasury, platform_cents: net - treasury };
}

/// The day's USD drip for a treasury of `cents` whose high-water mark is
/// `hwmCents`, and whether it is in scarcity.
export function dripFor(cents, hwmCents) {
  const hwm = Math.max(hwmCents, cents);
  const scarce = cents > 0 && cents <= hwm * TREASURY_SCARCITY_RATIO;
  const rate = scarce ? TREASURY_SCARCITY_RATE : TREASURY_DRIP_RATE;
  return { drip_cents: Math.floor(cents * rate), scarce, hwm_cents: hwm };
}

/// The high-water mark after a day's payout: it decays toward the treasury
/// balance before the payout, so one large deposit cannot hold the treasury
/// in scarcity forever, and it never falls below the balance now.
export function decayedHwm(hwmBefore, treasuryBefore, treasuryNow) {
  let hwm = hwmBefore;
  if (hwm > treasuryBefore) hwm = Math.max(treasuryBefore, Math.floor(hwm * (1 - TREASURY_HWM_DECAY)));
  return Math.max(hwm, treasuryNow);
}

/// The state the object starts from the first time it opens: the balances
/// the KV keys held before the treasury moved here.
export async function seedState(env) {
  const usd = async (key) => parseFloat((await env.PAYOUTS.get(key)) || '0') || 0;
  const cents = (v) => Math.round(v * 100);
  const treasury = Math.max(0, cents(await usd('treasury:total_usd')));
  return {
    cents: treasury,
    hwm_cents: Math.max(treasury, cents(await usd('treasury:hwm'))),
    platform_cents: cents(await usd('platform:total_usd')),
    fees_cents: Math.max(0, cents(await usd('costs:storefront_fees'))),
    paid_cents: Math.max(0, cents(await usd('payouts:total_paid'))),
    reversed_cents: 0,
    deposit_count: parseInt((await env.PAYOUTS.get('treasury:deposit_count')) || '0', 10) || 0,
    version: 0,
    seeded_at: new Date().toISOString(),
  };
}

// -----------------------------------------------------------------------------
// Plumbing
// -----------------------------------------------------------------------------

export const treasuryStub = (env) => env.TREASURY.get(env.TREASURY.idFromName('treasury'));

/// Call one operation on the treasury. Every reply is an object with `ok`; a
/// failure carries `status`, `error` and usually `code`.
export async function treasuryCall(env, op, args = {}) {
  if (!env.TREASURY) return { ok: false, status: 503, error: 'Treasury is not configured on this deployment', code: 'treasury_unavailable' };
  const res = await treasuryStub(env).fetch(`https://treasury.internal/${op}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args),
  });
  let body;
  try {
    body = await res.json();
  } catch {
    body = { ok: false, error: `${op}: unreadable reply` };
  }
  if (!res.ok && body.ok !== false) body = { ...body, ok: false };
  if (body.ok === false && !body.status) body.status = res.status;
  return body;
}

/// The treasury's balances in dollars, for the readers that show them.
export function treasuryView(state) {
  const usd = (c) => (Number(c) || 0) / 100;
  return {
    treasury_usd: usd(state.cents),
    hwm_usd: usd(state.hwm_cents),
    platform_usd: usd(state.platform_cents),
    storefront_fees_usd: usd(state.fees_cents),
    total_paid_usd: usd(state.paid_cents),
    // Owed to contributors whose payouts wait out a key-replacement freeze;
    // not part of the treasury the drip is taken from.
    held_usd: usd(state.held_cents),
    // The part of held_usd set aside for an administrator.
    held_review_usd: usd(state.review_cents),
    reversed_usd: usd(state.reversed_cents),
    deposit_count: state.deposit_count || 0,
  };
}

const fail = (status, error, code, extra = {}) => ({ ok: false, status, error, code, ...extra });

class BudgetExhausted extends Error {
  constructor() {
    super('KV operation budget for this step is spent');
  }
}

/// `env` with its KV namespaces counting operations, so a step stops before
/// the platform's per-invocation limit instead of failing at it.
function metered(env, budget) {
  const wrap = (ns) => ns && {
    get: (...a) => (budget.spend(), ns.get(...a)),
    getWithMetadata: (...a) => (budget.spend(), ns.getWithMetadata(...a)),
    put: (...a) => (budget.spend(), ns.put(...a)),
    delete: (...a) => (budget.spend(), ns.delete(...a)),
    list: (...a) => (budget.spend(), ns.list(...a)),
  };
  return {
    ...env,
    PAYOUTS: wrap(env.PAYOUTS),
    INVENTORY: wrap(env.INVENTORY),
    USERS: wrap(env.USERS),
    SOCIAL: wrap(env.SOCIAL),
  };
}

function newBudget(limit = KV_BUDGET) {
  return {
    used: 0,
    spend() {
      this.used += 1;
      if (this.used > limit) throw new BudgetExhausted();
    },
    left() {
      return limit - this.used;
    },
  };
}

async function putMany(storage, writes) {
  const entries = Object.entries(writes);
  for (let i = 0; i < entries.length; i += 128) await storage.put(Object.fromEntries(entries.slice(i, i + 128)));
}

const dayMs = (date) => Date.parse(`${date}T00:00:00.000Z`);
const isDate = (d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(d) && !Number.isNaN(dayMs(d));
const nowIso = () => new Date().toISOString();

/// Retry delay after a failed step: 30 s doubling to at most an hour. A
/// settlement is never abandoned, because it owes people money.
const retryDelay = (attempts) => Math.min(3600e3, 30e3 * 2 ** Math.max(0, attempts - 1));

function contributorScore(u) {
  return (Number(u.effort) || 0) + (Number(u.recorded_value) || 0) + (Number(u.value) || 0);
}

async function stripeTransfer(env, { amountCents, destination, idempotencyKey, description, metadata }) {
  const body = new URLSearchParams({ amount: String(amountCents), currency: 'usd', destination, description });
  for (const [k, v] of Object.entries(metadata || {})) body.set(`metadata[${k}]`, String(v));
  let res;
  try {
    res = await fetch('https://api.stripe.com/v1/transfers', {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${env.STRIPE_SECRET_KEY}`,
        'Content-Type': 'application/x-www-form-urlencoded',
        'Idempotency-Key': idempotencyKey,
      },
      body: body.toString(),
      signal: AbortSignal.timeout(20_000),
    });
  } catch (e) {
    return { ok: false, retryable: true, error: `network: ${e?.message || e}` };
  }
  let data = null;
  try {
    data = await res.json();
  } catch {
    data = null;
  }
  if (res.ok && data?.id) return { ok: true, id: data.id };
  // 409 is Stripe reporting the same idempotency key in flight elsewhere.
  const retryable = res.status === 409 || res.status === 429 || res.status >= 500;
  return {
    ok: false, retryable, status: res.status, error: data?.error?.message || `HTTP ${res.status}`,
    code: data?.error?.code || null, param: data?.error?.param || null,
  };
}

/// Stripe error codes that mean the destination itself cannot be paid (a
/// closed or invalid Connect account, transfers not enabled), as opposed to
/// a fault of ours or of the day (a short platform balance, a bad key).
const DESTINATION_FAULTS = new Set([
  'account_closed', 'account_invalid', 'insufficient_capabilities_for_transfer', 'transfers_not_allowed',
]);
const isDestinationFault = (t) =>
  DESTINATION_FAULTS.has(t.code) || (t.code === 'resource_missing' && t.param === 'destination');

/// Whether Stripe holds a payout transfer for `userId` and `date`, made since
/// `since`. `{id}` when it does, `{checked: true}` when it certainly does
/// not, `{error}` when Stripe could not be asked.
async function findTransfer(env, { destination, date, userId, since }) {
  const created = Math.floor((Date.parse(since) || Date.now() - 7 * 86400e3) / 1000) - 3600;
  let startingAfter = null;
  for (let page = 0; page < 20; page++) {
    const q = new URLSearchParams({ destination, limit: '100', 'created[gte]': String(created) });
    if (startingAfter) q.set('starting_after', startingAfter);
    let res;
    try {
      res = await fetch(`https://api.stripe.com/v1/transfers?${q}`, {
        headers: { Authorization: `Bearer ${env.STRIPE_SECRET_KEY}` },
        signal: AbortSignal.timeout(20_000),
      });
    } catch (e) {
      return { error: `network: ${e?.message || e}` };
    }
    let data = null;
    try {
      data = await res.json();
    } catch {
      data = null;
    }
    if (!res.ok || !Array.isArray(data?.data)) return { error: data?.error?.message || `HTTP ${res.status}` };
    const hit = data.data.find((t) => t.metadata?.user_id === userId && t.metadata?.date === date && !t.reversed);
    if (hit) return { id: hit.id };
    if (!data.has_more || data.data.length === 0) return { checked: true };
    startingAfter = data.data[data.data.length - 1].id;
  }
  return { error: 'too many transfers to search' };
}

// =============================================================================
// The Durable Object
// =============================================================================

export class Treasury {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
    this.opened = false;
    this.tail = null;
    this.stepping = null;
  }

  async fetch(request) {
    const op = new URL(request.url).pathname.slice(1);
    let args = {};
    try {
      const v = await request.json();
      if (v && typeof v === 'object' && !Array.isArray(v)) args = v;
    } catch {
      args = {};
    }
    let result;
    try {
      await this.open();
      const handler = TREASURY_OPS[op];
      result = handler ? await handler.call(this, args) : fail(404, `unknown operation ${op}`, 'unknown_operation');
    } catch (e) {
      console.error(`Treasury.${op} failed:`, e);
      result = fail(500, String(e?.message || e), 'internal_error');
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  async open() {
    if (this.opened) return;
    await this.ctx.blockConcurrencyWhile(async () => {
      if (this.opened) return;
      if (!(await this.storage.get('state'))) await this.storage.put('state', await seedState(this.env));
      this.opened = true;
    });
  }

  /// Run `fn` after every earlier critical section of this object has
  /// finished. Money operations are read-modify-writes of `state`; the queue
  /// keeps them whole even if one awaits something other than storage.
  atomic(fn) {
    const run = (this.tail || Promise.resolve()).then(() => fn());
    this.tail = run.then(() => {}, () => {});
    return run;
  }

  async alarm() {
    await this.open();
    await this.step();
  }

  /// Run one step of the active settlement, one step at a time.
  step() {
    if (!this.stepping) {
      this.stepping = this.runStep().finally(() => {
        this.stepping = null;
      });
    }
    return this.stepping;
  }

  async runStep() {
    const date = await this.storage.get('active');
    if (!date) return { idle: true };
    const job = await this.storage.get(`job:${date}`);
    if (!job || job.phase === 'done' || job.phase === 'failed') {
      await this.startNext();
      return { idle: true };
    }
    const next = structuredClone(job);
    const phase = next.phase;
    try {
      await PHASES[phase].call(this, next, newBudget());
    } catch (e) {
      const failure = { at: nowIso(), phase, error: String(e?.message || e) };
      console.error(`settlement ${date} ${phase} failed:`, e);
      job.attempts = (job.attempts || 0) + 1;
      job.last_error = failure;
      job.errors = [...(job.errors || []).slice(-9), failure];
      job.updated_at = failure.at;
      await this.storage.put(`job:${date}`, job);
      await this.storage.setAlarm(Date.now() + retryDelay(job.attempts));
      return { ok: false, phase, error: failure.error };
    }
    const delay = next.delay_ms || 10;
    delete next.delay_ms;
    next.attempts = 0;
    next.steps = (next.steps || 0) + 1;
    next.updated_at = nowIso();
    if (next.phase !== phase) next.phase_started_at = next.updated_at;
    await this.storage.put(`job:${date}`, next);
    if (next.phase === 'done' || next.phase === 'failed') {
      await this.recordSettlement(next);
      await this.startNext();
    } else {
      await this.storage.setAlarm(Date.now() + delay);
    }
    return { ok: true, phase: next.phase };
  }

  /// A transfer went through: count it paid.
  async settleTransfer(key, cents, u, transferId) {
    await this.atomic(async () => {
      const state = await this.storage.get('state');
      state.paid_cents += cents;
      state.version += 1;
      await this.storage.put({ state, [key]: { ...u, pay_status: 'paid', transfer_id: transferId, pay_error: null } });
    });
  }

  async hasReserved(prefix) {
    let startAfter;
    while (true) {
      const opts = { prefix, limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, u] of page) {
        startAfter = key;
        if (u.pay_status === 'reserved') return true;
      }
      if (page.size < STORAGE_PAGE) return false;
    }
  }

  /// Release one hold if it can be paid. Returns {paid: hold} when this call
  /// paid it, {returned: true} when its money went back to the treasury,
  /// {review: true} while it waits for an administrator, {unresolved: true}
  /// when it is to be tried again, {waiting: true} while the freeze lasts,
  /// and {} for a hold already paid and waiting to be recorded.
  async releaseHold(key, h) {
    if (h.status === 'paid') return {};
    if (h.status === 'returned') {
      await this.storage.delete(key);
      return {};
    }
    if (h.status === 'review') {
      if (Date.parse(h.review_since) + HOLD_REVIEW_RETENTION_MS <= Date.now()) {
        return this.returnHold(key, h, `review_expired:${h.review_reason}`);
      }
      return { review: true };
    }
    if (h.not_before && Date.now() < h.not_before) return { waiting: true };
    let frozen;
    try {
      frozen = await valueFrozenUntil(this.env, h.user_id);
    } catch (e) {
      console.error(`treasury: value freeze for ${h.user_id} unreadable, hold kept:`, e?.message || e);
      return { waiting: true };
    }
    if (frozen) {
      if (frozen !== h.frozen_until) await this.storage.put(key, { ...h, frozen_until: frozen });
      return { waiting: true };
    }
    const raw = await this.env.USERS.get(`user:${h.user_id}`);
    let user = null;
    try {
      user = raw ? JSON.parse(raw) : null;
    } catch {
      return { unresolved: true };
    }
    if (!user) return this.reviewHold(key, h, 'account_gone');
    if (user.banned) return this.reviewHold(key, h, 'banned');
    // After an attempt, an earlier one may have gone through with its answer
    // lost: ask Stripe before sending again.
    if (h.attempts > 0) {
      const found = await findTransfer(this.env, { destination: h.connect, date: h.date, userId: h.user_id, since: h.since || h.held_at });
      if (found.id) return this.settleHold(key, h, found.id);
      if (!found.checked) {
        await this.storage.put(key, { ...h, error: `lookup: ${found.error}` });
        return { unresolved: true };
      }
    }
    // Write the attempt down before Stripe is called, so a crash after Stripe
    // accepts it still leads the next attempt to look first.
    const attempts = h.attempts + 1;
    await this.storage.put(key, { ...h, attempts });
    const transfer = await stripeTransfer(this.env, {
      amountCents: h.cents,
      destination: h.connect,
      idempotencyKey: `bliss-hold-${h.date}-${h.user_id}-${attempts}`,
      description: `Bliss daily payout - ${h.username || h.user_id} (held after a key replacement)`,
      metadata: { user_id: h.user_id, date: h.date, hold: 'true' },
    });
    if (transfer.ok) return this.settleHold(key, { ...h, attempts }, transfer.id);
    // Only a fault of the destination sets the hold aside. Anything else (a
    // network failure, a short platform balance, a bad key) is tried again
    // next run, after asking Stripe whether this attempt went through.
    if (!isDestinationFault(transfer)) {
      const error = transfer.code ? `${transfer.code}: ${transfer.error}` : transfer.error;
      // A short platform balance or a bad key is ours, and alerted: tried
      // without limit, as is anything Stripe may answer differently later.
      // Any other rejection, once it has been seen often enough, is shown to
      // an administrator instead of being sent every day for ever.
      const ours = transfer.code === 'balance_insufficient' || transfer.status === 401 || transfer.status === 403;
      if (!transfer.retryable && !ours && attempts >= HOLD_MAX_REJECTED_ATTEMPTS) {
        const seen = await findTransfer(this.env, { destination: h.connect, date: h.date, userId: h.user_id, since: h.since || h.held_at });
        if (seen.id) return this.settleHold(key, { ...h, attempts }, seen.id);
        if (seen.checked) return this.reviewHold(key, { ...h, attempts, error }, 'stripe_rejected');
      }
      await this.storage.put(key, { ...h, attempts, error });
      return { unresolved: true };
    }
    const found = await findTransfer(this.env, { destination: h.connect, date: h.date, userId: h.user_id, since: h.since || h.held_at });
    if (found.id) return this.settleHold(key, { ...h, attempts }, found.id);
    if (found.checked) return this.reviewHold(key, { ...h, attempts, error: transfer.error }, 'destination_refused');
    await this.storage.put(key, { ...h, attempts, error: `${transfer.error}; lookup: ${found.error}` });
    return { unresolved: true };
  }

  async settleHold(key, h, transferId) {
    const paid = { ...h, status: 'paid', transfer_id: transferId, paid_at: nowIso(), error: null };
    await this.atomic(async () => {
      const state = await this.storage.get('state');
      state.held_cents = (state.held_cents || 0) - h.cents;
      state.paid_cents += h.cents;
      state.version += 1;
      await this.storage.put({ state, [key]: paid });
    });
    return { paid };
  }

  /// Set a hold aside for an administrator: it stays owed (in held_cents) and
  /// is not tried again until resolved, or returned after the retention.
  async reviewHold(key, h, reason) {
    await this.atomic(async () => {
      const state = await this.storage.get('state');
      state.review_cents = (state.review_cents || 0) + h.cents;
      state.version += 1;
      await this.storage.put({ state, [key]: { ...h, status: 'review', review_reason: reason, review_since: nowIso() } });
    });
    console.warn(`treasury: hold ${key} (${h.cents} cents) set aside for review: ${reason}`);
    return { review: true };
  }

  /// Put a hold's money back in the treasury.
  async returnHold(key, h, reason) {
    await this.atomic(async () => {
      const state = await this.storage.get('state');
      state.held_cents = (state.held_cents || 0) - h.cents;
      if (h.status === 'review') state.review_cents = (state.review_cents || 0) - h.cents;
      state.cents += h.cents;
      state.version += 1;
      await this.storage.put({ state, [key]: { ...h, status: 'returned', reason, returned_at: nowIso() } });
    });
    console.warn(`treasury: hold ${key} (${h.cents} cents) returned to the treasury: ${reason}`);
    await this.storage.delete(key);
    return { returned: true };
  }

  /// Add released holds to their day's payout record, once each.
  async recordReleases(date, holds) {
    const key = `payout:${date}`;
    const raw = await this.env.PAYOUTS.get(key);
    let record = null;
    try {
      record = raw ? JSON.parse(raw) : null;
    } catch {
      record = null;
    }
    if (!record) record = { date: nowIso(), score_date: date, total_paid_usd: 0, contributors_paid: 0, frozen_usd: 0, payouts: [] };
    if (!Array.isArray(record.payouts)) record.payouts = [];
    const usd = (c) => Math.round(c) / 100;
    const cents = (v) => Math.round((Number(v) || 0) * 100);
    for (const h of holds) {
      if (record.payouts.some((p) => p.user_id === h.user_id)) continue;
      record.payouts.push({
        user_id: h.user_id, username: h.username || null, amount_usd: usd(h.cents), amount_cents: h.cents,
        transfer_id: h.transfer_id, released_at: h.paid_at,
      });
      record.total_paid_usd = usd(cents(record.total_paid_usd) + h.cents);
      record.contributors_paid = (record.contributors_paid || 0) + 1;
      const bucket = { stripe_unavailable: 'stripe_unavailable_usd', destination_refused: 'destination_review_usd' }[h.reason] || 'frozen_usd';
      record[bucket] = usd(Math.max(0, cents(record[bucket]) - h.cents));
      record.released_usd = usd(cents(record.released_usd) + h.cents);
    }
    await this.env.PAYOUTS.put(key, JSON.stringify(record), { expirationTtl: 86400 * 365 * 5 });
  }

  async recordSettlement(job) {
    try {
      await this.env.PAYOUTS.put(`settlement:${job.date}`, JSON.stringify(jobSummary(job)), { expirationTtl: 86400 * 365 * 5 });
    } catch (e) {
      console.error(`settlement:${job.date} summary was not written:`, e);
    }
  }

  /// Make the oldest queued score date active, if the active one is finished.
  async startNext() {
    const date = await this.storage.get('active');
    if (date) {
      const job = await this.storage.get(`job:${date}`);
      if (job && job.phase !== 'done' && job.phase !== 'failed') return;
    }
    const queued = await this.storage.list({ prefix: 'queue:', limit: 1 });
    for (const key of queued.keys()) {
      const next = key.slice('queue:'.length);
      await this.storage.delete(key);
      if (!(await this.storage.get(`job:${next}`))) await this.storage.put(`job:${next}`, newJob(next));
      await this.storage.put('active', next);
      await this.storage.setAlarm(Date.now() + 10);
      return;
    }
  }
}

function newJob(date) {
  return {
    date,
    phase: 'init',
    created_at: nowIso(),
    updated_at: nowIso(),
    steps: 0,
    attempts: 0,
    errors: [],
    cursor: null,
  };
}

// -----------------------------------------------------------------------------
// Operations
// -----------------------------------------------------------------------------

const REF = /^[A-Za-z0-9_:.-]{1,200}$/;

// -----------------------------------------------------------------------------
// Month totals
// -----------------------------------------------------------------------------
//
// Each money write also adds to its calendar month's totals (UTC), in the same
// storage put, so the totals can never disagree with the records they count.
// A refund or dispute counts in the month it happens, against the kind of
// deposit it reverses; a dispute won counts in the month it is won.

const aggKey = (month) => `agg:${month}`;
const monthOf = (iso) => String(iso).slice(0, 7);
const MONTH = /^\d{4}-(0[1-9]|1[0-2])$/;
export const DEPOSIT_SUMS = ['gross_cents', 'fee_cents', 'treasury_cents', 'platform_cents', 'tickets', 'delta'];
export const REVERSAL_SUMS = ['gross_cents', 'treasury_cents', 'shortfall_cents', 'platform_cents', 'tickets', 'delta'];
const emptyMonth = (month) => ({ month, deposits: {}, reversals: {}, restored: {} });

/// Add one record to a month's totals, in `section` under `kind`.
export function addToMonth(totals, section, kind, rec, fields) {
  const rows = totals[section];
  const row = rows[kind] || (rows[kind] = { count: 0, ...Object.fromEntries(fields.map((k) => [k, 0])) });
  row.count += 1;
  for (const k of fields) row[k] += Number(rec[k]) || 0;
  return totals;
}

/// The totals of the month `iso` falls in, read inside the caller's critical section.
async function monthTotals(storage, iso) {
  const month = monthOf(iso);
  return (await storage.get(aggKey(month))) || emptyMonth(month);
}

/// What a dispute won puts back, in the reversal's fields: the platform gets back
/// what it bore for the treasury too.
const restoredSums = (rv) => ({
  gross_cents: rv.gross_cents, treasury_cents: rv.treasury_cents, shortfall_cents: rv.shortfall_cents || 0,
  platform_cents: rv.platform_cents, tickets: rv.tickets, delta: rv.delta || 0,
});

const TREASURY_OPS = {
  /// The running totals of the months asked for (YYYY-MM, at most 36), a month
  /// with no money in it as empty totals, and the balances now.
  async period_totals({ months } = {}) {
    if (!Array.isArray(months) || months.length === 0 || months.length > 36 || !months.every((m) => MONTH.test(m))) {
      return fail(400, 'months must be 1 to 36 YYYY-MM strings', 'invalid_request');
    }
    const found = await this.storage.get(months.map(aggKey));
    const state = await this.storage.get('state');
    return { ok: true, months: months.map((m) => found.get(aggKey(m)) || emptyMonth(m)), view: treasuryView(state), state };
  },

  /// Recompute every month's totals from the deposit and reversal records. For
  /// the first run after totals were added, and for repairs. Runs inside the
  /// critical section, so no deposit lands between the scan and the write.
  async rebuild_totals() {
    return this.atomic(async () => {
      const listAll = async (prefix) => {
        const out = new Map();
        let startAfter;
        for (;;) {
          const page = await this.storage.list({ prefix, startAfter, limit: STORAGE_PAGE });
          for (const [k, v] of page) out.set(k, v);
          if (page.size < STORAGE_PAGE) return out;
          startAfter = [...page.keys()].pop();
        }
      };
      const deps = await listAll('dep:');
      const revs = await listAll('rev:');
      const months = new Map();
      const of = (iso) => {
        const m = monthOf(iso);
        if (!months.has(m)) months.set(m, emptyMonth(m));
        return months.get(m);
      };
      for (const d of deps.values()) addToMonth(of(d.at), 'deposits', d.kind, d, DEPOSIT_SUMS);
      for (const r of revs.values()) {
        if (r.kind === 'dispute_won') {
          const rv = revs.get(`rev:${r.of}`);
          const dep = deps.get(`dep:${r.deposit_ref}`);
          if (rv && dep) addToMonth(of(r.at), 'restored', dep.kind, restoredSums(rv), REVERSAL_SUMS);
        } else {
          const dep = deps.get(`dep:${r.deposit_ref}`);
          addToMonth(of(r.at), 'reversals', r.deposit_kind || dep?.kind || 'unknown', r, REVERSAL_SUMS);
        }
      }
      const stale = [...(await listAll('agg:')).keys()].filter((k) => !months.has(k.slice(4)));
      for (let i = 0; i < stale.length; i += 128) await this.storage.delete(stale.slice(i, i + 128));
      const entries = [...months.values()].map((t) => [aggKey(t.month), t]);
      for (let i = 0; i < entries.length; i += 128) await this.storage.put(Object.fromEntries(entries.slice(i, i + 128)));
      return { ok: true, months: entries.length, deposits: deps.size, reversals: revs.size };
    });
  },

  async snapshot() {
    const state = await this.storage.get('state');
    const active = await this.storage.get('active');
    const job = active ? await this.storage.get(`job:${active}`) : null;
    return { ok: true, state, view: treasuryView(state), settlement: job ? jobSummary(job) : null };
  },

  /// The payouts held after key replacements, those set aside for review
  /// first. For administrators.
  async holds({ status = null } = {}) {
    const holds = [];
    let startAfter;
    while (true) {
      const opts = { prefix: 'hold:', limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, h] of page) {
        startAfter = key;
        if (!status || h.status === status) holds.push(h);
      }
      if (page.size < STORAGE_PAGE) break;
    }
    holds.sort((a, b) => (b.status === 'review') - (a.status === 'review') || (a.held_at < b.held_at ? -1 : 1));
    return { ok: true, holds };
  },

  /// An administrator's decision on a hold set aside for review: `pay` puts
  /// it back in the queue, and the next settlement pays it if the account is
  /// no longer banned; `return` puts its money back in the treasury.
  ///
  /// `pay` goes to the day's destination, or with `use_current_destination`
  /// to the account's own current one, read here from its record (set up
  /// through onboarding's identity, plan and pin checks) and only once the
  /// pin after a key replacement has passed. An administrator never names a
  /// destination: a stolen admin session must not be a way to move earnings.
  async resolve_hold({ user_id, date, action, use_current_destination = false, ...rest }) {
    if (typeof user_id !== 'string' || typeof date !== 'string') return fail(400, 'user_id and date are required', 'invalid_request');
    if (action !== 'pay' && action !== 'return') return fail(400, 'action must be pay or return', 'invalid_request');
    if ('destination' in rest) return fail(400, 'a destination cannot be named; use use_current_destination', 'invalid_request');
    const key = holdKey(user_id, date);
    const h = await this.storage.get(key);
    if (!h) return fail(404, 'No such hold', 'hold_not_found');
    if (h.status !== 'review') return fail(409, `The hold is ${h.status}, not waiting for review`, 'hold_not_in_review');
    if (action === 'return') {
      await this.returnHold(key, h, `admin_return:${h.review_reason}`);
      return { ok: true, status: 'returned', cents: h.cents };
    }
    let connect = h.connect;
    if (use_current_destination === true) {
      let user = null;
      try {
        const raw = await this.env.USERS.get(`user:${user_id}`);
        user = raw ? JSON.parse(raw) : null;
      } catch {
        user = null;
      }
      if (!user || typeof user.stripe_connect_id !== 'string' || !user.stripe_connect_id) {
        return fail(409, 'The account has no payout destination of its own', 'no_destination');
      }
      const pin = await payoutPinnedUntil(this.env, user_id);
      if (!pin.ok) return fail(503, 'Whether the destination is pinned cannot be checked now', 'pin_unknown');
      if (pin.until) {
        return fail(409, `The destination is pinned after a key replacement until ${new Date(pin.until).toISOString()}`, 'destination_pinned');
      }
      connect = user.stripe_connect_id;
    }
    await this.atomic(async () => {
      const state = await this.storage.get('state');
      state.review_cents = (state.review_cents || 0) - h.cents;
      state.version += 1;
      const next = { ...h, status: 'held', connect, review_reason: null, review_since: null, resolved_at: nowIso() };
      // A destination never sent to before starts its attempts afresh.
      if (connect !== h.connect) next.attempts = 0;
      await this.storage.put({ state, [key]: next });
    });
    return { ok: true, status: 'held', cents: h.cents, destination: connect === h.connect ? 'day' : 'current' };
  },

  /// Money in: a Ticket sale's treasury share, or a donation to the treasury.
  async deposit(args) {
    const {
      ref, kind, gross_cents, fee_cents = 0, treasury_cents, platform_cents = 0,
      user_id = null, payment_intent = null, tickets = 0, delta = 0, channel = null,
    } = args;
    if (typeof ref !== 'string' || !REF.test(ref)) return fail(400, 'invalid ref', 'invalid_request');
    if (!DEPOSIT_KINDS.includes(kind)) return fail(400, 'invalid kind', 'invalid_request');
    if (![gross_cents, fee_cents, treasury_cents, platform_cents, tickets, delta].every(isCents)) {
      return fail(400, 'amounts must be whole, non-negative cents', 'invalid_request');
    }
    if (fee_cents + treasury_cents + platform_cents !== gross_cents) {
      return fail(400, 'fee, treasury and platform shares must add up to the gross', 'invalid_request');
    }
    return this.atomic(async () => {
      const prior = await this.storage.get(`dep:${ref}`);
      if (prior) return { ok: true, replay: true, deposit: prior, state: await this.storage.get('state') };
      const state = await this.storage.get('state');
      state.cents += treasury_cents;
      state.platform_cents += platform_cents;
      state.fees_cents += fee_cents;
      state.deposit_count += 1;
      state.hwm_cents = Math.max(state.hwm_cents, state.cents);
      state.version += 1;
      const deposit = {
        ref, kind, gross_cents, fee_cents, treasury_cents, platform_cents, tickets, delta,
        user_id: typeof user_id === 'string' ? user_id : null,
        payment_intent: typeof payment_intent === 'string' ? payment_intent : null,
        channel: typeof channel === 'string' ? channel : null,
        at: nowIso(),
        refunded_cents: 0,
        reversed: { gross_cents: 0, treasury_cents: 0, platform_cents: 0, tickets: 0, delta: 0 },
      };
      const writes = { state, [`dep:${ref}`]: deposit };
      if (deposit.payment_intent) writes[`pi:${deposit.payment_intent}`] = ref;
      const totals = addToMonth(await monthTotals(this.storage, deposit.at), 'deposits', kind, deposit, DEPOSIT_SUMS);
      writes[aggKey(totals.month)] = totals;
      await this.storage.put(writes);
      return { ok: true, deposit, state };
    });
  },

  /// A refund of a deposit's payment. `refunded_total_cents` is the charge's
  /// cumulative `amount_refunded`, so each refund event reverses only what
  /// is new, and a repeat of an event reverses nothing.
  async refund({ payment_intent, refunded_total_cents }) {
    if (typeof payment_intent !== 'string' || !isCents(refunded_total_cents)) return fail(400, 'invalid refund', 'invalid_request');
    return this.atomic(async () => {
      const depRef = await this.storage.get(`pi:${payment_intent}`);
      const dep = depRef ? await this.storage.get(`dep:${depRef}`) : null;
      if (!dep) return fail(404, 'No deposit for that payment', 'deposit_not_found');
      const delta = refunded_total_cents - (dep.refunded_cents || 0);
      const ref = `refund:${payment_intent}:${refunded_total_cents}`;
      const prior = await this.storage.get(`rev:${ref}`);
      if (prior || delta <= 0) return { ok: true, replay: true, reversal: prior || null };
      dep.refunded_cents = refunded_total_cents;
      return reverseDeposit.call(this, dep, { ref, kind: 'refund', gross_cents: delta });
    });
  },

  /// A dispute opened on a deposit's payment. The disputed amount leaves the
  /// balance when it opens, so it is reversed then; `dispute_won` restores it.
  async dispute({ payment_intent, dispute_id, amount_cents }) {
    if (typeof payment_intent !== 'string' || typeof dispute_id !== 'string' || !isCents(amount_cents)) {
      return fail(400, 'invalid dispute', 'invalid_request');
    }
    return this.atomic(async () => {
      const ref = `dispute:${dispute_id}`;
      const prior = await this.storage.get(`rev:${ref}`);
      if (prior) return { ok: true, replay: true, reversal: prior };
      const depRef = await this.storage.get(`pi:${payment_intent}`);
      const dep = depRef ? await this.storage.get(`dep:${depRef}`) : null;
      if (!dep) return fail(404, 'No deposit for that payment', 'deposit_not_found');
      return reverseDeposit.call(this, dep, { ref, kind: 'dispute', gross_cents: amount_cents });
    });
  },

  /// A dispute closed in the platform's favour: undo its reversal.
  async dispute_won({ dispute_id }) {
    if (typeof dispute_id !== 'string') return fail(400, 'invalid dispute', 'invalid_request');
    return this.atomic(async () => {
      const ref = `dispute-won:${dispute_id}`;
      const prior = await this.storage.get(`rev:${ref}`);
      if (prior) return { ok: true, replay: true, reversal: prior };
      const rv = await this.storage.get(`rev:dispute:${dispute_id}`);
      if (!rv) return fail(404, 'No reversal for that dispute', 'reversal_not_found');
      const dep = await this.storage.get(`dep:${rv.deposit_ref}`);
      const state = await this.storage.get('state');
      state.cents += rv.treasury_cents;
      state.platform_cents += rv.platform_cents + rv.shortfall_cents;
      state.reversed_cents -= rv.gross_cents;
      state.hwm_cents = Math.max(state.hwm_cents, state.cents);
      state.version += 1;
      const r = dep.reversed;
      dep.reversed = {
        gross_cents: r.gross_cents - rv.gross_cents,
        treasury_cents: r.treasury_cents - rv.treasury_cents - rv.shortfall_cents,
        platform_cents: r.platform_cents - rv.platform_cents,
        tickets: r.tickets - rv.tickets,
        delta: (r.delta || 0) - (rv.delta || 0),
      };
      const restoration = {
        ref, kind: 'dispute_won', of: rv.ref, deposit_ref: rv.deposit_ref, gross_cents: rv.gross_cents,
        treasury_cents: rv.treasury_cents, tickets: rv.tickets, delta: rv.delta || 0, user_id: rv.user_id, at: nowIso(),
      };
      const totals = addToMonth(await monthTotals(this.storage, restoration.at), 'restored', dep.kind, restoredSums(rv), REVERSAL_SUMS);
      await this.storage.put({ state, [`dep:${rv.deposit_ref}`]: dep, [`rev:${ref}`]: restoration, [aggKey(totals.month)]: totals });
      return { ok: true, reversal: restoration };
    });
  },

  /// Begin settling `date` (a score date, YYYY-MM-DD), or wait for the one
  /// being settled. Repeats are harmless: a started date is left alone, and
  /// its alarm is set again in case it was lost.
  async start_settlement({ date }) {
    if (!isDate(date)) return fail(400, 'date must be YYYY-MM-DD', 'invalid_request');
    if (Date.now() < dayMs(date) + 86400e3) return fail(409, `${date} has not ended yet`, 'day_not_over');
    const existing = await this.storage.get(`job:${date}`);
    if (existing) {
      if (existing.phase !== 'done' && existing.phase !== 'failed') {
        const active = await this.storage.get('active');
        if (active === date && !(await this.storage.getAlarm())) await this.storage.setAlarm(Date.now() + 10);
      }
      return { ok: true, started: false, settlement: jobSummary(existing) };
    }
    const active = await this.storage.get('active');
    const activeJob = active ? await this.storage.get(`job:${active}`) : null;
    if (activeJob && activeJob.phase !== 'done' && activeJob.phase !== 'failed') {
      await this.storage.put(`queue:${date}`, 1);
      if (!(await this.storage.getAlarm())) await this.storage.setAlarm(Date.now() + 10);
      return { ok: true, started: false, queued: true, behind: active };
    }
    const job = newJob(date);
    await this.storage.put({ [`job:${date}`]: job, active: date });
    await this.storage.setAlarm(Date.now() + 10);
    return { ok: true, started: true, settlement: jobSummary(job) };
  },

  async settlement({ date }) {
    const d = date || (await this.storage.get('active'));
    const job = d ? await this.storage.get(`job:${d}`) : null;
    return job ? { ok: true, settlement: jobSummary(job) } : fail(404, 'No settlement for that date', 'resource_missing');
  },

  /// Run steps now rather than on the alarm, up to `max`. For tests and the
  /// admin escape hatch.
  async run_steps({ max = 1 } = {}) {
    let last = null;
    for (let i = 0; i < Math.min(Math.max(1, Number(max) || 1), 10_000); i++) {
      last = await this.step();
      if (last.idle || last.ok === false) break;
    }
    return { ok: true, last };
  },
};

/// Reverse part of a deposit, inside the caller's critical section. Shares
/// are cumulative, so partial reversals of one deposit add up exactly to the
/// whole of it. What the treasury no longer holds (already paid out) is
/// borne by the platform.
async function reverseDeposit(dep, { ref, kind, gross_cents }) {
  const state = await this.storage.get('state');
  const r = dep.reversed;
  const gross = Math.max(0, Math.min(gross_cents, dep.gross_cents - r.gross_cents));
  const after = r.gross_cents + gross;
  const upTo = (total) => (after >= dep.gross_cents ? total : Math.floor((total * after) / dep.gross_cents));
  const treasury = upTo(dep.treasury_cents) - r.treasury_cents;
  const platform = upTo(dep.platform_cents) - r.platform_cents;
  const tickets = upTo(dep.tickets) - r.tickets;
  const delta = upTo(dep.delta || 0) - (r.delta || 0);
  const fromTreasury = Math.min(treasury, Math.max(0, state.cents));
  const shortfall = treasury - fromTreasury;
  state.cents -= fromTreasury;
  state.platform_cents -= platform + shortfall;
  state.reversed_cents += gross;
  state.version += 1;
  dep.reversed = {
    gross_cents: after,
    treasury_cents: r.treasury_cents + treasury,
    platform_cents: r.platform_cents + platform,
    tickets: r.tickets + tickets,
    delta: (r.delta || 0) + delta,
  };
  const reversal = {
    ref, kind, deposit_ref: dep.ref, gross_cents: gross, treasury_cents: fromTreasury, shortfall_cents: shortfall,
    platform_cents: platform, tickets, delta, user_id: dep.user_id, deposit_kind: dep.kind, at: nowIso(),
  };
  const totals = addToMonth(await monthTotals(this.storage, reversal.at), 'reversals', dep.kind, reversal, REVERSAL_SUMS);
  await this.storage.put({ state, [`dep:${dep.ref}`]: dep, [`rev:${ref}`]: reversal, [aggKey(totals.month)]: totals });
  return { ok: true, reversal };
}

function jobSummary(job) {
  const s = { ...job };
  delete s.cursor;
  delete s.vcursor;
  delete s.vgroup;
  return s;
}

// =============================================================================
// Settlement phases
// =============================================================================
//
// Each phase gets a copy of the job and a KV budget. It does one bounded
// piece of work, records its progress in the copy, and sets `phase` when it
// is finished. The copy is saved only if the phase returns, so a failure
// leaves the job as it was, and every write a phase makes before the save is
// safe to make again.

const PHASES = {
  async init(job) {
    const env = this.env;
    const age = (Date.now() - dayMs(job.date)) / 86400e3;
    if (age > MAX_CREDIT_AGE_DAYS + 1) {
      job.phase = 'failed';
      job.failure = `Score date ${job.date} is ${Math.floor(age)} days old; crediting it now could land behind a ledger checkpoint. Settle it by hand.`;
      return;
    }
    if (age < 1) throw new Error(`Score date ${job.date} has not ended yet`);

    job.already_distributed = !!(await env.PAYOUTS.get(`distribution:${job.date}`));
    let genesis = await env.PAYOUTS.get('bliss:genesis_date');
    if (!genesis) {
      genesis = job.date;
      await env.PAYOUTS.put('bliss:genesis_date', genesis);
    }
    // The minor-unit counters, written from the legacy float keys the first
    // time a settlement runs without them.
    const supplyMinor = await readSupplyMinor(env);
    if (!(await env.PAYOUTS.get('bliss:supply_minor'))) await env.PAYOUTS.put('bliss:supply_minor', String(supplyMinor));
    const distributedMinor = await readDistributedMinor(env);
    if (distributedMinor && !(await env.PAYOUTS.get('bliss:distributed_minor'))) {
      await env.PAYOUTS.put('bliss:distributed_minor', String(distributedMinor));
    }
    const years = yearsSince(genesis, dayMs(job.date));
    const rate = blissEmissionRate(years);
    job.genesis = genesis;
    job.annual_rate = rate;
    job.supply_before_minor = supplyMinor;
    job.distributed_before_minor = distributedMinor;
    job.emission_ceiling_bls = (fromMinor(supplyMinor) * rate) / 365;
    job.total_score = 0;
    job.contributors = 0;
    job.top_score = 0;
    job.cursor = null;
    job.phase = 'collect_effort';
  },

  async collect_effort(job, budget) {
    const env = metered(this.env, budget);
    const prefix = `contrib:${job.date}:`;
    const page = await env.INVENTORY.list({ prefix, limit: COLLECT_PAGE, cursor: job.cursor || undefined });
    const writes = {};
    for (const k of page.keys) {
      const c = await readContrib(env, k);
      if (!c || c.effort + c.recorded_value <= 0) continue;
      writes[`u:${job.date}:${k.name.slice(prefix.length)}`] = { effort: c.effort, recorded_value: c.recorded_value, value: 0 };
    }
    await putMany(this.storage, writes);
    if (page.list_complete || !page.cursor) {
      job.cursor = null;
      job.phase = 'collect_value';
    } else {
      job.cursor = page.cursor;
    }
  },

  async collect_value(job, budget) {
    const env = metered(this.env, budget);
    const prefix = `vscore:${job.date}:`;
    const page = await env.INVENTORY.list({ prefix, limit: VALUE_PAGE, cursor: job.cursor || undefined });
    const writes = {};
    for (const k of page.keys) {
      const key = parseSaleKey(k.name, prefix);
      if (!key || key.creator.includes(':')) continue;
      const sale = await readSale(env, k);
      if (!sale || sale.buyer === key.creator || sale.buyer.includes(':')) continue;
      writes[`vs:${job.date}:${key.creator}:${sale.buyer}:${key.ref}`] = sale.tickets;
    }
    await putMany(this.storage, writes);
    if (page.list_complete || !page.cursor) {
      job.cursor = null;
      job.vcursor = null;
      job.vgroup = null;
      job.phase = 'value_finalize';
    } else {
      job.cursor = page.cursor;
    }
  },

  /// Stream the day's filed sales, sorted by creator then buyer, and set each
  /// creator's value score once: a buyer's Tickets capped, summed, scored,
  /// and the creator's score capped. The group being summed rides in the job
  /// between steps.
  async value_finalize(job) {
    const prefix = `vs:${job.date}:`;
    const opts = { prefix, limit: STORAGE_PAGE };
    if (job.vcursor) opts.startAfter = job.vcursor;
    const page = await this.storage.list(opts);
    let g = job.vgroup;
    const finished = [];
    const closeBuyer = (grp) => {
      grp.creator_tickets += Math.min(grp.buyer_tickets, VALUE_CAP_TICKETS_PER_BUYER);
      grp.buyer_tickets = 0;
    };
    for (const [key, tickets] of page) {
      const [, , creator, buyer] = key.split(':');
      if (!g || g.creator !== creator) {
        if (g) {
          closeBuyer(g);
          finished.push(g);
        }
        g = { creator, buyer, buyer_tickets: 0, creator_tickets: 0 };
      } else if (g.buyer !== buyer) {
        closeBuyer(g);
        g.buyer = buyer;
      }
      g.buyer_tickets += Number(tickets) || 0;
      job.vcursor = key;
    }
    const done = page.size < STORAGE_PAGE;
    if (done && g) {
      closeBuyer(g);
      finished.push(g);
      g = null;
    }
    const writes = {};
    for (const grp of finished) {
      const key = `u:${job.date}:${grp.creator}`;
      const u = (await this.storage.get(key)) || { effort: 0, recorded_value: 0 };
      u.value = Math.min(grp.creator_tickets * VALUE_SCORE_PER_TICKET, VALUE_CAP_SCORE_PER_CREATOR);
      u.value_tickets = grp.creator_tickets;
      writes[key] = u;
    }
    await putMany(this.storage, writes);
    job.vgroup = g;
    if (done) {
      job.vcursor = null;
      job.cursor = null;
      job.phase = 'total';
    }
  },

  async total(job) {
    const prefix = `u:${job.date}:`;
    const opts = { prefix, limit: STORAGE_PAGE };
    if (job.cursor) opts.startAfter = job.cursor;
    const page = await this.storage.list(opts);
    for (const [key, u] of page) {
      const score = contributorScore(u);
      job.cursor = key;
      if (score <= 0) continue;
      job.total_score += score;
      job.contributors += 1;
      if (score > job.top_score) job.top_score = score;
    }
    if (page.size < STORAGE_PAGE) {
      job.utilization = Math.min(1, job.total_score / FULL_DAY_SCORE);
      job.pool_minor = job.total_score > 0
        ? Math.floor(job.emission_ceiling_bls * BLISS_UNIT * job.utilization)
        : 0;
      job.top_share = job.total_score > 0 ? job.top_score / job.total_score : 0;
      // Observability, not a penalty: surfaces one account taking over half
      // of a day with real breadth, for a person to look at.
      job.concentration_flag = job.contributors >= 5 && job.top_share > 0.5;
      job.cursor = null;
      if (job.already_distributed) {
        // Settled before the treasury ran settlements: leave its emission and
        // payout as they were made, and back the ledger up.
        job.payout = { skipped: 'settled_before_treasury' };
        job.phase = 'backup';
      } else {
        job.phase = 'backfill';
      }
    }
  },

  /// Ledger entries written before entries carried metadata are read one by
  /// one wherever a balance is summed. Rewriting them with metadata once
  /// makes every later sum a listing. Skipped once a pass finds none left.
  async backfill(job, budget) {
    const env = metered(this.env, budget);
    if (!job.cursor && (await env.PAYOUTS.get('bliss:entries_have_metadata'))) {
      job.phase = 'credit';
      return;
    }
    const page = await env.PAYOUTS.list({ prefix: 'entry:', limit: BACKFILL_PAGE, cursor: job.cursor || undefined });
    for (const k of page.keys) {
      const e = await readEntry(env, k);
      if (e && !e.has_metadata) {
        await backfillEntryMetadata(env, k.name, e);
        job.backfilled = (job.backfilled || 0) + 1;
      }
    }
    if (page.list_complete || !page.cursor) {
      await env.PAYOUTS.put('bliss:entries_have_metadata', nowIso());
      job.cursor = null;
      job.phase = 'credit';
    } else {
      job.cursor = page.cursor;
    }
  },

  /// Credit each contributor's share of the pool. Each account's credit is
  /// idempotent (a deterministic entry key), and the account is marked done
  /// as soon as it is credited, so a step that stops part way loses nothing.
  async credit(job, budget) {
    const env = metered(this.env, budget);
    if ((Date.now() - dayMs(job.date)) / 86400e3 > MAX_CREDIT_AGE_DAYS + 1) {
      job.phase = 'failed';
      job.failure = `Crediting ${job.date} did not finish within ${MAX_CREDIT_AGE_DAYS} days; settle the rest by hand.`;
      return;
    }
    const prefix = `u:${job.date}:`;
    const opts = { prefix, limit: CREDIT_BATCH };
    if (job.cursor) opts.startAfter = job.cursor;
    const page = await this.storage.list(opts);
    try {
      for (const [key, u] of page) {
        if (!u.done) {
          const userId = key.slice(prefix.length);
          const next = { ...u };
          const score = contributorScore(u);
          if (score > 0) {
            const raw = await env.USERS.get(`user:${userId}`);
            if (!raw) {
              next.missing = true;
            } else {
              const user = JSON.parse(raw);
              next.username = user.username || null;
              next.connect = user.stripe_connect_id || null;
              // Cash-out needs identity verification (the Connect account)
              // and a Creator, Professional or Bundle plan.
              next.cashout_plan = next.connect ? await planAllowsCashout(env, userId) : false;
              if (user.banned) {
                next.banned = true;
              } else {
                await ledgerMigrateUser(env, userId, user);
                const share = job.total_score > 0 ? Math.floor(job.pool_minor * (score / job.total_score)) : 0;
                if (share > 0) {
                  await ledgerAppend(env, userId, {
                    amount_minor: share,
                    kind: 'emission',
                    ref: job.date,
                    ts: `${job.date}T00:00:00.000Z`,
                    id: 'dist',
                  });
                  await ledgerReconcile(env, userId);
                  next.share_minor = share;
                }
              }
            }
          }
          next.done = true;
          await this.storage.put(key, next);
        }
        job.cursor = key;
      }
    } catch (e) {
      // Out of budget part way: the accounts marked done stay done, the one
      // in progress is redone next step, and nothing is credited twice.
      if (!(e instanceof BudgetExhausted)) throw e;
      return;
    }
    if (page.size < CREDIT_BATCH) {
      job.cursor = null;
      job.phase = 'supply';
    }
  },

  /// Write the supply counters as absolute values (the supply before the day
  /// plus what the day minted), then the day's distribution record, so a
  /// repeat writes the same thing.
  async supply(job) {
    const env = this.env;
    const prefix = `u:${job.date}:`;
    let minted = 0;
    let credited = 0;
    let banned = 0;
    const recipients = [];
    let startAfter;
    while (true) {
      const opts = { prefix, limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, u] of page) {
        startAfter = key;
        if (u.banned) banned += 1;
        if (!(u.share_minor > 0)) continue;
        minted += u.share_minor;
        credited += 1;
        recipients.push({ user_id: key.slice(prefix.length), score: contributorScore(u), bls: fromMinor(u.share_minor), bls_minor: u.share_minor });
      }
      if (page.size < STORAGE_PAGE) break;
    }
    recipients.sort((a, b) => b.bls_minor - a.bls_minor || (a.user_id < b.user_id ? -1 : 1));
    await env.PAYOUTS.put('bliss:supply_minor', String(job.supply_before_minor + minted));
    await env.PAYOUTS.put('bliss:distributed_minor', String(job.distributed_before_minor + minted));
    const record = {
      date: job.date,
      emission_pool: job.emission_ceiling_bls,
      emission_ceiling: job.emission_ceiling_bls,
      annual_rate: job.annual_rate,
      supply_before: fromMinor(job.supply_before_minor),
      total_score: job.total_score,
      contributor_count: job.contributors,
      credited_count: credited,
      banned_count: banned,
      utilization: job.utilization,
      full_day_score: FULL_DAY_SCORE,
      value_caps: {
        score_per_ticket: VALUE_SCORE_PER_TICKET,
        tickets_per_buyer: VALUE_CAP_TICKETS_PER_BUYER,
        score_per_creator: VALUE_CAP_SCORE_PER_CREATOR,
      },
      truncated: false,
      concentration_flag: job.concentration_flag,
      top_share: job.top_share,
      minted: fromMinor(minted),
      minted_minor: minted,
      recipients,
    };
    await env.PAYOUTS.put(`distribution:${job.date}`, JSON.stringify(record), {
      expirationTtl: 86400 * 365 * 5,
      // The summary the public ledger summary lists without reading records.
      metadata: {
        date: job.date,
        emission_pool: record.emission_pool,
        minted: record.minted,
        total_score: record.total_score,
        utilization: record.utilization,
        contributor_count: record.contributor_count,
        concentration_flag: record.concentration_flag,
        truncated: false,
      },
    });
    await env.PAYOUTS.put('bliss:last_day', JSON.stringify({
      date: job.date, total_score: job.total_score, contributors: job.contributors,
      utilization: job.utilization, minted_minor: minted,
    }));
    job.minted_minor = minted;
    job.credited_count = credited;
    job.cursor = null;
    job.phase = 'payout_setup';
  },

  /// Fix the day's drip from the treasury as it stands now, effort-gated like
  /// the emission, and the weights it is shared by. The weights count every
  /// contributor in good standing, connected to Stripe or not, so a share
  /// that cannot be paid stays in the treasury instead of going to others.
  async payout_setup(job) {
    const env = this.env;
    const skip = (reason) => {
      job.payout = { skipped: reason };
      job.cursor = null;
      job.phase = 'release_holds';
    };
    if (!env.STRIPE_SECRET_KEY) return skip('stripe_not_configured');
    if (await env.PAYOUTS.get(`payout:${job.date}`)) return skip('already_paid');
    if (!(job.total_score > 0)) return skip('no_contributors');
    const state = await this.storage.get('state');
    if (!(state.cents > 0)) return skip('treasury_empty');
    const { drip_cents, scarce, hwm_cents } = dripFor(state.cents, state.hwm_cents);
    const pool = Math.floor(drip_cents * job.utilization);
    if (pool < STRIPE_MIN_TRANSFER_CENTS) return skip('below_stripe_minimum');

    const prefix = `u:${job.date}:`;
    const eligible = [];
    let startAfter;
    while (true) {
      const opts = { prefix, limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, u] of page) {
        startAfter = key;
        const score = contributorScore(u);
        if (u.done && !u.banned && !u.missing && score > 0) eligible.push({ id: key.slice(prefix.length), score });
      }
      if (page.size < STORAGE_PAGE) break;
    }
    eligible.sort((a, b) => b.score - a.score || (a.id < b.id ? -1 : 1));
    // While scarce, the top quarter by score is paid double weight.
    const topCount = scarce && eligible.length ? Math.max(1, Math.ceil(eligible.length * TREASURY_TOP_FRACTION)) : 0;
    let weightTotal = 0;
    eligible.forEach((c, i) => {
      weightTotal += c.score * (i < topCount ? TREASURY_TOP_BOOST : 1);
    });
    if (!(weightTotal > 0)) return skip('no_eligible_contributors');
    job.payout = {
      treasury_before_cents: state.cents,
      hwm_before_cents: hwm_cents,
      scarce,
      drip_cents,
      pool_cents: pool,
      utilization: job.utilization,
      eligible: eligible.length,
      top_count: topCount,
      cutoff: topCount ? { score: eligible[topCount - 1].score, id: eligible[topCount - 1].id } : null,
      weight_total: weightTotal,
      passes: 0,
    };
    job.cursor = null;
    job.phase = 'payout_plan';
  },

  async payout_plan(job) {
    const p = job.payout;
    const prefix = `u:${job.date}:`;
    const opts = { prefix, limit: STORAGE_PAGE };
    if (job.cursor) opts.startAfter = job.cursor;
    const page = await this.storage.list(opts);
    const writes = {};
    for (const [key, u] of page) {
      job.cursor = key;
      const score = contributorScore(u);
      if (!u.done || u.banned || u.missing || score <= 0 || u.pay_status) continue;
      const id = key.slice(prefix.length);
      const top = p.cutoff && (score > p.cutoff.score || (score === p.cutoff.score && id <= p.cutoff.id));
      const weight = score * (top ? TREASURY_TOP_BOOST : 1);
      const cents = Math.floor((p.pool_cents * weight) / p.weight_total);
      let status = 'pending';
      if (!u.connect) status = 'unconnected';
      else if (!u.cashout_plan) status = 'no_plan';
      else if (cents < STRIPE_MIN_TRANSFER_CENTS) status = 'below_minimum';
      writes[key] = { ...u, pay_cents: cents, pay_status: status };
    }
    await putMany(this.storage, writes);
    if (page.size < STORAGE_PAGE) {
      job.cursor = null;
      job.phase = 'payout';
    }
  },

  /// Pay a batch. The treasury is debited before each transfer (a
  /// reservation) and the debit is released only once Stripe has certainly
  /// not paid, so the pool never pays out money it does not hold and never
  /// counts money twice. A transfer is retried with the same idempotency
  /// key; when retries run out, Stripe's own list of transfers decides
  /// whether an earlier attempt went through.
  async payout(job) {
    const p = job.payout;
    const prefix = `u:${job.date}:`;
    const opts = { prefix, limit: PAYOUT_BATCH };
    if (job.cursor) opts.startAfter = job.cursor;
    const page = await this.storage.list(opts);
    for (const [key, u] of page) {
      job.cursor = key;
      if (u.pay_status !== 'pending' && u.pay_status !== 'reserved') continue;
      const userId = key.slice(prefix.length);
      const cents = u.pay_cents;
      if (u.pay_status === 'pending') {
        // A key replacement freezes the account's value for a few days
        // (src/fresh_auth.mjs). The share is still owed: it moves from the
        // treasury into a hold, paid to this day's destination (which the
        // replacement pins) once the freeze ends (release_holds). Checked
        // before anything is reserved. An unreadable freeze counts as frozen,
        // because this is money leaving.
        let frozen = 0;
        try {
          frozen = await valueFrozenUntil(this.env, userId);
        } catch (e) {
          console.error('treasury: value freeze unreadable, holding the payout:', e?.message || e);
          frozen = -1;
        }
        if (frozen) {
          await this.atomic(async () => {
            const state = await this.storage.get('state');
            if (state.cents < cents) {
              await this.storage.put(key, { ...u, pay_status: 'insufficient_treasury' });
              return;
            }
            state.cents -= cents;
            state.held_cents = (state.held_cents || 0) + cents;
            state.version += 1;
            const hold = {
              user_id: userId, date: job.date, cents, connect: u.connect, username: u.username || null,
              frozen_until: frozen > 0 ? frozen : null, held_at: nowIso(), status: 'held', attempts: 0, reason: 'key_replaced',
            };
            await this.storage.put({
              state,
              [key]: { ...u, pay_status: 'frozen', frozen_until: hold.frozen_until },
              [holdKey(userId, job.date)]: hold,
            });
          });
          continue;
        }
        const reserved = await this.atomic(async () => {
          const state = await this.storage.get('state');
          if (state.cents < cents) {
            await this.storage.put(key, { ...u, pay_status: 'insufficient_treasury' });
            return false;
          }
          state.cents -= cents;
          state.version += 1;
          await this.storage.put({ state, [key]: { ...u, pay_status: 'reserved', reserved_at: nowIso() } });
          return true;
        });
        if (!reserved) continue;
      }
      const transfer = await stripeTransfer(this.env, {
        amountCents: cents,
        destination: u.connect,
        idempotencyKey: `bliss-payout-${job.date}-${userId}`,
        description: `Bliss daily payout - ${u.username || userId}`,
        metadata: { user_id: userId, date: job.date },
      });
      const attempts = (u.pay_attempts || 0) + 1;
      if (transfer.ok) {
        await this.settleTransfer(key, cents, { ...u, pay_attempts: attempts }, transfer.id);
        continue;
      }
      if (transfer.retryable && attempts < PAYOUT_MAX_ATTEMPTS) {
        await this.storage.put(key, { ...u, pay_status: 'reserved', pay_attempts: attempts, pay_error: transfer.error });
        continue;
      }
      // Out of retries, or refused. An earlier attempt may still have reached
      // Stripe with its answer lost, so look first.
      const found = await findTransfer(this.env, { destination: u.connect, date: job.date, userId, since: job.created_at });
      if (found.id) {
        await this.settleTransfer(key, cents, { ...u, pay_attempts: attempts }, found.id);
        continue;
      }
      // Not paid, and the share is still the contributor's earnings, so it
      // becomes a hold instead of going back to the pool. A fault of the
      // destination waits for review; anything else (a short platform
      // balance, a bad key, Stripe unreachable) is paid on a later run by
      // release_holds. The reservation already took the share out of
      // state.cents, so it moves into held_cents without passing through the
      // pool. attempts stays at least 1, so the hold asks Stripe before it
      // sends: this attempt may have gone through with its answer lost.
      const refused = !!found.checked && isDestinationFault(transfer);
      const error = found.checked
        ? (transfer.code ? `${transfer.code}: ${transfer.error}` : transfer.error)
        : `${transfer.error}; lookup: ${found.error}`;
      await this.atomic(async () => {
        const state = await this.storage.get('state');
        state.held_cents = (state.held_cents || 0) + cents;
        if (refused) state.review_cents = (state.review_cents || 0) + cents;
        state.version += 1;
        const at = nowIso();
        const hold = {
          user_id: userId, date: job.date, cents, connect: u.connect, username: u.username || null,
          frozen_until: null, held_at: at, since: job.created_at, attempts: Math.max(1, attempts), error,
          not_before: refused ? null : Date.now() + HOLD_FIRST_TRY_DELAY_MS,
          reason: refused ? 'destination_refused' : 'stripe_unavailable',
          status: refused ? 'review' : 'held',
          ...(refused ? { review_reason: 'destination_refused', review_since: at } : {}),
        };
        await this.storage.put({
          state,
          [key]: { ...u, pay_status: refused ? 'review' : 'deferred', pay_attempts: attempts, pay_error: error },
          [holdKey(userId, job.date)]: hold,
        });
      });
    }
    if (page.size < PAYOUT_BATCH) {
      job.cursor = null;
      p.passes += 1;
      if (await this.hasReserved(prefix)) {
        // Another pass, later, for transfers worth retrying.
        job.delay_ms = 60e3 * p.passes;
      } else {
        job.phase = 'payout_finish';
      }
    }
  },

  async payout_finish(job) {
    const env = this.env;
    const p = job.payout;
    // Applied once per score date. The marker lives in `state`, written with
    // the new mark, so a failed step that is retried cannot decay it twice.
    const state = await this.atomic(async () => {
      const s = await this.storage.get('state');
      if (s.hwm_decayed_for !== job.date) {
        s.hwm_cents = decayedHwm(p.hwm_before_cents, p.treasury_before_cents, s.cents);
        s.hwm_decayed_for = job.date;
        s.version += 1;
        await this.storage.put('state', s);
      }
      return s;
    });
    const prefix = `u:${job.date}:`;
    const payouts = [];
    const failures = [];
    const totals = { paid: 0, unconnected: 0, no_plan: 0, frozen: 0, below_minimum: 0, failed: 0, deferred: 0, review: 0 };
    const counts = { sent: 0, deferred: 0, review: 0 };
    let startAfter;
    while (true) {
      const opts = { prefix, limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, u] of page) {
        startAfter = key;
        const userId = key.slice(prefix.length);
        const cents = u.pay_cents || 0;
        if (['paid', 'deferred', 'review', 'failed', 'unresolved'].includes(u.pay_status)) counts.sent += 1;
        if (u.pay_status === 'paid') {
          totals.paid += cents;
          payouts.push({ user_id: userId, username: u.username || null, amount_usd: cents / 100, amount_cents: cents, transfer_id: u.transfer_id });
        } else if (u.pay_status === 'unconnected') {
          totals.unconnected += cents;
        } else if (u.pay_status === 'no_plan') {
          totals.no_plan += cents;
        } else if (u.pay_status === 'frozen') {
          totals.frozen += cents;
        } else if (u.pay_status === 'below_minimum') {
          totals.below_minimum += cents;
        } else if (u.pay_status === 'deferred') {
          totals.deferred += cents;
          counts.deferred += 1;
        } else if (u.pay_status === 'review') {
          totals.review += cents;
          counts.review += 1;
        } else if (u.pay_status === 'failed' || u.pay_status === 'insufficient_treasury' || u.pay_status === 'unresolved') {
          totals.failed += cents;
          failures.push({ user_id: userId, amount_cents: cents, status: u.pay_status, error: u.pay_error || null });
        }
      }
      if (page.size < STORAGE_PAGE) break;
    }
    p.hwm_after_cents = state.hwm_cents;
    p.paid_cents = totals.paid;
    p.paid_count = payouts.length;
    p.unconnected_cents = totals.unconnected;
    p.no_plan_cents = totals.no_plan;
    p.frozen_cents = totals.frozen;
    p.below_minimum_cents = totals.below_minimum;
    p.failed_cents = totals.failed;
    p.failed_count = failures.length;
    p.stripe_unavailable_cents = totals.deferred;
    p.stripe_unavailable_count = counts.deferred;
    p.destination_review_cents = totals.review;
    p.destination_review_count = counts.review;
    // Loud on purpose: many shares Stripe would not take (a short platform
    // balance, a bad key) mean the books claim money Stripe does not hold, and
    // holds alone would only hide it as a growing pile.
    p.stripe_shortfall_alert = counts.sent > 0 && counts.deferred / counts.sent > STRIPE_SHORTFALL_ALERT_SHARE;
    const record = {
      date: nowIso(),
      score_date: job.date,
      drip_usd: p.drip_cents / 100,
      pool_usd: p.pool_cents / 100,
      utilization: p.utilization,
      scarcity_active: p.scarce,
      total_paid_usd: totals.paid / 100,
      contributors_paid: payouts.length,
      // Shares that stayed in the treasury: owed to contributors with no
      // Stripe account, without a plan that includes cash-out, or under
      // Stripe's minimum.
      unconnected_usd: totals.unconnected / 100,
      no_plan_usd: totals.no_plan / 100,
      // Held because the account's key was just replaced; paid once the
      // freeze ends, and then listed in `payouts` with `released_at`.
      frozen_usd: totals.frozen / 100,
      below_minimum_usd: totals.below_minimum / 100,
      // Held, not lost: Stripe would not take the transfer today (paid on a
      // later run), or refused the destination (waits for review).
      stripe_unavailable_usd: totals.deferred / 100,
      stripe_unavailable_count: counts.deferred,
      destination_review_usd: totals.review / 100,
      destination_review_count: counts.review,
      stripe_shortfall_alert: p.stripe_shortfall_alert,
      failed_usd: totals.failed / 100,
      failed: failures,
      treasury_before: p.treasury_before_cents / 100,
      treasury_after: state.cents / 100,
      payouts,
    };
    await env.PAYOUTS.put(`payout:${job.date}`, JSON.stringify(record), { expirationTtl: 86400 * 365 * 5 });
    job.cursor = null;
    job.phase = 'release_holds';
  },

  /// Pay the holds whose freeze has ended, a batch per step, every day. A
  /// hold is paid once: an attempt is recorded before Stripe is called, and
  /// every attempt after the first asks Stripe whether an earlier one went
  /// through. A hold for an account since banned or deleted, or whose
  /// destination Stripe refuses for good, waits for review instead: it may be
  /// a taken account's earnings. Paid holds are written into their days'
  /// payout records by record_holds.
  async release_holds(job) {
    const r = job.holds || (job.holds = {
      released: 0, released_cents: 0, waiting: 0, review: 0, review_cents: 0, returned_cents: 0, unresolved: 0,
    });
    const opts = { prefix: 'hold:', limit: PAYOUT_BATCH };
    if (job.cursor) opts.startAfter = job.cursor;
    const page = await this.storage.list(opts);
    for (const [key, h] of page) {
      job.cursor = key;
      const outcome = await this.releaseHold(key, h);
      if (outcome.paid) {
        r.released += 1;
        r.released_cents += h.cents;
      } else if (outcome.returned) {
        r.returned_cents += h.cents;
      } else if (outcome.review) {
        r.review += 1;
        r.review_cents += h.cents;
      } else if (outcome.unresolved) {
        r.unresolved += 1;
      } else if (outcome.waiting) {
        r.waiting += 1;
      }
    }
    if (page.size < PAYOUT_BATCH) {
      job.cursor = null;
      job.phase = 'record_holds';
    }
  },

  /// Write the paid holds into their days' payout records, so they show in
  /// the contributors' histories, then delete them. Each record is read and
  /// written once, so no KV key is written twice in a second; a failed write
  /// fails the step, and the retry finds the same holds still marked paid.
  async record_holds(job) {
    const byDate = new Map();
    let startAfter;
    while (true) {
      const opts = { prefix: 'hold:', limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, h] of page) {
        startAfter = key;
        if (h.status !== 'paid') continue;
        if (!byDate.has(h.date)) byDate.set(h.date, []);
        byDate.get(h.date).push([key, h]);
      }
      if (page.size < STORAGE_PAGE) break;
    }
    for (const [date, paid] of byDate) {
      await this.recordReleases(date, paid.map(([, h]) => h));
      await this.storage.delete(paid.map(([key]) => key));
    }
    job.cursor = null;
    job.phase = 'backup';
  },

  /// Copy the ledger to R2: one part per page of entries, then a manifest.
  /// A part is named by its number and a retried step lists the same page
  /// again, so a repeat rewrites the same part.
  async backup(job, budget) {
    const env = metered(this.env, budget);
    if (!this.env.SCENES) {
      job.backup = { skipped: 'r2_not_bound' };
      job.phase = 'cleanup';
      return;
    }
    const b = job.backup || (job.backup = { prefix: `ledger-backups/${job.date}/`, parts: 0, records: 0, legacy: false });
    let page;
    const records = [];
    try {
      page = await env.PAYOUTS.list({ prefix: 'entry:', limit: b.legacy ? BACKUP_PAGE_LEGACY : BACKUP_PAGE_CLEAN, cursor: job.cursor || undefined });
      for (const k of page.keys) {
        const e = await readEntry(env, k);
        if (!e) continue;
        if (!e.has_metadata) {
          b.legacy = true;
          await backfillEntryMetadata(env, k.name, e);
        }
        records.push([k.name, { amount_minor: e.amount_minor, kind: e.kind, ref: e.ref, ts: e.ts }]);
      }
    } catch (e) {
      if (!(e instanceof BudgetExhausted)) throw e;
      // A page of unmigrated entries: the metadata written so far stays, and
      // the page is read again, smaller, next step.
      b.legacy = true;
      return;
    }
    await this.env.SCENES.put(`${b.prefix}entries-${String(b.parts).padStart(5, '0')}.json`, JSON.stringify({ part: b.parts, records }), {
      httpMetadata: { contentType: 'application/json' },
    });
    b.parts += 1;
    b.records += records.length;
    if (page.list_complete || !page.cursor) {
      const state = await this.storage.get('state');
      const manifest = {
        version: 2,
        score_date: job.date,
        taken_at: nowIso(),
        unit_minor_per_bls: BLISS_UNIT,
        parts: b.parts,
        record_count: b.records,
        supply_minor: parseInt((await this.env.PAYOUTS.get('bliss:supply_minor')) || '0', 10),
        distributed_minor: parseInt((await this.env.PAYOUTS.get('bliss:distributed_minor')) || '0', 10),
        burned_minor: parseInt((await this.env.PAYOUTS.get('bliss:burned_minor')) || '0', 10),
        genesis_date: await this.env.PAYOUTS.get('bliss:genesis_date'),
        treasury: state,
      };
      await this.env.SCENES.put(`${b.prefix}manifest.json`, JSON.stringify(manifest), {
        httpMetadata: { contentType: 'application/json' },
      });
      job.cursor = null;
      job.phase = 'cleanup';
    } else {
      job.cursor = page.cursor;
    }
  },

  /// Drop the day's working records. The day's outcome lives on in its
  /// distribution, payout and settlement records in KV.
  async cleanup(job) {
    for (const prefix of [`u:${job.date}:`, `vs:${job.date}:`]) {
      const page = await this.storage.list({ prefix, limit: STORAGE_PAGE });
      if (page.size === 0) continue;
      const keys = [...page.keys()];
      for (let i = 0; i < keys.length; i += 128) await this.storage.delete(keys.slice(i, i + 128));
      return;
    }
    job.phase = 'done';
    job.completed_at = nowIso();
  },
};
