// The Eustress Guardian Code v2.2 (docs/architecture/AI_GUARDIAN_POLICY_v1.0.md), what it adds to v2.1, as the Worker applies
// it: B11, no hard drugs ever (cannabis included), with its life-saving exception (medical care, overdose first aid, the science
// of addiction) rated mature_17; the adult_21 band for alcohol, tobacco and showing psychedelic use (MDMA and ketamine
// included); the questionnaire that asks about them; and review_schema 2. The 21+ gate on every surface is
// tests/adult_content_gate.test.mjs and tests/adult_image_urls.test.mjs.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  decideFinal, decideTriage, buildJevQuestions, extractQualificationStatement, parseJudgeVerdict, makeToolExecutor, handleModerationRoute,
  authorView, AUTHOR_COPY, ARTICLE_SIGNALS, GROUP_B, RATINGS, DEFAULT_THRESHOLDS, JUDGE_OUTPUT_SCHEMA, POLICY_VERSION, POLICY_HASH_ANCHORED,
  REVIEW_SCHEMA, CODE_TESTS, SUBSTANCE_FLOORS, QUESTIONNAIRE_B11_NOTICE,
} from '../src/moderation.mjs';
import {
  QUESTIONS, QUESTIONNAIRE_VERSION, DESCRIPTORS, PSYCHEDELIC_USE_RATING, ratingFromAnswers, questionnaireRecord, questionnaireOf, questionnaireForm,
} from '../src/rating_questionnaire.mjs';
import { renderModerationEmail } from '../src/moderation_email.mjs';
import { GUARDIAN_POLICY_TEXT, MODERATION_PLAYBOOK_TEXT } from '../src/generated/policy_text.mjs';
import { questionnaireAnswers } from './helpers/claude_fake.mjs';

const EM_DASH = String.fromCodePoint(0x2014);

// ── fixtures ────────────────────────────────────────────────────────────────

const STATEMENT = extractQualificationStatement([
  'Category: Training',
  'Objective: Give naloxone to a person who has stopped breathing and keep them alive until help arrives.',
  'Lesson: How to recognise an opioid overdose and give naloxone.',
  'Real-world link: Follows the published overdose response steps.',
].join('\n'));
const ANSWERED = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers()) });
const tests = (score = 2) => Object.fromEntries(CODE_TESTS.map(k => [k, { score, evidence: `${k}: the trainee kneeling by the casualty in capture-1` }]));
const judge = (over = {}) => ({
  malformed: false, verdict: 'publish', articles: [], tests: tests(), statement_check: 'matches', rating_floor: 'all_ages', descriptors: [],
  is_game: false, is_fiction: false, featured_candidate: false, spatial_evidence: ['a casualty on a kitchen floor with a naloxone kit open'],
  rationale: 'Every test is shown.', confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, ...over,
});
const cite = (id, evidence, confidence = 0.9) => ({ id, confidence, evidence });
/// Jev's answers: each named noul question at its p, content_rating at `rating` (all_ages unless named).
const jev = (answers = {}, rating = 'all_ages') => ({
  answers: {
    ...Object.fromEntries(Object.entries(answers).map(([k, p]) => [k, { type: 'noul', p }])),
    content_rating: { type: 'choice', choice: rating, confidence: 0.9, probabilities: { [rating]: 0.9 } },
    quality_band: { type: 'score', probabilities: { solid_authored_experience: 0.85 }, confidence: 0.85 },
  },
});
/// Triage over Jev's answers with the system's signals, then the rules with the judge's answer.
function review(jevAnswers, judgeOver = {}, { questionnaire = ANSWERED, rating = 'all_ages' } = {}) {
  const triage = decideTriage({ signals: { statement: STATEMENT, questionnaire }, jev: jev(jevAnswers, rating) });
  return { triage, final: decideFinal({ triage, judge: judge(judgeOver) }) };
}
const last = d => d.reasons.at(-1);
const b11Of = t => [...t.articles.system, ...t.articles.signals].filter(a => a.article === 'B11');

// ── the Code ────────────────────────────────────────────────────────────────

