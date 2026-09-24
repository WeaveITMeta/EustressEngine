import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  handleWorldRoute, checkWorldManifest, isSafeRecordPath, chunkKey, manifestKey,
  MAX_UPLOAD_CHUNK_BYTES,
} from '../src/world.mjs';
import { isListable, canServe } from '../src/moderation.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); } };
}

/// R2 with the calls world.mjs makes: put (bytes or a stream), head, ranged
/// get, delete, and a paged list. Pages are two objects long so the cursor
/// loop is exercised.
function r2() {
  const m = new Map();
  const bytesOf = async (value) => {
    if (value instanceof Uint8Array) return value;
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (typeof value === 'string') return new TextEncoder().encode(value);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  const reading = (b) => ({
    size: b.length,
    body: new Response(b).body,
    arrayBuffer: async () => b.slice().buffer,
    text: async () => new TextDecoder().decode(b),
  });
  return {
    m,
    puts: 0,
    async put(key, value) { this.puts++; m.set(key, await bytesOf(value)); return { key }; },
    async head(key) { const b = m.get(key); return b ? { key, size: b.length } : null; },
    async get(key, opts) {
      const b = m.get(key);
      if (!b) return null;
      return reading(opts?.range ? b.slice(opts.range.offset, opts.range.offset + opts.range.length) : b);
    },
    async delete(key) { m.delete(key); },
    async list({ prefix = '', cursor, limit = 1000 } = {}) {
      const keys = [...m.keys()].filter((k) => k.startsWith(prefix)).sort();
      const start = cursor ? Number(cursor) : 0;
      const size = Math.min(limit, 2);
      const truncated = start + size < keys.length;
      return {
        objects: keys.slice(start, start + size).map((key) => ({ key, size: m.get(key).length })),
        truncated,
        cursor: truncated ? String(start + size) : undefined,
      };
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
const H1 = 'a'.repeat(64);
const H2 = 'b'.repeat(64);
const H3 = 'c'.repeat(64);

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

function manifest(overrides = {}) {
  const entry = (cx, blake3, size) => ({ cx, cz: 0, file: `${cx}_0.echk`, size, count: 1, blake3 });
  return {
    format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Vehicle Simulator',
    start_space: 'City',
    spaces: [
      { name: 'City', chunks: [entry(0, H1, C1.length)] },
      { name: 'Garage', chunks: [entry(0, H2, C2.length)] },
    ],
    assets: [],
    ...overrides,
  };
}

function environment(sim = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2() };
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Vehicle Simulator', author_id: 'author', is_public: true, version: 1,
    moderation: { status: 'pending' }, ...sim,
  }));
  return env;
}

const listing = (env) => JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
const approve = (env) => env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...listing(env), moderation: { status: 'approved' } }));

