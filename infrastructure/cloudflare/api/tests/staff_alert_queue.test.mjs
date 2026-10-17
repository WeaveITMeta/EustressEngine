// A staff alert past its lane's allowance waits on the over-cap queue for the hourly digest (src/staff_alerts.mjs). When the
// digest cannot be sent (a mail outage), the queue must not lapse in silence: once any entry has waited 24 hours, staff get one
// staff_failure alert a day on the legal lane, with counts, ages and kinds only, and the entries are kept 30 days, well past
// that. GET /api/admin/legal/staff-alerts shows the same state, audited before it is answered (fail-closed).
// Security's note on commit 65a9db55; and on 62193467: the queue is read from the list alone (each key's metadata), and the
// digests and the overdue check run after the case steps, the digests capped, so a filing flood cannot spend the cron run's
// storage budget on them.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, mintToken, ctx } from './helpers/load_worker.mjs';
import { createHash } from 'node:crypto';
import { alertStaffQueueOverdue, sweepTakedowns, renderMail } from '../src/takedown.mjs';
import { takeMailSlot, STAFF_ALERTS_PER_DAY } from '../src/mail_limits.mjs';
import {
  staffQueueState, sendStaffAlert, sendStaffDigests, alertOverdueStaffQueue, STAFF_OVER_CAP_TTL_SECONDS, STAFF_QUEUE_OVERDUE_MS,
  STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS, STAFF_QUEUE_OVERDUE_ALERT_KEY, STAFF_DIGEST_MAX_ENTRIES, STAFF_QUEUE_LEGACY_READS,
  staffDigestPlan, STAFF_DIGEST_RUN_OPS,
} from '../src/staff_alerts.mjs';

const worker = await loadWorker();
const STAFF = 'security@example.com';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const ADMIN_EMAIL = 'staff.person@example.com';
const NOWHERE = 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee';
const HOUR = 3600000;
const OVERDUE = /^Staff alerts undelivered for /;
const DIGEST = /^Staff alerts waiting: /;

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };

function readMail(m) {
  const part = (type) => {
    const at = m.raw.indexOf(`Content-Type: ${type}; charset=utf-8`);
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
  };
  const subjectLine = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
  const encoded = /^=\?UTF-8\?B\?(.*)\?=$/.exec(subjectLine);
  return { to: m.to, subject: encoded ? Buffer.from(encoded[1], 'base64').toString('utf8') : subjectLine, text: part('text/plain'), raw: m.raw };
}

/// A KV namespace in memory that keeps what the real one does and the shared fake does not: each key's metadata (at most
/// 1024 bytes) and expiration, both returned by list, a list paged 1000 keys at a time with a cursor, and every operation
/// counted (`ops`).
function metaKv() {
  const store = new Map();
  const meta = new Map();
  const exp = new Map();
  const ops = [];
  return {
    store, meta, exp, ops,
    async get(key) { ops.push(['get', key]); return store.has(key) ? store.get(key) : null; },
    async put(key, value, opts = {}) {
      ops.push(['put', key]);
      store.set(key, typeof value === 'string' ? value : String(value));
      if (opts.metadata !== undefined) {
        const json = JSON.stringify(opts.metadata);
        if (Buffer.byteLength(json) > 1024) throw new Error('KV metadata is over 1024 bytes');
        meta.set(key, JSON.parse(json));
      } else meta.delete(key);
      if (opts.expirationTtl) exp.set(key, Math.floor(Date.now() / 1000) + opts.expirationTtl); else exp.delete(key);
    },
    async delete(key) { ops.push(['delete', key]); store.delete(key); meta.delete(key); exp.delete(key); },
    async list({ prefix = '', limit = 1000, cursor } = {}) {
      ops.push(['list', prefix]);
      const all = [...store.keys()].filter((k) => k.startsWith(prefix)).sort();
      const start = cursor ? Number(cursor) : 0;
      const page = all.slice(start, start + Math.min(limit, 1000));
      const end = start + page.length;
      return {
        keys: page.map((name) => ({ name, ...(exp.has(name) ? { expiration: exp.get(name) } : {}), ...(meta.has(name) ? { metadata: meta.get(name) } : {}) })),
        list_complete: end >= all.length, ...(end < all.length ? { cursor: String(end) } : {}),
      };
    },
  };
}

