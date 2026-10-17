// A forum case's legal report debt (src/moderation.mjs, src/legal_reports.mjs), forums round 9: the owed index written before the
// report with a bounded retry, and a heal that finds a debt whose index was never written (R9-12); the owed sweep's cursor;
// the case lease released outside the forum sweep's budget (R9-8); the decision's own time in resolveForumCase's answer (R9-4);
// the report debt's outcomes (paid, settled, owed, stuck), the stuck index, its alert and the person who clears it; and two
// clock checks on the addresses a report carries.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { fakeKv } from './helpers/load_worker.mjs';
import {
  fileForumReport, resolveForumCase, loadForumCase, sealForumEvidence, forumCaseR2Key, forumPostIpKey, forumUploadIpsOf,
  writeOwedForumReports, forumReportOwedKey, FORUM_OWED_CURSOR_KEY, UNBUDGETED_ENV, clearReportDebt,
} from '../src/moderation.mjs';
import { VAULT_PREFIX } from '../src/evidence_vault.mjs';
import {
  loadLegalReport, sweepLegalReports, reportDebtOutcome, SETTLED_REPORT_REASONS, OWED_REPORT_REASONS, legalStuckKey, loadStuckReport,
  clearStuckReport, handleLegalReportRoute, STUCK_ALERT_EVERY_MS,
} from '../src/legal_reports.mjs';

const THREAD = 'd'.repeat(24);
const POST = `${THREAD}-1`;
const AUTHOR = 'author-0901';
const ADMIN = 'admin-0901';
const REPORTER = 'reporter-0901';
const NOW = Date.parse('2026-10-16T18:00:00.000Z');
const SECRET = 'test-secret-forum-debts';
const TEXT = 'NUMBAT-CANARY forum post text';
const IP = '198.51.100.77';
const enc = new TextEncoder();
const dec = new TextDecoder();

// R2 as it behaves (the forum_case_store harness): conditional puts, an etag that is the MD5 of the content, a paged list, and
// `hooks.beforePut` to act inside a put (make it slow, make it fail).
function bucket() {
  const objects = new Map();
  const hooks = { beforePut: null };
  const view = (key, o) => ({
    key, size: o.bytes.byteLength, etag: o.etag, customMetadata: o.customMetadata || {}, uploaded: new Date(), checksums: {},
    text: async () => dec.decode(o.bytes), arrayBuffer: async () => o.bytes.slice().buffer,
  });
  return {
    objects, hooks,
    async put(key, value, opts = {}) {
      const bytes = typeof value === 'string' ? enc.encode(value) : value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength));
      if (hooks.beforePut) await hooks.beforePut(key, opts);
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
      return {
        objects: page.map((key) => ({ key, size: objects.get(key).bytes.byteLength, ...(include.includes('customMetadata') ? { customMetadata: objects.get(key).customMetadata || {} } : {}) })),
        truncated, ...(truncated ? { cursor: page.at(-1) } : {}),
      };
    },
    // Writes the case directly, as another writer would.
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

// KV whose list pages with a cursor (the name of the last key of the page), as KV's does.
function pagedKv() {
  const kv = fakeKv();
  kv.lists = [];
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    kv.lists.push({ prefix, limit, cursor: cursor || null });
    const names = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
    const page = names.slice(0, limit);
    const more = names.length > limit;
    return { keys: page.map((name) => ({ name })), list_complete: !more, ...(more ? { cursor: page.at(-1) } : {}) };
  };
  return kv;
}

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

async function environment({ social = fakeKv() } = {}) {
  const env = { USERS: fakeKv(), SOCIAL: social, AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), SCENES: bucket(), JWT_SECRET: SECRET, __EmailMessage: StubMessage, mail: [] };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  const old = new Date(NOW - 30 * 86400 * 1000).toISOString();
  for (const id of [AUTHOR, ADMIN, REPORTER]) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: old }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return env;
}

