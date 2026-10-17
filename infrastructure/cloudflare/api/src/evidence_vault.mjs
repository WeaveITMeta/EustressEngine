// =============================================================================
// Evidence vault: illegal content sealed for legal process, read by no one in the normal course
// =============================================================================
//
// What moderation removes as illegal (a listing quarantined under Code A1 or A5, a forum post removed as csam or
// nonconsensual_sexual, a TAKE IT DOWN request that says the person shown is a minor) is sealed here first: the original bytes,
// or the text of a forum post, with the facts of the case (who uploaded it, when, from which address when the Worker has one,
// and which detection found it, with its confidence). The system needs no person to read any of it: removal, the report to an
// authority (src/legal_reports.mjs) and the author's notice all run from the case record and this vault's index, never from
// the content. A person reads it only as an administrator and only against a recorded legal-process reference (a subpoena, a
// court order, a warrant, a request from NCMEC), and every access is written to the audit log first.
//
// Law: 18 U.S.C. 2258A(h) as amended by the REPORT Act (Public Law 118-59, 2024) keeps the contents of a CyberTipline report for
// one year after it is made, preserved in line with the NIST Cybersecurity Framework. Sealed evidence is kept at least
// RETAIN_DAYS (366), recorded as `retain_until`; nothing in the Worker deletes it, and CSAM is marked `never_delete`.
//
// Encryption: AES-256-GCM, a fresh 96-bit IV for every object, the object's place (case, item, part) bound in as additional
// data so a sealed part cannot be moved to another case. The key is derived from JWT_SECRET with its own label
// (VAULT_KEY_LABEL), the way src/adult_content.mjs derives its image-URL key: HMAC-SHA256(JWT_SECRET, label), never the secret
// itself, derived on first use and never at module load. Each index records the key's id (a hash of the key, not the key), so
// a rotated JWT_SECRET is seen as a key that no longer matches rather than as damaged evidence. Rotating JWT_SECRET makes
// everything sealed before it unreadable: it must not be rotated while evidence is held, or the old secret must be kept.
//
// Storage, R2 SCENES under `evidence/{reference}/` (the case's EC- reference, src/case_reference.mjs):
//   {item}.c{n}        the content, part n of CHUNK_BYTES each: IV then ciphertext
//   {item}.facts       the facts of the item, the same way
//   {item}.json        the index, in the clear, written last: a present index means a complete seal. It holds no content and
//                      no personal data: sizes, times, the category, the key id, the retention.
// and under `evidence-views/{reference}/`:
//   {adminId}.json     the receipt of an administrator's reads of the case's content (readEvidence): when, which items, under
//                      which legal process. No content. It is what a person's "I viewed this content" (POST
//                      /api/admin/evidence/{reference}/viewed, src/moderation.mjs recordForumView) is checked against: a legal
//                      report says a person viewed the content only when that person read it here.
// and in KV SOCIAL:
//   vault-pending:{kind}:{id}:{item}   a seal that failed, retried by the hourly sweep (retryPendingEvidence) until it holds.
//                                      Its payload is encrypted when the key is there; without the key it keeps no text and
//                                      no upload address.
//
// A seal that fails never deletes anything: the content stays where it was (a quarantined listing's objects stay under the
// legal lock, a forum post stays hidden in the forum's storage), the failure is recorded and retried, and staff are told on the
// TAKE IT DOWN staff alert path (src/takedown.mjs). An object larger than VAULT_ITEM_MAX_BYTES is not copied: its key, size and
// SHA-256 are sealed as facts and the object stays under the legal lock.
// =============================================================================

import { assignCaseReference, resolveCaseReference, REFERENCE_PATTERN, CASE_KINDS } from './case_reference.mjs';
import { isAdmin } from './admin_check.mjs';
import { derivedCopyOf } from './user_images.mjs';

export const VAULT_PREFIX = 'evidence/';
export const VAULT_KEY_LABEL = 'evidence-vault-v1';
export const VAULT_ALG = 'AES-256-GCM';
/// At least a year (the REPORT Act's one year, and a day of margin). A floor: nothing deletes evidence on its own.
export const RETAIN_DAYS = 366;
/// Categories whose evidence is never deleted by anyone's routine: CSAM.
export const NEVER_DELETE_CATEGORIES = Object.freeze(['csam']);
/// One sealed part. A larger object is read and sealed a part at a time, so a large package never sits in memory whole.
export const CHUNK_BYTES = 8 * 1024 * 1024;
/// An object past this is not copied into the vault (its facts are), and stays under the legal lock.
export const VAULT_ITEM_MAX_BYTES = 512 * 1024 * 1024;
/// The items one seal takes: a listing's package, Space packages, thumbnail, cover, dossier and captures fit well within it.
export const VAULT_ITEMS_MAX = 48;
/// The kinds of legal process a read records. A read without one is refused.
export const LEGAL_PROCESS_KINDS = Object.freeze(['subpoena', 'court_order', 'search_warrant', 'ncmec_request', 'preservation_request', 'emergency_disclosure_request', 'other_legal_process']);
export const PENDING_PREFIX = 'vault-pending:';
/// The hourly sweep's staff alerts for a seal still failing: the first failure, then these attempts, then every 24th.
const ALERT_ATTEMPTS = [3, 12, 24];

