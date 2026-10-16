// Makers: GET /api/makers?q=&limit= answers the people who have at least one LISTED listing, for the Gallery's search
// (src/makers.mjs). The list is computed from the list route's own scan, so a listing counts exactly when the Gallery serves it.
// Most listed Spaces first, then most visits, then username. No id, no email, nothing an account holds beyond what a listing's
// author_name already shows. The real Worker runs against in-memory storage.
//
// Source note: no backslash-u escape appears in this file on purpose; every non-ASCII character is built from its code point.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { installCache } from './helpers/fake_cache.mjs';
import { makersQuery, MAKERS_DEFAULT_LIMIT, MAKERS_MAX_LIMIT } from '../src/makers.mjs';
import { routePolicy, PUBLIC } from '../src/route_policy.mjs';

const worker = await loadWorker();

const FULL_WIDTH_A = String.fromCodePoint(0xff41);
const FULL_WIDTH_D = String.fromCodePoint(0xff44);
const ROCKET = String.fromCodePoint(0x1f680);

const idOf = (n) => `00000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
const accountId = (n) => `99999999-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;

/// A world with a set of accounts and listings. `makers` is a list of { name, listings: [{ status, public, plays, visits, rating, archived }], avatar }.
async function world(makers) {
  const env = fakeEnv();
  let n = 0;
  for (const [index, maker] of makers.entries()) {
    const owner = accountId(index + 1);
    maker.account = owner;
    await env.USERS.put(`user:${owner}`, JSON.stringify({
      id: owner, username: maker.account_name ?? maker.name, email: `${maker.name}@example.com`, avatar_url: maker.avatar, created_at: '2026-01-01T00:00:00.000Z',
    }));
    for (const spec of maker.listings) {
      n += 1;
      const id = idOf(n);
      spec.id = id;
      await env.SOCIAL.put(`sim:${id}`, JSON.stringify({
        id, name: `World ${n}`, description: '', genre: 'all', max_players: 4, author_id: owner, author_name: maker.name,
        is_public: spec.public !== false, moderation: { status: spec.status || 'approved', rating: spec.rating || 'all_ages' },
        play_count: spec.plays || 0, favorite_count: 0, version: 1, published_at: `2026-03-${String((spec.day || n) % 28 + 1).padStart(2, '0')}T00:00:00.000Z`,
        updated_at: '2026-03-01T00:00:00.000Z', ...(spec.archived ? { archived: true } : {}),
      }));
      if (spec.visits) await env.SOCIAL.put(`visits:${id}`, String(spec.visits));
    }
  }
  return env;
}

/// An account that turned Adult content on (src/adult_content.mjs), set in storage as the setting's route leaves it.
const ADULT_VIEWER = '44444444-4444-4444-8444-444444444444';
const adultOn = (env) => env.USERS.put(`adult-content:${ADULT_VIEWER}`, '1');
const ADULT_TOKEN = mintToken(ADULT_VIEWER);

async function read(env, query = '', { origin = 'https://api.eustress.dev', token } = {}) {
  const res = await worker.fetch(new Request(`${origin}/api/makers${query}`, { headers: token ? { Authorization: `Bearer ${token}` } : {} }), env, ctx);
  const text = await res.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body };
}
const names = (r) => r.body.makers.map((m) => m.username);
const listed = (count, extra = {}) => Array.from({ length: count }, () => ({ ...extra }));

// -- the shape ----------------------------------------------------------------------------------------------------------------------

test('makers are the people with a listed listing: a name, an avatar, how many listed Spaces, how many visits, and nothing else', async () => {
  const env = await world([
    { name: 'ada', avatar: 'https://cdn.example.com/ada.png', listings: [{ plays: 4, visits: 3 }] },
    { name: 'bob', listings: [{}] },
  ]);
  const r = await read(env);
  assert.equal(r.status, 200, r.text);
  assert.deepEqual(Object.keys(r.body), ['makers']);
  assert.deepEqual(r.body.makers, [
    { username: 'ada', avatar_url: 'https://cdn.example.com/ada.png', space_count: 1, visits: 7 },
    { username: 'bob', avatar_url: null, space_count: 1, visits: 0 },
  ]);
  for (const maker of r.body.makers) assert.deepEqual(Object.keys(maker), ['username', 'avatar_url', 'space_count', 'visits']);
});

