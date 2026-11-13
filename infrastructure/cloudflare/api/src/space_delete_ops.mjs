// =============================================================================
// Deleting a published Space: the account object's part
// =============================================================================
//
// The pending deletion (or restore), its emailed code and its attempt counter
// live in the author's AccountAuth object, which runs one operation at a time:
// five attempts cannot become fifty in parallel, and a code cannot be spent
// twice by two requests that race. KV could not promise either.
//
// The object never sees a code. The Worker draws it, mails it, and hands the
// object only its HMAC digest (src/space_delete.mjs deleteCodeDigest); at
// confirm the Worker digests what the person typed and the object compares the
// two in constant time. The object also holds the account's email state, so it
// alone decides which address may receive a code: the verified address, and
// only once it has been verified for thirty days (email_ops.mjs emailView).
//
// State, inside `state.space_delete`:
//   pending: { [sim_id]: { request_id, action, sim_id, name_sha256, session_sha256, code_digest,
//                          requested_at, expires_at, attempts, max_attempts, status, confirmed_at, applied_at, ended_at } }
//   log:     { 'request:{sim_id}': [ms, ...], 'request': [ms, ...] }   sliding windows for the rate limits
// One pending entry per listing; a new request replaces it, which makes the old code useless. An entry that has ended is
// dropped a day later; the audit chain keeps the history. Never a code, never an address.
//
// These are operations of the AccountAuth object (`this` is the object), spread
// into its OPS the way emailOps is. Design: docs/architecture/DELETE_PUBLISHED_SPACE.md.
// =============================================================================

import { emailView, cleanAddress } from './email_ops.mjs';

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/// A code works for this long after it is sent.
export const DELETE_CODE_TTL_MS = 15 * MINUTE;
/// Wrong answers allowed per request; the fifth wrong one locks it.
export const DELETE_CODE_MAX_ATTEMPTS = 5;
/// Requests (delete and restore together) per listing per hour, and per account per day.
export const DELETE_REQUESTS_PER_LISTING_PER_HOUR = 3;
export const DELETE_REQUESTS_PER_ACCOUNT_PER_DAY = 5;
/// An entry that has ended (confirmed, expired, locked, cancelled) is kept this long, for a confirm that is retried.
const ENDED_KEPT_MS = DAY;

const SIM_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const REQUEST_ID = /^sdr_[A-Za-z0-9]{24}$/;
const HEX64 = /^[0-9a-f]{64}$/;
const ACTIONS = ['delete', 'restore'];

