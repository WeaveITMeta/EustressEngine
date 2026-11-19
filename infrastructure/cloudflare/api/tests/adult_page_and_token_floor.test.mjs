// Two holes the independent review of the solo signed-out change found (2026-11-19), each closed here.
//
// 1. The sim page (src/page.mjs): GET /api/simulations/{id}/page and /page/image/{slot} read no rating and no viewer, so an
//    adult listing's theme, links, README and page images went to anyone signed out, publicly cached, while the listing
//    GET itself answered 403. The page is now gated as the listing is (refuseAdult, the solo surface, judged as the
//    request's session or its play pass), an adult page is `private, no-store`, and its images are named by signed URL
//    and served only on one (the cover and thumbnail scheme, src/adult_content.mjs).
// 2. A world play token (src/world_play_token.mjs) minted while a listing is mature_17 kept serving chunks for up to 15
//    minutes after a re-rate to adult_18. Every rating write now moves the listing's play-token floor (an R2 object of its
//    own), and a chunk GET on a token minted at or before the floor runs the full gate.

import test from 'node:test';
import assert from 'node:assert/strict';
import { deflateSync } from 'node:zlib';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeBucket, fakeDoNamespace, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import { handlePageRoute } from '../src/page.mjs';
import { readLinkState, writeLinkState } from '../src/link_review.mjs';
import { adultContentKey, ADULT_CONTENT_OFF, AGE_21_REQUIRED, signedImageUrl } from '../src/adult_content.mjs';
import { makePlayPass } from '../src/play_pass.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { LiveHost } from '../src/live.mjs';
import { manifestKey, chunkKey } from '../src/world.mjs';
import { identOfObject, makeToolExecutor } from '../src/moderation.mjs';
import { PLAY_TOKEN_HEADER, playTokenFloorKey } from '../src/world_play_token.mjs';

// ── 1. the sim page ──────────────────────────────────────────────────────────

const SIM = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';
const ORIGIN = 'https://api.eustress.dev';
const OWNER = 'owner-1';
const FAN = 'fan-1'; // signed in, Adult content on, no verified age on record
const OFF = 'off-1'; // signed in, Adult content off
const SECRET_WORDS = 'Explicit words only an adult viewer may read.';

