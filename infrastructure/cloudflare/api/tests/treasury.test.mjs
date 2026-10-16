import test from 'node:test';
import assert from 'node:assert/strict';
import {
  Treasury, treasuryCall, splitSale, dripFor, decayedHwm, seedState,
  TREASURY_DRIP_RATE, STRIPE_MIN_TRANSFER_CENTS, MAX_CREDIT_AGE_DAYS, HOLD_MAX_REJECTED_ATTEMPTS,
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
function stripe({ refuse = new Set(), flaky = new Map(), lostAnswers = new Set(), errors = new Map(), listDown = { on: false }, lateBy = new Map() } = {}) {
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
        return new Response(JSON.stringify({ error: { message: 'Account cannot receive transfers', code: 'account_invalid' } }), { status: 400 });
      }
      if (errors.has(dest)) {
        const { status, code } = errors.get(dest);
        return new Response(JSON.stringify({ error: { message: `Stripe says ${code || status}`, code } }), { status });
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
        // A transfer Stripe completes only after our request timed out:
        // the list shows it once that time has passed.
        if (lateBy.has(dest)) t.visible_at = Date.now() + lateBy.get(dest);
        byKey.set(key, t);
        transfers.push(t);
      }
      if (lateBy.has(dest)) throw new Error('timed out');
      if (lostAnswers.has(dest)) throw new Error('connection reset');
      return new Response(JSON.stringify(t), { status: 200 });
    }
    if (u.pathname === '/v1/transfers' && (init.method || 'GET') === 'GET') {
      calls.list += 1;
      if (listDown.on) return new Response(JSON.stringify({ error: { message: 'Stripe is unavailable' } }), { status: 503 });
      const dest = u.searchParams.get('destination');
      const visible = transfers.filter((t) => t.destination === dest && !(t.visible_at > Date.now()));
      return new Response(JSON.stringify({ data: visible, has_more: false }), { status: 200 });
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

/// A test account. One with a Stripe Connect account also gets an active
/// Creator plan, since cash-out needs one, unless `no_plan` is set.
function user(env, id, extra = {}) {
  const { no_plan, ...fields } = extra;
  env.USERS.store.set(`user:${id}`, { value: JSON.stringify({ id, username: id, ledger_migrated: true, ...fields }) });
  if (fields.stripe_connect_id && !no_plan) {
    env.USERS.store.set(`plan:${id}`, { value: JSON.stringify({ plan: 'creator', status: 'active', current_period_end: Math.floor(Date.now() / 1000) + 30 * 86400 }) });
  }
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

test('a destination Stripe refuses is held for review, not returned to the pool; one that failed for a while is paid', async () => {
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
  assert.deepEqual(payout.failed, [], 'nothing is forfeited');
  assert.equal(payout.destination_review_count, 1);
  const share = payout.payouts[0].amount_cents;
  assert.equal(payout.destination_review_usd, share / 100);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.deepEqual([state.held_cents, state.review_cents], [share, share]);
  assert.equal(state.cents, 10_000_000 - 2 * share, 'the refused share left the pool once, into the hold');
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


test('a connected contributor without a cash-out plan keeps their share in the treasury', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(env, 'with-plan', { stripe_connect_id: 'acct_plan' });
  user(env, 'no-plan', { stripe_connect_id: 'acct_noplan', no_plan: true });
  contrib(env, D, 'with-plan', 720);
  contrib(env, D, 'no-plan', 720);
  const s = stripe();
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D));
  assert.equal(job.phase, 'done');
  assert.deepEqual(s.transfers.map((t) => t.destination), ['acct_plan']);
  const payout = JSON.parse(env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.equal(payout.contributors_paid, 1);
  assert.equal(payout.no_plan_usd, s.transfers[0].amount / 100);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.cents, 10_000_000 - s.transfers[0].amount);
});

test('a refund of a Delta pack reverses the Delta it bought', async () => {
  const { env } = world();
  await treasuryCall(env, 'deposit', { ref: 'stripe:cs_dp', kind: 'delta_purchase', gross_cents: 1000, fee_cents: 59, treasury_cents: 470, platform_cents: 471, user_id: 'u-1', payment_intent: 'pi_dp', delta: 1000 });
  const half = await treasuryCall(env, 'refund', { payment_intent: 'pi_dp', refunded_total_cents: 500 });
  assert.equal(half.reversal.delta, 500);
  assert.equal(half.reversal.tickets, 0);
  const rest = await treasuryCall(env, 'refund', { payment_intent: 'pi_dp', refunded_total_cents: 1000 });
  assert.equal(half.reversal.delta + rest.reversal.delta, 1000);
});

test('a contributor whose key was just replaced is not paid while the freeze lasts; their share is held for them', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(env, 'free', { stripe_connect_id: 'acct_free' });
  user(env, 'frozen', { stripe_connect_id: 'acct_frozen' });
  contrib(env, D, 'free', 720);
  contrib(env, D, 'frozen', 720);
  // What the account's object keeps for code with no session (src/fresh_auth.mjs).
  env.USERS.store.set('valuefreeze:frozen', { value: String(Date.now() + 24 * 3600 * 1000) });
  const s = stripe();
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D));
  assert.equal(job.phase, 'done');
  assert.deepEqual(s.transfers.map((t) => t.destination), ['acct_free']);
  const payout = JSON.parse(env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.equal(payout.contributors_paid, 1);
  assert.equal(payout.frozen_usd, s.transfers[0].amount / 100, 'the held share is reported');
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.held_cents, s.transfers[0].amount, 'the frozen share is held, not sent');
  assert.equal(state.cents, 10_000_000 - 2 * s.transfers[0].amount, 'a held share is out of the pool the drip is taken from');
});

