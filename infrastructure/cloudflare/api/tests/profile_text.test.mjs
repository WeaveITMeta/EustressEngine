// The profile text check (moderation.mjs checkProfileText): the deterministic refusals, the staff words, the Jev battery and
// its thresholds, the cost cap, metering under kind `profile`, the rule that a failure never passes, and the rule that the
// text never reaches console output, KV, R2, the metering record or the answer.

import test from 'node:test';
import assert from 'node:assert/strict';
import {
  checkProfileText, buildProfileQuestions, buildJevQuestions, decideProfileAnswers, profilePersonalInfo, staffWordIn, profileCheckKey, profileTextCleaned,
  PROFILE_TEXT_MAX, PROFILE_STAFF_WORDS, PROFILE_QUESTION_CATEGORIES, PROFILE_CATEGORIES, DEFAULT_THRESHOLDS, POLICY_VERSION, JEV_ENDPOINT,
  checkCommunityText, forumCheckKey, buildForumQuestions, decideForumAnswers, forumPostCleaned, PROFILE_FIELDS, FORUM_FIELDS,
  FORUM_QUESTION_CATEGORIES, FORUM_CATEGORIES, FORUM_REFUSE_CATEGORIES, FORUM_BORDERLINE_CATEGORIES, CONFIDENCE_BANDS, JEV_STATE_MAX_CHARS,
} from '../src/moderation.mjs';
import { costLedgerKey, meterKind, createReviewMeter, METERED_KINDS } from '../src/review_costs.mjs';

// Characters written by their code points, so no tool that decodes an escape in this file can change what a test means.
const cp = (...points) => String.fromCodePoint(...points);
const ZWSP = cp(0x200b);
const ZWJ = cp(0x200d);
const BOM = cp(0xfeff);
const WORD_JOINER = cp(0x2060);
const EM_DASH = cp(0x2014);
// ASCII letters and digits in their full-width forms (U+FF01 to U+FF5E), which NFKC folds back.
const fullWidth = s => [...s].map(c => cp(c.codePointAt(0) + 0xfee0)).join('');

// ── fakes ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    puts: [],
    get: async k => m.get(k) ?? null,
    put: async (k, v, opts) => { m.set(k, typeof v === 'string' ? v : String(v)); kvPuts.push({ k, v: String(v), opts: JSON.stringify(opts ?? null) }); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '' } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).map(name => ({ name })), list_complete: true }),
  };
}
let kvPuts = [];

function r2() {
  const m = new Map();
  return {
    m,
    put: async (key, body, opts = {}) => { m.set(key, { body: typeof body === 'string' ? body : Buffer.from(body).toString('utf8'), opts: JSON.stringify(opts) }); return { key }; },
    get: async key => (m.has(key) ? { key, text: async () => m.get(key).body } : null),
    head: async key => (m.has(key) ? { key } : null),
    delete: async key => { m.delete(key); },
    list: async () => ({ objects: [...m.keys()].map(key => ({ key })) }),
  };
}

const fresh = (extra = {}) => ({ JEV_API_KEY: 'test-jev-key', SOCIAL: kv(), USERS: kv(), SCENES: r2(), ...extra });

// Jev's answer: every category at `low`, with `over` setting some of them.
const USAGE = { input_tokens: 120, output_tokens: 14 };
function jevBody(over = {}, low = 0.05) {
  const answers = {};
  for (const c of PROFILE_QUESTION_CATEGORIES) answers[c] = { type: 'noul', noul: over[c] ?? low };
  return { model: 'jev-latest', answers, usage: USAGE };
}

// The same for the forum battery.
function forumBody(over = {}, low = 0.05) {
  const answers = {};
  for (const c of FORUM_QUESTION_CATEGORIES) answers[c] = { type: 'noul', noul: over[c] ?? low };
  return { model: 'jev-latest', answers, usage: USAGE };
}

// A fetch that answers with `body` (or `make(request)`), and remembers every request.
function jevFetch(body = jevBody(), { status = 200 } = {}) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url, init, body: JSON.parse(init.body) });
    const payload = typeof body === 'function' ? body(calls.at(-1)) : body;
    return new Response(typeof payload === 'string' ? payload : JSON.stringify(payload), { status, headers: { 'Content-Type': 'application/json' } });
  };
  fn.calls = calls;
  return fn;
}

// A fetch that never answers until its signal aborts.
function hangingFetch() {
  const calls = [];
  const fn = (url, init) => {
    calls.push({ url, init });
    return new Promise((_, reject) => init.signal.addEventListener('abort', () => reject(init.signal.reason)));
  };
  fn.calls = calls;
  return fn;
}

const silence = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};

const check = (args) => silence(() => checkProfileText(args));

// ── deterministic refusals ──────────────────────────────────────────────────

test('an empty display name refuses under `other`; an empty bio passes; neither calls a model', async () => {
  // Empty once format characters are removed counts as empty.
  for (const text of ['', '   \n\t ', ZWSP + ZWJ + BOM, null, undefined, 42]) {
    const name = jevFetch();
    const r = await check({ text, field: 'display_name', env: fresh(), fetchImpl: name, accountId: 'acct-e' });
    assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'other', 'empty'], `${JSON.stringify(text)} as a display name`);
    assert.equal(r.policy_version, POLICY_VERSION);
    assert.ok(r.message);
    const bio = jevFetch();
    const env = fresh();
    const b = await check({ text, field: 'bio', env, fetchImpl: bio, accountId: 'acct-e' });
    assert.deepEqual([b.verdict, b.category, b.reason, b.message], ['pass', null, 'empty', null], `${JSON.stringify(text)} as a bio`);
    assert.equal(name.calls.length + bio.calls.length, 0);
    assert.equal(env.SOCIAL.m.size, 0, 'an empty bio is not counted against the cap or metered');
  }
});

test('length is counted after NFKC and with format characters removed, as Website\'s route counts it', async () => {
  // 32 ZERO WIDTH SPACEs (Cf) inside a 30-letter name: 62 code points before, 30 after.
  const hidden = 'ab'.repeat(15).split('').join(ZWSP) + ZWSP.repeat(3);
  assert.equal([...hidden].length, 62);
  assert.equal([...profileTextCleaned(hidden)].length, 30);
  const fetchImpl = jevFetch();
  assert.equal((await check({ text: hidden, field: 'display_name', env: fresh(), fetchImpl })).verdict, 'pass');
  assert.equal(fetchImpl.calls[0].body.state.text, 'ab'.repeat(15), 'Jev reads the cleaned text');
  // NFKC can lengthen: U+FDFA (one code point) is 18 code points under NFKC. Two of them are 36 > 32.
  const ligature = cp(0xfdfa, 0xfdfa);
  assert.equal([...ligature].length, 2);
  assert.equal([...profileTextCleaned(ligature)].length, 36);
  const long = await check({ text: ligature, field: 'display_name', env: fresh(), fetchImpl: jevFetch() });
  assert.deepEqual([long.verdict, long.reason], ['refuse', 'too_long']);
  // A format character cannot split a staff word or a link.
  assert.equal((await check({ text: `Ad${ZWSP}min`, field: 'display_name', env: fresh(), fetchImpl: jevFetch() })).reason, 'staff_word');
  assert.equal((await check({ text: `www${WORD_JOINER}.site`, field: 'display_name', env: fresh(), fetchImpl: jevFetch() })).reason, 'link');
});