function png(w = 4, h = 4) {
  const table = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = buf => { let c = 0xffffffff; for (const b of buf) c = table[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const t = Buffer.from(type, 'ascii'); const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const c = Buffer.alloc(4); c.writeUInt32BE(crc(Buffer.concat([t, data]))); return Buffer.concat([len, t, data, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h, 0x7f); for (let y = 0; y < h; y++) raw[y * (w * 3 + 1)] = 0;
  return new Uint8Array(Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]));
}
const sniffFileType = buf => (new Uint8Array(buf.slice(0, 4))[0] === 0x89 ? 'image/png' : null);
const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json', ...headers } });
const SESSIONS = { 'Bearer owner': OWNER, 'Bearer fan': FAN, 'Bearer off': OFF };
const deps = { verifyAuth: async req => SESSIONS[req.headers.get('Authorization')] || null, json, cors: {}, sniffFileType, policyText: GUARDIAN_POLICY_TEXT };

const call = async (env, method, pathOrUrl, { who = null, body = null, headers = {} } = {}) => {
  const url = new URL(pathOrUrl.startsWith('http') ? pathOrUrl : `${ORIGIN}/api/simulations/${SIM}${pathOrUrl}`);
  const h = { ...headers };
  if (who) h.Authorization = `Bearer ${who}`;
  const req = new Request(url, { method, headers: h, ...(body !== null ? { body } : {}) });
  return handlePageRoute(req, url, env, null, deps);
};

/// A listing rated `rating` (undefined: none) with a full page whose review passed: a theme, a logo, a gallery image, and
/// a README with one image and words that must not leak.
async function pageEnv(rating) {
  const env = { SOCIAL: fakeKv(), SCENES: fakeBucket(), USERS: fakeKv(), KYC_STATUS: fakeKv(), JWT_SECRET };
  env.SOCIAL.store.set(`sim:${SIM}`, JSON.stringify({ id: SIM, author_id: OWNER, name: 'Lab', description: 'Lab', is_public: true, moderation: { status: 'approved', ...(rating ? { rating } : {}) } }));
  await env.USERS.put(adultContentKey(FAN), '1');
  const spec = { schema: 1, theme: { preset: 'studio', colors: {}, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 }, images: { logo: 'logo', banner: null, gallery: ['gallery-0'] }, links: [] };
  assert.equal((await call(env, 'PUT', '/page', { who: 'owner', body: JSON.stringify(spec) })).status, 200);
  for (const slot of ['logo', 'gallery-0', 'readme-0']) assert.equal((await call(env, 'PUT', `/page/image/${slot}`, { who: 'owner', body: png() })).status, 200, slot);
  assert.equal((await call(env, 'PUT', '/page/readme', { who: 'owner', body: new TextEncoder().encode(`# Lab\n\n${SECRET_WORDS}\n\n![Cell](eustress-image:readme-0)\n`) })).status, 200);
  // The review passed for everything (the judge itself is tested in tests/page.test.mjs).
  const state = await readLinkState(env, SIM);
  for (const slot of ['logo', 'gallery-0', 'readme-0']) state.images[slot] = { ...state.images[slot], status: 'approved' };
  state.readme = { ...state.readme, status: 'approved' };
  await writeLinkState(env, SIM, state);
  return env;
}

const IMAGE_SLOTS = ['logo', 'gallery-0', 'readme-0'];

test('signed out: an adult_18, adult_21 or unrated listing\'s page and page images are refused; a mature_17 one is served as before', async () => {
  for (const rating of ['adult_18', 'adult_21', undefined]) {
    const env = await pageEnv(rating);
    const page = await call(env, 'GET', '/page');
    const text = await page.text();
    assert.equal(page.status, 403, `${rating}: ${text}`);
    assert.deepEqual(JSON.parse(text), { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: false }, String(rating));
    assert.equal(page.headers.get('Cache-Control'), 'private, no-store', String(rating));
    assert.ok(!text.includes('Explicit') && !text.includes('studio'), `${rating}: nothing of the page`);
    for (const slot of IMAGE_SLOTS) {
      const img = await call(env, 'GET', `/page/image/${slot}`);
      assert.equal(img.status, 403, `${rating} ${slot}`);
      assert.equal((await img.json()).code, 'adult_image_unsigned', `${rating} ${slot}`);
    }
    // A signed-in viewer whose Adult content is off is refused the page too.
    const off = await call(env, 'GET', '/page', { who: 'off' });
    assert.deepEqual([off.status, (await off.json()).code], [403, 'adult_content_off'], String(rating));
  }
  // mature_17: the solo cap, served to anyone with its public caching and plain image paths.
  const env = await pageEnv('mature_17');
  const page = await call(env, 'GET', '/page');
  assert.equal(page.status, 200);
  assert.equal(page.headers.get('Cache-Control'), 'public, max-age=60');
  const view = await page.json();
  assert.ok(view.readme.markdown.includes(SECRET_WORDS));
  assert.equal(view.images.logo, `/api/simulations/${SIM}/page/image/logo`);
  assert.deepEqual(view.readme.images, [`/api/simulations/${SIM}/page/image/readme-0`]);
  for (const slot of IMAGE_SLOTS) {
    const img = await call(env, 'GET', `/page/image/${slot}`);
    assert.equal(img.status, 200, slot);
    assert.equal(img.headers.get('Cache-Control'), 'public, max-age=300', slot);
  }
});

test('signed in with Adult content on: an adult_18 page is served private, no-store, with every image named by a signed URL that alone serves it', async () => {
  const env = await pageEnv('adult_18');
  const page = await call(env, 'GET', '/page', { who: 'fan' });
  assert.equal(page.status, 200);
  assert.equal(page.headers.get('Cache-Control'), 'private, no-store');
  const view = await page.json();
  assert.ok(view.readme.markdown.includes(SECRET_WORDS));
  const urls = [view.images.logo, ...view.images.gallery, ...view.readme.images];
  assert.equal(urls.length, 3);
  for (const u of urls) assert.match(u, /^https:\/\/api\.eustress\.dev\/api\/simulations\/[a-f0-9-]+\/page\/image\/[a-z0-9-]+\?exp=\d+&sig=[A-Za-z0-9_-]{43}$/, u);
  // The README's Markdown names the same signed URL, and no unsigned path is left in it.
  assert.ok(view.readme.markdown.includes(`![Cell](${view.readme.images[0]})`), view.readme.markdown);
  assert.ok(!view.readme.markdown.includes(`/page/image/readme-0)`), view.readme.markdown);
  for (const u of urls) {
    const img = await call(env, 'GET', u);
    assert.equal(img.status, 200, u);
    assert.match(img.headers.get('Cache-Control'), /^private, max-age=\d+$/, u);
  }
  // The author, and a play pass for this listing, are judged the same way as the listing GET judges them.
  assert.equal((await call(env, 'GET', '/page', { who: 'owner' })).status, 200);
  const { pass } = await makePlayPass(env, { accountId: FAN, simId: SIM, issued: Math.floor(Date.now() / 1000) });
  const viaPass = await call(env, 'GET', '/page', { headers: { Authorization: `PlayPass ${pass}` } });
  assert.equal(viaPass.status, 200, 'a play pass stands for its account');
  // A signed URL for another slot is no key to this one.
  const other = await signedImageUrl(env, ORIGIN, `/api/simulations/${SIM}/page/image/gallery-0`);
  assert.equal((await call(env, 'GET', other.replace('gallery-0', 'logo'))).status, 403);
  // adult_21 needs a verified age of 21: this account has none on record.
  const env21 = await pageEnv('adult_21');
  const r21 = await call(env21, 'GET', '/page', { who: 'fan' });
  assert.deepEqual([r21.status, await r21.json()], [403, { error: AGE_21_REQUIRED, code: 'age_21_required', signed_in: true }]);
});

// ── 2. the play-token floor ──────────────────────────────────────────────────

const worker = await loadWorker();
const WORLD = 'bbbbbbbb-0000-4000-8000-000000000017';
const H = 'c'.repeat(64);
const WORLD_SHA = 'f'.repeat(64);
const ADMIN = '99999999-9999-4999-8999-999999999999';

async function worldEnv() {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  const shape = JSON.stringify({ format: 'echk', spaces: [{ name: 'Main', chunks: [{ blake3: H }] }], assets: [] });
  const sim = {
    id: WORLD, name: 'World', description: 'a place', author_id: '11111111-1111-4111-8111-111111111111', author_name: 'maker', is_public: true, format: 'echk',
    published_at: '2026-03-01T00:00:00.000Z', moderation: { status: 'approved', rating: 'mature_17' },
    world: { manifest_key: manifestKey(WORLD, WORLD_SHA), manifest_sha256: WORLD_SHA },
  };
  sim.r2_key = sim.world.manifest_key;
  await env.SCENES.put(sim.world.manifest_key, shape);
  await env.SCENES.put(chunkKey(WORLD, H), 'ECHK chunk bytes');
  await env.SOCIAL.put(`sim:${WORLD}`, JSON.stringify(sim));
  const content = identOfObject(await env.SCENES.head(sim.r2_key));
  await env.SCENES.put(`locks/approved/${WORLD}.json`, JSON.stringify({ v: 1, content, thumb: null, cover: null, source: null, captures: null, evidence_epoch: 0, by: 'test', at: 'now' }));
  await env.SOCIAL.put(`modcase:${WORLD}`, JSON.stringify({ sim_id: WORLD, author_id: sim.author_id, status: 'approved', decision: { outcome: 'approve', rating: 'mature_17' }, history: [], created_at: '2026-03-01T00:00:00.000Z', updated_at: '2026-03-01T00:00:00.000Z' }));
  return env;
}

const get = async (env, path, headers = {}) => {
  const res = await worker.fetch(new Request(`${ORIGIN}/api/simulations/${WORLD}${path}`, { headers: { 'CF-Connecting-IP': '203.0.113.7', ...headers } }), env, ctx);
  const text = await res.text();
  let body = null; try { body = JSON.parse(text); } catch { /* bytes */ }
  return { status: res.status, headers: res.headers, text, body };
};

test('a play token minted at mature_17 is refused after moderation_set_rating re-rates the listing adult_18', async () => {
  const env = await worldEnv();
  const manifest = await get(env, '/world/manifest');
  assert.equal(manifest.status, 200, manifest.text);
  const token = manifest.headers.get(PLAY_TOKEN_HEADER);
  assert.ok(token, 'a token is minted at mature_17');
  // Control: the token serves a chunk while the rating stands.
  const before = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
  assert.deepEqual([before.status, before.headers.get('X-Eustress-Chunk-Gate')], [200, 'play-token']);

  const exec = makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN });
  const set = await exec('moderation_set_rating', { sim_id: WORLD, rating: 'adult_18' });
  assert.equal(set.ok, true, JSON.stringify(set));
  assert.ok(await env.SCENES.head(playTokenFloorKey(WORLD)), 'the rating write moved the floor');

  const after = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
  assert.notEqual(after.headers.get('X-Eustress-Chunk-Gate'), 'play-token', 'not served on the token');
  assert.deepEqual([after.status, after.body?.code], [403, 'adult_content_off'], after.text);
  assert.ok(!after.text.includes('ECHK'));
  // And no new token is minted for an adult listing.
  assert.equal((await get(env, '/world/manifest')).status, 403);
});

