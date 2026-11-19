// Solo play needs no sign-in (McKale, 2026-11-19: "Do not require solo simulations to need a login. Only multiplayer
// spaces need a login." and "18+ keeps a sign-in."). A visitor with no session browses, opens and plays SOLO any listing
// rated up to mature_17 (src/adult_content.mjs SOLO_SIGNED_OUT_CAP): the Gallery and search, the listing page, the world
// manifest, its chunks and source, the .pak download and the play route's solo answer. adult_18, adult_21 and a listing
// with no rating (which reads as adult_21) stay closed to them on every route. Joining a hosted session keeps today's
// rules: no host's join link and no live status above ages_10 without a sign-in (SIGNED_OUT_CAP), and guests G to PG
// only. The Discord and Slack apps post into shared channels and keep ages_10. Checked against the real Worker, with
// in-memory storage and the real Durable Object classes.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, GUEST_CAPS_REQUIRED } from '../src/live.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { manifestKey, chunkKey } from '../src/world.mjs';
import { RATINGS } from '../src/moderation.mjs';
import {
  SIGNED_OUT, SIGNED_OUT_CAP, SOLO_SIGNED_OUT_CAP, SOLO, OFF_CAP, ADULT_RATINGS, ADULT_CONTENT_OFF, SIGN_IN_REQUIRED,
  viewerCap, ratingCap, mayView, withinSignedOutCap, signedOutCap, adultContentKey,
} from '../src/adult_content.mjs';
import { interactionScope } from '../src/discord_interactions.mjs';
import { commandScope, POST_CAP } from '../src/slack_bot.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222'; // signed in, Adult content off
const FAN = '33333333-3333-4333-8333-333333333333'; // signed in, Adult content on
const TEENER = '55555555-5555-4555-8555-555555555555'; // a maker whose only listing is mature_17
const ADULTER = '66666666-6666-4666-8666-666666666666'; // a maker whose only listing is adult_18
const ONLY_MATURE = 'dddddddd-0000-4000-8000-000000000021';
const ONLY_ADULT = 'dddddddd-0000-4000-8000-000000000022';
const tokens = { author: mintToken(AUTHOR), player: mintToken(PLAYER), fan: mintToken(FAN) };

// One listing per band, each with a world (manifest, one chunk, a source) and a .pak twin.
const BANDS = { all_ages: '1', ages_10: '2', teen_13: '3', mature_17: '4', adult_18: '5', adult_21: '6', unrated: '7' };
const worldId = (band) => `dddddddd-0000-4000-8000-00000000000${BANDS[band]}`;
const pakId = (band) => `dddddddd-0000-4000-8000-00000000001${BANDS[band]}`;
const SOLO_BANDS = ['ages_10', 'teen_13', 'mature_17'];
const CLOSED_BANDS = ['adult_18', 'adult_21', 'unrated'];

const H = 'c'.repeat(64);
const WORLD_SHA = 'f'.repeat(64);
const SOURCE_SHA = 'e'.repeat(64);
const ORIGIN = 'https://api.eustress.dev';
const IP = { 'CF-Connecting-IP': '203.0.113.7' };
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const PIN2 = 'cd'.repeat(32);
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;

