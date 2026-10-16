// ═══════════════════════════════════════════════════════════════════════════
// WORLD: a published simulation as .echk chunks
// ═══════════════════════════════════════════════════════════════════════════
//
// Studio publishes a Universe as content-addressed chunks plus one manifest
// that names them (the `WorldManifest` in eustress/crates/echk). A publish
// uploads only the chunks R2 does not already hold, so a republish that
// changed one corner of a map uploads that corner.
//
//   POST /api/simulations/{id}/world/begin           author  manifest in, missing chunks out
//   PUT  /api/simulations/{id}/world/chunks/{hash}   author  one chunk
//   POST /api/simulations/{id}/world/commit          author  the listing now plays this manifest
//   GET  /api/simulations/{id}/world/manifest        gated   the committed manifest
//   GET  /api/simulations/{id}/world/source          gated   the source world, for Edit
//   GET  /api/simulations/{id}/world/chunks/{hash}   gated   one chunk a manifest names
//
// Two worlds per listing. The manifest is what every player receives, with
// no server-only service in it (ServerScriptService, ServerStorage). The
// source world adds those services, and exists only when the author turned on
// Share Source: Studio commits it with `kind: "source"`, and only open-source
// listings (or their author, or an admin) can read it or its chunks.
//
// R2 layout: universes/{id}/chunks/{hash}.echk and
// universes/{id}/manifests/{sha256}.json. Chunks are stored per listing, so
// nothing one listing uploads can stand in for another's.
//
// Integrity. A Worker has no BLAKE3, so a chunk's name is its author's claim.
// The Worker checks what it can: the container header on upload, and at
// commit that every chunk the manifest names is stored at the size the
// manifest gives. The Player hashes every chunk before opening it and refuses
// a mismatch (crates/client/src/systems/space_fetch.rs), so a mislabelled
// chunk can break only its own listing's download.
//
// Reads serve only chunks the committed manifest names, and a committed chunk
// cannot be replaced. Without both, an approved listing would serve whatever
// its author uploaded afterwards, outside review.

export const WORLD_FORMAT = 'echk';
export const PAK_FORMAT = 'pak';

/// Whether a listing's current content is its world: an .echk listing whose r2_key names the manifest it serves. A .pak publish
/// moves r2_key and sets the format to pak; a world commit does the reverse. Both must hold, so the object an approval binds
/// (r2_key) is the object the world routes send. Without this, a world that was never reviewed is served under the approval of a
/// .pak published over it (Security's finding, 2026-10-08).
export function worldIsCurrent(sim) {
  return sim?.format === WORLD_FORMAT && !!sim.world?.manifest_key && sim.r2_key === sim.world.manifest_key;
}
/// Container versions a Player opens: 1 stores records raw, 2 as one zstd
/// frame (`eustress_echk::MIN_VERSION..=VERSION`).
export const WORLD_ENCODER_VERSIONS = [1, 2];
/// Limits a Player enforces (`eustress_echk`), repeated so a manifest the
/// Player would refuse is refused at upload instead.
export const MAX_WORLD_CHUNKS = 65_536;
export const MAX_CHUNK_BYTES = 512 * 1024 * 1024;
export const MAX_WORLD_BYTES = 8 * 1024 * 1024 * 1024;
/// One chunk per request, under the 100 MB request body limit.
export const MAX_UPLOAD_CHUNK_BYTES = 95 * 1024 * 1024;
/// A manifest lists at most 65,536 chunks at about 160 bytes each.
export const MAX_MANIFEST_BYTES = 16 * 1024 * 1024;
const MAX_PATH_BYTES = 1024;
const MAX_LISTING = { name: 120, description: 4000 };

/// How many players a session of a listing holds, as its Space authors it (Players.MaxPlayers). Everyone counts, the host's own
/// character included while it plays. The one bound is the gallery's own: the live heartbeat refuses more than
/// MAX_PLAYERS in live.mjs, which a test keeps equal to this. Unset means 10. docs/networking/MAX_PLAYERS.md.
export const DEFAULT_MAX_PLAYERS = 10;
export const MAX_PLAYERS_LIMIT = 10_000;
export const MAX_PLAYERS_MESSAGE = `max_players must be a whole number from 1 to ${MAX_PLAYERS_LIMIT.toLocaleString('en-US')}`;

