// The placement policy (src/fleet_policy.mjs) against the Rust module it ports, eustress/crates/forge/src/fleet.rs.
// Every test here has the name, the inputs and the assertions of one Rust test in that file's `mod tests`, so a change
// to either side that the other does not make fails one of them. The exact input and output pairs in
// tests/fixtures/fleet_policy_cases.json are checked at the end; the Rust side can read the same file.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import * as policy from '../src/fleet_policy.mjs';
import {
  fleetConfig, validateConfig, capacity, canPlace, chooseMachine, plan, project, occupancy, warmPoolSize, committedMillis,
  checkGpuSetting, gpuSettingOf, gpuVramUsedMb,
} from '../src/fleet_policy.mjs';

const cfg = () => fleetConfig();
const machine = (id, threads) => ({ id, threads, memory_mb: 16_384, region: 'us-east-1', draining: false, sessions: [] });
const occupied = (id, listing, players, max) => ({
  id, listing_id: listing, phase: 'hosting', players, reserved: 0, max_players: max, started_at: 0, empty_since: null,
});
const warm = (id, listing, since) => ({
  id, listing_id: listing, phase: 'hosting', players: 0, reserved: 0, max_players: 2, started_at: since, empty_since: since,
});
const request = (id, listing, at) => ({ request_id: id, listing_id: listing, max_players: 2, region: null, at });
const recent = (listing, at) => ({ listing_id: listing, at, max_players: 2, region: null, needed_new_session: true });
const state = (machines) => ({ machines, last_machine_change_at: null });
const clone = (v) => structuredClone(v);

function startsAndStops(actions) {
  const starts = [];
  const stops = [];
  for (const a of actions) {
    if (a.action === 'start_warm' || a.action === 'start_for_player') starts.push(a.listing_id);
    else if (a.action === 'stop') stops.push(a.listing_id);
  }
  return [starts, stops];
}

test('the_defaults_validate_and_occupied_never_runs_under_60_hz', () => {
  assert.equal(validateConfig(cfg()), null);
  for (const hz of [0, 5, 20, 30, 59]) assert.notEqual(validateConfig(fleetConfig({ occupied_hz: hz })), null, `${hz} Hz must be refused`);
  assert.equal(validateConfig(fleetConfig({ occupied_hz: 120 })), null);
  assert.notEqual(validateConfig(fleetConfig({ occupied_hz: 121 })), null);
  assert.notEqual(validateConfig(fleetConfig({ idle_hz: 60 })), null);
  assert.notEqual(validateConfig(fleetConfig({ utilisation_permille: 1001, activation_permille: 1001 })), null);
  assert.notEqual(validateConfig(fleetConfig({ utilisation_permille: 900, activation_permille: 800 })), null);
});

test('four_threads_hold_exactly_four_occupied_60_hz_sessions', () => {
  const c = cfg();
  const m = machine('m', 4);
  assert.equal(capacity(m, c).occupied_room, 4);
  let placed = 0;
  for (let i = 0; i < 10; i++) {
    if (!canPlace(m, 'occupied', null, c)) break;
    m.sessions.push(occupied(`s${i}`, 'pong', 2, 2));
    placed += 1;
  }
  assert.equal(placed, 4, '3.2 threads / 0.8 thread');
  assert.equal(committedMillis(m, c), 3200);
  assert.equal(canPlace(m, 'occupied', null, c), false, 'the fifth is refused');
  assert.equal(capacity(m, c).occupied_room, 0);
});

test('activating_every_warm_server_at_once_never_passes_the_threads', () => {
  const c = cfg();
  for (let threads = 1; threads <= 64; threads++) {
    for (let occupiedFirst = 0; occupiedFirst <= threads * 2; occupiedFirst++) {
      const m = machine('m', threads);
      for (let i = 0; i < occupiedFirst; i++) if (canPlace(m, 'occupied', null, c)) m.sessions.push(occupied(`o${i}`, 'l', 1, 2));
      let i = 0;
      while (canPlace(m, 'warm', null, c)) {
        m.sessions.push(warm(`w${i}`, 'l', 0));
        i += 1;
        assert.ok(i < 100_000);
      }
      assert.ok(committedMillis(m, c) <= threads * 800, `committed past 80% on ${threads} threads`);
      assert.ok(m.sessions.length * c.occupied_millis <= threads * 900, `${threads} threads, ${m.sessions.length} sessions`);
    }
  }
  const empty = machine('m', 4);
  while (canPlace(empty, 'warm', null, c)) empty.sessions.push(warm(`w${empty.sessions.length}`, 'l', 0));
  assert.equal(empty.sessions.length, 4, '3600 / 800: the activation line bounds the warm servers');
});

