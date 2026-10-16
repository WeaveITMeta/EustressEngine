// A listing carries tags beside its one primary genre: free-form hashtags (#zombies #tycoon #coop), at most 8, each a
// lowercase word of letters and digits with single hyphens between them. They come in on create
// (POST /api/simulations/publish) and in the listing of every whole-Universe commit (world.mjs). A bad tag refuses the
// whole request with code bad_tags and writes nothing. Tags are text a reviewer reads, so changing them sends an approved
// listing back to review. The gallery filters on `?tag=` (every tag asked for), searches them, and offers the most used
// ones. docs/networking/LISTING_TAGS.md. The create, list and popular-tags tests run the real Worker; the commit tests run
// the world route with the same fakes tests/max_players.test.mjs uses.
//
// Source note: no backslash-u escape appears in this file on purpose (a tool that edits it can decode one into the
// character itself), so every non-ASCII character below is built from its code point.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { handleWorldRoute } from '../src/world.mjs';
import { isListable, canServe, buildJevState, claudeJudge } from '../src/moderation.mjs';
import { fakeClaude } from './helpers/claude_fake.mjs';
import { LiveHost } from '../src/live.mjs';
import { routePolicy, PUBLIC } from '../src/route_policy.mjs';
import { MAX_TAGS, TAG_MIN, TAG_MAX, TAG_RULES, normalizeTag, checkTags, listingTags, tagsForReview, sameTagSet, tagFilter, queryMatchesTag } from '../src/tags.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const author = mintToken(AUTHOR);

const fullWidth = (text) => [...text].map((c) => String.fromCodePoint(c.codePointAt(0) + 0xfee0)).join('');
const ROCKET = String.fromCodePoint(0x1f680);
const E_ACUTE = String.fromCodePoint(0xe9);
const CYRILLIC_A = String.fromCodePoint(0x430);
const SHARP_S = String.fromCodePoint(0xdf);
const DOTTED_I = String.fromCodePoint(0x130);
const KELVIN = String.fromCodePoint(0x212a);
const NBSP = String.fromCodePoint(0xa0);
const IDEOGRAPHIC_SPACE = String.fromCodePoint(0x3000);
const RLO = String.fromCodePoint(0x202e);
const LINE_SEPARATOR = String.fromCodePoint(0x2028);
const ZERO_WIDTH_SPACE = String.fromCodePoint(0x200b);
const LONE_SURROGATE = String.fromCharCode(0xd83d);
const BACKSLASH = String.fromCharCode(92);
const ESC = (code) => `${BACKSLASH}u${code.toString(16).padStart(4, '0')}`;
const ELLIPSIS = String.fromCharCode(0x2026);

// ── the rule ─────────────────────────────────────────────────────────────────

test('the limits are 8 tags of 2 to 24 characters', () => {
  assert.equal(MAX_TAGS, 8);
  assert.equal(TAG_MIN, 2);
  assert.equal(TAG_MAX, 24);
  assert.match(TAG_RULES, /2 to 24 characters/);
  assert.match(TAG_RULES, /at most 8 tags/);
});

test('a tag is trimmed, lower-cased and loses one leading #', () => {
  assert.equal(normalizeTag('zombies'), 'zombies');
  assert.equal(normalizeTag('Zombies'), 'zombies');
  assert.equal(normalizeTag('ZOMBIES'), 'zombies');
  assert.equal(normalizeTag('#zombies'), 'zombies');
  assert.equal(normalizeTag('#Zombies'), 'zombies');
  assert.equal(normalizeTag('  #Tycoon  '), 'tycoon');
  assert.equal(normalizeTag('\t\n#coop \n'), 'coop');
  assert.equal(normalizeTag(NBSP + 'coop' + NBSP), 'coop', 'a no-break space is whitespace');
  assert.equal(normalizeTag(IDEOGRAPHIC_SPACE + 'coop'), 'coop', 'so is an ideographic space');
});

test('only one leading # is removed, and nothing else is stripped', () => {
  assert.equal(normalizeTag('##zombies'), null);
  assert.equal(normalizeTag('# zombies'), null, 'the # goes, and the space it leaves behind is not a valid tag');
  assert.equal(normalizeTag('zombies#'), null);
  assert.equal(normalizeTag('zom#bies'), null);
  assert.equal(normalizeTag('#'), null, 'only a #');
  assert.equal(normalizeTag('  #  '), null);
  assert.equal(normalizeTag(''), null);
  assert.equal(normalizeTag('   '), null);
});

test('Unicode NFKC folds full-width letters, digits, hyphens and # to ASCII', () => {
  assert.equal(normalizeTag(fullWidth('zombies')), 'zombies');
  assert.equal(normalizeTag(fullWidth('Zombies')), 'zombies');
  assert.equal(normalizeTag(fullWidth('#coop')), 'coop', 'a full-width # is a #');
  assert.equal(normalizeTag(fullWidth('co-op')), 'co-op', 'a full-width hyphen is a hyphen');
  assert.equal(normalizeTag(fullWidth('3d2')), '3d2');
  assert.equal(normalizeTag(`${KELVIN}ids`), 'kids', 'the Kelvin sign folds to k');
});

test('hyphens go between letters or digits only', () => {
  for (const ok of ['co-op', 'a-b', 'co-op-2', 'sci-fi-horror', '1-2-3', '#co-op']) assert.ok(normalizeTag(ok), ok);
  for (const bad of ['-coop', 'coop-', '-coop-', 'co--op', 'co---op', '--', '-', 'a--b', '#-coop', 'coop-#']) assert.equal(normalizeTag(bad), null, bad);
});

test('a tag is 2 to 24 characters after the # is removed', () => {
  assert.equal(normalizeTag('a'), null, '1 character');
  assert.equal(normalizeTag('#a'), null, '1 character behind a #');
  assert.equal(normalizeTag('ab'), 'ab', '2 characters');
  assert.equal(normalizeTag('#ab'), 'ab');
  assert.equal(normalizeTag('a'.repeat(24)), 'a'.repeat(24), '24 characters');
  assert.equal(normalizeTag('#' + 'a'.repeat(24)), 'a'.repeat(24), 'the # does not count');
  assert.equal(normalizeTag('a'.repeat(25)), null, '25 characters');
  assert.equal(normalizeTag('#' + 'a'.repeat(25)), null);
  assert.equal(normalizeTag('abcdefghijk-abcdefghijk'), 'abcdefghijk-abcdefghijk', 'hyphens count as characters: 23');
  assert.equal(normalizeTag('abcdefghijkl-abcdefghijkl'), null, '25 with the hyphen');
});

test('digits are fine on their own and mixed in', () => {
  for (const ok of ['2026', '3d', '1v1', 'top10', '007', '0-0', '24h']) assert.equal(normalizeTag(ok), ok);
});

