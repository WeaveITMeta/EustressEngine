// =============================================================================
// Fleet placement policy: the JavaScript port of eustress/crates/forge/src/fleet.rs
// =============================================================================
//
// Pure: no I/O, no clock reads (`now` is passed in, in seconds), and the same input always gives the same actions. The
// fleet Durable Object (src/fleet.mjs) runs it on every allocation and every host heartbeat. The Rust module is the
// specification; every type here has the field names its serde form has, so a value can move between the two as JSON.
// tests/fleet_policy.test.mjs ports every Rust test by name, and tests/fixtures/fleet_policy_cases.json holds exact
// input and output pairs both sides can read, so the two cannot drift without a test failing.
//
// CPU is counted in millithreads (1000 = one logical thread). Per machine of T threads and M MB, one more session is
// placed only if
//
//     committed + cost              <= T * utilisation_permille    (80% of the threads)
//     activation + occupied_millis  <= T * activation_permille     (90%: every session at 60 Hz at once still fits)
//     (sessions + 1) * memory_mb    <= M * memory_permille / 1000  (80% of the RAM)
//
// A listing whose `gpu` is 'required' places only on a machine with `gpu_available` and at least its
// `gpu_min_vram_mb` of VRAM not held by the machine's other GPU sessions; when none fits it is refused with
// 'no_gpu_capacity', never placed on a CPU machine. 'none' (the default) and 'optional' place as above, on any machine.
// docs/architecture/GPU_HOSTING_RULE.md.
//
// where an occupied session (players + reserved > 0) costs occupied_millis (800, MEASURED) and an empty one empty_millis
// (63, TARGET: 0.76 x 5 / 60). The warm pool keeps W = min(sum desired, max(floor(O * (1 - t) / t), min(sum floor,
// warm_floor_max))) empty servers, so at least 80% of running sessions have players once O >= 8. The policy never lowers
// a frame rate to fit more sessions: there is no action that does, and validateConfig refuses occupied_hz under 60.
// =============================================================================

/// MEASURED: 0.75 to 0.85 thread per 2-player session at 60 Hz, rounded up.
export const OCCUPIED_MILLIS = 800;
/// TARGET: an empty server at 5 Hz, 0.76 x 5 / 60. Not measured.
export const EMPTY_MILLIS = 63;
/// MEASURED: 57 to 60 MB private memory per session.
export const SESSION_MEMORY_MB = 60;
/// CONFIG DEFAULT: the policy commits at most 80% of a machine's threads.
export const UTILISATION_PERMILLE = 800;
/// CONFIG DEFAULT: every session at 60 Hz at once stays within 90% of the threads (decision 2). fleet.rs holds the same
/// number; tests/fleet_policy.test.mjs and fleet.rs `the_worker_port_holds_the_same_numbers` each read the other's.
export const ACTIVATION_PERMILLE = 900;
/// CONFIG DEFAULT: sessions use at most 80% of a machine's memory.
export const MEMORY_PERMILLE = 800;
/// CONFIG DEFAULT: at least 80% of running sessions have players.
export const OCCUPANCY_PERMILLE = 800;
/// CONFIG DEFAULT and floor: an occupied session runs at 60 Hz or faster.
export const OCCUPIED_HZ = 60;
export const MIN_OCCUPIED_HZ = 60;
/// The engine's ceiling for EUSTRESS_SERVER_HZ.
export const MAX_OCCUPIED_HZ = 120;
/// CONFIG DEFAULT: the engine's idle rate with nobody in the session.
export const IDLE_HZ = 5;
/// CONFIG DEFAULT: an empty server is kept this long before a stop.
export const EMPTY_HOLD_SECS = 120;
/// CONFIG DEFAULT: a join request counts as recent this long.
export const DEMAND_WINDOW_SECS = 600;
/// TARGET: seconds from start to hosting. Not measured.
export const START_LEAD_SECS = 60;
/// CONFIG DEFAULT: warm servers the floor may keep while few sessions run.
export const WARM_FLOOR_MAX = 2;
/// CONFIG DEFAULT: a player waiting on a start asks again after this long.
export const RETRY_AFTER_SECS = 5;
/// The least VRAM a listing that requires a GPU may ask for.
export const MIN_GPU_VRAM_MB = 256;
/// The most VRAM a listing that requires a GPU may ask for (256 GB).
export const MAX_GPU_VRAM_MB = 262144;
/// A listing's `gpu`: 'none' (the default), 'optional' (places as 'none'), or 'required' (with `gpu_min_vram_mb`).
export const GPU_MODES = Object.freeze(['none', 'optional', 'required']);

