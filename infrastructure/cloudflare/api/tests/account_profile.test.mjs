// The public profile an account edits (POST /api/account/profile): display name, bio and avatar picture, and what every
// visitor then reads at GET /api/community/users/{username}. A session is required and must be current, an account that
// must verify its identity is refused, edits are limited to 10 an hour, invisible characters are removed, display text is
// cleaned like other display text and refused over its limit, a display name may not pass for staff or for another
// account, a new name or bio waits for review before anyone else sees it, and the picture is a path under /assets, nothing
// else. The real Worker runs against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth } from '../src/account_auth.mjs';
import { routePolicy, FRESH } from '../src/route_policy.mjs';
import {
  cleanDisplayName, cleanBio, cleanAvatarUrl, readProfile, publicProfileOf, handleProfileRoute, profileKey, PROFILE_LIMITS,
  isReservedName, foldName, stripInvisible, profileEditsKey, usernameForms, isAnotherUsername, unconfuse,
  reviewText, pendingProfileOf, ownerPendingOf, cleanReason, checkedLength,
} from '../src/account_profile.mjs';
import { JEV_ENDPOINT, PROFILE_QUESTION_CATEGORIES, profileTextCleaned, profileCheckKey } from '../src/moderation.mjs';

const worker = await loadWorker();
const ACCOUNT = '77777777-7777-4777-8777-777777777777';
const OTHER = '88888888-8888-4888-8888-888888888888';
const UNVERIFIED = '99999999-9999-4999-8999-999999999999';
const BIGBEN = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
const ORIGIN = 'https://api.eustress.dev';
const token = mintToken(ACCOUNT);

async function environment() {
  const env = { ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  for (const [id, username, verified] of [[ACCOUNT, 'owner', true], [OTHER, 'other', true], [UNVERIFIED, 'early', false], [BIGBEN, 'Big_Ben', true]]) {
    const { publicKey } = crypto.generateKeyPairSync('ed25519');
    const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
    await env.USERS.put(`user:${id}`, JSON.stringify({ ...(verified ? { kyc_verified: true } : {}), id, username, public_key: hex, created_at: '2026-03-01T00:00:00Z' }));
    await env.USERS.put(`username:${username}`, id);
    await env.USERS.put(`pubkey:${hex}`, id);
  }
  return env;
}

async function call(env, method, route, { body, auth = token, raw } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (auth) headers.Authorization = `Bearer ${auth}`;
  const init = { method, headers };
  if (method !== 'GET') init.body = raw !== undefined ? raw : JSON.stringify(body || {});
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, init), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), headers: res.headers };
}

const edit = (env, body, opts = {}) => call(env, 'POST', '/api/account/profile', { body, ...opts });
const shown = async (env, username = 'owner') => (await call(env, 'GET', `/api/community/users/${username}`, { auth: null })).body;
/// A new hour: every account's edit counter is removed.
const newHour = (env) => { for (const k of [...env.USERS.store.keys()]) if (k.startsWith('profile-edits:')) env.USERS.store.delete(k); };

// ── the rules for each field ─────────────────────────────────────────────────

test('display text is cleaned like other display text and refused over its limit, counted in characters', () => {
  assert.equal(cleanDisplayName('  Ada \n\t Builds  '), 'Ada Builds', 'whitespace folded and trimmed');
  assert.equal(cleanDisplayName('Re\u202Efund\u0000\u0007'), 'Refund', 'bidi overrides and control characters go');
  assert.equal(cleanDisplayName('x'.repeat(PROFILE_LIMITS.display_name)), 'x'.repeat(32));
  assert.equal(cleanDisplayName('x'.repeat(33)), null, 'one over is refused, not cut');
  assert.equal(cleanDisplayName('\u00E9'.repeat(32)), '\u00E9'.repeat(32), 'characters, not bytes');
  assert.equal(cleanDisplayName('\u{1F680}'.repeat(32)), '\u{1F680}'.repeat(32), 'an astral character counts once');
  assert.equal(cleanDisplayName('a ' + ' '.repeat(100) + 'b'), 'a b', 'folding happens before measuring');
  for (const bad of [7, null, undefined, {}, ['a'], true]) assert.equal(cleanDisplayName(bad), null, JSON.stringify(bad));
  assert.equal(cleanBio('y'.repeat(280)), 'y'.repeat(280));
  assert.equal(cleanBio('y'.repeat(281)), null);
  assert.equal(cleanBio('line one\nline two'), 'line one line two');
});

