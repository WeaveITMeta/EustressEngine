// =============================================================================
// Eustress-hosted sessions: the fleet's control plane
// =============================================================================
//
// A player's Play click finds or starts a simulation server on a rented machine. One session is one
// `eustress-engine --server` process. The contract is docs/architecture/HOSTED_FLEET.md; the placement policy is
// src/fleet_policy.mjs, the port of eustress/crates/forge/src/fleet.rs.
//
//     POST   /api/fleet/hosts/{machine_id}/heartbeat           a host agent, Bearer ehost_ (Route 1)
//     POST   /api/simulations/{id}/allocate                    a player's Play click (Route 2)
//     POST   /api/admin/fleet/hosts/{machine_id}/credential    an administrator mints the machine's ehost_
//     DELETE /api/admin/fleet/hosts/{machine_id}/credential    an administrator revokes it
//     GET    /api/admin/fleet                                  an administrator reads the fleet (no secrets)
//
// One Durable Object, `Fleet`, named "fleet", holds the whole fleet, so every decision sees one consistent state:
//
//   m:<machine_id>                 the machine as its latest heartbeat reported it, and when that was (`seen`, ms)
//   c:<machine_id>:<command_id>    a command not yet acked or rejected; resent on each heartbeat for 60 s
//   p:<server_id>                  a session the fleet asked a host to start and has not yet seen in a heartbeat
//   r:<who>:<request_id>           an allocation's answer and the seat it reserved, kept 10 minutes
//   rate:<who>                     the times of this caller's new allocations in the last minute
//   hostsess:<sha256>              an eshs_ credential (src/fleet_credentials.mjs); sc:<server_id> lists its hashes
//   recent                         answered clicks in the demand window, for the warm pool
//   gv:<server_id>                 the VRAM a session of a listing that requires a GPU holds (MB), from the start the fleet
//                                  asked for; a host's own claim is never read. Gone with the session's credentials.
//   advice                         the plan's latest machine recommendation; a person rents, the plan never does
//
// Times in storage are milliseconds; the policy works in seconds. No secret is logged, and no secret is stored: an
// eshs_ is minted fresh each time a start command is sent, and only its hash is kept.
// =============================================================================

import { bearerOf, delistServer, liveServerRows, MAX_PLAYERS } from './live.mjs';
import { bodyObject, guestPlayOn, leaseIsLive } from './guest.mjs';
import { legalLockOf, simWithLegalLock } from './moderation.mjs';
import { refuseAdult } from './adult_content.mjs';
import {
  FLEET_ID, fleetCall, hostFromCredential, mintHostCredential, mintSecret, randomHex, revokeHostCredential, sameText, sha256Hex,
} from './fleet_credentials.mjs';
import { capacity, fleetConfig, gpuClassFits, gpuSettingOf, GPU_MODES, MAX_GPU_VRAM_MB, occupancy, plan, project } from './fleet_policy.mjs';
import { hasSourceWorld } from './world.mjs';

/// CONFIG DEFAULT: how often a host agent heartbeats.
export const HEARTBEAT_SECS = 5;
/// No heartbeat for this long: the fleet places nothing on the machine.
export const HOST_STALE_MS = 30_000;
/// A command is resent until it is acked, rejected, or this old.
export const COMMAND_TTL_MS = 60_000;
/// A seat reserved for a player who has not arrived is freed after this long (fleet_host::RESERVATION_SECS).
export const RESERVATION_MS = 60_000;
/// A seat whose player the listing admitted stops counting as reserved once the session's machine has heartbeated this
/// long after the admission: the agent reads each status file once a second, so that heartbeat counts the player itself.
export const ARRIVAL_SETTLE_MS = 3_000;
/// An allocation's answer is kept this long, for the same request_id asked again.
export const REQUEST_KEEP_MS = 600_000;
/// New allocations one account or lease may make in a minute. Asking again with a known request_id is not counted.
export const ALLOCATIONS_PER_MINUTE = 10;
/// An eshs_ that has not yet carried a /live heartbeat lasts this long: the start, the Space load and the first beat.
export const SESSION_CREDENTIAL_START_MS = 600_000;
/// After a /live heartbeat, an eshs_ lasts as long as that heartbeat's /live entry (live.mjs HEARTBEAT_TTL_MS).
export const SESSION_CREDENTIAL_BEAT_MS = 90_000;
/// A player refused for capacity, or a fleet that cannot be reached, asks again after this long.
export const FULL_RETRY_SECS = 30;
/// Most sessions one heartbeat may list, and most entries in each of its other lists.
export const MAX_HEARTBEAT_ITEMS = 256;
const MAX_HEARTBEAT_BYTES = 256 * 1024;
const MAX_RECENT = 10_000;
const SWEEP_MS = 60_000;

const LISTING_ID = /^[a-f0-9-]{1,64}$/;
const COMMAND_ID = /^cmd_[A-Za-z0-9]{1,60}$/;
const WHO = /^(u:[A-Za-z0-9-]{1,64}|g:[0-9a-f]{32})$/;
const FREED_WHY = new Set(['exited_empty', 'host_failed', 'stopped', 'dead']);
const PHASES = new Set(['starting', 'hosting', 'stopping']);
const secs = (ms) => Math.floor(ms / 1000);
const whole = (v, min, max) => Number.isSafeInteger(v) && v >= min && v <= max;

// -----------------------------------------------------------------------------
// The heartbeat body
// -----------------------------------------------------------------------------

