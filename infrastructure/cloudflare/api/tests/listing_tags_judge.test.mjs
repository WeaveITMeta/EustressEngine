// Tags are a listing's only categories (Guardian Code v2.3, McKale 2026-10-08: "Genre in publishing has to just be tags").
// What this file pins, and docs/networking/LISTING_TAGS.md states:
//   - every route that returns a listing carries `tags`, `tags_source` ("author" | "judge") and `genre`, a deprecated mirror
//     (the migrated genre, or null) for one release;
//   - a genre from before tags is read as a tag (not "all", valid, within the 8), and written into `tags` on the next write;
//   - `?genre=` is an alias of `?tag=`;
//   - an author who gives no tags (`tags: []`) gets the judge's: `suggested_tags`, normalized, at most 5, `fiction` and a genre
//     tag for fiction, written with "judge" for every outcome but a quarantine, a harm rejection or a legal-lane outcome, and told
//     in one line in the email and the case history;
//   - the judge's tags sent back as they are stay the judge's and are not reviewed again; a changed set is the author's and is;
//   - an older Studio's genre is ignored (except as the one tag of a listing that has none), and an empty one is absent;
//   - Jev's tag_fit and tag_content: a confident fault is "not yet, fix your tags"; tags never move the rating;
//   - a tag is never a phone number, an email address or a handle.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { fakeClaude, questionnaireAnswers } from './helpers/claude_fake.mjs';
import { LiveHost } from '../src/live.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { handleWorldRoute } from '../src/world.mjs';
import { routePolicy, FRESH } from '../src/route_policy.mjs';
import { corsHeaders } from '../src/cors.mjs';
import { reviewEntry } from '../src/case_history.mjs';
import { renderModerationEmail } from '../src/moderation_email.mjs';
import { questionnaireRecord, questionnaireOf } from '../src/rating_questionnaire.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import {
  MAX_TAGS, JUDGE_TAGS_MAX, TAG_RULES, normalizeTag, checkTags, listingTags, genreTag, withTagsWrittenBack, tagsSource, reviewChoosesTags,
  listingTagFields, setAuthorTags, judgeTags, genreAliasValues, personalInfoIn, tagFilter,
} from '../src/tags.mjs';
import {
  decideFinal, decideTriage, deterministicSignals, parseJudgeVerdict, buildJevQuestions, buildJevState, extractQualificationStatement,
  runModerationCase, loadCase, authorView, claudeJudge, assignedTags, fictionLabelled, isListable, canServe, tagsChosenLine,
  JUDGE_OUTPUT_SCHEMA, JUDGE_MODEL, POLICY_HASH_ANCHORED, POLICY_VERSION, CODE_TESTS, DEFAULT_THRESHOLDS, AUTHOR_COPY, FICTION_TAG,
} from '../src/moderation.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const author = mintToken(AUTHOR);
const stranger = mintToken(STRANGER);

// ── the tag rule: personal information is never a tag ────────────────────────

test('a phone number is never a tag, with or without hyphens; a year range and short numbers are', () => {
  for (const phone of ['5551234', '555-1234', '1-800-555-1234', 'call-555-1234', 'call5551234', '#5551234', '555-12-34']) {
    assert.equal(normalizeTag(phone), null, phone);
    assert.equal(personalInfoIn(phone), 'phone', phone);
  }
  for (const ok of ['2026', '007', '1939-1945', 'ww2-1944', 'top10', '24h', '3d', '1v1', '123456']) assert.equal(normalizeTag(ok), ok, ok);
});

test('an email address or a handle is never a tag, and the refusal names the kind without repeating it', () => {
  assert.equal(personalInfoIn('ann@mail.com'), 'email');
  assert.equal(personalInfoIn('ann-at-mail-com'), 'email');
  assert.equal(personalInfoIn('ann-at-mail-dot-com'), 'email');
  assert.equal(personalInfoIn('@ann_b'), 'handle');
  assert.equal(personalInfoIn('#@ann'), 'handle');
  assert.equal(normalizeTag('ann-at-mail-com'), null);
  for (const ok of ['chat-room', 'at-home', 'cat', 'meet-at-noon', 'b@d']) assert.equal(personalInfoIn(ok), null, ok);
  const cases = [['ann@mail.com', /Entry 2 of tags looks like an email address/], ['@ann', /Entry 2 of tags looks like a handle/], ['555-123-4567', /Entry 2 of tags looks like a phone number/]];
  for (const [raw, pattern] of cases) {
    const r = checkTags(['coop', raw]);
    assert.equal(r.ok, false);
    assert.match(r.error, pattern);
    assert.ok(!r.error.includes(raw), 'the personal information is not repeated back');
    assert.ok(r.error.endsWith(TAG_RULES));
  }
  assert.match(TAG_RULES, /never a phone number, an email address or a handle/);
  // The gallery filter shares the rule: a phone number matches nothing.
  assert.deepEqual(tagFilter(['555-1234']), { none: true, tags: [] });
});

// ── migration: a genre from before tags ──────────────────────────────────────

test('a stored genre reads as a tag: normalized, "all" skipped, an invalid one skipped, never twice, within the 8', () => {
  assert.equal(genreTag({ genre: 'Racing' }), 'racing');
  assert.equal(genreTag({ genre: '#Sci-Fi' }), 'sci-fi');
  for (const none of ['all', 'All', 'ALL', ' all ', '#all', '', '   ', 'Sci Fi', 'role_play', null, undefined, 5]) assert.equal(genreTag({ genre: none }), null, String(none));
  assert.deepEqual(listingTags({ genre: 'Racing' }), ['racing']);
  assert.deepEqual(listingTags({ genre: 'Racing', tags: ['coop'] }), ['coop', 'racing']);
  assert.deepEqual(listingTags({ genre: 'Racing', tags: ['racing', 'coop'] }), ['racing', 'coop'], 'already present: not added again');
  assert.deepEqual(listingTags({ genre: 'all', tags: ['coop'] }), ['coop'], '"all" named no category');
  assert.deepEqual(listingTags({ genre: 'Sci Fi', tags: ['coop'] }), ['coop'], 'not a valid tag: skipped');
  const eight = ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'];
  assert.deepEqual(listingTags({ genre: 'Racing', tags: eight }), eight, 'the cap of 8 holds');
  assert.deepEqual(listingTags({ genre: 'Racing', tags: eight.slice(0, 7) }), [...eight.slice(0, 7), 'racing'], 'the 8th fits');
  assert.equal(MAX_TAGS, 8);
  assert.deepEqual(listingTags({ genre: 'Racing', tags: ['coop'], genre_migrated: true }), ['coop'], 'once written, tags are the whole answer');
});

