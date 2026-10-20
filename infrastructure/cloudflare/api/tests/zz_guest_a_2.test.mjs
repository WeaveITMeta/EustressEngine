// PROBE (security review, guest join): /live/admit re-reads the rating for a guest, but not whether the listing is still
// guest-playable (public). A ticket minted while the listing was public is admitted after the author made it private
// (or archived it). GUEST_PLAY.md: guest-playable is `isListable` (approved AND public) and all_ages; the mint route says
// "one taken down, made private or re-rated since the lease opened is not joined". FAILS on the current code.

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

function environment() {
  // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on".
  const env = { ...fakeEnv(), GUEST_PLAY: 'on' };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}
const listing = (extra = {}) => ({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages' }, ...extra });
async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const author = mintToken(AUTHOR);
async function withClock(start, fn) {
  const real = Date.now;
  Date.now = () => start;
  try { return await fn(); } finally { Date.now = real; }
}

for (const [what, changed] of Object.entries({ 'made private': listing({ is_public: false }), 'archived': listing({ is_public: false, archived: true, archived_was_public: true }) })) {
  test(`PROBE: a guest ticket minted while the listing was public is not admitted once the listing is ${what}`, async () => {
    await withClock(T0, async () => {
      const env = environment();
      await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing()));
      assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/live`, { token: author, body: { link: linkFor(PIN), players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: ['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked' /* ADJUSTED (fix): round 3 adds host_chat_checked */] /* ADJUSTED (fix): guests join guest-only sessions */ /* ADJUSTED (fix) */ } })).status, 200);
      const lease = await call(env, 'POST', '/api/guest/play', { headers: IP, body: { sim_id: SIM, character: 'M', client: 'player' } });
      assert.equal(lease.status, 200);
      const minted = await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease: lease.body.ticket, audience: PIN } });
      assert.equal(minted.status, 200);

      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(changed));
      // The mint route itself now refuses (404), proving the listing is no longer guest-playable...
      assert.equal((await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease: lease.body.ticket, audience: PIN } })).status, 404);
      // ...but the ticket minted a moment earlier is still admitted as a guest.
      const r = await call(env, 'POST', `/api/simulations/${SIM}/live/admit`, { token: author, body: { ticket: minted.body.ticket, audience: PIN } });
      assert.notEqual(r.status, 200, `admitted as a guest to a listing that is ${what}: ${JSON.stringify(r.body)}`);
    });
  });
}
