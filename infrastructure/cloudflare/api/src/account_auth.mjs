// =============================================================================
// AccountAuth: one Durable Object per account, the authority for its key
// =============================================================================
//
// The current identity key, the keys it replaced, the session floor, the value
// freeze and the audit of every change live here, in one object that handles one
// request at a time. KV keeps a mirror for reads that may lag by about a minute
// (`user:{id}.public_key`, `pubkey:{key}`, `retired-key:{key}`, `authfloor:{id}`),
// written from here through an outbox so a failed write is retried and never
// leaves a new key unresolvable. Design, and why: docs/architecture/
// IDENTITY_KEY_ROTATION_AND_RECOVERY.md.
//
// Operations (POST https://account.internal/{op}, JSON in, JSON out):
//
//   state            what the object knows, or { seeded: false }
//   seed             first touch: take the account's key, floor and address from KV
//   check_login      is this key the account's current key? a retired one says so
//   bump_floor       end every token issued so far (sign out, ban, deletion)
//   begin_rotation   a one-time challenge for replacing the key
//   mark_prompt_seen the one-time prompt to replace the key has been shown
//   unlink_discord   the owner unlinked Discord while replacing the key
//   ack_notice       the route sent the notice of a key change
//   rotate           replace the key: two signatures, one step
//   admin_replace_key an administrator replaces the key after a manual identity check
//   begin_admin_action, verify_admin_action   the administrator signs it with their own key
//   restore_key      the link in the notice puts the earlier key back
//   ack_revoked      the route revoked credentials a replacement owes
//   resolve_review   an administrator closes a review flag
//   email_*          the verified address and changing it (src/email_ops.mjs)
//   notice_recipients every address a key notice goes to
//   audit            the newest audit records (the hash chain)
//   repair_mirror    run the outbox again
//   passkey_list, begin_passkey, passkey_add, passkey_remove, passkey_login
//                    the account's passkeys (src/passkeys.mjs): second keys that
//                    sign in to the website; adding or removing one needs the
//                    identity key's signature
//   passkeys_clear   the account is being deleted: every passkey goes
//
// The Worker is the only caller; nothing here is reachable from the internet.
// =============================================================================

import { emailOps, emailView, noticeSet, dropPendingChange } from './email_ops.mjs';
import { renderKeyNotice, sendNotice } from './account_notices.mjs';
import { accountTimeZone } from './account_time_zone.mjs';
import { hubStub, callObject } from './commerce.mjs';
import { siteOrigin } from './cors.mjs';
import { PASSKEY_ALGS, fromBase64url, verifyPasskeySignature, signedData } from './webauthn.mjs';
import { passkeyIndexCall } from './passkey_index.mjs';

const CHALLENGE_TTL_MS = 5 * 60 * 1000;
const MAX_OPEN_CHALLENGES = 5;
/// After a rotation, value is frozen for this long: longer when the account holds
/// value, because a thief with a leaked file who rotates first should not be able
/// to cash out before the owner has noticed.
export const FREEZE_WITH_VALUE_MS = 7 * 24 * 3600 * 1000;
export const FREEZE_WITHOUT_VALUE_MS = 72 * 3600 * 1000;
/// The payout destination stays pinned this long after a key change.
export const PAYOUT_PIN_MS = 30 * 24 * 3600 * 1000;

/// What replacing a key costs, in whole days, for the page that asks the owner to do it: the
/// pause on spending, transfers and payouts (longer when the account holds value) and the
/// time the payout bank account stays locked. Read from the constants the object enforces,
/// so the words on the page cannot drift from the rule.
export function keyReplaceTerms() {
  const day = 24 * 3600 * 1000;
  return { pause_days: FREEZE_WITHOUT_VALUE_MS / day, pause_days_with_value: FREEZE_WITH_VALUE_MS / day, payout_pin_days: PAYOUT_PIN_MS / day };
}
const MIRROR_RETRY_MS = 30 * 1000;
/// One KV write may take this long before it counts as failed and stays owed. The
/// object does one thing at a time, so a write that hangs holds up every
/// operation on the account, sign-in included.
export const MIRROR_WRITE_TIMEOUT_MS = 5000;
/// A caller of the object waits this long before answering "unavailable" (the
/// Worker's sign-in then says 503 instead of waiting on a stuck account).
export const CALL_TIMEOUT_MS = 8000;
/// Rotations allowed per account per day.
const ROTATIONS_PER_DAY = 3;
/// A key change answers its caller after at most this long spent on KV writes; what
/// is not done by then stays owed to the alarm. It is under CALL_TIMEOUT_MS, so a
/// stuck KV cannot make the caller give up on a change that has already happened.
export const INLINE_FLUSH_BUDGET_MS = 3000;
/// The route that made a change has this long to send its notice and say so, before
/// the object's own alarm sends it.
const NOTICE_GRACE_MS = 60 * 1000;
const NOTICE_MAX_ATTEMPTS = 6;
/// A revocation that keeps failing is tried this many times, with growing waits, then flagged for an administrator.
const REVOKE_MAX_ATTEMPTS = 8;
/// Administrator replacements one administrator may make in a day.
const ADMIN_ACTIONS_PER_DAY = 5;
/// The owner's link to put the earlier key back works at least this long.
const UNDO_MIN_MS = 7 * 24 * 3600 * 1000;

