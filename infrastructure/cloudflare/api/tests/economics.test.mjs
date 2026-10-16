// The Economics page's Actual mode: the Treasury's month totals, the public statement and the admin statement with its cost ledger.
import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { Treasury, treasuryCall, splitSale } from '../src/treasury.mjs';
import { resolvePeriod, payoutDates, receiptsOf, COST_CATEGORIES } from '../src/economics.mjs';

const worker = await loadWorker();
const ADMIN = '56565656-5656-4656-8656-565656565656';
const PLAYER = '12121212-1212-4212-8212-121212121212';
const ORIGIN = 'https://eustress.dev';

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN };
  env.TREASURY = fakeDoNamespace(Treasury, env);
  env.USERS.store.set(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user' }));
  env.USERS.store.set(`admin:${ADMIN}`, '1');
  env.USERS.store.set(`user:${PLAYER}`, JSON.stringify({ id: PLAYER, username: 'player' }));
  return env;
}

/// A deposit as the webhook files it: `gross` cents of `kind`, split as the web channel splits it.
async function deposit(env, ref, kind, gross, extra = {}) {
  const split = splitSale(gross, 'web');
  const r = await treasuryCall(env, 'deposit', {
    ref, kind, gross_cents: gross, fee_cents: split.fee_cents, treasury_cents: split.treasury_cents, platform_cents: split.platform_cents,
    payment_intent: `pi_${ref}`, channel: 'web', ...extra,
  });
  assert.equal(r.ok, true, JSON.stringify(r));
  return r.deposit;
}

/// Move a stored record's timestamp, as if it was written in another month (the Treasury stamps records with the time now).
async function backdate(env, key, iso) {
  const t = env.TREASURY.instances.get('treasury');
  const rec = await t.storage.get(key);
  rec.at = iso;
  await t.storage.put(key, rec);
}

const month = () => new Date().toISOString().slice(0, 7);

