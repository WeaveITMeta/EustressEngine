// Every image the judge reads fits under the judge's per-image limit, a run that leaves an image out never lists the Space, and
// an image too large to have been judged is never served to the public (src/moderation.mjs JUDGE_IMAGE_MAX_BYTES, judgeImages,
// decideFinal; src/index.js the thumbnail routes; src/cover.mjs the cover and preview routes; scripts/oversize_images.mjs).
//
// The uploads and the serving run through the real Worker against in-memory storage. The review runs the real pipeline
// (runModerationCase) on the same storage, with a scripted judge client and a scripted Jev.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import fs from 'node:fs';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import { fakeClaude, questionnaireAnswers } from './helpers/claude_fake.mjs';
import {
  runModerationCase, approvedCaptureKeys, snapshotReviewed, authorView, buildJevQuestions, judgeImages, makeToolExecutor, loadCase, sweepLocks, judgeRequest, IMAGES_CHECKED, imageVerdictKey, identOfObject, thumbnailApproved, simForServing, HARD_CATEGORIES, saveCase, publicImageBody,
  JUDGE_IMAGE_MAX_B64, JUDGE_IMAGE_MAX_BYTES, CAPTURE_MAX_BYTES, POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import { COVER_MAX_BYTES } from '../src/cover.mjs';
import { imageProblem, prepareUserImage, MULTIPLE_IMAGE_PROBLEM, storedImageVerdict, measurePngAlpha, ALPHA_MEASURE_BUDGET_MS } from '../src/user_images.mjs';
import { listingIcon, productFields, productView, productSummary, apiOriginOf } from '../src/commerce.mjs';
import { listingEvidenceItems } from '../src/evidence_vault.mjs';
import { listingHashes, blockKey } from '../src/takedown.mjs';
import { questionnaireRecord } from '../src/rating_questionnaire.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';
import { assertReadOnly, kvGetArgs, kvListArgs, r2GetArgs, ENVIRONMENTS } from '../scripts/voided_approvals.mjs';
import { replayListing, typeOfBytes, summarize, pageOf, imageKeys, makeRunner, parseArgs, artProblem, gifFrames, countAlpha, inJudgeOrder } from '../scripts/oversize_images.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM2 = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const tokens = { author: mintToken(AUTHOR), stranger: mintToken(STRANGER), admin: mintToken(ADMIN) };
const THUMB_LABEL = 'listing thumbnail (the image the Gallery shows)';
const COVER_LABEL = 'listing cover (the box art the Gallery shows)';

// -- images -----------------------------------------------------------------------------------------------------------------------

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();
const crc32 = (bytes) => {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const u32be = (n) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const concat = (parts) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
};
function pngChunk(type, data) {
  const body = concat([new TextEncoder().encode(type), Uint8Array.from(data)]);
  return concat([Uint8Array.from(u32be(data.length)), body, Uint8Array.from(u32be(crc32(body)))]);
}
/// A PNG of w x h whose picture data carries `weigh` bytes, so what the storage helper keeps is that heavy.
function png(w, h, weigh = 0, seed = 0) {
  const idat = concat([Uint8Array.from([0x78, 0x9c, 0x63, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]), new Uint8Array(weigh).fill(0x5a + (seed & 15))]);
  return concat([Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', [...u32be(w), ...u32be(h), 8, 2, 0, 0, 0]), pngChunk('IDAT', idat), pngChunk('IEND', [])]);
}
/// A PNG of exactly `total` bytes.
const pngOf = (total, seed = 0) => png(600, 800, total - png(600, 800).length, seed);
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
/// An object as an upload stores it: its type and its SHA-256, so a binding identifies it by its digest.
const store = (env, key, bytes, type = 'image/png') => env.SCENES.put(key, bytes, { httpMetadata: { contentType: type }, sha256: sha256(bytes) });
// Raw bytes whose base64 is over the judge's per-image limit: an image stored before uploads were capped to it.
const LEGACY_OVERSIZE = Math.ceil(JUDGE_IMAGE_MAX_B64 * 3 / 4) + 3;

// -- the world --------------------------------------------------------------------------------------------------------------------

const STATEMENT_TEXT = 'Category: Simulation\nObjective: Set the tide range and read the depth at the quay.\nLesson: How tidal range and basin shape set harbour depth.\nReal-world link: Models a tidal harbour from published tide tables.';

async function environment() {
  const env = { ...fakeEnv(), JEV_API_KEY: 'jev-test', MODERATION_THRESHOLDS: '{"audit_rate":0}' };
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada'], [ADMIN, 'root']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return env;
}

/// A listing waiting for its review: a package, a dossier, the captures given (bytes by number), and a thumbnail and cover
/// when given. `pak` names the package's bytes (a second listing with the same bytes is the dedup case).
async function listing(env, { id = SIM, captures = { 0: pngOf(400), 1: pngOf(500) }, thumb = pngOf(300), cover = null, status = 'pending', pak = 'PAKBYTES-1', captureType = 'image/png' } = {}) {
  const r2Key = `universes/${id}/universe.pak`;
  const stored = await env.SCENES.put(r2Key, new TextEncoder().encode(pak), { sha256: sha256(Buffer.from(pak)) });
  const sim = {
    id, name: 'Harbor Town', description: `A working harbor with a tide simulation.\n${STATEMENT_TEXT}`, genre: 'Simulation', author_id: AUTHOR, author_name: 'host',
    is_public: true, thumbnail_url: thumb ? `https://api.eustress.dev/api/simulations/${id}/thumbnail` : null, r2_key: r2Key, play_count: 0,
    published_at: '2026-10-01T00:00:00.000Z', updated_at: '2026-10-01T00:00:00.000Z', moderation: { status }, pak_etag: stored.etag, scene_size_bytes: stored.size,
    rating_questionnaire: questionnaireRecord(questionnaireAnswers()),
    ...(cover ? { cover: { ext: 'png', width: 600, height: 800, bytes: cover.length, updated_at: '2026-10-01T00:00:00.000Z' } } : {}),
  };
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(dossier()), { httpMetadata: { contentType: 'application/json' } });
  for (const [n, bytes] of Object.entries(captures)) await store(env, `universes/${id}/moderation/capture-${n}.png`, bytes, captureType);
  if (thumb) await store(env, `thumbnails/${id}/thumb.png`, thumb);
  if (cover) await store(env, `covers/${id}/cover.png`, cover);
  return sim;
}

const dossier = () => ({
  dossier_version: 1, content_root: 'blake3:abc', listing: { name: 'Harbor Town' },
  digest: { entity_count: 420, class_histogram: [{ class_name: 'Part', count: 300 }], default_material_fraction: 0.3, default_name_fraction: 0.2, duplicate_transform_fraction: 0.05, script_count: 3, text_string_count: 12, spawn_count: 2 },
  strings: ['Harbor Town', 'Welcome to the docks', 'Fish market'],
  scripts: [{ path: 'Soul/tide/tide.luau', language: 'luau', lines: 40, source: 'local tide = 0\nwhile true do tide = tide + 1 end' }],
  assets: { sounds: ['gulls.ogg'] }, signals: { urls: [], emails: [], phones: [], discord_invites: [] },
});

// A Jev answer set where everything is clean.
function jevAnswers() {
  const answers = {};
  for (const [k, def] of Object.entries(buildJevQuestions())) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: ['metadata_consistency', 'functional_purpose', 'spatial_intent', 'tag_fit', 'model_implemented', 'result_readout'].includes(k) ? 0.9 : 0.02 };
    else if (def.type === 'choice') {
      const first = k === 'content_rating' ? 'all_ages' : k === 'content_kind' ? 'simulation' : Object.keys(def.criteria)[0];
      answers[k] = { type: 'choice', choice: first, probabilities: Object.fromEntries(Object.keys(def.criteria).map(c => [c, c === first ? 0.9 : 0.01])), confidence: 0.9 };
    } else {
      const pick = k === 'quality_band' ? 'solid_authored_experience' : def.criteria[0];
      answers[k] = { type: 'score', score: def.criteria.indexOf(pick) / (def.criteria.length - 1), probabilities: Object.fromEntries(def.criteria.map(l => [l, l === pick ? 0.85 : 0.03])), legend: Object.fromEntries(def.criteria.map((l, i) => [l, i])), confidence: 0.85 };
    }
  }
  return { model: 'jev-test', answers, usage: { input_tokens: 1234, output_tokens: 0 } };
}

// A Part VII answer that lists the Space: no article, every test at 2, the statement matches.
const verdict = () => ({
  verdict: 'publish', articles: [],
  tests: Object.fromEntries(['build_quality', 'objective', 'lesson', 'real_world'].map(k => [k, { score: 2, evidence: `${k}: the tide gauge over the harbour basin in capture-0` }])),
  statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false, policy_version: '2.2', policy_hash: 'sha256:x',
  spatial_evidence: ['central plaza with four symmetric fountains', 'material variety across the arcade facades'], rationale: 'Deliberate composition, no prohibited category.',
  confidence: 0.9, flagged_for_review: false, suggested_edit_to_publish: null, suggested_tags: [],
});

const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

function deps() {
  const judge = fakeClaude(verdict());
  return {
    judge,
    claude: judge,
    verifyAuth: async () => null, requireAdmin: async () => null, json, cors: {}, auditLog: async () => {},
    grokFetch: async () => { throw new Error('no research in these tests'); }, grokModel: 'grok-test', extractGrokText: () => '',
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: async () => new Response(JSON.stringify(jevAnswers()), { status: 200, headers: { 'Content-Type': 'application/json' } }),
  };
}

async function call(env, method, path, { who, body } = {}) {
  const headers = who ? { Authorization: `Bearer ${tokens[who]}` } : {};
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  let parsed = null;
  try { parsed = JSON.parse(new TextDecoder().decode(bytes)); } catch { /* an image */ }
  return { status: res.status, bytes, body: parsed, type: res.headers.get('Content-Type') };
}

const record = async (env, id = SIM) => JSON.parse(await env.SOCIAL.get(`sim:${id}`));
const keysUnder = (env, prefix) => [...env.SCENES.objects.keys()].filter((k) => k.startsWith(prefix)).sort();
const sentImages = (d, n = 0) => d.judge.calls[n].body.messages[0].content.filter(c => c.type === 'image').length;

/// The approval a review leaves behind, for the objects in storage now: an approved listing whose bound images are what is
/// stored (legacy data from before uploads were capped).
async function approve(env, id = SIM) {
  const sim = await record(env, id);
  sim.moderation = { status: 'approved', rating: 'all_ages', evidence_epoch: 0 };
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  const now = await snapshotReviewed(env, sim, 0);
  await env.SCENES.put(`locks/approved/${id}.json`, JSON.stringify({ v: 1, ...now, evidence_epoch: 0, by: 'test', at: '2026-10-02T00:00:00.000Z' }));
  return sim;
}

// -- (b) the caps -----------------------------------------------------------------------------------------------------------------

test('the per-image cap is derived from the judge limit: its base64 fits under JUDGE_IMAGE_MAX_B64, and every image upload uses it', () => {
  assert.equal(JUDGE_IMAGE_MAX_BYTES, Math.floor(JUDGE_IMAGE_MAX_B64 * 3 / 4) - 64 * 1024);
  assert.ok(Buffer.alloc(JUDGE_IMAGE_MAX_BYTES).toString('base64').length <= JUDGE_IMAGE_MAX_B64);
  assert.equal(CAPTURE_MAX_BYTES, JUDGE_IMAGE_MAX_BYTES);
  assert.ok(COVER_MAX_BYTES <= JUDGE_IMAGE_MAX_BYTES);
});

