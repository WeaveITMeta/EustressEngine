#!/usr/bin/env node
// READ-ONLY. Two reports on the listings of one environment. Every call this script makes is a read.
//
//   node scripts/voided_approvals.mjs > voided.json
//       Approved listings whose approval may have been voided by a re-commit of identical bytes (the J1 check).
//   node scripts/voided_approvals.mjs --all > all.json
//       Every listing: stale current content (format echk, r2_key on a .pak), and approved listings with no binding or a revoked one.
//
// --env=staging (the default) or --env=production. Production is read only when it is named: the flag is the only way to reach
// it, and running it is McKale's call. This file does not run either environment by itself.
//
// What the approved-only report can and cannot see. The bindings live in R2 (locks/approved/{id}.json), not in KV, and wrangler
// cannot read an R2 object's version. So a candidate is a listing whose stored bytes are the bound bytes (same MD5, which is R2's
// etag for a single put; same SHA-256, which is the key), whose approval is still in force, whose case fingerprint still equals
// the listing's, and which was committed after the approval. Only the Worker can prove the version moved. Confirm each candidate
// with moderation_bind WITHOUT `expected` (an admin look; it writes nothing): if its objects.content differs from binding_content
// while the etag matches, the approval was voided.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { contentFingerprint } from '../src/moderation.mjs';

export const DEFAULT_ENV = 'staging';
// wrangler.toml: [env.staging] has its own namespaces and buckets. The top level is production, and its commands take no --env.
export const ENVIRONMENTS = {
  staging: { wrangler: ['--env', 'staging'], scenesBucket: 'eustress-simulations-staging' },
  production: { wrangler: [], scenesBucket: 'eustress-simulations' },
};
export const KV_BINDING = 'SOCIAL';
export const APPROVED_PREFIX = 'modq:approved:';
const CONCURRENCY = 4;
// The identity identOfObject writes for an object with no SHA-256: its etag (the MD5 of a single put), then R2's version. The
// version is R2's own string (hex in practice), so any token is accepted; the version is not checked.
const ETAG_IDENT = /^etag:([0-9a-fA-F]{32})(?::v[^:\s]+)?$/;
const LISTING_KEY = /^sim:([0-9a-fA-F-]{36})$/;
const READ_VERBS = [['kv', 'key', 'get'], ['kv', 'key', 'list'], ['r2', 'object', 'get']];

// The only wrangler calls this script can make. Every builder returns one of these reads; assertReadOnly refuses anything else.
export const kvGetArgs = (cfg, key) => ['kv', 'key', 'get', key, '--binding', KV_BINDING, ...cfg.wrangler, '--remote', '--text'];
export const kvListArgs = (cfg, prefix) => ['kv', 'key', 'list', '--binding', KV_BINDING, '--prefix', prefix, ...cfg.wrangler, '--remote'];
export const r2GetArgs = (cfg, key) => ['r2', 'object', 'get', `${cfg.scenesBucket}/${key}`, ...cfg.wrangler, '--remote', '--pipe'];

/// Throws unless the call is one of the three reads, judged by its verb (the first three words). Run before every wrangler spawn,
/// so a write cannot be made by mistake. The words after the verb are keys and flags, and a key may be any string.
export function assertReadOnly(args) {
  if (!READ_VERBS.some(verb => verb.every((word, i) => args[i] === word))) {
    throw new Error(`refusing a non-read wrangler call: ${args.slice(0, 3).join(' ')}`);
  }
}

/// The command line's options. Anything unknown is an error, and so is an environment name other than the two above.
export function parseArgs(argv) {
  const opts = { env: DEFAULT_ENV, all: false };
  for (const arg of argv) {
    if (arg === '--all') opts.all = true;
    else if (arg.startsWith('--env=')) {
      const name = arg.slice('--env='.length);
      if (!Object.hasOwn(ENVIRONMENTS, name)) throw new Error(`--env must be staging or production, not "${name}"`);
      opts.env = name;
    } else throw new Error(`unknown argument ${arg}; usage: [--all] [--env=staging|production]`);
  }
  return opts;
}

const md5Hex = bytes => createHash('md5').update(bytes).digest('hex');
const sha256Hex = bytes => createHash('sha256').update(bytes).digest('hex');
const parseOrNull = text => { try { return JSON.parse(text); } catch { return null; } };

