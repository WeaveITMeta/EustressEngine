// Security's review of the report debts (95e1b1c7, d24b7829): D-1 to D-12, each adapted from its probe (zz_debt_1 to 10) and
// each with a mutation that fails it. A forum or listing case that owes a legal report keeps it until it is paid, and the
// strongest finding on the case stands; a failed account read is never reported as a fact; a person's clear stands and never
// takes a payable debt with it; a debt that fails the same way every hour is stuck for a person; a case owes one debt per
// kind; retries do not rotate the case's own history; each kind's stuck re-alert is under its own setting; staff hear of a
// listing debt when it is created; a slow failing debt never starves the rest.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { fakeKv } from './helpers/load_worker.mjs';
import {
  recordForumRemoval, loadForumCase, forumCaseR2Key, forumReportOwedKey, fileForumReport, resolveForumCase, writeOwedForumReports,
  makeToolExecutor, loadCase, listingReportOwedKey, POLICY_HASH_ANCHORED, writeOwedListingReports, LISTING_OWED_RETRIES_PER_RUN, forumDebts,
} from '../src/moderation.mjs';
import {
  loadLegalReport, sweepLegalReports, writeLegalReport, loadStuckReport, legalStuckKey, alertStuckReports, handleLegalReportRoute,
  REPORT_DEBT_MAX_TRIES, RETRIES_EXHAUSTED, confirmUnconfirmedAccounts,
} from '../src/legal_reports.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

const enc = new TextEncoder();
const dec = new TextDecoder();
const THREAD = 'c'.repeat(24);
const POST = `${THREAD}-1`;
const AUTHOR = 'author-rv01';
const AUTHOR2 = 'author-rv02';
const ADMIN = 'admin-rv01';
const REPORTER = 'reporter-rv01';
const SECRET = 'test-secret-report-debts-review';
const CAT = 'csam_or_minor_sexualization';
const simId = (n) => `a1b2c3d4-0000-4000-8000-${String(n).padStart(12, '0')}`;
const SIM = simId(901);

// R2 as it behaves: conditional puts, an etag that is the MD5 of the content, a paged list, a hook inside a put.
function bucket() {
  const objects = new Map();
  const hooks = { beforePut: null };
  const view = (key, o) => ({ key, size: o.bytes.byteLength, etag: o.etag, customMetadata: o.customMetadata || {}, uploaded: new Date(), checksums: {}, text: async () => dec.decode(o.bytes), arrayBuffer: async () => o.bytes.slice().buffer });
  return {
    objects, hooks,
    async put(key, value, opts = {}) {
      const bytes = typeof value === 'string' ? enc.encode(value) : value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength));
      if (hooks.beforePut) await hooks.beforePut(key, value, opts);
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
      const keys = [...objects.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
      const page = keys.slice(0, limit);
      const truncated = keys.length > limit;
      return { objects: page.map((key) => ({ key, size: objects.get(key).bytes.byteLength, ...(include.includes('customMetadata') ? { customMetadata: objects.get(key).customMetadata || {} } : {}) })), truncated, ...(truncated ? { cursor: page.at(-1) } : {}) };
    },
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

// KV whose list pages with a cursor (the name of the last key of the page).
function pagedKv() {
  const kv = fakeKv();
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    const names = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
    const page = names.slice(0, limit);
    const more = names.length > limit;
    return { keys: page.map((name) => ({ name })), list_complete: !more, ...(more ? { cursor: page.at(-1) } : {}) };
  };
  return kv;
}

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
async function environment() {
  const env = { USERS: fakeKv(), SOCIAL: pagedKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), SCENES: bucket(), JWT_SECRET: SECRET, JEV_API_KEY: 'jev-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', __EmailMessage: StubMessage, mail: [] };
  env.EMAIL = { send: async (m) => { if (env.onMail) await env.onMail(m); env.mail.push(m); } };
  for (const id of [AUTHOR, AUTHOR2, ADMIN, REPORTER]) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return env;
}
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };
const reportsOf = async (env) => (await Promise.all([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalreport:')).map((k) => loadLegalReport(env, k.slice('legalreport:'.length))))).filter(Boolean);
const auditsOf = (env, action) => [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v)).filter((a) => a.action === action);
function failPuts(env, pattern) {
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (key, ...rest) => { if (pattern.test(String(key))) throw new Error('KV is unavailable'); return real(key, ...rest); };
  return () => { env.SOCIAL.put = real; };
}
// The subjects of the mail to the security mailbox (each staff alert also goes to every administrator's address).
function subjects(env) {
  return env.mail.filter((m) => m.to === 'security@eustress.dev').map((m) => {
    const line = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
    const b = /^=\?UTF-8\?B\?(.*)\?=$/.exec(line);
    return b ? Buffer.from(b[1], 'base64').toString('utf8') : line;
  });
}
const metaOf = (env, postId = POST) => env.SCENES.objects.get(forumCaseR2Key(postId)).customMetadata;
const csamCheck = { verdict: 'refuse', category: 'csam', confidence: 0.97, confidence_band: 'high' };
const checkRemoval = (env, extra = {}) => quiet(() => recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: 'NUMBAT csam text', check: csamCheck, ...extra }));
const THREAT = { religious_speech: false, political_speech: false, criticism: false, hyperbole: false, joke: false, place: true, time: true };
const threatRemoval = (env) => quiet(() => recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: 'a threat', check: { verdict: 'refuse', category: 'threat', confidence: 0.95, confidence_band: 'high' }, secondCheck: { source: 'claude:forum', category: 'threat', confidence: 0.92 }, threatContent: THREAT }));

