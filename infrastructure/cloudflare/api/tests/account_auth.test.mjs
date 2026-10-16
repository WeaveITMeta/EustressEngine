// The account Object: who holds an account's key, how it is replaced, and what a
// replacement leaves behind (docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md).
// The class runs against in-memory storage, with a fake KV standing in for the
// mirror it writes.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {
  AccountAuth, accountCall, accountState, rotationMessage, verifyAuditChain, auditHash, SMALL_ORDER_KEYS,
  FREEZE_WITH_VALUE_MS, FREEZE_WITHOUT_VALUE_MS, PAYOUT_PIN_MS,
} from '../src/account_auth.mjs';
import { fakeKv, fakeDoNamespace } from './helpers/load_worker.mjs';

const ACCOUNT = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ORIGIN = 'https://api.eustress.dev';

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

/// An environment with the account's KV record, the object namespace and the key
/// the account starts with.
async function setup({ email = 'owner@example.com', floor } = {}) {
  const env = { USERS: fakeKv() };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  const k0 = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ id: ACCOUNT, username: 'owner', public_key: k0.hex, email }));
  await env.USERS.put(`pubkey:${k0.hex}`, ACCOUNT);
  if (floor !== undefined) await env.USERS.put(`authfloor:${ACCOUNT}`, String(floor));
  return { env, k0 };
}

const call = (env, op, args = {}, account = ACCOUNT) => accountCall(env, account, op, args);

/// A full rotation request: the challenge, then both signatures over the message.
async function rotation(env, oldKey, { newKey = keyPair(), account = ACCOUNT, origin = ORIGIN, now, holds_value, mutate } = {}) {
  const begun = await call(env, 'begin_rotation', { now }, account);
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const message = rotationMessage({ origin, accountId: account, oldKey: oldKey.hex, newKey: newKey.hex, challenge: begun.challenge });
  const request = {
    new_public_key: newKey.hex, challenge: begun.challenge, origin, now, holds_value,
    old_signature: oldKey.sign(message), new_signature: newKey.sign(message),
  };
  if (mutate) mutate(request, { message, begun, newKey, oldKey });
  return { request, newKey, begun, result: await call(env, 'rotate', request, account) };
}

// ── Seeding ──────────────────────────────────────────────────────────────────

test('the first touch seeds the object from KV, and seeding twice changes nothing', async () => {
  const { env, k0 } = await setup({ floor: 1700000000 });
  const first = await accountState(env, ACCOUNT);
  assert.equal(first.ok, true, JSON.stringify(first));
  assert.equal(first.public_key, k0.hex);
  assert.equal(first.floor, 1700000000);
  assert.equal(first.key_epoch, 0);
  assert.equal(first.audit_seq, 1, 'the seeding is the first audit record');

  // A second seed, even with other data, is ignored.
  const again = await call(env, 'seed', { public_key: keyPair().hex, floor: 5 });
  assert.equal(again.ok, true);
  assert.equal(again.created, false);
  assert.equal(again.public_key, k0.hex);
  assert.equal((await accountState(env, ACCOUNT)).audit_seq, 1);
});

test('an account that does not exist is not seeded, and a request for another account is refused', async () => {
  const { env } = await setup();
  const gone = await accountState(env, OTHER);
  assert.equal(gone.ok, false);
  assert.equal(gone.status, 404);
  await accountState(env, ACCOUNT);
  // A request routed to this account's object but naming another account.
  const stub = env.ACCOUNT_AUTH.get(env.ACCOUNT_AUTH.idFromName(ACCOUNT));
  const res = await stub.fetch('https://account.internal/bump_floor', { method: 'POST', body: JSON.stringify({ account_id: OTHER }) });
  assert.equal(res.status, 400);
  assert.equal((await res.json()).code, 'wrong_account');
});

test('an operation on an object that was never seeded says so', async () => {
  const { env } = await setup();
  const r = await call(env, 'bump_floor');
  assert.equal(r.ok, false);
  assert.equal(r.code, 'not_seeded');
});

// ── Sign-in's question ───────────────────────────────────────────────────────

