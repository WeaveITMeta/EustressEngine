// The guest chat check (moderation.mjs checkCommunityText on field guest_chat, GUEST_PLAY.md section 13, "Request for Jev",
// round 3, B-3): its own battery of eight questions, its own threshold and bands, its categories, `adult` always false, no
// metering and no per-account count whatever REVIEW_METERING says, 500 characters at most, the phone and email rule, and the
// forum's own check unchanged beside it.

import test from 'node:test';
import assert from 'node:assert/strict';
import {
  checkCommunityText, buildGuestChatQuestions, decideGuestChatAnswers, buildForumQuestions, profilePersonalInfo,
  GUEST_CHAT_FIELD, GUEST_CHAT_QUESTION_CATEGORIES, GUEST_CHAT_CATEGORIES, FORUM_QUESTION_CATEGORIES, FORUM_FIELDS, PROFILE_FIELDS,
  PROFILE_TEXT_MAX, DEFAULT_THRESHOLDS, thresholdsFrom, CONFIDENCE_BANDS, POLICY_VERSION, JEV_STATE_MAX_CHARS,
} from '../src/moderation.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '' } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).map(name => ({ name })), list_complete: true }),
  };
}

const fresh = (extra = {}) => ({ JEV_API_KEY: 'test-jev-key', SOCIAL: kv(), USERS: kv(), ...extra });
const USAGE = { input_tokens: 120, output_tokens: 14 };

// Jev's answer over `categories`: every one at `low`, with `over` setting some of them.
function body(categories, over = {}, low = 0.05) {
  const answers = {};
  for (const c of categories) answers[c] = { type: 'noul', noul: over[c] ?? low };
  return { model: 'jev-latest', answers, usage: USAGE };
}
const chatBody = (over, low) => body(GUEST_CHAT_QUESTION_CATEGORIES, over, low);
const forumBody = (over, low) => body(FORUM_QUESTION_CATEGORIES, over, low);

function jevFetch(payload = chatBody()) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url, body: JSON.parse(init.body) });
    const out = typeof payload === 'function' ? payload(calls.at(-1)) : payload;
    return new Response(JSON.stringify(out), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  fn.calls = calls;
  return fn;
}

const logged = [];
const silence = async fn => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = (...a) => logged.push(a.map(String).join(' ')); }
  try { return await fn(); } finally { Object.assign(console, saved); }
};
const check = args => silence(() => checkCommunityText(args));
const chat = (text, over, extra = {}) => {
  const fetchImpl = jevFetch(chatBody(over));
  return check({ text, field: 'guest_chat', env: fresh(), fetchImpl, ...extra }).then(r => ({ r, fetchImpl }));
};

// ── the field ───────────────────────────────────────────────────────────────

test('guest_chat is a field of the check, apart from the forum fields, and a line is at most 500 characters', () => {
  assert.equal(GUEST_CHAT_FIELD, 'guest_chat');
  assert.ok(PROFILE_FIELDS.includes('guest_chat'));
  assert.ok(!FORUM_FIELDS.includes('guest_chat'), 'guest chat is not a forum field: no forum count, no forum metering, no adult gate');
  assert.equal(PROFILE_TEXT_MAX.guest_chat, 500);
  assert.ok(PROFILE_TEXT_MAX.guest_chat <= JEV_STATE_MAX_CHARS);
  assert.deepEqual([...GUEST_CHAT_CATEGORIES], ['csam', 'nonconsensual_sexual', 'threat', 'scam', 'adult_sexual', 'self_disclosure', 'contact_seeking', 'sexual_minor_context', 'personal_info', 'other']);
});

// ── the battery ─────────────────────────────────────────────────────────────

const SELF_DISCLOSURE = 'Does the line share personal information about the writer or someone they know that could identify or locate them: a full or last name, age or birthday, school, grade or teacher, town, city, neighbourhood, street or landmark near home, a parent\'s workplace, a photo or a description of themselves, or a username, handle, server or account on any platform?';
const CONTACT_SEEKING = 'Does the line ask another player for, or offer, a way to talk or meet outside this game: another app or platform, a private chat, a phone call or video call, an address, or an in-person meeting; or ask their age, where they live, their school, whether they are alone, or whether their parents are around; or ask them to keep something secret or move the conversation elsewhere?';
const SEXUAL_MINOR_CONTEXT = 'Does the line, from anyone, sexualise, flirt with, compliment the body of, or ask for pictures from another player, who may be a child?';

