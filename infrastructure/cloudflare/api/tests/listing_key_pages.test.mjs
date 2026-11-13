// The Gallery, Projects and Cases read every listing KV holds, not only the first 100 keys of one list call. KV returns a list a
// page at a time (at most 1,000 keys, and a page may hold fewer than asked for before the last one), so each read follows the
// cursor until the list says it is complete. Each read is also bounded, so one request stays inside the 1,000 storage
// operations an invocation may make. The filters are unchanged: approval, the viewer's rating cap, archived, the legal lock.
// Against the real Worker with fake storage that pages the way KV does.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const VIEWER = '33333333-3333-4333-8333-333333333333';
// Fewer keys per page than the 100 the old reads asked for: a read that does not follow the cursor sees one page and no more.
const PAGE = 40;
// What one invocation may make in storage operations (the repo's figure, docs/architecture/DELETE_PUBLISHED_SPACE.md section 9).
const OPS_BUDGET = 1000;

const idOf = (n) => `aaaaaaaa-0000-4000-8000-${String(n).padStart(12, '0')}`;

// KV's list as Cloudflare documents it: the keys under the prefix in byte order, no more per call than `limit` (at most 1,000),
// and here no more than PAGE, with a cursor until list_complete.
function pagedLikeKv(kv) {
  kv.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    const all = [...kv.store.keys()].filter((k) => k.startsWith(prefix)).sort();
    const start = cursor ? Number(cursor) : 0;
    const keys = all.slice(start, start + Math.min(limit, 1000, PAGE)).map((name) => ({ name }));
    const next = start + keys.length;
    return next < all.length ? { keys, list_complete: false, cursor: String(next) } : { keys, list_complete: true };
  };
  return kv;
}

// Every storage operation the Worker makes from here on, KV and R2 alike.
function countOps(env) {
  const count = { n: 0 };
  for (const name of ['USERS', 'SOCIAL', 'INVENTORY', 'AUDIT_LOG', 'SCREENING', 'TELEMETRY', 'SCENES']) {
    const ns = env[name];
    for (const method of ['get', 'put', 'delete', 'list', 'head']) {
      if (typeof ns?.[method] !== 'function') continue;
      const inner = ns[method].bind(ns);
      ns[method] = (...args) => { count.n += 1; return inner(...args); };
    }
  }
  return count;
}

async function environment() {
  const env = fakeEnv();
  pagedLikeKv(env.SOCIAL);
  for (const id of [AUTHOR, OTHER, VIEWER]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: id.slice(0, 4), created_at: '2026-01-01T00:00:00.000Z' }));
  return env;
}