test('a freeze that has ended pays as usual, and one that cannot be read holds the payout rather than sending it', async () => {
  const ended = world({ treasuryUsd: 100_000 });
  const D = day(1);
  user(ended.env, 'was-frozen', { stripe_connect_id: 'acct_was' });
  contrib(ended.env, D, 'was-frozen', 720);
  ended.env.USERS.store.set('valuefreeze:was-frozen', { value: String(Date.now() - 1000) });
  const paid = stripe();
  await withFetch(paid.fetch, () => settle(ended.env, ended.counter, D));
  assert.deepEqual(paid.transfers.map((t) => t.destination), ['acct_was']);

  const blind = world({ treasuryUsd: 100_000 });
  user(blind.env, 'someone', { stripe_connect_id: 'acct_someone' });
  contrib(blind.env, D, 'someone', 720);
  const realGet = blind.env.USERS.get;
  blind.env.USERS.get = async (key, ...rest) => {
    if (String(key).startsWith('valuefreeze:')) throw new Error('KV unavailable');
    return realGet(key, ...rest);
  };
  const held = stripe();
  const { job } = await withFetch(held.fetch, () => settle(blind.env, blind.counter, D));
  assert.equal(job.phase, 'done');
  assert.deepEqual(held.transfers, [], 'money that might belong to a frozen account is not sent');
  const payout = JSON.parse(blind.env.PAYOUTS.store.get(`payout:${D}`).value);
  assert.ok(payout.frozen_usd > 0);
});

test('a held payout is paid exactly once after the freeze ends, to the destination of its day', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2, D3] = [day(3), day(2), day(1)];
  user(env, 'replaced', { stripe_connect_id: 'acct_pinned' });
  contrib(env, D1, 'replaced', 720);
  env.USERS.store.set('valuefreeze:replaced', { value: String(Date.now() + 3 * 24 * 3600 * 1000) });
  const s = stripe();

  // Day 1: frozen, so held.
  await withFetch(s.fetch, () => settle(env, counter, D1));
  assert.deepEqual(s.transfers, []);
  const owed = (await treasuryCall(env, 'snapshot')).state.held_cents;
  assert.ok(owed > 0);

  // Day 2: still frozen; the hold waits.
  const waiting = await withFetch(s.fetch, () => settle(env, counter, D2));
  assert.equal(waiting.job.holds.waiting, 1);
  assert.deepEqual(s.transfers, []);

  // The freeze ends. Day 3's run releases the hold, once.
  env.USERS.store.set('valuefreeze:replaced', { value: String(Date.now() - 1000) });
  const { job } = await withFetch(s.fetch, () => settle(env, counter, D3));
  assert.equal(job.phase, 'done');
  assert.deepEqual(s.transfers.map((t) => [t.destination, t.amount, t.metadata.date]), [['acct_pinned', owed, D1]]);
  assert.deepEqual([job.holds.released, job.holds.released_cents], [1, owed]);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(state.held_cents, 0);
  const record = JSON.parse(env.PAYOUTS.store.get(`payout:${D1}`).value);
  const mine = record.payouts.find((p) => p.user_id === 'replaced');
  assert.equal(mine.amount_cents, owed, 'it shows in the history of the day it was earned');
  assert.ok(mine.released_at);
  assert.deepEqual([record.frozen_usd, record.contributors_paid], [0, 1]);
});

