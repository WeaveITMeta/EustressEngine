// Eustress-hosted sessions (src/fleet.mjs, src/fleet_credentials.mjs, the eshs_ check in src/live.mjs): the host
// credential, the host heartbeat (Route 1), the allocation (Route 2), and the hosted-session credential, against the
// real Fleet and LiveHost Durable Objects in memory. docs/architecture/HOSTED_FLEET.md is the contract.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Fleet, handleAllocate, handleFleetAdmin, handleFleetHeartbeat, checkHeartbeat } from '../src/fleet.mjs';
import { LiveHost, delistServer, handleLive, handleServerCredential } from '../src/live.mjs';
import { makeIdentityTicket } from '../src/identity.mjs';
import { fakeDoNamespace, fakeEnv, fakeKv, loadWorker, mintToken, ctx, JWT_SECRET } from './helpers/load_worker.mjs';

const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM2 = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const HIDDEN = 'cccccccc-3333-4333-8333-cccccccccccc';
const KEY = '0123456789abcdef0123456789abcdef';
const ORIGIN = 'https://api.eustress.dev';
const linkFor = (n) => `eustress-player://join/203.0.113.7:${7700 + n}?key=${KEY}&pin=${n.toString(16).padStart(2, '0').repeat(32)}`;
const pinFor = (n) => n.toString(16).padStart(2, '0').repeat(32);

let clock = Date.UTC(2026, 10, 16, 18, 0, 0);
const realNow = Date.now;
test.before(() => { Date.now = () => clock; });
test.after(() => { Date.now = realNow; });

/// A listing's committed world and its source world, as src/world.mjs writes them: the fleet hosts only a listing with a
/// source world (src/world.mjs hasSourceWorld).
const worldOf = (id, { source = true } = {}) => {
  const manifest = `universes/${id}/manifests/${'1'.repeat(64)}.json`;
  return {
    format: 'echk', r2_key: manifest,
    world: { manifest_key: manifest, manifest_sha256: '1'.repeat(64), ...(source ? { source_manifest_key: `universes/${id}/manifests/${'2'.repeat(64)}.json`, source_manifest_sha256: '2'.repeat(64) } : {}) },
  };
};

function environment() {
  const SOCIAL = fakeKv();
  const env = {
    SOCIAL,
    LIVE_HOSTS: fakeDoNamespace(LiveHost, { JWT_SECRET }),
    // The fleet object reads the listing before it sends a start, as the deployed one does through the same binding.
    FLEET: fakeDoNamespace(Fleet, { SOCIAL }),
  };
  const sim = (id, extra) => env.SOCIAL.store.set(`sim:${id}`, JSON.stringify({
    id, name: 'Pong', author_id: 'author', is_public: true, max_players: 2,
    moderation: { status: 'approved', rating: 'all_ages' }, ...worldOf(id), ...extra,
  }));
  sim(SIM);
  sim(SIM2);
  sim(HIDDEN, { moderation: { status: 'pending', rating: 'all_ages' } });
  return env;
}

// An account id is plain lowercase; a credential of any kind is never an account (as verifyAuth reads them).
const deps = {
  verifyAuth: async (request) => {
    const h = request.headers.get('Authorization') || '';
    const t = h.startsWith('Bearer ') ? h.slice(7) : '';
    return /^[a-z][a-z0-9-]*$/.test(t) ? t : null;
  },
  requireAdmin: async (request) => ((request.headers.get('Authorization') || '') === 'Bearer admin' ? 'admin' : null),
  json: (value, status, headers) => new Response(JSON.stringify(value), { status, headers }),
  isListable: (sim) => !!sim && sim.is_public !== false && sim.moderation?.status === 'approved',
};

const reply = async (res) => ({ status: res.status, body: await res.json(), headers: res.headers });
const req = (method, url, { token, body } = {}) => new Request(`${ORIGIN}${url}`, {
  method,
  headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body !== undefined ? { 'content-type': 'application/json' } : {}) },
  body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)),
});

async function mint(env, machine = 'm1', region = 'us-east-1', token = 'admin') {
  const path = `/api/admin/fleet/hosts/${machine}/credential`;
  return reply(await handleFleetAdmin(req('POST', path, { token, body: { region } }), env, {}, path, deps));
}
const beat = (seq, sessions = [], extra = {}) => ({
  boot_id: 'boot-1', seq, machine: { threads: 4, memory_mb: 8192, region: 'us-east-1', draining: false },
  sessions, freed: [], acked: [], rejected: [], agent_version: '0.1.0', ...extra,
});
const hb = async (env, token, body, machine = 'm1') => reply(await handleFleetHeartbeat(
  req('POST', `/api/fleet/hosts/${machine}/heartbeat`, { token, body }), env, {}, machine, deps,
));
const alloc = async (env, user, requestId, sim = SIM, extra = {}) => reply(await handleAllocate(
  req('POST', `/api/simulations/${sim}/allocate`, { token: user, body: { request_id: requestId, ...extra } }), env, {}, sim, deps,
));
const live = async (env, method, token, { body, query = '', sim = SIM, admit = false } = {}) => reply(await handleLive(
  req(method, `/api/simulations/${sim}/live${admit ? '/admit' : ''}${query}`, { token, body }), env, {}, sim, deps,
));
const session = (id, listing, players, extra = {}) => ({
  id, listing_id: listing, phase: 'hosting', players, reserved: 0, max_players: 2, started_at: Math.floor(clock / 1000) - 1000,
  empty_since: players > 0 ? null : Math.floor(clock / 1000) - 1000, port: 7777, ...extra,
});
const fleetStore = (env) => env.FLEET.instances.get('fleet').storage.map;
/// The server ids the listing's page shows.
const listedIds = async (env) => ((await live(env, 'GET', 'alice')).body.servers || []).map((s) => s.id).sort();