/// The heartbeat as the fleet keeps it, or `{ field }` naming the first field out of range. `region` is the region the
/// machine's credential was minted for; a heartbeat may not claim another.
export function checkHeartbeat(body, region) {
  const bad = (field) => ({ ok: false, field });
  if (!body || typeof body !== 'object' || Array.isArray(body)) return bad('body');
  if (typeof body.boot_id !== 'string' || !FLEET_ID.test(body.boot_id)) return bad('boot_id');
  if (!whole(body.seq, 0, Number.MAX_SAFE_INTEGER)) return bad('seq');
  const m = body.machine;
  if (!m || typeof m !== 'object' || Array.isArray(m)) return bad('machine');
  if (!whole(m.threads, 1, 1024)) return bad('machine.threads');
  if (!whole(m.memory_mb, 1, 16 * 1024 * 1024)) return bad('machine.memory_mb');
  if (m.draining !== undefined && typeof m.draining !== 'boolean') return bad('machine.draining');
  const reported = m.region === undefined || m.region === null ? '' : m.region;
  if (typeof reported !== 'string' || (reported !== '' && !FLEET_ID.test(reported))) return bad('machine.region');
  if (region && reported !== '' && reported !== region) return bad('machine.region');
  // The GPU (fleet_host BeatMachine, gpu_probe). Optional: an agent that predates it reports no GPU.
  if (m.gpu !== undefined && m.gpu !== null && (typeof m.gpu !== 'string' || m.gpu.length > 160)) return bad('machine.gpu');
  if (m.gpu_vram_mb !== undefined && m.gpu_vram_mb !== null && !whole(m.gpu_vram_mb, 0, MAX_GPU_VRAM_MB * 4)) return bad('machine.gpu_vram_mb');
  if (m.gpu_available !== undefined && m.gpu_available !== null && typeof m.gpu_available !== 'boolean') return bad('machine.gpu_available');
  if (body.capacity !== undefined && (body.capacity === null || typeof body.capacity !== 'object' || Array.isArray(body.capacity))) return bad('capacity');
  const list = (name) => (body[name] === undefined ? [] : body[name]);
  for (const name of ['sessions', 'freed', 'acked', 'rejected']) {
    if (!Array.isArray(list(name)) || list(name).length > MAX_HEARTBEAT_ITEMS) return bad(name);
  }
  const sessions = [];
  const ids = new Set();
  for (const [i, s] of list('sessions').entries()) {
    const f = (k) => `sessions[${i}].${k}`;
    if (!s || typeof s !== 'object') return bad(`sessions[${i}]`);
    if (typeof s.id !== 'string' || !FLEET_ID.test(s.id) || ids.has(s.id)) return bad(f('id'));
    if (typeof s.listing_id !== 'string' || !LISTING_ID.test(s.listing_id)) return bad(f('listing_id'));
    if (!PHASES.has(s.phase)) return bad(f('phase'));
    if (!whole(s.players, 0, MAX_PLAYERS)) return bad(f('players'));
    if (!whole(s.reserved ?? 0, 0, MAX_PLAYERS)) return bad(f('reserved'));
    if (!whole(s.max_players, 0, MAX_PLAYERS)) return bad(f('max_players'));
    if (!whole(s.started_at, 0, Number.MAX_SAFE_INTEGER)) return bad(f('started_at'));
    if (!(s.empty_since === null || s.empty_since === undefined || whole(s.empty_since, 0, Number.MAX_SAFE_INTEGER))) return bad(f('empty_since'));
    if (s.port !== undefined && s.port !== null && !whole(s.port, 1, 65535)) return bad(f('port'));
    ids.add(s.id);
    sessions.push({
      id: s.id, listing_id: s.listing_id, phase: s.phase, players: s.players, reserved: s.reserved ?? 0, max_players: s.max_players,
      started_at: s.started_at, empty_since: s.empty_since ?? null,
    });
  }
  const freed = [];
  for (const [i, x] of list('freed').entries()) {
    if (!x || typeof x.id !== 'string' || !FLEET_ID.test(x.id)) return bad(`freed[${i}].id`);
    if (!FREED_WHY.has(x.why)) return bad(`freed[${i}].why`);
    freed.push({ id: x.id, why: x.why });
  }
  const acked = [];
  for (const [i, id] of list('acked').entries()) {
    if (typeof id !== 'string' || !COMMAND_ID.test(id)) return bad(`acked[${i}]`);
    acked.push(id);
  }
  const rejected = [];
  for (const [i, x] of list('rejected').entries()) {
    if (!x || typeof x.command_id !== 'string' || !COMMAND_ID.test(x.command_id)) return bad(`rejected[${i}].command_id`);
    if (x.why !== undefined && (typeof x.why !== 'string' || x.why.length > 200)) return bad(`rejected[${i}].why`);
    rejected.push(x.command_id);
  }
  if (body.agent_version !== undefined && (typeof body.agent_version !== 'string' || !/^[0-9A-Za-z.+_-]{1,32}$/.test(body.agent_version))) {
    return bad('agent_version');
  }
  return {
    ok: true,
    value: {
      boot_id: body.boot_id, seq: body.seq,
      machine: {
        threads: m.threads, memory_mb: m.memory_mb, region: region || reported, draining: m.draining === true,
        gpu: typeof m.gpu === 'string' ? m.gpu : '', gpu_vram_mb: m.gpu_vram_mb ?? 0, gpu_available: m.gpu_available === true,
      },
      sessions, freed, acked, rejected, agent_version: body.agent_version ?? null,
    },
  };
}

// -----------------------------------------------------------------------------
// The Durable Object
// -----------------------------------------------------------------------------

const refuse = (status, code, error) => ({ ok: false, status, code, error });

async function values(storage, prefix) {
  return [...(await storage.list({ prefix })).values()];
}

async function entries(storage, prefix) {
  return [...(await storage.list({ prefix })).entries()];
}

/// Delete every eshs_ minted for this server. Returns the listing those credentials stood for and the machine they were
/// minted to (the Worker's own record, never a host's claim), or null when none was left.
async function revokeServer(storage, serverId) {
  const hashes = (await storage.get(`sc:${serverId}`)) || [];
  let held = null;
  for (const h of hashes) {
    const record = await storage.get(`hostsess:${h}`);
    if (record) {
      held = { sim_id: record.sim_id, machine_id: record.machine_id };
      break;
    }
  }
  const keys = [...hashes.map((h) => `hostsess:${h}`), `sc:${serverId}`, `gv:${serverId}`];
  for (let i = 0; i < keys.length; i += 128) await storage.delete(keys.slice(i, i + 128));
  return held;
}

