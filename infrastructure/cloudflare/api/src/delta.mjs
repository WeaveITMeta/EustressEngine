// =============================================================================
// Delta: the usage currency for premium Studio tools
// =============================================================================
//
// Delta is a whole number of units, held per account in a DeltaAccount
// Durable Object (one per account, like the Ticket Wallet): KV has no
// compare-and-set, and a balance that tools draw down needs one. Delta never
// converts to Bliss or Tickets and is never cashed out.
//
// Where Delta comes from:
//   - a trial grant, once, when an account passes identity verification
//   - a plan's grant (Creator, Professional, Bundle), on each paid invoice
//   - a pack bought through Stripe Checkout (web, or Studio through the
//     system browser; Delta is never sold through an app store)
// Where it goes: premium tools, charged per committed step through
// POST /api/delta/charge. Studio holds Delta locally while a step runs and
// commits only finished steps, so a rolled-back step is never charged.
//
// Every credit is a lot with an expiry: a pack, the trial or a restored
// dispute lasts 12 months; a plan grant lasts its own period plus one more
// (at most a month), so at most two grants are ever unspent at once. Charges
// draw the lot that expires first. A chargeback of Delta already spent takes
// the balance below zero as debt, which the next credit pays first.
//
// A tool may belong to a family with a monthly cap (65% of the matching
// seat licence, per calendar month in UTC). Once an account's charges for the
// family reach the cap, the rest of the month's use of that family is free.
//
// Every change is keyed by its source (`ref`), so a repeat of it changes
// nothing: a Stripe redelivery, a retried charge, a second trial request.
//
// Prices are found by Stripe lookup key and grants read from each price's
// metadata, so amounts are tuned in Stripe without a code change (see
// docs/monetization/DELTA_STRIPE_SETUP.md).
//
// Routes:
//   GET  /api/delta/balance             {delta, reserved, plan, expiring}  bearer
//   GET  /api/delta/packs               [{id, delta, usd}]                 public
//   GET  /api/delta/catalog             {tools, families}                  public
//   POST /api/delta/checkout            {pack} -> {url}                    bearer
//   POST /api/delta/charge              {ref, tool_id, amount}
//                                         -> {balance, charged}            bearer
//   POST /api/subscriptions/checkout    {plan, interval} -> {url}          bearer
//
// Plan state lives in USERS `plan:{account}` = {plan, status, subscription_id,
// current_period_end, updated_at}, written only by the Stripe webhook.
// =============================================================================

/// Delta granted once to an account that passes identity verification.
export const DELTA_TRIAL = 1000;

/// The packs, by Stripe lookup key, in the order they are offered. Keys name
/// the tier; the price and the Delta it buys live in Stripe.
export const PACK_KEYS = [
  'eustress_delta_session',
  'eustress_delta_project',
  'eustress_delta_week',
  'eustress_delta_sprint',
  'eustress_delta_month',
  'eustress_delta_studio',
];

export const PLANS = ['player_plus', 'creator', 'professional', 'bundle'];
export const INTERVALS = ['monthly', 'yearly'];

/// Plans whose subscribers may cash Bliss out (with identity verification).
export const CASHOUT_PLANS = ['creator', 'professional', 'bundle'];

/// A single committed charge may not exceed this, whatever the catalog says.
const MAX_CHARGE = 1_000_000;

const DAY_MS = 86_400_000;
/// How long a pack, the trial or restored Delta lasts.
export const LOT_LIFE_MS = 365 * DAY_MS;
/// The longest a plan grant outlives its own period.
export const PLAN_ROLLOVER_MAX_MS = 31 * DAY_MS;

/// When a plan grant for the period [start, end] (Unix seconds) expires: the
/// end of the next period, or a month after its own end for longer periods.
export function planGrantExpiry(periodStart, periodEnd) {
  const end = Number(periodEnd) * 1000;
  const start = Number(periodStart) * 1000;
  if (!(end > 0)) return null;
  const length = start > 0 && end > start ? end - start : PLAN_ROLLOVER_MAX_MS;
  return end + Math.min(length, PLAN_ROLLOVER_MAX_MS);
}

const monthKey = (ms) => new Date(ms).toISOString().slice(0, 7);

