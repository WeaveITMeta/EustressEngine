// The live index: every session hosted right now, for the Gallery's "live now" strip (src/live_index.mjs,
// docs/networking/LIVE_INDEX.md). A heartbeat of a LISTED listing tells it { sim_id, author, players, max_players, region, since };
// a host's stop removes the entry at once; an entry not renewed expires 90 seconds after its last beat on the object's own alarm;
// an author holds at most 5 entries. GET /api/simulations/live names only LISTED listings, never a join link, key or pin, and its
// totals cover every listed session even when the list is capped. `players` is what the host claims, capped at the players the
// Worker admitted plus the host. The region is the host's COUNTRY, validated; a data centre names a city and is never kept, and
// no address is. The computed strip is kept for 10 seconds. The real Worker runs against in-memory storage, with the real LiveHost
// and LiveIndex classes and the real ticket routes.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { fakeCache, installCache } from './helpers/fake_cache.mjs';
import { LiveHost, HEARTBEAT_TTL_MS } from '../src/live.mjs';
import {
  LiveIndex, LIVE_INDEX_TTL_MS, LIVE_SESSIONS_SHOWN, LIVE_INDEX_CALL_TIMEOUT_MS, LIVE_INDEX_PER_AUTHOR, LIVE_INDEX_CACHE_SECONDS, cleanRegion, regionOf, publicRegion,
} from '../src/live_index.mjs';
import { routePolicy, PUBLIC } from '../src/route_policy.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const PIN2 = 'cd'.repeat(32);
const LAN = '192.168.1.20';
const linkFor = (pin = PIN, host = LAN) => `eustress-player://join/${host}:7777?key=${KEY}&pin=${pin}`;
const idOf = (n) => `aaaaaaaa-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
const authorOf = (n) => `bbbbbbbb-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
/// An account that turned Adult content on (src/adult_content.mjs), set in storage as the setting's route leaves it.
const ADULT_VIEWER = '33333333-3333-4333-8333-333333333333';
const adultOn = (env, id = ADULT_VIEWER) => env.USERS.put(`adult-content:${id}`, '1');
const tokenCache = new Map();
const tokenFor = (who) => {
  if (!tokenCache.has(who)) tokenCache.set(who, mintToken(who === 'author' ? AUTHOR : who === 'stranger' ? STRANGER : who));
  return tokenCache.get(who);
};

async function environment({ index = true } = {}) {
  const env = fakeEnv();
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  if (index) env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  return env;
}

/// Listing number `n`, by `author` (the account AUTHOR unless another is named).
async function listing(env, n, { status = 'approved', is_public = true, rating = 'all_ages', cover, thumbnail = true, name = `World ${n}`, author = AUTHOR } = {}) {
  if (author !== AUTHOR) await env.USERS.put(`user:${author}`, JSON.stringify({ kyc_verified: true, id: author, username: `maker${n}` }));
  await env.SOCIAL.put(`sim:${idOf(n)}`, JSON.stringify({
    id: idOf(n), name, description: '', genre: 'all', max_players: 8, author_id: author, author_name: author === AUTHOR ? 'host' : `maker${n}`, is_public,
    moderation: { status, rating }, play_count: 0, favorite_count: 0, version: 1,
    published_at: '2026-03-01T00:00:00.000Z', updated_at: '2026-03-01T00:00:00.000Z',
    ...(thumbnail ? { thumbnail_url: `https://api.eustress.dev/api/simulations/${idOf(n)}/thumbnail` } : {}),
    ...(cover ? { cover } : {}),
  }));
  return idOf(n);
}

async function call(env, method, path, { who, body, cf, headers, origin = 'https://api.eustress.dev' } = {}) {
  const h = { ...(headers || {}) };
  if (who) h.Authorization = `Bearer ${tokenFor(who)}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const request = new Request(`${origin}${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) });
  if (cf) request.cf = cf;
  const res = await worker.fetch(request, env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body: parsed };
}

const beat = (env, n, { players = 1, max = 4, pin = PIN, host = LAN, cf, headers, who = 'author' } = {}) => call(env, 'POST', `/api/simulations/${idOf(n)}/live`, {
  who, cf, headers, body: { link: linkFor(pin, host), players, max_players: max, protocol: 3 },
});
const stop = (env, n, who = 'author') => call(env, 'DELETE', `/api/simulations/${idOf(n)}/live`, { who });
const liveNow = (env, query = '', options = {}) => call(env, 'GET', `/api/simulations/live${query}`, options);
const single = (env, n, who) => call(env, 'GET', `/api/simulations/${idOf(n)}/live`, { who });
const indexObject = (env) => env.LIVE_INDEX.instances.get('all');
const indexRows = (env) => [...(indexObject(env)?.storage.map.entries() || [])].filter(([k]) => k.startsWith('s:')).map(([, v]) => v);
const authorLists = (env) => Object.fromEntries([...(indexObject(env)?.storage.map.entries() || [])].filter(([k]) => k.startsWith('a:')).map(([k, v]) => [k.slice(2), v]));

/// Players the Worker admits to a hosted session: real tickets, minted by an account and admitted by the host's author. Returns
/// the tickets, so a test can try one again.
async function admit(env, n, count, { pin = PIN, as = 'author' } = {}) {
  const tickets = [];
  for (let i = 0; i < count; i++) {
    const minted = await call(env, 'POST', '/api/identity/ticket', { who: 'stranger', body: { audience: pin } });
    assert.equal(minted.status, 200, minted.text);
    const r = await call(env, 'POST', `/api/simulations/${idOf(n)}/live/admit`, { who: as, body: { ticket: minted.body.ticket, audience: pin } });
    assert.equal(r.status, 200, r.text);
    tickets.push(minted.body.ticket);
  }
  return tickets;
}

/// A host that is hosting, with `admitted` players really let in and `players` in its heartbeat (the host plus the admitted by default).
async function hosting(env, n, { players, admitted = 0, max = 8, pin = PIN, cf, as = 'author' } = {}) {
  assert.equal((await beat(env, n, { players: 0, max, pin, cf, who: as })).status, 200);
  await admit(env, n, admitted, { pin, as });
  const b = await beat(env, n, { players: players ?? admitted + 1, max, pin, cf, who: as });
  assert.equal(b.status, 200, b.text);
  return b;
}