test('the current key signs in, in either case; any other key does not', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  assert.equal((await call(env, 'check_login', { public_key: k0.hex })).ok, true);
  assert.equal((await call(env, 'check_login', { public_key: k0.hex.toUpperCase() })).ok, true);
  const other = await call(env, 'check_login', { public_key: keyPair().hex });
  assert.equal(other.ok, false);
  assert.equal(other.code, 'key_not_current');
  assert.equal((await call(env, 'check_login', { public_key: 'not a key' })).code, 'bad_key');
});

test('an account filed under a key that is not a key has no current key and cannot start a rotation', async () => {
  const env = { USERS: fakeKv() };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ id: ACCOUNT, username: 'old', public_key: 'a sentence that was once accepted as a key' }));
  const s = await accountState(env, ACCOUNT);
  assert.equal(s.public_key, null);
  assert.equal((await call(env, 'check_login', { public_key: 'a'.repeat(64) })).ok, false);
  assert.equal((await call(env, 'begin_rotation')).code, 'no_current_key');
});

// ── The session floor ────────────────────────────────────────────────────────

test('the floor moves one second past the later of now and itself, and is mirrored to KV', async () => {
  const { env } = await setup({ floor: 2000 });
  await accountState(env, ACCOUNT);
  const now = 5_000_000_000_000; // ms
  const a = await call(env, 'bump_floor', { now, reason: 'logout' });
  assert.equal(a.floor, 5_000_000_001);
  assert.equal(await env.USERS.get(`authfloor:${ACCOUNT}`), '5000000001');
  // The same second again: past the floor it just set.
  const b = await call(env, 'bump_floor', { now });
  assert.equal(b.floor, 5_000_000_002);
  // A clock that went backwards cannot lower it.
  const c = await call(env, 'bump_floor', { now: 1000 });
  assert.equal(c.floor, 5_000_000_003);
});

test('many floor bumps at once all land, each past the last', async () => {
  const { env } = await setup();
  await accountState(env, ACCOUNT);
  const results = await Promise.all(Array.from({ length: 12 }, () => call(env, 'bump_floor', { now: 9_000_000_000_000 })));
  const floors = results.map((r) => r.floor).sort((x, y) => x - y);
  assert.equal(new Set(floors).size, 12, 'no two bumps returned the same floor');
  assert.equal(floors[11], 9_000_000_012);
});

// ── Replacing the key ────────────────────────────────────────────────────────

test('the message both keys sign is the documented one, line by line', () => {
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: 'a'.repeat(64), newKey: 'b'.repeat(64), challenge: 'c'.repeat(64) });
  assert.equal(m, ['eustress-key-rotation-v1', ORIGIN, ACCOUNT, 'a'.repeat(64), 'b'.repeat(64), 'c'.repeat(64)].join(String.fromCharCode(10)));
});

test('a rotation with both signatures replaces the key, retires the old one and leaves a mirror in KV', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const now = 1_800_000_000_000;
  const { result, newKey } = await rotation(env, k0, { now });
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(result.key_epoch, 1);
  assert.equal(result.floor, 1_800_000_001);

  const state = await call(env, 'state');
  assert.equal(state.public_key, newKey.hex);
  assert.equal(state.key_epoch, 1);
  assert.equal(state.retired.length, 1);
  assert.equal(state.retired[0].how, 'rotation');
  assert.ok(!JSON.stringify(state).includes(k0.hex), 'a retired key is shown by fingerprint only');

  // The new key signs in; the old one is retired, with the date.
  assert.equal((await call(env, 'check_login', { public_key: newKey.hex })).ok, true);
  const old = await call(env, 'check_login', { public_key: k0.hex });
  assert.equal(old.ok, false);
  assert.equal(old.code, 'key_retired');
  assert.equal(old.how, 'rotation');
  assert.ok(old.retired_at);

  // KV mirror.
  assert.equal(await env.USERS.get(`pubkey:${newKey.hex}`), ACCOUNT);
  assert.equal(await env.USERS.get(`pubkey:${k0.hex}`), null);
  assert.equal(await env.USERS.get(`retired-key:${k0.hex}`), ACCOUNT);
  assert.equal(await env.USERS.get(`authfloor:${ACCOUNT}`), String(result.floor));
  // The user record is NOT rewritten: a read-modify-write of it can undo a ban or
  // an admin change that lands meanwhile. The object is where the key and the epoch are read.
  const user = JSON.parse(await env.USERS.get(`user:${ACCOUNT}`));
  assert.equal(user.public_key, k0.hex);
  assert.equal(user.key_epoch, undefined);
  assert.equal(user.key_rotated_at, undefined);
  assert.equal((await call(env, 'state')).key_epoch, 1);
});

