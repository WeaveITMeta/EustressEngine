// =============================================================================
// The deletion mark: the one truth that a published Space is deleted
// =============================================================================
//
// A deleted listing is marked by an R2 object, `locks/deleted/{id}.json`, made
// with a create-if-absent precondition. R2 is strongly consistent, and nothing
// that rewrites the listing's KV record (`sim:{id}`) touches it, so a writer that
// read the record before the deletion and writes it back afterwards cannot undo
// the deletion. Every serving path reads this mark through the legal-lock read
// paths (src/moderation.mjs simWithLegalLock and withLegalLockFrom), which fold
// it in here: the listing reads as private (`is_public: false`) with a `deletion`
// field, and is served to nobody but its author and an administrator.
//
// The overlay needs only the mark's custom metadata, so a single head (or one
// list for a page of listings) is enough; the body carries the rest for staff.
//
// This file imports nothing, so moderation.mjs and space_delete.mjs can both
// import it without a cycle. Design: docs/architecture/DELETE_PUBLISHED_SPACE.md.
// =============================================================================

export const DELETED_PREFIX = 'locks/deleted/';
export const deletionMarkKey = (simId) => `${DELETED_PREFIX}${simId}.json`;

/// The states a mark can be in. `soft`: hidden, bytes kept, restorable. `hard_deleting`: the hourly job has begun removing
/// the bytes; restore is closed. `hard_deleted`: removed; the mark stays forever and refuses every later write to the id.
export const MARK_STATES = Object.freeze(['soft', 'hard_deleting', 'hard_deleted']);

const isoOrNull = (v) => (typeof v === 'string' && Number.isFinite(Date.parse(v)) ? v : null);

/// What the author and the serving paths may know of a mark, from its custom metadata: never the request id, the reason a
/// removal waits, or who placed a hold. A mark with no readable metadata still reads as deleted (fail closed).
export function deletionView(meta) {
  const m = meta || {};
  const state = MARK_STATES.includes(m.state) ? m.state : 'soft';
  const hardAfter = isoOrNull(m.hard_delete_after);
  return {
    state,
    soft_deleted_at: isoOrNull(m.soft_deleted_at),
    hard_delete_after: hardAfter,
    // Restorable only while soft and before the due date (the route re-checks both under the lease).
    restore_until: state === 'soft' ? hardAfter : null,
  };
}

/// The author id the mark was made for, from its metadata, or null.
export const markAuthor = (meta) => (typeof meta?.author === 'string' && meta.author ? meta.author : null);

/// The listing as serving should see it once deleted: private, and carrying the mark's public view.
export function withDeletionMark(sim, view) {
  if (!sim || !view) return sim;
  return { ...sim, is_public: false, deletion: view };
}

/// The mark of one listing as `{ view, meta }`, or null. One head; never throws on a missing binding.
export async function deletionMarkHead(env, simId) {
  if (!env?.SCENES?.head || typeof simId !== 'string' || !simId) return null;
  const head = await env.SCENES.head(deletionMarkKey(simId));
  if (!head) return null;
  const meta = head.customMetadata || {};
  return { view: deletionView(meta), meta };
}

/// The public view of one listing's mark, or null.
export async function deletionMarkOf(env, simId) {
  return (await deletionMarkHead(env, simId))?.view || null;
}

/// The whole mark (its JSON body), or null.
export async function readDeletionMark(env, simId) {
  if (!env?.SCENES?.get) return null;
  const obj = await env.SCENES.get(deletionMarkKey(simId));
  if (!obj) return null;
  try {
    const body = JSON.parse(await obj.text());
    return body && typeof body === 'object' ? body : null;
  } catch {
    // An unreadable mark is still a mark: the listing stays deleted.
    return { sim_id: simId, state: 'soft', unreadable: true };
  }
}

/// Every mark at once, `id -> view`, for pages that list many listings: one paginated R2 list. Keys that are not a mark
/// (a bucket stub that ignores the prefix) are skipped.
export async function listDeletionMarks(env) {
  const out = new Map();
  if (!env?.SCENES?.list) return out;
  let cursor;
  do {
    const page = await env.SCENES.list({ prefix: DELETED_PREFIX, limit: 1000, include: ['customMetadata'], ...(cursor ? { cursor } : {}) });
    for (const o of page?.objects || []) {
      if (typeof o?.key !== 'string' || !o.key.startsWith(DELETED_PREFIX) || !o.key.endsWith('.json')) continue;
      out.set(o.key.slice(DELETED_PREFIX.length, -'.json'.length), deletionView(o.customMetadata));
    }
    cursor = page?.truncated ? page.cursor : undefined;
  } while (cursor);
  return out;
}

/// The list form rides on the legal lock list (src/moderation.mjs legalLockedIds) under a symbol, so the callers that
/// already pass that list to withLegalLockFrom fold deletions in without a change, and iterating the list is unchanged.
const DELETED_MARKS = Symbol.for('eustress.space_delete.marks');

export function attachDeletionMarks(ids, marks) {
  if (ids && typeof ids === 'object') ids[DELETED_MARKS] = marks instanceof Map ? marks : new Map();
  return ids;
}

export function withDeletionFrom(sim, ids) {
  const view = ids?.[DELETED_MARKS]?.get?.(sim?.id);
  return view ? withDeletionMark(sim, view) : sim;
}

// -----------------------------------------------------------------------------
// The tombstone of a listing removed for good
// -----------------------------------------------------------------------------

/// The KV record the hourly job leaves when it removes a listing for good (src/space_delete.mjs runPhases): who, what,
/// when, the case reference, and the SHA-256 of the files a later TAKE IT DOWN blocks again.
export const tombstoneKey = (simId) => `simgone:${simId}`;

const SHA256_HEX = /^[0-9a-f]{64}$/;

/// The removed listing's tombstone, or null when it has none (it was never removed, or the id is not one).
export async function readTombstone(env, simId) {
  if (!env?.SOCIAL?.get || typeof simId !== 'string' || !simId) return null;
  const raw = await env.SOCIAL.get(tombstoneKey(simId));
  if (!raw) return null;
  try {
    const tomb = JSON.parse(raw);
    return tomb && typeof tomb === 'object' ? tomb : null;
  } catch {
    return null;
  }
}

/// The files of a removed listing a TAKE IT DOWN can still name, as `{ sha256, kind, size }`, from its tombstone: the
/// package, the world and source manifests, each Space package, and every hash the job copied from the removed files
/// (`file_hashes`: packages, manifests, thumbnail, cover). Each hash once. [] for a tombstone without any.
export function tombstoneHashes(tomb) {
  const out = [];
  const add = (sha256, kind, size = null) => {
    const h = typeof sha256 === 'string' ? sha256.toLowerCase() : '';
    if (SHA256_HEX.test(h) && !out.some((x) => x.sha256 === h)) out.push({ sha256: h, kind, size: Number.isSafeInteger(size) ? size : null });
  };
  if (!tomb || typeof tomb !== 'object') return out;
  add(tomb.pak_sha256, 'package');
  add(tomb.world_manifest_sha256, 'world_manifest');
  add(tomb.source_manifest_sha256, 'source_manifest');
  for (const h of Array.isArray(tomb.space_sha256) ? tomb.space_sha256 : []) add(h, 'space_package');
  for (const f of Array.isArray(tomb.file_hashes) ? tomb.file_hashes : []) add(f?.sha256, typeof f?.kind === 'string' ? f.kind : 'file', f?.size);
  return out;
}
