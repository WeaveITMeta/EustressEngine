// The eustress-judge skill (.agents/skills/eustress-judge) carries a generated copy of the Guardian Code and of the author
// copy Studio shows, and runs the live verdict rules. These tests fail when a copy drifts from its source, and pin that the
// skill's verdict step renders a real review's findings exactly as the live rules decide them.
// A deploy export without .agents/ skips them (the skill is not part of the Worker).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { CANONICAL, CODE_SOURCE, drift, generated } from '../scripts/sync-judge-skill.mjs';
import { AUTHOR_COPY, POLICY_VERSION } from '../src/moderation.mjs';

const present = existsSync(CANONICAL);
const skip = present ? false : 'the eustress-judge skill is not in this checkout';

test('every copy of the eustress-judge skill matches its sources (run npm run sync-judge-skill)', { skip }, () => {
  assert.deepEqual(drift(), []);
});

test('the skill carries the whole Code and every finding Studio can show', { skip }, () => {
  const gen = generated();
  const code = readFileSync(CODE_SOURCE, 'utf8').replace(/\r\n/g, '\n');
  assert.ok(gen['references/code.md'].endsWith(code), 'code.md is the Code, word for word');
  const findings = JSON.parse(gen['references/findings.json']);
  assert.equal(findings.policy_version, POLICY_VERSION);
  for (const key of Object.keys(AUTHOR_COPY).filter(k => k !== 'requested_change')) {
    assert.deepEqual(findings.author_copy[key], AUTHOR_COPY[key], key);
  }
});

// Pong (listing 9a6951e9), reviewed 2026-11-14: changes requested with Lesson (Code T3), Qualification Statement missing and
// Label your fiction, in that order. A judge answer with those scores must render those findings through the skill.
test('the skill verdict step renders Pong\'s real findings from the live rules', { skip }, async () => {
  const { emulate } = await import(pathToFileURL(join(CANONICAL, 'scripts', 'verdict.mjs')).href);
  const evidence = 'ServerScriptService/PongServer moves the ball part each frame';
  const judge = {
    verdict: 'publish', articles: [],
    tests: {
      build_quality: { score: 2, evidence }, objective: { score: 2, evidence },
      lesson: { score: 1, evidence: 'No string in the Space names a skill; only score labels' },
      real_world: { score: 0, evidence: 'Ball speed is a fixed constant in PongServer, no real rules or data' },
    },
    statement_check: 'missing', rating_floor: 'all_ages', descriptors: [], is_game: true, is_fiction: true, featured_candidate: false,
    policy_version: POLICY_VERSION, policy_hash: 'sha256:test', spatial_evidence: ['capture-0 shows two paddles and a ball on a table'],
    rationale: 'T3 at 1, T4 at 0, unlabelled fiction.', confidence: 0.85, flagged_for_review: false,
    suggested_edit_to_publish: 'Name the lesson the play teaches and show it on screen.',
    suggested_tags: ['pong', 'arcade'], suggested_statement: null, signal_answers: [],
  };
  const r = await emulate({ listing: { name: 'Pong', description: 'A simple game of pong.', tags: [] }, judgeText: JSON.stringify(judge) });
  assert.equal(r.view.status, 'changes_requested');
  assert.deepEqual(r.view.reasons.map(x => x.category), ['test_lesson', 'statement_missing', 'fiction_label_missing']);
  assert.deepEqual(r.view.reasons.map(x => x.title), ['Lesson (Code T3)', 'Qualification Statement missing (Code Part III)', 'Label your fiction (Code Part III)']);
  assert.equal(r.view.reasons[0].what_to_change, AUTHOR_COPY.test_lesson.what_to_change);
});

// The Super Station (listing c9caf0e2), listed 2026-11-11: Category Tool, every test at 2, statement matching.
test('the skill verdict step lists a Space whose tests are all 2 and whose statement matches', { skip }, async () => {
  const { emulate } = await import(pathToFileURL(join(CANONICAL, 'scripts', 'verdict.mjs')).href);
  const evidence = 'SoulService/StationMind scores 260 word cubes against the typed question';
  const judge = {
    verdict: 'publish', articles: [],
    tests: Object.fromEntries(['build_quality', 'objective', 'lesson', 'real_world'].map(k => [k, { score: 2, evidence }])),
    statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false,
    policy_version: POLICY_VERSION, policy_hash: 'sha256:test', spatial_evidence: ['capture-0 shows 25 rings of word cubes around a sun'],
    rationale: 'A working tool on the visitor\'s own question.', confidence: 0.8, flagged_for_review: false,
    suggested_edit_to_publish: null, suggested_tags: [], suggested_statement: null, signal_answers: [],
  };
  const description = 'A tool.\nCategory: Tool\nObjective: Type a question.\nLesson: How a question breaks down.\nReal-world link: The author\'s own knowledge map.';
  const r = await emulate({ listing: { name: 'The Super Station', description, tags: ['knowledge-map'] }, judgeText: JSON.stringify(judge) });
  assert.equal(r.view.status, 'listed');
  assert.deepEqual(r.view.reasons, []);
});
