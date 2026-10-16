// What a user image must be before the platform stores it: a PNG, JPEG or WebP
// by its bytes, bounded dimensions, no metadata, and a SHA-256 to bind.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { inspectImage, imageProblem, stripMetadata, prepareUserImage, sha256Hex, USER_IMAGE_HEADERS, MAX_IMAGE_SIDE } from '../src/user_images.mjs';

const text = s => new TextEncoder().encode(s);
const cat = (...parts) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let at = 0; for (const p of parts) { out.set(p, at); at += p.length; } return out; };
const be32 = n => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
const be16 = n => new Uint8Array([(n >>> 8) & 255, n & 255]);
const le32 = n => new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255]);
const le24 = n => new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255]);

const SECRET = 'gps-52.5200N-13.4050E';

// ── synthetic images (headers are real; pixel data is a few bytes) ──

const pngChunk = (name, data) => cat(be32(data.length), text(name), data, new Uint8Array(4));
const png = (w, h, extra = []) => cat(
  new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  pngChunk('IHDR', cat(be32(w), be32(h), new Uint8Array([8, 2, 0, 0, 0]))),
  ...extra,
  pngChunk('IDAT', new Uint8Array([1, 2, 3, 4])),
  pngChunk('IEND', new Uint8Array(0)),
);

const jpegSeg = (marker, data) => cat(new Uint8Array([0xff, marker]), be16(data.length + 2), data);
const jpeg = (w, h) => cat(
  new Uint8Array([0xff, 0xd8]),
  jpegSeg(0xe0, text('JFIF\0\u0001\u0001\0\0\u0001\0\u0001\0\0')),
  jpegSeg(0xe1, text(`Exif\0\0${SECRET}`)),
  jpegSeg(0xfe, text(`comment ${SECRET}`)),
  jpegSeg(0xc0, cat(new Uint8Array([8]), be16(h), be16(w), new Uint8Array([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]))),
  jpegSeg(0xda, new Uint8Array([3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0])),
  new Uint8Array([9, 9, 9, 9, 0xff, 0xd9]),
);

const webpChunk = (name, data) => cat(text(name), le32(data.length), data, data.length & 1 ? new Uint8Array(1) : new Uint8Array(0));
const webp = (w, h) => {
  const body = cat(
    text('WEBP'),
    webpChunk('VP8X', cat(new Uint8Array([0x0c, 0, 0, 0]), le24(w - 1), le24(h - 1))),
    webpChunk('VP8L', cat(new Uint8Array([0x2f]), le32((w - 1) | ((h - 1) << 14)))),
    webpChunk('EXIF', text(`Exif\0\0${SECRET}`)),
    webpChunk('XMP ', text(`<x:xmpmeta>${SECRET}</x:xmpmeta>`)),
  );
  return cat(text('RIFF'), le32(body.length), body);
};

test('the type and the size come from the bytes', () => {
  assert.deepEqual(inspectImage(png(1280, 720)), { type: 'png', mime: 'image/png', ext: 'png', width: 1280, height: 720 });
  assert.deepEqual(inspectImage(jpeg(640, 360)), { type: 'jpeg', mime: 'image/jpeg', ext: 'jpg', width: 640, height: 360 });
  assert.deepEqual(inspectImage(webp(800, 450)), { type: 'webp', mime: 'image/webp', ext: 'webp', width: 800, height: 450 });
  assert.equal(inspectImage(text('<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>')), null, 'never SVG');
  assert.equal(inspectImage(text('GIF89a....')), null);
  assert.equal(inspectImage(new Uint8Array(0)), null);
  assert.equal(inspectImage(png(1, 1).subarray(0, 14)), null, 'a header cut short has no size');
});

test('extreme dimensions are refused, in words for the uploader', () => {
  assert.equal(imageProblem(png(4000, 4000)), null, '16 megapixels');
  assert.match(imageProblem(png(MAX_IMAGE_SIDE + 1, 100)), /most accepted/);
  assert.match(imageProblem(png(8000, 8000)), /megapixels/, '64 megapixels in a small file');
  assert.equal(inspectImage(png(20000, 20000)), null, 'inspectImage answers null for a size it will not accept');
  assert.notEqual(inspectImage(png(4000, 4000)), null);
  assert.match(imageProblem(text('plain text')), /PNG, JPEG or WebP/);
});

test('metadata is dropped from every format and the pixels are left alone', () => {
  const stripped = {
    png: stripMetadata(png(100, 50, [pngChunk('tEXt', text(`Comment\0${SECRET}`)), pngChunk('eXIf', text(SECRET)), pngChunk('tIME', new Uint8Array(7)), pngChunk('gAMA', new Uint8Array(4))])),
    jpeg: stripMetadata(jpeg(100, 50)),
    webp: stripMetadata(webp(100, 50)),
  };
  for (const [kind, bytes] of Object.entries(stripped)) {
    assert.ok(bytes, kind);
    assert.ok(!new TextDecoder('latin1').decode(bytes).includes(SECRET), `${kind}: the location is gone`);
    assert.deepEqual([inspectImage(bytes).width, inspectImage(bytes).height], [100, 50], `${kind}: still the same picture`);
  }
  const latin = bytes => new TextDecoder('latin1').decode(bytes);
  assert.ok(latin(stripped.png).includes('gAMA') && latin(stripped.png).includes('IDAT'), 'colour and image data stay');
  assert.ok(latin(stripped.jpeg).includes('JFIF'), 'the JFIF header stays');
  assert.ok(stripped.jpeg.length < jpeg(100, 50).length);
  // WebP: the container size is right and the extended header no longer promises metadata.
  const w = stripped.webp;
  assert.equal(w[4] | (w[5] << 8) | (w[6] << 16) | (w[7] << 24), w.length - 8);
  assert.equal(w[20] & 0x0c, 0);
});

test('a picture with nothing to strip comes back byte for byte, and one that cannot be read is refused', async () => {
  const clean = png(1280, 720);
  assert.deepEqual(stripMetadata(clean), clean);
  const cut = png(10, 10).subarray(0, 40);
  assert.throws(() => stripMetadata(cut), /could not be read/);
  assert.equal((await prepareUserImage(cut)).ok, false);
});

test('prepareUserImage hands back what to store: stripped bytes and their SHA-256', async () => {
  const prepared = await prepareUserImage(jpeg(1280, 720));
  assert.equal(prepared.ok, true);
  assert.equal(prepared.info.mime, 'image/jpeg');
  assert.equal(prepared.sha256, createHash('sha256').update(prepared.bytes).digest('hex'));
  assert.equal(await sha256Hex(prepared.bytes), prepared.sha256);
  assert.ok(!new TextDecoder('latin1').decode(prepared.bytes).includes(SECRET));
  const refused = await prepareUserImage(png(20000, 20000));
  assert.deepEqual([refused.ok, typeof refused.error], [false, 'string']);
});

test('user images are served with nosniff and a sandbox', () => {
  assert.equal(USER_IMAGE_HEADERS['X-Content-Type-Options'], 'nosniff');
  assert.equal(USER_IMAGE_HEADERS['Content-Security-Policy'], "default-src 'none'; sandbox");
});