const quiet = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};
const fileReport = (env, postId = POST, extra = {}) => quiet(() => fileForumReport({ env, postId, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTER, category: 'spam', postBody: TEXT, now: NOW, ...extra }));
const remove = (env, postId = POST, extra = {}) => quiet(() => resolveForumCase({ env, postId, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT, ...extra }));
const reportsOf = async (env) => Promise.all([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalreport:')).map((k) => loadLegalReport(env, k.slice('legalreport:'.length))));
const auditsOf = (env, action) => [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v)).filter((a) => a.action === action);
const keyedItem = (text) => `post-text-${crypto.createHmac('sha256', crypto.createHmac('sha256', SECRET).update('forum-evidence-v1').digest()).update(text, 'utf8').digest('hex').slice(0, 32)}`;
const metaOf = (env, postId = POST) => env.SCENES.objects.get(forumCaseR2Key(postId)).customMetadata;
const vaultSnapshot = (env) => JSON.stringify([...env.SCENES.objects.entries()].filter(([k]) => k.startsWith(VAULT_PREFIX)).map(([k, o]) => [k, o.etag]).sort());

// SOCIAL puts whose key matches `pattern` throw while `on()` says so (a KV outage for those keys).
function failPuts(env, pattern, on = () => true) {
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (key, ...rest) => { if (pattern.test(String(key)) && on(String(key))) throw new Error('KV is unavailable'); return real(key, ...rest); };
  return () => { env.SOCIAL.put = real; };
}

function readMail(m) {
  const part = (type) => {
    const at = m.raw.indexOf(`Content-Type: ${type}; charset=utf-8`);
    if (at < 0) return '';
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
  };
  const subjectLine = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
  const encoded = /^=\?UTF-8\?B\?(.*)\?=$/.exec(subjectLine);
  return { to: m.to, subject: encoded ? Buffer.from(encoded[1], 'base64').toString('utf8') : subjectLine, text: part('text/plain'), html: part('text/html') };
}
const stuckMails = (env) => env.mail.map(readMail).filter((m) => /A legal report is stuck/.test(m.subject));

// -- R9-12: the owed index, its retry and the heal --------------------------------------------------------------------------

test('R9-12: a csam removal whose owed index and report both fail in one KV outage is reported by the next owed sweep', async () => {
  const env = await environment();
  await fileReport(env);
  const restore = failPuts(env, /^(legalreport:|legalowed:)/);
  const r = await remove(env);
  restore();
  assert.deepEqual([r.ok, r.follow_up, r.pending.includes('legal_report')], [true, 'pending', true], JSON.stringify(r));
  const rec = await loadForumCase(env, POST);
  assert.ok(rec.report_owed, 'the case owes the report');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null, 'and its index was never written');
  assert.equal(metaOf(env).owed, '1', 'the debt is in the case\'s own R2 metadata, written in the same put');
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  const swept = await quiet(() => sweepLegalReports(env));
  assert.equal(swept.owed_reports.healed, 1, 'the heal wrote the missing index');
  assert.equal(swept.owed_reports.written, 1);
  const reports = await reportsOf(env);
  assert.deepEqual(reports.map((x) => [x.kind, x.when_where.case_id]), [['csam', POST]], 'the owed csam report is written by the first sweep');
  assert.equal((await loadForumCase(env, POST)).report_owed, null);
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null);
  assert.equal(metaOf(env).owed, '0');
});

test('R9-12: the owed index is tried again before the report; one that still fails leaves the report pending and the debt on the case', async () => {
  // Two failures, then KV takes it: the index is written, and the report (still failing) is pending for the sweep.
  const env = await environment();
  await fileReport(env);
  let failures = 0;
  const restore = failPuts(env, /^(legalreport:|legalowed:)/, (key) => (key.startsWith('legalowed:') ? failures++ < 2 : true));
  const r = await remove(env);
  restore();
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)) !== null, true, 'written on the third try');
  assert.ok(r.pending.includes('legal_report'), 'the report itself is still owed');
  // The index never takes, the report does: nothing is owed, so nothing is pending.
  const paid = await environment();
  await fileReport(paid);
  const back = failPuts(paid, /^legalowed:/);
  const ok = await remove(paid);
  back();
  assert.deepEqual([ok.ok, ok.pending], [true, []]);
  assert.equal((await reportsOf(paid)).length, 1);
  assert.equal((await loadForumCase(paid, POST)).report_owed, null);
});