const ITEM_ID = /^[a-z0-9][a-z0-9_.-]{0,63}$/;
/// The form of an item id: what a caller naming its own item (src/moderation.mjs sealForumEvidence `itemId`) must match.
export const VAULT_ITEM_ID = ITEM_ID;
const CASE_ID = /^[A-Za-z0-9-]{1,64}$/;
export const VIEW_RECEIPT_PREFIX = 'evidence-views/';
const VIEWER_ID = /^[A-Za-z0-9_-]{1,64}$/;
const VIEW_READS_KEPT = 20;
const JSON_META = { httpMetadata: { contentType: 'application/json' } };
const BIN_META = { httpMetadata: { contentType: 'application/octet-stream' } };
const enc = new TextEncoder();
const dec = new TextDecoder();

const hex = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
const b64 = (bytes) => { let s = ''; const u = new Uint8Array(bytes); for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000)); return btoa(s); };
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const indexKey = (ref, item) => `${VAULT_PREFIX}${ref}/${item}.json`;
const partKey = (ref, item, n) => `${VAULT_PREFIX}${ref}/${item}.c${n}`;
const factsKey = (ref, item) => `${VAULT_PREFIX}${ref}/${item}.facts`;
const pendingKey = (kind, id, item) => `${PENDING_PREFIX}${kind}:${id}:${item}`;
const viewReceiptKey = (ref, adminId) => `${VIEW_RECEIPT_PREFIX}${ref}/${adminId}.json`;
const aad = (kind, id, item, part) => enc.encode(`${VAULT_KEY_LABEL}|${kind}:${id}|${item}|${part}`);

// The key, derived on first use and kept for the isolate while JWT_SECRET is the same.
let vaultKey = { secret: null, key: null, id: null };

async function vaultKeyOf(env) {
  const secret = env?.JWT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  if (vaultKey.secret !== secret) {
    const outer = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const raw = new Uint8Array(await crypto.subtle.sign('HMAC', outer, enc.encode(VAULT_KEY_LABEL)));
    const idInput = new Uint8Array([...enc.encode(`${VAULT_KEY_LABEL}:key-id:`), ...raw]);
    const id = hex(await crypto.subtle.digest('SHA-256', idInput)).slice(0, 16);
    vaultKey = { secret, key: await crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']), id };
  }
  return vaultKey;
}

async function seal(key, data, additionalData) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData }, key, data));
  const out = new Uint8Array(iv.length + ct.length);
  out.set(iv, 0);
  out.set(ct, iv.length);
  return out;
}

async function unseal(key, stored, additionalData) {
  const bytes = new Uint8Array(stored);
  return new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: bytes.subarray(0, 12), additionalData }, key, bytes.subarray(12)));
}

/// The vault's own audit record, on the Worker's audit log (AUDIT_LOG, the shape index.js auditLog writes). Throws when it
/// cannot be written, so a read that cannot be audited does not happen.
async function audit(env, action, actor, target, details) {
  if (!env?.AUDIT_LOG?.put) throw new Error('the audit log is not configured');
  const entry = { action, admin_id: actor, target, details, timestamp: new Date().toISOString() };
  await env.AUDIT_LOG.put(`audit:${Date.now()}-${crypto.randomUUID().slice(0, 8)}`, JSON.stringify(entry), { expirationTtl: 86400 * 365 * 5 });
}
const auditQuietly = async (...args) => { try { await audit(...args); } catch (e) { console.error('evidence vault: an audit record was not written:', e?.name || 'Error'); } };
/// The same audit record, for another module's act that must not happen unaudited (src/moderation.mjs recordForumView).
/// Throws when it cannot be written.
export const writeAuditRecord = audit;

/// One part of an R2 object. A store that ignores the range (the test fakes) answers the whole object, which is cut here.
async function readPart(env, key, offset, length, size) {
  const object = size > length ? await env.SCENES.get(key, { range: { offset, length } }) : await env.SCENES.get(key);
  if (!object) throw new Error('the object is gone');
  const buf = new Uint8Array(await object.arrayBuffer());
  return buf.byteLength > length ? buf.slice(offset, offset + length) : buf;
}

