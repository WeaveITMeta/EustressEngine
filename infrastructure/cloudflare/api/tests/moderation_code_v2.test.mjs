// The Eustress Guardian Code (docs/architecture/AI_GUARDIAN_POLICY_v1.0.md, v2.1), as the Worker applies it: one test per
// Part IV rule and per ordering conflict between rules, the system citations and signals the triage raises, the Part VII
// answer the judge must give, the Qualification Statement, the limits on anyone but a person, and what the author is told.
// The v2.1 additions (A12 to A14, attestations, B6 sub-items, fiction, Product, ratings, the questionnaire, the Claude judge,
// the researcher, the shadow and the Review Workspace) are in tests/moderation_code_v21.test.mjs.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  decideFinal, decideTriage, deterministicSignals, parseJudgeVerdict, buildJevState, buildJevQuestions, extractQualificationStatement,
  prioritizeStrings, wouldList, isGame, makeToolExecutor, runModerationCase, loadCase, authorView, claudeJudge, thresholdsFrom,
  isListable, canServe, DEFAULT_THRESHOLDS, AUTHOR_COPY, CODE_TESTS, GROUP_B, ARTICLE_SIGNALS, POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import { GUARDIAN_POLICY_TEXT, MODERATION_PLAYBOOK_TEXT } from '../src/generated/policy_text.mjs';
import { fakeClaude, apiError, questionnaireAnswers } from './helpers/claude_fake.mjs';
import { questionnaireRecord, questionnaireOf } from '../src/rating_questionnaire.mjs';

const ANSWERED = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers()) });

// ── decision fixtures ───────────────────────────────────────────────────────

// Written with code points, not escapes, so no tool that decodes an escape in this file can change what it means.
const EM_DASH = String.fromCharCode(0x2014);
const EN_DASH = String.fromCharCode(0x2013);

const STATEMENT_TEXT = [
  'Category: Simulation',
  'Objective: Set the tide range and read the depth at the quay.',
  'Lesson: How tidal range and basin shape set harbour depth.',
  'Real-world link: Models a tidal harbour from published tide tables.',
].join('\n');
const STATEMENT = extractQualificationStatement(STATEMENT_TEXT);
const NO_STATEMENT = extractQualificationStatement('A working harbour.');

