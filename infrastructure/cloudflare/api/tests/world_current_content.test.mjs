// A listing has one current content: its world or its .pak. A .pak published over a world (or a world committed over a .pak)
// moves what the approval binds, and the world routes must send exactly that object. Before this, a world that was never
// reviewed was served under the approval of a .pak published over it (Security's finding, 2026-10-08). Checked against the real
// Worker with fake storage; the .pak uploads go through the routes a Studio uses.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { identOfObject } from '../src/moderation.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const SIM = '11111111-2222-3333-4444-555555555555';
const TOKEN = { author: mintToken(AUTHOR), other: mintToken(OTHER) };
const H1 = 'a'.repeat(64);
const PAK_KEY = `universes/${SIM}/universe.pak`;
const PAK_BYTES = new TextEncoder().encode(`EUSTRESS pak ${'x'.repeat(256)}`);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

function chunk() {
  const path = new TextEncoder().encode('Workspace/P/_instance.toml');
  const data = new TextEncoder().encode('[metadata]\nclass_name = "Part"\n');
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
const C1 = chunk();
const manifestText = universe => JSON.stringify({
  format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe, start_space: 'Main',
  spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: C1.length, count: 1, blake3: H1 }] }], assets: [],
});

async function send(env, method, path, who, { body, headers = {} } = {}) {
  const h = { ...(who ? { Authorization: `Bearer ${TOKEN[who]}` } : {}), ...headers };
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  return { status: res.status, text: new TextDecoder().decode(bytes), bytes };
}

async function seed() {
  const env = fakeEnv();
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Box Head', description: '', genre: 'all', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, open_source: true, moderation: { status: 'pending', rating: 'all_ages' }, play_count: 0, favorite_count: 0,
    version: 1, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  return env;
}

const stored = async env => JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));

// The world: its one chunk, then the commit. Returns the manifest text it committed.
async function commitWorld(env, universe = 'Box Head') {
  const up = await send(env, 'PUT', `/api/simulations/${SIM}/world/chunks/${H1}`, 'author', {
    body: C1, headers: { 'Content-Length': String(C1.length), 'Content-Type': 'application/octet-stream' },
  });
  assert.ok([200, 201].includes(up.status), up.text);
  const text = manifestText(universe);
  const res = await send(env, 'POST', `/api/simulations/${SIM}/world/commit`, 'author', {
    body: JSON.stringify({ manifest_json: text }), headers: { 'Content-Type': 'application/json' },
  });
  assert.equal(res.status, 200, res.text);
  return text;
}

async function commitSource(env) {
  const res = await send(env, 'POST', `/api/simulations/${SIM}/world/commit`, 'author', {
    body: JSON.stringify({ manifest_json: manifestText('Box Head source'), kind: 'source' }), headers: { 'Content-Type': 'application/json' },
  });
  assert.equal(res.status, 200, res.text);
}

// The .pak, uploaded as a whole file through the route a Studio uses.
const uploadPak = env => send(env, 'PUT', `/api/simulations/${SIM}/space`, 'author', {
  body: PAK_BYTES, headers: { 'Content-Length': String(PAK_BYTES.length) },
});

// The approval a review writes: a binding that names the listing's current objects, and the listing marked approved.
async function approve(env) {
  const sim = await stored(env);
  const binding = {
    v: 1,
    content: identOfObject(await env.SCENES.head(sim.r2_key)),
    thumb: null, cover: null,
    source: sim.world?.source_manifest_key ? identOfObject(await env.SCENES.head(sim.world.source_manifest_key)) : null,
    captures: null, evidence_epoch: 0,
  };
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify(binding));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { ...sim.moderation, status: 'approved' } }));
}

const manifestUrl = `/api/simulations/${SIM}/world/manifest`;
const sourceUrl = `/api/simulations/${SIM}/world/source`;
const chunkUrl = `/api/simulations/${SIM}/world/chunks/${H1}`;
const downloadUrl = `/api/simulations/${SIM}/download`;