// The listing harness (moderation_quarantine through the tool executor).
const json = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const toolDeps = () => ({ auditLog: async () => {}, json, cors: {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED });
async function listing(env, { id = SIM, p = 0.95, author = AUTHOR } = {}) {
  const sim = { id, name: 'Harbor Town', author_id: author, is_public: true, r2_key: `universes/${id}/universe.pak`, published_at: '2026-10-14T10:00:00.000Z', updated_at: '2026-10-14T10:00:00.000Z', moderation: { status: 'held' } };
  await env.SCENES.put(sim.r2_key, 'PAK-BYTES');
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, author_id: author, status: 'held', created_at: '2026-10-14T10:00:00.000Z', history: [], jev: { model: 'jev-test', answers: { [CAT]: { p } } }, decision: { reasons: [] } }));
}
const quarantineAs = (env, actor, actorId, id = SIM) => quiet(() => makeToolExecutor(env, toolDeps(), { actor, actorId })('moderation_quarantine', { sim_id: id, category: CAT, rationale: 'A rationale that is at least twenty characters long.' }, { actor, actorId }));
const routeDeps = (extra = {}) => ({ json, requireAdmin: async () => ADMIN, auditLog: async (env, action, adminId, target, details) => { await env.AUDIT_LOG.put(`audit:${Date.now()}-${crypto.randomUUID()}`, JSON.stringify({ action, admin_id: adminId, target, details })); }, ...extra });
const route = async (env, method, path, body, deps = routeDeps()) => {
  const url = new URL(`https://api.eustress.dev${path}`);
  const res = await quiet(() => handleLegalReportRoute(new Request(url, { method, headers: { 'Content-Type': 'application/json' }, ...(body ? { body: JSON.stringify(body) } : {}) }), url, env, {}, deps));
  return { status: res.status, body: await res.json() };
};

// The row GET /stuck shows for a case's stuck record of `kind`: what a clear or a retry must name (its report kind and since).
const rowOf = async (env, caseKind, caseId, kind = null) => { const r = await loadStuckReport(env, caseKind, caseId, kind); return r ? { report_kind: r.report_kind || null, since: r.since ?? null } : { since: null }; };

// -- D-1 -----------------------------------------------------------------------------------------------------------------

