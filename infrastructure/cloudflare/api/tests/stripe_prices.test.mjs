import test from 'node:test';
import assert from 'node:assert/strict';
import { checkPrices, EXPECTED } from '../scripts/check-stripe-prices.mjs';

/// What `stripe prices list` returns when everything is set up right.
const good = () => EXPECTED.map((w, i) => ({
  id: `price_${i}`, object: 'price', active: true, lookup_key: w.lookup_key, unit_amount: w.unit_amount, currency: w.currency,
  recurring: w.recurring ? { interval: w.recurring, interval_count: 1, usage_type: 'licensed' } : null,
  metadata: { ...w.metadata },
}));
const by = (prices, key) => prices.find((p) => p.lookup_key === key);

test('the table has 8 subscription prices and 6 Delta packs, each with a distinct key', () => {
  assert.equal(EXPECTED.length, 14);
  assert.equal(new Set(EXPECTED.map((w) => w.lookup_key)).size, 14);
  assert.equal(EXPECTED.filter((w) => w.recurring).length, 8);
  assert.deepEqual(EXPECTED.filter((w) => !w.recurring).map((w) => w.metadata.delta), ['1000', '2600', '5400', '11200', '23000', '60000']);
});

test('prices set up as documented pass', () => {
  assert.deepEqual(checkPrices(good()), []);
});

test('the checker says what is wrong, one line per problem', () => {
  const missing = good().filter((p) => p.lookup_key !== 'eustress_bundle_yearly');
  assert.deepEqual(checkPrices(missing), ['eustress_bundle_yearly: missing']);

  const wrongAmount = good(); by(wrongAmount, 'eustress_creator_monthly').unit_amount = 1099;
  assert.match(checkPrices(wrongAmount)[0], /eustress_creator_monthly: unit_amount is 1099, want 999/);

  const wrongInterval = good(); by(wrongInterval, 'eustress_player_plus_yearly').recurring.interval = 'month';
  assert.match(checkPrices(wrongInterval)[0], /recurring interval is "month", want "year"/);

  const everyThreeMonths = good(); by(everyThreeMonths, 'eustress_player_plus_monthly').recurring.interval_count = 3;
  assert.match(checkPrices(everyThreeMonths)[0], /interval_count is 3/);

  const oneTimeAsSubscription = good(); by(oneTimeAsSubscription, 'eustress_delta_session').recurring = { interval: 'month', interval_count: 1 };
  assert.match(checkPrices(oneTimeAsSubscription)[0], /eustress_delta_session: recurring interval is "month", want null/);

  const wrongGrant = good(); by(wrongGrant, 'eustress_bundle_monthly').metadata.delta_per_invoice = '90';
  assert.match(checkPrices(wrongGrant)[0], /metadata\.delta_per_invoice is "90", want "900"/);

  const noType = good(); delete by(noType, 'eustress_delta_project').metadata.type;
  assert.match(checkPrices(noType)[0], /metadata\.type is undefined, want "delta_purchase"/);

  const euros = good(); by(euros, 'eustress_delta_session').currency = 'eur';
  assert.match(checkPrices(euros)[0], /currency is "eur", want "usd"/);
});

test('an archived price does not count, and two active prices on one key do', () => {
  const archived = good(); by(archived, 'eustress_delta_session').active = false;
  assert.deepEqual(checkPrices(archived), ['eustress_delta_session: missing']);

  const twice = good(); twice.push({ ...by(twice, 'eustress_delta_session'), id: 'price_dup' });
  assert.deepEqual(checkPrices(twice), ['eustress_delta_session: 2 active prices share the key']);
});

test('a key outside the table that starts with eustress_ is reported, and unrelated prices are ignored', () => {
  const stray = good(); stray.push({ id: 'price_old', active: true, lookup_key: 'eustress_delta_250', unit_amount: 250, currency: 'usd', metadata: {} });
  assert.deepEqual(checkPrices(stray), ['eustress_delta_250: not in the table (an old or mistyped key)']);

  const other = good(); other.push({ id: 'price_x', active: true, lookup_key: 'something_else', unit_amount: 1, currency: 'usd', metadata: {} }, { id: 'price_y', active: true, unit_amount: 1, currency: 'usd', metadata: {} });
  assert.deepEqual(checkPrices(other), []);
});

test('anything that is not a list of prices is all missing, not a crash', () => {
  for (const bad of [undefined, null, {}, 'x', 42, [null]]) assert.equal(checkPrices(bad).length, EXPECTED.length);
});
