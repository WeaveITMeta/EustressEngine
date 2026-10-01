// =============================================================================
// Guest play: the Gallery without an account
// =============================================================================
//
// The design, its limits and McKale's decisions are in
// docs/architecture/GUEST_PLAY.md. In short:
//
//   - A guest is a network address, for one UTC day: 3 simulations and 60
//     minutes between them, however many people sit behind the address. The
//     guest id is a keyed hash of the address, so there is no token, cookie or
//     header, and the id changes every day.
//   - Time is a lease the Worker measures with its own clock. POST /play opens
//     one and returns a signed ticket; the browser Player beats every 30 s and
//     the Worker debits the time it measured itself. No beat for 90 s ends the
//     lease. There is no refund and no client- or host-reported time.
//   - Only approved, public, all-ages simulations are guest-playable.
//
//     POST /api/guest/play       {sim_id, character: "M"|"F"}  -> ticket
//     POST /api/guest/beat       {ticket}                       -> remaining
//     POST /api/guest/end        {ticket}                       -> remaining
//     GET  /api/guest/allowance                                 -> what is left
// =============================================================================

export const MAX_SIMS = 3;
export const DAILY_SECONDS = 60 * 60;
export const LEASE_MAX_SECONDS = 60 * 60;
export const BEAT_INTERVAL_SECONDS = 30;
/// No beat for this long and the lease is over.
export const LEASE_TIMEOUT_MS = 90_000;
/// One beat debits at most this much, however long since the last: 1.5 times
/// the interval, so a client that stalls and resumes is not charged for the gap.
export const BEAT_CAP_SECONDS = BEAT_INTERVAL_SECONDS * 1.5;
/// A guest's record is deleted this long after its day began.
const RETENTION_MS = 25 * 60 * 60 * 1000;
/// Slack on a ticket's own expiry, beyond the longest lease.
const TICKET_SLACK_SECONDS = 120;

const enc = new TextEncoder();

// -----------------------------------------------------------------------------
// Days and addresses
// -----------------------------------------------------------------------------

export const dayOf = (now) => new Date(now).toISOString().slice(0, 10);
export const dayStart = (now) => Date.parse(`${dayOf(now)}T00:00:00.000Z`);
export const nextMidnight = (now) => dayStart(now) + 24 * 60 * 60 * 1000;

/// The address reduced to what one guest is: an IPv4 address, or the /64 of an
/// IPv6 one, so a device cannot step through the addresses of its own network
/// to mint allowances. An IPv4-mapped IPv6 address reads as the IPv4 it is.
/// `null` for anything that is not an address.
export function addressKey(raw) {
  if (typeof raw !== 'string') return null;
  const text = raw.trim();
  if (!text) return null;
  let host;
  try {
    host = new URL(text.includes(':') ? `http://[${text}]/` : `http://${text}/`).hostname;
  } catch {
    return null;
  }
  if (!host.startsWith('[')) {
    // Only a dotted-quad IPv4 address is an address here; the URL parser would
    // also turn a bare name or number into a host.
    return /^\d{1,3}(\.\d{1,3}){3}$/.test(text) ? host : null;
  }
  const v6 = host.slice(1, -1);
  // ::ffff:a.b.c.d, which the parser writes as ::ffff:xxxx:xxxx
  const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(v6);
  if (mapped) {
    const hi = parseInt(mapped[1], 16);
    const lo = parseInt(mapped[2], 16);
    return `${hi >> 8}.${hi & 255}.${lo >> 8}.${lo & 255}`;
  }
  const groups = expandV6(v6);
  return groups ? `${groups.slice(0, 4).join(':')}::/64` : null;
}

/// The eight 16-bit groups of a canonical IPv6 text, as 4-digit lowercase hex.
function expandV6(text) {
  const halves = text.split('::');
  if (halves.length > 2) return null;
  const head = halves[0] ? halves[0].split(':') : [];
  const tail = halves.length === 2 && halves[1] ? halves[1].split(':') : [];
  if (halves.length === 1 && head.length !== 8) return null;
  const fill = 8 - head.length - tail.length;
  if (fill < 0 || (halves.length === 2 && fill < 1)) return null;
  const all = [...head, ...Array(halves.length === 2 ? fill : 0).fill('0'), ...tail];
  if (all.length !== 8 || !all.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return null;
  return all.map((g) => g.padStart(4, '0'));
}

// -----------------------------------------------------------------------------
// Keys, ids and tickets
// -----------------------------------------------------------------------------

const toHex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
const b64url = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const unb64url = (text) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));

