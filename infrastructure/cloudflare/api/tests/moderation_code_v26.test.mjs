// Guardian Code v2.6 (McKale, 2026-11-14: "allow video game violence with certain precautions. Must be historical or
// fictional. No loot boxes/gambling. No plots to harm or kill people that relate to real life."): combat is listed in fiction
// or history, within B3's limits; a game with combat has no random reward container or wager (B6(b)); violence sets a rating
// floor (Part V); and the Jev questions draw the same lines the Code does, so a system citation never rejects combat the Code
// allows.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  decideFinal, buildJevQuestions, extractQualificationStatement, authorView, violenceFloor,
  AUTHOR_COPY, CODE_TESTS, POLICY_VERSION, POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { createHash } from 'node:crypto';

const STATEMENT = extractQualificationStatement([
  'Category: Fiction',
  'Objective: Hold the river crossing against the iron golems for ten waves.',
  'Lesson: How flanking, cover and supply lines decide a defence.',
  'Real-world link: none; an invented war that carries the logistics of a real one.',
].join('\n'));

const scores = (over = {}) => Object.fromEntries(CODE_TESTS.map(k => [k, { score: over[k] ?? 2, evidence: `${k}: GolemWaves.server.luau spawns golems at the ford in capture-1` }]));
const judge = (over = {}) => ({
  malformed: false, verdict: 'publish', articles: [], tests: scores(over.scores), statement_check: 'matches', rating_floor: 'all_ages',
  descriptors: ['fantasy violence'], is_game: true, is_fiction: true, featured_candidate: false,
  spatial_evidence: ['a stone ford with barricades and three golems advancing'], rationale: 'Fictional combat within B3.', confidence: 0.9,
  flagged_for_review: false, suggested_edit_to_publish: null, second_opinion: true, ...over,
});
const triage = (over = {}) => ({
  outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'interactive_experience', rating: null, articles: { system: [], signals: [] },
  statement: STATEMENT, scripts_truncated: false, game_mechanics: 0.9, lawful_decision_training: 0.02,
  questionnaire: { answered: false, band: null, descriptors: [] }, fiction_p: 0.9, fiction_labelled: true,
  account: { attested_properties: [], verified_business: false, payment_connection: false }, ...over,
});
const decide = (t = {}, j = {}) => decideFinal({ triage: triage(t), judge: judge(j) });
const cite = (id, confidence = 0.9) => ({ id, confidence, evidence: `Code ${id}: GolemWaves.server.luau line 88`, place: null });

// v2.7 kept every v2.6 rule except its gore line (now excessive gore) and its B1 carve-out (now all mild fright), so the
// phrases below are v2.6's that still stand; the version and anchor are checked in moderation_code_v27.test.mjs.
test('Code v2.6 combat rules are in the Code shipped, and it is anchored', () => {
  assert.ok(Number(POLICY_VERSION) >= 2.6);
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  for (const words of [
    '**B3. Combat beyond its limits.**',
    'a real conflict that ended at least 25 years ago',
    'No attack on civilians, children, a school, a place of worship, a hospital or a crowd, even in fiction',
    'no torture and no execution as play',
    'In a game with combat (B3), any random reward container',
    'whether bought or earned in play, and any wager',
    'A game with combat (B3) is rated at least `teen_13`, and at least `mature_17` when its violence is realistic or shows blood',
    'zombies and other enemies the visitor can see and fight or outrun',
    'combat between combatants in a game that meets B3',
  ]) assert.ok(GUARDIAN_POLICY_TEXT.includes(words), words);
});

test('Code v2.6: combat in fiction that breaks no B3 limit is listed, rated at least teen_13', () => {
  const d = decide();
  assert.equal(d.outcome, 'approve');
  assert.equal(d.rating, 'teen_13');
});

test('Code v2.6 Part V: realistic violence or blood is at least mature_17, whatever the bands said', () => {
  assert.equal(decide({}, { descriptors: ['realistic violence'] }).rating, 'mature_17');
  assert.equal(decide({}, { descriptors: ['fantasy violence', 'blood'] }).rating, 'mature_17');
  assert.equal(decide({ rating: 'adult_18' }, { descriptors: ['fantasy violence'] }).rating, 'adult_18', 'a floor never lowers a rating');
  assert.equal(decide({}, { descriptors: [] }).rating, 'all_ages');
  assert.deepEqual(['fantasy violence', 'realistic violence', 'blood', 'language', null].map(x => violenceFloor(x ? [x] : [])), ['teen_13', 'mature_17', 'mature_17', null, null]);
});

test('Code v2.6: combat beyond B3 is not listed, with the combat copy', () => {
  const d = decide({}, { articles: [cite('B3')] });
  assert.equal(d.outcome, 'reject');
  const view = authorView({ status: 'rejected', decision: d }, { is_public: true });
  assert.deepEqual(view.reasons.map(r => r.title), ['Combat beyond its limits (Code B3)']);
  assert.match(view.reasons[0].why, /fiction or in history/);
  assert.match(view.reasons[0].why, /25 years/);
  // A system citation of B3 (Jev's harm_scored at its threshold) rejects too: the question only asks about B3's limits.
  assert.equal(decide({ articles: { system: [{ article: 'B3', key: 'harm_scored', p: 0.9 }], signals: [] } }).outcome, 'reject');
});

test('Code v2.6: a loot box or a wager in a game with combat is not yet, naming it', () => {
  const d = decide({}, { articles: [cite('B6')] });
  assert.equal(d.outcome, 'changes_requested');
  assert.deepEqual(d.changes, ['not_yet_B6']);
  assert.match(AUTHOR_COPY.not_yet_B6.why, /loot box, crate, spin or bet/);
  assert.match(AUTHOR_COPY.code_B6.why, /bought or earned, and no betting/);
});

test('Code v2.6: the Jev questions draw the Code\'s lines, so combat the Code allows raises no citation', () => {
  const q = buildJevQuestions();
  const text = key => JSON.stringify(q[key]);
  const harm = text('harm_scored');
  for (const words of ['fiction or history', '25 years', 'civilians', 'children', 'torture or execution', 'excessive gore', 'Invented enemies and historical armies']) assert.ok(harm.includes(words), `harm_scored: ${words}`);
  assert.ok(text('pursuing_entity').includes('enemies the visitor can see and fight or outrun'), 'pursuing_entity');
  assert.ok(text('playable_victims').includes('combat between combatants'), 'playable_victims');
  assert.ok(text('b6b_paid_chance').includes('even when earned in play'), 'b6b_paid_chance');
});