// One listing as publish writes it: the record and its author index key. `kind` is what the filters see.
async function listing(env, n, author, kind = 'shown') {
  const id = idOf(n);
  const at = new Date(Date.UTC(2026, 0, 1) + n * 60_000).toISOString();
  const rating = kind === 'teen' ? 'teen_13' : kind === 'adult' ? 'adult_18' : n % 2 ? 'all_ages' : 'ages_10';
  const sim = {
    id, name: `World ${n}`, description: '', author_id: author, author_name: 'maker', max_players: 4,
    is_public: kind !== 'archived', moderation: { status: kind === 'pending' ? 'pending' : 'approved', rating },
    play_count: 0, favorite_count: 0, version: 1, published_at: at, updated_at: at,
    ...(kind === 'archived' ? { archived: true, archived_was_public: true, archived_at: at } : {}),
  };
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${author}:${id}`, id);
  if (kind === 'locked') await env.SCENES.put(`locks/legal/${id}`, JSON.stringify({ kind: 'serve_block' }), { customMetadata: { kind: 'serve_block' } });
  return id;
}

// A review case for a listing, as moderation stores it.
async function reviewCase(env, id, author, n) {
  const at = new Date(Date.UTC(2026, 9, 1) + n * 60_000).toISOString();
  await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, author_id: author, created_at: at, updated_at: at, history: [], legal_hold: null, status: 'approved', decision: { outcome: 'approve', rating: 'all_ages' } }));
}

async function call(env, path, who) {
  const headers = who ? { Authorization: `Bearer ${mintToken(who)}` } : {};
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method: 'GET', headers }), env, ctx);
  const text = await res.text();
  return { status: res.status, text, body: JSON.parse(text) };
}

const sorted = (ids) => [...ids].sort();

// Eight kinds in turn, so every page holds some of each: three a signed-out visitor is shown, one a signed-in viewer is also
// shown, and four nobody is (unapproved, archived, above the Adult content cap, under a legal lock).
const KINDS = ['pending', 'archived', 'teen', 'adult', 'locked', 'shown', 'shown', 'shown'];

test('the Gallery serves every eligible listing of more than 100, across every page KV returns, and still only those', async () => {
  const env = await environment();
  const shown = [];
  const signedIn = [];
  for (let n = 0; n < 320; n++) {
    const kind = KINDS[n % KINDS.length];
    const id = await listing(env, n, n % 3 ? AUTHOR : OTHER, kind);
    if (kind === 'shown') { shown.push(id); signedIn.push(id); }
    if (kind === 'teen') signedIn.push(id);
  }
  assert.equal(shown.length, 120);
  assert.equal(signedIn.length, 160);

  for (const path of ['/api/simulations', '/api/gallery']) {
    const r = await call(env, path);
    assert.equal(r.status, 200, r.text);
    assert.equal(r.body.total, shown.length, `${path}: total`);
    assert.deepEqual(sorted(r.body.simulations.map((s) => s.id)), sorted(shown), `${path}: every eligible listing, and nothing else`);
    const stamps = r.body.simulations.map((s) => s.published_at);
    assert.deepEqual(stamps, [...stamps].sort().reverse(), `${path}: newest first, as before`);
  }

  // The viewer's own cap, signed in with Adult content off: teen_13 too, still nothing adult, archived, unapproved or locked.
  const mine = await call(env, '/api/simulations', VIEWER);
  assert.equal(mine.status, 200, mine.text);
  assert.deepEqual(sorted(mine.body.simulations.map((s) => s.id)), sorted(signedIn));
  // `?rating_max=` still narrows it.
  const narrow = await call(env, '/api/simulations?rating_max=all_ages', VIEWER);
  assert.deepEqual(sorted(narrow.body.simulations.map((s) => s.id)), sorted(shown.filter((id) => Number(id.slice(-12)) % 2)));
});

test('Projects lists every one of the author\'s listings of more than 100, across every page, and none of anyone else\'s', async () => {
  const env = await environment();
  const mine = [];
  let n = 0;
  for (let i = 0; i < 130; i++) mine.push(await listing(env, n++, AUTHOR, KINDS[i % KINDS.length]));
  for (let i = 0; i < 30; i++) await listing(env, n++, OTHER);

  const r = await call(env, '/api/projects', AUTHOR);
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.total, 130);
  assert.deepEqual(sorted(r.body.projects.map((p) => p.id)), sorted(mine));
  // The author sees each as before: an archived one as archived, an approved public one as published.
  const status = Object.fromEntries(r.body.projects.map((p) => [p.id, p.status]));
  assert.equal(status[idOf(1)], 'archived');
  assert.equal(status[idOf(5)], 'published');
  assert.notEqual(status[idOf(4)], 'published', 'a legally locked listing is not shown to its author as published');
  assert.deepEqual(Object.keys(r.body).sort(), ['limit', 'page', 'projects', 'total'], 'the same response shape');
});

test('Cases lists the review of every one of the author\'s listings of more than 100, page by page', async () => {
  const env = await environment();
  const mine = [];
  let n = 0;
  for (let i = 0; i < 130; i++) {
    const id = await listing(env, n++, AUTHOR, 'shown');
    await reviewCase(env, id, AUTHOR, i);
    mine.push(id);
  }
  for (let i = 0; i < 30; i++) {
    const id = await listing(env, n++, OTHER, 'shown');
    await reviewCase(env, id, OTHER, i);
  }

  const seen = [];
  let cursor = '0';
  for (let pages = 0; cursor !== null && pages < 10; pages++) {
    const r = await call(env, `/api/account/cases?limit=50&cursor=${cursor}`, AUTHOR);
    assert.equal(r.status, 200, r.text);
    assert.equal(r.body.total, 130, 'the total counts every review');
    seen.push(...r.body.cases.map((c) => c.listing.id));
    cursor = r.body.next_cursor;
  }
  assert.equal(cursor, null);
  assert.equal(seen.length, 130, 'each review once');
  assert.deepEqual(sorted(seen), sorted(mine));
});

// Past each bound, a read stops instead of running out of storage operations, which would end the request with an error.
test('each read is bounded: far more listings than it reads still leave the request inside the invocation\'s storage budget', async () => {
  const gallery = await environment();
  for (let n = 0; n < 600; n++) await listing(gallery, n, n % 2 ? AUTHOR : OTHER, 'shown');
  const ops = countOps(gallery);
  const g = await call(gallery, '/api/simulations');
  assert.equal(g.status, 200, g.text);
  assert.ok(g.body.simulations.length > 100, `more than one old window served (${g.body.simulations.length})`);
  assert.ok(ops.n < OPS_BUDGET, `the Gallery made ${ops.n} storage operations`);

  const author = await environment();
  for (let n = 0; n < 400; n++) {
    const id = await listing(author, n, AUTHOR, 'shown');
    await reviewCase(author, id, AUTHOR, n);
  }
  for (const path of ['/api/projects', '/api/account/cases?limit=50']) {
    const counted = countOps(author);
    const r = await call(author, path, AUTHOR);
    assert.equal(r.status, 200, `${path}: ${r.text}`);
    const served = r.body.projects ? r.body.projects.length : r.body.total;
    assert.ok(served > 100, `${path}: more than one old window read (${served})`);
    assert.ok(counted.n < OPS_BUDGET, `${path} made ${counted.n} storage operations`);
  }
});