/// A machine with a host credential and one heartbeat in.
async function machineUp(env, machine = 'm1', sessions = [], threads = 4) {
  const { body } = await mint(env, machine);
  const first = await hb(env, body.token, { ...beat(1, sessions), machine: { threads, memory_mb: 8192, region: 'us-east-1', draining: false } }, machine);
  assert.equal(first.status, 200, JSON.stringify(first.body));
  return body.token;
}

/// Allocate, then the start command the machine's next heartbeat carries.
async function startOne(env, token, user = 'alice', requestId = 'r1', seq = 2) {
  const a = await alloc(env, user, requestId);
  assert.equal(a.status, 202, JSON.stringify(a.body));
  const h = await hb(env, token, beat(seq));
  const start = h.body.commands.find((c) => c.do === 'start' && c.server_id === a.body.server_id);
  assert.ok(start, JSON.stringify(h.body));
  return { serverId: a.body.server_id, start };
}

// -----------------------------------------------------------------------------
// The host credential
// -----------------------------------------------------------------------------

test('an administrator mints a host credential; only its hash is kept, and minting again revokes the last', async () => {
  const env = environment();
  assert.equal((await mint(env, 'm1', 'us-east-1', 'alice')).status, 403, 'not an administrator');
  const first = await mint(env);
  assert.equal(first.status, 200);
  assert.equal(first.headers.get('Cache-Control'), 'no-store');
  assert.match(first.body.token, /^ehost_[0-9a-f]{64}$/);
  const stored = JSON.stringify([...env.SOCIAL.store.entries()]);
  assert.ok(!stored.includes(first.body.token.slice(6)), 'the token itself is never stored');
  assert.equal((await hb(env, first.body.token, beat(1))).status, 200);

  const firstRecord = [...env.SOCIAL.store.entries()].find(([k]) => k.startsWith('hostcred:'));
  const second = await mint(env);
  assert.equal((await hb(env, first.body.token, beat(2))).status, 401, 'the replaced credential is refused');
  // A KV location that still serves the replaced record (KV can trail a delete) does not bring it back: only the hash the
  // machine names now is accepted.
  env.SOCIAL.store.set(...firstRecord);
  assert.equal((await hb(env, first.body.token, beat(2))).status, 401, 'a stale record of the replaced credential is refused');
  assert.equal((await hb(env, second.body.token, beat(2))).status, 200);

  const path = '/api/admin/fleet/hosts/m1/credential';
  assert.equal((await reply(await handleFleetAdmin(req('DELETE', path, { token: 'admin' }), env, {}, path, deps))).body.revoked, true);
  assert.equal((await hb(env, second.body.token, beat(3))).status, 401, 'a revoked credential is refused');
});

test('the heartbeat refuses anything but the machine\'s own ehost_, and says nothing about machines when it does', async () => {
  const env = environment();
  const token = await machineUp(env, 'm1', [session('s1', SIM, 1)]);
  const other = (await mint(env, 'm2')).body.token;
  for (const bad of [undefined, 'alice', `ehost_${'ab'.repeat(32)}`, `esrv_${token.slice(6)}`, `eshs_${token.slice(6)}`, `${token}x`]) {
    const r = await hb(env, bad, beat(9));
    assert.equal(r.status, 401, String(bad));
    assert.deepEqual(Object.keys(r.body).sort(), ['code', 'error']);
    assert.equal(r.body.code, 'bad_host_credential');
  }
  // Checked before the body is read: an unauthenticated garbage body is still only a 401.
  assert.equal((await hb(env, undefined, '{not json')).status, 401);
  const wrong = await hb(env, other, beat(1), 'm1');
  assert.equal(wrong.status, 403);
  assert.equal(wrong.body.code, 'wrong_machine');
  assert.equal(fleetStore(env).get('m:m1').seq, 1, 'nothing of m1 changed');
});

test('a host credential is not a /live credential', async () => {
  const env = environment();
  const token = await machineUp(env);
  const r = await live(env, 'POST', token, { body: { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: 's1' } });
  assert.equal(r.status, 401);
  assert.deepEqual((await live(env, 'GET', 'alice')).body, { live: false });
});

test('a heartbeat body out of range is a 400 naming the field', async () => {
  const env = environment();
  const token = (await mint(env)).body.token;
  assert.equal((await hb(env, token, '{not json')).body.field, 'body');
  assert.equal((await hb(env, token, { ...beat(1), machine: { threads: 0, memory_mb: 8192 } })).body.field, 'machine.threads');
  assert.equal((await hb(env, token, { ...beat(1), machine: { threads: 4, memory_mb: 8192, region: 'eu-west-1' } })).body.field, 'machine.region');
  assert.equal((await hb(env, token, beat(1, [session('../x', SIM, 1)]))).body.field, 'sessions[0].id');
  assert.equal((await hb(env, token, beat(1, [], { acked: ['rm -rf'] }))).body.field, 'acked[0]');
  assert.equal(checkHeartbeat(beat(1), 'us-east-1').ok, true);
});

