// "Tell me when it is out": a signed-in account asks to be emailed once when the Player or Studio is released, and an
// administrator announces a release (src/release_notify.mjs, docs/networking/RELEASE_NOTIFY.md). The email is the account's own
// VERIFIED email, read when the notice is sent and never taken from a request; the subscription holds no copy of it; the
// send emails each subscriber once and ends their subscription; a failure for one leaves that one for the next run. The real
// Worker runs against in-memory storage, real account objects (the email predicate is theirs) and a fake mail binding.
//
// Source note: no backslash-u escape appears in this file on purpose; every character is plain ASCII.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeBucket, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountState, accountCall } from '../src/account_auth.mjs';
import { takeMailSlot, MAIL_PER_DAY } from '../src/mail_limits.mjs';
import { buildNoticeMime, scrubAddresses, maskAddress } from '../src/account_notices.mjs';
import { routePolicy, SESSION, FRESH } from '../src/route_policy.mjs';
import {
  PRODUCTS, SEND_PAGE_DEFAULT, SEND_PAGE_MAX, VERSION_PATTERN, RELEASE_LINK_HOSTS, subscriptionKey, checkReleaseUrl, renderReleaseEmail, handleReleaseNotify, emailStatusOf, claimKey, claimHash, claimSecret, CLAIM_STALE_MS,
  SUBREQUESTS_PER_SUBSCRIBER, SUBREQUESTS_PER_REQUEST, SUBREQUEST_LIMIT,
} from '../src/release_notify.mjs';

const worker = await loadWorker();

const ADMIN = '99999999-9999-4999-8999-999999999999';
const accountId = (n) => `aaaaaaaa-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
const tokens = new Map();
const tokenOf = (id) => { if (!tokens.has(id)) tokens.set(id, mintToken(id)); return tokens.get(id); };
const URL_OK = 'https://downloads.eustress.dev/player/0.1.0/eustress-player-0.1.0.msi';

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

/// The decoded text and HTML parts of a message the Worker built, and one of its headers.
function parts(message) {
  const decoded = [...message.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)].map((m) => Buffer.from(m[1].replace(/\r\n/g, ''), 'base64').toString());
  return { text: decoded[0] || '', html: decoded[1] || '' };
}
const header = (message, name) => new RegExp(`^${name}: (.*)$`, 'm').exec(message.raw)?.[1] ?? null;

async function environment({ failFor = new Set(), extra = {} } = {}) {
  const sent = [];
  const env = {
    ...fakeEnv(), LOGIN_ORIGINS: 'https://api.eustress.dev',
    EMAIL: { send: async (m) => { if (failFor.has(m.to)) throw new Error('mail is down'); sent.push(m); } }, __EmailMessage: StubMessage, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'root', email: 'root@example.com' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return { env, sent };
}

/// An account with an address on file; `verified` makes that address a verified one, through the account's own object.
async function account(env, n, { email = `user${n}@example.com`, verified = true } = {}) {
  const id = accountId(n);
  await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: `maker${n}`, public_key: crypto.randomBytes(32).toString('hex'), ...(email ? { email } : {}) }));
  if (verified) await verifyEmail(env, id);
  return id;
}
async function verifyEmail(env, id) {
  assert.equal((await accountState(env, id)).ok, true);
  const started = await accountCall(env, id, 'email_verify_start');
  assert.equal(started.ok, true, JSON.stringify(started));
  assert.equal((await accountCall(env, id, 'email_verify_confirm', { token: started.token.split('.')[1] })).ok, true);
}
/// The account's email stops being verified (as it is when the address on file changes): the object's own state.
async function unverify(env, id) {
  const object = env.ACCOUNT_AUTH.instances.get(id);
  const state = await object.storage.get('state');
  state.email.verified = null;
  await object.storage.put('state', state);
}

async function call(env, method, path, { who, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (who) h.Authorization = `Bearer ${tokenOf(who)}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body: parsed };
}
const sub = (product = 'player') => `/api/release-notify/${product}`;
const sendPath = (product = 'player') => `/api/admin/release-notify/${product}/send`;
const send = (env, body = {}, { product = 'player', who = ADMIN } = {}) => call(env, 'POST', sendPath(product), { who, body: { version: '0.1.0', url: URL_OK, ...body } });
const subscribe = (env, who, product = 'player') => call(env, 'PUT', sub(product), { who });
const subscribed = async (env, id, product = 'player') => (await env.USERS.get(subscriptionKey(product, id))) !== null;
const subscribers = (env, product = 'player') => [...env.USERS.store.keys()].filter((k) => k.startsWith(`relnotify:${product}:`));
const audits = async (env) => (await Promise.all([...env.AUDIT_LOG.store.entries()].filter(([k]) => k.startsWith('audit:')).map(([, v]) => JSON.parse(v)))).filter((e) => e.action === 'RELEASE_NOTIFY_SEND');

// -- the products -------------------------------------------------------------------------------------------------------------------

test('there are exactly two products, the player and studio, and any other name is a 404 on every route', async () => {
  assert.deepEqual(Object.keys(PRODUCTS), ['player', 'studio']);
  const { env, sent } = await environment();
  const id = await account(env, 1);
  const unknown = ['launcher', 'Player', 'PLAYER', 'players', 'player-2', 'constructor', 'toString', 'hasownproperty', 'valueof', '0', 'a'.repeat(31), 'x'];
  for (const product of unknown) {
    for (const method of ['GET', 'PUT', 'DELETE']) {
      const r = await call(env, method, sub(product), { who: id });
      assert.equal(r.status, 404, `${method} ${product}`);
      if (/^[a-z0-9-]+$/.test(product)) assert.equal(r.body.code, 'unknown_product', `${method} ${product}`);
    }
    const s = await send(env, {}, { product });
    assert.equal(s.status, 404, `send ${product}`);
  }
  assert.deepEqual(subscribers(env, ''), [], 'nothing was stored for any of them');
  assert.equal(sent.length, 0);
  for (const product of ['player', 'studio']) assert.equal((await call(env, 'GET', sub(product), { who: id })).status, 200, product);
});

test('the routes are named: a subscription is a session route, the send is a fresh admin route', () => {
  for (const method of ['GET', 'PUT', 'DELETE']) {
    for (const product of ['player', 'studio']) {
      const r = routePolicy(method, `/api/release-notify/${product}`);
      assert.equal(r.class, SESSION, `${method} ${product}`);
      assert.equal(r.explicit, true);
    }
  }
  for (const product of ['player', 'studio']) {
    const r = routePolicy('POST', `/api/admin/release-notify/${product}/send`);
    assert.equal(r.class, FRESH);
    assert.equal(r.explicit, true);
    assert.match(r.pattern, /release-notify/, 'a row of its own, not only the admin prefix');
  }
});

test('other methods are refused on both routes', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  assert.equal((await call(env, 'POST', sub(), { who: id, body: {} })).status, 405);
  assert.equal((await call(env, 'PATCH', sub(), { who: id, body: {} })).status, 405);
  assert.equal((await call(env, 'GET', sendPath(), { who: ADMIN })).status, 405);
  assert.equal((await call(env, 'PUT', sendPath(), { who: ADMIN, body: {} })).status, 405);
});

// -- the account's side -------------------------------------------------------------------------------------------------------------

test('every subscription route needs a signed-in account', async () => {
  const { env } = await environment();
  for (const method of ['GET', 'PUT', 'DELETE']) {
    const r = await call(env, method, sub());
    assert.equal(r.status, 401, method);
  }
  const forged = await call(env, 'GET', sub(), { headers: { Authorization: 'Bearer not.a.token' } });
  assert.equal(forged.status, 401);
});

test('an account whose email is not verified cannot subscribe: 409 email_not_verified, and nothing is stored', async () => {
  const { env } = await environment();
  const onFile = await account(env, 1, { verified: false });
  const noEmail = await account(env, 2, { email: null, verified: false });
  for (const id of [onFile, noEmail]) {
    const r = await subscribe(env, id);
    assert.equal(r.status, 409, id);
    assert.equal(r.body.code, 'email_not_verified');
    assert.equal(await subscribed(env, id), false);
    assert.deepEqual((await call(env, 'GET', sub(), { who: id })).body, { subscribed: false, email_verified: false });
  }
  assert.deepEqual(subscribers(env, ''), []);
});

test('a verified account subscribes: the status says so, and the store holds the time and nothing else, with no expiry', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  assert.deepEqual((await call(env, 'GET', sub(), { who: id })).body, { subscribed: false, email_verified: true });
  const puts = [];
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (key, value, options) => { puts.push([key, options]); return realPut(key, value, options); };
  const r = await subscribe(env, id);
  assert.equal(r.status, 200, r.text);
  assert.deepEqual(r.body, { ok: true, subscribed: true, email_verified: true });
  assert.deepEqual((await call(env, 'GET', sub(), { who: id })).body, { subscribed: true, email_verified: true });

  const stored = JSON.parse(await env.USERS.get(`relnotify:player:${id}`));
  assert.deepEqual(Object.keys(stored), ['at']);
  assert.match(stored.at, /^\d{4}-\d\d-\d\dT[\d:.]+Z$/);
  const write = puts.find(([key]) => key === `relnotify:player:${id}`);
  assert.ok(write, 'one write');
  assert.equal(write[1]?.expirationTtl, undefined, 'no expiry');
  assert.ok(!(await env.USERS.get(`relnotify:player:${id}`)).includes('@'), 'no copy of the address');
  // The other product is separate.
  assert.deepEqual((await call(env, 'GET', sub('studio'), { who: id })).body, { subscribed: false, email_verified: true });
});