async function sha256Of(chunks) {
  // One part: hashed directly. Several: the SHA-256 of the whole needs the whole, so the parts' own hashes are chained
  // instead and named as such (`sha256_parts`); the object's own SHA-256, when R2 kept one, is recorded beside it.
  if (chunks.length === 1) return { sha256: hex(await crypto.subtle.digest('SHA-256', chunks[0])) };
  return null;
}

/// Whether one item of a case is sealed (its index exists). Reads no content.
export async function isSealed(env, caseKind, caseId, itemId) {
  try {
    const ref = await assignCaseReference(env, caseKind, caseId);
    return !!(await env.SCENES.head(indexKey(ref, itemId)));
  } catch {
    return false;
  }
}

function checkItems(items) {
  if (!Array.isArray(items) || !items.length) return 'no_items';
  if (items.length > VAULT_ITEMS_MAX) return 'too_many_items';
  for (const it of items) {
    if (!it || !ITEM_ID.test(it.id || '')) return 'bad_item';
    const sources = ['r2Key', 'text', 'bytes'].filter((s) => it[s] !== undefined && it[s] !== null);
    if (sources.length !== 1) return 'bad_item';
    if (it.text !== undefined && typeof it.text !== 'string') return 'bad_item';
    if (it.r2Key !== undefined && typeof it.r2Key !== 'string') return 'bad_item';
  }
  if (new Set(items.map((i) => i.id)).size !== items.length) return 'bad_item';
  return null;
}

// Seals one item. Throws on any failure; writes the index last.
async function sealItem(env, k, { kind, id, ref, category, item, facts, now }) {
  if (await env.SCENES.head(indexKey(ref, item.id))) return { status: 'already' };
  let bytes = null;
  let size = 0;
  let sourceSha = null;
  if (item.text !== undefined) { bytes = enc.encode(item.text); size = bytes.byteLength; }
  else if (item.bytes !== undefined) { bytes = new Uint8Array(item.bytes); size = bytes.byteLength; }
  else {
    const head = await env.SCENES.head(item.r2Key);
    if (!head) return { status: 'missing' };
    size = Number(head.size) || 0;
    const known = head.checksums?.sha256;
    sourceSha = known ? (typeof known === 'string' ? known.toLowerCase() : hex(known)) : null;
  }
  const at = new Date(now).toISOString();
  const tooLarge = size > VAULT_ITEM_MAX_BYTES;
  const parts = tooLarge ? 0 : Math.max(1, Math.ceil(size / CHUNK_BYTES));
  const partHashes = [];
  let whole = null;
  for (let n = 0; n < parts; n++) {
    const plain = bytes ? bytes.subarray(n * CHUNK_BYTES, Math.min(size, (n + 1) * CHUNK_BYTES)) : await readPart(env, item.r2Key, n * CHUNK_BYTES, Math.min(CHUNK_BYTES, size - n * CHUNK_BYTES), size);
    partHashes.push(hex(await crypto.subtle.digest('SHA-256', plain)));
    if (parts === 1) whole = await sha256Of([plain]);
    await env.SCENES.put(partKey(ref, item.id, n), await seal(k.key, plain, aad(kind, id, item.id, `c${n}`)), BIN_META);
  }
  const itemFacts = {
    ...facts,
    item: item.id,
    source: item.r2Key ? { r2_key: item.r2Key } : { kind: item.text !== undefined ? 'text' : 'bytes' },
    bytes: size,
    content_sha256: whole?.sha256 || null,
    source_sha256: sourceSha,
    part_sha256: partHashes,
    copied: !tooLarge,
    ...(tooLarge ? { not_copied_reason: `larger than ${VAULT_ITEM_MAX_BYTES} bytes: kept in place under the legal lock` } : {}),
  };
  await env.SCENES.put(factsKey(ref, item.id), await seal(k.key, enc.encode(JSON.stringify(itemFacts)), aad(kind, id, item.id, 'facts')), BIN_META);
  const index = {
    v: 1, case_ref: ref, case_kind: kind, case_id: id, item: item.id, category,
    content: item.r2Key ? 'r2_object' : item.text !== undefined ? 'text' : 'bytes',
    bytes: size, parts, part_bytes: CHUNK_BYTES, copied: !tooLarge,
    alg: VAULT_ALG, key_label: VAULT_KEY_LABEL, key_id: k.id,
    sealed_at: at, retain_until: new Date(now + RETAIN_DAYS * 86400 * 1000).toISOString(), never_delete: NEVER_DELETE_CATEGORIES.includes(category),
  };
  await env.SCENES.put(indexKey(ref, item.id), JSON.stringify(index), JSON_META);
  return { status: 'sealed', index };
}

