// The Eustress Guardian Code v2.1 (docs/architecture/AI_GUARDIAN_POLICY_v1.0.md), what it adds to v2.0, as the Worker applies
// it: Group A's A12, A13 and A14 and the 0.75 split of rule 1; the property-ownership attestation; B6 as the fair economy, one
// question per sub-item and B6(h) with a verified business; fiction, its label and T4 for Fiction; the Product category; the
// age bands with ages_10 and the rating questionnaire; the changes to rules 2, 7 and 8; review_schema; the Claude judge, the
// shadow judge and the Grok researcher; the Review Workspace; and the takedown hold a person cannot clear here.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  decideFinal, decideTriage, deterministicSignals, parseJudgeVerdict, buildJevQuestions, extractQualificationStatement,
  makeToolExecutor, runModerationCase, loadCase, authorView, claudeJudge, judgeRequest, judgeSystemPrompt, handleModerationRoute,
  researchFacts, researchTrigger, researcherRequest, runResearcher, validateFindings, fictionLabelled, shadowAgreementReport,
  DEFAULT_THRESHOLDS, CODE_TESTS, GROUP_A, ARTICLE_SIGNALS, ARTICLES_QUARANTINE, ARTICLES_REJECT_A, REJECT_ARTICLES, RATINGS,
  JUDGE_OUTPUT_SCHEMA, JUDGE_MODEL, DEFAULT_JUDGE_EFFORT, POLICY_HASH_ANCHORED, POLICY_VERSION, REVIEW_SCHEMA, HARD_CATEGORIES,
  AUTHOR_COPY, JUDGE_IMAGE_MAX_B64,
} from '../src/moderation.mjs';
import { BANDS, DESCRIPTORS, QUESTIONS, checkAnswers, ratingFromAnswers, questionnaireRecord, questionnaireOf } from '../src/rating_questionnaire.mjs';
import { checkAttestations, checkBusiness, readPropertyAttestations, readVerifiedBusiness, accountFacts, propertiesNamedIn } from '../src/account_attestations.mjs';
import { ratingCap, ratingOf, mayView } from '../src/adult_content.mjs';
import { renderModerationEmail } from '../src/moderation_email.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { fakeClaude, claudeMessage, apiError, questionnaireAnswers } from './helpers/claude_fake.mjs';

// ── fixtures ────────────────────────────────────────────────────────────────

const STATEMENT_TEXT = [
  'Category: Simulation',
  'Objective: Set the tide range and read the depth at the quay.',
  'Lesson: How tidal range and basin shape set harbour depth.',
  'Real-world link: Models a tidal harbour from published tide tables.',
].join('\n');
const STATEMENT = extractQualificationStatement(STATEMENT_TEXT);
const statementAs = category => extractQualificationStatement(STATEMENT_TEXT.replace('Category: Simulation', `Category: ${category}`));
const ANSWERED = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers()) });
const UNANSWERED = questionnaireOf({});
const HOME = { address: '1428 Elm Street, Springwood', kind: 'own', verified_at: '2026-10-01T00:00:00.000Z', source: 'county deed record 2026-118812' };