test('subscribing again changes nothing: the first time stays and no second write is made', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  assert.equal((await subscribe(env, id)).status, 200);
  const first = await env.USERS.get(`relnotify:player:${id}`);
  const writes = [];
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (key, value, options) => { writes.push(key); return realPut(key, value, options); };
  const again = await subscribe(env, id);
  assert.equal(again.status, 200);
  assert.deepEqual(again.body, { ok: true, subscribed: true, email_verified: true });
  assert.equal(await env.USERS.get(`relnotify:player:${id}`), first);
  assert.deepEqual(writes.filter((k) => k.startsWith('relnotify')), []);
});

test('unsubscribing removes it, and unsubscribing again, or never having subscribed, is the same answer', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  const stranger = await account(env, 2, { verified: false });
  assert.deepEqual((await call(env, 'DELETE', sub(), { who: id })).body, { ok: true, subscribed: false }, 'never subscribed');
  await subscribe(env, id);
  await subscribe(env, id, 'studio');
  const r = await call(env, 'DELETE', sub(), { who: id });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, { ok: true, subscribed: false });
  assert.equal(await subscribed(env, id), false);
  assert.equal(await subscribed(env, id, 'studio'), true, 'only that product');
  assert.deepEqual((await call(env, 'DELETE', sub(), { who: id })).body, { ok: true, subscribed: false });
  assert.deepEqual((await call(env, 'DELETE', sub(), { who: stranger })).body, { ok: true, subscribed: false }, 'an account with no verified email can always leave');
  assert.deepEqual((await call(env, 'GET', sub(), { who: id })).body, { subscribed: false, email_verified: true });
});

test('a subscription is the caller\'s own: nothing in the request names another account', async () => {
  const { env } = await environment();
  const a = await account(env, 1);
  const b = await account(env, 2);
  await subscribe(env, a);
  assert.equal(await subscribed(env, b), false);
  // An id in the body, the path or a header is not read.
  const r = await call(env, 'PUT', sub(), { who: a, body: { account_id: b, id: b, user: b }, headers: { 'X-Account': b } });
  assert.equal(r.status, 200);
  assert.equal(await subscribed(env, b), false);
  await call(env, 'DELETE', sub(), { who: a, body: { account_id: b } });
  assert.equal(await subscribed(env, a), false);
});

test('an address in the request is never read, stored or answered', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  const hostile = { email: 'thief@evil.test', address: 'thief@evil.test', to: 'thief@evil.test', new_address: 'thief@evil.test' };
  const r = await call(env, 'PUT', sub(), { who: id, body: hostile });
  assert.equal(r.status, 200);
  assert.ok(!r.text.includes('@') && !r.text.includes('evil'));
  assert.deepEqual(Object.keys(JSON.parse(await env.USERS.get(`relnotify:player:${id}`))), ['at']);
  const unverified = await account(env, 2, { verified: false });
  assert.equal((await call(env, 'PUT', sub(), { who: unverified, body: hostile })).status, 409, 'an address in the body does not make an account verified');
  // Even a body that is not JSON changes nothing.
  assert.equal((await call(env, 'PUT', sub(), { who: id, body: '{not json', headers: { 'Content-Type': 'application/json' } })).status, 200);

  const s = await send(env);
  assert.equal(s.status, 200);
  assert.deepEqual(sent.map((m) => m.to), ['user1@example.com'], 'the notice goes to the address verified on the account, whatever the request said');
});

test('the status never carries an address, and a subscribed account whose email stops being verified says so', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  await unverify(env, id);
  const r = await call(env, 'GET', sub(), { who: id });
  assert.deepEqual(r.body, { subscribed: true, email_verified: false });
  assert.ok(!r.text.includes('@'));
});

test('writes are rate limited per account like the other account writes; a read is not', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  const asked = [];
  let allow = true;
  env.AUTH_RATE_LIMITER = { limit: async ({ key }) => { asked.push(key); return { success: allow }; } };
  assert.equal((await subscribe(env, id)).status, 200);
  assert.equal((await call(env, 'DELETE', sub(), { who: id })).status, 200);
  assert.deepEqual(asked, [`release-notify:${id}`, `release-notify:${id}`]);
  allow = false;
  for (const method of ['PUT', 'DELETE']) {
    const r = await call(env, method, sub(), { who: id });
    assert.equal(r.status, 429, method);
    assert.equal(r.body.code, 'rate_limited');
    assert.equal(r.headers.get('Retry-After'), '60');
  }
  assert.equal(await subscribed(env, id), false, 'the refused PUT subscribed nobody');
  asked.length = 0;
  assert.equal((await call(env, 'GET', sub(), { who: id })).status, 200);
  assert.deepEqual(asked, [], 'a read asks nothing');
});

test('a deployment with no account objects says it is not configured, and an account object that fails is a 503', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  const bare = { ...env, ACCOUNT_AUTH: undefined };
  for (const method of ['GET', 'PUT', 'DELETE']) assert.equal((await call(bare, method, sub(), { who: id })).status, 501, method);
  const broken = { ...env, ACCOUNT_AUTH: { idFromName: (n) => n, get: () => ({ fetch: () => { throw new Error('object is down'); } }) } };
  const real = console.error;
  console.error = () => {};
  try {
    for (const method of ['GET', 'PUT']) {
      const r = await call(broken, method, sub(), { who: id });
      assert.equal(r.status, 503, method);
      assert.equal(r.body.code, 'account_unavailable');
    }
  } finally { console.error = real; }
  assert.equal(await subscribed(env, id), false);
});

// -- the administrator's send: who and what -------------------------------------------------------------------------------------------------

test('only an administrator may send: anyone else is refused with 403, and nothing is sent or removed', async () => {
  const { env, sent } = await environment();
  const a = await account(env, 1);
  const b = await account(env, 2);
  await subscribe(env, a);
  await subscribe(env, b);
  for (const who of [a, null]) {
    const r = await send(env, {}, { who });
    assert.equal(r.status, 403, String(who));
    assert.equal(r.body.error, 'Admin access required');
  }
  const forged = await call(env, 'POST', sendPath(), { headers: { Authorization: 'Bearer forged.token.here' }, body: { version: '0.1.0', url: URL_OK } });
  assert.equal(forged.status, 403);
  assert.equal(sent.length, 0);
  assert.deepEqual(subscribers(env).sort(), [`relnotify:player:${a}`, `relnotify:player:${b}`].sort());
  assert.deepEqual(await audits(env), [], 'a refused request is not a send');
  // A role on the record counts, the same predicate as every admin route.
  await env.USERS.put(`user:${a}`, JSON.stringify({ kyc_verified: true, ...JSON.parse(await env.USERS.get(`user:${a}`)), role: 'admin' }));
  assert.equal((await send(env, { dry_run: true }, { who: a })).status, 200);
});