/// `{ ok: true, value }` for a whole number from 1 to 10,000, otherwise `{ ok: false, error }` naming the range. A string, a
/// boolean, a fraction, zero and a negative number are all refused: nothing is rounded or clamped to fit.
export function checkMaxPlayers(value) {
  if (typeof value === 'number' && Number.isSafeInteger(value) && value >= 1 && value <= MAX_PLAYERS_LIMIT) return { ok: true, value };
  return { ok: false, error: MAX_PLAYERS_MESSAGE };
}

export const chunkKey = (simId, hash) => `universes/${simId}/chunks/${hash}.echk`;
export const manifestKey = (simId, sha256) => `universes/${simId}/manifests/${sha256}.json`;
const chunkPrefix = (simId) => `universes/${simId}/chunks/`;

const ROUTE = /^\/api\/simulations\/([a-f0-9-]+)\/world\/(begin|commit|manifest|source|chunks\/([^/]+))$/;

/// Dispatch a `/world/` request. `null` for any path this module does not own.
export async function handleWorldRoute(request, url, env, cors, deps) {
  const match = url.pathname.match(ROUTE);
  if (!match) return null;
  const [, simId, route, hash] = match;
  const method = request.method;
  if (route === 'begin' && method === 'POST') return handleBegin(request, simId, env, cors, deps);
  if (route === 'commit' && method === 'POST') return handleCommit(request, simId, env, cors, deps);
  if (route === 'manifest' && method === 'GET') return handleGetManifest(request, simId, env, cors, deps);
  if (route === 'source' && method === 'GET') return handleGetSource(request, simId, env, cors, deps);
  if (hash !== undefined && method === 'PUT') return handlePutChunk(request, simId, hash, env, cors, deps);
  if (hash !== undefined && method === 'GET') return handleGetChunk(request, simId, hash, env, cors, deps);
  return deps.json({ error: 'Method not allowed' }, 405, cors);
}

// ─────────────────────────────────────────────────────────────────────────────
// The manifest
// ─────────────────────────────────────────────────────────────────────────────

export function isContentHash(s) {
  return typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);
}

/// `eustress_echk::is_safe_record_path`, character for character: no empty,
/// `.` or `..` segment, no leading `/`, no `\`, no `:`, no control character.
export function isSafeRecordPath(path) {
  if (typeof path !== 'string' || path.length === 0 || path.startsWith('/')) return false;
  if (new TextEncoder().encode(path).length > MAX_PATH_BYTES) return false;
  if (path.includes('\\') || path.includes(':') || /[\u0000-\u001f\u007f-\u009f]/.test(path)) return false;
  return path.split('/').every((seg) => seg !== '' && seg !== '.' && seg !== '..');
}

