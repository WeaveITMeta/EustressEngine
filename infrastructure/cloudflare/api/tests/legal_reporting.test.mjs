// The legal-reporting side of moderation (McKale, 2026-10-15): illegal content is removed and sealed (src/evidence_vault.mjs),
// a confident finding is written up for the authority the law names (src/legal_reports.mjs), the account is never frozen or
// banned, the author is told the rule and how to appeal (a CSAM notice waits for its report), and every sender is off by
// default. Also the forum hooks Website's forums Worker calls: sealForumEvidence and the pushed forum decision.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  makeToolExecutor, runModerationCase, loadCase, authorView, contentLockedFor, buildJevQuestions, loadForumCase,
  recordForumRemoval, resolveForumCase, fileForumReport, forumPostModeration, sealForumEvidence, registerForumDecisionApplier,
  forumDecisionKey, releaseRemovalNotice, POLICY_HASH_ANCHORED, handleModerationRoute,
} from '../src/moderation.mjs';
import { sealEvidence, readEvidence, retryPendingEvidence, listSealed, VAULT_PREFIX, PENDING_PREFIX, RETAIN_DAYS } from '../src/evidence_vault.mjs';
import {
  writeLegalReport, assessThreat, routeFor, takeoverCheck, reportedAccount, weeklyScamBatch, recordScamRemoval, loadLegalReport,
  sweepLegalReports, legalSettingsFrom, releaseReportNotice, cyberTiplineDraft, NOT_VIEWED, NO_LOGIN_HISTORY, SETTING_NAMES, weekStartOf,
} from '../src/legal_reports.mjs';
import { forumNoticeEntry } from '../src/case_history.mjs';
import { assignCaseReference } from '../src/case_reference.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { questionnaireAnswers } from './helpers/claude_fake.mjs';
import { questionnaireRecord } from '../src/rating_questionnaire.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

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

// R2 with conditional puts and, when `failPrefix` is set, puts under it that throw (an R2 outage for the vault only).
function r2(state = {}) {
  const m = new Map();
  const obj = (key, rec) => ({ key, size: rec.bytes.byteLength, etag: rec.etag, customMetadata: rec.customMetadata || {}, checksums: {}, uploaded: new Date(),
    text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0) });
  return {
    m, state,
    put: async (key, body, opts = {}) => {
      if (state.failPrefix && key.startsWith(state.failPrefix)) throw new Error('R2 is unavailable');
      if (opts.onlyIf?.etagDoesNotMatch === '*' && m.has(key)) return null;
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body) : new Uint8Array(body instanceof ArrayBuffer ? body : body.buffer ? body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength) : body);
      const rec = { bytes, etag: createHash('md5').update(bytes).digest('hex'), customMetadata: opts.customMetadata };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async (key) => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async (key) => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async (key) => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ objects: [...m.keys()].filter((k) => k.startsWith(prefix)).sort().slice(0, limit).map((key) => ({ key })) }),
  };
}

const AUTHOR = 'user-1';
const ADMIN = 'admin-7';
const SIM = 'a1b2c3d4-0000-4000-8000-000000000001';
const THREAD = 'a'.repeat(24);
const POST = `${THREAD}-2`;
const SECRET_TEXT = 'QUOKKA-CANARY-POST-TEXT';
const PACKAGE = 'PAK-BYTES-CANARY-QUOKKA';

function environment(extra = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), KYC_STATUS: kv(), SCENES: r2(), JWT_SECRET: 'test-secret', JEV_API_KEY: 'jev-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', ...extra };
  env.sent = [];
  env.EMAIL = { send: async (msg) => { env.sent.push(msg); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  env.USERS.m.set(`user:${AUTHOR}`, JSON.stringify({ id: AUTHOR, username: 'ann', email: 'ann@example.com', created_at: '2025-01-01T00:00:00.000Z' }));
  env.USERS.m.set(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'mod', email: 'mod@example.com', created_at: '2025-01-01T00:00:00.000Z' }));
  env.USERS.m.set(`admin:${ADMIN}`, '1');
  return env;
}

// The verified identity record, with every field a report must never carry, each holding a canary.
function kycRecord(env, over = {}) {
  env.KYC_STATUS.m.set(`kyc-${AUTHOR}-front`, JSON.stringify({
    status: 'linked', ocr_name: 'Ann Example', extracted_dob: '1990-04-02', jurisdiction: 'US', iso2: 'US',
    r2_key: 'kyc/CANARY-ID-FRONT-IMAGE.bin', back_r2_key: 'kyc/CANARY-ID-BACK-IMAGE.bin', selfie_r2_key: 'kyc/CANARY-SELFIE.bin',
    selfie: 'CANARY-SELFIE-BYTES', document_number: 'CANARY-DOC-NUMBER', hash: 'CANARY-ID-HASH', grok_analysis: { note: 'CANARY-ANALYSIS' },
    ...over,
  }));
}

