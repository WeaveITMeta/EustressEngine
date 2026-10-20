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

import { inspectIdentityTicket } from './identity.mjs';
import { leaseIsLive } from './guest.mjs';
import { simForServing, simWithLegalLock } from './moderation.mjs';
import { cleanRegion, publicRegion, regionOf, tellLiveIndex } from './live_index.mjs';
import { adultRefusal, isAdultListing, mayView, refuseAdult, viewerFor } from './adult_content.mjs';

/// A host with no heartbeat for this long is Offline. Three missed 30 s beats.
export const HEARTBEAT_TTL_MS = 90_000;

const MAX_LINK_LENGTH = 512;
export const MAX_PLAYERS = 10_000;

/// Most ticket ids one hosting session remembers. Tickets last 10 minutes, so
/// this is far above any real join rate; a full table refuses (`busy`) and
/// never forgets an id early, since forgetting would let a ticket be used twice.
export const MAX_ADMITTED = 5000;

/// The pin (64 hex) in a validated join link.
const pinOf = (link) => new URLSearchParams(String(link).split('?')[1] || '').get('pin');

// -----------------------------------------------------------------------------
// Server credentials
// -----------------------------------------------------------------------------
//
// A dedicated server on the author's computer is not a signed-in Studio
// session. The author mints one credential for a listing (POST
// /api/simulations/{id}/server-credential). The server sends it as
// `Authorization: Bearer esrv_…` on the heartbeat, the stop, and admission.
// The token is 32 random bytes. Only its SHA-256 is stored, under
// `srvcred:<hash>`, and `srvcred-sim:<sim>` names the hash that is current.
// Minting again replaces the previous one. The credential is tied to the
// listing's author: it can mark that listing live and nothing else, and it
// stops working if the listing's author_id is no longer the one it was minted
// for.

const SERVER_CREDENTIAL = /^esrv_[0-9a-f]{64}$/;
const credentialKey = (hash) => `srvcred:${hash}`;
const credentialSimKey = (simId) => `srvcred-sim:${simId}`;

/// The bearer token, or `''`.
export function bearerOf(request) {
  const header = request?.headers?.get?.('Authorization') || '';
  return header.startsWith('Bearer ') ? header.slice(7).trim() : '';
}

