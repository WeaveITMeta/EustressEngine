// ═══════════════════════════════════════════════════════════════════════════
// PACKAGE SCRIPTS: every script a published listing ships, read from the package itself
// ═══════════════════════════════════════════════════════════════════════════
//
// The listing review reads every script a listing ships, in full, from what was published: the `.echk` world package in R2
// (src/world.mjs), not the dossier Studio builds on the author's machine. Two packages per listing: the players' package
// (`sim.world.manifest_key`) and, when the author shares source, the source package (`sim.world.source_manifest_key`), which
// adds the server-only services. Each script is tagged with the package it ships in: audience `players` or `source`.
//
// What counts as a script: a record whose path ends in one of SCRIPT_EXTENSIONS (any case), and code written inline in an
// `_instance.toml` record, under `[script] source`. A `source` that is one line naming a script file (a SoulScript's
// `source = "StationMind.rune"`) is a reference to a sibling record, which is collected as a file of its own.
//
// Resumable. A production listing can name hundreds of chunks and half a gigabyte, more than one invocation's R2 operations
// and CPU allow. Each call reads chunks in a fixed order under a budget (`opts.maxChunks`, `opts.maxBytes`), and writes what
// it found and how far it got under universes/{simId}/moderation/package_scripts/{key}/, where {key} is the SHA-256 of the
// players' manifest, then `_` and the source manifest's SHA-256 when there is one. A republish names a new key and starts
// over; an unchanged package is read once. Nothing here writes, replaces or deletes anything under `chunks/` or `manifests/`:
// an approval is bound to those objects' versions.
//
//   {key}/units.json        the chunks to read, in order, each with the Spaces and audiences that name it (written once)
//   {key}/state.json        progress: next chunk, failed chunks, the part files, the sources already stored
//   {key}/parts/{id}.json   the scripts found by one checkpoint; a source is stored once, later copies name its sha256
//
// Fail closed. A chunk that cannot be fetched or decoded is never skipped: it is listed in `errors`, `complete` stays false,
// and the next call reads it again. A script too large for the review to read in full is an error too, never a truncated
// read, since anything past a cut would ship unread.
//
// Chunks are hostile input. Every length is bounds-checked against what the chunk declares and what the bytes hold, a
// version 2 chunk may inflate to at most MAX_RAW_BYTES and must inflate to exactly the length it declares, and its zstd
// frame header is checked before the decoder sees it (a forged window size would otherwise make the decoder allocate up to
// 2 GB). The container is `eustress_echk` (eustress/crates/echk/src/lib.rs): `decode_chunk`, `inflate_v2`, `parse_records`.
// Like the Player, this reads one zstd frame and ignores bytes after it, and ignores record-stream bytes after the declared
// record count.
//
// The Worker has no BLAKE3, so a chunk's bytes are not checked against its name here. The Player refuses a chunk whose bytes
// do not hash to its name (crates/client/src/systems/space_fetch.rs), so a chunk that differs from its name cannot run.

// fzstd 0.1.1 (MIT, 101arrowz), pinned exactly: the Workers runtime has no zstd DecompressionStream, and this is a small
// pure-JS decoder with a streaming API. 0.1.1 was published 2024-03-15 and is the latest release. Only its streaming
// `Decompress` is used: memory stays at one window (capped below by MAX_WINDOW_BYTES) plus one block, whatever the chunk's
// size, and the bytes it produces can be counted against the declared length as they arrive.
import { Decompress } from 'fzstd';

export const SCRIPT_EXTENSIONS = ['.luau', '.lua', '.rune', '.rn', '.soul'];
const RUNE_EXTENSIONS = new Set(['.rune', '.rn', '.soul']);

/// `eustress_echk::MAX_RAW_BYTES`: the largest record stream a version 2 chunk may inflate to.
export const MAX_RAW_BYTES = 256 * 1024 * 1024;
/// `eustress_echk::MAX_PATH_LEN`.
export const MAX_PATH_LEN = 1024;
/// The largest zstd window a chunk may ask for. A chunk is compressed at level 9 (`eustress_echk::ZSTD_LEVEL`), whose window
/// is at most 8 MiB; the decoder allocates the window up front, so a forged header is refused here.
export const MAX_WINDOW_BYTES = 32 * 1024 * 1024;
/// The largest script, file or `_instance.toml`, the review reads. Over it is an error, never a cut: the rest would ship
/// unread. Production's largest script is about 62 KB.
export const MAX_SCRIPT_BYTES = 8 * 1024 * 1024;
/// The most script bytes one chunk may hold, so one chunk cannot fill the isolate (16 MiB: a Worker isolate has 128 MB, and a
/// chunk's scripts are held while they are hashed and written; Code v2.4 review, 2026-11-11).
export const MAX_CHUNK_SCRIPT_BYTES = 16 * 1024 * 1024;
const MAX_MANIFEST_BYTES = 16 * 1024 * 1024;
const MAX_MANIFEST_CHUNKS = 65_536;
const MAX_CHUNK_BYTES = 512 * 1024 * 1024;
/// zstd's Block_Maximum_Size ceiling.
const MAX_BLOCK_BYTES = 128 * 1024;

const DEFAULT_MAX_CHUNKS = 300;
const DEFAULT_MAX_BYTES = 200 * 1000 * 1000;
/// Progress is written every this many chunks, so an invocation cut off by its CPU limit loses at most this much work.
const DEFAULT_CHECKPOINT_CHUNKS = 50;
/// A part file is written once the sources it holds pass this.
const PART_SOURCE_BYTES = 4 * 1024 * 1024;
const STATE_VERSION = 1;

const JSON_META = { httpMetadata: { contentType: 'application/json' } };
const HEX64 = /^[0-9a-f]{64}$/;
const SIM_ID = /^[A-Za-z0-9_-]{1,128}$/;

