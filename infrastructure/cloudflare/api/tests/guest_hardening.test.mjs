// Guest play hardening (Security's review of 2026-10-16, and McKale's decisions of the same day): hosted guest play is off
// unless GUEST_PLAY is exactly "on"; a guest joins only a guest-only session whose host declares the guest protections;
// guests play G to PG only, and signed-out visitors see G to PG only; every guest chat line is checked before delivery and
// a refused line ends the guest and bars its lease for the day; the guest routes fail closed without their limiter; nothing
// a guest says is stored. GUEST_PLAY.md sections 8, 12, 13 and 14. The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, GUEST_CAPS_REQUIRED, guestSessionName } from '../src/live.mjs';
import { GuestAllowance, DAILY_SECONDS, GUEST_RATINGS, leaseIsLive } from '../src/guest.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { GUEST_CHAT_QUESTION_CATEGORIES } from '../src/moderation.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '33333333-3333-4333-8333-333333333333';
const PLAYER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const PIN2 = 'cd'.repeat(32);
const IP = { 'CF-Connecting-IP': '203.0.113.7' };
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);
const CAPS = [...GUEST_CAPS_REQUIRED];

/// A chat limiter that always allows, and Jev's forum battery answering every category low (a pass).
const allow = { limit: async () => ({ success: true }) };

function environment(extra = {}) {
  const env = { ...fakeEnv(), GUEST_PLAY: 'on', GUEST_CHAT_RATE_LIMITER: allow, ...extra };
  for (const [k, v] of Object.entries(extra)) if (v === undefined) delete env[k];
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  return env;
}

const listing = (id, moderation = {}, extra = {}) => ({
  id, name: `Listing ${id.slice(0, 4)}`, author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages', ...moderation }, ...extra,
});

async function seed(env) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
  await env.SOCIAL.put(`sim:${SIM_B}`, JSON.stringify(listing(SIM_B)));
  return env;
}

async function call(env, method, path, { token, body, headers, raw } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined || raw !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: raw !== undefined ? raw : body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

const author = mintToken(AUTHOR);
const other = mintToken(OTHER);
const player = mintToken(PLAYER);
const liveOf = (sim) => `/api/simulations/${sim}/live`;
const guestBeat = (env, extra = {}, sim = SIM, pin = PIN) => call(env, 'POST', liveOf(sim), { token: author, body: { link: linkFor(pin), players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: CAPS, ...extra } });
const accountBeat = (env, extra = {}, sim = SIM, pin = PIN2) => call(env, 'POST', liveOf(sim), { token: author, body: { link: linkFor(pin), players: 1, max_players: 8, protocol: 3, ...extra } });
const admit = (env, ticket, { sim = SIM, audience = PIN, guestOnly = true } = {}) => call(env, 'POST', `${liveOf(sim)}/admit`, { token: author, body: { ticket, audience, guest_only: guestOnly } });
const openLease = (env, sim = SIM, headers = IP) => call(env, 'POST', '/api/guest/play', { headers, body: { sim_id: sim, character: 'F', client: 'player' } });
const mint = (env, lease, audience = PIN) => call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease, audience } });
const chat = (env, id, text, { sim = SIM, token = author } = {}) => call(env, 'POST', '/api/guest/chat/check', { token, body: { sim_id: sim, id, text } });
const seats = (env, sim = SIM) => env.LIVE_HOSTS.instances.get(guestSessionName(sim))?.storage;

async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

/// A guest connected to the guest-only session of SIM; returns the lease and the join ticket id.
async function connected(env, sim = SIM) {
  await seed(env);
  assert.equal((await guestBeat(env, {}, sim)).status, 200);
  const lease = await openLease(env, sim);
  assert.equal(lease.status, 200, JSON.stringify(lease.body));
  const minted = await mint(env, lease.body.ticket);
  assert.equal(minted.status, 200, JSON.stringify(minted.body));
  const r = await admit(env, minted.body.ticket, { sim });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return { lease: lease.body.ticket, id: minted.body.id };
}