test('a guest_chat check sends the guest battery, all eight questions, and not the forum battery', async () => {
  const { r, fetchImpl } = await chat('nice build, want to race?');
  assert.equal(r.verdict, 'pass');
  assert.equal(fetchImpl.calls.length, 1);
  const sent = fetchImpl.calls[0].body;
  assert.deepEqual(Object.keys(sent.questions), ['csam', 'nonconsensual_sexual', 'threat', 'scam', 'adult_sexual', 'self_disclosure', 'contact_seeking', 'sexual_minor_context']);
  assert.deepEqual(Object.keys(sent.questions), [...GUEST_CHAT_QUESTION_CATEGORIES]);
  for (const forumOnly of ['personal_info', 'spam']) assert.ok(!(forumOnly in sent.questions), `the forum's ${forumOnly} question is not asked of guest chat`);
  assert.deepEqual(sent.state, { field: 'guest_chat', text: 'nice build, want to race?' });
  for (const q of Object.values(sent.questions)) {
    assert.equal(q.type, 'noul');
    assert.match(q.instructions, /guests whose age is unknown, and any of them may be a child/);
    assert.doesNotMatch(q.instructions, /Eustress Forums|The forums allow free speech/, 'no forum preamble in a chat question');
  }
  // The three new questions, word for word.
  assert.ok(sent.questions.self_disclosure.instructions.endsWith(` ${SELF_DISCLOSURE}`));
  assert.ok(sent.questions.contact_seeking.instructions.endsWith(` ${CONTACT_SEEKING}`));
  assert.ok(sent.questions.sexual_minor_context.instructions.endsWith(` ${SEXUAL_MINOR_CONTEXT}`));
  // The five shared ones are the forum's own words after the preamble, and the same criteria.
  const forum = buildForumQuestions();
  const guest = buildGuestChatQuestions();
  for (const c of ['csam', 'nonconsensual_sexual', 'threat', 'scam', 'adult_sexual']) {
    const forumTail = forum[c].instructions.slice(forum[c].instructions.indexOf(' Code ') >= 0 ? forum[c].instructions.indexOf(' Code ') : forum[c].instructions.indexOf(' Part '));
    assert.ok(guest[c].instructions.endsWith(forumTail), `${c} is asked as the forum asks it`);
    assert.deepEqual(guest[c].criteria, forum[c].criteria);
  }
  assert.deepEqual(JSON.parse(JSON.stringify(sent.questions)), JSON.parse(JSON.stringify(guest)), 'the battery sent is buildGuestChatQuestions()');
});

test('a forum_post check still sends the forum battery beside it', async () => {
  const fetchImpl = jevFetch(forumBody());
  const r = await check({ text: 'Bridge notes', field: 'forum_post', env: fresh(), fetchImpl });
  assert.equal(r.verdict, 'pass');
  assert.deepEqual(Object.keys(fetchImpl.calls[0].body.questions), [...FORUM_QUESTION_CATEGORIES]);
});

// ── thresholds and bands ────────────────────────────────────────────────────

test('guest_chat_refuse is 0.50: 0.5 refuses and 0.49 passes, in every category; the band is borderline below 0.85 and high at or above', async () => {
  assert.equal(DEFAULT_THRESHOLDS.guest_chat_refuse, 0.5);
  assert.equal(DEFAULT_THRESHOLDS.guest_chat_high, 0.85);
  for (const c of GUEST_CHAT_QUESTION_CATEGORIES) {
    const below = (await chat('a line', { [c]: 0.49 })).r;
    assert.deepEqual([below.verdict, below.category, below.confidence_band], ['pass', null, null], `${c} at 0.49`);
    for (const [p, band] of [[0.5, 'borderline'], [0.84, 'borderline'], [0.849, 'borderline'], [0.85, 'high'], [0.99, 'high'], [1, 'high']]) {
      const r = (await chat('a line', { [c]: p })).r;
      assert.deepEqual([r.verdict, r.category, r.reason, r.confidence, r.confidence_band], ['refuse', c, 'model', p, band], `${c} at ${p}`);
      assert.ok(CONFIDENCE_BANDS.includes(r.confidence_band));
      assert.equal(r.policy_version, POLICY_VERSION);
      assert.ok(r.message && !r.message.includes('a line'));
    }
  }
  // The forum's 0.85 is not this field's threshold: 0.6 in a forum post's scam question passes, in a chat line it refuses.
  const forum = await check({ text: 'a line', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody({ scam: 0.6 })) });
  assert.equal(forum.verdict, 'pass');
  assert.equal((await chat('a line', { scam: 0.6 })).r.verdict, 'refuse');
});

test('the thresholds are set as the forum\'s are, from MODERATION_THRESHOLDS', async () => {
  const env = fresh({ MODERATION_THRESHOLDS: JSON.stringify({ guest_chat_refuse: 0.3, guest_chat_high: 0.6 }) });
  assert.equal(thresholdsFrom(env).guest_chat_refuse, 0.3);
  const run = async p => check({ text: 'a line', field: 'guest_chat', env, fetchImpl: jevFetch(chatBody({ contact_seeking: p })) });
  assert.equal((await run(0.29)).verdict, 'pass');
  assert.deepEqual([(await run(0.3)).verdict, (await run(0.3)).confidence_band], ['refuse', 'borderline']);
  assert.equal((await run(0.6)).confidence_band, 'high');
});