test('a held payout whose answer is lost is found at Stripe, not sent twice; one for a banned account waits for review', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2, D3] = [day(3), day(2), day(1)];
  user(env, 'lossy', { stripe_connect_id: 'acct_lossy' });
  user(env, 'banned-later', { stripe_connect_id: 'acct_banned' });
  contrib(env, D1, 'lossy', 720);
  contrib(env, D1, 'banned-later', 720);
  for (const id of ['lossy', 'banned-later']) env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() + 86400e3) });
  const s = stripe({ lostAnswers: new Set(['acct_lossy']) });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  const before = await treasuryCall(env, 'snapshot');
  const each = before.state.held_cents / 2;

  for (const id of ['lossy', 'banned-later']) env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() - 1000) });
  user(env, 'banned-later', { stripe_connect_id: 'acct_banned', banned: true });
  const second = await withFetch(s.fetch, () => settle(env, counter, D2));
  assert.equal(second.job.holds.unresolved, 1, 'the lost answer is tried again later');
  assert.equal(second.job.holds.review_cents, each, 'the banned account is not paid; its money waits for review');

  const third = await withFetch(s.fetch, () => settle(env, counter, D3));
  assert.equal(third.job.holds.released, 1);
  assert.deepEqual(s.transfers.map((t) => t.destination), ['acct_lossy'], 'one transfer, found rather than resent');
  assert.equal(s.calls.create, 1);
  assert.equal(third.job.holds.review, 1, 'a hold in review is not tried again');
  const { state, view } = await treasuryCall(env, 'snapshot');
  assert.deepEqual([state.held_cents, state.review_cents], [each, each], 'still owed, set aside');
  assert.equal(view.held_review_usd, each / 100);
  assert.equal(state.cents, before.state.cents, 'nothing went back to the pool');
});

test('a held payout goes to the destination of the day it was earned, not one named later', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  user(env, 'moved', { stripe_connect_id: 'acct_day' });
  contrib(env, D1, 'moved', 720);
  env.USERS.store.set('valuefreeze:moved', { value: String(Date.now() + 86400e3) });
  const s = stripe();
  await withFetch(s.fetch, () => settle(env, counter, D1));
  // The account names a new destination after the day (as a taken account would).
  user(env, 'moved', { stripe_connect_id: 'acct_new' });
  env.USERS.store.set('valuefreeze:moved', { value: String(Date.now() - 1000) });
  await withFetch(s.fetch, () => settle(env, counter, D2));
  assert.deepEqual(s.transfers.map((t) => t.destination), ['acct_day']);
});

