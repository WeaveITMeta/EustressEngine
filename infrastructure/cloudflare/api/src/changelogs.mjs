// =============================================================================
// Publish change logs: the author's record of what each publish changed
// =============================================================================
//
// Studio builds a CHANGELOG.md at the end of a publish and posts it here. It is
// the AUTHOR's record: it holds Space and folder names and listing text, it is
// never shown publicly, and it is not part of the listing's review or approval.
// Contract: docs/architecture/PUBLISH_CHANGELOG.md.
//
//   POST /api/simulations/{id}/changelogs        author; {version, publish_hash, markdown}; {id}
//   GET  /api/simulations/{id}/changelogs        author or admin; newest first, at most 50
//   GET  /api/simulations/{id}/changelogs/{log}  author or admin; the markdown as an attachment
//
// What holds, from Security's review:
//   - The POST is the author's, the body is READ through a capped reader (never
//     read whole and measured), the markdown is UTF-8 text of at most 64 KB with
//     no control characters, and `version` and `publish_hash` must be the
//     listing's real current ones: a client cannot file a log for another
//     version.
//   - A log is created if absent (R2's conditional put): the id is the version,
//     the hash and a sequence, so a log can never be overwritten. A cover-only
//     update posts a second log for the same version and hash (the next
//     sequence), at most 10 of them.
//   - Bounded: 200 logs and 8 MB per listing (the oldest go), 100 posts a day
//     per account.
//   - Under a hold (a legal hold, quarantine, a legal lock, a frozen author)
//     the writer is refused with the usual 409 in_review AND the author's reads
//     stop too, as for the other evidence objects; an admin still reads.
//   - Anyone else gets one 404, byte for byte the answer for a log that is not
//     there, and a list only ever names the ids of the listing asked about.
//
// Kept in R2, one object per log, the body the markdown and the custom metadata
// what the list needs, so a list never reads a body:
//   universes/{simId}/changelogs/{version, 10 digits}-{8 hex of the hash}-{seq}.md
// =============================================================================

import { contentLockedFor, inReview } from './moderation.mjs';

export const CHANGELOG_MAX_BYTES = 64 * 1024;
/// The JSON body around the markdown (escaping can double it) is read up to this much, and no further.
export const CHANGELOG_BODY_READ_LIMIT = 3 * CHANGELOG_MAX_BYTES;
export const CHANGELOG_LIST_LIMIT = 50;
export const CHANGELOG_KEEP = 200;
export const CHANGELOG_TOTAL_BYTES = 8 * 1024 * 1024;
export const CHANGELOG_PER_VERSION = 10;
export const CHANGELOG_POSTS_PER_DAY = 100;

const LIST_PATH = /^\/api\/simulations\/([a-f0-9-]+)\/changelogs$/;
const ONE_PATH = /^\/api\/simulations\/([a-f0-9-]+)\/changelogs\/(\d{10}-[a-f0-9]{8}-\d)$/;
const HASH = /^[A-Za-z0-9:_.\-]{1,128}$/;

const prefix = (simId) => `universes/${simId}/changelogs/`;
const key = (simId, logId) => `${prefix(simId)}${logId}.md`;
const normHash = (h) => String(h ?? '').replace(/^blake3:/i, '').toLowerCase();
const IF_ABSENT = { onlyIf: { etagDoesNotMatch: '*' } };

const bad = (deps, cors, code, error, status = 400) => deps.json({ error, code }, status, cors);
// One answer for a log that is not there, a listing that is not yours, and a listing under a hold.
const notFound = (deps, cors) => deps.json({ error: 'Not found' }, 404, cors);

async function loadListing(env, simId) {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

/// Whether this viewer may read the listing's logs: an admin always; the author
/// unless the listing is under a hold.
async function mayRead(request, env, sim, deps) {
  const viewer = await deps.verifyAuth(request, env);
  if (!viewer) return false;
  if (await deps.requireAdmin(request, env)) return true;
  return viewer === sim.author_id && !(await contentLockedFor(env, sim, viewer));
}

/// The request body as text, read through a reader that stops at `limit` bytes. Null when it is longer.
async function readCapped(request, limit) {
  if (!request.body) return '';
  const reader = request.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.length;
    if (total > limit) { await reader.cancel().catch(() => {}); return null; }
    chunks.push(value);
  }
  const bytes = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) { bytes.set(c, at); at += c.length; }
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
}