/// A clock the test moves: Date.now is the platform's, so a host and its index agree on it.
function clock() {
  const real = Date.now;
  let t = real();
  Date.now = () => t;
  return { advance: (ms) => { t += ms; }, now: () => t, restore: () => { Date.now = real; } };
}
const quietly = async (fn) => {
  const real = console.error;
  console.error = () => {};
  try { return await fn(); } finally { console.error = real; }
};

// -- registering and reading -----------------------------------------------------------------------------------------------------

test('a heartbeat registers the session, and the list names it with what the Gallery shows and nothing else', async () => {
  const env = await environment();
  await listing(env, 1, { name: 'Box Head', cover: { ext: 'png', width: 600, height: 800, bytes: 9, updated_at: '2026-02-03T04:05:06.000Z' } });
  const empty = await liveNow(env);
  assert.equal(empty.status, 200);
  assert.deepEqual(empty.body, { sessions: [], players_total: 0, sessions_total: 0 });

  const t = clock();
  try {
    await hosting(env, 1, { players: 3, admitted: 2, max: 8, cf: { country: 'DE', colo: 'FRA' } });
    const r = await liveNow(env);
    assert.equal(r.status, 200, r.text);
    assert.equal(r.headers.get('Cache-Control'), 'public, max-age=10');
    assert.equal(r.headers.get('Vary'), 'Origin, Authorization', 'the signed-out strip is kept apart from any session (src/adult_content.mjs)');
    assert.deepEqual(r.body, {
      sessions: [{
        id: idOf(1), name: 'Box Head', author_name: 'host',
        thumbnail_url: `https://api.eustress.dev/api/simulations/${idOf(1)}/thumbnail`,
        cover_url: `https://api.eustress.dev/api/simulations/${idOf(1)}/cover?v=${Date.parse('2026-02-03T04:05:06.000Z')}`,
        tags: [], tags_source: 'author', genre: null,
        players: 3, max_players: 8, region: { country: 'DE' }, since: new Date(t.now()).toISOString(),
      }],
      players_total: 3,
      sessions_total: 1,
    });
    assert.deepEqual(Object.keys(r.body.sessions[0]), ['id', 'name', 'author_name', 'thumbnail_url', 'cover_url', 'tags', 'tags_source', 'genre', 'players', 'max_players', 'region', 'since']);
    assert.ok(!r.text.includes(AUTHOR), 'the author the cap is kept by is never returned');
  } finally { t.restore(); }
});

test('a listing with no cover and no thumbnail says null for both, and a session with no known region says null', async () => {
  const env = await environment();
  await listing(env, 1, { thumbnail: false });
  await beat(env, 1);
  const s = (await liveNow(env)).body.sessions[0];
  assert.equal(s.cover_url, null);
  assert.equal(s.thumbnail_url, null);
  assert.equal(s.region, null);
});

test('the URLs name the origin that asked, so staging names staging', async () => {
  const env = await environment();
  await listing(env, 1, { cover: { ext: 'jpg', width: 800, height: 450, bytes: 9, updated_at: '2026-02-03T04:05:06.000Z' } });
  await beat(env, 1);
  const res = await worker.fetch(new Request('https://api-staging.eustress.dev/api/simulations/live'), env, ctx);
  const body = await res.json();
  assert.ok(body.sessions[0].cover_url.startsWith('https://api-staging.eustress.dev/api/simulations/'), body.sessions[0].cover_url);
});

test('two hosts: the totals add up, and the newest session is first', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  const t = clock();
  try {
    await hosting(env, 1, { players: 2, admitted: 1, max: 4 });
    t.advance(10_000);
    await hosting(env, 2, { players: 5, admitted: 4, max: 10 });
    const r = await liveNow(env);
    assert.deepEqual(r.body.sessions.map((s) => s.id), [idOf(2), idOf(1)], 'newest first');
    assert.deepEqual(r.body.sessions.map((s) => s.players), [5, 2]);
    assert.equal(r.body.players_total, 7);
    assert.equal(r.body.sessions_total, 2);
  } finally { t.restore(); }
});

test('a later beat renews the entry with the new counts and keeps when it started; a restart on a new link starts again', async () => {
  const env = await environment();
  await listing(env, 1);
  const t = clock();
  try {
    await hosting(env, 1, { players: 1 });
    const started = (await liveNow(env)).body.sessions[0].since;
    t.advance(30_000);
    await admit(env, 1, 3);
    await beat(env, 1, { players: 4 });
    const again = (await liveNow(env)).body.sessions[0];
    assert.equal(again.players, 4);
    assert.equal(again.since, started, 'the same run');
    t.advance(30_000);
    await beat(env, 1, { players: 2, pin: PIN2 });
    const restarted = (await liveNow(env)).body.sessions[0];
    assert.notEqual(restarted.since, started, 'a new certificate is a new run');
    assert.equal((await liveNow(env)).body.sessions_total, 1, 'and still one session');
  } finally { t.restore(); }
});

// -- only what is listed ---------------------------------------------------------------------------------------------------------

test('only a listed listing is ever told to the index, and what the index holds is still checked on every read', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2, { status: 'pending' });
  await listing(env, 3, { status: 'rejected' });
  await listing(env, 4, { status: 'quarantined' });
  await listing(env, 5, { is_public: false });
  await listing(env, 6, { is_public: false });
  const archived = JSON.parse(await env.SOCIAL.get(`sim:${idOf(6)}`));
  await env.SOCIAL.put(`sim:${idOf(6)}`, JSON.stringify({ ...archived, archived: true }));
  await listing(env, 7);
  await env.SCENES.put(`locks/legal/${idOf(7)}`, '{}', { customMetadata: { kind: 'serve_block' } });
  await listing(env, 8);
  await env.SCENES.put(`locks/legal/${idOf(8)}`, '{}', { customMetadata: { kind: 'content_lock' } });
  for (let n = 1; n <= 8; n++) assert.equal((await beat(env, n)).status, 200, `host ${n} is accepted: it is the author's own check`);
  assert.deepEqual(indexRows(env).map((row) => row.sim_id), [idOf(1)], 'the index holds the one listed listing, and nothing else');
  assert.deepEqual(authorLists(env), { [AUTHOR]: [idOf(1)] });

  // Entries that got there while their listing was listed, and a listing that does not exist at all.
  const now = Date.now();
  for (const n of [2, 3, 4, 5, 6, 7, 8, 9]) {
    await env.LIVE_INDEX.get('all').fetch('https://live-index.internal/beat', {
      method: 'POST', body: JSON.stringify({ sim_id: idOf(n), author: authorOf(n), players: 100, max_players: 200, region: null, since: now, now }),
    });
  }
  assert.equal(indexRows(env).length, 9);
  const r = await liveNow(env);
  assert.deepEqual(r.body.sessions.map((s) => s.id), [idOf(1)]);
  assert.equal(r.body.sessions_total, 1);
  assert.equal(r.body.players_total, 1);
  for (const n of [2, 3, 4, 5, 6, 7, 8, 9]) assert.ok(!r.text.includes(idOf(n)), `listing ${n} is not in the answer`);
});

