// What a key replacement does to money: value routes are frozen for a few days,
// the payout destination is pinned for thirty, and the nightly payout run skips a
// frozen account. And the two properties of API keys that keep the freeze from
// being walked around: a key cannot mint keys, and a live purchase needs a session.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountState, rotationMessage, FREEZE_WITH_VALUE_MS, FREEZE_WITHOUT_VALUE_MS, PAYOUT_PIN_MS } from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';

const worker = await loadWorker();
const ACCOUNT = '99999999-9999-4999-8999-999999999993';
const SELLER = '99999999-9999-4999-8999-999999999994';
const ORIGIN = 'https://api.eustress.dev';
const DAY = 24 * 3600 * 1000;

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

async function environment(extra = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, STRIPE_SECRET_KEY: 'sk_test_not_real', ...extra };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  const key = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: key.hex, email: 'owner@example.com' }));
  await env.USERS.put('username:owner', ACCOUNT);
  await env.USERS.put(`pubkey:${key.hex}`, ACCOUNT);
  await accountState(env, ACCOUNT);
  return { env, key };
}

async function send(env, method, route, { token, body, headers: extraHeaders } = {}) {
  const headers = { 'Content-Type': 'application/json', ...(extraHeaders || {}) };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method, headers, body: method === 'GET' ? undefined : JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

async function rotateAccount(env, key, holdsValue = false) {
  if (holdsValue) await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: key.hex, email: 'owner@example.com', stripe_connect_id: 'acct_x' }));
  const begun = await send(env, 'POST', '/api/identity/rotate/begin', { token: mintToken(ACCOUNT) });
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: key.hex, newKey: newKey.hex, challenge: begun.body.challenge });
  const r = await send(env, 'POST', '/api/identity/rotate', { token: mintToken(ACCOUNT), body: { new_public_key: newKey.hex, challenge: begun.body.challenge, old_signature: key.sign(m), new_signature: newKey.sign(m) } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

/// Run `fn` with the clock `ms` ahead.
async function later(ms, fn) {
  const realNow = Date.now;
  Date.now = () => realNow() + ms;
  try { return await fn(); } finally { Date.now = realNow; }
}

// ── The payout pin ───────────────────────────────────────────────────────────

test('the payout account cannot be set up or changed for thirty days after a key replacement, though the freeze ends sooner', async () => {
  const { env, key } = await environment();
  const rotated = await rotateAccount(env, key);
  assert.ok(rotated.payout_pinned_until - Date.now() > 29 * DAY, 'pinned about thirty days');

  const now = await send(env, 'POST', '/api/stripe/connect/onboard', { token: rotated.token });
  assert.equal(now.status, 423, JSON.stringify(now.body));
  assert.equal(now.body.code, 'account_frozen', 'the freeze answers first while it lasts');

  // Past the freeze (72 hours with no value), still inside the pin.
  const afterFreeze = await later(4 * DAY, () => send(env, 'POST', '/api/stripe/connect/onboard', { token: mintToken(ACCOUNT) }));
  assert.equal(afterFreeze.status, 423, JSON.stringify(afterFreeze.body));
  assert.equal(afterFreeze.body.code, 'payout_pinned');
  assert.equal(afterFreeze.body.until, new Date(rotated.payout_pinned_until).toISOString());

  const dayTwentyNine = await later(29 * DAY, () => send(env, 'POST', '/api/stripe/connect/onboard', { token: mintToken(ACCOUNT) }));
  assert.equal(dayTwentyNine.body?.code, 'payout_pinned', 'still pinned at day 29');

  const afterPin = await later(PAYOUT_PIN_MS + 60 * 1000, () => send(env, 'POST', '/api/stripe/connect/onboard', { token: mintToken(ACCOUNT) }));
  assert.notEqual(afterPin.body?.code, 'payout_pinned', 'the pin is over');
  assert.notEqual(afterPin.body?.code, 'account_frozen');
});

test('an account that never replaced its key is not pinned, and an unreachable object refuses rather than allows', async () => {
  const { env } = await environment();
  const free = await send(env, 'POST', '/api/stripe/connect/onboard', { token: mintToken(ACCOUNT) });
  assert.notEqual(free.body?.code, 'payout_pinned');
  assert.notEqual(free.body?.code, 'account_frozen');

  env.ACCOUNT_AUTH = { idFromName: (n) => n, get() { throw new Error('the object is down'); } };
  const down = await send(env, 'POST', '/api/stripe/connect/onboard', { token: mintToken(ACCOUNT) });
  assert.equal(down.status, 503);
  assert.equal(down.body.code, 'account_unavailable');
});

test('an account that held value is frozen for seven days, not three', async () => {
  const { env, key } = await environment();
  const rotated = await rotateAccount(env, key, true);
  assert.ok(Math.abs(rotated.value_frozen_until - Date.now() - FREEZE_WITH_VALUE_MS) < 5000);
  const day5 = await later(5 * DAY, () => send(env, 'POST', '/api/ledger/spend', { token: mintToken(ACCOUNT), body: {} }));
  assert.equal(day5.status, 423);
  const day8 = await later(8 * DAY, () => send(env, 'POST', '/api/ledger/spend', { token: mintToken(ACCOUNT), body: {} }));
  assert.notEqual(day8.status, 423);
});

// ── API keys do not walk around the freeze ───────────────────────────────────

async function mintLiveKey(env) {
  const made = await send(env, 'POST', '/api/keys', { token: mintToken(ACCOUNT), body: { name: 'live', mode: 'live' } });
  assert.equal(made.status, 201, JSON.stringify(made.body));
  assert.match(made.body.key, /^ek_live_/);
  return made.body.key;
}

test('an API key cannot mint API keys: only a signed-in session manages them', async () => {
  const { env } = await environment();
  const liveKey = await mintLiveKey(env);
  const r = await send(env, 'POST', '/api/keys', { token: liveKey, body: { name: 'child', mode: 'live' } });
  assert.equal(r.status, 403);
  assert.equal(r.body.code, 'session_required');
  const revoke = await send(env, 'DELETE', '/api/keys/key_abcdefghijklmnopqrstuvwx', { token: liveKey });
  assert.equal(revoke.status, 403);
});

test('a live purchase is refused for an API key: it takes a signed-in player, so the freeze on sessions covers it', async () => {
  const { env } = await environment();
  const liveKey = await mintLiveKey(env);
  const simId = '12345678-1234-4234-8234-123456789abc';
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify({ id: simId, author_id: SELLER, name: 'Harbor', is_public: true }));
  const r = await send(env, 'POST', '/api/commerce/purchases', {
    token: liveKey,
    body: { sim_id: simId, product: `prod_${'a'.repeat(24)}`, idempotency_key: 'idem-12345678', expected_price: 5 },
  });
  assert.equal(r.status, 403, JSON.stringify(r.body));
  assert.equal(r.body.code, 'live_purchase_requires_session');
});

// ── The health report says when the account objects are missing ──────────────

test('/health reports whether account objects and the value sources are bound, so a missing binding is seen', async () => {
  const { env } = await environment();
  const ok = await send(env, 'GET', '/health');
  assert.equal(ok.body.integrations.account_auth.configured, true);
  assert.deepEqual(ok.body.integrations.value_sources, { payouts: true, wallets: true, delta: false, social: true });
  const bare = { ...env, ACCOUNT_AUTH: undefined, WALLETS: undefined };
  const missing = await send(bare, 'GET', '/health');
  assert.equal(missing.body.integrations.account_auth.configured, false);
  assert.equal(missing.body.integrations.value_sources.wallets, false);
});

// ── What the one-time prompt says a replacement costs ────────────────────────

test('the signed-in view carries the pause and the pin in whole days, read from the same constants the object enforces', async () => {
  const { env } = await environment();
  const me = await send(env, 'GET', '/api/auth/me', { token: mintToken(ACCOUNT) });
  assert.equal(me.status, 200);
  const terms = me.body.key_replace_terms ?? me.body.user?.key_replace_terms;
  assert.deepEqual(terms, {
    pause_days: FREEZE_WITHOUT_VALUE_MS / DAY,
    pause_days_with_value: FREEZE_WITH_VALUE_MS / DAY,
    payout_pin_days: PAYOUT_PIN_MS / DAY,
  });
  assert.deepEqual(terms, { pause_days: 3, pause_days_with_value: 7, payout_pin_days: 30 });
  // And what the page says is what the rule does: the freeze after a replacement is that long.
  const { env: env2, key } = await environment();
  const rotated = await rotateAccount(env2, key);
  assert.ok(Math.abs(rotated.value_frozen_until - Date.now() - terms.pause_days * DAY) < 5000);
  assert.ok(Math.abs(rotated.payout_pinned_until - Date.now() - terms.payout_pin_days * DAY) < 5000);
});

test('a deployment without account objects tells the page nothing about it, and the page then says no number', async () => {
  const { env } = await environment();
  const bare = { ...env, ACCOUNT_AUTH: undefined };
  const me = await send(bare, 'GET', '/api/auth/me', { token: mintToken(ACCOUNT) });
  assert.equal((me.body.key_replace_terms ?? me.body.user?.key_replace_terms), undefined);
});
