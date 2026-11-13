// Listing-review latency for staff (src/review_latency.mjs; Technical Document 4.3): the statistics, the window boundaries,
// the empty case, the administrator-only route and its aggregate-only answer, the scan caps, the clock moderation.mjs writes,
// and the staff digest's section.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { fakeKv, fakeEnv, loadWorker, mintToken, ctx } from './helpers/load_worker.mjs';
import {
  summarize, percentile, samplesOf, aggregate, reviewLatency, digestLines, handleReviewLatencyRoute, WINDOWS,
} from '../src/review_latency.mjs';
import { sendStaffDigest, alertStaffForumReport, renderMail } from '../src/takedown.mjs';
import { makeToolExecutor, loadCase, POLICY_HASH_ANCHORED } from '../src/moderation.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

const NOW = Date.parse('2026-11-12T12:00:00.000Z');
const iso = (t) => new Date(t).toISOString();
const SEC = 1000;
const H = 3600 * SEC;
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };

// A KV that keeps list metadata and pages like the real one.
function metaKv() {
  const kv = fakeKv();
  const meta = new Map();
  const put = kv.put.bind(kv);
  kv.put = async (key, value, opts = {}) => { if (opts.metadata) meta.set(key, opts.metadata); else meta.delete(key); return put(key, value, opts); };
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    const names = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort().filter((k) => !cursor || k > cursor);
    const page = names.slice(0, limit);
    const more = names.length > limit;
    return { keys: page.map((name) => ({ name, ...(meta.has(name) ? { metadata: meta.get(name) } : {}) })), list_complete: !more, ...(more ? { cursor: page.at(-1) } : {}) };
  };
  return kv;
}

function caseOf({ submitted, started, decided, outcome = 'approve', appeal = null }) {
  return {
    review_clock: { submitted_at: iso(submitted), started_at: iso(started), ...(decided ? { decided_at: iso(decided) } : {}) },
    decision: { outcome }, ...(appeal ? { appeal } : {}),
  };
}
async function seed(env, id, status, rec, updated = NOW) {
  await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status, ...rec }));
  await env.SOCIAL.put(`modq:${status}:${id}`, iso(updated), { metadata: { u: iso(updated) } });
}

// ---- the statistics -------------------------------------------------------------------------------------------------