/// `chunkKey` in src/world.mjs, repeated so this module does not import world.mjs (which imports moderation.mjs, the module
/// that imports this one). A test keeps the two equal.
export const chunkKeyOf = (simId, hash) => `universes/${simId}/chunks/${hash}.echk`;
const storePrefix = (simId, key) => `universes/${simId}/moderation/package_scripts/${key}/`;

class ChunkError extends Error {}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Extract (or continue extracting) every script in sim's published package.
///
/// `opts`: `maxChunks` (default 300) and `maxBytes` (default 200 MB, compressed) bound one call, and `budgetMs` (optional)
/// stops it starting a new chunk once that much time has passed. A call always reads at least one chunk.
///
/// Returns `{ supported, complete, key, chunks_total, chunks_read, scripts_total, script_bytes, distinct_scripts,
/// distinct_bytes, errors: [{ chunk, reason }] }`. `scripts_total` and `script_bytes` count every script at every path and
/// audience; `distinct_*` count each distinct source once. A call on a complete package reads no chunk.
export async function extractPackageScripts(env, sim, opts = {}) {
  const located = await locate(env, sim);
  if (located.result) return located.result;
  const { simId, key, dir } = located;
  const maxChunks = positive(opts.maxChunks, DEFAULT_MAX_CHUNKS);
  const maxBytes = positive(opts.maxBytes, DEFAULT_MAX_BYTES);
  const checkpointChunks = positive(opts.checkpointChunks, DEFAULT_CHECKPOINT_CHUNKS);
  // Optional: no new chunk is started once this many milliseconds have passed. In a Worker the clock moves on I/O, and every
  // chunk is a read, so this bounds a call's wall time to about the budget plus one chunk.
  const budgetMs = positive(opts.budgetMs, Infinity);
  const started = Date.now();

  let { state, etag } = await readState(env, dir);
  if (state?.complete) return summary(key, state);

  let units = state ? await readUnits(env, dir, state) : null;
  if (!units) {
    const loaded = located.manifests || (await loadManifests(env, sim));
    if (loaded.error) return failure(key, loaded.error);
    units = buildUnits(loaded.manifests);
    await env.SCENES.put(`${dir}units.json`, JSON.stringify({ v: STATE_VERSION, key, units }), JSON_META);
    if (!state) {
      state = freshState(key, units.length);
      const created = await env.SCENES.put(`${dir}state.json`, JSON.stringify(state), { ...JSON_META, onlyIf: { etagDoesNotMatch: '*' } });
      if (!created) return summaryFromStore(env, dir, key);
      etag = created.etag;
      if (units.length === 0) return summary(key, state);
    }
  }

  // New chunks in order, then the chunks that failed before.
  const queue = [];
  for (let i = state.cursor; i < units.length; i++) queue.push(i);
  for (const index of Object.keys(state.failed).map(Number).sort((a, b) => a - b)) if (index < state.cursor) queue.push(index);

  const seen = new Set(state.seen);
  let batch = [];
  let batchSourceBytes = 0;
  let sinceCheckpoint = 0;
  let used = 0;
  let usedBytes = 0;

  const checkpoint = async () => {
    if (batch.length) {
      const id = crypto.randomUUID();
      await env.SCENES.put(`${dir}parts/${id}.json`, JSON.stringify({ v: STATE_VERSION, entries: batch }), JSON_META);
      state.parts.push(id);
    }
    state.seen = [...seen];
    state.complete = state.cursor === units.length && Object.keys(state.failed).length === 0;
    state.updated_at = new Date().toISOString();
    // Conditioned on the state this call read, so two calls racing on one package cannot interleave their progress: the
    // loser stops, and its part file (if any) is listed nowhere.
    const put = await env.SCENES.put(`${dir}state.json`, JSON.stringify(state), { ...JSON_META, onlyIf: { etagMatches: etag } });
    if (!put) return false;
    etag = put.etag;
    batch = [];
    batchSourceBytes = 0;
    sinceCheckpoint = 0;
    return true;
  };

  for (const index of queue) {
    const unit = units[index];
    if (used >= maxChunks) break;
    if (used > 0 && usedBytes + unit.size > maxBytes) break;
    if (used > 0 && Date.now() - started >= budgetMs) break;
    used += 1;
    usedBytes += unit.size;

    const read = await readChunkScripts(env, simId, unit);
    if (read.ok) {
      delete state.failed[index];
      for (const found of await scriptsOfRecords(read.records)) {
        for (const ctx of unit.contexts) {
          const path = (ctx.space ? `${ctx.space}/` : '') + found.path;
          const entry = { path, space: ctx.space, audience: ctx.audience, language: found.language, bytes: found.bytes, sha256: found.sha256 };
          if (!seen.has(found.sha256)) {
            seen.add(found.sha256);
            entry.source = found.source;
            batchSourceBytes += found.bytes;
            state.distinct_scripts += 1;
            state.distinct_bytes += found.bytes;
          }
          batch.push(entry);
          state.scripts_total += 1;
          state.script_bytes += found.bytes;
        }
      }
    } else {
      state.failed[index] = { chunk: unit.hash, reason: read.reason };
    }
    if (index === state.cursor) state.cursor += 1;
    sinceCheckpoint += 1;
    if (sinceCheckpoint >= checkpointChunks || batchSourceBytes >= PART_SOURCE_BYTES) {
      if (!(await checkpoint())) return summaryFromStore(env, dir, key);
    }
  }
  if (!(await checkpoint())) return summaryFromStore(env, dir, key);
  return summary(key, state);
}

