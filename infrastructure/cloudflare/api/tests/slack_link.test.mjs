// Linking a Slack account through the real routes (src/slack_link.mjs): begin
// from an admitted HTTPS site, finish only by the account that began, one Slack
// identity ({team}:{user}) per Eustress account, unlink, and a key replacement's
// revocation. The token Slack issues is revoked right after userInfo and nothing
// but the link is stored. The real Worker and account object run against
// in-memory storage; Slack is a stand-in on globalThis.fetch.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, rotationMessage } from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { currentSlackLink, SLACK_TOKEN, SLACK_USERINFO, SLACK_REVOKE } from '../src/slack_link.mjs';
import { routePolicy, FRESH, SESSION } from '../src/route_policy.mjs';

const worker = await loadWorker();
const API = 'https://api.eustress.dev';
const SITE = 'https://eustress.dev';
const ALICE = '73333333-3333-4333-8333-333333333333';
const BOB = '74444444-4444-4444-8444-444444444444';
const CLIENT_ID = '1111111111.2222222222';
const CLIENT_SECRET = 'test-slack-secret';

// ── Slack, stood in ─────────────────────────────────────────────────────────

/// code -> the Slack identity it signs in. A code works once, as Slack's do.
const slackCodes = new Map();
/// access token -> identity, while Slack still honours it.
const liveTokens = new Map();
const slackCalls = [];
/// Set to make the next token or userInfo reply a refusal (Slack answers HTTP 200 with ok:false).
const refuse = { token: null, userinfo: null };
let issued = 0;
const realFetch = globalThis.fetch;
globalThis.fetch = async (input, init = {}) => {
  const url = typeof input === 'string' ? input : input.url;
  const reply = (body) => new Response(JSON.stringify(body), { status: 200, headers: { 'Content-Type': 'application/json' } });
  if (url === SLACK_TOKEN) {
    slackCalls.push({ url, init });
    const form = new URLSearchParams(init.body);
    if (refuse.token) { const error = refuse.token; refuse.token = null; return reply({ ok: false, error }); }
    if (form.get('client_id') !== CLIENT_ID || form.get('client_secret') !== CLIENT_SECRET) return reply({ ok: false, error: 'invalid_client_id' });
    if (form.get('grant_type') !== 'authorization_code') return reply({ ok: false, error: 'invalid_grant_type' });
    const who = slackCodes.get(form.get('code'));
    slackCodes.delete(form.get('code'));
    if (!who) return reply({ ok: false, error: 'invalid_code' });
    if (form.get('redirect_uri') !== who.redirect) return reply({ ok: false, error: 'bad_redirect_uri' });
    const token = `xoxp-${who.team}-${who.user}-${++issued}`;
    liveTokens.set(token, who);
    return reply({ ok: true, access_token: token, token_type: 'Bearer', id_token: `idt.${token}.sig` });
  }
  if (url === SLACK_USERINFO) {
    slackCalls.push({ url, init });
    const token = String(init.headers?.Authorization || '').replace(/^Bearer /, '');
    if (refuse.userinfo) { const error = refuse.userinfo; refuse.userinfo = null; return reply({ ok: false, error }); }
    const who = liveTokens.get(token);
    if (!who) return reply({ ok: false, error: 'invalid_auth' });
    return reply({
      ok: true, sub: who.user, 'https://slack.com/user_id': who.user, 'https://slack.com/team_id': who.team,
      'https://slack.com/team_name': who.teamName || 'Acme', name: who.name || 'Alice Example', email_verified: false,
    });
  }
  if (url === SLACK_REVOKE) {
    slackCalls.push({ url, init });
    const token = String(init.headers?.Authorization || '').replace(/^Bearer /, '');
    const known = liveTokens.delete(token);
    return reply(known ? { ok: true, revoked: true } : { ok: false, error: 'invalid_auth' });
  }
  if (String(url).startsWith('https://slack.com/')) throw new Error(`unexpected Slack call ${url}`);
  return realFetch(input, init);
};

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

