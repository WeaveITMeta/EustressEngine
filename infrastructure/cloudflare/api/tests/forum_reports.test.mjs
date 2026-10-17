// Forum reports (src/moderation.mjs fileForumReport, resolveForumCase, forumPostModeration): one case per post, distinct
// reporters, the hide, remove and gate rules (csam and nonconsensual_sexual hide at the first report; 3 counted reporters
// citing csam, nonconsensual_sexual, personal_info, threat or scam remove the post; spam is never hidden or removed by reports;
// adult_sexual gates at 3), the new-account brigading rule, the reporter's daily cap, the refusals, the sealed evidence pointer
// (a hash of the body, never the body), the Review Workspace queue and its notices, the staff alert, a person's decision, and
// what the reporter and the author see in their case history (src/case_history.mjs).
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { fakeKv, fakeBucket } from './helpers/load_worker.mjs';
import {
  fileForumReport, resolveForumCase, forumPostModeration, loadForumCase, listWorkspaceQueue, makeToolExecutor, MODERATION_TOOLS,
  forumCaseKey, forumCaseR2Key, forumReportCapKey, forumPostLockKey, workspaceQueueKey, WORKSPACE_KINDS, FORUM_REPORT_CATEGORIES, FORUM_QUESTION_CATEGORIES,
  FORUM_REMOVABLE_CATEGORIES, FORUM_REMOVE_COUNTED, FORUM_HIDE_AT_ONCE, DEFAULT_THRESHOLDS, forumBodyHash, FORUM_EVIDENCE_KEY_LABEL,
} from '../src/moderation.mjs';
import { CASE_KINDS, REFERENCE_PATTERN, resolveCaseReference } from '../src/case_reference.mjs';
import { handleCaseRoute } from '../src/case_history.mjs';

const THREAD = '0123456789abcdef01234567';
const POST = `${THREAD}-3`;
const AUTHOR = 'author-0001';
const MEMBER = 'member-0001';
const OLD = ['old-0001', 'old-0002', 'old-0003', 'old-0004', 'old-0005', 'old-0006'];
const NEW = ['new-0001', 'new-0002', 'new-0003', 'new-0004', 'new-0005'];
const NOW = Date.parse('2026-10-15T18:00:00.000Z');
const NOTE = 'Zebra marmalade reviewer note';
// The post's body as the forum stores it. It is hashed and must never be kept anywhere.
const BODY = 'Pangolin saxophone body text\nwith a second line';
const JWT = 'test-jwt-secret-forum';
// The evidence hash, computed here the way the Worker must: HMAC-SHA256 under HMAC-SHA256(JWT_SECRET, 'forum-evidence-v1').
const keyedHash = (body, secret = JWT) => crypto.createHmac('sha256', crypto.createHmac('sha256', secret).update('forum-evidence-v1').digest()).update(body, 'utf8').digest('hex');
const PLAIN_SHA = crypto.createHash('sha256').update(BODY, 'utf8').digest('hex');
const BODY_HASH = keyedHash(BODY);
const SHAPE = ['ok', 'reference', 'reports', 'hide', 'gate', 'remove', 'hide_reason', 'case_version'];
// What serving honours, without the case version (asserted on its own where it matters).
const served = async (env, postId = POST) => { const { case_version: _v, ...rest } = await forumPostModeration({ env, postId }); return rest; };

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

async function envWith() {
  const env = { JWT_SECRET: JWT, USERS: fakeKv(), SOCIAL: fakeKv(), SCENES: fakeBucket(), AUDIT_LOG: fakeKv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: 'security@example.com', mail: [] };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  const user = (id, created) => env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, email: `${id}@example.com`, created_at: new Date(created).toISOString(), kyc_verified: true }));
  for (const id of [AUTHOR, MEMBER, ...OLD]) await user(id, NOW - 30 * 86400 * 1000);
  // Accounts made an hour before the report: recorded, not counted.
  for (const id of NEW) await user(id, NOW - 3600 * 1000);
  await env.SOCIAL.put('workspace:review', JSON.stringify({ space_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', members: [MEMBER] }));
  return env;
}

const quiet = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};
const report = (env, reporterId, category, extra = {}) => quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId, category, postBody: BODY, postCreatedAt: '2026-10-14T12:00:00.000Z', now: NOW, ...extra }));
const resolve = (env, args) => quiet(() => resolveForumCase({ env, postId: POST, reviewerId: 'admin-0001', ...args }));
const caseOf = (env, postId = POST) => loadForumCase(env, postId);

