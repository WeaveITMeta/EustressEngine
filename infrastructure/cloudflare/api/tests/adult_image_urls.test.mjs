// An adult listing's images (src/adult_content.mjs, "Signed image URLs"): image tags carry no session, so the thumbnail, the
// cover and the preview frames of a listing rated adult_18, or of one with no rating, are served only on a signed URL: an
// HMAC over the path and an expiry, keyed by HMAC(JWT_SECRET, "adult-image-url-v1"). The Worker writes those URLs into the
// listing's own thumbnail_url, cover_url and preview url fields for a viewer allowed to see the listing (one who turned
// Adult content on, or its author), and never for anyone else. A URL issued in one 15-minute window expires at the end of
// the next, so it lives 15 to 30 minutes and is the same URL for every request in its window. Every other listing's images
// keep their public, cached URLs. The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx, testPng, JWT_SECRET } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { captureR2Key, snapshotReviewed } from '../src/moderation.mjs';
import {
  adultContentKey, mayView, signedImageUrl, signedThumbnailUrl, verifyImageUrl, imageUrlExpiry, IMAGE_URL_WINDOW_SECONDS,
} from '../src/adult_content.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111'; // the author of every listing below; Adult content off
const ON = '22222222-2222-4222-8222-222222222222'; // turned Adult content on
const OFF = '33333333-3333-4333-8333-333333333333'; // signed in, never turned it on
const ADMIN = '66666666-6666-4666-8666-666666666666'; // an administrator, Adult content off unless a test turns it on
const YOUNG = '77777777-7777-4777-8777-777777777777'; // turned Adult content on; a verified age of 19
const tokens = { author: mintToken(AUTHOR), on: mintToken(ON), off: mintToken(OFF), admin: mintToken(ADMIN), young: mintToken(YOUNG) };