/// A started session failed before a heartbeat showed it: its credentials go, and an allocation waiting on it is placed
/// again when it asks again.
async function failPlanned(storage, serverId) {
  const p = await storage.get(`p:${serverId}`);
  if (p && !p.failed) await storage.put(`p:${serverId}`, { ...p, failed: true, failed_at: p.failed_at ?? null });
  await revokeServer(storage, serverId);
}

/// The fleet as the policy sees it now: machines heard from in the last 30 s, each with the sessions it reported, the
/// sessions started there and not yet reported, seats reserved by allocations, and a pending stop shown as `stopping`.
async function loadView(storage, now) {
  const machines = await values(storage, 'm:');
  const planned = (await values(storage, 'p:')).filter((p) => !p.failed);
  const commands = await values(storage, 'c:');
  const requests = await values(storage, 'r:');
  // When each session's machine last reported it. A reservation whose player the listing has admitted (`arrived_at`)
  // is the host's to count from the first heartbeat after that; counting it here as well would seat one player twice.
  const reportedAt = new Map();
  for (const m of machines) for (const s of m.sessions || []) reportedAt.set(s.id, m.seen);
  const reserved = new Map();
  for (const r of requests) {
    if (!r.server_id || !(r.reserved_until > now)) continue;
    if (Number.isSafeInteger(r.arrived_at) && (reportedAt.get(r.server_id) || 0) >= r.arrived_at + ARRIVAL_SETTLE_MS) continue;
    reserved.set(r.server_id, (reserved.get(r.server_id) || 0) + 1);
  }
  const stopping = new Set(commands.filter((c) => c.do === 'stop').map((c) => c.server_id));
  // The VRAM each GPU session holds, from the fleet's own record of its start.
  const held = new Map((await entries(storage, 'gv:')).map(([k, mb]) => [k.slice('gv:'.length), mb]));
  const known = new Set();
  for (const m of machines) for (const s of m.sessions || []) known.add(s.id);
  for (const p of planned) known.add(p.server_id);
  const fresh = machines.filter((m) => now - m.seen <= HOST_STALE_MS);
  const state = {
    machines: fresh.map((m) => ({
      id: m.id, threads: m.threads, memory_mb: m.memory_mb, region: m.region, draining: m.draining,
      gpu_available: m.gpu_available === true, gpu_vram_mb: m.gpu_vram_mb || 0,
      sessions: (m.sessions || []).map((s) => ({ ...s })),
    })),
    last_machine_change_at: (await storage.get('machine_change_at')) ?? null,
  };
  for (const p of planned) {
    const m = state.machines.find((x) => x.id === p.machine_id);
    if (!m || m.sessions.some((s) => s.id === p.server_id)) continue;
    m.sessions.push({
      id: p.server_id, listing_id: p.listing_id, phase: 'starting', players: 0, reserved: 0, max_players: p.max_players,
      started_at: secs(p.created), empty_since: p.kind === 'warm' ? secs(p.created) : null,
    });
  }
  for (const m of state.machines) {
    for (const s of m.sessions) {
      s.reserved = Math.max(s.reserved || 0, reserved.get(s.id) || 0);
      if (s.reserved > 0) s.empty_since = null;
      if (stopping.has(s.id)) s.phase = 'stopping';
      delete s.gpu_vram_mb;
      if (held.get(s.id) > 0) s.gpu_vram_mb = held.get(s.id);
    }
  }
  const session = (id) => {
    for (const m of state.machines) for (const s of m.sessions) if (s.id === id) return { session: s, machine: m };
    return null;
  };
  return { state, known, session };
}

/// The recent clicks still inside the demand window, oldest dropped past MAX_RECENT.
async function recentClicks(storage, nowS, cfg) {
  const list = (await storage.get('recent')) || [];
  return list.filter((r) => r.at <= nowS && nowS - r.at <= cfg.demand_window_secs).slice(-MAX_RECENT);
}

async function noteClick(storage, click, cfg) {
  const list = await recentClicks(storage, click.at, cfg);
  list.push(click);
  await storage.put('recent', list.slice(-MAX_RECENT));
}

async function ensureSweep(storage, now) {
  const alarm = await storage.getAlarm();
  if (alarm === null || alarm === undefined) await storage.setAlarm(now + SWEEP_MS);
}

/// Keep the plan's machine recommendations for a person to read (GET /api/admin/fleet).
async function keepAdvice(storage, actions, now) {
  const advice = actions.filter((a) => a.action === 'recommend_add_machine' || a.action === 'recommend_release_machine');
  if (advice.length) await storage.put('advice', { at: now, actions: advice });
}

/// Ask a host to start a session: a planned session and a start command, delivered on the machine's next heartbeat.
/// `gpuVramMb`: the VRAM the session holds on the machine's GPU (its listing requires one), kept as `gv:`.
async function planStart(storage, { machineId, listingId, maxPlayers, kind, forRequest, now, gpuVramMb = 0 }) {
  const serverId = randomHex(16);
  const commandId = `cmd_${randomHex(12)}`;
  if (Number.isSafeInteger(gpuVramMb) && gpuVramMb > 0) await storage.put(`gv:${serverId}`, gpuVramMb);
  await storage.put({
    [`p:${serverId}`]: {
      server_id: serverId, machine_id: machineId, listing_id: listingId, max_players: maxPlayers, kind, created: now,
      command_id: commandId, acked_seq: null, failed: false, for_request: forRequest,
    },
    [`c:${machineId}:${commandId}`]: {
      command_id: commandId, do: 'start', machine_id: machineId, server_id: serverId, listing_id: listingId,
      max_players: maxPlayers, for_request: forRequest, created: now,
    },
  });
  return serverId;
}

