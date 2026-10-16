// Adult content (src/adult_content.mjs; McKale, 2026-10-08): a listing rated adult_18 or adult_21, or one with no rating, is
// invisible and unplayable for every account until that account turns Adult content on in Profile. Off by default; signed out
// is off. An adult_21 listing (alcohol, tobacco, psychedelic use; Guardian Code v2.2 Part V), and one with no rating, which
// reads as it, also needs a verified age of 21 or over. Turning it on needs a verified adult (requireVerifiedAdult); turning it
// off is always allowed. The author always sees and plays their own listing. Every surface that serves a listing or its
// content is checked here against the real Worker, with in-memory storage and the real Durable Object classes; cached answers
// stay one per rating slice and are never handed to a viewer whose slice is lower than the one they were computed for.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { installCache } from './helpers/fake_cache.mjs';
import { LiveHost } from '../src/live.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { manifestKey, chunkKey } from '../src/world.mjs';
import { routePolicy, SESSION } from '../src/route_policy.mjs';
import {
  ratingOf, ratingCap, mayView, viewerFor, age21Eligible, ageAt, resolveDob, adultContentKey, ADULT_CONTENT_OFF, AGE_21_REQUIRED, SIGNED_OUT,
  DOB_CACHE_MS,
} from '../src/adult_content.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111'; // the author of every listing below; Adult content off
const ON = '22222222-2222-4222-8222-222222222222'; // turned Adult content on; a verified age of 30
const OFF = '33333333-3333-4333-8333-333333333333'; // signed in, never turned it on
const LONER = '55555555-5555-4555-8555-555555555555'; // a maker whose only listing is adult content
const YOUNG = '77777777-7777-4777-8777-777777777777'; // turned Adult content on; a verified age of 19
const tokens = { author: mintToken(AUTHOR), on: mintToken(ON), off: mintToken(OFF), loner: mintToken(LONER), young: mintToken(YOUNG) };

/// A date of birth `years` whole years before today (UTC), moved by `days`. Used only where today's date is not Feb 29.
function yearsBefore(years, days = 0, now = new Date()) {
  const d = new Date(Date.UTC(now.getUTCFullYear() - years, now.getUTCMonth(), now.getUTCDate() + days));
  return d.toISOString().slice(0, 10);
}
const bornJan1 = (age) => `${new Date().getUTCFullYear() - age}-01-01`;
/// An identity record as requireVerifiedAdult leaves a verified adult's, with the date read off the document.
const verified = (env, id, dob) => env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify({ status: 'verified', iso2: 'US', age_verified: true, extracted_dob: dob }));

const KIDS = 'aaaaaaaa-0000-4000-8000-000000000001';
const MATURE = 'aaaaaaaa-0000-4000-8000-000000000002';
const ADULT = 'aaaaaaaa-0000-4000-8000-000000000003';
const UNRATED = 'aaaaaaaa-0000-4000-8000-000000000004';
const ADULT_PAK = 'aaaaaaaa-0000-4000-8000-000000000005';
const LONE_ADULT = 'aaaaaaaa-0000-4000-8000-000000000006';
const ADULT_IDS = [ADULT, UNRATED, ADULT_PAK, LONE_ADULT];

const H = 'c'.repeat(64);
const WORLD_SHA = 'f'.repeat(64);
const SOURCE_SHA = 'e'.repeat(64);
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const LINK = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${PIN}`;
const ORIGIN = 'https://api.eustress.dev';

/// A listed listing by `author`. `rating` undefined writes none. A world (manifest, one chunk, a shared source) unless `pak`.
async function listing(env, id, rating, { author = AUTHOR, authorName = 'maker', pak = false, featured = false } = {}) {
  const sim = {
    id, name: `World ${id.slice(-1)}`, description: 'a place', genre: 'all', author_id: author, author_name: authorName, is_public: true,
    max_players: 8, tags: ['shared', `${(rating || 'unrated').replace('_', '-')}-tag`], play_count: 0, favorite_count: 0, version: 1,
    published_at: `2026-03-0${id.slice(-1)}T00:00:00.000Z`, updated_at: '2026-03-01T00:00:00.000Z',
    moderation: { status: 'approved', ...(rating ? { rating } : {}), ...(featured ? { featured: true } : {}) },
  };
  if (pak) {
    sim.format = 'pak';
    sim.r2_key = `universes/${id}/universe.pak`;
    await env.SCENES.put(sim.r2_key, 'the world');
  } else {
    const shape = JSON.stringify({ format: 'echk', spaces: [{ name: 'Main', chunks: [{ blake3: H }] }], assets: [] });
    sim.format = 'echk';
    sim.open_source = true;
    sim.world = { manifest_key: manifestKey(id, WORLD_SHA), manifest_sha256: WORLD_SHA, source_manifest_key: manifestKey(id, SOURCE_SHA), source_manifest_sha256: SOURCE_SHA };
    sim.r2_key = sim.world.manifest_key;
    await env.SCENES.put(sim.world.manifest_key, shape);
    await env.SCENES.put(sim.world.source_manifest_key, shape);
    await env.SCENES.put(chunkKey(id, H), 'ECHK chunk bytes');
  }
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${author}:${id}`, id);
}

