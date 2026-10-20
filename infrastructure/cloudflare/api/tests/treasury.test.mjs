import test from 'node:test';
import assert from 'node:assert/strict';
import {
  Treasury, treasuryCall, splitSale, dripFor, decayedHwm, seedState,
  TREASURY_DRIP_RATE, STRIPE_MIN_TRANSFER_CENTS, MAX_CREDIT_AGE_DAYS,
} from '../src/treasury.mjs';
import {
  BLISS_UNIT, FULL_DAY_SCORE, VALUE_SCORE_PER_TICKET, VALUE_CAP_TICKETS_PER_BUYER,
  ledgerBalanceMinor, creditValueScore,
} from '../src/bliss.mjs';

// ── Fakes ──────────────────────────────────────────────────────────────────

// KV as the Worker sees it, counting operations so a test can hold a step to
// the platform's 1,000 per invocation.
function kv(counter) {
  const store = new Map();
  const count = () => {
    counter.ops += 1;
  };
  return {
    store,
    async get(key) {
      count();
      return store.has(key) ? store.get(key).value : null;
    },
    async put(key, value, opts = {}) {
      count();
      if (opts.metadata !== undefined && Buffer.byteLength(JSON.stringify(opts.metadata)) > 1024)
        throw new Error('KV metadata exceeds 1024 bytes');
      store.set(key, { value: String(value), metadata: opts.metadata });
    },
    async delete(key) {
      count();
      store.delete(key);
    },
    async list({ prefix = '', limit = 1000, cursor } = {}) {
      count();
      const names = [...store.keys()].filter((k) => k.startsWith(prefix)).sort();
      const start = cursor ? Number(cursor) : 0;
      const slice = names.slice(start, start + limit);
      const end = start + slice.length;
      const complete = end >= names.length;
      return {
        keys: slice.map((name) => ({ name, metadata: store.get(name).metadata })),
        list_complete: complete,
        cursor: complete ? undefined : String(end),
      };
    },
  };
}

// Durable Object storage: values are structured clones, as in production.
function doStorage() {
  const map = new Map();
  let alarm = null;
  const clone = (v) => (v === undefined ? undefined : structuredClone(v));
  return {
    map,
    async get(key) {
      if (Array.isArray(key)) {
        const out = new Map();
        for (const k of key) if (map.has(k)) out.set(k, clone(map.get(k)));
        return out;
      }
      return clone(map.get(key));
    },
    async put(key, value) {
      if (typeof key === 'object') {
        const entries = Object.entries(key);
        if (entries.length > 128) throw new Error('put: more than 128 keys');
        for (const [k, v] of entries) map.set(k, clone(v));
      } else {
        map.set(key, clone(value));
      }
    },
    async delete(key) {
      if (Array.isArray(key)) {
        if (key.length > 128) throw new Error('delete: more than 128 keys');
        let n = 0;
        for (const k of key) if (map.delete(k)) n += 1;
        return n;
      }
      return map.delete(key);
    },
    async list({ prefix = '', start, startAfter, end, reverse = false, limit } = {}) {
      let keys = [...map.keys()].filter((k) => k.startsWith(prefix)).sort();
      if (start !== undefined) keys = keys.filter((k) => k >= start);
      if (startAfter !== undefined) keys = keys.filter((k) => k > startAfter);
      if (end !== undefined) keys = keys.filter((k) => k < end);
      if (reverse) keys.reverse();
      if (limit !== undefined) keys = keys.slice(0, limit);
      return new Map(keys.map((k) => [k, clone(map.get(k))]));
    },
    async getAlarm() { return alarm; },
    async setAlarm(at) { alarm = typeof at === 'number' ? at : at.getTime(); },
    async deleteAlarm() { alarm = null; },
  };
}