for (const status of ['pending', 'approved']) {
  test(`an oversize capture upload is refused with 413 and stores nothing (listing ${status})`, async () => {
    const env = await environment();
    await listing(env, { captures: {}, thumb: null, status });
    const before = keysUnder(env, `universes/${SIM}/moderation/capture-`);
    const r = await call(env, 'PUT', `/api/simulations/${SIM}/captures/0`, { who: 'author', body: pngOf(JUDGE_IMAGE_MAX_BYTES + 1) });
    assert.equal(r.status, 413);
    assert.deepEqual(keysUnder(env, `universes/${SIM}/moderation/capture-`), before);
  });

  test(`an oversize thumbnail upload is refused with 413 and stores nothing (listing ${status})`, async () => {
    const env = await environment();
    await listing(env, { captures: {}, thumb: null, status });
    const r = await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: pngOf(JUDGE_IMAGE_MAX_BYTES + 1) });
    assert.equal(r.status, 413);
    assert.deepEqual(keysUnder(env, `thumbnails/${SIM}/`), []);
  });

  test(`an oversize cover upload is refused with 413 and stores nothing (listing ${status})`, async () => {
    const env = await environment();
    await listing(env, { captures: {}, thumb: null, status });
    const r = await call(env, 'PUT', `/api/simulations/${SIM}/cover`, { who: 'author', body: pngOf(COVER_MAX_BYTES + 1) });
    assert.equal(r.status, 413);
    assert.deepEqual(keysUnder(env, `covers/${SIM}/`), []);
  });
}

test('a capture and a thumbnail at exactly the cap are accepted and stored (the control for the 413s)', async () => {
  const env = await environment();
  await listing(env, { captures: {}, thumb: null });
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/captures/0`, { who: 'author', body: pngOf(JUDGE_IMAGE_MAX_BYTES) })).status, 200);
  assert.equal(keysUnder(env, `universes/${SIM}/moderation/capture-`).length, 1);
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: pngOf(JUDGE_IMAGE_MAX_BYTES) })).status, 200);
  assert.equal(keysUnder(env, `thumbnails/${SIM}/`).length, 1);
});

// -- (a) and (c): a run that leaves an image out holds, and captures_used is what was sent ---------------------------------------

test('a normal run with every image under the limit still approves, sends every image and records each as used', async () => {
  const env = await environment();
  await listing(env, { cover: pngOf(350) });
  const d = deps();
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'approved');
  // The public art first, then the captures in slot order.
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL, COVER_LABEL, 'capture-0.png', 'capture-1.png']);
  assert.deepEqual(rec.judge.captures_left_out, []);
  assert.equal(sentImages(d), 4);
  assert.deepEqual((await approvedCaptureKeys(env, await record(env))).map(k => k.n), [0, 1]);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/preview/1`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).type, 'image/png');
});

