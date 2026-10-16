// A listing's cover image, and the review captures served as previews (src/cover.mjs, docs/networking/LISTING_COVER.md).
//
// A cover is the creator's own art: portrait box art or 16:9. The author sets it (PUT), removes it (DELETE) and everyone reads it
// (GET) when the listing is listed and a review has bound that exact image. The type comes from the bytes and never from the label;
// the size, the short side and the ratio are checked from the header; metadata is dropped before storage; a hold or a frozen author
// refuses a change the way every other write is refused. The real Worker runs against in-memory storage, with the real approval
// binding and the real image helper (src/user_images.mjs).
//
// Source note: no backslash-u escape appears in this file on purpose (a tool that edits it can decode one into the character
// itself), and every character is plain ASCII.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { handleCoverRoute, coverFields, coverOf, coverSizeProblem, COVER_MAX_BYTES, COVER_MIN_SHORT_SIDE, COVER_RULES } from '../src/cover.mjs';
import { coverApproved, thumbnailApproved, captureR2Key, isListable, snapshotReviewed, identOfObject } from '../src/moderation.mjs';
import { routePolicy, PUBLIC, SESSION } from '../src/route_policy.mjs';
import { inspectImage, sha256Hex } from '../src/user_images.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const GHOST = 'cccccccc-3333-4333-8333-cccccccccccc';
const tokens = { author: mintToken(AUTHOR), stranger: mintToken(STRANGER), admin: mintToken(ADMIN) };

// -- images, built byte by byte: only a header and just enough after it for the real helper to walk ----------------------------

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();
const crc32 = (bytes) => {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const ascii = (text) => [...text].map((ch) => ch.charCodeAt(0));
const u16be = (n) => [(n >>> 8) & 255, n & 255];
const u32be = (n) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const u16le = (n) => [n & 255, (n >>> 8) & 255];
const u32le = (n) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];
const u24le = (n) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255];

const concat = (parts) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
};
function pngChunk(type, data) {
  const body = concat([Uint8Array.from(ascii(type)), Uint8Array.from(data)]);
  return concat([Uint8Array.from(u32be(data.length)), body, Uint8Array.from(u32be(crc32(body)))]);
}
/// A PNG of w x h. `text` adds a tEXt chunk (metadata); `end: false` stops the file before IEND; `weigh` adds that many bytes of
/// picture data INSIDE the image data chunk (the storage helper keeps only what draws the picture, so weight has to be picture).
function png(w, h, { text, end = true, weigh = 0, chunks = [] } = {}) {
  const ihdr = [...u32be(w), ...u32be(h), 8, 2, 0, 0, 0];
  const idat = concat([Uint8Array.from([0x78, 0x9c, 0x63, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]), new Uint8Array(weigh).fill(0x5a)]);
  return concat([
    Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', ihdr), ...(text ? [pngChunk('tEXt', ascii(text))] : []),
    ...chunks, pngChunk('IDAT', idat), ...(end ? [pngChunk('IEND', [])] : []),
  ]);
}
/// A JPEG of w x h. `exif` adds an APP1 segment (metadata); `weigh` adds that many bytes of entropy-coded data INSIDE the scan,
/// before the end-of-image marker.
function jpeg(w, h, { exif, weigh = 0 } = {}) {
  const app0 = [0xff, 0xe0, 0x00, 0x10, ...ascii('JFIF'), 0, 1, 1, 0, 0, 1, 0, 1, 0, 0];
  const app1 = exif ? [0xff, 0xe1, ...u16be(exif.length + 2), ...ascii(exif)] : [];
  const sof = [0xff, 0xc0, 0x00, 0x11, 8, ...u16be(h), ...u16be(w), 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];
  const sos = [0xff, 0xda, 0x00, 0x0c, 3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0];
  return concat([Uint8Array.from([0xff, 0xd8, ...app0, ...app1, ...sof, ...sos, 0x12, 0x34]), new Uint8Array(weigh).fill(0x12), Uint8Array.from([0xff, 0xd9])]);
}
/// A picture of exactly `total` bytes in which every byte draws the picture, so what is stored is the same size.
const weighing = {
  jpeg: (total, w = 800, h = 800) => jpeg(w, h, { weigh: total - jpeg(w, h).length }),
  png: (total, w = 600, h = 800) => png(w, h, { weigh: total - png(w, h).length }),
};
const riff = (chunks) => {
  const body = [...ascii('WEBP'), ...chunks];
  return Uint8Array.from([...ascii('RIFF'), ...u32le(body.length), ...body]);
};
const webpChunk = (name, data) => [...ascii(name), ...u32le(data.length), ...data, ...(data.length & 1 ? [0] : [])];
/// A WebP of w x h: 'lossy' (VP8), 'lossless' (VP8L) or 'extended' (VP8X with a VP8 image after it).
function webp(kind, w, h, [frameWidth, frameHeight] = [w, h]) {
  const lossy = [0x50, 0x01, 0x00, 0x9d, 0x01, 0x2a, ...u16le(frameWidth), ...u16le(frameHeight), 0, 0, 0, 0, 0, 0, 0, 0];
  if (kind === 'lossy') return riff(webpChunk('VP8 ', lossy));
  if (kind === 'lossless') return riff(webpChunk('VP8L', [0x2f, ...u32le(((w - 1) | ((h - 1) << 14)) >>> 0), 0, 0, 0, 0]));
  return riff([...webpChunk('VP8X', [0, 0, 0, 0, ...u24le(w - 1), ...u24le(h - 1)]), ...webpChunk('VP8 ', lossy)]);
}
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const same = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
const contains = (bytes, text) => Buffer.from(bytes).includes(Buffer.from(text));

// -- the world around them --------------------------------------------------------------------------------------------------------

async function environment({ moderation = { status: 'approved', rating: 'all_ages' }, is_public = true, extra = {} } = {}) {
  const env = fakeEnv();
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada'], [ADMIN, 'root']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'Box Head', description: '', genre: 'all', max_players: 4, author_id: AUTHOR, author_name: 'host', is_public, moderation,
    play_count: 0, favorite_count: 0, version: 1, r2_key: `universes/${SIM}/universe.pak`,
    published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z', ...extra,
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  await env.SCENES.put(`universes/${SIM}/universe.pak`, 'the world');
  return env;
}

async function call(env, method, path, { who, body, headers, origin = 'https://api.eustress.dev' } = {}) {
  const h = { ...(headers || {}) };
  if (who) h.Authorization = `Bearer ${tokens[who]}`;
  const res = await worker.fetch(new Request(`${origin}${path}`, { method, headers: h, body }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const text = new TextDecoder().decode(bytes);
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* an image, or not JSON */ }
  return { status: res.status, headers: res.headers, bytes, text, body: parsed };
}

const cover = `/api/simulations/${SIM}/cover`;
const put = (env, bytes, who = 'author', headers = { 'Content-Type': 'image/png' }) => call(env, 'PUT', cover, { who, body: bytes, headers });
const del = (env, who = 'author') => call(env, 'DELETE', cover, { who });
const record = async (env) => JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
const coverKeys = (env) => [...env.SCENES.objects.keys()].filter((k) => k.startsWith(`covers/${SIM}/`)).sort();
const later = async (ms, fn) => { const real = Date.now; Date.now = () => real() + ms; try { return await fn(); } finally { Date.now = real; } };

/// What a review leaves behind: the approval binding for the objects in storage right now, as moderation.mjs identifies them
/// (a revoked one when asked).
async function bind(env, { evidence_epoch = 0, revoked = false } = {}) {
  if (revoked) {
    await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, revoked: true, at: '2026-01-02T00:00:00.000Z' }));
    return;
  }
  const now = await snapshotReviewed(env, await record(env), evidence_epoch);
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, ...now, evidence_epoch, by: 'test', at: '2026-01-02T00:00:00.000Z' }));
}

// -- the rules --------------------------------------------------------------------------------------------------------------------

