// Loads the real Worker (src/index.js) in plain Node, with fake storage, so a
// test can drive `worker.fetch(request, env, ctx)` the way Cloudflare does.
//
// index.js is a `.js` file and the package has no `"type": "module"`, so Node
// will not import it in place. The source tree is copied to a temp folder that
// has one, and imported from there. The copy is removed when the test exits.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';

export async function loadWorker() {
  const src = path.resolve(import.meta.dirname, '..', '..', 'src');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'eustress-worker-'));
  process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));
  fs.cpSync(src, tmp, { recursive: true });
  fs.writeFileSync(path.join(tmp, 'package.json'), '{"type":"module"}');
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

/// An R2 bucket in memory. Objects are `{ body: Uint8Array, meta }`.
export function fakeBucket() {
  const objects = new Map();
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
    httpMetadata: o.meta?.httpMetadata,
    customMetadata: o.meta?.customMetadata,
    body: new Response(o.body).body,
    arrayBuffer: async () => o.body.buffer.slice(o.body.byteOffset, o.body.byteOffset + o.body.byteLength),
    text: async () => new TextDecoder().decode(o.body),
    json: async () => JSON.parse(new TextDecoder().decode(o.body)),
  });
  return {
    objects,
    async put(key, value, meta) { objects.set(key, { body: await toBytes(value), meta }); return { key, size: objects.get(key).body.byteLength, etag: `etag-${key}` }; },
    async get(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async head(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async delete(key) { objects.delete(key); },
    async list({ prefix = '' } = {}) { return { objects: [...objects.keys()].filter((k) => k.startsWith(prefix)).map((key) => ({ key })), truncated: false }; },
    async createMultipartUpload(key) { return { uploadId: `upload-${key}`, key }; },
    resumeMultipartUpload(key, uploadId) {
      return {
        uploadId,
        async uploadPart(partNumber, body) { return { partNumber, etag: `part-${partNumber}`, size: (await toBytes(body)).byteLength }; },
        async complete() { objects.set(key, { body: new Uint8Array([1, 2, 3]), meta: {} }); return { key, size: 3, etag: `etag-${key}` }; },
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
    async get(key) { return clone(map.get(key)); },
    async put(key, value) { map.set(key, clone(value)); },
    async delete(key) { return Array.isArray(key) ? key.filter((k) => map.delete(k)).length : map.delete(key); },
    async deleteAll() { map.clear(); },
    async list({ prefix = '', limit } = {}) {
      const out = new Map();
      for (const [k, v] of [...map.entries()].sort(([a], [b]) => (a < b ? -1 : 1))) {
        if (!k.startsWith(prefix)) continue;
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
      if (!instances.has(id)) instances.set(id, new Class({ storage: fakeDoStorage() }, env));
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
