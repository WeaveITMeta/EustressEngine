// =============================================================================
// Identity verification: what an account is made from, and what an outage is
// =============================================================================
//
// Two rules live here.
//
// 1. A service failure is not a verdict. When xAI cannot be asked (no key, a
//    refusal such as 402 or 429 when the credits run out, an exception, or an
//    answer with no verdict in it), the applicant is told verification is
//    temporarily unavailable, nothing is rejected, the try is given back, and
//    the administrators are emailed at most once an hour. /health shows the
//    last failure. Fail closed still holds: only a parsed APPROVE verifies.
//
// 2. Every account is made from one completed, verified KYC session (Security's
//    ruling, option A), and only by whoever started that session.
//    - POST /api/kyc/start makes the session: a server-chosen id, a claim secret
//      and an upload-only token, 256 random bits each, returned once and stored
//      only as SHA-256. Upload, submit and the phone hand-off take the upload
//      token; the hand-off link carries the id and that token, never the claim
//      secret. Registration and POST /api/kyc/link take the id AND the claim
//      secret, compared in constant time.
//    - The identity facts (id_hash, id_type, date of birth) come from the
//      session's record, never from the request. id_hash is HMAC-SHA-256 under
//      the Worker secret KYC_ID_PEPPER; without it, sign-up fails closed (503).
//    - One person, one account: the id_hash is claimed with an R2 create-if-
//      absent (claims/idhash/<hash>) BEFORE the session is consumed (claims/
//      kyc-session/<id>), and released if the session is not had. Those claims
//      are the only uniqueness check: the KV `idhash:` index is a lookup and is
//      never read for it (the values stored before were client-supplied and
//      prove nothing). A true collision is resolved by a person
//      (/api/admin/kyc-collision).
//    - An account that predates this (no `kyc_verified: true` on its record) may
//      sign in and read its own account, and every other change it asks for is
//      refused with `kyc_required` until it links a verified session.
//
// What the gate blocks, exactly (kycGate): every POST, PUT, PATCH and DELETE
// that carries a valid session token, or a commerce API key, of a human account
// whose record lacks `kyc_verified: true`, except
//   - the routes src/route_policy.mjs classes `public` (sign-in, sign-up, the
//     verification routes, one-time links, anonymous beacons), which do not act
//     as the account;
//   - POST /api/auth/logout and POST /api/kyc/link;
//   - administrators (src/admin_check.mjs), so the TAKE IT DOWN and moderation
//     duties are never locked out; and agent accounts (account_type 'agent'),
//     which never see a document.
// Reads (GET, HEAD) are not blocked. A record that cannot be read refuses the
// request (503); a record that cannot be parsed is treated as unverified.
// =============================================================================

import { routePolicy, isMutating, PUBLIC } from './route_policy.mjs';
import { alertAdmins } from './grok_model.mjs';
import { isAdmin } from './admin_check.mjs';
import { keyModeOf } from './commerce.mjs';

/// How old a verified session may be when it makes or upgrades an account.
export const KYC_SESSION_MAX_AGE_MS = 24 * 3600 * 1000;

/// The screening flags performFullKycVerification sets when xAI could not be
/// asked. A stored rejection carrying one is an outage, not a verdict.
export const SERVICE_FAILURE_FLAGS = Object.freeze(['verification_error', 'verification_unavailable', 'unparseable_response', 'verification_exception']);

/// The xAI answers that mean the account (key or credits), not the request:
/// the administrators are emailed about these.
export const ALERT_STATUSES = new Set([401, 402, 403, 429]);

export const KYC_UNAVAILABLE_MESSAGE = 'ID verification is temporarily unavailable. Your upload is saved; try again in a few minutes.';
export const KYC_UNAVAILABLE_RETRY_AFTER = '120';

/// Refunded service failures per session per day. A failure costs the applicant
/// nothing, but an unparseable answer was still a paid call, so a session that
/// keeps producing them pays tries again after this many.
export const KYC_SERVICE_REFUNDS_PER_SESSION = 10;

const STATUS_KEY = 'kyc:grok-status';
const ALERT_KEY = 'kyc:grok-alert';
const ALERT_EVERY_SECONDS = 3600;
const KYC_SESSION_ID = /^[A-Za-z0-9_-]{8,64}$/;
const DOCUMENT_TYPES = new Set(['passport', 'drivers_license', 'national_id', 'other']);

// ---------------------------------------------------------------------------
// Rule 1: an outage is not a verdict
// ---------------------------------------------------------------------------

