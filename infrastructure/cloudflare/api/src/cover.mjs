// =============================================================================
// A listing's cover image, and the review captures served as previews
// =============================================================================
//
// A listing may carry a creator-uploaded cover: portrait box art, or 16:9. The Gallery shows the
// cover when there is one and the thumbnail otherwise. The routes, the rules and what review does
// with a cover are in docs/networking/LISTING_COVER.md.
//
//     PUT    /api/simulations/{id}/cover        the image bytes; the author only
//     DELETE /api/simulations/{id}/cover        remove it; the author only, idempotent
//     GET    /api/simulations/{id}/cover        the image
//     GET    /api/simulations/{id}/previews     { previews: [{ n, url }] }
//     GET    /api/simulations/{id}/preview/{n}  one review capture (n 0 to 7)
//
// A cover is the creator's own art, so it is reviewed like the thumbnail: the approval binds the
// exact object (coverApproved in moderation.mjs), and until a review binds a new one the public
// is served nothing for it (404, never a placeholder: the Gallery falls back to the thumbnail).
// A change here never starts a review by itself; the client posts /submit.

import {
  isListable, canServe, contentLockedFor, inReview, simWithLegalLock, coverApproved, approvedCaptureKeys, readApproval, identOfObject, sendBackToReview,
  JUDGE_IMAGE_MAX_BYTES, overJudgeImageLimit, publicImageBody, recordUploadedImage,
} from './moderation.mjs';
import { inspectImage, stripMetadata, sha256Hex, USER_IMAGE_HEADERS, pngColourType } from './user_images.mjs';
import { isAdultListing, refuseAdult, refuseUnsignedImage, imageCacheControl, signedImageUrl } from './adult_content.mjs';
import { refuseBlockedUpload } from './takedown.mjs';
import { withTagsWrittenBack } from './tags.mjs';

/// The most a cover may weigh: 2 MB, and never more than the judge can be sent (JUDGE_IMAGE_MAX_BYTES), since the judge reads it.
export const COVER_MAX_BYTES = Math.min(2 * 1024 * 1024, JUDGE_IMAGE_MAX_BYTES);
/// The shortest side a cover may have, in pixels.
export const COVER_MIN_SHORT_SIDE = 320;
/// The longest side may be at most this many times the shortest: ratios from 1:2 to 2:1.
export const COVER_MAX_ASPECT = 2;
export const COVER_RULES = 'A cover is a PNG, JPEG or WebP image of at most 2 MB, at least 320 px on its short side, with a width to height ratio between 1:2 and 2:1.';

/// The host the views name when no request carries one (the list handlers share galleryView's default).
const DEFAULT_ORIGIN = 'https://api.eustress.dev';

const COVER_PATH = /^\/api\/simulations\/([a-f0-9-]+)\/cover$/;
const PREVIEWS_PATH = /^\/api\/simulations\/([a-f0-9-]+)\/previews$/;
const PREVIEW_PATH = /^\/api\/simulations\/([a-f0-9-]+)\/preview\/([0-7])$/;

const EXTS = ['webp', 'png', 'jpg'];
const contentTypeOf = (ext) => `image/${ext === 'jpg' ? 'jpeg' : ext}`;
const coverKey = (simId, ext) => `covers/${simId}/cover.${ext}`;

/// The cover the listing record names, or null: an extension this module writes and sizes that are whole numbers.
export function coverOf(sim) {
  const c = sim?.cover;
  if (!sim?.id || !c || typeof c !== 'object') return null;
  if (!EXTS.includes(c.ext)) return null;
  if (!Number.isSafeInteger(c.width) || !Number.isSafeInteger(c.height) || c.width < 1 || c.height < 1) return null;
  return c;
}

/// The cover as a listing view carries it: an absolute URL on this Worker's origin (with the time it was set, so a
/// replaced cover is a new URL) and its size, or nulls when the listing has none.
export function coverFields(sim, origin = DEFAULT_ORIGIN) {
  const c = coverOf(sim);
  if (!c) return { cover_url: null, cover: null };
  return {
    cover_url: `${origin}/api/simulations/${sim.id}/cover?v=${Date.parse(c.updated_at) || 0}`,
    cover: { width: c.width, height: c.height },
  };
}

/// Whether a picture of this size is a cover: the short side and the ratio. Null when it is, otherwise the reason.
export function coverSizeProblem(width, height) {
  if (Math.min(width, height) < COVER_MIN_SHORT_SIDE) return `The cover is ${width} x ${height} px; its short side must be at least ${COVER_MIN_SHORT_SIDE} px. ${COVER_RULES}`;
  if (width > height * COVER_MAX_ASPECT || height > width * COVER_MAX_ASPECT) return `The cover is ${width} x ${height} px; its width to height ratio must be between 1:2 and 2:1. ${COVER_RULES}`;
  return null;
}

