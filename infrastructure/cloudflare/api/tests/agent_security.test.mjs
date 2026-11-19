// SI (agent) account hardening: security review 2026-11-15 (si-agent-security-review.md), F2 to F7. What this file pins:
//   F2  every money and credential handler refuses an agent's session itself: each is sent one with the gate taken out of
//       the Worker, and answers 403 agent_not_allowed, while a person's session gets past that check; /api/kyc/link, which
//       the gate used to leave open, refuses an agent with a clean token through the whole Worker;
//   F3  an agent's session on a route the gate leaves open (public, or its own open list) is refused, except signing in
//       and out and registering; signed out, those routes are anyone's;
//   F4  a suspended or revoked agent cannot sign in, nor one whose sponsor is missing, banned or under a value freeze; a
//       revoke retires the key; the standing holds even when KV still serves the old record; a staff suspension and a
//       sponsor's ban end the agent's sessions;
//   F5  a new name is unique by case fold, for people and agents, claimed atomically; an agent's name may not read as an
//       existing one (0/o, 1/l, 3/e, 5/s, case, underscores); the staff backfill files older names;
//   F6  5 publishes and 10 submits a day per agent, 20 publishes a day per sponsor across its agents: 429 agent_daily_limit;
//   F7  a sponsor's value freeze suspends its agents and stops it inviting.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';
import { handleAgentRoute, agreementView, openAgentCase, confusableFold, nameFold } from '../src/agents.mjs';
import { AGENT_AGREEMENT } from '../src/agent_agreement.mjs';

const worker = await loadWorker();
// The same Worker with kycGate taken out, so a request reaches the route's own checks (F2: the gate is never alone).
const GATE = 'export async function kycGate(request, env, cors, deps) {\n';
const gateless = await loadWorker({ patch: [['kyc_identity.mjs', GATE, `${GATE}  return null;\n`]] });

const SPONSOR = '11111111-1111-4111-8111-111111111111';
const PERSON = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const AGENT = '44444444-4444-4444-8444-444444444444';
const SIM = '00000000-0000-4000-8000-000000000001';
const API = 'https://api.eustress.dev';

const json = (v, status = 200, headers = {}) => new Response(JSON.stringify(v), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const hex = (buf) => Buffer.from(buf).toString('hex');
function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const raw = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
  return { publicHex: hex(raw), sign: (text) => hex(crypto.sign(null, Buffer.from(text), privateKey)) };
}

async function env0(over = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), PAYOUTS: fakeKv(), KYC_STATUS: fakeKv(), AGENT_SIGNUP: 'on', STRIPE_SECRET_KEY: 'sk_test_not_real', ...over };
  env.sent = [];
  env.EMAIL = { send: async (m) => { env.sent.push(m); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify({ id: SPONSOR, username: 'Simbuilder', account_type: 'human', kyc_verified: true, age_verified: true }));
  await env.USERS.put('username:Simbuilder', SPONSOR);
  await env.USERS.put(`user:${PERSON}`, JSON.stringify({ id: PERSON, username: 'ada', account_type: 'human', kyc_verified: true, age_verified: true }));
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'staff1', account_type: 'human', kyc_verified: true, age_verified: true, role: 'admin' }));
  return env;
}
async function seededAgent(env, id = AGENT, over = {}) {
  await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: `Bot${id.slice(0, 4)}`, account_type: 'agent', sponsor_id: SPONSOR, kyc_verified: false, age_verified: false,
    agent: { provider: 'anthropic', model: 'claude-opus-5-5', agreement: { version: AGENT_AGREEMENT.version }, suspended: null, revoked_at: null }, ...over }));
  const list = JSON.parse((await env.USERS.get(`agents:${SPONSOR}`)) || '[]');
  await env.USERS.put(`agents:${SPONSOR}`, JSON.stringify([...new Set([...list, id])]));
  return id;
}
async function send(w, env, method, path, { token, body, headers = {} } = {}) {
  const h = { 'Content-Type': 'application/json', ...headers };
  if (token) h.Authorization = `Bearer ${token}`;
  const res = await w.fetch(new Request(`${API}${path}`, { method, headers: h, body: method === 'GET' ? undefined : JSON.stringify(body ?? {}) }), env, ctx);
  let parsed = null;
  try { parsed = await res.json(); } catch { /* not JSON */ }
  return { status: res.status, body: parsed };
}
/// Outbound fetches (Stripe and the like) are recorded and refused: a refused agent must never reach one.
async function noOutbound(fn) {
  const real = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url) => { calls.push(String(url)); return new Response(JSON.stringify({ error: { message: 'test: no network' } }), { status: 500 }); };
  try { return { result: await fn(), calls }; } finally { globalThis.fetch = real; }
}

