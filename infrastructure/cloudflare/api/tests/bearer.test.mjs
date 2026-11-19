// The one reading of the Authorization header (src/bearer.mjs). Security review 2026-11-15, F1: the gate and the routes
// read the token differently, so a session token split by a space or a tab was "no session" to kycGate (which let it
// through) and a valid session to the route (atob drops whitespace). An SI account or an unverified person then wrote
// routes the gate exists to refuse: money, API keys, passkeys, identity links.
//
// These tests send the split token through the whole Worker and expect it refused, and check each layer on its own:
// the reader, the gate's fail-closed answer, and the token verifier's strict alphabet.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import { bearerToken, presentsBearer } from '../src/bearer.mjs';

const worker = await loadWorker();
const AGENT = '44444444-4444-4444-8444-444444444444';
const SPONSOR = '11111111-1111-4111-8111-111111111111';
const PERSON = '55555555-5555-4555-8555-555555555555';

async function env0() {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), PAYOUTS: fakeKv(), AGENT_SIGNUP: 'on' };
  await env.USERS.put(`user:${SPONSOR}`, JSON.stringify({ id: SPONSOR, username: 'Simbuilder', account_type: 'human', kyc_verified: true, age_verified: true }));
  await env.USERS.put(`user:${AGENT}`, JSON.stringify({ id: AGENT, username: 'ClaudeBot9000', account_type: 'agent', sponsor_id: SPONSOR, kyc_verified: false, age_verified: false,
    agent: { provider: 'anthropic', model: 'claude-opus-5-5', agreement: { version: '1' }, suspended: null, revoked_at: null } }));
  // A person whose account predates identity verification: the gate refuses their writes too.
  await env.USERS.put(`user:${PERSON}`, JSON.stringify({ id: PERSON, username: 'early_person', account_type: 'human', kyc_verified: false }));
  return env;
}
const req = (path, auth, method = 'POST', body = {}) => new Request(`https://api.eustress.dev${path}`, {
  method, headers: { Authorization: auth, 'Content-Type': 'application/json' }, body: method === 'GET' ? undefined : JSON.stringify(body),
});
const send = async (env, path, auth, method, body) => {
  const res = await worker.fetch(req(path, auth, method, body), env, ctx);
  let parsed = null;
  try { parsed = await res.json(); } catch { /* not JSON */ }
  return { status: res.status, body: parsed };
};
/// The token with its signature split by `gap`, which atob would quietly drop.
const split = (token, gap) => { const i = token.lastIndexOf('.') + 5; return token.slice(0, i) + gap + token.slice(i); };

test('the reader: one clean credential after "Bearer ", nothing split by whitespace', () => {
  const t = mintToken(AGENT);
  const r = (auth) => new Request('https://x/', { headers: auth === undefined ? {} : { Authorization: auth } });
  assert.equal(bearerToken(r(`Bearer ${t}`)), t);
  assert.equal(bearerToken(r(`bearer ${t}`)), t, 'the scheme is case-insensitive, for every reader alike');
  for (const bad of [split(t, ' '), split(t, '\t'), `Bearer  ${t}`, `Bearer\t${t}`, 'Bearer', 'Bearer ', `Basic ${t}`, `${t}`]) {
    assert.equal(bearerToken(r(bad.startsWith('Bearer') || bad.startsWith('Basic') ? bad : bad)), null, JSON.stringify(bad));
  }
  assert.equal(bearerToken(r(`Bearer ${split(t, ' ')}`)), null);
  assert.equal(bearerToken(r(`Bearer ${split(t, '\t')}`)), null);
  assert.equal(presentsBearer(r(`Bearer ${split(t, ' ')}`)), true);
  assert.equal(presentsBearer(r(undefined)), false);
  assert.equal(bearerToken(r('Bearer ek_live_' + 'a'.repeat(40))), 'ek_live_' + 'a'.repeat(40), 'a commerce API key still reads');
});

test('an SI account sending a split session token is refused, not let through to the route', async () => {
  const env = await env0();
  const t = mintToken(AGENT);
  // The control: the clean token reaches the gate, which refuses this route to an SI account by name.
  assert.equal((await send(env, '/api/keys', `Bearer ${t}`)).body?.code, 'agent_not_allowed');
  for (const gap of [' ', '\t']) {
    for (const path of ['/api/keys', '/api/cosign', '/api/commerce/products', '/api/kyc/session']) {
      const r = await send(env, path, `Bearer ${split(t, gap)}`);
      assert.equal(r.status, 401, `${path} with ${JSON.stringify(gap)}: ${JSON.stringify(r.body)}`);
    }
  }
  assert.equal(await env.USERS.get(`apikeys:${AGENT}`), null, 'no API key was made');
});

test('the gate answers a token it cannot read with 401 bad_token', async () => {
  const env = await env0();
  for (const who of [AGENT, PERSON]) {
    const r = await send(env, '/api/keys', `Bearer ${split(mintToken(who), ' ')}`);
    assert.deepEqual([r.status, r.body?.code], [401, 'bad_token'], who);
  }
});

test('an unverified person sending a split token is refused the same way', async () => {
  const env = await env0();
  assert.equal((await send(env, '/api/keys', `Bearer ${mintToken(PERSON)}`)).status, 403, 'the control: the gate refuses the clean token');
  assert.equal((await send(env, '/api/keys', `Bearer ${split(mintToken(PERSON), ' ')}`)).status, 401);
});

test('the verifier takes only the base64url alphabet: a "+" or "/" spelling of the same signature is no session', async () => {
  const env = await env0();
  // A signature holding "-" or "_", spelled with "+" or "/" instead. atob reads both spellings as the same bytes.
  const b64url = (s) => Buffer.from(s).toString('base64url');
  let token = null;
  for (let n = 0; n < 200 && !token; n++) {
    const now = Math.floor(Date.now() / 1000);
    const h = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
    const p = b64url(JSON.stringify({ sub: AGENT, iat: now, exp: now + 3600, n }));
    const sig = crypto.createHmac('sha256', JWT_SECRET).update(`${h}.${p}`).digest('base64url');
    if (/[-_]/.test(sig)) token = `${h}.${p}.${sig}`;
  }
  assert.ok(token, 'a signature with "-" or "_" was found');
  assert.equal((await send(env, '/api/auth/me', `Bearer ${token}`, 'GET')).status, 200, 'the control: the token itself is good');
  const respelled = token.replace(/\.([^.]+)$/, (_m, sig) => '.' + sig.replace(/-/g, '+').replace(/_/g, '/'));
  assert.equal((await send(env, '/api/auth/me', `Bearer ${respelled}`, 'GET')).status, 401);
});