async function assertHeldAndUnserved(env, rec, leftOut) {
  assert.equal(rec.status, 'held');
  assert.notEqual(rec.decision.outcome, 'approve');
  const reason = rec.decision.reasons.find(r => r.code === 'images_not_judged');
  assert.ok(reason, 'held for a person because images were not judged');
  assert.deepEqual(reason.images, leftOut);
  assert.deepEqual(rec.judge.captures_left_out, leftOut);
  const sim = await record(env);
  assert.notEqual(sim.moderation.status, 'approved');
  assert.deepEqual(await approvedCaptureKeys(env, sim), []);
  assert.deepEqual((await call(env, 'GET', `/api/simulations/${SIM}/previews`)).body, { previews: [] });
  for (let n = 0; n < 8; n++) assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/preview/${n}`)).status, 404);
  // The labels are the staff's: the author's view of the case names no reason and no image.
  const view = JSON.stringify(authorView(rec, sim));
  assert.doesNotMatch(view, /images_not_judged|capture-\d\.png|listing thumbnail|listing cover/);
}

test('a capture stored over the judge limit (legacy data) is left out, and the case is held, never approved', async () => {
  const env = await environment();
  await listing(env, { captures: { 0: pngOf(400), 1: pngOf(LEGACY_OVERSIZE) } });
  const d = deps();
  const rec = await runModerationCase(SIM, env, d);
  await assertHeldAndUnserved(env, rec, ['capture-1.png']);
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL, 'capture-0.png'], 'captures_used lists only what was sent');
  assert.equal(sentImages(d), 2);
});

test('a thumbnail stored over the judge limit is left out, and the case is held', async () => {
  const env = await environment();
  await listing(env, { thumb: pngOf(LEGACY_OVERSIZE) });
  const rec = await runModerationCase(SIM, env, deps());
  await assertHeldAndUnserved(env, rec, [THUMB_LABEL]);
  assert.deepEqual(rec.judge.captures_used, ['capture-0.png', 'capture-1.png']);
});

test('images under the per-image limit that overflow the request budget are left out, and the case is held', async () => {
  const env = await environment();
  // Five captures at the cap (each about 4.9 MB of base64): after the small thumbnail, sent first, four fit in the 24 MB budget and the fifth does not.
  const captures = Object.fromEntries([0, 1, 2, 3, 4].map(n => [n, pngOf(JUDGE_IMAGE_MAX_BYTES, n)]));
  await listing(env, { captures });
  const d = deps();
  const rec = await runModerationCase(SIM, env, d);
  await assertHeldAndUnserved(env, rec, ['capture-4.png']);
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL, 'capture-0.png', 'capture-1.png', 'capture-2.png', 'capture-3.png']);
  assert.equal(sentImages(d), 5);
});

test('a capture that is gone between the listing and the read, or of a type the judge cannot take, holds the case', async () => {
  const env = await environment();
  await listing(env);
  const get = env.SCENES.get.bind(env.SCENES);
  let first = true;
  env.SCENES.get = async (key) => {
    if (first && key.endsWith('/moderation/capture-1.png')) { first = false; return null; }
    return get(key);
  };
  const rec = await runModerationCase(SIM, env, deps());
  await assertHeldAndUnserved(env, rec, ['capture-1.png']);

  const other = await environment();
  await listing(other, { captureType: 'image/bmp' });
  const held = await runModerationCase(SIM, other, deps());
  await assertHeldAndUnserved(other, held, ['capture-0.png', 'capture-1.png']);
});

test('an approval is copied to a republish of the same package only with the same images; other images are reviewed in full', async () => {
  const env = await environment();
  await listing(env);
  const d = deps();
  assert.equal((await runModerationCase(SIM, env, d)).status, 'approved');
  // The same author, the same package, a different thumbnail: the judge reads it.
  await listing(env, { id: SIM2, thumb: pngOf(LEGACY_OVERSIZE) });
  const second = await runModerationCase(SIM2, env, d);
  assert.equal(second.decision.dedup_of, undefined);
  assert.equal(d.judge.calls.length, 2);
  assert.equal(second.status, 'held');
  // The same images: the approval is copied, as before.
  const env2 = await environment();
  await listing(env2);
  const d2 = deps();
  await runModerationCase(SIM, env2, d2);
  await listing(env2, { id: SIM2 });
  const copied = await runModerationCase(SIM2, env2, d2);
  assert.equal(copied.decision.dedup_of, SIM);
  assert.equal(d2.judge.calls.length, 1);
});

test('judgeImages names everything it does not send: oversize, past the budget, an unknown type, no bytes, past the count', () => {
  const ok = (label, size = 8) => ({ label, contentType: 'image/png', base64: 'A'.repeat(size) });
  const { sent, left } = judgeImages([ok('a'), ok('big', JUDGE_IMAGE_MAX_B64 + 1), { label: 'bmp', contentType: 'image/bmp', base64: 'AAAA' }, { label: 'gone', contentType: null, base64: null }, ...Array.from({ length: 12 }, (_, i) => ok(`n${i}`))]);
  // The count is over the images sent: the three left out for size, type and no bytes take no place (2c5).
  assert.deepEqual(sent.map(c => c.label), ['a', 'n0', 'n1', 'n2', 'n3', 'n4', 'n5', 'n6', 'n7', 'n8']);
  assert.deepEqual(left, ['big', 'bmp', 'gone', 'n9', 'n10', 'n11']);
});

test('a left-out capture never pushes the thumbnail or the cover out of the request: the count is over the images sent', () => {
  const ok = (label) => ({ label, contentType: 'image/png', base64: 'AAAA' });
  const leftOut = (label) => ({ label, contentType: null, base64: null });
  const captures = Array.from({ length: 8 }, (_, n) => ok(`capture-${n}.png`));
  const { sent, left } = judgeImages([...captures, leftOut('capture-0.webp'), leftOut('capture-1.webp'), leftOut('capture-8.png'), ok(THUMB_LABEL), ok(COVER_LABEL)]);
  assert.deepEqual(sent.map(c => c.label), [...captures.map(c => c.label), THUMB_LABEL, COVER_LABEL]);
  assert.deepEqual(left, ['capture-0.webp', 'capture-1.webp', 'capture-8.png']);
  // Past the count is still past the count: eleven images with bytes send ten.
  assert.deepEqual(judgeImages(Array.from({ length: 11 }, (_, n) => ok(`i${n}`))).left, ['i10']);
});

test('stray captures beside eight bound ones: the cover is still judged, and a harmful cover is quarantined', async () => {
  const env = await environment();
  const captures = Object.fromEntries([0, 1, 2, 3, 4, 5, 6, 7].map(n => [n, pngOf(400 + n)]));
  await listing(env, { captures, cover: pngOf(350) });
  // Objects the review never recorded under the capture prefix (a second format a failed delete or superseded record left,
  // a name the upload route never writes): each is named left out.
  await store(env, `universes/${SIM}/moderation/capture-0.webp`, pngOf(420, 3), 'image/webp');
  await store(env, `universes/${SIM}/moderation/capture-1.webp`, pngOf(421, 4), 'image/webp');
  await store(env, `universes/${SIM}/moderation/capture-8.png`, pngOf(422, 5));
  const d = deps();
  // The judge cites A2 only when it is shown the cover.
  const harmful = verdict();
  Object.assign(harmful, { verdict: 'reject', articles: [{ id: 'A2', confidence: 0.95, evidence: 'The listing cover (the box art the Gallery shows) is a recruitment poster for a named armed group, with its flag and a call to join' }], tests: Object.fromEntries(Object.entries(harmful.tests).map(([k, v]) => [k, { ...v, score: 0 }])) });
  d.judge = fakeClaude((body) => (body.messages[0].content.find(c => c.type === 'text').text.split('\n')[0].includes(COVER_LABEL) ? harmful : verdict()), d.judge.calls);
  d.claude = d.judge;
  const rec = await runModerationCase(SIM, env, d);
  assert.ok(rec.judge.captures_used.includes(COVER_LABEL), JSON.stringify(rec.judge.captures_used));
  assert.ok(rec.judge.captures_used.includes(THUMB_LABEL));
  // Three entries left out (one format of capture 0 and of capture 1, whichever the review did not record, and capture-8),
  // none of them in the count.
  assert.equal(rec.judge.captures_left_out.length, 3, JSON.stringify(rec.judge.captures_left_out));
  assert.ok(rec.judge.captures_left_out.includes('capture-8.png'));
  assert.equal(rec.judge.captures_used.length, 10, 'eight captures, the thumbnail and the cover');
  assert.equal(rec.status, 'quarantined', JSON.stringify(rec.decision?.reasons));
});

// -- (d) an approved listing whose bound image is over the limit is not served to the public --------------------------------------

test('an approved listing whose bound capture is over the limit: no public preview of it; staff still fetch it; the rest is served', async () => {
  const env = await environment();
  const big = pngOf(LEGACY_OVERSIZE);
  await listing(env, { captures: { 0: pngOf(400), 1: big } });
  const sim = await approve(env);
  assert.deepEqual((await approvedCaptureKeys(env, sim)).map(k => k.n), [0]);
  assert.deepEqual((await call(env, 'GET', `/api/simulations/${SIM}/previews`)).body.previews.map(p => p.n), [0]);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/preview/0`)).status, 200);
  for (const who of [undefined, 'stranger', 'author']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/preview/1`, { who });
    assert.deepEqual([r.status, r.body], [404, { error: 'No preview' }], `as ${who || 'the public'}`);
  }
  // Staff read it through the moderation capture route, unchanged. (Captures have no author read route.)
  const staff = await call(env, 'GET', `/api/admin/moderation/captures/${SIM}/1`, { who: 'admin' });
  assert.equal(staff.status, 200);
  assert.equal(staff.bytes.length, big.length);
});

test('an approved listing whose bound thumbnail is over the limit: absent to the public, served to its author and an admin', async () => {
  const env = await environment();
  const big = pngOf(LEGACY_OVERSIZE);
  await listing(env, { thumb: big });
  await approve(env);
  for (const who of [undefined, 'stranger']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who });
    assert.deepEqual([r.status, r.body], [404, { error: 'Thumbnail not found' }], `as ${who || 'the public'}: the same as no thumbnail`);
  }
  for (const who of ['author', 'admin']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who });
    assert.equal(r.status, 200, who);
    assert.equal(r.bytes.length, big.length, who);
  }
  // The control: a bound thumbnail under the limit is served to the public.
  const control = await environment();
  await listing(control, { thumb: pngOf(JUDGE_IMAGE_MAX_BYTES) });
  await approve(control);
  const served = await call(control, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.equal(served.status, 200);
  assert.equal(served.bytes.length, JUDGE_IMAGE_MAX_BYTES);
});

test('an approved listing whose bound cover is over the limit: absent to the public, served to its author and an admin', async () => {
  const env = await environment();
  const big = pngOf(LEGACY_OVERSIZE);
  await listing(env, { cover: big });
  await approve(env);
  for (const who of [undefined, 'stranger']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/cover`, { who });
    assert.deepEqual([r.status, r.body], [404, { error: 'No cover' }], `as ${who || 'the public'}: the same as no cover`);
  }
  for (const who of ['author', 'admin']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/cover`, { who });
    assert.equal(r.status, 200, who);
    assert.equal(r.bytes.length, big.length, who);
  }
  const control = await environment();
  await listing(control, { cover: pngOf(COVER_MAX_BYTES) });
  await approve(control);
  assert.equal((await call(control, 'GET', `/api/simulations/${SIM}/cover`)).status, 200);
});

// -- (e) the staff report: scripts/oversize_images.mjs -----------------------------------------------------------------------------

test('the report replays the judge: on the same images it leaves out exactly what judgeImages leaves out', () => {
  const png = (label, bytes, kind = 'capture', contentType = 'image/png') => ({ kind, key: label, label, contentType, bytes });
  const sets = {
    'all small': [png('capture-0.png', 400), png('capture-1.png', 500), png(THUMB_LABEL, 300, 'thumbnail')],
    'a legacy capture over the limit': [png('capture-0.png', 400), png('capture-1.png', LEGACY_OVERSIZE), png(THUMB_LABEL, 300, 'thumbnail')],
    'five at the cap overflow the budget': [0, 1, 2, 3, 4].map(n => png(`capture-${n}.png`, JUDGE_IMAGE_MAX_BYTES)).concat([png(COVER_LABEL, 300, 'cover')]),
    'a type the API does not take': [png('capture-0.png', 400, 'capture', 'application/octet-stream'), png(THUMB_LABEL, 300, 'thumbnail')],
    'past the count': Array.from({ length: 11 }, (_, n) => png(`capture-${String(n).padStart(2, '0')}.png`, 10)).concat([png(THUMB_LABEL, 10, 'thumbnail')]),
  };
  for (const [what, images] of Object.entries(sets)) {
    const judged = judgeImages(images.map(i => ({ label: i.label, contentType: i.contentType, base64: Buffer.alloc(i.bytes).toString('base64') })));
    assert.deepEqual(replayListing(images).map(l => l.label), judged.left, what);
  }
  const why = (images) => replayListing(images).map(l => [l.label, l.why, l.served_to_public]);
  assert.deepEqual(why(sets['a legacy capture over the limit']), [['capture-1.png', 'size', false]]);
  assert.deepEqual(why(sets['five at the cap overflow the budget']), [['capture-4.png', 'budget', true]]);
  assert.deepEqual(why(sets['a type the API does not take']), [['capture-0.png', 'type', true]]);
  assert.deepEqual(why(sets['past the count']).map(([label, reason]) => [label, reason]), [['capture-10.png', 'count'], [THUMB_LABEL, 'count']]);
  assert.deepEqual(why(sets['all small']), []);
  assert.deepEqual([typeOfBytes(pngOf(400)), typeOfBytes(new TextEncoder().encode('<svg/>'))], ['image/png', null]);
});

test('the report names every approved listing with any image left out, and why; nothing else counts', () => {
  const approved = (name) => ({ name, author_id: AUTHOR, moderation: { status: 'approved' } });
  const img = (kind, label, bytes, contentType = 'image/png') => ({ kind, key: label, label, contentType, bytes });
  const report = summarize([
    { id: 'a', sim: approved('A'), binding: { v: 1 }, images: [img('capture', 'capture-0.png', 10), img('thumbnail', THUMB_LABEL, LEGACY_OVERSIZE)] },
    { id: 'b', sim: approved('B'), binding: { v: 1 }, images: [img('capture', 'capture-0.png', 10), img('cover', COVER_LABEL, 10)] },
    { id: 'c', sim: { moderation: { status: 'held' } }, binding: null, images: [] },
    { id: 'd', sim: approved('D'), binding: null, images: [img('capture', 'capture-0.png', LEGACY_OVERSIZE)] },
    { id: 'e', sim: approved('E'), binding: { revoked: true }, images: [] },
    { id: 'f', sim: approved('F'), binding: { v: 1 }, images: [0, 1, 2, 3, 4].map(n => img('capture', `capture-${n}.png`, JUDGE_IMAGE_MAX_BYTES)).concat([img('cover', COVER_LABEL, 10, 'image/svg+xml')]) },
  ], { approvedIndexed: 6 });
  assert.deepEqual(report.flagged.map(f => [f.sim_id, f.images.map(i => [i.kind, i.why])]), [['a', [['thumbnail', 'size']]], ['f', [['capture', 'budget'], ['cover', 'type']]]]);
  assert.deepEqual(report.by_kind, { capture: 1, thumbnail: 1, cover: 1 });
  assert.deepEqual(report.by_reason, { size: 1, budget: 1, type: 1 });
  assert.deepEqual([report.checked, report.skipped], [3, { not_approved: 1, unbound: 1, revoked: 1 }]);
  // An empty answer is an answer, not an error.
  assert.deepEqual(summarize([]).flagged, []);
  assert.deepEqual(pageOf(['c', 'a', 'b', 'd'], { limit: 2 }), { page: ['a', 'b'], next_after: 'b' });
  assert.deepEqual(pageOf(['c', 'a', 'b', 'd'], { after: 'b', limit: 2 }), { page: ['c', 'd'], next_after: null });
  assert.deepEqual(imageKeys('a', { evidence_epoch: 2, captures: { 1: 'sha256:x' } }).filter(k => k.kind === 'capture').map(k => k.key), ['universes/a/moderation/e2/capture-1.png', 'universes/a/moderation/e2/capture-1.jpg', 'universes/a/moderation/e2/capture-1.webp']);
  assert.equal(parseArgs([]).env, 'staging', 'staging unless production is named');
  assert.throws(() => parseArgs(['--env=prod']));
  assert.throws(() => parseArgs(['--limit=501']));
});

test('the report runs only reads, through node with no shell, and refuses a write before anything is spawned', async () => {
  const spawned = [];
  const fakeSpawn = (cmd, args, opts) => {
    spawned.push({ cmd, args, opts });
    const listeners = {};
    const stream = () => ({ on: () => {} });
    const child = { stdout: stream(), stderr: stream(), on: (ev, fn) => { listeners[ev] = fn; } };
    setImmediate(() => listeners.close?.(0));
    return child;
  };
  const run = makeRunner({ spawnImpl: fakeSpawn, wranglerJs: '/x/wrangler.js', nodePath: '/x/node' });
  const cfg = ENVIRONMENTS.staging;
  for (const write of [['r2', 'object', 'put', 'b/k'], ['r2', 'object', 'delete', 'b/k'], ['kv', 'key', 'put', 'k', 'v'], ['kv', 'key', 'delete', 'k'], ['kv', 'bulk', 'delete']]) {
    assert.throws(() => run(write), /refusing a non-read wrangler call/);
  }
  assert.equal(spawned.length, 0, 'nothing was spawned for a write');
  for (const args of [kvGetArgs(cfg, 'sim:a'), kvListArgs(cfg, 'modq:approved:'), r2GetArgs(cfg, 'thumbnails/a/thumb.png')]) {
    assertReadOnly(args);
    await run(args);
  }
  assert.equal(spawned.length, 3);
  for (const s of spawned) {
    assert.equal(s.cmd, '/x/node');
    assert.equal(s.args[0], '/x/wrangler.js');
    assert.equal(s.opts.shell, false);
    assert.ok(s.args.includes('--env') && s.args.includes('staging'), 'staging by default');
  }
});

// -- B4: a dedup copies an approval only while the case still stands where its record says ----------------------------------------

test('an approval a person later rejected is not copied to a republish of the same package: it is reviewed in full', async () => {
  const env = await environment();
  await listing(env);
  const d = deps();
  assert.equal((await runModerationCase(SIM, env, d)).status, 'approved');
  const exec = makeToolExecutor(env, d, { actor: 'admin', actorId: ADMIN });
  const rejected = await exec('moderation_reject', { sim_id: SIM, lane: 'quality', category: 'low_effort_test', rationale: 'A person judged this to be a test upload with nothing in it.' });
  assert.equal(rejected.ok, true, JSON.stringify(rejected));
  assert.equal((await loadCase(env, SIM)).status, 'rejected');
  await listing(env, { id: SIM2 });
  const second = await runModerationCase(SIM2, env, d);
  assert.equal(second.decision.dedup_of, undefined, 'not a copy');
  assert.equal(d.judge.calls.length, 2, 'the judge read the republish');
});

test('an approval under a later takedown hold, with the status still approved, is not copied either', async () => {
  const env = await environment();
  await listing(env);
  const d = deps();
  assert.equal((await runModerationCase(SIM, env, d)).status, 'approved');
  const rec = await loadCase(env, SIM);
  rec.legal_hold = { category: 'ncii', source: 'takedown', takedowns: ['t1'], at: '2026-10-03T00:00:00.000Z', by: 'takedown' };
  rec.history.push({ at: '2026-10-03T00:00:00.000Z', event: 'takedown_hold' });
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
  await listing(env, { id: SIM2 });
  const second = await runModerationCase(SIM2, env, d);
  assert.equal(second.decision.dedup_of, undefined);
  assert.equal(d.judge.calls.length, 2);
});

// -- B2: every image the judge reads is the object the review recorded --------------------------------------------------------------

/// Storage whose next read of `key` finds `swap` in its place, after which the original is put back: an image replaced and
/// restored while the review ran. `swap` null: the object is gone for that read.
function swapOnce(env, key, swap) {
  const get = env.SCENES.get.bind(env.SCENES);
  let done = false;
  env.SCENES.get = async (k, ...rest) => {
    if (done || k !== key) return get(k, ...rest);
    done = true;
    const original = env.SCENES.objects.get(k);
    if (swap === null) return null;
    await store(env, k, swap);
    const seen = await get(k, ...rest);
    env.SCENES.objects.set(k, original);
    return seen;
  };
}

for (const [what, key, label, extra] of [
  ['thumbnail', `thumbnails/${SIM}/thumb.png`, THUMB_LABEL, {}],
  ['cover', `covers/${SIM}/cover.png`, COVER_LABEL, { cover: pngOf(350) }],
  ['capture', `universes/${SIM}/moderation/capture-1.png`, 'capture-1.png', {}],
]) {
  test(`a ${what} swapped A, B, A during the review is left out, and the case is held`, async () => {
    const env = await environment();
    await listing(env, extra);
    swapOnce(env, key, pngOf(777, 9));
    const d = deps();
    const rec = await runModerationCase(SIM, env, d);
    await assertHeldAndUnserved(env, rec, [label]);
    assert.ok(!rec.judge.captures_used.includes(label), 'B was never sent');
  });

  test(`a ${what} the review recorded that is gone when the judge reads it is left out, and the case is held`, async () => {
    const env = await environment();
    await listing(env, extra);
    swapOnce(env, key, null);
    const rec = await runModerationCase(SIM, env, deps());
    await assertHeldAndUnserved(env, rec, [label]);
  });
}

// -- S1: every stored capture reaches the judge's plan ------------------------------------------------------------------------------

test('a capture past the eighth is named as left out, not cut silently, and the case is held', async () => {
  const env = await environment();
  const captures = Object.fromEntries([0, 1, 2, 3, 4, 5, 6, 7].map(n => [n, pngOf(400 + n)]));
  await listing(env, { captures });
  await store(env, `universes/${SIM}/moderation/capture-8.png`, pngOf(410));
  const rec = await runModerationCase(SIM, env, deps());
  await assertHeldAndUnserved(env, rec, ['capture-8.png']);
  assert.equal(rec.judge.captures_used.length, 9, 'eight captures and the thumbnail');
});

// -- B5: one still picture, the same to every decoder ---------------------------------------------------------------------------------

/// A PNG with extra chunks after its header (an animation control, colour transforms).
function pngWith(chunks, w = 600, h = 800) {
  return concat([Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', [...u32be(w), ...u32be(h), 8, 2, 0, 0, 0]), ...chunks,
    pngChunk('IDAT', [0x78, 0x9c, 0x63, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]), pngChunk('IEND', [])]);
}
const le32 = (n) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];
const le24 = (n) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255];
const riffChunk = (name, data) => [...new TextEncoder().encode(name), ...le32(data.length), ...data, ...(data.length & 1 ? [0] : [])];
const vp8l = (w, h) => riffChunk('VP8L', [0x2f, ...le32(((w - 1) | ((h - 1) << 14)) >>> 0), 0, 0, 0]);
const riff = (...chunks) => { const body = [...new TextEncoder().encode('WEBP'), ...chunks.flat()]; return Uint8Array.from([...new TextEncoder().encode('RIFF'), ...le32(body.length), ...body]); };
const vp8x = (w, h, flags) => riffChunk('VP8X', [flags, 0, 0, 0, ...le24(w - 1), ...le24(h - 1)]);
const APNG = pngWith([pngChunk('acTL', [0, 0, 0, 2, 0, 0, 0, 0]), pngChunk('fcTL', new Array(26).fill(0))]);
const ANIMATED_WEBP = riff(vp8x(600, 800, 0x02), riffChunk('ANIM', [0, 0, 0, 0, 0, 0]), riffChunk('ANMF', [...le24(0), ...le24(0), ...le24(599), ...le24(799), ...le24(40), 0, ...vp8l(600, 800)]));
const COLOURED = pngWith([pngChunk('gAMA', [0, 0, 0x27, 0x10]), pngChunk('cHRM', new Array(32).fill(1)), pngChunk('sRGB', [0]), pngChunk('iCCP', [0x70, 0, 0, 1, 2, 3])]);
const PROFILED_WEBP = riff(vp8x(600, 800, 0x20), riffChunk('ICCP', new Array(64).fill(7)), vp8l(600, 800));
const routes = {
  capture: { path: `/api/simulations/${SIM}/captures/0`, prefix: `universes/${SIM}/moderation/capture-` },
  thumbnail: { path: `/api/simulations/${SIM}/thumbnail`, prefix: `thumbnails/${SIM}/` },
  cover: { path: `/api/simulations/${SIM}/cover`, prefix: `covers/${SIM}/` },
};

for (const status of ['pending', 'approved']) {
  for (const [route, { path, prefix }] of Object.entries(routes)) {
    test(`an animated PNG or WebP ${route} upload is refused with 415 and stores nothing (listing ${status})`, async () => {
      for (const [what, bytes] of [['APNG', APNG], ['animated WebP', ANIMATED_WEBP]]) {
        const env = await environment();
        await listing(env, { captures: {}, thumb: null, status });
        const r = await call(env, 'PUT', path, { who: 'author', body: bytes });
        assert.equal(r.status, 415, `${what}: ${JSON.stringify(r.body)}`);
        assert.deepEqual(keysUnder(env, prefix), [], `${what}: nothing stored`);
      }
    });
  }
}

test('colour transforms are dropped at upload on every route: PNG gAMA, cHRM, sRGB, iCCP and the WebP ICCP chunk', async () => {
  const latin = (bytes) => Buffer.from(bytes).toString('latin1');
  for (const [route, { path, prefix }] of Object.entries(routes)) {
    for (const [what, bytes, names] of [['PNG', COLOURED, ['gAMA', 'cHRM', 'sRGB', 'iCCP']], ['WebP', PROFILED_WEBP, ['ICCP']]]) {
      const env = await environment();
      await listing(env, { captures: {}, thumb: null });
      const r = await call(env, 'PUT', path, { who: 'author', body: bytes });
      assert.equal(r.status, 200, `${route} ${what}: ${JSON.stringify(r.body)}`);
      const [key] = keysUnder(env, prefix);
      const stored = env.SCENES.objects.get(key).body;
      for (const name of names) assert.ok(!latin(stored).includes(name), `${route} ${what}: ${name} is not stored`);
      if (what === 'WebP') assert.equal(stored[20] & 0x20, 0, `${route}: the extended header no longer promises a profile`);
    }
  }
});

// -- F1(a): a thumbnail change sends an approved listing back to review, binding or not -----------------------------------------------

for (const bound of [false, true]) {
  test(`a thumbnail uploaded to an approved listing sends it back to review (${bound ? 'bound' : 'no binding'}): pending, out of the Gallery`, async () => {
    const env = await environment();
    await listing(env);
    if (bound) await approve(env);
    else {
      const sim = await record(env);
      sim.moderation = { status: 'approved', rating: 'all_ages' };
      await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
    }
    assert.equal((await call(env, 'GET', '/api/simulations')).body.simulations.length, 1, 'the control: listed');
    const r = await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: pngOf(900, 3) });
    assert.equal(r.status, 200);
    assert.equal((await record(env)).moderation.status, 'pending');
    assert.equal((await call(env, 'GET', '/api/simulations')).body.simulations.length, 0, 'not listed until a review passes it');
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 404, 'and the new image is not served to the public');
  });
}

// -- F1(b): the binding sweep binds what the review recorded, and holds a listing whose stored art differs -----------------------

/// An approved listing whose binding is gone: as a listing approved before bindings existed reads.
async function approvedUnbound(env) {
  await listing(env);
  assert.equal((await runModerationCase(SIM, env, deps())).status, 'approved');
  env.SCENES.objects.delete(`locks/approved/${SIM}.json`);
  return loadCase(env, SIM);
}

test('the binding sweep binds the objects the review recorded when they are still what is stored', async () => {
  const env = await environment();
  const rec = await approvedUnbound(env);
  await sweepLocks(env);
  const binding = JSON.parse(await (await env.SCENES.get(`locks/approved/${SIM}.json`)).text());
  assert.equal(binding.by, 'backfill');
  for (const k of ['content', 'thumb', 'cover', 'source']) assert.equal(binding[k] ?? null, rec.reviewed[k] ?? null, k);
  assert.deepEqual(binding.captures, rec.reviewed.captures);
  assert.equal((await loadCase(env, SIM)).status, 'approved');
});

test('the binding sweep holds for a person a listing whose stored art is not what the review recorded, and binds nothing', async () => {
  const env = await environment();
  await approvedUnbound(env);
  // The thumbnail swapped while the listing had no binding (written straight to storage, as an unbound listing served it).
  await store(env, `thumbnails/${SIM}/thumb.png`, pngOf(901, 5));
  await sweepLocks(env);
  const rec = await loadCase(env, SIM);
  assert.equal(rec.status, 'held');
  assert.ok(rec.decision.reasons.some(r => r.code === 'stored_objects_not_reviewed'));
  assert.equal((await record(env)).moderation.status, 'held');
  const binding = JSON.parse(await (await env.SCENES.get(`locks/approved/${SIM}.json`)).text());
  assert.equal(binding.revoked, true, 'no approval is bound to what was never judged');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 404);
});

test('the binding sweep binds what is stored only for an approved listing with no recorded review (the control)', async () => {
  const env = await environment();
  await listing(env);
  const sim = await record(env);
  sim.moderation = { status: 'approved', rating: 'all_ages' };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  await env.SOCIAL.put(`modq:approved:${SIM}`, '2026-01-01T00:00:00.000Z');
  await sweepLocks(env);
  const binding = JSON.parse(await (await env.SCENES.get(`locks/approved/${SIM}.json`)).text());
  assert.equal(binding.by, 'backfill');
  assert.equal(binding.thumb, (await snapshotReviewed(env, sim, 0)).thumb);
});

// -- F2: transparency that can be told from the header and chunks is refused; Studio's RGBA PNG is accepted -----------------------

/// A PNG of colour type `type` (2 RGB, 4 grey and alpha, 6 RGBA) with real, deflated, filtered rows: `alpha` for every pixel.
function realPng(w, h, { type = 6, alpha = 255, chunks = [] } = {}) {
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[type];
  const row = new Uint8Array(1 + w * channels);
  for (let x = 0; x < w; x++) { for (let c = 0; c < channels; c++) row[1 + x * channels + c] = 90 + c; if (type === 4 || type === 6) row[x * channels + channels] = alpha; }
  const raw = new Uint8Array(row.length * h);
  for (let y = 0; y < h; y++) raw.set(row, y * row.length);
  return concat([Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', [...u32be(w), ...u32be(h), 8, type, 0, 0, 0]), ...chunks,
    pngChunk('IDAT', [...zlib.deflateSync(raw)]), pngChunk('IEND', [])]);
}
const vp8 = (w, h) => riffChunk('VP8 ', [0x50, 0x01, 0x00, 0x9d, 0x01, 0x2a, w & 255, (w >>> 8) & 0x3f, h & 255, (h >>> 8) & 0x3f, 0, 0, 0, 0, 0, 0, 0, 0]);
const vp8lAlpha = (w, h) => riffChunk('VP8L', [0x2f, ...le32((((w - 1) | ((h - 1) << 14)) | (1 << 28)) >>> 0), 0, 0, 0]);
const TRANSPARENT_ART = {
  'a grey-and-alpha PNG (colour type 4)': realPng(600, 800, { type: 4 }),
  'a PNG with a transparency chunk (tRNS)': realPng(600, 800, { type: 2, chunks: [pngChunk('tRNS', [0, 90, 0, 91, 0, 92])] }),
  'a WebP with an ALPH chunk, whatever its header says': riff(vp8x(600, 800, 0), riffChunk('ALPH', [0, 1, 2, 3]), vp8(600, 800)),
  'a WebP whose extended header says alpha': riff(vp8x(600, 800, 0x10), vp8(600, 800)),
  'a lossless WebP whose alpha-is-used bit is set': riff(vp8lAlpha(600, 800)),
};

for (const [route, { path, prefix }] of Object.entries(routes)) {
  for (const status of ['pending', 'approved']) {
    test(`transparent listing art is refused with 415 on the ${route} route, and nothing is stored (listing ${status})`, async () => {
      for (const [what, bytes] of Object.entries(TRANSPARENT_ART)) {
        const env = await environment();
        await listing(env, { captures: {}, thumb: null, status });
        const r = await call(env, 'PUT', path, { who: 'author', body: bytes });
        assert.equal(r.status, 415, `${what}: ${JSON.stringify(r.body)}`);
        assert.deepEqual(keysUnder(env, prefix), [], `${what}: nothing stored`);
      }
    });
  }
}

test("Studio's own output, an opaque 8-bit RGBA PNG at 1280x720, is accepted as a capture and a thumbnail; an RGBA cover is refused; a background hint is dropped", async () => {
  const studio = realPng(1280, 720, { type: 6, alpha: 255 });
  for (const [route, { path, prefix }] of Object.entries(routes)) {
    const env = await environment();
    await listing(env, { captures: {}, thumb: null });
    const r = await call(env, 'PUT', path, { who: 'author', body: studio });
    // Studio never makes a cover (it uploads the author's file), so an RGBA cover is refused with no publish to break.
    if (route === 'cover') {
      assert.deepEqual([r.status, r.body.code], [415, 'cover_bad_type'], JSON.stringify(r.body));
      assert.deepEqual(keysUnder(env, prefix), []);
    } else {
      assert.equal(r.status, 200, `${route}: ${JSON.stringify(r.body)}`);
      assert.equal(keysUnder(env, prefix).length, 1, route);
    }
    const hinted = await environment();
    await listing(hinted, { captures: {}, thumb: null });
    assert.equal((await call(hinted, 'PUT', path, { who: 'author', body: realPng(1280, 720, { type: route === 'cover' ? 2 : 6, chunks: [pngChunk('bKGD', [0, 0, 0, 0, 0, 0])] }) })).status, 200, route);
    const [key] = keysUnder(hinted, prefix);
    assert.ok(!Buffer.from(hinted.SCENES.objects.get(key).body).toString('latin1').includes('bKGD'), `${route}: bKGD is not stored`);
  }
});

// -- F3: legacy approved art an upload refuses or changes today is not served to the public ----------------------------------------

/// A GIF of `frames` frames, block by block.
function gif(frames) {
  const parts = [...new TextEncoder().encode('GIF89a'), 2, 0, 2, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255];
  for (let f = 0; f < frames; f++) parts.push(0x21, 0xf9, 4, 0, 10, 0, 0, 0, 0x2c, 0, 0, 0, 0, 2, 0, 2, 0, 0, 2, 2, 0x44, 0x01, 0);
  parts.push(0x3b);
  return Uint8Array.from(parts);
}
const CLEAN_ART = realPng(600, 800, { type: 6 });
const COLOURED_ART = realPng(600, 800, { type: 2, chunks: [pngChunk('gAMA', [0, 0, 0x27, 0x10])] });
const LEGACY_ART = {
  'an APNG': realPng(600, 800, { type: 6, chunks: [pngChunk('acTL', [0, 0, 0, 2, 0, 0, 0, 0])] }),
  'transparency (tRNS)': realPng(600, 800, { type: 2, chunks: [pngChunk('tRNS', [0, 90, 0, 91, 0, 92])] }),
  'an animated GIF': gif(3),
};

for (const [what, bytes] of Object.entries(LEGACY_ART)) {
  test(`an approved listing whose bound thumbnail and cover carry ${what}: absent to the public, served privately to its author and an admin`, async () => {
    const env = await environment();
    await listing(env, { thumb: bytes, cover: bytes });
    await approve(env);
    for (const who of [undefined, 'stranger']) {
      const t = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who });
      assert.deepEqual([t.status, t.body], [404, { error: 'Thumbnail not found' }], `thumbnail, ${who || 'the public'}`);
      const c = await call(env, 'GET', `/api/simulations/${SIM}/cover`, { who });
      assert.deepEqual([c.status, c.body], [404, { error: 'No cover' }], `cover, ${who || 'the public'}`);
    }
    for (const who of ['author', 'admin']) {
      const t = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIM}/thumbnail`, { headers: { Authorization: `Bearer ${tokens[who]}` } }), env, ctx);
      assert.equal(t.status, 200, `thumbnail, ${who}`);
      assert.equal(t.headers.get('Cache-Control'), 'private, no-store', `thumbnail, ${who}: never cached for anyone else`);
      assert.equal((await t.arrayBuffer()).byteLength, bytes.length);
      assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/cover`, { who })).status, 200, `cover, ${who}`);
    }
  });
}

test('the control: an approved listing whose bound art an upload would store as it is serves it to the public, cached', async () => {
  const env = await environment();
  await listing(env, { thumb: CLEAN_ART, cover: CLEAN_ART });
  await approve(env);
  const t = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIM}/thumbnail`), env, ctx);
  assert.equal(t.status, 200);
  assert.equal(t.headers.get('Cache-Control'), 'public, max-age=3600');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/cover`)).status, 200);
});

test('an approved listing whose bound capture an upload would refuse: no public preview of it; staff still read it', async () => {
  const env = await environment();
  await listing(env, { captures: { 0: CLEAN_ART, 1: LEGACY_ART['transparency (tRNS)'] } });
  await approve(env);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/preview/0`)).status, 200);
  assert.deepEqual((await call(env, 'GET', `/api/simulations/${SIM}/preview/1`)).body, { error: 'No preview' });
  assert.equal((await call(env, 'GET', `/api/admin/moderation/captures/${SIM}/1`, { who: 'admin' })).status, 200);
});

