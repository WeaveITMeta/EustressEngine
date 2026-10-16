// Passkeys through the real routes: a software authenticator (real keys, real
// CBOR, real signatures) adds a passkey with the identity file's signature, signs
// in with it, and is refused for every way an answer can be wrong. A passkey
// session can do everything but change keys. The real Worker and the real
// account object run against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import { SoftAuthenticator, cbor, b64url, fromB64url, authenticatorData } from './helpers/soft_authenticator.mjs';
import {
  AccountAuth, accountCall, accountState, rotationMessage, passkeyAddMessage, passkeyRemoveMessage, PASSKEY_LOGIN_SEEN_MAX,
  adminReplaceKeyMessage, verifyAuditChain, MAX_PASSKEYS,
} from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { PasskeyIndex, passkeyIndexCall, passkeyIndexName, ORPHAN_GRACE_MS } from '../src/passkey_index.mjs';
import { decodeCbor, decodeCborItem, derToRawEcdsa, fromBase64url, passkeyConfig, WebauthnError } from '../src/webauthn.mjs';

const worker = await loadWorker();
const OWNER = '13131313-1313-4313-8313-131313131313';
const OTHER = '24242424-2424-4424-8424-242424242424';
const ADMIN = '35353535-3535-4535-8535-353535353535';
const ORIGIN = 'https://api.eustress.dev';
const SITE = 'https://eustress.dev';
const ALGS = [['ES256', -7], ['EdDSA', -8], ['RS256', -257]];

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}
class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
const bodyOf = (m) => [...m.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
  .map((x) => Buffer.from(x[1].replace(/\r\n/g, ''), 'base64').toString()).join('\n');
const claims = (token) => JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString());
const hexOf = (b64) => Buffer.from(b64, 'base64url').toString('hex');

