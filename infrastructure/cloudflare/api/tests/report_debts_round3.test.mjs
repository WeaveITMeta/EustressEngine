// Security's third review of the report debts (1142651b): B-1, R-1 to R-5 and the notes, each adapted from its probe (zz_d3_1
// to 6) with a mutation that fails it. A report a try finds already written is repaired (its case index, its queue key, its
// dispatch) before it is counted paid; a clear or a retry names its row in full; a CSAM report is never held on the account's
// status; a debt with no recorded detection is never acted on by an automated try; a settlement stands only for its own
// quarantine; the claim on a report fences its save; a debt stuck before next_try_at existed is due; the stuck re-alert is
// capped; a new failure reason reaches the stuck record; busy reaches the admin as 409; a case write after a lost lease refuses.
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
import { healListingReportDebts, handleModerationRoute } from '../src/moderation.mjs';
import { REPORT_CLAIM_MS, STUCK_ALERTS_PER_RUN } from '../src/legal_reports.mjs';

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


// -- helpers of this round -------------------------------------------------------------------------------------------------

const DAY = 86400000;
const failOnce = (env, pattern) => {
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.put = async (key, ...rest) => { if (armed && pattern.test(String(key))) { armed = false; throw new Error('KV is unavailable'); } return real(key, ...rest); };
  return () => { env.SOCIAL.put = real; };
};
const rowOf = async (env, caseKind, caseId, kind = null) => { const r = await loadStuckReport(env, caseKind, caseId, kind); return r ? { report_kind: r.report_kind || null, since: r.since ?? null } : { since: null }; };
async function staleStuckListing(env, { p = 0.95, by = 'pipeline' } = {}) {
  await listing(env, { p });
  const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  Object.assign(rec, { status: 'quarantined', legal_hold: { category: CAT, rationale: 'old quarantine', at: '2026-09-01T00:00:00.000Z', by }, decision: { reasons: [{ code: CAT, action: 'quarantine', lane: 'legal' }] } });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  await quiet(() => sweepLegalReports(env));
  assert.equal((await loadCase(env, SIM)).report_owed?.stuck?.reason, 'no_detection_recorded');
}

// -- B-1, R-1 ------------------------------------------------------------------------------------------------------------

test('B-1 (zz_d3_4a): a saved report whose queue key failed to write is queued, listed and alerted by the next try, then paid', async () => {
  const env = await environment();
  await listing(env);
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  const restore = failOnce(env, /^legalq:ready_to_send:/);
  await quarantineAs(env, 'admin', ADMIN);
  restore();
  assert.ok((await loadCase(env, SIM)).report_owed, 'the try that lost the queue key left the debt owed');
  // One hourly sweep: the listing debt's try finds the report, queues it and dispatches it (its staff alert) at once.
  await quiet(() => sweepLegalReports(env));
  const csam = (await reportsOf(env)).filter((r) => r.kind === 'csam');
  assert.equal(csam.length, 1, 'one report');
  assert.ok(env.SOCIAL.store.has(`legalq:${csam[0].status}:${csam[0].id}`), 'in the send queue');
  assert.equal((await route(env, 'GET', '/api/admin/legal/reports')).body.items.length, 1, 'listed for staff');
  assert.ok(subjects(env).some((s) => /^A legal report is (waiting|held)/.test(s)), 'and staff were told');
  assert.equal((await loadCase(env, SIM)).report_owed, null, 'paid once it is recorded as a report is');
});

test('R-1 (zz_d3_4b): a saved report whose case index failed to write is indexed by the next try, and the heal stops owing it', async () => {
  const env = await environment();
  await listing(env);
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  const restore = failOnce(env, /^legalcase:csam:moderation:/);
  await quarantineAs(env, 'admin', ADMIN);
  restore();
  for (let h = 0; h < 4; h++) await quiet(() => sweepLegalReports(env));
  const [rep] = (await reportsOf(env)).filter((r) => r.kind === 'csam');
  assert.equal(env.SOCIAL.store.get(`legalcase:csam:moderation:${SIM}`), rep.id, 'the case index names its report');
  assert.ok(((await loadCase(env, SIM)).history || []).filter((e) => e?.event === 'legal_report_owed').length <= 1, 'not owed again every run');
  assert.equal((await reportsOf(env)).length, 1);
});