test('after a rotation value is frozen 7 days for an account that holds value and 72 hours for one that does not, and the payout is pinned 30 days', async () => {
  for (const [holds, span] of [[true, FREEZE_WITH_VALUE_MS], [false, FREEZE_WITHOUT_VALUE_MS], [undefined, FREEZE_WITHOUT_VALUE_MS]]) {
    const { env, k0 } = await setup();
    await accountState(env, ACCOUNT);
    const now = 1_800_000_000_000;
    const { result } = await rotation(env, k0, { now, holds_value: holds });
    assert.equal(result.ok, true);
    assert.equal(result.value_frozen_until, now + span, `holds_value ${holds}`);
    assert.equal(result.payout_pinned_until, now + PAYOUT_PIN_MS);
  }
  assert.equal(FREEZE_WITH_VALUE_MS, 7 * 24 * 3600 * 1000);
  assert.equal(FREEZE_WITHOUT_VALUE_MS, 72 * 3600 * 1000);
});

test('a rotation refuses each signature that is missing, wrong, or made for another account, origin, key or challenge, and changes nothing', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const stranger = keyPair();
  const variants = {
    'no old signature': (r) => { r.old_signature = ''; },
    'no new signature': (r) => { r.new_signature = undefined; },
    'old signature by another key': (r, c) => { r.old_signature = stranger.sign(c.message); },
    'new signature by another key': (r, c) => { r.new_signature = stranger.sign(c.message); },
    'the new key signed by the old key only': (r, c) => { r.new_signature = c.oldKey.sign(c.message); },
    'signed for another account': (r, c) => { const m = rotationMessage({ origin: ORIGIN, accountId: OTHER, oldKey: c.oldKey.hex, newKey: c.newKey.hex, challenge: c.begun.challenge }); r.old_signature = c.oldKey.sign(m); r.new_signature = c.newKey.sign(m); },
    'signed for another origin': (r, c) => { const m = rotationMessage({ origin: 'https://evil.example', accountId: ACCOUNT, oldKey: c.oldKey.hex, newKey: c.newKey.hex, challenge: c.begun.challenge }); r.old_signature = c.oldKey.sign(m); r.new_signature = c.newKey.sign(m); },
    'signed for another new key': (r, c) => { const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: c.oldKey.hex, newKey: stranger.hex, challenge: c.begun.challenge }); r.old_signature = c.oldKey.sign(m); r.new_signature = c.newKey.sign(m); },
    'signed over the bare challenge': (r, c) => { r.old_signature = c.oldKey.sign(c.begun.challenge); r.new_signature = c.newKey.sign(c.begun.challenge); },
    'a signature of the wrong length': (r) => { r.old_signature = r.old_signature.slice(2); },
    'a signature that is not hex': (r) => { r.new_signature = 'zz'.repeat(64); },
  };
  for (const [label, mutate] of Object.entries(variants)) {
    const { result } = await rotation(env, k0, { now: 1_800_000_000_000, mutate });
    assert.equal(result.ok, false, label);
    assert.equal(result.code, 'signature_invalid', label);
  }
  const state = await call(env, 'state');
  assert.equal(state.public_key, k0.hex, 'the key never changed');
  assert.equal(state.key_epoch, 0);
});

