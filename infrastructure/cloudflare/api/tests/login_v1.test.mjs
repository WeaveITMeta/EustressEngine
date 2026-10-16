// Signing in with an identity key: the signature covers a message that names the
// API it is for (docs/architecture/STUDIO_IDENTITY_SIGNIN.md), so a challenge
// relayed through another API cannot be turned into a sign-in here. The vectors
// are the file Studio's tests read too (docs/architecture/login-v1-vectors.json),
// so both sides agree on every byte.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();
const vectors = JSON.parse(fs.readFileSync(new URL('../../../../docs/architecture/login-v1-vectors.json', import.meta.url), 'utf8'));

const ACCOUNT = '33333333-3333-4333-8333-333333333333';
const PRODUCTION = 'https://api.eustress.dev';

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

const message = (origin, challenge) => `eustress-login-v1\n${origin}\n${challenge}`;

const SIGN_IN_ORIGINS = 'https://api.eustress.dev, https://api-staging.eustress.dev, http://127.0.0.1:8787';
const chalKey = (challenge) => `chal:${crypto.createHash('sha256').update(challenge).digest('hex')}`;

async function environment(key, extra = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: SIGN_IN_ORIGINS, ...extra };
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ id: ACCOUNT, username: 'signer', public_key: key }));
  await env.USERS.put(`pubkey:${key}`, ACCOUNT);
  return env;
}

/// A request that reaches the Worker at `origin`, the way a client reaches an API.
async function post(env, origin, route, body) {
  const res = await worker.fetch(new Request(`${origin}${route}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

const challengeFor = async (env, public_key, origin = PRODUCTION) => (await post(env, origin, '/api/auth/challenge', { public_key })).body.challenge;
const verify = (env, origin, body) => post(env, origin, '/api/auth/verify-challenge', { format: 'eustress-login-v1', ...body });

// ── The shared vectors ───────────────────────────────────────────────────────

test('the vectors are what they say: each signature verifies over its own message only', () => {
  const publicKey = crypto.createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: Buffer.from(vectors.public_key, 'hex').toString('base64url') }, format: 'jwk' });
  for (const v of vectors.signatures) {
    assert.equal(v.message, message(v.origin, v.challenge), 'the message is the three lines, joined by newlines, no trailing newline');
    assert.equal(crypto.verify(null, Buffer.from(v.message), publicKey, Buffer.from(v.signature, 'hex')), true, v.origin);
    for (const other of vectors.signatures.filter((o) => o.origin !== v.origin)) {
      assert.equal(crypto.verify(null, Buffer.from(other.message), publicKey, Buffer.from(v.signature, 'hex')), false, `${v.origin} signature on ${other.origin}'s message`);
    }
  }
});

test('the origin of a base address is what Studio says it is', () => {
  for (const { base, origin } of vectors.origins) assert.equal(new URL(base).origin, origin, base);
});

test('the Worker accepts each vector at its own origin, and refuses it at every other', async () => {
  for (const v of vectors.signatures) {
    for (const reached of vectors.signatures.map((s) => s.origin)) {
      const env = await environment(vectors.public_key);
      await env.CHALLENGES.put(chalKey(v.challenge), JSON.stringify({ public_key: vectors.public_key, challenge: v.challenge, expires_at: new Date(Date.now() + 60_000).toISOString() }));
      const r = await verify(env, reached, { public_key: vectors.public_key, challenge: v.challenge, signature: v.signature });
      if (reached === v.origin) assert.equal(r.status, 200, `${v.origin}: ${JSON.stringify(r.body)}`);
      else assert.equal(r.status, 401, `signed for ${v.origin}, sent to ${reached}: got ${r.status}`);
    }
  }
});

// ── The flow ─────────────────────────────────────────────────────────────────