async function hashStem(publishHash) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(normHash(publishHash)));
  return [...new Uint8Array(digest)].slice(0, 4).map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// One post per account per day-slot, counted through an R2 object per slot
/// created if absent (atomic, where a KV counter is not).
async function takePostSlot(env, authorId) {
  const day = new Date().toISOString().slice(0, 10);
  for (let n = 0; n < CHANGELOG_POSTS_PER_DAY; n++) {
    if (await env.SCENES.put(`locks/changelog/${authorId}/${day}/${n}`, '1', { httpMetadata: { contentType: 'text/plain' }, ...IF_ABSENT })) return true;
  }
  return false;
}

async function handlePost(request, simId, env, cors, deps) {
  const viewer = await deps.verifyAuth(request, env);
  if (!viewer) return deps.json({ error: 'Unauthorized' }, 401, cors);
  const sim = await loadListing(env, simId);
  if (!sim) return deps.json({ error: 'Simulation not found' }, 404, cors);
  if (sim.author_id !== viewer) return deps.json({ error: 'Not your simulation' }, 403, cors);
  if (await contentLockedFor(env, sim, viewer)) return inReview(deps.json, cors);

  const text = await readCapped(request, CHANGELOG_BODY_READ_LIMIT);
  if (text === null) return bad(deps, cors, 'changelog_too_large', `A change log is at most ${CHANGELOG_MAX_BYTES} bytes of text`, 413);
  let body;
  try { body = JSON.parse(text); } catch { return bad(deps, cors, 'changelog_not_json', 'The body must be JSON'); }
  if (!body || typeof body !== 'object' || Array.isArray(body)) return bad(deps, cors, 'changelog_not_json', 'The body must be a JSON object');
  const extra = Object.keys(body).filter((k) => !['version', 'publish_hash', 'markdown'].includes(k));
  if (extra.length) return bad(deps, cors, 'changelog_unknown_field', `Unknown field: ${extra[0]}`);
  if (!Number.isInteger(body.version) || body.version < 1 || body.version > 1_000_000_000) return bad(deps, cors, 'changelog_version', 'version must be a whole number from 1');
  if (typeof body.publish_hash !== 'string' || !HASH.test(body.publish_hash)) return bad(deps, cors, 'changelog_hash', 'publish_hash must be 1 to 128 characters of letters, digits and : _ . -');
  if (typeof body.markdown !== 'string' || !body.markdown.trim()) return bad(deps, cors, 'changelog_markdown', 'markdown must be text');
  const bytes = new TextEncoder().encode(body.markdown).length;
  if (bytes > CHANGELOG_MAX_BYTES) return bad(deps, cors, 'changelog_too_large', `A change log is at most ${CHANGELOG_MAX_BYTES} bytes of text`, 413);
  // Text only: a NUL is not text, and the other control characters are not part of a markdown document.
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(body.markdown)) return bad(deps, cors, 'changelog_markdown', 'markdown must be text');

  // The log is for the version the listing is at now, and the publish it was made from.
  const current = sim.world?.publish_hash;
  if (body.version !== (sim.version || 1) || !current || normHash(body.publish_hash) !== normHash(current))
    return bad(deps, cors, 'changelog_mismatch', 'This log is not for the version this listing is at', 409);

  if (!(await takePostSlot(env, viewer))) return bad(deps, cors, 'changelog_rate', 'Too many change logs today. Try again tomorrow.', 429);

  const stem = `${String(body.version).padStart(10, '0')}-${await hashStem(body.publish_hash)}`;
  const created = new Date().toISOString();
  let id = null;
  for (let seq = 0; seq < CHANGELOG_PER_VERSION && !id; seq++) {
    const candidate = `${stem}-${seq}`;
    // Create if absent: an existing log is never overwritten.
    const stored = await env.SCENES.put(key(simId, candidate), body.markdown, {
      httpMetadata: { contentType: 'text/markdown; charset=utf-8' },
      customMetadata: { version: String(body.version), publish_hash: body.publish_hash, created_at: created, bytes: String(bytes), author: viewer },
      ...IF_ABSENT,
    });
    if (stored) id = candidate;
  }
  if (!id) return bad(deps, cors, 'changelog_limit', `A version keeps at most ${CHANGELOG_PER_VERSION} change logs`, 409);

  // Bounded per listing in count and in bytes: the oldest go.
  const listed = await env.SCENES.list({ prefix: prefix(simId), limit: 1000, include: ['customMetadata'] });
  const rows = (listed?.objects || []).map((o) => ({ key: o.key, version: Number(o.customMetadata?.version) || 0, at: o.customMetadata?.created_at || '', bytes: Number(o.customMetadata?.bytes) || 0 }))
    .sort((a, b) => a.version - b.version || (a.at < b.at ? -1 : a.at > b.at ? 1 : 0));
  let total = rows.reduce((n, r) => n + r.bytes, 0);
  let count = rows.length;
  for (const r of rows) {
    if (count <= CHANGELOG_KEEP && total <= CHANGELOG_TOTAL_BYTES) break;
    if (r.key === key(simId, id)) continue;
    await env.SCENES.delete(r.key).catch(() => {});
    count -= 1;
    total -= r.bytes;
  }
  return deps.json({ id }, 201, cors);
}

