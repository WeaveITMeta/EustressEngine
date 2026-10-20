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
//   - Only approved, public simulations rated G to PG (`all_ages` or `ages_10`,
//     GUEST_RATINGS) are guest-playable (McKale, 2026-10-16).
//   - A guest joins only a guest-only hosted session (live.mjs): a host that
//     declares `guest_only` takes guests and no accounts, and every other host
//     takes accounts and no guests. Guest chat, and the host's own chat to its
//     guests, is checked line by line before the host delivers it (POST
//     /api/guest/chat/check): the Worker's own personal-information rules
//     (guest_chat_rules.mjs), then Jev within daily caps. A guest line refused
//     for personal information or by Jev ends the guest at once and bars it
//     from that listing for the day; the host is told deliver, block or kick.
//   - Nothing is saved for a guest: no progress, purchases or names.
//   - A guest joins a hosted session with a guest join ticket (`egt1`), minted
//     from a live lease for the session's own pin. It is single use at
//     /live/admit, like an account's identity ticket, and it can never pass as
//     one: another prefix and another key. The ticket carries only its own id,
//     the pin, the listing and an expiry: which guest and which lease it was
//     minted from are held by the session's LiveHost object under the ticket
//     id, so a host reads nothing it could match to the same guest elsewhere.
//     While a guest is connected, the host's heartbeat beats the guest's lease
//     (live.mjs), so hosted time is measured the same way, and the heartbeat
//     answers which guests' leases have ended so the host can let them go.
//   - Hosted guest play is off unless the deployment sets GUEST_PLAY to exactly
//     "on", and even then a guest joins only a host whose latest heartbeat
//     declares the guest protections (live.mjs GUEST_CAPS_REQUIRED).
//
//     POST /api/guest/play       {sim_id, character: "M"|"F", client?: "browser"|"player"}  -> lease ticket
//     POST /api/guest/beat       {ticket}                       -> remaining
//     POST /api/guest/end        {ticket}                       -> remaining
//     POST /api/guest/ticket     {lease, audience}              -> join ticket for one hosted session (GUEST_PLAY "on" only)
//     GET  /api/guest/allowance                                 -> what is left (stores nothing)
//     GET  /api/guest/live                                      -> the guest-only sessions live now (index.js)
//     POST /api/guest/chat/check {sim_id, id, text}             -> the host asks whether to deliver one guest chat line
//     POST /api/guest/chat/check {sim_id, from: "host", text}   -> ... or one of its own lines to its guests
// =============================================================================

import { checkCommunityText, simWithLegalLock } from './moderation.mjs';
import { guestChatRule } from './guest_chat_rules.mjs';

export const MAX_SIMS = 3;
export const DAILY_SECONDS = 60 * 60;
export const LEASE_MAX_SECONDS = 60 * 60;
export const BEAT_INTERVAL_SECONDS = 30;
/// No beat for this long and the lease is over.
export const LEASE_TIMEOUT_MS = 90_000;
/// One beat debits at most this much, however long since the last: 1.5 times
/// the interval, so a client that stalls and resumes is not charged for the gap.
export const BEAT_CAP_SECONDS = BEAT_INTERVAL_SECONDS * 1.5;
/// A guest's record is deleted this long after its day began. Nothing else may hold a guest id longer (live.mjs strips the
/// guest id it keeps for a connected guest at the same moment).
export const RETENTION_MS = 25 * 60 * 60 * 1000;
/// When hosted guest play is on: the deployment's GUEST_PLAY setting is exactly "on". Unset, or any other value, is off.
export const guestPlayOn = (env) => env?.GUEST_PLAY === 'on';

/// The ratings a guest may play: G to PG (McKale, 2026-10-16). A listing with no rating is none of these.
export const GUEST_RATINGS = Object.freeze(['all_ages', 'ages_10']);
export const isGuestRating = (sim) => GUEST_RATINGS.includes(sim?.moderation?.rating);