test('a rotation challenge works once, for this account, within five minutes', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const now = 1_800_000_000_000;
  const first = await rotation(env, k0, { now });
  assert.equal(first.result.ok, true);

  // Used already.
  const again = await call(env, 'rotate', first.request);
  assert.equal(again.code, 'challenge_invalid');

  // Expired: begun at now, used six minutes later.
  const { env: env2, k0: key2 } = await setup();
  await accountState(env2, ACCOUNT);
  const stale = await rotation(env2, key2, { now, mutate: (r) => { r.now = now + 6 * 60 * 1000; } });
  assert.equal(stale.result.code, 'challenge_invalid');
  assert.equal((await call(env2, 'state')).key_epoch, 0);

  // Never issued, malformed, or another account's.
  const never = await call(env2, 'rotate', { ...stale.request, challenge: 'f'.repeat(64) });
  assert.equal(never.code, 'challenge_invalid');
  assert.equal((await call(env2, 'rotate', { ...stale.request, challenge: 'zz' })).code, 'challenge_invalid');
});

test('a challenge issued for one account does not work for another', async () => {
  const { env, k0 } = await setup();
  const k1 = keyPair();
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ id: OTHER, username: 'other', public_key: k1.hex }));
  await accountState(env, ACCOUNT);
  await accountState(env, OTHER);
  const begun = await call(env, 'begin_rotation', {}, ACCOUNT);
  const nk = keyPair();
  const message = rotationMessage({ origin: ORIGIN, accountId: OTHER, oldKey: k1.hex, newKey: nk.hex, challenge: begun.challenge });
  const r = await call(env, 'rotate', { new_public_key: nk.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: k1.sign(message), new_signature: nk.sign(message) }, OTHER);
  assert.equal(r.code, 'challenge_invalid');
  assert.equal((await call(env, 'state', {}, OTHER)).key_epoch, 0);
});

test('a new key that is a small-order point, not a key, or already this account\'s is refused', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  assert.equal(SMALL_ORDER_KEYS.size, 12);
  for (const bad of [...SMALL_ORDER_KEYS, 'not a key', 'a'.repeat(63), 12345, null]) {
    const begun = await call(env, 'begin_rotation');
    const r = await call(env, 'rotate', { new_public_key: bad, challenge: begun.challenge, origin: ORIGIN, old_signature: '00'.repeat(64), new_signature: '00'.repeat(64) });
    assert.equal(r.ok, false, String(bad));
    assert.equal(r.code, 'key_unacceptable', String(bad));
  }
  // The current key as the new key.
  const same = await rotation(env, k0, { newKey: k0 });
  assert.equal(same.result.code, 'key_unacceptable');
  assert.equal((await call(env, 'state')).key_epoch, 0);
});

test('a retired key can never be the account\'s key again', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const first = await rotation(env, k0);
  assert.equal(first.result.ok, true);
  const back = await rotation(env, first.newKey, { newKey: k0 });
  assert.equal(back.result.ok, false);
  assert.equal(back.result.code, 'key_unacceptable');
});

test('two rotations at once: exactly one wins, and the loser\'s key never signs in', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const a = keyPair();
  const b = keyPair();
  const begunA = await call(env, 'begin_rotation');
  const begunB = await call(env, 'begin_rotation');
  const send = (nk, begun) => {
    const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: begun.challenge });
    return call(env, 'rotate', { new_public_key: nk.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: k0.sign(message), new_signature: nk.sign(message) });
  };
  const [ra, rb] = await Promise.all([send(a, begunA), send(b, begunB)]);
  assert.equal([ra, rb].filter((r) => r.ok).length, 1, JSON.stringify([ra, rb]));
  const winner = ra.ok ? a : b;
  const loser = ra.ok ? b : a;
  assert.equal((await call(env, 'check_login', { public_key: winner.hex })).ok, true);
  assert.equal((await call(env, 'check_login', { public_key: loser.hex })).ok, false);
  assert.equal((await call(env, 'state')).key_epoch, 1);
});

test('the object serializes whole operations itself, without relying on its caller to queue them', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const direct = (op, args) => object.fetch(new Request(`https://account.internal/${op}`, { method: 'POST', body: JSON.stringify({ ...args, account_id: ACCOUNT }) })).then((r) => r.json());
  const a = keyPair();
  const b = keyPair();
  const begunA = await direct('begin_rotation', {});
  const begunB = await direct('begin_rotation', {});
  const send = (nk, begun) => {
    const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: begun.challenge });
    return direct('rotate', { new_public_key: nk.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: k0.sign(message), new_signature: nk.sign(message) });
  };
  const [ra, rb] = await Promise.all([send(a, begunA), send(b, begunB)]);
  assert.equal([ra, rb].filter((r) => r.ok).length, 1, JSON.stringify([ra, rb]));
  assert.equal((await direct('state', {})).key_epoch, 1);
});

