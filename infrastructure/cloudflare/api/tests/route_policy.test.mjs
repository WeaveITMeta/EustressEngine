// Deny by default: every route that changes something is classed, the strict
// class is the default, and a path added to the source without a class fails here.
// Then the gate itself: a session the account's object has ended is refused on a
// fresh route even while the KV mirror still lets it through, a frozen account is
// refused on a value route, and nothing is let through when the object is down.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, accountState, rotationMessage, FREEZE_WITHOUT_VALUE_MS } from '../src/account_auth.mjs';
import { RULES, routePolicy, isMutating, PUBLIC, SESSION, FRESH, VALUE } from '../src/route_policy.mjs';
import { valueFrozenUntil } from '../src/fresh_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';

const SRC = path.resolve(import.meta.dirname, '..', 'src');

// ── The table ────────────────────────────────────────────────────────────────

test('only the methods that change something are classed', () => {
  for (const m of ['POST', 'PUT', 'PATCH', 'DELETE', 'post']) assert.equal(isMutating(m), true, m);
  for (const m of ['GET', 'HEAD', 'OPTIONS', '', undefined]) assert.equal(isMutating(m), false, String(m));
});

test('every rule has a known class and a reason', () => {
  for (const [pattern, klass, why] of RULES) {
    assert.ok([PUBLIC, SESSION, FRESH, VALUE].includes(klass), String(pattern));
    assert.ok(typeof why === 'string' && why.length > 3, `${pattern} has a reason`);
  }
});

test('a route the table does not name is fresh, and says it was not named', () => {
  for (const p of ['/api/brand/new/route', '/api/simulations/abc-123/new-thing', '/api/admin2', '/', '']) {
    const r = routePolicy('POST', p);
    assert.equal(r.class, FRESH, p);
    assert.equal(r.explicit, false, p);
  }
});

test('the routes that matter are in the class they must be in', () => {
  const expected = [
    // public: no session of its own
    ['POST', '/api/auth/register', PUBLIC], ['POST', '/api/auth/challenge', PUBLIC], ['POST', '/api/auth/verify-challenge', PUBLIC],
    ['POST', '/api/stripe/webhook', PUBLIC], ['POST', '/api/guest/play', PUBLIC], ['POST', '/api/kyc/submit', PUBLIC],
    ['POST', '/api/account/email/confirm', PUBLIC], ['POST', '/api/account/email/cancel', PUBLIC],
    // value: moves value out, or changes where it goes
    ['POST', '/api/stripe/connect/onboard', VALUE], ['POST', '/api/ledger/spend', VALUE], ['POST', '/api/commerce/purchases', VALUE],
    ['POST', '/api/commerce/purchases/pur_abcdefghijklmnopqrstuvwx/refund', VALUE],
    // fresh: acts as the account, or cannot be taken back
    ['POST', '/api/admin/ban', FRESH], ['POST', '/api/admin/delete-user', FRESH], ['POST', '/api/admin/moderation/tool', FRESH],
    ['POST', '/api/payouts/daily', FRESH], ['POST', '/api/accounting/costs', FRESH],
    ['PUT', '/api/admin/economics/costs', FRESH], ['PUT', '/api/admin/economics/settings', FRESH], ['POST', '/api/admin/economics/rebuild', FRESH],
    ['POST', '/api/identity/rotate', FRESH], ['POST', '/api/identity/rotate/begin', FRESH],
    ['POST', '/api/account/email/start', FRESH], ['POST', '/api/account/email/change', FRESH], ['POST', '/api/account/email/apply', FRESH],
    ['POST', '/api/keys', FRESH], ['DELETE', '/api/keys/key_abcdefghijklmnopqrstuvwx', FRESH],
    ['POST', '/api/commerce/webhook_endpoints', FRESH], ['DELETE', '/api/commerce/webhook_endpoints/we_abcdefghijklmnopqrstuvwx', FRESH],
    ['POST', '/api/commerce/products', FRESH], ['PUT', '/api/commerce/products/prod_abcdefghijklmnopqrstuvwx', FRESH],
    // Delta spending stays fresh and is not a value route: a key replacement does not pause it (McKale, 2026-10-06: the audit trail can revert the damage).
    ['POST', '/api/delta/charge', FRESH],
    ['POST', '/api/simulations/publish', FRESH], ['PUT', '/api/simulations/abc-123/space', FRESH], ['PUT', '/api/simulations/abc-123/spaces/Main', FRESH],
    ['POST', '/api/simulations/abc-123/space/multipart/complete', FRESH], ['PUT', '/api/simulations/abc-123/thumbnail', FRESH],
    ['PUT', '/api/simulations/abc-123/website-manifest', FRESH], ['POST', '/api/simulations/abc-123/world/begin', FRESH],
    ['POST', '/api/simulations/abc-123/submit', FRESH], ['POST', '/api/simulations/abc-123/appeal', FRESH],
    ['POST', '/api/simulations/abc-123/live', FRESH], ['DELETE', '/api/simulations/abc-123/live', FRESH], ['POST', '/api/simulations/abc-123/live/admit', FRESH],
    // session: the cheap check is enough
    ['POST', '/api/auth/logout', SESSION], ['PUT', '/api/avatar', SESSION], ['POST', '/api/social/follow', SESSION],
    ['POST', '/api/identity/ticket', SESSION], ['POST', '/api/tickets/checkout', SESSION], ['POST', '/api/stripe/checkout', SESSION],
    ['POST', '/api/simulations/abc-123/play', SESSION], ['POST', '/api/delta/checkout', SESSION],
  ];
  for (const [method, p, klass] of expected) {
    const r = routePolicy(method, p);
    assert.equal(r.class, klass, `${method} ${p} is ${r.class} (${r.pattern}), not ${klass}`);
    assert.equal(r.explicit, true, `${method} ${p} is named`);
  }
});

