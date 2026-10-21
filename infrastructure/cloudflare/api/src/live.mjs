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
// Guest-only sessions (McKale, 2026-10-16): a host whose heartbeat says
// `guest_only: true` runs a session for guests and nobody else, held in its own
// object (`guest:<simulation id>`, guestSessionName) beside the account session,
// and listed in its own index (GET /api/guest/live). A guest is admitted only to
// a guest-only session, and an account only to an account session. Its
// DELETE is `/live?guest_only=true`. GUEST_PLAY.md section 13.
//
// One Durable Object per simulation holds its current host. Each heartbeat
// rewrites it and moves its alarm out to 90 s; if the host stops without
// saying so, the alarm clears it and the listing goes Offline by itself. A
// Durable Object rather than KV because KV's Free plan allows 1,000 writes a
// day, one host's 30 s heartbeat alone is 2,880, and a KV read can trail a
// write by a minute, long enough to hand out a link that no longer answers.
// =============================================================================

import { inspectIdentityTicket } from './identity.mjs';
import {
  GUEST_NAME, RETENTION_MS, barGuest, beatGuestLease, bodyObject, dayOf, dayStart, guestPlayOn, inspectGuestJoin, isGuestJoinTicket,
  isGuestRating, leaseHeld, leaseIsLive, nextMidnight,
} from './guest.mjs';
import { simForServing, simWithLegalLock } from './moderation.mjs';
import { GUEST_LIVE_INDEX_NAME, cleanRegion, publicRegion, regionOf, tellLiveIndex } from './live_index.mjs';
import { adultRefusal, isAdultListing, mayView, refuseAdult, viewerFor } from './adult_content.mjs';

/// A host with no heartbeat for this long is Offline. Three missed 30 s beats.
export const HEARTBEAT_TTL_MS = 90_000;

const MAX_LINK_LENGTH = 512;
export const MAX_PLAYERS = 10_000;

/// Most ticket ids one hosting session remembers. Tickets last 10 minutes, so
/// this is far above any real join rate; a full table refuses (`busy`) and
/// never forgets an id early, since forgetting would let a ticket be used twice.
export const MAX_ADMITTED = 5000;

/// Most guests one hosting session holds at once. Each connected guest's lease is beaten from the host's heartbeat, one
/// call per guest per beat, so this bounds what one heartbeat costs. A full session refuses the next guest; accounts are
/// not counted against it.
export const MAX_GUESTS = 100;

/// A guest admitted, or last named by the host, this recently is kept even when the host's latest list does not name it:
/// the host's beat may have been sent before the admission reached it, or one list may have missed it. The grace runs from
/// the last heartbeat that named the guest, not from the admission.
const GUEST_GRACE_MS = HEARTBEAT_TTL_MS;

/// What a host must declare, in `guest_caps` on its latest heartbeat, before a guest is admitted to it (GUEST_PLAY.md
/// section 13): it delivers no guest's chat line until the Worker's check answers `deliver` (`no_guest_chat`), it names
/// every guest "Guest" whatever the guest's Hello says (`guest_name`), it disconnects every guest the heartbeat answer
/// names in `ended_guests`, and every guest on `end_all_guests` or after 90 s without a 200 heartbeat (`ended_guests`),
/// and it sends its own chat lines to its guests through the same check, delivering only on `deliver`
/// (`host_chat_checked`). A host that does not declare all four, including every host built before this, never takes a
/// guest, and a host that stops declaring them has its guests ended.
export const GUEST_CAPS_REQUIRED = Object.freeze(['no_guest_chat', 'guest_name', 'ended_guests', 'host_chat_checked']);
const MAX_GUEST_CAPS = 16;

/// Guest join tickets waiting to be used, per hosting session and per lease. A ticket names neither the guest nor the
/// lease (guest.mjs signGuestJoin); this object holds both under the ticket id until the ticket is used or expires. A
/// lease's oldest waiting ticket gives way to its newest past the per-lease bound, and a session with this many waiting
/// refuses the next (`busy`).
export const MAX_GUEST_TICKETS_PER_LEASE = 4;
export const MAX_PENDING_GUEST_TICKETS = 2000;

/// The LiveHost object of a listing's guest-only session. The account session's object is named by the listing id alone.
export const guestSessionName = (simId) => `guest:${simId}`;
const sessionName = (simId, guestOnly) => (guestOnly ? guestSessionName(simId) : simId);

/// Whether a host record declares every guest protection.
const hasGuestCaps = (host) => Array.isArray(host?.guest_caps) && GUEST_CAPS_REQUIRED.every((c) => host.guest_caps.includes(c));

/// A connected guest's record once its lease has ended, or once its guest id is past its 25 hours: the guest id and the
/// lease id are dropped, and only the join ticket id (random, for this one session) and the times remain, so the host is
/// still told to let the guest go.
const endedSeat = (g) => ({ ended: true, at: g.at, seen: g.seen });

/// Whether lease `tid`, or guest id `gid`, was removed from this session for chat and may not come back yet.
async function barredHere(storage, { tid, gid, now }) {
  const byLease = typeof tid === 'string' ? await storage.get(`gbar:${tid}`) : undefined;
  const byGuest = typeof gid === 'string' ? await storage.get(`gbarg:${gid}`) : undefined;
  return [byLease, byGuest].some((until) => typeof until === 'number' && now < until);
}

