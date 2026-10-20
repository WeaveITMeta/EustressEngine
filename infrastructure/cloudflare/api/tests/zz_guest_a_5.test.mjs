// PROBE (security review, guest join): LiveHost.beat forgets a guest the host does not name when `now - g.at > 90 s`, where
// `g.at` is the ADMISSION time, not the last time the host named it. So a guest connected for more than 90 s is dropped
// from the session's records by ONE heartbeat that omits it (a host-side race, a truncated list). From then on, while the
// host goes on naming it, its lease is never beaten (no time is charged) and, once the lease ends, it is never reported in
// `ended_guests`: the only lever the Worker has to get a guest let go is lost. The design says "a guest the host stops
// naming is forgotten after 90 s". FAILS on the current code.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const IP = { 'CF-Connecting-IP': '203.0.113.7' };
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);

async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const author = mintToken(AUTHOR);
const hostBeat = (env, admitted) => call(env, 'POST', `/api/simulations/${SIM}/live`, { token: author, body: { link: linkFor(PIN), players: 1, max_players: 8, protocol: 3, admitted, guest_only: true, guest_caps: ['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked' /* ADJUSTED (fix): round 3 adds host_chat_checked */] /* ADJUSTED (fix): guests join guest-only sessions */ /* ADJUSTED (fix) */ } });

test('PROBE: one heartbeat that omits a long-connected guest loses it for good: never charged again, never reported ended', async () => {
  const real = Date.now;
  const clock = { t: T0 };
  Date.now = () => clock.t;
  try {
    // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on".
    const env = { ...fakeEnv(), GUEST_PLAY: 'on' };
    env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
    env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
    await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages' } }));
    assert.equal((await hostBeat(env, [])).status, 200);
    const lease = (await call(env, 'POST', '/api/guest/play', { headers: IP, body: { sim_id: SIM, character: 'F', client: 'player' } })).body.ticket;
    const { ticket, id } = (await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease, audience: PIN } })).body;
    assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/live/admit`, { token: author, body: { ticket, audience: PIN } })).status, 200);

    // Connected and named for two minutes.
    for (let i = 0; i < 4; i++) { clock.t += 30_000; await hostBeat(env, [id]); }
    // One beat that does not name it (the guest is still connected).
    clock.t += 30_000; await hostBeat(env, []);
    // The host goes on naming it for ten more minutes.
    const answers = [];
    for (let i = 0; i < 20; i++) { clock.t += 30_000; answers.push((await hostBeat(env, [id])).body); }
    const leaseEnded = (await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease, audience: PIN } })).body.code;
    const left = (await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left;
    const reported = answers.some((a) => (a.ended_guests || []).includes(id));
    // ADJUSTED (fix): the grace now runs from the last beat that named the guest, so the one omission no longer loses it:
    // the guest stays tracked, its lease beaten and charged for the whole time connected. Either outcome lets the Worker
    // keep its lever on the guest; the defect is the third (forgotten, uncharged, never reported).
    const tracked = leaseEnded === undefined && 3600 - left >= 12 * 60;
    assert.ok(reported || tracked, `after ONE beat 30 s long without it, the guest is forgotten: its lease ${leaseEnded === 'lease_ended' ? 'has ended' : 'is ' + leaseEnded}, ${3600 - left} s charged in total for 12.5 minutes connected, and 20 later beats naming it never reported it in ended_guests`);
  } finally {
    Date.now = real;
  }
});
