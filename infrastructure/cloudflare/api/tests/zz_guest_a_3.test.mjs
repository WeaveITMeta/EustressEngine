// PROBE (security review, guest join): one guest lease mints any number of join tickets, and /live/admit admits every one
// of them. Nothing ties a seat to a lease, so:
//   (a) one lease holds many seats in one session at once (one allowance, one person's time, shared by many players);
//   (b) one anonymous lease fills the session's MAX_GUESTS and turns away every other guest (denial of service);
//   (c) every admission counts a new visit (`g:<ticket id>`), so an author can raise their own listing's visits and
//       totalPlays without a QUIC client: open a guest lease signed out, mint, and POST /live/admit with the author token.
// Each test FAILS on the current code if the defect is present.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, MAX_GUESTS } from '../src/live.mjs';
import { GuestAllowance, DAILY_SECONDS } from '../src/guest.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const ATTACKER = { 'CF-Connecting-IP': '203.0.113.7' };
const VICTIM = { 'CF-Connecting-IP': '198.51.100.9' };
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);

function environment() {
  // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on".
  const env = { ...fakeEnv(), GUEST_PLAY: 'on' };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}
async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const author = mintToken(AUTHOR);
const hostBeat = (env, extra = {}) => call(env, 'POST', `/api/simulations/${SIM}/live`, { token: author, body: { link: linkFor(PIN), players: 1, max_players: 200, protocol: 3, guest_only: true, guest_caps: ['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked' /* ADJUSTED (fix): round 3 adds host_chat_checked */] /* ADJUSTED (fix): guests join guest-only sessions */ /* ADJUSTED (fix) */, ...extra } });
const admit = (env, ticket) => call(env, 'POST', `/api/simulations/${SIM}/live/admit`, { token: author, body: { ticket, audience: PIN } });
const openLease = (env, headers) => call(env, 'POST', '/api/guest/play', { headers, body: { sim_id: SIM, character: 'F', client: 'player' } });
const mint = (env, lease, headers = ATTACKER) => call(env, 'POST', '/api/guest/ticket', { headers, body: { lease, audience: PIN } });
async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}
async function hosted(env) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages' } }));
  assert.equal((await hostBeat(env)).status, 200);
}

test('PROBE (a): one lease holds at most one seat in a session at a time', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await hosted(env);
    const lease = (await openLease(env, ATTACKER)).body.ticket;
    const first = (await mint(env, lease)).body;
    // The second ticket is minted from another address: mint works from anywhere (the lease is the proof).
    const second = (await mint(env, lease, VICTIM)).body;
    assert.equal((await admit(env, first.ticket)).status, 200);
    const again = await admit(env, second.ticket);
    // Two people on one allowance: both are named connected; two minutes of beats charge two minutes, not four.
    for (let i = 0; i < 4; i++) { clock.t += 30_000; await hostBeat(env, { admitted: [first.id, second.id] }); }
    const left = (await call(env, 'GET', '/api/guest/allowance', { headers: ATTACKER })).body.seconds_left;
    assert.notEqual(again.status, 200, `a second seat from the same lease was admitted (${JSON.stringify(again.body)}); two players then used ${DAILY_SECONDS - left} s of one allowance between them`);
  });
});

test('PROBE (b): one anonymous lease cannot fill the session\'s guest cap and lock out every other guest', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await hosted(env);
    const lease = (await openLease(env, ATTACKER)).body.ticket;
    let seats = 0;
    for (let i = 0; i < MAX_GUESTS; i++) {
      const t = (await mint(env, lease)).body.ticket;
      if ((await admit(env, t)).status === 200) seats++;
    }
    const victimLease = (await openLease(env, VICTIM)).body.ticket;
    const victimTicket = (await mint(env, victimLease, VICTIM)).body.ticket;
    const victim = await admit(env, victimTicket);
    assert.equal(victim.status, 200, `one lease took ${seats} of ${MAX_GUESTS} guest seats; an unrelated guest was refused: ${JSON.stringify(victim.body)}`);
  });
});

test('PROBE (c): one guest lease counts at most one visit a day, however many tickets it mints', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await hosted(env);
    const lease = (await openLease(env, ATTACKER)).body.ticket;
    // ADJUSTED (fix): only the first of these is admitted now (one seat per lease), so each admission is not asserted.
    for (let i = 0; i < 25; i++) await admit(env, (await mint(env, lease)).body.ticket);
    const visits = Number(await env.SOCIAL.get(`visits:${SIM}`));
    const plays = Number(await env.SOCIAL.get(`totalPlays:${AUTHOR}`));
    assert.ok(visits <= 1, `one lease, 25 tickets admitted by the author itself: visits:${SIM} = ${visits}, totalPlays:${AUTHOR} = ${plays}`);
  });
});