async function environment() {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  for (const [id, username] of [[AUTHOR, 'maker'], [ON, 'ada'], [OFF, 'bob'], [LONER, 'loner'], [YOUNG, 'cy']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  // The setting as the route leaves it when a verified adult turns it on.
  await env.USERS.put(adultContentKey(ON), '1');
  await verified(env, ON, bornJan1(30));
  await env.USERS.put(adultContentKey(YOUNG), '1');
  await verified(env, YOUNG, bornJan1(19));
  await listing(env, KIDS, 'all_ages', { featured: true });
  await listing(env, MATURE, 'mature_17');
  await listing(env, ADULT, 'adult_18', { featured: true });
  await listing(env, UNRATED, undefined, { featured: true });
  await listing(env, ADULT_PAK, 'adult_18', { pak: true });
  await listing(env, LONE_ADULT, 'adult_18', { author: LONER, authorName: 'loner' });
  return env;
}

async function call(env, method, path, { who, body, headers = {} } = {}) {
  const h = { ...headers };
  if (who) h.Authorization = `Bearer ${tokens[who] || who}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body: parsed };
}

const ids = (r) => (r.body?.simulations || r.body?.featured || r.body?.sessions || []).map((s) => s.id).sort();
const VIEWERS = { out: undefined, off: 'off', on: 'on', author: 'author' };

/// The refusal every gated surface gives.
function refused(r, signedIn, what) {
  assert.equal(r.status, 403, `${what}: ${r.text}`);
  assert.deepEqual(r.body, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: signedIn }, what);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store', `${what}: a refusal is never cached`);
}

const beat = (env, simId) => call(env, 'POST', `/api/simulations/${simId}/live`, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } });

// ── the rule itself ──────────────────────────────────────────────────────────

test('a listing with no rating, or a rating this Worker does not know, is adult content in the most restrictive band', () => {
  for (const r of ['all_ages', 'ages_10', 'teen_13', 'mature_17', 'adult_18', 'adult_21']) assert.equal(ratingOf({ moderation: { rating: r } }), r);
  for (const sim of [{}, { moderation: {} }, { moderation: { rating: null } }, { moderation: { rating: 'ADULT_18' } }, { moderation: { rating: 'adult_25' } }, { moderation: { rating: 'pg' } }, null]) {
    assert.equal(ratingOf(sim), 'adult_21', JSON.stringify(sim));
  }
});

test('the slice: off caps at mature_17 whatever is asked, on and under 21 at adult_18, on and 21 or over takes what is asked; rating_max narrows and never widens', () => {
  const on = { accountId: ON, signedIn: true, adult: true, age21: true };
  const young = { accountId: YOUNG, signedIn: true, adult: true, age21: false };
  const off = { accountId: OFF, signedIn: true, adult: false, age21: false };
  for (const viewer of [off, SIGNED_OUT, undefined, { accountId: OFF, signedIn: true, adult: false, age21: true }]) {
    assert.equal(ratingCap(null, viewer), 'mature_17');
    assert.equal(ratingCap('adult_18', viewer), 'mature_17');
    assert.equal(ratingCap('adult_21', viewer), 'mature_17');
    assert.equal(ratingCap('nonsense', viewer), 'mature_17');
    assert.equal(ratingCap('teen_13', viewer), 'teen_13');
  }
  for (const viewer of [young, { accountId: YOUNG, signedIn: true, adult: true }]) {
    assert.equal(ratingCap(null, viewer), 'adult_18');
    assert.equal(ratingCap('adult_21', viewer), 'adult_18');
    assert.equal(ratingCap('nonsense', viewer), 'adult_18');
    assert.equal(ratingCap('mature_17', viewer), 'mature_17');
  }
  assert.equal(ratingCap(null, on), 'adult_21');
  assert.equal(ratingCap('adult_21', on), 'adult_21');
  assert.equal(ratingCap('adult_18', on), 'adult_18');
  assert.equal(ratingCap('all_ages', on), 'all_ages');
  const adult = { author_id: AUTHOR, moderation: { rating: 'adult_18' } };
  const adult21 = { author_id: AUTHOR, moderation: { rating: 'adult_21' } };
  assert.equal(mayView(adult, on), true);
  assert.equal(mayView(adult, young), true, 'adult_18 needs the setting only');
  assert.equal(mayView(adult, off), false);
  assert.equal(mayView(adult, SIGNED_OUT), false);
  assert.equal(mayView(adult21, on), true);
  assert.equal(mayView(adult21, young), false, 'adult_21 needs a verified age of 21 or over');
  assert.equal(mayView(adult21, off), false);
  assert.equal(mayView(adult21, { ...off, age21: true }), false, 'an age of 21 without the setting is still off');
  assert.equal(mayView({ author_id: AUTHOR }, young), false, 'no rating reads as adult_21');
  assert.equal(mayView(adult, { accountId: AUTHOR, signedIn: true, adult: false }), true, 'the author always sees their own');
  assert.equal(mayView(adult21, { accountId: AUTHOR, signedIn: true, adult: false }), true, 'the author always sees their own');
  assert.equal(mayView({ author_id: AUTHOR, moderation: { rating: 'mature_17' } }, SIGNED_OUT), true);
});

// ── the setting ──────────────────────────────────────────────────────────────

test('the setting route is a session route, named in the policy table', () => {
  const r = routePolicy('POST', '/api/account/adult-content');
  assert.equal(r.class, SESSION);
  assert.equal(r.explicit, true);
});

test('Adult content is off by default, and turning it on is refused without a verified adult', async () => {
  const env = await environment();
  assert.equal((await call(env, 'GET', '/api/account/adult-content')).status, 401);
  assert.equal((await call(env, 'POST', '/api/account/adult-content', { body: { enabled: true } })).status, 401);

  const seen = await call(env, 'GET', '/api/account/adult-content', { who: 'off' });
  assert.equal(seen.status, 200);
  assert.equal(seen.body.enabled, false, 'off by default');
  assert.equal(seen.body.can_enable, false);
  assert.equal(seen.body.code, 'kyc_required');
  assert.equal(typeof seen.body.reason, 'string');
  assert.equal(seen.headers.get('Cache-Control'), 'private, no-store');

  const none = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } });
  assert.equal(none.status, 403, none.text);
  assert.equal(none.body.code, 'kyc_required');
  assert.equal(typeof none.body.reason, 'string');
  assert.equal(await env.USERS.get(adultContentKey(OFF)), null, 'nothing was written');

  await env.KYC_STATUS.put(`kyc-${OFF}-front`, JSON.stringify({ status: 'pending' }));
  const pending = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } });
  assert.equal(pending.status, 403);
  assert.equal(pending.body.code, 'kyc_incomplete');

  const fifteen = `${new Date().getUTCFullYear() - 15}-01-01`;
  await env.KYC_STATUS.put(`kyc-${OFF}-front`, JSON.stringify({ status: 'verified', iso2: 'US', extracted_dob: fifteen }));
  const young = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } });
  assert.equal(young.status, 403);
  assert.equal(young.body.code, 'underage');
  assert.equal(await env.USERS.get(adultContentKey(OFF)), null);
  // The verification check words its refusals for payouts; this setting's refusals never mention them.
  for (const r of [seen, none, pending, young]) assert.doesNotMatch(r.body.reason, /payout/i, r.body.reason);

  for (const bad of [{}, { enabled: 'yes' }, { enabled: 1 }, { enabled: null }]) {
    assert.equal((await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: bad })).status, 400, JSON.stringify(bad));
  }
});

test('a verified adult turns it on; asking again writes nothing', async () => {
  const env = await environment();
  await env.KYC_STATUS.put(`kyc-${OFF}-front`, JSON.stringify({ status: 'verified', age_verified: true }));
  // Verified, with no date of birth on record: an adult, but not shown to be 21.
  const before = await call(env, 'GET', '/api/account/adult-content', { who: 'off' });
  assert.deepEqual(before.body, { enabled: false, can_enable: true, age_21_eligible: false });

  const on = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } });
  assert.equal(on.status, 200, on.text);
  assert.deepEqual(on.body, { ok: true, enabled: true, changed: true });
  assert.equal(await env.USERS.get(adultContentKey(OFF)), '1');
  assert.deepEqual((await call(env, 'GET', '/api/account/adult-content', { who: 'off' })).body, { enabled: true, can_enable: true, age_21_eligible: false });

  const writes = [];
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (k, v, o) => { writes.push(k); return realPut(k, v, o); };
  const again = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } });
  assert.deepEqual(again.body, { ok: true, enabled: true, changed: false });
  assert.deepEqual(writes.filter((k) => k.startsWith('adult-content:')), [], 'no write when nothing changes');

  // And the account sees adult content from the next request.
  assert.equal((await call(env, 'GET', `/api/simulations/${ADULT}`, { who: 'off' })).status, 200);
});

test('turning it off is always allowed, verified or not, and turning it off again writes nothing', async () => {
  const env = await environment();
  // ON has the setting and, once this record is gone, no identity record at all: off is never gated.
  await env.KYC_STATUS.delete(`kyc-${ON}-front`);
  const off = await call(env, 'POST', '/api/account/adult-content', { who: 'on', body: { enabled: false } });
  assert.equal(off.status, 200, off.text);
  assert.deepEqual(off.body, { ok: true, enabled: false, changed: true });
  assert.equal(await env.USERS.get(adultContentKey(ON)), null);
  refused(await call(env, 'GET', `/api/simulations/${ADULT}`, { who: 'on' }), true, 'off at once');

  const touched = [];
  const realPut = env.USERS.put.bind(env.USERS);
  const realDelete = env.USERS.delete.bind(env.USERS);
  env.USERS.put = async (k, v, o) => { touched.push(k); return realPut(k, v, o); };
  env.USERS.delete = async (k) => { touched.push(k); return realDelete(k); };
  const again = await call(env, 'POST', '/api/account/adult-content', { who: 'on', body: { enabled: false } });
  assert.deepEqual(again.body, { ok: true, enabled: false, changed: false });
  const never = await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: false } });
  assert.deepEqual(never.body, { ok: true, enabled: false, changed: false });
  assert.deepEqual(touched.filter((k) => k.startsWith('adult-content:')), [], 'idempotent: nothing written or deleted');
});

// ── the lists ────────────────────────────────────────────────────────────────

test('the Gallery list hides adult content by default, signed out and signed in, and shows it to a viewer who turned it on', async () => {
  const env = await environment();
  const everyone = [KIDS, MATURE, ADULT, UNRATED, ADULT_PAK, LONE_ADULT].sort();
  const safe = [KIDS, MATURE].sort();
  for (const path of ['/api/simulations', '/api/gallery']) {
    assert.deepEqual(ids(await call(env, 'GET', path)), safe, `${path} signed out`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'off' })), safe, `${path} off`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'author' })), safe, `${path}: the browse list follows the setting, the author's too`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'on' })), everyone, `${path} on`);
  }
  // Search, tags and genre are the same list, cut the same way.
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?q=world')), safe);
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?tag=adult-18-tag', { who: 'off' })), []);
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?tag=adult-18-tag', { who: 'on' })), [ADULT, ADULT_PAK, LONE_ADULT].sort());
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?genre=all', { who: 'off' })), safe);
});