const ACCOUNT_ID = /^[A-Za-z0-9_-]{1,128}$/;
const REF = /^[A-Za-z0-9_:.-]{1,200}$/;
const TOOL_ID = /^[a-z0-9_]+:[a-z0-9_]+$/;
const isWhole = (n) => Number.isSafeInteger(n);
const nowIso = () => new Date().toISOString();
const fail = (status, error, code, extra = {}) => ({ ok: false, status, error, code, ...extra });

export const planKey = (accountId) => `plan:${accountId}`;
export const subscriptionLookupKey = (plan, interval) => `eustress_${plan}_${interval}`;

// =============================================================================
// The Durable Object
// =============================================================================
//
// Storage:
//   state     { account_id, balance, version, trial_granted, created,
//               lots: [{ref, kind, remaining, expires_at}], debt,
//               month: {key: 'YYYY-MM', spent: {family: delta}} }
//             balance = the lots' remaining minus debt, kept in step.
//   ref:{ref} the result of a credit or debit, for repeats; an expiry is
//             written as ref:expire:{lot ref}

export class DeltaAccount {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
    this.accountId = null;
    this.tail = null;
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
      await this.open(args.account_id);
      const handler = DELTA_OPS[op];
      result = handler ? await handler.call(this, args) : fail(404, `unknown operation ${op}`, 'unknown_operation');
    } catch (e) {
      console.error(`DeltaAccount.${op} failed:`, e);
      result = fail(500, String(e?.message || e), 'internal_error');
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  async open(accountId) {
    if (this.accountId) {
      if (accountId && accountId !== this.accountId) throw new Error('account belongs to another id');
      return;
    }
    await this.ctx.blockConcurrencyWhile(async () => {
      if (this.accountId) return;
      let state = await this.storage.get('state');
      if (!state) {
        if (typeof accountId !== 'string' || !ACCOUNT_ID.test(accountId)) throw new Error('opened without an account id');
        state = {
          account_id: accountId, balance: 0, version: 0, trial_granted: false, created: nowIso(),
          lots: [], debt: 0, month: null,
        };
        await this.storage.put('state', state);
      } else if (accountId && state.account_id !== accountId) {
        throw new Error('account belongs to another id');
      }
      this.accountId = state.account_id;
    });
  }

  atomic(fn) {
    const run = (this.tail || Promise.resolve()).then(() => fn());
    this.tail = run.then(() => {}, () => {});
    return run;
  }
}

/// Bring a state written before lots existed up to date: a positive balance
/// becomes one lot that lasts a pack's life from now, a negative one debt.
function ensureLots(state, now) {
  if (Array.isArray(state.lots)) return false;
  state.lots = state.balance > 0
    ? [{ ref: 'opening', kind: 'opening', remaining: state.balance, expires_at: now + LOT_LIFE_MS }]
    : [];
  state.debt = state.balance < 0 ? -state.balance : 0;
  state.month = null;
  return true;
}

const settleBalance = (state) => {
  state.balance = state.lots.reduce((n, lot) => n + lot.remaining, 0) - state.debt;
};

/// Expire the lots whose time has passed. Returns the expiry entries to write.
function sweep(state, now) {
  const writes = {};
  for (const lot of state.lots) {
    if (lot.remaining > 0 && lot.expires_at != null && lot.expires_at <= now) {
      writes[`ref:expire:${lot.ref}`] = {
        ref: `expire:${lot.ref}`, amount: -lot.remaining, kind: 'expire', reason: `${lot.kind} Delta expired`,
        tool_id: null, at: new Date(now).toISOString(),
      };
      lot.remaining = 0;
    }
  }
  state.lots = state.lots.filter((lot) => lot.remaining > 0);
  return writes;
}