const keys = {};
async function environment(extra = {}) {
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: API,
    SLACK_CLIENT_ID: CLIENT_ID, SLACK_CLIENT_SECRET: CLIENT_SECRET, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  for (const [id, name] of [[ALICE, 'alice'], [BOB, 'bob']]) {
    const key = keyPair();
    keys[id] = key;
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name, public_key: key.hex, email: `${name}@example.com`, created_at: '2026-01-01T00:00:00Z' }));
    await env.USERS.put(`username:${name}`, id);
    await env.USERS.put(`pubkey:${key.hex}`, id);
  }
  return env;
}

async function call(env, method, route, { token, body, origin = SITE } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (origin) headers.Origin = origin;
  const res = await worker.fetch(new Request(`${API}${route}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

/// Begin as `token`, have Slack hand back a code for `who`, and return the URL, state and code.
async function begin(env, token, who) {
  const r = await call(env, 'POST', '/api/auth/slack/begin', { token, body: {} });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const url = new URL(r.body.authorize_url);
  const code = `${Date.now()}.${crypto.randomBytes(8).toString('hex')}`;
  slackCodes.set(code, { ...who, redirect: url.searchParams.get('redirect_uri') });
  return { url, state: url.searchParams.get('state'), code };
}

async function link(env, token, who) {
  const { state, code } = await begin(env, token, who);
  return call(env, 'POST', '/api/auth/slack/finish', { token, body: { code, state } });
}

const me = async (env, token) => (await call(env, 'GET', '/api/auth/me', { token })).body;
const ACME_ALICE = { team: 'T0ACME0001', user: 'U0ALICE001', teamName: 'Acme', name: 'Alice Example' };

// ── The routes ───────────────────────────────────────────────────────────────

test('the routes are classed: linking is fresh, unlinking is the session\'s own', () => {
  assert.equal(routePolicy('POST', '/api/auth/slack/begin').class, FRESH);
  assert.equal(routePolicy('POST', '/api/auth/slack/finish').class, FRESH);
  assert.equal(routePolicy('POST', '/api/auth/slack/unlink').class, SESSION);
  for (const p of ['/api/auth/slack/begin', '/api/auth/slack/finish', '/api/auth/slack/unlink']) assert.equal(routePolicy('POST', p).explicit, true, p);
});

test('begin needs a session, both Slack secrets, a site this deployment admits, and HTTPS', async () => {
  const env = await environment();
  assert.equal((await call(env, 'POST', '/api/auth/slack/begin', { body: {} })).status, 401);
  assert.equal((await call(env, 'GET', '/api/auth/slack/begin', { token: mintToken(ALICE) })).status, 405);

  for (const missing of [{ SLACK_CLIENT_SECRET: '' }, { SLACK_CLIENT_ID: '' }]) {
    const off = await call(await environment(missing), 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {} });
    assert.equal(off.status, 503);
    assert.equal(off.body.code, 'slack_unavailable');
  }

  const elsewhere = await call(env, 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {}, origin: 'https://evil.example' });
  assert.equal(elsewhere.status, 400);
  assert.equal(elsewhere.body.code, 'bad_origin');
  const none = await call(env, 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {}, origin: null });
  assert.equal(none.body.code, 'bad_origin');

  // localhost is an admitted origin, but Slack refuses http redirects: say so instead of sending the browser there.
  for (const local of ['http://localhost:3000', 'http://127.0.0.1:3000']) {
    const r = await call(env, 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {}, origin: local });
    assert.equal(r.status, 400, local);
    assert.equal(r.body.code, 'slack_needs_https', local);
    assert.equal(r.body.authorize_url, undefined);
  }
  assert.equal(env.CHALLENGES.store.size, 0, 'no refused begin leaves a state behind');
});

test('begin sends the browser to Sign in with Slack for openid profile only, back to the site that asked', async () => {
  const env = await environment();
  const r = await call(env, 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {} });
  assert.equal(r.status, 200);
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
  const url = new URL(r.body.authorize_url);
  assert.equal(url.origin + url.pathname, 'https://slack.com/openid/connect/authorize');
  assert.equal(url.searchParams.get('response_type'), 'code');
  assert.equal(url.searchParams.get('scope'), 'openid profile');
  assert.equal(url.searchParams.get('client_id'), CLIENT_ID);
  assert.equal(url.searchParams.get('redirect_uri'), `${SITE}/auth/slack/callback`);
  assert.equal(url.searchParams.has('nonce'), false, 'no nonce: the id_token is not read, so none is checked');
  const state = url.searchParams.get('state');
  assert.match(state, /^[0-9a-f]{64}$/);
  // Stored hashed: the state in the URL is not in storage, as a key or a value.
  assert.equal(env.CHALLENGES.store.size, 1);
  for (const [k, v] of env.CHALLENGES.store) {
    assert.ok(!k.includes(state) && !String(v).includes(state));
    assert.match(k, /^slackstate:[0-9a-f]{64}$/);
  }
  const held = JSON.parse([...env.CHALLENGES.store.values()][0]);
  assert.deepEqual(held, { user_id: ALICE, redirect_uri: `${SITE}/auth/slack/callback` });

  // The other admitted site gets its own redirect.
  const www = await call(env, 'POST', '/api/auth/slack/begin', { token: mintToken(ALICE), body: {}, origin: 'https://www.eustress.dev' });
  assert.equal(new URL(www.body.authorize_url).searchParams.get('redirect_uri'), 'https://www.eustress.dev/auth/slack/callback');
});

test('finish links the Slack identity, revokes the token right after userInfo, and stores no token anywhere', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  slackCalls.length = 0;
  const r = await link(env, alice, ACME_ALICE);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.user.slack_linked, true);
  assert.equal(r.body.user.slack_name, 'Alice Example');
  assert.equal(r.body.user.slack_team_name, 'Acme');
  assert.equal(r.body.user.auth_method, 'key', 'the reply says how the session signed in, as /api/auth/me does');
  const shown = await me(env, alice);
  assert.deepEqual([shown.slack_linked, shown.slack_name, shown.slack_team_name], [true, 'Alice Example', 'Acme']);
  assert.equal(shown.discord_id, null, 'the Discord fields are untouched');

  // TOKEN, USERINFO, REVOKE, in that order, and nothing else.
  assert.deepEqual(slackCalls.map((c) => c.url), [SLACK_TOKEN, SLACK_USERINFO, SLACK_REVOKE]);
  const form = new URLSearchParams(slackCalls[0].init.body);
  assert.equal(slackCalls[0].init.method, 'POST');
  assert.deepEqual(Object.fromEntries(form), {
    client_id: CLIENT_ID, client_secret: CLIENT_SECRET, code: form.get('code'),
    redirect_uri: `${SITE}/auth/slack/callback`, grant_type: 'authorization_code',
  });
  const token = slackCalls[1].init.headers.Authorization.replace('Bearer ', '');
  assert.match(token, /^xoxp-T0ACME0001-U0ALICE001-\d+$/);
  assert.equal(slackCalls[2].init.headers.Authorization, `Bearer ${token}`, 'the same token is revoked');
  assert.equal(liveTokens.has(token), false, 'Slack no longer honours it');

  // The claim and the link; no token, id_token or secret in any stored value.
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), ALICE);
  const stored = JSON.parse(await env.USERS.get(`slacklink:${ALICE}`));
  assert.deepEqual(Object.keys(stored).sort(), ['linked_at', 'name', 'team_id', 'team_name', 'user_id']);
  for (const store of [env.USERS.store, env.CHALLENGES.store]) {
    for (const [k, v] of store) {
      const text = `${k} ${typeof v === 'string' ? v : JSON.stringify(v)}`;
      assert.ok(!text.includes('xoxp-') && !text.includes('idt.') && !text.includes(CLIENT_SECRET), `nothing secret under ${k}`);
    }
  }
  assert.equal(env.CHALLENGES.store.size, 0, 'the state was used up');
  // The user record is not rewritten.
  assert.equal(JSON.parse(await env.USERS.get(`user:${ALICE}`)).slack_linked, undefined);
});

test('Slack answering ok:false is refused plainly and links nothing', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);

  // A code Slack does not know: HTTP 200, ok:false, invalid_code.
  const { state } = await begin(env, alice, ACME_ALICE);
  slackCalls.length = 0;
  const rejected = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code: '1234.5678.notissued', state } });
  assert.equal(rejected.status, 400);
  assert.equal(rejected.body.code, 'slack_code_rejected');
  assert.deepEqual(slackCalls.map((c) => c.url), [SLACK_TOKEN], 'no token, so nothing to read or revoke');

  // Slack refusing the app itself (a wrong secret, say) is Slack failing, not the person's code.
  const second = await begin(env, alice, ACME_ALICE);
  refuse.token = 'invalid_client_id';
  const broken = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code: second.code, state: second.state } });
  assert.equal(broken.status, 502);
  assert.equal(broken.body.code, 'slack_failed');

  // userInfo refusing: the token it was handed is still revoked.
  const third = await begin(env, alice, ACME_ALICE);
  refuse.userinfo = 'invalid_auth';
  slackCalls.length = 0;
  const noInfo = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code: third.code, state: third.state } });
  assert.equal(noInfo.status, 502);
  assert.equal(noInfo.body.code, 'slack_failed');
  assert.deepEqual(slackCalls.map((c) => c.url), [SLACK_TOKEN, SLACK_USERINFO, SLACK_REVOKE]);

  const shown = await me(env, alice);
  assert.deepEqual([shown.slack_linked, shown.slack_name, shown.slack_team_name], [false, null, null]);
  assert.equal(await env.USERS.get(`slacklink:${ALICE}`), null);
  assert.equal([...env.USERS.store.keys()].some((k) => k.startsWith('slack:')), false);
});

test('an identity Slack sends without usable ids links nothing', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  for (const who of [{ team: 't-lower', user: 'U0ALICE001' }, { team: 'T0ACME0001', user: 'U0ALICE001:X' }, { team: 'T0ACME0001', user: '' }]) {
    const r = await link(env, alice, who);
    assert.equal(r.status, 502, JSON.stringify(who));
    assert.equal(r.body.code, 'slack_failed');
  }
  assert.equal((await me(env, alice)).slack_linked, false);
});

test('a state works once, and only for the account that began it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  const { state, code } = await begin(env, alice, ACME_ALICE);

  // Bob finishing Alice's link (a link sent to him, say) is refused, and Alice can still finish it.
  slackCalls.length = 0;
  const stolen = await call(env, 'POST', '/api/auth/slack/finish', { token: mintToken(BOB), body: { code, state } });
  assert.equal(stolen.status, 403);
  assert.equal(stolen.body.code, 'state_mismatch');
  assert.equal(slackCalls.length, 0, 'the code never reaches Slack for the wrong account');
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), null);
  assert.equal((await me(env, mintToken(BOB))).slack_linked, false);

  const ok = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code, state } });
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  const again = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code, state } });
  assert.equal(again.status, 400);
  assert.equal(again.body.code, 'state_expired');

  for (const body of [{ code: 'x', state: 'nope' }, { code: '1234.5678.abcdef', state: 'A'.repeat(64) }, {}]) {
    const junk = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body });
    assert.equal(junk.status, 400);
    assert.equal(junk.body.code, 'bad_request');
  }
  assert.equal((await call(env, 'POST', '/api/auth/slack/finish', { body: { code, state } })).status, 401);
});

test('a state begun for Discord does not finish a Slack link', async () => {
  const env = await environment({ DISCORD_CLIENT_ID: '1234567890', DISCORD_CLIENT_SECRET: 'x' });
  const alice = mintToken(ALICE);
  const d = await call(env, 'POST', '/api/auth/discord/begin', { token: alice, body: {} });
  assert.equal(d.status, 200, JSON.stringify(d.body));
  const state = new URL(d.body.authorize_url).searchParams.get('state');
  const r = await call(env, 'POST', '/api/auth/slack/finish', { token: alice, body: { code: '1234.5678.abcdef', state } });
  assert.equal(r.status, 400);
  assert.equal(r.body.code, 'state_expired');
});

test('one Slack identity links one Eustress account; unlinking frees it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  const bob = mintToken(BOB);
  assert.equal((await link(env, alice, ACME_ALICE)).status, 200);

  slackCalls.length = 0;
  const taken = await link(env, bob, ACME_ALICE);
  assert.equal(taken.status, 409);
  assert.equal(taken.body.code, 'slack_taken');
  assert.deepEqual(slackCalls.map((c) => c.url), [SLACK_TOKEN, SLACK_USERINFO, SLACK_REVOKE], 'the token is revoked even when the link is refused');
  assert.equal((await me(env, bob)).slack_linked, false);
  assert.equal(await env.USERS.get(`slacklink:${BOB}`), null);
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), ALICE);

  const off = await call(env, 'POST', '/api/auth/slack/unlink', { token: alice, body: {} });
  assert.equal(off.status, 200);
  assert.deepEqual([off.body.user.slack_linked, off.body.user.slack_name, off.body.user.slack_team_name], [false, null, null]);
  assert.equal((await me(env, alice)).slack_linked, false);
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), null);
  assert.equal(await env.USERS.get(`slacklink:${ALICE}`), null);

  assert.equal((await link(env, bob, ACME_ALICE)).status, 200);
  assert.equal((await me(env, bob)).slack_linked, true);
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), BOB);
});

test('the same user id in two workspaces is two identities', async () => {
  const env = await environment();
  assert.equal((await link(env, mintToken(ALICE), { team: 'T0ACME0001', user: 'U0SHARED01', teamName: 'Acme' })).status, 200);
  const other = await link(env, mintToken(BOB), { team: 'T0GLOBEX01', user: 'U0SHARED01', teamName: 'Globex', name: 'Bob B' });
  assert.equal(other.status, 200, JSON.stringify(other.body));
  assert.equal(other.body.user.slack_team_name, 'Globex');
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0SHARED01'), ALICE);
  assert.equal(await env.USERS.get('slack:T0GLOBEX01:U0SHARED01'), BOB);
});

test('linking another Slack identity replaces the first and releases its claim', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, ACME_ALICE)).status, 200);
  const r = await link(env, alice, { team: 'T0GLOBEX01', user: 'U0ALICE002', teamName: 'Globex', name: 'Alice G' });
  assert.equal(r.status, 200);
  assert.deepEqual([r.body.user.slack_name, r.body.user.slack_team_name], ['Alice G', 'Globex']);
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), null);
  assert.equal(await env.USERS.get('slack:T0GLOBEX01:U0ALICE002'), ALICE);
  // Relinking the same identity keeps its claim.
  assert.equal((await link(env, alice, { team: 'T0GLOBEX01', user: 'U0ALICE002', teamName: 'Globex', name: 'Alice G' })).status, 200);
  assert.equal(await env.USERS.get('slack:T0GLOBEX01:U0ALICE002'), ALICE);
  // The first identity is free for someone else.
  assert.equal((await link(env, mintToken(BOB), ACME_ALICE)).status, 200);
});

test('a key replacement revokes the link, and a relink after it shows', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, ACME_ALICE)).status, 200);

  // What a replacement that revokes the link writes (src/identity_routes.mjs).
  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await accountCall(env, ALICE, 'unlink_slack')).ok, true);
  assert.ok((await accountCall(env, ALICE, 'state')).slack_unlinked_at);
  assert.equal((await me(env, alice)).slack_linked, false, 'revoked by the unlink');

  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await link(env, alice, ACME_ALICE)).status, 200);
  assert.equal((await me(env, alice)).slack_linked, true, 'the owner linked it again');
});

test('replacing the key lists the Slack link, and revoking it there hides it until a relink', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, ACME_ALICE)).status, 200);

  const post = async (route, body, token) => {
    const res = await worker.fetch(new Request(`${API}${route}`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }, body: JSON.stringify(body) }), env, ctx);
    return { status: res.status, body: await res.json().catch(() => null) };
  };
  const begun = await post('/api/identity/rotate/begin', {}, alice);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  assert.equal(begun.body.credentials.slack_linked, true);
  assert.equal(begun.body.credentials.discord_linked, false);

  // A revoke that is not a boolean is refused before anything changes.
  const fresh = keyPair();
  const message = rotationMessage({ origin: API, accountId: ALICE, oldKey: keys[ALICE].hex, newKey: fresh.hex, challenge: begun.body.challenge });
  const request = { new_public_key: fresh.hex, challenge: begun.body.challenge, old_signature: keys[ALICE].sign(message), new_signature: fresh.sign(message) };
  assert.equal((await post('/api/identity/rotate', { ...request, revoke: { slack: 'yes' } }, alice)).status, 400);

  await new Promise((r) => setTimeout(r, 5));
  const rotated = await post('/api/identity/rotate', { ...request, revoke: { slack: true } }, alice);
  assert.equal(rotated.status, 200, JSON.stringify(rotated.body));
  assert.equal(rotated.body.revoked.slack, true);
  assert.equal(rotated.body.user.slack_linked, false, 'the account no longer shows the link');
  const token = rotated.body.token;
  assert.equal((await me(env, token)).slack_linked, false);
  // The record is still in KV, hidden by the account object's word, and still names Alice for her own relink.
  assert.ok(await env.USERS.get(`slacklink:${ALICE}`));

  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await link(env, token, ACME_ALICE)).status, 200);
  assert.equal((await me(env, token)).slack_linked, true);
});

test('deleting the account takes the link and its claim with it', async () => {
  const { deleteSlackLink } = await import('../src/slack_link.mjs');
  const env = await environment();
  assert.equal((await link(env, mintToken(ALICE), ACME_ALICE)).status, 200);
  // Another account's claim is never released by this one.
  await env.USERS.put('slack:T0OTHER001:U0OTHER001', BOB);
  await deleteSlackLink(env, ALICE);
  assert.equal(await env.USERS.get(`slacklink:${ALICE}`), null);
  assert.equal(await env.USERS.get('slack:T0ACME0001:U0ALICE001'), null);
  assert.equal(await env.USERS.get('slack:T0OTHER001:U0OTHER001'), BOB);
});

test('the shown link follows the account\'s unlink time', async () => {
  const env = { USERS: fakeKv() };
  const user = { id: ALICE };
  assert.equal(await currentSlackLink(env, user, null), null);
  await env.USERS.put(`slacklink:${ALICE}`, JSON.stringify({ team_id: 'T0ACME0001', user_id: 'U0ALICE001', name: 'A', team_name: 'Acme', linked_at: '2026-11-10T00:00:00.000Z' }));
  assert.equal((await currentSlackLink(env, user, null)).user_id, 'U0ALICE001');
  assert.equal((await currentSlackLink(env, user, '2026-11-09T00:00:00.000Z')).team_id, 'T0ACME0001', 'linked after the unlink');
  assert.equal(await currentSlackLink(env, user, '2026-11-11T00:00:00.000Z'), null, 'linked before the unlink');
  // A record whose ids are not Slack's shape is no link.
  await env.USERS.put(`slacklink:${ALICE}`, JSON.stringify({ team_id: 't', user_id: 'U0ALICE001', linked_at: '2026-11-10T00:00:00.000Z' }));
  assert.equal(await currentSlackLink(env, user, null), null);
});