function namespace(Klass, envRef) {
  const instances = new Map();
  return {
    instances,
    idFromName: (name) => name,
    get(id) {
      if (!instances.has(id)) {
        let blocked = Promise.resolve();
        const ctx = {
          storage: doStorage(),
          blockConcurrencyWhile(fn) {
            const run = blocked.then(() => fn());
            blocked = run.then(() => {}, () => {});
            return run;
          },
          waitUntil: () => {},
        };
        instances.set(id, new Klass(ctx, envRef.env));
      }
      const instance = instances.get(id);
      return { fetch: (url, init) => instance.fetch(new Request(url, init)) };
    },
  };
}

function r2() {
  const objects = new Map();
  return { objects, put: async (key, value) => { objects.set(key, String(value)); } };
}

// Stripe, as far as payouts use it: transfers and the transfer list.
function stripe({ refuse = new Set(), flaky = new Map(), lostAnswers = new Set() } = {}) {
  const transfers = [];
  const byKey = new Map();
  const calls = { create: 0, list: 0 };
  const fetch = async (url, init = {}) => {
    const u = new URL(url);
    if (u.pathname === '/v1/transfers' && init.method === 'POST') {
      calls.create += 1;
      const body = new URLSearchParams(init.body);
      const key = init.headers['Idempotency-Key'];
      const dest = body.get('destination');
      if (refuse.has(dest)) {
        return new Response(JSON.stringify({ error: { message: 'Account cannot receive transfers' } }), { status: 400 });
      }
      const failures = flaky.get(dest) || 0;
      if (failures > 0) {
        flaky.set(dest, failures - 1);
        return new Response(JSON.stringify({ error: { message: 'Stripe is unavailable' } }), { status: 503 });
      }
      let t = byKey.get(key);
      if (!t) {
        t = {
          id: `tr_${transfers.length + 1}`, amount: Number(body.get('amount')), destination: dest,
          metadata: { user_id: body.get('metadata[user_id]'), date: body.get('metadata[date]') }, reversed: false,
        };
        byKey.set(key, t);
        transfers.push(t);
      }
      if (lostAnswers.has(dest)) throw new Error('connection reset');
      return new Response(JSON.stringify(t), { status: 200 });
    }
    if (u.pathname === '/v1/transfers' && (init.method || 'GET') === 'GET') {
      calls.list += 1;
      const dest = u.searchParams.get('destination');
      return new Response(JSON.stringify({ data: transfers.filter((t) => t.destination === dest), has_more: false }), { status: 200 });
    }
    throw new Error(`unexpected fetch ${url}`);
  };
  return { transfers, calls, fetch };
}

function world({ treasuryUsd = 0, withStripe = true } = {}) {
  const counter = { ops: 0 };
  const ref = {};
  const env = {
    PAYOUTS: kv(counter), USERS: kv(counter), INVENTORY: kv(counter), SOCIAL: kv(counter),
    SCENES: r2(),
    STRIPE_SECRET_KEY: withStripe ? 'sk_test_x' : undefined,
  };
  env.TREASURY = namespace(Treasury, ref);
  ref.env = env;
  if (treasuryUsd) {
    env.PAYOUTS.store.set('treasury:total_usd', { value: String(treasuryUsd) });
    env.PAYOUTS.store.set('treasury:hwm', { value: String(treasuryUsd) });
  }
  return { env, counter };
}

const day = (offset) => new Date(Date.now() - offset * 86400e3).toISOString().slice(0, 10);

function user(env, id, extra = {}) {
  env.USERS.store.set(`user:${id}`, { value: JSON.stringify({ id, username: id, ledger_migrated: true, ...extra }) });
}

function contrib(env, date, id, score, { metadata = true } = {}) {
  env.INVENTORY.store.set(`contrib:${date}:${id}`, {
    value: JSON.stringify({ total_score: score, by_type: {}, by_seconds: {}, count: 1 }),
    metadata: metadata ? { s: score, v: 0 } : undefined,
  });
}