// -----------------------------------------------------------------------------
// Allocation and the start
// -----------------------------------------------------------------------------

test('a Play click starts a session on the machine, the start carries a fresh eshs_, and the same request_id gets the same session', async () => {
  const env = environment();
  const token = await machineUp(env);
  const { serverId, start } = await startOne(env, token);
  assert.match(serverId, /^[A-Za-z0-9_-]{1,64}$/);
  assert.equal(start.listing_id, SIM);
  assert.equal(start.max_players, 2);
  assert.equal(start.for_request, 'r1');
  assert.match(start.session_credential, /^eshs_[0-9a-f]{64}$/);

  const again = await alloc(env, 'alice', 'r1');
  assert.equal(again.status, 202);
  assert.equal(again.body.server_id, serverId, 'the same request_id, the same session');
  assert.equal(again.body.retry_after_secs, 5);

  // Acked, and the session reported: no resend.
  const acked = await hb(env, token, beat(3, [session(serverId, SIM, 0, { phase: 'starting', empty_since: null })], { acked: [start.command_id] }));
  assert.deepEqual(acked.body.commands, []);
  assert.equal(acked.body.next_heartbeat_secs, 5);

  // The session publishes its link through /live with its own credential; the 202 becomes a 200.
  const beatLive = await live(env, 'POST', start.session_credential, { body: { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId } });
  assert.equal(beatLive.status, 200, JSON.stringify(beatLive.body));
  const joined = await alloc(env, 'alice', 'r1');
  assert.equal(joined.status, 200);
  assert.deepEqual(joined.body, { status: 'join', link: linkFor(1), cert_sha256: pinFor(1), server_id: serverId, warm: false });

  // No secret is stored anywhere: not the eshs_, not the ehost_.
  const everything = JSON.stringify([...fleetStore(env).entries()]) + JSON.stringify([...env.SOCIAL.store.entries()]);
  assert.ok(!everything.includes(start.session_credential.slice(5)), 'the eshs_ is not stored');
  assert.ok(!everything.includes(token.slice(6)), 'the ehost_ is not stored');
});

test('allocation has GET /live\'s gates: signed out, a hidden listing, a bad request_id', async () => {
  const env = environment();
  await machineUp(env);
  assert.equal((await alloc(env, undefined, 'r1')).status, 401);
  assert.equal((await alloc(env, 'alice', 'r1', HIDDEN)).status, 404, 'an unapproved listing is its author\'s only');
  assert.equal((await alloc(env, 'author', 'r1', HIDDEN)).status, 202);
  assert.equal((await alloc(env, 'alice', 'r1', 'dddddddd-0000-4000-8000-000000000000')).status, 404);
  for (const bad of ['', 'a b', 'x'.repeat(65), 7]) assert.equal((await alloc(env, 'alice', bad)).body.code, 'bad_request_id', String(bad));
  assert.equal((await alloc(env, 'alice', 'r9', SIM, { region: 'us east' })).body.code, 'bad_region');
  // The same id for another listing is not a key to that listing's answer.
  await alloc(env, 'bob', 'r2');
  assert.equal((await alloc(env, 'bob', 'r2', SIM2)).status, 409);
});

test('a running session with players and a free seat is taken before anything starts (the Quick Join rank)', async () => {
  const env = environment();
  // The author's own host has a player and a free seat.
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(9), players: 1, max_players: 4, protocol: 3, server_id: 'home' } })).status, 200);
  await machineUp(env);
  const r = await alloc(env, 'alice', 'r1');
  assert.equal(r.status, 200);
  assert.equal(r.body.server_id, 'home');
  assert.equal(r.body.link, linkFor(9));
  assert.deepEqual([...fleetStore(env).keys()].filter((k) => k.startsWith('c:')), [], 'nothing was started');
});

test('the 80% line: a 4-thread machine with 4 occupied sessions starts nothing more and recommends a machine', async () => {
  const env = environment();
  const full = [1, 2, 3, 4].map((n) => session(`o${n}`, SIM2, 2));
  const token = await machineUp(env, 'm1', full);
  const refused = await alloc(env, 'alice', 'r1');
  assert.equal(refused.status, 503);
  assert.deepEqual(refused.body, { status: 'full', code: 'no_capacity', retry_after_secs: 30 });
  const status = await reply(await handleFleetAdmin(req('GET', '/api/admin/fleet', { token: 'admin' }), env, {}, '/api/admin/fleet', deps));
  assert.equal(status.body.advice.actions[0].action, 'recommend_add_machine');
  assert.equal(status.body.machines[0].capacity.committed_millis, 3200);

  // Three occupied: the fourth fits.
  await hb(env, token, beat(2, full.slice(0, 3)));
  assert.equal((await alloc(env, 'alice', 'r2')).status, 202);
});

test('the activation line: warm servers that could all go to 60 Hz at once leave no room past 90% of the threads', async () => {
  const env = environment();
  // Three warm servers on 4 threads: committed is 189 of 3200, and a fourth session makes 4 x 800 = 3200, under 3600.
  const threeWarm = [1, 2, 3].map((n) => session(`w${n}`, SIM2, 0));
  await machineUp(env, 'm1', threeWarm);
  assert.equal((await alloc(env, 'alice', 'r1')).status, 202, 'the fourth session fits: 4 x 800 = 3200 <= 3600');
  assert.equal((await alloc(env, 'alice', 'r2', SIM, {})).status, 202, 'r1\'s session is starting: r2 takes its other seat');
  assert.equal((await alloc(env, 'bob', 'r3')).status, 503, 'a fifth would make 5 x 800 = 4000, past 90% of 4 threads (3600)');
});

