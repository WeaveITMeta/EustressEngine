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
//   payout          Stripe transfers to connected contributors
//   payout_finish   the high-water mark and the day's payout record
//   backup          the BLS ledger to R2, one part per page of entries
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

// -----------------------------------------------------------------------------
// Policy
// -----------------------------------------------------------------------------

/// Contributors' share of a Ticket sale's net: what arrives after the
/// storefront or processor takes its fee. On the web that is about 48% of
/// gross; through a 30% app store, 35%.
export const TREASURY_SPLIT = 0.50;

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
  return { ok: false, retryable, status: res.status, error: data?.error?.message || `HTTP ${res.status}` };
}

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

const TREASURY_OPS = {
  async snapshot() {
    const state = await this.storage.get('state');
    const active = await this.storage.get('active');
    const job = active ? await this.storage.get(`job:${active}`) : null;
    return { ok: true, state, view: treasuryView(state), settlement: job ? jobSummary(job) : null };
  },

  /// Money in: a Ticket sale's treasury share, or a donation to the treasury.
  async deposit(args) {
    const {
      ref, kind, gross_cents, fee_cents = 0, treasury_cents, platform_cents = 0,
      user_id = null, payment_intent = null, tickets = 0, channel = null,
    } = args;
    if (typeof ref !== 'string' || !REF.test(ref)) return fail(400, 'invalid ref', 'invalid_request');
    if (kind !== 'ticket_purchase' && kind !== 'treasury_fund') return fail(400, 'invalid kind', 'invalid_request');
    if (![gross_cents, fee_cents, treasury_cents, platform_cents, tickets].every(isCents)) {
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
        ref, kind, gross_cents, fee_cents, treasury_cents, platform_cents, tickets,
        user_id: typeof user_id === 'string' ? user_id : null,
        payment_intent: typeof payment_intent === 'string' ? payment_intent : null,
        channel: typeof channel === 'string' ? channel : null,
        at: nowIso(),
        refunded_cents: 0,
        reversed: { gross_cents: 0, treasury_cents: 0, platform_cents: 0, tickets: 0 },
      };
      const writes = { state, [`dep:${ref}`]: deposit };
      if (deposit.payment_intent) writes[`pi:${deposit.payment_intent}`] = ref;
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
      };
      const restoration = {
        ref, kind: 'dispute_won', of: rv.ref, deposit_ref: rv.deposit_ref, gross_cents: rv.gross_cents,
        treasury_cents: rv.treasury_cents, tickets: rv.tickets, user_id: rv.user_id, at: nowIso(),
      };
      await this.storage.put({ state, [`dep:${rv.deposit_ref}`]: dep, [`rev:${ref}`]: restoration });
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
  };
  const reversal = {
    ref, kind, deposit_ref: dep.ref, gross_cents: gross, treasury_cents: fromTreasury, shortfall_cents: shortfall,
    platform_cents: platform, tickets, user_id: dep.user_id, deposit_kind: dep.kind, at: nowIso(),
  };
  await this.storage.put({ state, [`dep:${dep.ref}`]: dep, [`rev:${ref}`]: reversal });
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
      job.phase = 'backup';
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
      // Stripe with its answer lost, so look before releasing the money.
      const found = await findTransfer(this.env, { destination: u.connect, date: job.date, userId, since: job.created_at });
      if (found.id) {
        await this.settleTransfer(key, cents, { ...u, pay_attempts: attempts }, found.id);
      } else if (found.checked) {
        await this.atomic(async () => {
          const state = await this.storage.get('state');
          state.cents += cents;
          state.version += 1;
          await this.storage.put({ state, [key]: { ...u, pay_status: 'failed', pay_attempts: attempts, pay_error: transfer.error } });
        });
      } else {
        // Stripe could not be asked. The money stays reserved, and the
        // payout record lists it for a person to reconcile.
        await this.storage.put(key, { ...u, pay_status: 'unresolved', pay_attempts: attempts, pay_error: `${transfer.error}; lookup: ${found.error}` });
      }
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
    const totals = { paid: 0, unconnected: 0, below_minimum: 0, failed: 0 };
    let startAfter;
    while (true) {
      const opts = { prefix, limit: STORAGE_PAGE };
      if (startAfter) opts.startAfter = startAfter;
      const page = await this.storage.list(opts);
      for (const [key, u] of page) {
        startAfter = key;
        const userId = key.slice(prefix.length);
        const cents = u.pay_cents || 0;
        if (u.pay_status === 'paid') {
          totals.paid += cents;
          payouts.push({ user_id: userId, username: u.username || null, amount_usd: cents / 100, amount_cents: cents, transfer_id: u.transfer_id });
        } else if (u.pay_status === 'unconnected') {
          totals.unconnected += cents;
        } else if (u.pay_status === 'below_minimum') {
          totals.below_minimum += cents;
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
    p.below_minimum_cents = totals.below_minimum;
    p.failed_cents = totals.failed;
    p.failed_count = failures.length;
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
      // Stripe account, under Stripe's minimum, or refused by Stripe.
      unconnected_usd: totals.unconnected / 100,
      below_minimum_usd: totals.below_minimum / 100,
      failed_usd: totals.failed / 100,
      failed: failures,
      treasury_before: p.treasury_before_cents / 100,
      treasury_after: state.cents / 100,
      payouts,
    };
    await env.PAYOUTS.put(`payout:${job.date}`, JSON.stringify(record), { expirationTtl: 86400 * 365 * 5 });
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
