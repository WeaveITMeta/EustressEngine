// An approval is for the objects that were reviewed. A listing record that says
// "approved" is not enough to serve: the object about to be sent has to be the
// one the approval was bound to (locks/approved/{id}.json in R2). Checked
// against the real Worker with fake storage; the fake bucket's etag for a key is
// `etag-<key>`, so a binding naming any other etag stands for "the stored object
// is not the one that was reviewed".

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const TOKEN = { author: mintToken(AUTHOR), other: mintToken(OTHER), admin: mintToken(ADMIN) };
const PAK = `universes/${SIM}/universe.pak`;
const THUMB = `thumbnails/${SIM}/thumb.webp`;
const MANIFEST = `universes/${SIM}/world/manifest.json`;
const SOURCE = `universes/${SIM}/world/source-manifest.json`;
const sha = text => createHash('sha256').update(text).digest('hex');
// What each stored object holds. Objects are stored with their SHA-256, so an identity is known from the content.
const BODY = { [PAK]: 'the world', [THUMB]: 'RIFFxxxxWEBPthumb', [MANIFEST]: '{"chunks":[]}', [SOURCE]: '{"chunks":[]}' };
const etag = key => `sha256:${sha(BODY[key])}`;
const putWithHash = (env, key, body) => env.SCENES.put(key, body, { sha256: sha(body) });

async function seed({ binding, sim = {}, status = 'approved' } = {}) {
  const env = fakeEnv();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({
    id: SIM, name: 'World', description: '', genre: 'all', max_players: 10, author_id: AUTHOR, author_name: 'x',
    is_public: true, moderation: { status, rating: 'all_ages' }, play_count: 0, favorite_count: 0, version: 1,
    r2_key: PAK, published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z', ...sim,
  }));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${SIM}`, SIM);
  await putWithHash(env, PAK, BODY[PAK]);
  await putWithHash(env, THUMB, BODY[THUMB]);
  if (binding) await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, ...binding }));
  return env;
}

async function call(env, method, path, who, headers = {}) {
  const h = { ...(who ? { Authorization: `Bearer ${TOKEN[who]}` } : {}), ...headers };
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h }), env, ctx);
  return { status: res.status, type: res.headers.get('Content-Type'), cache: res.headers.get('Cache-Control'), text: await res.text().catch(() => '') };
}

const GOOD = { content: etag(PAK), thumb: etag(THUMB), source: null };

test('control: a binding that names the stored objects serves them', async () => {
  const env = await seed({ binding: GOOD });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  const thumb = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.equal(thumb.status, 200);
  assert.equal(thumb.type, 'image/webp');
});

test('a stale "approved" marker over a package that is not the reviewed one serves nobody but its author and an admin', async () => {
  const env = await seed({ binding: { ...GOOD, content: 'the-package-that-was-reviewed' } });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'other')).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'author')).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'admin')).status, 200);
});

test('a takedown that involved no legal hold holds even when the marker is written back as approved', async () => {
  const env = await seed({ binding: { ...GOOD, revoked: true } });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  const env2 = await seed({ binding: GOOD });
  await env2.SCENES.put(`locks/approved/${SIM}.json`, '{not json');
  assert.equal((await call(env2, 'GET', `/api/simulations/${SIM}/download`)).status, 403, 'an unreadable record grants nothing');
});

test('a listing approved before bindings existed serves as it did', async () => {
  const env = await seed({});
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).type, 'image/webp');
});

test('a thumbnail that is not the reviewed one is replaced by a plain panel for the public, and the listing stays available', async () => {
  const env = await seed({ binding: { ...GOOD, thumb: 'the-image-that-was-reviewed' } });
  const anon = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.equal(anon.status, 200);
  assert.equal(anon.type, 'image/svg+xml');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, 'other')).type, 'image/svg+xml');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, 'author')).type, 'image/webp', 'its author sees the image');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, 'admin')).type, 'image/webp');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200, 'the package is still the reviewed one');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`)).status, 200);
});

const worldSim = { format: 'echk', r2_key: MANIFEST, world: { manifest_key: MANIFEST, manifest_sha256: 'a'.repeat(64), source_manifest_key: SOURCE, source_manifest_sha256: 'b'.repeat(64) }, open_source: true };

async function seedWorld(binding) {
  const env = await seed({ binding, sim: worldSim });
  await putWithHash(env, MANIFEST, BODY[MANIFEST]);
  await putWithHash(env, SOURCE, BODY[SOURCE]);
  return env;
}

test('the world manifest follows the reviewed manifest, and the source world follows the reviewed source', async () => {
  const good = { content: etag(MANIFEST), thumb: etag(THUMB), source: etag(SOURCE) };
  const env = await seedWorld(good);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/manifest`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/source`)).status, 200);
  const swappedSource = await seedWorld({ ...good, source: 'another-source' });
  assert.equal((await call(swappedSource, 'GET', `/api/simulations/${SIM}/world/manifest`)).status, 200, 'the world itself is still the reviewed one');
  assert.equal((await call(swappedSource, 'GET', `/api/simulations/${SIM}/world/source`)).status, 403, 'the source is not');
  const swappedWorld = await seedWorld({ ...good, content: 'another-manifest' });
  assert.equal((await call(swappedWorld, 'GET', `/api/simulations/${SIM}/world/manifest`)).status, 403);
  assert.equal((await call(swappedWorld, 'GET', `/api/simulations/${SIM}/world/manifest`, 'author')).status, 200);
});

// ── the website manifest follows its listing ──

