// The last of the Worker security set: the identity-backup email, thumbnails,
// what a re-upload does to an approved listing, a ban reaching the token the
// account already holds, and the downloads Worker's token check. Each test is
// written against the old behavior first: it fails there and passes now.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeBucket, JWT_SECRET, ctx, testPng, testWebp } from './helpers/load_worker.mjs';
import { seedVerifiedKyc, claimSecretOf } from './helpers/kyc_session.mjs';

const worker = await loadWorker();

const ADMIN = '99999999-9999-4999-8999-999999999999';
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = { admin: mintToken(ADMIN), author: mintToken(AUTHOR), other: mintToken(OTHER) };
const PNG = testPng();
const WEBP = testWebp();
const HTML = new TextEncoder().encode('<html><script>fetch("/api/auth/me")</script></html>');
const PK = { author: 'a1'.repeat(32), other: 'b2'.repeat(32) };

/// The identity file the sign-up page builds for an account, in the shape of
/// generate_identity_toml in web/src/pages/login.rs.
function identityFile({ username, public_key }) {
  return `# Eustress Identity — generated 2026-09-30T18:42:11+00:00
# This file IS your account. Keep it safe. Do not share your private key.

[identity]
version = "1.0"
user_id = "3f2b8a52-9c1e-4d7a-8b0f-6a5e4c3d2b1a"
username = "${username}"
public_key = "${public_key}"
private_key = "${'0123456789abcdef'.repeat(4)}"
issued_at = "2026-09-30T18:42:11+00:00"

[profile]
birthday = "1990-01-01"

[verification]
id_type = "passport"
id_hash = "${'ab'.repeat(32)}"
verified = false
`;
}
const TOML = identityFile({ username: 'author_user', public_key: PK.author });

async function environment(extra = {}) {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), ...extra };
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'admin_user' }));
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author_user', public_key: PK.author, email: 'author@example.com' }));
  await env.USERS.put('username:author_user', AUTHOR);
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other_user', public_key: PK.other, email: 'other@example.com' }));
  await env.USERS.put('username:other_user', OTHER);
  return env;
}

