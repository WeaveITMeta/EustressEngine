// Linking a Discord account through the real routes (src/discord_link.mjs): begin
// from an admitted site, finish only by the account that began, one Discord
// account per Eustress account, unlink, and a key replacement's revocation; the
// Linked Roles connection pushed at link, and the refresh token kept sealed and
// revoked when the link goes. The real Worker and account object run against
// in-memory storage; Discord is a stand-in on globalThis.fetch.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall } from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { currentDiscordLink, openDiscordToken, repushRoleConnection, roleConnectionUrl, DISCORD_TOKEN, DISCORD_ME, DISCORD_REVOKE } from '../src/discord_link.mjs';
import { routePolicy, FRESH, SESSION } from '../src/route_policy.mjs';

const worker = await loadWorker();
const API = 'https://api.eustress.dev';
const SITE = 'https://eustress.dev';
const ALICE = '71111111-1111-4111-8111-111111111111';
const BOB = '72222222-2222-4222-8222-222222222222';

// ── Discord, stood in ───────────────────────────────────────────────────────

/// code -> the Discord user it signs in. A code works once, as Discord's do.
const discordCodes = new Map();
const discordCalls = [];
/// refresh token -> Discord user id, for the refresh grant. A refresh token works once: Discord rotates them.
const refreshTokens = new Map();
let issued = 0;
const ROLE_CONNECTION = roleConnectionUrl('1234567890');
const realFetch = globalThis.fetch;
globalThis.fetch = async (input, init = {}) => {
  const url = typeof input === 'string' ? input : input.url;
  if (url === DISCORD_TOKEN) {
    discordCalls.push({ url, init });
    const form = new URLSearchParams(init.body);
    if (form.get('grant_type') === 'refresh_token') {
      const id = refreshTokens.get(form.get('refresh_token'));
      refreshTokens.delete(form.get('refresh_token'));
      if (!id) return new Response(JSON.stringify({ error: 'invalid_grant' }), { status: 400 });
      const rt = `rt-${id}-${++issued}`;
      refreshTokens.set(rt, id);
      return new Response(JSON.stringify({ access_token: `at-${id}`, refresh_token: rt, token_type: 'Bearer' }), { status: 200 });
    }
    const who = discordCodes.get(form.get('code'));
    discordCodes.delete(form.get('code'));
    if (!who || !String(form.get('redirect_uri')).endsWith('/auth/discord/callback')) {
      return new Response(JSON.stringify({ error: 'invalid_grant' }), { status: 400 });
    }
    const rt = `rt-${who.id}-${++issued}`;
    refreshTokens.set(rt, who.id);
    return new Response(JSON.stringify({ access_token: `at-${who.id}`, refresh_token: rt, token_type: 'Bearer', scope: 'identify role_connections.write' }), { status: 200 });
  }
  if (url === ROLE_CONNECTION) {
    discordCalls.push({ url, init });
    return new Response('{}', { status: 200 });
  }
  if (url === DISCORD_ME) {
    discordCalls.push({ url, init });
    const id = String(init.headers.Authorization).replace('Bearer at-', '');
    return new Response(JSON.stringify({ id, username: `user${id.slice(-3)}`, discriminator: '0', global_name: 'Someone' }), { status: 200 });
  }
  if (url === DISCORD_REVOKE) {
    discordCalls.push({ url, init });
    return new Response('{}', { status: 200 });
  }
  return realFetch(input, init);
};

async function environment(extra = {}) {
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: API,
    DISCORD_CLIENT_ID: '1234567890', DISCORD_CLIENT_SECRET: 'test-discord-secret', ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  for (const [id, name] of [[ALICE, 'alice'], [BOB, 'bob']]) {
    const key = crypto.randomBytes(32).toString('hex');
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name, public_key: key, email: `${name}@example.com`, created_at: '2026-01-01T00:00:00Z' }));
    await env.USERS.put(`username:${name}`, id);
    await env.USERS.put(`pubkey:${key}`, id);
  }
  return env;
}

