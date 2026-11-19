// Agent accounts (src/agents.mjs, docs/architecture/AGENT_ACCOUNTS.md). What this file pins:
//   - sign-up is closed unless AGENT_SIGNUP is "on" AND the agreement is counsel's final text;
//   - only a verified adult human sponsors, only after accepting the exact agreement text, only a Claude agent, at most 5;
//   - the agent registers itself with its own key, its signature binding name, key, model, invite and agreement; one
//     registration per invite; it then signs in with the ordinary key challenge;
//   - an agent writes only what the allowlist names: never money, admin, passkeys, email, keys or sponsoring;
//   - an agent is never an administrator; revoking ends its sessions and sign-in; a banned sponsor stops its agents;
//   - an agent's harm or legal rejection opens a case; a person confirms it; a confirmed case outside the legal lane gets a
//     report to feedback@anthropic.com from bots@eustress.dev, sent only when a person sends it, naming no sponsor;
//     three confirmed cases (or one legal) suspend the agent.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';
import {
  handleAgentRoute, agentMayWrite, agentNameProblem, humanNameProblem, signupAvailable, registrationMessage, agreementView, openAgentCase,
  renderAgentReport, AGENT_PROVIDERS, AGENT_REPORT_FROM, AGENTS_PER_SPONSOR,
} from '../src/agents.mjs';
import { AGENT_AGREEMENT } from '../src/agent_agreement.mjs';
import { isAdmin } from '../src/admin_check.mjs';
import { routePolicy, PUBLIC, FRESH } from '../src/route_policy.mjs';

const worker = await loadWorker();
const SPONSOR = '11111111-1111-4111-8111-111111111111';
const HUMAN = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const FINAL = Object.freeze({ ...AGENT_AGREEMENT, version: 'test-1', final: true });

const json = (v, status = 200, headers = {}) => new Response(JSON.stringify(v), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const hex = (buf) => Buffer.from(buf).toString('hex');

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const raw = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
  return { publicHex: hex(raw), sign: (text) => hex(crypto.sign(null, Buffer.from(text), privateKey)) };
}

