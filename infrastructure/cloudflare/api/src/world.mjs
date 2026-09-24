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
//   GET  /api/simulations/{id}/world/chunks/{hash}   gated   one chunk the manifest names
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
const MAX_LISTING = { name: 120, description: 4000, genre: 40 };

export const chunkKey = (simId, hash) => `universes/${simId}/chunks/${hash}.echk`;
export const manifestKey = (simId, sha256) => `universes/${simId}/manifests/${sha256}.json`;
const chunkPrefix = (simId) => `universes/${simId}/chunks/`;

const ROUTE = /^\/api\/simulations\/([a-f0-9-]+)\/world\/(begin|commit|manifest|chunks\/([^/]+))$/;

/// Dispatch a `/world/` request. `null` for any path this module does not own.
export async function handleWorldRoute(request, url, env, cors, deps) {
  const match = url.pathname.match(ROUTE);
  if (!match) return null;
  const [, simId, route, hash] = match;
  const method = request.method;
  if (route === 'begin' && method === 'POST') return handleBegin(request, simId, env, cors, deps);
  if (route === 'commit' && method === 'POST') return handleCommit(request, simId, env, cors, deps);
  if (route === 'manifest' && method === 'GET') return handleGetManifest(request, simId, env, cors, deps);
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

/// A listing its author may publish into.
async function ownedListing(request, simId, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return { error: deps.json({ error: 'Unauthorized' }, 401, cors) };
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return { error: deps.json({ error: 'Simulation not found' }, 404, cors) };
  const sim = JSON.parse(raw);
  if (sim.author_id !== userId) return { error: deps.json({ error: 'Not your simulation' }, 403, cors) };
  // A legal hold covers storage, so its author cannot swap the content under it.
  if (sim.moderation?.status === 'quarantined')
    return { error: deps.json({ error: 'This listing is under legal review and cannot be changed', code: 'quarantined' }, 409, cors) };
  // Same freeze the create route honours: re-uploading into an existing
  // listing would otherwise walk around it.
  if (await env.USERS.get(`publish-frozen:${userId}`))
    return { error: deps.json({ error: 'Publishing is paused on this account pending review', code: 'publish_frozen' }, 403, cors) };
  return { sim, userId };
}

/// A listing this viewer may download: the same gate as the `.pak` download.
async function servableListing(request, simId, env, cors, deps) {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return { error: deps.json({ error: 'Simulation not found' }, 404, cors) };
  const sim = JSON.parse(raw);
  if (deps.isListable(sim)) return { sim, listable: true };
  const auth = await deps.verifyAuth(request, env);
  const admin = auth ? await deps.requireAdmin(request, env) : null;
  if (!deps.canServe(sim, auth, !!admin))
    return { error: deps.json({ error: 'Simulation not available' }, sim.moderation?.status === 'quarantined' ? 451 : 403, cors) };
  return { sim, listable: false };
}

/// Listing text sent with a commit. Returns whether anything changed.
function applyListing(sim, listing) {
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
  if (typeof listing.genre === 'string' && listing.genre.trim()) set('genre', listing.genre.trim().slice(0, MAX_LISTING.genre));
  if (typeof listing.is_public === 'boolean') set('is_public', listing.is_public);
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
  if (existing && owned.sim.world?.manifest_key) {
    const committed = await committedHashes(env, owned.sim.world.manifest_key);
    if (committed?.has(hash))
      return deps.json({ error: 'This chunk is part of the published world and cannot be replaced', code: 'chunk_committed' }, 409, cors);
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

  const stored = await storedChunks(env, simId);
  const missing = [...sizes].filter(([hash, size]) => stored.get(hash) !== size).map(([hash]) => hash);
  if (missing.length)
    return deps.json({ error: `${missing.length} chunk(s) are not uploaded`, code: 'chunks_missing', missing }, 409, cors);

  const sha = await sha256Hex(read.bytes);
  const key = manifestKey(simId, sha);
  await env.SCENES.put(key, read.bytes, {
    httpMetadata: { contentType: 'application/json' },
    customMetadata: { simId, authorId: owned.userId },
  });

  const sim = owned.sim;
  const now = new Date().toISOString();
  const sameWorld = sim.world?.manifest_sha256 === sha;
  const listingChanged = applyListing(sim, read.body.listing);
  const publishHash = isContentHash(read.body.publish_hash) ? read.body.publish_hash : null;
  if (sim.world && !sameWorld) sim.version = (sim.version || 1) + 1;

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
  // What moderation's submit heads before it will review a listing.
  sim.r2_key = key;
  // Moderation dedups on (author, etag, size) of an uploaded .pak. A manifest
  // is not one, so an .echk listing is always reviewed afresh.
  sim.pak_etag = null;
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
    };
  }
  sim.updated_at = now;
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return deps.json({
    ok: true,
    format: WORLD_FORMAT,
    manifest_sha256: sha,
    chunks: sizes.size,
    bytes,
    version: sim.version || 1,
    moderation: sim.moderation?.status || 'pending',
  }, 200, cors);
}

// ─────────────────────────────────────────────────────────────────────────────
// Player routes
// ─────────────────────────────────────────────────────────────────────────────

async function handleGetManifest(request, simId, env, cors, deps) {
  const gate = await servableListing(request, simId, env, cors, deps);
  if (gate.error) return gate.error;
  const sim = gate.sim;
  if (sim.format !== WORLD_FORMAT || !sim.world?.manifest_key) {
    if (!sim.r2_key) return deps.json({ error: 'Nothing has been published to this listing yet' }, 404, cors);
    // Published before .echk. The Player falls back to the .pak on this answer.
    return deps.json({ error: 'This simulation was published as a .pak', format: 'pak', download: `/api/simulations/${simId}/download` }, 409, cors);
  }
  const obj = await env.SCENES.get(sim.world.manifest_key);
  if (!obj) return deps.json({ error: 'The manifest is missing from storage' }, 404, cors);
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

async function handleGetChunk(request, simId, hash, env, cors, deps) {
  if (!isContentHash(hash)) return deps.json({ error: 'Not a chunk name' }, 400, cors);
  const gate = await servableListing(request, simId, env, cors, deps);
  if (gate.error) return gate.error;
  const sim = gate.sim;
  if (sim.format !== WORLD_FORMAT || !sim.world?.manifest_key) return deps.json({ error: 'Chunk not found' }, 404, cors);
  const committed = await committedHashes(env, sim.world.manifest_key);
  if (!committed?.has(hash)) return deps.json({ error: 'Chunk not found' }, 404, cors);
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
