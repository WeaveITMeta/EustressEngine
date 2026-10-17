// A TAKE IT DOWN case whose removal is still owed reminds staff every hour of the 48 the law allows, and no digest silences a
// reminder (src/takedown.mjs staffFailureAlertId, src/staff_alerts.mjs sendStaffAlert `once`). A digest's mark means
// delivered only for a retry of that same alert; a reminder while a lock step is owed is keyed by the UTC hour, so each hour
// is its own alert. Adapted from Security's probe zz_d4_1 (D4-1): with the reminder keyed by the day, an outage ending inside
// the hour-6 run left 17 silent hours on both staff addresses, and about 23 at worst.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { fakeKv } from './helpers/load_worker.mjs';
import { sweepTakedowns, staffFailureAlertId, sendStaffDigest } from '../src/takedown.mjs';
import { sendStaffAlert } from '../src/staff_alerts.mjs';

const STAFF = 'security@eustress.dev';
const ADMIN = 'admin-d4-1';
const AUTHOR = 'author-d4-1';
const ID = 'cccccccc-7777-4777-8777-cccccccccccc';
const LISTING = 'dddddddd-7777-4777-8777-dddddddddddd';
const REF = 'EC-D4-TD01-0000-0000-0000';
const HOUR = 3600000;
const FAILING = /^A TAKE IT DOWN step is failing/;
const DIGEST = /^Staff alerts waiting: /;

const RealDate = Date;
let fake = 0;
class FakeDate extends RealDate {
  constructor(...a) { if (a.length === 0) super(fake); else super(...a); }
  static now() { return fake; }
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
function subjectOf(m) {
  const line = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
  const b = /^=\?UTF-8\?B\?(.*)\?=$/.exec(line);
  return b ? Buffer.from(b[1], 'base64').toString('utf8') : line;
}
function textOf(m) {
  const at = m.raw.indexOf('Content-Type: text/plain; charset=utf-8');
  const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
  try { return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8'); } catch { return body; }
}
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };

async function environment(mailWorks, clock) {
  const env = { USERS: pagedKv(), SOCIAL: pagedKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), JWT_SECRET: 's', __EmailMessage: StubMessage, mail: [] };
  // Every R2 write fails: the lock step can never be written.
  env.SCENES = { put: async () => { throw new Error('R2 is unavailable'); }, get: async () => null, head: async () => null, delete: async () => {}, list: async () => ({ objects: [], truncated: false }) };
  env.EMAIL = { send: async (m) => { clock.sends += 1; if (!mailWorks(clock.hour, clock.sends, m)) throw new Error('the mail binding is unavailable'); env.mail.push({ hour: clock.hour, to: m.to, subject: subjectOf(m), text: textOf(m) }); } };
  for (const id of [AUTHOR, ADMIN]) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.SOCIAL.put(`tdcase:${ID}`, JSON.stringify({
    id: ID, reference: REF, status: 'received', possible_csam: true, received_at: new Date(fake - HOUR).toISOString(), removed_at: null, updated_at: new Date().toISOString(),
    request: { requester_name: 'Filer', requester_email: 'filer@example.org', description: 'x' },
    locations: [], targets: [{ sim_id: LISTING, author_id: AUTHOR, via: 'request', locked_at: null }], forum_targets: [], unresolved: [], hashes: [], pending: [`lock:${LISTING}`], step_errors: {}, reviews: [], history: [], attempts: 0,
  }));
  await env.SOCIAL.put(`tdpending:${ID}`, '1');
  return env;
}

/// 48 hourly sweeps on a fake clock from 00:10 UTC. `mailWorks(hour, sendNumberInTheRun, message)` says whether a send works.
/// `each(env, hour)` runs after each sweep. Gives, for each hour and each staff address, whether it heard about the case.
async function run48(mailWorks, { hours = 48, each = null } = {}) {
  globalThis.Date = FakeDate;
  fake = RealDate.parse('2026-10-20T00:10:00.000Z');
  try {
    const clock = { hour: 0, sends: 0 };
    const env = await environment(mailWorks, clock);
    const perHour = [];
    for (clock.hour = 0; clock.hour < hours; clock.hour++) {
      clock.sends = 0;
      await quiet(() => sweepTakedowns(env, {}));
      if (each) await each(env, clock.hour);
      const told = (to) => env.mail.filter((m) => m.hour === clock.hour && m.to === to && (FAILING.test(m.subject) || (DIGEST.test(m.subject) && m.text.includes(REF)))).length > 0;
      perHour.push({ hour: clock.hour, staff: told(STAFF), admin: told(`${ADMIN}@example.com`) });
      fake += HOUR;
    }
    return { env, perHour };
  } finally {
    globalThis.Date = RealDate;
  }
}
const silent = (perHour, from, who) => perHour.filter((h) => h.hour >= from && !h[who]).map((h) => h.hour);