const FLEET_OPS = {
  /// Route 1. The machine's credential was checked by the route; `machine_id` and `region` are the credential's.
  async heartbeat({ machine_id: machineId, beat, now }) {
    const storage = this.storage;
    const cfg = fleetConfig();
    // Sessions whose credentials go in this heartbeat. A server that has lost its credential can neither renew nor remove
    // its own entry on the listing, so the route takes each off the listing's live session at once.
    const ended = [];
    const end = async (serverId) => {
      const held = await revokeServer(storage, serverId);
      if (held && held.machine_id === machineId) ended.push({ sim_id: held.sim_id, server_id: serverId });
    };
    const answer = (commands) => ({ ok: true, seq: beat.seq, commands, next_heartbeat_secs: HEARTBEAT_SECS, ended });
    const key = `m:${machineId}`;
    const prev = await storage.get(key);
    // A replayed or reordered heartbeat changes nothing and gets nothing.
    if (prev && prev.boot_id === beat.boot_id && beat.seq <= prev.seq) return answer([]);

    const commandKeys = async () => (await entries(storage, `c:${machineId}:`));
    const myPlanned = async () => (await values(storage, 'p:')).filter((p) => p.machine_id === machineId && !p.failed);

    if (!prev) await storage.put('machine_change_at', secs(now));
    if (prev && prev.boot_id !== beat.boot_id) {
      // The agent restarted: its unacked commands are forgotten, and what it reports is the truth.
      for (const [k, c] of await commandKeys()) {
        await storage.delete(k);
        if (c.do === 'start') await failPlanned(storage, c.server_id);
      }
      const reported = new Set(beat.sessions.map((s) => s.id));
      for (const p of await myPlanned()) if (!reported.has(p.server_id)) await failPlanned(storage, p.server_id);
    }

    for (const id of beat.acked) {
      const c = await storage.get(`c:${machineId}:${id}`);
      if (!c) continue;
      await storage.delete(`c:${machineId}:${id}`);
      if (c.do === 'start') {
        const p = await storage.get(`p:${c.server_id}`);
        if (p && !p.failed) await storage.put(`p:${c.server_id}`, { ...p, acked_seq: beat.seq });
      } else if (c.do === 'stop') {
        // The stop is acknowledged: the session's credentials go now.
        await end(c.server_id);
      }
    }
    for (const id of beat.rejected) {
      const c = await storage.get(`c:${machineId}:${id}`);
      if (!c) continue;
      await storage.delete(`c:${machineId}:${id}`);
      if (c.do === 'start') await failPlanned(storage, c.server_id);
    }
    for (const f of beat.freed) {
      const p = await storage.get(`p:${f.id}`);
      // Only this machine's own sessions: a host cannot end another machine's.
      if (p && p.machine_id !== machineId) continue;
      if (p) await failPlanned(storage, f.id);
      const owned = p || (prev?.sessions || []).some((s) => s.id === f.id);
      if (owned) await end(f.id);
    }
    const reported = new Set(beat.sessions.map((s) => s.id));
    for (const p of await myPlanned()) {
      if (reported.has(p.server_id)) await storage.delete(`p:${p.server_id}`);
      else if (p.acked_seq !== null && p.acked_seq < beat.seq) await failPlanned(storage, p.server_id);
    }
    // A session this machine reported before and no longer reports has ended: its credentials go.
    for (const s of prev?.sessions || []) if (!reported.has(s.id)) await end(s.id);
    for (const [k, c] of await commandKeys()) {
      if (now - c.created <= COMMAND_TTL_MS) continue;
      await storage.delete(k);
      if (c.do === 'start') await failPlanned(storage, c.server_id);
    }
    // A host speaks for its own sessions only: an id another machine reports, or one started on another machine, is
    // left out of this machine's view.
    const elsewhere = new Set();
    for (const m of await values(storage, 'm:')) if (m.id !== machineId) for (const s of m.sessions || []) elsewhere.add(s.id);
    for (const p of await values(storage, 'p:')) if (p.machine_id !== machineId) elsewhere.add(p.server_id);
    const sessions = beat.sessions.filter((s) => !elsewhere.has(s.id));
    await storage.put(key, {
      id: machineId, threads: beat.machine.threads, memory_mb: beat.machine.memory_mb, region: beat.machine.region,
      draining: beat.machine.draining, boot_id: beat.boot_id, seq: beat.seq, seen: now, sessions, agent_version: beat.agent_version,
      gpu: beat.machine.gpu || '', gpu_vram_mb: beat.machine.gpu_vram_mb || 0, gpu_available: beat.machine.gpu_available === true,
    });

    // The warm pool and surplus stops, fleet-wide. Starts and stops for other machines wait for their own heartbeats.
    const view = await loadView(storage, now);
    const actions = plan(secs(now), view.state, { pending: [], recent: await recentClicks(storage, secs(now), cfg) }, cfg);
    const pendingStops = new Set((await values(storage, 'c:')).filter((c) => c.do === 'stop').map((c) => c.server_id));
    for (const a of actions) {
      if (a.action === 'start_warm') {
        await planStart(storage, {
          machineId: a.machine_id, listingId: a.listing_id, maxPlayers: a.max_players, kind: 'warm', forRequest: null, now, gpuVramMb: a.gpu_vram_mb || 0,
        });
      } else if (a.action === 'stop' && !pendingStops.has(a.session_id)) {
        const commandId = `cmd_${randomHex(12)}`;
        await storage.put(`c:${a.machine_id}:${commandId}`, { command_id: commandId, do: 'stop', machine_id: a.machine_id, server_id: a.session_id, created: now });
      }
    }
    await keepAdvice(storage, actions, now);

    // This machine's commands. A start carries a freshly minted eshs_; only its hash is kept.
    const out = [];
    for (const [, c] of await commandKeys()) {
      if (c.do === 'stop') {
        out.push({ command_id: c.command_id, do: 'stop', server_id: c.server_id });
        continue;
      }
      const p = await storage.get(`p:${c.server_id}`);
      if (!p || p.failed) continue;
      // A session runs the listing's server scripts from its source world, so no session starts for a listing without one,
      // whatever asked for it (a player's click or the warm pool). Read fresh at each send; a listing that cannot be read
      // is not started either. The route refuses first (handleAllocate); this covers a warm start and a source dropped since.
      if (!(await this.listingHostable(c.listing_id))) {
        await storage.delete(`c:${machineId}:${c.command_id}`);
        await failPlanned(storage, c.server_id);
        continue;
      }
      const token = mintSecret('eshs_');
      const hash = await sha256Hex(token);
      await storage.put(`hostsess:${hash}`, {
        v: 1, sim_id: c.listing_id, machine_id: machineId, server_id: c.server_id, expires_at: now + SESSION_CREDENTIAL_START_MS,
      });
      await storage.put(`sc:${c.server_id}`, [...((await storage.get(`sc:${c.server_id}`)) || []), hash]);
      out.push({
        command_id: c.command_id, do: 'start', listing_id: c.listing_id, max_players: c.max_players, server_id: c.server_id,
        for_request: c.for_request ?? null, session_credential: token,
      });
    }
    await ensureSweep(storage, now);
    return answer(out);
  },

  /// Route 2. The route has checked who the caller is and that they may play this listing, and passes the listing's
  /// live servers as Quick Join ranks them (`rows`: id, players, max_players, full; no links).
  // `gpu`, `gpu_min_vram_mb`: the listing's GPU setting (fleet_policy.mjs gpuSettingOf); absent is 'none'.
  async allocate({
    who, request_id: requestId, sim_id: simId, max_players: maxPlayers, region, rows, now, guest, hostable,
    gpu = 'none', gpu_min_vram_mb: gpuMinVramMb = 0,
  }) {
    const storage = this.storage;
    const cfg = fleetConfig();
    if (typeof who !== 'string' || !WHO.test(who) || typeof requestId !== 'string' || !FLEET_ID.test(requestId)
      || typeof simId !== 'string' || !LISTING_ID.test(simId) || !whole(maxPlayers, 1, MAX_PLAYERS) || !Array.isArray(rows)
      || !Number.isSafeInteger(now) || !GPU_MODES.includes(gpu) || !whole(gpuMinVramMb, 0, MAX_GPU_VRAM_MB)) {
      return refuse(400, 'bad_request', 'bad allocation');
    }
    const gpuNeed = gpu === 'required' ? gpuMinVramMb : null;
    const gpuFields = gpu === 'required' ? { gpu, gpu_min_vram_mb: gpuMinVramMb } : {};
    const key = `r:${who}:${requestId}`;
    let rec = await storage.get(key);
    if (rec && now - rec.at > REQUEST_KEEP_MS) rec = undefined;
    if (rec && rec.listing_id !== simId) return refuse(409, 'request_id_in_use', 'This request_id was used for another listing');
    const fresh = !rec;
    if (fresh) {
      const times = ((await storage.get(`rate:${who}`)) || []).filter((t) => now - t < 60_000);
      if (times.length >= ALLOCATIONS_PER_MINUTE) return refuse(429, 'rate_limited', 'Too many Play requests. Try again in a minute.');
      times.push(now);
      await storage.put(`rate:${who}`, times);
    }
    await ensureSweep(storage, now);
    const view = await loadView(storage, now);
    const row = (id) => rows.find((r) => r && r.id === id);
    const seat = { reserved_until: now + RESERVATION_MS };
    const save = async (r) => { await storage.put(key, { ...r, listing_id: simId, at: rec?.at ?? now }); };
    const join = (serverId, warm) => ({ ok: true, status: 'join', server_id: serverId, warm });
    const starting = (serverId) => ({ ok: true, status: 'starting', server_id: serverId, retry_after_secs: cfg.retry_after_secs });
    // No session may start for a listing without a source world, so its clicks are not demand the warm pool answers.
    const canStart = hostable === true;
    const click = (neededNew) => (fresh && canStart ? noteClick(storage, { listing_id: simId, at: secs(now), max_players: maxPlayers, region: region || null, needed_new_session: neededNew, ...gpuFields }, cfg) : null);

    // The same request_id again: the same session while it lives. A start becomes a join once its /live entry has a link.
    if (rec && (rec.status === 'join' || rec.status === 'starting') && rec.server_id) {
      const live = row(rec.server_id);
      const fleetSession = view.session(rec.server_id);
      const alive = fleetSession && fleetSession.session.phase !== 'stopping';
      if (rec.status === 'join' && live && (!view.known.has(rec.server_id) || alive)) return join(rec.server_id, rec.warm === true);
      if (alive) {
        if (live) {
          await save({ ...rec, status: 'join' });
          return join(rec.server_id, rec.warm === true);
        }
        return starting(rec.server_id);
      }
      // The server never hosted, or is gone (host-failed): placed again below.
    }

    // 1. The listing's live servers, in the Quick Join order (rankServers in live.mjs: not full first, then fullness,
    // hardware tier and ping, then players): the first that can take the player gets them, whoever hosts it. A creator's
    // own host, a fleet session with players, and an empty (warm) fleet session that already answers at a link all stand
    // in that one order, so the best ranked host is never passed over for a lower ranked one.
    for (const r of rows) {
      if (!r || typeof r.id !== 'string' || r.full === true) continue;
      if (view.known.has(r.id)) {
        if (guest) continue;
        const found = view.session(r.id);
        if (!found || found.session.phase === 'stopping') continue;
        // A listing that requires a GPU is seated on a fleet machine only when that machine has a GPU of its size.
        if (gpuNeed !== null && !gpuClassFits(found.machine, gpuNeed)) continue;
        const s = found.session;
        const seated = Math.max(Number(r.players) || 0, s.players) + s.reserved;
        if (seated >= s.max_players) continue;
        const warm = seated === 0;
        await save({ status: 'join', server_id: r.id, warm, ...seat });
        await click(false);
        return join(r.id, warm);
      }
      await save({ status: 'join', server_id: r.id, warm: false, reserved_until: 0 });
      await click(false);
      return join(r.id, false);
    }
    // A guest joins a guest-only session a host runs; the fleet starts none (HOSTED_FLEET.md, not implemented).
    if (guest) {
      await save({ status: 'full', server_id: null, reserved_until: 0 });
      return { ok: true, status: 'full' };
    }

    // No live server could take the player, and a listing without a source world gets no session from the fleet: one
    // would run with no server scripts (docs/architecture/PRIVATE_SOURCE_SECURITY_REVIEW.md).
    if (!canStart) {
      await save({ status: 'no_source', server_id: null, reserved_until: 0 });
      return { ok: true, status: 'no_source' };
    }

    // 2. The policy: a warm server, a session already starting, or a start on the fullest machine that fits.
    const nowS = secs(now);
    const [actions] = project(nowS, view.state, {
      pending: [{ request_id: requestId, listing_id: simId, max_players: maxPlayers, region: region || null, at: nowS, ...gpuFields }], recent: [],
    }, cfg);
    const action = actions.find((a) => a.request_id === requestId);
    if (!action || action.action === 'refuse') {
      await keepAdvice(storage, actions, now);
      await save({ status: 'full', server_id: null, reserved_until: 0 });
      await click(true);
      // A listing that requires a GPU is never started on a CPU machine: the refusal says why.
      if (action?.reason === 'no_gpu_capacity') return { ok: true, status: 'full', code: 'no_gpu_capacity', gpu_min_vram_mb: gpuMinVramMb };
      return { ok: true, status: 'full' };
    }
    if (action.action === 'route' || action.action === 'wait') {
      const linked = action.action === 'route' && !!row(action.session_id);
      await save({ status: linked ? 'join' : 'starting', server_id: action.session_id, warm: action.action === 'route' && action.warm, ...seat });
      await click(true);
      return linked ? join(action.session_id, action.warm) : starting(action.session_id);
    }
    // start_for_player
    const serverId = await planStart(storage, {
      machineId: action.machine_id, listingId: simId, maxPlayers, kind: 'occupied', forRequest: requestId, now, gpuVramMb: action.gpu_vram_mb || 0,
    });
    await save({ status: 'starting', server_id: serverId, warm: false, ...seat });
    await click(true);
    return starting(serverId);
  },

  /// Whether an eshs_ (by its hash) stands for `sim_id`, and for `server_id` when one is named. `beat`: an accepted /live
  /// heartbeat, which moves the credential's expiry to 90 s from now.
  async session_credential({ hash, sim_id: simId, server_id: serverId, now, beat }) {
    if (typeof hash !== 'string' || !/^[0-9a-f]{64}$/.test(hash) || !Number.isSafeInteger(now)) return refuse(401, 'bad_session_credential');
    const key = `hostsess:${hash}`;
    const rec = await this.storage.get(key);
    if (!rec || rec.v !== 1) return refuse(401, 'bad_session_credential');
    if (rec.expires_at <= now) {
      await this.storage.delete(key);
      return refuse(401, 'bad_session_credential');
    }
    if (rec.sim_id !== simId) return refuse(401, 'bad_session_credential');
    if (serverId !== null && serverId !== undefined && rec.server_id !== serverId) return refuse(401, 'bad_session_credential');
    if (beat === true) await this.storage.put(key, { ...rec, expires_at: now + SESSION_CREDENTIAL_BEAT_MS });
    return { ok: true, machine_id: rec.machine_id, server_id: rec.server_id };
  },

  /// The listing admitted `who` to the fleet session `server_id` (src/live.mjs, an admission made with that session's own
  /// eshs_): the seat an allocation reserved for them there is marked arrived. loadView stops counting it once the
  /// session's machine has heartbeated after the admission, because the host then counts the player itself.
  async arrived({ who, server_id: serverId, now }) {
    if (typeof who !== 'string' || !WHO.test(who) || typeof serverId !== 'string' || !FLEET_ID.test(serverId) || !Number.isSafeInteger(now)) {
      return refuse(400, 'bad_request', 'bad arrival');
    }
    let marked = 0;
    for (const [key, r] of await entries(this.storage, `r:${who}:`)) {
      if (r.server_id !== serverId || !(r.reserved_until > now) || Number.isSafeInteger(r.arrived_at)) continue;
      await this.storage.put(key, { ...r, arrived_at: now });
      marked += 1;
    }
    return { ok: true, marked };
  },

  /// For an administrator: each machine, its lines and room, occupancy, and the latest recommendation. No secret, no
  /// request id, no account.
  async status({ now }) {
    const cfg = fleetConfig();
    const view = await loadView(this.storage, now);
    const machines = (await values(this.storage, 'm:')).map((m) => {
      const live = view.state.machines.find((x) => x.id === m.id);
      return {
        id: m.id, region: m.region, threads: m.threads, memory_mb: m.memory_mb, draining: m.draining,
        gpu: m.gpu || '', gpu_vram_mb: m.gpu_vram_mb || 0, gpu_available: m.gpu_available === true,
        seen_secs_ago: Math.max(0, secs(now - m.seen)), stale: !live, agent_version: m.agent_version,
        sessions: (live || m).sessions.length, capacity: live ? capacity(live, cfg) : null,
      };
    });
    const [occupied, running] = occupancy(view.state);
    return { ok: true, machines, occupied, running, advice: (await this.storage.get('advice')) || null };
  },
};

