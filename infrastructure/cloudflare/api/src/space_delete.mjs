// =============================================================================
// Deleting a published Space
// =============================================================================
//
// What it deletes: a listing (`sim:{id}`), the published Universe and every
// Space it holds. Design and inventory: docs/architecture/DELETE_PUBLISHED_SPACE.md,
// with McKale's decisions of 2026-11-12, which this file follows where they
// differ from the document:
//
//   1. The second factor is a six-digit code emailed to the account's verified
//      address, and only once that address has been verified for thirty days.
//      The owner types the listing's name exactly; delete-request mails the
//      code; delete-confirm takes the code and the name again. The code is never
//      stored, logged or sent anywhere but that one mail: the account's object
//      keeps an HMAC digest of it, counts the attempts (five, then the request is
//      locked) and spends it once. Fifteen minutes. Three requests per listing an
//      hour, five per account a day. The mail is sent only while the setting
//      DELETE_CODE_EMAIL is exactly "on"; otherwise every request and confirm
//      answers 503 delete_unavailable. A valid, current session is required too.
//   2. A Space with money attached is refused (409, plain words) until every
//      product of the listing is off sale and the last live sale's 72-hour refund
//      window has closed.
//   3. Ninety days soft deleted (hidden everywhere, bytes untouched, restorable
//      by the owner with the same emailed code), then removed by the hourly job
//      in bounded, resumable pages. The job is armed by its own setting,
//      SPACE_HARD_DELETE, exactly "on" (off in both environments), and removes
//      nothing while DELETE_CODE_EMAIL is off, because nobody could restore.
//   4. The most conservative legal defaults: a deletion does not start, and no
//      removal step runs, while any legal lock, TAKE IT DOWN request, quarantine,
//      report debt, failed seal, evidence kept in place, open review or staff hold
//      applies; the legal lock is read again before every R2 delete call. Money,
//      receipts, payouts, audit chains, cases, the evidence vault, legal reports,
//      derived/ images and the review evidence under universes/{id}/moderation/
//      are never deleted here. The tombstone keeps the file hashes, so a case
//      reference keeps working and a later takedown can still name the files.
//      The author is never told which hold applies (the `in_review` copy).
//   5. The mark that a listing is deleted is an R2 object (src/space_delete_mark.mjs)
//      that every serving path reads through the legal-lock read paths, so a KV
//      whole-record rewrite cannot undo it. Every author write to a deleted
//      listing answers 409 listing_deleted. The website manifest stops serving,
//      and the namespace stays reserved to its owner.
//
// The soft delete writes nothing to the listing's KV record: the R2 mark is the
// one truth, so there is nothing for a racing whole-record writer to undo, and a
// restore puts back exactly the record that was there.
// =============================================================================

import {
  contentLockedFor, loadCase, legalLockOf, acquireLease, releaseLease, thresholdsFrom, inReview, caseLocksContent,
  legalLockedIds, listingReportOwedKey, workspaceQueueKey, queueKey, caseKey,
} from './moderation.mjs';
import { caseReferenceOf, assignCaseReference } from './case_reference.mjs';
import { accountCall, accountState } from './account_auth.mjs';
import { verifyFreshAuth } from './fresh_auth.mjs';
import { hubStub, callObject, REFUND_WINDOW_MS } from './commerce.mjs';
import { tellLiveIndex, GUEST_LIVE_INDEX_NAME } from './live_index.mjs';
import { END_ALL_GUESTS, isLiveHeartbeat, guestSessionName } from './live.mjs';
import { sendNotice, maskAddress } from './account_notices.mjs';
import { renderEmail, html, paragraph, note, strong, facts, formatWhen } from './email_shell.mjs';
import { accountTimeZone } from './account_time_zone.mjs';
import { PENDING_PREFIX as VAULT_PENDING_PREFIX, VAULT_PREFIX } from './evidence_vault.mjs';
import { deletionMarkKey, deletionMarkHead, readDeletionMark, deletionView, markAuthor, DELETED_PREFIX, tombstoneKey } from './space_delete_mark.mjs';

export { tombstoneKey };

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/// How long a deleted Space stays restorable before the hourly job removes it.
export const SOFT_DELETE_DAYS = 90;
export const SOFT_DELETE_MS = SOFT_DELETE_DAYS * DAY;
/// The label the code's key is derived under from JWT_SECRET. No separate secret exists for this; the label keeps the
/// derived key distinct from every other use of that secret (the evidence vault derives its own the same way).
export const DELETE_CODE_KEY_LABEL = 'eustress-space-delete-code-v1';
/// A staff rationale is at least this long.
export const STAFF_RATIONALE_MIN = 20;
/// The hourly job's own allowance of storage calls per run, so it never starves the takedown, evidence and settlement
/// sweeps that share the invocation, and the wall time it stops at.
export const SWEEP_CALL_BUDGET = 400;
export const SWEEP_WALL_MS = 10 * MINUTE;

const SIM_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const REQUEST_ID = /^sdr_[A-Za-z0-9]{24}$/;
const MUTATING = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);
const SUPPORT = 'support@eustress.dev';

const DUE_PREFIX = 'spacedel:due:';
const dueKey = (atMs, simId) => `${DUE_PREFIX}${new Date(atMs).toISOString().slice(0, 13)}:${simId}`;
const HOLD_PREFIX = 'locks/space-delete-hold/';
export const staffHoldKey = (simId) => `${HOLD_PREFIX}${simId}.json`;
export const tombstoneAuthorKey = (authorId, simId) => `simgone-author:${authorId}:${simId}`;
export const tombstoneFilesPrefix = (simId) => `tombstones/${simId}/`;
/// What the hard delete removes from R2: exactly these prefixes, with `{id}` a validated UUID, minus the review evidence.
export const removedPrefixes = (simId) => [`universes/${simId}/`, `thumbnails/${simId}/`, `covers/${simId}/`];
export const keptReviewPrefix = (simId) => `universes/${simId}/moderation/`;

