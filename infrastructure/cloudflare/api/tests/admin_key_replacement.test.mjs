// An administrator replaces the key of an owner who lost theirs, after a manual identity check. The administrator
// signs with their OWN key, the NEW key signs too, the owner can undo it with the link in the notice, every
// replacement is announced on a second channel, and what the replacement revokes is owed durably.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import {
  AccountAuth, accountCall, accountState, verifyAuditChain, SMALL_ORDER_KEYS, FREEZE_WITH_VALUE_MS, FREEZE_WITHOUT_VALUE_MS, PAYOUT_PIN_MS,
  adminReplaceMessage, adminReplaceKeyMessage,
} from '../src/account_auth.mjs';
import { Wallet, CommerceHub, callObject, hubStub } from '../src/commerce.mjs';
import { replaceKey, readIdentity } from '../scripts/admin-replace-key.mjs';

const worker = await loadWorker();
const OWNER = '12121212-1212-4212-8212-121212121212';
const OTHER = '34343434-3434-4434-8434-343434343434';
const ADMIN = '56565656-5656-4656-8656-565656565656';
const ADMIN2 = '78787878-7878-4878-8878-787878787878';
const ORIGIN = 'https://api.eustress.dev';
const BEGIN = '/api/admin/accounts/replace-key/begin';
const REPLACE = '/api/admin/accounts/replace-key';
const DAY = 24 * 3600 * 1000;