test('the length limits are Website\'s (1 to 32 and 0 to 280 characters), and over them refuses under `other`', async () => {
  assert.deepEqual({ ...PROFILE_TEXT_MAX }, { display_name: 32, bio: 280, forum_title: 120, forum_post: 10_000, guest_chat: 500 });
  const cases = [
    ['display_name', 'a'.repeat(32), 'pass'],
    ['display_name', 'a'.repeat(33), 'refuse'],
    ['display_name', `  ${'a'.repeat(15)}    \n   ${'b'.repeat(16)}  `, 'pass'], // folds to 32
    ['display_name', '\u{1F680}'.repeat(32), 'pass'], // code points, not UTF-16 units
    ['display_name', '\u{1F680}'.repeat(33), 'refuse'],
    ['bio', 'word '.repeat(56).trim(), 'pass'], // 279
    ['bio', 'b'.repeat(280), 'pass'],
    ['bio', 'b'.repeat(281), 'refuse'],
  ];
  for (const [field, text, want] of cases) {
    const fetchImpl = jevFetch();
    const r = await check({ text, field, env: fresh(), fetchImpl });
    assert.equal(r.verdict, want, `${field} of ${[...text].length} characters`);
    if (want === 'refuse') {
      assert.deepEqual([r.category, r.reason], ['other', 'too_long']);
      assert.equal(fetchImpl.calls.length, 0);
    } else assert.equal(fetchImpl.calls.length, 1);
  }
});

test('a phone number or an email address refuses under `personal_info` in either field, by the tag rule', async () => {
  const hits = [
    ['555-123-4567', 'phone'], ['Call me on 555 123 4567 any time', 'phone'], ['(555) 123.4567', 'phone'], ['+1 555 1234567', 'phone'],
    ['ann@mail.com', 'email'], ['write to ann@mail.example.org please', 'email'], ['ann at mail dot com', 'email'], ['reach me: ann at my mail dot co', 'email'],
    [fullWidth('5551234'), 'phone'], // full-width digits fold under NFKC
  ];
  for (const [text, kind] of hits) {
    assert.equal(profilePersonalInfo(text), kind, text);
    for (const field of ['display_name', 'bio']) {
      if ([...text].length > PROFILE_TEXT_MAX[field]) continue;
      const fetchImpl = jevFetch();
      const r = await check({ text, field, env: fresh(), fetchImpl });
      assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'personal_info', kind], `${text} in ${field}`);
      assert.equal(fetchImpl.calls.length, 0);
    }
  }
  // Numbers and words that are not contact details reach Jev.
  for (const text of ['1,000,000 plays and counting', 'Look at this and tell me', 'I live at home in the uk', 'WW2 builds, 1939-1945', 'Born 10/09/1990', 'Open 9:00-17:00', '@ann is my friend']) {
    assert.equal(profilePersonalInfo(text), null, text);
    const fetchImpl = jevFetch();
    const r = await check({ text, field: 'bio', env: fresh(), fetchImpl });
    assert.equal(r.verdict, 'pass', text);
    assert.equal(fetchImpl.calls.length, 1);
  }
});

test('a link, an invite or a handle in a display name refuses under `spam`; in a bio a link goes to Jev', async () => {
  for (const text of ['http://x.io', 'see https stuff', 'HTTPS://BUILDER.DEV', 'www.mysite', `${fullWidth('www')}.mysite`, 'discord.gg/abc', 'join discord.com/invite/abc', '@ann', 'Builder @ann']) {
    const fetchImpl = jevFetch();
    const r = await check({ text, field: 'display_name', env: fresh(), fetchImpl });
    assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'spam', 'link'], text);
    assert.equal(fetchImpl.calls.length, 0);
  }
  for (const text of ['Httpster', 'Wwwizard', 'Discord fan']) {
    const r = await check({ text, field: 'display_name', env: fresh(), fetchImpl: jevFetch() });
    assert.equal(r.verdict, 'pass', text);
  }
  const fetchImpl = jevFetch();
  const bio = await check({ text: 'My portfolio: https://example.dev and www.example.dev', field: 'bio', env: fresh(), fetchImpl });
  assert.equal(bio.verdict, 'pass');
  assert.equal(fetchImpl.calls.length, 1, 'a link in a bio is Jev\'s call');
});

// ── the staff words ─────────────────────────────────────────────────────────

test('the staff words are the documented list, matched whole, in any case, with 0, 1 and 3 read as o, i and e', async () => {
  assert.deepEqual([...PROFILE_STAFF_WORDS], ['eustress', 'admin', 'moderator', 'mod', 'staff', 'official', 'support']);
  const hits = [
    ['Eustress', 'eustress'], ['EUSTRESS team', 'eustress'], ['3ustr3ss', 'eustress'], ['Eustr3ss Builder', 'eustress'],
    ['Admin', 'admin'], ['Adm1n', 'admin'], ['admin1', 'admin'], ['xx_admin_xx', 'admin'], ['EustressAdmin', 'eustress'], ['SiteAdmin', 'admin'],
    ['Moderator', 'moderator'], ['M0d3rat0r', 'moderator'], ['Mod Ann', 'mod'], ['m0d', 'mod'],
    ['Staff', 'staff'], ['ann.staff', 'staff'], ['0fficial', 'official'], ['Official Ann', 'official'], ['0ff1c1al', 'official'],
    ['Support', 'support'], ['Supp0rt Desk', 'support'], [fullWidth('ADMIN'), 'admin'],
  ];
  for (const [text, word] of hits) {
    assert.equal(staffWordIn(text), word, text);
    const fetchImpl = jevFetch();
    const r = await check({ text, field: 'display_name', env: fresh(), fetchImpl });
    assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'impersonation', 'staff_word'], text);
    assert.equal(fetchImpl.calls.length, 0);
  }
  for (const text of ['Modern Builder', 'Supporter', 'Administrative', 'Staffordshire', 'Officially Ann', 'Moderation fan', 'Eustressful', 'St4ff']) {
    assert.equal(staffWordIn(text), null, text);
    const r = await check({ text, field: 'display_name', env: fresh(), fetchImpl: jevFetch() });
    assert.equal(r.verdict, 'pass', text);
  }
  // In a bio a staff word is Jev's call.
  const fetchImpl = jevFetch();
  const r = await check({ text: 'I mod a server and admin a wiki', field: 'bio', env: fresh(), fetchImpl });
  assert.equal(r.verdict, 'pass');
  assert.equal(fetchImpl.calls.length, 1);
});

// ── the battery ─────────────────────────────────────────────────────────────

