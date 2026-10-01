// Guest play: the Gallery without an account. One allowance per network address
// per UTC day, 3 simulations and 60 minutes, measured by leases. See
// docs/architecture/GUEST_PLAY.md. The real Worker runs against in-memory
// storage; time is moved by replacing Date.now.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import {
  GuestAllowance, addressKey, guestId, signLease, verifyLease, dayOf,
  MAX_SIMS, DAILY_SECONDS, BEAT_CAP_SECONDS, LEASE_TIMEOUT_MS,
} from '../src/guest.mjs';

const worker = await loadWorker();

const SIM_A = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const SIM_C = 'cccccccc-3333-4333-8333-cccccccccccc';
const SIM_D = 'dddddddd-4444-4444-8444-dddddddddddd';
const IP = '203.0.113.7';

function sim(id, moderation = {}, extra = {}) {
  return {
    id, name: `World ${id.slice(0, 4)}`, author_id: 'author', is_public: true,
    moderation: { status: 'approved', rating: 'all_ages', ...moderation }, ...extra,
  };
}

function environment(extra = {}) {
  const env = { ...fakeEnv(), ...extra };
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

async function seed(env, sims = [sim(SIM_A), sim(SIM_B), sim(SIM_C), sim(SIM_D)]) {
  for (const s of sims) await env.SOCIAL.put(`sim:${s.id}`, JSON.stringify(s));
  return env;
}

async function call(env, method, path, { ip = IP, body, token } = {}) {
  const headers = {};
  if (ip) headers['CF-Connecting-IP'] = ip;
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

const play = (env, simId, opts = {}) => call(env, 'POST', '/api/guest/play', { body: { sim_id: simId, character: 'M' }, ...opts });
const beat = (env, ticket, opts = {}) => call(env, 'POST', '/api/guest/beat', { body: { ticket }, ...opts });

/// Runs `fn` with Date.now under the test's control: `clock.t` is the time.
async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

const T0 = Date.UTC(2026, 8, 30, 12, 0, 0);

// ── Addresses and ids ────────────────────────────────────────────────────────

test('an address reduces to an IPv4 address or an IPv6 /64, and nothing else is an address', () => {
  assert.equal(addressKey('203.0.113.7'), '203.0.113.7');
  assert.equal(addressKey(' 203.0.113.7 '), '203.0.113.7');
  const same = ['2001:db8:1:2:aaaa:bbbb:cccc:dddd', '2001:DB8:1:2::1', '2001:db8:1:2:0:0:0:0', '2001:db8:1:2::'];
  const keys = same.map(addressKey);
  assert.ok(keys.every((k) => k === keys[0]), keys.join(' | '));
  assert.equal(keys[0], '2001:0db8:0001:0002::/64');
  assert.notEqual(addressKey('2001:db8:1:3::1'), keys[0], 'another /64 is another guest');
  assert.equal(addressKey('::ffff:203.0.113.7'), '203.0.113.7', 'an IPv4-mapped address is the IPv4 it is');
  assert.equal(addressKey('::ffff:cb00:7107'), '203.0.113.7');
  assert.equal(addressKey('::1'), '0000:0000:0000:0000::/64');
  for (const bad of ['', '   ', 'abc', '999.1.1.1', '1.2.3', '1.2.3.4.5', 'localhost', '::g', '1:2:3:4:5:6:7:8:9', '12345::1', null, undefined, 42]) {
    assert.equal(addressKey(bad), null, String(bad));
  }
});

test('IPv4-mapped addresses do not all collapse into one guest', () => {
  assert.notEqual(addressKey('::ffff:203.0.113.7'), addressKey('::ffff:198.51.100.9'));
});

test('a guest id is a keyed hash: the same address and day agree, a new day or address does not, and the address is not in it', async () => {
  const env = fakeEnv();
  const id = await guestId(env, IP, T0);
  assert.match(id, /^[0-9a-f]{32}$/);
  assert.equal(await guestId(env, IP, T0 + 3600_000), id, 'same UTC day');
  assert.notEqual(await guestId(env, IP, T0 + 24 * 3600_000), id, 'next UTC day');
  assert.notEqual(await guestId(env, '203.0.113.8', T0), id, 'another address');
  assert.ok(!id.includes('203'), 'the address is not readable in the id');
  assert.notEqual(await guestId({ ...env, JWT_SECRET: 'another-secret' }, IP, T0), id, 'another key');
});

// ── Tickets ──────────────────────────────────────────────────────────────────

test('a lease ticket verifies, and a changed or forged one does not', async () => {
  const env = fakeEnv();
  const { ticket } = await signLease(env, { tid: 'tid1', gid: 'gid1', sim: SIM_A, character: 'F', now: T0 });
  const payload = await verifyLease(env, ticket, T0 + 1000);
  assert.deepEqual([payload.t, payload.g, payload.s, payload.c, payload.d], ['tid1', 'gid1', SIM_A, 'F', '2026-09-30']);

  const [prefix, body, mac] = ticket.split('.');
  const forged = Buffer.from(JSON.stringify({ ...payload, g: 'someone-else' })).toString('base64url');
  assert.equal(await verifyLease(env, `${prefix}.${forged}.${mac}`, T0), null, 'payload changed');
  assert.equal(await verifyLease(env, `${prefix}.${body}.${mac.slice(0, -2)}AA`, T0), null, 'mac changed');
  assert.equal(await verifyLease(env, `egl2.${body}.${mac}`, T0), null, 'other prefix');
  assert.equal(await verifyLease(env, `${body}.${mac}`, T0), null, 'two parts');
  assert.equal(await verifyLease({ ...env, JWT_SECRET: 'another-secret' }, ticket, T0), null, 'signed by another key');
  assert.equal(await verifyLease(env, ticket, T0 + 3 * 3600_000), null, 'expired');
  for (const junk of ['', 'x', null, 12, 'a.b.c', 'egl1.%%%.%%%', 'x'.repeat(2000)]) assert.equal(await verifyLease(env, junk, T0), null, String(junk));
});

test('a lease ticket is not an account token, and an account token is not a lease ticket', async () => {
  const env = await seed(environment());
  const { ticket } = await signLease(env, { tid: 't', gid: 'g', sim: SIM_A, character: 'M', now: Date.now() });
  const asAccount = await call(env, 'GET', '/api/auth/me', { token: ticket });
  assert.equal(asAccount.status, 401);
  assert.equal(await verifyLease(env, mintToken('someone'), Date.now()), null);
});

// ── The allowance ────────────────────────────────────────────────────────────

test('a fresh guest has 3 simulations and 60 minutes', async () => {
  const env = await seed(environment());
  const r = await call(env, 'GET', '/api/guest/allowance');
  assert.equal(r.status, 200);
  assert.deepEqual([r.body.sims_left, r.body.seconds_left], [MAX_SIMS, DAILY_SECONDS]);
  assert.match(r.body.resets_at, /T00:00:00\.000Z$/);
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
});

test('opening a simulation gives a ticket and uses a slot; the 4th different simulation is refused with when it resets', async () => {
  await withClock(T0, async () => {
    const env = await seed(environment());
    const first = await play(env, SIM_A);
    assert.equal(first.status, 200, JSON.stringify(first.body));
    assert.equal(first.body.sims_left, 2);
    assert.equal(first.body.character, 'M');
    assert.equal(first.body.beat_interval, 30);
    assert.ok((await verifyLease(env, first.body.ticket, T0)));
    assert.equal(first.headers.get('Cache-Control'), 'no-store');

    assert.equal((await play(env, SIM_B)).status, 200);
    const third = await play(env, SIM_C);
    assert.equal(third.status, 200);
    assert.equal(third.body.sims_left, 0);

    const fourth = await play(env, SIM_D);
    assert.equal(fourth.status, 429);
    assert.equal(fourth.body.code, 'guest_limit');
    assert.equal(fourth.body.reason, 'sims');
    assert.equal(fourth.body.resets_at, '2026-10-01T00:00:00.000Z');
    assert.equal(Number(fourth.headers.get('Retry-After')), 12 * 3600);
    assert.match(fourth.body.error, /Register/);
  });
});

test('opening a simulation already counted today uses no new slot', async () => {
  await withClock(T0, async () => {
    const env = await seed(environment());
    for (const s of [SIM_A, SIM_B, SIM_C]) await play(env, s);
    const again = await play(env, SIM_A);
    assert.equal(again.status, 200);
    assert.equal(again.body.sims_left, 0);
  });
});

test('time is what the Worker measures between beats, capped per beat', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);

    clock.t += 30_000;
    let r = await beat(env, ticket);
    assert.equal(r.status, 200);
    assert.equal(r.body.seconds_left, DAILY_SECONDS - 30);

    // A gap inside the timeout but longer than 1.5 beats is charged at the cap.
    clock.t += 80_000;
    r = await beat(env, ticket);
    assert.equal(r.status, 200);
    assert.equal(r.body.seconds_left, DAILY_SECONDS - 30 - BEAT_CAP_SECONDS);

    // A beat sent a moment later charges a moment.
    clock.t += 1_000;
    r = await beat(env, ticket);
    assert.equal(r.body.seconds_left, DAILY_SECONDS - 30 - BEAT_CAP_SECONDS - 1);
  });
});