const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const audits = [];
const deps = (extra = {}) => ({ auditLog: async (e, action, adminId, target, details) => { audits.push({ action, adminId, target, details }); }, json, cors: {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED, ...extra });

// A listing with stored objects and a case whose classifier answered `p` for `category` (the pipeline's triage quarantine).
async function listingWithCase(env, { category = 'csam_or_minor_sexualization', p = 0.95 } = {}) {
  const sim = { id: SIM, name: 'Harbor Town', author_id: AUTHOR, is_public: true, r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-10-14T10:00:00.000Z', updated_at: '2026-10-14T10:00:00.000Z', moderation: { status: 'classifying' } };
  await env.SCENES.put(sim.r2_key, PACKAGE);
  await env.SCENES.put(`thumbnails/${SIM}/thumb.png`, 'THUMB-QUOKKA');
  await env.SCENES.put(`universes/${SIM}/moderation/capture-0.png`, 'CAPTURE-QUOKKA');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({
    sim_id: SIM, author_id: AUTHOR, status: 'classifying', created_at: '2026-10-14T10:00:00.000Z', history: [],
    jev: { model: 'jev-test', answers: { [category]: { p } } }, decision: { reasons: [{ code: category, action: 'quarantine', lane: 'legal' }] },
  }));
  return sim;
}

const quarantine = async (env, category = 'csam_or_minor_sexualization', actor = 'system') =>
  makeToolExecutor(env, deps(), { actor, actorId: actor === 'admin' ? ADMIN : 'pipeline' })('moderation_quarantine', { sim_id: SIM, category, rationale: 'Classifier probability 0.95 at publish; this is the test rationale.' }, { actor });

const reportsOf = async (env) => Promise.all([...env.SOCIAL.m.keys()].filter((k) => k.startsWith('legalreport:')).map((k) => loadLegalReport(env, k.slice('legalreport:'.length))));
const feedOf = (env) => [...env.SOCIAL.m.keys()].filter((k) => k.startsWith(`notif:${AUTHOR}:`)).map((k) => JSON.parse(env.SOCIAL.m.get(k)));
const allStored = (env) => [...env.SOCIAL.m.entries(), ...env.USERS.m.entries(), ...env.AUDIT_LOG.m.entries(), ...[...env.SCENES.m.entries()].map(([k, v]) => [k, new TextDecoder().decode(v.bytes)])].map(([k, v]) => `${k}=${v}`).join('\n');

// ── the vault ───────────────────────────────────────────────────────────────

test('sealing encrypts the content and its facts; a read needs an administrator and legal process, and every access is audited', async () => {
  const env = environment();
  const r = await sealEvidence(env, { caseKind: 'forum', caseId: POST, category: 'csam', subject: { kind: 'forum_post', post_id: POST }, uploader: { account_id: AUTHOR, upload_ip: '203.0.113.9' }, detection: { band: 'high' }, items: [{ id: 'post-text', text: SECRET_TEXT }] });
  assert.equal(r.ok, true);
  assert.match(r.case_ref, /^EC-/);
  const stored = allStored(env);
  assert.doesNotMatch(stored, /QUOKKA-CANARY/, 'the text is nowhere in the clear');
  assert.doesNotMatch(stored, /203\.0\.113\.9/, 'nor the uploader\'s address');
  const index = JSON.parse(new TextDecoder().decode(env.SCENES.m.get(`${VAULT_PREFIX}${r.case_ref}/post-text.json`).bytes));
  assert.deepEqual([index.alg, index.copied, index.never_delete, index.bytes], ['AES-256-GCM', true, true, SECRET_TEXT.length]);
  assert.ok(Date.parse(index.retain_until) - Date.parse(index.sealed_at) >= 365 * 86400 * 1000, `kept at least a year (${RETAIN_DAYS} days)`);
  assert.ok([...env.AUDIT_LOG.m.values()].some((v) => JSON.parse(v).action === 'EVIDENCE_SEALED'));

  const before = env.AUDIT_LOG.m.size;
  assert.equal((await readEvidence(env, { caseRef: r.case_ref, adminId: AUTHOR, legalProcess: { kind: 'subpoena', reference: 'SUB-2026-001' } })).code, 'admin_required');
  assert.equal((await readEvidence(env, { caseRef: r.case_ref, adminId: ADMIN })).code, 'legal_process_required');
  assert.equal((await readEvidence(env, { caseRef: r.case_ref, adminId: ADMIN, legalProcess: { kind: 'a feeling', reference: 'SUB-1' } })).code, 'legal_process_kind');
  assert.equal(env.AUDIT_LOG.m.size, before + 3, 'every refused read is audited');
  const opened = await readEvidence(env, { caseRef: r.case_ref, adminId: ADMIN, legalProcess: { kind: 'subpoena', reference: 'SUB-2026-001' } });
  assert.equal(opened.ok, true);
  assert.equal(new TextDecoder().decode(opened.items[0].content), SECRET_TEXT);
  assert.equal(opened.items[0].facts.uploader.upload_ip, '203.0.113.9');
  const actions = [...env.AUDIT_LOG.m.values()].map((v) => JSON.parse(v));
  const read = actions.find((a) => a.action === 'EVIDENCE_READ');
  assert.deepEqual([read.admin_id, read.details.legal_process.reference], [ADMIN, 'SUB-2026-001'], 'the read and its legal process are on the audit log');
  assert.ok(actions.some((a) => a.action === 'EVIDENCE_READ_DONE'));
  assert.equal((await listSealed(env, { caseRef: r.case_ref, adminId: ADMIN })).items.length, 1);

  // A read whose audit cannot be written does not happen.
  const noAudit = { ...env, AUDIT_LOG: { put: async () => { throw new Error('down'); } } };
  assert.equal((await readEvidence(noAudit, { caseRef: r.case_ref, adminId: ADMIN, legalProcess: { kind: 'subpoena', reference: 'SUB-2026-001' } })).code, 'audit_unavailable');
});

test('a sealed part cannot be moved to another case: the place is bound into the encryption', async () => {
  const env = environment();
  const a = await sealEvidence(env, { caseKind: 'forum', caseId: POST, category: 'csam', items: [{ id: 'post-text', text: 'first' }] });
  const b = await sealEvidence(env, { caseKind: 'forum', caseId: `${THREAD}-3`, category: 'csam', items: [{ id: 'post-text', text: 'second' }] });
  env.SCENES.m.set(`${VAULT_PREFIX}${b.case_ref}/post-text.c0`, env.SCENES.m.get(`${VAULT_PREFIX}${a.case_ref}/post-text.c0`));
  assert.equal((await readEvidence(env, { caseRef: b.case_ref, adminId: ADMIN, legalProcess: { kind: 'court_order', reference: 'ORDER-9' } })).code, 'damaged');
});

test('a seal that fails deletes nothing, alerts staff, and the sweep seals it later', async () => {
  const env = environment();
  await listingWithCase(env);
  env.SCENES.state.failPrefix = VAULT_PREFIX;
  const q = await quarantine(env);
  assert.equal(q.ok, true);
  assert.match(q.summary, /evidence pending/);
  assert.ok(env.SCENES.m.has(`universes/${SIM}/universe.pak`), 'the package is still there');
  assert.equal((await loadCase(env, SIM)).status, 'quarantined', 'and still removed');
  assert.equal(env.SCENES.m.get(`locks/legal/${SIM}`)?.customMetadata?.kind, 'serve_block');
  const pending = [...env.SOCIAL.m.keys()].filter((k) => k.startsWith(PENDING_PREFIX));
  assert.ok(pending.length >= 3, 'every item is pending');
  assert.doesNotMatch([...env.SOCIAL.m.values()].join('\n'), /PAK-BYTES-CANARY/, 'the pending record carries no content');
  assert.ok(env.sent.some((m) => m.to === 'security@eustress.dev' && /Evidence was not sealed/.test(m.raw)), 'staff were told');

  env.SCENES.state.failPrefix = null;
  const swept = await retryPendingEvidence(env);
  assert.equal(swept.still_pending, 0);
  assert.ok(swept.sealed >= 3);
  assert.equal([...env.SOCIAL.m.keys()].filter((k) => k.startsWith(PENDING_PREFIX)).length, 0);
  assert.ok([...env.SCENES.m.keys()].some((k) => k.startsWith(VAULT_PREFIX) && k.endsWith('/package.json')));
  assert.ok(env.SCENES.m.has(`universes/${SIM}/universe.pak`), 'and the package is never deleted');

  // A forum post's text that fails to seal is kept only encrypted, and sealed by the sweep.
  env.SCENES.state.failPrefix = VAULT_PREFIX;
  const text = await sealEvidence(env, { caseKind: 'forum', caseId: POST, category: 'csam', items: [{ id: 'post-text', text: SECRET_TEXT }] });
  assert.deepEqual(text.pending, ['post-text']);
  assert.doesNotMatch(allStored(env), /QUOKKA-CANARY/);
  env.SCENES.state.failPrefix = null;
  assert.equal((await retryPendingEvidence(env)).sealed, 1);
});

// ── no automatic freeze or ban ───────────────────────────────────────────────

for (const [label, category] of [['CSAM', 'csam_or_minor_sexualization'], ['terrorism', 'terrorism_or_extremist_promotion'], ['NCII', 'ncii_or_real_person_sexual']]) {
  test(`a ${label} quarantine leaves the account's access unchanged`, async () => {
    const env = environment();
    kycRecord(env);
    await listingWithCase(env, { category });
    const other = { id: 'b1b2c3d4-0000-4000-8000-000000000002', author_id: AUTHOR, moderation: { status: 'approved' } };
    await env.SOCIAL.put(`sim:${other.id}`, JSON.stringify(other));
    const usersBefore = JSON.stringify([...env.USERS.m.entries()].filter(([k]) => !k.startsWith('mailcap')).sort());
    assert.equal((await quarantine(env, category)).ok, true);
    assert.equal((await loadCase(env, SIM)).status, 'quarantined');
    assert.equal(JSON.stringify([...env.USERS.m.entries()].filter(([k]) => !k.startsWith('mailcap')).sort()), usersBefore, 'the account record and its keys are unchanged');
    assert.equal(JSON.parse(env.USERS.m.get(`user:${AUTHOR}`)).banned, undefined, 'never banned');
    assert.equal(await contentLockedFor(env, other, AUTHOR), false, 'the author\'s other listing is not locked');
    assert.equal(await contentLockedFor(env, (await loadCase(env, SIM)) && { id: SIM, author_id: AUTHOR, moderation: { status: 'quarantined' } }, AUTHOR), true, 'the removed one is');
  });
}

test('a forum removal leaves the account\'s access unchanged', async () => {
  const env = environment();
  const usersBefore = JSON.stringify([...env.USERS.m.entries()].sort());
  const r = await recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: SECRET_TEXT, check: { verdict: 'refuse', category: 'nonconsensual_sexual', confidence: 0.95, confidence_band: 'high' } });
  assert.equal(r.ok, true);
  const decided = await resolveForumCase({ env, postId: `${THREAD}-1`, decision: 'remove', category: 'scam', reviewerId: ADMIN });
  assert.equal(decided.code, 'no_case');
  assert.equal(JSON.stringify([...env.USERS.m.entries()].filter(([k]) => !k.startsWith('mailcap')).sort()), usersBefore);
  assert.equal((await forumPostModeration({ env, postId: POST })).removed, true, 'the post is removed');
});

