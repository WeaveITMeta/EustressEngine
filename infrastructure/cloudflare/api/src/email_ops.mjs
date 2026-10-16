// =============================================================================
// An account's email: verified, changed, and who is told
// =============================================================================
//
// The owner's "no" to a key change has to travel by a channel a thief who holds
// the key file does not hold. So the address is not whoever types first:
//
//   1. An account with an address on file gets its first verification link ONLY
//      at that address, never at one typed into a request.
//   2. Adding or changing the address takes the CURRENT key's signature, tells
//      every existing address with a cancel link, and waits seven days.
//   3. A verified address counts as verified once it is thirty days old; a
//      younger one gets notices but is not accepted as a recovery factor.
//   4. Every key notice goes to ALL of: the verified address, a pending address,
//      the address on file, and for thirty days any address that was replaced.
//      Never "or": a thief's address becoming verified must not displace the
//      owner's as a recipient.
//
// These are operations of the AccountAuth object (they run one at a time, with
// its state and audit chain), kept in their own file. `emailOps(h)` takes the
// object's helpers and returns the operations; `this` is the object.
// Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified email".
// =============================================================================

const HOUR = 3600 * 1000;
const DAY = 24 * HOUR;
export const VERIFY_LINK_MS = DAY;
/// How long an expired verification link is remembered, only to tell its owner it expired.
export const EXPIRED_LINK_KEPT_MS = 14 * DAY;
export const EMAIL_CHANGE_WAIT_MS = 7 * DAY;
/// A change that was requested and then not confirmed lapses this long after it
/// could have taken effect.
export const EMAIL_CHANGE_WINDOW_MS = 7 * DAY;
export const VERIFIED_AGE_MS = 30 * DAY;
export const REPLACED_KEEP_MS = 30 * DAY;
const VERIFY_STARTS_PER_HOUR = 3;
const CHANGE_REQUESTS_PER_DAY = 5;

const HEX64 = /^[0-9a-f]{64}$/;
const HEX_SIGNATURE = /^[0-9a-f]{128}$/;

/// A single plain address, lower case, or null: no display name, no list, no
/// control character. It goes into a `To:` header and the envelope.
const ADDRESS = /^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]{1,64}@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+$/;
export function cleanAddress(value) {
  if (typeof value !== 'string') return null;
  const v = value.trim();
  if (v.length > 254 || !ADDRESS.test(v)) return null;
  return v.toLowerCase();
}

/// What the current key signs to ask for an address change. The address is a
/// single lower-case address with no line break, so it cannot make the message
/// ambiguous.
export function emailChangeMessage({ origin, accountId, key, address, challenge }) {
  return ['eustress-email-change-v1', origin, accountId, key, address, challenge].join('\n');
}

/// The account's email state, created on first use, with what has expired
/// dropped: used-up or old tokens, replaced addresses past their thirty days, a
/// change that was never confirmed.
function emailOf(state, now) {
  const e = state.email || (state.email = { verified: null, pending: null, replaced: [], tokens: {}, log: {} });
  for (const [digest, t] of Object.entries(e.tokens)) {
    // A verification link that ran out is kept for a while, never usable, so the page can say "expired".
    const kept = t.purpose === 'verify' && t.exp + EXPIRED_LINK_KEPT_MS > now;
    if (t.exp <= now && !kept) delete e.tokens[digest];
  }
  e.replaced = e.replaced.filter((r) => r.until > now);
  if (e.pending && e.pending.effective_at + EMAIL_CHANGE_WINDOW_MS <= now) {
    delete e.tokens[e.pending.cancel_digest];
    e.pending = null;
  }
  return e;
}

/// Every address a key notice goes to. One list, one rule, for every notice.
export function noticeSet(state, e) {
  const out = new Set();
  for (const a of [e?.verified?.address, e?.pending?.address, state.email_on_file, ...(e?.replaced || []).map((r) => r.address)]) {
    const clean = cleanAddress(a);
    if (clean) out.add(clean);
  }
  return [...out];
}