/// Drive the settlement the way its alarm would, one step at a time,
/// recording the most KV operations any single step made.
async function settle(env, counter, date, { maxSteps = 5000 } = {}) {
  const start = await treasuryCall(env, 'start_settlement', { date });
  assert.equal(start.ok, true, start.error);
  let peak = 0;
  for (let i = 0; i < maxSteps; i++) {
    const before = counter.ops;
    const r = await treasuryCall(env, 'run_steps', { max: 1 });
    peak = Math.max(peak, counter.ops - before);
    const s = await treasuryCall(env, 'settlement', { date });
    if (s.settlement.phase === 'done' || s.settlement.phase === 'failed') return { job: s.settlement, peak };
    if (r.last && r.last.ok === false) throw new Error(`step failed: ${r.last.error}`);
  }
  throw new Error('settlement did not finish');
}

async function withFetch(fake, fn) {
  const original = globalThis.fetch;
  globalThis.fetch = fake;
  try {
    return await fn();
  } finally {
    globalThis.fetch = original;
  }
}

// ── Money math ─────────────────────────────────────────────────────────────

test('a sale splits into fee, treasury and platform cents that add up to the gross', () => {
  for (const [gross, channel] of [[499, 'web'], [999, 'web'], [9999, 'web'], [999, 'ios'], [999, 'android'], [999, 'unknown']]) {
    const s = splitSale(gross, channel);
    assert.equal(s.fee_cents + s.treasury_cents + s.platform_cents, gross);
    assert.ok(s.treasury_cents <= s.platform_cents);
  }
  assert.deepEqual(splitSale(999, 'web'), { gross_cents: 999, fee_cents: 59, net_cents: 940, treasury_cents: 470, platform_cents: 470 });
  assert.equal(splitSale(999, 'ios').fee_cents, 300);
  assert.equal(splitSale(999, 'unknown').fee_cents, 59);
});

test('the drip slows in scarcity and the high-water mark decays toward the balance', () => {
  assert.deepEqual(dripFor(100_000, 100_000), { drip_cents: Math.floor(100_000 * TREASURY_DRIP_RATE), scarce: false, hwm_cents: 100_000 });
  assert.equal(dripFor(10_000, 100_000).scarce, true);
  assert.equal(decayedHwm(100_000, 50_000, 49_000), Math.floor(100_000 * (1 - 0.00171)));
  assert.equal(decayedHwm(50_000, 60_000, 60_000), 60_000);
});

test('the treasury opens with the balances the KV keys held', async () => {
  const { env } = world();
  env.PAYOUTS.store.set('treasury:total_usd', { value: '1234.567' });
  env.PAYOUTS.store.set('platform:total_usd', { value: '99.5' });
  env.PAYOUTS.store.set('treasury:deposit_count', { value: '7' });
  const s = await seedState(env);
  assert.equal(s.cents, 123457);
  assert.equal(s.hwm_cents, 123457);
  assert.equal(s.platform_cents, 9950);
  assert.equal(s.deposit_count, 7);
  const snap = await treasuryCall(env, 'snapshot');
  assert.equal(snap.state.cents, 123457);
});

// ── Deposits, refunds, disputes ────────────────────────────────────────────

test('a deposit counts once, however often Stripe delivers it', async () => {
  const { env } = world();
  const split = splitSale(999, 'web');
  const args = { ref: 'stripe:cs_1', kind: 'ticket_purchase', ...split, user_id: 'u-1', payment_intent: 'pi_1', tickets: 880, channel: 'web' };
  delete args.gross_cents;
  delete args.net_cents;
  const first = await treasuryCall(env, 'deposit', { ...args, gross_cents: 999 });
  const again = await treasuryCall(env, 'deposit', { ...args, gross_cents: 999 });
  assert.equal(first.ok, true);
  assert.equal(again.replay, true);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 470);
  assert.equal(state.platform_cents, 470);
  assert.equal(state.fees_cents, 59);
  assert.equal(state.deposit_count, 1);
  assert.equal(state.hwm_cents, 470);
});