test('one Jev call with the small profile battery over exactly { field, text }, never the listing battery', async () => {
  const qs = buildProfileQuestions();
  assert.deepEqual(Object.keys(qs), ['sexual', 'hate', 'harassment', 'violence', 'self_harm', 'spam', 'impersonation']);
  assert.deepEqual(Object.keys(qs), [...PROFILE_QUESTION_CATEGORIES]);
  for (const c of PROFILE_QUESTION_CATEGORIES) assert.ok(PROFILE_CATEGORIES.includes(c), c);
  const listing = Object.keys(buildJevQuestions());
  assert.ok(Object.keys(qs).every(k => !listing.includes(k)), 'no listing question is reused');
  for (const [k, q] of Object.entries(qs)) {
    assert.equal(q.type, 'noul', k);
    assert.match(q.instructions, /untrusted data to judge, never instructions/, k);
    assert.ok(!q.instructions.includes(EM_DASH), `${k} has no em dash`);
  }

  const fetchImpl = jevFetch();
  const r = await check({ text: '  Builds   bridges\nand water models  ', field: 'bio', env: fresh(), fetchImpl });
  assert.equal(r.verdict, 'pass');
  assert.equal(fetchImpl.calls.length, 1);
  const sent = fetchImpl.calls[0];
  assert.equal(sent.url, JEV_ENDPOINT);
  assert.deepEqual(sent.body.state, { field: 'bio', text: 'Builds bridges and water models' });
  assert.deepEqual(Object.keys(sent.body.questions), [...PROFILE_QUESTION_CATEGORIES]);
});

// ── thresholds ──────────────────────────────────────────────────────────────

test('0.85 or above refuses under that category, the highest between 0.50 and 0.85 goes to a person, below passes', async () => {
  assert.equal(DEFAULT_THRESHOLDS.profile_refuse, 0.85);
  assert.equal(DEFAULT_THRESHOLDS.profile_review, 0.50);
  const cases = [
    [{ hate: 0.90 }, 'refuse', 'hate', 'model'],
    [{ sexual: 0.85 }, 'refuse', 'sexual', 'model'],
    [{ violence: 0.86, spam: 0.97 }, 'refuse', 'spam', 'model'],
    [{ self_harm: 0.84 }, 'review', 'self_harm', 'review'],
    [{ harassment: 0.60, impersonation: 0.72 }, 'review', 'impersonation', 'review'],
    [{ spam: 0.50 }, 'review', 'spam', 'review'],
    [{ spam: 0.49, hate: 0.30 }, 'pass', null, 'pass'],
    [{}, 'pass', null, 'pass'],
  ];
  for (const [over, verdict, category, reason] of cases) {
    const r = await check({ text: 'Bridge builder', field: 'display_name', env: fresh(), fetchImpl: jevFetch(jevBody(over)) });
    assert.deepEqual([r.verdict, r.category, r.reason], [verdict, category, reason], JSON.stringify(over));
    if (verdict !== 'pass') assert.equal(r.confidence, Math.max(...Object.values(over)));
    assert.equal(r.policy_version, POLICY_VERSION);
    assert.equal(r.message === null, verdict === 'pass');
  }
  // Tunable through MODERATION_THRESHOLDS like every other threshold.
  const tuned = fresh({ MODERATION_THRESHOLDS: JSON.stringify({ profile_refuse: 0.95, profile_review: 0.70 }) });
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env: tuned, fetchImpl: jevFetch(jevBody({ hate: 0.90 })) })).verdict, 'review');
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env: fresh({ MODERATION_THRESHOLDS: tuned.MODERATION_THRESHOLDS }), fetchImpl: jevFetch(jevBody({ hate: 0.65 })) })).verdict, 'pass');
});

// ── failures never pass ─────────────────────────────────────────────────────

test('a timeout, a provider error, a missing key and a malformed answer each answer `unavailable`, never `pass`', async () => {
  const expectUnavailable = (r, reason, label) => {
    assert.deepEqual([r.verdict, r.category, r.reason], ['unavailable', null, reason], label);
    assert.equal(r.policy_version, POLICY_VERSION);
    assert.ok(r.message, label);
  };
  const args = { text: 'Bridge builder', field: 'bio' };

  const hang = hangingFetch();
  expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: hang, timeoutMs: 40 }), 'timeout', 'timeout');
  assert.equal(hang.calls.length, 1);
  // The Workers AI binding has no bound of its own; the check bounds it.
  expectUnavailable(await check({ ...args, env: fresh({ JEV_API_KEY: undefined, AI: { run: () => new Promise(() => {}) } }), timeoutMs: 40 }), 'timeout', 'binding timeout');

  for (const status of [500, 400, 401, 503])
    expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: jevFetch({ error: 'boom' }, { status }) }), 'provider_error', `http ${status}`);
  expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: async () => { throw new TypeError('network down'); } }), 'provider_error', 'network');
  expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: jevFetch('not json') }), 'provider_error', 'not json');
  expectUnavailable(await check({ ...args, env: fresh({ JEV_API_KEY: undefined, AI: { run: async () => { throw new Error('binding down'); } } }) }), 'provider_error', 'binding error');

  const noKey = jevFetch();
  expectUnavailable(await check({ ...args, env: fresh({ JEV_API_KEY: undefined }), fetchImpl: noKey }), 'not_configured', 'missing key');
  expectUnavailable(await check({ ...args, env: fresh({ JEV_API_KEY: '' }), fetchImpl: noKey }), 'not_configured', 'empty key');
  expectUnavailable(await check({ ...args, env: undefined, fetchImpl: noKey }), 'not_configured', 'no env');
  assert.equal(noKey.calls.length, 0);

  const missing = jevBody();
  delete missing.answers.spam;
  const malformed = [
    ['a category missing', missing],
    ['no answers', { model: 'jev-latest', usage: USAGE }],
    ['a string probability', jevBody({ hate: '0.1' })],
    ['a probability over 1', jevBody({ hate: 1.5 })],
    ['a negative probability', jevBody({ violence: -0.2 })],
    ['a choice answer', { ...jevBody(), answers: { ...jevBody().answers, sexual: { type: 'choice', choice: 'no' } } }],
    ['null', 'null'],
    ['a list', '[]'],
  ];
  for (const [label, body] of malformed) expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: jevFetch(body) }), 'malformed', label);
  assert.equal(decideProfileAnswers(null), null);

  // A bad field or account id is a caller's mistake, and still never a pass.
  expectUnavailable(await check({ ...args, field: 'username', env: fresh(), fetchImpl: jevFetch() }), 'bad_field', 'bad field');
  expectUnavailable(await check({ ...args, env: fresh(), fetchImpl: jevFetch(), accountId: 'a b/c' }), 'bad_account', 'bad account');
  // A KV that cannot count is not a pass either.
  const broken = fresh({ SOCIAL: { get: async () => { throw new Error('kv down'); }, put: async () => {} } });
  expectUnavailable(await check({ ...args, env: broken, fetchImpl: jevFetch(), accountId: 'acct-1' }), 'cap_unavailable', 'kv down');
  expectUnavailable(await check({ ...args, env: fresh({ SOCIAL: undefined }), fetchImpl: jevFetch(), accountId: 'acct-1' }), 'cap_unavailable', 'no kv');
});

