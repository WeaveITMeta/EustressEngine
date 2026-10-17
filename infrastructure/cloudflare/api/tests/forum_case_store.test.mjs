// The forum case store (src/moderation.mjs): the case is kept in R2 under a compare-and-swap on its etag, adopted once from
// KV, and versioned (`case_version: { round, seq }`); listOpenForumCases for the forum's reconcile pass; the report allowance
// spent only once a report is accepted; whether a person viewed a post's content, recorded and never assumed
// (recordForumView, src/legal_reports.mjs); and sealForumEvidence's `itemId` and `uploadIp`.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { fakeKv } from './helpers/load_worker.mjs';
import {
  fileForumReport, resolveForumCase, forumPostModeration, loadForumCase, sealForumEvidence, recordForumRemoval, recordForumView,
  listOpenForumCases, forumCaseKey, forumCaseR2Key, forumReportCapKey, forumPostLockKey, forumTextItemId, forumUploadIpOf,
  FORUM_CASE_PREFIX, FORUM_CASE_WRITE_ATTEMPTS, FORUM_OPEN_LIST_MAX, FORUM_TEXT_ITEM_PREFIX,
  registerForumDecisionApplier, makeToolExecutor, forumPostIpKey, forumPostIpPrefix, forumUploadIpsOf, FORUM_UPLOAD_IPS_MAX,
  FORUM_UPLOAD_IP_MAX_AGE_DAYS, releaseForumNotice, writeOwedForumReports, handleModerationRoute,
} from '../src/moderation.mjs';
import { handleCaseRoute, forumNoticeEntry } from '../src/case_history.mjs';
import { readEvidence, sealEvidence, handleEvidenceRoute, retryPendingEvidence, VAULT_PREFIX, PENDING_PREFIX, PENDING_CURSOR_KEY } from '../src/evidence_vault.mjs';
import { loadLegalReport, writeLegalReport, recordScamRemoval, weeklyScamBatch, amendLegalReportUploadIps, amendLegalReport, sweepLegalReports, NOT_VIEWED_PERSON_DECIDED, VIEWED_BY_STAFF } from '../src/legal_reports.mjs';

const THREAD = 'c'.repeat(24);
const POST = `${THREAD}-1`;
const AUTHOR = 'author-0001';
const ADMIN = 'admin-0001';
const OTHER_ADMIN = 'admin-0002';
const REPORTERS = ['old-0001', 'old-0002', 'old-0003', 'old-0004'];
const NOW = Date.parse('2026-10-15T18:00:00.000Z');
const DAY = '2026-10-15';
const SECRET = 'test-secret-forum-store';
const TEXT = 'WOMBAT-CANARY forum post text';
const LEGAL = { kind: 'subpoena', reference: 'SUB-2026-0042' };
const enc = new TextEncoder();
const dec = new TextDecoder();

// R2 as it behaves: strongly consistent, an etag that is the MD5 of the content, conditional puts (create-if-absent and
// replace-if-unchanged) that write nothing and answer null when they fail, and a list that pages with a cursor. `hooks`
// lets a test act between a writer's read and its write (another location writing first) or make a put fail.
function bucket() {
  const objects = new Map();
  const hooks = { beforePut: null, puts: [], lists: [] };
  const view = (key, o) => ({
    key, size: o.bytes.byteLength, etag: o.etag, customMetadata: o.customMetadata || {}, uploaded: new Date(), checksums: {},
    text: async () => dec.decode(o.bytes), arrayBuffer: async () => o.bytes.slice().buffer,
  });
  return {
    objects, hooks,
    async put(key, value, opts = {}) {
      const bytes = typeof value === 'string' ? enc.encode(value) : value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength));
      if (hooks.beforePut) await hooks.beforePut(key, opts);
      hooks.puts.push({ key, onlyIf: opts.onlyIf || null, body: typeof value === 'string' ? value : null });
      const cur = objects.get(key);
      if (opts.onlyIf?.etagDoesNotMatch === '*' && cur) return null;
      if (opts.onlyIf?.etagMatches !== undefined && (!cur || cur.etag !== opts.onlyIf.etagMatches)) return null;
      const o = { bytes, etag: crypto.createHash('md5').update(bytes).digest('hex'), customMetadata: opts.customMetadata };
      objects.set(key, o);
      return view(key, o);
    },
    async get(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async head(key) { return objects.has(key) ? view(key, objects.get(key)) : null; },
    async delete(key) { objects.delete(key); },
    async list({ prefix = '', limit = 1000, cursor, include = [] } = {}) {
      hooks.lists.push({ prefix, limit, cursor });
      const keys = [...objects.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
      const page = keys.slice(0, limit);
      const truncated = keys.length > limit;
      return {
        objects: page.map((key) => ({ key, size: objects.get(key).bytes.byteLength, ...(include.includes('customMetadata') ? { customMetadata: objects.get(key).customMetadata || {} } : {}) })),
        truncated, ...(truncated ? { cursor: page.at(-1) } : {}),
      };
    },
    // Writes the case directly, as another writer would: a stored case changed after this writer read it.
    async rewriteCase(postId, change) {
      const key = forumCaseR2Key(postId);
      const o = objects.get(key);
      const rec = JSON.parse(dec.decode(o.bytes));
      change(rec);
      rec.seq += 1;
      const bytes = enc.encode(JSON.stringify(rec));
      objects.set(key, { bytes, etag: crypto.createHash('md5').update(bytes).digest('hex'), customMetadata: { ...o.customMetadata, seq: String(rec.seq) } });
    },
  };
}

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

async function environment({ secret = SECRET } = {}) {
  const env = { USERS: fakeKv(), SOCIAL: fakeKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), SCENES: bucket(), __EmailMessage: StubMessage, mail: [] };
  if (secret) env.JWT_SECRET = secret;
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  const old = new Date(NOW - 30 * 86400 * 1000).toISOString();
  for (const id of [AUTHOR, ADMIN, OTHER_ADMIN, ...REPORTERS, 'r-other']) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: old }));
  for (const id of [ADMIN, OTHER_ADMIN]) await env.USERS.put(`admin:${id}`, '1');
  return env;
}

const quiet = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};
const report = (env, reporterId, category, extra = {}) => quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId, category, postBody: TEXT, now: NOW, ...extra }));
const seal = (env, extra = {}) => quiet(() => sealForumEvidence({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: TEXT, category: 'csam', reviewerId: ADMIN, now: NOW, ...extra }));
const used = async (env, reporterId) => parseInt((await env.SOCIAL.get(forumReportCapKey(reporterId, DAY))) || '0', 10);
const stored = (env, key) => { const o = env.SCENES.objects.get(key); return o ? JSON.parse(dec.decode(o.bytes)) : null; };
const keyedHex = (text, secret = SECRET) => crypto.createHmac('sha256', crypto.createHmac('sha256', secret).update('forum-evidence-v1').digest()).update(text, 'utf8').digest('hex');
const keyedItem = (text, secret = SECRET) => `post-text-${keyedHex(text, secret).slice(0, 32)}`;
// The item addresses are sealed under when their text was sealed before them: the text's item, `.ip-`, 8 hex of the keyed
// hash of the entries.
const ipItemOf = (textItem, entries) => `${textItem.slice(0, 52)}.ip-${keyedHex(`upload-ips:${JSON.stringify(entries)}`).slice(0, 8)}`;
const legacyEntry = (ip) => ({ ip, at: null, item_id: null });
const reportsOf = async (env) => Promise.all([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalreport:')).map((k) => loadLegalReport(env, k.slice('legalreport:'.length))));
const plainStorage = (env) => [
  ...env.SOCIAL.store.entries(), ...env.USERS.store.entries(), ...env.AUDIT_LOG.store.entries(),
  ...[...env.SCENES.objects.entries()].map(([k, o]) => [k, dec.decode(o.bytes) + JSON.stringify(o.customMetadata || {})]),
].map(([k, v]) => `${k}=${v}`).join('\n');

// -- finding 1: the case is in R2, written with a compare-and-swap -------------------------------------------------------------

test('the case is kept in R2 at moderation/forumcase/{postId}.json, with its flags as metadata, and never in KV', async () => {
  const env = await environment();
  assert.equal((await report(env, REPORTERS[0], 'csam')).ok, true);
  assert.equal(forumCaseR2Key(POST), `${FORUM_CASE_PREFIX}${POST}.json`);
  assert.equal(FORUM_CASE_PREFIX, 'moderation/forumcase/');
  const rec = stored(env, forumCaseR2Key(POST));
  assert.deepEqual([rec.post_id, rec.reporters, rec.hidden, rec.seq], [POST, [REPORTERS[0]], true, 1]);
  assert.deepEqual(env.SCENES.objects.get(forumCaseR2Key(POST)).customMetadata, { seq: '1', round: '0', reference: rec.reference, hidden: '1', gated: '0', removed: '0', waiting: '1' });
  assert.equal(await env.SOCIAL.get(forumCaseKey(POST)), null, 'nothing is written to the old KV key');
  // Every case write is conditional: create-if-absent, then replace-if-unchanged on the etag that was read.
  const writes = env.SCENES.hooks.puts.filter((p) => p.key === forumCaseR2Key(POST));
  assert.deepEqual(writes.map((p) => p.onlyIf), [{ etagDoesNotMatch: '*' }]);
  const etagRead = env.SCENES.objects.get(forumCaseR2Key(POST)).etag;
  await report(env, REPORTERS[1], 'threat');
  assert.deepEqual(env.SCENES.hooks.puts.filter((p) => p.key === forumCaseR2Key(POST)).at(-1).onlyIf, { etagMatches: etagRead });
  assert.notEqual(env.SCENES.objects.get(forumCaseR2Key(POST)).etag, etagRead, 'every write changes the etag');
});

test('a write that meets a newer case reads it again and builds on it: no reporter and no hide is lost', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'threat');
  // Between this report's read and its write, another location records a csam report and hides the post.
  let injected = 0;
  env.SCENES.hooks.beforePut = async (key, opts) => {
    if (key !== forumCaseR2Key(POST) || !opts.onlyIf?.etagMatches || injected++) return;
    await env.SCENES.rewriteCase(POST, (rec) => {
      rec.reporters.push('r-other');
      rec.reports.push({ reporter_id: 'r-other', category: 'csam', note: null, at: new Date(NOW).toISOString(), counted: true, round: 0, body_hash: null, body_hash_unavailable: false });
      rec.tally.remove += 1;
      rec.hidden = true; rec.hide_reason = 'csam';
    });
  };
  const r = await report(env, REPORTERS[1], 'threat');
  assert.equal(r.ok, true, JSON.stringify(r));
  const casWrites = env.SCENES.hooks.puts.filter((p) => p.key === forumCaseR2Key(POST) && p.onlyIf?.etagMatches);
  assert.equal(casWrites.length, 2, 'the first write met a conflict; the second, on the newer case, held');
  const rec = await loadForumCase(env, POST);
  assert.deepEqual(rec.reporters, [REPORTERS[0], 'r-other', REPORTERS[1]], 'every reporter is kept');
  assert.deepEqual(rec.reports.map((x) => [x.reporter_id, x.category]), [[REPORTERS[0], 'threat'], ['r-other', 'csam'], [REPORTERS[1], 'threat']]);
  // The other writer's count is kept and built on: with it, this report is the third and removes the post.
  assert.deepEqual([rec.hidden, rec.removed, rec.tally.remove, r.remove], [true, true, 3, true], 'the other writer\'s hide and count are kept');
  assert.equal(rec.seq, 3, 'one write each: the first report, the other location, this one');
  assert.equal(r.reports, 3);
  assert.equal(await used(env, REPORTERS[1]), 1, 'the allowance is spent once');
});