const NOT_FOUND = Object.freeze({ error: 'Simulation not found' });
const UNAVAILABLE = Object.freeze({ error: 'Deleting or restoring a published Space is not available yet.', code: 'delete_unavailable' });
const BUSY = Object.freeze({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' });
const DELETED_COPY = 'This Space is deleted. Restore it from Projects to change it.';
const REMOVED_COPY = 'This Space was deleted and is being removed for good. It cannot be changed.';

const enc = new TextEncoder();
const hexOf = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
const iso = (ms) => new Date(ms).toISOString();
const parse = (raw) => { try { return raw ? JSON.parse(raw) : null; } catch { return null; } };
const nowOf = (deps) => (typeof deps?.now === 'function' ? deps.now() : Date.now());
const JSON_META = { httpMetadata: { contentType: 'application/json' } };

export const deleteCodeEmailOn = (env) => env?.DELETE_CODE_EMAIL === 'on';

/// The sender of the code mail and of the deleted and restored notices: DELETE_MAIL_FROM, else support@eustress.dev, shown as
/// "Eustress Support". No other mail takes this setting.
const DELETE_MAIL_FROM_DEFAULT = 'support@eustress.dev';
const deleteMailSender = (env) => ({ from: env?.DELETE_MAIL_FROM || DELETE_MAIL_FROM_DEFAULT, fromName: 'Eustress Support' });

/// The holds that are only a retention marker left behind (a released hold's preserve_until or never_delete, a report owed or
/// stuck, a pending seal, evidence kept in place). Alone they refuse a deletion with the neutral copy below, not the in_review
/// one: the listing serves and takes writes as usual, so "being reviewed" would tell the author a hold was once placed.
const RETENTION_ONLY = ['preserve_until', 'never_delete', 'report_owed', 'report_stuck', 'vault_pending', 'evidence_in_place'];
export const CANNOT_DELETE_NOW = { error: "This Space can't be deleted right now. Contact support@eustress.dev.", code: 'cannot_delete_now' };

/// The answer a deletion start gets for these hold reasons: null (none), CANNOT_DELETE_NOW (only retention markers) or the
/// in_review answer (any live hold, or anything unreadable).
const holdAnswer = (reasons) => (!reasons.length ? null : reasons.every((r) => RETENTION_ONLY.includes(r)) ? 'cannot_delete_now' : 'in_review');

async function sha256Hex(text) {
  return hexOf(await crypto.subtle.digest('SHA-256', enc.encode(text)));
}

/// The typed name against the listing's: NFC, trimmed, case sensitive, and not empty.
export function nameMatches(typed, name) {
  if (typeof typed !== 'string' || typeof name !== 'string') return false;
  const t = typed.normalize('NFC').trim();
  return t.length > 0 && t === name.normalize('NFC').trim();
}
const nameDigest = (name) => sha256Hex(`eustress-space-name:${String(name).normalize('NFC').trim()}`);

/// Six digits, every value equally likely: a 32-bit draw at or above 4,294,000,000 is drawn again, so the remainder by a
/// million has no bias.
export function drawCode() {
  const buf = new Uint32Array(1);
  for (;;) {
    crypto.getRandomValues(buf);
    if (buf[0] < 4_294_000_000) return String(buf[0] % 1_000_000).padStart(6, '0');
  }
}

const BASE62 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
/// `sdr_` and 24 base62 characters, unbiased (a byte at or above 248 is skipped).
export function newRequestId() {
  let out = '';
  while (out.length < 24) {
    for (const b of crypto.getRandomValues(new Uint8Array(32))) if (b < 248 && out.length < 24) out += BASE62[b % 62];
  }
  return `sdr_${out}`;
}

let codeKeyCache = { secret: null, key: null };
async function codeKeyOf(env) {
  const secret = env?.JWT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  if (codeKeyCache.secret !== secret) {
    const outer = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const raw = new Uint8Array(await crypto.subtle.sign('HMAC', outer, enc.encode(DELETE_CODE_KEY_LABEL)));
    codeKeyCache = { secret, key: await crypto.subtle.importKey('raw', raw, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']) };
  }
  return codeKeyCache.key;
}

/// The digest the account's object keeps for a code: HMAC-SHA-256 under a key derived from JWT_SECRET, over the account,
/// the listing, the request and the code, so a digest is good for one request of one listing of one account. Null when
/// the deployment has no secret.
export async function deleteCodeDigest(env, { accountId, simId, requestId, code }) {
  const key = await codeKeyOf(env);
  if (!key) return null;
  return hexOf(await crypto.subtle.sign('HMAC', key, enc.encode(`${accountId}|${simId}|${requestId}|${String(code ?? '')}`)));
}

/// The session a request came with, as a digest: a confirm must come from the session that asked.
async function sessionDigest(request) {
  const auth = request.headers.get('Authorization') || '';
  return sha256Hex(`eustress-session:${auth.startsWith('Bearer ') ? auth.slice(7) : ''}`);
}

async function readBody(request) {
  try {
    const v = await request.json();
    return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
  } catch {
    return {};
  }
}

async function ownedListing(env, simId, accountId) {
  const sim = parse(await env.SOCIAL.get(`sim:${simId}`));
  return sim && sim.author_id === accountId ? { ...sim, id: simId } : null;
}

/// One record in AUDIT_LOG, the shape index.js auditLog writes, kept five years. Never a code, never an address.
async function audit(env, action, actor, target, details) {
  if (!env?.AUDIT_LOG) return;
  try {
    const entry = { action, admin_id: details?.admin_id || null, actor, target, details, timestamp: new Date().toISOString() };
    await env.AUDIT_LOG.put(`audit:${Date.now()}-${crypto.randomUUID().slice(0, 8)}`, JSON.stringify(entry), { expirationTtl: 86400 * 365 * 5 });
  } catch (e) {
    console.error('space delete: the audit record was not written:', e?.name || 'Error');
  }
}

// -----------------------------------------------------------------------------
// What stops a deletion
// -----------------------------------------------------------------------------

const hasContent = (sim) => !!(sim && (sim.r2_key || sim.format || sim.world || sim.pak_etag || (sim.spaces && Object.keys(sim.spaces).length)));
const OPEN_REVIEW = ['pending', 'classifying', 'held'];
const OPEN_MARKERS = ['pending', 'classifying', 'held', 'appealed'];

async function anyKey(kv, prefix, accept = () => true) {
  let cursor;
  do {
    const page = await kv.list({ prefix, limit: 1000, ...(cursor ? { cursor } : {}) });
    if ((page?.keys || []).some((k) => k.name.startsWith(prefix) && accept(k.name))) return true;
    cursor = page?.list_complete === false ? page.cursor : undefined;
  } while (cursor);
  return false;
}

/// Every reason a listing may not be deleted now, or its removal must wait, as codes for staff: [] when there is none.
/// The author is never shown these, only the `in_review` copy. Read from the case, the strongly consistent R2 locks, the
/// report debts, the evidence vault, takedowns, the Review Workspace and the staff hold. Anything unreadable holds.
export async function holdsOf(env, sim, { simId, now = Date.now() } = {}) {
  const id = simId || sim?.id;
  const reasons = [];
  const add = (r) => { if (!reasons.includes(r)) reasons.push(r); };
  if (await legalLockOf(env, id)) add('legal_lock');
  if (sim?.moderation?.status === 'quarantined') add('quarantine');
  let rec = null;
  try { rec = await loadCase(env, id); } catch { add('case_unreadable'); }
  if (rec) {
    if (rec.status === 'quarantined') add('quarantine');
    if (caseLocksContent(rec)) add('case_hold');
    const preserve = [rec.legal_hold?.preserve_until, rec.legal_release?.preserve_until].map((v) => Date.parse(v)).filter(Number.isFinite);
    if (preserve.some((t) => t > now)) add('preserve_until');
    if (rec.legal_hold?.never_delete || rec.legal_release?.never_delete) add('never_delete');
    if (rec.report_owed) add('report_owed');
    if (OPEN_REVIEW.includes(rec.status) || rec.appeal?.status === 'pending') add('review_open');
  }
  // Content that changed and was not reviewed yet is reviewed before it can go (D5: wait).
  if (hasContent(sim) && OPEN_MARKERS.includes(sim?.moderation?.status)) add('review_open');
  if (await env.SOCIAL.get(listingReportOwedKey(id))) add('report_owed');
  const stuck = `legalstuck:moderation:${id}`;
  if (await anyKey(env.SOCIAL, stuck, (name) => name === stuck || name.startsWith(`${stuck}:`))) add('report_stuck');
  if (await anyKey(env.SOCIAL, `${VAULT_PENDING_PREFIX}moderation:${id}:`)) add('vault_pending');
  // TAKE IT DOWN requests naming the listing: one not found invalid or closed holds it, and so does its failing seal. Each
  // one's case reference is kept for the evidence read below: a takedown seals the listing's files under its own reference.
  const refs = [await caseReferenceOf(env, 'moderation', id)];
  let cursor;
  do {
    const page = await env.SOCIAL.list({ prefix: `tdlisting:${id}:`, limit: 1000, ...(cursor ? { cursor } : {}) });
    for (const k of page?.keys || []) {
      const tdId = k.name.slice(`tdlisting:${id}:`.length);
      const td = parse(await env.SOCIAL.get(`tdcase:${tdId}`));
      if (!td || !['invalid', 'closed'].includes(td.status)) add('takedown_open');
      if (/^[A-Za-z0-9-]{1,64}$/.test(tdId)) {
        if (await anyKey(env.SOCIAL, `${VAULT_PENDING_PREFIX}takedown:${tdId}:`)) add('vault_pending');
        refs.push(await caseReferenceOf(env, 'takedown', tdId));
      }
    }
    cursor = page?.list_complete === false ? page.cursor : undefined;
  } while (cursor);
  // Evidence sealed under the moderation case's reference and under every takedown's naming the listing (a possible-CSAM
  // TAKE IT DOWN seals under its own): an item kept in place (not copied into the vault) is the listing's own bytes, and an
  // item never to be deleted keeps the listing for good, whatever the moderation case says later.
  for (const ref of [...new Set(refs.filter(Boolean))]) {
    let c;
    do {
      const page = await env.SCENES.list({ prefix: `${VAULT_PREFIX}${ref}/`, limit: 1000, ...(c ? { cursor: c } : {}) });
      for (const o of page?.objects || []) {
        if (!o.key.startsWith(`${VAULT_PREFIX}${ref}/`) || !o.key.endsWith('.json')) continue;
        const obj = await env.SCENES.get(o.key);
        const index = obj ? parse(await obj.text()) : null;
        if (!index || index.copied === false) add('evidence_in_place');
        if (index?.never_delete) add('never_delete');
      }
      c = page?.truncated ? page.cursor : undefined;
    } while (c);
  }
  for (const kind of ['hold', 'quarantine', 'appeal']) if (await env.SOCIAL.get(workspaceQueueKey(kind, id))) add('workspace_open');
  if (await env.SCENES.head(staffHoldKey(id))) add('staff_hold');
  return reasons;
}

/// Why money stops the deletion, in plain words, or null: a product of the listing still on sale, or a live sale whose
/// 72-hour refund window is still open. `{ unavailable: true }` when the creator's hub cannot be asked.
export async function moneyBlockOf(env, sim, now, timeZone = null) {
  if (!env?.COMMERCE_HUBS) return null;
  try {
    const hub = hubStub(env, sim.author_id);
    const products = await callObject(hub, 'list_products', { account_id: sim.author_id, sim_id: sim.id, all: true });
    if (!products.ok) return { unavailable: true };
    const onSale = (products.data || []).filter((p) => p.active).length;
    if (onSale) return { code: 'products_on_sale', error: 'Take every product in this Space off sale first.', products_on_sale: onSale };
    // Live sales, newest first. Test-mode sales move no money and have no refund to protect.
    let latest = 0;
    let after;
    for (let page = 0; page < 1000; page++) {
      const r = await callObject(hub, 'list_sales', { account_id: sim.author_id, livemode: true, sim_id: sim.id, limit: 100, ...(after ? { starting_after: after } : {}) });
      if (!r.ok) return { unavailable: true };
      const data = r.data || [];
      let older = false;
      for (const s of data) {
        const closes = Number(s.created) * 1000 + REFUND_WINDOW_MS;
        if (closes <= now) { older = true; continue; }
        if (s.status !== 'refunded') latest = Math.max(latest, closes);
      }
      if (older || !r.has_more || !data.length) break;
      after = data[data.length - 1].id;
    }
    if (latest) {
      return { code: 'refund_window_open', error: `You can delete this Space after ${formatWhen(latest, timeZone)}, when the last refund window closes.`, until: iso(latest) };
    }
    return null;
  } catch (e) {
    console.error('space delete: the creator\'s hub could not be asked:', e?.name || 'Error');
    return { unavailable: true };
  }
}

/// The lease is free: nothing is reviewing, sealing or taking down this listing right now. Taken and given back.
async function leaseFree(env, simId) {
  const lease = await acquireLease(env, simId, thresholdsFrom(env), 'space_delete');
  if (!lease) return false;
  await releaseLease(env, lease);
  return true;
}

// -----------------------------------------------------------------------------
// Mail
// -----------------------------------------------------------------------------

/// The listing's name as a notice shows it: the name is the author's own text (or a session thief's, after a rename), so it
/// is cut short and any `scheme://` is broken, so no mail client turns it into a link inside a genuine Account Notice.
export const NOTICE_NAME_MAX = 60;
export function noticeName(name) {
  const flat = String(name ?? '').replace(/[\u0000-\u001f\u007f]+/g, ' ').trim();
  const short = flat.length > NOTICE_NAME_MAX ? `${flat.slice(0, NOTICE_NAME_MAX - 3)}...` : flat;
  return short.split('://').join(': //');
}

/// The code mail. Neither the subject nor the body names the Space: its name is author-chosen text, and this mail is the
/// signed Security Notice that carries the code (the page that asked shows which Space). The code is in the body only.
export function renderCodeEmail({ code, action, expiresAt, timeZone = null, assetOrigin }) {
  const verb = action === 'restore' ? 'restore' : 'delete';
  const until = formatWhen(expiresAt, timeZone);
  const text = [
    `Your code to ${verb} a Space on your Eustress account:`,
    '',
    code,
    '',
    `It works once, until ${until}. Type it on the page where you asked for it. Eustress never asks for this code by email, chat or phone.`,
    '',
    `If you did not ask to ${verb} a Space, someone is signed in to your account. Sign in with your identity file, replace your identity key in Settings, Security, and write to ${SUPPORT} from any address.`,
  ].join('\n');
  const body = html`${paragraph(`Your code to ${verb} a Space on your Eustress account:`)}
${facts([['Code', code], ['Works until', until]])}
${paragraph('It works once. Type it on the page where you asked for it. Eustress never asks for this code by email, chat or phone.')}
${note(`If you did not ask to ${verb} a Space, someone is signed in to your account. Sign in with your identity file, replace your identity key in Settings, Security, and write to ${SUPPORT} from any address.`)}`;
  const email = renderEmail({
    title: 'Your Eustress confirmation code', heading: 'Your confirmation code', label: 'Security Notice',
    preheader: 'A confirmation code for your Eustress account.', body, text, footer: 'plain', assetOrigin,
  });
  return { subject: 'Your Eustress confirmation code', text: email.text, html: email.html };
}

/// The notice that a Space was deleted or restored, to every address the account has. No link that acts.
export function renderSpaceNotice(kind, { name: rawName, at, hardDeleteAfter, timeZone = null, assetOrigin }) {
  const name = noticeName(rawName);
  const deleted = kind === 'space_deleted';
  const when = formatWhen(at, timeZone);
  const lines = deleted
    ? [
      `The Space "${name}" on your Eustress account was deleted on ${when}. It is hidden from everyone now.`,
      '',
      `It will be removed for good on ${formatWhen(hardDeleteAfter, timeZone)}. Until then you can restore it: sign in, open Projects, and choose Restore.`,
      '',
      `If you did not do this, someone is signed in to your account. Sign in with your identity file, restore the Space, replace your identity key in Settings, Security, and write to ${SUPPORT} from any address.`,
    ]
    : [
      `The Space "${name}" on your Eustress account was restored on ${when}. It is back as it was before it was deleted.`,
      '',
      `If you did not do this, write to ${SUPPORT} from any address and say your username.`,
    ];
  const text = lines.join('\n');
  const body = deleted
    ? html`${paragraph(html`The Space ${strong(name)} on your Eustress account was deleted on ${when}. It is hidden from everyone now.`)}
${paragraph(`It will be removed for good on ${formatWhen(hardDeleteAfter, timeZone)}. Until then you can restore it: sign in, open Projects, and choose Restore.`)}
${note(`If you did not do this, someone is signed in to your account. Sign in with your identity file, restore the Space, replace your identity key in Settings, Security, and write to ${SUPPORT} from any address.`)}`
    : html`${paragraph(html`The Space ${strong(name)} on your Eustress account was restored on ${when}. It is back as it was before it was deleted.`)}
${note(`If you did not do this, write to ${SUPPORT} from any address and say your username.`)}`;
  const title = deleted ? 'A Space on your Eustress account was deleted' : 'A Space on your Eustress account was restored';
  const email = renderEmail({ title, heading: title, label: 'Account Notice', preheader: `${title}.`, body, text, footer: 'plain', assetOrigin });
  return { subject: title, text: email.text, html: email.html };
}

/// The deleted or restored notice to every address the account has. Best effort: a notice that fails changes nothing.
async function sendSpaceNotice(env, accountId, kind, data, now) {
  try {
    const r = await accountCall(env, accountId, 'notice_recipients', { now });
    if (!r.ok || !Array.isArray(r.recipients) || !r.recipients.length) return;
    await sendNotice(env, r.recipients, renderSpaceNotice(kind, { ...data, timeZone: await accountTimeZone(env, accountId) }), { kind: 'notice', accountId, ...deleteMailSender(env) });
  } catch (e) {
    console.error('space delete: the notice was not sent:', e?.name || 'Error');
  }
}

// -----------------------------------------------------------------------------
// Answers
// -----------------------------------------------------------------------------

function rateLimited(json, cors, retryAfter) {
  const after = Math.max(1, Math.ceil(Number(retryAfter) || 60));
  return json({ error: 'Too many deletion requests. Try again later.', code: 'rate_limited', retry_after: after }, 429, { ...cors, 'Retry-After': String(after) });
}

/// An answer of the account's object, in the words the owner sees. Nothing the object said is passed through as text.
function fromObject(json, cors, r) {
  switch (r?.code) {
    case 'no_verified_email':
      return json({ error: 'This account has no verified email address old enough to receive a code. Verify your email address in Settings; a code can be sent to it 30 days after it is verified.', code: 'no_verified_email' }, 409, cors);
    case 'rate_limited': return rateLimited(json, cors, r.retry_after);
    case 'request_invalid': return json({ error: 'This confirmation is not valid. Ask for a new code.', code: 'request_invalid', ...(Number.isFinite(r.attempts_left) ? { attempts_left: r.attempts_left } : {}) }, 400, cors);
    case 'code_invalid': return json({ error: 'That code is not right.', code: 'code_invalid', attempts_left: Number(r.attempts_left) || 0, ...(r.locked ? { locked: true } : {}) }, 401, cors);
    case 'request_locked': return json({ error: 'Too many wrong codes. Ask for a new code.', code: 'request_locked', attempts_left: 0 }, 409, cors);
    case 'request_expired': return json({ error: 'This code has expired. Ask for a new one.', code: 'request_expired' }, 410, cors);
    case 'request_used': return json({ error: 'This code was already used.', code: 'request_used' }, 409, cors);
    case 'name_changed': return json({ error: 'This Space was renamed after the code was sent. Ask for a new code.', code: 'name_changed', ...(Number.isFinite(r.attempts_left) ? { attempts_left: r.attempts_left } : {}) }, 409, cors);
    default: return json(BUSY, 503, cors);
  }
}

/// The pause after a key replacement (decision D2, the document's recommendation): deleting is refused while the
/// account's value is frozen, so a thief who replaced the key cannot delete the owner's Spaces. Null when not frozen.
async function frozenRefusal(env, accountId, now, json, cors) {
  const held = await accountState(env, accountId);
  if (!held.ok) return json(BUSY, 503, cors);
  if (held.value_frozen_until > now) {
    return json({ error: `Deleting a Space is paused until ${formatWhen(held.value_frozen_until, await accountTimeZone(env, accountId))}, after the identity key was replaced.`, code: 'account_frozen', until: iso(held.value_frozen_until) }, 423, cors);
  }
  return null;
}

/// What stops a deletion from starting, as the answer the owner gets, or null. Holds first (one generic copy), then the
/// lease (a review, a takedown or a seal is writing the case right now: the same copy), then money.
async function startRefusal(env, sim, now, json, cors, { lease = true, timeZone = null } = {}) {
  const held = holdAnswer(await holdsOf(env, sim, { now }));
  if (held === 'cannot_delete_now') return json(CANNOT_DELETE_NOW, 409, cors);
  if (held) return inReview(json, cors);
  if (lease && !(await leaseFree(env, sim.id))) return inReview(json, cors);
  const money = await moneyBlockOf(env, sim, now, timeZone);
  if (money?.unavailable) return json(BUSY, 503, cors);
  if (money) return json(money, 409, cors);
  return null;
}

/// The mark's view with what only a reader at `now` knows: whether restoring is still possible, and whether the removal
/// is past its date (the Projects card then reads "Removal is in progress", true whatever the cause).
function deletionAt(view, now) {
  if (!view) return null;
  const due = Date.parse(view.hard_delete_after);
  return { ...view, restore_until: view.state === 'soft' && Number.isFinite(due) && now < due ? view.restore_until : null, removal_in_progress: view.state !== 'soft' || (Number.isFinite(due) && now >= due) };
}

// -----------------------------------------------------------------------------
// The soft delete and the restore
// -----------------------------------------------------------------------------

async function stopHosting(env, simId, now) {
  for (const name of [simId, guestSessionName(simId)]) {
    try {
      if (!env.LIVE_HOSTS) break;
      const stub = env.LIVE_HOSTS.get(env.LIVE_HOSTS.idFromName(name));
      await stub.fetch('https://live.internal/stop', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ now }) });
    } catch (e) {
      console.error('space delete: a live session did not stop:', e?.name || 'Error');
    }
  }
  await tellLiveIndex(env, 'stop', { sim_id: simId });
  await tellLiveIndex(env, 'stop', { sim_id: simId }, GUEST_LIVE_INDEX_NAME);
}

function markMeta(mark) {
  return {
    state: mark.state, author: mark.author_id, soft_deleted_at: mark.soft_deleted_at, hard_delete_after: mark.hard_delete_after,
    request_id: mark.request_id || '',
  };
}

/// The mark rewritten whole. Only under the moderation lease (the job), so it cannot race the restore, which takes it too.
async function writeMark(env, mark) {
  await env.SCENES.put(deletionMarkKey(mark.sim_id), JSON.stringify(mark), { ...JSON_META, customMetadata: markMeta(mark) });
}

/// Hide the listing, in an order every step of which can run again: the due index first (so a mark always has one), then
/// the mark (create-if-absent: from here the listing is deleted), then hosting, the audit and the notice. Nothing in the
/// listing's KV record changes.
async function softDelete(env, sim, { accountId, requestId, now }) {
  const hardAfter = now + SOFT_DELETE_MS;
  const mark = {
    v: 1, sim_id: sim.id, author_id: sim.author_id, name: sim.name || '', state: 'soft', request_id: requestId, factor: 'email',
    soft_deleted_at: iso(now), hard_delete_after: iso(hardAfter), was_public: sim.is_public !== false, was_archived: sim.archived === true,
    deferred: null, progress: null,
  };
  await env.SOCIAL.put(dueKey(hardAfter, sim.id), JSON.stringify({ sim_id: sim.id, author_id: sim.author_id, request_id: requestId, hard_delete_after: mark.hard_delete_after }));
  const made = await env.SCENES.put(deletionMarkKey(sim.id), JSON.stringify(mark), { ...JSON_META, customMetadata: markMeta(mark), onlyIf: { etagDoesNotMatch: '*' } });
  let held = mark;
  if (!made) {
    held = await readDeletionMark(env, sim.id);
    if (held?.request_id !== requestId) {
      // Another deletion of this listing won. Its own due key stands; this one is dropped, unless both fall in the same
      // hour, when they are the same key and deleting it would leave the winner with none.
      const winnerDue = Date.parse(held?.hard_delete_after);
      if (!Number.isFinite(winnerDue) || dueKey(winnerDue, sim.id) !== dueKey(hardAfter, sim.id)) {
        await env.SOCIAL.delete(dueKey(hardAfter, sim.id)).catch(() => {});
      }
      return { conflict: true };
    }
  }
  await finishSoftDelete(env, held, { accountId, now });
  return { mark: held };
}

/// The steps after the mark, each safe to repeat; the audit and the notice are written once (the object says whether it
/// already recorded this request).
async function finishSoftDelete(env, mark, { accountId, now }) {
  await stopHosting(env, mark.sim_id, now);
  const applied = await accountCall(env, accountId, 'space_delete_applied', { sim_id: mark.sim_id, request_id: mark.request_id, hard_delete_after: mark.hard_delete_after, now });
  try { await legalLockedIds(env, 0); } catch { /* this isolate's list refreshes within 10 seconds anyway */ }
  if (applied.ok && !applied.already) {
    await audit(env, 'SPACE_SOFT_DELETED', accountId, mark.sim_id, { request_id: mark.request_id, hard_delete_after: mark.hard_delete_after, factor: 'email' });
    await sendSpaceNotice(env, accountId, 'space_deleted', { name: mark.name, at: Date.parse(mark.soft_deleted_at), hardDeleteAfter: Date.parse(mark.hard_delete_after) }, now);
  }
}

function deletedBody(mark, now, extra = {}) {
  return {
    ok: true, state: 'soft_deleted', sim_id: mark.sim_id, hidden_at: mark.soft_deleted_at,
    hard_delete_after: mark.hard_delete_after, restore_until: deletionAt(deletionView(markMeta(mark)), now)?.restore_until || null, ...extra,
  };
}

// -----------------------------------------------------------------------------
// The owner's routes
// -----------------------------------------------------------------------------

const ROUTE = /^\/api\/account\/spaces\/([a-f0-9-]+)\/(delete-request|delete-confirm|restore-request|restore-confirm)$/;
const STATUS_ROUTE = /^\/api\/account\/spaces\/([a-f0-9-]+)\/deletion$/;
const STAFF_ACTION = /^\/api\/admin\/space-deletions\/([a-f0-9-]+)\/(hold|release-hold)$/;

/// Routes under /api/account/spaces/ and /api/admin/space-deletions. Null for a path that is not one of them.
/// `deps`: json, verifyAuth, verifyJwt, requireAdmin, and optionally now (a clock, for tests).
export async function handleSpaceDeleteRoute(request, url, env, ctx, cors, deps) {
  const path = url.pathname;
  let m;
  if ((m = path.match(ROUTE))) {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return ownerAction(request, env, cors, deps, m[1], m[2]);
  }
  if ((m = path.match(STATUS_ROUTE))) {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return ownerStatus(request, env, cors, deps, m[1]);
  }
  if (path === '/api/admin/space-deletions') {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return staffList(request, url, env, cors, deps);
  }
  if ((m = path.match(STAFF_ACTION))) {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return staffHold(request, env, cors, deps, m[1], m[2] === 'hold');
  }
  return null;
}

async function ownerAction(request, env, cors, deps, simId, step) {
  const json = deps.json;
  const now = nowOf(deps);
  const accountId = await verifyFreshAuth(request, env, deps);
  if (!accountId) return json({ error: 'Unauthorized' }, 401, cors);
  if (!deleteCodeEmailOn(env) || !env.ACCOUNT_AUTH || !env.EMAIL) return json(UNAVAILABLE, 503, cors);
  if (!SIM_ID.test(simId)) return json(NOT_FOUND, 404, cors);
  // One answer for a listing that does not exist and one that is not yours.
  const sim = await ownedListing(env, simId, accountId);
  if (!sim) return json(NOT_FOUND, 404, cors);
  if (env.AUTH_RATE_LIMITER) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `space-delete:${accountId}` });
    if (!success) return rateLimited(json, cors, 60);
  }
  const body = await readBody(request);
  if (!nameMatches(body.typed_name, sim.name)) return json({ error: 'The name you typed does not match this Space\'s name.', code: 'name_mismatch' }, 400, cors);
  const action = step.startsWith('delete') ? 'delete' : 'restore';
  if (action === 'delete') {
    const frozen = await frozenRefusal(env, accountId, now, json, cors);
    if (frozen) return frozen;
  }
  const timeZone = await accountTimeZone(env, accountId);
  const head = await deletionMarkHead(env, simId);
  const ctxt = { env, cors, json, deps, sim, accountId, now, timeZone, head, body, request };
  if (step === 'delete-request') return deleteRequest(ctxt);
  if (step === 'delete-confirm') return deleteConfirm(ctxt);
  if (step === 'restore-request') return restoreRequest(ctxt);
  return restoreConfirm(ctxt);
}