test("the author's view of a thumbnail no review passed is private and never stored (Cache-Control)", async () => {
  const env = await environment();
  await listing(env, { thumb: CLEAN_ART });
  // Rated, so the image is not adult content, which only a signed URL serves (src/adult_content.mjs).
  const sim = await record(env);
  sim.moderation = { status: 'pending', rating: 'all_ages' };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  const r = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIM}/thumbnail`, { headers: { Authorization: `Bearer ${tokens.author}` } }), env, ctx);
  assert.equal(r.status, 200);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
});

test('the report flags bound art an upload refuses or changes today, and the replay still matches judgeImages', () => {
  assert.equal(artProblem(CLEAN_ART), null);
  assert.equal(artProblem(COLOURED_ART), 'dropped_at_upload');
  assert.equal(artProblem(LEGACY_ART['an APNG']), 'animated');
  assert.equal(artProblem(LEGACY_ART['transparency (tRNS)']), 'transparent');
  assert.equal(artProblem(gif(3)), 'animated_gif');
  assert.equal(artProblem(gif(1)), 'gif');
  assert.deepEqual([gifFrames(gif(3)), gifFrames(gif(1)), gifFrames(CLEAN_ART)], [3, 1, 0]);
  const img = (label, bytes, problem = null) => ({ kind: 'capture', key: label, label, contentType: 'image/png', bytes, problem });
  const left = replayListing([img('capture-0.png', 400), img('capture-1.png', 400, 'animated'), img('capture-2.png', 400, 'dropped_at_upload')]);
  assert.deepEqual(left.map(l => [l.label, l.why, l.problem, l.served_to_public, l.served_as]), [['capture-1.png', null, 'animated', false, null], ['capture-2.png', null, 'dropped_at_upload', true, 'stripped']]);
  const report = summarize([{ id: 'a', sim: { name: 'A', moderation: { status: 'approved' } }, binding: { v: 1 }, images: [img('capture-0.png', 400, 'transparent')] }]);
  assert.deepEqual([report.flagged.length, report.by_problem], [1, { served_stripped: {}, refused: { transparent: 1 } }]);
  const both = summarize([{ id: 'b', sim: { name: 'B', moderation: { status: 'approved' } }, binding: { v: 1 }, images: [img('capture-0.png', 400, 'dropped_at_upload'), img('capture-1.png', 400, 'animated')] }]);
  assert.deepEqual(both.by_problem, { served_stripped: { dropped_at_upload: 1 }, refused: { animated: 1 } }, 'colour chunks alone are counted apart from refusals');
});

// -- F5 and F6: a dedup copies only an identity-checked approval that R2 still stands behind ------------------------------------------

for (const [what, spoil] of [
  ['its run predates the identity check (no marker)', async (env) => { const rec = await loadCase(env, SIM); delete rec.images_checked; await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec)); }],
  ['its approval in R2 is revoked while the KV case still says approved', async (env) => { await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, revoked: true })); }],
  ['a legal lock is in R2 while the KV case says nothing of it', async (env) => { await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: 'content_lock' }), { customMetadata: { kind: 'content_lock' } }); }],
]) {
  test(`an approval is not copied to a republish of the same package when ${what}`, async () => {
    const env = await environment();
    await listing(env);
    const d = deps();
    assert.equal((await runModerationCase(SIM, env, d)).status, 'approved');
    assert.equal((await loadCase(env, SIM)).images_checked, IMAGES_CHECKED, 'the control: the run recorded its check');
    await spoil(env);
    await listing(env, { id: SIM2 });
    const second = await runModerationCase(SIM2, env, d);
    assert.equal(second.decision.dedup_of, undefined);
    assert.equal(d.judge.calls.length, 2, 'the republish was judged');
  });
}

// -- the judge is told why each image was left out --------------------------------------------------------------------------------------

test('the judge is told the real reason each image was left out', () => {
  const ok = (label, size = 8) => ({ label, contentType: 'image/png', base64: 'A'.repeat(size) });
  const text = judgeRequest({ summary: {}, captures: [ok('a'), ok('big', JUDGE_IMAGE_MAX_B64 + 1), { label: 'bmp', contentType: 'image/bmp', base64: 'AAAA' }, { label: 'gone', contentType: null, base64: null }], policyText: 'P', policyHash: 'h' }).messages[0].content.at(-1).text;
  assert.match(text, /Left out, not sent: big \(too large to send\); bmp \(a file type the judge cannot read\); gone \(not readable, or not the image the review recorded\)\. Do not score anything only they would show\./);
  const budget = judgeRequest({ summary: {}, captures: [0, 1, 2, 3, 4].map(n => ok(`c${n}`, 4_900_000)), policyText: 'P', policyHash: 'h' }).messages[0].content.at(-1).text;
  assert.match(budget, /c4 \(past the request's budget for images\)/);
});

// -- one picture per file -----------------------------------------------------------------------------------------------------------

test('a file that holds a second picture is refused for listing art: a JFIF thumbnail, a second frame header, a second WebP image', async () => {
  const seg = (marker, data) => [0xff, marker, ((data.length + 2) >>> 8) & 255, (data.length + 2) & 255, ...data];
  const ascii = (t) => [...new TextEncoder().encode(t)];
  const sof = [8, 0, 50, 0, 100, 1, 1, 0x11, 0];
  const jpegWith = (...segments) => Uint8Array.from([0xff, 0xd8, ...segments.flat(), ...seg(0xda, [1, 1, 0, 0, 63, 0]), 0x12, 0x34, 0xff, 0xd9]);
  const jfif = (tw, th, extra = []) => seg(0xe0, [...ascii('JFIF'), 0, 1, 1, 0, 0, 1, 0, 1, tw, th, ...extra]);
  const cases = {
    'a JFIF header carrying a thumbnail': jpegWith(jfif(1, 1, [1, 2, 3]), seg(0xc0, sof)),
    'two JPEG frame headers': jpegWith(jfif(0, 0), seg(0xc0, sof), seg(0xc2, sof)),
    'two WebP image chunks': riff(vp8x(600, 800, 0), vp8(600, 800), vp8(600, 800)),
  };
  for (const [what, bytes] of Object.entries(cases)) {
    assert.equal(imageProblem(bytes), MULTIPLE_IMAGE_PROBLEM, what);
    assert.deepEqual([(await prepareUserImage(bytes)).ok, (await prepareUserImage(bytes)).status], [false, 415], what);
  }
  assert.equal(imageProblem(jpegWith(jfif(0, 0), seg(0xc0, sof))), null, 'the control: one JFIF header with no thumbnail and one frame');
  assert.equal(imageProblem(riff(vp8x(600, 800, 0), vp8(600, 800))), null, 'the control: one WebP image chunk');
});

// -- F7: a product's icon is null or one of its own listing's image routes --------------------------------------------------------

test("a product's icon is one of its own listing's image routes on this Worker, or nothing, when it is set and when it is read", () => {
  const own = `https://api.eustress.dev/api/simulations/${SIM}`;
  for (const icon of [`${own}/thumbnail`, `${own}/cover`, `${own}/preview/3`]) assert.equal(listingIcon(icon, SIM), icon, icon);
  // Only this environment's own host: production's api, staging's api-staging, never the other.
  const stagingIcon = `https://api-staging.eustress.dev/api/simulations/${SIM}/cover`;
  assert.equal(listingIcon(stagingIcon, SIM), null, 'a staging address in production');
  assert.equal(listingIcon(stagingIcon, SIM, apiOriginOf({ PUBLIC_API_ORIGIN: 'https://api-staging.eustress.dev' })), stagingIcon);
  assert.equal(listingIcon(`${own}/cover`, SIM, apiOriginOf({ PUBLIC_API_ORIGIN: 'https://api-staging.eustress.dev' })), null, 'a production address in staging');
  assert.equal(productView({ id: 'prod_x', sim_id: SIM, icon: stagingIcon }, apiOriginOf({})).icon, null);
  for (const icon of [
    'https://cdn.example.com/x.png', `https://forums.eustress.dev/api/simulations/${SIM}/thumbnail`, `https://api.eustress.dev/api/simulations/${SIM2}/thumbnail`,
    `${own}/thumbnail?v=1`, `${own}/preview/8`, `${own}/download`, `http://api.eustress.dev/api/simulations/${SIM}/cover`, '/assets/icons/star.svg', `${own}/thumbnail/x`,
  ]) assert.equal(listingIcon(icon, SIM), null, icon);
  assert.deepEqual(productFields({ icon: 'https://cdn.eustress.dev/x.png' }, { partial: true, simId: SIM }).param, 'icon', 'refused when it is set');
  assert.equal(productFields({ icon: `${own}/cover` }, { partial: true, simId: SIM }).fields.icon, `${own}/cover`);
  // A product stored before the rule reads with no icon.
  assert.equal(productView({ id: 'prod_x', sim_id: SIM, icon: 'https://media.eustress.dev/anything.png' }).icon, null);
  assert.equal(productSummary({ id: 'prod_x', sim_id: SIM, icon: 'https://media.eustress.dev/anything.png' }).icon, null);
  assert.equal(productView({ id: 'prod_x', sim_id: SIM, icon: `${own}/thumbnail` }).icon, `${own}/thumbnail`);
});