test('signing the login message for this API signs in, with the account and a token', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  assert.match(challenge, /^[0-9a-f]{64}$/);
  const r = await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.user.username, 'signer');
  assert.equal(r.body.user.id, ACCOUNT);
  assert.ok(r.body.token.split('.').length === 3);
});

test('a challenge relayed through another API cannot sign in here', async () => {
  // A Studio pointed at evil.example is handed this API's challenge and signs
  // the message for the origin it is talking to.
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  const relayed = await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message('https://evil.example', challenge)) });
  assert.equal(relayed.status, 401);
  // The challenge is spent, so the genuine signature cannot follow it.
  const late = await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) });
  assert.equal(late.status, 400);
});

test('staging, production and a local API each need their own signature', async () => {
  const key = keyPair();
  for (const origin of ['https://api-staging.eustress.dev', 'http://127.0.0.1:8787']) {
    const env = await environment(key.hex);
    const challenge = await challengeFor(env, key.hex, origin);
    assert.equal((await verify(env, origin, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) })).status, 401, `${origin} given production's signature`);
    const again = await challengeFor(env, key.hex, origin);
    assert.equal((await verify(env, origin, { public_key: key.hex, challenge: again, signature: key.sign(message(origin, again)) })).status, 200, origin);
  }
});

test('the bare challenge is no longer a login message: no format, or the old signature, is refused', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  const noFormat = await post(env, PRODUCTION, '/api/auth/verify-challenge', { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) });
  assert.equal(noFormat.status, 400);
  assert.equal(noFormat.body.code, 'login_format_required');
  for (const format of ['eustress-login-v0', 'eustress-login-v2', 'EUSTRESS-LOGIN-V1', 'eustress-login-v1 ', '', null, 1, undefined]) {
    const r = await post(env, PRODUCTION, '/api/auth/verify-challenge', { format, public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) });
    assert.equal(r.status, 400, `format ${JSON.stringify(format)}`);
  }
  // Refused for its format, the challenge was not used up: the right message still signs in.
  assert.equal((await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) })).status, 200);
  // A signature over the bare challenge, even with the format named, is no login.
  const fresh = await challengeFor(env, key.hex);
  assert.equal((await verify(env, PRODUCTION, { public_key: key.hex, challenge: fresh, signature: key.sign(fresh) })).status, 401);
});

test('a used challenge is refused, and so is an expired one', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  const good = { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) };
  assert.equal((await verify(env, PRODUCTION, good)).status, 200);
  const again = await verify(env, PRODUCTION, good);
  assert.equal(again.status, 400);
  assert.equal(again.body.error, 'No pending challenge');

  const stale = await challengeFor(env, key.hex);
  await env.CHALLENGES.put(chalKey(stale), JSON.stringify({ public_key: key.hex, challenge: stale, expires_at: new Date(Date.now() - 1000).toISOString() }));
  const r = await verify(env, PRODUCTION, { public_key: key.hex, challenge: stale, signature: key.sign(message(PRODUCTION, stale)) });
  assert.equal(r.status, 400);
  assert.equal(r.body.error, 'Challenge expired');
});

test('asking for a challenge in someone else\'s name does not take theirs away', async () => {
  const victim = keyPair();
  const env = await environment(victim.hex);
  const theirs = await challengeFor(env, victim.hex);
  // An attacker asks for challenges for the victim's key, repeatedly.
  for (let i = 0; i < 3; i++) await challengeFor(env, victim.hex);
  const r = await verify(env, PRODUCTION, { public_key: victim.hex, challenge: theirs, signature: victim.sign(message(PRODUCTION, theirs)) });
  assert.equal(r.status, 200, JSON.stringify(r.body));
});

test('a challenge belongs to the key it was issued for', async () => {
  const victim = keyPair();
  const attacker = keyPair();
  const env = await environment(victim.hex);
  await env.USERS.put(`pubkey:${attacker.hex}`, ACCOUNT);
  const challenge = await challengeFor(env, victim.hex);
  const r = await verify(env, PRODUCTION, { public_key: attacker.hex, challenge, signature: attacker.sign(message(PRODUCTION, challenge)) });
  assert.equal(r.status, 400);
  assert.equal(r.body.error, 'Challenge mismatch');
});