async function call(env, method, path, { who, body, bytes, length } = {}) {
  const headers = new Headers();
  if (who) headers.set('Authorization', `Bearer ${who}`);
  let payload;
  if (body !== undefined) {
    payload = JSON.stringify(body);
    headers.set('Content-Type', 'application/json');
  }
  if (bytes !== undefined) {
    payload = bytes;
    headers.set('Content-Length', String(length ?? bytes.length));
  }
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/${path}`);
  const res = await handleWorldRoute(new Request(url, { method, headers, body: payload }), url, env, {}, deps);
  return res;
}

const publish = (m, extra = {}) => ({ manifest_json: JSON.stringify(m), ...extra });

// ── the manifest check ─────────────────────────────────────────────────────

test('record paths follow eustress_echk::is_safe_record_path exactly', () => {
  // Same vectors as the Rust crate's `record_paths_cannot_escape`.
  for (const bad of ['', '/etc/passwd', '../x', 'a/../b', 'a/./b', 'a//b', 'C:/x', 'a\\b', 'Workspace/x:stream', 'a/\u0000b', 'a/\u0085b'])
    assert.equal(isSafeRecordPath(bad), false, `accepted ${JSON.stringify(bad)}`);
  for (const ok of ['Workspace/Floor/_instance.toml', 'assets/meshes/a.glb', '.eustress-free/x'])
    assert.equal(isSafeRecordPath(ok), true, `refused ${JSON.stringify(ok)}`);
  assert.equal(isSafeRecordPath('é'.repeat(513)), false, 'the limit counts UTF-8 bytes, as Rust does');
});

test('a manifest the Player would refuse is refused here', () => {
  const ok = checkWorldManifest(manifest());
  assert.equal(ok.error, undefined);
  assert.deepEqual(ok.spaces, ['City', 'Garage']);
  assert.equal(ok.start_space, 'City');
  assert.equal(ok.bytes, C1.length + C2.length);

  const bad = [
    manifest({ format: 'pak' }),
    manifest({ encoder_version: 3 }),
    manifest({ chunk_size: 0 }),
    manifest({ spaces: [] }),
    manifest({ start_space: 'Nowhere' }),
    manifest({ spaces: [{ name: '../escape', chunks: [] }] }),
    manifest({ spaces: [{ name: 'a/b', chunks: [] }] }),
    manifest({ spaces: [{ name: '.hidden', chunks: [] }] }),
    manifest({ spaces: [{ name: 'Twice', chunks: [] }, { name: 'Twice', chunks: [] }], start_space: '' }),
    manifest({ assets: [{ cx: 0, cz: 0, file: 'x', size: 10, count: 1, blake3: 'NOTAHASH' }] }),
    manifest({ assets: [{ cx: 0, cz: 0, file: 'x', size: 0, count: 1, blake3: H3 }] }),
    manifest({ assets: [{ cx: 0.5, cz: 0, file: 'x', size: 10, count: 1, blake3: H3 }] }),
    manifest({ assets: [{ cx: 0, cz: 0, file: 'x', size: C1.length + 1, count: 1, blake3: H1 }] }),
    manifest({ assets: Array.from({ length: 17 }, (_, i) => ({ cx: i, cz: 0, file: 'x', size: 512 * 1024 * 1024, count: 1, blake3: i.toString(16).padStart(64, '0') })) }),
  ];
  for (const m of bad) assert.ok(checkWorldManifest(m).error, `accepted ${JSON.stringify(m).slice(0, 160)}`);
});

// ── publishing ─────────────────────────────────────────────────────────────

test('a publish uploads only missing chunks, then commits', async () => {
  const env = environment();
  const m = manifest();

  let res = await call(env, 'POST', 'begin', { who: 'author', body: publish(m) });
  assert.equal(res.status, 200);
  let out = await res.json();
  assert.deepEqual(out.missing.sort(), [H1, H2]);
  assert.equal(out.chunks, 2);

  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: C1 })).status, 201);
  out = await (await call(env, 'POST', 'begin', { who: 'author', body: publish(m) })).json();
  assert.deepEqual(out.missing, [H2], 'a stored chunk is not asked for again');

  // Committing before the last chunk lands names what is missing.
  res = await call(env, 'POST', 'commit', { who: 'author', body: publish(m) });
  assert.equal(res.status, 409);
  assert.deepEqual((await res.json()).missing, [H2]);

  assert.equal((await call(env, 'PUT', `chunks/${H2}`, { who: 'author', bytes: C2 })).status, 201);
  const text = JSON.stringify(m);
  res = await call(env, 'POST', 'commit', {
    who: 'author',
    body: { manifest_json: text, publish_hash: H3, listing: { name: 'Vehicle Simulator', description: 'Drive.' } },
  });
  assert.equal(res.status, 200);
  out = await res.json();
  const sha = createHash('sha256').update(text).digest('hex');
  assert.equal(out.manifest_sha256, sha, 'the key hashes the exact bytes the engine sent');

  const sim = listing(env);
  assert.equal(sim.format, 'echk');
  assert.equal(sim.r2_key, manifestKey(SIM, sha), 'moderation submit heads this key');
  assert.equal(sim.pak_etag, null);
  assert.equal(sim.scene_size_bytes, C1.length + C2.length);
  assert.equal(sim.content_root, `blake3:${H3}`);
  assert.deepEqual(sim.space_names, ['City', 'Garage']);
  assert.equal(sim.description, 'Drive.');
  assert.equal(sim.moderation.status, 'pending');
  assert.equal(new TextDecoder().decode(env.SCENES.m.get(sim.r2_key)), text);
});

test('only the author publishes, and not under a legal hold or a freeze', async () => {
  const m = manifest();
  let env = environment();
  assert.equal((await call(env, 'POST', 'begin', { body: publish(m) })).status, 401);
  assert.equal((await call(env, 'POST', 'begin', { who: 'someone', body: publish(m) })).status, 403);
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'someone', bytes: C1 })).status, 403);
  assert.equal(env.SCENES.m.size, 0);

  env = environment({ moderation: { status: 'quarantined' } });
  assert.equal((await call(env, 'POST', 'commit', { who: 'author', body: publish(m) })).status, 409);

  env = environment();
  env.USERS.m.set('publish-frozen:author', '1');
  assert.equal((await call(env, 'POST', 'begin', { who: 'author', body: publish(m) })).status, 403);
});

test('an upload must be an .echk chunk under a content-hash name', async () => {
  const env = environment();
  assert.equal((await call(env, 'PUT', 'chunks/not-a-hash', { who: 'author', bytes: C1 })).status, 400);
  const garbage = new Uint8Array(64).fill(7);
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: garbage })).status, 422);
  assert.equal(env.SCENES.m.has(chunkKey(SIM, H1)), false, 'a refused upload is not left behind');
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: C1, length: MAX_UPLOAD_CHUNK_BYTES + 1 })).status, 413);
});

test('both container versions are accepted and stored exactly as sent', async () => {
  const env = environment();
  // A version 2 header (compressed body); the Worker never inflates it.
  const v2 = C1.slice();
  new DataView(v2.buffer).setUint32(4, 2, true);
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: v2 })).status, 201);
  assert.deepEqual(env.SCENES.m.get(chunkKey(SIM, H1)), v2);

  const v3 = C1.slice();
  new DataView(v3.buffer).setUint32(4, 3, true);
  assert.equal((await call(env, 'PUT', `chunks/${H2}`, { who: 'author', bytes: v3 })).status, 422);
  assert.equal(env.SCENES.m.has(chunkKey(SIM, H2)), false);
});

test('a manifest naming a chunk over the upload limit is refused at begin', async () => {
  const env = environment();
  const big = manifest({ assets: [{ cx: 0, cz: 0, file: 'assets_0.echk', size: MAX_UPLOAD_CHUNK_BYTES + 1, count: 1, blake3: H3 }] });
  const res = await call(env, 'POST', 'begin', { who: 'author', body: publish(big) });
  assert.equal(res.status, 413);
  assert.equal((await res.json()).code, 'chunk_too_large');
});

// ── after a commit ─────────────────────────────────────────────────────────

async function committed(env, m = manifest()) {
  await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: C1 });
  await call(env, 'PUT', `chunks/${H2}`, { who: 'author', bytes: C2 });
  const res = await call(env, 'POST', 'commit', { who: 'author', body: publish(m) });
  assert.equal(res.status, 200);
  return m;
}

test('the manifest and its chunks follow the download gate', async () => {
  const env = environment();
  const m = await committed(env);

  // Pending: the author and an admin, nobody else.
  assert.equal((await call(env, 'GET', 'manifest')).status, 403);
  let res = await call(env, 'GET', 'manifest', { who: 'author' });
  assert.equal(res.status, 200);
  assert.equal(await res.text(), JSON.stringify(m), 'served byte for byte');
  assert.equal((await call(env, 'GET', 'manifest', { who: 'admin' })).status, 200);

  approve(env);
  assert.equal((await call(env, 'GET', 'manifest')).status, 200);
  res = await call(env, 'GET', `chunks/${H1}`);
  assert.equal(res.status, 200);
  assert.deepEqual(new Uint8Array(await res.arrayBuffer()), C1);
  assert.equal(res.headers.get('ETag'), `"${H1}"`);

  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...listing(env), moderation: { status: 'quarantined' } }));
  assert.equal((await call(env, 'GET', `chunks/${H1}`)).status, 451);
  assert.equal((await call(env, 'GET', `chunks/${H1}`, { who: 'author' })).status, 451);
});

test('only chunks the committed manifest names are served, and they cannot be replaced', async () => {
  const env = environment();
  await committed(env);
  approve(env);

  // Uploaded after the commit and never committed: stored, not served.
  assert.equal((await call(env, 'PUT', `chunks/${H3}`, { who: 'author', bytes: C1 })).status, 201);
  assert.equal((await call(env, 'GET', `chunks/${H3}`)).status, 404);

  // Same bytes again: nothing written.
  const before = env.SCENES.puts;
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: C1 })).status, 200);
  assert.equal(env.SCENES.puts, before);
  // Different bytes under a committed name: refused.
  assert.equal((await call(env, 'PUT', `chunks/${H1}`, { who: 'author', bytes: C2 })).status, 409);
  assert.deepEqual(env.SCENES.m.get(chunkKey(SIM, H1)), C1);
});

test('committing the same world twice keeps its review; new content goes back to pending', async () => {
  const env = environment();
  const m = await committed(env);
  approve(env);

  await call(env, 'POST', 'commit', { who: 'author', body: publish(m) });
  assert.equal(listing(env).moderation.status, 'approved', 'a retried commit is not a new publish');
  assert.equal(listing(env).version, 1);

  const moved = manifest({ start_space: 'Garage' });
  await call(env, 'POST', 'commit', { who: 'author', body: publish(moved) });
  assert.equal(listing(env).moderation.status, 'pending');
  assert.equal(listing(env).version, 2);
});

test('a listing published as a .pak answers 409 so the Player falls back', async () => {
  const env = environment({ r2_key: `universes/${SIM}/universe.pak`, moderation: { status: 'approved' } });
  const res = await call(env, 'GET', 'manifest');
  assert.equal(res.status, 409);
  const out = await res.json();
  assert.equal(out.format, 'pak');
  assert.equal(out.download, `/api/simulations/${SIM}/download`);
  assert.equal((await call(env, 'GET', `chunks/${H1}`)).status, 404);
});

test('paths outside /world/ are left to the other routes', async () => {
  const env = environment();
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/download`);
  assert.equal(await handleWorldRoute(new Request(url), url, env, {}, deps), null);
});