// ── the report ───────────────────────────────────────────────────────────────

test('a confident CSAM quarantine writes one minimised report, queued ready_to_send, and staff are alerted', async () => {
  const env = environment();
  kycRecord(env);
  await listingWithCase(env);
  await quarantine(env);
  const [rep, ...more] = await reportsOf(env);
  assert.equal(more.length, 0);
  assert.deepEqual([rep.kind, rep.status, rep.route.authority, rep.what.article, rep.what.viewed_by_staff, rep.what.statement], ['csam', 'ready_to_send', 'NCMEC', 'A1', false, NOT_VIEWED]);
  assert.equal(rep.what.detection.checks[0].confidence, 0.95);
  assert.equal(rep.what.detection.two_agree, false, 'one check');
  assert.deepEqual([rep.who.account_id, rep.who.verified_name, rep.who.date_of_birth, rep.who.country, rep.who.email], [AUTHOR, 'Ann Example', '1990-04-02', 'US', 'ann@example.com']);
  assert.deepEqual(rep.who.upload_ips, []);
  // Said per sealed item: no address is known for any of the listing's items.
  assert.equal(rep.who.upload_ip_note, undefined);
  assert.deepEqual(rep.who.upload_ip_unknown_items, rep.evidence.items.map((item_id) => ({ item_id, upload_ip: null, reason: 'not_known' })));
  assert.ok(rep.evidence.items.length >= 1);
  assert.equal(rep.when_where.case_reference, await assignCaseReference(env, 'moderation', SIM), 'the case reference');
  assert.equal(rep.evidence.sealed, true);
  const text = JSON.stringify(rep) + JSON.stringify(cyberTiplineDraft(rep));
  for (const canary of ['CANARY-ID-FRONT-IMAGE', 'CANARY-ID-BACK-IMAGE', 'CANARY-SELFIE', 'CANARY-DOC-NUMBER', 'CANARY-ID-HASH', 'CANARY-ANALYSIS', 'kyc/']) assert.doesNotMatch(text, new RegExp(canary), `the report never carries ${canary}`);
  assert.ok(env.sent.some((m) => m.to === 'security@eustress.dev' && /A legal report is waiting to be sent/.test(m.raw)), 'staff were alerted');
  assert.equal(env.sent.filter((m) => m.to === 'ann@example.com').length, 0, 'the author was not told');
  // A second finding on the case is the same report.
  assert.equal((await writeLegalReport(env, { kind: 'csam', caseKind: 'moderation', caseId: SIM, detection: { band: 'high', checks: [] }, uploader: { account_id: AUTHOR } })).reason, 'already_reported');
});