/// `promise`, or a rejection after `ms`. The timer is cleared when the promise settles.
function withTimeout(promise, ms, label) {
  let timer;
  const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label} timed out after ${ms} ms`)), ms); });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}
const AUDIT_PAGE = 50;
/// Passkeys one account may hold.
export const MAX_PASSKEYS = 10;
/// A passkey sign-in challenge is remembered this long after it signed in, so the
/// same signed answer cannot sign in twice even if two requests raced past the KV
/// copy of the challenge. Only a sign-in whose signature verified is remembered,
/// and an entry is never dropped before it expires: past the most there may be,
/// sign-in answers 429 until some expire.
const PASSKEY_LOGIN_SEEN_MS = CHALLENGE_TTL_MS + 60 * 1000;
export const PASSKEY_LOGIN_SEEN_MAX = 200;

/// The twelve encodings of the Ed25519 points of small order (the eight
/// canonical ones and the four non-canonical spellings). A key that is one of
/// them verifies signatures for almost any message, so it is never an account's
/// key.
export const SMALL_ORDER_KEYS = new Set([
  '0100000000000000000000000000000000000000000000000000000000000000',
  'ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  '0000000000000000000000000000000000000000000000000000000000000000',
  '0000000000000000000000000000000000000000000000000000000000000080',
  'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a',
  'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa',
  '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05',
  '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85',
  'edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  'edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
  'eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  'eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
]);

const HEX64 = /^[0-9a-f]{64}$/;
const HEX_SIGNATURE = /^[0-9a-f]{128}$/;

/// The key as the account stores it (lower case), or null when it is not 64 hex.
export function normalKey(value) {
  return typeof value === 'string' && /^[0-9a-fA-F]{64}$/.test(value) ? value.toLowerCase() : null;
}

/// What both signatures of a rotation cover. Every field is hex or a UUID, so
/// none can hold a newline and make the message ambiguous.
export function rotationMessage({ origin, accountId, oldKey, newKey, challenge }) {
  return ['eustress-key-rotation-v1', origin, accountId, oldKey, newKey, challenge].join('\n');
}

/// What an administrator signs, with their own identity key, to replace another
/// account's key. Every field is hex, a UUID or a case reference, so none can hold a newline.
export function adminReplaceMessage({ origin, adminId, accountId, newKey, caseRef, challenge }) {
  return ['eustress-admin-replace-v1', origin, adminId, accountId, newKey, caseRef, challenge].join('\n');
}

/// What the holder of the NEW key signs, to show the administrator did not paste a key
/// nobody holds. It names the account by username (a locked-out owner knows no id) and the case.
export function adminReplaceKeyMessage({ origin, username, caseRef, newKey }) {
  return ['eustress-admin-replace-key-v1', origin, username, caseRef, newKey].join('\n');
}

/// What the identity key signs to add a passkey, and to remove one. Every field
/// is an origin, a UUID, base64url or hex, so none can hold a newline.
export function passkeyAddMessage({ origin, accountId, credentialId, challenge }) {
  return ['eustress-passkey-add-v1', origin, accountId, credentialId, challenge].join('\n');
}
export function passkeyRemoveMessage({ origin, accountId, credentialId, challenge }) {
  return ['eustress-passkey-remove-v1', origin, accountId, credentialId, challenge].join('\n');
}

/// What the owner sees of a passkey.
export const publicPasskey = (p) => ({ id: p.id, name: p.name, created_at: p.created_at, last_used_at: p.last_used_at || null, alg: p.alg, backed_up: !!p.backed_up });

const CREDENTIAL_ID = /^[A-Za-z0-9_-]{22,1364}$/;
const passkeyPrint = async (id) => (await sha256Hex(`eustress-passkey:${id}`)).slice(0, 16);

const enc = new TextEncoder();

function hexOf(bytes) {
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
function bytesOf(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}
export async function sha256Hex(text) {
  return hexOf(await crypto.subtle.digest('SHA-256', enc.encode(text)));
}

/// A short, stable name for a key in notices and the audit: the start of its
/// SHA-256, never the key.
export async function fingerprint(key) {
  return (await sha256Hex(`eustress-key:${key}`)).slice(0, 16);
}

async function verifyEd25519(publicKeyHex, signatureHex, message) {
  if (!HEX64.test(publicKeyHex) || !HEX_SIGNATURE.test(signatureHex)) return false;
  try {
    const key = await crypto.subtle.importKey('raw', bytesOf(publicKeyHex), { name: 'Ed25519' }, false, ['verify']);
    return await crypto.subtle.verify('Ed25519', key, bytesOf(signatureHex), enc.encode(message));
  } catch {
    return false;
  }
}

/// JSON with its keys in a fixed order, so a record hashes the same wherever it
/// is rebuilt.
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

export async function auditHash(prev, record) {
  return sha256Hex(`${prev || ''}|${canonical({ n: record.n, at: record.at, type: record.type, data: record.data })}`);
}

/// Whether a list of audit records is an unbroken chain, oldest first. Each
/// record holds the hash of the one before it.
export async function verifyAuditChain(records) {
  let prev = records.length ? records[0].prev : null;
  for (const r of records) {
    if (r.prev !== prev) return false;
    if ((await auditHash(r.prev, r)) !== r.hash) return false;
    prev = r.hash;
  }
  return true;
}

const credentialsOwed = (state) => !!state.credentials_pending && (state.credentials_pending.api_keys.length + state.credentials_pending.webhooks.length) > 0;
/// Something the object has to do for itself later: a write to retry, a notice to send, a credential to revoke.
const owes = (state) => state.mirror_pending.length > 0 || (state.notices_pending || []).length > 0 || credentialsOwed(state);

const auditKey = (n) => `audit:${String(n).padStart(10, '0')}`;

const fail = (status, code, error) => ({ ok: false, status, code, error });

// -----------------------------------------------------------------------------
// The object
// -----------------------------------------------------------------------------

export class AccountAuth {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
    // One operation at a time, whole: a Durable Object lets another request in
    // whenever an operation awaits something that is not storage (a signature
    // check, a KV write), and a key change must not interleave with another.
    this.chain = Promise.resolve();
  }

  fetch(request) {
    const run = this.chain.then(() => this.handle(request));
    this.chain = run.catch(() => {});
    return run;
  }

  async handle(request) {
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
      const handler = OPS[op];
      result = handler ? await handler.call(this, args) : fail(404, 'unknown_operation', `unknown operation ${op}`);
    } catch (e) {
      console.error(`AccountAuth.${op} failed:`, e);
      result = fail(500, 'internal', 'internal error');
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  /// The outbox: retry the KV writes still owed. It runs in the same chain as
  /// every operation, and reads the state inside the chain: an alarm that read
  /// the state, waited on KV, and saved its copy would undo an operation that
  /// landed while it waited.
  alarm() {
    const run = this.chain.then(() => this.retryOutbox());
    this.chain = run.catch(() => {});
    return run;
  }

  async retryOutbox() {
    this.now = Date.now();
    const state = await this.storage.get('state');
    if (!state) return;
    if (!owes(state)) return;
    await this.flushMirror(state);
    await this.deliverNotices(state);
    await this.revokePending(state);
    await this.storage.put('state', state);
    await this.armAlarm(state);
  }

  /// The next time the object has something to do for itself: a write to retry
  /// or a notice to send. Nothing owed, no alarm.
  async armAlarm(state) {
    const notices = state.notices_pending || [];
    if (notices.length) {
      const soonest = Math.min(...notices.map((n) => n.due_at));
      await this.storage.setAlarm(Math.max(Date.now() + 1000, soonest));
    } else if (credentialsOwed(state) && !state.mirror_pending.length) {
      await this.storage.setAlarm(Math.max(Date.now() + 1000, state.credentials_pending.next_at || 0));
    } else if (state.mirror_pending.length || credentialsOwed(state)) {
      await this.storage.setAlarm(Date.now() + MIRROR_RETRY_MS);
    }
  }

  /// Revoke the API keys and webhook endpoints a replacement owes, through the
  /// account's commerce hub. What is done leaves the list; what fails stays for
  /// the next alarm. Never throws.
  async revokePending(state) {
    const c = state.credentials_pending;
    if (!c) return;
    if (!this.env.COMMERCE_HUBS) { state.credentials_pending = null; return; }
    if ((c.next_at || 0) > Date.now()) return;
    const hub = hubStub(this.env, state.account_id);
    const revoke = async (op, id) => {
      try {
        const r = await withTimeout(callObject(hub, op, { account_id: state.account_id, id }), 5000, 'revoke');
        return r.ok === true || r.status === 404;
      } catch (e) {
        console.error('AccountAuth revoke failed:', op, e?.message || e);
        return false;
      }
    };
    const keys = [];
    for (const id of c.api_keys) if (!(await revoke('revoke_key', id))) keys.push(id);
    const hooks = [];
    for (const id of c.webhooks) if (!(await revoke('delete_endpoint', id))) hooks.push(id);
    c.api_keys = keys;
    c.webhooks = hooks;
    if (!credentialsOwed(state)) { state.credentials_pending = null; return; }
    c.attempts = (c.attempts || 0) + 1;
    if (c.attempts >= REVOKE_MAX_ATTEMPTS) {
      // Give up retrying and say so where an administrator will see it.
      const left = { api_keys: c.api_keys.length, webhooks: c.webhooks.length };
      state.credentials_pending = null;
      state.admin_review_flags = [...(state.admin_review_flags || []), { id: String(state.audit_seq + 1), at: new Date().toISOString(), reason: 'credentials_not_revoked', case_ref: null, admin_id: null, left }];
      await this.commit(state, 'revocation_abandoned', left, [
        { type: 'put', key: `admin-review:${state.account_id}`, value: JSON.stringify(state.admin_review_flags) },
      ]);
      return;
    }
    c.next_at = Date.now() + Math.min(2 ** c.attempts * 30 * 1000, 3600 * 1000);
  }

  /// Send the notices of key changes that their route did not acknowledge, once
  /// they are due. A recipient that got it is dropped; one that did not is tried
  /// again later, up to six times. Never throws.
  async deliverNotices(state) {
    const keep = [];
    const abandoned = [];
    for (const n of state.notices_pending || []) {
      if (n.due_at > Date.now()) { keep.push(n); continue; }
      let results;
      try {
        results = await withTimeout(sendNotice(this.env, n.recipients, renderKeyNotice(n.kind, { ...n.data, timeZone: await accountTimeZone(this.env, state.account_id) }), { accountId: state.account_id }), 20000, 'notice');
      } catch (e) {
        console.error('AccountAuth notice failed:', e?.message || e);
        results = n.recipients.map(() => ({ sent: false, reason: 'send_failed' }));
      }
      // Nothing more can be done for a recipient the deployment cannot mail or
      // whose allowance is spent; only a failed send is worth another try.
      const remaining = n.recipients.filter((_, i) => !(results[i]?.sent || results[i]?.reason === 'no_mail_binding' || results[i]?.reason === 'daily_cap'));
      if (!remaining.length) continue;
      n.attempts = (n.attempts || 0) + 1;
      if (n.attempts >= NOTICE_MAX_ATTEMPTS) { abandoned.push(n.id); continue; }
      n.recipients = remaining;
      n.due_at = Date.now() + Math.min(2 ** n.attempts * 60 * 1000, 3600 * 1000);
      keep.push(n);
    }
    state.notices_pending = keep;
    if (abandoned.length) console.error('AccountAuth gave up on notices:', abandoned.join(','));
  }

  /// Apply every write in `state.mirror_pending` and drop the ones that landed.
  /// A write that fails stays, and an alarm tries again. Never throws.
  /// `budgetMs` caps the time spent in all: what is not done by then stays owed.
  async flushMirror(state, budgetMs = Infinity) {
    const keep = [];
    const perWrite = Number(this.env.MIRROR_WRITE_TIMEOUT_MS) || MIRROR_WRITE_TIMEOUT_MS;
    const deadline = Date.now() + budgetMs;
    for (const op of state.mirror_pending) {
      const left = deadline - Date.now();
      if (left <= 0) { keep.push(op); continue; }
      try {
        await withTimeout(this.applyMirror(state.account_id, op), Math.min(perWrite, left), 'mirror write');
      } catch (e) {
        console.error('AccountAuth mirror write failed:', op.type, op.key || '', e?.message || e);
        keep.push(op);
      }
    }
    state.mirror_pending = keep;
  }

  async applyMirror(accountId, op) {
    if (op.type === 'passkey_release') {
      // Only this account's claim is released (src/passkey_index.mjs). A deployment
      // without the index has nothing to release.
      const r = await passkeyIndexCall(this.env, op.credential_id, 'release', { account_id: accountId });
      if (!r.ok && r.code !== 'no_binding') throw new Error(`passkey release: ${r.code}`);
      return;
    }
    const users = this.env.USERS;
    if (!users) return; // a deployment without the KV namespace has nothing to mirror to
    if (op.type === 'put' && op.key.startsWith('authfloor:')) {
      // A retried older write must not lower a floor something newer raised.
      const held = Number(await users.get(op.key)) || 0;
      if (held > Number(op.value)) return;
      await users.put(op.key, op.value, op.ttl ? { expirationTtl: op.ttl } : undefined);
    } else if (op.type === 'put') await users.put(op.key, op.value, op.ttl ? { expirationTtl: op.ttl } : undefined);
    else if (op.type === 'delete') await users.delete(op.key);
    else if (op.type === 'put_if_user') {
      // A key kept for an account that exists: a retry after the account was
      // deleted must not bring a deleted account's data back.
      if (!(await users.get(`user:${accountId}`))) return;
      await users.put(op.key, op.value);
    }
  }

  /// Save the state, with an audit record chained to the last one, in one write,
  /// then try the outbox and arm the alarm for what is left.
  async commit(state, auditType, auditData, mirror = []) {
    // Only the newest floor write is worth keeping.
    if (mirror.some((op) => op.key?.startsWith('authfloor:'))) {
      state.mirror_pending = state.mirror_pending.filter((op) => !op.key?.startsWith('authfloor:'));
    }
    state.mirror_pending.push(...mirror);
    const records = {};
    if (auditType) {
      const n = state.audit_seq + 1;
      const record = { n, at: new Date(this.now).toISOString(), type: auditType, data: auditData || {}, prev: state.audit_head };
      record.hash = await auditHash(record.prev, record);
      records[auditKey(n)] = record;
      state.audit_seq = n;
      state.audit_head = record.hash;
    }
    // The retry is armed before anything is written: a failure at any point after
    // it leaves an alarm behind, not an outbox nobody looks at. An alarm that
    // finds nothing owed does nothing.
    await this.armAlarm(state);
    // The state and its audit record land together or not at all.
    await this.storage.put({ state, ...records });
    await this.flushMirror(state, Number(this.env.MIRROR_INLINE_BUDGET_MS) || INLINE_FLUSH_BUDGET_MS);
    await this.storage.put('state', state);
  }

  /// A one-time challenge for `purpose`, kept as a digest. Expired ones are
  /// dropped and at most five stay open: the oldest goes first.
  async newChallenge(state, purpose) {
    for (const [digest, c] of Object.entries(state.challenges)) if (c.exp <= this.now) delete state.challenges[digest];
    const open = Object.entries(state.challenges).sort((a, b) => a[1].exp - b[1].exp);
    while (open.length >= MAX_OPEN_CHALLENGES) delete state.challenges[open.shift()[0]];
    const challenge = hexOf(crypto.getRandomValues(new Uint8Array(32)));
    state.challenges[await sha256Hex(challenge)] = { purpose, exp: this.now + CHALLENGE_TTL_MS };
    return challenge;
  }

  /// Spend a challenge: true only when it is open, unexpired and was made for
  /// `purpose`. The first attempt spends it, right or wrong.
  async takeChallenge(state, challenge, purpose) {
    const digest = typeof challenge === 'string' && HEX64.test(challenge) ? await sha256Hex(challenge) : null;
    const open = digest ? state.challenges[digest] : null;
    if (open) {
      delete state.challenges[digest];
      await this.storage.put('state', state);
    }
    return !!open && open.purpose === purpose && open.exp > this.now;
  }

  /// Replace the account's key: retire the current one, make `newKey` current, end
  /// every session, freeze value (longer when the account holds some), pin the
  /// payout destination, drop an address change the old key asked for, and write
  /// the audit record and the KV mirror. One function for a rotation and for an
  /// administrator's replacement, so the two cannot differ.
  async applyKeyChange(state, { newKey, how, holdsValue, auditType, auditExtra = {}, username = '', revoke = null, removePasskeys = [] }) {
    // Everyone the account has told about, before anything is dropped.
    const recipients = noticeSet(state, state.email);
    const oldKey = state.public_key;
    const oldPrint = await fingerprint(oldKey);
    const newPrint = await fingerprint(newKey);
    const nowSec = Math.floor(this.now / 1000);
    state.retired.push({ key: oldKey, fingerprint: oldPrint, at: new Date(this.now).toISOString(), how });
    state.public_key = newKey;
    state.legacy_key = null;
    state.key_epoch += 1;
    state.key_changed_at = new Date(this.now).toISOString();
    state.floor = Math.max(nowSec, state.floor) + 1;
    state.value_frozen_until = Math.max(state.value_frozen_until, this.now + (holdsValue ? FREEZE_WITH_VALUE_MS : FREEZE_WITHOUT_VALUE_MS));
    state.payout_pinned_until = Math.max(state.payout_pinned_until, this.now + PAYOUT_PIN_MS);
    // An address change the old key asked for was authorised by that key.
    const droppedEmailChange = dropPendingChange(state, this.now);

    // The notice is recorded with the change, in the same put. The route that made
    // the change sends it and says so (ack_notice); if it does not, or cannot, the
    // alarm sends it after a minute. A change nobody was told about is not a
    // change the owner can answer.
    if (username) state.username = String(username).slice(0, 64);
    // An administrator's replacement can be undone by whoever holds the notice: a
    // link, valid for the length of the freeze and at least a week, that puts the
    // earlier key back. A support person who was deceived must not be able to lock
    // the real owner out for good.
    let undoLink = null;
    let undoUntil = null;
    if (how === 'admin') {
      const secret = hexOf(crypto.getRandomValues(new Uint8Array(32)));
      undoUntil = Math.max(state.value_frozen_until, this.now + UNDO_MIN_MS);
      state.admin_undo = {
        digest: await sha256Hex(secret), old_key: oldKey, new_key: newKey, until: undoUntil,
        case_ref: auditExtra.case_ref || null, admin_id: auditExtra.admin_id || null, at: state.key_changed_at,
      };
      undoLink = `${siteOrigin(this.env)}/undo-key-replacement?token=${state.account_id}.${secret}`;
    }
    // What the replacement revokes is owed, in the same put: the route tries it at
    // once, and the alarm does what is left.
    if (revoke) {
      state.credentials_pending = { api_keys: [...(revoke.api_keys || [])], webhooks: [...(revoke.webhooks || [])], attempts: 0, next_at: this.now };
      if (revoke.discord && !state.discord_unlinked_at) state.discord_unlinked_at = state.key_changed_at;
      if (!credentialsOwed(state)) state.credentials_pending = null;
    }

    // Passkeys the owner chose to remove (or, for an administrator's replacement,
    // every one), in the same put as the key.
    const removed = this.removePasskeys(state, removePasskeys === 'all' ? (state.passkeys || []).map((p) => p.id) : removePasskeys);

    const noticeId = String(state.audit_seq + 1);
    state.notices_pending = state.notices_pending || [];
    state.notices_pending.push({
      id: noticeId,
      kind: how === 'admin' ? 'admin_replacement' : 'rotation',
      due_at: this.now + NOTICE_GRACE_MS,
      attempts: 0,
      recipients,
      data: {
        username: String(username || '').slice(0, 64), at: state.key_changed_at, oldFingerprint: oldPrint, newFingerprint: newPrint,
        valueFrozenUntil: state.value_frozen_until, payoutPinnedUntil: state.payout_pinned_until,
        caseRef: auditExtra.case_ref || null, revoked: null, undoLink, undoUntil,
      },
    });

    await this.commit(state, auditType, {
      from: oldPrint, to: newPrint, epoch: state.key_epoch, holds_value: holdsValue, dropped_email_change: droppedEmailChange, ...auditExtra,
      value_frozen_until: state.value_frozen_until, payout_pinned_until: state.payout_pinned_until,
      ...(removed.length ? { passkeys_removed: removed.length } : {}),
    }, [
      ...removed.map((id) => ({ type: 'passkey_release', credential_id: id })),
      { type: 'put', key: `pubkey:${newKey}`, value: state.account_id },
      { type: 'put', key: `retired-key:${oldKey}`, value: state.account_id },
      { type: 'delete', key: `pubkey:${oldKey}` },
      { type: 'put', key: `authfloor:${state.account_id}`, value: String(state.floor), ttl: 80 * 3600 },
      // For code that moves value with no session (the payout run): when the freeze ends.
      { type: 'put', key: `valuefreeze:${state.account_id}`, value: String(state.value_frozen_until), ttl: Math.ceil((state.value_frozen_until - this.now) / 1000) + 3600 },
    ]);

    return {
      ok: true, floor: state.floor, key_epoch: state.key_epoch, rotated_at: state.key_changed_at,
      value_frozen_until: state.value_frozen_until, payout_pinned_until: state.payout_pinned_until,
      old_key_fingerprint: oldPrint, new_key_fingerprint: newPrint,
      recipients, dropped_email_change: droppedEmailChange, notice_id: noticeId,
      undo_link: undoLink, undo_until: undoUntil ? new Date(undoUntil).toISOString() : null,
      credentials_pending: credentialsOwed(state) ? state.credentials_pending.api_keys.length + state.credentials_pending.webhooks.length : 0,
      passkeys_removed: removed,
    };
  }

  /// Take the passkeys named in `ids` off the account; returns the ids it held.
  /// The caller writes the state and deletes their KV index entries.
  removePasskeys(state, ids) {
    const wanted = new Set((Array.isArray(ids) ? ids : []).filter((id) => typeof id === 'string'));
    const held = state.passkeys || [];
    const removed = held.filter((p) => wanted.has(p.id)).map((p) => p.id);
    if (removed.length) state.passkeys = held.filter((p) => !wanted.has(p.id));
    return removed;
  }

  /// Record a notice about a passkey for every address, in the state, so the
  /// alarm sends it if the route does not.
  passkeyNotice(state, kind, data) {
    const noticeId = String(state.audit_seq + 1);
    const recipients = noticeSet(state, state.email);
    state.notices_pending = state.notices_pending || [];
    state.notices_pending.push({ id: noticeId, kind, due_at: this.now + NOTICE_GRACE_MS, attempts: 0, recipients, data });
    return { noticeId, recipients };
  }

  /// The state, or an error result when the account is unknown or the caller
  /// named another account (a request routed to the wrong object).
  async load(args) {
    this.now = Number.isFinite(args.now) ? args.now : Date.now();
    const state = await this.storage.get('state');
    if (!state) return { error: fail(409, 'not_seeded', 'this account has not been seeded') };
    // Writes owed and no alarm to retry them (an alarm that was lost): arm one.
    if (owes(state) && (await this.storage.getAlarm()) == null) {
      await this.armAlarm(state);
    }
    if (typeof args.account_id !== 'string' || args.account_id !== state.account_id) {
      return { error: fail(400, 'wrong_account', 'wrong account') };
    }
    return { state };
  }
}

// -----------------------------------------------------------------------------
// Operations
// -----------------------------------------------------------------------------

function publicState(state, now) {
  const email = emailView(state, now);
  return {
    seeded: true,
    account_id: state.account_id,
    public_key: state.public_key,
    key_epoch: state.key_epoch,
    key_changed_at: state.key_changed_at,
    prompt_seen_at: state.prompt_seen_at || null,
    retired: state.retired.map((r) => ({ key_fingerprint: r.fingerprint, at: r.at, how: r.how })),
    floor: state.floor,
    value_frozen_until: state.value_frozen_until,
    payout_pinned_until: state.payout_pinned_until,
    audit_seq: state.audit_seq,
    audit_head: state.audit_head,
    mirror_pending: state.mirror_pending.length,
    notices_pending: (state.notices_pending || []).length,
    credentials_pending: credentialsOwed(state) ? state.credentials_pending.api_keys.length + state.credentials_pending.webhooks.length : 0,
    admin_review: state.admin_review_flags || [],
    admin_undo_until: state.admin_undo ? state.admin_undo.until : null,
    discord_unlinked_at: state.discord_unlinked_at || null,
    email_verified_at: email.verified_at,
    email_aged: email.aged,
    email_change_pending: email.pending,
  };
}

const OPS = {
  ...emailOps({ fail, sha256Hex, hexOf, verifyEd25519 }),

  async state(args) {
    this.now = Date.now();
    const state = await this.storage.get('state');
    if (!state) return { ok: true, seeded: false };
    if (typeof args.account_id === 'string' && args.account_id !== state.account_id) return fail(400, 'wrong_account', 'wrong account');
    return { ok: true, ...publicState(state, this.now) };
  },

  /// First touch of an account: take what KV holds. Seeding twice changes
  /// nothing, so two requests that both find the object empty are safe.
  async seed(args) {
    this.now = Number.isFinite(args.now) ? args.now : Date.now();
    const existing = await this.storage.get('state');
    if (existing) return { ok: true, ...publicState(existing, this.now), created: false };
    if (typeof args.account_id !== 'string' || !args.account_id) return fail(400, 'bad_request', 'account_id required');
    const key = normalKey(args.public_key);
    const state = {
      v: 1,
      account_id: args.account_id,
      seeded_at: new Date(this.now).toISOString(),
      // An account filed under a key that is not 64 hex (it could never sign in)
      // has no current key until it is replaced.
      public_key: key,
      legacy_key: key ? null : String(args.public_key || ''),
      key_epoch: 0,
      key_changed_at: null,
      retired: [],
      floor: Number.isFinite(args.floor) && args.floor > 0 ? Math.floor(args.floor) : 0,
      value_frozen_until: 0,
      payout_pinned_until: 0,
      email_on_file: typeof args.email === 'string' && args.email ? args.email : null,
      challenges: {},
      audit_head: null,
      audit_seq: 0,
      mirror_pending: [],
    };
    await this.commit(state, 'seeded', { key_fingerprint: key ? await fingerprint(key) : null, floor: state.floor });
    return { ok: true, ...publicState(state, this.now), created: true };
  },

  /// Is `public_key` the key this account signs in with? A retired key gets its
  /// own answer, with the date, so a client can say the file was replaced.
  async check_login(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const key = normalKey(args.public_key);
    if (!key) return fail(400, 'bad_key', 'not a public key');
    if (state.public_key && key === state.public_key) return { ok: true, key_epoch: state.key_epoch, floor: state.floor };
    const old = state.retired.find((r) => r.key === key);
    if (old) return { ok: false, status: 403, code: 'key_retired', error: 'this identity key was replaced', retired_at: old.at, how: old.how };
    return fail(403, 'key_not_current', 'not the key of this account');
  },

  /// Every token issued so far ends: the floor moves one second past the later of
  /// now and the current floor, which is past every iat that can exist (a token
  /// is stamped no later than the later of now and the floor).
  async bump_floor(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const floor = Math.max(Math.floor(this.now / 1000), state.floor) + 1;
    state.floor = floor;
    await this.commit(state, 'sessions_ended', { reason: String(args.reason || 'unspecified').slice(0, 40), floor }, [
      { type: 'put', key: `authfloor:${state.account_id}`, value: String(floor), ttl: 80 * 3600 },
    ]);
    return { ok: true, floor };
  },

  /// A one-time challenge for replacing the key, kept as a digest.
  async begin_rotation(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key to replace');
    const challenge = await this.newChallenge(state, 'rotation');
    await this.storage.put('state', state);
    return { ok: true, challenge, public_key: state.public_key, expires_at: new Date(this.now + CHALLENGE_TTL_MS).toISOString() };
  },

  /// Replace the key. The old key signs (a stolen token alone cannot take the
  /// account), the new key signs (it is held), the challenge is one-time and
  /// bound to this account and this purpose, and nothing changes unless every
  /// check passes. One step: the key, the retired list, the floor, the freeze
  /// and the audit record land together.
  async rotate(args) {
    const { state, error } = await this.load(args);
    if (error) return error;

    // The challenge is spent by the first attempt, right or wrong.
    if (!(await this.takeChallenge(state, args.challenge, 'rotation'))) return fail(400, 'challenge_invalid', 'no such challenge');

    const newKey = normalKey(args.new_public_key);
    if (!newKey) return fail(400, 'key_unacceptable', 'the new key is not a public key');
    if (SMALL_ORDER_KEYS.has(newKey)) return fail(400, 'key_unacceptable', 'the new key is not acceptable');
    if (newKey === state.public_key || state.retired.some((r) => r.key === newKey)) {
      return fail(409, 'key_unacceptable', 'that key is already or was already this account\'s key');
    }
    if (typeof args.origin !== 'string' || !args.origin) return fail(400, 'bad_request', 'origin required');

    const message = rotationMessage({ origin: args.origin, accountId: state.account_id, oldKey: state.public_key, newKey, challenge: args.challenge });
    const oldSignature = typeof args.old_signature === 'string' ? args.old_signature.toLowerCase() : '';
    const newSignature = typeof args.new_signature === 'string' ? args.new_signature.toLowerCase() : '';
    const oldOk = await verifyEd25519(state.public_key, oldSignature, message);
    const newOk = await verifyEd25519(newKey, newSignature, message);
    if (!oldOk || !newOk) return fail(401, 'signature_invalid', 'signature verification failed');

    // Rotations per day: counted at the point where the signatures are good, so
    // someone with only a session cannot use the allowance up.
    state.rotation_log = (state.rotation_log || []).filter((t) => t > this.now - 24 * 3600 * 1000);
    if (state.rotation_log.length >= ROTATIONS_PER_DAY) return fail(429, 'rate_limited', 'too many key replacements today');
    state.rotation_log.push(this.now);

    return this.applyKeyChange(state, {
      newKey, how: 'rotation', holdsValue: args.holds_value === true, auditType: 'key_rotated', username: args.username,
      removePasskeys: Array.isArray(args.revoke_passkeys) ? args.revoke_passkeys : [],
    });
  },

  /// An administrator replaces the key after a manual identity check (a person
  /// who lost their key and cannot sign in). There is no signature to check: the
  /// check was made by a person, and the record says which person, which case
  /// and why. The new key is held by the owner, who made it on their own
  /// computer and sent only its public half. Everything else is a rotation's:
  /// the sessions end, value is frozen, the payout destination is pinned, and
  /// every address is told.
  async admin_replace_key(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const adminId = typeof args.admin_id === 'string' && /^[A-Za-z0-9_-]{8,64}$/.test(args.admin_id) ? args.admin_id : null;
    const caseRef = typeof args.case_ref === 'string' && /^[A-Za-z0-9_.:-]{3,80}$/.test(args.case_ref) ? args.case_ref : null;
    const reason = typeof args.reason === 'string' ? args.reason.trim() : '';
    if (!adminId) return fail(400, 'bad_request', 'admin_id required');
    if (!caseRef) return fail(400, 'bad_request', 'case_ref required: 3 to 80 of A-Z a-z 0-9 _ . : -');
    if (reason.length < 10 || reason.length > 200 || /[\r\n]/.test(reason)) return fail(400, 'bad_request', 'reason required: 10 to 200 characters on one line');
    const newKey = normalKey(args.new_public_key);
    if (!newKey || SMALL_ORDER_KEYS.has(newKey)) return fail(400, 'key_unacceptable', 'the new key is not acceptable');
    if (newKey === state.public_key || state.retired.some((r) => r.key === newKey)) {
      return fail(409, 'key_unacceptable', 'that key is already or was already this account\'s key');
    }
    // The sender of the new key holds it: it signs the case and the username. A
    // mistyped, truncated or invented key would retire the old one for good.
    const username = typeof args.username === 'string' ? args.username : '';
    if (!username || /[\r\n]/.test(username) || typeof args.origin !== 'string' || !args.origin) return fail(400, 'bad_request', 'username and origin required');
    const keySignature = typeof args.new_key_signature === 'string' ? args.new_key_signature.toLowerCase() : '';
    if (!HEX_SIGNATURE.test(keySignature) || !(await verifyEd25519(newKey, keySignature, adminReplaceKeyMessage({ origin: args.origin, username, caseRef, newKey })))) {
      return fail(401, 'new_key_signature_invalid', 'the new key did not sign for this case');
    }
    // While the owner's undo link is open, no second replacement: it would replace the
    // link, and the new link would put back the key the FIRST replacement installed,
    // which may be an attacker's.
    if (state.admin_undo && this.now < state.admin_undo.until) {
      return { ok: false, status: 409, code: 'replacement_in_window', error: 'a replacement of this account is still open to undo', until: new Date(state.admin_undo.until).toISOString() };
    }
    // Everything has been checked; a dry run stops here, so the route can tell the
    // second channel before anything changes.
    if (args.dry_run === true) return { ok: true, dry_run: true };
    return this.applyKeyChange(state, {
      newKey, how: 'admin', holdsValue: args.holds_value === true, auditType: 'key_replaced_by_admin',
      auditExtra: { admin_id: adminId, case_ref: caseRef, reason }, username,
      revoke: args.revoke && typeof args.revoke === 'object' ? args.revoke : null,
      // A passkey acts as the account as an API key does: every one goes, as they do.
      removePasskeys: 'all',
    });
  },

  /// An administrator asks to replace a key: a one-time challenge for them to
  /// sign with their OWN identity key. This runs in the administrator's object.
  async begin_admin_action(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key');
    const challenge = await this.newChallenge(state, 'admin_replace');
    await this.storage.put('state', state);
    return { ok: true, challenge, public_key: state.public_key, expires_at: new Date(this.now + CHALLENGE_TTL_MS).toISOString() };
  },

  /// The administrator's signature over `message`, checked against their own
  /// current key, once; and at most five replacements a day (counted here, where a
  /// signature is good, so a stolen session without the key uses none).
  async verify_admin_action(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!(await this.takeChallenge(state, args.challenge, 'admin_replace'))) return fail(400, 'challenge_invalid', 'no such challenge');
    const signature = typeof args.signature === 'string' ? args.signature.toLowerCase() : '';
    if (typeof args.message !== 'string' || !HEX_SIGNATURE.test(signature) || !(await verifyEd25519(state.public_key, signature, args.message))) {
      return fail(401, 'signature_invalid', 'signature verification failed');
    }
    state.admin_actions = (state.admin_actions || []).filter((t) => t > this.now - 24 * 3600 * 1000);
    if (state.admin_actions.length >= ADMIN_ACTIONS_PER_DAY) return fail(429, 'rate_limited', 'too many key replacements today');
    state.admin_actions.push(this.now);
    await this.storage.put('state', state);
    return { ok: true };
  },

  /// The link in the notice of an administrator's replacement: put the key it
  /// replaced back, retire whatever is current as disputed, end every session, keep
  /// value frozen and the payout pinned, and mark the case for a person to review.
  /// Works once, until the window closes.
  async restore_key(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const u = state.admin_undo;
    const digest = typeof args.token === 'string' && HEX64.test(args.token) ? await sha256Hex(args.token) : null;
    if (!u || !digest || digest !== u.digest || this.now >= u.until) return fail(400, 'token_invalid', 'this link is not valid');
    const recipients = noticeSet(state, state.email);
    const disputed = state.public_key;
    const nowIso = new Date(this.now).toISOString();
    state.retired = state.retired.filter((r) => r.key !== u.old_key);
    if (disputed) state.retired.push({ key: disputed, fingerprint: await fingerprint(disputed), at: nowIso, how: 'disputed' });
    state.public_key = u.old_key;
    state.key_epoch += 1;
    state.key_changed_at = nowIso;
    state.floor = Math.max(Math.floor(this.now / 1000), state.floor) + 1;
    state.value_frozen_until = Math.max(state.value_frozen_until, this.now + FREEZE_WITH_VALUE_MS);
    state.payout_pinned_until = Math.max(state.payout_pinned_until, this.now + PAYOUT_PIN_MS);
    // A flag is added to the list, never put in the place of one already there.
    const noticeId = String(state.audit_seq + 1);
    state.admin_review_flags = [...(state.admin_review_flags || []), { id: noticeId, at: nowIso, reason: 'replacement_undone', case_ref: u.case_ref, admin_id: u.admin_id }];
    state.admin_undo = null;
    // An administrator's replacement removed every passkey, so any the account
    // holds now were added with the disputed key: they go with it.
    const passkeysRemoved = this.removePasskeys(state, (state.passkeys || []).map((p) => p.id));
    state.notices_pending = state.notices_pending || [];
    state.notices_pending.push({
      id: noticeId, kind: 'key_restored', due_at: this.now + NOTICE_GRACE_MS, attempts: 0, recipients,
      data: { username: state.username || '', at: nowIso, replacedAt: u.at, valueFrozenUntil: state.value_frozen_until },
    });
    await this.commit(state, 'key_restored_after_admin_replacement', {
      epoch: state.key_epoch, case_ref: u.case_ref, admin_id: u.admin_id, disputed: disputed ? await fingerprint(disputed) : null,
      ...(passkeysRemoved.length ? { passkeys_removed: passkeysRemoved.length } : {}),
    }, [
      ...passkeysRemoved.map((id) => ({ type: 'passkey_release', credential_id: id })),
      { type: 'put', key: `pubkey:${u.old_key}`, value: state.account_id },
      ...(disputed ? [{ type: 'delete', key: `pubkey:${disputed}` }, { type: 'put', key: `retired-key:${disputed}`, value: state.account_id }] : []),
      { type: 'delete', key: `retired-key:${u.old_key}` },
      { type: 'put', key: `authfloor:${state.account_id}`, value: String(state.floor), ttl: 80 * 3600 },
      { type: 'put', key: `valuefreeze:${state.account_id}`, value: String(state.value_frozen_until), ttl: Math.ceil((state.value_frozen_until - this.now) / 1000) + 3600 },
      { type: 'put', key: `admin-review:${state.account_id}`, value: JSON.stringify(state.admin_review_flags) },
    ]);
    return {
      ok: true, key_epoch: state.key_epoch, floor: state.floor, recipients, notice_id: noticeId,
      replaced_at: u.at, restored_at: nowIso, value_frozen_until: state.value_frozen_until, username: state.username || '',
    };
  },

  /// An administrator has looked at an undone replacement (or an abandoned
  /// revocation) and closes ONE flag, named by its id (the only flag needs no id).
  /// The note and the administrator are kept. Other flags stay.
  async resolve_review(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const flags = state.admin_review_flags || [];
    if (!flags.length) return fail(404, 'no_flag', 'nothing to resolve');
    const note = typeof args.note === 'string' ? args.note.trim() : '';
    if (note.length < 5 || note.length > 200 || /[\r\n]/.test(note)) return fail(400, 'bad_request', 'note required: 5 to 200 characters on one line');
    const adminId = typeof args.admin_id === 'string' && /^[A-Za-z0-9_-]{8,64}$/.test(args.admin_id) ? args.admin_id : null;
    if (!adminId) return fail(400, 'bad_request', 'admin_id required');
    let flag;
    if (args.flag_id === undefined || args.flag_id === null) {
      if (flags.length > 1) return fail(409, 'flag_id_required', 'this account has more than one flag: name the one to resolve');
      flag = flags[0];
    } else {
      flag = flags.find((x) => x.id === String(args.flag_id));
      if (!flag) return fail(404, 'no_flag', 'no such flag');
    }
    state.admin_review_flags = flags.filter((x) => x !== flag);
    await this.commit(state, 'review_resolved', { admin_id: adminId, note, flag_id: flag.id, flagged_at: flag.at, reason: flag.reason }, [
      state.admin_review_flags.length
        ? { type: 'put', key: `admin-review:${state.account_id}`, value: JSON.stringify(state.admin_review_flags) }
        : { type: 'delete', key: `admin-review:${state.account_id}` },
    ]);
    return { ok: true, remaining: state.admin_review_flags.length };
  },

  /// The route revoked these credentials: they leave the list the object owes.
  async ack_revoked(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const c = state.credentials_pending;
    if (c) {
      const done = (list) => new Set(Array.isArray(list) ? list.map(String) : []);
      const keys = done(args.api_keys);
      const hooks = done(args.webhooks);
      c.api_keys = c.api_keys.filter((id) => !keys.has(id));
      c.webhooks = c.webhooks.filter((id) => !hooks.has(id));
      if (!credentialsOwed(state)) state.credentials_pending = null;
      await this.storage.put('state', state);
    }
    return { ok: true, pending: credentialsOwed(state) ? state.credentials_pending.api_keys.length + state.credentials_pending.webhooks.length : 0 };
  },

  /// The route sent the notice of a key change to `delivered`: those recipients are
  /// done. When none are left the notice is done. Idempotent.
  async ack_notice(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const delivered = new Set((Array.isArray(args.delivered) ? args.delivered : []).map((a) => String(a).toLowerCase()));
    const notices = state.notices_pending || [];
    const n = notices.find((x) => x.id === String(args.id));
    if (n) {
      n.recipients = n.recipients.filter((a) => !delivered.has(a));
      if (!n.recipients.length) state.notices_pending = notices.filter((x) => x !== n);
      await this.storage.put('state', state);
    }
    return { ok: true, pending: (state.notices_pending || []).length };
  },

  /// The owner unlinked Discord when replacing the key. The link itself lives in
  /// the user record; this is the account's word on it, so no write to that
  /// record is needed (a read-modify-write of it could undo a ban that landed
  /// meanwhile). A reader that shows the link treats it as gone when this is set.
  async unlink_discord(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!state.discord_unlinked_at) {
      state.discord_unlinked_at = new Date(this.now).toISOString();
      await this.commit(state, 'discord_unlinked', {});
    }
    return { ok: true, discord_unlinked_at: state.discord_unlinked_at };
  },

  /// The one-time "replace your key" prompt has been shown to this account.
  async mark_prompt_seen(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!state.prompt_seen_at) {
      state.prompt_seen_at = new Date(this.now).toISOString();
      await this.commit(state, 'prompt_seen', {});
    }
    return { ok: true, prompt_seen_at: state.prompt_seen_at };
  },

  async audit(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const limit = Math.min(Math.max(Math.floor(args.limit) || 20, 1), AUDIT_PAGE);
    const from = Math.max(1, state.audit_seq - limit + 1);
    const records = [];
    for (let n = from; n <= state.audit_seq; n++) {
      const r = await this.storage.get(auditKey(n));
      if (r) records.push(r);
    }
    return { ok: true, records, head: state.audit_head, seq: state.audit_seq };
  },

  // ── Passkeys ──────────────────────────────────────────────────────────────

  async passkey_list(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    return { ok: true, passkeys: (state.passkeys || []).map(publicPasskey) };
  },

  /// A one-time challenge to add a passkey, or to remove the one named `id`.
  async begin_passkey(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const purpose = args.purpose;
    if (purpose !== 'passkey_add' && purpose !== 'passkey_remove') return fail(400, 'bad_request', 'unknown purpose');
    if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key');
    const passkeys = state.passkeys || [];
    if (purpose === 'passkey_add' && passkeys.length >= MAX_PASSKEYS) return fail(409, 'too_many_passkeys', `an account holds at most ${MAX_PASSKEYS} passkeys`);
    if (purpose === 'passkey_remove' && !passkeys.some((p) => p.id === args.id)) return fail(404, 'passkey_not_found', 'no such passkey on this account');
    const challenge = await this.newChallenge(state, purpose);
    await this.storage.put('state', state);
    return { ok: true, challenge, expires_at: new Date(this.now + CHALLENGE_TTL_MS).toISOString(), passkeys: passkeys.map((p) => p.id) };
  },

  /// Add a passkey. The challenge is spent first, right or wrong; then the
  /// identity key's signature over the add message; then what the route found
  /// wrong with the WebAuthn answer (`refusal`); then the limits.
  async passkey_add(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!(await this.takeChallenge(state, args.challenge, 'passkey_add'))) return fail(400, 'challenge_invalid', 'no such challenge');
    const credentialId = typeof args.credential_id === 'string' && CREDENTIAL_ID.test(args.credential_id) ? args.credential_id : null;
    if (!credentialId) return fail(400, 'bad_request', 'credential id required');
    if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key');
    if (typeof args.origin !== 'string' || !args.origin) return fail(400, 'bad_request', 'origin required');
    const message = passkeyAddMessage({ origin: args.origin, accountId: state.account_id, credentialId, challenge: args.challenge });
    const signature = typeof args.signature === 'string' ? args.signature.toLowerCase() : '';
    if (!(await verifyEd25519(state.public_key, signature, message))) return fail(401, 'signature_invalid', 'signature verification failed');

    const refused = args.refusal;
    if (refused && typeof refused === 'object') {
      return { ok: false, status: Number(refused.status) || 400, code: String(refused.code || 'refused'), error: String(refused.error || 'refused') };
    }
    const r = args.record;
    if (!r || typeof r !== 'object' || r.id !== credentialId || !PASSKEY_ALGS.includes(r.alg) || !r.public_key || typeof r.public_key !== 'object') {
      return fail(400, 'bad_request', 'credential record required');
    }
    const passkeys = state.passkeys || [];
    if (passkeys.some((p) => p.id === credentialId)) return fail(409, 'passkey_registered', 'this passkey is already registered');
    if (passkeys.length >= MAX_PASSKEYS) return fail(409, 'too_many_passkeys', `an account holds at most ${MAX_PASSKEYS} passkeys`);
    // No other account may hold the id: the index object decides, strongly consistent.
    const claim = await passkeyIndexCall(this.env, credentialId, 'claim', { account_id: state.account_id });
    if (!claim.ok) {
      return claim.code === 'passkey_registered'
        ? fail(409, 'passkey_registered', 'this passkey is already registered')
        : fail(503, 'unavailable', 'the passkey index is unavailable');
    }

    const at = new Date(this.now).toISOString();
    const name = typeof args.name === 'string' && args.name ? args.name.slice(0, 64) : 'Passkey';
    const entry = {
      id: credentialId, alg: r.alg, public_key: r.public_key,
      sign_count: Number.isSafeInteger(r.sign_count) && r.sign_count >= 0 ? r.sign_count : 0,
      name, created_at: at, last_used_at: null,
      transports: Array.isArray(r.transports) ? r.transports.filter((t) => typeof t === 'string').slice(0, 6) : [],
      backup_eligible: r.backup_eligible === true, backed_up: r.backed_up === true,
    };
    state.passkeys = [...passkeys, entry];
    if (args.username) state.username = String(args.username).slice(0, 64);
    const { noticeId, recipients } = this.passkeyNotice(state, 'passkey_added', { username: state.username || '', at, passkeyName: name });
    await this.commit(state, 'passkey_added', { credential: await passkeyPrint(credentialId), alg: r.alg, name });
    return { ok: true, passkey: { id: entry.id, name, created_at: at, alg: entry.alg }, recipients, notice_id: noticeId };
  },

  /// Remove a passkey: the challenge, then the identity key's signature over the
  /// remove message.
  async passkey_remove(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    if (!(await this.takeChallenge(state, args.challenge, 'passkey_remove'))) return fail(400, 'challenge_invalid', 'no such challenge');
    const credentialId = typeof args.credential_id === 'string' && CREDENTIAL_ID.test(args.credential_id) ? args.credential_id : null;
    if (!credentialId) return fail(400, 'bad_request', 'credential id required');
    if (!state.public_key) return fail(409, 'no_current_key', 'this account has no usable key');
    if (typeof args.origin !== 'string' || !args.origin) return fail(400, 'bad_request', 'origin required');
    const message = passkeyRemoveMessage({ origin: args.origin, accountId: state.account_id, credentialId, challenge: args.challenge });
    const signature = typeof args.signature === 'string' ? args.signature.toLowerCase() : '';
    if (!(await verifyEd25519(state.public_key, signature, message))) return fail(401, 'signature_invalid', 'signature verification failed');
    const held = (state.passkeys || []).find((p) => p.id === credentialId);
    if (!held) return fail(404, 'passkey_not_found', 'no such passkey on this account');
    this.removePasskeys(state, [credentialId]);
    if (args.username) state.username = String(args.username).slice(0, 64);
    const at = new Date(this.now).toISOString();
    const { noticeId, recipients } = this.passkeyNotice(state, 'passkey_removed', { username: state.username || '', at, passkeyName: held.name });
    await this.commit(state, 'passkey_removed', { credential: await passkeyPrint(credentialId), name: held.name }, [
      { type: 'passkey_release', credential_id: credentialId },
    ]);
    return { ok: true, removed: credentialId, name: held.name, recipients, notice_id: noticeId };
  },

  /// A passkey sign-in, after the route has checked the client data, the RP and
  /// the flags. Here: the challenge has not signed in before, the account holds
  /// the credential, the signature verifies with its stored key, and the sign
  /// count moved forward. Then the count and the time of use are kept.
  async passkey_login(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const digest = typeof args.challenge_digest === 'string' && HEX64.test(args.challenge_digest) ? args.challenge_digest : null;
    if (!digest) return fail(400, 'challenge_invalid', 'no such challenge');

    const cred = (state.passkeys || []).find((p) => p.id === args.credential_id);
    if (!cred) return fail(401, 'passkey_unknown', 'this passkey is not on the account');
    const authData = fromBase64url(args.authenticator_data, 4096);
    const clientDataHash = fromBase64url(args.client_data_hash, 32);
    const signature = fromBase64url(args.signature, 1024);
    if (!authData || authData.length < 37 || !clientDataHash || clientDataHash.length !== 32 || !signature) return fail(400, 'bad_request', 'malformed assertion');
    if (!(await verifyPasskeySignature(cred.alg, cred.public_key, signedData(authData, clientDataHash), signature))) {
      return fail(401, 'signature_invalid', 'signature verification failed');
    }
    // A signed answer signs in once. Only answers that verified are remembered, so
    // nobody without the passkey can fill the list; and no entry is dropped early.
    const seen = Object.entries(state.passkey_logins || {}).filter(([, exp]) => exp > this.now);
    if (seen.some(([d]) => d === digest)) return fail(400, 'challenge_invalid', 'that challenge was used');
    if (seen.length >= PASSKEY_LOGIN_SEEN_MAX) return fail(429, 'rate_limited', 'too many passkey sign-ins; try again in a few minutes');
    // The count the authenticator signed, read from the signed bytes.
    const count = ((authData[33] << 24) >>> 0) + (authData[34] << 16) + (authData[35] << 8) + authData[36];
    const stored = Number(cred.sign_count) || 0;
    if ((count !== 0 || stored !== 0) && count <= stored) {
      await this.commit(state, 'passkey_cloned', { credential: await passkeyPrint(cred.id), stored, presented: count });
      return fail(403, 'passkey_cloned', 'this passkey\'s sign count went backwards: it may have been copied');
    }
    seen.push([digest, this.now + PASSKEY_LOGIN_SEEN_MS]);
    state.passkey_logins = Object.fromEntries(seen);
    cred.sign_count = count;
    cred.last_used_at = new Date(this.now).toISOString();
    cred.backed_up = !!(authData[32] & 0x10);
    await this.storage.put('state', state);
    return { ok: true, floor: state.floor, key_epoch: state.key_epoch };
  },

  /// The account is being deleted: every passkey goes, and its claim is released
  /// through the outbox.
  async passkeys_clear(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    const removed = this.removePasskeys(state, (state.passkeys || []).map((p) => p.id));
    state.passkey_logins = {};
    await this.commit(state, removed.length ? 'passkeys_cleared' : null, { count: removed.length },
      removed.map((id) => ({ type: 'passkey_release', credential_id: id })));
    return { ok: true, removed: removed.length };
  },

  async repair_mirror(args) {
    const { state, error } = await this.load(args);
    if (error) return error;
    await this.flushMirror(state);
    await this.storage.put('state', state);
    if (state.mirror_pending.length) await this.storage.setAlarm(Date.now() + MIRROR_RETRY_MS);
    return { ok: true, pending: state.mirror_pending.length };
  },
};

// -----------------------------------------------------------------------------
// The Worker's side
// -----------------------------------------------------------------------------

/// Call one operation of an account's object. Never throws: an object that is
/// unreachable answers `{ ok: false, status: 503, code: 'unavailable' }`, and a
/// caller that needs the object (sign-in) refuses on that.
export async function accountCall(env, accountId, op, args = {}) {
  if (!env.ACCOUNT_AUTH) return { ok: false, status: 503, code: 'no_binding', error: 'account objects are not configured' };
  try {
    const stub = env.ACCOUNT_AUTH.get(env.ACCOUNT_AUTH.idFromName(accountId));
    const call = (async () => {
      const res = await stub.fetch(`https://account.internal/${op}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ...args, account_id: accountId }),
      });
      return await res.json();
    })();
    return await withTimeout(call, Number(env.ACCOUNT_CALL_TIMEOUT_MS) || CALL_TIMEOUT_MS, `account ${op}`);
  } catch (e) {
    console.error(`accountCall ${op} failed:`, e?.message || e);
    return { ok: false, status: 503, code: 'unavailable', error: 'account object unavailable' };
  }
}

/// The account's state, seeding the object from KV the first time it is touched.
/// `{ ok: false }` when the account does not exist or the object is unavailable.
export async function accountState(env, accountId) {
  let state = await accountCall(env, accountId, 'state');
  if (state.ok && state.seeded) return state;
  if (!state.ok) return state;
  const raw = await env.USERS.get(`user:${accountId}`);
  if (!raw) return { ok: false, status: 404, code: 'no_account', error: 'no such account' };
  let user;
  try { user = JSON.parse(raw); } catch { return { ok: false, status: 500, code: 'bad_record', error: 'unreadable account record' }; }
  const floor = Number(await env.USERS.get(`authfloor:${accountId}`)) || 0;
  const seeded = await accountCall(env, accountId, 'seed', { public_key: user.public_key, floor, email: user.email || null });
  return seeded;
}