/// The request body, read no further than `limit` bytes: `{ bytes }`, or `{ tooLarge: true }` as soon as more than `limit` have
/// arrived (the rest is never read). A body with no declared length, or a stream that never ends, costs at most `limit` bytes.
async function readCapped(request, limit) {
  if (!request.body) return { bytes: new Uint8Array(0) };
  const reader = request.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > limit) {
      await reader.cancel().catch(() => {});
      return { tooLarge: true };
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(total);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.byteLength;
  }
  return { bytes };
}

async function readSim(env, simId) {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

/// Returns a Response for a cover or preview route, or null when the path is not one.
/// `deps` carries verifyAuth, requireAdmin and json from index.js. `approvedCaptureKeys` and `images` may
/// be replaced (a test hands in its own).
export async function handleCoverRoute(request, url, env, cors, deps) {
  const path = url.pathname;
  const method = request.method;
  let match;
  if ((match = COVER_PATH.exec(path))) {
    if (method === 'PUT') return putCover(request, match[1], env, cors, deps);
    if (method === 'DELETE') return deleteCover(request, match[1], env, cors, deps);
    if (method === 'GET') return getCover(request, match[1], env, cors, deps);
    return deps.json({ error: 'Method not allowed' }, 405, cors);
  }
  if ((match = PREVIEWS_PATH.exec(path))) {
    if (method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return listPreviews(request, url, match[1], env, cors, deps);
  }
  if ((match = PREVIEW_PATH.exec(path))) {
    if (method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return getPreview(request, match[1], Number(match[2]), env, cors, deps);
  }
  return null;
}

// -----------------------------------------------------------------------------
// The author's side
// -----------------------------------------------------------------------------

/// The listing, when it is the caller's: the one answer for a listing that does not exist and one that is not theirs,
/// and the same refusal as every other write while a hold is on the listing or the author's publishing is frozen.
async function ownedCoverTarget(request, simId, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return { error: deps.json({ error: 'Unauthorized' }, 401, cors) };
  const sim = await readSim(env, simId);
  if (!sim || sim.author_id !== userId) return { error: deps.json({ error: 'Simulation not found' }, 404, cors) };
  if (await contentLockedFor(env, sim, userId)) return { error: inReview(deps.json, cors) };
  return { userId, sim };
}

async function putCover(request, simId, env, cors, deps) {
  const { error, userId, sim } = await ownedCoverTarget(request, simId, env, cors, deps);
  if (error) return error;
  const images = deps.images || { inspectImage, stripMetadata, sha256Hex };
  const refuse = (status, code, message) => deps.json({ error: message, code }, status, cors);

  // A declared size over the limit is refused before the body is read. Whatever is declared, or nothing (a chunked body), the body is
  // read only as far as the limit: the cap holds while the bytes arrive, not after they have all been buffered.
  const declared = Number(request.headers.get('Content-Length'));
  if (Number.isFinite(declared) && declared > COVER_MAX_BYTES) return refuse(413, 'cover_too_large', `The cover is larger than 2 MB. ${COVER_RULES}`);
  const read = await readCapped(request, COVER_MAX_BYTES);
  if (read.tooLarge) return refuse(413, 'cover_too_large', `The cover is larger than 2 MB. ${COVER_RULES}`);
  const body = read.bytes;
  if (body.byteLength === 0) return refuse(400, 'cover_empty', `The request body is empty; send the image bytes. ${COVER_RULES}`);

  // The type is what the bytes are. The Content-Type the client sent is never read.
  const seen = images.inspectImage(body);
  if (!seen) return refuse(415, 'cover_bad_type', `The file is not a PNG, JPEG or WebP image whose size can be read, or it is larger than the platform accepts for an image. ${COVER_RULES}`);
  const ext = seen.ext;
  if (!EXTS.includes(ext)) return refuse(415, 'cover_bad_type', `The file is not a PNG, JPEG or WebP image. ${COVER_RULES}`);
  // A cover is the author's own file (Studio never makes one), and an alpha channel can hide a picture from one viewer and
  // not another, so a PNG with an alpha channel (RGBA, colour type 6) is refused. Save it as an RGB PNG, a JPEG or a WebP.
  if (pngColourType(body) === 6) return refuse(415, 'cover_bad_type', `The cover is a PNG with an alpha channel (RGBA); save it without transparency as an RGB PNG, a JPEG or a WebP. ${COVER_RULES}`);
  const problem = coverSizeProblem(seen.width, seen.height);
  if (problem) return refuse(422, Math.min(seen.width, seen.height) < COVER_MIN_SHORT_SIDE ? 'cover_too_small' : 'cover_bad_ratio', problem);

  // What is stored carries no metadata (location, device, text), and R2 checks the digest as it writes.
  let stored;
  try {
    stored = new Uint8Array(await images.stripMetadata(body));
  } catch {
    // A file whose structure cannot be walked is refused, never stored as it came.
    return refuse(415, 'cover_bad_type', `The image file could not be read to its end. ${COVER_RULES}`);
  }
  // An image on the takedown blocklist, as sent or as it would be stored, is refused (src/takedown.mjs).
  const blocked = await refuseBlockedUpload(env, [await sha256Hex(body), await sha256Hex(stored)], { json: deps.json, cors, accountId: userId, simId, route: 'cover' });
  if (blocked) return blocked;
  const key = coverKey(simId, ext);
  const storedSha = await images.sha256Hex(stored);
  await env.SCENES.put(key, stored, {
    httpMetadata: { contentType: contentTypeOf(ext) },
    customMetadata: { simId, authorId: userId },
    sha256: storedSha,
  });
  // Its public verdict, decided once now (src/moderation.mjs publicImageBody reads it).
  await recordUploadedImage(env, storedSha, stored);
  // One file per listing: a PNG replaced by a JPEG must not leave both, since the approval binds the first it finds.
  for (const other of EXTS) if (other !== ext) await env.SCENES.delete(coverKey(simId, other));

  // The record is read again just before it is written, so a write that landed while the image was
  // stored (a review decision on the same record) is not undone by this one.
  const cover = { ext, width: seen.width, height: seen.height, bytes: stored.byteLength, updated_at: new Date(Date.now()).toISOString() };
  const fresh = (await readSim(env, simId)) || sim;
  fresh.cover = cover;
  fresh.updated_at = cover.updated_at;
  // A new cover is not the one that was judged, binding or not: the listing goes back to review like any other change.
  sendBackToReview(fresh);
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(withTagsWrittenBack(fresh)));

  return deps.json({ ok: true, review: 'submit', ...coverFields(fresh, new URL(request.url).origin) }, 200, cors);
}

async function deleteCover(request, simId, env, cors, deps) {
  const { error, sim } = await ownedCoverTarget(request, simId, env, cors, deps);
  if (error) return error;
  let removed = !!sim.cover;
  for (const ext of EXTS) {
    if (await env.SCENES.head(coverKey(simId, ext))) {
      removed = true;
      await env.SCENES.delete(coverKey(simId, ext));
    }
  }
  if (sim.cover !== undefined) {
    const fresh = (await readSim(env, simId)) || sim;
    delete fresh.cover;
    fresh.updated_at = new Date(Date.now()).toISOString();
    await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(withTagsWrittenBack(fresh)));
  }
  // Nothing was there: nothing changed, so nothing needs a review.
  return deps.json({ ok: true, removed, review: removed ? 'submit' : null }, 200, cors);
}

// -----------------------------------------------------------------------------
// Reads
// -----------------------------------------------------------------------------

async function getCover(request, simId, env, cors, deps) {
  // One answer for every refusal: no cover, a listing that does not exist, and one that is held.
  const none = () => deps.json({ error: 'No cover' }, 404, cors);
  const raw = await readSim(env, simId);
  if (!raw) return none();
  // The listing as serving sees it: a legal lock overrides the marker.
  const sim = await simWithLegalLock(env, raw);
  // An adult listing's cover needs the signed URL its listing gives an allowed viewer, whoever asks: an image tag carries
  // no session (src/adult_content.mjs). Every other listing's cover is served as it always was.
  const unsigned = await refuseUnsignedImage(request, env, sim, deps.json, cors);
  if (unsigned) return unsigned;
  const cover = coverOf(sim);
  if (!cover) return none();

  const publicView = isListable(sim) && (await coverApproved(env, sim));
  const auth = await deps.verifyAuth(request, env);
  const admin = auth ? await deps.requireAdmin(request, env) : null;
  // The author and an admin may see their own cover before a review has bound it.
  const ownerView = !!auth && (auth === sim.author_id || !!admin) && canServe(sim, auth, !!admin);
  if (!publicView && !ownerView) return none();

  const object = await env.SCENES.get(coverKey(simId, cover.ext));
  if (!object) return none();
  // Too large to have been sent to the judge (stored before uploads were capped): absent to all but its author and an admin,
  // whatever the approval says. Decided from the stored size, never the body.
  if (!ownerView && overJudgeImageLimit(object)) { await object.body?.cancel?.(); return none(); }
  if (!ownerView) {
    // The object that is sent is the object that was judged: a file put between the check above and this read is not served.
    const binding = await readApproval(env, sim.id);
    if (binding && (binding.revoked || identOfObject(object) !== (binding.cover ?? null))) return none();
  }
  // The public is served the cover's stored verdict (src/moderation.mjs publicImageBody), streamed: the object, its rebuilt
  // copy, or nothing for one an upload would refuse today. Its author and an admin see the object itself.
  let body = object.body;
  if (!ownerView) {
    const shown = await publicImageBody(env, object);
    if (!shown) return none();
    body = shown.body;
  }
  return new Response(body, {
    headers: {
      ...cors,
      ...USER_IMAGE_HEADERS,
      'Content-Type': contentTypeOf(cover.ext),
      'Cache-Control': ownerView ? 'private, no-store' : imageCacheControl(request, sim, 'public, max-age=300'),
    },
  });
}

/// The review captures the public may see for this listing, as the moderation module reports them on this request.
/// `served` is the listing already read through the legal lock (or null), when the caller has read it.
async function previewsOf(simId, env, deps, served) {
  let sim = served;
  if (sim === undefined) {
    const raw = await readSim(env, simId);
    sim = raw ? await simWithLegalLock(env, raw) : null;
  }
  if (!sim || !isListable(sim)) return [];
  const ask = deps.approvedCaptureKeys || approvedCaptureKeys;
  const keys = await ask(env, sim);
  const byNumber = new Map();
  for (const k of Array.isArray(keys) ? keys : []) {
    if (Number.isInteger(k?.n) && k.n >= 0 && k.n <= 7 && EXTS.includes(k.ext) && typeof k.key === 'string' && !byNumber.has(k.n)) byNumber.set(k.n, k);
  }
  return [...byNumber.values()].sort((a, b) => a.n - b.n);
}

async function listPreviews(request, url, simId, env, cors, deps) {
  const origin = url.origin;
  const raw = await readSim(env, simId);
  const sim = raw ? await simWithLegalLock(env, raw) : null;
  // Adult content: the previews of a listed adult listing go to a viewer who turned it on, and to its author
  // (src/adult_content.mjs). The Gallery asks for this list with its session; such an answer is never cached, and each
  // frame's URL is signed, the only kind its route serves.
  let cache = 'public, max-age=60';
  const adult = !!sim && isListable(sim) && isAdultListing(sim);
  if (adult) {
    const refused = await refuseAdult(request, env, sim, { authenticate: deps.verifyAuth, json: deps.json, cors });
    if (refused) return refused;
    cache = 'private, no-store';
  }
  const previews = [];
  for (const k of await previewsOf(simId, env, deps, sim)) {
    const path = `/api/simulations/${simId}/preview/${k.n}`;
    const frameUrl = adult ? await signedImageUrl(env, origin, path) : `${origin}${path}`;
    // A frame that cannot be signed (no JWT_SECRET) is left out rather than offered as a URL its route refuses.
    if (frameUrl) previews.push({ n: k.n, url: frameUrl });
  }
  return deps.json({ previews }, 200, { ...cors, 'Cache-Control': cache });
}

/// One capture image. An adult listing's frame needs the signed URL its preview list gives an allowed viewer, whoever asks:
/// an image tag carries no session (src/adult_content.mjs).
async function getPreview(request, simId, n, env, cors, deps) {
  const none = () => deps.json({ error: 'No preview' }, 404, cors);
  const raw = await readSim(env, simId);
  const sim = raw ? await simWithLegalLock(env, raw) : null;
  if (sim) {
    const unsigned = await refuseUnsignedImage(request, env, sim, deps.json, cors);
    if (unsigned) return unsigned;
  }
  const hit = (await previewsOf(simId, env, deps, sim)).find((k) => k.n === n);
  if (!hit) return none();
  const object = await env.SCENES.get(hit.key);
  if (!object) return none();
  // The object that is sent is the capture the approval bound for this number, not whatever is stored under the key by now.
  const binding = await readApproval(env, simId);
  if (!binding || binding.revoked || identOfObject(object) !== (binding.captures?.[String(n)] ?? null)) return none();
  // The capture's stored verdict (src/moderation.mjs publicImageBody), streamed: the object, its rebuilt copy, or no preview
  // for one an upload would refuse today. Staff read it through the moderation capture route.
  const shown = await publicImageBody(env, object);
  if (!shown) return none();
  // As the author's tools uploaded it: served by the extension the moderation module names, never by a stored header.
  return new Response(shown.body, {
    headers: {
      ...cors,
      ...USER_IMAGE_HEADERS,
      'Content-Type': contentTypeOf(hit.ext),
      'Cache-Control': imageCacheControl(request, sim, 'public, max-age=300'),
    },
  });
}