/// `WorldManifest::validate`, plus what the listing needs from it.
///
/// Returns `{ error }`, or the Space names, the Space a player opens first,
/// every distinct chunk with its size, and the bytes a reader downloads.
export function checkWorldManifest(m) {
  const fail = (error) => ({ error });
  if (!m || typeof m !== 'object' || Array.isArray(m)) return fail('the manifest is not an object');
  if (m.format !== WORLD_FORMAT) return fail(`format ${JSON.stringify(m.format)}, expected "${WORLD_FORMAT}"`);
  if (!WORLD_ENCODER_VERSIONS.includes(m.encoder_version)) return fail(`unsupported .echk version ${JSON.stringify(m.encoder_version)}`);
  if (typeof m.chunk_size !== 'number' || !Number.isFinite(m.chunk_size) || m.chunk_size <= 0)
    return fail(`chunk_size ${JSON.stringify(m.chunk_size)}`);
  for (const key of ['engine_version', 'universe', 'start_space']) {
    if (m[key] !== undefined && typeof m[key] !== 'string') return fail(`${key} must be a string`);
  }
  if (!Array.isArray(m.spaces) || m.spaces.length === 0) return fail('no Spaces');
  const assets = m.assets === undefined ? [] : m.assets;
  if (!Array.isArray(assets)) return fail('assets must be a list');

  const names = [];
  const seen = new Set();
  const entries = [];
  for (const space of m.spaces) {
    const name = space?.name;
    if (!isSafeRecordPath(name) || name.includes('/') || name.startsWith('.'))
      return fail(`unsafe Space name ${JSON.stringify(name)}`);
    if (seen.has(name)) return fail(`Space ${JSON.stringify(name)} listed twice`);
    seen.add(name);
    names.push(name);
    if (!Array.isArray(space.chunks)) return fail(`Space ${JSON.stringify(name)}: chunks must be a list`);
    for (const c of space.chunks) entries.push(c);
  }
  const start = m.start_space || '';
  if (start && !seen.has(start)) return fail(`start_space ${JSON.stringify(start)} is not a listed Space`);
  for (const c of assets) entries.push(c);
  if (entries.length > MAX_WORLD_CHUNKS) return fail(`${entries.length} chunks exceeds ${MAX_WORLD_CHUNKS}`);

  const sizes = new Map();
  for (const c of entries) {
    if (!c || typeof c !== 'object' || Array.isArray(c)) return fail('a chunk entry is not an object');
    if (!isContentHash(c.blake3)) return fail(`chunk hash ${JSON.stringify(c.blake3)}`);
    if (!Number.isSafeInteger(c.cx) || !Number.isSafeInteger(c.cz) || !Number.isSafeInteger(c.count) || c.count < 0 || typeof c.file !== 'string')
      return fail(`chunk ${c.blake3}: malformed entry`);
    if (!Number.isSafeInteger(c.size) || c.size <= 0 || c.size > MAX_CHUNK_BYTES) return fail(`chunk ${c.blake3} size ${JSON.stringify(c.size)}`);
    const known = sizes.get(c.blake3);
    if (known !== undefined && known !== c.size) return fail(`chunk ${c.blake3} is listed with two sizes`);
    sizes.set(c.blake3, c.size);
  }
  let bytes = 0;
  for (const n of sizes.values()) bytes += n;
  if (bytes > MAX_WORLD_BYTES) return fail(`the world is ${bytes} bytes, over the ${MAX_WORLD_BYTES} limit`);
  return { spaces: names, start_space: start || names[0], sizes, bytes };
}

/// The request body of begin and commit: `{ manifest_json, publish_hash?, listing? }`.
///
/// The manifest travels as a string so the bytes stored, and hashed into its
/// key, are exactly the bytes the engine serialized.
async function readManifestBody(request, cors, deps) {
  const declared = Number(request.headers.get('Content-Length') || 0);
  if (declared > MAX_MANIFEST_BYTES + 64 * 1024) return { error: deps.json({ error: 'Manifest too large' }, 413, cors) };
  let body;
  try {
    body = await request.json();
  } catch {
    return { error: deps.json({ error: 'The body must be JSON' }, 400, cors) };
  }
  const text = body?.manifest_json;
  if (typeof text !== 'string') return { error: deps.json({ error: 'manifest_json is required' }, 400, cors) };
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > MAX_MANIFEST_BYTES) return { error: deps.json({ error: 'Manifest too large' }, 413, cors) };
  let manifest;
  try {
    manifest = JSON.parse(text);
  } catch {
    return { error: deps.json({ error: 'manifest_json is not JSON' }, 400, cors) };
  }
  const checked = checkWorldManifest(manifest);
  if (checked.error) return { error: deps.json({ error: `Bad world manifest: ${checked.error}`, code: 'bad_manifest' }, 422, cors) };
  const oversized = [...checked.sizes].filter(([, n]) => n > MAX_UPLOAD_CHUNK_BYTES);
  if (oversized.length) {
    return {
      error: deps.json({
        error: `${oversized.length} chunk(s) exceed the ${MAX_UPLOAD_CHUNK_BYTES} byte upload limit`,
        code: 'chunk_too_large',
        max_chunk_bytes: MAX_UPLOAD_CHUNK_BYTES,
        chunks: oversized.map(([hash, size]) => ({ hash, size })),
      }, 413, cors),
    };
  }
  return { body, bytes, manifest, checked };
}