test('a world committed and never approved, then a .pak published over it and approved: the world is not served, the .pak is', async () => {
  const env = await seed();
  await commitWorld(env);
  assert.equal((await uploadPak(env)).status, 200);
  await approve(env);
  const manifest = await send(env, 'GET', manifestUrl, 'other');
  assert.equal(manifest.status, 409, 'the world is not the current content');
  assert.equal((await send(env, 'GET', chunkUrl, 'other')).status, 404, 'its chunks are not served either');
  const pak = await send(env, 'GET', downloadUrl, 'other');
  assert.equal(pak.status, 200, pak.text);
  assert.deepEqual(pak.bytes, PAK_BYTES, 'the reviewed .pak is what downloads');
  assert.equal((await stored(env)).format, 'pak');
});

test('a .pak approved, then a world committed and approved: the .pak stops serving and the world is served', async () => {
  const env = await seed();
  await uploadPak(env);
  await approve(env);
  assert.equal((await send(env, 'GET', downloadUrl, 'other')).status, 200);
  const manifest = await commitWorld(env);
  assert.equal((await send(env, 'GET', downloadUrl, 'other')).status, 403, 'a listing with a new world is pending: its .pak is not served to others');
  await approve(env);
  assert.equal((await send(env, 'GET', downloadUrl, 'other')).status, 409, 'the world is the current content: the .pak is refused');
  const served = await send(env, 'GET', manifestUrl, 'other');
  assert.equal(served.status, 200, served.text);
  assert.equal(served.text, manifest, 'the approved world is served byte for byte');
});

test('a listing whose format still says echk while its current object is a .pak serves no world, though the .pak is approved', async () => {
  // The record the old upload path left behind: the world and its source remain, and r2_key moved to the .pak.
  const env = await seed();
  await commitWorld(env);
  await commitSource(env);
  await env.SCENES.put(PAK_KEY, PAK_BYTES, { sha256: sha(PAK_BYTES) });
  const sim = await stored(env);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, r2_key: PAK_KEY }));
  await approve(env);
  assert.equal((await send(env, 'GET', manifestUrl, 'other')).status, 409, 'the belt: the manifest is not the current object');
  assert.equal((await send(env, 'GET', chunkUrl, 'other')).status, 404, 'the belt: no chunk of the world');
  assert.equal((await send(env, 'GET', sourceUrl, 'other')).status, 404, 'the belt: no source of the world');
  assert.equal((await send(env, 'GET', downloadUrl, 'other')).status, 409, 'the format says echk: the .pak is refused too');
});

test('a source cannot be committed onto a listing whose current content is a .pak', async () => {
  const env = await seed();
  await commitWorld(env);
  await uploadPak(env);
  const res = await send(env, 'POST', `/api/simulations/${SIM}/world/commit`, 'author', {
    body: JSON.stringify({ manifest_json: manifestText('Box Head source'), kind: 'source' }), headers: { 'Content-Type': 'application/json' },
  });
  assert.equal(res.status, 409);
  assert.equal(JSON.parse(res.text).code, 'no_world');
});

test('a .pak uploaded in parts is the current content, as a whole-file upload is', async () => {
  const env = await seed();
  await commitWorld(env);
  const created = await send(env, 'POST', `/api/simulations/${SIM}/space/multipart/create`, 'author');
  assert.equal(created.status, 200, created.text);
  const uploadId = JSON.parse(created.text).upload_id;
  const part = await send(env, 'PUT', `/api/simulations/${SIM}/space/multipart/part?upload_id=${encodeURIComponent(uploadId)}&part_number=1`, 'author', {
    body: PAK_BYTES, headers: { 'Content-Length': String(PAK_BYTES.length) },
  });
  assert.equal(part.status, 200, part.text);
  const done = await send(env, 'POST', `/api/simulations/${SIM}/space/multipart/complete`, 'author', {
    body: JSON.stringify({ upload_id: uploadId, parts: [{ part_number: 1, etag: JSON.parse(part.text).etag }], total_size: PAK_BYTES.length }),
    headers: { 'Content-Type': 'application/json' },
  });
  assert.equal(done.status, 200, done.text);
  const sim = await stored(env);
  assert.deepEqual([sim.format, sim.r2_key], ['pak', PAK_KEY]);
  await approve(env);
  assert.equal((await send(env, 'GET', manifestUrl, 'other')).status, 409, 'the world is not served after a multipart .pak');
  assert.equal((await send(env, 'GET', downloadUrl, 'other')).status, 200);
});