test('the rules: at most 2 MB, 320 px on the short side, ratios from 1:2 to 2:1', () => {
  assert.equal(COVER_MAX_BYTES, 2 * 1024 * 1024);
  assert.equal(COVER_MIN_SHORT_SIDE, 320);
  assert.match(COVER_RULES, /PNG, JPEG or WebP/);
  assert.match(COVER_RULES, /2 MB/);
  assert.match(COVER_RULES, /320 px/);
  assert.match(COVER_RULES, /1:2 and 2:1/);
  for (const [w, h] of [[320, 320], [640, 320], [320, 640], [1920, 1080], [600, 800], [1000, 500], [500, 1000], [2048, 1024]]) assert.equal(coverSizeProblem(w, h), null, `${w}x${h}`);
  for (const [w, h] of [[319, 320], [320, 319], [319, 638], [638, 319], [641, 320], [320, 641], [3000, 320], [320, 3000], [100, 100], [1, 1]]) {
    const problem = coverSizeProblem(w, h);
    assert.ok(problem, `${w}x${h} is refused`);
    assert.match(problem, /PNG, JPEG or WebP/, 'and the message states the rules');
  }
  assert.match(coverSizeProblem(300, 300), /short side/);
  assert.match(coverSizeProblem(1000, 320), /ratio/);
});

// -- setting a cover --------------------------------------------------------------------------------------------------------------

const KINDS = [
  ['PNG', () => png(600, 800), 'png', 'image/png', 600, 800],
  ['JPEG', () => jpeg(1280, 720), 'jpg', 'image/jpeg', 1280, 720],
  ['lossy WebP', () => webp('lossy', 800, 450), 'webp', 'image/webp', 800, 450],
  ['lossless WebP', () => webp('lossless', 640, 640), 'webp', 'image/webp', 640, 640],
  ['extended WebP', () => webp('extended', 450, 800), 'webp', 'image/webp', 450, 800],
];

for (const [name, make, ext, mime, width, height] of KINDS) {
  test(`a ${name} cover is stored as covers/{id}/cover.${ext}, sized from its header, hashed for R2, and served back`, async () => {
    const env = await environment();
    const bytes = make();
    const r = await put(env, bytes, 'author', { 'Content-Type': mime });
    assert.equal(r.status, 200, r.text);
    assert.equal(r.body.ok, true);
    assert.equal(r.body.review, 'submit', 'nothing starts a review by itself: the client posts /submit');
    assert.deepEqual(r.body.cover, { width, height });
    assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.${ext}`]);

    const stored = env.SCENES.objects.get(`covers/${SIM}/cover.${ext}`);
    assert.ok(same(stored.body, bytes), 'an image with no metadata is stored as it came');
    assert.equal(stored.meta.httpMetadata.contentType, mime);
    assert.deepEqual(stored.meta.customMetadata, { simId: SIM, authorId: AUTHOR });
    assert.equal(stored.meta.sha256, sha256(stored.body), 'R2 is given the digest of the bytes it stores, and verifies it');

    const sim = await record(env);
    assert.deepEqual({ ...sim.cover, updated_at: undefined }, { ext, width, height, bytes: bytes.length, updated_at: undefined });
    assert.match(sim.cover.updated_at, /^\d{4}-\d\d-\d\dT[\d:.]+Z$/);
    assert.equal(r.body.cover_url, `https://api.eustress.dev/api/simulations/${SIM}/cover?v=${Date.parse(sim.cover.updated_at)}`);

    const got = await call(env, 'GET', cover, { who: 'author' });
    assert.equal(got.status, 200);
    assert.equal(got.headers.get('Content-Type'), mime);
    assert.ok(same(got.bytes, bytes));
  });
}

test('setting a cover leaves the review as it was: the listing stays listed, and the marker is not touched', async () => {
  const env = await environment();
  const before = await record(env);
  assert.equal((await put(env, png(600, 800))).status, 200);
  const after = await record(env);
  assert.deepEqual(after.moderation, before.moderation);
  assert.equal(after.is_public, true);
  const listed = (await call(env, 'GET', '/api/simulations')).body.simulations.map((s) => s.id);
  assert.deepEqual(listed, [SIM]);
});

test('the type is what the bytes are: the label is never read', async () => {
  const lies = await environment();
  assert.equal((await put(lies, png(600, 800), 'author', { 'Content-Type': 'text/html' })).status, 200, 'a PNG labelled as a page is a PNG');
  assert.deepEqual(coverKeys(lies), [`covers/${SIM}/cover.png`]);
  assert.equal((await call(lies, 'GET', cover, { who: 'author' })).headers.get('Content-Type'), 'image/png');

  const swapped = await environment();
  assert.equal((await put(swapped, jpeg(800, 800), 'author', { 'Content-Type': 'image/png' })).status, 200);
  assert.deepEqual(coverKeys(swapped), [`covers/${SIM}/cover.jpg`], 'a JPEG labelled PNG is stored and served as a JPEG');
  assert.equal((await call(swapped, 'GET', cover, { who: 'author' })).headers.get('Content-Type'), 'image/jpeg');
  assert.equal((await record(swapped)).cover.ext, 'jpg');

  const noLabel = await environment();
  assert.equal((await put(noLabel, webp('lossy', 700, 700), 'author', {})).status, 200, 'no label at all');
  assert.deepEqual(coverKeys(noLabel), [`covers/${SIM}/cover.webp`]);
});

test('what is not a PNG, JPEG or WebP is refused with the rules, whatever it is labelled, and nothing is stored', async () => {
  const text = (s) => new TextEncoder().encode(s);
  const refused = [
    ['an HTML page', text('<!doctype html><html><script>alert(1)</script></html>')],
    ['an SVG', text('<svg xmlns="http://www.w3.org/2000/svg" width="600" height="800"><script>alert(1)</script></svg>')],
    ['a GIF', Uint8Array.from([...ascii('GIF89a'), 0x58, 0x02, 0x20, 0x03, 0, 0, 0, 0, 0, 0, 0, 0])],
    ['a BMP', Uint8Array.from([0x42, 0x4d, ...new Array(60).fill(0)])],
    ['a PDF', text('%PDF-1.7 a document that is not a picture')],
    ['a PNG signature and nothing else', Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])],
    ['a PNG whose first chunk is not IHDR', Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...pngChunk('IDAT', [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13])])],
    ['a JPEG that has no frame header', Uint8Array.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, ...ascii('JFIF'), 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9])],
    ['a RIFF file that is not WebP', Uint8Array.from([...ascii('RIFF'), 40, 0, 0, 0, ...ascii('WAVEfmt '), ...new Array(40).fill(0)])],
    ['random bytes', Uint8Array.from(Array.from({ length: 4096 }, (_, i) => (i * 131 + 17) & 255))],
    ['an image too large to serve (9000 x 9000)', png(9000, 9000)],
    ['a PNG that stops before its end', png(600, 800, { end: false })],
    ['a PNG with a critical chunk this platform does not know', png(600, 800, { chunks: [pngChunk('ZZZZ', [1, 2, 3])] })],
    ['a PNG with a second header', png(600, 800, { chunks: [pngChunk('IHDR', [...u32be(600), ...u32be(800), 8, 2, 0, 0, 0])] })],
    ['a JPEG that stops before its end marker', jpeg(800, 800).slice(0, -2)],
    ['a JPEG with a second picture started inside it', concat([jpeg(800, 800).slice(0, -2), Uint8Array.from([0xff, 0xd8]), jpeg(800, 800)])],
    ['a WebP whose canvas says 600 x 800 and whose frame says 9000 x 9000', webp('extended', 600, 800, [9000, 9000])],
  ];
  for (const [what, bytes] of refused) {
    const env = await environment();
    const r = await put(env, bytes, 'author', { 'Content-Type': 'image/png' });
    assert.equal(r.status, 415, `${what}: ${r.text.slice(0, 160)}`);
    assert.equal(r.body.code, 'cover_bad_type', what);
    assert.match(r.body.error, /PNG, JPEG or WebP/, `${what}: the message states the rules`);
    assert.match(r.body.error, /2 MB/, what);
    assert.deepEqual(coverKeys(env), [], `${what}: nothing stored`);
    assert.equal((await record(env)).cover, undefined, `${what}: nothing recorded`);
  }
  const empty = await environment();
  const r = await put(empty, new Uint8Array(0));
  assert.equal(r.status, 400);
  assert.equal(r.body.code, 'cover_empty');
  assert.match(r.body.error, /1:2 and 2:1/);
});