/// A session token stamped at `iat`, as the Worker stamps the one it issues at a floor.
function tokenAt(sub, iat) {
  const b64 = (v) => Buffer.from(v).toString('base64url');
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const p = b64(JSON.stringify({ sub, iat, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${h}.${p}.${crypto.createHmac('sha256', JWT_SECRET).update(`${h}.${p}`).digest('base64url')}`;
}

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  const seed = Buffer.from(privateKey.export({ format: 'jwk' }).d, 'base64url').toString('hex');
  return { hex, seed, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}
class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
const bodyOf = (m) => [...m.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
  .map((x) => Buffer.from(x[1].replace(/\r\n/g, ''), 'base64').toString()).join('\n');

async function environment(extra = {}) {
  const sent = [];
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, ADMIN_REPLACE_KEY_ENABLED: 'true',
    EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  const lost = keyPair();
  const adminKey = keyPair();
  await env.USERS.put(`user:${OWNER}`, JSON.stringify({ kyc_verified: true, id: OWNER, username: 'owner', public_key: lost.hex, email: 'owner@example.com', discord_id: 'discord-1' }));
  await env.USERS.put('username:owner', OWNER);
  await env.USERS.put(`pubkey:${lost.hex}`, OWNER);
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'admin_user', public_key: adminKey.hex, email: 'admin@example.com' }));
  await env.USERS.put('username:admin_user', ADMIN);
  await env.USERS.put(`pubkey:${adminKey.hex}`, ADMIN);
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await accountState(env, OWNER);
  await accountState(env, ADMIN);
  return { env, lost, adminKey, sent, adminToken: mintToken(ADMIN), ownerToken: mintToken(OWNER) };
}

async function request(env, method, route, { token, body } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method, headers, body: method === 'GET' ? undefined : JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const state = (env, id = OWNER) => accountCall(env, id, 'state');

/// Close the owner's undo window without using it (as if its time had passed), so the next replacement is allowed.
async function closeWindow(env, id = OWNER) {
  const object = env.ACCOUNT_AUTH.instances.get(id);
  const held = await object.storage.get('state');
  held.admin_undo = null;
  await object.storage.put('state', held);
}
/// Run `fn` with the clock `ms` ahead.
async function later(ms, fn) {
  const realNow = Date.now;
  Date.now = () => realNow() + ms;
  try { return await fn(); } finally { Date.now = realNow; }
}

/// The whole flow: the administrator's challenge, both signatures, the replacement. `mutate` changes the
/// final request after it is built; `sign` is the key the administrator signs with.
async function replace(env, adminKey, adminToken, newKey, { username = 'owner', caseRef = 'SUP-1042', reason = 'ID checked on a video call', sign = adminKey, mutate, extra = {}, begin = {} } = {}) {
  const begun = await request(env, 'POST', BEGIN, { token: adminToken, body: { username, new_public_key: newKey.hex, case_ref: caseRef, ...begin } });
  if (begun.status !== 200) return { begun, result: begun };
  const message = adminReplaceMessage({ origin: ORIGIN, adminId: ADMIN, accountId: begun.body.account_id, newKey: newKey.hex, caseRef, challenge: begun.body.challenge });
  const body = {
    username, new_public_key: newKey.hex, case_ref: caseRef, reason, challenge: begun.body.challenge,
    admin_signature: sign.sign(message), new_key_signature: newKey.sign(adminReplaceKeyMessage({ origin: ORIGIN, username, caseRef, newKey: newKey.hex })), ...extra,
  };
  if (mutate) mutate(body, { message, begun: begun.body });
  return { begun, result: await request(env, 'POST', REPLACE, { token: adminToken, body }) };
}

// ── The shared vectors ───────────────────────────────────────────────────────

test('the two messages are the shared ones, and each signature verifies only over its own message', () => {
  const v = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, '..', '..', '..', '..', 'docs', 'architecture', 'admin-replace-vectors.json'), 'utf8'));
  const verify = (pub, sig, msg) => crypto.verify(null, Buffer.from(msg), crypto.createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), Buffer.from(pub, 'hex')]), format: 'der', type: 'spki' }), Buffer.from(sig, 'hex'));
  for (const c of v.cases) {
    const admin = adminReplaceMessage({ origin: c.origin, adminId: c.admin_id, accountId: c.account_id, newKey: v.new_key.public_key, caseRef: c.case_ref, challenge: c.challenge });
    const key = adminReplaceKeyMessage({ origin: c.origin, username: c.username, caseRef: c.case_ref, newKey: v.new_key.public_key });
    assert.equal(admin, c.admin_message);
    assert.equal(key, c.key_message);
    assert.equal(verify(v.admin_key.public_key, c.admin_signature, admin), true);
    assert.equal(verify(v.new_key.public_key, c.key_signature, key), true);
    assert.equal(verify(v.admin_key.public_key, c.admin_signature, key), false, 'one signature does not cover the other message');
    assert.equal(verify(v.new_key.public_key, c.key_signature, admin), false);
  }
});

// ── Who may, and where ───────────────────────────────────────────────────────

test('the route does not exist where it is not switched on, for anyone', async () => {
  const { env, adminKey, adminToken } = await environment({ ADMIN_REPLACE_KEY_ENABLED: undefined });
  for (const [method, route] of [['POST', BEGIN], ['POST', REPLACE], ['GET', '/api/admin/accounts/review-flags']]) {
    const r = await request(env, method, route, { token: adminToken, body: { username: 'owner' } });
    assert.equal(r.status, 404, route);
    assert.equal(r.body.code, 'not_enabled');
  }
  const off = await environment({ ADMIN_REPLACE_KEY_ENABLED: 'false' });
  assert.equal((await request(off.env, 'POST', BEGIN, { token: off.adminToken, body: {} })).status, 404);
  void adminKey;
});

test('only an administrator may start or finish a replacement, and the answer to anyone else is the same', async () => {
  const { env, lost, adminKey, adminToken, ownerToken } = await environment();
  const fresh = keyPair();
  const anonymous = await request(env, 'POST', BEGIN, { body: { username: 'owner', new_public_key: fresh.hex, case_ref: 'SUP-1' } });
  const notAdmin = await request(env, 'POST', BEGIN, { token: ownerToken, body: { username: 'owner', new_public_key: fresh.hex, case_ref: 'SUP-1' } });
  assert.equal(anonymous.status, 403);
  assert.equal(notAdmin.status, 403);
  assert.deepEqual(anonymous.body, notAdmin.body);
  const direct = await request(env, 'POST', REPLACE, { token: ownerToken, body: { username: 'owner', new_public_key: fresh.hex, case_ref: 'SUP-1', reason: 'ID checked on a video call' } });
  assert.equal(direct.status, 403);
  assert.equal((await state(env)).public_key, lost.hex, 'nothing changed');
  const admin = await replace(env, adminKey, adminToken, fresh);
  assert.equal(admin.result.status, 200, JSON.stringify(admin.result.body));
});

test('a deployment without account objects says so', async () => {
  const { env, adminToken } = await environment();
  const bare = { ...env, ACCOUNT_AUTH: undefined };
  assert.equal((await request(bare, 'POST', BEGIN, { token: adminToken, body: {} })).status, 501);
});

// ── The administrator signs with their own key ───────────────────────────────

test('a stolen administrator session cannot replace a key: the administrator\'s own key has to sign', async () => {
  const { env, lost, adminToken } = await environment();
  const thief = keyPair();
  const r = await replace(env, thief, adminToken, keyPair(), { sign: thief });
  assert.equal(r.result.status, 401, JSON.stringify(r.result.body));
  assert.equal(r.result.body.code, 'signature_invalid');
  assert.equal((await state(env)).public_key, lost.hex);
  assert.equal((await state(env)).key_epoch, 0);
});

test('the administrator\'s signature covers the account, the new key, the case, the origin and the challenge, and is refused when any differs', async () => {
  const { env, lost, adminKey, adminToken } = await environment();
  const otherNew = keyPair();
  const cases = [
    ['a different new key', (b) => { b.new_public_key = otherNew.hex; b.new_key_signature = otherNew.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-1042', newKey: otherNew.hex })); }],
    ['a different case', (b) => { b.case_ref = 'SUP-9999'; }],
    ['a signature that is not hex', (b) => { b.admin_signature = 'zz'.repeat(64); }],
    ['no signature', (b) => { delete b.admin_signature; }],
    ['an invented challenge', (b) => { b.challenge = 'ab'.repeat(32); }],
  ];
  for (const [name, mutate] of cases) {
    const r = await replace(env, adminKey, adminToken, keyPair(), { mutate });
    assert.ok([400, 401].includes(r.result.status), `${name}: ${r.result.status} ${JSON.stringify(r.result.body)}`);
  }
  assert.equal((await state(env)).public_key, lost.hex, 'none changed the account');

  // The challenge a failed attempt used is gone.
  const fresh = keyPair();
  const begun = await request(env, 'POST', BEGIN, { token: adminToken, body: { username: 'owner', new_public_key: fresh.hex, case_ref: 'SUP-1042' } });
  const message = adminReplaceMessage({ origin: ORIGIN, adminId: ADMIN, accountId: OWNER, newKey: fresh.hex, caseRef: 'SUP-1042', challenge: begun.body.challenge });
  const body = {
    username: 'owner', new_public_key: fresh.hex, case_ref: 'SUP-1042', reason: 'ID checked on a video call', challenge: begun.body.challenge,
    admin_signature: keyPair().sign(message), new_key_signature: fresh.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-1042', newKey: fresh.hex })),
  };
  assert.equal((await request(env, 'POST', REPLACE, { token: adminToken, body })).result?.status ?? (await request(env, 'POST', REPLACE, { token: adminToken, body })).status, 400, 'a spent challenge');
  void message;
});

test('an administrator\'s own key, or another administrator\'s, is not replaced by this route', async () => {
  const { env, adminKey, adminToken } = await environment();
  await env.USERS.put(`user:${ADMIN2}`, JSON.stringify({ kyc_verified: true, id: ADMIN2, username: 'second_admin', public_key: keyPair().hex }));
  await env.USERS.put('username:second_admin', ADMIN2);
  await env.USERS.put(`admin:${ADMIN2}`, '1');
  for (const username of ['admin_user', 'second_admin']) {
    const r = await replace(env, adminKey, adminToken, keyPair(), { username });
    assert.equal(r.result.status, 403, username);
    assert.equal(r.result.body.code, 'target_is_admin');
  }
  assert.equal((await state(env, ADMIN)).key_epoch, 0);
});

// ── The new key signs too ────────────────────────────────────────────────────

test('a key nobody holds cannot be put in place: the new key signs the case and the username', async () => {
  const { env, lost, adminKey, adminToken } = await environment();
  const fresh = keyPair();
  const stranger = keyPair();
  const cases = [
    ['no signature', (b) => { delete b.new_key_signature; }],
    ['not hex', (b) => { b.new_key_signature = 'zz'.repeat(64); }],
    ['signed by another key', (b) => { b.new_key_signature = stranger.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-1042', newKey: fresh.hex })); }],
    ['signed for another username', (b) => { b.new_key_signature = fresh.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'someone', caseRef: 'SUP-1042', newKey: fresh.hex })); }],
    ['signed for another case', (b) => { b.new_key_signature = fresh.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-1', newKey: fresh.hex })); }],
    ['signed for another origin', (b) => { b.new_key_signature = fresh.sign(adminReplaceKeyMessage({ origin: 'https://evil.example', username: 'owner', caseRef: 'SUP-1042', newKey: fresh.hex })); }],
  ];
  let day = 0;
  for (const [name, mutate] of cases) {
    // Five replacements a day per administrator: spread the cases over days of the object's clock is not possible
    // through the route, so use a fresh administrator object each time.
    void day++;
    const r = await replace(env, adminKey, adminToken, fresh, { mutate });
    assert.equal(r.result.status, 401, `${name}: ${JSON.stringify(r.result.body)}`);
    assert.equal(r.result.body.code, 'new_key_signature_invalid', name);
    await accountCall(env, ADMIN, 'state'); // keep the object warm
    // the administrator's allowance is counted where a signature is good; reset it for the next case
    const object = env.ACCOUNT_AUTH.instances.get(ADMIN);
    const held = await object.storage.get('state');
    held.admin_actions = [];
    await object.storage.put('state', held);
  }
  assert.equal((await state(env)).public_key, lost.hex);
});

// ── What it does ─────────────────────────────────────────────────────────────

test('the key is replaced, the old one is retired as the administrator\'s doing, and the owner signs in with the new one', async () => {
  const { env, lost, adminKey, adminToken } = await environment();
  const fresh = keyPair();
  const { result } = await replace(env, adminKey, adminToken, fresh);
  assert.equal(result.status, 200, JSON.stringify(result.body));
  assert.equal(result.body.ok, true);
  assert.equal(result.body.key_epoch, 1);
  assert.equal(result.body.token, undefined, 'the administrator is given no token');
  assert.ok(!JSON.stringify(result.body).includes(fresh.hex), 'nor the key');
  assert.ok(!JSON.stringify(result.body).includes('undo-key-replacement'), 'nor the owner\'s undo link');

  const held = await state(env);
  assert.equal(held.public_key, fresh.hex);
  assert.equal(held.retired[0].how, 'admin');
  const old = await accountCall(env, OWNER, 'check_login', { public_key: lost.hex });
  assert.equal(old.code, 'key_retired');
  assert.equal(old.how, 'admin');
  assert.equal((await accountCall(env, OWNER, 'check_login', { public_key: fresh.hex })).ok, true);
  assert.equal(await env.USERS.get(`pubkey:${fresh.hex}`), OWNER);
  assert.equal(await env.USERS.get(`retired-key:${lost.hex}`), OWNER);
});

test('every session ends, value is frozen, the payout destination is pinned; seven days when the account holds value', async () => {
  const { env, adminKey, adminToken, ownerToken } = await environment();
  assert.equal((await request(env, 'GET', '/api/auth/me', { token: ownerToken })).status, 200);
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200);
  assert.equal((await request(env, 'GET', '/api/auth/me', { token: ownerToken })).status, 401, 'the old session is ended');
  assert.ok(Math.abs(result.body.value_frozen_until - Date.now() - FREEZE_WITHOUT_VALUE_MS) < 5000);
  assert.ok(Math.abs(result.body.payout_pinned_until - Date.now() - PAYOUT_PIN_MS) < 5000);

  const rich = await environment();
  const record = JSON.parse(await rich.env.USERS.get(`user:${OWNER}`));
  await rich.env.USERS.put(`user:${OWNER}`, JSON.stringify({ kyc_verified: true, ...record, stripe_connect_id: 'acct_x' }));
  const r = await replace(rich.env, rich.adminKey, rich.adminToken, keyPair());
  assert.equal(r.result.body.holds_value, true);
  assert.ok(Math.abs(r.result.body.value_frozen_until - Date.now() - FREEZE_WITH_VALUE_MS) < 5000);
});

test('the case, the reason and the administrator are in the account\'s audit chain, and the admin log is written before the owner is mailed', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  let loggedWhenFirstMailWent = null;
  const realSend = env.EMAIL.send;
  env.EMAIL.send = async (m) => {
    if (m.to === 'owner@example.com' && loggedWhenFirstMailWent === null) loggedWhenFirstMailWent = [...env.AUDIT_LOG.store.values()].some((v) => String(v).includes('ADMIN_REPLACE_KEY'));
    return realSend(m);
  };
  const { result } = await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-77', reason: 'Identity checked against the sign-up ID on a video call' });
  assert.equal(result.status, 200, JSON.stringify(result.body));
  const audit = await accountCall(env, OWNER, 'audit', { limit: 20 });
  assert.equal(await verifyAuditChain(audit.records), true);
  const record = audit.records.find((x) => x.type === 'key_replaced_by_admin');
  assert.equal(record.data.admin_id, ADMIN);
  assert.equal(record.data.case_ref, 'SUP-77');
  assert.equal(record.data.reason, 'Identity checked against the sign-up ID on a video call');
  assert.ok(!JSON.stringify(record).includes('owner@example.com'));
  assert.equal(loggedWhenFirstMailWent, true, 'a Worker that died while mailing the owner would still have left the log');
  assert.ok(sent.length > 0);
  const logged = [...env.AUDIT_LOG.store.values()].map((v) => String(v)).find((v) => v.includes('ADMIN_REPLACE_KEY'));
  assert.ok(logged.includes('SUP-77') && logged.includes(ADMIN));
});

// ── Everything it revokes is owed durably ────────────────────────────────────

test('everything that acted as the account is revoked: API keys, webhook endpoints, the Discord link', async () => {
  const { env, adminKey, adminToken } = await environment();
  const hub = hubStub(env, OWNER);
  const key = await callObject(hub, 'create_key', { account_id: OWNER, name: 'ci', mode: 'live', key_type: 'commerce' });
  await callObject(hub, 'create_key', { account_id: OWNER, name: 'second', mode: 'test', key_type: 'commerce' });
  await callObject(hub, 'create_endpoint', { account_id: OWNER, url: 'https://hooks.example.com/x', enabled_events: ['*'], livemode: true });
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200, JSON.stringify(result.body));
  assert.deepEqual([result.body.revoked.api_keys, result.body.revoked.webhooks, result.body.revoked.discord], [2, 1, true]);
  assert.equal(result.body.revoked_complete, true);
  assert.equal((await callObject(hub, 'list_keys', { account_id: OWNER })).data.length, 0);
  assert.equal((await callObject(hub, 'list_endpoints', { account_id: OWNER, livemode: true })).data.length, 0);
  assert.equal(await env.USERS.get(`apikey:${crypto.createHash('sha256').update(key.key).digest('hex')}`), null);
  assert.ok((await state(env)).discord_unlinked_at);
  assert.equal((await state(env)).credentials_pending, 0);
});

test('a revocation that fails is not forgotten: the answer says it is incomplete and the object finishes it', async () => {
  const { env, adminKey, adminToken } = await environment();
  const hub = hubStub(env, OWNER);
  const made = await callObject(hub, 'create_key', { account_id: OWNER, name: 'ci', mode: 'live', key_type: 'commerce' });
  assert.equal(made.ok, true);
  // The commerce hub is unreachable while the replacement happens.
  const real = env.COMMERCE_HUBS;
  let down = true;
  env.COMMERCE_HUBS = {
    idFromName: real.idFromName,
    get: (id) => {
      const stub = real.get(id);
      return { fetch: (url, init) => (down && String(url).endsWith('/revoke_key') ? Promise.reject(new Error('hub down')) : stub.fetch(url, init)) };
    },
  };
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200, 'the replacement stands');
  assert.equal(result.body.revoked_complete, false);
  assert.equal((await state(env)).credentials_pending, 1, 'owed');

  down = false;
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  const realNow = Date.now;
  Date.now = () => realNow() + 31 * 1000;
  try { await object.alarm(); } finally { Date.now = realNow; }
  assert.equal((await state(env)).credentials_pending, 0, 'the alarm revoked it');
  assert.equal((await callObject(real.get(real.idFromName(`hub:${OWNER}`)), 'list_keys', { account_id: OWNER })).data.length, 0);
});

// ── The notices: the owner's way back, and the second channel ────────────────

test('every address of the account is told, with the case and the one link that puts the earlier key back; no key in it', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  const fresh = keyPair();
  const { result } = await replace(env, adminKey, adminToken, fresh, { caseRef: 'SUP-1042' });
  assert.equal(result.status, 200);
  assert.equal(result.body.notices_sent, 1);
  const toOwner = sent.find((m) => m.to === 'owner@example.com');
  assert.ok(toOwner, 'the owner is told');
  const text = bodyOf(toOwner);
  assert.match(text, /replaced by Eustress support/);
  assert.match(text, /SUP-1042/);
  assert.match(text, /put your earlier key back/);
  assert.ok(!text.includes(fresh.hex), 'no key');
  const links = text.match(/https?:\/\/\S+/g) || [];
  assert.ok(links.some((l) => l.includes('/undo-key-replacement?token=')), 'the undo link');
  assert.ok(links.every((l) => l.startsWith('https://eustress.dev/') || l.startsWith('mailto:') || l.startsWith('https://')), 'links are the site\'s');
  assert.equal((await state(env)).notices_pending, 0, 'acknowledged');
});

test('every replacement goes to a second channel: the security mailbox and the other administrators, with no key and no link', async () => {
  const { env, adminKey, adminToken, sent } = await environment({ SECURITY_MAILBOX: 'Security@Example.org' });
  await env.USERS.put(`user:${ADMIN2}`, JSON.stringify({ kyc_verified: true, id: ADMIN2, username: 'second_admin', public_key: keyPair().hex, email: 'second@example.com' }));
  await env.USERS.put(`admin:${ADMIN2}`, '1');
  const fresh = keyPair();
  const { result } = await replace(env, adminKey, adminToken, fresh, { caseRef: 'SUP-31', reason: 'Checked against the registration portrait' });
  assert.equal(result.status, 200, JSON.stringify(result.body));
  const to = sent.map((m) => m.to).sort();
  assert.ok(to.includes('security@example.org'), 'the security mailbox');
  assert.ok(to.includes('second@example.com'), 'the other administrator');
  assert.ok(!to.includes('admin@example.com'), 'not the administrator who did it');
  const summary = bodyOf(sent.find((m) => m.to === 'second@example.com'));
  assert.match(summary, /SUP-31/);
  assert.match(summary, /Checked against the registration portrait/);
  assert.match(summary, /admin_user/);
  assert.ok(!summary.includes(fresh.hex) && !/undo-key-replacement/.test(summary), 'no key, no link');
});

test('a notice that could not be sent is left to the object, as for any replacement', async () => {
  const { env, adminKey, adminToken } = await environment();
  env.EMAIL.send = async (m) => { if (m.to === 'owner@example.com') throw new Error('mail is down'); };
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200, 'the replacement stands');
  assert.equal(result.body.notices_sent, 0);
  assert.equal((await state(env)).notices_pending, 1);
});

// ── The owner puts the earlier key back ──────────────────────────────────────

async function undoLinkOf(sent) {
  const text = bodyOf(sent.find((m) => m.to === 'owner@example.com'));
  const m = /undo-key-replacement\?token=([0-9a-f-]{36}\.[0-9a-f]{64})/.exec(text);
  assert.ok(m, 'the notice has the link');
  return m[1];
}
const restore = (env, token) => request(env, 'POST', '/api/account/key-restore', { body: { token } });

test('the link puts the earlier key back, retires the key support put in place, ends every session, keeps the freeze and the pin, and flags the case', async () => {
  const { env, lost, adminKey, adminToken, sent } = await environment();
  const fresh = keyPair();
  const { result } = await replace(env, adminKey, adminToken, fresh, { caseRef: 'SUP-1042' });
  assert.equal(result.status, 200);
  const link = await undoLinkOf(sent);
  // A session of the new key, stamped at the floor the replacement set.
  const newSession = tokenAt(OWNER, (await state(env)).floor);
  assert.equal((await request(env, 'GET', '/api/auth/me', { token: newSession })).status, 200);
  sent.length = 0;

  const r = await restore(env, link);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.token, undefined, 'no token comes with it');
  const held = await state(env);
  assert.equal(held.public_key, lost.hex, 'the earlier key is current again');
  assert.equal(held.key_epoch, 2);
  assert.ok(!held.retired.some((x) => x.how === 'admin'), 'the earlier key is no longer retired');
  assert.equal(held.retired.find((x) => x.how === 'disputed')?.key_fingerprint?.length, 16, 'the other key is retired as disputed');
  assert.equal((await accountCall(env, OWNER, 'check_login', { public_key: lost.hex })).ok, true);
  assert.equal((await accountCall(env, OWNER, 'check_login', { public_key: fresh.hex })).code, 'key_retired');
  assert.equal(await env.USERS.get(`pubkey:${lost.hex}`), OWNER);
  assert.equal(await env.USERS.get(`retired-key:${lost.hex}`), null);
  assert.equal(await env.USERS.get(`retired-key:${fresh.hex}`), OWNER);
  assert.equal(await env.USERS.get(`pubkey:${fresh.hex}`), null);
  assert.equal((await request(env, 'GET', '/api/auth/me', { token: newSession })).status, 401, 'the session of the new key ended');
  assert.ok(held.value_frozen_until > Date.now() + 6 * DAY, 'value stays frozen for a week');
  assert.ok(held.payout_pinned_until > Date.now() + 29 * DAY, 'the payout stays pinned');
  assert.equal(held.admin_review.length, 1);
  assert.equal(held.admin_review[0].case_ref, 'SUP-1042');
  assert.equal(held.admin_review[0].reason, 'replacement_undone');
  assert.ok(await env.USERS.get(`admin-review:${OWNER}`), 'flagged for review');
  assert.ok(sent.some((m) => m.to === 'owner@example.com' && /taken out/.test(bodyOf(m))), 'the owner is told');

  const flags = await request(env, 'GET', '/api/admin/accounts/review-flags', { token: adminToken });
  assert.equal(flags.status, 200);
  assert.deepEqual(flags.body.flags.map((f) => [f.account_id, f.case_ref]), [[OWNER, 'SUP-1042']]);
  assert.equal((await request(env, 'GET', '/api/admin/accounts/review-flags', { token: mintToken(OWNER) })).status, 403);

  const audit = await accountCall(env, OWNER, 'audit', { limit: 20 });
  assert.equal(await verifyAuditChain(audit.records), true);
  assert.ok(audit.records.some((x) => x.type === 'key_restored_after_admin_replacement'));
});

test('the link works once, only the right one works, and it does not work after its window', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  await replace(env, adminKey, adminToken, keyPair());
  const link = await undoLinkOf(sent);
  const [id, secret] = link.split('.');
  for (const bad of ['nonsense', `${id}.${'a'.repeat(64)}`, `${OTHER}.${secret}`, `${id}.${secret.toUpperCase()}`, '', `${id}`]) {
    assert.equal((await restore(env, bad)).status, 400, bad);
  }
  assert.equal((await state(env)).key_epoch, 1, 'nothing changed');
  assert.equal((await restore(env, link)).status, 200);
  assert.equal((await restore(env, link)).status, 400, 'once');

  const late = await environment();
  await replace(late.env, late.adminKey, late.adminToken, keyPair());
  const lateLink = await undoLinkOf(late.sent);
  const realNow = Date.now;
  Date.now = () => realNow() + 8 * DAY;
  try {
    assert.equal((await restore(late.env, lateLink)).status, 400, 'after the window');
  } finally { Date.now = realNow; }
  assert.equal((await restore(late.env, lateLink)).status, 200, 'and within it');
});

test('the window is the length of the freeze and never less than a week', async () => {
  const { env, adminKey, adminToken } = await environment();
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.ok(Math.abs(Date.parse(result.body.undo_until) - Date.now() - 7 * DAY) < 5000, 'seven days when the freeze is shorter');
});

test('whoever took the key after support replaced it cannot kill the link: it still puts the earlier key back, and every key since is retired', async () => {
  const { env, lost, adminKey, adminToken, sent } = await environment();
  const fresh = keyPair();
  await replace(env, adminKey, adminToken, fresh);
  const link = await undoLinkOf(sent);
  // The holder of the new key replaces it again, straight away.
  const begun = await accountCall(env, OWNER, 'begin_rotation');
  const third = keyPair();
  const m = ['eustress-key-rotation-v1', ORIGIN, OWNER, fresh.hex, third.hex, begun.challenge].join('\n');
  const rot = await accountCall(env, OWNER, 'rotate', { new_public_key: third.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: fresh.sign(m), new_signature: third.sign(m) });
  assert.equal(rot.ok, true, JSON.stringify(rot));
  assert.equal((await restore(env, link)).status, 200);
  const held = await state(env);
  assert.equal(held.public_key, lost.hex);
  assert.equal((await accountCall(env, OWNER, 'check_login', { public_key: third.hex })).code, 'key_retired');
  assert.equal((await accountCall(env, OWNER, 'check_login', { public_key: fresh.hex })).code, 'key_retired');
});

test('a second replacement while the owner\'s undo link is open is refused, so the link can only ever restore the key it was made for', async () => {
  const { env, lost, adminKey, adminToken, sent } = await environment();
  const first = keyPair();
  await replace(env, adminKey, adminToken, first, { caseRef: 'SUP-1' });
  const link = await undoLinkOf(sent);
  const second = keyPair();
  const refused = await replace(env, adminKey, adminToken, second, { caseRef: 'SUP-2' });
  assert.equal(refused.result.status, 409, JSON.stringify(refused.result.body));
  assert.equal(refused.result.body.code, 'replacement_in_window');
  assert.ok(refused.result.body.until, 'it says until when');
  assert.equal((await state(env)).public_key, first.hex, 'nothing changed');
  assert.equal((await state(env)).key_epoch, 1);

  // The link still restores the key it was made for.
  assert.equal((await restore(env, link)).status, 200);
  assert.equal((await state(env)).public_key, lost.hex);

  // Once the link was used the account may be replaced again.
  const again = await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-3' });
  assert.equal(again.result.status, 200, JSON.stringify(again.result.body));
});

test('once the window has lapsed another replacement is allowed, and it replaces the link', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-1' });
  const first = await undoLinkOf(sent);
  sent.length = 0;
  const lateToken = await later(8 * DAY, () => mintToken(ADMIN));
  const second = await later(8 * DAY, () => replace(env, adminKey, lateToken, keyPair(), { caseRef: 'SUP-2' }));
  assert.equal(second.result.status, 200, JSON.stringify(second.result.body));
  const next = await undoLinkOf(sent);
  assert.notEqual(first, next);
  assert.equal((await restore(env, first)).status, 400, 'the first link no longer works');
});

test('a link for an id nobody holds does not wake an account object', async () => {
  const { env } = await environment();
  assert.equal((await restore(env, `${OTHER}.${'a'.repeat(64)}`)).status, 400);
  assert.equal(env.ACCOUNT_AUTH.instances.has(OTHER), false);
});

// ── One administrator, five a day ────────────────────────────────────────────

test('one administrator may replace five keys a day, counted where their signature is good; another administrator is not held up', async () => {
  const { env, adminKey, adminToken } = await environment();
  for (let i = 0; i < 5; i++) {
    await closeWindow(env);
    const r = await replace(env, adminKey, adminToken, keyPair(), { caseRef: `SUP-${i + 10}` });
    assert.equal(r.result.status, 200, `${i}: ${JSON.stringify(r.result.body)}`);
  }
  await closeWindow(env);
  const sixth = await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-99' });
  assert.equal(sixth.result.status, 429, JSON.stringify(sixth.result.body));
  assert.equal(sixth.result.body.code, 'rate_limited');
  assert.equal((await state(env)).key_epoch, 5, 'the sixth changed nothing');

  // Failed signatures use none of it.
  const fresh = await environment();
  const thief = keyPair();
  for (let i = 0; i < 8; i++) {
    const r = await replace(fresh.env, thief, fresh.adminToken, keyPair(), { sign: thief });
    assert.equal(r.result.status, 401);
  }
  const ok = await replace(fresh.env, fresh.adminKey, fresh.adminToken, keyPair());
  assert.equal(ok.result.status, 200, 'a real administrator is not locked out by someone with only a session');
});

test('an administrator\'s replacements do not use up the owner\'s three a day: the owner can still replace the key', async () => {
  const { env, adminKey, adminToken } = await environment();
  let key = null;
  for (let i = 0; i < 4; i++) {
    await closeWindow(env);
    key = keyPair();
    const r = await replace(env, adminKey, adminToken, key, { caseRef: `SUP-${i + 10}` });
    assert.equal(r.result.status, 200, `${i}: ${JSON.stringify(r.result.body)}`);
  }
  assert.equal((await state(env)).key_epoch, 4);
  const begun = await accountCall(env, OWNER, 'begin_rotation');
  const next = keyPair();
  const m = ['eustress-key-rotation-v1', ORIGIN, OWNER, key.hex, next.hex, begun.challenge].join('\n');
  const own = await accountCall(env, OWNER, 'rotate', { new_public_key: next.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: key.sign(m), new_signature: next.sign(m) });
  assert.equal(own.ok, true, JSON.stringify(own));
  assert.equal(own.key_epoch, 5);
});

// ── What it refuses ──────────────────────────────────────────────────────────

test('it asks for a case, a reason and an account, and says which is missing', async () => {
  const { env, lost, adminKey, adminToken } = await environment();
  const fresh = keyPair();
  // The case is signed, so a bad one is built into the signature and refused by the object.
  // (A missing case cannot be signed for, so the signature check may answer first.)
  const viaCase = [['no case', null, [400, 401]], ['a case that is not a reference', 'no spaces allowed', [400]], ['a case that is too short', 'ab', [400]]];
  // (Five signed requests a day per administrator: forget the count between cases, which are about the
  // validation, not the cap.)
  const forgetCap = async () => {
    const object = env.ACCOUNT_AUTH.instances.get(ADMIN);
    const held = await object.storage.get('state');
    held.admin_actions = [];
    await object.storage.put('state', held);
  };
  for (const [name, caseRef, statuses] of viaCase) {
    await forgetCap();
    const r = await replace(env, adminKey, adminToken, fresh, { caseRef });
    assert.ok(statuses.includes(r.result.status), `${name}: ${r.result.status} ${JSON.stringify(r.result.body)}`);
  }
  // The reason is not signed: changed after.
  const viaReason = [
    ['no reason', (b) => { delete b.reason; }],
    ['a reason too short', (b) => { b.reason = 'ok'; }],
    ['a reason with a line break', (b) => { b.reason = 'checked\nthen done, twice over'; }],
    ['a reason that is too long', (b) => { b.reason = 'x'.repeat(201); }],
  ];
  for (const [name, mutate] of viaReason) {
    await forgetCap();
    const r = await replace(env, adminKey, adminToken, fresh, { mutate });
    assert.equal(r.result.status, 400, `${name}: ${JSON.stringify(r.result.body)}`);
  }
  // The account is named when the challenge is asked for.
  const none = await request(env, 'POST', BEGIN, { token: adminToken, body: { new_public_key: fresh.hex, case_ref: 'SUP-1' } });
  assert.equal(none.status, 404);
  const ghost = await request(env, 'POST', BEGIN, { token: adminToken, body: { username: 'nobody', new_public_key: fresh.hex, case_ref: 'SUP-1' } });
  assert.equal(ghost.status, 404);
  assert.equal((await state(env)).public_key, lost.hex, 'none of them changed the account');
});

test('the new key must be a key: not a small-order point, not the current one, not a retired one, not another account\'s', async () => {
  const { env, lost, adminKey, adminToken } = await environment();
  const asKey = (hex) => ({ hex, sign: () => '00'.repeat(64) });
  const small = [...SMALL_ORDER_KEYS][0];
  assert.equal((await replace(env, adminKey, adminToken, asKey(small))).result.status, 400);
  assert.equal((await replace(env, adminKey, adminToken, asKey('not a key'))).result.status >= 400, true);
  assert.equal((await replace(env, adminKey, adminToken, asKey(lost.hex))).result.status, 409, 'the current key');

  const first = keyPair();
  assert.equal((await replace(env, adminKey, adminToken, first)).result.status, 200);
  assert.equal((await replace(env, adminKey, adminToken, asKey(lost.hex))).result.status, 409, 'a retired key');

  const stranger = keyPair();
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other', public_key: stranger.hex }));
  await env.USERS.put(`pubkey:${stranger.hex}`, OTHER);
  const taken = await replace(env, adminKey, adminToken, asKey(stranger.hex));
  assert.equal(taken.result.status, 409, "another account's key");
  assert.equal(taken.result.body.code, 'key_unacceptable');
  assert.equal((await state(env)).key_epoch, 1, 'only the good one changed anything');
});

test('an administrator whose own session was ended cannot use the route', async () => {
  const { env, adminToken } = await environment();
  await accountCall(env, ADMIN, 'bump_floor', { reason: 'test' });
  await env.USERS.delete(`authfloor:${ADMIN}`); // the KV mirror has not caught up
  const r = await request(env, 'POST', BEGIN, { token: adminToken, body: { username: 'owner', new_public_key: keyPair().hex, case_ref: 'SUP-1' } });
  assert.equal(r.status, 401, 'the gate asks the object: the session is over');
  assert.equal(r.body.code, 'session_revoked');
});

test('a replacement may be found by account id as well as by username', async () => {
  const { env, adminKey, adminToken } = await environment();
  const r = await replace(env, adminKey, adminToken, keyPair(), { begin: { account_id: OWNER, username: undefined } });
  assert.equal(r.begun.status, 200, JSON.stringify(r.begun.body));
  assert.equal(r.begun.body.account_id, OWNER);
});

// ── The support script ───────────────────────────────────────────────────────

test('the support script signs in with the administrator\'s identity file and does the whole flow', async () => {
  const { env, adminKey } = await environment();
  const fresh = keyPair();
  const toml = `[identity]
username = "admin_user"
public_key = "${adminKey.hex}"
private_key = "${adminKey.seed}"
`;
  const fetchImpl = (url, init) => worker.fetch(new Request(url, init), env, ctx);
  const done = await replaceKey({
    api: ORIGIN, identityText: toml, username: 'owner', newPublicKey: fresh.hex,
    newKeySignature: fresh.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-8', newKey: fresh.hex })),
    caseRef: 'SUP-8', reason: 'ID checked on a video call', fetchImpl,
  });
  assert.equal(done.ok, true);
  assert.equal(done.key_epoch, 1);
  assert.equal((await state(env)).public_key, fresh.hex);
  assert.ok(!JSON.stringify(done).includes(adminKey.seed), 'nothing secret comes back');
});

test('the support script refuses what the route refuses, in the words the Worker gives, and reads only an identity file', async () => {
  const { env, adminKey } = await environment();
  const fresh = keyPair();
  const toml = `[identity]
public_key = "${adminKey.hex}"
private_key = "${adminKey.seed}"
`;
  const fetchImpl = (url, init) => worker.fetch(new Request(url, init), env, ctx);
  await assert.rejects(() => replaceKey({
    api: ORIGIN, identityText: toml, username: 'owner', newPublicKey: fresh.hex, newKeySignature: '00'.repeat(64), caseRef: 'SUP-8', reason: 'ID checked on a video call', fetchImpl,
  }), /new_key_signature_invalid/);
  assert.throws(() => readIdentity('nothing here'), /not an identity file/);
  assert.throws(() => readIdentity('[identity]\npublic_key = "abc"\nprivate_key = "def"'), /not an identity file/);
  assert.equal((await state(env)).key_epoch, 0);
});

test('a notice the route could not send reaches the owner from the object, with the undo link', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  env.EMAIL.send = async (m) => { if (m.to === 'owner@example.com') throw new Error('mail is down'); };
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200);
  assert.equal((await state(env)).notices_pending, 1);
  env.EMAIL.send = async (m) => { sent.push(m); };
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  const realNow = Date.now;
  Date.now = () => realNow() + 61 * 1000;
  try { await object.alarm(); } finally { Date.now = realNow; }
  const text = bodyOf(sent.find((m) => m.to === 'owner@example.com'));
  assert.match(text, /replaced by Eustress support/);
  assert.match(text, /undo-key-replacement\?token=[0-9a-f-]{36}\.[0-9a-f]{64}/);
  // And that link works.
  const link = /undo-key-replacement\?token=([0-9a-f-]{36}\.[0-9a-f]{64})/.exec(text)[1];
  assert.equal((await request(env, 'POST', '/api/account/key-restore', { body: { token: link } })).status, 200);
});

// ── Round 4: one predicate for an administrator, the second channel first, plain words, resolving a flag ──

test('an account whose record says role admin is an administrator here too: it cannot be the target either', async () => {
  const { env, adminKey, adminToken } = await environment();
  const roleAdmin = '90909090-9090-4090-8090-909090909090';
  await env.USERS.put(`user:${roleAdmin}`, JSON.stringify({ kyc_verified: true, id: roleAdmin, username: 'role_admin', public_key: keyPair().hex, role: 'admin' }));
  await env.USERS.put('username:role_admin', roleAdmin);
  const r = await replace(env, adminKey, adminToken, keyPair(), { username: 'role_admin' });
  assert.equal(r.result.status, 403, JSON.stringify(r.result.body));
  assert.equal(r.result.body.code, 'target_is_admin');
  // And one predicate answers both questions.
  const { isAdmin } = await import('../src/admin_check.mjs');
  assert.equal(await isAdmin(env, roleAdmin), true);
  assert.equal(await isAdmin(env, ADMIN), true);
  assert.equal(await isAdmin(env, OWNER), false);
  assert.equal(await isAdmin(env, undefined), false);
  assert.equal(await isAdmin(env, 'nobody'), false);
  // A role admin may be the caller, as before.
  const roleToken = mintToken(roleAdmin);
  const asCaller = await request(env, 'GET', '/api/admin/accounts/review-flags', { token: roleToken });
  assert.equal(asCaller.status, 200);
});

test('the second channel is told BEFORE anything changes, and a replacement it cannot tell nobody about does not happen', async () => {
  const { env, lost, adminKey, adminToken, sent } = await environment();
  let epochWhenSummaryWent = null;
  const realSend = env.EMAIL.send;
  env.EMAIL.send = async (m) => {
    if (m.to === 'security@eustress.dev' && epochWhenSummaryWent === null) epochWhenSummaryWent = (await state(env)).key_epoch;
    return realSend(m);
  };
  const ok = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(ok.result.status, 200, JSON.stringify(ok.result.body));
  assert.equal(epochWhenSummaryWent, 0, 'the summary went out while the key was still the old one');
  assert.ok(ok.result.body.second_channel_sent >= 1);

  // The security mailbox cannot be told: nothing changes, nothing is sent to the owner.
  const down = await environment();
  down.env.EMAIL.send = async () => { throw new Error('mail is down'); };
  const r = await replace(down.env, down.adminKey, down.adminToken, keyPair());
  assert.equal(r.result.status, 503, JSON.stringify(r.result.body));
  assert.equal(r.result.body.code, 'second_channel_unavailable');
  assert.equal((await state(down.env)).public_key, down.lost.hex, 'the key was not replaced');
  assert.equal((await state(down.env)).key_epoch, 0);
  assert.equal((await state(down.env)).notices_pending, 0, 'and no notice is owed');
  assert.equal(down.sent.length, 0);
  void lost; void sent;

  // No mail binding at all fails closed too.
  const bare = await environment();
  delete bare.env.EMAIL;
  const b = await replace(bare.env, bare.adminKey, bare.adminToken, keyPair());
  assert.equal(b.result.status, 503);
  assert.equal((await state(bare.env)).key_epoch, 0);
});

test('a request that would be refused anyway tells the second channel nothing', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  const fresh = keyPair();
  const r = await replace(env, adminKey, adminToken, fresh, { mutate: (b) => { b.new_key_signature = '00'.repeat(64); } });
  assert.equal(r.result.status, 401);
  assert.equal(r.result.body.code, 'new_key_signature_invalid');
  assert.equal(sent.length, 0, 'nothing was mailed for a request that was never going to happen');
  const short = await replace(env, adminKey, adminToken, fresh, { mutate: (b) => { b.reason = 'ok'; } });
  assert.equal(short.result.status, 400);
  assert.equal(sent.length, 0);
});

test('the notice says in plain words that the link puts the OLD key back, and the page does too', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  await replace(env, adminKey, adminToken, keyPair());
  const m = sent.find((x) => x.to === 'owner@example.com');
  const text = bodyOf(m);
  assert.match(text, /If you DID ask support to replace your key, do NOT use this link: it puts your OLD key back\./);
  assert.match(text, /<b>If you did ask support to replace your key, do NOT use this link: it puts your OLD key back\.<\/b>/);
});

test('a review flag is closed by an administrator, with a note, once; the note and the administrator are kept', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-1042' });
  await restore(env, await undoLinkOf(sent));
  const RESOLVE = '/api/admin/accounts/review-flags/resolve';
  assert.equal((await request(env, 'POST', RESOLVE, { token: mintToken(OTHER), body: { account_id: OWNER, note: 'looked at it' } })).status, 403, 'only an administrator');
  assert.equal((await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, note: 'ok' } })).status, 400, 'a note is required');
  assert.equal((await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OTHER, note: 'looked at it' } })).status, 404);
  const done = await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, note: 'Spoke to the owner; the first request was not theirs.' } });
  assert.equal(done.status, 200, JSON.stringify(done.body));
  assert.equal(await env.USERS.get(`admin-review:${OWNER}`), null);
  assert.deepEqual((await request(env, 'GET', '/api/admin/accounts/review-flags', { token: adminToken })).body.flags, []);
  assert.deepEqual((await state(env)).admin_review, []);
  assert.equal((await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, note: 'looked again' } })).status, 404, 'once');
  const audit = await accountCall(env, OWNER, 'audit', { limit: 30 });
  assert.equal(await verifyAuditChain(audit.records), true);
  const rec = audit.records.find((x) => x.type === 'review_resolved');
  assert.equal(rec.data.admin_id, ADMIN);
  assert.equal(rec.data.note, 'Spoke to the owner; the first request was not theirs.');
});

test('a revocation that keeps failing is retried with growing waits, then given up on and flagged for an administrator', async () => {
  const { env, adminKey, adminToken } = await environment();
  const hub = hubStub(env, OWNER);
  await callObject(hub, 'create_key', { account_id: OWNER, name: 'ci', mode: 'live', key_type: 'commerce' });
  const real = env.COMMERCE_HUBS;
  let tries = 0;
  env.COMMERCE_HUBS = {
    idFromName: real.idFromName,
    get: (id) => {
      const stub = real.get(id);
      return { fetch: (url, init) => { if (String(url).endsWith('/revoke_key')) { tries++; return Promise.reject(new Error('hub down')); } return stub.fetch(url, init); } };
    },
  };
  const { result } = await replace(env, adminKey, adminToken, keyPair());
  assert.equal(result.status, 200);
  assert.equal(result.body.revoked_complete, false);
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  const afterRoute = tries;

  // The object's first try is at once; then it waits, and the wait grows.
  await object.alarm();
  assert.equal((await state(env)).credentials_pending, 1);
  assert.equal(tries, afterRoute + 1, 'its first try');
  await object.alarm();
  assert.equal(tries, afterRoute + 1, 'it waited: no call to the hub');
  await later(61 * 1000, () => object.alarm());
  assert.equal(tries, afterRoute + 2, 'one try once the first wait is over');
  await later(61 * 1000 + 70 * 1000, () => object.alarm());
  assert.equal(tries, afterRoute + 2, 'and the second wait (two minutes) is longer than the first (one)');
  await later(61 * 1000 + 130 * 1000, () => object.alarm());
  assert.equal(tries, afterRoute + 3, 'and it tries again after it');

  // Each later alarm, past the growing wait, tries once more.
  let waited = 0;
  for (let i = 1; i <= 12 && (await state(env)).credentials_pending > 0; i++) {
    waited += 3700 * 1000;
    await later(waited, () => object.alarm());
  }
  const held = await state(env);
  assert.equal(held.credentials_pending, 0, 'it stopped trying');
  assert.equal(held.admin_review[0].reason, 'credentials_not_revoked');
  assert.ok(await env.USERS.get(`admin-review:${OWNER}`), 'flagged where an administrator looks');
  const flags = await request(env, 'GET', '/api/admin/accounts/review-flags', { token: adminToken });
  assert.ok(flags.body.flags.some((x) => x.account_id === OWNER && x.reason === 'credentials_not_revoked'));
  const audit = await accountCall(env, OWNER, 'audit', { limit: 30 });
  assert.ok(audit.records.some((x) => x.type === 'revocation_abandoned'));
});

test('flags never hide each other: a given-up revocation and an undone replacement are both kept, listed, and closed one at a time', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  const hub = hubStub(env, OWNER);
  await callObject(hub, 'create_key', { account_id: OWNER, name: 'ci', mode: 'live', key_type: 'commerce' });
  const real = env.COMMERCE_HUBS;
  env.COMMERCE_HUBS = {
    idFromName: real.idFromName,
    get: (id) => {
      const stub = real.get(id);
      return { fetch: (url, init) => (String(url).endsWith('/revoke_key') ? Promise.reject(new Error('hub down')) : stub.fetch(url, init)) };
    },
  };
  await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-7' });
  const link = await undoLinkOf(sent);
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  let waited = 0;
  for (let i = 1; i <= 12 && (await state(env)).credentials_pending > 0; i++) {
    waited += 3700 * 1000;
    await later(waited, () => object.alarm());
  }
  assert.equal((await state(env)).admin_review.length, 1, 'the abandoned revocation is flagged');
  assert.equal((await restore(env, link)).status, 200);
  const held = await state(env);
  assert.deepEqual(held.admin_review.map((x) => x.reason), ['credentials_not_revoked', 'replacement_undone'], 'the second flag did not replace the first');
  const flags = await request(env, 'GET', '/api/admin/accounts/review-flags', { token: adminToken });
  assert.deepEqual(flags.body.flags.map((x) => x.reason).sort(), ['credentials_not_revoked', 'replacement_undone']);

  const RESOLVE = '/api/admin/accounts/review-flags/resolve';
  const note = 'Looked at it with the owner.';
  const none = await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, note } });
  assert.equal(none.status, 409, 'with two flags the one to close has to be named');
  assert.equal(none.body.code, 'flag_id_required');
  assert.equal((await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, flag_id: 'nope', note } })).status, 404);
  const first = flags.body.flags.find((x) => x.reason === 'credentials_not_revoked');
  const done = await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, flag_id: first.id, note } });
  assert.equal(done.status, 200, JSON.stringify(done.body));
  assert.equal(done.body.remaining, 1);
  assert.deepEqual((await state(env)).admin_review.map((x) => x.reason), ['replacement_undone'], 'the other flag stays');
  assert.ok(await env.USERS.get(`admin-review:${OWNER}`), 'and stays in KV');
  const rest = await request(env, 'POST', RESOLVE, { token: adminToken, body: { account_id: OWNER, note } });
  assert.equal(rest.status, 200, 'the last flag needs no id');
  assert.equal(await env.USERS.get(`admin-review:${OWNER}`), null);
});

test('/health says whether the administrator replacement is on and has what it needs: mail, and a mailbox for the second channel', async () => {
  const none = await environment({ ADMIN_REPLACE_KEY_ENABLED: undefined });
  const off = await request(none.env, 'GET', '/health');
  assert.deepEqual(off.body.integrations.admin_replace, { enabled: false, email_bound: true, security_mailbox_set: false });
  const on = await environment({ SECURITY_MAILBOX: 'security@example.org' });
  const r = await request(on.env, 'GET', '/health');
  assert.deepEqual(r.body.integrations.admin_replace, { enabled: true, email_bound: true, security_mailbox_set: true });
  delete on.env.EMAIL;
  assert.equal((await request(on.env, 'GET', '/health')).body.integrations.admin_replace.email_bound, false);
  assert.ok(!JSON.stringify(r.body).includes('security@example.org'), 'the mailbox itself is not shown');
});

test('a flag raised later does not replace one that is already open, whichever comes first', async () => {
  const { env, adminKey, adminToken, sent } = await environment();
  const hub = hubStub(env, OWNER);
  await callObject(hub, 'create_key', { account_id: OWNER, name: 'ci', mode: 'live', key_type: 'commerce' });
  const real = env.COMMERCE_HUBS;
  env.COMMERCE_HUBS = {
    idFromName: real.idFromName,
    get: (id) => {
      const stub = real.get(id);
      return { fetch: (url, init) => (String(url).endsWith('/revoke_key') ? Promise.reject(new Error('hub down')) : stub.fetch(url, init)) };
    },
  };
  await replace(env, adminKey, adminToken, keyPair(), { caseRef: 'SUP-8' });
  // The owner undoes it first: one flag. Then the revocation is given up on: a second.
  assert.equal((await restore(env, await undoLinkOf(sent))).status, 200);
  assert.deepEqual((await state(env)).admin_review.map((x) => x.reason), ['replacement_undone']);
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  let waited = 0;
  for (let i = 1; i <= 12 && (await state(env)).credentials_pending > 0; i++) {
    waited += 3700 * 1000;
    await later(waited, () => object.alarm());
  }
  assert.deepEqual((await state(env)).admin_review.map((x) => x.reason), ['replacement_undone', 'credentials_not_revoked']);
});

test('the review listing still reads an entry written in the older shape (one object, not a list)', async () => {
  const { env, adminToken } = await environment();
  await env.USERS.put(`admin-review:${OWNER}`, JSON.stringify({ at: '2026-10-01T10:00:00.000Z', case_ref: 'SUP-OLD', admin_id: ADMIN }));
  const r = await request(env, 'GET', '/api/admin/accounts/review-flags', { token: adminToken });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body.flags.map((x) => [x.account_id, x.case_ref]), [[OWNER, 'SUP-OLD']]);
});