/// Read the extracted scripts. Only valid once `extractPackageScripts` reported `complete`; throws otherwise (an error with
/// `code` `unsupported` or `not_complete`), so a caller can never mistake "not read yet" for "no scripts".
///
/// One entry per distinct source, in a stable order: path, then audience. Each is the first place (in that order) that
/// ships the source, plus `paths` (every distinct path that ships it, sorted) and `audiences`.
export async function readPackageScripts(env, sim) {
  const located = await locate(env, sim);
  if (located.result) {
    const err = new Error(located.result.supported ? 'The package scripts cannot be read' : 'Not an .echk listing');
    err.code = located.result.supported ? 'not_complete' : 'unsupported';
    throw err;
  }
  const { state } = await readState(env, located.dir);
  if (!state?.complete) {
    const err = new Error('The package scripts are not completely extracted yet');
    err.code = 'not_complete';
    throw err;
  }
  const bySha = new Map();
  const before = (a, b) => a.path < b.path || (a.path === b.path && a.audience < b.audience);
  for (const id of state.parts) {
    const obj = await env.SCENES.get(`${located.dir}parts/${id}.json`);
    if (!obj) throw new Error(`A part of the extracted scripts is missing (${id})`);
    const part = JSON.parse(await obj.text());
    for (const e of part.entries) {
      let kept = bySha.get(e.sha256);
      if (!kept) {
        kept = { path: e.path, space: e.space, audience: e.audience, language: e.language, bytes: e.bytes, sha256: e.sha256, source: undefined, paths: new Set(), audiences: new Set() };
        bySha.set(e.sha256, kept);
      } else if (before(e, kept)) {
        Object.assign(kept, { path: e.path, space: e.space, audience: e.audience, language: e.language });
      }
      if (typeof e.source === 'string') kept.source = e.source;
      kept.paths.add(e.path);
      kept.audiences.add(e.audience);
    }
  }
  const out = [];
  for (const kept of bySha.values()) {
    if (typeof kept.source !== 'string') throw new Error(`The extracted scripts lack the source of ${kept.sha256}`);
    out.push({ ...kept, paths: [...kept.paths].sort(byCodeUnits), audiences: [...kept.audiences].sort(byCodeUnits) });
  }
  return out.sort((a, b) => (before(a, b) ? -1 : before(b, a) ? 1 : 0));
}

// ─────────────────────────────────────────────────────────────────────────────
// Locating the package and its store
// ─────────────────────────────────────────────────────────────────────────────

const empty = (key = '') => ({
  supported: true, complete: false, key, chunks_total: 0, chunks_read: 0, scripts_total: 0, script_bytes: 0,
  distinct_scripts: 0, distinct_bytes: 0, errors: [],
});
const failure = (key, error) => ({ ...empty(key), errors: [error] });

function summary(key, state) {
  return {
    supported: true,
    complete: state.complete === true,
    key,
    chunks_total: state.chunks_total,
    chunks_read: state.cursor - Object.keys(state.failed).length,
    scripts_total: state.scripts_total,
    script_bytes: state.script_bytes,
    distinct_scripts: state.distinct_scripts,
    distinct_bytes: state.distinct_bytes,
    errors: Object.keys(state.failed).map(Number).sort((a, b) => a - b).map((i) => ({ chunk: state.failed[i].chunk, reason: state.failed[i].reason })),
  };
}

async function summaryFromStore(env, dir, key) {
  const { state } = await readState(env, dir);
  return state ? summary(key, state) : failure(key, { chunk: 'state', reason: 'another extraction of this package is writing its progress' });
}

const freshState = (key, total) => ({
  v: STATE_VERSION, key, chunks_total: total, cursor: 0, failed: {}, parts: [], seen: [],
  scripts_total: 0, script_bytes: 0, distinct_scripts: 0, distinct_bytes: 0, complete: total === 0,
});

/// The listing's package: its R2 store, or a `result` that ends the call (not an .echk listing, nothing committed, or a
/// manifest that cannot be read when the key has to come from its bytes).
async function locate(env, sim) {
  if (!sim || sim.format !== 'echk') return { result: { ...empty(), supported: false } };
  const simId = sim.id;
  if (typeof simId !== 'string' || !SIM_ID.test(simId)) return { result: failure('', { chunk: 'listing', reason: 'the listing has no usable id' }) };
  const world = sim.world || {};
  if (!world.manifest_key) return { result: failure('', { chunk: 'manifest:players', reason: 'the listing has no committed world' }) };
  const players = manifestSha(world.manifest_sha256, world.manifest_key);
  const source = world.source_manifest_key ? manifestSha(world.source_manifest_sha256, world.source_manifest_key) : null;
  if (players && (!world.source_manifest_key || source)) {
    const key = source ? `${players}_${source}` : players;
    return { simId, key, dir: storePrefix(simId, key) };
  }
  // The hashes are not on the record: the key comes from the manifests' own bytes.
  const loaded = await loadManifests(env, sim);
  if (loaded.error) return { result: failure('', loaded.error) };
  const key = loaded.manifests.map((m) => m.sha256).join('_');
  return { simId, key, dir: storePrefix(simId, key), manifests: loaded };
}

/// A manifest's SHA-256: the record's, or the one its key is named by (`manifests/{sha256}.json`).
function manifestSha(sha, key) {
  if (typeof sha === 'string' && HEX64.test(sha)) return sha;
  const m = typeof key === 'string' ? key.match(/\/manifests\/([0-9a-f]{64})\.json$/) : null;
  return m ? m[1] : null;
}

async function readState(env, dir) {
  const obj = await env.SCENES.get(`${dir}state.json`);
  if (!obj) return { state: null, etag: null };
  try {
    const state = JSON.parse(await obj.text());
    if (state?.v !== STATE_VERSION || !Number.isSafeInteger(state.cursor) || !Array.isArray(state.parts) || typeof state.failed !== 'object' || !Array.isArray(state.seen))
      return { state: null, etag: null };
    return { state, etag: obj.etag };
  } catch {
    return { state: null, etag: null };
  }
}