test('R9-12: the owed sweep reads its index with a cursor, so a case that stays owed never blocks the ones after it', async () => {
  const env = await environment({ social: pagedKv() });
  const posts = [`${THREAD}-1`, `${THREAD}-2`, `${THREAD}-3`];
  for (const p of posts) await fileReport(env, p);
  const restore = failPuts(env, /^legalreport:/);
  for (const p of posts) assert.equal((await remove(env, p)).ok, true);
  restore();
  for (const p of posts) assert.ok(await env.SOCIAL.get(forumReportOwedKey(p)), `${p} is indexed`);
  // The first case is held by another writer for the whole test: it stays owed every run.
  await env.SCENES.put(`locks/forum/${posts[0]}.json`, JSON.stringify({ holder: 'someone-else', expires_at: Date.now() + 3600_000 }));
  const run = () => quiet(() => writeOwedForumReports(env, {}, { limit: 1, maxPages: 1 }));
  const first = await run();
  assert.deepEqual([first.still_owed, first.written, first.complete], [1, 0, false]);
  assert.equal(await env.SOCIAL.get(FORUM_OWED_CURSOR_KEY), forumReportOwedKey(posts[0]), 'the cursor of the next page is kept');
  const second = await run();
  assert.equal(second.written, 1, 'the next run goes on past the case that stays owed');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(posts[1])), null);
  const third = await run();
  assert.deepEqual([third.written, third.complete], [1, true]);
  assert.equal(await env.SOCIAL.get(FORUM_OWED_CURSOR_KEY), null, 'the run that reached the end starts the next from the beginning');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(posts[0])), 'the held case still owes');
  // A cursor KV no longer takes is dropped, and the index is read from the start.
  await env.SCENES.delete(`locks/forum/${posts[0]}.json`);
  await env.SOCIAL.put(FORUM_OWED_CURSOR_KEY, 'stale-cursor');
  const list = env.SOCIAL.list;
  env.SOCIAL.list = async (opts) => { if (opts?.cursor === 'stale-cursor') throw new Error('bad cursor'); return list(opts); };
  const fresh = await run();
  env.SOCIAL.list = list;
  assert.equal(fresh.written, 1);
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(posts[0])), null);
  assert.equal((await reportsOf(env)).length, 3);
});

// -- R9-8: the lease is released outside the sweep's budget -----------------------------------------------------------------

// The forum sweep's budget wrapper (Website's src/forums.mjs budgetedEnv, its shape): every binding call is counted, and once
// the budget is spent each throws. `unwraps` answers the wrapped env at UNBUDGETED_ENV, the one line Website's wrapper adds.
function budgeted(env, budget, { unwraps = true } = {}) {
  const spend = () => { if (budget.spent) throw new Error('the forum sweep budget is spent'); };
  const wrap = (b) => new Proxy(b, { get(t, prop) { const v = t[prop]; return typeof v === 'function' ? async (...a) => { spend(); return v.apply(t, a); } : v; } });
  const wrapped = new Map();
  return new Proxy(env, {
    get(t, prop) {
      if (unwraps && prop === UNBUDGETED_ENV) return t[UNBUDGETED_ENV] || t;
      const v = t[prop];
      if (!v || typeof v !== 'object' || (typeof v.get !== 'function' && typeof v.send !== 'function')) return v;
      if (!wrapped.has(prop)) wrapped.set(prop, wrap(v));
      return wrapped.get(prop);
    },
  });
}

async function sealWhileTheBudgetRunsOut(env, opts) {
  const budget = { spent: false };
  // The budget runs out while the seal holds the lease: during its case write.
  env.SCENES.hooks.beforePut = async (key, o) => { if (key === forumCaseR2Key(POST) && o.onlyIf?.etagMatches) budget.spent = true; };
  const r = await quiet(() => sealForumEvidence({ env: budgeted(env, budget, opts), postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: TEXT, category: 'csam', reviewerId: ADMIN, now: NOW }));
  env.SCENES.hooks.beforePut = null;
  return r;
}