test('D-1 (zz_debt_1): a csam check removal owes its report from the removal\'s own write; no failure after it loses the debt', async () => {
  const env = await environment();
  // Every report write fails, every owed index write fails, and every case write after the removal's own one fails.
  const restore = failPuts(env, /^(legalreport:|legalowed:)/);
  let writes = 0;
  env.SCENES.hooks.beforePut = async (key) => { if (key === forumCaseR2Key(POST) && ++writes > 1) throw new Error('R2 is unavailable'); };
  const r = await checkRemoval(env);
  env.SCENES.hooks.beforePut = null;
  restore();
  assert.equal(r.ok, true, JSON.stringify(r));
  assert.equal(r.report, 'unavailable');
  const rec = await loadForumCase(env, POST);
  assert.deepEqual([rec.removed, rec.report_owed?.category, rec.report_owed?.reason], [true, 'csam', 'check_removal'], 'the debt is in the removal\'s own write');
  assert.equal(metaOf(env).owed, '1', 'and in its R2 metadata, written in the same put');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null, 'no index was written');
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  await quiet(() => sweepLegalReports(env));
  assert.equal((await reportsOf(env)).filter((x) => x.kind === 'csam').length, 1, 'the heal found it and the sweep wrote it');
  assert.equal((await loadForumCase(env, POST)).report_owed, null);
  // A removal whose single write fails is refused whole: nothing is removed without its debt.
  const whole = await environment();
  whole.SCENES.hooks.beforePut = async (key) => { if (key === forumCaseR2Key(POST)) throw new Error('R2 is unavailable'); };
  const refused = await checkRemoval(whole);
  assert.deepEqual([refused.ok, refused.code], [false, 'unavailable']);
  assert.equal(whole.SCENES.objects.has(forumCaseR2Key(POST)), false);
});

test('D-1: a check removal whose report is written owes nothing afterwards, and one whose report fails is owed and indexed', async () => {
  const env = await environment();
  const r = await checkRemoval(env);
  assert.equal(r.report, 'ready_to_send');
  assert.equal((await loadForumCase(env, POST)).report_owed, null, 'the debt the removal owed was closed by the report');
  assert.equal(metaOf(env).owed, '0');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null);
  const owed = await environment();
  const restore = failPuts(owed, /^legalreport:/);
  await checkRemoval(owed);
  restore();
  assert.equal((await loadForumCase(owed, POST)).report_owed?.category, 'csam');
  assert.ok(await owed.SOCIAL.get(forumReportOwedKey(POST)), 'indexed before the try');
});

// -- D-2 -----------------------------------------------------------------------------------------------------------------

test('D-2 (zz_debt_2): a person\'s owed csam debt survives a later automated quarantine, and is reported', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  const restore = failPuts(env, /^legalreport:/);
  assert.equal((await quarantineAs(env, 'admin', ADMIN)).ok, true);
  restore();
  assert.deepEqual([(await loadCase(env, SIM)).report_owed?.actor, (await loadCase(env, SIM)).report_owed?.reason], ['admin', 'unavailable']);
  const again = await quarantineAs(env, 'agent', 'agent');
  assert.equal(again.ok, true);
  const after = await loadCase(env, SIM);
  assert.equal(after.report_settled, undefined, 'the weaker finding did not settle the debt');
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1, 'the person\'s finding was reported');
  assert.equal((await reportsOf(env))[0].what.detection.source, 'person');
});

test('D-2: a debt is tried again under the thresholds it was recorded with, not the ones in force at the retry', async () => {
  const env = await environment();
  await listing(env, { p: 0.9 });
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  restore();
  assert.equal((await loadCase(env, SIM)).report_owed?.detection?.band, 'high', 'a confident finding at hard_quarantine 0.85');
  env.MODERATION_THRESHOLDS = '{"audit_rate":0,"hard_quarantine":0.95}';
  const out = await quiet(() => writeOwedListingReports(env));
  assert.deepEqual([out.written, out.settled], [1, 0], 'the threshold raised later does not settle it as borderline');
});

// -- D-3 -----------------------------------------------------------------------------------------------------------------