async function readUnits(env, dir, state) {
  const obj = await env.SCENES.get(`${dir}units.json`);
  if (!obj) return null;
  try {
    const { units } = JSON.parse(await obj.text());
    return Array.isArray(units) && units.length === state.chunks_total ? units : null;
  } catch {
    return null;
  }
}

/// Both manifests, read, hash-checked and validated: `{ manifests: [{ audience, sha256, manifest }] }` or `{ error }`.
async function loadManifests(env, sim) {
  const world = sim.world || {};
  const wanted = [{ audience: 'players', key: world.manifest_key, sha: world.manifest_sha256 }];
  if (world.source_manifest_key) wanted.push({ audience: 'source', key: world.source_manifest_key, sha: world.source_manifest_sha256 });
  const manifests = [];
  for (const w of wanted) {
    const chunk = `manifest:${w.audience}`;
    const obj = await env.SCENES.get(w.key);
    if (!obj) return { error: { chunk, reason: 'the manifest is missing from storage' } };
    if (obj.size > MAX_MANIFEST_BYTES) return { error: { chunk, reason: `the manifest is ${obj.size} bytes, over the ${MAX_MANIFEST_BYTES} limit` } };
    const bytes = new Uint8Array(await obj.arrayBuffer());
    const sha256 = await sha256Hex(bytes);
    const expected = manifestSha(w.sha, w.key);
    if (expected && expected !== sha256) return { error: { chunk, reason: `the manifest's bytes hash to ${sha256}, not ${expected}` } };
    let manifest;
    try {
      manifest = JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      return { error: { chunk, reason: 'the manifest is not JSON' } };
    }
    const bad = checkManifest(manifest);
    if (bad) return { error: { chunk, reason: bad } };
    manifests.push({ audience: w.audience, sha256, manifest });
  }
  return { manifests };
}

/// What this module relies on in a manifest; `checkWorldManifest` in src/world.mjs checked the rest at commit.
function checkManifest(m) {
  if (!m || typeof m !== 'object' || Array.isArray(m)) return 'the manifest is not an object';
  if (m.format !== 'echk') return `format ${JSON.stringify(m.format)}, expected "echk"`;
  if (!Array.isArray(m.spaces) || m.spaces.length === 0) return 'no Spaces';
  if (m.assets !== undefined && !Array.isArray(m.assets)) return 'assets must be a list';
  const names = new Set();
  let total = 0;
  const checkChunks = (list, where) => {
    if (!Array.isArray(list)) return `${where}: chunks must be a list`;
    for (const c of list) {
      if (!c || typeof c !== 'object' || typeof c.blake3 !== 'string' || !HEX64.test(c.blake3)) return `${where}: a chunk has no content hash`;
      if (!Number.isSafeInteger(c.size) || c.size <= 0 || c.size > MAX_CHUNK_BYTES) return `${where}: chunk ${c.blake3} size ${JSON.stringify(c.size)}`;
    }
    total += list.length;
    return null;
  };
  for (const s of m.spaces) {
    const name = s?.name;
    if (typeof name !== 'string' || !isSafeSegment(name)) return `unsafe Space name ${JSON.stringify(name)}`;
    if (names.has(name)) return `Space ${JSON.stringify(name)} listed twice`;
    names.add(name);
    const bad = checkChunks(s.chunks, `Space ${JSON.stringify(name)}`);
    if (bad) return bad;
  }
  const bad = checkChunks(m.assets || [], 'assets');
  if (bad) return bad;
  if (total > MAX_MANIFEST_CHUNKS) return `${total} chunks exceeds ${MAX_MANIFEST_CHUNKS}`;
  return null;
}

function isSafeSegment(s) {
  return s.length > 0 && s.length <= 255 && s !== '.' && s !== '..' && !s.startsWith('.') && !/[\\/:\u0000-\u001f\u007f]/.test(s);
}

/// The chunks to read, each once, in a fixed order: the players' package (its Spaces in order, then its shared assets),
/// then the source package. A chunk both name, or two Spaces name, is read once and credited to each.
function buildUnits(manifests) {
  const byHash = new Map();
  const units = [];
  const add = (c, audience, space) => {
    let unit = byHash.get(c.blake3);
    if (!unit) {
      unit = { hash: c.blake3, size: c.size, contexts: [] };
      byHash.set(c.blake3, unit);
      units.push(unit);
    } else if (unit.size !== c.size) {
      // Two sizes for one chunk: whichever R2 holds disagrees with one of them, and the read reports it.
      unit.sizes = [...new Set([...(unit.sizes || [unit.size]), c.size])];
    }
    if (!unit.contexts.some((x) => x.audience === audience && x.space === space)) unit.contexts.push({ audience, space });
  };
  for (const { audience, manifest } of manifests) {
    for (const space of manifest.spaces) for (const c of space.chunks) add(c, audience, space.name);
    for (const c of manifest.assets || []) add(c, audience, '');
  }
  return units;
}

// ─────────────────────────────────────────────────────────────────────────────
// Reading one chunk
// ─────────────────────────────────────────────────────────────────────────────

/// The script-bearing records of one chunk: `{ ok: true, records: [{ path, data, toml }] }` or `{ ok: false, reason }`.
async function readChunkScripts(env, simId, unit) {
  let obj;
  try {
    obj = await env.SCENES.get(chunkKeyOf(simId, unit.hash));
  } catch (e) {
    return { ok: false, reason: `the chunk could not be fetched: ${e?.message || e}` };
  }
  if (!obj) return { ok: false, reason: 'the chunk is missing from storage' };
  const sizes = unit.sizes || [unit.size];
  const reader = obj.body.getReader();
  try {
    if (sizes.some((s) => s !== obj.size)) throw new ChunkError(`the stored chunk is ${obj.size} bytes, the manifest says ${sizes.join(' or ')}`);
    const decoder = new ChunkDecoder(obj.size);
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      decoder.push(value instanceof Uint8Array ? value : new Uint8Array(value));
      if (decoder.finished) break;
    }
    decoder.end();
    return { ok: true, records: decoder.records.records };
  } catch (e) {
    return { ok: false, reason: e instanceof ChunkError ? e.message : `bad compressed .echk: ${e?.message || e}` };
  } finally {
    reader.cancel().catch(() => {});
  }
}