/// A message's subject, plain-text part and HTML part, decoded.
function mailText(m) {
  const part = (type) => {
    const at = m.raw.indexOf(`Content-Type: ${type}; charset=utf-8`);
    if (at < 0) return '';
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
  };
  const text = part('text/plain');
  assert.ok(text.length > 40, 'the message decodes');
  return `${/^Subject: (.*)$/m.exec(m.raw)?.[1] || ''}\n${text}\n${part('text/html')}`;
}

async function history(env, who, path = '/api/account/cases') {
  const deps = { verifyAuth: async () => who, json: (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers }) };
  const res = await handleCaseRoute(new Request(`https://api.eustress.dev${path}`, { method: 'GET' }), new URL(`https://api.eustress.dev${path}`), env, {}, deps);
  return { status: res.status, json: await res.json() };
}

// -- one case per post, distinct reporters ----------------------------------------------------------------------------------

test('the report categories are the forum battery\'s, and forum is a case kind and a Review Workspace kind', () => {
  assert.equal(FORUM_REPORT_CATEGORIES, FORUM_QUESTION_CATEGORIES, 'one list, not a copy');
  assert.deepEqual([...FORUM_REPORT_CATEGORIES], ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam', 'spam', 'adult_sexual']);
  assert.deepEqual([...FORUM_HIDE_AT_ONCE], ['csam', 'nonconsensual_sexual']);
  assert.deepEqual([...FORUM_REMOVE_COUNTED], ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam']);
  assert.ok(CASE_KINDS.includes('forum'));
  assert.ok(WORKSPACE_KINDS.includes('forum'));
  assert.deepEqual([DEFAULT_THRESHOLDS.forum_hide_reporters, DEFAULT_THRESHOLDS.forum_reports_per_reporter_per_day, DEFAULT_THRESHOLDS.forum_reporter_min_age_hours], [3, 20, 24]);
});

test('a post has one case; a repeat report from the same account changes nothing; 3 distinct reporters remove it', async () => {
  const env = await envWith();
  const first = await report(env, OLD[0], 'threat', { note: NOTE });
  assert.deepEqual(Object.keys(first), SHAPE);
  assert.deepEqual([first.ok, first.reports, first.hide, first.gate, first.remove, first.hide_reason], [true, 1, false, false, false, null]);
  assert.match(first.reference, REFERENCE_PATTERN);
  assert.deepEqual(await resolveCaseReference(env, first.reference), { reference: first.reference, kind: 'forum', id: POST });

  const repeat = await report(env, OLD[0], 'scam', { note: 'a second try' });
  assert.deepEqual(repeat, first, 'a repeat is answered as the first was');
  let rec = await caseOf(env);
  assert.equal(rec.reports.length, 1);
  assert.deepEqual(rec.reporters, [OLD[0]]);
  assert.equal(rec.reports[0].note, NOTE);
  assert.equal(await env.SOCIAL.get(forumReportCapKey(OLD[0], '2026-10-15')), '1', 'a repeat does not count against the cap');

  const second = await report(env, OLD[1], 'scam');
  assert.deepEqual([second.reports, second.hide, second.remove, second.reference], [2, false, false, first.reference]);
  const third = await report(env, OLD[2], 'personal_info');
  assert.deepEqual([third.reports, third.hide, third.gate, third.remove, third.hide_reason], [3, true, false, true, 'reports']);
  rec = await caseOf(env);
  assert.deepEqual([rec.post_id, rec.thread_id, rec.author_id, rec.status, rec.removed_by], [POST, THREAD, AUTHOR, 'removed', 'reports']);
  assert.deepEqual(rec.reports.map((r) => [r.reporter_id, r.category, r.counted]), [[OLD[0], 'threat', true], [OLD[1], 'scam', true], [OLD[2], 'personal_info', true]]);
  assert.ok(rec.history.some((h) => h.event === 'removed'));
  assert.deepEqual(await served(env), { hidden: true, gated: false, removed: true, locked: false, hide_reason: 'reports' });
  // Another post is its own case.
  const other = await quiet(() => fileForumReport({ env, postId: `${THREAD}-4`, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[0], category: 'threat', postBody: BODY, now: NOW }));
  assert.equal(other.reports, 1);
  assert.notEqual(other.reference, first.reference);
});

test('csam and nonconsensual_sexual hide the post at the first report, even from a new account', async () => {
  for (const category of ['csam', 'nonconsensual_sexual']) {
    for (const reporter of [OLD[0], NEW[0]]) {
      const env = await envWith();
      const r = await report(env, reporter, category);
      assert.deepEqual([r.reports, r.hide, r.gate, r.remove, r.hide_reason], [1, true, false, false, category], `${category} from ${reporter}`);
      assert.equal((await caseOf(env)).status, 'hidden');
    }
  }
  // The other categories do not hide at one report.
  for (const category of ['personal_info', 'threat', 'scam', 'spam', 'adult_sexual']) {
    const env = await envWith();
    const r = await report(env, OLD[0], category);
    assert.deepEqual([r.hide, r.remove], [false, false], category);
  }
});

test('below 3 counted reporters nothing is removed, and a mix of illegal categories counts together', async () => {
  const env = await envWith();
  assert.equal((await report(env, OLD[0], 'threat')).remove, false);
  assert.equal((await report(env, OLD[1], 'threat')).remove, false, 'two reports do not remove');
  assert.equal((await report(env, OLD[2], 'spam')).remove, false, 'spam does not count toward removal');
  assert.equal((await report(env, OLD[3], 'adult_sexual')).remove, false, 'adult content does not count toward removal');
  const r = await report(env, OLD[4], 'scam');
  assert.deepEqual([r.remove, r.hide], [true, true], 'the third report citing an illegal category removes');
  // A csam report hides at once and counts toward the 3 as well.
  const env2 = await envWith();
  assert.deepEqual([(await report(env2, OLD[0], 'csam')).remove, (await report(env2, OLD[1], 'threat')).remove], [false, false]);
  assert.equal((await report(env2, OLD[2], 'personal_info')).remove, true);
});

test('spam is never hidden or removed by reports, however many: a person reviews it', async () => {
  const env = await envWith();
  for (const id of OLD) {
    const r = await report(env, id, 'spam');
    assert.deepEqual([r.hide, r.gate, r.remove], [false, false, false]);
  }
  const rec = await caseOf(env);
  assert.equal(rec.status, 'open');
  assert.deepEqual((await listWorkspaceQueue(env, { kind: 'forum' })).items.map((i) => [i.post_id, i.status]), [[POST, 'open']]);
});

test('3 counted reports of adult content gate the post behind the Adult content gate; it is not hidden or removed', async () => {
  const env = await envWith();
  assert.equal((await report(env, OLD[0], 'adult_sexual')).gate, false);
  assert.equal((await report(env, NEW[0], 'adult_sexual')).gate, false);
  assert.equal((await report(env, NEW[1], 'adult_sexual')).gate, false, 'new accounts do not count toward the gate');
  assert.equal((await report(env, OLD[1], 'adult_sexual')).gate, false);
  const r = await report(env, OLD[2], 'adult_sexual');
  assert.deepEqual([r.hide, r.gate, r.remove, r.hide_reason, r.reports], [false, true, false, null, 5]);
  assert.deepEqual(await served(env), { hidden: false, gated: true, removed: false, locked: false, hide_reason: null });
});

test('a brigade of accounts less than 24 hours old is recorded and never removes a post on its own', async () => {
  const env = await envWith();
  for (const id of NEW) {
    const r = await report(env, id, 'threat');
    assert.deepEqual([r.hide, r.remove], [false, false], `new account ${id}`);
  }
  let rec = await caseOf(env);
  assert.equal(rec.reporters.length, 5, 'every report is recorded');
  assert.ok(rec.reports.every((x) => x.counted === false));
  assert.equal(rec.tally.remove, 0);
  assert.equal((await report(env, OLD[0], 'threat')).remove, false);
  assert.equal((await report(env, OLD[1], 'threat')).remove, false);
  const r = await report(env, OLD[2], 'threat');
  assert.deepEqual([r.remove, r.reports], [true, 8], 'three accounts old enough remove it');
  rec = await caseOf(env);
  assert.equal(rec.tally.remove, 3);
  // An account whose age cannot be read is not counted either.
  const env2 = await envWith();
  await env2.USERS.put('user:ageless-1', JSON.stringify({ id: 'ageless-1' }));
  await report(env2, 'ageless-1', 'threat');
  assert.equal((await caseOf(env2)).reports[0].counted, false);
});

test('a reporter files at most 20 reports a day; past that, a code and a message; the next day the count starts again', async () => {
  const env = await envWith();
  const fileOn = (n, now = NOW) => quiet(() => fileForumReport({ env, postId: `${THREAD}-${n}`, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[0], category: 'spam', postBody: BODY, now }));
  for (let n = 1; n <= 20; n++) assert.equal((await fileOn(n)).ok, true, `report ${n}`);
  const over = await fileOn(21);
  assert.deepEqual([over.ok, over.code, over.status], [false, 'rate_limited', 429]);
  assert.match(over.message, /20 posts a day/);
  assert.equal(await caseOf(env, `${THREAD}-21`), null, 'nothing was recorded');
  // A repeat of one already filed is still answered.
  assert.equal((await fileOn(5)).ok, true);
  assert.equal((await fileOn(21, NOW + 86400 * 1000)).ok, true, 'a new day');
  // Tunable.
  const tuned = await envWith();
  tuned.MODERATION_THRESHOLDS = JSON.stringify({ forum_reports_per_reporter_per_day: 1 });
  assert.equal((await quiet(() => fileForumReport({ env: tuned, postId: `${THREAD}-1`, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[0], category: 'spam', postBody: BODY, now: NOW }))).ok, true);
  assert.equal((await quiet(() => fileForumReport({ env: tuned, postId: `${THREAD}-2`, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[0], category: 'spam', postBody: BODY, now: NOW }))).code, 'rate_limited');
});

test('a self-report, a category outside the list, a bad id, a post outside its thread, no body, a long note and no account are refused', async () => {
  const env = await envWith();
  const refused = async (args, code) => {
    const r = await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[0], category: 'threat', postBody: BODY, now: NOW, ...args }));
    assert.equal(r.ok, false, code);
    assert.equal(r.code, code, JSON.stringify(args));
    assert.equal(typeof r.message, 'string');
  };
  await refused({ reporterId: AUTHOR }, 'self_report');
  for (const category of ['A1', 'A5', 'hate', 'harassment', 'criticism', '', null, undefined]) await refused({ category }, 'bad_category');
  for (const postId of ['abc', `${THREAD}-0`, `${THREAD}-1234567`, `${THREAD.toUpperCase()}-1`, `${THREAD}_1`, null]) await refused({ postId }, 'bad_post');
  await refused({ threadId: 'fedcba9876543210fedcba98' }, 'bad_thread');
  await refused({ threadId: 'nope' }, 'bad_thread');
  await refused({ reporterId: 'a b' }, 'bad_reporter');
  await refused({ postAuthorId: '' }, 'bad_author');
  for (const postBody of [undefined, '', 42]) await refused({ postBody }, 'bad_body');
  await refused({ note: 'n'.repeat(501) }, 'note_too_long');
  await refused({ note: 42 }, 'bad_note');
  await refused({ reporterId: 'nobody-0001' }, 'no_account');
  assert.equal(await env.SOCIAL.get(forumCaseKey(POST)), null, 'no refusal made a case');
  assert.equal(await env.SCENES.get(forumCaseR2Key(POST)), null, 'no refusal made a case in R2');
  // 500 characters is the most a note keeps.
  assert.equal((await report(env, OLD[0], 'threat', { note: 'n'.repeat(500) })).ok, true);
});