// ─────────────────────────────────────────────────────────────────────────────
// Storage helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Every chunk stored for a listing, hash to size.
async function storedChunks(env, simId) {
  const prefix = chunkPrefix(simId);
  const out = new Map();
  let cursor;
  do {
    const page = await env.SCENES.list({ prefix, cursor, limit: 1000 });
    for (const obj of page.objects) {
      const hash = obj.key.slice(prefix.length, -'.echk'.length);
      if (obj.key.endsWith('.echk') && isContentHash(hash)) out.set(hash, obj.size);
    }
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
  return out;
}

// Parsed chunk sets of committed manifests. A manifest's key is the SHA-256
// of its bytes, so an entry can never go stale; the cap only bounds memory.
const committedCache = new Map();
const COMMITTED_CACHE_CAP = 16;

/// The chunks a committed manifest names, or `null` when it cannot be read.
async function committedHashes(env, key) {
  const hit = committedCache.get(key);
  if (hit) return hit;
  const obj = await env.SCENES.get(key);
  if (!obj) return null;
  let manifest;
  try {
    manifest = JSON.parse(await obj.text());
  } catch {
    return null;
  }
  const set = new Set();
  for (const space of manifest?.spaces || []) for (const c of space?.chunks || []) set.add(c?.blake3);
  for (const c of manifest?.assets || []) set.add(c?.blake3);
  if (committedCache.size >= COMMITTED_CACHE_CAP) committedCache.delete(committedCache.keys().next().value);
  committedCache.set(key, set);
  return set;
}

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

import { contentLockedFor, inReview, simForServing, sourceApproved, REVIEW_SCHEMA } from './moderation.mjs';
import { checkTags, listingTags, genreTag, setAuthorTags, withTagsWrittenBack, listingTagFields } from './tags.mjs';
import { refuseAdult } from './adult_content.mjs';
import { refuseBlockedUpload } from './takedown.mjs';

/// A listing its author may publish into.
async function ownedListing(request, simId, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return { error: deps.json({ error: 'Unauthorized' }, 401, cors) };
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return { error: deps.json({ error: 'Simulation not found' }, 404, cors) };
  const sim = JSON.parse(raw);
  if (sim.author_id !== userId) return { error: deps.json({ error: 'Not your simulation' }, 403, cors) };
  // A legal hold covers storage, so its author cannot swap the content under
  // it, and the freeze covers every listing the author owns: re-uploading into
  // an existing listing would otherwise walk around it. The CASE decides, not
  // the listing's marker. The refusal reads as ordinary review: naming the
  // hold would tell a suspected uploader that one exists (see authorView in
  // moderation.mjs).
  if (await contentLockedFor(env, sim, userId)) return { error: inReview(deps.json, cors) };
  return { sim, userId };
}

/// A listing this viewer may download: the same gate as the `.pak` download.
async function servableListing(request, simId, env, cors, deps, kind = 'content') {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return { error: deps.json({ error: 'Simulation not found' }, 404, cors) };
  // Through the legal lock and the approval binding (see simForServing).
  const sim = await simForServing(env, JSON.parse(raw), kind);
  if (deps.isListable(sim)) {
    // Adult content goes to a viewer who turned it on, and to its author (src/adult_content.mjs). The manifest, every
    // chunk and the source world all pass here; the Player sends its session with each (crates/client space_fetch.rs).
    const refused = await refuseAdult(request, env, sim, { authenticate: deps.verifyAuth, json: deps.json, cors });
    if (refused) return { error: refused };
    return { sim, listable: true };
  }
  const auth = await deps.verifyAuth(request, env);
  const admin = auth ? await deps.requireAdmin(request, env) : null;
  if (!deps.canServe(sim, auth, !!admin))
    return { error: deps.json({ error: 'Simulation not available' }, 403, cors) };
  return { sim, listable: false };
}

/// Listing text sent with a commit. Returns whether anything changed. `tags` is the commit's already-checked tag list, or
/// null when the commit sent none.
function applyListing(sim, listing, tags = null) {
  if (!listing || typeof listing !== 'object') return false;
  let changed = false;
  const set = (key, value) => {
    if (sim[key] !== value) {
      sim[key] = value;
      changed = true;
    }
  };
  if (typeof listing.name === 'string' && listing.name.trim()) set('name', listing.name.trim().slice(0, MAX_LISTING.name));
  if (typeof listing.description === 'string') set('description', listing.description.trim().slice(0, MAX_LISTING.description));
  // Tags are the only category. A `genre` (an older Studio still sends one; an empty one is absent) is ignored, except when
  // the commit sends no tags and the listing has none: then a genre that is a valid tag, and not "all", becomes its one tag,
  // the author's, as a genre from before tags does (src/tags.mjs genreTag). It is a change the review reads.
  if (tags === null && listingTags(sim).length === 0) {
    const genre = genreTag({ genre: listing.genre });
    if (genre !== null) {
      setAuthorTags(sim, [genre]);
      sim.genre = genre;
      changed = true;
    }
  }
  // An archived listing stays private whatever a publish says; the publish's choice is what Unarchive restores.
  if (typeof listing.is_public === 'boolean') {
    if (sim.archived === true) sim.archived_was_public = listing.is_public;
    else set('is_public', listing.is_public);
  }
  // The author's "Share Source": the gallery offers Edit (open in Studio)
  // only on a listing that carries it.
  if (typeof listing.open_source === 'boolean') set('open_source', listing.open_source);
  // Tags are text a reviewer reads: a different set is the author's and sends the listing back to review, as a new name
  // does; `[]` asks the review to choose them. The same tags in another order are stored in that order and are no change,
  // and a judge's tags sent back as they are stay the judge's. docs/networking/LISTING_TAGS.md.
  if (tags !== null && setAuthorTags(sim, tags)) changed = true;
  return changed;
}

// ─────────────────────────────────────────────────────────────────────────────
// Author routes
// ─────────────────────────────────────────────────────────────────────────────

async function handleBegin(request, simId, env, cors, deps) {
  const owned = await ownedListing(request, simId, env, cors, deps);
  if (owned.error) return owned.error;
  const read = await readManifestBody(request, cors, deps);
  if (read.error) return read.error;
  const { sizes, bytes } = read.checked;
  const stored = await storedChunks(env, simId);
  const missing = [...sizes].filter(([hash, size]) => stored.get(hash) !== size).map(([hash]) => hash);
  return deps.json({
    missing,
    present: sizes.size - missing.length,
    chunks: sizes.size,
    bytes,
    max_chunk_bytes: MAX_UPLOAD_CHUNK_BYTES,
  }, 200, cors);
}

async function handlePutChunk(request, simId, hash, env, cors, deps) {
  const owned = await ownedListing(request, simId, env, cors, deps);
  if (owned.error) return owned.error;
  if (!isContentHash(hash)) return deps.json({ error: 'A chunk is named by its BLAKE3 hash: 64 lowercase hex characters' }, 400, cors);
  const length = Number(request.headers.get('Content-Length'));
  if (!Number.isSafeInteger(length) || length <= 0) return deps.json({ error: 'Content-Length is required' }, 411, cors);
  if (length < 12) return deps.json({ error: 'Not an .echk chunk' }, 422, cors);
  if (length > MAX_UPLOAD_CHUNK_BYTES)
    return deps.json({ error: `Chunk too large (max ${MAX_UPLOAD_CHUNK_BYTES} bytes)`, code: 'chunk_too_large' }, 413, cors);

  const key = chunkKey(simId, hash);
  const existing = await env.SCENES.head(key);
  if (existing && existing.size === length) return deps.json({ hash, stored: false, present: true }, 200, cors);
  if (existing && owned.sim.world) {
    for (const committedKey of [owned.sim.world.manifest_key, owned.sim.world.source_manifest_key].filter(Boolean)) {
      const committed = await committedHashes(env, committedKey);
      if (committed?.has(hash))
        return deps.json({ error: 'This chunk is part of the published world and cannot be replaced', code: 'chunk_committed' }, 409, cors);
    }
  }

  // Streamed: a request body with a Content-Length has the known length R2 needs.
  await env.SCENES.put(key, request.body, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, authorId: owned.userId },
  });

  // The header, read back: "ECHK" then the version as a little-endian u32.
  // Stored exactly as sent: a version 2 chunk is already compressed, and its
  // name hashes these bytes.
  const head = await env.SCENES.get(key, { range: { offset: 0, length: 8 } });
  const b = head ? new Uint8Array(await head.arrayBuffer()) : new Uint8Array(0);
  const valid = b.length >= 8 && b[0] === 0x45 && b[1] === 0x43 && b[2] === 0x48 && b[3] === 0x4b
    && WORLD_ENCODER_VERSIONS.includes(b[4] | (b[5] << 8) | (b[6] << 16) | (b[7] << 24));
  if (!valid) {
    await env.SCENES.delete(key);
    return deps.json({ error: `Not an .echk chunk of version ${WORLD_ENCODER_VERSIONS.join(' or ')}` }, 422, cors);
  }
  return deps.json({ hash, stored: true, size: length }, 201, cors);
}

