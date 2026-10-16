// The read-only finder for approvals voided by a re-commit of identical bytes (scripts/voided_approvals.mjs). Its
// classification is pure, so it is tested on fixtures that hold real bytes; its commands are checked to be reads only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { classify, kvGetArgs, kvListArgs, r2GetArgs, ENVIRONMENTS, parseArgs, assertReadOnly, summarizeAll } from '../scripts/voided_approvals.mjs';
import { contentFingerprint } from '../src/moderation.mjs';

const SIM = 'a1b2c3d4-0000-4000-8000-000000000011';
const HEX = '7e5f00a262f42dea7d8dd3d15c20b65b'; // R2 versions are hex strings, not counters
const WORLD = Buffer.from('{"format":"echk","universe":"Box Head","spaces":[]}');
const SOURCE = Buffer.from('{"format":"echk","universe":"Box Head source","spaces":[]}');
const sha = b => createHash('sha256').update(b).digest('hex');
const md5 = b => createHash('md5').update(b).digest('hex');
const SHA = sha(WORLD);
const SRC_SHA = sha(SOURCE);
const KEY = `universes/${SIM}/manifests/${SHA}.json`;
const SRC_KEY = `universes/${SIM}/manifests/${SRC_SHA}.json`;
const APPROVED = '2026-10-05T10:00:00.000Z';
const COMMIT_AFTER = '2026-10-06T09:00:00.000Z';
const COMMIT_BEFORE = '2026-10-04T09:00:00.000Z';

// A world listing as a commit leaves it: r2_key names the manifest it serves.
const listing = (over = {}) => ({
  id: SIM, name: 'Box Head', moderation: { status: 'approved' }, r2_key: KEY,
  world: { manifest_key: KEY, manifest_sha256: SHA, committed_at: COMMIT_AFTER }, ...over,
});
const binding = (over = {}) => ({ v: 1, content: `etag:${md5(WORLD)}:v${HEX}`, source: null, at: APPROVED, ...over });
const caseFor = sim => ({ sim_id: SIM, content_fp: contentFingerprint(sim) });

test('a listing whose stored bytes are the bound ones, committed after its approval, is a candidate', () => {
  const sim = listing();
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding(), worldBytes: WORLD });
  assert.deepEqual(r.reasons, []);
  assert.equal(r.candidates.length, 1);
  assert.deepEqual([r.candidates[0].kind, r.candidates[0].binding_content, r.candidates[0].bytes_etag], ['world', `etag:${md5(WORLD)}:v${HEX}`, md5(WORLD)]);
});

test('a commit before the approval cannot have voided it', () => {
  const sim = listing({ world: { manifest_key: KEY, manifest_sha256: SHA, committed_at: COMMIT_BEFORE } });
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding(), worldBytes: WORLD });
  assert.deepEqual(r, { candidates: [], reasons: ['world_no_commit_after_approval'] });
});

test('a binding by SHA-256 is not voided by a re-put, so it is not a candidate', () => {
  const sim = listing();
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: `sha256:${SHA}` }), worldBytes: WORLD });
  assert.deepEqual(r, { candidates: [], reasons: ['world_binding_sha256'] });
});

test('a listing whose fingerprint moved since the review is not a candidate, and a case with none is not either', () => {
  const sim = listing();
  const moved = classify({ id: SIM, sim, caseRec: { content_fp: contentFingerprint({ ...sim, pak_etag: 'another' }) }, binding: binding(), worldBytes: WORLD });
  assert.deepEqual(moved, { candidates: [], reasons: ['world_fingerprint_changed'] });
  const none = classify({ id: SIM, sim, caseRec: null, binding: binding(), worldBytes: WORLD });
  assert.deepEqual(none, { candidates: [], reasons: ['world_no_review_fingerprint'] });
});

test('stored bytes that are not the object its key names are not a candidate, and a missing object is reported as missing', () => {
  const sim = listing();
  const changed = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding(), worldBytes: Buffer.from('a different manifest') });
  assert.deepEqual(changed, { candidates: [], reasons: ['world_bytes_not_its_key'] });
  const missing = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding(), worldBytes: null });
  assert.deepEqual(missing, { candidates: [], reasons: ['world_missing_bytes'] });
});

