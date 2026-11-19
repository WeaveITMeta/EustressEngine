// CPU first, GPU option (docs/architecture/GPU_HOSTING_RULE.md), at the routes: a host reports its GPU in the fleet
// heartbeat and in the /live heartbeat's `hardware`; a listing's `gpu` setting is checked and stored by
// PATCH /api/simulations/{id}/listing; and a listing that requires a GPU is started only on a fleet machine with that much
// VRAM free, or refused with `no_gpu_capacity`. Against the real Fleet and LiveHost Durable Objects in memory.

import test from 'node:test';
import assert from 'node:assert/strict';
import { Fleet, handleAllocate, handleFleetAdmin, handleFleetHeartbeat, checkHeartbeat } from '../src/fleet.mjs';
import { LiveHost, handleLive } from '../src/live.mjs';
import { checkListingEdit, applyListingEdit } from '../src/listing_edit.mjs';
import { fakeDoNamespace, fakeKv, JWT_SECRET } from './helpers/load_worker.mjs';

const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const CITY = 'dddddddd-4444-4444-8444-dddddddddddd';
const ORIGIN = 'https://api.eustress.dev';
const KEY = '0123456789abcdef0123456789abcdef';
const linkFor = (n) => `eustress-player://join/203.0.113.7:${7700 + n}?key=${KEY}&pin=${n.toString(16).padStart(2, '0').repeat(32)}`;

let clock = Date.UTC(2026, 10, 19, 18, 0, 0);
const realNow = Date.now;
test.before(() => { Date.now = () => clock; });
test.after(() => { Date.now = realNow; });

const worldOf = (id) => {
  const manifest = `universes/${id}/manifests/${'1'.repeat(64)}.json`;
  return {
    format: 'echk', r2_key: manifest,
    world: {
      manifest_key: manifest, manifest_sha256: '1'.repeat(64),
      source_manifest_key: `universes/${id}/manifests/${'2'.repeat(64)}.json`, source_manifest_sha256: '2'.repeat(64),
    },
  };
};

function environment() {
  const SOCIAL = fakeKv();
  const env = { SOCIAL, LIVE_HOSTS: fakeDoNamespace(LiveHost, { JWT_SECRET }), FLEET: fakeDoNamespace(Fleet, { SOCIAL }) };
  const sim = (id, extra) => env.SOCIAL.store.set(`sim:${id}`, JSON.stringify({
    id, name: 'Pong', author_id: 'author', is_public: true, max_players: 2,
    moderation: { status: 'approved', rating: 'all_ages' }, ...worldOf(id), ...extra,
  }));
  sim(SIM);
  sim(CITY, { name: 'City', gpu: 'required', gpu_min_vram_mb: 8192 });
  return env;
}

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

const reply = async (res) => ({ status: res.status, body: await res.json() });
const req = (method, url, { token, body } = {}) => new Request(`${ORIGIN}${url}`, {
  method,
  headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body !== undefined ? { 'content-type': 'application/json' } : {}) },
  body: body === undefined ? undefined : JSON.stringify(body),
});
async function mint(env, machine) {
  const path = `/api/admin/fleet/hosts/${machine}/credential`;
  return (await reply(await handleFleetAdmin(req('POST', path, { token: 'admin', body: { region: 'us-east-1' } }), env, {}, path, deps))).body.token;
}
const machineOf = (extra = {}) => ({ threads: 4, memory_mb: 8192, region: 'us-east-1', draining: false, ...extra });
const beat = (seq, machine, sessions = []) => ({ boot_id: 'boot-1', seq, machine, sessions, freed: [], acked: [], rejected: [], agent_version: '0.1.0' });
const hb = async (env, token, machineId, body) => reply(await handleFleetHeartbeat(
  req('POST', `/api/fleet/hosts/${machineId}/heartbeat`, { token, body }), env, {}, machineId, deps,
));
const alloc = async (env, user, requestId, sim) => reply(await handleAllocate(
  req('POST', `/api/simulations/${sim}/allocate`, { token: user, body: { request_id: requestId } }), env, {}, sim, deps,
));
async function machineUp(env, machineId, machine) {
  const token = await mint(env, machineId);
  const first = await hb(env, token, machineId, beat(1, machine));
  assert.equal(first.status, 200, JSON.stringify(first.body));
  return token;
}
const fleetStore = (env) => env.FLEET.instances.get('fleet').storage.map;

