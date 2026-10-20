// Guests in hosted sessions: a person with no account joins a live session from a
// guest lease (GUEST_PLAY.md section 8). The lease mints a single-use guest join
// ticket for the session's own pin; the host's /live/admit admits it as a guest
// (no account, the name "Guest") only for an approved, all-ages listing and only
// while the lease still beats; the host's heartbeat then beats the lease and
// reports guests whose lease has ended. The real Worker runs against in-memory
// storage; time is moved by replacing Date.now.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, MAX_GUESTS, GUEST_CAPS_REQUIRED } from '../src/live.mjs';
import {
  GuestAllowance, DAILY_SECONDS, GUEST_JOIN_TTL_S, LEASE_TIMEOUT_MS,
  inspectGuestJoin, isGuestJoinTicket, signGuestJoin, verifyLease,
} from '../src/guest.mjs';
import { inspectIdentityTicket, makeIdentityTicket } from '../src/identity.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const PIN2 = 'cd'.repeat(32);
const IP = { 'CF-Connecting-IP': '203.0.113.7' };
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);

function environment(extra = {}) {
  // Hosted guest play on (GUEST_PLAY.md section 13); a test about it being off says so.
  const env = { ...fakeEnv(), GUEST_PLAY: 'on', ...extra };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

const listing = (id, moderation = {}) => ({
  id, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages', ...moderation },
});

async function seed(env) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
  await env.SOCIAL.put(`sim:${SIM_B}`, JSON.stringify(listing(SIM_B)));
  return env;
}

async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

const author = mintToken(AUTHOR);
const player = mintToken(PLAYER);
const liveOf = (sim) => `/api/simulations/${sim}/live`;
// A guest-only host that declares every guest protection, as a host must before it takes a guest.
const hostBeat = (env, extra = {}, sim = SIM, pin = PIN) => call(env, 'POST', liveOf(sim), { token: author, body: { link: linkFor(pin), players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: [...GUEST_CAPS_REQUIRED], ...extra } });
// The host of a guest-only session says so on the admit, as on its heartbeat.
const admit = (env, ticket, { sim = SIM, audience = PIN, guestOnly = true } = {}) => call(env, 'POST', `${liveOf(sim)}/admit`, { token: author, body: { ticket, audience, guest_only: guestOnly } });
const openLease = (env, sim = SIM, headers = IP) => call(env, 'POST', '/api/guest/play', { headers, body: { sim_id: sim, character: 'F', client: 'player' } });
const mint = (env, lease, audience = PIN) => call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease, audience } });

async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

/// A hosted listing and a guest with a live lease on it.
async function hostedWithGuest(env) {
  await seed(env);
  assert.equal((await hostBeat(env)).status, 200);
  const lease = await openLease(env);
  assert.equal(lease.status, 200, JSON.stringify(lease.body));
  return lease.body.ticket;
}

async function guestTicket(env, lease, audience = PIN) {
  const r = await mint(env, lease, audience);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

// ── Minting a guest join ticket ─────────────────────────────────────────────

test('a guest with a live lease gets a join ticket for the session hosting that listing, and the host admits it once, as a guest', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const minted = await guestTicket(env, lease);
    assert.ok(isGuestJoinTicket(minted.ticket));
    assert.match(minted.id, /^[0-9a-f]{32}$/);
    assert.equal(minted.expires, Math.floor(T0 / 1000) + GUEST_JOIN_TTL_S);
    assert.deepEqual([minted.guest, minted.name], [true, 'Guest']);

    const r = await admit(env, minted.ticket);
    assert.equal(r.status, 200, JSON.stringify(r.body));
    assert.deepEqual([r.body.ok, r.body.id, r.body.account_id, r.body.username, r.body.guest], [true, minted.id, null, 'Guest', true]);

    const again = await admit(env, minted.ticket);
    assert.deepEqual([again.status, again.body.code], [403, 'used']);
  });
});

test('the join ticket names nothing about the person, not even the guest or the lease: its own id, the pin, the listing and an expiry', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    const payload = JSON.parse(Buffer.from(ticket.split('.')[1], 'base64url').toString());
    assert.deepEqual(Object.keys(payload).sort(), ['e', 'h', 'j', 's']);
    assert.ok(!ticket.includes('203.0.113.7') && !JSON.stringify(payload).includes('203'), 'no address');
  });
});