test('a warm server is assigned before a start, and the answer says warm', async () => {
  const env = environment();
  // Started 10 s ago: inside its hold, so the plan keeps it.
  const now = Math.floor(clock / 1000);
  const token = await machineUp(env, 'm1', [session('w1', SIM, 0, { started_at: now - 10, empty_since: now - 10 })]);
  // Its /live entry: started by the fleet earlier, so it holds an eshs_; here the author's session stands in for it.
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(3), players: 0, max_players: 2, protocol: 3, server_id: 'w1' } })).status, 200);
  const r = await alloc(env, 'alice', 'r1');
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.server_id, 'w1');
  assert.equal(r.body.warm, true);
  // The seat is reserved: the next player shares it, then the session is full and a start follows.
  assert.equal((await alloc(env, 'bob', 'r2')).body.server_id, 'w1');
  const third = await alloc(env, 'carol', 'r3');
  assert.equal(third.status, 202);
  assert.notEqual(third.body.server_id, 'w1');
  void token;
});

test('the fleet starts no session for a listing with no source world; the creator\'s own host of it is still joined', async () => {
  const env = environment();
  const NOSRC = 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee';
  env.SOCIAL.store.set(`sim:${NOSRC}`, JSON.stringify({
    id: NOSRC, name: 'Pong', author_id: 'author', is_public: true, max_players: 2,
    moderation: { status: 'approved', rating: 'all_ages' }, ...worldOf(NOSRC, { source: false }),
  }));
  await machineUp(env);
  const refused = await alloc(env, 'alice', 'r1', NOSRC);
  assert.equal(refused.status, 409, JSON.stringify(refused.body));
  assert.equal(refused.body.code, 'no_source_world');
  assert.equal(refused.headers.get('Cache-Control'), 'no-store');
  assert.deepEqual([...fleetStore(env).keys()].filter((k) => k.startsWith('c:') || k.startsWith('p:')), [], 'nothing was planned');
  assert.deepEqual((fleetStore(env).get('recent') || []).filter((r) => r.listing_id === NOSRC), [], 'no demand the warm pool would answer');
  // A listing published as a .pak has no source world either.
  env.SOCIAL.store.set(`sim:${NOSRC}`, JSON.stringify({ ...JSON.parse(env.SOCIAL.store.get(`sim:${NOSRC}`)), format: 'pak', r2_key: `universes/${NOSRC}/universe.pak` }));
  assert.equal((await alloc(env, 'alice', 'r2', NOSRC)).body.code, 'no_source_world');
  // The author hosts it from their own computer: a player is sent there, as Quick Join would.
  assert.equal((await live(env, 'POST', 'author', { sim: NOSRC, body: { link: linkFor(9), players: 1, max_players: 4, protocol: 3, server_id: 'home' } })).status, 200);
  const joined = await alloc(env, 'alice', 'r3', NOSRC);
  assert.equal(joined.status, 200, JSON.stringify(joined.body));
  assert.equal(joined.body.server_id, 'home');
});

