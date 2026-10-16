// What the Worker asks of a deployment, read from wrangler.toml: a binding that is
// missing is not an error in the code that uses it, it is a feature that quietly
// switches off. The value check on a key replacement skips a source it does not
// have, so a production without WALLETS or DELTA_ACCOUNTS would treat every
// account as holding nothing and freeze it for 72 hours instead of 7 days.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const text = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'wrangler.toml'), 'utf8').split('\r\n').join('\n');
const stagingAt = text.indexOf('[env.staging]');
assert.ok(stagingAt > 0, 'wrangler.toml has a staging environment');
const environments = { production: text.slice(0, stagingAt), staging: text.slice(stagingAt) };

/// The names a deployment binds: KV and R2 by `binding`, Durable Objects by `name`.
const bound = (section) => new Set([...section.matchAll(/^(?:binding|name) = "([A-Z_]+)"/gm)].map((m) => m[1]));

// The sources the value check reads, the objects the key replacement needs, the live registry and index, the claims a release notice is
// made under (SCENES: R2 is where a create-if-absent is atomic), and the limiter the account writes are rate limited by. A limiter that is
// not bound is skipped silently by the routes that use it (they ask `if (env.AUTH_RATE_LIMITER)`), so it is required here.
const REQUIRED = ['USERS', 'SOCIAL', 'PAYOUTS', 'WALLETS', 'COMMERCE_HUBS', 'DELTA_ACCOUNTS', 'ACCOUNT_AUTH', 'LIVE_HOSTS', 'LIVE_INDEX', 'SCENES', 'AUTH_RATE_LIMITER'];

for (const [name, section] of Object.entries(environments)) {
  test(`${name} binds every source the value check and the key replacement read`, () => {
    const names = bound(section);
    for (const required of REQUIRED) assert.ok(names.has(required), `${name} does not bind ${required}`);
  });
}

test('the migrations create every Durable Object class a binding names', () => {
  for (const [name, section] of Object.entries(environments)) {
    assert.match(section, /new_sqlite_classes = \[[^\]]*"AccountAuth"/, `${name} has a migration that creates AccountAuth`);
    assert.match(section, /new_sqlite_classes = \[[^\]]*"LiveIndex"/, `${name} has a migration that creates LiveIndex`);
  }
});

test('the live index is bound and migrated once under its own tag, with the class the Worker exports', () => {
  for (const [name, section] of Object.entries(environments)) {
    assert.match(section, /name = "LIVE_INDEX"\nclass_name = "LiveIndex"/, `${name} binds LIVE_INDEX to the class LiveIndex`);
    assert.equal([...section.matchAll(/tag = "v7-live-index"\nnew_sqlite_classes = \["LiveIndex"\]/g)].length, 1, `${name} has the v7-live-index migration once`);
    const tags = [...section.matchAll(/^tag = "([^"]+)"/gm)].map((m) => m[1]);
    assert.equal(new Set(tags).size, tags.length, `${name} reuses no migration tag`);
  }
  const index = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'src', 'index.js'), 'utf8');
  assert.match(index, /export \{ LiveIndex \} from '\.\/live_index\.mjs';/, 'index.js exports LiveIndex next to LiveHost');
});