test('the minimised identity is built from an allowlist: nothing else on the identity record reaches it', async () => {
  const env = environment();
  kycRecord(env, { status: 'rejected' });
  const unverified = await reportedAccount(env, AUTHOR);
  assert.deepEqual([unverified.identity_verified, unverified.verified_name, unverified.date_of_birth], [false, null, null]);
  kycRecord(env);
  const who = await reportedAccount(env, AUTHOR);
  assert.deepEqual(Object.keys(who).sort(), ['account_id', 'country', 'date_of_birth', 'email', 'identity_documents', 'identity_verified', 'verified_name']);
  assert.doesNotMatch(JSON.stringify(who), /CANARY/);
});

test('a borderline removal is removed and sealed, and never reported', async () => {
  const env = environment();
  await listingWithCase(env, { p: 0.6 });
  const q = await quarantine(env);
  assert.match(q.summary, /evidence sealed; report borderline_never_reported/);
  assert.equal((await reportsOf(env)).length, 0);
  assert.equal((await loadCase(env, SIM)).legal_report.reason, 'borderline_never_reported');
  const forum = await recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: SECRET_TEXT, check: { verdict: 'refuse', category: 'csam', confidence: 0.6, confidence_band: 'borderline' } });
  assert.deepEqual([forum.removed, forum.sealed, forum.report], [true, true, 'borderline_never_reported']);
  assert.equal((await reportsOf(env)).length, 0);
  assert.equal((await writeLegalReport(env, { kind: 'csam', caseKind: 'forum', caseId: `${THREAD}-9`, detection: { band: 'borderline', checks: [] }, uploader: { account_id: AUTHOR } })).reason, 'borderline_never_reported');
});

test('routing by the account\'s verified country', () => {
  assert.equal(routeFor('csam', 'US').authority, 'NCMEC');
  assert.equal(routeFor('csam', 'DE').authority, 'NCMEC', 'Eustress is a US provider: CSAM goes to NCMEC wherever the account is');
  assert.equal(routeFor('csam', null).authority, 'NCMEC');
  assert.deepEqual([routeFor('threat', 'de').authority, routeFor('threat', 'DE').member_state], ['MEMBER_STATE:DE', 'DE']);
  assert.match(routeFor('threat', 'FR').basis, /Article 18/);
  assert.equal(routeFor('threat', null).authority, 'EUROPOL');
  assert.equal(routeFor('threat', 'zz?').authority, 'EUROPOL', 'not a country: unknown');
  assert.equal(routeFor('threat', 'US').authority, null, 'US disclosure is voluntary and not decided');
  assert.equal(routeFor('threat', 'NO').authority, null, 'an EEA state that is not a member is not routed');
  assert.equal(routeFor('scam', 'GB').authority, 'FTC_IC3');
  assert.equal(routeFor('ncii', 'US'), null, 'NCII of adults is never routed to an authority');
});

test('the takeover check flags a credential change shortly before the upload, and says the Worker keeps no sign-in history', async () => {
  const env = environment();
  const upload = '2026-10-14T10:00:00.000Z';
  const state = (over) => ({ accountState: async () => ({ key_changed_at: null, retired: [], email_verified_at: null, passkeys: [], ...over }) });
  const recent = await takeoverCheck(env, AUTHOR, upload, state({ key_changed_at: '2026-10-14T08:00:00.000Z' }));
  assert.equal(recent.flag, true);
  assert.deepEqual(recent.credential_changes.map((c) => c.what), ['identity_key_changed']);
  assert.equal((await takeoverCheck(env, AUTHOR, upload, state({ passkeys: [{ created_at: '2026-10-13T12:00:00.000Z' }] }))).flag, true, 'a passkey added the day before');
  const old = await takeoverCheck(env, AUTHOR, upload, state({ key_changed_at: '2026-09-01T08:00:00.000Z' }));
  assert.equal(old.flag, false);
  assert.equal((await takeoverCheck(env, AUTHOR, upload, state({ key_changed_at: '2026-10-15T08:00:00.000Z' }))).flag, false, 'a change after the upload is not before it');
  const none = await takeoverCheck(env, AUTHOR, upload, { accountState: async () => null });
  assert.deepEqual([none.flag, none.account_state_read, none.new_device, none.new_ip, none.login_history_kept, none.note], [false, false, null, null, false, NO_LOGIN_HISTORY]);
  assert.equal(none.new_account, false);
  env.USERS.m.set(`user:${AUTHOR}`, JSON.stringify({ id: AUTHOR, created_at: '2026-10-14T01:00:00.000Z' }));
  assert.equal((await takeoverCheck(env, AUTHOR, upload, {})).new_account, true);
});

