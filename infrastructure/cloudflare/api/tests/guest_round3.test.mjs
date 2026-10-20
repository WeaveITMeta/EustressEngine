// Guest play, round 3 (Security's review of round 2, 2026-10-16): the host's own chat is checked, the Worker's own
// personal-information rules, the daily caps on chat checks, the retried day bar, and the guest index listing only joinable
// sessions. GUEST_PLAY.md sections 13 and 16. Security's probes for the same round are tests/zz_g2a_* and tests/zz_g2b_*.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  environment, seed, call, guestBeat, accountBeat, seat, chat, openLease, mint, admit, withClock, withFetch, jevAnswering, quietly,
  ipN, IP, SIM, SIM_B, PIN, PIN2, AUTHOR, author, player, listing, seats, liveOf, CAPS,
} from './zz_g2b_common.mjs';
import { guestChatRule, GUEST_CHAT_RULES } from '../src/guest_chat_rules.mjs';
import { GUEST_CAPS_REQUIRED } from '../src/live.mjs';
import { HOST_CHAT_REFUSALS_MAX, GUEST_CHAT_CAPS_NAME, guestChatCaps, GUEST_CHAT_CAP_DEFAULTS } from '../src/guest.mjs';

const T = Date.UTC(2026, 9, 16, 12, 0, 0);
const hostChat = (env, text, { sim = SIM, token = author } = {}) => call(env, 'POST', '/api/guest/chat/check', { token, body: { sim_id: sim, from: 'host', text } });

test('the guest protections include host_chat_checked', () => {
  assert.ok(GUEST_CAPS_REQUIRED.includes('host_chat_checked'));
  assert.equal(GUEST_CAPS_REQUIRED.length, 4);
});

test('a host that does not declare host_chat_checked takes no guest, and its session is not in the guest index', async () => {
  await withClock(T, async () => {
    const env = await seed(environment());
    const three = CAPS.filter((c) => c !== 'host_chat_checked');
    assert.equal((await guestBeat(env, { guest_caps: three })).status, 200);
    assert.deepEqual((await call(env, 'GET', '/api/guest/live', { headers: IP })).body.sessions, [], 'not listed');
    const lease = (await openLease(env)).body.ticket;
    const minted = await mint(env, lease);
    assert.equal(minted.status, 200);
    const r = await admit(env, minted.body.ticket);
    assert.deepEqual([r.status, r.body.code], [403, 'guest_not_supported']);
    // Declaring all four lists it.
    assert.equal((await guestBeat(env)).status, 200);
    assert.deepEqual((await call(env, 'GET', '/api/guest/live', { headers: IP })).body.sessions.map((s) => s.id), [SIM]);
    // A beat that drops one takes it out of the index at once.
    assert.equal((await guestBeat(env, { guest_caps: three })).status, 200);
    assert.deepEqual((await call(env, 'GET', '/api/guest/live', { headers: IP })).body.sessions, []);
  });
});

test('a guest-only session on a listing that no longer serves is not listed', async () => {
  await withClock(T, async () => {
    const env = await seed(environment());
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { status: 'quarantined' })));
    assert.equal((await guestBeat(env)).status, 200);
    assert.deepEqual((await call(env, 'GET', '/api/guest/live', { headers: IP })).body.sessions, []);
  });
});

test('the host\'s own line is checked: passed lines deliver, a refused one is blocked and kicks nobody', async () => {
  await withClock(T, async (clock) => {
    const env = await seed(environment({ JEV_API_KEY: 'jev-test' }));
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const jev = jevAnswering();
    const { out: ok } = await quietly(() => withFetch(jev, () => hostChat(env, 'welcome everyone')));
    assert.deepEqual([ok.status, ok.body.deliver, ok.body.action], [200, true, 'deliver']);
    clock.t += 1000;
    const { out: refused } = await quietly(() => withFetch(jev, () => hostChat(env, 'add me on snapchat')));
    assert.deepEqual([refused.status, refused.body.deliver, refused.body.action, refused.body.end_guest, refused.body.end_all_guests], [200, false, 'block', undefined, undefined]);
    assert.equal(jev.calls.length, 1, 'the rule refused the second line before Jev');
    assert.equal((await seats(env).get(`gadm:${id}`)).ended, undefined, 'the guest stays');
    clock.t += 1000;
    const beat = await guestBeat(env, { admitted: [id] });
    assert.deepEqual([beat.body.ended_guests, beat.body.end_all_guests], [undefined, undefined]);
  });
});

