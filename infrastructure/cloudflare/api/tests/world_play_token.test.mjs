// The world play token (src/world_play_token.mjs, docs/architecture/WORLD_PLAY_TOKEN.md): a manifest GET that anyone signed
// out could make hands back a short-lived token, and a chunk GET that presents it is served without the listing record from
// KV. Every refusal the full gate makes must still be made: a token for another listing, another manifest, past its expiry,
// forged, or minted under an approval that has since been revoked, locked or deleted, buys nothing.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { handleWorldRoute, manifestKey, chunkKey } from '../src/world.mjs';
import { isListable, canServe, identOfObject } from '../src/moderation.mjs';
import { mintPlayToken, readPlayToken, PLAY_TOKEN_HEADER, PLAY_TOKEN_EXPIRES_HEADER, PLAY_TOKEN_TTL_SECONDS } from '../src/world_play_token.mjs';
import { deletionMarkKey } from '../src/space_delete_mark.mjs';

const SECRET = 'test-secret-not-real';

// ── fakes ───────────────────────────────────────────────────────────────────

/// KV that counts reads of listing records, so a test can show the token path never read one.
function kv() {
  const m = new Map();
  const store = { m, reads: 0, get: async (k) => { if (k.startsWith('sim:')) store.reads++; return m.get(k) ?? null; }, put: async (k, v) => { m.set(k, String(v)); } };
  return store;
}

/// R2 whose head and get carry an etag (the bytes' SHA-256) and a version that moves on every put, as R2's do.
function r2() {
  const m = new Map();
  let version = 0;
  const bytesOf = async (value) => {
    if (value instanceof Uint8Array) return value;
    if (typeof value === 'string') return new TextEncoder().encode(value);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  const meta = (key) => {
    const e = m.get(key);
    return { key, size: e.bytes.length, etag: e.etag, version: e.version, customMetadata: e.customMetadata };
  };
  return {
    m,
    async put(key, value, opts = {}) {
      const bytes = await bytesOf(value);
      m.set(key, { bytes, etag: createHash('sha256').update(bytes).digest('hex'), version: `v${++version}`, customMetadata: opts.customMetadata || {} });
      return { key };
    },
    async head(key) { return m.has(key) ? meta(key) : null; },
    async get(key) {
      if (!m.has(key)) return null;
      const b = m.get(key).bytes;
      return { ...meta(key), body: new Response(b).body, arrayBuffer: async () => b.slice().buffer, text: async () => new TextDecoder().decode(b) };
    },
    async delete(key) { m.delete(key); },
    async list({ prefix = '' } = {}) {
      return { objects: [...m.keys()].filter((k) => k.startsWith(prefix)).map((key) => ({ key, size: m.get(key).bytes.length })), truncated: false };
    },
  };
}

const json = (value, status = 200, headers = {}) =>
  new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

const deps = {
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace(/^Bearer /, '') || null,
  requireAdmin: async (req) => ((req.headers.get('Authorization') || '') === 'Bearer admin' ? 'admin' : null),
  json,
  isListable,
  canServe,
  readSpaceNames: (list) => (Array.isArray(list) ? [...new Set(list)] : null),
  MODERATION_VERSION: '1.0',
  POLICY_VERSION: '1.2',
};

const SIM = '11111111-2222-3333-4444-555555555555';
const OTHER_SIM = '99999999-2222-3333-4444-555555555555';
const H1 = 'a'.repeat(64);
const H2 = 'b'.repeat(64);
const H_STRAY = 'd'.repeat(64);
const C1 = new TextEncoder().encode('ECHK chunk one');
const C2 = new TextEncoder().encode('ECHK chunk two');
const STRAY = new TextEncoder().encode('ECHK a chunk no committed manifest names');

/// An approved, public, all-ages listing with a committed world of two chunks and an approval bound to its manifest. The
/// same world is published under OTHER_SIM, pending, so a token can be carried across listings.
async function world({ rating = 'all_ages', status = 'approved', isPublic = true, bind = true, secret = SECRET } = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2(), ...(secret ? { JWT_SECRET: secret } : {}) };
  const text = JSON.stringify({ format: 'echk', spaces: [{ name: 'Pong', chunks: [{ blake3: H1, size: C1.length }, { blake3: H2, size: C2.length }] }], assets: [] });
  const sha = createHash('sha256').update(text).digest('hex');
  for (const id of [SIM, OTHER_SIM]) {
    await env.SCENES.put(manifestKey(id, sha), text);
    await env.SCENES.put(chunkKey(id, H1), C1);
    await env.SCENES.put(chunkKey(id, H2), C2);
    await env.SCENES.put(chunkKey(id, H_STRAY), STRAY);
    env.SOCIAL.m.set(`sim:${id}`, JSON.stringify({
      id, name: 'Pong', author_id: 'author', is_public: id === SIM ? isPublic : true, version: 1, format: 'echk', r2_key: manifestKey(id, sha),
      world: { manifest_key: manifestKey(id, sha), manifest_sha256: sha }, moderation: { status: id === SIM ? status : 'pending', rating },
    }));
  }
  if (bind) await approveContent(env);
  return { env, sha };
}