test('letters outside a to z, emoji, underscores, spaces and punctuation are refused', () => {
  const refused = [
    `caf${E_ACUTE}`, `${CYRILLIC_A}bc`, `stra${SHARP_S}e`, `${DOTTED_I}stanbul`, ROCKET, `rocket${ROCKET}`, `${ROCKET}${ROCKET}`,
    'sci_fi', '_scifi', 'sci fi', 'sci\tfi', 'sci\nfi', 'sci.fi', 'sci/fi', 'sci+fi', 'c++', 'c#', 'a@b', 'what?', 'hi!', "it's", 'say"hi"',
    '<b>', '<script>', 'a,b', 'a;b', 'a:b', 'a(b)', 'a*b', 'a%20b', 'a' + String.fromCharCode(0) + 'b', ZERO_WIDTH_SPACE + 'coop', 'co' + ZERO_WIDTH_SPACE + 'op',
  ];
  for (const bad of refused) assert.equal(normalizeTag(bad), null, JSON.stringify(bad));
});

test('a value that is not a string is no tag', () => {
  for (const bad of [5, 0, null, undefined, true, false, {}, [], ['coop'], { tag: 'coop' }, () => 'coop', Symbol('coop')]) assert.equal(normalizeTag(bad), null, String(typeof bad));
});

// ── checkTags ────────────────────────────────────────────────────────────────

test('a list is normalized, de-duplicated and kept in the order it was sent', () => {
  const r = checkTags(['Zombies', '#zombies', 'ZOMBIES', 'tycoon', 'zombies', ' #Coop ', fullWidth('tycoon')]);
  assert.deepEqual(r, { ok: true, tags: ['zombies', 'tycoon', 'coop'] });
  assert.deepEqual(checkTags(['b2', 'a1', 'c3']).tags, ['b2', 'a1', 'c3'], 'order is the author\'s, not alphabetical');
});

test('an empty list is valid', () => {
  assert.deepEqual(checkTags([]), { ok: true, tags: [] });
});

test('at most 8 tags: 8 pass, 9 are refused with the limit in the message', () => {
  const nine = ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8', 't9'];
  assert.deepEqual(checkTags(nine.slice(0, 8)), { ok: true, tags: nine.slice(0, 8) });
  assert.deepEqual(checkTags(nine.slice(0, 7)), { ok: true, tags: nine.slice(0, 7) });
  const r = checkTags(nine);
  assert.equal(r.ok, false);
  assert.match(r.error, /at most 8 tags/);
  assert.match(r.error, /names 9/);
  // The limit is on tags after duplicates are dropped.
  assert.equal(checkTags([...nine.slice(0, 8), 'T1', '#t2', 't3']).ok, true, 'duplicates do not count');
  assert.equal(checkTags([...nine, 'dup', 'dup']).ok, false);
});

test('a list far past the limit is refused without reading it', () => {
  const huge = Array.from({ length: 300 }, () => 'ab');
  const r = checkTags(huge);
  assert.equal(r.ok, false);
  assert.match(r.error, /at most 8 tags/);
  assert.match(r.error, /300 entries/);
});