test('R9-8: a budget spent inside the seal still releases the case lease, and the next staff decision is not answered busy', async () => {
  const env = await environment();
  await fileReport(env);
  const r = await sealWhileTheBudgetRunsOut(env);
  assert.deepEqual([r.ok, r.code], [false, 'unavailable'], 'the seal stopped on the budget');
  assert.equal(env.SCENES.objects.has(`locks/forum/${POST}.json`), false, 'the lease is not left held');
  const decided = await quiet(() => resolveForumCase({ env, postId: POST, decision: 'restore', reviewerId: ADMIN }));
  assert.equal(decided.ok, true, JSON.stringify(decided));
  // Control: a wrapper that does not answer UNBUDGETED_ENV (Website's line missing) leaves the lease held, so the rig can fail.
  const control = await environment();
  await fileReport(control);
  await sealWhileTheBudgetRunsOut(control, { unwraps: false });
  assert.equal(control.SCENES.objects.has(`locks/forum/${POST}.json`), true, 'without the unwrapped env the release is refused by the budget');
});

// -- R9-4: the decision's own time -----------------------------------------------------------------------------------------

// The post-push case write fails (R2 refuses the case's second put of the call), which reaches afterForumDecision's catch.
function failSecondCaseWrite(env) {
  let n = 0;
  env.SCENES.hooks.beforePut = async (key) => { if (key === forumCaseR2Key(POST) && ++n >= 2) throw new Error('R2 is unavailable'); };
  return () => { env.SCENES.hooks.beforePut = null; };
}

test('R9-4: resolveForumCase answers decided_at, the decision\'s recorded time, on success and on every pending answer', async () => {
  const env = await environment();
  await fileReport(env);
  const ok = await remove(env);
  const rec = await loadForumCase(env, POST);
  assert.equal(ok.decided_at, rec.decisions.at(-1).at);
  assert.ok(Number.isFinite(Date.parse(ok.decided_at)) && ok.decided_at.endsWith('Z'));
  // A pending answer from the steps after the decision.
  const pending = await environment();
  await fileReport(pending);
  const restore = failPuts(pending, /^legalreport:/);
  const p = await remove(pending);
  restore();
  assert.equal(p.follow_up, 'pending');
  assert.equal(p.decided_at, (await loadForumCase(pending, POST)).decisions.at(-1).at);
  // The pending answer of resolveForumCase's own catch (a step after the decision threw).
  const thrown = await environment();
  await fileReport(thrown);
  const back = failSecondCaseWrite(thrown);
  const saved = console.error;
  console.error = (msg) => { if (String(msg).startsWith('forum case: the push outcome was not recorded')) throw new Error('a step after the decision threw'); };
  let c;
  try { c = await resolveForumCase({ env: thrown, postId: POST, decision: 'remove', category: 'csam', reviewerId: ADMIN, postBody: TEXT }); }
  finally { console.error = saved; back(); }
  assert.deepEqual([c.ok, c.follow_up, c.pending], [true, 'pending', ['case_write', 'legal_report']]);
  assert.equal(c.decided_at, (await loadForumCase(thrown, POST)).decisions.at(-1).at);
});

// -- the clock the addresses are judged at ---------------------------------------------------------------------------------

test('forumUploadIpsOf with a clock that is not a number judges at the machine\'s clock: a 31-day pair and a future pair are dropped', () => {
  const at = (ms) => new Date(Date.now() + ms).toISOString();
  const list = [
    { ip: '192.0.2.31', at: at(-31 * 86400 * 1000) },
    { ip: '192.0.2.32', at: at(3600 * 1000) },
    { ip: '192.0.2.33', at: at(-86400 * 1000) },
  ];
  for (const now of [NaN, null, 'soon']) assert.deepEqual(forumUploadIpsOf(list, now).map((e) => e.ip), ['192.0.2.33'], String(now));
});