test('the highest category decides, a tie goes to the earlier one, and csam outranks every other at the same score', async () => {
  assert.equal((await chat('a line', { self_disclosure: 0.7, contact_seeking: 0.9 })).r.category, 'contact_seeking');
  assert.equal((await chat('a line', { sexual_minor_context: 0.9, csam: 0.9 })).r.category, 'csam');
  assert.equal((await chat('a line', { contact_seeking: 0.6, self_disclosure: 0.6 })).r.category, 'self_disclosure');
});

// ── categories ──────────────────────────────────────────────────────────────

test('each category maps through to the answer: verdict, category and reason, never the text', async () => {
  const text = 'CANARY-7f3a line';
  for (const c of GUEST_CHAT_QUESTION_CATEGORIES) {
    const { r } = await chat(text, { [c]: 0.9 });
    assert.equal(r.verdict, 'refuse');
    assert.equal(r.category, c);
    assert.ok(GUEST_CHAT_CATEGORIES.includes(r.category));
    assert.equal(r.reason, 'model');
    assert.ok(!JSON.stringify(r).includes('CANARY-7f3a'), 'the answer never carries the text');
  }
  for (const [line, category, reason] of [['', 'other', 'empty'], ['x'.repeat(501), 'other', 'too_long'], ['555 123 4567', 'personal_info', 'phone']]) {
    const { r } = await chat(line);
    assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', category, reason]);
    assert.ok(GUEST_CHAT_CATEGORIES.includes(r.category));
  }
  assert.ok(!logged.some(l => l.includes('CANARY-7f3a')), 'the text never reaches the console');
});

// ── adult ───────────────────────────────────────────────────────────────────

test('adult is always false for guest chat: adult content is refused, not gated', async () => {
  for (const p of [0.99, 0.85, 0.6, 0.5, 0.49, 0.05]) {
    const { r } = await chat('a line', { adult_sexual: p });
    assert.equal(r.adult, false, `adult_sexual at ${p}`);
    assert.equal(r.verdict, p >= 0.5 ? 'refuse' : 'pass');
    if (p >= 0.5) assert.equal(r.category, 'adult_sexual');
  }
  for (const line of ['', 'x'.repeat(501), 'ann@mail.com']) assert.equal((await chat(line)).r.adult, false);
  // The decision itself says false too, not only the answer.
  const decided = decideGuestChatAnswers({ answers: Object.fromEntries(GUEST_CHAT_QUESTION_CATEGORIES.map(c => [c, { type: 'noul', p: c === 'adult_sexual' ? 0.99 : 0.01 }])) });
  assert.deepEqual(decided, { verdict: 'refuse', category: 'adult_sexual', confidence: 0.99, adult: false, confidence_band: 'high' });
  // The forum's gate is unchanged: adult_sexual at 0.6 in a forum post passes and marks it adult.
  const forum = await check({ text: 'a post', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody({ adult_sexual: 0.6 })) });
  assert.deepEqual([forum.verdict, forum.adult], ['pass', true]);
});

// ── metering and counts ─────────────────────────────────────────────────────

test('no modcost:run, modcost:forum or forumcheck: key is written for guest chat, with REVIEW_METERING unset, on or off', async () => {
  for (const metering of [undefined, 'on', 'off']) {
    for (const over of [{}, { contact_seeking: 0.9 }]) {
      const env = fresh(metering === undefined ? {} : { REVIEW_METERING: metering });
      const fetchImpl = jevFetch(chatBody(over));
      const r = await check({ text: 'a line', field: 'guest_chat', env, fetchImpl, accountId: 'acct-host' });
      assert.equal(fetchImpl.calls.length, 1, 'Jev was asked');
      assert.ok(['pass', 'refuse'].includes(r.verdict));
      const keys = [...env.SOCIAL.m.keys()];
      assert.deepEqual(keys.filter(k => k.startsWith('modcost:') || k.startsWith('forumcheck:') || k.startsWith('profilecheck:')), [], `REVIEW_METERING ${metering}`);
      assert.equal(env.SOCIAL.m.size + env.USERS.m.size, 0, 'nothing at all is written for a chat line');
    }
  }
  // The control: the same call on forum_post writes both, so this test can fail.
  const env = fresh();
  await check({ text: 'a line', field: 'forum_post', env, fetchImpl: jevFetch(forumBody()), accountId: 'acct-host' });
  const keys = [...env.SOCIAL.m.keys()];
  assert.ok(keys.some(k => k.startsWith('forumcheck:acct-host:')), 'a forum post is counted');
  assert.ok(keys.some(k => k.startsWith('modcost:run:')), 'a forum post is metered');
  assert.ok(keys.includes('modcost:forum:acct-host'), 'on the forum ledger');
});