function environment() {
  const env = { ...fakeEnv(), USERS: metaKv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: STAFF, mail: [], refuse: null };
  env.EMAIL = { send: async (m) => { if (env.refuse && env.refuse(readMail(m))) throw new Error('the mail binding failed'); env.mail.push(m); } };
  return env;
}
const staffMail = (env) => env.mail.filter((m) => m.to === STAFF).map(readMail);
const overdueMail = (env) => staffMail(env).filter((m) => OVERDUE.test(m.subject));

const FILER_NAME = 'Filer Quillon Marsh';
const FILER_EMAIL = 'quillon.marsh@example.org';
const STATEMENT = 'A private photo of me shared without my consent.';

async function file(env, i) {
  const body = {
    requester_name: `${FILER_NAME} ${i}`, signature: `${FILER_NAME} ${i}`, relationship: 'self', contact_email: FILER_EMAIL,
    locations: [`https://eustress.dev/simulation/${NOWHERE}`], description: STATEMENT, good_faith: true,
  };
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/takedown/requests', {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': `10.9.${i}.9` }, body: JSON.stringify(body),
  }), env, ctx);
  const json = await res.json();
  assert.equal(res.status, 201, JSON.stringify(json));
  return json.reference;
}

async function runCron(env) {
  const waits = [];
  await worker.scheduled({ scheduledTime: Date.now() }, env, { waitUntil: (p) => waits.push(p), passThroughOnException() {} });
  await Promise.all(waits);
}

const queueKeys = (env) => [...env.USERS.store.keys()].filter((k) => k.startsWith('staffmail-queue:'));

/// Moves every queued entry's time back by `ms`, as if it had waited that long: its value and its metadata.
function age(env, ms) {
  for (const k of queueKeys(env)) {
    const e = JSON.parse(env.USERS.store.get(k));
    const at = new Date(Date.parse(e.queued_at) - ms).toISOString();
    env.USERS.store.set(k, JSON.stringify({ ...e, queued_at: at }));
    assert.ok(env.USERS.meta.has(k), 'a queue entry is written with its metadata');
    env.USERS.meta.set(k, { ...env.USERS.meta.get(k), queued_at: at });
  }
}

/// The general lane of the staff address spent for the day, then `n` filings: their staff alerts go on the over-cap queue.
async function queued(env, n) {
  for (let i = 0; i < STAFF_ALERTS_PER_DAY; i++) assert.equal(await takeMailSlot(env, STAFF, 'staff'), true);
  const refs = [];
  for (let i = 0; i < n; i++) refs.push(await quiet(() => file(env, i + 1)));
  assert.equal(queueKeys(env).length, n, 'each alert waits on the queue');
  return refs;
}

test('the overdue alert comes well before any entry can expire', () => {
  assert.equal(STAFF_QUEUE_OVERDUE_MS, 24 * HOUR);
  assert.equal(STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS, 24 * HOUR);
  assert.equal(STAFF_OVER_CAP_TTL_SECONDS, 30 * 86400, 'entries are kept 30 days');
  assert.ok(STAFF_OVER_CAP_TTL_SECONDS * 1000 >= STAFF_QUEUE_OVERDUE_MS + 7 * STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS, 'a week of daily alerts at least before an entry can lapse');
});

test('a fresh queue, or an empty one, sends no overdue alert', async () => {
  const empty = environment();
  const none = await quiet(() => alertStaffQueueOverdue(empty));
  assert.equal(none.overdue, 0);
  assert.equal(none.alerted, false);
  assert.equal(overdueMail(empty).length, 0);

  const env = environment();
  await queued(env, 2);
  age(env, 23 * HOUR);
  const r = await quiet(() => alertStaffQueueOverdue(env));
  assert.equal(r.overdue, 0, 'an entry 23 hours old is not overdue');
  assert.equal(r.alerted, false);
  assert.equal(overdueMail(env).length, 0);
  assert.equal(env.USERS.store.has(STAFF_QUEUE_OVERDUE_ALERT_KEY), false);
});