/// SHA-256 of `text`, lowercase hex.
export async function sha256HexText(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// The author id a server credential stands for on this listing, or null.
/// A credential minted for another listing, another author, or one that has
/// been replaced, is null.
export async function authorFromServerCredential(env, token, simId, authorId) {
  if (typeof token !== 'string' || !SERVER_CREDENTIAL.test(token) || !env?.SOCIAL) return null;
  const hash = await sha256HexText(token);
  const raw = await env.SOCIAL.get(credentialKey(hash));
  if (!raw) return null;
  let record;
  try {
    record = JSON.parse(raw);
  } catch {
    return null;
  }
  if (record?.sim_id !== simId || record?.author_id !== authorId) return null;
  if ((await env.SOCIAL.get(credentialSimKey(simId))) !== hash) return null;
  return record.author_id;
}

/// Mint or revoke the listing's server credential. The author's session only:
/// a server credential cannot mint another one.
export async function handleServerCredential(request, env, cors, simId, deps) {
  const { verifyAuth, json } = deps;
  if (!env.SOCIAL) {
    return json({ error: 'Live hosting is not configured on this deployment', code: 'live_unavailable' }, 503, cors);
  }
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(raw);
  const presented = bearerOf(request);
  if (presented.startsWith('esrv_')) {
    return json({ error: 'Sign in as the author to mint a server credential', code: 'author_session_required' }, 403, cors);
  }
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  if (userId !== sim.author_id) {
    return json({ error: 'Only the author can create a server credential', code: 'not_author' }, 403, cors);
  }
  if (request.method === 'DELETE') {
    const hash = await env.SOCIAL.get(credentialSimKey(simId));
    if (hash) await env.SOCIAL.delete(credentialKey(hash));
    await env.SOCIAL.delete(credentialSimKey(simId));
    return json({ ok: true }, 200, cors);
  }
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  const minted = `esrv_${[...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')}`;
  const hash = await sha256HexText(minted);
  const previous = await env.SOCIAL.get(credentialSimKey(simId));
  if (previous && previous !== hash) await env.SOCIAL.delete(credentialKey(previous));
  await env.SOCIAL.put(credentialKey(hash), JSON.stringify({
    v: 1,
    sim_id: simId,
    author_id: sim.author_id,
    created_at: new Date().toISOString(),
  }));
  await env.SOCIAL.put(credentialSimKey(simId), hash);
  return json({
    ok: true,
    token: minted,
    sim_id: simId,
    author_id: sim.author_id,
  }, 200, cors);
}

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

/// Forget every admitted ticket id, and how many were admitted: the hosting session they belong to is over.
async function clearAdmitted(storage) {
  const keys = [...(await storage.list({ prefix: 'adm:' })).keys()];
  // delete() takes at most 128 keys at a time.
  for (let i = 0; i < keys.length; i += 128) await storage.delete(keys.slice(i, i + 128));
  await storage.delete(ADMITTED_COUNT);
}

/// How many players the Worker has admitted to this hosting session, in all: every ticket it let through, never fewer as
/// players leave. The host's own heartbeat can claim more players than that, but no more than these plus the host itself.
const ADMITTED_COUNT = 'adm_n';

/// The players a session shows: what the host claims, up to the players the Worker admitted plus the host's own character. The strip,
/// the listing's own live read and the play route all show this, so no page shows a bigger number than another.
export const cappedPlayers = (claimed, admitted) => Math.min(claimed, (admitted || 0) + 1);

/// 0 unknown, 1 low, 2 mid, 3 high. The same steps as
/// `eustress/crates/forge/src/local_launch.rs`.
export function hardwareTier(hardware) {
  const tier = typeof hardware === 'string' ? hardware : hardware?.tier;
  return { unknown: 0, low: 1, mid: 2, high: 3 }[tier] ?? 0;
}

/// A missing latency scores 0. It is not treated as 0 ms.
export function pingScore(latency) {
  if (!Number.isSafeInteger(latency) || latency < 1) return 0;
  return 1 - Math.min(latency, 300) / 300;
}

/// score = 0.5 * fullness + 0.3 * (tier / 3) + 0.2 * ping
export function rankScore(server) {
  const max = Math.max(1, server?.max_players || 0);
  const players = Math.min(server?.players || 0, max);
  const fullness = players / max;
  return 0.5 * fullness + 0.3 * (hardwareTier(server?.hardware) / 3) + 0.2 * pingScore(server?.latency_ms);
}

/// Busiest server that is not full, then the rest. Full servers last.
export function rankServers(servers) {
  return [...(servers || [])].sort((a, b) => {
    if (!!a.full !== !!b.full) return a.full ? 1 : -1;
    const delta = rankScore(b) - rankScore(a);
    if (delta !== 0) return delta;
    return (b.players || 0) - (a.players || 0);
  });
}

const SERVER_ID = /^[A-Za-z0-9_-]{1,64}$/;

/// Players the page shows. The default server stays on the admission cap.
/// Any other id shows its own claim, up to its own max: the admitted count
/// is for the listing, not for that process.
function shownPlayers(server, admitted) {
  if (!server) return 0;
  if (server.id === 'default') return cappedPlayers(server.players, admitted);
  return Math.min(server.players || 0, server.max_players || 0);
}

function publicServer(server, admitted, showLink) {
  const players = shownPlayers(server, admitted);
  const max = server.max_players || 0;
  const row = {
    id: server.id,
    players,
    max_players: max,
    free: Math.max(0, max - players),
    full: max > 0 && players >= max,
    region: server.region_name || null,
    country: server.country?.country || null,
    hardware: server.hardware && typeof server.hardware === 'object'
      ? server.hardware
      : { gpu: '', cpu: '', tier: 'unknown' },
  };
  if (Number.isSafeInteger(server.latency_ms) && server.latency_ms >= 1) row.latency_ms = server.latency_ms;
  if (showLink) {
    row.link = server.link;
    if (typeof server.cert_sha256 === 'string' && /^[0-9a-f]{64}$/.test(server.cert_sha256)) row.cert_sha256 = server.cert_sha256;
  }
  return row;
}

function freshServers(host, now) {
  const list = Array.isArray(host?.servers) ? host.servers : [];
  return list.filter((s) => s && now - s.last_seen <= HEARTBEAT_TTL_MS);
}

function winnerOf(servers, admitted) {
  const ranked = rankServers(servers.map((s) => ({ ...publicServer(s, admitted, false), _raw: s })));
  const winner = ranked.find((s) => !s.full) || ranked[0];
  return { ranked, winner, allFull: ranked.length > 0 && ranked.every((s) => s.full) };
}

const LIVE_OPS = {
  async beat({ host_id, link, players, max_players, protocol, connected, country, cert_sha256, now, server_id, region_name, latency_ms, hardware }) {
    const id = typeof server_id === 'string' && SERVER_ID.test(server_id) ? server_id : 'default';
    const prev = await this.storage.get('host');
    let servers = freshServers(prev, now).map((s) => ({ ...s }));
    if (prev && !Array.isArray(prev.servers)) {
      servers = [{
        id: 'default', link: prev.link, players: prev.players, max_players: prev.max_players, protocol: prev.protocol,
        last_seen: prev.last_seen, since: prev.since, cert_sha256: prev.cert_sha256 || null, region_name: null, latency_ms: null, hardware: null, country: prev.region,
      }];
    }
    const prior = servers.find((s) => s.id === id);
    // `since` is when this process started answering at this link. A restart
    // brings a new certificate, so a new pin and a new link, and a new since.
    const continuing = prior && prior.link === link && now - prior.last_seen <= HEARTBEAT_TTL_MS;
    // A new default session starts with no admitted tickets. Another process
    // on the same listing does not wipe tickets admitted to the default one.
    if (id === 'default' && prev && !continuing) await clearAdmitted(this.storage);
    const entry = {
      id,
      link,
      players,
      max_players,
      protocol,
      last_seen: now,
      since: continuing ? prior.since : now,
      cert_sha256: typeof cert_sha256 === 'string' ? cert_sha256 : null,
      region_name: region_name || null,
      latency_ms: Number.isSafeInteger(latency_ms) && latency_ms >= 1 ? latency_ms : null,
      hardware: hardware || null,
      country: cleanRegion(country),
    };
    servers = servers.filter((s) => s.id !== id);
    servers.push(entry);
    const admitted = (await this.storage.get(ADMITTED_COUNT)) || 0;
    const { winner } = winnerOf(servers, admitted);
    const raw = servers.find((s) => s.id === winner.id) || entry;
    const host = {
      host_id,
      link: raw.link,
      players: raw.players,
      max_players: raw.max_players,
      protocol: raw.protocol,
      // Ticket ids of the players still connected, for the guest leases they hold.
      // Kept from the default server: admission is listing-wide.
      connected: id === 'default' ? (connected || []) : (prev?.connected || []),
      since: raw.since,
      last_seen: Math.max(...servers.map((s) => s.last_seen)),
      // The country the host came in from, as the platform names it: never an address, and never the data centre, which
      // names a city. A host that moves has the new country on its next beat.
      // This object stays { country } only. A region name the host reports is on the server, not here.
      region: cleanRegion(country),
      cert_sha256: raw.cert_sha256 || null,
      servers,
    };
    await this.storage.put('host', host);
    const soonest = Math.min(...servers.map((s) => s.last_seen)) + HEARTBEAT_TTL_MS;
    await this.storage.setAlarm(soonest);
    return {
      ok: true,
      expires_at: new Date(now + HEARTBEAT_TTL_MS).toISOString(),
      since: raw.since,
      region: host.region,
      admitted,
      shown_players: shownPlayers(raw, admitted),
      max_players: raw.max_players,
    };
  },

  async stop({ server_id, now }) {
    if (server_id === undefined || server_id === null || server_id === '') {
      await this.storage.delete('host');
      await this.storage.deleteAlarm();
      await clearAdmitted(this.storage);
      return { ok: true, live: false };
    }
    const host = await this.storage.get('host');
    const nowMs = now || Date.now();
    const servers = freshServers(host, nowMs).filter((s) => s.id !== server_id);
    if (!host || servers.length === 0) {
      await this.storage.delete('host');
      await this.storage.deleteAlarm();
      await clearAdmitted(this.storage);
      return { ok: true, live: false };
    }
    const admitted = (await this.storage.get(ADMITTED_COUNT)) || 0;
    const { winner } = winnerOf(servers, admitted);
    const raw = servers.find((s) => s.id === winner.id);
    host.servers = servers;
    host.link = raw.link;
    host.players = raw.players;
    host.max_players = raw.max_players;
    host.protocol = raw.protocol;
    host.since = raw.since;
    host.cert_sha256 = raw.cert_sha256 || null;
    host.last_seen = Math.max(...servers.map((s) => s.last_seen));
    await this.storage.put('host', host);
    await this.storage.setAlarm(Math.min(...servers.map((s) => s.last_seen)) + HEARTBEAT_TTL_MS);
    return {
      ok: true, live: true, since: raw.since, region: host.region, admitted,
      shown_players: shownPlayers(raw, admitted), max_players: raw.max_players,
    };
  },

  /// The host asks whether one joining player may be admitted. A ticket is
  /// admitted once per hosting session: the Durable Object is single-threaded
  /// and its writes are atomic, which is why this lives here and not in KV.
  async admit({ ticket, audience, now }) {
    const refuse = (status, code, error) => ({ ok: false, status, code, error });
    const host = await this.storage.get('host');
    if (!host || now - host.last_seen > HEARTBEAT_TTL_MS) {
      return refuse(409, 'not_live', 'This simulation is not hosted right now');
    }
    const pin = pinOf(host.link);
    if (audience !== pin) return refuse(403, 'wrong_host', 'The ticket is not for this hosting session');

    const nowS = Math.floor(now / 1000);
    const seen = await inspectIdentityTicket(this.env, ticket, { audience: pin, now: nowS });
    if (!seen.ok) {
      const messages = {
        no_ticket: 'A ticket is required to join',
        invalid: 'This ticket is not valid',
        expired: 'This ticket has expired. Join again from the gallery.',
        wrong_host: 'The ticket is not for this hosting session',
      };
      return refuse(403, seen.code, messages[seen.code]);
    }
    // A ticket made before ids existed cannot be tracked, so it cannot be used once.
    if (!seen.id) return refuse(403, 'invalid', 'This ticket is out of date. Join again from the gallery.');

    // Drop ids whose tickets have expired, then count what is left.
    let count = 0;
    for (const [key, expires] of await this.storage.list({ prefix: 'adm:', limit: MAX_ADMITTED + 1 })) {
      if (expires < nowS) await this.storage.delete(key);
      else count++;
    }
    if ((await this.storage.get(`adm:${seen.id}`)) !== undefined) {
      return refuse(403, 'used', 'This ticket was already used. Join again from the gallery for a new one.');
    }
    if (count >= MAX_ADMITTED) return refuse(403, 'busy', 'The gallery could not confirm this join right now');
    await this.storage.put(`adm:${seen.id}`, seen.expires);
    await this.storage.put(ADMITTED_COUNT, ((await this.storage.get(ADMITTED_COUNT)) || 0) + 1);
    return { ok: true, id: seen.id, account_id: seen.accountId, username: seen.username, guest: false, expires: seen.expires };
  },

  async status({ now }) {
    const host = await this.storage.get('host');
    // The top-level link is what Play hands out. A record whose link is not
    // the join grammar is not live, even if a nested server row still holds
    // an older valid link: that row is not handed out either.
    if (!host || !checkJoinLink(host.link).ok) return { ok: true, live: false };
    let servers = freshServers(host, now).filter((s) => checkJoinLink(s.link).ok);
    if (servers.length === 0) {
      // A host written before per-server rows. The top-level fields are that one server.
      if (now - host.last_seen <= HEARTBEAT_TTL_MS) {
        servers = [{
          id: 'default',
          link: host.link,
          players: host.players,
          max_players: host.max_players,
          protocol: host.protocol,
          last_seen: host.last_seen,
          since: host.since,
          cert_sha256: host.cert_sha256 || null,
          region_name: null,
          latency_ms: null,
          hardware: null,
          country: host.region,
        }];
      } else {
        return { ok: true, live: false };
      }
    }
    const admitted = (await this.storage.get(ADMITTED_COUNT)) || 0;
    const { winner, allFull } = winnerOf(servers, admitted);
    const raw = servers.find((s) => s.id === winner.id);
    return {
      ok: true,
      live: true,
      link: raw.link,
      players: raw.players,
      shown_players: shownPlayers(raw, admitted),
      // How many the Worker admitted to this hosting session: what the players a reader shows are capped at (cappedPlayers).
      admitted,
      max_players: raw.max_players,
      protocol: raw.protocol,
      since: raw.since,
      last_seen: raw.last_seen,
      region: publicRegion(host.region),
      cert_sha256: raw.cert_sha256 || null,
      servers,
      all_full: allFull,
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
    const now = Date.now();
    const servers = freshServers(host, now);
    if (servers.length === 0) {
      await this.storage.delete('host');
      await clearAdmitted(this.storage);
      return;
    }
    if (!Array.isArray(host.servers) || servers.length !== host.servers.length) {
      const admitted = (await this.storage.get(ADMITTED_COUNT)) || 0;
      const { winner } = winnerOf(servers, admitted);
      const raw = servers.find((s) => s.id === winner.id);
      host.servers = servers;
      host.link = raw.link;
      host.players = raw.players;
      host.max_players = raw.max_players;
      host.since = raw.since;
      host.cert_sha256 = raw.cert_sha256 || null;
      host.last_seen = Math.max(...servers.map((s) => s.last_seen));
      await this.storage.put('host', host);
    }
    const due = Math.min(...servers.map((s) => s.last_seen)) + HEARTBEAT_TTL_MS;
    if (now >= due) {
      await this.storage.delete('host');
      await clearAdmitted(this.storage);
    } else {
      await this.storage.setAlarm(due);
    }
  }
}

/// What Play on a listing needs to know about its host: whether it is live now, who is in it, and the join
/// link, which only a visitor who may join receives (signed in, or holding a live guest lease; the same rule as
/// GET /live). `null` when nobody hosts it, when this deployment has no live registry, and when what the host
/// stored is not the exact join grammar: nothing is handed to the Player that checkJoinLink would refuse.
/// The caller has already decided that the visitor may play this listing at all (approved, or its author).
export async function livePlayInfo(env, simId, { signedIn, lease, now }) {
  if (!env.LIVE_HOSTS) return null;
  const s = await liveCall(env, simId, 'status', { now });
  if (s.ok === false || !s.live) return null;
  if (!s.all_full && !checkJoinLink(s.link).ok) return null;
  const players = Number.isSafeInteger(s.shown_players) ? s.shown_players : cappedPlayers(s.players, s.admitted);
  if (s.all_full) return { players, max_players: s.max_players, link: null, all_full: true };
  return { players, max_players: s.max_players, link: signedIn || lease ? s.link : null, all_full: false };
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
  const sim = await simWithLegalLock(env, JSON.parse(raw));
  const now = Date.now();

  // /live/admit takes POST only, from the author; it is not a second way to read /live.
  const admitting = new URL(request.url).pathname.endsWith('/live/admit');
  if (admitting && request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);

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
    } else {
      // Adult content: whether it is live, and where to join, go to a viewer who turned it on and to its author
      // (src/adult_content.mjs). A guest lease never reaches it: a guest is signed out, so off.
      const refused = await refuseAdult(request, env, sim, { authenticate: verifyAuth, json, cors });
      if (refused) return refused;
    }
    const s = await liveCall(env, simId, 'status', { now });
    if (s.ok === false) return json({ error: s.error }, s.status || 500, cors);
    if (!s.live) return json({ live: false }, 200, headers);
    const shown = Number.isSafeInteger(s.shown_players) ? s.shown_players : cappedPlayers(s.players, s.admitted);
    const body = {
      live: true,
      players: shown,
      max_players: s.max_players,
      protocol: s.protocol,
      since: new Date(s.since).toISOString(),
      region: s.region ?? null,
    };
    // The join link goes only to someone who may join: a signed-in account, or
    // a guest holding a live lease (the X-Eustress-Lease header). Anyone else
    // sees that the simulation is live and how many are playing, but not where.
    // The host still demands a ticket at Hello (POST /live/admit), so the link
    // is no longer a capability a pasted copy could use.
    const signedIn = await verifyAuth(request, env);
    const lease = signedIn ? false : await leaseIsLive(env, request.headers.get('X-Eustress-Lease'), now);
    const showLink = !!(signedIn || lease);
    const servers = rankServers((Array.isArray(s.servers) ? s.servers : []).map((server) => publicServer(server, s.admitted, showLink)));
    if (servers.length) body.servers = servers;
    const allFull = servers.length > 0 && servers.every((server) => server.full);
    if (allFull) {
      // No join link, and not link_hidden: the page must not ask the visitor to sign in.
      body.all_full = true;
    } else if (showLink) {
      body.link = s.link;
      // The pin, also on its own, so a browser can pass it to
      // serverCertificateHashes without parsing the link.
      if (typeof s.cert_sha256 === 'string' && /^[0-9a-f]{64}$/.test(s.cert_sha256)) body.cert_sha256 = s.cert_sha256;
    } else body.link_hidden = true;
    return json(body, 200, headers);
  }

  // A dedicated server presents `esrv_…` instead of the author's session.
  // Anything else is still the author's own sign-in. Checked before
  // verifyAuth so a credential is never treated as a user id.
  const presented = bearerOf(request);
  let userId;
  if (presented.startsWith('esrv_')) {
    userId = await authorFromServerCredential(env, presented, simId, sim.author_id);
    if (!userId) {
      return json({ error: 'This server credential is not valid for this listing', code: 'bad_server_credential' }, 401, cors);
    }
  } else {
    userId = await verifyAuth(request, env);
    if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
    if (userId !== sim.author_id) {
      return json({ error: 'Only the author can host this simulation for now', code: 'not_author' }, 403, cors);
    }
  }

  // The host asks whether a joining player may be admitted.
  if (admitting) {
    const body = await request.json().catch(() => ({}));
    if (typeof body.audience !== 'string' || !/^[0-9a-f]{64}$/.test(body.audience)) {
      return json({ error: 'audience must be the host certificate pin (64 hex characters)', code: 'bad_audience' }, 400, cors);
    }
    // A session whose listing does not serve (unapproved, held, quarantined, or bound to other bytes than were reviewed) admits its
    // author only. "Serves" is the test the download uses (simForServing), so admission and download cannot disagree, and the
    // author's privacy setting does not enter it: an approved private listing is joined by link, as its download is. Any
    // signed-in account can mint a ticket for a pin it holds a link to, so the ticket's own account decides here, before the
    // Durable Object spends the ticket. Every other joiner gets the same answer whichever reason applies (Security's rulings,
    // 2026-10-07). An invite list the author sets is the later extension; no admin joins a session that does not serve.
    const served = await simForServing(env, { id: simId, ...sim }, 'content');
    if (served?.moderation?.status !== 'approved') {
      const who = await inspectIdentityTicket(env, body.ticket, { audience: body.audience, now: Math.floor(now / 1000) });
      if (!who.ok || who.accountId !== sim.author_id) {
        return json({ error: 'This session is not available right now.', code: 'not_available' }, 403, { ...cors, 'Cache-Control': 'no-store' });
      }
    }
    // Adult content admits a joiner the gate lets see it (turned it on; for adult_21, a verified age of 21 or over too), and
    // its author (src/adult_content.mjs mayView). The ticket's own account decides, here, before the Durable Object spends
    // the ticket: a joiner who turns the setting on, or turns 21, can use the same ticket. A ticket that is not genuine goes
    // on to the object, which refuses it in its own words.
    if (isAdultListing(sim)) {
      const who = await inspectIdentityTicket(env, body.ticket, { audience: body.audience, now: Math.floor(now / 1000) });
      if (who.ok) {
        const joiner = await viewerFor(env, who.accountId, now);
        if (!mayView(sim, joiner)) return adultRefusal(json, cors, joiner);
      }
    }
    const r = await liveCall(env, simId, 'admit', { ticket: body.ticket, audience: body.audience, now });
    if (r.ok === false) {
      return json({ error: r.error, code: r.code || 'admit_failed' }, r.status || 500, { ...cors, 'Cache-Control': 'no-store' });
    }
    // The player was admitted: a visit by them, counted once a day (src/visits.mjs). Never fails the admission.
    if (deps.onJoin) await Promise.resolve(deps.onJoin(sim, { id: r.id, accountId: r.account_id, guest: r.guest })).catch((e) => console.error('onJoin failed:', e?.message || e));
    return json({ ok: true, id: r.id, account_id: r.account_id, username: r.username, guest: r.guest, expires: r.expires }, 200, { ...cors, 'Cache-Control': 'no-store' });
  }

  if (request.method === 'DELETE') {
    const serverId = new URL(request.url).searchParams.get('server_id');
    if (serverId !== null && !SERVER_ID.test(serverId)) {
      return json({ error: 'server_id must be 1 to 64 letters, digits, _ or -', code: 'bad_server_id' }, 400, cors);
    }
    const r = await liveCall(env, simId, 'stop', { now, server_id: serverId });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    if (r.live && isListable(sim)) {
      await tellLiveIndex(env, 'beat', {
        sim_id: simId, author: sim.author_id, players: r.shown_players, max_players: r.max_players, region: r.region ?? null, since: r.since, now,
      });
    } else if (!r.live) {
      // The live index drops the session at once. An index that fails never fails a stop.
      await tellLiveIndex(env, 'stop', { sim_id: simId });
    }
    return json({ ok: true }, 200, cors);
  }

  if (request.method === 'POST') {
    const body = await request.json().catch(() => ({}));
    const link = checkJoinLink(body.link);
    if (!link.ok) return json({ error: link.error, code: 'bad_link' }, 400, cors);
    // The certificate hash the browser pins. It is the pin already in the
    // link. Sending it again is how a dedicated server publishes it; a
    // mismatch is refused so the stored hash cannot disagree with the link.
    const pin = link.pin.toLowerCase();
    let certSha256 = pin;
    if (body.cert_sha256 !== undefined) {
      const presented = typeof body.cert_sha256 === 'string' ? body.cert_sha256.toLowerCase() : '';
      if (!/^[0-9a-f]{64}$/.test(presented) || presented !== pin) {
        return json({ error: 'cert_sha256 must be the SHA-256 pin in the join link (64 hex characters)', code: 'bad_cert' }, 400, cors);
      }
      certSha256 = presented;
    }
    const players = count(body.players, 0, MAX_PLAYERS);
    const maxPlayers = count(body.max_players, 1, MAX_PLAYERS);
    const protocol = count(body.protocol, 0, 999_999);
    if (players === null || maxPlayers === null || protocol === null) {
      return json({ error: 'players, max_players and protocol must be whole numbers', code: 'bad_counts' }, 400, cors);
    }
    // Optional: the ticket ids of the players still connected.
    let connected = [];
    if (body.admitted !== undefined) {
      const ok = Array.isArray(body.admitted)
        && body.admitted.length <= Math.min(maxPlayers, 1000)
        && body.admitted.every((id) => typeof id === 'string' && /^[0-9a-f]{32}$/.test(id));
      if (!ok) return json({ error: 'admitted must list at most max_players ticket ids, 32 hex characters each', code: 'bad_admitted' }, 400, cors);
      connected = body.admitted;
    }
    let serverId;
    if (body.server_id !== undefined) {
      if (typeof body.server_id !== 'string' || !SERVER_ID.test(body.server_id)) {
        return json({ error: 'server_id must be 1 to 64 letters, digits, _ or -', code: 'bad_server_id' }, 400, cors);
      }
      serverId = body.server_id;
    }
    let regionName = null;
    if (body.region !== undefined && body.region !== null && body.region !== '') {
      if (typeof body.region !== 'string' || !SERVER_ID.test(body.region)) {
        return json({ error: 'region must be a short name', code: 'bad_region' }, 400, cors);
      }
      regionName = body.region;
    }
    let latencyMs = null;
    if (body.latency_ms !== undefined && body.latency_ms !== null && body.latency_ms !== 0) {
      if (!Number.isSafeInteger(body.latency_ms) || body.latency_ms < 1 || body.latency_ms > 60_000) {
        return json({ error: 'latency_ms must be a whole number of milliseconds from 1 to 60000', code: 'bad_latency' }, 400, cors);
      }
      latencyMs = body.latency_ms;
    }
    let hardware = null;
    if (body.hardware !== undefined && body.hardware !== null) {
      const h = body.hardware;
      if (typeof h !== 'object' || Array.isArray(h)) {
        return json({ error: 'hardware must be an object', code: 'bad_hardware' }, 400, cors);
      }
      const clip = (v) => (typeof v === 'string' ? v.slice(0, 160) : '');
      const tier = ['unknown', 'low', 'mid', 'high'].includes(h.tier) ? h.tier : 'unknown';
      hardware = { gpu: clip(h.gpu), cpu: clip(h.cpu), tier };
    }
    const r = await liveCall(env, simId, 'beat', {
      host_id: userId, link: body.link, players, max_players: maxPlayers, protocol, connected, cert_sha256: certSha256, now,
      server_id: serverId, region_name: regionName, latency_ms: latencyMs, hardware, ...regionOf(request),
    });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    // The live index learns of this beat only while the listing is listed: an author's unlisted listings never fill it. It is told no
    // link, key or pin, only what the Gallery shows. The players are the Quick Join server: the busiest one that is not full.
    // On the default server that is still the admission cap. An index that fails never fails a heartbeat.
    if (isListable(sim)) {
      await tellLiveIndex(env, 'beat', {
        sim_id: simId,
        author: sim.author_id,
        players: Number.isSafeInteger(r.shown_players) ? r.shown_players : cappedPlayers(players, r.admitted),
        max_players: r.max_players || maxPlayers,
        region: r.region ?? null,
        since: r.since,
        now,
      });
    }
    return json({ ok: true, expires_at: r.expires_at }, 200, cors);
  }

  return json({ error: 'Method not allowed' }, 405, cors);
}