test('the_activation_line_is_90_percent_and_leaves_room_for_the_measured_spread', () => {
  const c = cfg();
  assert.equal(policy.ACTIVATION_PERMILLE, 900);
  assert.equal(c.activation_permille, 900);
  for (const [threads, most] of [[1, 1], [2, 2], [4, 4], [8, 9], [16, 18], [32, 36]]) {
    const m = machine('m', threads);
    while (canPlace(m, 'warm', null, c)) m.sessions.push(warm(`w${m.sessions.length}`, 'l', 0));
    assert.equal(m.sessions.length, most, `${threads} threads: floor(${threads} x 900 / 800)`);
    assert.ok(m.sessions.length * 850 <= threads * 1000, `${threads} threads at 0.85`);
  }
});

// The Rust module is the specification: each shared number is read from fleet.rs itself, so changing one side alone fails
// here (and fleet.rs `the_worker_port_holds_the_same_numbers` reads this port the same way).
test('the_worker_port_holds_the_same_numbers', () => {
  const rust = fs.readFileSync(path.resolve(import.meta.dirname, '..', '..', '..', '..', 'eustress', 'crates', 'forge', 'src', 'fleet.rs'), 'utf8');
  const rustConst = (name) => {
    const m = new RegExp(`^pub const ${name}: u64 = ([0-9_]+);`, 'm').exec(rust);
    assert.ok(m, `${name} is not a pub const u64 in fleet.rs`);
    return Number(m[1].replaceAll('_', ''));
  };
  for (const name of [
    'ACTIVATION_PERMILLE', 'UTILISATION_PERMILLE', 'OCCUPIED_MILLIS', 'EMPTY_MILLIS', 'SESSION_MEMORY_MB', 'MEMORY_PERMILLE',
    'OCCUPANCY_PERMILLE', 'MIN_GPU_VRAM_MB', 'MAX_GPU_VRAM_MB',
  ]) {
    assert.equal(policy[name], rustConst(name), `${name}: fleet_policy.mjs and fleet.rs disagree`);
  }
});

const gpuMachine = (id, threads, vram) => ({ ...machine(id, threads), gpu_available: true, gpu_vram_mb: vram });
const gpuRequest = (id, listing, at, vram) => ({ ...request(id, listing, at), gpu: 'required', gpu_min_vram_mb: vram });

test('a_gpu_listing_starts_only_on_a_gpu_host_with_the_vram_free', () => {
  const c = cfg();
  const cpu = machine('a-cpu', 16);
  const small = gpuMachine('b-small', 16, 4096);
  const big = gpuMachine('c-big', 16, 12288);
  const [actions, projected] = project(10, state([clone(cpu), clone(small), clone(big)]), { pending: [gpuRequest('r1', 'city', 10, 8192)], recent: [] }, c);
  assert.ok(actions[0].action === 'start_for_player' && actions[0].machine_id === 'c-big' && actions[0].gpu_vram_mb === 8192, JSON.stringify(actions));
  assert.equal(gpuVramUsedMb(projected.machines.find((m) => m.id === 'c-big')), 8192, 'the session holds its VRAM');

  const held = clone(big);
  held.sessions.push({ ...occupied('s1', 'other', 2, 2), gpu_vram_mb: 8192 });
  const refused = plan(20, state([clone(cpu), clone(small), held]), { pending: [gpuRequest('r2', 'city', 20, 8192)], recent: [] }, c);
  assert.ok(refused[0].action === 'refuse' && refused[0].reason === 'no_gpu_capacity', JSON.stringify(refused));
  assert.ok(refused.some((a) => a.action === 'recommend_add_machine' && a.gpu_vram_mb === 8192), 'the recommendation names the GPU');
  assert.deepEqual(startsAndStops(refused)[0], [], 'nothing starts on a CPU machine');
});