/// Why a stored manifest is or is not the bound one, as far as its bytes and the binding can say. `ok` names the etag it matched.
function manifestCheck({ ident, approvedAt, committedAt, bytes, key, sha }) {
  const m = ETAG_IDENT.exec(ident ?? '');
  // A binding by SHA-256 cannot be voided by a re-put: the version does not enter it. Anything else (no binding content, or a
  // form this script does not know) is not judged here either; the admin look shows what is stored.
  if (!m) return { ok: false, reason: /^sha256:/.test(ident ?? '') ? 'binding_sha256' : 'binding_other' };
  // A re-commit can only void an approval it came after.
  if (!(Date.parse(committedAt) > approvedAt)) return { ok: false, reason: 'no_commit_after_approval' };
  if (!bytes) return { ok: false, reason: 'missing_bytes' };
  const keySha = /\/manifests\/([0-9a-f]{64})\.json$/.exec(key ?? '')?.[1];
  if (!keySha || keySha !== sha) return { ok: false, reason: 'key_not_its_hash' };
  if (sha256Hex(bytes) !== keySha) return { ok: false, reason: 'bytes_not_its_key' };
  if (md5Hex(bytes) !== m[1].toLowerCase()) return { ok: false, reason: 'bytes_differ_from_bound' };
  return { ok: true, etag: m[1].toLowerCase() };
}

/// Decide, from what wrangler can read, whether a listing's approval was voided by a re-commit of identical bytes.
/// Pure: no I/O. `worldBytes` and `sourceBytes` are the stored manifests (Buffers) or null when missing.
export function classify({ id, sim, caseRec, binding, worldBytes = null, sourceBytes = null }) {
  if (!sim) return { candidates: [], reasons: ['no_listing'] };
  if (sim.moderation?.status !== 'approved') return { candidates: [], reasons: ['not_approved'] };
  if (!binding) return { candidates: [], reasons: ['unbound'] };
  if (binding.revoked) return { candidates: [], reasons: ['revoked'] };
  const approvedAt = Date.parse(binding.at);
  if (!Number.isFinite(approvedAt)) return { candidates: [], reasons: ['no_approval_time'] };

  const candidates = [];
  const reasons = [];
  if (sim.world?.manifest_key) {
    // The approval binds r2_key. A .pak publish moves r2_key to universe.pak and leaves the world record in place, so a binding
    // there is the .pak's identity and says nothing about the manifest: the voided-approval question does not arise for it.
    if (sim.r2_key !== sim.world.manifest_key) reasons.push('world_r2_key_not_manifest');
    else {
      const r = manifestCheck({ ident: binding.content, approvedAt, committedAt: sim.world.committed_at, bytes: worldBytes, key: sim.world.manifest_key, sha: sim.world.manifest_sha256 });
      if (!r.ok) reasons.push(`world_${r.reason}`);
      else if (typeof caseRec?.content_fp !== 'string') reasons.push('world_no_review_fingerprint');
      else if (caseRec.content_fp !== contentFingerprint(sim)) reasons.push('world_fingerprint_changed');
      else candidates.push({ kind: 'world', sim_id: id, name: sim.name ?? null, manifest_sha256: sim.world.manifest_sha256, binding_content: binding.content, binding_at: binding.at, committed_at: sim.world.committed_at, bytes_etag: r.etag });
    }
  }
  if (sim.world?.source_manifest_key) {
    // The review's fingerprint does not cover the source world, so the source is checked on its bytes, key and binding only.
    const r = manifestCheck({ ident: binding.source, approvedAt, committedAt: sim.world.source_committed_at, bytes: sourceBytes, key: sim.world.source_manifest_key, sha: sim.world.source_manifest_sha256 });
    if (!r.ok) reasons.push(`source_${r.reason}`);
    else candidates.push({ kind: 'source', sim_id: id, name: sim.name ?? null, manifest_sha256: sim.world.source_manifest_sha256, binding_content: binding.source, binding_at: binding.at, committed_at: sim.world.source_committed_at, bytes_etag: r.etag });
  }
  if (!sim.world?.manifest_key && !sim.world?.source_manifest_key) reasons.push('no_world');
  return { candidates, reasons };
}

