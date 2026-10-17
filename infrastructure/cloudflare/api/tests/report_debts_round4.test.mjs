// Security's fourth review of the report debts (d12c73fb): D4-2 to D4-5 and the notes, each adapted from its probe (zz_d4_2
// to 5; D4-1 and N-2 are the staff alert builder's) with a mutation that fails it. A refused case write never skips a csam
// debt or its alert; the repair of a written report only re-queues it, never sends it, and never pays with another case's
// report; a report is written only with its confirmation key; a 'known' amendment makes its takeover check and route again;
// the lease outlasts the judge; the stuck re-alert cap is per kind; the confirmation is bounded.
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
import { thresholdsFrom, DEFAULT_THRESHOLDS, LEASE_MARGIN_MS } from '../src/moderation.mjs';
import { reportDebtOutcome, STUCK_ALERTS_PER_RUN, STUCK_ALERTS_PER_KIND_PER_RUN, UNCONFIRMED_PER_RUN } from '../src/legal_reports.mjs';

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

const AUTHORITY = 'reports@europol.example.org';
const DAY = 86400000;
const plainEnv = (extra = {}) => {
  const env = { USERS: fakeKv(), SOCIAL: fakeKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), JWT_SECRET: 's', __EmailMessage: StubMessage, mail: [], ...extra };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  return env;
};
const threatFinding = {
  kind: 'threat', caseKind: 'forum', caseId: POST, category: 'threat', article: 'A3', subject: { kind: 'forum_post', post_id: POST, thread_id: THREAD },
  detection: { source: 'forum_check', band: 'high', checks: [{ kind: 'classifier', source: 'jev:forum', category: 'threat', confidence: 0.95, band: 'high' }, { kind: 'classifier', source: 'claude:forum', category: 'threat', confidence: 0.92, band: 'high' }] },
  content: { named_target: false, place: true, time: true, religious_speech: false, political_speech: false, criticism: false, hyperbole: false, joke: false },
  uploader: { account_id: AUTHOR, uploaded_at: '2026-10-15T10:00:00.000Z', upload_ips: [] },
};
const csamFinding = (sim, author) => ({
  kind: 'csam', caseKind: 'moderation', caseId: sim, category: 'csam', article: 'A1', subject: { kind: 'listing', sim_id: sim },
  detection: { source: 'person', checks: [{ kind: 'person', source: 'moderation_quarantine', category: 'csam', confidence: null, band: 'high' }], band: 'high', viewed_by_staff: false },
  uploader: { account_id: author, uploaded_at: new Date(Date.now() - 300_000).toISOString(), upload_ips: [] },
});
const newUser = (id) => JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: new Date(Date.now() - 600_000).toISOString() });

// -- D4-5 ----------------------------------------------------------------------------------------------------------------

test('D4-5 (zz_d4_5): a quarantine whose case write is refused after a lease takeover still records its csam debt and tells staff', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.get = async (key) => {
    if (armed && key === `sim:${SIM}`) { armed = false; await env.SCENES.put(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'another-run', sim_id: SIM, expires_at: Date.now() + 3600_000 })); }
    return get(key);
  };
  await quarantineAs(env, 'admin', ADMIN);
  env.SOCIAL.get = get;
  restore();
  assert.ok(!armed, 'the lease was taken during the try');
  const rec = await loadCase(env, SIM);
  assert.equal(rec.report_owed?.actor, 'admin', 'the debt is on the case');
  assert.equal(subjects(env).filter((s) => s.startsWith('A legal report is owed')).length, 1, 'and staff were told');
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'and it is indexed');
  // The other holder lets go; a later try writes it.
  await env.SCENES.delete(`locks/moderation/${SIM}.json`);
  await quiet(() => writeOwedListingReports(env));
  assert.equal((await reportsOf(env)).filter((r) => r.kind === 'csam').length, 1);
});

test('D4-5: the debt recorded after a refused write never weakens a debt of another finding the case holds by then', async () => {
  const env = await environment();
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  const scenesPut = env.SCENES.put.bind(env.SCENES);
  let armed = true;
  const theirs = '2026-10-15T00:00:00.000Z';
  // While the seal is being written (after the try read the case): another run takes the lease and records its own confident
  // debt (another finding, as strong) on the case.
  env.SCENES.put = async (key, ...rest) => {
    if (armed && String(key).startsWith('evidence/')) {
      armed = false;
      await scenesPut(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'another-run', sim_id: SIM, expires_at: Date.now() + 3600_000 }));
      const c = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
      c.report_owed = { since: theirs, category: CAT, actor: 'system', reason: 'unavailable', detection: { source: 'pipeline', checks: [{ kind: 'classifier', source: 'jev:x', category: CAT, confidence: 0.95, band: 'high' }], band: 'high', viewed_by_staff: false } };
      await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(c));
    }
    return scenesPut(key, ...rest);
  };
  await quarantineAs(env, 'system', 'pipeline');
  env.SCENES.put = scenesPut;
  restore();
  assert.ok(!armed, 'the other run wrote during the try');
  assert.equal((await loadCase(env, SIM)).report_owed?.since, theirs, 'the debt on the case stands');
});