// -- the record ---------------------------------------------------------------------------------------------------------------

test('the case keeps a sealed evidence pointer: the post id, a hash of the body and the times, never the body itself', async () => {
  const env = await envWith();
  const saved = {};
  const printed = [];
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = (...a) => printed.push(a.map(String).join(' ')); }
  try {
    for (const id of OLD.slice(0, 3)) await fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: id, category: 'threat', postBody: BODY, postCreatedAt: '2026-10-14T12:00:00.000Z', now: NOW });
    // The author edited the post before a later report: the first hash stays sealed, the new one is recorded beside it.
    await fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: OLD[3], category: 'csam', postBody: `${BODY} edited`, now: NOW + 60000 });
    await fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: 'nobody-0001', category: 'csam', postBody: BODY, now: NOW });
  } finally {
    Object.assign(console, saved);
  }
  const rec = await caseOf(env);
  assert.deepEqual(rec.evidence, {
    post_id: POST, thread_id: THREAD, body_hash: BODY_HASH, body_hash_alg: 'hmac-sha256:forum-evidence-v1', body_hash_unavailable: false,
    post_created_at: '2026-10-14T12:00:00.000Z',
    first_reported_at: new Date(NOW).toISOString(), sealed_at: new Date(NOW).toISOString(), removed_at: new Date(NOW).toISOString(),
    later: [{ body_hash: keyedHash(`${BODY} edited`), at: new Date(NOW + 60000).toISOString() }],
  });
  assert.ok(rec.reports.every((r) => /^[0-9a-f]{64}$/.test(r.body_hash) && r.body_hash_unavailable === false));
  // The body is nowhere: not in KV, R2, a notice, an email or the console.
  const needles = ['pangolin', 'saxophone'];
  const seen = (where, s) => { for (const n of needles) assert.ok(!String(s).toLowerCase().includes(n), `the body reached ${where}`); };
  for (const [k, v] of env.SOCIAL.store) { seen('a KV key', k); seen('a KV value', v); }
  for (const [k, v] of env.USERS.store) { seen('a KV key', k); seen('a KV value', v); }
  for (const [k, o] of env.SCENES.objects) { seen('an R2 key', k); seen('an R2 object', new TextDecoder().decode(o.body)); }
  for (const m of env.mail) seen('an email', mailText(m));
  for (const line of printed) seen('console output', line);
});