test('the old key alone cannot rotate and neither can the new key alone', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const nk = keyPair();
  const begun = await call(env, 'begin_rotation');
  const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: begun.challenge });
  const onlyOld = await call(env, 'rotate', { new_public_key: nk.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: k0.sign(message) });
  assert.equal(onlyOld.code, 'signature_invalid');
  const begun2 = await call(env, 'begin_rotation');
  const message2 = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: begun2.challenge });
  const onlyNew = await call(env, 'rotate', { new_public_key: nk.hex, challenge: begun2.challenge, origin: ORIGIN, new_signature: nk.sign(message2) });
  assert.equal(onlyNew.code, 'signature_invalid');
});

test('only a few challenges stay open at once, and the oldest goes first', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const begun = [];
  for (let i = 0; i < 7; i++) begun.push(await call(env, 'begin_rotation', { now: 1_800_000_000_000 + i }));
  const nk = keyPair();
  const use = (b) => {
    const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: b.challenge });
    return call(env, 'rotate', { new_public_key: nk.hex, challenge: b.challenge, origin: ORIGIN, now: 1_800_000_000_100, old_signature: k0.sign(message), new_signature: nk.sign(message) });
  };
  assert.equal((await use(begun[0])).code, 'challenge_invalid', 'the first of seven was dropped');
  assert.equal((await use(begun[6])).ok, true, 'the newest still works');
});

// ── The audit ────────────────────────────────────────────────────────────────

test('every change is in a hash-chained audit that shows a tampered record', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  await call(env, 'bump_floor', { reason: 'logout' });
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true);
  const audit = await call(env, 'audit', { limit: 20 });
  assert.deepEqual(audit.records.map((r) => r.type), ['seeded', 'sessions_ended', 'key_rotated']);
  assert.equal(audit.head, audit.records[2].hash);
  assert.equal(await verifyAuditChain(audit.records), true);
  // The record names the keys by fingerprint, never in full.
  assert.ok(!JSON.stringify(audit.records).includes(k0.hex));
  assert.ok(audit.records[2].data.from && audit.records[2].data.to);

  // Alter one, drop one, reorder.
  const tampered = structuredClone(audit.records);
  tampered[1].data.reason = 'something else';
  assert.equal(await verifyAuditChain(tampered), false);
  assert.equal(await verifyAuditChain([audit.records[0], audit.records[2]]), false);
  assert.equal(await verifyAuditChain([audit.records[1], audit.records[0], audit.records[2]]), false);

  // Rewrite a record, recompute its hash and repair the next record's link: the next record's own hash covers its link, so the chain still breaks.
  const forged = structuredClone(audit.records);
  forged[0].data.floor = 12345;
  forged[0].hash = await auditHash(forged[0].prev, forged[0]);
  forged[1].prev = forged[0].hash;
  assert.equal(await verifyAuditChain(forged), false);
});

// ── The mirror outbox ────────────────────────────────────────────────────────

test('a KV write that fails is retried by the alarm, and the account is never locked out in between', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  let failures = 2;
  env.USERS.put = async (key, ...rest) => {
    if (key.startsWith('pubkey:') && failures > 0) { failures--; throw new Error('KV unavailable'); }
    return realPut(key, ...rest);
  };
  const { result, newKey } = await rotation(env, k0);
  assert.equal(result.ok, true, 'the rotation itself is not undone by a failed mirror write');
  assert.equal((await call(env, 'check_login', { public_key: newKey.hex })).ok, true, 'the object already knows the new key');
  assert.equal((await call(env, 'state')).mirror_pending, 1, 'one write is still owed');
  assert.equal(await env.USERS.get(`pubkey:${newKey.hex}`), null);

  // The alarm tries again: the first retry fails once more, the second lands.
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await object.alarm();
  assert.equal((await call(env, 'state')).mirror_pending, 1, 'still owed after the first retry');
  await object.alarm();
  assert.equal(await env.USERS.get(`pubkey:${newKey.hex}`), ACCOUNT);
  assert.equal((await call(env, 'state')).mirror_pending, 0);
  assert.equal(failures, 0);
});