test('malformed requests are refused without leaking why a signature failed', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  const good = key.sign(message(PRODUCTION, challenge));
  // Not a challenge this API issued.
  for (const bad of ['not-a-challenge', 'a'.repeat(64), `${'A'.repeat(64)}`, 42, ['x']]) {
    assert.equal((await verify(env, PRODUCTION, { public_key: key.hex, challenge: bad, signature: good })).status, 400, JSON.stringify(bad));
  }
  // A signature that is not 128 hex characters.
  for (const bad of ['zz'.repeat(64), good.slice(2), good + '00', 7, '']) {
    const c = await challengeFor(env, key.hex);
    const r = await verify(env, PRODUCTION, { public_key: key.hex, challenge: c, signature: bad });
    assert.ok([400, 401].includes(r.status), `${JSON.stringify(bad)} got ${r.status}`);
    assert.ok(!/error:|Invalid|hex|Uint8|Failed to/i.test(JSON.stringify(r.body).replace('Signature verification failed', '')), JSON.stringify(r.body));
  }
  assert.equal((await post(env, PRODUCTION, '/api/auth/verify-challenge', 'not json')).status, 400);
});

test('an unregistered key gets 404 only after a valid signature, and a suspended account its reason', async () => {
  const stranger = keyPair();
  const env = await environment(keyPair().hex);
  const challenge = await challengeFor(env, stranger.hex);
  assert.equal((await verify(env, PRODUCTION, { public_key: stranger.hex, challenge, signature: stranger.sign(message(PRODUCTION, challenge)) })).status, 404);

  const key = keyPair();
  const banned = await environment(key.hex);
  await banned.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ id: ACCOUNT, username: 'signer', public_key: key.hex, banned: true, ban_reason: 'testing' }));
  const c = await challengeFor(banned, key.hex);
  const r = await verify(banned, PRODUCTION, { public_key: key.hex, challenge: c, signature: key.sign(message(PRODUCTION, c)) });
  assert.equal(r.status, 403);
  assert.equal(r.body.reason, 'testing');
});

// ── Where a sign-in may happen ───────────────────────────────────────────────

test('only the origins this deployment lists sign anyone in', async () => {
  const key = keyPair();
  // The same Worker reached on a hostname nobody listed (workers.dev, a preview).
  const env = await environment(key.hex);
  const unlisted = 'https://eustress-api.example.workers.dev';
  const challenge = await challengeFor(env, key.hex, unlisted);
  const refused = await verify(env, unlisted, { public_key: key.hex, challenge, signature: key.sign(message(unlisted, challenge)) });
  assert.equal(refused.status, 400);
  assert.equal(refused.body.code, 'login_origin_not_allowed');
  // Refusing the address did not use the challenge up.
  assert.equal((await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) })).status, 200);

  // With nothing configured, production's origin is the only one.
  const bare = await environment(key.hex, { LOGIN_ORIGINS: undefined });
  for (const [origin, expected] of [['https://api-staging.eustress.dev', 400], ['http://127.0.0.1:8787', 400], [PRODUCTION, 200]]) {
    const c = await challengeFor(bare, key.hex, origin);
    const r = await verify(bare, origin, { public_key: key.hex, challenge: c, signature: key.sign(message(origin, c)) });
    assert.equal(r.status, expected, origin);
  }
});

test('the challenges are filed by digest under the challenge, not by key', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const challenge = await challengeFor(env, key.hex);
  assert.deepEqual([...env.CHALLENGES.store.keys()], [chalKey(challenge)]);
  const record = JSON.parse(env.CHALLENGES.store.get(chalKey(challenge)));
  assert.deepEqual(Object.keys(record).sort(), ['challenge', 'expires_at', 'public_key']);
  assert.equal(record.public_key, key.hex);
  assert.ok(![...env.CHALLENGES.store.keys()].some((k) => k.includes(challenge) || k.includes(key.hex)), 'neither the challenge nor the key is a storage key');
});