/// Equal digests, compared in time that does not depend on where they differ. Both are 64 hex characters.
export function digestsEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || !HEX64.test(a) || !HEX64.test(b)) return false;
  let diff = 0;
  for (let i = 0; i < 64; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/// The state's deletion part, created on first use, with what has run out moved on: a pending entry past its expiry is
/// expired, an ended entry past a day is dropped, and log windows older than a day are forgotten.
function spaceDeleteOf(state, now) {
  const sd = state.space_delete || (state.space_delete = { pending: {}, log: {} });
  for (const [simId, e] of Object.entries(sd.pending)) {
    if (e.status === 'pending' && now >= e.expires_at) { e.status = 'expired'; e.ended_at = e.expires_at; }
    const ended = e.status === 'confirmed' ? (e.applied_at || e.confirmed_at) : e.ended_at;
    if (e.status !== 'pending' && Number.isFinite(ended) && ended + ENDED_KEPT_MS <= now) delete sd.pending[simId];
  }
  for (const [kind, times] of Object.entries(sd.log)) {
    const kept = (Array.isArray(times) ? times : []).filter((t) => t > now - DAY);
    if (kept.length) sd.log[kind] = kept; else delete sd.log[kind];
  }
  return sd;
}

/// Times in `log[kind]` inside the window.
const inWindow = (log, kind, windowMs, now) => (log[kind] || []).filter((t) => t > now - windowMs);

/// What the owner may see of an entry: never its digests.
function entryView(e, now) {
  return {
    request_id: e.request_id,
    action: e.action,
    status: e.status === 'pending' && now >= e.expires_at ? 'expired' : e.status,
    requested_at: new Date(e.requested_at).toISOString(),
    expires_at: new Date(e.expires_at).toISOString(),
    attempts_left: Math.max(0, e.max_attempts - e.attempts),
  };
}

export function spaceDeleteOps(h) {
  const { fail } = h;

  return {
    /// Open a pending deletion or restore. The Worker has checked the listing, its holds and its money; this checks the
    /// address and the rate limits, keeps the code's digest, and names the one address the code may go to.
    async space_delete_begin(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const { sim_id: simId, action, request_id: requestId, code_digest: codeDigest, name_sha256: nameSha, session_sha256: sessionSha } = args;
      if (typeof simId !== 'string' || !SIM_ID.test(simId) || !ACTIONS.includes(action) || typeof requestId !== 'string' || !REQUEST_ID.test(requestId)
        || !HEX64.test(String(codeDigest)) || !HEX64.test(String(nameSha)) || !HEX64.test(String(sessionSha))) {
        return fail(400, 'bad_request', 'bad space deletion request');
      }
      const sd = spaceDeleteOf(state, this.now);
      // Only the verified address, and only once it has been verified for thirty days: never a pending, replaced or
      // unverified one. Checked before any allowance is spent.
      const view = emailView(state, this.now);
      const address = view.verified && view.aged ? cleanAddress(view.on_file) : null;
      if (!address) {
        await this.storage.put('state', state);
        return fail(409, 'no_verified_email', 'no verified email address old enough to receive a code');
      }
      const perListing = inWindow(sd.log, `request:${simId}`, HOUR, this.now);
      const perAccount = inWindow(sd.log, 'request', DAY, this.now);
      if (perListing.length >= DELETE_REQUESTS_PER_LISTING_PER_HOUR || perAccount.length >= DELETE_REQUESTS_PER_ACCOUNT_PER_DAY) {
        const until = Math.max(
          perListing.length >= DELETE_REQUESTS_PER_LISTING_PER_HOUR ? Math.min(...perListing) + HOUR : 0,
          perAccount.length >= DELETE_REQUESTS_PER_ACCOUNT_PER_DAY ? Math.min(...perAccount) + DAY : 0,
        );
        await this.storage.put('state', state);
        return { ok: false, status: 429, code: 'rate_limited', error: 'too many deletion requests', retry_after: Math.max(1, Math.ceil((until - this.now) / 1000)) };
      }
      sd.log[`request:${simId}`] = [...perListing, this.now];
      sd.log.request = [...perAccount, this.now];
      // One entry per listing: a new request replaces the old one, whose code is then useless.
      const entry = {
        request_id: requestId, action, sim_id: simId, name_sha256: nameSha, session_sha256: sessionSha, code_digest: codeDigest,
        requested_at: this.now, expires_at: this.now + DELETE_CODE_TTL_MS, attempts: 0, max_attempts: DELETE_CODE_MAX_ATTEMPTS,
        status: 'pending', confirmed_at: null, applied_at: null, ended_at: null,
      };
      const replaced = sd.pending[simId]?.status === 'pending' ? sd.pending[simId].request_id : null;
      sd.pending[simId] = entry;
      await this.commit(state, 'space_delete_requested', { sim: simId, action, request: requestId, factor: 'email', ...(replaced ? { replaced } : {}) });
      return { ok: true, request_id: requestId, action, expires_at: entry.expires_at, recipient: address, attempts_left: DELETE_CODE_MAX_ATTEMPTS };
    },

    /// Check a code. The attempt is counted and saved before anything else is checked; the fifth wrong answer locks the
    /// request. A request confirmed once answers the same confirm again as a replay (so a retry after a dropped reply
    /// finishes the work) and anything else as used: the code works once.
    async space_delete_confirm(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const sd = spaceDeleteOf(state, this.now);
      const e = typeof args.sim_id === 'string' ? sd.pending[args.sim_id] : null;
      if (!e) { await this.storage.put('state', state); return fail(400, 'request_invalid', 'no such request'); }
      const same = e.request_id === args.request_id && e.action === args.action && e.session_sha256 === args.session_sha256;
      if (e.status === 'confirmed') {
        if (same && digestsEqual(e.code_digest, args.code_digest)) {
          return { ok: true, replay: true, request_id: e.request_id, action: e.action, confirmed_at: e.confirmed_at };
        }
        return fail(409, 'request_used', 'this request was already confirmed');
      }
      if (e.status === 'locked') return { ok: false, status: 409, code: 'request_locked', error: 'too many wrong codes', attempts_left: 0 };
      if (e.status === 'expired') { await this.storage.put('state', state); return fail(410, 'request_expired', 'this code has expired'); }
      if (e.status !== 'pending') return fail(400, 'request_invalid', 'no such request');

      e.attempts += 1;
      await this.storage.put('state', state);
      const left = Math.max(0, e.max_attempts - e.attempts);
      const refuse = async (status, code, message) => {
        if (e.attempts >= e.max_attempts) {
          e.status = 'locked';
          e.ended_at = this.now;
          await this.commit(state, 'space_delete_locked', { sim: e.sim_id, action: e.action, request: e.request_id });
          return { ok: false, status, code, error: message, attempts_left: 0, locked: true };
        }
        return { ok: false, status, code, error: message, attempts_left: left };
      };
      if (this.now >= e.expires_at) {
        e.status = 'expired';
        e.ended_at = e.expires_at;
        await this.storage.put('state', state);
        return fail(410, 'request_expired', 'this code has expired');
      }
      if (!same) return refuse(400, 'request_invalid', 'this is not the open request for this listing');
      if (!HEX64.test(String(args.name_sha256)) || args.name_sha256 !== e.name_sha256) return refuse(409, 'name_changed', 'the listing was renamed since the code was sent');
      if (!digestsEqual(e.code_digest, args.code_digest)) return refuse(401, 'code_invalid', 'wrong code');

      e.status = 'confirmed';
      e.confirmed_at = this.now;
      await this.commit(state, 'space_delete_confirmed', { sim: e.sim_id, action: e.action, request: e.request_id, attempts: e.attempts });
      return { ok: true, request_id: e.request_id, action: e.action, confirmed_at: e.confirmed_at };
    },

    /// The Worker finished the soft delete or the restore a confirmed request asked for. Recorded once.
    async space_delete_applied(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const sd = spaceDeleteOf(state, this.now);
      const e = typeof args.sim_id === 'string' ? sd.pending[args.sim_id] : null;
      if (!e || e.request_id !== args.request_id || e.status !== 'confirmed') return fail(409, 'request_invalid', 'no confirmed request to apply');
      if (e.applied_at) return { ok: true, already: true };
      e.applied_at = this.now;
      await this.commit(state, e.action === 'delete' ? 'space_soft_deleted' : 'space_restored', {
        sim: e.sim_id, request: e.request_id, ...(e.action === 'delete' && typeof args.hard_delete_after === 'string' ? { hard_delete_after: args.hard_delete_after } : {}),
      });
      return { ok: true };
    },

    /// The code could not be sent: the request ends, and the owner asks again.
    async space_delete_cancel(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const sd = spaceDeleteOf(state, this.now);
      const e = typeof args.sim_id === 'string' ? sd.pending[args.sim_id] : null;
      if (!e || e.request_id !== args.request_id || e.status !== 'pending') return { ok: true, already: true };
      e.status = 'cancelled';
      e.ended_at = this.now;
      await this.commit(state, 'space_delete_cancelled', { sim: e.sim_id, request: e.request_id, reason: typeof args.reason === 'string' ? args.reason.slice(0, 40) : 'cancelled' });
      return { ok: true };
    },

    /// The open request for a listing, as the owner may see it, or null.
    async space_delete_status(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = typeof args.sim_id === 'string' ? state.space_delete?.pending?.[args.sim_id] : null;
      return { ok: true, pending: e ? entryView(e, this.now) : null };
    },

    /// The hourly job removed the listing for good.
    async space_hard_deleted(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      if (typeof args.sim_id !== 'string' || !SIM_ID.test(args.sim_id)) return fail(400, 'bad_request', 'sim_id required');
      await this.commit(state, 'space_hard_deleted', {
        sim: args.sim_id, objects: Number.isFinite(args.objects) ? args.objects : null, bytes: Number.isFinite(args.bytes) ? args.bytes : null,
      });
      return { ok: true };
    },
  };
}