/// Jev, answering the guest chat battery with every category at `low` and `over` raising some.
function jevAnswering(over = {}, low = 0.02) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url: String(url), body: JSON.parse(init.body) });
    const answers = {};
    for (const c of GUEST_CHAT_QUESTION_CATEGORIES) answers[c] = { type: 'noul', noul: over[c] ?? low };
    return new Response(JSON.stringify({ model: 'jev-latest', answers, usage: { input_tokens: 10, output_tokens: 2 } }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  fn.calls = calls;
  return fn;
}
async function withFetch(fetchImpl, fn) {
  const real = globalThis.fetch;
  globalThis.fetch = fetchImpl;
  try { return await fn(); } finally { globalThis.fetch = real; }
}

/// Everything the Worker stored anywhere in this environment, as one string.
function everythingStored(env) {
  const parts = [];
  for (const v of Object.values(env)) {
    if (v?.store instanceof Map) for (const [k, x] of v.store) parts.push(k, String(x));
    if (v?.objects instanceof Map) for (const [k, o] of v.objects) parts.push(k, new TextDecoder().decode(o.body));
    if (v?.instances instanceof Map) for (const [name, o] of v.instances) { parts.push(name); for (const [k, x] of o.storage.map) parts.push(k, JSON.stringify(x)); }
  }
  return parts.join('\n');
}

// ── G-1 (a): off unless GUEST_PLAY is exactly "on" ───────────────────────────

test('hosted guest play is off unless GUEST_PLAY is exactly "on": no ticket, no admission, no guest-only session, no guest index', async () => {
  for (const setting of [undefined, 'off', 'ON', 'true', '1', ' on']) {
    await withClock(T0, async () => {
      const on = environment();
      const { lease } = await connected(on);
      const env = environment({ GUEST_PLAY: setting });
      await seed(env);
      // A lease is solo guest play, which the setting does not govern.
      const solo = await openLease(env);
      assert.equal(solo.status, 200, String(setting));
      const minted = await mint(env, solo.body.ticket);
      assert.deepEqual([minted.status, minted.body.code], [503, 'guest_join_unavailable'], String(setting));
      // A guest-only heartbeat is taken (so it can end guests) but the session is listed nowhere.
      assert.equal((await guestBeat(env)).status, 200, String(setting));
      assert.equal((await call(env, 'GET', '/api/guest/live', { headers: IP })).status, 503);
      // A ticket minted where it was on is refused where it is off.
      const ticket = (await mint(on, lease)).body.ticket;
      const r = await admit(env, ticket);
      assert.deepEqual([r.status, r.body.code], [403, 'guest_join_unavailable'], String(setting));
    });
  }
});

test('turning GUEST_PLAY off ends every connected guest on the next heartbeat, and its lease is not charged', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const { id } = await connected(env);
    env.GUEST_PLAY = 'off';
    clock.t += 30_000;
    const r = await guestBeat(env, { admitted: [id] });
    assert.deepEqual([r.status, r.body.ended_guests], [200, [id]]);
    // Not charged: the lease was not beaten.
    const left = (await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left;
    assert.equal(left, DAILY_SECONDS);
    env.GUEST_PLAY = 'on';
    clock.t += 30_000;
    assert.deepEqual((await guestBeat(env, { admitted: [id] })).body.ended_guests, [id], 'and stays ended when it comes back on');
  });
});

// ── G-1 (b): the host's latest heartbeat declares the guest protections ─────