// ── Signing out ──────────────────────────────────────────────────────────────

async function me(env, token) {
  const res = await worker.fetch(new Request(`${PRODUCTION}/api/auth/me`, { headers: token ? { Authorization: `Bearer ${token}` } : {} }), env, ctx);
  return res.status;
}
async function logout(env, token) {
  const res = await worker.fetch(new Request(`${PRODUCTION}/api/auth/logout`, { method: 'POST', headers: token ? { Authorization: `Bearer ${token}` } : {} }), env, ctx);
  return res.status;
}
const claims = (token) => JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString());

test('signing out ends every token the account holds, and signing in again works at once', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const other = '44444444-4444-4444-8444-444444444444';
  await env.USERS.put(`user:${other}`, JSON.stringify({ id: other, username: 'bystander' }));
  const signIn = async () => {
    const challenge = await challengeFor(env, key.hex);
    return (await verify(env, PRODUCTION, { public_key: key.hex, challenge, signature: key.sign(message(PRODUCTION, challenge)) })).body.token;
  };

  const first = await signIn();
  const bystander = mintToken(other);
  assert.equal(await me(env, first), 200);
  assert.equal(await logout(env, undefined), 401, 'signing out takes a token');
  assert.equal(await logout(env, first), 200);
  assert.equal(await me(env, first), 401, 'the old token is dead');
  assert.equal(await me(env, bystander), 200, 'another account is untouched');

  // Straight away, in the same second: the new token is not caught by the floor.
  const second = await signIn();
  assert.equal(await me(env, second), 200);
  assert.ok(claims(second).iat >= Number(await env.USERS.get(`authfloor:${ACCOUNT}`)), 'issued no earlier than the floor');

  // And a second sign-out ends that one too.
  assert.equal(await logout(env, second), 200);
  assert.equal(await me(env, second), 401);
  assert.equal(await me(env, await signIn()), 200);
});

test('a ban and a deletion end the tokens the account already holds', async () => {
  const ADMIN = '99999999-9999-4999-8999-999999999999';
  for (const [label, route, body] of [
    ['ban', '/api/admin/ban', { username: 'signer', reason: 'testing' }],
    ['deletion', '/api/admin/delete-user', { username: 'signer' }],
  ]) {
    const key = keyPair();
    const env = await environment(key.hex);
    await env.USERS.put('username:signer', ACCOUNT);
    await env.USERS.put(`admin:${ADMIN}`, '1');
    await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user' }));
    const held = mintToken(ACCOUNT);
    assert.equal(await me(env, held), 200, label);
    const res = await worker.fetch(new Request(`${PRODUCTION}${route}`, { method: 'POST', headers: { Authorization: `Bearer ${mintToken(ADMIN)}`, 'Content-Type': 'application/json' }, body: JSON.stringify(body) }), env, ctx);
    assert.equal(res.status, 200, label);
    assert.equal(await me(env, held), 401, `${label}: the held token is dead`);
    assert.equal(await me(env, mintToken(ADMIN)), 200, `${label}: the admin is untouched`);
  }
});

test('a token with no expiry is refused', async () => {
  const key = keyPair();
  const env = await environment(key.hex);
  const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
  const h = b64({ alg: 'HS256', typ: 'JWT' });
  const p = b64({ sub: ACCOUNT, iat: Math.floor(Date.now() / 1000) });
  const sig = crypto.createHmac('sha256', 'test-secret-not-real').update(`${h}.${p}`).digest('base64url');
  assert.equal(await me(env, `${h}.${p}.${sig}`), 401);
  assert.equal(await me(env, mintToken(ACCOUNT)), 200);
});