test('a deposit whose shares do not add up is refused', async () => {
  const { env } = world();
  const r = await treasuryCall(env, 'deposit', { ref: 'stripe:cs_bad', kind: 'ticket_purchase', gross_cents: 1000, fee_cents: 0, treasury_cents: 600, platform_cents: 500 });
  assert.equal(r.ok, false);
  assert.equal(r.status, 400);
});

test('refunds reverse the treasury share and the Tickets in step, and add up to the whole deposit', async () => {
  const { env } = world();
  await treasuryCall(env, 'deposit', { ref: 'stripe:cs_r', kind: 'ticket_purchase', gross_cents: 999, fee_cents: 59, treasury_cents: 470, platform_cents: 470, user_id: 'u-1', payment_intent: 'pi_r', tickets: 880 });
  const half = await treasuryCall(env, 'refund', { payment_intent: 'pi_r', refunded_total_cents: 500 });
  assert.equal(half.reversal.tickets, Math.floor(880 * 500 / 999));
  const repeat = await treasuryCall(env, 'refund', { payment_intent: 'pi_r', refunded_total_cents: 500 });
  assert.equal(repeat.replay, true);
  const rest = await treasuryCall(env, 'refund', { payment_intent: 'pi_r', refunded_total_cents: 999 });
  assert.equal(half.reversal.tickets + rest.reversal.tickets, 880);
  assert.equal(half.reversal.treasury_cents + rest.reversal.treasury_cents, 470);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 0);
  assert.equal(state.platform_cents, 0);
  assert.equal(state.reversed_cents, 999);
});

test('a refund of money already paid out is borne by the platform, never a negative treasury', async () => {
  const { env } = world();
  await treasuryCall(env, 'deposit', { ref: 'stripe:cs_p', kind: 'ticket_purchase', gross_cents: 999, fee_cents: 59, treasury_cents: 470, platform_cents: 470, user_id: 'u-1', payment_intent: 'pi_p', tickets: 880 });
  // Pay most of the pool out, as a settlement would.
  const t = env.TREASURY.instances.get('treasury');
  const state = await t.storage.get('state');
  state.cents = 100;
  await t.storage.put('state', state);
  const r = await treasuryCall(env, 'refund', { payment_intent: 'pi_p', refunded_total_cents: 999 });
  assert.equal(r.reversal.treasury_cents, 100);
  assert.equal(r.reversal.shortfall_cents, 370);
  const after = await treasuryCall(env, 'snapshot');
  assert.equal(after.state.cents, 0);
  assert.equal(after.state.platform_cents, 470 - 470 - 370);
});

test('a dispute reverses the deposit and a dispute won restores it', async () => {
  const { env } = world();
  await treasuryCall(env, 'deposit', { ref: 'stripe:cs_d', kind: 'ticket_purchase', gross_cents: 999, fee_cents: 59, treasury_cents: 470, platform_cents: 470, user_id: 'u-1', payment_intent: 'pi_d', tickets: 880 });
  const opened = await treasuryCall(env, 'dispute', { payment_intent: 'pi_d', dispute_id: 'dp_1', amount_cents: 999 });
  assert.equal(opened.reversal.tickets, 880);
  assert.equal((await treasuryCall(env, 'dispute', { payment_intent: 'pi_d', dispute_id: 'dp_1', amount_cents: 999 })).replay, true);
  const won = await treasuryCall(env, 'dispute_won', { dispute_id: 'dp_1' });
  assert.equal(won.reversal.tickets, 880);
  assert.equal(won.reversal.kind, 'dispute_won');
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 470);
  assert.equal(state.platform_cents, 470);
  assert.equal(state.reversed_cents, 0);
});

test('a refund for a payment the treasury never recorded is reported, not guessed at', async () => {
  const { env } = world();
  const r = await treasuryCall(env, 'refund', { payment_intent: 'pi_unknown', refunded_total_cents: 100 });
  assert.equal(r.ok, false);
  assert.equal(r.code, 'deposit_not_found');
});