test('a guest is admitted only while the host\'s latest heartbeat declares all three guest protections; dropping one ends the guests', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env);
    const lease = (await openLease(env)).body.ticket;
    for (const caps of [undefined, [], ['no_guest_chat', 'guest_name'], ['guest_name', 'ended_guests'], ['no_guest_chat', 'ended_guests']]) {
      assert.equal((await guestBeat(env, { guest_caps: caps })).status, 200, JSON.stringify(caps));
      const ticket = (await mint(env, lease)).body.ticket;
      const r = await admit(env, ticket);
      assert.deepEqual([r.status, r.body.code], [403, 'guest_not_supported'], JSON.stringify(caps));
    }
    assert.equal((await guestBeat(env, { guest_caps: [...CAPS, 'extra_cap'] })).status, 200);
    const minted = (await mint(env, lease)).body;
    assert.equal((await admit(env, minted.ticket)).status, 200);
    clock.t += 30_000;
    assert.equal((await guestBeat(env, { admitted: [minted.id] })).body.ended_guests, undefined);
    clock.t += 30_000;
    const dropped = await guestBeat(env, { admitted: [minted.id], guest_caps: ['no_guest_chat', 'guest_name'] });
    assert.deepEqual(dropped.body.ended_guests, [minted.id], 'a host that stops declaring them lets its guests go');
    // A malformed declaration is refused outright.
    for (const bad of ['no_guest_chat', [1], ['Has Space'], ['GUEST_NAME', 'no_guest_chat', 'ended_guests'], Array(17).fill('a')]) {
      assert.deepEqual([(await guestBeat(env, { guest_caps: bad })).body.code], ['bad_guest_caps'], JSON.stringify(bad));
    }
  });
});

// ── G-8: the guest routes fail closed, and an IPv6 /64 is one budget ─────────

test('the guest routes refuse without their limiter, or without an address, instead of running unlimited', async () => {
  await withClock(T0, async () => {
    const env = environment({ AUTH_RATE_LIMITER: undefined });
    await seed(env);
    for (const [method, path, body] of [['POST', '/api/guest/play', { sim_id: SIM, character: 'F' }], ['POST', '/api/guest/ticket', { lease: 'x', audience: PIN }], ['GET', '/api/guest/allowance'], ['GET', '/api/guest/live']]) {
      const r = await call(env, method, path, { headers: IP, body });
      assert.deepEqual([r.status, r.body.code], [503, 'guest_unavailable'], path);
    }
    const bound = environment();
    await seed(bound);
    for (const [method, path, body] of [['POST', '/api/guest/play', { sim_id: SIM, character: 'F' }], ['POST', '/api/guest/ticket', { lease: 'x', audience: PIN }], ['GET', '/api/guest/allowance']]) {
      const r = await call(bound, method, path, { body });
      assert.deepEqual([r.status, r.body.code], [400, 'address_unavailable'], `${path} with no address`);
    }
    // The other auth routes keep their old behaviour: no limiter bound, no limit.
    const signIn = await call(env, 'POST', '/api/auth/challenge', { headers: IP, body: {} });
    assert.notEqual(signIn.status, 503);
  });
});

test('the limiter keys an IPv6 address by its /64 and an IPv4 address whole', async () => {
  const keys = [];
  const env = environment({ AUTH_RATE_LIMITER: { limit: async ({ key }) => { keys.push(key); return { success: true }; } } });
  await withClock(T0, async () => {
    await seed(env);
    await openLease(env, SIM, { 'CF-Connecting-IP': '2001:db8:1:2:aaaa:bbbb:cccc:dddd' });
    await openLease(env, SIM, { 'CF-Connecting-IP': '2001:db8:1:2::9' });
    await openLease(env, SIM, { 'CF-Connecting-IP': '198.51.100.4' });
  });
  // ADJUSTED (fix): round 3 (D-1) keys each guest route family on its own, apart from sign-in.
  assert.deepEqual(keys, ['guest-play:2001:0db8:0001:0002::/64', 'guest-play:2001:0db8:0001:0002::/64', 'guest-play:198.51.100.4']);
});

// ── Notes: null bodies, a required simId, the allowance read, a failing allowance ──

test('a body of null, a number or an array is no body: every guest route answers 4xx, never 500', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    for (const raw of ['null', '7', '[]', '"x"']) {
      for (const path of ['/api/guest/play', '/api/guest/ticket', '/api/guest/beat', '/api/guest/end']) {
        const r = await call(env, 'POST', path, { headers: IP, raw });
        assert.ok(r.status >= 400 && r.status < 500, `${path} ${raw}: ${r.status} ${JSON.stringify(r.body)}`);
      }
      for (const path of [liveOf(SIM), `${liveOf(SIM)}/admit`, '/api/guest/chat/check']) {
        const r = await call(env, 'POST', path, { token: author, raw });
        assert.ok(r.status >= 400 && r.status < 500, `${path} ${raw}: ${r.status} ${JSON.stringify(r.body)}`);
      }
    }
  });
});