test('a join ticket is refused for a bad lease, a bad pin, an ended lease, and a pin nobody hosts the listing at', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    assert.deepEqual([(await mint(env, 'egl1.junk.junk')).status, (await mint(env, 'egl1.junk.junk')).body.code], [410, 'bad_ticket']);
    assert.equal((await mint(env, undefined)).status, 410);
    assert.equal((await mint(env, lease, 'not-a-pin')).body.code, 'bad_audience');
    const elsewhere = await mint(env, lease, PIN2);
    assert.deepEqual([elsewhere.status, elsewhere.body.code], [409, 'not_live'], 'a pin this listing is not hosted at');

    // The host stops: no session, no ticket.
    assert.equal((await call(env, 'DELETE', `${liveOf(SIM)}?guest_only=true`, { token: author })).status, 200);
    assert.equal((await mint(env, lease)).body.code, 'not_live');
    assert.equal((await hostBeat(env)).status, 200);
    assert.equal((await mint(env, lease)).status, 200, 'hosted again');

    // The lease goes silent past its timeout.
    clock.t += LEASE_TIMEOUT_MS + 1;
    const late = await mint(env, lease);
    assert.deepEqual([late.status, late.body.code], [410, 'lease_ended']);
  });
});

test('a listing re-rated, taken down or made private after the lease opened gets no join ticket', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    for (const rating of ['teen_13', 'mature_17', 'adult_18', 'adult_21', undefined]) {
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating })));
      const r = await mint(env, lease);
      assert.deepEqual([r.status, r.body.code], [403, 'guest_not_allowed'], String(rating));
    }
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { status: 'rejected' })));
    assert.equal((await mint(env, lease)).status, 404);
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...listing(SIM), is_public: false }));
    assert.equal((await mint(env, lease)).status, 404);
  });
});

test('minting a join ticket is rate limited per address, like opening a lease', async () => {
  let n = 0;
  const env = environment({ AUTH_RATE_LIMITER: { limit: async () => ({ success: ++n <= 2 }) } });
  await withClock(T0, async () => {
    const lease = await hostedWithGuest(env); // the first allowed call
    assert.equal((await mint(env, lease)).status, 200); // the second
    const third = await mint(env, lease);
    assert.deepEqual([third.status, third.body.code], [429, 'rate_limited']);
  });
});

// ── Admission ───────────────────────────────────────────────────────────────

test('a guest is never admitted to anything but an approved G-to-PG listing, and a refusal does not spend the ticket', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    for (const rating of ['adult_18', 'adult_21', 'mature_17', 'teen_13', undefined]) {
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating })));
      const r = await admit(env, ticket);
      assert.deepEqual([r.status, r.body.code], [403, 'guest_not_allowed'], String(rating));
    }
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { status: 'pending' })));
    assert.deepEqual([(await admit(env, ticket)).status, (await admit(env, ticket)).body.code], [403, 'not_available'], 'not approved');

    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
    assert.equal((await admit(env, ticket)).status, 200, 'the refusals spent nothing');
  });
});

test('a guest whose lease has ended, or was replaced by a newer one, is not admitted', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    // The same guest opens another lease: the first ends, and so does what it minted.
    assert.equal((await openLease(env, SIM_B)).status, 200);
    const r = await admit(env, ticket);
    assert.deepEqual([r.status, r.body.code], [403, 'lease_ended']);
  });
});

test('a guest ticket for one listing, or one session, does not open another', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    // Listing B hosted at the same pin: a ticket minted for listing A is still not for B.
    assert.equal((await hostBeat(env, {}, SIM_B, PIN)).status, 200);
    const { ticket } = await guestTicket(env, lease);
    const other = await admit(env, ticket, { sim: SIM_B });
    assert.deepEqual([other.status, other.body.code], [403, 'wrong_host']);
    // Another session's pin.
    const wrongPin = await admit(env, ticket, { audience: PIN2 });
    assert.deepEqual([wrongPin.status, wrongPin.body.code], [403, 'wrong_host']);
    assert.equal((await admit(env, ticket)).status, 200);

    // The host restarts: a new certificate, so a new pin. A ticket minted for the old session does not open the new one.
    const { ticket: stale } = await guestTicket(env, lease);
    assert.equal((await hostBeat(env, {}, SIM, PIN2)).status, 200);
    const restarted = await admit(env, stale, { audience: PIN2 });
    assert.deepEqual([restarted.status, restarted.body.code], [403, 'wrong_host']);
    const fresh = await guestTicket(env, lease, PIN2);
    assert.equal((await admit(env, fresh.ticket, { audience: PIN2 })).status, 200);
  });
});