// -- D4-2, N-4 -----------------------------------------------------------------------------------------------------------

test('D4-2 (zz_d4_2): a second finding while the sweep sends the report never sends it twice nor leaves a ghost queue key', async () => {
  const env = plainEnv({ LEGAL_REPORT_THREATS: 'on', LEGAL_REPORT_EU_AUTHORITIES: JSON.stringify({ EUROPOL: AUTHORITY }) });
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ id: AUTHOR, username: AUTHOR, email: `${AUTHOR}@example.com`, created_at: '2025-01-01T00:00:00.000Z' }));
  const w = await quiet(() => writeLegalReport(env, threatFinding));
  assert.equal((await loadLegalReport(env, w.id)).status, 'ready_to_send');
  env.LEGAL_REPORT_EU_EMAIL = 'on';
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.get = async (key) => {
    if (armed && key === `legalq:ready_to_send:${w.id}`) { armed = false; await quiet(() => sweepLegalReports(env)); }
    return get(key);
  };
  const second = await quiet(() => writeLegalReport(env, threatFinding));
  env.SOCIAL.get = get;
  assert.equal(second.reason, 'already_reported');
  assert.equal(env.mail.filter((m) => m.to === AUTHORITY).length, 1, 'once');
  const rep = await loadLegalReport(env, w.id);
  assert.equal(rep.status, 'sent');
  assert.equal(env.SOCIAL.store.has(`legalq:ready_to_send:${w.id}`), false, 'no ghost key');
  await quiet(() => sweepLegalReports(env));
  assert.equal(env.mail.filter((m) => m.to === AUTHORITY).length, 1, 'and not again');
});

test('D4-2: the repair of a report never queued only puts its queue key back: it neither saves nor dispatches the report', async () => {
  const env = plainEnv({ SCENES: bucket() });
  await env.USERS.put(`user:${AUTHOR}`, newUser(AUTHOR));
  const put = env.SOCIAL.put.bind(env.SOCIAL);
  let armed = true;
  env.SOCIAL.put = async (key, ...rest) => { if (armed && String(key).startsWith('legalq:ready_to_send:')) { armed = false; throw new Error('KV is unavailable'); } return put(key, ...rest); };
  const first = await quiet(() => writeLegalReport(env, csamFinding(simId(46), AUTHOR)));
  env.SOCIAL.put = put;
  assert.equal(first.reason, 'unavailable');
  const [saved] = await reportsOf(env);
  const before = env.SOCIAL.store.get(`legalreport:${saved.id}`);
  const mails = env.mail.length;
  const again = await quiet(() => writeLegalReport(env, csamFinding(simId(46), AUTHOR)));
  assert.deepEqual([again.reason, again.repaired], ['already_reported', true]);
  assert.equal(env.SOCIAL.store.get(`legalreport:${saved.id}`), before, 'the report record is not written by the repair');
  assert.equal(env.mail.length, mails, 'and nothing is sent or alerted from it');
  assert.ok(env.SOCIAL.store.has(`legalq:ready_to_send:${saved.id}`), 'its queue key is back, for the sweep');
});

test('N-4: a case index that names another case\'s report pays nothing, and the debt is stuck for a person', async () => {
  const env = plainEnv();
  await env.USERS.put(`user:${AUTHOR}`, newUser(AUTHOR));
  const other = await quiet(() => writeLegalReport(env, csamFinding(simId(41), AUTHOR)));
  await env.SOCIAL.put(`legalcase:csam:moderation:${simId(42)}`, other.id);
  const r = await quiet(() => writeLegalReport(env, csamFinding(simId(42), AUTHOR)));
  assert.deepEqual([r.written, r.reason], [false, 'claim_mismatch']);
  assert.equal(reportDebtOutcome(r), 'stuck');
});

// -- D4-3 ----------------------------------------------------------------------------------------------------------------

for (const variant of ['a: the account stays absent', 'b: the account is found on the next read']) {
  test(`D4-3 (zz_d4_3 ${variant}): a report whose confirmation key cannot be written is not written, and the next try is confirmed`, async () => {
    const author = variant.startsWith('a') ? 'author-d4-3-gone' : 'author-d4-3-new';
    const env = plainEnv();
    const put = env.SOCIAL.put.bind(env.SOCIAL);
    let armed = true;
    env.SOCIAL.put = async (key, ...rest) => { if (armed && String(key).startsWith('legalunconfirmed:')) { armed = false; throw new Error('KV is unavailable'); } return put(key, ...rest); };
    const first = await quiet(() => writeLegalReport(env, csamFinding(simId(43), author)));
    env.SOCIAL.put = put;
    assert.deepEqual([first.written, first.reason], [false, 'unavailable'], 'not written without its confirmation key: owed');
    assert.equal((await reportsOf(env)).length, 0);
    const w = await quiet(() => writeLegalReport(env, csamFinding(simId(43), author)));
    assert.equal(w.written, true);
    if (variant.startsWith('b')) await env.USERS.put(`user:${author}`, newUser(author));
    for (let h = 1; h <= 3; h++) await quiet(() => sweepLegalReports(env, {}, { now: Date.now() + h * 3600_000 }));
    assert.equal((await loadLegalReport(env, w.id)).who.account_status, variant.startsWith('a') ? 'deleted' : 'known');
    assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('legalunconfirmed:')).length, 0, 'the key goes once a read shows it confirmed');
  });
}