test('leaseIsLive needs the listing: a lease is never checked without one', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    const lease = (await openLease(env)).body.ticket;
    assert.equal(await leaseIsLive(env, lease, T0, SIM), true);
    assert.equal(await leaseIsLive(env, lease, T0, SIM_B), false);
    assert.equal(await leaseIsLive(env, lease, T0), false, 'no listing named');
    assert.equal(await leaseIsLive(env, lease, T0, ''), false);
  });
});

test('asking what is left stores nothing and sets no alarm, before and after a visitor plays elsewhere', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    for (let i = 0; i < 3; i++) assert.equal((await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left, DAILY_SECONDS);
    assert.ok([...env.GUEST_ALLOWANCE.instances.values()].every((o) => o.storage.map.size === 0 && o.storage.alarm === null));
  });
});

test('an allowance that throws while the host beats a guest fails closed: the guest is ended at once', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const { id } = await connected(env);
    const real = env.GUEST_ALLOWANCE.get;
    env.GUEST_ALLOWANCE.get = () => ({ fetch: async () => { throw new Error('allowance down'); } });
    clock.t += 30_000;
    const quiet = console.error;
    console.error = () => {};
    let r;
    try { r = await guestBeat(env, { admitted: [id] }); } finally { console.error = quiet; }
    env.GUEST_ALLOWANCE.get = real;
    assert.equal(r.status, 200);
    assert.deepEqual(r.body.ended_guests, [id]);
    const seat = await seats(env).get(`gadm:${id}`);
    assert.deepEqual(Object.keys(seat).sort(), ['at', 'ended', 'seen'], 'no guest id or lease id is kept for an ended guest');
  });
});

test('a seat whose guest id is past its 25 hours keeps no identifier, and the guest is reported ended', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const { id } = await connected(env);
    const seat = await seats(env).get(`gadm:${id}`);
    assert.equal(seat.gx, Date.UTC(2026, 9, 17, 1, 0, 0), 'the guest id goes 25 hours after its day began');
    // A seat that outlives that (its lease would have ended first; this is the backstop).
    await seats(env).put(`gadm:${id}`, { ...seat, gx: clock.t + 1000 });
    clock.t += 30_000;
    assert.deepEqual((await guestBeat(env, { admitted: [id] })).body.ended_guests, [id]);
    assert.deepEqual(Object.keys(await seats(env).get(`gadm:${id}`)).sort(), ['at', 'ended', 'seen']);
  });
});

test('hosted time is charged for the whole gap between host beats, up to the lease timeout', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const { id } = await connected(env);
    for (const gap of [30_000, 60_000, 89_000]) {
      clock.t += gap;
      assert.equal((await guestBeat(env, { admitted: [id] })).body.ended_guests, undefined);
    }
    const left = (await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left;
    assert.equal(DAILY_SECONDS - left, 30 + 60 + 89);
  });
});

test('a guest who leaves and comes back on the same lease takes one seat again and counts one visit', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const { lease, id } = await connected(env);
    // While the first seat is held (named, or inside the grace) the lease's next ticket is refused.
    const second = (await mint(env, lease)).body;
    assert.deepEqual([(await admit(env, second.ticket)).body.code], ['lease_in_use']);
    // The host lets the first seat go (the guest disconnected); past the grace the lease may sit down again.
    clock.t += 30_000;
    await guestBeat(env, { admitted: [id] });
    // Meanwhile the guest's own Player keeps its lease beating.
    const ownBeat = () => call(env, 'POST', '/api/guest/beat', { body: { ticket: lease } });
    clock.t += 30_000;
    await guestBeat(env, { admitted: [] });
    assert.equal((await ownBeat()).status, 200);
    clock.t += 30_000;
    await guestBeat(env, { admitted: [] });
    assert.equal((await ownBeat()).status, 200);
    clock.t += 31_000;
    assert.equal((await ownBeat()).status, 200);
    const back = await admit(env, second.ticket);
    assert.equal(back.status, 200, JSON.stringify(back.body));
    assert.equal(await env.SOCIAL.get(`visits:${SIM}`), '1', 'one lease, one visit');
  });
});