test('the version is a short semver-like string and the link is https on one of two hosts, or the request is refused whole', async () => {
  const { env, sent } = await environment();
  const a = await account(env, 1);
  await subscribe(env, a);
  assert.deepEqual([...RELEASE_LINK_HOSTS], ['downloads.eustress.dev', 'eustress.dev']);
  for (const version of ['1.0', 'v1.0.0', '1.0.0.0', '1.0.0; DROP', '', ' 1.0.0', '1.0.0 ', '1.0.0\n', '1.0.0-', '1.0.0-' + 'a'.repeat(21), 'x'.repeat(60), 5, null, undefined, {}, ['1.0.0'], '1.0.0+build', '12345.0.0', '-1.0.0']) {
    const r = await call(env, 'POST', sendPath(), { who: ADMIN, body: { version, url: URL_OK } });
    assert.equal(r.status, 400, JSON.stringify(version));
    assert.equal(r.body.code, 'bad_version', JSON.stringify(version));
  }
  for (const version of ['0.1.0', '1.0.0', '10.20.30', '1.0.0-alpha', '1.0.0-alpha.1', '0.0.1-rc1', '9999.9999.9999']) assert.match(version, VERSION_PATTERN, version);

  const bad = [
    'http://downloads.eustress.dev/player', 'https://evil.example/player', 'https://eustress.dev.evil.example/x', 'https://downloads.eustress.dev.evil.example/x',
    'https://evil.example/https://eustress.dev', 'https://eustress.dev@evil.example/x', 'https://user:pw@eustress.dev/x', 'https://eustress.dev:8443/x', 'https://eustress.dev:80/x',
    'https://www.eustress.dev/x', 'https://evil.eustress.dev/x', 'https://downloads.eustress.dev.', 'javascript:alert(1)', 'data:text/html,hi', '//eustress.dev/x', '/player', 'eustress.dev/x',
    'https://eustress.dev/a b', 'https://eustress.dev/\r\nBcc: x@y.zz', 'ftp://eustress.dev/x', '', ' https://eustress.dev/x', 'https://eustress.dev/' + 'a'.repeat(300), 5, null, undefined, {}, ['https://eustress.dev/x'],
    'https://127.0.0.1/x', 'https://[::1]/x', 'https://eustress.dev%2f@evil.example/',
    // each rule on its own, on the host that has no other rule to stop it: a name or password, a port, a space or control character, a length
    'https://user:pw@downloads.eustress.dev/player', 'https://user@eustress.dev/download/x', 'https://:pw@downloads.eustress.dev/p',
    'https://downloads.eustress.dev:8443/player', 'https://eustress.dev:8443/download/x', 'https://downloads.eustress.dev:80/player',
    'https://downloads.eustress.dev/a b', 'https://downloads.eustress.dev/a' + String.fromCharCode(9) + 'b', 'https://downloads.eustress.dev/a' + String.fromCharCode(10) + 'b',
    'https://downloads.eustress.dev/a' + String.fromCharCode(0) + 'b', 'https://downloads.eustress.dev/a' + String.fromCharCode(127) + 'b', 'https://downloads.eustress.dev/a' + String.fromCharCode(133) + 'b',
    'https://downloads.eustress.dev/' + 'a'.repeat(300), 'https://eustress.dev/download/' + 'a'.repeat(300),
    // a query or a fragment, on either host: `?to=` on a page of ours is a redirect somebody else can aim
    'https://eustress.dev/redirect?to=https://evil.example', 'https://eustress.dev/download?to=https://evil.example', 'https://downloads.eustress.dev/player?x=1', 'https://downloads.eustress.dev/player?',
    'https://downloads.eustress.dev/player#top', 'https://eustress.dev/download#x', 'https://eustress.dev/download/?', 'https://downloads.eustress.dev/?', 'https://downloads.eustress.dev/#',
    // on the site, a path that is not a download page
    'https://eustress.dev', 'https://eustress.dev/', 'https://eustress.dev/redirect', 'https://eustress.dev/gallery', 'https://eustress.dev/downloadsx', 'https://eustress.dev/download-x', 'https://eustress.dev/Download',
    'https://eustress.dev/download/../redirect', 'https://eustress.dev/downloads/../x', 'https://eustress.dev/%2e%2e/redirect', 'https://eustress.dev/download/%2e%2e/redirect', 'https://eustress.dev/x/download',
  ];
  for (const url of bad) {
    const r = await call(env, 'POST', sendPath(), { who: ADMIN, body: { version: '0.1.0', url } });
    assert.equal(r.status, 400, String(JSON.stringify(url)).slice(0, 60));
    assert.equal(r.body.code, 'bad_url', String(JSON.stringify(url)).slice(0, 60));
  }
  assert.equal(sent.length, 0);
  assert.equal(await subscribed(env, a), true);
  const good = [
    'https://downloads.eustress.dev/player/latest', URL_OK, 'https://downloads.eustress.dev', 'https://downloads.eustress.dev/', 'https://downloads.eustress.dev/any/path/at/all.msi',
    'https://eustress.dev/download', 'https://eustress.dev/download/', 'https://eustress.dev/download/player', 'https://eustress.dev/downloads', 'https://eustress.dev/downloads/', 'https://eustress.dev/downloads/player/0.1.0',
  ];
  for (const url of good) assert.equal(checkReleaseUrl(url).ok, true, url);
  assert.equal(checkReleaseUrl('https://DOWNLOADS.EUSTRESS.DEV/Player').url, 'https://downloads.eustress.dev/Player', 'the link is the address as it is read');
  assert.equal(checkReleaseUrl('https://eustress.dev:443/download/x').url, 'https://eustress.dev/download/x', 'the default https port is no port at all');
});

test('a body that is not a JSON object, a limit out of range, a cursor that is not text and a dry_run that is not a flag are refused', async () => {
  const { env } = await environment();
  for (const body of ['not json', '[]', 'null', '7', '"text"']) {
    const r = await call(env, 'POST', sendPath(), { who: ADMIN, body });
    assert.equal(r.status, 400, body);
    assert.equal(r.body.code, 'bad_request', body);
  }
  const noBody = await call(env, 'POST', sendPath(), { who: ADMIN });
  assert.equal(noBody.status, 400);
  for (const limit of [0, -1, SEND_PAGE_MAX + 1, 1.5, '5', null, NaN, true]) {
    const r = await send(env, { limit });
    assert.equal(r.status, 400, JSON.stringify(limit));
    assert.equal(r.body.code, 'bad_limit');
  }
  for (const cursor of [7, [], {}, '', 'x'.repeat(2000), true]) {
    const r = await send(env, { cursor });
    assert.equal(r.status, 400, JSON.stringify(cursor).slice(0, 30));
    assert.equal(r.body.code, 'bad_cursor');
  }
  for (const dry_run of ['yes', 1, 0, 'true', {}, []]) {
    const r = await send(env, { dry_run });
    assert.equal(r.status, 400, JSON.stringify(dry_run));
    assert.equal(r.body.code, 'bad_request');
  }
});

test('with no mail binding a send is refused before it touches a subscriber, and a dry run still works', async () => {
  const { env } = await environment({ extra: { EMAIL: undefined } });
  const a = await account(env, 1);
  await subscribe(env, a);
  const r = await send(env);
  assert.equal(r.status, 501);
  assert.equal(r.body.code, 'mail_not_configured');
  assert.equal(await subscribed(env, a), true);
  const dry = await send(env, { dry_run: true });
  assert.equal(dry.status, 200);
  assert.equal(dry.body.would_send, 1);
});

// -- the send ------------------------------------------------------------------------------------------------------------------------------

test('each subscriber is emailed once, at the address verified on their account, and then the subscription is gone', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3)];
  for (const id of ids) await subscribe(env, id);
  const outsider = await account(env, 4);
  await subscribe(env, outsider, 'studio');

  const r = await send(env);
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.ok, true);
  assert.deepEqual([r.body.sent, r.body.skipped_unverified, r.body.failed, r.body.already_sent], [3, 0, 0, 0]);
  assert.equal(r.body.done, true);
  assert.equal(r.body.next_cursor, null);
  assert.equal(r.body.dry_run, false);
  assert.deepEqual(sent.map((m) => m.to).sort(), ['user1@example.com', 'user2@example.com', 'user3@example.com']);
  assert.deepEqual(subscribers(env), [], 'every subscription was ended by its email');
  assert.equal(await subscribed(env, outsider, 'studio'), true, 'another product is not touched');

  // Run again: nobody is emailed twice.
  const again = await send(env);
  assert.deepEqual([again.body.sent, again.body.skipped_unverified, again.body.failed], [0, 0, 0]);
  assert.equal(sent.length, 3);
  // The account may ask again later, and a new version reaches them; the same version does not.
  await subscribe(env, ids[0]);
  const sameVersion = await send(env);
  assert.deepEqual([sameVersion.body.sent, sameVersion.body.already_sent], [0, 1], 'the same address, the same version');
  assert.equal(sent.length, 3);
  assert.equal(await subscribed(env, ids[0]), false, 'and its request is ended');
  await subscribe(env, ids[0]);
  const next = await send(env, { version: '0.2.0' });
  assert.equal(next.body.sent, 1);
  assert.equal(sent.length, 4);
});

test('the email says what it is, who it is for and that it is sent once; it carries the one link and nothing that tracks', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  await send(env, { url: 'https://downloads.eustress.dev/player/0.1.0/get&a=1' });
  assert.equal(sent.length, 1);
  const message = sent[0];
  const { text, html } = parts(message);
  assert.equal(header(message, 'To'), 'user1@example.com');
  assert.match(header(message, 'From'), /^Eustress <identity@eustress\.dev>$/, 'a release notice is not mail from Eustress Security');
  assert.equal(header(message, 'Subject'), 'Eustress Player 0.1.0 is available');
  const link = 'https://downloads.eustress.dev/player/0.1.0/get&a=1';
  assert.ok(text.includes(link), 'the plain text carries the link');
  assert.match(text, /Eustress Player 0\.1\.0 is available/);
  assert.match(text, /one-time notice/);
  assert.match(text, /verified email address of the Eustress account that asked/);
  assert.match(text, /sent once/);
  assert.match(text, /If you did not ask for it, ignore it/);
  assert.ok(html.includes('Eustress Player 0.1.0 is available'));
  assert.ok(html.includes('one-time notice'));
  // The links in the HTML: the button, and no other (the plain footer has none).
  const hrefs = [...html.matchAll(/<a\b[^>]*\shref="([^"]*)"/g)].map((m) => m[1].replace(/&amp;/g, '&'));
  assert.deepEqual(hrefs, [link]);
  assert.equal([...html.matchAll(/<img\b/g)].length, 1, 'one image: the header gear, no pixel');
  assert.ok(!/<script|onerror|onload|beacon|utm_|track|pixel/i.test(html));
  assert.ok(!/<(link|iframe|object|embed|form)\b/i.test(html));
  assert.ok(!html.includes('eustress.dev/gallery'), 'the plain footer carries no site links');
  assert.ok(!/user1@example\.com|maker1/.test(html + text), 'no address and no name in the body');
  assert.ok(!html.includes('<!DOCTYPE') || html.includes('eustress-email:body'), 'built in the shell');
});