/// The GPU setting a listing record holds, read safely: `{ gpu, gpu_min_vram_mb }`. Anything unknown reads as 'none'.
export function gpuSettingOf(sim) {
  const gpu = GPU_MODES.includes(sim?.gpu) ? sim.gpu : 'none';
  if (gpu !== 'required') return { gpu, gpu_min_vram_mb: 0 };
  const min = Number.isSafeInteger(sim.gpu_min_vram_mb) ? sim.gpu_min_vram_mb : MIN_GPU_VRAM_MB;
  return { gpu, gpu_min_vram_mb: Math.min(MAX_GPU_VRAM_MB, Math.max(MIN_GPU_VRAM_MB, min)) };
}

/// Checks an author's GPU setting. `{ ok: true, value: { gpu, gpu_min_vram_mb } }` or `{ ok: false, error, code, field }`.
/// 'required' needs `gpu_min_vram_mb`, a whole number of MB from MIN_GPU_VRAM_MB to MAX_GPU_VRAM_MB; the other modes take
/// none. Pure.
export function checkGpuSetting(gpu, minVramMb) {
  if (!GPU_MODES.includes(gpu)) {
    return { ok: false, error: 'gpu must be "none", "optional" or "required".', code: 'bad_gpu', field: 'gpu' };
  }
  if (gpu !== 'required') {
    if (minVramMb !== undefined && minVramMb !== null) {
      return { ok: false, error: 'gpu_min_vram_mb applies only when gpu is "required".', code: 'bad_gpu_min_vram', field: 'gpu_min_vram_mb' };
    }
    return { ok: true, value: { gpu, gpu_min_vram_mb: 0 } };
  }
  if (!Number.isSafeInteger(minVramMb) || minVramMb < MIN_GPU_VRAM_MB || minVramMb > MAX_GPU_VRAM_MB) {
    return {
      ok: false,
      error: `A listing that requires a GPU names its least VRAM: gpu_min_vram_mb, a whole number of MB from ${MIN_GPU_VRAM_MB} to ${MAX_GPU_VRAM_MB}.`,
      code: 'bad_gpu_min_vram',
      field: 'gpu_min_vram_mb',
    };
  }
  return { ok: true, value: { gpu, gpu_min_vram_mb: minVramMb } };
}

/// `fleet::gpu_hold_mb`: the VRAM one session of a listing holds on its machine's GPU.
export const gpuHoldMb = (gpu, minVramMb) => (gpu === 'required' ? minVramMb || 0 : 0);
/// The VRAM a request needs, or null when its listing does not require a GPU (`JoinRequest::gpu_need_mb`).
export const gpuNeedOf = (r) => (r?.gpu === 'required' ? r.gpu_min_vram_mb || 0 : null);

/// forge-orchestration 0.6 `AutoscalerConfig::default()`.
export const AUTOSCALER_DEFAULTS = Object.freeze({
  upscale_threshold: 0.8,
  downscale_threshold: 0.3,
  hysteresis_secs: 300,
  eval_interval_secs: 30,
  min_instances: 1,
  max_instances: 100,
  scale_up_step: 1,
  scale_down_step: 1,
  predictive_enabled: false,
  metrics_window_secs: 300,
});

/// `FleetConfig::default()`, with `overrides` laid over it.
export function fleetConfig(overrides = {}) {
  return {
    occupied_millis: OCCUPIED_MILLIS,
    empty_millis: EMPTY_MILLIS,
    session_memory_mb: SESSION_MEMORY_MB,
    utilisation_permille: UTILISATION_PERMILLE,
    activation_permille: ACTIVATION_PERMILLE,
    memory_permille: MEMORY_PERMILLE,
    occupancy_permille: OCCUPANCY_PERMILLE,
    occupied_hz: OCCUPIED_HZ,
    idle_hz: IDLE_HZ,
    empty_hold_secs: EMPTY_HOLD_SECS,
    demand_window_secs: DEMAND_WINDOW_SECS,
    start_lead_secs: START_LEAD_SECS,
    warm_floor_max: WARM_FLOOR_MAX,
    retry_after_secs: RETRY_AFTER_SECS,
    ...overrides,
    machines: { ...AUTOSCALER_DEFAULTS, ...(overrides.machines || {}) },
  };
}