test('a hold in review leaves only by an administrator, or after the retention', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2, D3] = [day(3), day(2), day(1)];
  user(env, 'banned', { stripe_connect_id: 'acct_b' });
  user(env, 'closed', { stripe_connect_id: 'acct_closed' });
  user(env, 'stale', { stripe_connect_id: 'acct_s' });
  for (const id of ['banned', 'closed', 'stale']) {
    contrib(env, D1, id, 720);
    env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() + 86400e3) });
  }
  const s = stripe({ refuse: new Set(['acct_closed']) });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  for (const id of ['banned', 'closed', 'stale']) env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() - 1000) });
  user(env, 'banned', { stripe_connect_id: 'acct_b', banned: true });
  user(env, 'stale', { stripe_connect_id: 'acct_s', banned: true });
  const second = await withFetch(s.fetch, () => settle(env, counter, D2));
  assert.equal(second.job.holds.review, 3, 'banned twice, and a destination Stripe refuses for good');
  const listed = await treasuryCall(env, 'holds', { status: 'review' });
  assert.deepEqual(listed.holds.map((h) => [h.user_id, h.review_reason]).sort(),
    [['banned', 'banned'], ['closed', 'destination_refused'], ['stale', 'banned']]);
  const owed = Object.fromEntries(listed.holds.map((h) => [h.user_id, h.cents]));

  // The ban is lifted and an administrator pays; the closed account is paid
  // to the destination its owner set up again, read from the account itself.
  user(env, 'banned', { stripe_connect_id: 'acct_b' });
  assert.equal((await treasuryCall(env, 'resolve_hold', { user_id: 'banned', date: D1, action: 'pay' })).status, 'held');
  user(env, 'closed', { stripe_connect_id: 'acct_reopened' });
  const named = await treasuryCall(env, 'resolve_hold', { user_id: 'closed', date: D1, action: 'pay', destination: 'acct_attacker' });
  assert.deepEqual([named.status, named.code], [400, 'invalid_request'], 'an administrator cannot name a destination');
  const current = await treasuryCall(env, 'resolve_hold', { user_id: 'closed', date: D1, action: 'pay', use_current_destination: true });
  assert.deepEqual([current.status, current.destination], ['held', 'current']);
  assert.equal((await treasuryCall(env, 'resolve_hold', { user_id: 'banned', date: D1, action: 'return' })).code, 'hold_not_in_review');
  await withFetch(s.fetch, () => settle(env, counter, D3));
  assert.deepEqual(s.transfers.map((t) => [t.destination, t.amount]).sort(),
    [['acct_b', owed.banned], ['acct_reopened', owed.closed]]);

  // The last one is never decided: after the retention it goes back. The
  // clock moves 91 days on, and the day settled is the day before that.
  const realNow = Date.now;
  const later = realNow() + 91 * 86400e3;
  Date.now = () => later;
  try {
    const D4 = day(1);
    const before = (await treasuryCall(env, 'snapshot')).state;
    const fourth = await withFetch(s.fetch, () => settle(env, counter, D4));
    assert.equal(fourth.job.holds.returned_cents, owed.stale);
    const after = (await treasuryCall(env, 'snapshot')).state;
    assert.deepEqual([after.held_cents, after.review_cents], [0, 0]);
    assert.equal(after.cents - before.cents >= owed.stale, true);
  } finally {
    Date.now = realNow;
  }
  assert.equal(s.transfers.length, 2);
});

test('paying a hold to the current destination waits for the pin after a key replacement', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  user(env, 'gone-acct', { stripe_connect_id: 'acct_closed' });
  contrib(env, D1, 'gone-acct', 720);
  env.USERS.store.set('valuefreeze:gone-acct', { value: String(Date.now() + 86400e3) });
  const s = stripe({ refuse: new Set(['acct_closed']) });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  env.USERS.store.set('valuefreeze:gone-acct', { value: String(Date.now() - 1000) });
  await withFetch(s.fetch, () => settle(env, counter, D2));
  user(env, 'gone-acct', { stripe_connect_id: 'acct_other' });
  // The account's object says the destination is pinned for another week.
  let pinnedUntil = Date.now() + 7 * 86400e3;
  env.ACCOUNT_AUTH = {
    idFromName: (n) => n,
    get: () => ({ fetch: async () => new Response(JSON.stringify({ ok: true, seeded: true, payout_pinned_until: pinnedUntil })) }),
  };
  const pinned = await treasuryCall(env, 'resolve_hold', { user_id: 'gone-acct', date: D1, action: 'pay', use_current_destination: true });
  assert.deepEqual([pinned.status, pinned.code], [409, 'destination_pinned']);
  env.ACCOUNT_AUTH = { idFromName: (n) => n, get: () => ({ fetch: async () => { throw new Error('unreachable'); } }) };
  const blind = await treasuryCall(env, 'resolve_hold', { user_id: 'gone-acct', date: D1, action: 'pay', use_current_destination: true });
  assert.deepEqual([blind.status, blind.code], [503, 'pin_unknown'], 'an unknown pin refuses');
  pinnedUntil = 0;
  env.ACCOUNT_AUTH = {
    idFromName: (n) => n,
    get: () => ({ fetch: async () => new Response(JSON.stringify({ ok: true, seeded: true, payout_pinned_until: pinnedUntil })) }),
  };
  const ok = await treasuryCall(env, 'resolve_hold', { user_id: 'gone-acct', date: D1, action: 'pay', use_current_destination: true });
  assert.equal(ok.status, 'held');
});

