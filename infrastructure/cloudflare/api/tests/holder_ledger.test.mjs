// The holder ledger (src/holder_ledger.mjs): who holds how many units, every change as an entry that is never altered, and who may
// read and write it. Driven through the real Worker and the real HolderLedger object on in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { HolderLedger, holderLedgerCall, entryHash, MAX_UNITS } from '../src/holder_ledger.mjs';

const worker = await loadWorker();
const ADMIN = '56565656-5656-4656-8656-565656565656';
const ADA = '12121212-1212-4212-8212-121212121212';
const BEN = '34343434-3434-4434-8434-343434343434';
const ORIGIN = 'https://eustress.dev';

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN };
  env.HOLDER_LEDGER = fakeDoNamespace(HolderLedger, env);
  for (const [id, username] of [[ADMIN, 'admin_user'], [ADA, 'ada'], [BEN, 'ben']]) {
    env.USERS.store.set(`user:${id}`, JSON.stringify({ id, username, kyc_verified: true }));
  }
  env.USERS.store.set(`admin:${ADMIN}`, '1');
  return env;
}

const call = (env, path, { method = 'GET', token = mintToken(ADMIN), body } = {}) => worker.fetch(new Request(`https://api.eustress.dev${path}`, {
  method,
  headers: { Origin: ORIGIN, 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  body: body ? JSON.stringify(body) : undefined,
}), env, ctx);
const post = async (env, what, body) => {
  const res = await call(env, `/api/admin/holders/${what}`, { method: what === 'record' ? 'PUT' : 'POST', body });
  return { status: res.status, json: await res.json() };
};
const read = async (env, path, opts) => (await call(env, path, opts)).json();

async function twoHolders(env) {
  assert.equal((await post(env, 'record', { holder_id: ADA, legal_name: 'Ada Lovelace', country: 'US', accredited: false })).status, 200);
  assert.equal((await post(env, 'record', { holder_id: BEN, legal_name: 'Ben Franklin', country: 'US', accredited: true })).status, 200);
}

const storage = (env) => env.HOLDER_LEDGER.instances.get('units').storage;

// ── Entries and balances ─────────────────────────────────────────────────────

test('units are issued, transferred and bought back, and the balances follow the entries', async () => {
  const env = world();
  await twoHolders(env);
  assert.equal((await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 100, price_cents: 500 })).status, 200);
  assert.equal((await post(env, 'issue', { ref: 'sub-2', to: BEN, units: 40, price_cents: 500 })).status, 200);
  assert.equal((await post(env, 'transfer', { ref: 'gift-1', from: ADA, to: BEN, units: 25, memo: 'gift' })).status, 200);
  const back = await post(env, 'repurchase', { ref: 'buyback-1', from: BEN, units: 15, price_cents: 550 });
  assert.equal(back.status, 200);
  assert.equal(back.json.issued_units, 125);
  assert.equal(back.json.entry.by, ADMIN, 'the entry names the administrator who recorded it');

  const list = await read(env, '/api/admin/holders');
  assert.deepEqual(list.holders.map((h) => [h.holder_id, h.units, h.legal_name]), [[ADA, 75, 'Ada Lovelace'], [BEN, 50, 'Ben Franklin']]);
  assert.deepEqual([list.issued_units, list.entries], [125, 4]);
  assert.deepEqual(list.counts, { holders_of_record: 2, non_accredited: 1, holders_of_record_limit: 2000, non_accredited_limit: 500 });

  const verify = await read(env, '/api/admin/holders/verify');
  assert.deepEqual([verify.sound, verify.entries, verify.issued_units, verify.problems], [true, 4, 125, []]);
});

test('a request repeated under the same ref makes one entry', async () => {
  const env = world();
  await twoHolders(env);
  const first = await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 100 });
  const again = await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 100 });
  assert.equal(again.json.replay, true);
  assert.equal(again.json.entry.hash, first.json.entry.hash);
  // Even with different words, the ref decides.
  assert.equal((await post(env, 'issue', { ref: 'sub-1', to: BEN, units: 999 })).json.replay, true);
  const list = await read(env, '/api/admin/holders');
  assert.deepEqual([list.issued_units, list.entries, list.holders.length], [100, 1, 1]);
});

