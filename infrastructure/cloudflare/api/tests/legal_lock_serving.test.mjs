// What serves a listing reads it through the legal lock. The listing record's
// marker is a KV copy that other writers rewrite whole, so a record that says
// "approved" while a lock says otherwise (a clobbered or stale copy) must not
// serve. Checked against the real Worker with fake storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx, testWebp } from './helpers/load_worker.mjs';
import { loadSimulation } from '../src/commerce.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = { author: mintToken(AUTHOR), other: mintToken(OTHER), admin: mintToken(ADMIN) };

// The record says approved: that is the clobbered state the lock exists for.
async function seed(lock) {
  const env = fakeEnv();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'World', description: '', genre: 'all', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, moderation: { status: 'approved', rating: 'all_ages' }, play_count: 0, favorite_count: 0, version: 1,
    r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  await env.SCENES.put(`universes/${SIM}/universe.pak`, 'the world');
  await env.SCENES.put(`thumbnails/${SIM}/thumb.webp`, testWebp());
  if (lock) await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: lock }), { customMetadata: { kind: lock } });
  return env;
}

async function call(env, method, path, who) {
  const headers = who ? { Authorization: `Bearer ${TOKEN[who]}` } : {};
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers }), env, ctx);
  return { status: res.status, text: await res.text().catch(() => '') };
}

const listed = async env => (JSON.parse((await call(env, 'GET', '/api/simulations')).text).simulations || []).some(s => s.id === SIM);

test('control: with no lock, an approved listing serves and lists', async () => {
  const env = await seed(null);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 200);
  assert.equal(await listed(env), true);
});

test('a serve_block lock beats an "approved" marker: nobody but an admin is served, and the listing is not listed', async () => {
  const env = await seed('serve_block');
  for (const who of [undefined, 'other', 'author']) {
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, who)).status, 403, `download as ${who}`);
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, who)).status, 404, `thumbnail as ${who}`);
  }
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`)).status, 404);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, 'other')).status, 404);
  assert.equal(await listed(env), false);
  assert.ok([401, 403].includes((await call(env, 'POST', `/api/simulations/${SIM}/play`)).status));
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'admin')).status, 200, 'an admin still passes');
});

test('a content_lock keeps the public out, and the author still reaches their own listing', async () => {
  const env = await seed('content_lock');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'other')).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'author')).status, 200, 'held, not quarantined: the author is not told anything different');
  assert.equal(await listed(env), false);
});

test('an author sees the same status for a locked listing as for one in review', async () => {
  const env = await seed('serve_block');
  const mine = JSON.parse((await call(env, 'GET', '/api/projects', 'author')).text);
  const row = (mine.projects || mine.simulations || []).find(p => p.id === SIM);
  assert.ok(row, 'the listing is on the author\'s page');
  assert.equal(row.status, 'in_review');
  assert.doesNotMatch(JSON.stringify(row), /quarantin|legal|hold/i);
});

test('the world routes read through the lock too', async () => {
  const env = await seed('serve_block');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`)).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`, 'author')).status, 403);
});

test('commerce loads a locked listing as not listed', async () => {
  const env = await seed('serve_block');
  const sim = await loadSimulation(env, SIM);
  assert.equal(sim.moderation.status, 'quarantined');
});