test('the body hash is keyed: the same body gives the same hash, never the plain SHA-256; without JWT_SECRET the report is filed with no hash', async () => {
  assert.equal(FORUM_EVIDENCE_KEY_LABEL, 'forum-evidence-v1');
  const env = await envWith();
  const once = await forumBodyHash(env, BODY);
  assert.equal(once, BODY_HASH, 'HMAC-SHA256 under the key derived from JWT_SECRET with the forum-evidence-v1 label');
  assert.equal(await forumBodyHash(env, BODY), once, 'the same body, the same hash');
  assert.notEqual(once, PLAIN_SHA, 'not the plain SHA-256');
  assert.notEqual(once, crypto.createHmac('sha256', JWT).update(BODY, 'utf8').digest('hex'), 'not keyed by the secret itself');
  assert.notEqual(await forumBodyHash(env, `${BODY}.`), once);
  assert.notEqual(await forumBodyHash({ JWT_SECRET: 'another-secret' }, BODY), once, 'another key, another hash');
  // Two reports of the same body seal one hash and record no later one.
  await report(env, OLD[0], 'spam');
  await report(env, OLD[1], 'spam');
  const rec = await caseOf(env);
  assert.deepEqual([rec.evidence.body_hash, rec.evidence.later], [BODY_HASH, []]);
  assert.deepEqual(rec.reports.map((r) => r.body_hash), [BODY_HASH, BODY_HASH]);

  // No key: the report is filed, the pointer says so, and no unkeyed hash is written anywhere.
  const bare = await envWith();
  delete bare.JWT_SECRET;
  assert.equal(await forumBodyHash(bare, BODY), null);
  const r = await report(bare, OLD[0], 'threat');
  assert.equal(r.ok, true);
  const noKey = await caseOf(bare);
  assert.deepEqual([noKey.evidence.body_hash, noKey.evidence.body_hash_unavailable, noKey.evidence.later], [null, true, []]);
  assert.deepEqual([noKey.reports[0].body_hash, noKey.reports[0].body_hash_unavailable], [null, true]);
  for (const [, v] of bare.SOCIAL.store) assert.ok(!String(v).includes(PLAIN_SHA), 'no plain SHA-256 of the body');
});

