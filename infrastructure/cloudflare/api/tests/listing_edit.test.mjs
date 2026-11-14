// An author edits a published listing from the website: PATCH /api/simulations/{id}/listing (src/listing_edit.mjs,
// docs/networking/LISTING_EDIT.md). What this file pins:
//   - only the author, signed in, and never while a hold is on the listing or a review is running (the run's lease);
//   - a changed title or description is text a review reads: it sends the listing back to review, and the next review
//     reads the new text; the same text is no change;
//   - visibility is not reviewed: an approved listing is listed or kept private at once; an archived one stays private;
//   - a value that breaks a rule refuses the whole request and writes nothing;
//   - Studio's next commit, sending back the title or description this edit replaced, keeps the edit (src/listing_guard.mjs);
//     a text the author writes in Studio is theirs.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { packageFrom } from './helpers/package_fake.mjs';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { fakeClaude, questionnaireAnswers } from './helpers/claude_fake.mjs';
import { routePolicy, FRESH } from '../src/route_policy.mjs';
import { checkListingEdit, applyListingEdit, handleEditListing, LISTING_EDIT_LIMITS } from '../src/listing_edit.mjs';
import { acquireLease, releaseLease, thresholdsFrom, runModerationCase, isListable, buildJevQuestions, CODE_TESTS, POLICY_VERSION, POLICY_HASH_ANCHORED } from '../src/moderation.mjs';
import { questionnaireRecord } from '../src/rating_questionnaire.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

const worker = await loadWorker();
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const author = mintToken(AUTHOR);
const stranger = mintToken(STRANGER);

// ── the rules (pure) ─────────────────────────────────────────────────────────

test('checkListingEdit: a title is 1 to 120 characters, a description at most 4000, is_public a boolean; nothing sent is refused', () => {
  assert.deepEqual(checkListingEdit({ name: '  The Justice Super Station  ' }), { ok: true, edit: { name: 'The Justice Super Station' } });
  assert.deepEqual(checkListingEdit({ description: '' }), { ok: true, edit: { description: '' } }, 'a description may be cleared');
  assert.deepEqual(checkListingEdit({ is_public: false, other: 1 }), { ok: true, edit: { is_public: false } }, 'other keys are ignored');
  assert.equal(checkListingEdit({ name: 'x'.repeat(LISTING_EDIT_LIMITS.name) }).ok, true);
  for (const [body, code, field] of [
    [{ name: '   ' }, 'bad_name', 'name'],
    [{ name: 7 }, 'bad_name', 'name'],
    [{ name: 'x'.repeat(121) }, 'too_long', 'name'],
    [{ description: 'x'.repeat(4001) }, 'too_long', 'description'],
    [{ description: null }, 'bad_description', 'description'],
    [{ is_public: 'yes' }, 'bad_visibility', 'is_public'],
    [{}, 'bad_request', null],
    [[], 'bad_request', null],
    [null, 'bad_request', null],
  ]) {
    const r = checkListingEdit(body);
    assert.deepEqual([r.ok, r.code, r.field], [false, code, field], JSON.stringify(body));
  }
  assert.match(checkListingEdit({ name: 'x'.repeat(130) }).error, /130 characters; the most is 120/);
});

test('applyListingEdit: only a changed title or description is reviewed; visibility is not; an archived listing keeps its choice for Unarchive', () => {
  const sim = { name: 'Justice', description: 'A courtroom.', is_public: true, listing_fix: { before: { description: 'x' } } };
  assert.deepEqual(applyListingEdit(sim, { name: 'Justice', description: 'A courtroom.' }), { changed: [], review: false });
  assert.deepEqual(sim.listing_fix, { before: { description: 'x' } }, 'no change leaves the guard as it was');
  assert.deepEqual(applyListingEdit(sim, { is_public: false }), { changed: ['is_public'], review: false });
  assert.equal(sim.is_public, false);
  assert.deepEqual(applyListingEdit(sim, { description: 'A courtroom. https://rust-justice.miksunbot.workers.dev/' }, 'T1'), { changed: ['description'], review: true });
  assert.deepEqual(sim.listing_fix, { at: 'T1', by: 'author_web', before: { description: 'A courtroom.' }, superseded: { name: [], description: ['x'], tags: [] } },
    'the replaced text is guarded against a Studio that still holds it, and the older guarded text is kept');
  assert.deepEqual(applyListingEdit(sim, { name: 'The Justice Super Station' }, 'T2'), { changed: ['name'], review: true });
  assert.deepEqual(sim.listing_fix.before, { description: 'A courtroom.', name: 'Justice' });
  const archived = { name: 'A', is_public: false, archived: true, archived_was_public: true };
  assert.deepEqual(applyListingEdit(archived, { is_public: false }), { changed: ['is_public'], review: false });
  assert.deepEqual([archived.is_public, archived.archived_was_public], [false, false], 'still private; Unarchive restores the new choice');
  assert.deepEqual(applyListingEdit(archived, { is_public: false }), { changed: [], review: false });
});