async function listing(env, id, band, { pak = false, author = AUTHOR, authorName = 'maker' } = {}) {
  const rating = band === 'unrated' ? undefined : band;
  const sim = {
    id, name: `World ${band}`, description: 'a place', genre: 'all', author_id: author, author_name: authorName, is_public: true,
    max_players: 8, tags: ['shared', `${band.replace('_', '-')}-tag`], play_count: 0, favorite_count: 0, version: 1,
    published_at: `2026-03-01T00:00:0${BANDS[band]}.000Z`, updated_at: '2026-03-01T00:00:00.000Z',
    thumbnail_url: `${ORIGIN}/api/simulations/${id}/thumbnail`,
    moderation: { status: 'approved', ...(rating ? { rating } : {}), featured: true },
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

async function environment(extra = {}) {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv(), AUTH_RATE_LIMITER: { limit: async () => ({ success: true }) }, ...extra };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  for (const [id, username] of [[AUTHOR, 'maker'], [PLAYER, 'ada'], [FAN, 'fan'], [TEENER, 'teener'], [ADULTER, 'adulter']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  await env.USERS.put(adultContentKey(FAN), '1');
  for (const band of Object.keys(BANDS)) {
    await listing(env, worldId(band), band);
    await listing(env, pakId(band), band, { pak: true });
  }
  await listing(env, ONLY_MATURE, 'mature_17', { author: TEENER, authorName: 'teener' });
  await listing(env, ONLY_ADULT, 'adult_18', { author: ADULTER, authorName: 'adulter' });
  return env;
}

async function call(env, method, path, { who, body, headers = {} } = {}) {
  const h = { ...IP, ...headers };
  if (who) h.Authorization = `Bearer ${tokens[who] || who}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body: parsed };
}

const ids = (r) => (r.body?.simulations || r.body?.featured || r.body?.sessions || []).map((s) => s.id);

/// Every solo route for one listing, as a signed-out Player or page reads it.
const soloReads = (band) => [
  ['GET', `/api/simulations/${worldId(band)}`],
  ['GET', `/api/simulations/${worldId(band)}/world/manifest`],
  ['GET', `/api/simulations/${worldId(band)}/world/chunks/${H}`],
  ['GET', `/api/simulations/${worldId(band)}/world/source`],
  ['GET', `/api/simulations/${pakId(band)}`],
  ['GET', `/api/simulations/${pakId(band)}/download`],
  ['POST', `/api/simulations/${worldId(band)}/play`],
  ['POST', `/api/simulations/${pakId(band)}/play`],
];

// ── the rule ─────────────────────────────────────────────────────────────────

test('the caps: solo signed out reaches mature_17, every other signed-out surface ages_10, and neither reaches an adult band', () => {
  assert.equal(SOLO_SIGNED_OUT_CAP, 'mature_17');
  assert.equal(SIGNED_OUT_CAP, 'ages_10');
  assert.equal(signedOutCap(SOLO), 'mature_17');
  for (const other of [undefined, null, '', 'shared', 'multiplayer', 'SOLO']) assert.equal(signedOutCap(other), 'ages_10', String(other));
  for (const band of ADULT_RATINGS) assert.ok(RATINGS.indexOf(SOLO_SIGNED_OUT_CAP) < RATINGS.indexOf(band), `${band} is above the solo cap`);
  for (const viewer of [SIGNED_OUT, undefined, { accountId: null, signedIn: false, adult: true, age21: true }]) {
    assert.equal(viewerCap(viewer, SOLO), 'mature_17');
    assert.equal(viewerCap(viewer), 'ages_10');
    for (const asked of [null, 'adult_18', 'adult_21', 'nonsense']) {
      assert.equal(ratingCap(asked, viewer, SOLO), 'mature_17', String(asked));
      assert.equal(ratingCap(asked, viewer), 'ages_10', String(asked));
    }
    assert.equal(ratingCap('teen_13', viewer, SOLO), 'teen_13', 'rating_max still narrows');
  }
  const sim = (rating) => ({ author_id: AUTHOR, moderation: rating ? { rating } : {} });
  for (const band of ['all_ages', ...SOLO_BANDS]) {
    assert.equal(mayView(sim(band), SIGNED_OUT, SOLO), true, `${band} solo`);
    assert.equal(withinSignedOutCap(sim(band), SOLO), true, `${band} solo`);
  }
  for (const band of ['teen_13', 'mature_17']) {
    assert.equal(mayView(sim(band), SIGNED_OUT), false, `${band} off the solo surface`);
    assert.equal(withinSignedOutCap(sim(band)), false, `${band} off the solo surface`);
  }
  for (const rating of ['adult_18', 'adult_21', undefined, 'adult_25']) {
    for (const viewer of [SIGNED_OUT, undefined, { accountId: null, signedIn: false, adult: true, age21: true }]) {
      assert.equal(mayView(sim(rating), viewer, SOLO), false, `${rating} solo`);
      assert.equal(mayView(sim(rating), viewer), false, `${rating}`);
    }
    assert.equal(withinSignedOutCap(sim(rating), SOLO), false, String(rating));
  }
  // Signed in with Adult content off: mature_17 on every surface, solo or not.
  const off = { accountId: PLAYER, signedIn: true, adult: false, age21: false };
  assert.equal(OFF_CAP, 'mature_17');
  assert.equal(viewerCap(off), 'mature_17');
  assert.equal(viewerCap(off, SOLO), 'mature_17');
  assert.equal(ratingCap('adult_21', off, SOLO), 'mature_17');
  assert.equal(mayView(sim('adult_18'), off, SOLO), false);
});

// ── solo, signed out ─────────────────────────────────────────────────────────

test('signed out sees ages_10, teen_13 and mature_17 in the Gallery, search, featured, makers and tags, and never an adult or unrated listing', async () => {
  const env = await environment();
  const shown = [...['all_ages', ...SOLO_BANDS].flatMap((b) => [worldId(b), pakId(b)]), ONLY_MATURE].sort();
  const hidden = [...CLOSED_BANDS.flatMap((b) => [worldId(b), pakId(b)]), ONLY_ADULT];
  for (const path of ['/api/simulations', '/api/gallery', '/api/simulations?q=world', '/api/simulations?tag=shared', '/api/simulations?rating_max=adult_21']) {
    const r = await call(env, 'GET', path);
    assert.equal(r.status, 200, path);
    assert.deepEqual(ids(r).sort(), shown, `${path} signed out`);
    for (const id of hidden) assert.ok(!r.text.includes(id), `${path}: ${id}`);
  }
  // rating_max still narrows a signed-out list.
  assert.deepEqual(ids(await call(env, 'GET', '/api/simulations?rating_max=ages_10')).sort(), ['all_ages', 'ages_10'].flatMap((b) => [worldId(b), pakId(b)]).sort());
  // Featured never carries adult content; below that, the solo cap.
  assert.deepEqual(ids(await call(env, 'GET', '/api/gallery/featured')).sort(), shown);
  const tags = (await call(env, 'GET', '/api/simulations/tags')).body.tags.map((t) => t.tag);
  for (const band of SOLO_BANDS) assert.ok(tags.includes(`${band.replace('_', '-')}-tag`), `tags: ${band}`);
  for (const band of CLOSED_BANDS) assert.ok(!tags.includes(`${band.replace('_', '-')}-tag`), `tags: ${band}`);
  // A maker whose only listing is mature_17 is named signed out; one whose only listing is adult content is not.
  const makers = await call(env, 'GET', '/api/makers');
  assert.deepEqual(makers.body.makers.map((m) => m.username).sort(), ['maker', 'teener']);
  // The signed-out answer is the same for everyone signed out, and varies on Authorization.
  assert.match(String((await call(env, 'GET', '/api/simulations')).headers.get('Vary')), /Authorization/);
});

test('signed out opens and plays solo ages_10, teen_13 and mature_17: the page, the world, its chunks and source, the .pak and the play route', async () => {
  const env = await environment();
  for (const band of SOLO_BANDS) {
    for (const [method, path] of soloReads(band)) {
      const r = await call(env, method, path);
      assert.equal(r.status, 200, `${band} ${method} ${path}: ${r.text}`);
    }
    const play = await call(env, 'POST', `/api/simulations/${worldId(band)}/play`);
    assert.deepEqual([play.body.status, play.body.launch.link], ['solo', `eustress-player://play/${worldId(band)}`], band);
    assert.equal((await call(env, 'GET', `/api/simulations/${pakId(band)}/download`)).text, 'the world', band);
    assert.equal((await call(env, 'GET', `/api/simulations/${worldId(band)}/world/chunks/${H}`)).text, 'ECHK chunk bytes', band);
    // A listing that is not adult content keeps its public image URL.
    const page = await call(env, 'GET', `/api/simulations/${worldId(band)}`);
    assert.equal(page.body.thumbnail_url.includes('sig='), false, band);
  }
});

test('signed out is refused adult_18, adult_21 and unrated on every solo route, with the codes of today', async () => {
  const env = await environment();
  for (const band of CLOSED_BANDS) {
    for (const [method, path] of [...soloReads(band), ['GET', `/api/simulations/${worldId(band)}/previews`], ['GET', `/api/simulations/${worldId(band)}/thumbnail`]]) {
      const r = await call(env, method, path);
      assert.equal(r.status, 403, `${band} ${method} ${path}: ${r.text}`);
      assert.equal(r.headers.get('Cache-Control'), 'private, no-store', `${band} ${path}`);
      assert.ok(!r.text.includes('ECHK') && !r.text.includes('the world') && !r.text.includes('echk'), `${band} ${path}: nothing of the listing`);
      if (path.endsWith('/thumbnail')) assert.equal(r.body.code, 'adult_image_unsigned', path);
      else assert.deepEqual(r.body, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: false }, `${band} ${method} ${path}`);
    }
  }
  // A garbage or expired session is signed out, and refused the same way.
  for (const band of CLOSED_BANDS) {
    const r = await call(env, 'GET', `/api/simulations/${worldId(band)}/world/manifest`, { who: 'not.a.token' });
    assert.deepEqual([r.status, r.body.code], [403, 'adult_content_off'], band);
  }
});