test('a lease with no beat for 90 seconds is over, and the time it did not use is not charged', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);
    clock.t += 30_000;
    await beat(env, ticket);
    clock.t += LEASE_TIMEOUT_MS + 1;
    const late = await beat(env, ticket);
    assert.equal(late.status, 410);
    assert.equal(late.body.code, 'lease_expired');
    assert.equal(late.body.expired, true);
    assert.equal(late.body.seconds_left, DAILY_SECONDS - 30, 'only the measured time was charged');
  });
});

test('a second lease ends the first: one ticket serves one player', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: first } = await play(env, SIM_A);
    const { body: second } = await play(env, SIM_B);
    clock.t += 30_000;
    const old = await beat(env, first.ticket);
    assert.equal(old.status, 410);
    assert.equal(old.body.code, 'lease_ended');
    assert.equal((await beat(env, second.ticket)).status, 200);
  });
});

test('ending a lease charges the last stretch and frees the guest to start another', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);
    clock.t += 20_000;
    const ended = await call(env, 'POST', '/api/guest/end', { body: { ticket } });
    assert.equal(ended.status, 200);
    assert.equal(ended.body.seconds_left, DAILY_SECONDS - 20);
    assert.equal((await beat(env, ticket)).body.code, 'lease_ended');
    assert.equal((await play(env, SIM_B)).status, 200);
  });
});