/// Drop what is past its time: answers older than 10 minutes, rate windows, expired credentials, failed starts.
async function sweep(storage, now) {
  const gone = [];
  for (const [k, r] of await entries(storage, 'r:')) if (now - r.at > REQUEST_KEEP_MS) gone.push(k);
  for (const [k, t] of await entries(storage, 'rate:')) if (!Array.isArray(t) || t.every((x) => now - x >= 60_000)) gone.push(k);
  const expired = new Set();
  for (const [k, c] of await entries(storage, 'hostsess:')) {
    if (!(c?.expires_at > now)) {
      gone.push(k);
      expired.add(k.slice('hostsess:'.length));
    }
  }
  for (const [k, p] of await entries(storage, 'p:')) if (p.failed && now - p.created > REQUEST_KEEP_MS) gone.push(k);
  for (let i = 0; i < gone.length; i += 128) await storage.delete(gone.slice(i, i + 128));
  for (const [k, hashes] of await entries(storage, 'sc:')) {
    const left = (hashes || []).filter((h) => !expired.has(h));
    if (left.length === 0) await storage.delete(k);
    else if (left.length !== hashes.length) await storage.put(k, left);
  }
  // A GPU hold outlives neither its session's credentials nor a start that failed.
  for (const [k] of await entries(storage, 'gv:')) {
    const serverId = k.slice('gv:'.length);
    const p = await storage.get(`p:${serverId}`);
    if ((p && !p.failed) || (await storage.get(`sc:${serverId}`)) !== undefined) continue;
    await storage.delete(k);
  }
  const cfg = fleetConfig();
  if ((await storage.get('recent')) !== undefined) await storage.put('recent', await recentClicks(storage, secs(now), cfg));
}