/// Open the request in the account's object and mail the code to the one address it names.
async function beginAndMail({ env, cors, json, sim, accountId, now, timeZone, request }, action) {
  const requestId = newRequestId();
  const code = drawCode();
  const codeDigest = await deleteCodeDigest(env, { accountId, simId: sim.id, requestId, code });
  if (!codeDigest) return json(UNAVAILABLE, 503, cors);
  const begun = await accountCall(env, accountId, 'space_delete_begin', {
    sim_id: sim.id, action, request_id: requestId, code_digest: codeDigest,
    name_sha256: await nameDigest(sim.name), session_sha256: await sessionDigest(request), now,
  });
  if (!begun.ok) return fromObject(json, cors, begun);
  const rendered = renderCodeEmail({ code, action, expiresAt: begun.expires_at, timeZone });
  const [result] = await sendNotice(env, [begun.recipient], rendered, { kind: 'notice', accountId, ...deleteMailSender(env) });
  if (!result?.sent) {
    await accountCall(env, accountId, 'space_delete_cancel', { sim_id: sim.id, request_id: requestId, reason: 'code_not_sent', now });
    return json({ error: 'We could not send the code. Try again in a few minutes.', code: 'code_not_sent' }, 503, cors);
  }
  return json({
    request_id: requestId, action, factor: 'email', expires_at: iso(begun.expires_at),
    sent_to: maskAddress(begun.recipient), attempts_left: begun.attempts_left,
  }, 200, cors);
}

