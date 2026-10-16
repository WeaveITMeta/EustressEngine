// moderation_bind on a world listing. A bind is for the bytes that were reviewed (Security's ruling, 2026-10-07): the case's
// content fingerprint must equal the listing's current one, and the stored manifest must hash to the SHA-256 in its key.
// Every bind is audited with the binding it replaced and the manifest it kept. This is how a voided approval is repaired
// (a re-commit of identical bytes moved the object's R2 version and voided its binding; the bytes did not change).

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { makeToolExecutor, contentFingerprint } from '../src/moderation.mjs';

const SIM = 'a1b2c3d4-0000-4000-8000-000000000009';
const MANIFEST = '{"format":"echk","universe":"Box Head","spaces":[]}';
const SHA = createHash('sha256').update(MANIFEST).digest('hex');
const KEY = `universes/${SIM}/manifests/${SHA}.json`;
const RATIONALE = 'A person read the manifest and its fingerprint against the review and they are the same world.';

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '' } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).map(name => ({ name })), list_complete: true }),
  };
}

// R2 with the calls moderation makes. A version moves on every put, as R2's does, so a re-put of identical bytes is visible.
function r2() {
  const m = new Map();
  let versions = 0;
  const view = (key, rec) => ({
    key, size: rec.bytes.byteLength, etag: rec.etag, version: rec.version, checksums: rec.checksums,
    arrayBuffer: async () => rec.bytes.slice(0), text: async () => new TextDecoder().decode(rec.bytes),
  });
  return {
    m,
    async put(key, body, opts = {}) {
      if (opts.onlyIf?.etagDoesNotMatch === '*' && m.has(key)) return null;
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body) : new Uint8Array(body);
      const rec = { bytes: bytes.slice(), etag: createHash('md5').update(bytes).digest('hex'), version: String(++versions), checksums: opts.sha256 ? { sha256: opts.sha256 } : {} };
      m.set(key, rec);
      return view(key, rec);
    },
    async get(key) { return m.has(key) ? view(key, m.get(key)) : null; },
    async head(key) { return m.has(key) ? view(key, m.get(key)) : null; },
    async delete(key) { m.delete(key); },
    async list({ prefix = '' } = {}) { return { objects: [...m.keys()].filter(k => k.startsWith(prefix)).map(key => ({ key })) }; },
  };
}

function environment() {
  return { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2() };
}

// A world listing that was approved on the manifest it names. `fingerprint` overrides what the case recorded at review.
// `legacy` stores the manifest the way commits did before its SHA-256 was passed (identity by etag and version).
async function seedWorld(env, { fingerprint, legacy = false } = {}) {
  const sim = {
    id: SIM, name: 'Box Head', author_id: 'user-1', is_public: true, moderation: { status: 'approved', rating: 'all_ages' },
    r2_key: KEY, world: { manifest_key: KEY, manifest_sha256: SHA, committed_at: '2026-10-06T10:00:00.000Z' }, version: 1,
  };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SCENES.put(KEY, MANIFEST, legacy ? {} : { sha256: SHA });
  const content_fp = fingerprint === undefined ? contentFingerprint(sim) : fingerprint;
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'user-1', status: 'approved', history: [], content_fp }));
  return env;
}

// The admin's two steps: look (no `expected`, nothing written), then bind exactly what was shown.
function binder(env, audits) {
  const deps = { auditLog: async (_env, action, actorId, target, details) => { audits.push({ action, actorId, target, details }); }, json: v => v };
  const run = makeToolExecutor(env, deps, { actor: 'admin', actorId: 'admin-1' });
  return async function bind() {
    const shown = await run('moderation_bind', { sim_id: SIM, rationale: RATIONALE });
    assert.equal(shown.ok && shown.bound, false, 'the look writes nothing');
    return run('moderation_bind', { sim_id: SIM, rationale: RATIONALE, expected: shown.objects });
  };
}

const approvalOf = env => JSON.parse(new TextDecoder().decode(env.SCENES.m.get(`locks/approved/${SIM}.json`).bytes));

test('a world listing binds when its case fingerprint is the listing\'s and its manifest hashes to its key, and the audit names the manifest', async () => {
  const env = await seedWorld(environment());
  const audits = [];
  const bound = await binder(env, audits)();
  assert.equal(bound.ok, true, JSON.stringify(bound));
  assert.equal(approvalOf(env).content, `sha256:${SHA}`);
  assert.deepEqual(audits.map(a => [a.action, a.details.manifest_sha256, a.details.binding_before]), [['moderation_bind', SHA, null]]);
});

test('a listing changed since its review is refused: it needs a review, not a bind', async () => {
  const env = await seedWorld(environment(), { fingerprint: 'an old fingerprint' });
  const bound = await binder(env, [])();
  assert.equal(bound.ok, false);
  assert.match(bound.error, /needs a review, not a bind/);
  assert.equal(env.SCENES.m.has(`locks/approved/${SIM}.json`), false, 'nothing was bound');
});

test('a case with no review fingerprint is refused the same way', async () => {
  const env = await seedWorld(environment(), { fingerprint: null });
  const bound = await binder(env, [])();
  assert.equal(bound.ok, false);
  assert.match(bound.error, /needs a review, not a bind/);
});

test('a stored manifest that does not hash to the SHA-256 in its key is refused', async () => {
  const env = await seedWorld(environment());
  await env.SCENES.put(KEY, '{"format":"echk","universe":"Tampered","spaces":[]}', { sha256: SHA });
  const bound = await binder(env, [])();
  assert.equal(bound.ok, false);
  assert.match(bound.error, /does not hash to the SHA-256 in its key/);
});

test('a source world whose stored bytes are not the ones its key names is refused, though the player world is sound', async () => {
  const SOURCE = '{"format":"echk","universe":"Box Head source","spaces":[]}';
  const SRC_SHA = createHash('sha256').update(SOURCE).digest('hex');
  const SRC_KEY = `universes/${SIM}/manifests/${SRC_SHA}.json`;
  const env = await seedWorld(environment());
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  sim.world = { ...sim.world, source_manifest_key: SRC_KEY, source_manifest_sha256: SRC_SHA, source_committed_at: '2026-10-06T10:00:00.000Z' };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SCENES.put(SRC_KEY, SOURCE, { sha256: SRC_SHA });
  assert.equal((await binder(env, [])()).ok, true, 'sound source and world bind');
  await env.SCENES.put(SRC_KEY, '{"format":"echk","universe":"Tampered","spaces":[]}', { sha256: SRC_SHA });
  const bound = await binder(env, [])();
  assert.equal(bound.ok, false);
  assert.match(bound.error, /source manifest does not hash to the SHA-256 in its key/);
});

test('re-binding a voided approval to identical bytes is allowed, and the audit names the binding it replaced', async () => {
  const env = await seedWorld(environment(), { legacy: true });
  const audits = [];
  const bind = binder(env, audits);
  assert.equal((await bind()).ok, true);
  const before = approvalOf(env).content;
  assert.match(before, /^etag:[0-9a-f]{32}:v\d+$/, 'a legacy object is bound by etag and version');
  // The re-commit of identical bytes (before the fix): a new version of the same bytes.
  await env.SCENES.put(KEY, MANIFEST, {});
  const repaired = await bind();
  assert.equal(repaired.ok, true, JSON.stringify(repaired));
  assert.notEqual(approvalOf(env).content, before, 'the binding now names the current version');
  assert.deepEqual(audits.map(a => [a.details.binding_before, a.details.binding_after, a.details.manifest_sha256]), [[null, before, SHA], [before, approvalOf(env).content, SHA]]);
});