// -- N4: avatars in community search and the leaderboard follow the website's rule ------------------------------------------------

test('community search and the leaderboard carry an avatar only by the website\'s rule: a path under /assets, never an address on a host', async () => {
  const env = await environment();
  const people = [['ada', '/assets/avatars/ada.png'], ['adam', 'https://cdn.example.com/adam.png'], ['adaline', 'https://api.eustress.dev/api/simulations/x/thumbnail']];
  for (const [i, [username, avatar_url]] of people.entries()) {
    const id = `4444444${i}-1111-4111-8111-111111111111`;
    await env.USERS.put(`user:${id}`, JSON.stringify({ id, username, avatar_url, kyc_verified: true, created_at: '2026-01-01T00:00:00.000Z' }));
    await env.USERS.put(`username:${username}`, id);
  }
  const found = Object.fromEntries((await call(env, 'GET', '/api/community/search?q=ada')).body.users.map(u => [u.username, u.avatar_url]));
  assert.deepEqual(found, { ada: '/assets/avatars/ada.png', adam: null, adaline: null });
  const board = await call(env, 'GET', '/api/community/leaderboard');
  const ranked = Object.fromEntries(board.body.entries.map(u => [u.username, u.avatar_url]));
  assert.equal(ranked.ada, '/assets/avatars/ada.png');
  assert.equal(ranked.adam, null);
  assert.equal(ranked.adaline, null);
});