test('an entry undelivered past 24 hours sends exactly one staff_failure alert a day on the legal lane, with no case, filer or address in it', async () => {
  const env = environment();
  const refs = await queued(env, 3);
  age(env, 25 * HOUR);
  const now = Date.now();
  const legal = () => [...env.USERS.store.entries()].filter(([k]) => k.startsWith('mailcap-flood:staff_legal:')).reduce((n, [, v]) => n + Number(v), 0);
  const priority = () => [...env.USERS.store.keys()].filter((k) => k.startsWith('mailcap:acct-staff_priority:')).length;
  const legalBefore = legal();
  const first = await quiet(() => alertStaffQueueOverdue(env, { now }));
  assert.equal(first.overdue, 3);
  assert.equal(first.alerted, true);
  assert.equal(legal() - legalBefore, 1, 'counted on the legal lane');
  assert.equal(priority(), 0, 'not on the priority lane');
  let mails = overdueMail(env);
  assert.equal(mails.length, 1, 'one overdue alert');
  // Sent although the general lane is spent for the day: it is on the legal lane, which no allowance stops.
  assert.match(mails[0].subject, /^Staff alerts undelivered for 25 hours: 3 past 24 hours$/);
  assert.match(mails[0].text, /Waiting: 3/);
  assert.match(mails[0].text, /Required: 3/);
  assert.match(mails[0].text, /Kinds past 24 hours: staff_new 3/);
  const secrets = [...refs, FILER_NAME, FILER_EMAIL, STATEMENT, NOWHERE, ...queueKeys(env).map((k) => k.split(':')[1]), ...queueKeys(env).map((k) => k.split(':')[2])];
  for (const s of secrets) assert.equal(mails[0].raw.includes(s) || mails[0].text.includes(s), false, `the overdue alert carries ${s}`);

  // The same day: none again, in the next hours, while the condition stands.
  for (const later of [1, 6, 23]) {
    const r = await quiet(() => alertStaffQueueOverdue(env, { now: now + later * HOUR }));
    assert.equal(r.skipped, 'alerted_today');
  }
  assert.equal(overdueMail(env).length, 1, 'at most one a day');
  // A day on, the queue still undelivered: the next one.
  const next = await quiet(() => alertStaffQueueOverdue(env, { now: now + 24 * HOUR }));
  assert.equal(next.alerted, true);
  mails = overdueMail(env);
  assert.equal(mails.length, 2);
  assert.match(mails[1].subject, /^Staff alerts undelivered for 49 hours/);
});

test('an overdue alert that cannot be sent is not marked, and the next hour tries again', async () => {
  const env = environment();
  await queued(env, 1);
  age(env, 30 * HOUR);
  env.refuse = (m) => OVERDUE.test(m.subject);
  const now = Date.now();
  const r = await quiet(() => alertStaffQueueOverdue(env, { now }));
  assert.equal(r.overdue, 1);
  assert.equal(r.alerted, false);
  assert.equal(env.USERS.store.has(STAFF_QUEUE_OVERDUE_ALERT_KEY), false, 'not sent: not marked');
  env.refuse = null;
  const again = await quiet(() => alertStaffQueueOverdue(env, { now: now + HOUR }));
  assert.equal(again.alerted, true);
  assert.equal(overdueMail(env).length, 1);
});

test('the hourly sweep: digests that fail for a day bring one overdue alert, and a digest that sends clears the condition', async () => {
  const env = environment();
  await queued(env, 2);
  env.refuse = (m) => DIGEST.test(m.subject);
  await quiet(() => runCron(env));
  assert.equal(overdueMail(env).length, 0, 'not yet 24 hours');
  age(env, 25 * HOUR);
  await quiet(() => runCron(env));
  await quiet(() => runCron(env));
  assert.equal(overdueMail(env).length, 1, 'one overdue alert from the sweep, not one an hour');
  assert.equal(staffMail(env).filter((m) => DIGEST.test(m.subject)).length, 0);
  // The mail comes back: the digest carries the entries, and nothing is overdue.
  env.refuse = null;
  await quiet(() => runCron(env));
  assert.equal(staffMail(env).filter((m) => DIGEST.test(m.subject)).length, 1);
  assert.equal(queueKeys(env).length, 0);
  assert.equal((await staffQueueState(env)).count, 0);
});

// --- the admin read ---

async function seededAdmin() {
  const env = environment();
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'staff', email: ADMIN_EMAIL }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  const refs = await queued(env, 2);
  age(env, 26 * HOUR);
  return { env, refs };
}

async function read(env) {
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/admin/legal/staff-alerts', { headers: { Authorization: `Bearer ${mintToken(ADMIN)}` } }), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: res.status, headers: res.headers, text, json };
}

const audits = (env) => [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v));