test('a holder cannot give or sell back more than they hold, and a holder with nothing drops off the list but stays on file', async () => {
  const env = world();
  await twoHolders(env);
  await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 10 });
  const over = await post(env, 'transfer', { ref: 't-1', from: ADA, to: BEN, units: 11 });
  assert.deepEqual([over.status, over.json.code], [409, 'insufficient_units']);
  assert.equal((await post(env, 'repurchase', { ref: 'b-1', from: ADA, units: 11 })).status, 409);
  assert.equal((await post(env, 'repurchase', { ref: 'b-2', from: BEN, units: 1 })).status, 409, 'Ben holds none');
  assert.equal((await post(env, 'repurchase', { ref: 'b-3', from: ADA, units: 10 })).status, 200);
  const list = await read(env, '/api/admin/holders');
  assert.deepEqual([list.holders.length, list.on_file, list.issued_units], [0, 2, 0]);
  assert.equal((await read(env, '/api/admin/holders/verify')).sound, true, 'a refusal leaves no trace');
});

test('units go only to a holder whose legal name is on file, and a bad request is refused before anything is written', async () => {
  const env = world();
  const unknown = await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 10 });
  assert.deepEqual([unknown.status, unknown.json.code], [409, 'holder_not_recorded']);
  await twoHolders(env);
  for (const body of [
    { ref: 'x', to: ADA, units: 0 }, { ref: 'x', to: ADA, units: 1.5 }, { ref: 'x', to: ADA, units: -1 }, { ref: 'x', to: ADA, units: '10' },
    { ref: 'x', to: ADA, units: MAX_UNITS + 1 }, { ref: 'bad ref', to: ADA, units: 1 }, { to: ADA, units: 1 }, { ref: 'x', units: 1 },
    { ref: 'x', to: ADA, units: 1, price_cents: -1 }, { ref: 'x', to: 'not a holder!', units: 1 },
  ]) {
    assert.equal((await post(env, 'issue', body)).status, 400, JSON.stringify(body));
  }
  assert.equal((await post(env, 'transfer', { ref: 'x', from: ADA, to: ADA, units: 1 })).status, 400, 'a transfer to oneself');
  for (const body of [{ holder_id: ADA, legal_name: 'A', country: 'US', accredited: false }, { holder_id: ADA, legal_name: 'Ada', country: 'usa', accredited: false }, { holder_id: ADA, legal_name: 'Ada', country: 'US', accredited: 'no' }]) {
    assert.equal((await post(env, 'record', body)).status, 400, JSON.stringify(body));
  }
  assert.equal((await read(env, '/api/admin/holders')).entries, 0);
});

test('correcting a holder on file keeps their units', async () => {
  const env = world();
  await twoHolders(env);
  await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 10 });
  const fixed = await post(env, 'record', { holder_id: ADA, legal_name: 'Ada King', country: 'GB', accredited: true });
  assert.deepEqual([fixed.json.holder.units, fixed.json.holder.legal_name, fixed.json.holder.accredited], [10, 'Ada King', true]);
  assert.equal((await read(env, '/api/admin/holders')).counts.non_accredited, 0);
});

// ── Tamper evidence ──────────────────────────────────────────────────────────

test('a changed entry, a missing entry and a changed balance are each found', async () => {
  const build = async () => {
    const env = world();
    await twoHolders(env);
    await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 100 });
    await post(env, 'issue', { ref: 'sub-2', to: BEN, units: 40 });
    await post(env, 'transfer', { ref: 't-1', from: ADA, to: BEN, units: 5 });
    return env;
  };
  const verify = async (env) => read(env, '/api/admin/holders/verify');

  let env = await build();
  const e2 = await storage(env).get('e:000000000002');
  await storage(env).put('e:000000000002', { ...e2, units: 4000 });
  let v = await verify(env);
  assert.equal(v.sound, false);
  assert.ok(v.problems.includes('entry 2 was changed'), v.problems.join('; '));

  // Rewriting the entry with a fresh hash of its own still breaks the entry after it.
  env = await build();
  const forged = { ...(await storage(env).get('e:000000000002')), units: 4000 };
  forged.hash = await entryHash(forged);
  await storage(env).put('e:000000000002', forged);
  v = await verify(env);
  assert.ok(v.problems.includes('entry 3 does not follow the entry before it'), v.problems.join('; '));

  // Pointing the next entry at the forgery changes that entry too, because an entry's hash covers the link to the one before it.
  const e3 = await storage(env).get('e:000000000003');
  await storage(env).put('e:000000000003', { ...e3, prev_hash: forged.hash });
  v = await verify(env);
  assert.equal(v.sound, false);
  assert.ok(v.problems.includes('entry 3 was changed'), v.problems.join('; '));

  env = await build();
  await storage(env).delete('e:000000000002');
  v = await verify(env);
  assert.ok(v.problems.includes('entry 2 is missing'), v.problems.join('; '));

  env = await build();
  const h = await storage(env).get(`h:${ADA}`);
  await storage(env).put(`h:${ADA}`, { ...h, units: 9999 });
  v = await verify(env);
  assert.ok(v.problems.some((p) => p.startsWith(`holder ${ADA} is recorded with 9999 units`)), v.problems.join('; '));
});