test('invisible characters go: NFKC, every format character, more than two marks on a letter; a bio keeps the emoji joiner', () => {
  assert.equal(cleanDisplayName('Ada\u200B\u200C\u200D\u2060\uFEFF Lovelace'), 'Ada Lovelace', 'zero-width spaces, joiners, word joiner, BOM');
  assert.equal(cleanDisplayName('a\u2066b\u2069c\u200Ed\u061Ce\u00ADf'), 'abcdef', 'isolates, marks, the Arabic mark, the soft hyphen');
  assert.equal(cleanDisplayName('\uFF21\uFF44\uFF41'), 'Ada', 'NFKC: full-width letters are letters');
  assert.equal(cleanDisplayName('\uFB01ne'), 'fine', 'NFKC: a ligature is its letters');
  assert.equal(cleanDisplayName('\u200B\u200B\u200B'), '', 'nothing visible is nothing');
  const zalgo = 'Z' + '\u0336\u0337\u0338\u0335\u0334'.repeat(4) + 'oe';
  assert.equal(cleanDisplayName(zalgo), 'Z\u0336\u0337oe', 'two marks on a letter, no more');
  assert.equal(cleanDisplayName('e\u0301'), '\u00E9', 'one accent composes');
  assert.equal(cleanDisplayName('\u{1F468}\u200D\u{1F469}'), '\u{1F468}\u{1F469}', 'a display name keeps no joiner at all');
  const family = '\u{1F468}\u200D\u{1F469}\u200D\u{1F467}';
  const coder = '\u{1F469}\u{1F3FD}\u200D\u{1F4BB}';
  const heart = '\u2764\uFE0F\u200D\u{1F525}';
  assert.equal(cleanBio(`${family} ${coder} ${heart}`), `${family} ${coder} ${heart}`, 'a bio keeps a joiner between two emoji');
  assert.equal(cleanBio('a\u200Db \u{1F600}\u200D x\u200D\u{1F600}'), 'ab \u{1F600} x\u{1F600}', 'and only there');
  assert.equal(cleanBio('hi\u200B\uFEFF there'), 'hi there');
  assert.equal(stripInvisible('\u0301\u0301\u0301x'), '\u0301\u0301x', 'marks at the start count too');
});

test('a display name may not pass for staff: any word, words run together, any spelling of them, or eustress anywhere', () => {
  for (const name of [
    'support', 'Admin', 'ADMINISTRATOR', 'Moderator', 'mod', 'Staff', 'Official', 'Eustress', 'system', 'Security', 'help', 'Team',
    'Trust', 'Safety',
    'Eustress Support', 'Ada from Support', 'support_team', 'admin.team', 'TrustSafety', 'S U P P O R T', 'EustressSupport',
    '4dm1n', 'Adm1n', 'admln', 'm0d', 'h3lp', '5upp0rt', '0ff1c1al', 'St4ff', 'S@fety', '$ecurity', 'tru5t', 'te4m', '5y5tem',
    '\u00E1dmin', '\uFF21\uFF24\uFF2D\uFF29\uFF2E', 'EustressFan', 'the eu5tre55 crew',
  ]) assert.equal(isReservedName(name), true, name);
  for (const name of ['Ada', 'Modeler', 'Helpful Hank', 'Steam fan', 'Trustworthy', 'Systematic Sam', 'Lily', 'Teamwork Tom', 'Bridge Builder']) {
    assert.equal(isReservedName(name), false, name);
  }
  assert.equal(foldName('Adm1n'), foldName('admin'));
  assert.equal(foldName('0.t-h_e r'), foldName('other'));
});

test('the picture is a path under /assets and nothing else: no address on any host, ours included', () => {
  for (const ok of ['/assets/icons/noob-head.svg', '/assets/avatars/a_b-c.png']) assert.equal(cleanAvatarUrl(ok), ok, ok);
  for (const bad of [
    'https://api.eustress.dev/api/simulations/abc/thumbnail', 'https://api.eustress.dev/api/simulations/abc/cover',
    'https://cdn.eustress.dev/a.png', 'https://eustress.dev/a.png', 'http://eustress.dev/a.png', 'https://evil.example/a.png',
    'javascript:alert(1)', 'data:image/png;base64,AAAA', '//eustress.dev/a.png', '/assets/../secret', '/assets//x',
    '/api/simulations/x/thumbnail', '/assets/a b.png', '/assets/<x>.png', '/assets/' + 'a'.repeat(600), '', 7, null,
  ]) assert.equal(cleanAvatarUrl(bad), null, String(bad).slice(0, 60));
});

