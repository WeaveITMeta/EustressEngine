// The review's one automatic pass over a listing's wording (McKale, 2026-11-14): docs/moderation/AUTO_APPLY_LISTING_FIXES.md.
// These tests pin the pure parts: which findings the pass fixes, where every word comes from, what it never touches, and what
// the author is shown. The run (one pass, the review again, the author's edit winning, the legal lane) is pinned in
// tests/automated_review.test.mjs under "auto-fix", and the commit guard in tests/world.test.mjs.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { autoFixPlan, autoFixView, setStatementLines, extractQualificationStatement, AUTO_FIX_CODES, LISTING_DESCRIPTION_MAX } from '../src/moderation.mjs';

const DRAFT = { category: 'Simulation', objective: 'Steer the barge through the lock.', lesson: 'How a lock raises a boat.', real_world_link: 'Models the Panama Canal locks from published dimensions.' };
const judge = (over = {}) => ({ malformed: false, suggested_statement: DRAFT, suggested_tags: ['canal', 'engineering'], is_fiction: false, ...over });
const sim = (over = {}) => ({ name: 'Lock Keeper', description: 'Run a canal lock.', tags: ['boats'], ...over });
const final = (changes, over = {}) => ({ outcome: 'changes_requested', lane: 'quality', changes, is_fiction: false, ...over });

test('setStatementLines writes a line in place of the first one with its label, drops repeats, and adds the rest at the end', () => {
  const text = 'Intro line.\nCategory: Toy\nObjective: Old objective.\nNotes here.\ncategory: Puzzle';
  const out = setStatementLines(text, { category: 'Simulation', lesson: 'L.' });
  assert.equal(out, 'Intro line.\nCategory: Simulation\nObjective: Old objective.\nNotes here.\n\nLesson: L.');
  assert.equal(extractQualificationStatement(out).category, 'Simulation');
  // The author's own line breaks are kept.
  assert.equal(setStatementLines('A.\r\nB.\r\n', { lesson: 'L.' }), 'A.\r\nB.\r\n\r\nLesson: L.');
  assert.equal(setStatementLines('', { category: 'Game' }), 'Category: Game');
});

test('a missing statement is written from the judge\'s draft, keeping every line the author wrote that counts', () => {
  const description = 'Run a canal lock.\nObjective: Fill the chamber and open the gate.';
  const plan = autoFixPlan({ sim: sim({ description }), final: final(['statement_missing']), judge: judge() });
  assert.deepEqual(plan.applied.map(a => a.code), ['statement_missing']);
  assert.equal(plan.resubmit, true);
  const st = extractQualificationStatement(plan.description);
  assert.deepEqual([st.present, st.objective, st.lesson, st.category], [true, 'Fill the chamber and open the gate.', DRAFT.lesson, DRAFT.category]);
  assert.ok(plan.description.startsWith(description), 'the author\'s text is untouched');
  assert.equal(plan.tags, null, 'the tags are not touched');
  assert.match(plan.applied[0].detail, /Category, Lesson, Real-world link/);
});

test('an unclear statement is rewritten whole from the draft; with no draft both stay the author\'s', () => {
  const description = 'Run a canal lock.\nCategory: Game\nObjective: Win.\nLesson: Fun.\nReal-world link: None.';
  const plan = autoFixPlan({ sim: sim({ description }), final: final(['statement_unclear']), judge: judge() });
  const st = extractQualificationStatement(plan.description);
  assert.deepEqual([st.category, st.objective, st.real_world_link], [DRAFT.category, DRAFT.objective, DRAFT.real_world_link]);
  const none = autoFixPlan({ sim: sim({ description }), final: final(['statement_unclear']), judge: judge({ suggested_statement: null }) });
  assert.deepEqual([none.applied, none.left, none.resubmit], [[], ['statement_unclear'], false]);
  // A draft that does not pass the author filter (a line about the review itself) is no draft.
  const unsafe = autoFixPlan({ sim: sim(), final: final(['statement_missing']), judge: judge({ suggested_statement: { ...DRAFT, lesson: 'The judge liked it.' } }) });
  assert.deepEqual(unsafe.left, ['statement_missing']);
});