// -- R-2 -----------------------------------------------------------------------------------------------------------------

for (const variant of [{ name: 'naming only its report_kind', body: { report_kind: 'csam' }, code: 'since_required', status: 400 }, { name: 'naming nothing', body: {}, code: 'since_required', status: 400 }, { name: 'naming the stale since it was shown', body: { report_kind: 'csam', since: '2026-09-01T00:00:00.000Z' }, code: 'record_changed', status: 409 }]) {
  test(`R-2 (zz_d3_1): a clear of a stale row ${variant.name} never takes the newer stuck csam debt`, async () => {
    const env = await environment();
    await staleStuckListing(env);
    const restore = failPuts(env, /^legalreport:/);
    await quarantineAs(env, 'admin', ADMIN);
    for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedListingReports(env));
    restore();
    assert.equal((await loadCase(env, SIM)).report_owed.stuck?.reason, RETRIES_EXHAUSTED);
    const done = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Old quarantine with no detection on record; reviewed.', ...variant.body });
    assert.deepEqual([done.status, done.body.code], [variant.status, variant.code]);
    assert.equal((await loadCase(env, SIM)).report_owed?.actor, 'admin', 'the person\'s debt stands');
    await quiet(() => writeOwedListingReports(env, {}, { now: () => Date.now() + DAY + 60_000 }));
    assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1, 'and is written at its daily try');
  });
}

test('R-2: a clear or a retry of a row whose record has a report kind must name it', async () => {
  const env = await environment();
  await staleStuckListing(env);
  const { since } = await rowOf(env, 'moderation', SIM);
  for (const action of ['clear', 'retry']) {
    const r = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/${action}`, { note: 'Reviewed the record with care.', since });
    assert.deepEqual([r.status, r.body.code], [400, 'report_kind_required'], action);
  }
  assert.ok((await loadCase(env, SIM)).report_owed?.stuck);
});

// -- R-3 -----------------------------------------------------------------------------------------------------------------

test('R-3 (zz_d3_3): a person\'s csam quarantine of a deleted account is reported at once with the owed sweep off, then confirmed', async () => {
  const env = await environment();
  const GONE = 'author-gone-r3';
  await listing(env, { author: GONE });
  const q = await quarantineAs(env, 'admin', ADMIN);
  assert.equal(q.ok, true);
  const [rep] = (await reportsOf(env)).filter((r) => r.kind === 'csam');
  assert.ok(rep, 'written at once, not held on the account');
  assert.equal(rep.who.account_status, 'unconfirmed');
  assert.equal((await loadCase(env, SIM)).report_owed, null, 'nothing is owed');
  await quiet(() => sweepLegalReports(env, {}, { now: Date.now() + 61_000 }));
  assert.equal((await loadLegalReport(env, rep.id)).who.account_status, 'deleted', 'confirmed on the second read');
});

test('R-3: a person can retry an owed debt (not stuck) at once, audited first, naming its since', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'admin', ADMIN);
  restore();
  const owed = (await loadCase(env, SIM)).report_owed;
  assert.ok(owed && !owed.stuck);
  const path = `/api/admin/legal/stuck/moderation/${SIM}/retry`;
  const blocked = await route(env, 'POST', path, { note: 'The store is back; try it now.', report_kind: 'csam', since: owed.since }, routeDeps({ auditLog: async () => { throw new Error('down'); } }));
  assert.equal(blocked.status, 503);
  const wrong = await route(env, 'POST', path, { note: 'The store is back; try it now.', report_kind: 'csam', since: '2020-01-01T00:00:00.000Z' });
  assert.deepEqual([wrong.status, wrong.body.code], [404, 'not_owed']);
  const done = await route(env, 'POST', path, { note: 'The store is back; try it now.', report_kind: 'csam', since: owed.since });
  assert.deepEqual([done.status, done.body.outcome], [200, 'paid']);
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
});

// -- R-4 -----------------------------------------------------------------------------------------------------------------

for (const [actor, actorId] of [['agent', 'agent'], ['system', 'pipeline']]) {
  test(`R-4 (zz_d3_2): a debt with no recorded detection is not settled by a later ${actor} quarantine`, async () => {
    const env = await environment();
    await staleStuckListing(env, { p: 0.3, by: 'unknown' });
    const before = await loadCase(env, SIM);
    assert.equal((await quarantineAs(env, actor, actorId)).ok, true);
    for (let h = 0; h < 2; h++) await quiet(() => sweepLegalReports(env));
    const c = await loadCase(env, SIM);
    assert.deepEqual([c.report_owed?.since, c.report_owed?.stuck?.reason, c.report_settled], [before.report_owed.since, 'no_detection_recorded', undefined], 'it stands as it was');
    assert.ok(await loadStuckReport(env, 'moderation', SIM), 'and its record');
    assert.equal(auditsOf(env, 'LEGAL_REPORT_STUCK_CLOSED').length, 0);
  });
}

test('R-4: a confident new finding replaces a debt with no recorded detection, and its record closes', async () => {
  const env = await environment();
  await staleStuckListing(env, { p: 0.95, by: 'unknown' });
  await quarantineAs(env, 'system', 'pipeline');
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null);
});

test('R-4: a person\'s retry of a debt with no recorded detection is refused, and the debt is not settled', async () => {
  const env = await environment();
  await staleStuckListing(env, { p: 0.3, by: 'unknown' });
  const r = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/retry`, { note: 'Try the old quarantine again.', ...(await rowOf(env, 'moderation', SIM)) });
  assert.deepEqual([r.status, r.body.code], [409, 'no_detection_recorded']);
  const c = await loadCase(env, SIM);
  assert.deepEqual([c.report_owed?.stuck?.reason, c.report_settled], ['no_detection_recorded', undefined]);
});