test('a case that keeps changing under a write is answered busy after a bounded number of tries, and nothing is written', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'threat');
  let n = 0;
  env.SCENES.hooks.beforePut = async (key, opts) => {
    if (key === forumCaseR2Key(POST) && opts.onlyIf?.etagMatches) await env.SCENES.rewriteCase(POST, (rec) => { rec.churn = ++n; });
  };
  const r = await report(env, REPORTERS[1], 'threat');
  assert.deepEqual([r.ok, r.code, r.status], [false, 'busy', 503]);
  assert.equal(n, FORUM_CASE_WRITE_ATTEMPTS, 'it tried FORUM_CASE_WRITE_ATTEMPTS times');
  const rec = await loadForumCase(env, POST);
  assert.ok(!rec.reporters.includes(REPORTERS[1]), 'the report was not recorded');
  assert.equal(await used(env, REPORTERS[1]), 0, 'and spent nothing');
});

test('a case kept in KV before the move is adopted into R2 once, and KV is never read for it again', async () => {
  const env = await environment();
  const legacy = { v: 1, kind: 'forum', post_id: POST, thread_id: THREAD, author_id: AUTHOR, reference: null, status: 'hidden', created_at: '2026-10-14T00:00:00.000Z', updated_at: '2026-10-14T00:00:00.000Z', round: 0, waiting: true, waiting_since: '2026-10-14T00:00:00.000Z', tally: { remove: 1, gate: 0 }, hidden: true, hidden_at: '2026-10-14T00:00:00.000Z', hide_reason: 'csam', gated: false, removed: false, evidence: { post_id: POST, thread_id: THREAD, body_hash: null, later: [] }, reporters: [REPORTERS[0]], reports: [{ reporter_id: REPORTERS[0], category: 'csam', counted: true, round: 0 }], reports_not_kept: 0, decisions: [], history: [] };
  await env.SOCIAL.put(forumCaseKey(POST), JSON.stringify(legacy));
  // Two readers at once adopt it once: one create-if-absent wins, the other reads R2's.
  const [a, b] = await Promise.all([loadForumCase(env, POST), loadForumCase(env, POST)]);
  assert.deepEqual([a.reporters, b.reporters, a.seq, b.seq], [[REPORTERS[0]], [REPORTERS[0]], 1, 1]);
  assert.ok(a.migrated_from_kv_at);
  assert.equal(env.SCENES.hooks.puts.filter((p) => p.key === forumCaseR2Key(POST) && env.SCENES.objects.has(p.key)).length >= 1, true);
  assert.equal(stored(env, forumCaseR2Key(POST)).seq, 1, 'adopted by one write');
  // A stale KV copy changes nothing once R2 has the case.
  await env.SOCIAL.put(forumCaseKey(POST), JSON.stringify({ ...legacy, reporters: [], hidden: false }));
  assert.deepEqual([(await loadForumCase(env, POST)).hidden, (await forumPostModeration({ env, postId: POST })).hidden], [true, true]);
  const r = await report(env, REPORTERS[1], 'threat');
  assert.deepEqual([r.ok, r.reports, r.hide], [true, 2, true]);
  assert.deepEqual(stored(env, forumCaseR2Key(POST)).reporters, [REPORTERS[0], REPORTERS[1]]);
  assert.deepEqual(JSON.parse(await env.SOCIAL.get(forumCaseKey(POST))).reporters, [], 'KV is left as it was, and not read');
});

// -- finding 3: the allowance is spent once, after the case write ------------------------------------------------------------

test('a busy filing spends nothing; its retry spends once; a repeat and a failed write spend nothing', async () => {
  const env = await environment();
  // Another writer holds the case's lease: the report waits, then is answered busy.
  await env.SCENES.put(`locks/forum/${POST}.json`, JSON.stringify({ holder: 'someone-else', sim_id: POST, who: 'forum_resolve', acquired_at: new Date().toISOString(), expires_at: Date.now() + 60_000 }));
  const busy = await report(env, REPORTERS[0], 'threat');
  assert.deepEqual([busy.ok, busy.code, busy.status], [false, 'busy', 503]);
  assert.equal(await used(env, REPORTERS[0]), 0, 'a busy filing spends nothing');
  await env.SCENES.delete(`locks/forum/${POST}.json`);
  const retry = await report(env, REPORTERS[0], 'threat');
  assert.deepEqual([retry.ok, retry.reports], [true, 1]);
  assert.equal(await used(env, REPORTERS[0]), 1, 'the retry spends once');
  assert.equal((await report(env, REPORTERS[0], 'threat')).ok, true);
  assert.equal(await used(env, REPORTERS[0]), 1, 'a repeat spends nothing');
  // The case write itself fails: the filing is refused and spends nothing.
  env.SCENES.hooks.beforePut = async (key) => { if (key === forumCaseR2Key(POST)) throw new Error('R2 is unavailable'); };
  const failed = await report(env, REPORTERS[1], 'threat');
  assert.deepEqual([failed.ok, failed.code], [false, 'unavailable']);
  assert.equal(await used(env, REPORTERS[1]), 0, 'a failed filing spends nothing');
  env.SCENES.hooks.beforePut = null;
  assert.equal((await report(env, REPORTERS[1], 'threat')).ok, true);
  assert.equal(await used(env, REPORTERS[1]), 1);
});

// -- finding 2: whether a person viewed the content is recorded, never assumed -----------------------------------------------

test('a removal decided without viewing the content says so: viewed_by_staff false, and the report says "Not viewed by Eustress staff"', async () => {
  const env = await environment();
  assert.equal((await report(env, REPORTERS[0], 'csam')).hide, true);
  // The administrator even opened the vault, but never said they viewed it: a read alone does not make the record say so.
  const sealed = await seal(env);
  assert.equal(sealed.ok, true);
  assert.equal((await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL })).ok, true);
  const decided = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  assert.equal(decided.ok, true);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.decisions.at(-1).viewed_by_staff, false, 'the decision records that its person did not view the content');
  const [rep, ...more] = await reportsOf(env);
  assert.equal(more.length, 0);
  assert.deepEqual([rep.kind, rep.what.viewed_by_staff, rep.what.view, rep.what.statement], ['csam', false, null, NOT_VIEWED_PERSON_DECIDED]);
  assert.match(rep.what.statement, /^Not viewed by Eustress staff\./);
  // The sealed facts record the same truth.
  const facts = (await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL })).items.map((i) => i.facts.detection);
  assert.ok(facts.length >= 1);
  for (const d of facts) assert.equal(d.viewed_by_staff, false, JSON.stringify(d));
});

test('a recorded view by the deciding person, after an audited vault read, makes the report say the content was viewed', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'csam');
  const sealed = await seal(env);
  // No audited read yet: "I viewed this content" is refused, and nothing is recorded.
  const early = await quiet(() => recordForumView({ env, postId: POST, reviewerId: ADMIN }));
  assert.deepEqual([early.ok, early.code, early.status], [false, 'not_viewed', 409]);
  assert.equal((await quiet(() => recordForumView({ env, postId: POST, reviewerId: AUTHOR }))).code, 'bad_reviewer');
  assert.equal((await quiet(() => recordForumView({ env, postId: POST, reviewerId: ADMIN, how: 'looked_at_it' }))).code, 'bad_how');
  assert.equal((await loadForumCase(env, POST)).views.length, 0);

  assert.equal((await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL })).ok, true);
  // A view that cannot be audited is not recorded.
  const noAudit = { ...env, AUDIT_LOG: { put: async () => { throw new Error('down'); } } };
  assert.equal((await quiet(() => recordForumView({ env: noAudit, postId: POST, reviewerId: ADMIN }))).code, 'audit_unavailable');
  assert.equal((await loadForumCase(env, POST)).views.length, 0);

  const viewed = await quiet(() => recordForumView({ env, postId: POST, reviewerId: ADMIN, now: NOW + 1000 }));
  assert.equal(viewed.ok, true, JSON.stringify(viewed));
  assert.deepEqual([viewed.viewed.by, viewed.viewed.how, viewed.viewed.items], [ADMIN, 'evidence_vault_read', [keyedItem(TEXT)]]);
  const rec = await loadForumCase(env, POST);
  assert.deepEqual(rec.views.map((v) => [v.by, v.how, v.legal_process.reference]), [[ADMIN, 'evidence_vault_read', LEGAL.reference]]);
  const audited = [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v)).filter((a) => a.action === 'FORUM_CONTENT_VIEWED');
  assert.deepEqual(audited.map((a) => [a.admin_id, a.target, a.details.post_id]), [[ADMIN, sealed.reference, POST]], 'who, which case, which post');

  // Another administrator's decision is not the viewer's: still not viewed.
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'csam');
  const sealed2 = await seal(env2);
  await readEvidence(env2, { caseRef: sealed2.reference, adminId: ADMIN, legalProcess: LEGAL });
  await quiet(() => recordForumView({ env: env2, postId: POST, reviewerId: ADMIN }));
  await quiet(() => resolveForumCase({ env: env2, postId: POST, decision: 'remove', category: 'csam', reviewerId: OTHER_ADMIN, postBody: TEXT }));
  assert.equal((await loadForumCase(env2, POST)).decisions.at(-1).viewed_by_staff, false, 'a view by someone else does not count');
  assert.equal((await reportsOf(env2))[0].what.viewed_by_staff, false);

  // The viewer decides: viewed.
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  assert.equal((await loadForumCase(env, POST)).decisions.at(-1).viewed_by_staff, true);
  const [rep] = await reportsOf(env);
  assert.deepEqual([rep.what.viewed_by_staff, rep.what.statement, rep.what.view.by, rep.what.view.how], [true, VIEWED_BY_STAFF, ADMIN, 'evidence_vault_read']);
  assert.doesNotMatch(rep.what.statement, /Not viewed/);
});

test('a view of the text before an edit is not a view of the edited text', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'csam');
  const sealed = await seal(env);
  await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL });
  assert.equal((await quiet(() => recordForumView({ env, postId: POST, reviewerId: ADMIN }))).ok, true);
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: `${TEXT} edited` }));
  assert.equal((await loadForumCase(env, POST)).decisions.at(-1).viewed_by_staff, false);
});