// =====================================================================================================================================
// 2c-3
// =====================================================================================================================================

// -- 1: Studio's republish order on an approved listing --------------------------------------------------------------------------

/// Run `fn` with the clock stopped. An upload measures an RGBA PNG's alpha within production's wall-clock budget
/// (ALPHA_MEASURE_BUDGET_MS, 250 ms), so on a busy machine the same upload can record 'unknown' instead of false. With the clock
/// stopped the budget never runs out, so what a test reads back does not depend on how fast the machine is. Date.now is the
/// clock the measure reads; production's budget is untouched.
async function withStoppedClock(fn) {
  const real = Date.now;
  const at = real();
  Date.now = () => at;
  try { return await fn(); } finally { Date.now = real; }
}

test("Studio's republish order on an approved listing (world unchanged, then 8 captures, then the thumbnail) is judged in full", async () => {
  const env = await environment();
  await listing(env);
  const d = deps();
  assert.equal((await runModerationCase(SIM, env, d)).status, 'approved');
  // The world commit changed nothing, so nothing sent the listing back. Studio then sends the orbit's captures (its real
  // 1280x720 RGBA PNGs, measured with the clock stopped so the measure is the same on any machine)...
  await withStoppedClock(async () => {
    for (let n = 0; n < 8; n++) {
      const r = await call(env, 'PUT', `/api/simulations/${SIM}/captures/${n}`, { who: 'author', body: realPng(1280, 720, { type: 6, alpha: 255, chunks: [pngChunk('pHYs', [0, 0, 0, n + 1, 0, 0, 0, n + 1, 1])] }) });
      assert.equal(r.status, 200, `capture ${n}: ${JSON.stringify(r.body)}`);
    }
    // ...then the thumbnail, then the dossier, then submits.
    assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: realPng(1280, 720, { type: 6, alpha: 255 }) })).status, 200);
  });
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/dossier`, { who: 'author', body: JSON.stringify(dossier()) })).status, 200);
  assert.equal((await record(env)).moderation.status, 'pending', 'back in review');
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'approved', JSON.stringify(rec.decision?.reasons));
  assert.equal(rec.evidence_epoch, 1, 'the captures went to the next epoch');
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL].concat([0, 1, 2, 3, 4, 5, 6, 7].map(n => `capture-${n}.png`)));
  assert.deepEqual(rec.judge.captures_left_out, []);
  assert.equal(sentImages(d, 1), 9);
  // The measure decides nothing, but with the clock stopped it reads the same everywhere: every one of Studio's images is opaque.
  assert.deepEqual(Object.values(rec.alpha_used), Array(9).fill(false));
});

test('a capture to a locked or quarantined listing is still refused, and its status is not touched', async () => {
  for (const [what, setup] of [
    ['quarantined', async (env) => { const sim = await record(env); sim.moderation = { status: 'quarantined', rating: 'all_ages' }; await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim)); }],
    ['under a legal lock', async (env) => { await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: 'content_lock' }), { customMetadata: { kind: 'content_lock' } }); }],
  ]) {
    const env = await environment();
    await listing(env, { status: 'approved' });
    await setup(env);
    const before = (await record(env)).moderation.status;
    const r = await call(env, 'PUT', `/api/simulations/${SIM}/captures/0`, { who: 'author', body: pngOf(600) });
    assert.equal(r.status, 409, `${what}: ${JSON.stringify(r.body)}`);
    assert.equal((await record(env)).moderation.status, before, what);
  }
});

// -- 2 and 3: the verdict is decided once; public reads stream it -----------------------------------------------------------------

/// Count how many times each key's bytes are read whole (arrayBuffer): a walk needs them, a stream does not.
function countWholeReads(env) {
  const counts = {};
  const get = env.SCENES.get.bind(env.SCENES);
  env.SCENES.get = async (key, ...rest) => {
    const o = await get(key, ...rest);
    if (!o) return o;
    const whole = o.arrayBuffer;
    return { ...o, body: o.body, arrayBuffer: async () => { counts[key] = (counts[key] || 0) + 1; return whole(); } };
  };
  return counts;
}

test('an uploaded thumbnail has its verdict from the upload: no public read ever reads its bytes whole', async () => {
  const env = await environment();
  await listing(env, { thumb: null });
  assert.equal((await withStoppedClock(() => call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: realPng(1280, 720) }))).status, 200);
  await approve(env);
  const key = keysUnder(env, `thumbnails/${SIM}/`)[0];
  const verdict = JSON.parse(await (await env.SCENES.get(imageVerdictKey(identOfObject(await env.SCENES.head(key))))).text());
  assert.deepEqual([verdict.verdict, verdict.by, verdict.alpha_used], ['ok', 'upload', false]);
  const counts = countWholeReads(env);
  for (let i = 0; i < 3; i++) assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 200);
  assert.equal(counts[key] ?? 0, 0, 'streamed every time');
});

test('a legacy thumbnail with no verdict is read whole once, on its first public read, and streamed after', async () => {
  const env = await environment();
  await listing(env, { thumb: CLEAN_ART });
  await approve(env);
  const counts = countWholeReads(env);
  const key = `thumbnails/${SIM}/thumb.png`;
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 200);
  assert.equal(counts[key], 1, 'the first public read decides the verdict');
  for (let i = 0; i < 3; i++) assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status, 200);
  assert.equal(counts[key], 1, 'and no later read walks or buffers it');
});

test('a legacy colour-chunk thumbnail is served stripped from a derived copy; its bound object is never written, and the binding holds', async () => {
  const env = await environment();
  await listing(env, { thumb: COLOURED_ART });
  await approve(env);
  const key = `thumbnails/${SIM}/thumb.png`;
  const before = env.SCENES.objects.get(key);
  const head = await env.SCENES.head(key);
  const stripped = storedImageVerdict(COLOURED_ART).bytes;
  for (let i = 0; i < 2; i++) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
    assert.equal(r.status, 200);
    assert.deepEqual([...r.bytes], [...stripped], `read ${i + 1}: the rebuilt copy, colour transform dropped`);
    assert.ok(!Buffer.from(r.bytes).toString('latin1').includes('gAMA'));
  }
  const after = env.SCENES.objects.get(key);
  const headAfter = await env.SCENES.head(key);
  assert.equal(after, before, 'the same stored record');
  assert.deepEqual([...after.body], [...COLOURED_ART], 'the same bytes');
  assert.deepEqual([identOfObject(headAfter), headAfter.etag, headAfter.version], [identOfObject(head), head.etag, head.version], 'the same sha256, etag and version');
  assert.equal(await thumbnailApproved(env, await record(env)), true, 'the binding still matches');
  const copies = keysUnder(env, 'derived/stripped/');
  assert.equal(copies.length, 1, 'one derived copy, written once');
  // The author of a listed, approved listing sees what the public sees.
  assert.deepEqual([...(await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who: 'author' })).bytes], [...stripped]);
});

test('a legacy colour-chunk cover and preview are served stripped too', async () => {
  const env = await environment();
  await listing(env, { captures: { 0: COLOURED_ART }, cover: COLOURED_ART });
  await approve(env);
  const stripped = [...storedImageVerdict(COLOURED_ART).bytes];
  assert.deepEqual([...(await call(env, 'GET', `/api/simulations/${SIM}/cover`)).bytes], stripped);
  assert.deepEqual([...(await call(env, 'GET', `/api/simulations/${SIM}/preview/0`)).bytes], stripped);
});

// -- 4: a refused first-format thumbnail is no thumbnail, never a stale other format -----------------------------------------------

test('a public viewer is not served a stale other-format thumbnail when the first one is refused', async () => {
  const env = await environment();
  await listing(env, { thumb: null });
  const sim = await record(env);
  sim.moderation = { status: 'approved', rating: 'all_ages' };
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
  // No binding: an approval from before bindings. The served format is webp; an old png is still stored.
  await store(env, `thumbnails/${SIM}/thumb.webp`, ANIMATED_WEBP);
  await store(env, `thumbnails/${SIM}/thumb.png`, CLEAN_ART);
  const r = await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`);
  assert.deepEqual([r.status, r.body], [404, { error: 'Thumbnail not found' }]);
});