test('the email for Studio names Studio, and a product that is not one cannot be rendered', () => {
  const studio = renderReleaseEmail({ product: 'studio', version: '1.2.3-rc1', url: URL_OK });
  assert.equal(studio.subject, 'Eustress Studio 1.2.3-rc1 is available');
  assert.match(studio.text, /Eustress Studio/);
  assert.ok(!/Eustress Player/.test(studio.text + studio.html));
  assert.throws(() => renderReleaseEmail({ product: 'launcher', version: '1.0.0', url: URL_OK }));
  assert.throws(() => renderReleaseEmail({ product: 'constructor', version: '1.0.0', url: URL_OK }));
  const hostile = renderReleaseEmail({ product: 'player', version: '1.0.0', url: 'https://eustress.dev/x?"><script>alert(1)</script>' });
  assert.ok(!hostile.html.includes('<script>'), 'a value is escaped in the HTML even if the route would have refused it');
});

test('one failure does not stop the run: that subscriber keeps their subscription, the others are mailed and ended', async () => {
  const { env, sent } = await environment({ failFor: new Set(['user2@example.com']) });
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3)];
  for (const id of ids) await subscribe(env, id);
  const real = console.error;
  console.error = () => {};
  let r;
  try { r = await send(env); } finally { console.error = real; }
  assert.equal(r.status, 200);
  assert.deepEqual([r.body.sent, r.body.skipped_unverified, r.body.failed], [2, 0, 1]);
  assert.deepEqual(sent.map((m) => m.to).sort(), ['user1@example.com', 'user3@example.com']);
  assert.deepEqual(subscribers(env), [`relnotify:player:${ids[1]}`], 'only the one that failed is left');

  // The next run reaches only that one, once the mail works.
  env.EMAIL = { send: async (m) => { sent.push(m); } };
  const retry = await send(env);
  assert.deepEqual([retry.body.sent, retry.body.failed], [1, 0]);
  assert.equal(sent.length, 3);
  assert.deepEqual(sent.map((m) => m.to).sort(), ['user1@example.com', 'user2@example.com', 'user3@example.com']);
  assert.deepEqual(subscribers(env), []);
});

test('a mail binding that throws for everyone fails everyone, loses nobody, and answers 200 with the counts', async () => {
  const { env } = await environment({ failFor: new Set(['user1@example.com', 'user2@example.com']) });
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  const real = console.error;
  console.error = () => {};
  let r;
  try { r = await send(env); } finally { console.error = real; }
  assert.equal(r.status, 200);
  assert.deepEqual([r.body.sent, r.body.failed], [0, 2]);
  assert.equal(subscribers(env).length, 2);
});

test('a subscriber whose email is no longer verified is skipped and kept, and reached once it is verified again', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  await unverify(env, ids[0]);
  const r = await send(env);
  assert.deepEqual([r.body.sent, r.body.skipped_unverified, r.body.failed], [1, 1, 0]);
  assert.deepEqual(sent.map((m) => m.to), ['user2@example.com']);
  assert.deepEqual(subscribers(env), [`relnotify:player:${ids[0]}`], 'kept');
  assert.deepEqual((await call(env, 'GET', sub(), { who: ids[0] })).body, { subscribed: true, email_verified: false });

  await verifyEmail(env, ids[0]);
  const next = await send(env);
  assert.deepEqual([next.body.sent, next.body.skipped_unverified], [1, 0]);
  assert.deepEqual(sent.map((m) => m.to).sort(), ['user1@example.com', 'user2@example.com']);
});

test('the address is read when the notice is sent: an account whose address on file changed gets nothing at the old one', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  // The address on file moves to one that is not verified (the object's own state, as a completed change leaves it).
  const object = env.ACCOUNT_AUTH.instances.get(id);
  const state = await object.storage.get('state');
  state.email_on_file = 'new@example.com';
  await object.storage.put('state', state);
  const r = await send(env);
  assert.deepEqual([r.body.sent, r.body.skipped_unverified], [0, 1], 'the verified address is no longer the one on file');
  assert.equal(sent.length, 0);
});

test('a verified flag with no usable address behind it is not a verified email, and an object that cannot be read is not an answer', async () => {
  const objectSaying = (email) => ({ USERS: { get: async () => '{}' }, ACCOUNT_AUTH: { idFromName: (n) => n, get: () => ({ fetch: async (url) => new Response(JSON.stringify(String(url).endsWith('/state') ? { ok: true, seeded: true } : { ok: true, email })) }) } });
  const id = accountId(1);
  assert.deepEqual(await emailStatusOf(objectSaying({ verified: true, on_file: 'user1@example.com' }), id), { ok: true, verified: true, address: 'user1@example.com' });
  assert.deepEqual(await emailStatusOf(objectSaying({ verified: true, on_file: ' User1@Example.COM ' }), id), { ok: true, verified: true, address: 'user1@example.com' }, 'the address as the email routes clean it');
  for (const on_file of [null, undefined, '', 'not an address', 'a@b', 'x@y.zz' + String.fromCharCode(13, 10) + 'Bcc: z@y.zz']) {
    const r = await emailStatusOf(objectSaying({ verified: true, on_file }), id);
    assert.equal(r.verified, false, JSON.stringify(on_file));
  }
  assert.equal((await emailStatusOf(objectSaying({ verified: false, on_file: 'user1@example.com' }), id)).verified, false);
  assert.equal((await emailStatusOf(objectSaying({ on_file: 'user1@example.com' }), id)).verified, false, 'no flag, not verified');
  assert.equal((await emailStatusOf(objectSaying({ verified: 'yes', on_file: 'user1@example.com' }), id)).verified, false, 'only true is true');
  assert.equal((await emailStatusOf({ USERS: { get: async () => '{}' } }, id)).ok, false, 'no account objects');
  // Only a user record that is not there is a definite not-found; the object saying no, or failing, is not.
  const gone = await emailStatusOf({ ...objectSaying({}), USERS: { get: async () => null } }, id);
  assert.deepEqual([gone.ok, gone.gone], [false, true]);
  const down = await emailStatusOf({ USERS: { get: async () => '{}' }, ACCOUNT_AUTH: { idFromName: (n) => n, get: () => ({ fetch: () => { throw new Error('object is down'); } }) } }, id);
  assert.deepEqual([down.ok, !!down.gone], [false, false]);
  await assert.rejects(emailStatusOf({ USERS: { get: async () => { throw new Error('KV is down'); } } }, id), /KV is down/, 'a failed read is not an answer');
});

test('the notice is sent from the release sender if there is one, else the notice sender, else identity@eustress.dev', async () => {
  for (const [extra, expected] of [[{}, 'identity@eustress.dev'], [{ NOTICE_MAIL_FROM: 'notices@eustress.dev' }, 'notices@eustress.dev'], [{ NOTICE_MAIL_FROM: 'notices@eustress.dev', RELEASE_MAIL_FROM: 'releases@eustress.dev' }, 'releases@eustress.dev']]) {
    const { env, sent } = await environment({ extra });
    const id = await account(env, 1);
    await subscribe(env, id);
    assert.equal((await send(env)).body.sent, 1);
    assert.equal(header(sent[0], 'From'), `Eustress <${expected}>`);
    assert.equal(sent[0].from, expected);
  }
});

test('a subscription whose account is definitely gone is removed and counted; one that fails for any other reason is a failure and is kept', async () => {
  const { env, sent } = await environment();
  const ok = await account(env, 1);
  await subscribe(env, ok);
  // An account that was deleted: its user record is gone (its object may still be there).
  const deleted = await account(env, 2);
  await subscribe(env, deleted);
  await env.USERS.delete(`user:${deleted}`);
  // An id nobody ever held.
  const never = accountId(77);
  await env.USERS.put(subscriptionKey('player', never), JSON.stringify({ at: '2026-10-01T00:00:00.000Z' }));
  await env.USERS.put('relnotify:player:', '{}');
  const dry = await send(env, { dry_run: true });
  assert.deepEqual([dry.body.would_remove_orphans, dry.body.orphans_removed, dry.body.would_send], [2, 0, 1]);
  assert.equal(await subscribed(env, deleted), true, 'a dry run removes nothing');
  const r = await send(env);
  assert.deepEqual([r.body.sent, r.body.orphans_removed, r.body.failed], [1, 2, 0]);
  assert.deepEqual(sent.map((m) => m.to), ['user1@example.com'], 'a deleted account is not emailed');
  assert.equal(await subscribed(env, deleted), false);
  assert.equal(await subscribed(env, never), false);
  assert.equal(await env.USERS.get('relnotify:player:'), '{}', 'a key with no account in it is not an account to remove');
  assert.deepEqual([...env.SCENES.objects.keys()].filter((k) => k.startsWith('locks/release-notify/')).length, 1, 'no claim for an account that is gone');
  assert.equal((await send(env)).body.orphans_removed, 0, 'and it is not counted again');
});

