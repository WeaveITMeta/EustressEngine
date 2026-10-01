// Who may do what to a published simulation, checked against the real Worker
// with fake in-memory storage. Every mutating route must refuse an anonymous
// caller (401) and another account (403 or 404), and every read of a listing
// that is not approved yet must refuse both. A route that moved without its
// ownership check shows up here as a row that fails.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const OTHER_SIM = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const HASH = 'c'.repeat(64);
const TOKENS = { author: mintToken(AUTHOR), stranger: mintToken(STRANGER) };

/// A pending listing by AUTHOR, plus another author's private world in the
/// same bucket, which no route of SIM's may ever hand to anyone.
async function seed() {
  const env = fakeEnv();
  for (const [id, name] of [[AUTHOR, 'author'], [STRANGER, 'stranger']]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: name, created_at: '2026-01-01T00:00:00.000Z' }));
  }
  const sim = (id, authorId, extra = {}) => ({
    id, name: `World ${id.slice(0, 4)}`, description: '', genre: 'all', max_players: 10,
    author_id: authorId, author_name: 'x', is_public: true,
    moderation: { status: 'pending' }, play_count: 0, favorite_count: 0, version: 1,
    published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z', ...extra,
  });
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim(SIM, AUTHOR)));
  await env.SOCIAL.put(`sim:${OTHER_SIM}`, JSON.stringify(sim(OTHER_SIM, STRANGER, { r2_key: `universes/${OTHER_SIM}/universe.pak` })));
  await env.SCENES.put(`universes/${OTHER_SIM}/universe.pak`, 'the other author\'s private world');
  return env;
}

async function call(env, method, path, who, { body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (who) h.Authorization = `Bearer ${TOKENS[who]}`;
  const init = { method, headers: h };
  if (body !== undefined) {
    init.body = body;
    if (typeof body === 'string') h['Content-Type'] = 'application/json';
  }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, init), env, ctx);
  const text = await res.text().catch(() => '');
  return { status: res.status, text };
}

const json = (v) => JSON.stringify(v);
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0]);

// Mutating routes: the author may use them, nobody else may.
const MUTATIONS = [
  ['PUT',  `/api/simulations/${SIM}/space`,                       () => new Uint8Array([1, 2, 3])],
  ['POST', `/api/simulations/${SIM}/space/multipart/create`,      () => json({})],
  ['PUT',  `/api/simulations/${SIM}/space/multipart/part?upload_id=upload-x&part_number=1`, () => new Uint8Array([1])],
  ['POST', `/api/simulations/${SIM}/space/multipart/complete`,    () => json({ upload_id: 'upload-x', parts: [{ part_number: 1, etag: 'part-1' }], total_size: 3 })],
  ['PUT',  `/api/simulations/${SIM}/spaces/Main`,                 () => new Uint8Array([1, 2, 3])],
  ['PUT',  `/api/simulations/${SIM}/thumbnail`,                   () => PNG, { 'Content-Type': 'image/png' }],
  ['PUT',  `/api/simulations/${SIM}/website-manifest`,            () => json({ schema: 1 })],
  ['POST', `/api/simulations/${SIM}/world/begin`,                 () => json({})],
  ['PUT',  `/api/simulations/${SIM}/world/chunks/${HASH}`,        () => new Uint8Array([9, 9])],
  ['POST', `/api/simulations/${SIM}/world/commit`,                () => json({})],
  ['PUT',  `/api/simulations/${SIM}/dossier`,                     () => json({ digest: {} })],
  ['PUT',  `/api/simulations/${SIM}/captures/0`,                  () => PNG, { 'Content-Type': 'image/png' }],
  ['POST', `/api/simulations/${SIM}/submit`,                      () => json({})],
  ['POST', `/api/simulations/${SIM}/appeal`,                      () => json({ text: 'Please look again, the world is a harmless pong game.' })],
];