// The agent-route harness (src/agents.mjs handleAgentRoute), as tests/agents.test.mjs drives it.
const deps = (over = {}) => ({
  json,
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace('Bearer ', '') || null,
  requireAdmin: async (req) => ((req.headers.get('Authorization') || '').replace('Bearer ', '') === ADMIN ? ADMIN : null),
  endSessions: async (env, id) => { env.ended = [...(env.ended || []), id]; },
  auditLog: async () => {},
  notify: async () => {},
  ...over,
});
async function route(env, method, path, { as, body, d = deps() } = {}) {
  const headers = {};
  if (as) headers.Authorization = `Bearer ${as}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const req = new Request(`${API}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const res = await handleAgentRoute(req, new URL(req.url), env, {}, d);
  return res ? { status: res.status, body: await res.json() } : null;
}
async function invite(env, name) {
  const view = await agreementView();
  return route(env, 'POST', '/api/agents/invites', { as: SPONSOR, body: { agree: true, agreement_sha256: view.sha256, provider: 'anthropic', name } });
}
async function register(env, token, key, username) {
  const view = await agreementView();
  const inviteHash = crypto.createHash('sha256').update(token).digest('hex');
  const fields = { username, public_key: key.publicHex, provider: 'anthropic', model: 'claude-opus-5-5' };
  const signature = key.sign(`eustress-agent-register:v1:${username}:${key.publicHex}:anthropic:claude-opus-5-5:${inviteHash}:${view.sha256}`);
  return route(env, 'POST', '/api/agents/register', { body: { invite: token, ...fields, signature } });
}
async function registeredAgent(env, name = 'ClaudeBot9000') {
  const key = keyPair();
  const made = await invite(env, name);
  assert.equal(made.status, 201, JSON.stringify(made.body));
  const reg = await register(env, made.body.invite, key, name);
  assert.equal(reg.status, 201, JSON.stringify(reg.body));
  return { id: reg.body.id, key };
}
async function signIn(env, key) {
  const ch = await send(worker, env, 'POST', '/api/auth/challenge', { body: { public_key: key.publicHex } });
  if (!ch.body?.challenge) return ch;
  const signature = key.sign(`eustress-login-v1\n${API}\n${ch.body.challenge}`);
  return send(worker, env, 'POST', '/api/auth/verify-challenge', { body: { public_key: key.publicHex, challenge: ch.body.challenge, signature, format: 'eustress-login-v1' } });
}

// ── F2: each money and credential handler refuses an agent by itself ────────

const MONEY_AND_CREDENTIAL_ROUTES = [
  ['POST', '/api/cosign', { contribution_type: 'build', contribution_hash: 'a'.repeat(16) }],
  ['POST', '/api/stripe/checkout', { tier: 'seed', mode: 'one_time' }],
  ['POST', '/api/tickets/checkout', { package: 'small' }],
  ['POST', '/api/delta/checkout', { pack: 'distance' }],
  ['POST', '/api/delta/charge', { tool: 'x', amount: 1 }],
  ['POST', '/api/sponsor/checkout', { tier: 'seed' }],
  ['POST', '/api/subscriptions/checkout', { plan: 'creator' }],
  ['POST', '/api/ledger/spend', { amount: 1, purpose: 'test' }],
  ['POST', '/api/keys', { name: 'k' }],
  ['POST', '/api/commerce/products', { name: 'p', price: 1 }],
  ['POST', '/api/commerce/purchases', { product_id: 'p' }],
  ['POST', '/api/stripe/connect/onboard', {}],
  ['POST', '/api/kyc/link', { kyc_session_id: 'x', kyc_claim_secret: 'y' }],
  ['POST', '/api/passkeys/register/begin', {}],
  ['POST', '/api/account/email', { email: 'agent@example.com' }],
  ['POST', '/api/auth/discord/begin', {}],
  ['POST', '/api/auth/slack/begin', {}],
  ['POST', `/api/simulations/${SIM}/live`, {}],
  ['POST', `/api/simulations/${SIM}/server-credential`, {}],
];