async function handleCommit(request, simId, env, cors, deps) {
  const owned = await ownedListing(request, simId, env, cors, deps);
  if (owned.error) return owned.error;
  const read = await readManifestBody(request, cors, deps);
  if (read.error) return read.error;
  const { sizes, bytes, spaces, start_space } = read.checked;

  // The authored player count travels with the listing text. Checked before anything is written, so a number outside the
  // range leaves the listing as it was.
  let maxPlayers = null;
  const sent = read.body.listing && typeof read.body.listing === 'object' ? read.body.listing.max_players : undefined;
  if (sent !== undefined && sent !== null) {
    const checked = checkMaxPlayers(sent);
    if (!checked.ok) return deps.json({ error: checked.error, code: 'bad_max_players' }, 400, cors);
    maxPlayers = checked.value;
  }

  // The listing's tags are checked the same way: a bad one refuses the whole commit before anything is written. Absent or
  // null is "not sent" (the tags stay as they were); an empty list clears them.
  let tags = null;
  const sentTags = read.body.listing && typeof read.body.listing === 'object' ? read.body.listing.tags : undefined;
  if (sentTags !== undefined && sentTags !== null) {
    const checked = checkTags(sentTags);
    if (!checked.ok) return deps.json({ error: checked.error, code: 'bad_tags' }, 400, cors);
    tags = checked.tags;
  }

  const stored = await storedChunks(env, simId);
  const missing = [...sizes].filter(([hash, size]) => stored.get(hash) !== size).map(([hash]) => hash);
  if (missing.length)
    return deps.json({ error: `${missing.length} chunk(s) are not uploaded`, code: 'chunks_missing', missing }, 409, cors);

  const sim = owned.sim;
  const source = read.body.kind === 'source';
  if (source && !worldIsCurrent(sim))
    return deps.json({ error: 'Commit the world before its source', code: 'no_world' }, 409, cors);
  if (source && sim.open_source !== true)
    return deps.json({ error: 'Share Source is off for this listing', code: 'not_open_source' }, 409, cors);

  const sha = await sha256Hex(read.bytes);
  // A world (or source world) manifest on the takedown blocklist is refused before anything is stored (src/takedown.mjs).
  const blocked = await refuseBlockedUpload(env, [sha], { json: deps.json, cors, accountId: owned.userId, simId, route: source ? 'world_source' : 'world_commit' });
  if (blocked) return blocked;
  const key = manifestKey(simId, sha);
  // Create-if-absent. The key is the bytes' own SHA-256, so bytes already stored under it are the same world, and a second
  // put would only move the object to a new R2 version. An approval is bound to the version it reviewed (identOfObject),
  // so an identical re-commit would send the listing back to pending for everyone but its author. Same rule for the
  // source world, which shares this key space. `sha256` makes R2 verify the bytes and keep the hash, so a new manifest's
  // identity is its SHA-256 and no later re-put, copy or migration can move it.
  await env.SCENES.put(key, read.bytes, {
    httpMetadata: { contentType: 'application/json' },
    customMetadata: { simId, authorId: owned.userId },
    onlyIf: { etagDoesNotMatch: '*' },
    sha256: sha,
  });
  const now = new Date().toISOString();

  // The source world only adds the server-only services to the world
  // players already get; it changes nothing they play, so no review.
  if (source) {
    sim.world = { ...sim.world, source_manifest_key: key, source_manifest_sha256: sha, source_committed_at: now };
    sim.updated_at = now;
    await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(withTagsWrittenBack(sim)));
    return deps.json({ ok: true, kind: 'source', manifest_sha256: sha, chunks: sizes.size, bytes }, 200, cors);
  }

  const prior = sim.world;
  const sameWorld = prior?.manifest_sha256 === sha;
  const listingChanged = applyListing(sim, read.body.listing, tags);
  // A player count is a number about hosting, not content or text a reviewer reads: changing it does not send an approved
  // listing back to review (listingChanged stays as the text left it).
  if (maxPlayers !== null) sim.max_players = maxPlayers;
  const publishHash = isContentHash(read.body.publish_hash) ? read.body.publish_hash : null;
  if (prior && !sameWorld) sim.version = (sim.version || 1) + 1;

  sim.format = WORLD_FORMAT;
  sim.world = {
    manifest_key: key,
    manifest_sha256: sha,
    publish_hash: publishHash,
    universe: String(read.manifest.universe || '').slice(0, 200),
    engine_version: String(read.manifest.engine_version || '').slice(0, 64),
    start_space,
    spaces: spaces.length,
    chunks: sizes.size,
    bytes,
    committed_at: now,
  };
  // A source belongs to the world it was committed with: kept when the same
  // world is committed again with Share Source still on, dropped otherwise.
  // Studio commits the fresh source right after a new world.
  if (sameWorld && sim.open_source === true && prior?.source_manifest_key) {
    sim.world.source_manifest_key = prior.source_manifest_key;
    sim.world.source_manifest_sha256 = prior.source_manifest_sha256;
    sim.world.source_committed_at = prior.source_committed_at;
  }
  // What moderation's submit heads before it will review a listing.
  sim.r2_key = key;
  // Moderation dedups on (author, etag, size) of an uploaded .pak. A manifest
  // is not one, so an .echk listing is always reviewed afresh.
  sim.pak_etag = null;
  sim.pak_sha256 = null;
  sim.scene_size_bytes = bytes;
  if (publishHash) sim.content_root = `blake3:${publishHash}`;
  const names = deps.readSpaceNames(spaces);
  if (names) sim.space_names = names;
  // New content, or new text describing it, goes back to review. Committing
  // the same world with the same text again (a retry) changes nothing.
  if (!sameWorld || listingChanged) {
    sim.moderation = {
      ...(sim.moderation || {}),
      status: 'pending',
      version: deps.MODERATION_VERSION,
      policy_version: deps.POLICY_VERSION,
      // Guardian Code Part VIII: the review schema the listing is published under.
      review_schema: REVIEW_SCHEMA,
    };
  }
  sim.updated_at = now;
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(withTagsWrittenBack(sim)));

  return deps.json({
    ok: true,
    format: WORLD_FORMAT,
    manifest_sha256: sha,
    chunks: sizes.size,
    bytes,
    version: sim.version || 1,
    // The listing's tags as stored, and who chose them, so Studio shows (and sends back) what the listing carries.
    ...listingTagFields(sim),
    // Author-facing status only; held and quarantined both read as in_review.
    moderation: ({ approved: sim.is_public === false ? 'approved_private' : 'listed', rejected: 'not_listed', changes_requested: 'changes_requested', appealed: 'appeal_in_review' })[sim.moderation?.status] || 'in_review',
  }, 200, cors);
}