/// A session token stamped at `iat`, as the Worker stamps the one it issues at a floor.
function tokenAt(sub, iat) {
  const b = (v) => Buffer.from(v).toString('base64url');
  const h = b(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const p = b(JSON.stringify({ sub, iat, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${h}.${p}.${crypto.createHmac('sha256', JWT_SECRET).update(`${h}.${p}`).digest('base64url')}`;
}

async function environment(extra = {}) {
  const sent = [];
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN,
    PASSKEY_RP_ID: 'eustress.dev', PASSKEY_ORIGINS: SITE, ADMIN_REPLACE_KEY_ENABLED: 'true',
    EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.PASSKEY_INDEX = fakeDoNamespace(PasskeyIndex, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  const keys = {};
  for (const [id, name] of [[OWNER, 'owner'], [OTHER, 'other'], [ADMIN, 'admin_user']]) {
    const k = keyPair();
    keys[id] = k;
    // Verified people, as every account that writes must be (the KYC gate answers 403 kyc_required otherwise).
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name, public_key: k.hex, email: `${name}@example.com` }));
    await env.USERS.put(`username:${name}`, id);
    await env.USERS.put(`pubkey:${k.hex}`, id);
    await accountState(env, id);
  }
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return { env, sent, keys, k0: keys[OWNER], token: mintToken(OWNER) };
}

async function send(env, method, route, { token, body, origin = ORIGIN } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${origin}${route}`, { method, headers, body: method === 'GET' ? undefined : JSON.stringify(body ?? {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}
const post = (env, route, body, token, origin) => send(env, 'POST', route, { token, body, origin });

/// begin, create the credential, sign the add message with the identity key, add.
/// `bend` changes the authenticator's answer; `mutate` changes the request.
async function addPasskey(env, token, key, auth, { account = OWNER, bend = {}, mutate, name = 'Laptop', begun } = {}) {
  const options = begun || (await post(env, '/api/passkeys/add/begin', {}, token));
  if (!begun) assert.equal(options.status, 200, JSON.stringify(options.body));
  const credential = auth.register(options.body, bend);
  const message = passkeyAddMessage({ origin: ORIGIN, accountId: account, credentialId: credential.id, challenge: hexOf(options.body.challenge) });
  const request = { challenge: options.body.challenge, name, credential, signature: key.sign(message) };
  if (mutate) mutate(request, { message, options: options.body });
  return post(env, '/api/passkeys/add', request, token);
}

/// login/begin, the authenticator's assertion, login.
async function signIn(env, auth, { bend = {}, mutate, begun } = {}) {
  const options = begun || (await post(env, '/api/passkeys/login/begin', {}));
  if (!begun) assert.equal(options.status, 200, JSON.stringify(options.body));
  const credential = auth.assert(options.body, bend);
  const request = { challenge: options.body.challenge, credential };
  if (mutate) mutate(request, options.body);
  return { ...(await post(env, '/api/passkeys/login', request)), request, options: options.body };
}

/// A real passkey session for `account`.
async function passkeySession(env, token, key, { alg = -7, account = OWNER } = {}) {
  const auth = new SoftAuthenticator(alg);
  const added = await addPasskey(env, token, key, auth, { account });
  assert.equal(added.status, 200, JSON.stringify(added.body));
  const signed = await signIn(env, auth);
  assert.equal(signed.status, 200, JSON.stringify(signed.body));
  return { auth, token: signed.body.token };
}

/// The account the passkey index says holds a credential id, or null.
const ownerOf = async (env, id) => (await passkeyIndexCall(env, id, 'owner')).account_id;

const objectState = async (env, id = OWNER) => env.ACCOUNT_AUTH.instances.get(id).storage.get('state');

// ── Add, then sign in, for each algorithm ───────────────────────────────────

for (const [label, alg] of ALGS) {
  test(`${label}: a passkey is added with the identity file, then signs in to the website with no file`, async () => {
    const { env, sent, k0, token } = await environment();
    const auth = new SoftAuthenticator(alg);

    const begun = await post(env, '/api/passkeys/add/begin', {}, token);
    assert.equal(begun.status, 200, JSON.stringify(begun.body));
    assert.equal(fromB64url(begun.body.challenge).length, 32);
    assert.deepEqual(begun.body.rp, { id: 'eustress.dev', name: 'Eustress' });
    assert.equal(Buffer.from(begun.body.user.id, 'base64url').toString(), OWNER, 'the user handle is the account id');
    assert.equal(begun.body.user.name, 'owner');
    assert.deepEqual(begun.body.pub_key_cred_params, [-8, -7, -257]);
    assert.deepEqual(begun.body.exclude_credentials, []);
    assert.equal(begun.body.timeout_ms, 300000);
    assert.deepEqual(begun.body.authenticator_selection, { resident_key: 'required', user_verification: 'required' });
    assert.equal(begun.body.attestation, 'none');
    assert.equal(begun.body.api_origin, ORIGIN);

    const added = await addPasskey(env, token, k0, auth, { begun, name: '  Work\nlaptop  ' });
    assert.equal(added.status, 200, JSON.stringify(added.body));
    assert.equal(added.body.ok, true);
    assert.equal(added.body.passkey.id, auth.id);
    assert.equal(added.body.passkey.alg, alg);
    assert.equal(added.body.passkey.name, 'Work laptop', 'the name is cleaned like other display text');
    assert.equal(await ownerOf(env, auth.id), OWNER, 'the passkey index names the account');
    // Every address is told, and the notice names the passkey.
    assert.equal(sent.length, 1);
    assert.equal(sent[0].to, 'owner@example.com');
    assert.match(bodyOf(sent[0]), /A passkey named "Work laptop" was added/);
    const held = await objectState(env);
    assert.equal(held.notices_pending.length, 0, 'the route said the notice went');

    const list = await send(env, 'GET', '/api/passkeys', { token });
    assert.equal(list.status, 200);
    assert.equal(list.headers.get('Cache-Control'), 'private, no-store');
    assert.deepEqual(list.body.passkeys.map((p) => [p.id, p.name, p.alg, p.last_used_at]), [[auth.id, 'Work laptop', alg, null]]);

    const loginBegun = await post(env, '/api/passkeys/login/begin', {});
    assert.equal(loginBegun.status, 200);
    assert.deepEqual(Object.keys(loginBegun.body).sort(), ['challenge', 'rp_id', 'timeout_ms', 'user_verification']);
    assert.equal(loginBegun.body.rp_id, 'eustress.dev');
    assert.equal(loginBegun.body.user_verification, 'required');

    const signed = await signIn(env, auth, { begun: loginBegun });
    assert.equal(signed.status, 200, JSON.stringify(signed.body));
    assert.equal(signed.body.user.id, OWNER);
    assert.equal(signed.body.amr, 'passkey');
    assert.equal(claims(signed.body.token).amr, 'passkey');
    assert.equal(claims(signed.body.token).sub, OWNER);
    const me = await send(env, 'GET', '/api/auth/me', { token: signed.body.token });
    assert.equal(me.status, 200);
    assert.equal(me.body.auth_method, 'passkey');

    const after = (await send(env, 'GET', '/api/passkeys', { token: signed.body.token })).body.passkeys[0];
    assert.ok(after.last_used_at, 'the time of use is kept');
    assert.equal(after.backed_up, true);
    assert.equal((await objectState(env)).passkeys[0].sign_count, 1, 'the sign count is kept');
    assert.ok(await verifyAuditChain((await accountCall(env, OWNER, 'audit', { limit: 50 })).records));
  });
}

test('a synced passkey whose sign count is always 0 signs in every time', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7, { countsUp: false });
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  for (let i = 0; i < 3; i++) assert.equal((await signIn(env, auth)).status, 200);
});

test('an identity-file session says so on /api/auth/me, with amr "key" or none', async () => {
  const { env, token } = await environment();
  assert.equal((await send(env, 'GET', '/api/auth/me', { token })).body.auth_method, 'key', 'a token without amr is a key session');
});

// ── The site origin, the RP, the flags ───────────────────────────────────────

test('a passkey made or used on another site, for another RP id, or without UP or UV is refused', async () => {
  const { env, k0, token } = await environment();
  const cases = [
    [{ origin: 'https://evil.example' }, 403, 'passkey_origin_not_allowed'],
    [{ origin: 'https://staging.eustress-cbb.pages.dev' }, 403, 'passkey_origin_not_allowed'],
    [{ origin: 'http://eustress.dev' }, 403, 'passkey_origin_not_allowed'],
    [{ rpId: 'evil.example' }, 403, 'rp_id_mismatch'],
    [{ rpId: 'api.eustress.dev' }, 403, 'rp_id_mismatch'],
    [{ flags: { uv: false } }, 403, 'user_verification_required'],
    [{ flags: { up: false } }, 403, 'user_presence_required'],
    [{ type: 'webauthn.get' }, 400, 'client_data_type'],
    [{ challenge: b64url(crypto.randomBytes(32)) }, 400, 'challenge_mismatch'],
    [{ clientExtra: { crossOrigin: true, topOrigin: 'https://evil.example' } }, 403, 'cross_origin'],
  ];
  for (const [bend, status, code] of cases) {
    const auth = new SoftAuthenticator(-7);
    const r = await addPasskey(env, token, k0, auth, { bend });
    assert.equal(r.status, status, `${JSON.stringify(bend)}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, code, JSON.stringify(bend));
    assert.equal(await ownerOf(env, auth.id), null);
  }
  assert.deepEqual((await objectState(env)).passkeys || [], [], 'nothing was added');

  // The same refusals at sign-in.
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  for (const [bend, status, code] of cases) {
    const r = await signIn(env, auth, { bend: bend.type ? { type: 'webauthn.create' } : bend });
    assert.equal(r.status, status, `${JSON.stringify(bend)}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, code, JSON.stringify(bend));
  }
  assert.equal((await signIn(env, auth)).status, 200, 'and the passkey itself still works');
});

test('the attested credential id must be the raw_id, and id and raw_id must agree', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const other = b64url(crypto.randomBytes(32));
  let r = await addPasskey(env, token, k0, auth, { bend: { credentialId: fromB64url(other) } });
  assert.equal(r.body.code, 'credential_id_mismatch');
  r = await addPasskey(env, token, k0, auth, { bend: { id: auth.id, raw_id: other } });
  assert.equal(r.body.code, 'bad_request');
  r = await addPasskey(env, token, k0, auth, { bend: { id: b64url(crypto.randomBytes(15)) } });
  assert.equal(r.body.code, 'bad_request', 'a credential id under 16 bytes');
});

test('a key that is not one of the three offered, or is too weak, is refused', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const x = auth.key.cose.get(-2);
  const y = auth.key.cose.get(-3);
  const bad = [
    new Map([[1, 2], [3, -35], [-1, 2], [-2, x], [-3, y]]), // ES384
    new Map([[1, 2], [3, -7], [-1, 1], [-2, x]]), // no y
    new Map([[1, 2], [3, -7], [-1, 1], [-2, x], [-3, new Uint8Array(32)]]), // not on the curve
    new Map([[1, 1], [3, -8], [-1, 6], [-2, fromB64url('AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA')]]), // small order
    new Map([[1, 3], [3, -257], [-1, new Uint8Array(128).fill(0xff)], [-2, new Uint8Array([1, 0, 1])]]), // RSA 1024
  ];
  for (const cose of bad) {
    const r = await addPasskey(env, token, k0, auth, { bend: { cose } });
    assert.equal(r.status, 400, JSON.stringify(r.body));
    assert.equal(r.body.code, 'key_unsupported');
  }
});

// ── Challenges ───────────────────────────────────────────────────────────────

test('a sign-in challenge is used once: a replay is refused, even one that raced past the KV copy', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7, { countsUp: false });
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const first = await signIn(env, auth);
  assert.equal(first.status, 200);
  const again = await post(env, '/api/passkeys/login', first.request);
  assert.equal(again.status, 400);
  assert.equal(again.body.code, 'challenge_invalid');

  // Two requests that both read the KV challenge before either deleted it: the
  // account's object still lets the signed answer sign in once.
  const begun = await post(env, '/api/passkeys/login/begin', {});
  const filed = [...env.CHALLENGES.store.entries()].find(([k]) => k.startsWith('pkchal:'));
  const ok = await signIn(env, auth, { begun });
  assert.equal(ok.status, 200);
  env.CHALLENGES.store.set(filed[0], filed[1]);
  const raced = await post(env, '/api/passkeys/login', ok.request);
  assert.equal(raced.status, 400);
  assert.equal(raced.body.code, 'challenge_invalid');
});

test('a wrong first try spends the add challenge, and a retry with it is refused', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const begun = await post(env, '/api/passkeys/add/begin', {}, token);
  const wrong = await addPasskey(env, token, k0, auth, { begun, bend: { origin: 'https://evil.example' } });
  assert.equal(wrong.body.code, 'passkey_origin_not_allowed');
  const retry = await addPasskey(env, token, k0, auth, { begun });
  assert.equal(retry.status, 400);
  assert.equal(retry.body.code, 'challenge_invalid');
});

test('a challenge from another account, or for another purpose, does not add a passkey or sign in', async () => {
  const { env, keys, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  // Another account's add challenge.
  const theirs = await post(env, '/api/passkeys/add/begin', {}, mintToken(OTHER));
  let r = await addPasskey(env, token, k0, auth, { begun: theirs });
  assert.equal(r.body.code, 'challenge_invalid');
  // The other account cannot use it either now: it was not spent there, but the owner's signature names the owner.
  // A remove challenge of this account.
  const keeper = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, keeper)).status, 200);
  const removeBegun = await post(env, '/api/passkeys/remove/begin', { id: keeper.id }, token);
  assert.equal(removeBegun.status, 200);
  r = await addPasskey(env, token, k0, auth, { begun: { body: { ...theirs.body, challenge: removeBegun.body.challenge } } });
  assert.equal(r.body.code, 'challenge_invalid');
  // A rotation challenge, re-spelled as base64url.
  const rotation = await post(env, '/api/identity/rotate/begin', {}, token);
  r = await addPasskey(env, token, k0, auth, { begun: { body: { ...theirs.body, challenge: b64url(Buffer.from(rotation.body.challenge, 'hex')), user: { id: b64url(Buffer.from(OWNER)) }, rp: { id: 'eustress.dev' } } } });
  assert.equal(r.body.code, 'challenge_invalid');
  // A sign-in challenge (KV) is not an add challenge (the object's).
  const loginBegun = await post(env, '/api/passkeys/login/begin', {});
  r = await addPasskey(env, token, k0, auth, { begun: { body: { ...theirs.body, challenge: loginBegun.body.challenge } } });
  assert.equal(r.body.code, 'challenge_invalid');
  // And an add challenge is not a sign-in challenge.
  const addBegun = await post(env, '/api/passkeys/add/begin', {}, token);
  r = await signIn(env, keeper, { begun: { body: { challenge: addBegun.body.challenge, rp_id: 'eustress.dev' } } });
  assert.equal(r.body.code, 'challenge_invalid');
  // Nor is an identity-file sign-in challenge.
  const idBegun = await post(env, '/api/auth/challenge', { public_key: keys[OWNER].hex });
  r = await signIn(env, keeper, { begun: { body: { challenge: b64url(Buffer.from(idBegun.body.challenge, 'hex')), rp_id: 'eustress.dev' } } });
  assert.equal(r.body.code, 'challenge_invalid');
  assert.deepEqual((await objectState(env)).passkeys.map((p) => p.id), [keeper.id], 'only the one passkey was ever added');
});