test('no account id, no email and no listing id is in the answer, however the response is read', async () => {
  const env = await world([{ name: 'ada', avatar: 'https://cdn.example.com/ada.png', listings: [{ plays: 2 }, { plays: 1 }] }, { name: 'bob', listings: [{}] }]);
  const r = await read(env);
  assert.equal(r.body.makers.length, 2);
  assert.ok(!/99999999-0000/.test(r.text), 'no account id');
  assert.ok(!/00000000-0000-4000/.test(r.text), 'no listing id');
  assert.ok(!/@example\.com/.test(r.text) && !/email/i.test(r.text), 'no email');
  assert.ok(!/author_id|"id"|account/i.test(r.text));
});

test('the answer is cached for five minutes and varies by origin, like the popular tags', async () => {
  const env = await world([{ name: 'ada', listings: [{}] }]);
  const r = await read(env);
  assert.equal(r.headers.get('Cache-Control'), 'public, max-age=300');
  assert.equal(r.headers.get('Vary'), 'Origin, Authorization', 'the signed-out answer is kept apart from any session (src/adult_content.mjs)');
  assert.equal(r.headers.get('Content-Type'), 'application/json');
  const empty = await read(await world([]));
  assert.equal(empty.status, 200);
  assert.deepEqual(empty.body, { makers: [] });
  assert.equal(empty.headers.get('Cache-Control'), 'public, max-age=300');
});

test('the route is public: no session is asked for, and it is named so', async () => {
  const env = await world([{ name: 'ada', listings: [{}] }]);
  assert.equal((await read(env)).status, 200);
  const r = routePolicy('GET', '/api/makers');
  assert.equal(r.class, PUBLIC);
  assert.equal(r.explicit, true);
});

// -- the order ----------------------------------------------------------------------------------------------------------------------

test('most listed Spaces first, then most visits, then username', async () => {
  const env = await world([
    { name: 'amy', listings: [{ visits: 5 }] },
    { name: 'zed', listings: [{ visits: 5 }] },
    { name: 'carol', listings: [{ visits: 999 }] },
    { name: 'dan', listings: [{ visits: 1 }, { visits: 1 }, { visits: 1 }] },
    { name: 'eve', listings: [{ visits: 40 }, { visits: 1 }, { visits: 1 }] },
    { name: 'Bob', listings: [{ visits: 5 }] },
    { name: 'alice', listings: [{ visits: 5 }] },
  ]);
  const r = await read(env);
  assert.deepEqual(names(r), ['eve', 'dan', 'carol', 'alice', 'amy', 'Bob', 'zed'], 'spaces, then visits, then name without regard to case');
  assert.deepEqual(r.body.makers.map((m) => [m.space_count, m.visits]), [[3, 42], [3, 3], [1, 999], [1, 5], [1, 5], [1, 5], [1, 5]]);
});

test('visits are what a reader of the Gallery sees: the record\'s count plus the visits counted since, over every listed listing', async () => {
  const env = await world([{ name: 'ada', listings: [{ plays: 10, visits: 4 }, { plays: 0, visits: 6 }, { plays: 3 }] }]);
  assert.equal((await read(env)).body.makers[0].visits, 23);
  const gallery = (await worker.fetch(new Request('https://api.eustress.dev/api/simulations'), env, ctx));
  const total = (await gallery.json()).simulations.reduce((sum, s) => sum + s.play_count, 0);
  assert.equal(total, 23, 'the Gallery adds up to the same number');
});

test('an author with two listings appears once, with both counted', async () => {
  const env = await world([{ name: 'ada', listings: [{ visits: 2 }, { visits: 3 }] }, { name: 'bob', listings: [{}] }]);
  const r = await read(env);
  assert.deepEqual(names(r), ['ada', 'bob']);
  assert.deepEqual(r.body.makers[0], { username: 'ada', avatar_url: null, space_count: 2, visits: 5 });
});