test('the admin read of the queue is refused when its audit cannot be written, and the queue is not read', async () => {
  const { env, refs } = await seededAdmin();
  const log = fakeKv();
  log.put = async () => { throw new Error('the audit log is down'); };
  const down = { ...env, AUDIT_LOG: log };
  const listed = [];
  const list = env.USERS.list.bind(env.USERS);
  down.USERS = { ...env.USERS, list: async (o) => { listed.push(o?.prefix); return list(o); } };
  const r = await quiet(() => read(down));
  assert.equal(r.status, 503, r.text);
  assert.deepEqual(r.json, { error: 'audit_unavailable', code: 'audit_unavailable' });
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
  assert.equal(listed.includes('staffmail-queue:'), false, 'the queue was not listed');
  for (const s of refs) assert.equal(r.text.includes(s), false);
});

test('the admin read answers the count, the oldest age and each kind, with one audit record and no case in it', async () => {
  const { env, refs } = await seededAdmin();
  const before = audits(env).length;
  const r = await read(env);
  assert.equal(r.status, 200, r.text);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
  const q = r.json.queue;
  assert.equal(q.count, 2);
  assert.equal(q.overdue, 2);
  assert.equal(q.required, 2);
  assert.deepEqual(q.by_kind, { staff_new: 2 });
  assert.deepEqual(q.overdue_by_kind, { staff_new: 2 });
  assert.ok(q.oldest_age_seconds >= 26 * 3600 && q.oldest_age_seconds < 27 * 3600, String(q.oldest_age_seconds));
  assert.ok(Number.isFinite(Date.parse(q.oldest_queued_at)));
  assert.equal(q.at_least, false);
  for (const s of [...refs, FILER_NAME, FILER_EMAIL, STAFF, ADMIN_EMAIL]) assert.equal(r.text.includes(s), false, `the answer carries ${s}`);
  const written = audits(env).slice(before);
  assert.equal(written.length, 1, 'one audit record for one read');
  assert.equal(written[0].action, 'STAFF_ALERT_QUEUE_READ');
  assert.equal(written[0].admin_id, ADMIN);
  assert.equal(written[0].target, 'staffmail-queue');
  assert.equal(written[0].details.route, 'GET /api/admin/legal/staff-alerts');
  // Not an administrator: refused.
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/admin/legal/staff-alerts', { headers: { Authorization: `Bearer ${mintToken('44444444-4444-4444-8444-444444444444')}` } }), env, ctx);
  assert.equal(res.status, 403);
});

// --- the storage budget (Security on 62193467) ---

const tag = (text) => createHash('sha256').update(String(text)).digest('hex').slice(0, 32);
const RENDERED = { subject: 'A staff alert for the test', text: 'A staff alert for the test.', html: '<p>A staff alert for the test.</p>' };
const queueGets = (env) => env.USERS.ops.filter(([op, k]) => op === 'get' && k.startsWith('staffmail-queue:')).length;

test('a queue of 3000 entries is read from the list alone: no read per entry, one list per 1000', async () => {
  const env = environment();
  // Three through the real write path (the general lane spent), the rest seeded as that path writes them.
  for (let i = 0; i < STAFF_ALERTS_PER_DAY; i++) await takeMailSlot(env, STAFF, 'staff');
  for (let i = 0; i < 3; i++) await quiet(() => sendStaffAlert(env, [STAFF], RENDERED, { alertId: `real-${i}`, kind: 'staff_new', reference: `EC-REAL-${i}`, required: true }));
  assert.equal(queueKeys(env).length, 3);
  for (const k of queueKeys(env)) assert.deepEqual(Object.keys(env.USERS.meta.get(k)).sort(), ['kind', 'lane', 'queued_at', 'reason', 'reference', 'required']);
  const addr = tag(STAFF);
  const old = new Date(Date.now() - 30 * HOUR).toISOString();
  const fresh = new Date().toISOString();
  for (let i = 0; i < 2997; i++) {
    const e = { kind: i % 3 ? 'staff_new' : 'staff_review', lane: 'general', reference: `EC-SEED-${i}`, queued_at: i < 10 ? old : fresh, required: i % 2 === 0 };
    await env.USERS.put(`staffmail-queue:${addr}:${tag(`seed-${i}`)}`, JSON.stringify({ alert_id: `seed-${i}`, ...e, digested_at: null }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS, metadata: e });
  }
  env.USERS.ops.length = 0;
  const s = await staffQueueState(env);
  assert.equal(env.USERS.ops.filter(([op]) => op === 'get').length, 0, 'no read of any entry');
  assert.equal(env.USERS.ops.filter(([op]) => op === 'list').length, 3, 'three lists of 1000');
  assert.equal(env.USERS.ops.length, 3);
  assert.equal(s.count, 3000);
  assert.equal(s.overdue, 10);
  assert.deepEqual(s.by_kind, { staff_new: 3 + 1998, staff_review: 999 });
  assert.equal(s.required, 3 + 1499);
  assert.equal(s.legacy_reads, 0);
  assert.equal(s.unknown_age, 0);
  assert.equal(s.at_least, false);
  // The overdue check over it: its once-a-day mark, the lists and the alert's own mail, never an entry.
  env.USERS.ops.length = 0;
  const r = await quiet(() => alertOverdueStaffQueue(env, [STAFF], () => RENDERED));
  assert.equal(r.alerted, true);
  assert.equal(queueGets(env), 0);
  assert.ok(env.USERS.ops.length <= 12, `the check took ${env.USERS.ops.length} operations`);
});