test('D-3 (zz_debt_5): a failed account read is never reported as a deleted account; the report waits, owed', async () => {
  const env = await environment();
  const finding = (caseId) => ({ kind: 'csam', caseKind: 'forum', caseId, category: 'csam', article: 'A1', subject: { kind: 'forum_post', post_id: caseId }, detection: { source: 'person', checks: [], band: 'high' }, uploader: { account_id: AUTHOR } });
  const real = env.USERS.get;
  env.USERS.get = async (k) => { if (String(k) === `user:${AUTHOR}`) throw new Error('KV read timed out'); return real(k); };
  const r = await quiet(() => writeLegalReport(env, finding(`${'b'.repeat(24)}-5`)));
  env.USERS.get = real;
  assert.deepEqual([r.written, r.reason], [false, 'unavailable'], 'owed, not written on a guess');
  const kyc = env.KYC_STATUS.get;
  env.KYC_STATUS.get = async () => { throw new Error('KV read timed out'); };
  const k = await quiet(() => writeLegalReport(env, finding(`${'b'.repeat(24)}-6`)));
  env.KYC_STATUS.get = kyc;
  assert.deepEqual([k.written, k.reason], [false, 'unavailable'], 'an identity record read that fails is owed too');
  const ok = await quiet(() => writeLegalReport(env, finding(`${'b'.repeat(24)}-7`)));
  assert.equal((await loadLegalReport(env, ok.id)).who.account_status, 'known');
  const absent = { ...finding(`${'b'.repeat(24)}-8`), uploader: { account_id: 'user-never-was' } };
  // R-3: a csam report is never held on the account's status: written at once, 'unconfirmed', and confirmed by an amendment.
  const gone = await quiet(() => writeLegalReport(env, absent));
  assert.equal(gone.written, true, 'written at once');
  assert.equal((await loadLegalReport(env, gone.id)).who.account_status, 'unconfirmed', 'one read that finds no record is not a fact');
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 30_000 }));
  assert.equal((await loadLegalReport(env, gone.id)).who.account_status, 'unconfirmed', 'nor are two reads within the minute');
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  assert.equal((await loadLegalReport(env, gone.id)).who.account_status, 'deleted', 'deleted only on a confirmed absent record');
  // The account found on the second read: amended to known, its fields filled, and the absence forgotten.
  const back = { ...finding(`${'b'.repeat(24)}-10`), uploader: { account_id: 'user-came-back' } };
  const w = await quiet(() => writeLegalReport(env, back));
  await env.USERS.put('user:user-came-back', JSON.stringify({ id: 'user-came-back', username: 'back', email: 'back@example.com', created_at: '2026-10-01T00:00:00.000Z' }));
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  const found = await loadLegalReport(env, w.id);
  assert.deepEqual([found.who.account_status, found.who.email], ['known', 'back@example.com']);
  assert.equal(await env.SOCIAL.get('legalabsent:user-came-back'), null);
  const noStore = { ...env, USERS: undefined };
  const u = await quiet(() => writeLegalReport(noStore, finding(`${'b'.repeat(24)}-9`)));
  assert.equal((await loadLegalReport(env, u.id)).who.account_status, 'unknown', 'unknown when it cannot be told');
});

// -- D-4 -----------------------------------------------------------------------------------------------------------------

async function staleStuckListing(env) {
  await listing(env, { p: 0.95 });
  const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  Object.assign(rec, { status: 'quarantined', legal_hold: { category: CAT, rationale: 'old quarantine', at: '2026-09-01T00:00:00.000Z', by: 'pipeline' }, decision: { reasons: [{ code: CAT, action: 'quarantine', lane: 'legal' }] } });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  await quiet(() => sweepLegalReports(env));
  assert.equal((await loadStuckReport(env, 'moderation', SIM))?.reason, 'no_detection_recorded', 'the heal made it stuck');
}

test('D-4 (zz_debt_3): clearing a stale stuck alert never clears a newer, payable csam debt', async () => {
  const env = await environment();
  await staleStuckListing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'admin', ADMIN);
  restore();
  assert.equal((await loadCase(env, SIM)).report_owed?.stuck, undefined, 'the new debt is payable');
  // The stale record was closed when the person's finding replaced its debt (F1): there is nothing stale to clear.
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null);
  assert.ok(auditsOf(env, 'LEGAL_REPORT_STUCK_CLOSED').some((a) => a.details.why === 'replaced'));
  const done = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Old quarantine with no detection on record; reviewed.', report_kind: 'csam', since: '2026-09-01T00:00:00.000Z' });
  assert.equal(done.status, 404, JSON.stringify(done));
  const rec = await loadCase(env, SIM);
  assert.deepEqual([!!rec.report_owed, rec.report_owed?.actor], [true, 'admin'], 'the payable debt is still owed after the clear');
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'and still indexed');
  await quiet(() => sweepLegalReports(env));
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
});