/// The approval binding as moderation.mjs writeApproval stores it, bound to the listing's current manifest object.
async function approveContent(env) {
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  const content = identOfObject(await env.SCENES.head(sim.r2_key));
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, content, thumb: null, cover: null, source: null, captures: null, evidence_epoch: 0, by: 'test', at: 'now' }));
  return content;
}

async function call(env, path, { sim = SIM, who, token } = {}) {
  const headers = new Headers();
  if (who) headers.set('Authorization', `Bearer ${who}`);
  if (token) headers.set(PLAY_TOKEN_HEADER, token);
  const url = new URL(`https://api.eustress.dev/api/simulations/${sim}/world/${path}`);
  return handleWorldRoute(new Request(url, { method: 'GET', headers }), url, env, {}, deps);
}

async function tokenFromManifest(env, opts) {
  const res = await call(env, 'manifest', opts);
  assert.equal(res.status, 200, 'the manifest serves');
  return res.headers.get(PLAY_TOKEN_HEADER);
}

const viaToken = (res) => res.headers.get('X-Eustress-Chunk-Gate') === 'play-token';

// ── minting ─────────────────────────────────────────────────────────────────

test('a manifest anyone signed out may read comes with a token, and its chunks are served on it without a KV read', async () => {
  const { env } = await world();
  const res = await call(env, 'manifest');
  assert.equal(res.status, 200);
  const token = res.headers.get(PLAY_TOKEN_HEADER);
  assert.match(token, /^wpt1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]{43}$/);
  const expires = Number(res.headers.get(PLAY_TOKEN_EXPIRES_HEADER));
  const now = Math.floor(Date.now() / 1000);
  assert.ok(expires > now && expires <= now + PLAY_TOKEN_TTL_SECONDS, 'expires within one lifetime');

  env.SOCIAL.reads = 0;
  for (const [hash, bytes] of [[H1, C1], [H2, C2]]) {
    const chunk = await call(env, `chunks/${hash}`, { token });
    assert.equal(chunk.status, 200);
    assert.ok(viaToken(chunk), 'served on the token');
    assert.deepEqual(new Uint8Array(await chunk.arrayBuffer()), bytes);
  }
  assert.equal(env.SOCIAL.reads, 0, 'the listing record was never read');

  // Control: the same chunk without the token runs the full gate, which reads the listing record.
  const full = await call(env, `chunks/${H1}`);
  assert.equal(full.status, 200);
  assert.equal(viaToken(full), false);
  assert.equal(env.SOCIAL.reads, 1);
});