test('entries from before metadata: aged by their 7-day expiration with no read, and at most 50 read when they have neither', async () => {
  const env = environment();
  const addr = tag(STAFF);
  const queuedAt = Date.now() - 25 * HOUR;
  for (let i = 0; i < 10; i++) {
    const key = `staffmail-queue:${addr}:${tag(`ttl-${i}`)}`;
    env.USERS.store.set(key, JSON.stringify({ kind: 'staff_new', queued_at: new Date(queuedAt).toISOString(), required: true }));
    env.USERS.exp.set(key, Math.floor(queuedAt / 1000) + 7 * 86400);
  }
  for (let i = 0; i < 80; i++) env.USERS.store.set(`staffmail-queue:${addr}:${tag(`bare-${i}`)}`, JSON.stringify({ kind: 'staff_review', queued_at: new Date(queuedAt).toISOString(), required: true }));
  env.USERS.ops.length = 0;
  const s = await staffQueueState(env);
  assert.equal(s.count, 90);
  assert.equal(queueGets(env), STAFF_QUEUE_LEGACY_READS, 'at most 50 entries read');
  assert.equal(s.legacy_reads, STAFF_QUEUE_LEGACY_READS);
  assert.equal(s.unknown_age, 80 - STAFF_QUEUE_LEGACY_READS);
  assert.equal(s.overdue, 10 + STAFF_QUEUE_LEGACY_READS, 'the expiration says 25 hours');
  assert.ok(s.oldest_age_seconds >= 25 * 3600 - 2 && s.oldest_age_seconds <= 25 * 3600 + 2, String(s.oldest_age_seconds));
});

test('one run of digests names at most STAFF_DIGEST_MAX_ENTRIES alerts, every address together, under 200 storage operations', async () => {
  const env = environment();
  const addresses = [STAFF, 'second.staff@example.com', 'third.staff@example.com'];
  for (const a of addresses) for (let i = 0; i < STAFF_ALERTS_PER_DAY; i++) await takeMailSlot(env, a, 'staff');
  for (let i = 0; i < 120; i++) await quiet(() => sendStaffAlert(env, addresses, RENDERED, { alertId: `cap-${i}`, kind: 'staff_new', reference: `EC-CAP-${i}`, required: true }));
  assert.equal(queueKeys(env).length, 360);
  const render = (d) => renderMail('staff_digest', { ...d, at: new Date().toISOString() });
  const now = Date.now();
  env.USERS.ops.length = 0;
  const r = await quiet(() => sendStaffDigests(env, addresses, render, { now }));
  assert.equal(r.digests, 3);
  assert.equal(r.named, STAFF_DIGEST_MAX_ENTRIES, 'the whole budget, shared: 17, 17 and 16');
  assert.ok(r.named <= STAFF_DIGEST_MAX_ENTRIES);
  assert.ok(env.USERS.ops.length <= 200, `the digests took ${env.USERS.ops.length} operations`);
  assert.equal(queueGets(env), 0, 'named from the metadata, no read per entry');
  assert.equal(queueKeys(env).length, 360 - r.named);
  const first = staffMail(env).filter((m) => DIGEST.test(m.subject));
  assert.equal(first.length, 1);
  assert.ok(first[0].subject.includes('waiting: 120 '), first[0].subject);
  assert.match(first[0].text, /(103|104) more are waiting/);
  // One address alone gets the whole share; the rest are named in the hours after.
  env.USERS.ops.length = 0;
  const solo = await quiet(() => sendStaffDigests(env, [STAFF], render, { now: now + HOUR }));
  assert.equal(solo.named, STAFF_DIGEST_MAX_ENTRIES);
  assert.ok(env.USERS.ops.length <= 200, `the digest took ${env.USERS.ops.length} operations`);
});

