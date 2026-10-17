// A REQUIRED or legal-lane staff alert is never lost to one failed send (src/staff_alerts.mjs sendStaffAlert). Before, only a
// result of daily_cap went on the over-cap list for the hourly digest; a send that failed was counted and dropped. The D-10
// alert (a new listing csam debt, src/legal_reports.mjs alertReportOwed) is sent once, at the debt's creation, and with
// LISTING_LEGAL_OWED_SWEEP off nothing tries it again: a mail failure at that moment meant staff were never told the debt
// exists. Adapted from Security's probe zz_dfix_5 (F5). The legal alerts also carry stable alert ids now (the case and the
// debt), so a retried alert has one entry on the list, not one per try.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { fakeKv } from './helpers/load_worker.mjs';
import { makeToolExecutor, loadCase, POLICY_HASH_ANCHORED, listingReportOwedKey } from '../src/moderation.mjs';
import { sweepLegalReports } from '../src/legal_reports.mjs';
import { sendStaffDigest, alertStaffReportOwed, alertStaffStuckReport } from '../src/takedown.mjs';
import { sendStaffAlert } from '../src/staff_alerts.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

const enc = new TextEncoder();
const dec = new TextDecoder();
const AUTHOR = 'author-f5';
const ADMIN = 'admin-f5';
const CAT = 'csam_or_minor_sexualization';
const SIM = 'a1b2c3d4-0000-4000-8000-000000000f05';
const STAFF = 'security@eustress.dev';

function bucket() {
  const objects = new Map();
  const view = (key, o) => ({ key, size: o.bytes.byteLength, etag: o.etag, customMetadata: o.customMetadata || {}, uploaded: new Date(), checksums: {}, text: async () => dec.decode(o.bytes), arrayBuffer: async () => o.bytes.slice().buffer });
  return {
    objects,
    async put(key, value, opts = {}) {
      const bytes = typeof value === 'string' ? enc.encode(value) : value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength));
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
  };
}
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
  const env = { USERS: pagedKv(), SOCIAL: pagedKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), SCENES: bucket(), JWT_SECRET: 'test-secret-f5', JEV_API_KEY: 'jev-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', __EmailMessage: StubMessage, mail: [] };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  for (const id of [AUTHOR, ADMIN]) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return env;
}
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };
function failPuts(env, pattern) {
  const real = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (key, ...rest) => { if (pattern.test(String(key))) throw new Error('KV is unavailable'); return real(key, ...rest); };
  return () => { env.SOCIAL.put = real; };
}
const json = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const toolDeps = () => ({ auditLog: async () => {}, json, cors: {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED });
async function listing(env, { p = 0.95 } = {}) {
  const sim = { id: SIM, name: 'Harbor Town', author_id: AUTHOR, is_public: true, r2_key: `universes/${SIM}/universe.pak`, published_at: '2026-10-14T10:00:00.000Z', updated_at: '2026-10-14T10:00:00.000Z', moderation: { status: 'held' } };
  await env.SCENES.put(sim.r2_key, 'PAK-BYTES');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: AUTHOR, status: 'held', created_at: '2026-10-14T10:00:00.000Z', history: [], jev: { model: 'jev-test', answers: { [CAT]: { p } } }, decision: { reasons: [] } }));
}
const quarantineAs = (env, actor, actorId) => quiet(() => makeToolExecutor(env, toolDeps(), { actor, actorId })('moderation_quarantine', { sim_id: SIM, category: CAT, rationale: 'A rationale that is at least twenty characters long.' }, { actor, actorId }));