test('600 unlisted listings hosting at once leave the index as it was: the one listed session, started earlier, is still shown', { timeout: 60_000 }, async () => {
  const env = await environment();
  const t = clock();
  try {
    await listing(env, 1);
    await hosting(env, 1, { players: 1 });
    t.advance(1_000);
    for (let n = 2; n <= 601; n++) {
      await listing(env, n, { status: n % 2 ? 'pending' : 'rejected', author: authorOf(n) });
      assert.equal((await beat(env, n, { who: authorOf(n) })).status, 200);
    }
    assert.deepEqual(indexRows(env).map((row) => row.sim_id), [idOf(1)], 'not one of the 600 is in the index');
    const r = await liveNow(env);
    assert.deepEqual(r.body.sessions.map((s) => s.id), [idOf(1)]);
    assert.equal(r.body.sessions_total, 1);
  } finally { t.restore(); }
});

test('an unlisted listing\'s heartbeat is accepted and tells the index nothing, and a listed one tells it once', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2, { status: 'pending' });
  const told = [];
  const real = env.LIVE_INDEX.get.bind(env.LIVE_INDEX);
  env.LIVE_INDEX.get = (id) => { const stub = real(id); return { fetch: (url, init) => { told.push(url); return stub.fetch(url, init); } }; };
  assert.equal((await beat(env, 2)).status, 200);
  assert.equal((await single(env, 2, 'author')).body.live, true, 'the author can still check an unlisted host');
  assert.deepEqual(told, []);
  assert.equal((await beat(env, 1)).status, 200);
  assert.deepEqual(told, ['https://live-index.internal/beat']);
});

test('the viewer\'s age slice is honoured: sessions above it are neither listed nor counted', async () => {
  const env = await environment();
  await listing(env, 1, { rating: 'all_ages' });
  await listing(env, 2, { rating: 'teen_13' });
  await listing(env, 3, { rating: 'adult_18' });
  await hosting(env, 1, { players: 1 });
  await hosting(env, 2, { players: 2, admitted: 1 });
  await adultOn(env, STRANGER); // the joiners of an adult session turned Adult content on
  await hosting(env, 3, { players: 4, admitted: 3 });
  // Signed out, Adult content is off: the adult session is neither listed nor counted (src/adult_content.mjs).
  assert.equal((await liveNow(env)).body.players_total, 3);
  assert.equal((await liveNow(env, '?rating_max=adult_18')).body.sessions_total, 2, 'rating_max cannot widen past the setting');
  // A viewer who turned it on sees every rating, and rating_max narrows it.
  await adultOn(env);
  const on = { who: ADULT_VIEWER };
  assert.equal((await liveNow(env, '', on)).body.players_total, 7);
  const kids = (await liveNow(env, '?rating_max=all_ages', on)).body;
  assert.deepEqual(kids.sessions.map((s) => s.id), [idOf(1)]);
  assert.deepEqual([kids.players_total, kids.sessions_total], [1, 1]);
  const teens = (await liveNow(env, '?rating_max=teen_13', on)).body;
  assert.deepEqual([teens.players_total, teens.sessions_total], [3, 2]);
  assert.equal((await liveNow(env, '?rating_max=nonsense', on)).body.sessions_total, 3);
});