// ── the text goes nowhere ───────────────────────────────────────────────────

test('the text never reaches console output, KV, R2, the metering record or the answer, on any path', async () => {
  const CANARY = 'Quokka xylophone canary';
  const saved = {};
  const printed = [];
  for (const k of ['log', 'info', 'warn', 'error', 'debug', 'trace']) {
    saved[k] = console[k];
    console[k] = (...a) => { printed.push(a.map(x => (x instanceof Error ? `${x.message}\n${x.stack}` : typeof x === 'string' ? x : JSON.stringify(x))).join(' ')); };
  }
  const env = fresh();
  kvPuts = [];
  const answers = [];
  const sentBodies = [];
  const watch = fetchImpl => { const fn = async (u, i) => { sentBodies.push(String(i.body)); return fetchImpl(u, i); }; return fn; };
  // A provider whose error text repeats the request, as some do.
  const echoing = async (u, init) => new Response(`bad request: ${init.body}`, { status: 400 });
  const scenarios = [
    ['pass', 'bio', jevFetch()],
    ['refuse', 'bio', jevFetch(jevBody({ hate: 0.99 }))],
    ['review', 'display_name', jevFetch(jevBody({ spam: 0.6 }))],
    ['provider error echoing the text', 'bio', echoing],
    ['malformed answer echoing the text', 'bio', jevFetch(req => ({ model: 'jev-latest', answers: { note: req.body.state.text }, usage: USAGE }))],
    ['usage echoing the text', 'bio', jevFetch(req => ({ ...jevBody(), usage: { input_tokens: 5, note: req.body.state.text } }))],
    ['model name echoing the text', 'bio', jevFetch(req => ({ ...jevBody(), model: req.body.state.text }))],
    ['timeout', 'bio', hangingFetch()],
  ];
  try {
    for (const [, field, fetchImpl] of scenarios)
      answers.push(await checkProfileText({ text: CANARY, field, env, fetchImpl: watch(fetchImpl), timeoutMs: 60, accountId: 'acct-canary' }));
    // Deterministic refusals carry the canary too.
    for (const [text, field] of [[`${CANARY} 555 123 4567`, 'bio'], [`${CANARY} @x`, 'display_name'], [`Admin ${CANARY}`, 'display_name'], [`${CANARY} `.repeat(20), 'bio']])
      answers.push(await checkProfileText({ text, field, env, fetchImpl: jevFetch(), accountId: 'acct-canary' }));
    // Past the cap.
    await env.SOCIAL.put(profileCheckKey('acct-full', new Date().toISOString().slice(0, 10)), '50');
    answers.push(await checkProfileText({ text: CANARY, field: 'bio', env, fetchImpl: jevFetch(), accountId: 'acct-full' }));
    // Not configured.
    answers.push(await checkProfileText({ text: CANARY, field: 'bio', env: { SOCIAL: env.SOCIAL, SCENES: env.SCENES } }));
    // The forum fields, through the forum battery, on every path.
    const forumScenarios = [
      ['forum_post', jevFetch(forumBody())],
      ['forum_title', jevFetch(forumBody({ threat: 0.99 }))],
      ['forum_post', jevFetch(forumBody({ csam: 0.6 }))],
      ['forum_post', echoing],
      ['forum_post', jevFetch(req => ({ model: 'jev-latest', answers: { note: req.body.state.text }, usage: USAGE }))],
      ['forum_post', jevFetch(req => ({ ...forumBody(), usage: { input_tokens: 5, note: req.body.state.text } }))],
      ['forum_post', jevFetch(req => ({ ...forumBody(), model: req.body.state.text }))],
      ['forum_post', hangingFetch()],
    ];
    for (const [field, fetchImpl] of forumScenarios)
      answers.push(await checkCommunityText({ text: `${CANARY}\n\n${CANARY}`, field, env, fetchImpl: watch(fetchImpl), timeoutMs: 60, accountId: 'acct-canary' }));
    for (const [text, field] of [[`${CANARY}\n555 123 4567`, 'forum_post'], [`${CANARY} www.x.io`, 'forum_title'], [`${CANARY}\n`.repeat(500), 'forum_post'], [`${CANARY} ann@mail.com`, 'forum_title']])
      answers.push(await checkCommunityText({ text, field, env, fetchImpl: jevFetch(forumBody()), accountId: 'acct-canary' }));
    await env.SOCIAL.put(forumCheckKey('acct-full', new Date().toISOString().slice(0, 10)), '200');
    answers.push(await checkCommunityText({ text: CANARY, field: 'forum_post', env, fetchImpl: jevFetch(forumBody()), accountId: 'acct-full' }));
  } finally {
    Object.assign(console, saved);
  }

  // The test can see the canary where it is meant to go: Jev's request.
  assert.ok(sentBodies.length >= 6 && sentBodies.every(b => b.includes(CANARY)), 'the text is sent to Jev');
  assert.ok(printed.length > 0, 'the console was watched and written to');
  assert.ok(kvPuts.length > 0, 'KV was written to');
  assert.ok([...env.SOCIAL.m.keys()].some(k => k === costLedgerKey('profile:acct-canary')), 'the metering record was written');
  assert.ok([...env.SOCIAL.m.keys()].some(k => k === costLedgerKey('forum:acct-canary')), 'the forum metering record was written');
  assert.ok(sentBodies.some(b => JSON.parse(b).state.field === 'forum_post'), 'forum text is sent to Jev');

  const needle = CANARY.toLowerCase();
  const seen = (where, s) => assert.ok(!String(s).toLowerCase().includes(needle) && !String(s).toLowerCase().includes('quokka'), `the text reached ${where}: ${String(s).slice(0, 200)}`);
  for (const line of printed) seen('console output', line);
  for (const [k, v] of env.SOCIAL.m) { seen('a KV key', k); seen('a KV value', v); }
  for (const p of kvPuts) { seen('a KV put', p.k); seen('a KV put', p.v); seen('KV put options', p.opts); }
  for (const [k, v] of env.USERS.m) { seen('a KV key', k); seen('a KV value', v); }
  for (const [k, v] of env.SCENES.m) { seen('an R2 key', k); seen('an R2 object', v.body); seen('R2 put options', v.opts); }
  for (const a of answers) seen('the answer', JSON.stringify(a));
  assert.deepEqual(answers.map(a => a.verdict), [
    'pass', 'refuse', 'review', 'unavailable', 'unavailable', 'pass', 'pass', 'unavailable', 'refuse', 'refuse', 'refuse', 'refuse', 'unavailable', 'unavailable',
    'pass', 'refuse', 'refuse', 'unavailable', 'unavailable', 'pass', 'pass', 'unavailable', 'refuse', 'refuse', 'refuse', 'refuse', 'unavailable',
  ]);
});

