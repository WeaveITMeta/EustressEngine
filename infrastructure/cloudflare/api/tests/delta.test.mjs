import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DeltaAccount, handleDeltaRoute, deltaCall, deltaCredit, deltaDebit, activePlan, planAllowsCashout,
  writePlan, planGrantExpiry, creditRefForDeposit, DELTA_TRIAL, PACK_KEYS, LOT_LIFE_MS,
} from '../src/delta.mjs';

// ── Fakes ──────────────────────────────────────────────────────────────────

function kv() {
  const store = new Map();
  return {
    store,
    async get(key) { return store.has(key) ? store.get(key).value : null; },
    async put(key, value, opts = {}) { store.set(key, { value: String(value), metadata: opts.metadata }); },
    async delete(key) { store.delete(key); },
    async list({ prefix = '' } = {}) {
      return { keys: [...store.keys()].filter((k) => k.startsWith(prefix)).sort().map((name) => ({ name })), list_complete: true };
    },
  };
}

function doStorage() {
  const map = new Map();
  const clone = (v) => (v === undefined ? undefined : structuredClone(v));
  return {
    map,
    async get(key) { return clone(map.get(key)); },
    async put(key, value) {
      if (typeof key === 'object') for (const [k, v] of Object.entries(key)) map.set(k, clone(v));
      else map.set(key, clone(value));
    },
    async delete(key) { return map.delete(key); },
  };
}

function namespace(Klass, envRef) {
  const instances = new Map();
  return {
    idFromName: (name) => name,
    get(id) {
      if (!instances.has(id)) {
        let blocked = Promise.resolve();
        const ctx = {
          storage: doStorage(),
          blockConcurrencyWhile(fn) { const run = blocked.then(() => fn()); blocked = run.then(() => {}, () => {}); return run; },
        };
        instances.set(id, new Klass(ctx, envRef.env));
      }
      const instance = instances.get(id);
      return { fetch: (url, init) => instance.fetch(new Request(url, init)) };
    },
  };
}

/// Stripe as far as Delta uses it: prices by lookup key, and Checkout.
function stripeFake(prices) {
  const sessions = [];
  return {
    sessions,
    async stripeRequest(method, endpoint, body) {
      if (method === 'GET' && endpoint.startsWith('/prices?')) {
        const keys = new URLSearchParams(endpoint.slice('/prices?'.length)).getAll('lookup_keys[]');
        return { data: prices.filter((p) => keys.includes(p.lookup_key)) };
      }
      if (method === 'POST' && endpoint === '/checkout/sessions') {
        const s = { id: `cs_${sessions.length + 1}`, url: `https://checkout.stripe.com/c/pay/cs_${sessions.length + 1}`, body };
        sessions.push(s);
        return s;
      }
      return { error: { message: `unexpected ${method} ${endpoint}` } };
    },
  };
}

const PRICES = [
  ...PACK_KEYS.map((key, i) => ({
    id: `price_pack_${i}`, lookup_key: key, unit_amount: [1000, 2500, 5000, 10000, 20000, 50000][i],
    metadata: { type: 'delta_purchase', delta: String([1000, 2600, 5400, 11200, 23000, 60000][i]) },
  })),
  { id: 'price_cr_m', lookup_key: 'eustress_creator_monthly', unit_amount: 999, metadata: { eustress_plan: 'creator', delta_per_invoice: '1000' } },
];