test('a listing that becomes unlisted disappears from the next read, and comes back when it is listed again', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1);
  assert.equal((await liveNow(env)).body.sessions_total, 1);
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${idOf(1)}`));
  await env.SOCIAL.put(`sim:${idOf(1)}`, JSON.stringify({ ...sim, moderation: { ...sim.moderation, status: 'pending' } }));
  assert.equal((await liveNow(env)).body.sessions_total, 0);
  await env.SOCIAL.put(`sim:${idOf(1)}`, JSON.stringify(sim));
  assert.equal((await liveNow(env)).body.sessions_total, 1);
});

// -- the totals and the cap ------------------------------------------------------------------------------------------------------

test('the list is capped at 24, newest first, and the totals still cover every listed session', async () => {
  const env = await environment();
  const t = clock();
  try {
    for (let n = 1; n <= 30; n++) {
      await listing(env, n, { author: authorOf(n) });
      assert.equal((await beat(env, n, { players: 1, max: 100, who: authorOf(n) })).status, 200);
      t.advance(1_000);
    }
    await listing(env, 31, { status: 'pending', author: authorOf(31) });
    await beat(env, 31, { players: 1000, max: 2000, who: authorOf(31) });
    const r = await liveNow(env);
    assert.equal(LIVE_SESSIONS_SHOWN, 24);
    assert.equal(r.body.sessions.length, 24);
    assert.deepEqual(r.body.sessions.map((s) => s.id), Array.from({ length: 24 }, (_, i) => idOf(30 - i)), 'the 24 newest');
    assert.equal(r.body.sessions_total, 30, 'every listed session');
    assert.equal(r.body.players_total, 30, 'every listed session\'s players, listed or not in the 24');
    assert.ok(!r.text.includes(idOf(1)) && !r.text.includes(idOf(31)));
  } finally { t.restore(); }
});

test('an author holds at most 5 entries: a sixth drops the oldest, and a renewed entry never costs another', async () => {
  const env = await environment();
  const t = clock();
  try {
    assert.equal(LIVE_INDEX_PER_AUTHOR, 5);
    for (let n = 1; n <= 7; n++) {
      await listing(env, n);
      await beat(env, n);
      t.advance(1_000);
    }
    const held = () => indexRows(env).map((row) => row.sim_id).sort();
    assert.deepEqual(held(), [3, 4, 5, 6, 7].map(idOf).sort(), 'the five newest');
    assert.deepEqual([...authorLists(env)[AUTHOR]].sort(), held(), 'the author\'s list is the same five');
    assert.deepEqual((await liveNow(env)).body.sessions.map((s) => s.id), [7, 6, 5, 4, 3].map(idOf));

    // Renewing one of the five changes nothing about who is held.
    for (let n = 3; n <= 7; n++) await beat(env, n);
    assert.deepEqual(held(), [3, 4, 5, 6, 7].map(idOf).sort());

    // A dropped host that beats again comes back and drops the oldest of the rest.
    await beat(env, 1);
    assert.deepEqual(held(), [1, 4, 5, 6, 7].map(idOf).sort(), 'the oldest of the five went');

    // Another author has a cap of their own.
    await listing(env, 8, { author: authorOf(8) });
    await beat(env, 8, { who: authorOf(8) });
    assert.equal(indexRows(env).length, 6);

    // A stop frees a place, and the list follows.
    await stop(env, 4);
    assert.deepEqual([...authorLists(env)[AUTHOR]].sort(), [1, 5, 6, 7].map(idOf).sort());
    await listing(env, 9);
    await beat(env, 9);
    assert.equal(indexRows(env).filter((row) => row.author_id === AUTHOR).length, 5);
    assert.ok(held().includes(idOf(1)), 'nothing was dropped to make room: a place was free');
  } finally { t.restore(); }
});

test('an author\'s list is cleared with their last entry, by the alarm as by a stop', async () => {
  const env = await environment();
  const t = clock();
  try {
    await listing(env, 1);
    await listing(env, 2);
    await beat(env, 1);
    await beat(env, 2);
    assert.equal(Object.keys(authorLists(env)).length, 1);
    await stop(env, 1);
    await stop(env, 2);
    assert.deepEqual(authorLists(env), {}, 'a stop clears an emptied list');

    await beat(env, 1);
    t.advance(LIVE_INDEX_TTL_MS);
    await indexObject(env).alarm();
    assert.deepEqual(indexRows(env), []);
    assert.deepEqual(authorLists(env), {}, 'so does the alarm');
  } finally { t.restore(); }
});

// -- players: what the host claims, up to what the Worker admitted --------------------------------------------------------------------------

test('players is what the host claims, capped at the players the Worker admitted plus the host, and the totals add up the capped numbers', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  await listing(env, 3);
  await listing(env, 4);
  await beat(env, 1, { players: 10_000, max: 10_000 });
  await hosting(env, 2, { players: 10_000, admitted: 3, max: 10_000 });
  await hosting(env, 3, { players: 2, admitted: 5 });
  await hosting(env, 4, { players: 0, admitted: 0 });
  const r = (await liveNow(env)).body;
  const players = Object.fromEntries(r.sessions.map((s) => [s.id, s.players]));
  assert.equal(players[idOf(1)], 1, 'a claim of 10,000 with nobody admitted is the host alone');
  assert.equal(players[idOf(2)], 4, 'three admitted and the host');
  assert.equal(players[idOf(3)], 2, 'a claim below the cap stands');
  assert.equal(players[idOf(4)], 0, 'a host that is not playing, with nobody in, is nobody');
  assert.equal(r.players_total, 7);
  assert.equal(r.sessions_total, 4);
  assert.equal(indexRows(env).find((row) => row.sim_id === idOf(2)).players, 4, 'the index holds the capped number');
});

test('a Space\'s page never shows a bigger number than the strip: the strip, the listing\'s live read and the play route agree', async () => {
  const env = await environment();
  await listing(env, 1);
  await hosting(env, 1, { players: 10_000, admitted: 3, max: 10_000 });
  const strip = (await liveNow(env)).body.sessions[0].players;
  const read = (await single(env, 1, 'stranger')).body.players;
  const played = (await call(env, 'POST', `/api/simulations/${idOf(1)}/play`, { who: 'stranger' })).body.live.players;
  assert.deepEqual([strip, read, played], [4, 4, 4]);
  assert.equal((await single(env, 1)).body.players, 4, 'and to a visitor with no account');
});

test('a ticket counts once: a ticket used again is refused and adds no player', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1, { players: 0 });
  const [ticket] = await admit(env, 1, 1);
  const again = await call(env, 'POST', `/api/simulations/${idOf(1)}/live/admit`, { who: 'author', body: { ticket, audience: PIN } });
  assert.equal(again.status, 403);
  assert.equal(again.body.code, 'used');
  await beat(env, 1, { players: 9 });
  assert.equal((await liveNow(env)).body.sessions[0].players, 2, 'one admitted and the host');
});

test('a new hosting session starts counting again: a restart on a new link, and a stop, each begin with nobody admitted', async () => {
  const env = await environment();
  await listing(env, 1);
  await hosting(env, 1, { players: 4, admitted: 3 });
  assert.equal((await liveNow(env)).body.sessions[0].players, 4);
  assert.equal(env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.get('adm_n'), 3);

  await beat(env, 1, { players: 4, pin: PIN2 });
  assert.equal((await liveNow(env)).body.sessions[0].players, 1, 'a new certificate is a new session, with nobody admitted to it');
  assert.equal(env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.has('adm_n'), false);

  await hosting(env, 1, { players: 3, admitted: 2, pin: PIN2 });
  assert.equal((await liveNow(env)).body.sessions[0].players, 3);
  await stop(env, 1);
  assert.equal(env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.has('adm_n'), false, 'a stop forgets the count');
  await beat(env, 1, { players: 6, pin: PIN2 });
  assert.equal((await liveNow(env)).body.sessions[0].players, 1);
});

// -- the cache ----------------------------------------------------------------------------------------------------------------------

test('the strip is computed once a window: a hammered URL costs one scan, and an age slice, an origin and a failure each behave', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2, { rating: 'adult_18' });
  await beat(env, 1);
  await beat(env, 2);
  const scans = [];
  const real = env.LIVE_INDEX.get.bind(env.LIVE_INDEX);
  env.LIVE_INDEX.get = (id) => { const stub = real(id); return { fetch: (url, init) => { if (url.endsWith('/list')) scans.push(url); return stub.fetch(url, init); } }; };
  const { cache, restore } = installCache();
  const t = clock();
  try {
    // Signed out, Adult content is off: the adult session is not on the strip (src/adult_content.mjs).
    const first = await liveNow(env);
    assert.equal(first.body.sessions_total, 1);
    for (let i = 0; i < 50; i++) assert.deepEqual((await liveNow(env)).body, first.body);
    assert.equal(scans.length, 1, 'fifty-one reads, one scan of the index');

    // Nothing a client adds to the URL gets its own entry or dodges the one that is there, and asking for the adult slice
    // signed out is the same strip.
    for (const query of ['?x=1', '?x=2&y=3', '?rating_max=nonsense', '?rating_max=', '?rating_max=ADULT_18', '?rating_max=adult_18', '?cachebust=' + Date.now()]) await liveNow(env, query);
    assert.equal(scans.length, 1, 'an unknown query string is the same strip');
    assert.equal(cache.store.size, 1);

    // One strip per age slice, and no more slices than there are: the adult slice only for a viewer who turned it on.
    for (const slice of ['all_ages', 'teen_13', 'mature_17']) await liveNow(env, `?rating_max=${slice}`);
    assert.equal(cache.store.size, 3);
    assert.equal(scans.length, 3);
    await adultOn(env);
    for (const query of ['', '?rating_max=adult_18', '?rating_max=nonsense']) assert.equal((await liveNow(env, query, { who: ADULT_VIEWER })).body.sessions_total, 2);
    assert.equal(cache.store.size, 4, 'the four slices');
    assert.equal(scans.length, 4);
    assert.equal((await liveNow(env, '?rating_max=all_ages', { who: ADULT_VIEWER })).body.sessions_total, 1, 'and the slices differ');
    assert.equal(scans.length, 4);

    // Another host's strip is its own.
    const staging = await liveNow(env, '', { origin: 'https://api-staging.eustress.dev' });
    assert.equal(staging.body.sessions_total, 1);
    assert.equal(scans.length, 5);

    // The response is built for each request: its CORS headers name the origin that asked, from the cache too.
    const fromSite = await liveNow(env, '', { headers: { Origin: 'https://eustress.dev' } });
    const fromLocal = await liveNow(env, '', { headers: { Origin: 'http://localhost:3000' } });
    assert.equal(fromSite.headers.get('Access-Control-Allow-Origin'), 'https://eustress.dev');
    assert.equal(fromLocal.headers.get('Access-Control-Allow-Origin'), 'http://localhost:3000');
    assert.equal(scans.length, 5, 'both from the cache');
    assert.equal(fromLocal.headers.get('Cache-Control'), 'public, max-age=10');

    // After the window the strip is computed again, and shows what changed.
    await stop(env, 1);
    assert.equal((await liveNow(env)).body.sessions_total, 1, 'inside the window: the earlier strip');
    t.advance(LIVE_INDEX_CACHE_SECONDS * 1000);
    assert.equal((await liveNow(env)).body.sessions_total, 0, 'after it: a new scan');
    assert.equal(scans.length, 6);
    assert.equal(LIVE_INDEX_CACHE_SECONDS, 10);

    // A failed scan is not kept: the next read tries again, and recovers at once.
    t.advance(LIVE_INDEX_CACHE_SECONDS * 1000);
    env.LIVE_INDEX.get = () => ({ fetch: () => { throw new Error('index is down'); } });
    const down = await quietly(() => liveNow(env));
    assert.equal(down.status, 503);
    assert.equal(cache.puts >= 1 && [...cache.store.values()].every((entry) => !entry.text.includes('live_unavailable')), true, 'the failure was not cached');
    env.LIVE_INDEX.get = real;
    assert.equal((await liveNow(env)).status, 200, 'recovered on the next read');
  } finally { t.restore(); restore(); }
});

test('a cache that fails costs a computation and never an answer', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1);
  const broken = { match: async () => { throw new Error('cache read failed'); }, put: async () => { throw new Error('cache write failed'); } };
  const { restore } = installCache(broken);
  try {
    const r = await quietly(() => liveNow(env));
    assert.equal(r.status, 200);
    assert.equal(r.body.sessions_total, 1);
  } finally { restore(); }
});

test('with no cache at all every read is computed, so a stop is seen at once', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1);
  assert.equal((await liveNow(env)).body.sessions_total, 1);
  await stop(env, 1);
  assert.equal((await liveNow(env)).body.sessions_total, 0);
});

// -- expiry and stop -------------------------------------------------------------------------------------------------------------

test('an entry that is not renewed expires 90 seconds after its last beat, by itself', async () => {
  const env = await environment();
  await listing(env, 1);
  const t = clock();
  try {
    await beat(env, 1);
    const object = indexObject(env);
    assert.equal(object.storage.alarm, t.now() + LIVE_INDEX_TTL_MS, 'an alarm waits for the expiry');
    assert.equal(LIVE_INDEX_TTL_MS, 90_000);

    t.advance(LIVE_INDEX_TTL_MS - 1);
    assert.equal((await liveNow(env)).body.sessions_total, 1, 'one millisecond before');
    t.advance(1);
    const gone = await liveNow(env);
    assert.deepEqual(gone.body, { sessions: [], players_total: 0, sessions_total: 0 }, 'gone at 90 seconds even before the alarm has run');
    assert.equal(indexRows(env).length, 1, 'still stored until the alarm clears it');

    await object.alarm();
    assert.equal(indexRows(env).length, 0, 'the alarm removed it');
  } finally { t.restore(); }
});

test('a late beat keeps the entry: an alarm that fires before the new deadline moves on to it and clears nothing', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  const t = clock();
  try {
    await beat(env, 1);
    const object = indexObject(env);
    const renewedAt = t.now() + 60_000;
    t.advance(60_000);
    await beat(env, 1);
    await beat(env, 2);
    t.advance(31_000);
    await object.alarm();
    assert.equal((await liveNow(env)).body.sessions_total, 2, 'both are live: the first beat was renewed');
    assert.equal(indexRows(env).length, 2);
    assert.equal(object.storage.alarm, renewedAt + LIVE_INDEX_TTL_MS, 'the alarm waits for the earliest remaining expiry');

    // Silence: entry 1 last beat 31 s ago, entry 2 the same. Past both deadlines everything goes.
    t.advance(LIVE_INDEX_TTL_MS);
    await object.alarm();
    assert.equal(indexRows(env).length, 0);
    assert.equal((await liveNow(env)).body.sessions_total, 0);

    // A host that comes back is live again from its next beat.
    await beat(env, 2);
    assert.equal((await liveNow(env)).body.sessions_total, 1);
  } finally { t.restore(); }
});

test('the alarm clears only what has expired and waits for the next one', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  const t = clock();
  try {
    await beat(env, 1);
    t.advance(40_000);
    await beat(env, 2);
    t.advance(50_000);
    const object = indexObject(env);
    await object.alarm();
    assert.deepEqual(indexRows(env).map((r) => r.sim_id), [idOf(2)], 'only the one that expired');
    assert.equal(object.storage.alarm, indexRows(env)[0].last_seen + LIVE_INDEX_TTL_MS, 'and the alarm is set for the one that remains');
  } finally { t.restore(); }
});

test('stopping removes the session at once, and leaves the others', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  await hosting(env, 1, { players: 2, admitted: 1 });
  await hosting(env, 2, { players: 3, admitted: 2 });
  assert.equal((await liveNow(env)).body.sessions_total, 2);
  assert.equal((await stop(env, 1)).status, 200);
  const r = await liveNow(env);
  assert.deepEqual(r.body.sessions.map((s) => s.id), [idOf(2)]);
  assert.equal(r.body.players_total, 3);
  assert.deepEqual(indexRows(env).map((row) => row.sim_id), [idOf(2)], 'removed from the index, not only hidden');
  assert.equal((await stop(env, 1)).status, 200, 'stopping again is fine');
  assert.deepEqual((await single(env, 1)).body, { live: false });
});

test('only the author can stop a session, and a refused stop leaves the index alone', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1);
  assert.equal((await stop(env, 1, 'stranger')).status, 403);
  assert.equal((await call(env, 'DELETE', `/api/simulations/${idOf(1)}/live`)).status, 401);
  assert.equal((await beat(env, 1, { who: 'stranger' })).status, 403);
  assert.equal((await liveNow(env)).body.sessions_total, 1);
});

// -- what is never in it ---------------------------------------------------------------------------------------------------------

test('no join link, key or pin is anywhere in the answer, or in what the index holds', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  await beat(env, 1, { cf: { country: 'US', colo: 'LAX' }, headers: { 'CF-Connecting-IP': '203.0.113.7' } });
  await beat(env, 2, { pin: PIN2 });
  const r = await liveNow(env);
  assert.equal(r.body.sessions.length, 2);
  for (const secret of [KEY, PIN, PIN2, LAN, 'eustress-player', '7777', 'key=', 'pin=', 'link', '203.0.113.7']) {
    assert.ok(!r.text.includes(secret), `the answer holds ${secret}`);
    assert.ok(!JSON.stringify(indexRows(env)).includes(secret), `the index holds ${secret}`);
  }
  // The single-listing read keeps its own rule: the link goes to a signed-in account only.
  const anonymous = await single(env, 1);
  assert.equal(anonymous.body.link, undefined);
  assert.equal(anonymous.body.link_hidden, true);
  assert.equal((await single(env, 1, 'stranger')).body.link, linkFor());
});

test('the index entry holds what the strip needs and the author it is capped by, and nothing else', async () => {
  const env = await environment();
  await listing(env, 1);
  const t = clock();
  try {
    await hosting(env, 1, { players: 2, admitted: 1, max: 6, cf: { country: 'US', colo: 'LAX' } });
    assert.deepEqual(indexRows(env), [{
      sim_id: idOf(1), author_id: AUTHOR, players: 2, max_players: 6, region: { country: 'US' }, since: t.now(), last_seen: t.now(),
    }]);
    const listed = await env.LIVE_INDEX.get('all').fetch('https://live-index.internal/list', { method: 'POST', body: JSON.stringify({ now: t.now() }) });
    const rows = (await listed.json()).sessions;
    assert.equal(rows.length, 1);
    assert.ok(!('author_id' in rows[0]), 'the list the Worker reads does not carry the author');
  } finally { t.restore(); }
});

// -- a failing index never fails a heartbeat ------------------------------------------------------------------------------------------

test('a deployment with no live index still takes heartbeats and stops, and says the live list is unavailable', async () => {
  const env = await environment({ index: false });
  await listing(env, 1);
  const logged = [];
  const realError = console.error;
  console.error = (...args) => { logged.push(args.join(' ')); };
  let r;
  try {
    assert.equal((await beat(env, 1)).status, 200);
    assert.equal((await single(env, 1)).body.live, true);
    r = await liveNow(env);
  } finally { console.error = realError; }
  assert.deepEqual(logged, [], 'a deployment without the index is a configuration, not a failure: nothing is logged as an error');
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'live_unavailable');
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
  assert.equal((await stop(env, 1)).status, 200);
  assert.deepEqual((await single(env, 1)).body, { live: false });
});

test('an index that throws, answers an error or answers nonsense fails nothing', async () => {
  const failures = {
    'throws': () => { throw new Error('index is down'); },
    'rejects': () => Promise.reject(new Error('index is down')),
    'answers 500': async () => new Response('{"ok":false,"error":"boom"}', { status: 500, headers: { 'content-type': 'application/json' } }),
    'answers text': async () => new Response('<html>nope</html>', { status: 200 }),
    'answers nothing': async () => undefined,
  };
  await quietly(async () => {
    for (const [what, fetch] of Object.entries(failures)) {
      const env = await environment();
      env.LIVE_INDEX = { idFromName: (name) => name, get: () => ({ fetch }) };
      await listing(env, 1);
      const b = await beat(env, 1, { players: 2 });
      assert.equal(b.status, 200, `a heartbeat when the index ${what}: ${b.text}`);
      assert.deepEqual(Object.keys(b.body), ['ok', 'expires_at']);
      assert.equal((await single(env, 1)).body.players, 1, `${what}: the host is live all the same (it claims 2, nobody was admitted)`);
      assert.equal((await stop(env, 1)).status, 200, `a stop when the index ${what}`);
      assert.deepEqual((await single(env, 1)).body, { live: false }, `${what}: and stopped`);
      const read = await liveNow(env);
      assert.equal(read.status, 503, `the list when the index ${what}`);
    }
  });
});

test('an index that never answers holds a heartbeat for its timeout and no longer', { timeout: 15_000 }, async () => {
  const env = await environment();
  env.LIVE_INDEX = { idFromName: (name) => name, get: () => ({ fetch: () => new Promise(() => {}) }) };
  await listing(env, 1);
  const started = performance.now();
  const b = await quietly(() => beat(env, 1, { players: 3 }));
  const took = performance.now() - started;
  assert.equal(b.status, 200);
  assert.ok(took >= LIVE_INDEX_CALL_TIMEOUT_MS - 100, `waited for the timeout (${Math.round(took)} ms)`);
  assert.ok(took < LIVE_INDEX_CALL_TIMEOUT_MS + 2500, `and not for much longer (${Math.round(took)} ms)`);
  assert.equal((await single(env, 1)).body.players, 1, 'live all the same (it claims 3, nobody was admitted)');
});

test('a refused heartbeat tells the index nothing', async () => {
  const env = await environment();
  await listing(env, 1);
  const told = [];
  const real = env.LIVE_INDEX.get.bind(env.LIVE_INDEX);
  env.LIVE_INDEX.get = (id) => { const stub = real(id); return { fetch: (url, init) => { told.push(url); return stub.fetch(url, init); } }; };
  assert.equal((await call(env, 'POST', `/api/simulations/${idOf(1)}/live`, { who: 'author', body: { link: 'https://example.com/', players: 1, max_players: 2, protocol: 3 } })).status, 400);
  assert.equal((await call(env, 'POST', `/api/simulations/${idOf(1)}/live`, { who: 'author', body: { link: linkFor(), players: -1, max_players: 2, protocol: 3 } })).status, 400);
  assert.equal((await beat(env, 1, { who: 'stranger' })).status, 403);
  assert.deepEqual(told, []);
  assert.equal((await beat(env, 1)).status, 200);
  assert.deepEqual(told, ['https://live-index.internal/beat']);
});

// -- the index object itself -----------------------------------------------------------------------------------------------------

test('the object refuses what is not a session, and an unknown operation', async () => {
  const env = await environment();
  const stub = env.LIVE_INDEX.get('all');
  const send = async (op, body) => {
    const res = await stub.fetch(`https://live-index.internal/${op}`, { method: 'POST', body: typeof body === 'string' ? body : JSON.stringify(body) });
    return { status: res.status, body: await res.json() };
  };
  const now = Date.now();
  const good = { sim_id: idOf(1), author: AUTHOR, players: 1, max_players: 2, since: now, now };
  for (const bad of [
    { ...good, sim_id: 'NOT AN ID' },
    { ...good, sim_id: 42 },
    { ...good, author: undefined },
    { ...good, author: 'has space' },
    { ...good, author: 'x'.repeat(200) },
    { ...good, author: 7 },
    { ...good, players: -1 },
    { ...good, max_players: 0 },
    { ...good, players: 1.5 },
    { ...good, max_players: 20_000 },
    { ...good, since: 'soon' },
    { ...good, now: undefined },
  ]) {
    const r = await send('beat', bad);
    assert.equal(r.status, 400, JSON.stringify(bad));
  }
  assert.equal(indexRows(env).length, 0);
  assert.equal((await send('beat', 'not json')).status, 400);
  assert.equal((await send('nothing', {})).status, 404);
  assert.equal((await send('stop', {})).status, 200);
  assert.deepEqual((await send('list', { now })).body, { ok: true, sessions: [] });

  // It cleans a region itself, whoever sends it, and keeps the country only.
  assert.equal((await send('beat', { ...good, region: { country: 'zz<', colo: 'x' } })).status, 200);
  assert.equal(indexRows(env)[0].region, null);
  await send('beat', { ...good, region: { country: 'FR', colo: 'CDG', city: 'Paris', ip: '203.0.113.9' } });
  assert.deepEqual(indexRows(env)[0].region, { country: 'FR' });
  assert.ok(!JSON.stringify(indexRows(env)).includes('Paris') && !JSON.stringify(indexRows(env)).includes('203.0.113') && !JSON.stringify(indexRows(env)).includes('CDG'));
  await send('beat', { ...good, region: 'FR' });
  assert.equal(indexRows(env)[0].region, null, 'a region that is not an object is no region');
});