test('every sender is off by default: reports stay ready_to_send, and an NCMEC setting without a registration sends nothing', async () => {
  assert.deepEqual(Object.values(legalSettingsFrom({})).filter((v) => v === true), [], 'every setting is off');
  assert.deepEqual(Object.entries(legalSettingsFrom(Object.fromEntries(SETTING_NAMES.map((n) => [n, 'true'])))).filter(([k, v]) => v === true && k !== 'ncmec_registered'), [], 'only the exact "on" turns one on');
  for (const settings of [{}, { LEGAL_REPORT_NCMEC: 'on' }, { LEGAL_REPORT_NCMEC: 'on', NCMEC_REGISTRATION: 'registered-id' }]) {
    const env = environment(settings);
    kycRecord(env);
    await listingWithCase(env);
    await quarantine(env);
    const [rep] = await reportsOf(env);
    assert.equal(rep.status, 'ready_to_send', JSON.stringify(settings));
    assert.match(rep.waiting_reason, settings.NCMEC_REGISTRATION ? /not_implemented/ : settings.LEGAL_REPORT_NCMEC ? /not registered/ : /LEGAL_REPORT_NCMEC is off/);
    const swept = await sweepLegalReports(env);
    assert.deepEqual([swept.sent, (await loadLegalReport(env, rep.id)).status], [0, 'ready_to_send']);
    assert.equal(env.sent.filter((m) => m.to === 'security@eustress.dev' && /A legal report/.test(m.raw)).length, 1, 'staff are alerted once, not every hour');
  }
});

// ── threats: built, off ──────────────────────────────────────────────────────

const CLEAN = { religious_speech: false, political_speech: false, criticism: false, hyperbole: false, joke: false };
const two = [{ kind: 'classifier', source: 'jev:forum', category: 'threat', confidence: 0.95 }, { kind: 'classifier', source: 'claude:forum', category: 'threat', confidence: 0.9 }];

test('a threat is reportable only when specific and two independent checks agree at high confidence', () => {
  assert.equal(assessThreat({ checks: two, content: { ...CLEAN, place: true, time: true } }).reportable, true);
  assert.deepEqual(assessThreat({ checks: two, content: { ...CLEAN } }).reasons, ['not_specific']);
  assert.deepEqual(assessThreat({ checks: [two[0]], content: { ...CLEAN, named_target: true } }).reasons, ['two_independent_checks_must_agree']);
  assert.equal(assessThreat({ checks: [two[0], { ...two[0], confidence: 0.99 }], content: { ...CLEAN, time: true } }).two_agree, false, 'one source twice is one check');
  assert.equal(assessThreat({ checks: [two[0], { ...two[1], confidence: 0.8 }], content: { ...CLEAN, time: true } }).reportable, false, 'below high confidence');
  // Report counts never count: only content signals.
  assert.equal(assessThreat({ checks: [two[0], { kind: 'reports', source: 'member_reports', category: 'threat', confidence: 1 }], content: { ...CLEAN, time: true } }).reportable, false);
  for (const speech of ['hyperbole', 'joke', 'political_speech', 'religious_speech', 'criticism']) {
    const r = assessThreat({ checks: two, content: { ...CLEAN, named_target: true, place: true, time: true, [speech]: true } });
    assert.deepEqual([r.reportable, r.excluded], [false, [speech]], speech);
  }
  const unanswered = assessThreat({ checks: two, content: { named_target: true } });
  assert.deepEqual([unanswered.reportable, unanswered.reasons], [false, ['exclusion_not_answered']], 'an exclusion no check answered stops a report');
});

test('the threat decision is identical whatever the user\'s religion, politics, nationality or citizenship, and reads none of them', () => {
  const reads = [];
  const person = (attrs) => new Proxy(attrs, { get: (t, k) => { reads.push(k); return t[k]; } });
  const cases = [{ checks: two, content: { ...CLEAN, place: true } }, { checks: two, content: { ...CLEAN, hyperbole: true, place: true } }, { checks: [two[0]], content: { ...CLEAN, time: true } }];
  for (const base of cases) {
    const decisions = [
      { religion: 'Christian', politics: 'conservative', nationality: 'US', citizenship: 'US' },
      { religion: 'Muslim', politics: 'socialist', nationality: 'PK', citizenship: 'none' },
      { religion: 'none', politics: 'anarchist', nationality: 'DE', citizenship: 'DE' },
      { religion: 'Jewish', politics: 'liberal', nationality: 'IL', citizenship: 'IL' },
    ].map((attrs) => JSON.stringify(assessThreat({ ...base, author: person(attrs), user: person(attrs), account: person(attrs), ...attrs })));
    assert.equal(new Set(decisions).size, 1, JSON.stringify(decisions));
  }
  assert.deepEqual(reads, [], 'no user attribute was read');
});

test('a threat report is held while LEGAL_REPORT_THREATS is off, and goes to no one', async () => {
  const env = environment({ LEGAL_REPORT_EU_EMAIL: 'on', LEGAL_REPORT_EU_AUTHORITIES: JSON.stringify({ DE: 'authority@example.de' }) });
  kycRecord(env, { jurisdiction: 'DE', iso2: 'DE' });
  const r = await recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: SECRET_TEXT, check: { verdict: 'refuse', category: 'threat', confidence: 0.95, confidence_band: 'high' }, secondCheck: { source: 'claude:forum', category: 'threat', confidence: 0.92 }, threatContent: { ...CLEAN, place: true, time: true } });
  assert.equal(r.report, 'held_setting_off');
  const [rep] = await reportsOf(env);
  assert.deepEqual([rep.kind, rep.status, rep.route.authority], ['threat', 'held_setting_off', 'MEMBER_STATE:DE']);
  assert.equal(env.sent.filter((m) => m.to === 'authority@example.de').length, 0, 'no authority is emailed');
  await sweepLegalReports(env);
  assert.equal((await loadLegalReport(env, rep.id)).status, 'held_setting_off');
  assert.equal(env.sent.filter((m) => m.to === 'authority@example.de').length, 0);
  // Not specific: removed, not reported at all.
  const vague = await recordForumRemoval({ env, postId: `${THREAD}-3`, threadId: THREAD, authorId: AUTHOR, text: 'x', check: { verdict: 'refuse', category: 'threat', confidence: 0.95, confidence_band: 'high' }, secondCheck: { source: 'claude:forum', category: 'threat', confidence: 0.92 }, threatContent: { ...CLEAN } });
  assert.equal(vague.report, 'threat_criteria_not_met');
});

