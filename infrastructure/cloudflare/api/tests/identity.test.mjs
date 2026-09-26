import test from 'node:test';
import assert from 'node:assert/strict';
import { handleIdentityRoute, makeIdentityTicket, verifyIdentityTicket, TICKET_TTL_S } from '../src/identity.mjs';

const PIN = 'ab'.repeat(32);
const OTHER_PIN = 'cd'.repeat(32);

function env(secret = 'test-jwt-secret') {
  const users = new Map([['user:player-1', JSON.stringify({ id: 'player-1', username: 'playerone' })]]);
  return { JWT_SECRET: secret, USERS: { get: async (k) => users.get(k) ?? null } };
}

const deps = {
  // Sessions are bearer user ids in these tests.
  verifyAuth: async (request) => {
    const h = request.headers.get('Authorization') || '';
    return h.startsWith('Bearer ') ? h.slice(7) : null;
  },
  json: (value, status, headers) => new Response(JSON.stringify(value), { status, headers }),
};

async function call(e, path, { who, body, method = 'POST' } = {}) {
  const headers = { 'content-type': 'application/json' };
  if (who) headers.Authorization = `Bearer ${who}`;
  const init = { method, headers };
  if (method !== 'GET') init.body = JSON.stringify(body);
  const request = new Request(`https://api.eustress.dev${path}`, init);
  const res = await handleIdentityRoute(request, new URL(request.url), e, {}, deps);
  return { status: res.status, body: await res.json() };
}

test('a ticket names the account for one host connection, and expires', async () => {
  const e = env();
  const now = 1_790_000_000;
  const { ticket, expires } = await makeIdentityTicket(e, { accountId: 'player-1', username: 'playerone', audience: PIN, now });
  assert.match(ticket, /^eit1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/);
  assert.equal(expires, now + TICKET_TTL_S);

  const who = await verifyIdentityTicket(e, ticket, { audience: PIN, now: now + 60 });
  assert.deepEqual(who, { accountId: 'player-1', username: 'playerone', expires });
  assert.equal(await verifyIdentityTicket(e, ticket, { audience: OTHER_PIN, now }), null, 'another host');
  assert.equal(await verifyIdentityTicket(e, ticket, { audience: PIN, now: expires + 1 }), null, 'expired');
  assert.equal(await verifyIdentityTicket(env('another-secret'), ticket, { audience: PIN, now }), null, 'another secret');
});

test('a changed ticket or a session token never passes', async () => {
  const e = env();
  const now = 1_790_000_000;
  const { ticket } = await makeIdentityTicket(e, { accountId: 'player-1', audience: PIN, now });
  const [prefix, body, sig] = ticket.split('.');
  const payload = JSON.parse(Buffer.from(body, 'base64url').toString());
  const forged = Buffer.from(JSON.stringify({ ...payload, a: 'someone-else' })).toString('base64url');
  assert.equal(await verifyIdentityTicket(e, `${prefix}.${forged}.${sig}`, { audience: PIN, now }), null, 'another account');
  const moved = Buffer.from(JSON.stringify({ ...payload, h: OTHER_PIN })).toString('base64url');
  assert.equal(await verifyIdentityTicket(e, `${prefix}.${moved}.${sig}`, { audience: OTHER_PIN, now }), null, 'another host written in');
  assert.equal(await verifyIdentityTicket(e, `${prefix}.${body}.${sig.slice(0, -2)}AA`, { audience: PIN, now }), null, 'a changed signature');
  // A session token signed with the same secret is not a ticket.
  const header = Buffer.from(JSON.stringify({ alg: 'HS256', typ: 'JWT' })).toString('base64url');
  assert.equal(await verifyIdentityTicket(e, `${header}.${body}.${sig}`, { audience: PIN, now }), null);
  for (const junk of ['', 'eit1', 'eit1..', 'eit1.!.!', null, 42, 'x'.repeat(3000)]) {
    assert.equal(await verifyIdentityTicket(e, junk, { audience: PIN, now }), null, String(junk).slice(0, 12));
  }
});

test('a signed-in account gets a ticket the other end can verify', async () => {
  const e = env();
  assert.equal((await call(e, '/api/identity/ticket', { body: { audience: PIN } })).status, 401, 'signed out');
  assert.equal((await call(e, '/api/identity/ticket', { who: 'player-1', body: { audience: 'not-a-pin' } })).status, 400);
  assert.equal((await call(e, '/api/identity/ticket', { who: 'player-1', body: { audience: PIN }, method: 'GET' })).status, 405);
  const made = await call(e, '/api/identity/ticket', { who: 'player-1', body: { audience: PIN } });
  assert.equal(made.status, 200, JSON.stringify(made.body));
  assert.deepEqual([made.body.account_id, made.body.username], ['player-1', 'playerone']);

  const checked = await call(e, '/api/identity/verify', { body: { ticket: made.body.ticket, audience: PIN } });
  assert.equal(checked.status, 200, JSON.stringify(checked.body));
  assert.deepEqual([checked.body.account_id, checked.body.username], ['player-1', 'playerone']);
  const wrongHost = await call(e, '/api/identity/verify', { body: { ticket: made.body.ticket, audience: OTHER_PIN } });
  assert.equal(wrongHost.status, 401);
  assert.equal(wrongHost.body.code, 'invalid_ticket');
});

test('without a signing secret the routes say so instead of signing with nothing', async () => {
  const e = { USERS: { get: async () => null } };
  const r = await call(e, '/api/identity/ticket', { who: 'player-1', body: { audience: PIN } });
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'identity_unavailable');
});