test('the sweep runs the TAKE IT DOWN case steps first, then the digests, then the overdue check', async () => {
  const env = environment();
  // A case that owes its staff step: the staff mail failed at filing.
  env.refuse = (m) => m.subject.startsWith('TAKE IT DOWN request received');
  await quiet(() => file(env, 50));
  env.refuse = null;
  // An alert past the allowance waiting 25 hours, and a digest that cannot be sent.
  for (let i = 0; i < STAFF_ALERTS_PER_DAY; i++) await takeMailSlot(env, STAFF, 'staff');
  await quiet(() => file(env, 51));
  assert.equal(queueKeys(env).length, 2, 'the failed send and the one past the allowance');
  age(env, 25 * HOUR);
  env.refuse = (m) => DIGEST.test(m.subject);
  // One log, in order, of the case steps' storage and the staff mail's.
  const log = env.USERS.ops;
  for (const op of ['get', 'put', 'delete', 'list']) {
    const inner = env.SOCIAL[op].bind(env.SOCIAL);
    env.SOCIAL[op] = async (a, ...rest) => { log.push([`social_${op}`, op === 'list' ? a?.prefix : a]); return inner(a, ...rest); };
  }
  log.length = 0;
  const out = await quiet(() => sweepTakedowns(env, {}));
  assert.ok(out.cases >= 1, 'a case step ran');
  assert.equal(out.digest.failed, 1, 'the digest ran, and could not be sent');
  assert.equal(out.staff_queue.alerted, true, 'the overdue check ran');
  const at = (pred) => log.findIndex(pred);
  const lastAt = (pred) => log.length - 1 - [...log].reverse().findIndex(pred);
  const caseStep = ([op, k]) => (op === 'social_list' && (k === 'tdpending:' || k === 'tdactive:')) || (op === 'social_put' && String(k).startsWith('tdcase:'));
  const digestOp = ([op, k]) => (op === 'list' && String(k).startsWith('staffmail-queue:') && k !== 'staffmail-queue:') || (op === 'get' && String(k).startsWith('staffmail-digest:'));
  const overdueOp = ([op, k]) => (op === 'get' && k === STAFF_QUEUE_OVERDUE_ALERT_KEY) || (op === 'list' && k === 'staffmail-queue:');
  assert.ok(at(caseStep) >= 0 && at(digestOp) >= 0 && at(overdueOp) >= 0);
  assert.ok(lastAt(caseStep) < at(digestOp), `the digest ran after the case steps (${lastAt(caseStep)} < ${at(digestOp)})`);
  assert.ok(lastAt(digestOp) < at(overdueOp), 'the overdue check ran after the digest');
});

test('the digests stay within 200 storage operations for any number of staff addresses, and the addresses take turns', async () => {
  for (let n = 1; n <= 200; n++) {
    const { addresses, entries, share } = staffDigestPlan(n);
    assert.ok(addresses >= 1 && addresses <= n && entries >= addresses && share >= 1, `plan for ${n}`);
    assert.ok(addresses * 5 + entries * 3 <= STAFF_DIGEST_RUN_OPS, `${n} addresses: ${addresses} x 5 + ${entries} x 3`);
    assert.ok(entries <= STAFF_DIGEST_MAX_ENTRIES);
  }
  assert.deepEqual(staffDigestPlan(1), { addresses: 1, entries: STAFF_DIGEST_MAX_ENTRIES, share: STAFF_DIGEST_MAX_ENTRIES });
  assert.deepEqual(staffDigestPlan(3), { addresses: 3, entries: 50, share: 16 });
  assert.deepEqual(staffDigestPlan(19), { addresses: 19, entries: 35, share: 1 }, '19 addresses: 35 entries an hour, not 19');
  assert.deepEqual(staffDigestPlan(0), { addresses: 0, entries: 0, share: 0 });

  const env = environment();
  const addresses = Array.from({ length: 60 }, (_, i) => `staff${i}@example.com`);
  const at = new Date().toISOString();
  for (const a of addresses) {
    for (let j = 0; j < 3; j++) {
      const e = { kind: 'staff_new', lane: 'general', reference: `EC-MANY-${j}`, queued_at: at, required: true, reason: 'daily_cap' };
      await env.USERS.put(`staffmail-queue:${tag(a)}:${tag(`many-${a}-${j}`)}`, JSON.stringify({ alert_id: `many-${a}-${j}`, ...e, digested_at: null }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS, metadata: e });
    }
  }
  const render = (d) => renderMail('staff_digest', { ...d, at: new Date().toISOString() });
  const plan = staffDigestPlan(addresses.length);
  const base = Math.floor(Date.now() / HOUR) * HOUR;
  const told = new Set();
  for (let hour = 0; hour * plan.addresses < addresses.length; hour++) {
    env.USERS.ops.length = 0;
    const r = await quiet(() => sendStaffDigests(env, addresses, render, { now: base + hour * HOUR }));
    assert.ok(env.USERS.ops.length <= STAFF_DIGEST_RUN_OPS, `hour ${hour}: ${env.USERS.ops.length} operations`);
    assert.ok(r.digests <= plan.addresses);
    assert.equal(r.rotated, addresses.length - plan.addresses);
    for (const m of env.mail.filter((x) => DIGEST.test(readMail(x).subject))) told.add(m.to);
  }
  assert.equal(told.size, addresses.length, 'every address had its digest within a few hours');
});