test('2 MB is allowed and one byte more is refused, for a picture that is 2 MB of picture', async () => {
  for (const [name, make, mime, ext] of [['JPEG', weighing.jpeg, 'image/jpeg', 'jpg'], ['PNG', weighing.png, 'image/png', 'png']]) {
    const exactly = await environment();
    const full = make(COVER_MAX_BYTES);
    assert.equal(full.length, COVER_MAX_BYTES, `the ${name} fixture weighs exactly 2 MB`);
    const ok = await put(exactly, full, 'author', { 'Content-Type': mime });
    assert.equal(ok.status, 200, `${name}: ${ok.text.slice(0, 200)}`);
    assert.equal(exactly.SCENES.objects.get(`covers/${SIM}/cover.${ext}`).body.length, COVER_MAX_BYTES, `${name}: all of it is stored, none of it metadata`);
    assert.equal((await record(exactly)).cover.bytes, COVER_MAX_BYTES, name);

    const over = await environment();
    const r = await put(over, make(COVER_MAX_BYTES + 1), 'author', { 'Content-Type': mime });
    assert.equal(r.status, 413, name);
    assert.equal(r.body.code, 'cover_too_large');
    assert.match(r.body.error, /2 MB/);
    assert.deepEqual(coverKeys(over), []);
    assert.equal((await record(over)).cover, undefined);
  }
});

test('a declared size over the limit is refused before the body is read, and a declared size at the limit is not', async () => {
  const env = await environment();
  const r = await put(env, png(600, 800), 'author', { 'Content-Type': 'image/png', 'Content-Length': String(COVER_MAX_BYTES + 1) });
  assert.equal(r.status, 413);
  assert.equal(r.body.code, 'cover_too_large');
  assert.deepEqual(coverKeys(env), []);
  const atTheLimit = await environment();
  assert.equal((await put(atTheLimit, png(600, 800), 'author', { 'Content-Type': 'image/png', 'Content-Length': String(COVER_MAX_BYTES) })).status, 200);
});

test('a file the storage helper cannot walk is refused and never stored as it came, even where the inspection let it through', async () => {
  const env = await environment();
  const url = new URL(`https://api.eustress.dev${cover}`);
  const answer = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
  const images = { inspectImage, sha256Hex, stripMetadata: () => { throw new Error('the structure cannot be walked'); } };
  const res = await handleCoverRoute(new Request(url, { method: 'PUT', body: png(600, 800) }), url, env, {}, { verifyAuth: async () => AUTHOR, requireAdmin: async () => null, json: answer, images });
  assert.equal(res.status, 415);
  assert.equal((await res.json()).code, 'cover_bad_type');
  assert.deepEqual(coverKeys(env), []);
  assert.equal((await record(env)).cover, undefined);
});

/// A request body that arrives in chunks with no declared length. `state` counts what was pulled and whether the reader gave up.
function chunkedBody(total, { chunk = 65536, endless = false } = {}) {
  const state = { pulled: 0, cancelled: false };
  const source = typeof total === 'number' ? null : total;
  let at = 0;
  const stream = new ReadableStream({
    pull(controller) {
      state.pulled += 1;
      if (endless) { controller.enqueue(new Uint8Array(chunk).fill(0x12)); return; }
      if (at >= source.length) { controller.close(); return; }
      controller.enqueue(source.subarray(at, at + chunk));
      at += chunk;
    },
    cancel() { state.cancelled = true; },
  });
  return { stream, state };
}
const putStream = async (env, stream, headers = { 'Content-Type': 'image/jpeg' }) => {
  const res = await worker.fetch(new Request(`https://api.eustress.dev${cover}`, { method: 'PUT', headers: { ...headers, Authorization: `Bearer ${tokens.author}` }, body: stream, duplex: 'half' }), env, ctx);
  const text = await res.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, text, body };
};

test('the size cap holds while the bytes arrive: a body that never ends is cut off at 2 MB and refused, and the rest is never read', { timeout: 15_000 }, async () => {
  const env = await environment();
  const { stream, state } = chunkedBody(0, { endless: true });
  const r = await putStream(env, stream);
  assert.equal(r.status, 413, r.text.slice(0, 160));
  assert.equal(r.body.code, 'cover_too_large');
  assert.match(r.body.error, /2 MB/);
  assert.ok(state.cancelled, 'the reader gave up on the body');
  assert.ok(state.pulled <= COVER_MAX_BYTES / 65536 + 4, `only what the cap allows was pulled (${state.pulled} chunks)`);
  assert.deepEqual(coverKeys(env), []);
  assert.equal((await record(env)).cover, undefined);
});

test('a body with no declared length is held to the same cap: 2 MB of picture in chunks is stored, one byte more is refused', async () => {
  for (const [name, make, mime, ext] of [['JPEG', weighing.jpeg, 'image/jpeg', 'jpg'], ['PNG', weighing.png, 'image/png', 'png']]) {
    const exactly = await environment();
    const ok = await putStream(exactly, chunkedBody(make(COVER_MAX_BYTES), { chunk: 100_003 }).stream, { 'Content-Type': mime });
    assert.equal(ok.status, 200, `${name}: ${ok.text.slice(0, 200)}`);
    assert.equal(exactly.SCENES.objects.get(`covers/${SIM}/cover.${ext}`).body.length, COVER_MAX_BYTES, name);

    const over = await environment();
    const r = await putStream(over, chunkedBody(make(COVER_MAX_BYTES + 1), { chunk: 100_003 }).stream, { 'Content-Type': mime });
    assert.equal(r.status, 413, name);
    assert.equal(r.body.code, 'cover_too_large');
    assert.deepEqual(coverKeys(over), []);
  }
});

test('a declared length that cannot be used does not matter: the body is read to the cap all the same', async () => {
  for (const declared of ['abc', '-5', '', '1e9x']) {
    const env = await environment();
    const r = await put(env, png(600, 800), 'author', { 'Content-Type': 'image/png', 'Content-Length': declared });
    assert.equal(r.status, 200, `Content-Length ${JSON.stringify(declared)}: ${r.text.slice(0, 120)}`);
  }
  const empty = await environment();
  assert.equal((await putStream(empty, chunkedBody(new Uint8Array(0)).stream)).status, 400, 'an empty chunked body');
});

test('the short side is at least 320 px and the ratio is from 1:2 to 2:1, read from the header', async () => {
  const accepted = [[320, 320], [640, 320], [320, 640], [1920, 1080], [600, 800], [2048, 1024]];
  for (const [w, h] of accepted) {
    const env = await environment();
    const r = await put(env, png(w, h));
    assert.equal(r.status, 200, `${w}x${h}: ${r.text.slice(0, 160)}`);
    assert.deepEqual((await record(env)).cover.width, w);
  }
  const small = [[319, 600], [600, 319], [319, 319], [100, 100], [1, 4000]];
  for (const [w, h] of small) {
    const env = await environment();
    const r = await put(env, png(w, h));
    assert.equal(r.status, 422, `${w}x${h}`);
    assert.equal(r.body.code, 'cover_too_small', `${w}x${h}`);
    assert.match(r.body.error, /320 px/);
    assert.deepEqual(coverKeys(env), []);
  }
  const wrongShape = [[641, 320], [320, 641], [4000, 320], [320, 4000], [1000, 400], [400, 1000]];
  for (const [w, h] of wrongShape) {
    const env = await environment();
    const r = await put(env, jpeg(w, h), 'author', { 'Content-Type': 'image/jpeg' });
    assert.equal(r.status, 422, `${w}x${h}`);
    assert.equal(r.body.code, 'cover_bad_ratio', `${w}x${h}`);
    assert.match(r.body.error, /1:2 and 2:1/);
    assert.deepEqual(coverKeys(env), []);
  }
});