test('a short platform balance or a bad key is tried again, not set aside for review', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2, D3] = [day(3), day(2), day(1)];
  user(env, 'short', { stripe_connect_id: 'acct_short' });
  user(env, 'badkey', { stripe_connect_id: 'acct_badkey' });
  for (const id of ['short', 'badkey']) {
    contrib(env, D1, id, 720);
    env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() + 86400e3) });
  }
  const errors = new Map([
    ['acct_short', { status: 400, code: 'balance_insufficient' }],
    ['acct_badkey', { status: 401, code: null }],
  ]);
  const s = stripe({ errors });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  for (const id of ['short', 'badkey']) env.USERS.store.set(`valuefreeze:${id}`, { value: String(Date.now() - 1000) });
  const second = await withFetch(s.fetch, () => settle(env, counter, D2));
  assert.deepEqual([second.job.holds.unresolved, second.job.holds.review], [2, 0]);
  errors.clear();
  const third = await withFetch(s.fetch, () => settle(env, counter, D3));
  assert.equal(third.job.holds.released, 2);
  assert.deepEqual(s.transfers.map((t) => t.destination).sort(), ['acct_badkey', 'acct_short']);
  assert.equal((await treasuryCall(env, 'snapshot')).state.held_cents, 0);
});

test('a crash after Stripe accepts a held payout leads the next attempt to look, not to send again', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  user(env, 'crash', { stripe_connect_id: 'acct_crash' });
  contrib(env, D1, 'crash', 720);
  env.USERS.store.set('valuefreeze:crash', { value: String(Date.now() + 86400e3) });
  const s = stripe();
  await withFetch(s.fetch, () => settle(env, counter, D1));
  env.USERS.store.set('valuefreeze:crash', { value: String(Date.now() - 1000) });

  // The write that marks the hold paid fails once, after Stripe made the
  // transfer: as if the object were evicted in between.
  const storage = env.TREASURY.instances.get('treasury').storage;
  const realPut = storage.put.bind(storage);
  let failed = false;
  storage.put = async (key, value) => {
    if (!failed && typeof key === 'object' && Object.values(key).some((v) => v?.status === 'paid' && v?.transfer_id)) {
      failed = true;
      throw new Error('evicted');
    }
    return realPut(key, value);
  };
  await assert.rejects(withFetch(s.fetch, () => settle(env, counter, D2)), /step failed/);
  assert.equal(failed, true);
  assert.equal(s.calls.create, 1);
  // The settlement resumes, as the alarm would resume it.
  const done = await withFetch(s.fetch, async () => {
    for (let i = 0; i < 500; i++) {
      await treasuryCall(env, 'run_steps', { max: 1 });
      const st = await treasuryCall(env, 'settlement', { date: D2 });
      if (st.settlement.phase === 'done') return st.settlement;
    }
    throw new Error('did not finish');
  });
  assert.equal(done.holds.released, 1);
  assert.equal(s.calls.create, 1, 'not sent again');
  assert.ok(s.calls.list >= 1, 'Stripe was asked first');
  assert.equal(s.transfers.length, 1);
  assert.equal((await treasuryCall(env, 'snapshot')).state.held_cents, 0);
});

test('a short platform balance on the day holds every share, keeps the books balanced, and pays each once later', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  const ids = ['a', 'b', 'c'];
  for (const id of ids) {
    user(env, id, { stripe_connect_id: `acct_${id}` });
    contrib(env, D1, id, 720);
  }
  const errors = new Map(ids.map((id) => [`acct_${id}`, { status: 400, code: 'balance_insufficient' }]));
  const s = stripe({ errors });
  const books = (st) => st.cents + (st.held_cents || 0) + st.paid_cents;
  const start = (await treasuryCall(env, 'snapshot')).state;
  const first = await withFetch(s.fetch, () => settle(env, counter, D1));
  assert.equal(first.job.phase, 'done');
  const record = JSON.parse(env.PAYOUTS.store.get(`payout:${D1}`).value);
  assert.deepEqual(record.failed, [], 'nothing is forfeited');
  assert.equal(record.stripe_unavailable_count, 3);
  assert.equal(record.stripe_shortfall_alert, true, 'a day Stripe would not pay is loud');
  assert.equal(first.job.payout.stripe_shortfall_alert, true);
  const held = (await treasuryCall(env, 'snapshot')).state;
  assert.equal(books(held), books(start), 'the conversion moves money, it does not make or lose any');
  assert.equal(held.held_cents, Math.round(record.stripe_unavailable_usd * 100));
  assert.equal(held.review_cents || 0, 0, 'a short balance is not a destination fault');
  assert.equal((await treasuryCall(env, 'holds', { status: 'held' })).holds.length, 3);

  errors.clear();
  // A hold from a day failure is first tried an hour on.
  const second = await atClock(2 * 3600e3, () => withFetch(s.fetch, () => settle(env, counter, D2)));
  assert.equal(second.job.holds.released, 3);
  assert.deepEqual(s.transfers.map((t) => t.destination).sort(), ['acct_a', 'acct_b', 'acct_c']);
  const after = (await treasuryCall(env, 'snapshot')).state;
  assert.equal(after.held_cents, 0);
  const updated = JSON.parse(env.PAYOUTS.store.get(`payout:${D1}`).value);
  assert.deepEqual([updated.contributors_paid, updated.stripe_unavailable_usd], [3, 0]);
});

