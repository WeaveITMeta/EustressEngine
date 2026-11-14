// The guard against a Studio that still holds listing text the listing replaced (src/listing_guard.mjs): the review's
// automatic fix, the link review and the website edit each record what they replaced; a commit that sends it back keeps
// the listing's text for that field.

import test from 'node:test';
import assert from 'node:assert/strict';
import { guardReplacedText, staleText, staleTags, supersededAfter, SUPERSEDED_MAX } from '../src/listing_guard.mjs';

test('a replaced text is stale only while the listing holds another; no guard, nothing is stale', () => {
  const sim = { name: 'B', description: 'New.', tags: ['b'] };
  assert.equal(staleText(sim, 'description', 'Old.'), false, 'no guard');
  guardReplacedText(sim, { old: { name: 'A', description: 'Old.', tags: ['a'] }, by: 'author_web', at: 'T' });
  assert.equal(staleText(sim, 'description', '  Old. '), true);
  assert.equal(staleText(sim, 'name', 'A'), true);
  assert.equal(staleText(sim, 'description', 'New.'), false, 'the listing\'s own text is never stale');
  assert.equal(staleText(sim, 'description', 'Other.'), false, 'text never replaced is the author\'s');
  assert.equal(staleText(sim, 'description', undefined), false);
  assert.equal(staleTags(sim, ['a'], sim.tags), true);
  assert.equal(staleTags(sim, ['b'], sim.tags), false);
  assert.equal(staleTags(sim, ['c'], sim.tags), false);
  // A guard another writer left naming the listing's own text never makes that text stale.
  const same = { description: 'Same.', tags: ['t'], listing_fix: { before: { description: 'Same.', tags: ['t'] }, superseded: { description: ['Same.'] } } };
  assert.equal(staleText(same, 'description', ' Same. '), false);
  assert.equal(staleTags(same, ['t'], same.tags), false);
});

test('a guard written over another keeps every text the older one guarded, so a Studio further behind is still caught', () => {
  const sim = { name: 'N', description: 'Fixed.', tags: ['x'] };
  // The review's automatic fix: Studio has "Orig.".
  guardReplacedText(sim, { old: { description: 'Orig.', tags: ['old'] }, after: { description: 'Fixed.' }, by: 'auto_fix', at: 'T1' });
  // Then the author edits on the website.
  sim.description = 'Edited.';
  guardReplacedText(sim, { old: { description: 'Fixed.' }, by: 'author_web', at: 'T2' });
  assert.deepEqual([sim.listing_fix.before.description, sim.listing_fix.superseded.description], ['Fixed.', ['Orig.']]);
  assert.deepEqual(sim.listing_fix.before.tags, ['old'], 'the fix\'s tag guard carries on');
  for (const behind of ['Orig.', 'Fixed.']) assert.equal(staleText(sim, 'description', behind), true, behind);
  // The author writes the original back on the website: it is the listing's text now, and no longer stale.
  sim.description = 'Orig.';
  guardReplacedText(sim, { old: { description: 'Edited.' }, by: 'author_web', at: 'T3' });
  assert.equal(staleText(sim, 'description', 'Orig.'), false);
  assert.ok(!sim.listing_fix.superseded.description.includes('Orig.'));
  assert.equal(staleText(sim, 'description', 'Edited.'), true);
});

test('superseded keeps the newest few, once each', () => {
  let prev = null;
  for (let i = 0; i < SUPERSEDED_MAX + 4; i++) prev = { before: { description: `t${i}` }, superseded: supersededAfter(prev, { description: 'now' }) };
  const kept = supersededAfter(prev, { description: 'now' }).description;
  assert.equal(kept.length, SUPERSEDED_MAX);
  assert.equal(kept.at(-1), `t${SUPERSEDED_MAX + 3}`);
  assert.deepEqual(supersededAfter({ before: { description: 'a' }, superseded: { description: ['a', 'b', 'a'] } }, { description: 'z' }).description, ['b', 'a']);
});
