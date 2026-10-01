import test from 'node:test';
import assert from 'node:assert/strict';
import { checkJoinLink, LiveHost, handleLive, HEARTBEAT_TTL_MS } from '../src/live.mjs';
import { fakeDoNamespace, JWT_SECRET } from './helpers/load_worker.mjs';

const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const PIN = 'ab'.repeat(32);
const KEY = '0123456789abcdef0123456789abcdef';
const LINK = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${PIN}`;

// The Durable Object fakes are shared (tests/helpers/load_worker.mjs).
const namespace = () => fakeDoNamespace(LiveHost, { JWT_SECRET });

function kv() {
  const store = new Map();
  return { store, get: async (k) => store.get(k) ?? null, put: async (k, v) => store.set(k, v) };
}

function environment({ listed = true, binding = true } = {}) {
  const env = { SOCIAL: kv(), LIVE_HOSTS: binding ? namespace() : undefined };
  env.SOCIAL.store.set(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Pong', author_id: 'mckale', is_public: true,
    moderation: { status: listed ? 'approved' : 'pending' },
  }));
  return env;
}

const deps = {
  verifyAuth: async (request) => {
    const h = request.headers.get('Authorization') || '';
    return h.startsWith('Bearer ') ? h.slice(7) : null;
  },
  requireAdmin: async (request) => ((request.headers.get('Authorization') || '') === 'Bearer admin' ? 'admin' : null),
  json: (value, status, headers) => new Response(JSON.stringify(value), { status, headers }),
  isListable: (sim) => !!sim && sim.is_public !== false && sim.moderation?.status === 'approved',
};

function req(method, { token, body } = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers['content-type'] = 'application/json';
  return new Request(`https://api.eustress.dev/api/simulations/${SIM}/live`, {
    method, headers, body: body === undefined ? undefined : JSON.stringify(body),
  });
}
const beat = (link = LINK, extra = {}) => ({ link, players: 1, max_players: 2, protocol: 3, ...extra });
const call = async (env, method, opts) => {
  const res = await handleLive(req(method, opts), env, {}, SIM, deps);
  return { status: res.status, body: await res.json(), headers: res.headers };
};

test('join links are exactly what the host produces: the Player scheme, a 32 hex key and a 64 hex pin', () => {
  for (const ok of [
    LINK,
    `eustress-player://join/play.eustress.dev:7777?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/[2001:db8::1]:7777?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY.toUpperCase()}&pin=${PIN.toUpperCase()}`,
    `eustress-player://join/10.0.0.5:7777?pin=${PIN}&key=${KEY}`,
  ]) assert.equal(checkJoinLink(ok).ok, true, ok);

  for (const bad of [
    `eustress://join/10.0.0.5:7777?key=${KEY}&pin=${PIN}`,
    `javascript:alert(1)//eustress-player://join/a:1?key=${KEY}&pin=${PIN}`,
    `https://example.com/?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}`,
    `eustress-player://join/10.0.0.5:7777?pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}&pin=${PIN.slice(2)}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY.slice(1)}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}0&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${'z'.repeat(32)}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=room-42_x&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:0?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:70000?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}&pin=${PIN}&redirect=x`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}&pin=${PIN}&v=3`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}&pin=${PIN}&sim=${SIM}`,
    `eustress-player://join/10.0.0.5:7777?key=${KEY}&key=${KEY}&pin=${PIN}`,
    `eustress-player://join/host name:7777?key=${KEY}&pin=${PIN}`,
    `eustress-player://join/10.0.0.5?key=${KEY}&pin=${PIN}`,
    'x'.repeat(600),
    '',
    null,
  ]) assert.equal(checkJoinLink(bad).ok, false, String(bad));
});

test('a link may not name the player\'s own machine or a link-local address, in any spelling; LAN addresses are fine', () => {
  const at = (host) => `eustress-player://join/${host}:7777?key=${KEY}&pin=${PIN}`;
  for (const host of [
    'localhost', 'LOCALHOST', 'localhost.', 'play.localhost',
    '127.0.0.1', '127.1', '127.0.0.1.', '127.255.255.254', '2130706433', '0x7f.1', '0177.0.0.1',
    '0', '0.0.0.0', '169.254.1.1', '169.254.169.254',
    '[::1]', '[0:0:0:0:0:0:0:1]', '[::]', '[fe80::1]', '[FE80::abcd]', '[febf::1]',
    '[::ffff:127.0.0.1]', '[::ffff:7f00:1]', '[::ffff:169.254.0.1]', '[::ffff:0.0.0.0]',
    '999.1.1.1', '[:::]',
  ]) assert.equal(checkJoinLink(at(host)).ok, false, host);

  for (const host of [
    '192.168.1.20', '10.0.0.5', '172.16.0.1', '100.64.0.1', '[fd00::1]', '[fec0::1]',
    '[2001:db8::1]', '[::ffff:192.168.1.20]', 'play.eustress.dev', 'localhost-relay.example.com',
    '126.255.255.255', '128.0.0.1', '169.253.1.1', '169.255.0.1', '[fe7f::1]', '[fec0::2]',
  ]) assert.equal(checkJoinLink(at(host)).ok, true, host);
});

