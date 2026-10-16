// The Guardian Code listing bar (v2.0, carried into v2.1). A listing is earned on four scored tests and a Qualification Statement that matches;
// a listing is acted on only with enough confidence, a Space that is close is told the change, a game is listed by a person,
// featured is a person's call, and fear is a Code B1 signal, never a rating.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  decideFinal, decideTriage, deterministicSignals, parseJudgeVerdict, makeToolExecutor, extractQualificationStatement,
  POLICY_VERSION, POLICY_HASH_ANCHORED, GAME_KINDS, DEFAULT_THRESHOLDS, AUTHOR_COPY, CODE_TESTS,
} from '../src/moderation.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { questionnaireRecord, questionnaireOf } from '../src/rating_questionnaire.mjs';
import { questionnaireAnswers } from './helpers/claude_fake.mjs';

const ANSWERED = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers()) });

const SIM = 'a1b2c3d4-0000-4000-8000-0000000000b1';

const STATEMENT = extractQualificationStatement([
  'Category: Simulation',
  'Objective: Set the tide range and read the harbour depth it gives.',
  'Lesson: How tidal range and basin shape set water depth.',
  'Real-world link: Models the tidal basin of Port Phillip from the published chart datum.',
].join('\n'));

const tests = (scores = {}) => Object.fromEntries(CODE_TESTS.map(k => [k, { score: scores[k] ?? 2, evidence: `${k}: the tide gauge readout in capture-1 and Soul/tide/tide.luau` }]));
const judge = (over = {}) => ({
  malformed: false, verdict: 'publish', articles: [], tests: tests(over.scores), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [],
  is_game: false, is_fiction: false, featured_candidate: false, policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED,
  spatial_evidence: ['central plaza with four symmetric fountains'], rationale: 'Every test is shown.',
  confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, ...over,
});
const triage = (over = {}) => ({
  outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'simulation', rating: 'all_ages',
  articles: { system: [], signals: [] }, statement: STATEMENT, scripts_truncated: false, game_mechanics: 0.02, questionnaire: ANSWERED, ...over,
});
// A Part VII answer as the judge would send it, before parsing.
const body = (over = {}) => JSON.stringify({
  verdict: 'publish', articles: [], tests: tests(), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false,
  policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED, spatial_evidence: ['a plaza with a purpose'], rationale: 'Every test is shown.',
  confidence: 0.8, flagged_for_review: false, suggested_edit_to_publish: null, ...over,
});

function env() {
  const kv = new Map();
  return {
    kv,
    SOCIAL: { get: async k => kv.get(k) ?? null, put: async (k, v) => { kv.set(k, String(v)); }, delete: async k => { kv.delete(k); }, list: async () => ({ keys: [], list_complete: true }) },
    SCENES: { get: async () => null, head: async () => null, put: async () => null, delete: async () => {}, list: async () => ({ objects: [] }) },
    USERS: { get: async () => null, put: async () => {}, delete: async () => {} },
    AUDIT_LOG: { get: async () => null, put: async () => {} },
  };
}
async function agentApproves(caseOver, args = {}) {
  const e = env();
  await e.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'user-1', status: 'held', history: [], triage: triage(), judge: judge(), ...caseOver }));
  const run = makeToolExecutor(e, { auditLog: async () => {}, json: v => v }, { actor: 'agent', actorId: 'grok-agent' });
  const res = await run('moderation_approve', { sim_id: SIM, rating: 'all_ages', rationale: 'The judge scored every test and the statement matches the Space.', ...args });
  return { res, status: JSON.parse(e.kv.get(`modcase:${SIM}`)).status };
}

