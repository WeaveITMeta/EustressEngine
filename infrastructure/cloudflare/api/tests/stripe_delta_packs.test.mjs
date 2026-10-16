// The script that creates the Delta packs in Stripe (scripts/stripe-delta-packs.mjs): the packs it makes are the ones the checker accepts,
// live mode asks first and every call carries --live, a failure stops the run and says where, and a dry run makes no call.

import test from 'node:test';
import assert from 'node:assert/strict';
import { PACKS, PRODUCT_NAME, asCommand, createPacks, checkAndReport, listArgs, packProblems, priceArgs, productArgs } from '../scripts/stripe-delta-packs.mjs';
import { EXPECTED } from '../scripts/check-stripe-prices.mjs';

/// A Stripe CLI that records its calls and answers like the real one: ids for creates, the prices made so far for a list.
function fakeStripe({ failOn } = {}) {
  const calls = [];
  const prices = [];
  const run = (args) => {
    calls.push(args);
    if (args.includes('products')) return { id: 'prod_new' };
    if (args.includes('create')) {
      const field = (name) => args.find((a) => a.startsWith(`${name}=`))?.slice(name.length + 1);
      const key = field('lookup_key');
      if (failOn && key === failOn) throw new Error('A price with that lookup key already exists.');
      const meta = { type: field('metadata[type]'), delta: field('metadata[delta]') };
      prices.push({ id: `price_${prices.length + 1}`, lookup_key: key, unit_amount: Number(field('unit_amount')), currency: field('currency'), recurring: null, active: true, metadata: meta });
      return { id: `price_${prices.length}` };
    }
    if (args.includes('list')) return { data: prices };
    throw new Error(`unexpected call ${args.join(' ')}`);
  };
  return { run, calls, prices };
}

const lines = () => {
  const out = [];
  return { out, log: (l) => out.push(l) };
};

test('the six packs are the ones the docs list, in this order, at these prices', () => {
  assert.deepEqual(PACKS.map((p) => [p.key, p.cents, p.delta]), [
    ['eustress_delta_session', 1000, '1000'],
    ['eustress_delta_project', 2500, '2600'],
    ['eustress_delta_week', 5000, '5400'],
    ['eustress_delta_sprint', 10000, '11200'],
    ['eustress_delta_month', 20000, '23000'],
    ['eustress_delta_studio', 50000, '60000'],
  ]);
  assert.equal(PRODUCT_NAME, 'Eustress Delta');
});

test('test mode makes one product and six prices, names the profile on every call, and never says --live', async () => {
  const { run, calls } = fakeStripe();
  const { out, log } = lines();
  assert.equal(await createPacks({ live: false, run, ask: async () => assert.fail('test mode asks nothing'), log }), true);
  assert.equal(calls.filter((a) => a.includes('products')).length, 1);
  assert.equal(calls.filter((a) => a.includes('prices') && a.includes('create')).length, 6);
  for (const args of calls) {
    assert.deepEqual(args.slice(0, 2), ['--project-name', 'default']);
    assert.ok(!args.includes('--live'), args.join(' '));
  }
  assert.ok(out[0].startsWith('product prod_new'));
  assert.ok(out.at(-1).includes('right in test mode'), out.at(-1));
});

test('--live comes last, after the subcommand, because the Stripe CLI rejects it anywhere before', () => {
  const calls = [productArgs(true), priceArgs(true, 'prod_x', PACKS[0]), listArgs(true)];
  for (const args of calls) {
    assert.equal(args.at(-1), '--live', args.join(' '));
    assert.deepEqual(args.slice(0, 2), ['--project-name', 'default']);
    assert.equal(args.filter((a) => a === '--live').length, 1);
  }
  for (const args of [productArgs(false), priceArgs(false, 'prod_x', PACKS[0]), listArgs(false)]) assert.ok(!args.includes('--live'));
});