/// Whether a seat still holds identifiers it may no longer keep.
const pastGuestId = (g, now) => !g.ended && (typeof g.gx !== 'number' || now >= g.gx);

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

/// Forget every admitted ticket id, and how many were admitted: the hosting session they belong to is over. The bars a
/// refused chat line set (`gbar:`, `gbarg:`, `gbarq:`) and the host's own chat refusals (`hostref:`, `hostbar`) are not the
/// session's: they last their day whatever the host does, and expireKept drops them when it is over.
async function clearAdmitted(storage) {
  const keys = [
    ...(await storage.list({ prefix: 'adm:' })).keys(),
    ...(await storage.list({ prefix: 'gadm:' })).keys(),
    ...(await storage.list({ prefix: 'gtk:' })).keys(),
  ];
  // delete() takes at most 128 keys at a time.
  for (let i = 0; i < keys.length; i += 128) await storage.delete(keys.slice(i, i + 128));
  await storage.delete(ADMITTED_COUNT);
}

/// When a kept record may no longer be kept, in milliseconds: a lease bar `gbar:<tid>` and a guest-id bar `gbarg:<gid>`
/// hold that time; a bar still to be written `gbarq:<tid>` holds `{gid, tid, gx}`; the host's refusal count
/// `hostref:<day>` and the day's guest chat caps `caps:<day>` last until 25 hours after their day began; `hostbar` holds
/// the end of the day it was set.
function keptUntil(key, value) {
  if (key.startsWith('gbar:') || key.startsWith('gbarg:') || key === 'hostbar') return typeof value === 'number' ? value : 0;
  if (key.startsWith('gbarq:')) return typeof value?.gx === 'number' ? value.gx : 0;
  if (key.startsWith('hostref:') || key.startsWith('caps:')) {
    const start = Date.parse(`${key.slice(key.indexOf(':') + 1)}T00:00:00.000Z`);
    return Number.isFinite(start) ? start + RETENTION_MS : 0;
  }
  return null;
}
const KEPT_PREFIXES = ['gbar:', 'gbarg:', 'gbarq:', 'hostref:', 'caps:', 'hostbar'];