// ── Every path in the source is named ────────────────────────────────────────

const FILL = 'a1b2c3d4e5f6a1b2c3d4e5f6';

/// The concrete paths a regex source stands for: classes become a token, optional
/// groups are taken both ways, alternations each way.
function expand(src) {
  const open = src.indexOf('(');
  if (open < 0) return [concrete(src)];
  const lead = concrete(src.slice(0, open));
  let depth = 0;
  let close = open;
  for (; close < src.length; close++) {
    if (src[close] === '\\') { close++; continue; }
    if (src[close] === '(') depth++;
    else if (src[close] === ')' && --depth === 0) break;
  }
  const inner = src.slice(open + 1, close);
  const optional = src[close + 1] === '?';
  const rest = src.slice(close + (optional ? 2 : 1));
  const alts = [];
  let d = 0;
  let from = 0;
  for (let i = 0; i < inner.length; i++) {
    if (inner[i] === '\\') { i++; continue; }
    if (inner[i] === '(') d++;
    else if (inner[i] === ')') d--;
    else if (inner[i] === '|' && d === 0) { alts.push(inner.slice(from, i)); from = i + 1; }
  }
  alts.push(inner.slice(from));
  const heads = alts.flatMap((a) => expand(a));
  if (optional) heads.push('');
  const tails = expand(rest);
  return heads.flatMap((h) => tails.map((t) => lead + h + t));
}
function concrete(s) {
  return s
    .replace(/\[[^\]]*\](\{[^}]*\}|\+|\*)/g, FILL)
    .replace(/\[([^\]^])[^\]]*\]/g, '$1')
    .split('\\/').join('/');
}