const tests = (scores = {}) => Object.fromEntries(CODE_TESTS.map(k => [k, { score: k in scores ? scores[k] : 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
const judge = (over = {}) => {
  const { scores, ...rest } = over;
  return {
    malformed: false, verdict: 'publish', articles: [], tests: tests(scores), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [],
    is_game: false, is_fiction: false, featured_candidate: false, spatial_evidence: ['four quay cranes along the east basin'], rationale: 'Every test is shown.',
    confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, ...rest,
  };
};
const cite = (id, confidence = 0.9) => ({ id, confidence, evidence: `Code ${id}: the vault door and camera layout in capture-2 and Soul/heist.luau` });
const triage = (over = {}) => ({
  outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'simulation', rating: 'all_ages', articles: { system: [], signals: [], exempt: [] },
  statement: STATEMENT, scripts_truncated: false, game_mechanics: 0.02, lawful_decision_training: 0.02,
  questionnaire: ANSWERED, fiction_p: 0.02, fiction_labelled: false, account: { attested_properties: [], verified_business: false, payment_connection: false }, ...over,
});
const decide = (t = {}, j = {}) => decideFinal({ triage: triage(t), judge: judge(j) });
const last = d => d.reasons.at(-1);
const noulJev = answers => ({ answers: Object.fromEntries(Object.entries(answers).map(([k, p]) => [k, { type: 'noul', p }])) });

// ── policy ──────────────────────────────────────────────────────────────────

test('the Code (v2.3, which carries v2.1) is anchored by its hash, and the Worker cites it', () => {
  assert.equal(POLICY_VERSION, '2.3');
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  assert.equal(POLICY_HASH_ANCHORED, 'sha256:99ced3f2e27e715a52483581375cb9eda9401944ca8f4412483da2f27e5f39b0');
  for (const id of ['A12', 'A13', 'A14']) assert.match(GUARDIAN_POLICY_TEXT, new RegExp(`\\*\\*${id}\\. `), id);
  assert.equal(REVIEW_SCHEMA, 2);
});

// ── rule 1: Group A ─────────────────────────────────────────────────────────

test('rule 1: A1, A2, A3, A5, A6 and A12 quarantine at any confidence; A12 through its own hard category', () => {
  assert.deepEqual(ARTICLES_QUARANTINE, ['A1', 'A2', 'A3', 'A5', 'A6', 'A12']);
  for (const id of ARTICLES_QUARANTINE) {
    const d = decide({}, { verdict: 'reject', articles: [cite(id, 0.3)] });
    assert.deepEqual([d.outcome, d.lane, last(d).article, last(d).rule], ['quarantine', 'legal', id, 1], id);
  }
  assert.equal(last(decide({}, { verdict: 'reject', articles: [cite('A12', 0.3)] })).code, 'war_or_military_targeting');
  assert.ok(HARD_CATEGORIES.includes('war_or_military_targeting'), 'moderation_quarantine accepts it');
});

test('rule 1: A4, A7, A8, A9, A13 and A14 reject at 0.75 or above, and are held below it', () => {
  assert.deepEqual(ARTICLES_REJECT_A, ['A4', 'A7', 'A8', 'A9', 'A13', 'A14']);
  for (const id of ARTICLES_REJECT_A) {
    const at = decide({}, { verdict: 'reject', articles: [cite(id, 0.75)] });
    assert.deepEqual([at.outcome, at.lane, last(at).article], ['reject', 'harm', id], `${id} at 0.75`);
    const below = decide({}, { verdict: 'reject', articles: [cite(id, 0.74)] });
    assert.deepEqual([below.outcome, below.lane, last(below).article, last(below).rule, last(below).below], ['hold', 'harm', id, 1, 0.75], `${id} at 0.74`);
  }
  assert.equal(DEFAULT_THRESHOLDS.group_a_reject, 0.75);
});

test('rule 1: a Group A citation against attested property is held; the attestation exempts A10, A13 and A14 only', () => {
  const attested = { account: { attested_properties: [HOME], verified_business: false, payment_connection: false } };
  for (const id of ['A13', 'A14']) {
    const d = decide(attested, { verdict: 'reject', articles: [cite(id, 0.95)] });
    assert.deepEqual([d.outcome, last(d).article, last(d).attested_property], ['hold', id, true], id);
    assert.equal(decide({}, { verdict: 'reject', articles: [cite(id, 0.95)] }).outcome, 'reject', `${id} without an attestation`);
  }
  assert.deepEqual([decide(attested, { articles: [cite('A10')] }).outcome, last(decide(attested, { articles: [cite('A10')] })).attested_property], ['hold', true]);
  assert.equal(decide(attested, { verdict: 'reject', articles: [cite('A4', 0.95)] }).outcome, 'reject', 'A4 is never exempt');
  assert.equal(decide(attested, { verdict: 'reject', articles: [cite('A12', 0.95)] }).outcome, 'quarantine', 'A12 is never exempt');
  assert.equal(decide(attested, { verdict: 'reject', articles: [cite('A13', 0.95), cite('A7', 0.9)] }).outcome, 'reject', 'an exempt citation does not shield another article');
});

test('Part VII: a reject must cite A1 to A9 or A12 to A14; A10 or A11 alone is malformed', () => {
  assert.deepEqual(REJECT_ARTICLES, ['A1', 'A2', 'A3', 'A4', 'A5', 'A6', 'A7', 'A8', 'A9', 'A12', 'A13', 'A14']);
  const answer = over => JSON.stringify({ ...judge(), malformed: undefined, policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED, ...over });
  assert.equal(parseJudgeVerdict(answer({ verdict: 'reject', articles: [cite('A13')] })).malformed, false);
  assert.equal(parseJudgeVerdict(answer({ verdict: 'reject', articles: [cite('A10')] })).error, 'judge_reject_without_group_a');
  assert.equal(parseJudgeVerdict(answer({ is_fiction: 'yes' })).error, 'judge_bad_enum');
  assert.equal(parseJudgeVerdict(answer({ descriptors: ['gore'] })).error, 'judge_bad_descriptors');
  assert.equal(parseJudgeVerdict(answer({ rating_floor: 'ages_10' })).rating_floor, 'ages_10');
  assert.deepEqual(parseJudgeVerdict(answer({ descriptors: ['blood', 'fantasy violence'] })).descriptors, ['fantasy violence', 'blood'], 'in the Code\'s order');
});

// ── the Jev battery ─────────────────────────────────────────────────────────

test('Jev asks A12, A13 and A14 in the Code\'s bright-line words, one question per B6 sub-item, and whether the Space is fiction', () => {
  const q = buildJevQuestions();
  assert.match(q.war_or_military_targeting.instructions, /^Code A12\. .*active military site.*strike planning.*Battles that ended long ago, museums and memorials, and civil engineering are not this/);
  assert.match(q.crime_rehearsal.instructions, /^Code A13\. .*cameras, alarms, vaults, guard posts, entry codes, schedules.*A heist in an invented place is fiction/);
  assert.match(q.stalking_private_person.instructions, /^Code A14\. .*private person.*Public places modelled as places are not this/);
  const b6 = Object.entries(ARTICLE_SIGNALS).filter(([, a]) => a === 'B6').map(([k]) => k);
  assert.deepEqual(b6, ['b6a_paid_progress', 'b6b_paid_chance', 'b6c_paid_skip', 'b6d_unasked_prompt', 'b6e_sales_pressure', 'b6f_hidden_prices', 'b6g_social_reach_reward', 'b6h_offsite_payment']);
  for (const [k, letter] of b6.map((k, i) => [k, 'abcdefgh'[i]])) assert.match(q[k].instructions, new RegExp(`^Code B6\\(${letter}\\)\\. `), k);
  assert.match(q.b6a_paid_progress.instructions, /single-player/);
  assert.ok(q.is_fiction && q.is_fiction.type === 'noul');
  assert.deepEqual(Object.keys(q.content_rating.criteria), RATINGS, 'Jev rates in the Code\'s bands, ages_10 included');
});

test('triage: Jev\'s A12 at 0.85 quarantines like A1 to A6; A13 and A14 signals escalate to the researcher and the judge; uncited, a person decides', () => {
  assert.equal(decideTriage({ signals: {}, jev: noulJev({ war_or_military_targeting: 0.9 }) }).outcome, 'quarantine');
  assert.deepEqual([decideTriage({ signals: {}, jev: noulJev({ war_or_military_targeting: 0.4 }) }).outcome, decideTriage({ signals: {}, jev: noulJev({ war_or_military_targeting: 0.4 }) }).lane], ['hold', 'legal']);
  for (const [key, article] of [['crime_rehearsal', 'A13'], ['stalking_private_person', 'A14']]) {
    const t = decideTriage({ signals: {}, jev: noulJev({ [key]: 0.6 }) });
    assert.deepEqual([t.outcome, t.lane, t.articles.signals[0].article, t.articles.signals[0].group], ['escalate', 'harm', article, 'A'], key);
    const d = decideFinal({ triage: { ...t, statement: STATEMENT, questionnaire: ANSWERED }, judge: judge() });
    assert.deepEqual([d.outcome, last(d).code, last(d).article, last(d).rule], ['hold', 'article_signal_not_cited', article, 6], key);
  }
});

// ── B6, the fair economy ────────────────────────────────────────────────────

test('B6: each sub-item at the system threshold is a B6 citation, not yet when every test is 2 or above, held when a test is null', () => {
  for (const key of Object.keys(ARTICLE_SIGNALS).filter(k => ARTICLE_SIGNALS[k] === 'B6')) {
    const t = decideTriage({ signals: {}, jev: noulJev({ [key]: 0.9 }) });
    assert.deepEqual(t.articles.system.map(s => [s.article, s.key]), [['B6', key]], key);
    const d = decideFinal({ triage: { ...t, statement: STATEMENT, questionnaire: ANSWERED }, judge: judge() });
    assert.deepEqual([d.outcome, d.changes], ['changes_requested', ['not_yet_B6']], key);
    const unscored = decideFinal({ triage: { ...t, statement: STATEMENT, questionnaire: ANSWERED }, judge: judge({ scores: { lesson: null } }) });
    assert.deepEqual([unscored.outcome, last(unscored).rule, last(unscored).tests_not_scored], ['hold', 2, ['lesson']], `${key} with a null test`);
  }
});

test('rule 2: B5, B6 or a B9 wording fault with a test null is held for a person (v2.0 rejected it)', () => {
  for (const id of ['B5', 'B6', 'B9']) {
    const d = decide({}, { articles: [cite(id, 0.8)], scores: { objective: null } });
    assert.deepEqual([d.outcome, last(d).article, last(d).rule], ['hold', id, 2], id);
  }
  assert.equal(decide({}, { articles: [cite('B6', 0.8)], scores: { objective: 1 } }).outcome, 'reject', 'a test below 2 and none null still rejects');
});

test('B6(h): payment off Eustress is allowed through a verified business connection, never in an Adult listing; raw links never', () => {
  const business = { attested_properties: [], verified_business: true, business_name: 'Harbour Works Ltd', payment_connection: true };
  const jev = noulJev({ b6h_offsite_payment: 0.9 });
  // A verified business connection, not Adult: the answer is recorded and not cited.
  const allowed = decideTriage({ signals: { account: business, statement: STATEMENT }, jev });
  assert.deepEqual([allowed.articles.system.length, allowed.articles.exempt.map(e => e.key)], [0, ['b6h_offsite_payment']]);
  assert.equal(decideFinal({ triage: { ...allowed, questionnaire: ANSWERED }, judge: judge() }).outcome, 'approve');
  // The same account, an Adult listing (by its statement): B6 whatever the account.
  const adult = decideTriage({ signals: { account: business, statement: statementAs('Adult') }, jev });
  assert.deepEqual(adult.articles.system.map(s => [s.article, s.key]), [['B6', 'b6h_offsite_payment']]);
  // Adult by the questionnaire, too.
  const explicit = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers({ sexual: 'explicit' })) });
  assert.equal(decideTriage({ signals: { account: business, statement: STATEMENT, questionnaire: explicit }, jev }).articles.system[0]?.key, 'b6h_offsite_payment');
  // Adult only once the judge rates it adult_18: the exemption is withdrawn in decideFinal.
  const late = decideFinal({ triage: { ...allowed, questionnaire: ANSWERED }, judge: judge({ rating_floor: 'adult_18' }) });
  assert.deepEqual([late.outcome, late.changes, last(late).system[0].adult_listing], ['changes_requested', ['not_yet_B6'], true]);
  // No verified business: cited as before.
  assert.equal(decideTriage({ signals: { statement: STATEMENT }, jev }).articles.system[0].key, 'b6h_offsite_payment');
  // A raw payment link is a system citation whatever the account.
  const link = deterministicSignals({ description: STATEMENT_TEXT }, { strings: ['Donate: paypal.me/harbour'] }, { account: business });
  assert.deepEqual(decideTriage({ signals: link, jev: null }).articles.system.map(s => s.key), ['payment_in_space']);
});

// ── fiction ─────────────────────────────────────────────────────────────────

test('the fiction label: the tag fiction and at least one genre tag of its own', () => {
  assert.equal(fictionLabelled(['fiction', 'science-fiction']), true);
  assert.equal(fictionLabelled(['fiction', 'fantasy', 'coop']), true);
  assert.equal(fictionLabelled(['fiction', 'climate-fiction']), true, 'a -fiction tag is a genre');
  assert.equal(fictionLabelled(['fiction']), false, 'no genre tag');
  assert.equal(fictionLabelled(['fiction', 'coop']), false, 'coop is not a genre');
  assert.equal(fictionLabelled(['science-fiction']), false, 'no fiction tag');
  assert.equal(deterministicSignals({ tags: ['fiction', 'fantasy'] }, null).fiction_labelled, true);
});

test('fiction: labelled fiction is listed; unlabelled fiction is not yet (rule 7), by the statement, the judge or Jev', () => {
  const labelled = decide({ statement: statementAs('Fiction'), fiction_labelled: true }, { is_fiction: true });
  assert.equal(labelled.outcome, 'approve');
  for (const [name, t, j] of [
    ['the category', { statement: statementAs('Fiction') }, {}],
    ['the judge', {}, { is_fiction: true }],
    ['Jev', { fiction_p: 0.6 }, {}],
  ]) {
    const d = decide(t, j);
    assert.deepEqual([d.outcome, d.changes, last(d).code, last(d).rule], ['changes_requested', ['fiction_label_missing'], 'fiction_label_missing', 7], name);
  }
  assert.equal(decide({ fiction_p: 0.59 }).outcome, 'approve', 'below Jev\'s fiction threshold it is not fiction');
  const view = authorView({ status: 'changes_requested', decision: { changes: ['fiction_label_missing'], lane: 'quality' } }, {});
  assert.match(view.reasons[0].what_to_change, /"fiction"/);
});

test('T4 for Fiction: fiction is scored and named under it; while the label is missing a T4 of 0 asks for the label, not a rejection', () => {
  const fiction = { statement: statementAs('Fiction'), fiction_labelled: true };
  const weak = decide(fiction, { is_fiction: true, scores: { real_world: 1 } });
  assert.deepEqual([weak.outcome, last(weak).code, last(weak).label, last(weak).rule], ['reject', 'test_real_world_fiction', 'T4 for Fiction', 4]);
  assert.ok(AUTHOR_COPY.test_real_world_fiction);
  assert.equal(decide(fiction, { is_fiction: true, scores: { real_world: 0 } }).outcome, 'reject', 'labelled, an idea absent is rejected');
  const unlabelled = decide({ statement: statementAs('Fiction') }, { is_fiction: true, scores: { real_world: 0 } });
  assert.deepEqual([unlabelled.outcome, unlabelled.changes], ['changes_requested', ['fiction_label_missing']]);
  assert.equal(decide({ statement: statementAs('Fiction') }, { is_fiction: true, scores: { lesson: 0 } }).outcome, 'reject', 'every other test still applies');
  assert.equal(last(decide({}, { scores: { real_world: 1 } })).code, 'test_real_world', 'not fiction: plain T4');
});

// ── Product ─────────────────────────────────────────────────────────────────

test('Product from an account without a verified business is B9, a wording fault: not yet, naming the category', () => {
  const unverified = deterministicSignals({ description: STATEMENT_TEXT.replace('Category: Simulation', 'Category: Product') }, null, {});
  const t = decideTriage({ signals: unverified, jev: null });
  assert.deepEqual(t.articles.system.map(s => [s.article, s.key]), [['B9', 'product_without_verified_business']]);
  const d = decideFinal({ triage: { ...t, questionnaire: ANSWERED }, judge: judge() });
  assert.deepEqual([d.outcome, d.changes], ['changes_requested', ['not_yet_B9']]);
  assert.match(d.suggested_edit, /Product is for a business Eustress has verified/);
  const verified = deterministicSignals({ description: STATEMENT_TEXT.replace('Category: Simulation', 'Category: Product') }, null, { account: { attested_properties: [], verified_business: true, payment_connection: false } });
  assert.deepEqual(decideTriage({ signals: verified, jev: null }).articles.system, []);
});

// ── ratings and the questionnaire ───────────────────────────────────────────

test('the bands: ages_10 sits between all_ages and teen_13, everywhere the Worker orders ratings', () => {
  assert.deepEqual(RATINGS, ['all_ages', 'ages_10', 'teen_13', 'mature_17', 'adult_18', 'adult_21']);
  assert.deepEqual(BANDS, RATINGS);
  assert.equal(ratingOf({ moderation: { rating: 'ages_10' } }), 'ages_10', 'the gate knows it (an unknown rating reads as adult_21)');
  assert.equal(ratingCap('ages_10', null), 'ages_10');
  assert.equal(ratingCap('ages_10', { adult: true }), 'ages_10');
  assert.equal(mayView({ moderation: { rating: 'ages_10' } }, null), true);
  assert.deepEqual(JUDGE_OUTPUT_SCHEMA.properties.rating_floor.enum, RATINGS);
});

test('the questionnaire maps each answer to a band and descriptors; the band is the highest answer', () => {
  assert.deepEqual(ratingFromAnswers(questionnaireAnswers()), { band: 'all_ages', descriptors: [], articles: [] });
  assert.deepEqual(ratingFromAnswers(questionnaireAnswers({ language: 'mild' })), { band: 'ages_10', descriptors: ['language'], articles: [] });
  assert.deepEqual(ratingFromAnswers(questionnaireAnswers({ violence: 'fantasy', gambling: 'simulated', interaction: 'yes' })), { band: 'teen_13', descriptors: ['fantasy violence', 'simulated gambling (no money)', 'players interact'], articles: [] });
  assert.equal(ratingFromAnswers(questionnaireAnswers({ violence: 'realistic', blood: 'mild' })).band, 'mature_17');
  assert.equal(ratingFromAnswers(questionnaireAnswers({ nudity: 'clinical' })).band, 'mature_17', 'clinical depiction is mature_17 (Part V)');
  assert.deepEqual(ratingFromAnswers(questionnaireAnswers({ sexual: 'explicit', purchases: 'yes' })), { band: 'adult_18', descriptors: ['sexual content', 'in-Space purchases'], articles: [] });
  assert.equal(ratingFromAnswers({ violence: 'none' }), null, 'a partial set gives no band');
  assert.match(checkAnswers({ ...questionnaireAnswers(), violence: 'lots' }).error, /not an answer/);
  assert.match(checkAnswers({ ...questionnaireAnswers(), extra: 'none' }).error, /no question/);
  for (const q of QUESTIONS) for (const o of q.options) {
    assert.ok(BANDS.includes(o.band), `${q.id}.${o.value} band`);
    for (const d of o.descriptors) assert.ok(DESCRIPTORS.includes(d), `${q.id}.${o.value} ${d}`);
  }
  assert.doesNotMatch(JSON.stringify(QUESTIONS), /ESRB|IARC|PEGI|Teen\b|Mature\b|Everyone\b/, 'Eustress\'s own words');
});

test('the rating is the highest of the questionnaire, Jev and the judge\'s floor, with both sets of descriptors', () => {
  const asked = band => questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers(band)) });
  const d = decideFinal({ triage: triage({ rating: 'all_ages', questionnaire: asked({ violence: 'realistic' }) }), judge: judge({ rating_floor: 'teen_13', descriptors: ['blood'] }) });
  assert.equal(d.rating, 'mature_17', 'the questionnaire');
  assert.deepEqual(d.descriptors, ['realistic violence', 'blood']);
  assert.equal(decideFinal({ triage: triage({ rating: 'teen_13', questionnaire: asked({ language: 'mild' }) }), judge: judge({ rating_floor: 'all_ages' }) }).rating, 'teen_13', 'Jev');
  assert.equal(decideFinal({ triage: triage({ rating: 'all_ages', questionnaire: asked({ language: 'mild' }) }), judge: judge({ rating_floor: 'mature_17' }) }).rating, 'mature_17', 'the judge');
  assert.equal(decideFinal({ triage: triage({ rating: 'all_ages', questionnaire: asked({ language: 'mild' }) }), judge: judge({ rating_floor: 'all_ages' }) }).rating, 'ages_10', 'ages_10 from the questionnaire alone');
});