function world({ verified = new Set(), prices = PRICES } = {}) {
  const ref = {};
  const env = { USERS: kv(), PAYOUTS: kv(), STRIPE_SECRET_KEY: 'sk_test_x' };
  env.DELTA_ACCOUNTS = namespace(DeltaAccount, ref);
  ref.env = env;
  const stripe = stripeFake(prices);
  const deps = {
    verifyAuth: async (req) => req.headers.get('Authorization')?.replace('Bearer ', '') || null,
    json: (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers }),
    stripeRequest: stripe.stripeRequest,
    siteOrigin: () => 'https://eustress.dev',
    isVerified: async (id) => verified.has(id),
  };
  const call = async (method, path, { user, body } = {}) => {
    const req = new Request(`https://api.eustress.dev${path}`, {
      method, headers: user ? { Authorization: `Bearer ${user}` } : {}, body: body ? JSON.stringify(body) : undefined,
    });
    const res = await handleDeltaRoute(req, new URL(req.url), env, {}, deps);
    return { status: res.status, body: await res.json() };
  };
  return { env, stripe, call };
}

// ── The account ────────────────────────────────────────────────────────────

test('a credit counts once per ref, and a debit is refused when short', async () => {
  const { env } = world();
  assert.equal((await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 500, kind: 'purchase' })).balance, 500);
  const again = await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 500, kind: 'purchase' });
  assert.equal(again.replay, true);
  assert.equal(again.balance, 500);
  const short = await deltaDebit(env, 'u1', { ref: 'use:r1', amount: 600, kind: 'tool' });
  assert.equal(short.status, 402);
  assert.equal(short.code, 'insufficient_delta');
  assert.deepEqual([short.needed, short.available], [600, 500]);
  assert.equal((await deltaDebit(env, 'u1', { ref: 'use:r2', amount: 120, kind: 'tool' })).balance, 380);
  assert.equal((await deltaDebit(env, 'u1', { ref: 'use:r2', amount: 120, kind: 'tool' })).balance, 380);
});

test('a chargeback may take Delta below zero', async () => {
  const { env } = world();
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 500, kind: 'purchase' });
  await deltaDebit(env, 'u1', { ref: 'use:r1', amount: 400, kind: 'tool' });
  const r = await deltaDebit(env, 'u1', { ref: 'reversal:dispute:dp_1', amount: 500, kind: 'reversal', allow_negative: true });
  assert.equal(r.balance, -400);
});

// ── Balance and the trial ─────────────────────────────────────────────────

test('the balance needs a sign-in, and a verified account gets the trial once', async () => {
  const { call } = world({ verified: new Set(['verified']) });
  assert.equal((await call('GET', '/api/delta/balance')).status, 401);
  assert.deepEqual((await call('GET', '/api/delta/balance', { user: 'unverified' })).body, { delta: 0, reserved: 0, plan: null, expiring: null });
  const first = await call('GET', '/api/delta/balance', { user: 'verified' });
  assert.equal(first.body.delta, DELTA_TRIAL);
  assert.equal(DELTA_TRIAL, 1000);
  assert.equal(first.body.expiring.delta, DELTA_TRIAL);
  const life = Date.parse(first.body.expiring.at) - Date.now();
  assert.ok(life > LOT_LIFE_MS - 60e3 && life <= LOT_LIFE_MS, `the trial lasts a year, not ${life} ms`);
  const second = await call('GET', '/api/delta/balance', { user: 'verified' });
  assert.equal(second.body.delta, DELTA_TRIAL);
});

// ── Packs, catalog and checkout ───────────────────────────────────────────

test('packs are read from Stripe by lookup key, in order', async () => {
  const { call } = world();
  const r = await call('GET', '/api/delta/packs');
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, [
    { id: 'eustress_delta_session', delta: 1000, usd: 10 },
    { id: 'eustress_delta_project', delta: 2600, usd: 25 },
    { id: 'eustress_delta_week', delta: 5400, usd: 50 },
    { id: 'eustress_delta_sprint', delta: 11200, usd: 100 },
    { id: 'eustress_delta_month', delta: 23000, usd: 200 },
    { id: 'eustress_delta_studio', delta: 60000, usd: 500 },
  ]);
});

