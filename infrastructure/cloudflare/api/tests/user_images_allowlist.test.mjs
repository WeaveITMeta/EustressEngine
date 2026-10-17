// User images are rebuilt from an allowlist of the segments that draw them. Every
// shape below is one a location, a name or a note can hide in; each carries the
// string GPS, and none of them may come out the other side holding it.

import test from 'node:test';
import assert from 'node:assert/strict';
import { inspectImage, imageProblem, stripMetadata, prepareUserImage, ANIMATED_IMAGE_PROBLEM, MULTIPLE_IMAGE_PROBLEM } from '../src/user_images.mjs';

const GPS = 'gps-52.5200N-13.4050E';
const text = s => new TextEncoder().encode(s);
const cat = (...parts) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let at = 0; for (const p of parts) { out.set(p, at); at += p.length; } return out; };
const be32 = n => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
const be16 = n => new Uint8Array([(n >>> 8) & 255, n & 255]);
const le32 = n => new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255]);
const le24 = n => new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255]);
const holds = bytes => new TextDecoder('latin1').decode(bytes).includes(GPS);

// ── JPEG ──
const seg = (marker, data) => cat(new Uint8Array([0xff, marker]), be16(data.length + 2), data);
const sof = (marker, w, h) => seg(marker, cat(new Uint8Array([8]), be16(h), be16(w), new Uint8Array([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1])));
const sos = () => seg(0xda, new Uint8Array([3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0]));
const SCAN = new Uint8Array([7, 7, 7, 0xff, 0x00, 7, 0xff, 0xd0, 7]); // entropy-coded bytes with a stuffed zero and a restart marker
const EOI = new Uint8Array([0xff, 0xd9]);
const SOI = new Uint8Array([0xff, 0xd8]);
const jpeg = (...segments) => cat(SOI, ...segments, sos(), SCAN, EOI);
const frame = (w = 100, h = 50) => sof(0xc0, w, h);

test('JPEG: a second image after the first end-of-image (motion photo, Ultra HDR) is dropped, GPS and all', () => {
  const first = jpeg(frame());
  const second = cat(SOI, seg(0xe1, text(`Exif\0\0${GPS}`)), frame(), sos(), SCAN, EOI);
  const out = stripMetadata(cat(first, second));
  assert.equal(holds(out), false);
  assert.deepEqual(out, first, 'what is left is the first image, untouched');
  assert.equal(inspectImage(out).width, 100);
});

test('JPEG: reserved markers, JFXX, MPF and FlashPix segments, comments and every other APPn are dropped', () => {
  for (const [label, segment] of [
    ['reserved FFF5', seg(0xf5, text(GPS))],
    ['JFXX in APP0', seg(0xe0, text(`JFXX\0${GPS}`))],
    ['MPF in APP2', seg(0xe2, text(`MPF\0${GPS}`))],
    ['FlashPix in APP2', seg(0xe2, text(`FPXR\0${GPS}`))],
    ['EXIF in APP1', seg(0xe1, text(`Exif\0\0${GPS}`))],
    ['XMP in APP1', seg(0xe1, text(`http://ns.adobe.com/xap/1.0/\0${GPS}`))],
    ['Photoshop in APP13', seg(0xed, text(`Photoshop 3.0\0${GPS}`))],
    ['vendor APP7', seg(0xe7, text(GPS))],
    ['comment', seg(0xfe, text(GPS))],
    ['JPG13 FFFD', seg(0xfd, text(GPS))],
  ]) {
    const out = stripMetadata(jpeg(segment, frame()));
    assert.equal(holds(out), false, label);
    assert.equal(inspectImage(out).height, 50, `${label}: still a picture`);
  }
});

test('JPEG: JFIF and the Adobe flag stay; an ICC colour profile of any size goes', () => {
  const jfif = seg(0xe0, text('JFIF\0\u0001\u0001\0\0\u0001\0\u0001\0\0'));
  const icc = size => seg(0xe2, cat(text('ICC_PROFILE\0'), new Uint8Array([1, 1]), new Uint8Array(size)));
  const adobe = seg(0xee, text('Adobe\0d\0\0\0\0\u0001'));
  const latin = new TextDecoder('latin1').decode(stripMetadata(jpeg(jfif, icc(2000), adobe, frame())));
  assert.ok(latin.includes('JFIF') && latin.includes('Adobe'));
  assert.ok(!latin.includes('ICC_PROFILE'), 'a profile one decoder honours and another does not');
});