test('the report follows the recorded view: viewed_by_staff true with no view on record is reported as not viewed', async () => {
  const env = await environment();
  const r = await quiet(() => writeLegalReport(env, { kind: 'csam', caseKind: 'forum', caseId: POST, category: 'csam', article: 'A1', detection: { source: 'person', band: 'high', checks: [], viewed_by_staff: true }, uploader: { account_id: AUTHOR } }));
  assert.equal(r.written, true);
  const rep = await loadLegalReport(env, r.id);
  assert.deepEqual([rep.what.viewed_by_staff, rep.what.statement], [false, NOT_VIEWED_PERSON_DECIDED]);
  // A person confirming a scam removal is not a person viewing it.
  await recordScamRemoval(env, { caseKind: 'forum', caseId: 'p1', subject: { kind: 'forum_post', post_id: 'p1' }, account_id: 'scammer', removed_at: '2026-10-06T01:00:00.000Z', confirmed_by: 'person', detection: { source: 'person', band: 'high', checks: [] } });
  const built = await quiet(() => weeklyScamBatch(env, Date.parse('2026-10-12T05:00:00.000Z')));
  assert.equal((await loadLegalReport(env, built.id)).what.viewed_by_staff, false);
});

test('POST /api/admin/evidence/{reference}/viewed records the view for an administrator with a read on record', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'csam');
  const sealed = await seal(env);
  const json = (body, status = 200) => new Response(JSON.stringify(body), { status });
  const call = async (adminId, ref = sealed.reference, method = 'POST') => {
    const url = new URL(`https://api.eustress.dev/api/admin/evidence/${ref}/viewed`);
    const res = await quiet(() => handleEvidenceRoute(new Request(url, { method, ...(method === 'POST' ? { body: '{}' } : {}) }), url, env, {}, { requireAdmin: async () => adminId, json }));
    return { status: res.status, body: await res.json() };
  };
  assert.equal((await call(null)).status, 403);
  assert.equal((await call(ADMIN, sealed.reference, 'GET')).status, 405);
  assert.deepEqual([(await call(ADMIN)).status, (await call(ADMIN)).body.code], [409, 'not_viewed']);
  await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL });
  const ok = await call(ADMIN);
  assert.deepEqual([ok.status, ok.body.ok, ok.body.viewed.by], [200, true, ADMIN]);
  assert.equal((await call(ADMIN, 'EC-0000-0000-0000-0000-0000')).status, 404);
});

// -- ask (a): uploadIp --------------------------------------------------------------------------------------------------------

test('uploadIp is an IPv4 or IPv6 address, sealed into the encrypted facts for csam and nonconsensual_sexual only', async () => {
  for (const ip of ['203.0.113.9', '0.0.0.0', '255.255.255.255']) assert.equal(forumUploadIpOf(ip), ip);
  assert.equal(forumUploadIpOf('2001:DB8:0:0:0:0:0:1'), '2001:db8::1');
  assert.equal(forumUploadIpOf('::1'), '::1');
  assert.ok(forumUploadIpOf('::ffff:192.0.2.1'));
  for (const bad of ['999.1.1.1', '01.2.3.4', '1.2.3', '1.2.3.4.5', '1.2.3.4:80', 'example.com', '2001:db8::1%eth0', '[::1]', '2001:db8:::1', '', ' ', 42, null, {}]) assert.equal(forumUploadIpOf(bad), null, JSON.stringify(bad));

  // An address that is not one is dropped, never refused: the seal goes ahead without it.
  const env = await environment();
  const dropped = await seal(env, { uploadIp: '999.1.1.1' });
  assert.deepEqual([dropped.ok, dropped.upload_ip_sealed], [true, false]);
  const env1 = await environment();
  const IP = '198.51.100.77';
  const r = await seal(env1, { uploadIp: IP });
  assert.deepEqual([r.ok, r.upload_ip_sealed], [true, true]);
  assert.ok(!plainStorage(env1).includes(IP), 'the address is in no case, no plain index, no audit record, no KV value');
  const read = await readEvidence(env1, { caseRef: r.reference, adminId: ADMIN, legalProcess: LEGAL });
  assert.deepEqual(read.items[0].facts.uploader.upload_ips, [legacyEntry(IP)], 'it is in the sealed facts, with no time (legacy)');
  assert.ok(!('uploader' in read.items[0].index), 'not in the plain index');

  // Another category: not kept at all.
  const env2 = await environment();
  const other = await seal(env2, { category: 'scam', uploadIp: IP });
  assert.deepEqual([other.ok, other.upload_ip_sealed], [true, false]);
  assert.ok(!plainStorage(env2).includes(IP));
  assert.deepEqual((await readEvidence(env2, { caseRef: other.reference, adminId: ADMIN, legalProcess: LEGAL })).items[0].facts.uploader.upload_ips, []);

  // A seal that fails without the vault key keeps no address in its pending record.
  const env3 = await environment();
  delete env3.JWT_SECRET;
  const pending = await quiet(() => sealEvidence(env3, { caseKind: 'forum', caseId: POST, category: 'csam', uploader: { account_id: AUTHOR, upload_ips: [{ ip: IP, at: '2026-10-01T00:00:00.000Z', item_id: null }] }, items: [{ id: 'post-text', text: TEXT }] }));
  assert.deepEqual(pending.pending, ['post-text']);
  const rec = [...env3.SOCIAL.store.entries()].find(([k]) => k.startsWith(PENDING_PREFIX));
  assert.ok(rec, 'the failure is recorded');
  assert.ok(!plainStorage(env3).includes(IP), 'without the key the address is not kept');
});

// Everything stored in the clear except the legal reports, which carry the address by design.
const plainOutsideReports = (env) => plainStorage(env).split('\n').filter((line) => !line.startsWith('legalreport:')).join('\n');
const remove = (env, extra) => quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', reviewerId: ADMIN, postBody: TEXT, ...extra }));

test('resolveForumCase: a valid uploadIp on a csam removal is sealed into the facts and reaches the legal report', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'csam');
  const IP = '203.0.113.50';
  const r = await remove(env, { category: 'csam', uploadIp: IP });
  assert.equal(r.ok, true);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.decisions.at(-1).upload_ip_kept, true);
  const [rep, ...more] = await reportsOf(env);
  assert.equal(more.length, 0);
  assert.deepEqual(rep.who.upload_ips, [legacyEntry(IP)], 'the report carries the address, time not recorded (legacy)');
  assert.equal(rep.who.upload_ip_note, undefined);
  // The text was sealed at the csam report, before the address was known; a sealed item is never rewritten, so the address is
  // sealed beside it as its own item.
  const read = await readEvidence(env, { caseRef: rec.reference, adminId: ADMIN, legalProcess: LEGAL });
  const textItem = keyedItem(TEXT);
  const entries = [legacyEntry(IP)];
  assert.deepEqual(read.items.map((i) => i.index.item).sort(), [textItem, ipItemOf(textItem, entries)].sort());
  assert.ok(!ipItemOf(textItem, entries).includes(IP), 'the item id names no address');
  const ipItem = read.items.find((i) => i.index.item === ipItemOf(textItem, entries));
  assert.deepEqual(ipItem.facts.uploader.upload_ips, entries, 'sealed into the encrypted facts');
  assert.deepEqual(JSON.parse(dec.decode(ipItem.content)), { upload_ips: entries, item: textItem });
  assert.ok(rep.evidence.items.includes(ipItemOf(textItem, entries)), 'the report names the sealed item');
  assert.ok(!plainOutsideReports(env).includes(IP), 'and in nothing else stored in the clear');
  // A post sealed for the first time at the removal carries it in the text's own facts.
  const env3 = await environment();
  await report(env3, REPORTERS[0], 'spam');
  const sealedFirst = await remove(env3, { category: 'csam', uploadIp: IP, postBody: `${TEXT} again` });
  assert.equal(sealedFirst.ok, true);
  const read3 = await readEvidence(env3, { caseRef: sealedFirst.reference, adminId: ADMIN, legalProcess: LEGAL });
  assert.deepEqual(read3.items.map((i) => [i.index.item, i.facts.uploader.upload_ips]), [[keyedItem(`${TEXT} again`), entries]], 'one item, the address in its facts');
  // nonconsensual_sexual seals it the same way (no report: there is no reporting duty).
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'nonconsensual_sexual');
  assert.equal((await remove(env2, { category: 'nonconsensual_sexual', uploadIp: '2001:DB8::7' })).ok, true);
  const rec2 = await loadForumCase(env2, POST);
  const facts2 = (await readEvidence(env2, { caseRef: rec2.reference, adminId: ADMIN, legalProcess: LEGAL })).items.map((i) => i.facts.uploader.upload_ips);
  assert.ok(facts2.some((f) => JSON.stringify(f) === JSON.stringify([legacyEntry('2001:db8::7')])));
});

test('resolveForumCase: an invalid uploadIp is dropped, never refused, and the removal stands', async () => {
  for (const bad of ['999.1.1.1', 'not an address', '1.2.3.4:80', 42, { ip: '1.2.3.4' }]) {
    const env = await environment();
    await report(env, REPORTERS[0], 'csam');
    const r = await remove(env, { category: 'csam', uploadIp: bad });
    assert.deepEqual([r.ok, r.action], [true, 'remove'], JSON.stringify(bad));
    assert.equal((await forumPostModeration({ env, postId: POST })).removed, true, 'the decision stands');
    const rec = await loadForumCase(env, POST);
    assert.equal(rec.decisions.at(-1).upload_ip_kept, false);
    const [rep] = await reportsOf(env);
    assert.deepEqual(rep.who.upload_ips, []);
    assert.deepEqual((await readEvidence(env, { caseRef: rec.reference, adminId: ADMIN, legalProcess: LEGAL })).items[0].facts.uploader.upload_ips, []);
    if (typeof bad === 'string') assert.ok(!plainStorage(env).includes(bad), 'the bad value is stored nowhere');
  }
});

test('resolveForumCase: a removal for any other category, or another decision, does not keep uploadIp', async () => {
  const IP = '198.51.100.23';
  for (const [decision, category] of [['remove', 'threat'], ['remove', 'scam'], ['remove', 'personal_info'], ['remove', 'spam'], ['gate', 'adult_sexual'], ['restore', null]]) {
    const env = await environment();
    await report(env, REPORTERS[0], 'spam');
    const r = await quiet(() => resolveForumCase({ env, postId: POST, decision, category, reviewerId: ADMIN, postBody: TEXT, uploadIp: IP }));
    assert.equal(r.ok, true, `${decision} ${category}`);
    assert.equal((await loadForumCase(env, POST)).decisions.at(-1).upload_ip_kept, false, `${decision} ${category}`);
    assert.ok(!plainStorage(env).includes(IP), `${decision} ${category}: the address is stored nowhere`);
  }
});

// -- ask (b): the item id follows the text ------------------------------------------------------------------------------------