test('a write that keeps failing stays owed and the alarm stays armed; an admin repair applies it once KV is back', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  let down = true;
  env.USERS.put = async (key, ...rest) => {
    if (down && key.startsWith('retired-key:')) throw new Error('KV unavailable');
    return realPut(key, ...rest);
  };
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await object.storage.deleteAlarm(); // an alarm that fires is spent
  await object.alarm();
  assert.equal((await call(env, 'state')).mirror_pending, 1);
  assert.ok(object.storage.alarm, 'the alarm re-arms itself while a write is still owed');
  down = false;
  const repaired = await call(env, 'repair_mirror');
  assert.equal(repaired.pending, 0);
  assert.equal(await env.USERS.get(`retired-key:${k0.hex}`), ACCOUNT);
});

test('a patch to a deleted account\'s record does nothing and does not stay owed', async () => {
  const { env } = await setup();
  await accountState(env, ACCOUNT);
  const started = await call(env, 'email_verify_start');
  assert.equal(started.ok, true, JSON.stringify(started));
  await env.USERS.delete(`user:${ACCOUNT}`);
  const confirmed = await call(env, 'email_verify_confirm', { token: started.token.split('.')[1] });
  assert.equal(confirmed.ok, true, JSON.stringify(confirmed));
  assert.equal((await call(env, 'state')).mirror_pending, 0);
});

// ── The outbox and the alarm, under interleaving ─────────────────────────────

const settle = () => new Promise((resolve) => setTimeout(resolve, 5));

test('the outbox retry runs in the operation chain: a rotation that lands while it waits on KV is not undone', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  let mode = 'fail';
  let release = null;
  env.USERS.put = async (key, ...rest) => {
    if (key.startsWith('retired-key:') && mode === 'fail') throw new Error('KV unavailable');
    if (key.startsWith('retired-key:') && mode === 'hang') {
      mode = 'ok';
      await new Promise((resolve) => { release = resolve; });
    }
    return realPut(key, ...rest);
  };
  const first = await rotation(env, k0);
  assert.equal(first.result.ok, true);
  assert.equal((await call(env, 'state')).mirror_pending, 1, 'one write is owed');

  mode = 'hang';
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const retry = object.alarm(); // reads the state, then waits on KV
  while (!release) await settle();
  const secondPromise = rotation(env, first.newKey); // arrives while the retry waits
  await settle();
  release();
  await retry;
  const second = await secondPromise;
  assert.equal(second.result.ok, true, JSON.stringify(second.result));
  const state = await call(env, 'state');
  assert.equal(state.key_epoch, 2, 'the second rotation is still there');
  assert.equal((await call(env, 'check_login', { public_key: second.newKey.hex })).ok, true);
  assert.equal((await call(env, 'check_login', { public_key: first.newKey.hex })).code, 'key_retired');
  assert.equal(state.retired.length, 2);
});

test('an outbox retry queued behind an operation reads the state after that operation, not before it', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  let down = true;
  env.USERS.put = async (key, ...rest) => { if (down && key.startsWith('retired-key:')) throw new Error('KV unavailable'); return realPut(key, ...rest); };
  const first = await rotation(env, k0);
  assert.equal(first.result.ok, true);
  down = false;

  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const begun = await call(env, 'begin_rotation');
  const newKey = keyPair();
  const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: first.newKey.hex, newKey: newKey.hex, challenge: begun.challenge });
  const rotating = object.fetch(new Request('https://account.internal/rotate', {
    method: 'POST',
    body: JSON.stringify({
      account_id: ACCOUNT, new_public_key: newKey.hex, challenge: begun.challenge, origin: ORIGIN,
      old_signature: first.newKey.sign(message), new_signature: newKey.sign(message),
    }),
  })).then((r) => r.json());
  const retry = object.alarm(); // queued behind the rotation, with writes still owed from the first one
  const [second] = await Promise.all([rotating, retry]);
  assert.equal(second.ok, true, JSON.stringify(second));
  const state = await call(env, 'state');
  assert.equal(state.key_epoch, 2, 'the retry did not save the state from before the rotation');
  assert.equal((await call(env, 'check_login', { public_key: newKey.hex })).ok, true);
});