test('a lookup that fails for any reason but a definite not-found keeps the subscription: an object that is down, a KV that fails, an account that cannot be read', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  const real = console.error;
  console.error = () => {};
  try {
    // The account's object is down (only this account's: the administrator's own gate has to work to run the send at all).
    const realObjects = env.ACCOUNT_AUTH;
    const objectsWhere = (stub) => ({ instances: realObjects.instances, idFromName: realObjects.idFromName, get: (name) => (name === id ? stub : realObjects.get(name)) });
    const downEnv = { ...env, ACCOUNT_AUTH: objectsWhere({ fetch: () => { throw new Error('object is down'); } }) };
    const a = await send(downEnv);
    assert.deepEqual([a.body.failed, a.body.orphans_removed, a.body.sent], [1, 0, 0]);
    // The KV read of the user record fails.
    const realGet = env.USERS.get.bind(env.USERS);
    env.USERS.get = async (key) => { if (key === `user:${id}`) throw new Error('KV read failed'); return realGet(key); };
    const b = await send(env);
    assert.deepEqual([b.body.failed, b.body.orphans_removed, b.body.sent], [1, 0, 0]);
    env.USERS.get = realGet;
    // The object answers with a failure that is not a not-found.
    const brokenEnv = { ...env, ACCOUNT_AUTH: objectsWhere({ fetch: async () => new Response(JSON.stringify({ ok: false, status: 500, code: 'bad_record' })) }) };
    const c = await send(brokenEnv);
    assert.deepEqual([c.body.failed, c.body.orphans_removed, c.body.sent], [1, 0, 0]);
  } finally { console.error = real; }
  assert.equal(await subscribed(env, id), true, 'kept through all of it');
  assert.equal(sent.length, 0);
  assert.equal((await send(env)).body.sent, 1, 'and reached once the lookup works');
});

test('a signed-in token for an account whose record is gone is told so, not 503', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  await env.USERS.delete(`user:${id}`);
  for (const method of ['GET', 'PUT']) {
    const r = await call(env, method, sub(), { who: id });
    assert.equal(r.status, 404, method);
    assert.equal(r.body.code, 'no_account');
  }
});

test('a dry run returns the counts and changes nothing: no mail, no ended subscription, no record of a send', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3)];
  for (const id of ids) await subscribe(env, id);
  await unverify(env, ids[2]);
  const before = new Map(env.USERS.store);
  const r = await send(env, { dry_run: true });
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.dry_run, true);
  assert.deepEqual([r.body.sent, r.body.would_send, r.body.skipped_unverified, r.body.failed], [0, 2, 1, 0]);
  assert.equal(sent.length, 0);
  assert.deepEqual([...env.USERS.store.keys()].filter((k) => k.startsWith('relnotify')).sort(), [...before.keys()].filter((k) => k.startsWith('relnotify')).sort());
  assert.equal(env.SCENES.objects.size, 0, 'no claim was made');
  assert.ok(![...env.USERS.store.keys()].some((k) => k.startsWith('mailcap')), 'no mail allowance was spent');
  // The real run after it sends what the dry run said it would.
  const live = await send(env);
  assert.deepEqual([live.body.sent, live.body.skipped_unverified], [2, 1]);

  // A dry run does not end the subscription of an address already told this version either.
  await subscribe(env, ids[0]);
  const dry = await send(env, { dry_run: true });
  assert.deepEqual([dry.body.already_sent, dry.body.sent], [1, 0]);
  assert.equal(await subscribed(env, ids[0]), true, 'a dry run ends nothing');
  assert.equal(sent.length, 2);
});

test('the daily mail allowance is the existing one: an account that has used it is failed and kept, the others are mailed', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  for (let i = 0; i < MAIL_PER_DAY.notice; i++) assert.equal(await takeMailSlot(env, 'user1@example.com', 'notice', ids[0]), true);
  const real = console.error;
  console.error = () => {};
  let r;
  try { r = await send(env); } finally { console.error = real; }
  assert.deepEqual([r.body.sent, r.body.failed], [1, 1]);
  assert.deepEqual(sent.map((m) => m.to), ['user2@example.com']);
  assert.equal(await subscribed(env, ids[0]), true);
  assert.equal(await subscribed(env, ids[1]), false);
});

test('one email per address per version: two accounts that name one address get one email, and both subscriptions end', async () => {
  const { env, sent } = await environment();
  const a = await account(env, 1, { email: 'shared@example.com' });
  const b = await account(env, 2, { email: 'Shared@Example.com' });
  await subscribe(env, a);
  await subscribe(env, b);
  const r = await send(env);
  assert.deepEqual([r.body.sent, r.body.already_sent], [1, 1]);
  assert.equal(sent.length, 1);
  assert.deepEqual(subscribers(env), []);
});

test('a listing that still shows a subscription which was just ended does not mail it again', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  assert.equal((await send(env)).body.sent, 2);
  // KV may go on listing a deleted key for a while: it is back, as a stale list would show it.
  for (const id of ids) await env.USERS.put(subscriptionKey('player', id), JSON.stringify({ at: '2026-10-01T00:00:00.000Z' }));
  const again = await send(env);
  assert.deepEqual([again.body.sent, again.body.already_sent], [0, 2]);
  assert.equal(sent.length, 2);
  assert.deepEqual(subscribers(env), []);
});

test('no address is in any response, in a subscription, in the record of a send or in the audit entry', async () => {
  const { env } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  const seen = [];
  for (const id of ids) seen.push((await subscribe(env, id)).text, (await call(env, 'GET', sub(), { who: id })).text);
  seen.push((await send(env, { dry_run: true })).text, (await send(env)).text, (await send(env)).text);
  seen.push((await call(env, 'DELETE', sub(), { who: ids[0] })).text);
  for (const text of seen) assert.ok(!/@|example\.com/.test(text), text.slice(0, 120));
  const mine = [...env.USERS.store.entries()].filter(([k]) => k.startsWith('relnotify'));
  for (const [key, value] of mine) assert.ok(!/@|example\.com/.test(`${key} ${value}`), key);
  const claims = [...env.SCENES.objects.entries()].filter(([k]) => k.startsWith('locks/release-notify/'));
  assert.equal(claims.length, 2, 'the record of each send is there to look at');
  for (const [key, object] of claims) {
    assert.match(key, /^locks\/release-notify\/player\/0\.1\.0\/[0-9a-f]{32}\.json$/);
    assert.ok(!/@|example\.com/.test(`${key} ${new TextDecoder().decode(object.body)}`), key);
  }
  const entries = await audits(env);
  assert.ok(entries.length >= 3);
  assert.ok(!/@|example\.com/.test(JSON.stringify(entries)));
});

// -- the audit log --------------------------------------------------------------------------------------------------------------------------

test('every send is written to the audit log with who, what and how many, and a dry run is marked as one', async () => {
  const { env } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  await unverify(env, ids[1]);
  await send(env, { dry_run: true });
  await send(env, { version: '0.1.0' });
  const entries = (await audits(env)).sort((a, b) => (a.details.dry_run ? -1 : 1));
  assert.equal(entries.length, 2);
  for (const entry of entries) {
    assert.equal(entry.admin_id, ADMIN);
    assert.equal(entry.target, 'player');
    assert.equal(entry.details.version, '0.1.0');
    assert.equal(entry.details.url, URL_OK);
    assert.ok(entry.timestamp);
  }
  assert.deepEqual([entries[0].details.dry_run, entries[0].details.would_send, entries[0].details.sent], [true, 1, 0]);
  assert.deepEqual([entries[1].details.dry_run, entries[1].details.sent, entries[1].details.skipped_unverified], [false, 1, 1]);
});

test('an audit log that cannot be written does not undo a send that was made', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  env.AUDIT_LOG.put = async () => { throw new Error('audit store is down'); };
  const real = console.error;
  console.error = () => {};
  let r;
  try { r = await send(env); } finally { console.error = real; }
  assert.equal(r.status, 200);
  assert.equal(r.body.sent, 1);
  assert.equal(sent.length, 1);
});

test('a listing of subscribers that fails stops the send with a 500 that says so, and is still audited', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  const realList = env.USERS.list.bind(env.USERS);
  env.USERS.list = async () => { throw new Error('list is down'); };
  const real = console.error;
  console.error = () => {};
  let r;
  try { r = await send(env); } finally { console.error = real; env.USERS.list = realList; }
  assert.equal(r.status, 500);
  assert.equal(r.body.code, 'send_failed');
  assert.equal(await subscribed(env, id), true);
  const [entry] = await audits(env);
  assert.match(entry.details.error, /list is down/);
});

// -- paging -----------------------------------------------------------------------------------------------------------------------------------

/// KV as the platform pages it: a list is at most 1000 keys, with a cursor that names where to go on from.
function pagedKv() {
  const kv = fakeKv();
  kv.calls = [];
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    kv.calls.push({ prefix, limit, cursor });
    const rest = [...kv.store.keys()].filter((k) => k.startsWith(prefix) && (!cursor || k > cursor)).sort();
    const keys = rest.slice(0, Math.min(limit, 1000));
    const complete = rest.length <= keys.length;
    return { keys: keys.map((name) => ({ name })), list_complete: complete, ...(complete ? {} : { cursor: keys[keys.length - 1] }) };
  };
  return kv;
}

/// A handler call that does not go through the Worker, with the account lookup replaced.
function directRun({ env, status, audit = [] }) {
  const json = (value, code = 200, headers = {}) => new Response(JSON.stringify(value), { status: code, headers: { ...headers, 'Content-Type': 'application/json' } });
  const deps = { verifyAuth: async () => ADMIN, requireAdmin: async () => ADMIN, json, auditLog: async (...args) => { audit.push(args); }, emailStatus: status };
  return async (body, product = 'player') => {
    const url = new URL(`https://api.eustress.dev${sendPath(product)}`);
    const res = await handleReleaseNotify(new Request(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ version: '0.1.0', url: URL_OK, ...body }) }), url, env, {}, deps);
    return { status: res.status, body: await res.json() };
  };
}