test('the sealed item id follows the text: a keyed hash, the same text is idempotent, an edited text is a new item', async () => {
  const env = await environment();
  const item = await forumTextItemId(env, TEXT);
  assert.equal(item, keyedItem(TEXT));
  assert.ok(item.startsWith(FORUM_TEXT_ITEM_PREFIX));
  assert.ok(!item.includes(crypto.createHash('sha256').update(TEXT, 'utf8').digest('hex').slice(0, 32)), 'never the plain SHA-256');
  assert.equal(await forumTextItemId({ JWT_SECRET: 'another-secret' }, TEXT) === item, false, 'keyed');

  const first = await seal(env);
  assert.deepEqual([first.ok, first.item_id], [true, item]);
  const index = (it) => env.SCENES.objects.has(`${VAULT_PREFIX}${first.reference}/${it}.json`);
  assert.ok(index(item));
  const parts = () => [...env.SCENES.objects.keys()].filter((k) => k.startsWith(VAULT_PREFIX)).length;
  const before = parts();
  const again = await seal(env);
  assert.deepEqual([again.ok, again.item_id, (await loadForumCase(env, POST)).evidence_seal.already], [true, item, [item]], 'sealed twice: already sealed');
  assert.equal(parts(), before, 'nothing new was written to the vault');

  const edited = await seal(env, { postBody: `${TEXT} edited` });
  assert.equal(edited.item_id, keyedItem(`${TEXT} edited`));
  assert.notEqual(edited.item_id, item);
  assert.deepEqual((await loadForumCase(env, POST)).evidence_seal.sealed, [edited.item_id], 'the edited text is sealed as a new item');
  assert.ok(index(item) && index(edited.item_id), 'both texts are sealed');
  assert.deepEqual((await loadForumCase(env, POST)).sealed_items, [item, edited.item_id]);
  const read = await readEvidence(env, { caseRef: first.reference, adminId: ADMIN, legalProcess: LEGAL });
  assert.deepEqual(read.items.map((i) => dec.decode(i.content)).sort(), [TEXT, `${TEXT} edited`].sort());

  // A caller may name the item; a name the vault would not take is refused.
  assert.equal((await seal(env, { itemId: 'post-text-forum-v7' })).item_id, 'post-text-forum-v7');
  for (const bad of ['Post-Text', '-x', 'a/b', 'x'.repeat(65), 42]) assert.equal((await seal(env, { itemId: bad })).code, 'bad_item', String(bad));
  // The report path seals under the same id.
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'csam');
  assert.equal((await loadForumCase(env2, POST)).evidence_seal.item_id, item);
});

// -- ask (c): case versions and the open list ---------------------------------------------------------------------------------

test('every snapshot carries case_version, and seq rises on every write', async () => {
  const env = await environment();
  assert.deepEqual((await forumPostModeration({ env, postId: POST })).case_version, { round: 0, seq: 0 }, 'no case');
  const seen = [];
  const r1 = await report(env, REPORTERS[0], 'threat'); seen.push(r1.case_version);
  const r2 = await report(env, REPORTERS[1], 'threat'); seen.push(r2.case_version);
  seen.push((await forumPostModeration({ env, postId: POST })).case_version);
  const s1 = await seal(env, { category: 'threat' }); seen.push(s1.case_version);
  const d1 = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'gate', category: 'adult_sexual', reviewerId: ADMIN })); seen.push(d1.case_version);
  const r3 = await report(env, REPORTERS[2], 'spam'); seen.push(r3.case_version);
  const d2 = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'restore', reviewerId: ADMIN })); seen.push(d2.case_version);
  seen.push((await forumPostModeration({ env, postId: POST })).case_version);
  for (const v of seen) assert.deepEqual(Object.keys(v), ['round', 'seq']);
  assert.deepEqual(seen.map((v) => v.round), [0, 0, 0, 0, 1, 1, 2, 2]);
  // Strictly rising across writes; a read answers the latest write's.
  assert.ok(r1.case_version.seq >= 1);
  assert.ok(r2.case_version.seq > r1.case_version.seq);
  assert.equal(seen[2].seq, r2.case_version.seq);
  assert.ok(s1.case_version.seq > r2.case_version.seq);
  assert.ok(d1.case_version.seq > s1.case_version.seq);
  assert.ok(r3.case_version.seq > d1.case_version.seq);
  assert.ok(d2.case_version.seq > r3.case_version.seq);
  // The decision answers the version its own write made; the push record is the one write after it.
  assert.equal(seen[7].seq, d2.case_version.seq + 1);
  assert.equal(stored(env, forumCaseR2Key(POST)).seq, seen[7].seq);
  assert.equal(env.SCENES.objects.get(forumCaseR2Key(POST)).customMetadata.seq, String(seen[7].seq));
  // A repeat writes nothing and answers the case as it is.
  const repeat = await report(env, REPORTERS[0], 'threat');
  assert.deepEqual(repeat.case_version, seen[7]);
  // A removal by the check carries it too.
  const removed = await quiet(() => recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: TEXT, check: { verdict: 'refuse', category: 'personal_info', confidence: 0.9, confidence_band: 'high' } }));
  assert.ok(removed.case_version.seq > seen[7].seq);
});

test('listOpenForumCases pages through the open cases, bounded, with no bodies, notes or reporters', async () => {
  const env = await environment();
  const post = (n) => `${THREAD}-${n}`;
  const file = (n, reporterId, category, note = null) => quiet(() => fileForumReport({ env, postId: post(n), threadId: THREAD, postAuthorId: AUTHOR, reporterId, category, note, postBody: TEXT, now: NOW }));
  await file(1, REPORTERS[0], 'csam', 'NOTE-CANARY-1');          // hidden
  await file(2, REPORTERS[0], 'spam');                            // waiting for a person
  await file(3, REPORTERS[0], 'spam');                            // decided and restored: closed
  await quiet(() => resolveForumCase({ env, postId: post(3), decision: 'restore', reviewerId: ADMIN }));
  for (const id of REPORTERS.slice(0, 3)) await file(4, id, 'adult_sexual');   // gated
  await file(5, REPORTERS[0], 'spam');                            // decided, then held by a TAKE IT DOWN lock
  await quiet(() => resolveForumCase({ env, postId: post(5), decision: 'restore', reviewerId: ADMIN }));
  await env.SOCIAL.put(forumPostLockKey(post(5)), JSON.stringify({ reason: 'takedown', reference: 'EC-0000-0000-0000-0000-0000', locked_at: new Date(NOW).toISOString() }));

  const all = [];
  let cursor = null;
  let pages = 0;
  do {
    const page = await listOpenForumCases({ env, cursor, limit: 2 });
    assert.equal(page.error, undefined);
    all.push(...page.cases);
    cursor = page.next_cursor;
    pages += 1;
  } while (cursor && pages < 10);
  assert.equal(pages, 3, 'five cases, two a page');
  assert.deepEqual(all.map((c) => c.postId), [post(1), post(2), post(4), post(5)], 'the restored case is closed and not listed');
  for (const c of all) assert.deepEqual(Object.keys(c), ['postId', 'reference', 'case_version', 'hidden', 'gated', 'removed', 'locked']);
  const byPost = Object.fromEntries(all.map((c) => [c.postId, c]));
  assert.deepEqual([byPost[post(1)].hidden, byPost[post(4)].gated, byPost[post(5)].locked, byPost[post(2)].hidden], [true, true, true, false]);
  assert.equal(byPost[post(5)].case_version.round, 1);
  for (const c of all) assert.deepEqual(c.case_version, (await forumPostModeration({ env, postId: c.postId })).case_version);
  const text = JSON.stringify(all);
  for (const secret of ['WOMBAT', 'NOTE-CANARY', ...REPORTERS, AUTHOR, ADMIN]) assert.ok(!text.includes(secret), secret);

  // Bounded: a page reads at most FORUM_OPEN_LIST_MAX.
  assert.equal(FORUM_OPEN_LIST_MAX, 100);
  env.SCENES.hooks.lists.length = 0;
  await listOpenForumCases({ env, limit: 5000 });
  await listOpenForumCases({ env, limit: 0 });
  await listOpenForumCases({ env });
  assert.deepEqual(env.SCENES.hooks.lists.map((l) => [l.prefix, l.limit]), [[FORUM_CASE_PREFIX, 100], [FORUM_CASE_PREFIX, 1], [FORUM_CASE_PREFIX, 50]]);
  assert.deepEqual(await listOpenForumCases({ env, cursor: 42 }), { cases: [], next_cursor: null, error: 'bad_cursor' });
  assert.deepEqual((await listOpenForumCases({ env, limit: 100 })).next_cursor, null);
});

// -- round 5: the lease is released before the push; the report carries the address; the csam silence ------------------------

const IP5 = '192.0.2.61';
const adminTool = (env) => makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN });
// The forum's applier as Website builds it: a removal in a sealed category seals the post, with its address, before the post
// is taken down. A seal refused (busy) fails the push.
const sealingApplier = (ip = IP5) => async ({ env, postId, action }) => {
  if (action !== 'remove') return;
  const r = await sealForumEvidence({ env, postId, threadId: THREAD, postAuthorId: AUTHOR, postBody: TEXT, category: 'csam', reviewerId: ADMIN, uploadIp: ip });
  if (!r.ok) throw new Error(`seal refused: ${r.code}`);
};

test('R5-4: an admin-tool csam removal whose applier seals the post succeeds: no busy, no push_failed, and the report carries the forum\'s address', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  const kvEntry = { ip: IP5, at: new Date(Date.now() - 86400 * 1000).toISOString(), item_id: keyedItem(TEXT) };
  await env.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify(kvEntry));
  assert.equal(forumPostIpKey(POST, keyedItem(TEXT)), `forum-post-ip:${POST}:${keyedItem(TEXT)}`);
  assert.equal(forumPostIpPrefix(POST), `forum-post-ip:${POST}:`);
  registerForumDecisionApplier(sealingApplier(null));
  try {
    const r = await quiet(() => adminTool(env)('moderation_forum_resolve', { post_id: POST, decision: 'remove', category: 'csam', rationale: 'Sexual content involving a minor, confirmed.' }));
    assert.deepEqual([r.ok, r.action], [true, 'remove'], JSON.stringify(r));
  } finally {
    registerForumDecisionApplier(null);
  }
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.decision_push.status, 'decision_pushed', JSON.stringify(rec.decision_push));
  assert.ok(!rec.history.some((h) => h.event === 'push_failed'));
  assert.ok(rec.history.some((h) => h.event === 'evidence_sealed'), 'the applier sealed the post');
  assert.equal(rec.decisions.at(-1).upload_ip_kept, true);
  const [rep, ...more] = await reportsOf(env);
  assert.equal(more.length, 0);
  assert.deepEqual(rep.who.upload_ips, [kvEntry], 'the report carries the forum\'s pair for the text the applier sealed');
  assert.ok(rep.evidence.items.includes(keyedItem(TEXT)), 'and names the text the applier sealed');
  assert.deepEqual(rep.who.upload_ip_unknown_items, [], 'every sealed text has its address');
  assert.equal(rep.evidence.sealed, true);
});

test('R5-4: with no uploadIp passed, the forum-post-ip version key is read under the same validation and the same illegal-only rule', async () => {
  const recent = new Date(Date.now() - 3600 * 1000).toISOString();
  // The forum's pair for the text sealed, on a csam removal: carried with its time.
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  const pair = { ip: '2001:DB8::61', at: recent, item_id: keyedItem(TEXT) };
  await env.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify(pair));
  assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }))).ok, true);
  const expected = [{ ip: '2001:db8::61', at: recent, item_id: keyedItem(TEXT) }];
  assert.deepEqual((await reportsOf(env))[0].who.upload_ips, expected);
  const rec = await loadForumCase(env, POST);
  assert.deepEqual((await readEvidence(env, { caseRef: rec.reference, adminId: ADMIN, legalProcess: LEGAL })).items[0].facts.uploader.upload_ips, expected);
  // Not an address: dropped, and the removal stands.
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'spam');
  await env2.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify({ ...pair, ip: 'not-an-address' }));
  assert.equal((await quiet(() => resolveForumCase({ env: env2, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }))).ok, true);
  assert.deepEqual((await reportsOf(env2))[0].who.upload_ips, []);
  // Another category: not kept.
  const env3 = await environment();
  await report(env3, REPORTERS[0], 'spam');
  await env3.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify({ ...pair, ip: IP5 }));
  await quiet(() => resolveForumCase({ env: env3, postId: POST, decision: 'remove', category: 'threat', reviewerId: ADMIN, postBody: TEXT }));
  assert.equal((await loadForumCase(env3, POST)).decisions.at(-1).upload_ip_kept, false);
  assert.ok(!plainStorage(env3).split('\n').filter((l) => !l.startsWith(forumPostIpPrefix(POST))).join('\n').includes(IP5), 'stored nowhere but the forum\'s own key');
});