test('the Code (v2.3, which carries v2.2) is anchored by its hash, with B11 in the founder\'s words and adult_21 as the top band', () => {
  assert.equal(POLICY_VERSION, '2.3');
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  assert.equal(POLICY_HASH_ANCHORED, 'sha256:99ced3f2e27e715a52483581375cb9eda9401944ca8f4412483da2f27e5f39b0');
  assert.match(GUARDIAN_POLICY_TEXT, /^# Eustress Guardian Code v2\.3\n\*\*Version:\*\* 2\.3\n/);
  assert.match(GUARDIAN_POLICY_TEXT, /\n- v2\.2 \(founder direction, 2026-10-08\): /);
  assert.ok(GUARDIAN_POLICY_TEXT.includes('- **B11. Hard drugs.** *No hard drugs, ever.* Showing the use, sale, making or glamorising of opioids outside medical care, cocaine or crack, methamphetamine, or cannabis. Not B11: medical care (a nurse giving prescribed morphine); overdose first aid (naloxone); the science of addiction, its treatment and its prevention, taught accurately, rated `mature_17`. Psychedelics (psilocybin, LSD, DMT, mescaline, MDMA and ketamine) are not B11; showing their use is rated `adult_21`.\n'));
  assert.ok(GUARDIAN_POLICY_TEXT.includes('- `adult_21`: alcohol and tobacco, and showing the use of psychedelics. Shown only to an account that has turned on Adult content and whose verified age is 21 or over.'));
  assert.match(GUARDIAN_POLICY_TEXT, /- `adult_18`: [^\n]*\n- `adult_21`: /, 'adult_21 follows adult_18');
  const mature = /- `mature_17`: ([^\n]*)/.exec(GUARDIAN_POLICY_TEXT)[1];
  assert.doesNotMatch(mature, /drug references/);
  assert.match(mature, /drug education\.$/, 'mature_17 keeps drug education');
  assert.match(GUARDIAN_POLICY_TEXT, /sexual content, alcohol, tobacco, psychedelics, simulated gambling/);
  assert.doesNotMatch(GUARDIAN_POLICY_TEXT, /alcohol, tobacco or drugs/);
  assert.match(GUARDIAN_POLICY_TEXT, /`adult_18` and `adult_21` listings are both adult content, and both need the Adult content setting/);
  assert.match(GUARDIAN_POLICY_TEXT, /An `adult_21` listing also needs a verified age of 21 or over/);
  assert.match(GUARDIAN_POLICY_TEXT, /"rating_floor": "all_ages" \| "ages_10" \| "teen_13" \| "mature_17" \| "adult_18" \| "adult_21",/);
  assert.match(GUARDIAN_POLICY_TEXT, /B11 is never a feature to remove: a Space whose subject is hard drugs is rejected, and a single removable hard-drug prop in a Space that otherwise qualifies is still B11\./);
  assert.match(GUARDIAN_POLICY_TEXT, /`review_schema`, 2 for this Code/);
  assert.equal(GUARDIAN_POLICY_TEXT.includes(EM_DASH), false);
  assert.match(MODERATION_PLAYBOOK_TEXT, /^# Eustress Moderation Playbook v2\.3/);
  assert.match(MODERATION_PLAYBOOK_TEXT, /\| B11 \| `hard_drug_depiction`/);
  assert.equal(MODERATION_PLAYBOOK_TEXT.includes(EM_DASH), false);
  assert.equal(REVIEW_SCHEMA, 2);
  assert.deepEqual(RATINGS, ['all_ages', 'ages_10', 'teen_13', 'mature_17', 'adult_18', 'adult_21']);
  assert.deepEqual(JUDGE_OUTPUT_SCHEMA.properties.rating_floor.enum, RATINGS);
  assert.deepEqual(JUDGE_OUTPUT_SCHEMA.properties.policy_version.enum, ['2.3']);
  assert.ok(GROUP_B.includes('B11'));
});

// ── the Jev battery ─────────────────────────────────────────────────────────

test('Jev asks B11 in the Code\'s bright-line words, naming every hard drug and every exception, and asks the three Part V questions', () => {
  const q = buildJevQuestions();
  assert.equal(ARTICLE_SIGNALS.hard_drug_depiction, 'B11');
  const b11 = q.hard_drug_depiction;
  assert.equal(b11.type, 'noul');
  assert.match(b11.instructions, /^Code B11\. No hard drugs, ever\. /);
  for (const drug of ['opioids outside medical care', 'cocaine or crack', 'methamphetamine', 'cannabis']) assert.ok(b11.instructions.includes(drug) && b11.criteria.true.includes(drug), drug);
  for (const words of ['use, sale, making or glamorising', 'a single prop included']) assert.ok(b11.instructions.includes(words), words);
  for (const exception of ['Medical care (a nurse giving prescribed morphine)', 'overdose first aid (naloxone)', 'the science of addiction, its treatment and its prevention, taught accurately']) assert.ok(b11.instructions.includes(exception), exception);
  for (const p of ['psilocybin', 'LSD', 'DMT', 'mescaline', 'MDMA', 'ketamine']) assert.ok(b11.instructions.includes(p), `${p} is named as not B11`);
  assert.match(q.drug_education.instructions, /^Code B11, what is not B11\. .*naloxone.*addiction/);
  assert.match(q.drug_education.criteria.false, /drugs also appear in some other way/, 'education only: a Space that also shows hard drugs is not the exception');
  assert.match(q.psychedelic_use.instructions, /^Code Part V, adult_21\. .*psilocybin, LSD, DMT, mescaline, MDMA or ketamine/);
  assert.match(q.alcohol_tobacco.instructions, /^Code Part V, adult_21\. .*alcohol or tobacco/);
  assert.equal('drugs_alcohol_tobacco' in q, false, 'the one mixed question is gone');
  assert.deepEqual(Object.keys(q.content_rating.criteria), RATINGS);
  assert.match(q.content_rating.criteria.adult_21, /^Alcohol or tobacco, or the use of psychedelics/);
  assert.doesNotMatch(q.content_rating.criteria.mature_17, /drug references/);
  assert.deepEqual(SUBSTANCE_FLOORS, { drug_education: 'mature_17', psychedelic_use: 'adult_21', alcohol_tobacco: 'adult_21' });
  assert.equal(DEFAULT_THRESHOLDS.substance_rating, 0.60);
});

// ── B11 rejects ─────────────────────────────────────────────────────────────

test('B11: a confident Jev answer is a system citation that rejects under rule 2, never removable, even with every test at 3', () => {
  const { triage, final } = review({ hard_drug_depiction: 0.9 }, { tests: tests(3) });
  assert.deepEqual(b11Of(triage).map(a => [a.key, a.p]), [['hard_drug_depiction', 0.9]]);
  assert.equal(triage.articles.system.some(a => a.article === 'B11'), true, 'a system citation the judge cannot clear');
  assert.deepEqual([final.outcome, final.lane, last(final).code, last(final).article, last(final).rule, last(final).action], ['reject', 'quality', 'code_B11', 'B11', 2, 'reject']);
  assert.equal(final.changes, null, 'not "not yet": nothing is named for removal');
});

test('B11 rejects the depiction of each hard drug the Code names, cannabis included, and a single prop', () => {
  const shown = {
    opioids: 'a character injects heroin in the alley behind the clinic, capture-2',
    'prescription opioids outside medical care': 'oxycodone pills sold from a car boot in capture-1; GUI "Pills: $20"',
    cocaine: 'lines of cocaine on the club table in capture-3, cheered by NPCs',
    crack: 'a crack pipe lit in the stairwell, capture-0, Soul/stairwell.luau plays the inhale',
    methamphetamine: 'a meth lab with labelled flasks in the trailer, capture-4',
    cannabis: 'avatars pass a lit cannabis joint around the campfire, capture-1',
    'cannabis sold': 'a dispensary counter selling cannabis edibles in capture-2',
    'one prop': 'a single baggie of cocaine on the bar counter of an otherwise accurate pub twin, capture-5',
  };
  for (const [drug, evidence] of Object.entries(shown)) {
    // The judge cites it, with every test at 3: rejected, not "not yet".
    const d = decideFinal({ triage: review({}).triage, judge: judge({ articles: [cite('B11', evidence)], tests: tests(3) }) });
    assert.deepEqual([d.outcome, last(d).code, last(d).rule, last(d).source], ['reject', 'code_B11', 2, 'judge'], drug);
    assert.equal(parseJudgeVerdict(JSON.stringify({ ...judge({ articles: [cite('B11', evidence)] }), malformed: undefined, policy_version: '2.2', policy_hash: POLICY_HASH_ANCHORED })).malformed, false, `${drug}: B11 is an article the judge may cite`);
  }
  // Below article_judge_cite, a person decides; a Jev signal the judge leaves uncited does too.
  const unsure = decideFinal({ triage: review({}).triage, judge: judge({ articles: [cite('B11', shown.cannabis, 0.6)] }) });
  assert.deepEqual([unsure.outcome, last(unsure).code, last(unsure).article, last(unsure).rule], ['hold', 'article_unsure', 'B11', 6]);
  const signal = review({ hard_drug_depiction: 0.6 });
  assert.deepEqual(b11Of(signal.triage).map(a => a.key), ['hard_drug_depiction']);
  assert.equal(signal.triage.articles.system.some(a => a.article === 'B11'), false);
  assert.deepEqual([signal.final.outcome, last(signal.final).code, last(signal.final).article], ['hold', 'article_signal_not_cited', 'B11']);
  // Thresholds are the other Group B articles': 0.85 cites, 0.50 signals.
  assert.equal(review({ hard_drug_depiction: 0.85 }).triage.articles.system.some(a => a.article === 'B11'), true);
  assert.equal(b11Of(review({ hard_drug_depiction: 0.49 }).triage).length, 0);
});

test('a person rejecting under B11 shows the author the B11 copy, which tells them plainly it cannot be listed', () => {
  const d = review({ hard_drug_depiction: 0.95 }).final;
  const view = authorView({ status: 'rejected', decision: { reasons: d.reasons, lane: d.lane } }, {});
  assert.deepEqual(view.reasons.map(r => r.category), ['code_B11']);
  const copy = AUTHOR_COPY.code_B11;
  assert.match(copy.why, /^Code B11: Eustress lists no hard drugs, ever/);
  for (const drug of ['opioids outside medical care', 'cocaine or crack', 'methamphetamine', 'cannabis']) assert.ok(copy.why.includes(drug), drug);
  assert.match(copy.what_to_change, /cannot be listed/);
  assert.equal(JSON.stringify(copy).includes(EM_DASH), false);
});

// ── the life-saving exception ───────────────────────────────────────────────

test('B11 never fires on naloxone training, medical morphine or addiction science; each is listed at mature_17', () => {
  const cases = {
    // Opioids are on screen in each: Jev's hard-drug answer is not zero, and its education answer is confident.
    'naloxone overdose training': { hard_drug_depiction: 0.62, drug_education: 0.93 },
    'a nurse giving prescribed morphine': { hard_drug_depiction: 0.55, drug_education: 0.9 },
    'the science of addiction and its treatment': { hard_drug_depiction: 0.71, drug_education: 0.88 },
  };
  for (const [what, answers] of Object.entries(cases)) {
    const { triage, final } = review(answers, { rating_floor: 'mature_17' });
    assert.deepEqual(b11Of(triage), [], `${what}: no B11 citation or signal`);
    assert.equal(triage.rating, 'mature_17', `${what}: Jev's rating is raised to mature_17`);
    assert.deepEqual([final.outcome, final.rating], ['approve', 'mature_17'], what);
  }
  // Education alone (no hard-drug answer at all) is mature_17 too, from Jev's floor even when its content_rating said less.
  const alone = review({ drug_education: 0.9 }, { rating_floor: 'all_ages' });
  assert.deepEqual([alone.triage.rating, alone.final.outcome, alone.final.rating], ['mature_17', 'approve', 'mature_17']);
  // Below substance_rating the answer is not the exception: the hard-drug signal stands.
  assert.deepEqual(b11Of(review({ hard_drug_depiction: 0.62, drug_education: 0.59 }).triage).map(a => a.key), ['hard_drug_depiction']);
});

test('the exception lowers a confident hard-drug answer to a signal, never to nothing, and never lowers the judge', () => {
  const { triage, final } = review({ hard_drug_depiction: 0.95, drug_education: 0.9 });
  assert.equal(triage.articles.system.some(a => a.article === 'B11'), false, 'not a system citation');
  assert.deepEqual(b11Of(triage).map(a => [a.key, a.drug_education]), [['hard_drug_depiction', true]], 'a signal the judge or a person decides');
  assert.deepEqual([final.outcome, last(final).code, last(final).article], ['hold', 'article_signal_not_cited', 'B11']);
  const cited = decideFinal({ triage, judge: judge({ articles: [cite('B11', 'a cocaine line on the mirror beside the naloxone poster, capture-2')] }) });
  assert.deepEqual([cited.outcome, last(cited).code], ['reject', 'code_B11'], 'the judge\'s own citation still rejects');
});

// ── adult_21 ────────────────────────────────────────────────────────────────

test('psychedelic use, alcohol and tobacco each rate adult_21, whatever Jev\'s content_rating said', () => {
  for (const [what, answers] of [['psychedelic use', { psychedelic_use: 0.9 }], ['alcohol or tobacco', { alcohol_tobacco: 0.9 }]]) {
    const { triage, final } = review(answers, { rating_floor: 'all_ages' }, { rating: 'teen_13' });
    assert.equal(triage.rating, 'adult_21', what);
    assert.deepEqual(b11Of(triage), [], `${what} is not B11`);
    assert.deepEqual([final.outcome, final.rating, final.featured_candidate], ['approve', 'adult_21', false], what);
    assert.equal(review(answers, {}, {}).triage.rating, 'adult_21');
    assert.equal(review({ [Object.keys(answers)[0]]: 0.59 }).triage.rating, 'all_ages', `${what} below substance_rating sets no floor`);
  }
  assert.equal(PSYCHEDELIC_USE_RATING, 'adult_21', 'McKale to confirm: one constant');
  // The judge's floor and the questionnaire's band raise it the same way.
  for (const [what, over] of [['alcohol', { alcohol_tobacco: 'alcohol' }], ['tobacco', { alcohol_tobacco: 'tobacco' }], ['psychedelics', { drugs: 'psychedelics' }]]) {
    const q = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers(over)) });
    assert.equal(review({}, {}, { questionnaire: q }).final.rating, 'adult_21', what);
  }
  assert.equal(review({}, { rating_floor: 'adult_21' }).final.rating, 'adult_21');
  // MDMA and ketamine are psychedelics here: named in the question and the answer that rate adult_21.
  const q = buildJevQuestions();
  for (const drug of ['MDMA', 'ketamine']) {
    assert.ok(q.psychedelic_use.instructions.includes(drug), drug);
    assert.ok(QUESTIONS.find(x => x.id === 'drugs').options.find(o => o.value === 'psychedelics').label.includes(drug), drug);
  }
});