test('D-4 (zz_debt_3b): a person\'s audited clear of a stuck listing debt stands; the heal does not re-open it', async () => {
  const env = await environment();
  await staleStuckListing(env);
  assert.equal((await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Reviewed with counsel: filed by hand under reference X-9.', ...(await rowOf(env, 'moderation', SIM)) })).status, 200);
  const mails = env.mail.length;
  for (let i = 0; i < 2; i++) await quiet(() => sweepLegalReports(env));
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null, 'not re-opened');
  assert.equal(env.mail.length, mails, 'and not re-alerted');
  assert.equal((await loadCase(env, SIM)).report_owed, null);
});

test('D-4: a forum clear removes only the stuck debt its record names; a payable debt of another kind stays owed', async () => {
  const env = await environment();
  await threatRemoval(env);
  // The threat debt is stuck for a person (a defect), and a csam debt is owed beside it.
  const since = new Date().toISOString();
  await env.SCENES.rewriteCase(POST, (r) => {
    r.report_owed = { since, category: 'threat', reason: 'check_removal', by: null, stuck: { reason: 'no_account', at: new Date().toISOString() } };
    r.report_owed_more = { csam: { since: new Date().toISOString(), category: 'csam', reason: 'check_removal', by: null, detection: { source: 'forum_check', checks: [{ kind: 'classifier', source: 'jev:forum', category: 'csam', confidence: 0.97, band: 'high' }], band: 'high', viewed_by_staff: false } } };
  });
  await env.SOCIAL.put(legalStuckKey('forum', POST), JSON.stringify({ case_kind: 'forum', case_id: POST, report_kind: 'threat', reason: 'no_account', since, stuck_at: new Date().toISOString(), alerted_at: null, alerts: 0 }));
  await env.SOCIAL.put(forumReportOwedKey(POST), new Date().toISOString());
  assert.equal((await route(env, 'POST', `/api/admin/legal/stuck/forum/${POST}/clear`, { note: 'Reviewed: the threat was handled by hand.', report_kind: 'threat', since })).status, 200);
  const rec = await loadForumCase(env, POST);
  assert.deepEqual(forumDebts(rec).map((d) => [d.category, !!d.stuck]), [['csam', false]], 'only the stuck threat debt went');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)), 'the payable csam debt stays indexed');
});

// -- D-6 -----------------------------------------------------------------------------------------------------------------

test('D-6 (zz_debt_9): a forum debt that fails the same way every hour is stuck for a person after REPORT_DEBT_MAX_TRIES tries', async () => {
  const env = await environment();
  await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTER, category: 'spam', postBody: 'text', now: Date.now() }));
  const restore = failPuts(env, /^legalreport:/);
  await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: 'text' }));
  restore();
  await env.SCENES.rewriteCase(POST, (r) => { r.report_owed.detection = { source: 'forum_check', band: 'high', checks: { not: 'an array' }, viewed_by_staff: false }; });
  assert.equal(REPORT_DEBT_MAX_TRIES, 24);
  let last;
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES - 1; hour++) last = await quiet(() => writeOwedForumReports(env));
  assert.equal(last.still_owed, 1, 'still owed one try before the limit');
  assert.equal((await loadForumCase(env, POST)).report_owed.tries, REPORT_DEBT_MAX_TRIES - 1, 'each try is counted');
  last = await quiet(() => writeOwedForumReports(env));
  assert.equal(last.stuck, 1);
  const stuck = await loadStuckReport(env, 'forum', POST);
  assert.equal(stuck?.reason, RETRIES_EXHAUSTED);
  assert.ok(subjects(env).some((s) => s.startsWith('A legal report is stuck')), 'staff are told');
  const debt = (await loadForumCase(env, POST)).report_owed;
  assert.equal(debt.stuck.last_reason, 'unavailable', 'with the reason the tries kept failing');
  assert.ok(Date.parse(debt.stuck.next_try_at) > Date.now(), 'and is still tried, once a day (F4)');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)), 'so it keeps its index');
});

test('D-6: a listing debt whose try throws before it starts (a corrupt listing record) is counted and stuck in the end', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  restore();
  await env.SOCIAL.put(`sim:${SIM}`, '{not json');
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedListingReports(env));
  assert.equal((await loadStuckReport(env, 'moderation', SIM))?.reason, RETRIES_EXHAUSTED);
  assert.equal((await loadCase(env, SIM)).report_owed.stuck.reason, RETRIES_EXHAUSTED);
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'still tried once a day, so still indexed');
});

// -- D-7 -----------------------------------------------------------------------------------------------------------------