test('R5-4: a report not yet sent is amended with a later address; a sent one is never edited and gets an addendum', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  // A staff csam removal with no address anywhere: the report says none was given.
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  let [rep] = await reportsOf(env);
  assert.deepEqual([rep.status, rep.who.upload_ips], ['ready_to_send', []]);
  // Said per sealed text item, not as a sentence for the whole report.
  assert.equal(rep.who.upload_ip_note, undefined);
  assert.deepEqual(rep.who.upload_ip_unknown_items, [{ item_id: keyedItem(TEXT), upload_ip: null, reason: 'not_known' }]);
  // The address arrives with a later seal: the report still waiting is amended.
  const sealed = await seal(env, { uploadIp: IP5 });
  assert.equal(sealed.ok, true);
  [rep] = await reportsOf(env);
  assert.deepEqual([rep.status, rep.who.upload_ips.map((x) => x.ip), rep.addenda], ['ready_to_send', [IP5], undefined]);
  // A legacy address names no text, so the text's own address is still not known.
  assert.deepEqual(rep.who.upload_ip_unknown_items, [{ item_id: keyedItem(TEXT), upload_ip: null, reason: 'not_known' }]);
  assert.ok(rep.history.some((h) => h.event === 'amended'));
  // The same address again changes nothing.
  assert.equal((await amendLegalReportUploadIps(env, { kind: 'csam', caseKind: 'forum', caseId: POST, uploadIps: [{ ip: IP5 }] })).reason, 'nothing_new');
  // Sent: never edited. A new address is an addendum, and staff are told it waits to be sent.
  await env.SOCIAL.put(`legalreport:${rep.id}`, JSON.stringify({ ...rep, status: 'sent', sent_at: new Date().toISOString() }));
  const sentBefore = await loadLegalReport(env, rep.id);
  const later = '198.51.100.200';
  assert.equal((await seal(env, { uploadIp: later })).ok, true);
  const after = await loadLegalReport(env, rep.id);
  assert.deepEqual(after.who, sentBefore.who, 'the sent report is not edited');
  assert.equal(after.status, 'sent');
  // The address and the item that seals it beside the text, both new to the sent report.
  assert.deepEqual(after.addenda.map((a) => [a.kind, a.upload_ips.map((x) => x.ip), a.items, a.sent]), [['upload_ips_and_evidence_items', [later], [ipItemOf(keyedItem(TEXT), [legacyEntry(later)])], false]]);
  assert.ok(after.history.some((h) => h.event === 'addendum'));
  assert.ok(await env.SOCIAL.get(`legalalert:${rep.id}:addendum:1`), 'staff were told');
  // The later address is sealed too, as its own item beside the text.
  const read = await readEvidence(env, { caseRef: sealed.reference, adminId: ADMIN, legalProcess: LEGAL });
  assert.ok(read.items.some((i) => i.index.item === ipItemOf(keyedItem(TEXT), [legacyEntry(later)]) && i.facts.uploader.upload_ips[0]?.ip === later));
});

test('R5-2: resolveForumCase answers the case_version its own write made, not a later one', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  registerForumDecisionApplier(sealingApplier());
  let r;
  try {
    r = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN }));
  } finally {
    registerForumDecisionApplier(null);
  }
  const decisionWrite = env.SCENES.hooks.puts.filter((p) => p.key === forumCaseR2Key(POST) && p.body).map((p) => JSON.parse(p.body)).find((b) => b.decisions?.length === 1);
  assert.ok(decisionWrite, 'the decision\'s write');
  assert.deepEqual(r.case_version, { round: decisionWrite.round, seq: decisionWrite.seq });
  assert.equal(r.case_version.round, 1);
  assert.ok((await loadForumCase(env, POST)).seq > r.case_version.seq, 'later writes (the seal, the push record) came after');
});

// The author's own case history entries, as the route answers them.
async function authorEntries(env) {
  const deps = { verifyAuth: async () => AUTHOR, json: (body, status = 200) => new Response(JSON.stringify(body), { status }) };
  const url = new URL('https://api.eustress.dev/api/account/cases');
  return (await (await handleCaseRoute(new Request(url, { method: 'GET' }), url, env, {}, deps)).json()).cases;
}
// An entry with what may differ (ids and times) taken out.
const bareEntry = (e) => JSON.stringify({ ...e, reference: null, post: null, opened_at: null, decided_at: null, updated_at: null });

test('R8-3 (removals): a person\'s removal in csam or nonconsensual_sexual shows one byte-identical generic entry at once; other categories stay named', async () => {
  const viewOf = async (category, seed) => {
    const env = await environment();
    await seed(env);
    assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category, reviewerId: ADMIN }))).ok, true, category);
    const cases = await authorEntries(env);
    assert.equal(cases.length, 1, `${category}: told at once`);
    return cases[0];
  };
  // An unreported post (a staff seal under another category makes its case), and a reported one.
  for (const seed of [(env) => seal(env, { category: 'personal_info' }), (env) => report(env, REPORTERS[0], 'spam')]) {
    const csam = await viewOf('csam', seed);
    const ncii = await viewOf('nonconsensual_sexual', seed);
    assert.equal(bareEntry(csam), bareEntry(ncii), 'byte-identical but for ids and times');
    assert.deepEqual([csam.status, csam.outcome, csam.headline], ['under_review', 'removed', 'Your forum post was removed while a person reviews it.']);
    assert.doesNotMatch(bareEntry(csam), /A1|A5|minor|sexual|consent|csam/i);
    for (const named of ['threat', 'scam', 'personal_info', 'spam']) {
      const e = await viewOf(named, seed);
      assert.match(e.feedback.reasons[0].why, /Code (A2|A6|A7|B4)/, `${named} stays named`);
      assert.equal(e.headline, 'A moderator removed your forum post.');
    }
  }
});

test('R8-3 (seals): a seal in csam or nonconsensual_sexual of a post that is down shows the same generic entry; a seal never changes one shown', async () => {
  // A post already down with nothing shown to its author (its case written down by another path): the seal shows the generic
  // entry.
  const down = async (category) => {
    const env = await environment();
    await seal(env, { category: 'personal_info' });
    await env.SCENES.rewriteCase(POST, (r) => { r.removed = true; r.hidden = true; r.removed_by = 'person'; r.shown_entries = []; });
    await seal(env, { category, postBody: `${TEXT} ${category}` });
    return (await authorEntries(env))[0];
  };
  const csam = await down('csam');
  const ncii = await down('nonconsensual_sexual');
  assert.ok(csam && ncii, 'both are told');
  assert.equal(bareEntry(csam), bareEntry(ncii));
  assert.equal(csam.headline, 'Your forum post was removed while a person reviews it.');
  // A seal of a post that is up shows nothing; a seal never changes an entry already shown.
  const up = await environment();
  await seal(up, { category: 'csam' });
  assert.deepEqual(await authorEntries(up), [], 'nothing happened to the post');
  const shown = await environment();
  await report(shown, REPORTERS[0], 'nonconsensual_sexual');
  const before = await authorEntries(shown);
  await seal(shown, { category: 'csam' });
  assert.deepEqual(await authorEntries(shown), before, 'the seal changed nothing shown');
});

// -- round 6 -------------------------------------------------------------------------------------------------------------------

test('R6-1 (now): a later removal under another category is a person\'s decision and shows its own entry; nothing is ever silenced', async () => {
  const env = await environment();
  await seal(env, { category: 'personal_info' });
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN }));
  assert.equal((await authorEntries(env))[0].headline, 'Your forum post was removed while a person reviews it.');
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'spam', reviewerId: ADMIN }));
  assert.match((await authorEntries(env))[0].feedback.reasons[0].why, /spam \(Code B4\)/);
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'restore', reviewerId: ADMIN }));
  assert.equal((await authorEntries(env))[0].outcome, 'restored');
  // Every entry it showed is on the case, in order.
  assert.deepEqual((await loadForumCase(env, POST)).shown_entries.map((e) => [e.kind, e.category || e.decision]), [['person_removed', 'csam'], ['person_removed', 'spam'], ['person_decided', 'restore']]);
});

const DAY_MS = 86400 * 1000;
const iso = (ms) => new Date(ms).toISOString();

test('R6-4: each address is reported with its own time only: written on day 0 from A, edited on day 20 from B, removed on day 25', async () => {
  const day25 = Date.now();
  const day0 = day25 - 25 * DAY_MS;
  const day20 = day25 - 5 * DAY_MS;
  const A = '203.0.113.10';
  const B = '198.51.100.20';
  const v1 = 'WOMBAT-CANARY first text';
  const v2 = 'WOMBAT-CANARY edited text';
  const versions = [{ ip: A, at: iso(day0), item_id: keyedItem(v1) }, { ip: B, at: iso(day20), item_id: keyedItem(v2) }];
  for (const via of ['call', 'kv']) {
    const env = await environment();
    await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTERS[0], category: 'spam', postBody: v1, postCreatedAt: iso(day0), now: day0 + 1000 }));
    if (via === 'kv') for (const v of versions) await env.SOCIAL.put(forumPostIpKey(POST, v.item_id), JSON.stringify(v));
    const r = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: v2, ...(via === 'call' ? { uploadIps: versions } : {}) }));
    assert.equal(r.ok, true, via);
    const [rep] = await reportsOf(env);
    // Passed by the forum: both pairs, as given. Read from the forum's keys: only the pair of the text sealed (v2), never A's,
    // whose text is not sealed (R7-3).
    const expected = via === 'call' ? versions : [versions[1]];
    assert.deepEqual(rep.who.upload_ips, expected, `${via}: each address at its own time`);
    assert.ok(!rep.who.upload_ips.some((x) => x.ip === B && x.at === iso(day0)), `${via}: never B at day 0`);
    assert.equal(rep.when_where.uploaded_at, iso(day0), 'the post\'s creation time is the post\'s, not an address\'s');
    const facts = (await readEvidence(env, { caseRef: rep.when_where.case_reference, adminId: ADMIN, legalProcess: LEGAL })).items.find((i) => i.index.item === keyedItem(v2)).facts;
    assert.deepEqual(facts.uploader.upload_ips, expected, `${via}: the sealed facts hold the same entries`);
  }
});

