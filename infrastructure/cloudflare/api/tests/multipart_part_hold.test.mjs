// The multipart Part route refuses a listing under a legal hold, as Create and Complete on either side of it do, with the same
// answer they give: a part is content, a held listing's content is evidence, and the refusal reads as ordinary review, never as
// a hold. Checked against the real Worker with fake storage, with R2's upload of the part watched.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = mintToken(AUTHOR);

// The listing, an optional review case and an optional R2 legal lock; R2's part uploads are recorded in `env.partsUploaded`.
async function seed(moderation, { rec = null, lock = null } = {}) {
  const env = fakeEnv();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author', created_at: '2026-01-01T00:00:00.000Z' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'World', description: '', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, moderation, play_count: 0, favorite_count: 0, version: 1,
    r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z',
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  await env.SCENES.put(`universes/${SIM}/universe.pak`, 'what the author published');
  if (rec) await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: AUTHOR, history: [], ...rec }));
  if (lock) await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: lock }), { customMetadata: { kind: lock } });
  env.partsUploaded = [];
  const resume = env.SCENES.resumeMultipartUpload.bind(env.SCENES);
  env.SCENES.resumeMultipartUpload = (key, uploadId) => {
    const upload = resume(key, uploadId);
    return { ...upload, uploadPart: async (n, body) => { env.partsUploaded.push({ key, uploadId, n }); return upload.uploadPart(n, body); } };
  };
  return env;
}

async function call(env, method, path, body) {
  const headers = { Authorization: `Bearer ${TOKEN}` };
  if (typeof body === 'string') headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body }), env, ctx);
  return { status: res.status, text: await res.text() };
}

const part = (env) => call(env, 'PUT', `/api/simulations/${SIM}/space/multipart/part?upload_id=upload-x&part_number=1`, new Uint8Array([1, 2, 3]));
const create = (env) => call(env, 'POST', `/api/simulations/${SIM}/space/multipart/create`, JSON.stringify({}));
const complete = (env) => call(env, 'POST', `/api/simulations/${SIM}/space/multipart/complete`, JSON.stringify({ upload_id: 'upload-x', parts: [{ part_number: 1, etag: 'part-1' }], total_size: 3 }));

const HOLDS = [
  ['a quarantined listing', () => seed({ status: 'quarantined' })],
  ['an R2 legal lock on an approved listing', () => seed({ status: 'approved', rating: 'all_ages' }, { lock: 'content_lock' })],
  ['an escalated legal hold on the case of an approved listing', () => seed({ status: 'approved', rating: 'all_ages' }, { rec: { status: 'approved', legal_hold: { category: 'terrorism_or_extremist_promotion', escalated: true } } })],
  ['a hard-category hold, whose marker says held', () => seed({ status: 'held' }, { rec: { status: 'held', decision: { lane: 'legal', outcome: 'hold' } } })],
];

for (const [what, make] of HOLDS) {
  test(`an upload part to ${what} is refused, with Create's and Complete's answer, and no part reaches R2`, async () => {
    const env = await make();
    const r = await part(env);
    assert.equal(r.status, 409, r.text);
    assert.equal(JSON.parse(r.text).code, 'in_review');
    assert.doesNotMatch(r.text, /quarantin|legal|terror|csam|hold/i, r.text);
    assert.deepEqual(env.partsUploaded, [], 'nothing was uploaded');
    for (const sibling of [create, complete]) {
      const s = await sibling(env);
      assert.equal(s.status, r.status, 'the same status as its sibling routes');
      assert.equal(s.text, r.text, 'and the same body');
    }
    assert.equal(await (await env.SCENES.get(`universes/${SIM}/universe.pak`)).text(), 'what the author published', 'the evidence is untouched');
  });
}

test('control: a part to a listing that is not held is uploaded, as before', async () => {
  for (const env of [
    await seed({ status: 'pending' }),
    // A hold in another lane does not lock the content: the author may fix what a person is looking at.
    await seed({ status: 'held' }, { rec: { status: 'held', decision: { lane: 'quality', outcome: 'hold' } } }),
  ]) {
    const r = await part(env);
    assert.equal(r.status, 200, r.text);
    assert.deepEqual(JSON.parse(r.text), { part_number: 1, etag: 'part-1', size: 3 });
    assert.deepEqual(env.partsUploaded, [{ key: `universes/${SIM}/universe.pak`, uploadId: 'upload-x', n: 1 }]);
  }
});

test('the author check still comes first: a part to another account\'s held listing reads as not yours, as Create does', async () => {
  const env = await seed({ status: 'quarantined' });
  const STRANGER = '22222222-2222-4222-8222-222222222222';
  await env.USERS.put(`user:${STRANGER}`, JSON.stringify({ kyc_verified: true, id: STRANGER, username: 'stranger', created_at: '2026-01-01T00:00:00.000Z' }));
  const stranger = { Authorization: `Bearer ${mintToken(STRANGER)}` };
  for (const [method, path, body] of [
    ['PUT', `/api/simulations/${SIM}/space/multipart/part?upload_id=upload-x&part_number=1`, new Uint8Array([1])],
    ['POST', `/api/simulations/${SIM}/space/multipart/create`, '{}'],
  ]) {
    const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: stranger, body }), env, ctx);
    assert.equal(res.status, 403, `${method} ${path}`);
  }
  assert.deepEqual(env.partsUploaded, []);
});