// -- R-5 -----------------------------------------------------------------------------------------------------------------

test('R-5: the heal owes a newer quarantine recorded nowhere though an earlier one settled, and skips the quarantine that settled', async () => {
  const env = await environment();
  await listing(env, { p: 0.95 });
  const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  Object.assign(rec, { status: 'quarantined', legal_hold: { category: CAT, rationale: 'a person', at: '2026-10-10T00:00:00.000Z', by: ADMIN }, decision: { reasons: [{ code: CAT, action: 'quarantine', lane: 'legal', by: 'admin' }] }, report_settled: { reason: 'borderline_never_reported', category: CAT, at: '2026-10-12T00:00:00.000Z', hold_at: '2026-10-01T00:00:00.000Z' } });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  // The settlement is later than the hold by its time, but of an earlier quarantine by its hold_at: this one still owes.
  assert.equal((await quiet(() => healListingReportDebts(env))).healed, 1);
  assert.equal((await loadCase(env, SIM)).report_owed?.actor, 'admin');
  // The same settlement of THIS quarantine stands.
  const other = await environment();
  await listing(other, { p: 0.95 });
  await other.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ ...rec, report_settled: { ...rec.report_settled, hold_at: rec.legal_hold.at } }));
  await other.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  assert.equal((await quiet(() => healListingReportDebts(other))).healed, 0);
});

// -- notes ---------------------------------------------------------------------------------------------------------------

test('notes (zz_d3_5): a writer that stalls past the claim and wakes after a takeover writes no second report', async () => {
  const env = await environment();
  const finding = { kind: 'csam', caseKind: 'forum', caseId: POST, category: 'csam', article: 'A1', subject: { kind: 'forum_post', post_id: POST }, detection: { source: 'person', checks: [], band: 'high' }, uploader: { account_id: AUTHOR } };
  let b = null;
  const stalled = { accountState: async () => { b = await writeLegalReport(env, finding, { now: () => Date.now() + REPORT_CLAIM_MS + 60_000 }); return null; } };
  const a = await quiet(() => writeLegalReport(env, finding, stalled));
  assert.equal(b?.written, true, 'the writer that took over wrote it');
  assert.deepEqual([a.written, a.reason], [false, 'busy'], 'the stalled one saves nothing');
  assert.equal((await reportsOf(env)).length, 1);
});