const KIDS = 'bbbbbbbb-0000-4000-8000-000000000001';
const ADULT = 'bbbbbbbb-0000-4000-8000-000000000002';
const UNRATED = 'bbbbbbbb-0000-4000-8000-000000000003';
const OTHER_ADULT = 'bbbbbbbb-0000-4000-8000-000000000004';
const PENDING_ADULT = 'bbbbbbbb-0000-4000-8000-000000000005';
const ADULT21 = 'bbbbbbbb-0000-4000-8000-000000000006'; // rated adult_21; a test that needs it lists it
const ORIGIN = 'https://api.eustress.dev';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const LINK = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${PIN}`;
const COVER_AT = '2026-02-03T04:05:06.000Z';
const THUMB = testPng(64, 36, 1);
const COVER = testPng(600, 800, 2);
const FRAME = testPng(640, 360, 3);

const thumbPath = (id) => `/api/simulations/${id}/thumbnail`;
const coverPath = (id) => `/api/simulations/${id}/cover`;
const framePath = (id, n = 0) => `/api/simulations/${id}/preview/${n}`;

/// A listing with a thumbnail, a cover and one preview frame, all bound by an approval as a review leaves them.
async function listing(env, id, rating, { status = 'approved' } = {}) {
  const sim = {
    id, name: `World ${id.slice(-1)}`, description: '', genre: 'all', author_id: AUTHOR, author_name: 'maker', is_public: true,
    max_players: 8, play_count: 0, favorite_count: 0, version: 1, published_at: `2026-03-0${id.slice(-1)}T00:00:00.000Z`, updated_at: COVER_AT,
    moderation: { status, ...(rating ? { rating } : {}) },
    thumbnail_url: `${ORIGIN}${thumbPath(id)}`,
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

async function environment() {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  for (const [id, username] of [[AUTHOR, 'maker'], [ON, 'ada'], [OFF, 'bob'], [ADMIN, 'root'], [YOUNG, 'cy']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  await env.USERS.put(adultContentKey(YOUNG), '1');
  await env.KYC_STATUS.put(`kyc-${YOUNG}-front`, JSON.stringify({ status: 'verified', age_verified: true, extracted_dob: `${new Date().getUTCFullYear() - 19}-01-01` }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(adultContentKey(ON), '1');
  // ON is a verified adult of 30, so the 21+ band (and a listing with no rating, which reads as it) is shown to them.
  await env.KYC_STATUS.put(`kyc-${ON}-front`, JSON.stringify({ status: 'verified', age_verified: true, extracted_dob: `${new Date().getUTCFullYear() - 30}-01-01` }));
  await listing(env, KIDS, 'all_ages');
  await listing(env, ADULT, 'adult_18');
  await listing(env, UNRATED, undefined);
  await listing(env, OTHER_ADULT, 'adult_18');
  await listing(env, PENDING_ADULT, 'adult_18', { status: 'pending' });
  return env;
}

/// A request to the Worker. `url` is a path on ORIGIN or a whole URL (a signed one, as a listing gives it).
async function call(env, method, url, { who, body } = {}) {
  const headers = {};
  if (who) headers.Authorization = `Bearer ${tokens[who]}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(url.startsWith('http') ? url : `${ORIGIN}${url}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const text = new TextDecoder().decode(bytes);
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* an image */ }
  return { status: res.status, headers: res.headers, bytes, text, body: parsed };
}
const get = (env, url, who) => call(env, 'GET', url, { who });
const same = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
const isSigned = (url) => typeof url === 'string' && /[?&]exp=\d+/.test(url) && /[?&]sig=[A-Za-z0-9_-]{43}(&|$)/.test(url);

/// Runs `fn` with Date.now at `ms`.
async function at(ms, fn) {
  const real = Date.now;
  Date.now = () => ms;
  try { return await fn(); } finally { Date.now = real; }
}

/// The URLs an opted-in viewer is given for a listing: its card's thumbnail and cover, and its first preview frame.
async function urlsFor(env, id, who = 'on') {
  const page = await get(env, `/api/simulations/${id}`, who);
  assert.equal(page.status, 200, page.text);
  const frames = await get(env, `/api/simulations/${id}/previews`, who);
  assert.equal(frames.status, 200, frames.text);
  return { thumbnail: page.body.thumbnail_url, cover: page.body.cover_url, frame: frames.body.previews[0]?.url };
}

function refusedImage(r, code, what) {
  assert.equal(r.status, 403, `${what}: ${r.status} ${r.text.slice(0, 120)}`);
  assert.equal(r.body?.code, code, what);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store', `${what}: a refusal is never cached`);
}

// ── the routes ───────────────────────────────────────────────────────────────

test('an adult or unrated listing\'s thumbnail, cover and preview frames are refused without a signature, to everyone, the author included', async () => {
  const env = await environment();
  for (const id of [ADULT, UNRATED]) {
    for (const path of [thumbPath(id), coverPath(id), `${coverPath(id)}?v=${Date.parse(COVER_AT)}`, framePath(id)]) {
      for (const who of [undefined, 'off', 'on', 'author', 'admin']) refusedImage(await get(env, path, who), 'adult_image_unsigned', `${path} ${who || 'signed out'}`);
    }
  }
});

test('the signed URL a listing gives an allowed viewer serves the image, privately, for no longer than it has left', async () => {
  const env = await environment();
  for (const id of [ADULT, UNRATED]) {
    const urls = await urlsFor(env, id);
    for (const [what, url, bytes, publicAge] of [['thumbnail', urls.thumbnail, THUMB, 3600], ['cover', urls.cover, COVER, 300], ['frame', urls.frame, FRAME, 300]]) {
      assert.ok(isSigned(url), `${what}: ${url}`);
      const r = await get(env, url);
      assert.equal(r.status, 200, `${what} ${id}: ${r.text.slice(0, 120)}`);
      assert.ok(same(r.bytes, bytes), `${what} is the stored image`);
      const left = Number(new URL(url).searchParams.get('exp')) - Math.floor(Date.now() / 1000);
      const maxAge = Number(/^private, max-age=(\d+)$/.exec(r.headers.get('Cache-Control'))?.[1]);
      assert.ok(maxAge > 0 && maxAge <= left && maxAge <= publicAge, `${what}: ${r.headers.get('Cache-Control')} with ${left} s left`);
    }
    assert.ok(urls.cover.includes(`v=${Date.parse(COVER_AT)}`), 'the cover keeps its own ?v=');
  }
});

test('every URL lives at least 15 minutes: one issued in the last second of a window works 14 minutes later and stops at its expiry', async () => {
  const env = await environment();
  const window = IMAGE_URL_WINDOW_SECONDS * 1000;
  const start = Math.floor(Date.now() / window) * window;
  const lastSecond = start + window - 1000;
  const urls = await at(lastSecond, () => urlsFor(env, ADULT));
  const expires = Number(new URL(urls.thumbnail).searchParams.get('exp'));
  assert.equal(expires * 1000, start + 2 * window, 'the end of the next window');
  assert.ok(expires * 1000 - lastSecond >= 15 * 60 * 1000, 'at least 15 minutes left');
  for (const url of Object.values(urls)) {
    assert.equal((await at(lastSecond + 14 * 60 * 1000, () => get(env, url))).status, 200, `14 minutes later: ${url}`);
    assert.equal((await at(expires * 1000 - 1000, () => get(env, url))).status, 200, 'its last second');
    refusedImage(await at(expires * 1000, () => get(env, url)), 'adult_image_expired', 'at its expiry');
    refusedImage(await at(expires * 1000 + 60 * 60 * 1000, () => get(env, url)), 'adult_image_expired', 'an hour past it');
  }
  // One URL for every request in a window, so a browser keeps it; a new one in the next window, and that lives 30 minutes.
  const first = await at(start, () => urlsFor(env, ADULT));
  assert.deepEqual(first, urls, 'the first and the last second of a window give the same URLs');
  const next = await at(start + window, () => urlsFor(env, ADULT));
  assert.notEqual(next.thumbnail, urls.thumbnail);
  assert.equal(Number(new URL(next.thumbnail).searchParams.get('exp')) * 1000, start + 3 * window);
  assert.equal(imageUrlExpiry(start) * 1000 - start, 2 * window, 'issued at the start of a window: 30 minutes');
});

test('a tampered URL is refused: another expiry, another path, another listing, a changed or malformed signature', async () => {
  const env = await environment();
  const { thumbnail, cover } = await urlsFor(env, ADULT);
  const with_ = (url, key, value) => { const u = new URL(url); u.searchParams.set(key, value); return u.toString(); };
  const exp = Number(new URL(thumbnail).searchParams.get('exp'));
  const sig = new URL(thumbnail).searchParams.get('sig');
  const flipped = (sig[0] === 'A' ? 'B' : 'A') + sig.slice(1);
  const cases = {
    'a later expiry': with_(thumbnail, 'exp', String(exp + IMAGE_URL_WINDOW_SECONDS)),
    'an expiry off the window': with_(thumbnail, 'exp', String(exp + 1)),
    'the signature on another path of the listing': `${ORIGIN}${coverPath(ADULT)}?exp=${exp}&sig=${sig}`,
    'the signature on another listing': `${ORIGIN}${thumbPath(OTHER_ADULT)}?exp=${exp}&sig=${sig}`,
    'the cover\'s signature on its thumbnail': `${ORIGIN}${thumbPath(ADULT)}?exp=${exp}&sig=${new URL(cover).searchParams.get('sig')}`,
    'one character of the signature': with_(thumbnail, 'sig', flipped),
    'a short signature': with_(thumbnail, 'sig', sig.slice(0, 42)),
    'a long signature': with_(thumbnail, 'sig', `${sig}A`),
    'an expiry that is not a number': with_(thumbnail, 'exp', 'soon'),
    'no expiry': `${ORIGIN}${thumbPath(ADULT)}?sig=${sig}`,
    'no signature': `${ORIGIN}${thumbPath(ADULT)}?exp=${exp}`,
  };
  for (const [what, url] of Object.entries(cases)) refusedImage(await get(env, url), 'adult_image_unsigned', what);
  assert.equal((await get(env, thumbnail)).status, 200, 'control: the URL as given');

  // A properly signed URL with an expiry further off than any this Worker issues is refused (a key that leaked cannot mint
  // a long-lived link that this check would honour).
  const far = await signedImageUrl(env, ORIGIN, thumbPath(ADULT), Date.now() + 24 * 3600 * 1000);
  assert.ok(isSigned(far));
  refusedImage(await get(env, far), 'adult_image_unsigned', 'a day ahead');
});

test('the key is HMAC(JWT_SECRET, "adult-image-url-v1"), and the signature an HMAC of the path and the expiry under it', async () => {
  const env = await environment();
  const url = new URL(await signedImageUrl(env, ORIGIN, thumbPath(ADULT)));
  const key = crypto.createHmac('sha256', JWT_SECRET).update('adult-image-url-v1').digest();
  const sig = crypto.createHmac('sha256', key).update(`${thumbPath(ADULT)}\n${url.searchParams.get('exp')}`).digest('base64url');
  assert.equal(url.searchParams.get('sig'), sig);
  const direct = crypto.createHmac('sha256', JWT_SECRET).update(`${thumbPath(ADULT)}\n${url.searchParams.get('exp')}`).digest('base64url');
  assert.notEqual(url.searchParams.get('sig'), direct, 'never the secret itself');
});

test('with no JWT_SECRET nothing is signed and every adult image is refused', async () => {
  const env = await environment();
  const signed = await signedImageUrl(env, ORIGIN, thumbPath(ADULT));
  const bare = { ...env, JWT_SECRET: undefined };
  assert.equal(await signedImageUrl(bare, ORIGIN, thumbPath(ADULT)), null);
  assert.equal(await signedThumbnailUrl(bare, ORIGIN, JSON.parse(await env.SOCIAL.get(`sim:${ADULT}`))), null);
  assert.deepEqual(await verifyImageUrl(bare, signed), { ok: false, code: 'adult_image_unsigned' });
  refusedImage(await get(bare, signed), 'adult_image_unsigned', 'no secret');
  assert.equal((await get(bare, thumbPath(KIDS))).status, 200, 'other listings are not affected');
});

test('a listing that is not adult content keeps its public URLs and its public caching exactly', async () => {
  const env = await environment();
  for (const who of [undefined, 'off', 'on', 'author']) {
    const page = await get(env, `/api/simulations/${KIDS}`, who);
    assert.equal(page.body.thumbnail_url, `${ORIGIN}${thumbPath(KIDS)}`);
    assert.equal(page.body.cover_url, `${ORIGIN}${coverPath(KIDS)}?v=${Date.parse(COVER_AT)}`);
    const frames = await get(env, `/api/simulations/${KIDS}/previews`, who);
    assert.deepEqual(frames.body.previews, [{ n: 0, url: `${ORIGIN}${framePath(KIDS)}` }]);
    assert.equal(frames.headers.get('Cache-Control'), 'public, max-age=60');
  }
  for (const [path, bytes, cache] of [[thumbPath(KIDS), THUMB, 'public, max-age=3600'], [coverPath(KIDS), COVER, 'public, max-age=300'], [framePath(KIDS), FRAME, 'public, max-age=300']]) {
    const r = await get(env, path);
    assert.equal(r.status, 200, path);
    assert.ok(same(r.bytes, bytes));
    assert.equal(r.headers.get('Cache-Control'), cache, path);
  }
});

// ── the listing responses ────────────────────────────────────────────────────

test('an opted-in viewer and the author get signed URLs in the fields they already read, on every listing response', async () => {
  const env = await environment();
  assert.equal((await call(env, 'POST', `/api/simulations/${ADULT}/live`, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } })).status, 200);
  const kidsCard = (await get(env, '/api/simulations', 'on')).body.simulations.find((s) => s.id === KIDS);

  const cards = [];
  for (const path of ['/api/simulations', '/api/gallery']) cards.push([`${path} on`, (await get(env, path, 'on')).body.simulations.find((s) => s.id === ADULT)]);
  cards.push(['page on', (await get(env, `/api/simulations/${ADULT}`, 'on')).body]);
  cards.push(['page author', (await get(env, `/api/simulations/${ADULT}`, 'author')).body]);
  cards.push(['live strip on', (await get(env, '/api/simulations/live', 'on')).body.sessions.find((s) => s.id === ADULT)]);
  cards.push(['projects author', (await get(env, '/api/projects', 'author')).body.projects.find((p) => p.id === ADULT)]);
  cards.push(['unlisted page author', (await get(env, `/api/simulations/${PENDING_ADULT}`, 'author')).body]);
  for (const [what, card] of cards) {
    assert.ok(card, what);
    assert.ok(isSigned(card.thumbnail_url), `${what}: ${card.thumbnail_url}`);
    assert.ok(isSigned(card.cover_url), `${what}: ${card.cover_url}`);
    assert.ok(!Object.keys(card).some((k) => /sign|exp/i.test(k)), `${what}: no new field`);
  }
  assert.deepEqual(Object.keys(cards[0][1]).sort(), Object.keys(kidsCard).sort(), 'the same fields as any other card');
  for (const who of ['on', 'author']) {
    const frames = (await get(env, `/api/simulations/${ADULT}/previews`, who)).body.previews;
    assert.deepEqual(frames.map((f) => f.n), [0]);
    assert.ok(isSigned(frames[0].url), `${who}: ${frames[0].url}`);
  }
  // And what they are given loads.
  for (const [what, card] of cards.slice(0, 6)) assert.equal((await get(env, card.thumbnail_url)).status, 200, what);
});

test('a viewer who may not see an adult listing is never given a signed URL', async () => {
  const env = await environment();
  await call(env, 'POST', `/api/simulations/${ADULT}/live`, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } });
  const paths = ['/api/simulations', '/api/gallery', '/api/gallery/featured', '/api/simulations/live', '/api/simulations/tags', '/api/makers', '/api/projects',
    ...[ADULT, UNRATED, PENDING_ADULT, KIDS].flatMap((id) => [`/api/simulations/${id}`, `/api/simulations/${id}/previews`])];
  for (const who of [undefined, 'off', 'admin']) {
    for (const path of paths) {
      const r = await get(env, path, who);
      assert.ok(!/[?&]sig=/.test(r.text), `${who || 'signed out'} ${path}: ${r.text.slice(0, 200)}`);
    }
  }
  // An administrator reads an unlisted adult listing's record, unsigned while their own setting is off, signed once it is on.
  const unsigned = (await get(env, `/api/simulations/${PENDING_ADULT}`, 'admin')).body;
  assert.equal(unsigned.thumbnail_url, `${ORIGIN}${thumbPath(PENDING_ADULT)}`);
  await env.USERS.put(adultContentKey(ADMIN), '1');
  assert.ok(isSigned((await get(env, `/api/simulations/${PENDING_ADULT}`, 'admin')).body.thumbnail_url));
});

test('an adult_21 listing\'s images are signed only for a viewer whose verified age is 21 or over, and for its author', async () => {
  const env = await environment();
  await listing(env, ADULT21, 'adult_21');
  await call(env, 'POST', `/api/simulations/${ADULT21}/live`, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } });

  // Setting on and 21 or over: signed URLs that load.
  const urls = await urlsFor(env, ADULT21, 'on');
  for (const [what, url] of Object.entries(urls)) {
    assert.ok(isSigned(url), `${what}: ${url}`);
    assert.equal((await get(env, url)).status, 200, what);
  }
  assert.ok(isSigned((await get(env, `/api/simulations/${ADULT21}`, 'author')).body.thumbnail_url), 'its author, setting off');

  // Setting on and 19: refused with the 21+ reason, and no response carries a signed URL for it.
  for (const path of [`/api/simulations/${ADULT21}`, `/api/simulations/${ADULT21}/previews`]) {
    const r = await get(env, path, 'young');
    assert.equal(r.status, 403, `${path}: ${r.text}`);
    assert.equal(r.body.code, 'age_21_required', path);
  }
  for (const path of ['/api/simulations', '/api/gallery', '/api/simulations/live', '/api/projects']) {
    const r = await get(env, path, 'young');
    assert.ok(!r.text.includes(ADULT21), `young ${path}: ${r.text.slice(0, 200)}`);
  }
  // The same viewer is still given adult_18 images: the setting is on.
  assert.ok(isSigned((await get(env, `/api/simulations/${ADULT}`, 'young')).body.thumbnail_url));
  // And the image routes themselves still want the signature, from anyone.
  for (const who of [undefined, 'young', 'on', 'author']) refusedImage(await get(env, thumbPath(ADULT21), who), 'adult_image_unsigned', `${who || 'signed out'} unsigned`);
});

// ── the exports Website calls ────────────────────────────────────────────────

test('signedThumbnailUrl and mayView: what a purchase record may carry', async () => {
  const env = await environment();
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${ADULT}`));
  assert.equal(mayView(sim, { accountId: ON, signedIn: true, adult: true }), true);
  assert.equal(mayView(sim, { accountId: AUTHOR, signedIn: true, adult: false }), true);
  assert.equal(mayView(sim, { accountId: OFF, signedIn: true, adult: false }), false);
  const url = await signedThumbnailUrl(env, ORIGIN, sim);
  assert.ok(url.startsWith(`${ORIGIN}${thumbPath(ADULT)}?exp=`), url);
  assert.equal((await get(env, url)).status, 200);
  assert.equal(await signedThumbnailUrl(env, ORIGIN, { ...sim, thumbnail_url: null }), null, 'no thumbnail, no URL');
  assert.equal(await signedImageUrl(env, ORIGIN, '/api/simulations/x/world/manifest'), null, 'only an image path is ever signed');
  assert.equal(await signedImageUrl(env, ORIGIN, `/api/simulations/${ADULT}/preview/8`), null);
});
