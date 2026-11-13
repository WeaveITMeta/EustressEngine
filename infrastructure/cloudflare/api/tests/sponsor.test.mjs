// Engine sponsorships (src/sponsor.mjs): the checkout the Worker opens, where a paid sponsorship's money goes, what a renewal and a
// refund do, and who the public sponsor list names. Driven through the real Worker with Stripe's API stubbed and the real Treasury
// object on in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, fakeEnv, fakeKv, fakeDoNamespace, ctx, mintToken } from './helpers/load_worker.mjs';
import { Treasury, treasuryCall, DEPOSIT_KINDS } from '../src/treasury.mjs';
import { splitSponsorship, readSponsorRequest, SPONSOR_KIND, SPONSOR_TIERS } from '../src/sponsor.mjs';
import { receiptsOf, DEPOSIT_KIND_NAMES } from '../src/economics.mjs';

const worker = await loadWorker();
const base = 'https://api.eustress.dev';
const SECRET = 'whsec_test_not_real';

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), STRIPE_SECRET_KEY: 'sk_test_not_real', STRIPE_WEBHOOK_SECRET: SECRET };
  env.TREASURY = fakeDoNamespace(Treasury, env);
  return env;
}

async function addUser(env, id, username, extra = {}) {
  await env.USERS.put(`user:${id}`, JSON.stringify({ id, username, kyc_verified: true, ...extra }));
}

/// POST the checkout with Stripe stubbed. Resolves to the status, the JSON, and the form sent to Stripe (null when none was).
async function checkout(env, body, { signedIn = true, raw } = {}) {
  const sent = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    assert.equal(String(url), 'https://api.stripe.com/v1/checkout/sessions');
    sent.push(Object.fromEntries(new URLSearchParams(init.body)));
    return new Response(JSON.stringify({ id: 'cs_test_1', url: 'https://checkout.stripe.com/c/pay/cs_test_1' }), { status: 200 });
  };
  try {
    const headers = { 'Content-Type': 'application/json', ...(signedIn ? { Authorization: `Bearer ${mintToken('user-1')}` } : {}) };
    const res = await worker.fetch(new Request(`${base}/api/sponsor/checkout`, { method: 'POST', headers, body: raw ?? JSON.stringify(body) }), env, ctx);
    return { status: res.status, json: await res.json(), form: sent[0] ?? null };
  } finally {
    globalThis.fetch = real;
  }
}

function deliver(env, event) {
  const body = JSON.stringify({ livemode: false, ...event });
  const t = Math.floor(Date.now() / 1000);
  const v1 = crypto.createHmac('sha256', SECRET).update(`${t}.${body}`).digest('hex');
  return worker.fetch(new Request(`${base}/api/stripe/webhook`, { method: 'POST', headers: { 'Stripe-Signature': `t=${t},v1=${v1}` }, body }), env, ctx);
}

const paidSession = (id, cents, { user = 'user-1', listed = '1', mode = 'payment', tier = 'builder', pi = `pi_${id}` } = {}) => ({
  id: `evt_${id}`, type: 'checkout.session.completed',
  data: { object: {
    id, payment_status: 'paid', amount_total: cents, mode, payment_intent: pi,
    metadata: { type: SPONSOR_KIND, tier, listed, ...(user ? { user_id: user } : {}) },
  } },
});

const state = async (env) => (await treasuryCall(env, 'snapshot')).state;
const sponsors = async (env) => (await (await worker.fetch(new Request(`${base}/api/sponsors`), env, ctx)).json());

// ── The split ────────────────────────────────────────────────────────────────

test('a sponsorship is the payment fee plus Eustress\'s share, with nothing for the Treasury, to the cent', () => {
  assert.deepEqual(splitSponsorship(5000), { gross_cents: 5000, fee_cents: 175, treasury_cents: 0, platform_cents: 4825 });
  for (const gross of [0, 1, 29, 31, 500, 999, 100_000, 10_000_000]) {
    const s = splitSponsorship(gross);
    assert.equal(s.fee_cents + s.treasury_cents + s.platform_cents, s.gross_cents, String(gross));
    assert.equal(s.treasury_cents, 0);
    assert.ok(s.platform_cents >= 0, String(gross));
  }
  assert.ok(DEPOSIT_KINDS.includes(SPONSOR_KIND));
  assert.equal(DEPOSIT_KIND_NAMES[SPONSOR_KIND], 'Engine sponsorships');
});

// ── Checkout ─────────────────────────────────────────────────────────────────

