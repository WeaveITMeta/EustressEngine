// A listing carries the player count its Space authors (Players.MaxPlayers): a whole number from 1 to 10,000, on create
// (POST /api/simulations/publish) and in the listing of every whole-Universe commit (world.mjs). Create without it stores
// 10; outside the range the request is refused with the range, and nothing is written. A change to the number alone does
// not send an approved listing back to review. docs/networking/MAX_PLAYERS.md. The create tests run the real Worker; the
// commit tests run the world route with the same fakes tests/world.test.mjs uses.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { handleWorldRoute, checkMaxPlayers, DEFAULT_MAX_PLAYERS, MAX_PLAYERS_LIMIT, MAX_PLAYERS_MESSAGE } from '../src/world.mjs';
import { isListable, canServe } from '../src/moderation.mjs';
import { LiveHost, MAX_PLAYERS } from '../src/live.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const author = mintToken(AUTHOR);

// ── the rule ─────────────────────────────────────────────────────────────────

test('a player count is a whole number from 1 to 10,000 and nothing is rounded or clamped to fit', () => {
  for (const ok of [1, 4, 10, 12, 100, 9_999, 10_000]) assert.deepEqual(checkMaxPlayers(ok), { ok: true, value: ok }, String(ok));
  for (const bad of [0, -1, 10_001, 1_000_000, 1.5, 4.0000001, NaN, Infinity, -Infinity, '4', '', ' 4', true, false, [], [4], {}, Number.MAX_SAFE_INTEGER + 2]) {
    const r = checkMaxPlayers(bad);
    assert.equal(r.ok, false, `accepted ${JSON.stringify(bad)}`);
    assert.equal(r.error, 'max_players must be a whole number from 1 to 10,000');
  }
  assert.equal(MAX_PLAYERS_MESSAGE, 'max_players must be a whole number from 1 to 10,000');
  assert.equal(DEFAULT_MAX_PLAYERS, 10);
});

test('the listing bound is the live heartbeat\'s own bound: one number in two places that must not drift', () => {
  assert.equal(MAX_PLAYERS_LIMIT, MAX_PLAYERS);
});

// ── create ───────────────────────────────────────────────────────────────────

async function seededEnv() {
  const env = fakeEnv();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author', created_at: '2026-01-01T00:00:00.000Z' }));
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  return env;
}