async function writePending(env, k, { kind, id, item, payload, error, now }) {
  const key = pendingKey(kind, id, item.id);
  let prior = null;
  try { prior = JSON.parse((await env.SOCIAL.get(key)) || 'null'); } catch { prior = null; }
  const at = new Date(now).toISOString();
  // The payload holds what the retry needs: the facts and the item's source (an R2 key, or the text). It is encrypted when the
  // key is there; without a key, a text item's text is not kept (the post stays in the forum's own storage), nor the address it
  // was uploaded from (an address is kept only sealed), and the rest is.
  let body;
  if (k) {
    body = { enc: b64(await seal(k.key, enc.encode(JSON.stringify(payload)), aad(kind, id, item.id, 'pending'))), key_id: k.id };
  } else {
    const { item: it, uploader, ...rest } = payload;
    const kept = uploader && typeof uploader === 'object'
      ? { ...uploader, upload_ip: null, ...(Array.isArray(uploader.upload_ips) ? { upload_ips: [] } : {}), ...(uploader.upload_ip || uploader.upload_ips?.length ? { upload_ip_note: 'Not kept: the vault key was not configured when the seal failed.' } : {}) }
      : uploader;
    body = { plain: { ...rest, uploader: kept, item: it.text !== undefined || it.bytes !== undefined ? { id: it.id, material: 'not_kept' } : it } };
  }
  const rec = {
    v: 1, case_kind: kind, case_id: id, item_id: item.id, category: payload.category,
    first_failed_at: prior?.first_failed_at || at, last_failed_at: at, attempts: (prior?.attempts || 0) + 1, last_error: String(error || 'error').slice(0, 200),
    ...body,
  };
  await env.SOCIAL.put(key, JSON.stringify(rec));
  return rec;
}

async function alertStaffOfSealFailure(env, facts) {
  try {
    const { alertStaffEvidence } = await import('./takedown.mjs');
    await alertStaffEvidence(env, facts);
  } catch (e) {
    console.error('evidence vault: the staff alert failed:', e?.name || 'Error');
  }
}

/// Seals a case's evidence. Never throws. `caseKind` and `caseId` name the case (src/case_reference.mjs CASE_KINDS: moderation,
/// takedown, forum); `category` is the finding (csam, nonconsensual_sexual, threat, ...); `subject` says what was removed
/// (`{ kind: 'listing', sim_id }` or `{ kind: 'forum_post', post_id, thread_id }`); `uploader` is `{ account_id, uploaded_at,
/// upload_ip, upload_ip_note }`, or for a forum post `{ account_id, uploaded_at, upload_ips: [{ ip, at, item_id }],
/// upload_ip_note }`, each address with its own time; `detection` is `{ source, checks, band, two_agree }`; `items` are `{ id, r2Key }`,
/// `{ id, text }` or `{ id, bytes }`. Returns `{ ok, case_ref, sealed, already, missing, pending, error }`: `ok` is true when
/// every item is sealed (now or before). An item that fails is recorded under `vault-pending:` for the sweep, an audit record
/// says so, and staff are alerted. Nothing is ever deleted, here or by a caller on a failure.
export async function sealEvidence(env, { caseKind, caseId, category, subject = null, uploader = {}, detection = null, items = [], now = Date.now() } = {}) {
  const out = { ok: false, case_ref: null, sealed: [], already: [], missing: [], pending: [], error: null };
  if (!CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !CASE_ID.test(caseId)) { out.error = 'bad_case'; return out; }
  if (typeof category !== 'string' || !category) { out.error = 'bad_category'; return out; }
  const bad = checkItems(items);
  if (bad) { out.error = bad; return out; }
  let k = null;
  try { k = await vaultKeyOf(env); } catch { k = null; }
  let ref = null;
  try { ref = await assignCaseReference(env, caseKind, caseId); } catch { ref = null; }
  out.case_ref = ref;
  const facts = { case_kind: caseKind, case_id: caseId, case_ref: ref, category, subject, uploader, detection, sealed_by: 'system' };
  for (const item of items) {
    try {
      if (!k) throw new Error('no vault key: JWT_SECRET is not configured');
      if (!ref || !REFERENCE_PATTERN.test(ref)) throw new Error('the case has no reference yet');
      const r = await sealItem(env, k, { kind: caseKind, id: caseId, ref, category, item, facts, now });
      out[r.status === 'sealed' ? 'sealed' : r.status === 'already' ? 'already' : 'missing'].push(item.id);
      if (r.status !== 'missing') await env.SOCIAL.delete(pendingKey(caseKind, caseId, item.id)).catch(() => {});
    } catch (e) {
      out.pending.push(item.id);
      const payload = { category, subject, uploader, detection, item };
      let rec = null;
      try { rec = await writePending(env, k, { kind: caseKind, id: caseId, item, payload, error: e?.message, now }); }
      catch (w) { console.error('evidence vault: the failed seal could not be recorded either:', w?.name || 'Error'); }
      console.error(`evidence vault: ${caseKind} ${caseId} item ${item.id} was not sealed:`, String(e?.message || e).slice(0, 200));
      if (!rec || rec.attempts === 1) await alertStaffOfSealFailure(env, { reference: ref, case_kind: caseKind, case_id: caseId, item: item.id, category, attempts: rec?.attempts || 1, error: String(e?.message || 'error').slice(0, 200), material_kept: !!k || item.r2Key !== undefined });
    }
  }
  out.ok = out.pending.length === 0;
  if (out.sealed.length) await auditQuietly(env, 'EVIDENCE_SEALED', 'system', ref || `${caseKind}:${caseId}`, { case_kind: caseKind, case_id: caseId, category, items: out.sealed });
  if (out.pending.length) await auditQuietly(env, 'EVIDENCE_SEAL_FAILED', 'system', ref || `${caseKind}:${caseId}`, { case_kind: caseKind, case_id: caseId, category, items: out.pending });
  return out;
}