const readU32 = (b, at) => (b[at] | (b[at + 1] << 8) | (b[at + 2] << 16) | (b[at + 3] << 24)) >>> 0;

/// A whole chunk, pushed as it streams: the container header, then the record stream (raw in version 1, through one zstd
/// frame in version 2).
class ChunkDecoder {
  constructor(size) {
    this.size = size;
    this.head = new Uint8Array(24);
    this.headFill = 0;
    this.version = 0;
    this.records = null;
    this.cutter = null;
    this.zstd = null;
    this.produced = 0;
    this.rawLen = 0;
  }

  /// A version 1 chunk is finished once its last record is read; a version 2 one only at the end of its bytes, since its
  /// inflated length has to be checked.
  get finished() {
    return this.version === 1 && this.records.done;
  }

  push(bytes) {
    let i = 0;
    while (!this.version) {
      if (i >= bytes.length) return;
      const want = this.headFill >= 12 && readU32(this.head, 4) === 2 ? 24 : 12;
      const take = Math.min(want - this.headFill, bytes.length - i);
      this.head.set(bytes.subarray(i, i + take), this.headFill);
      this.headFill += take;
      i += take;
      if (this.headFill >= 4 && !(this.head[0] === 0x45 && this.head[1] === 0x43 && this.head[2] === 0x48 && this.head[3] === 0x4b))
        throw new ChunkError('not an .echk chunk (bad magic)');
      if (this.headFill < 12) return;
      const version = readU32(this.head, 4);
      if (version !== 1 && version !== 2) throw new ChunkError(`unsupported .echk version ${version}`);
      if (version === 2 && this.headFill < 24) continue;
      this.start(version);
    }
    if (i < bytes.length) this.feed(bytes.subarray(i));
  }

  start(version) {
    const count = readU32(this.head, 8);
    if (version === 1) {
      this.version = 1;
      this.records = new RecordStream(count, this.size - 12);
      return;
    }
    const codec = this.head[12];
    const flags = this.head[13];
    const reserved = this.head[14] | (this.head[15] << 8);
    if (codec !== 1) throw new ChunkError(`bad compressed .echk: unknown codec ${codec}`);
    if (flags !== 0 || reserved !== 0) throw new ChunkError(`bad compressed .echk: unknown flags ${flags}/${reserved}`);
    const lo = readU32(this.head, 16);
    const hi = readU32(this.head, 20);
    const rawLen = hi * 2 ** 32 + lo;
    if (hi !== 0 || rawLen > MAX_RAW_BYTES) throw new ChunkError(`bad compressed .echk: declares ${rawLen} bytes, over the ${MAX_RAW_BYTES} limit`);
    this.version = 2;
    this.rawLen = rawLen;
    this.records = new RecordStream(count, rawLen);
    this.cutter = new FrameCutter(rawLen);
    this.zstd = new Decompress((data) => {
      if (!data.length) return;
      this.produced += data.length;
      if (this.produced > this.rawLen) throw new ChunkError(`bad compressed .echk: inflates past the ${this.rawLen} bytes it declares`);
      this.records.push(data);
    });
  }

  feed(bytes) {
    if (this.version === 1) {
      this.records.push(bytes);
      return;
    }
    if (this.cutter.done) return;
    for (const piece of this.cutter.cut(bytes)) this.zstd.push(piece, false);
    if (this.cutter.done) this.zstd.push(new Uint8Array(0), true);
  }

  end() {
    if (!this.version) {
      if (this.headFill < 12) throw new ChunkError('not an .echk chunk (bad magic)');
      throw new ChunkError('truncated .echk (version 2 header)');
    }
    if (this.version === 2) {
      if (!this.cutter.done) throw new ChunkError('bad compressed .echk: the zstd frame is truncated');
      if (this.produced !== this.rawLen) throw new ChunkError(`bad compressed .echk: inflates to ${this.produced} bytes, declares ${this.rawLen}`);
    }
    this.records.end();
  }
}

/// The bytes of a zstd stream that belong to its first frame. Walks the frame header and block headers without decoding,
/// and checks the header before the decoder sees it: no dictionary, a window within MAX_WINDOW_BYTES, a content size (when
/// given) equal to the chunk's raw_len, and blocks within zstd's 128 KiB maximum.
class FrameCutter {
  constructor(rawLen) {
    this.rawLen = rawLen;
    this.phase = 'header';
    this.buf = new Uint8Array(18);
    this.fill = 0;
    this.want = 5;
    this.window = 0;
    this.checksum = false;
    this.remaining = 0;
    this.last = false;
    this.done = false;
  }

  cut(bytes) {
    const out = [];
    let i = 0;
    while (i < bytes.length && !this.done) {
      if (this.phase === 'header' || this.phase === 'block') {
        const take = Math.min(this.want - this.fill, bytes.length - i);
        this.buf.set(bytes.subarray(i, i + take), this.fill);
        this.fill += take;
        i += take;
        if (this.fill < this.want) continue;
        if (this.phase === 'header') {
          if (this.want === 5) {
            this.want = this.headerLength();
            if (this.fill < this.want) continue;
          }
          this.checkHeader();
          out.push(this.buf.slice(0, this.want));
          this.startBlockHeader();
        } else {
          const bh = this.buf[0] | (this.buf[1] << 8) | (this.buf[2] << 16);
          const type = (bh >> 1) & 3;
          const size = bh >>> 3;
          if (type === 3) throw new ChunkError('bad compressed .echk: a reserved zstd block type');
          if (size > Math.min(this.window, MAX_BLOCK_BYTES)) throw new ChunkError('bad compressed .echk: a zstd block over the maximum block size');
          this.last = (bh & 1) === 1;
          out.push(this.buf.slice(0, 3));
          this.remaining = type === 1 ? 1 : size;
          this.phase = 'payload';
          this.afterPayload();
        }
      } else {
        const take = Math.min(this.remaining, bytes.length - i);
        out.push(bytes.subarray(i, i + take));
        this.remaining -= take;
        i += take;
        this.afterPayload();
      }
    }
    return out;
  }

