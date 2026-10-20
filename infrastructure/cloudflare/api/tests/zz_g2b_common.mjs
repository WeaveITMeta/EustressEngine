// Shared rig for the zz_g2b_* review probes (Security, guest round 2). Not a test file itself. Setup follows
// tests/guest_hardening.test.mjs: the real Worker against in-memory storage.
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, GUEST_CAPS_REQUIRED, guestSessionName } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { GUEST_CHAT_QUESTION_CATEGORIES } from '../src/moderation.mjs';

export const worker = await loadWorker();

export const AUTHOR = '11111111-1111-4111-8111-111111111111';
export const PLAYER = '22222222-2222-4222-8222-222222222222';
export const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
export const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
export const KEY = '0123456789abcdef0123456789abcdef';
export const PIN = 'ab'.repeat(32);
export const PIN2 = 'cd'.repeat(32);
export const IP = { 'CF-Connecting-IP': '203.0.113.7' };
export const ipN = (n) => ({ 'CF-Connecting-IP': `198.51.100.${n}` });
export const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
export const CAPS = [...GUEST_CAPS_REQUIRED];
export const allow = { limit: async () => ({ success: true }) };

/// A limiter that behaves like Cloudflare's simple limiter: `limit` per key per 60 s of Date.now.
export function windowLimiter(limit = 20) {
  const hits = new Map();
  return {
    hits,
    async limit({ key }) {
      const now = Date.now();
      const recent = (hits.get(key) || []).filter((t) => now - t < 60_000);
      if (recent.length >= limit) { hits.set(key, recent); return { success: false }; }
      recent.push(now);
      hits.set(key, recent);
      return { success: true };
    },
  };
}

export function environment(extra = {}) {
  const env = { ...fakeEnv(), GUEST_PLAY: 'on', GUEST_CHAT_RATE_LIMITER: allow, ...extra };
  for (const [k, v] of Object.entries(extra)) if (v === undefined) delete env[k];
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  return env;
}

export const listing = (id, moderation = {}, extra = {}) => ({
  id, name: `Listing ${id.slice(0, 4)}`, author_id: AUTHOR, is_public: true, moderation: { status: 'approved', rating: 'all_ages', ...moderation }, ...extra,
});

export async function seed(env) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)));
  await env.SOCIAL.put(`sim:${SIM_B}`, JSON.stringify(listing(SIM_B)));
  return env;
}

export async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

export const author = mintToken(AUTHOR);
export const player = mintToken(PLAYER);
export const liveOf = (sim) => `/api/simulations/${sim}/live`;
export const guestBeat = (env, extra = {}, sim = SIM, pin = PIN) => call(env, 'POST', liveOf(sim), { token: author, body: { link: linkFor(pin), players: 1, max_players: 200, protocol: 3, guest_only: true, guest_caps: CAPS, ...extra } });
export const accountBeat = (env, extra = {}, sim = SIM, pin = PIN2) => call(env, 'POST', liveOf(sim), { token: author, body: { link: linkFor(pin), players: 1, max_players: 8, protocol: 3, ...extra } });
export const admit = (env, ticket, { sim = SIM, audience = PIN, guestOnly = true } = {}) => call(env, 'POST', `${liveOf(sim)}/admit`, { token: author, body: { ticket, audience, guest_only: guestOnly } });
export const openLease = (env, sim = SIM, headers = IP) => call(env, 'POST', '/api/guest/play', { headers, body: { sim_id: sim, character: 'F', client: 'player' } });
export const mint = (env, lease, audience = PIN, headers = IP) => call(env, 'POST', '/api/guest/ticket', { headers, body: { lease, audience } });
export const chat = (env, id, text, { sim = SIM, token = author } = {}) => call(env, 'POST', '/api/guest/chat/check', { token, body: { sim_id: sim, id, text } });
export const seats = (env, sim = SIM) => env.LIVE_HOSTS.instances.get(guestSessionName(sim))?.storage;
export const payloadOf = (ticket) => JSON.parse(Buffer.from(ticket.split('.')[1], 'base64url').toString());

export async function withClock(start, fn) {
  const real = Date.now;
  const clock = { t: start };
  Date.now = () => clock.t;
  try { return await fn(clock); } finally { Date.now = real; }
}

/// One guest from `headers` connected to the guest-only session of `sim` (the session must already be beating).
export async function seat(env, headers = IP, sim = SIM) {
  const lease = await openLease(env, sim, headers);
  assert.equal(lease.status, 200, JSON.stringify(lease.body));
  const minted = await mint(env, lease.body.ticket, PIN, headers);
  assert.equal(minted.status, 200, JSON.stringify(minted.body));
  const r = await admit(env, minted.body.ticket, { sim });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return { lease: lease.body.ticket, id: minted.body.id };
}

/// Jev, answering the forum battery with every category at `low` and `over` raising some. Counts its calls.
export function jevAnswering(over = {}, low = 0.02) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url: String(url), body: JSON.parse(init.body) });
    // ADJUSTED (guest_chat field): the stub answers the guest chat battery.
    const answers = {};
    for (const c of GUEST_CHAT_QUESTION_CATEGORIES) answers[c] = { type: 'noul', noul: over[c] ?? low };
    return new Response(JSON.stringify({ model: 'jev-latest', answers, usage: { input_tokens: 10, output_tokens: 2 } }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  fn.calls = calls;
  return fn;
}

export async function withFetch(fetchImpl, fn) {
  const real = globalThis.fetch;
  globalThis.fetch = fetchImpl;
  try { return await fn(); } finally { globalThis.fetch = real; }
}

export async function quietly(fn) {
  const real = { log: console.log, warn: console.warn, error: console.error };
  const lines = [];
  console.log = (...a) => lines.push(a.map(String).join(' '));
  console.warn = console.log;
  console.error = console.log;
  try { return { out: await fn(), lines }; } finally { Object.assign(console, real); }
}