test('zz_d4_1 A: an outage over hours 0-5 that ends before the hour-6 run: both staff addresses hear every hour from hour 6', async () => {
  const { perHour } = await run48((h) => h >= 6);
  assert.deepEqual(silent(perHour, 6, 'staff'), []);
  assert.deepEqual(silent(perHour, 6, 'admin'), []);
});

test('zz_d4_1 B: the outage ends inside the hour-6 run (its direct sends fail, the digest after them sends): no silent hour', async () => {
  const { perHour } = await run48((h, n) => h > 6 || (h === 6 && n > 2));
  assert.deepEqual(silent(perHour, 6, 'staff'), []);
  assert.deepEqual(silent(perHour, 6, 'admin'), []);
});

test('zz_d4_1 C: no outage, one transient failure of the hour-10 send to security@: the digest carries it and later hours are told', async () => {
  const { perHour } = await run48((h, n, m) => !(h === 10 && m.to === STAFF && FAILING.test(subjectOf(m))));
  assert.deepEqual(silent(perHour, 0, 'staff'), []);
  assert.deepEqual(silent(perHour, 0, 'admin'), []);
});

test('zz_d4_1 every ending: whichever run the outage ends in, and wherever inside it, no hour after it is silent', async () => {
  for (const end of [1, 3, 6, 11, 23]) {
    for (const inside of [0, 1, 2]) {
      const { perHour } = await run48((h, n) => h > end || (h === end && n > inside), { hours: end + 30 });
      assert.deepEqual(silent(perHour, end, 'staff'), [], `outage to hour ${end}, ${inside} failed sends inside it: security@`);
      assert.deepEqual(silent(perHour, end, 'admin'), [], `outage to hour ${end}, ${inside} failed sends inside it: the administrator`);
    }
  }
});

test('while a lock step is owed, each hour of an outage is its own reminder on the list, and the digest after it names each', async () => {
  const counts = [];
  const { env } = await run48((h) => h >= 4, {
    hours: 5,
    each: (env) => { counts.push([...env.USERS.store.keys()].filter((k) => k.startsWith('staffmail-queue:')).length); },
  });
  // Two staff addresses: two entries an hour through hours 0-3; the hour-4 digest carries them.
  assert.deepEqual(counts.slice(0, 4), [2, 4, 6, 8]);
  const digest = env.mail.find((m) => m.to === STAFF && DIGEST.test(m.subject));
  assert.ok(digest, 'the digest after the outage');
  assert.equal(digest.text.split('staff_failure').length - 1, 4, 'one row for each hour of the outage');
  const lock = { id: ID, pending: [`lock:${LISTING}`] };
  assert.match(staffFailureAlertId(lock, Date.parse('2026-10-20T05:10:00.000Z')), /:2026-10-20T05$/, 'a lock owed: the hour');
  assert.match(staffFailureAlertId({ id: ID, pending: ['notice:author'] }, Date.parse('2026-10-20T05:10:00.000Z')), /:2026-10-20$/, 'otherwise the day');
});

test('a digest\'s mark is delivered only for a retry of the same alert; a new reminder under the same id is sent, or queued again', async () => {
  const env = { USERS: pagedKv(), __EmailMessage: StubMessage, mail: [] };
  let up = false;
  env.EMAIL = { send: async (m) => { if (!up) throw new Error('the mail binding is unavailable'); env.mail.push(m); } };
  const rendered = { subject: 'A TAKE IT DOWN step is failing: test', text: 'x', html: '<p>x</p>' };
  const opts = { lane: 'priority', alertId: 'staff-failure:test:notice:2026-10-20', kind: 'staff_failure', reference: REF };
  assert.equal((await quiet(() => sendStaffAlert(env, [STAFF], rendered, opts))).queued, 1);
  up = true;
  await quiet(() => sendStaffDigest(env, { recipients: [STAFF] }));
  const queue = () => [...env.USERS.store.keys()].filter((k) => k.startsWith('staffmail-queue:')).length;
  assert.equal(queue(), 0, 'the digest carried it');
  // The next reminder the same day: sent, not skipped as already delivered.
  const next = await quiet(() => sendStaffAlert(env, [STAFF], rendered, opts));
  assert.deepEqual([next.sent, next.digested], [1, 0]);
  // And one that fails after a digest carried the last: queued again, for the next digest.
  up = false;
  const failed = await quiet(() => sendStaffAlert(env, [STAFF], rendered, opts));
  assert.deepEqual([failed.queued, failed.digested], [1, 0]);
  assert.equal(queue(), 1);
  // A retry of one alert (`once`, as a required alert is) finds the digest's mark: delivered, not sent twice.
  up = true;
  await quiet(() => sendStaffDigest(env, { recipients: [STAFF], now: Date.now() + HOUR }));
  const retry = await quiet(() => sendStaffAlert(env, [STAFF], rendered, { ...opts, once: true }));
  assert.deepEqual([retry.sent, retry.digested], [0, 1]);
});