// -- the Review Workspace -----------------------------------------------------------------------------------------------------

test('every forum case is queued as a record; the queue loads a forum case, not a listing\'s; members are told at the first report and when it is removed', async () => {
  const env = await envWith();
  await report(env, OLD[0], 'threat', { note: NOTE });
  let q = await listWorkspaceQueue(env, { kind: 'forum' });
  assert.equal(q.items.length, 1);
  assert.deepEqual([q.items[0].kind, q.items[0].post_id, q.items[0].thread_id, q.items[0].status, q.items[0].hidden, q.items[0].removed], ['forum', POST, THREAD, 'open', false, false]);
  assert.match(q.items[0].reference, REFERENCE_PATTERN);
  assert.deepEqual(q.items[0].categories, ['threat']);
  // The whole queue reads it too (no kind), next to listing cases.
  assert.deepEqual((await listWorkspaceQueue(env)).items.map((i) => i.kind), ['forum']);
  assert.ok(await env.SOCIAL.get(workspaceQueueKey('forum', POST)));

  const notices = async () => (await env.SOCIAL.list({ prefix: `notif:${MEMBER}:` })).keys.map((k) => k.name);
  assert.equal((await notices()).length, 1, 'told at the first report');
  await report(env, OLD[1], 'threat');
  assert.equal((await notices()).length, 1, 'not again for the second');
  await report(env, OLD[2], 'scam');
  const items = await Promise.all((await notices()).map(async (k) => JSON.parse(await env.SOCIAL.get(k))));
  assert.deepEqual(items.map((i) => i.event).sort(), ['forum_removed', 'forum_reported']);
  assert.ok(items.every((i) => i.post_id === POST && i.kind === 'review_workspace'));
  // Removed without a person, and still in the queue as a record a person can read without opening the post.
  q = await listWorkspaceQueue(env, { kind: 'forum' });
  assert.deepEqual([q.items[0].status, q.items[0].hidden, q.items[0].removed, q.items[0].reports], ['removed', true, true, 3]);
  assert.deepEqual(q.items[0].categories.sort(), ['scam', 'threat']);
  // Every notice and email names the case, never a reporter, the note or the body.
  const told = [...items.map((i) => JSON.stringify(i)), ...env.mail.map(mailText)].join('\n');
  assert.ok(env.mail.length >= 2, 'the member was emailed');
  for (const secret of [NOTE, 'Zebra', 'Pangolin', OLD[0], OLD[1], OLD[2]]) assert.ok(!told.includes(secret), `a notice carried ${secret}`);
  // The tool lists forum among the kinds.
  assert.ok(MODERATION_TOOLS.find((t) => t.name === 'moderation_workspace_queue').parameters.properties.kind.enum.includes('forum'));
});