test('adult_21 is adult content to the rules: never featured, and B6(h) is cited in it whatever the account', () => {
  const best = { tests: Object.fromEntries(CODE_TESTS.map(k => [k, { score: 3, evidence: `${k}: the brewing line in capture-1 with its gauges` }])), featured_candidate: true };
  assert.equal(review({}, best).final.featured_candidate, true, 'control: an all_ages candidate is recorded');
  assert.equal(review({}, { ...best, rating_floor: 'adult_21' }).final.featured_candidate, false);
  assert.equal(review({ alcohol_tobacco: 0.9 }, best).final.featured_candidate, false);
  const business = { statement: STATEMENT, questionnaire: ANSWERED, account: { attested_properties: [], verified_business: true, business_name: 'Brewer', payment_connection: true } };
  const allowed = decideTriage({ signals: business, jev: jev({ b6h_offsite_payment: 0.9 }) });
  assert.equal(allowed.articles.exempt.length, 1, 'control: a verified business connection allows it on an all_ages listing');
  const brewery = decideTriage({ signals: business, jev: jev({ b6h_offsite_payment: 0.9, alcohol_tobacco: 0.9 }) });
  assert.deepEqual([brewery.articles.exempt.length, brewery.articles.system.some(a => a.article === 'B6')], [0, true], 'adult_21 is an Adult listing (B6(h))');
  const byJudge = decideFinal({ triage: allowed, judge: judge({ rating_floor: 'adult_21' }) });
  assert.equal(byJudge.outcome === 'changes_requested' && byJudge.reasons.some(r => r.article === 'B6'), true, 'cited after all when the judge rates it adult_21');
});