test('the retry is armed before the first mirror write is tried, so a write that never returns still leaves an alarm', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  env.USERS.put = () => new Promise(() => {}); // a KV write that never comes back
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  rotation(env, k0).catch(() => {});
  // Wait for the rotation to reach its first mirror write (a busy machine takes longer than a fixed pause).
  let held = null;
  for (let i = 0; i < 400 && !(held && held.key_epoch === 1); i++) {
    await settle();
    held = await object.storage.get('state');
  }
  assert.ok(object.storage.alarm, 'the alarm is armed while the first write is still out');
  assert.ok(held.mirror_pending.length > 0, 'the writes owed are on disk');
  assert.equal(held.key_epoch, 1, 'and so is the new key');
});

test('an object that finds writes owed and no alarm armed arms one', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (key, ...rest) => { if (key.startsWith('retired-key:')) throw new Error('KV unavailable'); return realPut(key, ...rest); };
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await object.storage.deleteAlarm(); // the alarm was lost
  assert.equal(object.storage.alarm, null);
  await call(env, 'check_login', { public_key: k0.hex });
  assert.ok(object.storage.alarm, 'the next operation noticed and armed it');
});

test('a retried older floor write cannot lower a newer floor, and only the newest floor is kept in the outbox', async () => {
  const { env } = await setup();
  await accountState(env, ACCOUNT);
  const realPut = env.USERS.put.bind(env.USERS);
  let down = true;
  env.USERS.put = async (key, ...rest) => { if (down && key.startsWith('authfloor:')) throw new Error('KV unavailable'); return realPut(key, ...rest); };
  const a = await call(env, 'bump_floor', { reason: 'logout', now: 1_900_000_000_000 });
  const b = await call(env, 'bump_floor', { reason: 'logout', now: 1_900_000_100_000 });
  assert.ok(b.floor > a.floor);
  assert.equal((await call(env, 'state')).mirror_pending, 1, 'one floor write is queued, the newest');

  // Something else (the KV-only fallback of a ban) raised the floor further meanwhile.
  down = false;
  await env.USERS.put(`authfloor:${ACCOUNT}`, String(b.floor + 1000));
  const repaired = await call(env, 'repair_mirror');
  assert.equal(repaired.pending, 0);
  assert.equal(await env.USERS.get(`authfloor:${ACCOUNT}`), String(b.floor + 1000), 'the retry did not lower it');
});

// ── A deployment with no objects ─────────────────────────────────────────────

test('without the binding, a call says so and does not throw', async () => {
  const r = await accountCall({ USERS: fakeKv() }, ACCOUNT, 'state');
  assert.equal(r.ok, false);
  assert.equal(r.status, 503);
  const gone = await accountCall({ ACCOUNT_AUTH: { get() { throw new Error('boom'); }, idFromName: (n) => n } }, ACCOUNT, 'state');
  assert.equal(gone.status, 503);
  assert.equal(gone.code, 'unavailable');
});

// ── The shared vectors ───────────────────────────────────────────────────────

import fs from 'node:fs';