// Every test at 2 unless named; a name given as null is scored null.
const tests = (scores = {}) => Object.fromEntries(CODE_TESTS.map(k => [k, { score: k in scores ? scores[k] : 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
const judge = (over = {}) => {
  const { scores, ...rest } = over;
  return {
    malformed: false, verdict: 'publish', articles: [], tests: tests(scores), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [],
    is_game: false, is_fiction: false, featured_candidate: false, spatial_evidence: ['four quay cranes along the east basin'], rationale: 'Every test is shown.',
    confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, ...rest,
  };
};
const cite = (id, confidence = 0.9) => ({ id, confidence, evidence: `Code ${id}: the GUI string 'Rebirth' in Soul/ui/shop.luau` });
const sys = (article, key, p = 0.9) => ({ article, key, p });
const triage = (over = {}) => ({
  outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'simulation', rating: 'all_ages', articles: { system: [], signals: [] },
  statement: STATEMENT, scripts_truncated: false, game_mechanics: 0.02, lawful_decision_training: 0.02,
  questionnaire: ANSWERED, fiction_p: 0.02, fiction_labelled: false, account: { attested_properties: [], verified_business: false, payment_connection: false }, ...over,
});
const decide = (t = {}, j = {}) => decideFinal({ triage: triage(t), judge: judge(j) });
const last = d => d.reasons.at(-1);
const withSystem = (...system) => ({ articles: { system, signals: [] } });
const withSignals = (...signals) => ({ articles: { system: [], signals } });

// ── rule 0 ──────────────────────────────────────────────────────────────────

test('rule 0: a flagged, unsure or malformed answer is held for a person', () => {
  assert.deepEqual([decide({}, { flagged_for_review: true }).outcome, last(decide({}, { flagged_for_review: true })).code], ['hold', 'judge_flagged']);
  assert.equal(decide({}, { verdict: 'flag_for_human_review' }).outcome, 'hold');
  const unsure = decide({}, { confidence: 0.59 });
  assert.deepEqual([unsure.outcome, last(unsure).rule], ['hold', 0]);
  const malformed = decideFinal({ triage: triage(), judge: { malformed: true, error: 'judge_unknown_article' } });
  assert.deepEqual([malformed.outcome, last(malformed).code, last(malformed).rule], ['hold', 'judge_unknown_article', 0]);
  assert.equal(DEFAULT_THRESHOLDS.judge_min_confidence, 0.60);
});

test('rule 0 before rule 1: a flag that cites A1 or A11 is held in the legal lane (content locked), a flag that cites A4 is held, not rejected', () => {
  const a1 = decide({}, { flagged_for_review: true, articles: [cite('A1', 0.6)] });
  assert.deepEqual([a1.outcome, a1.lane, last(a1).rule], ['hold', 'legal', 0]);
  assert.equal(decide({}, { verdict: 'flag_for_human_review', articles: [cite('A11')] }).lane, 'legal');
  const a4 = decide({}, { flagged_for_review: true, articles: [cite('A4')] });
  assert.deepEqual([a4.outcome, a4.lane], ['hold', 'judge']);
});

// ── rule 1 ──────────────────────────────────────────────────────────────────

test('rule 1: A1, A2, A3, A5 and A6 quarantine, through the hard category the legal lane uses', () => {
  const category = { A1: 'csam_or_minor_sexualization', A2: 'terrorism_or_extremist_promotion', A3: 'mass_casualty_attack_planning', A5: 'ncii_or_real_person_sexual', A6: 'doxxing_or_targeted_harassment' };
  for (const [id, code] of Object.entries(category)) {
    const d = decide({}, { verdict: 'reject', articles: [cite(id)] });
    assert.deepEqual([d.outcome, d.lane, last(d).article, last(d).code, last(d).rule], ['quarantine', 'legal', id, code, 1], id);
  }
});

test('rule 1: A4, A7, A8 and A9 reject in the harm lane; A10 holds in the harm lane and A11 in the legal lane', () => {
  for (const id of ['A4', 'A7', 'A8', 'A9']) {
    const d = decide({}, { verdict: 'reject', articles: [cite(id)] });
    assert.deepEqual([d.outcome, d.lane, last(d).article], ['reject', 'harm', id], id);
  }
  assert.deepEqual([decide({}, { articles: [cite('A10')] }).outcome, decide({}, { articles: [cite('A10')] }).lane], ['hold', 'harm']);
  assert.deepEqual([decide({}, { articles: [cite('A11')] }).outcome, decide({}, { articles: [cite('A11')] }).lane], ['hold', 'legal']);
});

test('rule 1, its own order: a quarantine article beats a reject article, and a reject beats a hold', () => {
  assert.equal(decide({}, { verdict: 'reject', articles: [cite('A4'), cite('A1', 0.6)] }).outcome, 'quarantine');
  assert.equal(decide({}, { verdict: 'reject', articles: [cite('A10'), cite('A7')] }).outcome, 'reject');
});

test('rule 1 before rule 2: a Group A citation decides before a Group B citation or a system citation', () => {
  const d = decide(withSystem(sys('B1', 'scripted_startle')), { verdict: 'reject', articles: [cite('A4'), cite('B1')] });
  assert.deepEqual([d.outcome, d.lane, last(d).article, last(d).rule], ['reject', 'harm', 'A4', 1]);
});

// ── rule 2 ──────────────────────────────────────────────────────────────────

test('rule 2: a B5 system citation with every test at 2 or above is not yet, naming the loop to remove', () => {
  const d = decide(withSystem(sys('B5', 'accumulation_loop')));
  assert.deepEqual([d.outcome, d.lane, last(d).article, last(d).rule, last(d).source], ['changes_requested', 'quality', 'B5', 2, 'system']);
  assert.deepEqual(d.changes, ['not_yet_B5']);
  const view = authorView({ status: 'changes_requested', decision: { changes: d.changes, lane: d.lane } }, {});
  assert.deepEqual(view.reasons.map(r => r.category), ['not_yet_B5']);
  assert.match(view.reasons[0].why, /Code B5/);
});

test('rule 2: B5 with T3 at 1 is rejected under B5', () => {
  const d = decide(withSystem(sys('B5', 'accumulation_loop')), { scores: { lesson: 1 } });
  assert.deepEqual([d.outcome, last(d).article, last(d).rule], ['reject', 'B5', 2]);
});

test('rule 2: B6 and a B9 wording fault are not yet; Jev\'s false teaching is not a wording fault and is rejected', () => {
  assert.deepEqual(decide({}, { articles: [cite('B6', 0.8)] }).changes, ['not_yet_B6']);
  assert.deepEqual(decide({}, { articles: [cite('B9', 0.8)] }).changes, ['not_yet_B9']);
  const falseTeaching = decide(withSystem(sys('B9', 'false_teaching')), { articles: [cite('B9', 0.8)] });
  assert.deepEqual([falseTeaching.outcome, last(falseTeaching).article], ['reject', 'B9']);
});

test('rule 2: B7 is held for a person; B3 is held when Jev reads lawful decision training, and rejected otherwise', () => {
  assert.deepEqual([decide({}, { articles: [cite('B7')] }).outcome, last(decide({}, { articles: [cite('B7')] })).article], ['hold', 'B7']);
  assert.equal(decide({ lawful_decision_training: 0.7 }, { articles: [cite('B3')] }).outcome, 'hold');
  assert.equal(decide(withSystem(sys('B3', 'harm_scored')), {}).outcome, 'reject');
  assert.equal(decide({ ...withSystem(sys('B3', 'harm_scored')), lawful_decision_training: 0.6 }, {}).outcome, 'hold');
});

test('rule 2: a system citation the judge did not cite still decides; the judge cannot clear it', () => {
  const d = decide(withSystem(sys('B1', 'scripted_startle', 0.86)), { articles: [] });
  assert.deepEqual([d.outcome, last(d).article, last(d).source], ['reject', 'B1', 'system']);
  // Every other article of Group B rejects the same way.
  for (const a of GROUP_B.filter(a => !['B3', 'B5', 'B6', 'B7', 'B9'].includes(a))) assert.equal(decide(withSystem(sys(a, 'k')), {}).outcome, 'reject', a);
});

test('rule 2: the judge\'s own Group B citation decides at 0.75; at 0.74 a person decides (rule 6)', () => {
  assert.equal(decide({}, { articles: [cite('B4', 0.75)] }).outcome, 'reject');
  const unsure = decide({}, { articles: [cite('B4', 0.74)] });
  assert.deepEqual([unsure.outcome, last(unsure).code, last(unsure).rule], ['hold', 'article_unsure', 6]);
});

test('rule 2, its own order: a citation that rejects beats one that holds, and one that holds beats a removable feature', () => {
  assert.equal(decide({}, { articles: [cite('B7'), cite('B1')] }).outcome, 'reject');
  assert.equal(decide({}, { articles: [cite('B7'), cite('B5')] }).outcome, 'hold');
});

test('rule 2 before rule 3: B1 with a contradicted statement is rejected under B1', () => {
  const d = decide({}, { articles: [cite('B1')], statement_check: 'contradicted' });
  assert.deepEqual([d.outcome, last(d).article, last(d).rule], ['reject', 'B1', 2]);
});

test('rule 2 before rule 4: B5 with a test at 0 is rejected under B5, not under the test', () => {
  const d = decide(withSystem(sys('B5', 'return_reward')), { scores: { objective: 0 } });
  assert.deepEqual([d.outcome, last(d).article, last(d).rule], ['reject', 'B5', 2]);
});

// ── rule 3 ──────────────────────────────────────────────────────────────────

test('rule 3: a contradicted statement is rejected under B9, before a failing test', () => {
  const d = decide({}, { statement_check: 'contradicted', scores: { build_quality: 0 } });
  assert.deepEqual([d.outcome, last(d).code, last(d).article, last(d).rule], ['reject', 'statement_contradicted', 'B9', 3]);
});

// ── rule 4 ──────────────────────────────────────────────────────────────────

test('rule 4: a B9 citation at 0.6 with T3 at 0 is rejected by rule 4, naming the lesson', () => {
  const d = decide({}, { articles: [cite('B9', 0.6)], scores: { lesson: 0 } });
  assert.deepEqual([d.outcome, last(d).test, last(d).rule], ['reject', 'lesson', 4]);
});

test('rule 4: T4 at 1 with the statement present is rejected (Tower of Hell: a climb, a timer, a real-world theme as decoration)', () => {
  const d = decide({}, { scores: { real_world: 1 }, suggested_edit_to_publish: 'Connect the climb to something real.' });
  assert.deepEqual([d.outcome, last(d).test, last(d).label, last(d).rule], ['reject', 'real_world', 'T4', 4]);
});

test('rule 4 needs the statement present: T4 at 1 with no statement is not yet, naming the test and the statement', () => {
  const d = decide({ statement: NO_STATEMENT }, { scores: { real_world: 1 }, statement_check: 'missing', suggested_edit_to_publish: 'Model the tide from published tables.' });
  assert.deepEqual([d.outcome, d.changes], ['changes_requested', ['test_real_world', 'statement_missing']]);
});

test('rule 4 before rule 6: a test at 0 rejects even when another test is null and the scripts were cut', () => {
  const d = decide({ scripts_truncated: true }, { scores: { objective: 0, lesson: null } });
  assert.deepEqual([d.outcome, last(d).rule], ['reject', 4]);
});

// ── rule 5 ──────────────────────────────────────────────────────────────────

test('rule 5: two or more tests at 1 are rejected, naming them, before a reason to hold', () => {
  const d = decide({ scripts_truncated: true }, { scores: { build_quality: 1, objective: 1 } });
  assert.equal(d.outcome, 'reject');
  assert.deepEqual(d.reasons.filter(r => r.rule === 5).map(r => r.test), ['build_quality', 'objective']);
});

// ── rule 6 ──────────────────────────────────────────────────────────────────

test('rule 6: a test scored null is held for a person', () => {
  const d = decide({}, { scores: { lesson: null } });
  assert.deepEqual([d.outcome, last(d).code, last(d).tests], ['hold', 'test_not_scored', ['lesson']]);
});

test('rule 6: a Jev article signal the judge did not cite is held; the same signal cited at 0.75 decides by rule 2', () => {
  const signal = withSignals({ article: 'B5', key: 'accumulation_loop', p: 0.6 });
  const d = decide(signal, {});
  assert.deepEqual([d.outcome, last(d).code, last(d).article, last(d).key], ['hold', 'article_signal_not_cited', 'B5', 'accumulation_loop']);
  assert.equal(decide(signal, { articles: [cite('B5', 0.8)] }).outcome, 'changes_requested');
});

test('rule 6: scripts Jev could not read in full, and judge confidence below 0.75, each hold', () => {
  assert.equal(last(decide({ scripts_truncated: true }, {})).code, 'scripts_not_read_in_full');
  assert.equal(last(decide({}, { confidence: 0.74 })).code, 'judge_unsure_to_list');
});

test('rule 6 before rule 7: a missing statement with a null test is held, not sent to the author', () => {
  const d = decide({ statement: NO_STATEMENT }, { statement_check: 'missing', scores: { objective: null } });
  assert.deepEqual([d.outcome, last(d).rule], ['hold', 6]);
});

// ── rule 7 ──────────────────────────────────────────────────────────────────

test('rule 7: the statement missing is not yet, and the author is shown the four labelled lines', () => {
  const d = decide({ statement: NO_STATEMENT }, { statement_check: 'missing' });
  assert.deepEqual([d.outcome, d.changes, last(d).rule], ['changes_requested', ['statement_missing'], 7]);
  const view = authorView({ status: 'changes_requested', decision: { changes: d.changes, lane: d.lane } }, {});
  for (const label of ['Category: ', 'Objective: ', 'Lesson: ', 'Real-world link: ']) assert.ok(view.reasons[0].what_to_change.includes(label), label);
});

test('rule 7: one test at 1 and the statement missing name both changes; with no edit for the test, a person decides', () => {
  const both = decide({ statement: NO_STATEMENT }, { statement_check: 'missing', scores: { build_quality: 1 }, suggested_edit_to_publish: 'Replace the placeholder quay with the surveyed one.' });
  assert.deepEqual(both.changes, ['test_build_quality', 'statement_missing']);
  const noEdit = decide({ statement: NO_STATEMENT }, { statement_check: 'missing', scores: { build_quality: 1 } });
  assert.deepEqual([noEdit.outcome, last(noEdit).code], ['hold', 'judge_not_yet_without_change']);
});

// ── rules 8 and 9 ───────────────────────────────────────────────────────────

test('rule 8: every test at 2 or above with a matching statement lists; a present statement the judge did not find matching is held', () => {
  assert.equal(decide().outcome, 'approve');
  const odd = decide({}, { statement_check: 'missing' });
  assert.deepEqual([odd.outcome, last(odd).code], ['hold', 'judge_statement_inconsistent']);
});

test('rule 8: game_mechanics alone makes a game, and a game is held for a person first', () => {
  assert.equal(DEFAULT_THRESHOLDS.game_mechanics, 0.60);
  const game = decide({ content_kind: 'simulation', game_mechanics: 0.6 }, { is_game: false });
  assert.deepEqual([game.outcome, last(game).code], ['hold', 'game_needs_a_person']);
  assert.equal(isGame({ game_mechanics: 0.6 }, { is_game: false }), true);
  assert.equal(decide({ game_mechanics: 0.59 }).outcome, 'approve', 'below the threshold it is not a game');
});

test('rule 9: featured is never automatic; a candidate needs T1, T3 and T4 at 3, the judge\'s proposal, and a rating below adult_18', () => {
  const best = { build_quality: 3, objective: 2, lesson: 3, real_world: 3 };
  const d = decide({}, { featured_candidate: true, scores: best });
  assert.deepEqual([d.outcome, d.featured, d.featured_candidate], ['approve', false, true]);
  assert.equal(decide({}, { featured_candidate: true, scores: { ...best, lesson: 2 } }).featured_candidate, false, 'T3 at 2');
  assert.equal(decide({}, { featured_candidate: false, scores: best }).featured_candidate, false, 'not proposed');
  assert.equal(decide({}, { featured_candidate: true, scores: best, rating_floor: 'adult_18' }).featured_candidate, false, 'adult_18 is never featured');
});

test('the rating is the higher of Jev\'s and the judge\'s rating_floor', () => {
  assert.equal(decide({ rating: 'teen_13' }, { rating_floor: 'mature_17' }).rating, 'mature_17');
  assert.equal(decide({ rating: 'mature_17' }, { rating_floor: 'all_ages' }).rating, 'mature_17');
  assert.equal(decide({ rating: null }, { rating_floor: 'adult_18' }).rating, 'adult_18');
});

test('escalate: a case the triage escalated is decided by the same rules (v2.1: no agent); its signals hold it unless the judge cites them', () => {
  assert.equal(decide({ outcome: 'escalate', lane: 'harm' }).outcome, 'approve', 'an escalation with nothing behind it is the rules\' to list');
  const signalled = decide({ outcome: 'escalate', lane: 'harm', ...withSignals({ article: 'A4', key: 'real_crime_instructions', p: 0.55, group: 'A' }) });
  assert.deepEqual([signalled.outcome, last(signalled).code, last(signalled).article], ['hold', 'article_signal_not_cited', 'A4']);
  assert.equal(decide({ outcome: 'escalate', lane: 'harm', ...withSystem(sys('B4', 'meme_content')) }).outcome, 'reject');
  assert.equal(decide({ outcome: 'escalate', lane: 'harm' }, { scores: { lesson: null } }).outcome, 'hold');
  assert.equal(decideFinal({ triage: triage({ outcome: 'hold', reasons: [{ code: 'x', action: 'hold' }] }), judge: judge() }).outcome, 'hold', 'a triage decision is not reopened');
});

// ── the judge's answer (Part VII) ───────────────────────────────────────────

const answer = (over = {}) => JSON.stringify({
  verdict: 'publish', articles: [], tests: tests(), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false,
  policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED, spatial_evidence: ['four quay cranes along the east basin'], rationale: 'Shown.',
  confidence: 0.8, flagged_for_review: false, suggested_edit_to_publish: null, ...over,
});

test('Part VII: the answer is checked field by field, and a null test score is an answer', () => {
  const ok = parseJudgeVerdict(answer({ articles: [cite('B5', 0.6)], tests: tests({ lesson: null }) }));
  assert.equal(ok.malformed, false);
  assert.deepEqual([ok.articles[0].id, ok.articles[0].confidence, ok.tests.lesson.score, ok.tests.objective.score], ['B5', 0.6, null, 2]);
  assert.equal(parseJudgeVerdict(answer({ articles: [cite('B12')] })).error, 'judge_unknown_article');
  assert.equal(parseJudgeVerdict(answer({ articles: [cite('C1')] })).error, 'judge_unknown_article');
  assert.equal(parseJudgeVerdict(answer({ articles: [cite('B5', 1.2)] })).error, 'judge_bad_article_confidence');
  assert.equal(parseJudgeVerdict(answer({ articles: [{ id: 'B5', confidence: 0.9, evidence: 'looks like a loop' }] })).error, 'judge_generic_article_evidence');
  assert.equal(parseJudgeVerdict(answer({ articles: 'none' })).error, 'judge_bad_articles');
  assert.equal(parseJudgeVerdict(answer({ tests: { ...tests(), lesson: { score: 4, evidence: 'lesson: the tide table panel' } } })).error, 'judge_bad_test_score');
  assert.equal(parseJudgeVerdict(answer({ tests: { ...tests(), lesson: { score: '2', evidence: 'lesson: the tide table panel' } } })).error, 'judge_bad_test_score');
  assert.equal(parseJudgeVerdict(answer({ tests: { ...tests(), lesson: { score: 2, evidence: 'fine' } } })).error, 'judge_generic_test_evidence');
  assert.equal(parseJudgeVerdict(answer({ verdict: 'approve' })).error, 'judge_bad_enum');
  assert.equal(parseJudgeVerdict(answer({ spatial_evidence: ['looks fine'] })).error, 'judge_no_spatial_evidence');
});

test('Part VII: a missing test is malformed, and a reject with no article from A1 to A9 is malformed and held', () => {
  const t = tests();
  delete t.objective;
  assert.equal(parseJudgeVerdict(answer({ tests: t })).error, 'judge_missing_test');
  for (const articles of [[], [cite('B1')], [cite('A10')]]) {
    const parsed = parseJudgeVerdict(answer({ verdict: 'reject', articles }));
    assert.equal(parsed.error, 'judge_reject_without_group_a', JSON.stringify(articles));
    assert.equal(decideFinal({ triage: triage(), judge: parsed }).outcome, 'hold');
  }
  assert.equal(parseJudgeVerdict(answer({ verdict: 'reject', articles: [cite('A4')] })).malformed, false);
});

// ── the Qualification Statement (Part III) ──────────────────────────────────

test('statement: the four labelled lines are present; labels are case-insensitive and start their line', () => {
  assert.deepEqual([STATEMENT.present, STATEMENT.category, STATEMENT.missing], [true, 'Simulation', []]);
  assert.equal(STATEMENT.real_world_link, 'Models a tidal harbour from published tide tables.');
  const loose = extractQualificationStatement('CATEGORY: stem lab.\n  objective:  Measure g.\nLESSON: Free fall.\nreal world link: A drop tower.');
  assert.deepEqual([loose.present, loose.category, loose.objective], [true, 'STEM lab', 'Measure g.']);
  assert.equal(extractQualificationStatement('Our category: Simulation\nObjective: a\nLesson: b\nReal-world link: c').present, false, 'a label mid-line does not count');
});

test('statement: a missing line, an empty line, or a category not in the Code\'s list is missing', () => {
  const partial = extractQualificationStatement('Category: Simulation\nObjective: Read the depth.\nLesson: Tides.');
  assert.deepEqual([partial.present, partial.missing], [false, ['Real-world link']]);
  assert.deepEqual(extractQualificationStatement(`${STATEMENT_TEXT.replace('Lesson: How tidal range and basin shape set harbour depth.', 'Lesson:')}`).missing, ['Lesson']);
  const wrong = extractQualificationStatement(STATEMENT_TEXT.replace('Category: Simulation', 'Category: Obby'));
  assert.deepEqual([wrong.present, wrong.missing, wrong.category_not_in_list], [false, ['Category'], true]);
  assert.equal(extractQualificationStatement(null).present, false);
});

test('statement: labels late in a long description are found, by Jev and by the judge, though the description each reads is clipped', async () => {
  const description = `${'A long history of the harbour. '.repeat(200)}\n${STATEMENT_TEXT}`;
  assert.ok(description.length > 6000);
  assert.equal(extractQualificationStatement(description).present, true);
  assert.equal(buildJevState({ name: 'Harbour', description }, null).listing.statement.present, true);
  const sent = [];
  await claudeJudge({ sim: { name: 'Harbour', description }, dossier: null, jev: { answers: {} }, triage: {}, captures: [], policyText: 'P', policyHash: 'h', env: {}, deps: { claude: fakeClaude(() => { throw apiError(500); }, sent) } });
  const summary = JSON.parse(sent[0].body.messages[0].content.at(-1).text.split('\n')[2]);
  assert.ok(summary.listing.description.length <= 1501, 'the description the judge reads is clipped');
  assert.deepEqual([summary.statement.present, summary.statement.category], [true, 'Simulation']);
});

// ── deterministic citations and signals ─────────────────────────────────────

test('B6: a payment link, a crypto address or a paid unlock code in the Space is a system citation; a mention without a link is left to Jev', () => {
  const found = strings => deterministicSignals({}, { strings }).payment.map(f => f.kind);
  assert.deepEqual(found(['Support the maker at ko-fi.com/harbourmaker']), ['payment_link']);
  assert.deepEqual(found([`Tips: 0x${'ab12'.repeat(10)}`]), ['crypto_address']);
  assert.deepEqual(found(['bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq']), ['crypto_address']);
  assert.deepEqual(found(['1BoatSLRHtKNngkdXEeobR76b53LETtpyT']), ['crypto_address']);
  assert.deepEqual(found(['Buy the unlock code for $5 on my site']), ['paid_unlock_code']);
  assert.deepEqual(found(['Unlock code: enter the tide table page number', 'Patreon supporters are thanked in the credits', 'Supercalifragilisticexpialidocious']), []);
  assert.deepEqual(deterministicSignals({}, { strings: [], signals: { urls: ['https://www.patreon.com/harbour'] } }).payment.map(f => f.kind), ['payment_link']);

  const signals = { ...deterministicSignals({ description: STATEMENT_TEXT }, { strings: ['Donate: paypal.me/harbour'] }) };
  const t = decideTriage({ signals, jev: null });
  assert.deepEqual(t.articles.system.map(s => [s.article, s.key]), [['B6', 'payment_in_space']]);
  const d = decideFinal({ triage: t, judge: judge() });
  assert.deepEqual([d.outcome, d.changes], ['changes_requested', ['not_yet_B6']]);
  assert.match(d.suggested_edit, /paypal\.me\/harbour/, 'the author is told which link');
  assert.equal(decideFinal({ triage: t, judge: judge({ scores: { lesson: 1 } }) }).outcome, 'reject');
});

test('B1(c): the listing\'s own words for horror are an article signal, as whole words; the judge cites B1 or a person decides', () => {
  assert.deepEqual(deterministicSignals({ name: 'Haunted Mill' }, null).horror_terms, ['haunted']);
  assert.deepEqual(deterministicSignals({ description: 'One JUMP SCARE per room.' }, null).horror_terms, ['jump scare']);
  assert.deepEqual(deterministicSignals({ tags: ['horror-survival'], genre: 'Creepy' }, null).horror_terms.sort(), ['creepy', 'horror']);
  assert.deepEqual(deterministicSignals({ name: 'Scarycat Labs', description: 'Horrors of the deep, a jumpscares compilation' }, null).horror_terms, []);
  const t = decideTriage({ signals: deterministicSignals({ name: 'Haunted Mill', description: STATEMENT_TEXT }, null), jev: null });
  assert.deepEqual(t.articles.signals.map(s => [s.article, s.key]), [['B1', 'listing_horror_terms']]);
  assert.equal(t.outcome, 'judge', 'a signal does not end triage: the rules need the judge\'s scores');
  assert.deepEqual([decideFinal({ triage: t, judge: judge() }).outcome, last(decideFinal({ triage: t, judge: judge() })).code], ['hold', 'article_signal_not_cited']);
  assert.equal(decideFinal({ triage: t, judge: judge({ articles: [cite('B1')] }) }).outcome, 'reject');
});

test('triage: Jev at 0.85 on a Group B question is a system citation, at 0.50 an article signal, and neither ends triage', () => {
  const jev = (answers) => ({ answers: Object.fromEntries(Object.entries(answers).map(([k, p]) => [k, { type: 'noul', p }])) });
  const t = decideTriage({ signals: {}, jev: jev({ accumulation_loop: 0.85, meme_content: 0.5, copied_signal: 0.49, harm_scored: 0.84 }) });
  assert.deepEqual(t.articles.system.map(s => [s.article, s.key, s.p]), [['B5', 'accumulation_loop', 0.85]]);
  assert.deepEqual(t.articles.signals.map(s => [s.article, s.key]), [['B3', 'harm_scored'], ['B4', 'meme_content']].sort((a, b) => Object.keys(ARTICLE_SIGNALS).indexOf(a[1]) - Object.keys(ARTICLE_SIGNALS).indexOf(b[1])));
  assert.equal(t.outcome, 'judge');
  assert.deepEqual(decideTriage({ signals: {}, jev: jev({ lawful_decision_training: 0.95 }) }).articles, { system: [], signals: [{ article: 'B3', key: 'lawful_decision_training', p: 0.95 }], exempt: [] }, 'lawful decision training is a signal, never a citation');
  // v2.1: A10 is a Group A signal. The researcher and the judge run, and the rules hold it for a person (rule 1 or rule 6).
  const place = decideTriage({ signals: {}, jev: jev({ sensitive_real_place: 0.5 }) });
  assert.deepEqual([place.outcome, place.lane, place.reasons.at(-1).article], ['escalate', 'harm', 'A10']);
  assert.deepEqual(place.articles.signals.map(s => [s.article, s.key, s.group]), [['A10', 'sensitive_real_place', 'A']]);
  assert.equal(decideFinal({ triage: { ...place, statement: STATEMENT, questionnaire: ANSWERED }, judge: judge() }).outcome, 'hold', 'uncited, a person decides');
  assert.equal(decideFinal({ triage: { ...place, statement: STATEMENT, questionnaire: ANSWERED }, judge: judge({ articles: [cite('A10')] }) }).outcome, 'hold', 'cited, a person decides');
});

test('triage: a confident asset flip is cited under B7 and held for a person; empty and default-only are B8 rejects', () => {
  const band = level => ({ answers: { quality_band: { probabilities: { [level]: 0.9 }, confidence: 0.9 }, spatial_intent: { type: 'noul', p: 0.1 } } });
  const flip = decideTriage({ signals: {}, jev: band('unmodified_template_or_asset_flip') });
  assert.deepEqual([flip.outcome, flip.reasons.at(-1).article, flip.reasons.at(-1).action], ['hold', 'B7', 'hold']);
  assert.deepEqual(flip.articles.system.map(s => s.article), ['B7']);
  const empty = decideTriage({ signals: { empty: true }, jev: null });
  assert.deepEqual([empty.outcome, empty.reasons[0].article], ['reject', 'B8']);
  assert.equal(decideTriage({ signals: { default_only: true }, jev: null }).reasons[0].article, 'B8');
});

test('scripts not read in full: a script cut short, cut by the engine, or left out marks scripts_truncated; strings cut alone do not', () => {
  const script = (n, source = 'local x = 1') => ({ path: `Soul/s${n}.luau`, language: 'luau', lines: 1, source });
  assert.equal(buildJevState({}, { scripts: [script(1)] }).scripts_truncated, false);
  assert.equal(buildJevState({}, { scripts: [script(1, 'x'.repeat(50_000))] }, { maxChars: 20_000 }).scripts_truncated, true);
  assert.equal(buildJevState({}, { scripts: [{ ...script(1), truncated: true }] }).scripts_truncated, true, 'the engine cut it');
  assert.equal(buildJevState({}, { scripts: Array.from({ length: 40 }, (_, n) => script(n)) }).scripts_truncated, true, 'the engine sends at most 40 and names no others');
  const strings = buildJevState({}, { scripts: [script(1)], strings: Array.from({ length: 400 }, (_, i) => `glossary entry ${i} `.repeat(10)) }, { maxChars: 20_000 });
  assert.deepEqual([strings.truncated, strings.scripts_truncated], [true, false]);
  assert.equal(decide({ scripts_truncated: true }).outcome, 'hold', 'held instead of listed');
});

test('strings: those with article terms come first, so a long glossary cannot push "Rebirth" out of the judge\'s sample', async () => {
  const strings = [...Array.from({ length: 100 }, (_, i) => `Glossary ${i}: basin`), 'Rebirth', '+1 Coins/sec'];
  assert.deepEqual(prioritizeStrings(strings).slice(0, 2), ['Rebirth', '+1 Coins/sec']);
  const sent = [];
  await claudeJudge({ sim: { name: 'Harbour' }, dossier: { strings }, jev: { answers: {} }, triage: {}, captures: [], policyText: 'P', policyHash: 'h', env: {}, deps: { claude: fakeClaude(() => { throw apiError(500); }, sent) } });
  const summary = JSON.parse(sent[0].body.messages[0].content.at(-1).text.split('\n')[2]);
  assert.equal(summary.strings_sample.length, 60);
  assert.deepEqual(summary.strings_sample.slice(0, 2), ['Rebirth', '+1 Coins/sec']);
});

test('thresholds: the Code\'s thresholds are defaults, overridable like the rest, and fear sets no rating', () => {
  assert.deepEqual([DEFAULT_THRESHOLDS.article_system_cite, DEFAULT_THRESHOLDS.article_signal, DEFAULT_THRESHOLDS.article_judge_cite, DEFAULT_THRESHOLDS.game_mechanics, DEFAULT_THRESHOLDS.fear_signal_confidence], [0.85, 0.50, 0.75, 0.60, 0.55]);
  assert.equal('fear_rating_confidence' in DEFAULT_THRESHOLDS, false);
  const t = thresholdsFrom({ MODERATION_THRESHOLDS: '{"article_system_cite":0.95}' });
  assert.equal(t.article_system_cite, 0.95);
  const triaged = decideTriage({ signals: {}, jev: { answers: { accumulation_loop: { type: 'noul', p: 0.9 } } }, thresholds: t });
  assert.deepEqual([triaged.articles.system.length, triaged.articles.signals[0].article], [0, 'B5'], 'at 0.9 it is a signal under the raised threshold');
});

// ── the agent ───────────────────────────────────────────────────────────────

const SIM = 'a1b2c3d4-0000-4000-8000-0000000002a1';
function memEnv() {
  const kv = new Map();
  return {
    kv,
    SOCIAL: { get: async k => kv.get(k) ?? null, put: async (k, v) => { kv.set(k, String(v)); }, delete: async k => { kv.delete(k); }, list: async () => ({ keys: [], list_complete: true }) },
    SCENES: { get: async () => null, head: async () => null, put: async () => null, delete: async () => {}, list: async () => ({ objects: [] }) },
    USERS: { get: async () => null, put: async () => {}, delete: async () => {} },
    AUDIT_LOG: { get: async () => null, put: async () => {} },
  };
}
async function approveAs(actor, caseOver = {}, args = {}) {
  const e = memEnv();
  await e.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'user-1', status: 'held', history: [], triage: triage({ outcome: 'escalate', lane: 'harm' }), judge: judge(), ...caseOver }));
  const run = makeToolExecutor(e, { auditLog: async () => {}, json: v => v }, { actor, actorId: actor === 'admin' ? 'admin-1' : 'grok-agent' });
  const res = await run('moderation_approve', { sim_id: SIM, rating: 'all_ages', rationale: 'Every test is shown and the statement matches the harbour.', ...args });
  return { res, rec: JSON.parse(e.kv.get(`modcase:${SIM}`)) };
}

test('the agent cannot approve a game or a cited Space, cannot set featured, and approves what the rules would list', async () => {
  const game = await approveAs('agent', { triage: triage({ outcome: 'escalate', game_mechanics: 0.7 }) });
  assert.deepEqual([game.res.ok, game.rec.status], [false, 'held']);
  assert.match(game.res.error, /a game is listed by a person/);
  for (const [name, over] of [
    ['a system citation', { triage: triage({ outcome: 'escalate', ...withSystem(sys('B5', 'accumulation_loop')) }) }],
    ['a judge citation', { judge: judge({ articles: [cite('B1')] }) }],
    ['an article signal', { triage: triage({ outcome: 'escalate', ...withSignals({ article: 'B4', key: 'meme_content', p: 0.6 }) }) }],
    ['the statement missing', { triage: triage({ outcome: 'escalate', statement: NO_STATEMENT }), judge: judge({ statement_check: 'missing' }) }],
  ]) {
    const { res, rec } = await approveAs('agent', over);
    assert.equal(res.ok, false, name);
    assert.match(res.error, /would not list/, name);
    assert.equal(rec.status, 'held', name);
  }
  const featured = await approveAs('agent', {}, { featured: true });
  assert.deepEqual([featured.res.ok, featured.rec.status], [false, 'held']);
  assert.match(featured.res.error, /featured is decided by a person/);
  const ok = await approveAs('agent');
  assert.deepEqual([ok.res.ok, ok.rec.status, ok.rec.decision.featured], [true, 'approved', false]);
  const rated = await approveAs('agent', { decision: { rating: 'mature_17' } }, { rating: 'all_ages' });
  assert.deepEqual([rated.res.ok, rated.rec.decision.rating], [true, 'mature_17'], 'the agent cannot lower the decided rating');
  assert.equal(wouldList({ triage: triage(), judge: judge() }), true);
  assert.equal(wouldList({ triage: triage({ outcome: 'reject' }), judge: judge() }), false, 'a triage decision is not reopened');
});

test('a person sets featured, and never on an adult_18 listing', async () => {
  const set = await approveAs('admin', {}, { featured: true });
  assert.deepEqual([set.res.ok, set.rec.decision.featured], [true, true]);
  const adult = await approveAs('admin', {}, { featured: true, rating: 'adult_18' });
  assert.equal(adult.res.ok, false);
  assert.match(adult.res.error, /never featured/);
});

// ── what the author is told ─────────────────────────────────────────────────

test('author copy: one sentence per article and test citing the Code, the statement lines, the not-yet copy, and no em dashes', () => {
  for (const a of GROUP_B) assert.match(AUTHOR_COPY[`code_${a}`].why, new RegExp(`^Code ${a}: `), a);
  for (const [k, label] of [['build_quality', 'T1'], ['objective', 'T2'], ['lesson', 'T3'], ['real_world', 'T4']]) assert.match(AUTHOR_COPY[`test_${k}`].why, new RegExp(`^Code ${label}: `), k);
  for (const a of ['B5', 'B6', 'B9']) assert.match(AUTHOR_COPY[`not_yet_${a}`].why, new RegExp(`Code ${a}`), a);
  assert.ok(AUTHOR_COPY.statement_missing && AUTHOR_COPY.statement_contradicted);
  assert.equal([EM_DASH, EN_DASH].some(c => JSON.stringify(AUTHOR_COPY).includes(c)), false, 'no em or en dashes');
  for (const old of ['quality_not_yet', 'quality_no_value', 'quality_low_effort', 'quality_asset_flip']) assert.equal(old in AUTHOR_COPY, false, old);
});

test('a rejection names the article or test the author has to change, and no criterion number reaches anyone', () => {
  const b1 = decide(withSystem(sys('B1', 'pursuing_entity')));
  const view = authorView({ status: 'rejected', decision: { reasons: b1.reasons, lane: b1.lane } }, {});
  assert.deepEqual(view.reasons.map(r => r.category), ['code_B1']);
  const t4 = decide({}, { scores: { real_world: 1 } });
  assert.deepEqual(authorView({ status: 'rejected', decision: { reasons: t4.reasons, lane: t4.lane } }, {}).reasons.map(r => r.category), ['test_real_world']);
  for (const d of [b1, t4, decide({ statement: NO_STATEMENT }, { statement_check: 'missing' })]) for (const r of d.reasons) assert.equal('criterion' in r, false);
});

test('moderation_reject citing a Code article or a test shows that copy to the author', async () => {
  const e = memEnv();
  await e.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'user-1', status: 'held', history: [], decision: { reasons: [] } }));
  const run = makeToolExecutor(e, { auditLog: async () => {}, json: v => v }, { actor: 'admin', actorId: 'admin-1' });
  assert.equal((await run('moderation_reject', { sim_id: SIM, lane: 'quality', category: 'B5', rationale: 'A person read the shop script: Rebirth doubles coins per click.' })).ok, true);
  assert.deepEqual(authorView(JSON.parse(e.kv.get(`modcase:${SIM}`)), {}).reasons.map(r => r.category), ['code_B5']);
});