/// `FleetConfig::validate`: null when the configuration keeps every target, else why not.
export function validateConfig(cfg) {
  if (cfg.occupied_hz < MIN_OCCUPIED_HZ || cfg.occupied_hz > MAX_OCCUPIED_HZ) {
    return `occupied_hz is ${cfg.occupied_hz}; an occupied session runs at ${MIN_OCCUPIED_HZ} to ${MAX_OCCUPIED_HZ} Hz`;
  }
  if (cfg.idle_hz === 0 || cfg.idle_hz >= cfg.occupied_hz) return `idle_hz is ${cfg.idle_hz}; it must be at least 1 and under occupied_hz`;
  if (cfg.occupied_millis === 0 || cfg.empty_millis === 0 || cfg.empty_millis > cfg.occupied_millis) {
    return 'empty_millis must be at least 1 and at most occupied_millis';
  }
  if (cfg.session_memory_mb === 0) return 'session_memory_mb must be at least 1';
  if (cfg.utilisation_permille === 0 || cfg.utilisation_permille > cfg.activation_permille || cfg.activation_permille > 1000) {
    return '0 < utilisation_permille <= activation_permille <= 1000';
  }
  if (cfg.memory_permille === 0 || cfg.memory_permille > 1000) return 'memory_permille must be 1 to 1000';
  if (cfg.occupancy_permille === 0 || cfg.occupancy_permille >= 1000) return 'occupancy_permille must be 1 to 999';
  if (cfg.demand_window_secs === 0) return 'demand_window_secs must be at least 1';
  if (cfg.machines.upscale_threshold <= cfg.machines.downscale_threshold) return 'upscale_threshold must be greater than downscale_threshold';
  if (cfg.machines.min_instances > cfg.machines.max_instances) return 'min_instances cannot exceed max_instances';
  return null;
}

const sub = (a, b) => Math.max(0, a - b);
const div = (a, b) => Math.floor(a / b);
const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const byNumber = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
/// Lexicographic order over arrays of numbers and booleans (false before true), as Rust orders tuples.
const byTuple = (a, b) => {
  for (let i = 0; i < a.length; i++) {
    const d = byNumber(Number(a[i]), Number(b[i]));
    if (d !== 0) return d;
  }
  return 0;
};

// -----------------------------------------------------------------------------
// Sessions and machines
// -----------------------------------------------------------------------------

export const seated = (s) => (s.players || 0) + (s.reserved || 0);
export const isOccupied = (s) => seated(s) > 0;
export const hasRoom = (s) => s.max_players > 0 && seated(s) < s.max_players;
/// Empty and not on its way out: part of the warm pool.
export const isWarm = (s) => !isOccupied(s) && s.phase !== 'stopping';
export const emptyFor = (s, now) => (s.empty_since === null || s.empty_since === undefined ? 0 : sub(now, s.empty_since));
export const costMillis = (s, cfg) => (isOccupied(s) ? cfg.occupied_millis : cfg.empty_millis);

/// `Machine::gpu_vram_used_mb`.
export const gpuVramUsedMb = (m) => (m.sessions || []).reduce((n, s) => n + (s.gpu_vram_mb || 0), 0);
/// `Machine::gpu_class_fits`: a GPU host with at least `need` MB in all (a seat in a session already running there).
export const gpuClassFits = (m, need) => m.gpu_available === true && (m.gpu_vram_mb || 0) >= need;
/// `Machine::gpu_room_fits`: one more session holding `need` MB fits this machine's GPU.
export const gpuRoomFits = (m, need) => m.gpu_available === true && sub(m.gpu_vram_mb || 0, gpuVramUsedMb(m)) >= need;