test('afterForumDecision\'s catch branch reads the report\'s addresses at the clock resolveForumCase was given', async () => {
  const PAST = Date.parse('2026-01-01T12:00:00.000Z');
  const pair = { ip: '192.0.2.95', at: new Date(PAST - 29.5 * 86400 * 1000).toISOString(), item_id: keyedItem(TEXT) };
  const env = await environment();
  await fileReport(env, POST, { now: PAST });
  await env.SOCIAL.put(forumPostIpKey(POST, keyedItem(TEXT)), JSON.stringify(pair));
  const back = failSecondCaseWrite(env);
  const r = await remove(env, POST, { now: PAST });
  back();
  assert.ok(r.pending.includes('case_write'), 'the post-push case write failed, so the catch branch ran');
  const [rep] = await reportsOf(env);
  assert.deepEqual(rep.who.upload_ips, [pair], 'kept: 29.5 days old at the given clock');
});

// -- the debt's outcomes ---------------------------------------------------------------------------------------------------

test('a report try comes to paid, settled, owed or stuck, and a reason named nowhere is stuck', () => {
  assert.equal(reportDebtOutcome({ written: true, id: 'x', status: 'ready_to_send' }), 'paid');
  assert.equal(reportDebtOutcome({ written: false, reason: 'already_reported' }), 'paid');
  for (const reason of ['not_a_reported_kind', 'borderline_never_reported', 'threat_criteria_not_met']) assert.equal(reportDebtOutcome({ written: false, reason }), 'settled', reason);
  for (const reason of ['unavailable', 'busy', 'budget']) assert.equal(reportDebtOutcome({ written: false, reason }), 'owed', reason);
  assert.equal(reportDebtOutcome(null), 'owed', 'a try that did not get that far');
  for (const reason of ['no_account', 'something_new', undefined]) assert.equal(reportDebtOutcome({ written: false, reason }), 'stuck', String(reason));
  assert.deepEqual([...SETTLED_REPORT_REASONS], ['not_a_reported_kind', 'borderline_never_reported', 'threat_criteria_not_met']);
  assert.deepEqual([...OWED_REPORT_REASONS], ['unavailable', 'busy', 'budget']);
});

// A case that owes a threat report on a person's finding: the threat criteria (two independent classifiers) cannot be met.
async function personThreatDebt(env) {
  await fileReport(env);
  await env.SCENES.rewriteCase(POST, (r) => { r.removed = true; r.removed_at = new Date(NOW).toISOString(); r.report_owed = { since: new Date(NOW).toISOString(), category: 'threat', reason: 'person_removal', by: ADMIN }; });
  await env.SOCIAL.put(forumReportOwedKey(POST), new Date(NOW).toISOString());
}

test('a settled debt is closed with its reason, audited first; an audit that cannot be written keeps it owed', async () => {
  const env = await environment();
  await personThreatDebt(env);
  // The audit log is down: nothing is settled unaudited.
  const audit = env.AUDIT_LOG;
  env.AUDIT_LOG = { put: async () => { throw new Error('down'); }, store: new Map() };
  const blocked = await quiet(() => writeOwedForumReports(env));
  env.AUDIT_LOG = audit;
  assert.deepEqual([blocked.settled, blocked.still_owed], [0, 1]);
  assert.ok((await loadForumCase(env, POST)).report_owed, 'still owed');
  assert.ok(await env.SOCIAL.get(forumReportOwedKey(POST)));
  const out = await quiet(() => writeOwedForumReports(env));
  assert.deepEqual([out.settled, out.still_owed], [1, 0]);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.report_owed, null);
  assert.deepEqual([rec.report_settled.reason, rec.report_settled.category], ['threat_criteria_not_met', 'threat']);
  assert.equal(auditsOf(env, 'LEGAL_REPORT_SETTLED').length, 1);
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null);
  assert.equal((await reportsOf(env)).length, 0, 'no report is owed, and none is written');
  assert.equal(await loadStuckReport(env, 'forum', POST), null);
});