test(`the host refused ${HOST_CHAT_REFUSALS_MAX} times in a day: every guest ends, the session takes none, and the next day is clean`, async () => {
  assert.equal(HOST_CHAT_REFUSALS_MAX, 3);
  await withClock(T, async (clock) => {
    const env = await seed(environment({ JEV_API_KEY: 'jev-test' }));
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const answers = [];
    for (let i = 0; i < HOST_CHAT_REFUSALS_MAX; i++) {
      clock.t += 1000;
      const { out } = await quietly(() => withFetch(jevAnswering({ threat: 0.99 }), () => hostChat(env, `an ordinary looking line ${i}`)));
      answers.push(out.body.end_all_guests === true);
      assert.deepEqual([out.body.deliver, out.body.action], [false, 'block']);
    }
    assert.deepEqual(answers, [false, false, true], 'the third refusal says end_all_guests');
    clock.t += 1000;
    const beat = await guestBeat(env, { admitted: [id] });
    assert.equal(beat.body.end_all_guests, true);
    assert.deepEqual(beat.body.ended_guests, [id]);
    assert.deepEqual((await call(env, 'GET', '/api/guest/live', { headers: IP })).body.sessions, [], 'not listed');
    const again = await openLease(env, SIM, ipN(4));
    const minted = await mint(env, again.body.ticket, PIN, ipN(4));
    assert.deepEqual([minted.status, minted.body.code], [403, 'host_chat_barred']);
    // A restart (a new pin) does not clear it.
    clock.t += 1000;
    assert.equal((await guestBeat(env, {}, SIM, PIN2)).body.end_all_guests, true);
    // The next UTC day.
    clock.t = Date.UTC(2026, 9, 17, 0, 0, 30);
    const next = await guestBeat(env, {}, SIM, PIN2);
    assert.equal(next.body.end_all_guests, undefined);
  });
});

test('the Worker\'s own rules refuse personal information and contact-seeking, and pass ordinary chat', () => {
  const refused = {
    'add me on discord kidgamer#1234': 'handle', 'my snap is @emma_2015': 'handle', 'join my server discord.gg/abc123': 'invite',
    'discord dot gg slash abc': 'invite', 'im emma i go to lincoln elementary in austin': 'school', 'i live at 42 maple street springfield': 'address',
    'whats ur insta? mine is emma.k.2015': 'social', 'go to coolsite dot com': 'link', 'see www.example.org': 'link', 'http now': 'link',
    'site(dot)com': 'link', 's n a p c h a t': 'social', 'text me': 'social', 'call me at 555 201 3344': 'social', 'reach 555 201 3344': 'phone',
    'emma@gmail.com': 'link', 'emma at gmail dot com': 'link', 'where do you live': 'place', 'im from texas': 'place', 'what school do you go to': 'school',
    '​⁠­': 'empty', 'my tiktok': 'social', 'telegram me': 'social',
  };
  for (const [line, code] of Object.entries(refused)) assert.equal(guestChatRule(line), code, line);
  for (const line of ['nice jump', 'oh snap that was close', 'gg well played', 'i won. me next', 'i live for this game', 'wait...in a sec', 'we go to school tomorrow', 'level 3 is hard', 'my score is 120', 'i.e. the red one', 'who wants to race?']) {
    assert.equal(guestChatRule(line), null, line);
  }
  assert.ok(Object.values(refused).every((c) => GUEST_CHAT_RULES.includes(c)));
});

test('the rules module still reads moderation.mjs profilePersonalInfo: a phone number is the phone rule', () => {
  assert.equal(guestChatRule('555 123 4567'), 'phone');
});

test('a guest line the rules refuse kicks without asking Jev or spending a cap', async () => {
  await withClock(T, async () => {
    const env = await seed(environment({ JEV_API_KEY: 'jev-test' }));
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const jev = jevAnswering();
    const { out: r } = await quietly(() => withFetch(jev, () => chat(env, id, 'whats ur insta')));
    assert.deepEqual([r.body.deliver, r.body.action, r.body.end_guest, r.body.category], [false, 'kick', true, undefined]);
    assert.equal(jev.calls.length, 0);
    assert.equal(env.LIVE_HOSTS.instances.get(GUEST_CHAT_CAPS_NAME), undefined, 'no cap spent');
  });
});