async function env0(over = {}) {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), PAYOUTS: fakeKv(), AGENT_SIGNUP: 'on', ...over };
  env.sent = [];
  env.EMAIL = { send: async (m) => { env.sent.push(m); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify({ id: SPONSOR, username: 'Simbuilder', account_type: 'human', kyc_verified: true, age_verified: true }));
  await env.USERS.put(`user:${HUMAN}`, JSON.stringify({ id: HUMAN, username: 'ada', account_type: 'human', kyc_verified: true, age_verified: true }));
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'staff1', account_type: 'human', kyc_verified: true, age_verified: true, role: 'admin' }));
  return env;
}
const audits = [];
const deps = (over = {}) => ({
  json,
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace('Bearer ', '') || null,
  requireAdmin: async (req) => ((req.headers.get('Authorization') || '').replace('Bearer ', '') === ADMIN ? ADMIN : null),
  endSessions: async (env, id) => { env.ended = [...(env.ended || []), id]; },
  auditLog: async (_env, action, actor, target) => { audits.push({ action, actor, target }); },
  notify: async (env, id, item) => { env.notices = [...(env.notices || []), { id, item }]; },
  agreement: FINAL,
  ...over,
});
async function route(env, method, path, { as, body, d = deps() } = {}) {
  const headers = {};
  if (as) headers.Authorization = `Bearer ${as}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const req = new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const res = await handleAgentRoute(req, new URL(req.url), env, {}, d);
  return res ? { status: res.status, body: await res.json() } : null;
}
async function invite(env, over = {}) {
  const view = await agreementView(FINAL);
  return route(env, 'POST', '/api/agents/invites', { as: SPONSOR, body: { agree: true, agreement_sha256: view.sha256, provider: 'anthropic', name: 'ClaudeBot9000', ...over } });
}
async function registerWith(env, token, key, over = {}) {
  const view = await agreementView(FINAL);
  const inviteHash = crypto.createHash('sha256').update(token).digest('hex');
  const fields = { username: 'ClaudeBot9000', public_key: key.publicHex, provider: 'anthropic', model: 'claude-opus-5-5', ...over };
  const signature = over.signature || key.sign(registrationMessage({ ...fields, inviteHash, agreementSha: view.sha256 }));
  return route(env, 'POST', '/api/agents/register', { body: { invite: token, ...fields, signature } });
}

// ── rules ────────────────────────────────────────────────────────────────────

test('sign-up opens only with AGENT_SIGNUP "on" and the agreement final; the agreement in the tree is in force', async () => {
  assert.equal(AGENT_AGREEMENT.final, true, 'McKale 2026-11-15: the agreement is in force');
  assert.ok(!/NOT IN FORCE/.test(AGENT_AGREEMENT.text));
  const DRAFT = Object.freeze({ ...AGENT_AGREEMENT, version: 'test-draft', final: false });
  assert.equal(signupAvailable({ AGENT_SIGNUP: 'on' }, DRAFT), false);
  assert.equal(signupAvailable({ AGENT_SIGNUP: 'on' }), true);
  assert.equal(signupAvailable({ AGENT_SIGNUP: 'on' }, FINAL), true);
  assert.equal(signupAvailable({ AGENT_SIGNUP: 'yes' }, FINAL), false);
  const env = await env0();
  assert.equal((await route(env, 'POST', '/api/agents/invites', { as: SPONSOR, body: {}, d: deps({ agreement: DRAFT }) })).status, 503);
  assert.equal((await route(await env0({ AGENT_SIGNUP: 'off' }), 'POST', '/api/agents/invites', { as: SPONSOR, body: {} })).status, 503);
  const shown = await route(env, 'GET', '/api/agents/agreement');
  assert.deepEqual([shown.body.available, shown.body.providers.map((p) => p.id), shown.body.limit], [true, ['anthropic'], AGENTS_PER_SPONSOR]);
});

test('the allowlist: an agent builds, publishes and plays, and never touches money, staff, keys, email or other agents', () => {
  for (const [m, p] of [['POST', '/api/simulations/publish'], ['PATCH', '/api/simulations/0a/listing'], ['POST', '/api/simulations/0a/submit'],
    ['POST', '/api/simulations/0a/appeal'], ['POST', '/api/simulations/0a/world/commit'], ['POST', '/api/auth/logout'], ['POST', '/api/simulations/0a/play-ticket']]) {
    assert.equal(agentMayWrite(m, p), true, `${m} ${p}`);
  }
  for (const [m, p] of [['POST', '/api/commerce/products'], ['POST', '/api/payouts/request'], ['POST', '/api/subscriptions'], ['POST', '/api/keys'],
    ['POST', '/api/admin/moderation/tool'], ['POST', '/api/agents/invites'], ['POST', '/api/passkeys/register'], ['POST', '/api/account/email/change'],
    ['POST', '/api/identity/rotate/begin'], ['POST', '/api/social/follow'], ['DELETE', '/api/simulations/0a/listing'], ['POST', '/api/delta/buy']]) {
    assert.equal(agentMayWrite(m, p), false, `${m} ${p}`);
  }
  assert.equal(agentNameProblem('ClaudeBot9000'), null);
  for (const bad of ['EustressBot', 'OfficialClaude', 'admin_bot', 'xx', 'has space', 'a'.repeat(33)]) assert.ok(agentNameProblem(bad), bad);
  // McKale 2026-11-15: an agent's name says it is one, and a person's never holds "bot".
  for (const ok of ['ClaudeBot9000', 'helper_agent', 'AGENTk', 'botanist']) assert.equal(agentNameProblem(ok), null, ok);
  for (const bad of ['Claude9000', 'Helper', 'Ag3nt']) assert.match(agentNameProblem(bad), /"bot" or "agent"/, bad);
  for (const bad of ['RoboBot', 'robot', 'BOTANY', 'my_bot']) assert.ok(humanNameProblem(bad), bad);
  for (const ok of ['ada', 'Simbuilder', 'agent_smith']) assert.equal(humanNameProblem(ok), null, ok);
  assert.equal(routePolicy('POST', '/api/agents/register').class, PUBLIC);
  for (const p of ['/api/agents/invites', '/api/agents/0abc/revoke']) assert.equal(routePolicy('POST', p).class, FRESH, p);
});

// ── sign-up ──────────────────────────────────────────────────────────────────

test('ClaudeBot9000 end to end: invite, self-registration, key sign-in, one use per invite', async () => {
  const env = await env0();
  const made = await invite(env);
  assert.equal(made.status, 201, JSON.stringify(made.body));
  // The invite says what it is, and comes with what an agent that never saw Eustress needs to use it.
  assert.match(made.body.invite, /^eustress-agent-invite-[0-9a-f]{64}$/);
  assert.equal(made.body.name, 'ClaudeBot9000');
  const hash = crypto.createHash('sha256').update(made.body.invite).digest('hex');
  for (const part of ['ClaudeBot9000', made.body.invite, hash, made.body.agreement_sha256, 'https://api.eustress.dev/api/agents/register',
    `eustress-agent-register:v1:ClaudeBot9000:<public key>:anthropic:<model>:${hash}:${made.body.agreement_sha256}`, 'eustress-login-v1']) {
    assert.ok(made.body.instructions.includes(part), part);
  }
  const key = keyPair();
  const reg = await registerWith(env, made.body.invite, key);
  assert.equal(reg.status, 201, JSON.stringify(reg.body));
  const agent = JSON.parse(await env.USERS.get(`user:${reg.body.id}`));
  assert.deepEqual([agent.account_type, agent.sponsor_id, agent.agent.provider, agent.agent.model, agent.kyc_verified, agent.age_verified, agent.agent.agreement.version],
    ['agent', SPONSOR, 'anthropic', 'claude-opus-5-5', false, false, 'test-1']);
  assert.equal(agent.email, null);
  assert.equal(await env.USERS.get(`pubkey:${key.publicHex}`), reg.body.id);
  // The same invite again, even with another name and key: refused.
  const again = await registerWith(env, made.body.invite, keyPair(), { username: 'ClaudeBot9001' });
  assert.equal(again.status, 403);
  // Sign in with the ordinary key challenge, through the Worker.
  const ch = await worker.fetch(new Request('https://api.eustress.dev/api/auth/challenge', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ public_key: key.publicHex }) }), env, ctx);
  const { challenge } = await ch.json();
  const signature = key.sign(`eustress-login-v1\nhttps://api.eustress.dev\n${challenge}`);
  const signIn = await worker.fetch(new Request('https://api.eustress.dev/api/auth/verify-challenge', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ public_key: key.publicHex, challenge, signature, format: 'eustress-login-v1' }) }), env, ctx);
  assert.equal(signIn.status, 200, await signIn.clone().text());
  assert.ok((await signIn.json()).token);
});