/// True when a stored record was rejected only because xAI could not be asked.
export function rejectedForService(rec) {
  const flags = rec?.grok_analysis?.screening_flags;
  return rec?.status === 'rejected' && Array.isArray(flags) && flags.some((f) => SERVICE_FAILURE_FLAGS.includes(f));
}

/// The model's own flags, without the names reserved for an outage, so a
/// document cannot talk its way into the outage path.
export function modelFlags(flags) {
  return Array.isArray(flags) ? flags.filter((f) => typeof f === 'string' && !SERVICE_FAILURE_FLAGS.includes(f)).slice(0, 20) : [];
}

/// xAI's error text, short and without anything that looks like a key. It goes
/// to the administrators only, never to the applicant or /health.
export function cleanXaiError(text) {
  let s = typeof text === 'string' ? text : '';
  try {
    const j = JSON.parse(s);
    const e = j?.error ?? j?.message ?? j?.code;
    s = typeof e === 'string' ? e : (typeof e?.message === 'string' ? e.message : s);
  } catch { /* plain text */ }
  return s.replace(/xai-[A-Za-z0-9_-]+/g, 'xai-[redacted]').replace(/Bearer\s+\S+/gi, 'Bearer [redacted]').replace(/\s+/g, ' ').trim().slice(0, 300);
}

/// Records how the last ID verification call to xAI went, for /health, and
/// emails the administrators (at most once an hour) when xAI refused the call
/// for the account's sake. Never throws. `outcome` is `{ ok: true }` or
/// `{ ok: false, status, kind, error }`; it carries no applicant data.
export async function noteGrokKyc(env, outcome, deps = {}) {
  const now = new Date(deps.now ?? Date.now()).toISOString();
  const kv = env.SCREENING;
  try {
    const prev = JSON.parse((await kv.get(STATUS_KEY)) || 'null');
    if (outcome.ok) {
      if (prev?.ok !== true) await kv.put(STATUS_KEY, JSON.stringify({ ok: true, at: now }));
    } else {
      await kv.put(STATUS_KEY, JSON.stringify({
        ok: false, status: Number.isInteger(outcome.status) ? outcome.status : null, kind: outcome.kind || 'error', at: now,
        last_ok_at: prev?.ok === true ? prev.at : (prev?.last_ok_at || null),
      }));
    }
  } catch (e) {
    console.error('kyc: the Grok status was not saved:', e?.message || e);
  }
  if (outcome.ok || !ALERT_STATUSES.has(outcome.status)) return { alerted: false };
  try {
    if (await kv.get(ALERT_KEY)) return { alerted: false, throttled: true };
    await kv.put(ALERT_KEY, JSON.stringify({ at: now, status: outcome.status }), { expirationTtl: ALERT_EVERY_SECONDS });
    const { sent } = await alertAdmins(env, deps, 'kyc_refused', { status: outcome.status, error: cleanXaiError(outcome.error), at: now });
    return { alerted: true, sent };
  } catch (e) {
    console.error('kyc: the administrators were not told:', e?.message || e);
    return { alerted: false };
  }
}

/// The ID verification line of /health: ok, or the last failure's status and
/// time. No key, no error text.
export async function grokKycHealth(env) {
  let s = null;
  try { s = JSON.parse((await env.SCREENING?.get(STATUS_KEY)) || 'null'); } catch { s = null; }
  if (!s) return { status: 'ok', checked_at: null };
  if (s.ok === true) return { status: 'ok', checked_at: s.at || null };
  return { status: 'failing', last_failure_status: s.status ?? null, last_failure_kind: s.kind || null, last_failure_at: s.at || null, last_ok_at: s.last_ok_at || null };
}

/// A record rejected only by an outage, put back to its uploads so the next
/// submit runs the check again. The verdict fields go; the upload stays.
export function reopenedUpload(rec, now = Date.now()) {
  const keep = ['upload_id', 'session_id', 'iso2', 'id_type', 'side', 'r2_key', 'uploaded_at', 'hash'];
  const out = { status: 'uploaded', reopened_after_outage_at: new Date(now).toISOString() };
  for (const k of keep) if (rec[k] !== undefined) out[k] = rec[k];
  return out;
}

// ---------------------------------------------------------------------------
// Rule 2: one verified session, one account
// ---------------------------------------------------------------------------