test('the Code and the playbook both name v2.3 and the same rules', () => {
  assert.match(GUARDIAN_POLICY_TEXT, /The first rule that applies decides/);
  assert.match(GUARDIAN_POLICY_TEXT, /^# Eustress Guardian Code v2\.3/);
  assert.match(MODERATION_PLAYBOOK_TEXT, /Guardian Code v2\.3/);
  assert.doesNotMatch(MODERATION_PLAYBOOK_TEXT, /decision C|criterion \d|rejected_low_effort/);
  assert.equal(MODERATION_PLAYBOOK_TEXT.includes(EM_DASH), false);
  assert.equal(Object.keys(buildJevQuestions()).filter(k => k in ARTICLE_SIGNALS).length, Object.keys(ARTICLE_SIGNALS).length);
});

// ── the pipeline ────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(name => ({ name })), list_complete: true }),
  };
}
function r2() {
  const m = new Map();
  let versions = 0;
  const obj = (key, rec) => ({
    key, size: rec.bytes.byteLength, etag: rec.etag, httpMetadata: rec.httpMetadata || {}, customMetadata: rec.customMetadata || {}, checksums: rec.checksums || {}, version: rec.version,
    text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0), body: rec.bytes,
  });
  return {
    m,
    put: async (key, body, opts = {}) => {
      if (opts.onlyIf) {
        const cur = m.get(key);
        if (opts.onlyIf.etagDoesNotMatch === '*' && cur) return null;
        if (opts.onlyIf.etagMatches !== undefined && (!cur || cur.etag !== opts.onlyIf.etagMatches)) return null;
      }
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body).buffer : (body instanceof ArrayBuffer ? body : body.buffer);
      const rec = { version: String(++versions), bytes, etag: createHash('md5').update(Buffer.from(bytes)).digest('hex'), httpMetadata: opts.httpMetadata, customMetadata: opts.customMetadata, checksums: opts.sha256 ? { sha256: Uint8Array.from(Buffer.from(opts.sha256, 'hex')).buffer } : undefined };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async key => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000, include = [] } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key, ...(include.includes('customMetadata') ? { customMetadata: m.get(key).customMetadata || {} } : {}) })) }),
  };
}
const environment = () => ({ SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}' });

// Every Jev answer clean, then the overrides (a number is a yes/no probability).
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
  for (const [k, v] of Object.entries(overrides)) answers[k] = typeof v === 'number' ? { type: 'noul', noul: v } : { ...answers[k], ...v };
  return { model: 'jev-test', answers };
}