test('every tier opens a Stripe page priced inline, once or monthly, with the metadata the webhook settles by', async () => {
  const env = world();
  for (const tier of SPONSOR_TIERS) {
    const once = await checkout(env, { tier: tier.id, mode: 'one_time', listed: true });
    assert.equal(once.status, 200, tier.id);
    assert.equal(once.json.url, 'https://checkout.stripe.com/c/pay/cs_test_1');
    assert.equal(once.form['line_items[0][price_data][unit_amount]'], String(tier.cents), tier.id);
    assert.equal(once.form['line_items[0][price]'], undefined, 'no Stripe price object is needed');
    assert.match(once.form['line_items[0][price_data][product_data][name]'], /sponsorship/);
    assert.equal(once.form.mode, 'payment');
    assert.deepEqual([once.form['metadata[type]'], once.form['metadata[tier]'], once.form['metadata[listed]'], once.form['metadata[user_id]']], [SPONSOR_KIND, tier.id, '1', 'user-1']);
    assert.match(once.form.success_url, /\/sponsor\?sponsored=true$/);
    assert.match(once.form.cancel_url, /\/sponsor\?sponsored=false$/);
    const monthly = await checkout(env, { tier: tier.id, mode: 'recurring' });
    assert.equal(monthly.form.mode, 'subscription');
    assert.equal(monthly.form['line_items[0][price_data][recurring][interval]'], 'month');
    assert.deepEqual([monthly.form['subscription_data[metadata][type]'], monthly.form['subscription_data[metadata][user_id]']], [SPONSOR_KIND, 'user-1']);
    assert.equal(monthly.form['metadata[listed]'], '0', 'listing is asked for, never assumed');
  }
});

test('a signed-out sponsor can pay and cannot be listed', async () => {
  const r = await checkout(world(), { tier: 'supporter', listed: true }, { signedIn: false });
  assert.equal(r.status, 200);
  assert.equal(r.form['metadata[listed]'], '0');
  assert.equal(r.form['metadata[user_id]'], undefined);
});

test('a custom amount is $5 to $100,000, and anything else is refused before Stripe is called', async () => {
  const env = world();
  assert.equal((await checkout(env, { custom_amount: 75 })).form['line_items[0][price_data][unit_amount]'], '7500');
  assert.equal((await checkout(env, { custom_amount: 5 })).status, 200);
  assert.equal((await checkout(env, { custom_amount: 100_000 })).status, 200);
  for (const body of [{ custom_amount: 4.99 }, { custom_amount: 100_000.01 }, { custom_amount: '75' }, { custom_amount: null }, { tier: 'nope' }, { tier: 'constructor' }, { tier: 7 }, {}]) {
    const r = await checkout(env, body);
    assert.equal(r.status, 400, JSON.stringify(body));
    assert.equal(r.form, null, 'nothing was sent to Stripe');
  }
  for (const raw of ['{not json', 'null', '[]', '7']) {
    const r = await checkout(env, null, { raw });
    assert.equal(r.status, 400, raw);
    assert.equal(r.form, null, raw);
  }
  assert.deepEqual(readSponsorRequest({ tier: 'builder', custom_amount: 9 }), { cents: 5000, tier: 'builder', monthly: false, listed: false }, 'a tier wins over a custom amount');
});

// ── Settling ─────────────────────────────────────────────────────────────────

test('a paid sponsorship goes whole to Eustress\'s side after the fee, once, and the Treasury does not move', async () => {
  const env = world();
  await addUser(env, 'user-1', 'ada');
  const before = await state(env);
  assert.equal((await deliver(env, paidSession('cs_a', 5000))).status, 200);
  const after = await state(env);
  assert.equal(after.cents, before.cents, 'the contributors\' treasury is untouched');
  assert.equal(after.platform_cents - before.platform_cents, 4825);
  assert.equal(after.fees_cents - before.fees_cents, 175);
  // A redelivery adds nothing.
  assert.equal((await deliver(env, paidSession('cs_a', 5000))).status, 200);
  assert.equal((await state(env)).platform_cents, after.platform_cents);
  assert.equal(JSON.parse(await env.PAYOUTS.get('sponsor:user-1')).total_cents, 5000, 'the sponsor record counted it once');
  assert.equal(JSON.parse(await env.PAYOUTS.get('deposit:cs_a')).type, SPONSOR_KIND);
});