async function hmacKey(secretBytes, usage) {
  return crypto.subtle.importKey('raw', secretBytes, { name: 'HMAC', hash: 'SHA-256' }, false, usage);
}

async function hmac(secret, message) {
  return crypto.subtle.sign('HMAC', await hmacKey(enc.encode(secret), ['sign']), enc.encode(message));
}

/// A key for one purpose, derived from the Worker's existing secret, so there
/// is nothing new to configure and a guest ticket can never verify as an
/// account token (a different key and a different prefix).
async function derivedKey(env, purpose) {
  return hmacKey(await hmac(env.JWT_SECRET, purpose), ['sign', 'verify']);
}

/// The guest id for `ip` on the UTC day of `now`: a keyed hash of the address
/// under a key that changes every day. It cannot be turned back into the
/// address, and two days' ids cannot be matched.
export async function guestId(env, ip, now) {
  const key = await derivedKey(env, `eustress-guest-day:${dayOf(now)}`);
  const mac = await crypto.subtle.sign('HMAC', key, enc.encode(ip));
  return toHex(mac).slice(0, 32);
}

const PREFIX = 'egl1';

/// A lease ticket: permission to heartbeat one lease. `egl1.<payload>.<mac>`.
export async function signLease(env, { tid, gid, sim, character, now }) {
  const iat = Math.floor(now / 1000);
  const payload = { t: tid, g: gid, s: sim, c: character, d: dayOf(now), i: iat, e: iat + LEASE_MAX_SECONDS + TICKET_SLACK_SECONDS };
  const body = b64url(enc.encode(JSON.stringify(payload)));
  const key = await derivedKey(env, 'eustress-guest-lease-v1');
  const mac = await crypto.subtle.sign('HMAC', key, enc.encode(`${PREFIX}.${body}`));
  return { ticket: `${PREFIX}.${body}.${b64url(mac)}`, expires_in: LEASE_MAX_SECONDS + TICKET_SLACK_SECONDS };
}

/// The payload of a valid, unexpired lease ticket, or null.
export async function verifyLease(env, ticket, now) {
  if (typeof ticket !== 'string' || ticket.length > 1024) return null;
  const parts = ticket.split('.');
  if (parts.length !== 3 || parts[0] !== PREFIX) return null;
  let mac;
  let payload;
  try {
    mac = unb64url(parts[2]);
    payload = JSON.parse(new TextDecoder().decode(unb64url(parts[1])));
  } catch {
    return null;
  }
  const key = await derivedKey(env, 'eustress-guest-lease-v1');
  // verify() compares in constant time.
  const ok = await crypto.subtle.verify('HMAC', key, mac, enc.encode(`${PREFIX}.${parts[1]}`));
  if (!ok || !payload || typeof payload.e !== 'number' || payload.e < Math.floor(now / 1000)) return null;
  if (typeof payload.t !== 'string' || typeof payload.g !== 'string' || typeof payload.s !== 'string') return null;
  return payload;
}

// -----------------------------------------------------------------------------
// The allowance: one Durable Object per guest id
// -----------------------------------------------------------------------------

const seconds3 = (n) => Math.round(n * 1000) / 1000;