// The Claude judge answers `judgeAnswer`. Any other model call (an xAI call with tools: the agent of v2.0, which v2.1 retired)
// is counted under `agent`, so a test can show none is made.
function pipelineDeps(judgeAnswer, jev = jevAnswers()) {
  const calls = { judge: 0, agent: 0 };
  return {
    calls,
    json: (v, status, headers) => new Response(JSON.stringify(v), { status, headers }), cors: {},
    auditLog: async () => {},
    claude: fakeClaude(() => { calls.judge += 1; return judgeAnswer; }),
    grokFetch: async () => { calls.agent += 1; return { ok: false, status: 500, text: async () => '' }; },
    grokModel: 'grok-test',
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: async () => new Response(JSON.stringify(jev), { status: 200 }),
  };
}
const judgeAnswer = (over = {}) => JSON.parse(answer({ confidence: 0.9, ...over }));

async function publish(env, { id = SIM, description = `A working harbour.\n${STATEMENT_TEXT}`, dossier = {} } = {}) {
  const sim = { id, name: 'Harbour', description, genre: 'Simulation', author_id: 'user-1', is_public: true, r2_key: `universes/${id}/universe.pak`, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()) };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(`PAK-${id}`).buffer, { sha256: createHash('sha256').update(`PAK-${id}`).digest('hex') });
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  const full = { dossier_version: 1, digest: { entity_count: 300, default_material_fraction: 0.2, default_name_fraction: 0.2, script_count: 1, text_string_count: 4 }, strings: ['Tide range', 'Depth at quay'], scripts: [{ path: 'Soul/tide.luau', language: 'luau', lines: 3, source: 'local depth = basin + tide' }], signals: { urls: [] }, ...dossier };
  await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(full));
  await env.SCENES.put(`universes/${id}/moderation/capture-0.png`, new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1]).buffer, { httpMetadata: { contentType: 'image/png' } });
  return sim;
}