test('the name shown is the one on the listing, newest first, and a listing with no author name names nobody', async () => {
  const env = await world([{ name: 'ada', account_name: 'ada-renamed', listings: [{ day: 1 }, { day: 5 }] }]);
  assert.deepEqual(names(await read(env)), ['ada']);
  const nameless = await world([{ name: '', listings: [{}] }, { name: 'bob', listings: [{}] }]);
  assert.deepEqual(names(await read(nameless)), ['bob']);
});

// -- only what is listed ------------------------------------------------------------------------------------------------------------

test('only listed listings count: unlisted, archived, pending, rejected and quarantined ones are nobody\'s', async () => {
  const env = await world([
    { name: 'listed', listings: [{ visits: 7 }] },
    { name: 'pending', listings: [{ status: 'pending' }] },
    { name: 'rejected', listings: [{ status: 'rejected' }] },
    { name: 'quarantined', listings: [{ status: 'quarantined' }] },
    { name: 'private', listings: [{ public: false }] },
    { name: 'archived', listings: [{ public: false, archived: true }] },
    { name: 'mixed', listings: [{ visits: 1 }, { status: 'pending', visits: 500 }, { status: 'quarantined', visits: 500 }, { public: false, visits: 500 }, { archived: true, public: false, visits: 500 }] },
  ]);
  const r = await read(env);
  assert.deepEqual(r.body.makers.map((m) => m.username).sort(), ['listed', 'mixed']);
  const mixed = r.body.makers.find((m) => m.username === 'mixed');
  assert.deepEqual([mixed.space_count, mixed.visits], [1, 1], 'one listed listing, and only its visits');
  assert.equal(r.body.makers.find((m) => m.username === 'listed').visits, 7);
});