/// The longest guest chat line the Worker checks. A longer one is not delivered.
export const GUEST_CHAT_MAX_CHARS = 500;
/// The host's own chat lines to its guests are checked too (GUEST_PLAY.md section 13, `host_chat_checked`). A host whose
/// lines are refused this many times in one UTC day has every guest of the listing's guest-only session ended, and takes
/// no guest again until the day is over.
export const HOST_CHAT_REFUSALS_MAX = 3;
/// The daily caps on guest chat checks that reach Jev (each one a paid model call), per listing, per host account and for
/// the whole deployment, from GUEST_CHAT_CAP_LISTING, GUEST_CHAT_CAP_HOST and GUEST_CHAT_CAP_TOTAL. McKale sets the final
/// numbers; these are the defaults when a setting is unset or not a whole number. Past a cap a line is not delivered and
/// nobody is kicked.
export const GUEST_CHAT_CAP_DEFAULTS = Object.freeze({ listing: 5000, host: 5000, total: 50000 });
/// The LiveHost object that counts the day's guest chat checks against the caps. Not a listing id (those are hex and
/// hyphens), so it never names a hosting session.
export const GUEST_CHAT_CAPS_NAME = 'guest-chat-caps';

/// The caps this deployment sets.
export function guestChatCaps(env) {
  const read = (v, d) => {
    if (v === undefined || v === null || String(v).trim() === '') return d;
    const n = Number(v);
    return Number.isSafeInteger(n) && n >= 0 ? n : d;
  };
  return {
    listing: read(env?.GUEST_CHAT_CAP_LISTING, GUEST_CHAT_CAP_DEFAULTS.listing),
    host: read(env?.GUEST_CHAT_CAP_HOST, GUEST_CHAT_CAP_DEFAULTS.host),
    total: read(env?.GUEST_CHAT_CAP_TOTAL, GUEST_CHAT_CAP_DEFAULTS.total),
  };
}
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
// Guest join tickets: a guest's way into one hosted session
// -----------------------------------------------------------------------------

const JOIN_PREFIX = 'egt1';
const JOIN_KEY_LABEL = 'eustress-guest-join-v1';
/// As long as an account's identity ticket (identity.mjs TICKET_TTL_S): long enough to connect, too short to keep.
export const GUEST_JOIN_TTL_S = 10 * 60;
/// What a host shows for a guest. A guest has no name of its own: nothing about the person is asked or kept.
export const GUEST_NAME = 'Guest';

/// Whether `ticket` is shaped as a guest join ticket, so /live/admit knows which kind to check. Shape only: never a proof.
export const isGuestJoinTicket = (ticket) => typeof ticket === 'string' && ticket.startsWith(`${JOIN_PREFIX}.`);

/// A guest join ticket for lease `lease` (a verified lease payload) on the hosting session whose pin is `audience`.
/// `egt1.<payload>.<mac>`, payload {j: id, h: audience, s: sim, e: expiry in unix seconds}. The payload is readable by the
/// host it is handed to, so it names neither the guest nor the lease: the caller holds those server-side under `id`
/// (live.mjs holdGuestTicket) before handing the ticket out.
export async function signGuestJoin(env, { lease, audience, now }) {
  const id = toHex(crypto.getRandomValues(new Uint8Array(16)));
  const nowS = Math.floor(now / 1000);
  const payload = { j: id, h: audience, s: lease.s, e: nowS + GUEST_JOIN_TTL_S };
  const body = b64url(enc.encode(JSON.stringify(payload)));
  const key = await derivedKey(env, JOIN_KEY_LABEL);
  const mac = await crypto.subtle.sign('HMAC', key, enc.encode(`${JOIN_PREFIX}.${body}`));
  return { ticket: `${JOIN_PREFIX}.${body}.${b64url(mac)}`, id, expires: payload.e };
}

/// When the guest id a lease names stops being kept anywhere: 25 hours after the UTC day the lease was opened on began.
export const guestIdExpiry = (lease) => Date.parse(`${lease.d}T00:00:00.000Z`) + RETENTION_MS;