test('D-7 (zz_debt_10): a csam debt is not lost because the case already owes a threat report', async () => {
  const env = await environment();
  const restore = failPuts(env, /^legalreport:/);
  assert.equal((await threatRemoval(env)).report, 'unavailable');
  assert.equal((await loadForumCase(env, POST)).report_owed?.category, 'threat');
  assert.equal((await checkRemoval(env)).report, 'unavailable');
  restore();
  assert.deepEqual(forumDebts(await loadForumCase(env, POST)).map((d) => d.category).sort(), ['csam', 'threat'], 'one debt per kind');
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  env.LEGAL_REPORT_THREATS = 'on';
  for (let i = 0; i < 2; i++) await quiet(() => sweepLegalReports(env));
  assert.deepEqual((await reportsOf(env)).map((r) => r.kind).sort(), ['csam', 'threat'], 'both reports are written');
  assert.deepEqual(forumDebts(await loadForumCase(env, POST)), []);
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null);
});

// -- D-8 -----------------------------------------------------------------------------------------------------------------

test('D-8 (zz_debt_7): hourly retries add a history entry once per state change, and the quarantine stays in the history', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  for (let hour = 0; hour < 65; hour++) await quiet(() => writeOwedListingReports(env));
  restore();
  const events = (await loadCase(env, SIM)).history.map((h) => h.event);
  assert.ok(events.includes('quarantined'), 'the quarantine is still in the history');
  assert.ok(events.filter((e) => e === 'legal_lane').length <= 2, `one legal_lane entry per state, not per try (${events.filter((e) => e === 'legal_lane').length})`);
});

// -- D-9 -----------------------------------------------------------------------------------------------------------------

test('D-9 (zz_debt_6b): a forum stuck debt is re-alerted daily under FORUM_LEGAL_OWED_SWEEP, the listing setting off', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * 86400000).toISOString();
  await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-7`), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-7`, report_kind: 'threat', reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  await env.SOCIAL.put(legalStuckKey('moderation', SIM), JSON.stringify({ case_kind: 'moderation', case_id: SIM, report_kind: 'csam', reason: 'no_detection_recorded', stuck_at: old, alerted_at: old, alerts: 1 }));
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  const out = await quiet(() => sweepLegalReports(env));
  assert.deepEqual([out.stuck.stuck, out.stuck.alerted], [1, 1], 'the forum debt only');
  assert.equal(subjects(env).filter((s) => s.startsWith('A legal report is stuck')).length, 1);
  assert.equal((await loadStuckReport(env, 'moderation', SIM)).alerts, 1, 'the listing debt waits for its own setting');
});

// -- D-10 ----------------------------------------------------------------------------------------------------------------

test('D-10: a listing csam debt is told to staff once when it is created, with the listing setting off', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  restore();
  const owed = () => subjects(env).filter((s) => s.startsWith('A legal report is owed'));
  assert.equal(owed().length, 1, 'staff were told at once');
  await quiet(() => sweepLegalReports(env));
  const again = failPuts(env, /^legalreport:/);
  await quiet(() => writeOwedListingReports(env));
  again();
  assert.equal(owed().length, 1, 'once per case and debt');
});

// -- D-11 ----------------------------------------------------------------------------------------------------------------

test('D-11 (zz_debt_6): a slow failing debt at the head of a page does not starve the forum debts behind it', async () => {
  const env = await environment();
  const posts = [[`${THREAD}-1`, AUTHOR], [`${THREAD}-2`, AUTHOR2], [`${THREAD}-3`, AUTHOR2]];
  for (const [p, author] of posts) {
    await quiet(() => fileForumReport({ env, postId: p, threadId: THREAD, postAuthorId: author, reporterId: REPORTER, category: 'spam', postBody: 'text', now: Date.now() }));
    const restore = failPuts(env, /^legalreport:/);
    await quiet(() => resolveForumCase({ env, postId: p, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: 'text' }));
    restore();
  }
  let clock = 0;
  let failNext = false;
  const deps = { accountState: async (_env, id) => { if (id === AUTHOR) { clock += 21_000; failNext = true; } return null; } };
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (k, ...r) => { if (failNext && String(k).startsWith('legalreport:')) { failNext = false; throw new Error('KV down'); } return real(k, ...r); };
  const base = Date.now();
  for (let i = 0; i < 3; i++) await quiet(() => writeOwedForumReports(env, deps, { now: () => base + clock }));
  env.SOCIAL.put = real;
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(`${THREAD}-2`)), null, 'post 2 is reported');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(`${THREAD}-3`)), null, 'post 3 is reported');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(`${THREAD}-1`)), 'the slow one is still owed');
});

