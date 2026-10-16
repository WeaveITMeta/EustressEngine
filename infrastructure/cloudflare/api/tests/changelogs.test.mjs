// The author's change logs (docs/architecture/PUBLISH_CHANGELOG.md), against the
// real Worker with fake storage: only the author posts, for the version the
// listing is at; only the author and an admin read, and not under a hold;
// nobody else learns the logs exist; a log is never overwritten.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = { author: mintToken(AUTHOR), other: mintToken(OTHER), admin: mintToken(ADMIN) };
const HASH = 'ab'.repeat(32);
const MD = '# Box Head: version 7\n\n## Listing\n\n- Max players: 10 → 4\n';

async function seed({ moderation = { status: 'approved' }, version = 7, hash = HASH } = {}) {
  const env = fakeEnv();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'World', author_id: AUTHOR, is_public: true, moderation, version, world: { publish_hash: hash }, updated_at: '2026-01-01T00:00:00.000Z' }));
  return env;
}
const setListing = async (env, patch) => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)), ...patch }));

async function call(env, method, path, who, body) {
  const headers = { ...(who ? { Authorization: `Bearer ${TOKEN[who]}` } : {}), ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}) };
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, json };
}

const post = (env, body, who = 'author') => call(env, 'POST', `/api/simulations/${SIM}/changelogs`, who, body);
const good = { version: 7, publish_hash: `blake3:${HASH}`, markdown: MD };
const listOf = (env, who = 'author') => call(env, 'GET', `/api/simulations/${SIM}/changelogs`, who);

test('the author stores a log, reads the list newest first, and downloads a log as a text attachment', async () => {
  const env = await seed();
  const first = await post(env, good);
  assert.equal(first.status, 201, first.text);
  assert.match(first.json.id, /^0000000007-[a-f0-9]{8}-0$/);
  await setListing(env, { version: 8, world: { publish_hash: 'cd'.repeat(32) } });
  const second = await post(env, { version: 8, publish_hash: 'cd'.repeat(32), markdown: '# v8\n' });
  assert.equal(second.status, 201);
  const list = await listOf(env);
  assert.equal(list.status, 200);
  assert.deepEqual(list.json.map(r => r.version), [8, 7], 'newest first');
  assert.deepEqual(Object.keys(list.json[1]).sort(), ['bytes', 'created_at', 'id', 'publish_hash', 'version']);
  assert.equal(list.json[1].bytes, new TextEncoder().encode(MD).length);
  assert.match(list.json[1].created_at, /^\d{4}-\d\d-\d\dT[\d:.]+Z$/, 'ISO 8601 UTC');
  assert.equal(list.headers.get('Cache-Control'), 'private, no-store');
  const one = await call(env, 'GET', `/api/simulations/${SIM}/changelogs/${first.json.id}`, 'author');
  assert.equal(one.status, 200);
  assert.equal(one.text, MD);
  assert.equal(one.headers.get('Content-Type'), 'text/markdown; charset=utf-8');
  assert.equal(one.headers.get('Content-Disposition'), 'attachment; filename="CHANGELOG-v7.md"');
  assert.equal(one.headers.get('X-Content-Type-Options'), 'nosniff');
  assert.match(one.headers.get('Content-Security-Policy'), /sandbox/);
});

test('an admin may read; anyone else gets one 404, byte for byte the answer for a log that is not there', async () => {
  const env = await seed();
  const { json } = await post(env, good);
  const paths = [`/api/simulations/${SIM}/changelogs`, `/api/simulations/${SIM}/changelogs/${json.id}`];
  for (const p of paths) assert.equal((await call(env, 'GET', p, 'admin')).status, 200);
  const missing = await call(env, 'GET', `/api/simulations/${SIM}/changelogs/9999999999-deadbeef-0`, 'author');
  assert.equal(missing.status, 404);
  for (const who of [undefined, 'other']) {
    for (const p of paths) {
      const r = await call(env, 'GET', p, who);
      assert.equal(r.status, 404, `${who} ${p}`);
      assert.equal(r.text, missing.text, 'the same bytes');
      assert.equal(r.headers.get('Content-Type'), missing.headers.get('Content-Type'));
    }
  }
  const absent = await call(env, 'GET', `/api/simulations/bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb/changelogs`, 'author');
  assert.equal(absent.status, 404);
  assert.equal(absent.text, missing.text, 'a listing that does not exist reads the same');
  assert.equal((await post(env, good, 'other')).status, 403);
  assert.equal((await post(env, good, 'admin')).status, 403, 'only the author posts');
  assert.equal((await post(env, good, null)).status, 401);
});