test('a_gpu_listing_never_takes_a_seat_on_a_cpu_machine', () => {
  const c = cfg();
  const cpu = machine('a-cpu', 16);
  cpu.sessions.push(occupied('cpu-room', 'city', 1, 4), warm('cpu-warm', 'city', 0));
  const gpu = gpuMachine('b-gpu', 16, 16384);
  let actions = plan(10, state([clone(cpu), clone(gpu)]), { pending: [gpuRequest('r1', 'city', 10, 8192)], recent: [] }, c);
  assert.ok(actions[0].action === 'start_for_player' && actions[0].machine_id === 'b-gpu', JSON.stringify(actions));
  actions = plan(10, state([clone(cpu), clone(gpu)]), { pending: [request('r1', 'city', 10)], recent: [] }, c);
  assert.ok(actions[0].action === 'route' && actions[0].session_id === 'cpu-room', JSON.stringify(actions));
});

test('none_and_optional_place_exactly_as_a_listing_without_the_setting', () => {
  const c = cfg();
  const a = gpuMachine('a', 8, 8192);
  a.sessions.push(occupied('a1', 'pong', 1, 2), warm('a2', 'tag', 0));
  const b = machine('b', 4);
  b.sessions.push(occupied('b1', 'tag', 2, 2));
  const plain = { pending: [request('r1', 'pong', 10), request('r2', 'tag', 11), request('r3', 'new', 12)], recent: [recent('tag', 5)] };
  const base = plan(20, state([clone(a), clone(b)]), plain, c);
  assert.ok(base.length > 0);
  for (const gpu of ['none', 'optional']) {
    const demand = clone(plain);
    for (const r of [...demand.pending, ...demand.recent]) Object.assign(r, { gpu, gpu_min_vram_mb: 4096 });
    assert.deepEqual(plan(20, state([clone(a), clone(b)]), demand, c), base, gpu);
  }
});

test('a_gpu_listing_keeps_its_warm_server_on_a_gpu_host', () => {
  const c = cfg();
  const cpu = machine('a-cpu', 16);
  const gpu = gpuMachine('b-gpu', 4, 8192);
  const demand = { pending: [], recent: [{ ...recent('city', 990), gpu: 'required', gpu_min_vram_mb: 6144 }] };
  const actions = plan(1000, state([clone(cpu), gpu]), demand, c);
  assert.deepEqual(startsAndStops(actions)[0], ['city']);
  assert.ok(actions.some((a) => a.action === 'start_warm' && a.machine_id === 'b-gpu' && a.gpu_vram_mb === 6144), JSON.stringify(actions));
  assert.deepEqual(startsAndStops(plan(1000, state([clone(cpu)]), demand, c))[0], [], 'no GPU host: no warm server at all');
});

test('a listing GPU setting: "required" names its VRAM, the others take none, anything unknown reads as "none"', () => {
  assert.deepEqual(checkGpuSetting('none'), { ok: true, value: { gpu: 'none', gpu_min_vram_mb: 0 } });
  assert.deepEqual(checkGpuSetting('optional', null), { ok: true, value: { gpu: 'optional', gpu_min_vram_mb: 0 } });
  assert.deepEqual(checkGpuSetting('required', 8192), { ok: true, value: { gpu: 'required', gpu_min_vram_mb: 8192 } });
  for (const [gpu, vram, field] of [
    ['required', undefined, 'gpu_min_vram_mb'], ['required', 255, 'gpu_min_vram_mb'], ['required', 262145, 'gpu_min_vram_mb'],
    ['required', 1.5, 'gpu_min_vram_mb'], ['required', '8192', 'gpu_min_vram_mb'], ['none', 8192, 'gpu_min_vram_mb'],
    ['always', undefined, 'gpu'], [true, undefined, 'gpu'],
  ]) {
    const r = checkGpuSetting(gpu, vram);
    assert.deepEqual([r.ok, r.field], [false, field], JSON.stringify([gpu, vram]));
  }
  assert.deepEqual(gpuSettingOf({}), { gpu: 'none', gpu_min_vram_mb: 0 });
  assert.deepEqual(gpuSettingOf({ gpu: 'weird', gpu_min_vram_mb: 9 }), { gpu: 'none', gpu_min_vram_mb: 0 });
  assert.deepEqual(gpuSettingOf({ gpu: 'optional', gpu_min_vram_mb: 9 }), { gpu: 'optional', gpu_min_vram_mb: 0 });
  assert.deepEqual(gpuSettingOf({ gpu: 'required', gpu_min_vram_mb: 8192 }), { gpu: 'required', gpu_min_vram_mb: 8192 });
});