// ── the weekly scam batch ────────────────────────────────────────────────────

test('the confirmed scam removals of last week become one batched report, queued for FTC and IC3, built once', async () => {
  const env = environment();
  const monday = Date.parse('2026-10-12T05:00:00.000Z');
  assert.equal(weekStartOf(monday), '2026-10-12');
  for (const [id, at] of [['p1', '2026-10-05T01:00:00.000Z'], ['p2', '2026-10-11T23:59:00.000Z'], ['p3', '2026-10-04T23:00:00.000Z'], ['p4', '2026-10-12T00:30:00.000Z']]) {
    await recordScamRemoval(env, { caseKind: 'forum', caseId: id, subject: { kind: 'forum_post', post_id: id }, account_id: 'scammer', removed_at: at, confirmed_by: 'person' });
  }
  const built = await weeklyScamBatch(env, monday);
  assert.deepEqual([built.built, built.week_start, built.count], [true, '2026-10-05', 2]);
  const rep = await loadLegalReport(env, built.id);
  assert.deepEqual([rep.kind, rep.status, rep.route.authority, rep.items.map((i) => i.case_id)], ['scam_batch', 'ready_to_send', 'FTC_IC3', ['p1', 'p2']]);
  assert.match(rep.waiting_reason, /LEGAL_REPORT_SCAMS is off/);
  const again = await weeklyScamBatch(env, monday + 3600 * 1000);
  assert.deepEqual([again.built, again.id], [false, built.id], 'the next hour does not build it again');
  // A person's scam removal on the forum, and a confident check's, join the batch.
  await sealForumEvidence({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: 'free coins', category: 'scam', reviewerId: ADMIN });
  await resolveForumCase({ env, postId: POST, decision: 'remove', category: 'scam', reviewerId: ADMIN });
  await recordForumRemoval({ env, postId: `${THREAD}-4`, threadId: THREAD, authorId: AUTHOR, text: 'send btc', check: { verdict: 'refuse', category: 'scam', confidence: 0.9, confidence_band: 'high' } });
  assert.equal([...env.SOCIAL.m.keys()].filter((k) => k.startsWith('scamremoval:') && /forum:a{24}-[24]$/.test(k)).length, 2);
});

// ── notices and appeals ──────────────────────────────────────────────────────

test('a CSAM removal\'s notice is withheld until its report is sent; then the author gets the rule and the appeal', async () => {
  const env = environment();
  kycRecord(env);
  await listingWithCase(env);
  await quarantine(env);
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  const held = authorView(await loadCase(env, SIM), sim);
  assert.deepEqual([held.status, held.reasons, held.can_appeal], ['in_review', [], false]);
  assert.equal(feedOf(env).length, 0);
  assert.equal(env.sent.filter((m) => m.to === 'ann@example.com').length, 0);
  await sweepLegalReports(env);
  assert.equal(authorView(await loadCase(env, SIM), sim).status, 'in_review', 'a sweep with the sender off releases nothing');

  // The report is sent (the sender's success is what calls this).
  const [rep] = await reportsOf(env);
  assert.equal((await releaseReportNotice(env, rep, { by: 'system', reason: 'the report was sent' })).ok, true);
  const told = authorView(await loadCase(env, SIM), sim);
  assert.deepEqual([told.status, told.can_appeal, told.reasons[0].title], ['removed', true, 'Sexual content involving a minor (Code A1)']);
  assert.match(told.headline, /account and your other listings are not affected/);
  assert.deepEqual(feedOf(env).map((f) => f.status), ['removed']);
  assert.ok(env.sent.some((m) => m.to === 'ann@example.com' && /was removed from Eustress/.test(m.raw)));
  assert.equal((await loadLegalReport(env, rep.id)).notice.withheld, false);
});

test('the release setting releases a withheld CSAM notice without the report being sent', async () => {
  const env = environment({ LEGAL_REPORT_CSAM_NOTICE_RELEASE: 'on' });
  kycRecord(env);
  await listingWithCase(env);
  await quarantine(env);
  assert.equal((await sweepLegalReports(env)).notices_released, 1);
  assert.equal(authorView(await loadCase(env, SIM), JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`))).status, 'removed');
});

test('a non-CSAM removal is told at once with its Code article, and is appealed through the existing appeal path', async () => {
  const env = environment();
  await listingWithCase(env, { category: 'terrorism_or_extremist_promotion' });
  await quarantine(env, 'terrorism_or_extremist_promotion');
  const rec = await loadCase(env, SIM);
  const view = authorView(rec, JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)));
  assert.deepEqual([view.status, view.can_appeal, view.reasons[0].title], ['removed', true, 'Terrorism or violent extremism (Code A2)']);
  assert.deepEqual(feedOf(env).map((f) => [f.status, f.can_appeal]), [['removed', true]]);
  const route = (method, path, body) => handleModerationRoute(new Request(`https://api.eustress.dev${path}`, { method, headers: { Authorization: `Bearer ${AUTHOR}` }, ...(body ? { body: JSON.stringify(body) } : {}) }), new URL(`https://api.eustress.dev${path}`), env, {}, { ...deps(), verifyAuth: async (r) => (r.headers.get('Authorization') || '').replace('Bearer ', ''), requireAdmin: async () => null });
  const appeal = await route('POST', `/api/simulations/${SIM}/appeal`, { text: 'This is a history lesson about the event, not promotion.' });
  assert.equal(appeal.status, 202, await appeal.clone().text());
  const after = await loadCase(env, SIM);
  assert.deepEqual([after.status, after.appeal.status, !!after.legal_hold], ['appealed', 'pending', true], 'the removal stays while a person decides');
  assert.equal(JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)).moderation.status, 'quarantined');
  assert.equal(authorView(after, {}).status, 'appeal_in_review');
  const upheld = await makeToolExecutor(env, deps(), { actor: 'admin', actorId: ADMIN })('moderation_resolve_appeal', { sim_id: SIM, decision: 'upheld', rationale: 'Reviewed: the Space recruits for the group it depicts.' });
  assert.equal(upheld.ok, true);
  assert.equal(authorView(await loadCase(env, SIM), {}).status, 'removed');
});

