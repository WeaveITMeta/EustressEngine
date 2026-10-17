// Security's second review of the report debts (ac3301f6): F1 to F7 and the notes, each adapted from its probe (zz_dfix_1 to
// 7; zz_dfix_5 is the staff alert queue's, not here) with a mutation that fails it. A stuck record is one debt's (its kind and
// since) and goes when that debt does; a clear names the record it was shown; each kind has its own record; a slow first debt
// does not starve the cases behind it; tries count only when they fail the same way, and a debt stuck on its tries is still
// tried daily, or at once by a person; a quarantine runs under the case lease and ranks against the case as it is; a
// settlement stands only for its own quarantine; one report per case even with two writers.
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
  REPORT_DEBT_MAX_TRIES, RETRIES_EXHAUSTED,
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

// -- helpers of this round -------------------------------------------------------------------------------------------------

const stuckSubjects = (env) => subjects(env).filter((s) => s.startsWith('A legal report is stuck'));
const stuckTexts = (env) => env.mail.filter((m) => m.to === 'security@eustress.dev').map((m) => {
  const at = m.raw.indexOf('Content-Type: text/plain; charset=utf-8');
  const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
  return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
}).filter((t) => t.includes('stuck'));
const DAY = 86400000;

async function staleStuckListing(env) {
  await listing(env, { p: 0.95 });
  const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  Object.assign(rec, { status: 'quarantined', legal_hold: { category: CAT, rationale: 'old quarantine', at: '2026-09-01T00:00:00.000Z', by: 'pipeline' }, decision: { reasons: [{ code: CAT, action: 'quarantine', lane: 'legal' }] } });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await env.SOCIAL.put(`modq:quarantined:${SIM}`, '1');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  await quiet(() => sweepLegalReports(env));
  assert.equal((await loadStuckReport(env, 'moderation', SIM))?.reason, 'no_detection_recorded');
}

// -- F1 ------------------------------------------------------------------------------------------------------------------

test('F1 (zz_dfix_1): a stale stuck record is closed when its debt is replaced, and a clear of what was shown never takes the newer debt', async () => {
  const env = await environment();
  await staleStuckListing(env);
  const shown = (await route(env, 'GET', '/api/admin/legal/stuck')).body.items;
  assert.deepEqual(shown.map((i) => [i.reason, i.since]), [['no_detection_recorded', '2026-09-01T00:00:00.000Z']]);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'admin', ADMIN);
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null, 'the stale record went with the debt it named');
  const mails = stuckSubjects(env).length;
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedListingReports(env));
  restore();
  const rec = await loadCase(env, SIM);
  assert.deepEqual([rec.report_owed.actor, rec.report_owed.stuck?.reason], ['admin', RETRIES_EXHAUSTED]);
  const now = (await route(env, 'GET', '/api/admin/legal/stuck')).body.items;
  assert.deepEqual(now.map((i) => [i.reason, i.since, i.last_reason]), [[RETRIES_EXHAUSTED, rec.report_owed.since, 'unavailable']], 'the record of the person\'s debt is listed');
  assert.equal(stuckSubjects(env).length, mails + 1, 'and staff were told of it');
  // A clear of the record the person was shown before (the stale one) is refused: the debt is not that one.
  const done = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Old quarantine with no detection on record; reviewed.', report_kind: 'csam', since: shown[0].since });
  assert.deepEqual([done.status, done.body.code], [409, 'record_changed']);
  assert.equal((await loadCase(env, SIM)).report_owed.actor, 'admin', 'the person\'s debt stands');
  // The store is back: the daily try writes it, and its stuck record closes.
  await quiet(() => writeOwedListingReports(env, {}, { now: () => Date.now() + DAY + 60_000 }));
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
  assert.equal((await loadCase(env, SIM)).report_owed, null);
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null);
});