test('R6-4: the legacy address has at: null, never the post\'s creation time; invalid and future times are dropped', async () => {
  const now = Date.now();
  // Legacy uploadIp, on a post whose creation time the case knows: still no time.
  const env = await environment();
  await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTERS[0], category: 'spam', postBody: TEXT, postCreatedAt: iso(now - 3 * DAY_MS), now }));
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, uploadIp: '203.0.113.99' }));
  assert.deepEqual((await reportsOf(env))[0].who.upload_ips, [legacyEntry('203.0.113.99')]);
  // The check's removal with a creation time and a legacy address: no time either.
  const env2 = await environment();
  await quiet(() => recordForumRemoval({ env: env2, postId: POST, threadId: THREAD, authorId: AUTHOR, text: TEXT, check: { verdict: 'refuse', category: 'csam', confidence: 0.97, confidence_band: 'high' }, uploadIp: '203.0.113.98', postedAt: iso(now - DAY_MS) }));
  assert.deepEqual((await reportsOf(env2))[0].who.upload_ips, [legacyEntry('203.0.113.98')]);

  // The entry rules.
  const good = { ip: '192.0.2.1', at: iso(now - 1000), item_id: keyedItem(TEXT) };
  assert.deepEqual(forumUploadIpsOf([good], now), [good]);
  assert.deepEqual(forumUploadIpsOf([{ ...good, item_id: undefined }], now), [{ ...good, item_id: null }], 'item_id is optional');
  for (const bad of [
    { ...good, at: iso(now + 60_000) },             // in the future
    { ...good, at: null },                           // no time: dropped, never given one
    { ...good, at: '2026-10-01' },                   // not a time
    { ...good, at: '2026-10-01T00:00:00+02:00' },    // not UTC
    { ...good, at: '2026-13-45T99:00:00Z' },         // does not parse
    { ...good, at: 1760000000000 },                  // not a string
    { ...good, ip: '999.1.1.1' },
    { ...good, item_id: 'post-text' },
    { ...good, item_id: 'other-item' },
    null, 'x', 42,
  ]) assert.deepEqual(forumUploadIpsOf([bad], now), [], JSON.stringify(bad));
  assert.deepEqual(forumUploadIpsOf([good, good], now), [good], 'each entry once');
  assert.equal(forumUploadIpsOf(Array.from({ length: 30 }, (_, i) => ({ ip: `192.0.2.${i + 1}`, at: iso(now - 1000) })), now).length, FORUM_UPLOAD_IPS_MAX);
  assert.equal(FORUM_UPLOAD_IPS_MAX, 20);
  // Through the call: only invalid entries, and the removal still stands with no address.
  const env4 = await environment();
  await report(env4, REPORTERS[0], 'spam');
  const r = await quiet(() => resolveForumCase({ env: env4, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, uploadIps: [{ ip: '192.0.2.5', at: iso(now + DAY_MS) }, { ip: '192.0.2.6' }] }));
  assert.equal(r.ok, true);
  assert.deepEqual((await reportsOf(env4))[0].who.upload_ips, []);
  assert.ok(!plainStorage(env4).includes('192.0.2.5') && !plainStorage(env4).includes('192.0.2.6'));
  // uploadIps passed (even empty) means the forum's key is not read.
  const env5 = await environment();
  await report(env5, REPORTERS[0], 'spam');
  await env5.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify({ ip: '192.0.2.7', at: iso(now - 1000), item_id: keyedItem(TEXT) }));
  await quiet(() => resolveForumCase({ env: env5, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, uploadIps: [] }));
  assert.deepEqual((await reportsOf(env5))[0].who.upload_ips, []);
});

test('R6-4: amendments are de-duplicated on (ip, at, item_id); a sent report gets an addendum', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  const t1 = iso(Date.now() - 10 * DAY_MS);
  const t2 = iso(Date.now() - 2 * DAY_MS);
  const e1 = { ip: '192.0.2.40', at: t1, item_id: keyedItem(TEXT) };
  const amend = (uploadIps) => amendLegalReport(env, { kind: 'csam', caseKind: 'forum', caseId: POST, uploadIps });
  assert.equal((await amend([e1])).upload_ips, 1);
  assert.equal((await amend([e1])).reason, 'nothing_new', 'the same (ip, at, item_id) again');
  assert.equal((await amend([{ ...e1, at: t2 }])).upload_ips, 1, 'the same address at another time is another entry');
  assert.equal((await amend([{ ...e1, item_id: null }])).upload_ips, 1, 'the same address with no text is another entry');
  let [rep] = await reportsOf(env);
  assert.deepEqual(rep.who.upload_ips, [e1, { ...e1, at: t2 }, { ...e1, item_id: null }]);
  await env.SOCIAL.put(`legalreport:${rep.id}`, JSON.stringify({ ...rep, status: 'sent' }));
  assert.equal((await amend([e1])).reason, 'nothing_new', 'already on the sent report');
  const add = await amend([{ ip: '192.0.2.41', at: t2, item_id: null }]);
  assert.deepEqual([add.how, add.upload_ips], ['addendum', 1]);
  assert.equal((await amend([{ ip: '192.0.2.41', at: t2, item_id: null }])).reason, 'nothing_new', 'already on an addendum');
  [rep] = await reportsOf(env);
  assert.deepEqual(rep.who.upload_ips.length, 3, 'the sent report is not edited');
  assert.deepEqual(rep.addenda.map((a) => a.upload_ips), [[{ ip: '192.0.2.41', at: t2, item_id: null }]]);
});

test('item 6: after a failed push, the seal that comes later amends the report\'s sealed text items; a sent report gets an addendum', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  const A = { ip: '192.0.2.50', at: iso(Date.now() - 3 * DAY_MS), item_id: keyedItem(TEXT) };
  // The push fails: the forum's applier could not seal, and the report is written without the text.
  registerForumDecisionApplier(async () => { throw new Error('forum down'); });
  try {
    assert.equal((await quiet(() => adminTool(env)('moderation_forum_resolve', { post_id: POST, decision: 'remove', category: 'csam', rationale: 'Sexual content involving a minor, confirmed.' }))).ok, true);
  } finally {
    registerForumDecisionApplier(null);
  }
  assert.equal((await loadForumCase(env, POST)).decision_push.status, 'push_failed');
  let [rep] = await reportsOf(env);
  assert.deepEqual([rep.status, rep.evidence.items, rep.evidence.sealed], ['ready_to_send', [], false]);
  // The forum retries: the seal amends the report still waiting.
  assert.equal((await seal(env, { now: Date.now(), uploadIps: [A] })).ok, true);
  [rep] = await reportsOf(env);
  assert.deepEqual([rep.status, rep.evidence.items, rep.evidence.sealed, rep.who.upload_ips], ['ready_to_send', [keyedItem(TEXT)], true, [A]]);
  assert.ok(rep.history.some((h) => h.event === 'amended' && h.detail.items === 1));
  // Sent: a later text is an addendum, the report is not edited, and staff are told.
  await env.SOCIAL.put(`legalreport:${rep.id}`, JSON.stringify({ ...rep, status: 'sent' }));
  const before = await loadLegalReport(env, rep.id);
  assert.equal((await seal(env, { now: Date.now(), postBody: `${TEXT} v2`, uploadIps: [] })).ok, true);
  const after = await loadLegalReport(env, rep.id);
  assert.deepEqual([after.evidence, after.who], [before.evidence, before.who]);
  assert.deepEqual(after.addenda.map((a) => [a.kind, a.items, a.sealed, a.sent]), [['evidence_items', [keyedItem(`${TEXT} v2`)], true, false]]);
  assert.ok(await env.SOCIAL.get(`legalalert:${rep.id}:addendum:1`), 'staff were told');
  // Sealing the same text again adds nothing.
  await seal(env, { now: Date.now(), postBody: `${TEXT} v2`, uploadIps: [] });
  assert.equal((await loadLegalReport(env, rep.id)).addenda.length, 1);
});

// -- round 7 -------------------------------------------------------------------------------------------------------------------

// KV that pages its list as Cloudflare's does: at most `size` keys a page, a cursor, list_complete false until the end.
function paged(kv, size) {
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    const keys = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
    const n = Math.min(limit, size);
    const page = keys.slice(0, n);
    const complete = keys.length <= n;
    return { keys: page.map((name) => ({ name })), list_complete: complete, ...(complete ? {} : { cursor: page.at(-1) }) };
  };
  return kv;
}
const HELD = 'WOMBAT-CANARY a held edit';
const pairFor = (text, ip, ageMs) => ({ ip, at: iso(Date.now() - ageMs), item_id: keyedItem(text) });
const putPair = (env, pair) => env.SOCIAL.put(forumPostIpKey(POST, pair.item_id), JSON.stringify(pair));
const sealedIpsOf = async (env, reference, item) => (await readEvidence(env, { caseRef: reference, adminId: ADMIN, legalProcess: LEGAL })).items.filter((i) => i.index.item.startsWith(item)).flatMap((i) => i.facts.uploader.upload_ips);

test('R7-3: the forum\'s pairs are read only for the text sealed in that call, never another version\'s', async () => {
  // The body's own pair has expired (gone from KV); a held edit's pair is still there. Neither seal path takes it.
  const env = await environment();
  await putPair(env, pairFor(HELD, '192.0.2.70', DAY_MS));
  const sealed = await seal(env, { now: Date.now() });
  assert.equal(sealed.upload_ip_sealed, false);
  assert.deepEqual(await sealedIpsOf(env, sealed.reference, keyedItem(TEXT)), [], 'the body is not sealed with the held edit\'s address');
  assert.ok(!plainStorage(env).split('\n').filter((l) => !l.startsWith(forumPostIpPrefix(POST))).join('\n').includes('192.0.2.70'));
  // The admin-tool path: the applier seals the body with no address passed; the report takes only that text's pair.
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'spam');
  await putPair(env2, pairFor(HELD, '192.0.2.71', DAY_MS));
  const own = pairFor(TEXT, '192.0.2.72', 2 * DAY_MS);
  await putPair(env2, own);
  registerForumDecisionApplier(sealingApplier(null));
  try {
    assert.equal((await quiet(() => adminTool(env2)('moderation_forum_resolve', { post_id: POST, decision: 'remove', category: 'csam', rationale: 'Sexual content involving a minor, confirmed.' }))).ok, true);
  } finally {
    registerForumDecisionApplier(null);
  }
  const [rep] = await reportsOf(env2);
  assert.deepEqual(rep.who.upload_ips, [own], 'the sealed text\'s own pair, and not the held edit\'s');
  // The held edit sealed later is a new item: its pair is added to the report with it.
  await seal(env2, { now: Date.now(), postBody: HELD });
  const [after] = await reportsOf(env2);
  assert.deepEqual(after.who.upload_ips.map((x) => x.ip).sort(), ['192.0.2.71', '192.0.2.72']);
  assert.ok(after.who.upload_ips.every((x) => [keyedItem(TEXT), keyedItem(HELD)].includes(x.item_id)));
});