/// Run `fn(state, now)` on the swept state, inside the account's queue, and
/// store the state with any entry it returns. `fn` returns {entry?, result?}
/// or a failure. A ref seen before replays its first result.
function change(self, ref, fn) {
  return self.atomic(async () => {
    const prior = ref ? await self.storage.get(`ref:${ref}`) : null;
    const state = await self.storage.get('state');
    const now = Date.now();
    const migrated = ensureLots(state, now);
    const writes = sweep(state, now);
    const swept = migrated || Object.keys(writes).length > 0;
    if (prior) {
      settleBalance(state);
      if (swept) await self.storage.put({ state, ...writes });
      return { ok: true, replay: true, entry: prior, charged: -Math.min(0, prior.amount), balance: state.balance };
    }
    const out = fn(state, now);
    settleBalance(state);
    if (out.ok === false) {
      if (swept) await self.storage.put({ state, ...writes });
      return out;
    }
    if (out.entry) {
      state.version += 1;
      out.entry.balance_after = state.balance;
      writes[`ref:${ref}`] = out.entry;
    }
    if (out.entry || swept) await self.storage.put({ state, ...writes });
    return { ok: true, ...(out.entry ? { entry: out.entry } : {}), ...out.result, balance: state.balance };
  });
}

const validRef = (ref) => typeof ref === 'string' && REF.test(ref) && !ref.startsWith('expire:');
const validAmount = (amount) => isWhole(amount) && amount > 0 && amount <= MAX_CHARGE * 100;

const DELTA_OPS = {
  async balance() {
    return change(this, null, (state) => {
      const next = state.lots
        .filter((lot) => lot.expires_at != null)
        .sort((a, b) => a.expires_at - b.expires_at)[0];
      return {
        result: {
          trial_granted: !!state.trial_granted,
          expiring: next ? { delta: next.remaining, at: new Date(next.expires_at).toISOString() } : null,
        },
      };
    });
  },

  /// Credit `amount` once per ref, as a lot that expires at `expires_at` (ms
  /// since the epoch; a pack's life from now when absent). Debt is paid first.
  async credit({ ref, amount, kind = 'credit', reason, expires_at = null }) {
    if (!validRef(ref)) return fail(400, 'invalid ref', 'invalid_request');
    if (!validAmount(amount)) return fail(400, 'a credit is a positive whole number', 'invalid_request');
    if (expires_at != null && !isWhole(expires_at)) return fail(400, 'invalid expires_at', 'invalid_request');
    return change(this, ref, (state, now) => {
      if (kind === 'trial') state.trial_granted = true;
      const toDebt = Math.min(state.debt, amount);
      state.debt -= toDebt;
      const expiry = expires_at ?? now + LOT_LIFE_MS;
      if (amount > toDebt && expiry > now) state.lots.push({ ref, kind, remaining: amount - toDebt, expires_at: expiry });
      return {
        entry: {
          ref, amount, kind, reason: String(reason || '').slice(0, 120), tool_id: null,
          expires_at: new Date(expiry).toISOString(), at: new Date(now).toISOString(),
        },
      };
    });
  },

  /// Debit `amount` once per ref, from the lot `prefer_lot` first (the credit
  /// a refund reverses), then the lots that expire first. Refused when short
  /// unless `allow_negative` (a refund or chargeback of Delta already spent).
  /// With a `family` and `family_cap`, only what keeps this calendar month's
  /// charges for the family within the cap is taken; `charged` says how much.
  async debit({
    ref, amount, kind = 'debit', reason, tool_id = null, allow_negative = false, prefer_lot = null,
    family = null, family_cap = null,
  }) {
    if (!validRef(ref)) return fail(400, 'invalid ref', 'invalid_request');
    if (!validAmount(amount)) return fail(400, 'a debit is a positive whole number', 'invalid_request');
    return change(this, ref, (state, now) => {
      let charge = amount;
      const capped = typeof family === 'string' && family.length > 0 && isWhole(family_cap) && family_cap > 0;
      if (capped) {
        const key = monthKey(now);
        if (!state.month || state.month.key !== key) state.month = { key, spent: {} };
        charge = Math.min(amount, Math.max(0, family_cap - (state.month.spent[family] || 0)));
      }
      const available = Math.max(0, state.lots.reduce((n, lot) => n + lot.remaining, 0) - state.debt);
      if (charge > available && !allow_negative) {
        return fail(402, 'Not enough Delta', 'insufficient_delta', { needed: charge, available });
      }
      let left = charge;
      const order = [...state.lots].sort((a, b) =>
        (b.ref === prefer_lot) - (a.ref === prefer_lot) || (a.expires_at ?? Infinity) - (b.expires_at ?? Infinity));
      for (const lot of order) {
        if (left === 0) break;
        const take = Math.min(lot.remaining, left);
        lot.remaining -= take;
        left -= take;
      }
      state.debt += left;
      state.lots = state.lots.filter((lot) => lot.remaining > 0);
      if (capped) state.month.spent[family] = (state.month.spent[family] || 0) + charge;
      return {
        entry: {
          ref, amount: -charge, kind, reason: String(reason || '').slice(0, 120), tool_id,
          ...(capped ? { family, requested: amount } : {}), at: new Date(now).toISOString(),
        },
        result: { charged: charge },
      };
    });
  },

  /// The one-time trial grant. The caller has checked identity verification.
  async grant_trial({ amount = DELTA_TRIAL }) {
    const state = await this.storage.get('state');
    if (state.trial_granted) return { ...(await DELTA_OPS.balance.call(this)), replay: true };
    return DELTA_OPS.credit.call(this, { ref: 'trial', amount, kind: 'trial', reason: 'Trial for verified accounts' });
  },
};