for (const [method, path, body] of MONEY_AND_CREDENTIAL_ROUTES) {
  test(`F2: ${method} ${path} refuses an agent's session in its own handler, with the gate taken out`, async () => {
    const env = await env0();
    await seededAgent(env);
    const { result, calls } = await noOutbound(() => send(gateless, env, method, path, { token: mintToken(AGENT), body }));
    assert.deepEqual([result.status, result.body?.code], [403, 'agent_not_allowed'], JSON.stringify(result.body));
    assert.deepEqual(calls, [], 'nothing left the Worker');
    // A person's session is not stopped by that check (the route answers it for its own reasons).
    const person = await noOutbound(() => send(gateless, env, method, path, { token: mintToken(PERSON), body }));
    assert.notEqual(person.result.body?.code, 'agent_not_allowed');
  });
}

test('F2: /api/kyc/link refuses an agent with a clean token through the whole Worker (the gate used to leave it open)', async () => {
  const env = await env0();
  await seededAgent(env);
  const r = await send(worker, env, 'POST', '/api/kyc/link', { token: mintToken(AGENT), body: { kyc_session_id: 'x', kyc_claim_secret: 'y' } });
  assert.deepEqual([r.status, r.body?.code], [403, 'agent_not_allowed']);
  const rec = JSON.parse(await env.USERS.get(`user:${AGENT}`));
  assert.deepEqual([rec.kyc_verified, rec.age_verified], [false, false]);
});

// ── F3: open routes ─────────────────────────────────────────────────────────

test('F3: an agent\'s session on a public route is refused, except signing in and out and registering; signed out, it is anyone\'s', async () => {
  const env = await env0();
  await seededAgent(env);
  const token = mintToken(AGENT);
  for (const [path, body] of [
    ['/api/node/heartbeat', { node_id: 'n1', mode: 'full', uptime_secs: 60 }],
    ['/api/takedown/requests', { url: 'https://eustress.dev/sim/x' }],
    ['/api/telemetry/comment', { comment: 'hi' }],
    ['/api/account/email/cancel', { token: 'x' }],
  ]) {
    const r = await send(worker, env, 'POST', path, { token, body });
    assert.deepEqual([r.status, r.body?.code], [403, 'agent_not_allowed'], path);
    const signedOut = await send(worker, env, 'POST', path, { body });
    assert.notEqual(signedOut.body?.code, 'agent_not_allowed', `${path} signed out`);
    const person = await send(worker, env, 'POST', path, { token: mintToken(PERSON), body });
    assert.notEqual(person.body?.code, 'agent_not_allowed', `${path} as a person`);
  }
  assert.equal(JSON.parse((await env.SOCIAL.get(`presence:${new Date().toISOString().slice(0, 10)}:${AGENT}`)) || 'null'), null, 'no presence was credited to the agent');
  for (const path of ['/api/auth/logout', '/api/auth/challenge', '/api/agents/register']) {
    const r = await send(worker, env, 'POST', path, { token, body: {} });
    assert.notEqual(r.body?.code, 'agent_not_allowed', path);
  }
});

// ── F4: sign-in and sessions ────────────────────────────────────────────────