test('the fleet starts no session for a listing that is deleted or under a legal lock, even for its author', async () => {
  for (const [key, meta] of [[`locks/legal/${SIM}`, { kind: 'content_lock' }], [`locks/deleted/${SIM}.json`, {}]]) {
    const env = environment();
    env.SCENES = { head: async (k) => (k === key ? { key: k, size: 2, customMetadata: meta } : null) };
    await machineUp(env);
    const r = await alloc(env, 'author', 'r1');
    assert.equal(r.status, 409, `${key}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'not_hostable');
    assert.deepEqual([...fleetStore(env).keys()].filter((k) => k.startsWith('p:')), [], `${key}: nothing was planned`);
  }
});

test('a start for a listing whose source world is gone by the next heartbeat is never sent', async () => {
  const env = environment();
  const token = await machineUp(env);
  const a = await alloc(env, 'alice', 'r1');
  assert.equal(a.status, 202);
  // A new world is committed and its source is not (yet): the listing has no source world when the start would go out.
  const listing = JSON.parse(env.SOCIAL.store.get(`sim:${SIM}`));
  env.SOCIAL.store.set(`sim:${SIM}`, JSON.stringify({ ...listing, ...worldOf(SIM, { source: false }) }));
  const h = await hb(env, token, beat(2));
  assert.deepEqual(h.body.commands.filter((c) => c.do === 'start'), [], JSON.stringify(h.body));
  assert.equal(fleetStore(env).get(`p:${a.body.server_id}`).failed, true, 'the planned session failed');
  assert.deepEqual([...fleetStore(env).keys()].filter((k) => k.startsWith('hostsess:')), [], 'no eshs_ was minted');
});

test('the Quick Join order decides: an empty fleet session ranked above an empty creator host gets the player', async () => {
  const env = environment();
  const now = Math.floor(clock / 1000);
  await machineUp(env, 'm1', [session('w1', SIM, 0, { started_at: now - 10, empty_since: now - 10 })]);
  // Both empty. The creator's host is low tier at 250 ms; the fleet session is high tier at 20 ms, so it ranks first.
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(9), players: 0, max_players: 2, protocol: 3, server_id: 'home', latency_ms: 250, hardware: { gpu: '', cpu: 'Celeron', tier: 'low' } } })).status, 200);
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(3), players: 0, max_players: 2, protocol: 3, server_id: 'w1', latency_ms: 20, hardware: { gpu: '', cpu: 'EPYC', tier: 'high' } } })).status, 200);
  const ranked = (await live(env, 'GET', 'alice')).body.servers.map((s) => s.id);
  assert.deepEqual(ranked, ['w1', 'home'], 'Quick Join ranks the fleet session first');
  const r = await alloc(env, 'alice', 'r1');
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.server_id, 'w1', 'allocation sends the player where Quick Join would');
  assert.equal(r.body.warm, true);
  // The other way round, the creator's host ranks first and gets the player: who runs a host never enters the order.
  const env2 = environment();
  await machineUp(env2, 'm1', [session('w1', SIM, 0, { started_at: now - 10, empty_since: now - 10 })]);
  assert.equal((await live(env2, 'POST', 'author', { body: { link: linkFor(9), players: 0, max_players: 2, protocol: 3, server_id: 'home', latency_ms: 20, hardware: { gpu: '', cpu: 'EPYC', tier: 'high' } } })).status, 200);
  assert.equal((await live(env2, 'POST', 'author', { body: { link: linkFor(3), players: 0, max_players: 2, protocol: 3, server_id: 'w1', latency_ms: 250, hardware: { gpu: '', cpu: 'Celeron', tier: 'low' } } })).status, 200);
  assert.equal((await alloc(env2, 'alice', 'r1')).body.server_id, 'home');
});

test('a reserved seat is counted once: after the listing admits its player, the heartbeat of the host owns the seat', async () => {
  const env = environment();
  // Alice's click starts a 2-seat session and reserves a seat in it for 60 s.
  const { token, serverId, eshs } = await hostedSession(env);
  const hosting = (players) => session(serverId, SIM, players, { started_at: Math.floor(clock / 1000) - 30, empty_since: null });
  const liveBeat = (players) => live(env, 'POST', eshs, { body: { link: linkFor(1), players, max_players: 2, protocol: 3, server_id: serverId } });
  assert.equal((await liveBeat(0)).status, 200);
  await hb(env, token, beat(4, [hosting(0)]));
  assert.equal((await alloc(env, 'alice', 'r1')).status, 200, 'the session hosts: Alice is sent its link');

  // Alice connects, and the session asks the listing to admit her, with its own eshs_.
  const { ticket } = await makeIdentityTicket({ JWT_SECRET }, { accountId: 'alice', username: 'alice', audience: pinFor(1), now: Math.floor(clock / 1000) });
  const admitted = await live(env, 'POST', eshs, { admit: true, body: { ticket, audience: pinFor(1) } });
  assert.equal(admitted.status, 200, JSON.stringify(admitted.body));
  assert.equal(admitted.body.account_id, 'alice');
  const reservation = fleetStore(env).get('r:u:alice:r1');
  assert.equal(reservation.arrived_at, clock, 'the fleet learns she arrived');
  assert.ok(reservation.reserved_until > clock, 'the seat stays held until the host reports her');

  // Until the machine heartbeats after the admission the seat is still the reservation's: nothing is undercounted.
  const fullFor = async (user, requestId) => (await alloc(env, user, requestId)).body.server_id !== serverId;
  assert.equal(await fullFor('carol', 'r8'), false, 'one seat reserved, one free: Carol shares the session');
  assert.equal(await fullFor('dave', 'r9'), true, 'Alice arriving and Carol reserved: no third seat');
  fleetStore(env).delete('r:u:carol:r8');

  // Five seconds on, the host reports Alice as a player. Her seat is counted once, so the second seat is free for Bob,
  // well inside the 60 s her reservation would otherwise have kept counting beside her.
  clock += 5_000;
  await hb(env, token, beat(5, [hosting(1)]));
  assert.equal((await liveBeat(1)).status, 200);
  const planned = () => [...fleetStore(env).keys()].filter((k) => k.startsWith('p:')).length;
  const before = planned();
  const bob = await alloc(env, 'bob', 'r2');
  assert.equal(bob.status, 200, JSON.stringify(bob.body));
  assert.equal(bob.body.server_id, serverId, 'Bob joins Alice instead of a second session starting');
  assert.equal(planned(), before, 'nothing new was started for Bob');

  // An admission by any other caller, or for another server, marks nothing.
  const object = env.FLEET.get('fleet');
  const arrived = async (args) => (await object.fetch('https://fleet.internal/arrived', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args) })).json();
  assert.equal((await arrived({ who: 'u:bob', server_id: 'another-server', now: clock })).marked, 0);
  assert.equal((await arrived({ who: 'bob', server_id: serverId, now: clock })).ok, false, 'who is u:<account>');
  assert.equal((await arrived({ who: 'u:bob', server_id: serverId, now: clock })).marked, 1);
  assert.equal((await arrived({ who: 'u:bob', server_id: serverId, now: clock + 1 })).marked, 0, 'once');
});

test('ten new allocations a minute per account; asking again with a known request_id is not counted', async () => {
  const env = environment();
  await machineUp(env, 'm1', [], 64);
  for (let i = 0; i < 10; i++) assert.notEqual((await alloc(env, 'alice', `r${i}`)).status, 429, `r${i}`);
  for (let i = 0; i < 5; i++) assert.notEqual((await alloc(env, 'alice', 'r0')).status, 429, 'a poll of a known id');
  const limited = await alloc(env, 'alice', 'r10');
  assert.equal(limited.status, 429);
  assert.equal(limited.body.code, 'rate_limited');
  assert.notEqual((await alloc(env, 'bob', 'r10')).status, 429, 'another account has its own minute');
  clock += 61_000;
  assert.notEqual((await alloc(env, 'alice', 'r10')).status, 429, 'a minute later');
});

test('a machine that stops heartbeating gets nothing placed on it', async () => {
  const env = environment();
  const token = await machineUp(env);
  clock += 31_000;
  assert.equal((await alloc(env, 'alice', 'r1')).status, 503, 'no heartbeat for 30 s');
  await hb(env, token, beat(2));
  assert.equal((await alloc(env, 'alice', 'r2')).status, 202, 'back once it beats');
});

test('a start the host rejects, or a host that restarts, is placed again on the next ask with the same request_id', async () => {
  const env = environment();
  const token = await machineUp(env);
  const { serverId, start } = await startOne(env, token);
  await hb(env, token, beat(3, [], { rejected: [{ command_id: start.command_id, why: 'would pass the activation line' }] }));
  // Its credential went with it.
  assert.equal((await live(env, 'POST', start.session_credential, { body: { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId } })).status, 401);
  const again = await alloc(env, 'alice', 'r1');
  assert.equal(again.status, 202);
  assert.notEqual(again.body.server_id, serverId, 'host-failed: placed again');
  const resend = await hb(env, token, beat(4));
  assert.ok(resend.body.commands.some((c) => c.do === 'start' && c.server_id === again.body.server_id));

  // A new boot_id: the agent forgot its commands, and the session it never reported is placed again.
  const rebooted = await hb(env, token, { ...beat(1), boot_id: 'boot-2' });
  assert.equal(rebooted.status, 200);
  const third = await alloc(env, 'alice', 'r1');
  assert.equal(third.status, 202);
  assert.notEqual(third.body.server_id, again.body.server_id);
});

test('a heartbeat at or below the last seq for its boot gets no commands and changes nothing', async () => {
  const env = environment();
  const token = await machineUp(env);
  await alloc(env, 'alice', 'r1');
  const replay = await hb(env, token, beat(1));
  assert.deepEqual(replay.body, { seq: 1, commands: [], next_heartbeat_secs: 5 });
  assert.equal((await hb(env, token, beat(2))).body.commands.length, 1);
});

test('a start is resent until acked, each time with a new eshs_, and dropped 60 s later', async () => {
  const env = environment();
  const token = await machineUp(env);
  const { start } = await startOne(env, token);
  const resent = (await hb(env, token, beat(3))).body.commands[0];
  assert.equal(resent.command_id, start.command_id);
  assert.notEqual(resent.session_credential, start.session_credential);
  clock += 61_000;
  const later = (await hb(env, token, beat(4))).body.commands;
  assert.ok(!later.some((c) => c.command_id === start.command_id), JSON.stringify(later));
});

// -----------------------------------------------------------------------------
// The hosted-session credential on /live
// -----------------------------------------------------------------------------

async function hostedSession(env) {
  const token = await machineUp(env);
  const { serverId, start } = await startOne(env, token);
  await hb(env, token, beat(3, [session(serverId, SIM, 0, { phase: 'starting', empty_since: null })], { acked: [start.command_id] }));
  return { token, serverId, eshs: start.session_credential };
}

test('an eshs_ is accepted for its own listing and its own server_id only', async () => {
  const env = environment();
  const { serverId, eshs } = await hostedSession(env);
  const body = (extra = {}) => ({ link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId, ...extra });
  assert.equal((await live(env, 'POST', eshs, { body: body() })).status, 200);
  assert.equal((await live(env, 'POST', eshs, { body: body(), sim: SIM2 })).status, 401, 'another listing');
  assert.equal((await live(env, 'POST', eshs, { body: body({ server_id: 'default' }) })).status, 401, 'another server, even the default');
  assert.equal((await live(env, 'POST', eshs, { body: body({ server_id: undefined }) })).status, 400, 'a server_id is required');
  assert.equal((await live(env, 'POST', eshs, { body: body({ guest_only: true }) })).status, 403, 'never a guest-only session');
  assert.equal((await live(env, 'POST', `eshs_${'ab'.repeat(32)}`, { body: body() })).status, 401, 'a made-up credential');
  assert.equal((await live(env, 'DELETE', eshs)).status, 400, 'a stop names its server');
  assert.equal((await live(env, 'DELETE', eshs, { query: '?server_id=default' })).status, 401);
  // Admission: only for a ticket whose audience is this server's own pin. Its own pin goes on to the ticket check.
  // The route names the credential's own server to the listing's object, which holds the ticket to that server's pin.
  const named = [];
  const get = env.LIVE_HOSTS.get;
  env.LIVE_HOSTS.get = (id) => {
    const stub = get(id);
    return { fetch: (url, init) => { if (String(url).endsWith('/admit')) named.push(JSON.parse(init.body).server_id); return stub.fetch(url, init); } };
  };
  const own = await live(env, 'POST', eshs, { admit: true, body: { ticket: 'x', audience: pinFor(1) } });
  assert.equal(own.status, 403);
  assert.notEqual(own.body.code, 'wrong_host', 'its own pin reaches the ticket check');
  // Another server of the same listing, the Quick Join one, whose pin the listing's object would accept: refused here.
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(2), players: 1, max_players: 4, protocol: 3, server_id: 'home' } })).status, 200);
  const other = await live(env, 'POST', eshs, { admit: true, body: { ticket: 'x', audience: pinFor(2) } });
  assert.equal(other.status, 403);
  assert.equal(other.body.code, 'wrong_host');
  assert.deepEqual(named, [serverId], 'one admission reached the object, naming this server');
  env.LIVE_HOSTS.get = get;
  // The listing's object holds the same line: named a server, it admits only at that server's pin, though it lists both.
  const object = env.LIVE_HOSTS.get(SIM);
  const direct = async (audience, server_id) => (await object.fetch('https://live.internal/admit', {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ticket: 'x', audience, now: clock, server_id }),
  })).json();
  assert.equal((await direct(pinFor(2), serverId)).code, 'wrong_host');
  assert.notEqual((await direct(pinFor(1), serverId)).code, 'wrong_host');
  assert.notEqual((await direct(pinFor(2), undefined)).code, 'wrong_host', 'the author reaches any of its servers');
  // An eshs_ is not a host credential either.
  assert.equal((await hb(env, eshs, beat(9))).status, 401);
  // Its own stop works and removes only its own server.
  assert.equal((await live(env, 'DELETE', eshs, { query: `?server_id=${serverId}` })).status, 200);
  assert.equal((await live(env, 'GET', 'alice')).body.servers.map((s) => s.id).join(), 'home');
});

test('the esrv_ server credential works exactly as before beside the fleet', async () => {
  const env = environment();
  await hostedSession(env);
  const minted = await reply(await handleServerCredential(req('POST', `/api/simulations/${SIM}/server-credential`, { token: 'author' }), env, {}, SIM, deps));
  assert.match(minted.body.token, /^esrv_[0-9a-f]{64}$/);
  assert.equal((await live(env, 'POST', minted.body.token, { body: { link: linkFor(5), players: 1, max_players: 2, protocol: 3 } })).status, 200);
  assert.equal((await live(env, 'POST', minted.body.token, { body: { link: linkFor(5), players: 1, max_players: 2, protocol: 3 }, sim: SIM2 })).status, 401);
});

test('an eshs_ ends with its session: a stop the host acknowledged, a session reported freed, or 90 s without a /live beat', async () => {
  // Stop acknowledged: an empty server past its hold and beyond the warm pool gets a stop, and the ack revokes its credential.
  const env = environment();
  const { token, serverId, eshs } = await hostedSession(env);
  const body = { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId };
  const startedAt = Math.floor(clock / 1000);
  // Twenty-four /live beats 30 s apart, and the machine's own: the click ages out of the demand window.
  for (let i = 0; i < 24; i++) {
    assert.equal((await live(env, 'POST', eshs, { body })).status, 200, `beat ${i}`);
    clock += 30_000;
  }
  const idle = session(serverId, SIM, 0, { started_at: startedAt, empty_since: startedAt });
  assert.equal((await live(env, 'POST', eshs, { body })).status, 200);
  const stop = (await hb(env, token, beat(4, [idle]))).body.commands.find((c) => c.do === 'stop');
  assert.ok(stop, 'the surplus empty server is stopped');
  assert.equal(stop.server_id, serverId);
  assert.equal((await live(env, 'POST', eshs, { body })).status, 200, 'still valid until the stop is acknowledged');
  assert.deepEqual(await listedIds(env), [serverId], 'and still on the listing');
  await hb(env, token, beat(5, [{ ...idle, phase: 'stopping' }], { acked: [stop.command_id] }));
  assert.equal((await live(env, 'POST', eshs, { body })).status, 401, 'revoked by the acknowledged stop');
  assert.deepEqual(await listedIds(env), [], 'and off the listing at once, not 90 s later');

  // Freed: the host reports the session gone.
  const env2 = environment();
  const two = await hostedSession(env2);
  await hb(env2, two.token, beat(4, [], { freed: [{ id: two.serverId, why: 'host_failed' }] }));
  assert.equal((await live(env2, 'POST', two.eshs, { body: { ...body, server_id: two.serverId } })).status, 401, 'revoked when freed');

  // 90 s after the last accepted /live beat.
  const env3 = environment();
  const three = await hostedSession(env3);
  const b3 = { ...body, server_id: three.serverId };
  assert.equal((await live(env3, 'POST', three.eshs, { body: b3 })).status, 200);
  clock += 91_000;
  assert.equal((await live(env3, 'POST', three.eshs, { body: b3 })).status, 401, 'expired with its /live entry');
});

test('a session whose credential the fleet revoked leaves the listing at once, and only that session', async () => {
  const env = environment();
  const { token, serverId, eshs } = await hostedSession(env);
  // The author's own server and the fleet's session, both on the listing.
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(2), players: 1, max_players: 4, protocol: 3, server_id: 'home' } })).status, 200);
  assert.equal((await live(env, 'POST', eshs, { body: { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId } })).status, 200);
  assert.deepEqual(await listedIds(env), ['home', serverId].sort());
  // The host reports the session dead: its credential goes, and its entry with it.
  await hb(env, token, beat(4, [], { freed: [{ id: serverId, why: 'dead' }] }));
  assert.deepEqual(await listedIds(env), ['home']);
  // A host that names a server it never ran, then drops it, takes nothing off the listing: the listing to act on is
  // read from the credential the fleet minted, and it minted none for that id.
  await hb(env, token, beat(5, [session('home', SIM, 1)]));
  await hb(env, token, beat(6, [], { freed: [{ id: 'home', why: 'dead' }] }));
  assert.deepEqual(await listedIds(env), ['home']);
});

test('a session the host simply stops reporting leaves the listing at once too', async () => {
  const env = environment();
  const { token, serverId, eshs } = await hostedSession(env);
  assert.equal((await live(env, 'POST', eshs, { body: { link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: serverId } })).status, 200);
  assert.deepEqual(await listedIds(env), [serverId]);
  await hb(env, token, beat(4, []));
  assert.deepEqual(await listedIds(env), []);
});

test('taking a server off a listing changes nothing when the listing does not list it', async () => {
  const env = environment();
  assert.equal((await live(env, 'POST', 'author', { body: { link: linkFor(2), players: 1, max_players: 4, protocol: 3 } })).status, 200);
  // A record from before per-server rows: one host, no list of servers.
  const stored = env.LIVE_HOSTS.instances.get(SIM).storage.map;
  const host = { ...stored.get('host') };
  delete host.servers;
  stored.set('host', host);
  await delistServer(env, SIM, 'never-listed', clock);
  assert.equal((await live(env, 'GET', 'alice')).body.live, true);
});

test('a host cannot claim another machine\'s session', async () => {
  const env = environment();
  const { serverId } = await hostedSession(env);
  const t2 = await machineUp(env, 'm2');
  await hb(env, t2, beat(2, [session(serverId, SIM, 2)]), 'm2');
  assert.ok(!fleetStore(env).get('m:m2').sessions.some((s) => s.id === serverId), 'the claim is left out of m2\'s view');
  await hb(env, t2, beat(3, [], { freed: [{ id: serverId, why: 'stopped' }] }), 'm2');
  assert.equal(fleetStore(env).get(`p:${serverId}`), undefined, 'm1 still reports it; m2 cannot end it');
  assert.ok(fleetStore(env).get('m:m1').sessions.some((s) => s.id === serverId));
});

test('the alarm forgets old answers, rate windows and expired credentials', async () => {
  const env = environment();
  await hostedSession(env);
  const obj = env.FLEET.instances.get('fleet');
  assert.ok(obj.storage.alarm !== null, 'an alarm is set');
  clock += REQUEST_KEEP + 1;
  await obj.alarm();
  const keys = [...fleetStore(env).keys()];
  assert.deepEqual(keys.filter((k) => /^(r:|rate:|hostsess:)/.test(k)), []);
});
const REQUEST_KEEP = 600_000;

// -----------------------------------------------------------------------------
// The deployment and the real Worker
// -----------------------------------------------------------------------------

test('wrangler.toml binds FLEET to the Fleet class and migrates it once in each environment', () => {
  const text = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'wrangler.toml'), 'utf8').split('\r\n').join('\n');
  const at = text.indexOf('[env.staging]');
  for (const section of [text.slice(0, at), text.slice(at)]) {
    assert.match(section, /name = "FLEET"\nclass_name = "Fleet"/);
    assert.equal([...section.matchAll(/tag = "v10-fleet"\nnew_sqlite_classes = \["Fleet"\]/g)].length, 1);
  }
  const index = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'src', 'index.js'), 'utf8');
  assert.match(index, /export \{ Fleet \} from '\.\/fleet\.mjs';/);
});

test('the real Worker routes the heartbeat past its gates, and the admin routes refuse a non-administrator', async () => {
  const worker = await loadWorker();
  const env = { ...fakeEnv(), LIVE_HOSTS: fakeDoNamespace(LiveHost, { JWT_SECRET }), FLEET: fakeDoNamespace(Fleet, {}) };
  const minted = await mint(env);
  const res = await worker.fetch(new Request(`${ORIGIN}/api/fleet/hosts/m1/heartbeat`, {
    method: 'POST', headers: { Authorization: `Bearer ${minted.body.token}`, 'content-type': 'application/json' }, body: JSON.stringify(beat(1)),
  }), env, ctx);
  assert.equal(res.status, 200, await res.clone().text());
  assert.deepEqual(await res.json(), { seq: 1, commands: [], next_heartbeat_secs: 5 });
  const anon = await worker.fetch(new Request(`${ORIGIN}/api/fleet/hosts/m1/heartbeat`, { method: 'POST', body: '{}' }), env, ctx);
  assert.equal(anon.status, 401);
  const user = '99999999-9999-4999-8999-999999999991';
  const admin = await worker.fetch(new Request(`${ORIGIN}/api/admin/fleet`, { headers: { Authorization: `Bearer ${mintToken(user)}` } }), env, ctx);
  assert.equal(admin.status, 403);
  const allocate = await worker.fetch(new Request(`${ORIGIN}/api/simulations/${SIM}/allocate`, { method: 'POST', body: '{}' }), env, ctx);
  assert.equal(allocate.status, 404, 'no such listing in this env');
});