// ── the cap ─────────────────────────────────────────────────────────────────

test('an account gets 50 checks that reach Jev a day; the next is `unavailable` with a reason Website can show, and calls nothing', async () => {
  assert.equal(DEFAULT_THRESHOLDS.profile_checks_per_account_per_day, 50);
  const env = fresh();
  const fetchImpl = jevFetch();
  for (let i = 0; i < 50; i++)
    assert.equal((await check({ text: `Bridge builder ${i}`, field: 'bio', env, fetchImpl, accountId: 'acct-7' })).verdict, 'pass', `check ${i + 1}`);
  assert.equal(fetchImpl.calls.length, 50);
  const key = profileCheckKey('acct-7', new Date().toISOString().slice(0, 10));
  assert.equal(env.SOCIAL.m.get(key), '50');
  assert.equal(kvPuts.filter(p => p.k === key).at(-1).opts, JSON.stringify({ expirationTtl: 172800 }));

  const over = await check({ text: 'Bridge builder', field: 'bio', env, fetchImpl, accountId: 'acct-7' });
  assert.deepEqual([over.verdict, over.category, over.reason], ['unavailable', null, 'cap']);
  assert.match(over.message, /50 times a day/);
  assert.equal(fetchImpl.calls.length, 50, 'past the cap, Jev is not called');

  // A deterministic refusal costs nothing and is not counted; another account has its own count.
  assert.equal((await check({ text: 'Admin', field: 'display_name', env, fetchImpl, accountId: 'acct-7' })).reason, 'staff_word');
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env, fetchImpl, accountId: 'acct-8' })).verdict, 'pass');
  // Without an account the check runs uncapped and counts nothing.
  const before = [...env.SOCIAL.m.keys()].filter(k => k.startsWith('profilecheck:')).length;
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env, fetchImpl })).verdict, 'pass');
  assert.equal([...env.SOCIAL.m.keys()].filter(k => k.startsWith('profilecheck:')).length, before);
  // Tunable.
  const tuned = fresh({ MODERATION_THRESHOLDS: JSON.stringify({ profile_checks_per_account_per_day: 1 }) });
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env: tuned, fetchImpl: jevFetch(), accountId: 'a' })).verdict, 'pass');
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env: tuned, fetchImpl: jevFetch(), accountId: 'a' })).reason, 'cap');
});

// ── metering ────────────────────────────────────────────────────────────────

test('the Jev call is metered under kind and reason `profile`: usage only', async () => {
  const meter = createReviewMeter({}, 'profile', 'profile');
  meterKind(meter, 'profile');
  assert.equal(meter.kind, 'profile');

  const env = fresh();
  await check({ text: 'Bridge builder', field: 'bio', env, fetchImpl: jevFetch(), accountId: 'acct-9' });
  const ledger = JSON.parse(env.SOCIAL.m.get(costLedgerKey('profile:acct-9')));
  assert.equal(ledger.runs.length, 1);
  const run = ledger.runs[0];
  assert.equal(run.kind, 'profile');
  assert.equal(run.reason, 'profile');
  assert.ok(run.finished_at);
  assert.equal(run.calls.length, 1);
  assert.equal(run.calls[0].stage, 'jev');
  assert.equal(run.calls[0].ok, true);
  assert.equal(run.calls[0].model, 'jev-latest');
  assert.equal(run.calls[0].usage.input_tokens, 120);
  assert.equal(run.calls[0].usage.output_tokens, 14);
  assert.equal(run.calls[0].usage_raw, null, 'the provider\'s raw usage object is not kept');
  assert.equal(run.calls[0].model_reported, 'jev-latest');
  const lines = [...env.SOCIAL.m.keys()].filter(k => k.startsWith('modcost:run:'));
  assert.equal(lines.length, 1);
  const line = JSON.parse(env.SOCIAL.m.get(lines[0]));
  assert.deepEqual([line.k, line.r], ['profile', 'profile']);
  assert.ok(!JSON.stringify([...env.SOCIAL.m.values()]).includes('Bridge builder'));

  // A failed call is metered too, and without an account the ledger is the shared one.
  await check({ text: 'Bridge builder', field: 'bio', env, fetchImpl: jevFetch({ error: 'x' }, { status: 500 }) });
  const shared = JSON.parse(env.SOCIAL.m.get(costLedgerKey('profile')));
  assert.deepEqual([shared.runs[0].kind, shared.runs[0].calls[0].ok, shared.runs[0].calls[0].error], ['profile', false, 'http_500']);

  // A deterministic refusal calls nothing and meters nothing; metering switched off leaves the check unchanged.
  const quiet = fresh();
  await check({ text: 'Admin', field: 'display_name', env: quiet, fetchImpl: jevFetch(), accountId: 'acct-9' });
  assert.equal([...quiet.SOCIAL.m.keys()].filter(k => k.startsWith('modcost:')).length, 0);
  const off = fresh({ REVIEW_METERING: 'off' });
  assert.equal((await check({ text: 'Bridge builder', field: 'bio', env: off, fetchImpl: jevFetch(), accountId: 'acct-9' })).verdict, 'pass');
  assert.equal([...off.SOCIAL.m.keys()].filter(k => k.startsWith('modcost:')).length, 0);
});

// ── forum fields ────────────────────────────────────────────────────────────

const forumCheck = (args) => silence(() => checkCommunityText(args));

test('checkCommunityText is the same check, and the forum fields are forum_title and forum_post', () => {
  assert.equal(checkCommunityText, checkProfileText);
  assert.deepEqual([...PROFILE_FIELDS], ['display_name', 'bio', 'forum_title', 'forum_post', 'guest_chat']);
  assert.deepEqual([...FORUM_FIELDS], ['forum_title', 'forum_post']);
  assert.ok(PROFILE_TEXT_MAX.forum_post <= JEV_STATE_MAX_CHARS, 'a whole forum post fits the Jev state budget');
});

test('a forum title is 1 to 120 characters and a forum post 1 to 10,000; empty refuses under `other` for both', async () => {
  const cases = [
    ['forum_title', 'a'.repeat(120), 'pass'],
    ['forum_title', 'a'.repeat(121), 'refuse'],
    ['forum_title', `  ${'a'.repeat(60)}  \n\t ${'b'.repeat(59)} `, 'pass'], // folds to 120
    ['forum_post', 'p'.repeat(10_000), 'pass'],
    ['forum_post', 'p'.repeat(10_001), 'refuse'],
    ['forum_post', '\u{1F680}'.repeat(10_000), 'pass'],
  ];
  for (const [field, text, want] of cases) {
    const fetchImpl = jevFetch(forumBody());
    const r = await forumCheck({ text, field, env: fresh(), fetchImpl });
    assert.equal(r.verdict, want, `${field} of ${[...text].length} characters`);
    if (want === 'refuse') {
      assert.deepEqual([r.category, r.reason], ['other', 'too_long']);
      assert.match(r.message, field === 'forum_post' ? /at most 10,000 characters/ : /at most 120 characters/);
      assert.equal(fetchImpl.calls.length, 0);
    } else assert.equal(fetchImpl.calls.length, 1);
  }
  for (const field of FORUM_FIELDS) {
    for (const text of ['', ' \n\n\t ', ZWSP + BOM, null]) {
      const fetchImpl = jevFetch(forumBody());
      const r = await forumCheck({ text, field, env: fresh(), fetchImpl });
      assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'other', 'empty'], `${JSON.stringify(text)} as ${field}`);
      assert.equal(fetchImpl.calls.length, 0);
    }
  }
});

