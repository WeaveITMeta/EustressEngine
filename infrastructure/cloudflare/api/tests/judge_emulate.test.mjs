// The judge emulator behind the Eustress MCP's judge_verdict (McKale, 2026-11-14: "The judge skill needs to now be built
// into the Eustress MCP included in Studio. That way agents can check if their space is compliant from the start."):
// POST /api/moderation/emulate runs the review's own rules (emulateReview) over a listing and a judge answer the caller
// wrote, stores nothing and calls no model, and is limited per address.
import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { emulateReview, CODE_TESTS, POLICY_VERSION, POLICY_HASH_ANCHORED, EMULATE_MAX_BYTES } from '../src/moderation.mjs';

const STATEMENT = [
  'Category: Game',
  'Objective: Return the ball with your paddle and reach 7 points first.',
  'Lesson: Reaction time and spatial awareness: read the ball off the walls and meet it in time.',
  'Real-world link: Practises reaction time and spatial awareness through play.',
].join('\n');
const LISTING = { name: 'Pong', description: `A simple game of pong.\n${STATEMENT}`, tags: ['arcade', 'reaction-time'], is_public: true };
const judge = (over = {}) => ({
  policy_version: POLICY_VERSION, policy_hash: POLICY_HASH_ANCHORED, verdict: 'publish', articles: [],
  tests: Object.fromEntries(CODE_TESTS.map(k => [k, { score: 2, evidence: `${k}: PongServer.server.luau bounceOff lines 215-238 and the seven-segment score` }])),
  statement_check: 'matches', rating_floor: 'all_ages', descriptors: ['players interact'], is_game: true, is_fiction: false,
  featured_candidate: false, spatial_evidence: ['a 44 by 28 table with two paddles, a ball and two walls'],
  rationale: 'A skill game: T3 and T4(i).', confidence: 0.85, flagged_for_review: false, suggested_edit_to_publish: null,
  suggested_statement: null, suggested_tags: [], ...over,
});
const post = async (worker, env, body, ip = '203.0.113.7') => worker.fetch(new Request('https://api.eustress.dev/api/moderation/emulate', {
  method: 'POST', headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': ip }, body: typeof body === 'string' ? body : JSON.stringify(body),
}), env, ctx);

test('emulateReview: a listed skill game and a missing statement read exactly as Studio would show them', () => {
  const ok = emulateReview({ listing: LISTING, judge: judge() });
  assert.equal(ok.emulated, true);
  assert.equal(ok.policy_version, POLICY_VERSION);
  assert.equal(ok.outcome, 'approve');
  assert.equal(ok.view.status, 'listed');
  const missing = emulateReview({ listing: { ...LISTING, description: 'A simple game of pong.' }, judge: judge({ statement_check: 'missing' }) });
  assert.equal(missing.outcome, 'changes_requested');
  assert.deepEqual(missing.view.reasons.map(r => r.category), ['statement_missing']);
});

test('emulateReview: the answer may be text or an object; a malformed answer says why', () => {
  assert.equal(emulateReview({ listing: LISTING, judge: JSON.stringify(judge()) }).outcome, 'approve');
  const bad = emulateReview({ listing: LISTING, judge: { verdict: 'publish' } });
  assert.equal(bad.error, 'judge_malformed');
  assert.ok(bad.detail, 'the detail tells the agent what to fix');
});

test('POST /api/moderation/emulate: the Worker serves the emulator publicly, stores nothing, and refuses bad bodies', async () => {
  const worker = await loadWorker();
  const env = fakeEnv();
  const res = await post(worker, env, { listing: LISTING, judge: judge() });
  assert.equal(res.status, 200);
  const body = await res.json();
  assert.equal(body.outcome, 'approve');
  assert.equal(body.view.status, 'listed');
  // Nothing written anywhere.
  for (const ns of ['USERS', 'SOCIAL', 'INVENTORY', 'AUDIT_LOG', 'SCREENING', 'TELEMETRY']) {
    const listed = await env[ns].list({});
    assert.equal(listed.keys.length, 0, `${ns} untouched`);
  }
  assert.equal((await post(worker, env, { listing: LISTING, judge: { verdict: 'publish' } })).status, 422);
  assert.equal((await post(worker, env, 'not json')).status, 400);
  assert.equal((await post(worker, env, { listing: LISTING })).status, 400);
  assert.equal((await post(worker, env, 'x'.repeat(EMULATE_MAX_BYTES + 1))).status, 413);
  const get = await worker.fetch(new Request('https://api.eustress.dev/api/moderation/emulate'), env, ctx);
  assert.equal(get.status, 405);
});

test('POST /api/moderation/emulate: limited per address', async () => {
  const worker = await loadWorker();
  const env = fakeEnv();
  const seen = [];
  env.AUTH_RATE_LIMITER = { limit: async ({ key }) => { seen.push(key); return { success: seen.length <= 1 }; } };
  assert.equal((await post(worker, env, { listing: LISTING, judge: judge() }, '198.51.100.9')).status, 200);
  assert.equal((await post(worker, env, { listing: LISTING, judge: judge() }, '198.51.100.9')).status, 429);
  assert.deepEqual(seen, ['judge-emulate:198.51.100.9', 'judge-emulate:198.51.100.9']);
});