test('a teen_13 or mature_17 world is solo play open signed out (McKale, 2026-11-19), so its manifest comes with a token too', async () => {
  for (const rating of ['ages_10', 'teen_13', 'mature_17']) {
    const { env } = await world({ rating });
    const token = await tokenFromManifest(env);
    assert.match(String(token), /^wpt1\./, rating);
    const chunk = await call(env, `chunks/${H1}`, { token });
    assert.deepEqual([chunk.status, viaToken(chunk)], [200, true], rating);
  }
});

test('no token where the manifest was not open to anyone signed out', async () => {
  const cases = [
    ['adult content', { rating: 'adult_18' }, {}],
    ['adult content, read by its author', { rating: 'adult_18' }, { who: 'author' }],
    ['21+ content, read by its author', { rating: 'adult_21' }, { who: 'author' }],
    ['no rating, read by its author', { rating: null }, { who: 'author' }],
    ['pending, read by its author', { status: 'pending' }, { who: 'author' }],
    ['private, read by its author', { isPublic: false }, { who: 'author' }],
    ['no approval binding', { bind: false }, {}],
    ['no JWT_SECRET', { secret: null }, {}],
  ];
  for (const [name, setup, opts] of cases) {
    const { env } = await world(setup);
    const res = await call(env, 'manifest', opts);
    // An adult listing refuses a signed-out reader outright; every other case serves the manifest, without a token.
    if (setup.rating === 'adult_18' && !opts.who) assert.notEqual(res.status, 200, name);
    else assert.equal(res.status, 200, `${name}: the manifest still serves`);
    assert.equal(res.headers.get(PLAY_TOKEN_HEADER), null, `${name}: no token`);
  }
});

// ── what a token cannot do ──────────────────────────────────────────────────

test('a token for one listing is nothing on another', async () => {
  const { env } = await world();
  const token = await tokenFromManifest(env);
  // OTHER_SIM holds the same chunks but is pending: the full gate refuses a stranger.
  const res = await call(env, `chunks/${H1}`, { sim: OTHER_SIM, token });
  assert.equal(viaToken(res), false);
  assert.equal(res.status, 403);
  assert.equal(await readPlayToken(env, token, OTHER_SIM), null);
});

test('a token serves only the chunks its own manifest names', async () => {
  const { env } = await world();
  const token = await tokenFromManifest(env);
  // Stored under the listing, named by no committed manifest: the full gate's 404.
  const res = await call(env, `chunks/${H_STRAY}`, { token });
  assert.equal(viaToken(res), false);
  assert.equal(res.status, 404);
});

test('a token bound to another manifest hash serves nothing', async () => {
  const { env } = await world();
  const content = identOfObject(await env.SCENES.head(JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)).r2_key));
  // Correctly signed, right listing, right approval, but naming a manifest that is not stored.
  const minted = await mintPlayToken(env, { simId: SIM, manifestSha: 'e'.repeat(64), ident: content });
  const res = await call(env, `chunks/${H1}`, { token: minted.token });
  assert.equal(viaToken(res), false, 'falls through to the full gate');
});

test('an expired token is refused, and so is one dated further out than one lifetime', async () => {
  const { env, sha } = await world();
  const content = await approveContent(env);
  const now = Math.floor(Date.now() / 1000);
  const old = await mintPlayToken(env, { simId: SIM, manifestSha: sha, ident: content }, now - PLAY_TOKEN_TTL_SECONDS - 1);
  assert.equal(await readPlayToken(env, old.token, SIM, now), null);
  assert.equal(viaToken(await call(env, `chunks/${H1}`, { token: old.token })), false);
  // The last second of its life it is good; at its expiry it is not.
  const fresh = await mintPlayToken(env, { simId: SIM, manifestSha: sha, ident: content }, now);
  assert.ok(await readPlayToken(env, fresh.token, SIM, fresh.expires - 1));
  assert.equal(await readPlayToken(env, fresh.token, SIM, fresh.expires), null);
  const future = await mintPlayToken(env, { simId: SIM, manifestSha: sha, ident: content }, now + 3600);
  assert.equal(await readPlayToken(env, future.token, SIM, now), null);
});

