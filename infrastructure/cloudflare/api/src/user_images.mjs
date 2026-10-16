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
//
// A SHA-256 of the stored bytes goes in with them (`put(..., { sha256 })`, which
// R2 verifies and returns from `head`), because an MD5 etag can be collided and
// an approval is bound to the exact bytes that were reviewed (moderation.mjs,
// the approval binding).
// =============================================================================

export const MAX_IMAGE_SIDE = 8192;
export const MAX_IMAGE_PIXELS = 36_000_000;
/// The most colour-profile data kept. A profile can carry arbitrary text, so a
/// larger one is dropped (the picture then shows in the viewer's default colours).
export const MAX_ICC_BYTES = 16 * 1024;

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
// Kept: the image (IHDR, PLTE, IDAT, IEND), colour and transparency, physical size, animation. Every other
// chunk is dropped (text, time, EXIF, anything unknown). An unknown CRITICAL chunk is not an image this
// platform can vouch for: the file is refused. Bytes after IEND are dropped.
const PNG_KEEP = new Set(['IHDR', 'PLTE', 'IDAT', 'tRNS', 'gAMA', 'cHRM', 'sRGB', 'iCCP', 'sBIT', 'bKGD', 'hIST', 'pHYs', 'acTL', 'fcTL', 'fdAT']);

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
    } else if (name === 'IHDR') return null;
    if (name === 'IEND') { kept.push(b.subarray(o, end)); sawEnd = true; break; }
    if (PNG_KEEP.has(name)) {
      if (name !== 'iCCP' || length <= MAX_ICC_BYTES) kept.push(b.subarray(o, end));
    } else if (name[0] === name[0].toUpperCase()) return null;
    o = end;
  }
  // A file that stops before its end marker is not a whole image.
  return sawEnd && size ? { size, bytes: join(kept) } : null;
}

// ── JPEG ──
// Kept: the frame, tables and scans (SOFn, DHT, DAC, DQT, DRI, DNL, SOS and the scan data with its restart
// markers), JFIF (APP0 beginning "JFIF\0"), the ICC colour profile (APP2 beginning "ICC_PROFILE\0", capped) and
// Adobe's colour flag (APP14). Dropped: everything else, including EXIF and XMP (APP1), JFXX (APP0), MPF and
// FlashPix (APP2), Photoshop (APP13), comments, reserved markers, and everything after the first end-of-image,
// which is where a motion photo or an Ultra HDR file keeps its second image.
const isSof = (m) => m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc;

function keepJpegSegment(marker, payload, state) {
  if (isSof(marker) || marker === 0xc4 || marker === 0xcc || marker === 0xdb || marker === 0xdd || marker === 0xdc || marker === 0xda) return true;
  if (marker === 0xe0) return startsWith(payload, 'JFIF\0');
  if (marker === 0xe2) {
    if (!startsWith(payload, 'ICC_PROFILE\0')) return false;
    state.icc += payload.length;
    return state.icc <= MAX_ICC_BYTES;
  }
  if (marker === 0xee) return startsWith(payload, 'Adobe');
  return false;
}

function walkJpeg(b) {
  const kept = [b.subarray(0, 2)];
  const sofs = [];
  const state = { icc: 0 };
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
      sofs.push({ height: u16be(payload, 1), width: u16be(payload, 3) });
    }
    if (keepJpegSegment(marker, payload, state)) kept.push(b.subarray(o, end));
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
// Kept: the image (VP8, VP8L, ALPH, VP8X), animation (ANIM, ANMF with only its frame data) and a capped ICC profile
// (ICCP). Every other chunk is dropped (EXIF, XMP and anything unknown), as are bytes after the RIFF's own length.
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

// The frame chunks inside an ANMF payload (after its 16-byte header), kept only if they draw the frame.
function webpFrameChunks(b, start, end, sizes) {
  const out = [];
  let o = start;
  while (o + 8 <= end) {
    const name = ascii(b, o, 4);
    const size = u32le(b, o + 4);
    const stop = o + 8 + size + (size & 1);
    if (stop > end) return null;
    if (name === 'VP8 ' || name === 'VP8L' || name === 'ALPH') {
      if (name === 'VP8 ') { const s = vp8Size(b, o + 8, size); if (s) sizes.push(s); }
      if (name === 'VP8L') { const s = vp8lSize(b, o + 8, size); if (s) sizes.push(s); }
      out.push(webpChunk(name, b.subarray(o + 8, o + 8 + size)));
    }
    o = stop;
  }
  return out;
}