// ── multiplayer keeps a sign-in ──────────────────────────────────────────────

const hostAccount = (env, simId, pin = PIN) => call(env, 'POST', `/api/simulations/${simId}/live`, { who: 'author', body: { link: linkFor(pin), players: 2, max_players: 8, protocol: 3 } });
const hostGuests = (env, simId, pin = PIN2) => call(env, 'POST', `/api/simulations/${simId}/live`, { who: 'author', body: { link: linkFor(pin), players: 1, max_players: 200, protocol: 3, guest_only: true, guest_caps: [...GUEST_CAPS_REQUIRED] } });

test('signed out gets no join link and no live status for a hosted teen_13 or mature_17 listing; signed in does', async () => {
  const env = await environment({ GUEST_PLAY: 'on', GUEST_CHAT_RATE_LIMITER: { limit: async () => ({ success: true }) } });
  for (const band of ['teen_13', 'mature_17']) {
    const id = worldId(band);
    assert.equal((await hostAccount(env, id)).status, 200, band);
    assert.equal((await hostGuests(env, id)).status, 200, `${band}: a guest-only session is hosted too`);
    // The play route: the solo answer, and nothing about the live session.
    const play = await call(env, 'POST', `/api/simulations/${id}/play`);
    assert.equal(play.status, 200, play.text);
    assert.equal(play.body.status, 'solo', band);
    assert.equal(play.body.launch.link, `eustress-player://play/${id}`, band);
    assert.ok(!('live' in play.body) && !('link_hidden' in play.body), `${band}: ${play.text}`);
    assert.ok(!play.text.includes(KEY) && !play.text.includes(PIN) && !play.text.includes(PIN2), `${band}: no host`);
    // A guest lease on another listing is no key to this one, and none can be opened on this one.
    const lease = await call(env, 'POST', '/api/guest/play', { body: { sim_id: id, character: 'F', client: 'player' } });
    assert.deepEqual([lease.status, lease.body.reason], [403, 'rating'], band);
    const kids = await call(env, 'POST', '/api/guest/play', { body: { sim_id: worldId('ages_10'), character: 'F', client: 'player' } });
    assert.equal(kids.status, 200, kids.text);
    const withLease = await call(env, 'POST', `/api/simulations/${id}/play`, { headers: { 'X-Eustress-Lease': kids.body.ticket } });
    assert.ok(!withLease.text.includes(KEY), `${band}: another listing's lease`);
    assert.equal(withLease.body.status, 'solo');
    // /live keeps today's answer: sign in first.
    const live = await call(env, 'GET', `/api/simulations/${id}/live`);
    assert.deepEqual([live.status, live.body.code], [403, 'sign_in_required'], band);
    assert.deepEqual(live.body, { error: SIGN_IN_REQUIRED, code: 'sign_in_required', signed_in: false });
    // The live-now strip, signed out, leaves it out.
    assert.ok(!ids(await call(env, 'GET', '/api/simulations/live')).includes(id), `${band}: live now`);
    // Signed out cannot mint a join ticket.
    assert.equal((await call(env, 'POST', '/api/identity/ticket', { body: { audience: PIN } })).status, 401, `${band}: join ticket`);
    // Signed in: the link.
    const signedIn = await call(env, 'POST', `/api/simulations/${id}/play`, { who: 'player' });
    assert.deepEqual([signedIn.body.status, signedIn.body.launch.link], ['live', linkFor(PIN)], band);
    assert.equal((await call(env, 'GET', `/api/simulations/${id}/live`, { who: 'player' })).body.link, linkFor(PIN), band);
  }
  // Today's rule for a hosted ages_10 listing is kept: told it is live, never given the link.
  const ten = worldId('ages_10');
  assert.equal((await hostAccount(env, ten)).status, 200);
  const play = await call(env, 'POST', `/api/simulations/${ten}/play`);
  assert.deepEqual([play.body.status, play.body.link_hidden, typeof play.body.live?.players], ['solo', true, 'number']);
  assert.ok(!play.text.includes(KEY));
  // And a guest with a live lease on it is still given its guest-only session's link, as today.
  assert.equal((await hostGuests(env, ten)).status, 200);
  const lease = await call(env, 'POST', '/api/guest/play', { body: { sim_id: ten, character: 'F', client: 'player' } });
  assert.equal(lease.status, 200, lease.text);
  const guest = await call(env, 'POST', `/api/simulations/${ten}/play`, { headers: { 'X-Eustress-Lease': lease.body.ticket } });
  assert.deepEqual([guest.body.status, guest.body.launch.link], ['live', linkFor(PIN2)]);
});

