// Sign-in and sign-out with account objects in place: the object, not the KV
// index, decides which key is current, and ends tokens. The real Worker runs
// against in-memory storage and the real object class.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, rotationMessage } from '../src/account_auth.mjs';

const worker = await loadWorker();
const ACCOUNT = '33333333-3333-4333-8333-333333333333';
const ORIGIN = 'https://api.eustress.dev';

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}
const message = (challenge) => ['eustress-login-v1', ORIGIN, challenge].join(String.fromCharCode(10));

async function environment({ objects = true } = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN };
  if (objects) env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  const key = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ id: ACCOUNT, username: 'owner', public_key: key.hex, email: 'owner@example.com' }));
  await env.USERS.put(`pubkey:${key.hex}`, ACCOUNT);
  return { env, key };
}

async function post(env, route, body, token) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method: 'POST', headers, body: JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
async function me(env, token) {
  const res = await worker.fetch(new Request(`${ORIGIN}/api/auth/me`, { headers: token ? { Authorization: `Bearer ${token}` } : {} }), env, ctx);
  return res.status;
}
async function signIn(env, key) {
  const challenge = (await post(env, '/api/auth/challenge', { public_key: key.hex })).body.challenge;
  return post(env, '/api/auth/verify-challenge', { format: 'eustress-login-v1', public_key: key.hex, challenge, signature: key.sign(message(challenge)) });
}
const claims = (token) => JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString());

/// Replace the account's key through the object, as the rotation route will.
async function rotate(env, oldKey, newKey = keyPair()) {
  const begun = await accountCall(env, ACCOUNT, 'begin_rotation');
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: oldKey.hex, newKey: newKey.hex, challenge: begun.challenge });
  const r = await accountCall(env, ACCOUNT, 'rotate', { new_public_key: newKey.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: oldKey.sign(m), new_signature: newKey.sign(m) });
  assert.equal(r.ok, true, JSON.stringify(r));
  return { newKey, rotated: r };
}

test('with account objects, the current key signs in and the object is seeded from KV on the way', async () => {
  const { env, key } = await environment();
  const r = await signIn(env, key);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.user.username, 'owner');
  const state = await accountCall(env, ACCOUNT, 'state');
  assert.equal(state.seeded, true);
  assert.equal(state.public_key, key.hex);
});

test('a key the account replaced is refused with the reason, even while the KV index still names it', async () => {
  const { env, key } = await environment();
  assert.equal((await signIn(env, key)).status, 200);
  const { newKey } = await rotate(env, key);
  // The KV index lags: put the old entry back, as a stale read at another location would see it.
  await env.USERS.put(`pubkey:${key.hex}`, ACCOUNT);
  const old = await signIn(env, key);
  assert.equal(old.status, 403);
  assert.equal(old.body.code, 'key_retired');
  assert.match(old.body.error, /replaced/);
  assert.ok(old.body.reason);
  assert.ok(old.body.retired_at);

  // With the index as the mirror leaves it (the old entry deleted), the answer is the same, not "not registered".
  await env.USERS.delete(`pubkey:${key.hex}`);
  const again = await signIn(env, key);
  assert.equal(again.status, 403);
  assert.equal(again.body.code, 'key_retired');

  // The new key signs in.
  const fresh = await signIn(env, newKey);
  assert.equal(fresh.status, 200, JSON.stringify(fresh.body));
  assert.equal(fresh.body.user.username, 'owner');
});

test('a key that was never the account\'s is still "not registered", and nothing says it was replaced', async () => {
  const { env } = await environment();
  const stranger = keyPair();
  const r = await signIn(env, stranger);
  assert.equal(r.status, 404);
  assert.equal(r.body.code, undefined);
});

test('an index entry that points at an account whose current key is another one is refused, not trusted', async () => {
  const { env } = await environment();
  const squatter = keyPair();
  await env.USERS.put(`pubkey:${squatter.hex}`, ACCOUNT); // a stale or planted index entry
  const r = await signIn(env, squatter);
  assert.equal(r.status, 403);
  assert.equal(r.body.code, 'key_retired');
});

test('an object that cannot be reached means no sign-in, never a sign-in on the KV hint', async () => {
  const { env, key } = await environment();
  assert.equal((await signIn(env, key)).status, 200);
  env.ACCOUNT_AUTH = { idFromName: (n) => n, get() { throw new Error('the object is down'); } };
  const r = await signIn(env, key);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'account_unavailable');
  assert.equal(r.body.token, undefined);
});

test('an object that answers 404 for something other than "no such account" is no sign-in either', async () => {
  const { env, key } = await environment();
  const unknown = new Response(JSON.stringify({ ok: false, status: 404, code: 'unknown_operation' }), { status: 404 });
  env.ACCOUNT_AUTH = { idFromName: (n) => n, get: () => ({ fetch: async () => unknown.clone() }) };
  const r = await signIn(env, key);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'account_unavailable');
});

test('without account objects the older sign-in still works', async () => {
  const { env, key } = await environment({ objects: false });
  const r = await signIn(env, key);
  assert.equal(r.status, 200, JSON.stringify(r.body));
});

