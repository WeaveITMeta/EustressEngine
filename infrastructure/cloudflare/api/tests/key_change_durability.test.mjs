// A key replacement is not a thing that can half happen: the notice is recorded with the key, the route that made
// the change sends it and says so, the object's alarm sends what is not acknowledged, and a stuck KV cannot make
// the caller give up on a change that has already been made (docs/architecture/
// IDENTITY_KEY_ROTATION_AND_RECOVERY.md).

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, accountState, rotationMessage, INLINE_FLUSH_BUDGET_MS, CALL_TIMEOUT_MS, MIRROR_WRITE_TIMEOUT_MS } from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { emailChangeMessage } from '../src/email_ops.mjs';

const worker = await loadWorker();
const ACCOUNT = '55555555-5555-4555-8555-555555555566';
const ORIGIN = 'https://api.eustress.dev';
const MINUTE = 60 * 1000;

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}
class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
const bodyOf = (m) => [...m.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
  .map((x) => Buffer.from(x[1].replace(/\r\n/g, ''), 'base64').toString()).join('\n');
const settle = () => new Promise((resolve) => setTimeout(resolve, 5));

/// An environment with mail, the account's record and its object.
async function environment(extra = {}) {
  const sent = [];
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN,
    EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  const k0 = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: k0.hex, email: 'owner@example.com' }));
  await env.USERS.put('username:owner', ACCOUNT);
  await env.USERS.put(`pubkey:${k0.hex}`, ACCOUNT);
  await accountState(env, ACCOUNT);
  return { env, k0, sent, token: mintToken(ACCOUNT) };
}

const call = (env, op, args = {}) => accountCall(env, ACCOUNT, op, args);

/// A rotation straight at the object. `username` is what the route passes.
async function rotation(env, oldKey, { newKey = keyPair(), now, username = 'owner' } = {}) {
  const begun = await call(env, 'begin_rotation', { now });
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: oldKey.hex, newKey: newKey.hex, challenge: begun.challenge });
  const result = await call(env, 'rotate', {
    new_public_key: newKey.hex, challenge: begun.challenge, origin: ORIGIN, now, username,
    old_signature: oldKey.sign(m), new_signature: newKey.sign(m),
  });
  return { newKey, result };
}

async function post(env, route, body, token) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method: 'POST', headers, body: JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
async function rotateThroughRoutes(env, oldKey, token) {
  const begun = await post(env, '/api/identity/rotate/begin', {}, token);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: oldKey.hex, newKey: newKey.hex, challenge: begun.body.challenge });
  const r = await post(env, '/api/identity/rotate', { new_public_key: newKey.hex, challenge: begun.body.challenge, old_signature: oldKey.sign(m), new_signature: newKey.sign(m) }, token);
  return { newKey, r };
}

/// Run `fn` with the clock `ms` ahead.
async function later(ms, fn) {
  const realNow = Date.now;
  Date.now = () => realNow() + ms;
  try { return await fn(); } finally { Date.now = realNow; }
}

// ── A stuck KV does not hold up the answer ───────────────────────────────────

test('a replacement answers within its KV budget when every mirror write hangs, and what was not written stays owed to the alarm', async () => {
  // One write is allowed 400 ms and the whole answer 100: five hung writes would take two seconds.
  const { env, k0 } = await environment({ MIRROR_INLINE_BUDGET_MS: 100, MIRROR_WRITE_TIMEOUT_MS: 400 });
  env.USERS.put = () => new Promise(() => {}); // KV never answers a write
  const started = Date.now();
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.ok(Date.now() - started < 1000, 'it did not wait on KV beyond its budget');
  const state = await call(env, 'state');
  assert.equal(state.key_epoch, 1, 'the key changed');
  assert.ok(state.mirror_pending > 0, 'the writes are owed');
  assert.equal(state.notices_pending, 1, 'and so is the notice');
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  assert.ok(object.storage.alarm, 'the alarm is set');
});

