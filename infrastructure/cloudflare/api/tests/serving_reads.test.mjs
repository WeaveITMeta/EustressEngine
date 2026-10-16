// A new route that decides whether to serve or list a listing has to read it
// through the legal lock (withLegalLock in moderation.mjs). The check is
// structural: any top-level function that asks isListable() or canServe() must
// also overlay the lock, or load the listing through a function that does.
// If this fails for your function: read the listing with simWithLegalLock(env,
// sim) (one listing) or withLegalLockFrom(sim, await legalLockedIds(env)) (a
// page of them) before you ask.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const src = path.resolve(import.meta.dirname, '..', 'src');
const OVERLAY = /\b(simWithLegalLock|simForServing|withLegalLockFrom|withLegalLock|loadSimulation)\(/;
// The definitions themselves, and the moderation module that owns the gates.
const SKIP_FILES = new Set(['moderation.mjs']);

function functionsOf(text) {
  const heads = [...text.matchAll(/^(?:export\s+)?(?:async\s+)?function\s+(\w+)/gm)];
  return heads.map((m, i) => ({ name: m[1], body: text.slice(m.index, i + 1 < heads.length ? heads[i + 1].index : text.length) }));
}

test('every function that asks isListable or canServe reads the listing through the legal lock', () => {
  const missing = [];
  let checked = 0;
  for (const file of fs.readdirSync(src).filter(f => /\.(mjs|js)$/.test(f) && !SKIP_FILES.has(f) && !f.endsWith('.bak'))) {
    for (const fn of functionsOf(fs.readFileSync(path.join(src, file), 'utf8'))) {
      if (!/\b(isListable|canServe)\(/.test(fn.body)) continue;
      checked += 1;
      if (!OVERLAY.test(fn.body)) missing.push(`${file}: ${fn.name}`);
    }
  }
  assert.ok(checked >= 8, `the scan found the serving functions (${checked})`);
  assert.deepEqual(missing, [], `these ask isListable/canServe without the legal lock overlay:\n  ${missing.join('\n  ')}`);
});