test('a day transfer whose answer is lost, when Stripe cannot be asked that day, is found later and not sent again', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  user(env, 'lost', { stripe_connect_id: 'acct_lost' });
  contrib(env, D1, 'lost', 720);
  const listDown = { on: true };
  const s = stripe({ lostAnswers: new Set(['acct_lost']), listDown });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  assert.equal(s.transfers.length, 1, 'the transfer went through; its answers were lost');
  assert.equal((await treasuryCall(env, 'holds', { status: 'held' })).holds.length, 1);
  const createsOnTheDay = s.calls.create;

  listDown.on = false;
  const second = await atClock(2 * 3600e3, () => withFetch(s.fetch, () => settle(env, counter, D2)));
  assert.equal(second.job.holds.released, 1);
  assert.equal(s.calls.create, createsOnTheDay, 'found at Stripe, not sent again');
  assert.equal(s.transfers.length, 1);
  assert.equal((await treasuryCall(env, 'snapshot')).state.held_cents, 0);
});

/// Run `fn` with the clock `aheadMs` on, as a later run would see it.
async function atClock(aheadMs, fn) {
  const real = Date.now;
  const at = real() + aheadMs;
  Date.now = () => at;
  try {
    return await fn();
  } finally {
    Date.now = real;
  }
}

test('a day transfer that times out but completes at Stripe is not sent again by the same run', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  const [D1, D2] = [day(2), day(1)];
  user(env, 'slow', { stripe_connect_id: 'acct_slow' });
  contrib(env, D1, 'slow', 720);
  // Stripe makes the transfer, but our request times out, and Stripe's list
  // shows it only a minute later.
  const s = stripe({ lateBy: new Map([['acct_slow', 60e3]]) });
  const first = await withFetch(s.fetch, () => settle(env, counter, D1));
  assert.equal(first.job.payout.stripe_unavailable_count, 1);
  assert.equal(first.job.holds.waiting, 1, 'the hold made a moment ago is not tried in the same run');
  assert.equal(s.transfers.length, 1);
  const second = await atClock(2 * 3600e3, () => withFetch(s.fetch, () => settle(env, counter, D2)));
  assert.equal(second.job.holds.released, 1);
  assert.equal(s.transfers.length, 1, 'found, not sent again');
  assert.equal((await treasuryCall(env, 'snapshot')).state.held_cents, 0);
});

test('a rejection that is neither the destination nor ours goes to review after repeated attempts', async () => {
  const { env, counter } = world({ treasuryUsd: 100_000 });
  user(env, 'odd', { stripe_connect_id: 'acct_odd' });
  user(env, 'short', { stripe_connect_id: 'acct_short' });
  const D1 = day(1);
  contrib(env, D1, 'odd', 720);
  contrib(env, D1, 'short', 720);
  const s = stripe({ errors: new Map([
    ['acct_odd', { status: 400, code: 'amount_too_large' }],
    ['acct_short', { status: 400, code: 'balance_insufficient' }],
  ]) });
  await withFetch(s.fetch, () => settle(env, counter, D1));
  // One settlement a day for two weeks, each seeing the clock a day further on.
  let review = null;
  for (let k = 1; k <= 14 && !review; k++) {
    await atClock(k * 86400e3, async () => {
      await withFetch(s.fetch, () => settle(env, counter, day(1)));
      review = (await treasuryCall(env, 'holds', { status: 'review' })).holds.find((h) => h.user_id === 'odd') || null;
    });
  }
  assert.ok(review, 'the odd rejection reached review');
  assert.equal(review.review_reason, 'stripe_rejected');
  assert.match(review.error, /amount_too_large/);
  assert.equal(review.attempts, HOLD_MAX_REJECTED_ATTEMPTS);
  const short = (await treasuryCall(env, 'holds', { status: 'held' })).holds.find((h) => h.user_id === 'short');
  assert.ok(short && short.attempts >= HOLD_MAX_REJECTED_ATTEMPTS, 'a short balance is ours: tried without limit');
});