  afterPayload() {
    if (this.remaining > 0) return;
    if (this.phase === 'payload' && this.last) {
      this.phase = 'checksum';
      this.remaining = this.checksum ? 4 : 0;
    }
    if (this.phase === 'checksum') {
      if (this.remaining === 0) this.done = true;
      return;
    }
    if (this.phase === 'payload') this.startBlockHeader();
  }

  startBlockHeader() {
    this.phase = 'block';
    this.fill = 0;
    this.want = 3;
  }

  headerLength() {
    const b = this.buf;
    if (!(b[0] === 0x28 && b[1] === 0xb5 && b[2] === 0x2f && b[3] === 0xfd)) throw new ChunkError('bad compressed .echk: the stream does not start with a zstd frame');
    const fhd = b[4];
    const fcsFlag = fhd >> 6;
    const single = (fhd >> 5) & 1;
    const dictBytes = [0, 1, 2, 4][fhd & 3];
    const fcsBytes = fcsFlag === 0 ? single : [0, 2, 4, 8][fcsFlag];
    return 5 + (single ? 0 : 1) + dictBytes + fcsBytes;
  }

  checkHeader() {
    const b = this.buf;
    const fhd = b[4];
    const fcsFlag = fhd >> 6;
    const single = (fhd >> 5) & 1;
    if (fhd & 8) throw new ChunkError('bad compressed .echk: a reserved zstd header bit is set');
    this.checksum = ((fhd >> 2) & 1) === 1;
    let at = 5;
    let window = 0;
    if (!single) {
      const wd = b[at++];
      const base = 2 ** (10 + (wd >> 3));
      window = base + (base / 8) * (wd & 7);
    }
    const dictBytes = [0, 1, 2, 4][fhd & 3];
    let dict = 0;
    for (let k = 0; k < dictBytes; k++) dict += b[at + k] * 2 ** (8 * k);
    at += dictBytes;
    if (dict !== 0) throw new ChunkError('bad compressed .echk: the zstd frame needs a dictionary');
    const fcsBytes = fcsFlag === 0 ? single : [0, 2, 4, 8][fcsFlag];
    if (fcsBytes) {
      let fcs = 0;
      for (let k = 0; k < fcsBytes; k++) fcs += b[at + k] * 2 ** (8 * k);
      if (fcsBytes === 2) fcs += 256;
      if (fcs !== this.rawLen) throw new ChunkError(`bad compressed .echk: the zstd frame declares ${fcs} bytes, the chunk ${this.rawLen}`);
      if (single) window = fcs;
    }
    if (window > MAX_WINDOW_BYTES) throw new ChunkError(`bad compressed .echk: a zstd window of ${window} bytes, over the ${MAX_WINDOW_BYTES} limit`);
    this.window = window;
  }
}

const isScriptPath = (lower) => SCRIPT_EXTENSIONS.some((ext) => lower.endsWith(ext));
const isInstanceToml = (lower) => lower === '_instance.toml' || lower.endsWith('/_instance.toml');
const utf8 = new TextDecoder('utf-8');

/// `parse_records`, pushed as it streams: `count` records in `total` bytes. Keeps the data of script files and
/// `_instance.toml` records only, and skips the rest without holding it.
class RecordStream {
  constructor(count, total) {
    this.count = count;
    this.total = total;
    this.offset = 0;
    this.read = 0;
    this.records = [];
    this.kept = 0;
    // Every record needs at least 8 bytes of length prefixes.
    if (count > Math.floor(total / 8)) throw new ChunkError('truncated .echk (record count)');
    this.field = new Uint8Array(4);
    this.fill = 0;
    this.phase = count === 0 ? 'done' : 'path_len';
    this.path = null;
    this.pathBytes = null;
    this.need = 0;
    this.data = null;
    this.dataFill = 0;
    this.toml = false;
  }

  get done() {
    return this.phase === 'done';
  }

  push(bytes) {
    let i = 0;
    while (i < bytes.length && this.phase !== 'done') {
      const avail = bytes.length - i;
      if (this.phase === 'path_len' || this.phase === 'data_len') {
        const take = Math.min(4 - this.fill, avail);
        this.field.set(bytes.subarray(i, i + take), this.fill);
        this.fill += take;
        i += take;
        this.offset += take;
        if (this.fill < 4) continue;
        this.fill = 0;
        const n = readU32(this.field, 0);
        if (this.phase === 'path_len') {
          if (n > MAX_PATH_LEN || this.offset + n > this.total) throw new ChunkError('truncated .echk (path)');
          this.pathBytes = new Uint8Array(n);
          this.need = n;
          this.phase = 'path';
          if (n === 0) this.endPath();
        } else {
          if (this.offset + n > this.total) throw new ChunkError('truncated .echk (data)');
          this.startData(n);
        }
      } else if (this.phase === 'path') {
        const take = Math.min(this.need, avail);
        this.pathBytes.set(bytes.subarray(i, i + take), this.pathBytes.length - this.need);
        this.need -= take;
        i += take;
        this.offset += take;
        if (this.need === 0) this.endPath();
      } else {
        const take = Math.min(this.need, avail);
        if (this.data) this.data.set(bytes.subarray(i, i + take), this.dataFill);
        this.dataFill += take;
        this.need -= take;
        i += take;
        this.offset += take;
        if (this.need === 0) this.endRecord();
      }
    }
  }