/// Where the hourly sweep left off in the pending seals: the KV list cursor of the next page, or absent at the start.
export const PENDING_CURSOR_KEY = 'vault-pending-cursor';

/// The hourly sweep: every seal that failed is tried again, with the payload it recorded. A success removes the pending
/// record; a failure counts the attempt and alerts staff on the 3rd, 12th and 24th attempt, then every 24th. The pending
/// seals are read a page of `limit` at a time with a cursor, at most `maxPages` pages a run; a run that stops before the end
/// keeps its cursor (PENDING_CURSOR_KEY) and the next run starts there, and the run that reaches the end starts the next one
/// from the beginning, so every pending seal is reached. A cursor KV no longer takes is dropped and the list read from the
/// start. Never throws. `{ tried, sealed, still_pending, pages, complete }`.
export async function retryPendingEvidence(env, { limit = 25, maxPages = 8, now = Date.now() } = {}) {
  const out = { tried: 0, sealed: 0, still_pending: 0, pages: 0, complete: false };
  let k = null;
  try { k = await vaultKeyOf(env); } catch { k = null; }
  let cursor;
  try { cursor = (await env.SOCIAL.get(PENDING_CURSOR_KEY)) || undefined; } catch { cursor = undefined; }
  while (out.pages < maxPages) {
    let listed;
    try { listed = await env.SOCIAL.list({ prefix: PENDING_PREFIX, limit, ...(cursor ? { cursor } : {}) }); }
    catch {
      if (!cursor) return out;
      cursor = undefined;
      continue;
    }
    out.pages += 1;
    await retryPendingPage(env, k, listed?.keys || [], out, now);
    if (listed?.list_complete !== false || !listed.cursor) { cursor = undefined; out.complete = true; break; }
    cursor = listed.cursor;
  }
  try {
    if (cursor) await env.SOCIAL.put(PENDING_CURSOR_KEY, cursor);
    else await env.SOCIAL.delete(PENDING_CURSOR_KEY);
  } catch { /* the next run reads from where KV says, or from the start */ }
  return out;
}

// One page of pending seals, tried again.
async function retryPendingPage(env, k, keys, out, now) {
  for (const entry of keys) {
    let rec;
    try { rec = JSON.parse((await env.SOCIAL.get(entry.name)) || 'null'); } catch { rec = null; }
    if (!rec) continue;
    out.tried += 1;
    let payload = null;
    try {
      if (rec.enc && k && rec.key_id === k.id) payload = JSON.parse(dec.decode(await unseal(k.key, unb64(rec.enc), aad(rec.case_kind, rec.case_id, rec.item_id, 'pending'))));
      else if (rec.plain) payload = rec.plain;
    } catch { payload = null; }
    const usable = payload?.item && payload.item.material !== 'not_kept';
    const r = usable
      ? await sealEvidence(env, { caseKind: rec.case_kind, caseId: rec.case_id, category: payload.category, subject: payload.subject, uploader: payload.uploader, detection: payload.detection, items: [payload.item], now })
      : null;
    if (r?.ok) { out.sealed += 1; continue; }
    out.still_pending += 1;
    // sealEvidence counted this attempt when it ran; a payload that cannot be used is counted here.
    let attempts = rec.attempts || 1;
    if (!usable) {
      attempts += 1;
      await env.SOCIAL.put(entry.name, JSON.stringify({ ...rec, attempts, last_failed_at: new Date(now).toISOString(), last_error: rec.enc && !k ? 'no vault key' : 'the material to seal is not kept here' })).catch(() => {});
    } else {
      try { attempts = JSON.parse((await env.SOCIAL.get(entry.name)) || 'null')?.attempts || attempts; } catch { /* the count read above stands */ }
    }
    if (ALERT_ATTEMPTS.includes(attempts) || attempts % 24 === 0) {
      await alertStaffOfSealFailure(env, { reference: r?.case_ref || null, case_kind: rec.case_kind, case_id: rec.case_id, item: rec.item_id, category: rec.category, attempts, error: usable ? 'the seal is still failing' : 'the material to seal is not kept here: keep the content at its source', material_kept: usable });
    }
  }
  return out;
}