// ── the route ────────────────────────────────────────────────────────────────

test('the route is fresh and named in the policy table', () => {
  const r = routePolicy('POST', '/api/account/profile');
  assert.equal(r.class, FRESH);
  assert.equal(r.explicit, true);
});


test('the common Cyrillic and Greek look-alikes read as the Latin letters they imitate', () => {
  for (const name of ['\u0430dmin', '\u0430\u0434min'.replace('\u0434', 'd'), '\u0405upport', 's\u0443stem', 'mod\u0435rator', '\u039C\u039F\u0394'.replace('\u0394', 'D'), '\u0391\u0394\u039C\u0399\u039D'.replace('\u0394', 'D'), '\u0435ustr\u0435ss', '\u0455\u0430f\u0435t\u0443', 'h\u0435lp', '\u03C4eam']) {
    assert.equal(isReservedName(name), true, name);
  }
  assert.equal(unconfuse('\u0430dmin'), 'admin', 'a Cyrillic a is a');
  assert.equal(unconfuse('\u0397i'), 'Hi', 'an upper-case eta is H');
  assert.equal(foldName('\u03B7'), 'n', 'and a lower-case eta is n');
  assert.equal(foldName('\u043E\u0442h\u0435r'), 'other');
});

test('a display name is checked against usernames by a few direct reads, in the forms registration stores, never a scan', async () => {
  const env = await environment();
  assert.deepEqual(usernameForms('Big Ben'), ['BigBen', 'bigben', 'Big_Ben', 'big_ben']);
  assert.ok(usernameForms('0ther').includes('other'), 'the folded spelling is one of the forms');
  assert.deepEqual(usernameForms('A'), [], 'too short to be a username');
  let listed = 0;
  let reads = 0;
  const realList = env.USERS.list.bind(env.USERS);
  const realGet = env.USERS.get.bind(env.USERS);
  env.USERS.list = async (...a) => { listed++; return realList(...a); };
  env.USERS.get = async (k) => { if (k.startsWith('username:')) reads++; return realGet(k); };
  assert.equal(await isAnotherUsername(env, 'Big Ben', ACCOUNT), true, 'Big_Ben is taken');
  assert.equal(await isAnotherUsername(env, '\u043Ether', ACCOUNT), true, 'a Cyrillic o in other');
  assert.equal(await isAnotherUsername(env, 'Owner', ACCOUNT), false, 'your own username is yours');
  assert.equal(await isAnotherUsername(env, 'Ada Lovelace', ACCOUNT), false);
  assert.equal(listed, 0, 'no key is ever listed');
  assert.ok(reads <= 4 * 8, `a few reads per name, not one per account (${reads})`);
});

// ── the route ────────────────────────────────────────────────────────────────

const plainJson = (body, status, headers) => new Response(JSON.stringify(body), { status, headers });
/// The route with a stand-in for Jev's check: what it was asked, and what it answers.
async function reviewed(env, body, answer, asked = []) {
  const check = async (args) => { asked.push(args); if (answer instanceof Error) throw answer; return answer; };
  const res = await handleProfileRoute(
    new Request(`${ORIGIN}/api/account/profile`, { method: 'POST', body: JSON.stringify(body) }),
    new URL(`${ORIGIN}/api/account/profile`), env, { authenticate: async () => ACCOUNT, json: plainJson, cors: {}, checkProfileText: check });
  return { status: res.status, body: await res.json() };
}