test('the fiction label is the judge\'s genre tag, fiction, and the Category line; without a genre tag it stays the author\'s', () => {
  const plan = autoFixPlan({ sim: sim(), final: final(['fiction_label_missing', 'statement_missing'], { is_fiction: true }), judge: judge({ is_fiction: true, suggested_tags: ['canal', 'steampunk'] }) });
  assert.deepEqual(plan.tags, ['fiction', 'steampunk', 'boats']);
  assert.equal(plan.tags_source, 'author', 'the author\'s tags with the label stay the author\'s');
  assert.equal(extractQualificationStatement(plan.description).category, 'Fiction', 'the label wins over the draft\'s category');
  assert.equal(plan.resubmit, true);
  const noGenre = autoFixPlan({ sim: sim(), final: final(['fiction_label_missing'], { is_fiction: true }), judge: judge({ is_fiction: true, suggested_tags: ['canal'] }) });
  assert.deepEqual([noGenre.applied, noGenre.left, noGenre.description], [[], ['fiction_label_missing'], 'Run a canal lock.']);
});

test('misfit tags are replaced by the review\'s own choice, which the next review chooses again', () => {
  const plan = autoFixPlan({ sim: sim(), final: final(['not_yet_tags']), judge: judge() });
  assert.deepEqual([plan.tags, plan.tags_source, plan.description, plan.resubmit], [['canal', 'engineering'], 'judge', 'Run a canal lock.', true]);
  assert.deepEqual(autoFixPlan({ sim: sim(), final: final(['not_yet_tags']), judge: judge({ suggested_tags: [] }) }).left, ['not_yet_tags']);
});

test('anything about the Space stays the author\'s, and the review runs again only when nothing is left', () => {
  assert.equal(autoFixPlan({ sim: sim(), final: final(['test_lesson', 'confirm_real_world', 'statement_contradicted']), judge: judge() }), null);
  const plan = autoFixPlan({ sim: sim(), final: final(['test_lesson', 'statement_missing']), judge: judge() });
  assert.deepEqual([plan.left, plan.resubmit], [['test_lesson'], false]);
  assert.ok(!('name' in plan), 'the plan has no title to write');
  assert.deepEqual(AUTO_FIX_CODES, ['statement_missing', 'statement_unclear', 'fiction_label_missing', 'not_yet_tags']);
});

test('a description that would pass the listing limit is not written; a tag fix still is', () => {
  const long = 'x'.repeat(LISTING_DESCRIPTION_MAX - 20);
  const plan = autoFixPlan({ sim: sim({ description: long }), final: final(['statement_missing', 'not_yet_tags']), judge: judge() });
  assert.deepEqual([plan.description, plan.tags, plan.left, plan.resubmit], [long, ['canal', 'engineering'], ['statement_missing'], false]);
  assert.deepEqual(plan.applied.map(a => a.code), ['not_yet_tags']);
});

test('the author view shows a pass only with the review it belongs to, never on a hold, and only fixed copy', () => {
  const auto_fix = { pass: 1, at: '2026-11-14T13:00:00Z', state: 'resubmitted', applied: [{ code: 'statement_missing', detail: 'Added the Qualification Statement lines Lesson from what the review saw in your Space.' }, { code: 'bogus', detail: 'x' }], left: [], before: { description: 'a', tags: [] }, after: { description: 'b', tags: ['c'] } };
  const view = autoFixView({ status: 'pending', auto_fix }, 'in_review');
  assert.deepEqual(view.applied.map(a => [a.category, a.title]), [['statement_missing', 'Added your Qualification Statement']]);
  assert.deepEqual([view.reviewing_again, view.description_before, view.description_after, view.tags_after], [true, 'a', 'b', ['c']]);
  assert.equal(autoFixView({ status: 'held', auto_fix }, 'in_review'), null, 'a hold reads like any hold');
  assert.equal(autoFixView({ status: 'quarantined', auto_fix }, 'in_review'), null);
  assert.equal(autoFixView({ status: 'changes_requested', auto_fix: { ...auto_fix, state: 'skipped' } }, 'changes_requested'), null);
  assert.equal(autoFixView({ status: 'approved', auto_fix }, 'listed').reviewing_again, false);
});