// ── Settlement ─────────────────────────────────────────────────────────────

test('a settlement credits the emission by share, pays the effort-gated drip, and leaves unpaid shares in the treasury', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(env, 'u-connected', { stripe_connect_id: 'acct_1' });
  user(env, 'u-unconnected');
  user(env, 'u-banned', { banned: true, stripe_connect_id: 'acct_3' });
  contrib(env, D, 'u-connected', 300);
  contrib(env, D, 'u-unconnected', 300, { metadata: false });
  contrib(env, D, 'u-banned', 600);
  contrib(env, D, 'u-missing', 60);
  const s = stripe();
  const { job, peak } = await withFetch(s.fetch, () => settle(env, counter, D));

  assert.equal(job.phase, 'done', JSON.stringify(job.errors));
  assert.ok(peak <= 1000, `a step made ${peak} KV operations`);
  // Effort gate: the day's total, 1,260, is under a full day.
  assert.equal(job.total_score, 1260);
  assert.equal(job.utilization, 1260 / FULL_DAY_SCORE);
  const supplyMinor = 100_000_000 * BLISS_UNIT;
  const pool = Math.floor((supplyMinor / BLISS_UNIT) * 0.05 / 365 * BLISS_UNIT * (1260 / FULL_DAY_SCORE));
  const share = Math.floor(pool * (300 / 1260));
  assert.equal(await ledgerBalanceMinor(env, 'u-connected'), share);
  assert.equal(await ledgerBalanceMinor(env, 'u-unconnected'), share);
  assert.equal(await ledgerBalanceMinor(env, 'u-banned'), 0);
  // Supply grows by exactly what was credited.
  assert.equal(Number(env.PAYOUTS.store.get('bliss:supply_minor').value), supplyMinor + 2 * share);
  const dist = JSON.parse(env.PAYOUTS.store.get(`distribution:${D}`).value);
  assert.equal(dist.minted_minor, 2 * share);
  assert.equal(dist.recipients.length, 2);
  assert.equal(env.PAYOUTS.store.get(`distribution:${D}`).metadata.date, D);

  // The drip: 0.276% of $100,000, gated by the day's effort, shared by
  // everyone in good standing; only the connected account is paid.
  const drip = Math.floor(10_000_000 * TREASURY_DRIP_RATE);
  const payPool = Math.floor(drip * (1260 / FULL_DAY_SCORE));
  const eachCents = Math.floor(payPool * (300 / 600));
  assert.equal(s.transfers.length, 1);
  assert.equal(s.transfers[0].amount, eachCents);
  assert.equal(s.transfers[0].destination, 'acct_1');
  const payout = JSON.parse(env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.equal(payout.total_paid_usd, eachCents / 100);
  assert.equal(payout.unconnected_usd, eachCents / 100);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 10_000_000 - eachCents);
  assert.equal(state.paid_cents, eachCents);

  // Backup parts and a manifest, then the working records are gone.
  const parts = [...env.SCENES.objects.keys()].filter((k) => k.startsWith(`ledger-backups/${D}/`));
  assert.ok(parts.includes(`ledger-backups/${D}/manifest.json`));
  const t = env.TREASURY.instances.get('treasury');
  assert.equal([...t.storage.map.keys()].filter((k) => k.startsWith(`u:${D}:`)).length, 0);
  assert.ok(env.PAYOUTS.store.has(`settlement:${D}`));
});