// -- 5: the sweep revokes when no approved case stands behind what is stored --------------------------------------------------------

for (const [what, change] of [
  ['the stored art differs', async (env) => { await store(env, `thumbnails/${SIM}/thumb.png`, pngOf(902, 6)); }],
  ['the stored art is what was recorded', async () => {}],
]) {
  test(`the binding sweep revokes an approved-looking listing whose case is not approved when ${what}`, async () => {
    const env = await environment();
    const rec = await approvedUnbound(env);
    rec.status = 'rejected';
    await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify(rec));
    await change(env);
    await sweepLocks(env);
    const binding = JSON.parse(await (await env.SCENES.get(`locks/approved/${SIM}.json`)).text());
    assert.equal(binding.revoked, true);
    assert.equal((await simForServing(env, await record(env))).moderation.status, 'pending', 'nothing is served under it');
  });
}

// -- alpha, measured at upload ----------------------------------------------------------------------------------------------------

/// An RGBA PNG of w x h at `depth` 8 or 16 whose rows use each filter type in turn (None, Sub, Up, Average, Paeth), every
/// pixel opaque except `clear` ([x, y] pixels whose alpha is one below full). `idat` replaces the image data.
function filteredPng(w, h, { depth = 8, clear = [], idat = null, interlace = 0, idatSplit = 1 } = {}) {
  const bpp = 4 * (depth / 8), stride = w * bpp;
  const raw = [];
  for (let y = 0; y < h; y++) {
    const row = new Uint8Array(stride);
    for (let x = 0; x < w; x++) {
      for (let c = 0; c < bpp; c++) row[x * bpp + c] = (x * 7 + y * 13 + c * 29) & 255;
      for (let k = 0; k < depth / 8; k++) row[x * bpp + 3 * (depth / 8) + k] = 255;
      if (clear.some(([cx, cy]) => cx === x && cy === y)) row[x * bpp + bpp - 1] = 254;
    }
    raw.push(row);
  }
  const out = [];
  for (let y = 0; y < h; y++) {
    const f = y % 5, row = raw[y], up = y ? raw[y - 1] : new Uint8Array(stride), line = new Uint8Array(1 + stride);
    line[0] = f;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? row[x - bpp] : 0, b = up[x], c = x >= bpp ? up[x - bpp] : 0;
      const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c);
      const pred = f === 0 ? 0 : f === 1 ? a : f === 2 ? b : f === 3 ? (a + b) >>> 1 : (pa <= pb && pa <= pc ? a : pb <= pc ? b : c);
      line[1 + x] = (row[x] - pred) & 255;
    }
    out.push(line);
  }
  const data = idat || zlib.deflateSync(Buffer.concat(out.map(l => Buffer.from(l))));
  const parts = [];
  const size = Math.ceil(data.length / idatSplit);
  for (let i = 0; i < data.length; i += size) parts.push(pngChunk('IDAT', [...data.subarray(i, i + size)]));
  return concat([Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), pngChunk('IHDR', [...u32be(w), ...u32be(h), depth, 6, 0, 0, interlace]), ...parts, pngChunk('IEND', [])]);
}

// No time budget in these (budgetMs: Infinity): what they test is the unfiltering, not how fast a busy machine runs it. The
// budget has its own test below, with an injected clock. Production keeps ALPHA_MEASURE_BUDGET_MS (250 ms).
const NO_BUDGET = { budgetMs: Infinity };

test('the alpha measure unfilters every filter type: opaque reads false, one pixel below full reads true, at depth 8 and 16', async () => {
  for (const depth of [8, 16]) {
    const opaque = await measurePngAlpha(filteredPng(37, 25, { depth, idatSplit: 3 }), NO_BUDGET);
    assert.deepEqual([opaque.alpha_used, opaque.reason], [false, 'opaque'], `depth ${depth}`);
    for (const [x, y] of [[0, 0], [36, 4], [18, 24]]) {
      const used = await measurePngAlpha(filteredPng(37, 25, { depth, clear: [[x, y]] }), NO_BUDGET);
      assert.deepEqual([used.alpha_used, used.row], [true, y], `depth ${depth}, pixel ${x},${y}`);
    }
  }
  // Studio's own output: an opaque 1280x720 RGBA PNG.
  assert.equal((await measurePngAlpha(realPng(1280, 720, { type: 6, alpha: 255 }), NO_BUDGET)).alpha_used, false);
  assert.equal((await measurePngAlpha(realPng(64, 64, { type: 6, alpha: 0 }), NO_BUDGET)).alpha_used, true);
});

test('a deflate bomb is stopped at the size its header states, recorded unknown, and never more than two rows are held', async () => {
  const w = 16, h = 16, stride = w * 4;
  // Opaque rows (filter None, every byte 255), far more of them than the header's 16: the data claims to be an opaque image.
  const opaqueRows = (stride, count) => { const row = Buffer.alloc(1 + stride, 255); row[0] = 0; return Buffer.concat(Array.from({ length: count }, () => row)); };
  const bomb = filteredPng(w, h, { idat: zlib.deflateSync(opaqueRows(stride, 160_000)) });
  assert.ok(bomb.length < 64 * 1024, 'a small file');
  const r = await measurePngAlpha(bomb, NO_BUDGET);
  assert.deepEqual([r.alpha_used, r.reason], ['unknown', 'over_ihdr_size']);
  assert.ok(r.inflated <= h * (1 + stride) + 256 * 1024, `stopped near the stated size, not at 10 MB: ${r.inflated}`);
  assert.ok(r.held_max <= 2 * (1 + stride), `two rows held at most: ${r.held_max}`);
  const big = await measurePngAlpha(realPng(1280, 720, { type: 6, alpha: 255 }), NO_BUDGET);
  assert.ok(big.held_max <= 2 * (1 + 1280 * 4), `two rows of a 1280-wide image, not the image: ${big.held_max}`);
  // Short, interlaced, over the time budget: unknown.
  assert.equal((await measurePngAlpha(filteredPng(w, h, { idat: zlib.deflateSync(opaqueRows(stride, 3)) }), NO_BUDGET)).reason, 'short_stream');
  assert.equal((await measurePngAlpha(filteredPng(w, h, { interlace: 1 }), NO_BUDGET)).reason, 'interlaced');
  let t = 0;
  assert.equal((await measurePngAlpha(filteredPng(64, 64, { idatSplit: 8 }), { now: () => (t += 1000), budgetMs: 10 })).reason, 'time_budget');
});

test('the measure is recorded at upload and on the case for staff, decides nothing, and the author is told nothing of it', async () => {
  const env = await environment();
  await listing(env, { captures: {}, thumb: null });
  // Small images, measured with the clock stopped: what is recorded does not depend on the machine.
  await withStoppedClock(async () => {
    assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/captures/0`, { who: 'author', body: filteredPng(48, 32, { clear: [[5, 5]] }) })).status, 200, 'not refused');
    assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/captures/1`, { who: 'author', body: filteredPng(48, 32) })).status, 200);
  });
  const row = Buffer.alloc(1 + 1280 * 4, 255); row[0] = 0;
  const bomb = zlib.deflateSync(Buffer.concat(Array.from({ length: 2000 }, () => row)));
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { who: 'author', body: filteredPng(1280, 720, { idat: bomb }) })).status, 200, 'a bomb is stored like any image and measured unknown');
  const rec = await runModerationCase(SIM, env, deps());
  assert.equal(rec.status, 'approved', 'measured, not held');
  assert.deepEqual(rec.alpha_used, { 'capture-0.png': true, 'capture-1.png': false, [THUMB_LABEL]: 'unknown' });
  assert.doesNotMatch(JSON.stringify(authorView(rec, await record(env))), /alpha/);
});

// =====================================================================================================================================
// 2c-4: derived copies
// =====================================================================================================================================

const STRIPPED = storedImageVerdict(COLOURED_ART).bytes;
const isStripped = (bytes) => bytes.length === STRIPPED.length && bytes.every((v, i) => v === STRIPPED[i]);