// =============================================================================
// Worker-side helpers
// =============================================================================

export const deltaStub = (env, accountId) => env.DELTA_ACCOUNTS.get(env.DELTA_ACCOUNTS.idFromName(`delta:${accountId}`));

export async function deltaCall(env, accountId, op, args = {}) {
  if (!env.DELTA_ACCOUNTS) return fail(503, 'Delta is not configured on this deployment', 'delta_unavailable');
  const res = await deltaStub(env, accountId).fetch(`https://delta.internal/${op}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ account_id: accountId, ...args }),
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

export const deltaCredit = (env, accountId, { ref, amount, kind, reason, expires_at = null }) =>
  deltaCall(env, accountId, 'credit', { ref, amount, kind, reason, expires_at });

export const deltaDebit = (env, accountId, {
  ref, amount, kind, reason, tool_id, allow_negative, prefer_lot = null, family = null, family_cap = null,
}) => deltaCall(env, accountId, 'debit', {
  ref, amount, kind, reason, tool_id, allow_negative, prefer_lot, family, family_cap,
});

/// The Delta credit a Treasury deposit paid for: a pack is credited under the
/// deposit's own ref, a plan invoice under `sub:{invoice}`.
export const creditRefForDeposit = (depositRef) =>
  typeof depositRef === 'string' && depositRef.startsWith('stripe-invoice:')
    ? `sub:${depositRef.slice('stripe-invoice:'.length)}`
    : depositRef || null;

/// The account's plan record, or null.
export async function readPlan(env, accountId) {
  const raw = await env.USERS.get(planKey(accountId));
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/// The plan an account holds right now: its record's plan while the
/// subscription is active (or trialing, or past due inside Stripe's retry
/// window), else null.
export function activePlan(record, nowMs = Date.now()) {
  if (!record || !PLANS.includes(record.plan)) return null;
  if (!['active', 'trialing', 'past_due'].includes(record.status)) return null;
  if (record.current_period_end && record.current_period_end * 1000 + 3 * 86400e3 < nowMs) return null;
  return record.plan;
}

/// May this account cash Bliss out (on the plan side; identity is separate)?
export async function planAllowsCashout(env, accountId) {
  return CASHOUT_PLANS.includes(activePlan(await readPlan(env, accountId)));
}

export async function writePlan(env, accountId, record) {
  await env.USERS.put(planKey(accountId), JSON.stringify({ ...record, updated_at: nowIso() }));
}

/// The premium tool catalog, kept in PAYOUTS `delta:catalog`:
///   tools:    tool_id -> {base, per_unit, unit, max_per_run, byok, family?}
///   families: family -> {monthly_cap, label}
/// A tool with no family, or a family with no cap, is never capped.
export async function readCatalog(env) {
  const raw = await env.PAYOUTS.get('delta:catalog');
  const empty = { tools: {}, families: {} };
  if (!raw) return empty;
  try {
    const parsed = JSON.parse(raw);
    const obj = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? v : {});
    return { tools: obj(parsed?.tools), families: obj(parsed?.families) };
  } catch {
    return empty;
  }
}

/// Active Stripe prices for `keys`, by lookup key.
export async function pricesByLookupKey(env, deps, keys) {
  const q = new URLSearchParams({ active: 'true', limit: '100' });
  for (const k of keys) q.append('lookup_keys[]', k);
  const res = await deps.stripeRequest('GET', `/prices?${q}`, null, env);
  if (res?.error) throw new Error(res.error.message || 'Stripe price lookup failed');
  const out = new Map();
  for (const p of res?.data || []) if (p.lookup_key) out.set(p.lookup_key, p);
  return out;
}

const keyMode = (env) => (/^(sk|rk)_live_/.test(env.STRIPE_SECRET_KEY || '') ? 'live' : 'test');

/// The packs on offer, cached ten minutes.
export async function listPacks(env, deps) {
  const cacheKey = `delta:packs:${keyMode(env)}`;
  const cached = await env.PAYOUTS.get(cacheKey);
  if (cached) {
    try {
      return JSON.parse(cached);
    } catch {
      // Rebuild below.
    }
  }
  const prices = await pricesByLookupKey(env, deps, PACK_KEYS);
  const packs = [];
  for (const key of PACK_KEYS) {
    const p = prices.get(key);
    const delta = Number(p?.metadata?.delta);
    if (!p || p.metadata?.type !== 'delta_purchase' || !isWhole(delta) || delta <= 0) continue;
    packs.push({ id: key, delta, usd: p.unit_amount / 100 });
  }
  try {
    await env.PAYOUTS.put(cacheKey, JSON.stringify(packs), { expirationTtl: 600 });
  } catch {
    // A concurrent rebuild wrote it.
  }
  return packs;
}

// =============================================================================
// HTTP
// =============================================================================

/// Route `/api/delta/*` and `/api/subscriptions/checkout`. Null for anything
/// else. `deps`: verifyAuth, json, stripeRequest, siteOrigin, isVerified
/// (account id -> bool, the identity check that unlocks the trial).
export async function handleDeltaRoute(request, url, env, cors, deps) {
  const path = url.pathname;
  if (!path.startsWith('/api/delta/') && path !== '/api/subscriptions/checkout') return null;
  const method = request.method;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const reply = (body, status = 200) => deps.json(body, status, headers);
  const error = (status, message, code, extra = {}) => reply({ error: message, code, ...extra }, status);

  // Public reads.
  if (path === '/api/delta/packs' && method === 'GET') {
    if (!env.STRIPE_SECRET_KEY) return error(503, 'Payments are not configured', 'payments_unavailable');
    try {
      return deps.json(await listPacks(env, deps), 200, { ...cors, 'Cache-Control': 'public, max-age=300' });
    } catch (e) {
      return error(502, String(e?.message || e), 'stripe_error');
    }
  }
  if (path === '/api/delta/catalog' && method === 'GET') {
    return deps.json(await readCatalog(env), 200, { ...cors, 'Cache-Control': 'public, max-age=300' });
  }

  const account = await deps.verifyAuth(request, env);
  if (!account) return error(401, 'Sign in first', 'unauthenticated');

  if (path === '/api/delta/balance' && method === 'GET') {
    let b = await deltaCall(env, account, 'balance');
    if (!b.ok) return error(b.status || 500, b.error, b.code);
    if (!b.trial_granted && (await deps.isVerified(account, env))) {
      const t = await deltaCall(env, account, 'grant_trial', { amount: DELTA_TRIAL });
      if (t.ok) b = await deltaCall(env, account, 'balance');
      if (!b.ok) return error(b.status || 500, b.error, b.code);
    }
    return reply({
      delta: b.balance, reserved: 0, plan: activePlan(await readPlan(env, account)), expiring: b.expiring ?? null,
    });
  }

  if (path === '/api/delta/charge' && method === 'POST') {
    const body = await request.json().catch(() => null);
    if (!body) return error(400, 'Body must be JSON', 'invalid_json');
    const { ref, tool_id, amount } = body;
    if (typeof tool_id !== 'string' || !TOOL_ID.test(tool_id)) return error(400, 'tool_id is invalid', 'invalid_request');
    const catalog = await readCatalog(env);
    const price = catalog.tools[tool_id];
    // Only a tool the catalog prices can be charged: everything else is free.
    if (!price) return error(404, `${tool_id} is not a premium tool`, 'tool_not_priced');
    const cap = Math.min(MAX_CHARGE, isWhole(price.max_per_run) && price.max_per_run > 0 ? price.max_per_run : MAX_CHARGE);
    if (!isWhole(amount) || amount < 1 || amount > cap) return error(400, `amount must be a whole number from 1 to ${cap}`, 'invalid_request');
    if (typeof ref !== 'string' || !REF.test(ref)) return error(400, 'ref is invalid', 'invalid_request');
    const family = typeof price.family === 'string' && price.family ? price.family : null;
    const familyCap = family ? catalog.families[family]?.monthly_cap : null;
    const r = await deltaDebit(env, account, {
      ref: `use:${ref}`, amount, kind: 'tool', reason: tool_id, tool_id,
      family: isWhole(familyCap) && familyCap > 0 ? family : null,
      family_cap: isWhole(familyCap) && familyCap > 0 ? familyCap : null,
    });
    if (!r.ok) return error(r.status || 500, r.error, r.code, { needed: r.needed, available: r.available });
    return reply({ ok: true, balance: r.balance, charged: r.charged ?? amount, replay: !!r.replay });
  }

  if (!env.STRIPE_SECRET_KEY) return error(503, 'Payments are not configured', 'payments_unavailable');
  const site = deps.siteOrigin(env);

  if (path === '/api/delta/checkout' && method === 'POST') {
    const body = await request.json().catch(() => null);
    const pack = body?.pack;
    if (!PACK_KEYS.includes(pack)) return error(400, `pack must be one of ${PACK_KEYS.join(', ')}`, 'invalid_request');
    let price;
    try {
      price = (await pricesByLookupKey(env, deps, [pack])).get(pack);
    } catch (e) {
      return error(502, String(e?.message || e), 'stripe_error');
    }
    const delta = Number(price?.metadata?.delta);
    if (!price || !isWhole(delta) || delta <= 0) return error(404, 'That pack is not on sale', 'pack_not_open');
    const session = await deps.stripeRequest('POST', '/checkout/sessions', {
      mode: 'payment',
      'line_items[0][price]': price.id,
      'line_items[0][quantity]': '1',
      'metadata[type]': 'delta_purchase',
      'metadata[pack]': pack,
      'metadata[delta]': String(delta),
      'metadata[user_id]': account,
      client_reference_id: account,
      success_url: `${site}/delta/thanks`,
      // Back to the page that sells the packs, which says the purchase was cancelled.
      cancel_url: `${site}/delta?cancelled=1`,
    }, env);
    if (session?.error || !session?.url) return error(502, session?.error?.message || 'Checkout failed', 'stripe_error');
    return reply({ url: session.url });
  }

  if (path === '/api/subscriptions/checkout' && method === 'POST') {
    const body = await request.json().catch(() => null);
    const plan = body?.plan;
    const interval = body?.interval;
    if (!PLANS.includes(plan)) return error(400, `plan must be one of ${PLANS.join(', ')}`, 'invalid_request');
    if (!INTERVALS.includes(interval)) return error(400, 'interval must be monthly or yearly', 'invalid_request');
    if (activePlan(await readPlan(env, account))) {
      return error(409, 'This account already has a plan; manage it from your subscription settings', 'already_subscribed');
    }
    const key = subscriptionLookupKey(plan, interval);
    let price;
    try {
      price = (await pricesByLookupKey(env, deps, [key])).get(key);
    } catch (e) {
      return error(502, String(e?.message || e), 'stripe_error');
    }
    if (!price) return error(404, 'Subscriptions are not open yet', 'plans_not_open');
    const session = await deps.stripeRequest('POST', '/checkout/sessions', {
      mode: 'subscription',
      'line_items[0][price]': price.id,
      'line_items[0][quantity]': '1',
      'metadata[type]': 'subscription',
      'metadata[eustress_plan]': plan,
      'metadata[user_id]': account,
      'subscription_data[metadata][type]': 'subscription',
      'subscription_data[metadata][eustress_plan]': plan,
      'subscription_data[metadata][user_id]': account,
      client_reference_id: account,
      success_url: `${site}/premium?subscribed=${plan}`,
      cancel_url: `${site}/premium`,
    }, env);
    if (session?.error || !session?.url) return error(502, session?.error?.message || 'Checkout failed', 'stripe_error');
    return reply({ url: session.url });
  }

  return error(404, 'Not found', 'not_found');
}