test('D4-3: the confirmation key stays after the amending run, and goes on the run that reads the report back confirmed', async () => {
  const env = plainEnv();
  const w = await quiet(() => writeLegalReport(env, csamFinding(simId(44), 'author-d4-3-stays')));
  const key = `legalunconfirmed:${w.id}`;
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  assert.equal((await loadLegalReport(env, w.id)).who.account_status, 'deleted');
  assert.ok(await env.SOCIAL.get(key), 'kept until read back');
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 62_000 }));
  assert.equal(await env.SOCIAL.get(key), null);
});

// -- D4-4 ----------------------------------------------------------------------------------------------------------------

test('D4-4 (zz_d4_4): an account found by the confirmation has its takeover check and route made again', async () => {
  const env = plainEnv();
  const w = await quiet(() => writeLegalReport(env, csamFinding(simId(45), 'author-d4-4')));
  assert.equal((await loadLegalReport(env, w.id)).takeover.account_state_read, false);
  await env.USERS.put('user:author-d4-4', newUser('author-d4-4'));
  await env.KYC_STATUS.put('kyc-author-d4-4-front', JSON.stringify({ status: 'verified', ocr_name: 'A Person', jurisdiction: 'DE' }));
  await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  const rep = await loadLegalReport(env, w.id);
  assert.equal(rep.who.account_status, 'known');
  assert.deepEqual([rep.takeover.account_age_hours, rep.takeover.new_account], [0, true], 'the just-made account is a new account');
  assert.equal(rep.route.country, 'DE', 'the route has the account\'s country');
});

// -- notes ---------------------------------------------------------------------------------------------------------------

test('N-1: lease_seconds must outlast judge_timeout_ms with a margin, or the defaults stand', async () => {
  const t = (x) => quiet(() => thresholdsFrom({ MODERATION_THRESHOLDS: JSON.stringify(x) }));
  assert.equal((await t({ lease_seconds: 30 })).lease_seconds, DEFAULT_THRESHOLDS.lease_seconds, 'shorter than the judge');
  assert.equal((await t({ lease_seconds: 300 })).lease_seconds, DEFAULT_THRESHOLDS.lease_seconds, 'no margin');
  assert.equal((await t({ lease_seconds: 420 })).lease_seconds, 420);
  assert.deepEqual([(await t({ lease_seconds: 120, judge_timeout_ms: 30_000 })).lease_seconds], [120], 'a short lease for a short judge');
  const long = await t({ judge_timeout_ms: 600_000 });
  assert.deepEqual([long.lease_seconds, long.judge_timeout_ms], [DEFAULT_THRESHOLDS.lease_seconds, DEFAULT_THRESHOLDS.judge_timeout_ms], 'a judge the lease cannot cover keeps its default');
  assert.ok(LEASE_MARGIN_MS >= 30_000);
});

test('N-3: the stuck re-alert cap is per case kind, so one kind\'s backlog never starves the other', async () => {
  const env = await environment();
  const old = new Date(Date.now() - 2 * DAY).toISOString();
  for (let n = 1; n <= STUCK_ALERTS_PER_RUN; n++) await env.SOCIAL.put(legalStuckKey('forum', `${THREAD}-${n}`, 'csam'), JSON.stringify({ case_kind: 'forum', case_id: `${THREAD}-${n}`, report_kind: 'csam', reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  await env.SOCIAL.put(legalStuckKey('moderation', SIM, 'csam'), JSON.stringify({ case_kind: 'moderation', case_id: SIM, report_kind: 'csam', reason: 'no_detection_recorded', stuck_at: old, alerted_at: old, alerts: 1 }));
  const out = await quiet(() => alertStuckReports(env));
  assert.equal(out.alerted, STUCK_ALERTS_PER_KIND_PER_RUN + 1);
  assert.equal((await loadStuckReport(env, 'moderation', SIM, 'csam')).alerts, 2, 'the listing debt behind a forum backlog is alerted');
});

test('N-5: the confirmation reads UNCONFIRMED_PER_RUN reports a run, and goes on from its cursor', async () => {
  const env = plainEnv();
  env.SOCIAL = (() => {
    const kv = fakeKv();
    kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
      const names = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
      const page = names.slice(0, limit);
      return { keys: page.map((name) => ({ name })), list_complete: names.length <= limit, ...(names.length > limit ? { cursor: page.at(-1) } : {}) };
    };
    return kv;
  })();
  assert.equal(UNCONFIRMED_PER_RUN, 25);
  for (let i = 0; i < 30; i++) await quiet(() => writeLegalReport(env, csamFinding(simId(500 + i), `author-n5-${i}`)));
  const first = await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  assert.equal(first.confirmed, 25);
  const second = await quiet(() => confirmUnconfirmedAccounts(env, { now: Date.now() + 61_000 }));
  assert.equal(second.confirmed, 5);
});