export const committedCapMillis = (m, cfg) => m.threads * cfg.utilisation_permille;
export const activationCapMillis = (m, cfg) => m.threads * cfg.activation_permille;
export const memoryCapMb = (m, cfg) => div(m.memory_mb * cfg.memory_permille, 1000);
export const committedMillis = (m, cfg) => m.sessions.reduce((n, s) => n + costMillis(s, cfg), 0);
/// CPU if every session on the machine ran at the occupied rate at once.
export const activationMillis = (m, cfg) => m.sessions.length * cfg.occupied_millis;
export const memoryUsedMb = (m, cfg) => m.sessions.length * cfg.session_memory_mb;
/// Committed CPU as millionths of the 80% line.
export const fillPpm = (m, cfg) => div(committedMillis(m, cfg) * 1_000_000, Math.max(1, committedCapMillis(m, cfg)));

/// `fleet::capacity`: a machine's lines, what is committed against them, and the room left.
export function capacity(m, cfg) {
  const committedCap = committedCapMillis(m, cfg);
  const committed = committedMillis(m, cfg);
  const activationCap = activationCapMillis(m, cfg);
  const activation = activationMillis(m, cfg);
  const memoryCap = memoryCapMb(m, cfg);
  const memoryUsed = memoryUsedMb(m, cfg);
  let occupiedRoom = 0;
  let warmRoom = 0;
  if (!m.draining) {
    const cpuLeft = sub(committedCap, committed);
    const activationRoom = div(sub(activationCap, activation), Math.max(1, cfg.occupied_millis));
    const memoryRoom = div(sub(memoryCap, memoryUsed), Math.max(1, cfg.session_memory_mb));
    occupiedRoom = Math.min(div(cpuLeft, Math.max(1, cfg.occupied_millis)), activationRoom, memoryRoom);
    warmRoom = Math.min(div(cpuLeft, Math.max(1, cfg.empty_millis)), activationRoom, memoryRoom);
  }
  return {
    committed_cap_millis: committedCap,
    committed_millis: committed,
    activation_cap_millis: activationCap,
    activation_millis: activation,
    memory_cap_mb: memoryCap,
    memory_used_mb: memoryUsed,
    sessions: m.sessions.length,
    occupied_room: occupiedRoom,
    warm_room: warmRoom,
  };
}

/// `fleet::can_place`: whether one more session of `kind` ('occupied' or 'warm') fits on `m`, and in `region` when one
/// is named, without crossing the 80% line, the 90% activation line, or the memory line.
export function canPlace(m, kind, region, cfg) {
  if (m.draining) return false;
  if (typeof region === 'string' && region !== '' && m.region !== region) return false;
  const cost = kind === 'occupied' ? cfg.occupied_millis : cfg.empty_millis;
  return sub(committedCapMillis(m, cfg), committedMillis(m, cfg)) >= cost
    && sub(memoryCapMb(m, cfg), memoryUsedMb(m, cfg)) >= cfg.session_memory_mb
    && sub(activationCapMillis(m, cfg), activationMillis(m, cfg)) >= cfg.occupied_millis;
}

/// `fleet::choose_machine`: the fullest machine (committed share of its 80% line) that still fits. Ties go to more
/// sessions, then the lower id. The index, or null. `gpuNeed` (`fleet::choose_gpu_machine`): when not null, only a
/// machine whose GPU has that many MB free is a candidate.
export function chooseMachine(machines, kind, region, cfg, gpuNeed = null) {
  let best = null;
  machines.forEach((m, i) => {
    if (!canPlace(m, kind, region, cfg)) return;
    if (gpuNeed !== null && gpuNeed !== undefined && !gpuRoomFits(m, gpuNeed)) return;
    const fill = fillPpm(m, cfg);
    const sessions = m.sessions.length;
    const better = best === null
      || fill > best.fill
      || (fill === best.fill && (sessions > best.sessions || (sessions === best.sessions && m.id < machines[best.i].id)));
    if (better) best = { i, fill, sessions };
  });
  return best === null ? null : best.i;
}