test('the index and the host agree on how long a silent host stays: one number in two places that must not drift', () => {
  assert.equal(LIVE_INDEX_TTL_MS, HEARTBEAT_TTL_MS);
});

test('the route is public and named', () => {
  const r = routePolicy('GET', '/api/simulations/live');
  assert.equal(r.class, PUBLIC);
  assert.equal(r.explicit, true);
});

// -- region: the country only, validated, never a data centre and never an address ----------------------------------------------------------

test('a region is a two-letter upper-case country, or nothing, and a data centre is never part of it', () => {
  assert.deepEqual(cleanRegion('US'), { country: 'US' });
  assert.deepEqual(cleanRegion('DE', 'FRA'), { country: 'DE' }, 'a data centre passed in is not kept');
  assert.equal(cleanRegion(undefined), null);
  assert.equal(cleanRegion(null), null);
  for (const country of ['us', 'Us', 'USA', 'U', '', 'U1', '1U', 'T1', 'XX', '  ', 'U S', 'US\n', '<s>', 'DE; DROP', 42, null, {}, ['US'], true]) {
    assert.equal(cleanRegion(country), null, `country ${JSON.stringify(country)}`);
  }
  assert.deepEqual(regionOf({ cf: { country: 'US', colo: 'LAX', city: 'Los Angeles', latitude: '34.05', postalCode: '90001', asn: 1 } }), { country: 'US' }, 'only the country is read from the request');
  assert.deepEqual(regionOf({}), { country: undefined });
  assert.deepEqual(regionOf(null), { country: undefined });
  assert.deepEqual(publicRegion({ country: 'US', colo: 'PHX', city: 'Phoenix' }), { country: 'US' }, 'a record that carries more shows the country only');
  assert.equal(publicRegion({ colo: 'PHX' }), null);
  assert.equal(publicRegion(null), null);
  assert.equal(publicRegion('US'), null);
});