/// Why a guest join ticket is refused, or what it names (its id, listing and expiry; never the guest). The codes are
/// identity.mjs's: `no_ticket`, `invalid`, `expired`, `wrong_host` (made for another hosting session's pin).
export async function inspectGuestJoin(env, ticket, { audience, now }) {
  if (ticket === undefined || ticket === null || ticket === '') return { ok: false, code: 'no_ticket' };
  if (typeof ticket !== 'string' || ticket.length > 2048) return { ok: false, code: 'invalid' };
  const parts = ticket.split('.');
  if (parts.length !== 3 || parts[0] !== JOIN_PREFIX || !env?.JWT_SECRET) return { ok: false, code: 'invalid' };
  let mac;
  let payload;
  try {
    mac = unb64url(parts[2]);
    payload = JSON.parse(new TextDecoder().decode(unb64url(parts[1])));
  } catch {
    return { ok: false, code: 'invalid' };
  }
  const key = await derivedKey(env, JOIN_KEY_LABEL);
  // verify() compares in constant time.
  const ok = await crypto.subtle.verify('HMAC', key, mac, enc.encode(`${JOIN_PREFIX}.${parts[1]}`));
  if (!ok || !payload || !Number.isSafeInteger(payload.e)) return { ok: false, code: 'invalid' };
  if (typeof payload.j !== 'string' || !/^[0-9a-f]{32}$/.test(payload.j) || typeof payload.s !== 'string') {
    return { ok: false, code: 'invalid' };
  }
  if (payload.e < Math.floor(now / 1000)) return { ok: false, code: 'expired' };
  if (payload.h !== audience) return { ok: false, code: 'wrong_host' };
  return { ok: true, id: payload.j, sim: payload.s, expires: payload.e };
}

// -----------------------------------------------------------------------------
// The allowance: one Durable Object per guest id
// -----------------------------------------------------------------------------

const seconds3 = (n) => Math.round(n * 1000) / 1000;
/// A day with nothing used, which is what a guest id with no record has.
const blankDay = (now) => ({ day: dayOf(now), sims: [], seconds: 0, lease: null });