async function call(env, method, route, { token, body, origin = SITE } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (origin) headers.Origin = origin;
  const res = await worker.fetch(new Request(`${API}${route}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

/// Begin as `token`, have Discord hand back a code for `discordId`, and return the state and code.
async function begin(env, token, discordId) {
  const r = await call(env, 'POST', '/api/auth/discord/begin', { token, body: {} });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const url = new URL(r.body.authorize_url);
  const code = `code-${crypto.randomBytes(8).toString('hex')}`;
  discordCodes.set(code, { id: discordId });
  return { url, state: url.searchParams.get('state'), code };
}

async function link(env, token, discordId) {
  const { state, code } = await begin(env, token, discordId);
  return call(env, 'POST', '/api/auth/discord/finish', { token, body: { code, state } });
}

const me = async (env, token) => (await call(env, 'GET', '/api/auth/me', { token })).body;

// ── The routes ───────────────────────────────────────────────────────────────

test('the routes are classed: linking is fresh, unlinking is the session\'s own', () => {
  assert.equal(routePolicy('POST', '/api/auth/discord/begin').class, FRESH);
  assert.equal(routePolicy('POST', '/api/auth/discord/finish').class, FRESH);
  assert.equal(routePolicy('POST', '/api/auth/discord/unlink').class, SESSION);
  for (const p of ['/api/auth/discord/begin', '/api/auth/discord/finish', '/api/auth/discord/unlink']) assert.equal(routePolicy('POST', p).explicit, true, p);
});

test('begin needs a session, the Discord secrets, and a site this deployment admits', async () => {
  const env = await environment();
  assert.equal((await call(env, 'POST', '/api/auth/discord/begin', { body: {} })).status, 401);

  const unset = await environment({ DISCORD_CLIENT_SECRET: '' });
  const off = await call(unset, 'POST', '/api/auth/discord/begin', { token: mintToken(ALICE), body: {} });
  assert.equal(off.status, 503);
  assert.equal(off.body.code, 'discord_unavailable');

  const elsewhere = await call(env, 'POST', '/api/auth/discord/begin', { token: mintToken(ALICE), body: {}, origin: 'https://evil.example' });
  assert.equal(elsewhere.status, 400);
  assert.equal(elsewhere.body.code, 'bad_origin');
});

test('begin sends the browser to Discord for identify and Linked Roles only, back to the site that asked', async () => {
  const env = await environment();
  const { url, state } = await begin(env, mintToken(ALICE), '100000000000000001');
  assert.equal(url.origin + url.pathname, 'https://discord.com/oauth2/authorize');
  assert.equal(url.searchParams.get('client_id'), '1234567890');
  assert.equal(url.searchParams.get('scope'), 'identify role_connections.write');
  assert.equal(url.searchParams.get('response_type'), 'code');
  assert.equal(url.searchParams.get('redirect_uri'), `${SITE}/auth/discord/callback`);
  assert.match(state, /^[0-9a-f]{64}$/);
  // Stored hashed: the state in the URL is not a key in storage.
  assert.ok(![...env.CHALLENGES.store.keys()].some((k) => k.includes(state)));

  const local = await call(env, 'POST', '/api/auth/discord/begin', { token: mintToken(ALICE), body: {}, origin: 'http://localhost:3000' });
  assert.equal(new URL(local.body.authorize_url).searchParams.get('redirect_uri'), 'http://localhost:3000/auth/discord/callback');
});

test('finish links the Discord account, pushes its Linked Roles connection, and keeps only the refresh token, sealed', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  discordCalls.length = 0;
  const r = await link(env, alice, '100000000000000001');
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.user.discord_id, '100000000000000001');
  assert.equal(r.body.user.discord_username, 'user001');
  assert.equal(r.body.user.auth_method, 'key', 'the reply says how the session signed in, as /api/auth/me does');
  assert.equal((await me(env, alice)).discord_id, '100000000000000001');
  assert.equal(await env.USERS.get('discord:100000000000000001'), ALICE);
  // The token was asked for, read who it is, and wrote the Linked Roles connection. Nothing is revoked: the link keeps it.
  assert.deepEqual(discordCalls.map((c) => c.url), [DISCORD_TOKEN, DISCORD_ME, ROLE_CONNECTION]);
  assert.match(discordCalls[0].init.headers.Authorization, /^Basic /);
  const push = discordCalls[2].init;
  assert.equal(push.method, 'PUT');
  assert.equal(push.headers.Authorization, 'Bearer at-100000000000000001');
  const pushed = JSON.parse(push.body);
  assert.equal(pushed.platform_name, 'Eustress');
  assert.equal(pushed.platform_username, 'alice');
  assert.deepEqual(pushed.metadata, { verified: 1, creator: 0, spaces: 0, member_since: '2026-01-01T00:00:00.000Z' });
  // The refresh token is kept sealed: no stored value carries either token in the clear, and it opens only for Alice.
  const record = JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`));
  for (const v of env.USERS.store.values()) {
    assert.ok(!String(v).includes('at-100000000000000001'));
    assert.ok(!String(v).includes('rt-100000000000000001'));
  }
  assert.match(await openDiscordToken(env, ALICE, record), /^rt-100000000000000001-\d+$/);
  assert.equal(await openDiscordToken(env, BOB, record), null, 'bound to the account it was kept for');
  assert.equal(await openDiscordToken({ ...env, DISCORD_CLIENT_SECRET: 'another-secret' }, ALICE, record), null, 'and to the client secret');
  // The user record is not rewritten.
  assert.equal(JSON.parse(await env.USERS.get(`user:${ALICE}`)).discord_id, undefined);
});