test('rating_max narrows the list and never widens it past the setting', async () => {
  const env = await environment();
  for (const who of [undefined, 'off']) {
    for (const asked of ['adult_18', 'ADULT_18', 'nonsense', '']) {
      assert.deepEqual(ids(await call(env, 'GET', `/api/simulations?rating_max=${asked}`, { who })), [KIDS, MATURE].sort(), `${who || 'signed out'} ${asked}`);
    }
    assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=all_ages', { who })), [KIDS]);
  }
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=mature_17', { who: 'on' })), [KIDS, MATURE].sort());
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=all_ages', { who: 'on' })), [KIDS]);
});

test('ages_10 (Guardian Code v2.1) is a band like the others: shown with its setting off, ordered between all_ages and teen_13 by rating_max', async () => {
  const env = await environment();
  const TEN = 'aaaaaaaa-0000-4000-8000-000000000007';
  await listing(env, TEN, 'ages_10');
  for (const who of [undefined, 'off', 'on']) {
    assert.ok(ids(await call(env, 'GET', '/api/simulations', { who })).includes(TEN), `${who || 'signed out'}: not adult content, so not gated`);
    assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=all_ages', { who })), [KIDS], 'all_ages leaves it out');
    assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=ages_10', { who })), [KIDS, TEN].sort(), 'ages_10 keeps it and drops mature_17');
  }
  assert.equal((await call(env, 'GET', `/api/simulations/${TEN}`)).status, 200, 'its page is served signed out');
});

test('the featured shelf never carries adult content, or a listing with no rating, even for a viewer who turned it on', async () => {
  const env = await environment();
  for (const who of [undefined, 'off', 'on']) assert.deepEqual(ids(await call(env, 'GET', '/api/gallery/featured', { who })), [KIDS], String(who));
});