// ── other surfaces ───────────────────────────────────────────────────────────

test('the Discord and Slack apps keep ages_10 for anyone not a linked, verified adult', async () => {
  const env = await environment();
  assert.equal(POST_CAP, 'ages_10');
  const discord = await interactionScope(env, { context: 0, channel: { nsfw: false }, member: { user: { id: '1234' } } });
  assert.equal(discord.cap, 'ages_10');
  const discordDm = await interactionScope(env, { context: 1, user: { id: '1234' } });
  assert.equal(discordDm.cap, 'ages_10', 'an unlinked person in a DM');
  const slack = await commandScope(env, { team_id: 'T1', user_id: 'U1', channel_id: 'C1' });
  assert.equal(slack.cap, 'ages_10');
});

// ── signed in ────────────────────────────────────────────────────────────────

test('signed in with Adult content off is still capped at mature_17, and with it on sees adult_18', async () => {
  const env = await environment();
  const list = ids(await call(env, 'GET', '/api/simulations', { who: 'player' }));
  for (const band of ['all_ages', ...SOLO_BANDS]) assert.ok(list.includes(worldId(band)), band);
  for (const band of CLOSED_BANDS) assert.ok(!list.includes(worldId(band)), band);
  for (const band of CLOSED_BANDS) {
    const r = await call(env, 'GET', `/api/simulations/${worldId(band)}/world/manifest`, { who: 'player' });
    assert.deepEqual([r.status, r.body], [403, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: true }], band);
  }
  assert.ok(ids(await call(env, 'GET', '/api/simulations', { who: 'fan' })).includes(worldId('adult_18')));
  assert.equal((await call(env, 'GET', `/api/simulations/${worldId('adult_18')}/world/manifest`, { who: 'fan' })).status, 200);
});

test('the play pass still works: minted with the session, it opens an adult listing for an account that turned Adult content on', async () => {
  const env = await environment();
  const id = worldId('adult_18');
  const minted = await call(env, 'POST', `/api/simulations/${id}/play-ticket`, { who: 'fan' });
  assert.equal(minted.status, 200, minted.text);
  const redeemed = await call(env, 'POST', `/api/simulations/${id}/play-pass`, { body: { ticket: minted.body.ticket } });
  assert.equal(redeemed.status, 200, redeemed.text);
  const pass = { Authorization: `PlayPass ${redeemed.body.pass}` };
  for (const path of [`/api/simulations/${id}`, `/api/simulations/${id}/world/manifest`, `/api/simulations/${id}/world/chunks/${H}`]) {
    assert.equal((await call(env, 'GET', path, { headers: pass })).status, 200, path);
    assert.equal((await call(env, 'GET', path)).status, 403, `${path} signed out`);
  }
  // Signed out cannot mint one.
  assert.equal((await call(env, 'POST', `/api/simulations/${worldId('teen_13')}/play-ticket`)).status, 401);
});