test('the single-listing read of a live host carries its country, and null when the platform did not say', async () => {
  const env = await environment();
  await listing(env, 1);
  await listing(env, 2);
  await beat(env, 1, { cf: { country: 'JP', colo: 'NRT' } });
  await beat(env, 2);
  assert.deepEqual((await single(env, 1)).body.region, { country: 'JP' });
  assert.equal((await single(env, 2)).body.region, null);
  assert.deepEqual((await single(env, 1, 'stranger')).body.region, { country: 'JP' }, 'the same to a signed-in account');
  assert.deepEqual(Object.keys((await single(env, 1)).body.region), ['country'], 'and nothing else about where');
  assert.deepEqual((await single(env, 2)).body.live, true);
  await stop(env, 1);
  assert.deepEqual((await single(env, 1)).body, { live: false }, 'a listing that is not live has no region to give');
});

test('a data centre is never published: not on the strip, not on the single read, and not even held by the host or the index', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1, { cf: { country: 'US', colo: 'PHX', city: 'Phoenix' } });
  const seen = [
    (await single(env, 1)).text, (await single(env, 1, 'stranger')).text, (await liveNow(env)).text,
    JSON.stringify([...env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.entries()]), JSON.stringify(indexRows(env)),
  ].join('\n');
  assert.ok(!seen.includes('PHX') && !seen.includes('colo') && !seen.includes('Phoenix'), 'no data centre anywhere');
  assert.ok(seen.includes('"country":"US"'));

  // A record from before the country-only rule, that carries a data centre, is shown as the country alone.
  const legacy = env.LIVE_HOSTS.instances.get(idOf(1));
  const host = await legacy.storage.get('host');
  await legacy.storage.put('host', { ...host, region: { country: 'US', colo: 'PHX' } });
  assert.deepEqual((await single(env, 1)).body.region, { country: 'US' });
  await env.LIVE_INDEX.get('all').fetch('https://live-index.internal/beat', {
    method: 'POST', body: JSON.stringify({ sim_id: idOf(1), author: AUTHOR, players: 1, max_players: 4, region: { country: 'US', colo: 'PHX' }, since: Date.now(), now: Date.now() }),
  });
  assert.ok(!(await liveNow(env)).text.includes('PHX'));
});