  endPath() {
    this.path = utf8.decode(this.pathBytes);
    this.pathBytes = null;
    this.phase = 'data_len';
  }

  startData(n) {
    const lower = this.path.toLowerCase();
    const script = isScriptPath(lower);
    this.toml = !script && isInstanceToml(lower);
    this.data = null;
    if (script || this.toml) {
      if (n > MAX_SCRIPT_BYTES) throw new ChunkError(`${this.path} is ${n} bytes, over the ${MAX_SCRIPT_BYTES} bytes a review reads in full`);
      this.kept += n;
      if (this.kept > MAX_CHUNK_SCRIPT_BYTES) throw new ChunkError(`the chunk holds over ${MAX_CHUNK_SCRIPT_BYTES} bytes of scripts`);
      this.data = new Uint8Array(n);
    }
    this.dataFill = 0;
    this.need = n;
    this.phase = 'data';
    if (n === 0) this.endRecord();
  }

  endRecord() {
    if (this.data) this.records.push({ path: this.path, data: this.data, toml: this.toml });
    this.data = null;
    this.read += 1;
    this.phase = this.read === this.count ? 'done' : 'path_len';
  }

  end() {
    if (this.phase !== 'done') throw new ChunkError(`truncated .echk (${this.phase})`);
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Records to scripts
// ─────────────────────────────────────────────────────────────────────────────

function languageOfPath(lower) {
  const dot = lower.lastIndexOf('.');
  return RUNE_EXTENSIONS.has(lower.slice(dot)) ? 'rune' : 'luau';
}

/// The scripts in a chunk's kept records: `{ path, language, bytes, sha256, source }`, path relative to its Space.
async function scriptsOfRecords(records) {
  const out = [];
  for (const r of records) {
    if (!r.toml) {
      out.push({ path: r.path, language: languageOfPath(r.path.toLowerCase()), bytes: r.data.length, sha256: await sha256Hex(r.data), source: utf8.decode(r.data) });
      continue;
    }
    const text = utf8.decode(r.data);
    const inline = inlineScriptOf(text);
    if (inline.kind !== 'code' && inline.kind !== 'unparsed') continue;
    // Code that could not be read out of its TOML is read as the whole file, so nothing in it goes unread.
    const source = inline.kind === 'code' ? inline.source : text;
    const bytes = new TextEncoder().encode(source);
    out.push({ path: `${r.path}#script`, language: inline.language || 'luau', bytes: bytes.length, sha256: await sha256Hex(bytes), source });
  }
  return out;
}

/// The code written inline in an `_instance.toml`: `{ kind: 'none' }` (no script table, or an empty source), `{ kind:
/// 'reference', value }` (a one-line source naming a script file), `{ kind: 'code', source, language }`, or `{ kind:
/// 'unparsed', language }` when a script table is present but its source cannot be read (the caller reads the whole file).
export function inlineScriptOf(text) {
  if (!/script/i.test(text)) return { kind: 'none' };
  let doc;
  try {
    doc = scanToml(text);
  } catch {
    return { kind: 'unparsed', language: /class_name\s*=\s*["']SoulScript["']/.test(text) ? 'rune' : 'luau' };
  }
  const cls = doc.values.get('metadata\u0000class_name');
  const ctx = doc.values.get('script\u0000run_context');
  const language = (cls?.kind === 'string' && cls.value === 'SoulScript') || (ctx?.kind === 'string' && ctx.value === 'Rune') ? 'rune' : 'luau';
  if (doc.opaque) return { kind: 'unparsed', language };
  if (!doc.scriptSeen) return { kind: 'none' };
  const src = doc.values.get('script\u0000source');
  if (!src) return { kind: 'none' };
  if (src.kind !== 'string') return { kind: 'unparsed', language };
  const value = src.value;
  if (value.trim() === '') return { kind: 'none' };
  if (FILE_REFERENCE.test(value.trim())) return { kind: 'reference', value };
  return { kind: 'code', source: value, language };
}

/// A `source` that only names a script file: one token, no whitespace and none of the characters a statement needs, ending
/// in a script extension. Anything else is read as code, so a one-line script that happens to end in `.luau` (`print(1) --
/// a.luau`, `return a.luau`) is never skipped as a reference.
const FILE_REFERENCE = /^[^\s"'`()[\]{};=,<>+*%^#~|&!?\\]+\.(?:luau|lua|rune|rn|soul)$/i;

const WANTED_KEYS = new Set(['script\u0000source', 'script\u0000run_context', 'metadata\u0000class_name']);

/// A targeted TOML reader: table headers, keys (bare, quoted, dotted) and the four string forms, enough to read
/// `[script] source`, `[script] run_context` and `[metadata] class_name` exactly. Other values are skipped (arrays and inline
/// tables by bracket depth, with strings inside them skipped whole). Throws on anything it does not understand; the caller
/// then reads the whole file.
function scanToml(text) {
  const n = text.length;
  let i = text.charCodeAt(0) === 0xfeff ? 1 : 0;
  let table = [];
  const values = new Map();
  let scriptSeen = false;
  let opaque = false;
  const fail = () => {
    throw new Error('TOML');
  };
  const skipInline = () => {
    while (i < n && (text[i] === ' ' || text[i] === '\t')) i++;
  };
  const endOfLine = () => {
    skipInline();
    if (text[i] === '#') while (i < n && text[i] !== '\n') i++;
    if (i < n && text[i] === '\r' && text[i + 1] === '\n') i++;
    if (i < n && text[i] !== '\n') fail();
    i++;
  };
  const readKey = () => {
    const parts = [];
    for (;;) {
      skipInline();
      if (text[i] === '"') parts.push(readBasic());
      else if (text[i] === "'") parts.push(readLiteral());
      else {
        const start = i;
        while (i < n && /[A-Za-z0-9_-]/.test(text[i])) i++;
        if (i === start) fail();
        parts.push(text.slice(start, i));
      }
      skipInline();
      if (text[i] !== '.') return parts;
      i++;
    }
  };
  const readBasic = () => {
    i++;
    let out = '';
    for (;;) {
      if (i >= n) fail();
      const c = text[i];
      if (c === '"') {
        i++;
        return out;
      }
      if (c === '\n' || c === '\r') fail();
      if (c === '\\') out += readEscape();
      else {
        out += c;
        i++;
      }
    }
  };
  const readEscape = () => {
    const e = text[i + 1];
    const simple = { b: '\b', t: '\t', n: '\n', f: '\f', r: '\r', e: '\u001b', '"': '"', '\\': '\\' }[e];
    if (simple !== undefined) {
      i += 2;
      return simple;
    }
    const width = e === 'x' ? 2 : e === 'u' ? 4 : e === 'U' ? 8 : 0;
    if (!width) fail();
    const hex = text.slice(i + 2, i + 2 + width);
    if (hex.length !== width || !/^[0-9A-Fa-f]+$/.test(hex)) fail();
    const cp = parseInt(hex, 16);
    if (cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) fail();
    i += 2 + width;
    return String.fromCodePoint(cp);
  };
  const readLiteral = () => {
    const end = text.indexOf("'", i + 1);
    if (end < 0) fail();
    const value = text.slice(i + 1, end);
    if (/[\r\n]/.test(value)) fail();
    i = end + 1;
    return value;
  };
  const skipNewlineAfterOpen = () => {
    if (text[i] === '\n') i++;
    else if (text[i] === '\r' && text[i + 1] === '\n') i += 2;
  };
  /// The closing run of a multi-line string at `i`: up to two quotes before the closing three belong to the content.
  const closeRun = (q) => {
    let run = 0;
    while (text[i + run] === q) run++;
    if (run < 3) return null;
    if (run > 5) fail();
    i += run;
    return q.repeat(run - 3);
  };
  const readMultiBasic = () => {
    i += 3;
    skipNewlineAfterOpen();
    let out = '';
    for (;;) {
      if (i >= n) fail();
      const c = text[i];
      if (c === '"') {
        const tail = closeRun('"');
        if (tail !== null) return out + tail;
        out += c;
        i++;
      } else if (c === '\\') {
        // A line-ending backslash trims the newline and all whitespace after it.
        let j = i + 1;
        while (text[j] === ' ' || text[j] === '\t') j++;
        if (text[j] === '\n' || (text[j] === '\r' && text[j + 1] === '\n')) {
          i = j;
          while (i < n && /[ \t\r\n]/.test(text[i])) i++;
        } else out += readEscape();
      } else {
        out += c;
        i++;
      }
    }
  };
  const readMultiLiteral = () => {
    i += 3;
    skipNewlineAfterOpen();
    let out = '';
    for (;;) {
      if (i >= n) fail();
      if (text[i] === "'") {
        const tail = closeRun("'");
        if (tail !== null) return out + tail;
      }
      out += text[i];
      i++;
    }
  };
  const skipStringAt = () => {
    if (text.startsWith('"""', i)) readMultiBasic();
    else if (text[i] === '"') readBasic();
    else if (text.startsWith("'''", i)) readMultiLiteral();
    else readLiteral();
  };
  const skipBracketed = () => {
    let depth = 0;
    for (;;) {
      if (i >= n) fail();
      const c = text[i];
      if (c === '"' || c === "'") skipStringAt();
      else if (c === '#') while (i < n && text[i] !== '\n') i++;
      else {
        if (c === '[' || c === '{') depth++;
        else if (c === ']' || c === '}') depth--;
        i++;
        if (depth === 0) return;
      }
    }
  };
  const readValue = () => {
    if (text.startsWith('"""', i)) return { kind: 'string', value: readMultiBasic() };
    if (text[i] === '"') return { kind: 'string', value: readBasic() };
    if (text.startsWith("'''", i)) return { kind: 'string', value: readMultiLiteral() };
    if (text[i] === "'") return { kind: 'string', value: readLiteral() };
    if (text[i] === '[') {
      skipBracketed();
      return { kind: 'array' };
    }
    if (text[i] === '{') {
      skipBracketed();
      return { kind: 'inline' };
    }
    const start = i;
    while (i < n && text[i] !== '\n' && text[i] !== '#' && text[i] !== '\r') i++;
    if (text.slice(start, i).trim() === '') fail();
    return { kind: 'other' };
  };

  while (i < n) {
    const c = text[i];
    if (c === ' ' || c === '\t' || c === '\r' || c === '\n') {
      i++;
      continue;
    }
    if (c === '#') {
      while (i < n && text[i] !== '\n') i++;
      continue;
    }
    if (c === '[') {
      const array = text[i + 1] === '[';
      i += array ? 2 : 1;
      const keys = readKey();
      if (text[i] !== ']') fail();
      i++;
      if (array) {
        if (text[i] !== ']') fail();
        i++;
      }
      if (keys[0] === 'script') {
        if (array) opaque = true;
        else scriptSeen = true;
      }
      table = array ? null : keys;
      endOfLine();
      continue;
    }
    const keys = readKey();
    if (text[i] !== '=') fail();
    i++;
    skipInline();
    const full = table === null ? null : [...table, ...keys];
    const value = readValue();
    if (full) {
      if (full[0] === 'script') {
        scriptSeen = true;
        if (full.length === 1) opaque = true;
      }
      const name = full.join('\u0000');
      if (WANTED_KEYS.has(name)) {
        if (values.has(name)) fail();
        values.set(name, value);
      }
    }
    endOfLine();
  }
  return { values, scriptSeen, opaque };
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function positive(value, fallback) {
  return Number.isSafeInteger(value) && value > 0 ? value : fallback;
}

function byCodeUnits(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
