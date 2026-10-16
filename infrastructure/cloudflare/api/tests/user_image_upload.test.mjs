// The routes that take a user image check it, strip its metadata, fingerprint it
// with a SHA-256 R2 verifies, and keep one thumbnail per listing. Against the
// real Worker with fake storage, with a spy on put to see what R2 was given.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = mintToken(AUTHOR);
const GPS = 'gps-52.5200N-13.4050E';

const text = s => new TextEncoder().encode(s);
const cat = (...parts) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let at = 0; for (const p of parts) { out.set(p, at); at += p.length; } return out; };
const be32 = n => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
const be16 = n => new Uint8Array([(n >>> 8) & 255, n & 255]);
const pngChunk = (name, data) => cat(be32(data.length), text(name), data, new Uint8Array(4));
const png = (w, h, extra = []) => cat(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', cat(be32(w), be32(h), new Uint8Array([8, 2, 0, 0, 0]))), ...extra, pngChunk('IDAT', new Uint8Array([1, 2, 3])), pngChunk('IEND', new Uint8Array(0)));
const seg = (marker, data) => cat(new Uint8Array([0xff, marker]), be16(data.length + 2), data);
const jpegWithGps = (w, h) => cat(new Uint8Array([0xff, 0xd8]), seg(0xe0, text('JFIF\0\u0001\u0001\0\0\u0001\0\u0001\0\0')), seg(0xe1, text(`Exif\0\0${GPS}`)),
  seg(0xc0, cat(new Uint8Array([8]), be16(h), be16(w), new Uint8Array([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]))), seg(0xda, new Uint8Array([3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0])), new Uint8Array([9, 9, 0xff, 0xd9]));

async function seed() {
  const env = fakeEnv();
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'World', author_id: AUTHOR, is_public: true, moderation: { status: 'pending' }, r2_key: `universes/${SIM}/universe.pak`, updated_at: '2026-01-01T00:00:00.000Z' }));
  const puts = [];
  const realPut = env.SCENES.put;
  env.SCENES.put = async (key, value, meta) => { puts.push({ key, meta, value }); return realPut(key, value, meta); };
  return { env, puts };
}

async function call(env, method, path, body, headers = {}) {
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: { Authorization: `Bearer ${TOKEN}`, ...headers }, body }), env, ctx);
  return { status: res.status, text: await res.text().catch(() => '') };
}

const asText = bytes => new TextDecoder('latin1').decode(bytes);

test('a thumbnail is stored without its metadata, with its SHA-256, and replaces one in another format', async () => {
  const { env, puts } = await seed();
  await env.SCENES.put(`thumbnails/${SIM}/thumb.webp`, 'an older thumbnail in another format');
  const r = await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, jpegWithGps(1280, 720));
  assert.equal(r.status, 200, r.text);
  const put = puts.find(p => p.key === `thumbnails/${SIM}/thumb.jpg`);
  assert.ok(put, 'stored as the format its bytes are');
  assert.ok(!asText(put.value).includes(GPS), 'the location is not stored');
  assert.equal(put.meta.sha256, createHash('sha256').update(Buffer.from(put.value)).digest('hex'), 'R2 is given the SHA-256 of the stored bytes');
  assert.equal(put.meta.httpMetadata.contentType, 'image/jpeg');
  assert.equal(env.SCENES.objects.has(`thumbnails/${SIM}/thumb.webp`), false, 'the older one is gone: the route serves the first it finds');
});

test('a thumbnail of extreme dimensions or not an image is refused', async () => {
  const { env } = await seed();
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, png(20000, 20000))).status, 413, '400 megapixels in a small file');
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, text('<svg xmlns="http://www.w3.org/2000/svg"/>'), { 'Content-Type': 'image/svg+xml' })).status, 415);
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, png(1280, 720))).status, 200);
});

test('a review capture is stored without metadata and with its SHA-256 too', async () => {
  const { env, puts } = await seed();
  const r = await call(env, 'PUT', `/api/simulations/${SIM}/captures/2`, png(1280, 720, [pngChunk('tEXt', text(`Comment\0${GPS}`))]));
  assert.equal(r.status, 200, r.text);
  const put = puts.find(p => /moderation\/capture-2\.png$/.test(p.key));
  assert.ok(put);
  assert.ok(!asText(put.value).includes(GPS));
  assert.equal(put.meta.sha256, createHash('sha256').update(Buffer.from(put.value)).digest('hex'));
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/captures/2`, png(20000, 20000))).status, 413);
});

test('a .pak is stored with its SHA-256, which R2 verifies and an approval binds', async () => {
  const { env, puts } = await seed();
  const body = text('the world package');
  const r = await call(env, 'PUT', `/api/simulations/${SIM}/space`, body);
  assert.equal(r.status, 200, r.text);
  const put = puts.find(p => p.key === `universes/${SIM}/universe.pak`);
  assert.equal(put.meta.sha256, createHash('sha256').update(Buffer.from(body)).digest('hex'));
  assert.equal(JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)).pak_sha256, put.meta.sha256, 'and kept on the listing, for the dedup and the fingerprint');
});