/// An approved listing (reviewed by a run, so it has a case and a binding) whose thumbnail, cover and capture 0 carry a colour
/// transform, each read once by the public so its derived copy exists.
async function strippedListing() {
  const env = await environment();
  await listing(env, { captures: { 0: COLOURED_ART }, thumb: COLOURED_ART, cover: COLOURED_ART });
  assert.equal((await runModerationCase(SIM, env, deps())).status, 'approved');
  for (const path of ['thumbnail', 'cover', 'preview/0']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/${path}`);
    assert.equal(r.status, 200, path);
    assert.ok(isStripped(r.bytes), `${path}: the control serves the derived copy`);
  }
  assert.equal(keysUnder(env, 'derived/stripped/').length, 1, 'one copy for the one picture');
  return env;
}
/// The public body of the stored image at `key`, read once (src/moderation.mjs publicImageBody), as a route would.
const publicImageBodyOf = async (env, key) => { const shown = await publicImageBody(env, await env.SCENES.get(key)); if (shown?.body?.cancel) await shown.body.cancel(); return shown; };
const servedStrippedAnywhere = async (env, who) => {
  for (const path of ['thumbnail', 'cover', 'preview/0']) {
    const r = await call(env, 'GET', `/api/simulations/${SIM}/${path}`, { who });
    if (r.status === 200 && isStripped(r.bytes)) return path;
  }
  return null;
};

for (const [what, apply] of [
  ['the listing is removed (quarantined by a person)', async (env) => {
    const exec = makeToolExecutor(env, deps(), { actor: 'admin', actorId: ADMIN });
    const r = await exec('moderation_quarantine', { sim_id: SIM, category: HARD_CATEGORIES[0], rationale: 'A person found the listing carries removable content.' });
    assert.equal(r.ok, true, JSON.stringify(r));
  }],
  ['a person holds it', async (env) => {
    const exec = makeToolExecutor(env, deps(), { actor: 'admin', actorId: ADMIN });
    assert.equal((await exec('moderation_hold', { sim_id: SIM, reason: 'A person wants a second look.' })).ok, true);
  }],
  ['a legal lock is on it', async (env) => { await env.SCENES.put(`locks/legal/${SIM}`, JSON.stringify({ kind: 'serve_block' }), { customMetadata: { kind: 'serve_block' } }); }],
  ['a takedown holds it', async (env) => {
    const rec = await loadCase(env, SIM);
    rec.legal_hold = { category: 'ncii_or_real_person_sexual', source: 'takedown', takedowns: ['t1'], at: '2026-10-16T00:00:00.000Z', by: 'takedown', reported: false };
    await saveCase(env, rec, rec.status);
  }],
  ['its stored art no longer matches the binding', async (env) => {
    await store(env, `thumbnails/${SIM}/thumb.png`, pngOf(903, 7));
    await store(env, `covers/${SIM}/cover.png`, pngOf(904, 8));
    await store(env, `universes/${SIM}/moderation/capture-0.png`, pngOf(905, 9));
  }],
]) {
  test(`a derived copy is never served once ${what}`, async () => {
    const env = await strippedListing();
    await apply(env);
    for (const who of [undefined, 'stranger']) assert.equal(await servedStrippedAnywhere(env, who), null, `${what}, ${who || 'the public'}`);
  });
}

test('a derived copy is sealed with its original, as its own item, and no path deletes it', async () => {
  const env = await strippedListing();
  const items = await listingEvidenceItems(env, await record(env));
  const copy = keysUnder(env, 'derived/stripped/')[0];
  // The thumbnail, cover and capture are one picture, so one copy, sealed once as its own item.
  assert.deepEqual(items.filter(i => i.r2Key === copy).map(i => i.id), ['thumbnail-png-stripped']);
  // A capture that is another picture has its own copy, sealed under its own name.
  await store(env, `universes/${SIM}/moderation/capture-1.png`, realPng(600, 800, { type: 2, chunks: [pngChunk('gAMA', [0, 0, 0x27, 0x10]), pngChunk('pHYs', [0, 0, 0, 9, 0, 0, 0, 9, 1])] }));
  const second = await env.SCENES.head(`universes/${SIM}/moderation/capture-1.png`);
  await publicImageBodyOf(env, `universes/${SIM}/moderation/capture-1.png`);
  const again = await listingEvidenceItems(env, await record(env));
  assert.ok(second && again.some(i => i.id === 'capture-1.png-stripped' && i.r2Key.startsWith('derived/stripped/') && i.r2Key !== copy), JSON.stringify(again.map(i => i.id)));
  assert.ok(items.some(i => i.id === 'thumbnail-png' && i.r2Key === `thumbnails/${SIM}/thumb.png`), 'and the original is sealed as before');
  // Removing the cover and a removal leave every copy where it is.
  const copies = keysUnder(env, 'derived/stripped/');
  assert.equal(copies.length, 2);
  assert.equal((await call(env, 'DELETE', `/api/simulations/${SIM}/cover`, { who: 'author' })).status, 200);
  const exec = makeToolExecutor(env, deps(), { actor: 'admin', actorId: ADMIN });
  await exec('moderation_quarantine', { sim_id: SIM, category: HARD_CATEGORIES[0], rationale: 'A person found the listing carries removable content.' });
  assert.deepEqual(keysUnder(env, 'derived/stripped/'), copies);
  // No source line deletes a derived key.
  const dir = new URL('../src/', import.meta.url);
  for (const name of fs.readdirSync(dir).filter(n => /\.(mjs|js)$/.test(n))) {
    for (const line of fs.readFileSync(new URL(name, dir), 'utf8').split('\n')) assert.ok(!(line.includes('derived/') && /\.delete\(/.test(line)), `${name}: ${line.trim()}`);
  }
});

test('a derived copy is served only through the listing\'s own routes, as the image it is, never as a bare object', async () => {
  const env = await strippedListing();
  const copy = keysUnder(env, 'derived/stripped/')[0];
  assert.equal(env.SCENES.objects.get(copy).meta.httpMetadata.contentType, 'image/png', 'stored as the image it is');
  for (const path of ['thumbnail', 'cover', 'preview/0']) {
    const r = await worker.fetch(new Request(`https://api.eustress.dev/api/simulations/${SIM}/${path}`), env, ctx);
    assert.equal(r.headers.get('Content-Type'), 'image/png', path);
    assert.equal(r.headers.get('X-Content-Type-Options'), 'nosniff', path);
    assert.ok(isStripped(new Uint8Array(await r.arrayBuffer())), path);
  }
  for (const path of [`/${copy}`, `/api/${copy}`, `/api/scenes/${copy}`, `/api/simulations/${SIM}/${copy}`]) {
    const r = await call(env, 'GET', path);
    assert.ok(!(r.status === 200 && isStripped(r.bytes)), `${path} serves nothing`);
  }
});

test('a TAKE IT DOWN blocklist made from an original also blocks its stripped copy, with or without a derived copy written', async () => {
  for (const written of [true, false]) {
    const env = written ? await strippedListing() : await environment();
    if (!written) { await listing(env, { thumb: COLOURED_ART }); await approve(env); }
    const hashes = await listingHashes(env, await record(env));
    const strippedSha = sha256(STRIPPED);
    assert.ok(hashes.some(h => h.sha256 === sha256(COLOURED_ART) && h.kind === 'thumbnail'), 'the original');
    assert.ok(hashes.some(h => h.sha256 === strippedSha && h.kind === 'thumbnail_stripped'), `its stripped copy (copy written: ${written})`);
    // What a confirmed takedown writes for each hash (takedown.mjs blockHash).
    for (const h of hashes) await env.SOCIAL.put(blockKey(h.sha256), JSON.stringify({ first_at: '2026-10-16T00:00:00.000Z', kinds: [h.kind], cases: ['t1'] }));
    await listing(env, { id: SIM2, thumb: null, pak: 'OTHER-PAK' });
    for (const bytes of [STRIPPED, COLOURED_ART]) {
      const r = await call(env, 'PUT', `/api/simulations/${SIM2}/thumbnail`, { who: 'author', body: bytes });
      assert.deepEqual([r.status, r.body?.code], [422, 'content_blocked'], `copy written: ${written}`);
    }
    assert.equal((await call(env, 'PUT', `/api/simulations/${SIM2}/thumbnail`, { who: 'author', body: pngOf(906, 11) })).status, 200, 'the control: another picture');
  }
});

test('a verdict record that cannot be read fails closed: the public is answered as if the image were absent', async () => {
  const env = await environment();
  await listing(env, { thumb: CLEAN_ART, cover: CLEAN_ART });
  await approve(env);
  for (const key of [`thumbnails/${SIM}/thumb.png`, `covers/${SIM}/cover.png`]) {
    await env.SCENES.put(imageVerdictKey(identOfObject(await env.SCENES.head(key))), '{not json');
  }
  assert.deepEqual((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).body, { error: 'Thumbnail not found' });
  assert.deepEqual((await call(env, 'GET', `/api/simulations/${SIM}/cover`)).body, { error: 'No cover' });
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who: 'author' })).status, 200, 'its author still sees it');
});

// =====================================================================================================================================
// 2c6
// =====================================================================================================================================

// -- 1: the public art is sent first, so the budget can never cut it ------------------------------------------------------------

test('the thumbnail and cover are sent before the captures: heavy captures overflow, the art is judged, and a harmful cover quarantines', async () => {
  const env = await environment();
  // Eight heavy captures: five of them alone would fill the 24 MB budget to within a few kilobytes, leaving no room for a
  // cover sent after them.
  const captures = Object.fromEntries([0, 1, 2, 3, 4, 5, 6, 7].map(n => [n, pngOf(3_599_000, n)]));
  await listing(env, { captures, thumb: pngOf(300), cover: pngOf(20_000, 12) });
  const harmful = { ...verdict(), verdict: 'reject', articles: [{ id: 'A1', confidence: 0.95, evidence: 'the listing cover shows the prohibited material' }] };
  const judge = fakeClaude((body) => (/listing cover/.test(body.messages[0].content.at(-1).text.split('\n')[0]) ? harmful : verdict()));
  const d = { ...deps(), judge, claude: judge };
  const rec = await runModerationCase(SIM, env, d);
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL, COVER_LABEL, 'capture-0.png', 'capture-1.png', 'capture-2.png', 'capture-3.png']);
  assert.deepEqual(rec.judge.captures_left_out, ['capture-4.png', 'capture-5.png', 'capture-6.png', 'capture-7.png'], 'the overflow is named, all of it captures');
  assert.match(judge.calls[0].body.messages[0].content.at(-1).text, /capture-4\.png \(past the request's budget for images\)/);
  assert.equal(rec.status, 'quarantined', 'the cover was judged, and its citation quarantined the listing');
  assert.notEqual((await record(env)).moderation.status, 'approved');
});

test('the identity checks and the dedup image match hold in the new order', async () => {
  // A swapped cover is left out (identity), with the art first.
  const env = await environment();
  await listing(env, { cover: pngOf(350) });
  const get = env.SCENES.get.bind(env.SCENES);
  let once = false;
  env.SCENES.get = async (key, ...rest) => {
    if (!once && key === `covers/${SIM}/cover.png`) { once = true; const original = env.SCENES.objects.get(key); await store(env, key, pngOf(777, 9)); const seen = await get(key, ...rest); env.SCENES.objects.set(key, original); return seen; }
    return get(key, ...rest);
  };
  const rec = await runModerationCase(SIM, env, deps());
  assert.deepEqual(rec.judge.captures_left_out, [COVER_LABEL]);
  assert.deepEqual(rec.judge.captures_used, [THUMB_LABEL, 'capture-0.png', 'capture-1.png']);
  // The same images still copy an approval; other images still do not.
  const env2 = await environment();
  await listing(env2);
  const d2 = deps();
  assert.equal((await runModerationCase(SIM, env2, d2)).status, 'approved');
  await listing(env2, { id: SIM2 });
  assert.equal((await runModerationCase(SIM2, env2, d2)).decision.dedup_of, SIM);
  assert.equal(d2.judge.calls.length, 1);
});

// -- 3: the alpha measure's time budget, on an injected clock ---------------------------------------------------------------------

test('a tiny injected budget records unknown, on an injected clock; production keeps 250 ms', async () => {
  let t = 0;
  const r = await measurePngAlpha(filteredPng(64, 64, { idatSplit: 8 }), { budgetMs: 0, now: () => (t += 1) });
  assert.deepEqual([r.alpha_used, r.reason], ['unknown', 'time_budget']);
  assert.equal(ALPHA_MEASURE_BUDGET_MS, 250);
});

// -- the alpha measure, reported --------------------------------------------------------------------------------------------------

test("the staff report replays the judge's order: the thumbnail, the cover, then the captures by key", () => {
  const img = (kind, key) => ({ kind, key, label: key });
  const order = inJudgeOrder([img('capture', 'u/e1/capture-1.png'), img('cover', 'covers/x/cover.png'), img('capture', 'u/e1/capture-0.png'), img('thumbnail', 'thumbnails/x/thumb.png')]);
  assert.deepEqual(order.map(i => i.key), ['thumbnails/x/thumb.png', 'covers/x/cover.png', 'u/e1/capture-0.png', 'u/e1/capture-1.png']);
});

test('the staff report counts the alpha measure of RGBA captures and thumbnails, with the unknown rate', () => {
  const img = (kind, alpha_used, rgba = true) => ({ kind, key: kind, label: kind, rgba, alpha_used });
  const entries = [
    { id: 'a', images: [img('thumbnail', false), img('capture', false), img('capture', true), img('capture', 'unknown')] },
    { id: 'b', images: [img('capture', 'unknown'), img('capture', undefined), img('cover', 'unknown'), img('capture', 'unknown', false)] },
  ];
  assert.deepEqual(countAlpha(entries), { true: 1, false: 2, unknown: 2, unmeasured: 1, unknown_rate: 0.4 });
  assert.deepEqual(countAlpha([]), { true: 0, false: 0, unknown: 0, unmeasured: 0, unknown_rate: null }, 'nothing measured: no rate');
  const approved = { name: 'A', moderation: { status: 'approved' } };
  const report = summarize([
    { id: 'a', sim: approved, binding: { v: 1 }, images: [img('capture', 'unknown'), img('thumbnail', false)] },
    { id: 'c', sim: approved, binding: { revoked: true }, images: [img('capture', 'unknown')] },
  ]);
  assert.deepEqual(report.alpha, { true: 0, false: 1, unknown: 1, unmeasured: 0, unknown_rate: 0.5 }, 'only listings the report checks');
});
