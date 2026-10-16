// A test-mode Stripe key prices a Tickets pack and a treasury contribution inline, so staging can buy them end to end. The price ids and
// the product id in the Worker are live-mode objects: under a test key Stripe answers "No such price". A live key keeps using them,
// unchanged. Driven through the real Worker with Stripe's API stubbed.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, ctx, mintToken } from './helpers/load_worker.mjs';

const worker = await loadWorker();
const base = 'https://api.eustress.dev';
const TEST_KEY = 'sk_test_not_real';
const LIVE_KEY = 'sk_live_not_real';

/// POST `path` as `user-1` (or with no sign-in) with Stripe's API stubbed. Resolves to the Worker's response, its JSON, and the form
/// the Worker sent Stripe (null when it sent none).
async function buy(path, body, key, { signedIn = true, raw } = {}) {
  const env = { ...fakeEnv(), STRIPE_SECRET_KEY: key };
  const sent = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    assert.equal(String(url), 'https://api.stripe.com/v1/checkout/sessions', 'the only call out is to Stripe Checkout');
    sent.push(Object.fromEntries(new URLSearchParams(init.body)));
    return new Response(JSON.stringify({ id: 'cs_test_1', url: 'https://checkout.stripe.com/c/pay/cs_test_1' }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    const headers = { 'Content-Type': 'application/json', ...(signedIn ? { Authorization: `Bearer ${mintToken('user-1')}` } : {}) };
    const res = await worker.fetch(new Request(`${base}${path}`, { method: 'POST', headers, body: raw ?? JSON.stringify(body) }), env, ctx);
    return { status: res.status, json: await res.json(), form: sent[0] ?? null };
  } finally {
    globalThis.fetch = real;
  }
}

// ── Tickets ──────────────────────────────────────────────────────────────────

test('a test key prices a Tickets pack inline, at the Worker\'s own amount, with the metadata the webhook credits by', async () => {
  const { status, json, form } = await buy('/api/tickets/checkout', { package: 'standard' }, TEST_KEY);
  assert.equal(status, 200);
  assert.equal(json.url, 'https://checkout.stripe.com/c/pay/cs_test_1');
  assert.equal(form['line_items[0][price]'], undefined, 'no live price id under a test key');
  assert.equal(form['line_items[0][price_data][currency]'], 'usd');
  assert.equal(form['line_items[0][price_data][unit_amount]'], '999');
  assert.match(form['line_items[0][price_data][product_data][name]'], /Standard/);
  assert.equal(form['line_items[0][quantity]'], '1');
  assert.equal(form.mode, 'payment');
  assert.deepEqual([form['metadata[type]'], form['metadata[package]'], form['metadata[tickets]'], form['metadata[user_id]'], form.client_reference_id], ['ticket_purchase', 'standard', '880', 'user-1', 'user-1']);
  assert.match(form.success_url, /\/tickets\?purchased=true$/);
  assert.match(form.cancel_url, /\/tickets\?purchased=false$/);
});

test('every Tickets pack is the amount the packages list says, under a test key', async () => {
  const list = await (await worker.fetch(new Request(`${base}/api/tickets/packages`), fakeEnv(), ctx)).json();
  assert.equal(list.packages.length, 5);
  for (const pack of list.packages) {
    const { form } = await buy('/api/tickets/checkout', { package: pack.id }, TEST_KEY);
    assert.equal(form['line_items[0][price_data][unit_amount]'], String(Math.round(pack.usd * 100)), pack.id);
    assert.equal(form['metadata[tickets]'], String(pack.total), pack.id);
  }
});

test('a live key still buys a Tickets pack by its price id', async () => {
  const { status, form } = await buy('/api/tickets/checkout', { package: 'standard' }, LIVE_KEY);
  assert.equal(status, 200);
  assert.equal(form['line_items[0][price]'], 'price_1THyBTRgsC7hEeKMqirqajPl');
  assert.equal(form['line_items[0][price_data][unit_amount]'], undefined, 'no inline price under a live key');
  assert.equal(form['metadata[package]'], 'standard');
});

test('a Tickets purchase needs a sign-in and a real pack, and a body that is not JSON is a refusal, not a crash', async () => {
  const signedOut = await buy('/api/tickets/checkout', { package: 'standard' }, TEST_KEY, { signedIn: false });
  assert.equal(signedOut.status, 401);
  assert.equal(signedOut.form, null, 'nothing was sent to Stripe');
  for (const body of [{ package: 'nope' }, { package: 'constructor' }, { package: '__proto__' }, { package: 7 }, {}]) {
    const r = await buy('/api/tickets/checkout', body, TEST_KEY);
    assert.equal(r.status, 400, JSON.stringify(body));
    assert.equal(r.json.error, 'Invalid package');
    assert.equal(r.form, null, 'nothing was sent to Stripe');
  }
  for (const raw of ['{not json', 'null', '[]', '7', '"standard"']) {
    const garbled = await buy('/api/tickets/checkout', null, TEST_KEY, { raw });
    assert.equal(garbled.status, 400, raw);
    assert.equal(garbled.form, null, raw);
  }
});

// ── The treasury ─────────────────────────────────────────────────────────────

test('a test key prices every treasury tier inline at the live price\'s amount, monthly or not', async () => {
  const cents = { seed: '2500', growth: '10000', sustainer: '50000', patron: '100000' };
  for (const [tier, amount] of Object.entries(cents)) {
    const once = await buy('/api/stripe/checkout', { tier, mode: 'one_time' }, TEST_KEY);
    assert.equal(once.status, 200, tier);
    assert.equal(once.form['line_items[0][price]'], undefined, `${tier}: no live price id`);
    assert.equal(once.form['line_items[0][price_data][unit_amount]'], amount, tier);
    assert.equal(once.form['line_items[0][price_data][product]'], undefined, `${tier}: no live product id`);
    assert.ok(once.form['line_items[0][price_data][product_data][name]'], tier);
    assert.equal(once.form.mode, 'payment');
    assert.equal(once.form['line_items[0][price_data][recurring][interval]'], undefined);
    const monthly = await buy('/api/stripe/checkout', { tier, mode: 'recurring' }, TEST_KEY);
    assert.equal(monthly.form['line_items[0][price_data][unit_amount]'], amount, `${tier} monthly`);
    assert.equal(monthly.form['line_items[0][price_data][recurring][interval]'], 'month');
    assert.equal(monthly.form.mode, 'subscription');
    assert.equal(monthly.form['subscription_data[metadata][type]'], 'treasury_fund');
  }
});

test('a custom treasury amount under a test key is inline with no live product, and a live key is unchanged', async () => {
  const test = await buy('/api/stripe/checkout', { custom_amount: 75, mode: 'one_time' }, TEST_KEY);
  assert.equal(test.form['line_items[0][price_data][unit_amount]'], '7500');
  assert.equal(test.form['line_items[0][price_data][product]'], undefined);
  assert.ok(test.form['line_items[0][price_data][product_data][name]']);
  const live = await buy('/api/stripe/checkout', { custom_amount: 75, mode: 'one_time' }, LIVE_KEY);
  assert.equal(live.form['line_items[0][price_data][unit_amount]'], '7500');
  assert.equal(live.form['line_items[0][price_data][product]'], 'prod_UGVCI0rliegrSC');
  assert.equal(live.form['line_items[0][price_data][product_data][name]'], undefined);
  const tier = await buy('/api/stripe/checkout', { tier: 'growth', mode: 'recurring' }, LIVE_KEY);
  assert.equal(tier.form['line_items[0][price]'], 'price_1THyCARgsC7hEeKMvMQWDog5');
  assert.equal(tier.form['line_items[0][price_data][unit_amount]'], undefined);
});

test('the treasury still refuses an amount outside $5 to $100,000, and records the buyer when signed in', async () => {
  for (const custom_amount of [4.99, 100_000.01, 'x', null]) {
    const r = await buy('/api/stripe/checkout', { custom_amount }, TEST_KEY);
    assert.equal(r.status, 400, String(custom_amount));
    assert.equal(r.form, null);
  }
  const signedIn = await buy('/api/stripe/checkout', { tier: 'seed', mode: 'one_time' }, TEST_KEY);
  assert.equal(signedIn.form['metadata[user_id]'], 'user-1');
  assert.equal(signedIn.form['metadata[type]'], 'treasury_fund');
  assert.match(signedIn.form.success_url, /\/bliss\?funded=true$/);
  assert.match(signedIn.form.cancel_url, /\/bliss\?funded=false$/);
  const anonymous = await buy('/api/stripe/checkout', { tier: 'seed', mode: 'one_time' }, TEST_KEY, { signedIn: false });
  assert.equal(anonymous.status, 200, 'the treasury takes a contribution with no sign-in');
  assert.equal(anonymous.form['metadata[user_id]'], undefined);
});