test('the Code is version 2.3, and the text the judge reads is the one anchored', () => {
  assert.equal(POLICY_VERSION, '2.3');
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  assert.equal(POLICY_HASH_ANCHORED, 'sha256:99ced3f2e27e715a52483581375cb9eda9401944ca8f4412483da2f27e5f39b0');
  assert.match(GUARDIAN_POLICY_TEXT, /^# Eustress Guardian Code v2\.3/);
  assert.match(GUARDIAN_POLICY_TEXT, /## Part IV\. Verdict Rules/);
  // Written with a code point, not an escape, so no tool that decodes an escape in this file can change what it means.
  assert.equal(GUARDIAN_POLICY_TEXT.includes(String.fromCharCode(0x2014)), false, 'no em dashes in the Code');
});

test('a Space that passes all four tests with a matching statement is approved; a featured proposal is recorded, never set', () => {
  const d = decideFinal({ triage: triage(), judge: judge() });
  assert.deepEqual([d.outcome, d.featured, d.featured_candidate], ['approve', false, false]);
  const proposed = decideFinal({ triage: triage(), judge: judge({ featured_candidate: true, scores: { build_quality: 3, objective: 2, lesson: 3, real_world: 3 } }) });
  assert.deepEqual([proposed.outcome, proposed.featured, proposed.featured_candidate], ['approve', false, true]);
});

test('a listing is earned: a qualifying answer below the listing confidence is held for a person (rule 6)', () => {
  const d = decideFinal({ triage: triage(), judge: judge({ confidence: DEFAULT_THRESHOLDS.listing_confidence - 0.01 }) });
  assert.equal(d.outcome, 'hold');
  assert.equal(d.reasons.at(-1).code, 'judge_unsure_to_list');
  assert.equal(d.reasons.at(-1).rule, 6);
  assert.equal(DEFAULT_THRESHOLDS.listing_confidence, 0.75);
});

test('not yet asks the author for a specific change: one test at 1 with an edit gives changes_requested naming the test (rule 7)', () => {
  const edit = 'Show the harbour depth on a gauge the visitor can read.';
  const d = decideFinal({ triage: triage(), judge: judge({ scores: { objective: 1 }, suggested_edit_to_publish: edit }) });
  assert.equal(d.outcome, 'changes_requested');
  assert.equal(d.lane, 'quality');
  assert.deepEqual([d.reasons.at(-1).code, d.reasons.at(-1).rule], ['test_objective', 7]);
  assert.deepEqual(d.changes, ['test_objective']);
  assert.equal(d.suggested_edit, edit);
  assert.ok(AUTHOR_COPY.test_objective, 'the author sees a catalogue entry for the test');
});

test('not yet on a case the triage escalated stands, and the rules decide its listing too (v2.1: no agent confirms it)', () => {
  const d = decideFinal({ triage: triage({ outcome: 'escalate', lane: 'harm' }), judge: judge({ scores: { lesson: 1 }, suggested_edit_to_publish: 'Let the visitor change the basin shape and see the depth change.' }) });
  assert.equal(d.outcome, 'changes_requested');
  const listing = decideFinal({ triage: triage({ outcome: 'escalate', lane: 'harm' }), judge: judge() });
  assert.equal(listing.outcome, 'approve', 'nothing cited and nothing signalled: the rules list it');
  const signalled = decideFinal({ triage: triage({ outcome: 'escalate', lane: 'harm', articles: { system: [], signals: [{ article: 'A9', key: 'hate_incitement', p: 0.5, group: 'A' }] } }), judge: judge() });
  assert.equal(signalled.outcome, 'hold', 'a gray-band signal the judge did not cite goes to a person');
});

test('a game is never listed automatically: a qualifying answer holds it for a person (rule 8)', () => {
  assert.deepEqual(GAME_KINDS, ['interactive_experience']);
  for (const featured_candidate of [false, true]) {
    const d = decideFinal({ triage: triage({ content_kind: 'interactive_experience' }), judge: judge({ featured_candidate, scores: { build_quality: 3, lesson: 3, real_world: 3 } }) });
    assert.equal(d.outcome, 'hold');
    assert.equal(d.reasons.at(-1).code, 'game_needs_a_person');
    assert.equal(d.featured_candidate, featured_candidate, 'the person who takes the game sees the proposal');
  }
});

test('denial still applies to a game: the rules can reject it or ask for changes', () => {
  const game = triage({ content_kind: 'interactive_experience' });
  assert.equal(decideFinal({ triage: game, judge: judge({ scores: { lesson: 0 } }) }).outcome, 'reject');
  assert.equal(decideFinal({ triage: game, judge: judge({ scores: { objective: 1 }, suggested_edit_to_publish: 'Show the course time against the target.' }) }).outcome, 'changes_requested');
});

test('an answer missing a test is malformed and held, never approved by default', () => {
  const missing = JSON.parse(body());
  delete missing.tests.real_world;
  const parsed = parseJudgeVerdict(JSON.stringify(missing));
  assert.deepEqual([parsed.malformed, parsed.error], [true, 'judge_missing_test']);
  const d = decideFinal({ triage: triage(), judge: parsed });
  assert.equal(d.outcome, 'hold');
  assert.equal(d.reasons.at(-1).code, 'judge_missing_test');
});

test('the judge reports articles and scores, not an outcome: its enums are checked and any `quality` it adds is ignored', () => {
  for (const statement_check of ['matches', 'missing', 'contradicted']) assert.equal(parseJudgeVerdict(body({ statement_check })).malformed, false, statement_check);
  for (const rating_floor of ['all_ages', 'ages_10', 'teen_13', 'mature_17', 'adult_18']) assert.equal(parseJudgeVerdict(body({ rating_floor })).malformed, false, rating_floor);
  assert.equal(parseJudgeVerdict(body({ statement_check: 'maybe' })).error, 'judge_bad_enum');
  assert.equal(parseJudgeVerdict(body({ rating_floor: 'adults' })).error, 'judge_bad_enum');
  const withQuality = parseJudgeVerdict(body({ quality: 'featured' }));
  assert.equal(withQuality.malformed, false);
  assert.equal('quality' in withQuality, false, 'the system computes the outcome');
});

test('the agent cannot list a game: the approval is refused before anything is written', async () => {
  const { res, status } = await agentApproves({ triage: triage({ content_kind: 'interactive_experience' }) });
  assert.equal(res.ok, false);
  assert.match(res.error, /a game is listed by a person/);
  assert.equal(status, 'held', 'the case is still held');
});

test('a Space the judge calls a game is held for a person, whatever Jev filed it as', () => {
  const d = decideFinal({ triage: triage({ content_kind: 'simulation' }), judge: judge({ is_game: true }) });
  assert.equal(d.outcome, 'hold');
  assert.equal(d.reasons.at(-1).code, 'game_needs_a_person');
  assert.equal(decideFinal({ triage: triage({ content_kind: 'simulation' }), judge: judge() }).outcome, 'approve', 'not a game when nothing says so');
});

test('the judge\'s is_game and featured_candidate are read strictly: a boolean, or the answer is malformed', () => {
  assert.equal(parseJudgeVerdict(body({ is_game: true })).is_game, true);
  assert.equal(parseJudgeVerdict(body({ is_game: false })).is_game, false);
  const absent = JSON.parse(body());
  delete absent.is_game;
  assert.equal(parseJudgeVerdict(JSON.stringify(absent)).error, 'judge_bad_enum', 'absent is not an answer');
  assert.equal(parseJudgeVerdict(body({ is_game: 'yes' })).error, 'judge_bad_enum', 'a string is not a flag');
  assert.equal(parseJudgeVerdict(body({ featured_candidate: 1 })).error, 'judge_bad_enum');
});

test('the agent cannot list a Space the judge calls a game, even when Jev filed it as a simulation', async () => {
  const { res } = await agentApproves({ judge: judge({ is_game: true }) });
  assert.equal(res.ok, false);
  assert.match(res.error, /a game is listed by a person/);
});

test('the agent cannot approve what the rules would not list: a test at 1, or an answer below the listing confidence', async () => {
  for (const [name, j] of [['a test at 1', judge({ scores: { objective: 1 }, suggested_edit_to_publish: 'Add a readout.' })], ['low confidence', judge({ confidence: DEFAULT_THRESHOLDS.listing_confidence - 0.05 })]]) {
    const { res, status } = await agentApproves({ triage: triage({ outcome: 'escalate' }), judge: j });
    assert.equal(res.ok, false, name);
    assert.match(res.error, /would not list/, name);
    assert.equal(status, 'held', name);
  }
  assert.equal((await agentApproves({ triage: triage({ outcome: 'escalate' }) })).res.ok, true, 'control: a case the rules would list');
});

test('a test at 1 with no change named is held for a person, never sent to the author as an instruction with nothing in it', () => {
  const d = decideFinal({ triage: triage(), judge: judge({ scores: { lesson: 1 }, suggested_edit_to_publish: null }) });
  assert.equal(d.outcome, 'hold');
  assert.equal(d.reasons.at(-1).code, 'judge_not_yet_without_change');
  const blank = decideFinal({ triage: triage(), judge: judge({ scores: { lesson: 1 }, suggested_edit_to_publish: '  ' }) });
  assert.equal(blank.outcome, 'hold', 'an empty edit counts as none');
  const named = decideFinal({ triage: triage(), judge: judge({ scores: { lesson: 1 }, suggested_edit_to_publish: 'Let the visitor change the basin shape.' }) });
  assert.equal(named.outcome, 'changes_requested');
});

test('the craft profile is measured from the digest, and is absent when there is no digest', () => {
  const dossier = { digest: { entity_count: 12, default_name_fraction: 0.25, default_material_fraction: 0.5, default_color_fraction: 0.4, unique_materials: 3, unique_colors: 4, hierarchy_depth: 2, duplicate_transform_fraction: 0.1, script_count: 1, script_lines: 40, text_string_count: 2, spawn_count: 1, gui_count: 0, sound_count: 0 } };
  const craft = deterministicSignals({ id: SIM }, dossier).craft;
  assert.deepEqual(craft, {
    non_default_name_fraction: 0.75, non_default_material_fraction: 0.5, non_default_color_fraction: 0.6,
    unique_materials: 3, unique_colors: 4, hierarchy_depth: 2, duplicate_transform_fraction: 0.1,
    script_count: 1, script_lines: 40, text_string_count: 2, spawn_count: 1, gui_count: 0, sound_count: 0,
  });
  assert.equal(deterministicSignals({ id: SIM }, null).craft, undefined, 'no digest, no profile');
});

test('the craft profile is recorded only: adding it does not change any triage decision', () => {
  const dossier = { digest: { entity_count: 5, default_name_fraction: 0.2, default_material_fraction: 0.9, default_color_fraction: 0.9, script_count: 0, text_string_count: 0 } };
  const withProfile = deterministicSignals({ id: SIM }, dossier);
  const without = { ...withProfile };
  delete without.craft;
  assert.ok(withProfile.craft, 'the profile is there');
  assert.deepEqual(decideTriage({ signals: withProfile, jev: null }), decideTriage({ signals: without, jev: null }));
});

// Fear is a Code B1 signal, never a rating (Part V: ratings say how intense, and adult_18 is sexual content only).
const withFear = (choice, probs, confidence) => ({
  answers: {
    content_rating: choice === null ? undefined : { choice, confidence: 0.9, probabilities: {} },
    fear_level: { probabilities: probs, confidence },
  },
});
const fearSignals = d => d.articles.signals.filter(s => s.key === 'fear_level');

test('fear: frightening material is a B1 signal at confidence 0.55 or above, and leaves the rating and the outcome alone', () => {
  const d = decideTriage({ signals: {}, jev: withFear('all_ages', { none: 0.1, mild: 0.1, frightening: 0.8 }, 0.8) });
  assert.equal(d.rating, 'all_ages', 'fear sets no rating');
  assert.equal(d.outcome, 'judge', 'the judge still runs: the rules need its scores');
  assert.deepEqual(fearSignals(d).map(s => [s.article, s.p]), [['B1', 0.8]]);
  assert.equal(DEFAULT_THRESHOLDS.fear_signal_confidence, 0.55);
});

test('fear: mild spookiness is neither a signal nor a rating', () => {
  const d = decideTriage({ signals: {}, jev: withFear('all_ages', { none: 0.1, mild: 0.8, frightening: 0.1 }, 0.8) });
  assert.deepEqual([d.rating, fearSignals(d).length], ['all_ages', 0]);
});

test('fear: an unconfident frightening answer is no signal', () => {
  const d = decideTriage({ signals: {}, jev: withFear('all_ages', { none: 0.1, mild: 0.1, frightening: 0.8 }, 0.5) });
  assert.equal(fearSignals(d).length, 0);
});

test('fear: a rating already higher is kept as it is', () => {
  const d = decideTriage({ signals: {}, jev: withFear('adult_18', { none: 0.1, mild: 0.1, frightening: 0.8 }, 0.9) });
  assert.equal(d.rating, 'adult_18');
  assert.equal(fearSignals(d).length, 1);
});

test('fear: with no content rating answer, frightening material leaves the rating unset and is still a signal', () => {
  const d = decideTriage({ signals: {}, jev: withFear(null, { none: 0.1, mild: 0.1, frightening: 0.8 }, 0.8) });
  assert.equal(d.rating, null);
  assert.equal(fearSignals(d).length, 1);
  // A signal the judge did not cite holds the case for a person (rule 6).
  const held = decideFinal({ triage: { ...d, statement: STATEMENT }, judge: judge() });
  assert.deepEqual([held.outcome, held.reasons.at(-1).code, held.reasons.at(-1).article], ['hold', 'article_signal_not_cited', 'B1']);
});