/// The code the person typed, as the digest the object compares. A code that is not six digits still counts as an attempt.
async function typedDigest({ env, sim, accountId, body }) {
  const typed = typeof body.code === 'number' ? String(body.code).padStart(6, '0') : typeof body.code === 'string' ? body.code.trim() : '';
  return deleteCodeDigest(env, { accountId, simId: sim.id, requestId: body.request_id, code: typed });
}

async function confirmInObject(c, action) {
  const { env, sim, accountId, now, body, request } = c;
  return accountCall(env, accountId, 'space_delete_confirm', {
    sim_id: sim.id, action, request_id: body.request_id, code_digest: await typedDigest(c),
    name_sha256: await nameDigest(sim.name), session_sha256: await sessionDigest(request), now,
  });
}

async function deleteRequest(c) {
  const { env, cors, json, sim, now, timeZone, head } = c;
  if (head) return json({ error: 'This Space is already deleted.', code: 'already_deleted', restore_until: deletionAt(head.view, now)?.restore_until || null }, 409, cors);
  const refused = await startRefusal(env, sim, now, json, cors, { timeZone });
  if (refused) return refused;
  return beginAndMail(c, 'delete');
}

async function deleteConfirm(c) {
  const { env, cors, json, sim, accountId, now, timeZone, head, body } = c;
  if (typeof body.request_id !== 'string' || !REQUEST_ID.test(body.request_id)) return fromObject(json, cors, { code: 'request_invalid' });
  if (head) {
    // The same confirm again after a reply was lost: the object says whether it is the same request and code, and only
    // then are the steps after the mark run again. Anything else: the listing is already deleted.
    const mark = await readDeletionMark(env, sim.id);
    if (mark?.request_id !== body.request_id || mark.state !== 'soft') return json({ error: 'This Space is already deleted.', code: 'already_deleted', restore_until: deletionAt(head.view, now)?.restore_until || null }, 409, cors);
    const again = await confirmInObject(c, 'delete');
    if (!again.ok) return fromObject(json, cors, again);
    await finishSoftDelete(env, mark, { accountId, now });
    return json(deletedBody(mark, now, { replay: true }), 200, cors);
  }
  const refused = await startRefusal(env, sim, now, json, cors, { lease: false, timeZone });
  if (refused) return refused;
  const lease = await acquireLease(env, sim.id, thresholdsFrom(env), 'space_delete');
  if (!lease) return inReview(json, cors);
  try {
    // Under the lease, the strongly consistent lock once more: a hold placed since the checks above wins.
    if (await legalLockOf(env, sim.id)) return inReview(json, cors);
    const confirmed = await confirmInObject(c, 'delete');
    if (!confirmed.ok) return fromObject(json, cors, confirmed);
    const done = await softDelete(env, sim, { accountId, requestId: body.request_id, now });
    if (done.conflict) return json({ error: 'This Space is already deleted.', code: 'already_deleted' }, 409, cors);
    return json(deletedBody(done.mark, now, confirmed.replay ? { replay: true } : {}), 200, cors);
  } finally {
    await releaseLease(env, lease);
  }
}