// A data defect: a case that owes a threat report the criteria are met for (two independent classifiers, specific, no
// exclusion), whose author is not known. No retry can write it.
async function stuckDebt(env) {
  await fileReport(env);
  await quiet(() => sealForumEvidence({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: TEXT, category: 'csam', reviewerId: ADMIN, uploadIps: [{ ip: IP, at: new Date(Date.now() - 3600_000).toISOString() }] }));
  const checks = [{ kind: 'classifier', source: 'jev:forum', category: 'threat', confidence: 0.97, band: 'high' }, { kind: 'classifier', source: 'second-model', category: 'threat', confidence: 0.95, band: 'high' }];
  const content = { named_target: true, place: false, time: false, religious_speech: false, political_speech: false, criticism: false, hyperbole: false, joke: false };
  await env.SCENES.rewriteCase(POST, (r) => {
    r.author_id = null; r.removed = true; r.removed_at = new Date(NOW).toISOString();
    r.report_owed = { since: new Date(NOW).toISOString(), category: 'threat', reason: 'check_removal', by: null, detection: { source: 'forum_check', checks, band: 'high', viewed_by_staff: false }, threat_content: content };
  });
  await env.SOCIAL.put(forumReportOwedKey(POST), new Date(NOW).toISOString());
}

const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const routeDeps = (extra = {}) => ({
  json, requireAdmin: async () => ADMIN,
  auditLog: async (env, action, adminId, target, details) => { await env.AUDIT_LOG.put(`audit:${Date.now()}-${crypto.randomUUID()}`, JSON.stringify({ action, admin_id: adminId, target, details })); },
  ...extra,
});
const route = async (env, method, path, body, deps = routeDeps()) => {
  const url = new URL(`https://api.eustress.dev${path}`);
  const res = await quiet(() => handleLegalReportRoute(new Request(url, { method, headers: { 'Content-Type': 'application/json' }, ...(body ? { body: JSON.stringify(body) } : {}) }), url, env, {}, deps));
  return { status: res.status, body: await res.json() };
};

test('a stuck debt leaves the owed list for its own index, staff are alerted at once and daily with no content, account or address, and it is listed for staff', async () => {
  const env = await environment();
  await stuckDebt(env);
  const out = await quiet(() => writeOwedForumReports(env));
  assert.deepEqual([out.stuck, out.still_owed, out.written], [1, 0, 0]);
  const stuck = await loadStuckReport(env, 'forum', POST);
  assert.deepEqual([stuck.case_kind, stuck.case_id, stuck.reason, stuck.report_kind, stuck.alerts], ['forum', POST, 'no_account', 'threat', 1]);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.report_owed.stuck.reason, 'no_account', 'the debt stays on the case, marked stuck');
  assert.equal(metaOf(env).owed, 'stuck');
  assert.equal(await env.SOCIAL.get(forumReportOwedKey(POST)), null, 'and is off the hourly owed list');
  assert.equal(auditsOf(env, 'LEGAL_REPORT_STUCK').length, 1);
  // The alert, at once: the case only.
  const mails = stuckMails(env);
  assert.ok(mails.length >= 1, 'staff were alerted');
  for (const m of mails) {
    const shown = [m.subject, m.text, m.html].join('\n');
    assert.match(shown, new RegExp(POST));
    for (const secret of [TEXT, 'NUMBAT', AUTHOR, `${AUTHOR}@example.com`, IP, REPORTER]) assert.equal(shown.includes(secret), false, `the alert does not carry ${secret}`);
  }
  // Not retried and not alerted again within the day; alerted again a day later.
  const sent = env.mail.length;
  // The daily re-alert waits for LISTING_LEGAL_OWED_SWEEP: off, a day later still sends nothing.
  const off = await quiet(() => sweepLegalReports(env, {}, { now: Date.now() + STUCK_ALERT_EVERY_MS + 60_000 }));
  assert.equal(off.stuck, 'off');
  assert.equal(env.mail.length, sent, 'no re-alert while the setting is off');
  env.LISTING_LEGAL_OWED_SWEEP = 'on';
  const again = await quiet(() => sweepLegalReports(env, {}, { now: Date.now() }));
  assert.deepEqual([again.stuck.stuck, again.stuck.alerted], [1, 0]);
  assert.equal(env.mail.length, sent);
  const daily = await quiet(() => sweepLegalReports(env, {}, { now: Date.now() + STUCK_ALERT_EVERY_MS + 60_000 }));
  assert.equal(daily.stuck.alerted, 1);
  assert.equal((await loadStuckReport(env, 'forum', POST)).alerts, 2);
  assert.equal((await reportsOf(env)).length, 0);
  // The admin legal queue's stuck view: audited, and no account or content in it.
  const listed = await route(env, 'GET', '/api/admin/legal/stuck');
  assert.equal(listed.status, 200);
  assert.deepEqual(listed.body.items.map((i) => [i.case_kind, i.case_id, i.reason]), [['forum', POST, 'no_account']]);
  assert.equal(JSON.stringify(listed.body).includes(AUTHOR), false);
  assert.equal(auditsOf(env, 'LEGAL_STUCK_LISTED').length, 1);
  // The heal writes a stuck index that is missing, from the case.
  await env.SOCIAL.delete(legalStuckKey('forum', POST));
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  const healed = await quiet(() => sweepLegalReports(env));
  assert.equal(healed.owed_reports.healed, 1);
  assert.equal((await loadStuckReport(env, 'forum', POST)).reason, 'no_account');
});