test('a forged or altered token is refused', async () => {
  const { env, sha } = await world();
  const token = await tokenFromManifest(env);
  const [prefix, body, mac] = token.split('.');
  const claims = JSON.parse(Buffer.from(body, 'base64url').toString());
  const b64 = (v) => Buffer.from(JSON.stringify(v)).toString('base64url');
  const forgeries = {
    'claims changed, MAC kept': `${prefix}.${b64({ ...claims, exp: claims.exp + 10 })}.${mac}`,
    'MAC under JWT_SECRET itself': `${prefix}.${body}.${createHmac('sha256', SECRET).update(`${prefix}.${body}`).digest('base64url')}`,
    'MAC under the right label and another secret': (() => {
      const key = createHmac('sha256', 'another-secret').update('world-play-token-v1').digest();
      return `${prefix}.${body}.${createHmac('sha256', key).update(`${prefix}.${body}`).digest('base64url')}`;
    })(),
    'MAC truncated': `${prefix}.${body}.${mac.slice(0, 20)}`,
    'no MAC': `${prefix}.${body}.`,
    'another prefix': `wpt2.${body}.${mac}`,
    'a session token': 'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhdXRob3IifQ.c2ln',
    'not base64': `${prefix}.${body}!.${mac}`,
    'too long': `${prefix}.${'A'.repeat(2000)}.${mac}`,
  };
  for (const [name, forged] of Object.entries(forgeries)) {
    assert.equal(await readPlayToken(env, forged, SIM), null, name);
    assert.equal(viaToken(await call(env, `chunks/${H1}`, { token: forged })), false, name);
  }
  // The genuine token still works, so the forgeries failed for being forgeries.
  assert.ok(viaToken(await call(env, `chunks/${H1}`, { token })));
  assert.equal(claims.sha, sha);
});

// ── refusals that arrive after the mint ─────────────────────────────────────

test('an approval revoked after the mint ends the token at once', async () => {
  const { env } = await world();
  const token = await tokenFromManifest(env);
  assert.ok(viaToken(await call(env, `chunks/${H1}`, { token })), 'control: served before the revoke');
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, revoked: true, at: 'now' }));
  const res = await call(env, `chunks/${H1}`, { token });
  assert.equal(viaToken(res), false);
  assert.equal(res.status, 403, 'the full gate reads the revoked approval as pending and refuses a stranger');
});

test('a legal lock or a deletion after the mint ends the token at once', async () => {
  for (const [name, key, meta] of [
    ['serve_block lock', `locks/legal/${SIM}`, { kind: 'serve_block' }],
    ['content_lock', `locks/legal/${SIM}`, { kind: 'content_lock' }],
    ['deletion mark', deletionMarkKey(SIM), {}],
  ]) {
    const { env } = await world();
    const token = await tokenFromManifest(env);
    await env.SCENES.put(key, '{}', { customMetadata: meta });
    const res = await call(env, `chunks/${H1}`, { token });
    assert.equal(viaToken(res), false, name);
    assert.equal(res.status, 403, `${name}: refused`);
  }
});

test('a new approval (a reviewed republish) retires tokens minted under the old one', async () => {
  const { env } = await world();
  const token = await tokenFromManifest(env);
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, content: 'sha256:' + 'f'.repeat(64), at: 'later' }));
  assert.equal(viaToken(await call(env, `chunks/${H1}`, { token })), false);
});

test('documented window: a change only the KV record holds (the author makes it private) reaches token holders at expiry', async () => {
  const { env } = await world();
  const token = await tokenFromManifest(env);
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...sim, is_public: false }));
  // A new download cannot start: the manifest runs the full gate.
  assert.equal((await call(env, 'manifest')).status, 403);
  // A download already under way finishes within the token's life (WORLD_PLAY_TOKEN.md, "What the token does not see").
  assert.ok(viaToken(await call(env, `chunks/${H1}`, { token })));
});