/// Why a deleted listing cannot be restored now, or null.
function restoreClosed(json, cors, head, now) {
  if (!head) return json({ error: 'This Space is not deleted.', code: 'not_deleted' }, 409, cors);
  if (head.view.state !== 'soft') return json({ error: 'Removal of this Space has begun. It can no longer be restored.', code: 'removal_started' }, 410, cors);
  const due = Date.parse(head.view.hard_delete_after);
  if (!Number.isFinite(due) || now >= due) return json({ error: 'The time to restore this Space has passed.', code: 'restore_window_closed' }, 410, cors);
  return null;
}

async function restoreRequest(c) {
  const { env, cors, json, sim, now, head } = c;
  const closed = restoreClosed(json, cors, head, now);
  if (closed) return closed;
  // Restoring publishes the listing again: refused under any hold, like every other change to it.
  if ((await holdsOf(env, sim, { now })).length || !(await leaseFree(env, sim.id))) return inReview(json, cors);
  return beginAndMail(c, 'restore');
}

async function restoreConfirm(c) {
  const { env, cors, json, sim, accountId, now, head, body } = c;
  if (typeof body.request_id !== 'string' || !REQUEST_ID.test(body.request_id)) return fromObject(json, cors, { code: 'request_invalid' });
  if (!head) {
    // Already restored: the same confirm again is a replay, and nothing else is.
    const again = await confirmInObject(c, 'restore');
    if (again.ok && again.replay) return json({ ok: true, state: 'restored', sim_id: sim.id, replay: true }, 200, cors);
    if (!again.ok && again.code !== 'request_invalid') return fromObject(json, cors, again);
    return json({ error: 'This Space is not deleted.', code: 'not_deleted' }, 409, cors);
  }
  const closed = restoreClosed(json, cors, head, now);
  if (closed) return closed;
  if ((await holdsOf(env, sim, { now })).length) return inReview(json, cors);
  const lease = await acquireLease(env, sim.id, thresholdsFrom(env), 'space_delete');
  if (!lease) return inReview(json, cors);
  try {
    if (await legalLockOf(env, sim.id)) return inReview(json, cors);
    // The mark as it is now, under the lease: the hourly job takes the same lease, so it has not begun meanwhile.
    const mark = await readDeletionMark(env, sim.id);
    const fresh = mark ? { view: deletionView(markMeta(mark)) } : null;
    const late = restoreClosed(json, cors, fresh, now);
    if (late) return late;
    const confirmed = await confirmInObject(c, 'restore');
    if (!confirmed.ok) return fromObject(json, cors, confirmed);
    // The mark goes first: a failure after it leaves only a due key, which the job drops when it finds no mark.
    await env.SCENES.delete(deletionMarkKey(sim.id));
    const due = Date.parse(mark.hard_delete_after);
    if (Number.isFinite(due)) await env.SOCIAL.delete(dueKey(due, sim.id)).catch(() => {});
    const applied = await accountCall(env, accountId, 'space_delete_applied', { sim_id: sim.id, request_id: body.request_id, now });
    try { await legalLockedIds(env, 0); } catch { /* refreshes within 10 seconds */ }
    if (applied.ok && !applied.already) {
      await audit(env, 'SPACE_RESTORED', accountId, sim.id, { request_id: body.request_id, factor: 'email' });
      await sendSpaceNotice(env, accountId, 'space_restored', { name: sim.name, at: now }, now);
    }
    return json({ ok: true, state: 'restored', sim_id: sim.id, restored_at: iso(now) }, 200, cors);
  } finally {
    await releaseLease(env, lease);
  }
}

