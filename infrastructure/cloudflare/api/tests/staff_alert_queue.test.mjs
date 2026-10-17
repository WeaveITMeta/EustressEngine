// A staff alert past its lane's allowance waits on the over-cap queue for the hourly digest (src/staff_alerts.mjs). When the
// digest cannot be sent (a mail outage), the queue must not lapse in silence: once any entry has waited 24 hours, staff get one
// staff_failure alert a day on the legal lane, with counts, ages and kinds only, and the entries are kept 30 days, well past
// that. GET /api/admin/legal/staff-alerts shows the same state, audited before it is answered (fail-closed).
// Security's note on commit 65a9db55.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, mintToken, ctx } from './helpers/load_worker.mjs';
import { alertStaffQueueOverdue } from '../src/takedown.mjs';
import { takeMailSlot, STAFF_ALERTS_PER_DAY } from '../src/mail_limits.mjs';
import { staffQueueState, STAFF_OVER_CAP_TTL_SECONDS, STAFF_QUEUE_OVERDUE_MS, STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS, STAFF_QUEUE_OVERDUE_ALERT_KEY } from '../src/staff_alerts.mjs';

const worker = await loadWorker();
const STAFF = 'security@example.com';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const ADMIN_EMAIL = 'staff.person@example.com';
const NOWHERE = 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee';
const HOUR = 3600000;
const OVERDUE = /^Staff alerts undelivered for /;
const DIGEST = /^Staff alerts past the day's allowance/;

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

function environment() {
  const env = { ...fakeEnv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: STAFF, mail: [], refuse: null };
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

/// Moves every queued entry's time back by `ms`, as if it had waited that long.
function age(env, ms) {
  for (const k of queueKeys(env)) {
    const e = JSON.parse(env.USERS.store.get(k));
    env.USERS.store.set(k, JSON.stringify({ ...e, queued_at: new Date(Date.parse(e.queued_at) - ms).toISOString() }));
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