test('the migration is written back on the next write, once, and a listing with nothing to migrate is left alone', () => {
  const sim = withTagsWrittenBack({ genre: 'Racing', tags: ['coop'] });
  assert.deepEqual([sim.tags, sim.genre_migrated, sim.genre], [['coop', 'racing'], true, 'Racing']);
  assert.deepEqual(withTagsWrittenBack({ genre: 'all', tags: ['coop'] }), { genre: 'all', tags: ['coop'] });
  assert.deepEqual(withTagsWrittenBack({ genre: 'Racing', tags: ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'] }).tags.length, 8);
  // The author removing the migrated tag later is not undone by a later read.
  const later = { ...sim };
  setAuthorTags(later, ['coop']);
  assert.deepEqual(listingTags(later), ['coop']);
});

test('the category fields: tags, who chose them, and the genre mirror', () => {
  assert.deepEqual(listingTagFields({ genre: 'Racing', tags: ['coop'] }), { tags: ['coop', 'racing'], tags_source: 'author', genre: 'racing' });
  assert.deepEqual(listingTagFields({ genre: 'all' }), { tags: [], tags_source: 'author', genre: null });
  assert.deepEqual(listingTagFields({ tags: ['harbour'], tags_source: 'judge' }), { tags: ['harbour'], tags_source: 'judge', genre: null });
  assert.equal(tagsSource({ tags: [], tags_source: 'judge' }), 'author', 'no tags are nobody\'s choice');
  assert.equal(reviewChoosesTags({ tags: [] }), true);
  assert.equal(reviewChoosesTags({ tags: ['harbour'], tags_source: 'judge' }), true, 'the review chooses its own again');
  assert.equal(reviewChoosesTags({ tags: ['harbour'] }), false);
  assert.equal(reviewChoosesTags({ genre: 'Racing' }), false, 'a migrated genre is the author\'s');
});

test('the judge\'s tags sent back as they are stay the judge\'s; any change is the author\'s', () => {
  const sim = { tags: ['harbour', 'tides'], tags_source: 'judge' };
  assert.equal(setAuthorTags(sim, ['tides', 'harbour']), false, 'the same set, reordered, is no change');
  assert.deepEqual([sim.tags, sim.tags_source], [['tides', 'harbour'], 'judge']);
  assert.equal(setAuthorTags(sim, ['tides']), true);
  assert.equal(sim.tags_source, 'author');
  const mine = { tags: ['coop'] };
  assert.equal(setAuthorTags(mine, []), true, 'clearing is a change: the review chooses');
  assert.equal(reviewChoosesTags(mine), true);
});

test('?genre= is an alias of ?tag=: normalized, and "all" or empty asks nothing', () => {
  assert.deepEqual(genreAliasValues(['Racing']), ['Racing']);
  assert.deepEqual(genreAliasValues(['all', 'ALL', '', '  ']), []);
  assert.deepEqual(genreAliasValues(undefined), []);
});

// ── the judge's tags ─────────────────────────────────────────────────────────

test('the judge\'s suggestions are normalized, invalid ones and personal information dropped, at most 5', () => {
  assert.equal(JUDGE_TAGS_MAX, 5);
  assert.deepEqual(judgeTags(['#Harbour', 'TIDES', 'sci_fi', 'harbour', '555-1234', 'ann@mail.com', 'a', 'Cranes', 'ships', 'port', 'docks', 'quay']), ['harbour', 'tides', 'cranes', 'ships', 'port']);
  assert.deepEqual(judgeTags('harbour'), [], 'not a list: none');
  assert.deepEqual(judgeTags([5, null, {}, 'ok-tag']), ['ok-tag']);
  assert.deepEqual(assignedTags(['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'h7'], false), ['h1', 'h2', 'h3', 'h4', 'h5']);
});

test('fiction: the assigned tags carry fiction and the judge\'s genre tag inside the 5, so the label rule passes', () => {
  const tags = assignedTags(['harbour', 'tides', 'ships', 'cranes', 'port', 'Science-Fiction'], true);
  assert.deepEqual(tags, ['fiction', 'science-fiction', 'harbour', 'tides', 'ships']);
  assert.equal(fictionLabelled(tags), true);
  assert.deepEqual(assignedTags(['fiction', 'mars', 'space-opera'], true), ['fiction', 'space-opera', 'mars']);
  assert.deepEqual(assignedTags(['mars', 'hard-sf'], true), ['fiction', 'hard-sf', 'mars']);
  assert.equal(fictionLabelled(assignedTags(['mars'], true)), false, 'no genre named: the label is still missing');
  assert.equal(assignedTags(['mars', 'fantasy'], false).includes(FICTION_TAG), false, 'not fiction: no fiction tag added');
});

test('the judge\'s schema carries suggested_tags, always required, and the parser reads it leniently', () => {
  assert.deepEqual(JUDGE_OUTPUT_SCHEMA.properties.suggested_tags, { type: 'array', items: { type: 'string' } });
  assert.ok(JUDGE_OUTPUT_SCHEMA.required.includes('suggested_tags'));
  const base = judgeAnswer();
  const parsed = parseJudgeVerdict(JSON.stringify({ ...base, suggested_tags: ['#Harbour', 'bad tag', 'tides'] }));
  assert.equal(parsed.malformed, false);
  assert.deepEqual(parsed.suggested_tags, ['harbour', 'tides']);
  const { suggested_tags: _, ...without } = base;
  const missing = parseJudgeVerdict(JSON.stringify(without));
  assert.equal(missing.malformed, false, 'tags decide nothing, so a missing list is no malformed answer');
  assert.deepEqual(missing.suggested_tags, []);
});

test('the judge is told whether the author gave tags, and the Code tells it what to do', async () => {
  const sent = [];
  await claudeJudge({ sim: { name: 'Harbour', tags: [] }, dossier: null, jev: { answers: {} }, triage: {}, captures: [], policyText: 'P', policyHash: 'h', env: {}, deps: { claude: fakeClaude(judgeAnswer(), sent) } });
  const text = sent[0].body.messages[0].content.at(-1).text;
  const summary = JSON.parse(text.split('\n')[text.split('\n').indexOf('Case summary (JSON):') + 1]);
  assert.equal(summary.listing.author_gave_tags, false);
  assert.equal('genre' in summary.listing, false);
  assert.match(GUARDIAN_POLICY_TEXT, /`suggested_tags` is always present\. Fill it only when the case summary says the author gave no tags/);
  assert.match(GUARDIAN_POLICY_TEXT, /^# Eustress Guardian Code v2\.3/);
});

// ── the verdict rules ───────────────────────────────────────────────────────

const STATEMENT_TEXT = [
  'Category: Simulation',
  'Objective: Set the tide range and read the depth at the quay.',
  'Lesson: How tidal range and basin shape set harbour depth.',
  'Real-world link: Models a tidal harbour from published tide tables.',
].join('\n');
const STATEMENT = extractQualificationStatement(STATEMENT_TEXT);
const ANSWERED = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers()) });
const scores = () => Object.fromEntries(CODE_TESTS.map(k => [k, { score: 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
function judgeAnswer(over = {}) {
  return {
    verdict: 'publish', articles: [], tests: scores(), statement_check: 'matches', rating_floor: 'ages_10', descriptors: [], is_game: false, is_fiction: false,
    featured_candidate: false, policy_version: POLICY_VERSION, policy_hash: POLICY_HASH_ANCHORED, spatial_evidence: ['four quay cranes along the east basin'],
    rationale: 'Every test is shown.', confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, suggested_tags: [], ...over,
  };
}
const parsedJudge = over => parseJudgeVerdict(JSON.stringify(judgeAnswer(over)));
const triage = (over = {}) => ({
  outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'simulation', rating: 'all_ages', articles: { system: [], signals: [], exempt: [] },
  statement: STATEMENT, scripts_truncated: false, game_mechanics: 0.02, lawful_decision_training: 0.02, questionnaire: ANSWERED,
  fiction_p: 0.02, fiction_labelled: false, review_chooses_tags: true, account: { attested_properties: [], verified_business: false, payment_connection: false }, ...over,
});
const cite = (id, confidence = 0.9) => ({ id, confidence, evidence: `Code ${id}: the vault door and camera layout in capture-2 and Soul/heist.luau` });

test('the review chooses: the judge\'s tags are assigned; the author chose: none are', () => {
  const d = decideFinal({ triage: triage(), judge: parsedJudge({ suggested_tags: ['Harbour', 'tides'] }) });
  assert.equal(d.outcome, 'approve');
  assert.deepEqual(d.assigned_tags, ['harbour', 'tides']);
  const mine = decideFinal({ triage: triage({ review_chooses_tags: false }), judge: parsedJudge({ suggested_tags: ['harbour'] }) });
  assert.deepEqual(mine.assigned_tags, [], 'the author\'s tags stand');
  assert.deepEqual(decideFinal({ triage: triage(), judge: { malformed: true, error: 'judge_malformed' } }).assigned_tags, [], 'no answer, no tags');
});

test('fiction with no author tags: the label is checked after assignment, so the judge\'s fiction tags satisfy it', () => {
  const labelled = decideFinal({ triage: triage(), judge: parsedJudge({ is_fiction: true, suggested_tags: ['mars', 'science-fiction'] }) });
  assert.deepEqual(labelled.assigned_tags, ['fiction', 'science-fiction', 'mars']);
  assert.equal(labelled.outcome, 'approve', JSON.stringify(labelled.reasons));
  const unlabelled = decideFinal({ triage: triage(), judge: parsedJudge({ is_fiction: true, suggested_tags: ['mars'] }) });
  assert.equal(unlabelled.outcome, 'changes_requested');
  assert.ok(unlabelled.changes.includes('fiction_label_missing'));
  // The author's own tags are checked as they are, as before.
  const authors = decideFinal({ triage: triage({ review_chooses_tags: false, fiction_labelled: false }), judge: parsedJudge({ is_fiction: true, suggested_tags: ['science-fiction'] }) });
  assert.ok(authors.changes.includes('fiction_label_missing'));
});

test('no judge tags on a quarantine, a harm (Group A) rejection or a legal-lane hold; a quality outcome keeps them', () => {
  const suggest = { suggested_tags: ['harbour', 'tides'] };
  const quarantined = decideFinal({ triage: triage(), judge: parsedJudge({ ...suggest, verdict: 'reject', articles: [cite('A1', 0.9)] }) });
  assert.deepEqual([quarantined.outcome, quarantined.assigned_tags], ['quarantine', []]);
  const harm = decideFinal({ triage: triage(), judge: parsedJudge({ ...suggest, verdict: 'reject', articles: [cite('A4', 0.9)] }) });
  assert.deepEqual([harm.outcome, harm.lane, harm.assigned_tags], ['reject', 'harm', []]);
  const legalHold = decideFinal({ triage: triage(), judge: parsedJudge({ ...suggest, verdict: 'flag_for_human_review', articles: [cite('A11', 0.5)] }) });
  assert.deepEqual([legalHold.outcome, legalHold.lane, legalHold.assigned_tags], ['hold', 'legal', []]);
  const quality = decideFinal({ triage: triage(), judge: parsedJudge({ ...suggest, tests: { ...scores(), objective: { score: 0, evidence: 'objective: nothing to do anywhere in capture-1' } } }) });
  assert.deepEqual([quality.outcome, quality.lane, quality.assigned_tags], ['reject', 'quality', ['harbour', 'tides']]);
  const notYet = decideFinal({ triage: triage({ questionnaire: questionnaireOf({}) }), judge: parsedJudge(suggest) });
  assert.deepEqual([notYet.outcome, notYet.assigned_tags], ['changes_requested', ['harbour', 'tides']]);
});

// ── Jev: tag_fit and tag_content ────────────────────────────────────────────

const jevNoul = answers => ({ answers: Object.fromEntries(Object.entries(answers).map(([k, p]) => [k, { type: 'noul', p }])) });
const signalsFor = (sim) => ({ ...deterministicSignals(sim, null), questionnaire: ANSWERED, statement: STATEMENT });

test('the battery asks tag_fit and tag_content, and no longer genre_fit', () => {
  const q = buildJevQuestions();
  assert.equal('genre_fit' in q, false);
  assert.equal(q.tag_fit.instructions, 'Do the listing\'s tags describe what the Space contains? Tags that misdescribe the Space (claiming a topic, genre or audience it does not have) do not fit.');
  assert.equal(q.tag_content.instructions, 'Is any tag a slur, an insult aimed at a person or group, or personal information about a real person?');
  assert.match(q.horror_framing.instructions, /Do the listing or its tags call the Space horror/);
});

test('a confident tag misfit, or a tag that is a slur or personal information, is not yet: "Fix your tags"', () => {
  const sim = { name: 'Harbour', description: STATEMENT_TEXT, tags: ['zombies', 'horde'] };
  for (const jev of [jevNoul({ tag_fit: 0.05 }), jevNoul({ tag_fit: 0.9, tag_content: 0.95 })]) {
    const t = decideTriage({ signals: signalsFor(sim), jev });
    assert.ok(t.articles.system.some(s => s.article === 'B9' && ['tag_misfit', 'tag_content'].includes(s.key)), JSON.stringify(t.articles.system));
    const d = decideFinal({ triage: t, judge: parsedJudge() });
    assert.equal(d.outcome, 'changes_requested', JSON.stringify(d.reasons));
    assert.deepEqual(d.changes, ['not_yet_tags']);
    assert.match(d.suggested_edit, /^Fix your tags/);
    const view = authorView({ status: 'changes_requested', decision: d, updated_at: 'now' }, { is_public: true });
    assert.deepEqual(view.reasons.map(r => r.category), ['not_yet_tags']);
    assert.match(view.reasons[0].title, /^Not yet: fix your tags \(Code B9\)$/);
    assert.match(view.reasons[0].what_to_change, /^Fix your tags/);
  }
  assert.equal(AUTHOR_COPY.not_yet_tags.what_to_change.startsWith('Fix your tags'), true);
});

test('an unsure tag answer decides nothing, and the tag questions are not read when the review chooses the tags', () => {
  const sim = { name: 'Harbour', description: STATEMENT_TEXT, tags: ['zombies'] };
  const unsure = decideTriage({ signals: signalsFor(sim), jev: jevNoul({ tag_fit: 0.4, tag_content: 0.6 }) });
  assert.equal(unsure.articles.system.some(s => s.article === 'B9'), false);
  const chosen = decideTriage({ signals: signalsFor({ name: 'Harbour', tags: [] }), jev: jevNoul({ tag_fit: 0.01, tag_content: 0.99 }) });
  assert.equal(chosen.articles.system.some(s => s.article === 'B9'), false);
  const judges = decideTriage({ signals: signalsFor({ name: 'Harbour', tags: ['harbour'], tags_source: 'judge' }), jev: jevNoul({ tag_fit: 0.01 }) });
  assert.equal(judges.articles.system.some(s => s.article === 'B9'), false, 'the review\'s own earlier tags are not the author\'s');
});

test('tags are discovery, not rating: a Space\'s rating is the same with and without tags, and with the judge\'s', () => {
  const jev = { answers: { content_rating: { type: 'choice', choice: 'teen_13', probabilities: { teen_13: 0.9 }, confidence: 0.9 }, tag_fit: { type: 'noul', p: 0.9 } } };
  const ratingOf = (sim, judge) => {
    const t = decideTriage({ signals: signalsFor(sim), jev });
    const d = decideFinal({ triage: t, judge: parsedJudge(judge) });
    return [t.rating, d.rating, d.descriptors, d.outcome];
  };
  const plain = { name: 'Harbour', description: STATEMENT_TEXT };
  const without = ratingOf({ ...plain, tags: [] }, { suggested_tags: ['harbour', 'mature', 'adults-only', 'gore'] });
  const withTags = ratingOf({ ...plain, tags: ['harbour', 'tides', 'teens', 'kids', 'all-ages'] }, {});
  const legacy = ratingOf({ ...plain, genre: 'Racing' }, {});
  assert.deepEqual(without, ['teen_13', 'teen_13', [], 'approve']);
  assert.deepEqual(withTags, without);
  assert.deepEqual(legacy, without);
});

test('the dossier\'s listing.genre is never read: an empty one, a horror one and none are the same', () => {
  const sim = { name: 'Harbour', description: 'A harbour.', tags: ['harbour'] };
  const dossier = genre => ({ dossier_version: 1, listing: { name: 'Harbour', description: 'A harbour.', ...(genre === undefined ? {} : { genre }) }, digest: { entity_count: 3 }, strings: [], scripts: [], signals: {} });
  const none = buildJevState(sim, dossier(undefined));
  assert.deepEqual(buildJevState(sim, dossier('')), none);
  assert.deepEqual(buildJevState(sim, dossier('Horror')), none);
  assert.deepEqual(deterministicSignals(sim, dossier('')).horror_terms, []);
  assert.deepEqual(deterministicSignals(sim, dossier('Horror')).horror_terms, []);
});

// ── the pipeline: the judge's tags written, told, and left alone where they must be ─────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).sort().slice(0, limit).map(name => ({ name })), list_complete: true }),
  };
}
function r2() {
  const m = new Map();
  let versions = 0;
  const obj = (key, rec) => ({ key, size: rec.bytes.byteLength, etag: rec.etag, httpMetadata: rec.httpMetadata || {}, customMetadata: rec.customMetadata || {}, checksums: rec.checksums || {}, version: rec.version, text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0), body: rec.bytes });
  return {
    m,
    put: async (key, body, opts = {}) => {
      if (opts.onlyIf) { const cur = m.get(key); if (opts.onlyIf.etagDoesNotMatch === '*' && cur) return null; if (opts.onlyIf.etagMatches !== undefined && (!cur || cur.etag !== opts.onlyIf.etagMatches)) return null; }
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body).buffer : (body instanceof ArrayBuffer ? body : body.buffer);
      const rec = { version: String(++versions), bytes, etag: createHash('md5').update(Buffer.from(bytes)).digest('hex'), httpMetadata: opts.httpMetadata, customMetadata: opts.customMetadata, checksums: opts.sha256 ? { sha256: Uint8Array.from(Buffer.from(opts.sha256, 'hex')).buffer } : undefined };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async key => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key, size: m.get(key).bytes.byteLength })), truncated: false }),
  };
}
const json = (v, status = 200, headers = {}) => new Response(JSON.stringify(v), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
function modEnv() {
  const env = { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', JWT_SECRET: 'test-secret' };
  env.sent = [];
  env.EMAIL = { send: async m => { env.sent.push(m); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  env.USERS.m.set('user:user-1', JSON.stringify({ id: 'user-1', username: 'ann', email: 'ann@example.com' }));
  return env;
}
const mailText = m => Buffer.from(m.raw.split('Content-Transfer-Encoding: base64\r\n\r\n')[1].split('\r\n--')[0].replace(/\r\n/g, ''), 'base64').toString('utf8');
function jevAnswers(overrides = {}) {
  const answers = {};
  for (const [k, def] of Object.entries(buildJevQuestions())) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: ['metadata_consistency', 'functional_purpose', 'spatial_intent', 'tag_fit', 'model_implemented', 'result_readout'].includes(k) ? 0.9 : 0.02 };
    else if (def.type === 'choice') {
      const first = k === 'content_rating' ? 'all_ages' : k === 'content_kind' ? 'simulation' : Object.keys(def.criteria)[0];
      answers[k] = { type: 'choice', choice: first, probabilities: Object.fromEntries(Object.keys(def.criteria).map(c => [c, c === first ? 0.9 : 0.01])), confidence: 0.9 };
    } else {
      const pick = k === 'quality_band' ? 'solid_authored_experience' : def.criteria[0];
      answers[k] = { type: 'score', score: def.criteria.indexOf(pick) / (def.criteria.length - 1), probabilities: Object.fromEntries(def.criteria.map(l => [l, l === pick ? 0.85 : 0.03])), confidence: 0.85 };
    }
  }
  for (const [k, v] of Object.entries(overrides)) answers[k] = { type: 'noul', noul: v };
  return { model: 'jev-test', answers };
}
function pipelineDeps({ judge = judgeAnswer(), jev = jevAnswers() } = {}) {
  return {
    json, cors: {}, auditLog: async () => {},
    verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async () => null,
    claude: fakeClaude(() => judge),
    grokFetch: async () => ({ ok: true, status: 200, json: async () => ({ output: [] }), text: async () => '' }),
    grokModel: 'grok-test',
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: async () => new Response(JSON.stringify(jev), { status: 200 }),
  };
}
const MSIM = 'a1b2c3d4-0000-4000-8000-0000000023a1';
async function publish(env, listing = {}) {
  const sim = {
    id: MSIM, name: 'Harbour Basin', description: `A working harbour.\n${STATEMENT_TEXT}`, tags: [], author_id: 'user-1', is_public: true,
    r2_key: `universes/${MSIM}/universe.pak`, published_at: new Date().toISOString(), updated_at: new Date().toISOString(),
    moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()), ...listing,
  };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(`PAK-${MSIM}`).buffer, { sha256: createHash('sha256').update(`PAK-${MSIM}`).digest('hex') });
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${MSIM}`, JSON.stringify(sim));
  await env.SCENES.put(`universes/${MSIM}/moderation/dossier.json`, JSON.stringify({ dossier_version: 1, digest: { entity_count: 300, default_material_fraction: 0.2, default_name_fraction: 0.2, script_count: 1, text_string_count: 4 }, strings: ['Crane 4'], scripts: [{ path: 'Soul/tide.luau', language: 'luau', lines: 3, source: 'local depth = basin + tide' }], signals: { urls: [] } }));
  await env.SCENES.put(`universes/${MSIM}/moderation/capture-0.png`, new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]).buffer, { httpMetadata: { contentType: 'image/png' } });
  return sim;
}
const listing = async env => JSON.parse(await env.SOCIAL.get(`sim:${MSIM}`));

test('tags: [] gets the judge\'s tags: normalized, at most 5, written with "judge", and told in the email and the case history', async () => {
  const env = modEnv();
  await publish(env);
  const rec = await runModerationCase(MSIM, env, pipelineDeps({ judge: judgeAnswer({ suggested_tags: ['#Harbour', 'Tides', 'sci_fi', '555-1234', 'cranes', 'ships', 'port', 'docks'] }) }));
  assert.equal(rec.status, 'approved', JSON.stringify(rec.decision?.reasons));
  const chosen = ['harbour', 'tides', 'cranes', 'ships', 'port'];
  assert.deepEqual(rec.decision.assigned_tags, chosen);
  const after = await listing(env);
  assert.deepEqual([after.tags, after.tags_source], [chosen, 'judge']);
  assert.equal(isListable(after), true);
  const line = 'Tags chosen for your Space: harbour, tides, cranes, ships, port';
  assert.equal(tagsChosenLine(chosen), line);
  const view = authorView(await loadCase(env, MSIM), after);
  assert.equal(view.tags_chosen, line);
  assert.deepEqual(view.assigned_tags, chosen);
  assert.equal(env.sent.length, 1);
  const mail = mailText(env.sent[0]);
  assert.ok(mail.includes(line), mail);
  assert.ok(renderModerationEmail({ view, kind: 'decision', simName: 'Harbour Basin', reviewUrl: 'https://eustress.dev/r', unsubUrl: 'https://api.eustress.dev/u' }).html.includes(line));
  const entry = reviewEntry('EC-TEST', await loadCase(env, MSIM), { ...after, id: MSIM });
  assert.equal(entry.feedback.tags_chosen, line);
  assert.deepEqual(entry.feedback.assigned_tags, chosen);
});

test('a fiction Space with no tags gets fiction and the judge\'s genre tag, and is listed without asking for a label', async () => {
  const env = modEnv();
  await publish(env);
  const rec = await runModerationCase(MSIM, env, pipelineDeps({ judge: judgeAnswer({ is_fiction: true, suggested_tags: ['harbour', 'tides', 'ships', 'cranes', 'port', 'science-fiction'] }) }));
  assert.equal(rec.status, 'approved', JSON.stringify(rec.decision?.reasons));
  const after = await listing(env);
  assert.deepEqual(after.tags, ['fiction', 'science-fiction', 'harbour', 'tides', 'ships']);
  assert.equal(fictionLabelled(after.tags), true);
});

test('no judge tags on a quarantine or a harm rejection: the listing keeps what it had', async () => {
  for (const [judge, status] of [[judgeAnswer({ verdict: 'reject', articles: [cite('A1', 0.9)], suggested_tags: ['harbour'] }), 'quarantined'], [judgeAnswer({ verdict: 'reject', articles: [cite('A4', 0.95)], suggested_tags: ['harbour'] }), 'rejected']]) {
    const env = modEnv();
    await publish(env);
    await runModerationCase(MSIM, env, pipelineDeps({ judge }));
    const rec = await loadCase(env, MSIM);
    assert.equal(rec.status, status);
    assert.deepEqual(rec.decision.assigned_tags || [], [], status);
    const after = await listing(env);
    assert.deepEqual([after.tags, after.tags_source], [[], undefined], status);
    assert.equal(authorView(rec, after).tags_chosen, null);
  }
});

test('an author\'s tags stand: the judge\'s suggestions are not written, and a misfit is not yet in the email', async () => {
  const env = modEnv();
  await publish(env, { tags: ['zombies', 'horde'] });
  const rec = await runModerationCase(MSIM, env, pipelineDeps({ judge: judgeAnswer({ suggested_tags: ['harbour'] }), jev: jevAnswers({ tag_fit: 0.05 }) }));
  assert.equal(rec.status, 'changes_requested');
  assert.deepEqual(rec.decision.changes, ['not_yet_tags']);
  assert.deepEqual((await listing(env)).tags, ['zombies', 'horde']);
  const mail = mailText(env.sent[0]);
  assert.match(mail, /Not yet: fix your tags \(Code B9\)/);
  assert.match(mail, /What to change: Fix your tags/);
  assert.ok(!mail.includes('Tags chosen'), 'no tags were chosen');
});

test('the rating is the same through the pipeline with and without tags', async () => {
  const ratings = [];
  for (const tags of [[], ['harbour', 'kids', 'mature']]) {
    const env = modEnv();
    await publish(env, { tags });
    const rec = await runModerationCase(MSIM, env, pipelineDeps({ judge: judgeAnswer({ suggested_tags: ['adults-only'] }) }));
    ratings.push([rec.status, rec.decision.rating, (await listing(env)).moderation.rating]);
  }
  assert.deepEqual(ratings[0], ['approved', 'ages_10', 'ages_10']);
  assert.deepEqual(ratings[1], ratings[0]);
});

// ── the routes ───────────────────────────────────────────────────────────────

let seq = 0;
async function routeEnv() {
  const env = fakeEnv();
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  return env;
}
async function seed(env, fields = {}) {
  const id = `00000000-0000-4000-8000-${String(++seq).padStart(12, '0')}`;
  const sim = {
    id, name: `World ${seq}`, description: '', author_id: AUTHOR, author_name: 'host', is_public: true, max_players: 8,
    moderation: { status: 'approved', rating: 'all_ages', featured: true }, published_at: new Date(1_700_000_000_000 + seq * 1000).toISOString(), ...fields,
  };
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
  return id;
}
async function call(env, method, path, { token, body } = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: parsed, text };
}
const stored = async (env, id) => JSON.parse(await env.SOCIAL.get(`sim:${id}`));
const fields = v => ({ tags: v.tags, tags_source: v.tags_source, genre: v.genre });

test('every route that returns a listing carries tags, tags_source and the genre mirror', async () => {
  const env = await routeEnv();
  const legacy = await seed(env, { genre: 'Racing', tags: ['coop'] });
  const judged = await seed(env, { tags: ['harbour', 'tides'], tags_source: 'judge', genre: null });
  const all = await seed(env, { genre: 'all' });
  const want = {
    [legacy]: { tags: ['coop', 'racing'], tags_source: 'author', genre: 'racing' },
    [judged]: { tags: ['harbour', 'tides'], tags_source: 'judge', genre: null },
    [all]: { tags: [], tags_source: 'author', genre: null },
  };
  const check = (list, where) => {
    assert.equal(list.length, 3, where);
    for (const v of list) assert.deepEqual(fields(v), want[v.id], `${where} ${v.id}`);
  };
  check((await call(env, 'GET', '/api/simulations')).body.simulations, '/api/simulations');
  check((await call(env, 'GET', '/api/gallery')).body.simulations, '/api/gallery');
  check((await call(env, 'GET', '/api/gallery/featured')).body.featured, '/api/gallery/featured');
  check((await call(env, 'GET', '/api/projects', { token: author })).body.projects, '/api/projects');
  for (const id of [legacy, judged, all]) assert.deepEqual(fields((await call(env, 'GET', `/api/simulations/${id}`)).body), want[id], `/api/simulations/${id}`);
  // An unlisted listing read by its author carries them too.
  const pending = await seed(env, { genre: 'Racing', moderation: { status: 'pending' } });
  assert.deepEqual(fields((await call(env, 'GET', `/api/simulations/${pending}`, { token: author })).body), { tags: ['racing'], tags_source: 'author', genre: 'racing' });
  // The live strip: a listed listing being hosted.
  const beat = await call(env, 'POST', `/api/simulations/${legacy}/live`, { token: author, body: { link: `eustress-player://join/192.168.1.20:7777?key=${'0123456789abcdef'.repeat(2)}&pin=${'ab'.repeat(32)}`, players: 1, max_players: 4, protocol: 3 } });
  assert.equal(beat.status, 200, beat.text);
  const live = (await call(env, 'GET', '/api/simulations/live')).body.sessions;
  assert.equal(live.length, 1);
  assert.deepEqual(fields(live[0]), want[legacy], '/api/simulations/live');
  // Makers returns people, not listings: no category field, and the listings are still counted.
  const makers = (await call(env, 'GET', '/api/makers')).body.makers;
  assert.deepEqual(makers.map(m => [m.username, m.space_count]), [['host', 3]]);
  assert.equal('tags' in makers[0], false);
});