/// GET /api/account/spaces/{id}/deletion: what a deletion of this listing would do, whether one can start, and where an
/// existing one is. For the website's and Studio's modals.
async function ownerStatus(request, env, cors, deps, simId) {
  const json = deps.json;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const now = nowOf(deps);
  const accountId = await verifyFreshAuth(request, env, deps);
  if (!accountId) return json({ error: 'Unauthorized' }, 401, headers);
  if (!SIM_ID.test(simId)) return json(NOT_FOUND, 404, headers);
  const head = await deletionMarkHead(env, simId);
  const sim = await ownedListing(env, simId, accountId);
  if (!sim) {
    // Removed for good: the mark still names its author, who may see that it is gone.
    if (head && markAuthor(head.meta) === accountId) {
      const mark = await readDeletionMark(env, simId);
      return json({ sim_id: simId, name: mark?.name || null, state: head.view.state, deletion: deletionAt(head.view, now), pending: null, can_delete: false, can_restore: false, blocked: null, soft_delete_days: SOFT_DELETE_DAYS }, 200, headers);
    }
    return json(NOT_FOUND, 404, headers);
  }
  const available = deleteCodeEmailOn(env) && !!env.ACCOUNT_AUTH && !!env.EMAIL;
  const timeZone = await accountTimeZone(env, accountId);
  let pending = null;
  let sentTo = null;
  let hasAddress = false;
  if (env.ACCOUNT_AUTH) {
    const s = await accountCall(env, accountId, 'space_delete_status', { sim_id: simId, now });
    pending = s.ok ? s.pending : null;
    const e = await accountCall(env, accountId, 'email_status', { now });
    hasAddress = !!(e.ok && e.email?.verified && e.email?.aged);
    sentTo = hasAddress && e.email.on_file ? maskAddress(e.email.on_file) : null;
  }
  const deletion = deletionAt(head?.view || null, now);
  let blocked = null;
  let products = null;
  if (!available) blocked = { ...UNAVAILABLE };
  else if (!hasAddress) blocked = { code: 'no_verified_email', error: 'This account has no verified email address old enough to receive a code. Verify your email address in Settings; a code can be sent to it 30 days after it is verified.' };
  else if (head && head.view.state !== 'soft') blocked = { code: 'removal_started', error: 'Removal of this Space has begun. It can no longer be restored.' };
  else if (head && !deletion.restore_until) blocked = { code: 'restore_window_closed', error: 'The time to restore this Space has passed.' };
  else if (holdAnswer(await holdsOf(env, sim, { now })) === 'cannot_delete_now' && !head) blocked = { ...CANNOT_DELETE_NOW };
  else if ((await holdsOf(env, sim, { now })).length) blocked = { code: 'in_review', error: 'This listing is being reviewed and cannot be changed right now' };
  else if (!head) {
    const money = await moneyBlockOf(env, sim, now, timeZone);
    if (money?.unavailable) blocked = { code: 'account_unavailable', error: BUSY.error };
    else if (money) blocked = money;
  }
  if (env.COMMERCE_HUBS) {
    const r = await callObject(hubStub(env, sim.author_id), 'list_products', { account_id: sim.author_id, sim_id: simId, all: true }).catch(() => null);
    if (r?.ok) products = { total: (r.data || []).length, on_sale: (r.data || []).filter((p) => p.active).length };
  }
  const namespace = await env.SOCIAL.get(`website-ns-of:${simId}`);
  return json({
    sim_id: simId, name: sim.name || '', state: head ? (head.view.state === 'soft' ? 'soft_deleted' : head.view.state) : sim.archived === true ? 'archived' : sim.is_public !== false ? 'public' : 'private',
    deletion, pending,
    can_delete: !head && !blocked, can_restore: !!head && !blocked,
    blocked,
    email_code: { available, sent_to: sentTo, expires_minutes: 15, attempts: 5 },
    soft_delete_days: SOFT_DELETE_DAYS,
    removes: {
      listing: true, gallery_card: true, play_and_join_links: true, live_sessions: true, website_namespace: namespace || null,
      products, spaces: Array.isArray(sim.space_names) ? sim.space_names : Object.keys(sim.spaces || {}),
    },
    keeps: ['Your Universe files in Studio on your computer', 'Receipts and sales records', 'Review and case records'],
    outside_our_reach: ['Copies players already downloaded', 'Copies opened with Share Source', 'Websites that saved your manifest', 'Pages and pictures already in someone\'s browser, for up to a day'],
  }, 200, headers);
}

// -----------------------------------------------------------------------------
// Staff
// -----------------------------------------------------------------------------

async function staffList(request, url, env, cors, deps) {
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const admin = await deps.requireAdmin(request, env);
  if (!admin) return deps.json({ error: 'Admin access required' }, 403, headers);
  const want = url.searchParams.get('state');
  const marks = [];
  let cursor;
  do {
    const page = await env.SCENES.list({ prefix: DELETED_PREFIX, limit: 1000, ...(cursor ? { cursor } : {}) });
    for (const o of page?.objects || []) {
      if (marks.length >= 200 || !o.key.startsWith(DELETED_PREFIX) || !o.key.endsWith('.json')) continue;
      const mark = await readDeletionMark(env, o.key.slice(DELETED_PREFIX.length, -'.json'.length));
      if (!mark) continue;
      const state = mark.deferred && mark.state !== 'hard_deleted' ? 'deferred' : mark.state;
      if (want && want !== state && want !== mark.state) continue;
      marks.push({
        sim_id: mark.sim_id, author_id: mark.author_id, name: mark.name, state: mark.state, soft_deleted_at: mark.soft_deleted_at,
        hard_delete_after: mark.hard_delete_after, hard_deleted_at: mark.hard_deleted_at || null, deferred: mark.deferred || null,
        progress: mark.progress ? { phase: mark.progress.phase, objects: mark.progress.objects, bytes: mark.progress.bytes } : null,
        staff_hold: !!(await env.SCENES.head(staffHoldKey(mark.sim_id))),
      });
    }
    cursor = page?.truncated && marks.length < 200 ? page.cursor : undefined;
  } while (cursor);
  return deps.json({ deletions: marks }, 200, headers);
}

async function staffHold(request, env, cors, deps, simId, hold) {
  const admin = await deps.requireAdmin(request, env);
  if (!admin) return deps.json({ error: 'Admin access required' }, 403, cors);
  if (!SIM_ID.test(simId)) return deps.json({ error: 'Not a listing id', code: 'bad_request' }, 400, cors);
  const body = await readBody(request);
  const rationale = typeof body.rationale === 'string' ? body.rationale.trim().slice(0, 1000) : '';
  if (rationale.length < STAFF_RATIONALE_MIN) return deps.json({ error: `A rationale of at least ${STAFF_RATIONALE_MIN} characters is required.`, code: 'rationale_required' }, 400, cors);
  if (hold) {
    await env.SCENES.put(staffHoldKey(simId), JSON.stringify({ sim_id: simId, by: admin, at: new Date().toISOString(), rationale }), JSON_META);
  } else {
    await env.SCENES.delete(staffHoldKey(simId));
  }
  await audit(env, hold ? 'SPACE_DELETE_STAFF_HOLD' : 'SPACE_DELETE_STAFF_HOLD_RELEASED', admin, simId, { admin_id: admin, rationale });
  return deps.json({ ok: true, sim_id: simId, staff_hold: hold }, 200, cors);
}

// -----------------------------------------------------------------------------
// Every author write to a deleted listing
// -----------------------------------------------------------------------------

/// Run before the listing routes: a request that changes something under /api/simulations/{id} for a listing with a
/// deletion mark is answered here. Its author gets 409 listing_deleted (a hold answers `in_review` first, the same copy as
/// everywhere); anyone else gets the 404 of a listing that does not exist, and a host's heartbeat is also told to let every
/// guest go. Ending a live session and revoking a server credential still pass: they only wind the listing down.
/// `deps`: json, verifyAuth.
export async function spaceDeleteWriteGate(request, url, env, cors, deps) {
  if (!MUTATING.has(request.method)) return null;
  const parts = url.pathname.split('/');
  if (parts[1] !== 'api' || parts[2] !== 'simulations' || !SIM_ID.test(parts[3] || '')) return null;
  const simId = parts[3];
  const rest = parts.slice(4).join('/');
  if (request.method === 'DELETE' && (rest === 'live' || rest === 'server-credential')) return null;
  const head = await deletionMarkHead(env, simId);
  if (!head) return null;
  const extra = isLiveHeartbeat(request, url.pathname) ? END_ALL_GUESTS : {};
  const caller = await deps.verifyAuth(request, env);
  const author = markAuthor(head.meta);
  if (!caller || !author || caller !== author) return deps.json({ ...NOT_FOUND, ...extra }, 404, cors);
  const sim = parse(await env.SOCIAL.get(`sim:${simId}`));
  if (sim && (await contentLockedFor(env, { ...sim, id: simId }, caller))) {
    const held = await inReview(deps.json, cors).json();
    return deps.json({ ...held, ...extra }, 409, cors);
  }
  return deps.json({
    error: head.view.state === 'soft' ? DELETED_COPY : REMOVED_COPY, code: 'listing_deleted',
    restore_until: deletionAt(head.view, Date.now())?.restore_until || null, ...extra,
  }, 409, cors);
}

// -----------------------------------------------------------------------------
// The hourly job: removing a deleted listing for good
// -----------------------------------------------------------------------------

/// The removal for good is armed by its own setting, SPACE_HARD_DELETE, exactly "on"; anything else, or unset, is off.
/// Switching the code mail on never arms it: the hourly job reads this before anything else and, while it is off, reads
/// and removes nothing.
export const spaceHardDeleteOn = (env) => env?.SPACE_HARD_DELETE === 'on';

/// The bytes the job may read in one run to take the SHA-256 of files R2 holds no hash for (world chunks, and anything
/// stored without one), and the largest file hashed in memory where the platform cannot stream a digest.
export const SWEEP_HASH_BYTES = 512 * 1024 * 1024;
export const HASH_IN_MEMORY_MAX = 64 * 1024 * 1024;
/// At most this many file hashes are kept on the tombstone record itself, for a later TAKE IT DOWN (src/takedown.mjs).
export const TOMBSTONE_HASHES_MAX = 500;
/// The kinds of removed file a later TAKE IT DOWN blocks again: what src/takedown.mjs listingHashes reads from a listing
/// still there. A world chunk is named by its BLAKE3 and its upload is not checked against the blocklist; the world
/// manifest that names it is.
const TOMBSTONE_KINDS = new Set(['package', 'space_package', 'world_manifest', 'thumbnail', 'cover']);

/// Count one storage call; true when the run has spent its allowance or its time, or the lease it holds is about to run out.
const spent = (meter) => meter.calls >= meter.budget || Date.now() >= meter.deadline || (meter.leaseUntil !== undefined && Date.now() >= meter.leaseUntil);
const spend = (meter, n = 1) => { meter.calls += n; return spent(meter); };