/// The 409 for an ID already linked to an account. It says how to reach a
/// person: a true collision (two people the document reads alike) is resolved
/// by manual review (POST /api/admin/kyc-collision).
export const KYC_ID_IN_USE = Object.freeze({
  error: 'This ID is already linked to an account. If you think that is wrong, email support@eustress.dev and a person will review it.',
  code: 'kyc_id_in_use',
  support: 'support@eustress.dev',
});

const HEX64 = /^[0-9a-f]{64}$/;

/// A name as printed, reduced so two documents of one person agree: accents
/// off, case off, punctuation off, word order off.
export function normalizedName(name) {
  return String(name || '').normalize('NFKD').replace(/\p{M}+/gu, '').toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ').trim().split(' ').filter(Boolean).sort().join(' ');
}

async function sha256Hex(text) {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function hmacHex(key, text) {
  const k = await crypto.subtle.importKey('raw', new TextEncoder().encode(key), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const sig = await crypto.subtle.sign('HMAC', k, new TextEncoder().encode(text));
  return [...new Uint8Array(sig)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

const randomHex = (bytes) => [...crypto.getRandomValues(new Uint8Array(bytes))].map((b) => b.toString(16).padStart(2, '0')).join('');

/// Equal strings, compared in time that does not depend on where they differ.
export function constantTimeEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  let diff = a.length ^ b.length;
  for (let i = 0; i < Math.max(a.length, b.length); i++) diff |= (a.charCodeAt(i) || 0) ^ (b.charCodeAt(i) || 0);
  return diff === 0;
}

/// True when KYC_ID_PEPPER, the secret id_hash is keyed with, is set.
export const idPepperSet = (env) => typeof env?.KYC_ID_PEPPER === 'string' && env.KYC_ID_PEPPER.trim().length >= 16;

/// The server's id_hash for a verified record: HMAC-SHA-256 under the Worker
/// secret KYC_ID_PEPPER of the person as the document reads (legal name and
/// date of birth), so one person makes one account whichever document they
/// show, and the hash cannot be recomputed from a name and a birthday by
/// anyone without the secret. With no name read, the document photo's hash.
/// `reviewed` (a session id) is set only by an administrator's collision
/// review, and gives that one applicant a hash of their own. Null when the
/// record has neither a name and date of birth nor a photo hash.
export async function kycIdHash(rec, pepper, reviewed = null) {
  if (typeof pepper !== 'string' || pepper.trim().length < 16) throw new Error('KYC_ID_PEPPER is not set');
  const name = normalizedName(rec?.grok_analysis?.extracted_name);
  const dob = /^\d{4}-\d{2}-\d{2}$/.test(rec?.extracted_dob || '') ? rec.extracted_dob : '';
  const review = reviewed ? `|reviewed:${reviewed}` : '';
  if (name && dob) return hmacHex(pepper, `eustress-kyc-person-v1|${name}|${dob}${review}`);
  if (typeof rec?.hash === 'string' && HEX64.test(rec.hash)) return hmacHex(pepper, `eustress-kyc-document-v1|${rec.hash}${review}`);
  return null;
}

/// The model's one-sentence reason for refusing a document, fit to show the
/// applicant: nothing about the document's holder ever leaves the Worker in an
/// answer. Null (the caller says "Document could not be verified") when the
/// sentence carries a word of the name on the record (read off the document or
/// typed by the applicant) or any run of four or more digits (a date, a document
/// number). Cut to 200 characters.
export function holderFreeReason(text, rec) {
  if (typeof text !== 'string' || !text.trim()) return null;
  const said = normalizedName(text).split(' ');
  const names = new Set([rec?.ocr_name, rec?.grok_analysis?.extracted_name]
    .flatMap((n) => normalizedName(n).split(' ')).filter((w) => w.length >= 3));
  if (said.some((w) => names.has(w))) return null;
  if (/\d{4,}/.test(text.replace(/[\s./-]/g, ''))) return null;
  return text.trim().slice(0, 200);
}

/// The document type as the model read it, else as the upload named it.
export function kycIdType(rec) {
  const read = rec?.grok_analysis?.document_type;
  if (DOCUMENT_TYPES.has(read)) return read;
  const named = typeof rec?.id_type === 'string' ? rec.id_type.replace(/[^\p{L}\p{N} '._-]/gu, '').slice(0, 40) : '';
  return named || null;
}

// ── Sessions: server-chosen ids, a claim secret, an upload-only token ──────
//
// POST /api/kyc/start makes a session: a server-chosen id, a claim secret and
// an upload token (256 random bits each), returned once. Only their SHA-256
// is stored (`kycsess:<id>`). The upload token uploads, submits and starts the
// phone hand-off; the hand-off link carries it and the id, never the claim
// secret. Only the claim secret, beside the id, makes or links an account.

export const KYC_SESSION_TTL_SECONDS = 30 * 86400;
const sessionKey = (id) => `kycsess:${id}`;

/// POST /api/kyc/start. `deps`: json.
export async function handleKycStart(request, env, cors, { json }) {
  if (!env.KYC_STATUS) return json({ error: 'Identity verification is not available', code: 'kyc_unavailable' }, 503, cors);
  const session_id = crypto.randomUUID();
  const claim_secret = randomHex(32);
  const upload_token = randomHex(32);
  await env.KYC_STATUS.put(sessionKey(session_id), JSON.stringify({
    claim_hash: await sha256Hex(claim_secret),
    upload_hash: await sha256Hex(upload_token),
    created_at: new Date().toISOString(),
  }), { expirationTtl: KYC_SESSION_TTL_SECONDS });
  return json({ session_id, claim_secret, upload_token, expires_in: KYC_SESSION_TTL_SECONDS }, 200, { ...cors, 'Cache-Control': 'no-store' });
}

async function sessionRecord(env, sessionId) {
  if (typeof sessionId !== 'string' || !KYC_SESSION_ID.test(sessionId)) return null;
  const raw = await env.KYC_STATUS?.get(sessionKey(sessionId));
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

/// True when `token` is the upload token of a started session `sessionId`.
export async function uploadTokenValid(env, sessionId, token) {
  if (typeof token !== 'string' || !HEX64.test(token)) return false;
  const s = await sessionRecord(env, sessionId);
  return !!s && constantTimeEqual(await sha256Hex(token), String(s.upload_hash || ''));
}

/// Who is reading a session's records (GET /api/kyc/session, /status, /handoff):
/// null when no session `sessionId` was started (the route answers 404),
/// 'holder' when the X-KYC-Token header is the session's upload token or claim
/// secret, else 'bare'. The token is never taken from the query string, which
/// lands in request logs. A bare reader, holding only the id (which the phone
/// link carries in the clear), gets the status and nothing about the applicant.
export const KYC_TOKEN_HEADER = 'X-KYC-Token';
export async function kycReader(request, env, sessionId) {
  const session = await sessionRecord(env, sessionId);
  if (!session) return null;
  const token = request.headers.get(KYC_TOKEN_HEADER);
  if (typeof token !== 'string' || !HEX64.test(token)) return 'bare';
  const h = await sha256Hex(token);
  const upload = constantTimeEqual(h, String(session.upload_hash || ''));
  const claim = constantTimeEqual(h, String(session.claim_hash || ''));
  return upload || claim ? 'holder' : 'bare';
}

/// True when `secret` is the claim secret of a started session `sessionId`.
async function claimSecretValid(session, secret) {
  if (!session || typeof secret !== 'string' || !HEX64.test(secret)) return false;
  return constantTimeEqual(await sha256Hex(secret), String(session.claim_hash || ''));
}

export const KYC_UPLOAD_REFUSED = Object.freeze({ error: 'This verification session is not valid. Start the verification again.', code: 'kyc_session_invalid' });

const kycRequired = (reason) => ({
  ok: false, status: 403,
  body: { error: 'Identity verification is required', code: 'kyc_required', reason },
});

/// The verified session a sign-up (or a link) is made from, or the refusal.
/// It needs the session id AND its claim secret. `{ ok: true, record, id_hash,
/// id_type, dob }` or `{ ok: false, status, body }`. Without KYC_ID_PEPPER it
/// fails closed with 503 `kyc_unconfigured`.
export async function kycSessionForAccount(env, sessionId, claimSecret, now = Date.now()) {
  if (!idPepperSet(env))
    return { ok: false, status: 503, body: { error: 'Sign-up is unavailable: identity verification is not configured on this server (KYC_ID_PEPPER is not set).', code: 'kyc_unconfigured' } };
  if (typeof sessionId !== 'string' || !KYC_SESSION_ID.test(sessionId))
    return kycRequired('Verify your identity with a government photo ID first.');
  // An account's own id names its linked record, never a session.
  if (await env.USERS.get(`user:${sessionId}`)) return kycRequired('Verify your identity with a government photo ID first.');
  const session = await sessionRecord(env, sessionId);
  // The same answer for an unknown session and a wrong secret.
  if (!(await claimSecretValid(session, claimSecret)))
    return kycRequired('This verification does not belong to this sign-up. Verify your ID again.');
  const raw = await env.KYC_STATUS.get(`kyc-${sessionId}-front`);
  if (!raw) return kycRequired('No completed identity verification was found. Verify your ID again.');
  let rec;
  try { rec = JSON.parse(raw); } catch { return kycRequired('No completed identity verification was found. Verify your ID again.'); }
  if (rec?.status !== 'verified' || rec.age_verified !== true)
    return kycRequired('Your identity verification has not passed.');
  const at = Date.parse(rec.verified_at || '');
  if (!Number.isFinite(at) || at > now + 60_000 || now - at > KYC_SESSION_MAX_AGE_MS)
    return kycRequired('Your identity verification is more than 24 hours old. Verify your ID again.');
  const id_hash = await kycIdHash(rec, env.KYC_ID_PEPPER, session.collision_review ? sessionId : null);
  if (!id_hash) return kycRequired('Your identity verification could not be tied to a document. Verify your ID again.');
  return { ok: true, record: rec, id_hash, id_type: kycIdType(rec), dob: rec.extracted_dob };
}

/// Consumes a verified session for `userId`: true for the one caller that
/// claims it, false for every other. An R2 create-if-absent, the primitive the
/// moderation lease uses; KV cannot stop two parallel claims. Throws when the
/// bucket is not bound, so a caller refuses rather than skipping the claim.
export async function consumeKycSession(env, sessionId, userId) {
  if (!env.KYC_BUCKET) throw new Error('KYC_BUCKET is not bound');
  const put = await env.KYC_BUCKET.put(`claims/kyc-session/${sessionId}`, JSON.stringify({ user_id: userId, at: new Date().toISOString() }), {
    httpMetadata: { contentType: 'application/json' },
    onlyIf: { etagDoesNotMatch: '*' },
  });
  return !!put;
}

const idClaimKey = (idHash) => `claims/idhash/${idHash}`;

/// Claims `idHash` for `userId` with an R2 create-if-absent: 'created' when
/// this call made the claim, 'held' when the account already held it, false
/// when another account holds it. This is the only uniqueness check: the KV `idhash:` index is
/// written for lookups and never read for it, so the client-supplied hashes
/// stored before this change can block no one. Throws without the bucket.
export async function claimIdHash(env, idHash, userId) {
  if (!env.KYC_BUCKET) throw new Error('KYC_BUCKET is not bound');
  if (typeof idHash !== 'string' || !HEX64.test(idHash)) throw new Error('not an id_hash');
  const put = await env.KYC_BUCKET.put(idClaimKey(idHash), JSON.stringify({ user_id: userId, at: new Date().toISOString() }), {
    httpMetadata: { contentType: 'application/json' },
    onlyIf: { etagDoesNotMatch: '*' },
  });
  if (put) return 'created';
  const held = await env.KYC_BUCKET.get(idClaimKey(idHash));
  if (!held) return false;
  try { return (await held.json()).user_id === userId ? 'held' : false; } catch { return false; }
}

/// Removes `userId`'s claim on `idHash` (a failed sign-up, or the account's
/// deletion). A claim naming another account is left alone. Never throws.
export async function releaseIdHash(env, idHash, userId) {
  try {
    if (!env.KYC_BUCKET || typeof idHash !== 'string' || !HEX64.test(idHash)) return false;
    const held = await env.KYC_BUCKET.get(idClaimKey(idHash));
    if (!held || (await held.json()).user_id !== userId) return false;
    await env.KYC_BUCKET.delete(idClaimKey(idHash));
    return true;
  } catch (e) {
    console.error('kyc: an id_hash claim was not released:', e?.message || e);
    return false;
  }
}

/// Moves a consumed session's records to the account, as `linked`.
export async function linkKycRecords(env, sessionId, userId) {
  for (const side of ['front', 'back']) {
    const key = `kyc-${sessionId}-${side}`;
    const raw = await env.KYC_STATUS.get(key);
    if (!raw) continue;
    const rec = JSON.parse(raw);
    rec.user_id = userId;
    rec.status = 'linked';
    await env.KYC_STATUS.put(`kyc-${userId}-${side}`, JSON.stringify(rec), { expirationTtl: 86400 * 365 * 7 });
    await env.KYC_STATUS.delete(key);
  }
}

/// Claims the person, then the session, for `userId`, releasing the person
/// again when the session is not had. `{ ok: true }` or `{ ok: false, status, body }`.
export async function claimPersonAndSession(env, idHash, sessionId, userId) {
  let person;
  try { person = await claimIdHash(env, idHash, userId); } catch (e) {
    console.error('kyc: the id_hash could not be claimed:', e?.message || e);
    return { ok: false, status: 503, body: { error: 'Verification is temporarily unavailable. Try again in a minute.', code: 'signup_unavailable' } };
  }
  if (!person) return { ok: false, status: 409, body: { ...KYC_ID_IN_USE } };
  let session;
  try { session = await consumeKycSession(env, sessionId, userId); } catch (e) {
    console.error('kyc: the session could not be claimed:', e?.message || e);
    if (person === 'created') await releaseIdHash(env, idHash, userId);
    return { ok: false, status: 503, body: { error: 'Verification is temporarily unavailable. Try again in a minute.', code: 'signup_unavailable' } };
  }
  if (!session) {
    if (person === 'created') await releaseIdHash(env, idHash, userId);
    return { ok: false, status: 409, body: { error: 'This identity verification was already used', code: 'kyc_session_used' } };
  }
  return { ok: true, created: person === 'created' };
}

/// A sign-up whose account writes failed part way: what was written for
/// `user_id` is removed (each index only when it names this account) and the
/// person's claim is released, so a claim never names an account that was
/// never written and no half account can sign in. The consumed session stays
/// consumed: the applicant verifies again. Never throws.
export async function undoAccountWrites(env, { user_id, username, public_key, id_hash }) {
  const drop = async (key, ifNames) => {
    try {
      if (ifNames && (await env.USERS.get(key)) !== user_id) return;
      await env.USERS.delete(key);
    } catch (e) {
      console.error('kyc: a half-written account record was not removed:', e?.message || e);
    }
  };
  await drop(`user:${user_id}`, false);
  if (username) await drop(`username:${username}`, true);
  if (public_key) await drop(`pubkey:${public_key}`, true);
  if (id_hash) await drop(`idhash:${id_hash}`, true);
  await releaseIdHash(env, id_hash, user_id);
}

/// True for an account that must verify before it may change anything: a
/// human account whose record does not say `kyc_verified: true`.
export function needsKyc(user) {
  return !!user && user.kyc_verified !== true && user.account_type !== 'agent';
}

const GATE_OPEN = new Set(['/api/auth/logout', '/api/kyc/link']);

/// The account a mutating request acts as: from a session token, or from a
/// commerce API key. Null when the request names none the Worker accepts.
async function actingAccount(request, env, deps) {
  const m = /^Bearer\s+(\S+)\s*$/i.exec(request.headers.get('Authorization') || '');
  if (!m) return null;
  const token = m[1];
  if (keyModeOf(token)) {
    const raw = await env.USERS.get(`apikey:${await sha256Hex(token)}`);
    if (!raw) return null;
    try { return JSON.parse(raw).account_id || null; } catch { return null; }
  }
  const claims = await deps.verifyJwt(token, env.JWT_SECRET);
  return claims && typeof claims.sub === 'string' && claims.sub ? claims.sub : null;
}

/// A response that stops the request, or null to let it through. See the top
/// of this file for exactly what it blocks. `deps`: json, verifyJwt.
export async function kycGate(request, env, cors, deps) {
  if (!isMutating(request.method)) return null;
  const path = new URL(request.url).pathname;
  if (GATE_OPEN.has(path)) return null;
  if (routePolicy(request.method, path).class === PUBLIC) return null;
  let raw;
  let accountId;
  try {
    accountId = await actingAccount(request, env, deps);
    // No session or key the Worker accepts: the route refuses it itself.
    if (!accountId) return null;
    raw = await env.USERS.get(`user:${accountId}`);
  } catch (e) {
    console.error('kyc gate: the account could not be read:', e?.message || e);
    return deps.json({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  }
  // No such account: the route answers.
  if (raw === null || raw === undefined) return null;
  let user;
  try { user = JSON.parse(raw); } catch { user = {}; }
  if (!needsKyc(user)) return null;
  try {
    if (await isAdmin(env, accountId)) return null;
  } catch (e) {
    console.error('kyc gate: the admin check failed:', e?.message || e);
  }
  return deps.json({
    error: 'Verify your identity to keep using your account',
    code: 'kyc_required',
    reason: 'Your account was created before identity verification was required. Verify your ID to use everything beyond reading your own account.',
  }, 403, cors);
}

// ---------------------------------------------------------------------------
// POST /api/kyc/link: an account that predates the rule verifies now
// ---------------------------------------------------------------------------

/// `{ kyc_session_id, kyc_claim_secret }` with the account's session. `deps`: verifyAuth, json, publicUser.
export async function handleKycLink(request, env, cors, deps) {
  const { verifyAuth, json, publicUser } = deps;
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const body = await request.json().catch(() => ({}));
  const raw = await env.USERS.get(`user:${userId}`);
  if (!raw) return json({ error: 'User not found' }, 404, cors);
  if (JSON.parse(raw).kyc_verified === true) return json({ ok: true, already_verified: true }, 200, cors);

  const kyc = await kycSessionForAccount(env, body?.kyc_session_id, body?.kyc_claim_secret);
  if (!kyc.ok) return json(kyc.body, kyc.status, cors);
  const claimed = await claimPersonAndSession(env, kyc.id_hash, body.kyc_session_id, userId);
  if (!claimed.ok) return json(claimed.body, claimed.status, cors);

  // Read again just before the write, so the change is the verification only.
  const user = JSON.parse((await env.USERS.get(`user:${userId}`)) || raw);
  const oldHash = user.id_hash;
  Object.assign(user, {
    kyc_verified: true, kyc_verified_at: new Date().toISOString(),
    age_verified: true, verified_dob: kyc.dob, birthday: kyc.dob,
    id_type: kyc.id_type, id_hash: kyc.id_hash,
  });
  try {
    await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  } catch (e) {
    // The account does not say it is verified: the person is not left claimed for it.
    console.error('kyc link: the account was not written; the person is released:', e?.message || e);
    if (claimed.created) await releaseIdHash(env, kyc.id_hash, userId);
    return json({ error: 'Verification could not be saved. Verify your ID again and retry.', code: 'link_failed' }, 503, cors);
  }
  // The account record is written and is the truth; the indexes below are lookups.
  try {
    // The client-supplied hash the account had before is dropped from the index.
    if (oldHash && oldHash !== kyc.id_hash && (await env.USERS.get(`idhash:${oldHash}`)) === userId) await env.USERS.delete(`idhash:${oldHash}`);
    await env.USERS.put(`idhash:${kyc.id_hash}`, userId);
  } catch (e) {
    console.error('kyc link: the id_hash index was not updated:', e?.message || e);
  }
  await linkKycRecords(env, body.kyc_session_id, userId);
  return json({ ok: true, user: await publicUser(user, env) }, 200, cors);
}

// ---------------------------------------------------------------------------
// /api/admin/kyc-collision: a true collision, resolved by a person
// ---------------------------------------------------------------------------

//   GET  /api/admin/kyc-collision?session=<id>   who holds the id_hash the session's document gives
//   POST /api/admin/kyc-collision {kyc_session_id, reason}
//        after a manual review found two different people whose documents read
//        alike (same name and date of birth): this session's applicant gets an
//        id_hash of their own, and may register or link with it. Audited. The
//        holder's account is not touched. The review lasts as long as the
//        session (a verification is good for 24 hours).
export async function handleKycCollisionRoute(request, url, env, cors, deps) {
  const { requireAdmin, json, auditLog } = deps;
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (!idPepperSet(env)) return json({ error: 'KYC_ID_PEPPER is not set', code: 'kyc_unconfigured' }, 503, cors);
  const body = request.method === 'POST' ? await request.json().catch(() => ({})) : {};
  const sessionId = request.method === 'POST' ? body.kyc_session_id : url.searchParams.get('session');
  const session = await sessionRecord(env, sessionId);
  const raw = session ? await env.KYC_STATUS.get(`kyc-${sessionId}-front`) : null;
  if (!session || !raw) return json({ error: 'No such verification session', code: 'not_found' }, 404, cors);
  const rec = JSON.parse(raw);
  const base = await kycIdHash(rec, env.KYC_ID_PEPPER);
  const held = base ? await env.KYC_BUCKET?.get(idClaimKey(base)) : null;
  const holder = held ? (await held.json()).user_id || null : null;
  if (request.method === 'GET') return json({ session_id: sessionId, status: rec.status, holder, reviewed: !!session.collision_review }, 200, cors);
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  const reason = typeof body.reason === 'string' ? body.reason.trim().slice(0, 500) : '';
  if (!reason) return json({ error: 'A reason (what the review found) is required', code: 'reason_required' }, 400, cors);
  if (!holder) return json({ error: 'No account holds this ID; there is no collision to resolve', code: 'no_collision' }, 409, cors);
  session.collision_review = { by: adminId, at: new Date().toISOString(), reason, holder };
  await env.KYC_STATUS.put(sessionKey(sessionId), JSON.stringify(session), { expirationTtl: KYC_SESSION_TTL_SECONDS });
  if (auditLog) await auditLog(env, 'KYC_COLLISION_REVIEWED', adminId, sessionId, { holder, reason });
  return json({ ok: true, session_id: sessionId, holder }, 200, cors);
}

// ---------------------------------------------------------------------------
// /api/admin/kyc-legacy: count the accounts made without KYC (and reconcile)
// ---------------------------------------------------------------------------

//   GET  /api/admin/kyc-legacy?limit=&cursor=   counts one page; writes nothing
//   POST /api/admin/kyc-legacy {limit, cursor, apply: true}
//        sets `kyc_verified` from false to true on each record of the page that
//        has a KYC record (kyc-<id>-front, verified or linked), and nothing else.
//   POST ... {revoke_unbacked: true}
//        the other direction, asked for separately: sets `kyc_verified` from true
//        to false where no KYC record is behind it (which gates the account).
//   A POST with neither is a dry run. No account is ever deleted. The counts list
//   the gated administrators by id (exempt from the gate, for McKale to verify).
// Page through with `cursor` until `done`, summing `counts`.
export async function handleKycLegacyRoute(request, url, env, cors, deps) {
  const { requireAdmin, json, auditLog } = deps;
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (request.method !== 'GET' && request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  const body = request.method === 'POST' ? await request.json().catch(() => ({})) : {};
  const apply = request.method === 'POST' && body.apply === true;
  const revoke = request.method === 'POST' && body.revoke_unbacked === true;
  const limit = Math.max(1, Math.min(200, Number(body.limit ?? url.searchParams.get('limit')) || 100));
  const cursor = body.cursor ?? url.searchParams.get('cursor') ?? undefined;

  const page = await env.USERS.list({ prefix: 'user:', limit, ...(cursor ? { cursor } : {}) });
  // gated: no `kyc_verified: true`, so the gate blocks it now (admins are counted
  // and exempt). flag_without_record: says verified with no KYC record behind it.
  // record_without_flag: the other way (opened by apply).
  const counts = { scanned: 0, verified: 0, gated: 0, gated_with_id_hash: 0, gated_admins: 0, agents: 0, flag_without_record: 0, record_without_flag: 0, unreadable: 0 };
  const gatedAdminIds = [];
  const changes = [];
  for (const { name } of page.keys || []) {
    const id = name.slice('user:'.length);
    const raw = await env.USERS.get(name);
    if (!raw) continue;
    counts.scanned++;
    let user;
    try { user = JSON.parse(raw); } catch { counts.unreadable++; continue; }
    if (user.account_type === 'agent') { counts.agents++; continue; }
    let backed = false;
    try {
      const k = JSON.parse((await env.KYC_STATUS.get(`kyc-${id}-front`)) || 'null');
      backed = k?.status === 'linked' || k?.status === 'verified';
    } catch { backed = false; }
    const flag = user.kyc_verified === true;
    if (flag && backed) counts.verified++;
    if (flag && !backed) { counts.flag_without_record++; changes.push({ id, kyc_verified: false }); }
    if (!flag && backed) { counts.record_without_flag++; changes.push({ id, kyc_verified: true }); }
    if (!flag) {
      counts.gated++;
      if (user.id_hash) counts.gated_with_id_hash++;
      if (await isAdmin(env, id)) { counts.gated_admins++; gatedAdminIds.push(id); }
    }
  }

  let changed = 0;
  for (const c of changes) {
    if (c.kyc_verified ? !apply : !revoke) continue;
    const fresh = await env.USERS.get(`user:${c.id}`);
    if (!fresh) continue;
    const user = JSON.parse(fresh);
    if ((user.kyc_verified === true) === c.kyc_verified) continue;
    user.kyc_verified = c.kyc_verified;
    user.kyc_reconciled_at = new Date().toISOString();
    await env.USERS.put(`user:${c.id}`, JSON.stringify(user));
    changed++;
  }
  if ((apply || revoke) && auditLog) await auditLog(env, 'KYC_LEGACY_RECONCILE', adminId, null, { counts, changed, apply, revoke_unbacked: revoke });
  const done = page.list_complete !== false;
  return json({
    dry_run: !apply && !revoke, counts, gated_admin_ids: gatedAdminIds,
    would_open: changes.filter((c) => c.kyc_verified).length, would_gate: changes.filter((c) => !c.kyc_verified).length,
    changed, cursor: done ? null : page.cursor, done,
  }, 200, cors);
}
