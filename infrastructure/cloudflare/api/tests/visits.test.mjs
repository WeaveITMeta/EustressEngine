// A listing's visit count: one launch or one join, once a day per person or address, per listing. The play route (it only says how
// to open a listing) and every read that fetches nothing count nothing. A visit never rewrites the listing record; the count is its
// own key, added to the record's own count when the listing is read. An unauthenticated flood is held to one count per address and to a
// per-listing allowance of new addresses. The real Worker runs against in-memory storage. src/visits.mjs.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { countVisit, viewerKey, deviceKey, playCountOf, visitsOf } from '../src/visits.mjs';
import { manifestKey } from '../src/world.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const ADA = '22222222-2222-4222-8222-222222222222';
const BOB = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const LINK = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${PIN}`;
const DAY = 24 * 3600 * 1000;
const tokens = { author: mintToken(AUTHOR), ada: mintToken(ADA), bob: mintToken(BOB) };

// Rated all_ages: a listing with no rating is adult content, played only with Adult content on (src/adult_content.mjs).
async function environment({ format = 'pak', moderation = { status: 'approved', rating: 'all_ages' }, play_count = 0 } = {}) {
  const env = fakeEnv();
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  for (const [id, name] of [[AUTHOR, 'host'], [ADA, 'ada'], [BOB, 'bob']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  const sim = { id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation, play_count, format };
  if (format === 'pak') {
    sim.r2_key = `universes/${SIM}/universe.pak`;
    await env.SCENES.put(sim.r2_key, 'the world');
  } else {
    sim.world = { manifest_key: manifestKey(SIM, 'f'.repeat(64)), manifest_sha256: 'f'.repeat(64) };
    sim.r2_key = sim.world.manifest_key;
    await env.SCENES.put(sim.world.manifest_key, JSON.stringify({ format: 'echk' }));
  }
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  return env;
}

async function call(env, method, path, { who, body, headers, token } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  else if (who) h.Authorization = `Bearer ${tokens[who]}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: parsed, text };
}

/// The count a reader sees, through the route that serves it (the record's own count plus what has been counted since).
const plays = async (env) => (await call(env, 'GET', `/api/simulations/${SIM}`, { who: 'author' })).body.play_count;
const authorTotal = async (env) => Number((await env.SOCIAL.get(`totalPlays:${AUTHOR}`)) || 0);
const later = async (ms, fn) => { const real = Date.now; Date.now = () => real() + ms; try { return await fn(); } finally { Date.now = real; } };

/// Every key a run writes to storage, in order.
function watchWrites(env) {
  const writes = [];
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (k, v, o) => { writes.push(k); return real(k, v, o); };
  return writes;
}

test('the play route says how to open a listing and counts nothing, however often it is asked', async () => {
  const env = await environment();
  for (let i = 0; i < 4; i++) assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/play`, { who: 'ada' })).status, 200);
  assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/play`)).status, 200, 'a visitor with no account too');
  assert.equal(await plays(env), 0);
  assert.equal(await authorTotal(env), 0);
});

test('fetching the .pak to play it is a visit, once a day per person, and the author\'s total follows', async () => {
  const env = await environment();
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' })).status, 200);
  assert.equal(await plays(env), 1);
  assert.equal(await authorTotal(env), 1);
  // Retries, a restarted Player, a second tab: the same person the same day.
  for (let i = 0; i < 3; i++) await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' });
  assert.equal(await plays(env), 1);
  // Another person counts apart.
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'bob' });
  assert.equal(await plays(env), 2);
  assert.equal(await authorTotal(env), 2);
  // The next day the same person counts again (a session minted that day: Ada is signed in then too, so this is her and not a device).
  await later(DAY + 60_000, () => call(env, 'GET', `/api/simulations/${SIM}/download`, { token: mintToken(ADA) }));
  assert.equal(await plays(env), 3);
  // And the same person twice that day is still one.
  await later(DAY + 120_000, () => call(env, 'GET', `/api/simulations/${SIM}/download`, { token: mintToken(ADA) }));
  assert.equal(await plays(env), 3);
  assert.ok([...env.SOCIAL.store.keys()].filter((k) => k.includes(`:u:${ADA}`)).length === 2, 'one marker per day');
});

test('the count a reader sees is the record\'s own count plus the visits counted since, and the record is not touched to count one', async () => {
  const env = await environment({ play_count: 41 });
  assert.equal(await plays(env), 41, 'before any visit');
  const before = await env.SOCIAL.get(`sim:${SIM}`);
  const writes = watchWrites(env);
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' });
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'bob' });
  assert.equal(await plays(env), 43);
  assert.equal(await visitsOf(env, SIM), 2);
  assert.equal(await playCountOf(env, JSON.parse(before)), 43);
  assert.ok(!writes.some((k) => k === `sim:${SIM}`), `a visit never rewrites the listing record: ${writes.join(', ')}`);
  assert.equal(await env.SOCIAL.get(`sim:${SIM}`), before, 'the record is byte-identical');
  // The public list and the author's projects say the same number.
  const listed = (await call(env, 'GET', '/api/simulations')).body.simulations.find((s) => s.id === SIM);
  assert.equal(listed.play_count, 43);
  const mine = (await call(env, 'GET', '/api/projects', { who: 'author' })).body.projects.find((p) => p.id === SIM);
  assert.equal(mine.play_count, 43);
});