test('summarize: count, median, p90 and max, in seconds, from a known set', () => {
  const s = summarize([1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map((n) => n * SEC));
  assert.deepEqual(s, { count: 10, median_seconds: 5.5, p90_seconds: 9, max_seconds: 10 });
  assert.deepEqual(summarize([7 * SEC]), { count: 1, median_seconds: 7, p90_seconds: 7, max_seconds: 7 });
  assert.equal(percentile([1, 2, 3, 4, 5], 0.9), 5, 'nearest rank');
  assert.equal(summarize([30 * SEC, 1 * SEC, 2 * SEC]).max_seconds, 30, 'order does not matter');
  assert.equal(summarize([NaN, -5, 4 * SEC]).count, 1, 'a bad value is not a sample');
});

test('the empty case: no cases means zero counts and null numbers, never a throw or a fake zero', async () => {
  const env = { SOCIAL: metaKv() };
  const r = await reviewLatency(env, { now: NOW });
  const none = { count: 0, median_seconds: null, p90_seconds: null, max_seconds: null };
  assert.deepEqual(r.in_review_to_decided.all.last_24h, none);
  assert.deepEqual(r.submitted_to_in_review.all.last_7d, none);
  assert.deepEqual(r.appeal_to_outcome.all.last_7d, none);
  assert.equal(r.scan.truncated, false);
  assert.deepEqual(digestLines(r).map(([, v]) => v), ['24h: no samples; 7d: no samples', '24h: no samples; 7d: no samples', '24h: no samples; 7d: no samples']);
});

// ---- the windows ----------------------------------------------------------------------------------------------------

test('window boundaries: a sample counts by when it ended; exactly 24 hours old is in, one second older is out of the 24 hours', () => {
  const at = (ageMs) => samplesOf(caseOf({ submitted: NOW - ageMs - 60 * SEC, started: NOW - ageMs - 50 * SEC, decided: NOW - ageMs }));
  const samples = [
    ...at(0), ...at(WINDOWS.last_24h), ...at(WINDOWS.last_24h + SEC), ...at(WINDOWS.last_7d), ...at(WINDOWS.last_7d + SEC),
    ...samplesOf(caseOf({ submitted: NOW, started: NOW, decided: NOW + SEC })), // ends in the future: counted nowhere
  ];
  const r = aggregate(samples, NOW).in_review_to_decided.all;
  assert.equal(r.last_24h.count, 2, 'age 0 and exactly 24 hours');
  assert.equal(r.last_7d.count, 4, 'adds the one 1 second past 24 hours and the one at exactly 7 days');
});

test('decided_at is the end of the review: a 20 hour review decided 1 hour ago is in the 24 hours', () => {
  const rec = caseOf({ submitted: NOW - 21 * H, started: NOW - 21 * H + SEC, decided: NOW - H });
  const r = aggregate(samplesOf(rec), NOW).in_review_to_decided.all;
  assert.equal(r.last_24h.count, 1);
  assert.equal(r.last_24h.max_seconds, 20 * 3600 - 1);
});

test('the three measures and their groups, from one case each', () => {
  const approve = caseOf({ submitted: NOW - 100 * SEC, started: NOW - 90 * SEC, decided: NOW - 30 * SEC, outcome: 'approve' });
  const reject = caseOf({ submitted: NOW - 100 * SEC, started: NOW - 95 * SEC, decided: NOW - 5 * SEC, outcome: 'reject', appeal: { at: iso(NOW - 3600 * SEC), resolved_at: iso(NOW - 600 * SEC), status: 'overturned' } });
  const pendingAppeal = { appeal: { at: iso(NOW - 60 * SEC), status: 'pending' } };
  const samples = [approve, reject, pendingAppeal].flatMap(samplesOf);
  const r = aggregate(samples, NOW);
  assert.equal(r.submitted_to_in_review.all.last_24h.count, 2);
  assert.equal(r.submitted_to_in_review.all.last_24h.median_seconds, 7.5);
  assert.equal(r.in_review_to_decided.approve.last_24h.median_seconds, 60);
  assert.equal(r.in_review_to_decided.reject.last_24h.median_seconds, 90);
  assert.equal(r.in_review_to_decided.quarantine, undefined, 'a group with no sample is left out');
  assert.equal(r.appeal_to_outcome.overturned.last_24h.median_seconds, 3000);
  assert.equal(r.appeal_to_outcome.all.last_24h.count, 1, 'a pending appeal is not an outcome');
});

test('a case with no clock gives no sample', () => {
  assert.deepEqual(samplesOf({ status: 'approved', created_at: iso(NOW), decision: { outcome: 'approve' } }), []);
  assert.deepEqual(samplesOf(null), []);
  const undecided = samplesOf(caseOf({ submitted: NOW - 9 * SEC, started: NOW - 4 * SEC }));
  assert.deepEqual(undecided.map((s) => s.metric), ['submitted_to_in_review'], 'an undecided case gives the first measure only');
});

// ---- the scan -------------------------------------------------------------------------------------------------------

test('the scan skips cases untouched for 7 days without reading them, reads the rest, and never goes past its caps', async () => {
  const env = { SOCIAL: metaKv() };
  await seed(env, 'new1', 'approved', caseOf({ submitted: NOW - 40 * SEC, started: NOW - 30 * SEC, decided: NOW - 10 * SEC }));
  await seed(env, 'old1', 'approved', caseOf({ submitted: NOW - 9 * 86400000, started: NOW - 9 * 86400000, decided: NOW - 9 * 86400000 + 5 * SEC }), NOW - 9 * 86400000);
  const reads = [];
  const get = env.SOCIAL.get.bind(env.SOCIAL);
  env.SOCIAL.get = async (k, ...a) => { reads.push(k); return get(k, ...a); };
  const r = await reviewLatency(env, { now: NOW });
  assert.deepEqual(reads, ['modcase:new1'], 'the old case was not read');
  assert.equal(r.scan.skipped_old, 1);
  assert.equal(r.in_review_to_decided.all.last_24h.count, 1);

  for (let i = 0; i < 12; i++) await seed(env, `c${String(i).padStart(2, '0')}`, 'rejected', caseOf({ submitted: NOW - 20 * SEC, started: NOW - 10 * SEC, decided: NOW - SEC, outcome: 'reject' }));
  reads.length = 0;
  const capped = await reviewLatency(env, { now: NOW, maxReads: 5 });
  assert.equal(reads.length, 5, 'at most maxReads case reads');
  assert.equal(capped.scan.truncated, true);
  const paged = await reviewLatency(env, { now: NOW, maxPages: 3 });
  assert.equal(paged.scan.truncated, true, 'at most maxPages list calls');
});

// ---- the route ------------------------------------------------------------------------------------------------------

const json = (v, status = 200, h = {}) => new Response(JSON.stringify(v), { status, headers: { ...h, 'Content-Type': 'application/json' } });
const ask = (env, { admin, method = 'GET', path = '/api/staff/review-latency' } = {}) => {
  const url = new URL(`https://api.example.com${path}`);
  return handleReviewLatencyRoute(new Request(url, { method }), url, env, {}, { json, requireAdmin: async () => (admin ? 'admin-1' : null) });
};

test('the route refuses a non-admin with 403 and reads nothing; a wrong method and an unrelated path are not served', async () => {
  const env = { SOCIAL: metaKv() };
  let touched = 0;
  env.SOCIAL.list = async () => { touched += 1; return { keys: [], list_complete: true }; };
  const refused = await ask(env, { admin: false });
  assert.equal(refused.status, 403);
  assert.equal(touched, 0, 'no storage was read for a refused caller');
  assert.equal((await ask(env, { admin: true, method: 'POST' })).status, 405);
  assert.equal(await ask(env, { admin: true, path: '/api/staff/other' }), null);
  assert.equal((await ask(env, { admin: true })).status, 200, 'the same request from an administrator is answered');
});

test('the route answers an administrator with aggregates only: no listing id, author id, name or reference anywhere in the body', async () => {
  const env = { SOCIAL: metaKv() };
  const rec = { ...caseOf({ submitted: NOW - 40 * SEC, started: NOW - 30 * SEC, decided: NOW - 10 * SEC }), author_id: 'AUTHOR-SECRET-77', reference: 'EC-REF-SECRET', name: 'Harbor Secret Town' };
  await seed(env, 'sim-secret-id-9', 'approved', rec, NOW);
  const realNow = Date.now;
  Date.now = () => NOW;
  let res;
  try { res = await ask(env, { admin: true }); } finally { Date.now = realNow; }
  assert.equal(res.status, 200);
  const text = await res.text();
  for (const secret of ['AUTHOR-SECRET-77', 'EC-REF-SECRET', 'Harbor Secret Town', 'sim-secret-id-9']) assert.ok(!text.includes(secret), secret);
  const body = JSON.parse(text);
  assert.equal(body.in_review_to_decided.all.last_24h.count, 1);
  assert.equal(body.in_review_to_decided.all.last_24h.median_seconds, 20);
});

// ---- moderation.mjs writes the clock --------------------------------------------------------------------------------

test('moderation.mjs records the clock at the decision: decided_at is stamped on a case that has a clock, and the queue key carries updated_at', async () => {
  const SIM = 'a1b2c3d4-0000-4000-8000-000000000e11';
  const env = { USERS: fakeKv(), SOCIAL: metaKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), JWT_SECRET: 's', JEV_API_KEY: 'j', MODERATION_THRESHOLDS: '{"audit_rate":0}' };
  const started = iso(Date.now() - 5000);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'X', author_id: 'au', is_public: true, moderation: { status: 'held' } }));
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'au', status: 'held', created_at: started, history: [], decision: { reasons: [] }, review_clock: { submitted_at: started, started_at: started } }));
  const deps = { auditLog: async () => {}, json, cors: {}, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED };
  const run = makeToolExecutor(env, deps, { actor: 'admin', actorId: 'admin-1' });
  await quiet(() => run('moderation_reject', { sim_id: SIM, lane: 'quality', category: 'B1', rationale: 'A rationale that is at least twenty characters long.' }, { actor: 'admin', actorId: 'admin-1' }));
  const rec = await loadCase(env, SIM);
  assert.equal(rec.status, 'rejected');
  assert.ok(Date.parse(rec.review_clock.decided_at) >= Date.parse(started), 'decided_at was written');
  assert.equal(rec.review_clock.started_at, started, 'and the rest of the clock is kept');
  const listed = await env.SOCIAL.list({ prefix: 'modq:rejected:' });
  assert.ok(listed.keys[0].metadata?.u, 'the queue key carries updated_at for the scan');
});