// ── McKale 1: G to PG for guests, and for every signed-out visitor ──────────

test('guests play G to PG: all_ages and ages_10 at the lease, the ticket and the admission; anything above is refused', async () => {
  assert.deepEqual([...GUEST_RATINGS], ['all_ages', 'ages_10']);
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env);
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating: 'ages_10' })));
    const { id } = await connected(env).catch(() => ({}));
    assert.ok(id, 'an ages_10 listing is joined');
    for (const rating of ['teen_13', 'mature_17', 'adult_18', 'adult_21', undefined]) {
      await env.SOCIAL.put(`sim:${SIM_B}`, JSON.stringify(listing(SIM_B, { rating })));
      const r = await openLease(env, SIM_B);
      assert.deepEqual([r.status, r.body.reason], [403, 'rating'], String(rating));
    }
    // Re-rated above PG while the guest is connected: let go on the next heartbeat.
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating: 'teen_13' })));
    clock.t += 30_000;
    assert.deepEqual((await guestBeat(env, { admitted: [id] })).body.ended_guests, [id]);
  });
});

test('a signed-out visitor sees and fetches G to PG only; signing in opens up to mature_17', async () => {
  const env = environment();
  await seed(env);
  const TEEN = 'cccccccc-3333-4333-8333-cccccccccccc';
  const TEN = 'dddddddd-4444-4444-8444-dddddddddddd';
  for (const [id, rating] of [[TEEN, 'teen_13'], [TEN, 'ages_10']]) {
    await env.SOCIAL.put(`sim:${id}`, JSON.stringify(listing(id, { rating }, { r2_key: `universes/${id}/universe.pak` })));
    await env.SCENES.put(`universes/${id}/universe.pak`, 'the world');
  }
  for (const path of [`/api/simulations/${TEEN}`, `/api/simulations/${TEEN}/download`]) {
    const out = await call(env, 'GET', path, { headers: IP });
    assert.deepEqual([out.status, out.body?.code], [403, 'sign_in_required'], path);
    assert.equal((await call(env, 'GET', path, { token: player })).status, 200, `${path} signed in`);
    assert.equal((await call(env, 'GET', path.replace(TEEN, TEN), { headers: IP })).status, 200, `${path} ages_10 signed out`);
  }
  const list = (await call(env, 'GET', '/api/simulations')).body.simulations.map((s) => s.id);
  assert.ok(list.includes(TEN) && !list.includes(TEEN), 'the Gallery, signed out');
  const signedIn = (await call(env, 'GET', '/api/simulations', { token: player })).body.simulations.map((s) => s.id);
  assert.ok(signedIn.includes(TEEN), 'the Gallery, signed in');
});

// ── McKale 2: guest-only sessions ───────────────────────────────────────────

test('guests join guest-only sessions only, accounts join account sessions only, and the two are hosted side by side', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    assert.equal((await accountBeat(env)).status, 200); // the account session, at PIN2
    const lease = (await openLease(env)).body.ticket;
    // No guest-only session yet: no ticket, even for the account session's pin.
    assert.deepEqual([(await mint(env, lease, PIN2)).body.code], ['not_live']);
    assert.equal((await guestBeat(env)).status, 200); // the guest-only session, at PIN
    const { ticket } = (await mint(env, lease)).body;
    // A guest ticket presented by the account session's host goes to the guest-only session, which is at another pin.
    const wrong = await admit(env, ticket, { audience: PIN2, guestOnly: false });
    assert.equal(wrong.status, 403);
    assert.equal((await admit(env, ticket)).status, 200);
    // An account ticket: the account session admits it, the guest-only session refuses it.
    const forGuestSession = (await call(env, 'POST', '/api/identity/ticket', { token: player, body: { audience: PIN } })).body.ticket;
    assert.deepEqual([(await admit(env, forGuestSession)).body.code], ['guest_only_session']);
    const forAccounts = (await call(env, 'POST', '/api/identity/ticket', { token: player, body: { audience: PIN2 } })).body.ticket;
    const r = await admit(env, forAccounts, { audience: PIN2, guestOnly: false });
    assert.deepEqual([r.status, r.body.guest], [200, false]);
    // Stopping one leaves the other.
    assert.equal((await call(env, 'DELETE', `${liveOf(SIM)}?guest_only=true`, { token: author })).status, 200);
    assert.equal((await call(env, 'GET', liveOf(SIM), { token: player })).body.live, true, 'the account session is still live');
  });
});

