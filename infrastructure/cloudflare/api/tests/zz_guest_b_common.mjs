// Shared setup for the zz_guest_b_* review probes (setup copied from guest_join.test.mjs). Not a test file itself.
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';

export const worker = await loadWorker();

export const AUTHOR = '11111111-1111-4111-8111-111111111111';
export const AUTHOR2 = '33333333-3333-4333-8333-333333333333';
export const PLAYER = '22222222-2222-4222-8222-222222222222';
export const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
export const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
export const KEY = '0123456789abcdef0123456789abcdef';
export const PIN = 'ab'.repeat(32);
export const PIN2 = 'cd'.repeat(32);
export const IP = { 'CF-Connecting-IP': '203.0.113.7' };
export const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
export const T0 = Date.UTC(2026, 9, 16, 12, 0, 0);

export function environment(extra = {}) {
  // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on".
  const env = { ...fakeEnv(), GUEST_PLAY: 'on', ...extra };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

export const listing = (id, moderation = {}, author = AUTHOR) => ({
  id, name: 'Pong', author_id: author, is_public: true, moderation: { status: 'approved', rating: 'all_ages', ...moderation },
});

export async function seed(env, { simBAuthor = AUTHOR } = {}) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${AUTHOR2}`, JSON.stringify({ kyc_verified: true, id: AUTHOR2, username: 'host2' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
  await env.SOCIAL.put(`sim:${SIM_B}`, JSON.stringify(listing(SIM_B, {}, simBAuthor)));
  return env;
}

export async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

export const tokenOf = (who) => mintToken(who);
export const liveOf = (sim) => `/api/simulations/${sim}/live`;
// ADJUSTED (fix): a host declares the guest protections before it takes a guest.
export const hostBeat = (env, extra = {}, sim = SIM, pin = PIN, token = mintToken(AUTHOR)) =>
  call(env, 'POST', liveOf(sim), { token, body: { link: linkFor(pin), players: 1, max_players: 8, protocol: 3, guest_only: true, guest_caps: ['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked' /* ADJUSTED (fix): round 3 adds host_chat_checked */] /* ADJUSTED (fix): guests join guest-only sessions */, ...extra } });
export const admit = (env, ticket, { sim = SIM, audience = PIN, token = mintToken(AUTHOR) } = {}) =>
  call(env, 'POST', `${liveOf(sim)}/admit`, { token, body: { ticket, audience } });
export const openLease = (env, sim = SIM, headers = IP, token) =>
  call(env, 'POST', '/api/guest/play', { headers, token, body: { sim_id: sim, character: 'F', client: 'player' } });
export const mint = (env, lease, audience = PIN, headers = IP) => call(env, 'POST', '/api/guest/ticket', { headers, body: { lease, audience } });

export async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

export async function hostedWithGuest(env) {
  await seed(env);
  assert.equal((await hostBeat(env)).status, 200);
  const lease = await openLease(env);
  assert.equal(lease.status, 200, JSON.stringify(lease.body));
  return lease.body.ticket;
}

export async function guestTicket(env, lease, audience = PIN) {
  const r = await mint(env, lease, audience);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

export const payloadOf = (ticket) => JSON.parse(Buffer.from(ticket.split('.')[1], 'base64url').toString());