test('create: tags set the author\'s; [] leaves them to the review; an older Studio\'s genre is ignored unless no tags were sent', async () => {
  const env = await routeEnv();
  const create = body => call(env, 'POST', '/api/simulations/publish', { token: author, body });
  const tagged = await create({ name: 'A', tags: ['Coop'], genre: 'Racing' });
  assert.equal(tagged.status, 201, tagged.text);
  assert.deepEqual(fields(tagged.body), { tags: ['coop'], tags_source: 'author', genre: null });
  const empty = await create({ name: 'B', tags: [], genre: 'Racing' });
  assert.deepEqual(fields(empty.body), { tags: [], tags_source: 'author', genre: null });
  assert.equal(reviewChoosesTags(await stored(env, empty.body.id)), true, 'the review chooses');
  const old = await create({ name: 'C', genre: 'Racing' });
  assert.deepEqual(fields(old.body), { tags: ['racing'], tags_source: 'author', genre: 'racing' }, 'an older Studio: its genre is the one tag');
  for (const genre of ['All', 'all', '', 'Sci Fi']) assert.deepEqual(fields((await create({ name: 'D', genre })).body), { tags: [], tags_source: 'author', genre: null }, genre);
});

test('the lazy migration is written back on the listing\'s next write, and "all" is never a tag', async () => {
  const env = await routeEnv();
  const legacy = await seed(env, { genre: 'Racing', tags: ['coop'] });
  const all = await seed(env, { genre: 'All', tags: ['coop'] });
  const full = await seed(env, { genre: 'Racing', tags: ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'] });
  assert.deepEqual((await stored(env, legacy)).tags, ['coop'], 'read, not yet written');
  for (const id of [legacy, all, full]) assert.equal((await call(env, 'POST', `/api/simulations/${id}/archive`, { token: author })).status, 200);
  const written = await stored(env, legacy);
  assert.deepEqual([written.tags, written.genre_migrated], [['coop', 'racing'], true]);
  assert.deepEqual((await stored(env, all)).tags, ['coop']);
  assert.equal((await stored(env, all)).genre_migrated, undefined, 'nothing to migrate');
  assert.deepEqual((await stored(env, full)).tags, ['t1', 't2', 't3', 't4', 't5', 't6', 't7', 't8'], 'the cap holds on write');
});

test('?genre= aliases ?tag=, normalized; "all" is ignored; it combines with ?tag= (all of them)', async () => {
  const env = await routeEnv();
  const legacy = await seed(env, { genre: 'Racing', tags: ['coop'] });
  const tagged = await seed(env, { tags: ['racing'] });
  const other = await seed(env, { tags: ['harbour'] });
  const ids = async q => (await call(env, 'GET', `/api/simulations${q}`)).body.simulations.map(s => s.id).sort();
  assert.deepEqual(await ids('?genre=Racing'), [legacy, tagged].sort());
  assert.deepEqual(await ids('?genre=%23RACING'), [legacy, tagged].sort());
  assert.deepEqual(await ids('?genre=all'), [legacy, tagged, other].sort());
  assert.deepEqual(await ids('?genre=racing&tag=coop'), [legacy]);
  assert.deepEqual(await ids('?genre=racing&tag=harbour'), []);
  assert.deepEqual(await ids('?genre=Sci%20Fi'), [], 'not a valid tag: matches nothing, no error');
  assert.deepEqual((await call(env, 'GET', '/api/gallery?genre=racing')).body.simulations.map(s => s.id).sort(), [legacy, tagged].sort());
});

test('PATCH /api/simulations/{id}/tags: the same set keeps the judge\'s and is not reviewed; a change is the author\'s and is', async () => {
  const env = await routeEnv();
  const id = await seed(env, { tags: ['harbour', 'tides'], tags_source: 'judge' });
  const patch = (body, token = author) => call(env, 'PATCH', `/api/simulations/${id}/tags`, { token, body });
  const same = await patch({ tags: ['#Tides', 'harbour'] });
  assert.equal(same.status, 200, same.text);
  assert.deepEqual([same.body.changed, same.body.tags, same.body.tags_source, same.body.status], [false, ['tides', 'harbour'], 'judge', 'listed']);
  assert.deepEqual([(await stored(env, id)).moderation.status, (await stored(env, id)).tags_source], ['approved', 'judge']);
  const changed = await patch({ tags: ['harbour', 'ferries'] });
  assert.deepEqual([changed.body.changed, changed.body.tags_source, changed.body.status], [true, 'author', 'in_review']);
  assert.equal((await stored(env, id)).moderation.status, 'pending', 'sent back to review, as a commit\'s changed text is');
  const before = await env.SOCIAL.get(`sim:${id}`);
  for (const [body, status] of [[{ tags: ['sci_fi'] }, 400], [{ tags: ['555-1234'] }, 400], [{}, 400], [{ tags: null }, 400]]) {
    const r = await patch(body);
    assert.equal(r.status, status, JSON.stringify(body));
    assert.equal(r.body.code, 'bad_tags');
  }
  assert.equal((await patch({ tags: ['x1'] }, stranger)).status, 404, 'only the author');
  assert.equal((await call(env, 'PATCH', `/api/simulations/${id}/tags`, { body: { tags: ['x1'] } })).status, 401);
  assert.equal(await env.SOCIAL.get(`sim:${id}`), before, 'a refusal writes nothing');
  const cleared = await patch({ tags: [] });
  assert.deepEqual([cleared.body.changed, cleared.body.tags], [true, []]);
  assert.equal(reviewChoosesTags(await stored(env, id)), true, '[] asks the review to choose');
  assert.equal(routePolicy('PATCH', `/api/simulations/${id}/tags`).class, FRESH);
  assert.match(corsHeaders(new Request('https://api.eustress.dev/'), {})['Access-Control-Allow-Methods'], /PATCH/);
});

// The commit route, with the fakes tests/listing_tags.test.mjs uses.
const WSIM = '11111111-2222-3333-4444-555555555556';
const H1 = 'a'.repeat(64);
function echk() {
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
const C1 = echk();
const manifest = { format: 'echk', encoder_version: 1, chunk_size: 256, engine_version: '0.3.6', universe: 'Box Head', start_space: 'Main', spaces: [{ name: 'Main', chunks: [{ cx: 0, cz: 0, file: '0_0.echk', size: C1.length, count: 1, blake3: H1 }] }], assets: [] };
const worldDeps = {
  verifyAuth: async (req) => (req.headers.get('Authorization') || '').replace(/^Bearer /, '') || null,
  requireAdmin: async () => null, json, isListable, canServe,
  readSpaceNames: (list) => (Array.isArray(list) ? [...new Set(list)] : null),
  MODERATION_VERSION: '1.0', POLICY_VERSION,
};
async function worldEnv(fieldsOf = {}) {
  const env = { SOCIAL: kv(), USERS: kv(), SCENES: r2() };
  env.SOCIAL.m.set(`sim:${WSIM}`, JSON.stringify({ id: WSIM, name: 'Box Head', author_id: 'author', is_public: true, version: 1, max_players: 10, moderation: { status: 'pending' }, ...fieldsOf }));
  await env.SCENES.put(`universes/${WSIM}/chunks/${H1}.echk`, C1);
  return env;
}
const worldStored = env => JSON.parse(env.SOCIAL.m.get(`sim:${WSIM}`));
async function commit(env, listingSent) {
  const url = new URL(`https://api.eustress.dev/api/simulations/${WSIM}/world/commit`);
  const body = JSON.stringify({ manifest_json: JSON.stringify(manifest), ...(listingSent === undefined ? {} : { listing: listingSent }) });
  const res = await handleWorldRoute(new Request(url, { method: 'POST', headers: { Authorization: 'Bearer author', 'Content-Type': 'application/json' }, body }), url, env, {}, worldDeps);
  return { status: res.status, body: await res.json() };
}
/// Committed once, then approved with `fields` (the judge's tags, say): the next commit is the one a test reads.
async function approvedWorld(fieldsOf) {
  const env = await worldEnv();
  assert.equal((await commit(env, { name: 'Box Head' })).status, 200);
  env.SOCIAL.m.set(`sim:${WSIM}`, JSON.stringify({ ...worldStored(env), ...fieldsOf, moderation: { status: 'approved' } }));
  return env;
}

test('a commit sending the judge\'s tags back keeps them the judge\'s with no re-review; a changed set is the author\'s and is reviewed', async () => {
  const env = await approvedWorld({ tags: ['harbour', 'tides'], tags_source: 'judge' });
  const same = await commit(env, { name: 'Box Head', tags: ['tides', '#Harbour'] });
  assert.equal(same.status, 200, JSON.stringify(same.body));
  assert.deepEqual([worldStored(env).tags, worldStored(env).tags_source, worldStored(env).moderation.status], [['tides', 'harbour'], 'judge', 'approved']);
  assert.deepEqual([same.body.tags, same.body.tags_source], [['tides', 'harbour'], 'judge'], 'the commit answers with what the listing carries');
  const changed = await commit(env, { name: 'Box Head', tags: ['tides', 'ferries'] });
  assert.equal(changed.status, 200);
  assert.deepEqual([worldStored(env).tags_source, worldStored(env).moderation.status], ['author', 'pending']);
});

test('a commit from an older Studio: a genre is ignored when the listing has tags, and an empty one is absent', async () => {
  const env = await approvedWorld({ tags: ['harbour'] });
  for (const genre of ['Horror', '', '   ', 'all']) {
    const r = await commit(env, { name: 'Box Head', genre });
    assert.equal(r.status, 200);
    assert.deepEqual([worldStored(env).tags, worldStored(env).moderation.status], [['harbour'], 'approved'], JSON.stringify(genre));
  }
  // A listing with no tags at all takes a valid genre as its one tag (the author's), which the review reads.
  const bare = await approvedWorld({ tags: [] });
  for (const genre of ['', 'all']) {
    await commit(bare, { name: 'Box Head', genre });
    assert.deepEqual([worldStored(bare).tags, worldStored(bare).moderation.status], [[], 'approved'], JSON.stringify(genre));
  }
  await commit(bare, { name: 'Box Head', genre: 'Racing' });
  assert.deepEqual([worldStored(bare).tags, worldStored(bare).tags_source, worldStored(bare).genre, worldStored(bare).moderation.status], [['racing'], 'author', 'racing', 'pending']);
  // Sent with tags, a genre is ignored, and [] leaves the tags to the review.
  const withTags = await approvedWorld({ tags: [] });
  await commit(withTags, { name: 'Box Head', tags: [], genre: 'Racing' });
  assert.deepEqual(worldStored(withTags).tags, []);
});

test('a commit to a listing from before tags writes its migrated genre back', async () => {
  const env = await approvedWorld({ genre: 'Racing', tags: ['coop'] });
  assert.equal((await commit(env, { name: 'Box Head' })).status, 200);
  assert.deepEqual([worldStored(env).tags, worldStored(env).genre_migrated, worldStored(env).moderation.status], [['coop', 'racing'], true, 'approved'], 'the same tags, written: no change to review');
});

test('normalizeTag and checkTags still agree on every tag the judge may write', () => {
  for (const tag of judgeTags(['#Harbour', 'ships', 'Science-Fiction'])) assert.deepEqual(checkTags([tag]), { ok: true, tags: [tag] });
  assert.equal(DEFAULT_THRESHOLDS.article_system_cite, 0.85);
});