test('two registrations racing on one invite: exactly one account is made', async () => {
  const env = await env0();
  const token = (await invite(env, { name: 'RaceBot' })).body.invite;
  const results = await Promise.all([
    registerWith(env, token, keyPair(), { username: 'RaceBot' }),
    registerWith(env, token, keyPair(), { username: 'RaceBot' }),
  ]);
  assert.deepEqual(results.map((r) => r.status === 201).sort(), [false, true]);
  assert.equal(JSON.parse(await env.USERS.get(`agents:${SPONSOR}`)).length, 1);
});

test('registration refuses a forged signature, a model swap, another provider, a reserved name, a taken name and a stale agreement', async () => {
  const env = await env0();
  const key = keyPair();
  const token = (await invite(env)).body.invite;
  // Signed for one model, sent with another: the signature binds the model.
  const view = await agreementView(FINAL);
  const inviteHash = crypto.createHash('sha256').update(token).digest('hex');
  const signedFor = key.sign(registrationMessage({ username: 'ClaudeBot9000', public_key: key.publicHex, provider: 'anthropic', model: 'claude-opus-5-5', inviteHash, agreementSha: view.sha256 }));
  assert.equal((await registerWith(env, token, key, { model: 'claude-haiku-5-5', signature: signedFor })).status, 401);
  assert.equal((await registerWith(env, token, key, { signature: keyPair().sign('x') })).status, 401, 'another key\'s signature');
  assert.equal((await registerWith(env, token, key, { provider: 'openai', model: 'gpt-9' })).status, 403);
  assert.equal((await registerWith(env, token, key, { username: 'EustressSupport' })).status, 400);
  assert.equal((await registerWith(env, token, key, { username: 'OtherBot' })).body.code, 'name_mismatch', 'the invite registers only the name it was made for');
  assert.equal((await registerWith(env, token.slice('eustress-agent-invite-'.length), key)).body.code, 'bad_invite', 'the bare hex is not an invite');
  await env.USERS.put('username:clairebot', HUMAN);
  assert.equal((await invite(env, { name: 'ClaireBot' })).status, 409, 'a name differing only in case is taken');
  assert.equal((await invite(env, { name: 'Claude9000' })).body.code, 'bad_name', 'no "bot" or "agent"');
  assert.equal((await invite(env, { name: undefined })).body.code, 'bad_name', 'the sponsor names the agent');
  await env.USERS.put('username:claudebot9000', HUMAN);
  assert.equal((await registerWith(env, token, key)).status, 409, 'taken after the invite was made');
  await env.USERS.delete('username:claudebot9000');
  // The agreement text changed after the invite: a new invite is needed.
  const changed = await route(env, 'POST', '/api/agents/register', { body: { invite: token, username: 'ClaudeBot9000', public_key: key.publicHex, provider: 'anthropic', model: 'claude-opus-5-5', signature: signedFor }, d: deps({ agreement: { ...FINAL, text: FINAL.text + ' amended' } }) });
  assert.equal(changed.status, 403);
  assert.equal((await registerWith(env, token, key)).status, 201, 'the invite was not spent by any refusal');
});