test('pipeline control: a qualifying Space is approved, with featured left to a person', async () => {
  const env = environment();
  await publish(env);
  const rec = await runModerationCase(SIM, env, pipelineDeps(judgeAnswer({ featured_candidate: true, tests: tests({ build_quality: 3, lesson: 3, real_world: 3 }) })));
  assert.equal(rec.status, 'approved');
  assert.deepEqual([rec.decision.featured, rec.decision.featured_candidate, rec.policy_version, rec.review_schema], [false, true, '2.3', 2]);
  assert.equal(JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)).moderation.featured, false);
});

test('pipeline rule 1: a judge\'s A1 citation from the captures alone quarantines: storage locked, the account untouched, evidence kept', async () => {
  const env = environment();
  await publish(env);
  const d = pipelineDeps(judgeAnswer({ verdict: 'reject', articles: [cite('A1', 0.8)] }));
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'quarantined');
  assert.equal(rec.legal_hold.category, 'csam_or_minor_sexualization');
  assert.equal(env.USERS.m.has('publish-frozen:user-1'), false, 'the author is not frozen (McKale, 2026-10-15)');
  // The judge's 0.8 is below the high band (0.85): removed and sealed, never reported.
  assert.equal(rec.legal_report.written, false);
  assert.equal(rec.legal_report.reason, 'borderline_never_reported');
  assert.equal(env.SCENES.m.get(`locks/legal/${SIM}`)?.customMetadata?.kind, 'serve_block', 'storage is locked');
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  assert.deepEqual([isListable(sim), canServe(sim, 'user-1', false)], [false, false]);
  assert.equal(authorView(rec, sim).status, 'in_review', 'the author is told only that it is in review');
  assert.equal(d.calls.agent, 0);
});