/// The all-listings report from every listing record. `entries` are { id, sim, binding }: `sim` is the parsed record (null when
/// unreadable), and `binding` is read for each approved listing only: null when there is none, { revoked: true } when it
/// cannot be read, otherwise the binding. Pure: no I/O.
export function summarizeAll(entries, skippedKeys = 0) {
  const report = {
    listings: 0, unreadable: skippedKeys, by_status: {}, formats: {},
    stale_current_content: [], echk_without_manifest: [], pak_with_world_record: 0,
    approved: { total: 0, bound: 0, unbound: [], revoked: [] },
  };
  for (const { id, sim, binding } of entries) {
    if (!sim) { report.unreadable += 1; continue; }
    report.listings += 1;
    const status = sim.moderation?.status ?? 'none';
    report.by_status[status] = (report.by_status[status] || 0) + 1;
    const format = sim.format ?? 'unset';
    report.formats[format] = (report.formats[format] || 0) + 1;
    const row = { sim_id: id, name: sim.name ?? null, moderation_status: status, is_public: sim.is_public !== false, r2_key: sim.r2_key ?? null, manifest_key: sim.world?.manifest_key ?? null };
    // An echk listing whose r2_key names a .pak: the state the one-current-content fix ends. Unplayable until the author republishes.
    if (sim.format === 'echk' && sim.world?.manifest_key && sim.r2_key && sim.r2_key !== sim.world.manifest_key) report.stale_current_content.push(row);
    else if (sim.format === 'echk' && !sim.world?.manifest_key) report.echk_without_manifest.push(row);
    if (sim.format !== 'echk' && sim.world?.manifest_key) report.pak_with_world_record += 1;
    if (status === 'approved') {
      report.approved.total += 1;
      if (binding === null) report.approved.unbound.push(row);
      else if (binding.revoked) report.approved.revoked.push(row);
      else report.approved.bound += 1;
    }
  }
  return report;
}

/// Run wrangler with the given arguments. Each call is checked as a read first. Arguments are quoted for the shell, which is
/// what `wrangler` needs on Windows.
function run(args) {
  assertReadOnly(args);
  return new Promise((resolve, reject) => {
    const line = ['wrangler', ...args].map(a => `"${String(a).replace(/"/g, '\\"')}"`).join(' ');
    const child = spawn(line, { shell: true, windowsHide: true });
    const out = [];
    const err = [];
    child.stdout.on('data', d => out.push(d));
    child.stderr.on('data', d => err.push(d));
    child.on('error', reject);
    child.on('close', code => resolve({ code, stdout: Buffer.concat(out), stderr: Buffer.concat(err).toString('utf8') }));
  });
}

/// The stdout of a read, or null when the object is not there. Any other failure stops the run: a missing object is reported
/// per listing, a broken binding or bucket is not hidden as one.
async function read(args) {
  const r = await run(args);
  if (r.code === 0) return r.stdout;
  if (/key|object/i.test(r.stderr) && /not ?found|does not exist|no such|NoSuchKey|10007|10009/i.test(r.stderr)) return null;
  throw new Error(`wrangler ${args.slice(0, 3).join(' ')} failed (exit ${r.code}): ${r.stderr.trim().slice(0, 400)}`);
}

const kvText = async (cfg, key) => {
  const b = await read(kvGetArgs(cfg, key));
  return b && b.length ? b.toString('utf8') : null;
};
const kvList = async (cfg, prefix) => {
  const b = await read(kvListArgs(cfg, prefix));
  return b && b.length ? JSON.parse(b.toString('utf8')) : [];
};

/// A listing's approval binding as the Worker reads it: null when there is none, { revoked: true } when it cannot be read.
async function readBinding(cfg, id) {
  const bytes = await read(r2GetArgs(cfg, `locks/approved/${id}.json`));
  if (!bytes || !bytes.length) return null;
  return parseOrNull(bytes.toString('utf8')) || { revoked: true };
}

async function gatherApproved(cfg, id) {
  const sim = parseOrNull((await kvText(cfg, `sim:${id}`)) ?? 'null');
  const caseRec = parseOrNull((await kvText(cfg, `modcase:${id}`)) ?? 'null');
  const binding = await readBinding(cfg, id);
  // Bytes are fetched only where a binding could have been voided: an etag binding that is still in force.
  const etagBound = b => !!b && !b.revoked && ETAG_IDENT.test(b.content ?? '');
  const sourceBound = b => !!b && !b.revoked && ETAG_IDENT.test(b.source ?? '');
  const worldBytes = sim?.world?.manifest_key && etagBound(binding) ? await read(r2GetArgs(cfg, sim.world.manifest_key)) : null;
  const sourceBytes = sim?.world?.source_manifest_key && sourceBound(binding) ? await read(r2GetArgs(cfg, sim.world.source_manifest_key)) : null;
  return { id, sim, caseRec, binding, worldBytes, sourceBytes };
}