test('a guest_chat check needs no SOCIAL binding, since it counts nothing', async () => {
  const r = await check({ text: 'a line', field: 'guest_chat', env: { JEV_API_KEY: 'k' }, fetchImpl: jevFetch(chatBody()), accountId: 'acct-host' });
  assert.equal(r.verdict, 'pass');
});

// ── length ──────────────────────────────────────────────────────────────────

test('500 characters is the most a line may be, counted after cleaning', async () => {
  assert.equal((await chat('a'.repeat(500))).r.verdict, 'pass');
  const long = (await chat('a'.repeat(501))).r;
  assert.deepEqual([long.verdict, long.category, long.reason, long.confidence_band, long.adult], ['refuse', 'other', 'too_long', 'high', false]);
  assert.match(long.message, /at most 500 characters/);
  assert.equal((await chat('\u{1F680}'.repeat(500))).r.verdict, 'pass', 'code points, not UTF-16 units');
  assert.equal((await chat('\u{1F680}'.repeat(501))).r.reason, 'too_long');
  const folded = await chat(`  ${'a'.repeat(250)}  \n\t  ${'b'.repeat(249)} `);
  assert.equal(folded.r.verdict, 'pass', 'whitespace folds to one space: 500 characters');
  assert.equal(folded.fetchImpl.calls[0].body.state.text, `${'a'.repeat(250)} ${'b'.repeat(249)}`);
  const none = await chat('a'.repeat(501));
  assert.equal(none.fetchImpl.calls.length, 0, 'a line too long is never sent to Jev');
});

// ── the deterministic rule ──────────────────────────────────────────────────

test('a phone number or an email address refuses as personal_info by the profilePersonalInfo rule, before Jev', async () => {
  for (const [text, kind] of [['555-123-4567', 'phone'], ['call 555 123 4567 now', 'phone'], ['(555) 123.4567', 'phone'], ['ann@mail.com', 'email'], ['ann at mail dot com', 'email']]) {
    assert.equal(profilePersonalInfo(text), kind);
    const { r, fetchImpl } = await chat(text);
    assert.deepEqual([r.verdict, r.category, r.reason, r.confidence_band, r.adult], ['refuse', 'personal_info', kind, 'high', false], text);
    assert.equal(fetchImpl.calls.length, 0, 'Jev is not asked');
    // The same rule refuses the same text in a forum post.
    const forum = await check({ text, field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) });
    assert.deepEqual([forum.verdict, forum.category, forum.reason], ['refuse', 'personal_info', kind]);
  }
  assert.equal((await chat('1,000,000 coins')).r.verdict, 'pass');
});

// ── failures never pass ─────────────────────────────────────────────────────

test('a malformed answer, a missing new question or no Jev answers unavailable, never pass', async () => {
  const missing = body(GUEST_CHAT_QUESTION_CATEGORIES.filter(c => c !== 'contact_seeking'));
  assert.equal((await check({ text: 'a line', field: 'guest_chat', env: fresh(), fetchImpl: jevFetch(missing) })).reason, 'malformed');
  // The forum's answers do not satisfy the guest battery.
  const forumOnly = await check({ text: 'a line', field: 'guest_chat', env: fresh(), fetchImpl: jevFetch(forumBody()) });
  assert.deepEqual([forumOnly.verdict, forumOnly.reason], ['unavailable', 'malformed']);
  const unconfigured = await check({ text: 'a line', field: 'guest_chat', env: { SOCIAL: kv() } });
  assert.deepEqual([unconfigured.verdict, unconfigured.reason, unconfigured.confidence_band, unconfigured.adult], ['unavailable', 'not_configured', null, false]);
  const failing = async () => new Response('nope', { status: 500 });
  assert.equal((await check({ text: 'a line', field: 'guest_chat', env: fresh(), fetchImpl: failing })).verdict, 'unavailable');
  assert.equal(decideGuestChatAnswers(null), null);
});

// ── the forum beside it ─────────────────────────────────────────────────────

test('forum_post is unchanged: csam at 0.6 is a borderline refusal, scam at 0.6 passes, adult is gated', async () => {
  const run = over => check({ text: 'a post', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody(over)) });
  const csam = await run({ csam: 0.6 });
  assert.deepEqual([csam.verdict, csam.category, csam.confidence_band, csam.adult], ['refuse', 'csam', 'borderline', false]);
  const high = await run({ personal_info: 0.9 });
  assert.deepEqual([high.verdict, high.category, high.confidence_band], ['refuse', 'personal_info', 'high']);
  assert.deepEqual([(await run({ scam: 0.6 })).verdict, (await run({ adult_sexual: 0.9 })).adult], ['pass', true]);
  assert.equal((await check({ text: 'p'.repeat(10_000), field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass');
});
