// Who may join a hosted session depends on the listing, not only on the ticket. A listing that is not listable (unapproved,
// private, quarantined, or under a legal hold) admits its author only: any signed-in account can mint a ticket for a pin it
// holds a link to, so the Worker must check the ticket's account before the Durable Object spends the ticket (Security's
// ruling, 2026-10-07). Checked against the real Worker, with the same fakes live_admit.test.mjs uses.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222';
const OTHER = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;
const REFUSAL = { error: 'This session is not available right now.', code: 'not_available' };

function environment() {
  const env = { ...fakeEnv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

async function seed(env, { moderation = { status: 'approved', rating: 'all_ages' }, is_public = true } = {}) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'bob' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public, moderation }));
  return env;
}

async function call(env, method, path, { token, body } = {}) {
  const h = {};
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

const tokens = { author: mintToken(AUTHOR), player: mintToken(PLAYER), other: mintToken(OTHER) };
const live = `/api/simulations/${SIM}/live`;
const beat = (env) => call(env, 'POST', live, { token: tokens.author, body: { link: linkFor(PIN), players: 1, max_players: 4, protocol: 3 } });
// The host (the author's token) asks; the joiner's ticket is what is checked.
const admit = (env, ticket) => call(env, 'POST', `${live}/admit`, { token: tokens.author, body: { ticket, audience: PIN } });

async function ticketFor(env, who) {
  const r = await call(env, 'POST', '/api/identity/ticket', { token: tokens[who], body: { audience: PIN } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body.ticket;
}

async function hosting(env, options) {
  await seed(env, options);
  assert.equal((await beat(env)).status, 200);
  return env;
}

async function setSim(env, patch) {
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, ...patch }));
}

test('an approved listing admits any signed-in account, as before', async () => {
  const env = await hosting(environment());
  const r = await admit(env, await ticketFor(env, 'player'));
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.account_id, PLAYER);
});

test('an unapproved listing admits its author, and refuses another signed-in account with the same answer as a bad ticket', async () => {
  const env = await hosting(environment(), { moderation: { status: 'pending', rating: 'all_ages' } });
  const own = await admit(env, await ticketFor(env, 'author'));
  assert.equal(own.status, 200, JSON.stringify(own.body));
  assert.equal(own.body.account_id, AUTHOR);

  const stranger = await admit(env, await ticketFor(env, 'player'));
  const forged = await admit(env, 'pw1.not-a-ticket.at-all');
  assert.equal(stranger.status, 403);
  assert.deepEqual(stranger.body, REFUSAL);
  assert.deepEqual([forged.status, forged.body], [stranger.status, REFUSAL], 'a bad ticket and a stranger get one answer');
});

test('a refused ticket is not spent: once the listing is approved, the same ticket is admitted', async () => {
  const env = await hosting(environment(), { moderation: { status: 'pending', rating: 'all_ages' } });
  const ticket = await ticketFor(env, 'player');
  assert.equal((await admit(env, ticket)).status, 403);
  await setSim(env, { moderation: { status: 'approved', rating: 'all_ages' } });
  const r = await admit(env, ticket);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.account_id, PLAYER);
});

test('a listing quarantined while it is being hosted refuses new joiners, and the author is still admitted', async () => {
  const env = await hosting(environment());
  assert.equal((await admit(env, await ticketFor(env, 'player'))).status, 200, 'admitted before the quarantine');
  await setSim(env, { moderation: { status: 'quarantined', rating: 'all_ages' } });
  const later = await admit(env, await ticketFor(env, 'other'));
  assert.equal(later.status, 403);
  assert.deepEqual(later.body, REFUSAL);
  assert.equal((await admit(env, await ticketFor(env, 'author'))).status, 200, 'the author still joins');
});

test('a legal hold placed while the listing is hosted refuses new joiners, and the author is still admitted', async () => {
  const env = await hosting(environment());
  await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: 'content_lock' }), { customMetadata: { kind: 'content_lock' } });
  const r = await admit(env, await ticketFor(env, 'player'));
  assert.equal(r.status, 403);
  assert.deepEqual(r.body, REFUSAL);
  assert.equal((await admit(env, await ticketFor(env, 'author'))).status, 200);
});

test('an approved private listing admits any signed-in ticket holder, as its download does: privacy is not a moderation state', async () => {
  const env = await hosting(environment(), { is_public: false });
  const r = await admit(env, await ticketFor(env, 'player'));
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.account_id, PLAYER);
});

test('an approval whose binding is voided admits its author only, public or private, as the download refuses it', async () => {
  // The stored objects are no longer the ones that were reviewed: the same answer the download gives (simForServing).
  for (const is_public of [true, false]) {
    const env = await hosting(environment(), { is_public });
    await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, revoked: true }));
    assert.deepEqual((await admit(env, await ticketFor(env, 'player'))).body, REFUSAL, `is_public ${is_public}`);
    assert.equal((await admit(env, await ticketFor(env, 'author'))).status, 200, `the author, is_public ${is_public}`);
  }
});
