// Committing an identical world must not void its approval. The commit used to put the manifest again on every call; R2
// gives each put a new version, and an approval is bound to the version it reviewed (identOfObject), so the listing read
// as pending for everyone but its author. A Studio republish that changed only Players.MaxPlayers did exactly that.
// Checked against the real Worker with fake storage (versions advance on every put, as R2's do).

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { identOfObject } from '../src/moderation.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const SIM = '11111111-2222-3333-4444-555555555555';
const TOKEN = { author: mintToken(AUTHOR), other: mintToken(OTHER) };
const H1 = 'a'.repeat(64);

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

// A world that names its one chunk. `universe` changes the bytes, so the source world is a different object.
const manifest = (universe) => JSON.stringify({
  format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe, start_space: 'Main',
  spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: C1.length, count: 1, blake3: H1 }] }], assets: [],
});

async function send(env, method, path, who, { body, headers = {} } = {}) {
  const h = { ...(who ? { Authorization: `Bearer ${TOKEN[who]}` } : {}), ...headers };
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body }), env, ctx);
  return { status: res.status, text: await res.text() };
}

async function seed() {
  const env = fakeEnv();
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Box Head', description: '', genre: 'all', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, open_source: true, moderation: { status: 'pending', rating: 'all_ages' }, play_count: 0, favorite_count: 0,
    version: 1, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  const up = await send(env, 'PUT', `/api/simulations/${SIM}/world/chunks/${H1}`, 'author', {
    body: C1, headers: { 'Content-Length': String(C1.length), 'Content-Type': 'application/octet-stream' },
  });
  assert.equal(up.status, 201, up.text);
  return env;
}

const commit = (env, universe = 'Box Head', kind) => send(env, 'POST', `/api/simulations/${SIM}/world/commit`, 'author', {
  body: JSON.stringify({ manifest_json: manifest(universe), ...(kind ? { kind } : {}) }),
  headers: { 'Content-Type': 'application/json' },
});
const get = (env, path, who) => send(env, 'GET', path, who);
const stored = async (env) => JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));

// What the review does once it has approved a listing: a binding that names the objects as they are stored now (the
// identity moderation's approval binding uses), then the listing marked approved.
async function approve(env) {
  const sim = await stored(env);
  const binding = {
    v: 1,
    content: identOfObject(await env.SCENES.head(sim.r2_key)),
    thumb: null,
    cover: null,
    source: sim.world?.source_manifest_key ? identOfObject(await env.SCENES.head(sim.world.source_manifest_key)) : null,
    captures: null,
    evidence_epoch: 0,
  };
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify(binding));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { ...sim.moderation, status: 'approved' } }));
}

test('re-committing the same world keeps an approved listing playable for everyone', async () => {
  const env = await seed();
  assert.equal((await commit(env)).status, 200);
  await approve(env);
  assert.equal((await get(env, `/api/simulations/${SIM}/world/manifest`, 'other')).status, 200, 'approved and bound: playable');

  // The author retries the same commit, or Studio republishes after changing only Players.MaxPlayers.
  assert.equal((await commit(env)).status, 200);
  assert.equal((await get(env, `/api/simulations/${SIM}/world/manifest`, 'other')).status, 200, 'the same bytes keep the approval');
  assert.equal((await stored(env)).moderation.status, 'approved', 'and the listing stays approved');
});

test('re-committing the same source world keeps an approved source readable for everyone', async () => {
  const env = await seed();
  assert.equal((await commit(env)).status, 200);
  assert.equal((await commit(env, 'Box Head source', 'source')).status, 200);
  await approve(env);
  assert.equal((await get(env, `/api/simulations/${SIM}/world/source`, 'other')).status, 200, 'approved source: readable');

  assert.equal((await commit(env, 'Box Head source', 'source')).status, 200);
  assert.equal((await get(env, `/api/simulations/${SIM}/world/source`, 'other')).status, 200, 'the same source bytes keep the approval');
});

test('a new manifest, player or source, is identified by its SHA-256, so no later re-put can move its approval', async () => {
  const env = await seed();
  assert.equal((await commit(env)).status, 200);
  assert.equal((await commit(env, 'Box Head source', 'source')).status, 200);
  const sim = await stored(env);
  assert.equal(identOfObject(await env.SCENES.head(sim.world.manifest_key)), `sha256:${sim.world.manifest_sha256}`);
  assert.equal(identOfObject(await env.SCENES.head(sim.world.source_manifest_key)), `sha256:${sim.world.source_manifest_sha256}`);
});