test('JPEG: a file with two frame headers is two pictures and is refused, as is a second picture inside the file and a file with no end', () => {
  for (const twoFrames of [jpeg(frame(100, 50), sof(0xc2, 20000, 20000)), jpeg(frame(100, 50), sof(0xc2, 100, 50))]) {
    assert.equal(imageProblem(twoFrames), MULTIPLE_IMAGE_PROBLEM);
    assert.equal(inspectImage(twoFrames), null);
  }
  assert.equal(imageProblem(cat(SOI, frame(), sos(), SCAN, SOI, frame(), sos(), SCAN, EOI)), 'The image must be a PNG, JPEG or WebP file', 'a start-of-image where a segment should be');
  assert.throws(() => stripMetadata(cat(SOI, frame(), sos(), SCAN)), /could not be read/, 'no end-of-image');
});

test('JPEG: restart markers and a stuffed zero inside a scan are scan data, and progressive scans all stay', () => {
  const progressive = cat(SOI, sof(0xc2, 64, 64), sos(), SCAN, seg(0xc4, new Uint8Array(20)), sos(), SCAN, EOI);
  const out = stripMetadata(progressive);
  assert.deepEqual(out, progressive);
  assert.equal(inspectImage(out).width, 64);
});

// ── WebP ──
const chunk = (name, data) => cat(text(name), le32(data.length), data, data.length & 1 ? new Uint8Array(1) : new Uint8Array(0));
const riff = (...chunks) => { const body = cat(text('WEBP'), ...chunks); return cat(text('RIFF'), le32(body.length), body); };
const vp8x = (w, h, flags = 0) => chunk('VP8X', cat(new Uint8Array([flags, 0, 0, 0]), le24(w - 1), le24(h - 1)));
const vp8l = (w, h) => chunk('VP8L', cat(new Uint8Array([0x2f]), le32((w - 1) | ((h - 1) << 14)), new Uint8Array([0, 0, 0])));

test('WebP: only the chunks that draw the picture stay; EXIF, XMP and every unknown chunk go', () => {
  const dirty = riff(vp8x(100, 50, 0x0c), vp8l(100, 50), chunk('EXIF', text(`Exif\0\0${GPS}`)), chunk('XMP ', text(GPS)), chunk('GPSX', text(GPS)), chunk('junk', text(GPS)));
  const out = stripMetadata(dirty);
  assert.equal(holds(out), false);
  assert.equal(inspectImage(out).width, 100);
  assert.equal(out[20] & 0x0c, 0, 'the extended header no longer promises metadata');
  assert.equal(out[4] | (out[5] << 8) | (out[6] << 16) | (out[7] << 24), out.length - 8, 'and the container size is right');
});

test('WebP: bytes after the RIFF length are dropped, an animation is refused, and the ICC profile goes', async () => {
  const still = riff(vp8x(100, 50), vp8l(100, 50));
  assert.equal(holds(stripMetadata(cat(still, text(GPS)))), false, 'trailing bytes');
  const frameHeader = cat(le24(0), le24(0), le24(99), le24(49), le24(40), new Uint8Array([0]));
  const anmf = chunk('ANMF', cat(frameHeader, vp8l(100, 50)));
  for (const [what, animated] of [
    ['ANIM and ANMF', riff(vp8x(100, 50, 0x02), chunk('ANIM', new Uint8Array(6)), anmf)],
    ['an ANMF frame alone', riff(vp8x(100, 50), anmf)],
    ['the animation flag alone', riff(vp8x(100, 50, 0x02), vp8l(100, 50))],
  ]) {
    assert.equal(imageProblem(animated), ANIMATED_IMAGE_PROBLEM, what);
    assert.equal(inspectImage(animated), null, what);
    assert.throws(() => stripMetadata(animated), /animated/, what);
    assert.deepEqual([(await prepareUserImage(animated)).ok, (await prepareUserImage(animated)).status], [false, 415], what);
  }
  const profiled = stripMetadata(riff(vp8x(100, 50, 0x20), chunk('ICCP', new Uint8Array(100)), vp8l(100, 50)));
  assert.ok(!new TextDecoder('latin1').decode(profiled).includes('ICCP'), 'the profile chunk goes');
  assert.equal(profiled[20] & 0x20, 0, 'and the extended header no longer promises it');
});