test('a refused upload leaves the cover that was there untouched', async () => {
  const env = await environment();
  const good = png(600, 800);
  assert.equal((await put(env, good)).status, 200);
  const before = JSON.stringify(await record(env));
  for (const bad of [png(100, 100), png(4000, 320), new TextEncoder().encode('<html></html>'), weighing.png(COVER_MAX_BYTES + 1)]) {
    assert.ok([413, 415, 422].includes((await put(env, bad)).status));
  }
  assert.equal(JSON.stringify(await record(env)), before);
  assert.ok(same(env.SCENES.objects.get(`covers/${SIM}/cover.png`).body, good));
});

test('bytes after the end of the picture, and anything appended there, are not stored', async () => {
  const appended = new TextEncoder().encode('a second picture or notes appended after the end of the first');
  for (const [name, base, mime, ext] of [['PNG', png(600, 800), 'image/png', 'png'], ['JPEG', jpeg(800, 800), 'image/jpeg', 'jpg']]) {
    const env = await environment();
    const r = await put(env, concat([base, appended]), 'author', { 'Content-Type': mime });
    assert.equal(r.status, 200, `${name}: ${r.text.slice(0, 160)}`);
    const stored = env.SCENES.objects.get(`covers/${SIM}/cover.${ext}`).body;
    assert.ok(same(stored, base), `${name}: the picture alone is stored`);
    assert.ok(!contains(stored, 'appended after'), name);
    assert.equal((await record(env)).cover.bytes, base.length, `${name}: the record holds the stored size, not the uploaded one`);
  }
});

test('metadata is dropped before storage: EXIF and text chunks never reach R2, and what is stored is what the digest covers', async () => {
  const withExif = await environment();
  const marked = jpeg(1280, 720, { exif: 'Exif--GPS 32.2226 N 110.9747 W--' });
  assert.ok(contains(marked, 'GPS 32.2226'));
  assert.equal((await put(withExif, marked, 'author', { 'Content-Type': 'image/jpeg' })).status, 200);
  const storedJpeg = withExif.SCENES.objects.get(`covers/${SIM}/cover.jpg`);
  assert.ok(!contains(storedJpeg.body, 'GPS 32.2226'), 'the location is gone');
  assert.equal(storedJpeg.meta.sha256, sha256(storedJpeg.body));
  assert.equal((await record(withExif)).cover.bytes, storedJpeg.body.length, 'the record holds the size of what is stored');

  const withText = await environment();
  assert.equal((await put(withText, png(600, 800, { text: 'Author' + String.fromCharCode(0) + 'Jane Doe at home' }))).status, 200);
  assert.ok(!contains(withText.SCENES.objects.get(`covers/${SIM}/cover.png`).body, 'Jane Doe'), 'the text chunk is gone');
});

test('replacing a cover with another type leaves one file, the new one', async () => {
  const env = await environment();
  assert.equal((await put(env, png(600, 800))).status, 200);
  assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.png`]);
  const first = (await record(env)).cover;

  const afterJpeg = await later(5_000, () => put(env, jpeg(800, 600), 'author', { 'Content-Type': 'image/jpeg' }));
  assert.equal(afterJpeg.status, 200);
  assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.jpg`], 'the PNG is not left behind');
  const second = (await record(env)).cover;
  assert.equal(second.ext, 'jpg');
  assert.notEqual(Date.parse(second.updated_at), Date.parse(first.updated_at));
  assert.notEqual(afterJpeg.body.cover_url, `https://api.eustress.dev/api/simulations/${SIM}/cover?v=${Date.parse(first.updated_at)}`, 'a replaced cover is a new URL');

  assert.equal((await later(10_000, () => put(env, webp('lossy', 700, 700), 'author', { 'Content-Type': 'image/webp' }))).status, 200);
  assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.webp`]);
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).headers.get('Content-Type'), 'image/webp');
  assert.equal((await put(env, png(640, 640))).status, 200);
  assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.png`]);
});

test('the record is read again before it is written: a review decision that landed while the image was stored is not undone', async () => {
  const env = await environment();
  const realPut = env.SCENES.put.bind(env.SCENES);
  env.SCENES.put = async (key, value, meta) => {
    const result = await realPut(key, value, meta);
    if (key.startsWith('covers/')) {
      const sim = JSON.parse(await env.SOCIAL.get(`sim:${SIM}`));
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { ...sim.moderation, status: 'pending', reviewer_note: 'landed in between' } }));
    }
    return result;
  };
  assert.equal((await put(env, png(600, 800))).status, 200);
  const sim = await record(env);
  assert.equal(sim.moderation.status, 'pending', 'the later write to the marker stands');
  assert.equal(sim.moderation.reviewer_note, 'landed in between');
  assert.equal(sim.cover.ext, 'png', 'and the cover is recorded');
});

// -- who may set it -----------------------------------------------------------------------------------------------------------------

test('only the author may set or remove a cover, and a listing that is not theirs answers as one that does not exist', async () => {
  const env = await environment();
  const image = png(600, 800);
  assert.equal((await put(env, image, null)).status, 401);
  assert.equal((await del(env, null)).status, 401);

  const stranger = await put(env, image, 'stranger');
  const missing = await call(env, 'PUT', `/api/simulations/${GHOST}/cover`, { who: 'stranger', body: image, headers: { 'Content-Type': 'image/png' } });
  assert.equal(stranger.status, 404);
  assert.deepEqual(stranger.body, missing.body, 'not told apart');
  assert.equal((await put(env, image, 'admin')).status, 404, 'an admin is not the author either');
  assert.equal((await del(env, 'stranger')).status, 404);
  assert.deepEqual(coverKeys(env), [], 'nothing was stored');
  assert.equal((await record(env)).cover, undefined);

  assert.equal((await put(env, image)).status, 200);
  assert.equal((await del(env, 'stranger')).status, 404);
  assert.equal((await del(env, 'admin')).status, 404);
  assert.deepEqual(coverKeys(env), [`covers/${SIM}/cover.png`], 'a stranger cannot remove it');
  assert.equal((await put(env, image, 'author')).status, 200);
});

test('a listing under a hold is refused as ordinary review, and nothing changes; a freeze from before no longer refuses', async () => {
  const image = png(600, 800);
  const wording = /quarantin|legal|hold|frozen|freeze|csam/i;

  const quarantined = await environment({ moderation: { status: 'quarantined' } });
  const q = await put(quarantined, image);
  assert.equal(q.status, 409, q.text);
  assert.equal(q.body.code, 'in_review');
  assert.doesNotMatch(q.text, wording);
  assert.deepEqual(coverKeys(quarantined), []);
  assert.equal((await record(quarantined)).cover, undefined);
  assert.equal((await del(quarantined)).status, 409);

  // A publish freeze from before 2026-10-15 is not read: no moderation outcome restricts the account (McKale).
  const frozen = await environment();
  await frozen.USERS.put(`publish-frozen:${AUTHOR}`, '1');
  const f = await put(frozen, image);
  assert.equal(f.status, 200, 'the author still sets a cover on their own listing');
  assert.deepEqual(coverKeys(frozen), [`covers/${SIM}/cover.png`]);

  const locked = await environment();
  await locked.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'content_lock' } });
  const l = await put(locked, image);
  assert.equal(l.status, 409);
  assert.equal(l.body.code, 'in_review');
  assert.doesNotMatch(l.text, wording);
  assert.deepEqual(coverKeys(locked), []);

  // A cover that was there before the hold cannot be removed under it.
  const had = await environment({ extra: { cover: { ext: 'png', width: 600, height: 800, bytes: 9, updated_at: '2026-01-05T00:00:00.000Z' } } });
  await had.SCENES.put(`covers/${SIM}/cover.png`, 'evidence');
  await had.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.equal((await del(had)).status, 409);
  assert.deepEqual(coverKeys(had), [`covers/${SIM}/cover.png`]);
  assert.ok((await record(had)).cover);
});

// -- removing it --------------------------------------------------------------------------------------------------------------------