test('an unreadable play-token floor honours no token, and a rating write that keeps the band still sends old tokens to the full gate', async () => {
  const env = await worldEnv();
  const token = (await get(env, '/world/manifest')).headers.get(PLAY_TOKEN_HEADER);
  assert.ok(token);
  for (const stored of ['not json', JSON.stringify({ v: 1, floor: 'soon' })]) {
    await env.SCENES.put(playTokenFloorKey(WORLD), stored);
    const garbled = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
    assert.equal(garbled.headers.get('X-Eustress-Chunk-Gate'), null, `an unreadable floor (${stored}): the full gate`);
    assert.equal(garbled.status, 200, 'which serves a mature_17 world signed out');
  }
  await env.SCENES.delete(playTokenFloorKey(WORLD));
  assert.equal((await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token })).headers.get('X-Eustress-Chunk-Gate'), 'play-token');
  const exec = makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN });
  assert.equal((await exec('moderation_set_rating', { sim_id: WORLD, rating: 'teen_13' })).ok, true);
  const lowered = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
  assert.deepEqual([lowered.status, lowered.headers.get('X-Eustress-Chunk-Gate')], [200, null]);
});

// ── 3. a page is held back where its listing GET is ─────────────────────────

test('a pending listing\'s page and page image get the listing GET\'s own answer, signed out and as a stranger; its author is served', async () => {
  const PENDING = 'cccccccc-0000-4000-8000-0000000000aa';
  const AUTHOR_ID = '11111111-1111-4111-8111-111111111111';
  const author = mintToken(AUTHOR_ID);
  const stranger = mintToken('22222222-2222-4222-8222-222222222222');
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  for (const [id, username] of [[AUTHOR_ID, 'maker'], ['22222222-2222-4222-8222-222222222222', 'stranger']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  await env.SOCIAL.put(`sim:${PENDING}`, JSON.stringify({ id: PENDING, author_id: AUTHOR_ID, author_name: 'maker', name: 'Lab', description: 'Lab', is_public: true, published_at: '2026-03-01T00:00:00.000Z', moderation: { status: 'pending', rating: 'teen_13' } }));
  const req = async (method, path, token, body) => {
    const res = await worker.fetch(new Request(path.startsWith('http') ? path : `${ORIGIN}/api/simulations/${PENDING}${path}`, { method, headers: { 'CF-Connecting-IP': '203.0.113.7', ...(token ? { Authorization: `Bearer ${token}` } : {}) }, ...(body ? { body } : {}) }), env, ctx);
    const text = await res.text();
    let parsed = null; try { parsed = JSON.parse(text); } catch { /* bytes */ }
    return { status: res.status, headers: res.headers, text, body: parsed };
  };
  const spec = { schema: 1, theme: { preset: 'studio', colors: {}, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 }, images: { logo: 'logo', banner: null, gallery: [] }, links: [] };
  assert.equal((await req('PUT', '/page', author, JSON.stringify(spec))).status, 200);
  assert.equal((await req('PUT', '/page/image/logo', author, png())).status, 200);
  const state = await readLinkState(env, PENDING);
  state.images.logo = { ...state.images.logo, status: 'approved' };
  await writeLinkState(env, PENDING, state);

  for (const [who, token] of [['signed out', null], ['a stranger', stranger]]) {
    const listing = await req('GET', '', token);
    assert.deepEqual([listing.status, listing.body], [404, { error: 'Simulation not found' }], `${who}: the listing GET`);
    for (const path of ['/page', '/page/image/logo']) {
      const r = await req('GET', path, token);
      assert.deepEqual([r.status, r.body], [listing.status, listing.body], `${who}: ${path} answers as the listing GET`);
      assert.ok(!r.text.includes('studio'), `${who}: ${path}`);
    }
  }
  // The author sees the page, never cached, its image by a signed URL that serves it privately.
  const own = await req('GET', '/page', author);
  assert.equal(own.status, 200, own.text);
  assert.equal(own.headers.get('Cache-Control'), 'private, no-store');
  assert.match(own.body.images.logo, /\/page\/image\/logo\?exp=\d+&sig=/);
  const img = await req('GET', own.body.images.logo, null);
  assert.equal(img.status, 200);
  assert.match(img.headers.get('Cache-Control'), /^private, max-age=\d+$/);
});

// ── 4. review follow-ups M1, L1, L2 ──────────────────────────────────────────

test('M1: a missing, held, quarantined, pending or private listing\'s page and page image answer alike, signed out and as a stranger', async () => {
  const AUTHOR_ID = '11111111-1111-4111-8111-111111111111';
  const author = mintToken(AUTHOR_ID);
  const stranger = mintToken('22222222-2222-4222-8222-222222222222');
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  for (const [id, username] of [[AUTHOR_ID, 'maker'], ['22222222-2222-4222-8222-222222222222', 'stranger']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  const req = async (method, simId, path, token, body) => {
    const res = await worker.fetch(new Request(`${ORIGIN}/api/simulations/${simId}${path}`, { method, headers: { 'CF-Connecting-IP': '203.0.113.7', ...(token ? { Authorization: `Bearer ${token}` } : {}) }, ...(body ? { body } : {}) }), env, ctx);
    return { status: res.status, cache: res.headers.get('Cache-Control'), text: await res.text() };
  };
  const spec = { schema: 1, theme: { preset: 'studio', colors: {}, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 }, images: { logo: 'logo', banner: null, gallery: [] }, links: [] };
  const record = (id, moderation, extra = {}) => JSON.stringify({ id, author_id: AUTHOR_ID, author_name: 'maker', name: 'Lab', description: 'Lab', is_public: true, published_at: '2026-03-01T00:00:00.000Z', moderation, ...extra });
  const ids = {
    missing: 'dddddddd-0000-4000-8000-0000000000a0',
    held: 'dddddddd-0000-4000-8000-0000000000a1',
    quarantined: 'dddddddd-0000-4000-8000-0000000000a2',
    pending: 'dddddddd-0000-4000-8000-0000000000a3',
    private: 'dddddddd-0000-4000-8000-0000000000a4',
  };
  const finalRecord = {
    held: (id) => record(id, { status: 'held', rating: 'teen_13' }),
    quarantined: (id) => record(id, { status: 'quarantined', rating: 'teen_13' }),
    pending: (id) => record(id, { status: 'pending', rating: 'teen_13' }),
    private: (id) => record(id, { status: 'approved', rating: 'teen_13' }, { is_public: false }),
  };
  // Each one has a full page whose review passed; only its listing's state differs.
  for (const [kind, make] of Object.entries(finalRecord)) {
    const id = ids[kind];
    await env.SOCIAL.put(`sim:${id}`, record(id, { status: 'pending', rating: 'teen_13' }));
    assert.equal((await req('PUT', id, '/page', author, JSON.stringify(spec))).status, 200, kind);
    assert.equal((await req('PUT', id, '/page/image/logo', author, png())).status, 200, kind);
    const state = await readLinkState(env, id);
    state.images.logo = { ...state.images.logo, status: 'approved' };
    await writeLinkState(env, id, state);
    await env.SOCIAL.put(`sim:${id}`, make(id));
  }
  for (const [who, token] of [['signed out', null], ['a stranger', stranger]]) {
    for (const path of ['/page', '/page/image/logo']) {
      const answers = {};
      for (const [kind, id] of Object.entries(ids)) answers[kind] = await req('GET', id, path, token);
      for (const [kind, a] of Object.entries(answers)) {
        assert.deepEqual(a, { status: 404, cache: 'no-store', text: JSON.stringify({ error: 'Simulation not found' }) }, `${who} ${path} ${kind}`);
      }
    }
  }
  // The author still reaches the pending and the private page.
  for (const kind of ['pending', 'private']) assert.equal((await req('GET', ids[kind], '/page', author)).status, 200, kind);
});

/// A listing at mature_17 with a world and an approval binding (worldEnv), its token, and the moderation tool executor.
async function tokenAtMature() {
  const env = await worldEnv();
  const token = (await get(env, '/world/manifest')).headers.get(PLAY_TOKEN_HEADER);
  assert.ok(token, 'a token is minted at mature_17');
  return { env, token, exec: makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN }) };
}
const ratingOfWorld = async (env) => JSON.parse(await env.SOCIAL.get(`sim:${WORLD}`)).moderation.rating;
const caseRatingOfWorld = async (env) => JSON.parse(await env.SOCIAL.get(`modcase:${WORLD}`)).decision.rating;

test('L1: a floor write that throws never leaves a re-rated listing serving old tokens: the re-rate is refused, or retried until the floor is written', async () => {
  // Storage refuses the floor every time: the re-rate is refused with nothing written, so the listing is not re-rated.
  {
    const { env, token, exec } = await tokenAtMature();
    const put = env.SCENES.put.bind(env.SCENES);
    let tries = 0;
    env.SCENES.put = async (key, ...rest) => { if (key === playTokenFloorKey(WORLD)) { tries += 1; throw new Error('R2 unavailable'); } return put(key, ...rest); };
    const set = await exec('moderation_set_rating', { sim_id: WORLD, rating: 'adult_18' });
    assert.deepEqual([set.ok, set.code], [false, 'play_token_floor_unavailable'], JSON.stringify(set));
    assert.equal(tries, 3, 'tried three times');
    assert.equal(await ratingOfWorld(env), 'mature_17', 'the listing was not re-rated');
    assert.equal(await caseRatingOfWorld(env), 'mature_17', 'nor the case');
    // The token serves what the listing still is.
    assert.equal((await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token })).status, 200);
  }
  // Storage refuses it twice, then takes it: the re-rate goes through, and the old token is refused.
  {
    const { env, token, exec } = await tokenAtMature();
    const put = env.SCENES.put.bind(env.SCENES);
    let fails = 2;
    env.SCENES.put = async (key, ...rest) => { if (key === playTokenFloorKey(WORLD) && fails > 0) { fails -= 1; throw new Error('R2 unavailable'); } return put(key, ...rest); };
    assert.equal((await exec('moderation_set_rating', { sim_id: WORLD, rating: 'adult_18' })).ok, true);
    assert.equal(await ratingOfWorld(env), 'adult_18');
    const after = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
    assert.deepEqual([after.status, after.headers.get('X-Eustress-Chunk-Gate')], [403, null]);
  }
});