test('a person never sets featured on an adult_21 listing', async () => {
  const kv = new Map();
  const env = {
    SOCIAL: { get: async k => kv.get(k) ?? null, put: async (k, v) => { kv.set(k, String(v)); }, delete: async k => { kv.delete(k); }, list: async () => ({ keys: [], list_complete: true }) },
    SCENES: { get: async () => null, head: async () => null, put: async () => null, delete: async () => {}, list: async () => ({ objects: [] }) },
    USERS: { get: async () => null, put: async () => {}, delete: async () => {} },
    AUDIT_LOG: { get: async () => null, put: async () => {} },
  };
  const SIM = 'a1b2c3d4-0000-4000-8000-0000000022a1';
  const triage = decideTriage({ signals: { statement: STATEMENT, questionnaire: ANSWERED }, jev: jev({}) });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: 'user-1', status: 'held', history: [], triage, judge: judge() }));
  const run = makeToolExecutor(env, { auditLog: async () => {}, json: v => v }, { actor: 'admin', actorId: 'admin-1' });
  const refused = await run('moderation_approve', { sim_id: SIM, rating: 'adult_21', featured: true, rationale: 'A person read the brewery twin and its gauges.' });
  assert.equal(refused.ok, false);
  assert.equal(refused.error, 'an adult_21 listing is never featured');
  const listed = await run('moderation_approve', { sim_id: SIM, rating: 'adult_21', rationale: 'A person read the brewery twin and its gauges.' });
  assert.equal(listed.ok, true, 'listed at adult_21 without featured');
  assert.equal(JSON.parse(kv.get(`modcase:${SIM}`)).decision.rating, 'adult_21');
});