test('a csam report goes to staff at once on the staff alert path, once, without the reporter, the note or the body', async () => {
  const env = await envWith();
  await report(env, NEW[0], 'csam', { note: NOTE });
  await report(env, NEW[1], 'csam');
  const staff = env.mail.filter((m) => m.to === 'security@example.com');
  assert.equal(staff.length, 1, 'staff are told once a round');
  const text = mailText(staff[0]);
  assert.match(text, /Possible CSAM reported in a forum post/);
  assert.match(text, new RegExp(POST));
  for (const secret of [NOTE, NEW[0], 'Pangolin']) assert.ok(!text.includes(secret), secret);
  // A threat report sends no staff alert.
  const env2 = await envWith();
  await report(env2, OLD[0], 'threat');
  assert.equal(env2.mail.filter((m) => m.to === 'security@example.com').length, 0);
});

// -- a person's decision ------------------------------------------------------------------------------------------------------

test('resolveForumCase records the decision and answers the action; restore is the appeal path; the next report starts a new round', async () => {
  const env = await envWith();
  for (const id of OLD.slice(0, 3)) await report(env, id, 'threat');
  assert.equal((await caseOf(env)).removed, true);
  const restored = await resolve(env, { decision: 'restore' });
  assert.deepEqual([restored.ok, restored.action], [true, 'restore']);
  let rec = await caseOf(env);
  assert.deepEqual([rec.status, rec.hidden, rec.removed, rec.round, rec.decisions.length], ['decided', false, false, 1, 1]);
  assert.equal(rec.decisions[0].by, 'admin-0001');
  assert.deepEqual((await listWorkspaceQueue(env, { kind: 'forum' })).items, [], 'a decided case leaves the queue');
  assert.deepEqual(await served(env), { hidden: false, gated: false, removed: false, locked: false, hide_reason: null });
  // The reports before the decision are settled: two new reporters do not remove it again, and an old one's repeat is a no-op.
  assert.equal((await report(env, OLD[0], 'threat')).remove, false);
  assert.equal((await report(env, OLD[3], 'threat')).remove, false);
  assert.equal((await report(env, OLD[4], 'threat')).remove, false);
  assert.equal((await report(env, OLD[5], 'threat')).remove, true, 'three in the new round remove it');
  assert.equal((await caseOf(env)).status, 'removed');

  assert.equal((await resolve(env, { decision: 'gate', category: 'adult_sexual' })).action, 'gate');
  assert.equal((await forumPostModeration({ env, postId: POST })).gated, true);
  assert.equal((await resolve(env, { decision: 'ungate' })).action, 'ungate');
  assert.equal((await forumPostModeration({ env, postId: POST })).gated, false);

  const removed = await resolve(env, { decision: 'remove', category: 'threat' });
  assert.deepEqual([removed.ok, removed.action], [true, 'remove']);
  rec = await caseOf(env);
  assert.deepEqual([rec.status, rec.removed, rec.hidden, rec.removed_by], ['removed', true, true, 'person']);
  const late = await report(env, NEW[0], 'spam');
  assert.deepEqual(late, { ok: true, reference: rec.reference, reports: 7, hide: true, gate: false, remove: true, hide_reason: 'removed', case_version: { round: rec.round, seq: rec.seq + 1 } });
  assert.deepEqual((await listWorkspaceQueue(env, { kind: 'forum' })).items, [], 'a post a person removed waits for no one');
  // Refusals.
  assert.equal((await resolve(env, { decision: 'delete' })).code, 'bad_decision');
  assert.equal((await resolve(env, { decision: 'restore', postId: 'nope' })).code, 'bad_post');
  assert.equal((await resolve(env, { decision: 'restore', postId: `${THREAD}-99` })).code, 'no_case');
  assert.equal((await resolve(env, { decision: 'restore', category: 'hate' })).code, 'bad_category');
});