test('removing a cover deletes the file and the record, says a review is owed, and is idempotent', async () => {
  const env = await environment();
  assert.equal((await put(env, png(600, 800))).status, 200);
  const r = await del(env);
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, { ok: true, removed: true, review: 'submit' });
  assert.deepEqual(coverKeys(env), []);
  const sim = await record(env);
  assert.ok(!('cover' in sim), 'no residue in the record');
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 404);

  const again = await del(env);
  assert.equal(again.status, 200);
  assert.deepEqual(again.body, { ok: true, removed: false, review: null }, 'nothing there: nothing changed, nothing to review');
  const never = await environment();
  assert.deepEqual((await del(never)).body, { ok: true, removed: false, review: null });
});

test('removing a cover clears a file the record does not know about, and the other extensions', async () => {
  const env = await environment();
  await env.SCENES.put(`covers/${SIM}/cover.webp`, 'stray');
  await env.SCENES.put(`covers/${SIM}/cover.png`, 'stray');
  const r = await del(env);
  assert.equal(r.body.removed, true);
  assert.deepEqual(coverKeys(env), []);
});

// -- what a listing says about its cover --------------------------------------------------------------------------------------------

const OLD_CARD_FIELDS = ['id', 'name', 'description', 'genre', 'tags', 'max_players', 'author_id', 'author_name', 'thumbnail_url', 'play_count', 'favorite_count',
  'version', 'published_at', 'updated_at', 'rating', 'child_directed', 'featured', 'scene_size_bytes', 'is_open_source', 'format'];

test('the list, the single read and the author\'s projects carry cover_url and the cover\'s size, and every field they had stays', async () => {
  const env = await environment();
  const none = (await call(env, 'GET', '/api/simulations')).body.simulations[0];
  assert.equal(none.cover_url, null);
  assert.equal(none.cover, null);
  for (const field of OLD_CARD_FIELDS) assert.ok(field in none, `${field} is still there`);

  assert.equal((await put(env, png(600, 800))).status, 200);
  const v = Date.parse((await record(env)).cover.updated_at);
  const url = `https://api.eustress.dev/api/simulations/${SIM}/cover?v=${v}`;

  const listed = (await call(env, 'GET', '/api/simulations')).body.simulations[0];
  assert.equal(listed.cover_url, url);
  assert.deepEqual(listed.cover, { width: 600, height: 800 });
  for (const field of OLD_CARD_FIELDS.filter((f) => f !== 'updated_at')) assert.deepEqual(listed[field], none[field], `${field} did not change`);
  assert.equal(listed.updated_at, (await record(env)).cover.updated_at, 'setting a cover is an update to the listing');

  const single = (await call(env, 'GET', `/api/simulations/${SIM}`)).body;
  assert.equal(single.cover_url, url);
  assert.deepEqual(single.cover, { width: 600, height: 800 });

  const gallery = (await call(env, 'GET', '/api/gallery')).body.simulations[0];
  assert.equal(gallery.cover_url, url);

  const mine = (await call(env, 'GET', '/api/projects', { who: 'author' })).body.projects.find((p) => p.id === SIM);
  assert.equal(mine.cover_url, url);
  assert.deepEqual(mine.cover, { width: 600, height: 800 });
  assert.equal(mine.name, 'Box Head');

  await del(env);
  const gone = (await call(env, 'GET', '/api/simulations')).body.simulations[0];
  assert.equal(gone.cover_url, null);
  assert.equal(gone.cover, null);
  assert.equal((await call(env, 'GET', '/api/projects', { who: 'author' })).body.projects[0].cover_url, null);
});

test('the URL is on the origin of the request that asked, so staging names staging', async () => {
  const env = await environment({ extra: { cover: { ext: 'png', width: 600, height: 800, bytes: 9, updated_at: '2026-02-03T04:05:06.000Z' } } });
  const v = Date.parse('2026-02-03T04:05:06.000Z');
  const origin = 'https://api-staging.eustress.dev';
  assert.equal((await call(env, 'GET', '/api/simulations', { origin })).body.simulations[0].cover_url, `${origin}/api/simulations/${SIM}/cover?v=${v}`);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, { origin })).body.cover_url, `${origin}/api/simulations/${SIM}/cover?v=${v}`);
  assert.equal((await call(env, 'GET', '/api/projects', { origin, who: 'author' })).body.projects[0].cover_url, `${origin}/api/simulations/${SIM}/cover?v=${v}`);
});

test('a listing record that names no usable cover has none: a made-up extension never becomes a storage key', () => {
  const ok = { id: SIM, cover: { ext: 'png', width: 600, height: 800, bytes: 1, updated_at: '2026-01-01T00:00:00.000Z' } };
  assert.ok(coverOf(ok));
  assert.deepEqual(coverFields(ok), { cover_url: `https://api.eustress.dev/api/simulations/${SIM}/cover?v=${Date.parse('2026-01-01T00:00:00.000Z')}`, cover: { width: 600, height: 800 } });
  assert.deepEqual(coverFields({ ...ok, cover: { ...ok.cover, updated_at: 'not a date' } }).cover_url, `https://api.eustress.dev/api/simulations/${SIM}/cover?v=0`);
  for (const bad of [
    undefined, null, 'png', [], {}, { ...ok.cover, ext: 'svg' }, { ...ok.cover, ext: '../thumbnails/x/thumb.png' }, { ...ok.cover, ext: 'jpeg' },
    { ...ok.cover, width: '600' }, { ...ok.cover, width: 0 }, { ...ok.cover, height: 1.5 }, { ...ok.cover, height: -1 },
  ]) {
    assert.deepEqual(coverFields({ id: SIM, cover: bad }), { cover_url: null, cover: null }, JSON.stringify(bad));
  }
  assert.deepEqual(coverFields({ cover: ok.cover }), { cover_url: null, cover: null }, 'a record with no id');
  assert.deepEqual(coverFields(null), { cover_url: null, cover: null });
  assert.equal(coverFields(ok, 'https://api-staging.eustress.dev').cover_url.startsWith('https://api-staging.eustress.dev/'), true);
});

// -- reading it ---------------------------------------------------------------------------------------------------------------------

const readyWith = async (options) => {
  const env = await environment(options);
  assert.equal((await put(env, png(600, 800))).status, 200);
  return env;
};

test('an approved and public listing serves its cover to anyone, cached for five minutes, as a picture that can never run as a page', async () => {
  const env = await readyWith();
  for (const who of [undefined, 'stranger']) {
    const r = await call(env, 'GET', cover, { who });
    assert.equal(r.status, 200, `${who || 'anonymous'}`);
    assert.equal(r.headers.get('Content-Type'), 'image/png');
    assert.equal(r.headers.get('Cache-Control'), 'public, max-age=300');
    assert.equal(r.headers.get('X-Content-Type-Options'), 'nosniff');
    assert.match(r.headers.get('Content-Security-Policy'), /default-src 'none'; sandbox/);
    assert.equal(r.headers.get('Set-Cookie'), null);
    assert.equal(r.headers.get('Vary'), 'Origin');
    assert.ok(same(r.bytes, png(600, 800)));
  }
});

test('the author and an admin are served their own cover privately, and nothing about it is cached for others', async () => {
  const env = await readyWith();
  for (const who of ['author', 'admin']) {
    const r = await call(env, 'GET', cover, { who });
    assert.equal(r.status, 200, who);
    assert.equal(r.headers.get('Cache-Control'), 'private, no-store', who);
    assert.equal(r.headers.get('X-Content-Type-Options'), 'nosniff');
  }
});

test('a listing with no cover answers 404 No cover, and so does one that does not exist', async () => {
  const env = await environment();
  for (const who of [undefined, 'author', 'admin']) {
    const r = await call(env, 'GET', cover, { who });
    assert.equal(r.status, 404);
    assert.deepEqual(r.body, { error: 'No cover' });
  }
  const ghost = await call(env, 'GET', `/api/simulations/${GHOST}/cover`);
  assert.equal(ghost.status, 404);
  assert.deepEqual(ghost.body, { error: 'No cover' });

  const orphan = await environment({ extra: { cover: { ext: 'png', width: 600, height: 800, bytes: 9, updated_at: '2026-01-05T00:00:00.000Z' } } });
  const gone = await call(orphan, 'GET', cover, { who: 'author' });
  assert.equal(gone.status, 404, 'the record names a cover the store no longer has');
  assert.deepEqual(gone.body, { error: 'No cover' });
});