/// Occupied sessions and running sessions (`stopping` excluded from both).
export function occupancy(state) {
  let occupied = 0;
  let running = 0;
  for (const m of state.machines) {
    for (const s of m.sessions) {
      if (s.phase === 'stopping') continue;
      running += 1;
      if (isOccupied(s)) occupied += 1;
    }
  }
  return [occupied, running];
}

/// `fleet::warm_pool_size`: W for `occupied` sessions, given the summed floors and desires.
export function warmPoolSize(occupied, floorTotal, desiredTotal, cfg) {
  const permille = Math.min(999, Math.max(1, cfg.occupancy_permille));
  const cap = div(occupied * (1000 - permille), permille);
  return Math.min(desiredTotal, Math.max(cap, Math.min(floorTotal, cfg.warm_floor_max)));
}

// -----------------------------------------------------------------------------
// The plan
// -----------------------------------------------------------------------------

/// `local_launch::rank_score` and `rank_servers`, as `place_request` calls them (tier 0, no latency): busiest server
/// that is not full first, ties toward more players. Array.prototype.sort is stable, as Rust's sort_by is.
const rankFullness = (r) => {
  const max = Math.max(1, r.max_players);
  return 0.5 * Math.min(1, Math.max(0, Math.min(r.players, max) / max));
};
const rankFull = (r) => r.max_players > 0 && r.players >= r.max_players;
function rankRooms(rows) {
  return [...rows].sort((a, b) => byNumber(Number(rankFull(a)), Number(rankFull(b)))
    || byNumber(rankFullness(b), rankFullness(a))
    || byNumber(b.players, a.players));
}

const inWindow = (now, at, cfg) => at <= now && now - at <= cfg.demand_window_secs;
const ceilDiv = (a, b) => (b === 0 ? a : div(a, b) + (a % b !== 0 ? 1 : 0));

/// The actions for this moment.
export function plan(now, state, demand, cfg) {
  return project(now, state, demand, cfg)[0];
}

/// `fleet::project`: the actions, and the fleet as it will be once they are applied (started sessions present as
/// `starting`, stopped ones as `stopping`, routed players as reservations).
export function project(now, state, demand, cfg) {
  const fleet = structuredClone({ machines: state.machines || [], last_machine_change_at: state.last_machine_change_at ?? null });
  fleet.machines.sort((a, b) => byText(a.id, b.id));
  for (const m of fleet.machines) m.sessions.sort((a, b) => byText(a.id, b.id));
  const actions = [];
  const seq = { n: 0 };
  const recent = (demand.recent || []).filter((r) => inWindow(now, r.at, cfg)).map((r) => ({ ...r }));
  const refused = { cpu: new Set(), gpu: new Map() };

  const pending = [...(demand.pending || [])].sort((a, b) => byNumber(a.at, b.at) || byText(a.request_id, b.request_id));
  for (const request of pending) {
    const [action, neededNew] = placeRequest(now, fleet, request, cfg, seq);
    const invalid = action.action === 'refuse' && action.reason === 'invalid_request';
    const region = request.region || '';
    if (action.action === 'refuse' && action.reason === 'no_capacity') refused.cpu.add(region);
    if (action.action === 'refuse' && action.reason === 'no_gpu_capacity') {
      refused.gpu.set(region, Math.max(refused.gpu.get(region) || 0, request.gpu_min_vram_mb || 0));
    }
    if (!invalid) {
      const click = {
        listing_id: request.listing_id,
        at: Math.min(request.at, now),
        max_players: request.max_players,
        region: request.region ?? null,
        needed_new_session: neededNew,
      };
      if (request.gpu && request.gpu !== 'none') click.gpu = request.gpu;
      if (request.gpu_min_vram_mb) click.gpu_min_vram_mb = request.gpu_min_vram_mb;
      recent.push(click);
    }
    actions.push(action);
  }

  warmPool(now, fleet, recent, cfg, seq, actions);
  machineActions(now, fleet, refused, cfg, actions);
  return [actions, fleet];
}

const nextPlanned = (seq) => `planned-${seq.n++}`;

