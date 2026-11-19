// A private source world (src/world.mjs): Studio commits the source on every publish, and Share Source decides who reads it.
// Off, only the author (their session, their esrv_, or an eshs_ the fleet minted for that listing) and an admin read it, never
// while the listing is deleted or under a legal lock, and it is never cacheable. On, it reads publicly as before.
// docs/architecture/PRIVATE_SOURCE_SECURITY_REVIEW.md.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { handleWorldRoute } from '../src/world.mjs';
import { isListable, canServe } from '../src/moderation.mjs';
import { Fleet } from '../src/fleet.mjs';
import { DELETED_PREFIX } from '../src/space_delete_mark.mjs';
import { fakeDoNamespace } from './helpers/load_worker.mjs';

const SIM = '11111111-2222-3333-4444-555555555555';
const OTHER = '99999999-8888-4777-8666-555555555555';
const H1 = 'a'.repeat(64);
const H2 = 'b'.repeat(64);
const H4 = 'd'.repeat(64);

function kv() {
  const m = new Map();
  return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); }, delete: async (k) => { m.delete(k); } };
}

function r2() {
  const m = new Map();
  const meta = new Map();
  const bytesOf = async (value) => {
    if (value instanceof Uint8Array) return value;
    if (typeof value === 'string') return new TextEncoder().encode(value);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  const reading = (b) => ({ size: b.length, body: new Response(b).body, arrayBuffer: async () => b.slice().buffer, text: async () => new TextDecoder().decode(b) });
  return {
    m,
    async put(key, value, opts) { m.set(key, await bytesOf(value)); meta.set(key, opts?.customMetadata || {}); return { key }; },
    async head(key) { const b = m.get(key); return b ? { key, size: b.length, customMetadata: meta.get(key) || {} } : null; },
    async get(key, opts) {
      const b = m.get(key);
      if (!b) return null;
      return reading(opts?.range ? b.slice(opts.range.offset, opts.range.offset + opts.range.length) : b);
    },
    async delete(key) { m.delete(key); },
    async list({ prefix = '' } = {}) {
      const keys = [...m.keys()].filter((k) => k.startsWith(prefix)).sort();
      return { objects: keys.map((key) => ({ key, size: m.get(key).length })), truncated: false };
    },
  };
}

const json = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
// An account id is plain lowercase; a credential of any kind is never an account (as the deployed verifyAuth reads them).
const deps = {
  verifyAuth: async (req) => {
    const t = (req.headers.get('Authorization') || '').replace(/^Bearer /, '');
    return /^[a-z][a-z0-9-]*$/.test(t) ? t : null;
  },
  requireAdmin: async (req) => ((req.headers.get('Authorization') || '') === 'Bearer admin' ? 'admin' : null),
  json,
  isListable,
  canServe,
  readSpaceNames: (list) => (Array.isArray(list) ? [...new Set(list)] : null),
  MODERATION_VERSION: '1.0',
  POLICY_VERSION: '1.2',
};

/// A valid .echk chunk: "ECHK", version 1, one record.
function chunk(text) {
  const path = new TextEncoder().encode('Workspace/P/_instance.toml');
  const data = new TextEncoder().encode(text);
  const out = new Uint8Array(12 + 8 + path.length + data.length);
  const view = new DataView(out.buffer);
  out.set([0x45, 0x43, 0x48, 0x4b]);
  view.setUint32(4, 1, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, path.length, true);
  out.set(path, 16);
  view.setUint32(16 + path.length, data.length, true);
  out.set(data, 20 + path.length);
  return out;
}
const C1 = chunk('[metadata]\nclass_name = "Part"\n');
const C2 = chunk('[metadata]\nclass_name = "SpawnLocation"\n');
const S1 = chunk('[metadata]\nclass_name = "Script"\n');

const entry = (cx, blake3, size, file = `${cx}_0.echk`) => ({ cx, cz: 0, file, size, count: 1, blake3 });
const world = () => ({
  format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Pong', start_space: 'Table',
  spaces: [{ name: 'Table', chunks: [entry(0, H1, C1.length)] }, { name: 'Lobby', chunks: [entry(0, H2, C2.length)] }],
  assets: [],
});
/// The world plus ServerScriptService: one chunk only the source names.
const sourceWorld = () => {
  const w = world();
  w.spaces[0].chunks.push(entry(0, H4, S1.length, 'server.echk'));
  return w;
};

function environment(extra = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2(), FLEET: fakeDoNamespace(Fleet, {}) };
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Pong', author_id: 'author', is_public: true, version: 1, moderation: { status: 'pending' }, ...extra,
  }));
  return env;
}
const listing = (env) => JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
const approve = (env) => env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...listing(env), moderation: { status: 'approved', rating: 'all_ages' } }));

