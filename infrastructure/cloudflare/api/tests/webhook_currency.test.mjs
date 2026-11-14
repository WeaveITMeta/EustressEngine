// The Stripe webhook's money in, for the paths the Treasury owns (Ticket packs, Delta packs, plans, treasury contributions): the
// deposit is in US cents whatever currency the buyer was shown, a refund or dispute stated in that currency converts back in
// proportion, an invoice's tax is read from `total_taxes`, and a subscription's PaymentIntent is found on its invoice so a refund
// of it reverses the deposit. Driven through the real Worker with Stripe's API stubbed and the real Treasury object.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { Treasury, treasuryCall, depositUsdCents } from '../src/treasury.mjs';

const worker = await loadWorker();
const base = 'https://api.eustress.dev';
const SECRET = 'whsec_test_not_real';

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), STRIPE_SECRET_KEY: 'sk_test_not_real', STRIPE_WEBHOOK_SECRET: SECRET };
  env.TREASURY = fakeDoNamespace(Treasury, env);
  return env;
}

/// Deliver a signed event. `invoices` answers GET /v1/invoices/{id}, the only Stripe call these paths make.
async function deliver(env, event, { invoices = {} } = {}) {
  const body = JSON.stringify({ livemode: false, ...event });
  const t = Math.floor(Date.now() / 1000);
  const v1 = crypto.createHmac('sha256', SECRET).update(`${t}.${body}`).digest('hex');
  const real = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url) => {
    const u = new URL(String(url));
    asked.push(u.pathname + u.search);
    const m = u.pathname.match(/^\/v1\/invoices\/([^/]+)$/);
    if (u.hostname === 'api.stripe.com' && m && invoices[m[1]]) return new Response(JSON.stringify(invoices[m[1]]), { status: 200 });
    return new Response(JSON.stringify({ error: { message: `unexpected ${u.pathname}` } }), { status: 404 });
  };
  try {
    const res = await worker.fetch(new Request(`${base}/api/stripe/webhook`, { method: 'POST', headers: { 'Stripe-Signature': `t=${t},v1=${v1}` }, body }), env, ctx);
    return { status: res.status, asked };
  } finally {
    globalThis.fetch = real;
  }
}

const state = async (env) => (await treasuryCall(env, 'snapshot')).state;
const booked = (s) => s.cents + s.platform_cents + s.fees_cents;

/// A Ticket pack bought by a signed-out buyer (no wallet credit), shown in yen.
const yenTickets = (id, { tax = 0 } = {}) => ({
  id: `evt_${id}`, type: 'checkout.session.completed',
  data: { object: {
    id, object: 'checkout.session', payment_status: 'paid', mode: 'payment', payment_intent: `pi_${id}`,
    currency: 'jpy', amount_total: 1500 + tax, total_details: { amount_tax: tax },
    currency_conversion: { source_currency: 'usd', amount_total: 999, fx_rate: '150.15' },
    metadata: { type: 'ticket_purchase', package: 'standard' },
  } },
});

test('a Ticket pack shown in yen is deposited as the dollars it cost, not as 1,500 dollars of cents', async () => {
  const env = world();
  const res = await deliver(env, yenTickets('cs_jpy'));
  assert.equal(res.status, 200);
  const s = await state(env);
  assert.equal(booked(s), 999, 'fee + treasury + platform equal the 9.99 USD the buyer paid');
  const dep = (await treasuryCall(env, 'snapshot')).state;
  assert.ok(dep.cents > 0);
});

test('a refund stated in yen reverses the deposit in proportion, in dollars', async () => {
  const env = world();
  await deliver(env, yenTickets('cs_jpy_r'));
  const before = await state(env);
  const half = await deliver(env, { id: 'evt_rf1', type: 'charge.refunded', data: { object: { id: 'ch_1', payment_intent: 'pi_cs_jpy_r', currency: 'jpy', amount: 1500, amount_refunded: 750 } } });
  assert.equal(half.status, 200);
  const mid = await state(env);
  assert.ok(mid.cents < before.cents && mid.cents > 0, `half a refund leaves part of the treasury share: ${before.cents} -> ${mid.cents}`);
  // The same sale in dollars, refunded 4.99 (half of 9.99, rounded down): the yen refund must reverse exactly as much.
  const control = world();
  await deliver(control, { id: 'evt_ctl', type: 'checkout.session.completed', data: { object: {
    id: 'cs_ctl', payment_status: 'paid', mode: 'payment', payment_intent: 'pi_ctl', currency: 'usd', amount_total: 999,
    metadata: { type: 'ticket_purchase', package: 'standard' },
  } } });
  await deliver(control, { id: 'evt_ctl_r', type: 'charge.refunded', data: { object: { id: 'ch_c', payment_intent: 'pi_ctl', currency: 'usd', amount: 999, amount_refunded: 499 } } });
  assert.deepEqual(
    [mid.cents, mid.platform_cents],
    [(await state(control)).cents, (await state(control)).platform_cents],
    'half a yen refund reverses what a 4.99 dollar refund reverses',
  );
  await deliver(env, { id: 'evt_rf2', type: 'charge.refunded', data: { object: { id: 'ch_1', payment_intent: 'pi_cs_jpy_r', currency: 'jpy', amount: 1500, amount_refunded: 1500 } } });
  const after = await state(env);
  assert.equal(after.cents, 0, 'a full refund in yen takes back the whole treasury share, no more');
  assert.equal(await env.PAYOUTS.get('unmatched-reversal:evt_rf2'), null);
});