test('the account session object never admits a guest, even asked directly', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    // ADJUSTED (fix): round 3 (R-1) refuses both sessions at one pin at once, so the guest-only session mints the ticket,
    // stops, and the account session then takes that pin.
    assert.equal((await guestBeat(env, {}, SIM, PIN)).status, 200);
    const ticket = (await mint(env, (await openLease(env)).body.ticket)).body.ticket;
    assert.equal((await call(env, 'DELETE', `${liveOf(SIM)}?guest_only=true`, { token: author })).status, 200);
    assert.equal((await accountBeat(env, {}, SIM, PIN)).status, 200);
    const stub = env.LIVE_HOSTS.get(SIM);
    const r = await (await stub.fetch('https://live.internal/admit', { method: 'POST', body: JSON.stringify({ ticket, audience: PIN, now: T0 }) })).json();
    assert.deepEqual([r.ok, r.code], [false, 'account_session']);
  });
});

test('guest-only sessions are listed on their own, for a guest\'s Play chooser, and never on the Gallery\'s strip', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    const TEEN = 'cccccccc-3333-4333-8333-cccccccccccc';
    await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13' })));
    assert.equal((await guestBeat(env, {}, SIM, PIN)).status, 200);
    assert.equal((await accountBeat(env, {}, SIM_B, PIN2)).status, 200);
    assert.equal((await guestBeat(env, {}, TEEN, PIN2)).status, 200, 'a teen listing\'s guest-only heartbeat is taken but never listed');
    const guests = (await call(env, 'GET', '/api/guest/live', { headers: IP })).body;
    assert.deepEqual(guests.sessions.map((s) => s.id), [SIM]);
    const strip = (await call(env, 'GET', '/api/simulations/live', { token: player })).body;
    assert.deepEqual(strip.sessions.map((s) => s.id), [SIM_B]);
  });
});

// ── McKale 3: every guest chat line is checked before delivery ──────────────

test('a guest chat line Jev passes is delivered; nothing about it is stored or logged', async () => {
  await withClock(T0, async () => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    const { id } = await connected(env);
    const jev = jevAnswering();
    const logged = [];
    const real = { log: console.log, warn: console.warn, error: console.error };
    console.log = (...a) => logged.push(a.join(' '));
    console.warn = console.log;
    console.error = console.log;
    let r;
    const LINE = 'nice jump zebra-quartz-71';
    try { r = await withFetch(jev, () => chat(env, id, LINE)); } finally { Object.assign(console, real); }
    assert.deepEqual([r.status, r.body.deliver], [200, true]);
    assert.equal(jev.calls.length, 1, 'Jev read the line');
    assert.ok(!everythingStored(env).includes('zebra-quartz'), 'the line is not stored anywhere');
    assert.ok(!logged.join('\n').includes('zebra-quartz'), 'the line is not logged');
    assert.ok(logged.some((l) => l.includes('"guest_chat_check"') && l.includes('"deliver"')), 'only the outcome is logged');
    assert.equal(await seats(env).get('chat_n:deliver'), 1);
  });
});