/// An environment for a handler call that does not go through the Worker: paged KV, a bucket for the claims, a secret, a mail binding.
const bareEnv = (sent) => ({ USERS: pagedKv(), SCENES: fakeBucket(), AUDIT_LOG: fakeKv(), JWT_SECRET: 'test-secret-not-real', EMAIL: { send: async (m) => { sent.push(m.to); } }, __EmailMessage: StubMessage, ACCOUNT_AUTH: {} });

test('a send pages through more than a thousand subscribers by cursor: each is mailed once, in bounded requests, and none is skipped or repeated', { timeout: 120_000 }, async () => {
  const sent = [];
  const env = bareEnv(sent);
  const N = 2500;
  for (let n = 1; n <= N; n++) await env.USERS.put(subscriptionKey('player', accountId(n)), JSON.stringify({ at: '2026-10-01T00:00:00.000Z' }));
  const audit = [];
  const run = directRun({ env, status: async (_env, id) => ({ ok: true, verified: true, address: `u${parseInt(id.slice(-12), 16)}@example.com` }), audit });

  let cursor;
  let calls = 0;
  let total = 0;
  const cursors = [];
  for (;;) {
    const r = await run({ limit: SEND_PAGE_MAX, ...(cursor ? { cursor } : {}) });
    assert.equal(r.status, 200);
    calls += 1;
    total += r.body.sent;
    assert.ok(r.body.sent <= SEND_PAGE_MAX, 'a request does bounded work');
    if (r.body.done) { assert.equal(r.body.next_cursor, null); break; }
    assert.ok(r.body.next_cursor, 'more to do: the answer says where to go on');
    assert.notEqual(r.body.next_cursor, cursor);
    cursors.push(r.body.next_cursor);
    cursor = r.body.next_cursor;
    assert.ok(calls < 100, 'it ends');
  }
  assert.equal(total, N);
  assert.equal(sent.length, N);
  assert.equal(new Set(sent).size, N, 'no address twice');
  assert.equal(calls, Math.ceil(N / SEND_PAGE_MAX));
  assert.equal([...env.SCENES.objects.keys()].filter((k) => k.startsWith('locks/release-notify/player/0.1.0/')).length, N, 'one claim for each address, every one confirmed as sent');
  assert.equal(env.USERS.calls.length, calls);
  assert.deepEqual(env.USERS.calls.map((c) => c.cursor), [undefined, ...cursors], 'each call went on from the cursor the last one gave');
  assert.ok(env.USERS.calls.every((c) => c.prefix === 'relnotify:player:' && c.limit === SEND_PAGE_MAX));
  assert.equal([...env.USERS.store.keys()].filter((k) => k.startsWith('relnotify:player:')).length, 0, 'every subscription was ended');
  assert.equal(audit.length, calls, 'one audit entry per request');
  assert.equal(audit.at(-1)[4].done, true);
});

test('a page of subscribers that are skipped does not hold the run back: it goes on past them by cursor', { timeout: 120_000 }, async () => {
  const sent = [];
  const env = bareEnv(sent);
  const N = 1250;
  for (let n = 1; n <= N; n++) await env.USERS.put(subscriptionKey('player', accountId(n)), '{}');
  // The first 1100 are not verified; the rest are.
  const run = directRun({ env, status: async (_env, id) => { const n = parseInt(id.slice(-12), 16); return { ok: true, verified: n > 1100, address: `u${n}@example.com` }; } });
  let cursor;
  let skipped = 0;
  for (let i = 0; i < 40; i++) {
    const r = await run({ limit: SEND_PAGE_MAX, ...(cursor ? { cursor } : {}) });
    skipped += r.body.skipped_unverified;
    if (r.body.done) break;
    cursor = r.body.next_cursor;
  }
  assert.equal(skipped, 1100);
  assert.equal(sent.length, 150);
  assert.equal([...env.USERS.store.keys()].filter((k) => k.startsWith('relnotify:player:')).length, 1100, 'the skipped ones are kept');
});

test('the default is one page of 50 and no more than 60 is allowed in a request', async () => {
  assert.equal(SEND_PAGE_DEFAULT, 50);
  assert.equal(SEND_PAGE_MAX, 60);
  const sent = [];
  const env = bareEnv(sent);
  for (let n = 1; n <= 120; n++) await env.USERS.put(subscriptionKey('player', accountId(n)), '{}');
  const run = directRun({ env, status: async (_e, id) => ({ ok: true, verified: true, address: `u${parseInt(id.slice(-12), 16)}@example.com` }) });
  const first = await run({});
  assert.equal(first.body.sent, 50);
  assert.equal(first.body.done, false);
  assert.equal(env.USERS.calls[0].limit, 50);
  const second = await run({ cursor: first.body.next_cursor, limit: SEND_PAGE_MAX });
  assert.equal(second.body.sent, 60);
  assert.equal(second.body.done, false);
  const rest = await run({ cursor: second.body.next_cursor, limit: SEND_PAGE_MAX });
  assert.equal(rest.body.sent, 10);
  assert.equal(rest.body.done, true);
  assert.equal(sent.length, 120);
});

test('one address is told once per product and version: the player notice does not use up the studio one', async () => {
  const { env, sent } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id, 'player');
  await subscribe(env, id, 'studio');
  assert.equal((await send(env, {}, { product: 'player' })).body.sent, 1);
  assert.equal((await send(env, {}, { product: 'studio' })).body.sent, 1, 'the same version number, another product');
  assert.deepEqual(sent.map((m) => header(m, 'Subject')).sort(), ['Eustress Player 0.1.0 is available', 'Eustress Studio 0.1.0 is available']);
  assert.deepEqual(subscribers(env, ''), []);
});

test('a cursor continues only the product it was asked for, and one product\'s subscribers are never mailed for another', async () => {
  const { env, sent } = await environment();
  const a = await account(env, 1);
  const b = await account(env, 2);
  await subscribe(env, a, 'player');
  await subscribe(env, b, 'studio');
  const r = await send(env, {}, { product: 'studio' });
  assert.equal(r.body.sent, 1);
  assert.deepEqual(sent.map((m) => m.to), ['user2@example.com']);
  assert.match(header(sent[0], 'Subject'), /Eustress Studio/);
  assert.equal(await subscribed(env, a, 'player'), true);
});

// -- at most once: the claim -------------------------------------------------------------------------------------------------------------------

const claimKeysOf = (env) => [...env.SCENES.objects.keys()].filter((k) => k.startsWith('locks/release-notify/'));
const claimOf = async (env, key) => JSON.parse(new TextDecoder().decode(env.SCENES.objects.get(key).body));
/// The clock a test moves. Date.now is the platform's, and the claim's age is read from it.
function clock() {
  const real = Date.now;
  let now = real();
  Date.now = () => now;
  return { advance: (ms) => { now += ms; }, now: () => now, restore: () => { Date.now = real; } };
}
const silently = async (fn) => {
  const real = console.error;
  console.error = () => {};
  try { return await fn(); } finally { console.error = real; }
};

test('sends that overlap over the same subscribers email each address once: a retry, a double click or three at once', async () => {
  const { env, sent } = await environment();
  const ids = [1, 2, 3].map((n) => account(env, n));
  for (const id of await Promise.all(ids)) await subscribe(env, id);
  const runs = await Promise.all([send(env), send(env), send(env)]);
  for (const r of runs) assert.equal(r.status, 200);
  assert.equal(sent.length, 3, 'three subscribers, three emails');
  assert.equal(new Set(sent.map((m) => m.to)).size, 3, 'no address twice');
  assert.equal(runs.reduce((sum, r) => sum + r.body.sent, 0), 3, 'between them, three sent');
  assert.equal(runs.reduce((sum, r) => sum + r.body.already_sent, 0), 6, 'the others found each address taken');
  assert.equal(runs.reduce((sum, r) => sum + r.body.failed + r.body.unconfirmed, 0), 0);
  assert.deepEqual(subscribers(env), [], 'and every subscription ended');
  assert.equal(claimKeysOf(env).length, 3);
  for (const key of claimKeysOf(env)) assert.equal((await claimOf(env, key)).state, 'sent');
});

test('the address is claimed before the mail can leave: a claim exists, holding no address, the moment the binding is called', async () => {
  const seen = [];
  const { env } = await environment();
  const real = env.EMAIL.send;
  env.EMAIL.send = async (m) => {
    const keys = claimKeysOf(env);
    seen.push({ keys, state: keys.length ? (await claimOf(env, keys[0])).state : null });
    return real(m);
  };
  const id = await account(env, 1);
  await subscribe(env, id);
  assert.equal((await send(env)).body.sent, 1);
  assert.equal(seen.length, 1);
  assert.equal(seen[0].keys.length, 1, 'the claim was there when the mail was handed over');
  assert.equal(seen[0].state, 'claiming');
  assert.match(seen[0].keys[0], /^locks\/release-notify\/player\/0\.1\.0\/[0-9a-f]{32}\.json$/);
  assert.equal((await claimOf(env, seen[0].keys[0])).state, 'sent', 'and it is rewritten as sent once the mail has gone');
  assert.ok(!/@|example\.com/.test(JSON.stringify(await claimOf(env, seen[0].keys[0]))));
});