test('capacity_agrees_with_placing_one_at_a_time', () => {
  const c = cfg();
  for (let threads = 1; threads <= 32; threads++) {
    for (const kind of ['occupied', 'warm']) {
      const m = machine('m', threads);
      m.sessions.push(occupied('seed', 'l', 1, 2));
      const room = kind === 'occupied' ? capacity(m, c).occupied_room : capacity(m, c).warm_room;
      let placed = 0;
      while (canPlace(m, kind, null, c)) {
        m.sessions.push(kind === 'occupied' ? occupied(`s${placed}`, 'l', 1, 2) : warm(`s${placed}`, 'l', 0));
        placed += 1;
      }
      assert.equal(placed, room, `${threads} threads, ${kind}`);
    }
  }
  const draining = { ...machine('d', 8), draining: true };
  assert.equal(canPlace(draining, 'warm', null, c), false);
  assert.equal(capacity(draining, c).occupied_room, 0);
});

test('packing_prefers_the_fuller_machine_that_still_fits', () => {
  const c = cfg();
  const a = machine('a', 8);
  for (let i = 0; i < 3; i++) a.sessions.push(occupied(`a${i}`, 'l', 1, 2));
  const b = machine('b', 8);
  b.sessions.push(occupied('b0', 'l', 1, 2));
  assert.equal(chooseMachine([clone(b), clone(a)], 'occupied', null, c), 1, 'a is fuller');

  const full = clone(a);
  while (canPlace(full, 'occupied', null, c)) full.sessions.push(occupied(`f${full.sessions.length}`, 'l', 1, 2));
  assert.equal(chooseMachine([full, clone(b)], 'occupied', null, c), 1, 'a full machine is skipped');

  assert.equal(chooseMachine([machine('z', 8), machine('y', 8)], 'occupied', null, c), 1, 'a tie goes to the lower id');

  const west = { ...machine('w', 8), region: 'us-west-2' };
  assert.equal(chooseMachine([clone(a), west], 'occupied', 'us-west-2', c), 1);
  assert.equal(chooseMachine([clone(a), west], 'occupied', 'eu-west-1', c), null);

  const actions = plan(10, state([b, a]), { pending: [request('r1', 'new', 10)], recent: [] }, c);
  assert.ok(actions.some((x) => x.action === 'start_for_player' && x.machine_id === 'a'), JSON.stringify(actions));
});

test('a_click_takes_a_seat_before_anything_starts', () => {
  const c = cfg();
  const m = machine('m', 8);
  m.sessions.push(occupied('busy', 'pong', 3, 4), occupied('quiet', 'pong', 1, 4), occupied('full', 'pong', 4, 4), warm('spare', 'pong', 0));
  let actions = plan(50, state([clone(m)]), { pending: [request('r1', 'pong', 50)], recent: [] }, c);
  assert.deepEqual(actions[0], { action: 'route', request_id: 'r1', listing_id: 'pong', machine_id: 'm', session_id: 'busy', warm: false });
  assert.deepEqual(startsAndStops(actions)[0], []);

  // No seat in a session with players: the warm server, then a start.
  m.sessions = m.sessions.filter((s) => s.id === 'full' || s.id === 'spare');
  actions = plan(50, state([m]), { pending: [request('r1', 'pong', 50), request('r2', 'pong', 51)], recent: [] }, c);
  assert.ok(actions[0].action === 'route' && actions[0].session_id === 'spare' && actions[0].warm === true, JSON.stringify(actions[0]));
  assert.ok(actions[1].action === 'route' && actions[1].session_id === 'spare' && actions[1].warm === false, JSON.stringify(actions[1]));
});

test('a_full_fleet_refuses_and_recommends_a_machine_instead_of_slowing_anyone', () => {
  const c = cfg();
  const m = machine('m', 4);
  for (let i = 0; i < 4; i++) m.sessions.push(occupied(`s${i}`, 'other', 2, 2));
  const demand = { pending: [request('r1', 'pong', 10)], recent: [] };
  const actions = plan(10, state([m]), demand, c);
  assert.ok(actions.some((a) => a.action === 'refuse' && a.reason === 'no_capacity'));
  assert.ok(actions.some((a) => a.action === 'recommend_add_machine'));
  assert.deepEqual(startsAndStops(actions)[0], []);

  const empty = plan(10, { machines: [], last_machine_change_at: null }, demand, c);
  assert.ok(empty.some((a) => a.action === 'recommend_add_machine'));
});