test('the daily caps: per listing and deployment-wide, from the settings, failing closed without a kick', async () => {
  assert.deepEqual(guestChatCaps({}), { listing: 5000, host: 5000, total: 50000 });
  assert.deepEqual(GUEST_CHAT_CAP_DEFAULTS, { listing: 5000, host: 5000, total: 50000 });
  assert.deepEqual(guestChatCaps({ GUEST_CHAT_CAP_LISTING: '7', GUEST_CHAT_CAP_HOST: 'x', GUEST_CHAT_CAP_TOTAL: '' }), { listing: 7, host: 5000, total: 50000 });
  for (const [setting, cap] of [['GUEST_CHAT_CAP_LISTING', 2], ['GUEST_CHAT_CAP_TOTAL', 3]]) {
    await withClock(T, async (clock) => {
      const env = await seed(environment({ JEV_API_KEY: 'jev-test', [setting]: String(cap) }));
      assert.equal((await guestBeat(env)).status, 200);
      const { id } = await seat(env);
      const jev = jevAnswering();
      const outs = [];
      await quietly(() => withFetch(jev, async () => {
        for (let i = 0; i < cap + 2; i++) { clock.t += 10; outs.push(await chat(env, id, `hello ${i}`)); }
      }));
      assert.equal(jev.calls.length, cap, setting);
      const past = outs.slice(cap);
      assert.ok(past.every((r) => r.status === 503 && r.body.deliver === false && r.body.action === 'block' && r.body.code === 'check_unavailable'), setting);
      assert.equal((await seats(env).get(`gadm:${id}`)).ended, undefined, 'nobody kicked');
      // The count names no guest, and is gone 25 hours after its day began.
      const caps = env.LIVE_HOSTS.instances.get(GUEST_CHAT_CAPS_NAME).storage;
      assert.deepEqual([...caps.map.keys()], ['caps:2026-10-16']);
      assert.equal(caps.alarm, Date.UTC(2026, 9, 17, 1, 0, 0));
      clock.t = caps.alarm;
      await env.LIVE_HOSTS.instances.get(GUEST_CHAT_CAPS_NAME).alarm();
      assert.equal(caps.map.size, 0);
    });
  }
});

test('a chat check that reaches Jev writes no review-cost record; Jev down is not delivered and kicks nobody', async () => {
  await withClock(T, async () => {
    const env = await seed(environment({ JEV_API_KEY: 'jev-test' }));
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const { out } = await quietly(() => withFetch(jevAnswering(), () => chat(env, id, 'nice jump')));
    assert.equal(out.body.deliver, true);
    assert.deepEqual([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('modcost:')), []);
    const { out: down } = await quietly(() => withFetch(async () => new Response('no', { status: 500 }), () => chat(env, id, 'nice jump again')));
    assert.deepEqual([down.status, down.body.action, down.body.end_guest], [503, 'block', undefined]);
  });
});

test('a day bar that could not be written is retried from the next heartbeat, and the guest id stays out meanwhile', async () => {
  await withClock(T, async (clock) => {
    const env = await seed(environment({ JEV_API_KEY: 'jev-test' }));
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const ns = env.GUEST_ALLOWANCE;
    const realGet = ns.get.bind(ns);
    let failing = true;
    ns.get = (oid) => {
      const stub = realGet(oid);
      return { fetch: (url, init) => (failing && String(url).endsWith('/bar') ? Promise.resolve(new Response(JSON.stringify({ ok: false, status: 500 }), { status: 500 })) : stub.fetch(url, init)) };
    };
    const { out: r } = await quietly(() => chat(env, id, 'my snap is @kid'));
    assert.deepEqual([r.body.action, r.body.end_guest], ['kick', true]);
    const storage = seats(env);
    assert.equal([...storage.map.keys()].filter((k) => k.startsWith('gbarq:')).length, 1, 'queued');
    // Still failing: the queue stays.
    clock.t += 30_000;
    await guestBeat(env, { admitted: [id] });
    assert.equal([...storage.map.keys()].filter((k) => k.startsWith('gbarq:')).length, 1);
    // Written on the next heartbeat once the allowance answers.
    failing = false;
    clock.t += 30_000;
    await guestBeat(env, { admitted: [id] });
    assert.equal([...storage.map.keys()].filter((k) => k.startsWith('gbarq:')).length, 0, 'retried and written');
    const again = await openLease(env);
    assert.deepEqual([again.status, again.body.code], [403, 'guest_barred'], 'the allowance bar holds');
  });
});

