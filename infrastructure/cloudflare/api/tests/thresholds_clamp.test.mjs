// MODERATION_THRESHOLDS is clamped (src/moderation.mjs thresholdsFrom, thresholdValueOk): a value outside [0, 1] for a
// probability or a rate (every battery's: listing, forum, profile, guest_chat), a negative count or time, or anything that is
// not a finite number falls back to its default, never "refuse nothing". Security, on 11ec9bf9.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import {
  checkCommunityText, GUEST_CHAT_QUESTION_CATEGORIES, FORUM_QUESTION_CATEGORIES, DEFAULT_THRESHOLDS, thresholdsFrom, thresholdValueOk,
} from '../src/moderation.mjs';

function kv() {
  const m = new Map();
  return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); }, delete: async (k) => { m.delete(k); }, list: async ({ prefix = '' } = {}) => ({ keys: [...m.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })), list_complete: true }) };
}
const envWith = (thresholds) => ({ JEV_API_KEY: 'test-jev-key', SOCIAL: kv(), USERS: kv(), MODERATION_THRESHOLDS: typeof thresholds === 'string' ? thresholds : JSON.stringify(thresholds) });
const answers = (categories, over) => ({ model: 'jev-latest', answers: Object.fromEntries(categories.map((c) => [c, { type: 'noul', noul: over[c] ?? 0.05 }])), usage: { input_tokens: 1, output_tokens: 1 } });
const jev = (payload) => async () => new Response(JSON.stringify(payload), { status: 200, headers: { 'Content-Type': 'application/json' } });
const quiet = async (fn) => { const s = {}; for (const k of ['log', 'info', 'warn', 'error', 'debug']) { s[k] = console[k]; console[k] = () => {}; } try { return await fn(); } finally { Object.assign(console, s); } };
const chat = (env, p) => quiet(() => checkCommunityText({ text: 'a line', field: 'guest_chat', env, fetchImpl: jev(answers(GUEST_CHAT_QUESTION_CATEGORIES, { contact_seeking: p })) }));
const forum = (env, p) => quiet(() => checkCommunityText({ text: 'a post', field: 'forum_post', env, fetchImpl: jev(answers(FORUM_QUESTION_CATEGORIES, { scam: p })) }));

test('guest_chat_refuse set to 2, -1 or "x" still refuses at 0.50 and passes at 0.49', async () => {
  for (const bad of [2, -1, 'x', null, 1.0000001]) {
    const env = envWith({ guest_chat_refuse: bad });
    assert.equal((await quiet(() => thresholdsFrom(env))).guest_chat_refuse, DEFAULT_THRESHOLDS.guest_chat_refuse, JSON.stringify(bad));
    assert.equal((await chat(env, 0.5)).verdict, 'refuse', `${JSON.stringify(bad)}: 0.5 refuses`);
    assert.equal((await chat(env, 0.49)).verdict, 'pass', `${JSON.stringify(bad)}: 0.49 passes`);
  }
});

test('a forum threshold set to 5 uses its default; a valid one in [0, 1] is taken', async () => {
  const env = envWith({ forum_refuse: 5, forum_borderline: 5 });
  const t = await quiet(() => thresholdsFrom(env));
  assert.deepEqual([t.forum_refuse, t.forum_borderline], [DEFAULT_THRESHOLDS.forum_refuse, DEFAULT_THRESHOLDS.forum_borderline]);
  assert.equal((await forum(env, 0.9)).verdict, 'refuse', 'scam at 0.9 still refuses at the default 0.85');
  assert.equal((await quiet(() => thresholdsFrom(envWith({ forum_refuse: 0.6 })))).forum_refuse, 0.6);
  // R-6: a refusal threshold is strictly inside (0, 1): at 1 nothing is refused, at 0 everything is.
  for (const edge of [0, 1]) assert.equal((await quiet(() => thresholdsFrom(envWith({ guest_chat_refuse: edge })))).guest_chat_refuse, DEFAULT_THRESHOLDS.guest_chat_refuse, String(edge));
  assert.equal((await quiet(() => thresholdsFrom(envWith({ forum_refuse: 1 })))).forum_refuse, DEFAULT_THRESHOLDS.forum_refuse);
  assert.equal((await quiet(() => thresholdsFrom(envWith({ guest_chat_refuse: 0.01 })))).guest_chat_refuse, 0.01);
  // A rate may be 0 or 1.
  assert.deepEqual([(await quiet(() => thresholdsFrom(envWith({ audit_rate: 0 })))).audit_rate, (await quiet(() => thresholdsFrom(envWith({ audit_rate: 1 })))).audit_rate], [0, 1]);
});

test('every probability field of every battery is clamped, and a count or a time may not be negative', async () => {
  const probabilities = Object.entries(DEFAULT_THRESHOLDS).filter(([, d]) => d >= 0 && d <= 1).map(([k]) => k);
  for (const k of ['hard_quarantine', 'csam_hold', 'profile_refuse', 'profile_review', 'forum_refuse', 'forum_adult', 'guest_chat_refuse', 'guest_chat_high', 'audit_rate']) assert.ok(probabilities.includes(k), k);
  const t = await quiet(() => thresholdsFrom(envWith(Object.fromEntries(probabilities.map((k) => [k, 7])))));
  for (const k of probabilities) assert.equal(t[k], DEFAULT_THRESHOLDS[k], k);
  const counts = await quiet(() => thresholdsFrom(envWith({ lease_seconds: -1, forum_hide_reporters: -3, judge_timeout_ms: 1e3 })));
  assert.deepEqual([counts.lease_seconds, counts.forum_hide_reporters, counts.judge_timeout_ms], [DEFAULT_THRESHOLDS.lease_seconds, DEFAULT_THRESHOLDS.forum_hide_reporters, 1e3]);
  assert.equal(thresholdValueOk('guest_chat_refuse', Number.NaN), false);
  assert.equal(thresholdValueOk('not_a_field', 0.5), false);
  assert.deepEqual(await quiet(() => thresholdsFrom(envWith('not json'))), { ...DEFAULT_THRESHOLDS });
});

test('R-6: lease_seconds is at least 30, and each high threshold stays at or above its lower one (else both defaults)', async () => {
  const t = (x) => quiet(() => thresholdsFrom(envWith(x)));
  assert.equal((await t({ lease_seconds: 29 })).lease_seconds, DEFAULT_THRESHOLDS.lease_seconds);
  assert.equal((await t({ lease_seconds: 30 })).lease_seconds, 30);
  const crossed = await t({ guest_chat_refuse: 0.9, guest_chat_high: 0.6 });
  assert.deepEqual([crossed.guest_chat_refuse, crossed.guest_chat_high], [DEFAULT_THRESHOLDS.guest_chat_refuse, DEFAULT_THRESHOLDS.guest_chat_high], 'the guest chat pair');
  const forum = await t({ forum_borderline: 0.9, forum_refuse: 0.6 });
  assert.deepEqual([forum.forum_borderline, forum.forum_refuse], [DEFAULT_THRESHOLDS.forum_borderline, DEFAULT_THRESHOLDS.forum_refuse], 'the forum pair');
  const profile = await t({ profile_review: 0.9 });
  assert.deepEqual([profile.profile_review, profile.profile_refuse], [DEFAULT_THRESHOLDS.profile_review, DEFAULT_THRESHOLDS.profile_refuse], 'the profile pair, a lower one raised past the default higher one');
  const ok = await t({ guest_chat_refuse: 0.4, guest_chat_high: 0.4 });
  assert.deepEqual([ok.guest_chat_refuse, ok.guest_chat_high], [0.4, 0.4], 'equal is allowed');
});