test('remove is allowed only for csam, nonconsensual_sexual, personal_info, threat, scam or spam, or under a TAKE IT DOWN lock', async () => {
  assert.deepEqual([...FORUM_REMOVABLE_CATEGORIES], ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam', 'spam']);
  const env = await envWith();
  await report(env, OLD[0], 'adult_sexual');
  const adult = await resolve(env, { decision: 'remove', category: 'adult_sexual' });
  assert.deepEqual([adult.ok, adult.code], [false, 'not_removable']);
  assert.match(adult.message, /gated, not removed/);
  const none = await resolve(env, { decision: 'remove' });
  assert.deepEqual([none.ok, none.code], [false, 'category_required']);
  assert.equal((await caseOf(env)).removed, false, 'nothing was removed');
  for (const category of FORUM_REMOVABLE_CATEGORIES) {
    const e = await envWith();
    await report(e, OLD[0], 'spam');
    assert.equal((await resolve(e, { decision: 'remove', category })).action, 'remove', category);
  }
  // Under a TAKE IT DOWN lock a post may be removed with no category, and is not restored through this path.
  await env.SOCIAL.put(forumPostLockKey(POST), JSON.stringify({ reason: 'takedown', reference: 'EC-0000-0000-0000-0000-0000', locked_at: new Date(NOW).toISOString() }));
  assert.equal((await resolve(env, { decision: 'restore' })).code, 'takedown_lock');
  assert.equal((await resolve(env, { decision: 'remove' })).action, 'remove');
  assert.equal((await caseOf(env)).decisions.at(-1).under_takedown, true);
  assert.equal((await forumPostModeration({ env, postId: POST })).locked, true);
});

test('a person acts through the admin tool route; the tools are a person\'s only', async () => {
  const env = await envWith();
  await report(env, OLD[0], 'threat', { note: NOTE });
  const audits = [];
  const admin = makeToolExecutor(env, { auditLog: async (_e, action, actor, target, details) => audits.push({ action, actor, target, details }) }, { actor: 'admin', actorId: 'admin-0001' });
  const read = await admin('moderation_forum_case', { post_id: POST });
  assert.equal(read.ok, true);
  assert.equal(read.case.reports[0].note, NOTE, 'the reviewer reads the note');
  assert.equal(read.case.evidence.body_hash, BODY_HASH, 'and the evidence pointer, not the body');
  assert.ok(!JSON.stringify(read).includes('Pangolin'));
  assert.equal(read.takedown_lock, false);
  const refused = await quiet(() => admin('moderation_forum_resolve', { post_id: POST, decision: 'remove', category: 'adult_sexual', rationale: 'Explicit but adult.' }));
  assert.deepEqual([refused.ok, refused.code], [false, 'not_removable']);
  const done = await quiet(() => admin('moderation_forum_resolve', { post_id: POST, decision: 'remove', category: 'threat', rationale: 'A credible threat against a named member.' }));
  assert.deepEqual([done.ok, done.action], [true, 'remove']);
  assert.deepEqual(audits.map((a) => [a.action, a.actor, a.target]), [['moderation_forum_resolve', 'admin-0001', POST]]);
  assert.equal((await admin('moderation_forum_resolve', { post_id: POST, decision: 'restore', rationale: 'x' })).ok, false, 'a rationale is required');
  for (const actor of ['agent', 'system']) {
    const other = makeToolExecutor(env, { auditLog: async () => {} }, { actor, actorId: actor });
    assert.equal((await other('moderation_forum_resolve', { post_id: POST, decision: 'restore', rationale: 'Restoring this post now.' })).ok, false, actor);
    assert.equal((await other('moderation_forum_case', { post_id: POST })).ok, false, actor);
  }
});

// -- case history ---------------------------------------------------------------------------------------------------------------

test('the reporter sees forum_report with its status only; the author sees forum_notice only once the post is acted on, never a reporter or the note', async () => {
  const env = await envWith();
  await report(env, OLD[0], 'threat', { note: NOTE });
  let mine = await history(env, OLD[0]);
  assert.equal(mine.status, 200);
  assert.equal(mine.json.cases.length, 1);
  let e = mine.json.cases[0];
  assert.deepEqual([e.kind, e.status, e.outcome, e.category, e.post.id, e.post.thread_id], ['forum_report', 'received', null, 'threat', POST, THREAD]);
  assert.match(e.reference, REFERENCE_PATTERN);
  assert.deepEqual((await history(env, AUTHOR)).json.cases, [], 'the author sees nothing before the post is acted on');

  await report(env, OLD[1], 'threat');
  await report(env, OLD[2], 'threat');
  let theirs = (await history(env, AUTHOR)).json.cases;
  assert.equal(theirs.length, 1);
  // Hidden or removed by reports: the one generic entry, whatever the category.
  assert.deepEqual([theirs[0].kind, theirs[0].status, theirs[0].outcome], ['forum_notice', 'under_review', 'hidden']);
  assert.equal(theirs[0].headline, 'Your forum post was hidden after a report while a person reviews it.');
  assert.match(theirs[0].feedback.reasons[0].why, /support@eustress\.dev/, 'the author is told how to ask for it back');
  // Removed by reports, not by a person: the reporter learns nothing of it.
  e = (await history(env, OLD[0])).json.cases[0];
  assert.deepEqual([e.status, e.outcome], ['received', null]);

  await resolve(env, { decision: 'remove', category: 'threat' });
  theirs = (await history(env, AUTHOR)).json.cases;
  assert.deepEqual([theirs[0].status, theirs[0].outcome], ['decided', 'removed']);
  assert.match(theirs[0].headline, /A moderator removed/);
  assert.match(theirs[0].feedback.reasons[0].why, /threat of violence/);
  mine = await history(env, OLD[0]);
  e = mine.json.cases[0];
  assert.deepEqual([e.status, e.outcome], ['decided', null], 'the reporter learns it was looked at, not what happened to the author');
  assert.ok(!JSON.stringify(e).includes('remove'));

  const authorText = JSON.stringify(theirs);
  for (const secret of [NOTE, 'Zebra', 'Pangolin', OLD[0], OLD[1], OLD[2], 'reporter']) assert.ok(!authorText.includes(secret), `the author saw ${secret}`);
  // Nobody else sees either entry.
  assert.deepEqual((await history(env, OLD[3])).json.cases, []);

  // The status by reference names no person and no outcome.
  const status = await history(env, null, `/api/cases/${e.reference}`);
  assert.equal(status.status, 200);
  assert.deepEqual(Object.keys(status.json).sort(), ['decided_at', 'kind', 'received_at', 'reference', 'removed_at', 'status']);
  assert.deepEqual([status.json.kind, status.json.status], ['forum', 'decided']);
});

// The author is told nothing at all: no case history entry, no feed item, no email.
async function authorToldNothing(env, label) {
  assert.deepEqual((await history(env, AUTHOR)).json.cases, [], `${label}: no forum_notice`);
  assert.deepEqual((await env.SOCIAL.list({ prefix: `notif:${AUTHOR}:` })).keys, [], `${label}: no feed item`);
  assert.equal(env.mail.filter((m) => m.to === `${AUTHOR}@example.com`).length, 0, `${label}: no email`);
}

test('a csam report hides the post and its author sees the one generic entry at once, as for any category; a person\'s decision shows the next', async () => {
  const env = await envWith();
  await report(env, OLD[0], 'csam', { note: NOTE });
  assert.equal((await caseOf(env)).hide_reason, 'csam');
  let cases = (await history(env, AUTHOR)).json.cases;
  assert.deepEqual([cases.length, cases[0].status, cases[0].outcome, cases[0].headline], [1, 'under_review', 'hidden', 'Your forum post was hidden after a report while a person reviews it.']);
  // Removed by reports after it: the entry already shown stands.
  const shown = JSON.stringify(cases);
  await report(env, OLD[1], 'threat');
  await report(env, OLD[2], 'threat');
  assert.equal((await caseOf(env)).removed_by, 'reports');
  assert.equal(JSON.stringify((await history(env, AUTHOR)).json.cases), shown, 'a later report changes nothing shown');
  // A person removing it as csam: the generic removal entry, naming nothing.
  await resolve(env, { decision: 'remove', category: 'csam' });
  cases = (await history(env, AUTHOR)).json.cases;
  assert.deepEqual([cases[0].status, cases[0].outcome, cases[0].headline], ['under_review', 'removed', 'Your forum post was removed while a person reviews it.']);
  // A person restores it: the restored entry.
  assert.equal((await resolve(env, { decision: 'restore' })).action, 'restore');
  cases = (await history(env, AUTHOR)).json.cases;
  assert.deepEqual([cases.length, cases[0].kind, cases[0].outcome], [1, 'forum_notice', 'restored']);
  // A new csam report after that: the generic hide entry again.
  await report(env, OLD[3], 'csam');
  assert.equal((await history(env, AUTHOR)).json.cases[0].headline, 'Your forum post was hidden after a report while a person reviews it.');
  const text = JSON.stringify((await history(env, AUTHOR)).json.cases.map(({ reference, ...rest }) => rest));
  assert.ok(!/csam|minor|A1|A5|sexual/i.test(text), text);
  // The author is never emailed or put in a feed for a forum case.
  assert.deepEqual((await env.SOCIAL.list({ prefix: `notif:${AUTHOR}:` })).keys, []);
  assert.equal(env.mail.filter((m) => m.to === `${AUTHOR}@example.com`).length, 0);
});

test('the gate is told to the author as the gate', async () => {
  const env2 = await envWith();
  for (const id of OLD.slice(0, 3)) await report(env2, id, 'adult_sexual');
  const gated = (await history(env2, AUTHOR)).json.cases;
  assert.deepEqual([gated[0].kind, gated[0].status], ['forum_notice', 'under_review']);
  assert.match(gated[0].headline, /Adult content gate/);
});