test('F1: a clear whose record names another debt than the case holds is refused, and the record is made the case\'s again', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  restore();
  const rec = await loadCase(env, SIM);
  rec.report_owed.stuck = { reason: 'no_detection_recorded', at: new Date().toISOString() };
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  // The record names an older debt (another since) than the stuck one the case holds.
  await env.SOCIAL.put(legalStuckKey('moderation', SIM, 'csam'), JSON.stringify({ case_kind: 'moderation', case_id: SIM, report_kind: 'csam', reason: 'no_detection_recorded', since: '2026-01-01T00:00:00.000Z', stuck_at: new Date().toISOString(), alerts: 0 }));
  const done = await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Reviewed the old quarantine.', report_kind: 'csam', since: '2026-01-01T00:00:00.000Z' });
  assert.deepEqual([done.status, done.body.code], [409, 'debt_changed']);
  assert.ok((await loadCase(env, SIM)).report_owed, 'nothing was cleared');
  assert.equal((await loadStuckReport(env, 'moderation', SIM, 'csam')).since, rec.report_owed.since, 'the record now names the case\'s debt');
  assert.equal((await route(env, 'POST', `/api/admin/legal/stuck/moderation/${SIM}/clear`, { note: 'Reviewed the current debt.', report_kind: 'csam', since: rec.report_owed.since })).status, 200);
});

// -- F2 ------------------------------------------------------------------------------------------------------------------

test('F2 (zz_dfix_2): a csam debt stuck beside a stuck threat debt on one post has its own record, alert and list item', async () => {
  const env = await environment();
  const restore = failPuts(env, /^legalreport:/);
  assert.equal((await threatRemoval(env)).report, 'unavailable');
  assert.equal((await checkRemoval(env)).report, 'unavailable');
  await env.SCENES.rewriteCase(POST, (r) => { r.author_id = null; });
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedForumReports(env));
  restore();
  const kinds = forumDebts(await loadForumCase(env, POST)).map((d) => [d.category, d.stuck?.reason]).sort();
  assert.deepEqual(kinds, [['csam', RETRIES_EXHAUSTED], ['threat', 'no_account']]);
  const listed = (await route(env, 'GET', '/api/admin/legal/stuck')).body.items.map((i) => [i.report_kind, i.reason]).sort();
  assert.deepEqual(listed, [['csam', RETRIES_EXHAUSTED], ['threat', 'no_account']], 'each kind is listed');
  assert.equal(stuckSubjects(env).length, 2, 'and alerted');
  // The threat's clear names its kind and leaves the csam debt.
  assert.deepEqual([(await route(env, 'POST', `/api/admin/legal/stuck/forum/${POST}/clear`, { note: 'Reviewed: two records here.', since: null })).body.code], ['report_kind_required']);
  assert.equal((await route(env, 'POST', `/api/admin/legal/stuck/forum/${POST}/clear`, { note: 'Reviewed: the threat was filed by hand.', ...(await rowOf(env, 'forum', POST, 'threat')) })).status, 200);
  assert.deepEqual(forumDebts(await loadForumCase(env, POST)).map((d) => d.category), ['csam']);
  assert.ok(await loadStuckReport(env, 'forum', POST, 'csam'));
});

// -- F3 ------------------------------------------------------------------------------------------------------------------

test('F3 (zz_dfix_3): a slow first debt of a two-debt case does not starve the forum debts behind it', async () => {
  const env = await environment();
  const P2 = `${THREAD}-2`;
  const P3 = `${THREAD}-3`;
  const restore = failPuts(env, /^legalreport:/);
  await threatRemoval(env);
  await checkRemoval(env);
  for (const p of [P2, P3]) {
    await quiet(() => fileForumReport({ env, postId: p, threadId: THREAD, postAuthorId: AUTHOR2, reporterId: REPORTER, category: 'spam', postBody: 'text', now: Date.now() }));
    await quiet(() => resolveForumCase({ env, postId: p, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: 'text' }));
  }
  restore();
  let clock = 0;
  let failNext = false;
  const deps = { accountState: async (_env, id) => { if (id === AUTHOR) { clock += 21_000; failNext = true; } return null; } };
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (k, ...r) => { if (failNext && String(k).startsWith('legalreport:')) { failNext = false; throw new Error('KV down'); } return real(k, ...r); };
  const base = Date.now();
  for (let i = 0; i < 3; i++) await quiet(() => writeOwedForumReports(env, deps, { now: () => base + clock }));
  env.SOCIAL.put = real;
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(P2)), null, 'post 2 is reported');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(P3)), null, 'post 3 is reported');
});

// -- F4 ------------------------------------------------------------------------------------------------------------------