const ALLOWANCE_OPS = {
  /// What is left. Reads only: a visitor who asks and never plays leaves no record and no alarm.
  async status({ now }) {
    const a = (await this.storage.get('a')) || blankDay(now);
    return { ok: true, ...this.summary(a) };
  },

  /// The only op that creates a record. `next`: the same address's guest id for the next UTC day, kept so a chat refusal
  /// after midnight on a lease opened before it bars the address for the day it is refused on (bar).
  async begin({ now, sim, tid, next }) {
    const a = await this.load(now, { create: true });
    // A guest whose chat was refused on this listing today does not come back to it today.
    if (Array.isArray(a.barred) && a.barred.includes(sim)) return { ok: false, status: 403, code: 'guest_barred', ...this.summary(a) };
    if (a.seconds >= DAILY_SECONDS) return { ok: false, status: 429, reason: 'minutes', ...this.summary(a) };
    if (!a.sims.includes(sim)) {
      if (a.sims.length >= MAX_SIMS) return { ok: false, status: 429, reason: 'sims', ...this.summary(a) };
      a.sims.push(sim);
    }
    if (typeof next === 'string' && /^[0-9a-f]{32}$/.test(next)) a.next = next;
    // One lease at a time: this one ends the last. The old lease is not
    // charged for the time since its final beat, since only beats debit.
    a.lease = { tid, sim, started: now, last_beat: now };
    await this.storage.put('a', a);
    return { ok: true, ...this.summary(a) };
  },

  /// `hosted`: the beat comes from the host's heartbeat (live.mjs), not the guest's own Player. A host beats up to the
  /// lease timeout apart while the guest plays the whole time, so a hosted beat is charged the whole gap (at most the
  /// timeout, past which the lease has already ended), not the 1.5-interval cap that forgives a stalled client.
  /// No record (never begun, or deleted by the alarm at day start + 25 h): the lease has ended, and nothing is written.
  async beat({ now, tid, hosted }) {
    const a = await this.load(now);
    if (!a) return { ok: false, status: 410, code: 'lease_ended', ...this.summary(blankDay(now)) };
    const lease = a.lease;
    if (!lease || lease.tid !== tid) return { ok: false, status: 410, code: 'lease_ended', ...this.summary(a) };
    if (now - lease.last_beat > LEASE_TIMEOUT_MS) {
      a.lease = null;
      await this.storage.put('a', a);
      return { ok: false, status: 410, code: 'lease_expired', ...this.summary(a) };
    }
    const cap = hosted === true ? LEASE_TIMEOUT_MS / 1000 : BEAT_CAP_SECONDS;
    a.seconds = seconds3(a.seconds + Math.min(Math.max(0, now - lease.last_beat) / 1000, cap));
    lease.last_beat = Math.max(lease.last_beat, now);
    if (a.seconds >= DAILY_SECONDS) {
      a.seconds = DAILY_SECONDS;
      a.lease = null;
      await this.storage.put('a', a);
      return { ok: false, status: 410, code: 'guest_limit', reason: 'minutes', ...this.summary(a) };
    }
    await this.storage.put('a', a);
    return { ok: true, ...this.summary(a) };
  },

  /// A chat line from this guest on listing `sim` was refused: lease `tid` ends now, and the listing is barred for the rest of
  /// the day (the record is deleted 25 hours after its day began, so the bar goes with it). Nothing about the line is kept.
  /// `day` is the UTC day this guest id belongs to. A record is created here only for the current UTC day's guest id (its
  /// alarm is that day's start + 25 h), never for an earlier day's, so no guest id outlives its 25 hours. When this record
  /// is an earlier day's (a lease opened before midnight, refused after it), the answer names `next`, the same address's
  /// guest id for today, which the caller bars as well (barGuest).
  async bar({ now, sim, tid, day }) {
    const a = await this.load(now, { create: day === dayOf(now) });
    if (!a) return { ok: true, next: null };
    if (typeof sim === 'string' && !(a.barred || []).includes(sim)) a.barred = [...(a.barred || []), sim];
    if (a.lease && a.lease.tid === tid) a.lease = null;
    await this.storage.put('a', a);
    return { ok: true, next: a.day !== dayOf(now) && typeof a.next === 'string' ? a.next : null };
  },

  /// Whether lease `tid` is the guest's current lease and has beaten recently.
  /// Reads only: asking creates nothing.
  async holds({ now, tid }) {
    const lease = (await this.storage.get('a'))?.lease;
    return { ok: true, live: !!lease && lease.tid === tid && now - lease.last_beat <= LEASE_TIMEOUT_MS };
  },

  async end({ now, tid }) {
    const a = await this.load(now);
    if (!a) return { ok: true, ...this.summary(blankDay(now)) };
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

  /// The record, or null when there is none. Only `create` (begin, and a bar on the current day's guest id) writes a new
  /// one: a beat, an end or a bar that finds none writes nothing, so a lease ticket still valid after its guest id's alarm
  /// (a lease opened in the last minutes of a UTC day) never brings that guest id back.
  async load(now, { create = false } = {}) {
    let a = await this.storage.get('a');
    if (!a && create) {
      a = blankDay(now);
      await this.storage.put('a', a);
      // Gone 25 hours after the day began, so nothing outlives its use.
      await this.storage.setAlarm(dayStart(now) + RETENTION_MS);
    }
    return a || null;
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

/// Whether lease `tid` is still guest `gid`'s current, beating lease. False on a deployment without guest play.
export async function leaseHeld(env, gid, tid, now) {
  if (!env.GUEST_ALLOWANCE || typeof gid !== 'string' || typeof tid !== 'string') return false;
  const r = await allowanceCall(env, gid, 'holds', { now, tid });
  return r.ok === true && r.live === true;
}

/// Beat guest `gid`'s lease `tid` on behalf of the host it is connected to: time measured here, the whole gap since the
/// last beat up to the lease timeout. True while the lease goes on; false once it has ended for any reason, and false
/// when the allowance cannot be reached or answers an error, so a guest whose time cannot be charged is let go.
export async function beatGuestLease(env, gid, tid, now) {
  if (!env.GUEST_ALLOWANCE || typeof gid !== 'string' || typeof tid !== 'string') return false;
  try {
    const r = await allowanceCall(env, gid, 'beat', { now, tid, hosted: true });
    return r?.ok === true;
  } catch (e) {
    console.error('beatGuestLease failed:', e?.message || e);
    return false;
  }
}

/// Bar the guest a refused chat line came from on listing `sim` for the rest of the UTC day it is refused on, and end its
/// lease `tid`: in the allowance of guest id `gid` (the lease's day, `gx` = that day's start + 25 h), and, when that day is
/// over (a lease opened before midnight, refused after it), in today's allowance of the same address too. True only when
/// every write succeeded: a caller that gets false still ends the guest, and retries (live.mjs `gbarq:`).
export async function barGuest(env, { gid, tid, sim, gx, now }) {
  if (!env.GUEST_ALLOWANCE || typeof gid !== 'string' || typeof sim !== 'string' || typeof gx !== 'number') return false;
  try {
    const r = await allowanceCall(env, gid, 'bar', { now, sim, tid, day: dayOf(gx - RETENTION_MS) });
    if (r?.ok !== true) return false;
    if (typeof r.next !== 'string') return true;
    const today = await allowanceCall(env, r.next, 'bar', { now, sim, tid: null, day: dayOf(now) });
    return today?.ok === true;
  } catch (e) {
    console.error('guest bar failed:', e?.message || e);
    return false;
  }
}

/// Spend one guest chat check against the day's caps (guestChatCaps): `true` when it is within every cap and was counted,
/// `false` past a cap, and `false` when the count cannot be reached (fail closed). Counts only: the listing, the host
/// account and the day, in the LiveHost object GUEST_CHAT_CAPS_NAME, deleted 25 hours after the day began.
async function spendGuestChatCheck(env, { sim, host, now }) {
  if (!env.LIVE_HOSTS) return false;
  try {
    const stub = env.LIVE_HOSTS.get(env.LIVE_HOSTS.idFromName(GUEST_CHAT_CAPS_NAME));
    const res = await stub.fetch('https://live.internal/chat_spend', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ sim, host, now, caps: guestChatCaps(env) }),
    });
    const r = await res.json();
    return r?.ok === true && r.allowed === true;
  } catch (e) {
    console.error('guest chat cap failed:', e?.message || e);
    return false;
  }
}