// ─────────────────────────────────────────────────────────────────────────────
// Player routes
// ─────────────────────────────────────────────────────────────────────────────

async function handleGetManifest(request, simId, env, cors, deps) {
  const gate = await servableListing(request, simId, env, cors, deps);
  if (gate.error) return gate.error;
  const sim = gate.sim;
  if (!worldIsCurrent(sim)) {
    if (!sim.r2_key) return deps.json({ error: 'Nothing has been published to this listing yet' }, 404, cors);
    // Published before .echk. The Player falls back to the .pak on this answer.
    return deps.json({ error: 'This simulation was published as a .pak', format: 'pak', download: `/api/simulations/${simId}/download` }, 409, cors);
  }
  const obj = await env.SCENES.get(sim.world.manifest_key);
  if (!obj) return deps.json({ error: 'The manifest is missing from storage' }, 404, cors);
  // A Player fetching the world to play it is a visit (once a day per person: src/visits.mjs). Never fails the read.
  if (deps.onLaunch) await deps.onLaunch(request, sim).catch((e) => console.error('onLaunch failed:', e?.message || e));
  const headers = {
    ...cors,
    'Content-Type': 'application/json',
    // The listing can be republished at any time; the manifest it points at is
    // read fresh each time.
    'Cache-Control': 'private, no-store',
    'X-Eustress-Manifest-Sha256': sim.world.manifest_sha256,
  };
  if (sim.world.publish_hash) headers['X-Eustress-Publish-Hash'] = sim.world.publish_hash;
  return new Response(obj.body, { headers });
}