test('signing out ends the account\'s tokens through the object, which keeps the floor and mirrors it to KV', async () => {
  const { env, key } = await environment();
  const first = await signIn(env, key);
  const token = first.body.token;
  assert.equal(await me(env, token), 200);
  assert.equal((await post(env, '/api/auth/logout', {}, token)).status, 200);
  assert.equal(await me(env, token), 401);

  const state = await accountCall(env, ACCOUNT, 'state');
  assert.ok(state.floor > 0);
  assert.equal(await env.USERS.get(`authfloor:${ACCOUNT}`), String(state.floor));
  const audit = await accountCall(env, ACCOUNT, 'audit', { limit: 10 });
  assert.deepEqual(audit.records.map((r) => r.type), ['seeded', 'sessions_ended']);
  assert.equal(audit.records[1].data.reason, 'logout');

  // Straight back in, in the same second: the new token is stamped at the floor.
  const second = await signIn(env, key);
  assert.equal(second.status, 200);
  assert.ok(claims(second.body.token).iat >= state.floor);
  assert.equal(await me(env, second.body.token), 200);
});

test('a rotation ends the tokens the account held, and a sign-in with the new key is not caught by the floor', async () => {
  const { env, key } = await environment();
  const held = (await signIn(env, key)).body.token;
  assert.equal(await me(env, held), 200);
  const { newKey, rotated } = await rotate(env, key);
  assert.equal(await me(env, held), 401, 'the token issued before the rotation is refused');
  const fresh = await signIn(env, newKey);
  assert.equal(fresh.status, 200);
  assert.ok(claims(fresh.body.token).iat >= rotated.floor);
  assert.equal(await me(env, fresh.body.token), 200);
});

test('a token is stamped no earlier than the floor of the object even when the KV mirror of it is missing', async () => {
  const { env, key } = await environment();
  await signIn(env, key);
  const bumped = await accountCall(env, ACCOUNT, 'bump_floor', { reason: 'test', now: Date.now() });
  await env.USERS.delete(`authfloor:${ACCOUNT}`); // a stale read at another location
  const r = await signIn(env, key);
  assert.equal(r.status, 200);
  assert.ok(claims(r.body.token).iat >= bumped.floor);
});

test('a ban ends tokens through the object', async () => {
  const { env, key } = await environment();
  const ADMIN = '99999999-9999-4999-8999-999999999999';
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user' }));
  await env.USERS.put('username:owner', ACCOUNT);
  const held = (await signIn(env, key)).body.token;
  const ban = await post(env, '/api/admin/ban', { username: 'owner', reason: 'testing' }, mintToken(ADMIN));
  assert.equal(ban.status, 200, JSON.stringify(ban.body));
  assert.equal(await me(env, held), 401);
  const audit = await accountCall(env, ACCOUNT, 'audit', { limit: 10 });
  assert.equal(audit.records.at(-1).data.reason, 'ban');
});

test('deleting an account ends its tokens even though its record is gone and the object cannot be seeded', async () => {
  const { env, key } = await environment();
  const ADMIN = '99999999-9999-4999-8999-999999999999';
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user' }));
  await env.USERS.put('username:owner', ACCOUNT);
  const held = mintToken(ACCOUNT);
  const del = await post(env, '/api/admin/delete-user', { username: 'owner' }, mintToken(ADMIN));
  assert.equal(del.status, 200, JSON.stringify(del.body));
  assert.equal(await me(env, held), 401);
  assert.ok(Number(await env.USERS.get(`authfloor:${ACCOUNT}`)) > 0, 'the floor is in KV');
});

test('deleting a replaced account removes the index entry of its current key, not only of the key it joined with', async () => {
  const { env, key } = await environment();
  const ADMIN = '99999999-9999-4999-8999-999999999999';
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user' }));
  await env.USERS.put('username:owner', ACCOUNT);
  assert.equal((await signIn(env, key)).status, 200);
  const { newKey } = await rotate(env, key);
  await env.USERS.put(`pubkey:${newKey.hex}`, ACCOUNT); // the mirror of the rotation
  await env.USERS.put(`email:${ACCOUNT}`, JSON.stringify({ address: 'new@example.com', verified_at: null }));
  await env.USERS.put(`valuefreeze:${ACCOUNT}`, String(Date.now() + 86400000));
  const del = await post(env, '/api/admin/delete-user', { username: 'owner' }, mintToken(ADMIN));
  assert.equal(del.status, 200, JSON.stringify(del.body));
  assert.equal(await env.USERS.get(`pubkey:${newKey.hex}`), null, 'the current key no longer points at a deleted account');
  assert.equal(await env.USERS.get(`pubkey:${key.hex}`), null);
  assert.equal(await env.USERS.get(`retired-key:${key.hex}`), ACCOUNT, 'a replaced key stays retired for good');
  assert.equal(await env.USERS.get(`email:${ACCOUNT}`), null, 'what the object kept for the account goes with it');
  assert.equal(await env.USERS.get(`valuefreeze:${ACCOUNT}`), null);
});

test('a key an account replaced cannot be registered as a new account', async () => {
  const { env, key } = await environment();
  await signIn(env, key);
  await rotate(env, key);
  await env.USERS.delete(`pubkey:${key.hex}`);
  const r = await post(env, '/api/auth/register', { username: 'someone_new', public_key: key.hex, birthday: '1990-01-01' });
  assert.equal(r.status, 409);
});
