// PROBE (security review, guest join): the public mint route POST /api/guest/ticket is limited by AUTH_RATE_LIMITER keyed on
// the raw CF-Connecting-IP. guest.mjs reduces an IPv6 address to its /64 precisely so "a device cannot step through the
// addresses of its own network", but the limiter does not: every address in one /64 is a fresh budget, so one IPv6 host
// mints without limit (and, with zz_guest_a_3, fills a session's guest seats from one lease). FAILS on the current code.

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
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);

async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

test('PROBE: two addresses in one IPv6 /64 share one mint budget', async () => {
  const real = Date.now;
  Date.now = () => T0;
  try {
    // A limiter that allows 2 calls per key (one lease open, one mint), the real Worker otherwise.
    const used = new Map();
    // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on".
    const env = { ...fakeEnv(), GUEST_PLAY: 'on', AUTH_RATE_LIMITER: { limit: async ({ key }) => { used.set(key, (used.get(key) || 0) + 1); return { success: used.get(key) <= 2 }; } } };
    env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
    env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
    await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
    await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages' } }));
    assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/live`, { token: mintToken(AUTHOR), body: { link: linkFor(PIN), players: 1, max_players: 8, protocol: 3 } })).status, 200);

    const at = (n) => ({ 'CF-Connecting-IP': `2001:db8:1:2::${n.toString(16)}` });
    const lease = (await call(env, 'POST', '/api/guest/play', { headers: at(1), body: { sim_id: SIM, character: 'F', client: 'player' } })).body.ticket;
    let minted = 0;
    for (let n = 1; n <= 20; n++) {
      const r = await call(env, 'POST', '/api/guest/ticket', { headers: at(n), body: { lease, audience: PIN } });
      if (r.status === 200) minted++;
    }
    assert.ok(minted <= 1, `20 addresses of one /64 minted ${minted} tickets under a 2-per-minute budget; limiter keys seen: ${[...used.keys()].length}`);
  } finally {
    Date.now = real;
  }
});