test('a refused guest chat line is not delivered, ends the guest at once, and bars its lease from the listing for the day', async () => {
  await withClock(T0, async (clock) => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    const { id, lease } = await connected(env);
    // Personal information is refused before Jev is asked (moderation.mjs checkCommunityText).
    const jev = jevAnswering();
    const r = await withFetch(jev, () => chat(env, id, 'call me at 555 201 3344'));
    // ADJUSTED (fix): round 3 (B-4, R-8) tells the host deliver, block or kick only, never the category.
    assert.deepEqual([r.status, r.body.deliver, r.body.action, r.body.end_guest, r.body.ended_guests, r.body.category], [200, false, 'kick', true, [id], undefined]);
    assert.ok(!everythingStored(env).includes('201 3344') && !everythingStored(env).includes('2013344'));
    // Reported ended at once, the lease is over, and the guest cannot come back to this listing today.
    clock.t += 1000;
    assert.deepEqual((await guestBeat(env, { admitted: [id] })).body.ended_guests, [id]);
    assert.equal((await mint(env, lease)).status, 410);
    const again = await openLease(env);
    assert.deepEqual([again.status, again.body.code], [403, 'guest_barred']);
    assert.equal((await openLease(env, SIM_B)).status, 200, 'another listing is still open');
    // The next UTC day starts clean.
    clock.t = Date.UTC(2026, 9, 17, 0, 1, 0);
    assert.equal((await openLease(env)).status, 200);
  });
});

test('Jev refusing a line by its own reading kicks the guest the same way', async () => {
  await withClock(T0, async () => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    const { id } = await connected(env);
    const r = await withFetch(jevAnswering({ threat: 0.99 }), () => chat(env, id, 'an ordinary looking line'));
    assert.deepEqual([r.body.deliver, r.body.end_guest], [false, true]);
    assert.equal(await seats(env).get('chat_n:block'), 1);
  });
});

test('the chat check fails closed: Jev unavailable, no limiter, the limit reached, a stranger, a guest not here, the setting off', async () => {
  await withClock(T0, async () => {
    const env = environment(); // no JEV_API_KEY: Jev cannot be asked
    const { id } = await connected(env);
    const quiet = console.warn;
    console.warn = () => {};
    let unavailable;
    try { unavailable = await chat(env, id, 'hello there'); } finally { console.warn = quiet; }
    assert.deepEqual([unavailable.status, unavailable.body.deliver, unavailable.body.code], [503, false, 'check_unavailable']);
    assert.equal((await seats(env).get(`gadm:${id}`)).ended, undefined, 'an unavailable check never kicks');

    const failing = await withFetch(async () => { throw new Error('down'); }, async () => {
      const e2 = environment({ JEV_API_KEY: 'jev-test' });
      const c = await connected(e2);
      console.warn = () => {};
      try { return await chat(e2, c.id, 'hello there'); } finally { console.warn = quiet; }
    });
    assert.deepEqual([failing.body.deliver, failing.body.code], [false, 'check_unavailable']);

    // No limiter bound, with a check that would pass (Jev configured and answering low): still not delivered, so the
    // refusal is the missing limiter's and nothing else's (round 3, R-10).
    env.JEV_API_KEY = 'jev-test';
    const passing = jevAnswering();
    assert.deepEqual([(await withFetch(passing, () => chat(env, id, 'hi there'))).body.deliver], [true], 'control: with the limiter, the line passes');
    delete env.GUEST_CHAT_RATE_LIMITER;
    const unlimited = await withFetch(passing, () => chat(env, id, 'hi there'));
    assert.deepEqual([unlimited.status, unlimited.body.deliver, unlimited.body.code], [503, false, 'check_unavailable']);
    assert.equal(passing.calls.length, 1, 'Jev is never asked without the limiter');
    delete env.JEV_API_KEY;
    env.GUEST_CHAT_RATE_LIMITER = { limit: async () => ({ success: false }) };
    const limited = await chat(env, id, 'hi');
    assert.deepEqual([limited.status, limited.body.deliver, limited.body.code], [429, false, 'rate_limited']);
    env.GUEST_CHAT_RATE_LIMITER = allow;

    assert.deepEqual([(await chat(env, id, 'hi', { token: other })).body.code], ['not_author']);
    assert.equal((await chat(env, id, 'hi', { token: '' })).status, 401);
    assert.deepEqual([(await chat(env, 'f'.repeat(32), 'hi')).body.code], ['not_connected']);
    assert.deepEqual([(await chat(env, id, 'x'.repeat(501))).body.code], ['too_long']);
    env.GUEST_PLAY = 'off';
    assert.deepEqual([(await chat(env, id, 'hi')).status, (await chat(env, id, 'hi')).body.deliver], [503, false]);
  });
});