test('a digest names possible CSAM first, then legal, then the rest, and the run uses its whole budget', async () => {
  const env = environment();
  const at = new Date().toISOString();
  const seed = async (address, lane, kind, n) => {
    for (let j = 0; j < n; j++) {
      const e = { kind, lane, reference: `EC-${lane.toUpperCase()}-${j}`, queued_at: at, required: false, reason: 'send_failed' };
      await env.USERS.put(`staffmail-queue:${tag(address)}:${tag(`${address}-${lane}-${j}`)}`, JSON.stringify({ alert_id: `${lane}-${j}`, ...e, digested_at: null }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS, metadata: e });
    }
  };
  await seed(STAFF, 'general', 'staff_new', 60);
  await seed(STAFF, 'legal', 'staff_legal_owed', 5);
  await seed(STAFF, 'priority', 'staff_forum_csam', 5);
  const render = (d) => renderMail('staff_digest', { ...d, at: new Date().toISOString() });
  const r = await quiet(() => sendStaffDigests(env, [STAFF], render));
  assert.equal(r.named, STAFF_DIGEST_MAX_ENTRIES);
  const [d] = staffMail(env).filter((m) => DIGEST.test(m.subject));
  const rows = d.text.split('\n').filter((l) => /^staff_[a-z_]+: EC-/.test(l)).map((l) => l.split(':')[0]);
  assert.equal(rows.length, STAFF_DIGEST_MAX_ENTRIES);
  assert.deepEqual(rows.slice(0, 10), [...Array(5).fill('staff_forum_csam'), ...Array(5).fill('staff_legal_owed')], 'possible CSAM, then legal');
  assert.ok(rows.slice(10).every((k) => k === 'staff_new'));
  // Every priority and legal entry was named; 20 general entries wait for the next hour.
  const left = queueKeys(env).map((k) => env.USERS.meta.get(k).lane);
  assert.equal(left.length, 20);
  assert.ok(left.every((lane) => lane === 'general'));

  // 19 addresses with 3 waiting each: the budget's 35 entries an hour are all named, not one per address.
  const many = environment();
  const addresses = Array.from({ length: 19 }, (_, i) => `staff${i}@example.com`);
  for (const a of addresses) {
    for (let j = 0; j < 3; j++) {
      const e = { kind: 'staff_new', lane: 'general', reference: `EC-NINETEEN-${j}`, queued_at: at, required: true, reason: 'daily_cap' };
      await many.USERS.put(`staffmail-queue:${tag(a)}:${tag(`${a}-${j}`)}`, JSON.stringify({ alert_id: `${a}-${j}`, ...e, digested_at: null }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS, metadata: e });
    }
  }
  many.USERS.ops.length = 0;
  const all = await quiet(() => sendStaffDigests(many, addresses, render));
  assert.equal(all.digests, 19);
  assert.equal(all.named, staffDigestPlan(19).entries, 'the whole budget');
  assert.equal(all.named, 35);
  assert.ok(many.USERS.ops.length <= STAFF_DIGEST_RUN_OPS, `${many.USERS.ops.length} operations`);
});