test('using up the 60 minutes ends the lease and refuses the next simulation until midnight UTC', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);
    let last;
    // 30 s beats; the 120th brings the total to 3600.
    for (let i = 0; i < 120; i++) { clock.t += 30_000; last = await beat(env, ticket); if (last.status !== 200) break; }
    assert.equal(last.status, 410);
    assert.equal(last.body.code, 'guest_limit');
    assert.equal(last.body.reason, 'minutes');
    assert.equal(last.body.seconds_left, 0);
    assert.ok(Number(last.headers.get('Retry-After')) > 0);

    const next = await play(env, SIM_B);
    assert.equal(next.status, 429);
    assert.equal(next.body.reason, 'minutes');
    assert.match(next.body.error, /60 minutes/);
  });
});

test('the allowance is per address: another address has its own, and a whole IPv6 /64 shares one', async () => {
  await withClock(T0, async () => {
    const env = await seed(environment());
    for (const s of [SIM_A, SIM_B, SIM_C]) await play(env, s);
    assert.equal((await play(env, SIM_D)).status, 429);
    assert.equal((await play(env, SIM_D, { ip: '198.51.100.9' })).status, 200, 'another address');

    assert.equal((await play(env, SIM_A, { ip: '2001:db8:1:2::1' })).status, 200);
    const sameNetwork = await call(env, 'GET', '/api/guest/allowance', { ip: '2001:db8:1:2:ffff:ffff:ffff:ffff' });
    assert.equal(sameNetwork.body.sims_left, 2, 'the same /64 sees the same allowance');
  });
});

test('the next UTC day is a fresh allowance', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    for (const s of [SIM_A, SIM_B, SIM_C]) await play(env, s);
    assert.equal((await play(env, SIM_D)).status, 429);
    clock.t = Date.UTC(2026, 9, 1, 0, 0, 1);
    const r = await play(env, SIM_D);
    assert.equal(r.status, 200);
    assert.equal(r.body.sims_left, 2);
  });
});

test('a lease that straddles midnight keeps charging the day it started on', async () => {
  await withClock(Date.UTC(2026, 8, 30, 23, 59, 0), async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);
    clock.t += 30_000;
    await beat(env, ticket);
    clock.t = Date.UTC(2026, 9, 1, 0, 0, 30);
    const after = await beat(env, ticket);
    assert.equal(after.status, 200, 'not cut at midnight');
    assert.equal(after.body.seconds_left, DAILY_SECONDS - 30 - BEAT_CAP_SECONDS + 0, 'charged to the old day, capped');
    const fresh = await call(env, 'GET', '/api/guest/allowance');
    assert.equal(fresh.body.seconds_left, DAILY_SECONDS, 'the new day is untouched');
  });
});

test('a beat works from another address, since the ticket names the guest', async () => {
  await withClock(T0, async (clock) => {
    const env = await seed(environment());
    const { body: { ticket } } = await play(env, SIM_A);
    clock.t += 30_000;
    const r = await beat(env, ticket, { ip: '198.51.100.99' });
    assert.equal(r.status, 200);
    assert.equal(r.body.seconds_left, DAILY_SECONDS - 30);
  });
});