/// Jev's endpoint answering every category at 0.05 but those in `levels`, for the real check inside the real Worker. Every
/// call is recorded; anything else goes to the real fetch.
async function withJev(levels, fn) {
  const real = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, init) => {
    if (String(url) !== JEV_ENDPOINT) return real(url, init);
    calls.push(JSON.parse(init.body));
    const answers = {};
    for (const c of PROFILE_QUESTION_CATEGORIES) answers[c] = { type: 'noul', noul: levels[c] ?? 0.05 };
    return new Response(JSON.stringify({ model: 'jev-latest', answers, usage: { input_tokens: 1, output_tokens: 1 } }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    return await fn(calls);
  } finally {
    globalThis.fetch = real;
  }
}

test('through the Worker with Jev configured: pass publishes, refuse answers with the category and the check\'s sentence, review waits for a person', async () => {
  const env = { ...(await environment()), JEV_API_KEY: 'test-jev-key' };
  await withJev({}, async (calls) => {
    const ok = await edit(env, { display_name: ' Ada \u200B  Builds ', bio: 'I model bridges.' });
    assert.equal(ok.status, 200, JSON.stringify(ok.body));
    assert.deepEqual(ok.body.profile, { display_name: 'Ada Builds', bio: 'I model bridges.', avatar_url: null });
    assert.deepEqual(ok.body.pending, {});
    assert.equal(calls.length, 2, 'one call for each field');
    assert.deepEqual(calls.map((c) => [c.state.field, c.state.text]), [['display_name', 'Ada Builds'], ['bio', 'I model bridges.']]);
    assert.equal((await shown(env)).display_name, 'Ada Builds', 'published for every visitor');
    assert.equal(await env.SOCIAL.get(profileCheckKey(ACCOUNT, new Date().toISOString().slice(0, 10))), '2', 'the checks are counted for this account');
    // Clearing is never sent.
    newHour(env);
    assert.equal((await edit(env, { display_name: '', bio: '' })).status, 200);
    assert.equal(calls.length, 2, 'no call to clear a name or a bio');
    assert.equal((await shown(env)).display_name, 'owner');
  });
  newHour(env);
  await withJev({ [PROFILE_QUESTION_CATEGORIES[2]]: 0.95 }, async () => {
    const no = await edit(env, { bio: 'Something Jev refuses.' });
    assert.equal(no.status, 400, JSON.stringify(no.body));
    assert.equal(no.body.code, 'refused');
    assert.equal(no.body.category, PROFILE_QUESTION_CATEGORIES[2]);
    assert.ok(typeof no.body.error === 'string' && no.body.error.length > 0 && !no.body.error.includes('Jev refuses'), 'the check\'s sentence, never the text');
    assert.deepEqual(no.body.pending, {});
  });
  newHour(env);
  await withJev({ [PROFILE_QUESTION_CATEGORIES[0]]: 0.6 }, async () => {
    const held = await edit(env, { bio: 'Something for a person.' });
    assert.equal(held.status, 200);
    assert.deepEqual(held.body.pending, { bio: 'Something for a person.' });
    assert.deepEqual(held.body.pending_notes, { bio: { state: 'review' } });
    assert.equal((await shown(env)).bio, '', 'nothing public while a person looks');
  });
});

test('the text goes to the check as the owner sent it, and the route counts it the way the check does', async () => {
  const env = await environment();
  const asked = [];
  const raw = '  Ada \u200B\u200B  Builds\u2060 ';
  const r = await reviewed(env, { display_name: raw }, { verdict: 'unavailable', message: null }, asked);
  assert.equal(r.status, 200);
  assert.equal(asked[0].text, raw, 'raw, not the route\'s cleaned copy');
  assert.equal(asked[0].field, 'display_name');
  assert.deepEqual(r.body.pending, { display_name: 'Ada Builds' }, 'what waits is the cleaned text');
  // The check counts: NFKC, format characters gone, whitespace runs folded, code points.
  for (const [value, ok] of [
    ['x'.repeat(32) + '\u200B'.repeat(5), true],
    ['x'.repeat(30) + '   ' + 'y', true],
    ['x'.repeat(31) + '   ' + 'y', false],
    ['x'.repeat(32) + '\u0007', false],
    ['x'.repeat(29) + 'e\u0301\u0302\u0303\u0304', false],
  ]) {
    assert.equal(checkedLength(value) <= PROFILE_LIMITS.display_name, ok, JSON.stringify(value));
    assert.equal(checkedLength(value), [...profileTextCleaned(value)].length);
    newHour(env);
    const res = await reviewed(env, { display_name: value }, { verdict: 'unavailable' });
    assert.equal(res.status === 200, ok, `${JSON.stringify(value)}: ${JSON.stringify(res.body)}`);
    if (!ok) assert.equal(res.body.code, 'too_long');
  }
});

test('through the Worker with Jev not configured: the real check answers unavailable, the values wait, visitors see the approved values, the owner sees them with the check\'s sentence', async () => {
  const env = await environment();
  const before = await env.USERS.get(`user:${ACCOUNT}`);
  const set = await edit(env, { display_name: '  Ada  Builds ', bio: 'I model bridges.\nAnd cranes.', avatar_url: '/assets/avatars/ada.png' });
  assert.equal(set.status, 200, JSON.stringify(set.body));
  assert.equal(set.body.changed, true);
  assert.deepEqual(set.body.profile, { display_name: '', bio: '', avatar_url: '/assets/avatars/ada.png' });
  assert.deepEqual(set.body.pending, { display_name: 'Ada Builds', bio: 'I model bridges. And cranes.' });
  for (const field of ['display_name', 'bio']) {
    const note = set.body.pending_notes[field];
    assert.equal(note.state, 'unavailable', field);
    assert.ok(typeof note.reason === 'string' && note.reason.length > 0, 'the check says why');
    assert.ok(!note.reason.includes('Ada') && !note.reason.includes('bridges'), 'and never repeats the text');
  }
  assert.equal(await env.USERS.get(`user:${ACCOUNT}`), before, 'the account record is never rewritten');

  const visitor = await shown(env);
  assert.equal(visitor.display_name, 'owner', 'a visitor sees the username while the name waits');
  assert.equal(visitor.bio, '', 'and no bio');
  assert.equal(visitor.avatar_url, '/assets/avatars/ada.png', 'the picture needs no review');
  assert.ok(!('pending' in visitor) && !('pending_notes' in visitor), 'a visitor is never told what waits');
  const signedInStranger = (await call(env, 'GET', '/api/community/users/owner', { auth: mintToken(OTHER) })).body;
  assert.ok(!('pending' in signedInStranger) && signedInStranger.display_name === 'owner', 'nor is another signed-in account');
  const owner = await call(env, 'GET', '/api/community/users/owner');
  assert.deepEqual(owner.body.pending, { display_name: 'Ada Builds', bio: 'I model bridges. And cranes.' }, 'the owner sees it pending');
  assert.deepEqual(owner.body.pending_notes, set.body.pending_notes, 'the owner reads the same notes');
  assert.equal(owner.headers.get('Cache-Control'), 'private, no-store');
  assert.equal(owner.body.display_name, 'owner');
});

test('pass publishes; the next change waits again while the approved value stays; clearing needs no review', async () => {
  const env = await environment();
  const asked = [];
  const ok = await reviewed(env, { display_name: 'Ada Builds', bio: 'Bridges.' }, { verdict: 'pass', category: null }, asked);
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  assert.deepEqual(ok.body.profile, { display_name: 'Ada Builds', bio: 'Bridges.', avatar_url: null });
  assert.deepEqual(ok.body.pending, {});
  assert.deepEqual(asked.map((a) => [a.field, a.text, a.timeoutMs]), [['display_name', 'Ada Builds', PROFILE_LIMITS.review_ms], ['bio', 'Bridges.', PROFILE_LIMITS.review_ms]]);
  assert.equal((await shown(env)).display_name, 'Ada Builds');
  assert.equal((await shown(env)).bio, 'Bridges.');

  // A new name with no check: the approved one stays public.
  newHour(env);
  assert.equal((await edit(env, { display_name: 'Ada B' })).status, 200);
  assert.equal((await shown(env)).display_name, 'Ada Builds');
  assert.deepEqual(await pendingProfileOf(env, ACCOUNT), { display_name: 'Ada B' });

  // Clearing is public at once and drops what waited.
  newHour(env);
  const cleared = await edit(env, { display_name: '' });
  assert.equal(cleared.status, 200);
  assert.equal((await shown(env)).display_name, 'owner');
  assert.deepEqual(await pendingProfileOf(env, ACCOUNT), {});

  // The same values again write nothing but the hour's counter.
  newHour(env);
  const writes = [];
  const realPut = env.USERS.put.bind(env.USERS);
  env.USERS.put = async (k, v, o) => { writes.push(k); return realPut(k, v, o); };
  assert.equal((await edit(env, { bio: 'Bridges.' })).body.changed, false);
  assert.deepEqual(writes.filter((k) => !k.startsWith('profile-edits:')), []);
});

test('refuse drops the pending value and answers with the category alone; the text is never logged', async () => {
  const env = await environment();
  const secret = 'Ada Lovelace Fan Club';
  const logged = [];
  const saved = { log: console.log, warn: console.warn, error: console.error, info: console.info };
  for (const k of Object.keys(saved)) console[k] = (...a) => { logged.push(a.map(String).join(' ')); };
  let r;
  try {
    r = await reviewed(env, { display_name: secret }, { verdict: 'refuse', category: 'harassment', reason: 'model', message: null });
  } finally {
    Object.assign(console, saved);
  }
  assert.equal(r.status, 400);
  assert.equal(r.body.code, 'refused');
  assert.equal(r.body.category, 'harassment');
  assert.equal(r.body.field, 'display_name');
  assert.ok(!JSON.stringify(r.body).includes(secret), 'the answer never repeats the text');
  assert.ok(!logged.some((l) => l.includes(secret)), 'nothing logged it');
  assert.deepEqual(await pendingProfileOf(env, ACCOUNT), {}, 'the pending value is gone');
  assert.equal((await shown(env)).display_name, 'owner');
  const odd = await reviewed(env, { bio: 'x' }, { verdict: 'refuse', category: 'something-new' });
  assert.equal(odd.body.category, 'other', 'a category the route does not know reads as other');
  // The check's own sentence is the one shown when it gives one.
  const phone = await reviewed(env, { bio: 'call me' }, { verdict: 'refuse', category: 'personal_info', reason: 'phone', message: 'Your bio cannot show a phone number.' });
  assert.equal(phone.body.error, 'Your bio cannot show a phone number.');
  assert.equal(phone.body.category, 'personal_info');
});

test('unavailable, an error, or an answer that is none of the three leaves the value pending and unpublished', async () => {
  for (const answer of [{ verdict: 'unavailable', category: null }, new Error('Jev is down'), { verdict: 'maybe' }, null, { pass: true }]) {
    const env = await environment();
    const r = await reviewed(env, { display_name: 'Ada Builds' }, answer);
    assert.equal(r.status, 200, String(answer));
    assert.deepEqual(r.body.pending, { display_name: 'Ada Builds' }, String(answer));
    assert.equal((await shown(env)).display_name, 'owner', String(answer));
  }
  // What waits is sent again with the next save, and published when the check passes it.
  const env = await environment();
  await reviewed(env, { display_name: 'Ada Builds' }, { verdict: 'unavailable' });
  const later = await reviewed(env, { avatar_url: '/assets/avatars/a.png' }, { verdict: 'pass' });
  assert.deepEqual(later.body.profile.display_name, 'Ada Builds');
  assert.deepEqual(await reviewText(null, {}, 'bio', 'x', ACCOUNT), { hold: 'unavailable' }, 'no check is unavailable, never a pass');
});

test('review holds the value for a person: pending, nothing public, and the owner is told a person will look', async () => {
  const env = await environment();
  const asked = [];
  const r = await reviewed(env, { display_name: 'Ada Builds' }, { verdict: 'review', category: null }, asked);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.deepEqual(r.body.pending, { display_name: 'Ada Builds' });
  assert.deepEqual(r.body.pending_notes, { display_name: { state: 'review' } });
  assert.equal(asked[0].accountId, ACCOUNT, 'the check is told whose text it is, for its daily allowance');
  assert.equal((await shown(env)).display_name, 'owner', 'nothing is public');
  assert.deepEqual(await ownerPendingOf(env, ACCOUNT), { pending: { display_name: 'Ada Builds' }, pending_notes: { display_name: { state: 'review' } } });
});

test('unavailable leaves it pending with the reason the check gave, for the owner alone', async () => {
  const env = await environment();
  const reason = 'You have used today\'s 50 checks. Your change waits until tomorrow.';
  const r = await reviewed(env, { bio: 'Bridges.' }, { verdict: 'unavailable', category: null, reason: 'cap', message: reason });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body.pending_notes, { bio: { state: 'unavailable', reason } });
  const owner = await call(env, 'GET', '/api/community/users/owner');
  assert.deepEqual(owner.body.pending_notes, { bio: { state: 'unavailable', reason } }, 'the owner reads the reason');
  const visitor = await shown(env);
  assert.ok(!JSON.stringify(visitor).includes('50 checks') && visitor.bio === '', 'a visitor reads neither');
  // The reason is one plain line, at most 200 characters.
  assert.equal(cleanReason('a\u0000b\u202Ec\n d'), 'a bc d');
  assert.equal([...cleanReason('x'.repeat(500))].length, 200);
  assert.equal(cleanReason(7), '');
  // With no reason, or an error, it is still unavailable.
  const plain = await reviewed(env, { bio: 'Again.' }, { verdict: 'unavailable' });
  assert.deepEqual(plain.body.pending_notes, { bio: { state: 'unavailable' } });
});