test('planned_occupancy_stays_at_or_above_80_percent_once_a_handful_run', () => {
  const c = cfg();
  for (let count = 0; count <= 60; count++) {
    const machines = Array.from({ length: 8 }, (_, i) => machine(`m${i}`, 16));
    for (let i = 0; i < count; i++) machines[i % 8].sessions.push(occupied(`o${i}`, `busy${i}`, 2, 2));
    // Forty Spaces with a recent click and no free seat: every one wants a warm server.
    const demand = { pending: [], recent: [] };
    for (let k = 0; k < 40; k++) demand.recent.push(recent(`cold${k}`, 990));
    for (let i = 0; i < count; i++) demand.recent.push(recent(`busy${i}`, 990));
    const [actions, projected] = project(1000, state(machines), demand, c);
    const [occupiedNow, running] = occupancy(projected);
    assert.equal(occupiedNow, count);
    const warmCount = running - occupiedNow;
    if (count >= 8) assert.ok(occupiedNow * 1000 >= 800 * running, `${count} occupied, ${running} running: ${JSON.stringify(actions)}`);
    else assert.ok(warmCount <= 2, `${count} occupied, ${warmCount} warm`);
    assert.ok(warmCount >= 1, 'a Space clicked moments ago keeps a warm server');
  }
});

test('empty_servers_beyond_the_pool_stop_once_held', () => {
  const c = cfg();
  const m = machine('m', 32);
  for (let i = 0; i < 8; i++) m.sessions.push(occupied(`o${i}`, `busy${i}`, 2, 2));
  for (let i = 0; i < 10; i++) m.sessions.push(warm(`w${i}`, `idle${i}`, 0));
  let [actions, projected] = project(5000, state([clone(m)]), { pending: [], recent: [] }, c);
  assert.equal(startsAndStops(actions)[1].length, 10, 'no recent click: none is kept');
  assert.deepEqual(occupancy(projected), [8, 8]);

  [actions, projected] = project(5000, state([m]), { pending: [], recent: [recent('idle3', 4990)] }, c);
  assert.equal(startsAndStops(actions)[1].length, 9);
  assert.ok(!startsAndStops(actions)[1].includes('idle3'), 'a recent click keeps its warm server');
  assert.deepEqual(occupancy(projected), [8, 9]);
});

test('hysteresis_holds_across_a_join_and_a_leave', () => {
  const c = cfg();
  const m = machine('m', 8);
  m.sessions.push(occupied('s1', 'pong', 2, 2), warm('w1', 'pong', 0));
  const demand = { pending: [], recent: [recent('pong', 5)] };

  assert.deepEqual(plan(10, state([clone(m)]), demand, c), [], 'the warm server is the floor');

  demand.pending.push(request('r1', 'pong', 20));
  const joined = plan(20, state([clone(m)]), demand, c);
  assert.equal(joined.length, 1, JSON.stringify(joined));
  assert.ok(joined[0].action === 'route' && joined[0].session_id === 'w1' && joined[0].warm === true);
  demand.pending = [];
  demand.recent.push({ ...recent('pong', 20), needed_new_session: true });

  // The player leaves at 30: w1 is empty again, the click is recent, nothing moves.
  m.sessions.find((s) => s.id === 'w1').empty_since = 30;
  for (const now of [40, 100, 400, 620]) assert.deepEqual(plan(now, state([clone(m)]), demand, c), [], `at ${now}`);
  // Clicks aged out and w1 empty past the hold: stopped.
  const late = plan(1000, state([clone(m)]), demand, c);
  assert.ok(late.length === 1 && late[0].action === 'stop' && late[0].session_id === 'w1', JSON.stringify(late));

  // A leave long after the last click still waits out the hold.
  m.sessions.find((s) => s.id === 'w1').empty_since = 1000;
  assert.deepEqual(plan(1010, state([clone(m)]), demand, c), [], 'inside the hold');
  assert.deepEqual(plan(1119, state([clone(m)]), demand, c), [], 'inside the hold');
  assert.deepEqual(startsAndStops(plan(1120, state([m]), demand, c))[1], ['pong']);
});