test('anything the contract does not allow is a coded refusal, and nothing is stored', async () => {
  const env = await seed();
  const refused = async (body, code, status = 400) => {
    const r = await post(env, body);
    assert.equal(r.status, status, JSON.stringify(body).slice(0, 80));
    assert.equal(r.json.code, code);
  };
  await refused('not json', 'changelog_not_json');
  await refused('[1]', 'changelog_not_json');
  await refused({ ...good, version: 0 }, 'changelog_version');
  await refused({ ...good, version: 7.5 }, 'changelog_version');
  await refused({ ...good, version: '7' }, 'changelog_version');
  await refused({ ...good, publish_hash: '' }, 'changelog_hash');
  await refused({ ...good, publish_hash: 'a b/c' }, 'changelog_hash');
  await refused({ ...good, markdown: '' }, 'changelog_markdown');
  await refused({ ...good, markdown: 'a\u0000b' }, 'changelog_markdown');
  await refused({ ...good, markdown: 5 }, 'changelog_markdown');
  await refused({ ...good, extra: 1 }, 'changelog_unknown_field');
  await refused({ ...good, markdown: 'x'.repeat(64 * 1024 + 1) }, 'changelog_too_large', 413);
  assert.deepEqual((await listOf(env)).json, []);
  assert.equal((await post(env, { ...good, markdown: 'x'.repeat(64 * 1024) })).status, 201, 'exactly the limit');
  assert.equal((await post(env, { ...good, markdown: '→'.repeat(Math.floor(64 * 1024 / 3) + 1) })).status, 413, 'counted in bytes: three each');
});

test('a log is for the version and the publish the listing is at now, and for no other', async () => {
  const env = await seed();
  for (const [label, body] of [['another version', { ...good, version: 6 }], ['a later version', { ...good, version: 8 }], ['another publish', { ...good, publish_hash: 'ff'.repeat(32) }]]) {
    const r = await post(env, body);
    assert.equal(r.status, 409, label);
    assert.equal(r.json.code, 'changelog_mismatch');
  }
  assert.equal((await post(env, { ...good, publish_hash: HASH.toUpperCase() })).status, 201, 'the hash with or without its prefix, in any case');
  const unpublished = await seed({ hash: null });
  assert.equal((await post(unpublished, good)).status, 409, 'a listing with no world publish has no hash to match');
});

test('a log is created if absent: never overwritten, at most ten for a version, and parallel posts get different ids', async () => {
  const env = await seed();
  const answers = await Promise.all(Array.from({ length: 12 }, (_, n) => post(env, { ...good, markdown: `# attempt ${n}\n` })));
  const ok = answers.filter(r => r.status === 201);
  assert.equal(ok.length, 10);
  assert.equal(new Set(ok.map(r => r.json.id)).size, 10, 'ten different ids');
  assert.deepEqual(answers.filter(r => r.status !== 201).map(r => r.json.code), ['changelog_limit', 'changelog_limit']);
  const texts = new Set(await Promise.all(ok.map(async r => (await call(env, 'GET', `/api/simulations/${SIM}/changelogs/${r.json.id}`, 'author')).text)));
  assert.equal(texts.size, 10, 'every post kept its own text');
});