/// The email facts the owner sees, without changing anything.
export function emailView(state, now) {
  const e = state.email || {};
  const onFile = cleanAddress(state.email_on_file);
  const verified = e.verified && e.verified.address === onFile ? e.verified : null;
  const pending = e.pending && e.pending.effective_at + EMAIL_CHANGE_WINDOW_MS > now ? e.pending : null;
  const verifiedAt = verified ? Date.parse(verified.verified_at) : null;
  return {
    on_file: onFile,
    verified: !!verified,
    verified_at: verified ? verified.verified_at : null,
    // Accepted as a recovery factor only once it has been verified this long.
    aged: !!verified && now - verifiedAt >= VERIFIED_AGE_MS,
    aged_at: verified ? new Date(verifiedAt + VERIFIED_AGE_MS).toISOString() : null,
    pending: pending ? {
      address: pending.address,
      requested_at: pending.requested_at,
      effective_at: new Date(pending.effective_at).toISOString(),
      ready: now >= pending.effective_at,
    } : null,
  };
}

function allow(e, kind, max, windowMs, now) {
  const log = (e.log[kind] || []).filter((t) => t > now - windowMs);
  e.log[kind] = log;
  if (log.length >= max) return false;
  log.push(now);
  return true;
}

const iso = (ms) => new Date(ms).toISOString();

/// A short stable name for an address in the audit: an HMAC with a server secret,
/// so a known address cannot be checked against a leaked audit by hashing it.
async function addressPrint(env, address) {
  const secret = env.AUDIT_PRINT_KEY || env.JWT_SECRET || 'eustress-audit-print';
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(`eustress-address:${address}`)));
  return [...mac].map((b) => b.toString(16).padStart(2, '0')).join('').slice(0, 16);
}

/// The address an account's mail goes to: the one its object keeps in
/// `email:{id}` once it has changed or verified one, otherwise the one on the user
/// record. Nothing writes the address into the user record: a read-modify-write
/// of it can undo a ban that lands meanwhile.
export async function readAccountEmail(env, user) {
  if (user?.id && env?.USERS) {
    try {
      const raw = await env.USERS.get(`email:${user.id}`);
      if (raw) {
        const address = cleanAddress(JSON.parse(raw).address);
        if (address) return address;
      }
    } catch (e) {
      console.error('account email record unreadable, using the user record:', e?.message || e);
    }
  }
  return typeof user?.email === 'string' ? user.email : null;
}