test('D-11: a slow failing listing debt at the head of a page does not starve the listing debts behind it', async () => {
  const env = await environment();
  const ids = [simId(1), simId(2), simId(3)];
  for (const [i, id] of ids.entries()) {
    await listing(env, { id, author: i === 0 ? AUTHOR : AUTHOR2 });
    const restore = failPuts(env, /^legalreport:/);
    await quarantineAs(env, 'system', 'pipeline', id);
    restore();
  }
  let clock = 0;
  let failNext = false;
  const deps = { accountState: async (_env, id) => { if (id === AUTHOR) { clock += 21_000; failNext = true; } return null; } };
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (k, ...r) => { if (failNext && String(k).startsWith('legalreport:')) { failNext = false; throw new Error('KV down'); } return real(k, ...r); };
  const base = Date.now();
  for (let i = 0; i < 3; i++) await quiet(() => writeOwedListingReports(env, deps, { now: () => base + clock }));
  env.SOCIAL.put = real;
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(ids[1])), null);
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(ids[2])), null);
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(ids[0])));
});

test('D-11 (zz_debt_6c): every stuck debt is re-alerted however many there are, and GET /stuck pages with a cursor', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * 86400000).toISOString();
  for (const n of [1, 2]) await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-${n}`), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-${n}`, reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  for (let day = 0; day < 3; day++) await quiet(() => alertStuckReports(env, { now: Date.now() + day * 86400000 + 60_000, limit: 1, maxPages: 1 }));
  assert.ok((await loadStuckReport(env, 'forum', `${THREAD}-2`)).alerts > 1, 'the second is reached');
  // The admin list: 450 stuck debts are read over pages, each with the cursor of the one before.
  for (let n = 3; n < 450; n++) await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-${n}`), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-${n}`, reason: 'no_account', stuck_at: old, alerts: 0 }));
  const seen = [];
  let cursor = null;
  for (let page = 0; page < 5; page++) {
    const res = await route(env, 'GET', `/api/admin/legal/stuck${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`);
    assert.equal(res.status, 200);
    seen.push(...res.body.items.map((i) => i.case_id));
    cursor = res.body.next_cursor;
    if (!cursor) break;
  }
  assert.equal(new Set(seen).size, 449, 'every stuck debt is listed once over the pages');
});

test('notes: GET /stuck is not served when its audit cannot be written', async () => {
  const env = await environment();
  await env.SOCIAL.put(legalStuckKey('forum', POST), JSON.stringify({ case_kind: 'forum', case_id: POST, reason: 'no_account', stuck_at: new Date().toISOString(), alerts: 0 }));
  const res = await route(env, 'GET', '/api/admin/legal/stuck', null, routeDeps({ auditLog: async () => { throw new Error('down'); } }));
  assert.deepEqual([res.status, res.body.code], [503, 'audit_unavailable']);
  assert.equal(JSON.stringify(res.body).includes(POST), false);
});

test('notes: a clear that fails is audited as attempted and failed, never as cleared', async () => {
  const env = await environment();
  await env.SOCIAL.put(legalStuckKey('forum', POST), JSON.stringify({ case_kind: 'forum', case_id: POST, report_kind: 'threat', reason: 'no_account', stuck_at: new Date().toISOString(), alerts: 0 }));
  // The case's lease is held by another writer: the clear answers busy (503).
  await env.SCENES.put(`locks/forum/${POST}.json`, JSON.stringify({ holder: 'someone-else', expires_at: Date.now() + 3600_000 }));
  const res = await route(env, 'POST', `/api/admin/legal/stuck/forum/${POST}/clear`, { note: 'Reviewed: this one was filed by hand.', report_kind: 'threat', since: null });
  assert.equal(res.status, 503);
  assert.equal(auditsOf(env, 'LEGAL_STUCK_CLEARED').length, 0, 'no audit says it was cleared');
  assert.equal(auditsOf(env, 'LEGAL_STUCK_CLEAR_REQUESTED').length, 1);
  assert.deepEqual(auditsOf(env, 'LEGAL_STUCK_CLEAR_FAILED').map((a) => a.details.code), ['busy']);
});