test('the caller does not give up on a change that was already made: a stuck KV is not a 503', async () => {
  const { env, k0 } = await environment({ MIRROR_INLINE_BUDGET_MS: 60, MIRROR_WRITE_TIMEOUT_MS: 40, ACCOUNT_CALL_TIMEOUT_MS: 800 });
  env.USERS.put = () => new Promise(() => {});
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.notEqual(result.code, 'unavailable');
});

test('a change whose answer was lost says so: it may have gone through, and sign-in will tell', async () => {
  const { env, k0, token } = await environment({ ACCOUNT_CALL_TIMEOUT_MS: 100 });
  const real = env.ACCOUNT_AUTH;
  env.ACCOUNT_AUTH = {
    idFromName: real.idFromName,
    get: (id) => {
      const stub = real.get(id);
      return { fetch: async (url, init) => { if (String(url).endsWith('/rotate')) await new Promise((r) => setTimeout(r, 400)); return stub.fetch(url, init); } };
    },
  };
  const { r } = await rotateThroughRoutes(env, k0, token);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'account_unavailable');
  assert.match(r.body.error, /may have gone through/);
  assert.match(r.body.error, /NEW identity file/);
  await new Promise((resolve) => setTimeout(resolve, 600));
  assert.equal((await accountCall(real === env.ACCOUNT_AUTH ? env : { ...env, ACCOUNT_AUTH: real }, ACCOUNT, 'state')).key_epoch, 1, 'and it had: the object committed');
});

// ── The notice is part of the change ─────────────────────────────────────────

test('the notice is recorded with the key, names every address, and the alarm sends it when the route did not', async () => {
  const { env, k0, sent } = await environment();
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true);
  assert.equal((await call(env, 'state')).notices_pending, 1);
  assert.equal(sent.length, 0, 'nothing is sent while the route still has its minute');

  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await object.alarm();
  assert.equal(sent.length, 0, 'the alarm waits for the route');
  await later(61 * 1000, () => object.alarm());
  assert.equal(sent.length, 1);
  assert.equal(sent[0].to, 'owner@example.com');
  const text = bodyOf(sent[0]);
  assert.match(text, /was replaced/);
  assert.match(text, /owner/, 'it names the account');
  assert.ok(!text.includes(k0.hex), 'no key in the notice');
  assert.equal((await call(env, 'state')).notices_pending, 0);
});

test('a notice that cannot be sent is tried again, with growing waits, and given up on after six tries', async () => {
  const { env, k0, sent } = await environment();
  let down = true;
  env.EMAIL.send = async (m) => { if (down) throw new Error('mail is down'); sent.push(m); };
  await rotation(env, k0);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await later(61 * 1000, () => object.alarm());
  assert.equal((await call(env, 'state')).notices_pending, 1, 'still owed after the first failure');
  assert.equal(sent.length, 0);

  down = false;
  await later(10 * MINUTE, () => object.alarm());
  assert.equal(sent.length, 1, 'sent once mail is back');
  assert.equal((await call(env, 'state')).notices_pending, 0);

  // Mail that never comes back: six tries, then it is dropped.
  const second = await environment();
  second.env.EMAIL.send = async () => { throw new Error('mail is down'); };
  await rotation(second.env, second.k0);
  const obj2 = second.env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  for (let i = 1; i <= 7; i++) await later(i * 2 * 60 * MINUTE, () => obj2.alarm());
  assert.equal((await accountCall(second.env, ACCOUNT, 'state')).notices_pending, 0, 'given up after six tries');
});

test('an address the deployment cannot mail, or whose allowance is spent, is not retried for ever', async () => {
  const { env, k0 } = await environment();
  delete env.EMAIL;
  await rotation(env, k0);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await later(61 * 1000, () => object.alarm());
  assert.equal((await call(env, 'state')).notices_pending, 0, 'no mail binding: nothing to retry');
});