// ── The identity key's signature ─────────────────────────────────────────────

test('adding needs the identity key\'s signature over this add: a wrong one, another account\'s, or a retired key is refused', async () => {
  const { env, keys, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const cases = [
    ['garbage', (req) => { req.signature = 'zz'; }],
    ['another key', (req, { message }) => { req.signature = keyPair().sign(message); }],
    ['another account\'s key', (req, { message }) => { req.signature = keys[OTHER].sign(message); }],
    ['another origin', (req, { options }) => { req.signature = k0.sign(passkeyAddMessage({ origin: 'https://evil.example', accountId: OWNER, credentialId: auth.id, challenge: hexOf(options.challenge) })); }],
    ['another credential', (req, { options }) => { req.signature = k0.sign(passkeyAddMessage({ origin: ORIGIN, accountId: OWNER, credentialId: b64url(crypto.randomBytes(32)), challenge: hexOf(options.challenge) })); }],
    ['another account id', (req, { options }) => { req.signature = k0.sign(passkeyAddMessage({ origin: ORIGIN, accountId: OTHER, credentialId: auth.id, challenge: hexOf(options.challenge) })); }],
  ];
  for (const [label, mutate] of cases) {
    const r = await addPasskey(env, token, k0, auth, { mutate });
    assert.equal(r.status, 401, `${label}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'signature_invalid', label);
  }

  // After a rotation the old key no longer adds a passkey.
  const k1 = keyPair();
  const rb = await post(env, '/api/identity/rotate/begin', {}, token);
  const message = rotationMessage({ origin: ORIGIN, accountId: OWNER, oldKey: k0.hex, newKey: k1.hex, challenge: rb.body.challenge });
  const rotated = await post(env, '/api/identity/rotate', { new_public_key: k1.hex, challenge: rb.body.challenge, old_signature: k0.sign(message), new_signature: k1.sign(message) }, token);
  assert.equal(rotated.status, 200, JSON.stringify(rotated.body));
  const fresh = rotated.body.token;
  const retired = await addPasskey(env, fresh, k0, auth);
  assert.equal(retired.status, 401);
  assert.equal(retired.body.code, 'signature_invalid');
  assert.equal((await addPasskey(env, fresh, k1, auth)).status, 200, 'the current key does');
});

// ── Already registered, and the limit ────────────────────────────────────────

test('a credential already registered, to this account or another, is not added again', async () => {
  const { env, keys, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const again = await addPasskey(env, token, k0, auth);
  assert.equal(again.status, 409);
  assert.equal(again.body.code, 'passkey_registered');
  const elsewhere = await addPasskey(env, mintToken(OTHER), keys[OTHER], auth, { account: OTHER });
  assert.equal(elsewhere.status, 409);
  assert.equal(elsewhere.body.code, 'passkey_registered');
  assert.equal(await ownerOf(env, auth.id), OWNER);
  // Another account cannot release the owner's claim, and the owner's sign-in still works.
  assert.deepEqual(await passkeyIndexCall(env, auth.id, 'release', { account_id: OTHER }), { ok: true, released: false });
  assert.equal(await ownerOf(env, auth.id), OWNER);
  assert.equal((await signIn(env, auth, { bend: { userHandle: b64url(Buffer.from(OWNER)) } })).status, 200);
});

test(`an account holds at most ${MAX_PASSKEYS} passkeys, even when two adds race`, async () => {
  const { env, k0, token } = await environment();
  for (let i = 0; i < MAX_PASSKEYS - 1; i++) assert.equal((await addPasskey(env, token, k0, new SoftAuthenticator(-8))).status, 200, `passkey ${i + 1}`);
  // Two challenges taken while there is room for one.
  const a = await post(env, '/api/passkeys/add/begin', {}, token);
  const b = await post(env, '/api/passkeys/add/begin', {}, token);
  assert.equal(a.status, 200);
  assert.equal(b.status, 200);
  assert.equal((await addPasskey(env, token, k0, new SoftAuthenticator(-8), { begun: a })).status, 200);
  const over = await addPasskey(env, token, k0, new SoftAuthenticator(-8), { begun: b });
  assert.equal(over.status, 409);
  assert.equal(over.body.code, 'too_many_passkeys');
  const full = await post(env, '/api/passkeys/add/begin', {}, token);
  assert.equal(full.status, 409);
  assert.equal(full.body.code, 'too_many_passkeys');
  assert.equal((await objectState(env)).passkeys.length, MAX_PASSKEYS);
});

// ── A passkey session cannot change keys ─────────────────────────────────────

test('a passkey session is refused by every key-change route, and accepted everywhere else', async () => {
  const { env, k0, token } = await environment();
  const { auth, token: pk } = await passkeySession(env, token, k0);
  const refused = [
    ['/api/identity/rotate/begin', {}],
    ['/api/identity/rotate', { new_public_key: keyPair().hex, challenge: 'a'.repeat(64), old_signature: 'b'.repeat(128), new_signature: 'c'.repeat(128) }],
    ['/api/passkeys/add/begin', {}],
    ['/api/passkeys/add', { challenge: b64url(crypto.randomBytes(32)), credential: {}, signature: 'x' }],
    ['/api/passkeys/remove/begin', { id: auth.id }],
    ['/api/passkeys/remove', { id: auth.id, challenge: b64url(crypto.randomBytes(32)), signature: 'x' }],
  ];
  for (const [route, body] of refused) {
    const r = await post(env, route, body, pk);
    assert.equal(r.status, 403, `${route}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'identity_file_required', route);
    assert.equal(r.body.error, 'Sign in with your identity file to change your keys.');
  }
  // The passkey is still there, and the key unchanged.
  assert.equal((await objectState(env)).passkeys.length, 1);
  assert.equal((await objectState(env)).public_key, k0.hex);

  // Everything else accepts the session.
  assert.equal((await send(env, 'GET', '/api/auth/me', { token: pk })).status, 200);
  const list = await send(env, 'GET', '/api/passkeys', { token: pk });
  assert.equal(list.status, 200);
  assert.equal(list.body.passkeys.length, 1);
  const fresh = await post(env, '/api/account/email/cancel', {}, pk);
  assert.notEqual(fresh.status, 401, JSON.stringify(fresh.body));
  assert.notEqual(fresh.body?.code, 'identity_file_required');
  const prompt = await post(env, '/api/identity/rotate/prompt-seen', {}, pk);
  assert.equal(prompt.status, 200, 'marking the prompt seen changes no key');
});

// McKale, 2026-10-08: the account's email address and the credentials that act as
// the account are guarded like keys.
const ADDRESS_ROUTES = ['/api/account/email/start', '/api/account/email/change/begin', '/api/account/email/change', '/api/account/email/apply'];
const MINTING_ROUTES = [['/api/keys', { name: 'ci', mode: 'test' }], ['/api/commerce/webhook_endpoints', { url: 'https://hooks.example.com/eustress', enabled_events: ['*'] }]];

test('a passkey session cannot change the email address or its verification; an identity-file session can', async () => {
  const { env, sent, k0, token } = await environment();
  const { token: pk } = await passkeySession(env, token, k0);
  const mailBefore = sent.length;
  for (const route of ADDRESS_ROUTES) {
    const r = await post(env, route, { new_address: 'thief@example.com', challenge: 'a'.repeat(64), signature: 'b'.repeat(128) }, pk);
    assert.equal(r.status, 403, `${route}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'identity_file_required', route);
    assert.equal(r.body.error, 'Sign in with your identity file to change your keys.');
  }
  assert.equal(sent.length, mailBefore, 'no verification or change mail went out');
  assert.equal((await accountCall(env, OWNER, 'email_status')).email.pending, null, 'no change is pending');

  // Reading the address, and cancelling a pending change, stay open to it.
  const read = await send(env, 'GET', '/api/account/email', { token: pk });
  assert.equal(read.status, 200, JSON.stringify(read.body));
  const cancel = await post(env, '/api/account/email/cancel', {}, pk);
  assert.notEqual(cancel.body?.code, 'identity_file_required');

  // The identity-file session reaches every one of them.
  const start = await post(env, '/api/account/email/start', {}, token);
  assert.equal(start.status, 200, JSON.stringify(start.body));
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  const message = ['eustress-email-change-v1', ORIGIN, OWNER, k0.hex, 'new@example.com', begun.body.challenge].join('\n');
  const changed = await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'new@example.com', signature: k0.sign(message) }, token);
  assert.equal(changed.status, 200, JSON.stringify(changed.body));
  assert.ok(changed.body.pending, 'the change is pending');
  // Applying before the wait is over is refused by the rule of the wait, not by the session.
  const applied = await post(env, '/api/account/email/apply', {}, token);
  assert.equal(applied.status, 409, JSON.stringify(applied.body));
  assert.equal(applied.body.code, 'not_yet');
});

test('a passkey session cannot mint an API key or a webhook endpoint; it can list and revoke them, and an identity-file session can mint', async () => {
  const { env, k0, token } = await environment();
  const { token: pk } = await passkeySession(env, token, k0);
  for (const [route, body] of MINTING_ROUTES) {
    const r = await post(env, route, body, pk);
    assert.equal(r.status, 403, `${route}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'identity_file_required', route);
  }
  // Minted by the identity-file session.
  const key = await post(env, '/api/keys', MINTING_ROUTES[0][1], token);
  assert.equal(key.status, 201, JSON.stringify(key.body));
  const hook = await post(env, '/api/commerce/webhook_endpoints', MINTING_ROUTES[1][1], token);
  assert.equal(hook.status, 201, JSON.stringify(hook.body));

  // Listed and revoked by the passkey session.
  const keys = await send(env, 'GET', '/api/keys', { token: pk });
  assert.equal(keys.status, 200);
  assert.deepEqual(keys.body.keys.map((k) => k.id), [key.body.id]);
  const hooks = await send(env, 'GET', '/api/commerce/webhook_endpoints', { token: pk });
  assert.equal(hooks.status, 200, JSON.stringify(hooks.body));
  assert.equal((await send(env, 'DELETE', `/api/keys/${key.body.id}`, { token: pk })).status, 200);
  assert.equal((await send(env, 'DELETE', `/api/commerce/webhook_endpoints/${hook.body.id}`, { token: pk })).status, 200);
  assert.deepEqual((await send(env, 'GET', '/api/keys', { token: pk })).body.keys, []);
  // An API key the identity-file session minted still makes webhook endpoints, as before.
  const apiKey = (await post(env, '/api/keys', { mode: 'test' }, token)).body.key;
  const byKey = await send(env, 'POST', '/api/commerce/webhook_endpoints', { token: apiKey, body: MINTING_ROUTES[1][1] });
  assert.equal(byKey.status, 201, JSON.stringify(byKey.body));
});

test('a passkey session cannot set where payouts go; an identity-file session goes on to the payout checks', async () => {
  // A Stripe key the test never lets reach Stripe: both sessions stop before any call.
  const { env, k0, token } = await environment({ STRIPE_SECRET_KEY: 'sk_test_not_real' });
  const { token: pk } = await passkeySession(env, token, k0);
  const refused = await post(env, '/api/stripe/connect/onboard', {}, pk);
  assert.equal(refused.status, 403, JSON.stringify(refused.body));
  assert.equal(refused.body.code, 'identity_file_required');
  assert.equal(refused.body.error, 'Sign in with your identity file to change your keys.');
  assert.equal(JSON.parse(await env.USERS.get(`user:${OWNER}`)).stripe_connect_id, undefined, 'no payout account was made');
  // The identity-file session passes this guard and meets the next one (no verified
  // identity in this test, so the money gate refuses it in its own words).
  const allowed = await post(env, '/api/stripe/connect/onboard', {}, token);
  assert.equal(allowed.status, 403, JSON.stringify(allowed.body));
  assert.equal(allowed.body.code, 'kyc_required', 'the next gate answered, not this one');
  // Reading the payout status stays open to a passkey session.
  const status = await send(env, 'GET', '/api/stripe/connect/status', { token: pk });
  assert.notEqual(status.body?.code, 'identity_file_required', JSON.stringify(status.body));
});

test('an administrator signed in with a passkey cannot replace another account\'s key', async () => {
  const { env, keys } = await environment();
  const adminToken = mintToken(ADMIN);
  const { token: pk } = await passkeySession(env, adminToken, keys[ADMIN], { account: ADMIN });
  assert.equal(claims(pk).sub, ADMIN);
  for (const route of ['/api/admin/accounts/replace-key/begin', '/api/admin/accounts/replace-key']) {
    const r = await post(env, route, { account_id: OWNER, new_public_key: keyPair().hex, case_ref: 'SUP-1', reason: 'checked on a call' }, pk);
    assert.equal(r.status, 403, `${route}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'identity_file_required', route);
  }
  const ok = await post(env, '/api/admin/accounts/replace-key/begin', { account_id: OWNER, new_public_key: keyPair().hex, case_ref: 'SUP-1' }, adminToken);
  assert.equal(ok.status, 200, 'the same administrator with an identity-file session may begin');
});

// ── Sign-in refusals ─────────────────────────────────────────────────────────

test('a user handle that names another account is refused', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  for (const handle of [b64url(Buffer.from(OTHER)), b64url(Buffer.from(OWNER + ' ')), b64url(Buffer.from('x'))]) {
    const r = await signIn(env, auth, { bend: { userHandle: handle } });
    assert.equal(r.status, 401, JSON.stringify(r.body));
    assert.equal(r.body.code, 'passkey_not_accepted');
  }
  const none = await signIn(env, auth, { bend: { userHandle: '' } });
  assert.equal(none.status, 400);
});

test('a removed passkey no longer signs in, even where the passkey index still names the account', async () => {
  const { env, sent, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const keep = new SoftAuthenticator(-8);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  assert.equal((await addPasskey(env, token, k0, keep)).status, 200);
  const begun = await post(env, '/api/passkeys/remove/begin', { id: auth.id }, token);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  assert.equal(begun.body.api_origin, ORIGIN);
  // A wrong signature removes nothing and spends the challenge.
  const wrong = await post(env, '/api/passkeys/remove', { id: auth.id, challenge: begun.body.challenge, signature: keyPair().sign('x') }, token);
  assert.equal(wrong.body.code, 'signature_invalid');
  const begun2 = await post(env, '/api/passkeys/remove/begin', { id: auth.id }, token);
  const message = passkeyRemoveMessage({ origin: ORIGIN, accountId: OWNER, credentialId: auth.id, challenge: hexOf(begun2.body.challenge) });
  const removed = await post(env, '/api/passkeys/remove', { id: auth.id, challenge: begun2.body.challenge, signature: k0.sign(message) }, token);
  assert.equal(removed.status, 200, JSON.stringify(removed.body));
  assert.equal(removed.body.removed, auth.id);
  assert.match(bodyOf(sent.at(-1)), /The passkey named "Laptop" was removed/);
  assert.equal(await ownerOf(env, auth.id), null, 'the index entry is gone');

  let r = await signIn(env, auth);
  assert.equal(r.status, 401);
  assert.equal(r.body.code, 'passkey_not_accepted');
  // An index that still names the account (a release not yet landed): the object, which no longer holds it, decides.
  assert.equal((await passkeyIndexCall(env, auth.id, 'claim', { account_id: OWNER })).ok, true);
  r = await signIn(env, auth);
  assert.equal(r.status, 401);
  assert.equal(r.body.code, 'passkey_not_accepted');
  assert.equal((await signIn(env, keep)).status, 200, 'the other passkey still signs in');
  const unknown = await post(env, '/api/passkeys/remove/begin', { id: auth.id }, token);
  assert.equal(unknown.status, 404);
  assert.equal(unknown.body.code, 'passkey_not_found');
});

test('a sign count that goes backwards is refused as a copied passkey, and recorded', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  assert.equal((await signIn(env, auth, { bend: { signCount: 5 } })).status, 200);
  for (const count of [5, 4, 0]) {
    const r = await signIn(env, auth, { bend: { signCount: count } });
    assert.equal(r.status, 403, `count ${count}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'passkey_cloned');
  }
  const audit = await accountCall(env, OWNER, 'audit', { limit: 50 });
  assert.equal(audit.records.filter((x) => x.type === 'passkey_cloned').length, 3);
  assert.ok(await verifyAuditChain(audit.records));
  assert.equal((await signIn(env, auth, { bend: { signCount: 6 } })).status, 200, 'a count that moved forward is accepted');
});

test('a signature by another key, or over other bytes, does not sign in', async () => {
  const { env, k0, token } = await environment();
  for (const [, alg] of ALGS) {
    const auth = new SoftAuthenticator(alg);
    assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
    const impostor = new SoftAuthenticator(alg);
    const forged = await signIn(env, auth, { bend: { signature: impostor.key.sign(Buffer.from('anything')) } });
    assert.equal(forged.status, 401, JSON.stringify(forged.body));
    assert.equal(forged.body.code, 'passkey_not_accepted');
    // A signature over the right bytes, then the authenticator data changed after signing.
    const begun = await post(env, '/api/passkeys/login/begin', {});
    const good = auth.assert(begun.body);
    const tampered = Buffer.from(good.response.authenticator_data, 'base64url');
    tampered[36] ^= 0x40; // the sign count
    good.response.authenticator_data = b64url(tampered);
    const r = await post(env, '/api/passkeys/login', { challenge: begun.body.challenge, credential: good });
    assert.equal(r.status, 401);
    assert.equal(r.body.code, 'passkey_not_accepted');
  }
});

test('a banned account does not sign in with a passkey', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const user = JSON.parse(await env.USERS.get(`user:${OWNER}`));
  await env.USERS.put(`user:${OWNER}`, JSON.stringify({ ...user, banned: true, ban_reason: 'spam' }));
  const r = await signIn(env, auth);
  assert.equal(r.status, 403);
  assert.equal(r.body.error, 'Account suspended');
  assert.equal(r.body.token, undefined);
});

