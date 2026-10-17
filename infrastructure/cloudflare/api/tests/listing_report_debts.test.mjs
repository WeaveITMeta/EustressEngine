// A listing's legal report debt (src/moderation.mjs sealAndReportListing, writeOwedListingReports, healListingReportDebts), a
// CSAM report written when the account is not known, and an account deletion that leaves a case owing a report, and its
// sealed evidence, as they were (forums round 9).
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';
import {
  makeToolExecutor, loadCase, listingReportOwedKey, fileForumReport, resolveForumCase, loadForumCase, forumCaseR2Key, forumReportOwedKey,
  POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import { VAULT_PREFIX } from '../src/evidence_vault.mjs';
import { writeLegalReport, loadLegalReport, sweepLegalReports } from '../src/legal_reports.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

function kv() {
  const m = new Map();
  return {
    m,
    get: async (k) => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async (k) => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter((k) => k.startsWith(prefix)).sort().slice(0, limit).map((name) => ({ name })), list_complete: true }),
  };
}

function r2() {
  const m = new Map();
  const obj = (key, rec) => ({ key, size: rec.bytes.byteLength, etag: rec.etag, customMetadata: rec.customMetadata || {}, checksums: {}, uploaded: new Date(),
    text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0) });
  return {
    m,
    put: async (key, body, opts = {}) => {
      if (opts.onlyIf?.etagDoesNotMatch === '*' && m.has(key)) return null;
      if (opts.onlyIf?.etagMatches !== undefined && (!m.has(key) || m.get(key).etag !== opts.onlyIf.etagMatches)) return null;
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body) : new Uint8Array(body instanceof ArrayBuffer ? body : body.buffer ? body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength) : body);
      const rec = { bytes, etag: createHash('md5').update(bytes).digest('hex'), customMetadata: opts.customMetadata };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async (key) => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async (key) => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async (key) => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000, include = [] } = {}) => ({ objects: [...m.keys()].filter((k) => k.startsWith(prefix)).sort().slice(0, limit).map((key) => ({ key, ...(include.includes('customMetadata') ? { customMetadata: m.get(key).customMetadata || {} } : {}) })), truncated: false }),
  };
}

const AUTHOR = 'user-0902';
const ADMIN = 'admin-0902';
const SIM = 'a1b2c3d4-0000-4000-8000-000000000902';

function environment() {
  const env = { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), KYC_STATUS: kv(), SCENES: r2(), JWT_SECRET: 'test-secret-listing-debts', JEV_API_KEY: 'jev-test', MODERATION_THRESHOLDS: '{"audit_rate":0}' };
  env.sent = [];
  env.EMAIL = { send: async (msg) => { env.sent.push(msg); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  env.USERS.m.set(`user:${AUTHOR}`, JSON.stringify({ id: AUTHOR, username: 'ann', email: 'ann@example.com', created_at: '2025-01-01T00:00:00.000Z' }));
  env.USERS.m.set(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'mod', email: 'mod@example.com', created_at: '2025-01-01T00:00:00.000Z' }));
  env.USERS.m.set(`admin:${ADMIN}`, '1');
  return env;
}