test('an unapproved listing shows its cover to its author and admins only; the public gets 404, the same as no cover', async () => {
  const env = await readyWith({ moderation: { status: 'pending', rating: 'all_ages' } });
  const nothing = await call(await environment(), 'GET', cover);
  for (const who of [undefined, 'stranger']) {
    const r = await call(env, 'GET', cover, { who });
    assert.equal(r.status, 404, who || 'anonymous');
    assert.deepEqual(r.body, nothing.body, 'not told apart from a listing with no cover');
  }
  for (const who of ['author', 'admin']) {
    const r = await call(env, 'GET', cover, { who });
    assert.equal(r.status, 200, who);
    assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
  }
});

test('a private or archived listing serves its cover to nobody but its author and admins', async () => {
  const env = await readyWith({ is_public: false });
  assert.equal((await call(env, 'GET', cover)).status, 404);
  assert.equal((await call(env, 'GET', cover, { who: 'stranger' })).status, 404);
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 200);
  assert.equal((await call(env, 'GET', cover, { who: 'admin' })).status, 200);
});

test('a quarantined listing serves nothing, not even to its author; an admin still sees it', async () => {
  const env = await readyWith();
  const sim = await record(env);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, moderation: { status: 'quarantined', rating: 'all_ages' } }));
  for (const who of [undefined, 'stranger', 'author']) assert.equal((await call(env, 'GET', cover, { who })).status, 404, who || 'anonymous');
  assert.equal((await call(env, 'GET', cover, { who: 'admin' })).status, 200);
});

test('a legal lock beats an approved marker: nothing is served to the public, whatever the record says', async () => {
  const serveBlock = await readyWith();
  await serveBlock.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } });
  for (const who of [undefined, 'stranger', 'author']) assert.equal((await call(serveBlock, 'GET', cover, { who })).status, 404, `serve_block, ${who || 'anonymous'}`);
  assert.equal((await call(serveBlock, 'GET', cover, { who: 'admin' })).status, 200);

  const contentLock = await readyWith();
  await contentLock.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'content_lock' } });
  for (const who of [undefined, 'stranger']) assert.equal((await call(contentLock, 'GET', cover, { who })).status, 404, `content_lock, ${who || 'anonymous'}`);
  assert.equal((await call(contentLock, 'GET', cover, { who: 'author' })).status, 200, 'held, not quarantined: the author is not told anything different');
});

test('a listing with no author on record is not "its own author" to a visitor with no session', async () => {
  const env = await readyWith({ moderation: { status: 'pending', rating: 'all_ages' } });
  const sim = await record(env);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, author_id: null }));
  assert.equal((await call(env, 'GET', cover)).status, 404);
  assert.equal((await call(env, 'GET', cover, { who: 'stranger' })).status, 404);

  // The same on a listed listing whose cover has not been reviewed, which is the case an owner's view would open.
  const listed = await environment();
  await bind(listed);
  assert.equal((await put(listed, png(600, 800))).status, 200);
  assert.equal((await call(listed, 'GET', cover)).status, 404, 'control: unreviewed, so not public');
  const record2 = await record(listed);
  await listed.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...record2, author_id: null }));
  assert.equal((await call(listed, 'GET', cover)).status, 404, 'a visitor with no session is not the author of a listing that names none');
  assert.equal((await call(listed, 'GET', cover, { who: 'stranger' })).status, 404);
});

test('no other cover is reachable through this one: the extension in the record is the only key read', async () => {
  const env = await readyWith();
  await env.SCENES.put(`thumbnails/${SIM}/thumb.png`, 'the thumbnail');
  const r = await call(env, 'GET', cover);
  assert.ok(same(r.bytes, png(600, 800)), 'the cover, never the thumbnail');
});

// -- the approval binding -----------------------------------------------------------------------------------------------------------

test('a cover is bound by its digest: another file of the same type under the same name is a different object, and R2 holds the digest it was given', async () => {
  const env = await environment();
  const first = png(600, 800);
  assert.equal((await put(env, first)).status, 200);
  const key = `covers/${SIM}/cover.png`;
  const head = await env.SCENES.head(key);
  assert.equal(Buffer.from(head.checksums.sha256).toString('hex'), sha256(first), 'R2 returns the digest the cover was stored with');
  await bind(env);
  assert.equal(await coverApproved(env, await record(env)), true);
  assert.equal((await call(env, 'GET', cover)).status, 200);

  const second = png(600, 800, { weigh: 5 });
  assert.equal((await put(env, second)).status, 200);
  assert.deepEqual(coverKeys(env), [key], 'the same key');
  const again = await env.SCENES.head(key);
  assert.notEqual(again.version, head.version, 'a put is a new version');
  assert.equal(Buffer.from(again.checksums.sha256).toString('hex'), sha256(second));
  assert.equal(await coverApproved(env, await record(env)), false, 'the approval was for the first file');
  assert.equal((await call(env, 'GET', cover)).status, 404, 'so the public is not served the second');
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 200);
  await bind(env);
  assert.equal((await call(env, 'GET', cover)).status, 200, 'until a review binds it');
});

test('a cover set after the approval is not served to the public until a review binds it; the Gallery gets 404 and no placeholder', async () => {
  const env = await environment();
  await bind(env);
  const approved = await record(env);
  assert.equal(await coverApproved(env, approved), true, 'no cover, and none bound');
  assert.equal(await thumbnailApproved(env, approved), true);

  assert.equal((await put(env, png(600, 800))).status, 200);
  const sim = await record(env);
  assert.equal(await coverApproved(env, sim), false, 'a changed cover is not the reviewed one');
  assert.equal(await thumbnailApproved(env, sim), true, 'and the thumbnail binding is not affected by it');
  assert.equal(isListable(sim), true, 'the listing stays listed meanwhile');

  const strangerView = await call(env, 'GET', cover, { who: 'stranger' });
  assert.equal(strangerView.status, 404);
  assert.deepEqual(strangerView.body, { error: 'No cover' });
  assert.notEqual(strangerView.headers.get('Content-Type'), 'image/svg+xml', 'no placeholder panel for a cover');
  const anon = await call(env, 'GET', cover);
  assert.equal(anon.status, 404);
  assert.equal((await call(env, 'GET', '/api/simulations')).body.simulations.length, 1, 'the listing is still in the Gallery');
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 200, 'the author sees what is waiting for review');
  assert.equal((await call(env, 'GET', cover, { who: 'admin' })).status, 200);

  // A review runs and binds the cover that is stored now.
  await bind(env);
  assert.equal(await coverApproved(env, sim), true);
  const served = await call(env, 'GET', cover);
  assert.equal(served.status, 200);
  assert.equal(served.headers.get('Cache-Control'), 'public, max-age=300');
});

test('replacing an approved cover with a different file makes it unreviewed again, and removing it leaves nothing to serve', async () => {
  const env = await environment();
  assert.equal((await put(env, png(600, 800))).status, 200);
  await bind(env);
  assert.equal((await call(env, 'GET', cover)).status, 200);

  assert.equal((await put(env, jpeg(800, 800), 'author', { 'Content-Type': 'image/jpeg' })).status, 200);
  assert.equal(await coverApproved(env, await record(env)), false);
  assert.equal((await call(env, 'GET', cover)).status, 404, 'the public is not served the new file before it is reviewed');

  assert.equal((await del(env)).status, 200);
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 404);
  assert.equal((await call(env, 'GET', cover)).status, 404);
});

test('where an approval binding is required, a cover with no binding is not served to the public until a review writes one', async () => {
  const env = await readyWith();
  env.REQUIRE_APPROVAL_BINDING = 'true';
  assert.equal((await call(env, 'GET', cover)).status, 404);
  assert.equal((await call(env, 'GET', cover, { who: 'stranger' })).status, 404);
  assert.equal((await call(env, 'GET', cover, { who: 'author' })).status, 200);
  await bind(env);
  assert.equal((await call(env, 'GET', cover)).status, 200);
});