test('an object that cannot be reached means no passkey sign-in', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  env.ACCOUNT_AUTH = { idFromName: (n) => n, get() { throw new Error('the object is down'); } };
  const r = await signIn(env, auth);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'account_unavailable');
});

// ── Malformed data ───────────────────────────────────────────────────────────

test('malformed CBOR is refused: truncated, indefinite length, oversized, tagged, float, duplicate keys', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  const good = Buffer.from(auth.register({ challenge: 'x', rp: { id: 'eustress.dev' }, user: { id: 'x' } }).response.attestation_object, 'base64url');
  const indefinite = Buffer.concat([Buffer.from([0xbf]), good.subarray(1), Buffer.from([0xff])]); // map of indefinite length
  const cases = [
    ['truncated', good.subarray(0, good.length - 10), 400, 'cbor_invalid'],
    ['indefinite length', indefinite, 400, 'cbor_invalid'],
    ['a byte string longer than the data', Buffer.concat([Buffer.from([0xa1, 0x63]), Buffer.from('fmt'), Buffer.from([0x5a, 0x7f, 0xff, 0xff, 0xff])]), 400, 'cbor_invalid'],
    ['trailing bytes', Buffer.concat([good, Buffer.from([0x00])]), 400, 'cbor_invalid'],
    ['oversized', Buffer.alloc(40000, 0x41), 400, 'bad_request'],
  ];
  for (const [label, bytes, status, code] of cases) {
    const r = await addPasskey(env, token, k0, auth, { bend: { attestationObject: new Uint8Array(bytes) } });
    assert.equal(r.status, status, `${label}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, code, label);
  }
  // The credential's COSE key, malformed inside the authenticator data.
  const badCose = await addPasskey(env, token, k0, auth, { bend: { cose: new Uint8Array([0xa5, 0x01, 0x02]) } });
  assert.equal(badCose.body.code, 'authenticator_data_invalid');
  // Extensions with a float in them.
  const floatExt = await addPasskey(env, token, k0, auth, { bend: { extensions: new Uint8Array([0xa1, 0x61, 0x78, 0xf9, 0x3c, 0x00]) } });
  assert.equal(floatExt.body.code, 'authenticator_data_invalid');
  // A boolean extension output (hmac-secret) is fine.
  assert.equal((await addPasskey(env, token, k0, auth, { bend: { extensions: new Map([['hmac-secret', true], ['credProtect', 2]]) } })).status, 200);
});

test('the CBOR decoder takes only what WebAuthn needs', () => {
  const ok = [
    [[0x00], 0], [[0x17], 23], [[0x18, 0x18], 24], [[0x19, 0x01, 0x00], 256], [[0x20], -1], [[0x38, 0x18], -25],
    [[0x39, 0x01, 0x00], -257], [[0xf4], false], [[0xf5], true], [[0xf6], null], [[0x60], ''], [[0x80], []],
  ];
  for (const [bytes, value] of ok) assert.deepEqual(decodeCbor(new Uint8Array(bytes)), value, JSON.stringify(bytes));
  assert.deepEqual(decodeCbor(new Uint8Array([0x42, 1, 2])), new Uint8Array([1, 2]));
  assert.deepEqual(decodeCbor(new Uint8Array([0xa2, 0x01, 0x02, 0x61, 0x61, 0xf5])), new Map([[1, 2], ['a', true]]));
  const refused = {
    'empty': [],
    'tag': [0xc0, 0x00],
    'half float': [0xf9, 0x3c, 0x00],
    'double': [0xfb, 0, 0, 0, 0, 0, 0, 0, 0],
    'undefined': [0xf7],
    'simple value': [0xf8, 0x20],
    'indefinite bytes': [0x5f, 0x41, 0x00, 0xff],
    'indefinite array': [0x9f, 0x00, 0xff],
    'indefinite map': [0xbf, 0x01, 0x02, 0xff],
    'break': [0xff],
    'reserved': [0x1c],
    'truncated integer': [0x19, 0x01],
    'truncated bytes': [0x43, 0x01, 0x02],
    'array longer than the data': [0x9a, 0x00, 0x01, 0x00, 0x00],
    'map longer than the data': [0xb9, 0x10, 0x00, 0x01],
    'duplicate key': [0xa2, 0x01, 0x02, 0x01, 0x03],
    'bytes as a key': [0xa1, 0x41, 0x00, 0x01],
    'integer beyond 2^53': [0x1b, 0x00, 0x20, 0, 0, 0, 0, 0, 0],
    'text that is not UTF-8': [0x62, 0xc3, 0x28],
    'trailing': [0x00, 0x00],
    'nested too deeply': [...new Array(12).fill(0x81), 0x00],
  };
  for (const [label, bytes] of Object.entries(refused)) {
    assert.throws(() => decodeCbor(new Uint8Array(bytes)), (e) => e instanceof WebauthnError && e.code === 'cbor_invalid', label);
  }
  // One item at an offset (how the authenticator data's COSE key is read): a length
  // past the end is refused there, not left for a later check to notice.
  for (const bytes of [[0x43, 0x01, 0x02], [0x63, 0x61, 0x62], [0x82, 0x01]]) {
    assert.throws(() => decodeCborItem(new Uint8Array(bytes), 0), (e) => e instanceof WebauthnError && e.code === 'cbor_invalid', JSON.stringify(bytes));
  }
  assert.deepEqual(decodeCborItem(new Uint8Array([0xff, 0x41, 0x07, 0xff]), 1), { value: new Uint8Array([7]), end: 3 });
});

test('ES256 signatures are strict DER, and come out as raw r || s', () => {
  const r = Buffer.alloc(32, 0x11);
  const s = Buffer.alloc(32, 0x22);
  const der = (rb, sb) => Buffer.concat([Buffer.from([0x30, rb.length + sb.length + 4, 0x02, rb.length]), rb, Buffer.from([0x02, sb.length]), sb]);
  assert.deepEqual(Buffer.from(derToRawEcdsa(new Uint8Array(der(r, s)))), Buffer.concat([r, s]));
  // A high bit needs one leading zero; a short integer is padded.
  const high = Buffer.alloc(32, 0x99);
  assert.deepEqual(Buffer.from(derToRawEcdsa(new Uint8Array(der(Buffer.concat([Buffer.from([0]), high]), Buffer.from([0x05]))))), Buffer.concat([high, Buffer.alloc(31), Buffer.from([5])]));
  const bad = {
    'negative': der(high, s),
    'not minimal': der(Buffer.concat([Buffer.from([0]), r]), s),
    'too long': der(Buffer.concat([Buffer.from([1]), r]), s),
    'trailing': Buffer.concat([der(r, s), Buffer.from([0])]),
    'wrong sequence length': Buffer.from([0x30, 0x10, ...der(r, s).subarray(2)]),
    'not a sequence': Buffer.from([0x31, ...der(r, s).subarray(1)]),
  };
  for (const [label, bytes] of Object.entries(bad)) assert.throws(() => derToRawEcdsa(new Uint8Array(bytes)), WebauthnError, label);
});

test('base64url is canonical and bounded', () => {
  assert.deepEqual(fromBase64url('AQID'), new Uint8Array([1, 2, 3]));
  assert.equal(fromBase64url('AQI='), null, 'padding');
  assert.equal(fromBase64url('AQJ'), null, 'stray bits: one value has one spelling');
  assert.equal(fromBase64url('A+/D'), null, 'base64, not base64url');
  assert.equal(fromBase64url('AQID', 2), null, 'over the bound');
  assert.equal(fromBase64url(12), null);
});

// ── Replacing the identity file, and signing out ─────────────────────────────

test('replacing the identity file lists the passkeys and removes the ones the owner ticked, in the same step', async () => {
  const { env, sent, k0, token } = await environment();
  const drop = new SoftAuthenticator(-7);
  const keep = new SoftAuthenticator(-8);
  assert.equal((await addPasskey(env, token, k0, drop, { name: 'Old phone' })).status, 200);
  assert.equal((await addPasskey(env, token, k0, keep, { name: 'Laptop' })).status, 200);
  const k1 = keyPair();
  const rb = await post(env, '/api/identity/rotate/begin', {}, token);
  assert.equal(rb.status, 200);
  assert.deepEqual(rb.body.credentials.passkeys.map((p) => [p.id, p.name]), [[drop.id, 'Old phone'], [keep.id, 'Laptop']]);
  const message = rotationMessage({ origin: ORIGIN, accountId: OWNER, oldKey: k0.hex, newKey: k1.hex, challenge: rb.body.challenge });
  const notMine = b64url(crypto.randomBytes(32));
  const rotated = await post(env, '/api/identity/rotate', {
    new_public_key: k1.hex, challenge: rb.body.challenge, old_signature: k0.sign(message), new_signature: k1.sign(message),
    revoke: { passkeys: [drop.id, notMine] },
  }, token);
  assert.equal(rotated.status, 200, JSON.stringify(rotated.body));
  assert.equal(rotated.body.revoked.passkeys, 1);
  assert.deepEqual(rotated.body.revoked.failed, [notMine], 'an id that is not the account\'s is reported');
  assert.equal(claims(rotated.body.token).amr, 'key');
  const held = await objectState(env);
  assert.deepEqual(held.passkeys.map((p) => p.id), [keep.id]);
  assert.equal(held.public_key, k1.hex);
  assert.equal(await ownerOf(env, drop.id), null);
  assert.equal((await signIn(env, drop)).body.code, 'passkey_not_accepted');
  assert.equal((await signIn(env, keep)).status, 200, 'the one left signs in, with a session after the rotation');
  const audit = await accountCall(env, OWNER, 'audit', { limit: 50 });
  assert.equal(audit.records.find((x) => x.type === 'key_rotated').data.passkeys_removed, 1);
  assert.match(bodyOf(sent.at(-1)), /1 passkey\(s\)/, 'the notice says what was revoked');

  // A revoke list with something that is not a credential id refuses the whole request.
  const rb2 = await post(env, '/api/identity/rotate/begin', {}, rotated.body.token);
  const bad = await post(env, '/api/identity/rotate', { new_public_key: keyPair().hex, challenge: rb2.body.challenge, old_signature: 'a', new_signature: 'b', revoke: { passkeys: ['not an id!'] } }, rotated.body.token);
  assert.equal(bad.status, 400);
  assert.equal(bad.body.code, 'bad_request');
});

test('signing out everywhere ends a passkey session', async () => {
  const { env, k0, token } = await environment();
  const { auth, token: pk } = await passkeySession(env, token, k0);
  assert.equal((await send(env, 'GET', '/api/auth/me', { token: pk })).status, 200);
  assert.equal((await post(env, '/api/auth/logout', {}, token)).status, 200, 'signed out from the identity-file session');
  assert.equal((await send(env, 'GET', '/api/auth/me', { token: pk })).status, 401, 'the passkey session ended too');
  assert.equal((await send(env, 'GET', '/api/passkeys', { token: pk })).status, 401);
  const again = await signIn(env, auth);
  assert.equal(again.status, 200, 'and the passkey signs in again after');
  assert.equal((await send(env, 'GET', '/api/auth/me', { token: again.body.token })).status, 200);
});

test('an administrator\'s replacement removes every passkey, and putting the earlier key back removes those added since', async () => {
  const { env, k0, token } = await environment();
  const before = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, before)).status, 200);
  const newKey = keyPair();
  const replaced = await accountCall(env, OWNER, 'admin_replace_key', {
    admin_id: ADMIN, case_ref: 'SUP-77', reason: 'identity checked on a video call', new_public_key: newKey.hex,
    new_key_signature: newKey.sign(adminReplaceKeyMessage({ origin: ORIGIN, username: 'owner', caseRef: 'SUP-77', newKey: newKey.hex })),
    origin: ORIGIN, username: 'owner', holds_value: false,
  });
  assert.equal(replaced.ok, true, JSON.stringify(replaced));
  assert.deepEqual(replaced.passkeys_removed, [before.id]);
  assert.deepEqual((await objectState(env)).passkeys, []);
  assert.equal(await ownerOf(env, before.id), null);

  // Whoever holds the new key signs in (a session at the new floor) and adds a
  // passkey; the owner then puts the earlier key back.
  const after = new SoftAuthenticator(-7);
  const sessionAfter = tokenAt(OWNER, (await objectState(env)).floor);
  assert.equal((await addPasskey(env, sessionAfter, newKey, after)).status, 200);
  const token2 = replaced.undo_link.split('token=')[1];
  const restored = await post(env, '/api/account/key-restore', { token: token2 });
  assert.equal(restored.status, 200, JSON.stringify(restored.body));
  assert.deepEqual((await objectState(env)).passkeys, [], 'the passkey added with the disputed key is gone');
  assert.equal(await ownerOf(env, after.id), null);
  assert.equal((await signIn(env, after)).body.code, 'passkey_not_accepted');
});

// ── Adversarial review, 2026-10-08 ───────────────────────────────────────────

test('another account cannot take over a credential id, even reading a stale KV, nor release the owner\'s claim', async () => {
  const { env, keys, k0, token } = await environment();
  const victim = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, victim)).status, 200);
  const attacker = new SoftAuthenticator(-7);
  attacker.credentialId = victim.credentialId;
  attacker.id = victim.id;
  // A KV read that says nothing is there (a stale edge cache) changes nothing: KV is not asked.
  const realGet = env.USERS.get.bind(env.USERS);
  env.USERS.get = async (k) => (k.startsWith('passkey:') ? null : realGet(k));
  const taken = await addPasskey(env, mintToken(OTHER), keys[OTHER], attacker, { account: OTHER });
  env.USERS.get = realGet;
  assert.equal(taken.status, 409, JSON.stringify(taken.body));
  assert.equal(taken.body.code, 'passkey_registered');
  assert.equal(await ownerOf(env, victim.id), OWNER);
  assert.deepEqual((await objectState(env, OTHER)).passkeys || [], []);
  // The attacker has nothing to remove, and a release in its name frees nothing.
  const rb = await post(env, '/api/passkeys/remove/begin', { id: victim.id }, mintToken(OTHER));
  assert.equal(rb.status, 404);
  assert.deepEqual(await passkeyIndexCall(env, victim.id, 'release', { account_id: OTHER }), { ok: true, released: false });
  assert.equal((await signIn(env, victim, { bend: { userHandle: b64url(Buffer.from(OWNER)) } })).status, 200, 'the owner still signs in');
});

test('unsigned attempts do not flush the replay record, and a full record answers 429 without dropping anything', async () => {
  const { env, k0, token } = await environment();
  const victim = new SoftAuthenticator(-7, { countsUp: false });
  assert.equal((await addPasskey(env, token, k0, victim)).status, 200);
  const begun = await post(env, '/api/passkeys/login/begin', {});
  const filed = [...env.CHALLENGES.store.entries()].find(([k]) => k.startsWith('pkchal:'));
  const captured = { challenge: begun.body.challenge, credential: victim.assert(begun.body) };
  assert.equal((await post(env, '/api/passkeys/login', captured)).status, 200);
  // Someone who knows the credential id and the account, but holds no key.
  const forger = new SoftAuthenticator(-7, { countsUp: false });
  forger.id = victim.id;
  forger.credentialId = victim.credentialId;
  forger.userHandle = victim.userHandle;
  for (let i = 0; i < PASSKEY_LOGIN_SEEN_MAX; i++) assert.equal((await signIn(env, forger)).status, 401);
  assert.equal(Object.keys((await objectState(env)).passkey_logins).length, 1, 'only the sign-in that verified is remembered');
  env.CHALLENGES.store.set(filed[0], filed[1]); // a stale KV copy of the spent challenge
  const replay = await post(env, '/api/passkeys/login', captured);
  assert.equal(replay.status, 400, JSON.stringify(replay.body));
  assert.equal(replay.body.code, 'challenge_invalid');

  // A full record: the next sign-in waits; nothing unexpired is dropped.
  const object = env.ACCOUNT_AUTH.instances.get(OWNER);
  const held = await object.storage.get('state');
  const exp = Date.now() + 5 * 60 * 1000;
  held.passkey_logins = Object.fromEntries([...Array(PASSKEY_LOGIN_SEEN_MAX)].map((_, i) => [crypto.createHash('sha256').update(String(i)).digest('hex'), exp]));
  await object.storage.put('state', held);
  const full = await signIn(env, victim);
  assert.equal(full.status, 429, JSON.stringify(full.body));
  assert.equal(full.body.code, 'rate_limited');
  assert.equal(Object.keys((await objectState(env)).passkey_logins).length, PASSKEY_LOGIN_SEEN_MAX, 'nothing was dropped');
});

test('sign-in says the same thing for an unknown passkey, one naming another account, a bad signature and a deleted account', async () => {
  const { env, k0, token } = await environment();
  const victim = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, victim)).status, 200);
  const guess = new SoftAuthenticator(-7);
  guess.userHandle = b64url(Buffer.from(OTHER));
  const unknown = await signIn(env, guess);
  guess.id = victim.id;
  guess.credentialId = victim.credentialId;
  const wrongAccount = await signIn(env, guess);
  guess.userHandle = b64url(Buffer.from(OWNER));
  const badSignature = await signIn(env, guess);
  await env.USERS.delete(`user:${OWNER}`);
  const deleted = await signIn(env, victim);
  const answers = [unknown, wrongAccount, badSignature, deleted].map((r) => JSON.stringify([r.status, r.body]));
  assert.equal(new Set(answers).size, 1, answers.join('\n'));
  assert.equal(unknown.status, 401);
  assert.equal(unknown.body.code, 'passkey_not_accepted');
});

test('deleting an account removes its passkeys and their claims, and its passkey signs nobody in', async () => {
  const { env, keys, k0, token } = await environment();
  const auth = new SoftAuthenticator(-8);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const r = await post(env, '/api/admin/delete-user', { username: 'owner' }, mintToken(ADMIN));
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.deepEqual((await objectState(env)).passkeys, []);
  assert.equal(await ownerOf(env, auth.id), null, 'the claim was released');
  const signed = await signIn(env, auth);
  assert.equal(signed.status, 401, JSON.stringify(signed.body));
  assert.equal(signed.body.code, 'passkey_not_accepted');
  const audit = await accountCall(env, OWNER, 'audit', { limit: 50 });
  assert.ok(audit.records.some((x) => x.type === 'passkeys_cleared'), 'the clearing is in the audit chain');
  // The freed id can be registered by another account.
  const reuse = await addPasskey(env, mintToken(OTHER), keys[OTHER], auth, { account: OTHER });
  assert.equal(reuse.status, 200, JSON.stringify(reuse.body));
  assert.equal(await ownerOf(env, auth.id), OTHER);
});

/// Delete OWNER through the admin route while the account object's passkeys_clear
/// fails, so the passkey's claim is left behind; then, unless `fresh`, age the claim
/// past the grace window the index waits before judging it.
async function orphanedClaim({ fresh = false } = {}) {
  const { env, keys, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const realGet = env.ACCOUNT_AUTH.get.bind(env.ACCOUNT_AUTH);
  env.ACCOUNT_AUTH.get = (id) => {
    const stub = realGet(id);
    return { fetch: (url, init) => (String(url).endsWith('/passkeys_clear') ? Promise.reject(new Error('the object is down')) : stub.fetch(url, init)) };
  };
  const deleted = await post(env, '/api/admin/delete-user', { username: 'owner' }, mintToken(ADMIN));
  env.ACCOUNT_AUTH.get = realGet;
  assert.equal(deleted.status, 200, JSON.stringify(deleted.body));
  assert.equal((await objectState(env)).passkeys.length, 1, 'clearing failed: the object still holds the passkey');
  const index = env.PASSKEY_INDEX.instances.get(await passkeyIndexName(auth.id));
  const claim = await index.storage.get('owner');
  assert.equal(claim.account_id, OWNER, 'and its claim is left behind');
  if (!fresh) await index.storage.put('owner', { ...claim, at: new Date(Date.now() - ORPHAN_GRACE_MS - 60000).toISOString() });
  return { env, keys, auth, index };
}

test('a sign-in that finds a deleted account\'s claim releases it, and another account can then register the id', async () => {
  const { env, keys, auth, index } = await orphanedClaim();
  const signed = await signIn(env, auth);
  assert.equal(signed.status, 401, JSON.stringify(signed.body));
  assert.equal(signed.body.code, 'passkey_not_accepted', 'still the one answer');
  assert.equal(await index.storage.get('owner'), undefined, 'the lookup released the orphaned claim');
  const reuse = await addPasskey(env, mintToken(OTHER), keys[OTHER], auth, { account: OTHER });
  assert.equal(reuse.status, 200, JSON.stringify(reuse.body));
  assert.equal(await ownerOf(env, auth.id), OTHER);
});

test('an add that finds a deleted account\'s claim releases it and takes the id', async () => {
  const { env, keys, auth } = await orphanedClaim();
  const reuse = await addPasskey(env, mintToken(OTHER), keys[OTHER], auth, { account: OTHER });
  assert.equal(reuse.status, 200, JSON.stringify(reuse.body));
  assert.equal(await ownerOf(env, auth.id), OTHER);
});

test('a claim younger than the grace window is not judged, even when its account cannot be read yet', async () => {
  const { env, keys, auth } = await orphanedClaim({ fresh: true });
  const refused = await addPasskey(env, mintToken(OTHER), keys[OTHER], auth, { account: OTHER });
  assert.equal(refused.status, 409, JSON.stringify(refused.body));
  assert.equal(refused.body.code, 'passkey_registered');
  assert.equal(await ownerOf(env, auth.id), OWNER);
  assert.equal((await signIn(env, auth)).body.code, 'passkey_not_accepted');
});

test('remove/begin names the account the signature must name', async () => {
  const { env, k0, token } = await environment();
  const auth = new SoftAuthenticator(-7);
  assert.equal((await addPasskey(env, token, k0, auth)).status, 200);
  const begun = await post(env, '/api/passkeys/remove/begin', { id: auth.id }, token);
  assert.equal(begun.status, 200);
  assert.equal(begun.body.account_id, OWNER);
});

// ── Every admin route needs an identity-file session (McKale, 2026-10-08) ────

const ADMIN_SAMPLE = [
  ['POST', '/api/admin/ban', { username: 'nobody-here', reason: 'test' }],
  ['POST', '/api/admin/delete-user', { username: 'nobody-here' }],
  ['GET', '/api/admin/treasury/holds'],
  ['POST', '/api/admin/treasury/holds/resolve', {}],
  ['POST', '/api/payouts/daily', { date: '2026-01-01' }],
  ['GET', '/api/admin/takedowns'],
];

test('an administrator signed in with a passkey is refused on admin routes, and allowed with the identity file', async () => {
  const { env, keys } = await environment();
  const { token: pk } = await passkeySession(env, mintToken(ADMIN), keys[ADMIN], { account: ADMIN });
  for (const [method, route, body] of ADMIN_SAMPLE) {
    const r = await send(env, method, route, { token: pk, body });
    assert.equal(r.status, 403, `${route}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'identity_file_required', route);
    assert.equal(r.body.error, 'Sign in with your identity file to change your keys.');
  }
  assert.equal(JSON.parse(await env.USERS.get(`user:${OTHER}`)).banned, undefined, 'nobody was banned');
  const admin = mintToken(ADMIN);
  for (const [method, route, body] of ADMIN_SAMPLE) {
    const r = await send(env, method, route, { token: admin, body });
    assert.notEqual(r.body?.code, 'identity_file_required', `${route}: ${JSON.stringify(r.body)}`);
    assert.notEqual(r.body?.error, 'Admin access required', `${route}: ${JSON.stringify(r.body)}`);
  }
  // An account that is not an administrator is told what it always was.
  const { token: userPk } = await passkeySession(env, mintToken(OTHER), keys[OTHER], { account: OTHER });
  const plain = await send(env, 'POST', '/api/admin/ban', { token: userPk, body: { username: 'owner' } });
  assert.equal(plain.status, 403);
  assert.equal(plain.body.error, 'Admin access required');
});