test('the live-now strip, the makers and the popular tags follow the setting', async () => {
  const env = await environment();
  assert.equal((await beat(env, KIDS)).status, 200);
  assert.equal((await beat(env, ADULT)).status, 200);

  for (const who of [undefined, 'off', 'author']) {
    const live = await call(env, 'GET', '/api/simulations/live', { who });
    assert.deepEqual(ids(live), [KIDS], `live, ${who || 'signed out'}`);
    assert.equal(live.body.sessions_total, 1, 'nor counted');
    assert.deepEqual((await call(env, 'GET', '/api/simulations/live?rating_max=adult_18', { who })).body.sessions_total, 1);
  }
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations/live', { who: 'on' })), [ADULT, KIDS].sort());

  const makers = async (who, q = '') => (await call(env, 'GET', `/api/makers${q}`, { who })).body.makers.map((m) => m.username).sort();
  assert.deepEqual(await makers(undefined), ['maker']);
  assert.deepEqual(await makers('off', '?rating_max=adult_18'), ['maker'], 'a maker whose only Space is adult content is not named');
  assert.deepEqual(await makers('on'), ['loner', 'maker']);

  const tags = async (who, q = '') => (await call(env, 'GET', `/api/simulations/tags${q}`, { who })).body.tags.map((t) => t.tag);
  for (const who of [undefined, 'off']) {
    const named = await tags(who, '?rating_max=adult_18');
    assert.ok(!named.includes('adult-18-tag') && !named.includes('unrated-tag'), `${who || 'signed out'}: ${named}`);
    assert.ok(named.includes('all-ages-tag'));
  }
  const all = await tags('on');
  assert.ok(all.includes('adult-18-tag') && all.includes('unrated-tag'));
  assert.equal((await call(env, 'GET', '/api/simulations/tags', { who: 'on' })).body.tags.find((t) => t.tag === 'shared').count, 6);
});

test('an answer computed for a signed-in viewer is never cached; the signed-out one keeps its caching and varies on Authorization', async () => {
  const env = await environment();
  await beat(env, ADULT);
  for (const path of ['/api/simulations', '/api/simulations/tags', '/api/simulations/live', '/api/makers']) {
    for (const who of ['on', 'off']) {
      const r = await call(env, 'GET', path, { who });
      assert.equal(r.headers.get('Cache-Control'), 'private, no-store', `${path} ${who}`);
      assert.match(r.headers.get('Vary'), /\bAuthorization\b/, `${path} ${who}`);
    }
    const out = await call(env, 'GET', path);
    assert.notEqual(out.headers.get('Cache-Control'), 'private, no-store', `${path} signed out keeps its caching`);
    assert.equal(out.headers.get('Vary'), 'Origin, Authorization', path);
  }
  assert.equal((await call(env, 'GET', '/api/simulations/tags')).headers.get('Cache-Control'), 'public, max-age=300');
  assert.equal((await call(env, 'GET', '/api/makers')).headers.get('Cache-Control'), 'public, max-age=300');
  assert.equal((await call(env, 'GET', '/api/simulations/live')).headers.get('Cache-Control'), 'public, max-age=10');
});

test('a cached slice computed for a viewer who turned it on is never served to a viewer who did not', async () => {
  const env = await environment();
  await beat(env, KIDS);
  await beat(env, ADULT);
  const { cache, restore } = installCache();
  try {
    // The opted-in viewer asks first, so the platform's cache holds the strip and the makers with adult content in them.
    const onLive = await call(env, 'GET', '/api/simulations/live', { who: 'on' });
    assert.deepEqual(ids(onLive), [ADULT, KIDS].sort());
    const onMakers = await call(env, 'GET', '/api/makers', { who: 'on' });
    assert.ok(onMakers.text.includes('loner'));
    const held = cache.store.size;
    assert.ok(held >= 2);

    // The same URLs, inside the cache window, from a viewer whose setting is off, signed in or not.
    for (const who of ['off', undefined, 'author']) {
      for (const query of ['', '?rating_max=adult_18', '?rating_max=nonsense']) {
        const live = await call(env, 'GET', `/api/simulations/live${query}`, { who });
        assert.deepEqual(ids(live), [KIDS], `live${query}, ${who || 'signed out'}`);
        assert.ok(!live.text.includes(ADULT));
        const makers = await call(env, 'GET', `/api/makers${query}`, { who });
        assert.ok(!makers.text.includes('loner'), `makers${query}, ${who || 'signed out'}`);
      }
    }
    // One entry per slice, never one per account: the off viewers share one more entry for each route.
    assert.equal(cache.store.size, held + 2);
    assert.ok([...cache.store.keys()].every((k) => /rating_max=(all_ages|teen_13|mature_17|adult_18|adult_21)$/.test(k)), [...cache.store.keys()].join(' '));
    assert.ok(![...cache.store.keys()].some((k) => k.includes(ON) || k.includes(OFF)), 'no account in a key');
  } finally { restore(); }
});

// ── one listing ──────────────────────────────────────────────────────────────

test('the listing page: 403 with the reason for a viewer who is off, 200 for a viewer who turned it on, and for its author', async () => {
  const env = await environment();
  for (const id of [ADULT, UNRATED]) {
    refused(await call(env, 'GET', `/api/simulations/${id}`), false, `signed out ${id}`);
    refused(await call(env, 'GET', `/api/simulations/${id}`, { who: 'off' }), true, `off ${id}`);
    const on = await call(env, 'GET', `/api/simulations/${id}`, { who: 'on' });
    assert.equal(on.status, 200, on.text);
    assert.equal(on.body.id, id);
    assert.equal(on.headers.get('Cache-Control'), 'private, no-store', 'an adult listing is never cached');
    const own = await call(env, 'GET', `/api/simulations/${id}`, { who: 'author' });
    assert.equal(own.status, 200, 'the author, whose setting is off, sees their own');
  }
  const kids = await call(env, 'GET', `/api/simulations/${KIDS}`);
  assert.equal(kids.status, 200);
  assert.equal(kids.headers.get('Cache-Control'), null, 'any other listing is as it was');
});