test('F4: an agent signs in only while it stands: not suspended, not revoked, its sponsor present, not banned, not frozen', async () => {
  const env = await env0();
  const { id, key } = await registeredAgent(env);
  assert.equal((await signIn(env, key)).status, 200, 'standing: signs in');

  assert.equal((await route(env, 'POST', `/api/agents/${id}/suspend`, { as: SPONSOR })).status, 200);
  assert.deepEqual([(await signIn(env, key)).status, (await signIn(env, key)).body?.code], [403, 'agent_suspended'], 'suspended by its sponsor');
  await route(env, 'POST', `/api/agents/${id}/unsuspend`, { as: SPONSOR });
  assert.equal((await signIn(env, key)).status, 200, 'resumed');

  const sponsor = JSON.parse(await env.USERS.get(`user:${SPONSOR}`));
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify({ ...sponsor, banned: true }));
  assert.equal((await signIn(env, key)).body?.code, 'agent_suspended', 'sponsor banned');
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify(sponsor));
  await env.USERS.put(`valuefreeze:${SPONSOR}`, String(Date.now() + 86400e3));
  assert.equal((await signIn(env, key)).body?.code, 'agent_suspended', 'sponsor under a value freeze (F7)');
  await env.USERS.delete(`valuefreeze:${SPONSOR}`);
  await env.USERS.delete(`user:${SPONSOR}`);
  assert.equal((await signIn(env, key)).body?.code, 'agent_suspended', 'sponsor gone');
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify(sponsor));
  assert.equal((await signIn(env, key)).status, 200);
});

test('F4: a revoke holds at once, even where KV still serves the old record; the key is retired for good', async () => {
  const env = await env0();
  const { id, key } = await registeredAgent(env);
  const before = await env.USERS.get(`user:${id}`);
  assert.equal((await route(env, 'POST', `/api/agents/${id}/revoke`, { as: SPONSOR })).status, 200);
  assert.equal(await env.USERS.get(`retired-key:${key.publicHex}`), id, 'the key is retired');
  // An edge that has not seen the revoke yet: the old record and the old key index.
  await env.USERS.put(`user:${id}`, before);
  await env.USERS.put(`pubkey:${key.publicHex}`, id);
  assert.deepEqual([(await signIn(env, key)).status, (await signIn(env, key)).body?.code], [403, 'agent_revoked'], 'sign-in reads the standing written after the revoke');
  const w = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(id), body: { name: 'x' } });
  assert.deepEqual([w.status, w.body?.code], [403, 'agent_revoked'], 'so does the gate');
  // The retired key never registers again, even with a fresh invite.
  const again = await invite(env, 'OtherBot');
  assert.equal((await register(env, again.body.invite, key, 'OtherBot')).body?.code, 'key_taken');
});

test('F4: a staff suspension ends the agent\'s sessions; a sponsor\'s ban ends its agents\' sessions', async () => {
  const env = await env0();
  const id = await seededAgent(env);
  const c = await openAgentCase(env, id, { source: 'listing', status: 'quarantined', lane: 'legal' });
  await route(env, 'POST', `/api/admin/agents/cases/${c.id}/resolve`, { as: ADMIN, body: { decision: 'confirmed', category: 'harmful_content', rationale: 'Confirmed in the legal lane by a person.' } });
  assert.ok((env.ended || []).includes(id), 'a staff suspension ended its sessions');

  const env2 = await env0();
  const a2 = await seededAgent(env2);
  await env2.USERS.put('username:Simbuilder', SPONSOR);
  const ban = await send(worker, env2, 'POST', '/api/admin/ban', { token: mintToken(ADMIN), body: { username: 'Simbuilder', reason: 'test' } });
  assert.equal(ban.status, 200, JSON.stringify(ban.body));
  assert.ok(Number(await env2.USERS.get(`authfloor:${a2}`)) > 0, 'the agent\'s session floor moved with its sponsor\'s ban');
});

// ── F5: names ───────────────────────────────────────────────────────────────