test('a send that fails, or is refused by the daily allowance, gives its claim back, and the next run sends', async () => {
  const { env, sent } = await environment({ failFor: new Set(['user2@example.com']) });
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3)];
  for (const id of ids) await subscribe(env, id);
  for (let i = 0; i < MAIL_PER_DAY.notice; i++) assert.equal(await takeMailSlot(env, 'user3@example.com', 'notice', ids[2]), true);
  const first = await silently(() => send(env));
  assert.deepEqual([first.body.sent, first.body.failed], [1, 2]);
  assert.equal(claimKeysOf(env).length, 1, 'only the address that was mailed keeps a claim');
  assert.deepEqual(subscribers(env).sort(), [`relnotify:player:${ids[1]}`, `relnotify:player:${ids[2]}`].sort());
  env.EMAIL = { send: async (m) => { sent.push(m); } };
  // A new day's allowance.
  for (const key of [...env.USERS.store.keys()].filter((k) => k.startsWith('mailcap'))) await env.USERS.delete(key);
  const retry = await send(env);
  assert.deepEqual([retry.body.sent, retry.body.failed, retry.body.already_sent, retry.body.unconfirmed], [2, 0, 0, 0], 'both are reached on the next run, and the first is not mailed again');
  assert.deepEqual(sent.map((m) => m.to).sort(), ['user1@example.com', 'user2@example.com', 'user3@example.com']);
});

test('a claim that holds in another run is not sent over, and a claim that was never confirmed is counted as unconfirmed and is not treated as sent', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3), await account(env, 4)];
  for (const id of ids) await subscribe(env, id);
  const c = clock();
  try {
    const put = async (n, body, { raw = false } = {}) => env.SCENES.put(claimKey('player', '0.1.0', await claimHash(env, `user${n}@example.com`)), raw ? body : JSON.stringify(body));
    await put(1, { state: 'claiming', at: Date.now() - (CLAIM_STALE_MS + 1), run: 'dead-run' });   // a Worker died between claim and mail
    await put(2, { state: 'claiming', at: Date.now() - 1000, run: 'live-run' });                    // another run is sending it now
    await put(3, { state: 'sent', at: Date.now() - 86_400_000, run: 'earlier-run' });               // sent, and the subscription was left behind
    await put(4, 'not json at all', { raw: true });                                                  // a claim that cannot be read
    const dry = await send(env, { dry_run: true });
    assert.deepEqual([dry.body.unconfirmed, dry.body.already_sent, dry.body.would_send], [2, 2, 0], 'a dry run reads the claims and says so');

    const r = await send(env);
    assert.equal(r.status, 200, r.text);
    assert.deepEqual([r.body.sent, r.body.unconfirmed, r.body.already_sent, r.body.failed], [0, 2, 2, 0]);
    assert.equal(sent.length, 0, 'nothing is mailed over a claim');
    assert.deepEqual(subscribers(env).sort(), [`relnotify:player:${ids[0]}`, `relnotify:player:${ids[1]}`, `relnotify:player:${ids[3]}`].sort(),
      'the unconfirmed and the in-progress subscriptions are kept; only the one whose claim says sent is ended');
    // The claims were not changed by a run that found them.
    assert.equal((await claimOf(env, claimKeysOf(env)[0])).run === undefined, false);
    // Once time passes the in-progress claim is unconfirmed too, and still nobody is mailed.
    c.advance(CLAIM_STALE_MS);
    const later = await send(env);
    assert.deepEqual([later.body.unconfirmed, later.body.already_sent, later.body.sent], [3, 0, 0]);
    assert.equal(sent.length, 0);
  } finally { c.restore(); }
});

test('a claim is stale at exactly the limit and not a moment before', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  const c = clock();
  try {
    await env.SCENES.put(claimKey('player', '0.1.0', await claimHash(env, 'user1@example.com')), JSON.stringify({ state: 'claiming', at: Date.now() }));
    assert.equal(CLAIM_STALE_MS, 5 * 60 * 1000, 'a few minutes');
    c.advance(CLAIM_STALE_MS - 1);
    assert.deepEqual([(await send(env, { dry_run: true })).body.already_sent, (await send(env, { dry_run: true })).body.unconfirmed], [1, 0]);
    c.advance(1);
    assert.deepEqual([(await send(env, { dry_run: true })).body.already_sent, (await send(env, { dry_run: true })).body.unconfirmed], [0, 1]);
  } finally { c.restore(); }
});

test('the mail leaves once even when the confirmation and the end of the subscription BOTH fail: the claim stands, and no later run sends again', async () => {
  const { env, sent } = await environment();
  const ids = [await account(env, 1), await account(env, 2)];
  for (const id of ids) await subscribe(env, id);
  const realPut = env.SCENES.put.bind(env.SCENES);
  const realDelete = env.USERS.delete.bind(env.USERS);
  // A KV outage, or a burst of 429s: the records after the mail cannot be written.
  env.SCENES.put = async (key, value, meta) => { if (/"state":"sent"/.test(String(value))) throw new Error('R2 is down'); return realPut(key, value, meta); };
  env.USERS.delete = async (key) => { if (key.startsWith('relnotify:')) throw new Error('KV is down'); return realDelete(key); };
  const c = clock();
  try {
    const first = await silently(() => send(env));
    assert.deepEqual([first.body.sent, first.body.failed], [2, 0], 'the mail went, so it is sent');
    assert.equal(sent.length, 2);
    assert.equal(subscribers(env).length, 2, 'neither subscription could be ended');
    for (const key of claimKeysOf(env)) assert.equal((await claimOf(env, key)).state, 'claiming', 'and neither claim could be confirmed');
    // Run again straight away: the claims say another run is on it.
    const again = await silently(() => send(env));
    assert.deepEqual([again.body.sent, again.body.already_sent], [0, 2]);
    assert.equal(sent.length, 2, 'not mailed again');
    // Run again after the claims have gone stale: unconfirmed, counted, still not mailed.
    c.advance(CLAIM_STALE_MS + 1000);
    const later = await silently(() => send(env));
    assert.deepEqual([later.body.sent, later.body.unconfirmed, later.body.already_sent], [0, 2, 0]);
    assert.equal(sent.length, 2, 'at most once: a person looks at an unconfirmed claim');
  } finally { c.restore(); }
});

test('an address is named by a keyed hash: HMAC-SHA-256 under a key derived from the audit key, else the Worker secret, for this one purpose', async () => {
  const expected = (secret, address) => {
    const subkey = crypto.createHmac('sha256', secret).update('eustress-release-notify-v1').digest();
    return crypto.createHmac('sha256', subkey).update(address).digest('hex').slice(0, 32);
  };
  const plain = (address) => crypto.createHash('sha256').update(address).digest('hex').slice(0, 32);
  assert.equal(await claimHash({ JWT_SECRET: 'secret-a' }, 'user1@example.com'), expected('secret-a', 'user1@example.com'));
  assert.equal(await claimHash({ JWT_SECRET: 'secret-a' }, 'User1@Example.COM'), expected('secret-a', 'user1@example.com'), 'one address however it is cased');
  assert.notEqual(await claimHash({ JWT_SECRET: 'secret-a' }, 'user1@example.com'), plain('user1@example.com'), 'not the bare hash of the address');
  assert.notEqual(await claimHash({ JWT_SECRET: 'secret-a' }, 'user1@example.com'), plain('eustress-release-notify:user1@example.com'));
  assert.notEqual(await claimHash({ JWT_SECRET: 'secret-b' }, 'user1@example.com'), await claimHash({ JWT_SECRET: 'secret-a' }, 'user1@example.com'), 'another secret, another name');
  assert.equal(await claimHash({ AUDIT_PRINT_KEY: 'print-key', JWT_SECRET: 'secret-a' }, 'user1@example.com'), expected('print-key', 'user1@example.com'), 'the audit key is the one the account object uses for the same job');
  assert.equal(claimSecret({ AUDIT_PRINT_KEY: 'k', JWT_SECRET: 's' }), 'k');
  assert.equal(claimSecret({ JWT_SECRET: 's' }), 's');
  assert.equal(claimSecret({}), null);
  assert.match(await claimHash({ JWT_SECRET: 's' }, 'a@b.cd'), /^[0-9a-f]{32}$/);

  // The claims in storage are named that way, and with no secret the send does not run.
  const { env } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  await send(env);
  assert.deepEqual(claimKeysOf(env), [claimKey('player', '0.1.0', expected(env.JWT_SECRET, 'user1@example.com'))]);
  const bare = await environment({ extra: { JWT_SECRET: undefined } });
  assert.equal(claimSecret(bare.env), null);
});

test('with no secret to key the claims under, or no bucket, a send is refused before it touches a subscriber', async () => {
  const sent = [];
  for (const [extra, what] of [[{ JWT_SECRET: undefined }, 'no secret'], [{ SCENES: undefined }, 'no bucket']]) {
    const env = bareEnv(sent);
    Object.assign(env, extra);
    await env.USERS.put(subscriptionKey('player', accountId(1)), '{}');
    const run = directRun({ env, status: async () => ({ ok: true, verified: true, address: 'a@example.com' }) });
    const r = await run({});
    assert.equal(r.status, 501, what);
    assert.equal(r.body.code, 'not_configured', what);
    assert.equal(sent.length, 0);
    assert.equal(env.USERS.calls.length, 0, `${what}: no subscriber was listed`);
  }
});

// -- an address never reaches the log ---------------------------------------------------------------------------------------------------------