test('a display name is 1 to 32 visible characters and a bio 0 to 280, counted after NFKC and format characters go', async () => {
  const env = await environment();
  for (const name of ['\u200B\u200B', '\uFEFF', '   ', '\u2060 \u200D']) {
    newHour(env);
    const r = await edit(env, { display_name: name });
    assert.equal(r.status, 400, JSON.stringify(name));
    assert.equal(r.body.code, 'too_short', JSON.stringify(name));
  }
  newHour(env);
  assert.equal((await edit(env, { display_name: 'A' })).status, 200, 'one character is a name');
  newHour(env);
  assert.equal((await edit(env, { display_name: 'x'.repeat(32) + '\u200B'.repeat(10) })).status, 200, 'invisible characters do not count');
  newHour(env);
  const bio = await edit(env, { bio: '\u200B' });
  assert.equal(bio.status, 200, 'an empty bio is allowed');
  assert.equal(bio.body.profile.bio, '');
});

test('a display name that passes for staff or for another account is refused before review, and the text is never logged', async () => {
  const env = await environment();
  const logged = [];
  const saved = { log: console.log, warn: console.warn, error: console.error, info: console.info };
  for (const k of Object.keys(saved)) console[k] = (...a) => { logged.push(a.map(String).join(' ')); };
  try {
    for (const name of ['Eustress Support', '4dm1n', 'Official', 'm0d squad', 'Trust & Safety', '\u0430dmin']) {
      newHour(env);
      const r = await edit(env, { display_name: name });
      assert.equal(r.status, 400, name);
      assert.equal(r.body.code, 'reserved_name', name);
      assert.ok(!JSON.stringify(r.body).includes(name), 'the answer does not repeat the name');
    }
    for (const name of ['other', '0ther', 'o.t.h.e.r', '\uFF4F\uFF54\uFF48\uFF45\uFF52', 'o\u200Bther', '\u043Ether', 'Big Ben']) {
      newHour(env);
      const r = await edit(env, { display_name: name });
      assert.equal(r.status, 400, name);
      assert.equal(r.body.code, 'name_is_a_username', name);
    }
  } finally {
    Object.assign(console, saved);
  }
  for (const line of logged) assert.ok(!/support|4dm1n|official|squad|0ther|o\.t\.h|big ben/i.test(line), `logged: ${line.slice(0, 80)}`);
  assert.equal(await env.USERS.get(profileKey(ACCOUNT)), null, 'nothing saved');

  newHour(env);
  assert.equal((await edit(env, { display_name: 'Owner' })).status, 200, 'your own username, in any case, is yours');
  newHour(env);
  assert.equal((await edit(env, { display_name: 'Modeler of Bridges' })).status, 200, 'a word that only contains a reserved one is fine');
});