async function handleList(request, simId, env, cors, deps) {
  const sim = await loadListing(env, simId);
  if (!sim || !(await mayRead(request, env, sim, deps))) return notFound(deps, cors);
  const listed = await env.SCENES.list({ prefix: prefix(simId), limit: 1000, include: ['customMetadata'] });
  const rows = (listed?.objects || [])
    .map((o) => ({ id: o.key.slice(prefix(simId).length, -3), version: Number(o.customMetadata?.version) || 0, publish_hash: o.customMetadata?.publish_hash || '', created_at: o.customMetadata?.created_at || '', bytes: Number(o.customMetadata?.bytes) || 0 }))
    .sort((a, b) => b.version - a.version || (a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0))
    .slice(0, CHANGELOG_LIST_LIMIT);
  return deps.json(rows, 200, { ...cors, 'Cache-Control': 'private, no-store' });
}

async function handleGetOne(request, simId, logId, env, cors, deps) {
  const sim = await loadListing(env, simId);
  if (!sim || !(await mayRead(request, env, sim, deps))) return notFound(deps, cors);
  const object = await env.SCENES.get(key(simId, logId));
  if (!object) return notFound(deps, cors);
  const version = Number.parseInt(object.customMetadata?.version, 10);
  const name = `CHANGELOG-v${Number.isInteger(version) && version > 0 ? version : 0}.md`;
  return new Response(object.body, {
    headers: {
      ...cors,
      'Content-Type': 'text/markdown; charset=utf-8',
      'Content-Disposition': `attachment; filename="${name}"`,
      'X-Content-Type-Options': 'nosniff',
      'Content-Security-Policy': "default-src 'none'; sandbox",
      'Cache-Control': 'private, no-store',
    },
  });
}

/// Returns a Response for a change-log route, or null when the path is not one.
/// `deps`: verifyAuth, requireAdmin, json.
export async function handleChangelogRoute(request, url, env, cors, deps) {
  const one = url.pathname.match(ONE_PATH);
  if (one) {
    if (request.method === 'GET') return handleGetOne(request, one[1], one[2], env, cors, deps);
    return deps.json({ error: 'Method not allowed' }, 405, cors);
  }
  const list = url.pathname.match(LIST_PATH);
  if (!list) return null;
  if (request.method === 'POST') return handlePost(request, list[1], env, cors, deps);
  if (request.method === 'GET') return handleList(request, list[1], env, cors, deps);
  return deps.json({ error: 'Method not allowed' }, 405, cors);
}