/// Whether a legal-process record is complete: `{ kind, reference }` (a reference of 4 to 200 characters, the subpoena's or
/// order's number), with an optional `note`. `{ value }` or `{ error }`.
export function checkLegalProcess(lp) {
  if (!lp || typeof lp !== 'object') return { error: 'legal_process_required' };
  if (!LEGAL_PROCESS_KINDS.includes(lp.kind)) return { error: 'legal_process_kind' };
  const reference = typeof lp.reference === 'string' ? lp.reference.replace(/[\x00-\x1f\x7f]+/g, ' ').trim() : '';
  if (reference.length < 4 || reference.length > 200) return { error: 'legal_process_reference' };
  const note = typeof lp.note === 'string' ? lp.note.replace(/[\x00-\x1f\x7f]+/g, ' ').trim().slice(0, 1000) : null;
  return { value: { kind: lp.kind, reference, note } };
}

/// The indexes of a case's sealed items (no content, no facts), for an administrator. Audited.
export async function listSealed(env, { caseRef, adminId } = {}) {
  if (typeof caseRef !== 'string' || !REFERENCE_PATTERN.test(caseRef)) return { ok: false, code: 'bad_reference' };
  if (!(await isAdmin(env, adminId))) return { ok: false, code: 'admin_required' };
  try { await audit(env, 'EVIDENCE_LISTED', adminId, caseRef, {}); } catch { return { ok: false, code: 'audit_unavailable' }; }
  const listed = await env.SCENES.list({ prefix: `${VAULT_PREFIX}${caseRef}/`, limit: 500 });
  const items = [];
  for (const o of listed?.objects || []) {
    if (!o.key.endsWith('.json')) continue;
    const obj = await env.SCENES.get(o.key);
    try { items.push(JSON.parse(await obj.text())); } catch { /* an unreadable index is skipped */ }
  }
  return { ok: true, items };
}

/// Opens sealed evidence for an administrator under legal process. The read is audited before anything is decrypted (a read
/// whose audit cannot be written is refused) and its outcome after. `itemId` null reads every item of the case. Returns
/// `{ ok: true, items: [{ index, facts, content }] }` (`content` a Uint8Array, or null for an item not copied), or
/// `{ ok: false, code }`: bad_reference, admin_required, legal_process_required, legal_process_kind, legal_process_reference,
/// audit_unavailable, not_found, no_key, key_mismatch, damaged.
export async function readEvidence(env, { caseRef, itemId = null, adminId, legalProcess } = {}) {
  if (typeof caseRef !== 'string' || !REFERENCE_PATTERN.test(caseRef)) return { ok: false, code: 'bad_reference' };
  if (itemId !== null && !ITEM_ID.test(itemId)) return { ok: false, code: 'not_found' };
  const lp = checkLegalProcess(legalProcess);
  if (!(await isAdmin(env, adminId))) {
    await auditQuietly(env, 'EVIDENCE_READ_REFUSED', String(adminId || 'unknown').slice(0, 64), caseRef, { reason: 'admin_required' });
    return { ok: false, code: 'admin_required' };
  }
  if (lp.error) {
    await auditQuietly(env, 'EVIDENCE_READ_REFUSED', adminId, caseRef, { reason: lp.error });
    return { ok: false, code: lp.error };
  }
  try { await audit(env, 'EVIDENCE_READ', adminId, caseRef, { item: itemId, legal_process: lp.value }); } catch { return { ok: false, code: 'audit_unavailable' }; }
  const k = await vaultKeyOf(env).catch(() => null);
  if (!k) return { ok: false, code: 'no_key' };
  const listed = await env.SCENES.list({ prefix: `${VAULT_PREFIX}${caseRef}/`, limit: 500 });
  const indexKeys = (listed?.objects || []).map((o) => o.key).filter((key) => key.endsWith('.json') && (itemId === null || key === indexKey(caseRef, itemId)));
  if (!indexKeys.length) return { ok: false, code: 'not_found' };
  const items = [];
  try {
    for (const key of indexKeys) {
      const index = JSON.parse(await (await env.SCENES.get(key)).text());
      if (index.key_id !== k.id) { await auditQuietly(env, 'EVIDENCE_READ_FAILED', adminId, caseRef, { item: index.item, reason: 'key_mismatch' }); return { ok: false, code: 'key_mismatch' }; }
      const factsObj = await env.SCENES.get(factsKey(caseRef, index.item));
      const facts = JSON.parse(dec.decode(await unseal(k.key, await factsObj.arrayBuffer(), aad(index.case_kind, index.case_id, index.item, 'facts'))));
      let content = null;
      if (index.copied) {
        const parts = [];
        for (let n = 0; n < index.parts; n++) {
          const p = await env.SCENES.get(partKey(caseRef, index.item, n));
          parts.push(await unseal(k.key, await p.arrayBuffer(), aad(index.case_kind, index.case_id, index.item, `c${n}`)));
        }
        content = new Uint8Array(parts.reduce((s, p) => s + p.byteLength, 0));
        let at = 0;
        for (const p of parts) { content.set(p, at); at += p.byteLength; }
      }
      items.push({ index, facts, content });
    }
  } catch {
    await auditQuietly(env, 'EVIDENCE_READ_FAILED', adminId, caseRef, { item: itemId, reason: 'damaged' });
    return { ok: false, code: 'damaged' };
  }
  await auditQuietly(env, 'EVIDENCE_READ_DONE', adminId, caseRef, { items: items.map((i) => i.index.item) });
  await writeViewReceipt(env, { caseRef, adminId, items: items.filter((i) => i.content).map((i) => i.index.item), legalProcess: lp.value });
  return { ok: true, items };
}