test('an unpaid session moves nothing, and a paid one shows on the Economics receipts under its own kind', async () => {
  const env = world();
  const unpaid = paidSession('cs_wait', 5000);
  unpaid.data.object.payment_status = 'unpaid';
  await deliver(env, unpaid);
  assert.equal((await state(env)).platform_cents, 0);
  await deliver(env, paidSession('cs_b', 100_000, { tier: 'founding' }));
  const month = new Date().toISOString().slice(0, 7);
  const totals = await treasuryCall(env, 'period_totals', { months: [month] });
  const line = receiptsOf(totals.months).by_kind[SPONSOR_KIND];
  assert.deepEqual([line.name, line.count, line.gross_usd, line.fees_usd, line.treasury_usd, line.eustress_usd], ['Engine sponsorships', 1, 1000, 29.3, 0, 970.7]);
});

test('a monthly renewal is deposited by its invoice, and the first invoice is left to the checkout session', async () => {
  const env = world();
  await addUser(env, 'user-1', 'ada');
  await deliver(env, paidSession('cs_m', 1000, { mode: 'subscription', tier: 'supporter', listed: '1' }));
  const invoice = (id, reason) => ({
    id: `evt_${id}`, type: 'invoice.paid',
    data: { object: { id, billing_reason: reason, amount_paid: 1000, payment_intent: `pi_${id}`, subscription_details: { metadata: { type: SPONSOR_KIND, tier: 'supporter', user_id: 'user-1' } } } },
  });
  await deliver(env, invoice('in_first', 'subscription_create'));
  assert.equal((await state(env)).platform_cents, 941, 'the first invoice is not counted twice');
  await deliver(env, invoice('in_2', 'subscription_cycle'));
  await deliver(env, invoice('in_2', 'subscription_cycle'));
  assert.equal((await state(env)).platform_cents, 941 * 2);
  const rec = JSON.parse(await env.PAYOUTS.get('sponsor:user-1'));
  assert.deepEqual([rec.total_cents, rec.payments, rec.monthly, rec.listed], [2000, 2, true, true], 'a renewal keeps the listing choice');
});

test('a refund takes the sponsorship back from Eustress\'s side, not from the Treasury', async () => {
  const env = world();
  await deliver(env, paidSession('cs_r', 5000, { pi: 'pi_r' }));
  const before = await state(env);
  const res = await deliver(env, { id: 'evt_refund', type: 'charge.refunded', data: { object: { id: 'ch_r', payment_intent: 'pi_r', amount_refunded: 5000 } } });
  assert.equal(res.status, 200);
  const after = await state(env);
  assert.equal(after.cents, before.cents);
  assert.ok(after.platform_cents < before.platform_cents, `${before.platform_cents} -> ${after.platform_cents}`);
});

// ── The sponsor list ─────────────────────────────────────────────────────────

test('the list names only accounts that asked to be listed, by username, with no amounts', async () => {
  const env = world();
  await addUser(env, 'user-1', 'ada');
  await addUser(env, 'user-2', 'quiet');
  await addUser(env, 'user-3', 'gone', { banned: true });
  await deliver(env, paidSession('cs_1', 25_000, { user: 'user-1', listed: '1' }));
  await deliver(env, paidSession('cs_2', 100_000, { user: 'user-2', listed: '0' }));
  await deliver(env, paidSession('cs_3', 1000, { user: 'user-3', listed: '1' }));
  await deliver(env, paidSession('cs_4', 1000, { user: 'user-missing', listed: '1' }));
  await deliver(env, paidSession('cs_5', 1000, { user: null, listed: '1' }));
  const body = await sponsors(env);
  assert.deepEqual(body.sponsors, [{ username: 'ada', since: new Date().toISOString().slice(0, 7), monthly: false }]);
  assert.deepEqual(body.tiers.map((t) => [t.id, t.usd]), [['supporter', 10], ['builder', 50], ['champion', 250], ['founding', 1000]]);
  assert.ok(!JSON.stringify(body.sponsors).includes('250'), 'no amount is shown');
});

test('a sponsor takes their name off the list and puts it back; an account that never sponsored has nothing to list', async () => {
  const env = world();
  await addUser(env, 'user-1', 'ada');
  await deliver(env, paidSession('cs_1', 5000));
  const put = (body, token = mintToken('user-1')) => worker.fetch(new Request(`${base}/api/sponsor/listing`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) }, body: JSON.stringify(body),
  }), env, ctx);
  assert.equal((await put({ listed: false })).status, 200);
  assert.deepEqual((await sponsors(env)).sponsors, []);
  assert.equal((await put({ listed: true })).status, 200);
  assert.equal((await sponsors(env)).sponsors.length, 1);
  assert.equal((await put({ listed: 'yes' })).status, 400);
  assert.equal((await put({ listed: true }, null)).status, 401);
  assert.equal((await put({ listed: true }, mintToken('user-9'))).status, 404);
});