test('WebP: the canvas and every frame state a size, and the largest counts', () => {
  assert.match(imageProblem(riff(vp8x(100, 100), vp8l(16384, 16384))), /most accepted/, 'a small canvas over a huge frame');
  assert.match(imageProblem(riff(vp8x(20000, 20000), vp8l(100, 100))), /most accepted/, 'a huge canvas over a small frame');
  const huge = cat(le24(0), le24(0), le24(19999), le24(19999), le24(40), new Uint8Array([0]));
  assert.equal(imageProblem(riff(vp8x(100, 100, 0x02), chunk('ANMF', cat(huge, vp8l(100, 100))))), ANIMATED_IMAGE_PROBLEM, 'an animation is refused whatever its frames say');
  assert.equal(imageProblem(riff(vp8x(100, 100), vp8l(100, 100))), null);
});

// ── PNG ──
const pngChunk = (name, data) => cat(be32(data.length), text(name), data, new Uint8Array(4));
const ihdr = (w, h) => pngChunk('IHDR', cat(be32(w), be32(h), new Uint8Array([8, 2, 0, 0, 0])));
const PNG_SIG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const png = (...chunks) => cat(PNG_SIG, ihdr(100, 50), ...chunks, pngChunk('IDAT', new Uint8Array([1, 2, 3])), pngChunk('IEND', new Uint8Array(0)));

test('PNG: text, time, EXIF, unknown ancillary chunks and every colour transform go, bytes after IEND go; an APNG is refused', async () => {
  const dirty = png(pngChunk('tEXt', text(`Comment\0${GPS}`)), pngChunk('zTXt', text(GPS)), pngChunk('iTXt', text(GPS)), pngChunk('eXIf', text(GPS)), pngChunk('tIME', new Uint8Array(7)), pngChunk('prVt', text(GPS)));
  assert.equal(holds(stripMetadata(cat(dirty, text(GPS)))), false);
  const colour = new TextDecoder('latin1').decode(stripMetadata(png(pngChunk('gAMA', new Uint8Array(4)), pngChunk('cHRM', new Uint8Array(32)), pngChunk('sRGB', new Uint8Array(1)), pngChunk('iCCP', cat(text('p\0\0'), new Uint8Array(100))))));
  for (const name of ['gAMA', 'cHRM', 'sRGB', 'iCCP']) assert.ok(!colour.includes(name), `${name} goes`);
  assert.ok(colour.includes('IDAT'), 'the picture stays');
  const apng = png(pngChunk('acTL', new Uint8Array(8)), pngChunk('fcTL', new Uint8Array(26)));
  assert.equal(imageProblem(apng), ANIMATED_IMAGE_PROBLEM);
  assert.equal(inspectImage(apng), null);
  assert.deepEqual([(await prepareUserImage(apng)).ok, (await prepareUserImage(apng)).status], [false, 415]);
  // Frame chunks with no animation control are not an animation to any decoder; they are dropped like any unknown chunk.
  assert.ok(!new TextDecoder('latin1').decode(stripMetadata(png(pngChunk('fcTL', new Uint8Array(26))))).includes('fcTL'));
});

test('PNG: an unknown critical chunk, a second header and a missing end are refused', () => {
  assert.throws(() => stripMetadata(png(pngChunk('ABCD', text(GPS)))), /could not be read/, 'an unknown critical chunk is not an image we can vouch for');
  assert.throws(() => stripMetadata(png(ihdr(8000, 8000))), /could not be read/, 'a second IHDR');
  assert.throws(() => stripMetadata(cat(PNG_SIG, ihdr(10, 10), pngChunk('IDAT', new Uint8Array(3)))), /could not be read/, 'no IEND');
});

test('prepareUserImage rebuilds from the allowlist and fingerprints what it stored', async () => {
  const prepared = await prepareUserImage(cat(jpeg(seg(0xe1, text(`Exif\0\0${GPS}`)), frame(1280, 720)), jpeg(frame())));
  assert.equal(prepared.ok, true);
  assert.equal(holds(prepared.bytes), false);
  assert.deepEqual([prepared.info.width, prepared.info.height], [1280, 720]);
  assert.match(prepared.sha256, /^[0-9a-f]{64}$/);
});