test('a legal lock removes a maker the way it removes a listing from the Gallery', async () => {
  const env = await world([{ name: 'locked', listings: [{}] }, { name: 'free', listings: [{}] }]);
  const lockedId = idOf(1);
  await env.SCENES.put(`locks/legal/${lockedId}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.deepEqual(names(await read(env)), ['free']);
  const gallery = (await (await worker.fetch(new Request('https://api.eustress.dev/api/simulations'), env, ctx)).json()).simulations;
  assert.deepEqual(gallery.map((s) => s.author_name), ['free'], 'and the Gallery agrees');
});

test('the viewer\'s age slice is honoured the way the list route honours it: a maker whose Spaces are above it is not named', async () => {
  const env = await world([
    { name: 'kids', listings: [{ rating: 'all_ages' }] },
    { name: 'teens', listings: [{ rating: 'teen_13' }] },
    { name: 'adults', listings: [{ rating: 'adult_18' }] },
    { name: 'both', listings: [{ rating: 'all_ages' }, { rating: 'adult_18' }] },
  ]);
  // Signed out, Adult content is off: a maker whose only Spaces are adult content is not named (src/adult_content.mjs).
  assert.deepEqual(names(await read(env)).sort(), ['both', 'kids', 'teens']);
  assert.deepEqual(names(await read(env, '?rating_max=adult_18')).sort(), ['both', 'kids', 'teens'], 'rating_max cannot widen past the setting');
  await adultOn(env);
  const on = { token: ADULT_TOKEN };
  assert.deepEqual((await read(env, '', on)).body.makers.map((m) => m.username).sort(), ['adults', 'both', 'kids', 'teens']);
  const all = await read(env, '?rating_max=all_ages', on);
  assert.deepEqual(names(all).sort(), ['both', 'kids']);
  assert.equal(all.body.makers.find((m) => m.username === 'both').space_count, 1, 'only the all-ages Space counts');
  assert.deepEqual(names(await read(env, '?rating_max=teen_13', on)).sort(), ['both', 'kids', 'teens']);
  assert.equal((await read(env, '?rating_max=nonsense', on)).body.makers.length, 4, 'a value that is not a rating asks for no slice');
});

// -- the limit ----------------------------------------------------------------------------------------------------------------------

test('the limit is 8 unless asked, at least 1, and at most 24', async () => {
  const many = await world(Array.from({ length: 30 }, (_, i) => ({ name: `maker${String(i).padStart(2, '0')}`, listings: [{}] })));
  assert.equal(MAKERS_DEFAULT_LIMIT, 8);
  assert.equal(MAKERS_MAX_LIMIT, 24);
  assert.equal(names(await read(many)).length, 8, 'default');
  assert.equal(names(await read(many, '?limit=3')).length, 3);
  assert.equal(names(await read(many, '?limit=24')).length, 24);
  assert.equal(names(await read(many, '?limit=25')).length, 24, 'clamped, not refused');
  assert.equal(names(await read(many, '?limit=100000')).length, 24);
  assert.equal(names(await read(many, '?limit=0')).length, 1);
  assert.equal(names(await read(many, '?limit=-5')).length, 1);
  assert.equal(names(await read(many, '?limit=abc')).length, 8, 'not a number: the default');
  assert.equal(names(await read(many, '?limit=')).length, 8);
  assert.equal(names(await read(many, '?limit=2.9')).length, 2);
  assert.deepEqual(names(await read(many, '?limit=4')), ['maker00', 'maker01', 'maker02', 'maker03'], 'the first of the order');
  const few = await world([{ name: 'ada', listings: [{}] }]);
  assert.equal(names(await read(few, '?limit=24')).length, 1);
  assert.deepEqual(makersQuery(new URLSearchParams('limit=7')), { q: '', limit: 7 });
});

// -- the search ---------------------------------------------------------------------------------------------------------------------

test('q is a case-insensitive substring of the username, and one leading @ is ignored', async () => {
  const env = await world([
    { name: 'Ada_Lovelace', listings: [{ visits: 3 }] },
    { name: 'adam', listings: [{ visits: 2 }] },
    { name: 'bob', listings: [{ visits: 1 }] },
  ]);
  assert.deepEqual(names(await read(env, '?q=ada')), ['Ada_Lovelace', 'adam']);
  assert.deepEqual(names(await read(env, '?q=ADA')), ['Ada_Lovelace', 'adam']);
  assert.deepEqual(names(await read(env, '?q=LOVE')), ['Ada_Lovelace'], 'inside the name');
  assert.deepEqual(names(await read(env, '?q=elace')), ['Ada_Lovelace'], 'at the end');
  assert.deepEqual(names(await read(env, '?q=%40ada')), ['Ada_Lovelace', 'adam'], 'a leading @');
  assert.deepEqual(names(await read(env, '?q=%40%20Ada')), ['Ada_Lovelace', 'adam'], 'a leading @ and a space');
  assert.deepEqual(names(await read(env, '?q=%20%20bob%20')), ['bob'], 'spaces around');
  assert.deepEqual(names(await read(env, '?q=')), ['Ada_Lovelace', 'adam', 'bob'], 'an empty q is no filter');
  assert.deepEqual(names(await read(env, '?q=%40')), ['Ada_Lovelace', 'adam', 'bob'], 'a lone @ is an empty q');
  assert.deepEqual(names(await read(env, '?q=%20%0A%09')), ['Ada_Lovelace', 'adam', 'bob'], 'so is text of nothing but spaces');
  assert.deepEqual(names(await read(env, '?q=ada&limit=1')), ['Ada_Lovelace'], 'q and limit together');
});

test('q is compared as normalized text: full-width letters match, and text that matches nobody is an empty list, never an error', async () => {
  const env = await world([{ name: 'ada', listings: [{}] }, { name: 'bob', listings: [{}] }]);
  assert.deepEqual(names(await read(env, `?q=${encodeURIComponent(FULL_WIDTH_A + FULL_WIDTH_D + FULL_WIDTH_A)}`)), ['ada'], 'NFKC folds full-width letters');
  const unmatchable = [
    'zzz-no-one', '%40%40ada', 'ada%20bob', '.*', '%5B', '%5C', '(', '%25', 'a%0Ab', '%00', '%3Cscript%3E', encodeURIComponent(ROCKET), '%ED%A0%BD',
    'x'.repeat(5000), `%40${'y'.repeat(300)}`, '%EF%BF%BD',
  ];
  for (const q of unmatchable) {
    const r = await read(env, `?q=${q}`);
    assert.equal(r.status, 200, `q=${q.slice(0, 40)}`);
    assert.deepEqual(r.body, { makers: [] }, `q=${q.slice(0, 40)}`);
  }
  assert.equal((await read(env, '?q=ada&q=bob')).status, 200, 'a repeated parameter is read as the first');
});

test('the search does not narrow the order, only the set', async () => {
  const env = await world([
    { name: 'ana', listings: [{ visits: 1 }] },
    { name: 'anna', listings: [{ visits: 9 }, { visits: 1 }] },
    { name: 'anne', listings: [{ visits: 5 }] },
  ]);
  assert.deepEqual(names(await read(env, '?q=an')), ['anna', 'anne', 'ana']);
});

// -- the avatar ---------------------------------------------------------------------------------------------------------------------

test('the avatar is the account\'s avatar_url, as the profile reads show it, and null when there is none or it is not a web address', async () => {
  const env = await world([
    { name: 'a-https', avatar: 'https://cdn.example.com/a.png', listings: [{ visits: 9 }] },
    { name: 'b-none', listings: [{ visits: 8 }] },
    { name: 'c-http', avatar: 'http://cdn.example.com/c.png', listings: [{ visits: 7 }] },
    { name: 'd-script', avatar: 'javascript:alert(1)', listings: [{ visits: 6 }] },
    { name: 'e-data', avatar: 'data:image/svg+xml;base64,PHN2Zy8+', listings: [{ visits: 5 }] },
    { name: 'f-number', avatar: 42, listings: [{ visits: 4 }] },
    { name: 'g-space', avatar: 'https://cdn.example.com/has space.png', listings: [{ visits: 3 }] },
    { name: 'h-long', avatar: `https://cdn.example.com/${'a'.repeat(600)}`, listings: [{ visits: 2 }] },
  ]);
  const byName = Object.fromEntries((await read(env, '?limit=24')).body.makers.map((m) => [m.username, m.avatar_url]));
  assert.equal(byName['a-https'], 'https://cdn.example.com/a.png');
  for (const name of ['b-none', 'c-http', 'd-script', 'e-data', 'f-number', 'g-space', 'h-long']) assert.equal(byName[name], null, name);

  const gone = await world([{ name: 'ada', avatar: 'https://cdn.example.com/a.png', listings: [{}] }]);
  await gone.USERS.delete(`user:${accountId(1)}`);
  assert.deepEqual((await read(gone)).body.makers, [{ username: 'ada', avatar_url: null, space_count: 1, visits: 0 }], 'an account record that cannot be read costs the avatar and nothing else');
  const broken = await world([{ name: 'ada', listings: [{}] }]);
  await broken.USERS.put(`user:${accountId(1)}`, '{not json');
  assert.equal((await read(broken)).body.makers[0].avatar_url, null);
});