test('rule 7: an unanswered questionnaire is not yet; rule 8 lists only when it is answered', () => {
  const d = decide({ questionnaire: UNANSWERED });
  assert.deepEqual([d.outcome, d.changes, last(d).code, last(d).rule], ['changes_requested', ['questionnaire_unanswered'], 'questionnaire_unanswered', 7]);
  assert.deepEqual(decide({ questionnaire: UNANSWERED, statement: extractQualificationStatement('A harbour.') }, { statement_check: 'missing' }).changes, ['statement_missing', 'questionnaire_unanswered'], 'each change named');
  assert.equal(decide({ questionnaire: UNANSWERED }, { scores: { lesson: null } }).outcome, 'hold', 'rule 6 first');
  assert.equal(decide({ questionnaire: ANSWERED }).outcome, 'approve');
  assert.equal(decide({ questionnaire: undefined }).outcome, 'changes_requested', 'a triage without one is unanswered');
  assert.match(authorView({ status: 'changes_requested', decision: { changes: d.changes, lane: 'quality' } }, {}).reasons[0].what_to_change, /rating questionnaire/);
});

test('an answer the evidence disproves is a B9 wording fault: not yet, the change is the answer', () => {
  const d = decide({}, { articles: [{ id: 'B9', confidence: 0.85, evidence: 'The questionnaire answers violence none; capture-3 shows a sword fight scored by Soul/duel.luau' }], suggested_edit_to_publish: 'Answer the violence question with fantasy fighting.' });
  assert.deepEqual([d.outcome, d.changes, d.suggested_edit], ['changes_requested', ['not_yet_B9'], 'Answer the violence question with fantasy fighting.']);
});

