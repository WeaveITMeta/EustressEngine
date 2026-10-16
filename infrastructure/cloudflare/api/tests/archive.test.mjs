// Archive: an author unlists their own listing and relists it. The Gallery stops serving an archived listing; its review, its
// world and its counts are untouched; Unarchive puts back exactly what was there. Only the author, never while a hold is on it.
// The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const tokens = { author: mintToken(AUTHOR), stranger: mintToken(STRANGER) };

// Rated all_ages: a listing with no rating is adult content, which a viewer sees only with Adult content on (src/adult_content.mjs).
async function environment({ moderation = { status: 'approved', rating: 'all_ages' }, is_public = true, extra = {} } = {}) {
  const env = fakeEnv();
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Box Head', author_id: AUTHOR, author_name: 'host', is_public, moderation, play_count: 7, version: 3, max_players: 4, ...extra }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  return env;
}

async function call(env, method, path, who) {
  const headers = {};
  if (who) headers.Authorization = `Bearer ${tokens[who]}`;
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers }), env, ctx);
  const text = await res.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body, text };
}

const record = async (env) => JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
const archive = (env, who = 'author') => call(env, 'POST', `/api/simulations/${SIM}/archive`, who);
const unarchive = (env, who = 'author') => call(env, 'POST', `/api/simulations/${SIM}/unarchive`, who);
const inGallery = async (env) => ((await call(env, 'GET', '/api/simulations')).body?.simulations || []).some((s) => s.id === SIM);