test('what R2 returns for an object is what the approval binds it by: a digest when it was stored with one, an etag and version when it was not', async () => {
  const env = await environment();
  assert.equal((await put(env, png(600, 800))).status, 200);
  const hashed = await env.SCENES.head(`covers/${SIM}/cover.png`);
  assert.match(identOfObject(hashed), /^sha256:[0-9a-f]{64}$/);
  assert.equal(identOfObject(hashed), (await snapshotReviewed(env, await record(env))).cover);

  const plain = await environment();
  await plain.SCENES.put(`covers/${SIM}/cover.png`, png(600, 800));
  const unhashed = await plain.SCENES.head(`covers/${SIM}/cover.png`);
  assert.match(identOfObject(unhashed), /^etag:etag-covers\/.+:v\d+$/);
  assert.equal(identOfObject(unhashed), (await snapshotReviewed(plain, await record(plain))).cover);

  const captures = await withCaptures();
  const head = await captures.SCENES.head(captureR2Key(SIM, 0, 'png', 0));
  assert.equal(identOfObject(head), (await snapshotReviewed(captures, await record(captures), 0)).captures['0']);
});

/// Puts a different object under `key` the first time it is read, as an upload landing between the check and the read would.
function swapOnFirstRead(env, key, replacement, options) {
  const realGet = env.SCENES.get.bind(env.SCENES);
  let swapped = false;
  env.SCENES.get = async (k) => {
    if (!swapped && k === key) {
      swapped = true;
      await env.SCENES.put(k, replacement, options);
    }
    return realGet(k);
  };
  return () => swapped;
}

test('the cover that is sent is the cover that was judged: a file put between the check and the read is not served, with or without a digest', async () => {
  const unreviewed = png(600, 800, { weigh: 7 });
  for (const [what, seed, options] of [
    ['a stored digest', async (env) => { await put(env, png(600, 800)); }, { sha256: sha256(unreviewed) }],
    ['no digest, only an etag and a version', async (env) => {
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...(await record(env)), cover: { ext: 'png', width: 600, height: 800, bytes: 9, updated_at: '2026-01-05T00:00:00.000Z' } }));
      await env.SCENES.put(`covers/${SIM}/cover.png`, png(600, 800));
    }, undefined],
  ]) {
    const control = await environment();
    await seed(control);
    await bind(control);
    assert.equal((await call(control, 'GET', cover)).status, 200, `${what}: control`);

    const env = await environment();
    await seed(env);
    await bind(env);
    const happened = swapOnFirstRead(env, `covers/${SIM}/cover.png`, unreviewed, options);
    const r = await call(env, 'GET', cover);
    assert.ok(happened(), `${what}: the swap ran`);
    assert.equal(r.status, 404, `${what}: the public is not served an object nobody reviewed`);
    assert.deepEqual(r.body, { error: 'No cover' });
    assert.ok(!same(r.bytes, unreviewed));

    // The author may see what is waiting, whichever object it is.
    const owner = await environment();
    await seed(owner);
    await bind(owner);
    swapOnFirstRead(owner, `covers/${SIM}/cover.png`, unreviewed, options);
    const own = await call(owner, 'GET', cover, { who: 'author' });
    assert.equal(own.status, 200, what);
    assert.equal(own.headers.get('Cache-Control'), 'private, no-store');
  }
});

test('a preview that is sent is the capture the approval bound: a file put between the check and the read is not served', async () => {
  const unreviewed = png(640, 360, { weigh: 3 });
  const control = await withCaptures();
  assert.equal((await call(control, 'GET', preview(0))).status, 200);

  const env = await withCaptures();
  const happened = swapOnFirstRead(env, captureR2Key(SIM, 0, 'png', 0), unreviewed, undefined);
  const r = await call(env, 'GET', preview(0));
  assert.ok(happened(), 'the swap ran');
  assert.equal(r.status, 404);
  assert.deepEqual(r.body, { error: 'No preview' });
  assert.ok(!same(r.bytes, unreviewed));
  // The other captures are the ones that were bound.
  assert.equal((await call(env, 'GET', preview(1))).status, 200);
});

test('a revoked approval serves no cover to the public, and an approval from before covers serves the one that was there', async () => {
  const revoked = await readyWith();
  await bind(revoked, { revoked: true });
  assert.equal((await call(revoked, 'GET', cover)).status, 404);
  assert.equal((await call(revoked, 'GET', cover, { who: 'author' })).status, 200);

  const unbound = await readyWith();
  assert.equal((await call(unbound, 'GET', cover)).status, 200, 'a listing approved before bindings existed has no record to differ from');
});

// -- route classes ------------------------------------------------------------------------------------------------------------------

test('the cover routes are named: setting and removing are session routes, the previews are public reads', () => {
  for (const method of ['PUT', 'DELETE', 'GET']) {
    const r = routePolicy(method, '/api/simulations/abc-123/cover');
    assert.equal(r.class, SESSION, method);
    assert.equal(r.explicit, true, method);
  }
  for (const p of ['/api/simulations/abc-123/previews', '/api/simulations/abc-123/preview/0', '/api/simulations/abc-123/preview/7']) {
    const r = routePolicy('GET', p);
    assert.equal(r.class, PUBLIC, p);
    assert.equal(r.explicit, true, p);
  }
  assert.equal(routePolicy('GET', '/api/simulations/abc-123/preview/8').explicit, false, 'only 0 to 7 are previews');
});

test('other methods on the cover and preview routes are refused, and the routes are only what they say', async () => {
  const env = await readyWith();
  assert.equal((await call(env, 'POST', cover, { who: 'author', body: png(600, 800) })).status, 405);
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/previews`, { who: 'author', body: '{}' })).status, 405);
  assert.equal((await call(env, 'DELETE', `/api/simulations/${SIM}/preview/0`, { who: 'author' })).status, 405);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/cover/extra`)).status, 404);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/preview/8`)).status, 404);
});

// -- previews: the review captures of an approved listing -----------------------------------------------------------------------------

const PNG_A = png(640, 360);
const JPG_B = jpeg(640, 360);
const WEBP_C = webp('lossy', 640, 360);
const preview = (n) => `/api/simulations/${SIM}/preview/${n}`;
const previews = `/api/simulations/${SIM}/previews`;

async function withCaptures(options = {}) {
  const env = await environment(options);
  // The stored header lies on purpose: the type comes from the extension.
  await env.SCENES.put(captureR2Key(SIM, 0, 'png', 0), PNG_A, { httpMetadata: { contentType: 'text/html' } });
  await env.SCENES.put(captureR2Key(SIM, 1, 'jpg', 0), JPG_B, { httpMetadata: { contentType: 'text/html' } });
  await env.SCENES.put(captureR2Key(SIM, 3, 'webp', 0), WEBP_C, { httpMetadata: { contentType: 'text/html' } });
  await bind(env, { evidence_epoch: 0 });
  // A capture from a later epoch, which no approval has bound.
  await env.SCENES.put(captureR2Key(SIM, 5, 'png', 1), PNG_A);
  return env;
}

test('an approved listing offers the captures its approval bound, as absolute URLs on this origin, and serves them', async () => {
  const env = await withCaptures();
  const list = await call(env, 'GET', previews);
  assert.equal(list.status, 200, list.text);
  assert.deepEqual(list.body, { previews: [0, 1, 3].map((n) => ({ n, url: `https://api.eustress.dev/api/simulations/${SIM}/preview/${n}` })) });
  assert.equal(list.headers.get('Cache-Control'), 'public, max-age=60');

  const staging = await call(env, 'GET', previews, { origin: 'https://api-staging.eustress.dev' });
  assert.equal(staging.body.previews[0].url, `https://api-staging.eustress.dev/api/simulations/${SIM}/preview/0`);

  for (const [n, bytes, mime] of [[0, PNG_A, 'image/png'], [1, JPG_B, 'image/jpeg'], [3, WEBP_C, 'image/webp']]) {
    const r = await call(env, 'GET', preview(n));
    assert.equal(r.status, 200, `preview ${n}`);
    assert.equal(r.headers.get('Content-Type'), mime, 'the type follows the extension, never the stored header');
    assert.equal(r.headers.get('Cache-Control'), 'public, max-age=300');
    assert.equal(r.headers.get('X-Content-Type-Options'), 'nosniff');
    assert.match(r.headers.get('Content-Security-Policy'), /default-src 'none'; sandbox/);
    assert.equal(r.headers.get('Set-Cookie'), null);
    assert.ok(same(r.bytes, bytes), `preview ${n} is the capture as uploaded`);
  }
});