/// The Discord calls an unlink of a kept token makes: refresh, clear the Linked Roles values, revoke the newest grant.
function assertClearedThenRevoked(calls, kept) {
  assert.deepEqual(calls.map((c) => c.url), [DISCORD_TOKEN, ROLE_CONNECTION, DISCORD_REVOKE]);
  assert.equal(new URLSearchParams(calls[0].init.body).get('refresh_token'), kept);
  const cleared = JSON.parse(calls[1].init.body);
  assert.equal(calls[1].init.method, 'PUT');
  assert.deepEqual(cleared.metadata, {}, 'the role connection is emptied, so server roles stop matching');
  const revoked = Object.fromEntries(new URLSearchParams(calls[2].init.body));
  assert.equal(revoked.token_type_hint, 'refresh_token');
  assert.notEqual(revoked.token, kept, 'the refresh rotated the token; the newest one is revoked');
  assert.match(revoked.token, /^rt-\d+-\d+$/);
  assert.match(calls[2].init.headers.Authorization, /^Basic /);
}

test('unlinking clears the Linked Roles values, then revokes the kept token at Discord and deletes it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000011')).status, 200);
  const kept = await openDiscordToken(env, ALICE, JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`)));
  assert.ok(kept);
  discordCalls.length = 0;
  assert.equal((await call(env, 'POST', '/api/auth/discord/unlink', { token: alice, body: {} })).status, 200);
  assertClearedThenRevoked(discordCalls, kept);
  assert.equal(await env.USERS.get(`discordtoken:${ALICE}`), null);
});

test('linking another Discord account revokes the token kept for the first; relinking the same one only replaces it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000012')).status, 200);
  const first = await openDiscordToken(env, ALICE, JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`)));
  discordCalls.length = 0;
  assert.equal((await link(env, alice, '100000000000000012')).status, 200);
  assert.equal(discordCalls.filter((c) => c.url === DISCORD_REVOKE).length, 0, 'the same Discord account: no revoke that could end the new grant');
  const second = await openDiscordToken(env, ALICE, JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`)));
  assert.notEqual(second, first);
  discordCalls.length = 0;
  assert.equal((await link(env, alice, '100000000000000013')).status, 200);
  // The first Discord account's grant: refreshed from `second`, its roles cleared, then revoked.
  const refreshed = discordCalls.filter((c) => c.url === DISCORD_TOKEN && new URLSearchParams(c.init.body).get('grant_type') === 'refresh_token');
  assert.deepEqual(refreshed.map((c) => new URLSearchParams(c.init.body).get('refresh_token')), [second]);
  const revoked = discordCalls.filter((c) => c.url === DISCORD_REVOKE).map((c) => new URLSearchParams(c.init.body).get('token'));
  assert.equal(revoked.length, 1);
  assert.match(revoked[0], /^rt-100000000000000012-/);
  assert.match(await openDiscordToken(env, ALICE, JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`))), /^rt-100000000000000013-/);
});

test('a key replacement that revokes the link has the kept token revoked and deleted by the next push', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000014')).status, 200);
  const kept = await openDiscordToken(env, ALICE, JSON.parse(await env.USERS.get(`discordtoken:${ALICE}`)));
  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await accountCall(env, ALICE, 'unlink_discord')).ok, true);
  discordCalls.length = 0;
  assert.equal(await repushRoleConnection(env, ALICE, 3), 'revoked');
  assertClearedThenRevoked(discordCalls, kept);
  assert.equal(await env.USERS.get(`discordtoken:${ALICE}`), null);
});

test('a changed count is pushed with a refreshed token; Discord refusing the refresh deletes the kept token', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000015')).status, 200);
  assert.equal(await repushRoleConnection(env, ALICE, 0), 'unchanged', 'what was pushed at link');
  discordCalls.length = 0;
  assert.equal(await repushRoleConnection(env, ALICE, 2), 'pushed');
  assert.deepEqual(discordCalls.map((c) => c.url), [DISCORD_TOKEN, ROLE_CONNECTION]);
  assert.equal(new URLSearchParams(discordCalls[0].init.body).get('grant_type'), 'refresh_token');
  assert.deepEqual(JSON.parse(discordCalls[1].init.body).metadata, { verified: 1, creator: 1, spaces: 2, member_since: '2026-01-01T00:00:00.000Z' });
  assert.equal(await repushRoleConnection(env, ALICE, 2), 'unchanged');
  // Discord no longer knows the refresh token (the person removed the app): the kept token goes.
  refreshTokens.clear();
  assert.equal(await repushRoleConnection(env, ALICE, 4), 'failed');
  assert.equal(await env.USERS.get(`discordtoken:${ALICE}`), null);
  assert.equal(await repushRoleConnection(env, ALICE, 5), 'no_token');
});