async function call(env, method, path, { who, body, bytes } = {}) {
  const headers = new Headers();
  if (who) headers.set('Authorization', `Bearer ${who}`);
  let payload;
  if (body !== undefined) {
    payload = JSON.stringify(body);
    headers.set('Content-Type', 'application/json');
  }
  if (bytes !== undefined) {
    payload = bytes;
    headers.set('Content-Length', String(bytes.length));
  }
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/${path}`);
  return handleWorldRoute(new Request(url, { method, headers, body: payload }), url, env, {}, deps);
}

/// The world and its source committed by the author, with Share Source as `openSource` says.
async function published(env, { openSource = false } = {}) {
  for (const [hash, bytes] of [[H1, C1], [H2, C2], [H4, S1]]) {
    assert.equal((await call(env, 'PUT', `chunks/${hash}`, { who: 'author', bytes })).status, 201);
  }
  const w = await call(env, 'POST', 'commit', { who: 'author', body: { manifest_json: JSON.stringify(world()), listing: { open_source: openSource } } });
  assert.equal(w.status, 200);
  const s = await call(env, 'POST', 'commit', { who: 'author', body: { manifest_json: JSON.stringify(sourceWorld()), kind: 'source' } });
  assert.equal(s.status, 200, 'a source is committed with Share Source off as well as on');
  approve(env);
}

/// An eshs_ as the fleet object mints one when it tells a host to start a session of `simId` (src/fleet.mjs heartbeat):
/// only the hash is kept, with the listing, the machine, the server and the expiry.
async function mintSession(env, simId, { expiresIn = 600_000 } = {}) {
  const token = `eshs_${createHash('sha256').update(`${simId}:${Math.random()}`).digest('hex')}`;
  const hash = createHash('sha256').update(token).digest('hex');
  env.FLEET.get('fleet');
  await env.FLEET.instances.get('fleet').storage.put(`hostsess:${hash}`, {
    v: 1, sim_id: simId, machine_id: 'm1', server_id: 'srv1', expires_at: Date.now() + expiresIn,
  });
  return token;
}

const status = async (env, path, who) => (await call(env, 'GET', path, { who })).status;

test('the author commits a source world with Share Source off and reads it; the listing stays private about it', async () => {
  const env = environment();
  await published(env);
  assert.equal(listing(env).open_source, false);
  assert.ok(listing(env).world.source_manifest_key, 'the source world is stored');
  const res = await call(env, 'GET', 'source', { who: 'author' });
  assert.equal(res.status, 200);
  assert.equal(await res.text(), JSON.stringify(sourceWorld()));
  assert.equal(res.headers.get('Cache-Control'), 'private, no-store');
  const piece = await call(env, 'GET', `chunks/${H4}`, { who: 'author' });
  assert.equal(piece.status, 200, 'the author reads the server-only chunk');
  assert.equal(piece.headers.get('Cache-Control'), 'private, no-store', 'a private source chunk is never cached');
  assert.equal(await status(env, 'source', 'admin'), 200, 'an admin reads it');
  // The player world never names the server-only chunk.
  assert.equal((await (await call(env, 'GET', 'manifest')).text()).includes(H4), false);
});

test('an anonymous request, and any other account, is refused a private source and its server-only chunks', async () => {
  const env = environment();
  await published(env);
  assert.equal(await status(env, 'source'), 404);
  assert.equal(await status(env, `chunks/${H4}`), 404);
  assert.equal(await status(env, 'source', 'someone'), 404);
  assert.equal(await status(env, `chunks/${H4}`, 'someone'), 404);
  // The player world is still everyone's on an approved listing.
  assert.equal(await status(env, 'manifest'), 200);
  assert.equal(await status(env, `chunks/${H1}`), 200);
});

test('an eshs_ minted for listing A reads A\'s private source, its server-only chunks and its player world', async () => {
  const env = environment();
  await published(env);
  const eshs = await mintSession(env, SIM);
  const res = await call(env, 'GET', 'source', { who: eshs });
  assert.equal(res.status, 200);
  assert.equal(await res.text(), JSON.stringify(sourceWorld()));
  assert.equal(res.headers.get('Cache-Control'), 'private, no-store');
  const piece = await call(env, 'GET', `chunks/${H4}`, { who: eshs });
  assert.equal(piece.status, 200);
  assert.equal(piece.headers.get('Cache-Control'), 'private, no-store');
  assert.equal(await status(env, 'manifest', eshs), 200);
  assert.equal(await status(env, `chunks/${H1}`, eshs), 200);
});

test('an eshs_ minted for listing B is refused on listing A', async () => {
  const env = environment();
  await published(env);
  const other = await mintSession(env, OTHER);
  assert.equal(await status(env, 'source', other), 404);
  assert.equal(await status(env, `chunks/${H4}`, other), 404);
});

test('an expired, unknown or malformed eshs_ is refused', async () => {
  const env = environment();
  await published(env);
  const expired = await mintSession(env, SIM, { expiresIn: -1 });
  assert.equal(await status(env, 'source', expired), 404, 'expired');
  assert.equal(await status(env, 'source', `eshs_${'0'.repeat(64)}`), 404, 'never minted');
  assert.equal(await status(env, 'source', `eshs_${'Z'.repeat(64)}`), 404, 'not the grammar');
  // No fleet binding at all: no session credential can be checked, so none is accepted.
  const bare = environment();
  await published(bare);
  delete bare.FLEET;
  assert.equal(await status(bare, 'source', `eshs_${'ab'.repeat(32)}`), 404);
});

test('an eshs_ reads the world of its listing while the listing is not yet approved, as the author\'s own server does', async () => {
  const env = environment();
  await published(env);
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...listing(env), moderation: { status: 'pending' } }));
  const eshs = await mintSession(env, SIM);
  assert.equal(await status(env, 'manifest', eshs), 200);
  assert.equal(await status(env, 'source', eshs), 200);
  assert.equal(await status(env, `chunks/${H4}`, eshs), 200);
  assert.equal(await status(env, 'manifest', await mintSession(env, OTHER)), 403, 'another listing\'s credential is nobody here');
  assert.equal(await status(env, 'manifest'), 403);
});

test('a deleted listing, or one under a legal lock, refuses its private source to the session and to the author; an admin reads', async () => {
  // The deletion mark; a legal lock that blocks serving; a legal lock on the content only, under which the author still
  // passes the listing's gate, so the source's own check is what refuses.
  for (const [key, kind] of [[`${DELETED_PREFIX}${SIM}.json`, null], [`locks/legal/${SIM}`, 'serve_block'], [`locks/legal/${SIM}`, 'content_lock']]) {
    const env = environment();
    await published(env);
    const eshs = await mintSession(env, SIM);
    await env.SCENES.put(key, '{}', kind ? { customMetadata: { kind } } : undefined);
    // Refused: by the listing's gate (403, the listing reads as private) or as no source (404); never served.
    const refused = (s) => s === 403 || s === 404;
    assert.ok(refused(await status(env, 'source', eshs)), `${key}: the session`);
    assert.ok(refused(await status(env, `chunks/${H4}`, eshs)), `${key}: the session's chunk`);
    assert.ok(refused(await status(env, 'manifest', eshs)), `${key}: a session downloads nothing`);
    assert.ok(refused(await status(env, 'source', 'author')), `${key}: the author`);
    assert.ok(refused(await status(env, `chunks/${H4}`, 'author')), `${key}: the author's chunk`);
    assert.equal(await status(env, 'source', 'admin'), 200, `${key}: an admin`);
  }
});

test('Share Source on: the source reads publicly, and its chunks cache as before', async () => {
  const env = environment();
  await published(env, { openSource: true });
  const res = await call(env, 'GET', 'source');
  assert.equal(res.status, 200, 'anyone reads a shared source');
  assert.equal(res.headers.get('Cache-Control'), 'private, no-store');
  const piece = await call(env, 'GET', `chunks/${H4}`);
  assert.equal(piece.status, 200);
  assert.equal(piece.headers.get('Cache-Control'), 'private, max-age=86400, immutable');
  // Share Source off again, same world: the source stays, private.
  assert.equal((await call(env, 'POST', 'commit', { who: 'author', body: { manifest_json: JSON.stringify(world()), listing: { open_source: false } } })).status, 200);
  approve(env);
  assert.ok(listing(env).world.source_manifest_key, 'kept as a private source');
  assert.equal(await status(env, 'source'), 404);
  assert.equal(await status(env, `chunks/${H4}`), 404);
  assert.equal(await status(env, 'source', 'author'), 200);
});