test('F4 (zz_dfix_4): a csam debt stuck by a day-long outage is tried daily and written once the store is back', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => sweepLegalReports(env));
  assert.equal((await loadCase(env, SIM)).report_owed.stuck?.reason, RETRIES_EXHAUSTED);
  assert.ok(stuckTexts(env).some((t) => t.includes('last failure: unavailable')), 'the alert names the reason the tries kept failing');
  // Still down a day later: the daily try fails, and the next is a day after it.
  await quiet(() => writeOwedListingReports(env, {}, { now: () => Date.now() + DAY + 60_000 }));
  const next = Date.parse((await loadCase(env, SIM)).report_owed.stuck.next_try_at);
  assert.ok(next > Date.now() + DAY / 2, 'moved a day on');
  restore();
  // The store is back, but the next try is not due within the hour: nothing is tried.
  await quiet(() => writeOwedListingReports(env));
  assert.equal((await reportsOf(env)).length, 0);
  // At the daily try it is written, and the stuck record closes.
  await quiet(() => writeOwedListingReports(env, {}, { now: () => next + 60_000 }));
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null);
  assert.equal(await env.SOCIAL.get(listingReportOwedKey(SIM)), null);
});

test('F4: only tries that fail the same way in a row count; a different reason starts the count again', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  const good = await env.SOCIAL.get(`sim:${SIM}`);
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES + 6; hour++) {
    // Every other hour the listing record cannot be read (an exception), the rest the report store is down (unavailable).
    await env.SOCIAL.put(`sim:${SIM}`, hour % 2 ? '{not json' : good);
    await quiet(() => writeOwedListingReports(env));
  }
  restore();
  const debt = (await loadCase(env, SIM)).report_owed;
  assert.equal(debt.stuck, undefined, 'never the same reason twice in a row: not stuck');
  assert.equal(debt.tries, 1);
});

test('F4: a person can try a stuck debt again now, audited first and fail-closed; a try that pays closes it', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  await quarantineAs(env, 'system', 'pipeline');
  for (let hour = 0; hour < REPORT_DEBT_MAX_TRIES; hour++) await quiet(() => writeOwedListingReports(env));
  restore();
  const path = `/api/admin/legal/stuck/moderation/${SIM}/retry`;
  const row = await rowOf(env, 'moderation', SIM, 'csam');
  const blocked = await route(env, 'POST', path, { note: 'The store is back; try it now.', ...row }, routeDeps({ auditLog: async () => { throw new Error('down'); } }));
  assert.deepEqual([blocked.status, blocked.body.code], [503, 'audit_unavailable']);
  assert.equal((await loadCase(env, SIM)).report_owed.stuck?.reason, RETRIES_EXHAUSTED, 'nothing changed');
  const done = await route(env, 'POST', path, { note: 'The store is back; try it now.', ...row });
  assert.deepEqual([done.status, done.body.outcome], [200, 'paid']);
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
  assert.equal(await loadStuckReport(env, 'moderation', SIM), null);
  assert.deepEqual([auditsOf(env, 'LEGAL_STUCK_RETRY_REQUESTED').length, auditsOf(env, 'LEGAL_STUCK_RETRIED').map((a) => a.details.outcome)], [1, ['paid']]);
});

// -- F6 ------------------------------------------------------------------------------------------------------------------

test('F6: a quarantine while the case is being written is answered busy and changes nothing', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  await env.SCENES.put(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'someone-else', sim_id: SIM, expires_at: Date.now() + 3600_000 }));
  const r = await quarantineAs(env, 'admin', ADMIN);
  assert.deepEqual([r.ok, r.code], [false, 'busy']);
  assert.equal((await loadCase(env, SIM)).status, 'held');
});

test('F6 (zz_dfix_6): an agent\'s try ranks against the debt the case holds now, not the copy it loaded', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  // A person's confident debt lands on the case while the agent's quarantine is in flight (written past the lease, or read
  // stale by the agent): the agent's try must see it.
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.get = async (key) => {
    if (armed && key === `sim:${SIM}`) {
      armed = false;
      const c = JSON.parse(await get(`modcase:${SIM}`));
      c.report_owed = { since: new Date().toISOString(), category: CAT, actor: 'admin', reason: 'unavailable', detection: { source: 'person', checks: [{ kind: 'person', source: 'moderation_quarantine', category: CAT, confidence: null, band: 'high' }], band: 'high', viewed_by_staff: false } };
      await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(c));
      await env.SOCIAL.put(listingReportOwedKey(SIM), c.report_owed.since);
    }
    return get(key);
  };
  const a = await quarantineAs(env, 'agent', 'agent');
  env.SOCIAL.get = get;
  assert.equal(a.ok, true, JSON.stringify(a));
  const csam = (await reportsOf(env)).filter((r) => r.kind === 'csam');
  assert.deepEqual(csam.map((r) => r.what.detection.source), ['person'], 'the person\'s finding was reported at once');
  assert.equal((await loadCase(env, SIM)).report_settled, undefined);
});

