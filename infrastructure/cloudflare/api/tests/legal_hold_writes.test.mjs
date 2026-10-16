// A listing under a legal hold: its stored content is evidence and its author
// is frozen. Checked against the real Worker with fake storage. No route that
// writes content may overwrite what is stored, and none may say why: the
// refusal reads as ordinary review (see authorView in moderation.mjs).

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx, testPng } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = mintToken(AUTHOR);
const EVIDENCE = 'what the author published';
const PNG = testPng();

async function seed(moderation) {
  const env = fakeEnv();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author', created_at: '2026-01-01T00:00:00.000Z' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'World', description: '', genre: 'all', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, moderation, play_count: 0, favorite_count: 0, version: 1,
    r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  }));
  await env.SCENES.put(`universes/${SIM}/universe.pak`, EVIDENCE);
  await env.SCENES.put(`universes/${SIM}/spaces/Main.pak`, EVIDENCE);
  await env.SCENES.put(`thumbnails/${SIM}/thumb.png`, EVIDENCE);
  return env;
}

async function call(env, method, path, body) {
  const headers = { Authorization: `Bearer ${TOKEN}` };
  const init = { method, headers };
  if (body !== undefined) { init.body = body; if (typeof body === 'string') headers['Content-Type'] = 'application/json'; }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, init), env, ctx);
  return { status: res.status, text: await res.text().catch(() => '') };
}

const WRITES = [
  ['PUT', `/api/simulations/${SIM}/space`, () => new Uint8Array([1, 2, 3])],
  ['POST', `/api/simulations/${SIM}/space/multipart/create`, () => JSON.stringify({})],
  ['POST', `/api/simulations/${SIM}/space/multipart/complete`, () => JSON.stringify({ upload_id: 'upload-x', parts: [{ part_number: 1, etag: 'part-1' }], total_size: 3 })],
  ['PUT', `/api/simulations/${SIM}/spaces/Main`, () => new Uint8Array([1, 2, 3])],
  ['PUT', `/api/simulations/${SIM}/thumbnail`, () => PNG],
];

const stored = async env => [
  await (await env.SCENES.get(`universes/${SIM}/universe.pak`)).text(),
  await (await env.SCENES.get(`universes/${SIM}/spaces/Main.pak`)).text(),
  await (await env.SCENES.get(`thumbnails/${SIM}/thumb.png`)).text(),
];

test('no content-writing route overwrites a quarantined listing, and none says why', async () => {
  const env = await seed({ status: 'quarantined' });
  for (const [method, path, body] of WRITES) {
    const r = await call(env, method, path, body());
    assert.equal(r.status, 409, `${method} ${path}`);
    assert.equal(JSON.parse(r.text).code, 'in_review');
    assert.doesNotMatch(r.text, /quarantin|legal|csam|hold/i, `${method} ${path}: ${r.text}`);
  }
  assert.deepEqual(await stored(env), [EVIDENCE, EVIDENCE, EVIDENCE], 'the evidence is untouched');
  assert.equal(JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)).moderation.status, 'quarantined', 'and the hold marker stays');
});

test('a publish freeze from before 2026-10-15 no longer locks the author\'s other listings', async () => {
  // McKale, 2026-10-15: no automatic freeze, no automatic ban. A quarantine removes one listing; the author keeps the rest.
  const env = await seed({ status: 'pending' });
  await env.USERS.put(`publish-frozen:${AUTHOR}`, JSON.stringify({ sim_id: 'another-listing', at: new Date().toISOString() }));
  for (const [method, path, body] of [WRITES[0], WRITES[3], WRITES[4]]) {
    const r = await call(env, method, path, body());
    assert.equal(r.status, 200, `${method} ${path}: ${r.text}`);
  }
  assert.notDeepEqual(await stored(env), [EVIDENCE, EVIDENCE, EVIDENCE], 'the author\'s writes went through');
});

test('control: the same writes still work for a listing that is not held', async () => {
  const env = await seed({ status: 'pending' });
  for (const [method, path, body] of [WRITES[0], WRITES[3], WRITES[4]]) {
    const r = await call(env, method, path, body());
    assert.equal(r.status, 200, `${method} ${path}: ${r.text}`);
  }
  assert.notDeepEqual(await stored(env), [EVIDENCE, EVIDENCE, EVIDENCE], 'the content was replaced');
});

// ── the case, not the listing's marker, decides whether content is locked ──

async function withCase(moderation, rec) {
  const env = await seed(moderation);
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: AUTHOR, history: [], ...rec }));
  return env;
}

test('an escalated legal hold on an approved listing locks every content writer, though the marker says approved', async () => {
  const env = await withCase({ status: 'approved' }, { status: 'approved', legal_hold: { category: 'terrorism_or_extremist_promotion', escalated: true } });
  for (const [method, path, body] of [...WRITES, ['PUT', `/api/simulations/${SIM}/website-manifest`, () => JSON.stringify({})]]) {
    const r = await call(env, method, path, body());
    assert.equal(r.status, 409, `${method} ${path}: ${r.text}`);
    assert.equal(JSON.parse(r.text).code, 'in_review');
    assert.doesNotMatch(r.text, /quarantin|legal|terror|hold/i);
  }
  assert.deepEqual(await stored(env), [EVIDENCE, EVIDENCE, EVIDENCE]);
});

test('a hard-category hold, whose listing marker says held, locks the content the same way', async () => {
  const env = await withCase({ status: 'held' }, { status: 'held', decision: { lane: 'legal', outcome: 'hold' } });
  for (const [method, path, body] of WRITES) {
    assert.equal((await call(env, method, path, body())).status, 409, `${method} ${path}`);
  }
  assert.deepEqual(await stored(env), [EVIDENCE, EVIDENCE, EVIDENCE]);
});

test('a hold in another lane does not lock the content: the author may fix what a person is looking at', async () => {
  const env = await withCase({ status: 'held' }, { status: 'held', decision: { lane: 'quality', outcome: 'hold' } });
  for (const [method, path, body] of [WRITES[0], WRITES[3], WRITES[4]]) {
    assert.equal((await call(env, method, path, body())).status, 200, `${method} ${path}`);
  }
});

test('an author with a publish freeze from before 2026-10-15 still publishes a new listing', async () => {
  const env = await seed({ status: 'approved' });
  await env.USERS.put(`publish-frozen:${AUTHOR}`, JSON.stringify({ sim_id: SIM }));
  const r = await call(env, 'POST', '/api/simulations/publish', JSON.stringify({ name: 'Another' }));
  assert.notEqual(r.status, 409, r.text);
  assert.notEqual(JSON.parse(r.text).code, 'in_review');
});