/// Every listing whose 90 days are over, oldest first, removed in steps that resume where the last run stopped. A hold
/// defers it (the reason is written on the mark for staff); a busy lease skips it for an hour. `deps.now` is a clock for
/// tests. Returns counts.
export async function sweepSpaceDeletions(env, deps = {}, { now = nowOf(deps), budget = SWEEP_CALL_BUDGET, wallMs = SWEEP_WALL_MS } = {}) {
  const out = { due: 0, hard_deleted: 0, progressed: 0, deferred: {}, refiled: 0, busy: 0, dropped: 0, not_yet: 0, failed: 0, budget_spent: false, switched_off: false };
  if (!spaceHardDeleteOn(env)) { out.switched_off = true; return out; }
  if (!env?.SOCIAL?.list || !env?.SCENES?.list) return out;
  const meter = { calls: 0, budget, deadline: Date.now() + wallMs, hashed: 0 };
  const nowHour = iso(now).slice(0, 13);
  const due = [];
  let cursor;
  do {
    const page = await env.SOCIAL.list({ prefix: DUE_PREFIX, limit: 1000, ...(cursor ? { cursor } : {}) });
    spend(meter);
    for (const k of page?.keys || []) {
      const rest = k.name.slice(DUE_PREFIX.length);
      const hour = rest.slice(0, 13);
      const id = rest.slice(14);
      if (hour <= nowHour && SIM_ID.test(id)) due.push({ key: k.name, hour, id });
    }
    cursor = page?.list_complete === false ? page.cursor : undefined;
  } while (cursor && due.length < 1000);
  due.sort((a, b) => (a.hour < b.hour ? -1 : a.hour > b.hour ? 1 : 0));
  for (const d of due) {
    if (spent(meter)) { out.budget_spent = true; break; }
    out.due++;
    let r;
    try {
      r = await hardDeleteOne(env, d, now, meter);
    } catch (e) {
      console.error('space delete: a removal step failed; it resumes next hour:', e?.name || 'Error', String(e?.message || '').slice(0, 120));
      r = { failed: true };
    }
    if (r.dropped) out.dropped++;
    else if (r.not_yet) out.not_yet++;
    else if (r.busy) out.busy++;
    else if (r.deferred) out.deferred[r.deferred] = (out.deferred[r.deferred] || 0) + 1;
    else if (r.done) out.hard_deleted++;
    else if (r.progress) { out.progressed++; out.budget_spent = true; } // a listing stops part way only at the run's allowance
    else if (r.failed) out.failed++;
    // A listing that is held, only part removed, or failed goes to the back: its due key is filed again at the next hour,
    // so every listing due before it is reached first next run. Left where it was, a listing held for good would sit at
    // the head of the oldest-first index and spend the run's allowance there every hour.
    if (r.deferred || r.progress || r.failed) {
      try {
        if (await refile(env, d, now, r)) out.refiled++;
      } catch (e) {
        console.error('space delete: a due key was not filed again; it is tried again next hour:', e?.name || 'Error');
      }
      spend(meter, 2);
    }
  }
  return out;
}

async function refile(env, d, now, r) {
  const next = dueKey(now + HOUR, d.id);
  if (next === d.key) return false;
  await env.SOCIAL.put(next, JSON.stringify({ sim_id: d.id, refiled_at: iso(now), after: r.deferred ? 'deferred' : r.progress ? 'progress' : 'failed' }));
  await env.SOCIAL.delete(d.key);
  return true;
}

async function hardDeleteOne(env, d, now, meter) {
  const first = await readDeletionMark(env, d.id);
  spend(meter);
  if (!first || first.state === 'hard_deleted') {
    // Restored (no mark), or done: the due key has nothing left to do.
    await env.SOCIAL.delete(d.key);
    spend(meter);
    return { dropped: true };
  }
  if (first.state === 'soft' && !(now >= Date.parse(first.hard_delete_after))) return { not_yet: true };
  const lease = await acquireLease(env, d.id, thresholdsFrom(env), 'space_delete');
  spend(meter, 2);
  if (!lease) return { busy: true };
  // The lease is never extended, so the listing stops, its place saved, a minute before the lease would run out: past it a
  // review run or a takedown's case step may take the lease over.
  meter.leaseUntil = Date.now() + thresholdsFrom(env).lease_seconds * 1000 - MINUTE;
  try {
    // Read again under the lease: a restore that finished between the first read and the lease has deleted the mark, and a
    // mark written now would undo it.
    const mark = await readDeletionMark(env, d.id);
    spend(meter);
    if (!mark || mark.state === 'hard_deleted') {
      await env.SOCIAL.delete(d.key);
      return { dropped: true };
    }
    if (mark.unreadable) return { deferred: 'mark_unreadable' };
    const sim = parse(await env.SOCIAL.get(`sim:${d.id}`));
    const reasons = await holdsOf(env, sim ? { ...sim, id: d.id } : null, { simId: d.id, now });
    spend(meter, 12);
    // Defensive: no live sale can happen while the listing is deleted, but a refund window still open is never closed here.
    if (sim && mark.state === 'soft') {
      const money = await moneyBlockOf(env, { ...sim, id: d.id }, now);
      spend(meter, 2);
      if (money?.unavailable) reasons.push('commerce_unavailable');
      else if (money?.code === 'refund_window_open') reasons.push('refund_window_open');
    }
    // With the code mail off nobody can restore (every owner route answers 503), so nothing is removed while it is off,
    // whatever the stage: the 90 days exist for an owner whose account was used to delete their Space.
    if (!deleteCodeEmailOn(env)) reasons.push('restore_unavailable');
    if (reasons.length) return defer(env, mark, reasons, now);
    if (mark.state === 'soft') {
      mark.state = 'hard_deleting';
      mark.hard_deleting_at = iso(now);
      mark.progress = { phase: 'tombstone', prefix: 0, cursor: null, pages: 0, objects: 0, bytes: 0, counted: false };
    }
    mark.deferred = null;
    await writeMark(env, mark);
    spend(meter);
    return await runPhases(env, mark, sim, now, meter, d);
  } finally {
    delete meter.leaseUntil;
    await releaseLease(env, lease);
  }
}

async function defer(env, mark, reasons, now) {
  mark.deferred = { reason: reasons[0], reasons, since: mark.deferred?.since || iso(now), checked_at: iso(now) };
  await writeMark(env, mark);
  return { deferred: reasons[0] };
}

/// What a removed file is, for the tombstone's `file_hashes`: the kinds src/takedown.mjs listingHashes names.
export function removedFileKind(simId, key) {
  if (key === `universes/${simId}/universe.pak`) return 'package';
  if (key.startsWith(`universes/${simId}/spaces/`)) return 'space_package';
  if (key.startsWith(`universes/${simId}/manifests/`)) return 'world_manifest';
  if (key.startsWith(`universes/${simId}/chunks/`)) return 'chunk';
  if (key.startsWith(`thumbnails/${simId}/`)) return 'thumbnail';
  if (key.startsWith(`covers/${simId}/`)) return 'cover';
  return 'file';
}

const shaOf = (v) => (typeof v === 'string' ? v.toLowerCase() : hexOf(v));

/// `{ sha256, from }` of a file about to be removed. `from` says where the hash came from: `r2`, the SHA-256 R2 verified
/// when the file was stored with one (in the listing, or on the object read); `read`, the bytes hashed here, streamed where
/// the platform can (Workers' DigestStream) and in memory up to HASH_IN_MEMORY_MAX where it cannot. `sha256` is null only
/// for a file gone already (`gone`) or too large to hash in memory (`too_large`). A read counts one call and its bytes.
async function removedFileSha256(env, o, meter) {
  if (o.checksums?.sha256) return { sha256: shaOf(o.checksums.sha256), from: 'r2' };
  const object = await env.SCENES.get(o.key);
  spend(meter);
  if (!object) return { sha256: null, from: 'gone' };
  if (object.checksums?.sha256) {
    try { await object.body?.cancel?.(); } catch { /* nothing to give back */ }
    return { sha256: shaOf(object.checksums.sha256), from: 'r2' };
  }
  const size = Number(object.size) || 0;
  if (typeof crypto.DigestStream === 'function' && object.body) {
    const digest = new crypto.DigestStream('SHA-256');
    await object.body.pipeTo(digest);
    meter.hashed += size;
    return { sha256: hexOf(await digest.digest), from: 'read' };
  }
  if (size > HASH_IN_MEMORY_MAX) {
    try { await object.body?.cancel?.(); } catch { /* nothing to give back */ }
    return { sha256: null, from: 'too_large' };
  }
  meter.hashed += size;
  return { sha256: hexOf(await crypto.subtle.digest('SHA-256', await object.arrayBuffer())), from: 'read' };
}