test('R7-3: a pair more than 30 days old is dropped, read from the forum or passed', async () => {
  assert.equal(FORUM_UPLOAD_IP_MAX_AGE_DAYS, 30);
  const now = Date.now();
  const at = (days) => ({ ip: '192.0.2.80', at: iso(now - days * DAY_MS), item_id: keyedItem(TEXT) });
  assert.deepEqual(forumUploadIpsOf([at(29)], now).length, 1);
  assert.deepEqual(forumUploadIpsOf([at(30)], now).length, 1, 'thirty days to the millisecond is kept');
  assert.deepEqual(forumUploadIpsOf([{ ...at(30), at: iso(now - 30 * DAY_MS - 1) }], now), [], 'a moment past thirty days is not');
  assert.deepEqual(forumUploadIpsOf([at(31), at(400)], now), []);
  const env = await environment();
  await putPair(env, { ...at(31), ip: '192.0.2.81' });
  assert.equal((await seal(env, { now: Date.now() })).upload_ip_sealed, false, 'the forum\'s old pair');
  const env2 = await environment();
  assert.equal((await seal(env2, { now: Date.now(), uploadIps: [{ ...at(31), ip: '192.0.2.82' }] })).upload_ip_sealed, false, 'a passed old pair');
  for (const e of [env, env2]) assert.ok(!/192\.0\.2\.8[12]/.test(plainStorage(e).split('\n').filter((l) => !l.startsWith(forumPostIpPrefix(POST))).join('\n')));
});

test('Required 1: one key per version, read by prefix and paged; the old single key is never read', async () => {
  // The old key, in every form it had: ignored.
  for (const old of ['192.0.2.90', JSON.stringify({ ip: '192.0.2.90' }), JSON.stringify({ versions: [pairFor(TEXT, '192.0.2.90', DAY_MS)] })]) {
    const env = await environment();
    await env.SOCIAL.put(`forum-post-ip:${POST}`, old);
    await report(env, REPORTERS[0], 'spam');
    const r = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
    assert.equal(r.ok, true);
    assert.deepEqual((await reportsOf(env))[0].who.upload_ips, [], old);
    assert.equal((await seal(env, { now: Date.now() })).upload_ip_sealed, false, old);
  }
  // Many versions, listed two keys a page: the sealed text's key, at the end, is reached.
  const env = await environment();
  paged(env.SOCIAL, 2);
  for (let i = 0; i < 6; i++) await putPair(env, pairFor(`${HELD} ${i}`, `192.0.2.${100 + i}`, DAY_MS));
  const own = pairFor(TEXT, '192.0.2.120', DAY_MS);
  await putPair(env, own);
  const listedBefore = (await env.SOCIAL.list({ prefix: forumPostIpPrefix(POST) })).list_complete;
  assert.equal(listedBefore, false, 'the versions take more than one page');
  const r = await seal(env, { now: Date.now() });
  assert.equal(r.upload_ip_sealed, true);
  assert.deepEqual(await sealedIpsOf(env, r.reference, keyedItem(TEXT)), [own]);
  // A key whose value names another item is not this item's pair.
  const env2 = await environment();
  await env2.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify(pairFor(HELD, '192.0.2.121', DAY_MS)));
  assert.equal((await seal(env2, { now: Date.now() })).upload_ip_sealed, false);
});

test('Required 2: the report says per sealed text item whether its address is known, and never that the Worker records none', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'csam');
  await seal(env, { postBody: HELD, uploadIps: [] });
  const own = pairFor(TEXT, '192.0.2.130', DAY_MS);
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, uploadIps: [own] }));
  const [rep] = await reportsOf(env);
  assert.deepEqual(rep.who.upload_ips, [own]);
  assert.deepEqual(rep.who.upload_ip_unknown_items, [{ item_id: keyedItem(HELD), upload_ip: null, reason: 'not_known' }], 'the held text, and only it');
  assert.doesNotMatch(JSON.stringify(rep), /does not record/);
  assert.equal(rep.who.upload_ip_note, undefined);
});

test('Required 4: a check removal as csam or as nonconsensual_sexual gives its author one identical generic entry', async () => {
  const env = await environment();
  const P1 = `${THREAD}-5`;
  const P2 = `${THREAD}-6`;
  await quiet(() => recordForumRemoval({ env, postId: P1, threadId: THREAD, authorId: AUTHOR, text: 'one', check: { verdict: 'refuse', category: 'csam', confidence: 0.97, confidence_band: 'high' } }));
  await quiet(() => recordForumRemoval({ env, postId: P2, threadId: THREAD, authorId: AUTHOR, text: 'two', check: { verdict: 'refuse', category: 'nonconsensual_sexual', confidence: 0.97, confidence_band: 'high' } }));
  const [a, b] = [forumNoticeEntry(await loadForumCase(env, P1)), forumNoticeEntry(await loadForumCase(env, P2))];
  assert.ok(a && b, 'both are told: an absent entry would tell csam apart');
  const bare = (e) => JSON.stringify({ ...e, reference: null, post: null, opened_at: null, decided_at: null, updated_at: null });
  assert.equal(bare(a), bare(b), 'byte-identical but for ids and times');
  assert.doesNotMatch(bare(a), /A1|A5|minor|sexual|consent|csam/i);
  // And through the case history route: two entries, the same.
  const deps = { verifyAuth: async () => AUTHOR, json: (body, status = 200) => new Response(JSON.stringify(body), { status }) };
  const url = new URL('https://api.eustress.dev/api/account/cases');
  const cases = (await (await handleCaseRoute(new Request(url, { method: 'GET' }), url, env, {}, deps)).json()).cases;
  assert.equal(cases.length, 2);
  assert.equal(bare(cases[0]), bare(cases[1]));
  // A reported csam post is no longer silent either (R8-3): it shows the one generic hide entry.
  const env2 = await environment();
  await report(env2, REPORTERS[0], 'csam');
  assert.equal(forumNoticeEntry(await loadForumCase(env2, POST)).headline, 'Your forum post was hidden after a report while a person reviews it.');
});

test('Required 10: the pending seals are paged with a cursor, and every one is reached across runs', async () => {
  const env = await environment();
  paged(env.SOCIAL, 1000);
  const failVault = async (key) => { if (key.startsWith(VAULT_PREFIX)) throw new Error('R2 is unavailable'); };
  env.SCENES.hooks.beforePut = failVault;
  const posts = Array.from({ length: 60 }, (_, i) => `${'d'.repeat(24)}-${i + 1}`);
  for (const p of posts) await quiet(() => sealEvidence(env, { caseKind: 'forum', caseId: p, category: 'csam', items: [{ id: 'post-text', text: `t ${p}` }] }));
  const pendingKeys = () => [...env.SOCIAL.store.keys()].filter((k) => k.startsWith(PENDING_PREFIX));
  assert.equal(pendingKeys().length, 60);
  // Still failing: three runs of one page each try every seal once, then the cursor starts over.
  const runs = [];
  for (let i = 0; i < 3; i++) runs.push(await quiet(() => retryPendingEvidence(env, { limit: 25, maxPages: 1 })));
  assert.deepEqual(runs.map((r) => [r.tried, r.complete]), [[25, false], [25, false], [10, true]]);
  assert.ok(pendingKeys().every((k) => JSON.parse(env.SOCIAL.store.get(k)).attempts === 2), 'every pending seal was reached once');
  assert.equal(await env.SOCIAL.get(PENDING_CURSOR_KEY), null, 'the run that reached the end starts the next from the beginning');
  // The vault is back: one run of several pages seals them all.
  env.SCENES.hooks.beforePut = null;
  const swept = await quiet(() => retryPendingEvidence(env, { limit: 25 }));
  assert.deepEqual([swept.sealed, swept.still_pending, swept.pages, swept.complete], [60, 0, 3, true]);
  assert.equal(pendingKeys().length, 0);
});

// -- round 8: Required 11, a first post's title is sealed beside its body ----------------------------------------------------------

const TITLE = 'WOMBAT-CANARY offending thread title';
const titleItemOf = (title) => `post-title-${keyedHex(title).slice(0, 32)}`;

test('Required 11: a report of post 1 seals its title beside the body, as its own item, under the reports\' detection', async () => {
  const env = await environment();
  const r = await report(env, REPORTERS[0], 'csam', { postTitle: TITLE });
  assert.equal(r.hide, true);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.evidence_seal.ok, true);
  assert.deepEqual([...rec.evidence_seal.sealed].sort(), [keyedItem(TEXT), titleItemOf(TITLE)].sort());
  assert.equal(rec.evidence_seal.title_item_id, titleItemOf(TITLE));
  assert.ok(rec.sealed_items.includes(titleItemOf(TITLE)));
  assert.ok(!titleItemOf(TITLE).includes(crypto.createHash('sha256').update(TITLE, 'utf8').digest('hex').slice(0, 32)), 'a keyed hash, not a plain one');
  const read = await readEvidence(env, { caseRef: rec.reference, adminId: ADMIN, legalProcess: LEGAL });
  const title = read.items.find((i) => i.index.item === titleItemOf(TITLE));
  assert.equal(dec.decode(title.content), TITLE);
  assert.equal(title.facts.detection.source, 'member_reports');
  assert.deepEqual(title.facts.uploader.upload_ips, [], 'no address is paired with a title');
  assert.ok(!plainStorage(env).includes('offending thread title'), 'the title is stored nowhere in the clear');
  // A title the forum_title rules do not allow is dropped, and the report is still filed and sealed.
  for (const bad of ['x'.repeat(121), 42, '   ']) {
    const e = await environment();
    const filed = await report(e, REPORTERS[0], 'csam', { postTitle: bad });
    assert.equal(filed.ok, true);
    const c = await loadForumCase(e, POST);
    assert.deepEqual(c.evidence_seal.sealed, [keyedItem(TEXT)], JSON.stringify(bad));
  }
});

test('Required 11: resolveForumCase and sealForumEvidence take postTitle too; the report lists it with no address', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  const own = pairFor(TEXT, '192.0.2.140', DAY_MS);
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, postTitle: TITLE, uploadIps: [own] }));
  const [rep] = await reportsOf(env);
  assert.ok(rep.evidence.items.includes(titleItemOf(TITLE)), 'a sealed item of the report');
  assert.deepEqual(rep.who.upload_ips, [own], 'the body\'s pair only');
  assert.deepEqual(rep.who.upload_ip_unknown_items, [{ item_id: titleItemOf(TITLE), upload_ip: null, reason: 'not_known' }], 'the title, which has no pair');
  // The same title sealed again is already sealed; another title is a new item.
  const again = await seal(env, { postTitle: TITLE });
  assert.ok((await loadForumCase(env, POST)).evidence_seal.already.includes(titleItemOf(TITLE)));
  assert.equal(again.ok, true);
  await seal(env, { postTitle: `${TITLE} edited` });
  assert.ok((await loadForumCase(env, POST)).evidence_seal.sealed.includes(titleItemOf(`${TITLE} edited`)));
});

// -- forums round 8 (Security): one generic entry, frozen; report_owed; the owed sweep; the forum case read audited -----------

test('R8-3: every report hide or removal shows one byte-identical entry, whatever the category', async () => {
  const views = {};
  for (const [category, reporters] of [['csam', 1], ['nonconsensual_sexual', 1], ['personal_info', 3], ['threat', 3], ['scam', 3]]) {
    const env = await environment();
    for (const id of REPORTERS.slice(0, reporters)) await report(env, id, category);
    const cases = await authorEntries(env);
    assert.equal(cases.length, 1, `${category}: told at once`);
    views[category] = cases[0];
  }
  const want = bareEntry(views.csam);
  for (const [category, e] of Object.entries(views)) assert.equal(bareEntry(e), want, category);
  assert.deepEqual([views.csam.status, views.csam.outcome, views.csam.headline], ['under_review', 'hidden', 'Your forum post was hidden after a report while a person reviews it.']);
  assert.doesNotMatch(want, /A1|A5|A6|A2|A7|minor|sexual|consent|threat|scam|private/i);
});