test('edits are limited to 10 an hour for each account', async () => {
  const env = await environment();
  for (let i = 0; i < PROFILE_LIMITS.edits_per_hour; i++) {
    assert.equal((await edit(env, { bio: `edit ${i}` })).status, 200, `edit ${i}`);
  }
  const over = await edit(env, { bio: 'one more' });
  assert.equal(over.status, 429);
  assert.equal(over.body.code, 'rate_limited');
  assert.equal(over.headers.get('Retry-After'), '3600');
  assert.equal((await pendingProfileOf(env, ACCOUNT)).bio, 'edit 9', 'the refused edit saved nothing');
  // A refused request counts too, so a loop of bad requests is bounded the same way.
  assert.equal((await edit(env, { display_name: 'x'.repeat(40) })).status, 429);
  // Another account has its own hour.
  assert.equal((await edit(env, { bio: 'mine' }, { auth: mintToken(OTHER) })).status, 200);
  // The next hour, the owner may edit again.
  newHour(env);
  assert.equal((await edit(env, { bio: 'next hour' })).status, 200);
  assert.ok([...env.USERS.store.keys()].some((k) => k.startsWith(`profile-edits:${ACCOUNT}:`)));
});

test('no session, a made-up session, and an account that must verify its identity cannot edit', async () => {
  const env = await environment();
  assert.equal((await edit(env, { bio: 'x' }, { auth: null })).status, 401);
  assert.equal((await edit(env, { bio: 'x' }, { auth: 'not-a-token' })).status, 401);
  const early = await edit(env, { display_name: 'Ada' }, { auth: mintToken(UNVERIFIED) });
  assert.equal(early.status, 403, JSON.stringify(early.body));
  assert.equal(early.body.code, 'kyc_required');
  assert.equal(await readProfile(env, UNVERIFIED), null, 'nothing saved');
  assert.equal((await shown(env, 'early')).display_name, 'early');

  // The handler refuses it on its own too, should the route ever be reclassed past the gate in front of it.
  const json = (body, status) => new Response(JSON.stringify(body), { status });
  const direct = await handleProfileRoute(
    new Request(`${ORIGIN}/api/account/profile`, { method: 'POST', body: JSON.stringify({ bio: 'x' }) }),
    new URL(`${ORIGIN}/api/account/profile`), env, { authenticate: async () => UNVERIFIED, json, cors: {} });
  assert.equal(direct.status, 403);
  assert.equal((await direct.json()).code, 'kyc_required');
});

