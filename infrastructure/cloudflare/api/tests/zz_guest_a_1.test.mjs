// PROBE (security review, guest join): a guest already connected to a hosted session stays connected, and its lease keeps
// being beaten, after the listing stops being guest-eligible (re-rated adult or above all_ages, quarantined, sent back to
// review, made private). The policy: a guest must never see adult content and may join only an all_ages, non-adult
// listing. The Worker's only lever on a connected guest is `ended_guests` in the heartbeat answer, so the heartbeat must
// name such a guest. Each test FAILS on the current code if the defect is present.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance, DAILY_SECONDS } from '../src/guest.mjs';

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
const listing = (id, moderation = {}, extra = {}) => ({
  id, name: 'Pong', author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages', ...moderation }, ...extra,
});
async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const author = mintToken(AUTHOR);
const liveOf = (sim) => `/api/simulations/${sim}/live`;
const hostBeat = (env, extra = {}) => call(env, 'POST', liveOf(SIM), { token: author, body: { link: linkFor(PIN), players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: ['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked' /* ADJUSTED (fix): round 3 adds host_chat_checked */] /* ADJUSTED (fix): guests join guest-only sessions */ /* ADJUSTED (fix) */, ...extra } });
const admit = (env, ticket) => call(env, 'POST', `${liveOf(SIM)}/admit`, { token: author, body: { ticket, audience: PIN } });
async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

/// A guest admitted to an all-ages hosted session; returns its ticket id.
async function connectedGuest(env) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
  assert.equal((await hostBeat(env)).status, 200);
  const lease = await call(env, 'POST', '/api/guest/play', { headers: IP, body: { sim_id: SIM, character: 'F', client: 'player' } });
  assert.equal(lease.status, 200, JSON.stringify(lease.body));
  const minted = await call(env, 'POST', '/api/guest/ticket', { headers: IP, body: { lease: lease.body.ticket, audience: PIN } });
  assert.equal(minted.status, 200, JSON.stringify(minted.body));
  const r = await admit(env, minted.body.ticket);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return minted.body.id;
}

const CHANGES = {
  're-rated adult_18': listing(SIM, { rating: 'adult_18' }),
  're-rated adult_21': listing(SIM, { rating: 'adult_21' }),
  're-rated mature_17': listing(SIM, { rating: 'mature_17' }),
  'rating removed': listing(SIM, { rating: undefined }),
  'quarantined': listing(SIM, { status: 'quarantined' }),
  'sent back to review (pending)': listing(SIM, { status: 'pending' }),
  'made private (archived)': listing(SIM, {}, { is_public: false }),
};

for (const [what, changed] of Object.entries(CHANGES)) {
  test(`PROBE: a connected guest is let go once its listing is ${what}`, async () => {
    await withClock(T0, async (clock) => {
      const env = environment();
      const id = await connectedGuest(env);
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(changed));
      clock.t += 30_000;
      const beat = await hostBeat(env, { admitted: [id] });
      assert.equal(beat.status, 200, JSON.stringify(beat.body));
      const left = await call(env, 'GET', '/api/guest/allowance', { headers: IP });
      const charged = DAILY_SECONDS - left.body.seconds_left;
      assert.deepEqual(
        beat.body.ended_guests,
        [id],
        `listing ${what}: the heartbeat answered ${JSON.stringify(beat.body)} (no ended_guests) and beat the guest's lease (${charged} s charged), so the guest stays`,
      );
    });
  });
}