// -- the cache ----------------------------------------------------------------------------------------------------------------------

test('the scan is done once a window: a hammered URL, with any q and limit, costs one scan, and no query string adds an entry', async () => {
  const env = await world([
    { name: 'ada', avatar: 'https://cdn.example.com/ada.png', listings: [{ visits: 3 }, { visits: 2, rating: 'adult_18' }] },
    { name: 'bob', listings: [{}] },
  ]);
  let scans = 0;
  const realList = env.SOCIAL.list.bind(env.SOCIAL);
  env.SOCIAL.list = async (options) => { if (options?.prefix === 'sim:') scans += 1; return realList(options); };
  const { cache, restore } = installCache();
  const realNow = Date.now;
  let now = realNow();
  Date.now = () => now;
  try {
    const first = await read(env);
    assert.equal(first.body.makers.length, 2);
    assert.equal(scans, 1);
    for (let i = 0; i < 40; i++) assert.deepEqual((await read(env)).body, first.body);
    for (const query of ['?q=a', '?q=ada', '?q=%40bob', '?limit=1', '?limit=24', '?q=zzz', `?q=${'x'.repeat(500)}`, '?x=1', `?cachebust=${Date.now()}`, '?q=a&limit=1&x=2']) {
      const r = await read(env, query);
      assert.equal(r.status, 200, query);
    }
    assert.equal(scans, 1, 'fifty reads with every q and limit, one scan');
    assert.equal(cache.store.size, 1, 'and one cache entry');
    assert.deepEqual(names(await read(env, '?q=bob')), ['bob'], 'q and limit are applied to what the cache holds');
    assert.deepEqual(names(await read(env, '?limit=1')), ['ada']);

    // One entry per age slice, and no more slices than there are. Signed out, Adult content is off, so asking for the adult
    // slice is the mature_17 one (src/adult_content.mjs); a viewer who turned it on has the fourth.
    for (const slice of ['all_ages', 'teen_13', 'mature_17', 'adult_18', 'nonsense', '', 'ALL_AGES']) await read(env, `?rating_max=${slice}`);
    assert.equal(cache.store.size, 3, 'three slices signed out');
    await adultOn(env);
    for (const slice of ['adult_18', 'nonsense', '']) await read(env, `?rating_max=${slice}`, { token: ADULT_TOKEN });
    assert.equal(cache.store.size, 4, 'the four slices');
    assert.equal((await read(env, '?rating_max=all_ages', { token: ADULT_TOKEN })).body.makers.find((m) => m.username === 'ada').space_count, 1, 'and each slice is its own answer');
    assert.equal((await read(env, '', { token: ADULT_TOKEN })).body.makers.find((m) => m.username === 'ada').space_count, 2);
    assert.equal((await read(env)).body.makers.find((m) => m.username === 'ada').space_count, 1, 'signed out: the adult Space is not counted');

    // Another host has its own entry, and the response is built for each request, so CORS names the origin that asked.
    scans = 0;
    await read(env, '', { origin: 'https://api-staging.eustress.dev' });
    assert.equal(scans, 1);
    const fromSite = await worker.fetch(new Request('https://api.eustress.dev/api/makers', { headers: { Origin: 'https://eustress.dev' } }), env, ctx);
    const fromLocal = await worker.fetch(new Request('https://api.eustress.dev/api/makers', { headers: { Origin: 'http://localhost:3000' } }), env, ctx);
    assert.equal(fromSite.headers.get('Access-Control-Allow-Origin'), 'https://eustress.dev');
    assert.equal(fromLocal.headers.get('Access-Control-Allow-Origin'), 'http://localhost:3000');
    assert.equal(fromLocal.headers.get('Cache-Control'), 'public, max-age=300');
    assert.equal(scans, 1, 'both answered from the cache');

    // What the cache holds is no more than the answer shows: no account id, no listing id, no email.
    const held = [...cache.store.values()].map((entry) => entry.text).join('\n');
    assert.ok(!/99999999-0000|00000000-0000-4000|@example\.com|email|author_id/.test(held), held.slice(0, 200));

    // After the window the scan is done again and shows what changed.
    scans = 0;
    await env.SOCIAL.put(`sim:${idOf(3)}`, JSON.stringify({ ...JSON.parse(await env.SOCIAL.get(`sim:${idOf(3)}`)), moderation: { status: 'pending' } }));
    assert.equal((await read(env)).body.makers.length, 2, 'inside the window: the earlier answer');
    assert.equal(scans, 0);
    now += 10_000;
    assert.deepEqual(names(await read(env)), ['ada'], 'after it: a new scan');
    assert.equal(scans, 1);
  } finally { Date.now = realNow; restore(); }
});

test('a cache that fails costs a scan and never an answer', async () => {
  const env = await world([{ name: 'ada', listings: [{}] }]);
  const { restore } = installCache({ match: async () => { throw new Error('cache read failed'); }, put: async () => { throw new Error('cache write failed'); } });
  const real = console.error;
  console.error = () => {};
  try {
    const r = await read(env);
    assert.equal(r.status, 200);
    assert.deepEqual(names(r), ['ada']);
  } finally { console.error = real; restore(); }
});