const ALLOWANCE_OPS = {
  async status({ now }) {
    const a = await this.load(now);
    return { ok: true, ...this.summary(a) };
  },

  async begin({ now, sim, tid }) {
    const a = await this.load(now);
    if (a.seconds >= DAILY_SECONDS) return { ok: false, status: 429, reason: 'minutes', ...this.summary(a) };
    if (!a.sims.includes(sim)) {
      if (a.sims.length >= MAX_SIMS) return { ok: false, status: 429, reason: 'sims', ...this.summary(a) };
      a.sims.push(sim);
    }
    // One lease at a time: this one ends the last. The old lease is not
    // charged for the time since its final beat, since only beats debit.
    a.lease = { tid, sim, started: now, last_beat: now };
    await this.storage.put('a', a);
    return { ok: true, ...this.summary(a) };
  },

  async beat({ now, tid }) {
    const a = await this.load(now);
    const lease = a.lease;
    if (!lease || lease.tid !== tid) return { ok: false, status: 410, code: 'lease_ended', ...this.summary(a) };
    if (now - lease.last_beat > LEASE_TIMEOUT_MS) {
      a.lease = null;
      await this.storage.put('a', a);
      return { ok: false, status: 410, code: 'lease_expired', ...this.summary(a) };
    }
    a.seconds = seconds3(a.seconds + Math.min((now - lease.last_beat) / 1000, BEAT_CAP_SECONDS));
    lease.last_beat = now;
    if (a.seconds >= DAILY_SECONDS) {
      a.seconds = DAILY_SECONDS;
      a.lease = null;
      await this.storage.put('a', a);
      return { ok: false, status: 410, code: 'guest_limit', reason: 'minutes', ...this.summary(a) };
    }
    await this.storage.put('a', a);
    return { ok: true, ...this.summary(a) };
  },

  /// Whether lease `tid` is the guest's current lease and has beaten recently.
  /// Reads only: asking creates nothing.
  async holds({ now, tid }) {
    const lease = (await this.storage.get('a'))?.lease;
    return { ok: true, live: !!lease && lease.tid === tid && now - lease.last_beat <= LEASE_TIMEOUT_MS };
  },

  async end({ now, tid }) {
    const a = await this.load(now);
    const lease = a.lease;
    if (lease && lease.tid === tid) {
      if (now - lease.last_beat <= LEASE_TIMEOUT_MS) {
        a.seconds = Math.min(DAILY_SECONDS, seconds3(a.seconds + Math.min((now - lease.last_beat) / 1000, BEAT_CAP_SECONDS)));
      }
      a.lease = null;
      await this.storage.put('a', a);
    }
    return { ok: true, ...this.summary(a) };
  },
};

/// One guest's day. Named by the guest id, which already contains the day.
export class GuestAllowance {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
  }

  async load(now) {
    let a = await this.storage.get('a');
    if (!a) {
      a = { day: dayOf(now), sims: [], seconds: 0, lease: null };
      await this.storage.put('a', a);
      // Gone 25 hours after the day began, so nothing outlives its use.
      await this.storage.setAlarm(dayStart(now) + RETENTION_MS);
    }
    return a;
  }

  summary(a) {
    return {
      sims_left: Math.max(0, MAX_SIMS - a.sims.length),
      seconds_left: Math.max(0, Math.floor(DAILY_SECONDS - a.seconds)),
      resets_at: new Date(nextMidnight(Date.parse(`${a.day}T12:00:00.000Z`))).toISOString(),
    };
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
      const handler = ALLOWANCE_OPS[op];
      result = handler ? await handler.call(this, args) : { ok: false, status: 404, error: `unknown operation ${op}` };
    } catch (e) {
      console.error(`GuestAllowance.${op} failed:`, e);
      result = { ok: false, status: 500, error: String(e?.message || e) };
    }
    return new Response(JSON.stringify(result), {
      status: result.ok === false ? result.status || 500 : 200,
      headers: { 'content-type': 'application/json' },
    });
  }

  async alarm() {
    await this.storage.deleteAll();
  }
}