// The receipt of a read that decrypted content: who, when, which items, under which legal process. Only items whose content was
// opened count (an item not copied has none to read). A receipt that cannot be written loses nothing but the person's ability
// to say they viewed the content (recordForumView), which is the safe direction: never throws.
async function writeViewReceipt(env, { caseRef, adminId, items, legalProcess }) {
  if (!items.length || typeof adminId !== 'string' || !VIEWER_ID.test(adminId)) return;
  try {
    const key = viewReceiptKey(caseRef, adminId);
    let prior = null;
    try { prior = JSON.parse(await (await env.SCENES.get(key))?.text() || 'null'); } catch { prior = null; }
    const reads = [...(Array.isArray(prior?.reads) ? prior.reads : []), { at: new Date().toISOString(), items, legal_process: { kind: legalProcess.kind, reference: legalProcess.reference } }].slice(-VIEW_READS_KEPT);
    await env.SCENES.put(key, JSON.stringify({ v: 1, case_ref: caseRef, admin_id: adminId, reads }), JSON_META);
  } catch (e) {
    console.error('evidence vault: the read receipt was not written:', e?.name || 'Error');
  }
}

/// The receipt of `adminId`'s reads of a case's sealed content, `{ v, case_ref, admin_id, reads: [{ at, items, legal_process }] }`,
/// or null when that person never opened it. Reads no content.
export async function readViewReceipt(env, { caseRef, adminId } = {}) {
  if (typeof caseRef !== 'string' || !REFERENCE_PATTERN.test(caseRef) || typeof adminId !== 'string' || !VIEWER_ID.test(adminId)) return null;
  try {
    const obj = await env.SCENES.get(viewReceiptKey(caseRef, adminId));
    const receipt = obj ? JSON.parse(await obj.text()) : null;
    return receipt && receipt.case_ref === caseRef && receipt.admin_id === adminId && Array.isArray(receipt.reads) ? receipt : null;
  } catch {
    return null;
  }
}

