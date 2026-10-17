// Staff alerts have their own allowance, past it an hourly digest, and nothing is dropped (src/staff_alerts.mjs, src/takedown.mjs
// alertStaff). Before this, every staff alert was counted against one account's 'notice' allowance ('takedown-alerts', 30 a
// day per staff address), and a REQUIRED alert past it was refused as 'daily_cap' without a throw: about 30 TAKE IT DOWN filings
// silenced the day's staff mail, possible CSAM included. Adapted from Security's probe zz_debt_4.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';
import { alertStuckReports, legalStuckKey, loadStuckReport, recordStuckReport } from '../src/legal_reports.mjs';
import { alertStaffForumReport, staffLaneOf } from '../src/takedown.mjs';
import { takeMailSlot, STAFF_ALERTS_PER_DAY, STAFF_PRIORITY_ALERTS_PER_DAY, MAIL_PER_DAY } from '../src/mail_limits.mjs';
import { STAFF_DIGEST_MAX_ENTRIES } from '../src/staff_alerts.mjs';

const worker = await loadWorker();
const STAFF = 'security@example.com';
const NOWHERE = 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee';

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
  return { to: m.to, subject: encoded ? Buffer.from(encoded[1], 'base64').toString('utf8') : subjectLine, text: part('text/plain') };
}

function environment() {
  const env = { ...fakeEnv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: STAFF, mail: [], refuse: null };
  env.EMAIL = { send: async (m) => { if (env.refuse && env.refuse(readMail(m))) throw new Error('the mail binding failed'); env.mail.push(m); } };
  return env;
}
const staffMail = (env) => env.mail.filter((m) => m.to === STAFF).map(readMail);
const DIGEST = /^Staff alerts past the day's allowance/;

async function file(env, i, { minor = false } = {}) {
  const body = {
    requester_name: `Filer Number ${i}`, signature: `Filer Number ${i}`, relationship: 'self', contact_email: `filer${i}@example.org`,
    locations: [`https://eustress.dev/simulation/${NOWHERE}`], description: 'A private photo of me shared without my consent.', good_faith: true,
    ...(minor ? { depicts_minor: true } : {}),
  };
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/takedown/requests', {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': `10.${Math.floor(i / 200)}.${i % 200}.9` }, body: JSON.stringify(body),
  }), env, ctx);
  const json = await res.json();
  assert.equal(res.status, 201, JSON.stringify(json));
  return json.reference;
}
const caseOf = async (env, reference) => JSON.parse(await env.SOCIAL.get(`tdcase:${JSON.parse(await env.SOCIAL.get(`caseref:${reference}`)).id}`));

async function runCron(env) {
  const waits = [];
  await worker.scheduled({ scheduledTime: Date.now() }, env, { waitUntil: (p) => waits.push(p), passThroughOnException() {} });
  await Promise.all(waits);
}

test('the lanes: staff mail has buckets of its own, and possible CSAM and legal reports are never in the general lane', () => {
  assert.equal(STAFF_ALERTS_PER_DAY, 200);
  assert.ok(STAFF_PRIORITY_ALERTS_PER_DAY > STAFF_ALERTS_PER_DAY, 'the possible-CSAM lane has the higher allowance');
  assert.equal(MAIL_PER_DAY.staff, STAFF_ALERTS_PER_DAY);
  assert.equal(MAIL_PER_DAY.notice, 30, 'the users\' notice allowance is unchanged');
  for (const k of ['staff_new', 'staff_review', 'staff_failure', 'staff_relocked', 'staff_candidates', 'staff_attempt']) assert.equal(staffLaneOf(k, {}), 'general', k);
  for (const k of ['staff_csam', 'staff_forum_csam', 'staff_evidence_failure']) assert.equal(staffLaneOf(k, {}), 'priority', k);
  for (const k of ['staff_failure', 'staff_review', 'staff_attempt']) assert.equal(staffLaneOf(k, { possible_csam: true }), 'priority', `${k} about possible CSAM`);
  for (const k of ['staff_legal_report', 'staff_legal_stuck']) assert.equal(staffLaneOf(k, {}), 'legal', k);
});

