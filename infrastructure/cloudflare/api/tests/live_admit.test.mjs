// Who may join a hosted gallery session. The host asks the Worker to admit each
// joining player (POST /api/simulations/{id}/live/admit); a ticket is admitted
// once per hosting session, and the join link goes only to an account or a live
// guest lease. See docs/networking/HOSTED_ADMISSION.md. The real Worker runs
// against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, MAX_ADMITTED } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { makeIdentityTicket } from '../src/identity.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const PIN2 = 'cd'.repeat(32);
const linkFor = (pin) => `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${pin}`;

function environment() {
  const env = { ...fakeEnv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

async function seed(env, moderation = { status: 'approved', rating: 'all_ages' }) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation }));
  return env;
}

async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

const author = mintToken(AUTHOR);
const player = mintToken(PLAYER);
const live = `/api/simulations/${SIM}/live`;
const beat = (env, pin = PIN, extra = {}) => call(env, 'POST', live, { token: author, body: { link: linkFor(pin), players: 1, max_players: 4, protocol: 3, ...extra } });
const admit = (env, ticket, audience = PIN, token = author) => call(env, 'POST', `${live}/admit`, { token, body: { ticket, audience } });

/// A ticket as a Player mints it: through the real route, as a signed-in account.
async function ticketFor(env, token, audience = PIN) {
  const r = await call(env, 'POST', '/api/identity/ticket', { token, body: { audience } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

async function hosting(env) {
  await seed(env);
  assert.equal((await beat(env)).status, 200);
  return env;
}

// ── Tickets carry an id ──────────────────────────────────────────────────────

test('a ticket carries a random id, and two tickets for one account never share it', async () => {
  const env = await seed(environment());
  const a = await ticketFor(env, player);
  const b = await ticketFor(env, player);
  assert.match(a.id, /^[0-9a-f]{32}$/);
  assert.notEqual(a.id, b.id);
  assert.notEqual(a.ticket, b.ticket);
});

// ── Admission ────────────────────────────────────────────────────────────────

test('the host admits a signed-in player once, and learns who they are', async () => {
  const env = await hosting(environment());
  const { ticket } = await ticketFor(env, player);
  const r = await admit(env, ticket);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.deepEqual([r.body.ok, r.body.account_id, r.body.username, r.body.guest], [true, PLAYER, 'ada', false]);
  assert.ok(r.body.expires > Date.now() / 1000);
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
});

test('the same ticket is refused the second time, with the code the Player is told', async () => {
  const env = await hosting(environment());
  const { ticket } = await ticketFor(env, player);
  assert.equal((await admit(env, ticket)).status, 200);
  const again = await admit(env, ticket);
  assert.equal(again.status, 403);
  assert.equal(again.body.code, 'used');
  assert.match(again.body.error, /Join again/);
});

test('two joins with one ticket at once admit exactly one', async () => {
  const env = await hosting(environment());
  const { ticket } = await ticketFor(env, player);
  const results = await Promise.all([admit(env, ticket), admit(env, ticket), admit(env, ticket)]);
  assert.deepEqual(results.map((r) => r.status).sort(), [200, 403, 403]);
});

test('a new ticket for the same account is admitted: a reconnect gets a fresh one', async () => {
  const env = await hosting(environment());
  assert.equal((await admit(env, (await ticketFor(env, player)).ticket)).status, 200);
  assert.equal((await admit(env, (await ticketFor(env, player)).ticket)).status, 200);
});

test('no ticket, a made-up one, an expired one and one for another host are each refused with their own code', async () => {
  const env = await hosting(environment());
  const code = async (ticket, audience) => {
    const r = await admit(env, ticket, audience);
    assert.equal(r.status, 403, JSON.stringify(r.body));
    return r.body.code;
  };
  assert.equal(await code(undefined), 'no_ticket');
  assert.equal(await code(''), 'no_ticket');
  assert.equal(await code('garbage'), 'invalid');
  assert.equal(await code('eit1.a.b'), 'invalid');
  assert.equal(await code(12345), 'invalid');

  const old = await makeIdentityTicket(env, { accountId: PLAYER, username: 'ada', audience: PIN, now: Math.floor(Date.now() / 1000) - 3600 });
  assert.equal(await code(old.ticket), 'expired');

  const other = await ticketFor(env, player, PIN2);
  assert.equal(await code(other.ticket), 'wrong_host', 'a ticket made for another pin');

  const fine = await ticketFor(env, player);
  assert.equal(await code(fine.ticket, PIN2), 'wrong_host', 'the host names a pin that is not this session');
});

test('a ticket made before ids existed is refused, since it cannot be tracked', async () => {
  const env = await hosting(environment());
  const { ticket } = await ticketFor(env, player);
  // Rebuild the ticket without its id, signed with the real key.
  const [prefix, body] = ticket.split('.');
  const payload = JSON.parse(Buffer.from(body, 'base64url').toString());
  delete payload.j;
  const noId = Buffer.from(JSON.stringify(payload)).toString('base64url');
  const forged = await admit(env, `${prefix}.${noId}.${ticket.split('.')[2]}`);
  assert.equal(forged.body.code, 'invalid', 'changing the payload breaks the signature');
});

test('only the author, signed in, can ask the Worker to admit', async () => {
  const env = await hosting(environment());
  const { ticket } = await ticketFor(env, player);
  const anon = await call(env, 'POST', `${live}/admit`, { body: { ticket, audience: PIN } });
  assert.equal(anon.status, 401);
  const stranger = await admit(env, ticket, PIN, player);
  assert.equal(stranger.status, 403);
  assert.equal(stranger.body.code, 'not_author');
  assert.equal((await admit(env, ticket)).status, 200, 'the refusals used nothing up');
});

test('a bad audience is a bad request, and admitting needs a live host', async () => {
  const env = await seed(environment());
  const { ticket } = await ticketFor(env, player);
  assert.equal((await admit(env, ticket, 'not-a-pin')).body.code, 'bad_audience');
  const notLive = await admit(env, ticket);
  assert.equal(notLive.status, 409);
  assert.equal(notLive.body.code, 'not_live');
  const get = await call(env, 'GET', `${live}/admit`, { token: author });
  assert.equal(get.status, 405);
});

test('the set of admitted ids is bounded: a full table says busy, and expired ids make room', async () => {
  const env = await hosting(environment());
  const storage = env.LIVE_HOSTS.instances.get(SIM).storage;
  const nowS = Math.floor(Date.now() / 1000);

  for (let i = 0; i < MAX_ADMITTED; i++) await storage.put(`adm:${i.toString(16).padStart(32, '0')}`, nowS + 600);
  const { ticket } = await ticketFor(env, player);
  const full = await admit(env, ticket);
  assert.equal(full.status, 403);
  assert.equal(full.body.code, 'busy');
  assert.match(full.body.error, /could not confirm/);
  assert.equal((await storage.list({ prefix: 'adm:' })).size, MAX_ADMITTED, 'nothing was forgotten to make room');

  for (let i = 0; i < MAX_ADMITTED; i++) await storage.put(`adm:${i.toString(16).padStart(32, '0')}`, nowS - 1);
  assert.equal((await admit(env, ticket)).status, 200, 'expired ids no longer count');
  assert.equal((await storage.list({ prefix: 'adm:' })).size, 1, 'and were dropped');
});

test('a new hosting session, or the host stopping, forgets the admitted tickets', async () => {
  const env = await hosting(environment());
  const storage = env.LIVE_HOSTS.instances.get(SIM).storage;
  assert.equal((await admit(env, (await ticketFor(env, player)).ticket)).status, 200);
  assert.equal((await storage.list({ prefix: 'adm:' })).size, 1);

  // The same link beating again is the same session.
  assert.equal((await beat(env)).status, 200);
  assert.equal((await storage.list({ prefix: 'adm:' })).size, 1);

  // A restart brings a new certificate, so a new pin: a new session.
  assert.equal((await beat(env, PIN2)).status, 200);
  assert.equal((await storage.list({ prefix: 'adm:' })).size, 0);

  assert.equal((await admit(env, (await ticketFor(env, player, PIN2)).ticket, PIN2)).status, 200);
  assert.equal((await call(env, 'DELETE', live, { token: author })).status, 200);
  assert.equal((await storage.list({ prefix: 'adm:' })).size, 0);
});

test('a host that goes silent forgets the admitted tickets when its alarm clears it', async () => {
  const realNow = Date.now;
  const t0 = realNow();
  Date.now = () => t0;
  try {
    const env = await hosting(environment());
    const obj = env.LIVE_HOSTS.instances.get(SIM);
    assert.equal((await admit(env, (await ticketFor(env, player)).ticket)).status, 200);
    Date.now = () => t0 + 91_000;
    await obj.alarm();
    assert.equal((await obj.storage.list({ prefix: 'adm:' })).size, 0);
    assert.equal(obj.storage.map.has('host'), false);
  } finally {
    Date.now = realNow;
  }
});

// ── The heartbeat's list of connected players ────────────────────────────────

test('a heartbeat may list the ticket ids still connected, and the list is checked', async () => {
  const env = await seed(environment());
  const id = (n) => n.toString(16).padStart(32, '0');
  assert.equal((await beat(env, PIN, { admitted: [id(1), id(2)] })).status, 200);
  assert.equal((await beat(env, PIN, { admitted: [] })).status, 200);
  for (const bad of ['x', [123], ['short'], ['ABCDEF0123456789ABCDEF0123456789'], [id(1), id(2), id(3), id(4), id(5)], { 0: id(1) }]) {
    const r = await beat(env, PIN, { admitted: bad });
    assert.equal(r.status, 400, JSON.stringify(bad));
    assert.equal(r.body.code, 'bad_admitted');
  }
});

// ── The link ─────────────────────────────────────────────────────────────────

test('the join link goes to a signed-in account, and an anonymous visitor sees only that the simulation is live', async () => {
  const env = await hosting(environment());
  const anon = await call(env, 'GET', live);
  assert.equal(anon.status, 200);
  assert.equal(anon.body.live, true);
  assert.equal(anon.body.players, 1);
  assert.equal(anon.body.link_hidden, true);
  assert.ok(!('link' in anon.body), JSON.stringify(anon.body));
  assert.ok(!JSON.stringify(anon.body).includes('eustress-player'));

  for (const token of [player, author]) {
    const r = await call(env, 'GET', live, { token });
    assert.equal(r.body.link, linkFor(PIN));
    assert.ok(!('link_hidden' in r.body));
  }
  assert.equal(anon.headers.get('Cache-Control'), 'no-store');
});

test('a guest holding a live lease gets the link; an ended, forged or missing lease does not', async () => {
  const env = await hosting(environment());
  const ip = { 'CF-Connecting-IP': '203.0.113.7' };
  const open = await call(env, 'POST', '/api/guest/play', { headers: ip, body: { sim_id: SIM, character: 'F' } });
  assert.equal(open.status, 200, JSON.stringify(open.body));
  const lease = { 'X-Eustress-Lease': open.body.ticket };

  assert.equal((await call(env, 'GET', live, { headers: lease })).body.link, linkFor(PIN), 'a live lease');
  assert.equal((await call(env, 'GET', live, { headers: { 'X-Eustress-Lease': open.body.ticket.slice(0, -3) + 'abc' } })).body.link_hidden, true, 'a forged one');
  assert.equal((await call(env, 'GET', live, { headers: { 'X-Eustress-Lease': 'egl1.junk.junk' } })).body.link_hidden, true, 'a made-up one');
  assert.equal((await call(env, 'GET', live)).body.link_hidden, true, 'none');

  await call(env, 'POST', '/api/guest/end', { body: { ticket: open.body.ticket } });
  assert.equal((await call(env, 'GET', live, { headers: lease })).body.link_hidden, true, 'a lease that has ended');
});

test('an unapproved listing still shows nothing to the public, and its author still sees the link', async () => {
  const env = await seed(environment(), { status: 'pending' });
  await beat(env);
  assert.deepEqual((await call(env, 'GET', live)).body, { live: false });
  assert.deepEqual((await call(env, 'GET', live, { token: player })).body, { live: false });
  assert.equal((await call(env, 'GET', live, { token: author })).body.link, linkFor(PIN));
});

test('the lease header is allowed by CORS, so the site can send it', async () => {
  const env = await hosting(environment());
  const res = await worker.fetch(new Request(`https://api.eustress.dev${live}`, {
    method: 'OPTIONS', headers: { Origin: 'https://eustress.dev', 'Access-Control-Request-Method': 'GET', 'Access-Control-Request-Headers': 'x-eustress-lease' },
  }), env, ctx);
  assert.match(res.headers.get('Access-Control-Allow-Headers'), /X-Eustress-Lease/);
});