test('a listing that is not approved, is unbound, or has a revoked binding is not a candidate', () => {
  const sim = listing();
  assert.deepEqual(classify({ id: SIM, sim: listing({ moderation: { status: 'pending' } }), caseRec: caseFor(sim), binding: binding(), worldBytes: WORLD }).reasons, ['not_approved']);
  assert.deepEqual(classify({ id: SIM, sim, caseRec: caseFor(sim), binding: null, worldBytes: WORLD }).reasons, ['unbound']);
  assert.deepEqual(classify({ id: SIM, sim, caseRec: caseFor(sim), binding: { revoked: true }, worldBytes: WORLD }).reasons, ['revoked']);
  assert.deepEqual(classify({ id: SIM, sim: null, caseRec: null, binding: binding() }).reasons, ['no_listing']);
});

test('the source world is a candidate on its own bytes, key and binding, and a world with both is two candidates', () => {
  const sim = listing({ world: { manifest_key: KEY, manifest_sha256: SHA, committed_at: COMMIT_BEFORE, source_manifest_key: SRC_KEY, source_manifest_sha256: SRC_SHA, source_committed_at: COMMIT_AFTER } });
  const b = binding({ content: `sha256:${SHA}`, source: `etag:${md5(SOURCE)}:v${HEX}` });
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: b, worldBytes: WORLD, sourceBytes: SOURCE });
  assert.deepEqual(r.reasons, ['world_binding_sha256'], 'the world was bound by SHA-256, so it is not refused by its etag');
  assert.deepEqual(r.candidates.map(c => c.kind), ['source']);
  assert.equal(r.candidates[0].binding_content, `etag:${md5(SOURCE)}:v${HEX}`);

  const both = classify({ id: SIM, sim: listing({ world: { manifest_key: KEY, manifest_sha256: SHA, committed_at: COMMIT_AFTER, source_manifest_key: SRC_KEY, source_manifest_sha256: SRC_SHA, source_committed_at: COMMIT_AFTER } }), caseRec: caseFor(listing()), binding: binding({ source: `etag:${md5(SOURCE)}:v${HEX}` }), worldBytes: WORLD, sourceBytes: SOURCE });
  assert.deepEqual(both.candidates.map(c => c.kind), ['world', 'source']);
});

test('every call the finder makes is a read, for either environment, and each environment is named by its own flags', () => {
  const { staging, production } = ENVIRONMENTS;
  for (const cfg of [staging, production]) {
    const calls = [kvGetArgs(cfg, 'sim:x'), kvListArgs(cfg, 'sim:'), kvListArgs(cfg, 'modq:approved:'), r2GetArgs(cfg, 'locks/approved/x.json')];
    for (const args of calls) {
      assert.doesNotThrow(() => assertReadOnly(args), args.join(' '));
      assert.equal(args.includes('--remote'), true, 'the remote namespace, not a local copy');
      assert.equal(args.some(a => /^(put|delete|bulk|create|upload|update|rename)$/.test(a)), false, 'no write verb');
    }
  }
  assert.deepEqual([kvGetArgs(staging, 'k'), r2GetArgs(staging, 'k')].map(a => a.slice(0, 3).join(' ')), ['kv key get', 'r2 object get']);
  // Staging is named by its flag. Production is the top level, which takes no --env, so it is never reached by a missing flag.
  assert.equal(kvGetArgs(staging, 'k')[kvGetArgs(staging, 'k').indexOf('--env') + 1], 'staging');
  assert.equal(kvGetArgs(production, 'k').includes('--env'), false, 'production has no --env flag');
  assert.equal(r2GetArgs(staging, 'k')[3], 'eustress-simulations-staging/k');
  assert.equal(r2GetArgs(production, 'k')[3], 'eustress-simulations/k');
});

test('the write guard refuses every write, whatever the environment', () => {
  for (const cfg of [ENVIRONMENTS.staging, ENVIRONMENTS.production]) {
    assert.throws(() => assertReadOnly(['kv', 'key', 'put', 'sim:x', 'v', '--binding', 'SOCIAL', ...cfg.wrangler, '--remote']), /non-read/);
    assert.throws(() => assertReadOnly(['kv', 'key', 'delete', 'sim:x', '--binding', 'SOCIAL', ...cfg.wrangler, '--remote']), /non-read/);
    assert.throws(() => assertReadOnly(['kv', 'bulk', 'put', 'file.json', '--binding', 'SOCIAL', ...cfg.wrangler, '--remote']), /non-read/);
    assert.throws(() => assertReadOnly(['r2', 'object', 'put', `${cfg.scenesBucket}/k`, ...cfg.wrangler, '--remote']), /non-read/);
    assert.throws(() => assertReadOnly(['r2', 'object', 'delete', `${cfg.scenesBucket}/k`, ...cfg.wrangler, '--remote']), /non-read/);
  }
  // A read whose key happens to contain a write word is still a read; only the verb decides.
  assert.doesNotThrow(() => assertReadOnly(['kv', 'key', 'get', 'put', '--binding', 'SOCIAL', '--env', 'staging', '--remote']));
});

