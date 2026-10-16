// People search (GET /api/community/search), the route behind the site's Friends page. Each result carries what the site
// reads: username, display_name, avatar_url, is_verified, a follower_count that is the real one (the list the follow route
// keeps), created_at and joined_at. No account id leaves this route; a follow is by username. The route answers anyone, signed
// in or not, so it makes no paid call: with an xAI key set, a search that finds nobody still sends nothing to xAI and the
// answer carries no suggestion. The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();
const ORIGIN = 'https://api.eustress.dev';
const ADA = '11111111-1111-4111-8111-111111111111';
const ADAM = '22222222-2222-4222-8222-222222222222';
const BOB = '33333333-3333-4333-8333-333333333333';
const FAN_ONE = '44444444-4444-4444-8444-444444444444';
const FAN_TWO = '55555555-5555-4555-8555-555555555555';
const FAN_THREE = '66666666-6666-4666-8666-666666666666';

async function environment(extra = {}) {
  const env = { ...fakeEnv(), ...extra };
  for (const [id, username, verified, created] of [
    [ADA, 'ada_builds', true, '2026-03-01T00:00:00.000Z'],
    [ADAM, 'adam', false, '2026-04-02T00:00:00.000Z'],
    [BOB, 'bob', true, '2026-05-03T00:00:00.000Z'],
    [FAN_ONE, 'fan_one', false, '2026-06-01T00:00:00.000Z'],
    [FAN_TWO, 'fan_two', false, '2026-06-02T00:00:00.000Z'],
    [FAN_THREE, 'fan_three', false, '2026-06-03T00:00:00.000Z'],
  ]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ id, username, created_at: created, ...(verified ? { kyc_verified: true } : {}) }));
    await env.USERS.put(`username:${username}`, id);
  }
  // Three accounts follow ada_builds, the way the follow route stores it.
  await env.SOCIAL.put(`followers:${ADA}`, JSON.stringify([FAN_ONE, FAN_TWO, FAN_THREE]));
  return env;
}

async function search(env, query, { auth = null } = {}) {
  const headers = auth ? { Authorization: `Bearer ${auth}` } : {};
  const res = await worker.fetch(new Request(`${ORIGIN}/api/community/search?q=${encodeURIComponent(query)}`, { headers }), env, ctx);
  return { status: res.status, text: await res.clone().text(), body: await res.json() };
}

/// Every call to the network, answered with an xAI-shaped reply, so a call that should not happen is seen.
async function withFetchSpy(fn) {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push(String(url));
    return new Response(JSON.stringify({ output: [{ type: 'message', content: [{ type: 'output_text', text: 'Try a username.' }] }] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try { await fn(calls); } finally { globalThis.fetch = real; }
}

test('a result carries joined_at beside created_at, and the fields the site reads', async () => {
  const env = await environment();
  const { status, body } = await search(env, 'ada_b');
  assert.equal(status, 200);
  assert.equal(body.users.length, 1);
  const user = body.users[0];
  assert.equal(user.username, 'ada_builds');
  assert.equal(user.display_name, 'ada_builds');
  assert.equal(user.is_verified, true);
  assert.equal(user.created_at, '2026-03-01T00:00:00.000Z');
  assert.equal(user.joined_at, '2026-03-01T00:00:00.000Z', 'joined_at is the account\'s created_at');
  assert.equal(user.avatar_url, null);
  assert.deepEqual(Object.keys(user).sort(), ['avatar_url', 'created_at', 'display_name', 'follower_count', 'is_verified', 'joined_at', 'username']);
});

test('no account id leaves the search; a follow goes by username', async () => {
  const env = await environment();
  const { text } = await search(env, 'ada');
  for (const id of [ADA, ADAM]) assert.ok(!text.includes(id), 'an account id is in the answer');
  assert.ok(!/"(?:id|user_id)"/.test(text));
});

test('follower_count is the real count the follow route keeps, and moves with a follow', async () => {
  const env = await environment();
  let { body } = await search(env, 'ada');
  const byName = (b) => Object.fromEntries(b.users.map((u) => [u.username, u.follower_count]));
  assert.deepEqual(byName(body), { ada_builds: 3, adam: 0 });

  // A real follow through the Worker's own route.
  const res = await worker.fetch(new Request(`${ORIGIN}/api/social/follow`, {
    method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${mintToken(BOB)}` }, body: JSON.stringify({ username: 'adam' }),
  }), env, ctx);
  assert.equal(res.status, 200);
  ({ body } = await search(env, 'ada'));
  assert.deepEqual(byName(body), { ada_builds: 3, adam: 1 });
});

test('a search reads one follower list per hit and answers at most 20 hits', async () => {
  const env = await environment();
  for (let i = 0; i < 30; i++) {
    const id = `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`;
    await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: `crowd_${i}`, created_at: '2026-07-01T00:00:00.000Z' }));
    await env.USERS.put(`username:crowd_${i}`, id);
  }
  const reads = [];
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  env.SOCIAL.get = async (key) => { reads.push(key); return get(key); };
  const res = await worker.fetch(new Request(`${ORIGIN}/api/community/search?q=crowd&limit=500`), env, ctx);
  const body = await res.json();
  assert.equal(body.users.length, 20);
  assert.equal(reads.filter((k) => k.startsWith('followers:')).length, 20);
  assert.equal(reads.length, 20, 'nothing else is read from SOCIAL');
});

test('a search that finds nobody makes no call to xAI, even with a key set, signed in or not', async () => {
  const env = await environment({ GROK_API_KEY: 'xai-test-key-not-real' });
  await withFetchSpy(async (calls) => {
    for (const auth of [null, mintToken(BOB)]) {
      const { status, body, text } = await search(env, 'zzzzqq', { auth });
      assert.equal(status, 200);
      assert.deepEqual(body, { users: [], query: 'zzzzqq' });
      assert.ok(!text.includes('ai_suggestion'));
    }
    assert.deepEqual(calls, [], 'the network was called');
  });
});

test('a search that finds someone makes no call to xAI either, and a short query finds nobody without one', async () => {
  const env = await environment({ GROK_API_KEY: 'xai-test-key-not-real' });
  await withFetchSpy(async (calls) => {
    const hit = await search(env, 'bob');
    assert.equal(hit.body.users.length, 1);
    const short = await search(env, 'b');
    assert.deepEqual(short.body, { users: [], query: 'b' });
    assert.deepEqual(calls, []);
  });
});