test('a forum post keeps its line breaks, folds spaces within a line, collapses 3 or more blank lines to 2, and counts the breaks', async () => {
  assert.equal(forumPostCleaned('  first   line \t here  \r\nsecond\rthird\n\n\n\n\n\nlast  \n\n'), 'first line here\nsecond\nthird\n\n\nlast');
  assert.equal(forumPostCleaned('a\n\nb\n\n\nc'), 'a\n\nb\n\n\nc', 'one and two blank lines are kept');
  assert.equal(forumPostCleaned(`a${ZWSP}b\n${fullWidth('cd')}`), 'ab\ncd', 'NFKC and format characters as in a profile');
  assert.equal(forumPostCleaned(`x${cp(0x2028)}y${cp(0x2029)}z`), 'x\ny\nz', 'the Unicode line and paragraph separators are line breaks');

  const fetchImpl = jevFetch(forumBody());
  const r = await forumCheck({ text: 'Hello   all\r\n\r\n\r\n\r\n\r\nBridge   notes\n- one\n- two', field: 'forum_post', env: fresh(), fetchImpl });
  assert.equal(r.verdict, 'pass');
  assert.deepEqual(fetchImpl.calls[0].body.state, { field: 'forum_post', text: 'Hello all\n\n\nBridge notes\n- one\n- two' });
  // A title folds its line breaks, as a display name does.
  const title = jevFetch(forumBody());
  await forumCheck({ text: 'Bridge\nloads', field: 'forum_title', env: fresh(), fetchImpl: title });
  assert.equal(title.calls[0].body.state.text, 'Bridge loads');

  // The line break counts: 5,000 + 1 + 4,999 is 10,000 and passes; one more character refuses.
  assert.equal((await forumCheck({ text: `${'a'.repeat(5000)}\n${'b'.repeat(4999)}`, field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass');
  assert.equal((await forumCheck({ text: `${'a'.repeat(5000)}\n${'b'.repeat(5000)}`, field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).reason, 'too_long');
  // Blank lines past two do not count, spaces in a run count once.
  assert.equal((await forumCheck({ text: `${'a'.repeat(5000)}\n\n\n\n\n\n\n\n${'b'.repeat(4997)}`, field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass');
  assert.equal((await forumCheck({ text: `${'a'.repeat(5000)}${' '.repeat(400)}${'b'.repeat(4999)}`, field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass');
});

test('a link, an invite or a handle refuses in a forum title under `spam`; in a forum post it goes to Jev', async () => {
  for (const text of ['See https://x.io', 'www.mysite', 'join discord.gg/abc', 'Ask @ann']) {
    const fetchImpl = jevFetch(forumBody());
    const r = await forumCheck({ text, field: 'forum_title', env: fresh(), fetchImpl });
    assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'spam', 'link'], text);
    assert.equal(r.message, 'A forum title cannot carry a link, an invite or an @handle.');
    assert.equal(fetchImpl.calls.length, 0);
    const post = jevFetch(forumBody());
    assert.equal((await forumCheck({ text, field: 'forum_post', env: fresh(), fetchImpl: post })).verdict, 'pass', `${text} in a post`);
    assert.equal(post.calls.length, 1, 'a link in a post is Jev\'s call');
  }
  // A staff word is a display name's rule only.
  assert.equal((await forumCheck({ text: 'Admin question about the moderator tools', field: 'forum_title', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass');
});

test('a phone number or an email address refuses a forum title or post under `personal_info`, without a model call', async () => {
  for (const [text, kind] of [['Call me on 555 123 4567', 'phone'], ['write to ann@mail.example.org', 'email'], ['ann at mail dot com', 'email'], ['(555) 123.4567', 'phone']]) {
    for (const field of FORUM_FIELDS) {
      const fetchImpl = jevFetch(forumBody());
      const r = await forumCheck({ text: field === 'forum_post' ? `Hi all.\n\n${text}\nThanks` : text, field, env: fresh(), fetchImpl });
      assert.deepEqual([r.verdict, r.category, r.reason], ['refuse', 'personal_info', kind], `${text} in ${field}`);
      assert.ok(FORUM_CATEGORIES.includes(r.category));
      assert.equal(fetchImpl.calls.length, 0);
      // A post's refusal names what it saw, so an author whose numbers only look like a phone number can reword; it never
      // repeats the text.
      if (field === 'forum_post') assert.match(r.message, kind === 'phone' ? /looks like a phone number/ : /looks like an email address/);
      for (const piece of ['555', '123', '4567', 'ann@', 'ann at', 'mail.example', 'mail dot']) assert.ok(!r.message.includes(piece), `${field} message repeats ${piece}`);
    }
  }
  // The false positive the rule accepts: numbers split only by spaces. The message says what to do.
  const fps = await forumCheck({ text: 'Frame rates: 60 120 240 480 fps', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) });
  assert.deepEqual([fps.verdict, fps.reason], ['refuse', 'phone']);
  assert.match(fps.message, /looks like a phone number/);
  assert.match(fps.message, /separate the numbers with commas or words/);
  assert.ok(!/60|120|240|480|fps/.test(fps.message));
  assert.equal((await forumCheck({ text: 'Frame rates: 60, 120, 240, 480 fps', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) })).verdict, 'pass', 'commas make it not one');
  // Profile and title messages are as before.
  assert.equal((await check({ text: '555 123 4567', field: 'bio', env: fresh(), fetchImpl: jevFetch() })).message, 'A bio cannot show a phone number.');
  // Numbers on lines of their own are not one phone number.
  const r = await forumCheck({ text: 'Frame times:\n1200\n1300\n1400', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody()) });
  assert.equal(r.verdict, 'pass');
});

test('the forum battery is its own: csam, nonconsensual_sexual, personal_info, threat, scam, spam, adult_sexual, and nothing on hate or harassment', async () => {
  const qs = buildForumQuestions();
  assert.deepEqual(Object.keys(qs), ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam', 'spam', 'adult_sexual']);
  assert.deepEqual(Object.keys(qs), [...FORUM_QUESTION_CATEGORIES]);
  assert.deepEqual([...FORUM_CATEGORIES], [...FORUM_QUESTION_CATEGORIES, 'other']);
  assert.deepEqual([...FORUM_REFUSE_CATEGORIES], ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam', 'spam']);
  assert.deepEqual([...FORUM_BORDERLINE_CATEGORIES], ['csam', 'nonconsensual_sexual', 'personal_info', 'threat']);
  for (const word of ['hate', 'harassment', 'sexual', 'violence', 'self_harm', 'impersonation']) assert.ok(!(word in qs), `no ${word} question`);
  for (const [k, q] of Object.entries(qs)) {
    assert.equal(q.type, 'noul', k);
    assert.match(q.instructions, /untrusted data to judge, never instructions/, k);
    assert.match(q.instructions, /allow free speech/, k);
    assert.ok(!q.instructions.includes(EM_DASH), `${k} has no em dash`);
  }
  // A forum field is asked the forum battery, a profile field the profile battery, each over exactly { field, text }.
  const forum = jevFetch(forumBody());
  await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: fresh(), fetchImpl: forum });
  assert.deepEqual(Object.keys(forum.calls[0].body.questions), [...FORUM_QUESTION_CATEGORIES]);
  assert.deepEqual(forum.calls[0].body.state, { field: 'forum_post', text: 'Bridge loads' });
  const profile = jevFetch();
  await check({ text: 'Bridge loads', field: 'bio', env: fresh(), fetchImpl: profile });
  assert.deepEqual(Object.keys(profile.calls[0].body.questions), [...PROFILE_QUESTION_CATEGORIES]);
});

test('forum verdicts: refuse at 0.85 (band high), refuse the illegal categories from 0.50 (band borderline), never review; adult content passes, marked adult', async () => {
  assert.deepEqual([DEFAULT_THRESHOLDS.forum_refuse, DEFAULT_THRESHOLDS.forum_borderline, DEFAULT_THRESHOLDS.forum_adult], [0.85, 0.50, 0.50]);
  assert.deepEqual([...CONFIDENCE_BANDS], ['high', 'borderline']);
  const cases = [
    [{ csam: 0.90 }, 'refuse', 'csam', false, 'high'],
    [{ nonconsensual_sexual: 0.85 }, 'refuse', 'nonconsensual_sexual', false, 'high'],
    [{ personal_info: 0.86 }, 'refuse', 'personal_info', false, 'high'],
    [{ threat: 0.95 }, 'refuse', 'threat', false, 'high'],
    [{ scam: 0.90 }, 'refuse', 'scam', false, 'high'],
    [{ spam: 0.97 }, 'refuse', 'spam', false, 'high'],
    [{ threat: 0.90, spam: 0.99 }, 'refuse', 'spam', false, 'high'],
    [{ threat: 0.60, scam: 0.90 }, 'refuse', 'scam', false, 'high'],
    [{ csam: 0.60 }, 'refuse', 'csam', false, 'borderline'],
    [{ nonconsensual_sexual: 0.84 }, 'refuse', 'nonconsensual_sexual', false, 'borderline'],
    [{ personal_info: 0.70, threat: 0.50 }, 'refuse', 'personal_info', false, 'borderline'],
    [{ threat: 0.50 }, 'refuse', 'threat', false, 'borderline'],
    [{ threat: 0.49 }, 'pass', null, false, null],
    [{ scam: 0.84 }, 'pass', null, false, null],
    [{ spam: 0.80, scam: 0.60 }, 'pass', null, false, null],
    [{ adult_sexual: 0.99 }, 'pass', null, true, null],
    [{ adult_sexual: 0.50 }, 'pass', null, true, null],
    [{ adult_sexual: 0.49 }, 'pass', null, false, null],
    [{ adult_sexual: 0.99, threat: 0.60 }, 'refuse', 'threat', true, 'borderline'],
    [{}, 'pass', null, false, null],
  ];
  for (const [over, verdict, category, adult, band] of cases) {
    const r = await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(forumBody(over)) });
    assert.deepEqual([r.verdict, r.category, r.adult, r.confidence_band], [verdict, category, adult, band], JSON.stringify(over));
    assert.equal(r.reason, verdict === 'refuse' ? 'model' : verdict);
    assert.equal(r.message === null, verdict === 'pass');
  }
  // A borderline answer on an illegal category is a refusal at every point of the band, in either forum field: never a
  // review and never a pass. No forum answer at all is a review.
  for (const category of FORUM_BORDERLINE_CATEGORIES) {
    for (const p of [0.50, 0.51, 0.60, 0.70, 0.80, 0.84, 0.849]) {
      const d = decideForumAnswers(normalizedForum({ [category]: p }));
      assert.deepEqual([d.verdict, d.category, d.confidence_band], ['refuse', category, 'borderline'], `${category} at ${p}`);
    }
    for (const field of FORUM_FIELDS) {
      const r = await forumCheck({ text: 'Bridge loads', field, env: fresh(), fetchImpl: jevFetch(forumBody({ [category]: 0.6 })) });
      assert.deepEqual([r.verdict, r.confidence_band], ['refuse', 'borderline'], `${category} in ${field}`);
    }
  }
  for (const category of FORUM_QUESTION_CATEGORIES) {
    for (const p of [0, 0.3, 0.5, 0.7, 0.85, 1]) assert.notEqual(decideForumAnswers(normalizedForum({ [category]: p })).verdict, 'review', `${category} at ${p}`);
  }
  // A rule's refusal is certain: band high. A profile field has no band, and keeps its review.
  for (const [text, field] of [['a'.repeat(121), 'forum_title'], ['Call 555 123 4567', 'forum_post'], ['www.x.io', 'forum_title'], ['', 'forum_post']])
    assert.equal((await forumCheck({ text, field, env: fresh(), fetchImpl: jevFetch(forumBody()) })).confidence_band, 'high', `${field}: ${text}`);
  const bio = await check({ text: 'Bridge loads', field: 'bio', env: fresh(), fetchImpl: jevFetch(jevBody({ hate: 0.6 })) });
  assert.deepEqual([bio.verdict, bio.confidence_band], ['review', null], 'profiles keep review');
  assert.equal((await check({ text: 'Bridge loads', field: 'bio', env: fresh(), fetchImpl: jevFetch(jevBody({ hate: 0.9 })) })).confidence_band, null);
  // adult_sexual is never refused, however sure Jev is.
  for (const p of [0.85, 0.99, 1]) assert.equal(decideForumAnswers(normalizedForum({ adult_sexual: p })).verdict, 'pass');
  // A profile field is never adult.
  for (const field of ['display_name', 'bio']) assert.equal((await check({ text: 'Bridge loads', field, env: fresh(), fetchImpl: jevFetch(jevBody({ sexual: 0.6 })) })).adult, false);
  // Tunable.
  const tuned = fresh({ MODERATION_THRESHOLDS: JSON.stringify({ forum_refuse: 0.95, forum_borderline: 0.7, forum_adult: 0.9 }) });
  const t1 = await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: tuned, fetchImpl: jevFetch(forumBody({ threat: 0.9, adult_sexual: 0.8 })) });
  assert.deepEqual([t1.verdict, t1.category, t1.adult, t1.confidence_band], ['refuse', 'threat', false, 'borderline']);
  const t2 = await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: tuned, fetchImpl: jevFetch(forumBody({ threat: 0.65 })) });
  assert.equal(t2.verdict, 'pass');
  // A malformed forum answer (a category missing) is unavailable, never a pass.
  const missing = forumBody();
  delete missing.answers.adult_sexual;
  assert.deepEqual([(await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(missing) })).verdict], ['unavailable']);
  assert.equal((await forumCheck({ text: 'Bridge loads', field: 'forum_post', env: fresh(), fetchImpl: jevFetch(jevBody()) })).reason, 'malformed', 'the profile battery\'s answers are not the forum\'s');
});

// What normalizeAnswers makes of forumBody, for decideForumAnswers directly.
function normalizedForum(over) {
  const answers = {};
  for (const c of FORUM_QUESTION_CATEGORIES) answers[c] = { type: 'noul', p: over[c] ?? 0.05 };
  return { answers };
}

// Jev as it would answer a battery that asked about anything but the illegal categories: every question it does not know as
// one of those is answered "yes, certainly". Against the forum battery, only adult_sexual is such a question.
const ILLEGAL = ['csam', 'nonconsensual_sexual', 'personal_info', 'threat', 'scam', 'spam'];
const yesToEverythingElse = jevFetch(req => {
  const answers = {};
  for (const k of Object.keys(req.body.questions)) answers[k] = { type: 'noul', noul: ILLEGAL.includes(k) ? 0.05 : 0.99 };
  return { model: 'jev-latest', answers, usage: USAGE };
});

test('hate, harassment, insults and criticism of Eustress publish in the forums with no hold; a profile still refuses hate', async () => {
  const texts = [
    'Eustress is garbage and the people running it are clowns. Worst update ever.',
    'The moderators here are incompetent and the staff should be ashamed of this release.',
    'You are an idiot and your build is the ugliest thing I have seen.',
    'I despise people who play on easy mode, they are pathetic.',
  ];
  for (const text of texts) {
    for (const field of FORUM_FIELDS) {
      const r = await forumCheck({ text, field, env: fresh(), fetchImpl: yesToEverythingElse });
      assert.equal(r.verdict, 'pass', `${field}: ${text}`);
      assert.equal(r.category, null);
    }
  }
  // The same Jev, asked by a profile, refuses: the profile rules are unchanged.
  const bio = await check({ text: texts[0], field: 'bio', env: fresh(), fetchImpl: yesToEverythingElse });
  assert.equal(bio.verdict, 'refuse');
});

test('the forum fields have their own cap (200 a day), apart from the profile fields\' 50, under their own key', async () => {
  assert.equal(DEFAULT_THRESHOLDS.forum_checks_per_account_per_day, 200);
  const env = fresh();
  const fetchImpl = jevFetch(forumBody());
  const profile = jevFetch();
  for (let i = 0; i < 50; i++) await check({ text: `Bridge ${i}`, field: 'bio', env, fetchImpl: profile, accountId: 'acct-f' });
  assert.equal((await check({ text: 'Bridge', field: 'bio', env, fetchImpl: profile, accountId: 'acct-f' })).reason, 'cap', 'the profile family is spent');
  for (let i = 0; i < 200; i++)
    assert.equal((await forumCheck({ text: `Bridge ${i}`, field: i % 2 ? 'forum_post' : 'forum_title', env, fetchImpl, accountId: 'acct-f' })).verdict, 'pass', `forum check ${i + 1}`);
  const day = new Date().toISOString().slice(0, 10);
  assert.equal(env.SOCIAL.m.get(forumCheckKey('acct-f', day)), '200');
  assert.equal(env.SOCIAL.m.get(profileCheckKey('acct-f', day)), '50');
  assert.notEqual(forumCheckKey('acct-f', day), profileCheckKey('acct-f', day));
  const over = await forumCheck({ text: 'Bridge', field: 'forum_post', env, fetchImpl, accountId: 'acct-f' });
  assert.deepEqual([over.verdict, over.category, over.reason, over.adult], ['unavailable', null, 'cap', false]);
  assert.match(over.message, /Forum text can be checked 200 times a day/);
  assert.equal(fetchImpl.calls.length, 200, 'past the cap, Jev is not called');
  // Another account, and a fresh profile family, are not affected by a spent forum family.
  const env2 = fresh();
  await env2.SOCIAL.put(forumCheckKey('acct-g', day), '200');
  assert.equal((await check({ text: 'Bridge', field: 'bio', env: env2, fetchImpl: jevFetch(), accountId: 'acct-g' })).verdict, 'pass');
  assert.equal((await forumCheck({ text: 'Bridge', field: 'forum_post', env: env2, fetchImpl: jevFetch(forumBody()), accountId: 'acct-h' })).verdict, 'pass');
  // Tunable.
  const tuned = fresh({ MODERATION_THRESHOLDS: JSON.stringify({ forum_checks_per_account_per_day: 1 }) });
  assert.equal((await forumCheck({ text: 'Bridge', field: 'forum_post', env: tuned, fetchImpl: jevFetch(forumBody()), accountId: 'a' })).verdict, 'pass');
  assert.equal((await forumCheck({ text: 'Bridge', field: 'forum_post', env: tuned, fetchImpl: jevFetch(forumBody()), accountId: 'a' })).reason, 'cap');
});

test('a forum check is metered under kind and reason `forum`, apart from `profile`: usage only', async () => {
  assert.ok(METERED_KINDS.includes('forum'));
  const env = fresh();
  await forumCheck({ text: 'Bridge loads\nand tides', field: 'forum_post', env, fetchImpl: jevFetch(forumBody()), accountId: 'acct-m' });
  const ledger = JSON.parse(env.SOCIAL.m.get(costLedgerKey('forum:acct-m')));
  assert.equal(ledger.runs.length, 1);
  assert.deepEqual([ledger.runs[0].kind, ledger.runs[0].reason], ['forum', 'forum']);
  assert.equal(ledger.runs[0].calls[0].usage.input_tokens, 120);
  assert.equal(ledger.runs[0].calls[0].usage_raw, null);
  assert.equal(env.SOCIAL.m.get(costLedgerKey('profile:acct-m')), undefined, 'not on the profile ledger');
  const lines = [...env.SOCIAL.m.keys()].filter(k => k.startsWith('modcost:run:'));
  assert.deepEqual(lines.map(k => { const l = JSON.parse(env.SOCIAL.m.get(k)); return [l.k, l.r]; }), [['forum', 'forum']]);
  assert.ok(!JSON.stringify([...env.SOCIAL.m.values()]).includes('Bridge loads'));
  // Without an account, the shared forum ledger.
  await forumCheck({ text: 'Bridge loads', field: 'forum_title', env, fetchImpl: jevFetch(forumBody()) });
  assert.equal(JSON.parse(env.SOCIAL.m.get(costLedgerKey('forum'))).runs[0].kind, 'forum');
});