for (const [method, path, makeBody, headers] of MUTATIONS) {
  test(`${method} ${path.replace(SIM, '{id}').replace(HASH, '{hash}')}: refuses anonymous callers and other accounts`, async () => {
    const env = await seed();
    const anon = await call(env, method, path, null, { body: makeBody(), headers });
    assert.equal(anon.status, 401, `anonymous got ${anon.status}: ${anon.text.slice(0, 120)}`);
    const stranger = await call(env, method, path, 'stranger', { body: makeBody(), headers });
    assert.ok([403, 404].includes(stranger.status), `another account got ${stranger.status}: ${stranger.text.slice(0, 120)}`);

    // The refusal changed nothing the author owns.
    assert.equal(await env.SCENES.get(`universes/${SIM}/universe.pak`), null);
    assert.equal(await env.SCENES.get(`universes/${SIM}/spaces/Main.pak`), null);
    assert.equal(await env.SCENES.get(`thumbnails/${SIM}/thumb.png`), null);

    // The author is not refused by authorization. Other outcomes (a 400 for an
    // empty body, a 409 for a step out of order) are the handlers' own.
    const author = await call(env, method, path, 'author', { body: makeBody(), headers });
    assert.ok(![401, 403].includes(author.status), `the author got ${author.status}: ${author.text.slice(0, 120)}`);
    assert.ok(author.status < 500, `the author got ${author.status}: ${author.text.slice(0, 120)}`);
  });
}

// Reads of a listing that review has not approved: the author's alone.
const READS = [
  `/api/simulations/${SIM}`,
  `/api/simulations/${SIM}/download`,
  `/api/simulations/${SIM}/world/manifest`,
  `/api/simulations/${SIM}/world/source`,
  `/api/simulations/${SIM}/world/chunks/${HASH}`,
  `/api/simulations/${SIM}/moderation`,
];

for (const path of READS) {
  test(`GET ${path.replace(SIM, '{id}').replace(HASH, '{hash}')}: an unapproved listing is not readable by anyone but its author`, async () => {
    const env = await seed();
    for (const who of [null, 'stranger']) {
      const res = await call(env, 'GET', path, who);
      assert.ok([401, 403, 404, 451].includes(res.status), `${who || 'anonymous'} got ${res.status}: ${res.text.slice(0, 120)}`);
    }
  });
}

test('an unapproved listing\'s host link is never shown to anyone but its author', async () => {
  const env = await seed();
  const res = await call(env, 'GET', `/api/simulations/${SIM}/live`, 'stranger');
  assert.ok(!res.text.includes('eustress-player://'), res.text.slice(0, 120));
});

test('the admin queue refuses anonymous callers and ordinary accounts', async () => {
  const env = await seed();
  for (const path of ['/api/admin/moderation/queue?status=held', `/api/admin/moderation/case/${SIM}`]) {
    const anon = await call(env, 'GET', path, null);
    assert.ok([401, 403].includes(anon.status), `anonymous got ${anon.status} on ${path}`);
    const stranger = await call(env, 'GET', path, 'stranger');
    assert.ok([401, 403].includes(stranger.status), `an ordinary account got ${stranger.status} on ${path}`);
  }
  const tool = await call(env, 'POST', '/api/admin/moderation/tool', 'stranger', { body: json({ name: 'moderation_approve', args: { sim_id: SIM, rating: 'all_ages', rationale: 'a stranger tries to approve a listing' } }) });
  assert.ok([401, 403].includes(tool.status), `an ordinary account got ${tool.status} on the tool route`);
  const after = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
  assert.equal(after.moderation.status, 'pending', 'the listing is still pending');
});

// What a listing may point at. A new listing takes its stored object from the
// upload routes, which derive the key from the listing's own id, and never from
// the request that creates it.
test('a new listing cannot name the object it serves: the key comes from the upload routes', async () => {
  const env = await seed();
  const foreign = `universes/${OTHER_SIM}/universe.pak`;
  const created = await call(env, 'POST', '/api/simulations/publish', 'author', {
    body: json({ name: 'Mine', r2_key: foreign, thumbnail_url: 'javascript:alert(1)' }),
  });
  assert.equal(created.status, 201, created.text.slice(0, 160));
  const sim = JSON.parse(created.text);
  assert.equal(sim.r2_key, null, 'the key sent with the request is ignored');
  assert.equal(sim.thumbnail_url, null, 'a thumbnail address that is not https is dropped');

  const download = await call(env, 'GET', `/api/simulations/${sim.id}/download`, 'author');
  assert.ok(!download.text.includes('private world'), 'the other author\'s object is not served');
});

test('a listing record that already names another listing\'s object still does not serve it', async () => {
  const env = await seed();
  const raw = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
  raw.r2_key = `universes/${OTHER_SIM}/universe.pak`;
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(raw));
  const download = await call(env, 'GET', `/api/simulations/${SIM}/download`, 'author');
  assert.ok(!download.text.includes('private world'), 'the other author\'s object is not served');
  assert.notEqual(download.status, 200);
});