test('L1: a decision that changes the rating is refused whole, before the case is saved, when the floor cannot be written', async () => {
  const { env, token, exec } = await tokenAtMature();
  const put = env.SCENES.put.bind(env.SCENES);
  env.SCENES.put = async (key, ...rest) => { if (key === playTokenFloorKey(WORLD)) throw new Error('R2 unavailable'); return put(key, ...rest); };
  let answer;
  try { answer = await exec('moderation_approve', { sim_id: WORLD, rating: 'adult_18', rationale: 'Re-rated by a person after a second look.' }); }
  catch (e) { answer = { thrown: e.name }; }
  assert.ok(answer?.ok !== true, JSON.stringify(answer));
  assert.equal(await caseRatingOfWorld(env), 'mature_17', 'the case was not saved with the new rating');
  assert.equal(await ratingOfWorld(env), 'mature_17', 'nor the listing');
  assert.equal((await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token })).status, 200, 'the token serves what the listing still is');
});

test('L2: a token whose manifest GET read the listing before the floor was written is refused, however late it was minted', async () => {
  const env = await worldEnv();
  const exec = makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN });
  const realNow = Date.now;
  let now = realNow();
  Date.now = () => now;
  try {
    // The manifest GET reads the listing at mature_17; while it is in flight the listing is re-rated adult_18, and the GET
    // then takes five minutes more before it mints (past the floor's lead).
    const get0 = env.SOCIAL.get.bind(env.SOCIAL);
    let armed = true;
    env.SOCIAL.get = async (key, ...rest) => {
      const value = await get0(key, ...rest);
      if (armed && key === `sim:${WORLD}`) {
        armed = false;
        assert.equal((await exec('moderation_set_rating', { sim_id: WORLD, rating: 'adult_18' })).ok, true);
        now += 300_000;
      }
      return value;
    };
    const manifest = await get(env, '/world/manifest');
    env.SOCIAL.get = get0;
    assert.equal(manifest.status, 200, 'served from the read made before the re-rate');
    const token = manifest.headers.get(PLAY_TOKEN_HEADER);
    assert.ok(token, 'and a token was minted from it');
    const chunk = await get(env, `/world/chunks/${H}`, { [PLAY_TOKEN_HEADER]: token });
    assert.notEqual(chunk.headers.get('X-Eustress-Chunk-Gate'), 'play-token', 'the token is not honoured');
    assert.deepEqual([chunk.status, chunk.body?.code], [403, 'adult_content_off']);
  } finally {
    Date.now = realNow;
  }
});