const quiet = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};
const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const deps = () => ({ auditLog: async () => {}, json, cors: {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED });

// A listing with stored objects and a case whose classifier answered `p` (the forum_case_store and legal_reporting harness).
async function listingWithCase(env, { p = 0.95 } = {}) {
  const category = 'csam_or_minor_sexualization';
  const sim = { id: SIM, name: 'Harbor Town', author_id: AUTHOR, is_public: true, r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-10-14T10:00:00.000Z', updated_at: '2026-10-14T10:00:00.000Z', moderation: { status: 'classifying' } };
  await env.SCENES.put(sim.r2_key, 'PAK-BYTES-CANARY-WALLABY');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({
    sim_id: SIM, author_id: AUTHOR, status: 'classifying', created_at: '2026-10-14T10:00:00.000Z', history: [],
    jev: { model: 'jev-test', answers: { [category]: { p } } }, decision: { reasons: [{ code: category, action: 'quarantine', lane: 'legal' }] },
  }));
}
const quarantine = (env) => quiet(() => makeToolExecutor(env, deps(), { actor: 'system', actorId: 'pipeline' })('moderation_quarantine', { sim_id: SIM, category: 'csam_or_minor_sexualization', rationale: 'Classifier probability 0.95 at publish; this is the test rationale.' }, { actor: 'system' }));
const reportsOf = async (env) => Promise.all([...env.SOCIAL.m.keys()].filter((k) => k.startsWith('legalreport:')).map((k) => loadLegalReport(env, k.slice('legalreport:'.length))));
const auditsOf = (env, action) => [...env.AUDIT_LOG.m.values()].map((v) => JSON.parse(v)).filter((a) => a.action === action);
function failPuts(env, pattern) {
  const real = env.SOCIAL.put;
  env.SOCIAL.put = async (key, ...rest) => { if (pattern.test(String(key))) throw new Error('KV is unavailable'); return real(key, ...rest); };
  return () => { env.SOCIAL.put = real; };
}

test('a listing csam report that fails is owed on the case and indexed, and the hourly sweep writes it', async () => {
  const env = environment();
  await listingWithCase(env);
  const restore = failPuts(env, /^legalreport:/);
  const q = await quarantine(env);
  restore();
  assert.equal(q.ok, true);
  const rec = await loadCase(env, SIM);
  assert.deepEqual([rec.status, rec.report_owed?.category, rec.report_owed?.actor, rec.report_owed?.reason], ['quarantined', 'csam_or_minor_sexualization', 'system', 'unavailable']);
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'indexed for the sweep');
  assert.equal((await reportsOf(env)).length, 0);
  // With LISTING_LEGAL_OWED_SWEEP off (the default) the hourly sweep touches no listing case, no listing owed index and sends
  // no stuck mail: the debt simply waits.
  const touched = [];
  const { get, list } = env.SOCIAL;
  env.SOCIAL.get = (k, ...a) => { touched.push(String(k)); return get.call(env.SOCIAL, k, ...a); };
  env.SOCIAL.list = (o, ...a) => { touched.push(String(o?.prefix)); return list.call(env.SOCIAL, o, ...a); };
  const mailBefore = (env.mail || []).length;
  const off = await quiet(() => sweepLegalReports(env));
  env.SOCIAL.get = get; env.SOCIAL.list = list;
  assert.deepEqual([off.owed_listing_reports, off.stuck], ['off', 'off']);
  assert.ok(touched.length > 0, 'the sweep did run');
  assert.equal(touched.some((k) => k.startsWith('modcase:') || k.startsWith('legalowed:moderation') || k.startsWith('legalstuck:') || k.startsWith('modq:')), false, 'no listing case, owed index, stuck index or queue read');
  assert.equal((env.mail || []).length, mailBefore, 'no mail');
  assert.equal((await reportsOf(env)).length, 0);
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'the debt is kept');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  const swept = await quiet(() => sweepLegalReports(env));
  assert.deepEqual([swept.owed_listing_reports.written, swept.owed_listing_reports.still_owed], [1, 0]);
  const [rep] = await reportsOf(env);
  assert.deepEqual([rep.kind, rep.when_where.case_id], ['csam', SIM]);
  assert.equal((await loadCase(env, SIM)).report_owed, null);
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(SIM)), null);
});

test('a listing csam quarantine whose report was recorded nowhere is found by the heal and reported', async () => {
  const env = environment();
  await listingWithCase(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantine(env);
  restore();
  // The write after the quarantine was lost: no debt on the case and no index.
  const rec = await loadCase(env, SIM);
  delete rec.report_owed;
  delete rec.legal_report;
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await env.SOCIAL.delete(listingReportOwedKey(SIM));
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  const swept = await quiet(() => sweepLegalReports(env));
  assert.deepEqual([swept.owed_listing_reports.healed, swept.owed_listing_reports.written], [1, 1]);
  assert.equal((await reportsOf(env)).length, 1);
  assert.equal((await loadCase(env, SIM)).report_owed, null);
});

test('a borderline listing removal is settled: no report, no debt, and the reason audited', async () => {
  const env = environment();
  await listingWithCase(env, { p: 0.6 });
  assert.match((await quarantine(env)).summary, /report borderline_never_reported/);
  const rec = await loadCase(env, SIM);
  assert.equal(rec.report_owed, null);
  assert.equal(rec.report_settled.reason, 'borderline_never_reported');
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(SIM)), null);
  assert.deepEqual(auditsOf(env, 'LEGAL_REPORT_SETTLED').map((a) => a.details.reason), ['borderline_never_reported']);
  assert.equal((await reportsOf(env)).length, 0);
});