test('the author\'s email names the 21+ band by age', () => {
  const view = { status: 'listed', rating: 'adult_21', headline: 'Listed', reasons: [], descriptors: ['alcohol'] };
  const mail = renderModerationEmail({ view, kind: 'decision', simName: 'Brewery', username: 'ann', reviewUrl: 'https://eustress.dev/projects', unsubUrl: 'https://api.eustress.dev/u' });
  assert.ok(mail.text.includes('Adults only (21+)'), mail.text);
});

// ── the questionnaire ───────────────────────────────────────────────────────

test('the questionnaire: alcohol, tobacco and psychedelic use map to adult_21, drug education to mature_17, hard drugs to B11', () => {
  assert.equal(QUESTIONNAIRE_VERSION, 2);
  assert.deepEqual(QUESTIONS.map(q => q.id), ['violence', 'blood', 'language', 'sexual', 'nudity', 'alcohol_tobacco', 'drugs', 'gambling', 'purchases', 'interaction']);
  assert.deepEqual(DESCRIPTORS.slice(7, 10), ['alcohol', 'tobacco', 'psychedelics']);
  assert.equal(DESCRIPTORS.includes('alcohol, tobacco or drugs'), false);
  const of = over => ratingFromAnswers(questionnaireAnswers(over));
  assert.deepEqual(of({ alcohol_tobacco: 'alcohol' }), { band: 'adult_21', descriptors: ['alcohol'], articles: [] });
  assert.deepEqual(of({ alcohol_tobacco: 'tobacco' }), { band: 'adult_21', descriptors: ['tobacco'], articles: [] });
  assert.deepEqual(of({ alcohol_tobacco: 'both' }), { band: 'adult_21', descriptors: ['alcohol', 'tobacco'], articles: [] });
  assert.deepEqual(of({ drugs: 'psychedelics' }), { band: 'adult_21', descriptors: ['psychedelics'], articles: [] });
  assert.deepEqual(of({ drugs: 'education' }), { band: 'mature_17', descriptors: [], articles: [] });
  assert.deepEqual(of({ drugs: 'education', violence: 'realistic' }), { band: 'mature_17', descriptors: ['realistic violence'], articles: [] });
  assert.deepEqual(of({ drugs: 'hard_drugs' }).articles, ['B11']);
  assert.deepEqual(of({ sexual: 'explicit', alcohol_tobacco: 'alcohol' }).band, 'adult_21', 'the highest band wins');
  // The words the author reads.
  const drugs = QUESTIONS.find(q => q.id === 'drugs');
  const hard = drugs.options.find(o => o.value === 'hard_drugs').label;
  for (const words of ['opioids outside medical care', 'cocaine or crack', 'methamphetamine', 'cannabis', 'cannot be listed']) assert.ok(hard.includes(words), words);
  assert.match(drugs.options.find(o => o.value === 'education').label, /naloxone.*addiction/);
  // The form hides bands and articles alike: the author answers what the Space contains.
  const form = questionnaireForm();
  assert.equal(form.version, 2);
  assert.equal(JSON.stringify(form).includes('B11'), true, 'the label itself says it');
  assert.equal(form.questions.every(q => q.options.every(o => !('band' in o) && !('articles' in o))), true);
  // A version 1 record (one "alcohol, tobacco or drugs" answer) cannot say which band the Space is in: it reads as unanswered.
  const v1 = { version: 1, answers: { violence: 'none', blood: 'none', language: 'none', sexual: 'none', nudity: 'none', substances: 'present', gambling: 'none', purchases: 'none', interaction: 'none' }, band: 'mature_17' };
  assert.deepEqual(questionnaireOf({ rating_questionnaire: v1 }), { answered: false, band: null, descriptors: [], articles: [], answers: null });
  assert.equal(JSON.stringify(QUESTIONS).includes(EM_DASH), false);
});