test('no_plan_starts_and_stops_the_same_listing', () => {
  const c = cfg();
  for (let now = 0; now < 2000; now += 37) {
    const m = machine('m', 16);
    m.sessions.push(occupied('s1', 'pong', 2, 2), warm('w1', 'pong', 0), warm('w2', 'pong', 0), warm('w3', 'tag', 100));
    const demand = { pending: [request(`r${now}`, 'tag', now)], recent: [recent('pong', Math.max(0, now - 300)), recent('tag', Math.max(0, now - 50))] };
    const [starts, stops] = startsAndStops(plan(now, state([m]), demand, c));
    for (const listing of starts) assert.ok(!stops.includes(listing), `at ${now}: ${listing} started and stopped`);
  }
});

test('a_machine_with_no_players_is_recommended_for_release_outside_the_hysteresis', () => {
  const c = cfg();
  const a = machine('a', 8);
  a.sessions.push(occupied('o1', 'pong', 2, 2));
  const fleet = state([a, machine('b', 8)]);
  const actions = plan(10_000, fleet, { pending: [], recent: [] }, c);
  assert.ok(actions.some((x) => x.action === 'recommend_release_machine' && x.machine_id === 'b'), JSON.stringify(actions));
  fleet.last_machine_change_at = 9_900;
  assert.ok(!plan(10_000, fleet, { pending: [], recent: [] }, c).some((x) => x.action === 'recommend_release_machine'));
  assert.deepEqual(plan(10_000, state([machine('only', 8)]), { pending: [], recent: [] }, c), [], 'the last machine is never released');
});

test('the_plan_is_deterministic_whatever_order_the_machines_arrive_in', () => {
  const c = cfg();
  const a = machine('a', 8);
  a.sessions.push(occupied('a1', 'pong', 1, 2), warm('a2', 'tag', 0));
  const b = machine('b', 4);
  b.sessions.push(occupied('b1', 'tag', 2, 2));
  const demand = { pending: [request('r2', 'tag', 11), request('r1', 'pong', 10), request('r3', 'new', 12)], recent: [recent('tag', 5)] };
  const one = plan(20, state([clone(a), clone(b)]), demand, c);
  const two = plan(20, state([b, a]), demand, c);
  assert.deepEqual(one, two);
  assert.ok(one.length > 0);
});

test('the_warm_pool_formula', () => {
  const c = cfg();
  assert.equal(warmPoolSize(0, 5, 9, c), 2, 'floor term while nothing runs');
  assert.equal(warmPoolSize(4, 1, 9, c), 1, 'floor of one Space');
  assert.equal(warmPoolSize(8, 9, 9, c), 2, '8 occupied: cap 2');
  assert.equal(warmPoolSize(40, 0, 30, c), 10, '40 occupied: cap 10');
  assert.equal(warmPoolSize(40, 0, 3, c), 3, 'never more than wanted');
  assert.equal(warmPoolSize(40, 0, 0, c), 0);
  for (let o = 8; o < 500; o++) {
    const w = warmPoolSize(o, 1000, 1000, c);
    assert.ok(o * 1000 >= 800 * (o + w), `${o} occupied, ${w} warm`);
  }
});

// -----------------------------------------------------------------------------
// Exact input and output pairs (tests/fixtures/fleet_policy_cases.json)
// -----------------------------------------------------------------------------

const FIXTURES = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, 'fixtures', 'fleet_policy_cases.json'), 'utf8'));

test('every fixture case gives exactly the recorded output', () => {
  assert.ok(FIXTURES.cases.length >= 10, 'the fixture file holds its cases');
  for (const k of FIXTURES.cases) {
    const c = fleetConfig(k.config || {});
    let got;
    if (k.fn === 'capacity') got = capacity(k.machine, c);
    else if (k.fn === 'can_place') got = canPlace(k.machine, k.kind, k.region ?? null, c);
    else if (k.fn === 'choose_machine') got = chooseMachine(k.machines, k.kind, k.region ?? null, c);
    else if (k.fn === 'warm_pool_size') got = warmPoolSize(k.occupied, k.floor_total, k.desired_total, c);
    else if (k.fn === 'plan') got = plan(k.now, k.state, k.demand, c);
    else throw new Error(`unknown fixture fn ${k.fn}`);
    assert.deepEqual(got, k.expect, `${k.name} (mirrors ${k.rust_test})`);
  }
});
