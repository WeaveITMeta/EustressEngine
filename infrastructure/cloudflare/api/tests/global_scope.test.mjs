// The Workers runtime refuses to load a Worker that does asynchronous I/O (fetch, connect), sets a timer, or makes random values
// in global scope: the deploy fails with error 10021 and the old version keeps running. Plain Node allows all of these at import,
// so the rest of the suite cannot see the mistake. This test imports the real Worker with each of those operations turned into a
// throw for the duration of the import, and fails naming the first one that ran.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

test('the Worker does no I/O, timers or random values in global scope (Cloudflare refuses the upload otherwise)', async () => {
  const src = path.resolve(import.meta.dirname, '..', 'src');
  const tmp = fs.mkdtempSync(path.join(path.resolve(src, '..'), '.test-global-scope-'));
  fs.cpSync(src, tmp, { recursive: true });
  fs.writeFileSync(path.join(tmp, 'package.json'), '{"type":"module"}');
  fs.writeFileSync(path.join(tmp, 'cloudflare_email_stub.mjs'), 'export class EmailMessage { constructor() {} }\n');
  const entry = path.join(tmp, 'index.js');
  fs.writeFileSync(entry, fs.readFileSync(entry, 'utf8').replaceAll("'cloudflare:email'", "'./cloudflare_email_stub.mjs'"));

  const hits = [];
  const trap = (name) => () => { const e = new Error(`${name} called in global scope`); hits.push(e); throw e; };
  const saved = {
    setTimeout: globalThis.setTimeout, setInterval: globalThis.setInterval, fetch: globalThis.fetch,
    getRandomValues: crypto.getRandomValues, randomUUID: crypto.randomUUID,
  };
  globalThis.setTimeout = trap('setTimeout');
  globalThis.setInterval = trap('setInterval');
  globalThis.fetch = trap('fetch');
  crypto.getRandomValues = trap('crypto.getRandomValues');
  crypto.randomUUID = trap('crypto.randomUUID');
  let threw = null;
  try {
    await import(pathToFileURL(entry).href);
  } catch (e) {
    threw = e;
  } finally {
    globalThis.setTimeout = saved.setTimeout;
    globalThis.setInterval = saved.setInterval;
    globalThis.fetch = saved.fetch;
    crypto.getRandomValues = saved.getRandomValues;
    crypto.randomUUID = saved.randomUUID;
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  const first = hits[0] || threw;
  assert.equal(hits.length, 0, first ? `${first.message}\n${String(first.stack).split('\n').slice(1, 4).join('\n')}` : '');
  assert.equal(threw, null, threw ? `the Worker failed to import: ${threw.message}` : '');
});