test('archiving unlists the listing from the Gallery and from a stranger, leaving its review, world and counts as they were', async () => {
  const env = await environment();
  assert.equal(await inGallery(env), true);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, 'stranger')).status, 200);

  const r = await archive(env);
  assert.equal(r.status, 200, r.text);
  assert.deepEqual(r.body, { ok: true, archived: true, changed: true });
  assert.equal(await inGallery(env), false, 'the Gallery no longer serves it');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, 'stranger')).status, 404, 'and a stranger cannot open it');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`)).status, 404);

  const sim = await record(env);
  assert.equal(sim.moderation.status, 'approved', 'the review is untouched');
  assert.equal(sim.play_count, 7);
  assert.equal(sim.version, 3);
  assert.equal(sim.max_players, 4);
  assert.equal(sim.is_public, false);
  assert.equal(sim.archived, true);
  assert.equal(sim.archived_was_public, true);

  // The author still sees it, and their list says archived.
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, 'author')).status, 200);
  const mine = await call(env, 'GET', '/api/projects', 'author');
  assert.equal(mine.body.projects.find((p) => p.id === SIM).status, 'archived');
});

test('unarchiving puts back exactly what was there, and a listing that was private stays private', async () => {
  const env = await environment();
  await archive(env);
  const back = await unarchive(env);
  assert.deepEqual(back.body, { ok: true, archived: false, changed: true });
  assert.equal(await inGallery(env), true);
  const sim = await record(env);
  assert.equal(sim.is_public, true);
  assert.ok(!('archived' in sim) && !('archived_was_public' in sim) && !('archived_at' in sim), 'no residue');
  assert.equal(sim.moderation.status, 'approved');

  const priv = await environment({ is_public: false });
  await archive(priv);
  await unarchive(priv);
  assert.equal((await record(priv)).is_public, false, 'it was private before, and is private after');
  assert.equal(await inGallery(priv), false);
});

test('each answer is idempotent: a second archive or unarchive changes nothing and says so', async () => {
  const env = await environment();
  assert.equal((await unarchive(env)).body.changed, false, 'nothing to restore');
  assert.equal((await archive(env)).body.changed, true);
  const before = await env.SOCIAL.get(`sim:${SIM}`);
  assert.deepEqual((await archive(env)).body, { ok: true, archived: true, changed: false });
  assert.equal(await env.SOCIAL.get(`sim:${SIM}`), before, 'a second archive writes nothing');
  assert.equal((await unarchive(env)).body.changed, true);
});

test('only the author may, and a listing that is not theirs answers as one that does not exist', async () => {
  const env = await environment();
  assert.equal((await archive(env, null)).status, 401);
  assert.equal((await unarchive(env, null)).status, 401);
  const stranger = await archive(env, 'stranger');
  const missing = await call(env, 'POST', '/api/simulations/bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb/archive', 'stranger');
  assert.equal(stranger.status, 404);
  assert.deepEqual(stranger.body, missing.body, 'not told apart');
  assert.equal((await record(env)).archived, undefined, 'nothing changed');
  await archive(env);
  assert.equal((await unarchive(env, 'stranger')).status, 404);
  assert.equal((await record(env)).archived, true, 'a stranger cannot relist it either');
});

test('a listing under a hold cannot be changed, and the refusal reads as ordinary review', async () => {
  const env = await environment({ moderation: { status: 'quarantined' } });
  const r = await archive(env);
  assert.equal(r.status, 409, r.text);
  assert.equal(r.body.code, 'in_review');
  assert.equal((await record(env)).archived, undefined);
  // A publish freeze from before 2026-10-15 is not read: no moderation outcome restricts the account (McKale).
  const frozen = await environment();
  await frozen.USERS.put(`publish-frozen:${AUTHOR}`, '1');
  const frozenAnswer = await archive(frozen);
  assert.equal(frozenAnswer.status, 200, 'the author still archives their own listing');
  assert.equal((await record(frozen)).archived, true);
});

test('a publish while archived cannot make the listing public again; Unarchive restores what the publish chose', async () => {
  const { handleWorldRoute, manifestKey } = await import('../src/world.mjs');
  const { isListable, canServe } = await import('../src/moderation.mjs');
  const kv = () => { const m = new Map(); return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); } }; };
  const bucket = () => {
    const m = new Map();
    const bytes = async (v) => (v instanceof Uint8Array ? v : typeof v === 'string' ? new TextEncoder().encode(v) : new Uint8Array(await new Response(v).arrayBuffer()));
    return { m, async put(k, v) { m.set(k, await bytes(v)); }, async head(k) { const b = m.get(k); return b ? { size: b.length } : null; }, async get(k) { const b = m.get(k); return b ? { size: b.length, body: new Response(b).body, text: async () => new TextDecoder().decode(b) } : null; }, async delete(k) { m.delete(k); }, async list({ prefix = '' } = {}) { return { objects: [...m.keys()].filter((x) => x.startsWith(prefix)).map((key) => ({ key, size: m.get(key).length })), truncated: false }; } };
  };
  const H1 = 'a'.repeat(64);
  const path = new TextEncoder().encode('Workspace/P/_instance.toml');
  const data = new TextEncoder().encode('[metadata]\nclass_name = "Part"\n');
  const chunk = new Uint8Array(20 + path.length + data.length);
  const view = new DataView(chunk.buffer);
  chunk.set([0x45, 0x43, 0x48, 0x4b]); view.setUint32(4, 1, true); view.setUint32(8, 1, true); view.setUint32(12, path.length, true); chunk.set(path, 16); view.setUint32(16 + path.length, data.length, true); chunk.set(data, 20 + path.length);
  const manifest = { format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Box Head', start_space: 'Main', spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: chunk.length, count: 1, blake3: H1 }] }], assets: [] };
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: bucket() };
  await env.SCENES.put(`universes/${SIM}/chunks/${H1}.echk`, chunk);
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Box Head', author_id: 'author', is_public: true, version: 1, moderation: { status: 'pending' } }));
  const deps = { verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace(/^Bearer /, '') || null, requireAdmin: async () => null, json: (v, st = 200, h = {}) => new Response(JSON.stringify(v), { status: st, headers: { ...h, 'Content-Type': 'application/json' } }), isListable, canServe, readSpaceNames: (l) => (Array.isArray(l) ? l : null), MODERATION_VERSION: '1.0', POLICY_VERSION: '1.2' };
  const commit = async (listing) => {
    const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/commit`);
    return handleWorldRoute(new Request(url, { method: 'POST', headers: { Authorization: 'Bearer author', 'Content-Type': 'application/json' }, body: JSON.stringify({ manifest_json: JSON.stringify(manifest), listing }) }), url, env, {}, deps);
  };
  const stored = () => JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  // Archived by hand the way the route leaves a record; then the author publishes with the public box ticked.
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...stored(), archived: true, archived_was_public: false, is_public: false }));
  assert.equal((await commit({ name: 'Box Head', is_public: true })).status, 200);
  assert.equal(stored().is_public, false, 'the publish cannot relist an archived listing');
  assert.equal(stored().archived, true);
  assert.equal(stored().archived_was_public, true, 'but its choice is what Unarchive restores');
});

test('a review decision on an archived listing does not make it public: the Gallery asks is_public and approval', async () => {
  const env = await environment({ moderation: { status: 'pending' } });
  await archive(env);
  const sim = await record(env);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { status: 'approved', rating: 'all_ages' } }));
  assert.equal(await inGallery(env), false, 'approved, but archived');
  await unarchive(env);
  assert.equal(await inGallery(env), true);
});