async function call(env, method, route, { token, body, headers } = {}) {
  // Every account is made from one verified KYC session (src/kyc_identity.mjs):
  // a registration here gets a fresh one, with its claim secret, unless the test names its own.
  if (route === '/api/auth/register' && body && typeof body === 'object' && !('kyc_session_id' in body)) {
    const kyc_session_id = await seedVerifiedKyc(env);
    body = { ...body, kyc_session_id, kyc_claim_secret: claimSecretOf(kyc_session_id) };
  }
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  const init = { method, headers: h };
  if (body !== undefined) {
    init.body = typeof body === 'string' || body instanceof Uint8Array ? body : JSON.stringify(body);
    if (typeof body === 'object' && !(body instanceof Uint8Array)) h['Content-Type'] = 'application/json';
  }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${route}`, init), env, ctx);
  const raw = new Uint8Array(await res.arrayBuffer());
  let parsed = null;
  try { parsed = JSON.parse(new TextDecoder().decode(raw)); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, body: parsed, raw };
}

async function seedListing(env, moderationStatus) {
  const sim = {
    id: SIM, name: 'World', description: '', genre: 'all', max_players: 10,
    author_id: AUTHOR, author_name: 'author_user', is_public: true,
    moderation: { status: moderationStatus, version: 1, policy_version: 1 },
    r2_key: `universes/${SIM}/universe.pak`, pak_etag: 'the-judged-etag', scene_size_bytes: 10,
    play_count: 0, favorite_count: 0, version: 1,
    published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SCENES.put(sim.r2_key, 'the world that was judged');
}

const status = async (env) => JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)).moderation.status;

// ── The identity-backup email ────────────────────────────────────────────────

/// What the Worker would hand to Cloudflare Email, and what it would send
/// through the MailChannels fallback, recorded instead of sent.
async function withMailRecorder(fn) {
  const real = globalThis.fetch;
  const sent = [];
  globalThis.fetch = async (url, init) => {
    sent.push({ url: String(url), body: init?.body ? JSON.parse(init.body) : null });
    return new Response('', { status: 202 });
  };
  try { await fn(sent); } finally { globalThis.fetch = real; }
}

const backup = (env, token, body) => call(env, 'POST', '/api/identity/email-backup', { token, body });
const validBackup = (extra = {}) => ({ email: 'author@example.com', toml_content: TOML, ...extra });

test('the backup email needs a signed-in account: anonymous callers send nothing', async () => {
  const env = await environment();
  await withMailRecorder(async (sent) => {
    const r = await backup(env, undefined, { email: 'victim@example.com', toml_content: TOML });
    assert.equal(r.status, 401);
    assert.equal(sent.length, 0);
  });
});

test('the backup goes only to the address on the account, and the name in it is the account\'s', async () => {
  const env = await environment();
  await withMailRecorder(async (sent) => {
    const elsewhere = await backup(env, TOKEN.author, { email: 'victim@example.com', toml_content: TOML });
    assert.equal(elsewhere.status, 403);
    assert.equal(elsewhere.body.code, 'email_not_on_account');
    assert.equal(sent.length, 0, 'nothing was sent to a stranger');

    const own = await backup(env, TOKEN.author, validBackup({ email: 'Author@Example.com', username: 'Evil Name' }));
    assert.equal(own.status, 200, JSON.stringify(own.body));
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].body.personalizations, [{ to: [{ email: 'author@example.com' }] }]);
    const text = JSON.stringify(sent[0].body);
    assert.ok(text.includes('author_user'), 'the account\'s name is used');
    assert.ok(!text.includes('Evil Name'), 'the request\'s name is not');
  });
});

test('an address that could add a header or a second recipient is refused', async () => {
  const env = await environment();
  await withMailRecorder(async (sent) => {
    for (const email of [
      'author@example.com\r\nBcc: victim@example.com',
      'author@example.com\nBcc: victim@example.com',
      'author@example.com,victim@example.com',
      '"Author" <author@example.com>',
      'author@example.com; victim@example.com',
      'author example@example.com',
      '',
      42,
      null,
    ]) {
      const r = await backup(env, TOKEN.author, validBackup({ email }));
      assert.equal(r.status, 400, `${JSON.stringify(email)} got ${r.status}`);
    }
    assert.equal(sent.length, 0);
  });
});

test('what is mailed is this account\'s own identity file and nothing a caller wrote', async () => {
  const env = await environment();
  const real = identityFile({ username: 'author_user', public_key: PK.author });
  const swap = (from, to) => real.replace(from, to);
  const forged = [
    ['too short', 'short'],
    ['too long', real + '# ' + 'x'.repeat(4 * 1024)],
    ['not text', 12345],
    ['absent', undefined],
    ['another username', swap('username = "author_user"', 'username = "someone_else"')],
    ['another public key', swap(PK.author, PK.other)],
    ['no username', swap('username = "author_user"\n', '')],
    ['no public key', swap(`public_key = "${PK.author}"\n`, '')],
    ['an extra key', real.replace('[verification]', 'message = "Your account is locked: call 555-0100"\n[verification]')],
    ['an extra section', real + '\n[notes]\nmessage = "hello"\n'],
    ['a free-text comment', '# Your account is locked, visit https://evil.example\n' + real],
    ['a duplicated key', real.replace('[profile]', 'username = "author_user"\n[profile]')],
    ['a value with a quote or break', swap('id_type = "passport"', 'id_type = "passport\\" and more"')],
    ['a long id_type', swap('id_type = "passport"', `id_type = "${'a'.repeat(49)}"`)],
    ['a short key', swap(/private_key = "[0-9a-f]+"/.exec(real)[0], 'private_key = "ab"')],
    ['a value on the wrong key', swap(/birthday = "[^"]+"/.exec(real)[0], 'birthday = "call me on 555-0100"')],
  ];
  await withMailRecorder(async (sent) => {
    for (const [label, toml_content] of forged) {
      const r = await backup(env, TOKEN.author, validBackup({ toml_content }));
      assert.equal(r.status, 400, `${label} got ${r.status}`);
    }
    assert.equal(sent.length, 0, 'nothing went out');
    // The real file, with Windows line endings, still goes.
    assert.equal((await backup(env, TOKEN.author, validBackup({ toml_content: real.replace(/\n/g, '\r\n') }))).status, 200);
  });
});

test('the backup email is rate limited per account', async () => {
  const keys = [];
  const env = await environment({ AUTH_RATE_LIMITER: { limit: async ({ key }) => { keys.push(key); return { success: false }; } } });
  await withMailRecorder(async (sent) => {
    const r = await backup(env, TOKEN.author, validBackup());
    assert.equal(r.status, 429);
    assert.equal(r.headers.get('retry-after'), '60');
    assert.deepEqual(keys, [`identity-backup:${AUTHOR}`]);
    assert.equal(sent.length, 0);
  });
});

test('one inbox gets three identity emails a day, however many accounts name it', async () => {
  const env = await environment();
  // An attacker's account whose unverified address is the author's.
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other_user', public_key: PK.other, email: 'author@example.com' }));
  await withMailRecorder(async (sent) => {
    const mine = (who) => validBackup({ toml_content: identityFile({ username: who === 'author' ? 'author_user' : 'other_user', public_key: PK[who] }) });
    assert.equal((await backup(env, TOKEN.author, mine('author'))).status, 200);
    assert.equal((await backup(env, TOKEN.other, mine('other'))).status, 200);
    assert.equal((await backup(env, TOKEN.author, mine('author'))).status, 200);
    assert.equal(sent.length, 3);
    for (const who of ['author', 'other']) {
      const r = await backup(env, TOKEN[who], mine(who));
      assert.equal(r.status, 429, who);
      assert.equal(r.body.code, 'daily_limit');
    }
    assert.equal(sent.length, 3, 'the fourth never went out');
    // The address is kept only as a digest.
    assert.ok([...env.USERS.store.keys()].filter((k) => k.startsWith('mailcap:')).every((k) => !k.includes('author@example.com')));
  });
});

test('one account gets three identity emails a day, whatever address it names', async () => {
  const env = await environment();
  await withMailRecorder(async (sent) => {
    for (let i = 0; i < 3; i++) {
      await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author_user', public_key: PK.author, email: `address${i}@example.com` }));
      assert.equal((await backup(env, TOKEN.author, validBackup({ email: `address${i}@example.com` }))).status, 200);
    }
    await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author_user', public_key: PK.author, email: 'address9@example.com' }));
    const r = await backup(env, TOKEN.author, validBackup({ email: 'address9@example.com' }));
    assert.equal(r.status, 429);
    assert.equal(sent.length, 3);
  });
});

test('a failed send does not tell the caller which provider or what it said', async () => {
  const env = await environment();
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response('mailchannels internal: key abc123 rejected for domain', { status: 500 });
  try {
    const r = await backup(env, TOKEN.author, validBackup());
    assert.equal(r.status, 502);
    const text = new TextDecoder().decode(r.raw);
    assert.ok(!/mailchannels|abc123|internal/i.test(text), text);
  } finally { globalThis.fetch = real; }
});

// Registration sends the same email, from the same address, before anything has
// been verified, so it holds to the same rules.
function mailbox() {
  const messages = [];
  return { messages, send: async (message) => { messages.push(message); } };
}
const register = (n, extra = {}) => {
  const username = `reg_user_${n}`;
  const public_key = String(n).repeat(64);
  return { username, public_key, birthday: '1990-01-01', email: 'new@example.com', toml_content: identityFile({ username, public_key }), ...extra };
};
const registered = (r) => ({ ok: [200, 201].includes(r.status), mailed: r.body?.backup_emailed });

test('registration emails the file it was registered with, to the address it was given', async () => {
  const EMAIL = mailbox();
  const env = await environment({ EMAIL });
  const r = await call(env, 'POST', '/api/auth/register', { body: register(1) });
  assert.deepEqual(registered(r), { ok: true, mailed: true }, JSON.stringify(r.body));
  assert.equal(EMAIL.messages.length, 1);
  assert.equal(EMAIL.messages[0].to, 'new@example.com');
  assert.match(EMAIL.messages[0].raw, /^To: new@example\.com\r\n/m);
});

test('registration is not a mail relay: a file that is not its own, or a second send to one inbox, goes nowhere', async () => {
  const EMAIL = mailbox();
  const env = await environment({ EMAIL });
  const mailed = async (n, extra) => {
    const r = await call(env, 'POST', '/api/auth/register', { body: register(n, extra) });
    assert.ok(registered(r).ok, `registration itself still works: ${JSON.stringify(r.body)}`);
    return EMAIL.messages.length;
  };
  const free = 'new@example.com';
  // Text a caller wrote, a file for another account, an oversized one.
  assert.equal(await mailed(1, { toml_content: '# Your account is locked, call 555-0100\n'.repeat(5) }), 0);
  assert.equal(await mailed(2, { toml_content: identityFile({ username: 'someone_else', public_key: '2'.repeat(64) }) }), 0);
  assert.equal(await mailed(3, { toml_content: register(3).toml_content + '# ' + 'x'.repeat(32 * 1024) }), 0);
  // An address that could carry a second recipient.
  assert.equal(await mailed(4, { email: `${free}\r\nBcc: victim@example.com` }), 0);
  // Three real ones reach the inbox; the fourth account naming it is not mailed.
  assert.equal(await mailed(5, { email: free }), 1);
  assert.equal(await mailed(6, { email: free }), 2);
  assert.equal(await mailed(7, { email: free }), 3);
  assert.equal(await mailed(8, { email: free }), 3, 'a fourth to one inbox');
  assert.equal(await mailed(9, { email: 'another@example.com' }), 4, 'a different inbox is unaffected');
});

test('registration keeps only a plain address, and stores it in lower case', async () => {
  const env = await environment();
  const bad = await call(env, 'POST', '/api/auth/register', { body: register(1, { email: 'new@example.com\r\nBcc: victim@example.com' }) });
  assert.ok(registered(bad).ok, JSON.stringify(bad.body));
  assert.equal(JSON.parse(await env.USERS.get(`user:${bad.body.user.id}`)).email, null);

  const good = await call(env, 'POST', '/api/auth/register', { body: register(2, { email: ' New@Example.COM ' }) });
  assert.ok(registered(good).ok, JSON.stringify(good.body));
  assert.equal(JSON.parse(await env.USERS.get(`user:${good.body.user.id}`)).email, 'new@example.com');
});

test('registration takes a public key only as 64 hex characters', async () => {
  const env = await environment();
  const phishing = 'Your account is locked, sign in again at https://evil.example/login to keep it';
  for (const [label, public_key] of [
    ['a sentence', phishing],
    ['a sentence long enough to pass the old length check', phishing.padEnd(424, '.')],
    ['63 characters', 'a'.repeat(63)],
    ['65 characters', 'a'.repeat(65)],
    ['not hex', 'g'.repeat(64)],
    ['a number', 12345678901234567890123456789012345678901234567890],
    ['a list', ['a'.repeat(64)]],
    ['absent', undefined],
  ]) {
    const r = await call(env, 'POST', '/api/auth/register', { body: { username: 'reg_user_k', public_key, birthday: '1990-01-01' } });
    assert.equal(r.status, 400, `${label} got ${r.status}`);
    assert.equal(r.body.error, 'Invalid public key', label);
  }
  assert.equal(await env.USERS.get('username:reg_user_k'), null, 'nothing was created');
  const good = await call(env, 'POST', '/api/auth/register', { body: { username: 'reg_user_k', public_key: 'AbCdEf0123456789'.repeat(4), birthday: '1990-01-01' } });
  assert.ok([200, 201].includes(good.status), JSON.stringify(good.body));
});

/// A real Ed25519 key pair: the key as the page sends it (64 hex), and a signer.
function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

async function signIn(env, key, spelling) {
  const challenge = await call(env, 'POST', '/api/auth/challenge', { body: { public_key: spelling } });
  if (challenge.status !== 200) return challenge;
  return call(env, 'POST', '/api/auth/verify-challenge', { body: { format: 'eustress-login-v1', public_key: spelling, challenge: challenge.body.challenge, signature: key.sign(['eustress-login-v1', 'https://api.eustress.dev', challenge.body.challenge].join(String.fromCharCode(10))) } });
}

test('one key is one account, however its letters are cased', async () => {
  const env = await environment({ CHALLENGES: fakeKv() });
  const key = keyPair();
  const body = (username, public_key) => ({ username, public_key, birthday: '1990-01-01' });
  const first = await call(env, 'POST', '/api/auth/register', { body: body('case_user_a', key.hex.toLowerCase()) });
  assert.ok([200, 201].includes(first.status), JSON.stringify(first.body));
  for (const [i, spelling] of [key.hex.toUpperCase(), key.hex.slice(0, 20).toUpperCase() + key.hex.slice(20)].entries()) {
    const second = await call(env, 'POST', '/api/auth/register', { body: body(`case_user_b${i}`, spelling) });
    assert.equal(second.status, 409, `${spelling.slice(0, 8)}... got ${second.status}`);
  }
  // And the same the other way round.
  const other = keyPair();
  assert.ok([200, 201].includes((await call(env, 'POST', '/api/auth/register', { body: body('case_user_c', other.hex.toUpperCase()) })).status));
  assert.equal((await call(env, 'POST', '/api/auth/register', { body: body('case_user_d', other.hex.toLowerCase()) })).status, 409);
  // It is stored in one spelling.
  assert.equal(await env.USERS.get(`pubkey:${other.hex.toLowerCase()}`) !== null, true);
  assert.equal(await env.USERS.get(`pubkey:${other.hex.toUpperCase()}`), null);
});

test('signing in works with the key in either case, and only a real key gets a challenge', async () => {
  const env = await environment({ CHALLENGES: fakeKv() });
  const key = keyPair();
  const registered = await call(env, 'POST', '/api/auth/register', { body: { username: 'case_user_e', public_key: key.hex, birthday: '1990-01-01' } });
  assert.ok([200, 201].includes(registered.status), JSON.stringify(registered.body));
  for (const spelling of [key.hex, key.hex.toUpperCase()]) {
    const r = await signIn(env, key, spelling);
    assert.equal(r.status, 200, `${spelling.slice(0, 8)}... got ${r.status} ${JSON.stringify(r.body)}`);
    assert.equal(r.body.user.username, 'case_user_e');
  }
  // An account filed under an upper-case spelling, from before keys were lower-cased.
  const legacy = keyPair();
  const upper = legacy.hex.toUpperCase();
  await env.USERS.put(`pubkey:${upper}`, AUTHOR);
  assert.equal((await signIn(env, legacy, upper)).status, 200);
  // The lower-case spelling of that same key cannot register a second account
  // and take its sign-ins.
  const taken = await call(env, 'POST', '/api/auth/register', { body: { username: 'case_user_f', public_key: legacy.hex.toLowerCase(), birthday: '1990-01-01' } });
  assert.equal(taken.status, 409);
  assert.equal((await signIn(env, legacy, upper)).body.user.id, AUTHOR, 'the legacy owner still reaches their own account');
  // Junk gets no stored challenge.
  for (const public_key of ['x'.repeat(64), 'a'.repeat(63), 'a'.repeat(600), 42]) {
    assert.equal((await call(env, 'POST', '/api/auth/challenge', { body: { public_key } })).status, 400);
  }
  assert.equal([...env.CHALLENGES.store.keys()].some((k) => !/^login-challenge:[0-9a-f]{64}$/.test(k)), false, 'every stored challenge is filed under a challenge');
});

test('the identity file names the account\'s key whatever the case of its letters', async () => {
  const env = await environment();
  await withMailRecorder(async (sent) => {
    const upper = identityFile({ username: 'author_user', public_key: PK.author.toUpperCase() });
    assert.equal((await backup(env, TOKEN.author, validBackup({ toml_content: upper }))).status, 200);
    assert.equal(sent.length, 1);
    // Still not another key.
    const elsewhere = identityFile({ username: 'author_user', public_key: PK.other.toUpperCase() });
    assert.equal((await backup(env, TOKEN.author, validBackup({ toml_content: elsewhere }))).status, 400);
  });
});

test('an account whose stored key is not a key gets no identity email', async () => {
  // An account registered before the key shape was enforced.
  const env = await environment();
  const phishing = 'Your account is locked, sign in again at https://evil.example/login to keep it';
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author_user', public_key: phishing, email: 'author@example.com' }));
  await withMailRecorder(async (sent) => {
    const r = await backup(env, TOKEN.author, validBackup({ toml_content: identityFile({ username: 'author_user', public_key: phishing }) }));
    assert.equal(r.status, 400);
    assert.equal(sent.length, 0);
  });
});

test('every kind of ID the page offers is accepted, the "+" in "18+ photo ID" included', async () => {
  const env = await environment();
  // Each label becomes lower case with spaces as "_" and apostrophes dropped.
  const labels = ['Passport', "Driver's License", 'National ID Card', '18+ Photo ID', 'Cédula de Identidad', 'Resident Permit (EU)', 'ID/Voter Card'];
  await withMailRecorder(async (sent) => {
    for (const [i, label] of labels.entries()) {
      const id_type = label.toLowerCase().replace(/ /g, '_').replace(/'/g, '');
      const toml_content = identityFile({ username: 'author_user', public_key: PK.author }).replace('id_type = "passport"', `id_type = "${id_type}"`);
      // A fresh day-count for each, so the allowance is not what is under test.
      for (const k of [...env.USERS.store.keys()]) if (k.startsWith('mailcap:')) env.USERS.store.delete(k);
      const r = await backup(env, TOKEN.author, validBackup({ toml_content }));
      assert.equal(r.status, 200, `${label} (${id_type}) got ${r.status}`);
    }
    assert.equal(sent.length, labels.length);
  });
});

test('the file the sign-up page builds is accepted exactly as it builds it', async () => {
  // Copied from generate_identity_toml in web/src/pages/login.rs. If that file
  // changes shape, this test and the Worker's check change with it. It carries
  // the account, the keys and `verified`: no birthday, ID type or ID hash.
  const fromThePage = `# Eustress Identity — generated 2026-10-01T18:42:11.123456789+00:00
# This file IS your account. Keep it safe. Do not share your private key.

[identity]
version = "1.0"
user_id = "3f2b8a52-9c1e-4d7a-8b0f-6a5e4c3d2b1a"
username = "author_user"
public_key = "${PK.author}"
private_key = "${'0123456789abcdef'.repeat(4)}"
issued_at = "2026-10-01T18:42:11.123456789+00:00"

[verification]
verified = false
`;
  const env = await environment();
  await withMailRecorder(async (sent) => {
    const r = await backup(env, TOKEN.author, validBackup({ toml_content: fromThePage }));
    assert.equal(r.status, 200, JSON.stringify(r.body));
    assert.equal(sent.length, 1);
    // With CRLF line endings too, as a file saved on Windows may have.
    assert.equal((await backup(env, TOKEN.author, validBackup({ toml_content: fromThePage.replace(/\n/g, '\r\n') }))).status, 200);
  });
});

test('a file issued before the ID data was taken out is still accepted, so the Worker and the site can deploy in either order', async () => {
  const fromTheOldPage = `# Eustress Identity — generated 2026-09-30T18:42:11.123456789+00:00
# This file IS your account. Keep it safe. Do not share your private key.

[identity]
version = "1.0"
user_id = "3f2b8a52-9c1e-4d7a-8b0f-6a5e4c3d2b1a"
username = "author_user"
public_key = "${PK.author}"
private_key = "${'0123456789abcdef'.repeat(4)}"
issued_at = "2026-09-30T18:42:11.123456789+00:00"

[profile]
birthday = "1990-01-01"

[verification]
id_type = "drivers_license"
id_hash = "${'ab'.repeat(32)}"
verified = false
`;
  const env = await environment();
  await withMailRecorder(async (sent) => {
    const r = await backup(env, TOKEN.author, validBackup({ toml_content: fromTheOldPage }));
    assert.equal(r.status, 200, JSON.stringify(r.body));
    assert.equal(sent.length, 1);
  });
});


// ── Thumbnails ───────────────────────────────────────────────────────────────

test('a thumbnail must be an image by its own bytes, whatever the request calls it', async () => {
  const env = await environment();
  await seedListing(env, 'approved');
  const put = (body, type) => call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { token: TOKEN.author, body, headers: { 'Content-Type': type } });

  const page = await put(HTML, 'image/png');
  assert.equal(page.status, 415);
  assert.equal([...env.SCENES.objects.keys()].filter((k) => k.startsWith('thumbnails/')).length, 0, 'nothing was stored');
  assert.equal((await put(new TextEncoder().encode('%PDF-1.7 not an image'), 'image/png')).status, 415);

  assert.equal((await put(PNG, 'text/html')).status, 200, 'the label does not matter, the bytes do');
  const stored = env.SCENES.objects.get(`thumbnails/${SIM}/thumb.png`);
  assert.equal(stored.meta.httpMetadata.contentType, 'image/png');
  assert.equal((await put(WEBP, 'application/octet-stream')).status, 200);
  assert.ok(env.SCENES.objects.has(`thumbnails/${SIM}/thumb.webp`));
});

test('a thumbnail is served as an image that cannot run as a page', async () => {
  const env = await environment();
  await seedListing(env, 'approved');
  // Rated all_ages: a listing with no rating is adult content, whose thumbnail is served on a signed URL only (src/adult_content.mjs).
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { ...sim.moderation, rating: 'all_ages' } }));
  // Written by the old upload route: its author chose the type.
  await env.SCENES.put(`thumbnails/${SIM}/thumb.png`, HTML, { httpMetadata: { contentType: 'text/html' } });
  const r = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.equal(r.status, 200);
  assert.equal(r.headers.get('content-type'), 'image/png');
  assert.equal(r.headers.get('x-content-type-options'), 'nosniff');
  assert.match(r.headers.get('content-security-policy'), /sandbox/);
});

// ── A new package goes back to review ────────────────────────────────────────

test('re-uploading the package into an approved listing takes the approval away until it is judged again', async () => {
  for (const [label, run] of [
    ['whole-package upload', (env) => call(env, 'PUT', `/api/simulations/${SIM}/space`, { token: TOKEN.author, body: new Uint8Array([9, 9, 9, 9]) })],
    ['multipart complete', (env) => call(env, 'POST', `/api/simulations/${SIM}/space/multipart/complete`, { token: TOKEN.author, body: { upload_id: 'upload-x', parts: [{ part_number: 1, etag: 'part-1' }], total_size: 3 } })],
    ['single-space upload', (env) => call(env, 'PUT', `/api/simulations/${SIM}/spaces/Level1`, { token: TOKEN.author, body: new Uint8Array([9, 9, 9, 9]) })],
  ]) {
    const env = await environment();
    await seedListing(env, 'approved');
    const r = await run(env);
    assert.equal(r.status, 200, `${label}: ${JSON.stringify(r.body)}`);
    assert.equal(await status(env), 'pending', `${label}: the new package was not the one judged`);
    // Not listed, and not served to a stranger, until it passes again.
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { token: TOKEN.other })).status === 200, false, label);
  }
});

test('an upload does not lift a quarantine', async () => {
  for (const [label, run] of [
    ['whole-package upload', (env) => call(env, 'PUT', `/api/simulations/${SIM}/space`, { token: TOKEN.author, body: new Uint8Array([9, 9, 9, 9]) })],
    ['multipart complete', (env) => call(env, 'POST', `/api/simulations/${SIM}/space/multipart/complete`, { token: TOKEN.author, body: { upload_id: 'upload-x', parts: [{ part_number: 1, etag: 'part-1' }], total_size: 3 } })],
    ['single-space upload', (env) => call(env, 'PUT', `/api/simulations/${SIM}/spaces/Level1`, { token: TOKEN.author, body: new Uint8Array([9, 9, 9, 9]) })],
  ]) {
    const env = await environment();
    await seedListing(env, 'quarantined');
    await run(env);
    assert.equal(await status(env), 'quarantined', label);
  }
});

// ── A ban reaches the token the account holds ────────────────────────────────

test('banning an account ends its signed-in sessions at once, and nobody else\'s', async () => {
  const env = await environment();
  const puts = [];
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (key, value, options) => { puts.push({ key, options }); return realPut(key, value, options); };

  assert.equal((await call(env, 'GET', '/api/auth/me', { token: TOKEN.author })).status, 200);
  const ban = await call(env, 'POST', '/api/admin/ban', { token: TOKEN.admin, body: { username: 'author_user', reason: 'testing' } });
  assert.equal(ban.status, 200, JSON.stringify(ban.body));

  assert.equal((await call(env, 'GET', '/api/auth/me', { token: TOKEN.author })).status, 401, 'the banned account\'s old token is dead');
  assert.equal((await call(env, 'GET', '/api/auth/me', { token: TOKEN.other })).status, 200, 'another account is untouched');
  assert.equal((await call(env, 'GET', '/api/auth/me', { token: TOKEN.admin })).status, 200);

  // The marker outlives the longest token (72 hours) and then lapses.
  const marker = puts.find((p) => p.key === `authfloor:${AUTHOR}`);
  assert.ok(marker, 'a marker was written');
  assert.ok(marker.options.expirationTtl >= 72 * 3600, `ttl ${marker.options.expirationTtl}`);
});

test('only an admin bans: an ordinary account bans nobody', async () => {
  const env = await environment();
  const r = await call(env, 'POST', '/api/admin/ban', { token: TOKEN.other, body: { username: 'author_user' } });
  assert.ok([401, 403].includes(r.status));
  assert.equal(await env.USERS.get(`authfloor:${AUTHOR}`), null);
  assert.equal((await call(env, 'GET', '/api/auth/me', { token: TOKEN.author })).status, 200);
});

// ── The downloads Worker ─────────────────────────────────────────────────────

const downloads = await (async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'eustress-downloads-'));
  process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));
  const file = path.join(tmp, 'download-worker.mjs');
  fs.copyFileSync(path.resolve(import.meta.dirname, '..', '..', 'download-worker.js'), file);
  return (await import(pathToFileURL(file).href)).default;
})();

const b64url = (b) => Buffer.from(b).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
function signToken(payload, { secret = JWT_SECRET, header = { alg: 'HS256', typ: 'JWT' } } = {}) {
  const h = b64url(JSON.stringify(header));
  const p = b64url(JSON.stringify(payload));
  return `${h}.${p}.${b64url(crypto.createHmac('sha256', secret).update(`${h}.${p}`).digest())}`;
}
const NOW = Math.floor(Date.now() / 1000);

async function releases(options = {}) {
  const url = options.url ?? 'https://downloads.eustress.dev/v1.2.3/eustress-windows-x64.zip';
  const secret = 'secret' in options ? options.secret : JWT_SECRET;
  const DOWNLOADS = fakeBucket();
  await DOWNLOADS.put('latest.json', JSON.stringify({ version: '1.2.3', platforms: { 'windows-x64': { url, file: 'eustress-windows-x64.zip' } } }));
  await DOWNLOADS.put('v1.2.3/eustress-windows-x64.zip', 'the installer bytes');
  await DOWNLOADS.put('secret/private.txt', 'not a release');
  return { DOWNLOADS, ...(secret === undefined ? {} : { JWT_SECRET: secret }) };
}

async function download(env, { token, query = '', cookie } = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (cookie) headers.Cookie = `auth_token=${cookie}`;
  const res = await downloads.fetch(new Request(`https://downloads.eustress.dev/api/releases/download?platform=windows-x64${query}`, { headers }), env, ctx);
  return { status: res.status, text: await res.text() };
}