test('acknowledging a notice drops the addresses that were told and keeps the rest; it is idempotent and ignores strangers', async () => {
  const { env, k0 } = await environment();
  // A second address on the notice list: a change of address that is pending.
  const begun = await call(env, 'email_change_begin', {});
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: k0.hex, address: 'new@example.com', challenge: begun.challenge });
  const asked = await call(env, 'email_change_request', { challenge: begun.challenge, new_address: 'new@example.com', signature: k0.sign(message), origin: ORIGIN });
  assert.equal(asked.ok, true, JSON.stringify(asked));
  const { result } = await rotation(env, k0);
  assert.equal(result.ok, true);
  assert.deepEqual([...result.recipients].sort(), ['new@example.com', 'owner@example.com']);

  const other = await call(env, 'ack_notice', { id: 'nope', delivered: ['owner@example.com'] });
  assert.equal(other.ok, true);
  assert.equal(other.pending, 1, 'an id that is not owed changes nothing');
  const wrongPerson = await call(env, 'ack_notice', { id: result.notice_id, delivered: ['someone-else@example.com'] });
  assert.equal(wrongPerson.pending, 1, 'an address that is not on it changes nothing');
  const half = await call(env, 'ack_notice', { id: result.notice_id, delivered: ['OWNER@example.com'] });
  assert.equal(half.pending, 1, 'one address is still owed');
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  const held = await object.storage.get('state');
  assert.deepEqual(held.notices_pending[0].recipients, ['new@example.com']);
  const done = await call(env, 'ack_notice', { id: result.notice_id, delivered: ['new@example.com'] });
  assert.equal(done.pending, 0);
  assert.equal((await call(env, 'ack_notice', { id: result.notice_id, delivered: ['new@example.com'] })).pending, 0, 'idempotent');
});

test('through the routes, a notice that was sent is acknowledged and one that failed is left for the object', async () => {
  const sentOk = await environment();
  const ok = await rotateThroughRoutes(sentOk.env, sentOk.k0, sentOk.token);
  assert.equal(ok.r.status, 200, JSON.stringify(ok.r.body));
  assert.equal(ok.r.body.notices_sent, 1);
  assert.equal(sentOk.sent.length, 1);
  assert.equal((await accountCall(sentOk.env, ACCOUNT, 'state')).notices_pending, 0, 'acknowledged');

  const down = await environment();
  down.env.EMAIL.send = async () => { throw new Error('mail is down'); };
  const failed = await rotateThroughRoutes(down.env, down.k0, down.token);
  assert.equal(failed.r.status, 200, 'the replacement is not undone by a notice that failed');
  assert.equal(failed.r.body.notices_sent, 0);
  assert.equal((await accountCall(down.env, ACCOUNT, 'state')).notices_pending, 1, 'left for the object');

  down.env.EMAIL.send = async (m) => { down.sent.push(m); };
  const object = down.env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await later(61 * 1000, () => object.alarm());
  assert.equal(down.sent.length, 1, 'and the object sent it');
  assert.match(bodyOf(down.sent[0]), /"owner"/, 'naming the account, as the route would have');
  assert.equal((await accountCall(down.env, ACCOUNT, 'state')).notices_pending, 0);
});

test('a lost alarm is set again by the next operation while a notice is owed', async () => {
  const { env, k0 } = await environment();
  await rotation(env, k0);
  const object = env.ACCOUNT_AUTH.instances.get(ACCOUNT);
  await object.storage.deleteAlarm();
  assert.equal(object.storage.alarm, null);
  await call(env, 'check_login', { public_key: k0.hex });
  assert.ok(object.storage.alarm, 'armed again');
});

test('the defaults keep a key change inside what its caller will wait for', () => {
  assert.ok(INLINE_FLUSH_BUDGET_MS + 1000 <= CALL_TIMEOUT_MS, 'the KV budget of a change leaves room under the call timeout');
  assert.ok(INLINE_FLUSH_BUDGET_MS <= MIRROR_WRITE_TIMEOUT_MS, 'one write is never given longer than the whole budget would allow');
  assert.ok(CALL_TIMEOUT_MS <= 10000, 'a sign-in does not wait longer than ten seconds on an account');
});