test('the .pak download, the play route and a counted play are refused to a viewer who is off', async () => {
  const env = await environment();
  refused(await call(env, 'GET', `/api/simulations/${ADULT_PAK}/download`), false, 'download signed out');
  refused(await call(env, 'GET', `/api/simulations/${ADULT_PAK}/download`, { who: 'off' }), true, 'download off');
  for (const who of ['on', 'author']) {
    const got = await call(env, 'GET', `/api/simulations/${ADULT_PAK}/download`, { who });
    assert.equal(got.status, 200, `${who}: ${got.text}`);
    assert.equal(got.text, 'the world');
    assert.equal(got.headers.get('Cache-Control'), 'private, no-store');
  }
  assert.equal((await call(env, 'GET', `/api/simulations/${KIDS}/download`)).status, 409, 'a world listing still answers 409 to anyone');

  refused(await call(env, 'POST', `/api/simulations/${ADULT}/play`), false, 'play signed out');
  refused(await call(env, 'POST', `/api/simulations/${ADULT}/play`, { who: 'off' }), true, 'play off');
  assert.equal((await call(env, 'POST', `/api/simulations/${ADULT}/play`, { who: 'on' })).status, 200);
  assert.equal((await call(env, 'POST', `/api/simulations/${ADULT}/play`, { who: 'author' })).status, 200);
  assert.equal((await call(env, 'POST', `/api/simulations/${KIDS}/play`)).status, 200);

  refused(await call(env, 'POST', '/api/social/play', { who: 'off', body: { simulation_id: ADULT } }), true, 'social play off');
  assert.equal(await env.SOCIAL.get(`plays:${ADULT}`), null, 'nothing was counted');
  assert.equal((await call(env, 'POST', '/api/social/play', { who: 'on', body: { simulation_id: ADULT } })).status, 200);
});

test('the world manifest, its chunks and the source world are refused to a viewer who is off', async () => {
  const env = await environment();
  for (const route of ['world/manifest', `world/chunks/${H}`, 'world/source']) {
    refused(await call(env, 'GET', `/api/simulations/${ADULT}/${route}`), false, `${route} signed out`);
    refused(await call(env, 'GET', `/api/simulations/${ADULT}/${route}`, { who: 'off' }), true, `${route} off`);
    for (const who of ['on', 'author']) assert.equal((await call(env, 'GET', `/api/simulations/${ADULT}/${route}`, { who })).status, 200, `${route} ${who}`);
    assert.equal((await call(env, 'GET', `/api/simulations/${KIDS}/${route}`)).status, 200, `${route} of an all-ages listing, signed out`);
  }
});

test('the listing\'s live read and its preview list are refused to a viewer who is off', async () => {
  const env = await environment();
  await beat(env, ADULT);
  refused(await call(env, 'GET', `/api/simulations/${ADULT}/live`), false, 'live signed out');
  refused(await call(env, 'GET', `/api/simulations/${ADULT}/live`, { who: 'off' }), true, 'live off');
  const on = await call(env, 'GET', `/api/simulations/${ADULT}/live`, { who: 'on' });
  assert.equal(on.status, 200);
  assert.equal(on.body.link, LINK);

  refused(await call(env, 'GET', `/api/simulations/${ADULT}/previews`), false, 'previews signed out');
  refused(await call(env, 'GET', `/api/simulations/${ADULT}/previews`, { who: 'off' }), true, 'previews off');
  const previews = await call(env, 'GET', `/api/simulations/${ADULT}/previews`, { who: 'on' });
  assert.equal(previews.status, 200);
  assert.equal(previews.headers.get('Cache-Control'), 'private, no-store');
  assert.equal((await call(env, 'GET', `/api/simulations/${KIDS}/previews`)).headers.get('Cache-Control'), 'public, max-age=60');
});

test('a hosted session of adult content admits a joiner who turned it on, and the author; a refused ticket is not spent', async () => {
  const env = await environment();
  assert.equal((await beat(env, ADULT)).status, 200);
  const ticketFor = async (who) => {
    const r = await call(env, 'POST', '/api/identity/ticket', { who, body: { audience: PIN } });
    assert.equal(r.status, 200, r.text);
    return r.body.ticket;
  };
  const admit = (ticket) => call(env, 'POST', `/api/simulations/${ADULT}/live/admit`, { who: 'author', body: { ticket, audience: PIN } });

  const offTicket = await ticketFor('off');
  const r = await admit(offTicket);
  assert.equal(r.status, 403, r.text);
  assert.deepEqual(r.body, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: true });

  const on = await admit(await ticketFor('on'));
  assert.equal(on.status, 200, on.text);
  assert.equal(on.body.account_id, ON);
  const own = await admit(await ticketFor('author'));
  assert.equal(own.status, 200, 'the author joins their own session with the setting off');

  // The joiner turns it on, as a verified adult; the ticket refused above was never spent.
  await env.KYC_STATUS.put(`kyc-${OFF}-front`, JSON.stringify({ status: 'verified', age_verified: true }));
  assert.equal((await call(env, 'POST', '/api/account/adult-content', { who: 'off', body: { enabled: true } })).status, 200);
  const later = await admit(offTicket);
  assert.equal(later.status, 200, later.text);
  assert.equal(later.body.account_id, OFF);

  // A ticket that is not genuine is refused by the object in its own words.
  assert.equal((await admit('pw1.not-a-ticket.at-all')).body.code, 'invalid');
});

// The catalog read is not gated: the Player asks for it in game with no session, so a gate would close every shop in an
// adult listing to the players who may play it (src/commerce.mjs). A live purchase always carries a session, and is gated.
test('what an adult listing sells is sold only to an account that turned it on', async () => {
  const env = await environment();
  const buy = (who) => call(env, 'POST', '/api/commerce/purchases', {
    who, headers: { 'Eustress-Mode': 'live' },
    body: { idempotency_key: `buy-${who}-00000001`, product: `prod_${'a'.repeat(24)}`, sim_id: ADULT, expected_price: 10 },
  });
  refused2(await buy('off'), true, 'purchase off');
  const on = await buy('on');
  assert.notEqual(on.body?.code, 'adult_content_off', on.text);
  assert.equal(on.body.code, 'product_not_found', 'past the gate: there is simply no such product');
});

/// The commerce routes answer in their own shape (a code and a message), with the same code and words.
function refused2(r, signedIn, what) {
  assert.equal(r.status, 403, `${what}: ${r.text}`);
  assert.equal(r.body.code, 'adult_content_off', what);
  assert.equal(r.body.error, ADULT_CONTENT_OFF, what);
  assert.equal(r.body.signed_in, signedIn, what);
}

test('guest play stays all-ages only: adult content and a listing with no rating are refused to a guest', async () => {
  const env = await environment();
  const guest = (simId) => call(env, 'POST', '/api/guest/play', { headers: { 'CF-Connecting-IP': '203.0.113.9' }, body: { sim_id: simId, character: 'M' } });
  for (const id of [ADULT, UNRATED, MATURE]) {
    const r = await guest(id);
    assert.equal(r.status, 403, `${id}: ${r.text}`);
    assert.equal(r.body.code, 'guest_not_allowed');
  }
  assert.equal((await guest(KIDS)).status, 200);
});