for (const indexed of [false, true]) {
  test(`notes (zz_d3_6): a debt stuck on its tries before next_try_at existed (index ${indexed ? 'kept' : 'deleted'}) is tried, and written`, async () => {
    const env = await environment();
    await listing(env);
    await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
    const restore = failPuts(env, /^legalreport:/);
    await quarantineAs(env, 'admin', ADMIN);
    restore();
    const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
    const at = new Date(Date.now() - 3 * DAY).toISOString();
    rec.report_owed = { ...rec.report_owed, tries: 24, last_reason: 'unavailable', stuck: { reason: RETRIES_EXHAUSTED, at, last_reason: 'unavailable' } };
    await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
    if (!indexed) await env.SOCIAL.delete(listingReportOwedKey(SIM));
    await env.SOCIAL.put(`legalstuck:moderation:${SIM}`, JSON.stringify({ case_kind: 'moderation', case_id: SIM, report_kind: 'csam', reason: RETRIES_EXHAUSTED, since: rec.report_owed.since, stuck_at: at, alerted_at: at, alerts: 1 }));
    await quiet(() => writeOwedListingReports(env));
    assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
  });
}

test('notes: the stuck re-alert sends at most STUCK_ALERTS_PER_RUN alerts a run, and the rest the next run', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * DAY).toISOString();
  for (let n = 1; n <= STUCK_ALERTS_PER_RUN + 10; n++) await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-${n}`, 'csam'), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-${n}`, report_kind: 'csam', reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  const first = await quiet(() => alertStuckReports(env));
  assert.deepEqual([first.alerted, first.capped], [STUCK_ALERTS_PER_RUN, true]);
  const second = await quiet(() => alertStuckReports(env));
  assert.equal(second.alerted, 10);
});

test('notes: a daily try that fails for a new reason writes that reason to the stuck record', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedListingReports(env));
  restore();
  assert.equal((await loadStuckReport(env, 'moderation', SIM, 'csam')).last_reason, 'unavailable');
  // Another writer holds the case's report claim: the daily try comes to busy.
  await env.SCENES.put(`locks/legalcase/csam/moderation/${SIM}.json`, JSON.stringify({ id: crypto.randomUUID(), at: new Date(Date.now() + DAY).toISOString() }));
  await quiet(() => writeOwedListingReports(env, {}, { now: () => Date.now() + DAY + 60_000 }));
  assert.equal((await loadStuckReport(env, 'moderation', SIM, 'csam')).last_reason, 'busy');
});

test('notes: a quarantine answered busy reaches the admin as 409', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  await env.SCENES.put(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'someone-else', sim_id: SIM, expires_at: Date.now() + 3600_000 }));
  const url = new URL('https://api.eustress.dev/api/admin/moderation/tool');
  const res = await quiet(() => handleModerationRoute(new Request(url, { method: 'POST', body: JSON.stringify({ name: 'moderation_quarantine', args: { sim_id: SIM, category: CAT, rationale: 'A rationale that is at least twenty characters long.' } }) }), url, env, {}, { json, cors: {}, requireAdmin: async () => ADMIN, auditLog: async () => {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED }));
  assert.equal(res.status, 409);
});

test('notes: the seal-and-report case write refuses after its lease was taken by another run', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  // The lease is taken over by another run while the quarantine's report is being tried.
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.get = async (key) => {
    if (armed && key === `sim:${SIM}`) { armed = false; await env.SCENES.put(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'another-run', sim_id: SIM, expires_at: Date.now() + 3600_000 })); }
    return get(key);
  };
  await quarantineAs(env, 'admin', ADMIN);
  env.SOCIAL.get = get;
  restore();
  const rec = await loadCase(env, SIM);
  assert.equal(rec.legal_report, undefined, 'nothing was written after the lease was lost');
});