test('no flag reads staging; production needs --env=production, and anything else is refused', () => {
  assert.deepEqual(parseArgs([]), { env: 'staging', all: false }, 'the default is staging');
  assert.deepEqual(parseArgs(['--all']), { env: 'staging', all: true });
  assert.deepEqual(parseArgs(['--env=production', '--all']), { env: 'production', all: true });
  assert.throws(() => parseArgs(['--env=prod']), /staging or production/, 'no aliases');
  assert.throws(() => parseArgs(['--env', 'production']), /unknown argument/, 'the flag takes its value with =');
  assert.throws(() => parseArgs(['--write']), /unknown argument/);
});

test('the all-listings summary names stale listings, approvals with no binding and revoked ones, and counts the rest', () => {
  const pakKey = `universes/${SIM}/universe.pak`;
  const healthy = { id: 'h', sim: { name: 'Healthy', format: 'echk', r2_key: KEY, world: { manifest_key: KEY }, moderation: { status: 'approved' } }, binding: { content: 'x' } };
  const stale = { id: 's', sim: { name: 'Stale', format: 'echk', r2_key: pakKey, world: { manifest_key: KEY }, moderation: { status: 'pending' } } };
  const pak = { id: 'p', sim: { name: 'Pak', format: 'pak', r2_key: pakKey, world: { manifest_key: KEY }, moderation: { status: 'approved' } }, binding: { content: 'y' } };
  const unbound = { id: 'u', sim: { name: 'Unbound', moderation: { status: 'approved' } }, binding: null };
  const revoked = { id: 'r', sim: { name: 'Revoked', moderation: { status: 'approved' } }, binding: { revoked: true } };
  const r = summarizeAll([healthy, stale, pak, unbound, revoked, { id: 'x', sim: null }], 2);
  assert.equal(r.listings, 5);
  assert.equal(r.unreadable, 3, 'one unreadable record and two keys that are not listings');
  assert.deepEqual(r.stale_current_content.map(row => row.sim_id), ['s']);
  assert.equal(r.pak_with_world_record, 1, 'only the .pak listing that keeps a world record counts here; the stale one is echk');
  assert.equal(r.approved.total, 4);
  assert.equal(r.approved.bound, 2);
  assert.deepEqual(r.approved.unbound.map(row => row.sim_id), ['u']);
  assert.deepEqual(r.approved.revoked.map(row => row.sim_id), ['r']);
  assert.deepEqual(r.formats, { echk: 2, pak: 1, unset: 2 });
});

test('a listing approved on its .pak, with a stale world record, is not judged as a manifest voiding', () => {
  // A .pak publish moves r2_key to universe.pak (checksummed by SHA-256) and leaves the world record in place.
  const pakKey = `universes/${SIM}/universe.pak`;
  const sim = listing({ r2_key: pakKey });
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: `sha256:${sha(Buffer.from('pak bytes'))}` }), worldBytes: WORLD });
  assert.deepEqual(r, { candidates: [], reasons: ['world_r2_key_not_manifest'] });
});

test('a binding with no content, or one of an unknown form, is reported as other rather than as a SHA-256 binding', () => {
  const sim = listing();
  const none = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: null }), worldBytes: WORLD });
  assert.deepEqual(none, { candidates: [], reasons: ['world_binding_other'] });
  const odd = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: 'etag:not-hex:v1' }), worldBytes: WORLD });
  assert.deepEqual(odd, { candidates: [], reasons: ['world_binding_other'] });
});

test('R2\'s hex version is read as an etag binding, as the staging listing\'s is', () => {
  // The identity R2 gave a staging listing: an etag, then R2's hex version (not a counter). The bytes here are not that etag's
  // bytes, so the finder gets as far as the bytes check and stops there, which proves the identity was read.
  const exact = 'etag:8eedae7eb9bab538b36ae09b5f243981:v7e5f00a262f42dea7d8dd3d15c20b65b';
  const sim = listing();
  const r = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: exact }), worldBytes: WORLD });
  assert.deepEqual(r, { candidates: [], reasons: ['world_bytes_differ_from_bound'] });
  // An uppercase etag names the same MD5; the bytes that match it are a candidate.
  const upper = classify({ id: SIM, sim, caseRec: caseFor(sim), binding: binding({ content: `etag:${md5(WORLD).toUpperCase()}:v${HEX}` }), worldBytes: WORLD });
  assert.equal(upper.candidates.length, 1);
});