test('a questionnaire answer of hard drugs is a B11 signal: the judge cites it, or a person decides', () => {
  const q = questionnaireOf({ rating_questionnaire: questionnaireRecord(questionnaireAnswers({ drugs: 'hard_drugs' })) });
  assert.deepEqual([q.answered, q.articles], [true, ['B11']]);
  const { triage, final } = review({}, {}, { questionnaire: q });
  assert.deepEqual(b11Of(triage).map(a => [a.key, a.deterministic]), [['questionnaire_answer', true]]);
  assert.deepEqual([final.outcome, last(final).code, last(final).article], ['hold', 'article_signal_not_cited', 'B11']);
  const cited = decideFinal({ triage, judge: judge({ articles: [cite('B11', 'cannabis plants under grow lights in the shed, capture-3')] }) });
  assert.deepEqual([cited.outcome, last(cited).code], ['reject', 'code_B11']);
});

test('the questionnaire route tells an author who answers hard drugs that the Space cannot be listed', async () => {
  const kv = () => { const m = new Map(); return { m, get: async k => m.get(k) ?? null, put: async (k, v) => { m.set(k, String(v)); }, delete: async k => { m.delete(k); }, list: async () => ({ keys: [], list_complete: true }) }; };
  const env = { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: { get: async () => null, head: async () => null, put: async () => null, list: async () => ({ objects: [] }) } };
  const SIM = 'a1b2c3d4-0000-4000-8000-0000000022b2';
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, author_id: 'user-1', moderation: { status: 'pending' } }));
  const deps = { json: (v, status, headers) => new Response(JSON.stringify(v), { status, headers }), cors: {}, auditLog: async () => {}, verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null, requireAdmin: async () => null };
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}/rating-questionnaire`);
  const post = answers => handleModerationRoute(new Request(url, { method: 'POST', headers: { Authorization: 'Bearer user-1' }, body: JSON.stringify({ answers }) }), url, env, null, deps);

  const hard = await post(questionnaireAnswers({ drugs: 'hard_drugs' }));
  assert.equal(hard.status, 200);
  assert.deepEqual(await hard.json(), { ok: true, changed: true, band: 'adult_21', descriptors: [], articles: ['B11'], notice: QUESTIONNAIRE_B11_NOTICE, review_schema: 2 });
  assert.match(QUESTIONNAIRE_B11_NOTICE, /cannot be listed/);
  assert.equal(QUESTIONNAIRE_B11_NOTICE.includes(EM_DASH), false);
  assert.deepEqual(JSON.parse(env.SOCIAL.m.get(`sim:${SIM}`)).rating_questionnaire.articles, ['B11'], 'stored as given');

  const beer = await post(questionnaireAnswers({ alcohol_tobacco: 'alcohol' }));
  assert.deepEqual(await beer.json(), { ok: true, changed: true, band: 'adult_21', descriptors: ['alcohol'], review_schema: 2 }, 'no notice for a band');
});