test('a csam report is written when the account is not known or was deleted, and says so; a threat with no account is not', async () => {
  const env = environment();
  const finding = (caseId, account_id) => ({ kind: 'csam', caseKind: 'forum', caseId, category: 'csam', article: 'A1', subject: { kind: 'forum_post', post_id: caseId }, detection: { source: 'person', checks: [], band: 'high' }, uploader: { account_id } });
  const unknown = await quiet(() => writeLegalReport(env, finding(`${'e'.repeat(24)}-1`, undefined)));
  assert.equal(unknown.written, true, JSON.stringify(unknown));
  const u = await loadLegalReport(env, unknown.id);
  assert.deepEqual([u.who.account_id, u.who.account_status, u.who.email, u.takeover.account_state_read], [null, 'unknown', null, false]);
  // 'deleted' rests on two reads that found no user record, at least a minute apart: the first try is owed.
  const first = await quiet(() => writeLegalReport(env, finding(`${'e'.repeat(24)}-2`, 'user-gone-0902')));
  assert.deepEqual([first.written, first.reason], [false, 'account_unconfirmed']);
  const deleted = await quiet(() => writeLegalReport(env, finding(`${'e'.repeat(24)}-2`, 'user-gone-0902'), { now: () => Date.now() + 61_000 }));
  assert.equal((await loadLegalReport(env, deleted.id)).who.account_status, 'deleted');
  const known = await quiet(() => writeLegalReport(env, finding(`${'e'.repeat(24)}-3`, AUTHOR)));
  assert.equal((await loadLegalReport(env, known.id)).who.account_status, 'known');
  const threat = await quiet(() => writeLegalReport(env, {
    kind: 'threat', caseKind: 'forum', caseId: `${'e'.repeat(24)}-4`, category: 'threat',
    detection: { source: 'forum_check', band: 'high', checks: [{ kind: 'classifier', source: 'a', category: 'threat', confidence: 0.97, band: 'high' }, { kind: 'classifier', source: 'b', category: 'threat', confidence: 0.96, band: 'high' }] },
    content: { named_target: true, religious_speech: false, political_speech: false, criticism: false, hyperbole: false, joke: false }, uploader: {},
  }));
  assert.deepEqual([threat.written, threat.reason], [false, 'no_account']);
});

// -- account deletion -------------------------------------------------------------------------------------------------------

test('deleting an account leaves a forum case that owes a report, and its sealed evidence, exactly as they were', async () => {
  const worker = await loadWorker();
  const ADMIN_ID = '99999999-9999-4999-8999-999999999902';
  const VICTIM = '11111111-1111-4111-8111-111111110902';
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), SCENES: r2() };
  await env.USERS.put(`admin:${ADMIN_ID}`, '1');
  await env.USERS.put(`user:${ADMIN_ID}`, JSON.stringify({ id: ADMIN_ID, username: 'staff_one' }));
  await env.USERS.put(`user:${VICTIM}`, JSON.stringify({ id: VICTIM, username: 'victim_0902', created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put('username:victim_0902', VICTIM);
  const post = `${'f'.repeat(24)}-1`;
  await quiet(() => fileForumReport({ env, postId: post, threadId: 'f'.repeat(24), postAuthorId: VICTIM, reporterId: ADMIN_ID, category: 'spam', postBody: 'KOALA-CANARY text' }));
  const restore = failPuts(env, /^legalreport:/);
  const r = await quiet(() => resolveForumCase({ env, postId: post, decision: 'remove', category: 'csam', reviewerId: ADMIN_ID, postBody: 'KOALA-CANARY text' }));
  restore();
  assert.equal(r.ok, true);
  assert.ok((await loadForumCase(env, post)).report_owed, 'the case owes its report');
  const vaultKeys = [...env.SCENES.m.keys()].filter((k) => k.startsWith(VAULT_PREFIX));
  assert.ok(vaultKeys.length > 0, 'and holds sealed evidence');
  const before = JSON.stringify([...env.SCENES.m.entries()].filter(([k]) => k.startsWith(VAULT_PREFIX) || k === forumCaseR2Key(post)).map(([k, v]) => [k, v.etag]).sort());
  const socialBefore = JSON.stringify([...env.SOCIAL.store.entries()].filter(([k]) => k.startsWith('legal') || k.startsWith('vault') || k.startsWith('forum')).sort());
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/admin/delete-user', { method: 'POST', headers: { Authorization: `Bearer ${mintToken(ADMIN_ID)}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'victim_0902' }) }), env, ctx);
  assert.equal(res.status, 200, await res.clone().text());
  assert.equal(await env.USERS.get(`user:${VICTIM}`), null, 'the account is gone');
  const after = JSON.stringify([...env.SCENES.m.entries()].filter(([k]) => k.startsWith(VAULT_PREFIX) || k === forumCaseR2Key(post)).map(([k, v]) => [k, v.etag]).sort());
  assert.equal(after, before, 'the case and its sealed evidence are untouched');
  assert.equal(JSON.stringify([...env.SOCIAL.store.entries()].filter(([k]) => k.startsWith('legal') || k.startsWith('vault') || k.startsWith('forum')).sort()), socialBefore, 'and so are its owed index and evidence records');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(post)));
  // The debt is still paid: the sweep writes the report, with the account marked deleted.
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  // The first sweep finds no user record and waits to read it again (two reads a minute apart); the next one writes it.
  await quiet(() => sweepLegalReports(env));
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalreport:')).length, 0, 'not on one read');
  await quiet(() => sweepLegalReports(env, { now: () => Date.now() + 61_000 }));
  const reports = [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalreport:'));
  assert.equal(reports.length, 1);
  assert.equal(JSON.parse(env.SOCIAL.store.get(reports[0])).who.account_status, 'deleted');
});