test('a forged, altered or expired guest ticket is refused, and a lease ticket is not a join ticket', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    const [p, body, mac] = ticket.split('.');
    const payload = JSON.parse(Buffer.from(body, 'base64url').toString());
    const altered = `${p}.${Buffer.from(JSON.stringify({ ...payload, s: SIM_B })).toString('base64url')}.${mac}`;
    assert.equal((await admit(env, altered)).body.code, 'invalid');
    assert.equal((await admit(env, `${p}.${body}.${mac.slice(0, -2)}AA`)).body.code, 'invalid');
    assert.equal((await admit(env, 'egt1.junk.junk')).body.code, 'invalid');
    assert.equal((await admit(env, lease)).body.code, 'invalid', 'the lease itself');

    // Signed with the real key but long expired.
    const old = await signGuestJoin(env, { lease: await verifyLease(env, lease, clock.t), audience: PIN, now: clock.t - (GUEST_JOIN_TTL_S + 60) * 1000 });
    assert.equal((await admit(env, old.ticket)).body.code, 'expired');
    assert.equal((await admit(env, ticket)).status, 200);
  });
});

test('a guest join ticket is never an account: not an identity ticket, not a session, and an identity ticket is not a guest ticket', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    assert.equal((await inspectIdentityTicket(env, ticket, { audience: PIN, now: Math.floor(T0 / 1000) })).ok, false);
    const verify = await call(env, 'POST', '/api/identity/verify', { body: { ticket, audience: PIN } });
    assert.equal(verify.status, 401);
    assert.equal((await call(env, 'GET', '/api/auth/me', { token: ticket })).status, 401);
    assert.equal((await call(env, 'POST', '/api/identity/ticket', { token: ticket, body: { audience: PIN } })).status, 401);

    const identity = await makeIdentityTicket(env, { accountId: PLAYER, username: 'ada', audience: PIN, now: Math.floor(T0 / 1000) });
    assert.equal((await inspectGuestJoin(env, identity.ticket, { audience: PIN, now: T0 })).ok, false);
    // An account never joins a guest-only session; it joins an account session as itself.
    const refused = await admit(env, identity.ticket);
    assert.deepEqual([refused.status, refused.body.code], [403, 'guest_only_session']);
    assert.equal((await hostBeat(env, { guest_only: false }, SIM, PIN2)).status, 200);
    const asItself = await makeIdentityTicket(env, { accountId: PLAYER, username: 'ada', audience: PIN2, now: Math.floor(T0 / 1000) });
    const r = await admit(env, asItself.ticket, { audience: PIN2, guestOnly: false });
    assert.deepEqual([r.status, r.body.guest, r.body.account_id], [200, false, PLAYER]);
  });
});

test('a full guest session turns the next guest away, and never admits an account', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const storage = env.LIVE_HOSTS.instances.get(`guest:${SIM}`).storage;
    for (let i = 0; i < MAX_GUESTS; i++) await storage.put(`gadm:${i.toString(16).padStart(32, '0')}`, { gid: 'x', tid: 'y', at: T0 });
    const { ticket } = await guestTicket(env, lease);
    const full = await admit(env, ticket);
    assert.deepEqual([full.status, full.body.code], [403, 'not_available']);
    const identity = await call(env, 'POST', '/api/identity/ticket', { token: player, body: { audience: PIN } });
    const account = await admit(env, identity.body.ticket);
    assert.deepEqual([account.status, account.body.code], [403, 'guest_only_session']);
  });
});

// ── Time while hosted ───────────────────────────────────────────────────────