test('only a verified adult human sponsors, after accepting the exact text, a Claude agent, at most 5', async () => {
  const env = await env0();
  await env.USERS.put(`user:${HUMAN}`, JSON.stringify({ id: HUMAN, username: 'ada', account_type: 'human', kyc_verified: false }));
  assert.equal((await route(env, 'POST', '/api/agents/invites', { as: HUMAN, body: { agree: true, agreement_sha256: (await agreementView(FINAL)).sha256, provider: 'anthropic' } })).status, 403);
  assert.equal((await invite(env, { agree: false })).status, 400);
  assert.equal((await invite(env, { agreement_sha256: 'f'.repeat(64) })).status, 400, 'an older text is not the one accepted');
  assert.equal((await invite(env, { provider: 'openai' })).status, 403);
  assert.equal((await route(env, 'POST', '/api/agents/invites', { body: {} })).status, 401);
  for (let n = 0; n < AGENTS_PER_SPONSOR; n++) {
    const r = await registerWith(env, (await invite(env, { name: `ClaudeBot${n}` })).body.invite, keyPair(), { username: `ClaudeBot${n}` });
    assert.equal(r.status, 201, JSON.stringify(r.body));
  }
  assert.equal((await invite(env)).status, 409, 'the sixth is refused');
});

// ── what an agent may do, through the Worker's gate ─────────────────────────

async function seededAgent(env, over = {}) {
  const id = '44444444-4444-4444-8444-444444444444';
  await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: 'ClaudeBot9000', account_type: 'agent', sponsor_id: SPONSOR, kyc_verified: false, age_verified: false, agent: { provider: 'anthropic', model: 'claude-opus-5-5', agreement: { version: AGENT_AGREEMENT.version }, suspended: null, revoked_at: null }, ...over }));
  return id;
}
const call = async (env, method, path, token, body) => {
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  let parsed = null;
  try { parsed = await res.json(); } catch { /* not JSON */ }
  return { status: res.status, body: parsed };
};

test('the gate: an agent is refused every write off the allowlist, and every write while revoked, suspended or under a banned sponsor', async () => {
  const env = await env0();
  const id = await seededAgent(env);
  const token = mintToken(id);
  for (const [m, p] of [['POST', '/api/agents/invites'], ['POST', '/api/keys'], ['POST', '/api/commerce/products'], ['POST', '/api/admin/moderation/tool'], ['POST', '/api/social/follow']]) {
    const r = await call(env, m, p, token, {});
    assert.deepEqual([r.status, r.body?.code], [403, 'agent_not_allowed'], `${m} ${p}`);
  }
  const allowed = await call(env, 'POST', '/api/auth/logout', token, {});
  assert.notEqual(allowed.body?.code, 'agent_not_allowed');
  const listed = await call(env, 'PATCH', '/api/simulations/00000000-0000-4000-8000-000000000001/listing', mintToken(id), { name: 'x' });
  assert.notEqual(listed.body?.code, 'agent_not_allowed', 'an allowed route reaches its handler (which answers for itself)');
  await seededAgent(env, { agent: { agreement: { version: AGENT_AGREEMENT.version }, suspended: { by: 'staff' } } });
  assert.equal((await call(env, 'POST', '/api/simulations/publish', mintToken(id), {})).body?.code, 'agent_suspended');
  await seededAgent(env, { agent: { agreement: { version: 'old' } } });
  assert.equal((await call(env, 'POST', '/api/simulations/publish', mintToken(id), {})).body?.code, 'agent_suspended', 'a new agreement suspends until accepted');
  await seededAgent(env);
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify({ id: SPONSOR, account_type: 'human', kyc_verified: true, age_verified: true, banned: true }));
  assert.equal((await call(env, 'POST', '/api/simulations/publish', mintToken(id), {})).body?.code, 'agent_suspended');
});