/// Seat the requester in session (mi, si): a reservation, so the next request sees one seat fewer.
function seat(fleet, mi, si, request, cfg) {
  const machineId = fleet.machines[mi].id;
  const s = fleet.machines[mi].sessions[si];
  const warm = !isOccupied(s);
  s.reserved = (s.reserved || 0) + 1;
  s.empty_since = null;
  if (s.phase === 'hosting') {
    return { action: 'route', request_id: request.request_id, listing_id: request.listing_id, machine_id: machineId, session_id: s.id, warm };
  }
  return {
    action: 'wait', request_id: request.request_id, listing_id: request.listing_id, machine_id: machineId, session_id: s.id,
    retry_after_secs: cfg.retry_after_secs,
  };
}

/// One click, in order: a running session with players and a free seat (busiest first, the Quick Join rank); a warm
/// server (on the machine furthest under its 80% line); a session already starting; a new session on the fullest
/// machine that fits; a refusal.
function placeRequest(now, fleet, request, cfg, seq) {
  if (!request.listing_id || !(request.max_players > 0)) {
    return [{ action: 'refuse', request_id: request.request_id, listing_id: request.listing_id, reason: 'invalid_request' }, false];
  }
  const region = typeof request.region === 'string' && request.region !== '' ? request.region : null;
  const gpuNeed = gpuNeedOf(request);

  const rooms = [];
  const warm = [];
  const starting = [];
  fleet.machines.forEach((m, mi) => {
    if (region !== null && m.region !== region) return;
    // A listing that requires a GPU takes a seat only on a GPU host of its size.
    if (gpuNeed !== null && !gpuClassFits(m, gpuNeed)) return;
    const fill = fillPpm(m, cfg);
    m.sessions.forEach((s, si) => {
      if (s.listing_id !== request.listing_id || !hasRoom(s)) return;
      if (s.phase === 'hosting' && isOccupied(s)) rooms.push([mi, si]);
      else if (s.phase === 'hosting') warm.push([fill, mi, si]);
      else if (s.phase === 'starting') starting.push([!isOccupied(s), mi, si]);
    });
  });

  const ranked = rankRooms(rooms.map(([mi, si], k) => {
    const s = fleet.machines[mi].sessions[si];
    return { k, players: seated(s), max_players: s.max_players };
  }));
  if (ranked.length) {
    const [mi, si] = rooms[ranked[0].k];
    return [seat(fleet, mi, si, request, cfg), false];
  }

  warm.sort(byTuple);
  if (warm.length) return [seat(fleet, warm[0][1], warm[0][2], request, cfg), true];

  starting.sort(byTuple);
  if (starting.length) return [seat(fleet, starting[0][1], starting[0][2], request, cfg), true];

  const mi = chooseMachine(fleet.machines, 'occupied', region, cfg, gpuNeed);
  if (mi !== null) {
    const sessionId = nextPlanned(seq);
    const session = {
      id: sessionId, listing_id: request.listing_id, phase: 'starting', players: 0, reserved: 1,
      max_players: request.max_players, started_at: now, empty_since: null,
    };
    const start = {
      action: 'start_for_player', request_id: request.request_id, listing_id: request.listing_id, machine_id: fleet.machines[mi].id,
      session_id: sessionId, max_players: request.max_players, retry_after_secs: cfg.retry_after_secs,
    };
    // Serde leaves a zero `gpu_vram_mb` out; so does the port.
    if (gpuNeed) {
      session.gpu_vram_mb = gpuNeed;
      start.gpu_vram_mb = gpuNeed;
    }
    fleet.machines[mi].sessions.push(session);
    return [start, true];
  }
  const reason = gpuNeed !== null ? 'no_gpu_capacity' : 'no_capacity';
  return [{ action: 'refuse', request_id: request.request_id, listing_id: request.listing_id, reason }, true];
}