test('an allowance is deleted by its alarm 25 hours after its day began', async () => {
  await withClock(T0, async () => {
    const env = await seed(environment());
    await play(env, SIM_A);
    const obj = [...env.GUEST_ALLOWANCE.instances.values()][0];
    assert.equal(env.GUEST_ALLOWANCE.instances.size, 1);
    const storage = obj.storage;
    assert.equal(storage.alarm, Date.UTC(2026, 8, 30, 0, 0, 0) + 25 * 3600_000);
    assert.ok(storage.map.has('a'));
    await obj.alarm();
    assert.equal(storage.map.size, 0);
  });
});

// ── Which simulations ────────────────────────────────────────────────────────

test('only approved, public, all-ages simulations are guest-playable', async () => {
  await withClock(T0, async () => {
    const env = await seed(environment(), [
      sim(SIM_A, { rating: 'teen_13' }),
      sim(SIM_B, { status: 'pending' }),
      sim(SIM_C, {}, { is_public: false }),
      sim(SIM_D, { rating: undefined }),
      sim('eeeeeeee-5555-4555-8555-eeeeeeeeeeee', {}, { browser_play: false }),
      sim('ffffffff-6666-4666-8666-ffffffffffff', {}, { browser_play: true }),
    ]);
    const rated = await play(env, SIM_A);
    assert.equal(rated.status, 403);
    assert.equal(rated.body.reason, 'rating');
    assert.equal((await play(env, SIM_B)).status, 404, 'pending reads as missing');
    assert.equal((await play(env, SIM_C)).status, 404, 'private reads as missing');
    assert.equal((await play(env, SIM_D)).status, 403, 'unrated is refused, not assumed');
    assert.equal((await play(env, 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee')).body.reason, 'browser');
    assert.equal((await play(env, 'ffffffff-6666-4666-8666-ffffffffffff')).status, 200);
    assert.equal((await play(env, '99999999-0000-4000-8000-999999999999')).status, 404, 'unknown');

    const allowance = await call(env, 'GET', '/api/guest/allowance');
    assert.equal(allowance.body.sims_left, 2, 'only the one that opened used a slot');
  });
});

test('a signed-in caller is told to play as themselves and uses no guest allowance', async () => {
  const env = await seed(environment());
  const r = await play(env, SIM_A, { token: mintToken('someone') });
  assert.equal(r.status, 409);
  assert.equal(r.body.code, 'signed_in');
  assert.equal(env.GUEST_ALLOWANCE.instances.size, 0);
});

test('a request names a character and a simulation id the Worker can use', async () => {
  const env = await seed(environment());
  const post = (body) => call(env, 'POST', '/api/guest/play', { body });
  assert.equal((await post({ sim_id: SIM_A })).status, 400);
  assert.equal((await post({ sim_id: SIM_A, character: 'R' })).status, 400, 'robots are not offered to guests');
  assert.equal((await post({ sim_id: SIM_A, character: 'male' })).status, 400);
  assert.equal((await post({ sim_id: '../admin', character: 'M' })).status, 400);
  assert.equal((await post({ character: 'M' })).status, 400);
  assert.equal((await post({ sim_id: 'x'.repeat(100), character: 'F' })).status, 400);
  assert.equal(env.GUEST_ALLOWANCE.instances.size, 0, 'nothing was counted');
});

test('with no readable address the Worker refuses, and with no Durable Object it says so', async () => {
  const env = await seed(environment());
  const none = await call(env, 'POST', '/api/guest/play', { ip: null, body: { sim_id: SIM_A, character: 'M' } });
  assert.equal(none.status, 400);
  assert.equal(none.body.code, 'address_unavailable');
  const bad = await call(env, 'GET', '/api/guest/allowance', { ip: 'not an address' });
  assert.equal(bad.status, 400);

  const unbound = await seed({ ...fakeEnv() });
  const r = await play(unbound, SIM_A);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'guest_unavailable');
});

test('opening a guest session is rate limited per address, like the auth routes', async () => {
  let n = 0;
  const env = await seed(environment({ AUTH_RATE_LIMITER: { limit: async () => ({ success: ++n <= 2 }) } }));
  await withClock(T0, async () => {
    assert.equal((await play(env, SIM_A)).status, 200);
    assert.equal((await play(env, SIM_A)).status, 200);
    const third = await play(env, SIM_A);
    assert.equal(third.status, 429);
    assert.equal(third.body.code, 'rate_limited');
  });
});

test('a ticket that is not valid is refused, and the day in a ticket is the day it was issued', async () => {
  const env = await seed(environment());
  const bad = await beat(env, 'egl1.garbage.garbage');
  assert.equal(bad.status, 410);
  assert.equal(bad.body.code, 'bad_ticket');
  const { body } = await play(env, SIM_A);
  const payload = await verifyLease(env, body.ticket, Date.now());
  assert.equal(payload.d, dayOf(Date.now()));
});