const WEBSITE = `universes/${SIM}/website-manifest.json`;
async function seedWebsite(status) {
  const env = await seed({ binding: GOOD, status });
  await env.SCENES.put(WEBSITE, JSON.stringify({ namespace: 'harbor', values: {}, publish_hash: 'h1' }), { customMetadata: { publishHash: 'h1', namespace: 'harbor' } });
  return env;
}

test('a website manifest of a listing under a hold or decided against answers like a manifest that does not exist', async () => {
  for (const status of ['quarantined', 'held', 'rejected', 'changes_requested', 'appealed']) {
    const env = await seedWebsite(status);
    const r = await call(env, 'GET', `/api/simulation/${SIM}/manifest`);
    assert.equal(r.status, 404, status);
    assert.equal(r.cache, 'no-store', `${status}: a takedown is not cached`);
    assert.doesNotMatch(r.text, /quarantin|legal|hold|reject/i, status);
  }
  const locked = await seedWebsite('approved');
  await locked.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.equal((await call(locked, 'GET', `/api/simulation/${SIM}/manifest`)).status, 404, 'a legal lock beats an approved marker');
  for (const status of ['approved', 'pending']) {
    assert.equal((await call(await seedWebsite(status), 'GET', `/api/simulation/${SIM}/manifest`)).status, 200, status);
  }
});

test('a pinned manifest response is cached for an hour, not a year', async () => {
  const env = await seedWebsite('approved');
  const r = await call(env, 'GET', `/api/simulation/${SIM}/manifest?v=h1`);
  assert.equal(r.status, 200);
  assert.equal(r.cache, 'public, max-age=3600');
});

test('a chunk only the source world names is served only while the source is the reviewed one', async () => {
  const HASH = 'c'.repeat(64);
  const SOURCE_BODY = JSON.stringify({ spaces: [{ chunks: [{ blake3: HASH }] }] });
  const good = { content: etag(MANIFEST), thumb: etag(THUMB), source: `sha256:${sha(SOURCE_BODY)}` };
  for (const [binding, expected] of [[good, 200], [{ ...good, source: 'another-source' }, 404]]) {
    const env = await seedWorld(binding);
    await putWithHash(env, SOURCE, SOURCE_BODY);
    await env.SCENES.put(`universes/${SIM}/chunks/${HASH}.echk`, 'chunk bytes');
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/world/chunks/${HASH}`)).status, expected);
  }
});

test('where bindings are required, an approved listing with no binding serves only its author and an admin', async () => {
  const env = await seed({});
  env.REQUIRE_APPROVAL_BINDING = 'true';
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, 'author')).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).type, 'image/svg+xml', 'the public sees the placeholder');
});

test('production requires approval bindings from day one, and staging does not', async () => {
  const { readFileSync } = await import('node:fs');
  const toml = readFileSync(new URL('../wrangler.toml', import.meta.url), 'utf8');
  const at = toml.search(/^\[env\.staging/m);
  assert.ok(at > 0, 'the staging environment is in the file');
  // Production states the flag explicitly ("false" until the backfill is checked, then "true"); staging never sets it.
  assert.match(toml.slice(0, at), /^REQUIRE_APPROVAL_BINDING\s*=\s*"(true|false)"/m);
  assert.doesNotMatch(toml.slice(at), /REQUIRE_APPROVAL_BINDING/);
});

// ── an object put between the head and the get ──

// Swaps the stored object the moment the route fetches it: after the head that judged the listing servable, before the bytes are streamed.
function swapOnFetch(env, key, body) {
  const realGet = env.SCENES.get.bind(env.SCENES);
  const state = { swapped: false };
  env.SCENES.get = async (k, ...rest) => {
    if (k === key && !state.swapped) { state.swapped = true; await putWithHash(env, key, body); }
    return realGet(k, ...rest);
  };
  return state;
}

test('a package put between the head and the get is not served unreviewed; its author and an admin still get it', async () => {
  for (const [who, expected] of [[undefined, 403], ['other', 403], ['author', 200], ['admin', 200]]) {
    const env = await seed({ binding: GOOD });
    const state = swapOnFetch(env, PAK, 'a package nobody reviewed');
    const r = await call(env, 'GET', `/api/simulations/${SIM}/download`, who);
    assert.equal(state.swapped, true, 'the swap happened between the two reads');
    assert.equal(r.status, expected, String(who));
    if (expected === 403) assert.doesNotMatch(r.text, /nobody reviewed/);
  }
});

test('a thumbnail put between the head and the get is replaced by the plain panel for the public', async () => {
  const env = await seed({ binding: GOOD });
  const state = swapOnFetch(env, THUMB, 'an image nobody reviewed');
  const anon = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.equal(state.swapped, true);
  assert.equal(anon.type, 'image/svg+xml');
  const forAuthor = await seed({ binding: GOOD });
  swapOnFetch(forAuthor, THUMB, 'an image nobody reviewed');
  assert.equal((await call(forAuthor, 'GET', `/api/simulations/${SIM}/thumbnail`, 'author')).type, 'image/webp');
});

test('the object that was bound is served as before when nothing was put between the reads', async () => {
  const env = await seed({ binding: GOOD });
  const r = await call(env, 'GET', `/api/simulations/${SIM}/download`);
  assert.equal(r.status, 200);
  assert.equal(r.text, BODY[PAK]);
});

test('an approval revoked between the head and the get stops the download, though the object is unchanged', async () => {
  const env = await seed({ binding: GOOD });
  const realGet = env.SCENES.get.bind(env.SCENES);
  let revoked = false;
  env.SCENES.get = async (k, ...rest) => {
    if (k === PAK && !revoked) { revoked = true; await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, ...GOOD, revoked: true })); }
    return realGet(k, ...rest);
  };
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal(revoked, true);
});