// -- D-12 ----------------------------------------------------------------------------------------------------------------

test('D-12 (M9): the stuck alert\'s write-back never restores a record a person cleared while it was being sent', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * 86400000).toISOString();
  const key = legalStuckKey('forum', POST);
  await env.SOCIAL.put(key, JSON.stringify({ case_kind: 'forum', case_id: POST, reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  env.onMail = async () => { await env.SOCIAL.delete(key); };
  assert.equal((await quiet(() => alertStuckReports(env))).alerted, 1);
  assert.equal(await env.SOCIAL.get(key), null, 'the cleared record stays cleared');
  // Recorded again during the send (a new stuck_at): the new record is not overwritten with the old one.
  await env.SOCIAL.put(key, JSON.stringify({ case_kind: 'forum', case_id: POST, reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  const fresh = new Date().toISOString();
  env.onMail = async () => { await env.SOCIAL.put(key, JSON.stringify({ case_kind: 'forum', case_id: POST, reason: 'retries_exhausted', stuck_at: fresh, alerted_at: null, alerts: 0 })); };
  await quiet(() => alertStuckReports(env));
  env.onMail = null;
  assert.deepEqual([(await loadStuckReport(env, 'forum', POST)).reason, (await loadStuckReport(env, 'forum', POST)).stuck_at], ['retries_exhausted', fresh]);
});

// A forum case whose threat debt no retry can write (no account): stuck on its first try.
async function forumStuckThreat(env) {
  await threatRemoval(env);
  await env.SCENES.rewriteCase(POST, (r) => { r.author_id = null; });
}

test('D-12 (M10, M11): a stuck debt whose stuck record cannot be written stays owed, marked as it was, and stays indexed', async () => {
  const env = await environment();
  const restore = failPuts(env, /^legalreport:/);
  await forumStuckThreat(env);
  restore();
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)));
  // M11: the try comes to stuck, and the stuck record cannot be written.
  const back = failPuts(env, /^legalstuck:/);
  await quiet(() => writeOwedForumReports(env));
  back();
  let rec = await loadForumCase(env, POST);
  assert.equal(rec.report_owed.stuck, undefined, 'not marked stuck without its record');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)), 'and still on the owed list');
  // M10: a debt already marked stuck, whose record was lost, keeps its index while the record cannot be written.
  await env.SCENES.rewriteCase(POST, (r) => { r.report_owed.stuck = { reason: 'no_account', at: new Date().toISOString() }; });
  const again = failPuts(env, /^legalstuck:/);
  await quiet(() => writeOwedForumReports(env));
  again();
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)), 'the index stays until the stuck record is written');
  await quiet(() => writeOwedForumReports(env));
  assert.equal((await loadStuckReport(env, 'forum', POST))?.reason, 'no_account');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null);
  rec = await loadForumCase(env, POST);
  assert.ok(rec.report_owed.stuck);
});

test('D-12 (M20): the listing sweep records a stuck debt\'s stuck index and does not try it again', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  const rec = await loadCase(env, SIM);
  rec.report_owed.stuck = { reason: 'no_detection_recorded', at: new Date().toISOString() };
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  const out = await quiet(() => writeOwedListingReports(env));
  restore();
  assert.equal(out.tried, 0, 'a stuck debt is not tried');
  assert.equal((await loadStuckReport(env, 'moderation', SIM))?.reason, 'no_detection_recorded');
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(SIM)), null);
});

// -- the per-run cap -----------------------------------------------------------------------------------------------------

test('the listing owed sweep tries at most LISTING_OWED_RETRIES_PER_RUN debts a run, and the next run goes on after them', async () => {
  const env = await environment();
  const ids = Array.from({ length: 12 }, (_, i) => simId(100 + i));
  const restore = failPuts(env, /^legalreport:/);
  for (const id of ids) { await listing(env, { id }); await quarantineAs(env, 'system', 'pipeline', id); }
  restore();
  assert.equal(LISTING_OWED_RETRIES_PER_RUN, 10);
  const first = await quiet(() => writeOwedListingReports(env));
  assert.deepEqual([first.tried, first.written, first.stopped], [10, 10, true]);
  const second = await quiet(() => writeOwedListingReports(env));
  assert.deepEqual([second.tried, second.written], [2, 2]);
  assert.equal((await reportsOf(env)).length, 12);
});