function mails(env) {
  return env.mail.filter((m) => m.to === STAFF).map((m) => {
    const line = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
    const b = /^=\?UTF-8\?B\?(.*)\?=$/.exec(line);
    const at = m.raw.indexOf('Content-Type: text/plain; charset=utf-8');
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return { subject: b ? Buffer.from(b[1], 'base64').toString('utf8') : line, text: Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8') };
  });
}
const owedMails = (env) => mails(env).filter((m) => m.subject.startsWith('A legal report is owed'));
const digests = (env) => mails(env).filter((m) => m.subject.startsWith("Staff alerts past the day's allowance"));
const queueKeys = (env) => [...env.USERS.store.keys()].filter((k) => k.startsWith('staffmail-queue:'));

async function quarantineDuringMailOutage(env, { mailDown }) {
  await listing(env);
  const restore = failPuts(env, /^legalreport:/);
  const send = env.EMAIL.send;
  if (mailDown) env.EMAIL.send = async () => { throw new Error('the mail binding is unavailable'); };
  await quarantineAs(env, 'system', 'pipeline');
  env.EMAIL.send = send;
  restore();
  assert.ok((await loadCase(env, SIM)).report_owed, 'the debt is on the case');
  assert.ok(await env.SOCIAL.get(listingReportOwedKey(SIM)), 'and indexed');
}

test('F5: staff are told of a listing csam debt created while the mail binding failed, with the listing setting off', async () => {
  const env = await environment();
  await quarantineDuringMailOutage(env, { mailDown: true });
  assert.equal(owedMails(env).length, 0, 'the owed alert itself was not sent');
  assert.ok(queueKeys(env).length >= 1, 'it waits on the list for the digest');
  // Mail is back. A day of hourly runs with LISTING_LEGAL_OWED_SWEEP off (the default): the legal sweep and the staff digest.
  for (let hour = 0; hour < 24; hour++) { await quiet(() => sweepLegalReports(env)); await quiet(() => sendStaffDigest(env, { now: Date.now() + hour * 3600000 })); }
  const carried = digests(env).filter((m) => m.text.includes('staff_legal_owed'));
  assert.equal(carried.length, 1, 'one digest carries the owed alert');
  assert.match(carried[0].text, /after its send failed/);
  const reference = /staff_legal_owed: (EC-[A-Z0-9-]+),/.exec(carried[0].text)?.[1];
  assert.ok(reference, 'it names the case by its reference');
  assert.ok([...env.SOCIAL.store.values()].some((v) => v.includes(reference) && v.includes(SIM)), 'the reference is the case of this listing');
  assert.equal(queueKeys(env).length, 0, 'and it left the list');
});

test('F5 control: with mail up at the quarantine the owed alert is sent once and nothing waits', async () => {
  const env = await environment();
  await quarantineDuringMailOutage(env, { mailDown: false });
  for (let hour = 0; hour < 3; hour++) { await quiet(() => sweepLegalReports(env)); await quiet(() => sendStaffDigest(env, { now: Date.now() + hour * 3600000 })); }
  assert.equal(owedMails(env).length, 1);
  assert.equal(digests(env).length, 0);
  assert.equal(queueKeys(env).length, 0);
});

test('a failed send waits on the list only for a required or legal-lane alert; a general one that is not required is counted failed', async () => {
  const env = await environment();
  env.EMAIL.send = async () => { throw new Error('the mail binding is unavailable'); };
  const rendered = { subject: 'A staff alert for the test', text: 'A staff alert for the test.', html: '<p>A staff alert for the test.</p>' };
  const general = await quiet(() => sendStaffAlert(env, [STAFF], rendered, { lane: 'general', alertId: 'g-1', kind: 'staff_review', required: false }));
  assert.deepEqual([general.queued, general.failed], [0, 1]);
  const required = await quiet(() => sendStaffAlert(env, [STAFF], rendered, { lane: 'general', alertId: 'r-1', kind: 'staff_new', required: true }));
  assert.deepEqual([required.queued, required.failed, required.delivered], [1, 0, false]);
  const legal = await quiet(() => sendStaffAlert(env, [STAFF], rendered, { lane: 'legal', alertId: 'l-1', kind: 'staff_legal_report', required: false }));
  assert.deepEqual([legal.queued, legal.failed], [1, 0]);
  // No mail binding at all: a required alert still waits on the list.
  const bare = await environment();
  delete bare.EMAIL;
  const none = await quiet(() => sendStaffAlert(bare, [STAFF], rendered, { lane: 'general', alertId: 'n-1', kind: 'staff_new', required: true }));
  assert.equal(none.queued, 1);
  const entry = JSON.parse(bare.USERS.store.get(queueKeys(bare)[0]));
  assert.equal(entry.reason, 'no_mail_binding');
  assert.equal(queueKeys(env).length, 2);
  for (const k of queueKeys(env)) assert.equal(JSON.parse(env.USERS.store.get(k)).reason, 'send_failed');
});

test('a legal alert retried while mail is down has one entry on the list: its alert id is the case and the debt', async () => {
  const env = await environment();
  env.EMAIL.send = async () => { throw new Error('the mail binding is unavailable'); };
  const owed = { reference: 'EC-F5-0001', case_kind: 'moderation', case_id: SIM, category: CAT, reason: 'write_failed', since: '2026-10-16T01:00:00.000Z', admin_url: 'https://x' };
  for (let i = 0; i < 3; i++) await quiet(() => alertStaffReportOwed(env, owed));
  assert.equal(queueKeys(env).length, 2, 'one entry per staff address (the mailbox and the administrator), not one per try');
  // A new debt for the same case (a later `since`) is its own alert.
  await quiet(() => alertStaffReportOwed(env, { ...owed, since: '2026-10-17T01:00:00.000Z' }));
  assert.equal(queueKeys(env).length, 4);
  // A stuck debt's daily alert: one entry per alert number.
  const stuck = { reference: 'EC-F5-0002', case_kind: 'forum', case_id: 'f5f5f5f5f5f5f5f5f5f5f5f5-1', report_kind: 'threat', reason: 'no_account', since: '2026-10-15T00:00:00.000Z', stuck_at: '2026-10-15T01:00:00.000Z', alerts: 2, admin_url: 'https://x' };
  for (let i = 0; i < 3; i++) await quiet(() => alertStaffStuckReport(env, stuck));
  assert.equal(queueKeys(env).length, 6);
  // Once a digest carried it, a retry finds it delivered.
  env.EMAIL.send = async (m) => { env.mail.push(m); };
  await quiet(() => sendStaffDigest(env));
  assert.equal(queueKeys(env).length, 0);
  const again = await quiet(() => alertStaffReportOwed(env, owed));
  assert.equal(again.sent, 0);
  assert.equal(again.digested, 2, 'carried by the digest: not sent twice');
});