test('a number that is not offered is 404, and so is a capture of an epoch the approval did not bind', async () => {
  const env = await withCaptures();
  for (const n of [2, 4, 5, 6, 7]) {
    const r = await call(env, 'GET', preview(n));
    assert.equal(r.status, 404, `preview ${n}`);
    assert.deepEqual(r.body, { error: 'No preview' });
  }
  assert.equal((await call(env, 'GET', preview(8))).status, 404);
});

test('previews are offered to the public only while the listing is approved, public, bound and unlocked', async () => {
  const offered = async (env) => (await call(env, 'GET', previews)).body.previews.map((p) => p.n);

  assert.deepEqual(await offered(await withCaptures()), [0, 1, 3], 'control');

  // Rated all_ages: a listing with no rating is adult content, whose frames also need a signed URL (src/adult_content.mjs).
  const pending = await withCaptures({ moderation: { status: 'pending', rating: 'all_ages' } });
  assert.deepEqual(await offered(pending), []);
  assert.equal((await call(pending, 'GET', preview(0))).status, 404);

  const quarantined = await withCaptures({ moderation: { status: 'quarantined', rating: 'all_ages' } });
  assert.deepEqual(await offered(quarantined), []);
  assert.equal((await call(quarantined, 'GET', preview(0))).status, 404);

  const priv = await withCaptures({ is_public: false });
  assert.deepEqual(await offered(priv), []);
  assert.equal((await call(priv, 'GET', preview(0))).status, 404);

  const locked = await withCaptures();
  await locked.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.deepEqual(await offered(locked), []);
  assert.equal((await call(locked, 'GET', preview(0))).status, 404);
  const held = await withCaptures();
  await held.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'content_lock' } });
  assert.deepEqual(await offered(held), []);

  const revoked = await withCaptures();
  await bind(revoked, { revoked: true });
  assert.deepEqual(await offered(revoked), []);
  assert.equal((await call(revoked, 'GET', preview(1))).status, 404);

  const swapped = await withCaptures();
  const sim = await record(swapped);
  await swapped.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...sim, r2_key: `universes/${SIM}/another.pak` }));
  await swapped.SCENES.put(`universes/${SIM}/another.pak`, 'another');
  assert.deepEqual(await offered(swapped), [], 'a package that is not the reviewed one offers nothing');

  const unbound = await environment();
  await unbound.SCENES.put(captureR2Key(SIM, 0, 'png', 0), PNG_A);
  assert.deepEqual(await offered(unbound), [], 'an approved listing with no binding offers nothing');

  const missing = await call(await environment(), 'GET', `/api/simulations/${GHOST}/previews`);
  assert.equal(missing.status, 200);
  assert.deepEqual(missing.body, { previews: [] }, 'a listing that does not exist reads as one with no previews');
});

// The handler asks the moderation module on every request and serves only what it returns.
const jsonOut = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
async function direct(env, path, approvedCaptureKeys) {
  const url = new URL(`https://api.eustress.dev${path}`);
  const res = await handleCoverRoute(new Request(url), url, env, {}, { verifyAuth: async () => null, requireAdmin: async () => null, json: jsonOut, approvedCaptureKeys });
  const bytes = new Uint8Array(await res.arrayBuffer());
  return { status: res.status, headers: res.headers, bytes, body: (() => { try { return JSON.parse(new TextDecoder().decode(bytes)); } catch { return null; } })() };
}

/// An approval that binds the capture objects at these keys, by number, for a handler whose captures come from a stub.
async function bindCaptureObjects(env, keysByNumber) {
  const captures = {};
  for (const [n, key] of Object.entries(keysByNumber)) captures[n] = identOfObject(await env.SCENES.head(key));
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, captures, evidence_epoch: 0, by: 'test', at: '2026-01-02T00:00:00.000Z' }));
}

test('every request asks which captures may be shown: an answer that changes is followed at once, with no memory in between', async () => {
  const env = await environment();
  await env.SCENES.put('evidence/x/capture-0.png', PNG_A);
  await bindCaptureObjects(env, { 0: 'evidence/x/capture-0.png' });
  let answer = [{ n: 0, ext: 'png', key: 'evidence/x/capture-0.png' }];
  let asked = 0;
  const ask = async () => { asked += 1; return answer; };
  assert.equal((await direct(env, preview(0), ask)).status, 200);
  assert.equal(asked, 1);
  assert.equal((await direct(env, preview(0), ask)).status, 200);
  assert.equal(asked, 2, 'asked again, not remembered');
  assert.deepEqual((await direct(env, previews, ask)).body.previews.map((p) => p.n), [0]);
  assert.equal(asked, 3);
  answer = [];
  assert.equal((await direct(env, preview(0), ask)).status, 404, 'the approval moved: the capture is not served on the next request');
  assert.deepEqual((await direct(env, previews, ask)).body, { previews: [] });
  assert.equal(asked, 5);
});

test('only the key the moderation module returns for that number is served, never another object', async () => {
  const env = await environment();
  await env.SCENES.put('evidence/x/capture-0.png', PNG_A);
  await env.SCENES.put('evidence/other/capture-0.png', JPG_B);
  await env.SCENES.put('evidence/x/capture-1.png', JPG_B);
  await bindCaptureObjects(env, { 0: 'evidence/x/capture-0.png' });
  const ask = async () => [{ n: 0, ext: 'png', key: 'evidence/x/capture-0.png' }];
  const hit = await direct(env, preview(0), ask);
  assert.ok(same(hit.bytes, PNG_A));
  assert.equal((await direct(env, preview(1), ask)).status, 404, 'capture 1 exists in storage and is not offered');

  // An extension decides the type; a made-up one, an out-of-range number and a repeat are ignored.
  const odd = async () => [
    { n: 0, ext: 'jpg', key: 'evidence/x/capture-0.png' },
    { n: 0, ext: 'png', key: 'evidence/other/capture-0.png' },
    { n: 9, ext: 'png', key: 'evidence/x/capture-1.png' },
    { n: 2, ext: 'svg', key: 'evidence/x/capture-1.png' },
    { n: 3, ext: 'png' },
    null,
  ];
  const first = await direct(env, preview(0), odd);
  assert.equal(first.headers.get('Content-Type'), 'image/jpeg', 'the first answer for a number wins, typed by its extension');
  assert.ok(same(first.bytes, PNG_A));
  assert.equal((await direct(env, preview(2), odd)).status, 404);
  assert.equal((await direct(env, preview(3), odd)).status, 404);
  assert.deepEqual((await direct(env, previews, odd)).body.previews.map((p) => p.n), [0]);
  assert.equal((await direct(env, preview(0), async () => 'not a list')).status, 404);
  assert.equal((await direct(env, preview(0), async () => [{ n: 0, ext: 'png', key: 'evidence/missing.png' }])).status, 404, 'offered but gone from storage');
});

test('a listing that is not listed is not asked about at all: the legal lock is read first', async () => {
  const env = await environment({ moderation: { status: 'pending', rating: 'all_ages' } });
  let asked = 0;
  const ask = async () => { asked += 1; return [{ n: 0, ext: 'png', key: 'x' }]; };
  assert.equal((await direct(env, preview(0), ask)).status, 404);
  assert.equal(asked, 0);

  const locked = await environment();
  await locked.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.equal((await direct(locked, previews, ask)).body.previews.length, 0);
  assert.equal(asked, 0, 'a lock that the record does not show still counts');
});