/// The fleet: every machine, every session started, every allocation. One instance, named "fleet".
export class Fleet {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
  }

  async fetch(request) {
    const op = new URL(request.url).pathname.slice(1);
    let args = {};
    try {
      const v = await request.json();
      if (v && typeof v === 'object' && !Array.isArray(v)) args = v;
    } catch {
      args = {};
    }
    let result;
    try {
      const handler = Object.hasOwn(FLEET_OPS, op) ? FLEET_OPS[op] : null;
      result = handler ? await handler.call(this, args) : refuse(404, 'unknown_operation');
    } catch (e) {
      console.error(`Fleet.${op} failed:`, e?.name || 'Error');
      result = refuse(500, 'fleet_failed');
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  /// Whether a session may start for this listing: it has a source world for the world it plays (src/world.mjs
  /// hasSourceWorld). A listing that cannot be read is not started. Without a SOCIAL binding (a test of placement alone)
  /// the route's check in handleAllocate is the only one.
  async listingHostable(simId) {
    if (!this.env?.SOCIAL) return true;
    try {
      const raw = await this.env.SOCIAL.get(`sim:${simId}`);
      return !!raw && hasSourceWorld(JSON.parse(raw));
    } catch {
      return false;
    }
  }

  async alarm() {
    const now = Date.now();
    await sweep(this.storage, now);
    for (const prefix of ['r:', 'rate:', 'hostsess:', 'p:', 'c:', 'gv:']) {
      if ((await this.storage.list({ prefix, limit: 1 })).size > 0) {
        await this.storage.setAlarm(now + SWEEP_MS);
        return;
      }
    }
  }
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

const NO_STORE = { 'Cache-Control': 'no-store' };

/// POST /api/fleet/hosts/{machine_id}/heartbeat. `deps`: json.
export async function handleFleetHeartbeat(request, env, cors, machineId, deps) {
  const { json } = deps;
  const headers = { ...cors, ...NO_STORE };
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, headers);
  // The credential before anything else: a request without one learns nothing about any machine.
  const host = await hostFromCredential(env, bearerOf(request));
  if (!host) return json({ error: 'This host credential is not valid', code: 'bad_host_credential' }, 401, headers);
  if (!sameText(host.machine_id, machineId)) {
    return json({ error: 'This host credential is for another machine', code: 'wrong_machine' }, 403, headers);
  }
  if (!env.FLEET) return json({ error: 'The fleet is not available', code: 'fleet_unavailable', retry_after_secs: FULL_RETRY_SECS }, 503, headers);
  const text = await request.text();
  if (text.length > MAX_HEARTBEAT_BYTES) return json({ error: 'The heartbeat is too large', code: 'bad_heartbeat', field: 'body' }, 400, headers);
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return json({ error: 'The heartbeat is not JSON', code: 'bad_heartbeat', field: 'body' }, 400, headers);
  }
  const checked = checkHeartbeat(body, host.region);
  if (!checked.ok) return json({ error: `${checked.field} is missing or out of range`, code: 'bad_heartbeat', field: checked.field }, 400, headers);
  const now = Date.now();
  const r = await fleetCall(env, 'heartbeat', { machine_id: host.machine_id, beat: checked.value, now });
  if (r?.ok !== true) return json({ error: 'The fleet is not available', code: 'fleet_unavailable', retry_after_secs: FULL_RETRY_SECS }, 503, headers);
  // Each session whose credentials just went leaves its listing now, so the page never hands out a link nobody
  // answers. A failure here never fails the heartbeat: the entry then lapses by itself within 90 s.
  for (const e of Array.isArray(r.ended) ? r.ended : []) {
    await delistServer(env, e.sim_id, e.server_id, now).catch((error) => console.error('delist failed:', error?.message || error));
  }
  return json({ seq: r.seq, commands: r.commands, next_heartbeat_secs: r.next_heartbeat_secs }, 200, headers);
}

