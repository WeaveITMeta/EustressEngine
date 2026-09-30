// =============================================================================
// Live hosts: which published simulations someone is hosting right now
// =============================================================================
//
// Studio's host path (F9) sends a heartbeat while it hosts, carrying the join
// link the host itself produced (JoinLink::to_link in eustress-networking). A
// simulation's listing reads it back to show Live or Offline and to hand that
// link, unchanged, to the desktop Player. The goal and its milestones are in
// docs/launch/GOAL_LIVE_GALLERY_PLAY.md (M3).
//
//     POST   /api/simulations/{id}/live   heartbeat, the author only
//     DELETE /api/simulations/{id}/live   stopped hosting, the author only
//     GET    /api/simulations/{id}/live   { live, link?, players, ... } for the listing
//
// One Durable Object per simulation holds its current host. Each heartbeat
// rewrites it and moves its alarm out to 90 s; if the host stops without
// saying so, the alarm clears it and the listing goes Offline by itself. A
// Durable Object rather than KV because KV's Free plan allows 1,000 writes a
// day, one host's 30 s heartbeat alone is 2,880, and a KV read can trail a
// write by a minute, long enough to hand out a link that no longer answers.
// =============================================================================

/// A host with no heartbeat for this long is Offline. Three missed 30 s beats.
export const HEARTBEAT_TTL_MS = 90_000;

const MAX_LINK_LENGTH = 512;
const MAX_PLAYERS = 10_000;

// -----------------------------------------------------------------------------
// Join links
// -----------------------------------------------------------------------------

/// Why a link's host can never serve a gallery join, or `null` when it can.
///
/// Refused: the player's own machine (loopback, `localhost`, the unspecified
/// address) and link-local addresses, which mean something only on one
/// network segment. Private LAN addresses stay allowed, because Stage 1's
/// link is the workstation's LAN address and invited players reach it through
/// the tunnel's private route. The host is read through the URL parser first,
/// which reads numeric hosts the way inet_aton and so most system resolvers
/// do, so 2130706433, 0x7f.1 and [::ffff:127.0.0.1] are caught as the
/// loopback they are. A name that only resolves to loopback in DNS is not
/// caught here; the pin still keeps the Player from completing a handshake
/// with anything but the host.
function localOnlyHost(host) {
  let name;
  try {
    name = new URL(`http://${host}/`).hostname;
  } catch {
    return 'host is not a valid address';
  }
  const reason = 'link must name an address other players can reach, not loopback or link-local';
  // Resolvers read `localhost.` as `localhost`.
  const bare = name.replace(/\.+$/, '');
  if (bare === 'localhost' || bare.endsWith('.localhost')) return reason;
  const local4 = (a, b) => a === 127 || a === 0 || (a === 169 && b === 254);
  const v4 = /^(\d+)\.(\d+)\.\d+\.\d+$/.exec(name);
  if (v4) return local4(Number(v4[1]), Number(v4[2])) ? reason : null;
  if (name.startsWith('[')) {
    const v6 = name.slice(1, -1);
    if (v6 === '::1' || v6 === '::' || /^fe[89ab][0-9a-f]:/.test(v6)) return reason;
    // An IPv4-mapped address, which the parser writes as two hex groups.
    const mapped = /^::ffff:([0-9a-f]{1,4}):[0-9a-f]{1,4}$/.exec(v6);
    if (mapped) {
      const high = parseInt(mapped[1], 16);
      return local4(high >> 8, high & 0xff) ? reason : null;
    }
  }
  return null;
}