function walkWebp(b) {
  if (b.length < 20) return null;
  const riffEnd = u32le(b, 4) + 8;
  if (riffEnd > b.length || riffEnd < 20) return null;
  const kept = [];
  const sizes = [];
  let vp8xAt = -1;
  let iccKept = false;
  let o = 12;
  while (o + 8 <= riffEnd) {
    const name = ascii(b, o, 4);
    const size = u32le(b, o + 4);
    const stop = o + 8 + size + (size & 1);
    if (stop > riffEnd + (size & 1)) return null;
    const data = b.subarray(o + 8, o + 8 + size);
    if (name === 'VP8X') {
      if (size < 10) return null;
      sizes.push({ width: u24le(b, o + 12) + 1, height: u24le(b, o + 15) + 1 });
      vp8xAt = kept.length;
      kept.push(webpChunk(name, data));
    } else if (name === 'VP8 ') {
      const s = vp8Size(b, o + 8, size);
      if (s) sizes.push(s);
      kept.push(webpChunk(name, data));
    } else if (name === 'VP8L') {
      const s = vp8lSize(b, o + 8, size);
      if (s) sizes.push(s);
      kept.push(webpChunk(name, data));
    } else if (name === 'ALPH' || name === 'ANIM') {
      kept.push(webpChunk(name, data));
    } else if (name === 'ICCP') {
      if (size <= MAX_ICC_BYTES) { kept.push(webpChunk(name, data)); iccKept = true; }
    } else if (name === 'ANMF') {
      if (size < 16) return null;
      sizes.push({ width: u24le(b, o + 14) + 1, height: u24le(b, o + 17) + 1 });
      const frames = webpFrameChunks(b, o + 24, o + 8 + size, sizes);
      if (!frames) return null;
      kept.push(webpChunk(name, join([data.subarray(0, 16), ...frames])));
    }
    o = stop;
  }
  if (!sizes.length) return null;
  // The extended header must stop promising metadata (EXIF 0x08, XMP 0x04) and a profile it no longer holds (ICC 0x20).
  if (vp8xAt >= 0) kept[vp8xAt][8] &= ~(0x08 | 0x04 | (iccKept ? 0 : 0x20));
  const body = join([new TextEncoder().encode('WEBP'), ...kept]);
  const bytes = join([new TextEncoder().encode('RIFF'), new Uint8Array([body.length & 0xff, (body.length >>> 8) & 0xff, (body.length >>> 16) & 0xff, (body.length >>> 24) & 0xff]), body]);
  return { sizes, bytes };
}

// What the bytes are, whatever their size: `{ type, mime, ext, width, height, bytes }`, or null when they are not
// a PNG, JPEG or WebP whose structure can be walked. `width` and `height` are the largest the file states anywhere.
function readImage(input) {
  const b = toBytes(input);
  const type = typeOf(b);
  if (!type) return null;
  let sizes;
  let bytes;
  if (type === 'png') {
    const walked = walkPng(b);
    if (!walked) return null;
    sizes = [walked.size];
    bytes = walked.bytes;
  } else if (type === 'jpeg') {
    const walked = walkJpeg(b);
    if (!walked || !walked.sofs.length) return null;
    sizes = walked.sofs;
    bytes = walked.bytes;
  } else {
    const walked = walkWebp(b);
    if (!walked) return null;
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
  return info && inBounds(info) ? publicInfo(info) : null;
}

/// Why these bytes cannot be stored as a user image, in words fit for the
/// uploader, or null when they can.
export function imageProblem(input) {
  const info = readImage(input);
  if (!info) return 'The image must be a PNG, JPEG or WebP file';
  if (!inBounds(info))
    return `The image is ${info.width}x${info.height}; the most accepted is ${MAX_IMAGE_SIDE} pixels on a side and ${MAX_IMAGE_PIXELS / 1_000_000} megapixels`;
  return null;
}

/// The same image rebuilt from the segments that draw it (see the allowlists above):
/// no EXIF or GPS, XMP, text, comments, vendor segments, appended second image or
/// trailing bytes. The pixels are not touched. Throws when the structure cannot be
/// walked: a file that cannot be read is refused, never stored as it came.
export function stripMetadata(input) {
  const info = readImage(input);
  if (!info) throw new Error('The image file could not be read');
  return info.bytes;
}

/// Hex SHA-256 of bytes: what R2 verifies when it is passed as `sha256`, and
/// what an approval binds.
export async function sha256Hex(input) {
  const digest = await crypto.subtle.digest('SHA-256', toBytes(input));
  return [...new Uint8Array(digest)].map((x) => x.toString(16).padStart(2, '0')).join('');
}

/// Check, clean and fingerprint an uploaded image: `{ ok: true, bytes, info,
/// sha256 }` for what to store, or `{ ok: false, error }` for the uploader.
export async function prepareUserImage(input) {
  const problem = imageProblem(input);
  if (problem) return { ok: false, error: problem };
  const info = readImage(input);
  return { ok: true, bytes: info.bytes, info: publicInfo(info), sha256: await sha256Hex(info.bytes) };
}