test('a person cannot sign up with "bot" in the name', async () => {
  const env = await env0();
  for (const username of ['RoboBot', 'robot_fan', 'xBOTx']) {
    const res = await worker.fetch(new Request('https://api.eustress.dev/api/auth/register', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username, public_key: 'ab'.repeat(32) }) }), env, ctx);
    const body = await res.json();
    assert.deepEqual([res.status, body.code], [400, 'name_reserved_for_agents'], username);
  }
  const fine = await worker.fetch(new Request('https://api.eustress.dev/api/auth/register', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'ada_lovelace', public_key: 'ab'.repeat(32) }) }), env, ctx);
  assert.notEqual((await fine.clone().json().catch(() => ({}))).code, 'name_reserved_for_agents');
});

test('an agent is never an administrator, whatever is written for it', async () => {
  const env = await env0();
  const id = await seededAgent(env, { role: 'admin' });
  await env.USERS.put(`admin:${id}`, '1');
  assert.equal(await isAdmin(env, id), false);
  assert.equal(await isAdmin(env, ADMIN), true, 'a human administrator still is one');
});

test('the sponsor lists, suspends and revokes; revoking ends sessions and sign-in; staff suspension is lifted by staff only', async () => {
  const env = await env0();
  const key = keyPair();
  const reg = await registerWith(env, (await invite(env)).body.invite, key);
  const id = reg.body.id;
  assert.equal((await route(env, 'GET', '/api/agents', { as: SPONSOR })).body.agents[0].username, 'ClaudeBot9000');
  assert.equal((await route(env, 'POST', `/api/agents/${id}/revoke`, { as: HUMAN })).status, 404, 'not your agent');
  assert.equal((await route(env, 'POST', `/api/agents/${id}/suspend`, { as: SPONSOR })).body.agent.suspended, true);
  assert.equal((await route(env, 'POST', `/api/agents/${id}/unsuspend`, { as: SPONSOR })).body.agent.suspended, false);
  const rec = JSON.parse(await env.USERS.get(`user:${id}`));
  rec.agent.suspended = { by: 'staff' };
  await env.USERS.put(`user:${id}`, JSON.stringify(rec));
  assert.equal((await route(env, 'POST', `/api/agents/${id}/unsuspend`, { as: SPONSOR })).status, 403);
  const revoked = await route(env, 'POST', `/api/agents/${id}/revoke`, { as: SPONSOR });
  assert.equal(revoked.body.agent.revoked, true);
  assert.ok(env.ended.includes(id), 'its sessions were ended');
  assert.equal(await env.USERS.get(`pubkey:${key.publicHex}`), null, 'its key no longer signs in');
});

// ── cases and the feedback report ──────────────────────────────────────────