// ---- the digest -----------------------------------------------------------------------------------------------------

test('renderMail staff_digest carries the review latency rows, and leaves them out when none are passed', () => {
  const rows = [['Review latency: in review to decided', '24h: 3 done, median 4s, p90 9s, max 12s; 7d: no samples']];
  const text = JSON.stringify(renderMail('staff_digest', { total: 1, entries: [], more: 0, at: iso(NOW), latency: rows }));
  assert.match(text, /Review latency: in review to decided/);
  assert.match(text, /median 4s, p90 9s, max 12s/);
  assert.ok(!JSON.stringify(renderMail('staff_digest', { total: 1, entries: [], more: 0, at: iso(NOW) })).includes('Review latency'));
});

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }
async function digestEnv() {
  const env = { USERS: metaKv(), SOCIAL: metaKv(), AUDIT_LOG: fakeKv(), KYC_STATUS: fakeKv(), JWT_SECRET: 's', __EmailMessage: StubMessage, mail: [] };
  await env.USERS.put('user:adm', JSON.stringify({ id: 'adm', username: 'adm', email: 'adm@example.com', created_at: '2025-01-01T00:00:00.000Z' }));
  await env.USERS.put('admin:adm', '1');
  return env;
}
async function waitingAlert(env, ref) {
  env.EMAIL = { send: async () => { throw new Error('mail down'); } };
  await quiet(() => alertStaffForumReport(env, { reference: ref, post_id: `p-${ref}`, thread_id: 't', at: iso(NOW), queue_url: 'https://x' }));
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
}
const bodies = (env) => env.mail.map((m) => { const at = m.raw.indexOf('Content-Type: text/plain; charset=utf-8'); const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0]; return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8'); });

test('the staff digest that is actually sent includes the latency section computed from the cases', async () => {
  const env = await digestEnv();
  await seed(env, 'd1', 'approved', caseOf({ submitted: NOW - 100 * SEC, started: NOW - 90 * SEC, decided: NOW - 60 * SEC }), NOW);
  await waitingAlert(env, 'EC-LAT-1');
  const r = await quiet(() => sendStaffDigest(env, { now: NOW }));
  assert.ok(r.digests >= 1, 'a digest was sent');
  const digest = bodies(env).find((t) => /Review latency: in review to decided/.test(t));
  assert.ok(digest, 'the digest has the section');
  assert.match(digest, /24h: 1 done, median 30s, p90 30s, max 30s/);
});

test('a latency read that fails never stops the digest', async () => {
  const env = await digestEnv();
  await waitingAlert(env, 'EC-LAT-2');
  env.SOCIAL.list = async () => { throw new Error('KV is down'); };
  const r = await quiet(() => sendStaffDigest(env, { now: NOW }));
  assert.ok(r.digests >= 1, 'the digest still went');
  assert.ok(!bodies(env).some((t) => /Review latency/.test(t)), 'without the section');
});

// ---- wired into the Worker ------------------------------------------------------------------------------------------

test('GET /api/staff/review-latency on the real Worker: an administrator is answered, anyone else and a signed-out caller are refused', async () => {
  const worker = await loadWorker();
  const ADMIN = '33333333-3333-4333-8333-333333333333';
  const OTHER = '44444444-4444-4444-8444-444444444444';
  const env = { ...fakeEnv() };
  await env.USERS.put('user:' + ADMIN, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'staff', email: 'staff.person@example.com' }));
  await env.USERS.put('admin:' + ADMIN, '1');
  const get = (token) => worker.fetch(new Request('https://api.eustress.dev/api/staff/review-latency', { headers: token ? { Authorization: 'Bearer ' + token } : {} }), env, ctx);
  const ok = await get(mintToken(ADMIN));
  assert.equal(ok.status, 200);
  assert.equal((await ok.json()).unit, 'seconds');
  assert.equal((await get(mintToken(OTHER))).status, 403);
  assert.equal((await get(null)).status, 403);
});