function warmPool(now, fleet, recent, cfg, seq, actions) {
  const occupied = occupancy(fleet)[0];

  const demand = new Map();
  for (const r of recent) {
    if (!inWindow(now, r.at, cfg) || !r.listing_id) continue;
    if (!demand.has(r.listing_id)) demand.set(r.listing_id, { latest: r.at, max_players: r.max_players, region: r.region ?? null, cold: 0, gpuNeed: null });
    const entry = demand.get(r.listing_id);
    if (r.at >= entry.latest) {
      entry.latest = r.at;
      if (r.max_players > 0) entry.max_players = r.max_players;
      entry.region = r.region ?? null;
      // The listing's latest setting decides where its warm servers go.
      entry.gpuNeed = gpuNeedOf(r);
    }
    if (r.needed_new_session) entry.cold += 1;
  }

  const sessions = () => fleet.machines.flatMap((m) => m.sessions);
  const rows = [...demand.keys()].sort(byText).map((listingId) => {
    const d = demand.get(listingId);
    const room = fleet.machines
      .filter((m) => d.gpuNeed === null || gpuClassFits(m, d.gpuNeed))
      .flatMap((m) => m.sessions)
      .some((s) => s.listing_id === listingId && s.phase !== 'stopping' && isOccupied(s) && hasRoom(s));
    const floor = room ? 0 : 1;
    const wanted = ceilDiv(d.cold * cfg.start_lead_secs, cfg.demand_window_secs);
    return {
      listing_id: listingId, latest: d.latest, max_players: Math.max(1, d.max_players || 0), region: d.region, floor,
      desired: Math.max(floor, wanted), gpuNeed: d.gpuNeed,
    };
  });

  const floorTotal = rows.reduce((n, r) => n + r.floor, 0);
  const desiredTotal = rows.reduce((n, r) => n + r.desired, 0);
  const pool = warmPoolSize(occupied, floorTotal, desiredTotal, cfg);

  rows.sort((a, b) => byNumber(b.latest, a.latest) || byText(a.listing_id, b.listing_id));
  const allot = new Map();
  let left = pool;
  for (const row of rows) {
    const give = Math.min(row.floor, left);
    left -= give;
    allot.set(row.listing_id, (allot.get(row.listing_id) || 0) + give);
  }
  for (const row of rows) {
    const give = Math.min(row.desired - row.floor, left);
    left -= give;
    allot.set(row.listing_id, (allot.get(row.listing_id) || 0) + give);
  }

  const have = new Map();
  for (const s of sessions()) if (isWarm(s)) have.set(s.listing_id, (have.get(s.listing_id) || 0) + 1);

  for (const row of rows) {
    const want = allot.get(row.listing_id) || 0;
    let count = have.get(row.listing_id) || 0;
    while (count < want) {
      const mi = chooseMachine(fleet.machines, 'warm', row.region, cfg, row.gpuNeed);
      if (mi === null) break;
      const sessionId = nextPlanned(seq);
      const session = {
        id: sessionId, listing_id: row.listing_id, phase: 'starting', players: 0, reserved: 0,
        max_players: row.max_players, started_at: now, empty_since: now,
      };
      const start = { action: 'start_warm', listing_id: row.listing_id, machine_id: fleet.machines[mi].id, session_id: sessionId, max_players: row.max_players };
      if (row.gpuNeed) {
        session.gpu_vram_mb = row.gpuNeed;
        start.gpu_vram_mb = row.gpuNeed;
      }
      fleet.machines[mi].sessions.push(session);
      actions.push(start);
      count += 1;
    }
  }

  for (const listingId of [...have.keys()].sort(byText)) {
    const count = have.get(listingId);
    const want = allot.get(listingId) || 0;
    if (count <= want) continue;
    // Draining machines first, then machines past their 80% line, then the emptiest machine, then the longest empty.
    const candidates = [];
    fleet.machines.forEach((m, mi) => {
      const over = committedMillis(m, cfg) > committedCapMillis(m, cfg);
      const fill = fillPpm(m, cfg);
      m.sessions.forEach((s, si) => {
        if (s.listing_id !== listingId || !isWarm(s) || s.phase !== 'hosting') return;
        const held = emptyFor(s, now) >= cfg.empty_hold_secs && sub(now, s.started_at) >= cfg.empty_hold_secs;
        if (!held) return;
        candidates.push([!m.draining, !over, fill, -emptyFor(s, now), mi, si]);
      });
    });
    candidates.sort(byTuple);
    for (const [, , , , mi, si] of candidates.slice(0, count - want)) {
      const s = fleet.machines[mi].sessions[si];
      s.phase = 'stopping';
      actions.push({ action: 'stop', machine_id: fleet.machines[mi].id, session_id: s.id, listing_id: s.listing_id, reason: 'surplus_empty' });
    }
  }
}