test('one bad tag refuses the whole list, and the message names the first bad one', () => {
  const r = checkTags(['good', 'b@d', 'w^rse', 'fine']);
  assert.equal(r.ok, false);
  assert.match(r.error, /^Tag "b@d" is not a valid tag\./);
  assert.ok(!r.error.includes('w^rse'), 'only the first is named');
  // The first bad tag wins over the count, so an author fixes the tag, then the count.
  const many = ['a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 'a8', 'a9', 'b@d'];
  assert.match(checkTags(many).error, /^Tag "b@d" is not a valid tag\./);
});

test('the refusal states the rules', () => {
  for (const bad of [['b@d'], ['a'], ['a'.repeat(25)], 'zombies', [5]]) {
    const r = checkTags(bad);
    assert.equal(r.ok, false, JSON.stringify(bad));
    assert.ok(r.error.endsWith(TAG_RULES), JSON.stringify(bad));
  }
  assert.doesNotMatch(TAG_RULES, new RegExp(String.fromCharCode(0x2014)), 'no em dash');
});

test('a value that is not a list of strings is refused', () => {
  for (const bad of ['zombies', 'zombies,coop', 5, true, {}, { 0: 'coop' }, null, undefined]) {
    const r = checkTags(bad);
    assert.equal(r.ok, false, JSON.stringify(bad));
    assert.match(r.error, /^tags must be a list of strings\./);
  }
  const entries = [[[5], /Entry 1 of tags must be a string, not a number/], [['ok', null], /Entry 2 .* not null/], [['ok', ['x']], /Entry 2 .* not a list/], [[{}], /Entry 1 .* not a object/], [[undefined], /Entry 1 .* not a undefined/], [['ok', true], /Entry 2 .* not a boolean/]];
  for (const [bad, pattern] of entries) {
    const r = checkTags(bad);
    assert.equal(r.ok, false, JSON.stringify(bad));
    assert.match(r.error, pattern);
  }
});

test('a refused tag is repeated at most 40 characters, escaped, never raw', () => {
  const ten_kb = 'x'.repeat(10_000);
  const long = checkTags([ten_kb]);
  assert.equal(long.ok, false);
  assert.ok(long.error.includes(`"${'x'.repeat(40)}${ELLIPSIS}"`), 'the first 40 and an ellipsis');
  assert.ok(!long.error.includes('x'.repeat(41)), 'never more than 40');
  assert.ok(long.error.length < 400, `a short message: ${long.error.length}`);

  const exactly40 = checkTags(['x'.repeat(40) + '!']);
  assert.ok(exactly40.error.includes(`"${'x'.repeat(40)}${ELLIPSIS}"`), '41 characters: 40 and an ellipsis');
  const just40 = checkTags(['y'.repeat(39) + '!']);
  assert.ok(just40.error.includes(`"${'y'.repeat(39)}!"`), '40 characters: all, no ellipsis');

  const script = checkTags(['<script>alert("x")</script>']);
  assert.ok(!script.error.includes('<'), 'no raw <');
  assert.ok(!script.error.includes('>'), 'no raw >');
  assert.ok(!script.error.includes('<script>'));
  assert.ok(script.error.includes(`${ESC(0x3c)}script${ESC(0x3e)}alert(${ESC(0x22)}x${ESC(0x22)})`), script.error);

  assert.ok(checkTags(['a"b']).error.includes(`Tag "a${ESC(0x22)}b"`), 'a double quote cannot close the quotes around the tag');
  assert.ok(checkTags(["it's"]).error.includes(`it${ESC(0x27)}s`));
  assert.ok(checkTags(['a&b']).error.includes(`a${ESC(0x26)}b`));
  assert.ok(checkTags([`a${BACKSLASH}b`]).error.includes(`a${ESC(0x5c)}b`));
  assert.ok(checkTags(['a`b']).error.includes(`a${ESC(0x60)}b`));
  assert.ok(checkTags(['a\nb']).error.includes(`a${ESC(0x0a)}b`), 'a newline');
  assert.ok(checkTags(['a' + String.fromCharCode(0) + 'b']).error.includes(`a${ESC(0)}b`), 'a NUL');
  assert.ok(checkTags([`a${RLO}b`]).error.includes(`a${ESC(0x202e)}b`), 'a direction override cannot reorder the message');
  assert.ok(checkTags([`a${LINE_SEPARATOR}b`]).error.includes(`a${ESC(0x2028)}b`));
  assert.ok(checkTags([`a${ZERO_WIDTH_SPACE}b`]).error.includes(`a${ESC(0x200b)}b`), 'a zero-width space is shown');
  assert.ok(checkTags([`a${LONE_SURROGATE}b`]).error.includes(`a${ESC(0xd83d)}b`), 'a lone surrogate');
  const hazard = new RegExp(`[${[[0, 0x1f], [0x7f, 0x9f], [0x200b, 0x200f], [0x2028, 0x202e], [0xfeff, 0xfeff]].map(([a, b]) => `${String.fromCharCode(a)}-${String.fromCharCode(b)}`).join('')}]`);
  assert.ok(!hazard.test(script.error + checkTags([`a${RLO}b`]).error), 'no control or direction character survives');
  // Emoji are not hazards: they stay readable.
  assert.ok(checkTags([`rocket${ROCKET}`]).error.includes(`"rocket${ROCKET}"`));
  assert.ok(checkTags([`${ROCKET.repeat(60)}`]).error.includes(`"${ROCKET.repeat(40)}${ELLIPSIS}"`), 'counted in characters, not code units');
});

test('what a message shows of a hostile list is JSON-safe', () => {
  for (const hostile of ['<script>', '"; DROP TABLE', ROCKET.repeat(5000), 'x'.repeat(10_000), `${LONE_SURROGATE}${RLO}\n\r\t`, '%s%s%s', '${x}', '{{x}}']) {
    const r = checkTags([hostile]);
    assert.equal(r.ok, false);
    assert.equal(JSON.parse(JSON.stringify({ error: r.error })).error, r.error);
    assert.ok(r.error.length < 900, `${hostile.slice(0, 12)} -> ${r.error.length}`);
  }
});

// ── helpers that read a record ───────────────────────────────────────────────

test('a listing from before tags reads as no tags, and a stored list reads as itself', () => {
  assert.deepEqual(listingTags({}), []);
  assert.deepEqual(listingTags(undefined), []);
  assert.deepEqual(listingTags({ tags: null }), []);
  assert.deepEqual(listingTags({ tags: 'zombies' }), []);
  assert.deepEqual(listingTags({ tags: ['zombies', 'coop'] }), ['zombies', 'coop']);
  assert.deepEqual(listingTags({ tags: ['zombies', 5, null, 'coop'] }), ['zombies', 'coop']);
});

test('the same tags in another order are the same set', () => {
  assert.equal(sameTagSet(['a1', 'b2'], ['b2', 'a1']), true);
  assert.equal(sameTagSet(['a1'], ['a1', 'b2']), false);
  assert.equal(sameTagSet(['a1', 'b2'], ['a1']), false);
  assert.equal(sameTagSet(['a1', 'b2'], ['a1', 'c3']), false);
  assert.equal(sameTagSet([], []), true);
  assert.equal(sameTagSet([], ['a1']), false);
});

test('a gallery request names tags that are normalized like stored ones', () => {
  assert.deepEqual(tagFilter(['Zombies', '#coop', 'zombies']), { none: false, tags: ['zombies', 'coop'] });
  assert.deepEqual(tagFilter([]), { none: false, tags: [] });
  assert.deepEqual(tagFilter(null), { none: false, tags: [] });
  assert.deepEqual(tagFilter(['', '   ']), { none: false, tags: [] }, 'an empty tag= asks nothing');
  for (const bad of [['bad_tag'], ['zombies', 'bad_tag'], ['<script>'], ['#'], ['a'], ['a b'], ['x'.repeat(10_000)], [ROCKET]]) assert.deepEqual(tagFilter(bad), { none: true, tags: [] }, JSON.stringify(bad).slice(0, 40));
});

test('a free-text search matches part of a tag, with or without the #', () => {
  assert.equal(queryMatchesTag('zomb', ['zombies', 'coop']), true);
  assert.equal(queryMatchesTag('#zomb', ['zombies']), true);
  assert.equal(queryMatchesTag('COOP', ['coop']), true);
  assert.equal(queryMatchesTag('co-', ['co-op']), true);
  assert.equal(queryMatchesTag(fullWidth('zomb'), ['zombies']), true);
  assert.equal(queryMatchesTag('tycoon', ['zombies', 'coop']), false);
  assert.equal(queryMatchesTag('zombies', []), false);
  assert.equal(queryMatchesTag('#', ['zombies']), false, 'a lone # matches nothing');
  assert.equal(queryMatchesTag('', ['zombies']), false);
});

// ── create ───────────────────────────────────────────────────────────────────

async function seededEnv() {
  const env = fakeEnv();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'author', created_at: '2026-01-01T00:00:00.000Z' }));
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  return env;
}

async function create(env, body) {
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/simulations/publish', {
    method: 'POST', headers: { Authorization: `Bearer ${author}`, 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  }), env, ctx);
  const text = await res.text();
  let body_ = null;
  try { body_ = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: body_, text };
}

const simCount = async (env) => Number((await env.SOCIAL.get(`simCount:${AUTHOR}`)) || 0);
const simKeys = (env) => [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('sim:') || k.startsWith('sim-author:'));

test('create stores the tags normalized, in order, de-duplicated, and returns them', async () => {
  const env = await seededEnv();
  const made = await create(env, { name: 'Zombie Yard', genre: 'survival', tags: ['#Zombies', 'coop', 'ZOMBIES', ' Tycoon '] });
  assert.equal(made.status, 201, made.text);
  assert.deepEqual(made.body.tags, ['zombies', 'coop', 'tycoon']);
  assert.deepEqual(JSON.parse(await env.SOCIAL.get(`sim:${made.body.id}`)).tags, ['zombies', 'coop', 'tycoon'], 'and what is stored is the same');
  assert.equal(made.body.genre, null, 'tags are the only category: a genre sent with tags is ignored');
  assert.equal(made.body.tags_source, 'author');
});

test('create without tags stores an empty list, and so does null and []', async () => {
  const env = await seededEnv();
  for (const body of [{ name: 'None' }, { name: 'Null', tags: null }, { name: 'Empty', tags: [] }]) {
    const r = await create(env, body);
    assert.equal(r.status, 201, r.text);
    assert.deepEqual(r.body.tags, [], JSON.stringify(body));
    assert.deepEqual(JSON.parse(await env.SOCIAL.get(`sim:${r.body.id}`)).tags, [], JSON.stringify(body));
  }
});