export function emailOps(h) {
  const { fail, sha256Hex, hexOf, verifyEd25519 } = h;

  /// A short stable name for an address in the audit: the start of its SHA-256.
  const newToken = () => hexOf(crypto.getRandomValues(new Uint8Array(32)));

  return {
    async email_status(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      return { ok: true, email: emailView(state, this.now) };
    },

    /// Every address a notice goes to now, for a caller that sends one.
    async notice_recipients(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      return { ok: true, recipients: noticeSet(state, emailOf(state, this.now)) };
    },

    /// The first verification link, to the address ON FILE and no other. The
    /// token is for the Worker to put in a link; the object keeps its digest.
    async email_verify_start(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = emailOf(state, this.now);
      const address = cleanAddress(state.email_on_file);
      if (!address) return fail(409, 'no_email_on_file', 'this account has no email address on file');
      if (e.verified && e.verified.address === address) return { ok: true, already_verified: true, verified_at: e.verified.verified_at };
      if (!allow(e, 'verify_start', VERIFY_STARTS_PER_HOUR, HOUR, this.now)) {
        await this.storage.put('state', state);
        return fail(429, 'rate_limited', 'too many verification emails');
      }
      // One link at a time: asking again replaces the earlier one.
      for (const [digest, t] of Object.entries(e.tokens)) if (t.purpose === 'verify') delete e.tokens[digest];
      const token = newToken();
      e.tokens[await sha256Hex(token)] = { purpose: 'verify', address, exp: this.now + VERIFY_LINK_MS };
      await this.commit(state, 'email_verify_requested', { address: await addressPrint(this.env, address) });
      return { ok: true, token: `${state.account_id}.${token}`, address, expires_at: iso(this.now + VERIFY_LINK_MS) };
    },

    /// The link was opened: the address on file is now verified, from today. The
    /// token is spent by the first use, and only a verification token counts.
    async email_verify_confirm(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = emailOf(state, this.now);
      const digest = typeof args.token === 'string' && HEX64.test(args.token) ? await sha256Hex(args.token) : null;
      const t = digest ? e.tokens[digest] : null;
      if (!t || t.purpose !== 'verify') return fail(400, 'token_invalid', 'this link is not valid');
      // Nothing is spent or written: the same link says "expired" again until it is forgotten.
      if (t.exp <= this.now) return fail(410, 'token_expired', 'this link has expired');
      delete e.tokens[digest];
      // The address may have changed since the link was sent.
      if (t.address !== cleanAddress(state.email_on_file)) {
        await this.storage.put('state', state);
        return fail(400, 'token_invalid', 'this link is not valid');
      }
      e.verified = { address: t.address, verified_at: iso(this.now) };
      await this.commit(state, 'email_verified', { address: await addressPrint(this.env, t.address) }, [
        { type: 'put_if_user', key: `email:${state.account_id}`, value: JSON.stringify({ address: t.address, verified_at: e.verified.verified_at }) },
      ]);
      return { ok: true, verified_at: e.verified.verified_at, aged_at: iso(this.now + VERIFIED_AGE_MS), address: t.address };
    },

    /// A one-time challenge for asking to change the address.
    async email_change_begin(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key');
      // Not limited here: a session without the key could use the day's allowance
      // up by asking. Challenges are bounded already (five open, five minutes).
      const challenge = await this.newChallenge(state, 'email_change');
      await this.storage.put('state', state);
      return { ok: true, challenge, public_key: state.public_key, expires_at: iso(this.now + 5 * 60 * 1000) };
    },

    /// Ask for a new address. The current key signs it; nothing changes for seven
    /// days; every existing address is told, with a link that cancels it.
    async email_change_request(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = emailOf(state, this.now);
      if (!(await this.takeChallenge(state, args.challenge, 'email_change'))) return fail(400, 'challenge_invalid', 'no such challenge');
      const address = cleanAddress(args.new_address);
      if (!address) return fail(400, 'bad_address', 'that is not an email address');
      if (typeof args.origin !== 'string' || !args.origin) return fail(400, 'bad_request', 'origin required');
      const message = emailChangeMessage({ origin: args.origin, accountId: state.account_id, key: state.public_key, address, challenge: args.challenge });
      const signature = typeof args.signature === 'string' ? args.signature.toLowerCase() : '';
      if (!HEX_SIGNATURE.test(signature) || !(await verifyEd25519(state.public_key, signature, message))) {
        return fail(401, 'signature_invalid', 'signature verification failed');
      }
      if (address === cleanAddress(state.email_on_file) || address === e.verified?.address || address === e.pending?.address) {
        return fail(409, 'same_address', 'that is already the address');
      }

      // Limited where a request has succeeded, which takes the current key.
      if (!allow(e, 'change', CHANGE_REQUESTS_PER_DAY, DAY, this.now)) {
        await this.storage.put('state', state);
        return fail(429, 'rate_limited', 'too many address changes today');
      }
      // The people to warn are the ones the account has now, before this one joins.
      const existing = noticeSet(state, e);
      if (e.pending) delete e.tokens[e.pending.cancel_digest];
      const cancel = newToken();
      const cancelDigest = await sha256Hex(cancel);
      const effective = this.now + EMAIL_CHANGE_WAIT_MS;
      e.tokens[cancelDigest] = { purpose: 'cancel', exp: effective + EMAIL_CHANGE_WINDOW_MS };
      e.pending = { address, requested_at: iso(this.now), effective_at: effective, cancel_digest: cancelDigest };
      await this.commit(state, 'email_change_requested', { to: await addressPrint(this.env, address), effective_at: iso(effective), warned: existing.length });
      return {
        ok: true,
        cancel_token: `${state.account_id}.${cancel}`,
        pending: emailView(state, this.now).pending,
        notify: { existing, pending_address: address },
      };
    },

    /// Stop a pending change: by the link in the notice (`token`), which also
    /// ends every sign-in because it means "this was not me", or from a signed-in
    /// session (`by_session`).
    async email_change_cancel(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = emailOf(state, this.now);
      let viaLink = false;
      if (typeof args.token === 'string') {
        const digest = HEX64.test(args.token) ? await sha256Hex(args.token) : null;
        const t = digest ? e.tokens[digest] : null;
        if (!t || t.purpose !== 'cancel' || !e.pending || e.pending.cancel_digest !== digest) return fail(400, 'token_invalid', 'this link is not valid');
        viaLink = true;
      } else if (args.by_session !== true) {
        return fail(400, 'bad_request', 'a link or a session is needed');
      }
      if (!e.pending) return fail(404, 'no_pending', 'there is no address change to cancel');
      const cancelled = e.pending;
      // Told: the addresses the account had, not the one that was asked for.
      const existing = noticeSet(state, e).filter((a) => a !== cancelled.address);
      delete e.tokens[cancelled.cancel_digest];
      e.pending = null;
      const mirror = [];
      let floor;
      if (viaLink) {
        floor = Math.max(Math.floor(this.now / 1000), state.floor) + 1;
        state.floor = floor;
        mirror.push({ type: 'put', key: `authfloor:${state.account_id}`, value: String(floor), ttl: 80 * 3600 });
      }
      await this.commit(state, 'email_change_cancelled', { to: await addressPrint(this.env, cancelled.address), by: viaLink ? 'link' : 'session' }, mirror);
      return { ok: true, cancelled: true, sessions_ended: viaLink, floor, notify: { existing, cancelled_address: cancelled.address } };
    },

    /// After the wait, make the new address the one on file. It starts
    /// unverified; the old addresses stay on the notice list for thirty days.
    async email_change_apply(args) {
      const { state, error } = await this.load(args);
      if (error) return error;
      const e = emailOf(state, this.now);
      const p = e.pending;
      if (!p) return fail(404, 'no_pending', 'there is no address change to apply');
      if (this.now < p.effective_at) {
        return { ok: false, status: 409, code: 'not_yet', error: 'the waiting period has not passed', effective_at: iso(p.effective_at) };
      }
      const before = noticeSet(state, e);
      const from = cleanAddress(state.email_on_file);
      for (const old of new Set([from, e.verified?.address].filter(Boolean))) {
        if (old === p.address) continue;
        e.replaced = e.replaced.filter((r) => r.address !== old);
        e.replaced.push({ address: old, until: this.now + REPLACED_KEEP_MS });
      }
      delete e.tokens[p.cancel_digest];
      state.email_on_file = p.address;
      e.verified = null;
      e.pending = null;
      await this.commit(state, 'email_changed', { to: await addressPrint(this.env, p.address), from: from ? await addressPrint(this.env, from) : null }, [
        { type: 'put_if_user', key: `email:${state.account_id}`, value: JSON.stringify({ address: p.address, verified_at: null }) },
      ]);
      return { ok: true, address: p.address, notify: { recipients: before }, replaced_until: iso(this.now + REPLACED_KEEP_MS) };
    },
  };
}

/// A pending address change requested under a key that has since been replaced
/// was authorised by that key, so a key rotation drops it. Returns whether one
/// was dropped.
export function dropPendingChange(state, now) {
  const e = emailOf(state, now);
  if (!e.pending) return false;
  delete e.tokens[e.pending.cancel_digest];
  e.pending = null;
  return true;
}
