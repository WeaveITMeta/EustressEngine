// A listing's thumbnail address is this Worker's own thumbnail route, or nothing (src/index.js handlePublishSimulation,
// liveThumbnailUrl, galleryView). An address the author named was never judged and can change after an approval, so publish
// never takes one, and a record that still carries one is served with no thumbnail in every response that carries the field.
// The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const OTHER = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const ORIGIN = 'https://api.eustress.dev';
const OWN = `${ORIGIN}/api/simulations/${SIM}/thumbnail`;
const token = mintToken(AUTHOR);

async function environment(thumbnail_url) {
  const env = fakeEnv();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  if (thumbnail_url !== undefined) {
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
      id: SIM, name: 'Box Head', description: '', genre: 'all', tags: [], max_players: 4, author_id: AUTHOR, author_name: 'host', is_public: true,
      moderation: { status: 'approved', rating: 'all_ages' }, thumbnail_url, play_count: 0, favorite_count: 0, version: 1, r2_key: `universes/${SIM}/universe.pak`,
      published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
    }));
    await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  }
  return env;
}

async function call(env, method, path, { auth = false, body } = {}) {
  const headers = { ...(auth ? { Authorization: `Bearer ${token}` } : {}), ...(body ? { 'Content-Type': 'application/json' } : {}) };
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method, headers, body: body ? JSON.stringify(body) : undefined }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

/// The thumbnail address each response that carries one gives for the listing: the listing page, the Gallery list
/// and the author's projects.
async function servedEverywhere(env) {
  const page = await call(env, 'GET', `/api/simulations/${SIM}`);
  const list = await call(env, 'GET', '/api/simulations');
  const projects = await call(env, 'GET', '/api/projects', { auth: true });
  return {
    page: page.body?.thumbnail_url,
    list: list.body?.simulations?.find((s) => s.id === SIM)?.thumbnail_url,
    projects: projects.body?.projects?.find((p) => p.id === SIM)?.thumbnail_url,
  };
}

test('publish never takes a thumbnail address from the request, external or not', async () => {
  for (const sent of ['https://cdn.example.com/cat.png', `${ORIGIN}/api/simulations/${OTHER}/thumbnail`, 'javascript:alert(1)']) {
    const env = await environment();
    const r = await call(env, 'POST', '/api/simulations/publish', { auth: true, body: { name: 'Mine', thumbnail_url: sent } });
    assert.equal(r.status, 201, sent);
    assert.equal(r.body.thumbnail_url, null, `${sent}: not in the answer`);
    assert.equal(JSON.parse(await env.SOCIAL.get(`sim:${r.body.id}`)).thumbnail_url, null, `${sent}: not stored`);
  }
});

test('a legacy record with an external thumbnail address is served with none, in every response that carries one', async () => {
  for (const legacy of [
    'https://cdn.example.com/cat.png',
    `https://cdn.example.com/api/simulations/${SIM}/thumbnail`,
    `${ORIGIN}/api/simulations/${OTHER}/thumbnail`,
    `${OWN}?v=2`,
    `http://api.eustress.dev/api/simulations/${SIM}/thumbnail`,
    `https://user@api.eustress.dev/api/simulations/${SIM}/thumbnail`,
    'not a url',
  ]) {
    const env = await environment(legacy);
    assert.deepEqual(await servedEverywhere(env), { page: null, list: null, projects: null }, legacy);
  }
});

test('the Worker\'s own route is served as it is, and the retired simulations host is rewritten to it (the control)', async () => {
  assert.deepEqual(await servedEverywhere(await environment(OWN)), { page: OWN, list: OWN, projects: OWN });
  const staging = `https://api-staging.eustress.dev/api/simulations/${SIM}/thumbnail`;
  assert.deepEqual(await servedEverywhere(await environment(staging)), { page: staging, list: staging, projects: staging });
  assert.deepEqual(await servedEverywhere(await environment(`https://simulations.eustress.dev/thumbnails/${SIM}/thumb.webp`)), { page: OWN, list: OWN, projects: OWN });
  assert.deepEqual(await servedEverywhere(await environment(null)), { page: null, list: null, projects: null });
});