/// `fleet::machine_decision`: `{ decision: 'scale_up' | 'scale_down' | 'no_change', index }`. A refusal asks for a machine
/// even inside the hysteresis; everything else waits for it.
export function machineDecision(now, fleet, refused, cfg) {
  const limits = cfg.machines;
  const count = fleet.machines.length;
  if (refused) return { decision: count < Math.max(1, limits.max_instances) ? 'scale_up' : 'no_change', index: null };
  const at = fleet.last_machine_change_at;
  const settled = at === null || at === undefined || sub(now, at) >= limits.hysteresis_secs;
  if (!settled) return { decision: 'no_change', index: null };
  let cap = 0;
  let committed = 0;
  for (const m of fleet.machines) {
    if (m.draining) continue;
    cap += committedCapMillis(m, cfg);
    committed += committedMillis(m, cfg);
  }
  if (cap > 0 && committed / cap >= limits.upscale_threshold && count < limits.max_instances) return { decision: 'scale_up', index: null };
  if (count <= limits.min_instances) return { decision: 'no_change', index: null };
  const candidates = fleet.machines
    .map((m, i) => [m, i])
    .filter(([m]) => !m.sessions.some((s) => s.phase !== 'stopping' && isOccupied(s)))
    .map(([m, i]) => ({ key: [!m.draining, m.sessions.length, committedMillis(m, cfg)], id: m.id, i }))
    .sort((a, b) => byTuple(a.key, b.key) || byText(a.id, b.id) || byNumber(a.i, b.i));
  for (const { i } of candidates) {
    const m = fleet.machines[i];
    const ownCap = m.draining ? 0 : committedCapMillis(m, cfg);
    const ownCommitted = m.draining ? 0 : committedMillis(m, cfg);
    const restCap = sub(cap, ownCap);
    const restCommitted = sub(committed, ownCommitted);
    if (restCap === 0) continue;
    if (restCommitted / restCap <= limits.downscale_threshold) return { decision: 'scale_down', index: i };
  }
  return { decision: 'no_change', index: null };
}

function machineActions(now, fleet, refused, cfg, actions) {
  const any = refused.cpu.size > 0 || refused.gpu.size > 0;
  const place = (region) => (region === '' ? '' : ` in ${region}`);
  const { decision, index } = machineDecision(now, fleet, any, cfg);
  if (decision === 'scale_up' && any) {
    for (const region of [...refused.cpu].sort(byText)) {
      actions.push({
        action: 'recommend_add_machine',
        region,
        reason: `a player was refused: no machine${place(region)} had room for another ${cfg.occupied_hz} Hz session`,
      });
    }
    for (const region of [...refused.gpu.keys()].sort(byText)) {
      const need = refused.gpu.get(region);
      actions.push({
        action: 'recommend_add_machine',
        region,
        reason: `a player was refused: no machine${place(region)} with a GPU of at least ${need} MB of VRAM free had room for another ${cfg.occupied_hz} Hz session`,
        gpu_vram_mb: Math.max(1, need),
      });
    }
  } else if (decision === 'scale_up') {
    let best = null;
    for (const m of fleet.machines) {
      if (m.draining) continue;
      if (best === null || fillPpm(m, cfg) > fillPpm(best, cfg) || (fillPpm(m, cfg) === fillPpm(best, cfg) && m.id < best.id)) best = m;
    }
    actions.push({
      action: 'recommend_add_machine',
      region: best ? best.region : '',
      reason: `committed CPU is at or past ${(cfg.machines.upscale_threshold * 100).toFixed(0)}% of the machines' ${div(cfg.utilisation_permille, 10)}% lines`,
    });
  } else if (decision === 'scale_down' && index !== null) {
    const m = fleet.machines[index];
    const warm = m.sessions.filter(isWarm).length;
    actions.push({
      action: 'recommend_release_machine',
      machine_id: m.id,
      reason: `no session with players; the rest of the fleet stays at or under ${(cfg.machines.downscale_threshold * 100).toFixed(0)}% of its lines without it; drain it first (${warm} warm server(s) stop with it)`,
    });
  }
}