test('F5: a new name is unique by case fold, for agents and people; an agent\'s name may not read as an existing one', async () => {
  const env = await env0();
  await registeredAgent(env, 'ClaudeBot9000');
  assert.equal(await env.USERS.get(`usernamefold:${nameFold('ClaudeBot9000')}`) !== null, true, 'the fold is filed');
  for (const name of ['CLAUDEBOT9000', 'claudebot9000', 'ClaudeBOT9000']) {
    assert.equal((await invite(env, name)).body?.code, 'name_taken', name);
  }
  assert.equal(confusableFold('C1aude_Bot9000'), confusableFold('ClaudeBot9000'));
  for (const name of ['C1audeBot9000', 'Claude_Bot9000', 'ClaudeBot9OOO']) {
    assert.equal((await invite(env, name)).body?.code, 'name_too_close', name);
  }
  assert.equal((await invite(env, 'ClaudeBot9001')).status, 201, 'a different name is fine');
  // A person's new name: a case variant of a name filed by fold is taken, before any identity check runs.
  await env.USERS.put('usernamefold:ada_l', PERSON);
  const r = await send(worker, env, 'POST', '/api/auth/register', { body: { username: 'ADA_L', public_key: 'ab'.repeat(32) } });
  assert.equal(r.status, 409, JSON.stringify(r.body));
});

/// Holds every read and write under `prefix` for a few ms, so requests sent together really interleave there (the fakes
/// otherwise answer at once, and one request can finish before the next one starts checking).
function slowPrefix(store, prefix, methods) {
  for (const m of methods) {
    const inner = store[m].bind(store);
    store[m] = async (key, ...rest) => {
      if (String(key).startsWith(prefix)) await new Promise((r) => setTimeout(r, 5));
      return inner(key, ...rest);
    };
  }
}

test('F5: a name another sign-up has claimed, but not yet written, is taken', async () => {
  const env = await env0();
  const made = await invite(env, 'PendingBot');
  // Another registration holds the fold's claim and has not written its KV records yet.
  await env.SCENES.put('usernames/fold/pendingbot', JSON.stringify({ account_id: 'someone-else' }));
  const r = await register(env, made.body.invite, keyPair(), 'PendingBot');
  assert.deepEqual([r.status, r.body?.code], [409, 'name_taken']);
  await env.SCENES.delete('usernames/fold/pendingbot');
  assert.equal((await register(env, made.body.invite, keyPair(), 'PendingBot')).status, 201, 'the invite was not spent by losing the name');
});

test('F5: two sign-ups racing for names that differ only in case: exactly one gets it', async () => {
  const env = await env0();
  const a = await invite(env, 'RaceBot');
  const b = await invite(env, 'RACEBOT');
  slowPrefix(env.USERS, 'usernamefold:', ['get', 'put']);
  const results = await Promise.all([register(env, a.body.invite, keyPair(), 'RaceBot'), register(env, b.body.invite, keyPair(), 'RACEBOT')]);
  assert.deepEqual(results.map((r) => r.status).sort(), [201, 409], JSON.stringify(results.map((r) => r.body)));
  const lost = results.find((r) => r.status === 409);
  assert.equal(lost.body.code, 'name_taken');
});

test('F5: the staff backfill files older names, counts first, and never overwrites another account\'s fold', async () => {
  const env = await env0();
  await env.USERS.put('username:OldBot', PERSON);
  await env.USERS.put('username:Bob', SPONSOR);
  await env.USERS.put('username:bob', ADMIN); // two older accounts that differ only in case
  assert.equal((await route(env, 'POST', '/api/admin/agents/name-folds', { as: SPONSOR, body: {} })).status, 403, 'staff only');
  const counted = await route(env, 'POST', '/api/admin/agents/name-folds', { as: ADMIN, body: {} });
  assert.equal(counted.body.applied, false);
  assert.equal(await env.USERS.get('usernamefold:oldbot'), null, 'counting writes nothing');
  const applied = await route(env, 'POST', '/api/admin/agents/name-folds', { as: ADMIN, body: { apply: true } });
  assert.equal(applied.body.done, true);
  assert.equal(applied.body.collisions.length, 1, 'Bob and bob share a fold: reported, not overwritten');
  assert.equal(await env.USERS.get('usernamefold:oldbot'), PERSON);
  assert.equal((await invite(env, 'OLDBOT')).body?.code, 'name_taken', 'a case variant of an older name is taken once filed');
});