// ── the route ────────────────────────────────────────────────────────────────

let seq = 0;
async function routeEnv() {
  const env = fakeEnv();
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  return env;
}
async function seed(env, fields = {}) {
  const id = `00000000-0000-4000-8000-${String(++seq).padStart(12, '0')}`;
  const sim = {
    id, name: `Station ${seq}`, description: 'A courtroom.', author_id: AUTHOR, author_name: 'host', is_public: true, max_players: 8,
    moderation: { status: 'approved', rating: 'all_ages' }, published_at: new Date(1_700_000_000_000 + seq * 1000).toISOString(), ...fields,
  };
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
  return id;
}
async function call(env, method, path, { token, body } = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: parsed, text };
}
const stored = async (env, id) => JSON.parse(await env.SOCIAL.get(`sim:${id}`));
const edit = (env, id, body, token = author) => call(env, 'PATCH', `/api/simulations/${id}/listing`, { token, body });

test('PATCH .../listing: a new description sends an approved listing back to review; the same text again is no change', async () => {
  const env = await routeEnv();
  const id = await seed(env);
  const link = 'A courtroom.\nPlay the companion site: https://rust-justice.miksunbot.workers.dev/';
  const r = await edit(env, id, { description: link });
  assert.equal(r.status, 200, r.text);
  assert.deepEqual([r.body.changed, r.body.review, r.body.status, r.body.description], [['description'], true, 'in_review', link]);
  const after = await stored(env, id);
  assert.deepEqual([after.description, after.moderation.status], [link, 'pending']);
  assert.equal(isListable(after), false, 'off the gallery until the review passes the new text');
  const before = await env.SOCIAL.get(`sim:${id}`);
  const again = await edit(env, id, { description: `  ${link}  ` });
  assert.deepEqual([again.status, again.body.changed, again.body.review], [200, [], false]);
  assert.equal(await env.SOCIAL.get(`sim:${id}`), before, 'no change writes nothing');
  const renamed = await edit(env, id, { name: 'The Justice Super Station' });
  assert.deepEqual([renamed.body.changed, renamed.body.review, (await stored(env, id)).name], [['name'], true, 'The Justice Super Station']);
});

test('PATCH .../listing: visibility alone is not reviewed; an approved listing is private or listed at once', async () => {
  const env = await routeEnv();
  const id = await seed(env);
  const hide = await edit(env, id, { is_public: false });
  assert.deepEqual([hide.status, hide.body.changed, hide.body.review, hide.body.status], [200, ['is_public'], false, 'approved_private']);
  assert.equal((await stored(env, id)).moderation.status, 'approved', 'the review stands');
  const show = await edit(env, id, { is_public: true });
  assert.deepEqual([show.body.status, isListable(await stored(env, id))], ['listed', true]);
  const archived = await seed(env, { is_public: false, archived: true, archived_was_public: true });
  const r = await edit(env, archived, { is_public: true });
  assert.deepEqual([r.body.is_public, r.body.archived], [false, true], 'an archived listing stays private');
});

test('PATCH .../listing: only the author, signed in; a bad value refuses the whole request and writes nothing', async () => {
  const env = await routeEnv();
  const id = await seed(env);
  const before = await env.SOCIAL.get(`sim:${id}`);
  assert.equal((await edit(env, id, { name: 'Mine now' }, stranger)).status, 404, 'not yours reads as not found');
  assert.equal((await edit(env, '00000000-0000-4000-8000-ffffffffffff', { name: 'x' })).status, 404);
  assert.equal((await call(env, 'PATCH', `/api/simulations/${id}/listing`, { body: { name: 'x' } })).status, 401);
  for (const body of [{ name: 'Fine', description: 'x'.repeat(4001) }, { name: '' }, { is_public: 'no' }, {}]) {
    const r = await edit(env, id, body);
    assert.equal(r.status, 400, JSON.stringify(body));
    assert.ok(r.body.code, r.text);
  }
  assert.equal(await env.SOCIAL.get(`sim:${id}`), before, 'a refusal writes nothing, the valid title included');
  assert.equal(routePolicy('PATCH', `/api/simulations/${id}/listing`).class, FRESH);
});