// ── account facts ───────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  const meta = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v, o = {}) => { m.set(k, typeof v === 'string' ? v : String(v)); if (o.metadata !== undefined) meta.set(k, o.metadata); },
    delete: async k => { m.delete(k); meta.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).sort().slice(0, limit).map(name => ({ name, ...(meta.has(name) ? { metadata: meta.get(name) } : {}) })), list_complete: true }),
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
    list: async ({ prefix = '', limit = 1000, include = [] } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key, ...(include.includes('customMetadata') ? { customMetadata: m.get(key).customMetadata || {} } : {}) })) }),
  };
}
const environment = (extra = {}) => ({ SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', ...extra });
const json = (v, status, headers) => new Response(JSON.stringify(v), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

test('attestations and verified businesses: read from USERS, checked, and set only by an administrator', async () => {
  assert.deepEqual(checkAttestations([HOME]).properties, [HOME]);
  assert.match(checkAttestations([{ kind: 'own', verified_at: '2026-10-01', source: 'deed' }]).error, /address or a parcel_id/);
  assert.match(checkAttestations([{ ...HOME, kind: 'rent' }]).error, /own, lease, protect/);
  assert.match(checkBusiness({ name: 'X', verified_at: 'no' }).error, /verified_at/);
  const env = environment();
  assert.deepEqual(await readPropertyAttestations(env, 'user-1'), []);
  assert.equal(await readVerifiedBusiness(env, 'user-1'), null);
  const route = (method, path, token, body) => handleModerationRoute(new Request(`https://api.eustress.dev${path}`, { method, headers: token ? { Authorization: `Bearer ${token}` } : {}, body: body ? JSON.stringify(body) : undefined }), new URL(`https://api.eustress.dev${path}`), env, null, {
    json, cors: {}, auditLog: async () => {}, verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async r => ((r.headers.get('Authorization') || '').replace('Bearer ', '') === 'admin-1' ? 'admin-1' : null),
  });
  assert.equal((await route('PUT', '/api/admin/moderation/property-attestation/user-1', 'user-1', { properties: [HOME] })).status, 403, 'the author cannot attest their own property');
  assert.equal((await route('PUT', '/api/admin/moderation/property-attestation/user-1', 'admin-1', { properties: [HOME] })).status, 200);
  assert.equal((await route('PUT', '/api/admin/moderation/verified-business/user-1', 'admin-1', { name: 'Harbour Works Ltd', verified_at: '2026-10-02', source: 'state registry', payment_connection: 'stripe:acct_harbour' })).status, 200);
  assert.equal((await route('PUT', '/api/admin/moderation/verified-business/user-1', 'admin-1', { name: '' })).status, 400);
  assert.deepEqual(await accountFacts(env, 'user-1'), { attested_properties: [HOME], verified_business: true, business_name: 'Harbour Works Ltd', payment_connection: true });
  assert.deepEqual((await (await route('GET', '/api/admin/moderation/property-attestation/user-1', 'admin-1')).json()).properties, [HOME]);
  assert.equal(propertiesNamedIn([HOME], 'a twin of 1428 Elm Street, Springwood with its cameras').length, 1);
  assert.equal(propertiesNamedIn([HOME], 'a twin of 14 Elm Street').length, 0);
  assert.equal((await route('DELETE', '/api/admin/moderation/property-attestation/user-1', 'admin-1')).status, 200);
  assert.deepEqual(await readPropertyAttestations(env, 'user-1'), []);
});

test('the judge\'s case summary shows the author\'s attestations, the business, the questionnaire, the fiction check and the research', async () => {
  const sent = [];
  const t = triage({ account: { attested_properties: [HOME], verified_business: true, payment_connection: true }, fiction_labelled: true });
  await claudeJudge({ sim: { name: 'Home Defence Twin' }, dossier: null, jev: { answers: {} }, triage: t, captures: [], policyText: 'P', policyHash: 'h', research: { ran: true, trigger: 'group_a_signal', findings: [{ fact: '1428 Elm Street', finding: 'A private residence.', source_url: 'https://records.example.gov/1428' }] }, env: {}, deps: { claude: fakeClaude(JSON.stringify(judge()), sent) } });
  const summary = JSON.parse(sent[0].body.messages[0].content.at(-1).text.split('\n')[2]);
  assert.deepEqual(summary.account.attested_properties, [{ address: HOME.address, parcel_id: null, kind: 'own', verified_at: HOME.verified_at }]);
  assert.deepEqual([summary.account.verified_business, summary.account.payment_connection, summary.fiction_labelled], [true, true, true]);
  assert.equal(summary.rating_questionnaire.answered, true);
  assert.deepEqual(summary.research.findings.map(f => f.source_url), ['https://records.example.gov/1428']);
  assert.equal('source' in summary.account.attested_properties[0], false, 'the proof\'s source stays with the system');
});

// ── the Claude judge ────────────────────────────────────────────────────────

// The subset of JSON Schema structured outputs supports (tool-use-concepts: no numeric or string constraints, every object closed).
function validate(schema, v, at = '$') {
  if (schema.anyOf) return schema.anyOf.some(s => !validate(s, v, at).length) ? [] : [`${at}: no branch of anyOf`];
  const t = schema.type;
  const ok = t === 'null' ? v === null : t === 'integer' ? Number.isInteger(v) : t === 'number' ? typeof v === 'number' : t === 'array' ? Array.isArray(v) : t === 'object' ? !!v && typeof v === 'object' && !Array.isArray(v) : typeof v === t;
  if (!ok) return [`${at}: not ${t}`];
  const errors = [];
  if (schema.enum && !schema.enum.includes(v)) errors.push(`${at}: ${JSON.stringify(v)} not in enum`);
  if (t === 'object') {
    for (const k of schema.required || []) if (!(k in v)) errors.push(`${at}.${k}: missing`);
    for (const [k, x] of Object.entries(v)) {
      if (schema.properties?.[k]) errors.push(...validate(schema.properties[k], x, `${at}.${k}`));
      else if (schema.additionalProperties === false) errors.push(`${at}.${k}: not in the schema`);
    }
  }
  if (t === 'array' && schema.items) v.forEach((x, i) => errors.push(...validate(schema.items, x, `${at}[${i}]`)));
  return errors;
}
function schemaKeywords(schema, out = new Set()) {
  if (!schema || typeof schema !== 'object') return out;
  for (const [k, v] of Object.entries(schema)) {
    out.add(k);
    if (k === 'properties') for (const s of Object.values(v)) schemaKeywords(s, out);
    else if (k === 'items') schemaKeywords(v, out);
    else if (k === 'anyOf') for (const s of v) schemaKeywords(s, out);
  }
  return out;
}
function closedEverywhere(schema) {
  if (schema.anyOf) return schema.anyOf.every(closedEverywhere);
  if (schema.type === 'object') return schema.additionalProperties === false && Object.keys(schema.properties).every(k => schema.required.includes(k)) && Object.values(schema.properties).every(closedEverywhere);
  if (schema.type === 'array') return closedEverywhere(schema.items);
  return true;
}
const PART_VII = { ...judge(), malformed: undefined, policy_version: POLICY_VERSION, policy_hash: POLICY_HASH_ANCHORED, articles: [cite('B6', 0.8)], descriptors: ['in-Space purchases'], tests: tests({ lesson: null }), suggested_tags: [] };
delete PART_VII.malformed;

test('the judge\'s answer is schema-valid Part VII JSON: the schema structured outputs enforce, and the parser agree', () => {
  assert.deepEqual(validate(JUDGE_OUTPUT_SCHEMA, PART_VII), []);
  assert.equal(parseJudgeVerdict(JSON.stringify(PART_VII)).malformed, false);
  assert.ok(validate(JUDGE_OUTPUT_SCHEMA, { ...PART_VII, quality: 'listed' }).length, 'an extra field is outside the schema');
  assert.ok(validate(JUDGE_OUTPUT_SCHEMA, { ...PART_VII, tests: tests({ lesson: 4 }) }).length, 'a score of 4 is outside it');
  const unsupported = [...schemaKeywords(JUDGE_OUTPUT_SCHEMA)].filter(k => ['minimum', 'maximum', 'minLength', 'maxLength', 'minItems', 'maxItems', 'pattern', 'multipleOf'].includes(k));
  assert.deepEqual(unsupported, [], 'no constraint structured outputs does not support');
  assert.equal(closedEverywhere(JUDGE_OUTPUT_SCHEMA), true, 'every object closed and every field required');
});

test('the judge\'s request: Haiku 5.5 at effort "max", adaptive thinking, structured outputs, the cached Code first, base64 captures, no prefill', async () => {
  const sent = [];
  const captures = [{ contentType: 'image/png', base64: 'AAAA', label: 'capture-0.png' }, { contentType: 'image/jpeg', base64: 'BBBB', label: 'capture-1.jpg' }];
  const out = await claudeJudge({ sim: { name: 'Harbour' }, dossier: null, jev: { answers: {} }, triage: triage(), captures, policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED, env: { ANTHROPIC_API_KEY: 'sk-test' }, deps: { claude: fakeClaude(PART_VII, sent) } });
  assert.equal(out.malformed, false);
  const { body, options } = sent[0];
  assert.deepEqual([body.model, JUDGE_MODEL, DEFAULT_JUDGE_EFFORT], ['claude-haiku-5-5', 'claude-haiku-5-5', 'max']);
  assert.equal(body.output_config.effort, 'max');
  assert.deepEqual(body.output_config.format, { type: 'json_schema', schema: JUDGE_OUTPUT_SCHEMA });
  assert.deepEqual(body.thinking, { type: 'adaptive' });
  assert.equal(body.max_tokens, 64000);
  assert.equal(DEFAULT_THRESHOLDS.judge_max_tokens, 64000);
  assert.equal(DEFAULT_THRESHOLDS.judge_timeout_ms, 300000);
  for (const k of ['temperature', 'top_p', 'top_k']) assert.equal(k in body, false, `${k} is not sent (the model refuses it)`);
  assert.deepEqual(body.system, [{ type: 'text', text: judgeSystemPrompt(GUARDIAN_POLICY_TEXT, POLICY_HASH_ANCHORED), cache_control: { type: 'ephemeral' } }]);
  assert.equal(body.messages.length, 1);
  assert.equal(body.messages.at(-1).role, 'user', 'no assistant prefill');
  assert.deepEqual(body.messages[0].content.slice(0, 2), [
    { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'AAAA' } },
    { type: 'image', source: { type: 'base64', media_type: 'image/jpeg', data: 'BBBB' } },
  ]);
  assert.ok(options.signal instanceof AbortSignal, 'bounded by the judge\'s time limit');
  // JUDGE_EFFORT tunes it without a code change; a value the API does not take falls back to max.
  await claudeJudge({ sim: {}, dossier: null, jev: {}, triage: triage(), captures: [], policyText: 'P', policyHash: 'h', env: { JUDGE_EFFORT: 'high' }, deps: { claude: fakeClaude(PART_VII, sent) } });
  await claudeJudge({ sim: {}, dossier: null, jev: {}, triage: triage(), captures: [], policyText: 'P', policyHash: 'h', env: { JUDGE_EFFORT: 'extreme' }, deps: { claude: fakeClaude(PART_VII, sent) } });
  assert.deepEqual([sent[1].body.output_config.effort, sent[2].body.output_config.effort], ['high', 'max']);
});

test('the judge\'s system block is byte-identical across calls; only the user turn carries the case', async () => {
  const sent = [];
  for (const name of ['Harbour', 'Bridge']) await claudeJudge({ sim: { name }, dossier: null, jev: { answers: {} }, triage: triage(), captures: [], policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED, env: {}, deps: { claude: fakeClaude(PART_VII, sent) } });
  assert.equal(JSON.stringify(sent[0].body.system), JSON.stringify(sent[1].body.system));
  assert.match(sent[0].body.system[0].text, /=== POLICY BEGIN ===/);
  assert.equal(sent[0].body.system[0].text.includes('Harbour'), false, 'nothing about a case is in the cached prefix');
  assert.notEqual(sent[0].body.messages[0].content.at(-1).text, sent[1].body.messages[0].content.at(-1).text);
});

test('a refusal, an answer cut off at max_tokens, and a missing key each hold the case (fail closed)', async () => {
  const run = async (respond, env = {}, deps) => claudeJudge({ sim: {}, dossier: null, jev: {}, triage: triage(), captures: [], policyText: 'P', policyHash: 'h', env, deps: deps || { claude: fakeClaude(respond) } });
  const refused = await run(claudeMessage(null, { stop_reason: 'refusal', stop_details: { type: 'refusal', category: 'general_harms', explanation: null } }));
  assert.deepEqual([refused.malformed, refused.error, refused.refusal_category], [true, 'judge_refusal', 'general_harms']);
  assert.equal(decideFinal({ triage: triage(), judge: refused }).outcome, 'hold');
  // A cut-off answer can still parse as JSON: it is held all the same.
  const cut = await run(claudeMessage(JSON.stringify(PART_VII), { stop_reason: 'max_tokens' }));
  assert.deepEqual([cut.malformed, cut.error], [true, 'judge_max_tokens']);
  assert.equal(decideFinal({ triage: triage(), judge: cut }).outcome, 'hold');
  const nokey = await run(null, {}, {});
  assert.deepEqual([nokey.malformed, nokey.error], [true, 'judge_unavailable_no_key']);
  assert.equal(decideFinal({ triage: triage(), judge: nokey }).outcome, 'hold');
  const down = await run(() => { throw apiError(529); });
  assert.equal(down.error, 'judge_unavailable_529');
});

// The real SDK over a scripted fetch: what reaches the wire, and a streamed answer read back.
function sse(text, { stop_reason = 'end_turn', model = 'claude-haiku-5-5' } = {}) {
  const events = [
    ['message_start', { type: 'message_start', message: { id: 'msg_1', type: 'message', role: 'assistant', model, content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 900, cache_creation_input_tokens: 0, cache_read_input_tokens: 8000, output_tokens: 1 } } }],
    ['content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }],
    ...(text ? [['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }]] : []),
    ['content_block_stop', { type: 'content_block_stop', index: 0 }],
    ['message_delta', { type: 'message_delta', delta: { stop_reason, stop_sequence: null }, usage: { output_tokens: 1200 } }],
    ['message_stop', { type: 'message_stop' }],
  ];
  return new Response(events.map(([e, d]) => `event: ${e}\ndata: ${JSON.stringify(d)}\n\n`).join(''), { status: 200, headers: { 'content-type': 'text/event-stream' } });
}

test('through the real @anthropic-ai/sdk: the request reaches /v1/messages streamed with the key, and a streamed verdict is read back', async () => {
  const wire = [];
  const anthropicFetch = async (url, init) => { wire.push({ url: String(url), headers: new Headers(init.headers), body: JSON.parse(init.body) }); return sse(JSON.stringify(PART_VII)); };
  const out = await claudeJudge({ sim: { name: 'Harbour' }, dossier: null, jev: { answers: {} }, triage: triage(), captures: [{ contentType: 'image/png', base64: 'AAAA', label: 'capture-0.png' }], policyText: 'P', policyHash: 'h', env: { ANTHROPIC_API_KEY: 'sk-ant-test-not-real' }, deps: { anthropicFetch } });
  assert.equal(out.malformed, false, JSON.stringify(out));
  assert.deepEqual([out.stop_reason, out.effort, out.model], ['end_turn', 'max', 'claude-haiku-5-5']);
  assert.equal(wire.length, 1);
  assert.match(wire[0].url, /\/v1\/messages$/);
  assert.equal(wire[0].headers.get('x-api-key'), 'sk-ant-test-not-real');
  assert.deepEqual([wire[0].body.stream, wire[0].body.model, wire[0].body.output_config.effort, wire[0].body.output_config.format.type, wire[0].body.thinking.type], [true, 'claude-haiku-5-5', 'max', 'json_schema', 'adaptive']);
  const refused = await claudeJudge({ sim: {}, dossier: null, jev: {}, triage: triage(), captures: [], policyText: 'P', policyHash: 'h', env: { ANTHROPIC_API_KEY: 'sk-ant-test-not-real' }, deps: { anthropicFetch: async () => sse('', { stop_reason: 'refusal' }) } });
  assert.equal(refused.error, 'judge_refusal');
});

test('a capture too large for the Messages API is left out and named to the judge, not sent', () => {
  const big = { contentType: 'image/png', base64: 'A'.repeat(JUDGE_IMAGE_MAX_B64 + 4), label: 'capture-7.png' };
  const body = judgeRequest({ summary: {}, captures: [{ contentType: 'image/png', base64: 'AAAA', label: 'capture-0.png' }, big], policyText: 'P', policyHash: 'h' });
  assert.equal(body.messages[0].content.filter(c => c.type === 'image').length, 1);
  assert.match(body.messages[0].content.at(-1).text, /Left out, not sent: capture-7\.png \(too large to send\)\. Do not score anything only they would show\./);
});

// ── the pipeline ────────────────────────────────────────────────────────────

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
const SIM = 'a1b2c3d4-0000-4000-8000-0000000021a1';
const TWIN_TEXT = 'A digital twin. marker-desc-sentence stays in the listing. Built for Project Bluefin Internal Review.\nCategory: Digital twin\nObjective: Inspect how the Port of Tacoma stages containers.\nLesson: How a terminal moves cargo from ship to rail.\nReal-world link: A twin of the Port of Tacoma terminal at 1 Sitcum Way, from public aerial imagery.';

function pipelineDeps({ judgeAnswer = judge(), jev = jevAnswers(), shadow = null, research = null } = {}) {
  const calls = { judge: [], xai: [] };
  return {
    calls,
    json, cors: {}, auditLog: async () => {},
    verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async r => ((r.headers.get('Authorization') || '').replace('Bearer ', '') === 'admin-1' ? 'admin-1' : null),
    claude: fakeClaude((body) => (body.model !== JUDGE_MODEL && shadow ? shadow(body) : { ...judgeAnswer, malformed: undefined, policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED }), calls.judge),
    grokFetch: async (body, key, opts) => {
      calls.xai.push({ body: JSON.parse(JSON.stringify(body)), key, opts });
      if (research) return research(body);
      return { ok: true, status: 200, json: async () => ({ model: 'grok-test', output: [{ type: 'message', content: [{ type: 'output_text', text: '{"findings":[{"fact":"Port of Tacoma","finding":"A public container port in Tacoma, Washington.","source_url":"https://www.portoftacoma.com/"},{"fact":"x","finding":"made up","source_url":"https://not-cited.example/"}]}', annotations: [{ type: 'url_citation', url: 'https://www.portoftacoma.com/' }] }] }] }), text: async () => '' };
    },
    grokModel: 'grok-test',
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: async () => new Response(JSON.stringify(jev), { status: 200 }),
  };
}

const CAPTURE_MARKER = 'marker-capture-pixels';
const DOSSIER = {
  dossier_version: 1,
  digest: { entity_count: 300, default_material_fraction: 0.2, default_name_fraction: 0.2, script_count: 1, text_string_count: 4 },
  strings: ['marker-string-in-the-space', 'Crane 4 control'],
  scripts: [{ path: 'Soul/marker-script-path/terminal.luau', language: 'luau', lines: 3, source: 'local marker_script_source = basin + tide' }],
  signals: { urls: [] },
};
async function publish(env, { id = SIM, description = `A working harbour.\n${STATEMENT_TEXT}`, tags = [], answers = questionnaireAnswers(), author = 'user-1' } = {}) {
  const sim = { id, name: 'Port of Tacoma Terminal', description, tags, genre: 'Simulation', author_id: author, is_public: true, r2_key: `universes/${id}/universe.pak`, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' }, ...(answers ? { rating_questionnaire: questionnaireRecord(answers) } : {}) };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(`PAK-${id}`).buffer, { sha256: createHash('sha256').update(`PAK-${id}`).digest('hex') });
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(DOSSIER));
  await env.SCENES.put(`universes/${id}/moderation/capture-0.png`, new Uint8Array([0x89, 0x50, 0x4e, 0x47, ...new TextEncoder().encode(CAPTURE_MARKER)]).buffer, { httpMetadata: { contentType: 'image/png' } });
  return sim;
}

test('the researcher receives only the public facts: no capture, script, string or dossier byte reaches its request', async () => {
  const env = environment();
  await publish(env, { description: TWIN_TEXT });
  const d = pipelineDeps();
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(d.calls.xai.length, 1, 'a Digital twin naming a real place is researched');
  const sent = JSON.stringify(d.calls.xai[0].body);
  const captureB64 = Buffer.from(new Uint8Array([0x89, 0x50, 0x4e, 0x47, ...new TextEncoder().encode(CAPTURE_MARKER)])).toString('base64');
  for (const secret of [CAPTURE_MARKER, captureB64, 'marker-string-in-the-space', 'Crane 4 control', 'marker-script-path', 'marker_script_source', JSON.stringify(DOSSIER), 'marker-desc-sentence', 'Bluefin', 'entity_count'])
    assert.equal(sent.includes(secret), false, `${secret.slice(0, 40)} is not sent`);
  // The facts, written out: the title, the category, the real-world link, and the names, addresses and patents in those lines
  // (addresses and patent numbers anywhere in the description). Nothing else of the description.
  const facts = {
    title: 'Port of Tacoma Terminal',
    category: 'Digital twin',
    real_world_link: 'A twin of the Port of Tacoma terminal at 1 Sitcum Way, from public aerial imagery.',
    places_or_businesses: ['Port of Tacoma Terminal', 'Port of Tacoma', 'Sitcum Way'],
    addresses: ['1 Sitcum Way'],
    patents: [],
  };
  assert.deepEqual(rec.research.facts, facts);
  assert.deepEqual(d.calls.xai[0].body, researcherRequest(facts), 'exactly the instruction and these facts');
  assert.deepEqual(Object.keys(d.calls.xai[0].body).sort(), ['input', 'tools']);
  // The findings: one cited source kept, the made-up one dropped, both given to the judge as data.
  assert.deepEqual(rec.research.findings.map(f => f.source_url), ['https://www.portoftacoma.com/']);
  assert.equal(rec.research.dropped, 1);
  const summary = JSON.parse(d.calls.judge[0].body.messages[0].content.at(-1).text.split('\n').at(-1));
  assert.deepEqual(summary.research.findings.map(f => f.source_url), ['https://www.portoftacoma.com/']);
  assert.equal(rec.status, 'approved');
});

test('the researcher\'s failure is not fatal: the judge runs without findings, and the case says so', async () => {
  const env = environment();
  await publish(env, { description: TWIN_TEXT });
  const rec = await runModerationCase(SIM, env, pipelineDeps({ research: () => ({ ok: false, status: 503, text: async () => '' }) }));
  assert.deepEqual([rec.status, rec.research.error, rec.decision.research.error], ['approved', 'researcher_unavailable_503', 'researcher_unavailable_503']);
  assert.ok(rec.history.some(h => h.event === 'researched' && h.detail.error === 'researcher_unavailable_503'));
});

test('the judge is told when the researcher failed, and judges without findings', async () => {
  const sent = [];
  await claudeJudge({ sim: {}, dossier: null, jev: {}, triage: triage(), captures: [], policyText: 'P', policyHash: 'h', research: { ran: true, trigger: 'real_place_or_business', findings: [], error: 'researcher_timeout' }, env: {}, deps: { claude: fakeClaude(PART_VII, sent) } });
  const summary = JSON.parse(sent[0].body.messages[0].content.at(-1).text.split('\n').at(-1));
  assert.deepEqual(summary.research, { ran: true, trigger: 'real_place_or_business', findings: [], failed: true, note: 'The researcher failed; judge without findings.' });
});

test('when the researcher runs: Group A signals, a Digital twin or Product naming a place or business, a patent claim; never without facts', () => {
  const facts = researchFacts({ name: 'Harbour', description: STATEMENT_TEXT });
  assert.equal(researchTrigger(triage(), facts), null, 'a Simulation with no Group A signal');
  assert.equal(researchTrigger(triage({ articles: { system: [], signals: [{ article: 'A13', key: 'crime_rehearsal', p: 0.6, group: 'A' }] } }), facts), 'group_a_signal');
  assert.equal(researchTrigger(triage(), facts, judge({ articles: [cite('A14')] })), 'judge_group_a');
  assert.equal(researchTrigger(triage(), researchFacts({ name: 'Port of Tacoma Terminal', description: TWIN_TEXT })), 'real_place_or_business');
  const patent = researchFacts({ name: 'Wave Turbine', description: `${STATEMENT_TEXT}\nBuilt on our patented design, US 10,123,456 B2.` });
  assert.deepEqual([researchTrigger(triage(), patent), patent.patents], ['patent_claim', ['US 10,123,456 B2']]);
  assert.equal(researchTrigger(triage({ articles: { system: [], signals: [{ article: 'A10', key: 'sensitive_real_place', p: 0.6, group: 'A' }] } }), { places_or_businesses: [], addresses: [], patents: [], real_world_link: null }), null, 'nothing to look up');
});

test('findings are validated: an https source the response cited, a capped size, everything else dropped', () => {
  const ok = validateFindings('{"findings":[{"fact":"a","finding":"b","source_url":"https://a.example/x"},{"fact":"c","finding":"d","source_url":"http://a.example/y"},{"fact":"e","finding":"","source_url":"https://a.example/z"}]}');
  assert.deepEqual([ok.findings.map(f => f.source_url), ok.dropped], [['https://a.example/x'], 2]);
  assert.equal(validateFindings('{"findings":[{"fact":"a","finding":"b","source_url":"https://made.up/"}]}', new Set(['https://a.example/x'])).findings.length, 0, 'a source the response did not cite is dropped');
  assert.equal(validateFindings('{"findings":' + JSON.stringify(Array.from({ length: 40 }, (_, i) => ({ fact: 'f', finding: 'x'.repeat(900), source_url: `https://a.example/${i}` }))) + '}').findings.length <= 12, true);
  assert.equal(validateFindings('not json').error, 'research_not_json');
});

test('the shadow judge never changes the outcome: it is recorded beside the case, and the report counts agreement by outcome', async () => {
  const env = environment({ JUDGE_SHADOW_MODEL: 'claude-sonnet-5-5' });
  await publish(env);
  const d = pipelineDeps({ shadow: () => ({ ...judge(), malformed: undefined, verdict: 'reject', articles: [cite('A4', 0.9)], policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED }) });
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'approved', 'the judge listed it; the shadow\'s reject decides nothing');
  assert.deepEqual([rec.shadow.model, rec.shadow.primary_outcome, rec.shadow.shadow_outcome, rec.shadow.agree], ['claude-sonnet-5-5', 'approve', 'reject', false]);
  assert.deepEqual(d.calls.judge.map(c => c.body.model).sort(), ['claude-haiku-5-5', 'claude-sonnet-5-5']);
  // A shadow that fails changes nothing either.
  const env2 = environment({ JUDGE_SHADOW_MODEL: 'claude-sonnet-5-5' });
  await publish(env2);
  const rec2 = await runModerationCase(SIM, env2, pipelineDeps({ shadow: () => { throw apiError(500); } }));
  assert.deepEqual([rec2.status, rec2.shadow.shadow_error], ['approved', 'judge_unavailable_500']);
  // A control with no shadow: the same decision.
  const env3 = environment();
  await publish(env3);
  const rec3 = await runModerationCase(SIM, env3, pipelineDeps());
  assert.deepEqual([rec3.status, rec3.decision.outcome, rec3.decision.rating, rec3.shadow], [rec.status, rec.decision.outcome, rec.decision.rating, null]);
  const report = (await shadowAgreementReport(env, new URLSearchParams())).body;
  assert.deepEqual([report.cases, report.agree, report.by_primary_outcome.approve.cases, report.by_primary_outcome.approve.shadow_outcomes], [1, 0, 1, { reject: 1 }]);
  const route = await handleModerationRoute(new Request('https://api.eustress.dev/api/admin/moderation/shadow', { headers: { Authorization: 'Bearer admin-1' } }), new URL('https://api.eustress.dev/api/admin/moderation/shadow'), env, null, d);
  assert.equal(route.status, 200);
  assert.equal((await handleModerationRoute(new Request('https://api.eustress.dev/api/admin/moderation/shadow', { headers: { Authorization: 'Bearer user-1' } }), new URL('https://api.eustress.dev/api/admin/moderation/shadow'), env, null, d)).status, 403);
});

test('review_schema and the case reference: on every case and listing, from the first decision, in the author\'s view and email', async () => {
  const env = environment();
  await publish(env);
  const rec = await runModerationCase(SIM, env, pipelineDeps());
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  assert.deepEqual([rec.review_schema, sim.moderation.review_schema], [2, 2]);
  assert.match(rec.reference, /^EC-[0-9A-HJKMNP-TV-Z]{4}(-[0-9A-HJKMNP-TV-Z]{4}){4}$/);
  assert.equal(env.SOCIAL.m.get(`caseref-of:moderation:${SIM}`), rec.reference, 'assigned through case_reference.mjs when the case was made');
  const view = authorView(rec, sim);
  assert.deepEqual([view.reference, view.review_schema], [rec.reference, 2]);
  const mail = renderModerationEmail({ view, kind: 'decision', simName: 'Harbour', username: 'ann', reviewUrl: 'https://eustress.dev/projects', unsubUrl: 'https://api.eustress.dev/u' });
  assert.ok(mail.text.includes(`Case reference: ${rec.reference}`));
  assert.ok(mail.html.includes(rec.reference));
});

test('an unanswered questionnaire at publish is not yet; answering it reopens the listing for review', async () => {
  const env = environment();
  await publish(env, { answers: null });
  const d = pipelineDeps();
  const rec = await runModerationCase(SIM, env, d);
  assert.deepEqual([rec.status, rec.decision.changes], ['changes_requested', ['questionnaire_unanswered']]);
  const post = (token, answers) => handleModerationRoute(new Request(`https://api.eustress.dev/api/simulations/${SIM}/rating-questionnaire`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ answers }) }), new URL(`https://api.eustress.dev/api/simulations/${SIM}/rating-questionnaire`), env, null, d);
  assert.equal((await post('user-2', questionnaireAnswers())).status, 403, 'the author only');
  assert.equal((await post('user-1', { violence: 'none' })).status, 400, 'a partial set is refused');
  const res = await post('user-1', questionnaireAnswers({ violence: 'fantasy' }));
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { ok: true, changed: true, band: 'teen_13', descriptors: ['fantasy violence'], review_schema: 2 });
  const sim = JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`));
  assert.deepEqual([sim.moderation.status, sim.rating_questionnaire.band], ['pending', 'teen_13']);
  const again = await runModerationCase(SIM, env, d);
  assert.deepEqual([again.status, again.decision.rating, again.decision.descriptors], ['approved', 'teen_13', ['fantasy violence']]);
  const form = await (await handleModerationRoute(new Request(`https://api.eustress.dev/api/simulations/${SIM}/rating-questionnaire`, { headers: { Authorization: 'Bearer user-1' } }), new URL(`https://api.eustress.dev/api/simulations/${SIM}/rating-questionnaire`), env, null, d)).json();
  assert.deepEqual([form.answered, form.questions.length, 'band' in form.questions[0].options[0]], [true, QUESTIONS.length, false]);
});