test('none of the refusals is written with an em dash', () => {
  // Built from its code point: this file carries no em dash of its own.
  assert.ok(!ADULT_CONTENT_OFF.includes(String.fromCodePoint(0x2014)));
});

test('every adult listing above is refused to a signed-out visitor on its page', async () => {
  const env = await environment();
  for (const id of ADULT_IDS) refused(await call(env, 'GET', `/api/simulations/${id}`), false, id);
});

// ── the 21+ band (Guardian Code v2.2 Part V) ─────────────────────────────────

const ADULT21 = 'aaaaaaaa-0000-4000-8000-000000000008';
const ADULT21_PAK = 'aaaaaaaa-0000-4000-8000-000000000009';
const LONE21 = 'aaaaaaaa-0000-4000-8000-000000000017';
const LONER21 = '88888888-8888-4888-8888-888888888888'; // a maker whose only listing is adult_21

/// environment(), with adult_21 listings: one featured world, one .pak, and the only listing of its maker.
async function environment21() {
  const env = await environment();
  await env.USERS.put(`user:${LONER21}`, JSON.stringify({ kyc_verified: true, id: LONER21, username: 'solo21' }));
  await listing(env, ADULT21, 'adult_21', { featured: true });
  await listing(env, ADULT21_PAK, 'adult_21', { pak: true });
  await listing(env, LONE21, 'adult_21', { author: LONER21, authorName: 'solo21' });
  return env;
}

/// The refusal an adult_21 listing gives a viewer whose setting is on and whose verified age is under 21 (or not on record).
function refused21(r, what) {
  assert.equal(r.status, 403, `${what}: ${r.text}`);
  assert.deepEqual(r.body, { error: AGE_21_REQUIRED, code: 'age_21_required', signed_in: true }, what);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store', `${what}: a refusal is never cached`);
}

test('the lists: adult_21 is hidden from a viewer who is off and from one who is on and under 21, and shown at 21 or over', async () => {
  const env = await environment21();
  const safe = [KIDS, MATURE];
  const adult18 = [ADULT, ADULT_PAK, LONE_ADULT];
  const adult21 = [ADULT21, ADULT21_PAK, LONE21, UNRATED];
  for (const path of ['/api/simulations', '/api/gallery', '/api/simulations?q=world', '/api/simulations?genre=all', '/api/simulations?rating_max=adult_21']) {
    assert.deepEqual(ids(await call(env, 'GET', path)), safe.sort(), `${path} signed out`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'off' })), safe.sort(), `${path} off`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'young' })), [...safe, ...adult18].sort(), `${path} on, 19`);
    assert.deepEqual(ids(await call(env, 'GET', path, { who: 'on' })), [...safe, ...adult18, ...adult21].sort(), `${path} on, 30`);
  }
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=adult_18', { who: 'on' })), [...safe, ...adult18].sort(), 'rating_max narrows at 21 too');
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?tag=adult-21-tag', { who: 'young' })), []);
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?tag=adult-21-tag', { who: 'on' })), [ADULT21, ADULT21_PAK, LONE21].sort());
  for (const who of [undefined, 'off', 'young', 'on']) assert.deepEqual(ids(await call(env, 'GET', '/api/gallery/featured', { who })), [KIDS], `featured never carries adult_21: ${who}`);

  await beat(env, KIDS);
  await beat(env, ADULT);
  await beat(env, ADULT21);
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations/live', { who: 'off' })), [KIDS]);
  const youngLive = await call(env, 'GET', '/api/simulations/live?rating_max=adult_21', { who: 'young' });
  assert.deepEqual(ids(youngLive), [ADULT, KIDS].sort());
  assert.equal(youngLive.body.sessions_total, 2, 'nor counted');
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations/live', { who: 'on' })), [ADULT, ADULT21, KIDS].sort());

  const makers = async (who, q = '') => (await call(env, 'GET', `/api/makers${q}`, { who })).body.makers.map((m) => m.username).sort();
  assert.deepEqual(await makers('off'), ['maker']);
  assert.deepEqual(await makers('young', '?rating_max=adult_21'), ['loner', 'maker'], 'a maker whose only Space is adult_21 is not named');
  assert.deepEqual(await makers('on'), ['loner', 'maker', 'solo21']);

  const tags = async (who) => (await call(env, 'GET', '/api/simulations/tags?rating_max=adult_21', { who })).body.tags.map((t) => t.tag);
  const young = await tags('young');
  assert.ok(young.includes('adult-18-tag') && !young.includes('adult-21-tag') && !young.includes('unrated-tag'), young.join(' '));
  const on = await tags('on');
  assert.ok(on.includes('adult-21-tag') && on.includes('unrated-tag'), on.join(' '));
});

test('one adult_21 listing on every surface: off and signed out are told to turn Adult content on, on and under 21 is told it needs 21, at 21 or over and its author are served', async () => {
  const env = await environment21();
  assert.equal((await beat(env, ADULT21)).status, 200);
  const surfaces = [
    ['GET', `/api/simulations/${ADULT21}`],
    ['GET', `/api/simulations/${UNRATED}`],
    ['GET', `/api/simulations/${ADULT21_PAK}/download`],
    ['POST', `/api/simulations/${ADULT21}/play`],
    ['GET', `/api/simulations/${ADULT21}/world/manifest`],
    ['GET', `/api/simulations/${ADULT21}/world/chunks/${H}`],
    ['GET', `/api/simulations/${ADULT21}/world/source`],
    ['GET', `/api/simulations/${ADULT21}/live`],
    ['GET', `/api/simulations/${ADULT21}/previews`],
  ];
  for (const [method, path] of surfaces) {
    refused(await call(env, method, path), false, `${path} signed out`);
    refused(await call(env, method, path, { who: 'off' }), true, `${path} off`);
    refused21(await call(env, method, path, { who: 'young' }), `${path} on, 19`);
    for (const who of ['on', 'author']) {
      const r = await call(env, method, path, { who });
      assert.equal(r.status, 200, `${path} ${who}: ${r.text.slice(0, 160)}`);
    }
  }
  // The same under-21 viewer is served adult_18 on the same surfaces: the setting is on.
  for (const path of [`/api/simulations/${ADULT}`, `/api/simulations/${ADULT_PAK}/download`, `/api/simulations/${ADULT}/world/manifest`, `/api/simulations/${ADULT}/previews`]) {
    assert.equal((await call(env, 'GET', path, { who: 'young' })).status, 200, `${path} on, 19`);
  }
  // A counted play.
  refused21(await call(env, 'POST', '/api/social/play', { who: 'young', body: { simulation_id: ADULT21 } }), 'social play on, 19');
  assert.equal(await env.SOCIAL.get(`plays:${ADULT21}`), null, 'nothing was counted');
  assert.equal((await call(env, 'POST', '/api/social/play', { who: 'on', body: { simulation_id: ADULT21 } })).status, 200);
});