test('PATCH .../listing: refused while a review runs (its lease) and while held, in the same words, and writes nothing', async () => {
  const env = await routeEnv();
  const id = await seed(env, { moderation: { status: 'pending' } });
  const lease = await acquireLease(env, id, thresholdsFrom(env), 'run');
  assert.ok(lease);
  const before = await env.SOCIAL.get(`sim:${id}`);
  const running = await edit(env, id, { description: 'Edited mid-run' });
  assert.deepEqual([running.status, running.body.code], [409, 'in_review']);
  assert.equal(await env.SOCIAL.get(`sim:${id}`), before);
  await releaseLease(env, lease);
  assert.equal((await edit(env, id, { description: 'Edited after the run' })).status, 200, 'the run ended: the edit goes through');
  assert.equal(await env.SCENES.get(`locks/moderation/${id}.json`), null, 'the edit released the lease it took');
  const held = await seed(env, { moderation: { status: 'quarantined' } });
  const heldBefore = await env.SOCIAL.get(`sim:${held}`);
  const refused = await edit(env, held, { description: 'x' });
  assert.deepEqual([refused.status, refused.body], [409, running.body], 'a hold reads exactly as a running review');
  assert.equal(await env.SOCIAL.get(`sim:${held}`), heldBefore);
});

// ── the review reads the new text ───────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).sort().slice(0, limit).map(name => ({ name })), list_complete: true }),
  };
}
function r2() {
  const m = new Map();
  let versions = 0;
  const obj = (key, rec) => ({ key, size: rec.bytes.byteLength, etag: rec.etag, httpMetadata: rec.httpMetadata || {}, customMetadata: rec.customMetadata || {}, checksums: rec.checksums || {}, version: rec.version, text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0), body: rec.bytes });
  return {
    m,
    put: async (key, body, opts = {}) => {
      if (opts.onlyIf) { const cur = m.get(key); if (opts.onlyIf.etagDoesNotMatch === '*' && cur) return null; if (opts.onlyIf.etagMatches !== undefined && (!cur || cur.etag !== opts.onlyIf.etagMatches)) return null; }
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body).buffer : (body instanceof ArrayBuffer ? body : body.buffer);
      const rec = { version: String(++versions), bytes, etag: createHash('md5').update(Buffer.from(bytes)).digest('hex') + versions, httpMetadata: opts.httpMetadata, customMetadata: opts.customMetadata, checksums: opts.sha256 ? { sha256: Uint8Array.from(Buffer.from(opts.sha256, 'hex')).buffer } : undefined };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async key => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key, size: m.get(key).bytes.byteLength })), truncated: false }),
  };
}
const json = (v, status = 200, headers = {}) => new Response(JSON.stringify(v), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
const STATEMENT_TEXT = [
  'Category: Simulation',
  'Objective: Set the tide range and read the depth at the quay.',
  'Lesson: How tidal range and basin shape set harbour depth.',
  'Real-world link: Models a tidal harbour from published tide tables.',
].join('\n');
const scores = () => Object.fromEntries(CODE_TESTS.map(k => [k, { score: 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
const judgeAnswer = () => ({
  verdict: 'publish', articles: [], tests: scores(), statement_check: 'matches', rating_floor: 'ages_10', descriptors: [], is_game: false, is_fiction: false,
  featured_candidate: false, policy_version: POLICY_VERSION, policy_hash: POLICY_HASH_ANCHORED, spatial_evidence: ['four quay cranes along the east basin'],
  rationale: 'Every test is shown.', confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, suggested_tags: [],
});
function jevAnswers() {
  const answers = {};
  for (const [k, def] of Object.entries(buildJevQuestions())) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: ['metadata_consistency', 'functional_purpose', 'spatial_intent', 'tag_fit', 'model_implemented', 'result_readout'].includes(k) ? 0.9 : 0.02 };
    else if (def.type === 'choice') {
      const first = k === 'content_rating' ? 'all_ages' : k === 'content_kind' ? 'simulation' : Object.keys(def.criteria)[0];
      answers[k] = { type: 'choice', choice: first, probabilities: Object.fromEntries(Object.keys(def.criteria).map(c => [c, c === first ? 0.9 : 0.01])), confidence: 0.9 };
    } else {
      const pick = k === 'quality_band' ? 'solid_authored_experience' : def.criteria[0];
      answers[k] = { type: 'score', score: def.criteria.indexOf(pick) / (def.criteria.length - 1), probabilities: Object.fromEntries(def.criteria.map(l => [l, l === pick ? 0.85 : 0.03])), confidence: 0.85 };
    }
  }
  return { model: 'jev-test', answers };
}
const MSIM = 'a1b2c3d4-0000-4000-8000-00000000ed17';

test('the review after a web edit reads the new description, and lists the listing with it', async () => {
  const env = { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', JWT_SECRET: 'test-secret' };
  env.EMAIL = { send: async () => {} };
  env.__EmailMessage = class { constructor(from, to, raw) { this.raw = raw; } };
  env.USERS.m.set('user:user-1', JSON.stringify({ id: 'user-1', username: 'ann', email: 'ann@example.com' }));
  const sim = {
    id: MSIM, name: 'Harbour Basin', description: `A working harbour.\n${STATEMENT_TEXT}`, tags: ['harbour'], tags_source: 'author', author_id: 'user-1', is_public: true,
    r2_key: `universes/${MSIM}/universe.pak`, published_at: new Date().toISOString(), updated_at: new Date().toISOString(),
    moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()),
  };
  const pak = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(`PAK-${MSIM}`).buffer, { sha256: createHash('sha256').update(`PAK-${MSIM}`).digest('hex') });
  sim.pak_etag = pak.etag; sim.scene_size_bytes = pak.size;
  await env.SOCIAL.put(`sim:${MSIM}`, JSON.stringify(sim));
  await env.SCENES.put(`universes/${MSIM}/moderation/dossier.json`, JSON.stringify({ dossier_version: 1, digest: { entity_count: 300, default_material_fraction: 0.2, default_name_fraction: 0.2, script_count: 1, text_string_count: 4 }, strings: ['Crane 4'], scripts: [{ path: 'Soul/tide.luau', language: 'luau', lines: 3, source: 'local depth = basin + tide' }], signals: { urls: [] } }));
  await env.SCENES.put(`universes/${MSIM}/moderation/capture-0.png`, new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]).buffer, { httpMetadata: { contentType: 'image/png' } });
  const seen = [];
  const deps = {
    json, cors: {}, auditLog: async () => {},
    verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async () => null,
    claude: fakeClaude(body => { seen.push(JSON.stringify(body)); return judgeAnswer(); }),
    grokFetch: async () => ({ ok: true, status: 200, json: async () => ({ output: [] }), text: async () => '' }),
    grokModel: 'grok-test', policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    packageScripts: packageFrom(),
    fetch: async () => new Response(JSON.stringify(jevAnswers()), { status: 200 }),
  };
  assert.equal((await runModerationCase(MSIM, env, deps)).status, 'approved');
  assert.equal(isListable(JSON.parse(await env.SOCIAL.get(`sim:${MSIM}`))), true);

  const linked = `A working harbour. Tide tables: https://tides.example.org/basin\n${STATEMENT_TEXT}`;
  const req = new Request(`https://api.eustress.dev/api/simulations/${MSIM}/listing`, { method: 'PATCH', headers: { Authorization: 'Bearer user-1', 'Content-Type': 'application/json' }, body: JSON.stringify({ description: linked }) });
  const res = await handleEditListing(req, MSIM, env, {}, deps);
  const answer = await res.json();
  assert.deepEqual([res.status, answer.review, answer.status], [200, true, 'in_review']);
  assert.equal(isListable(JSON.parse(await env.SOCIAL.get(`sim:${MSIM}`))), false);

  const judged = seen.length;
  const rec = await runModerationCase(MSIM, env, deps);
  assert.equal(rec.status, 'approved', JSON.stringify(rec.decision?.reasons));
  assert.ok(seen.length > judged, 'the judge was asked again, not a copy of the old verdict');
  assert.ok(seen.slice(judged).some(b => b.includes('https://tides.example.org/basin')), 'the judge read the new description');
  const after = JSON.parse(await env.SOCIAL.get(`sim:${MSIM}`));
  assert.deepEqual([after.description, isListable(after)], [linked, true]);
});