/// Whether `ticket` is a genuine lease ticket on simulation `simId` whose lease
/// is still the guest's current, beating one. `simId` is required: a lease on
/// one listing is never the key to another's. False for anything else,
/// including a missing ticket or listing and a deployment without guest play.
export async function leaseIsLive(env, ticket, now, simId) {
  if (!ticket || typeof simId !== 'string' || !simId || !env.GUEST_ALLOWANCE || !env.JWT_SECRET) return false;
  const lease = await verifyLease(env, ticket, now);
  if (!lease) return false;
  if (lease.s !== simId) return false;
  return leaseHeld(env, lease.g, lease.t, now);
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

/// A request's JSON body when it is an object, otherwise {}: a body of `null`, a number or an array is no body.
export async function bodyObject(request) {
  const v = await request.json().catch(() => null);
  return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
}

/// `deps` carries the index.js helpers: verifyAuth, json, isListable and bannedBearer, and live.mjs's livePin,
/// holdGuestTicket and guestSession (a call into a listing's guest-only LiveHost object).
export async function handleGuest(request, env, cors, deps) {
  const { verifyAuth, json, isListable, livePin, holdGuestTicket, bannedBearer, guestSession } = deps;
  const path = new URL(request.url).pathname;
  const now = Date.now();

  if (!env.GUEST_ALLOWANCE || !env.JWT_SECRET) {
    return json({ error: 'Guest play is not available on this deployment', code: 'guest_unavailable' }, 503, cors);
  }

  // Beat and end carry the ticket, which names the guest, so they work from
  // any address (a phone moving between networks mid-play).
  if (request.method === 'POST' && (path === '/api/guest/beat' || path === '/api/guest/end')) {
    const body = await bodyObject(request);
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

  // A join ticket for one hosted session. The lease names the guest and the simulation, so this works from any address,
  // as a beat does. Which guest and which lease the ticket was minted from are held by the session's LiveHost object
  // under the ticket id (live.mjs holdGuestTicket), never written into the ticket the host reads.
  if (request.method === 'POST' && path === '/api/guest/ticket') {
    const headers = { ...cors, 'Cache-Control': 'no-store' };
    // Hosted guest play is off unless the deployment turns it on (GUEST_PLAY.md section 13).
    if (!guestPlayOn(env) || !holdGuestTicket) {
      return json({ error: 'Joining a hosted session as a guest is not available yet. Sign in to join.', code: 'guest_join_unavailable' }, 503, headers);
    }
    const body = await bodyObject(request);
    const lease = await verifyLease(env, body.lease, now);
    if (!lease) return json({ error: 'This guest session is not valid', code: 'bad_ticket', expired: true }, 410, headers);
    if (typeof body.audience !== 'string' || !/^[0-9a-f]{64}$/.test(body.audience)) {
      return json({ error: 'audience must be the host certificate pin (64 hex characters)', code: 'bad_audience' }, 400, headers);
    }
    if (!(await leaseHeld(env, lease.g, lease.t, now))) {
      return json({ error: 'The guest session has ended', code: 'lease_ended', expired: true }, 410, headers);
    }
    // The listing is read again: one taken down, made private or re-rated since the lease opened is not joined.
    const raw = await env.SOCIAL.get(`sim:${lease.s}`);
    const sim = raw ? await simWithLegalLock(env, JSON.parse(raw)) : null;
    if (!sim || !isListable(sim)) return json({ error: 'Simulation not found' }, 404, headers);
    if (!isGuestRating(sim)) {
      return json({ error: 'This simulation needs an account', code: 'guest_not_allowed', reason: 'rating' }, 403, headers);
    }
    // Only for the guest-only session hosting that simulation now: a ticket is never minted for a pin nobody hosts it at,
    // nor for a session that takes accounts.
    const pin = livePin ? await livePin(env, lease.s, now, { guestOnly: true }) : null;
    if (!pin || pin !== body.audience) {
      return json({ error: 'This simulation is not hosted at that session right now', code: 'not_live' }, 409, headers);
    }
    // One certificate pin is one host process: when the listing's account session is live at the same pin, that process
    // would hold accounts and guests at once, so no guest ticket is made for it (live.mjs refuses the heartbeat too).
    if ((await livePin(env, lease.s, now, { guestOnly: false })) === body.audience) {
      return json({ error: 'This session also takes signed-in players, so a guest cannot join it', code: 'pin_in_use' }, 409, headers);
    }
    const { ticket, id, expires } = await signGuestJoin(env, { lease, audience: body.audience, now });
    const held = await holdGuestTicket(env, lease.s, { id, gid: lease.g, tid: lease.t, gx: guestIdExpiry(lease), expires, audience: body.audience, now });
    if (held?.ok !== true) {
      return json({ error: held?.error || 'This simulation is not hosted at that session right now', code: held?.code || 'not_live' }, held?.status || 409, headers);
    }
    return json({ ticket, id, expires, guest: true, name: GUEST_NAME }, 200, headers);
  }

  // The host of a guest-only session asks, for one chat line, whether to deliver it: a line a guest sent (`id`, the guest's
  // join ticket id), or the host's own line to its guests (`from: "host"`, no id). Every line is checked before delivery:
  // the host's own filter masks first, then the Worker's rules (guest_chat_rules.mjs: links, invites, handles, contact,
  // phone, email, street, school, place), then, within the day's caps, Jev. The host is told one of three actions and
  // nothing more (no category): `deliver`; `block` (drop the line, nobody leaves); `kick` (a guest's line refused for
  // personal information or by Jev: drop it, and the guest is ended at once, in `ended_guests` from the next heartbeat on,
  // its lease barred from this listing for the day). A refused host line is dropped and kicks nobody; the host's
  // HOST_CHAT_REFUSALS_MAX-th refusal in a UTC day ends every guest of the session (`end_all_guests: true`) and the session
  // takes no guest for the rest of that day. Fail closed: no check, no delivery. No chat text is stored or logged.
  if (request.method === 'POST' && path === '/api/guest/chat/check') {
    const headers = { ...cors, 'Cache-Control': 'no-store' };
    const hold = (status, code, error, extra = {}, more = {}) => json({ deliver: false, action: 'block', code, error, ...extra }, status, { ...headers, ...more });
    if (!guestPlayOn(env) || !guestSession) return hold(503, 'guest_join_unavailable', 'Guest chat is not available');
    const userId = await verifyAuth(request, env);
    if (!userId) return hold(401, 'unauthorized', 'Unauthorized');
    const body = await bodyObject(request);
    const fromHost = body.from === 'host';
    if (body.from !== undefined && body.from !== 'host' && body.from !== 'guest') return hold(400, 'bad_request', 'from must be "host" or "guest"');
    if (typeof body.sim_id !== 'string' || !SIM_ID.test(body.sim_id) || (!fromHost && (typeof body.id !== 'string' || !/^[0-9a-f]{32}$/.test(body.id)))) {
      return hold(400, 'bad_request', 'sim_id and id (the guest\'s join ticket id), or from: "host", are required');
    }
    if (typeof body.text !== 'string' || !body.text.trim()) return hold(400, 'bad_request', 'text required');
    if ([...body.text].length > GUEST_CHAT_MAX_CHARS) return hold(400, 'too_long', `A guest chat line is at most ${GUEST_CHAT_MAX_CHARS} characters`);
    const raw = await env.SOCIAL.get(`sim:${body.sim_id}`);
    const sim = raw ? JSON.parse(raw) : null;
    if (!sim || sim.author_id !== userId) return hold(403, 'not_author', 'Only the host of this session can check its chat');
    if (fromHost) {
      // The host's own line goes to the guests of this listing's guest-only session, which must be live now.
      const state = await guestSession(env, body.sim_id, 'host_state', { now });
      if (state?.ok !== true || state.live !== true) return hold(404, 'not_live', 'This listing has no guest-only session live now');
      if (state.barred === true) return hold(403, 'host_chat_barred', 'This session takes no guests for the rest of the day', { end_all_guests: true });
    } else {
      // The guest must be connected to this listing's guest-only session now.
      const seat = await guestSession(env, body.sim_id, 'guest_seat', { id: body.id, now });
      if (seat?.ok !== true) return hold(404, 'not_connected', 'No such guest in this session');
    }
    // Per guest (and the host's own lines on a key of their own), so one chatty guest does not hold up the rest; unbound
    // fails closed.
    if (!env.GUEST_CHAT_RATE_LIMITER) return hold(503, 'check_unavailable', 'Guest chat cannot be checked right now');
    const { success } = await env.GUEST_CHAT_RATE_LIMITER.limit({ key: `${body.sim_id}:${fromHost ? 'host' : body.id}` });
    if (!success) return hold(429, 'rate_limited', 'Too many chat lines. Wait a moment.', {}, { 'Retry-After': '60' });

    // The only records: a count per outcome in the session object, the day's cap counts, and a log line with the outcome
    // and a reason code. Never the text, the listing, the guest or the host.
    const record = async (outcome, reason) => {
      console.log(JSON.stringify({ event: 'guest_chat_check', from: fromHost ? 'host' : 'guest', outcome, reason: reason || null }));
      await guestSession(env, body.sim_id, 'chat_counted', { outcome }).catch(() => null);
    };
    // The Worker's own rules, before anything is spent. An empty line (only spaces and invisible characters) is dropped.
    const rule = guestChatRule(body.text);
    if (rule === 'empty') {
      await record('empty', 'empty');
      return hold(200, 'empty', 'This line is empty, so it was not delivered');
    }
    // Why the line is refused for personal information or by Jev's reading (a kick, for a guest's line), or null.
    let refusedFor = rule ? `rule:${rule}` : null;
    if (!refusedFor) {
      // Each check that reaches Jev is a paid call: counted against the day's caps, and refused past them (fail closed).
      if (!(await spendGuestChatCheck(env, { sim: body.sim_id, host: userId, now }))) {
        await record('capped', 'cap');
        return hold(503, 'check_unavailable', 'Chat cannot be checked right now, so this line was not delivered');
      }
      let verdict;
      try {
        // No review-cost run record for a chat line (REVIEW_METERING off for this one call): the day's cap counts are the
        // only cost record, deleted 25 hours after the day began (GUEST_PLAY.md section 16).
        verdict = await checkCommunityText({ text: body.text, field: 'guest_chat', env: { ...env, REVIEW_METERING: 'off' } });
      } catch {
        verdict = { verdict: 'unavailable', category: null };
      }
      if (verdict?.verdict === 'pass' && verdict?.adult !== true) {
        await record('deliver', null);
        return json({ deliver: true, action: 'deliver' }, 200, headers);
      }
      if (verdict?.verdict === 'refuse' && (verdict.reason === 'model' || verdict.category === 'personal_info')) refusedFor = `jev:${verdict.reason}`;
      else if (verdict?.adult === true) refusedFor = 'jev:adult';
      else if (verdict?.verdict === 'refuse') {
        // Refused for the line's shape (nothing left once cleaned, too long), not its content: dropped, nobody kicked.
        await record('empty', verdict.reason);
        return hold(200, 'empty', 'This line could not be delivered');
      } else {
        await record('unavailable', verdict?.reason || 'unavailable');
        return hold(503, 'check_unavailable', 'This line could not be checked, so it was not delivered');
      }
    }
    await record('block', refusedFor);
    if (fromHost) {
      // A refused host line is dropped and kicks nobody. At HOST_CHAT_REFUSALS_MAX in a day the session ends its guests.
      const counted = await guestSession(env, body.sim_id, 'host_chat_refused', { now, max: HOST_CHAT_REFUSALS_MAX }).catch(() => null);
      return hold(200, 'refused', 'This line was not delivered', counted?.barred === true ? { end_all_guests: true } : {});
    }
    // A guest's line refused: end the guest now, and bar it from this listing for the rest of the day. A bar that cannot be
    // written is tried twice here, then retried from the session's heartbeats, and the session refuses that guest id
    // meanwhile (live.mjs `gbarg:`, `gbarq:`).
    const kicked = await guestSession(env, body.sim_id, 'guest_kick', { id: body.id, now });
    if (kicked?.ok === true && typeof kicked.gid === 'string') {
      const args = { gid: kicked.gid, tid: kicked.tid, sim: body.sim_id, gx: kicked.gx, now };
      const barred = (await barGuest(env, args)) || (await barGuest(env, args));
      if (!barred) await guestSession(env, body.sim_id, 'bar_pending', { tid: kicked.tid, gid: kicked.gid, gx: kicked.gx, now }).catch(() => null);
    }
    return json({ deliver: false, action: 'kick', code: 'refused', end_guest: true, ended_guests: [body.id] }, 200, headers);
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
    // A banned account's own token, which verifyAuth now refuses, does not turn it into a guest. (Without the token it is
    // an address like any other: GUEST_PLAY.md section 13.)
    if (bannedBearer && (await bannedBearer(request, env))) {
      return json({ error: 'This account cannot play', code: 'account_banned' }, 403, cors);
    }
    const body = await bodyObject(request);
    if (typeof body.sim_id !== 'string' || !SIM_ID.test(body.sim_id)) return json({ error: 'sim_id required', code: 'bad_request' }, 400, cors);
    if (body.character !== 'M' && body.character !== 'F') return json({ error: 'character must be M or F', code: 'bad_request' }, 400, cors);
    // Which Player runs it: the browser Player (the default) needs a listing that runs in a browser; the desktop Player
    // runs any. The allowance and every other rule are the same for both.
    const client = body.client === undefined ? 'browser' : body.client;
    if (client !== 'browser' && client !== 'player') return json({ error: 'client must be browser or player', code: 'bad_request' }, 400, cors);

    const raw = await env.SOCIAL.get(`sim:${body.sim_id}`);
    const sim = raw ? await simWithLegalLock(env, JSON.parse(raw)) : null;
    // A listing that is not public answers like one that does not exist.
    if (!sim || !isListable(sim)) return json({ error: 'Simulation not found' }, 404, cors);
    if (!isGuestRating(sim)) {
      return json({ error: 'This simulation needs an account', code: 'guest_not_allowed', reason: 'rating' }, 403, cors);
    }
    if (client === 'browser' && sim.browser_play === false) {
      return json({ error: 'This simulation does not run in the browser', code: 'guest_not_allowed', reason: 'browser' }, 403, cors);
    }

    const tid = toHex(crypto.getRandomValues(new Uint8Array(16)));
    // The same address's guest id for the next UTC day, held in this day's record only, so a chat refusal after midnight on
    // this lease bars the address on the day it is refused (barGuest). Deleted with the record.
    const next = await guestId(env, ip, nextMidnight(now));
    const r = await allowanceCall(env, gid, 'begin', { now, sim: body.sim_id, tid, next });
    if (r.ok === false) {
      if (r.code === 'guest_barred') {
        return json({ error: 'This simulation is not available to you as a guest today. Sign in to play it.', code: 'guest_barred' }, 403, cors);
      }
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