test('only a person clears a stuck debt, with a rationale audited before anything changes; the evidence stays', async () => {
  const env = await environment();
  await stuckDebt(env);
  await quiet(() => writeOwedForumReports(env));
  const vault = vaultSnapshot(env);
  assert.ok(vault.length > 2, 'the case has sealed evidence');
  const path = `/api/admin/legal/stuck/forum/${POST}/clear`;
  // No rationale: refused.
  assert.equal((await route(env, 'POST', path, {})).status, 400);
  // The audit cannot be written: nothing changes.
  const broken = await route(env, 'POST', path, { note: 'Reviewed: the account was never recorded.' }, routeDeps({ auditLog: async () => { throw new Error('down'); } }));
  assert.deepEqual([broken.status, broken.body.code], [503, 'audit_unavailable']);
  assert.ok(await loadStuckReport(env, 'forum', POST), 'still stuck');
  assert.ok((await loadForumCase(env, POST)).report_owed, 'still owed');
  // Not the system, and not without a rationale, whoever calls it.
  assert.equal((await clearStuckReport(env, { caseKind: 'forum', caseId: POST, by: 'system', rationale: 'The sweep tries to clear this.' })).code, 'person_required');
  assert.equal((await clearStuckReport(env, { caseKind: 'forum', caseId: POST, by: ADMIN, rationale: 'short' })).code, 'rationale_required');
  // The person is checked first, before anything is read, at both layers.
  assert.equal((await clearStuckReport(env, { caseKind: 'forum', caseId: `${THREAD}-9`, by: 'system', rationale: 'The sweep tries to clear this.' })).code, 'person_required');
  assert.equal((await clearReportDebt(env, { caseKind: 'forum', caseId: POST, by: 'system', rationale: 'The sweep tries to clear this.' })).code, 'person_required');
  assert.ok((await loadForumCase(env, POST)).report_owed, 'still owed');
  // No sweep clears it.
  env.FORUM_LEGAL_OWED_SWEEP = 'on';
  await quiet(() => sweepLegalReports(env));
  assert.ok(await loadStuckReport(env, 'forum', POST));
  // A person, with a rationale: audited, then cleared.
  const done = await route(env, 'POST', path, { note: 'Reviewed with counsel: filed by hand under reference X-1.' });
  assert.deepEqual([done.status, done.body.ok], [200, true]);
  assert.equal(await loadStuckReport(env, 'forum', POST), null);
  const rec = await loadForumCase(env, POST);
  assert.equal(rec.report_owed, null);
  assert.deepEqual([rec.report_cleared.by, rec.report_cleared.reason], [ADMIN, 'no_account']);
  assert.match(rec.report_cleared.rationale, /filed by hand/);
  const cleared = auditsOf(env, 'LEGAL_STUCK_CLEARED');
  assert.deepEqual(cleared.map((a) => [a.admin_id, a.details.case_id]), [[ADMIN, POST]]);
  assert.equal(vaultSnapshot(env), vault, 'the sealed evidence is untouched');
  assert.equal((await route(env, 'POST', path, { note: 'A second clear of the same case.' })).status, 404);
});