async function create(env, body) {
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/simulations/publish', {
    method: 'POST', headers: { Authorization: `Bearer ${author}`, 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

const simCount = async (env) => Number((await env.SOCIAL.get(`simCount:${AUTHOR}`)) || 0);

test('create stores the number the Space authored, and 10 when none was sent', async () => {
  const env = await seededEnv();
  const four = await create(env, { name: 'Box Head', max_players: 4 });
  assert.equal(four.status, 201, JSON.stringify(four.body));
  assert.equal(four.body.max_players, 4);
  assert.equal(JSON.parse(await env.SOCIAL.get(`sim:${four.body.id}`)).max_players, 4, 'and what is stored is the same');

  for (const body of [{ name: 'Default' }, { name: 'Null', max_players: null }]) {
    const r = await create(env, body);
    assert.equal(r.status, 201, JSON.stringify(r.body));
    assert.equal(r.body.max_players, 10, JSON.stringify(body));
  }
  const edge = await create(env, { name: 'Big', max_players: 10_000 });
  assert.equal(edge.body.max_players, 10_000);
  const one = await create(env, { name: 'Solo', max_players: 1 });
  assert.equal(one.body.max_players, 1);
});

test('create refuses a number outside the range with the range in the message, and writes nothing', async () => {
  const env = await seededEnv();
  for (const bad of [0, -3, 10_001, 2.5, '4', true, [], {}]) {
    const r = await create(env, { name: 'Refused', max_players: bad });
    assert.equal(r.status, 400, JSON.stringify(bad));
    assert.equal(r.body.code, 'bad_max_players');
    assert.match(r.body.error, /whole number from 1 to 10,000/);
  }
  assert.equal(await simCount(env), 0, 'no listing was made');
  assert.equal([...env.SOCIAL.store?.keys?.() ?? []].filter((k) => k.startsWith('sim:')).length, 0);
});

test('the gallery shows the stored number, and 10 for a listing from before it was stored', async () => {
  const env = await seededEnv();
  const made = await create(env, { name: 'Box Head', max_players: 4 });
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${made.body.id}`));
  // Rated all_ages: a listing with no rating is adult content, shown only with Adult content on (src/adult_content.mjs).
  await env.SOCIAL.put(`sim:${made.body.id}`, JSON.stringify({ ...sim, moderation: { status: 'approved', rating: 'all_ages' } }));
  const read = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${made.body.id}`), env, ctx);
  assert.equal((await read.json()).max_players, 4);
  const { max_players, ...legacy } = JSON.parse(await env.SOCIAL.get(`sim:${made.body.id}`));
  await env.SOCIAL.put(`sim:${made.body.id}`, JSON.stringify(legacy));
  const old = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${made.body.id}`), env, ctx);
  assert.equal((await old.json()).max_players, 10);
});

// ── commit ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); } };
}

function r2() {
  const m = new Map();
  const bytesOf = async (value) => {
    if (value instanceof Uint8Array) return value;
    if (typeof value === 'string') return new TextEncoder().encode(value);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  return {
    m,
    async put(key, value) { m.set(key, await bytesOf(value)); return { key }; },
    async head(key) { const b = m.get(key); return b ? { key, size: b.length } : null; },
    async get(key) { const b = m.get(key); return b ? { size: b.length, body: new Response(b).body, arrayBuffer: async () => b.slice().buffer, text: async () => new TextDecoder().decode(b) } : null; },
    async delete(key) { m.delete(key); },
    async list({ prefix = '' } = {}) { return { objects: [...m.keys()].filter((k) => k.startsWith(prefix)).map((key) => ({ key, size: m.get(key).length })), truncated: false }; },
  };
}

const json = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const deps = {
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace(/^Bearer /, '') || null,
  requireAdmin: async () => null,
  json, isListable, canServe,
  readSpaceNames: (list) => (Array.isArray(list) ? [...new Set(list)] : null),
  MODERATION_VERSION: '1.0', POLICY_VERSION: '1.2',
};

const SIM = '11111111-2222-3333-4444-555555555555';
const H1 = 'a'.repeat(64);
function chunk() {
  const path = new TextEncoder().encode('Workspace/P/_instance.toml');
  const data = new TextEncoder().encode('[metadata]\nclass_name = "Part"\n');
  const out = new Uint8Array(12 + 8 + path.length + data.length);
  const view = new DataView(out.buffer);
  out.set([0x45, 0x43, 0x48, 0x4b]);
  view.setUint32(4, 1, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, path.length, true);
  out.set(path, 16);
  view.setUint32(16 + path.length, data.length, true);
  out.set(data, 20 + path.length);
  return out;
}
const C1 = chunk();
const manifest = { format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Box Head', start_space: 'Main', spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: C1.length, count: 1, blake3: H1 }] }], assets: [] };

async function worldEnv(sim = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2() };
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Box Head', author_id: 'author', is_public: true, version: 1, max_players: 10, moderation: { status: 'pending' }, ...sim }));
  await env.SCENES.put(`universes/${SIM}/chunks/${H1}.echk`, C1);
  return env;
}
const stored = (env) => JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
async function commit(env, listing) {
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/commit`);
  const body = JSON.stringify({ manifest_json: JSON.stringify(manifest), ...(listing === undefined ? {} : { listing }) });
  const res = await handleWorldRoute(new Request(url, { method: 'POST', headers: { Authorization: 'Bearer author', 'Content-Type': 'application/json' }, body }), url, env, {}, deps);
  return { status: res.status, body: await res.json() };
}