async function approvedReport(cfg, log) {
  const listed = await kvList(cfg, APPROVED_PREFIX);
  const ids = listed.map(k => k.name.slice(APPROVED_PREFIX.length)).filter(Boolean);
  const candidates = [];
  const reasons = {};
  // Every listing that is not a candidate, with what was seen, so each can be looked at by id.
  const notCandidates = [];
  for (let i = 0; i < ids.length; i += CONCURRENCY) {
    const batch = await Promise.all(ids.slice(i, i + CONCURRENCY).map(id => gatherApproved(cfg, id)));
    for (const g of batch) {
      const r = classify(g);
      candidates.push(...r.candidates);
      for (const why of r.reasons.length ? r.reasons : ['candidate']) reasons[why] = (reasons[why] || 0) + 1;
      if (!r.candidates.length) notCandidates.push({ sim_id: g.id, name: g.sim?.name ?? null, reasons: r.reasons, r2_key: g.sim?.r2_key ?? null, binding_content: g.binding?.content ?? null, binding_source: g.binding?.source ?? null });
    }
    log(`checked ${Math.min(i + CONCURRENCY, ids.length)} of ${ids.length}`);
  }
  return {
    report: {
      approved_listings: ids.length,
      candidates,
      reasons,
      not_candidates: notCandidates,
      note: 'A candidate has the bytes that were bound and a commit after the approval; wrangler cannot read R2 versions. Confirm each one with moderation_bind WITHOUT expected (an admin look, read only): objects.content differing from binding_content with the same etag means the approval was voided. Pak listings are not checked. Re-binds are a reviewer action and check the case fingerprint and the key hash in the Worker.',
    },
    summary: `${candidates.length} candidate(s) of ${ids.length} approved listing(s)`,
  };
}

async function allReport(cfg, log) {
  const listed = await kvList(cfg, 'sim:');
  const ids = listed.map(k => LISTING_KEY.exec(k.name)?.[1]).filter(Boolean);
  const skipped = listed.length - ids.length;
  const entries = [];
  for (let i = 0; i < ids.length; i += CONCURRENCY) {
    const batch = await Promise.all(ids.slice(i, i + CONCURRENCY).map(async id => {
      const sim = parseOrNull((await kvText(cfg, `sim:${id}`)) ?? 'null');
      const binding = sim?.moderation?.status === 'approved' ? await readBinding(cfg, id) : undefined;
      return { id, sim, binding };
    }));
    entries.push(...batch);
    log(`checked ${Math.min(i + CONCURRENCY, ids.length)} of ${ids.length} listings`);
  }
  const report = summarizeAll(entries, skipped);
  return {
    report: {
      ...report,
      note: 'Read only. stale_current_content: listings whose format says echk while r2_key names a .pak; after the one-current-content fix they are unplayable until the author republishes. approved.unbound: approved listings with no binding; with REQUIRE_APPROVAL_BINDING set they read as pending. approved.revoked: bindings that were revoked or cannot be read. Statuses come from each listing record, not from the modq index.',
    },
    summary: `${report.listings} listing(s): ${report.stale_current_content.length} stale, ${report.approved.unbound.length} approved without a binding, ${report.approved.revoked.length} with a revoked binding`,
  };
}

async function main(argv) {
  const opts = parseArgs(argv);
  const cfg = { name: opts.env, ...ENVIRONMENTS[opts.env] };
  const log = msg => process.stderr.write(`${msg}\n`);
  const { report, summary } = opts.all ? await allReport(cfg, log) : await approvedReport(cfg, log);
  process.stdout.write(`${JSON.stringify({ env: cfg.name, mode: opts.all ? 'all' : 'approved', generated_at: new Date().toISOString(), ...report }, null, 2)}\n`);
  log(`${summary} on ${cfg.name}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  main(process.argv.slice(2)).catch(e => {
    process.stderr.write(`${e.message}\n`);
    process.exitCode = 1;
  });
}