test('settling a day twice credits and pays nothing twice', async () => {
  const { env, counter } = world({ treasuryUsd: 50_000 });
  const D = day(1);
  user(env, 'u-1', { stripe_connect_id: 'acct_1' });
  contrib(env, D, 'u-1', 1440);
  const s = stripe();
  await withFetch(s.fetch, () => settle(env, counter, D));
  const balance = await ledgerBalanceMinor(env, 'u-1');
  const supply = env.PAYOUTS.store.get('bliss:supply_minor').value;
  const again = await treasuryCall(env, 'start_settlement', { date: D });
  assert.equal(again.started, false);
  await withFetch(s.fetch, () => treasuryCall(env, 'run_steps', { max: 50 }));
  assert.equal(await ledgerBalanceMinor(env, 'u-1'), balance);
  assert.equal(env.PAYOUTS.store.get('bliss:supply_minor').value, supply);
  assert.equal(s.transfers.length, 1);
});

test('value score joins the day with its caps, and a buyer buying from themselves never scores', async () => {
  const { env, counter } = world();
  const D = day(1);
  const at = new Date(`${D}T12:00:00Z`);
  user(env, 'creator');
  user(env, 'worker');
  contrib(env, D, 'worker', 100);
  await creditValueScore(env, { creatorId: 'creator', buyerId: 'whale', creatorTickets: 4000, ref: 'pur_a' }, at);
  await creditValueScore(env, { creatorId: 'creator', buyerId: 'fan', creatorTickets: 200, ref: 'pur_b' }, at);
  await creditValueScore(env, { creatorId: 'creator', buyerId: 'creator', creatorTickets: 5000, ref: 'pur_self' }, at);
  const { job } = await settle(env, counter, D);
  const expectedValue = (VALUE_CAP_TICKETS_PER_BUYER + 200) * VALUE_SCORE_PER_TICKET;
  assert.equal(job.total_score, 100 + expectedValue);
  const dist = JSON.parse(env.PAYOUTS.store.get(`distribution:${D}`).value);
  const creatorRow = dist.recipients.find((r) => r.user_id === 'creator');
  assert.equal(creatorRow.score, expectedValue);
});

test('a large day settles in steps that each stay inside the KV operation limit', async (t) => {
  const { env, counter } = world({ treasuryUsd: 20_000 });
  const D = day(1);
  for (let i = 0; i < 180; i++) {
    const id = `u-${String(i).padStart(4, '0')}`;
    user(env, id, i % 3 === 0 ? { stripe_connect_id: `acct_${i}` } : {});
    contrib(env, D, id, 10 + (i % 50), { metadata: i % 2 === 0 });
    // Some history without metadata, as older entries have.
    env.PAYOUTS.store.set(`entry:${id}:2026-01-01T00:00:00.000Z:dist`, {
      value: JSON.stringify({ amount_minor: 100, kind: 'emission', ref: '2026-01-01', ts: '2026-01-01T00:00:00.000Z' }),
    });
  }
  const s = stripe();
  const { job, peak } = await withFetch(s.fetch, () => settle(env, counter, D));
  assert.equal(job.phase, 'done', JSON.stringify(job.errors));
  t.diagnostic(`most KV operations in one step: ${peak}, over ${job.steps} steps`);
  assert.ok(peak <= 1000, `a step made ${peak} KV operations`);
  // All 180 in one invocation would have taken several thousand.
  assert.ok(job.steps > 5);
  assert.equal(job.contributors, 180);
  // Every entry now carries metadata, so balances are sums of listings.
  const legacy = [...env.PAYOUTS.store.entries()].filter(([k, v]) => k.startsWith('entry:') && !v.metadata);
  assert.equal(legacy.length, 0);
  const dist = JSON.parse(env.PAYOUTS.store.get(`distribution:${D}`).value);
  assert.equal(dist.recipients.length, 180);
});

test('a transfer Stripe refuses goes back to the pool; one that failed for a while is paid', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(env, 'refused', { stripe_connect_id: 'acct_bad' });
  user(env, 'flaky', { stripe_connect_id: 'acct_flaky' });
  contrib(env, D, 'refused', 720);
  contrib(env, D, 'flaky', 720);
  const s = stripe({ refuse: new Set(['acct_bad']), flaky: new Map([['acct_flaky', 1]]) });
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D));
  assert.equal(job.phase, 'done');
  const payout = JSON.parse(env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.equal(payout.contributors_paid, 1);
  assert.equal(payout.failed.length, 1);
  assert.equal(payout.failed[0].status, 'failed');
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 10_000_000 - payout.payouts[0].amount_cents);
});