test('create accepts exactly 8 tags and the edges of the length range', async () => {
  const env = await seededEnv();
  const eight = ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'];
  const r = await create(env, { name: 'Eight', tags: eight });
  assert.equal(r.status, 201, r.text);
  assert.deepEqual(r.body.tags, eight);
  const edges = await create(env, { name: 'Edges', tags: ['ab', 'a'.repeat(24)] });
  assert.equal(edges.status, 201, edges.text);
});

test('create refuses a bad tag with its name and the rules, and writes nothing', async () => {
  const env = await seededEnv();
  const refused = [
    [['zombies', 'sci_fi'], /Tag "sci_fi" is not a valid tag/],
    [['a'], /Tag "a" is not a valid tag/],
    [['a'.repeat(25)], /is not a valid tag/],
    [[ROCKET], /is not a valid tag/],
    [['co--op'], /Tag "co--op" is not a valid tag/],
    [['#'], /Tag "#" is not a valid tag/],
    [['ok', 'sci fi'], /Tag "sci fi" is not a valid tag/],
    [['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8', 't9'], /at most 8 tags/],
    [[5], /Entry 1 of tags must be a string/],
    ['zombies', /tags must be a list of strings/],
    [{ 0: 'zombies' }, /tags must be a list of strings/],
    [true, /tags must be a list of strings/],
  ];
  for (const [tags, pattern] of refused) {
    const r = await create(env, { name: 'Refused', tags });
    assert.equal(r.status, 400, JSON.stringify(tags));
    assert.equal(r.body.code, 'bad_tags', JSON.stringify(tags));
    assert.match(r.body.error, pattern);
    assert.ok(r.body.error.endsWith(TAG_RULES), 'states the rules');
  }
  assert.equal(await simCount(env), 0, 'no listing was counted');
  assert.deepEqual(simKeys(env), [], 'and none was made');
});

test('create never repeats a hostile tag unescaped, and a 10 KB tag costs a short answer', async () => {
  const env = await seededEnv();
  for (const tag of ['<script>alert(1)</script>', '"><img src=x onerror=alert(1)>', "';--", `${RLO}evil`, 'x'.repeat(10_240)]) {
    const r = await create(env, { name: 'Hostile', tags: [tag] });
    assert.equal(r.status, 400);
    assert.equal(r.body.code, 'bad_tags');
    assert.ok(!r.text.includes('<script>'), 'no raw script tag in the body');
    assert.ok(!r.text.includes('<img'), 'no raw markup in the body');
    assert.ok(!r.body.error.includes('<'), 'no raw < in the message');
    assert.ok(!r.body.error.includes(RLO), 'no raw direction override in the message');
    assert.ok(!r.body.error.includes('x'.repeat(41)), 'never more than 40 characters of it');
    assert.ok(r.text.length < 1200, `a short answer: ${r.text.length}`);
  }
  assert.deepEqual(simKeys(env), []);
});

test('create checks the tags before the author\'s publish count is touched, with the player count beside them', async () => {
  const env = await seededEnv();
  const both = await create(env, { name: 'Both', max_players: 4, tags: ['coop'] });
  assert.equal(both.status, 201, both.text);
  assert.equal(both.body.max_players, 4);
  assert.deepEqual(both.body.tags, ['coop']);
  const badTags = await create(env, { name: 'Bad', max_players: 4, tags: ['no way'] });
  assert.equal(badTags.status, 400);
  assert.equal(badTags.body.code, 'bad_tags');
  assert.equal(await simCount(env), 1, 'only the first listing was made');
});

// ── commit ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return { m, get: async (k) => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); } };
}

function r2() {
  const m = new Map();
  const bytesOf = async (value) => {
    if (value instanceof Uint8Array) return value;
    if (typeof value === 'string') return new TextEncoder().encode(value);
    return new Uint8Array(await new Response(value).arrayBuffer());
  };
  return {
    m,
    async put(key, value) { m.set(key, await bytesOf(value)); return { key }; },
    async head(key) { const b = m.get(key); return b ? { key, size: b.length } : null; },
    async get(key) { const b = m.get(key); return b ? { size: b.length, body: new Response(b).body, arrayBuffer: async () => b.slice().buffer, text: async () => new TextDecoder().decode(b) } : null; },
    async delete(key) { m.delete(key); },
    async list({ prefix = '' } = {}) { return { objects: [...m.keys()].filter((k) => k.startsWith(prefix)).map((key) => ({ key, size: m.get(key).length })), truncated: false }; },
  };
}

const json = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const deps = {
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace(/^Bearer /, '') || null,
  requireAdmin: async () => null,
  json, isListable, canServe,
  readSpaceNames: (list) => (Array.isArray(list) ? [...new Set(list)] : null),
  MODERATION_VERSION: '1.0', POLICY_VERSION: '1.2',
};