async function runPhases(env, mark, sim, now, meter, d) {
  const id = mark.sim_id;
  const p = mark.progress;
  const save = async () => { await writeMark(env, mark); spend(meter); };

  if (p.phase === 'tombstone') {
    // First, so no case is ever orphaned: who, what, when, and where the file hashes go.
    const rec = await loadCase(env, id);
    const tomb = {
      v: 1, id, author_id: mark.author_id, name: mark.name || sim?.name || '', soft_deleted_at: mark.soft_deleted_at, deleted_at: mark.hard_deleting_at || iso(now),
      request_id: mark.request_id, case_reference: rec ? await assignCaseReference(env, 'moderation', id) : null, was_public: mark.was_public,
      format: sim?.format || null, version: sim?.version || null, space_names: Array.isArray(sim?.space_names) ? sim.space_names : [],
      published_at: sim?.published_at || null, content_root: sim?.content_root || null, pak_sha256: sim?.pak_sha256 || null,
      world_manifest_sha256: sim?.world?.manifest_sha256 || null, source_manifest_sha256: sim?.world?.source_manifest_sha256 || null,
      space_sha256: Object.values(sim?.spaces || {}).map((s) => s?.sha256).filter((h) => typeof h === 'string' && /^[0-9a-f]{64}$/.test(h)).slice(0, 50),
      website_namespace: (await env.SOCIAL.get(`website-ns-of:${id}`)) || null,
      // Every removed object's key, size and SHA-256 is written page by page before that page is deleted, and the hashes a
      // later TAKE IT DOWN blocks again are copied here (`file_hashes`) once the files are gone.
      files_prefix: tombstoneFilesPrefix(id), file_hashes: null,
    };
    await env.SOCIAL.put(tombstoneKey(id), JSON.stringify(tomb));
    await env.SOCIAL.put(tombstoneAuthorKey(mark.author_id, id), id);
    spend(meter, 5);
    p.phase = 'case';
    await save();
  }

  if (p.phase === 'case') {
    // The case stays, stamped. Written under the lease the job holds, as every case write is.
    const rec = await loadCase(env, id);
    if (rec && !rec.listing_deleted) {
      rec.listing_deleted = { at: mark.hard_deleting_at || iso(now), request_id: mark.request_id };
      await env.SOCIAL.put(caseKey(id), JSON.stringify(rec));
    }
    spend(meter, 2);
    p.phase = 'commerce';
    await save();
  }

  if (p.phase === 'commerce') {
    // Products off sale; product and sale records stay (sales name them). Nothing in any wallet is touched.
    if (env.COMMERCE_HUBS && mark.author_id) {
      const hub = hubStub(env, mark.author_id);
      const r = await callObject(hub, 'list_products', { account_id: mark.author_id, sim_id: id, all: true });
      if (!r.ok) throw new Error('the product list could not be read');
      for (const product of r.data || []) {
        if (!product.active) continue;
        const u = await callObject(hub, 'update_product', { account_id: mark.author_id, id: product.id, patch: { active: false } });
        if (!u.ok) throw new Error('a product could not be taken off sale');
      }
      spend(meter, 2 + (r.data || []).length);
    }
    p.phase = 'live';
    await save();
  }

  if (p.phase === 'live') {
    await stopHosting(env, id, now);
    const hash = await env.SOCIAL.get(`srvcred-sim:${id}`);
    if (typeof hash === 'string' && /^[0-9a-f]{64}$/.test(hash)) await env.SOCIAL.delete(`srvcred:${hash}`);
    await env.SOCIAL.delete(`srvcred-sim:${id}`);
    spend(meter, 6);
    p.phase = 'r2';
    p.prefix = 0;
    p.cursor = null;
    await save();
  }

  if (p.phase === 'r2') {
    const prefixes = removedPrefixes(id);
    const kept = keptReviewPrefix(id);
    while (p.prefix < prefixes.length) {
      if (spent(meter)) { await save(); return { progress: true }; }
      const prefix = prefixes[p.prefix];
      const page = await env.SCENES.list({ prefix, limit: 1000, ...(p.cursor ? { cursor: p.cursor } : {}) });
      spend(meter);
      const doomed = (page?.objects || []).filter((o) => typeof o?.key === 'string' && o.key.startsWith(prefix) && !o.key.startsWith(kept));
      if (doomed.length) {
        // The legal lock, and a staff hold, read again before every delete call: one that appeared stops the job here and
        // leaves what is left.
        const locked = await legalLockOf(env, id);
        const staff = await env.SCENES.head(staffHoldKey(id));
        spend(meter, 2);
        if (locked || staff) return defer(env, mark, [locked ? 'legal_lock' : 'staff_hold'], now);
        // The SHA-256 of every file before it goes: the one R2 verified and keeps (stored with one), or the bytes hashed
        // here. Bounded by the run's calls and SWEEP_HASH_BYTES: at least one file a page, and a page not finished stops the
        // run with its cursor unchanged, so the next run lists the same page again and finds only what is left of it.
        const files = [];
        for (const o of doomed) {
          if (files.length && (spent(meter) || meter.hashed >= SWEEP_HASH_BYTES)) break;
          const h = await removedFileSha256(env, o, meter);
          const size = Number(o.size) || 0;
          files.push({ key: o.key, size, sha256: h.sha256, sha256_from: h.from, etag: o.etag || null });
          const kind = removedFileKind(id, o.key);
          p.hashes = Array.isArray(p.hashes) ? p.hashes : [];
          if (h.sha256 && TOMBSTONE_KINDS.has(kind) && !p.hashes.some((x) => x.sha256 === h.sha256)) {
            if (p.hashes.length < TOMBSTONE_HASHES_MAX) p.hashes.push({ sha256: h.sha256, kind, size });
            else p.hashes_truncated = true;
          }
        }
        await env.SCENES.put(`${tombstoneFilesPrefix(id)}files-${String(p.pages).padStart(5, '0')}.json`, JSON.stringify({ v: 1, sim_id: id, prefix, files }), JSON_META);
        await env.SCENES.delete(files.map((f) => f.key));
        spend(meter, 2);
        p.pages += 1;
        p.objects += files.length;
        p.bytes += files.reduce((n, f) => n + f.size, 0);
        if (files.length < doomed.length) { await save(); return { progress: true }; }
      }
      p.cursor = page?.truncated ? page.cursor : null;
      if (!p.cursor) p.prefix += 1;
      await save();
    }
    // The hashes a later TAKE IT DOWN blocks again, on the tombstone record, so the request reads one record for them.
    const tomb = parse(await env.SOCIAL.get(tombstoneKey(id)));
    if (tomb) {
      tomb.file_hashes = Array.isArray(p.hashes) ? p.hashes : [];
      if (p.hashes_truncated) tomb.file_hashes_truncated = true;
      await env.SOCIAL.put(tombstoneKey(id), JSON.stringify(tomb));
    }
    spend(meter, 2);
    // The approval binding grants serving; there is nothing left to serve. The review's own state (locks/state) stays with
    // the review evidence.
    await env.SCENES.delete(`locks/approved/${id}.json`);
    spend(meter);
    p.phase = 'kv';
    await save();
  }

  if (p.phase === 'kv') {
    if (spent(meter)) return { progress: true };
    const rec = await loadCase(env, id);
    // The website namespace stays claimed, reserved to its owner (D8): nobody else can take it and serve their manifest to
    // a site that names it. The claim keeps naming this id, whose manifest is gone, so a read answers 404.
    const ns = await env.SOCIAL.get(`website-ns-of:${id}`);
    if (ns) {
      const claim = parse(await env.SOCIAL.get(`website-ns:${ns}`));
      if (claim && claim.sim_id === id && !claim.reserved) {
        await env.SOCIAL.put(`website-ns:${ns}`, JSON.stringify({ ...claim, reserved: true, released_at: iso(now) }));
      }
      await env.SOCIAL.delete(`website-ns-of:${id}`);
    }
    for (const key of [`sim-author:${mark.author_id}:${id}`, `plays:${id}`, `visits:${id}`, workspaceQueueKey('audit', id), ...(rec?.status ? [queueKey(rec.status, id)] : [])]) {
      await env.SOCIAL.delete(key);
    }
    if (!p.counted && mark.author_id) {
      p.counted = true;
      await save();
      const n = parseInt((await env.SOCIAL.get(`simCount:${mark.author_id}`)) || '0', 10) || 0;
      await env.SOCIAL.put(`simCount:${mark.author_id}`, String(Math.max(0, n - 1)));
    }
    // The listing's own record last, so a takedown filed while the job ran still finds it.
    await env.SOCIAL.delete(`sim:${id}`);
    spend(meter, 14);
    p.phase = 'done';
    await save();
  }

  mark.state = 'hard_deleted';
  mark.hard_deleted_at = iso(now);
  await save();
  if (mark.author_id) await accountCall(env, mark.author_id, 'space_hard_deleted', { sim_id: id, objects: p.objects, bytes: p.bytes, now });
  await audit(env, 'SPACE_HARD_DELETED', 'system', id, { author_id: mark.author_id, objects: p.objects, bytes: p.bytes, request_id: mark.request_id });
  await env.SOCIAL.delete(d.key);
  try { await legalLockedIds(env, 0); } catch { /* refreshes within 10 seconds */ }
  return { done: true };
}