test('fetching the world manifest of an .echk listing is a visit, with the same once-a-day rule', async () => {
  const env = await environment({ format: 'echk' });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`, { who: 'ada' })).status, 200);
  assert.equal(await plays(env), 1);
  await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`, { who: 'ada' });
  assert.equal(await plays(env), 1);
  await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`, { who: 'bob' });
  assert.equal(await plays(env), 2);
});

test('someone not signed in counts once a day per ADDRESS: 60 browser strings from one address are one visit, and no address is stored', async () => {
  const env = await environment();
  const writes = watchWrites(env);
  for (let i = 0; i < 60; i++) await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: { 'CF-Connecting-IP': '203.0.113.7', 'User-Agent': `Agent/${i}` } });
  assert.equal(await plays(env), 1, 'sixty browser strings, one address');
  // One marker, one counter, one author total: three writes in all, never a listing write.
  assert.equal(writes.length, 3, writes.join(', '));
  assert.ok(!writes.includes(`sim:${SIM}`));
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: { 'CF-Connecting-IP': '203.0.113.8', 'User-Agent': 'Agent/1' } });
  assert.equal(await plays(env), 2, 'another address');
  const markers = [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('visit:'));
  assert.equal(markers.length, 2);
  assert.ok(markers.every((k) => !k.includes('203.0.113')), 'the marker holds a keyed hash, never the address');
});

test('new anonymous visits to one listing are held to the platform limiter\'s allowance; people and returning addresses are not', async () => {
  const env = await environment();
  let allowance = 3;
  const asked = [];
  env.VISIT_RATE_LIMITER = { limit: async ({ key }) => { asked.push(key); return { success: allowance-- > 0 }; } };
  for (let i = 0; i < 10; i++) await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: { 'CF-Connecting-IP': `198.51.100.${i}` } });
  assert.equal(await plays(env), 3, 'ten new addresses, the allowance said three');
  assert.ok(asked.every((k) => k === SIM), 'keyed on the listing');
  // A returning address does not ask again: its marker is found first.
  const askedBefore = asked.length;
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: { 'CF-Connecting-IP': '198.51.100.0' } });
  assert.equal(asked.length, askedBefore);
  // Signed-in people are not subject to it.
  allowance = 0;
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' });
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'bob' });
  assert.equal(await plays(env), 5);
  // A refused count never refuses the launch.
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: { 'CF-Connecting-IP': '198.51.100.99' } })).status, 200);
});

test('an address is keyed under a key derived for this purpose, not under the Worker\'s own secret', async () => {
  const { createHmac } = await import('node:crypto');
  const secret = 'a secret';
  const subkey = createHmac('sha256', secret).update('eustress-visit-v1').digest();
  const expected = `d:${createHmac('sha256', subkey).update('203.0.113.7').digest('hex').slice(0, 32)}`;
  assert.equal(await deviceKey({ JWT_SECRET: secret }, '203.0.113.7'), expected);
  const direct = `d:${createHmac('sha256', secret).update('203.0.113.7').digest('hex').slice(0, 32)}`;
  assert.notEqual(await deviceKey({ JWT_SECRET: secret }, '203.0.113.7'), direct, 'the secret itself signs nothing here');
  assert.notEqual(await deviceKey({ JWT_SECRET: 'another' }, '203.0.113.7'), expected);
  assert.equal(await deviceKey({}, '203.0.113.7'), 'd:unknown');
  assert.equal(await deviceKey({ JWT_SECRET: secret }, ''), 'd:unknown');
});

test('a read that fetches nothing, or is refused, counts nothing', async () => {
  const env = await environment({ moderation: { status: 'pending' } });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' })).status, 403, 'a stranger on an unapproved listing');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal(await visitsOf(env, SIM), 0);
  const approved = await environment();
  await call(approved, 'GET', `/api/simulations/${SIM}`, { who: 'ada' });
  await call(approved, 'GET', `/api/simulations/${SIM}/live`, { who: 'ada' });
  await call(approved, 'GET', '/api/simulations');
  assert.equal(await visitsOf(approved, SIM), 0, 'looking at a listing is not a visit');
  // A listing whose object is missing from storage serves nothing and counts nothing.
  const gone = await environment();
  const raw = JSON.parse(await gone.SOCIAL.get(`sim:${SIM}`));
  await gone.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...raw, r2_key: `universes/${SIM}/missing.pak` }));
  assert.equal((await call(gone, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' })).status, 404);
  assert.equal(await visitsOf(gone, SIM), 0);
});

// ── a join ───────────────────────────────────────────────────────────────────

const live = `/api/simulations/${SIM}/live`;
async function ticketFor(env, who, audience = PIN) {
  const r = await call(env, 'POST', '/api/identity/ticket', { who, body: { audience } });
  assert.equal(r.status, 200, r.text);
  return r.body.ticket;
}

test('a player the host admits is a visit by that player, once a day, and a refused join counts nothing', async () => {
  const env = await environment();
  assert.equal((await call(env, 'POST', live, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } })).status, 200);
  const ada = await ticketFor(env, 'ada');
  const admitted = await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: ada, audience: PIN } });
  assert.equal(admitted.status, 200, admitted.text);
  assert.equal(await plays(env), 1);
  assert.equal(await authorTotal(env), 1);

  // The same ticket again is refused: it is admitted once, and a refusal counts nothing.
  const again = await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: ada, audience: PIN } });
  assert.notEqual(again.status, 200);
  assert.equal(await plays(env), 1);

  // Ada joins again on a fresh ticket the same day: still one visit by her. Bob is a second.
  assert.equal((await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: await ticketFor(env, 'ada'), audience: PIN } })).status, 200);
  assert.equal(await plays(env), 1);
  assert.equal((await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: await ticketFor(env, 'bob'), audience: PIN } })).status, 200);
  assert.equal(await plays(env), 2);

  // A wrong audience is refused and counts nothing.
  const wrong = await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: await ticketFor(env, 'bob', 'cd'.repeat(32)), audience: PIN } });
  assert.notEqual(wrong.status, 200);
  assert.equal(await plays(env), 2);
});

test('a player who fetches the world and then joins is one visit, not two', async () => {
  const env = await environment();
  await call(env, 'POST', live, { who: 'author', body: { link: LINK, players: 1, max_players: 4, protocol: 3 } });
  await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'ada' });
  const admitted = await call(env, 'POST', `${live}/admit`, { who: 'author', body: { ticket: await ticketFor(env, 'ada'), audience: PIN } });
  assert.equal(admitted.status, 200, admitted.text);
  assert.equal(await plays(env), 1);
});

// ── the helper ───────────────────────────────────────────────────────────────

test('counting never fails a launch: a storage error is logged and the answer is no', async () => {
  const env = await environment();
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
  const failing = { ...env, SOCIAL: { get: async () => { throw new Error('KV is down'); }, put: async () => { throw new Error('KV is down'); } } };
  const real = console.error;
  console.error = () => {};
  try { assert.equal(await countVisit(failing, sim, 'u:x'), false); } finally { console.error = real; }
  assert.equal(await countVisit({}, sim, 'u:x'), false);
  assert.equal(await countVisit(env, sim, ''), false, 'nobody to count');
  assert.equal(await countVisit(env, null, 'u:x'), false, 'a listing that does not exist');
  assert.equal(await visitsOf(env, SIM), 0);
  assert.equal(await visitsOf(failing, SIM), 0, 'an unreadable counter reads as none');
});

test('a failing counter never fails the read that carries it: the manifest is served and the error is logged', async () => {
  const { handleWorldRoute } = await import('../src/world.mjs');
  const { isListable, canServe } = await import('../src/moderation.mjs');
  const env = await environment({ format: 'echk' });
  const deps = {
    verifyAuth: async () => null, requireAdmin: async () => null, canServe, isListable,
    json: (v, st = 200, h = {}) => new Response(JSON.stringify(v), { status: st, headers: { ...h, 'Content-Type': 'application/json' } }),
    onLaunch: async () => { throw new Error('the counter is down'); },
  };
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/manifest`);
  const real = console.error;
  const logged = [];
  console.error = (...a) => logged.push(a.join(' '));
  try {
    const res = await handleWorldRoute(new Request(url, { method: 'GET' }), url, env, {}, deps);
    assert.equal(res.status, 200);
    assert.equal((await res.json()).format, 'echk');
  } finally { console.error = real; }
  assert.ok(logged.some((l) => l.includes('the counter is down')), 'and the failure was logged');
});

test('the viewer key is the account when signed in, a keyed hash of the address otherwise, and stable for one address', async () => {
  const one = new Request('https://x.test/', { headers: { 'CF-Connecting-IP': '203.0.113.7', 'User-Agent': 'P/1' } });
  const same = new Request('https://x.test/', { headers: { 'CF-Connecting-IP': '203.0.113.7', 'User-Agent': 'P/2' } });
  const other = new Request('https://x.test/', { headers: { 'CF-Connecting-IP': '203.0.113.8', 'User-Agent': 'P/1' } });
  assert.equal(await viewerKey({ JWT_SECRET: 's' }, one, ADA), `u:${ADA}`);
  const a = await viewerKey({ JWT_SECRET: 'one' }, one, null);
  assert.match(a, /^d:[0-9a-f]{32}$/);
  assert.equal(await viewerKey({ JWT_SECRET: 'one' }, same, null), a, 'a browser string does not change it');
  assert.notEqual(await viewerKey({ JWT_SECRET: 'one' }, other, null), a);
  assert.notEqual(await viewerKey({ JWT_SECRET: 'two' }, one, null), a);
  assert.equal(await viewerKey({}, one, null), 'd:unknown');
});