test('the pipeline quarantines a confident CSAM finding, seals it and writes its report, and the author is not told', async () => {
  const env = environment();
  kycRecord(env);
  const sim = { id: SIM, name: 'Harbor Town', description: 'A harbor.\nCategory: Simulation\nObjective: Read the tide.\nLesson: Tides.\nReal-world link: Tide tables.', author_id: AUTHOR, is_public: true, r2_key: `universes/${SIM}/universe.pak`, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()) };
  const stored = await env.SCENES.put(sim.r2_key, PACKAGE);
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  const answers = {};
  for (const [k, def] of Object.entries(buildJevQuestions())) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: k === 'csam_or_minor_sexualization' ? 0.95 : 0.02 };
    else if (def.type === 'choice') { const first = Object.keys(def.criteria)[0]; answers[k] = { type: 'choice', choice: first, probabilities: { [first]: 0.9 }, confidence: 0.9 }; }
    else answers[k] = { type: 'score', score: 0, probabilities: { [def.criteria[0]]: 0.9 }, legend: Object.fromEntries(def.criteria.map((l, i) => [l, i])), confidence: 0.9 };
  }
  const fetch = async () => new Response(JSON.stringify({ model: 'jev-test', answers, usage: { input_tokens: 1, output_tokens: 0 } }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  const rec = await runModerationCase(SIM, env, deps({ fetch }), { trigger: 'publish' });
  assert.equal(rec.status, 'quarantined');
  assert.equal(rec.evidence_seal.ok, true);
  assert.deepEqual([rec.legal_report.written, rec.legal_report.status, rec.legal_report.band], [true, 'ready_to_send', 'high']);
  assert.equal(env.USERS.m.has(`publish-frozen:${AUTHOR}`), false);
  assert.equal(feedOf(env).length, 0);
  assert.doesNotMatch([...env.SCENES.m.entries()].filter(([k]) => k.startsWith(VAULT_PREFIX)).map(([, v]) => new TextDecoder().decode(v.bytes)).join(''), /PAK-BYTES-CANARY/);
});

test('a forum CSAM removal by the check is reported, and its author is told only the generic entry, before and after the release', async () => {
  const env = environment();
  kycRecord(env);
  const r = await recordForumRemoval({ env, postId: POST, threadId: THREAD, authorId: AUTHOR, text: SECRET_TEXT, check: { verdict: 'refuse', category: 'csam', confidence: 0.97, confidence_band: 'high' }, uploadIp: '198.51.100.4', postedAt: '2026-10-14T09:00:00.000Z' });
  assert.deepEqual([r.removed, r.sealed, r.report], [true, true, 'ready_to_send']);
  const [rep] = await reportsOf(env);
  // The legacy single address has no time of its own: it is never paired with the post's creation time.
  assert.deepEqual(rep.who.upload_ips, [{ ip: '198.51.100.4', at: null, item_id: null }]);
  // Refused at posting: the generic entry at once (McKale), naming no category or article, so it reads as a
  // nonconsensual_sexual refusal does (tests/forum_case_store.test.mjs compares the two).
  const before = forumNoticeEntry(await loadForumCase(env, POST));
  assert.equal(before.outcome, 'removed');
  // The random case reference, the post id and the times are left out of the check: a reference is random base32 and can spell
  // a forbidden word by chance (as tests/forum_case_store.test.mjs does).
  assert.doesNotMatch(JSON.stringify({ ...before, reference: null, post: null, opened_at: null, decided_at: null, updated_at: null }), /A1|A5|minor|sexual|csam/i);
  assert.match(before.feedback.reasons[0].why, /^Your account is not affected/);
  assert.doesNotMatch([...env.SOCIAL.m.values()].join('\n'), /QUOKKA-CANARY/, 'the body is in no case record');
  await releaseReportNotice(env, rep, { by: 'system', reason: 'the report was sent' });
  const entry = forumNoticeEntry(await loadForumCase(env, POST));
  assert.deepEqual(entry, before, 'the release changes nothing the author reads');
  // A non-CSAM removal is told at once.
  await recordForumRemoval({ env, postId: `${THREAD}-3`, threadId: THREAD, authorId: AUTHOR, text: 'doxx', check: { verdict: 'refuse', category: 'personal_info', confidence: 0.9, confidence_band: 'high' } });
  assert.match(forumNoticeEntry(await loadForumCase(env, `${THREAD}-3`)).feedback.reasons[0].why, /private information/);
});

test('a person releases a withheld listing notice that has no report', async () => {
  const env = environment();
  await listingWithCase(env, { p: 0.6 });
  await quarantine(env);
  assert.equal(authorView(await loadCase(env, SIM), {}).status, 'in_review');
  assert.equal((await releaseRemovalNotice(env, SIM, { by: ADMIN, reason: 'released by a person after the report decision' })).ok, true);
  assert.equal(authorView(await loadCase(env, SIM), {}).status, 'removed');
  assert.equal((await releaseRemovalNotice(env, SIM, { by: ADMIN })).code, 'not_withheld');
});