test('a passkey session gets no admin power where a route only shows an administrator more', async () => {
  const { env, keys } = await environment();
  const sim = 'abcdef12-3456-4789-8abc-def123456789';
  await env.SOCIAL.put(`sim:${sim}`, JSON.stringify({ id: sim, name: 'Held', author_id: OTHER, is_public: true, moderation: { status: 'pending' } }));
  const asAdmin = await send(env, 'GET', `/api/simulations/${sim}`, { token: mintToken(ADMIN) });
  assert.equal(asAdmin.status, 200, JSON.stringify(asAdmin.body));
  const { token: pk } = await passkeySession(env, mintToken(ADMIN), keys[ADMIN], { account: ADMIN });
  const asPasskey = await send(env, 'GET', `/api/simulations/${sim}`, { token: pk });
  assert.equal(asPasskey.status, 404, 'the held listing is not shown to a passkey session');
});

// ── Configuration ────────────────────────────────────────────────────────────

test('without PASSKEY_RP_ID, PASSKEY_ORIGINS or the passkey index every passkey route answers 503', async () => {
  for (const missing of [{ PASSKEY_RP_ID: undefined }, { PASSKEY_ORIGINS: undefined }, { PASSKEY_ORIGINS: 'https://evil.example' }, { noIndex: true }]) {
    const { env, token } = await environment(missing);
    if (missing.noIndex) env.PASSKEY_INDEX = undefined;
    for (const [method, route] of [['GET', '/api/passkeys'], ['POST', '/api/passkeys/add/begin'], ['POST', '/api/passkeys/add'], ['POST', '/api/passkeys/remove/begin'], ['POST', '/api/passkeys/remove'], ['POST', '/api/passkeys/login/begin'], ['POST', '/api/passkeys/login']]) {
      const r = await send(env, method, route, { token, body: {} });
      assert.equal(r.status, 503, `${JSON.stringify(missing)} ${route}`);
      assert.equal(r.body.code, 'passkeys_unavailable');
    }
  }
});

