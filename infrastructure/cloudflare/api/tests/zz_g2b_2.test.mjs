// Probe G2B-2: a listing's images and its page agree for a visitor with no session. The probe found the page of a teen_13
// or mature_17 listing refused signed out (the ages_10 cap of 2026-10-16) while its thumbnail, cover and previews were
// served, because those routes gate on isAdultListing only (refuseUnsignedImage; listPreviews calls refuseAdult only for
// adult listings). McKale, 2026-11-19, closed it the other way: solo browsing needs no sign-in up to mature_17
// (src/adult_content.mjs SOLO_SIGNED_OUT_CAP), so the page is served signed out as its images are. An adult listing's
// images still need a signed URL, which only a viewer allowed to see it is given.
import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, fakeDoNamespace, ctx, testPng } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { captureR2Key, snapshotReviewed } from '../src/moderation.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const ORIGIN = 'https://api.eustress.dev';
const COVER_AT = '2026-02-03T04:05:06.000Z';
const THUMB = testPng(64, 36, 1);
const COVER = testPng(600, 800, 2);
const FRAME = testPng(640, 360, 3);

async function listing(env, id, rating) {
  const sim = {
    id, name: `World ${id.slice(-1)}`, description: '', genre: 'all', author_id: AUTHOR, author_name: 'maker', is_public: true,
    max_players: 8, play_count: 0, favorite_count: 0, version: 1, published_at: '2026-03-01T00:00:00.000Z', updated_at: COVER_AT,
    moderation: { status: 'approved', rating },
    thumbnail_url: `${ORIGIN}/api/simulations/${id}/thumbnail`,
    cover: { ext: 'png', width: 600, height: 800, bytes: COVER.length, updated_at: COVER_AT },
    r2_key: `universes/${id}/universe.pak`,
  };
  await env.SCENES.put(sim.r2_key, 'the world');
  await env.SCENES.put(`thumbnails/${id}/thumb.png`, THUMB);
  await env.SCENES.put(`covers/${id}/cover.png`, COVER);
  await env.SCENES.put(captureR2Key(id, 0, 'png', 0), FRAME);
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
  const reviewed = await snapshotReviewed(env, sim, 0);
  await env.SCENES.put(`locks/approved/${id}.json`, JSON.stringify({ v: 1, ...reviewed, evidence_epoch: 0, by: 'test', at: '2026-01-02T00:00:00.000Z' }));
}

async function get(env, path) {
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method: 'GET', headers: { 'CF-Connecting-IP': '203.0.113.7' } }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  let body = null;
  try { body = JSON.parse(new TextDecoder().decode(bytes)); } catch { /* an image */ }
  return { status: res.status, body, bytes };
}

test('G2B-2: a signed-out visitor is served a teen or mature listing page and its thumbnail, cover and previews alike; an adult one neither', async () => {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'maker' }));
  const imagePaths = (id) => [`/api/simulations/${id}/thumbnail`, `/api/simulations/${id}/cover`, `/api/simulations/${id}/previews`, `/api/simulations/${id}/preview/0`];
  for (const [id, rating] of [['cccccccc-0000-4000-8000-000000000001', 'teen_13'], ['cccccccc-0000-4000-8000-000000000002', 'mature_17']]) {
    await listing(env, id, rating);
    const page = await get(env, `/api/simulations/${id}`);
    assert.equal(page.status, 200, `${rating}: the page is served signed out`);
    for (const path of imagePaths(id)) assert.equal((await get(env, path)).status, 200, `${rating} ${path.split(id)[1]}`);
  }
  // Adult content: the page and every image are refused to the same visitor.
  const adult = 'cccccccc-0000-4000-8000-000000000003';
  await listing(env, adult, 'adult_18');
  const page = await get(env, `/api/simulations/${adult}`);
  assert.deepEqual([page.status, page.body?.code], [403, 'adult_content_off'], 'adult_18: the page');
  for (const path of imagePaths(adult)) assert.equal((await get(env, path)).status, 403, `adult_18 ${path.split(adult)[1]}`);
});