test('an attested owner\'s home twin cited under A13 is held for a person, not rejected', async () => {
  const env = environment();
  await env.USERS.put('property-attestation:user-1', JSON.stringify([HOME]));
  await publish(env);
  const rec = await runModerationCase(SIM, env, pipelineDeps({ judgeAnswer: judge({ verdict: 'reject', articles: [cite('A13', 0.95)] }) }));
  assert.deepEqual([rec.status, rec.decision.reasons.at(-1).article, rec.decision.reasons.at(-1).attested_property], ['held', 'A13', true]);
  assert.ok(env.SOCIAL.m.has(`wsq:hold:${SIM}`), 'queued for the Review Workspace');
  const env2 = environment();
  await publish(env2);
  assert.equal((await runModerationCase(SIM, env2, pipelineDeps({ judgeAnswer: judge({ verdict: 'reject', articles: [cite('A13', 0.95)] }) }))).status, 'rejected', 'control: no attestation');
});

test('an appeal goes to the Review Workspace and its members are told; no model is asked', async () => {
  const env = environment();
  await env.SOCIAL.put('workspace:review', JSON.stringify({ space_id: 'b0b0b0b0-0000-4000-8000-00000000beef', members: ['admin-1'] }));
  await publish(env);
  const d = pipelineDeps({ judgeAnswer: judge({ verdict: 'reject', articles: [cite('A4', 0.9)] }) });
  assert.equal((await runModerationCase(SIM, env, d)).status, 'rejected');
  const before = d.calls.judge.length + d.calls.xai.length;
  const res = await handleModerationRoute(new Request(`https://api.eustress.dev/api/simulations/${SIM}/appeal`, { method: 'POST', headers: { Authorization: 'Bearer user-1' }, body: JSON.stringify({ text: 'The lab script teaches the principle, not a recipe.' }) }), new URL(`https://api.eustress.dev/api/simulations/${SIM}/appeal`), env, null, d);
  assert.equal(res.status, 202);
  assert.equal(d.calls.judge.length + d.calls.xai.length, before, 'no model reviews an appeal');
  assert.equal(JSON.parse(env.SOCIAL.m.get(`wsq:appeal:${SIM}`)).kind, 'appeal');
  const feed = [...env.SOCIAL.m.keys()].filter(k => k.startsWith('notif:admin-1:')).map(k => JSON.parse(env.SOCIAL.m.get(k)));
  assert.deepEqual(feed.map(i => [i.kind, i.event]), [['review_workspace', 'appeal']]);
  // A person resolves it; it leaves the queue.
  const exec = makeToolExecutor(env, d, { actor: 'admin', actorId: 'admin-1' });
  assert.equal((await exec('moderation_resolve_appeal', { sim_id: SIM, decision: 'upheld', rationale: 'Soul/lab/recipe.luau lists quantities and steps.' })).ok, true);
  assert.equal(env.SOCIAL.m.has(`wsq:appeal:${SIM}`), false);
  // The workspace itself is set by an administrator only.
  const put = (token, body) => handleModerationRoute(new Request('https://api.eustress.dev/api/admin/moderation/workspace', { method: 'PUT', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify(body) }), new URL('https://api.eustress.dev/api/admin/moderation/workspace'), env, null, d);
  assert.equal((await put('user-1', { space_id: 'b0b0b0b0-0000-4000-8000-00000000beef', members: ['user-1'] })).status, 403);
  assert.equal((await put('admin-1', { space_id: 'nope', members: ['admin-1'] })).status, 400);
  assert.equal((await put('admin-1', { space_id: 'b0b0b0b0-0000-4000-8000-00000000beef', members: ['admin-1', 'admin-2'] })).status, 200);
});