test('a bad request is refused whole and saves nothing', async () => {
  const env = await environment();
  const cases = [
    [{ display_name: 'x'.repeat(33) }, 'too_long'],
    [{ bio: 'y'.repeat(281) }, 'too_long'],
    [{ avatar_url: 'https://api.eustress.dev/api/simulations/abc/thumbnail' }, 'bad_avatar'],
    [{ avatar_url: 'https://evil.example/a.png' }, 'bad_avatar'],
    [{ avatar_url: 'javascript:alert(1)' }, 'bad_avatar'],
    [{ display_name: 7 }, 'bad_field'],
    [{ bio: ['a'] }, 'bad_field'],
    [{ display_name: 'Fine', avatar_url: 'https://eustress.dev/a.png' }, 'bad_avatar'],
    [{}, 'nothing_to_change'],
    [{ unknown: 'x' }, 'nothing_to_change'],
  ];
  for (const [body, code] of cases) {
    newHour(env);
    const r = await edit(env, body);
    assert.equal(r.status, 400, JSON.stringify(body));
    assert.equal(r.body.code, code, JSON.stringify(body));
  }
  for (const raw of ['not json', '[1,2]', 'null', JSON.stringify({ bio: 'z'.repeat(9000) })]) {
    newHour(env);
    const r = await edit(env, null, { raw });
    assert.equal(r.status, 400, raw.slice(0, 20));
    assert.equal(r.body.code, 'bad_body', raw.slice(0, 20));
  }
  assert.equal(await env.USERS.get(profileKey(ACCOUNT)), null, 'no refusal wrote anything');
});