test('the shared rotation vectors: each signature pair verifies at its own origin, and at no other', async () => {
  const vectors = JSON.parse(fs.readFileSync(new URL('../../../../docs/architecture/key-rotation-vectors.json', import.meta.url), 'utf8'));
  for (const v of vectors.rotations) {
    assert.equal(v.message, rotationMessage({ origin: v.origin, accountId: v.account_id, oldKey: vectors.old_key.public_key, newKey: vectors.new_key.public_key, challenge: v.challenge }));
    for (const reached of vectors.rotations.map((r) => r.origin)) {
      const env = { USERS: fakeKv() };
      env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
      await env.USERS.put(`user:${v.account_id}`, JSON.stringify({ id: v.account_id, username: 'vector', public_key: vectors.old_key.public_key }));
      await accountState(env, v.account_id);
      // Put the vector's challenge where begin_rotation would have: filed by digest.
      const object = env.ACCOUNT_AUTH.instances.get(v.account_id);
      const state = await object.storage.get('state');
      state.challenges[crypto.createHash('sha256').update(v.challenge).digest('hex')] = { purpose: 'rotation', exp: Date.now() + 60_000 };
      await object.storage.put('state', state);
      const r = await accountCall(env, v.account_id, 'rotate', {
        new_public_key: vectors.new_key.public_key, challenge: v.challenge, origin: reached,
        old_signature: v.old_signature, new_signature: v.new_signature,
      });
      if (reached === v.origin) assert.equal(r.ok, true, `${v.origin}: ${JSON.stringify(r)}`);
      else assert.equal(r.code, 'signature_invalid', `signed for ${v.origin}, sent as ${reached}`);
    }
  }
});

// ── Review round 2: a stuck KV write must not hold the account hostage ────────

test('a KV write that never returns is given up on after a time limit, so the account\'s operations are not held for it', async () => {
  const { env, k0 } = await setup();
  env.MIRROR_WRITE_TIMEOUT_MS = 40;
  await accountState(env, ACCOUNT);
  env.USERS.put = (key) => (key.startsWith('retired-key:') ? new Promise(() => {}) : Promise.resolve());
  const started = Date.now();
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.ok(Date.now() - started < 2000, 'the rotation did not wait on the hung write');
  assert.equal((await call(env, 'state')).mirror_pending, 1, 'the write that hung is still owed');

  // The retry that hangs again does not hold sign-in either.
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const retry = object.alarm();
  const check = await call(env, 'check_login', { public_key: k0.hex });
  assert.equal(check.code, 'key_retired');
  await retry;
});

test('a caller gives up on an object that does not answer, with "unavailable", so sign-in says 503 and does not hang', async () => {
  const env = { ACCOUNT_CALL_TIMEOUT_MS: 40, ACCOUNT_AUTH: { idFromName: (n) => n, get: () => ({ fetch: () => new Promise(() => {}) }) } };
  const started = Date.now();
  const r = await accountCall(env, ACCOUNT, 'state');
  assert.equal(r.ok, false);
  assert.equal(r.code, 'unavailable');
  assert.equal(r.status, 503);
  assert.ok(Date.now() - started < 2000);
});

test('the retry is armed before the state is written: a write that stalls still leaves an alarm', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const realPut = object.storage.put.bind(object.storage);
  object.storage.put = (...args) => (args[0] && typeof args[0] === 'object' && args[0].state ? new Promise(() => {}) : realPut(...args));
  rotation(env, k0).catch(() => {});
  for (let i = 0; i < 400 && !object.storage.alarm; i++) await settle();
  assert.ok(object.storage.alarm, 'the alarm is set while the first write is still out');
});

test('three key replacements a day, counted where the signatures are good', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  let key = k0;
  for (let i = 0; i < 3; i++) {
    const r = await rotation(env, key, { now: 1_900_000_000_000 + i * 1000 });
    assert.equal(r.result.ok, true, JSON.stringify(r.result));
    key = r.newKey;
  }
  const fourth = await rotation(env, key, { now: 1_900_000_000_000 + 10_000 });
  assert.equal(fourth.result.ok, false);
  assert.equal(fourth.result.code, 'rate_limited');
  assert.equal((await call(env, 'state')).key_epoch, 3, 'nothing changed');
  const later = await rotation(env, key, { now: 1_900_000_000_000 + 25 * 3600 * 1000 });
  assert.equal(later.result.ok, true, 'a day later it works again');
});

test('failed attempts do not use up the day\'s replacements', async () => {
  const { env, k0 } = await setup();
  await accountState(env, ACCOUNT);
  const stranger = keyPair();
  for (let i = 0; i < 6; i++) {
    const { result } = await rotation(env, k0, { mutate: (req, ctx) => { req.old_signature = stranger.sign(ctx.message); } });
    assert.equal(result.code, 'signature_invalid');
  }
  const ok = await rotation(env, k0);
  assert.equal(ok.result.ok, true, 'the real owner can still replace the key');
});
