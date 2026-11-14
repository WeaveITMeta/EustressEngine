// Guardian Code v2.7 (McKale, 2026-11-14: "Allow passive income mechanisms, allow gore just not excessive, allow mild fright
// just not excessively spammy jump scares. Allow simple mind games like pong. recognize it teaches reaction time and spatial
// awareness."): B1 carries only fright beyond mild, B3(e) only excessive gore, B5 no longer counts passive income, and a game's
// lesson and real-world connection can be the skill its play exercises (T3, T4(i)). The Jev questions, the listing's horror
// words and the author copy draw the same lines, so a system citation never refuses what the Code allows.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  decideFinal, decideTriage, deterministicSignals, buildJevQuestions, extractQualificationStatement,
  AUTHOR_COPY, CODE_TESTS, POLICY_VERSION, POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

test('Code v2.7 is the Code shipped, cited and anchored, with its four changes', () => {
  // Later Codes keep v2.7's rules; the exact version is pinned by the Code in force (link_review.test.mjs for v2.8, and the hash pins).
  assert.ok(Number(POLICY_VERSION) >= 2.7);
  assert.match(GUARDIAN_POLICY_TEXT, /^# Eustress Guardian Code v2\.\d+\n\*\*Version:\*\* 2\.\d+\n/);
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  for (const words of [
    '**B1. Fright beyond mild.** *Mild fright is fine; fear is never the product.*',
    'more than one in any two minutes of play, or startles as the main thing the Space delivers',
    'which the visitor can neither fight nor escape',
    'the listing or its tags sell it as horror or a jumpscare game',
    'Not B1 (mild fright): a spooky or dark setting, eerie sound, a haunted house, monsters, zombies',
    '(e) **No excessive gore.** Blood, hit effects and brief, stylised pieces flying from a kill are allowed and rated (Part V).',
    'body parts or remains left in the scene, harm to bodies after death, or gore as the reward or the point',
    'grows only by clicking, repeating an action that cannot fail, or resetting for a multiplier',
    'Not B5: passive income from something the visitor built, bought with what they earned, or chose to invest in',
    'For a game, the lesson can be the skill its play exercises: reaction time, hand-eye coordination, spatial awareness',
    'A simple skill game such as Pong practises reaction time and spatial awareness; that lesson is present (2)',
    '(i) **Real skill through play**',
    'A simple skill game such as Pong scores 2',
    'A fiction game may instead be scored by T4(i)',
  ]) assert.ok(GUARDIAN_POLICY_TEXT.includes(words), words);
  // What v2.7 took out stays out.
  for (const gone of ['offline earnings, refilling energy', 'No graphic gore', 'call it horror, scary, creepy, haunted']) {
    assert.ok(!GUARDIAN_POLICY_TEXT.includes(gone), gone);
  }
});

test('Code v2.7 B1(c): only "horror" and "jumpscare" in the listing are signals; spooky words are mild fright', () => {
  assert.deepEqual(deterministicSignals({ name: 'Haunted Mill', description: 'A creepy, scary, spooky walk.' }, null).horror_terms, []);
  assert.deepEqual(deterministicSignals({ name: 'Mill', tags: ['horror'] }, null).horror_terms, ['horror']);
  assert.deepEqual(deterministicSignals({ description: 'A jump scare in every room.' }, null).horror_terms, ['jump scare']);
  const quiet = decideTriage({ signals: deterministicSignals({ name: 'Haunted Mill' }, null), jev: null });
  assert.deepEqual(quiet.articles.signals, []);
});

test('Code v2.7: the Jev questions ask about fright beyond mild, excessive gore and loops without passive income', () => {
  const q = buildJevQuestions();
  const text = key => JSON.stringify(q[key]);
  assert.ok(text('scripted_startle').includes('more than once in any two minutes'), 'scripted_startle');
  assert.ok(text('scripted_startle').includes('An occasional startle is not this'), 'scripted_startle exemption');
  assert.ok(text('pursuing_entity').includes('who can neither fight nor escape'), 'pursuing_entity');
  assert.ok(text('horror_framing').includes('sell the Space as horror or a jumpscare game'), 'horror_framing');
  assert.ok(!/scary|creepy|haunted/.test(text('horror_framing')), 'horror_framing drops the mild words');
  assert.ok(text('harm_scored').includes('excessive gore'), 'harm_scored');
  assert.ok(!text('harm_scored').includes('graphic gore'), 'harm_scored drops graphic gore');
  assert.ok(text('accumulation_loop').includes('Passive income from what the visitor built or chose'), 'accumulation_loop');
  assert.ok(!text('accumulation_loop').includes('by waiting'), 'accumulation_loop drops waiting');
  assert.ok(text('return_reward').includes('Passive income, offline included, is not this'), 'return_reward');
  assert.ok(JSON.stringify(q.fear_level).includes('an occasional startle'), 'fear_level: an occasional startle is mild');
});

test('Code v2.7: the author copy says what is allowed now', () => {
  assert.equal(AUTHOR_COPY.code_B1.title, 'Fright beyond mild (Code B1)');
  assert.match(AUTHOR_COPY.code_B1.why, /mild fright is fine/);
  assert.match(AUTHOR_COPY.code_B1.what_to_change, /an occasional startle are fine/);
  assert.match(AUTHOR_COPY.code_B3.why, /no excessive gore \(blood, hit effects and brief pieces from a kill are fine/);
  assert.match(AUTHOR_COPY.code_B5.why, /Passive income from what the visitor built or chose is fine/);
  assert.match(AUTHOR_COPY.not_yet_B5.why, /Passive income is fine/);
  assert.match(AUTHOR_COPY.test_lesson.why, /reaction time or spatial awareness/);
  assert.match(AUTHOR_COPY.test_real_world.why, /skill its play exercises/);
});

test('Code v2.7: a simple skill game with its statement and every test at 2 is listed', () => {
  const statement = extractQualificationStatement([
    'Category: Game',
    'Objective: Return the ball with your paddle and reach 7 points first.',
    'Lesson: Reaction time and spatial awareness: reading the ball off the walls and meeting it in time.',
    'Real-world link: Practises reaction time and spatial awareness through play.',
  ].join('\n'));
  const scores = Object.fromEntries(CODE_TESTS.map(k => [k, { score: 2, evidence: `${k}: PongServer.server.luau bounceOff and the seven-segment score in capture-0` }]));
  const judge = {
    malformed: false, verdict: 'publish', articles: [], tests: scores, statement_check: 'matches', rating_floor: 'all_ages',
    descriptors: ['players interact'], is_game: true, is_fiction: false, featured_candidate: false,
    spatial_evidence: ['a table with two paddles and a ball between two walls'], rationale: 'A skill game, T3 and T4(i).', confidence: 0.85,
    flagged_for_review: false, suggested_edit_to_publish: null, second_opinion: true,
  };
  const triage = {
    outcome: 'judge', lane: 'ok', reasons: [], content_kind: 'interactive_experience', rating: null, articles: { system: [], signals: [] },
    statement, scripts_truncated: false, game_mechanics: 0.9, lawful_decision_training: 0.01,
    questionnaire: { answered: false, band: null, descriptors: [] }, fiction_p: 0.05, fiction_labelled: false,
    account: { attested_properties: [], verified_business: false, payment_connection: false },
  };
  const d = decideFinal({ triage, judge });
  assert.equal(d.outcome, 'approve');
  assert.equal(d.rating, 'all_ages');
});