test('garbage from the platform is ignored and never stored, on the host or in the index', async () => {
  const garbage = [
    { country: 'usa', colo: 'LAX1' },
    { country: '<script>alert(1)</script>', colo: '"><img src=x>' },
    { country: 'U1', colo: 'la' },
    { country: 'XX', colo: undefined },
    { country: { evil: true }, colo: ['LAX'] },
    { country: 'x'.repeat(10_000), colo: 'y'.repeat(10_000) },
    { country: 'us', colo: 'lax' },
  ];
  for (const cf of garbage) {
    const env = await environment();
    await listing(env, 1);
    assert.equal((await beat(env, 1, { cf })).status, 200, JSON.stringify(cf).slice(0, 60));
    assert.equal((await single(env, 1)).body.region, null, JSON.stringify(cf).slice(0, 60));
    assert.equal((await liveNow(env)).body.sessions[0].region, null);
    const hostRecord = env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.get('host');
    assert.equal(hostRecord.region, null, 'not on the host');
    assert.equal(indexRows(env)[0].region, null, 'not in the index');
    const everything = JSON.stringify([...env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.entries(), ...indexRows(env)]);
    assert.ok(!everything.includes('script') && !everything.includes('evil') && !everything.includes('xxxx'), 'nothing of it is held anywhere');
  }
  // A good country is kept even when the data centre beside it is garbage.
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1, { cf: { country: 'US', colo: '<bad>' } });
  assert.deepEqual((await single(env, 1)).body.region, { country: 'US' });
  assert.deepEqual((await liveNow(env)).body.sessions[0].region, { country: 'US' });
});