test('a whole-Universe commit stores the number in its listing, and a commit without it leaves the number as it was', async () => {
  const env = await worldEnv();
  const first = await commit(env, { name: 'Box Head', max_players: 4 });
  assert.equal(first.status, 200, JSON.stringify(first.body));
  assert.equal(stored(env).max_players, 4);
  const again = await commit(env, { name: 'Box Head' });
  assert.equal(again.status, 200);
  assert.equal(stored(env).max_players, 4, 'not sent: unchanged');
  const none = await commit(env);
  assert.equal(none.status, 200);
  assert.equal(stored(env).max_players, 4, 'no listing at all: unchanged');
  const asNull = await commit(env, { max_players: null });
  assert.equal(asNull.status, 200);
  assert.equal(stored(env).max_players, 4, 'null reads as not sent');
  assert.equal((await commit(env, { max_players: 10_000 })).status, 200);
  assert.equal(stored(env).max_players, 10_000);
  assert.equal((await commit(env, { max_players: 1 })).status, 200);
  assert.equal(stored(env).max_players, 1);
});

test('a number outside the range refuses the whole commit with the range, before anything is written', async () => {
  const env = await worldEnv({ max_players: 4, moderation: { status: 'approved' } });
  const before = env.SOCIAL.m.get(`sim:${SIM}`);
  const putsBefore = env.SCENES.m.size;
  for (const bad of [0, -1, 10_001, 3.5, '4', true, []]) {
    const r = await commit(env, { name: 'Renamed', max_players: bad });
    assert.equal(r.status, 400, JSON.stringify(bad));
    assert.equal(r.body.code, 'bad_max_players');
    assert.match(r.body.error, /whole number from 1 to 10,000/);
  }
  assert.equal(env.SOCIAL.m.get(`sim:${SIM}`), before, 'the listing is exactly as it was: not even the name changed');
  assert.equal(env.SCENES.m.size, putsBefore, 'and no manifest was stored');
});

test('changing the number alone does not send an approved listing back to review, but changing the text still does', async () => {
  const env = await worldEnv({ max_players: 4 });
  // The world is committed once and approved; then the same world and the same text with a new number.
  assert.equal((await commit(env, { name: 'Box Head', description: '', max_players: 4 })).status, 200);
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...stored(env), moderation: { status: 'approved' } }));
  assert.equal((await commit(env, { name: 'Box Head', description: '', max_players: 8 })).status, 200);
  assert.equal(stored(env).max_players, 8);
  assert.equal(stored(env).moderation.status, 'approved', 'a number about hosting is not content a reviewer reads');
  // Text that changes does go back.
  assert.equal((await commit(env, { name: 'Box Head 2', max_players: 8 })).status, 200);
  assert.equal(stored(env).moderation.status, 'pending');
});

test('a session that counts its host holds as many as authored: the heartbeat takes players equal to max_players', async () => {
  // From build 19 both numbers count everyone, the host's own character included: 2 of 4 with the host as one of the two.
  const env = fakeEnv();
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  const SIMID = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.SOCIAL.put(`sim:${SIMID}`, JSON.stringify({ id: SIMID, name: 'Box Head', author_id: AUTHOR, is_public: true, max_players: 4, moderation: { status: 'approved', rating: 'all_ages' } }));
  const link = `eustress-player://join/192.168.1.20:7777?key=${'0123456789abcdef'.repeat(2)}&pin=${'ab'.repeat(32)}`;
  const beat = async (players, max) => worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIMID}/live`, {
    method: 'POST', headers: { Authorization: `Bearer ${author}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ link, players, max_players: max, protocol: 3 }),
  }), env, ctx);
  assert.equal((await beat(2, 4)).status, 200);
  const live = await (await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIMID}/live`, { headers: { Authorization: `Bearer ${mintToken('22222222-2222-4222-8222-222222222222')}` } }), env, ctx)).json();
  assert.equal(live.players, 1, 'the heartbeat claims 2; nobody was admitted, so the page shows the host alone (docs/networking/LIVE_INDEX.md, Players)');
  assert.equal(live.max_players, 4);
  assert.equal((await beat(4, 4)).status, 200, 'full is 4 of 4');
  assert.equal((await beat(10_000, 10_000)).status, 200, 'the bound itself');
  assert.equal((await beat(1, 10_001)).status, 400, 'past the bound is refused, as before');
});