test('the fleet heartbeat: an agent that predates the GPU fields still beats; the fields are checked when sent', () => {
  const old = checkHeartbeat(beat(1, machineOf()), 'us-east-1');
  assert.equal(old.ok, true);
  assert.deepEqual(old.value.machine, {
    threads: 4, memory_mb: 8192, region: 'us-east-1', draining: false, gpu: '', gpu_vram_mb: 0, gpu_available: false,
  });
  const withGpu = checkHeartbeat(beat(1, machineOf({ gpu: 'NVIDIA GeForce RTX 2080 Ti', gpu_vram_mb: 11264, gpu_available: true })), 'us-east-1');
  assert.deepEqual([withGpu.value.machine.gpu, withGpu.value.machine.gpu_vram_mb, withGpu.value.machine.gpu_available],
    ['NVIDIA GeForce RTX 2080 Ti', 11264, true]);
  for (const [extra, field] of [
    [{ gpu: 7 }, 'machine.gpu'], [{ gpu: 'x'.repeat(161) }, 'machine.gpu'], [{ gpu_vram_mb: -1 }, 'machine.gpu_vram_mb'],
    [{ gpu_vram_mb: 1.5 }, 'machine.gpu_vram_mb'], [{ gpu_available: 'yes' }, 'machine.gpu_available'],
  ]) {
    assert.deepEqual(checkHeartbeat(beat(1, machineOf(extra)), 'us-east-1'), { ok: false, field }, JSON.stringify(extra));
  }
});

test('a listing that requires a GPU starts only on a machine with that much VRAM free, and holds it', async () => {
  const env = environment();
  await machineUp(env, 'cpu1', machineOf({ threads: 16 }));
  const gpuToken = await machineUp(env, 'gpu1', machineOf({ gpu: 'NVIDIA GeForce RTX 2080 Ti', gpu_vram_mb: 11264, gpu_available: true }));

  const first = await alloc(env, 'alice', 'r1', CITY);
  assert.equal(first.status, 202, JSON.stringify(first.body));
  const sent = await hb(env, gpuToken, 'gpu1', beat(2, machineOf({ gpu: 'NVIDIA GeForce RTX 2080 Ti', gpu_vram_mb: 11264, gpu_available: true })));
  assert.ok(sent.body.commands.some((c) => c.do === 'start' && c.server_id === first.body.server_id && c.listing_id === CITY), JSON.stringify(sent.body));
  assert.equal(fleetStore(env).get(`gv:${first.body.server_id}`), 8192, 'the session holds 8192 MB of the GPU');

  // r1's session has one more seat; a third player needs a second session: 8192 + 8192 > 11264, and the CPU machine is
  // never used for this listing.
  assert.equal((await alloc(env, 'bob', 'r2', CITY)).status, 202, 'the starting session\'s other seat');
  const refused = await alloc(env, 'carol', 'r3', CITY);
  assert.equal(refused.status, 503);
  assert.deepEqual([refused.body.status, refused.body.code, refused.body.gpu_min_vram_mb], ['full', 'no_gpu_capacity', 8192]);
  assert.match(refused.body.error, /GPU with at least 8192 MB/);

  // A CPU-only listing is placed exactly as before: the fullest machine that fits, GPU or not.
  const pong = await alloc(env, 'dave', 'r4', SIM);
  assert.equal(pong.status, 202, JSON.stringify(pong.body));
  assert.equal(fleetStore(env).get(`gv:${pong.body.server_id}`), undefined, 'a CPU-only session holds no VRAM');
});

test('a listing that requires a GPU, with no GPU machine in the fleet, is refused and never started on a CPU machine', async () => {
  const env = environment();
  const token = await machineUp(env, 'cpu1', machineOf({ threads: 16 }));
  const r = await alloc(env, 'alice', 'r1', CITY);
  assert.equal(r.status, 503);
  assert.equal(r.body.code, 'no_gpu_capacity');
  const next = await hb(env, token, 'cpu1', beat(2, machineOf({ threads: 16 })));
  assert.deepEqual(next.body.commands, [], 'no start, warm or otherwise, goes to the CPU machine');
  const status = await reply(await handleFleetAdmin(req('GET', '/api/admin/fleet', { token: 'admin' }), env, {}, '/api/admin/fleet', deps));
  assert.ok(status.body.advice.actions.some((a) => a.action === 'recommend_add_machine' && a.gpu_vram_mb === 8192), JSON.stringify(status.body.advice));
});