const SIM = '11111111-2222-3333-4444-555555555555';
const H1 = 'a'.repeat(64);
function chunk() {
  const path = new TextEncoder().encode('Workspace/P/_instance.toml');
  const data = new TextEncoder().encode('[metadata]\nclass_name = "Part"\n');
  const out = new Uint8Array(12 + 8 + path.length + data.length);
  const view = new DataView(out.buffer);
  out.set([0x45, 0x43, 0x48, 0x4b]);
  view.setUint32(4, 1, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, path.length, true);
  out.set(path, 16);
  view.setUint32(16 + path.length, data.length, true);
  out.set(data, 20 + path.length);
  return out;
}
const C1 = chunk();
const manifest = { format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Box Head', start_space: 'Main', spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: C1.length, count: 1, blake3: H1 }] }], assets: [] };

async function worldEnv(sim = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2() };
  env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Box Head', author_id: 'author', is_public: true, version: 1, max_players: 10, moderation: { status: 'pending' }, ...sim }));
  await env.SCENES.put(`universes/${SIM}/chunks/${H1}.echk`, C1);
  return env;
}
const stored = (env) => JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
async function commit(env, listing) {
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/world/commit`);
  const body = JSON.stringify({ manifest_json: JSON.stringify(manifest), ...(listing === undefined ? {} : { listing }) });
  const res = await handleWorldRoute(new Request(url, { method: 'POST', headers: { Authorization: 'Bearer author', 'Content-Type': 'application/json' }, body }), url, env, {}, deps);
  return { status: res.status, body: await res.json() };
}
const approve = (env) => env.SOCIAL.m.set(`sim:${SIM}`, JSON.stringify({ ...stored(env), moderation: { status: 'approved' } }));

/// A listing that was committed once and approved, carrying `tags`: the next commit is the one a test looks at.
async function approvedWith(tags) {
  const env = await worldEnv();
  const first = await commit(env, { name: 'Box Head', description: '', ...(tags === undefined ? {} : { tags }) });
  assert.equal(first.status, 200, JSON.stringify(first.body));
  approve(env);
  assert.equal(stored(env).moderation.status, 'approved');
  return env;
}

test('a whole-Universe commit stores the tags in its listing, normalized', async () => {
  const env = await worldEnv();
  const r = await commit(env, { name: 'Box Head', tags: ['#Zombies', 'coop', 'ZOMBIES'] });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.deepEqual(stored(env).tags, ['zombies', 'coop']);
});

test('a commit without tags leaves them as they were, and so does null and no listing at all', async () => {
  const env = await worldEnv({ tags: ['zombies', 'coop'] });
  assert.equal((await commit(env, { name: 'Box Head' })).status, 200);
  assert.deepEqual(stored(env).tags, ['zombies', 'coop'], 'not sent: unchanged');
  assert.equal((await commit(env)).status, 200);
  assert.deepEqual(stored(env).tags, ['zombies', 'coop'], 'no listing at all: unchanged');
  assert.equal((await commit(env, { tags: null })).status, 200);
  assert.deepEqual(stored(env).tags, ['zombies', 'coop'], 'null reads as not sent');
});

test('an empty list clears the tags', async () => {
  const env = await worldEnv({ tags: ['zombies', 'coop'] });
  assert.equal((await commit(env, { tags: [] })).status, 200);
  assert.deepEqual(stored(env).tags, []);
});

test('a commit stores 8 tags and refuses 9', async () => {
  const env = await worldEnv();
  const eight = ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'];
  assert.equal((await commit(env, { tags: eight })).status, 200);
  assert.deepEqual(stored(env).tags, eight);
  const r = await commit(env, { tags: [...eight, 't9'] });
  assert.equal(r.status, 400);
  assert.equal(r.body.code, 'bad_tags');
  assert.match(r.body.error, /at most 8 tags/);
  assert.deepEqual(stored(env).tags, eight, 'the stored tags are untouched');
});

test('a bad tag refuses the whole commit, names the tag and the rules, before anything is written', async () => {
  const env = await worldEnv({ tags: ['zombies'], moderation: { status: 'approved' } });
  const before = env.SOCIAL.m.get(`sim:${SIM}`);
  const objectsBefore = env.SCENES.m.size;
  const refused = [
    [['good', 'sci_fi'], /Tag "sci_fi" is not a valid tag/],
    [['a'], /Tag "a" is not a valid tag/],
    [['a'.repeat(25)], /is not a valid tag/],
    [[ROCKET], /is not a valid tag/],
    [['-coop'], /Tag "-coop" is not a valid tag/],
    [['coop-'], /Tag "coop-" is not a valid tag/],
    [['co--op'], /Tag "co--op" is not a valid tag/],
    [['#'], /Tag "#" is not a valid tag/],
    [['##coop'], /Tag "##coop" is not a valid tag/],
    ['coop', /tags must be a list of strings/],
    [[7], /Entry 1 of tags must be a string/],
  ];
  for (const [tags, pattern] of refused) {
    const r = await commit(env, { name: 'Renamed', description: 'New words', max_players: 6, tags });
    assert.equal(r.status, 400, JSON.stringify(tags));
    assert.equal(r.body.code, 'bad_tags', JSON.stringify(tags));
    assert.match(r.body.error, pattern);
    assert.ok(r.body.error.endsWith(TAG_RULES), 'states the rules');
  }
  assert.equal(env.SOCIAL.m.get(`sim:${SIM}`), before, 'the listing is byte for byte as it was: not even the name changed');
  assert.equal(env.SCENES.m.size, objectsBefore, 'and no manifest was stored');
});

test('a hostile tag in a commit is refused and never echoed raw', async () => {
  const env = await worldEnv({ tags: ['zombies'] });
  const before = env.SOCIAL.m.get(`sim:${SIM}`);
  for (const tag of ['<script>alert(1)</script>', '"><svg onload=alert(1)>', 'x'.repeat(10_240)]) {
    const r = await commit(env, { tags: [tag] });
    assert.equal(r.status, 400);
    assert.equal(r.body.code, 'bad_tags');
    const text = JSON.stringify(r.body);
    assert.ok(!text.includes('<script>') && !text.includes('<svg'), 'no raw markup');
    assert.ok(!r.body.error.includes('<'), 'no raw <');
    assert.ok(!r.body.error.includes('x'.repeat(41)), 'never more than 40 characters of it');
    assert.ok(text.length < 1200, `a short answer: ${text.length}`);
  }
  assert.equal(env.SOCIAL.m.get(`sim:${SIM}`), before);
});

test('a bad tag is refused even when the player count beside it is fine, and a bad player count is still refused first', async () => {
  const env = await worldEnv();
  const bad = await commit(env, { max_players: 4, tags: ['no way'] });
  assert.equal(bad.status, 400);
  assert.equal(bad.body.code, 'bad_tags');
  assert.equal(stored(env).max_players, 10, 'the player count was not stored either');
  const players = await commit(env, { max_players: 0, tags: ['no way'] });
  assert.equal(players.status, 400);
  assert.equal(players.body.code, 'bad_max_players');
});

test('changing the tags sends an approved listing back to review', async () => {
  const env = await approvedWith(['zombies', 'coop']);
  // A tag added.
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies', 'coop', 'tycoon'] })).status, 200);
  assert.deepEqual(stored(env).tags, ['zombies', 'coop', 'tycoon']);
  assert.equal(stored(env).moderation.status, 'pending', 'a new tag is text a reviewer reads');
  assert.equal(stored(env).moderation.version, '1.0');
  // A tag removed.
  approve(env);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies', 'coop'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'pending', 'removing one is a change too');
  // A tag swapped for another of the same count.
  approve(env);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies', 'tycoon'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'pending');
  // Cleared.
  approve(env);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: [] })).status, 200);
  assert.equal(stored(env).moderation.status, 'pending', 'clearing them is a change');
});

test('the same tags again do not: spelled differently, in another order, or sent with nothing else changed', async () => {
  const env = await approvedWith(['zombies', 'coop']);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies', 'coop'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'the same tags');
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['#ZOMBIES', ' Coop ', 'coop'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'the same tags, spelled differently');
  assert.deepEqual(stored(env).tags, ['zombies', 'coop']);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['coop', 'zombies'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'reordering is not a change a reviewer reads again');
  assert.deepEqual(stored(env).tags, ['coop', 'zombies'], 'and the new order is stored');
  assert.equal((await commit(env, { name: 'Box Head', description: '' })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'no tags sent');
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: null })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'null');
});

test('a listing from before tags: an empty list is no change, and a first tag is', async () => {
  const env = await approvedWith(undefined);
  assert.equal(stored(env).tags, undefined, 'the fixture has no tags field');
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: [] })).status, 200);
  assert.equal(stored(env).moderation.status, 'approved', 'nothing to read that was not there before');
  assert.deepEqual(stored(env).tags, [], 'and it now carries the list');
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies'] })).status, 200);
  assert.equal(stored(env).moderation.status, 'pending');
});

test('a changed tag leaves the gallery: a listing in review is not listed', async () => {
  const env = await approvedWith(['zombies']);
  assert.equal(isListable(stored(env)), true);
  assert.equal((await commit(env, { name: 'Box Head', description: '', tags: ['zombies', 'tycoon'] })).status, 200);
  assert.equal(isListable(stored(env)), false, 'listable again only when the review approves it');
});

// ── what a reader sees ───────────────────────────────────────────────────────

let seq = 0;
/// A listing straight into storage. `tags: undefined` writes a listing from before tags existed.
async function seed(env, { name, tags, status = 'approved', isPublic = true, rating = 'all_ages', genre = 'all', authorName = 'Ann', description = '' }) {
  const id = `00000000-0000-4000-8000-${String(++seq).padStart(12, '0')}`;
  const sim = {
    id, name, description, genre, author_id: AUTHOR, author_name: authorName, is_public: isPublic,
    moderation: { status, rating }, published_at: new Date(1_700_000_000_000 + seq * 1000).toISOString(),
  };
  if (tags !== undefined) sim.tags = tags;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
  return id;
}

async function get(env, path, headers = {}) {
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { headers }), env, ctx);
  const text = await res.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body, text, headers: res.headers };
}
const names = (r) => r.body.simulations.map((s) => s.name).sort();

/// The gallery these tests filter: five listed, and four that are not.
async function galleryEnv() {
  const env = await seededEnv();
  await seed(env, { name: 'Alpha', tags: ['zombies', 'coop'], genre: 'survival' });
  await seed(env, { name: 'Bravo', tags: ['zombies', 'tycoon'] });
  await seed(env, { name: 'Charlie', tags: ['tycoon'], authorName: 'Zed Writer' });
  await seed(env, { name: 'Delta', tags: [], description: 'a quiet harbor' });
  await seed(env, { name: 'Echo', tags: undefined });
  await seed(env, { name: 'Foxtrot', tags: ['zombies', 'secret'], status: 'pending' });
  await seed(env, { name: 'Golf', tags: ['hidden', 'zombies'], isPublic: false });
  await seed(env, { name: 'Hotel', tags: ['rejected-tag'], status: 'rejected' });
  await seed(env, { name: 'India', tags: ['held-tag'], status: 'held' });
  return env;
}

test('the gallery card carries tags: always a list, empty for a listing from before', async () => {
  const env = await galleryEnv();
  const all = await get(env, '/api/simulations');
  assert.equal(all.status, 200);
  assert.deepEqual(names(all), ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo']);
  const byName = Object.fromEntries(all.body.simulations.map((s) => [s.name, s.tags]));
  // Alpha's genre from before tags (survival) reads as one more tag.
  assert.deepEqual(byName, { Alpha: ['zombies', 'coop', 'survival'], Bravo: ['zombies', 'tycoon'], Charlie: ['tycoon'], Delta: [], Echo: [] });
  for (const s of all.body.simulations) assert.ok(Array.isArray(s.tags), s.name);
});

test('one listing read by id carries its tags, listed or in review', async () => {
  const env = await seededEnv();
  const listed = await seed(env, { name: 'Alpha', tags: ['zombies', 'coop'] });
  const old = await seed(env, { name: 'Echo', tags: undefined });
  const pending = await seed(env, { name: 'Foxtrot', tags: ['zombies', 'secret'], status: 'pending' });
  assert.deepEqual((await get(env, `/api/simulations/${listed}`)).body.tags, ['zombies', 'coop']);
  assert.deepEqual((await get(env, `/api/simulations/${old}`)).body.tags, []);
  // In review, the author sees their own record, tags included; a stranger sees nothing.
  const own = await get(env, `/api/simulations/${pending}`, { Authorization: `Bearer ${author}` });
  assert.equal(own.status, 200, own.text);
  assert.deepEqual(own.body.tags, ['zombies', 'secret']);
  assert.equal(own.body.listable, false);
  assert.equal((await get(env, `/api/simulations/${pending}`)).status, 404, 'a stranger is told nothing, tags included');
});

test('the author\'s project list carries tags', async () => {
  const env = await seededEnv();
  await seed(env, { name: 'Alpha', tags: ['zombies'] });
  await seed(env, { name: 'Echo', tags: undefined, status: 'pending' });
  const r = await get(env, '/api/projects', { Authorization: `Bearer ${author}` });
  assert.equal(r.status, 200, r.text);
  const byName = Object.fromEntries(r.body.projects.map((p) => [p.name, p.tags]));
  assert.deepEqual(byName, { Alpha: ['zombies'], Echo: [] });
});

test('the listing create returns is the one the gallery serves once approved', async () => {
  const env = await seededEnv();
  const made = await create(env, { name: 'Zombie Yard', tags: ['#Zombies', 'Coop'] });
  assert.equal(made.status, 201, made.text);
  const sim = JSON.parse(await env.SOCIAL.get(`sim:${made.body.id}`));
  await env.SOCIAL.put(`sim:${made.body.id}`, JSON.stringify({ ...sim, moderation: { status: 'approved', rating: 'all_ages' } }));
  const list = await get(env, '/api/simulations?tag=zombies');
  assert.deepEqual(list.body.simulations.map((s) => [s.name, s.tags]), [['Zombie Yard', ['zombies', 'coop']]]);
});

// ── ?tag= ────────────────────────────────────────────────────────────────────

test('?tag= keeps the listed listings that carry the tag', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?tag=zombies')), ['Alpha', 'Bravo'], 'not Foxtrot (in review) or Golf (private)');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=tycoon')), ['Bravo', 'Charlie']);
  assert.deepEqual(names(await get(env, '/api/simulations?tag=coop')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/simulations?tag=nothing-uses-this')), []);
  const r = await get(env, '/api/simulations?tag=zombies');
  assert.equal(r.body.total, 2);
});

test('several ?tag= mean every one of them', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?tag=zombies&tag=coop')), ['Alpha'], 'both, not either');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=zombies&tag=tycoon')), ['Bravo']);
  assert.deepEqual(names(await get(env, '/api/simulations?tag=coop&tag=tycoon')), [], 'no listing has both');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=zombies&tag=zombies')), ['Alpha', 'Bravo'], 'the same tag twice is one tag');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=zombies&tag=coop&tag=tycoon')), []);
});

test('a ?tag= value is normalized like a stored tag', async () => {
  const env = await galleryEnv();
  for (const spelled of ['%23zombies', '%23ZOMBIES', 'Zombies', '%20zombies%20', '%20%23Zombies', encodeURIComponent(fullWidth('zombies'))]) {
    assert.deepEqual(names(await get(env, `/api/simulations?tag=${spelled}`)), ['Alpha', 'Bravo'], spelled);
  }
  assert.deepEqual(names(await get(env, '/api/simulations?tag=%23Zombies&tag=%23COOP')), ['Alpha']);
});

test('a ?tag= that is not a valid tag matches nothing, and is not an error', async () => {
  const env = await galleryEnv();
  const invalid = ['%3Cscript%3E', 'a%20b', '%23', 'sci_fi', 'a', 'co--op', '-coop', encodeURIComponent(ROCKET), 'x'.repeat(10_000), '%23%23zombies', 'caf' + encodeURIComponent(E_ACUTE)];
  for (const tag of invalid) {
    const r = await get(env, `/api/simulations?tag=${tag}`);
    assert.equal(r.status, 200, tag.slice(0, 30));
    assert.deepEqual(r.body, { simulations: [], total: 0 }, tag.slice(0, 30));
  }
  // One bad tag among good ones: every tag must match, and a bad one cannot.
  const mixed = await get(env, '/api/simulations?tag=zombies&tag=bad_tag');
  assert.equal(mixed.status, 200);
  assert.deepEqual(mixed.body.simulations, []);
});

test('an empty ?tag= asks for nothing, as an empty genre does', async () => {
  const env = await galleryEnv();
  for (const q of ['?tag=', '?tag=%20', '?tag=&tag=', '?tag=zombies&tag=']) {
    const r = await get(env, `/api/simulations${q}`);
    assert.equal(r.status, 200);
    assert.deepEqual(names(r), q.includes('zombies') ? ['Alpha', 'Bravo'] : ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo'], q);
  }
});

test('?tag= works with ?genre= and on the gallery alias', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?genre=survival&tag=zombies')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/simulations?genre=Survival&tag=zombies')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/simulations?genre=survival&tag=tycoon')), []);
  assert.deepEqual(names(await get(env, '/api/gallery?tag=coop')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/gallery?tag=zombies&tag=coop')), ['Alpha']);
});

test('a listing still in review is not found by its tag', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?tag=secret')), [], 'Foxtrot is pending');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=hidden')), [], 'Golf is private');
  assert.deepEqual(names(await get(env, '/api/simulations?tag=rejected-tag')), []);
  assert.deepEqual(names(await get(env, '/api/simulations?tag=held-tag')), []);
});

// ── free-text search ─────────────────────────────────────────────────────────

test('?q= matches a tag, with or without the #, and part of one', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?q=zombies')), ['Alpha', 'Bravo'], 'no name or description says zombies: the tags do');
  assert.deepEqual(names(await get(env, '/api/simulations?q=ZOMBIES')), ['Alpha', 'Bravo']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=%23zomb')), ['Alpha', 'Bravo']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=zomb')), ['Alpha', 'Bravo']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=coop')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=tycoon')), ['Bravo', 'Charlie']);
  assert.deepEqual(names(await get(env, `/api/simulations?q=${encodeURIComponent(fullWidth('coop'))}`)), ['Alpha']);
});

test('?q= still matches the name, the description and the author', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?q=charlie')), ['Charlie']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=HARBOR')), ['Delta']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=writer')), ['Charlie']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=zzzz')), []);
  assert.deepEqual(names(await get(env, '/api/simulations?q=')), ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo'], 'an empty search asks nothing');
  assert.deepEqual(names(await get(env, '/api/simulations?q=%20%20')), ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo']);
});

test('?q= never finds a tag of a listing that is not listed', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?q=secret')), []);
  assert.deepEqual(names(await get(env, '/api/simulations?q=hidden')), []);
});

test('?q= and ?tag= together narrow each other', async () => {
  const env = await galleryEnv();
  assert.deepEqual(names(await get(env, '/api/simulations?q=zombies&tag=coop')), ['Alpha']);
  assert.deepEqual(names(await get(env, '/api/simulations?q=bravo&tag=coop')), []);
});

// ── the popular tags ─────────────────────────────────────────────────────────

test('the popular tags are counted over listed listings, most used first, ties by tag', async () => {
  const env = await galleryEnv();
  const r = await get(env, '/api/simulations/tags');
  assert.equal(r.status, 200, r.text);
  assert.deepEqual(r.body, {
    tags: [
      { tag: 'tycoon', count: 2 },
      { tag: 'zombies', count: 2 },
      { tag: 'coop', count: 1 },
      { tag: 'survival', count: 1 },
    ],
  }, 'zombies is on 2 listed listings (not on Foxtrot or Golf), tycoon is the alphabetical first of the tie, and Alpha\'s genre counts as its tag');
});

test('the popular tags never name a tag only an unlisted listing carries', async () => {
  const env = await galleryEnv();
  const tags = (await get(env, '/api/simulations/tags')).body.tags.map((t) => t.tag);
  for (const unlisted of ['secret', 'hidden', 'rejected-tag', 'held-tag']) assert.ok(!tags.includes(unlisted), unlisted);
  // And a tag on a listed listing and an unlisted one counts the listed one only.
  assert.equal((await get(env, '/api/simulations/tags')).body.tags.find((t) => t.tag === 'zombies').count, 2);
});

test('the popular tags count a tag once per listing', async () => {
  const env = await seededEnv();
  await seed(env, { name: 'Alpha', tags: ['dup-tag', 'dup-tag', 'one-tag'] });
  const r = await get(env, '/api/simulations/tags');
  assert.deepEqual(r.body.tags, [{ tag: 'dup-tag', count: 1 }, { tag: 'one-tag', count: 1 }]);
});

test('the popular tags are the top 30', async () => {
  const env = await seededEnv();
  for (let i = 1; i <= 35; i++) await seed(env, { name: `Game ${i}`, tags: ['common', `t${String(i).padStart(2, '0')}`] });
  const r = await get(env, '/api/simulations/tags');
  assert.equal(r.body.tags.length, 30);
  assert.deepEqual(r.body.tags[0], { tag: 'common', count: 35 });
  assert.deepEqual(r.body.tags[1], { tag: 't01', count: 1 });
  assert.deepEqual(r.body.tags[29], { tag: 't29', count: 1 }, 'the 30th by count then tag');
  assert.ok(!r.body.tags.some((t) => t.tag === 't30'), 'the rest are left off');
});

test('the popular tags are empty when nothing has a tag, with no listing and with no tags', async () => {
  const env = await seededEnv();
  assert.deepEqual((await get(env, '/api/simulations/tags')).body, { tags: [] });
  await seed(env, { name: 'Bare', tags: [] });
  await seed(env, { name: 'Old', tags: undefined });
  assert.deepEqual((await get(env, '/api/simulations/tags')).body, { tags: [] });
});

test('the popular tags are cached for five minutes by whoever holds them, and need no sign-in', async () => {
  const env = await galleryEnv();
  const r = await get(env, '/api/simulations/tags');
  assert.equal(r.headers.get('Cache-Control'), 'public, max-age=300');
  assert.match(r.headers.get('Content-Type'), /application\/json/);
  assert.equal(r.headers.get('Vary'), 'Origin, Authorization', 'a shared cache keeps one answer per origin, and the signed-out one apart from any session (src/adult_content.mjs)');
  const withOrigin = await get(env, '/api/simulations/tags', { Origin: 'https://eustress.dev' });
  assert.equal(withOrigin.headers.get('Access-Control-Allow-Origin'), 'https://eustress.dev');
  // The list itself is not cached this way: it follows the filters a visitor chooses.
  assert.notEqual((await get(env, '/api/simulations')).headers.get('Cache-Control'), 'public, max-age=300');
});

test('the popular tags follow the viewer\'s rating limit, like the list does', async () => {
  const env = await seededEnv();
  await seed(env, { name: 'Kids', tags: ['kid-safe', 'shared'], rating: 'all_ages' });
  await seed(env, { name: 'Teens', tags: ['teen-stuff', 'shared'], rating: 'teen_13' });
  await seed(env, { name: 'Adults', tags: ['adult-only', 'shared'], rating: 'adult_18' });
  // Signed out, Adult content is off: an adult listing's tags are not named (src/adult_content.mjs).
  const out = (await get(env, '/api/simulations/tags')).body.tags;
  assert.deepEqual(out[0], { tag: 'shared', count: 2 });
  assert.ok(!out.some((t) => t.tag === 'adult-only'));
  // A viewer who turned Adult content on has every rating, and rating_max still narrows it.
  const VIEWER = '44444444-4444-4444-8444-444444444444';
  await env.USERS.put(`adult-content:${VIEWER}`, '1');
  const on = { Authorization: `Bearer ${mintToken(VIEWER)}` };
  const all = (await get(env, '/api/simulations/tags', on)).body.tags;
  assert.deepEqual(all[0], { tag: 'shared', count: 3 });
  assert.ok(all.some((t) => t.tag === 'adult-only'));
  const teen = (await get(env, '/api/simulations/tags?rating_max=teen_13', on)).body.tags;
  assert.deepEqual(teen[0], { tag: 'shared', count: 2 });
  assert.ok(!teen.some((t) => t.tag === 'adult-only'), 'an all-ages page does not name an adult listing\'s tag');
  const kids = (await get(env, '/api/simulations/tags?rating_max=all_ages', on)).body.tags;
  assert.deepEqual(kids.map((t) => t.tag).sort(), ['kid-safe', 'shared']);
});

test('the popular tags ignore the list\'s own filters in the query', async () => {
  const env = await galleryEnv();
  const plain = (await get(env, '/api/simulations/tags')).body;
  assert.deepEqual((await get(env, '/api/simulations/tags?tag=coop&q=alpha&genre=survival')).body, plain);
});

test('the popular-tags route is named in the route policy as a public read', () => {
  const r = routePolicy('GET', '/api/simulations/tags');
  assert.equal(r.explicit, true);
  assert.equal(r.class, PUBLIC);
});

test('a listing id is still read as an id, and tags is not one', async () => {
  const env = await seededEnv();
  const id = await seed(env, { name: 'Alpha', tags: ['zombies'] });
  assert.equal((await get(env, `/api/simulations/${id}`)).body.name, 'Alpha');
  assert.equal((await get(env, '/api/simulations/tags')).body.tags[0].tag, 'zombies');
});

// ── moderation reads the tags ────────────────────────────────────────────────

test('the classifier\'s state carries the tags beside the name and the description', () => {
  const state = buildJevState({ name: 'Zombie Yard', description: 'Hold the line.', genre: 'survival', tags: ['zombies', 'coop'] }, null);
  assert.deepEqual(state.listing.tags, ['zombies', 'coop', 'survival'], 'a genre from before tags is one of the tags');
  assert.equal(state.listing.name, 'Zombie Yard');
  assert.equal('genre' in state.listing, false, 'tags are the only category Jev reads');
  assert.deepEqual(buildJevState({ name: 'Old' }, null).listing.tags, [], 'a listing from before tags');
});

test('what a model reads of the tags is capped, even for a record written some other way', () => {
  const sim = { name: 'n', tags: Array.from({ length: 30 }, () => 'y'.repeat(100)) };
  assert.equal(tagsForReview(sim).length, 8);
  assert.ok(tagsForReview(sim).every((t) => t.length === 24));
  assert.deepEqual(tagsForReview({ tags: ['ok', 5, null] }), ['ok']);
  assert.deepEqual(buildJevState(sim, null).listing.tags, tagsForReview(sim));
});

/// The case summary the judge (Claude, Guardian Code v2.1) is sent for `sim`: the JSON after "Case summary (JSON):" in the
/// request's text.
async function judgeSummaryFor(sim) {
  const sent = [];
  const fakeDeps = { claude: fakeClaude('', sent) };
  await claudeJudge({ sim, dossier: null, jev: { answers: {} }, triage: {}, captures: [], policyText: 'policy', policyHash: 'hash', env: {}, deps: fakeDeps });
  assert.equal(sent.length, 1);
  const parts = sent[0].body.messages[0].content.filter((p) => p.type === 'text').map((p) => p.text);
  const summaryText = parts.find((t) => t.includes('Case summary (JSON):'));
  assert.ok(summaryText, 'the case summary part is there');
  const rows = summaryText.split('\n');
  return JSON.parse(rows[rows.indexOf('Case summary (JSON):') + 1]);
}

test('the judge\'s case summary carries the tags beside the name and the description', async () => {
  const summary = await judgeSummaryFor({ name: 'Zombie Yard', description: 'Hold the line.', genre: 'survival', tags: ['zombies', 'coop'], is_public: true });
  assert.deepEqual(summary.listing.tags, ['zombies', 'coop', 'survival'], 'a genre from before tags is one of the tags');
  assert.equal('genre' in summary.listing, false, 'tags are the only category the judge reads');
  assert.equal(summary.listing.author_gave_tags, true);
  assert.equal(summary.listing.name, 'Zombie Yard');
  assert.equal(summary.listing.description, 'Hold the line.');
  assert.deepEqual((await judgeSummaryFor({ name: 'Old', is_public: true })).listing.tags, [], 'a listing from before tags');
});

test('the judge\'s case summary caps the tags too, even for a record written some other way', async () => {
  const sim = { name: 'n', tags: Array.from({ length: 30 }, () => 'y'.repeat(100)), is_public: true };
  const summary = await judgeSummaryFor(sim);
  assert.equal(summary.listing.tags.length, 8);
  assert.ok(summary.listing.tags.every((t) => t.length === 24));
  assert.deepEqual(summary.listing.tags, tagsForReview(sim));
});
