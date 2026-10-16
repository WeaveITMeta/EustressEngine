// Loads the real Worker (src/index.js) in plain Node, with fake storage, so a
// test can drive `worker.fetch(request, env, ctx)` the way Cloudflare does.
//
// index.js is a `.js` file and the package has no `"type": "module"`, so Node
// will not import it in place. The source tree is copied to a folder that has
// one, and imported from there. The copy is removed when the test exits. It
// sits in the package's own folder (ignored by git), so a package the source
// imports (the Anthropic SDK) resolves from the package's node_modules the
// way it does in place, with no link to make or to remove.

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';

export async function loadWorker() {
  const src = path.resolve(import.meta.dirname, '..', '..', 'src');
  const tmp = fs.mkdtempSync(path.join(path.resolve(src, '..'), '.test-worker-'));
  process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));
  fs.cpSync(src, tmp, { recursive: true });
  fs.writeFileSync(path.join(tmp, 'package.json'), '{"type":"module"}');
  // `cloudflare:email` exists only inside the Workers runtime. A stand-in with
  // the same shape lets a test hand the Worker an `env.EMAIL` and read back the
  // message it built.
  fs.writeFileSync(path.join(tmp, 'cloudflare_email_stub.mjs'), 'export class EmailMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }\n');
  const entry = path.join(tmp, 'index.js');
  fs.writeFileSync(entry, fs.readFileSync(entry, 'utf8').replaceAll("'cloudflare:email'", "'./cloudflare_email_stub.mjs'"));
  const mod = await import(pathToFileURL(path.join(tmp, 'index.js')).href);
  return mod.default;
}

export const JWT_SECRET = 'test-secret-not-real';

const b64url = (b) => Buffer.from(b).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

/// An HS256 token for `sub`, valid for an hour, as verifyJwt expects.
export function mintToken(sub) {
  const now = Math.floor(Date.now() / 1000);
  const h = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const p = b64url(JSON.stringify({ sub, iat: now, exp: now + 3600 }));
  const sig = b64url(crypto.createHmac('sha256', JWT_SECRET).update(`${h}.${p}`).digest());
  return `${h}.${p}.${sig}`;
}

/// A KV namespace in memory: get, put, delete, list.
export function fakeKv() {
  const store = new Map();
  return {
    store,
    async get(key) { return store.has(key) ? store.get(key) : null; },
    async put(key, value) { store.set(key, typeof value === 'string' ? value : String(value)); },
    async delete(key) { store.delete(key); },
    async list({ prefix = '', limit = 1000 } = {}) {
      const keys = [...store.keys()].filter((k) => k.startsWith(prefix)).slice(0, limit).map((name) => ({ name }));
      return { keys, list_complete: true };
    },
  };
}

/// The bytes of a SHA-256 an object was put with. R2 takes it as hex or as bytes and returns it from get and head as an ArrayBuffer.
function sha256Bytes(value) {
  if (typeof value === 'string') return /^[0-9a-f]{64}$/i.test(value) ? Uint8Array.from(value.match(/../g).map((pair) => parseInt(pair, 16))).buffer : undefined;
  if (value instanceof ArrayBuffer) return value.byteLength === 32 ? value.slice(0) : undefined;
  if (ArrayBuffer.isView(value)) return value.byteLength === 32 ? new Uint8Array(value.buffer, value.byteOffset, value.byteLength).slice().buffer : undefined;
  return undefined;
}