test('what a visitor reads is checked again on the way out, saved or legacy', async () => {
  const env = await environment();
  // A record written before these rules, or by hand.
  await env.USERS.put(profileKey(ACCOUNT), JSON.stringify({ display_name: 'Ad\u202Ea', bio: 'b'.repeat(400), avatar_url: 'https://cdn.eustress.dev/x.png' }));
  const saved = await publicProfileOf(env, ACCOUNT, { username: 'owner' });
  assert.equal(saved.display_name, 'Ada');
  assert.equal(saved.bio, '', 'an over-long saved bio reads as none');
  assert.equal(saved.avatar_url, null, 'an address saved before the rule is not shown');
  // A name a later rule reserves stops being shown: the username shows instead.
  await env.USERS.put(profileKey(ACCOUNT), JSON.stringify({ display_name: 'Eustress Staff', bio: '', avatar_url: '' }));
  assert.equal((await shown(env)).display_name, 'owner');
  // No saved profile: the account record's own fields, by the same rules.
  const legacy = await publicProfileOf(env, OTHER, { username: 'other', bio: 'hi\u0000\u200B there', avatar_url: 'https://tracker.example/p.gif' });
  assert.deepEqual(legacy, { display_name: 'other', bio: 'hi there', avatar_url: null });
  const legacyOk = await publicProfileOf(env, OTHER, { username: 'other', avatar_url: '/assets/icons/noob-head.svg' });
  assert.equal(legacyOk.avatar_url, '/assets/icons/noob-head.svg');
});