test('a hosted adult_21 session admits a joiner of 21 or over and its author; an under-21 joiner is refused before the ticket is spent', async () => {
  const env = await environment21();
  assert.equal((await beat(env, ADULT21)).status, 200);
  const ticketFor = async (who) => {
    const r = await call(env, 'POST', '/api/identity/ticket', { who, body: { audience: PIN } });
    assert.equal(r.status, 200, r.text);
    return r.body.ticket;
  };
  const admit = (simId, ticket) => call(env, 'POST', `/api/simulations/${simId}/live/admit`, { who: 'author', body: { ticket, audience: PIN } });

  const off = await admit(ADULT21, await ticketFor('off'));
  assert.equal(off.status, 403, off.text);
  assert.equal(off.body.code, 'adult_content_off');
  const young = await admit(ADULT21, await ticketFor('young'));
  assert.equal(young.status, 403, young.text);
  assert.deepEqual(young.body, { error: AGE_21_REQUIRED, code: 'age_21_required', signed_in: true });
  const on = await admit(ADULT21, await ticketFor('on'));
  assert.equal(on.status, 200, on.text);
  assert.equal(on.body.account_id, ON);
  assert.equal((await admit(ADULT21, await ticketFor('author'))).status, 200, 'the author joins their own session');

  // The under-21 joiner is admitted to an adult_18 session: the setting is on.
  assert.equal((await beat(env, ADULT)).status, 200);
  assert.equal((await admit(ADULT, await ticketFor('young'))).status, 200);
});

test('what an adult_21 listing sells is sold only to an account of 21 or over with the setting on', async () => {
  const env = await environment21();
  const buy = (who) => call(env, 'POST', '/api/commerce/purchases', {
    who, headers: { 'Eustress-Mode': 'live' },
    body: { idempotency_key: `buy21-${who}-0000001`, product: `prod_${'b'.repeat(24)}`, sim_id: ADULT21, expected_price: 10 },
  });
  refused2(await buy('off'), true, 'purchase off');
  const young = await buy('young');
  assert.equal(young.status, 403, young.text);
  assert.deepEqual([young.body.code, young.body.error, young.body.signed_in], ['age_21_required', AGE_21_REQUIRED, true]);
  const on = await buy('on');
  assert.equal(on.body.code, 'product_not_found', `past the gate: ${on.text}`);
});

test('guest play refuses adult_21, and a listing with no rating, which reads as it', async () => {
  const env = await environment21();
  const guest = (simId) => call(env, 'POST', '/api/guest/play', { headers: { 'CF-Connecting-IP': '203.0.113.21' }, body: { sim_id: simId, character: 'F' } });
  for (const id of [ADULT21, ADULT21_PAK, UNRATED]) {
    const r = await guest(id);
    assert.equal(r.status, 403, `${id}: ${r.text}`);
    assert.equal(r.body.code, 'guest_not_allowed');
  }
  assert.equal((await guest(KIDS)).status, 200, 'control: all_ages plays');
});

test('a cached slice computed for a viewer of 21 or over is never served to a viewer under 21, nor to one who is off', async () => {
  const env = await environment21();
  await beat(env, KIDS);
  await beat(env, ADULT);
  await beat(env, ADULT21);
  const { cache, restore } = installCache();
  try {
    // The 21+ viewer asks first, so the platform's cache holds the strip and the makers with adult_21 in them.
    assert.deepEqual(ids(await call(env, 'GET', '/api/simulations/live', { who: 'on' })), [ADULT, ADULT21, KIDS].sort());
    assert.ok((await call(env, 'GET', '/api/makers', { who: 'on' })).text.includes('solo21'));
    const held = cache.store.size;
    assert.equal(held, 2);

    // The same URLs inside the cache window, from a viewer with the setting on and a verified age of 19.
    for (const query of ['', '?rating_max=adult_21', '?rating_max=nonsense']) {
      const live = await call(env, 'GET', `/api/simulations/live${query}`, { who: 'young' });
      assert.deepEqual(ids(live), [ADULT, KIDS].sort(), `live${query}`);
      assert.ok(!live.text.includes(ADULT21));
      const makers = await call(env, 'GET', `/api/makers${query}`, { who: 'young' });
      assert.ok(!makers.text.includes('solo21') && makers.text.includes('loner'), `makers${query}: ${makers.text}`);
      for (const who of [undefined, 'off']) {
        assert.deepEqual(ids(await call(env, 'GET', `/api/simulations/live${query}`, { who })), [KIDS], `live${query}, ${who || 'signed out'}`);
        assert.ok(!(await call(env, 'GET', `/api/makers${query}`, { who })).text.includes('solo21'));
      }
    }
    // One entry per slice and route, never one per account: adult_21, adult_18 and mature_17.
    assert.equal(cache.store.size, held + 4);
    const keys = [...cache.store.keys()];
    for (const slice of ['adult_21', 'adult_18', 'mature_17']) assert.equal(keys.filter((k) => k.endsWith(`rating_max=${slice}`)).length, 2, slice);
    assert.ok(!keys.some((k) => k.includes(ON) || k.includes(YOUNG) || k.includes(OFF)), 'no account in a key');
  } finally { restore(); }
});