/// An R2 bucket in memory. Objects are `{ body: Uint8Array, meta, version }`. A put given `sha256` has it back from get and head as
/// `checksums.sha256` (an ArrayBuffer, as R2 returns it), so a binding identifies the object by that digest; an object put without one
/// has no checksum and is identified by its etag. `version` is a string that changes with every put.
export function fakeBucket() {
  const objects = new Map();
  let versions = 0;
  const toBytes = async (value) => {
    if (typeof value === 'string') return new TextEncoder().encode(value);
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  const view = (key, o) => ({
    key,
    size: o.body.byteLength,
    etag: `etag-${key}`,
    version: o.version ?? '0',
    checksums: o.meta?.sha256 && sha256Bytes(o.meta.sha256) ? { sha256: sha256Bytes(o.meta.sha256) } : {},
    httpMetadata: o.meta?.httpMetadata,
    customMetadata: o.meta?.customMetadata,
    body: new Response(o.body).body,
    arrayBuffer: async () => o.body.buffer.slice(o.body.byteOffset, o.body.byteOffset + o.body.byteLength),
    text: async () => new TextDecoder().decode(o.body),
    json: async () => JSON.parse(new TextDecoder().decode(o.body)),
  });
  return {
    objects,
    async put(key, value, meta) {
      // The bytes are read first, so the precondition and the write below run with no gap between them (R2's own are atomic).
      const bytes = await toBytes(value);
      // R2's put preconditions: create-if-absent (`etagDoesNotMatch: '*'`) and replace-if-unchanged (`etagMatches`). A put that fails one writes nothing and returns null.
      const gate = meta?.onlyIf;
      if (gate?.etagDoesNotMatch === '*' && objects.has(key)) return null;
      if (gate?.etagMatches !== undefined && (!objects.has(key) || `etag-${key}` !== gate.etagMatches)) return null;
      objects.set(key, { body: bytes, meta, version: String(++versions) }); return { key, size: objects.get(key).body.byteLength, etag: `etag-${key}`, version: objects.get(key).version }; },
    async get(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async head(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async delete(key) { objects.delete(key); },
    async list({ prefix = '', include = [] } = {}) {
      return {
        objects: [...objects.keys()].filter((k) => k.startsWith(prefix)).map((key) => ({ key, size: objects.get(key).body.byteLength, ...(include.includes('customMetadata') ? { customMetadata: objects.get(key).meta?.customMetadata || {} } : {}) })),
        truncated: false,
      };
    },
    async createMultipartUpload(key) { return { uploadId: `upload-${key}`, key }; },
    resumeMultipartUpload(key, uploadId) {
      return {
        uploadId,
        async uploadPart(partNumber, body) { return { partNumber, etag: `part-${partNumber}`, size: (await toBytes(body)).byteLength }; },
        async complete() { objects.set(key, { body: new Uint8Array([1, 2, 3]), meta: {}, version: String(++versions) }); return { key, size: 3, etag: `etag-${key}` }; },
      };
    },
  };
}

/// Durable Object storage in memory, with an alarm, values kept as clones.
export function fakeDoStorage() {
  const map = new Map();
  let alarm = null;
  const clone = (v) => (v === undefined ? undefined : structuredClone(v));
  return {
    map,
    async get(key) {
      if (Array.isArray(key)) return new Map(key.filter((k) => map.has(k)).map((k) => [k, clone(map.get(k))]));
      return clone(map.get(key));
    },
    // Both forms the platform has: put(key, value) and put({ key: value, ... }), the second all at once.
    async put(key, value) {
      if (key && typeof key === 'object' && value === undefined) { for (const [k, v] of Object.entries(key)) map.set(k, clone(v)); return; }
      map.set(key, clone(value));
    },
    async delete(key) { return Array.isArray(key) ? key.filter((k) => map.delete(k)).length : map.delete(key); },
    async deleteAll() { map.clear(); },
    async list({ prefix = '', limit, startAfter } = {}) {
      const out = new Map();
      for (const [k, v] of [...map.entries()].sort(([a], [b]) => (a < b ? -1 : 1))) {
        if (!k.startsWith(prefix)) continue;
        if (startAfter !== undefined && k <= startAfter) continue;
        out.set(k, clone(v));
        if (limit && out.size >= limit) break;
      }
      return out;
    },
    async getAlarm() { return alarm; },
    async setAlarm(at) { alarm = typeof at === 'number' ? at : at.getTime(); },
    async deleteAlarm() { alarm = null; },
    get alarm() { return alarm; },
  };
}

/// A Durable Object namespace that routes stub.fetch into one `Class` instance
/// per name, so the real class runs against in-memory storage.
export function fakeDoNamespace(Class, env = {}) {
  const instances = new Map();
  const queues = new Map();
  return {
    instances,
    idFromName: (name) => name,
    get(id) {
      if (!instances.has(id)) instances.set(id, new Class({ storage: fakeDoStorage(), blockConcurrencyWhile: (fn) => fn() }, env));
      const obj = instances.get(id);
      // One request at a time per object, as Cloudflare runs a Durable Object.
      return {
        fetch: (url, init) => {
          const run = (queues.get(id) || Promise.resolve()).then(() => obj.fetch(new Request(url, init)));
          queues.set(id, run.catch(() => {}));
          return run;
        },
      };
    },
  };
}

/// The bindings a publish route touches, all in memory.
export function fakeEnv() {
  return {
    JWT_SECRET,
    FORK_ID: 'eustress.dev',
    USERS: fakeKv(),
    SOCIAL: fakeKv(),
    INVENTORY: fakeKv(),
    AUDIT_LOG: fakeKv(),
    SCREENING: fakeKv(),
    TELEMETRY: fakeKv(),
    SCENES: fakeBucket(),
  };
}

export const ctx = { waitUntil() {}, passThroughOnException() {} };

/// A PNG with a real header (default 64x36) that the image routes accept: signature,
/// IHDR with the size, a tiny IDAT, IEND. `seed` makes the bytes differ.
export function testPng(width = 64, height = 36, seed = 0) {
  const chunk = (name, data) => {
    const out = new Uint8Array(12 + data.length);
    new DataView(out.buffer).setUint32(0, data.length);
    out.set(new TextEncoder().encode(name), 4);
    out.set(data, 8);
    return out;
  };
  const ihdr = new Uint8Array(13);
  const view = new DataView(ihdr.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const parts = [new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', new Uint8Array([seed & 255, 1, 2, 3])), chunk('IEND', new Uint8Array(0))];
  const bytes = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { bytes.set(p, at); at += p.length; }
  return bytes;
}

/// A WebP (lossless, VP8L) with a real header (default 64x36) that the image routes accept.
export function testWebp(width = 64, height = 36) {
  const le32 = (n) => new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255]);
  const text = (s) => new TextEncoder().encode(s);
  const bits = ((width - 1) | ((height - 1) << 14)) >>> 0;
  const vp8l = new Uint8Array([0x2f, ...le32(bits), 0, 0, 0]);
  const chunk = new Uint8Array([...text('VP8L'), ...le32(vp8l.length), ...vp8l]);
  const body = new Uint8Array([...text('WEBP'), ...chunk]);
  return new Uint8Array([...text('RIFF'), ...le32(body.length), ...body]);
}