test('wrangler.toml sets the relying party for production and for staging, each its own', () => {
  const text = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'wrangler.toml'), 'utf8').split('\r\n').join('\n');
  const stagingAt = text.indexOf('[env.staging]');
  const sections = { production: text.slice(0, stagingAt), staging: text.slice(stagingAt) };
  const vars = (section) => Object.fromEntries([...section.matchAll(/^(PASSKEY_[A-Z_]+) = "([^"]*)"/gm)].map((m) => [m[1], m[2]]));
  assert.deepEqual(vars(sections.production), { PASSKEY_RP_ID: 'eustress.dev', PASSKEY_ORIGINS: 'https://eustress.dev' });
  assert.deepEqual(vars(sections.staging), { PASSKEY_RP_ID: 'staging.eustress-cbb.pages.dev', PASSKEY_ORIGINS: 'https://staging.eustress-cbb.pages.dev' });
  for (const section of Object.values(sections)) assert.ok(passkeyConfig(vars(section)), 'each environment\'s values make a relying party');
  // The passkey index object is bound and migrated in each environment, once, under its own tag.
  for (const [name, section] of Object.entries(sections)) {
    assert.match(section, /name = "PASSKEY_INDEX"\nclass_name = "PasskeyIndex"/, `${name} binds PASSKEY_INDEX`);
    assert.equal([...section.matchAll(/tag = "v8-passkey-index"\nnew_sqlite_classes = \["PasskeyIndex"\]/g)].length, 1, `${name} migrates PasskeyIndex once`);
  }
  assert.match(fs.readFileSync(path.resolve(import.meta.dirname, '..', 'src', 'index.js'), 'utf8'), /export \{ PasskeyIndex \} from '\.\/passkey_index\.mjs';/);
});