test('a refund in a currency the deposit was never shown in is set aside, not reversed', async () => {
  const env = world();
  await deliver(env, yenTickets('cs_jpy_x'));
  const before = await state(env);
  const res = await deliver(env, { id: 'evt_rfx', type: 'charge.refunded', data: { object: { id: 'ch_x', payment_intent: 'pi_cs_jpy_x', currency: 'eur', amount: 900, amount_refunded: 900 } } });
  assert.equal(res.status, 200);
  assert.equal((await state(env)).cents, before.cents);
  assert.ok(await env.PAYOUTS.get('unmatched-reversal:evt_rfx'));
});

test('a foreign-currency sale with tax has no exact dollar figure: nothing is deposited and a person is asked', async () => {
  const env = world();
  const res = await deliver(env, yenTickets('cs_jpy_tax', { tax: 150 }));
  assert.equal(res.status, 200);
  assert.equal(booked(await state(env)), 0);
  const note = JSON.parse(await env.PAYOUTS.get('finance-review:stripe:cs_jpy_tax'));
  assert.deepEqual([note.reason, note.kind, note.currency], ['currency', 'ticket_purchase', 'jpy']);
});

test('a dollar sale is unchanged', async () => {
  const env = world();
  await deliver(env, { id: 'evt_usd', type: 'checkout.session.completed', data: { object: {
    id: 'cs_usd', payment_status: 'paid', mode: 'payment', payment_intent: 'pi_usd', currency: 'usd', amount_total: 999,
    metadata: { type: 'ticket_purchase', package: 'standard' },
  } } });
  assert.equal(booked(await state(env)), 999);
  assert.equal(await env.PAYOUTS.get('finance-review:stripe:cs_usd'), null);
});

/// A renewal of a recurring treasury contribution, in the invoice shape of Stripe API 2025-03-31 and later: no `payment_intent`
/// and no `tax` on the invoice; the tax is in `total_taxes` and the PaymentIntent in `payments`.
const renewal = (id) => ({
  id: `evt_${id}`, type: 'invoice.paid',
  data: { object: {
    id, object: 'invoice', billing_reason: 'subscription_cycle', currency: 'usd', amount_paid: 2700,
    total_taxes: [{ amount: 200, tax_behavior: 'exclusive' }],
    parent: { subscription_details: { metadata: { type: 'treasury_fund', user_id: 'user-1' } } },
  } },
});
const invoiceWithPayment = (id) => ({ id, object: 'invoice', payments: { data: [{ status: 'paid', payment: { type: 'payment_intent', payment_intent: `pi_${id}` } }] } });

test('a renewal is deposited without its tax, and a refund of it finds the deposit through the invoice', async () => {
  const env = world();
  const res = await deliver(env, renewal('in_r1'), { invoices: { in_r1: invoiceWithPayment('in_r1') } });
  assert.equal(res.status, 200);
  assert.ok(res.asked.some((p) => p.startsWith('/v1/invoices/in_r1') && p.includes('expand')), 'the PaymentIntent is read from the invoice');
  assert.equal((await state(env)).cents, 2500, 'the 2.00 tax is not treasury money');
  const refund = await deliver(env, { id: 'evt_rr1', type: 'charge.refunded', data: { object: { id: 'ch_r1', payment_intent: 'pi_in_r1', currency: 'usd', amount: 2700, amount_refunded: 2500 } } });
  assert.equal(refund.status, 200);
  assert.equal(await env.PAYOUTS.get('unmatched-reversal:evt_rr1'), null, 'the refund matched its deposit');
  assert.equal((await state(env)).cents, 0);
});

test('a renewal whose PaymentIntent cannot be found is deposited and flagged for a person', async () => {
  const env = world();
  await deliver(env, renewal('in_r2'));
  assert.equal((await state(env)).cents, 2500);
  const note = JSON.parse(await env.PAYOUTS.get('finance-review:stripe-invoice:in_r2'));
  assert.equal(note.reason, 'no_payment_intent');
});

test('a renewal invoiced in another currency is not booked as dollars', async () => {
  const env = world();
  const ev = renewal('in_eur');
  ev.data.object.currency = 'eur';
  await deliver(env, ev, { invoices: { in_eur: invoiceWithPayment('in_eur') } });
  assert.equal((await state(env)).cents, 0);
  assert.equal(JSON.parse(await env.PAYOUTS.get('finance-review:stripe-invoice:in_eur')).currency, 'eur');
});

test('the conversion back to dollars is proportional, capped at the deposit, and refuses a currency it was not shown in', () => {
  const dep = { gross_cents: 999, presented_currency: 'jpy', presented_cents: 1500 };
  assert.equal(depositUsdCents(dep, 1500, 'jpy'), 999);
  assert.equal(depositUsdCents(dep, 750, 'JPY'), 499);
  assert.equal(depositUsdCents(dep, 9999, 'jpy'), 999);
  assert.equal(depositUsdCents(dep, 500, 'usd'), 500);
  assert.equal(depositUsdCents(dep, 500, 'eur'), null);
  assert.equal(depositUsdCents({ gross_cents: 999 }, 500, 'jpy'), null);
});
