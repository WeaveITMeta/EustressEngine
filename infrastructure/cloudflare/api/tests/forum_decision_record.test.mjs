// The forum's record of a person's latest decision, forum-decision:{postId} (src/moderation.mjs afterForumDecision), and the
// decision pushed to the forum's applier. Forums round 10, from Website: R9-11, the record carries a TTL so the forum's
// decisions pass does not grow without bound; R5-3 (Security's zz_rev_a_1b), on a clock that moves once a second two decisions
// in one instant looked like one, so the push and the record carry the decision's case_version ({ round, seq }) and category,
// and the forum orders by (decided_at, case_version).
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { fakeKv, fakeBucket } from './helpers/load_worker.mjs';
import { fileForumReport, resolveForumCase, forumDecisionKey, registerForumDecisionApplier, FORUM_DECISION_TTL_SECONDS } from '../src/moderation.mjs';

const THREAD = 'f'.repeat(24);
const POST = `${THREAD}-1`;
const AUTHOR = 'author-r10';
const ADMIN = 'admin-r10';
const REPORTER = 'reporter-r10';

async function environment() {
  const env = { USERS: fakeKv(), SOCIAL: fakeKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), SCENES: fakeBucket(), JWT_SECRET: 'test-secret-r10' };
  for (const id of [AUTHOR, ADMIN, REPORTER]) await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: id, created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  // Every put with its options, so a test can read the TTL a write carried.
  env.puts = [];
  const put = env.SOCIAL.put.bind(env.SOCIAL);
  env.SOCIAL.put = async (key, value, opts) => { env.puts.push({ key, value, opts }); return put(key, value, opts); };
  return env;
}
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };

// The clock stands still: every new Date() and Date.now() is one instant, as a clock that moves once a second looks.
async function atOneInstant(fn) {
  const Real = Date;
  const fixed = Real.now();
  class Frozen extends Real {
    constructor(...a) { if (a.length) super(...a); else super(fixed); }
    static now() { return fixed; }
  }
  globalThis.Date = Frozen;
  try { return await fn(); } finally { globalThis.Date = Real; }
}

test('R9-11: every write of forum-decision: carries a TTL of 90 days or more', async () => {
  const env = await environment();
  await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTER, category: 'spam', postBody: 'text' }));
  for (const decision of ['gate', 'ungate', 'restore']) assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision, reviewerId: ADMIN }))).ok, true, decision);
  const writes = env.puts.filter((p) => p.key === forumDecisionKey(POST));
  assert.equal(writes.length, 3);
  assert.ok(FORUM_DECISION_TTL_SECONDS >= 90 * 86400);
  for (const w of writes) assert.equal(w.opts?.expirationTtl, FORUM_DECISION_TTL_SECONDS, 'the record expires');
});

test('R5-3 (zz_rev_a_1b): two decisions in the same instant carry distinct, increasing case versions on the push and the record', async () => {
  const env = await environment();
  await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: AUTHOR, reporterId: REPORTER, category: 'spam', postBody: 'text' }));
  const pushed = [];
  registerForumDecisionApplier(async (a) => { pushed.push({ decidedAt: a.decidedAt, caseVersion: a.caseVersion, category: a.category, action: a.action }); });
  const records = [];
  try {
    await atOneInstant(async () => {
      for (const [decision, category] of [['gate', 'adult_sexual'], ['ungate', null]]) {
        const r = await quiet(() => resolveForumCase({ env, postId: POST, decision, category, reviewerId: ADMIN }));
        assert.equal(r.ok, true, JSON.stringify(r));
        records.push(JSON.parse(await env.SOCIAL.get(forumDecisionKey(POST))));
      }
    });
  } finally {
    registerForumDecisionApplier(null);
  }
  assert.equal(pushed.length, 2);
  assert.equal(pushed[0].decidedAt, pushed[1].decidedAt, 'one instant: the time alone cannot tell them apart');
  const later = (a, b) => b.round > a.round || (b.round === a.round && b.seq > a.seq);
  for (const v of [...pushed.map((p) => p.caseVersion), ...records.map((r) => r.case_version)]) assert.ok(Number.isInteger(v?.round) && Number.isInteger(v?.seq), JSON.stringify(v));
  assert.ok(later(pushed[0].caseVersion, pushed[1].caseVersion), 'the push: the second is later');
  assert.ok(later(records[0].case_version, records[1].case_version), 'the record: the second is later');
  assert.deepEqual(records.map((r) => r.case_version), pushed.map((p) => p.caseVersion), 'the push and the record agree');
  assert.deepEqual([pushed[0].category, pushed[1].category, records[0].category, records[1].category], ['adult_sexual', null, 'adult_sexual', null]);
  assert.deepEqual(records.map((r) => r.action), ['gate', 'ungate']);
});