test('the setting route says whether the verified age is 21 or over, whatever the setting', async () => {
  const env = await environment();
  const SETTLED = '99999999-9999-4999-8999-999999999990'; // 25, setting off
  await env.USERS.put(`user:${SETTLED}`, JSON.stringify({ kyc_verified: true, id: SETTLED, username: 'dee' }));
  await verified(env, SETTLED, bornJan1(25));
  const seen = async (who) => (await call(env, 'GET', '/api/account/adult-content', { who })).body;
  assert.deepEqual(await seen('on'), { enabled: true, can_enable: true, age_21_eligible: true });
  assert.deepEqual(await seen('young'), { enabled: true, can_enable: true, age_21_eligible: false });
  assert.deepEqual(await seen(mintToken(SETTLED)), { enabled: false, can_enable: true, age_21_eligible: true });
  const none = await seen('off');
  assert.deepEqual([none.enabled, none.can_enable, none.age_21_eligible, none.code], [false, false, false, 'kyc_required']);
});

test('a viewer becomes eligible on their 21st birthday, with nobody doing anything', async (t) => {
  const today = new Date();
  if (today.getUTCMonth() === 1 && today.getUTCDate() === 29) return t.skip('no date 21 years before a Feb 29 that is not itself one');
  const env = await environment21();
  const TURNS = '99999999-9999-4999-8999-999999999991'; // 21 today
  const EVE = '99999999-9999-4999-8999-999999999992'; // 21 tomorrow
  for (const [id, dob] of [[TURNS, yearsBefore(21)], [EVE, yearsBefore(21, 1)]]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: id.slice(-1) }));
    await env.USERS.put(adultContentKey(id), '1');
    await verified(env, id, dob);
  }
  const turns = mintToken(TURNS);
  const eve = mintToken(EVE);
  assert.equal((await call(env, 'GET', `/api/simulations/${ADULT21}`, { who: turns })).status, 200, 'on the birthday');
  refused21(await call(env, 'GET', `/api/simulations/${ADULT21}`, { who: eve }), 'the day before');
  assert.ok(ids(await call(env, 'GET', '/api/simulations', { who: turns })).includes(ADULT21));
  assert.ok(!ids(await call(env, 'GET', '/api/simulations', { who: eve })).includes(ADULT21));
  assert.equal((await call(env, 'GET', '/api/account/adult-content', { who: turns })).body.age_21_eligible, true);
  assert.equal((await call(env, 'GET', '/api/account/adult-content', { who: eve })).body.age_21_eligible, false);
});

test('the age is computed at view time from the date of birth: eligible from the first minute of the 21st birthday, while the date is held', async () => {
  const env = await environment();
  const id = '99999999-9999-4999-8999-999999999993';
  await env.USERS.put(adultContentKey(id), '1');
  await verified(env, id, '2005-10-09');
  let reads = 0;
  const realGet = env.KYC_STATUS.get.bind(env.KYC_STATUS);
  env.KYC_STATUS.get = async (k) => { reads += 1; return realGet(k); };
  const adult21 = { author_id: AUTHOR, moderation: { rating: 'adult_21' } };
  const eve = Date.UTC(2026, 9, 8, 23, 59); // 20 years old
  const birthday = Date.UTC(2026, 9, 9, 0, 1); // 21, two minutes later

  assert.equal(await age21Eligible(env, id, eve), false);
  const before = await viewerFor(env, id, eve);
  assert.deepEqual([before.adult, before.age21, mayView(adult21, before), ratingCap(null, before)], [true, false, false, 'adult_18']);
  const readsAtEve = reads;
  assert.equal(await age21Eligible(env, id, birthday), true, 'two minutes later, on the birthday');
  assert.equal(reads, readsAtEve, 'the date of birth was held, and the age was worked out from it again');
  const after = await viewerFor(env, id, birthday);
  assert.deepEqual([after.age21, mayView(adult21, after), ratingCap(null, after)], [true, true, 'adult_21']);
  // Past the hold the record is read again.
  assert.equal(await age21Eligible(env, id, birthday + DOB_CACHE_MS), true);
  assert.ok(reads > readsAtEve);
});

test('the date of birth is the one requireVerifiedAdult resolves: the document first, then the pinned one, then the declared one; only a verified record counts', async () => {
  assert.deepEqual(resolveDob({ extracted_dob: '2006-01-01', age_dob: '1990-01-01' }, { birthday: '1980-01-01' }), { dob: '2006-01-01', source: 'document' });
  assert.deepEqual(resolveDob({ age_dob: '1990-01-01', age_source: 'declared' }, { birthday: '2006-01-01' }), { dob: '1990-01-01', source: 'declared' });
  assert.deepEqual(resolveDob({ age_dob: '1990-01-01' }, { birthday: '2006-01-01' }), { dob: '1990-01-01', source: 'pinned' });
  assert.deepEqual(resolveDob({}, { birthday: '1990-01-01' }), { dob: '1990-01-01', source: 'declared' });
  assert.deepEqual(resolveDob(null, null), { dob: '', source: null });
  const now = Date.UTC(2026, 9, 8, 12);
  assert.equal(ageAt('2005-10-08', now), 21);
  assert.equal(ageAt('2005-10-09', now), 20);
  for (const bad of ['', '2005-02-30', '05-10-08', '2030-01-01', null]) assert.equal(ageAt(bad, now), null, String(bad));

  const env = await environment();
  const cases = [
    ['a document of 19 beats a declared 30', { status: 'verified', extracted_dob: '2007-01-01', age_dob: '1996-01-01' }, '1996-01-01', false],
    ['a pinned 30 beats a declared 19', { status: 'verified', age_dob: '1996-01-01' }, '2007-01-01', true],
    ['the declared date when there is no other', { status: 'linked' }, '1996-01-01', true],
    ['not verified', { status: 'pending', extracted_dob: '1996-01-01' }, '1996-01-01', false],
    ['no date at all', { status: 'verified', age_verified: true }, undefined, false],
  ];
  for (const [i, [what, kyc, birthday, eligible]] of cases.entries()) {
    const id = `99999999-9999-4999-8999-99999999990${i + 4}`;
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, ...(birthday ? { birthday } : {}) }));
    await env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify(kyc));
    assert.equal(await age21Eligible(env, id, now), eligible, what);
  }
  assert.equal(await age21Eligible({}, ON, now), false, 'no identity storage: not eligible');
});

test('the 21+ refusal is written plainly, with no em dash', () => {
  assert.equal(AGE_21_REQUIRED, 'This listing is for adults 21 and over. It opens once your verified age is 21.');
  assert.ok(!AGE_21_REQUIRED.includes(String.fromCodePoint(0x2014)));
});