test('every price carries its product, its amount in cents, its lookup key and what it grants', () => {
  for (const pack of PACKS) {
    const args = priceArgs(false, 'prod_x', pack);
    for (const want of ['product=prod_x', 'currency=usd', `unit_amount=${pack.cents}`, `lookup_key=${pack.key}`, 'metadata[type]=delta_purchase', `metadata[delta]=${pack.delta}`]) {
      assert.ok(args.includes(want), `${pack.key}: ${want}`);
    }
  }
  assert.ok(productArgs(false).includes('name=Eustress Delta'));
});

test('live mode asks first: only the word live goes on, anything else creates nothing', async () => {
  for (const answer of ['', 'no', 'yes', 'LIVE', 'live now']) {
    const { run, calls } = fakeStripe();
    const { out, log } = lines();
    assert.equal(await createPacks({ live: true, run, ask: async () => answer, log }), false, JSON.stringify(answer));
    assert.equal(calls.length, 0, `${JSON.stringify(answer)} made a call`);
    assert.ok(out.includes('Nothing was created.'));
  }
  const { run, calls } = fakeStripe();
  assert.equal(await createPacks({ live: true, run, ask: async () => ' live \n', log: () => {} }), true);
  assert.equal(calls.length, 8, 'a product, six prices and a list');
  for (const args of calls) assert.ok(args.includes('--live'), `every call is live: ${args.join(' ')}`);
});

test('the first failure stops the run, says which pack, and names the product that was left', async () => {
  const { run, calls } = fakeStripe({ failOn: 'eustress_delta_week' });
  const { out, log } = lines();
  await assert.rejects(createPacks({ live: false, run, ask: async () => '', log }), (e) => {
    assert.match(e.message, /Stopped at eustress_delta_week: A price with that lookup key already exists\./);
    assert.match(e.message, /prod_new/);
    return true;
  });
  assert.equal(calls.filter((a) => a.includes('prices') && a.includes('create')).length, 3, 'the two before it and the one that failed; nothing after');
  assert.deepEqual(out.map((l) => l.split(' ')[0]), ['product', 'eustress_delta_session', 'eustress_delta_project']);
});

test('a dry run prints the calls and makes none, live or not, and asks nothing', async () => {
  for (const live of [false, true]) {
    const { run, calls } = fakeStripe();
    const { out, log } = lines();
    assert.equal(await createPacks({ live, run, ask: async () => assert.fail('a dry run asks nothing'), log, dryRun: true }), true);
    assert.equal(calls.length, 0);
    assert.equal(out.length, 7);
    assert.ok(out.every((l) => l.startsWith('stripe --project-name default')), out[0]);
    assert.equal(out.every((l) => l.includes('--live')), live);
  }
  assert.equal(asCommand(['a', 'name=Eustress Delta', 'metadata[type]=x', 'b']), 'stripe a "name=Eustress Delta" "metadata[type]=x" b');
});

test('the check passes the six packs when they are right and says what is wrong when they are not', () => {
  const perfect = EXPECTED.map((w) => ({ id: `p_${w.lookup_key}`, active: true, lookup_key: w.lookup_key, unit_amount: w.unit_amount, currency: 'usd', recurring: w.recurring ? { interval: w.recurring } : null, metadata: w.metadata }));
  assert.deepEqual(packProblems(perfect), []);
  // Plan prices are not this script's business: with only the packs listed, the plans are missing and that is not reported.
  const packsOnly = perfect.filter((p) => p.lookup_key.startsWith('eustress_delta_'));
  assert.deepEqual(packProblems(packsOnly), []);
  const wrong = packsOnly.map((p) => (p.lookup_key === 'eustress_delta_week' ? { ...p, unit_amount: 4000 } : p)).filter((p) => p.lookup_key !== 'eustress_delta_studio');
  assert.deepEqual(packProblems(wrong), ['eustress_delta_week: unit_amount is 4000, want 5000', 'eustress_delta_studio: missing']);
  const { out, log } = lines();
  assert.equal(checkAndReport({ live: true, run: () => ({ data: wrong }), log }), false);
  assert.deepEqual(out, ['eustress_delta_week: unit_amount is 4000, want 5000', 'eustress_delta_studio: missing']);
  assert.deepEqual(listArgs(true), ['--project-name', 'default', 'prices', 'list', '--limit', '100', '--live']);
});
