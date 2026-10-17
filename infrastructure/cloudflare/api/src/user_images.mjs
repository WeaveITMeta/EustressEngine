// =============================================================================
// Images users upload: what may be stored and how it may be served
// =============================================================================
//
// A thumbnail, a cover or a preview capture is an image a user chose, shown to
// the public from this origin. Four things hold for all of them:
//
//   1. The type is taken from the bytes (PNG, JPEG or WebP), never from the
//      request's header or the file's name, and never SVG: an SVG from a user
//      is a document, not a picture.
//   2. Dimensions are read from the header(s) and bounded. A small file can
//      decode to a huge bitmap in a visitor's browser. Every place a file
//      states a size (a WebP's canvas and each of its frames, every JPEG frame
//      header) is read, and the largest counts.
//   3. Only what draws the picture is kept. The file is rebuilt from an
//      ALLOWLIST of its segments, so metadata no one has thought of (EXIF with
//      GPS, XMP, text chunks, comments, vendor segments, a second image
//      appended after the first, bytes after the end marker) is dropped by
//      default. The pixels are never re-encoded.
//   4. It is served with `nosniff` and a sandbox policy, so even a mislabelled
//      body cannot run as a page on this origin.
//   5. It is one still picture that every decoder draws the same way, because the
//      AI judge reviews these images and the public sees them in browsers. An
//      animation (APNG, animated WebP) is refused: a decoder that shows only the
//      first frame and one that plays it show different pictures. Colour
//      transforms (PNG gAMA, cHRM, sRGB, iCCP; the WebP ICCP chunk; a JPEG ICC
//      profile) are dropped: one decoder honours them and another does not, and
//      a gamma or profile can hide a picture from one of them. Transparency
//      is refused where it can be told from the header and chunks alone: a
//      grey-and-alpha PNG (colour type 4), a PNG transparency chunk (tRNS), and
//      every WebP alpha marker (an ALPH chunk, the extended header's alpha flag,
//      the lossless alpha-is-used bit). A picture carried in the alpha channel
//      is blank to a decoder that drops alpha or composites on white, and plain
//      to one that composites on dark. An RGBA PNG (colour type 6) is still
//      accepted: Studio's own captures are RGBA PNGs, and whether their alpha is
//      used can only be told by inflating the image data. The PNG background
//      chunk (bKGD), a hint some viewers composite on, is dropped.
//
// A SHA-256 of the stored bytes goes in with them (`put(..., { sha256 })`, which
// R2 verifies and returns from `head`), because an MD5 etag can be collided and
// an approval is bound to the exact bytes that were reviewed (moderation.mjs,
// the approval binding).
// =============================================================================

export const MAX_IMAGE_SIDE = 8192;
export const MAX_IMAGE_PIXELS = 36_000_000;
export const ANIMATED_IMAGE_PROBLEM = 'The image is animated; send a single still PNG, JPEG or WebP picture';
export const TRANSPARENT_IMAGE_PROBLEM = 'The image has transparency; send an opaque PNG, JPEG or WebP picture';
export const MULTIPLE_IMAGE_PROBLEM = 'The file holds more than one picture; send a single PNG, JPEG or WebP picture';
/// What a walker returns for an image it refuses outright (see 5 above): `refused` is the reason, in the uploader's words,
/// and `code` names it for a report.
const ANIMATED = Object.freeze({ refused: ANIMATED_IMAGE_PROBLEM, code: 'animated' });
const TRANSPARENT = Object.freeze({ refused: TRANSPARENT_IMAGE_PROBLEM, code: 'transparent' });
/// A second picture in one file: a second JPEG frame header, a JFIF thumbnail, a second WebP image chunk. A decoder may
/// show either one.
const MULTIPLE = Object.freeze({ refused: MULTIPLE_IMAGE_PROBLEM, code: 'multiple_pictures' });