// ── F6: daily limits ────────────────────────────────────────────────────────

test('F6: 5 publishes and 10 submits a day per agent; the sixth publish and eleventh submit answer 429 agent_daily_limit', async () => {
  const env = await env0();
  await seededAgent(env);
  const token = mintToken(AGENT);
  for (let n = 1; n <= 5; n++) {
    const r = await send(worker, env, 'POST', '/api/simulations/publish', { token, body: { name: `s${n}` } });
    assert.notEqual(r.body?.code, 'agent_daily_limit', `publish ${n}`);
  }
  const sixth = await send(worker, env, 'POST', '/api/simulations/publish', { token, body: { name: 's6' } });
  assert.deepEqual([sixth.status, sixth.body?.code, sixth.body?.limit], [429, 'agent_daily_limit', 5]);
  assert.match(sixth.body.error, /5 publishes for today/);
  const multipart = await send(worker, env, 'POST', `/api/simulations/${SIM}/space/multipart/create`, { token, body: {} });
  assert.equal(multipart.body?.code, 'agent_daily_limit', 'a multipart upload counts as a publish');
  for (let n = 1; n <= 10; n++) {
    const r = await send(worker, env, 'POST', `/api/simulations/${SIM}/submit`, { token, body: {} });
    assert.notEqual(r.body?.code, 'agent_daily_limit', `submit ${n}`);
  }
  assert.equal((await send(worker, env, 'POST', `/api/simulations/${SIM}/submit`, { token, body: {} })).status, 429);
  const person = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(PERSON), body: { name: 'p' } });
  assert.notEqual(person.body?.code, 'agent_daily_limit', 'people are not limited by this');
});

test('F6: a sponsor\'s agents share 20 publishes a day', async () => {
  const env = await env0();
  const ids = [];
  for (let n = 0; n < 5; n++) ids.push(await seededAgent(env, `4444444${n}-4444-4444-8444-444444444444`));
  for (const id of ids.slice(0, 4)) {
    for (let n = 1; n <= 5; n++) {
      const r = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(id), body: { name: `s${n}` } });
      assert.notEqual(r.body?.code, 'agent_daily_limit', `${id} publish ${n}`);
    }
  }
  const fifth = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(ids[4]), body: { name: 'x' } });
  assert.deepEqual([fifth.status, fifth.body?.code, fifth.body?.kind, fifth.body?.limit], [429, 'agent_daily_limit', 'sponsor_publish', 20]);
});

test('F6: parallel publishes never take more than 5 slots', async () => {
  const env = await env0();
  await seededAgent(env);
  const token = mintToken(AGENT);
  slowPrefix(env.SCENES, 'agents/limits/', ['head', 'get', 'put']);
  const results = await Promise.all(Array.from({ length: 12 }, (_, n) => send(worker, env, 'POST', '/api/simulations/publish', { token, body: { name: `p${n}` } })));
  assert.equal(results.filter((r) => r.body?.code !== 'agent_daily_limit').length, 5);
});

// ── F7: the sponsor's value freeze ──────────────────────────────────────────

test('F7: a sponsor under a value freeze cannot invite, and its agents are suspended until it ends', async () => {
  const env = await env0();
  await seededAgent(env);
  await env.USERS.put(`valuefreeze:${SPONSOR}`, String(Date.now() + 86400e3));
  assert.equal((await invite(env, 'FrozenBot')).body?.code, 'sponsor_not_allowed');
  const w = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(AGENT), body: { name: 'x' } });
  assert.deepEqual([w.status, w.body?.code], [403, 'agent_suspended']);
  await env.USERS.put(`valuefreeze:${SPONSOR}`, String(Date.now() - 1000));
  assert.equal((await invite(env, 'ThawedBot')).status, 201);
  const ok = await send(worker, env, 'POST', '/api/simulations/publish', { token: mintToken(AGENT), body: { name: 'x' } });
  assert.notEqual(ok.body?.code, 'agent_suspended');
});