test('a heartbeat makes the listing Live with the host\'s own link', async () => {
  const env = environment();
  assert.deepEqual((await call(env, 'GET')).body, { live: false });

  const r = await call(env, 'POST', { token: 'mckale', body: beat() });
  assert.equal(r.status, 200);
  assert.ok(r.body.expires_at);

  const anonymous = await call(env, 'GET');
  assert.equal(anonymous.body.live, true);
  assert.equal(anonymous.body.link, undefined, 'an anonymous visitor is not given the link');
  assert.equal(anonymous.body.link_hidden, true);

  const g = await call(env, 'GET', { token: 'someone-else' });
  assert.equal(g.headers.get('Cache-Control'), 'no-store');
  assert.equal(g.body.live, true);
  assert.equal(g.body.link, LINK, 'a signed-in account gets the link exactly as the host sent it');
  assert.deepEqual([g.body.players, g.body.max_players, g.body.protocol], [1, 2, 3]);
});

test('only the author can host, and only a well-formed link is stored', async () => {
  const env = environment();
  assert.equal((await call(env, 'POST', { body: beat() })).status, 401);
  assert.equal((await call(env, 'POST', { token: 'someone-else', body: beat() })).status, 403);
  assert.equal((await call(env, 'DELETE', { token: 'someone-else' })).status, 403);
  assert.equal((await call(env, 'POST', { token: 'mckale', body: beat(`eustress://join/a:1?key=${KEY}&pin=${PIN}`) })).status, 400);
  assert.equal((await call(env, 'POST', { token: 'mckale', body: beat(LINK, { players: -1 }) })).status, 400);
  assert.equal((await call(env, 'POST', { token: 'mckale', body: beat(LINK, { max_players: 0 }) })).status, 400);
  assert.equal((await call(env, 'POST', { token: 'mckale', body: beat(LINK, { protocol: 1.5 }) })).status, 400);
  assert.deepEqual((await call(env, 'GET')).body, { live: false }, 'nothing was stored');
});

test('stopping takes the listing Offline at once', async () => {
  const env = environment();
  await call(env, 'POST', { token: 'mckale', body: beat() });
  assert.equal((await call(env, 'DELETE', { token: 'mckale' })).status, 200);
  assert.deepEqual((await call(env, 'GET')).body, { live: false });
  assert.equal(env.LIVE_HOSTS.instances.get(SIM).storage.alarm, null);
});

test('a host that stops beating goes Offline on its own, and a late beat keeps it Live', async () => {
  const env = environment();
  const realNow = Date.now;
  let t = Date.UTC(2026, 8, 24, 12, 0, 0);
  Date.now = () => t;
  try {
    await call(env, 'POST', { token: 'mckale', body: beat() });
    const obj = env.LIVE_HOSTS.instances.get(SIM);
    assert.equal(obj.storage.alarm, t + HEARTBEAT_TTL_MS);

    // A beat 60 s later moves the deadline; an alarm set by the first beat
    // that fires then must not clear the host.
    t += 60_000;
    await call(env, 'POST', { token: 'mckale', body: beat() });
    t += 31_000;
    await obj.alarm();
    assert.equal((await call(env, 'GET')).body.live, true);

    // Then silence past the deadline: read as Offline even before the alarm,
    // and the alarm clears it.
    t += HEARTBEAT_TTL_MS;
    assert.deepEqual((await call(env, 'GET')).body, { live: false });
    await obj.alarm();
    assert.equal(obj.storage.map.has('host'), false);
  } finally {
    Date.now = realNow;
  }
});

test('since follows the server run: the same link keeps it, a restart with a new pin resets it', async () => {
  const env = environment();
  const realNow = Date.now;
  let t = Date.UTC(2026, 8, 24, 12, 0, 0);
  Date.now = () => t;
  try {
    await call(env, 'POST', { token: 'mckale', body: beat() });
    const first = (await call(env, 'GET')).body.since;
    t += 30_000;
    await call(env, 'POST', { token: 'mckale', body: beat() });
    assert.equal((await call(env, 'GET')).body.since, first);
    t += 30_000;
    const restarted = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${'cd'.repeat(32)}`;
    await call(env, 'POST', { token: 'mckale', body: beat(restarted) });
    const g = (await call(env, 'GET', { token: 'someone-else' })).body;
    assert.equal(g.link, restarted);
    assert.notEqual(g.since, first);
  } finally {
    Date.now = realNow;
  }
});

test('a listing that is not approved keeps its host private to the author and admins', async () => {
  const env = environment({ listed: false });
  await call(env, 'POST', { token: 'mckale', body: beat() });
  assert.deepEqual((await call(env, 'GET')).body, { live: false }, 'the public sees nothing');
  assert.deepEqual((await call(env, 'GET', { token: 'someone-else' })).body, { live: false });
  assert.equal((await call(env, 'GET', { token: 'mckale' })).body.live, true, 'the author can check the host');
  assert.equal((await call(env, 'GET', { token: 'admin' })).body.live, true, 'an admin can too');
});

test('a deployment without the binding says so, and an unknown simulation is 404', async () => {
  assert.equal((await call(environment({ binding: false }), 'GET')).status, 503);
  const env = environment();
  const res = await handleLive(req('GET'), env, {}, 'cccccccc-3333-4333-8333-cccccccccccc', deps);
  assert.equal(res.status, 404);
});