test('zz_debt_4: 31 stuck re-alerts are all sent and recorded, and a possible-CSAM staff alert after them is still sent', async () => {
  const env = { USERS: fakeKv(), SOCIAL: fakeKv(), AUDIT_LOG: fakeKv(), JWT_SECRET: 'zz4', __EmailMessage: StubMessage, mail: [] };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  const old = new Date(Date.now() - 2 * 86400000).toISOString();
  for (let i = 0; i < 31; i++) {
    const id = `${String(i).padStart(2, '0')}${'a'.repeat(22)}-1`;
    await env.SOCIAL.put(legalStuckKey('forum', id), JSON.stringify({ case_kind: 'forum', case_id: id, report_kind: 'threat', reason: 'no_account', stuck_at: old, alerted_at: old, alerts: 1 }));
  }
  const out = await quiet(() => alertStuckReports(env));
  assert.equal(out.alerted, 31);
  assert.equal(env.mail.length, 31, 'every stuck re-alert was sent');
  const last = await loadStuckReport(env, 'forum', `30${'a'.repeat(22)}-1`);
  assert.equal(last.alerts, 2);
  const told = await quiet(() => alertStaffForumReport(env, { reference: 'EC-TEST', post_id: 'p', thread_id: 't', at: new Date().toISOString(), queue_url: 'https://x' }));
  assert.ok(told.sent >= 1, 'a possible-CSAM staff alert is still delivered');
  assert.ok(![...env.USERS.store.keys()].some((k) => k.includes('notice:takedown-alerts')), 'no staff alert spends an account notice allowance');
});

test('250 filings in a day: each required alert is sent or carried by a digest that sent, possible CSAM is never delayed, nothing is dropped', async () => {
  const env = environment();
  const minors = new Set([30, 205, 230, 249]);
  const refs = [];
  for (let i = 0; i < 250; i++) {
    const before = staffMail(env).length;
    const ref = await quiet(() => file(env, i, { minor: minors.has(i) }));
    refs.push(ref);
    if (minors.has(i)) {
      // The possible-CSAM alert went out with the filing, whatever the general lane holds.
      const fresh = staffMail(env).slice(before);
      assert.ok(fresh.some((m) => m.subject === `Possible CSAM in a TAKE IT DOWN request: ${ref}`), `the possible-CSAM alert of filing ${i} was sent at once`);
    }
  }
  const direct = staffMail(env);
  const received = direct.filter((m) => m.subject.startsWith('TAKE IT DOWN request received: '));
  assert.equal(received.length, STAFF_ALERTS_PER_DAY, 'the general lane sends its allowance and no more');
  assert.equal(direct.filter((m) => m.subject.startsWith('Possible CSAM')).length, minors.size);
  const queued = [];
  for (const ref of refs) {
    const rec = await caseOf(env, ref);
    if (rec.pending.includes('staff')) queued.push(ref);
  }
  assert.equal(queued.length, 250 - minors.size - STAFF_ALERTS_PER_DAY, 'every alert past the allowance stays owed: none was counted as told');
  for (const ref of queued) assert.match((await caseOf(env, ref)).step_errors.staff.error, /hourly digest/);

  // The hourly cron: the owed steps, then the digest (last, so it spends only what the steps left).
  await quiet(() => runCron(env));
  const digests = staffMail(env).filter((m) => DIGEST.test(m.subject));
  assert.equal(digests.length, 1, 'one digest');
  assert.ok(queued.length <= STAFF_DIGEST_MAX_ENTRIES);
  for (const ref of queued) assert.ok(digests[0].text.includes(ref), `the digest names ${ref}`);
  assert.ok(digests[0].subject.includes(`${queued.length} waiting`), digests[0].subject);
  // The next run's steps find each alert carried by the digest that sent.
  await quiet(() => runCron(env));
  for (const ref of refs) {
    const rec = await caseOf(env, ref);
    assert.ok(!rec.pending.includes('staff'), `the staff step of ${ref} is done`);
    const named = staffMail(env).some((m) => m.subject.endsWith(`: ${ref}`) || (DIGEST.test(m.subject) && m.text.includes(ref)));
    assert.ok(named, `${ref} reached staff, directly or in the digest`);
  }
  // A second run in the same hour sends no second digest, and no alert is mailed twice.
  const count = staffMail(env).length;
  await quiet(() => runCron(env));
  assert.equal(staffMail(env).filter((m) => DIGEST.test(m.subject)).length, 1);
  assert.equal(staffMail(env).filter((m) => m.subject.startsWith('TAKE IT DOWN request received: ')).length, STAFF_ALERTS_PER_DAY);
  assert.ok(staffMail(env).length >= count);
  assert.ok(![...env.USERS.store.keys()].some((k) => k.includes('notice:takedown-alerts')), 'staff mail never shares the notice bucket');
});