async function allowanceCall(env, gid, op, args) {
  const stub = env.GUEST_ALLOWANCE.get(env.GUEST_ALLOWANCE.idFromName(gid));
  const res = await stub.fetch(`https://guest.internal/${op}`, {
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

/// Whether `ticket` is a genuine lease ticket whose lease is still the guest's
/// current, beating one. False for anything else, including a missing ticket
/// and a deployment without guest play.
export async function leaseIsLive(env, ticket, now) {
  if (!ticket || !env.GUEST_ALLOWANCE || !env.JWT_SECRET) return false;
  const lease = await verifyLease(env, ticket, now);
  if (!lease) return false;
  const r = await allowanceCall(env, lease.g, 'holds', { now, tid: lease.t });
  return r.ok === true && r.live === true;
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

const SIM_ID =/^[a-f0-9-]{1,64}$/;

function limitBody(r, error) {
  return {
    error,
    code: 'guest_limit',
    reason: r.reason,
    resets_at: r.resets_at,
    sims_left: r.sims_left,
    seconds_left: r.seconds_left,
  };
}

function retryAfter(r, now) {
  return String(Math.max(1, Math.ceil((Date.parse(r.resets_at) - now) / 1000)));
}

/// `deps` carries the index.js helpers: verifyAuth, json and isListable.
export async function handleGuest(request, env, cors, deps) {
  const { verifyAuth, json, isListable } = deps;
  const path = new URL(request.url).pathname;
  const now = Date.now();

  if (!env.GUEST_ALLOWANCE || !env.JWT_SECRET) {
    return json({ error: 'Guest play is not available on this deployment', code: 'guest_unavailable' }, 503, cors);
  }

  // Beat and end carry the ticket, which names the guest, so they work from
  // any address (a phone moving between networks mid-play).
  if (request.method === 'POST' && (path === '/api/guest/beat' || path === '/api/guest/end')) {
    const body = await request.json().catch(() => ({}));
    const lease = await verifyLease(env, body.ticket, now);
    if (!lease) return json({ error: 'This ticket is not valid', code: 'bad_ticket', expired: true }, 410, cors);
    const op = path === '/api/guest/beat' ? 'beat' : 'end';
    const r = await allowanceCall(env, lease.g, op, { now, tid: lease.t });
    if (r.ok === false) {
      const headers = r.code === 'guest_limit' ? { ...cors, 'Retry-After': retryAfter(r, now) } : cors;
      return json({ error: 'The guest session has ended', code: r.code || 'guest_error', reason: r.reason, expired: true, seconds_left: r.seconds_left, sims_left: r.sims_left, resets_at: r.resets_at }, r.status || 500, headers);
    }
    return json({ ok: true, expired: false, seconds_left: r.seconds_left, sims_left: r.sims_left, resets_at: r.resets_at }, 200, cors);
  }

  // Everything else is keyed on the caller's address, which Cloudflare supplies.
  // With none (a local `wrangler dev`) there is nothing to key on, so refuse.
  const ip = addressKey(request.headers.get('CF-Connecting-IP'));
  if (!ip) return json({ error: 'Your network address could not be read', code: 'address_unavailable' }, 400, cors);
  const gid = await guestId(env, ip, now);

  if (request.method === 'GET' && path === '/api/guest/allowance') {
    const r = await allowanceCall(env, gid, 'status', { now });
    if (r.ok === false) return json({ error: r.error }, r.status || 500, cors);
    return json({ sims_left: r.sims_left, seconds_left: r.seconds_left, resets_at: r.resets_at }, 200, { ...cors, 'Cache-Control': 'no-store' });
  }

  if (request.method === 'POST' && path === '/api/guest/play') {
    // A signed-in caller plays as themselves and never touches a guest allowance.
    if (await verifyAuth(request, env)) {
      return json({ error: 'You are signed in, so you do not need a guest session', code: 'signed_in' }, 409, cors);
    }
    const body = await request.json().catch(() => ({}));
    if (typeof body.sim_id !== 'string' || !SIM_ID.test(body.sim_id)) return json({ error: 'sim_id required', code: 'bad_request' }, 400, cors);
    if (body.character !== 'M' && body.character !== 'F') return json({ error: 'character must be M or F', code: 'bad_request' }, 400, cors);

    const raw = await env.SOCIAL.get(`sim:${body.sim_id}`);
    const sim = raw ? JSON.parse(raw) : null;
    // A listing that is not public answers like one that does not exist.
    if (!sim || !isListable(sim)) return json({ error: 'Simulation not found' }, 404, cors);
    if (sim.moderation?.rating !== 'all_ages') {
      return json({ error: 'This simulation needs an account', code: 'guest_not_allowed', reason: 'rating' }, 403, cors);
    }
    if (sim.browser_play === false) {
      return json({ error: 'This simulation does not run in the browser', code: 'guest_not_allowed', reason: 'browser' }, 403, cors);
    }

    const tid = toHex(crypto.getRandomValues(new Uint8Array(16)));
    const r = await allowanceCall(env, gid, 'begin', { now, sim: body.sim_id, tid });
    if (r.ok === false) {
      if (r.status === 429) {
        const what = r.reason === 'sims' ? 'You have started 3 simulations today' : 'You have used your 60 minutes today';
        return json(limitBody(r, `${what}. Register to keep playing.`), 429, { ...cors, 'Retry-After': retryAfter(r, now) });
      }
      return json({ error: r.error || 'Guest play failed' }, r.status || 500, cors);
    }
    const { ticket, expires_in } = await signLease(env, { tid, gid, sim: body.sim_id, character: body.character, now });
    return json({
      ticket,
      sim_id: body.sim_id,
      character: body.character,
      expires_in,
      beat_interval: BEAT_INTERVAL_SECONDS,
      seconds_left: r.seconds_left,
      lease_seconds: Math.min(r.seconds_left, LEASE_MAX_SECONDS),
      sims_left: r.sims_left,
      resets_at: r.resets_at,
    }, 200, { ...cors, 'Cache-Control': 'no-store' });
  }

  return json({ error: 'Not found' }, 404, cors);
}