test('while the host names a guest as connected, its heartbeat beats the guest\'s lease, measured by the Worker', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);

    for (let i = 1; i <= 4; i++) {
      clock.t += 30_000;
      const r = await hostBeat(env, { admitted: [id] });
      assert.equal(r.status, 200);
      assert.ok(!('ended_guests' in r.body), 'nobody to let go');
    }
    // Two minutes of beats: the lease is alive well past its own 90 s timeout, and two minutes were charged.
    const left = await call(env, 'GET', '/api/guest/allowance', { headers: IP });
    assert.equal(left.body.seconds_left, DAILY_SECONDS - 120);
    assert.equal((await mint(env, lease)).status, 200, 'still live');
  });
});

test('the heartbeat reports a guest whose lease ended, on every beat until the host lets it go', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    // The guest ends its lease (or opens another, or its time runs out).
    assert.equal((await call(env, 'POST', '/api/guest/end', { body: { ticket: lease } })).status, 200);

    clock.t += 30_000;
    assert.deepEqual((await hostBeat(env, { admitted: [id] })).body.ended_guests, [id]);
    clock.t += 30_000;
    assert.deepEqual((await hostBeat(env, { admitted: [id] })).body.ended_guests, [id], 'again, for a host that missed it');
    clock.t += 30_000;
    assert.equal((await hostBeat(env, { admitted: [] })).body.ended_guests, undefined, 'the host let it go');
  });
});

test('a guest the host stops naming is forgotten after the grace, and its lease is no longer beaten', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    const storage = env.LIVE_HOSTS.instances.get(`guest:${SIM}`).storage;

    clock.t += 30_000;
    await hostBeat(env, { admitted: [] });
    assert.ok(storage.map.has(`gadm:${id}`), 'a beat sent before the admission reached the host does not drop the guest');
    clock.t += 70_000;
    await hostBeat(env, { admitted: [] });
    assert.ok(!storage.map.has(`gadm:${id}`), 'past the grace the guest is forgotten');
    clock.t += 30_000;
    await hostBeat(env, { admitted: [id] });
    assert.equal((await mint(env, lease)).status, 410, 'nothing beat the lease, so it ended by its own timeout');
  });
});

test('a new hosting session, or the host stopping, forgets its guests', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    const storage = env.LIVE_HOSTS.instances.get(`guest:${SIM}`).storage;
    assert.equal((await storage.list({ prefix: 'gadm:' })).size, 1);
    assert.equal((await call(env, 'DELETE', `${liveOf(SIM)}?guest_only=true`, { token: author })).status, 200);
    assert.equal((await storage.list({ prefix: 'gadm:' })).size, 0);
  });
});

// ── The link and the Play route ─────────────────────────────────────────────

test('a lease on one listing never hands out another listing\'s host link', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    // Listing B is hosted for accounts: a lease on A is not a key to it.
    assert.equal((await hostBeat(env, { guest_only: false }, SIM_B, PIN2)).status, 200);
    const h = { 'X-Eustress-Lease': lease };
    assert.equal((await call(env, 'GET', liveOf(SIM), { headers: h })).body.link, linkFor(PIN));
    const other = await call(env, 'GET', liveOf(SIM_B), { headers: h });
    assert.equal(other.body.link_hidden, true);
    assert.ok(!('link' in other.body));
    const play = await call(env, 'POST', `/api/simulations/${SIM_B}/play`, { headers: h });
    assert.equal(play.status, 200, JSON.stringify(play.body));
    assert.equal(play.body.link_hidden, true);
    assert.equal(play.body.status, 'solo');
    const own = await call(env, 'POST', `/api/simulations/${SIM}/play`, { headers: h });
    assert.deepEqual([own.body.status, own.body.launch.link], ['live', linkFor(PIN)]);
  });
});

test('the desktop Player may open a lease on a listing the browser cannot run; the browser may not, and the client is checked', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...listing(SIM), browser_play: false }));
    const browser = await call(env, 'POST', '/api/guest/play', { headers: IP, body: { sim_id: SIM, character: 'M' } });
    assert.deepEqual([browser.status, browser.body.reason], [403, 'browser']);
    assert.equal((await openLease(env, SIM)).status, 200);
    const bad = await call(env, 'POST', '/api/guest/play', { headers: IP, body: { sim_id: SIM, character: 'M', client: 'studio' } });
    assert.deepEqual([bad.status, bad.body.code], [400, 'bad_request']);
  });
});