/// Drop every kept record whose time is over, and answer when the next one is due (null when none is left), so an
/// object whose host is gone still forgets them on time (alarm).
async function expireKept(storage, now) {
  let next = null;
  const gone = [];
  for (const prefix of KEPT_PREFIXES) {
    for (const [key, value] of await storage.list({ prefix })) {
      const until = keptUntil(key, value);
      if (until === null) continue;
      if (until <= now) gone.push(key);
      else if (next === null || until < next) next = until;
    }
  }
  for (let i = 0; i < gone.length; i += 128) await storage.delete(gone.slice(i, i + 128));
  return next;
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
  async beat({ host_id, link, players, max_players, protocol, connected, guest_caps, guest_only, country, cert_sha256, now, server_id, region_name, latency_ms, hardware }) {
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
      // The guest protections this host declares on this beat (GUEST_CAPS_REQUIRED). Each beat replaces the last.
      guest_caps: Array.isArray(guest_caps) ? guest_caps : [],
      // A guest-only session: guests and nobody else. Fixed by the object it lives in (guestSessionName).
      guest_only: guest_only === true,
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
    await this.storage.setAlarm(Math.min(...servers.map((s) => s.last_seen)) + HEARTBEAT_TTL_MS);
    // The guests this session admitted. One the host names as connected has its lease beaten (by the route, which
    // reaches the guest allowances) and is marked seen now; one it no longer names has left, and is forgotten once past
    // the grace from the last beat that named it. Its lease is then beaten by nobody and ends by its own timeout. A seat
    // whose guest id is past its 25 hours keeps no identifiers and is reported ended.
    const named = new Set(host.connected);
    const guests = [];
    const writes = {};
    for (const [key, g] of await this.storage.list({ prefix: 'gadm:' })) {
      const id = key.slice('gadm:'.length);
      if (named.has(id)) {
        const seat = pastGuestId(g, now) ? { ...endedSeat(g), seen: now } : { ...g, seen: now };
        writes[key] = seat;
        guests.push(seat.ended ? { id, ended: true } : { id, gid: seat.gid, tid: seat.tid, ended: false });
      } else if (now - Math.max(g.at || 0, g.seen || 0) > GUEST_GRACE_MS) {
        await this.storage.delete(key);
      } else if (pastGuestId(g, now)) {
        writes[key] = endedSeat(g);
      }
    }
    const entries = Object.entries(writes);
    // put() takes at most 128 keys at a time.
    for (let i = 0; i < entries.length; i += 128) await this.storage.put(Object.fromEntries(entries.slice(i, i + 128)));
    // A guest-only host names only guests: an id it names that holds no seat here (the seats were forgotten when the
    // heartbeat lapsed past 90 s, or by the alarm, while the guest stayed connected to the host) is reported ended, so it is
    // never kept unbeaten and unchecked.
    if (host.guest_only) {
      const reported = new Set(guests.map((g) => g.id));
      for (const id of named) if (!reported.has(id)) guests.push({ id, ended: true });
    }
    // Waiting guest tickets that expired are dropped, with the guest and lease ids they held, and so are bars, refusal
    // counts and cap counts whose time is over.
    const nowS = Math.floor(now / 1000);
    const expired = [...(await this.storage.list({ prefix: 'gtk:' }))].filter(([, t]) => !(t?.e >= nowS)).map(([key]) => key);
    for (let i = 0; i < expired.length; i += 128) await this.storage.delete(expired.slice(i, i + 128));
    await expireKept(this.storage, now);
    // Chat bars that could not be written to the guest's allowance yet, for the route to try again.
    const barRetry = [...(await this.storage.list({ prefix: 'gbarq:' })).values()].map((q) => ({ gid: q.gid, tid: q.tid, gx: q.gx }));
    const hostBar = await this.storage.get('hostbar');
    return {
      ok: true, expires_at: new Date(now + HEARTBEAT_TTL_MS).toISOString(), since: host.since, region: host.region,
      admitted, guests, guest_caps_ok: hasGuestCaps(host),
      // The Quick Join server's players and seats (several servers can host one listing).
      shown_players: shownPlayers(raw, admitted), max_players: raw.max_players,
      host_barred: typeof hostBar === 'number' && now < hostBar, bar_retry: barRetry,
    };
  },

  /// A chat bar that could not be written to the guest's allowance (guest.mjs barGuest): kept here until it is, or until
  /// the guest id's 25 hours are over, and retried from each heartbeat. Meanwhile the session refuses that guest id
  /// (`gbarg:`, set by guest_kick).
  async bar_pending({ tid, gid, gx }) {
    if (![tid, gid].every((v) => typeof v === 'string' && /^[0-9a-f]{32}$/.test(v)) || typeof gx !== 'number') return { ok: false, status: 400, code: 'bad_request' };
    await this.storage.put(`gbarq:${tid}`, { gid, tid, gx });
    return { ok: true };
  },

  /// The bar was written: forget the guest and lease ids it held.
  async bar_done({ tid }) {
    if (typeof tid === 'string') await this.storage.delete(`gbarq:${tid}`);
    return { ok: true };
  },

  /// For the host's own chat check: whether this guest-only session is live, and whether the host's chat was refused
  /// HOST_CHAT_REFUSALS_MAX times today (then it takes no guest until the day is over).
  async host_state({ now }) {
    const host = await this.storage.get('host');
    const bar = await this.storage.get('hostbar');
    return { ok: true, live: !!host && host.guest_only === true && now - host.last_seen <= HEARTBEAT_TTL_MS, barred: typeof bar === 'number' && now < bar };
  },

  /// One of the host's own chat lines was refused. At `max` refusals in one UTC day the session ends every guest (the
  /// next heartbeat answers `end_all_guests`) and takes none until the day is over. Counts only: no text.
  async host_chat_refused({ now, max }) {
    const key = `hostref:${dayOf(now)}`;
    const n = ((await this.storage.get(key)) || 0) + 1;
    await this.storage.put(key, n);
    const barred = Number.isSafeInteger(max) && n >= max;
    if (barred) await this.storage.put('hostbar', nextMidnight(now));
    return { ok: true, refusals: n, barred };
  },

  /// One guest chat check that reaches Jev, counted against the day's caps (guest.mjs guestChatCaps) for the deployment,
  /// the host account and the listing. Only the object named GUEST_CHAT_CAPS_NAME is asked. Counts only, under the day,
  /// deleted 25 hours after the day began (expireKept, from this object's alarm).
  async chat_spend({ now, sim, host, caps }) {
    const whole = (v) => Number.isSafeInteger(v) && v >= 0;
    if (typeof sim !== 'string' || typeof host !== 'string' || !caps || !whole(caps.listing) || !whole(caps.host) || !whole(caps.total)) {
      return { ok: false, status: 400, code: 'bad_request' };
    }
    const key = `caps:${dayOf(now)}`;
    const c = (await this.storage.get(key)) || { total: 0, hosts: {}, sims: {} };
    const over = c.total >= caps.total ? 'total' : (c.hosts[host] || 0) >= caps.host ? 'host' : (c.sims[sim] || 0) >= caps.listing ? 'listing' : null;
    if (!over) {
      c.total += 1;
      c.hosts[host] = (c.hosts[host] || 0) + 1;
      c.sims[sim] = (c.sims[sim] || 0) + 1;
      await this.storage.put(key, c);
    }
    // The alarm is the earliest count still kept, so yesterday's goes at its own time.
    const due = await expireKept(this.storage, now);
    if (due !== null && !(await this.storage.get('host'))) await this.storage.setAlarm(due);
    return over ? { ok: true, allowed: false, over } : { ok: true, allowed: true };
  },

  /// These guests' leases have ended, or they may no longer stay: remember it, so they are reported without asking
  /// again, and drop the guest and lease ids they held.
  async guests_ended({ ids }) {
    for (const id of Array.isArray(ids) ? ids : []) {
      const g = await this.storage.get(`gadm:${id}`);
      if (g && !g.ended) await this.storage.put(`gadm:${id}`, endedSeat(g));
    }
    return { ok: true };
  },

  /// Hold the guest and lease a guest join ticket was minted from, under the ticket's id, for the session hosting at
  /// `audience` now. Nothing is held for a session that is not live at that pin.
  async hold_guest_ticket({ id, gid, tid, gx, expires, audience, now }) {
    const refuse = (status, code, error) => ({ ok: false, status, code, error });
    if (![id, gid, tid].every((v) => typeof v === 'string' && /^[0-9a-f]{32}$/.test(v)) || !Number.isSafeInteger(expires) || typeof gx !== 'number') {
      return refuse(400, 'bad_request', 'bad guest ticket');
    }
    const host = await this.storage.get('host');
    if (!host || now - host.last_seen > HEARTBEAT_TTL_MS || String(pinOf(host.link)).toLowerCase() !== audience) {
      return refuse(409, 'not_live', 'This simulation is not hosted at that session right now');
    }
    // A guest removed for chat (by its lease, or by its guest id when its allowance bar is not written yet), and a session
    // whose host's own chat was refused too often today, get no ticket.
    if (await barredHere(this.storage, { tid, gid, now })) return refuse(403, 'guest_barred', 'This guest session cannot join here again today.');
    const hostBar = await this.storage.get('hostbar');
    if (typeof hostBar === 'number' && now < hostBar) return refuse(403, 'host_chat_barred', 'This session takes no guests for the rest of the day.');
    // Never past the guest id's 25 hours: the waiting ticket is dropped by then, with the ids it holds.
    if (now >= gx) return refuse(410, 'lease_ended', 'The guest session has ended');
    const nowS = Math.floor(now / 1000);
    expires = Math.min(expires, Math.floor(gx / 1000));
    let count = 0;
    const mine = [];
    for (const [key, t] of await this.storage.list({ prefix: 'gtk:', limit: MAX_PENDING_GUEST_TICKETS + 1 })) {
      if (!(t?.e >= nowS)) await this.storage.delete(key);
      else {
        count++;
        if (t.tid === tid) mine.push([key, t.e]);
      }
    }
    mine.sort((a, b) => a[1] - b[1]);
    while (mine.length >= MAX_GUEST_TICKETS_PER_LEASE) {
      await this.storage.delete(mine.shift()[0]);
      count--;
    }
    if (count >= MAX_PENDING_GUEST_TICKETS) return refuse(503, 'busy', 'The gallery could not make a ticket right now');
    await this.storage.put(`gtk:${id}`, { gid, tid, gx, e: expires });
    return { ok: true };
  },

  /// The guest and lease a waiting guest join ticket was minted from, for the route to check the lease before the ticket is
  /// spent. Not found: the ticket was used, expired, or never held here.
  async guest_pending({ id }) {
    const t = typeof id === 'string' ? await this.storage.get(`gtk:${id}`) : undefined;
    return t ? { ok: true, gid: t.gid, tid: t.tid } : { ok: false, status: 404, code: 'invalid' };
  },

  /// Whether guest `id` holds a seat in this session now (admitted, not ended), for the chat check.
  async guest_seat({ id }) {
    const g = typeof id === 'string' ? await this.storage.get(`gadm:${id}`) : undefined;
    return g && !g.ended ? { ok: true } : { ok: false, status: 404, code: 'not_connected' };
  },

  /// A chat line from guest `id` was refused: the seat ends now (reported in ended_guests from the next heartbeat on), and
  /// its lease and its guest id are barred from this session until the guest id's 25 hours are over (the guest id bar
  /// keeps a new lease from the same address out even when the allowance bar cannot be written). Answers the guest and
  /// lease ids and `gx`, for the route to bar the guest in its allowance too.
  async guest_kick({ id, now }) {
    const g = typeof id === 'string' ? await this.storage.get(`gadm:${id}`) : undefined;
    if (!g || g.ended) return { ok: false, status: 404, code: 'not_connected' };
    const until = typeof g.gx === 'number' ? g.gx : now;
    await this.storage.put({ [`gadm:${id}`]: endedSeat(g), [`gbar:${g.tid}`]: until, ...(typeof g.gid === 'string' ? { [`gbarg:${g.gid}`]: until } : {}) });
    return { ok: true, gid: g.gid, tid: g.tid, gx: g.gx };
  },

  /// How many guest chat lines were checked, by outcome. Counts only: no text, no guest.
  async chat_counted({ outcome }) {
    const key = `chat_n:${['deliver', 'block', 'unavailable', 'empty', 'capped'].includes(outcome) ? outcome : 'other'}`;
    await this.storage.put(key, ((await this.storage.get(key)) || 0) + 1);
    return { ok: true };
  },

  /// Stop one server (`server_id`), or the whole session when none is named. The session ends when no server is left.
  async stop({ server_id, now } = {}) {
    const nowMs = typeof now === 'number' ? now : Date.now();
    const host = await this.storage.get('host');
    const named = !(server_id === undefined || server_id === null || server_id === '');
    const servers = named ? freshServers(host, nowMs).filter((s) => s.id !== server_id) : [];
    if (!host || servers.length === 0) {
      await this.storage.delete('host');
      await this.storage.deleteAlarm();
      await clearAdmitted(this.storage);
      // What outlives the session (bars, refusal counts) is still forgotten on time.
      const due = await expireKept(this.storage, nowMs);
      if (due !== null) await this.storage.setAlarm(due);
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
    // A guest join ticket (guest.mjs) or an account's identity ticket. The route has already checked what only it can reach
    // (the listing's rating and the guest's lease); this object checks the ticket itself and spends it once.
    const guest = isGuestJoinTicket(ticket);
    const seen = guest
      ? await inspectGuestJoin(this.env, ticket, { audience: pin, now })
      : await inspectIdentityTicket(this.env, ticket, { audience: pin, now: nowS });
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
    // A guest joins a guest-only session and nothing else; an account joins anything but a guest-only session.
    if (guest && !host.guest_only) return refuse(403, 'account_session', 'This session takes signed-in players only. Sign in to join, or play in a guest session.');
    if (!guest && host.guest_only) return refuse(403, 'guest_only_session', 'This session is for guests only.');
    let pending = null;
    if (guest) {
      // Hosted guest play is on for this deployment, and this host's latest heartbeat declares every guest protection.
      if (!guestPlayOn(this.env)) return refuse(403, 'guest_join_unavailable', 'Joining as a guest is not available yet. Sign in to join.');
      if (!hasGuestCaps(host)) return refuse(403, 'guest_not_supported', 'This session cannot take guests. Sign in to join.');
      // Which guest and which lease the ticket was minted from: held here at mint, never read from the ticket.
      pending = await this.storage.get(`gtk:${seen.id}`);
      if (!pending) return refuse(403, 'invalid', 'This ticket is not valid');
      if (await barredHere(this.storage, { tid: pending.tid, gid: pending.gid, now })) return refuse(403, 'guest_barred', 'This guest session cannot join here again today.');
      const hostBar = await this.storage.get('hostbar');
      if (typeof hostBar === 'number' && now < hostBar) return refuse(403, 'host_chat_barred', 'This session takes no guests for the rest of the day.');
      // One seat per lease: a seat the host still names, or one inside the grace, keeps the lease's other tickets out. An
      // older seat of the same lease that the host has let go is closed for good.
      const seats = await this.storage.list({ prefix: 'gadm:' });
      const named = new Set(host.connected || []);
      const closed = {};
      for (const [key, g] of seats) {
        if (g.ended || g.tid !== pending.tid) continue;
        if (named.has(key.slice('gadm:'.length)) || now - Math.max(g.at || 0, g.seen || 0) <= GUEST_GRACE_MS) {
          return refuse(403, 'lease_in_use', 'This guest session is already playing here.');
        }
        closed[key] = endedSeat(g);
      }
      if (seats.size >= MAX_GUESTS) return refuse(403, 'not_available', 'This session is not available right now.');
      if (Object.keys(closed).length) await this.storage.put(closed);
    }
    await this.storage.put(`adm:${seen.id}`, seen.expires);
    await this.storage.put(ADMITTED_COUNT, ((await this.storage.get(ADMITTED_COUNT)) || 0) + 1);
    if (guest) {
      // Which lease to beat while this guest stays connected, and until when its guest id may be kept. The guest id is a
      // day-keyed hash, never an address, and never leaves this Worker.
      await this.storage.put(`gadm:${seen.id}`, { gid: pending.gid, tid: pending.tid, gx: pending.gx, at: now, seen: now });
      await this.storage.delete(`gtk:${seen.id}`);
      // `visit`: what the route counts this join's visit by (the guest id, once a day, through visits.mjs guestVisitKey),
      // never handed to the host.
      return { ok: true, id: seen.id, account_id: null, username: GUEST_NAME, guest: true, expires: seen.expires, visit: pending.gid };
    }
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
  /// Once the host is gone, what outlives the session (chat bars, refusal counts, the day's guest chat cap counts) is
  /// dropped when its time is over, and the alarm is set again for the next one.
  async alarm() {
    const now = Date.now();
    const host = await this.storage.get('host');
    if (host) {
      const servers = freshServers(host, now);
      // A server that stopped beating leaves the session; the Quick Join server is chosen again from those left.
      if (servers.length && (!Array.isArray(host.servers) || servers.length !== host.servers.length)) {
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
      // A record from before per-server rows is one server, due by its own last beat.
      const due = servers.length ? Math.min(...servers.map((s) => s.last_seen)) + HEARTBEAT_TTL_MS : host.last_seen + HEARTBEAT_TTL_MS;
      if (now < due) {
        await this.storage.setAlarm(due);
        return;
      }
      await this.storage.delete('host');
      await clearAdmitted(this.storage);
    }
    const next = await expireKept(this.storage, now);
    if (next !== null) await this.storage.setAlarm(next);
  }
}

/// What Play on a listing needs to know about its host: whether it is live now, who is in it, and the join
/// link, which only a visitor who may join receives (signed in, or holding a live guest lease; the same rule as
/// GET /live). `null` when nobody hosts it, when this deployment has no live registry, and when what the host
/// stored is not the exact join grammar: nothing is handed to the Player that checkJoinLink would refuse.
/// The caller has already decided that the visitor may play this listing at all (approved, or its author).
export async function livePlayInfo(env, simId, { signedIn, lease, now }) {
  if (!env.LIVE_HOSTS) return null;
  // A guest (signed out, with a lease, while hosted guest play is on) is shown the listing's guest-only session; everyone
  // else its account session, whose link goes to a signed-in account only.
  const guest = !signedIn && !!lease && guestPlayOn(env);
  const s = await liveCall(env, sessionName(simId, guest), 'status', { now });
  if (s.ok === false || !s.live) return null;
  if (!s.all_full && !checkJoinLink(s.link).ok) return null;
  const players = Number.isSafeInteger(s.shown_players) ? s.shown_players : cappedPlayers(s.players, s.admitted);
  if (s.all_full) return { players, max_players: s.max_players, link: null, all_full: true };
  return { players, max_players: s.max_players, link: signedIn || guest ? s.link : null, all_full: false };
}

/// Hold a guest join ticket's guest and lease in the LiveHost object of `simId` (its op hold_guest_ticket), so the ticket
/// itself carries neither. The answer is the object's: `{ ok: true }`, or a refusal with status and code.
export async function holdGuestTicket(env, simId, args) {
  if (!env.LIVE_HOSTS) return { ok: false, status: 409, code: 'not_live', error: 'This simulation is not hosted at that session right now' };
  return liveCall(env, guestSessionName(simId), 'hold_guest_ticket', args);
}

/// Whether a guest (no account, so no age is known and the person may be a child) may be in a session of listing `sim`
/// now: hosted guest play is on, the listing serves as approved (simForServing, the download's own test), it is public
/// (isListable), it is rated G to PG (guest.mjs GUEST_RATINGS: all_ages or ages_10), and it is not adult content. The same test admits a guest and keeps one connected:
/// a listing that fails it after a guest joined has that guest ended on the host's next heartbeat.
export async function guestEligible(env, simId, sim, isListable) {
  if (!guestPlayOn(env) || !sim) return false;
  const served = await simForServing(env, { id: simId, ...sim }, 'content');
  return !!served && served.moderation?.status === 'approved' && isListable(served)
    && isGuestRating(served) && !isAdultListing(served);
}

/// The certificate pin of the session hosting `simId` now, or `null` when nobody hosts it, this deployment has no live
/// registry, or what the host stored is not the exact join grammar. A guest join ticket is minted only for this pin.
export async function livePin(env, simId, now, { guestOnly = false } = {}) {
  if (!env.LIVE_HOSTS) return null;
  const s = await liveCall(env, sessionName(simId, guestOnly), 'status', { now });
  if (s.ok === false || !s.live) return null;
  const link = checkJoinLink(s.link);
  return link.ok ? link.pin.toLowerCase() : null;
}

/// A call into a listing's guest-only session object (guest.mjs reaches it this way for the chat check).
export async function guestSession(env, simId, op, args) {
  if (!env.LIVE_HOSTS) return { ok: false, status: 503, code: 'live_unavailable' };
  return liveCall(env, guestSessionName(simId), op, args);
}

async function liveCall(env, name, op, args) {
  const stub = env.LIVE_HOSTS.get(env.LIVE_HOSTS.idFromName(name));
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

/// What a refused heartbeat carries, so a host lets every guest go (GUEST_PLAY.md section 13).
export const END_ALL_GUESTS = Object.freeze({ end_all_guests: true });

/// Whether `request` is a gallery heartbeat (POST /api/simulations/{id}/live), whose refusals carry END_ALL_GUESTS.
export const isLiveHeartbeat = (request, path) => request.method === 'POST' && /^\/api\/simulations\/[^/]+\/live$/.test(path);

/// `deps` carries the index.js helpers: verifyAuth, requireAdmin, json, and
/// isListable from moderation.mjs.
export async function handleLive(request, env, cors, simId, deps) {
  const { verifyAuth, requireAdmin, json, isListable } = deps;
  if (!env.LIVE_HOSTS) {
    return json({ error: 'Live hosting is not configured on this deployment', code: 'live_unavailable' }, 503, cors);
  }
  // /live/admit takes POST only, from the author; it is not a second way to read /live.
  const admitting = new URL(request.url).pathname.endsWith('/live/admit');
  if (admitting && request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  // A heartbeat the Worker refuses (the listing gone, the author's session revoked or banned, not the author) tells the
  // host to let every guest go (`end_all_guests: true`), since no answer it can act on will follow. GUEST_PLAY.md section
  // 13 also has the host end every guest after 90 s without a 200 heartbeat, which covers the refusals that cannot say so.
  const endAll = request.method === 'POST' && !admitting ? END_ALL_GUESTS : {};
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return json({ error: 'Simulation not found', ...endAll }, 404, cors);
  const sim = await simWithLegalLock(env, JSON.parse(raw));
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
    } else {
      // Adult content: whether it is live, and where to join, go to a viewer who turned it on and to its author
      // (src/adult_content.mjs). A guest lease never reaches it: a guest is signed out, so off.
      const refused = await refuseAdult(request, env, sim, { authenticate: verifyAuth, json, cors });
      if (refused) return refused;
    }
    // The join link goes only to someone who may join: a signed-in account (the account session), or a guest holding a
    // live lease on this listing (the X-Eustress-Lease header), who is answered about the guest-only session, the only
    // kind a guest joins. Anyone else sees whether the account session is live and how many are playing, but not where.
    // The host still demands a ticket at Hello (POST /live/admit), so the link is no longer a capability a pasted copy
    // could use.
    const signedIn = await verifyAuth(request, env);
    // A guest's lease must be on this simulation: a lease on one listing is never the key to another's host. With hosted
    // guest play off, a lease reads nothing about the guest-only session: it is answered as any signed-out visitor is.
    const lease = signedIn || !guestPlayOn(env) ? false : await leaseIsLive(env, request.headers.get('X-Eustress-Lease'), now, simId);
    const s = await liveCall(env, sessionName(simId, !!lease), 'status', { now });
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
    if (lease) body.guest_only = true;
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
      return json({ error: 'This server credential is not valid for this listing', code: 'bad_server_credential', ...endAll }, 401, cors);
    }
  } else {
    userId = await verifyAuth(request, env);
    if (!userId) return json({ error: 'Unauthorized', ...endAll }, 401, cors);
    if (userId !== sim.author_id) {
      return json({ error: 'Only the author can host this simulation for now', code: 'not_author', ...endAll }, 403, cors);
    }
  }

  // The host asks whether a joining player may be admitted.
  if (admitting) {
    const body = await bodyObject(request);
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
    // A guest (no account, so no age is known and the person may be a child) joins only while hosted guest play is on, only a
    // public, approved, all-ages listing that is not adult content (guestEligible), and only while the lease the ticket was
    // minted from is still beating; the ticket must be for this listing. Checked here, before the Durable Object spends the
    // ticket, so a refused guest keeps it. The object then checks the host's declared guest protections and the one seat per
    // lease. GUEST_PLAY.md sections 8 and 13.
    if (isGuestJoinTicket(body.ticket)) {
      const headers = { ...cors, 'Cache-Control': 'no-store' };
      if (!guestPlayOn(env)) {
        return json({ error: 'Joining as a guest is not available yet. Sign in to join.', code: 'guest_join_unavailable' }, 403, headers);
      }
      const g = await inspectGuestJoin(env, body.ticket, { audience: body.audience, now });
      if (g.ok) {
        if (g.sim !== simId) return json({ error: 'The ticket is not for this hosting session', code: 'wrong_host' }, 403, headers);
        if (!(await guestEligible(env, simId, sim, isListable))) {
          return json({ error: 'This session needs an account. Sign in to join.', code: 'guest_not_allowed' }, 403, headers);
        }
        // The lease the ticket was minted from, held by the session at mint. A ticket not held there goes on to the object,
        // which refuses it in its own words (used, or not valid).
        const p = await liveCall(env, guestSessionName(simId), 'guest_pending', { id: g.id });
        if (p.ok === true && !(await leaseHeld(env, p.gid, p.tid, now))) {
          return json({ error: 'Your guest time has ended. Sign in to keep playing.', code: 'lease_ended' }, 403, headers);
        }
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
    // A guest ticket is admitted (or refused) by the guest-only session; any other ticket by the session the host says it
    // runs (`guest_only: true` on the admit, as on its heartbeat), which refuses an account if it is guest-only.
    const toGuestSession = isGuestJoinTicket(body.ticket) || body.guest_only === true;
    // One certificate pin is one host process. When the listing's other session (account or guest-only) is live at the pin
    // this ticket is for, that process would hold accounts and guests at once: nobody is admitted to it.
    if ((await livePin(env, simId, now, { guestOnly: !toGuestSession })) === body.audience) {
      return json({ error: 'This session cannot take players right now.', code: 'pin_in_use' }, 403, { ...cors, 'Cache-Control': 'no-store' });
    }
    const r = await liveCall(env, sessionName(simId, toGuestSession), 'admit', { ticket: body.ticket, audience: body.audience, now });
    if (r.ok === false) {
      return json({ error: r.error, code: r.code || 'admit_failed' }, r.status || 500, { ...cors, 'Cache-Control': 'no-store' });
    }
    // The player was admitted: a visit by them, counted once a day (src/visits.mjs); a guest once per lease, however many
    // times it joins. Never fails the admission.
    if (deps.onJoin) await Promise.resolve(deps.onJoin(sim, { id: r.id, accountId: r.account_id, guest: r.guest, visit: r.visit })).catch((e) => console.error('onJoin failed:', e?.message || e));
    return json({ ok: true, id: r.id, account_id: r.account_id, username: r.username, guest: r.guest, expires: r.expires }, 200, { ...cors, 'Cache-Control': 'no-store' });
  }

  if (request.method === 'DELETE') {
    const params = new URL(request.url).searchParams;
    const guestOnly = params.get('guest_only') === 'true';
    const serverId = params.get('server_id');
    if (serverId !== null && !SERVER_ID.test(serverId)) {
      return json({ error: 'server_id must be 1 to 64 letters, digits, _ or -', code: 'bad_server_id' }, 400, cors);
    }
    const r = await liveCall(env, sessionName(simId, guestOnly), 'stop', { now, server_id: serverId });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    if (r.live && !guestOnly && isListable(sim)) {
      // Other servers still host the listing: the index shows the Quick Join server's players.
      await tellLiveIndex(env, 'beat', {
        sim_id: simId, author: sim.author_id, players: r.shown_players, max_players: r.max_players, region: r.region ?? null, since: r.since, now,
      });
    } else if (!r.live) {
      // The live index drops the session at once. An index that fails never fails a stop.
      await tellLiveIndex(env, 'stop', { sim_id: simId }, guestOnly ? GUEST_LIVE_INDEX_NAME : undefined);
    }
    return json({ ok: true }, 200, cors);
  }

  if (request.method === 'POST') {
    const body = await bodyObject(request);
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
    // Optional: the guest protections this host declares (GUEST_CAPS_REQUIRED). Missing means none.
    let guestCaps = [];
    if (body.guest_caps !== undefined) {
      const ok = Array.isArray(body.guest_caps)
        && body.guest_caps.length <= MAX_GUEST_CAPS
        && body.guest_caps.every((c) => typeof c === 'string' && /^[a-z_]{1,32}$/.test(c));
      if (!ok) return json({ error: `guest_caps must list at most ${MAX_GUEST_CAPS} names of lowercase letters and underscores`, code: 'bad_guest_caps' }, 400, cors);
      guestCaps = [...new Set(body.guest_caps)];
    }
    // A guest-only session takes guests and nobody else. Its heartbeat is taken even while hosted guest play is off, so the
    // answer can name every connected guest in ended_guests (a refused heartbeat would leave them with the host); it then
    // admits nobody and is listed nowhere.
    if (body.guest_only !== undefined && typeof body.guest_only !== 'boolean') {
      return json({ error: 'guest_only must be true or false', code: 'bad_guest_only' }, 400, cors);
    }
    const guestOnly = body.guest_only === true;
    // One certificate pin is one host process: a session may not beat at the pin the listing's other session (account or
    // guest-only) is live at, or that one process would take accounts and guests at once. A guest-only beat refused here
    // also tells the host to let its guests go.
    const otherPin = await livePin(env, simId, now, { guestOnly: !guestOnly });
    if (otherPin && otherPin === link.pin.toLowerCase()) {
      return json({
        error: 'The listing\'s other session is live at this certificate pin. Guest-only and account sessions run in separate host processes.',
        code: 'pin_in_use', ...(guestOnly ? END_ALL_GUESTS : {}),
      }, 409, cors);
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
    const r = await liveCall(env, sessionName(simId, guestOnly), 'beat', {
      host_id: userId, link: body.link, players, max_players: maxPlayers, protocol, connected, guest_caps: guestCaps, guest_only: guestOnly,
      cert_sha256: certSha256, now, server_id: serverId, region_name: regionName, latency_ms: latencyMs, hardware, ...regionOf(request),
    });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    // Each connected guest's lease is beaten from here, so hosted guest time is measured by this Worker's clock the same way
    // as a guest's own beats. A guest is ended, and reported in `ended_guests` on every beat until the host stops naming it,
    // when its lease has ended (time used up, a newer lease from the same guest, or ended by the guest), when its lease
    // cannot be beaten (the allowance failed: fail closed), when the host names it but holds no seat for it here (the seats
    // were forgotten after a heartbeat gap), and when it may no longer stay at all: hosted guest play turned off, this beat
    // not declaring the guest protections, the host's own chat refused too often today, or the listing no longer
    // guest-eligible (re-rated, unrated, quarantined, back to review, made private, adult).
    const ended = (r.guests || []).filter((g) => g.ended).map((g) => g.id);
    const beating = (r.guests || []).filter((g) => !g.ended);
    // Whether guests may be in this session now. Only a guest-only session ever takes one.
    const guestsAllowed = guestOnly && guestPlayOn(env) && r.guest_caps_ok === true && r.host_barred !== true
      && (await guestEligible(env, simId, sim, isListable).catch((e) => { console.error('guestEligible failed:', e?.message || e); return false; }));
    let newlyEnded = [];
    if (beating.length) {
      if (!guestsAllowed) newlyEnded = beating.map((g) => g.id);
      else {
        const results = await Promise.all(beating.map((g) => beatGuestLease(env, g.gid, g.tid, now).catch(() => false)));
        newlyEnded = beating.filter((_, i) => results[i] !== true).map((g) => g.id);
      }
    }
    if (newlyEnded.length) await liveCall(env, sessionName(simId, guestOnly), 'guests_ended', { ids: newlyEnded });
    // A chat bar that could not be written to a guest's allowance when its line was refused is tried again on every beat
    // until it is written (or the guest id's 25 hours are over, when the session drops it). Never fails a heartbeat.
    for (const q of r.bar_retry || []) {
      if (await barGuest(env, { gid: q.gid, tid: q.tid, sim: simId, gx: q.gx, now })) {
        await liveCall(env, sessionName(simId, guestOnly), 'bar_done', { tid: q.tid }).catch(() => null);
      }
    }
    // The live index learns of this beat only while the listing is listed: an author's unlisted listings never fill it. It is told no
    // link, key or pin, only what the Gallery shows. The players are the Quick Join server's: the busiest one that is not full,
    // on the default server still capped at the players the Worker admitted (plus the host). An index that fails never fails a heartbeat.
    // A guest-only session goes to the guest index (GET /api/guest/live), and only while a guest could join it: hosted guest
    // play on, the host declaring every guest protection on this beat, its chat not refused too often today, and the listing
    // guest-eligible (guestEligible). One that stops being joinable is taken out at once.
    const listed = guestOnly ? guestsAllowed : isListable(sim);
    if (listed) {
      await tellLiveIndex(env, 'beat', {
        sim_id: simId, author: sim.author_id, players: Number.isSafeInteger(r.shown_players) ? r.shown_players : cappedPlayers(players, r.admitted), max_players: r.max_players || maxPlayers, region: r.region ?? null, since: r.since, now,
      }, guestOnly ? GUEST_LIVE_INDEX_NAME : undefined);
    } else if (guestOnly) {
      await tellLiveIndex(env, 'stop', { sim_id: simId }, GUEST_LIVE_INDEX_NAME);
    }
    // Named only when there is a guest to let go, so a session without one answers exactly what it always did.
    const out = { ok: true, expires_at: r.expires_at };
    if (ended.length || newlyEnded.length) out.ended_guests = [...ended, ...newlyEnded];
    // The host's own chat was refused HOST_CHAT_REFUSALS_MAX times today: every guest goes, and none joins until tomorrow.
    if (guestOnly && r.host_barred === true) out.end_all_guests = true;
    return json(out, 200, cors);
  }

  return json({ error: 'Method not allowed' }, 405, cors);
}