/// The R2 keys of a listing's stored files that a seal copies: the package, each Space package, the thumbnail, the cover, the
/// review dossier and every review capture. A world's chunks are named by the manifest, which is sealed as facts; the
/// chunks themselves stay under the legal lock.
export async function listingEvidenceItems(env, sim) {
  const items = [];
  if (!sim?.id) return items;
  const own = (key) => typeof key === 'string' && key.startsWith(`universes/${sim.id}/`);
  const add = (id, r2Key) => { if (items.length < VAULT_ITEMS_MAX && !items.some((i) => i.r2Key === r2Key)) items.push({ id, r2Key }); };
  if (own(sim.r2_key)) add('package', sim.r2_key);
  let n = 0;
  for (const space of Object.values(sim.spaces || {}).slice(0, 20)) if (own(space?.r2_key)) add(`space-${n++}`, space.r2_key);
  // A listing image the public is served as a rebuilt copy (src/moderation.mjs publicImageBody) is sealed with that copy too,
  // as its own item named for the original with "-stripped".
  const addImage = async (id, r2Key) => {
    add(id, r2Key);
    const copy = await derivedCopyOf(env, r2Key);
    if (copy) add(`${id}-stripped`, copy.key);
  };
  for (const ext of ['webp', 'png', 'jpg']) if (await env.SCENES.head(`thumbnails/${sim.id}/thumb.${ext}`)) await addImage(`thumbnail-${ext}`, `thumbnails/${sim.id}/thumb.${ext}`);
  for (const ext of ['webp', 'png', 'jpg']) if (await env.SCENES.head(`covers/${sim.id}/cover.${ext}`)) await addImage(`cover-${ext}`, `covers/${sim.id}/cover.${ext}`);
  const listed = await env.SCENES.list({ prefix: `universes/${sim.id}/moderation/`, limit: 500 });
  for (const o of listed?.objects || []) {
    const m = /\/moderation\/(?:e(\d+)\/)?(dossier\.json|capture-[0-7]\.(?:png|jpg|webp))$/.exec(o.key);
    if (!m) continue;
    const id = `${m[1] ? `e${m[1]}-` : ''}${m[2].replace(/\.json$/, '')}`.replace(/[^a-z0-9_.-]/g, '-');
    if (m[2].startsWith('capture-')) await addImage(id, o.key); else add(id, o.key);
  }
  return items;
}

/// The admin routes: GET /api/admin/evidence/{reference} lists a case's sealed items; POST /api/admin/evidence/{reference}/read
/// with `{ item?, legal_process: { kind, reference, note? } }` opens them; POST /api/admin/evidence/{reference}/viewed with
/// `{ how?: 'evidence_vault_read', note? }` records "I viewed this content" for a forum case (src/moderation.mjs
/// recordForumView), which is refused unless this administrator's read is on record. All are audited. Null for any other path.
export async function handleEvidenceRoute(request, url, env, cors, deps) {
  const viewed = /^\/api\/admin\/evidence\/([^/]+)\/viewed$/.exec(url.pathname);
  if (viewed) return viewedRoute(request, viewed[1], env, cors, deps);
  const m = /^\/api\/admin\/evidence\/([^/]+)(\/read)?$/.exec(url.pathname);
  if (!m) return null;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return deps.json({ error: 'Admin access required' }, 403, cors);
  let ref = '';
  try { ref = decodeURIComponent(m[1]); } catch { ref = ''; }
  if (!m[2]) {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    const r = await listSealed(env, { caseRef: ref, adminId });
    return deps.json(r.ok ? { items: r.items } : { error: r.code, code: r.code }, r.ok ? 200 : r.code === 'bad_reference' ? 400 : 503, headers);
  }
  if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
  let body = {};
  try { body = await request.json(); } catch { body = {}; }
  const r = await readEvidence(env, { caseRef: ref, itemId: typeof body.item === 'string' ? body.item : null, adminId, legalProcess: body.legal_process });
  if (!r.ok) {
    const status = { bad_reference: 400, legal_process_required: 400, legal_process_kind: 400, legal_process_reference: 400, admin_required: 403, not_found: 404 }[r.code] || 503;
    return deps.json({ error: r.code, code: r.code }, status, headers);
  }
  return deps.json({ items: r.items.map((i) => ({ index: i.index, facts: i.facts, content_base64: i.content ? b64(i.content) : null })) }, 200, headers);
}

// POST /api/admin/evidence/{reference}/viewed: a person says they viewed the case's content. Only a forum case records it today
// (src/moderation.mjs recordForumView); its own checks (the read on record, the audit written first) decide.
async function viewedRoute(request, raw, env, cors, deps) {
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return deps.json({ error: 'Admin access required' }, 403, cors);
  let ref = '';
  try { ref = decodeURIComponent(raw); } catch { ref = ''; }
  const found = REFERENCE_PATTERN.test(ref) ? await resolveCaseReference(env, ref) : null;
  if (!found) return deps.json({ error: 'No case has this reference.', code: 'bad_reference' }, 404, headers);
  if (found.kind !== 'forum') return deps.json({ error: 'A view is recorded for a forum case only.', code: 'not_supported' }, 409, headers);
  let body = {};
  try { body = await request.json(); } catch { body = {}; }
  const { recordForumView } = await import('./moderation.mjs');
  const r = await recordForumView({ env, postId: found.id, reviewerId: adminId, how: body?.how ?? 'evidence_vault_read', note: typeof body?.note === 'string' ? body.note : null });
  if (!r.ok) return deps.json({ error: r.message, code: r.code }, r.status || 400, headers);
  return deps.json(r, 200, headers);
}
