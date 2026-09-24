import test from 'node:test';
import assert from 'node:assert/strict';
import {
  BLISS_UNIT, FULL_DAY_SCORE, FOLD_HORIZON_MS, VALUE_SCORE_PER_TICKET,
  VALUE_CAP_TICKETS_PER_BUYER, VALUE_CAP_SCORE_PER_CREATOR,
  toMinor, fromMinor, formatBliss, blissEmissionRate, yearsSince,
  ledgerAppend, ledgerDeriveMinor, ledgerBalanceMinor, ledgerReconcile, ledgerMigrateUser, ledgerSpend,
  readEntry, creditValueScore, cappedValueScore, valueScoreForDay, projectEmissionMinor, readSupplyMinor,
} from '../src/bliss.mjs';

// KV as the Worker sees it: listings sorted, with each key's metadata.
function kv() {
  const store = new Map();
  return {
    store,
    ops: 0,
    async get(key) {
      this.ops += 1;
      return store.has(key) ? store.get(key).value : null;
    },
    async put(key, value, opts = {}) {
      this.ops += 1;
      if (opts.metadata !== undefined && Buffer.byteLength(JSON.stringify(opts.metadata)) > 1024)
        throw new Error('KV metadata exceeds 1024 bytes');
      store.set(key, { value: String(value), metadata: opts.metadata });
    },
    async delete(key) {
      this.ops += 1;
      store.delete(key);
    },
    async list({ prefix = '', limit = 1000, cursor } = {}) {
      this.ops += 1;
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

const env = () => ({ PAYOUTS: kv(), USERS: kv(), INVENTORY: kv() });
const USER = '11111111-1111-4111-8111-111111111111';
const iso = (ms) => new Date(ms).toISOString();

test('units convert to and from integer minor units at two decimals', () => {
  assert.equal(BLISS_UNIT, 100);
  assert.equal(toMinor(12.345), 1235);
  assert.equal(fromMinor(1235), 12.35);
  assert.equal(formatBliss(205676_59), '205676.59');
  assert.equal(toMinor('garbage'), 0);
});

test('emission halves every four years and never falls below the tail', () => {
  assert.equal(blissEmissionRate(0), 0.05);
  assert.equal(blissEmissionRate(3), 0.05);
  assert.equal(blissEmissionRate(4), 0.025);
  assert.equal(blissEmissionRate(40), 0.005);
  assert.equal(yearsSince(null, Date.now()), 0);
  assert.equal(yearsSince('2026-01-01', Date.parse('2027-01-02')), 1);
});

test('an appended entry carries its fields in metadata and is written once', async () => {
  const e = env();
  const first = await ledgerAppend(e, USER, { amount_minor: 500, kind: 'emission', ref: '2026-09-01', ts: '2026-09-01T00:00:00.000Z', id: 'dist' });
  const again = await ledgerAppend(e, USER, { amount_minor: 500, kind: 'emission', ref: '2026-09-01', ts: '2026-09-01T00:00:00.000Z', id: 'dist' });
  assert.equal(first, true);
  assert.equal(again, false);
  const stored = e.PAYOUTS.store.get(`entry:${USER}:2026-09-01T00:00:00.000Z:dist`);
  assert.deepEqual(stored.metadata, { a: 500, k: 'emission', r: '2026-09-01', t: '2026-09-01T00:00:00.000Z' });
  // No cache existed, so none is invented: the next read derives it.
  assert.equal(e.PAYOUTS.store.has(`bal:${USER}`), false);
  assert.equal(await ledgerBalanceMinor(e, USER), 500);
});

test('an entry written without metadata is read from its value', async () => {
  const e = env();
  e.PAYOUTS.store.set(`entry:${USER}:2026-01-01T00:00:00.000Z:dist`, {
    value: JSON.stringify({ amount_minor: 1234, kind: 'emission', ref: 'x', ts: '2026-01-01T00:00:00.000Z' }),
    metadata: undefined,
  });
  const list = await e.PAYOUTS.list({ prefix: `entry:${USER}:` });
  const entry = await readEntry(e, list.keys[0]);
  assert.equal(entry.amount_minor, 1234);
  assert.equal(entry.has_metadata, false);
});

test('a checkpoint never skips an entry dated before it but written after it', async () => {
  const e = env();
  const now = Date.parse('2026-09-20T12:00:00.000Z');
  // Old history, folded into a checkpoint.
  await ledgerAppend(e, USER, { amount_minor: 1000, kind: 'emission', ts: iso(now - 30 * 86400e3), id: 'dist' });
  // A spend early today, then the reconcile after it.
  await ledgerAppend(e, USER, { amount_minor: -100, kind: 'spend', ts: '2026-09-20T00:00:05.000Z', id: 'spend-a' });
  const derived = await ledgerDeriveMinor(e, USER, now);
  e.PAYOUTS.store.set(`cp:${USER}`, { value: JSON.stringify(derived.checkpoint) });
  // The checkpoint folds only what is older than the horizon.
  assert.ok(derived.checkpoint.through < `entry:${USER}:${iso(now - FOLD_HORIZON_MS)}`);
  // Then yesterday's emission credit lands, dated at the start of yesterday.
  await ledgerAppend(e, USER, { amount_minor: 250, kind: 'emission', ts: '2026-09-19T00:00:00.000Z', id: 'dist' });
  const after = await ledgerDeriveMinor(e, USER, now);
  assert.equal(after.balance_minor, 1000 - 100 + 250);
});

test('a checkpoint from before the fold horizon existed is not trusted', async () => {
  const e = env();
  await ledgerAppend(e, USER, { amount_minor: 700, kind: 'emission', ts: '2026-09-19T00:00:00.000Z', id: 'dist' });
  // A version-less checkpoint claiming entries it did not sum.
  e.PAYOUTS.store.set(`cp:${USER}`, { value: JSON.stringify({ balance_minor: 0, through: `entry:${USER}:2026-09-19T23:00:00.000Z:x` }) });
  const { balance_minor } = await ledgerDeriveMinor(e, USER, Date.parse('2026-09-20T12:00:00.000Z'));
  assert.equal(balance_minor, 700);
});

test('reconcile writes a versioned checkpoint and corrects the cache', async () => {
  const e = env();
  await ledgerAppend(e, USER, { amount_minor: 900, kind: 'emission', ts: '2020-01-01T00:00:00.000Z', id: 'dist' });
  e.PAYOUTS.store.set(`bal:${USER}`, { value: '5' });
  assert.equal(await ledgerReconcile(e, USER), 900);
  assert.equal(e.PAYOUTS.store.get(`bal:${USER}`).value, '900');
  const cp = JSON.parse(e.PAYOUTS.store.get(`cp:${USER}`).value);
  assert.equal(cp.v, 2);
  assert.equal(cp.balance_minor, 900);
});

test('a legacy float balance migrates once, on first read', async () => {
  const e = env();
  e.USERS.store.set(`user:${USER}`, { value: JSON.stringify({ id: USER, bliss_balance: 12.5 }) });
  assert.equal(await ledgerBalanceMinor(e, USER), 1250);
  const user = JSON.parse(e.USERS.store.get(`user:${USER}`).value);
  assert.equal(user.ledger_migrated, true);
  await ledgerMigrateUser(e, USER, user);
  e.PAYOUTS.store.delete(`bal:${USER}`);
  assert.equal(await ledgerBalanceMinor(e, USER), 1250);
});

test('a spend reference spends once, and a spend cut off after its marker completes on retry', async () => {
  const e = env();
  await ledgerAppend(e, USER, { amount_minor: 1000, kind: 'emission', ts: '2026-09-01T00:00:00.000Z', id: 'dist' });
  const first = await ledgerSpend(e, USER, { amount_minor: 300, purpose: 'Test', ref: 'order-1' });
  assert.equal(first.ok, true);
  const second = await ledgerSpend(e, USER, { amount_minor: 300, purpose: 'Test', ref: 'order-1' });
  assert.equal(second.ok, false);
  assert.equal(second.error, 'Duplicate spend reference');
  e.PAYOUTS.store.delete(`bal:${USER}`);
  assert.equal(await ledgerBalanceMinor(e, USER), 700);

  // The marker landed, the entry did not.
  e.PAYOUTS.store.set(`spendref:${USER}:order-2`, { value: `entry:${USER}:2026-09-10T10:00:00.000Z:spend-order-2` });
  const resumed = await ledgerSpend(e, USER, { amount_minor: 200, purpose: 'Test', ref: 'order-2' });
  assert.equal(resumed.ok, true);
  assert.ok(e.PAYOUTS.store.has(`entry:${USER}:2026-09-10T10:00:00.000Z:spend-order-2`));
  e.PAYOUTS.store.delete(`bal:${USER}`);
  assert.equal(await ledgerBalanceMinor(e, USER), 500);
  assert.equal(e.PAYOUTS.store.get('bliss:burned_minor').value, '500');
});

test('a spend beyond the balance is refused', async () => {
  const e = env();
  const r = await ledgerSpend(e, USER, { amount_minor: 1, purpose: 'Test' });
  assert.equal(r.ok, false);
  assert.equal(r.error, 'Insufficient balance');
});

test('a sale files its value score once, under the day of its first attempt', async () => {
  const e = env();
  const creator = 'c-1';
  const score = await creditValueScore(e, { creatorId: creator, buyerId: 'b-1', creatorTickets: 70, ref: 'pur_abc' }, new Date('2026-09-20T10:00:00Z'));
  assert.equal(score, 70 * VALUE_SCORE_PER_TICKET);
  // A retry the next day files under the first day, the same key.
  await creditValueScore(e, { creatorId: creator, buyerId: 'b-1', creatorTickets: 70, ref: 'pur_abc' }, new Date('2026-09-21T01:00:00Z'));
  const keys = [...e.INVENTORY.store.keys()].filter((k) => k.startsWith('vscore:'));
  assert.deepEqual(keys, [`vscore:2026-09-20:${creator}:pur_abc`]);
  assert.deepEqual(e.INVENTORY.store.get(keys[0]).metadata, { t: 70, b: 'b-1' });
});

test('buying from yourself, or with a bad reference, files no value score', async () => {
  const e = env();
  assert.equal(await creditValueScore(e, { creatorId: 'c-1', buyerId: 'c-1', creatorTickets: 70, ref: 'pur_self' }), 0);
  assert.equal(await creditValueScore(e, { creatorId: 'c-1', buyerId: 'b-1', creatorTickets: 70, ref: 'bad:ref' }), 0);
  assert.equal(await creditValueScore(e, { creatorId: 'c-1', buyerId: 'b-1', creatorTickets: 0, ref: 'pur_zero' }), 0);
  assert.equal(e.INVENTORY.store.size, 0);
});

test('value score caps one buyer per creator per day, and each creator per day', async () => {
  assert.equal(cappedValueScore([
    { buyer: 'a', tickets: VALUE_CAP_TICKETS_PER_BUYER + 500 },
    { buyer: 'b', tickets: 100 },
  ]), (VALUE_CAP_TICKETS_PER_BUYER + 100) * VALUE_SCORE_PER_TICKET);
  const many = Array.from({ length: 100 }, (_, i) => ({ buyer: `b${i}`, tickets: VALUE_CAP_TICKETS_PER_BUYER }));
  assert.equal(cappedValueScore(many), VALUE_CAP_SCORE_PER_CREATOR);

  const e = env();
  const day = '2026-09-20';
  const at = new Date(`${day}T12:00:00Z`);
  await creditValueScore(e, { creatorId: 'c-1', buyerId: 'whale', creatorTickets: 5000, ref: 'pur_1' }, at);
  await creditValueScore(e, { creatorId: 'c-1', buyerId: 'fan', creatorTickets: 70, ref: 'pur_2' }, at);
  assert.equal(await valueScoreForDay(e, day, 'c-1'), (VALUE_CAP_TICKETS_PER_BUYER + 70) * VALUE_SCORE_PER_TICKET);
});

test('the projection pays a quiet day by the minute and a busy day by share', () => {
  const emission = 13_698.63;
  // Alone on a quiet day: emission x score / FULL_DAY_SCORE, whatever the total.
  assert.equal(projectEmissionMinor({ emissionBls: emission, score: 60, dayTotal: 60 }), Math.floor(emission * BLISS_UNIT * 60 / FULL_DAY_SCORE));
  assert.equal(projectEmissionMinor({ emissionBls: emission, score: 60, dayTotal: 1000 }), Math.floor(emission * BLISS_UNIT * 60 / FULL_DAY_SCORE));
  // A busy day: a share of the whole ceiling.
  assert.equal(projectEmissionMinor({ emissionBls: emission, score: 600, dayTotal: 6000 }), Math.floor(emission * BLISS_UNIT * 0.1));
  assert.equal(projectEmissionMinor({ emissionBls: emission, score: 0, dayTotal: 6000 }), 0);
});

test('supply reads the legacy float key until the minor-unit key exists', async () => {
  const e = env();
  assert.equal(await readSupplyMinor(e), 100_000_000 * BLISS_UNIT);
  e.PAYOUTS.store.set('bliss:current_supply', { value: '100000123.45' });
  assert.equal(await readSupplyMinor(e), 10_000_012_345);
  e.PAYOUTS.store.set('bliss:supply_minor', { value: '10000020000' });
  assert.equal(await readSupplyMinor(e), 10_000_020_000);
});