/// POST and DELETE /api/admin/fleet/hosts/{machine_id}/credential, and GET /api/admin/fleet. `deps`: requireAdmin, json.
export async function handleFleetAdmin(request, env, cors, path, deps) {
  const { requireAdmin, json } = deps;
  const headers = { ...cors, ...NO_STORE };
  const admin = await requireAdmin(request, env);
  if (!admin) return json({ error: 'Admin access required' }, 403, headers);
  if (path === '/api/admin/fleet') {
    if (request.method !== 'GET') return json({ error: 'Method not allowed' }, 405, headers);
    const r = await fleetCall(env, 'status', { now: Date.now() });
    if (r?.ok !== true) return json({ error: 'The fleet is not available', code: 'fleet_unavailable' }, 503, headers);
    return json({ machines: r.machines, occupied: r.occupied, running: r.running, advice: r.advice }, 200, headers);
  }
  const m = /^\/api\/admin\/fleet\/hosts\/([^/]+)\/credential$/.exec(path);
  if (!m) return json({ error: 'Not found' }, 404, headers);
  const machineId = m[1];
  if (!FLEET_ID.test(machineId)) return json({ error: 'machine_id must be 1 to 64 letters, digits, _ or -', code: 'bad_machine_id' }, 400, headers);
  if (!env.SOCIAL) return json({ error: 'The fleet is not available', code: 'fleet_unavailable' }, 503, headers);
  if (request.method === 'DELETE') return json({ ok: true, revoked: await revokeHostCredential(env, machineId) }, 200, headers);
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, headers);
  const body = await bodyObject(request);
  const region = body.region === undefined || body.region === null ? '' : body.region;
  if (typeof region !== 'string' || (region !== '' && !FLEET_ID.test(region))) {
    return json({ error: 'region must be 1 to 64 letters, digits, _ or -', code: 'bad_region' }, 400, headers);
  }
  const token = await mintHostCredential(env, machineId, region);
  return json({ ok: true, token, machine_id: machineId, region }, 200, headers);
}