test('a required alert past the allowance is delivered only once a digest carrying it has SENT', async () => {
  const env = environment();
  for (let i = 0; i < STAFF_ALERTS_PER_DAY; i++) assert.equal(await takeMailSlot(env, STAFF, 'staff'), true);
  const ref = await quiet(() => file(env, 1));
  assert.ok((await caseOf(env, ref)).pending.includes('staff'));
  // The digest cannot be sent this hour: the step stays owed, and nothing is marked carried.
  env.refuse = (m) => DIGEST.test(m.subject);
  await quiet(() => runCron(env));
  let rec = await caseOf(env, ref);
  assert.ok(rec.pending.includes('staff'), 'a digest that did not send delivers nothing');
  assert.equal(rec.step_errors.staff.tries, 2);
  assert.equal(staffMail(env).filter((m) => DIGEST.test(m.subject)).length, 0);
  // The next run sends it after the steps, and the step's retry on the run after finds it carried.
  env.refuse = null;
  await quiet(() => runCron(env));
  const digests = staffMail(env).filter((m) => DIGEST.test(m.subject));
  assert.equal(digests.length, 1);
  assert.ok(digests[0].text.includes(ref));
  await quiet(() => runCron(env));
  rec = await caseOf(env, ref);
  assert.ok(!rec.pending.includes('staff'), 'carried by a digest that sent: delivered');
});

test('a required alert refused for any reason throws, stays owed, and the hourly sweep sends it', async () => {
  const env = environment();
  env.refuse = (m) => m.to === STAFF;
  const ref = await quiet(() => file(env, 2));
  let rec = await caseOf(env, ref);
  assert.ok(rec.pending.includes('staff'), 'a refused required alert is not counted as told');
  assert.match(rec.step_errors.staff.error, /the send failed\) and waits for the hourly digest/, 'a required alert whose send failed waits on the list');
  env.refuse = null;
  await quiet(() => runCron(env));
  rec = await caseOf(env, ref);
  assert.ok(!rec.pending.includes('staff'));
  assert.ok(staffMail(env).some((m) => m.subject === `TAKE IT DOWN request received: ${ref}`));
  // No mail binding at all: nothing can be sent, and the step stays owed.
  const bare = environment();
  delete bare.EMAIL;
  const other = await quiet(() => file(bare, 3));
  assert.ok((await caseOf(bare, other)).pending.includes('staff'));
});

test('a stuck alert is recorded as alerted only when it was sent', async () => {
  const env = environment();
  env.refuse = (m) => m.subject.startsWith('A legal report is stuck');
  const r = await quiet(() => recordStuckReport(env, { caseKind: 'forum', caseId: `${'d'.repeat(24)}-1`, reportKind: 'threat', category: 'threat', reason: 'no_account', reference: 'EC-STUCK' }));
  assert.equal(r.ok, true);
  let rec = await loadStuckReport(env, 'forum', `${'d'.repeat(24)}-1`);
  assert.equal(rec.alerted_at, null, 'not sent: not recorded as alerted');
  assert.equal(rec.alerts, 0);
  env.refuse = null;
  const out = await quiet(() => alertStuckReports(env));
  assert.equal(out.alerted, 1, 'the next run sends it');
  rec = await loadStuckReport(env, 'forum', `${'d'.repeat(24)}-1`);
  assert.ok(rec.alerted_at);
  assert.equal(rec.alerts, 1);
  assert.equal(staffMail(env).filter((m) => m.subject.startsWith('A legal report is stuck')).length, 1);
});