/// Headers every user image is served with, to be spread into a Response's headers.
export const USER_IMAGE_HEADERS = Object.freeze({
  'X-Content-Type-Options': 'nosniff',
  'Content-Security-Policy': "default-src 'none'; sandbox",
});

const MIME = { png: 'image/png', jpeg: 'image/jpeg', webp: 'image/webp' };
const EXT = { png: 'png', jpeg: 'jpg', webp: 'webp' };

const toBytes = (input) => (input instanceof Uint8Array ? input : new Uint8Array(input instanceof ArrayBuffer ? input : input.buffer ?? input));
const u16be = (b, o) => (b[o] << 8) | b[o + 1];
const u32be = (b, o) => ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0;
const u16le = (b, o) => b[o] | (b[o + 1] << 8);
const u24le = (b, o) => b[o] | (b[o + 1] << 8) | (b[o + 2] << 16);
const u32le = (b, o) => (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
const ascii = (b, o, n) => String.fromCharCode(...b.subarray(o, o + n));
const startsWith = (b, text) => b.length >= text.length && ascii(b, 0, text.length) === text;

function join(parts) {
  const bytes = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { bytes.set(p, at); at += p.length; }
  return bytes;
}

function typeOf(b) {
  if (b.length >= 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47 && b[4] === 0x0d && b[5] === 0x0a && b[6] === 0x1a && b[7] === 0x0a) return 'png';
  if (b.length >= 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'jpeg';
  if (b.length >= 12 && ascii(b, 0, 4) === 'RIFF' && ascii(b, 8, 4) === 'WEBP') return 'webp';
  return null;
}

// ── PNG ──
// Kept: the image (IHDR, PLTE, IDAT, IEND), significant bits, histogram and physical size. Every other chunk is
// dropped (colour transforms, the background hint, text, time, EXIF, anything unknown). An APNG (acTL), a grey-and-alpha
// image (colour type 4) and a transparency chunk (tRNS) are refused. An unknown CRITICAL chunk is not an image this
// platform can vouch for: the file is refused. Bytes after IEND are dropped.
const PNG_KEEP = new Set(['IHDR', 'PLTE', 'IDAT', 'sBIT', 'hIST', 'pHYs']);

function walkPng(b) {
  const kept = [b.subarray(0, 8)];
  let size = null;
  let o = 8;
  let sawEnd = false;
  while (o + 12 <= b.length) {
    const length = u32be(b, o);
    const name = ascii(b, o + 4, 4);
    const end = o + 12 + length;
    if (end > b.length) return null;
    if (size === null) {
      if (name !== 'IHDR' || length !== 13) return null;
      size = { width: u32be(b, o + 8), height: u32be(b, o + 12) };
      // The colour type (IHDR byte 9): 4 is grey with alpha.
      if (b[o + 17] === 4) return TRANSPARENT;
    } else if (name === 'IHDR') return null;
    if (name === 'IEND') { kept.push(b.subarray(o, end)); sawEnd = true; break; }
    if (name === 'acTL') return ANIMATED;
    if (name === 'tRNS') return TRANSPARENT;
    if (PNG_KEEP.has(name)) kept.push(b.subarray(o, end));
    else if (name[0] === name[0].toUpperCase()) return null;
    o = end;
  }
  // A file that stops before its end marker is not a whole image.
  return sawEnd && size ? { size, bytes: join(kept) } : null;
}

// ── JPEG ──
// Kept: the frame, tables and scans (SOFn, DHT, DAC, DQT, DRI, DNL, SOS and the scan data with its restart
// markers), JFIF (APP0 beginning "JFIF\0") and Adobe's colour flag (APP14), which says how the scan's channels
// decode. Dropped: everything else, including the ICC colour profile and MPF and FlashPix (APP2), EXIF and XMP
// (APP1), JFXX (APP0), Photoshop (APP13), comments, reserved markers, and everything after the first end-of-image,
// which is where a motion photo or an Ultra HDR file keeps its second image.
const isSof = (m) => m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc;

// A JFIF header (APP0) states the size of a thumbnail it carries in its bytes 12 and 13 (after "JFIF\0", the version, the
// units and the densities); a JFIF header with a thumbnail, or with more bytes than its 14, holds a second picture.
const jfifThumbnail = (payload) => startsWith(payload, 'JFIF\0') && (payload.length > 14 || (payload.length >= 14 && payload[12] * payload[13] > 0));

function keepJpegSegment(marker, payload) {
  if (isSof(marker) || marker === 0xc4 || marker === 0xcc || marker === 0xdb || marker === 0xdd || marker === 0xdc || marker === 0xda) return true;
  if (marker === 0xe0) return startsWith(payload, 'JFIF\0');
  if (marker === 0xee) return startsWith(payload, 'Adobe');
  return false;
}

function walkJpeg(b) {
  const kept = [b.subarray(0, 2)];
  const sofs = [];
  let o = 2;
  while (o + 2 <= b.length) {
    if (b[o] !== 0xff) return null;
    const marker = b[o + 1];
    if (marker === 0xff) { o += 1; continue; }
    if (marker === 0xd9) { kept.push(b.subarray(o, o + 2)); return { sofs, bytes: join(kept) }; }
    // A second start-of-image outside a payload is a second picture: refused.
    if (marker === 0xd8) return null;
    // Markers without a length that sit outside a scan carry nothing.
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) { o += 2; continue; }
    if (o + 4 > b.length) return null;
    const length = u16be(b, o + 2);
    if (length < 2 || o + 2 + length > b.length) return null;
    const end = o + 2 + length;
    const payload = b.subarray(o + 4, end);
    if (isSof(marker)) {
      if (payload.length < 5) return null;
      if (sofs.length) return MULTIPLE;
      sofs.push({ height: u16be(payload, 1), width: u16be(payload, 3) });
    }
    if (marker === 0xe0 && jfifThumbnail(payload)) return MULTIPLE;
    if (keepJpegSegment(marker, payload)) kept.push(b.subarray(o, end));
    o = end;
    if (marker === 0xda) {
      // The scan: entropy-coded bytes up to the next marker, where a stuffed zero, a restart marker and fill bytes are not markers.
      const start = o;
      while (o + 1 < b.length) {
        if (b[o] === 0xff) {
          const n = b[o + 1];
          if (n === 0x00 || (n >= 0xd0 && n <= 0xd7)) { o += 2; continue; }
          if (n === 0xff) { o += 1; continue; }
          break;
        }
        o += 1;
      }
      if (o + 1 >= b.length) return null;
      kept.push(b.subarray(start, o));
    }
  }
  return null;
}

// ── WebP ──
// Kept: the image (VP8, VP8L, VP8X). An animation (ANIM, ANMF, or the extended header's animation flag) and alpha (an
// ALPH chunk, the extended header's alpha flag, or the lossless alpha-is-used bit) are refused. Every other chunk is
// dropped (the ICC profile, EXIF, XMP and anything unknown), as are bytes after the RIFF's own length.
function vp8Size(b, o, size) {
  if (size >= 10 && b[o + 3] === 0x9d && b[o + 4] === 0x01 && b[o + 5] === 0x2a) return { width: u16le(b, o + 6) & 0x3fff, height: u16le(b, o + 8) & 0x3fff };
  return null;
}

function vp8lSize(b, o, size) {
  if (size < 5 || b[o] !== 0x2f) return null;
  const bits = (b[o + 1] | (b[o + 2] << 8) | (b[o + 3] << 16) | (b[o + 4] << 24)) >>> 0;
  return { width: (bits & 0x3fff) + 1, height: ((bits >>> 14) & 0x3fff) + 1 };
}

function webpChunk(name, data) {
  const out = new Uint8Array(8 + data.length + (data.length & 1));
  out.set(new TextEncoder().encode(name), 0);
  out[4] = data.length & 0xff; out[5] = (data.length >>> 8) & 0xff; out[6] = (data.length >>> 16) & 0xff; out[7] = (data.length >>> 24) & 0xff;
  out.set(data, 8);
  return out;
}

function walkWebp(b) {
  if (b.length < 20) return null;
  const riffEnd = u32le(b, 4) + 8;
  if (riffEnd > b.length || riffEnd < 20) return null;
  const kept = [];
  const sizes = [];
  let vp8xAt = -1;
  let pictures = 0;
  let o = 12;
  while (o + 8 <= riffEnd) {
    const name = ascii(b, o, 4);
    const size = u32le(b, o + 4);
    const stop = o + 8 + size + (size & 1);
    if (stop > riffEnd + (size & 1)) return null;
    const data = b.subarray(o + 8, o + 8 + size);
    if (name === 'ANIM' || name === 'ANMF') return ANIMATED;
    if (name === 'VP8X') {
      if (size < 10) return null;
      if (data[0] & 0x02) return ANIMATED;
      if (data[0] & 0x10) return TRANSPARENT;
      sizes.push({ width: u24le(b, o + 12) + 1, height: u24le(b, o + 15) + 1 });
      vp8xAt = kept.length;
      kept.push(webpChunk(name, data));
    } else if (name === 'VP8 ') {
      if (pictures++) return MULTIPLE;
      const s = vp8Size(b, o + 8, size);
      if (s) sizes.push(s);
      kept.push(webpChunk(name, data));
    } else if (name === 'VP8L') {
      // The lossless header's alpha-is-used bit (bit 28 after the signature byte).
      if (size >= 5 && (u32le(b, o + 9) >>> 28) & 1) return TRANSPARENT;
      if (pictures++) return MULTIPLE;
      const s = vp8lSize(b, o + 8, size);
      if (s) sizes.push(s);
      kept.push(webpChunk(name, data));
    } else if (name === 'ALPH') {
      return TRANSPARENT;
    }
    o = stop;
  }
  if (!sizes.length) return null;
  // The extended header must stop promising metadata (EXIF 0x08, XMP 0x04) and a profile it no longer holds (ICC 0x20).
  if (vp8xAt >= 0) kept[vp8xAt][8] &= ~(0x08 | 0x04 | 0x20);
  const body = join([new TextEncoder().encode('WEBP'), ...kept]);
  const bytes = join([new TextEncoder().encode('RIFF'), new Uint8Array([body.length & 0xff, (body.length >>> 8) & 0xff, (body.length >>> 16) & 0xff, (body.length >>> 24) & 0xff]), body]);
  return { sizes, bytes };
}

// What the bytes are, whatever their size: `{ type, mime, ext, width, height, bytes }`, `{ refused }` for an image refused
// outright (an animation, transparency), or null when they are not a PNG, JPEG or WebP whose structure can be walked.
// `width` and `height` are the largest the file states anywhere.
function readImage(input) {
  const b = toBytes(input);
  const type = typeOf(b);
  if (!type) return null;
  let sizes;
  let bytes;
  if (type === 'png') {
    const walked = walkPng(b);
    if (!walked || walked.refused) return walked;
    sizes = [walked.size];
    bytes = walked.bytes;
  } else if (type === 'jpeg') {
    const walked = walkJpeg(b);
    if (!walked || walked.refused) return walked;
    if (!walked.sofs.length) return null;
    sizes = walked.sofs;
    bytes = walked.bytes;
  } else {
    const walked = walkWebp(b);
    if (!walked || walked.refused) return walked;
    sizes = walked.sizes;
    bytes = walked.bytes;
  }
  const width = Math.max(...sizes.map((s) => s.width));
  const height = Math.max(...sizes.map((s) => s.height));
  if (!width || !height) return null;
  return { type, mime: MIME[type], ext: EXT[type], width, height, bytes };
}

const inBounds = (info) => info.width <= MAX_IMAGE_SIDE && info.height <= MAX_IMAGE_SIDE && info.width * info.height <= MAX_IMAGE_PIXELS;
const publicInfo = ({ type, mime, ext, width, height }) => ({ type, mime, ext, width, height });

/// What the bytes are, when they are an image this platform accepts at a size it
/// accepts: `{ type, mime, ext, width, height }`, else null (not a PNG, JPEG or
/// WebP, a structure that cannot be walked, or dimensions over the bounds).
export function inspectImage(input) {
  const info = readImage(input);
  return info && !info.refused && inBounds(info) ? publicInfo(info) : null;
}

/// Why these bytes cannot be stored as a user image, in words fit for the
/// uploader, or null when they can.
export function imageProblem(input) {
  return problemOf(readImage(input));
}
function problemOf(info) {
  if (!info) return 'The image must be a PNG, JPEG or WebP file';
  if (info.refused) return info.refused;
  if (!inBounds(info))
    return `The image is ${info.width}x${info.height}; the most accepted is ${MAX_IMAGE_SIDE} pixels on a side and ${MAX_IMAGE_PIXELS / 1_000_000} megapixels`;
  return null;
}

/// Why bytes already stored as listing art are not what an upload would store today, as a code, or null when they are:
/// 'animated', 'transparent', 'multiple_pictures' (refused at upload), 'not_an_accepted_image' (not a PNG, JPEG or WebP
/// whose structure can be walked), 'dimensions' (over the bounds) or 'dropped_at_upload' (it carries chunks or segments an
/// upload drops now, such as a colour transform or EXIF, and is served as its rebuilt copy). Pure, and no more than the
/// walk an upload does: the pixels are never decoded.
export function storedImageProblem(input) {
  return storedImageVerdict(input).problem;
}

/// What the public is served for bytes stored as listing art, decided once (src/moderation.mjs publicImageBody keeps it):
/// `{ verdict: 'ok', problem: null }`, the bytes as they are; `{ verdict: 'stripped', problem: 'dropped_at_upload', bytes }`,
/// the same picture rebuilt from the segments that draw it (colour transforms, EXIF and the like dropped, the pixels not
/// touched); or `{ verdict: 'refused', problem }` for an animation, transparency, a second picture, a file that is not an
/// accepted image, or dimensions over the bounds, which the public is answered as absent. Pure.
export function storedImageVerdict(input) {
  const b = toBytes(input);
  const info = readImage(b);
  if (!info) return { verdict: 'refused', problem: 'not_an_accepted_image' };
  if (info.refused) return { verdict: 'refused', problem: info.code };
  if (!inBounds(info)) return { verdict: 'refused', problem: 'dimensions' };
  if (info.bytes.length === b.length && info.bytes.every((v, i) => v === b[i])) return { verdict: 'ok', problem: null };
  return { verdict: 'stripped', problem: 'dropped_at_upload', bytes: info.bytes, mime: info.mime };
}

// ── Stored objects: their identity, their verdict record, their derived copy ──
// Shared by moderation.mjs (which decides and serves), evidence_vault.mjs (which seals) and takedown.mjs (which blocks), so
// none of them needs another's module.

/// The identity of an R2 object as head() or get() returned it: its SHA-256 when R2 holds one, else its etag and version.
export function identOfObject(object) {
  if (!object) return null;
  const sha = object.checksums?.sha256;
  const hex = (buf) => [...new Uint8Array(buf)].map((x) => x.toString(16).padStart(2, '0')).join('');
  // `version` is unique to each upload of a key (strongly consistent), so a swap that keeps the etag still moves.
  return sha ? `sha256:${typeof sha === 'string' ? sha : hex(sha)}` : `etag:${object.etag}${object.version ? `:v${object.version}` : ''}`;
}

/// Where a stored listing image's verdict record lives (src/moderation.mjs publicImageBody), by the object's identity.
export const imageVerdictKey = (ident) => `derived/verdicts/${ident}`;

/// The derived copy of the stored image at `key`, `{ key, sha256 }`, when its verdict record names one; else null. The copy is
/// the same picture rebuilt without colour transforms and the like (storedImageVerdict), written once, never deleted.
export async function derivedCopyOf(env, key) {
  const head = key ? await env.SCENES.head(key) : null;
  const stored = head ? await env.SCENES.get(imageVerdictKey(identOfObject(head))) : null;
  if (!stored) return null;
  let record;
  try { record = JSON.parse(await stored.text()); } catch { return null; }
  const m = typeof record?.stripped_key === 'string' ? /^derived\/stripped\/([0-9a-f]{64})$/.exec(record.stripped_key) : null;
  return m ? { key: record.stripped_key, sha256: m[1] } : null;
}

/// The colour type a PNG's header states (IHDR byte 9: 2 RGB, 3 palette, 4 grey and alpha, 6 RGBA), or null for bytes that
/// are not a PNG with a header where the format puts it.
export function pngColourType(input) {
  const b = toBytes(input);
  return typeOf(b) === 'png' && b.length >= 33 && ascii(b, 12, 4) === 'IHDR' ? b[25] : null;
}

// ── Alpha, measured (a PNG of colour type 6) ──
// Whether an RGBA PNG uses its alpha channel: true when any pixel's alpha is below full, false when every pixel is opaque,
// 'unknown' when it cannot be told within the bounds. Measured at upload only, never when an image is served, and only
// recorded: nothing is refused or held on it. The image data is inflated as a stream and unfiltered row by row, holding
// two rows (the row and the one above, which the filters read) and never the whole image. The inflate stops, unknown, as
// soon as it yields more than the header says the image holds (height * (1 + bytes per row)), when the stream ends short,
// past the time budget, or for an interlaced (Adam7) image.
export const ALPHA_MEASURE_BUDGET_MS = 250;

/// `{ alpha_used, reason, inflated, held_max }`: `held_max` is the most bytes of rows held at once (two rows), for a test.
/// `opts.now` and `opts.budgetMs` replace the clock and the budget; `opts.decompress` the DecompressionStream maker.
export async function measurePngAlpha(input, opts = {}) {
  const b = toBytes(input);
  const unknown = (reason, extra = {}) => ({ alpha_used: 'unknown', reason, inflated: 0, held_max: 0, ...extra });
  if (pngColourType(b) !== 6) return unknown('not_rgba_png');
  const width = u32be(b, 16), height = u32be(b, 20), depth = b[24], interlace = b[28];
  if (interlace !== 0) return unknown('interlaced');
  if (depth !== 8 && depth !== 16) return unknown('bit_depth');
  const bpp = 4 * (depth / 8);
  const stride = width * bpp;
  const expected = height * (1 + stride);
  if (!width || !height || !Number.isSafeInteger(expected)) return unknown('header');
  // The image data: every IDAT payload, in order, as views of the bytes (nothing copied).
  const parts = [];
  for (let o = 8; o + 12 <= b.length;) {
    const length = u32be(b, o);
    const name = ascii(b, o + 4, 4);
    if (o + 12 + length > b.length) return unknown('truncated');
    if (name === 'IDAT') parts.push(b.subarray(o + 8, o + 8 + length));
    if (name === 'IEND') break;
    o += 12 + length;
  }
  const now = opts.now || (() => Date.now());
  const deadline = now() + (opts.budgetMs ?? ALPHA_MEASURE_BUDGET_MS);
  const source = new ReadableStream({ start(c) { for (const part of parts) c.enqueue(part); c.close(); } });
  const reader = source.pipeThrough(opts.decompress ? opts.decompress() : new DecompressionStream('deflate')).getReader();
  // The two rows held: the row being filled (with its filter byte) and the unfiltered row above it.
  let prev = new Uint8Array(stride);
  let cur = new Uint8Array(1 + stride);
  const held = () => prev.length + cur.length;
  let heldMax = held();
  let filled = 0, rows = 0, inflated = 0;
  const stop = async (result) => { await reader.cancel().catch(() => {}); return { ...result, inflated, held_max: heldMax }; };
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      inflated += value.length;
      if (inflated > expected) return stop({ alpha_used: 'unknown', reason: 'over_ihdr_size' });
      if (now() > deadline) return stop({ alpha_used: 'unknown', reason: 'time_budget' });
      for (let i = 0; i < value.length;) {
        const take = Math.min(cur.length - filled, value.length - i);
        cur.set(value.subarray(i, i + take), filled);
        filled += take;
        i += take;
        if (filled < cur.length) break;
        // A whole row: unfilter it in place against the row above, then read its alpha samples.
        const row = cur.subarray(1);
        const filter = cur[0];
        if (filter > 4) return stop({ alpha_used: 'unknown', reason: 'bad_filter' });
        for (let x = 0; x < stride; x++) {
          const a = x >= bpp ? row[x - bpp] : 0, up = prev[x], c = x >= bpp ? prev[x - bpp] : 0;
          let pred = 0;
          if (filter === 1) pred = a;
          else if (filter === 2) pred = up;
          else if (filter === 3) pred = (a + up) >>> 1;
          else if (filter === 4) { const pa = Math.abs(up - c), pb = Math.abs(a - c), pc = Math.abs(a + up - 2 * c); pred = pa <= pb && pa <= pc ? a : pb <= pc ? up : c; }
          row[x] = (row[x] + pred) & 255;
        }
        // Alpha is the last sample of each pixel: one byte at depth 8, two (big-endian) at depth 16.
        for (let x = bpp - (depth / 8); x < stride; x += bpp) {
          if (row[x] < 255 || (depth === 16 && row[x + 1] < 255)) return stop({ alpha_used: true, reason: 'alpha_below_full', row: rows });
        }
        prev.set(row);
        filled = 0;
        rows += 1;
        heldMax = Math.max(heldMax, held());
      }
    }
  } catch {
    return stop({ alpha_used: 'unknown', reason: 'inflate_error' });
  }
  if (inflated < expected || rows < height) return { alpha_used: 'unknown', reason: 'short_stream', inflated, held_max: heldMax };
  return { alpha_used: false, reason: 'opaque', inflated, held_max: heldMax };
}