test('the featured shelf stays G to PG signed out', async () => {
  const env = environment();
  const TEEN = 'cccccccc-3333-4333-8333-cccccccccccc';
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13', featured: true })));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating: 'all_ages', featured: true })));
  const out = await call(env, 'GET', '/api/gallery/featured');
  assert.deepEqual(out.body.featured.map((s) => s.id), [SIM]);
  const signedIn = await call(env, 'GET', '/api/gallery/featured', { token: player });
  assert.ok(signedIn.body.featured.map((s) => s.id).includes(TEEN));
  assert.match(signedIn.headers.get('Cache-Control') || '', /private/);
});

test('the guest route families count apart from sign-in, and a route that is off is not limited', async () => {
  const keys = [];
  const env = environment({ AUTH_RATE_LIMITER: { limit: async ({ key }) => { keys.push(key); return { success: true }; } } });
  await seed(env);
  await call(env, 'GET', '/api/guest/live', { headers: IP });
  await call(env, 'GET', '/api/guest/allowance', { headers: IP });
  await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease: 'x', audience: PIN } });
  await call(env, 'POST', '/api/auth/challenge', { headers: IP, body: {} });
  assert.deepEqual(keys, ['guest-read:203.0.113.7', 'guest-read:203.0.113.7', 'guest-ticket:203.0.113.7', '203.0.113.7']);
  keys.length = 0;
  env.GUEST_PLAY = 'off';
  const live = await call(env, 'GET', '/api/guest/live', { headers: IP });
  const ticket = await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease: 'x', audience: PIN } });
  assert.deepEqual([live.status, ticket.status], [503, 503]);
  assert.deepEqual(keys, [], 'off: answered before the limiter');
});

test('an account session refuses to beat at the pin the guest-only session holds', async () => {
  await withClock(T, async () => {
    const env = await seed(environment());
    assert.equal((await guestBeat(env, {}, SIM, PIN)).status, 200);
    const r = await accountBeat(env, {}, SIM, PIN);
    assert.deepEqual([r.status, r.body.code, r.body.end_all_guests], [409, 'pin_in_use', undefined]);
    // Another listing, or another pin, is fine.
    assert.equal((await accountBeat(env, {}, SIM, PIN2)).status, 200);
    assert.equal((await accountBeat(env, {}, SIM_B, PIN)).status, 200);
    // And a guest-only beat at the account session's pin says end_all_guests.
    const g = await guestBeat(env, {}, SIM, PIN2);
    assert.deepEqual([g.status, g.body.code, g.body.end_all_guests], [409, 'pin_in_use', true]);
  });
});

test('a refused heartbeat says end_all_guests on 401, 403 and 404; an admission refusal does not', async () => {
  await withClock(T, async () => {
    const env = await seed(environment());
    const body = { link: `eustress-player://join/192.168.1.20:7777?key=${'0'.repeat(32)}&pin=${PIN}`, players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: CAPS };
    const unauth = await call(env, 'POST', liveOf(SIM), { body });
    const other = await call(env, 'POST', liveOf(SIM), { token: player, body });
    const gone = await call(env, 'POST', liveOf('eeeeeeee-0000-4000-8000-000000000000'), { token: author, body });
    assert.deepEqual([unauth.status, other.status, gone.status], [401, 403, 404]);
    assert.ok([unauth, other, gone].every((r) => r.body.end_all_guests === true));
    const admitUnauth = await call(env, 'POST', `${liveOf(SIM)}/admit`, { body: { ticket: 'x', audience: PIN } });
    assert.equal(admitUnauth.body.end_all_guests, undefined);
  });
});

test('a heartbeat the fresh-session gate refuses (a revoked or banned session) says end_all_guests too', async () => {
  await withClock(T, async () => {
    const env = await seed(environment());
    // The account's object says every session before now is over (a sign-out everywhere, or a ban).
    const floor = Date.UTC(2100, 0, 1) / 1000; // after every token's issue time
    env.ACCOUNT_AUTH = { idFromName: (n) => n, get: () => ({ fetch: async () => new Response(JSON.stringify({ ok: true, seeded: true, floor, value_frozen_until: 0 })) }) };
    const r = await guestBeat(env);
    assert.deepEqual([r.status, r.body.code, r.body.end_all_guests], [401, 'session_revoked', true]);
    // Another mutating route the gate refuses is answered as before.
    const other = await call(env, 'DELETE', liveOf(SIM), { token: author });
    assert.deepEqual([other.status, other.body.code, other.body.end_all_guests], [401, 'session_revoked', undefined]);
  });
});