test('downloads: a signed, unexpired token gets the build', async () => {
  const env = await releases();
  const r = await download(env, { token: signToken({ sub: AUTHOR, exp: NOW + 3600 }) });
  assert.equal(r.status, 200);
  assert.equal(r.text, 'the installer bytes');
  assert.equal((await download(env, { cookie: signToken({ sub: AUTHOR, exp: NOW + 3600 }) })).status, 200, 'the cookie still works');
});

test('downloads: with no secret configured nothing is accepted, not everything', async () => {
  const env = await releases({ secret: undefined });
  for (const token of [signToken({ sub: AUTHOR, exp: NOW + 3600 }), signToken({ sub: 'anyone', exp: NOW + 3600 }, { secret: 'whatever' }), 'a.b.c']) {
    assert.equal((await download(env, { token })).status, 403);
  }
  assert.equal((await download(await releases({ secret: '' }), { token: signToken({ sub: AUTHOR, exp: NOW + 3600 }, { secret: '' }) })).status, 403);
});

test('downloads: a forged, unsigned or never-expiring token is refused', async () => {
  const env = await releases();
  const good = signToken({ sub: AUTHOR, exp: NOW + 3600 });
  const [h, p] = good.split('.');
  for (const [label, token] of [
    ['wrong secret', signToken({ sub: AUTHOR, exp: NOW + 3600 }, { secret: 'not-the-secret' })],
    ['no signature', `${h}.${p}.`],
    ['alg none', `${b64url('{"alg":"none"}')}.${p}.`],
    ['other algorithm', signToken({ sub: AUTHOR, exp: NOW + 3600 }, { header: { alg: 'HS512', typ: 'JWT' } })],
    ['no expiry', signToken({ sub: AUTHOR })],
    ['expiry as text', signToken({ sub: AUTHOR, exp: 'never' })],
    ['expired', signToken({ sub: AUTHOR, exp: NOW - 10 })],
    ['payload swapped', `${h}.${b64url(JSON.stringify({ sub: 'someone-else', exp: NOW + 3600 }))}.${good.split('.')[2]}`],
  ]) {
    assert.equal((await download(env, { token })).status, 403, label);
  }
});

test('downloads: a token in the URL is not read', async () => {
  const env = await releases();
  const r = await download(env, { query: `&token=${signToken({ sub: AUTHOR, exp: NOW + 3600 })}` });
  assert.equal(r.status, 401);
});

test('downloads: a manifest entry that is not a release artifact reads no other key', async () => {
  for (const url of ['https://downloads.eustress.dev/secret/private.txt', 'https://downloads.eustress.dev/latest.json', 'https://downloads.eustress.dev/v1.2.3/../secret/private.txt', 'https://downloads.eustress.dev/v1.2.3/a/b.zip']) {
    const env = await releases({ url });
    const r = await download(env, { token: signToken({ sub: AUTHOR, exp: NOW + 3600 }) });
    assert.equal(r.status, 404, url);
    assert.ok(!r.text.includes('not a release'), url);
  }
});