test('a host that moves has its new country on its next beat, and a beat the platform could not place clears it', async () => {
  const env = await environment();
  await listing(env, 1);
  await beat(env, 1, { cf: { country: 'DE', colo: 'FRA' } });
  assert.deepEqual((await single(env, 1)).body.region, { country: 'DE' });
  await beat(env, 1, { cf: { country: 'US', colo: 'LAX' } });
  assert.deepEqual((await single(env, 1)).body.region, { country: 'US' });
  assert.deepEqual((await liveNow(env)).body.sessions[0].region, { country: 'US' });
  await beat(env, 1);
  assert.equal((await single(env, 1)).body.region, null, 'a stale place is not kept');
  assert.equal((await liveNow(env)).body.sessions[0].region, null);
});

test('no network address is stored or shown: the heartbeat carries one, and neither the host record nor the index keeps it', async () => {
  const env = await environment();
  await listing(env, 1);
  const headers = { 'CF-Connecting-IP': '203.0.113.7', 'X-Forwarded-For': '203.0.113.7', 'X-Real-IP': '203.0.113.7' };
  const cf = { country: 'US', colo: 'LAX', city: 'Tucson', postalCode: '85701', latitude: '32.22', longitude: '-110.97', asn: 7922, timezone: 'America/Phoenix' };
  await beat(env, 1, { cf, headers });
  const seen = [
    (await single(env, 1, 'stranger')).text,
    (await single(env, 1)).text,
    (await liveNow(env)).text,
    JSON.stringify([...env.LIVE_HOSTS.instances.get(idOf(1)).storage.map.entries()]),
    JSON.stringify(indexRows(env)),
  ].join('\n');
  for (const leak of ['203.0.113', 'Tucson', '85701', '32.22', '110.97', '7922', 'Phoenix', 'LAX']) assert.ok(!seen.includes(leak), `${leak} is held or shown`);
});

test('the heartbeat answers what it always did, and the live read keeps every field it had', async () => {
  const env = await environment();
  await listing(env, 1);
  const b = await hosting(env, 1, { players: 2, admitted: 1, max: 5, cf: { country: 'US', colo: 'LAX' } });
  assert.deepEqual(Object.keys(b.body), ['ok', 'expires_at']);
  assert.equal(b.body.ok, true);
  assert.ok(Date.parse(b.body.expires_at) > Date.now());
  const read = (await single(env, 1, 'stranger')).body;
  for (const field of ['live', 'players', 'max_players', 'protocol', 'since', 'link']) assert.ok(field in read, `${field} is still there`);
  assert.deepEqual([read.live, read.players, read.max_players, read.protocol, read.link], [true, 2, 5, 3, linkFor()]);
});