function pathsInSource() {
  const found = new Map();
  const note = (p, where) => { if (!found.has(p)) found.set(p, where); };
  for (const file of fs.readdirSync(SRC).filter((f) => /\.(mjs|js)$/.test(f))) {
    const text = fs.readFileSync(path.join(SRC, file), 'utf8');
    for (const m of text.matchAll(/(['"`])(\/api\/[A-Za-z0-9_./{}:*-]*)\1/g)) {
      const raw = m[2];
      // A prefix that dispatches (ends in "/" or "*") is not a route.
      if (raw.includes('$') || raw.endsWith('/') || raw.includes('*')) continue;
      note(raw.split('{id}').join('abc-123'), file);
    }
    for (const m of text.matchAll(/\/\^(\\\/api\\\/[^$]*)\$\//g)) {
      for (const p of expand(m[1])) note(p, `${file} (regex)`);
    }
  }
  return found;
}

test('the scan finds the paths it should (so a pass means something)', () => {
  const found = pathsInSource();
  for (const p of ['/api/auth/register', '/api/keys', '/api/stripe/connect/onboard', '/api/simulations/publish', '/api/delta/charge', '/api/account/email/start']) {
    assert.ok(found.has(p), `${p} is found`);
  }
  assert.ok([...found.keys()].some((p) => /^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/part$/.test(p)), 'a regex route is expanded');
  assert.ok([...found.keys()].some((p) => p.endsWith('/world/begin')), 'an alternation is expanded');
  assert.ok([...found.keys()].some((p) => /\/purchases\/pur_[A-Za-z0-9]+\/refund$/.test(p)), 'an optional group is taken both ways');
  assert.ok(found.size > 120, `${found.size} paths`);
});

test('every path in the Worker source is named in the table: a new route has to be classed', () => {
  const unnamed = [];
  for (const [p, where] of pathsInSource()) {
    if (!routePolicy('POST', p).explicit) unnamed.push(`${p}  (${where})`);
  }
  assert.deepEqual(unnamed, [], `these paths are in the source and not in src/route_policy.mjs:\n  ${unnamed.join('\n  ')}`);
});

// ── The gate ─────────────────────────────────────────────────────────────────

const worker = await loadWorker();
const ACCOUNT = '99999999-9999-4999-8999-999999999991';
const ORIGIN = 'https://api.eustress.dev';

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

async function environment(extra = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, ...extra };
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

async function send(env, method, route, { token, body } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method, headers, body: method === 'GET' ? undefined : JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

/// The object ends every session, but the KV mirror has not caught up yet.
async function endSessionsInTheObjectOnly(env) {
  const r = await accountCall(env, ACCOUNT, 'bump_floor', { reason: 'test' });
  assert.equal(r.ok, true);
  await env.USERS.delete(`authfloor:${ACCOUNT}`);
}

test('a session the object has ended is refused on a fresh route while the KV mirror still lets it through the cheap one', async () => {
  const { env } = await environment();
  const token = mintToken(ACCOUNT);
  await endSessionsInTheObjectOnly(env);

  const cheap = await send(env, 'POST', '/api/social/follow', { token, body: {} });
  assert.notEqual(cheap.status, 401, 'the cheap check is still fooled by the lagging mirror');

  for (const [method, route] of [['POST', '/api/keys'], ['POST', '/api/account/email/start'], ['POST', '/api/simulations/publish'], ['POST', '/api/ledger/spend']]) {
    const fresh = await send(env, method, route, { token, body: {} });
    assert.equal(fresh.status, 401, `${route}`);
    assert.equal(fresh.body.code, 'session_revoked', route);
  }
});

test('a fresh route accepts a current session, and reads are not gated', async () => {
  const { env } = await environment();
  const token = mintToken(ACCOUNT);
  const keys = await send(env, 'POST', '/api/keys', { token, body: { name: 'ci' } });
  assert.notEqual(keys.status, 401, JSON.stringify(keys.body));
  assert.notEqual(keys.body?.code, 'session_revoked');
  await endSessionsInTheObjectOnly(env);
  const read = await send(env, 'GET', '/api/keys', { token });
  assert.notEqual(read.body?.code, 'session_revoked', 'a read is not gated');
});

test('a request with no session reaches the route, which answers for itself', async () => {
  const { env } = await environment();
  const none = await send(env, 'POST', '/api/ledger/spend', { body: {} });
  assert.equal(none.status, 401, JSON.stringify(none.body));
  assert.equal(none.body.error, 'Unauthorized', 'the route answered in its own words: the gate stood aside');
  assert.notEqual(none.body?.code, 'session_revoked');
  assert.notEqual(none.body?.code, 'account_frozen');
  const apiKey = await send(env, 'POST', '/api/commerce/purchases', { token: 'sk_test_notAJwtAtAll', body: {} });
  assert.notEqual(apiKey.body?.code, 'session_revoked', 'an API key is not a session');
  const junk = await send(env, 'POST', '/api/keys', { token: 'not.a.token', body: {} });
  assert.equal(junk.status, 401);
});

test('a public route is not gated at all', async () => {
  const { env } = await environment();
  await endSessionsInTheObjectOnly(env);
  const r = await send(env, 'POST', '/api/auth/challenge', { token: mintToken(ACCOUNT), body: { public_key: 'a'.repeat(64) } });
  assert.notEqual(r.body?.code, 'session_revoked');
});

test('a fresh route is refused, not let through, when the object cannot be asked; a session route is not affected', async () => {
  const { env } = await environment();
  const token = mintToken(ACCOUNT);
  env.ACCOUNT_AUTH = { idFromName: (n) => n, get() { throw new Error('the object is down'); } };
  const fresh = await send(env, 'POST', '/api/keys', { token, body: {} });
  assert.equal(fresh.status, 503);
  assert.equal(fresh.body.code, 'account_unavailable');
  const cheap = await send(env, 'POST', '/api/social/follow', { token, body: {} });
  assert.notEqual(cheap.status, 503);
});

test('a deployment without account objects keeps the cheap check, and an account the object does not know is left to the route', async () => {
  const { env } = await environment();
  const token = mintToken(ACCOUNT);
  const bare = { ...env, ACCOUNT_AUTH: undefined };
  const r = await send(bare, 'POST', '/api/keys', { token, body: {} });
  assert.notEqual(r.body?.code, 'account_unavailable');
  assert.notEqual(r.body?.code, 'session_revoked');

  const ghost = mintToken('99999999-9999-4999-8999-999999999992');
  const g = await send(env, 'POST', '/api/keys', { token: ghost, body: {} });
  assert.notEqual(g.status, 503, 'the gate does not stand in the way of the route');
  assert.notEqual(g.body?.code, 'account_unavailable');
  assert.notEqual(g.body?.code, 'session_revoked');
});

// ── The value freeze ─────────────────────────────────────────────────────────

async function rotateAccount(env, key) {
  const begun = await send(env, 'POST', '/api/identity/rotate/begin', { token: mintToken(ACCOUNT) });
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: key.hex, newKey: newKey.hex, challenge: begun.body.challenge });
  const r = await send(env, 'POST', '/api/identity/rotate', { token: mintToken(ACCOUNT), body: { new_public_key: newKey.hex, challenge: begun.body.challenge, old_signature: key.sign(m), new_signature: newKey.sign(m) } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

test('after a key replacement a value route is refused with the date, and a fresh route is not', async () => {
  const { env, key } = await environment();
  const rotated = await rotateAccount(env, key);
  const token = rotated.token;
  for (const [method, route] of [['POST', '/api/ledger/spend'], ['POST', '/api/stripe/connect/onboard'], ['POST', '/api/commerce/purchases']]) {
    const r = await send(env, method, route, { token, body: {} });
    assert.equal(r.status, 423, route);
    assert.equal(r.body.code, 'account_frozen', route);
    assert.equal(r.body.until, new Date(rotated.value_frozen_until).toISOString(), route);
  }
  const fresh = await send(env, 'POST', '/api/keys', { token, body: { name: 'after' } });
  assert.notEqual(fresh.status, 423, 'a fresh route is not a value route');
  const read = await send(env, 'GET', '/api/ledger/me', { token });
  assert.notEqual(read.status, 423);
});

test('the freeze ends on its own, and an account that never replaced its key is never frozen', async () => {
  const { env, key } = await environment();
  const never = await send(env, 'POST', '/api/ledger/spend', { token: mintToken(ACCOUNT), body: {} });
  assert.notEqual(never.status, 423);

  const rotated = await rotateAccount(env, key);
  const realNow = Date.now;
  Date.now = () => realNow() + FREEZE_WITHOUT_VALUE_MS + 60 * 1000;
  try {
    // Mint a token for that later time: the object's floor stays well behind it.
    const later = mintToken(ACCOUNT);
    const r = await send(env, 'POST', '/api/ledger/spend', { token: later, body: {} });
    assert.notEqual(r.status, 423, JSON.stringify(r.body));
    assert.notEqual(r.body?.code, 'account_frozen');
  } finally {
    Date.now = realNow;
  }
  assert.ok(rotated.value_frozen_until > Date.now());
});

test('code that moves value with no session reads the freeze from the KV mirror the object keeps', async () => {
  const { env, key } = await environment();
  assert.equal(await valueFrozenUntil(env, ACCOUNT), 0, 'never frozen');
  const rotated = await rotateAccount(env, key);
  assert.equal(await env.USERS.get(`valuefreeze:${ACCOUNT}`), String(rotated.value_frozen_until));
  assert.equal(await valueFrozenUntil(env, ACCOUNT), rotated.value_frozen_until);
  await env.USERS.put(`valuefreeze:${ACCOUNT}`, String(Date.now() - 1000));
  assert.equal(await valueFrozenUntil(env, ACCOUNT), 0, 'a freeze that has ended is not one');
  const down = { USERS: { get: async () => { throw new Error('KV unavailable'); } } };
  await assert.rejects(() => valueFrozenUntil(down, ACCOUNT), /KV unavailable/, 'unreadable is an error, never "not frozen"');
});

test('the address routes ask the object even when a session reaches them by the public cancel route', async () => {
  const { env } = await environment();
  const token = mintToken(ACCOUNT);
  await endSessionsInTheObjectOnly(env);
  const cancel = await send(env, 'POST', '/api/account/email/cancel', { token, body: {} });
  assert.equal(cancel.status, 401, 'a session the object ended cannot cancel');
  assert.notEqual(cancel.body?.code, 'no_pending');
});

test('only the strict classes may use a prefix: a route added under a public or session family is unnamed, so it is fresh and fails the scan', () => {
  for (const [pattern, klass] of RULES) {
    if (typeof pattern === 'string' && pattern.endsWith('*')) {
      assert.ok(klass === FRESH || klass === VALUE, `${pattern} is ${klass}`);
    }
  }
  assert.equal(routePolicy('POST', '/api/commerce/something-new').class, FRESH);
  assert.equal(routePolicy('POST', '/api/social/something-new').class, FRESH);
  assert.equal(routePolicy('POST', '/api/kyc/something-new').class, FRESH);
  assert.equal(routePolicy('POST', '/api/guest/something-new').class, FRESH);
  assert.equal(routePolicy('POST', '/api/tickets/something-new').class, FRESH);
});