test('R8-3: an entry once shown is frozen: a later report never withdraws or changes it (variant A), and the case records what was shown', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'nonconsensual_sexual');
  const shown = JSON.stringify(await authorEntries(env));
  // A later csam report, a report that removes, a csam seal: nothing the author sees changes.
  await report(env, REPORTERS[1], 'csam');
  assert.equal(JSON.stringify(await authorEntries(env)), shown, 'the ncii entry did not vanish at a csam report');
  await report(env, REPORTERS[2], 'threat');
  await seal(env, { category: 'csam' });
  assert.equal(JSON.stringify(await authorEntries(env)), shown);
  const rec = await loadForumCase(env, POST);
  assert.deepEqual(rec.shown_entries.map((e) => e.kind), ['report_hidden'], 'one entry shown, and kept');
  // A case written before shown_entries existed keeps the entry its state showed.
  const legacy = await environment();
  await report(legacy, REPORTERS[0], 'nonconsensual_sexual');
  await legacy.SCENES.rewriteCase(POST, (r) => { delete r.shown_entries; });
  const legacyShown = JSON.stringify(await authorEntries(legacy));
  await report(legacy, REPORTERS[1], 'csam');
  assert.equal(JSON.stringify(await authorEntries(legacy)), legacyShown);
  assert.deepEqual((await loadForumCase(legacy, POST)).shown_entries.map((e) => e.kind), ['report_hidden']);
});

test('R8-3: a case the check refused at posting is not decided by a person: 409 check_removed (variant B), and its entry stands', async () => {
  const env = await environment();
  await quiet(() => recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: TEXT, check: { verdict: 'refuse', category: 'nonconsensual_sexual', confidence: 0.97, confidence_band: 'high' } }));
  const shown = JSON.stringify(await authorEntries(env));
  const r = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN }));
  assert.deepEqual([r.ok, r.code, r.status], [false, 'check_removed', 409]);
  // Through the admin tool route too, answered 409.
  const json = (body, status = 200) => new Response(JSON.stringify(body), { status });
  const url = new URL('https://api.eustress.dev/api/admin/moderation/tool');
  const res = await quiet(() => handleModerationRoute(new Request(url, { method: 'POST', body: JSON.stringify({ name: 'moderation_forum_resolve', args: { post_id: POST, decision: 'remove', category: 'csam', rationale: 'Acting on the placeholder as csam.' } }) }), url, env, {}, { json, cors: {}, requireAdmin: async () => ADMIN, auditLog: async () => {} }));
  assert.equal(res.status, 409);
  assert.equal((await res.json()).code, 'check_removed');
  assert.equal(JSON.stringify(await authorEntries(env)), shown, 'the generic entry already shown stands');
  assert.deepEqual((await loadForumCase(env, POST)).decisions, []);
});

// Every case write after the first conflict from here on (another writer, again and again), and legal report writes failing.
function breakAfterDecision(env) {
  env.SCENES.hooks.beforePut = async (key, opts) => {
    if (key === forumCaseR2Key(POST) && opts.onlyIf?.etagMatches) await env.SCENES.rewriteCase(POST, (r) => { r.churn = (r.churn || 0) + 1; });
  };
  const realPut = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (key, value, opts) => { if (key.startsWith('legalreport:')) throw new Error('KV is unavailable'); return realPut(key, value, opts); };
  return () => { env.SCENES.hooks.beforePut = null; env.SOCIAL.put = realPut; };
}

test('R8-2: a failure after the decision answers ok with pending; report_owed is written before the push, and the sweep writes the report', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  let restore = null;
  const pushed = [];
  registerForumDecisionApplier(async (a) => { pushed.push(a); restore = breakAfterDecision(env); });
  let r;
  try {
    r = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  } finally {
    registerForumDecisionApplier(null);
  }
  assert.ok(Date.parse(pushed[0].decidedAt), 'the applier still receives decidedAt');
  assert.deepEqual([r.ok, r.action, r.follow_up], [true, 'remove', 'pending']);
  assert.deepEqual([...r.pending].sort(), ['case_write', 'legal_report']);
  assert.equal(r.case_version.round, 1, 'the decision\'s own version');
  restore();
  let rec = await loadForumCase(env, POST);
  assert.equal(rec.removed, true, 'the decision stands');
  assert.deepEqual([rec.report_owed?.category, rec.report_owed?.reason, rec.report_owed?.by], ['csam', 'person_removal', ADMIN], 'the debt was written with the decision');
  assert.ok(await env.SOCIAL.get(`legalowed:forum:${POST}`), 'and indexed for the sweep');
  assert.equal((await reportsOf(env)).length, 0, 'no report yet');
  // With FORUM_LEGAL_OWED_SWEEP off (the default) the hourly sweep never reaches the forum code: its index is not even listed,
  // and the debt stays for the day the forums are turned on.
  const listed = [];
  const list = env.SOCIAL.list.bind(env.SOCIAL);
  env.SOCIAL.list = (opts) => { listed.push(opts?.prefix); return list(opts); };
  const off = await quiet(() => sweepLegalReports(env));
  env.SOCIAL.list = list;
  assert.equal(off.owed_reports, 'off');
  assert.ok(listed.length > 0 && !listed.some((p) => String(p).startsWith('legalowed:')), 'the forum index is never listed while the setting is off');
  assert.equal((await reportsOf(env)).length, 0, 'and no report is written');
  assert.ok(await env.SOCIAL.get(`legalowed:forum:${POST}`), 'the debt is kept');
  // On, the hourly legal sweep writes it, and the debt goes.
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  const swept = await quiet(() => sweepLegalReports(env));
  assert.deepEqual([swept.owed_reports.written, swept.owed_reports.still_owed], [1, 0]);
  const [rep] = await reportsOf(env);
  assert.deepEqual([rep.kind, rep.when_where.case_id], ['csam', POST]);
  rec = await loadForumCase(env, POST);
  assert.equal(rec.report_owed, null);
  assert.equal(await env.SOCIAL.get(`legalowed:forum:${POST}`), null);
  // A plain success has pending: [] and no follow_up; the admin tool route answers 202 only when something is pending.
  const ok = await environment();
  await report(ok, REPORTERS[0], 'spam');
  const plain = await quiet(() => resolveForumCase({ env: ok, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }));
  assert.deepEqual([plain.ok, plain.pending, 'follow_up' in plain], [true, [], false]);
  assert.equal((await loadForumCase(ok, POST)).report_owed, null, 'written at once, so nothing is owed');
  const viaRoute = await environment();
  await report(viaRoute, REPORTERS[0], 'spam');
  registerForumDecisionApplier(async () => { breakAfterDecision(viaRoute); });
  try {
    const json = (body, status = 200) => new Response(JSON.stringify(body), { status });
    const url = new URL('https://api.eustress.dev/api/admin/moderation/tool');
    const res = await quiet(() => handleModerationRoute(new Request(url, { method: 'POST', body: JSON.stringify({ name: 'moderation_forum_resolve', args: { post_id: POST, decision: 'remove', category: 'csam', rationale: 'Sexual content involving a minor, confirmed.' } }) }), url, viaRoute, {}, { json, cors: {}, requireAdmin: async () => ADMIN, auditLog: async () => {} }));
    assert.equal(res.status, 202);
    const body = await res.json();
    assert.deepEqual([body.ok, body.follow_up, body.pending.includes('legal_report')], [true, 'pending', true]);
  } finally {
    registerForumDecisionApplier(null);
  }
});

test('Required 2: the owed-report sweep lets go of the case lease when its budget runs out mid-case', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'spam');
  let restore = null;
  registerForumDecisionApplier(async () => { restore = breakAfterDecision(env); });
  try { await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT })); }
  finally { registerForumDecisionApplier(null); }
  restore();
  assert.ok((await loadForumCase(env, POST)).report_owed);
  // The clock passes the budget after the lease is taken.
  let t = 0;
  const clock = () => (t++ < 2 ? 0 : 10_000);
  const out = await quiet(() => writeOwedForumReports(env, {}, { budgetMs: 1, now: clock }));
  assert.equal(out.stopped, true, 'stopped on the budget');
  assert.equal(env.SCENES.objects.has(`locks/forum/${POST}.json`), false, 'the lease was released');
  assert.ok((await loadForumCase(env, POST)).report_owed, 'still owed, for the next run');
  // The next run, with time, writes it.
  assert.equal((await quiet(() => writeOwedForumReports(env, {}))).written, 1);
});

test('moderation_forum_case is audited before it answers, and a read whose audit cannot be written does not happen', async () => {
  const env = await environment();
  await report(env, REPORTERS[0], 'threat', { note: 'NOTE-CANARY-AUDIT' });
  const exec = (e) => makeToolExecutor(e, { auditLog: async () => {} }, { actor: 'admin', actorId: ADMIN });
  const read = await exec(env)('moderation_forum_case', { post_id: POST });
  assert.equal(read.ok, true);
  const audited = [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v)).filter((a) => a.action === 'FORUM_CASE_READ');
  assert.deepEqual(audited.map((a) => [a.admin_id, a.details.post_id]), [[ADMIN, POST]]);
  // The audit log throws: refused, and nothing of the case is answered.
  const broken = { ...env, AUDIT_LOG: { put: async () => { throw new Error('down'); } } };
  const refused = await quiet(() => exec(broken)('moderation_forum_case', { post_id: POST }));
  assert.deepEqual([refused.ok, refused.code, refused.status], [false, 'audit_unavailable', 503]);
  assert.equal(refused.case, undefined);
  assert.ok(!JSON.stringify(refused).includes('NOTE-CANARY-AUDIT'));
  assert.ok(!JSON.stringify(refused).includes(REPORTERS[0]));
});

test("resolveForumCase reads the forum's pairs against the clock it is given, not the machine's", async () => {
  // A fixed moment long past: a pair 29.5 days before it is inside the 30-day window at that moment, and far outside it by the
  // real clock. Passing `now` must keep the pair; reading the real clock would drop it.
  const PAST = Date.parse('2026-01-01T12:00:00.000Z');
  const pair = { ip: '192.0.2.91', at: new Date(PAST - 29.5 * 86400 * 1000).toISOString(), item_id: keyedItem(TEXT) };
  const env = await environment();
  await report(env, REPORTERS[0], 'spam', { now: PAST });
  await env.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify(pair));
  assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, now: PAST }))).ok, true);
  assert.deepEqual((await reportsOf(env))[0].who.upload_ips, [pair], 'kept: 29.5 days old at the given clock');
  const rec = await loadForumCase(env, POST);
  assert.deepEqual((await readEvidence(env, { caseRef: rec.reference, adminId: ADMIN, legalProcess: LEGAL })).items[0].facts.uploader.upload_ips, [pair], 'and sealed');
});