test('a listing under a hold refuses a log and stops the author reading; an admin still reads', async () => {
  for (const setup of [
    async () => seed({ moderation: { status: 'quarantined' } }),
    async () => { const e = await seed(); await e.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } }); return e; },
  ]) {
    const env = await setup();
    env.SCENES.objects.set(`universes/${SIM}/changelogs/0000000007-00000000-0.md`, { body: new TextEncoder().encode(MD), meta: { customMetadata: { version: '7', publish_hash: 'h', created_at: 'then', bytes: '5' } } });
    const refused = await post(env, good);
    assert.equal(refused.status, 409);
    assert.equal(refused.json.code, 'in_review');
    assert.doesNotMatch(refused.text, /quarantin|legal|hold|frozen/i);
    const gone = await call(env, 'GET', `/api/simulations/${SIM}/changelogs`, 'author');
    assert.equal(gone.status, 404, 'the evidence is held for the author too');
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/changelogs/0000000007-00000000-0`, 'author')).status, 404);
    assert.equal((await listOf(env, 'admin')).status, 200);
    assert.equal((await listOf(env, 'admin')).json.length, 1);
  }
  // A publish freeze from before 2026-10-15 is not read: no moderation outcome restricts the account (McKale).
  const frozen = await seed();
  await frozen.USERS.put(`publish-frozen:${AUTHOR}`, '{}');
  assert.equal((await post(frozen, good)).status, 201, 'the author still posts a log');
});

test('a listing keeps at most 200 logs and 8 MB, the oldest go, and the list shows 50', async () => {
  const env = await seed({ version: 300 });
  for (let n = 1; n <= 205; n++) env.SCENES.objects.set(`universes/${SIM}/changelogs/${String(n).padStart(10, '0')}-${String(n).padStart(8, '0')}-0.md`, { body: new TextEncoder().encode('x'), meta: { customMetadata: { version: String(n), publish_hash: 'h', created_at: `2026-01-01T00:00:${String(n % 60).padStart(2, '0')}Z`, bytes: '1' } } });
  assert.equal((await post(env, { ...good, version: 300 })).status, 201);
  const keys = [...env.SCENES.objects.keys()].filter(k => k.startsWith(`universes/${SIM}/changelogs/`));
  assert.equal(keys.length, 200);
  assert.ok(!keys.some(k => k.includes('/0000000001-')), 'the oldest went');
  const list = (await listOf(env)).json;
  assert.equal(list.length, 50);
  assert.equal(list[0].version, 300, 'the new one is first');

  const heavy = await seed({ version: 400 });
  for (let n = 1; n <= 5; n++) heavy.SCENES.objects.set(`universes/${SIM}/changelogs/${String(n).padStart(10, '0')}-${String(n).padStart(8, '0')}-0.md`, { body: new TextEncoder().encode('x'), meta: { customMetadata: { version: String(n), publish_hash: 'h', created_at: '2026-01-01T00:00:00Z', bytes: String(2 * 1024 * 1024) } } });
  assert.equal((await post(heavy, { ...good, version: 400 })).status, 201);
  const left = [...heavy.SCENES.objects.keys()].filter(k => k.startsWith(`universes/${SIM}/changelogs/`));
  assert.ok(left.length <= 5, `over 8 MB the oldest are dropped (${left.length} left)`);
  assert.ok(left.some(k => k.includes('0000000400-')), 'the new log stays');
});

test('the body is read through a cap: a huge body is refused without being read whole, and a day has 100 posts', async () => {
  const env = await seed();
  const big = await post(env, JSON.stringify({ ...good, markdown: 'y'.repeat(2 * 1024 * 1024) }));
  assert.equal(big.status, 413);
  assert.equal(big.json.code, 'changelog_too_large');
  const day = new Date().toISOString().slice(0, 10);
  for (let n = 0; n < 100; n++) await env.SCENES.put(`locks/changelog/${AUTHOR}/${day}/${n}`, '1');
  const limited = await post(env, good);
  assert.equal(limited.status, 429);
  assert.equal(limited.json.code, 'changelog_rate');
  assert.deepEqual((await listOf(env)).json, [], 'nothing was stored');
});

test('the log id in a path is exactly what the route made, and other methods are refused', async () => {
  const env = await seed();
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/changelogs/..%2F..%2Fsim`, 'author')).status, 404);
  assert.equal((await call(env, 'DELETE', `/api/simulations/${SIM}/changelogs`, 'author')).status, 405);
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/changelogs/1700000000-abcdef01-0`, 'author', {})).status, 405);
});