test('a state works once, and only for the account that began it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  const { state, code } = await begin(env, alice, '100000000000000002');

  // Bob finishing Alice's link (a link sent to him, say) is refused, and Alice can still finish it.
  const stolen = await call(env, 'POST', '/api/auth/discord/finish', { token: mintToken(BOB), body: { code, state } });
  assert.equal(stolen.status, 403);
  assert.equal(stolen.body.code, 'state_mismatch');
  assert.equal(await env.USERS.get('discord:100000000000000002'), null);

  const ok = await call(env, 'POST', '/api/auth/discord/finish', { token: alice, body: { code, state } });
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  const again = await call(env, 'POST', '/api/auth/discord/finish', { token: alice, body: { code, state } });
  assert.equal(again.status, 400);
  assert.equal(again.body.code, 'state_expired');

  const junk = await call(env, 'POST', '/api/auth/discord/finish', { token: alice, body: { code: 'x', state: 'nope' } });
  assert.equal(junk.status, 400);
});

test('a code Discord refuses is said plainly and links nothing', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  const { state } = await begin(env, alice, '100000000000000003');
  const r = await call(env, 'POST', '/api/auth/discord/finish', { token: alice, body: { code: 'code-not-issued-by-discord', state } });
  assert.equal(r.status, 400);
  assert.equal(r.body.code, 'discord_code_rejected');
  assert.equal((await me(env, alice)).discord_id, null);
});

test('one Discord account links one Eustress account; unlinking frees it', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  const bob = mintToken(BOB);
  assert.equal((await link(env, alice, '100000000000000004')).status, 200);

  discordCalls.length = 0;
  const taken = await link(env, bob, '100000000000000004');
  assert.equal(taken.status, 409);
  assert.equal(taken.body.code, 'discord_taken');
  // The token that code was exchanged for is revoked, and nothing is kept for Bob.
  assert.deepEqual(discordCalls.filter((c) => c.url === DISCORD_REVOKE).map((c) => new URLSearchParams(c.init.body).get('token')), ['at-100000000000000004']);
  assert.equal(discordCalls.filter((c) => c.url === ROLE_CONNECTION).length, 0);
  assert.equal(await env.USERS.get(`discordtoken:${BOB}`), null);
  assert.equal((await me(env, bob)).discord_id, null);

  const off = await call(env, 'POST', '/api/auth/discord/unlink', { token: alice, body: {} });
  assert.equal(off.status, 200);
  assert.equal(off.body.user.discord_id, null);
  assert.equal((await me(env, alice)).discord_id, null);
  assert.equal(await env.USERS.get('discord:100000000000000004'), null);

  assert.equal((await link(env, bob, '100000000000000004')).status, 200);
  assert.equal((await me(env, bob)).discord_id, '100000000000000004');
});

test('linking another Discord account replaces the first and releases its claim', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000005')).status, 200);
  assert.equal((await link(env, alice, '100000000000000006')).status, 200);
  assert.equal((await me(env, alice)).discord_id, '100000000000000006');
  assert.equal(await env.USERS.get('discord:100000000000000005'), null);
  assert.equal(await env.USERS.get('discord:100000000000000006'), ALICE);
});

test('a key replacement revokes the link, and a relink after it shows', async () => {
  const env = await environment();
  const alice = mintToken(ALICE);
  assert.equal((await link(env, alice, '100000000000000007')).status, 200);

  // What a replacement that revokes the link writes (src/identity_routes.mjs).
  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await accountCall(env, ALICE, 'unlink_discord')).ok, true);
  assert.equal((await me(env, alice)).discord_id, null, 'revoked by the unlink');

  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await link(env, alice, '100000000000000007')).status, 200);
  assert.equal((await me(env, alice)).discord_id, '100000000000000007', 'the owner linked it again');
});

test('the shown link follows the account\'s unlink time', async () => {
  const env = { USERS: fakeKv() };
  const user = { id: ALICE };
  assert.equal(await currentDiscordLink(env, user, null), null);
  await env.USERS.put(`discordlink:${ALICE}`, JSON.stringify({ id: '100000000000000008', username: 'u', linked_at: '2026-11-10T00:00:00.000Z' }));
  assert.equal((await currentDiscordLink(env, user, null)).id, '100000000000000008');
  assert.equal((await currentDiscordLink(env, user, '2026-11-09T00:00:00.000Z')).id, '100000000000000008', 'linked after the unlink');
  assert.equal(await currentDiscordLink(env, user, '2026-11-11T00:00:00.000Z'), null, 'linked before the unlink');
  // An older record's discord_id has no time: any unlink hides it.
  const legacy = { id: BOB, discord_id: '100000000000000009' };
  assert.equal((await currentDiscordLink(env, legacy, null)).id, '100000000000000009');
  assert.equal(await currentDiscordLink(env, legacy, '2020-01-01T00:00:00.000Z'), null);
});