test('a transfer whose answer was lost is found in Stripe and counted once', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(env, 'lost', { stripe_connect_id: 'acct_lost' });
  contrib(env, D, 'lost', 1440);
  const s = stripe({ lostAnswers: new Set(['acct_lost']) });
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D));
  assert.equal(job.phase, 'done');
  assert.equal(s.transfers.length, 1);
  assert.ok(s.calls.list >= 1);
  const payout = JSON.parse(env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.equal(payout.contributors_paid, 1);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 10_000_000 - s.transfers[0].amount);
  assert.equal(state.paid_cents, s.transfers[0].amount);
});

test('a share under Stripe\'s minimum stays in the treasury', async () => {
  const { env, counter } = world({ treasuryUsd: 50 });
  const D = day(1);
  user(env, 'small', { stripe_connect_id: 'acct_small' });
  contrib(env, D, 'small', 1440);
  const s = stripe();
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D));
  // 0.276% of $50 is under $0.50, so no payout runs at all.
  assert.equal(job.payout.skipped, 'below_stripe_minimum');
  assert.ok(Math.floor(5000 * TREASURY_DRIP_RATE) < STRIPE_MIN_TRANSFER_CENTS);
  assert.equal(s.transfers.length, 0);
});

test('a day that has not ended, or is too old to credit safely, is refused', async () => {
  const { env, counter } = world();
  const today = await treasuryCall(env, 'start_settlement', { date: day(0) });
  assert.equal(today.ok, false);
  assert.equal(today.code, 'day_not_over');
  const old = day(MAX_CREDIT_AGE_DAYS + 3);
  user(env, 'u-1');
  contrib(env, old, 'u-1', 100);
  const { job } = await settle(env, counter, old);
  assert.equal(job.phase, 'failed');
  assert.equal(await ledgerBalanceMinor(env, 'u-1'), 0);
});

test('score dates settle in order: a second date waits for the first', async () => {
  const { env, counter } = world();
  const d2 = day(2);
  const d1 = day(1);
  user(env, 'u-1');
  contrib(env, d2, 'u-1', 100);
  contrib(env, d1, 'u-1', 100);
  await treasuryCall(env, 'start_settlement', { date: d2 });
  const queued = await treasuryCall(env, 'start_settlement', { date: d1 });
  assert.equal(queued.queued, true);
  for (let i = 0; i < 500; i++) {
    await treasuryCall(env, 'run_steps', { max: 1 });
    const s1 = await treasuryCall(env, 'settlement', { date: d1 });
    if (s1.ok && s1.settlement.phase === 'done') break;
  }
  const s2 = (await treasuryCall(env, 'settlement', { date: d2 })).settlement;
  const s1 = (await treasuryCall(env, 'settlement', { date: d1 })).settlement;
  assert.equal(s2.phase, 'done');
  assert.equal(s1.phase, 'done');
  // The second day's emission is computed from the supply the first left.
  assert.equal(s1.supply_before_minor, s2.supply_before_minor + s2.minted_minor);
  void counter;
});

test('a day settled before the treasury existed is left as it was', async () => {
  const { env, counter } = world();
  const D = day(1);
  user(env, 'u-1');
  contrib(env, D, 'u-1', 100);
  env.PAYOUTS.store.set(`distribution:${D}`, { value: JSON.stringify({ date: D, minted: 1 }) });
  const { job } = await settle(env, counter, D);
  assert.equal(job.phase, 'done');
  assert.equal(job.payout.skipped, 'settled_before_treasury');
  assert.equal(await ledgerBalanceMinor(env, 'u-1'), 0);
});