const req = (path, { token, method = 'GET', body } = {}) => new Request(`https://api.eustress.dev${path}`, {
  method,
  headers: { Origin: ORIGIN, 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  body: body ? JSON.stringify(body) : undefined,
});

// ── Periods ────────────────────────────────────────────────────────────────

test('a period is a calendar month or a quarter, the current one by default, and nothing else', () => {
  const now = new Date('2026-10-07T12:00:00Z');
  const m = resolvePeriod('month', null, now);
  assert.deepEqual([m.label, m.months, m.from, m.to, m.current], ['2026-10', ['2026-10'], '2026-10-01', '2026-10-31', true]);
  const q = resolvePeriod('quarter', '2026-Q1', now);
  assert.deepEqual([q.label, q.months, q.from, q.to, q.current], ['2026-Q1', ['2026-01', '2026-02', '2026-03'], '2026-01-01', '2026-03-31', false]);
  assert.equal(resolvePeriod('quarter', null, now).label, '2026-Q4');
  assert.equal(resolvePeriod('month', '2024-02', now).to, '2024-02-29', 'a leap February');
  for (const [k, at] of [['month', '2026-13'], ['month', '26-01'], ['quarter', '2026-Q5'], ['year', null]]) assert.equal(resolvePeriod(k, at, now), null, `${k} ${at}`);
  // payouts are read for the days that have ended
  assert.deepEqual(payoutDates(m, now).slice(-1), ['2026-10-06']);
  assert.equal(payoutDates(m, now).length, 6);
  assert.equal(payoutDates(q, now).length, 90);
});

// ── Month totals in the Treasury ───────────────────────────────────────────

test('each deposit adds to its month by kind, in the same write as the money', async () => {
  const env = world();
  await deposit(env, 'cs_t1', 'ticket_purchase', 999, { tickets: 880 });
  await deposit(env, 'cs_t2', 'ticket_purchase', 1999, { tickets: 1840 });
  await deposit(env, 'in_p1', 'subscription', 999);
  await deposit(env, 'cs_d1', 'delta_purchase', 2500, { delta: 2500 });
  await deposit(env, 'cs_t1', 'ticket_purchase', 999, { tickets: 880 }); // Stripe delivers it twice: counted once
  const r = await treasuryCall(env, 'period_totals', { months: [month()] });
  const t = r.months[0];
  assert.equal(t.deposits.ticket_purchase.count, 2);
  assert.equal(t.deposits.ticket_purchase.gross_cents, 999 + 1999);
  assert.equal(t.deposits.ticket_purchase.tickets, 880 + 1840);
  assert.equal(t.deposits.subscription.count, 1);
  assert.equal(t.deposits.delta_purchase.delta, 2500);
  // the totals agree with the balances
  const { state } = await treasuryCall(env, 'snapshot');
  const sum = (f) => Object.values(t.deposits).reduce((a, row) => a + row[f], 0);
  assert.equal(sum('treasury_cents'), state.cents);
  assert.equal(sum('platform_cents'), state.platform_cents);
  assert.equal(sum('fee_cents'), state.fees_cents);
  // a month with nothing in it reads as empty, and bad months are refused
  assert.deepEqual((await treasuryCall(env, 'period_totals', { months: ['2001-01'] })).months[0].deposits, {});
  assert.equal((await treasuryCall(env, 'period_totals', { months: ['2001-13'] })).status, 400);
  assert.equal((await treasuryCall(env, 'period_totals', { months: [] })).status, 400);
});

test('refunds and disputes count in the month they happen, against the kind they reverse, and a dispute won puts them back', async () => {
  const env = world();
  await deposit(env, 'cs_a', 'ticket_purchase', 999, { tickets: 880 });
  await deposit(env, 'in_b', 'subscription', 1299);
  await treasuryCall(env, 'refund', { payment_intent: 'pi_cs_a', refunded_total_cents: 999 });
  await treasuryCall(env, 'dispute', { payment_intent: 'pi_in_b', dispute_id: 'dp_1', amount_cents: 1299 });
  let t = (await treasuryCall(env, 'period_totals', { months: [month()] })).months[0];
  assert.equal(t.reversals.ticket_purchase.gross_cents, 999);
  assert.equal(t.reversals.ticket_purchase.tickets, 880);
  assert.equal(t.reversals.subscription.gross_cents, 1299);
  let r = receiptsOf([t]);
  assert.equal(r.totals.gross_usd, 22.98);
  assert.equal(r.totals.refunded_usd, 22.98);
  assert.equal(r.totals.treasury_after_refunds_usd, 0);
  assert.equal(r.totals.eustress_after_refunds_usd, 0);
  await treasuryCall(env, 'dispute_won', { dispute_id: 'dp_1' });
  t = (await treasuryCall(env, 'period_totals', { months: [month()] })).months[0];
  assert.equal(t.restored.subscription.gross_cents, 1299);
  r = receiptsOf([t]);
  assert.equal(r.totals.refunded_usd, 9.99);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(Math.round(r.totals.treasury_after_refunds_usd * 100), state.cents);
  assert.equal(Math.round(r.totals.eustress_after_refunds_usd * 100), state.platform_cents);
  assert.equal(r.by_kind.subscription.refunded_usd, 0);
  assert.equal(r.by_kind.ticket_purchase.refunded_usd, 9.99);
  assert.equal(r.totals.plan_invoices, 1);
});

test('a refund of money already paid out takes the shortfall from Eustress\'s half, in the totals as in the balances', async () => {
  const env = world();
  await deposit(env, 'cs_p', 'ticket_purchase', 999, { tickets: 880 });
  const t0 = env.TREASURY.instances.get('treasury');
  const st = await t0.storage.get('state');
  st.cents = 100; // most of the pool was paid out
  await t0.storage.put('state', st);
  await treasuryCall(env, 'refund', { payment_intent: 'pi_cs_p', refunded_total_cents: 999 });
  const r = receiptsOf((await treasuryCall(env, 'period_totals', { months: [month()] })).months);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(Math.round(r.totals.eustress_after_refunds_usd * 100), state.platform_cents);
  assert.equal(r.totals.eustress_after_refunds_usd, -3.70);
});

test("a dispute won after a shortfall gives Eustress's half back what it bore, in the totals as in the balances", async () => {
  const env = world();
  await deposit(env, 'cs_s', 'ticket_purchase', 999, { tickets: 880 });
  const t0 = env.TREASURY.instances.get('treasury');
  const st = await t0.storage.get('state');
  st.cents = 100;
  await t0.storage.put('state', st);
  await treasuryCall(env, 'dispute', { payment_intent: 'pi_cs_s', dispute_id: 'dp_s', amount_cents: 999 });
  await treasuryCall(env, 'dispute_won', { dispute_id: 'dp_s' });
  const r = receiptsOf((await treasuryCall(env, 'period_totals', { months: [month()] })).months);
  const { state } = await treasuryCall(env, 'snapshot');
  assert.equal(Math.round(r.totals.eustress_after_refunds_usd * 100), state.platform_cents);
  // the Treasury received its whole share in the month; what it had paid out before is not a receipt
  assert.equal(Math.round(r.totals.treasury_after_refunds_usd * 100), splitSale(999, 'web').treasury_cents);
  assert.equal(r.totals.refunded_usd, 0);
});

test('rebuilding the totals from the records gives the totals the writes kept, months apart', async () => {
  const env = world();
  await deposit(env, 'cs_old', 'ticket_purchase', 4999, { tickets: 5000 });
  await deposit(env, 'cs_new', 'delta_purchase', 1000, { delta: 1000 });
  await treasuryCall(env, 'refund', { payment_intent: 'pi_cs_new', refunded_total_cents: 500 });
  // the old deposit was made in August: move its record, then rebuild
  await backdate(env, 'dep:cs_old', '2026-08-15T10:00:00.000Z');
  const rebuilt = await treasuryCall(env, 'rebuild_totals');
  assert.deepEqual([rebuilt.ok, rebuilt.deposits, rebuilt.reversals], [true, 2, 1]);
  const [aug, now] = (await treasuryCall(env, 'period_totals', { months: ['2026-08', month()] })).months;
  assert.equal(aug.deposits.ticket_purchase.gross_cents, 4999);
  assert.equal(now.deposits.ticket_purchase, undefined, 'the August deposit left this month');
  assert.equal(now.deposits.delta_purchase.gross_cents, 1000);
  assert.equal(now.reversals.delta_purchase.gross_cents, 500);
  // a month left with nothing is dropped: move the Delta deposit and its refund away too
  await backdate(env, 'dep:cs_new', '2026-07-01T00:00:00.000Z');
  const t1 = env.TREASURY.instances.get('treasury');
  for (const [k, v] of await t1.storage.list({ prefix: 'rev:' })) { v.at = '2026-07-02T00:00:00.000Z'; await t1.storage.put(k, v); }
  await treasuryCall(env, 'rebuild_totals');
  assert.equal(await t1.storage.get('agg:' + month()), undefined, 'this month has no totals left');
  await backdate(env, 'dep:cs_new', new Date().toISOString());
  for (const [k, v] of await t1.storage.list({ prefix: 'rev:' })) { v.at = new Date().toISOString(); await t1.storage.put(k, v); }
  await treasuryCall(env, 'rebuild_totals');
  // a second rebuild changes nothing
  await treasuryCall(env, 'rebuild_totals');
  assert.deepEqual((await treasuryCall(env, 'period_totals', { months: ['2026-08', month()] })).months, [aug, now]);
});

// ── The routes ─────────────────────────────────────────────────────────────

test('the public statement has the Treasury and its payouts and nothing about receipts or costs', async () => {
  const env = world();
  await deposit(env, 'cs_x', 'ticket_purchase', 9999, { tickets: 10800 });
  const y = new Date(Date.now() - 86400e3).toISOString().slice(0, 10);
  env.PAYOUTS.store.set(`payout:${y}`, JSON.stringify({ total_paid_usd: 12.34, contributors_paid: 3, drip_usd: 13 }));
  const at = y.slice(0, 7);
  const res = await worker.fetch(req(`/api/economics/actual?period=month&at=${at}`), env, ctx);
  assert.equal(res.status, 200);
  assert.equal(res.headers.get('Access-Control-Allow-Origin'), '*');
  const body = await res.json();
  assert.equal(body.period.label, at);
  assert.equal(body.treasury.balance_usd, splitSale(9999, 'web').treasury_cents / 100);
  assert.ok(body.treasury.daily_drip_usd > 0);
  assert.equal(body.contributors.paid_usd, 12.34);
  assert.deepEqual(body.contributors.days, [{ date: y, paid_usd: 12.34, contributors: 3, drip_usd: 13 }]);
  for (const k of ['receipts', 'expenses', 'settings']) assert.equal(body[k], undefined, `public has no ${k}`);
  const text = JSON.stringify(body);
  for (const word of ['platform', 'eustress', 'gross', 'labor', 'costs']) assert.ok(!text.includes(word), `public says ${word}`);
  assert.equal((await worker.fetch(req('/api/economics/actual?period=week'), env, ctx)).status, 400);
});

test('the admin statement is for administrators only', async () => {
  const env = world();
  assert.equal((await worker.fetch(req('/api/admin/economics/actual'), env, ctx)).status, 403);
  assert.equal((await worker.fetch(req('/api/admin/economics/actual', { token: mintToken(PLAYER) }), env, ctx)).status, 403);
  for (const [path, method] of [['/api/admin/economics/costs', 'PUT'], ['/api/admin/economics/settings', 'PUT'], ['/api/admin/economics/rebuild', 'POST']]) {
    assert.equal((await worker.fetch(req(path, { method, token: mintToken(PLAYER), body: {} }), env, ctx)).status, 403, path);
  }
});

test('the admin statement has receipts by kind, the cost ledger, labor at its share and what Eustress keeps', async () => {
  const env = world();
  const token = mintToken(ADMIN);
  await deposit(env, 'cs_1', 'ticket_purchase', 999, { tickets: 880 });
  await deposit(env, 'in_1', 'subscription', 999);
  const m = month();
  const put = (body) => worker.fetch(req('/api/admin/economics/costs', { method: 'PUT', token, body }), env, ctx);
  assert.equal((await put({ month: m, category: 'cloudflare', amount_usd: 5, note: 'Workers Paid' })).status, 200);
  assert.equal((await put({ month: m, category: 'servers', amount_usd: 2.5 })).status, 200);
  assert.equal((await put({ month: m, category: 'servers', amount_usd: 1.25 })).status, 200, 'setting again replaces');
  assert.equal((await put({ month: m, category: 'domain', amount_usd: 0.83 })).status, 200);
  const removed = await put({ month: m, category: 'domain', amount_usd: 0 });
  assert.equal(removed.status, 200);
  assert.deepEqual(Object.keys((await removed.json()).costs).sort(), ['cloudflare', 'servers'], 'zero removes the category');
  for (const bad of [{ month: '2026-13', category: 'servers', amount_usd: 1 }, { month: m, category: 'labor', amount_usd: 1 }, { month: m, category: 'servers', amount_usd: -1 }, { month: m, category: 'servers', amount_usd: 'x' }]) {
    assert.equal((await put(bad)).status, 400, JSON.stringify(bad));
  }
  const set = (labor_share) => worker.fetch(req('/api/admin/economics/settings', { method: 'PUT', token, body: { labor_share } }), env, ctx);
  assert.equal((await set(1.5)).status, 400);
  assert.equal((await set(0.4)).status, 200);
  const res = await worker.fetch(req('/api/admin/economics/actual?period=month', { token }), env, ctx);
  assert.equal(res.status, 200);
  const s = await res.json();
  assert.equal(s.receipts.by_kind.ticket_purchase.name, 'Ticket packs');
  assert.equal(s.receipts.by_kind.subscription.count, 1);
  assert.equal(s.receipts.totals.gross_usd, 19.98);
  const half = s.receipts.totals.eustress_after_refunds_usd;
  assert.equal(half, (splitSale(999, 'web').platform_cents * 2) / 100);
  assert.deepEqual(s.expenses.by_category, { cloudflare: 5, servers: 1.25, domain: 0, tools: 0, other: 0 });
  assert.equal(s.expenses.labor.share, 0.4);
  assert.equal(s.expenses.labor.usd, Math.round(half * 0.4 * 100) / 100);
  assert.equal(s.expenses.total_usd, Math.round((6.25 + s.expenses.labor.usd) * 100) / 100);
  assert.equal(s.expenses.retained_usd, Math.round((half - s.expenses.total_usd) * 100) / 100);
  assert.equal(s.expenses.by_month[m].cloudflare.note, 'Workers Paid');
  assert.deepEqual(COST_CATEGORIES, ['cloudflare', 'servers', 'domain', 'tools', 'other']);
  // and the treasury figures the public sees are the same
  const pub = await (await worker.fetch(req('/api/economics/actual?period=month'), env, ctx)).json();
  assert.deepEqual(pub.treasury, s.treasury);
});

test('an administrator can rebuild the month totals from the routes', async () => {
  const env = world();
  await deposit(env, 'cs_r', 'ticket_purchase', 999, { tickets: 880 });
  const res = await worker.fetch(req('/api/admin/economics/rebuild', { method: 'POST', token: mintToken(ADMIN) }), env, ctx);
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { ok: true, months: 1, deposits: 1, reversals: 0 });
});