test('a confirmed case gets a report to feedback@anthropic.com from bots@eustress.dev, sent by a person, naming no sponsor', async () => {
  const env = await env0();
  const id = await seededAgent(env);
  assert.equal(await openAgentCase(env, HUMAN, { source: 'listing', lane: 'harm' }), null, 'a person\'s rejection opens no agent case');
  const opened = await openAgentCase(env, id, { source: 'listing', ref: 'EU-1', lane: 'harm', status: 'rejected', reasons: ['B2'] });
  assert.ok(opened);
  const queue = await route(env, 'GET', '/api/admin/agents/cases', { as: ADMIN });
  assert.deepEqual(queue.body.cases.map((c) => c.id), [opened.id]);
  assert.equal((await route(env, 'GET', '/api/admin/agents/cases', { as: SPONSOR })).status, 403);
  const decided = await route(env, 'POST', `/api/admin/agents/cases/${opened.id}/resolve`, { as: ADMIN, body: { decision: 'confirmed', category: 'harmful_content', rationale: 'The listing taught a real-world harm.', summary: 'The agent published a Space whose text gave step-by-step instructions for a dangerous act.', action: 'the Space was not listed' } });
  assert.equal(decided.status, 200, JSON.stringify(decided.body));
  assert.equal(env.sent.length, 0, 'nothing is sent by deciding');
  assert.equal((await route(env, 'GET', '/api/admin/agents/cases', { as: ADMIN })).body.cases.length, 1, 'it waits to be sent');
  const sent = await route(env, 'POST', `/api/admin/agents/cases/${opened.id}/send-report`, { as: ADMIN });
  assert.equal(sent.status, 200, JSON.stringify(sent.body));
  assert.deepEqual([env.sent.length, env.sent[0].from, env.sent[0].to], [1, AGENT_REPORT_FROM, AGENT_PROVIDERS.anthropic.report_to]);
  assert.equal(AGENT_PROVIDERS.anthropic.report_to, 'feedback@anthropic.com');
  assert.equal(AGENT_REPORT_FROM, 'bots@eustress.dev');
  const report = renderAgentReport(JSON.parse(await env.USERS.get(`agentcase:${opened.id}`)), AGENT_PROVIDERS.anthropic);
  assert.ok(!report.text.includes(SPONSOR) && !report.text.includes('Simbuilder'), 'the report never names the sponsor (D7)');
  assert.match(report.text, /claude-opus-5-5/);
  assert.equal((await route(env, 'POST', `/api/admin/agents/cases/${opened.id}/send-report`, { as: ADMIN })).status, 409, 'once');
  assert.equal((await route(env, 'GET', '/api/admin/agents/cases', { as: ADMIN })).body.cases.length, 0);
  assert.equal(env.notices?.[0]?.id, SPONSOR, 'the sponsor is told the outcome');
  assert.equal(await env.USERS.get(`agentstrikes:${SPONSOR}`), '1', 'it counts toward the sponsor (D6)');
});

test('a legal-lane case never produces a report and suspends at once; three confirmed cases suspend', async () => {
  const env = await env0();
  const id = await seededAgent(env);
  const legal = await openAgentCase(env, id, { source: 'listing', status: 'quarantined', lane: 'legal' });
  const r = await route(env, 'POST', `/api/admin/agents/cases/${legal.id}/resolve`, { as: ADMIN, body: { decision: 'confirmed', category: 'harmful_content', rationale: 'Confirmed in the legal lane by a person.' } });
  assert.equal(r.body.case.report, null);
  assert.equal((await route(env, 'POST', `/api/admin/agents/cases/${legal.id}/send-report`, { as: ADMIN })).status, 409);
  assert.equal(JSON.parse(await env.USERS.get(`user:${id}`)).agent.suspended?.by, 'staff');
  assert.equal(env.notices, undefined, 'a legal-lane case is never announced to the sponsor');
  const env2 = await env0();
  const id2 = await seededAgent(env2);
  for (let n = 0; n < 3; n++) {
    const c = await openAgentCase(env2, id2, { source: 'report', note: 'x' });
    await route(env2, 'POST', `/api/admin/agents/cases/${c.id}/resolve`, { as: ADMIN, body: { decision: 'confirmed', category: 'spam', rationale: 'Posted the same thing repeatedly.', summary: 'The agent repeated the same message many times in chat.' } });
    assert.equal(!!JSON.parse(await env2.USERS.get(`user:${id2}`)).agent.suspended, n === 2, `after ${n + 1}`);
  }
});

test('a profile says an agent is one, and names its sponsor; a person\'s says human', async () => {
  const env = await env0();
  const id = await seededAgent(env);
  await env.USERS.put('username:ClaudeBot9000', id);
  await env.USERS.put(`user:${id}`, JSON.stringify({ ...JSON.parse(await env.USERS.get(`user:${id}`)), created_at: '2026-11-14T00:00:00.000Z' }));
  await env.USERS.put('username:Simbuilder', SPONSOR);
  const seen = async (name) => (await worker.fetch(new Request(`https://api.eustress.dev/api/community/users/${name}`), env, ctx)).json();
  const agent = await seen('ClaudeBot9000');
  assert.deepEqual([agent.account_type, agent.sponsored_by, agent.agent_model], ['agent', 'Simbuilder', 'claude-opus-5-5']);
  const person = await seen('Simbuilder');
  assert.equal(person.account_type, 'human');
  assert.equal('sponsored_by' in person, false);
});