/// Whether this viewer may read a listing's source world: anyone when its
/// author shares the source, otherwise its author or an admin.
async function mayReadSource(request, sim, env, deps) {
  if (sim.open_source === true) return true;
  const auth = await deps.verifyAuth(request, env);
  if (!auth) return false;
  return auth === sim.author_id || !!(await deps.requireAdmin(request, env));
}

async function handleGetSource(request, simId, env, cors, deps) {
  const gate = await servableListing(request, simId, env, cors, deps, 'source');
  if (gate.error) return gate.error;
  const sim = gate.sim;
  // Not found either way, so a private source is indistinguishable from none.
  if (!worldIsCurrent(sim) || !sim.world?.source_manifest_key || !(await mayReadSource(request, sim, env, deps)))
    return deps.json({ error: 'This listing shares no source' }, 404, cors);
  const obj = await env.SCENES.get(sim.world.source_manifest_key);
  if (!obj) return deps.json({ error: 'The source manifest is missing from storage' }, 404, cors);
  return new Response(obj.body, {
    headers: {
      ...cors,
      'Content-Type': 'application/json',
      'Cache-Control': 'private, no-store',
      'X-Eustress-Manifest-Sha256': sim.world.source_manifest_sha256,
    },
  });
}

async function handleGetChunk(request, simId, hash, env, cors, deps) {
  if (!isContentHash(hash)) return deps.json({ error: 'Not a chunk name' }, 400, cors);
  const gate = await servableListing(request, simId, env, cors, deps);
  if (gate.error) return gate.error;
  const sim = gate.sim;
  if (!worldIsCurrent(sim)) return deps.json({ error: 'Chunk not found' }, 404, cors);
  // A chunk the world names, or one only the source world names (a
  // server-only service) for a viewer who may read the source.
  let named = (await committedHashes(env, sim.world.manifest_key))?.has(hash);
  if (!named && sim.world.source_manifest_key && (await mayReadSource(request, sim, env, deps)) && (!gate.listable || (await sourceApproved(env, sim))))
    named = (await committedHashes(env, sim.world.source_manifest_key))?.has(hash);
  if (!named) return deps.json({ error: 'Chunk not found' }, 404, cors);
  const obj = await env.SCENES.get(chunkKey(simId, hash));
  if (!obj) return deps.json({ error: 'Chunk not found' }, 404, cors);
  return new Response(obj.body, {
    headers: {
      ...cors,
      'Content-Type': 'application/octet-stream',
      'Content-Length': String(obj.size),
      // A chunk's bytes never change under its name. Private either way: a
      // listing can be withdrawn, and shared caches would not notice.
      'Cache-Control': gate.listable ? 'private, max-age=86400, immutable' : 'private, no-store',
      ETag: `"${hash}"`,
    },
  });
}