test('pack checkout opens Stripe Checkout with the account and amount in metadata', async () => {
  const { call, stripe } = world();
  assert.equal((await call('POST', '/api/delta/checkout', { user: 'u1', body: { pack: 'eustress_delta_500' } })).status, 400);
  const r = await call('POST', '/api/delta/checkout', { user: 'u1', body: { pack: 'eustress_delta_project' } });
  assert.equal(r.status, 200);
  assert.match(r.body.url, /^https:\/\/checkout\.stripe\.com\//);
  const s = stripe.sessions.at(-1).body;
  assert.equal(s['metadata[type]'], 'delta_purchase');
  assert.equal(s['metadata[delta]'], '2600');
  assert.equal(s['metadata[user_id]'], 'u1');
  assert.equal(s['line_items[0][price]'], 'price_pack_1');
  assert.match(s.success_url, /\/delta\/thanks$/, 'a paid purchase lands on the thanks page');
  assert.match(s.cancel_url, /\/delta\?cancelled=1$/, 'a cancelled one returns to the page that sells the packs');
});

test('subscription checkout: closed plans say so, and an account with a plan cannot buy a second', async () => {
  const { env, call, stripe } = world();
  const closed = await call('POST', '/api/subscriptions/checkout', { user: 'u1', body: { plan: 'bundle', interval: 'yearly' } });
  assert.equal(closed.status, 404);
  assert.equal(closed.body.code, 'plans_not_open');
  const ok = await call('POST', '/api/subscriptions/checkout', { user: 'u1', body: { plan: 'creator', interval: 'monthly' } });
  assert.equal(ok.status, 200);
  const s = stripe.sessions.at(-1).body;
  assert.equal(s.mode, 'subscription');
  assert.equal(s['subscription_data[metadata][eustress_plan]'], 'creator');
  assert.equal(s['subscription_data[metadata][user_id]'], 'u1');
  await writePlan(env, 'u1', { plan: 'creator', status: 'active', current_period_end: Math.floor(Date.now() / 1000) + 86400 });
  const twice = await call('POST', '/api/subscriptions/checkout', { user: 'u1', body: { plan: 'bundle', interval: 'monthly' } });
  assert.equal(twice.status, 409);
});

test('only a tool the catalog prices can be charged, within its cap', async () => {
  const { env, call } = world();
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 100, kind: 'purchase' });
  assert.equal((await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:1', tool_id: 'data:import', amount: 5 } })).status, 404);
  await env.PAYOUTS.put('delta:catalog', JSON.stringify({ tools: { 'cwtr:pipe_sizer': { base: 5, per_unit: 2, unit: '1k_entities', max_per_run: 50, byok: false } } }));
  assert.equal((await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:1', tool_id: 'cwtr:pipe_sizer', amount: 51 } })).status, 400);
  const ok = await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:1', tool_id: 'cwtr:pipe_sizer', amount: 30 } });
  assert.deepEqual([ok.status, ok.body.balance, ok.body.charged], [200, 70, 30]);
  const replay = await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:1', tool_id: 'cwtr:pipe_sizer', amount: 30 } });
  assert.deepEqual([replay.body.balance, replay.body.replay], [70, true]);
  const short = await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:2', tool_id: 'cwtr:pipe_sizer', amount: 50 } });
  assert.equal(short.status, 200);
  const broke = await call('POST', '/api/delta/charge', { user: 'u1', body: { ref: 'run1:3', tool_id: 'cwtr:pipe_sizer', amount: 50 } });
  assert.deepEqual([broke.status, broke.body.code, broke.body.needed, broke.body.available], [402, 'insufficient_delta', 50, 20]);
});

// ── Plans and the cash-out rule ───────────────────────────────────────────

test('cash-out needs an active Creator, Professional or Bundle plan', async () => {
  const { env } = world();
  const soon = Math.floor(Date.now() / 1000) + 86400;
  assert.equal(await planAllowsCashout(env, 'u1'), false);
  await writePlan(env, 'u1', { plan: 'player_plus', status: 'active', current_period_end: soon });
  assert.equal(await planAllowsCashout(env, 'u1'), false);
  await writePlan(env, 'u1', { plan: 'creator', status: 'active', current_period_end: soon });
  assert.equal(await planAllowsCashout(env, 'u1'), true);
  await writePlan(env, 'u1', { plan: 'professional', status: 'active', current_period_end: soon });
  assert.equal(await planAllowsCashout(env, 'u1'), true);
  await writePlan(env, 'u1', { plan: 'creator_pro', status: 'active', current_period_end: soon });
  assert.equal(await planAllowsCashout(env, 'u1'), false, 'the old plan name is not a plan');
  await writePlan(env, 'u1', { plan: 'bundle', status: 'canceled', current_period_end: soon });
  assert.equal(await planAllowsCashout(env, 'u1'), false);
  assert.equal(activePlan({ plan: 'bundle', status: 'active', current_period_end: Math.floor(Date.now() / 1000) - 5 * 86400 }), null);
});

test('the account object answers unknown operations with 404', async () => {
  const { env } = world();
  const r = await deltaCall(env, 'u1', 'nope');
  assert.equal(r.status, 404);
});

// ── Expiry, debt and the monthly cap ──────────────────────────────────────

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const balanceOf = async (env, id) => (await deltaCall(env, id, 'balance'));

test('a lot expires at its time, and the expiry is written as an entry', async () => {
  const { env } = world();
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_short', amount: 300, kind: 'purchase', expires_at: Date.now() + 40 });
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_long', amount: 200, kind: 'purchase' });
  assert.equal((await balanceOf(env, 'u1')).balance, 500);
  await sleep(60);
  const b = await balanceOf(env, 'u1');
  assert.equal(b.balance, 200);
  assert.equal(b.expiring.delta, 200);
  const short = await deltaDebit(env, 'u1', { ref: 'use:r1', amount: 250, kind: 'tool' });
  assert.deepEqual([short.status, short.available], [402, 200]);
});

test('a charge draws the lot that expires first', async () => {
  const { env } = world();
  const soon = Date.now() + 30 * 86400e3;
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_pack', amount: 100, kind: 'purchase' });
  await deltaCredit(env, 'u1', { ref: 'sub:in_1', amount: 100, kind: 'plan', expires_at: soon });
  await deltaDebit(env, 'u1', { ref: 'use:r1', amount: 60, kind: 'tool' });
  const b = await balanceOf(env, 'u1');
  assert.equal(b.balance, 140);
  assert.deepEqual(b.expiring, { delta: 40, at: new Date(soon).toISOString() });
});

test('a refund takes the Delta from the lot it bought first', async () => {
  const { env } = world();
  const soon = Date.now() + 30 * 86400e3;
  await deltaCredit(env, 'u1', { ref: 'sub:in_1', amount: 100, kind: 'plan', expires_at: soon });
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_pack', amount: 100, kind: 'purchase' });
  await deltaDebit(env, 'u1', { ref: 'reversal:re_1', amount: 100, kind: 'reversal', allow_negative: true, prefer_lot: creditRefForDeposit('stripe:cs_pack') });
  const b = await balanceOf(env, 'u1');
  assert.deepEqual([b.balance, b.expiring.delta], [100, 100], 'the plan lot is untouched');
  assert.equal(creditRefForDeposit('stripe-invoice:in_9'), 'sub:in_9');
});

test('debt from a chargeback is paid by the next credit before anything else', async () => {
  const { env } = world();
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 100, kind: 'purchase' });
  await deltaDebit(env, 'u1', { ref: 'use:r1', amount: 100, kind: 'tool' });
  assert.equal((await deltaDebit(env, 'u1', { ref: 'reversal:dp_1', amount: 50, kind: 'reversal', allow_negative: true })).balance, -50);
  assert.equal((await deltaDebit(env, 'u1', { ref: 'use:r2', amount: 1, kind: 'tool' })).status, 402);
  assert.equal((await deltaCredit(env, 'u1', { ref: 'stripe:cs_2', amount: 80, kind: 'purchase' })).balance, 30);
  assert.equal((await balanceOf(env, 'u1')).expiring.delta, 30);
});

test('a plan grant lasts its period and one more, so at most two are unspent', async () => {
  const day = 86400;
  const now = Math.floor(Date.now() / 1000);
  assert.equal(planGrantExpiry(now, now + 30 * day), (now + 60 * day) * 1000);
  assert.equal(planGrantExpiry(now, now + 365 * day), (now + 396 * day) * 1000, 'a yearly grant rolls over one month');
  assert.equal(planGrantExpiry(now, null), null);
  const { env } = world();
  for (const [i, start] of [[1, now - 60 * day], [2, now - 30 * day], [3, now]].map(([i, s]) => [i, s])) {
    await deltaCredit(env, 'u1', { ref: `sub:in_${i}`, amount: 1000, kind: 'plan', expires_at: planGrantExpiry(start, start + 30 * day) });
  }
  assert.equal((await balanceOf(env, 'u1')).balance, 2000);
});

test('a tool family stops charging at its monthly cap', async () => {
  const { env, call } = world();
  await deltaCredit(env, 'u1', { ref: 'stripe:cs_1', amount: 1000, kind: 'purchase' });
  await env.PAYOUTS.put('delta:catalog', JSON.stringify({
    tools: {
      'cwtr:pipe_sizer': { base: 5, per_unit: 2, unit: '1k_entities', max_per_run: 80, byok: false, family: 'civil_class' },
      'cwtr:grader': { base: 5, per_unit: 2, unit: '1k_entities', max_per_run: 80, byok: false, family: 'civil_class' },
      'arch:render': { base: 5, per_unit: 2, unit: null, max_per_run: 80, byok: false },
    },
    families: { civil_class: { monthly_cap: 100, label: 'Civil design' } },
  }));
  const charge = (ref, tool_id) => call('POST', '/api/delta/charge', { user: 'u1', body: { ref, tool_id, amount: 80 } });
  assert.deepEqual((await charge('a', 'cwtr:pipe_sizer')).body.charged, 80);
  const second = await charge('b', 'cwtr:grader');
  assert.deepEqual([second.body.charged, second.body.balance], [20, 900]);
  const third = await charge('c', 'cwtr:pipe_sizer');
  assert.deepEqual([third.status, third.body.charged, third.body.balance], [200, 0, 900]);
  assert.deepEqual((await charge('b', 'cwtr:grader')).body.charged, 20, 'a replay reports what it charged');
  assert.deepEqual((await charge('d', 'arch:render')).body.charged, 80, 'a tool with no family is never capped');
  const catalog = await call('GET', '/api/delta/catalog');
  assert.equal(catalog.body.families.civil_class.monthly_cap, 100);
});

test('an account written before lots existed keeps its balance', async () => {
  const storage = new Map([['state', { account_id: 'old', balance: 250, version: 3, trial_granted: true, created: '2026-10-01T00:00:00Z' }]]);
  const ctx = {
    storage: {
      async get(k) { return structuredClone(storage.get(k)); },
      async put(k, v) { if (typeof k === 'object') for (const [a, b] of Object.entries(k)) storage.set(a, structuredClone(b)); else storage.set(k, structuredClone(v)); },
    },
    blockConcurrencyWhile: (fn) => fn(),
  };
  const account = new DeltaAccount(ctx, {});
  const ask = async (op, args = {}) => (await account.fetch(new Request(`https://delta.internal/${op}`, { method: 'POST', body: JSON.stringify({ account_id: 'old', ...args }) }))).json();
  const b = await ask('balance');
  assert.deepEqual([b.balance, b.expiring.delta], [250, 250]);
  assert.equal(storage.get('state').lots.length, 1, 'the opening lot is stored');
  assert.equal((await ask('debit', { ref: 'use:x', amount: 50, kind: 'tool' })).balance, 200);
});