// ── a takedown's hold ───────────────────────────────────────────────────────

test('a TAKE IT DOWN request\'s hold is not lifted by moderation_release or an overturned appeal; only the takedown review lifts it', async () => {
  const env = environment();
  const held = { sim_id: SIM, author_id: 'user-1', status: 'quarantined', history: [], decision: { lane: 'legal', reasons: [] }, legal_hold: { category: 'ncii_or_real_person_sexual', source: 'takedown', takedowns: ['td-1'], at: '2026-10-08T00:00:00Z' } };
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(held));
  const exec = makeToolExecutor(env, { auditLog: async () => {} }, { actor: 'admin', actorId: 'admin-1' });
  const released = await exec('moderation_release', { sim_id: SIM, rationale: 'A person reviewed the captures and found no real person.' });
  assert.equal(released.ok, false);
  assert.match(released.error, /TAKE IT DOWN/);
  assert.deepEqual((await loadCase(env, SIM)).legal_hold.takedowns, ['td-1'], 'the hold stands');
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ ...held, status: 'appealed', appeal: { status: 'pending', from_status: 'rejected', text: 'x' } }));
  const overturned = await exec('moderation_resolve_appeal', { sim_id: SIM, decision: 'overturned', rationale: 'The appeal is right about the article.' });
  assert.equal(overturned.ok, false);
  assert.match(overturned.error, /TAKE IT DOWN/);
  assert.deepEqual((await loadCase(env, SIM)).legal_hold.takedowns, ['td-1']);
  // Control: the same release with no takedown in the hold goes through.
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ ...held, legal_hold: { ...held.legal_hold, source: 'moderation', takedowns: [] } }));
  assert.equal((await exec('moderation_release', { sim_id: SIM, rationale: 'A person reviewed the captures and found no real person.' })).ok, true);
});