test('a mail binding whose error quotes the recipient does not put the address in the log: it is masked, in any shape it is quoted', async () => {
  const quoting = [
    (to) => new Error(`destination address ${to} is not allowed`),
    (to) => new Error(`rejected: "${to.toUpperCase()}" <${to}> ${encodeURIComponent(to)} ${JSON.stringify({ to })}`),
    (to) => `plain string thrown for ${to}`,
    (to) => Object.assign(new Error(`550 5.1.1 <${to}>: Recipient address rejected`), { name: 'MailError' }),
  ];
  for (const make of quoting) {
    const { env } = await environment({ extra: { EMAIL: { send: async (m) => { throw make(m.to); } } } });
    const ids = [await account(env, 1), await account(env, 2)];
    for (const id of ids) await subscribe(env, id);
    const logged = [];
    const real = console.error;
    console.error = (...args) => { logged.push(args.map((a) => (typeof a === 'string' ? a : JSON.stringify(a))).join(' ')); };
    let r;
    try { r = await send(env); } finally { console.error = real; }
    assert.deepEqual([r.body.sent, r.body.failed], [0, 2]);
    const text = logged.join('\n');
    assert.ok(logged.length >= 2, 'the failures are logged');
    assert.ok(!/user\d@example\.com|USER\d@EXAMPLE\.COM|user\d%40example|%40example/i.test(text), `an address is in the log: ${text.slice(0, 300)}`);
    assert.match(text, /\*\*\*@example\.com/i, 'the masked address is what the log shows');
    assert.ok(!/@|example\.com/.test(r.text + JSON.stringify(await audits(env))), 'and in nothing else the run produced');
  }
});

test('scrubAddresses masks every address in a message, however it is quoted, and leaves the rest of the message', () => {
  assert.equal(maskAddress('user1@example.com'), 'u***@example.com');
  assert.equal(scrubAddresses('destination address user1@example.com is not allowed'), 'destination address u***@example.com is not allowed');
  assert.equal(scrubAddresses('a@b.cd and e@f.gh'), 'a***@b.cd and e***@f.gh');
  assert.equal(scrubAddresses('"Zed@Example.COM"'), '"***@Example.COM"'.replace('***', '***'), 'quoted');
  assert.ok(!scrubAddresses('<x@y.zz>,').includes('x@y'));
  assert.ok(!scrubAddresses('x%40y.zz').includes('%40') || !scrubAddresses('x%40y.zz').includes('x%40y'));
  assert.equal(scrubAddresses('no address here'), 'no address here');
  assert.equal(scrubAddresses(undefined), '');
  assert.equal(scrubAddresses(new Error('failed for q@r.st')), 'Error: failed for q***@r.st');
  assert.equal(scrubAddresses(42), '42');
});

test('the logs of the send itself carry no address either: a failing store and a failing audit say what failed and not for whom', async () => {
  const { env } = await environment();
  const id = await account(env, 1);
  await subscribe(env, id);
  const realPut = env.SCENES.put.bind(env.SCENES);
  env.SCENES.put = async (key, value, meta) => { if (/"state":"sent"/.test(String(value))) throw new Error('could not confirm user1@example.com'); return realPut(key, value, meta); };
  env.AUDIT_LOG.put = async () => { throw new Error('audit for user1@example.com failed'); };
  const logged = [];
  const real = console.error;
  console.error = (...args) => { logged.push(args.join(' ')); };
  try { await send(env); } finally { console.error = real; }
  assert.ok(logged.length >= 2);
  assert.ok(!/user1@example\.com/.test(logged.join('\n')), logged.join('\n'));
});

// -- the sender's display name -------------------------------------------------------------------------------------------------------------

test('a sender name is plain words: letters, digits, spaces, periods and hyphens, and nothing that could restructure the From header', () => {
  const mime = (fromName) => buildNoticeMime({ from: 'identity@eustress.dev', to: 'owner@example.com', subject: 's', text: 't', html: 'h', ...(fromName === undefined ? {} : { fromName }) });
  assert.match(mime(), /^From: Eustress Security <identity@eustress\.dev>\r\n/, 'the default is what it was');
  for (const ok of ['Eustress', 'Eustress Gallery', 'A.B-C 9', 'x'.repeat(60)]) assert.match(mime(ok), new RegExp(`^From: ${ok} <identity@eustress\\.dev>\\r\\n`), ok);
  for (const bad of ['Eustress <evil@example.com>', 'Eustress, Evil', 'Ev"il', 'Ev;il', 'Ev:il', 'Ev@il', 'Ev(il)', 'Ev\\il', "Ev'il", 'Ev\r\nBcc: x@y.zz', 'Ev\nil', 'Ev<il>', '<Eustress>', 'a>b', '', ' '.repeat(0), 'x'.repeat(61), 7, null, {}, 'Ev\tl', String.fromCharCode(233) + 'ustress']) {
    assert.throws(() => mime(bad), (e) => /sender name|line break/.test(e.message), JSON.stringify(bad));
  }
});

// -- what one request may spend ---------------------------------------------------------------------------------------------------------------

/// Counts every KV, R2, Durable Object and mail call an environment is given.
function instrumented(env) {
  const calls = { total: 0, by: {} };
  const count = (name) => { calls.total += 1; calls.by[name] = (calls.by[name] || 0) + 1; };
  for (const name of ['USERS', 'SOCIAL', 'AUDIT_LOG', 'SCENES']) {
    const target = env[name];
    for (const method of ['get', 'put', 'delete', 'list', 'head']) {
      if (typeof target?.[method] !== 'function') continue;
      const real = target[method].bind(target);
      target[method] = (...args) => { count(`${name}.${method}`); return real(...args); };
    }
  }
  const realObjects = env.ACCOUNT_AUTH;
  env.ACCOUNT_AUTH = { instances: realObjects.instances, idFromName: realObjects.idFromName, get: (id) => { const stub = realObjects.get(id); return { fetch: (...args) => { count('ACCOUNT_AUTH.fetch'); return stub.fetch(...args); } }; } };
  const realSend = env.EMAIL.send;
  env.EMAIL.send = (...args) => { count('EMAIL.send'); return realSend(...args); };
  return calls;
}

test('a page is within the platform limit: the calls one subscriber and one request make are counted, and the largest page fits under the limit', async () => {
  assert.equal(SUBREQUEST_LIMIT, 1000);
  assert.ok(SEND_PAGE_MAX * SUBREQUESTS_PER_SUBSCRIBER + SUBREQUESTS_PER_REQUEST <= SUBREQUEST_LIMIT, 'the largest page, every subscriber at the worst, plus the request itself');
  assert.equal(SEND_PAGE_MAX, 60);
  assert.equal(SUBREQUESTS_PER_SUBSCRIBER, 14);
  assert.equal(SUBREQUESTS_PER_REQUEST, 12);

  // The request itself: no subscriber at all.
  const empty = await environment();
  const overhead = instrumented(empty.env);
  assert.equal((await send(empty.env)).status, 200);
  assert.ok(overhead.total <= SUBREQUESTS_PER_REQUEST, `a request alone is ${overhead.total} calls (${JSON.stringify(overhead.by)})`);

  // One subscriber's whole path, with the account's object already set up.
  const { env } = await environment();
  const ids = [await account(env, 1), await account(env, 2), await account(env, 3)];
  for (const id of ids) await subscribe(env, id);
  await send(env, { dry_run: true });
  const counted = instrumented(env);
  assert.equal((await send(env)).body.sent, 3);
  const perSubscriber = (counted.total - overhead.total) / 3;
  assert.ok(perSubscriber <= 11, `a subscriber is ${perSubscriber} calls at most 11 (${JSON.stringify(counted.by)})`);
  assert.ok(perSubscriber <= SUBREQUESTS_PER_SUBSCRIBER);
  assert.ok(perSubscriber >= 9, `and it is counted, not assumed: ${perSubscriber}`);

  // The first time an account's object is looked at it is set up: three calls more.
  const fresh = await environment();
  const id = await account(fresh.env, 1);
  await subscribe(fresh.env, id);
  const seeded = fresh.env.ACCOUNT_AUTH.instances.get(id);
  await seeded.storage.deleteAll();
  const first = instrumented(fresh.env);
  assert.deepEqual([(await send(fresh.env)).body.skipped_unverified], [1], 'an unseeded object has verified nothing');
  assert.ok(first.total - overhead.total <= SUBREQUESTS_PER_SUBSCRIBER, `an account that is set up in this run costs ${first.total - overhead.total}`);
});

test('a claim that was released between the two calls is tried once more, and one lost twice is another run\'s: nothing is sent over it and the subscription stays', async () => {
  // A put that loses its precondition writes nothing and answers null; here the object is not there to read afterwards either.
  for (const [losses, expectSent] of [[1, 1], [2, 0]]) {
    const { env, sent } = await environment();
    const id = await account(env, 1);
    await subscribe(env, id);
    const realPut = env.SCENES.put.bind(env.SCENES);
    let lost = 0;
    env.SCENES.put = async (key, value, meta) => {
      if (meta?.onlyIf && lost < losses) { lost += 1; return null; }
      return realPut(key, value, meta);
    };
    const r = await send(env);
    assert.equal(r.status, 200, r.text);
    assert.deepEqual([r.body.sent, r.body.already_sent, r.body.failed], [expectSent, 1 - expectSent, 0], `lost ${losses} time(s)`);
    assert.equal(sent.length, expectSent);
    assert.equal(await subscribed(env, id), expectSent === 0, 'kept unless it was sent');
  }
});