test('the relying party is read from the environment; an origin outside the RP id is dropped', () => {
  assert.equal(passkeyConfig({}), null);
  const c = passkeyConfig({ PASSKEY_RP_ID: 'Staging.Eustress-cbb.pages.dev', PASSKEY_ORIGINS: 'https://staging.eustress-cbb.pages.dev, https://eustress.dev, http://staging.eustress-cbb.pages.dev' });
  assert.equal(c.rpId, 'staging.eustress-cbb.pages.dev');
  assert.deepEqual([...c.origins], ['https://staging.eustress-cbb.pages.dev']);
});

test('the add and remove routes need a session, and an API origin this deployment signs on', async () => {
  const { env, token } = await environment();
  assert.equal((await post(env, '/api/passkeys/add/begin', {})).status, 401);
  assert.equal((await send(env, 'GET', '/api/passkeys', {})).status, 401);
  const elsewhere = await post(env, '/api/passkeys/add/begin', {}, token, 'https://eustress-api.example.workers.dev');
  assert.equal(elsewhere.status, 400);
  assert.equal(elsewhere.body.code, 'origin_not_allowed');
});

test('a CBOR helper round trip: the encoder in the test helper and the decoder agree', () => {
  const value = new Map([['fmt', 'none'], ['attStmt', new Map()], ['authData', authenticatorData({ rpId: 'eustress.dev', flags: { up: true, uv: true } })]]);
  const back = decodeCbor(new Uint8Array(cbor(value)));
  assert.equal(back.get('fmt'), 'none');
  assert.equal(back.get('authData').length, 37);
});