/// The seats a new session for this listing gets: the listing's own max_players, else 10 (as the gallery shows it).
const listingSeats = (sim) => (Number.isSafeInteger(sim?.max_players) && sim.max_players >= 1 ? Math.min(sim.max_players, MAX_PLAYERS) : 10);

/// POST /api/simulations/{id}/allocate. Who may ask, and about which listing, are GET /live's rules: a signed-in account,
/// or a guest holding a live lease on this listing; the listing listable, or the viewer its author or an administrator;
/// and the adult content gate. `deps`: verifyAuth, requireAdmin, json, isListable.
export async function handleAllocate(request, env, cors, simId, deps) {
  const { verifyAuth, requireAdmin, json, isListable } = deps;
  const headers = { ...cors, ...NO_STORE };
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, headers);
  const raw = env.SOCIAL ? await env.SOCIAL.get(`sim:${simId}`) : null;
  if (!raw) return json({ error: 'Simulation not found' }, 404, headers);
  const sim = await simWithLegalLock(env, JSON.parse(raw));
  const now = Date.now();
  const signedIn = await verifyAuth(request, env);
  if (!isListable(sim)) {
    const admin = signedIn ? await requireAdmin(request, env) : null;
    if (!signedIn || (signedIn !== sim.author_id && !admin)) return json({ error: 'Simulation not found' }, 404, headers);
  } else {
    const refused = await refuseAdult(request, env, sim, { authenticate: verifyAuth, json, cors: headers });
    if (refused) return refused;
  }
  const leaseHeader = request.headers.get('X-Eustress-Lease');
  const lease = signedIn || !guestPlayOn(env) ? false : await leaseIsLive(env, leaseHeader, now, simId);
  if (!signedIn && !lease) return json({ error: 'Sign in to play', code: 'sign_in_required' }, 401, headers);

  const body = await bodyObject(request);
  if (typeof body.request_id !== 'string' || !FLEET_ID.test(body.request_id)) {
    return json({ error: 'request_id must be 1 to 64 letters, digits, _ or -', code: 'bad_request_id' }, 400, headers);
  }
  const region = body.region === undefined || body.region === null ? '' : body.region;
  if (typeof region !== 'string' || (region !== '' && !FLEET_ID.test(region))) {
    return json({ error: 'region must be 1 to 64 letters, digits, _ or -', code: 'bad_region' }, 400, headers);
  }
  const unavailable = () => json({ error: 'Hosted sessions are not available right now', code: 'fleet_unavailable', retry_after_secs: FULL_RETRY_SECS }, 503, headers);
  if (!env.FLEET) return unavailable();

  // The caller's key: the account, or a hash of the lease (never the lease itself).
  const who = signedIn ? `u:${signedIn}` : `g:${(await sha256Hex(String(leaseHeader))).slice(0, 32)}`;
  if (!WHO.test(who)) return json({ error: 'Unauthorized' }, 401, headers);
  const rows = await liveServerRows(env, simId, now, { guestOnly: !!lease });
  // A session runs the listing's server scripts from its source world: without one the fleet starts nothing. Nor for a
  // listing that is deleted or under a legal lock, whose world a session credential cannot read (src/world.mjs).
  const closed = !!sim.deletion || !!(await legalLockOf(env, simId));
  const r = await fleetCall(env, 'allocate', {
    who, request_id: body.request_id, sim_id: simId, max_players: listingSeats(sim), region, now, guest: !!lease,
    hostable: !closed && hasSourceWorld(sim),
    rows: rows.map((x) => ({ id: x.id, players: x.players, max_players: x.max_players, full: x.full })),
    ...gpuSettingOf(sim),
  });
  if (r?.ok !== true) {
    if (r?.status === 429) return json({ error: 'Too many Play requests. Try again in a minute.', code: 'rate_limited' }, 429, headers);
    if (r?.status === 409) return json({ error: 'This request_id was used for another listing', code: 'request_id_in_use' }, 409, headers);
    return unavailable();
  }
  if (r.status === 'join') {
    const row = rows.find((x) => x.id === r.server_id);
    if (row) return json({ status: 'join', link: row.link, cert_sha256: row.cert_sha256 ?? null, server_id: r.server_id, warm: r.warm === true }, 200, headers);
    return json({ status: 'starting', server_id: r.server_id, retry_after_secs: fleetConfig().retry_after_secs }, 202, headers);
  }
  if (r.status === 'starting') return json({ status: 'starting', server_id: r.server_id, retry_after_secs: r.retry_after_secs }, 202, headers);
  if (r.status === 'no_source') {
    if (closed) return json({ status: 'unavailable', code: 'not_hostable', error: 'Eustress cannot host a session of this listing right now.' }, 409, headers);
    return json({
      status: 'unavailable', code: 'no_source_world',
      error: 'This listing has no source world, so Eustress cannot host a session of it. Its author publishes it again from Studio.',
    }, 409, headers);
  }
  if (r.code === 'no_gpu_capacity') {
    return json({
      status: 'full', code: 'no_gpu_capacity', retry_after_secs: FULL_RETRY_SECS, gpu_min_vram_mb: r.gpu_min_vram_mb,
      error: `This Space needs a GPU with at least ${r.gpu_min_vram_mb} MB of memory, and no such machine has room right now.`,
    }, 503, headers);
  }
  return json({ status: 'full', code: 'no_capacity', retry_after_secs: FULL_RETRY_SECS }, 503, headers);
}