test('pipeline rule 2: a B5 loop is not yet, applied directly: the author gets changes_requested and the agent is not asked', async () => {
  const env = environment();
  await publish(env, { dossier: { strings: ['Rebirth', '+1 Coins/sec'] } });
  const d = pipelineDeps(judgeAnswer({ suggested_edit_to_publish: 'Remove the Rebirth button and the per-second coins.' }), jevAnswers({ accumulation_loop: 0.92 }));
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'changes_requested');
  assert.deepEqual(rec.decision.changes, ['not_yet_B5']);
  assert.equal(d.calls.agent, 0, 'not left to the agent');
  const view = authorView(rec, JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)));
  assert.deepEqual([view.status, view.reasons[0].category, view.suggested_edit], ['changes_requested', 'not_yet_B5', 'Remove the Rebirth button and the per-second coins.']);
});

test('pipeline rule 6: a Space whose scripts Jev could not read in full is held instead of listed', async () => {
  const env = environment();
  await publish(env, { dossier: { scripts: [{ path: 'Soul/tide.luau', language: 'luau', lines: 900, source: 'local depth = basin + tide', truncated: true }] } });
  const rec = await runModerationCase(SIM, env, pipelineDeps(judgeAnswer()));
  assert.equal(rec.status, 'held');
  assert.equal(rec.jev.scripts_truncated, true);
  assert.ok(rec.decision.reasons.some(r => r.code === 'scripts_not_read_in_full'));
});

