// Replacing an identity key through the real routes: the challenge, the two
// signatures, what the key owns, the token for the device that rotated, and the
// notice. The real Worker and the real account object run against in-memory
// storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, rotationMessage, FREEZE_WITH_VALUE_MS, FREEZE_WITHOUT_VALUE_MS } from '../src/account_auth.mjs';
import { Wallet, CommerceHub, callObject, hubStub } from '../src/commerce.mjs';
import { renderKeyNotice, noticeRecipients, buildNoticeMime } from '../src/account_notices.mjs';

const worker = await loadWorker();
const ACCOUNT = '55555555-5555-4555-8555-555555555555';
const STRANGER = '66666666-6666-4666-8666-666666666666';
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
  const k0 = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: k0.hex, email: 'owner@example.com', discord_id: 'discord-123' }));
  await env.USERS.put('username:owner', ACCOUNT);
  await env.USERS.put(`pubkey:${k0.hex}`, ACCOUNT);
  return { env, k0, token: mintToken(ACCOUNT) };
}

async function post(env, route, body, token, origin = ORIGIN) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${origin}${route}`, { method: 'POST', headers, body: JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
async function me(env, token) {
  const res = await worker.fetch(new Request(`${ORIGIN}/api/auth/me`, { headers: token ? { Authorization: `Bearer ${token}` } : {} }), env, ctx);
  return res.status;
}

/// begin, then both signatures, then rotate.
async function rotate(env, token, oldKey, { newKey = keyPair(), revoke, mutate, origin = ORIGIN } = {}) {
  const begun = await post(env, '/api/identity/rotate/begin', {}, token, origin);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  const message = rotationMessage({ origin, accountId: ACCOUNT, oldKey: oldKey.hex, newKey: newKey.hex, challenge: begun.body.challenge });
  const request = { new_public_key: newKey.hex, challenge: begun.body.challenge, old_signature: oldKey.sign(message), new_signature: newKey.sign(message), revoke };
  if (mutate) mutate(request, { message, begun: begun.body, newKey, oldKey });
  return { begun: begun.body, newKey, ...(await post(env, '/api/identity/rotate', request, token, origin)) };
}

// ── The challenge ────────────────────────────────────────────────────────────

test('a challenge needs a signed-in session, an origin this deployment signs for, and an account object', async () => {
  const { env, token } = await environment();
  assert.equal((await post(env, '/api/identity/rotate/begin', {})).status, 401);
  assert.equal((await post(env, '/api/identity/rotate/begin', {}, 'not.a.token')).status, 401);
  const workersDev = await post(env, '/api/identity/rotate/begin', {}, token, 'https://eustress-api.example.workers.dev');
  assert.equal(workersDev.status, 400);
  assert.equal(workersDev.body.code, 'origin_not_allowed');

  const ok = await post(env, '/api/identity/rotate/begin', {}, token);
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  assert.match(ok.body.challenge, /^[0-9a-f]{64}$/);
  assert.equal(ok.body.origin, ORIGIN);
  assert.equal(ok.body.account_id, ACCOUNT);
  assert.equal(ok.body.format, 'eustress-key-rotation-v1');

  const bare = { ...env, ACCOUNT_AUTH: undefined };
  assert.equal((await post(bare, '/api/identity/rotate/begin', {}, token)).status, 501);
});

test('the challenge says what the key owns: API keys, webhook endpoints, the Discord link; no secret is in it', async () => {
  const { env, token } = await environment();
  const hub = hubStub(env, ACCOUNT);
  const made = await callObject(hub, 'create_key', { account_id: ACCOUNT, name: 'ci', mode: 'live', key_type: 'commerce' });
  const endpoint = await callObject(hub, 'create_endpoint', { account_id: ACCOUNT, url: 'https://hooks.example.com/eustress', enabled_events: ['*'], livemode: true });
  assert.equal(made.ok, true, JSON.stringify(made));
  assert.equal(endpoint.ok, true, JSON.stringify(endpoint));
  const begun = await post(env, '/api/identity/rotate/begin', {}, token);
  const { credentials } = begun.body;
  assert.equal(credentials.api_keys.length, 1);
  assert.equal(credentials.api_keys[0].name, 'ci');
  assert.equal(credentials.webhooks.length, 1);
  assert.equal(credentials.webhooks[0].host, 'hooks.example.com');
  assert.equal(credentials.discord_linked, true);
  assert.equal(credentials.payout_connected, false);
  const text = JSON.stringify(begun.body);
  assert.ok(!text.includes(made.key), 'the API key itself is never listed');
  assert.ok(!text.includes('whsec_'), 'no webhook secret is listed');
  assert.ok(!text.includes('hooks.example.com/eustress'), 'only the host of a webhook is shown');
});

// ── Replacing the key ───────────────────────────────────────────────────────

test('a rotation replaces the key, returns a token for this device, ends the others and refuses the old key', async () => {
  const { env, k0, token } = await environment();
  assert.equal(await me(env, token), 200);
  const r = await rotate(env, token, k0);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.ok, true);
  assert.equal(r.body.key_epoch, 1);
  assert.equal(await me(env, token), 401, 'the token the request carried is ended');
  assert.equal(await me(env, r.body.token), 200, 'the new token works');
  assert.equal(r.body.user.username, 'owner');

  // Signing in: the new key works, the old one is told it was replaced.
  const message = (c) => ['eustress-login-v1', ORIGIN, c].join(String.fromCharCode(10));
  const login = async (key) => {
    const ch = (await post(env, '/api/auth/challenge', { public_key: key.hex })).body.challenge;
    return post(env, '/api/auth/verify-challenge', { format: 'eustress-login-v1', public_key: key.hex, challenge: ch, signature: key.sign(message(ch)) });
  };
  assert.equal((await login(r.newKey)).status, 200);
  const old = await login(k0);
  assert.equal(old.status, 403);
  assert.equal(old.body.code, 'key_retired');
});

test('a stolen token alone cannot replace the key: the old key has to sign', async () => {
  const { env, k0, token } = await environment();
  const thief = keyPair();
  // The thief has the token and a key of their own, and signs with that for both.
  const r = await rotate(env, token, thief, { newKey: thief });
  assert.equal(r.status, 401);
  assert.equal(r.body.code, 'signature_invalid');
  // And with the real old key's signature missing.
  const r2 = await rotate(env, token, k0, { mutate: (req) => { req.old_signature = undefined; } });
  assert.equal(r2.status, 401);
  assert.equal(await me(env, token), 200, 'nothing changed');
});

test('a signature made for another origin, account or key is refused', async () => {
  const { env, k0, token } = await environment();
  const other = keyPair();
  for (const [label, make] of Object.entries({
    'another origin': (c) => rotationMessage({ origin: 'https://evil.example', accountId: ACCOUNT, oldKey: c.oldKey.hex, newKey: c.newKey.hex, challenge: c.begun.challenge }),
    'another account': (c) => rotationMessage({ origin: ORIGIN, accountId: STRANGER, oldKey: c.oldKey.hex, newKey: c.newKey.hex, challenge: c.begun.challenge }),
    'another new key': (c) => rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: c.oldKey.hex, newKey: other.hex, challenge: c.begun.challenge }),
  })) {
    const r = await rotate(env, token, k0, { mutate: (req, c) => { const m = make(c); req.old_signature = c.oldKey.sign(m); req.new_signature = c.newKey.sign(m); } });
    assert.equal(r.status, 401, label);
  }
  assert.equal(await me(env, token), 200);
});

test('a challenge is used once, and a malformed request is refused', async () => {
  const { env, k0, token } = await environment();
  const first = await rotate(env, token, k0);
  assert.equal(first.status, 200);
  const stillOk = first.body.token;
  const again = await post(env, '/api/identity/rotate', { new_public_key: first.newKey.hex, challenge: first.begun.challenge, old_signature: '00'.repeat(64), new_signature: '00'.repeat(64) }, stillOk);
  assert.equal(again.status, 400);
  assert.equal(again.body.code, 'challenge_invalid');
});

test('a revoke list that is not a list of credential ids refuses the whole request, and the same challenge still works without it', async () => {
  for (const revoke of ['everything', { api_keys: 'x'.repeat(10) }, { api_keys: ['../etc'] }, { webhooks: [42] }, { discord: 'yes' }, { api_keys: Array.from({ length: 60 }, (_, i) => `key_${i}`) }, [ 'key_a' ]]) {
    const { env, k0, token } = await environment();
    const begun = await post(env, '/api/identity/rotate/begin', {}, token);
    const nk = keyPair();
    const message = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: nk.hex, challenge: begun.body.challenge });
    const request = { new_public_key: nk.hex, challenge: begun.body.challenge, old_signature: k0.sign(message), new_signature: nk.sign(message) };
    const bad = await post(env, '/api/identity/rotate', { ...request, revoke }, token);
    assert.equal(bad.status, 400, JSON.stringify(revoke));
    assert.equal(bad.body.code, 'bad_request', JSON.stringify(revoke));
    assert.equal(await me(env, token), 200, 'nothing changed');
    // The same signed request without the bad list is still good: the challenge was not spent.
    const good = await post(env, '/api/identity/rotate', request, token);
    assert.equal(good.status, 200, JSON.stringify(good.body));
  }
});

test('another account\'s key, current or replaced, is never the new key', async () => {
  const { env, k0, token } = await environment();
  const theirs = keyPair();
  await env.USERS.put(`pubkey:${theirs.hex}`, STRANGER);
  const taken = await rotate(env, token, k0, { newKey: theirs });
  assert.equal(taken.status, 409);
  assert.equal(taken.body.code, 'key_unacceptable');
  const retired = keyPair();
  await env.USERS.put(`retired-key:${retired.hex}`, STRANGER);
  const gone = await rotate(env, token, k0, { newKey: retired });
  assert.equal(gone.status, 409);
});

// ── What the key owns ────────────────────────────────────────────────────────

test('the credentials the owner chooses are revoked in the same step, and only those', async () => {
  const { env, k0, token } = await environment();
  const hub = hubStub(env, ACCOUNT);
  const keep = await callObject(hub, 'create_key', { account_id: ACCOUNT, name: 'keep', mode: 'test', key_type: 'commerce' });
  const drop = await callObject(hub, 'create_key', { account_id: ACCOUNT, name: 'drop', mode: 'live', key_type: 'commerce' });
  const endpoint = await callObject(hub, 'create_endpoint', { account_id: ACCOUNT, url: 'https://hooks.example.com/x', enabled_events: ['*'], livemode: true });
  const r = await rotate(env, token, k0, { revoke: { api_keys: [drop.record.id], webhooks: [endpoint.endpoint.id], discord: true } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.deepEqual([r.body.revoked.api_keys, r.body.revoked.webhooks, r.body.revoked.discord], [1, 1, true]);
  const left = await callObject(hub, 'list_keys', { account_id: ACCOUNT });
  assert.deepEqual(left.data.map((k) => k.name), ['keep']);
  assert.equal((await callObject(hub, 'list_endpoints', { account_id: ACCOUNT, livemode: true })).data.length, 0);
  assert.equal(await env.USERS.get(`apikey:${crypto.createHash('sha256').update(drop.key).digest('hex')}`), null, 'the revoked key no longer authenticates');
  assert.ok(await env.USERS.get(`apikey:${crypto.createHash('sha256').update(keep.key).digest('hex')}`), 'the other key does');
  // Recorded in the account's object; the user record is not rewritten.
  assert.equal(JSON.parse(await env.USERS.get(`user:${ACCOUNT}`)).discord_id, 'discord-123');
  assert.equal(r.body.user.discord_id, null, 'the account no longer shows the link');
  assert.ok((await accountCall(env, ACCOUNT, 'state')).discord_unlinked_at);
});

test('a credential id that is not the account\'s changes nothing and is reported, and the rotation still happens', async () => {
  const { env, k0, token } = await environment();
  const r = await rotate(env, token, k0, { revoke: { api_keys: ['key_doesnotexist'] } });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body.revoked.failed, ['key_doesnotexist']);
});

// ── Value ────────────────────────────────────────────────────────────────────

test('value is frozen 7 days when the account holds value and 72 hours when it does not', async () => {
  const cases = [
    ['nothing', async () => {}, FREEZE_WITHOUT_VALUE_MS],
    ['a listing', async (env) => env.SOCIAL.put(`simCount:${ACCOUNT}`, '2'), FREEZE_WITH_VALUE_MS],
    ['a payout connection', async (env) => {
      const u = JSON.parse(await env.USERS.get(`user:${ACCOUNT}`)); u.stripe_connect_id = 'acct_123'; await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify(u));
    }, FREEZE_WITH_VALUE_MS],
    ['an unreadable balance', async (env) => { env.DELTA_ACCOUNTS = { idFromName: (n) => n, get() { throw new Error('delta is down'); } }; }, FREEZE_WITH_VALUE_MS],
  ];
  for (const [label, arrange, span] of cases) {
    const { env, k0, token } = await environment();
    await arrange(env);
    const before = Date.now();
    const r = await rotate(env, token, k0);
    assert.equal(r.status, 200, label);
    const frozen = r.body.value_frozen_until - before;
    assert.ok(Math.abs(frozen - span) < 5000, `${label}: ${frozen}`);
    assert.equal(r.body.holds_value, span === FREEZE_WITH_VALUE_MS, label);
  }
});

// ── The notice ───────────────────────────────────────────────────────────────

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

test('the owner is told, with fingerprints and no key, and the notice carries no link to act on', async () => {
  const sent = [];
  const { env, k0, token } = await environment({ EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage });
  const r = await rotate(env, token, k0);
  assert.equal(r.status, 200);
  assert.equal(r.body.notices_sent, 1);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].to, 'owner@example.com');
  const raw = sent[0].raw;
  assert.match(raw, /^To: owner@example\.com\r\n/m);
  const decoded = [...raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)].map((m) => Buffer.from(m[1].replace(/\r\n/g, ''), 'base64').toString());
  const text = decoded.join('\n');
  assert.ok(text.includes('replaced'));
  assert.ok(!text.includes(k0.hex) && !text.includes(r.newKey.hex), 'neither key is in the notice');
  // The header gear is the one remote address in a notice (a static asset, no query string); every other address is a link.
  const gear = /<img src="https:\/\/api\.eustress\.dev\/assets\/eustress-gear\.png" [^>]*>/;
  assert.ok(!/https?:\/\//.test(text.replace(gear, '')), 'no link in the notice');
  assert.ok(text.includes('support@eustress.dev'));
});

test('a notice that cannot be sent does not undo the replacement', async () => {
  const { env, k0, token } = await environment({ EMAIL: { send: async () => { throw new Error('mail is down'); } }, __EmailMessage: StubMessage });
  const r = await rotate(env, token, k0);
  assert.equal(r.status, 200);
  assert.equal(r.body.notices_sent, 0);
  assert.equal(r.body.key_epoch, 1);
});

test('a notice is built from escaped values, one plain address and no header injection', () => {
  const d = { username: '<img src=x onerror=alert(1)>', at: '2026-10-01T10:00:00.000Z', oldFingerprint: 'aaaa', newFingerprint: 'bbbb', valueFrozenUntil: 0, payoutPinnedUntil: 0 };
  const { html, text } = renderKeyNotice('rotation', d);
  assert.ok(!html.includes('<img src=x') && html.includes('&lt;img src=x onerror=alert(1)&gt;'), 'the username is escaped in the HTML');
  assert.equal((html.match(/<img\b/g) || []).length, 1, 'and the only image is the header gear');
  assert.ok(text.includes('<img src=x'), 'and plain in the text');
  assert.throws(() => renderKeyNotice('other', d));
  assert.throws(() => buildNoticeMime({ from: 'a@b.co', to: 'x@y.co\r\nBcc: z@y.co', subject: 's', text: 't', html: 'h' }));
  assert.deepEqual(noticeRecipients({ email: ' Owner@Example.com ' }), ['owner@example.com']);
  assert.deepEqual(noticeRecipients({ email: 'a@b.co\nBcc: c@d.co' }), []);
  assert.deepEqual(noticeRecipients({ email: 'a@b.co, c@d.co' }), []);
  assert.deepEqual(noticeRecipients({}), []);
});

// ── Around it ────────────────────────────────────────────────────────────────

test('the routes are rate limited per account', async () => {
  const keys = [];
  const { env, token } = await environment({ AUTH_RATE_LIMITER: { limit: async ({ key }) => { keys.push(key); return { success: false }; } } });
  const r = await post(env, '/api/identity/rotate/begin', {}, token);
  assert.equal(r.status, 429);
  assert.deepEqual(keys, [`identity-rotate:${ACCOUNT}`]);
});

test('the identity backup email reads the account\'s key from the object, not from the user record', async () => {
  const { env, k0, token } = await environment();
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response('', { status: 202 });
  try {
    const r = await rotate(env, token, k0);
    assert.equal(r.status, 200);
    // A stale write put the old key back into the user record.
    const u = JSON.parse(await env.USERS.get(`user:${ACCOUNT}`));
    u.public_key = k0.hex;
    await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify(u));
    const file = (key) => `# Eustress Identity — generated 2026-10-01T10:00:00+00:00\n# This file IS your account. Keep it safe. Do not share your private key.\n\n[identity]\nversion = "1.0"\nuser_id = "3f2b8a52-9c1e-4d7a-8b0f-6a5e4c3d2b1a"\nusername = "owner"\npublic_key = "${key}"\nprivate_key = "${'0123456789abcdef'.repeat(4)}"\nissued_at = "2026-10-01T10:00:00+00:00"\n\n[verification]\nverified = false\n`;
    const send = (key) => post(env, '/api/identity/email-backup', { email: 'owner@example.com', toml_content: file(key) }, r.body.token);
    assert.equal((await send(k0.hex)).status, 400, 'a file naming the replaced key is refused');
    assert.equal((await send(r.newKey.hex)).status, 200, 'a file naming the current key is sent');
  } finally { globalThis.fetch = real; }
});