// ── the forums Worker's hooks ────────────────────────────────────────────────

test('sealForumEvidence makes the case and seals the post before a removal, with no report, for an administrator only', async () => {
  const env = environment();
  const args = { env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: SECRET_TEXT, category: 'nonconsensual_sexual', reviewerId: ADMIN };
  assert.equal((await sealForumEvidence({ ...args, reviewerId: AUTHOR })).code, 'bad_reviewer');
  assert.equal((await sealForumEvidence({ ...args, category: 'rudeness' })).code, 'bad_category');
  assert.equal((await sealForumEvidence({ ...args, threadId: 'b'.repeat(24) })).code, 'bad_thread');
  assert.equal(await loadForumCase(env, POST), null, 'a refusal makes no case');
  const r = await sealForumEvidence(args);
  assert.equal(r.ok, true);
  assert.match(r.reference, /^EC-/);
  const rec = await loadForumCase(env, POST);
  assert.deepEqual([rec.reference, rec.reporters.length, rec.evidence_seal.ok], [r.reference, 0, true]);
  assert.doesNotMatch(allStored(env), /QUOKKA-CANARY/, 'the body is stored nowhere outside the encrypted vault');
  const opened = await readEvidence(env, { caseRef: r.reference, adminId: ADMIN, legalProcess: { kind: 'subpoena', reference: 'SUB-7' } });
  assert.equal(new TextDecoder().decode(opened.items[0].content), SECRET_TEXT);
  assert.equal((await sealForumEvidence(args)).reference, r.reference, 'a second seal keeps the case and its reference');
  // The removal now finds its case without any report.
  assert.equal((await resolveForumCase({ env, postId: POST, decision: 'remove', category: 'nonconsensual_sexual', reviewerId: ADMIN })).ok, true);
  assert.equal((await forumPostModeration({ env, postId: POST })).removed, true);
});

test('a registered applier is called once per decision with the decision, and the durable record is written', async () => {
  const env = environment();
  await sealForumEvidence({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: 'scam text', category: 'scam', reviewerId: ADMIN });
  const calls = [];
  registerForumDecisionApplier(async (a) => { calls.push(a); });
  try {
    const r = await resolveForumCase({ env, postId: POST, decision: 'remove', category: 'scam', reviewerId: ADMIN });
    assert.equal(calls.length, 1);
    assert.deepEqual([calls[0].env, calls[0].postId, calls[0].action, calls[0].reference], [env, POST, 'remove', r.reference]);
    assert.ok(Date.parse(calls[0].decidedAt));
    assert.deepEqual(JSON.parse(env.SOCIAL.m.get(forumDecisionKey(POST))), { action: 'remove', decided_at: calls[0].decidedAt, reference: r.reference });
    assert.equal((await loadForumCase(env, POST)).decision_push.status, 'decision_pushed');
    await makeToolExecutor(env, deps(), { actor: 'admin', actorId: ADMIN })('moderation_forum_resolve', { post_id: POST, decision: 'restore', rationale: 'Reviewed: an honest giveaway by the developer.' });
    assert.deepEqual(calls.map((c) => c.action), ['remove', 'restore'], 'the admin tool goes through it too');
    assert.equal(JSON.parse(env.SOCIAL.m.get(forumDecisionKey(POST))).action, 'restore');
    assert.equal((await forumPostModeration({ env, postId: POST })).removed, false, 'serving reflects it');
  } finally {
    registerForumDecisionApplier(null);
  }
});

test('with no applier the decision is recorded and the key written; an applier that throws never undoes the decision', async () => {
  const env = environment();
  await sealForumEvidence({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, postBody: 'scam text', category: 'scam', reviewerId: ADMIN });
  assert.equal((await resolveForumCase({ env, postId: POST, decision: 'remove', category: 'scam', reviewerId: ADMIN })).ok, true);
  let rec = await loadForumCase(env, POST);
  assert.deepEqual([rec.removed, rec.decision_push.status], [true, 'decision_push_skipped']);
  assert.ok(rec.history.some((h) => h.event === 'decision_push_skipped'), 'the history says so');
  assert.equal(JSON.parse(env.SOCIAL.m.get(forumDecisionKey(POST))).action, 'remove');
  for (const failing of [async () => { throw new Error('forum down'); }, () => Promise.reject(new TypeError('nope'))]) {
    registerForumDecisionApplier(failing);
    try {
      assert.equal((await resolveForumCase({ env, postId: POST, decision: 'gate', reviewerId: ADMIN })).ok, true);
      rec = await loadForumCase(env, POST);
      assert.deepEqual([rec.gated, rec.removed, rec.decision_push.status], [true, true, 'push_failed']);
      assert.ok(rec.history.some((h) => h.event === 'push_failed'));
      assert.equal(JSON.parse(env.SOCIAL.m.get(forumDecisionKey(POST))).action, 'gate');
      assert.equal((await forumPostModeration({ env, postId: POST })).gated, true);
    } finally {
      registerForumDecisionApplier(null);
    }
    await resolveForumCase({ env, postId: POST, decision: 'ungate', reviewerId: ADMIN });
  }
});

test('a report filed by members still works beside the new hooks: a csam report hides and seals, and reports nothing', async () => {
  const env = environment();
  env.USERS.m.set('user:rep-1', JSON.stringify({ id: 'rep-1', created_at: '2025-01-01T00:00:00.000Z' }));
  const r = await fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: 'rep-1', category: 'csam', postBody: SECRET_TEXT });
  assert.equal(r.hide, true);
  assert.equal((await loadForumCase(env, POST)).evidence_seal.ok, true);
  assert.equal((await reportsOf(env)).length, 0, 'reports are not a confident finding');
  assert.doesNotMatch([...env.SOCIAL.m.values()].join('\n'), /QUOKKA-CANARY/);
});