test('pipeline triage: a confident asset flip is held as B7 before the judge runs', async () => {
  const env = environment();
  await publish(env);
  const d = pipelineDeps(judgeAnswer(), jevAnswers({ quality_band: { probabilities: { unmodified_template_or_asset_flip: 0.9 }, confidence: 0.9 }, spatial_intent: 0.1 }));
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.reasons.at(-1).article, 'B7');
  assert.equal(d.calls.judge, 0);
});

test('pipeline: the listing\'s rating is the higher of Jev\'s and the judge\'s', async () => {
  const env = environment();
  await publish(env);
  const rec = await runModerationCase(SIM, env, pipelineDeps(judgeAnswer({ rating_floor: 'mature_17' })));
  assert.deepEqual([rec.status, rec.decision.rating, JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)).moderation.rating], ['approved', 'mature_17', 'mature_17']);
});

test('pipeline: a listing with no Qualification Statement is not yet, and its author is shown the four lines', async () => {
  const env = environment();
  await publish(env, { description: 'A working harbour with a tide model.' });
  const rec = await runModerationCase(SIM, env, pipelineDeps(judgeAnswer({ statement_check: 'missing' })));
  assert.equal(rec.status, 'changes_requested');
  assert.deepEqual(rec.decision.changes, ['statement_missing']);
  assert.equal(authorView(rec, {}).reasons[0].category, 'statement_missing');
});