test('F6: a stronger debt recorded after the try began stands: the try\'s weaker outcome neither settles nor replaces it', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  const put = env.SCENES.put.bind(env.SCENES);
  let armed = true;
  let since = null;
  // The person's debt is written while the agent's seal is being written (after its try read the case).
  env.SCENES.put = async (key, ...rest) => {
    if (armed && String(key).startsWith('evidence/')) {
      armed = false;
      const c = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
      since = new Date().toISOString();
      c.report_owed = { since, category: CAT, actor: 'admin', reason: 'unavailable', detection: { source: 'person', checks: [], band: 'high', viewed_by_staff: false } };
      await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(c));
      await env.SOCIAL.put(listingReportOwedKey(SIM), since);
    }
    return put(key, ...rest);
  };
  await quarantineAs(env, 'agent', 'agent');
  env.SCENES.put = put;
  assert.ok(!armed, 'the debt was written during the agent\'s try');
  const rec = await loadCase(env, SIM);
  assert.deepEqual([rec.report_owed?.actor, rec.report_owed?.since, rec.report_settled], ['admin', since, undefined], 'the person\'s debt stands, unsettled');
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'and indexed');
  await quiet(() => writeOwedListingReports(env));
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
});

// -- F7 ------------------------------------------------------------------------------------------------------------------

test('F7 (zz_dfix_7): the heal owes a person\'s csam quarantine recorded nowhere, though an earlier agent quarantine settled', async () => {
  const env = await environment();
  await listing(env, { p: 0.3 });
  assert.equal((await quarantineAs(env, 'agent', 'agent')).ok, true);
  assert.equal((await loadCase(env, SIM)).report_settled?.reason, 'borderline_never_reported');
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  let caseWrites = 0;
  env.SOCIAL.put = async (key, ...rest) => {
    const k = String(key);
    if (k.startsWith('legalreport:') || k.startsWith('legalowed:moderation:')) throw new Error('KV is unavailable');
    if (k === `modcase:${SIM}` && ++caseWrites > 1) throw new Error('KV is unavailable');
    return real(key, ...rest);
  };
  await quarantineAs(env, 'admin', ADMIN);
  env.SOCIAL.put = real;
  assert.equal((await loadCase(env, SIM)).legal_hold?.by, ADMIN);
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  for (let hour = 0; hour < 3; hour++) await quiet(() => sweepLegalReports(env));
  const csam = (await reportsOf(env)).filter((r) => r.kind === 'csam');
  assert.deepEqual(csam.map((r) => r.what.detection.source), ['person']);
});

// -- notes ---------------------------------------------------------------------------------------------------------------

test('notes: two writers of one case\'s report at once write one report (the claim in R2)', async () => {
  const env = await environment();
  const finding = { kind: 'csam', caseKind: 'forum', caseId: POST, category: 'csam', article: 'A1', subject: { kind: 'forum_post', post_id: POST }, detection: { source: 'person', checks: [], band: 'high' }, uploader: { account_id: AUTHOR } };
  const both = await quiet(() => Promise.all([writeLegalReport(env, finding), writeLegalReport(env, finding)]));
  assert.equal((await reportsOf(env)).length, 1, JSON.stringify(both));
  assert.deepEqual(both.map((r) => r.written).sort(), [false, true]);
  // A writer that failed lets its claim go: the next try is not answered busy.
  const other = await environment();
  const restore = failPuts(other, /^legalreport:/);
  assert.equal((await quiet(() => writeLegalReport(other, finding))).reason, 'unavailable');
  restore();
  assert.equal((await quiet(() => writeLegalReport(other, finding))).written, true);
});

test('notes: the daily stuck re-alert reads one page per kind a run and goes on from its cursor', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * DAY).toISOString();
  for (const n of [1, 2, 3]) await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-${n}`, 'csam'), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-${n}`, report_kind: 'csam', reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  const first = await quiet(() => alertStuckReports(env, { limit: 2 }));
  assert.deepEqual([first.stuck, first.alerted], [2, 2], 'one page');
  const second = await quiet(() => alertStuckReports(env, { limit: 2 }));
  assert.deepEqual([second.stuck, second.alerted], [1, 1], 'the next run goes on');
});