test('the listing setting: "required" names its VRAM, is stored, and is not text a review reads', () => {
  assert.deepEqual(checkListingEdit({ gpu: 'required', gpu_min_vram_mb: 8192 }), { ok: true, edit: { gpu: 'required', gpu_min_vram_mb: 8192 } });
  assert.deepEqual(checkListingEdit({ gpu: 'optional' }), { ok: true, edit: { gpu: 'optional', gpu_min_vram_mb: 0 } });
  for (const [body, code, field] of [
    [{ gpu: 'required' }, 'bad_gpu_min_vram', 'gpu_min_vram_mb'],
    [{ gpu: 'required', gpu_min_vram_mb: 100 }, 'bad_gpu_min_vram', 'gpu_min_vram_mb'],
    [{ gpu: 'none', gpu_min_vram_mb: 4096 }, 'bad_gpu_min_vram', 'gpu_min_vram_mb'],
    [{ gpu_min_vram_mb: 4096 }, 'bad_gpu', 'gpu'],
    [{ gpu: 'gpu' }, 'bad_gpu', 'gpu'],
  ]) {
    const r = checkListingEdit(body);
    assert.deepEqual([r.ok, r.code, r.field], [false, code, field], JSON.stringify(body));
  }
  const sim = { name: 'City', description: 'Streets.' };
  assert.deepEqual(applyListingEdit(sim, { gpu: 'required', gpu_min_vram_mb: 8192 }), { changed: ['gpu'], review: false });
  assert.deepEqual([sim.gpu, sim.gpu_min_vram_mb], ['required', 8192]);
  assert.deepEqual(applyListingEdit(sim, { gpu: 'required', gpu_min_vram_mb: 8192 }), { changed: [], review: false }, 'the same is no change');
  assert.deepEqual(applyListingEdit(sim, { gpu: 'none', gpu_min_vram_mb: 0 }), { changed: ['gpu'], review: false });
  assert.ok(!('gpu' in sim) && !('gpu_min_vram_mb' in sim), '"none" is the default and is stored as no setting');
});

test('the /live heartbeat: hardware keeps {gpu, cpu, tier} for an old host and carries VRAM for a new one', async () => {
  const env = environment();
  const live = async (method, token, body) => reply(await handleLive(req(method, `/api/simulations/${SIM}/live`, { token, body }), env, {}, SIM, deps));
  const old = await live('POST', 'author', {
    link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: 'old', hardware: { gpu: '', cpu: 'Intel Xeon', tier: 'high' },
  });
  assert.equal(old.status, 200, JSON.stringify(old.body));
  const fresh = await live('POST', 'author', {
    link: linkFor(2), players: 0, max_players: 2, protocol: 3, server_id: 'new',
    hardware: { gpu: 'NVIDIA GeForce RTX 2080 Ti', cpu: 'Intel Xeon', tier: 'high', gpu_vram_mb: 11264, gpu_available: true },
  });
  assert.equal(fresh.status, 200, JSON.stringify(fresh.body));
  const rows = (await live('GET', 'alice')).body.servers || [];
  const byId = Object.fromEntries(rows.map((r) => [r.id, r.hardware]));
  assert.deepEqual(byId.old, { gpu: '', cpu: 'Intel Xeon', tier: 'high' });
  assert.deepEqual(byId.new, { gpu: 'NVIDIA GeForce RTX 2080 Ti', cpu: 'Intel Xeon', tier: 'high', gpu_vram_mb: 11264, gpu_available: true });
  const bad = await live('POST', 'author', {
    link: linkFor(1), players: 0, max_players: 2, protocol: 3, server_id: 'old', hardware: { gpu: '', cpu: '', tier: 'low', gpu_vram_mb: -5, gpu_available: 'yes' },
  });
  assert.equal(bad.status, 200, 'a value out of range is left out, never a refusal');
  assert.deepEqual((await live('GET', 'alice')).body.servers.find((r) => r.id === 'old').hardware, { gpu: '', cpu: '', tier: 'low' });
});