/// The same image rebuilt from the segments that draw it (see the allowlists above):
/// no EXIF or GPS, XMP, text, comments, vendor segments, appended second image or
/// trailing bytes. The pixels are not touched. Throws when the structure cannot be
/// walked: a file that cannot be read is refused, never stored as it came.
export function stripMetadata(input) {
  const info = readImage(input);
  if (!info || info.refused) throw new Error(info ? info.refused : 'The image file could not be read');
  return info.bytes;
}

/// Hex SHA-256 of bytes: what R2 verifies when it is passed as `sha256`, and
/// what an approval binds.
export async function sha256Hex(input) {
  const digest = await crypto.subtle.digest('SHA-256', toBytes(input));
  return [...new Uint8Array(digest)].map((x) => x.toString(16).padStart(2, '0')).join('');
}

/// Check, clean and fingerprint an uploaded image: `{ ok: true, bytes, info,
/// sha256 }` for what to store, or `{ ok: false, error, status }` for the uploader
/// (415 for an animation or transparency, a kind of image this platform does not take; 413 otherwise).
export async function prepareUserImage(input) {
  const info = readImage(input);
  const problem = problemOf(info);
  if (problem) return { ok: false, error: problem, status: info?.refused ? 415 : 413 };
  return { ok: true, bytes: info.bytes, info: publicInfo(info), sha256: await sha256Hex(info.bytes) };
}