test('entries are read oldest first, a page at a time', async () => {
  const env = world();
  await twoHolders(env);
  for (let i = 1; i <= 5; i++) await post(env, 'issue', { ref: `sub-${i}`, to: ADA, units: i });
  const first = await read(env, '/api/admin/holders/entries?limit=2');
  assert.deepEqual([first.entries.map((e) => e.seq), first.next_after, first.more], [[1, 2], 2, true]);
  const rest = await read(env, '/api/admin/holders/entries?after=2&limit=10');
  assert.deepEqual([rest.entries.map((e) => e.seq), rest.more], [[3, 4, 5], false]);
});

// ── Who may read and write ───────────────────────────────────────────────────

test('only an administrator reads or writes the ledger', async () => {
  const env = world();
  await twoHolders(env);
  for (const token of [null, mintToken(ADA)]) {
    for (const path of ['/api/admin/holders', '/api/admin/holders/verify', '/api/admin/holders/entries']) {
      assert.equal((await call(env, path, { token })).status, 403, path);
    }
    const write = await call(env, '/api/admin/holders/issue', { method: 'POST', token, body: { ref: 'steal', to: ADA, units: 1_000_000 } });
    assert.ok([401, 403].includes(write.status), `a write without admin is refused: ${write.status}`);
  }
  assert.equal((await read(env, '/api/admin/holders')).entries, 0);
  assert.equal((await call(env, '/api/admin/holders/nope')).status, 404);
});

test('who recorded an entry is the administrator\'s session, never the body', async () => {
  const env = world();
  await twoHolders(env);
  const r = await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 1, by: 'someone-else' });
  assert.equal(r.json.entry.by, ADMIN);
});

test('a holder sees their own units and entries, without names or who recorded them', async () => {
  const env = world();
  await twoHolders(env);
  await post(env, 'issue', { ref: 'sub-1', to: ADA, units: 100, price_cents: 500 });
  await post(env, 'transfer', { ref: 't-1', from: ADA, to: BEN, units: 30 });
  const mine = await read(env, '/api/holdings', { token: mintToken(ADA) });
  assert.equal(mine.units, 70);
  assert.deepEqual(mine.entries.map((e) => [e.kind, e.units, e.direction]), [['issue', 100, 'in'], ['transfer', 30, 'out']]);
  const raw = JSON.stringify(mine);
  for (const hidden of ['Ben Franklin', 'Ada Lovelace', BEN, ADMIN, 'hash']) assert.ok(!raw.includes(hidden), hidden);
  assert.equal((await read(env, '/api/holdings', { token: mintToken(BEN) })).units, 30);
  assert.equal((await call(env, '/api/holdings', { token: null })).status, 401);
  assert.deepEqual(await read(env, '/api/holdings', { token: mintToken(ADMIN) }), { units: 0, since: null, entries: [] });
});

test('a deployment without the ledger bound answers 503, never a made-up balance', async () => {
  const env = world();
  delete env.HOLDER_LEDGER;
  assert.equal((await call(env, '/api/admin/holders')).status, 503);
  assert.equal((await call(env, '/api/holdings', { token: mintToken(ADA) })).status, 503);
  assert.equal((await holderLedgerCall(env, 'holders')).ok, false);
});