// ── The one-time prompt ──────────────────────────────────────────────────────

test('the user view says how many times the key was replaced and whether the prompt was shown', async () => {
  const { env, k0, token } = await environment();
  const meBody = async (t) => {
    const res = await worker.fetch(new Request(`${ORIGIN}/api/auth/me`, { headers: { Authorization: `Bearer ${t}` } }), env, ctx);
    return res.json();
  };
  const before = await meBody(token);
  assert.equal(before.key_epoch, 0);
  assert.equal(before.key_prompt_seen_at, null);

  assert.equal((await post(env, '/api/identity/rotate/prompt-seen', {})).status, 401, 'a session is needed');
  const seen = await post(env, '/api/identity/rotate/prompt-seen', {}, token);
  assert.equal(seen.status, 200);
  assert.ok(seen.body.key_prompt_seen_at);
  const first = (await meBody(token)).key_prompt_seen_at;
  assert.ok(first);
  // Marking it again keeps the first time.
  await post(env, '/api/identity/rotate/prompt-seen', {}, token);
  assert.equal((await meBody(token)).key_prompt_seen_at, first);

  const r = await rotate(env, token, k0);
  assert.equal(r.status, 200);
  assert.equal(r.body.user.key_epoch, 1);
  assert.equal((await meBody(r.body.token)).key_epoch, 1);
});

test('without account objects the user view simply has no key fields', async () => {
  const { env, token } = await environment();
  const bare = { ...env, ACCOUNT_AUTH: undefined };
  const res = await worker.fetch(new Request(`${ORIGIN}/api/auth/me`, { headers: { Authorization: `Bearer ${token}` } }), bare, ctx);
  const body = await res.json();
  assert.equal(res.status, 200);
  assert.equal(body.key_epoch, undefined);
});