/// Check a Player join link before it is stored and handed to anyone:
///
///     eustress-player://join/<host>:<port>?key=<32 hex>&pin=<64 hex>
///
/// Exactly what the host produces (JoinLink::to_link with new_join_key in
/// eustress-networking), and nothing else. The listing hands this string to
/// window.location, so anything looser would let a heartbeat point a Play
/// button at another scheme or another page. Only `eustress-player://` is
/// accepted, since `eustress://` opens Studio rather than the Player. The pin,
/// the SHA-256 of the host's certificate, is required because a gallery join
/// is never to the player's own machine.
export function checkJoinLink(link) {
  if (typeof link !== 'string' || link.length === 0) return { ok: false, error: 'link required' };
  if (link.length > MAX_LINK_LENGTH) return { ok: false, error: `link longer than ${MAX_LINK_LENGTH} characters` };
  const m = /^eustress-player:\/\/join\/(\[[0-9A-Fa-f:.]+\]|[A-Za-z0-9.-]+):(\d{1,5})\?([^#\s]*)$/.exec(link);
  if (!m) return { ok: false, error: 'link must be eustress-player://join/<host>:<port>?key=<32 hex>&pin=<64 hex>' };
  const port = Number(m[2]);
  if (port < 1 || port > 65535) return { ok: false, error: 'port must be 1 to 65535' };
  const hostProblem = localOnlyHost(m[1]);
  if (hostProblem) return { ok: false, error: hostProblem };

  const params = new URLSearchParams(m[3]);
  const names = [...params.keys()].sort();
  if (names.length !== 2 || names[0] !== 'key' || names[1] !== 'pin') {
    return { ok: false, error: 'link takes exactly two parameters, key and pin' };
  }
  const key = params.get('key');
  if (!/^[0-9A-Fa-f]{32}$/.test(key)) return { ok: false, error: 'key must be 32 hex characters' };
  const pin = params.get('pin');
  if (!/^[0-9A-Fa-f]{64}$/.test(pin)) return { ok: false, error: 'pin must be 64 hex characters' };
  return { ok: true, host: m[1], port, key, pin };
}

/// A whole count in range, or `null` when the value is not one.
function count(value, min, max) {
  return Number.isSafeInteger(value) && value >= min && value <= max ? value : null;
}

// -----------------------------------------------------------------------------
// The Durable Object
// -----------------------------------------------------------------------------

const LIVE_OPS = {
  async beat({ host_id, link, players, max_players, protocol, now }) {
    const prev = await this.storage.get('host');
    // `since` is when the host started answering at this link. A restart
    // brings a new certificate, so a new pin and a new link, and a new since.
    const continuing = prev && prev.link === link && now - prev.last_seen <= HEARTBEAT_TTL_MS;
    const host = {
      host_id,
      link,
      players,
      max_players,
      protocol,
      since: continuing ? prev.since : now,
      last_seen: now,
    };
    await this.storage.put('host', host);
    await this.storage.setAlarm(now + HEARTBEAT_TTL_MS);
    return { ok: true, expires_at: new Date(now + HEARTBEAT_TTL_MS).toISOString() };
  },

  async stop() {
    await this.storage.delete('host');
    await this.storage.deleteAlarm();
    return { ok: true };
  },

  async status({ now }) {
    const host = await this.storage.get('host');
    if (!host || now - host.last_seen > HEARTBEAT_TTL_MS) return { ok: true, live: false };
    return {
      ok: true,
      live: true,
      link: host.link,
      players: host.players,
      max_players: host.max_players,
      protocol: host.protocol,
      since: host.since,
      last_seen: host.last_seen,
    };
  },
};

/// The current host of one simulation. Named by the simulation id.
export class LiveHost {
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
      const handler = LIVE_OPS[op];
      result = handler ? await handler.call(this, args) : { ok: false, status: 404, error: `unknown operation ${op}` };
    } catch (e) {
      console.error(`LiveHost.${op} failed:`, e);
      result = { ok: false, status: 500, error: String(e?.message || e) };
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  /// No heartbeat for HEARTBEAT_TTL_MS: the host is gone. A beat that landed
  /// after this alarm was set has moved the deadline, so check before clearing.
  async alarm() {
    const host = await this.storage.get('host');
    if (!host) return;
    const due = host.last_seen + HEARTBEAT_TTL_MS;
    if (Date.now() >= due) await this.storage.delete('host');
    else await this.storage.setAlarm(due);
  }
}

async function liveCall(env, simId, op, args) {
  const stub = env.LIVE_HOSTS.get(env.LIVE_HOSTS.idFromName(simId));
  const res = await stub.fetch(`https://live.internal/${op}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args),
  });
  try {
    return await res.json();
  } catch {
    return { ok: false, status: 502, error: `${op}: unreadable reply` };
  }
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

/// `deps` carries the index.js helpers: verifyAuth, requireAdmin, json, and
/// isListable from moderation.mjs.
export async function handleLive(request, env, cors, simId, deps) {
  const { verifyAuth, requireAdmin, json, isListable } = deps;
  if (!env.LIVE_HOSTS) {
    return json({ error: 'Live hosting is not configured on this deployment', code: 'live_unavailable' }, 503, cors);
  }
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(raw);
  const now = Date.now();

  if (request.method === 'GET') {
    // Always fresh: a cached "live" could hand out a link that has closed.
    const headers = { ...cors, 'Cache-Control': 'no-store' };
    // Until a listing is approved its host is not public: the moderation gate
    // covers joining a world, not only seeing its page. Its author and admins
    // still see it, so a host can be checked before approval.
    if (!isListable(sim)) {
      const viewer = await verifyAuth(request, env);
      const admin = viewer ? await requireAdmin(request, env) : null;
      if (viewer !== sim.author_id && !admin) return json({ live: false }, 200, headers);
    }
    const s = await liveCall(env, simId, 'status', { now });
    if (s.ok === false) return json({ error: s.error }, s.status || 500, cors);
    if (!s.live) return json({ live: false }, 200, headers);
    return json({
      live: true,
      link: s.link,
      players: s.players,
      max_players: s.max_players,
      protocol: s.protocol,
      since: new Date(s.since).toISOString(),
    }, 200, headers);
  }

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  if (userId !== sim.author_id) {
    return json({ error: 'Only the author can host this simulation for now', code: 'not_author' }, 403, cors);
  }

  if (request.method === 'DELETE') {
    const r = await liveCall(env, simId, 'stop', { now });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    return json({ ok: true }, 200, cors);
  }

  if (request.method === 'POST') {
    const body = await request.json().catch(() => ({}));
    const link = checkJoinLink(body.link);
    if (!link.ok) return json({ error: link.error, code: 'bad_link' }, 400, cors);
    const players = count(body.players, 0, MAX_PLAYERS);
    const maxPlayers = count(body.max_players, 1, MAX_PLAYERS);
    const protocol = count(body.protocol, 0, 999_999);
    if (players === null || maxPlayers === null || protocol === null) {
      return json({ error: 'players, max_players and protocol must be whole numbers', code: 'bad_counts' }, 400, cors);
    }
    const r = await liveCall(env, simId, 'beat', {
      host_id: userId, link: body.link, players, max_players: maxPlayers, protocol, now,
    });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    return json({ ok: true, expires_at: r.expires_at }, 200, cors);
  }

  return json({ error: 'Method not allowed' }, 405, cors);
}
